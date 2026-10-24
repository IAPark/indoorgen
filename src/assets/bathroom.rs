//! bathroom/*.py: Toilet, Bathtub, BathroomSink, StandingSink, Hardware
use super::*;
use crate::materials::*;
use super::cabnodes::{ExactMesh, GMesh};
use super::{bathfree, gweld, hwexact, toiletx};

pub fn handles(c: &str) -> bool {
    matches!(c, "ToiletFactory" | "BathtubFactory" | "BathroomSinkFactory" | "StandingSinkFactory" | "HardwareFactory")
}

fn ceramic(r: &mut Rng) -> Material {
    let c = r.uniform(0.78, 0.95) as f32;
    mat("ceramic", [c, c, c * 0.98], 0.15)
}

pub struct Toilet {
    pub size: f64,
    pub width: f64,
    pub height: f64,
    pub size_mid: f64,
    pub curve_scale: f64,
    pub depth: f64,
    pub tube_scale: f64,
    pub thickness: f64,
    pub extrude_height: f64,
    pub stand_depth: f64,
    pub stand_scale: f64,
    pub bottom_offset: f64,
    pub back_thickness: f64,
    pub back_size: f64,
    pub back_scale: f64,
    pub seat_size: f64,
    pub tank_width: f64,
    pub tank_height: f64,
    pub tank_size: f64,
    pub tank_cap_height: f64,
    pub tank_cap_extrude: f64,
    pub cover_rotation: f64,
    pub handle: bool,
    pub hw_radius: f64,
    pub hw_length: f64,
    pub hw_side: bool,
    pub seed: u64,
    pub curve_scales: [f64; 4],
    pub seat_thickness: f64,
    pub has_seat_cut: bool,
    pub hw_cap: f64,
    pub fseed: u64,
}

impl Toilet {
    pub fn new(rng: &mut Rng) -> Toilet {
        let mut r = rng.clone();
        let size = r.uniform(0.4, 0.5);
        let width = size * r.uniform(0.7, 0.8);
        let height = size * r.uniform(0.8, 0.9);
        let size_mid = r.uniform(0.6, 0.65);
        let curve_scales = [r.log_uniform(0.8, 1.2), r.log_uniform(0.8, 1.2), r.log_uniform(0.8, 1.2), r.log_uniform(0.8, 1.2)];
        let curve_scale = curve_scales[0];
        let depth = size * r.uniform(0.5, 0.6);
        let tube_scale = r.uniform(0.25, 0.3);
        let thickness = r.uniform(0.05, 0.06);
        let extrude_height = r.uniform(0.015, 0.02);
        let stand_depth = depth * r.uniform(0.85, 0.95);
        let stand_scale = r.uniform(0.7, 0.85);
        let bottom_offset = r.uniform(0.5, 1.5);
        let back_thickness = thickness * r.uniform(0.0, 0.8);
        let back_size = size * r.uniform(0.55, 0.65);
        let back_scale = r.uniform(0.8, 1.0);
        let seat_thickness = r.uniform(0.1, 0.3) * thickness;
        let seat_size = thickness * r.uniform(1.2, 1.6);
        let has_seat_cut = r.f64() < 0.1;
        let tank_width = width * r.uniform(1.0, 1.2);
        let tank_height = height * r.uniform(0.6, 1.0);
        let tank_size = back_size - seat_size - r.uniform(0.02, 0.03);
        let tank_cap_height = r.uniform(0.03, 0.04);
        let tank_cap_extrude = if r.f64() < 0.5 { 0.0 } else { r.uniform(0.005, 0.01) };
        let cover_rotation = -r.uniform(0.0, std::f64::consts::FRAC_PI_2);
        let handle = r.choice_idx(2) == 1;
        let hw_cap = r.uniform(0.01, 0.015);
        let hw_radius = r.uniform(0.015, 0.02);
        let hw_length = r.uniform(0.04, 0.05);
        let hw_side = r.f64() < 0.5;
        Toilet { size, width, height, size_mid, curve_scale, depth, tube_scale, thickness, extrude_height, stand_depth, stand_scale, bottom_offset, back_thickness, back_size, back_scale, seat_size, tank_width, tank_height, tank_size, tank_cap_height, tank_cap_extrude, cover_rotation, handle, hw_radius, hw_length, hw_side, seed: r.randint(0, 1_000_000) as u64, curve_scales, seat_thickness, has_seat_cut, hw_cap, fseed: 0 }
    }
    pub fn mid_offset(&self) -> f64 {
        (1.0 - self.size_mid) * self.size
    }
    /// closed outline (x, y) of the bowl rim: two quarter-ellipse arcs mirrored in x
    fn outline(&self) -> Vec<[f64; 2]> {
        let (w2, a, b) = (self.width / 2.0, self.size_mid * self.size, self.mid_offset());
        let k = 0.5523 * self.curve_scale;
        let bez = |p0: [f64; 2], h0: [f64; 2], h1: [f64; 2], p1: [f64; 2], t: f64| -> [f64; 2] {
            let u = 1.0 - t;
            [0, 1].map(|c| u * u * u * p0[c] + 3.0 * u * u * t * h0[c] + 3.0 * u * t * t * h1[c] + t * t * t * p1[c])
        };
        let n = 16;
        let mut half: Vec<[f64; 2]> = vec![];
        for i in 0..n {
            let t = i as f64 / n as f64;
            half.push(bez([0.0, -a], [k * w2, -a], [w2, -k * a], [w2, 0.0], t));
        }
        for i in 0..=n {
            let t = i as f64 / n as f64;
            half.push(bez([w2, 0.0], [w2, k * b], [k * w2, b], [0.0, b], t));
        }
        let mut full = half.clone();
        for q in half.iter().rev().skip(1).take(half.len() - 2) {
            full.push([-q[0], q[1]]);
        }
        full
    }
    /// funnel ring at relative depth u in [0,1]: outline scaled toward tube_scale and drifting in y
    fn ring(&self, u: f64, out: &[[f64; 2]]) -> Vec<V3> {
        let s = (1.0 - u * u * (1.0 - self.tube_scale * self.tube_scale)).sqrt();
        let cy = u * self.tube_scale * self.mid_offset() / 2.0;
        out.iter().map(|p| [p[0] * s, p[1] * s + cy, -u * self.depth]).collect()
    }
}

