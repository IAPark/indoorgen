//! cactus/*.py (CactusFactory: globular / columnar / prickly pear bodies + spikes) on the exported geometry-node groups (data/cactus_gn.json):
//! body graph -> geo_extension (2D) -> voxel REMESH + nearest-surface transfer -> DISPLACE -> spikes (5 tree meshes through geo_radius, instanced by geo_spikes).
//! RNG draws follow the python order (graph construction draws are patched into the exported node values).
use super::gnedit::{node_mut, set_in};
use super::leafkit::LMesh;
use super::legtex::{Tex, TexKind};
use super::shell::QuadSpline;
use super::*;
use crate::bl32::Obj32;
use crate::gn;
use crate::gn::geom::{Attr, Dom, Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::{Ty, Val};
use crate::mesh::Mesh;
use crate::rng::Rng;
use serde_json::{json, Value as J};
use std::f64::consts::PI;
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    c == "CactusFactory"
}

type V = [f64; 3];

fn tpl() -> &'static J {
    static T: OnceLock<J> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../data/cactus_gn.json")).unwrap())
}

// ------------------------------------------------------------------------------------------------ node graph plumbing

/// bind a modifier input socket to a point attribute (surface.add_geomod input_attributes): replace the group input by a Named Attribute node
fn bind_attr(j: &mut J, group: &str, socket: &str, name: &str) {
    let g = &mut j["groups"][group];
    let node = json!({
        "name": "__attr", "type": "GeometryNodeInputNamedAttribute", "p": {"data_type": "FLOAT"},
        "in": [{"id": "Name", "name": "Name", "t": "STRING", "v": name}],
        "out": [{"id": "Attribute", "name": "Attribute", "t": "FLOAT"}, {"id": "Exists", "name": "Exists", "t": "BOOLEAN"}]
    });
    g["nodes"].as_array_mut().unwrap().push(node);
    for l in g["links"].as_array_mut().unwrap() {
        if l["fn"].as_str() == Some("Group Input") && l["fs"].as_str() == Some(socket) {
            l["fn"] = json!("__attr");
            l["fs"] = json!("Attribute");
        }
    }
}

fn gmesh(pos: &[[f32; 3]], faces: &[Vec<u32>], edges: &[[u32; 2]]) -> GMeshT {
    GMeshT::from_faces(pos.to_vec(), faces, edges)
}

fn set_point_attr(m: &mut GMeshT, name: &str, ty: Ty, data: Vec<Val>) {
    m.attrs.insert(name.to_string(), Attr { dom: Dom::Point, ty, data });
}

/// evaluate `top` of the (patched) groups on `geo`; `out_attrs` names non-geometry outputs to store as attributes; instances are realized
fn run(j: J, top: &str, geo: GGeom, objects: &gn::Objects, out_attrs: Option<J>) -> GGeom {
    let groups = gn::load::load_groups(j);
    let g = groups.map.get(top).unwrap();
    let mut inputs: crate::dhash::HashMap<String, gn::Value> = crate::dhash::HashMap::default();
    let gi = g.inputs.iter().find(|i| i.ty == Ty::Geo).unwrap();
    inputs.insert(gi.id.clone(), gn::Value::geo(geo));
    let outs = gn::eval(&groups, top, &inputs, objects).unwrap_or_else(|e| panic!("{top}: {e}"));
    let oi = g.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
    match &outs[oi].1 {
        gn::Value::G(gg) => {
            let geo = match &out_attrs {
                Some(a) => gn::apply_out_attrs(g, &outs, (**gg).clone(), a),
                None => (**gg).clone(),
            };
            gn::meshops::realize_instances(&geo, 1000)
        }
        _ => panic!("{top}: no geometry output"),
    }
}

fn dummy_cube() -> GGeom {
    GGeom::from_mesh(gmesh(&[[0.0; 3]], &[], &[]))
}

/// geo_extension (musgrave 2D) with the three draws of decorate.geo_extension
fn geo_extension_2d(geo: GGeom, r: &mut Rng) -> GGeom {
    let strength = r.uniform(0.1, 0.2);
    let scale = r.uniform(1.4, 2.8);
    let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
    let mut j = tpl().clone();
    set_in(&mut j, "geo_extension", "Vector Math.002", "Vector_001", json!(off));
    set_in(&mut j, "geo_extension", "Noise Texture", "Scale", json!(scale));
    set_in(&mut j, "geo_extension", "Math.002", "Value_001", json!(strength));
    run(j, "geo_extension", geo, &gn::Objects::default(), None)
}

