//! small_plants/snake_plant.py SnakePlantFactory: n petals, each the node group nodegroup_single_petal (exported from Blender, data/gn/snake_petal.json)
//! with the per-petal random constants patched in, transformed and joined; scaled by 0.2.
use super::*;
use crate::gn;
use crate::materials::spec_material;
use crate::gn::geom::Geometry as GGeom;
use crate::gn::val::Ty;
use serde_json::{json, Value as J};
use crate::dhash::HashMap;
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    c == "SnakePlantFactory"
}

pub struct SnakePlant {
    fs: u64,
}

static TPL: OnceLock<J> = OnceLock::new();

fn tpl() -> &'static J {
    TPL.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/snake_petal.json")).unwrap())
}

fn node_mut<'a>(j: &'a mut J, group: &str, node: &str) -> &'a mut J {
    j["groups"][group]["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["name"].as_str() == Some(node)).unwrap()
}

fn set_input(j: &mut J, group: &str, node: &str, id: &str, v: J) {
    let n = node_mut(j, group, node);
    for s in n["in"].as_array_mut().unwrap() {
        if s["id"].as_str() == Some(id) {
            s["v"] = v;
            return;
        }
    }
    panic!("socket {} not found", id);
}

fn set_curve(j: &mut J, group: &str, node: &str, ys: &[f64]) {
    let n = node_mut(j, group, node);
    let pts = n["p"]["curve"]["pts"].as_array_mut().unwrap();
    for (p, y) in pts.iter_mut().zip(ys) {
        p[1] = json!(*y);
    }
}

struct PetalParams {
    end_x: f64,
    end_y: f64,
    curve_e: [f64; 5],
    curve_z: [f64; 5],
    z_mult: f64,
    thick: f64,
}

fn petal_geometry(p: &PetalParams) -> GGeom {
    let mut j = tpl().clone();
    set_input(&mut j, "nodegroup_setup", "Quadratic Bézier", "End", json!([p.end_x, p.end_y, 2.0]));
    set_curve(&mut j, "nodegroup_edge_extrusion", "Float Curve", &p.curve_e);
    set_curve(&mut j, "nodegroup_z_petal_rotation", "Float Curve", &p.curve_z);
    set_input(&mut j, "nodegroup_z_petal_rotation", "Math", "Value_001", json!(p.z_mult));
    node_mut(&mut j, "nodegroup_petal_thickness", "Value")["p"]["value"] = json!(p.thick);
    let groups = gn::load::load_groups(j);
    let objects = gn::Objects::default();
    let outs = gn::eval(&groups, "nodegroup_single_petal", &HashMap::default(), &objects).expect("petal eval");
    let g = &groups.map["nodegroup_single_petal"];
    let gi = g.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
    match &outs[gi].1 {
        gn::Value::G(gg) => (**gg).clone(),
        _ => panic!("petal output is not geometry"),
    }
}

impl Factory for SnakePlant {
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fs, inst);
        let n = r.randint(4, 8) as usize;
        let mut params: Vec<(f64, f64, f64, f64)> = vec![];
        let mut c = 0;
        while c < 50 && params.len() < n {
            c += 1;
            let scale = r.uniform(0.7, 1.0);
            let xr = r.normal(0.0, 0.15);
            let x = r.uniform(-0.7, 0.7);
            let y = r.uniform(-0.7, 0.7);
            let r1 = (scale * xr.sin()).max(0.2);
            let near = params.iter().any(|p| {
                let r2 = (p.0 * p.1.sin()).max(0.2);
                let d = ((x - p.2).powi(2) + (y - p.3).powi(2)).sqrt();
                r1 + r2 > d
            });
            if !near {
                params.push((scale, xr, x, y));
            }
        }
        let mut meshes: Vec<GGeom> = vec![];
        for &(scale, xr, x, y) in &params {
            let z_rot = r.uniform(0.0, 6.28);
            let z2 = r.uniform(0.0, 6.28);
            let end_x = r.uniform(-0.2, 0.2);
            let end_y = r.uniform(0.2, 0.2);
            let iw = r.uniform(0.15, 0.3);
            let e1 = iw + r.uniform(0.0, 0.1);
            let e2 = iw + r.uniform(0.02, 0.18);
            let e3 = iw + r.uniform(0.02, 0.1);
            let z1 = 0.25 + r.uniform(-0.1, 0.1);
            let z2c = 0.5 + r.uniform(-0.15, 0.15);
            let z3 = 0.5 + r.uniform(0.25, 0.25);
            let z_mult = r.uniform(0.8, 2.0);
            let thick = r.uniform(0.1, 0.35);
            let g = petal_geometry(&PetalParams { end_x, end_y, curve_e: [iw, e1, e2, e3, 0.0], curve_z: [0.0, z1, z2c, z3, 1.0], z_mult, thick });
            let m1 = gn::meshops::matrix_from_trs([0.0; 3], gn::math::euler_to_quat([0.0, 0.0, z_rot as f32]), [scale as f32; 3]);
            let g1 = gn::meshops::transform_geometry(&g, &m1, None);
            let m2 = gn::meshops::matrix_from_trs([0.0; 3], gn::math::euler_to_quat([xr as f32, 0.0, 0.0]), [1.0; 3]);
            let g2 = gn::meshops::transform_geometry(&g1, &m2, None);
            let m3 = gn::meshops::matrix_from_trs([x as f32, y as f32, 0.0], gn::math::euler_to_quat([0.0, 0.0, z2 as f32]), [1.0; 3]);
            meshes.push(gn::meshops::transform_geometry(&g2, &m3, None));
        }
        let rcs: Vec<std::rc::Rc<GGeom>> = meshes.into_iter().rev().map(std::rc::Rc::new).collect();
        let joined = gn::meshops::join_geometries(&rcs);
        let joined = gn::meshops::realize_instances(&joined, 1000);
        let mut m = Mesh::new();
        if let Some(gm) = &joined.mesh {
            for p in &gm.pos {
                m.pos.push([(p[0] * 0.2f32) as f64, (p[1] * 0.2f32) as f64, (p[2] * 0.2f32) as f64]);
            }
            m.faces = gm.faces_vec();
        }
        let mat = spec_material("snake_plant", json!({"cls": "plant.SnakePlant", "kwargs": {}, "seed": self.fs}), [0.2, 0.4, 0.2]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(SnakePlant { fs }))
}