fn loft(rings: &[Vec<V3>], m: &mut Mesh, flip: bool) {
    for k in 0..rings.len() - 1 {
        let n = rings[k].len();
        for i in 0..n {
            let j = (i + 1) % n;
            if flip {
                m.add_quad(rings[k][i], rings[k + 1][i], rings[k + 1][j], rings[k][j]);
            } else {
                m.add_quad(rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]);
            }
        }
    }
}

fn expand(ring: &[V3], d: f64, dz: f64) -> Vec<V3> {
    let n = ring.len();
    let c = [ring.iter().map(|p| p[0]).sum::<f64>() / n as f64, ring.iter().map(|p| p[1]).sum::<f64>() / n as f64];
    ring.iter()
        .enumerate()
        .map(|(i, p)| {
            let (pa, pb) = (ring[(i + n - 1) % n], ring[(i + 1) % n]);
            let t = [pb[0] - pa[0], pb[1] - pa[1]];
            let l = (t[0] * t[0] + t[1] * t[1]).sqrt().max(1e-9);
            let mut nv = [t[1] / l, -t[0] / l];
            if nv[0] * (p[0] - c[0]) + nv[1] * (p[1] - c[1]) < 0.0 {
                nv = [-nv[0], -nv[1]];
            }
            [p[0] + nv[0] * d, p[1] + nv[1] * d, p[2] + dz]
        })
        .collect()
}

impl Factory for Toilet {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        let hi_z = self.tank_height.max(-self.cover_rotation.sin() * (self.seat_size + self.size + self.thickness + self.thickness));
        let (lo, hi) = ([-self.mid_offset() - self.back_size - self.tank_cap_extrude, -self.width / 2.0 - self.thickness * 1.1, -self.height], [self.size_mid * self.size + self.thickness + self.thickness, self.width / 2.0 + self.thickness * 1.1, hi_z]);
        // toilet.py create_placeholder: new_bbox in the unrotated object frame (the asset itself is rotated by pi/2 afterwards)
        a.cuboid([lo[0], lo[1], lo[2]], [hi[0], hi[1], hi[2]], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let ceramic = spec_material("toilet_ceramic", serde_json::json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.seed}), [0.9, 0.9, 0.9]);
        let metal = spec_material("toilet_hw", serde_json::json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.seed + 1}), [0.7, 0.7, 0.72]);
        let mut r = inst_rng(self.fseed, i);
        let p = toiletx::build(self, &mut r);
        let body = super::bathkit::join(&[&p.tube, &p.seat, &p.cover, &p.stand, &p.back, &p.tank]);
        let mut a = Asset::default();
        a.mesh(body, &ceramic, None);
        a.mesh(p.hw, &metal, None);
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

