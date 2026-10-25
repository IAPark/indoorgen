//! Port of Blender 4.2's exact mesh boolean (`mesh_boolean_mesh_arr` of geometry_mesh_boolean.cc with identity
//! transforms): `meshes_to_imesh`, `boolean_mesh` (mesh_boolean.cc / mesh_intersect.cc) and `imesh_to_mesh`.

use crate::exbool::boolean::{boolean_mesh, BoolOpType};
use crate::exbool::imesh::*;
use crate::exbool::mpq::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct BoolInput {
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u32>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoolOp {
    Intersect,
    Union,
    Difference,
}

pub struct BoolOutput {
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u32>>,
    /// output edges as imesh_to_mesh creates them (vertex pairs, in Blender's order)
    pub edges: Vec<[u32; 2]>,
    /// for every output face: (operand index, face index in that operand)
    pub face_orig: Vec<(usize, usize)>,
    /// for every output vertex: Some((operand, vertex)) when it is an unchanged input vertex
    pub vert_orig: Vec<Option<(usize, usize)>>,
}

/// Number of edge hash maps `bke::mesh_calc_edges` uses for meshes with >= 1000 faces
/// (`power_of_2_min(min(8, BLI_system_thread_count()))`).  0 = derive from this machine's thread count.
/// Only affects the order of the output edges of large meshes (the Blender reference run's thread count).
pub static EDGE_PARALLEL_MAPS: AtomicUsize = AtomicUsize::new(0);

fn parallel_maps_count(faces_num: usize) -> usize {
    if faces_num < 1000 {
        return 1;
    }
    let forced = EDGE_PARALLEL_MAPS.load(Ordering::Relaxed);
    let threads = if forced > 0 { forced } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) };
    let m = threads.min(8).max(1);
    // power_of_2_min
    let mut p = 1;
    while p * 2 <= m {
        p *= 2;
    }
    p
}

/// `bke::mesh_calc_edges(mesh, false, false)` edge order.
fn calc_edges(faces: &[Vec<u32>]) -> Vec<[u32; 2]> {
    let maps = parallel_maps_count(faces.len());
    let mask = (maps - 1) as u32;
    let mut per_map: Vec<(std::collections::HashSet<[u32; 2]>, Vec<[u32; 2]>)> = (0..maps).map(|_| (Default::default(), vec![])).collect();
    for face in faces {
        let n = face.len();
        for c in 0..n {
            let vert = face[c];
            let vert_prev = face[(c + n - 1) % n];
            if vert_prev != vert {
                let e = if vert_prev < vert { [vert_prev, vert] } else { [vert, vert_prev] };
                let task = (mask & e[0]) as usize;
                if per_map[task].0.insert(e) {
                    per_map[task].1.push(e);
                }
            }
        }
    }
    let mut out = vec![];
    for (_, v) in per_map {
        out.extend(v);
    }
    out
}

pub fn exact_boolean(meshes: &[BoolInput], op: BoolOp, use_self: bool, hole_tolerant: bool) -> BoolOutput {
    let mut arena = Arena::new();
    let nmeshes = meshes.len();
    let mut face_offset = vec![0usize; nmeshes];
    let mut imesh: Vec<usize> = vec![];
    let mut vert_src: Vec<Option<(usize, usize)>> = vec![];
    let mut f = 0usize;
    for (mi, mesh) in meshes.iter().enumerate() {
        face_offset[mi] = f;
        let mut mesh_to_imesh_vert: Vec<u32> = Vec::with_capacity(mesh.verts.len());
        for (i, co) in mesh.verts.iter().enumerate() {
            let mco = [q_from_f64(co[0] as f64), q_from_f64(co[1] as f64), q_from_f64(co[2] as f64)];
            let dco = q3_get_d(&mco);
            let before = arena.verts.len();
            let id = arena.add_or_find_vert_with_d(mco, dco, i as i32);
            if arena.verts.len() > before {
                vert_src.push(Some((mi, i)));
            }
            mesh_to_imesh_vert.push(id);
        }
        for face in &mesh.faces {
            let flen = face.len();
            let fv: Vec<u32> = face.iter().map(|&c| mesh_to_imesh_vert[c as usize]).collect();
            // original edge ids only matter through "!= NO_INDEX"; use a unique non-negative number per corner.
            let eo: Vec<i32> = (0..flen).map(|i| (f * 1000 + i) as i32).collect();
            let fi = arena.add_face(fv, f as i32, eo, vec![false; flen]);
            imesh.push(fi);
            f += 1;
        }
    }
    let offsets = face_offset.clone();
    let shape_fn = move |fidx: i32| -> i32 {
        for mi in 0..offsets.len().saturating_sub(1) {
            if (fidx as usize) < offsets[mi + 1] {
                return mi as i32;
            }
        }
        offsets.len() as i32 - 1
    };
    let bop = match op {
        BoolOp::Intersect => BoolOpType::Intersect,
        BoolOp::Union => BoolOpType::Union,
        BoolOp::Difference => BoolOpType::Difference,
    };
    let out_faces = boolean_mesh(&mut arena, &imesh, bop, nmeshes, &shape_fn, use_self, hole_tolerant);

    // imesh_to_mesh
    let (order, idx) = populate_vert(&arena, &out_faces);
    let mut verts = vec![];
    let mut vert_orig = vec![];
    for &v in &order {
        let vv = &arena.verts[v as usize];
        verts.push([vv.d[0] as f32, vv.d[1] as f32, vv.d[2] as f32]);
        vert_orig.push(if vv.orig != NO_INDEX { vert_src[v as usize] } else { None });
    }
    let mut faces = vec![];
    let mut face_orig = vec![];
    for &fi in &out_faces {
        let face = &arena.faces[fi];
        faces.push(face.vert.iter().map(|v| idx[v] as u32).collect::<Vec<u32>>());
        let o = face.orig as usize;
        let mut mi = nmeshes - 1;
        for k in 0..nmeshes.saturating_sub(1) {
            if o < face_offset[k + 1] {
                mi = k;
                break;
            }
        }
        face_orig.push((mi, o - face_offset[mi]));
    }
    if std::env::var("BOOL_DEBUG").is_ok() {
        for (i, &fi) in out_faces.iter().enumerate() {
            let face = &arena.faces[fi];
            eprintln!("outface {} arena {} orig {} verts {:?} eo {:?} isect {:?}", i, fi, face.orig, faces[i], face.edge_orig, face.is_intersect);
        }
    }
    let edges = calc_edges(&faces);
    BoolOutput { verts, faces, edges, face_orig, vert_orig }
}
