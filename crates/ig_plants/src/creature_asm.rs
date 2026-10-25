//! genome_to_creature + join_and_rig_parts for NatureShelfTrinkets (hair off, no rig, no remesh): part meshes, raycast attachment (apply_attach_transform),
//! world transforms, remove_doubles(0.01) + BEAUTY triangulation of the body parts, extras joined untouched.
use super::creature::*;
use super::creature_gen::*;
use super::creature_nurbs::skeleton_to_tangents;
use ig_core::rng::Rng;
use serde_json::{json, Value as J};

type V3 = [f64; 3];
type M4 = [[f64; 4]; 4]; // row-major, column vectors: p' = M * p

const ID4: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn mm(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}
fn xf_point(m: &M4, p: V3) -> V3 {
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}

// quaternions (w, x, y, z) like mathutils
type Q = [f64; 4];
fn qmul(a: Q, b: Q) -> Q {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}
fn qrot(q: Q, v: V3) -> V3 {
    // q * v * q^-1 for a unit quaternion
    let p = qmul(qmul(q, [0.0, v[0], v[1], v[2]]), [q[0], -q[1], -q[2], -q[3]]);
    [p[1], p[2], p[3]]
}
/// Euler XYZ (radians) -> quaternion (mathutils eul_to_quat)
fn eul_to_q(e: V3) -> Q {
    let (ti, tj, th) = (e[0] * 0.5, e[1] * 0.5, e[2] * 0.5);
    let (ci, cj, ch) = (ti.cos(), tj.cos(), th.cos());
    let (si, sj, sh) = (ti.sin(), tj.sin(), th.sin());
    let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
    [cj * cc + sj * ss, cj * sc - sj * cs, cj * ss + sj * cc, cj * cs - sj * sc]
}
fn euler_deg(r: f64, p: f64, y: f64) -> Q {
    eul_to_q([r.to_radians(), p.to_radians(), y.to_radians()])
}
fn q_to_m4(q: Q) -> M4 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (w, x, y, z) = (q[0] / n, q[1] / n, q[2] / n, q[3] / n);
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y), 0.0],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x), 0.0],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y), 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
/// quat_align_vecs: Quaternion(a.cross(b), a.angle(b))
fn quat_align(a: V3, b: V3) -> Q {
    let ax = cross(a, b);
    let l = len(ax);
    if l == 0.0 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let (la, lb) = (len(a), len(b));
    let ang = if la == 0.0 || lb == 0.0 { 0.0 } else { (dot(a, b) / (la * lb)).clamp(-1.0, 1.0).acos() };
    let s = (ang * 0.5).sin();
    [(ang * 0.5).cos(), ax[0] / l * s, ax[1] / l * s, ax[2] / l * s]
}

// ------------------------------------------------------------------------------------------------ part data

pub struct PartData {
    pub skin: CMesh,
    pub skeleton: Vec<V3>,
    /// attach_basemesh ("Base Mesh" output)
    pub base: Option<CMesh>,
    pub extras: Vec<(String, CMesh)>,
    /// cutter extras (name contains "Cutter") are removed after cutting the parent
    pub cutters: Vec<CMesh>,
}

fn infer_skeleton(m: &CMesh) -> Vec<V3> {
    if m.pos.is_empty() {
        return vec![[0.0, 0.0, 0.0], [0.1, 0.0, 0.0]];
    }
    let mut imin = 0;
    let mut imax = 0;
    for (i, p) in m.pos.iter().enumerate() {
        if p[0] < m.pos[imin][0] {
            imin = i;
        }
        if p[0] > m.pos[imax][0] {
            imax = i;
        }
    }
    let f = |p: [f32; 3]| [p[0] as f64, p[1] as f64, p[2] as f64];
    vec![f(m.pos[imin]), f(m.pos[imax])]
}

fn vert_mesh() -> CMesh {
    CMesh { pos: vec![[0.0; 3]], faces: vec![] }
}

