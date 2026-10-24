//! Reusable node-graph helpers evaluated with the gn interpreter (data/gn/*.json exported with tools/gn_export.py).
use crate::gn;
use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::Ty;
use crate::mesh::Mesh;
use serde_json::{json, Value as J};
use crate::dhash::HashMap;
use std::sync::OnceLock;

fn set_node(j: &mut J, group: &str, node: &str, f: &dyn Fn(&mut J)) {
    if let Some(nodes) = j["groups"][group]["nodes"].as_array_mut() {
        for n in nodes.iter_mut() {
            if n["name"].as_str() == Some(node) {
                f(n);
            }
        }
    }
}

fn set_input(n: &mut J, sock: &str, v: J) {
    if let Some(ins) = n["in"].as_array_mut() {
        for s in ins.iter_mut() {
            if s["name"].as_str() == Some(sock) {
                s["v"] = v.clone();
            }
        }
    }
}

/// gn mesh -> crate Mesh
pub fn to_mesh(g: &GGeom) -> Mesh {
    let mut m = Mesh::new();
    if let Some(gm) = &g.mesh {
        m.pos = gm.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        m.faces = gm.faces_vec();
    }
    m
}

/// assets/utils/nodegroup.geo_radius: tube of `radius` with a `resolution`-gon section (caps) along every polyline, merged within `merge`
pub fn geo_radius(polylines: &[Vec<[f64; 3]>], radius: f64, resolution: i64, merge: f64) -> Mesh {
    geo_radius_ex(polylines, radius, resolution, merge, true)
}

pub fn geo_radius_ex(polylines: &[Vec<[f64; 3]>], radius: f64, resolution: i64, merge: f64, align_tilt: bool) -> Mesh {
    let mut v = vec![];
    let mut e = vec![];
    for pl in polylines {
        let base = v.len();
        for (i, p) in pl.iter().enumerate() {
            v.push(*p);
            if i > 0 {
                e.push([(base + i - 1) as u32, (base + i) as u32]);
            }
        }
    }
    geo_radius_graph(&v, &e, radius, resolution, merge, align_tilt)
}

/// geo_radius on an arbitrary wire mesh (vertices + edges, e.g. a branching skeleton)
pub fn geo_radius_graph(verts: &[[f64; 3]], edges: &[[u32; 2]], radius: f64, resolution: i64, merge: f64, align_tilt: bool) -> Mesh {
    static TPL: OnceLock<J> = OnceLock::new();
    static TPL2: OnceLock<J> = OnceLock::new();
    let tpl = if align_tilt {
        TPL.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/geo_radius.json")).unwrap())
    } else {
        TPL2.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/geo_radius_notilt.json")).unwrap())
    };
    let mut j = tpl.clone();
    set_node(&mut j, "geo_radius", "Value", &|n| n["p"]["value"] = json!(radius));
    set_node(&mut j, "geo_radius", "Curve Circle", &|n| set_input(n, "Resolution", json!(resolution)));
    set_node(&mut j, "geo_radius", "Merge by Distance", &|n| set_input(n, "Distance", json!(merge)));
    let groups = gn::load::load_groups(j);
    let g = groups.map.get("geo_radius").unwrap();
    let v: Vec<J> = verts.iter().map(|p| json!([p[0], p[1], p[2]])).collect();
    let e: Vec<J> = edges.iter().map(|x| json!([x[0], x[1]])).collect();
    let mesh = gn::load::mesh_from_json(&json!({"v": v, "e": e, "f": []}));
    let mut inputs: HashMap<String, gn::Value> = HashMap::default();
    let gi = g.inputs.iter().find(|i| i.ty == Ty::Geo).unwrap();
    inputs.insert(gi.id.clone(), gn::Value::geo(GGeom::from_mesh(mesh)));
    let outs = gn::eval(&groups, "geo_radius", &inputs, &gn::Objects::default()).expect("geo_radius eval");
    let oi = g.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
    match &outs[oi].1 {
        gn::Value::G(gg) => to_mesh(&gn::meshops::realize_instances(gg, 1000)),
        _ => Mesh::new(),
    }
}

fn set_val_idx(n: &mut J, sock: &str, which: usize, v: J) {
    let mut k = 0;
    if let Some(ins) = n["in"].as_array_mut() {
        for s in ins.iter_mut() {
            if s["name"].as_str() == Some(sock) {
                if k == which {
                    s["v"] = v.clone();
                }
                k += 1;
            }
        }
    }
}

/// decorate.geo_extension: noise displacement `pos * (noise + 0.25) * strength`, direction = normalize(pos) + offset
pub fn geo_extension(m: &Mesh, offset: [f64; 3], scale: f64, strength: f64) -> Mesh {
    static TPL: OnceLock<J> = OnceLock::new();
    let tpl = TPL.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/geo_extension.json")).unwrap());
    let mut j = tpl.clone();
    set_node(&mut j, "geo_extension", "Vector Math.002", &|n| set_val_idx(n, "Vector", 1, json!(offset)));
    set_node(&mut j, "geo_extension", "Noise Texture", &|n| set_input(n, "Scale", json!(scale)));
    set_node(&mut j, "geo_extension", "Math.002", &|n| set_val_idx(n, "Value", 1, json!(strength)));
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
        gn::Value::G(gg) => to_mesh(&gn::meshops::realize_instances(gg, 1000)),
        _ => Mesh::new(),
    }
}
