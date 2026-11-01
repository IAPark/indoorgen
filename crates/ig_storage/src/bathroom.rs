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
    /// collision, in the build's frame (y front to back, z = 0 at the seat) turned by pi/2 as the build is: the bowl's rim (its outline
    /// grown by the wall and the seat's lip) to the tank's cap at the back, the stand to the floor, the tank's top or the cover tipped up
    /// about its hinge, and a flush handle on the tank's side
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (a, mo, t, e) = (self.size_mid * self.size, self.mid_offset(), self.thickness, self.extrude_height);
        let front = a + t + e + 0.01;
        let back = mo + self.back_size + self.tank_cap_extrude + 0.004;
        let mut half = (self.width / 2.0 + t + 0.01).max(self.tank_width / 2.0 + self.tank_cap_extrude + 0.004);
        if self.handle && !self.hw_side { half = half.max(self.tank_width / 2.0 + self.hw_length + self.hw_cap + 0.01) }
        // the cover: the whole seat plane (front to the hinge), turned up by -cover_rotation about the hinge 1.5 e over the seat
        let cover = (front + mo + self.seat_size) * (-self.cover_rotation).sin() + 2.5 * e + 0.004;
        let top = (self.tank_height + self.tank_cap_height / 2.0 + 0.004).max(cover);
        Some(vec![crate::piece::Volume::hard([-back, -half, -self.height - 0.006], [front, half, top])])
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
    pub beveler_amount: f64,
}