fn top_group(k: &Kind) -> &'static str {
    match k {
        Kind::Tail => "nodegroup_tail",
        Kind::BackLeg => "nodegroup_quadruped_back_leg",
        Kind::FrontLeg => "nodegroup_quadruped_front_leg",
        Kind::Foot => "nodegroup_foot",
        Kind::CarnivoreHead => "nodegroup_carnivore_head",
        Kind::Jaw => "nodegroup_carnivore_jaw (no gc)",
        Kind::Eye => "nodegroup_mammal_eye (no gc)",
        Kind::CatNose => "nodegroup_cat_nose",
        Kind::CatEar => "nodegroup_cat_ear",
        Kind::HoofAnkle => "nodegroup_hoof",
        Kind::Horn => "nodegroup_horn",
        _ => "",
    }
}

fn to_mesh(m: &CMesh) -> crate::mesh::Mesh {
    crate::mesh::Mesh { pos: m.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.faces.clone(), uv: vec![] }
}
fn from_mesh(m: &crate::mesh::Mesh) -> CMesh {
    CMesh { pos: m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), faces: m.faces.clone() }
}

/// SUBSURF modifier levels=2 (apply) then optional uniform scale
fn subsurf2(m: &CMesh, scale: f32, gedges: Option<&[[u32; 2]]>) -> CMesh {
    let mm = to_mesh(m);
    let edges = match gedges { Some(e) => e.to_vec(), None => crate::bathkit::edge_list(&mm.faces).into_iter().map(|(a, b)| [a, b]).collect::<Vec<_>>() };
    let s = crate::subsurf::subsurf_osd(&mm, 2, false, Some(&edges)).unwrap_or_else(|| crate::subsurf::subsurf(&mm, 2, false));
    let mut o = from_mesh(&s);
    if scale != 1.0 {
        for p in o.pos.iter_mut() {
            for k in 0..3 {
                p[k] *= scale;
            }
        }
    }
    o
}

pub fn make_part(fac: &Fac, rng: &mut Rng) -> PartData {
    match &fac.kind {
        Kind::NurbsBody(p) | Kind::NurbsHead(p) => {
            let (skin, skeleton) = make_nurbs_part(p);
            PartData { skin, skeleton, base: None, extras: vec![], cutters: vec![] }
        }
        Kind::HoofClaw => {
            let m = hoof_claw_mesh(&fac.params, rng);
            PartData { skin: vert_mesh(), skeleton: vec![[0.0; 3]], base: None, extras: vec![("HoofClaw".into(), m)], cutters: vec![] }
        }
        k => {
            let out = run_group(top_group(k), &fac.params);
            let geo = out.mesh("Geometry").cloned();
            let geo_edges = out.names.iter().position(|n| n == "Geometry").map(|i| out.edges[i].clone());
            let mut skin = geo.clone().unwrap_or_else(vert_mesh);
            let skeleton = out.skeleton().unwrap_or_else(|| infer_skeleton(&skin));
            let base = out.mesh("Base Mesh").cloned();
            let mut extras = vec![];
            let mut cutters = vec![];
            for (n, m) in out.names.iter().zip(out.meshes.iter()) {
                if n == "Geometry" || n == "Base Mesh" || n == "Skeleton Curve" {
                    continue;
                }
                if n.contains("Cutter") {
                    cutters.push(m.clone());
                } else {
                    extras.push((n.clone(), m.clone()));
                }
            }
            match k {
                Kind::CatNose => {
                    // part.obj = spawn_vert; the nose mesh becomes the "Nose" extra
                    extras.push(("Nose".into(), skin));
                    skin = vert_mesh();
                }
                Kind::Horn => {
                    let h = subsurf2(&skin, 1.0, geo_edges.as_deref());
                    extras.push(("Horn".into(), h));
                    skin = vert_mesh();
                }
                Kind::HoofAnkle => {
                    skin = subsurf2(&skin, 0.8, geo_edges.as_deref());
                }
                _ => {}
            }
            PartData { skin, skeleton, base, extras, cutters }
        }
    }
}

// ------------------------------------------------------------------------------------------------ raycast attachment

fn lerp_sample32(vec: &[[f32; 3]], t: f32) -> [f32; 3] {
    let idx = (t.floor() as i64).clamp(0, vec.len() as i64 - 1) as usize;
    let rem = t - idx as f32;
    let mut res = vec[idx];
    if idx < vec.len() - 1 {
        for c in 0..3 {
            res[c] = (1.0 - rem) * res[c] + rem * vec[idx + 1][c];
        }
    }
    res
}

