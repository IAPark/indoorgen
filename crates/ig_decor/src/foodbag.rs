//! tableware/food_bag.py FoodBagFactory: default cylinder -> subdivide_edge_ring(64) -> squash y by the curve profile -> WELD(1e-3) ->
//! extrude_edges_move of both end rings (seal) -> SUBSURF 2 -> geo_extension (musgrave 2D).
use super::textdraw::text_ctor_and_generate;
use super::*;
use crate::dhash::HashMap;
use crate::gn;
use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::Ty;
use crate::materials::*;
use serde_json::{json, Value as J};
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    c == "FoodBagFactory"
}

pub struct FoodBag {
    length: f64,
    width: f64,
    depth: f64,
    curve_profile: f64,
    extrude_length: f64,
    fs: u64,
}

impl FoodBag {
    fn new(fs: u64) -> FoodBag {
        let mut r = Rng::seeded(fs);
        let length = r.uniform(0.1, 0.3);
        let is_packet = r.uniform(0.0, 1.0) < 0.6;
        let (width, depth, curve_profile);
        if is_packet {
            width = length * r.log_uniform(0.6, 1.0);
            depth = width * r.uniform(0.5, 0.8);
            curve_profile = r.uniform(2.0, 4.0);
        } else {
            width = length * r.log_uniform(0.2, 0.4);
            depth = width * r.uniform(0.6, 1.0);
            curve_profile = r.uniform(4.0, 8.0);
        }
        let extrude_length = r.uniform(0.05, 0.1);
        text_ctor_and_generate(&mut r);
        r.uniform(0.0, 1.0); // texture_shared
        FoodBag { length, width, depth, curve_profile, extrude_length, fs }
    }
}

/// bpy.ops.mesh.primitive_cylinder_add() defaults, in Blender's vertex / face / edge order (dumped from Blender)
fn cylinder() -> (Mesh, Vec<[u32; 2]>) {
    let j: J = serde_json::from_str(include_str!("foodbag_cyl.json")).unwrap();
    let mut m = Mesh::new();
    for v in j["verts"].as_array().unwrap() {
        let a = v.as_array().unwrap();
        m.pos.push([a[0].as_f64().unwrap() as f32 as f64, a[1].as_f64().unwrap() as f32 as f64, a[2].as_f64().unwrap() as f32 as f64]);
    }
    for f in j["polys"].as_array().unwrap() {
        m.faces.push(f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect());
    }
    let e = j["edges"].as_array().unwrap().iter().map(|e| [e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32]).collect();
    (m, e)
}

