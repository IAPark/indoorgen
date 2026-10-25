//! Asset data types and the Factory trait (shared by every asset module); the class registry is in mod.rs.
use crate::geom::*;
use crate::geom::{Assets, PlaceholderKind};
use crate::int_hash;
use crate::mesh::*;
use crate::rng::Rng;
use crate::materials::*;
use super::cabnodes;

#[derive(Clone)]
pub struct Part {
    pub mesh: Mesh,
    pub mat: Material,
    /// axis aligned collision box (local space)
    pub col: Option<(V3, V3)>,
}

/// A cloth simulation the Rust solver approximated: the exact pre-sim mesh, colliders (asset parts, read in their *current* state when
/// the job runs) and the python `cloth_sim` keyword arguments, so a Blender run (render_rust.py / tools/cloth_jobs.py) can redo it exactly.
#[derive(Clone, Debug)]
pub struct ClothJob {
    /// part receiving the simulated (and subsurfed) mesh
    pub target: usize,
    pub pre: Mesh,
    pub collide: Vec<usize>,
    /// bake_cloth kwargs as python passes them (mass, tension_stiffness, gravity, use_pressure, uniform_pressure_force, ...)
    pub kwargs: serde_json::Value,
    pub end_frame: u32,
    pub subsurf: usize,
    /// vertices of the "pin" vertex group (vertex_group_mass="pin")
    pub pin: Vec<u32>,
    /// pre mesh was placed at z = top of `.0` (max z) : `.1` is the value Rust used; Blender shifts by (new top - used)
    pub z_from: Option<(usize, f64)>,
    /// build the pre mesh in Blender with python's own factory code instead of importing `pre` (exact edge/loop order => reproducible sims)
    pub build: Option<serde_json::Value>,
}

/// object resting on cloth/bed surfaces: Blender re-drops it by the change of the supporting surface height at (x, y)
#[derive(Clone, Debug)]
pub struct Follower {
    pub part: usize,
    pub xy: [f64; 2],
    pub z_used: f64,
    pub bases: Vec<usize>,
}

#[derive(Clone, Default)]
pub struct Asset {
    pub cloth: Vec<ClothJob>,
    pub followers: Vec<Follower>,
    pub parts: Vec<Part>,
    /// top faces of these boxes carry the SupportSurface tag
    pub supports: Vec<(V3, V3)>,
}

impl Asset {
    /// cloth jobs and followers as sidecar JSON; part `i` is named `{prefix}#{i}`; `m` places the asset in the world (z-rotation + translation)
    pub fn cloth_json(&self, prefix: &str, m: &Mat4) -> Vec<serde_json::Value> {
        use serde_json::json;
        let nm = |i: usize| format!("{prefix}#{i}");
        let zt = m.0[2][3];
        let mut out = vec![];
        for j in &self.cloth {
            let mut pre = j.pre.clone();
            pre.transform(m);
            let verts: Vec<_> = pre.pos.iter().map(|p| json!([p[0], p[1], p[2]])).collect();
            out.push(json!({
                "kind": "cloth", "target": nm(j.target), "verts": verts, "faces": pre.faces,
                "collide": j.collide.iter().map(|&i| nm(i)).collect::<Vec<_>>(), "kwargs": j.kwargs, "end_frame": j.end_frame,
                "subsurf": j.subsurf, "pin": j.pin, "build": j.build, "pose": m.0,
                "z_from": j.z_from.map(|(p, z)| json!({"part": nm(p), "z_used": z + zt})),
            }));
        }
        for f in &self.followers {
            let mut v = [f.xy[0], f.xy[1], 0.0];
            let q = m.apply(v);
            v = q;
            out.push(json!({"kind": "follow", "part": nm(f.part), "xy": [v[0], v[1]], "z_used": f.z_used + zt, "bases": f.bases.iter().map(|&i| nm(i)).collect::<Vec<_>>()}));
        }
        out
    }
    pub fn cuboid(&mut self, lo: V3, hi: V3, mat: &Material) {
        self.parts.push(Part { mesh: Mesh::cuboid(lo, hi), mat: mat.clone(), col: Some((lo, hi)) });
    }
    /// board whose top face is a support surface
    pub fn board(&mut self, lo: V3, hi: V3, mat: &Material) {
        self.cuboid(lo, hi, mat);
        self.supports.push((lo, hi));
    }
    pub fn cylinder(&mut self, cx: f64, cy: f64, z0: f64, z1: f64, r: f64, mat: &Material) {
        self.parts.push(Part { mesh: Mesh::cylinder(cx, cy, z0, z1, r, 20), mat: mat.clone(), col: Some(([cx - r, cy - r, z0], [cx + r, cy + r, z1])) });
    }
    pub fn frustum(&mut self, cx: f64, cy: f64, z0: f64, z1: f64, r0: f64, r1: f64, mat: &Material) {
        let r = r0.max(r1);
        self.parts.push(Part { mesh: Mesh::frustum(cx, cy, z0, z1, r0, r1, 20), mat: mat.clone(), col: Some(([cx - r, cy - r, z0], [cx + r, cy + r, z1])) });
    }
    pub fn mesh(&mut self, mesh: Mesh, mat: &Material, col: Option<(V3, V3)>) {
        self.parts.push(Part { mesh, mat: mat.clone(), col });
    }
    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &self.parts {
            let (a, b) = p.mesh.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        (lo, hi)
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in self.parts.iter_mut() {
            p.mesh.transform(m);
            if let Some((lo, hi)) = p.col {
                let a = m.apply(lo);
                let b = m.apply(hi);
                p.col = Some(([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]));
            }
        }
        for s in self.supports.iter_mut() {
            let a = m.apply(s.0);
            let b = m.apply(s.1);
            *s = ([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]);
        }
    }

