//! MediaConsole (not in infinigen): TV stands as real media consoles come, beyond infinigen's open cubby bench: a carcass divided into
//! columns, each with drawers, a door, a slatted door or an open cubby (mirrored so the front is symmetric), on a recessed plinth,
//! tapered legs, a metal frame or a recessed "floating" plinth, with knobs, bar pulls or push-to-open fronts. Frame as the other
//! storage pieces after their turn: x = depth (0 at the back, the front at +x), y = width centred, z = 0 on the floor.
use super::*;
use crate::materials::*;
use serde_json::json;

const BODY: Weighted = &[("wood.Wood", 3.0), ("wood.WhitePlywood", 2.0), ("wood.BlackPlywood", 1.2), ("wood.BlondePlywood", 1.5)];
const FRONT: Weighted = &[("wood.Wood", 2.0), ("wood.WhitePlywood", 1.5), ("wood.BlackPlywood", 0.8), ("plastic.PlasticRough", 0.6)];
const METAL: Weighted = &[("metal.BrushedBlackMetal", 2.0), ("metal.BrushedMetal", 1.5), ("metal.MetalBasic", 1.0)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Base { Plinth, Legs, Frame, Floating }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Col { Drawers(usize), Door, Slats, Open }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pull { Knob, Bar, Push }

pub struct MediaConsole {
    w: f64,
    d: f64,
    h: f64,
    base: Base,
    base_h: f64,
    cols: Vec<Col>,
    pull: Pull,
    /// the fronts in the body's material or a contrasting one
    contrast: bool,
    seed: u64,
}

fn bx(lo: [f64; 3], hi: [f64; 3]) -> Mesh { Mesh::cuboid(lo, hi) }

impl MediaConsole {
    pub fn new(r: &mut Rng) -> MediaConsole {
        let (mut w, mut d, mut h) = (r.uniform(1.3, 2.1), r.uniform(0.38, 0.48), r.uniform(0.42, 0.6));
        if let Some(sz) = crate::api::size_hint() { w = sz[0]; d = sz[1]; h = sz[2]; }
        let base = [Base::Plinth, Base::Legs, Base::Frame, Base::Floating][r.choice_p(&[0.25, 0.4, 0.15, 0.2])];
        let base_h = match base { Base::Plinth => r.uniform(0.04, 0.07), Base::Legs => (h * r.uniform(0.25, 0.38)).min(0.2), Base::Frame => (h * 0.25).min(0.14), Base::Floating => r.uniform(0.06, 0.1) };
        // columns ~0.4-0.6 m wide, chosen for the left half and mirrored (an odd middle one of its own)
        let n = ((w / r.uniform(0.42, 0.6)).round() as usize).clamp(2, 5);
        let mut pick = |r: &mut Rng| match r.choice_p(&[0.35, 0.3, 0.15, 0.2]) { 0 => Col::Drawers(if h - base_h > 0.42 { 1 + r.randint(1, 3) as usize } else { 1 + r.randint(0, 2) as usize }), 1 => Col::Door, 2 => Col::Slats, _ => Col::Open };
        let half: Vec<Col> = (0..n / 2).map(|_| pick(r)).collect();
        let mut cols = half.clone();
        if n % 2 == 1 { cols.push(pick(r)); }
        cols.extend(half.iter().rev());
        let pull = [Pull::Knob, Pull::Bar, Pull::Push][r.choice_p(&[0.3, 0.4, 0.3])];
        MediaConsole { w, d, h, base, base_h, cols, pull, contrast: r.f64() < 0.35, seed: r.randint(0, 1_000_000) as u64 }
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let bc = weighted(&mut r, BODY);
        let body = spec_material("console_body", json!({"cls": bc, "kwargs": {}, "seed": self.seed}), [0.45, 0.32, 0.2]);
        let front = if self.contrast { spec_material("console_front", json!({"cls": weighted(&mut r, FRONT), "kwargs": {}, "seed": self.seed + 1}), [0.8, 0.78, 0.74]) } else { body.clone() };
        let metal = spec_material("console_metal", json!({"cls": weighted(&mut r, METAL), "kwargs": {}, "seed": self.seed + 2}), [0.1, 0.1, 0.1]);
        // the carcass depth: the total less what stands proud of the front (pulls, slats), so the piece keeps to its layout box
        let (w, h, bh) = (self.w, self.h, self.base_h);
        let d = self.d - self.proud();
        let hw = w / 2.0;
        let t = 0.018; // board thickness
        let mut a = Asset::default();
        let mut add = |a: &mut Asset, m: Mesh, mat: &Material| { let b = m.bounds(); a.mesh(m, mat, Some(b)); };
        // carcass: top, bottom, sides, back, dividers
        let z0 = bh;
        add(&mut a, bx([0.0, -hw, h - t], [d, hw, h]), &body);
        add(&mut a, bx([0.0, -hw + t, z0], [d - 0.002, hw - t, z0 + t]), &body);
        add(&mut a, bx([0.0, -hw, z0], [d, -hw + t, h - t]), &body);
        add(&mut a, bx([0.0, hw - t, z0], [d, hw, h - t]), &body);
        add(&mut a, bx([0.0, -hw + t, z0 + t], [0.008, hw - t, h - t]), &body);
        let n = self.cols.len();
        let cw = (w - 2.0 * t) / n as f64;
        for i in 1..n {
            let y = -hw + t + cw * i as f64;
            add(&mut a, bx([0.008, y - t / 2.0, z0 + t], [d - 0.01, y + t / 2.0, h - t]), &body);
        }
        // fronts, flush with the carcass front (a 3 mm reveal round each)
        let (fz0, fz1) = (z0 + t, h - t);
        let gap = 0.003;
        let fx = d - 0.018;
        let mut pulls: Vec<Mesh> = vec![];
        let mut pull_at = |y: f64, z: f64, horizontal: bool| {
            match self.pull {
                Pull::Knob => pulls.push(ig_kit::geomkit::limb([d, y, z], [d + 0.025, y, z], 0.011, 0.014, true)),
                Pull::Bar => {
                    let l = 0.12;
                    let (p0, p1) = if horizontal { ([d + 0.03, y - l / 2.0, z], [d + 0.03, y + l / 2.0, z]) } else { ([d + 0.03, y, z - l / 2.0], [d + 0.03, y, z + l / 2.0]) };
                    let mut m = ig_kit::geomkit::limb(p0, p1, 0.006, 0.006, true);
                    for p in [p0, p1] { m.append(&ig_kit::geomkit::limb([d, p[1], p[2]], p, 0.005, 0.005, true)); }
                    pulls.push(m);
                }
                Pull::Push => {}
            }
        };
        for (i, c) in self.cols.iter().enumerate() {
            let (y0, y1) = (-hw + t + cw * i as f64 + gap, -hw + t + cw * (i + 1) as f64 - gap);
            let yc = 0.5 * (y0 + y1);
            match *c {
                Col::Drawers(k) => {
                    let dh = (fz1 - fz0) / k as f64;
                    for j in 0..k {
                        let (za, zb) = (fz0 + dh * j as f64 + gap, fz0 + dh * (j + 1) as f64 - gap);
                        add(&mut a, bx([fx, y0, za], [d, y1, zb]), &front);
                        // push-to-open drawers: a finger groove along the top edge
                        if self.pull == Pull::Push { add(&mut a, bx([d - 0.004, y0 + 0.02, zb - 0.018], [d + 0.001, y1 - 0.02, zb - 0.012]), &metal); } else { pull_at(yc, 0.5 * (za + zb), true); }
                    }
                }
                Col::Door | Col::Slats => {
                    add(&mut a, bx([fx, y0, fz0 + gap], [d, y1, fz1 - gap]), &front);
                    if *c == Col::Slats {
                        // vertical slats proud of the door
                        let ns = ((y1 - y0) / 0.035).floor().max(3.0) as usize;
                        for s in 0..ns {
                            let y = y0 + (y1 - y0) * (s as f64 + 0.5) / ns as f64;
                            add(&mut a, bx([d, y - 0.009, fz0 + gap + 0.01], [d + 0.012, y + 0.009, fz1 - gap - 0.01]), &front);
                        }
                    }
                    // the pull on the edge toward the middle of the piece
                    let ye = if yc < 0.0 { y1 - 0.04 } else { y0 + 0.04 };
                    if self.pull != Pull::Push { pull_at(ye, fz0 + (fz1 - fz0) * 0.6, false); }
                }
                Col::Open => {
                    // an open cubby: a shelf halfway when it is tall enough
                    if fz1 - fz0 > 0.32 { add(&mut a, bx([0.008, y0, 0.5 * (fz0 + fz1) - t / 2.0], [d - 0.01, y1, 0.5 * (fz0 + fz1) + t / 2.0]), &body); }
                }
            }
        }
        for m in pulls { add(&mut a, m, &metal); }
        // base
        match self.base {
            Base::Plinth => add(&mut a, bx([0.04, -hw + 0.03, 0.0], [d - 0.04, hw - 0.03, bh]), &body),
            Base::Floating => add(&mut a, bx([0.08, -hw + 0.12, 0.0], [d - 0.1, hw - 0.12, bh]), &metal),
            Base::Legs => {
                let mut ys = vec![-hw + 0.06, hw - 0.06];
                if w > 1.6 { ys.push(0.0); }
                for y in ys { for x in [0.06, d - 0.06] {
                    let sx = if x > d / 2.0 { 0.015 } else { -0.015 };
                    add(&mut a, ig_kit::geomkit::limb([x, y, bh], [x + sx, y, 0.0], 0.02, 0.012, true), &body);
                }}
            }
            Base::Frame => {
                // a square-tube sled at each end (and the middle of a long one): two posts, a floor rail
                let mut ys = vec![-hw + 0.05, hw - 0.05];
                if w > 1.6 { ys.push(0.0); }
                let s = 0.012;
                for y in ys {
                    add(&mut a, bx([0.03, y - s, 0.0], [d - 0.03, y + s, 2.0 * s]), &metal);
                    for x in [0.03, d - 0.03 - 2.0 * s] { add(&mut a, bx([x, y - s, 0.0], [x + 2.0 * s, y + s, bh]), &metal); }
                }
            }
        }
        a.supports.push(([0.0, -hw, h], [d, hw, h + 0.001]));
        a
    }
}

impl MediaConsole {
    fn proud(&self) -> f64 {
        // (pulls only on fronts: a console of open cubbies has none)
        let fronts = self.cols.iter().any(|c| *c != Col::Open);
        let p: f64 = if !fronts { 0.0 } else { match self.pull { Pull::Knob => 0.025, Pull::Bar => 0.036, Pull::Push => 0.001 } };
        p.max(if self.cols.contains(&Col::Slats) { 0.012 } else { 0.0 })
    }
}

impl Factory for MediaConsole {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> { Some([self.w, self.d, self.h]) }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> { Some(([0.0, -self.w / 2.0, 0.0], [self.d, self.w / 2.0, self.h])) }
    fn asset(&self, _i: u64) -> Asset { self.build() }
}
