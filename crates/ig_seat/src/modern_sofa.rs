//! ModernSofa (not in infinigen): a sofa / armchair built from separate upholstered parts, the way real ones (and 3D-FRONT's models) are:
//! a frame on a recessed plinth, plump seat cushions with gaps between them, back cushions leaning back, track / rolled / slim arms, and
//! wood / block / metal legs. Every part is bevelled and subdivided (rounded, slightly puffed edges) and gets box-projected UVs in metres,
//! so knit / sofa fabrics map their weave cleanly. Frame as infinigen's SofaFactory: x = depth (0 at the back, front at +x), y centred,
//! z = 0 at the bottom of the frame, legs below it.
use super::*;
use crate::materials::*;
use ig_kit::geomkit::limb;
use serde_json::json;

const FAB: Weighted = &[("fabric.SofaFabric", 2.0), ("fabric.CoarseKnitFabric", 1.2), ("fabric.FineKnitFabric", 1.0), ("fabric.Velvet", 0.8), ("fabric.Leather", 0.6)];
const LEG: Weighted = &[("wood.Wood", 2.0), ("metal.BrushedBlackMetal", 1.0), ("metal.MetalBasic", 0.6)];

#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Track,
    Rolled,
    Slim,
}

#[derive(Clone, Copy, PartialEq)]
enum Legs {
    Tapered,
    Block,
    Metal,
}

pub struct ModernSofa {
    width: f64,
    depth: f64,
    height: f64,
    seat_h: f64,
    arm_h: f64,
    arm_w: f64,
    arm: Arm,
    legs: Legs,
    leg_h: f64,
    n_seats: usize,
    cushion_t: f64,
    back_t: f64,
    back_tilt: f64,
    plinth: f64,
    seed: u64,
}

