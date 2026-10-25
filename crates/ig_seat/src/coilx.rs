//! make_coiled in float32 on a BMesh (poke chain, exact vertex bevel, extrude_region_shrink_fatten x2, ScaleElements, TRIANGULATE, SMOOTH),
//! so that tip-quad diagonals are decided by the same f32 noise as Blender.
use crate::bm::bevel::{bm_mesh_bevel_ex, BevelOpts, OffsetType};
use crate::bm::query as q;
use crate::bm::{BMesh, ELEM_TAG};
use crate::dhash::HashMap;
use crate::mesh::Mesh;

type V3 = [f32; 3];

fn mesh_to_bm_poked(m: &Mesh) -> BMesh {
    mesh_to_bm_stage(m, 4)
}

fn mesh_to_bm_stage(m: &Mesh, nstage: usize) -> BMesh {
    let pos: Vec<V3> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut ce: Vec<Vec<u32>> = vec![];
    {
        let ie = IN_EDGES.lock().unwrap();
        for e in ie.iter() {
            emap.insert((e[0].min(e[1]), e[0].max(e[1])), edges.len() as u32);
            edges.push(*e);
        }
    }
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([a, b]);
                (edges.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    crate::bm::hull::poke_all(&mut bm);
    if nstage > 1 {
        crate::bm::hull::tris_to_quads_all(&mut bm);
    }
    if nstage > 2 {
        crate::bm::hull::poke_all(&mut bm);
    }
    if nstage > 3 {
        crate::bm::hull::poke_all(&mut bm);
    }
    bm
}

pub struct Stage {
    pub pos: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
    pub tip: Vec<usize>,
    pub edges: Vec<[u32; 2]>,
}

/// edge array of the input mesh (python `mesh.edges` order); empty = derive from first appearance in faces
pub static IN_EDGES: std::sync::Mutex<Vec<[u32; 2]>> = std::sync::Mutex::new(Vec::new());

pub fn poke3_stage(m: &Mesh) -> Stage {
    let n: usize = std::env::var("NST").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let bm = mesh_to_bm_stage(m, n);
    let (pos, edges, faces) = bm.to_mesh();
    Stage { pos, faces, tip: vec![], edges }
}

/// bevel of the degree-16 vertices
pub fn bevel_stage(m: &Mesh, radius: f32) -> Result<Stage, String> {
    let mut bm = mesh_to_bm_poked(m);
    for v in bm.vert_ids() {
        if bm.edges_of_vert(v).len() == 16 {
            bm.verts[v].flag |= ELEM_TAG;
        }
    }
    let o = BevelOpts { offset: radius, offset_type: OffsetType::Offset, segments: 1, profile: 0.5, limit_offset: false, loop_slide: true, mat_nr: -1 };
    bm_mesh_bevel_ex(&mut bm, &o, true)?;
    let (pos, edges, faces) = bm.to_mesh();
    Ok(Stage { pos, faces, tip: vec![], edges })
}

/// blender::math::safe_acos_approx (used by BM vertex normals since 4.1)
fn saacos(x: f32) -> f32 {
    if !var("CX_ACPLAIN") {
        let f = x.abs();
        let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
        let p = m.mul_add(m.mul_add(m.mul_add(-0.02164095f32, 0.077980478f32), -0.213300989f32), 1.5707963267f32);
        let a = (1.0 - m).sqrt() * p;
        return if x < 0.0 { std::f32::consts::PI - a } else { a };
    }
    crate::bevmod::safe_acos_approx(x)
}

fn var(name: &str) -> bool {
    std::env::var(name).is_ok()
}

/// BM vertex normal contribution of a single polygon (the other faces at the new vertices are zero-area walls)
fn cap_vertex_normals(pos: &[V3], cap: &[u32]) -> Vec<V3> {
    let k = cap.len();
    let rot: usize = std::env::var("CX_ROT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let co = |i: usize| pos[cap[(i + rot) % k] as usize];
    // face normal (BM_face_calc_normal: Newell for n-gons)
    let fno = if k == 3 {
        q::normalize(q::cross(q::sub(co(0), co(1)), q::sub(co(1), co(2))))
    } else if k == 4 {
        q::normalize(q::cross(q::sub(co(2), co(0)), q::sub(co(3), co(1))))
    } else {
        let mut nrm = [0.0f32; 3];
        let mut prev = co(k - 1);
        for i in 0..k {
            let cur = co(i);
            if var("CX_NFMA") {
                nrm[0] = (prev[1] - cur[1]).mul_add(prev[2] + cur[2], nrm[0]);
                nrm[1] = (prev[2] - cur[2]).mul_add(prev[0] + cur[0], nrm[1]);
                nrm[2] = (prev[0] - cur[0]).mul_add(prev[1] + cur[1], nrm[2]);
            } else {
                nrm[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
                nrm[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
                nrm[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
            }
            prev = cur;
        }
        normalize_rec(nrm)
    };
    (0..k)
        .map(|i| {
            let (p, c, n) = (co((i + k - 1) % k), co(i), co((i + 1) % k));
            let e1 = normalize_rec(q::sub(c, p));
            let e2 = normalize_rec(q::sub(n, c));
            let fac = saacos(-dotv(e1, e2));
            if var("CX_RAWN") {
                fno
            } else if var("CX_VDIV") {
                normalize_div(q::mul(fno, fac))
            } else {
                normalize_rec(q::mul(fno, fac))
            }
        })
        .collect()
}

/// normalize_v3 with a true division (the variant that reproduces Blender's vertex normals)
fn dotv(a: V3, b: V3) -> f32 {
    match std::env::var("CX_DOT").ok().as_deref() {
        Some("1") => a[2].mul_add(b[2], a[1].mul_add(b[1], a[0] * b[0])),
        Some("2") => a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2])),
        Some("3") => a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1])),
        Some("4") => a[1].mul_add(b[1], a[0].mul_add(b[0], a[2] * b[2])),
        _ => q::dot(a, b),
    }
}

