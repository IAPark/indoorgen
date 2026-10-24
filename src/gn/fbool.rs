//! Mesh Boolean node, solver FLOAT: port of `geometry/intern/mesh_boolean.cc::mesh_boolean_float` on top of the BMesh
//! intersect port in `crate::bm::isect` (`BM_mesh_intersect`).  Output vertex / face order follows Blender; face attributes are
//! carried from the source faces, vertex attributes are dropped (like the EXACT wiring in csg.rs).
use super::geom::*;
use crate::bm::edgenet::{axis_dominant_v3_to_m3, mul_v2_m3v3};
use crate::bm::isect::{mesh_intersect_boolean, BOOLEAN_DIFFERENCE, BOOLEAN_ISECT, BOOLEAN_UNION};
use crate::bm::query::normalize;
use crate::bm::{BMesh, ELEM_DRAW, NONE};
use crate::exbool::polyfill::{polyfill_beautify, polyfill_calc, quad_rotate_calc};

type V3 = [f32; 3];

/// BM_mesh_calc_tessellation_beauty (loops of every triangle, in pool order of faces)
fn tessellation_beauty(bm: &BMesh) -> Vec<[usize; 3]> {
    let mut out = vec![];
    for f in bm.face_ids() {
        let ls = bm.loops_of_face(f);
        let co = |l: usize| bm.verts[bm.loops[l].v].co;
        match ls.len() {
            3 => out.push([ls[0], ls[1], ls[2]]),
            4 => {
                let m = axis_dominant_v3_to_m3(bm.faces[f].no, false);
                // l_v1 = first, l_v2 = next, l_v3 = next->next, l_v4 = prev
                let (l1, l2, l3, l4) = (ls[0], ls[1], ls[2], ls[3]);
                let q = [mul_v2_m3v3(&m, co(l1)), mul_v2_m3v3(&m, co(l2)), mul_v2_m3v3(&m, co(l3)), mul_v2_m3v3(&m, co(l4))];
                let split_13 = quad_rotate_calc(q[0], q[1], q[2], q[3], false).0 < 0.0;
                if split_13 {
                    out.push([l1, l2, l3]);
                    out.push([l1, l3, l4]);
                } else {
                    out.push([l1, l2, l4]);
                    out.push([l2, l3, l4]);
                }
            }
            n => {
                let m = axis_dominant_v3_to_m3(bm.faces[f].no, true);
                let proj: Vec<[f32; 2]> = ls.iter().map(|&l| mul_v2_m3v3(&m, co(l))).collect();
                let mut tris = polyfill_calc(&proj);
                polyfill_beautify(&proj, &mut tris);
                debug_assert_eq!(tris.len(), n - 2);
                for t in tris {
                    out.push([ls[t[0] as usize], ls[t[1] as usize], ls[t[2] as usize]]);
                }
            }
        }
    }
    out
}

/// mesh_bm_concat for two meshes (identity transforms): operand 0 faces are tagged (BM_FACE_TAG), `user` = local face id
fn bm_concat(m0: &Mesh, m1: &Mesh) -> (BMesh, Vec<[usize; 3]>) {
    let mut bm = BMesh::new();
    let mut fid = 0usize;
    for (mi, m) in [m0, m1].iter().enumerate() {
        let vbase = bm.vert_ids().len();
        let ebase = bm.edge_ids().len();
        for p in &m.pos {
            bm.vert_create(*p, None);
        }
        for e in &m.edges {
            bm.edge_create(vbase + e[0] as usize, vbase + e[1] as usize, None, false);
        }
        let mat = m.attrs.get("material_index");
        for fi in 0..m.nf() {
            let r = m.face_range(fi);
            let vs: Vec<usize> = m.cv[r.clone()].iter().map(|&v| vbase + v as usize).collect();
            let es: Vec<usize> = m.ce[r].iter().map(|&e| ebase + e as usize).collect();
            let f = bm.face_create(&vs, &es, None, false);
            // BM_face_normal_update
            let no = bm.face_calc_normal(f);
            bm.faces[f].no = no;
            bm.faces[f].user = fid;
            fid += 1;
            if let Some(a) = mat {
                bm.faces[f].mat_nr = a.data[fi].i() as i16;
            }
            if mi == 0 {
                bm.faces[f].flag |= ELEM_DRAW;
            }
        }
    }
    let looptris = tessellation_beauty(&bm);
    // transform vertices (identity matrix: x*1 + y*0 + z*0 + 0) and faces normals (transform_direction, normalize_v3)
    for v in bm.vert_ids() {
        let c = bm.verts[v].co;
        bm.verts[v].co = [c[0] * 1.0 + c[1] * 0.0 + c[2] * 0.0 + 0.0, c[0] * 0.0 + c[1] * 1.0 + c[2] * 0.0 + 0.0, c[0] * 0.0 + c[1] * 0.0 + c[2] * 1.0 + 0.0];
    }
    for f in bm.face_ids() {
        let n = bm.faces[f].no;
        let n = [n[0] * 1.0 + n[1] * 0.0 + n[2] * 0.0, n[0] * 0.0 + n[1] * 1.0 + n[2] * 0.0, n[0] * 0.0 + n[1] * 0.0 + n[2] * 1.0];
        bm.faces[f].no = normalize(n);
    }
    (bm, looptris)
}

