//! WardrobeFactory (not in infinigen): a bedroom wardrobe -- a tall carcass on a plinth with 2-4 hinged doors (optionally over a row of
//! drawers) or two overlapping sliding doors, bar or knob handles, in wood / painted plywood finishes.
//! Frame: x = depth (front toward +x), y = width, z = height, origin at the back-left-bottom corner (as appliances.rs).
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "WardrobeFactory"
}

const BODY: Weighted = &[("wood.Wood", 3.0), ("wood.WhitePlywood", 2.5), ("wood.BlondePlywood", 1.5), ("wood.BlackPlywood", 0.8), ("plastic.PlasticRough", 0.7)];
const HANDLE_M: Weighted = &[("metal.BrushedMetal", 2.0), ("metal.MetalBasic", 1.0), ("metal.BrushedBlackMetal", 1.5), ("wood.Wood", 0.7)];

pub struct Wardrobe {
    depth: f64,
    width: f64,
    height: f64,
    doors: usize,
    sliding: bool,
    drawers: bool,
    knobs: bool,
    plinth: f64,
    contrast_doors: bool,
    seed: u64,
}

const GAP: f64 = 0.003;

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.6, 0.6, 0.62])
}

impl Wardrobe {
    fn new(r: &mut Rng) -> Wardrobe {
        let sliding = r.f64() < 0.3;
        let doors = if sliding { 2 } else { r.randint(2, 5) as usize };
        let width = if sliding { r.uniform(1.4, 2.2) } else { doors as f64 * r.uniform(0.42, 0.55) };
        Wardrobe {
            depth: r.uniform(0.55, 0.63),
            width,
            height: r.uniform(1.9, 2.25),
            doors,
            sliding,
            drawers: !sliding && r.f64() < 0.35,
            knobs: r.f64() < 0.35,
            plinth: r.uniform(0.04, 0.1),
            contrast_doors: r.f64() < 0.2,
            seed: r.randint(0, 1_000_000) as u64,
        }
    }

    fn build(&self) -> Asset {
        let mut a = Asset::default();
        let mut r = Rng::seeded(self.seed);
        let (d, w, h, p) = (self.depth, self.width, self.height, self.plinth);
        let s = self.seed;
        let body_cls = weighted(&mut r, BODY);
        let door_cls = if self.contrast_doors { weighted(&mut r, BODY) } else { body_cls };
        let body = spec("wardrobe_body", body_cls, s);
        let door = spec("wardrobe_door", door_cls, s + 1);
        let handle = spec("wardrobe_handle", weighted(&mut r, HANDLE_M), s + 2);
        let t = 0.02; // door thickness
        let x0 = d - t;
        let mut cab = Mesh::new();
        cab.append(&Mesh::cuboid([0.0, 0.0, p], [x0, w, h]));
        // recessed plinth
        cab.append(&Mesh::cuboid([0.0, 0.02, 0.0], [x0 - 0.04, w - 0.02, p]));
        let cb = cab.bounds();
        a.mesh(cab, &body, Some(cb));
        a.supports.push(([0.0, 0.0, h], [d, w, h + 0.001]));
        let panel = |a: &mut Asset, xa: f64, xb: f64, y0: f64, y1: f64, z0: f64, z1: f64| {
            let m = Mesh::cuboid([xa, y0 + GAP, z0 + GAP], [xb, y1 - GAP, z1 - GAP]);
            let b = m.bounds();
            a.mesh(m, &door, Some(b));
        };
        let off = 0.03;
        let vbar = |a: &mut Asset, x: f64, y: f64, zc: f64, len: f64| {
            let mut m = limb([x + off, y, zc - len / 2.0], [x + off, y, zc + len / 2.0], 0.007, 0.007, true);
            for z in [zc - len / 2.0 + 0.015, zc + len / 2.0 - 0.015] {
                m.append(&limb([x, y, z], [x + off, y, z], 0.005, 0.005, false));
            }
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let knob = |a: &mut Asset, x: f64, y: f64, z: f64| {
            let m = limb([x, y, z], [x + 0.025, y, z], 0.012, 0.012, true);
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let zc = p + (h - p) * 0.5;
        if self.sliding {
            // two panels, the left one in front, overlapping by 4 cm
            let half = w / 2.0;
            panel(&mut a, d + 0.001, d + 0.019, 0.0, half + 0.02, p, h);
            panel(&mut a, x0 + 0.001, d - 0.001, half - 0.02, w, p, h);
            // flat vertical pulls near the outer edges
            for (y, x) in [(0.05, d + 0.019), (w - 0.05, d - 0.001)] {
                let m = Mesh::cuboid([x, y - 0.012, zc - 0.2], [x + 0.006, y + 0.012, zc + 0.2]);
                let b = m.bounds();
                a.mesh(m, &handle, Some(b));
            }
            return a;
        }
        let zd = if self.drawers { p + (h - p) * 0.2 } else { p };
        if self.drawers {
            let n = (self.doors / 2).max(1);
            for i in 0..n {
                let (y0, y1) = (w * i as f64 / n as f64, w * (i + 1) as f64 / n as f64);
                panel(&mut a, x0, d, y0, y1, p, zd);
                let ym = (y0 + y1) / 2.0;
                let zm = (p + zd) / 2.0;
                if self.knobs {
                    knob(&mut a, d, ym, zm);
                } else {
                    let m = limb([d + off, ym - 0.08, zm], [d + off, ym + 0.08, zm], 0.007, 0.007, true);
                    let b = m.bounds();
                    a.mesh(m, &handle, Some(b));
                }
            }
        }
        let dw = w / self.doors as f64;
        let hz = zd + (h - zd) * 0.45;
        for i in 0..self.doors {
            let (y0, y1) = (dw * i as f64, dw * (i + 1) as f64);
            panel(&mut a, x0, d, y0, y1, zd, h);
            // handle on the opening edge: doors pair up (left door opens at its right edge)
            let y = if i % 2 == 0 { y1 - 0.04 } else { y0 + 0.04 };
            if self.knobs {
                knob(&mut a, d, y, hz);
            } else {
                vbar(&mut a, d, y, hz, 0.3);
            }
        }
        a
    }
}

impl Factory for Wardrobe {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([0.0, 0.0, 0.0], [self.depth + 0.045, self.width, self.height]))
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}

pub fn make(_c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Wardrobe::new(&mut rng.clone())))
}
