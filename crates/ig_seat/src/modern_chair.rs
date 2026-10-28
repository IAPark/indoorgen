//! ModernChair (not in infinigen): dining chairs built from real parts -- four tapered legs with stretchers, a seat (wooden board or
//! upholstered pad), and a back that is upholstered, slatted (3-5 vertical slats) or a curved top rail with spindles; optional arms.
//! Frame as infinigen's ChairFactory: x = depth (back legs near x = 0, front toward +x), y centred, z = 0 at the seat top, the floor at
//! z = -seat_h. Wood parts use the vertical-grain wood shader along their length; upholstery gets box uvs.
use super::modern_sofa::{box_uv, pad};
use super::*;
use crate::materials::*;
use ig_kit::geomkit::limb;
use serde_json::json;

const SEAT_FAB: Weighted = &[("fabric.SofaFabric", 2.0), ("fabric.Velvet", 1.0), ("fabric.Leather", 1.0), ("fabric.CoarseKnitFabric", 0.8)];
const FRAME: Weighted = &[("wood.Wood", 3.0), ("metal.BrushedBlackMetal", 0.8), ("plastic.PlasticRough", 0.4)];

#[derive(Clone, Copy, PartialEq)]
enum Back {
    Upholstered,
    Slats,
    Spindles,
}

pub struct ModernChair {
    w: f64,
    d: f64,
    seat_h: f64,
    back_h: f64,
    back: Back,
    padded_seat: bool,
    arms: bool,
    leg_r: f64,
    n_slats: usize,
    back_tilt: f64,
    seed: u64,
}

