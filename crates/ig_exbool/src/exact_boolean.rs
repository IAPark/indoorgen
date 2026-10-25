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

pub use crate::boolean::{set_patch_vote, set_slow_paths};

pub fn exact_boolean(meshes: &[BoolInput], op: BoolOp, use_self: bool, hole_tolerant: bool) -> BoolOutput {
    let tsetup = std::time::Instant::now();
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
    if std::env::var_os("EXBOOL_PROF").is_some() {
        eprintln!("exbool: setup {:?} ({} verts {} faces)", tsetup.elapsed(), arena.verts.len(), arena.faces.len());
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
    let tprof = std::time::Instant::now();
    let out_faces = boolean_mesh(&mut arena, &imesh, bop, nmeshes, &shape_fn, use_self, hole_tolerant);
    let t_after = tprof.elapsed();

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
    if std::env::var_os("EXBOOL_PROF").is_some() {
        eprintln!("exbool: boolean_mesh {:?}, tail (to mesh + edges) {:?}", t_after, tprof.elapsed() - t_after);
    }
    BoolOutput { verts, faces, edges, face_orig, vert_orig }
}

#[cfg(test)]
mod fastpath_tests {
    use super::*;

    fn cube(c: [f32; 3], h: [f32; 3], open: bool) -> BoolInput {
        let mut verts = vec![];
        for x in [-1.0f32, 1.0] {
            for y in [-1.0f32, 1.0] {
                for z in [-1.0f32, 1.0] {
                    verts.push([c[0] + x * h[0], c[1] + y * h[1], c[2] + z * h[2]]);
                }
            }
        }
        let mut faces = vec![vec![0, 1, 3, 2], vec![2, 3, 7, 6], vec![6, 7, 5, 4], vec![4, 5, 1, 0], vec![2, 6, 4, 0], vec![7, 3, 1, 5]];
        if open {
            faces.pop();
        }
        BoolInput { verts, faces }
    }

    fn join(parts: Vec<BoolInput>) -> BoolInput {
        let mut o = BoolInput { verts: vec![], faces: vec![] };
        for p in parts {
            let base = o.verts.len() as u32;
            o.verts.extend(p.verts);
            o.faces.extend(p.faces.into_iter().map(|f| f.into_iter().map(|v| v + base).collect::<Vec<_>>()));
        }
        o
    }

    fn run(a: &BoolInput, b: &BoolInput, op: BoolOp, hole_tolerant: bool, slow: bool) -> BoolOutput {
        set_slow_paths(slow);
        let r = exact_boolean(&[BoolInput { verts: a.verts.clone(), faces: a.faces.clone() }, BoolInput { verts: b.verts.clone(), faces: b.faces.clone() }], op, false, hole_tolerant);
        set_slow_paths(false);
        r
    }

    fn same(x: &BoolOutput, y: &BoolOutput) {
        assert_eq!(x.verts, y.verts);
        assert_eq!(x.faces, y.faces);
        assert_eq!(x.face_orig, y.face_orig);
    }

    /// the BVH ray scan and the pruned component-container search give exactly the output of the brute-force paths
    #[test]
    fn pruned_paths_match_brute_force() {
        // many disjoint components (cubes on a grid) + a box swallowing some of them + a nested one
        let mut parts = vec![];
        for i in 0..5 {
            for j in 0..5 {
                parts.push(cube([i as f32 * 0.5, j as f32 * 0.5, 0.0], [0.1, 0.1, 0.1], false));
            }
        }
        parts.push(cube([1.0, 1.0, 0.0], [0.02, 0.02, 0.02], false)); // inside nothing of A's solids
        let a = join(parts);
        let b = join(vec![cube([0.5, 0.5, 0.0], [0.45, 0.45, 0.3], false), cube([1.6, 1.6, 0.0], [0.2, 0.2, 0.05], false)]);
        for op in [BoolOp::Union, BoolOp::Difference, BoolOp::Intersect] {
            same(&run(&a, &b, op, false, false), &run(&a, &b, op, false, true));
        }
        // non-closed operand: raycast classification (hole tolerant and per patch)
        let open = join(vec![cube([0.5, 0.5, 0.0], [0.3, 0.3, 0.3], true), cube([2.0, 0.0, 0.0], [0.1, 0.1, 0.1], false)]);
        for op in [BoolOp::Union, BoolOp::Difference] {
            for ht in [false, true] {
                same(&run(&open, &b, op, ht, false), &run(&open, &b, op, ht, true));
                same(&run(&a, &open, op, ht, false), &run(&a, &open, op, ht, true));
            }
        }
    }
}