    /// geometry for the solver when the real asset acts as placeholder: board faces + canonical + support tags, boxes as proxies
    pub fn solver_geom(&self) -> ObjGeom {
        let (lo, hi) = self.bounds();
        let mut tris: Vec<Tri> = vec![];
        let mut proxies = vec![];
        for p in &self.parts {
            if let Some((a, b)) = p.col {
                tris.extend(box_tris(a, b, 0));
                proxies.push((Proxy::Box { center: [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.], half: [(b[0] - a[0]) / 2., (b[1] - a[1]) / 2., (b[2] - a[2]) / 2.] }, u32::MAX));
            }
        }
        canonical_tag(&mut tris, lo, hi);
        // support surfaces: upward triangles of support boards
        for (a, b) in &self.supports {
            for mut t in box_tris(*a, *b, 0) {
                let u = sub(t.v[1], t.v[0]);
                let v = sub(t.v[2], t.v[0]);
                if cross(u, v)[2] > 1e-12 {
                    t.tags = SUPPORT;
                    tris.push(t);
                }
            }
        }
        ObjGeom::from_tris(tris, proxies)
    }

    /// python's box placeholder layout (8 verts, 12 tris) for the bounds of this asset: see ObjGeom::cuboid
    pub fn bbox_exact(&self) -> cabnodes::ExactMesh {
        let (lo, hi) = self.bounds();
        cabnodes::box_exact(lo, hi)
    }

    pub fn bbox_geom(&self) -> ObjGeom {
        let (lo, hi) = self.bounds();
        crate::geom::cuboid_py(f32v(lo), f32v(hi), 0, 0)
    }

    /// bbox_mesh_from_hipoly(use_pholder=True): box_from_corners (the node-group cube; vertex/face layout variant 1)
    pub fn bbox_geom_corners(&self) -> ObjGeom {
        let (lo, hi) = self.bounds();
        box_from_corners_geom(lo, hi)
    }
}

fn f32v(v: V3) -> V3 {
    [v[0] as f32 as f64, v[1] as f32 as f64, v[2] as f32 as f64]
}

/// python bbox_from_mesh.box_from_corners in float32: cube of size (max-min), translated by mix(min, max, 0.5); vertex order x fastest
pub fn box_from_corners_verts(lo: V3, hi: V3) -> [V3; 8] {
    let (lo, hi) = ([lo[0] as f32, lo[1] as f32, lo[2] as f32], [hi[0] as f32, hi[1] as f32, hi[2] as f32]);
    let size: [f32; 3] = std::array::from_fn(|k| hi[k] - lo[k]);
    let half: [f32; 3] = std::array::from_fn(|k| size[k] * 0.5);
    let c: [f32; 3] = std::array::from_fn(|k| lo[k] * 0.5 + hi[k] * 0.5);
    std::array::from_fn(|i| std::array::from_fn(|k| {
        let s = if (i >> k) & 1 == 0 { -half[k] } else { half[k] };
        (s + c[k]) as f64
    }))
}