impl ModernChair {
    pub fn new(r: &mut Rng) -> ModernChair {
        let back = [Back::Upholstered, Back::Slats, Back::Spindles][r.choice_p(&[0.4, 0.35, 0.25])];
        ModernChair {
            w: r.uniform(0.42, 0.5),
            d: r.uniform(0.44, 0.52),
            seat_h: r.uniform(0.44, 0.48),
            back_h: r.uniform(0.36, 0.48),
            back,
            padded_seat: back == Back::Upholstered || r.f64() < 0.5,
            arms: r.f64() < 0.12,
            leg_r: r.uniform(0.016, 0.022),
            n_slats: r.randint(3, 6) as usize,
            back_tilt: r.uniform(0.08, 0.16),
            seed: r.randint(0, 1_000_000) as u64,
        }
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let frame_cls = weighted(&mut r, FRAME);
        let frame = if frame_cls == "wood.Wood" {
            spec_material(&format!("chair_wood:{}", self.seed), json!({"cls": "shader.wood.wood.shader_wood", "kwargs": {"vertical": true}, "seed": self.seed}), [0.4, 0.28, 0.17])
        } else {
            spec_material(&format!("chair_frame:{}", self.seed), json!({"cls": frame_cls, "kwargs": {}, "seed": self.seed}), [0.1, 0.1, 0.1])
        };
        let fab = spec_material(&format!("chair_fabric:{}", self.seed), json!({"cls": weighted(&mut r, SEAT_FAB), "kwargs": {}, "seed": self.seed + 1}), [0.5, 0.48, 0.45]);
        let (w, d, sh, bh) = (self.w, self.d, self.seat_h, self.back_h);
        let hw = w / 2.0;
        let lr = self.leg_r;
        let st = 0.035; // seat board thickness
        let mut a = Asset::default();
        let tilt = self.back_tilt;
        let back_x = |z: f64| -z * tilt; // back posts lean back with height
        // legs: back legs continue up into the back posts; front legs taper
        let ins = lr + 0.01;
        for y in [-hw + ins, hw - ins] {
            let front = limb([d - ins, y, -st], [d - ins - 0.01, y, -sh], lr, lr * 0.7, true);
            a.mesh(front, &frame, None);
            let back_leg = limb([ins + 0.02, y, -sh], [ins, y, -st], lr * 0.75, lr, true);
            a.mesh(back_leg, &frame, None);
            let post = limb([ins, y, -st], [ins + back_x(bh), y, bh], lr, lr * 0.85, true);
            a.mesh(post, &frame, None);
        }
        // stretchers (side and front) low between the legs
        let zs = -sh * 0.72;
        for y in [-hw + ins, hw - ins] {
            a.mesh(limb([ins + 0.02, y, zs], [d - ins - 0.008, y, zs], lr * 0.45, lr * 0.45, true), &frame, None);
        }
        a.mesh(limb([d - ins - 0.008, -hw + ins, zs + 0.06], [d - ins - 0.008, hw - ins, zs + 0.06], lr * 0.45, lr * 0.45, true), &frame, None);
        // seat apron + seat
        let apron = Mesh::cuboid([ins, -hw + ins, -st - 0.07], [d - ins, hw - ins, -st]);
        a.mesh(apron, &frame, None);
        if self.padded_seat {
            let mut board = Mesh::cuboid([0.0, -hw, -st], [d, hw, -st + 0.012]).smoothed(0.005, 1, 0);
            box_uv(&mut board);
            a.mesh(board, &frame, None);
            a.mesh(pad([0.005, -hw + 0.005, -st + 0.01], [d - 0.005, hw - 0.005, 0.0], 0.03, 2), &fab, Some(([0.0, -hw, -st], [d, hw, 0.0])));
        } else {
            let mut board = Mesh::cuboid([0.0, -hw, -st], [d, hw, 0.0]).smoothed(0.008, 2, 0);
            box_uv(&mut board);
            a.mesh(board, &frame, Some(([0.0, -hw, -st], [d, hw, 0.0])));
        }
        a.supports.push(([0.05, -hw + 0.05, 0.0], [d - 0.03, hw - 0.05, 0.001]));
        // back
        let (y0, y1) = (-hw + ins, hw - ins);
        let top_rail = |a: &mut Asset, z: f64, hgt: f64| {
            let x = ins + back_x(z);
            let mut m = Mesh::cuboid([x - 0.012, y0, z - hgt], [x + 0.012, y1, z]).smoothed(0.006, 1, 0);
            box_uv(&mut m);
            a.mesh(m, &frame, None);
        };
        match self.back {
            Back::Upholstered => {
                let (z0, z1) = (bh * 0.25, bh);
                let mut m = pad([ins - 0.03, y0 - 0.005, z0], [ins + 0.025, y1 + 0.005, z1], 0.03, 2);
                // lean with the posts
                for p in m.pos.iter_mut() { p[0] += back_x(p[2]); }
                a.mesh(m, &fab, None);
            }
            Back::Slats => {
                top_rail(a.by_ref(), bh, 0.06);
                top_rail(a.by_ref(), bh * 0.3, 0.035);
                let n = self.n_slats;
                for i in 0..n {
                    let yc = y0 + (y1 - y0) * (i as f64 + 1.0) / (n as f64 + 1.0);
                    let sw = ((y1 - y0) / (n as f64 + 1.0) * 0.55).min(0.05);
                    let (za, zb) = (bh * 0.3, bh - 0.06);
                    let mut m = Mesh::cuboid([-0.008, yc - sw / 2.0, za], [0.008, yc + sw / 2.0, zb]).smoothed(0.003, 1, 0);
                    for p in m.pos.iter_mut() { p[0] += ins + back_x(p[2]); }
                    box_uv(&mut m);
                    a.mesh(m, &frame, None);
                }
            }
            Back::Spindles => {
                // curved top rail (bowed backwards) + round spindles
                let n = 12;
                let mut rail = Mesh::new();
                for i in 0..n {
                    let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                    let ya = y0 - 0.01 + (y1 - y0 + 0.02) * t0;
                    let yb = y0 - 0.01 + (y1 - y0 + 0.02) * t1;
                    let bow = |t: f64| -0.04 * (1.0 - (2.0 * t - 1.0).powi(2));
                    let xa = ins + back_x(bh) + bow(t0);
                    let xb = ins + back_x(bh) + bow(t1);
                    rail.append(&limb([xa, ya, bh - 0.03], [xb, yb, bh - 0.03], 0.03, 0.03, false));
                }
                box_uv(&mut rail);
                a.mesh(rail, &frame, None);
                for i in 0..5 {
                    let t = (i as f64 + 1.0) / 6.0;
                    let y = y0 + (y1 - y0) * t;
                    let bow = -0.04 * (1.0 - (2.0 * t - 1.0).powi(2));
                    let m = limb([ins + 0.01 + bow * 0.3, y, 0.0], [ins + back_x(bh - 0.05) + bow, y, bh - 0.05], 0.008, 0.007, true);
                    a.mesh(m, &frame, None);
                }
            }
        }
        if self.arms {
            for y in [-hw + ins, hw - ins] {
                a.mesh(limb([d - ins - 0.02, y, -st], [d - ins - 0.03, y, 0.22], lr * 0.8, lr * 0.8, true), &frame, None);
                a.mesh(limb([ins + back_x(0.22), y, 0.22], [d - ins + 0.01, y, 0.22], lr * 0.9, lr * 0.9, true), &frame, None);
            }
        }
        a
    }
}

trait ByRef { fn by_ref(&mut self) -> &mut Self; }
impl ByRef for Asset { fn by_ref(&mut self) -> &mut Self { self } }

impl Factory for ModernChair {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([-(self.back_h * self.back_tilt) - 0.08, -self.w / 2.0 - 0.01, -self.seat_h], [self.d + 0.01, self.w / 2.0 + 0.01, self.back_h + 0.01]))
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (lo, hi) = self.bounds_hint(0).unwrap();
        let mut a = Asset::default();
        a.cuboid(lo, hi, &Material::diffuse("chair_ph", [0.5; 3], 0.8));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}