fn normalize_rec(a: V3) -> V3 {
    let d = dotv(a, a);
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        let f = 1.0f32 / l;
        [a[0] * f, a[1] * f, a[2] * f]
    } else {
        [0.0; 3]
    }
}

fn normalize_div(a: V3) -> V3 {
    let d = dotv(a, a);
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0; 3]
    }
}

fn madd_v(a: V3, b: V3, f: f32) -> V3 {
    if var("CX_FMA") {
        [b[0].mul_add(f, a[0]), b[1].mul_add(f, a[1]), b[2].mul_add(f, a[2])]
    } else {
        [a[0] + b[0] * f, a[1] + b[1] * f, a[2] + b[2] * f]
    }
}

/// the two region extrusions of every 16-gon cap (shrink_fatten -depth, then +depth along the cap vertex normals)
pub fn extrude_stage(st: &Stage, depth: f32) -> Stage {
    let mut pos = st.pos.clone();
    let mut faces: Vec<Vec<u32>> = vec![];
    let mut caps: Vec<Vec<u32>> = vec![];
    for f in &st.faces {
        if f.len() == 16 {
            caps.push(f.clone());
        } else {
            faces.push(f.clone());
        }
    }
    let mut tip = vec![];
    for cap in &caps {
        let k = cap.len();
        let n1 = cap_vertex_normals(&pos, cap);
        if var("CX_DUMPVN") { for (i, n) in n1.iter().enumerate() { eprintln!("VN {} {:?} -> {:?}", i, pos[cap[i] as usize], n); } }
        let sg = if var("CX_SIGN") { 1.0f32 } else { -1.0f32 };
        let l1: Vec<u32> = (0..k)
            .map(|i| {
                let p = pos[cap[i] as usize];
                pos.push(madd_v(p, n1[i], sg * depth));
                (pos.len() - 1) as u32
            })
            .collect();
        let n2 = if var("CX_N2SAME") { n1.clone() } else { cap_vertex_normals(&pos, &l1) };
        let l2: Vec<u32> = (0..k)
            .map(|i| {
                let p = pos[l1[i] as usize];
                pos.push(madd_v(p, n2[i], -sg * depth));
                (pos.len() - 1) as u32
            })
            .collect();
        for i in 0..k {
            let j = (i + 1) % k;
            tip.push(faces.len());
            faces.push(vec![cap[i], cap[j], l1[j], l1[i]]);
        }
        tip.push(faces.len());
        faces.push(l2.clone());
        for i in 0..k {
            let j = (i + 1) % k;
            tip.push(faces.len());
            faces.push(vec![l1[i], l1[j], l2[j], l2[i]]);
        }
    }
    Stage { pos, faces, tip, edges: vec![] }
}

