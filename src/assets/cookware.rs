//! tableware/pan.py, pot.py and appliances/tv.py: PanFactory, PotFactory, TVFactory, MonitorFactory
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    matches!(c, "PanFactory" | "PotFactory" | "TVFactory" | "MonitorFactory")
}

const CUP: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 0.7),
    ("ceramic.VaseCeramic", 4.0),
    ("metal.MetalBasic", 2.0),
    ("plastic.Plastic", 5.0),
    ("ceramic.Glass", 3.0),
];
const WOODS: Weighted = &[("wood.Wood", 1.0), ("wood.WhitePlywood", 0.1), ("wood.BlackPlywood", 0.1), ("wood.BlondePlywood", 0.1)];
const NEUTRAL: Weighted = &[("metal.MetalBasic", 1.0)];

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.6, 0.6, 0.6])
}

pub struct Pan {
    pot: bool,
    r_expand: f64,
    r_mid: f64,
    depth: f64,
    has_handle: bool,
    has_bar: bool,
    x_handle: f64,
    z_handle: f64,
    thickness: f64,
    scale: f64,
    bar_height: f64,
    bar_radius: f64,
    seed: u64,
    has_guard: bool,
    has_handle_hole: bool,
    x_guard: f64,
    guard_depth: f64,
    z_handle_mid: f64,
    s_handle: f64,
    bar_inner: f64,
    bar_x_offset: f64,
    fseed: u64,
}

impl Pan {
    fn new(rng: &mut Rng, pot: bool) -> Pan {
        // PanFactory.__init__ block (fresh FixedSeed stream)
        let mut a = rng.clone();
        let r = &mut a;
        let r_expand = if r.f64() < 0.2 { 1.0 } else { r.log_uniform(1.0, 1.2) };
        let depth = r.log_uniform(0.3, 0.8);
        let r_mid = if r_expand == 1.0 { r.log_uniform(1.0, 1.3) } else { 1.0 + (r_expand - 1.0) * if r.f64() < 0.5 { r.uniform(0.5, 0.85) } else { 0.5 } };
        let has_handle_hole = r.f64() < 0.6;
        let x_handle = r.log_uniform(1.2, 2.0);
        let z_handle = x_handle * r.uniform(0.0, 0.2);
        let z_handle_mid = r.uniform(0.6, 0.8) * z_handle;
        let s_handle = r.log_uniform(0.8, 1.2);
        let thickness = r.log_uniform(0.04, 0.06);
        let has_guard = r.f64() < 0.8;
        let x_guard = r_expand + r.uniform(0.0, 0.2) * x_handle;
        let guard_depth = r.log_uniform(1.0, 2.0) * thickness;
        let scale = r.log_uniform(0.1, 0.15);
        let mut pan = Pan { pot, r_expand, r_mid, depth, has_handle: true, has_bar: false, x_handle, z_handle, thickness, scale, bar_height: 0.0, bar_radius: 0.0, seed: 0, has_guard, has_handle_hole, x_guard, guard_depth, z_handle_mid, s_handle, bar_inner: 0.0, bar_x_offset: 0.0, fseed: 0 };
        if pot {
            // PotFactory.__init__ re-seeds FixedSeed(factory_seed): its own block overrides depth/r_expand/r_mid/scale and adds bars
            let mut b = rng.clone();
            let r = &mut b;
            pan.depth = r.log_uniform(0.6, 2.0);
            pan.r_expand = 1.0;
            pan.r_mid = 1.0;
            pan.has_bar = r.f64() < 0.5;
            // PotFactory: `self.has_handle = not self.has_handle` (Pan sets True) and post_init() is never called
            pan.has_handle = false;
            pan.bar_height = pan.depth * r.uniform(0.75, 0.85);
            pan.bar_radius = r.log_uniform(0.2, 0.3);
            let _bar_x = 1.0 + r.uniform(-pan.bar_radius, pan.bar_radius) * 0.05;
            pan.bar_inner = r.log_uniform(0.2, 0.4) * pan.bar_radius;
            let sc = r.log_uniform(0.6, 1.5);
            let _bs = (r.log_uniform(0.6, 1.0) * sc, sc, r.log_uniform(0.6, 1.2) * sc);
            let _taper = r.log_uniform(0.3, 0.8);
            let _rot = r.uniform(-std::f64::consts::PI / 6.0, 0.0);
            pan.bar_x_offset = pan.bar_radius * r.uniform(-0.1, 0.1);
            let _gd = r.log_uniform(0.5, 1.0) * pan.thickness;
            pan.scale = r.log_uniform(0.1, 0.15);
        }
        pan.seed = a.randint(0, 1_000_000) as u64;
        pan
    }
}