// ------------------------------------------------------------------------------------------------ trees (trees/tree.py build_radius_tree)

#[derive(Clone, Default)]
struct PathK {
    n_pts: usize,
    sz: f64,
    std: f64,
    momentum: f64,
    pull_dir: Option<V>,
    pull_init: f64,
    pull_factor: f64,
}

#[derive(Clone, Default)]
struct SpawnK {
    rng: Option<[f64; 2]>,
    ang_min: Option<f64>,
    ang_max: Option<f64>,
    axis2: Option<V>,
    init_vec: Option<V>,
}

struct Branch {
    n: usize,
    path: Box<dyn Fn(&mut Rng, usize) -> PathK>,
    spawn: Box<dyn Fn(&mut Rng, usize) -> SpawnK>,
    children: Vec<Branch>,
}

fn vsub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vadd(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn vmul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn vdot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn vcross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn vlen(a: V) -> f64 {
    vdot(a, a).sqrt()
}
fn vdiv(a: V, s: f64) -> V {
    [a[0] / s, a[1] / s, a[2] / s]
}

/// helper.rodrigues_rot
fn rodrigues(vec: V, axis: V, angle: f64) -> V {
    let axis = vdiv(axis, vlen(axis));
    let (cs, sn) = (angle.cos(), angle.sin());
    let a = vmul(vec, cs);
    let b = vmul(vcross(axis, vec), sn);
    let c = vmul(vmul(axis, vdot(axis, vec)), 1.0 - cs);
    vadd(vadd(a, b), c)
}

fn randn3(r: &mut Rng) -> V {
    [r.randn(), r.randn(), r.randn()]
}

/// trees/tree.py rand_path
fn rand_path(r: &mut Rng, k: &PathK, init_vec: V, init_pt: V) -> Vec<V> {
    let mut iv = init_vec;
    if let Some(p) = k.pull_dir {
        iv = vadd(iv, vmul(p, k.pull_init));
    }
    iv = vdiv(iv, vlen(iv));
    let mut path = vec![[0.0; 3]; k.n_pts];
    path[0] = init_pt;
    for i in 1..k.n_pts {
        let prev_delta = if i == 1 { vmul(iv, k.sz) } else { vsub(path[i - 1], path[i - 2]) };
        let prev_sz = vlen(prev_delta);
        let mut nd = vadd(prev_delta, vmul(randn3(r), k.std));
        if let Some(p) = k.pull_dir {
            nd = vadd(nd, vmul(p, k.pull_factor));
        }
        nd = vmul(vdiv(nd, vlen(nd)), prev_sz);
        let tm = 1.0 - (1.0 - k.momentum) * (i as f64 + 1.0) / k.n_pts as f64;
        let delta = vadd(vmul(prev_delta, tm), vmul(nd, 1.0 - tm));
        let delta = vmul(vdiv(delta, vlen(delta)), k.sz);
        path[i] = vadd(path[i - 1], delta);
    }
    path
}

/// trees/tree.py get_spawn_pt -> (index into path, point, init_vec)
fn get_spawn_pt(r: &mut Rng, path: &[V], s: &SpawnK) -> (usize, V, V) {
    let n = path.len();
    if n == 1 {
        return (0, path[0], s.init_vec.unwrap());
    }
    let rng = s.rng.unwrap_or([0.5, 1.0]);
    let idx = r.randint((n as f64 * rng[0]) as i64, (n as f64 * rng[1]) as i64) as usize;
    let init_vec = match s.init_vec {
        Some(v) => v,
        None => {
            let cv = vsub(path[idx], path[idx - 1]);
            let axis1 = [cv[1], -cv[0], 0.0];
            let axis2 = s.axis2.unwrap_or_else(|| rodrigues(cv, axis1, PI / 2.0));
            let (amin, amax) = (s.ang_min.unwrap_or(PI / 6.0), s.ang_max.unwrap_or(0.9 * PI / 2.0));
            let mut ang = r.f64() * (amax - amin) + amin;
            let sign = if r.randn() > 0.0 { 1.0 } else { -1.0 };
            ang *= sign;
            rodrigues(cv, axis2, ang)
        }
    };
    (idx, path[idx], init_vec)
}

struct FineTree {
    vtxs: Vec<V>,
    resolution: usize,
    detailed: Vec<V>,
    radius: Vec<f64>,
    detailed_parents: Vec<i64>,
    radius_fn: Box<dyn FnMut(&mut Rng, f64, usize, usize) -> Vec<f64>>,
}

impl FineTree {
    fn append(&mut self, r: &mut Rng, v: &[V], p0: usize) {
        // interp1d(arange(n+1), [vtxs[p0]] + v, kind='quadratic') at linspace(0, n, n*res+1)[1:]
        let n = v.len();
        let xs: Vec<f64> = (0..=n).map(|i| i as f64).collect();
        let mut pts = vec![self.vtxs[p0]];
        pts.extend_from_slice(v);
        self.vtxs.extend_from_slice(v);
        let sp: Vec<QuadSpline> = (0..3).map(|c| QuadSpline::new(&xs, &pts.iter().map(|p| p[c]).collect::<Vec<_>>())).collect();
        let num = n * self.resolution + 1;
        let step = n as f64 / (num - 1) as f64;
        for i in 1..num {
            let t = if i == num - 1 { n as f64 } else { i as f64 * step };
            self.detailed.push([sp[0].eval(t), sp[1].eval(t), sp[2].eval(t)]);
        }
        let base = self.radius[p0 * self.resolution];
        let rad = (self.radius_fn)(r, base, n, self.resolution);
        self.radius.extend(rad);
        self.detailed_parents.push((p0 * self.resolution) as i64);
        let len0 = self.detailed_parents.len() as i64;
        for k in 0..(n * self.resolution - 1) as i64 {
            self.detailed_parents.push(k + len0 - 1);
        }
    }
}

fn recursive_path(r: &mut Rng, tree: &mut FineTree, parent_idxs: &[usize], b: &Branch) {
    for idx in 0..b.n {
        let k = (b.path)(r, idx);
        let s = (b.spawn)(r, idx);
        let pv: Vec<V> = parent_idxs.iter().map(|&i| tree.vtxs[i]).collect();
        let (pi, init_pt, init_vec) = get_spawn_pt(r, &pv, &s);
        let parent_idx = parent_idxs[pi];
        let path = rand_path(r, &k, init_vec, init_pt);
        let new_vtxs = &path[1..];
        let start = tree.vtxs.len();
        let mut node_idxs = vec![parent_idx];
        node_idxs.extend(start..start + new_vtxs.len());
        tree.append(r, new_vtxs, node_idxs[0]);
        for c in &b.children {
            recursive_path(r, tree, &node_idxs, c);
        }
    }
}

type RadiusFn = Box<dyn FnMut(&mut Rng, f64, usize, usize) -> Vec<f64>>;

/// build_radius_tree: (detailed locations, edges (child, parent), radius per vertex)
fn build_radius_tree(r: &mut Rng, radius_fn: RadiusFn, cfg: &Branch, resolution: usize, fix_first: bool) -> (Vec<V>, Vec<[u32; 2]>, Vec<f64>) {
    let mut t = FineTree { vtxs: vec![[0.0; 3]], resolution, detailed: vec![[0.0; 3]], radius: vec![1.0], detailed_parents: vec![-1], radius_fn };
    recursive_path(r, &mut t, &[0], cfg);
    if fix_first {
        t.radius[0] = t.radius[1];
    }
    let edges: Vec<[u32; 2]> = (0..t.detailed.len()).filter(|&i| t.detailed_parents[i] != -1).map(|i| [i as u32, t.detailed_parents[i] as u32]).collect();
    (t.detailed, edges, t.radius)
}

// ------------------------------------------------------------------------------------------------ spikes

fn sample_direction(r: &mut Rng, min_z: f64) -> V {
    for _ in 0..100 {
        let x = randn3(r);
        let y = vdiv(x, vlen(x));
        if y[2] > min_z {
            return y;
        }
    }
    [0.0, 0.0, 1.0]
}

/// spike.build_spikes + geo_radius
fn build_spike(r: &mut Rng, base_radius: f64) -> GMeshT {
    let cfg = Branch {
        n: 4,
        path: Box::new(|r, _| PathK { n_pts: 9, std: 0.5, momentum: 0.85, sz: r.uniform(0.005, 0.01), pull_init: 1.0, ..Default::default() }),
        spawn: Box::new(|r, _| SpawnK { init_vec: Some(sample_direction(r, 0.8)), ..Default::default() }),
        children: vec![],
    };
    let rf: RadiusFn = Box::new(|_, base, size, res| (0..size * res).map(|i| base * 0.5f64.powf(i as f64 / (size * res) as f64)).collect());
    let (verts, edges, radius) = build_radius_tree(r, rf, &cfg, 1, false);
    let pos: Vec<[f32; 3]> = verts.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut m = gmesh(&pos, &[], &edges);
    set_point_attr(&mut m, "rad", Ty::F, radius.iter().map(|&x| Val::F((base_radius * x) as f32)).collect());
    let mut j = tpl().clone();
    bind_attr(&mut j, "geo_radius", "Socket_2", "rad");
    if ig_kit::api::dense() {
        // a 2 mm spike does not need a 32-gon cross section: 4 sides
        set_in(&mut j, "geo_radius", "Curve Circle", "Resolution", json!(std::env::var("CACTUS_SPIKE_RES").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(4)));
    }
    let g = run(j, "geo_radius", GGeom::from_mesh(m), &gn::Objects::default(), None);
    let o = g.mesh.unwrap();
    static SPK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let k = SPK.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    dump(&format!("spike{k}"), &o.pos, &o.faces_vec());
    o
}

// ------------------------------------------------------------------------------------------------ bodies

struct Body {
    mesh: GMeshT,
    /// "selection" point attribute
    selection: Vec<f32>,
}

fn set_float_curve(j: &mut J, group: &str, node: &str, pts: &[(f64, f64)]) {
    node_mut(j, group, node)["p"]["curve"]["pts"] = J::Array(pts.iter().map(|&(x, y)| json!([x, y, "AUTO"])).collect());
}

fn selection_of(m: &GMeshT) -> Vec<f32> {
    match m.attrs.get("selection") {
        Some(a) => a.data.iter().map(|v| match v { Val::B(b) => *b as i32 as f32, Val::F(f) => *f, _ => 0.0 }).collect(),
        None => vec![1.0; m.pos.len()],
    }
}

fn globular(r: &mut Rng) -> Body {
    let star = r.randint(6, 12);
    let frequency = r.uniform(-0.2, 0.2);
    let ridge = r.uniform(1.1, 1.2);
    let a1 = r.uniform(0.2, 0.4);
    let a2 = (r.uniform(0.4, 0.6), r.log_uniform(0.5, 0.8));
    let a3 = (r.uniform(0.8, 0.85), r.uniform(0.4, 0.6));
    let mult = r.log_uniform(0.5, 1.0);
    let mut j = tpl().clone();
    set_in(&mut j, "geo_globular", "Mesh Circle", "Vertices", json!(star * 3));
    set_in(&mut j, "geo_globular", "Vector Math", "Scale", json!(ridge));
    set_float_curve(&mut j, "geo_globular", "Float Curve", &[(0.0, a1), a2, a3, (1.0, 0.05)]);
    set_in(&mut j, "geo_globular", "Math.001", "Value_001", json!(mult));
    set_in(&mut j, "geo_globular", "Math.002", "Value_001", json!(2.0 * PI * frequency));
    let oa = json!([{"id": "Socket_2", "name": "selection", "domain": "POINT"}]);
    let g = run(j, "geo_globular", dummy_cube(), &gn::Objects::default(), Some(oa));
    let g = geo_extension_2d(g, r);
    let mut m = g.mesh.unwrap();
    let sel = selection_of(&m);
    let s = [r.uniform(0.8, 1.5), r.uniform(0.8, 1.5), r.uniform(0.8, 1.5)];
    let rz = r.uniform(0.0, 2.0 * PI);
    apply_scale_rot(&mut m.pos, [s[0] as f32, s[1] as f32, s[2] as f32], [0.0, 0.0, rz as f32]);
    Body { mesh: m, selection: sel }
}

/// obj.scale / rotation_euler then butil.apply_transform (rotation and scale)
fn apply_scale_rot(pos: &mut [[f32; 3]], scale: [f32; 3], eul: [f32; 3]) {
    let mut o = Obj32::new([0.0; 3], eul);
    o.scale = scale;
    let m = o.matrix_world();
    for p in pos.iter_mut() {
        let (x, y, z) = (p[0], p[1], p[2]);
        *p = [x * m[0][0] + y * m[1][0] + z * m[2][0], x * m[0][1] + y * m[1][1] + z * m[2][1], x * m[0][2] + y * m[1][2] + z * m[2][2]];
    }
}

fn pear_leaf(r: &mut Rng) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    let a1 = r.uniform(0.15, 0.2);
    let a2 = (r.uniform(0.4, 0.6), r.log_uniform(0.4, 0.5));
    let mult = r.log_uniform(0.5, 1.5);
    let mut j = tpl().clone();
    set_float_curve(&mut j, "geo_leaf", "Float Curve", &[(0.0, a1), a2, (1.0, 0.05)]);
    set_in(&mut j, "geo_leaf", "Math", "Value_001", json!(mult));
    let g = run(j, "geo_leaf", dummy_cube(), &gn::Objects::default(), None);
    let g = geo_extension_2d(g, r);
    let m = g.mesh.unwrap();
    let mut pos = m.pos.clone();
    let s = [r.uniform(0.8, 1.2), r.uniform(0.2, 0.25), r.uniform(0.8, 1.2)];
    apply_scale_rot(&mut pos, [s[0] as f32, s[1] as f32, s[2] as f32], [0.0; 3]);
    (pos, m.faces_vec())
}

