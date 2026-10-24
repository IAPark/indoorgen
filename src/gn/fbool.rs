//! Mesh Boolean node, solver FLOAT: port of `geometry/intern/mesh_boolean.cc::mesh_boolean_float` on top of the BMesh
//! intersect port in `crate::bm::isect` (`BM_mesh_intersect`).  Output vertex / face order follows Blender; face attributes are
//! carried from the source faces, vertex attributes are dropped (like the EXACT wiring in csg.rs).
use super::geom::*;
use super::val::*;

use crate::bm::edgenet::{axis_dominant_v3_to_m3, mul_v2_m3v3};
use crate::bm::isect::{mesh_intersect_boolean, BOOLEAN_DIFFERENCE, BOOLEAN_ISECT, BOOLEAN_UNION};
use crate::bm::query::normalize;
use crate::bm::cdata::{LKind, LoopData};
use crate::bm::{BMesh, ELEM_DRAW, ELEM_SMOOTH, NONE};
use crate::exbool::polyfill::{polyfill_beautify, polyfill_calc, quad_rotate_calc};

type V3 = [f32; 3];

/// math::transform_point (float4x4, Blender column layout m[col][row])
fn transform_point(m: &M4, c: V3) -> V3 {
    let mut r = [0f32; 3];
    for row in 0..3 {
        let mut v = 0.0f32;
        v += m[0][row] * c[0];
        v += m[1][row] * c[1];
        v += m[2][row] * c[2];
        r[row] = v + m[3][row];
    }
    r
}
fn transform_direction(m: &M4, c: V3) -> V3 {
    let mut r = [0f32; 3];
    for row in 0..3 {
        let mut v = 0.0f32;
        v += m[0][row] * c[0];
        v += m[1][row] * c[1];
        v += m[2][row] * c[2];
        r[row] = v;
    }
    r
}
/// math::is_negative(float4x4) (is_negative_m4)
fn is_negative(m: &M4) -> bool {
    let c = [m[0][1] * m[1][2] - m[0][2] * m[1][1], m[0][2] * m[1][0] - m[0][0] * m[1][2], m[0][0] * m[1][1] - m[0][1] * m[1][0]];
    c[0] * m[2][0] + c[1] * m[2][1] + c[2] * m[2][2] < 0.0
}

/// A mesh with `xf` applied to its positions (faces reversed for negative transforms)
pub fn transform_mesh(m: &Mesh, xf: &M4) -> Mesh {
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        *p = transform_point(xf, *p);
    }
    o.vnor = None;
    if is_negative(xf) {
        for f in 0..o.nf() {
            let r = o.face_range(f);
            o.cv[r].reverse();
        }
        o.rebuild_corner_edges();
    }
    o
}

fn dbg_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GNBOOL_DUMP").is_some())
}