impl Bathtub {
    pub fn new(rng: &mut Rng) -> Bathtub {
        let mut r = rng.clone();
        let mut width = r.uniform(1.5, 2.0);
        let mut size = r.uniform(0.8, 1.0);
        let mut depth = r.uniform(0.55, 0.7);
        // the solver's chosen size (furn::sizes 'exact'): length along the wall, depth from it, rim height
        if let Some(h) = crate::api::size_hint() {
            (width, size, depth) = (h[0], h[1], h[2]);
        }
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
        let mut rb = r.clone();
        let kskip: usize = std::env::var("BEVK").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
        for _ in 0..kskip {
            rb.f64();
        }
        let beveler_amount = rb.uniform(0.001, 0.006);
        Bathtub { width, size, depth, alcove, has_curve, has_legs, thickness, disp_x, disp_y, leg_height, leg_radius, hole_radius, seed: r.randint(0, 1_000_000) as u64, taper, fseed: 0, leg_side, leg_y_scale, leg_sub, alcove_levels, stretch, beveler_amount }
    }
    /// make_box_contour(t, i)
    pub fn contour(&self, t: f64, i: f64) -> Vec<[f64; 2]> {
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
        // the browser build has no OpenSubdiv (the exact tub chains subdivide): a tub of boards there, walls and a floor in the
        // placeholder's box (it panicked the generation on every bathtub)
        if cfg!(target_arch = "wasm32") {
            let mut a = Asset::default();
            let (x0, y1, h) = (-self.size, self.width, self.depth);
            let t = 0.07;
            for (lo, hi) in [([x0, 0.0, 0.0], [0.0, t, h]), ([x0, y1 - t, 0.0], [0.0, y1, h]), ([x0, t, 0.0], [x0 + t, y1 - t, h]), ([-t, t, 0.0], [0.0, y1 - t, h]), ([x0 + t, t, 0.0], [-t, y1 - t, 0.08])] {
                let m = Mesh::cuboid(lo, hi); let b = m.bounds(); a.mesh(m, &ceramic, Some(b));
            }
            let _ = metal;
            return a;
        }
        let b = super::bathtub::build(self, &mut r);
        let mut a = Asset::default();
        a.mesh(b.body, &ceramic, None);
        a.mesh(b.hole, &metal, None);
        if !self.alcove {
            a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        }
        a
    }
    /// collision: an alcove tub fills its box; a freestanding one is its bowl's cage (the contour, wider at the top when curved, cut at
    /// the rim height), the rim's lip, then the two simple deforms (taper, bend) taken on the cage box's corners (their scale factors
    /// are bilinear in x and y/z, the bend's x^2 term extreme at the ends or x = 0), the feet stepping out along the bowl's normals, all
    /// turned by pi/2 as the build is
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        if self.alcove {
            return Some(vec![Volume::hard([-self.size - 0.002, -0.002, -0.002], [0.002, self.width + 0.002, self.depth + 0.004]), Volume::support([-self.size + 0.02, 0.02, self.depth], [-0.02, self.width - 0.02, self.depth + 0.001])]);
        }
        let (w, sz, t) = (self.width, self.size, self.thickness);
        let (dx, dy) = if self.has_curve { (self.disp_x, self.disp_y) } else { ([0.0; 2], 0.0) };
        let deform = |p: [f64; 3], taper: bool, f: f64, range: f64| -> [f64; 3] {
            let sf = f / range.max(1e-6);
            if taper { let sc = 1.0 + p[0] * sf; [p[0], p[1] * sc, p[2] * sc] } else { let sc = p[0] * p[0] * sf - sf + 1.0; [p[0] * (1.0 + sf), p[1] * sc, p[2] * sc] }
        };
        // the cage's x range (the rim's lip included) sets both deforms' ranges
        let (x0, x1) = (-dx[0] - t * 1.05, w + dx[1] + t * 1.05);
        let r0 = x1 - x0;
        let r1 = { let a = deform([x0, 0.0, 0.0], true, self.taper, r0)[0]; let b = deform([x1, 0.0, 0.0], true, self.taper, r0)[0]; (b - a).abs() };
        // a box of the cage (y about the middle) through the deforms, as bounds in the built frame
        let warp = |lo: [f64; 3], hi: [f64; 3]| -> ([f64; 3], [f64; 3]) {
            let (mut a, mut b) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for x in [lo[0], 0.0f64.clamp(lo[0], hi[0]), hi[0]] { for y in [lo[1], hi[1]] { for z in [lo[2], hi[2]] {
                let q = deform(deform([x, y - sz / 2.0, z], true, self.taper, r0), false, self.taper, r1);
                for k in 0..3 { a[k] = a[k].min(q[k]); b[k] = b[k].max(q[k]); }
            } } }
            a[1] += sz / 2.0; b[1] += sz / 2.0;
            (a, b)
        };
        // the bowl: the cage at the top (z = 2 depth) is the widest, the rim's lip grows its loop by t in x, t / 2 in y, and stands up to t
        let (mut lo, mut hi) = warp([x0, -dy - t * 0.55, 0.0], [x1, sz + dy + t * 0.55, self.depth + t]);
        // (the deformed bowl is lifted to the floor)
        let shift = (-lo[2]).max(0.0);
        lo[2] = 0.0; hi[2] += shift;
        let m = 0.004;
        let turn = |lo: [f64; 3], hi: [f64; 3]| Volume::hard([-hi[1] - m, lo[0] - m, lo[2] - m], [-lo[1] + m, hi[0] + m, hi[2] + m]);
        let mut v = vec![turn(lo, hi)];
        if self.has_legs {
            // the feet: from the bowl's lowest band (the cage's bottom, narrower when curved), out along its normals by leg_side (y by
            // leg_y_scale), down to the floor
            let (gx, gy) = (self.leg_side + self.leg_radius * 1.2, self.leg_side * self.leg_y_scale + self.leg_radius * 1.2);
            let (a, b) = warp([dx[0] - gx, dy - gy, 0.0], [w - dx[1] + gx, sz - dy + gy, self.leg_height + self.leg_radius]);
            v.push(turn([a[0], a[1], 0.0], [b[0], b[1], b[2] + shift]));
        }
        Some(v)
    }
}

pub struct Sink {
    pub width: f64,
    pub size: f64,
    pub depth: f64,
    pub alcove: bool,
    pub has_extrude: bool,
    pub has_stand: bool,
    pub alcove_levels: usize,
    pub thickness: f64,
    pub size_extrude: f64,
    pub tap_offset: f64,
    pub stand_radius: f64,
    pub stand_bottom: f64,
    pub stand_height: f64,
    pub stand_circular: bool,
    pub disp_x: f64,
    pub disp_y: f64,
    pub tap_h: f64,
    pub seed: u64,
    pub tub: Bathtub,
    pub fseed: u64,
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
        let tub2 = Bathtub { width, size, depth, alcove, thickness, alcove_levels, disp_x: [tub.disp_x[0], tub.disp_x[0]], has_curve: tub.has_curve, has_legs: tub.has_legs, disp_y: tub.disp_y, leg_height: tub.leg_height, leg_radius: tub.leg_radius, hole_radius: tub.hole_radius, seed: tub.seed, taper: tub.taper, fseed: 0, leg_side: tub.leg_side, leg_y_scale: tub.leg_y_scale, leg_sub: tub.leg_sub, stretch: tub.stretch, beveler_amount: tub.beveler_amount };
        Sink { width, size, depth, alcove, has_extrude, has_stand, alcove_levels, thickness, size_extrude, tap_offset, stand_radius, stand_bottom, stand_height, stand_circular, disp_x: tub.disp_x[0], disp_y: tub.disp_y, tap_h: r.uniform(0.5, 1.0), seed: r.randint(0, 1_000_000) as u64, tub: tub2, fseed: 0 }
    }
}