/// GN ScaleElements (face domain, uniform scale `s`) on the tip faces; islands connect through shared vertices
pub fn scale_stage(st: &Stage, s: f32) -> Stage {
    let n = st.pos.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let nx = p[c];
            p[c] = r;
            c = nx;
        }
        r
    }
    for &fi in &st.tip {
        let f = &st.faces[fi];
        for w in f.windows(2) {
            let (a, b) = (find(&mut parent, w[0] as usize), find(&mut parent, w[1] as usize));
            parent[a] = b;
        }
    }
    let mut acc: HashMap<usize, (V3, u32, f32)> = HashMap::default();
    let mut used = vec![false; n];
    for &fi in &st.tip {
        let f = &st.faces[fi];
        let mut c = [0.0f32; 3];
        for &v in f {
            used[v as usize] = true;
            let p = st.pos[v as usize];
            c = [c[0] + p[0], c[1] + p[1], c[2] + p[2]];
        }
        let kf = f.len() as f32;
        c = if var("CX_FC") { q::mul(c, 1.0 / kf) } else { [c[0] / kf, c[1] / kf, c[2] / kf] };
        let r = find(&mut parent, f[0] as usize);
        let e = acc.entry(r).or_insert(([0.0; 3], 0, 0.0));
        e.0 = [e.0[0] + c[0], e.0[1] + c[1], e.0[2] + c[2]];
        e.1 += 1;
        e.2 += s;
    }
    let mut pos = st.pos.clone();
    for v in 0..n {
        if !used[v] {
            continue;
        }
        let r = find(&mut parent, v);
        let (c, k, ssum) = acc[&r];
        let kf = k as f32;
        let s = if var("CX_SSUM") { ssum / kf } else { s };
        let c = if var("CX_IC") { q::mul(c, 1.0 / kf) } else { [c[0] / kf, c[1] / kf, c[2] / kf] };
        let p = st.pos[v];
        pos[v] = if var("CX_SMAT") {
            // translate(c) * scale * translate(-c) as a matrix: s*p + (c - s*c)
            [p[0] * s + (c[0] - c[0] * s), p[1] * s + (c[1] - c[1] * s), p[2] * s + (c[2] - c[2] * s)]
        } else {
            [c[0] + (p[0] - c[0]) * s, c[1] + (p[1] - c[1]) * s, c[2] + (p[2] - c[2]) * s]
        };
    }
    Stage { pos, faces: st.faces.clone(), tip: st.tip.clone(), edges: vec![] }
}