/// box_from_corners mesh as the solver sees it: nodegroup cube, quads_convert_to_tris(BEAUTY) and canonical tags; vertex/face order exact
pub fn box_from_corners_exact(lo: V3, hi: V3) -> cabnodes::ExactMesh {
    let verts: Vec<V3> = box_from_corners_verts(lo, hi).to_vec();
    let polys = triangulate_quads_beauty(&verts, &CUBE_QUADS_XFAST);
    let names = canonical_names(&verts, &polys, &[]);
    cabnodes::ExactMesh { verts, polys, names }
}

pub fn box_from_corners_geom(lo: V3, hi: V3) -> ObjGeom {
    exact_geom(&box_from_corners_exact(lo, hi))
}

pub trait Factory {
    /// create_asset (final, detailed asset)
    fn asset(&self, inst_seed: u64) -> Asset;
    /// create_placeholder override, if the class defines one
    fn placeholder(&self, _inst_seed: u64) -> Option<Asset> {
        None
    }
    /// bounds of the final asset when the full mesh is not ported (used for bbox-of-asset placeholders)
    fn bounds_hint(&self, _inst_seed: u64) -> Option<(V3, V3)> {
        None
    }
    /// vertex-order-exact python placeholder mesh when it is not a plain box (e.g. KitchenSpace: two joined tagged boxes)
    fn placeholder_exact(&self, _inst_seed: u64) -> Option<cabnodes::ExactMesh> {
        None
    }
    /// vertex-order-exact python mesh of spawn_asset(inst) (float32 values), when the class is ported at topology level
    fn exact(&self, _inst_seed: u64) -> Option<cabnodes::ExactMesh> {
        None
    }
}

/// bounding box placeholder of the final asset (uses the float32 exact mesh when the class is ported at topology level)
pub fn bbox_of_asset(f: &dyn Factory, inst: u64) -> ObjGeom {
    if let Some((lo, hi)) = f.bounds_hint(inst) {
        return box_from_corners_geom(lo, hi);
    }
    if let Some(e) = f.exact(inst) {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &e.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        return box_from_corners_geom(lo, hi);
    }
    let (lo, hi) = f.asset(inst).bounds();
    box_from_corners_geom(lo, hi)
}

/// The asset scenes should use: the exact (python-matching) geometry when the class has one behind `Factory::exact`,
/// with each exact face given the material of the nearest part of the approximate `asset()` (which carries the
/// material choices); otherwise `asset()` itself.
pub fn scene_asset(f: &dyn Factory, inst: u64) -> Asset {
    let approx = f.asset(inst);
    let Some(e) = f.exact(inst) else { return approx };
    if approx.parts.is_empty() || e.polys.is_empty() {
        return approx;
    }
    let boxes: Vec<(V3, V3)> = approx.parts.iter().map(|p| p.mesh.bounds()).collect();
    let dist = |c: V3, b: &(V3, V3)| -> f64 {
        let mut d = 0.0;
        for k in 0..3 {
            let x = (b.0[k] - c[k]).max(0.0).max(c[k] - b.1[k]);
            d += x * x;
        }
        d
    };
    let mut meshes: Vec<Mesh> = approx.parts.iter().map(|_| Mesh::new()).collect();
    let mut maps: Vec<crate::dhash::HashMap<u32, u32>> = approx.parts.iter().map(|_| crate::dhash::HashMap::default()).collect();
    for poly in &e.polys {
        let n = poly.len() as f64;
        let mut c = [0.0; 3];
        for &i in poly {
            for k in 0..3 {
                c[k] += e.verts[i as usize][k] / n;
            }
        }
        let best = (0..boxes.len()).min_by(|&a, &b| dist(c, &boxes[a]).partial_cmp(&dist(c, &boxes[b])).unwrap()).unwrap();
        let m = &mut meshes[best];
        let map = &mut maps[best];
        let face: Vec<u32> = poly.iter().map(|&i| *map.entry(i).or_insert_with(|| {
            m.pos.push(e.verts[i as usize]);
            (m.pos.len() - 1) as u32
        })).collect();
        m.faces.push(face);
    }
    let mut out = Asset { parts: vec![], supports: approx.supports.clone(), ..Default::default() };
    for (k, m) in meshes.into_iter().enumerate() {
        if !m.faces.is_empty() {
            out.parts.push(Part { mesh: m, mat: approx.parts[k].mat.clone(), col: approx.parts[k].col });
        }
    }
    out
}