/// BVHTree.ray_cast on the triangulated mesh: closest hit in front of the origin; returns (location, normal)
fn ray_cast(m: &CMesh, origin: [f32; 3], dir: [f32; 3]) -> Option<([f32; 3], [f32; 3])> {
    let tris = tessellate(m);
    let dl = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let d = [dir[0] / dl, dir[1] / dl, dir[2] / dl];
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let fma = std::env::var("CRFMA").is_ok();
    let fm2 = std::env::var("CRFMA").as_deref() == Ok("2");
    let cr = |a: [f32; 3], b: [f32; 3]| if fm2 { [(-a[2]).mul_add(b[1], a[1] * b[2]), (-a[0]).mul_add(b[2], a[2] * b[0]), (-a[1]).mul_add(b[0], a[0] * b[1])] } else if fma { [a[1].mul_add(b[2], -(a[2] * b[1])), a[2].mul_add(b[0], -(a[0] * b[2])), a[0].mul_add(b[1], -(a[1] * b[0]))] } else { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] };
    let dt = |a: [f32; 3], b: [f32; 3]| if fm2 { a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2])) } else if fma { a[2].mul_add(b[2], a[1].mul_add(b[1], a[0] * b[0])) } else { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] };
    let mut best: Option<(f32, [u32; 3])> = None;
    for t in &tris {
        let (v0, v1, v2) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        let e1 = sub(v1, v0);
        let e2 = sub(v2, v0);
        let p = cr(d, e2);
        let a = dt(e1, p);
        if a > -1e-6 && a < 1e-6 {
            continue;
        }
        let f = 1.0 / a;
        let s = sub(origin, v0);
        let u = f * dt(s, p);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = cr(s, e1);
        let v = f * dt(d, q);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let lam = f * dt(e2, q);
        if lam < 0.0 {
            continue;
        }
        if best.map_or(true, |b| lam < b.0) {
            best = Some((lam, *t));
        }
    }
    best.map(|(lam, t)| {
        let (v0, v1, v2) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        let n = cr(sub(v0, v1), sub(v1, v2));
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        ([origin[0] + d[0] * lam, origin[1] + d[1] * lam, origin[2] + d[2] * lam], [n[0] / nl, n[1] / nl, n[2] / nl])
    })
}

/// Blender corner tris (mesh_calc_tessellation_for_face): quads flip to (0,1,3),(1,2,3) per is_quad_flip_v3_first_third_fast; n-gons by ear clipping
pub fn tessellate(m: &CMesh) -> Vec<[u32; 3]> {
    let mut t = vec![];
    for f in &m.faces {
        match f.len() {
            3 => t.push([f[0], f[1], f[2]]),
            4 => {
                let p = |i: usize| m.pos[f[i] as usize];
                let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
                let cr = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
                let (d12, d13, d14) = (sub(p(1), p(0)), sub(p(2), p(0)), sub(p(3), p(0)));
                let (ca, cb) = (cr(d12, d13), cr(d14, d13));
                let dd = ca[0] * cb[0] + ca[1] * cb[1] + ca[2] * cb[2];
                let flip = dd > 0.0; // is_quad_flip_v3_first_third_fast (verified against python BVHTree.ray_cast hits)
                if flip {
                    t.push([f[0], f[1], f[3]]);
                    t.push([f[1], f[2], f[3]]);
                } else {
                    t.push([f[0], f[1], f[2]]);
                    t.push([f[0], f[2], f[3]]);
                }
            }
            n if n > 4 => {
                let co: Vec<[f32; 3]> = f.iter().map(|&i| m.pos[i as usize]).collect();
                for tr in crate::polyfill::triangulate_ngon(&co) {
                    t.push([f[tr[0] as usize], f[tr[1] as usize], f[tr[2] as usize]]);
                }
            }
            _ => {}
        }
    }
    t
}