/// mattress.make_coiled in float32 (see module doc). `edges`: python mesh.edges order of `m` when known.
pub fn make_coiled_f32(m: &Mesh, dot_distance: f64, dot_depth: f64, dot_size: f64, radius_u: f64, smooth_f: f64, edges: Option<&[[u32; 2]]>) -> Option<Mesh> {
    let radius = dot_distance * radius_u;
    {
        let mut ie = IN_EDGES.lock().unwrap();
        ie.clear();
        if let Some(e) = edges {
            ie.extend_from_slice(e);
        }
    }
    let dump = |name: &str, pos: &[V3], faces: &[Vec<u32>]| {
        if let Ok(d) = std::env::var("COILX_DUMP") {
            let v: Vec<_> = pos.iter().map(|p| serde_json::json!([p[0] as f64, p[1] as f64, p[2] as f64])).collect();
            std::fs::write(format!("{d}/{name}.json"), serde_json::json!({"verts": v, "polys": faces}).to_string()).ok();
        }
    };
    let e = if var("CX_OLDEXTR") {
        let b = bevel_stage(m, radius as f32).ok()?;
        dump("bevel", &b.pos, &b.faces);
        extrude_stage(&b, dot_depth as f32)
    } else {
        bevel_extrude_bm(m, radius as f32, dot_depth as f32).ok()?
    };
    dump("extr", &e.pos, &e.faces);
    let s = scale_stage(&e, (dot_size / radius) as f32);
    dump("scale", &s.pos, &s.faces);
    let mut o = Mesh::new();
    o.pos = s.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    o.faces = s.faces;
    let t = triangulate_shortest(&o);
    let tp: Vec<V3> = t.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    dump("tri", &tp, &t.faces);
    let f = crate::coil::smooth(&t, smooth_f, 5);
    let fp: Vec<V3> = f.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    dump("final", &fp, &f.faces);
    Some(f)
}