fn rounded(m: Mesh, _r: f64) -> Mesh {
    m
}

pub struct Bathtub {
    pub width: f64,
    pub size: f64,
    pub depth: f64,
    pub alcove: bool,
    pub has_curve: bool,
    pub has_legs: bool,
    pub thickness: f64,
    pub disp_x: [f64; 2],
    pub disp_y: f64,
    pub leg_height: f64,
    pub leg_radius: f64,
    pub hole_radius: f64,
    pub seed: u64,
    pub taper: f64,
    pub fseed: u64,
    pub leg_side: f64,
    pub leg_y_scale: f64,
    pub leg_sub: i64,
    pub alcove_levels: usize,
    pub stretch: f64,
}

impl Bathtub {
    pub fn new(rng: &mut Rng) -> Bathtub {
        let mut r = rng.clone();
        let width = r.uniform(1.5, 2.0);
        let size = r.uniform(0.8, 1.0);
        let depth = r.uniform(0.55, 0.7);
        let alcove = r.choice_p(&[0.5, 0.5]) == 0;
        let has_curve = r.f64() < 0.5;
        let has_legs = r.f64() < 0.5;
        let thickness = if alcove { r.uniform(0.04, 0.08) } else { r.uniform(0.02, 0.04) };
        let disp_x = [r.uniform(0.0, 0.2), r.uniform(0.0, 0.2)];
        let disp_y = r.uniform(0.0, 0.1);
        let leg_height = r.uniform(0.2, 0.3) * depth;
        let leg_side = r.uniform(0.05, 0.1);
        let leg_radius = r.uniform(0.02, 0.03);
        let leg_y_scale = r.f64();
        let leg_sub = r.randint(0, 3);
        let taper = r.uniform(-0.1, 0.1);
        let stretch = r.uniform(-0.2, 0.2);
        let mut alcove_levels = 1usize;
        if alcove {
            alcove_levels = r.randint(1, 3) as usize;
        }
        let hole_radius = r.uniform(0.015, 0.02);
        Bathtub { width, size, depth, alcove, has_curve, has_legs, thickness, disp_x, disp_y, leg_height, leg_radius, hole_radius, seed: r.randint(0, 1_000_000) as u64, taper, fseed: 0, leg_side, leg_y_scale, leg_sub, alcove_levels, stretch }
    }
    /// make_box_contour(t, i)
    fn contour(&self, t: f64, i: f64) -> Vec<[f64; 2]> {
        let (w, s) = (self.width, self.size);
        vec![
            [t + self.disp_x[0] * i, t + self.disp_y * i],
            [w - t - self.disp_x[1] * i, t + self.disp_y * i],
            [w - t - self.disp_x[1] * i, s - t - self.disp_y * i],
            [t + self.disp_x[0] * i, s - t - self.disp_y * i],
        ]
    }
}

fn ring3(poly: &[[f64; 2]], z: f64) -> Vec<V3> {
    poly.iter().map(|p| [p[0], p[1], z]).collect()
}

fn cap_fan(ring: &[V3], up: bool, m: &mut Mesh) {
    let n = ring.len();
    let c = [ring.iter().map(|p| p[0]).sum::<f64>() / n as f64, ring.iter().map(|p| p[1]).sum::<f64>() / n as f64, ring[0][2]];
    for i in 0..n {
        let j = (i + 1) % n;
        if up {
            m.add_tri(ring[i], ring[j], c);
        } else {
            m.add_tri(ring[j], ring[i], c);
        }
    }
}