fn pear_leaves(r: &mut Rng, level: u32) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    if level == 0 {
        return pear_leaf(r);
    }
    let n = r.randint(1, 3) as usize;
    let leaves: Vec<_> = (0..n).map(|_| pear_leaves(r, level - 1)).collect();
    let (bpos, bfaces) = pear_leaf(r);
    let mut angles = vec![-r.uniform(PI / 3.0, PI / 2.0), r.uniform(-PI / 16.0, PI / 16.0), r.uniform(PI / 3.0, PI / 2.0)];
    r.shuffle(&mut angles);
    let angles = &angles[..n];
    let mut pos = bpos.clone();
    let mut faces = bfaces;
    for (a, (lpos, lfaces)) in angles.iter().zip(leaves) {
        let v = [a.sin(), 0.0, a.cos() + 0.5];
        let mut best = 0usize;
        let mut bd = f64::MIN;
        for (i, p) in bpos.iter().enumerate() {
            let d = p[0] as f64 * v[0] + p[1] as f64 * v[1] + p[2] as f64 * v[2];
            if d > bd {
                bd = d;
                best = i;
            }
        }
        // leaf.location[-1] -= 0.15 then apply; scale / location / rotation set, then join (matrix_world)
        let sc = r.uniform(0.5, 0.75);
        let rz = r.uniform(-PI / 3.0, PI / 3.0);
        let mut ob = Obj32::new(bpos[best], [0.0, *a as f32, rz as f32]);
        ob.scale = [sc as f32; 3];
        let m = ob.matrix_world();
        let off = pos.len() as u32;
        for p in &lpos {
            let (x, y, z) = (p[0], p[1], p[2] + (-0.15f32));
            pos.push([x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0], x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1], x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2]]);
        }
        for f in lfaces {
            faces.push(f.iter().map(|&i| i + off).collect());
        }
    }
    (pos, faces)
}