/// TRIANGULATE modifier (min_vertices 4, quad_method SHORTEST_DIAGONAL (the modifier default), ngon_method BEAUTY), float32
pub fn triangulate_shortest(m: &Mesh) -> Mesh {
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    let c3 = |v: u32| {
        let p = m.pos[v as usize];
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    for f in &m.faces {
        if f.len() <= 3 {
            o.faces.push(f.clone());
        } else if f.len() == 4 {
            // l_v1 = corner 1, l_v2 = corner 2, l_v3 = corner 3, l_v4 = corner 0
            let (v1, v2, v3, v4) = (c3(f[1]), c3(f[2]), c3(f[3]), c3(f[0]));
            let d1 = q::len_squared_v3v3(v4, v2);
            let d2 = q::len_squared_v3v3(v1, v3);
            if (d2 - d1) > 0.0 {
                o.faces.push(vec![f[0], f[1], f[2]]);
                o.faces.push(vec![f[0], f[2], f[3]]);
            } else {
                o.faces.push(vec![f[0], f[1], f[3]]);
                o.faces.push(vec![f[1], f[2], f[3]]);
            }
        } else {
            let co: Vec<[f32; 3]> = f.iter().map(|&v| c3(v)).collect();
            for t in crate::polyfill::triangulate_ngon(&co) {
                o.faces.push(vec![f[t[0] as usize], f[t[1] as usize], f[t[2] as usize]]);
            }
        }
    }
    o
}

// ---------------------------------------------------------------------------------------------------------------------------------
// BMesh version of the two region extrusions: Blender's element order (pool slots), vertex normals of BM_vert_calc_normal_ex

/// bmo_extrude_face_region for the faces `sel` (all selected faces at once): duplicate (verts, edges, faces in pool order), delete the
/// originals, side faces over the boundary edges. Returns (new faces in pool order, side faces in creation order).
fn extrude_region_bm(bm: &mut BMesh, sel: &[usize]) -> (Vec<usize>, Vec<usize>) {
    use crate::bm::NONE;
    let mut in_f: crate::dhash::HashSet<usize> = crate::dhash::HashSet::default();
    let mut in_v: crate::dhash::HashSet<usize> = crate::dhash::HashSet::default();
    let mut in_e: crate::dhash::HashSet<usize> = crate::dhash::HashSet::default();
    for &f in sel {
        in_f.insert(f);
        for v in bm.verts_of_face(f) {
            in_v.insert(v);
        }
        for e in bm.edges_of_face(f) {
            in_e.insert(e);
        }
    }
    let mut vmap: HashMap<usize, usize> = HashMap::default();
    let mut emap: HashMap<usize, usize> = HashMap::default();
    // duplicate: verts, edges, faces in pool order
    for v in bm.vert_ids() {
        if in_v.contains(&v) {
            let nv = bm.vert_create(bm.verts[v].co, None);
            bm.elem_attrs_copy_vert(v, nv);
            vmap.insert(v, nv);
        }
    }
    let mut boundary: Vec<(usize, usize)> = vec![];
    for e in bm.edge_ids() {
        if in_e.contains(&e) {
            let (a, b) = (vmap[&bm.edges[e].v1], vmap[&bm.edges[e].v2]);
            let ne = bm.edge_create(a, b, None, false);
            bm.elem_attrs_copy_edge(e, ne);
            // radial faces that are inputs
            let mut rlen = 0;
            for f in bm.faces_of_edge(e) {
                if in_f.contains(&f) {
                    rlen += 1;
                }
            }
            if rlen < 2 {
                boundary.push((e, ne));
            }
            emap.insert(e, ne);
        }
    }
    let mut new_faces = vec![];
    for f in bm.face_ids() {
        if in_f.contains(&f) {
            let ls = bm.loops_of_face(f);
            let vt: Vec<usize> = ls.iter().map(|&l| vmap[&bm.loops[l].v]).collect();
            let et: Vec<usize> = ls.iter().map(|&l| emap[&bm.loops[l].e]).collect();
            let nf = bm.face_create(&vt, &et, None, false);
            bm.elem_attrs_copy_face(f, nf);
            new_faces.push(nf);
        }
    }
    // delete the original faces (the original verts / edges stay: they border non-input faces)
    for &f in sel {
        bm.face_kill(f);
    }
    let mut walls = vec![];
    for (e, ne) in boundary {
        let l = bm.edges[ne].l;
        let fwd = if l != NONE { bm.loops[l].v == bm.edges[ne].v1 } else { true };
        let flip = !fwd;
        let (e1, e2) = (bm.edges[e].v1, bm.edges[e].v2);
        let (n1, n2) = (bm.edges[ne].v1, bm.edges[ne].v2);
        let fv = if !flip { [e1, e2, n2, n1] } else { [e2, e1, n1, n2] };
        let f = bm.face_create_verts(&fv, None, true);
        walls.push(f);
    }
    (new_faces, walls)
}

/// BM_vert_calc_normal_ex(v, SELECT) for a vertex of the selected face `f` (only `f` is selected around the new vertices)
fn bm_vert_normal_sel(bm: &BMesh, f: usize, v: usize) -> V3 {
    let l = bm.face_vert_share_loop(f, v);
    let co = bm.verts[bm.loops[l].v].co;
    let prev = bm.verts[bm.loops[bm.loops[l].p].v].co;
    let next = bm.verts[bm.loops[bm.loops[l].n].v].co;
    let v1 = normalize_rec(q::sub(co, prev));
    let v2 = normalize_rec(q::sub(next, co));
    let fac = saacos(-dotv(v1, v2));
    let mut no = [0.0f32; 3];
    let fno = bm.faces[f].no;
    no = [no[0] + fno[0] * fac, no[1] + fno[1] * fac, no[2] + fno[2] * fac];
    normalize_rec(no)
}

/// transform shrink/fatten of the verts of the selected caps: co = co + n * distance
fn shrink_fatten_bm(bm: &mut BMesh, caps: &[usize], distance: f32) {
    let mut moves: Vec<(usize, V3)> = vec![];
    for &f in caps {
        for v in bm.verts_of_face(f) {
            let n = bm_vert_normal_sel(bm, f, v);
            moves.push((v, madd_v(bm.verts[v].co, n, distance)));
        }
    }
    for (v, c) in moves {
        bm.verts[v].co = c;
    }
}

/// bevel -> extrude_region_shrink_fatten(-depth) -> extrude_region_shrink_fatten(+depth) -> select_more x2, all on one BMesh
pub fn bevel_extrude_bm(m: &Mesh, radius: f32, depth: f32) -> Result<Stage, String> {
    let mut bm = mesh_to_bm_poked(m);
    for v in bm.vert_ids() {
        if bm.edges_of_vert(v).len() == 16 {
            bm.verts[v].flag |= ELEM_TAG;
        }
    }
    let o = BevelOpts { offset: radius, offset_type: OffsetType::Offset, segments: 1, profile: 0.5, limit_offset: false, loop_slide: true, mat_nr: -1 };
    bm_mesh_bevel_ex(&mut bm, &o, true)?;
    bm.normals_update();
    let caps: Vec<usize> = bm.face_ids().into_iter().filter(|&f| bm.faces[f].len == 16).collect();
    let (caps1, walls1) = extrude_region_bm(&mut bm, &caps);
    shrink_fatten_bm(&mut bm, &caps1, -depth);
    bm.normals_update();
    let (caps2, walls2) = extrude_region_bm(&mut bm, &caps1);
    shrink_fatten_bm(&mut bm, &caps2, depth);
    bm.normals_update();
    // select_more x2 from the new caps: caps2 + walls2, then walls1 (faces touching their vertices)
    let mut tip_ids: Vec<usize> = vec![];
    tip_ids.extend(&caps2);
    tip_ids.extend(&walls2);
    tip_ids.extend(&walls1);
    let (pos, edges, faces) = bm.to_mesh();
    let fids = bm.face_ids();
    let mut rank: HashMap<usize, usize> = HashMap::default();
    for (i, &f) in fids.iter().enumerate() {
        rank.insert(f, i);
    }
    let mut tip: Vec<usize> = tip_ids.iter().map(|f| rank[f]).collect();
    tip.sort();
    Ok(Stage { pos, faces, tip, edges })
}

// ---------------------------------------------------------------------------------------------------------------------------------
/// bpy.ops.mesh.normals_make_consistent(inside=False) (bmo_recalc_face_normals) on a BMesh: face groups over manifold edges, outer-most
/// loop of every group decides whether the group is flipped, flipping keeps the first loop (BM_face_normal_flip).
pub fn recalc_face_normals_bm(bm: &mut BMesh) {
    use crate::bm::NONE;
    bm.normals_update();
    let fids = bm.face_ids();
    let mut group_of: HashMap<usize, usize> = HashMap::default();
    let mut groups: Vec<Vec<usize>> = vec![];
    for &f0 in &fids {
        if group_of.contains_key(&f0) {
            continue;
        }
        let gi = groups.len();
        let mut grp = vec![];
        let mut stack = vec![f0];
        group_of.insert(f0, gi);
        while let Some(f) = stack.pop() {
            grp.push(f);
            for l in bm.loops_of_face(f) {
                let e = bm.loops[l].e;
                if bm.edge_is_manifold(e) {
                    let lo = bm.loops[l].rn;
                    let fo = bm.loops[lo].f;
                    if fo != f && !group_of.contains_key(&fo) {
                        group_of.insert(fo, gi);
                        stack.push(fo);
                    }
                }
            }
        }
        grp.sort();
        groups.push(grp);
    }
    for grp in groups {
        // area-weighted median-weighted centre
        let mut cent = [0.0f32; 3];
        let mut area_acc = 0.0f32;
        let cent_fac = 1.0f32 / grp.len() as f32;
        for &f in &grp {
            let vs = bm.verts_of_face(f);
            let co: Vec<V3> = vs.iter().map(|&v| bm.verts[v].co).collect();
            // area via fan cross products
            let mut a = [0.0f32; 3];
            for i in 1..co.len() - 1 {
                let c = q::cross(q::sub(co[i], co[0]), q::sub(co[i + 1], co[0]));
                a = [a[0] + c[0], a[1] + c[1], a[2] + c[2]];
            }
            let area = 0.5 * q::len(a);
            // BM_face_calc_center_median_weighted
            let ls = bm.loops_of_face(f);
            let k = ls.len();
            let mut fc = [0.0f32; 3];
            let mut totw = 0.0f32;
            let mut w_prev = bm.edge_calc_length(bm.loops[ls[k - 1]].e);
            for &l in &ls {
                let w_curr = bm.edge_calc_length(bm.loops[l].e);
                let w = w_curr + w_prev;
                fc = q::madd(fc, bm.verts[bm.loops[l].v].co, w);
                totw += w;
                w_prev = w_curr;
            }
            if totw != 0.0 {
                fc = q::mul(fc, 1.0 / totw);
            }
            cent = q::madd(cent, fc, cent_fac * area);
            area_acc += area;
        }
        if area_acc != 0.0 {
            cent = q::mul(cent, 1.0 / area_acc);
        }
        let eps = f32::EPSILON;
        let (mut best_d, mut best_e, mut best_l) = (eps, -f32::MAX, -f32::MAX);
        let (mut start, mut is_flip) = (0usize, false);
        for (i, &f) in grp.iter().enumerate() {
            for l in bm.loops_of_face(f) {
                let co = bm.verts[bm.loops[l].v].co;
                let mut dir = q::sub(co, cent);
                let dist_sq = q::dot(dir, dir);
                let is_best_d = dist_sq > best_d;
                if is_best_d || dist_sq == best_d {
                    dir = q::mul(dir, 1.0 / dist_sq.sqrt());
                    let nx = bm.verts[bm.loops[bm.loops[l].n].v].co;
                    let pv = bm.verts[bm.loops[bm.loops[l].p].v].co;
                    let (e0, l0) = q::normalize_len(q::sub(nx, co));
                    let (e1, l1) = q::normalize_len(q::sub(pv, co));
                    if l0 > eps && l1 > eps {
                        let edge_dot = q::dot(dir, e0).max(q::dot(dir, e1));
                        let is_best_e = edge_dot > best_e;
                        if is_best_d || is_best_e || edge_dot == best_e {
                            let (mut loop_dir, ll) = q::normalize_len(q::cross(e0, e1));
                            if ll > eps {
                                if q::dot(loop_dir, bm.faces[f].no) < 0.0 {
                                    loop_dir = [-loop_dir[0], -loop_dir[1], -loop_dir[2]];
                                }
                                let ld = q::dot(dir, loop_dir);
                                let loop_dot = ld.abs();
                                if is_best_d || is_best_e || loop_dot > best_l {
                                    best_d = dist_sq;
                                    best_e = edge_dot;
                                    best_l = loop_dot;
                                    start = i;
                                    is_flip = ld < 0.0;
                                }
                            }
                        }
                    }
                }
            }
        }
        // propagate the flip state
        let mut flip: HashMap<usize, bool> = HashMap::default();
        let mut seen: crate::dhash::HashSet<usize> = crate::dhash::HashSet::default();
        let fs = grp[start];
        if is_flip {
            flip.insert(fs, true);
        }
        let mut stack = vec![fs];
        seen.insert(fs);
        while let Some(f) = stack.pop() {
            let state = *flip.get(&f).unwrap_or(&false);
            for l in bm.loops_of_face(f) {
                let lo = bm.loops[l].rn;
                if lo != l && bm.edge_is_manifold(bm.loops[l].e) {
                    let fo = bm.loops[lo].f;
                    if !seen.contains(&fo) {
                        seen.insert(fo);
                        flip.insert(fo, (bm.loops[lo].v == bm.loops[l].v) != state);
                        stack.push(fo);
                    }
                }
            }
        }
        for &f in &grp {
            if *flip.get(&f).unwrap_or(&false) {
                bm.face_normal_flip(f);
            }
        }
        let _ = NONE;
    }
}
