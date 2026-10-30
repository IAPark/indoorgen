//! lamp/ceiling_lights.py: CeilingLightFactory (black housing + translucent bulb cover; the point light sits at z=-0.03)
use super::gnlite::{self, F3};
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "CeilingLightFactory" | "DeskLampFactory" | "FloorLampFactory" | "LampFactory")
}

pub struct CeilingLight {
    radius: f64,
    thickness: f64,
    inner_radius: f64,
    height: f64,
    inner_height: f64,
    curvature: f64,
    seed: u64,
}

fn clip_gaussian(r: &mut Rng, mean: f64, std: f64, lo: f64, hi: f64) -> f64 {
    let mut i = 0;
    loop {
        let v = r.normal(mean, std);
        if v >= lo && v <= hi {
            return v;
        }
        if i == 20 {
            return v.clamp(lo, hi);
        }
        i += 1;
    }
}

impl CeilingLight {
    fn new(rng: &mut Rng) -> CeilingLight {
        let mut r = rng.clone();
        let radius = clip_gaussian(&mut r, 0.12, 0.04, 0.1, 0.25);
        let thickness = r.uniform(0.005, 0.05);
        let inner_radius = radius * r.uniform(0.4, 0.9);
        let height = 0.7 * clip_gaussian(&mut r, 0.09, 0.03, 0.07, 0.15);
        let inner_height = height * r.uniform(0.5, 1.1);
        let curvature = r.uniform(0.1, 0.5);
        CeilingLight { radius, thickness, inner_radius, height, inner_height, curvature, seed: r.randint(0, 1_000_000) as u64 }
    }
}

impl Factory for CeilingLight {
    fn asset(&self, _i: u64) -> Asset {
        let black = spec_material("ceiling_black", json!({"cls": "lamp_shaders.shader_black", "kwargs": {}, "seed": self.seed}), [0.15, 0.15, 0.15]);
        let white = spec_material("ceiling_bulb", json!({"cls": "lamp_shaders.shader_lamp_bulb_nonemissive", "kwargs": {}, "seed": self.seed}), [0.8, 0.75, 0.65]);
        let (r, t, h) = (self.radius, self.thickness, self.height);
        let mut a = Asset::default();
        // housing: ring wall (outer R+t / inner R), flat top disc
        // open tubes: Mesh::cylinder/frustum add end caps, which closed the housing and trapped the point light inside
        let mut wall = open_tube(r + t, -h, 0.0, 48);
        wall.append(&open_tube(r, -h, 0.0, 48).flipped());
        let rim = ring_flat(r, r + t, -h, 48);
        wall.append(&rim);
        let disc = Mesh::cylinder(0.0, 0.0, -0.0005, 0.0, r, 48);
        wall.append(&disc);
        let wb = wall.bounds();
        a.mesh(wall, &black, Some(wb));
        // bulb cover: cylinder + curved cap (translucent to light, transparent to the camera)
        let ih = self.inner_height;
        let mut cover = Mesh::cylinder(0.0, 0.0, -ih, -0.001, self.inner_radius, 32); // python: CurveToMesh with Fill Caps
        let mut cap = Mesh::ellipsoid([0.0, 0.0, -ih], [self.inner_radius, self.inner_radius, self.inner_radius * self.curvature], 32, 8);
        // keep the lower hemisphere only
        let mut keep = Mesh::new();
        for t3 in &cap.triangles() {
            let c = [0, 1, 2].map(|k| (cap.pos[t3[0] as usize][k] + cap.pos[t3[1] as usize][k] + cap.pos[t3[2] as usize][k]) / 3.0);
            if c[2] < -ih + 1e-6 {
                keep.add_tri(cap.pos[t3[0] as usize], cap.pos[t3[1] as usize], cap.pos[t3[2] as usize]);
            }
        }
        cap = keep;
        cover.append(&cap);
        let cb = cover.bounds();
        a.mesh(cover, &white, Some(cb));
        a
    }
}