pub fn inst_rng(factory_seed: u64, inst_seed: u64) -> Rng {
    Rng::seeded(int_hash(&[&factory_seed.to_string(), &inst_seed.to_string()]))
}

pub fn mat(name: &str, rgb: [f32; 3], rough: f32) -> Material {
    Material::diffuse(name, rgb, rough)
}


/// GEN2PROF=1: accumulated wall time per (entry point, class); printed by `prof_report`
static PROF: std::sync::Mutex<Vec<(String, f64, u32)>> = std::sync::Mutex::new(Vec::new());

pub fn prof_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GEN2PROF").is_some())
}

/// CPU seconds of the calling thread (immune to machine load; 0 off macOS/linux)
pub fn thread_cpu() -> f64 {
    #[repr(C)]
    struct Ts {
        s: i64,
        n: i64,
    }
    extern "C" {
        fn clock_gettime(id: i32, ts: *mut Ts) -> i32;
    }
    #[cfg(target_os = "macos")]
    const ID: i32 = 16;
    #[cfg(not(target_os = "macos"))]
    const ID: i32 = 3;
    let mut t = Ts { s: 0, n: 0 };
    unsafe { clock_gettime(ID, &mut t) };
    t.s as f64 + t.n as f64 * 1e-9
}

pub fn prof_add(label: String, secs: f64) {
    let mut p = PROF.lock().unwrap();
    if let Some(e) = p.iter_mut().find(|e| e.0 == label) {
        e.1 += secs;
        e.2 += 1;
    } else {
        p.push((label, secs, 1));
    }
}

pub fn prof_report() {
    let mut p = PROF.lock().unwrap().clone();
    p.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (l, s, n) in p.iter().take(40) {
        eprintln!("PROF {s:8.3}s {n:5}x  {l}");
    }
}

/// python meshes are float32: round every coordinate (and the bounds) to f32
pub fn round_geom_f32(mut g: ObjGeom) -> ObjGeom {
    let r = |v: &mut V3| {
        for k in 0..3 {
            v[k] = v[k] as f32 as f64;
        }
    };
    for t in g.tris.iter_mut() {
        for v in t.v.iter_mut() {
            r(v);
        }
    }
    r(&mut g.lo);
    r(&mut g.hi);
    g
}

/// tagging.tag_canonical_surfaces on a triangle mesh: per face the "back.front.top.bottom" names (min/max x, max/min z, all vertices within
/// isclose(atol = 0.01 * dimension)); `extra[i]` is appended (generator tags such as "support").
pub fn canonical_names(verts: &[V3], polys: &[Vec<u32>], extra: &[String]) -> Vec<String> {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let close = |a: f64, b: f64, axis: usize| (a - b).abs() <= 0.01 * (hi[axis] - lo[axis]) + 1e-5 * b.abs();
    let defs: [(&str, usize, bool); 4] = [("back", 0, false), ("front", 0, true), ("top", 2, true), ("bottom", 2, false)];
    polys
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut parts: Vec<String> = vec![];
            for (name, axis, is_max) in defs {
                let t = if is_max { hi[axis] } else { lo[axis] };
                if p.iter().all(|&v| close(verts[v as usize][axis], t, axis)) {
                    parts.push(name.to_string());
                }
            }
            if let Some(e) = extra.get(i) {
                for s in e.split('.').filter(|s| !s.is_empty()) {
                    parts.push(s.to_string());
                }
            }
            parts.join(".")
        })
        .collect()
}

