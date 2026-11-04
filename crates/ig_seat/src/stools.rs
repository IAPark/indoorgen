//! seating/chairs/{bar_chair,office_chair}.py: seat on straight / single-stand / wheeled legs
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    matches!(c, "NoSuchStool")
}

const SEAT: Weighted = &[("fabric.Velvet", 0.3), ("fabric.SofaFabric", 0.5), ("fabric.Leather", 0.2)];
const LEG: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("wood.Wood", 1.0),
    // (no glass: legs of it were a ghost under the seat)
];

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.35, 0.3, 0.3])
}

pub struct Stool {
    office: bool,
    x: f64,
    z: f64,
    top_t: f64,
    style: usize, // 0 straight 1 single_stand 2 wheeled
    ngon32: bool,
    bottom_scale: f64,
    leg_d: f64,
    curve: Vec<(f64, f64)>,
    poles: usize,
    joint_h: f64,
    front_w: f64,
    mid_w: f64,
    back_w: f64,
    legs: usize,
    stretch_pos: f64,
    incr: usize,
    seed: u64,
}

impl Stool {
    fn new(c: &str, rng: &mut Rng) -> Stool {
        // exact python draw order (bar_chair.py / office_chair.py sample_parameters), factory stream
        let mut r = rng.clone();
        let r = &mut r;
        let office = c == "OfficeChairFactory";
        let (x, z) = if office { (r.uniform(0.5, 0.6), r.uniform(1.0, 1.4)) } else { (r.uniform(0.35, 0.45), r.uniform(0.7, 1.0)) };
        let top_t = if office { r.uniform(0.5, 0.7) } else { r.uniform(0.06, 0.10) };
        let style = if office { [1, 2][r.choice_idx(2)] } else { r.choice_idx(3) };
        let (mut front_w, mut mid_w, mut back_w) = (0.65, 0.7, 0.75);
        if office {
            front_w = r.uniform(0.5, 0.8);
            let _front_bent = r.uniform(-1.5, -0.4);
            let _seat_bent = r.uniform(-1.5, -0.4);
            let _mid_bent = r.uniform(-2.4, -0.5);
            mid_w = r.uniform(0.5, 0.9);
            let _back_bent = r.uniform(-1.0, -0.1);
            back_w = r.uniform(0.6, 0.9);
            let _mid_pos = r.uniform(0.4, 0.6);
        }
        let ngon32 = r.choice_idx(2) == 1;
        let bottom_scale = r.uniform(1.1, 1.3);
        let (mut leg_d, mut curve, mut poles, mut joint_h) = (0.05, vec![], 4, 0.0);
        let (mut legs, mut stretch_pos, mut incr) = (1usize, 0.5, 0usize);
        match style {
            1 => {
                leg_d = r.uniform(0.7 * x, 0.9 * x);
                curve = vec![(0.0, r.uniform(0.1, 0.2)), (0.5, r.uniform(0.1, 0.2)), (0.9, r.uniform(0.2, 0.3)), (1.0, 1.0)];
            }
            0 => {
                leg_d = r.uniform(0.04, 0.06);
                legs = 3 + r.choice_idx(2);
                curve = vec![(0.0, 1.0), (0.4, r.uniform(0.85, 0.95)), (1.0, r.uniform(0.4, 0.6))];
                stretch_pos = r.uniform(0.6, 0.9);
                incr = r.choice_idx(3);
            }
            _ => {
                leg_d = r.uniform(0.03, 0.05);
                poles = [4, 5][r.choice_idx(2)];
                joint_h = r.uniform(0.5, 0.8) * (z - top_t);
                let _arc = r.uniform(120.0, 240.0);
                let _wheel_w = r.uniform(0.11, 0.15);
                let _wheel_rot = r.uniform(0.0, 360.0);
                let _pole_len = r.uniform(1.6, 2.0);
            }
        }
        Stool { office, x, z, top_t, style, ngon32, bottom_scale, leg_d, curve, poles, joint_h, front_w, mid_w, back_w, legs, stretch_pos, incr, seed: r.randint(0, 1_000_000) as u64 }
    }
    fn prof(&self, t: f64) -> f64 {
        for w in self.curve.windows(2) {
            if t <= w[1].0 {
                let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
                return w[0].1 + (w[1].1 - w[0].1) * f;
            }
        }
        self.curve.last().map(|c| c.1).unwrap_or(1.0)
    }
}