fn operand_of(bm: &BMesh, f: usize) -> i32 {
    if bm.faces[f].flag & ELEM_DRAW != 0 {
        0
    } else {
        1
    }
}

/// One BM_mesh_intersect pass over two meshes; returns the result mesh and, per output face, the (operand, face) source
/// according to `src0` / `src1` (sources of the faces of m0 and m1).
fn pass(m0: &Mesh, src0: &[(u8, u32)], m1: &Mesh, src1: &[(u8, u32)], mode: i32) -> (Mesh, Vec<(u8, u32)>) {
    let (mut bm, looptris) = bm_concat(m0, m1);
    mesh_intersect_boolean(&mut bm, &looptris, &|b: &BMesh, f: usize| operand_of(b, f), mode, 1e-6f32);
    let (pos, edges, faces) = bm.to_mesh();
    let mut srcs = vec![];
    for f in bm.face_ids() {
        let u = bm.faces[f].user;
        srcs.push(if u < src0.len() { src0[u] } else { src1[u - src0.len()] });
    }
    let _ = NONE;
    let m = Mesh::from_faces(pos, &faces, &edges);
    (m, srcs)
}

/// geometry::boolean::mesh_boolean_float
pub fn mesh_boolean_float(op: &str, first: &Mesh, others: &[&Mesh]) -> Mesh {
    let mode = match op {
        "INTERSECT" => BOOLEAN_ISECT,
        "UNION" => BOOLEAN_UNION,
        _ => BOOLEAN_DIFFERENCE,
    };
    let mut meshes: Vec<&Mesh> = vec![first];
    meshes.extend(others.iter().copied());
    if meshes.len() == 1 {
        return first.clone();
    }
    let ident = |k: u8, m: &Mesh| -> Vec<(u8, u32)> { (0..m.nf()).map(|f| (k, f as u32)).collect() };
    let mut cur_mesh: Mesh = Mesh::new();
    let mut cur_src: Vec<(u8, u32)> = vec![];
    for i in 0..meshes.len() - 1 {
        let (a, sa): (&Mesh, Vec<(u8, u32)>) = if i == 0 { (meshes[0], ident(0, meshes[0])) } else { (&cur_mesh, cur_src.clone()) };
        let b = meshes[i + 1];
        let sb = ident((i + 1) as u8, b);
        let (m, s) = pass(a, &sa, b, &sb, mode);
        cur_mesh = m;
        cur_src = s;
    }
    let mut m = cur_mesh;
    if std::env::var_os("GNBOOL_DUMP").is_some() {
        eprintln!("RUST nv {} ne {} nf {}", m.nv(), m.ne(), m.nf());
        eprintln!("V {:?}", m.pos);
        eprintln!("F {:?}", (0..m.nf()).map(|f| m.face(f).to_vec()).collect::<Vec<_>>());
        eprintln!("E {:?}", m.edges);
    }
    super::csg::propagate_face_attrs_pub(&mut m, first, others, &cur_src);
    m.mats = first.mats.clone();
    m
}