/// side wall of a cylinder (no end caps), outward normals
fn open_tube(r: f64, z0: f64, z1: f64, n: usize) -> Mesh {
    let mut m = Mesh::new();
    for i in 0..n {
        let (a0, a1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
        m.add_quad([r * a0.cos(), r * a0.sin(), z0], [r * a1.cos(), r * a1.sin(), z0], [r * a1.cos(), r * a1.sin(), z1], [r * a0.cos(), r * a0.sin(), z1]);
    }
    m
}

fn ring_flat(r0: f64, r1: f64, z: f64, n: usize) -> Mesh {
    let mut m = Mesh::new();
    for i in 0..n {
        let (a0, a1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
        m.add_quad([r0 * a0.cos(), r0 * a0.sin(), z], [r1 * a0.cos(), r1 * a0.sin(), z], [r1 * a1.cos(), r1 * a1.sin(), z], [r0 * a1.cos(), r0 * a1.sin(), z]);
    }
    m
}

pub fn make(c: &str, rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(match c {
        "CeilingLightFactory" => Box::new(CeilingLight::new(rng)),
        "DeskLampFactory" => Box::new(StandLamp::new(fs, true)),
        "FloorLampFactory" | "LampFactory" => Box::new(StandLamp::new(fs, false)),
        _ => return None,
    })
}

pub struct StandLamp {
    p: LampParams,
    seed: u64,
}

#[derive(Clone)]
pub struct LampParams {
    pub stand_radius: f64,
    pub base_radius: f64,
    pub base_height: f64,
    pub shade_height: f64,
    pub top_radius: f64,
    pub bot_radius: f64,
    pub rack_t: f64,
    pub cp: [[f64; 3]; 3],
}

const LAMPSHADE: Weighted = &[("lamp_shaders.LampShade", 1.0)];
const LEG_MATERIAL: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("wood.Wood", 1.0),
    ("plastic.PlasticRough", 1.0),
    ("metal.BrushedBlackMetal", 3.0),
    ("wood.BlackPlywood", 3.0),
];

impl StandLamp {
    /// LampFactory.sample_parameters inside FixedSeed(factory_seed)
    fn new(fs: u64, desk: bool) -> StandLamp {
        let mut r = Rng::seeded(fs);
        let stand_radius = r.uniform(0.005, 0.015);
        let base_radius = r.uniform(0.05, 0.15);
        let base_height = r.uniform(0.01, 0.03);
        let shade_height = r.uniform(0.18, 0.3);
        let top_radius = r.uniform(0.07, 0.15);
        let bot_radius = top_radius + r.uniform(0.0, 0.05);
        let rack_t = r.uniform(0.001, 0.003);
        let height = if desk { r.uniform(0.25, 0.4) } else { r.uniform(1.0, 1.5) };
        let z1 = r.uniform(base_height, height);
        let z2 = r.uniform(z1, height);
        let z3 = height;
        let p = LampParams { stand_radius, base_radius, base_height, shade_height, top_radius, bot_radius, rack_t, cp: [[0.0, 0.0, z1], [0.0, 0.0, z2], [0.0, 0.0, z3]] };
        let mut l = StandLamp { p, seed: fs };
        // built to the layout box [width (y), depth (x), height]: round, the smaller of width and depth is the diameter. The radii
        // (base, shade) scale together; the stand reaches the height with the bends keeping their share of it, the shade (which hangs
        // 0.4 of its height below the stand's end) shrinks if the stand is too short for it.
        if let Some(h) = crate::api::size_hint() {
            let d = h[0].min(h[1]);
            for _ in 0..4 {
                let e = l.extents();
                let f = d / e[0].max(e[1]);
                l.p.base_radius *= f;
                l.p.top_radius *= f;
                l.p.bot_radius *= f;
                let z3 = l.p.cp[2][2];
                let z3n = (z3 + h[2] - e[2]).max(l.p.base_height + 0.05);
                let k = (z3n - l.p.base_height) / (z3 - l.p.base_height);
                for q in 0..2 {
                    l.p.cp[q][2] = l.p.base_height + (l.p.cp[q][2] - l.p.base_height) * k;
                }
                l.p.cp[2][2] = z3n;
                l.p.shade_height = l.p.shade_height.min((z3n - l.p.base_height - 0.02) / 0.4);
            }
        }
        l
    }

    /// overall [x extent, y extent, z extent] of the lamp parts
    fn extents(&self) -> [f64; 3] {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for m in lamp_parts(&self.p) {
            let (a, b) = m.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }
}

/// sample_curve at a length factor (CurveToMesh-free helper used by the rack): position on a polyline
fn sample_poly(pts: &[F3], cyclic: bool, factor: f32) -> F3 {
    let n = pts.len();
    let nseg = if cyclic { n } else { n - 1 };
    let mut lengths = Vec::with_capacity(nseg);
    let mut acc = 0.0f32;
    for i in 0..nseg {
        acc += gnlite::dist(pts[i], pts[(i + 1) % n]);
        lengths.push(acc);
    }
    let total = *lengths.last().unwrap();
    let target = factor * total;
    let idx = lengths.partition_point(|&l| l < target).min(nseg - 1);
    let prev = if idx == 0 { 0.0 } else { lengths[idx - 1] };
    let seg = lengths[idx] - prev;
    let t = (target - prev) / seg;
    let (a, b) = (pts[idx], pts[(idx + 1) % n]);
    [a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t, a[2] * (1.0 - t) + b[2] * t]
}

/// nodegroup_lamp_geometry -> parts in python's vertex order: shade (lampshade material), rack, stand, base
pub fn lamp_parts(p: &LampParams) -> [Mesh; 4] {
    use gnlite::*;
    let f = |x: f64| x as f32;
    let (sr, br, bh, sh, top, bot, rt) = (f(p.stand_radius), f(p.base_radius), f(p.base_height), f(p.shade_height), f(p.top_radius), f(p.bot_radius), f(p.rack_t));
    let cp = |k: usize| [f(p.cp[k][0]), f(p.cp[k][1]), f(p.cp[k][2])];
    // --- base
    let circ = |r: f32| curve_circle(r, 100);
    let base = curve_to_mesh(&[curve_line([0.0; 3], [0.0, 0.0, bh])], &circ(br), true);
    // --- stand: join [bezier, line] -> [line, bezier]
    let mut bez = bezier_points([0.0, 0.0, bh], cp(0), cp(1), cp(2), 100);
    *bez.last_mut().unwrap() = cp(2);
    let stand = curve_to_mesh(&[curve_line([0.0; 3], [0.0, 0.0, bh]), Spline::new(bez.clone(), false)], &circ(sr), true);
    // --- head
    let rev = 1.0f32;
    let m = sh * 0.4f32;
    let m1 = sh * 0.2f32;
    let rack_h = m * rev + m1;
    let t08 = top * 0.8f32;
    let inner_r = t08 * 0.15f32;
    let ma = rev * 2.0f32 + -1.0f32;
    let m2 = rack_h * ma;
    // reversiable_bulb is instantiated with Reverse unconnected (False): RackSupport = -0.015 * (0 * 2 - 1)
    let support = -0.015f32 * (0.0f32 * 2.0f32 + -1.0f32);
    // shade
    let sub_ = sh - rack_h;
    let m3 = ma * -1.0f32;
    let m4 = sub_ * m3;
    let mut shade_curve = curve_line([0.0, 0.0, m2], [0.0, 0.0, m4]);
    let rad = |fac: f32| top + ((fac - 0.0) / (1.0 - 0.0)) * (bot - top);
    shade_curve.radius = vec![rad(0.0), rad(1.0)];
    let shade0 = curve_to_mesh(&[shade_curve], &circ(1.0), false);
    let edges: Vec<(u32, u32)> = (0..2u32).flat_map(|r| (0..100u32).map(move |j| (r * 100 + j, r * 100 + (j + 1) % 100))).collect();
    let ext = extrude_faces(&shade0, 0.005, &edges);
    let flip = flip_faces(&shade0);
    let shade = join_rev(&[&flip, &ext]);
    // rack
    let lift = |mut s: Spline, z: f32| {
        for q in s.pts.iter_mut() {
            q[2] += z;
        }
        s
    };
    let outer = lift(circ(top), m2);
    let inner_rad = rt * 0.5f32 + inner_r;
    let inner = lift(circ(inner_rad), support);
    let divide = 1.0f32 / 3.0f32;
    let mut lines = vec![];
    for k in 0..3 {
        let fac = k as f32 * divide;
        let a = sample_poly(&inner.pts, true, fac);
        let b = sample_poly(&outer.pts, true, fac);
        lines.push(Spline::new(vec![a, b], false));
    }
    let mut rack_splines = vec![inner];
    rack_splines.extend(lines);
    rack_splines.push(outer);
    let rack = curve_to_mesh(&rack_splines, &circ(rt), true);
    // head transform: translation = end of the stand curve
    let t = cp(2);
    let mv = |mut m: Mesh| {
        for q in m.pos.iter_mut() {
            for k in 0..3 {
                q[k] = (q[k] as f32 + t[k]) as f64;
            }
        }
        m
    };
    [mv(shade), mv(rack), stand, base]
}

impl Factory for StandLamp {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        let e = self.extents();
        Some([e[1], e[0], e[2]])
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let shade_m = spec_material("lampshade", json!({"cls": weighted(&mut r, LAMPSHADE), "kwargs": {}, "seed": self.seed}), [0.9, 0.85, 0.7]);
        let metal = spec_material("lamp_metal", json!({"cls": weighted(&mut r, LEG_MATERIAL), "kwargs": {}, "seed": self.seed + 1}), [0.3, 0.3, 0.3]);
        let [shade, rack, stand, base] = lamp_parts(&self.p);
        let mut a = Asset::default();
        for (m, mt) in [(shade, &shade_m), (rack, &metal), (stand, &metal), (base, &metal)] {
            let b = m.bounds();
            a.mesh(m, mt, Some(b));
        }
        a
    }
}
