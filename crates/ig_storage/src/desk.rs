//! ModernDesk (not in infinigen): desks as real ones come, beyond infinigen's board on four pins: a top on tapered legs, slab panel
//! ends (with a modesty panel), timber trestles, metal A-frames or sled loops, with a drawer pedestal or open cubby at one or both ends
//! and, on some, a shallow drawer under the top. The knee hole in the middle stays at least 0.6 m wide (the desk chair is centred in
//! front). Frame as SimpleDesk: x = depth centred (the front at +x, where the chair stands), y = width centred, z = 0 on the floor.
use super::*;
use crate::materials::*;
use ig_kit::geomkit::limb;
use serde_json::json;

const TOP: Weighted = &[("wood.Wood", 3.0), ("wood.WhitePlywood", 2.0), ("wood.BlackPlywood", 1.0), ("wood.BlondePlywood", 1.5)];
const BODY: Weighted = &[("wood.Wood", 2.0), ("wood.WhitePlywood", 2.0), ("wood.BlackPlywood", 1.0), ("plastic.PlasticRough", 0.5)];
const METAL: Weighted = &[("metal.BrushedBlackMetal", 2.5), ("metal.BrushedMetal", 1.2), ("metal.MetalBasic", 0.8)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Base { Legs, Panels, Trestle, AFrame, Sled }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ped { Drawers(usize), Cubby }

pub struct ModernDesk {
    d: f64,
    w: f64,
    h: f64,
    t: f64,
    base: Base,
    /// a pedestal at the -y end, the +y end
    peds: [Option<Ped>; 2],
    ped_w: f64,
    apron_drawer: bool,
    seed: u64,
}

fn bx(lo: [f64; 3], hi: [f64; 3]) -> Mesh { Mesh::cuboid(lo, hi) }

impl ModernDesk {
    pub fn new(r: &mut Rng) -> ModernDesk {
        let (mut d, mut w, mut h) = (r.uniform(0.55, 0.75), r.uniform(1.0, 1.8), r.uniform(0.72, 0.77));
        if let Some(sz) = crate::api::size_hint() { w = sz[0]; d = sz[1]; h = sz[2]; }
        let t = r.uniform(0.022, 0.04);
        let base = [Base::Legs, Base::Panels, Base::Trestle, Base::AFrame, Base::Sled][r.choice_p(&[0.3, 0.25, 0.15, 0.15, 0.15])];
        let ped_w = r.uniform(0.36, 0.45).min((w - 0.62) / 2.0).max((w - 0.62).min(0.42));
        // pedestals while the knee hole keeps 0.6 m: both ends on a wide desk, one end, or none
        let room1 = w - ped_w >= 0.62 + 0.1 && ped_w >= 0.3;
        let room2 = w - 2.0 * ped_w >= 0.62 + 0.1;
        let mut ped = |r: &mut Rng| if r.f64() < 0.75 { Ped::Drawers(r.randint(2, 4) as usize) } else { Ped::Cubby };
        let k = r.f64();
        let peds = if room2 && k < 0.25 { let p = ped(r); [Some(p), Some(p)] } else if room1 && k < 0.7 { if r.f64() < 0.5 { [Some(ped(r)), None] } else { [None, Some(ped(r))] } } else { [None, None] };
        let apron_drawer = r.f64() < 0.35;
        ModernDesk { d, w, h, t, base, peds, ped_w, apron_drawer, seed: r.randint(0, 1_000_000) as u64 }
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let top = spec_material("desk_top", json!({"cls": weighted(&mut r, TOP), "kwargs": {}, "seed": self.seed}), [0.5, 0.36, 0.22]);
        let body = if r.f64() < 0.5 { top.clone() } else { spec_material("desk_body", json!({"cls": weighted(&mut r, BODY), "kwargs": {}, "seed": self.seed + 1}), [0.85, 0.84, 0.8]) };
        // 20 % painted carcass and base under a wood top (own stream, NO_PAINT=1: off)
        let body = if std::env::var_os("NO_PAINT").is_none() && crate::int_hash(&["desk_paint", &self.seed.to_string()]) % 100 < 20 { painted_material(&format!("desk_paint:{}", self.seed), self.seed, [0.85, 0.84, 0.8]) } else { body };
        let metal = spec_material("desk_metal", json!({"cls": weighted(&mut r, METAL), "kwargs": {}, "seed": self.seed + 2}), [0.1, 0.1, 0.1]);
        let pulls_metal = r.f64() < 0.6;
        let (d, w, h, t) = (self.d, self.w, self.h, self.t);
        let (x0, x1, hw) = (-d / 2.0, d / 2.0, w / 2.0);
        let zt = h - t; // underside of the top
        let mut a = Asset::default();
        let add = |a: &mut Asset, m: Mesh, mat: &Material| { let b = m.bounds(); a.mesh(m, mat, Some(b)); };
        add(&mut a, bx([x0, -hw, zt], [x1, hw, h]), &top);
        let pull_mat = if pulls_metal { metal.clone() } else { body.clone() };
        let bt = 0.018;
        // pedestals: a carcass from the floor (on a 4 cm plinth) to the top, set in 2 cm from the end and the front
        let mut ped_span = [(-hw, -hw), (hw, hw)]; // the y range each end's pedestal occupies (empty: none)
        for (side, p) in self.peds.iter().enumerate() {
            let Some(p) = p else { continue };
            let (y0, y1) = if side == 0 { (-hw + 0.02, -hw + 0.02 + self.ped_w) } else { (hw - 0.02 - self.ped_w, hw - 0.02) };
            ped_span[side] = (y0, y1);
            let (px0, px1) = (x0 + 0.02, x1 - 0.03);
            let z0 = 0.04;
            add(&mut a, bx([px0 + 0.03, y0 + 0.02, 0.0], [px1 - 0.03, y1 - 0.02, z0]), &body);
            for y in [y0, y1 - bt] { add(&mut a, bx([px0, y, z0], [px1, y + bt, zt]), &body); }
            add(&mut a, bx([px0, y0 + bt, z0], [px0 + 0.01, y1 - bt, zt]), &body);
            add(&mut a, bx([px0, y0 + bt, z0], [px1, y1 - bt, z0 + bt]), &body);
            match *p {
                Ped::Drawers(n) => {
                    // the drawer fronts flush with the carcass (3 mm reveals), the bottom one deepest on a 3-drawer stack
                    let (fz0, fz1) = (z0 + bt, zt - 0.01);
                    let hts: Vec<f64> = if n == 3 { vec![0.42, 0.29, 0.29] } else { vec![1.0 / n as f64; n] };
                    let mut z = fz0;
                    for f in hts {
                        let dz = (fz1 - fz0) * f;
                        add(&mut a, bx([px1 - 0.018, y0 + 0.003, z + 0.003], [px1, y1 - 0.003, z + dz - 0.003]), &body);
                        let (yc, zc) = ((y0 + y1) / 2.0, z + dz - 0.05_f64.min(dz / 2.0));
                        add(&mut a, limb([px1 + 0.018, yc - 0.05, zc], [px1 + 0.018, yc + 0.05, zc], 0.006, 0.006, true), &pull_mat);
                        for yy in [yc - 0.05, yc + 0.05] { add(&mut a, limb([px1, yy, zc], [px1 + 0.018, yy, zc], 0.005, 0.005, true), &pull_mat); }
                        z += dz;
                    }
                }
                Ped::Cubby => {
                    let zm = (z0 + zt) / 2.0;
                    add(&mut a, bx([px0 + 0.01, y0 + bt, zm - bt / 2.0], [px1 - 0.01, y1 - bt, zm + bt / 2.0]), &body);
                }
            }
        }
        // the free y range under the top between the pedestals (the base supports the ends without one)
        let (ya, yb) = (if self.peds[0].is_some() { ped_span[0].1 } else { -hw }, if self.peds[1].is_some() { ped_span[1].0 } else { hw });
        let open = [self.peds[0].is_none(), self.peds[1].is_none()];
        let end_y = |side: usize| if side == 0 { -hw } else { hw };
        match self.base {
            Base::Legs => {
                let lr = r.uniform(0.018, 0.026);
                let square = r.f64() < 0.4;
                for side in 0..2 { if !open[side] { continue }
                    let y = end_y(side) - (side as f64 * 2.0 - 1.0) * 0.05;
                    for x in [x0 + 0.05, x1 - 0.05] {
                        let m = if square { bx([x - lr, y - lr, 0.0], [x + lr, y + lr, zt]) } else { limb([x, y, zt], [x, y, 0.0], lr, lr * 0.65, true) };
                        add(&mut a, m, &body);
                    }
                }
                // an apron (rails under the top) between the legs and pedestals
                add(&mut a, bx([x1 - 0.06, ya + 0.02, zt - 0.07], [x1 - 0.04, yb - 0.02, zt]), &body);
                add(&mut a, bx([x0 + 0.04, ya + 0.02, zt - 0.07], [x0 + 0.06, yb - 0.02, zt]), &body);
            }
            Base::Panels => {
                for side in 0..2 { if !open[side] { continue }
                    let y = if side == 0 { -hw + 0.01 } else { hw - 0.01 - 0.025 };
                    add(&mut a, bx([x0 + 0.01, y, 0.0], [x1 - 0.02, y + 0.025, zt]), &body);
                }
                // a modesty panel at the back, its bottom well clear of the floor
                add(&mut a, bx([x0 + 0.03, ya + 0.02, zt * 0.45], [x0 + 0.048, yb - 0.02, zt]), &body);
            }
            Base::Trestle => {
                // timber trestles: a foot along the depth, a post, a bearer under the top, and a stretcher at the back
                for side in 0..2 { if !open[side] { continue }
                    let y = end_y(side) - (side as f64 * 2.0 - 1.0) * 0.1;
                    add(&mut a, bx([x0 + 0.04, y - 0.03, 0.0], [x1 - 0.04, y + 0.03, 0.06]), &body);
                    add(&mut a, bx([-0.035, y - 0.03, 0.06], [0.035, y + 0.03, zt - 0.05]), &body);
                    add(&mut a, bx([x0 + 0.06, y - 0.03, zt - 0.05], [x1 - 0.06, y + 0.03, zt]), &body);
                }
                let (sa, sb) = (if open[0] { -hw + 0.1 } else { ya }, if open[1] { hw - 0.1 } else { yb });
                add(&mut a, bx([x0 + 0.08, sa, 0.32], [x0 + 0.13, sb, 0.4]), &body);
            }
            Base::AFrame | Base::Sled => {
                let s = 0.014;
                for side in 0..2 { if !open[side] { continue }
                    let y = end_y(side) - (side as f64 * 2.0 - 1.0) * 0.05;
                    if self.base == Base::AFrame {
                        // two square tubes from the floor at the front and back up to a point under the top's middle
                        for xf in [x0 + 0.06, x1 - 0.06] { add(&mut a, tube([xf, y, 0.0], [xf * 0.25, y, zt - 0.03], s), &metal); }
                        add(&mut a, bx([x0 + 0.06, y - s, zt - 0.03], [x1 - 0.06, y + s, zt]), &metal);
                    } else {
                        // a closed loop: floor rail, two posts, a rail under the top
                        add(&mut a, bx([x0 + 0.04, y - s, 0.0], [x1 - 0.04, y + s, 2.0 * s]), &metal);
                        add(&mut a, bx([x0 + 0.06, y - s, zt - 2.0 * s], [x1 - 0.06, y + s, zt]), &metal);
                        for xf in [x0 + 0.04, x1 - 0.04 - 2.0 * s] { add(&mut a, bx([xf, y - s, 0.0], [xf + 2.0 * s, y + s, zt]), &metal); }
                    }
                }
                // a back rail between the frames
                add(&mut a, bx([x0 + 0.05, ya + 0.02, zt - 0.06], [x0 + 0.05 + 2.0 * s, yb - 0.02, zt - 0.06 + 2.0 * s]), &metal);
            }
        }
        // a shallow drawer under the top in the knee hole's middle (a drop of 8 cm leaves ~0.6 m for the knees)
        if self.apron_drawer && yb - ya > 0.7 {
            let dw = (yb - ya - 0.2).min(0.6);
            add(&mut a, bx([x0 + 0.1, -dw / 2.0, zt - 0.08], [x1 - 0.04, dw / 2.0, zt]), &body);
            add(&mut a, limb([x1 - 0.024, -0.04, zt - 0.04], [x1 - 0.024, 0.04, zt - 0.04], 0.006, 0.006, true), &pull_mat);
            for yy in [-0.04, 0.04] { add(&mut a, limb([x1 - 0.04, yy, zt - 0.04], [x1 - 0.024, yy, zt - 0.04], 0.005, 0.005, true), &pull_mat); }
        }
        a.supports.push(([x0, -hw, h], [x1, hw, h + 0.001]));
        a
    }
}

/// a square tube between two points in an xz plane (y fixed)
fn tube(p0: [f64; 3], p1: [f64; 3], s: f64) -> Mesh {
    let (dx, dz) = (p1[0] - p0[0], p1[2] - p0[2]);
    let l = (dx * dx + dz * dz).sqrt();
    let (nx, nz) = (-dz / l * s, dx / l * s);
    // (the cuboid's corner order with x along the tube, y across, z along the normal: e_len x e_y = n, right-handed)
    let c = |x: usize, y: usize, z: usize| { let p = if x == 0 { p0 } else { p1 }; let (a, b) = (if z == 0 { -1.0 } else { 1.0 }, if y == 0 { -1.0 } else { 1.0 }); [p[0] + nx * a, p[1] + s * b, p[2] + nz * a] };
    let mut m = Mesh::new();
    m.add_quad(c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0));
    m.add_quad(c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1));
    m.add_quad(c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1));
    m.add_quad(c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0));
    m.add_quad(c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0));
    m.add_quad(c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1));
    m
}

impl Factory for ModernDesk {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> { Some([self.w, self.d, self.h]) }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> { Some(([-self.d / 2.0, -self.w / 2.0, 0.0], [self.d / 2.0, self.w / 2.0, self.h])) }
    fn asset(&self, _i: u64) -> Asset { self.build() }
}
