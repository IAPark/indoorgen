//! tableware/plant_container.py, tableware/pot.py|pan.py, monocot/*.py (+ simplified small_plants/cactus/mushroom):
//! PlantContainerFactory and LargePlantContainerFactory with real pot geometry and procedural leaf growth.
use super::*;
use super::matdraw;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "PlantContainerFactory" | "LargePlantContainerFactory")
}

const DECOR_HARD: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("ceramic.VaseCeramic", 4.0),
    ("ceramic.ColoredGlass", 2.0),
    ("ceramic.Marble", 1.0),
    ("ceramic.GlassVolume", 2.0),
    ("ceramic.Ceramic", 1.0),
];
const SOIL: Weighted = &[("terrain.Mud", 1.0), ("terrain.Sand", 1.0), ("terrain.Soil", 3.0), ("terrain.Dirt", 6.0)];

fn hsv(h: f64, s: f64, v: f64) -> [f32; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r as f32, g as f32, b as f32]
}

/// Catmull-Rom style smooth interpolation of anchors (x increasing) sampled at n points
fn smooth_profile(xs: &[f64], ys: &[f64], n: usize) -> Vec<(f64, f64)> {
    let x0 = xs[0];
    let x1 = *xs.last().unwrap();
    (0..n)
        .map(|k| {
            let x = x0 + (x1 - x0) * k as f64 / (n - 1) as f64;
            // find segment
            let mut s = 0;
            while s + 2 < xs.len() && x > xs[s + 1] {
                s += 1;
            }
            let t = ((x - xs[s]) / (xs[s + 1] - xs[s]).max(1e-9)).clamp(0.0, 1.0);
            let y = |i: i64| ys[i.clamp(0, ys.len() as i64 - 1) as usize];
            let (p0, p1, p2, p3) = (y(s as i64 - 1), y(s as i64), y(s as i64 + 1), y(s as i64 + 2));
            let t2 = t * t;
            let t3 = t2 * t;
            let v = 0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
            (x, v.max(0.0))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Tussock,
    Grasses,
    Maize,
    Agave,
    Banana,
    Taro,
    Veratrum,
    Fern,
    Succulent,
    Spider,
    Snake,
    Cactus,
    Mushroom,
}

struct Growth {
    kind: Kind,
    count: usize,
    perturb: f64,
    angle: f64,
    min_y: f64,
    max_y: f64,
    leaf_prob: f64,
    leaf_range: (f64, f64),
    stem_offset: f64,
    scale_curve: Vec<(f64, f64)>,
    radius: f64,
    bend: f64,
    twist: f64,
    z_drag: f64,
    z_scale: f64,
    hue: f64,
    bud_angle: f64,
    bright: [f32; 3],
    dark: [f32; 3],
}

fn pi() -> f64 {
    std::f64::consts::PI
}

impl Growth {
    fn new(kind: Kind, r: &mut Rng) -> Growth {
        let mut g = Growth {
            kind,
            count: 128,
            perturb: 0.05,
            angle: pi() / 6.0,
            min_y: 0.0,
            max_y: pi() / 2.0,
            leaf_prob: r.uniform(0.8, 0.9),
            leaf_range: (0.0, 1.0),
            stem_offset: 0.2,
            scale_curve: vec![(0.0, 1.0), (1.0, 1.0)],
            radius: 0.01,
            bend: pi() / 4.0,
            twist: pi() / 6.0,
            z_drag: 0.0,
            z_scale: r.uniform(1.0, 1.2),
            hue: r.uniform(0.15, 0.35),
            bud_angle: 0.0,
            bright: [0.0; 3],
            dark: [0.0; 3],
        };
        match kind {
            Kind::Tussock => {
                g.stem_offset = r.uniform(0.0, 0.2);
                g.angle = r.uniform(pi() / 20.0, pi() / 18.0);
                g.z_drag = r.uniform(0.1, 0.2);
                g.min_y = r.uniform(pi() * 0.2, pi() * 0.25);
                g.count = r.log_uniform(512.0, 1024.0) as usize;
                g.scale_curve = vec![(0.0, r.uniform(0.6, 1.0)), (1.0, r.uniform(0.6, 1.0))];
            }
            Kind::Grasses | Kind::Maize => {
                g.stem_offset = r.uniform(1.5, 2.0);
                g.angle = r.uniform(pi() / 6.0, pi() / 3.0);
                g.z_drag = r.uniform(0.0, 0.2);
                g.min_y = r.uniform(pi() * 0.35, pi() * 0.45);
                g.max_y = r.uniform(pi() * 0.45, pi() * 0.5);
                g.count = r.log_uniform(16.0, 64.0) as usize;
                g.scale_curve = vec![(0.0, 1.0), (1.0, 0.2)];
                g.bend = pi() / 2.0;
            }
            Kind::Agave => {
                g.stem_offset = r.uniform(0.0, 0.5);
                g.angle = r.uniform(pi() / 9.0, pi() / 6.0);
                g.z_drag = r.uniform(0.05, 0.1);
                g.min_y = r.uniform(pi() * 0.1, pi() * 0.15);
                g.max_y = r.uniform(pi() * 0.4, pi() * 0.52);
                g.count = r.log_uniform(32.0, 64.0) as usize;
                g.scale_curve = vec![(0.0, r.uniform(0.8, 1.0)), (0.5, 1.0), (1.0, r.uniform(0.6, 1.0))];
                g.bud_angle = r.uniform(pi() / 8.0, pi() / 4.0);
            }
            Kind::Banana | Kind::Taro => {
                g.stem_offset = r.uniform(0.6, 1.0);
                g.angle = r.uniform(pi() / 4.0, pi() / 3.0);
                g.z_scale = r.uniform(1.0, 1.5);
                g.z_drag = r.uniform(0.1, 0.2);
                g.min_y = r.uniform(pi() * 0.05, pi() * 0.1);
                g.max_y = r.uniform(pi() * 0.25, pi() * 0.45);
                g.leaf_range = (r.uniform(0.5, 0.7), 1.0);
                g.count = r.log_uniform(16.0, 24.0) as usize;
                g.scale_curve = vec![(0.0, r.uniform(0.4, 1.0)), (1.0, r.uniform(0.6, 1.0))];
                g.radius = r.uniform(0.04, 0.06);
                g.bud_angle = r.uniform(pi() / 8.0, pi() / 6.0);
                if kind == Kind::Taro {
                    g.stem_offset = r.uniform(0.05, 0.1);
                    g.radius = r.uniform(0.02, 0.04);
                    g.z_drag = r.uniform(0.2, 0.3);
                    g.bud_angle = r.uniform(pi() * 0.6, pi() * 0.7);
                    g.count = r.log_uniform(12.0, 16.0) as usize;
                    g.min_y = r.uniform(-pi() * 0.25, -pi() * 0.05);
                    g.max_y = r.uniform(-pi() * 0.05, 0.0);
                }
            }
            Kind::Veratrum => {
                g.stem_offset = r.uniform(0.3, 0.6);
                g.angle = r.uniform(pi() / 6.0, pi() / 4.0);
                g.min_y = r.uniform(pi() * 0.1, pi() * 0.2);
                g.max_y = r.uniform(pi() * 0.3, pi() * 0.4);
                g.count = r.log_uniform(16.0, 40.0) as usize;
                g.scale_curve = vec![(0.0, 1.0), (1.0, 0.5)];
            }
            Kind::Fern => {
                g.stem_offset = r.uniform(0.0, 0.1);
                g.angle = r.uniform(pi() / 4.0, pi() / 3.0);
                g.min_y = r.uniform(pi() * 0.15, pi() * 0.25);
                g.max_y = r.uniform(pi() * 0.35, pi() * 0.45);
                g.count = r.log_uniform(12.0, 24.0) as usize;
                g.bend = pi() / 3.0;
            }
            Kind::Succulent => {
                g.stem_offset = r.uniform(0.0, 0.05);
                g.angle = r.uniform(pi() / 6.0, pi() * 0.45);
                g.min_y = r.uniform(pi() * 0.05, pi() * 0.15);
                g.max_y = r.uniform(pi() * 0.4, pi() * 0.5);
                g.count = r.log_uniform(24.0, 60.0) as usize;
                g.scale_curve = vec![(0.0, 0.5), (1.0, 1.0)];
            }
            Kind::Spider => {
                g.stem_offset = r.uniform(0.0, 0.1);
                g.angle = r.uniform(pi() / 8.0, pi() / 5.0);
                g.min_y = r.uniform(pi() * 0.1, pi() * 0.2);
                g.max_y = r.uniform(pi() * 0.3, pi() * 0.45);
                g.count = r.log_uniform(30.0, 70.0) as usize;
                g.bend = pi() / 2.0;
                g.z_drag = r.uniform(0.2, 0.5);
            }
            Kind::Snake => {
                g.stem_offset = r.uniform(0.0, 0.1);
                g.angle = r.uniform(pi() / 3.0, pi() / 2.0);
                g.min_y = 0.0;
                g.max_y = r.uniform(pi() * 0.05, pi() * 0.12);
                g.count = r.log_uniform(6.0, 14.0) as usize;
                g.bend = 0.2;
            }
            Kind::Cactus | Kind::Mushroom => {}
        }
        g.bright = hsv(g.hue, r.uniform(0.6, 0.8), r.log_uniform(0.05, 0.1) * 6.0);
        g.dark = hsv(g.hue + r.uniform(-0.03, 0.03), r.uniform(0.8, 1.0), r.log_uniform(0.05, 0.2) * 3.0);
        g
    }

    fn leaf_anchors(&self, r: &mut Rng) -> (Vec<f64>, Vec<f64>) {
        match self.kind {
            Kind::Tussock => (vec![0.0, r.uniform(0.3, 0.7), 1.0], vec![0.0, 0.01, 0.0]),
            Kind::Grasses => (vec![0.0, r.uniform(0.1, 0.2), r.uniform(0.5, 0.7), 1.0], vec![0.0, r.uniform(0.02, 0.03), r.uniform(0.02, 0.03), 0.0]),
            Kind::Maize => (vec![0.0, r.uniform(0.1, 0.2), r.uniform(0.5, 0.7), 1.0], vec![0.0, r.uniform(0.03, 0.06), r.uniform(0.03, 0.06), 0.0]),
            Kind::Agave => (
                vec![0.0, 0.2 * self.bud_angle.cos(), r.uniform(1.0, 1.4), 1.5],
                vec![0.0, 0.2 * self.bud_angle.sin(), r.uniform(0.1, 0.15), 0.0],
            ),
            Kind::Banana | Kind::Taro => (
                vec![0.0, 0.2 * self.bud_angle.cos(), r.uniform(0.8, 1.2), 2.0],
                vec![0.0, 0.2 * self.bud_angle.sin(), r.uniform(0.2, 0.25), 0.0],
            ),
            Kind::Veratrum => (vec![0.0, 0.1, r.uniform(0.5, 0.7), 1.0], vec![0.0, r.uniform(0.06, 0.1), r.uniform(0.05, 0.09), 0.0]),
            Kind::Fern => (vec![0.0, 0.3, 0.7, 1.0], vec![0.0, r.uniform(0.04, 0.07), r.uniform(0.03, 0.06), 0.0]),
            Kind::Succulent => (vec![0.0, 0.3, 0.7, 1.0], vec![0.0, r.uniform(0.18, 0.25), r.uniform(0.12, 0.2), 0.0]),
            Kind::Spider => (vec![0.0, 0.3, 0.7, 1.0], vec![0.0, 0.025, 0.02, 0.0]),
            Kind::Snake => (vec![0.0, 0.2, 0.7, 1.0], vec![0.0, r.uniform(0.09, 0.14), r.uniform(0.08, 0.12), 0.0]),
            _ => (vec![0.0, 0.5, 1.0], vec![0.0, 0.05, 0.0]),
        }
    }

    /// leaf blade in local frame: x along the blade (length ~1), y across, with a bend about y and gentle droop
    fn leaf(&self, r: &mut Rng) -> Mesh {
        let (xs, ys) = self.leaf_anchors(r);
        let prof = smooth_profile(&xs, &ys, 14);
        let y_bend = r.uniform(0.5, 1.0) * pi() / 6.0 * if matches!(self.kind, Kind::Fern | Kind::Spider) { 2.5 } else { 1.0 };
        let z_bend = r.uniform(-1.0, 1.0) * pi() / 6.0;
        let ybend_ratio = 4.0 * r.f64();
        let len = *xs.last().unwrap();
        let mut spine = vec![];
        let mut w = vec![];
        let mut side = vec![];
        for &(x, hw) in &prof {
            let t = x / len;
            let a = y_bend * t; // curvature: tip bends downward
            let zb = z_bend * t;
            spine.push([x * (1.0 - 0.2 * a), zb * x * 0.3, -a * x * 0.6 + ybend_ratio * 0.0]);
            w.push(hw);
            side.push([0.0, 1.0, 0.0]);
        }
        let _ = ybend_ratio;
        // fold along the midrib for stiff leaves (succulent/snake/agave)
        let mut m = Mesh::ribbon(&spine, &w, &side);
        if matches!(self.kind, Kind::Succulent | Kind::Snake | Kind::Agave | Kind::Banana | Kind::Taro) {
            let mut thick = Mesh::ribbon(&spine.iter().map(|p| [p[0], p[1], p[2] - 0.01]).collect::<Vec<_>>(), &w.iter().map(|x| x * 0.9).collect::<Vec<_>>(), &side);
            thick.pos.iter_mut().for_each(|_| {});
            m.append(&thick);
        }
        m
    }

    /// full plant mesh at the origin (z up), already decorated
    fn build(&self, r: &mut Rng) -> Mesh {
        let mut out = Mesh::new();
        let variants: Vec<Mesh> = (0..10).map(|_| self.leaf(r)).collect();
        // stem
        let stem_r = self.radius.max(0.004);
        if self.stem_offset > 0.01 {
            out.append(&Mesh::cylinder(0.0, 0.0, 0.0, self.stem_offset, stem_r, 8));
        }
        let mut z_rot = 0.0;
        let freq = 2.0 * pi() / self.angle;
        let angle = if (0.01..0.05).contains(&(freq - freq.trunc())) {
            2.0 * pi() / (freq + 0.05)
        } else if (-0.05..-0.01).contains(&(freq - freq.trunc())) {
            2.0 * pi() / (freq - 0.05)
        } else {
            self.angle
        };
        for k in 0..self.count {
            let p = if self.count > 1 { k as f64 / (self.count - 1) as f64 } else { 0.0 };
            z_rot += r.uniform(angle * 0.95, angle * 1.05);
            if r.f64() >= self.leaf_prob || p < self.leaf_range.0 || p > self.leaf_range.1 {
                continue;
            }
            let y_rot = -(self.min_y + (self.max_y - self.min_y) * p);
            // scale curve (piecewise linear)
            let mut sc = self.scale_curve.last().unwrap().1;
            for w in self.scale_curve.windows(2) {
                if p >= w[0].0 && p <= w[1].0 {
                    let t = (p - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
                    sc = w[0].1 + (w[1].1 - w[0].1) * t;
                    break;
                }
            }
            sc += r.uniform(-self.perturb, self.perturb);
            let rot = [r.uniform(-self.perturb, self.perturb), y_rot + r.uniform(-self.perturb, self.perturb), z_rot + r.uniform(-self.perturb, self.perturb)];
            let mut leaf = variants[r.choice_idx(variants.len())].clone();
            let m = Mat4::translation([0.0, 0.0, self.stem_offset * p]).mul(&Mat4::euler_xyz(rot)).mul(&Mat4::scale([sc, sc, sc]));
            leaf.transform(&m);
            out.append(&leaf);
        }
        // decorate_monocot: z drag, bend
        let drag = self.z_drag;
        for p in out.pos.iter_mut() {
            p[2] -= drag * (p[0] * p[0] + p[1] * p[1]);
        }
        let s = [r.uniform(0.8, 1.2), r.uniform(0.8, 1.2), self.z_scale];
        let rz = r.uniform(0.0, 2.0 * pi());
        out.transform(&Mat4::rot_z(rz).mul(&Mat4::scale(s)));
        out
    }
}

struct Pot {
    depth: f64,
    r_expand: f64,
    r_mid: f64,
    thickness: f64,
    scale: f64,
    mat: Material,
}

impl Pot {
    /// PlantPotFactory: PotFactory.scale (own FixedSeed block), then PlantPot block (depth, r_expand, alpha)
    fn new(fs: u64, mat_cls: &str, mat_seed: i64) -> Pot {
        // PotFactory block: restarts at the factory seed
        let mut r = Rng::seeded(fs);
        r.log_uniform(0.6, 2.0); // depth
        r.uniform(0.0, 1.0); // has_bar
        r.uniform(0.75, 0.85);
        let bar_radius = r.log_uniform(0.2, 0.3);
        r.uniform(-bar_radius, bar_radius);
        r.log_uniform(0.2, 0.4);
        r.log_uniform(0.6, 1.5);
        r.log_uniform(0.6, 1.0);
        r.log_uniform(0.6, 1.2);
        r.log_uniform(0.3, 0.8);
        r.uniform(-std::f64::consts::PI / 6.0, 0.0);
        r.uniform(-0.1, 0.1);
        r.log_uniform(0.5, 1.0);
        let scale = r.log_uniform(0.1, 0.15);
        // PanFactory block holds the thickness
        let mut rp = Rng::seeded(fs);
        rp.uniform(0.0, 1.0);
        rp.log_uniform(1.0, 1.2);
        // thickness is only needed by the approximate geometry: use the Pan stream value (see asset())
        let mut rq = Rng::seeded(fs);
        let depth = rq.log_uniform(0.5, 1.0);
        let r_expand = rq.uniform(1.1, 1.3);
        let alpha = rq.uniform(0.5, 0.8);
        let r_mid = (r_expand - 1.0) * alpha + 1.0;
        let mat = spec_material(&format!("pot:{mat_cls}"), json!({"cls": mat_cls, "kwargs": {}, "seed": mat_seed}), [0.6, 0.35, 0.25]);
        Pot { depth, r_expand, r_mid, thickness: 0.05, scale, mat }
    }
    fn mesh(&self) -> Mesh {
        let s = self.scale;
        let prof = [(1.0 * s, 0.0), (self.r_mid * s, self.depth / 2.0 * s), (self.r_expand * s, self.depth * s)];
        Mesh::vessel(0.0, 0.0, &smooth(&prof), self.thickness * s * 1.4, 28)
    }
}

fn smooth(p: &[(f64, f64)]) -> Vec<(f64, f64)> {
    // densify the 3-point profile with a quadratic curve
    let (a, b, c) = (p[0], p[1], p[2]);
    (0..=8)
        .map(|i| {
            let t = i as f64 / 8.0;
            ((1.0 - t) * (1.0 - t) * a.0 + 2.0 * t * (1.0 - t) * b.0 + t * t * c.0, (1.0 - t) * (1.0 - t) * a.1 + 2.0 * t * (1.0 - t) * b.1 + t * t * c.1)
        })
        .collect()
}

pub struct PlantContainer {
    pot: Pot,
    side_size: f64,
    top_size: f64,
    dirt_ratio: f64,
    kind: Kind,
    factory_seed: u64,
    large: bool,
    succ_thin: bool,
}

impl PlantContainer {
    fn new(large: bool, fs: u64) -> PlantContainer {
        // container stream: PlantPotFactory's trailing weighted_sample(decorative_hard)()() runs on it first
        let mut r = Rng::seeded(fs);
        let cls = matdraw::sample_call(&mut r, matdraw::DECORATIVE_HARD);
        let kinds = [Kind::Cactus, Kind::Mushroom, Kind::Fern, Kind::Succulent, Kind::Spider, Kind::Snake];
        let ki = r.randint(0, kinds.len() as i64) as usize;
        let dirt_ratio = r.uniform(0.7, 0.8);
        let mut succ_thin = true;
        if kinds[ki] == Kind::Succulent {
            succ_thin = r.choice_p(&[0.65, 0.35]) == 0; // SucculentFactory.__init__ draws outside its own FixedSeed
        }
        let top0 = r.uniform(0.4, 0.6);
        let mut pot = Pot::new(fs, cls, (fs % 1_000_000) as i64);
        let mut side_size = pot.scale * pot.r_expand;
        let mut top_size = top0;
        let mut kind = kinds[ki];
        if large {
            let mut r = Rng::seeded(fs);
            pot.depth = r.log_uniform(1.0, 1.5);
            pot.scale = r.log_uniform(0.15, 0.25);
            side_size = pot.scale * r.uniform(1.5, 2.0) * pot.r_expand;
            top_size = r.uniform(1.0, 1.5);
            let ks = [Kind::Tussock, Kind::Grasses, Kind::Maize, Kind::Agave, Kind::Banana, Kind::Taro, Kind::Veratrum];
            kind = ks[Rng::seeded(fs).choice_idx(ks.len())];
        }
        PlantContainer { pot, side_size, top_size, dirt_ratio, kind, factory_seed: fs, large, succ_thin }
    }
}

impl Factory for PlantContainer {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        // new_bbox: unit cube with object scale / location (float32 properties), applied as x * s + l in float32
        let ax = |a: f64, b: f64| -> (f64, f64) {
            let (l, s) = (((a + b) / 2.0) as f32, ((b - a) / 2.0) as f32);
            (((-1.0f32) * s + l) as f64, (1.0f32 * s + l) as f64)
        };
        let (x0, x1) = ax(-self.side_size, self.side_size);
        let (z0, z1) = ax(-0.02, self.pot.depth * self.pot.scale + self.top_size);
        let mut a = Asset::default();
        a.cuboid([x0, x0, z0], [x1, x1, z1], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }

    fn asset(&self, i: u64) -> Asset {
        let mut a = Asset::default();
        let pot_mesh = self.pot.mesh();
        a.mesh(pot_mesh, &self.pot.mat, Some(([-self.pot.scale * self.pot.r_expand, -self.pot.scale * self.pot.r_expand, 0.0], [self.pot.scale * self.pot.r_expand, self.pot.scale * self.pot.r_expand, self.pot.depth * self.pot.scale])));
        let dirt_z = self.dirt_ratio * self.pot.depth * self.pot.scale;
        let soil = weighted(&mut inst_rng(self.factory_seed, 99), SOIL);
        let cname = if self.large { "LargePlantContainerFactory" } else { "PlantContainerFactory" };
        // material of the plant, replayed by python: PlantContainerFactory(fs).<attr>
        let pm = |name: &str, attr: &str, rgb: [f32; 3]| ig_core::materials::spec_material(name, json!({"cls": format!("factory.tableware.plant_container.{}", cname), "kwargs": {"attr": attr}, "seed": self.factory_seed}), rgb);
        let dirt_r = self.pot.scale * (1.0 + (self.pot.r_mid - 1.0) * 0.8) * 0.92;
        a.mesh(Mesh::cylinder(0.0, 0.0, dirt_z - 0.02, dirt_z, dirt_r, 20), &{
            let _ = soil;
            ig_core::materials::spec_material(&format!("soil:{soil}"), json!({"cls": format!("factory.tableware.plant_container.{}", cname), "kwargs": {"attr": "dirt_surface"}, "seed": self.factory_seed}), [0.18, 0.12, 0.08])
        }, None);
        let mut r = inst_rng(self.factory_seed, i);
        let mut plant = Mesh::new();
        let leaf_mat;
        if self.large && super::monocot::supported(self.factory_seed) {
            // exact monocot (assets::monocot) placed like PlantContainerFactory.create_asset: origin2lowest(approximate), fit into [side, side, top]
            let m = super::monocot::MonocotFactory::new(self.factory_seed).mesh(i);
            let (v, sc) = super::monocot::fit_in_container(&m, self.side_size, self.top_size, dirt_z);
            let _ = sc;
            let mesh = Mesh { pos: v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.f.clone(), uv: vec![] };
            let b = mesh.bounds();
            a.mesh(mesh, &pm("leaf", "plant_factory.factory.material", [0.1, 0.3, 0.1]), Some(b));
            return a;
        }
        match self.kind {
            Kind::Cactus => {
                plant = super::cactus::CactusFactory::new(self.factory_seed).mesh(i, 0.003);
                leaf_mat = pm("cactus", "plant_factory.material", [0.1, 0.3, 0.12]);
            }
            Kind::Mushroom => {
                plant = super::mushroom::MushroomFactory::new(self.factory_seed).mesh(i);
                leaf_mat = pm("mushroom", "plant_factory.factory.cap_factory.material_cap", [0.5, 0.35, 0.25]);
            }
            Kind::Snake | Kind::Spider | Kind::Succulent => {
                // exact node-graph ports (snakeplant.rs, spiderplant.rs, succulent.rs): the plant factory is built with the container's seed
                let f: Box<dyn Factory> = match self.kind {
                    Kind::Snake => super::snakeplant::make("SnakePlantFactory", &mut r, self.factory_seed).unwrap(),
                    Kind::Spider => super::spiderplant::make("SpiderPlantFactory", &mut r, self.factory_seed).unwrap(),
                    _ => Box::new(super::succulent::Succulent::with_mode(self.factory_seed, self.succ_thin)),
                };
                let pa = f.asset(i);
                plant = pa.parts[0].mesh.clone();
                leaf_mat = pa.parts[0].mat.clone();
            }
            Kind::Fern => {
                plant = super::fern::FernFactory::new(self.factory_seed).mesh(i);
                leaf_mat = ig_core::materials::spec_material("fern", json!({"cls": "plant.SimpleGreenery", "kwargs": {}, "seed": self.factory_seed}), [0.1, 0.35, 0.1]);
            }
            k => {
                let g = Growth::new(k, &mut r);
                plant = g.build(&mut r);
                leaf_mat = pm("leaf", "plant_factory.factory.material", g.dark);
                let _ = g.bright;
            }
        }
        // origin2lowest + fit into [side, side, top]
        let (lo, hi) = plant.bounds();
        let ext = [lo[0].abs().max(hi[0].abs()), lo[1].abs().max(hi[1].abs()), (hi[2] - lo[2]).max(1e-6)];
        let scale = (self.side_size / ext[0].max(1e-6)).min(self.side_size / ext[1].max(1e-6)).min(self.top_size / ext[2]);
        plant.transform(&Mat4::translation([0.0, 0.0, dirt_z]).mul(&Mat4::scale([scale; 3])).mul(&Mat4::translation([0.0, 0.0, -lo[2]])));
        let (plo, phi) = plant.bounds();
        a.mesh(plant, &leaf_mat, Some((plo, phi)));
        let _ = self.large;
        a
    }
}

pub fn make(c: &str, _rng: &mut Rng, seed: u64) -> Option<Box<dyn Factory>> {
    Some(match c {
        "PlantContainerFactory" => Box::new(PlantContainer::new(false, seed)),
        "LargePlantContainerFactory" => Box::new(PlantContainer::new(true, seed)),
        _ => return None,
    })
}