impl Factory for Bathtub {
    /// alcove (built-in) bathtubs have exactly the placeholder formula as bounding box; freestanding ones are not ported
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        if self.alcove {
            Some(([-self.size, 0.0, 0.0], [0.0, self.width, self.depth]))
        } else {
            let mut r = inst_rng(self.fseed, _i);
            let u = r.uniform(0.5, 0.7);
            let p = bathfree::BowlParams {
                width: self.width,
                size: self.size,
                depth: self.depth,
                thickness: self.thickness,
                taper: self.taper,
                has_curve: self.has_curve,
                disp_x: self.disp_x,
                disp_y: self.disp_y,
                alcove_levels: 1,
                levels: 5,
                side_levels: 2,
            };
            let st = bathfree::pipeline(&p, u);
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for q in &st.fin.pos {
                let q = [-q[1], q[0], q[2]];
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            Some((lo, hi))
        }
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        a.cuboid([-self.size, 0.0, 0.0], [0.0, self.width, self.depth], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let ceramic = spec_material("tub_ceramic", serde_json::json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.seed}), [0.92, 0.92, 0.92]);
        let metal = spec_material("tub_metal", serde_json::json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.seed + 1}), [0.7, 0.7, 0.72]);
        let mut r = inst_rng(self.fseed, i);
        let b = super::bathtub::build(self, &mut r);
        let mut a = Asset::default();
        a.mesh(b.body, &ceramic, None);
        a.mesh(b.hole, &metal, None);
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

pub struct Sink {
    width: f64,
    size: f64,
    depth: f64,
    alcove: bool,
    has_extrude: bool,
    has_stand: bool,
    alcove_levels: usize,
    thickness: f64,
    size_extrude: f64,
    tap_offset: f64,
    stand_radius: f64,
    stand_bottom: f64,
    stand_height: f64,
    stand_circular: bool,
    disp_x: f64,
    disp_y: f64,
    tap_h: f64,
    seed: u64,
}

impl Sink {
    fn new(rng: &mut Rng, standing: bool) -> Sink {
        // BathtubFactory.__init__ runs first (only disp_x[0] / disp_y survive), then the sink block reseeds
        let tub = Bathtub::new(rng);
        let mut r = rng.clone();
        let width = r.uniform(0.6, 0.9);
        let size = width * r.log_uniform(0.55, 0.8);
        let depth = width * r.log_uniform(0.2, 0.4);
        let kind = r.randint(0, 3); // undermount, drop-in, vessel
        let mut has_stand = false;
        let (mut alcove, mut has_extrude) = (false, true);
        match kind {
            0 => {
                alcove = false;
                has_extrude = r.f64() < 0.7;
            }
            1 => {
                alcove = true;
                has_extrude = true;
            }
            _ => {
                alcove = r.choice_idx(2) == 0;
                has_extrude = r.f64() < 0.7;
                has_stand = true;
            }
        }
        let alcove_levels = if r.f64() < 0.5 { 0 } else { r.randint(2, 4) as usize };
        let thickness = if alcove { 0.01 } else { r.uniform(0.01, 0.03) };
        let size_extrude = r.uniform(0.2, 0.35);
        let tap_offset = r.uniform(0.0, 0.05);
        let stand_radius = width / 2.0 * r.log_uniform(0.15, 0.2);
        let stand_bottom = if r.f64() < 0.6 { width * r.log_uniform(0.2, 0.3) } else { stand_radius };
        let stand_height = r.uniform(0.7, 0.9) - depth;
        let stand_circular = r.f64() < 0.5;
        let (mut alcove, mut has_extrude, mut has_stand) = (alcove, has_extrude, has_stand);
        if standing {
            alcove = false;
            has_extrude = true;
            has_stand = true;
        }
        Sink { width, size, depth, alcove, has_extrude, has_stand, alcove_levels, thickness, size_extrude, tap_offset, stand_radius, stand_bottom, stand_height, stand_circular, disp_x: tub.disp_x[0], disp_y: tub.disp_y, tap_h: r.uniform(0.5, 1.0), seed: r.randint(0, 1_000_000) as u64 }
    }
}