impl Factory for Sink {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        a.cuboid([-(self.size_extrude + 1.0) * self.size, 0.0, if self.has_stand { -self.stand_height } else { 0.0 }], [0.0, self.width, self.depth], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let ceramic = spec_material("sink_ceramic", serde_json::json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.seed}), [0.92, 0.92, 0.92]);
        let metal = spec_material("sink_metal", serde_json::json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.seed + 1}), [0.7, 0.7, 0.72]);
        let mut r = inst_rng(self.fseed, i);
        let b = super::bathsink::build(self, &mut r);
        let mut a = Asset::default();
        a.mesh(b.body, &ceramic, None);
        a.mesh(b.hole, &metal, None);
        if let Some(t) = b.tap {
            a.mesh(t, &metal, None);
        }
        a
    }
    /// collision: the basin is fitted to its box (bathroom_sink.py scales it to [width, size, depth], the deck extruded back by
    /// size_extrude, the pedestal below); the tap stands on the deck at the back, its lowest x at the deck's back edge plus tap_offset
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        let x0 = -(if self.has_extrude { self.size_extrude } else { 0.0 } + 1.0) * self.size;
        let z0 = if self.has_stand { -self.stand_height } else { 0.0 };
        let m = 0.002;
        let mut v = vec![Volume::hard([x0 - m, -m, z0 - m], [m, self.width + m, self.depth + m])];
        if self.has_extrude {
            // the build's draws before the tap's seed: the pedestal's cuts, a free basin's drain spot
            let mut r = inst_rng(self.fseed, i);
            if self.has_stand { r.randint(3, 6); }
            if !self.alcove { r.uniform(0.35, 0.4); }
            let seed = r.randint(0, 10_000_000) as u64;
            let (lo, hi) = super::taps::tap_bounds(self.fseed, seed);
            let tx = (-1.0 - self.size_extrude + self.tap_offset) * self.size;
            v.push(Volume::hard([tx, self.width / 2.0 + lo[1], self.depth + lo[2]], [tx + hi[0] - lo[0], self.width / 2.0 + hi[1], self.depth + hi[2]]));
        }
        Some(v)
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
        let m = super::matpick::spec_of("hardware_metal", "metal.MetalBasic", self.fseed); // metal_neutral = MetalBasic only
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
    /// collision: the wall plate and stem, then the hook, the holder or the bar (with its second plate) or the ring, in the python
    /// frame (wall normal -y) turned by pi/2
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let _ = i;
        let y = -self.depth;
        let (mut lo, mut hi) = ([-self.ar.max(self.r), -self.depth, -self.ar.max(self.r)], [self.ar.max(self.r), 0.0, self.ar.max(self.r)]);
        let mut add = |l: [f64; 3], h: [f64; 3]| for k in 0..3 { lo[k] = lo[k].min(l[k]); hi[k] = hi[k].max(h[k]); };
        match self.ty {
            0 => add([-self.r, y - self.r, -self.hook_length / 2.0], [self.r, y + self.r, self.hook_length / 2.0]),
            1 => add([-self.ext, y - self.r, -self.r], [self.holder_length, y + self.r, self.r]),
            2 => add([-self.ext, y - self.r, -self.ar.max(self.r)], [self.bar_length + self.ext.max(self.ar), y + self.r, self.ar.max(self.r)]),
            _ => {
                // (the ring's tube: at most 0.7 r)
                let (rr, minor, yc) = (self.ring_radius, self.r * 0.7, self.ad - self.depth);
                add([-rr - minor, yc - minor, -2.0 * rr - minor], [rr + minor, yc + minor, minor]);
            }
        }
        let m = 0.002;
        Some(vec![crate::piece::Volume::hard([-hi[1] - m, lo[0] - m, lo[2] - m], [-lo[1] + m, hi[0] + m, hi[2] + m])])
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
        "BathroomSinkFactory" | "StandingSinkFactory" => {
            let mut k = Sink::new(rng, c == "StandingSinkFactory");
            k.fseed = fseed;
            k.tub.fseed = fseed;
            k.tub.alcove = k.alcove;
            Box::new(k)
        }
        "HardwareFactory" => {
            let mut h = Hardware::new(rng);
            h.fseed = fseed;
            Box::new(h)
        }
        _ => return None,
    })
}