/// box-projected uv (metres) per corner; `m` must have per-face vertices (as `Mesh::smoothed` returns)
fn box_uv(m: &mut Mesh) {
    m.uv = vec![[0.0, 0.0]; m.pos.len()];
    for f in &m.faces {
        if f.len() < 3 { continue }
        let (a, b, c) = (m.pos[f[0] as usize], m.pos[f[1] as usize], m.pos[f[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let ax = (0..3).max_by(|&i, &j| n[i].abs().partial_cmp(&n[j].abs()).unwrap()).unwrap();
        for &i in f {
            let p = m.pos[i as usize];
            m.uv[i as usize] = match ax { 0 => [p[1], p[2]], 1 => [p[0], p[2]], _ => [p[0], p[1]] };
        }
    }
}

/// rounded upholstered block: cuboid, bevelled edges, subdivided (puffed), box uvs
fn pad(lo: V3, hi: V3, bevel: f64, levels: usize) -> Mesh {
    let b = bevel.min(0.45 * (hi[0] - lo[0]).min(hi[1] - lo[1]).min(hi[2] - lo[2]));
    let mut m = Mesh::cuboid(lo, hi).smoothed(b, 2, levels);
    box_uv(&mut m);
    m
}

fn rot_about_y(m: &mut Mesh, pivot: V3, a: f64) {
    let (c, s) = (a.cos(), a.sin());
    for p in m.pos.iter_mut() {
        let (x, z) = (p[0] - pivot[0], p[2] - pivot[2]);
        p[0] = pivot[0] + c * x + s * z;
        p[2] = pivot[2] - s * x + c * z;
    }
}

impl ModernSofa {
    pub fn new(r: &mut Rng, armchair: bool) -> ModernSofa {
        let arm = [Arm::Track, Arm::Rolled, Arm::Slim][r.choice_p(&[0.45, 0.25, 0.3])];
        let arm_w = match arm { Arm::Slim => r.uniform(0.07, 0.12), Arm::Track => r.uniform(0.14, 0.24), Arm::Rolled => r.uniform(0.16, 0.24) };
        let width = if armchair { r.uniform(0.8, 1.0) } else { r.uniform(1.6, 2.6) };
        let arm_w = if armchair { arm_w.min(0.16) } else { arm_w };
        let inner = width - 2.0 * arm_w;
        let n_seats = if armchair { 1 } else { ((inner / 0.68).round() as usize).clamp(2, 4) };
        ModernSofa {
            width,
            depth: r.uniform(0.85, 1.0),
            height: r.uniform(0.76, 0.9),
            seat_h: r.uniform(0.42, 0.47),
            arm_h: r.uniform(0.56, 0.68),
            arm_w,
            arm,
            legs: [Legs::Tapered, Legs::Block, Legs::Metal][r.choice_p(&[0.5, 0.25, 0.25])],
            leg_h: r.uniform(0.06, 0.16),
            n_seats,
            cushion_t: r.uniform(0.12, 0.18),
            back_t: r.uniform(0.16, 0.24),
            back_tilt: r.uniform(0.15, 0.25),
            plinth: r.uniform(0.0, 0.03),
            seed: r.randint(0, 1_000_000) as u64,
        }
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let fab = spec_material("sofa_fabric", json!({"cls": weighted(&mut r, FAB), "kwargs": {}, "seed": self.seed}), [0.55, 0.53, 0.5]);
        let legm = spec_material("sofa_leg", json!({"cls": weighted(&mut r, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.25, 0.18, 0.12]);
        let (w, d, h) = (self.width, self.depth, self.height);
        let (hw, aw, lh) = (w / 2.0, self.arm_w, self.leg_h);
        // z = 0 at the floor of the frame: frame bottom at lh above the floor; everything below z = lh is legs (shifted at the end)
        let z0 = lh;
        let seat_top = self.seat_h;
        let deck = seat_top - self.cushion_t; // top of the frame deck under the seat cushions
        let back_frame_d = 0.12;
        let mut a = Asset::default();
        let mut add = |a: &mut Asset, m: Mesh| { let b = m.bounds(); a.mesh(m, &fab, Some(b)); };
        // base frame (slightly recessed plinth band under it)
        add(&mut a, pad([0.0, -hw, z0 + self.plinth], [d, hw, deck], 0.02, 1));
        // back frame
        add(&mut a, pad([0.0, -hw + aw * 0.5, deck - 0.02], [back_frame_d, hw - aw * 0.5, h - 0.12], 0.03, 1));
        // arms
        for side in [-1.0f64, 1.0] {
            let (y0, y1) = if side < 0.0 { (-hw, -hw + aw) } else { (hw - aw, hw) };
            match self.arm {
                Arm::Track | Arm::Slim => add(&mut a, pad([0.0, y0, z0 + self.plinth], [d, y1, self.arm_h], if self.arm == Arm::Slim { 0.02 } else { 0.04 }, 2)),
                Arm::Rolled => {
                    // body plus a wider, fully rounded top that overhangs outwards (the "roll")
                    add(&mut a, pad([0.0, y0, z0 + self.plinth], [d - 0.02, y1, self.arm_h - 0.08], 0.03, 1));
                    let over = 0.03;
                    let (ry0, ry1) = if side < 0.0 { (y0 - over, y1) } else { (y0, y1 + over) };
                    add(&mut a, pad([0.0, ry0, self.arm_h - 0.16], [d, ry1, self.arm_h], 0.5 * (ry1 - ry0).min(0.16), 2));
                }
            }
        }
        // seat cushions
        let (iy0, iy1) = (-hw + aw, hw - aw);
        let cw = (iy1 - iy0) / self.n_seats as f64;
        let gap = 0.008;
        let cx0 = back_frame_d + self.back_t * 0.55;
        for i in 0..self.n_seats {
            let y0 = iy0 + cw * i as f64 + gap;
            let y1 = iy0 + cw * (i + 1) as f64 - gap;
            add(&mut a, pad([cx0, y0, deck], [d + 0.01, y1, seat_top], 0.055, 2));
        }
        // back cushions, leaning back about their bottom edge
        for i in 0..self.n_seats {
            let y0 = iy0 + cw * i as f64 + gap;
            let y1 = iy0 + cw * (i + 1) as f64 - gap;
            let mut m = pad([back_frame_d, y0, seat_top - 0.01], [back_frame_d + self.back_t, y1, h], 0.06, 2);
            rot_about_y(&mut m, [back_frame_d + self.back_t, 0.0, seat_top], -self.back_tilt);
            add(&mut a, m);
        }
        // legs (4, plus 2 middle legs on long sofas)
        let inset = 0.06;
        let mut legs_xy = vec![[inset, -hw + inset], [inset, hw - inset], [d - inset, -hw + inset], [d - inset, hw - inset]];
        if w > 2.0 { legs_xy.extend([[inset, 0.0], [d - inset, 0.0]]); }
        for [x, y] in legs_xy {
            let m = match self.legs {
                Legs::Tapered => limb([x, y, z0 + 0.01], [x + if x > d / 2.0 { 0.015 } else { -0.015 }, y, 0.0], 0.022, 0.013, true),
                Legs::Block => Mesh::cuboid([x - 0.03, y - 0.03, 0.0], [x + 0.03, y + 0.03, z0 + 0.01]),
                Legs::Metal => limb([x, y, z0 + 0.01], [x, y, 0.0], 0.008, 0.008, true),
            };
            let b = m.bounds();
            a.mesh(m, &legm, Some(b));
        }
        // to infinigen's frame: legs below z = 0
        for p in a.parts.iter_mut() {
            for q in p.mesh.pos.iter_mut() { q[2] -= lh; }
            if let Some(c) = p.col.as_mut() { c.0[2] -= lh; c.1[2] -= lh; }
        }
        a.supports.push(([cx0, iy0, seat_top - lh], [d, iy1, seat_top - lh + 0.001]));
        a
    }
}

impl Factory for ModernSofa {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([0.0, -self.width / 2.0, -self.leg_h], [self.depth + 0.02, self.width / 2.0, self.height - self.leg_h + 0.01]))
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (lo, hi) = self.bounds_hint(0).unwrap();
        let mut a = Asset::default();
        a.cuboid(lo, hi, &Material::diffuse("sofa_ph", [0.5; 3], 0.8));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}
