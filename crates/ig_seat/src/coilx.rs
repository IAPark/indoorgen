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

fn saacos(x: f32) -> f32 {
    if x <= -1.0 {
        std::f32::consts::PI
    } else if x >= 1.0 {
        0.0
    } else {
        x.acos()
    }
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
        q::normalize(nrm)
    };
    (0..k)
        .map(|i| {
            let (p, c, n) = (co((i + k - 1) % k), co(i), co((i + 1) % k));
            let e1 = q::normalize(q::sub(c, p));
            let e2 = q::normalize(q::sub(n, c));
            let fac = saacos(-q::dot(e1, e2));
            if var("CX_RAWN") {
                fno
            } else {
                q::normalize(q::mul(fno, fac))
            }
        })
        .collect()
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
            tip.push(faces.len());
            faces.push(vec![l1[i], l1[j], l2[j], l2[i]]);
        }
        tip.push(faces.len());
        faces.push(l2);
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
        let s = if var("CX_NOSS") { s } else { ssum / kf };
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
    let b = bevel_stage(m, radius as f32).ok()?;
    let e = extrude_stage(&b, dot_depth as f32);
    let s = scale_stage(&e, (dot_size / radius) as f32);
    let mut o = Mesh::new();
    o.pos = s.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    o.faces = s.faces;
    let t = crate::coil::triangulate(&o);
    Some(crate::coil::smooth(&t, smooth_f, 5))
}
