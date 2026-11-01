//! table_decorations/sink.py TapFactory: the water-tap node group (data/gn/tap.json) with the interface defaults the python factory
//! draws when it builds the group inside create_asset (`nodegroup_water_tap()` expose_input defaults), then scale 0.4 / rotate z by pi.
use super::gnasset::*;
use super::inst_rng;
use crate::mesh::Mesh;
use serde_json::json;
use std::sync::OnceLock;

pub const TAP_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/tap.json"));

/// nodegroup_water_tap's interface values for this instance (in socket order; booleans are `True if U() > p else False`)
fn tap_values(fseed: u64, inst: u64) -> Vec<serde_json::Value> {
    let mut rr = inst_rng(fseed, inst);
    (0..13)
        .map(|i| match i {
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
        })
        .collect()
}

/// bounds of `tap_mesh(fseed, inst)` from the group's values, part by part as nodegroup_water_tap lays them out (group frame, then
/// scale 0.4 / rotate pi): the base plate (base_width x 0.7, 0.05 high), the stem (r 0.05, 0.15 high), the spout (a pipe 0.6 high and
/// its head -- an arc of r 0.2 squashed to 0.7, or a trimmed bezier scaled (5.2, 0.5, 7.8) and turned 2.522 -- turned by roation_z and
/// stretched by tap_height), the handles (two lever bezier tubes at y = +-0.2, or the knob cylinders scaled by hands_length), or the
/// other type's arched spout and lever (turned pi)
pub fn tap_bounds(fseed: u64, inst: u64) -> ([f64; 3], [f64; 3]) {
    let v = tap_values(fseed, inst);
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0);
    let b = |i: usize| v[i].as_bool().unwrap_or(false);
    let (base_width, tap_head, rot_z, tap_height, switch, hand_type, hx, hy, different) = (f(0), f(1), f(2), f(3), b(5), b(7), f(8), f(9), b(11));
    let mut boxes: Vec<([f64; 3], [f64; 3])> = vec![([-base_width / 2.0, -0.35, 0.0], [base_width / 2.0, 0.35, 0.05])];
    // a box's xy corners turned about z
    let turn = |lo: [f64; 3], hi: [f64; 3], a: f64| -> ([f64; 3], [f64; 3]) {
        let (s, c) = a.sin_cos();
        let (mut l, mut h) = ([f64::INFINITY, f64::INFINITY, lo[2]], [f64::NEG_INFINITY, f64::NEG_INFINITY, hi[2]]);
        for x in [lo[0], hi[0]] { for y in [lo[1], hi[1]] { let q = [x * c - y * s, x * s + y * c]; for k in 0..2 { l[k] = l[k].min(q[k]); h[k] = h[k].max(q[k]); } } }
        (l, h)
    };
    if different {
        // the arched spout (x to 1, profile r <= 0.127 widened to 2.5x past x = 0.2, scaled (0.4, 0.4, 0.3), turned 0.6807, up 0.1), the
        // stem (r 0.055), the lever (a 0.7 rod scaled (1.7, 3.1), tipped -2.042 about y, at (0.3, 0, 0.25)) and its knob, x scaled 0.9;
        // all turned pi
        let spout = turn([-0.052, -0.128, 0.06], [0.452, 0.128, 0.41], 0.6807);
        boxes.push(turn(spout.0, spout.1, std::f64::consts::PI));
        boxes.push(([-0.055, -0.055, 0.0], [0.055, 0.055, 0.15]));
        boxes.push(turn([-0.02, -0.032, 0.07], [0.56, 0.032, 0.43], std::f64::consts::PI));
    } else {
        boxes.push(([-0.05, -0.05, 0.0], [0.05, 0.05, 0.15]));
        // the pipe (r 0.03, 0.6 high) and the head on it, its reach about the pipe whatever roation_z turns it to: the arc (r 0.2 off
        // the pipe by 0.2, tube r 0.03) reaches 0.43 and stands 0.28 * tap_head up; the bezier (x -0.05..0.09, y up to 0.54) scaled
        // (5.2, 0.5) reaches 0.5 and 0.3 up
        // (a Switch takes its second input when true: the bezier)
        let (reach, top) = if switch { (0.5, 0.6 + 0.27 + 0.03) } else { (0.43, 0.6 + 0.28 * tap_head + 0.03) };
        let _ = rot_z;
        boxes.push(([-reach, -reach, -0.03 * tap_height], [reach, reach, top * tap_height]));
        if hand_type {
            // the knobs: cylinders (r 0.03 along y at y +-0.05, r 0.005 at y +-0.08, one stretched 4.1x in x), scaled by hands_length
            boxes.push(([-0.06 * hx, -0.1 * hy, 0.05], [0.04 * hx, 0.1 * hy, 0.21]));
        } else {
            // the levers: the handle tube (x -0.25..1.05, y +-0.25, z -0.25..0.95) scaled 0.3, turned 210 / 150 degrees at y -+0.2
            for (y, a) in [(-0.2, 3.6652f64), (0.2, 2.6180)] {
                let (lo, hi) = turn([-0.075, -0.075, -0.075], [0.315, 0.075, 0.285], a);
                boxes.push(([lo[0], lo[1] + y, lo[2]], [hi[0], hi[1] + y, hi[2]]));
            }
        }
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (a, c) in &boxes { for k in 0..3 { lo[k] = lo[k].min(a[k]); hi[k] = hi[k].max(c[k]); } }
    // scale 0.4, turned pi
    ([-0.4 * hi[0] - 0.003, -0.4 * hi[1] - 0.003, 0.4 * lo[2] - 0.003], [-0.4 * lo[0] + 0.003, -0.4 * lo[1] + 0.003, 0.4 * hi[2] + 0.003])
}

/// tap mesh for TapFactory(fseed).spawn_asset(inst) in the tap frame (before the sink places it)
pub fn tap_mesh(fseed: u64, inst: u64) -> Mesh {
    static TPL: OnceLock<GnTemplate> = OnceLock::new();
    let tpl = TPL.get_or_init(|| GnTemplate::parse(TAP_JSON));
    let mut t = tpl.clone_template();
    let vals = tap_values(fseed, inst);
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
    fn volumes(&self, inst: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = tap_bounds(self.fseed, inst);
        Some(vec![crate::piece::Volume::hard(lo, hi)])
    }
}
