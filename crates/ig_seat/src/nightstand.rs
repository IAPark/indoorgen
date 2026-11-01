//! Nightstand (not in infinigen): side tables as bedside cabinets come -- a small carcass under an overhanging top with a drawer over
//! an open shelf, two or three drawers, or a drawer over a door, on tapered legs, a recessed plinth or short block feet, with knobs,
//! bar pulls or finger grooves. Frame as tables.rs / ModernTable: centred on x / y, z = 0 on the floor; the fronts face +x (a
//! nightstand's back stands on the headboard line, its front toward the foot of the bed).
use super::*;
use crate::materials::*;
use ig_kit::geomkit::limb;
use serde_json::json;

const BODY: Weighted = &[("wood.Wood", 3.0), ("wood.WhitePlywood", 2.0), ("wood.BlackPlywood", 0.8), ("wood.BlondePlywood", 1.5), ("plastic.PlasticRough", 0.4)];
const METAL: Weighted = &[("metal.BrushedMetal", 1.5), ("metal.BrushedBlackMetal", 1.5), ("metal.MetalBasic", 0.8)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Fronts { DrawerShelf, Drawers(usize), DrawerDoor }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Base { Legs, Plinth, Feet }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pull { Knob, Bar, Groove }

pub struct Nightstand {
    hx: f64,
    hy: f64,
    h: f64,
    fronts: Fronts,
    base: Base,
    pull: Pull,
    /// the door (drawer over a door) hinges on the +y side (else -y)
    hinge_pos: bool,
    seed: u64,
}

fn bx(lo: [f64; 3], hi: [f64; 3]) -> Mesh { Mesh::cuboid(lo, hi) }

impl Nightstand {
    pub fn new(r: &mut Rng) -> Nightstand {
        let (mut hx, mut hy, mut h) = (r.uniform(0.17, 0.22), r.uniform(0.2, 0.28), r.uniform(0.5, 0.62));
        if let Some(sz) = crate::api::size_hint() { hy = sz[0] / 2.0; hx = sz[1] / 2.0; h = sz[2]; }
        let fronts = [Fronts::DrawerShelf, Fronts::Drawers(2), Fronts::Drawers(3), Fronts::DrawerDoor][r.choice_p(&[0.35, 0.3, 0.15, 0.2])];
        let base = [Base::Legs, Base::Plinth, Base::Feet][r.choice_p(&[0.45, 0.3, 0.25])];
        let pull = [Pull::Knob, Pull::Bar, Pull::Groove][r.choice_p(&[0.4, 0.35, 0.25])];
        let seed = r.randint(0, 1_000_000) as u64;
        Nightstand { hx, hy, h, fronts, base, pull, hinge_pos: crate::int_hash(&["nightstand_hinge", &seed.to_string()]) % 2 == 0, seed }
    }

    /// the front's x (the pulls stand proud of it up to hx), the carcass's back and front x, the base height, the carcass half width
    fn frame(&self) -> (f64, f64, f64, f64, f64) {
        let proud = match self.pull { Pull::Knob => 0.025, Pull::Bar => 0.03, Pull::Groove => 0.0 };
        let xf = self.hx - proud;
        let bh = match self.base { Base::Legs => (self.h * 0.3).clamp(0.12, 0.2), Base::Plinth => 0.06, Base::Feet => 0.035 };
        (xf, -self.hx + 0.01, xf - 0.018, bh, self.hy - 0.012)
    }

    /// the fronts' z ranges (bottom to top) and whether each is a drawer (else the door)
    fn fronts_z(&self) -> Vec<(f64, f64, bool)> {
        let (_, _, _, bh, _) = self.frame();
        let bt = 0.016;
        let (fz0, fz1) = (bh + bt, self.h - 0.025);
        let span = fz1 - fz0;
        match self.fronts {
            Fronts::DrawerShelf => { let zd = fz1 - (span * 0.38).clamp(0.11, 0.17); vec![(zd, fz1, true)] }
            Fronts::Drawers(n) => (0..n).map(|i| (fz0 + span * i as f64 / n as f64, fz0 + span * (i + 1) as f64 / n as f64, true)).collect(),
            Fronts::DrawerDoor => { let zd = fz1 - (span * 0.3).clamp(0.1, 0.15); vec![(fz0, zd, false), (zd, fz1, true)] }
        }
    }

    /// collision: the body (top, carcass, base, pulls) and an opening per drawer (pulled out 3/4 of its depth) and door (100 degrees)
    fn vols(&self) -> Vec<crate::piece::Volume> {
        use crate::piece::{Leaf, Volume};
        let (hx, hy, h) = (self.hx, self.hy, self.h);
        let (xf, x0, x1, _, cy) = self.frame();
        // (the tapered legs' tips end 1 mm under the floor)
        let mut v = vec![Volume::hard([-hx, -hy, -0.002], [hx, hy, h])];
        v.push(Volume::support([-hx, -hy, h - 0.001], [xf.max(hx - 0.01), hy, h]));
        let (fy0, fy1) = (-cy + 0.016, cy - 0.016);
        for (z0, z1, drawer) in self.fronts_z() {
            if drawer {
                let panel = ([x0 + 0.01, fy0, z0], [hx, fy1, z1]);
                v.push(Volume::opening(Leaf::Drawer { panel, dir: [1.0, 0.0, 0.0], travel: 0.75 * (x1 - x0) }));
            } else {
                // a door on a vertical hinge at its front corner, swinging out
                let (hy_, ax) = if self.hinge_pos { (fy1, [0.0, 0.0, -1.0]) } else { (fy0, [0.0, 0.0, 1.0]) };
                let panel = ([x1, fy0, z0], [hx, fy1, z1]);
                v.push(Volume::opening(Leaf::Hinged { panel, hinge: [xf, hy_, z0], axis: ax, max_angle: 100f64.to_radians() }));
            }
        }
        v
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let bc = weighted(&mut r, BODY);
        let body = spec_material("nightstand_body", json!({"cls": bc, "kwargs": {}, "seed": self.seed}), [0.5, 0.36, 0.22]);
        // 25 % painted (own stream, NO_PAINT=1: off)
        let body = if std::env::var_os("NO_PAINT").is_none() && crate::int_hash(&["nightstand_paint", &self.seed.to_string()]) % 100 < 25 { painted_material(&format!("nightstand_paint:{}", self.seed), self.seed, [0.5, 0.36, 0.22]) } else { body };
        let front = if r.f64() < 0.3 { spec_material("nightstand_front", json!({"cls": weighted(&mut r, BODY), "kwargs": {}, "seed": self.seed + 1}), [0.85, 0.84, 0.8]) } else { body.clone() };
        let metal = spec_material("nightstand_metal", json!({"cls": weighted(&mut r, METAL), "kwargs": {}, "seed": self.seed + 2}), [0.6, 0.6, 0.6]);
        let (hx, hy, h) = (self.hx, self.hy, self.h);
        // what stands proud of the fronts: the carcass and fronts sit back by it, so the piece keeps to its box
        let proud = match self.pull { Pull::Knob => 0.025, Pull::Bar => 0.03, Pull::Groove => 0.0 };
        let xf = hx - proud; // the face of the fronts
        let tt = 0.025; // top thickness
        let zt = h - tt;
        let bh = match self.base { Base::Legs => (h * 0.3).clamp(0.12, 0.2), Base::Plinth => 0.06, Base::Feet => 0.035 };
        let (cy, bt) = (hy - 0.012, 0.016); // carcass half width (the top overhangs it 12 mm), board thickness
        let mut a = Asset::default();
        let add = |a: &mut Asset, m: Mesh, mat: &Material| { let b = m.bounds(); a.mesh(m, mat, Some(b)); };
        // top over the full box (the pulls' depth taken off its front only when it would stand proud of them)
        add(&mut a, bx([-hx, -hy, zt], [xf.max(hx - 0.01), hy, h]), &body);
        // carcass: sides, back, bottom
        let (x0, x1) = (-hx + 0.01, xf - 0.018);
        for y in [-cy, cy - bt] { add(&mut a, bx([x0, y, bh], [x1 + 0.018, y + bt, zt]), &body); }
        add(&mut a, bx([x0, -cy + bt, bh], [x0 + 0.008, cy - bt, zt]), &body);
        add(&mut a, bx([x0, -cy + bt, bh], [x1, cy - bt, bh + bt]), &body);
        // fronts (3 mm reveals) between the bottom board and the top
        let (fz0, fz1, fy0, fy1) = (bh + bt, zt, -cy + bt, cy - bt);
        let g = 0.003;
        let pulls = std::cell::RefCell::new(Vec::<Mesh>::new());
        let front_at = |a: &mut Asset, z0: f64, z1: f64, drawer: bool| {
            add(a, bx([x1, fy0 + g, z0 + g], [xf, fy1 - g, z1 - g]), &front);
            let zc = if drawer { (z0 + z1) / 2.0 } else { z1 - 0.06 };
            match self.pull {
                Pull::Knob => pulls.borrow_mut().push(limb([xf, 0.0, zc], [xf + 0.025, 0.0, zc], 0.009, 0.012, true)),
                Pull::Bar => {
                    let l = (fy1 - fy0 - 0.08).min(0.1);
                    let mut m = limb([xf + 0.024, -l / 2.0, zc], [xf + 0.024, l / 2.0, zc], 0.005, 0.005, true);
                    for y in [-l / 2.0, l / 2.0] { m.append(&limb([xf, y, zc], [xf + 0.024, y, zc], 0.004, 0.004, true)); }
                    pulls.borrow_mut().push(m);
                }
                // a dark groove along the top edge of the front
                Pull::Groove => add(a, bx([xf - 0.004, fy0 + 0.02, z1 - g - 0.016], [xf + 0.0005, fy1 - 0.02, z1 - g - 0.01]), &metal),
            }
        };
        let span = fz1 - fz0;
        match self.fronts {
            Fronts::DrawerShelf => {
                let zd = fz1 - (span * 0.38).clamp(0.11, 0.17);
                front_at(&mut a, zd, fz1, true);
                // an open shelf below: the drawer's bottom board, and a shelf halfway when there is room
                add(&mut a, bx([x0 + 0.008, fy0, zd - bt], [x1, fy1, zd]), &body);
                if zd - fz0 > 0.24 { add(&mut a, bx([x0 + 0.008, fy0, (fz0 + zd) / 2.0 - bt / 2.0], [x1, fy1, (fz0 + zd) / 2.0 + bt / 2.0]), &body); }
            }
            Fronts::Drawers(n) => {
                for i in 0..n { front_at(&mut a, fz0 + span * i as f64 / n as f64, fz0 + span * (i + 1) as f64 / n as f64, true); }
            }
            Fronts::DrawerDoor => {
                let zd = fz1 - (span * 0.3).clamp(0.1, 0.15);
                front_at(&mut a, zd, fz1, true);
                front_at(&mut a, fz0, zd, false);
            }
        }
        for m in pulls.into_inner() { add(&mut a, m, if self.pull == Pull::Knob && r.f64() < 0.3 { &body } else { &metal }); }
        // base
        match self.base {
            Base::Legs => {
                let lr = 0.016;
                for x in [x0 + 0.03, x1 - 0.02] { for y in [-cy + 0.03, cy - 0.03] {
                    let (sx, sy) = (if x > 0.0 { 0.012 } else { -0.012 }, if y > 0.0 { 0.012 } else { -0.012 });
                    add(&mut a, limb([x, y, bh], [x + sx, y + sy, 0.0], lr, lr * 0.6, true), &body);
                }}
            }
            Base::Plinth => add(&mut a, bx([x0 + 0.02, -cy + 0.02, 0.0], [x1 - 0.02, cy - 0.02, bh]), &body),
            Base::Feet => for x in [x0 + 0.02, x1 - 0.05] { for y in [-cy + 0.02, cy - 0.05] { add(&mut a, bx([x, y, 0.0], [x + 0.03, y + 0.03, bh]), &body); } },
        }
        a.supports.push(([-hx, -hy, h - 0.001], [xf.max(hx - 0.01), hy, h]));
        a
    }
}

impl Factory for Nightstand {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> { Some([2.0 * self.hy, 2.0 * self.hx, self.h]) }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> { Some(([-self.hx, -self.hy, 0.0], [self.hx, self.hy, self.h])) }
    fn asset(&self, _i: u64) -> Asset { self.build() }
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> { Some(self.vols()) }
}