/// solver geometry for a vertex-order-exact python placeholder mesh (triangles, python face names); one box proxy per connected component
pub fn exact_geom(e: &cabnodes::ExactMesh) -> ObjGeom {
    let tags_of = |n: &str| -> u32 {
        n.split('.')
            .map(|s| match s {
                "back" => BACK,
                "front" => FRONT,
                "top" => TOP,
                "bottom" => BOTTOM,
                "side" => SIDE,
                "support" => SUPPORT,
                _ => 0,
            })
            .fold(0, |a, b| a | b)
    };
    let mut tris = vec![];
    for (p, n) in e.polys.iter().zip(e.names.iter()) {
        for k in 1..p.len() - 1 {
            tris.push(Tri { v: [e.verts[p[0] as usize], e.verts[p[k] as usize], e.verts[p[k + 1] as usize]], tags: tags_of(n) });
        }
    }
    // connected components of the vertex graph
    let mut comp: Vec<usize> = (0..e.verts.len()).collect();
    fn find(c: &mut Vec<usize>, a: usize) -> usize {
        let mut r = a;
        while c[r] != r {
            r = c[r];
        }
        let mut a = a;
        while c[a] != r {
            let n = c[a];
            c[a] = r;
            a = n;
        }
        r
    }
    for p in &e.polys {
        for w in p.windows(2) {
            let (a, b) = (find(&mut comp, w[0] as usize), find(&mut comp, w[1] as usize));
            comp[a] = b;
        }
    }
    let mut boxes: Vec<(usize, V3, V3)> = vec![];
    for i in 0..e.verts.len() {
        let r = find(&mut comp, i);
        match boxes.iter_mut().find(|b| b.0 == r) {
            Some(b) => {
                for k in 0..3 {
                    b.1[k] = b.1[k].min(e.verts[i][k]);
                    b.2[k] = b.2[k].max(e.verts[i][k]);
                }
            }
            None => boxes.push((r, e.verts[i], e.verts[i])),
        }
    }
    let proxies = boxes.iter().map(|(_, a, b)| (Proxy::Box { center: [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.], half: [(b[0] - a[0]) / 2., (b[1] - a[1]) / 2., (b[2] - a[2]) / 2.] }, u32::MAX)).collect();
    ObjGeom::from_tris(tris, proxies).with_mesh_verts(e.verts.clone())
}

/// python `obj.rotation_euler.z += pi/2; butil.apply_transform(obj)` of a mesh (float32 mul_m4_v3 of the rotation matrix)
pub fn rot_z90_f32(verts: &[V3]) -> Vec<V3> {
    let m = crate::bl32::eul_to_mat3([0.0, 0.0, (std::f64::consts::FRAC_PI_2) as f32]);
    verts
        .iter()
        .map(|v| {
            let (x, y, z) = (v[0] as f32, v[1] as f32, v[2] as f32);
            let r = |row: usize| -> f64 { (x * m[0][row] + y * m[1][row] + z * m[2][row]) as f64 };
            [r(0), r(1), r(2)]
        })
        .collect()
}

/// quads of the blender cube primitive in `new_bbox_exact` vertex order (i = 4x + 2y + z), in blender's face order
pub const CUBE_QUADS_ZFAST: [[u32; 4]; 6] = [[0, 1, 3, 2], [2, 3, 7, 6], [6, 7, 5, 4], [4, 5, 1, 0], [2, 6, 4, 0], [7, 3, 1, 5]];

/// triangulate quads like the python placeholder pipeline (BM_face_triangulate, quad method BEAUTY, float32): all first triangles in place,
/// then the second triangles appended
pub fn triangulate_quads_beauty(verts: &[V3], quads: &[[u32; 4]]) -> Vec<Vec<u32>> {
    let mut first = vec![];
    let mut second = vec![];
    for q in quads {
        let (a, b, c, d) = (q[0], q[1], q[2], q[3]);
        let p = |i: u32| -> [f32; 3] { let v = verts[i as usize]; [v[0] as f32, v[1] as f32, v[2] as f32] };
        if crate::polyfill::quad_split_24_beauty([p(a), p(b), p(c), p(d)]) {
            first.push(vec![a, c, d]);
            second.push(vec![a, b, c]);
        } else {
            first.push(vec![b, d, a]);
            second.push(vec![b, c, d]);
        }
    }
    first.extend(second);
    first
}

/// quads of the geometry-nodes cube (mesh cube node) in `box_exact` vertex order (i = x + 2y + 4z), in its face order
pub const CUBE_QUADS_XFAST: [[u32; 4]; 6] = [[0, 2, 3, 1], [0, 1, 5, 4], [4, 5, 7, 6], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