fn prickly(r: &mut Rng) -> Body {
    let (pos, faces) = pear_leaves(r, 2);
    let m = gmesh(&pos, &faces, &[]);
    let sel = vec![1.0; pos.len()];
    Body { mesh: m, selection: sel }
}

fn columnar(r: &mut Rng) -> Body {
    let resolution = 16usize;
    let base_radius = 0.25;
    // branch_config property
    let n_major = 16usize;
    let n_minor = r.randint(10, 14) as usize;
    let b_minor = r.randint(2, 4) as usize;
    let angles = loop {
        let a: Vec<f64> = (0..b_minor).map(|_| r.uniform(0.0, 2.0 * PI)).collect();
        let mut s = a.clone();
        s.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let ok = (0..b_minor).all(|i| {
            let nxt = if i + 1 < b_minor { s[i + 1] } else { s[0] + 2.0 * PI };
            nxt - s[i] > PI / 3.0
        });
        if ok {
            break a;
        }
    };
    let ang2 = angles.clone();
    let minor = Branch {
        n: b_minor,
        path: Box::new(move |_, _| PathK { n_pts: n_minor, std: 0.4, momentum: 0.1, sz: 0.2, pull_dir: Some([0.0, 0.0, 1.0]), pull_init: 0.0, pull_factor: 4.0 }),
        spawn: Box::new(move |_, idx| SpawnK { ang_min: Some(PI / 2.5), ang_max: Some(PI / 2.0), rng: Some([0.2, 0.6]), axis2: Some([ang2[idx].cos(), ang2[idx].sin(), 0.0]), init_vec: None }),
        children: vec![],
    };
    let major = Branch {
        n: 1,
        path: Box::new(move |_, _| PathK { n_pts: n_major, std: 0.4, momentum: 0.99, sz: 0.3, pull_init: 1.0, ..Default::default() }),
        spawn: Box::new(|_, _| SpawnK { init_vec: Some([0.0, 0.0, 1.0]), ..Default::default() }),
        children: vec![minor],
    };
    let rf: RadiusFn = Box::new(move |r, base, size, res| {
        let decay = r.uniform(0.5, 0.8);
        let root = r.uniform(0.7, 0.9);
        let alpha = r.uniform(2.0, 3.0);
        let total = size * res;
        let mut rad = vec![base * decay; total];
        for k in 0..res {
            rad[k] *= root.powf(1.0 - k as f64 / res as f64);
        }
        for k in 0..res {
            let idx = total - res + k;
            rad[idx] *= (1.0 - (k as f64 / res as f64).powf(alpha)).powf(1.0 / alpha);
        }
        rad
    });
    let (verts, edges, radius) = build_radius_tree(r, rf, &major, resolution, true);
    let pos: Vec<[f32; 3]> = verts.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut m = gmesh(&pos, &[], &edges);
    dump("tree", &pos, &[]);
    set_point_attr(&mut m, "rad", Ty::F, radius.iter().map(|&x| Val::F((base_radius * x) as f32)).collect());
    // geo_star draws (node construction order)
    let star = r.randint(5, 8);
    let seed_perturb = r.randint(0, 100000);
    let sc = [r.uniform(0.8, 1.0), r.uniform(0.8, 1.0)];
    let ridge = r.uniform(1.15, 1.25);
    let mut j = tpl().clone();
    bind_attr(&mut j, "geo_star", "Socket_2", "rad");
    set_in(&mut j, "geo_star", "Mesh Circle", "Vertices", json!(star * 3));
    set_in(&mut j, "geo_star", "Random Value", "Seed", json!(seed_perturb));
    set_in(&mut j, "geo_star", "Transform Geometry", "Scale", json!([sc[0], sc[1], 1.0]));
    set_in(&mut j, "geo_star", "Vector Math", "Scale", json!(ridge));
    // align_tilt(noise_strength=uniform(pi/4, pi/2))
    let noise_strength = r.uniform(PI / 4.0, PI / 2.0);
    set_in(&mut j, "geo_star", "Math.001", "Value", json!(noise_strength));
    let oa = json!([{"id": "Socket_3", "name": "selection", "domain": "POINT"}]);
    let g = run(j, "geo_star", GGeom::from_mesh(m), &gn::Objects::default(), Some(oa));
    let g = geo_extension_2d(g, r);
    let m = g.mesh.unwrap();
    let sel = selection_of(&m);
    Body { mesh: m, selection: sel }
}