/// local transform of `part` attached to `target` (apply_attach_transform): S_y(side) * T(loc) * R(rot)
fn attach_local(target: &PartData, att: &Att, bvh_mesh: &CMesh) -> M4 {
    let (u, v, rad) = (att.coord[0], att.coord[1], att.coord[2]);
    let sk = &target.skeleton;
    let n = sk.len();
    let idx = (u * (n - 1) as f64) as f32;
    let tang = skeleton_to_tangents(sk);
    let forward = lerp_sample32(&tang, idx);
    let sk32: Vec<[f32; 3]> = sk.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let origin = lerp_sample32(&sk32, idx);
    let fwd = [forward[0] as f64, forward[1] as f64, forward[2] as f64];
    let basis = quat_align([1.0, 0.0, 0.0], fwd);
    let dir_rot = qmul(euler_deg(180.0 * v, 0.0, 0.0), euler_deg(0.0, 90.0, 0.0));
    let direction = qrot(qmul(basis, dir_rot), [1.0, 0.0, 0.0]);
    let hit = ray_cast(bvh_mesh, origin, [direction[0] as f32, direction[1] as f32, direction[2] as f32]);
    if std::env::var("CRDBG").is_ok() { eprintln!("attach u {u} v {v} rad {rad}: origin {:?} dir {:?} hit {:?}", origin, direction, hit); }
    let (location, normal) = match hit {
        Some((l, nrm)) => (l, [nrm[0] as f64, nrm[1] as f64, nrm[2] as f64]),
        None => (origin, [1.0, 0.0, 0.0]),
    };
    let r = rad as f32;
    let loc = [0, 1, 2].map(|c| ((1.0 - r) * origin[c] + r * location[c]) as f64);
    let basis_rot = match att.basis {
        Basis::Global => [1.0, 0.0, 0.0, 0.0],
        Basis::Normal => quat_align([1.0, 0.0, 0.0], normal),
    };
    let rot = qmul(basis_rot, euler_deg(att.rest[0], att.rest[1], att.rest[2]));
    let mut m = q_to_m4(rot);
    for i in 0..3 {
        m[i][3] = loc[i];
    }
    let mut s = ID4;
    s[1][1] = att.side as f64;
    mm(&s, &m)
}

// ------------------------------------------------------------------------------------------------ assembly

thread_local! {
    /// debugging: world matrices (preorder part order) of the last build
    pub static LAST_WORLDS: std::cell::RefCell<Vec<[[f64; 4]; 4]>> = std::cell::RefCell::new(vec![]);
    /// debugging: raycast meshes to use instead of the generated ones, by preorder index
    pub static BVH_OVERRIDE: std::cell::RefCell<std::collections::HashMap<usize, CMesh>> = std::cell::RefCell::new(Default::default());
    pub static LAST_SKINS: std::cell::RefCell<Vec<CMesh>> = std::cell::RefCell::new(vec![]);
}

fn xf_mesh(m: &CMesh, w: &M4) -> CMesh {
    CMesh { pos: m.pos.iter().map(|p| xf_point(w, [p[0] as f64, p[1] as f64, p[2] as f64])).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), faces: m.faces.clone() }
}