fn corner_edges(m: &Mesh, edges: &[[u32; 2]]) -> (Vec<[u32; 2]>, Vec<Vec<u32>>) {
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut es: Vec<[u32; 2]> = vec![];
    for e in edges {
        emap.insert((e[0].min(e[1]), e[0].max(e[1])), es.len() as u32);
        es.push(*e);
    }
    let mut ce = vec![];
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                es.push([a, b]);
                (es.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    (es, ce)
}

/// decorate.subdivide_edge_ring(obj, cuts, axis z) on the exact bmesh operator port; returns the mesh and its edge array
fn subdivide_edge_ring(m: &Mesh, edges: &[[u32; 2]], cuts: usize) -> (Mesh, Vec<[u32; 2]>) {
    use crate::bm::BMesh;
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let (es, ce) = corner_edges(m, edges);
    let mut bm = BMesh::from_mesh(&pos, &es, &m.faces, &ce);
    let mut sel = vec![];
    for (i, e) in es.iter().enumerate() {
        let (a, b) = (pos[e[0] as usize], pos[e[1] as usize]);
        let d = [b[0] as f64 - a[0] as f64, b[1] as f64 - a[1] as f64, b[2] as f64 - a[2] as f64];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > 0.0 && (d[2] / l).abs() > 1.0 - 1e-3 {
            sel.push(i);
        }
    }
    crate::bm::edgering::subdivide_edgering(&mut bm, &sel, cuts);
    let (p, e, f) = bm.to_mesh();
    let mut o = Mesh::new();
    o.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    o.faces = f;
    (o, e)
}

fn dump(name: &str, m: &Mesh) {
    if let Ok(d) = std::env::var("FBDUMP") {
        let v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let j = json!({"verts": v, "polys": m.faces});
        std::fs::write(format!("{d}/{name}.json"), j.to_string()).unwrap();
    }
}

fn set_node(j: &mut J, group: &str, node: &str, f: &dyn Fn(&mut J)) {
    if let Some(nodes) = j["groups"][group]["nodes"].as_array_mut() {
        for n in nodes.iter_mut() {
            if n["name"].as_str() == Some(node) {
                f(n);
            }
        }
    }
}
fn set_in(n: &mut J, sock: &str, idx: usize, v: J) {
    let mut k = 0;
    if let Some(ins) = n["in"].as_array_mut() {
        for s in ins.iter_mut() {
            if s["name"].as_str() == Some(sock) {
                if k == idx {
                    s["v"] = v.clone();
                }
                k += 1;
            }
        }
    }
}

/// decorate.geo_extension(musgrave_dimensions="2D"): same graph as data/gn/geo_extension.json with a 2D noise
fn geo_extension_2d(m: &Mesh, offset: [f64; 3], scale: f64, strength: f64) -> Mesh {
    static TPL: OnceLock<J> = OnceLock::new();
    let tpl = TPL.get_or_init(|| serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/geo_extension.json"))).unwrap());
    let mut j = tpl.clone();
    set_node(&mut j, "geo_extension", "Vector Math.002", &|n| set_in(n, "Vector", 1, json!(offset)));
    set_node(&mut j, "geo_extension", "Noise Texture", &|n| {
        n["p"]["noise_dimensions"] = json!("2D");
        set_in(n, "Scale", 0, json!(scale));
    });
    set_node(&mut j, "geo_extension", "Math.002", &|n| set_in(n, "Value", 1, json!(strength)));
    let groups = gn::load::load_groups(j);
    let g = groups.map.get("geo_extension").unwrap();
    let v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let gm = GMeshT::from_faces(v, &m.faces, &[]);
    let mut inputs: HashMap<String, gn::Value> = HashMap::default();
    let gi = g.inputs.iter().find(|i| i.ty == Ty::Geo).unwrap();
    inputs.insert(gi.id.clone(), gn::Value::geo(GGeom::from_mesh(gm)));
    let outs = gn::eval(&groups, "geo_extension", &inputs, &gn::Objects::default()).expect("geo_extension eval");
    let oi = g.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
    match &outs[oi].1 {
        gn::Value::G(gg) => gnhelpers::to_mesh(&gn::meshops::realize_instances(gg, 1000)),
        _ => Mesh::new(),
    }
}

impl FoodBag {
    fn make_base(&self) -> (Mesh, Vec<[u32; 2]>) {
        let (cyl, cedges) = cylinder();
        dump("cyl", &cyl);
        let (mut m, e) = subdivide_edge_ring(&cyl, &cedges, 64);
        dump("ring", &m);
        let sc = [(self.width / 2.0) as f32, (self.depth / 2.0) as f32, (self.length / 2.0) as f32];
        for p in m.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc[k]) as f64;
            }
        }
        for p in m.pos.iter_mut() {
            let ratio = 1.0 - (2.0 * p[2].abs() / self.length).powf(self.curve_profile);
            p[1] = (ratio * p[1]) as f32 as f64;
            p[0] = p[0] as f32 as f64;
            p[2] = p[2] as f32 as f64;
        }
        dump("squash", &m);
        // WELD modifier (mode ALL, 1e-3)
        let lm = super::leafkit::LMesh { v: m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: m.faces.clone(), e: e.clone() };
        let w = lm.weld(1e-3 as f32 as f64);
        let mut o = Mesh::new();
        o.pos = w.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        o.faces = w.f.clone();
        dump("weld", &o);
        // add_seal: bpy.ops.mesh.extrude_edges_move with only vertices selected moves them by (0, 0, extrude_length * length * i)
        let lim = self.length / 2.0 - 1e-3;
        let idx: Vec<Vec<usize>> = [-1.0f64, 1.0].iter().map(|&i| (0..o.pos.len()).filter(|&k| o.pos[k][2] * i >= lim).collect()).collect();
        for (n, &i) in [-1.0f64, 1.0].iter().enumerate() {
            let dz = (self.extrude_length * self.length * i) as f32;
            for &k in &idx[n] {
                o.pos[k][2] = (o.pos[k][2] as f32 + dz) as f64;
            }
        }
        dump("seal", &o);
        (o, w.e)
    }
}

impl Factory for FoodBag {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let (m, e) = self.make_base();
        let m = match super::subsurf::subsurf_osd(&m, 2, false, Some(&e)) {
            Some(x) => x,
            None => super::subsurf::subsurf(&m, 2, false),
        };
        dump("subsurf", &m);
        let strength = r.uniform(0.1, 0.2);
        let scale = r.uniform(2.0 * 0.7, 2.0 * 1.4);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        let m = geo_extension_2d(&m, off, scale, strength);
        let b = m.bounds();
        let mut a = Asset::default();
        a.mesh(m, &ig_core::materials::spec_material("foodbag", serde_json::json!({"cls": "factory.tableware.food_bag.FoodBagFactory", "kwargs": {"attr": "surface"}, "seed": self.fs}), [0.7, 0.7, 0.7]), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(FoodBag::new(fs)))
}