impl Factory for Sink {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        a.cuboid([-(self.size_extrude + 1.0) * self.size, 0.0, if self.has_stand { -self.stand_height } else { 0.0 }], [0.0, self.width, self.depth], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        use super::geomkit::{chaikin, limb};
        let ceramic = spec_material("sink_ceramic", serde_json::json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.seed}), [0.92, 0.92, 0.92]);
        let metal = spec_material("sink_metal", serde_json::json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.seed + 1}), [0.7, 0.7, 0.72]);
        let (w, s, d, t) = (self.width, self.size, self.depth, self.thickness);
        let (dx, dy) = (self.disp_x.min(w * 0.2), self.disp_y.min(s * 0.2));
        let contour = |ins: f64, i: f64| -> Vec<[f64; 2]> { vec![[ins + dx * i, ins + dy * i], [w - ins - dx * i, ins + dy * i], [w - ins - dx * i, s - ins - dy * i], [ins + dx * i, s - ins - dy * i]] };
        let smooth = if self.alcove_levels == 0 { 0 } else { self.alcove_levels + 1 };
        let mut a = Asset::default();
        let mut m = Mesh::new();
        let c = [w / 2.0, s / 2.0];
        if self.alcove {
            // drop-in: slab-walled box with the basin cut in
            // drop-in: solid slab whose basin is the (subsurf-shrunk, rounded) cutter
            let outer = contour(0.0, 0.0);
            let (cx, cy) = (c[0], c[1]);
            let (hx, hy) = (w / 2.0, s / 2.0);
            let n = 28;
            let ell = |sc: f64| -> Vec<[f64; 2]> {
                (0..n).map(|i| {
                    let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                    let (co, si) = (a.cos(), a.sin());
                    let e = 0.25; // superellipse exponent: boxy when smoothing is light
                    let kx = co.abs().powf(2.0 / (2.0 + 2.0 * e)).copysign(co);
                    let ky = si.abs().powf(2.0 / (2.0 + 2.0 * e)).copysign(si);
                    [cx + hx * 0.78 * sc * kx, cy + hy * 0.78 * sc * ky]
                }).collect()
            };
            let rect_out: Vec<[f64; 2]> = ell(1.0).iter().map(|p| {
                let (ex, ey) = (p[0] - cx, p[1] - cy);
                let k = (hx / ex.abs().max(1e-9)).min(hy / ey.abs().max(1e-9));
                [cx + ex * k, cy + ey * k]
            }).collect();
            let _ = outer;
            let (ro_t, ro_b) = (ring3(&rect_out, d), ring3(&rect_out, 0.0));
            loft(&[ro_t.clone(), ro_b.clone()], &mut m, true);
            cap_fan(&ro_b, false, &mut m);
            loft(&[ro_t, ring3(&ell(1.0), d)], &mut m, true);
            let k = 8;
            let rings: Vec<Vec<V3>> = (0..=k).map(|q| {
                let u = q as f64 / k as f64;
                let sc = (1.0 - (1.0 - u).powi(2)).max(0.0).sqrt() * 0.9 + 0.1 * u;
                ell(sc).iter().map(|p| [p[0], p[1], t + (d - t) * u]).collect()
            }).collect();
            loft(&rings, &mut m, false);
            cap_fan(&rings[0], true, &mut m);
        } else {
            let rim = chaikin(&contour(0.0, 0.0), smooth.max(2).min(3));
            let n = 10;
            let ring_at = |u: f64, off: f64| -> Vec<V3> {
                let sc = 0.28 + 0.72 * (1.0 - (1.0 - u).powi(2)).max(0.0).sqrt();
                rim.iter().map(|p| {
                    let (ex, ey) = (p[0] - c[0], p[1] - c[1]);
                    let l = (ex * ex + ey * ey).sqrt().max(1e-9);
                    [c[0] + ex * sc + ex / l * off, c[1] + ey * sc + ey / l * off, d * u]
                }).collect()
            };
            let inner: Vec<Vec<V3>> = (0..=n).map(|q| ring_at(q as f64 / n as f64, -t)).collect();
            let outer: Vec<Vec<V3>> = (0..=n).map(|q| ring_at(q as f64 / n as f64, 0.0)).collect();
            loft(&outer, &mut m, false);
            loft(&inner, &mut m, true);
            loft(&[inner[n].clone(), outer[n].clone()], &mut m, true);
            cap_fan(&outer[0], false, &mut m);
            cap_fan(&inner[0], true, &mut m);
        }
        let b = m.bounds();
        a.mesh(m, &ceramic, Some(b));
        if self.has_extrude {
            let deck = Mesh::cuboid([0.0, s, d - 0.025], [w, s * (1.0 + self.size_extrude), d]);
            let db = deck.bounds();
            a.mesh(deck, &ceramic, Some(db));
        }
        if self.has_stand {
            let h = self.stand_height;
            let (r0, r1) = (self.stand_radius, self.stand_bottom);
            let stand = if self.stand_circular {
                Mesh::frustum(c[0], c[1], -h, 0.0, r1, r0, 16)
            } else {
                let mut q = Mesh::new();
                let top = [[c[0] - r0, c[1] - r0], [c[0] + r0, c[1] - r0], [c[0] + r0, c[1] + r0], [c[0] - r0, c[1] + r0]];
                let bot = [[c[0] - r1, c[1] - r1], [c[0] + r1, c[1] - r1], [c[0] + r1, c[1] + r1], [c[0] - r1, c[1] + r1]];
                loft(&[ring3(&top, 0.0), ring3(&bot, -h)], &mut q, false);
                cap_fan(&ring3(&bot, -h), false, &mut q);
                q
            };
            let sb = stand.bounds();
            a.mesh(stand, &ceramic, Some(sb));
        }
        let hole = Mesh::cylinder(c[0], c[1], t, t + 0.006, 0.018, 14);
        let hb = hole.bounds();
        a.mesh(hole, &metal, Some(hb));
        if self.has_extrude {
            // gooseneck tap on the deck
            let (tx, ty) = (w / 2.0, s * (1.0 + self.size_extrude * 0.45));
            let hh = 0.16 * self.tap_h + 0.08;
            let base = Mesh::cylinder(tx, ty, d, d + 0.02, 0.025, 16);
            let bb = base.bounds();
            a.mesh(base, &metal, Some(bb));
            let pts: Vec<V3> = (0..=10)
                .map(|k| {
                    let u = k as f64 / 10.0;
                    let ang = std::f64::consts::PI * 0.5 * u;
                    [tx, ty - (1.0 - ang.cos()) * 0.0 - 0.1 * (1.0 - ang.cos()) * (u), d + 0.02 + hh * (ang.sin()) ]
                })
                .collect();
            for k in 0..pts.len() - 1 {
                let l = limb(pts[k], pts[k + 1], 0.011, 0.011, true);
                let lb = l.bounds();
                a.mesh(l, &metal, Some(lb));
            }
            let top = pts[pts.len() - 1];
            let spout = limb(top, [top[0], top[1] - 0.12, top[2] - 0.03], 0.011, 0.009, true);
            let sb = spout.bounds();
            a.mesh(spout, &metal, Some(sb));
            let lever = limb([tx, ty, d + 0.04], [tx + 0.06, ty + 0.02, d + 0.05], 0.007, 0.007, true);
            let lb = lever.bounds();
            a.mesh(lever, &metal, Some(lb));
        }
        // python: x = -y, y = x after the +90 degree rotation about z
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