/// (body parts joined, extras joined) in creature space
pub fn build_creature_parts(herb: bool, rng: &mut Rng) -> (CMesh, CMesh) {
    let (g, root) = if herb { herbivore_genome(rng) } else { tiger_genome(rng) };
    let order = g.preorder(root);
    let mut data: Vec<Option<PartData>> = (0..g.nodes.len()).map(|_| None).collect();
    for &n in &order {
        data[n] = Some(make_part(&g.facs[g.nodes[n].fac], rng));
    }
    // attach children before their parents (postorder); the BVH of a target is built once from its mesh at the first raycast
    let mut local: Vec<M4> = vec![ID4; g.nodes.len()];
    let mut bvh: Vec<Option<CMesh>> = vec![None; g.nodes.len()];
    for (parent, n) in g.postorder_pairs(root) {
        let Some(p) = parent else { continue };
        let att = g.nodes[n].att.clone().unwrap();
        if bvh[p].is_none() {
            let d = data[p].as_ref().unwrap();
            let pre = order.iter().position(|&x| x == p).unwrap();
            let ov = BVH_OVERRIDE.with(|o| o.borrow().get(&pre).cloned());
            bvh[p] = Some(ov.unwrap_or_else(|| d.base.clone().unwrap_or_else(|| d.skin.clone())));
        }
        local[n] = attach_local(data[p].as_ref().unwrap(), &att, bvh[p].as_ref().unwrap());
        // cutters of the attached part are subtracted from the target's mesh
        let cut: Vec<CMesh> = data[n].as_ref().unwrap().cutters.iter().map(|c| xf_mesh(c, &local[n])).collect();
        for c in cut {
            let tgt = data[p].as_mut().unwrap();
            tgt.skin = cut_mesh(&tgt.skin, &c);
        }
    }
    let mut world: Vec<M4> = vec![ID4; g.nodes.len()];
    for &n in &order {
        if let Some((pi, _)) = g.postorder_pairs(root).into_iter().find(|(_, c)| *c == n).and_then(|(p, c)| p.map(|p| (p, c))) {
            world[n] = mm(&world[pi], &local[n]);
        }
    }
    LAST_WORLDS.with(|w| *w.borrow_mut() = order.iter().map(|&n| world[n]).collect());
    LAST_SKINS.with(|w| *w.borrow_mut() = order.iter().map(|&n| data[n].as_ref().unwrap().skin.clone()).collect());
    let mut out = CMesh::default();
    let mut ext = CMesh::default();
    let add = |m: &CMesh, out: &mut CMesh| {
        let base = out.pos.len() as u32;
        out.pos.extend_from_slice(&m.pos);
        for f in &m.faces {
            out.faces.push(f.iter().map(|&i| i + base).collect());
        }
    };
    for &n in &order {
        let d = data[n].as_ref().unwrap();
        // body part: world transform, remove_doubles(0.01), BEAUTY triangulation
        let w = xf_mesh(&d.skin, &world[n]);
        let merged = crate::blops::remove_doubles(&to_mesh(&w), 0.01);
        let tri = triangulate_beauty_c(&from_mesh(&merged));
        if std::env::var("CRDBG").is_ok() { eprintln!("part {n}: skin {} verts {} faces -> merged {} verts {} faces -> tris {}", d.skin.pos.len(), d.skin.faces.len(), merged.pos.len(), merged.faces.len(), tri.faces.len()); }
        add(&tri, &mut out);
    }
    for &n in &order {
        let d = data[n].as_ref().unwrap();
        for (_, e) in &d.extras {
            add(&xf_mesh(e, &world[n]), &mut ext);
        }
    }
    (out, ext)
}

pub fn build_creature(herb: bool, rng: &mut Rng) -> CMesh {
    let (mut a, e) = build_creature_parts(herb, rng);
    let base = a.pos.len() as u32;
    a.pos.extend_from_slice(&e.pos);
    for f in &e.faces {
        a.faces.push(f.iter().map(|&i| i + base).collect());
    }
    a
}

/// bpy.ops.mesh.quads_convert_to_tris(quad_method=BEAUTY, ngon_method=BEAUTY): same order as trinkets::triangulate_beauty (triangles first, then the second halves)
pub fn triangulate_beauty_c(m: &CMesh) -> CMesh {
    let mut first: Vec<Vec<u32>> = vec![];
    let mut extra: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        match f.len() {
            3 => first.push(f.clone()),
            4 => {
                let p = |k: usize| m.pos[f[k] as usize];
                if crate::polyfill::quad_split_24_beauty([p(0), p(1), p(2), p(3)]) {
                    first.push(vec![f[0], f[2], f[3]]);
                    extra.push(vec![f[0], f[1], f[2]]);
                } else {
                    first.push(vec![f[1], f[3], f[0]]);
                    extra.push(vec![f[1], f[2], f[3]]);
                }
            }
            n => {
                for k in 1..n - 1 {
                    first.push(vec![f[0], f[k], f[k + 1]]);
                }
            }
        }
    }
    first.extend(extra);
    CMesh { pos: m.pos.clone(), faces: first }
}