impl Factory for Pan {
    /// PotFactory.create_placeholder (PanFactory has none: bbox of the asset)
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        if !self.pot {
            return None;
        }
        let t = self.thickness;
        let (x0, x1, y0, y1) = if self.has_bar {
            let r = 1.0 + self.bar_x_offset + self.bar_radius + self.bar_inner + t;
            (-r, r, -1.0 - t, 1.0 + t)
        } else if self.has_handle {
            (-1.0 - t, 1.0 + t + self.x_handle, -1.0 - t, 1.0 + t)
        } else {
            (-1.0 - t, 1.0 + t, -1.0 - t, 1.0 + t)
        };
        let s = self.scale;
        let mut a = Asset::default();
        a.cuboid([x0 * s, y0 * s, 0.0], [x1 * s, y1 * s, self.depth * s], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn bounds_hint(&self, i: u64) -> Option<(V3, V3)> {
        if self.pot {
            return None;
        }
        let mut r = inst_rng(self.fseed, i);
        let n = 4 * (r.log_uniform(4.0, 8.0) as usize);
        let p = super::pangeom::PanParams {
            r_expand: self.r_expand,
            r_mid: self.r_mid,
            depth: self.depth,
            x_handle: self.x_handle,
            z_handle: self.z_handle,
            z_handle_mid: self.z_handle_mid,
            s_handle: self.s_handle,
            thickness: self.thickness,
            has_guard: self.has_guard,
            x_guard: self.x_guard,
            guard_depth: self.guard_depth,
            n,
        };
        let m = super::pangeom::pan_unit_mesh(&p)?;
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for q in &m.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        if self.has_handle_hole {
            // add_handle_hole: cutter = 32-gon prism, scale (u, u, 1), x = r_expand + u2 * x_handle (draws follow log_uniform(n) and randint(n // 4))
            let _ = r.randint(0, (n / 4) as i64);
            let rad = r.uniform(0.06, 0.1);
            let cx = self.r_expand + r.uniform(0.8, 0.9) * self.x_handle;
            let (l2, h2) = super::pangeom::bounds_minus_prism(&m, cx, rad);
            lo = l2;
            hi = h2;
        }
        // the flat bottom plateau (fill_grid cap) sits exactly at z = -thickness, the fan used here for the cap cannot reproduce it
        let zmin = lo[2].min(-self.thickness);
        let s = self.scale;
        Some(([lo[0] * s, lo[1] * s, 0.0], [hi[0] * s, hi[1] * s, (hi[2] - zmin) * s]))
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let body = spec("pan", weighted(&mut r, CUP), self.seed);
        let handle_m = spec("pan_handle", weighted(&mut r, CUP), self.seed + 1);
        let mut a = Asset::default();
        let s = self.scale;
        let prof: Vec<(f64, f64)> = chaikin_open(&[(0.0, 0.0), (1.0, 0.0), (self.r_mid, self.depth / 2.0), (self.r_expand, self.depth)], 2)
            .into_iter().map(|(x, z)| (x * s, z * s)).collect();
        let m = Mesh::vessel(0.0, 0.0, &prof, (self.thickness * s).max(0.003), 40);
        let b = m.bounds();
        a.mesh(m, &body, Some(b));
        let top = self.depth * s;
        if self.has_handle {
            let (x0, x1) = (self.r_expand * s, (self.r_expand + self.x_handle) * s);
            let w = 0.12 * s;
            let h = Mesh::cuboid([x0 - 0.002, -w / 2.0, top - 0.012 * s * 3.0], [x1, w / 2.0, top]);
            let hb = h.bounds();
            a.mesh(h, &handle_m, Some(hb));
        }
        if self.has_bar {
            for sgn in [-1.0, 1.0] {
                let rr = self.bar_radius * s;
                let cx = sgn * (self.r_expand * s + rr * 0.8);
                let z = self.bar_height * s;
                let n = 14;
                let pts: Vec<V3> = (0..=n).map(|k| { let t = 2.0 * PI * k as f64 / n as f64; [cx + sgn * rr * 0.6 * t.cos() - sgn * rr * 0.6, 0.0, z + rr * 0.9 * t.sin()] }).collect();
                for k in 0..n {
                    let l = limb(pts[k], pts[k + 1], rr * 0.2, rr * 0.2, true);
                    let lb = l.bounds();
                    a.mesh(l, &handle_m, Some(lb));
                }
            }
        }
        let _ = self.pot;
        a
    }
}

pub struct Tv {
    width: f64,
    height: f64,
    depth: f64,
    side: f64,
    bottom: f64,
    extrude: f64,
    has_extrude: bool,
    two_legs: bool,
    leg_len: f64,
    leg_len_y: f64,
    leg_r: f64,
    leg_w: f64,
    emission: f64,
    seed: u64,
}

impl Tv {
    fn new(rng: &mut Rng, monitor: bool) -> Tv {
        let mut r = rng.clone();
        let aspect = [9.0 / 16.0, 3.0 / 4.0][r.choice_idx(2)];
        let width = r.uniform(0.6, 2.1);
        let _bevel = r.uniform(0.0, 0.01);
        let side = r.log_uniform(0.005, 0.01);
        let bottom = r.uniform(0.005, 0.03);
        let depth = r.uniform(0.02, 0.04);
        let has_extrude = r.f64() < 0.4;
        let extrude = if has_extrude { depth * r.uniform(2.0, 5.0) } else { depth * 1.5 };
        let two_legs = r.choice_idx(2) == 0;
        let leg_len = r.uniform(0.1, 0.2);
        let leg_len_y = r.uniform(0.1, 0.15);
        let leg_r = r.uniform(0.008, 0.015);
        let leg_w = r.uniform(0.5, 0.8);
        let emission = 0.0;
        // MonitorFactory.__init__ re-seeds FixedSeed(factory_seed): width = log_uniform(0.4, 0.8) from the stream start, single-legged
        let (width, two_legs) = if monitor { (rng.clone().log_uniform(0.4, 0.8), false) } else { (width, two_legs) };
        Tv { width, height: aspect * width, depth, side, bottom, extrude, has_extrude, two_legs, leg_len, leg_len_y, leg_r, leg_w, emission, seed: r.randint(0, 1_000_000) as u64 }
    }
}

impl Factory for Tv {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let tw = self.width + 2.0 * self.side;
        let th = self.height + self.side + self.bottom;
        let max_x = if self.two_legs { self.leg_len_y / 2.0 - (1.0 - self.leg_w) * self.extrude } else { self.leg_len_y / 2.0 - self.extrude / 2.0 };
        // pre-rotation box: x depth (neg = front), y width, z height; then +90deg about z
        let (lo, hi) = ([-self.extrude - self.depth, -tw / 2.0, -self.leg_len - self.leg_r / 2.0], [max_x, tw / 2.0, th]);
        let mut a = Asset::default();
        a.cuboid(lo, hi, &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let shell = spec("tv_body", weighted(&mut r, NEUTRAL), self.seed);
        let stand = spec("tv_leg", weighted(&mut r, NEUTRAL), self.seed + 1);
        let screen = spec_material("tv_screen", json!({"cls": "art.Art", "kwargs": {}, "seed": self.seed + 2}), [0.05, 0.05, 0.06]);
        let (tw, th) = (self.width + 2.0 * self.side, self.height + self.side + self.bottom);
        let mut a = Asset::default();
        let d = self.depth;
        // body (back shell, bezelled) and the screen quad on the front (-y) face
        let mut body = Mesh::cuboid([-tw / 2.0, 0.0, 0.0], [tw / 2.0, d, th]);
        if self.has_extrude {
            body.append(&Mesh::cuboid([-tw * 0.2, d, th * 0.25], [tw * 0.2, d + self.extrude, th * 0.6]));
        }
        let bb = body.bounds();
        a.mesh(body, &shell, Some(bb));
        let (sx0, sx1, sz0, sz1) = (-self.width / 2.0, self.width / 2.0, self.bottom, self.bottom + self.height);
        let mut sc = Mesh::new();
        sc.add_quad([sx0, -0.001, sz0], [sx1, -0.001, sz0], [sx1, -0.001, sz1], [sx0, -0.001, sz1]);
        sc.uv = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        a.mesh(sc, &screen, Some(([sx0, -0.002, sz0], [sx1, 0.0, sz1])));
        let lr = self.leg_r;
        if self.two_legs {
            for sgn in [-1.0, 1.0] {
                let x = sgn * tw / 2.0 * self.leg_w;
                for sy in [-1.0, 1.0] {
                    let l = limb([x * 0.9, d / 2.0, th * 0.35], [x, d / 2.0 + sy * self.leg_len_y / 2.0, -self.leg_len], lr, lr, true);
                    let lb = l.bounds();
                    a.mesh(l, &stand, Some(lb));
                }
            }
        } else {
            let l = limb([0.0, d / 2.0, th * 0.3], [0.0, d / 2.0, -self.leg_len], lr, lr, true);
            let lb = l.bounds();
            a.mesh(l, &stand, Some(lb));
            let foot = Mesh::cuboid([-self.leg_w * tw * 0.25, d / 2.0 - self.leg_len_y / 2.0, -self.leg_len - lr], [self.leg_w * tw * 0.25, d / 2.0 + self.leg_len_y / 2.0, -self.leg_len + lr]);
            let fb = foot.bounds();
            a.mesh(foot, &stand, Some(fb));
        }
        let _ = (self.emission, WOODS.len());
        a.transform(&Mat4::rot_z(PI / 2.0));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(match c {
        "PanFactory" => {
            let mut p = Pan::new(rng, false);
            p.fseed = fseed;
            Box::new(p)
        }
        "PotFactory" => {
            let mut p = Pan::new(rng, true);
            p.fseed = fseed;
            Box::new(p)
        }
        "TVFactory" => Box::new(Tv::new(rng, false)),
        _ => Box::new(Tv::new(rng, true)),
    })
}