fn lkind(t: Ty) -> LKind {
    match t {
        Ty::F => LKind::Float,
        Ty::I => LKind::Int,
        Ty::B => LKind::Bool,
        Ty::V => LKind::Vec3,
        _ => LKind::Color,
    }
}
fn r_start(m: &Mesh, f: usize) -> usize {
    m.foff[f] as usize
}
fn push_val(out: &mut Vec<f64>, v: &Val) {
    match v {
        Val::F(x) => out.push(*x as f64),
        Val::I(x) => out.push(*x as f64),
        Val::B(x) => out.push(if *x { 1.0 } else { 0.0 }),
        Val::V(x) => out.extend(x.iter().map(|c| *c as f64)),
        Val::C(x) => out.extend(x.iter().map(|c| *c as f64)),
        _ => out.push(0.0),
    }
}
fn val_from(ty: Ty, d: &[f64]) -> Val {
    match ty {
        Ty::F => Val::F(d[0] as f32),
        Ty::I => Val::I(d[0] as i32),
        Ty::B => Val::B(d[0] != 0.0),
        Ty::V => Val::V([d[0] as f32, d[1] as f32, d[2] as f32]),
        _ => Val::C([d[0] as f32, d[1] as f32, d[2] as f32, d[3] as f32]),
    }
}

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
                let cost = quad_rotate_calc(q[0], q[1], q[2], q[3], false).0;
                let mut split_13 = cost < 0.0;
                if dbg_on() {
                    eprintln!("QUAD face slot {} cost {:e} split_13 {}", f, cost, split_13);
                }
                if let Ok(l) = std::env::var("GNBOOL_QUAD_FLIP") {
                    if l.split(',').any(|x| x.parse::<usize>().ok() == Some(f)) {
                        split_13 = !split_13;
                    }
                }
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
fn bm_concat(m0: &Mesh, x0: &M4, m1: &Mesh, x1: &M4) -> (BMesh, Vec<[usize; 3]>) {
    let mut bm = BMesh::new();
    // corner (loop) layers: union of both operands, in order of first appearance
    let mut layers: Vec<(String, Ty)> = vec![];
    for m in [m0, m1] {
        for (name, a) in m.attrs.iter() {
            if a.dom != Dom::Corner || !matches!(a.ty, Ty::F | Ty::I | Ty::B | Ty::V | Ty::C) {
                continue;
            }
            match layers.iter_mut().find(|l| &l.0 == name) {
                Some(l) => l.1 = a.ty,
                None => layers.push((name.clone(), a.ty)),
            }
        }
    }
    if !layers.is_empty() {
        let kinds: Vec<LKind> = layers.iter().map(|l| lkind(l.1)).collect();
        bm.ld = LoopData::new(&kinds);
    }
    let mut fid = 0usize;
    let mut vid = 0usize;
    let mut eid = 0usize;
    let neg = [is_negative(x0), is_negative(x1)];
    let flip1 = neg[1] != neg[0];
    for (mi, m) in [m0, m1].iter().enumerate() {
        let vbase = bm.vert_ids().len();
        let ebase = bm.edge_ids().len();
        for p in &m.pos {
            let v = bm.vert_create(*p, None);
            vid += 1;
            bm.verts[v].user = vid;
        }
        let sharp = m.attrs.get("sharp_edge");
        for (ei, e) in m.edges.iter().enumerate() {
            let be = bm.edge_create(vbase + e[0] as usize, vbase + e[1] as usize, None, false);
            eid += 1;
            bm.edges[be].user = eid;
            // e->head.hflag = 0; SMOOTH unless sharp_edge
            bm.edges[be].flag = 0;
            if !sharp.map_or(false, |a| a.data[ei].b()) {
                bm.edges[be].flag |= ELEM_SMOOTH;
            }
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
            if !layers.is_empty() {
                let ls = bm.loops_of_face(f);
                for (k, &l) in ls.iter().enumerate() {
                    let mut vals: Vec<f64> = vec![];
                    for (name, ty) in &layers {
                        match m.attrs.get(name).filter(|a| a.dom == Dom::Corner) {
                            Some(a) => push_val(&mut vals, &a.data[r_start(m, fi) + k].convert(*ty)),
                            None => push_val(&mut vals, &Val::zero(*ty)),
                        }
                    }
                    bm.ld.set_block(l, &vals);
                }
            }
            if mi == 1 && flip1 {
                bm.face_normal_flip(f);
            }
        }
    }
    let looptris = tessellation_beauty(&bm);
    // transform vertices / face normals into the target space (verts_end / faces_end split the operands)
    let nv0 = m0.nv();
    let nf0 = m0.nf();
    for (i, v) in bm.vert_ids().into_iter().enumerate() {
        let x = if i < nv0 { x0 } else { x1 };
        bm.verts[v].co = transform_point(x, bm.verts[v].co);
    }
    for (i, f) in bm.face_ids().into_iter().enumerate() {
        let x = if i < nf0 { x0 } else { x1 };
        let mut n = transform_direction(x, bm.faces[f].no);
        if if i < nf0 { neg[0] } else { neg[1] } {
            n = [-n[0], -n[1], -n[2]];
        }
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

/// sources of the faces / vertices of a mesh: (operand, element)
#[derive(Clone, Default)]
struct Src {
    f: Vec<(u8, u32)>,
    v: Vec<(u8, u32)>,
    e: Vec<(u8, u32)>,
}

/// One BM_mesh_intersect pass over two meshes; returns the result mesh and the sources of its faces and vertices
/// (vertices created by the intersection have operand `u8::MAX`).
fn pass(m0: &Mesh, x0: &M4, s0: &Src, m1: &Mesh, x1: &M4, s1: &Src, mode: i32) -> (Mesh, Src) {
    let (mut bm, looptris) = bm_concat(m0, x0, m1, x1);
    mesh_intersect_boolean(&mut bm, &looptris, &|b: &BMesh, f: usize| operand_of(b, f), mode, 1e-6f32);
    let (pos, edges, faces) = bm.to_mesh();
    if dbg_on() {
        eprintln!("SLOTS {:?}", bm.face_ids());
    }
    let mut src = Src::default();
    for f in bm.face_ids() {
        let u = bm.faces[f].user;
        src.f.push(if u < s0.f.len() { s0.f[u] } else { s1.f[u - s0.f.len()] });
    }
    for v in bm.vert_ids() {
        let u = bm.verts[v].user;
        src.v.push(if u == 0 { (u8::MAX, 0) } else if u - 1 < s0.v.len() { s0.v[u - 1] } else { s1.v[u - 1 - s0.v.len()] });
    }
    for e in bm.edge_ids() {
        let u = bm.edges[e].user;
        src.e.push(if u == 0 { (u8::MAX, 0) } else if u - 1 < s0.e.len() { s0.e[u - 1] } else { s1.e[u - 1 - s0.e.len()] });
    }
    let _ = NONE;
    let mut m = Mesh::from_faces(pos, &faces, &edges);
    // corner layers back to attributes (same layer order as in bm_concat)
    if !bm.ld.is_empty() {
        let mut names: Vec<(String, Ty)> = vec![];
        for mm in [m0, m1] {
            for (name, a) in mm.attrs.iter() {
                if a.dom != Dom::Corner || !matches!(a.ty, Ty::F | Ty::I | Ty::B | Ty::V | Ty::C) {
                    continue;
                }
                match names.iter_mut().find(|l| &l.0 == name) {
                    Some(l) => l.1 = a.ty,
                    None => names.push((name.clone(), a.ty)),
                }
            }
        }
        let mut cols: Vec<Vec<Val>> = names.iter().map(|_| vec![]).collect();
        for f in bm.face_ids() {
            for l in bm.loops_of_face(f) {
                let blk = bm.ld.block(l);
                for (i, (_, ty)) in names.iter().enumerate() {
                    let (_, off) = bm.ld.layers[i];
                    cols[i].push(val_from(*ty, &blk[off..]));
                }
            }
        }
        for ((name, ty), data) in names.into_iter().zip(cols) {
            m.attrs.insert(name, Attr { dom: Dom::Corner, ty, data });
        }
    }
    // sharp_edge from the BM_ELEM_SMOOTH flag (written only when some edge is not smooth)
    let sharp: Vec<bool> = bm.edge_ids().iter().map(|&e| bm.edges[e].flag & ELEM_SMOOTH == 0).collect();
    if sharp.iter().any(|&x| x) {
        m.attrs.insert("sharp_edge".into(), Attr { dom: Dom::Edge, ty: Ty::B, data: sharp.iter().map(|&b| Val::B(b)).collect() });
    }
    (m, src)
}

/// geometry::boolean::mesh_boolean_float: `operands[0]` is the first mesh (Mesh 1 for DIFFERENCE), each with its transform
pub fn mesh_boolean_float(op: &str, operands: &[(&Mesh, M4)]) -> Mesh {
    let mode = match op {
        "INTERSECT" => BOOLEAN_ISECT,
        "UNION" => BOOLEAN_UNION,
        _ => BOOLEAN_DIFFERENCE,
    };
    let first = operands[0].0;
    let t0 = std::time::Instant::now();
    if operands.len() == 1 {
        return first.clone();
    }
    let ident: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    if dbg_on() {
        for (i, (m, _)) in operands.iter().enumerate() {
            eprintln!("IN{} V {:?}", i, m.pos);
            eprintln!("IN{} F {:?}", i, (0..m.nf()).map(|f| m.face(f).to_vec()).collect::<Vec<_>>());
        }
    }
    let src_of = |k: usize, m: &Mesh| -> Src { Src { f: (0..m.nf()).map(|f| (k as u8, f as u32)).collect(), v: (0..m.nv()).map(|v| (k as u8, v as u32)).collect(), e: (0..m.ne()).map(|e| (k as u8, e as u32)).collect() } };
    let mut cur_mesh: Mesh = Mesh::new();
    let mut cur_src: Src = Src::default();
    for i in 0..operands.len() - 1 {
        let (b, xb) = (operands[i + 1].0, &operands[i + 1].1);
        let sb = src_of(i + 1, b);
        let (m, s) = if i == 0 {
            let sa = src_of(0, first);
            pass(first, &operands[0].1, &sa, b, xb, &sb, mode)
        } else {
            let sa = cur_src.clone();
            pass(&cur_mesh, &ident, &sa, b, xb, &sb, mode)
        };
        cur_mesh = m;
        cur_src = s;
    }
    let mut m = cur_mesh;
    if dbg_on() {
        eprintln!("RUST nv {} ne {} nf {}", m.nv(), m.ne(), m.nf());
        eprintln!("V {:?}", m.pos);
        eprintln!("F {:?}", (0..m.nf()).map(|f| m.face(f).to_vec()).collect::<Vec<_>>());
        eprintln!("E {:?}", m.edges);
    }
    if std::env::var_os("GNBOOL_TIME").is_some() {
        eprintln!("fbool: {} operands, {} faces in -> {} faces, {:.3}s", operands.len(), operands.iter().map(|x| x.0.nf()).sum::<usize>(), m.nf(), t0.elapsed().as_secs_f64());
    }
    let others: Vec<&Mesh> = operands[1..].iter().map(|x| x.0).collect();
    super::csg::propagate_face_attrs_pub(&mut m, first, &others, &cur_src.f);
    super::csg::propagate_point_attrs_pub(&mut m, first, &others, &cur_src.v);
    super::csg::propagate_attrs_pub(&mut m, first, &others, &cur_src.e, Dom::Edge);
    m.mats = first.mats.clone();
    if dbg_on() {
        for (k, a) in m.attrs.iter() {
            eprintln!("ATTR {} {:?} {:?}", k, a.dom, a.data.iter().map(|v| v.f()).collect::<Vec<_>>());
        }
    }
    m
}