// ------------------------------------------------------------------------------------------------ factory

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CKind {
    Globular,
    Columnar,
    PrickyPear,
}

pub struct CactusFactory {
    pub fs: u64,
    pub kind: CKind,
    pub spike_distance: f64,
}

fn dump(name: &str, pos: &[[f32; 3]], faces: &[Vec<u32>]) {
    if let Ok(d) = std::env::var("CACTUSDUMP") {
        let _ = std::fs::write(format!("{d}/{name}.json"), json!({"verts": pos, "polys": faces}).to_string());
    }
}

impl CactusFactory {
    pub fn new(fs: u64) -> CactusFactory {
        let mut r = Rng::seeded(fs);
        let kind = [CKind::Globular, CKind::Columnar, CKind::PrickyPear][r.choice_p(&[1.0, 1.0, 1.0])];
        CactusFactory { fs, kind, spike_distance: 0.08 }
    }

    /// CactusFactory.create_asset
    pub fn mesh(&self, inst: u64, face_size: f64) -> Mesh {
        let mut r = inst_rng(self.fs, inst);
        let body = match self.kind {
            CKind::Globular => globular(&mut r),
            CKind::Columnar => columnar(&mut r),
            CKind::PrickyPear => prickly(&mut r),
        };
        dump("body", &body.mesh.pos, &body.mesh.faces_vec());
        // remesh_with_attrs: voxel remesh, then transfer position / selection from the source surface
        let src = LMesh { v: body.mesh.pos.clone(), f: body.mesh.faces_vec(), e: vec![] };
        let tris = super::dualcon::corner_tris(&src.v, &src.f);
        let nets = super::voxelremesh::voxel_remesh(&src.v, &tris, face_size as f32);
        dump("remesh_raw", &nets.v, &nets.f);
        let mut v = nets.v.clone();
        let samp = super::leafkit::sample_nearest_surface(&src, &body.selection, &v);
        let mut sel: Vec<f32> = Vec::with_capacity(v.len());
        for (i, s) in samp.iter().enumerate() {
            v[i] = s.1;
            sel.push(if s.0 >= 0.5 { 1.0 } else { 0.0 });
        }
        dump("remesh", &v, &nets.f);
        // DISPLACE (noise_strength 0.02)
        let marble = r.choice_idx(2) == 1;
        let scale = r.log_uniform(0.1, 0.15);
        let kind = if marble { TexKind::Marble(0, 0) } else { TexKind::Stucci(0) };
        super::bpymod::displace(&mut v, &nets.f, Some(&Tex::new(kind, scale)), 0.02, 0.0, super::bpymod::DispDir::Normal, None);
        dump("displace", &v, &nets.f);
        let mut body_mesh = Mesh::new();
        body_mesh.pos = v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        body_mesh.faces = nets.f.clone();
        // spikes
        let spikes: Vec<GMeshT> = (0..5)
            .map(|_| {
                r.f64();
                build_spike(&mut r, 0.002)
            })
            .collect();
        let seeds: Vec<i64> = (0..5).map(|_| r.randint(0, 100000)).collect();
        let mut j = tpl().clone();
        bind_attr(&mut j, "geo_spikes", "Socket_2", "selection");
        for (n, s) in ["Random Value", "Random Value.001", "Random Value.002", "Random Value.003", "Random Value.004"].iter().zip(&seeds) {
            set_in(&mut j, "geo_spikes", n, "Seed", json!(*s));
        }
        let names: Vec<String> = (0..5).map(|i| format!("spike{i}")).collect();
        j["collections"] = json!({"assets:spikes": {"objects": names, "children": [], "offset": [0.0, 0.0, 0.0]}});
        let mut objects = gn::Objects::default();
        for (n, m) in names.iter().zip(spikes) {
            objects.map.insert(n.clone(), gn::eval::Object { geo: GGeom::from_mesh(m), matrix: gn::M4_ID });
        }
        objects.collections.insert("assets:spikes".to_string(), gn::eval::Collection { objects: names.clone(), children: vec![], offset: [0.0; 3] });
        let mut bm = gmesh(&v, &nets.f, &[]);
        set_point_attr(&mut bm, "selection", Ty::F, sel.iter().map(|&x| Val::F(x)).collect());
        let sp = run(j, "geo_spikes", GGeom::from_mesh(bm), &objects, None);
        if let Some(sm) = sp.mesh {
            let mut m = Mesh::new();
            m.pos = sm.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
            m.faces = sm.faces_vec();
            dump("spikes", &sm.pos, &m.faces);
            body_mesh.append(&m);
        }
        body_mesh
    }
}

impl Factory for CactusFactory {
    fn asset(&self, inst: u64) -> Asset {
        let mesh = self.mesh(inst, 0.003);
        let b = mesh.bounds();
        let mut a = Asset::default();
        a.mesh(mesh, &mat("cactus", [0.1, 0.25, 0.08], 0.6), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(CactusFactory::new(fs)))
}