/// Boolean modifier DIFFERENCE (solver FAST) of `tgt` minus `cutter`
fn cut_mesh(tgt: &CMesh, cutter: &CMesh) -> CMesh {
    use crate::gn::geom::Mesh as GM;
    let id: crate::gn::geom::M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let a = GM::from_faces(tgt.pos.clone(), &tgt.faces, &[]);
    let b = GM::from_faces(cutter.pos.clone(), &cutter.faces, &[]);
    let r = crate::gn::fbool::mesh_boolean_float("DIFFERENCE", &[(&a, id), (&b, id)]);
    CMesh::from_gn(&r)
}

/// hoof.Hoof.generate (HoofClaw.make_part): a closed-ish lofted nurbs cup (geomdl, face_size 0.01) with a cap n-gon of its z == 0 ring, origin moved to the centre of
/// the top ring and rotated by -pi/2 about Y. Draws 2 normals per ring point from the global stream, in order.
pub fn hoof_claw_mesh(p: &J, rng: &mut Rng) -> CMesh {
    let n = p["n"].as_f64().unwrap() as usize;
    let m = p["m"].as_f64().unwrap() as usize;
    let (sx, sy, sz, r) = (p["sx"].as_f64().unwrap(), p["sy"].as_f64().unwrap(), p["sz"].as_f64().unwrap(), p["r"].as_f64().unwrap());
    let pi = std::f64::consts::PI;
    let mut points = vec![];
    for i in 0..m {
        let theta = 2.0 * pi * i as f64 / m as f64;
        let nx = rng.normal(0.0, 0.01);
        let ny = rng.normal(0.0, 0.01);
        let (fi, fm) = (i as f64, m as f64);
        if fi >= fm - r || fi <= r {
            points.push((-0.2 * theta.cos() + nx, 0.05 * theta.sin() + ny));
        } else if fi >= fm - 2.0 * r || fi <= 2.0 * r {
            points.push((theta.cos() + nx, 0.2 * theta.sin() + ny));
        } else {
            points.push((theta.cos() + nx, theta.sin() + ny));
        }
    }
    let mut ctrls: Vec<Vec<[f64; 3]>> = vec![];
    for i in 0..n {
        let pp = i as f64 / (n - 1) as f64;
        let mut row = vec![];
        for j in 0..m {
            let theta = 2.0 * pi * j as f64 / m as f64;
            let scale = 1.0 - 0.2 * pp;
            let tr = -0.6 * pp;
            let down = 0.4 * pp * theta.cos();
            let mut c = [scale * points[j].0 + tr, scale * points[j].1, pp + down];
            c[0] *= sx;
            c[1] *= sy;
            c[2] *= sz;
            row.push(c);
        }
        ctrls.push(row);
    }
    let (pts, faces) = super::creature_nurbs::nurbs_mesh(&ctrls, 0.01);
    let pts32: Vec<[f32; 3]> = pts.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let ring: Vec<usize> = (0..pts32.len()).filter(|&i| pts32[i][2] == 0.0).collect();
    let mut pos: Vec<[f32; 3]> = ring.iter().map(|&i| pts32[i]).collect();
    let nring = pos.len() as u32;
    pos.extend_from_slice(&pts32);
    let mut fs: Vec<Vec<u32>> = vec![];
    if nring >= 3 {
        fs.push((0..nring).collect());
    }
    for f in &faces {
        fs.push(f.iter().map(|&i| i + nring).collect());
    }
    // origin to the top ring centre, then rotation_euler.y -= pi / 2 applied
    let top = ctrls[n - 1].iter().fold([0.0f64; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]);
    let top = [(top[0] / m as f64) as f32, (top[1] / m as f64) as f32, (top[2] / m as f64) as f32];
    let rm = q_to_m4(eul_to_q([0.0, -pi / 2.0, 0.0]));
    for q in pos.iter_mut() {
        let d = [(q[0] - top[0]) as f64, (q[1] - top[1]) as f64, (q[2] - top[2]) as f64];
        let o = xf_point(&rm, d);
        *q = [o[0] as f32, o[1] as f32, o[2] as f32];
    }
    CMesh { pos, faces: fs }
}

#[allow(unused)]
fn _unused() -> J {
    json!(null)
}
