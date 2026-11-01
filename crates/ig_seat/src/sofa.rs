//! Sofas and armchairs as one piece, built from separate upholstered parts the way real ones (and 3D-FRONT's models) are: a frame on a
//! recessed plinth, plump seat cushions with gaps between them (or one bench cushion), a back of loose cushions, a tight panel, channels
//! or buttons leaning back, track / rolled / slim / tuxedo arms or none, tapered / block / metal / bun / hairpin legs or a skirt, and on
//! wide sofas sometimes a chaise (the seat at one end running on forward: an L-shaped sofa). Every part is bevelled and subdivided
//! (rounded, slightly puffed edges) with box-projected UVs in metres, so knit / sofa fabrics map their weave cleanly. Its collision is
//! written from these parameters (one box, a second for a chaise). Frame: x = depth (0 at the back, front at +x), y centred, z = 0 at
//! the bottom of the frame, legs below it.
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
    /// arms level with the back (a tuxedo sofa)
    Tuxedo,
    /// no arms (a settee / slipper chair)
    None,
}

#[derive(Clone, Copy, PartialEq)]
enum Legs {
    Tapered,
    Block,
    Metal,
    /// turned bun feet
    Bun,
    /// a fabric skirt to the floor (no legs showing)
    Skirt,
    /// thin hairpin (V) legs
    Hairpin,
}

#[derive(Clone, Copy, PartialEq)]
enum Back {
    /// loose back cushions, one per seat
    Pillow,
    /// one tight upholstered back panel
    Tight,
    /// vertical channels (channel tufting)
    Channel,
    /// a tight back with a grid of buttons (Chesterfield-like)
    Tufted,
}

pub fn handles(c: &str) -> bool {
    matches!(c, "SofaFactory" | "ArmChairFactory")
}

pub struct Sofa {
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
    back: Back,
    /// one long seat cushion instead of one per seat
    bench: bool,
    /// a chaise: the seat at one end (-1 / +1 side of y) running on forward by this much past the front
    chaise: Option<(f64, f64)>,
    seed: u64,
}