pub struct Hardware {
    ar: f64,
    ad: f64,
    r: f64,
    depth: f64,
    circular: bool,
    ty: usize,
    hook_length: f64,
    holder_length: f64,
    bar_length: f64,
    ext: f64,
    ring_radius: f64,
    fseed: u64,
}

impl Hardware {
    fn new(rng: &mut Rng) -> Hardware {
        let mut r = rng.clone();
        let ar = r.uniform(0.02, 0.03);
        let ad = r.uniform(0.01, 0.015);
        let rad = r.uniform(0.01, 0.015);
        let depth = r.uniform(0.06, 0.1);
        let circular = r.f64() < 0.5;
        let ty = r.choice_idx(4);
        let hook_length = ar * r.uniform(2.0, 4.0);
        let holder_length = r.uniform(0.15, 0.25);
        let bar_length = r.uniform(0.4, 0.8);
        let ext = ar * r.uniform(2.0, 3.0);
        let ring_radius = r.log_uniform(2.0, 6.0) * ar;
        Hardware { ar, ad, r: rad, depth, circular, ty, hook_length, holder_length, bar_length, ext, ring_radius, fseed: 0 }
    }
    fn prim(&self) -> GMesh {
        if self.circular {
            hwexact::bpy_cylinder()
        } else {
            hwexact::bpy_cube()
        }
    }
    fn attachment(&self) -> GMesh {
        use std::f64::consts::FRAC_PI_2 as H;
        let base = hwexact::apply_obj(&self.prim(), [self.ar, self.ar, self.ad / 2.0], [H, 0.0, 0.0], [0.0, -self.ad / 2.0, 0.0], true);
        let rod = hwexact::apply_obj(&self.prim(), [self.r, self.r, self.depth / 2.0], [H, 0.0, 0.0], [0.0, -self.depth / 2.0, 0.0], true);
        hwexact::join_fwd(&[base, rod])
    }
    fn exact_mesh(&self, inst: u64) -> GMesh {
        use std::f64::consts::FRAC_PI_2 as H;
        let z = [0.0; 3];
        let one = [1.0; 3];
        let extra = match self.ty {
            0 => hwexact::apply_obj(&self.prim(), [self.r, self.r, self.hook_length / 2.0], z, z, false),
            1 => hwexact::apply_obj(
                &self.prim(),
                [self.r, self.r, (self.holder_length + self.ext) / 2.0],
                [0.0, H, 0.0],
                [(self.holder_length - self.ext) / 2.0, 0.0, 0.0],
                true,
            ),
            2 => hwexact::apply_obj(&self.prim(), [self.r, self.r, self.bar_length / 2.0 + self.ext], [0.0, H, 0.0], [self.bar_length / 2.0, 0.0, 0.0], true),
            _ => self.ring(inst),
        };
        let _ = one;
        let extra = hwexact::apply_obj(&extra, [1.001; 3], z, [0.0, -self.depth, 0.0], true);
        let mut parts = vec![self.attachment(), extra];
        if self.ty == 2 {
            parts.push(hwexact::apply_obj(&self.attachment(), one, z, [self.bar_length, 0.0, 0.0], true));
        }
        let j = hwexact::join_fwd(&parts);
        hwexact::apply_obj(&j, one, [0.0, 0.0, H], z, false)
    }
    fn ring(&self, inst: u64) -> GMesh {
        let mut r = inst_rng(self.fseed, inst);
        let minor = self.r * r.uniform(0.4, 0.7);
        let t = hwexact::bpy_torus(128, 12, self.ring_radius, minor);
        let mut t = hwexact::apply_obj(&t, [1.0; 3], [std::f64::consts::FRAC_PI_2, 0.0, 0.0], [0.0, self.ad, -self.ring_radius], true);
        t.e = hwexact::calc_edges_bl(&t.p, 4);
        let ev = hwexact::regular_bspline_eval(&t);
        gweld::subdivide_ev(&t, 2, Some(&ev))
    }
}

