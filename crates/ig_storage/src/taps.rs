//! table_decorations/sink.py TapFactory: the water-tap node group (data/gn/tap.json) with the interface defaults the python factory
//! draws when it builds the group inside create_asset (`nodegroup_water_tap()` expose_input defaults), then scale 0.4 / rotate z by pi.
use super::gnasset::*;
use super::inst_rng;
use crate::mesh::Mesh;
use serde_json::json;
use std::sync::OnceLock;

pub const TAP_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/tap.json"));

/// tap mesh for TapFactory(fseed).spawn_asset(inst) in the tap frame (before the sink places it)
pub fn tap_mesh(fseed: u64, inst: u64) -> Mesh {
    static TPL: OnceLock<GnTemplate> = OnceLock::new();
    let tpl = TPL.get_or_init(|| GnTemplate::parse(TAP_JSON));
    let mut t = tpl.clone_template();
    let mut rr = inst_rng(fseed, inst);
    // nodegroup_water_tap expose_input defaults, in order; booleans are `True if U() > p else False`
    let mut vals: Vec<serde_json::Value> = vec![];
    for i in 0..13 {
        let v = match i {
            0 => json!(rr.uniform(0.2, 0.3)),
            1 => json!(rr.uniform(0.7, 1.1)),
            2 => json!(rr.uniform(5.5, 7.0)),
            3 => json!(rr.uniform(0.5, 1.0)),
            4 => json!(rr.uniform(0.0, 0.1)),
            5 | 10 => json!(rr.f64() > 0.5),
            6 => json!(rr.uniform(-0.5, -0.06)),
            7 => json!(rr.f64() > 0.2),
            8 => json!(rr.uniform(0.75, 1.25)),
            9 => json!(rr.uniform(0.95, 1.55)),
            _ => json!(rr.f64() > 0.8),
        };
        vals.push(v);
    }
    for (k, v) in vals.iter().enumerate() {
        t.set_group_default("nodegroup_water_tap", &format!("Socket_{k}"), v.clone());
    }
    let g = t.run(&Params::default()).expect("tap gn");
    let mut m = super::gnhelpers::to_mesh(&g);
    // scale 0.4, rotation z += pi (float32 apply_transform: x' = -0.4 x, y' = -0.4 y)
    for p in m.pos.iter_mut() {
        let (x, y, z) = (p[0] as f32 * 0.4, p[1] as f32 * 0.4, p[2] as f32 * 0.4);
        let (s, c) = (std::f32::consts::PI.sin(), std::f32::consts::PI.cos());
        *p = [(x * c - y * s) as f64, (x * s + y * c) as f64, z as f64];
    }
    m
}

pub struct TapFactory {
    pub fseed: u64,
}

impl super::Factory for TapFactory {
    fn asset(&self, inst: u64) -> super::Asset {
        let m = tap_mesh(self.fseed, inst);
        let mut a = super::Asset::default();
        a.mesh(m, &super::mat("tap", [0.7, 0.7, 0.72], 0.2), None);
        a
    }
}