/// box-projected uv (metres) per corner; `m` must have per-face vertices (as `Mesh::smoothed` returns)
pub fn box_uv(m: &mut Mesh) {
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
pub fn pad(lo: V3, hi: V3, bevel: f64, levels: usize) -> Mesh {
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

impl Sofa {
    pub fn new(r: &mut Rng, armchair: bool) -> Sofa {
        let arm = [Arm::Track, Arm::Rolled, Arm::Slim][r.choice_p(&[0.45, 0.25, 0.3])];
        let arm_w = match arm { Arm::Slim => r.uniform(0.07, 0.12), Arm::Track => r.uniform(0.14, 0.24), Arm::Rolled => r.uniform(0.16, 0.24), Arm::Tuxedo | Arm::None => 0.0 };
        let width = if armchair { r.uniform(0.8, 1.0) } else { r.uniform(1.6, 2.6) };
        let arm_w = if armchair { arm_w.min(0.16) } else { arm_w };
        let mut depth = r.uniform(0.85, 1.0);
        let mut height = r.uniform(0.76, 0.9);
        let mut seat_h = r.uniform(0.42, 0.47);
        let mut arm_h = r.uniform(0.56, 0.68);
        let legs = [Legs::Tapered, Legs::Block, Legs::Metal][r.choice_p(&[0.5, 0.25, 0.25])];
        let leg_h = r.uniform(0.06, 0.16);
        let cushion_t = r.uniform(0.12, 0.18);
        let back_t = r.uniform(0.16, 0.24);
        let back_tilt = r.uniform(0.15, 0.25);
        let plinth = r.uniform(0.0, 0.03);
        let seed = r.randint(0, 1_000_000) as u64;
        // styles beyond the first three arm / leg kinds, from their own stream (the draws above keep their values): back style, a
        // bench seat, tuxedo / armless arms, bun / skirted / hairpin bases
        let mut q = Rng::seeded(seed ^ 0x5eed_50fa);
        // (NO_SOFA_STYLES=1: the original pillow back / three arms / three leg kinds only)
        let plain = std::env::var_os("NO_SOFA_STYLES").is_some();
        let back = if plain { Back::Pillow } else { [Back::Pillow, Back::Tight, Back::Channel, Back::Tufted][q.choice_p(&[0.5, 0.15, 0.2, 0.15])] };
        let bench = !plain && q.f64() < 0.3;
        let ua = q.f64();
        // (tuxedo arms on sofas only: an armchair beside a window raised its arm into the window's zone)
        let (arm, arm_w) = if plain { (arm, arm_w) } else if ua < 0.14 && !armchair { (Arm::Tuxedo, q.uniform(0.12, 0.2)) } else if ua < 0.2 { (Arm::None, 0.0) } else { (arm, arm_w) };
        let ul = q.f64();
        let legs = if plain { legs } else if ul < 0.12 { Legs::Bun } else if ul < 0.22 { Legs::Skirt } else if ul < 0.3 { Legs::Hairpin } else { legs };
        let leg_h = if legs == Legs::Skirt { leg_h.min(0.08) } else { leg_h };
        let mut width = width;
        // built to the layout box [width (y), depth (x), height]: the overall extents are the box's (rolled arms overhang, the seat
        // front and the cushion bulge stand a little proud); heights of the seat and arms follow the total height
        if let Some(h) = crate::api::size_hint() {
            let hs = height;
            width = h[0] - if arm == Arm::Rolled { 0.06 } else { 0.0 };
            depth = h[1] - 0.01;
            // total height = top of the tilted back cushions (see `top_z`); seat and arm heights keep their proportion of it
            let cs = back_tilt.cos();
            height = (h[2] + CUSHION_SHRINK) / (seat_h * (1.0 - cs) / hs + cs);
            seat_h *= height / hs;
            arm_h *= height / hs;
        }
        let inner = width - 2.0 * arm_w;
        let n_seats = if armchair { 1 } else { ((inner / 0.68).round() as usize).clamp(2, 4) };
        // a quarter of wide sofas have a chaise at one end (its own stream; not on a skirted base); the sofa is that much deeper
        let mut cq = Rng::seeded(seed ^ 0xc4a1_5e00);
        let chaise = if !armchair && !plain && width >= 2.0 && legs != Legs::Skirt && cq.f64() < 0.25 { Some((if cq.f64() < 0.5 { -1.0 } else { 1.0 }, cq.uniform(0.55, 0.75))) } else { None };
        Sofa { width, depth, height, seat_h, arm_h, arm_w, arm, legs, leg_h, n_seats, cushion_t, back_t, back_tilt, plinth, back, bench, chaise, seed }
    }

    /// the chaise's y range (the end seat section, arm included) and its front x
    fn chaise_span(&self) -> Option<(f64, f64, f64)> {
        let (side, ext) = self.chaise?;
        let hw = self.width / 2.0;
        let (iy0, iy1) = (-hw + self.arm_w, hw - self.arm_w);
        let cw = (iy1 - iy0) / self.n_seats as f64;
        Some(if side > 0.0 { (iy1 - cw, hw, self.depth + ext) } else { (-hw, iy0 + cw, self.depth + ext) })
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
                Arm::None => {}
                Arm::Track | Arm::Slim => add(&mut a, pad([0.0, y0, z0 + self.plinth], [d, y1, self.arm_h], if self.arm == Arm::Slim { 0.02 } else { 0.04 }, 2)),
                // level with the top of the back, squared off
                Arm::Tuxedo => add(&mut a, pad([0.0, y0, z0 + self.plinth], [d, y1, self.top_z() - 0.01], 0.03, 2)),
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
        let chaise = self.chaise_span();
        if self.bench {
            add(&mut a, pad([cx0, iy0 + gap, deck], [d + 0.01, iy1 - gap, seat_top], 0.055, 2));
        } else {
            for i in 0..self.n_seats {
                let y0 = iy0 + cw * i as f64 + gap;
                let y1 = iy0 + cw * (i + 1) as f64 - gap;
                // (the chaise's end cushion runs on to its front)
                let front = match chaise { Some((c0, c1, xf)) if y0 > c0 - 1e-6 && y1 < c1 + 1e-6 => xf, _ => d } + 0.01;
                add(&mut a, pad([cx0, y0, deck], [front, y1, seat_top], 0.055, 2));
            }
        }
        if let Some((c0, c1, xf)) = chaise {
            // its base runs on under it, arm side included (a bench sofa: its own cushion on it)
            add(&mut a, pad([d - 0.04, c0, z0 + self.plinth], [xf, c1, deck], 0.02, 1));
            let (s0, s1) = (c0.max(iy0), c1.min(iy1));
            if self.bench { add(&mut a, pad([d, s0 + gap, deck], [xf + 0.01, s1 - gap, seat_top], 0.055, 2)); }
        }
        // the back, leaning back about its bottom edge: loose cushions, one tight panel, vertical channels, or a buttoned panel
        let pivot = [back_frame_d + self.back_t, 0.0, seat_top];
        match self.back {
            Back::Pillow => for i in 0..self.n_seats {
                let y0 = iy0 + cw * i as f64 + gap;
                let y1 = iy0 + cw * (i + 1) as f64 - gap;
                let mut m = pad([back_frame_d, y0, seat_top - 0.01], [back_frame_d + self.back_t, y1, h], 0.06, 2);
                rot_about_y(&mut m, pivot, -self.back_tilt);
                add(&mut a, m);
            },
            Back::Tight | Back::Tufted => {
                let mut m = pad([back_frame_d + self.back_t * 0.4, iy0, seat_top - 0.01], [back_frame_d + self.back_t, iy1, h], 0.04, 2);
                rot_about_y(&mut m, pivot, -self.back_tilt);
                add(&mut a, m);
                if self.back == Back::Tufted {
                    // buttons on the panel's front face: rows 0.14 m apart, columns ~0.17 m, staggered (diamond tufting)
                    let rows = (((h - seat_top) - 0.12) / 0.14).floor().max(1.0) as usize;
                    let cols = (((iy1 - iy0) - 0.1) / 0.17).floor().max(1.0) as usize;
                    for rI in 0..rows {
                        let z = seat_top + 0.1 + 0.14 * rI as f64;
                        let off = if rI % 2 == 1 { 0.5 } else { 0.0 };
                        for cI in 0..cols {
                            let t = (cI as f64 + 0.5 + off) / cols as f64;
                            if t >= 1.0 { continue }
                            let y = iy0 + 0.05 + (iy1 - iy0 - 0.1) * t;
                            let x = back_frame_d + self.back_t * 0.4 - 0.004;
                            let mut b = limb([x + 0.012, y, z], [x - 0.006, y, z], 0.011, 0.009, true);
                            rot_about_y(&mut b, pivot, -self.back_tilt);
                            add(&mut a, b);
                        }
                    }
                }
            }
            Back::Channel => {
                let n = (((iy1 - iy0) / 0.17).round() as usize).max(3);
                let w0 = (iy1 - iy0) / n as f64;
                for i in 0..n {
                    let y0 = iy0 + w0 * i as f64 + 0.003;
                    let mut m = pad([back_frame_d + self.back_t * 0.35, y0, seat_top - 0.01], [back_frame_d + self.back_t, y0 + w0 - 0.006, h], 0.5 * w0.min(0.08), 2);
                    rot_about_y(&mut m, pivot, -self.back_tilt);
                    add(&mut a, m);
                }
            }
        }
        // legs (4, plus 2 middle legs on long sofas)
        let inset = 0.06;
        let mut legs_xy = vec![[inset, -hw + inset], [inset, hw - inset], [d - inset, -hw + inset], [d - inset, hw - inset]];
        if w > 2.0 { legs_xy.extend([[inset, 0.0], [d - inset, 0.0]]); }
        if let Some((c0, c1, xf)) = self.chaise_span() { legs_xy.extend([[xf - inset, c0 + inset], [xf - inset, c1 - inset]]); }
        if self.legs == Legs::Skirt {
            // a fabric skirt round the base from the frame to just above the floor (kick pleats at the corners: the four panels stop short)
            let (s0, s1) = (0.008, z0 + self.plinth + 0.06);
            add(&mut a, pad([d - 0.012, -hw + 0.02, s0], [d + 0.004, hw - 0.02, s1], 0.006, 1));
            add(&mut a, pad([-0.004, -hw + 0.02, s0], [0.012, hw - 0.02, s1], 0.006, 1));
            add(&mut a, pad([0.02, -hw - 0.004, s0], [d - 0.02, -hw + 0.012, s1], 0.006, 1));
            add(&mut a, pad([0.02, hw - 0.012, s0], [d - 0.02, hw + 0.004, s1], 0.006, 1));
        } else {
            for [x, y] in legs_xy {
                let m = match self.legs {
                    Legs::Tapered => limb([x, y, z0 + 0.01], [x + if x > d / 2.0 { 0.015 } else { -0.015 }, y, 0.0], 0.022, 0.013, true),
                    Legs::Block => Mesh::cuboid([x - 0.03, y - 0.03, 0.0], [x + 0.03, y + 0.03, z0 + 0.01]),
                    Legs::Metal => limb([x, y, z0 + 0.01], [x, y, 0.0], 0.008, 0.008, true),
                    // a turned foot: wide in the middle, narrower at the top and the floor
                    Legs::Bun => { let mut m = limb([x, y, z0 + 0.01], [x, y, z0 * 0.5], 0.03, 0.045, true); m.append(&limb([x, y, z0 * 0.5], [x, y, 0.0], 0.045, 0.028, true)); m }
                    // two thin rods splayed into a V
                    Legs::Hairpin => { let sx = if x > d / 2.0 { 0.03 } else { -0.03 }; let mut m = limb([x, y - 0.025, z0 + 0.01], [x + sx, y, 0.0], 0.005, 0.005, true); m.append(&limb([x, y + 0.025, z0 + 0.01], [x + sx, y, 0.0], 0.005, 0.005, true)); m }
                    Legs::Skirt => unreachable!(),
                };
                let b = m.bounds();
                a.mesh(m, &legm, Some(b));
            }
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


/// the bevelled, puffed back cushion tops out this far below the sharp-edged one
const CUSHION_SHRINK: f64 = 0.013;

impl Sofa {
    /// overall height (legs included): the top edge of the back cushions (rear edge) after they lean back about their bottom edge
    fn top_z(&self) -> f64 {
        self.seat_h + self.back_tilt.cos() * (self.height - self.seat_h) - CUSHION_SHRINK
    }
}

impl Factory for Sofa {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        Some([self.width + if self.arm == Arm::Rolled { 0.06 } else { 0.0 }, self.depth + self.chaise.map_or(0.0, |c| c.1) + if self.legs == Legs::Skirt { 0.014 } else { 0.01 }, self.top_z()])
    }
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
    /// collision: one box (a sofa is solid to the floor, nothing tucks under it) -- rolled arms overhang 3 cm a side, the seat front and
    /// cushion bulge 1 cm (a skirt 1.4 cm) -- and the seat cushions' tops as the support
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        let (hw, lh) = (self.width / 2.0, self.leg_h);
        let roll: f64 = if self.arm == Arm::Rolled { 0.03 } else { 0.0 };
        let sk: f64 = if self.legs == Legs::Skirt { 0.012 } else { 0.0 };
        let front = self.depth + if self.legs == Legs::Skirt { 0.014 } else { 0.01 };
        let (iy0, iy1) = (-hw + self.arm_w, hw - self.arm_w);
        let cx0 = 0.12 + self.back_t * 0.55;
        let mut v = vec![
            // (the back cushions puff up to 3 mm over their top edge mid-cushion)
            // (a skirt's puffed panels stand 1 cm out at the sides, 4 mm behind)
            Volume::hard([-sk * 0.6, -hw - roll.max(sk), -lh - 0.002], [front, hw + roll.max(sk), self.top_z() - lh + 0.006]),
            Volume::support([cx0, iy0, self.seat_h - lh], [self.depth, iy1, self.seat_h - lh + 0.001]),
        ];
        // a chaise: its base, cushion and front legs, and its seat
        if let Some((c0, c1, xf)) = self.chaise_span() {
            v.push(Volume::hard([self.depth - 0.05, c0, -lh - 0.002], [xf + 0.016, c1, self.seat_h - lh + 0.006]));
            v.push(Volume::support([self.depth, c0.max(iy0) + 0.01, self.seat_h - lh], [xf, c1.min(iy1) - 0.01, self.seat_h - lh + 0.001]));
        }
        Some(v)
    }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    Some(Box::new(Sofa::new(&mut rng.clone(), c == "ArmChairFactory")))
}