impl Factory for Hardware {
    fn exact(&self, i: u64) -> Option<ExactMesh> {
        let mut e = self.exact_mesh(i).to_exact();
        e.names.clear();
        Some(e)
    }
    fn asset(&self, i: u64) -> Asset {
        let m = mat("hardware_metal", [0.75, 0.75, 0.77], 0.25);
        let mut a = Asset::default();
        let mut r = inst_rng(self.fseed, i);
        // built in the python frame (wall normal = -y), then rotated by pi/2 about z
        let attach = |a: &mut Asset, x0: f64| {
            a.cuboid([x0 - self.ar, -self.ad, -self.ar], [x0 + self.ar, 0.0, self.ar], &m);
            a.cuboid([x0 - self.r, -self.depth, -self.r], [x0 + self.r, 0.0, self.r], &m);
        };
        attach(&mut a, 0.0);
        let y = -self.depth;
        match self.ty {
            0 => a.cuboid([-self.r, y - self.r, -self.hook_length / 2.0], [self.r, y + self.r, self.hook_length / 2.0], &m),
            1 => a.cuboid([-self.ext, y - self.r, -self.r], [self.holder_length, y + self.r, self.r], &m),
            2 => {
                a.cuboid([-self.ext, y - self.r, -self.r], [self.bar_length + self.ext, y + self.r, self.r], &m);
                attach(&mut a, self.bar_length);
            }
            _ => {
                let rr = self.ring_radius;
                let minor = self.r * r.uniform(0.4, 0.7);
                let yc = self.ad - self.depth;
                a.cuboid([-rr - minor, yc - minor, -2.0 * rr - minor], [rr + minor, yc + minor, minor], &m);
            }
        }
        let _ = self.circular;
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(match c {
        "ToiletFactory" => {
            let mut t = Toilet::new(rng);
            t.fseed = fseed;
            Box::new(t)
        }
        "BathtubFactory" => {
            let mut b = Bathtub::new(rng);
            b.fseed = fseed;
            Box::new(b)
        }
        "BathroomSinkFactory" => Box::new(Sink::new(rng, false)),
        "StandingSinkFactory" => Box::new(Sink::new(rng, true)),
        "HardwareFactory" => {
            let mut h = Hardware::new(rng);
            h.fseed = fseed;
            Box::new(h)
        }
        _ => return None,
    })
}