impl Factory for Stool {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        None
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let seat_m = spec("seat", weighted(&mut r, SEAT), self.seed);
        let leg_m = spec("stool_leg", weighted(&mut r, LEG), self.seed + 1);
        let mut a = Asset::default();
        let (x, z, t) = (self.x, self.z, self.top_t);
        let top_z = z - t;
        let leg_h = top_z;
        // seat
        let n = 32;
        if !self.office {
            // generate_round_seats: capped disc of radius = Top Profile Width (x), thickness = Top Thickness
            let outline: Vec<[f64; 2]> = (0..n).map(|i| { let a = 2.0 * PI * i as f64 / n as f64; [x * a.cos(), x * a.sin()] }).collect();
            let seat = extrude(&outline, top_z, top_z + t);
            let sb = seat.bounds();
            a.mesh(seat, &seat_m, Some(sb));
            a.supports.push(([sb.0[0], sb.0[1], sb.1[2] - 0.001], sb.1));
        }
        if self.office {
            let hw = x / 2.0;
            let pts = [[-hw * self.back_w, -hw], [hw * self.front_w, -hw], [hw * self.front_w, hw], [-hw * self.back_w, hw]];
            let outline = chaikin(&pts, 3);
            let seat_t = 0.08;
            let seat = extrude(&outline, top_z, top_z + seat_t);
            let sb = seat.bounds();
            a.mesh(seat, &seat_m, Some(sb));
            a.supports.push(([sb.0[0], sb.0[1], sb.1[2] - 0.001], sb.1));
            let bh = (t - seat_t).max(0.3);
            let mut back = Mesh::cuboid([-0.03, -x / 2.0 * self.mid_w, 0.0], [0.0, x / 2.0 * self.mid_w, bh]);
            for p in back.pos.iter_mut() {
                p[0] += -x / 2.0 * self.back_w - 0.12 * p[2];
                p[2] += top_z + seat_t;
            }
            let bb = back.bounds();
            a.mesh(back, &seat_m, Some(bb));
        }
        let k = 12;
        match self.style {
            1 | 0 => {
                use super::tableparts::*;
                let aw = 0.7 * x; // Leg Placement Top Relative Scale (0.7) * Top Profile Width
                let anch = anchors(if self.style == 1 { 1 } else { self.legs }, aw, 1.0);
                let tops: Vec<V3> = anch.iter().map(|p| [p[0], p[1], top_z]).collect();
                let curve = self.curve.clone();
                if self.style == 1 {
                    let prof = ngon_profile(64, self.leg_d, 1.0, 0.0);
                    let leg = ngon_cylinder(&prof, 1.0, 64, &|t| float_curve(&curve, t), true);
                    for p in &tops {
                        let m = place_leg(&leg, *p, 1.0f64.max(self.bottom_scale * 0.0 + 1.0), true);
                        let b = m.bounds();
                        a.mesh(m, &leg_m, Some(b));
                    }
                } else {
                    let prof = ngon_profile(if self.ngon32 { 32 } else { 4 }, self.leg_d, 1.0, 0.1);
                    let leg = ngon_cylinder(&prof, 1.0, 32, &|t| float_curve(&curve, t), true);
                    for p in &tops {
                        let m = place_leg(&leg, *p, self.bottom_scale, true);
                        let b = m.bounds();
                        a.mesh(m, &leg_m, Some(b));
                    }
                    if self.incr > 0 && tops.len() > 1 {
                        let nn = tops.len();
                        let pts: Vec<V3> = tops.iter().map(|p| { let v = [p[0] - p[0] * self.bottom_scale, p[1] - p[1] * self.bottom_scale, p[2]]; [p[0] + v[0] * (self.stretch_pos - 1.0), p[1] + v[1] * (self.stretch_pos - 1.0), p[2] + v[2] * (self.stretch_pos - 1.0)] }).collect();
                        for i in 0..nn {
                            let j = (i + self.incr) % nn;
                            if (self.incr * 2 == nn && i >= nn / 2) || i == j {
                                continue;
                            }
                            let m = stretcher(pts[i], pts[j], self.leg_d * 0.5, 32);
                            let b = m.bounds();
                            a.mesh(m, &leg_m, Some(b));
                        }
                    }
                }
            }
            _ => {
                // gas-lift pole with a star base and casters
                let pr = self.leg_d / 2.0;
                let pole = limb([0.0, 0.0, self.joint_h], [0.0, 0.0, top_z], pr, pr, true);
                let pb = pole.bounds();
                a.mesh(pole, &leg_m, Some(pb));
                let arm = (x * 0.55).max(0.25);
                for i in 0..self.poles {
                    let ang = 2.0 * PI * i as f64 / self.poles as f64;
                    let (ex, ey) = (arm * ang.cos(), arm * ang.sin());
                    let l = limb([0.0, 0.0, self.joint_h], [ex, ey, 0.05], pr * 0.8, pr * 0.8, true);
                    let lb = l.bounds();
                    a.mesh(l, &leg_m, Some(lb));
                    let mut wh = Mesh::cylinder(0.0, 0.0, -0.02, 0.02, 0.025, 10);
                    wh.transform(&Mat4::rot_x(PI / 2.0));
                    wh.transform(&Mat4::translation([ex, ey, 0.025]));
                    let wb = wh.bounds();
                    a.mesh(wh, &leg_m, Some(wb));
                }
            }
        }
        let _ = self.office;
        a
    }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    Some(Box::new(Stool::new(c, rng)))
}
