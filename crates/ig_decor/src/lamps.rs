//! lamp/ceiling_lights.py: CeilingLightFactory (black housing + translucent bulb cover; the point light sits at z=-0.03)
use super::gnlite::{self, F3};
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "CeilingLightFactory" | "DeskLampFactory" | "FloorLampFactory" | "LampFactory")
}

pub struct CeilingLight {
    /// 0 the puck, then the forms of `forms`; a stream of its own, so the puck's draws stay as they were
    form: u8,
    form_seed: u64,
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
        CeilingLight { form: 0, form_seed: 0, radius, thickness, inner_radius, height, inner_height, curvature, seed: r.randint(0, 1_000_000) as u64 }
    }
}

/// which form a fitting takes: one main form per house (rules/styles.json params light_*), some fittings another
const FORMS: [(&str, f64); 8] = [("light_puck", 2.0), ("light_recessed", 2.0), ("light_drum", 1.5), ("light_dome", 1.5), ("light_globe", 1.0), ("light_sputnik", 0.6), ("light_track", 0.7), ("light_panel", 0.7)];
fn draw_form(fs: u64) -> (u8, u64) {
    let pick = |u: f64| -> u8 {
        let w: Vec<f64> = FORMS.iter().map(|(k, d)| crate::mesh::style_param(k, *d).max(0.0)).collect();
        let mut x = u * w.iter().sum::<f64>();
        for (i, v) in w.iter().enumerate() { if x < *v { return i as u8 } x -= v; }
        0
    };
    let h = crate::mesh::house_seed();
    let main = pick(Rng::seeded(crate::int_hash(&[&h.to_string(), "light_form"])).f64());
    let mut r = Rng::seeded(crate::int_hash(&[&fs.to_string(), "light_form"]));
    let form = if r.f64() < 0.7 { main } else { pick(r.f64()) };
    (form, r.randint(0, 1 << 40) as u64)
}

impl CeilingLight {
    /// (the shell, the diffuser the light passes through, metal): z = 0 the ceiling, down is negative
    fn form_meshes(&self) -> (Mesh, Mesh, Mesh, [f64; 3], [f64; 3]) {
        let mut r = Rng::seeded(self.form_seed);
        let (mut shell, mut glow, mut metal) = (Mesh::new(), Mesh::new(), Mesh::new());
        match self.form {
            1 => {
                // recessed can: a trim ring and a lens almost flush
                let a = r.uniform(0.05, 0.09);
                metal.append(&ring_flat(a, a + 0.014, -0.006, 32));
                metal.append(&open_tube(a + 0.014, -0.006, 0.0, 32));
                glow.append(&Mesh::cylinder(0.0, 0.0, -0.004, 0.0, a, 32));
            }
            2 => {
                // a drum of cloth, a diffuser across its bottom
                let (a, h) = (r.uniform(0.17, 0.3), r.uniform(0.09, 0.16));
                let mut w = open_tube(a, -h - 0.02, -0.02, 48);
                w.append(&open_tube(a - 0.002, -h - 0.02, -0.02, 48).flipped());
                shell.append(&w);
                glow.append(&Mesh::cylinder(0.0, 0.0, -h - 0.012, -h - 0.008, a - 0.004, 48));
                metal.append(&Mesh::cylinder(0.0, 0.0, -0.02, 0.0, a * 0.35, 24));
            }
            3 => {
                // an opal dome on a rim
                let (a, h) = (r.uniform(0.12, 0.22), r.uniform(0.07, 0.13));
                metal.append(&Mesh::cylinder(0.0, 0.0, -0.018, 0.0, a + 0.006, 40));
                let m = Mesh::ellipsoid([0.0, 0.0, -0.018], [a, a, h], 32, 12);
                let mut low = Mesh::new();
                for t in &m.triangles() {
                    let c = [m.pos[t[0] as usize][2], m.pos[t[1] as usize][2], m.pos[t[2] as usize][2]];
                    if (c[0] + c[1] + c[2]) / 3.0 < -0.018 { low.add_tri(m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]); }
                }
                glow.append(&low);
            }
            4 => {
                // a globe on a short rod
                let (a, rod) = (r.uniform(0.08, 0.14), r.uniform(0.12, 0.3));
                metal.append(&Mesh::cylinder(0.0, 0.0, -0.02, 0.0, 0.05, 20));
                metal.append(&Mesh::cylinder(0.0, 0.0, -rod, 0.0, 0.007, 10));
                glow.append(&Mesh::ellipsoid([0.0, 0.0, -rod - a * 0.9], [a, a, a], 20, 12));
            }
            5 => {
                // a sputnik: arms out from a hub, a small globe on each
                let (n, reach, drop) = (r.randint(5, 10) as usize, r.uniform(0.22, 0.42), r.uniform(0.25, 0.42));
                metal.append(&Mesh::cylinder(0.0, 0.0, -0.02, 0.0, 0.05, 20));
                metal.append(&Mesh::cylinder(0.0, 0.0, -drop, 0.0, 0.007, 10));
                metal.append(&Mesh::ellipsoid([0.0, 0.0, -drop], [0.03, 0.03, 0.03], 12, 8));
                for i in 0..n {
                    let a = i as f64 / n as f64 * std::f64::consts::TAU;
                    let (dx, dy) = (a.cos(), a.sin());
                    let up = if i % 2 == 0 { 0.08 } else { -0.05 };
                    let tip = [dx * reach, dy * reach, -drop + up];
                    let t = 0.005;
                    let (px, py) = (-dy * t, dx * t);
                    let hub = [0.0, 0.0, -drop];
                    metal.add_quad([hub[0] + px, hub[1] + py, hub[2] - t], [tip[0] + px, tip[1] + py, tip[2] - t], [tip[0] + px, tip[1] + py, tip[2] + t], [hub[0] + px, hub[1] + py, hub[2] + t]);
                    metal.add_quad([hub[0] - px, hub[1] - py, hub[2] + t], [tip[0] - px, tip[1] - py, tip[2] + t], [tip[0] - px, tip[1] - py, tip[2] - t], [hub[0] - px, hub[1] - py, hub[2] - t]);
                    glow.append(&Mesh::ellipsoid(tip, [0.03, 0.03, 0.03], 12, 8));
                }
            }
            6 => {
                // a track with spots along it
                let (len, n) = (r.uniform(0.7, 1.3), r.randint(3, 5) as usize);
                metal.append(&Mesh::cuboid([-len / 2.0, -0.018, -0.025], [len / 2.0, 0.018, 0.0]));
                for i in 0..n {
                    let x = -len / 2.0 + len * (i as f64 + 0.5) / n as f64;
                    metal.append(&Mesh::cylinder(x, 0.0, -0.05, -0.025, 0.008, 10));
                    let rs = 0.035;
                    let mut w = open_tube(rs, -0.14, -0.05, 20);
                    w.append(&Mesh::cylinder(0.0, 0.0, -0.052, -0.05, rs, 20));
                    let tilt = r.uniform(-0.25, 0.25);
                    w.transform(&crate::mesh::Mat4::translation([0.0, 0.0, 0.095]));
                    w.transform(&crate::mesh::Mat4::rot_x(tilt));
                    w.transform(&crate::mesh::Mat4::translation([x, 0.0, -0.095]));
                    metal.append(&w);
                    let mut lens = Mesh::cylinder(0.0, 0.0, -0.138, -0.134, rs - 0.002, 20);
                    lens.transform(&crate::mesh::Mat4::translation([0.0, 0.0, 0.095]));
                    lens.transform(&crate::mesh::Mat4::rot_x(tilt));
                    lens.transform(&crate::mesh::Mat4::translation([x, 0.0, -0.095]));
                    glow.append(&lens);
                }
            }
            _ => {
                // a flat panel, square or long
                let (a, b) = (r.uniform(0.2, 0.3), if r.bool(0.5) { r.uniform(0.2, 0.3) } else { r.uniform(0.5, 0.6) });
                metal.append(&Mesh::cuboid([-b - 0.008, -a - 0.008, -0.03], [b + 0.008, a + 0.008, 0.0]));
                glow.append(&Mesh::cuboid([-b, -a, -0.034], [b, a, -0.03]));
            }
        }
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for m in [&shell, &glow, &metal] { for p in &m.pos { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } } }
        (shell, glow, metal, lo, hi)
    }
}

impl Factory for CeilingLight {
    /// collision: the housing (radius + thickness, height down from the ceiling) and the bulb cover's cylinder and its cap
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        if self.form != 0 { let (_, _, _, lo, hi) = self.form_meshes(); let m = 0.002; return Some(vec![Volume::hard([lo[0] - m, lo[1] - m, lo[2] - m], [hi[0] + m, hi[1] + m, m])]) }
        let (r, ri, m) = (self.radius + self.thickness + 0.002, self.inner_radius + 0.002, 0.002);
        Some(vec![
            Volume::hard([-r, -r, -self.height - m], [r, r, m]),
            Volume::hard([-ri, -ri, -self.inner_height - self.inner_radius * self.curvature - m], [ri, ri, m]),
        ])
    }
    fn asset(&self, _i: u64) -> Asset {
        if self.form != 0 {
            let (shell, glow, metal, _, _) = self.form_meshes();
            let mut a = Asset::default();
            let mut r = Rng::seeded(self.form_seed ^ 0x51);
            let (mc, mrgb) = [("metal.BrushedBlackMetal", [0.03, 0.03, 0.03]), ("metal.MetalBasic", [0.72, 0.56, 0.3]), ("metal.BrushedMetal", [0.7, 0.7, 0.72]), ("lamp_shaders.shader_black", [0.85, 0.85, 0.83])][r.choice_idx(4)];
            let m = spec_material("ceiling_metal", json!({"cls": mc, "kwargs": {}, "seed": self.form_seed}), [mrgb[0] as f32, mrgb[1] as f32, mrgb[2] as f32]);
            let g = spec_material("ceiling_bulb", json!({"cls": "lamp_shaders.shader_lamp_bulb_nonemissive", "kwargs": {}, "seed": self.form_seed}), [0.95, 0.93, 0.88]);
            let cloth = spec_material("lampshade", json!({"cls": "lamp_shaders.LampShade", "kwargs": {}, "seed": self.form_seed}), [0.8, 0.77, 0.7]);
            if !shell.pos.is_empty() { a.mesh(shell, &cloth, None); }
            if !metal.pos.is_empty() { a.mesh(metal, &m, None); }
            if !glow.pos.is_empty() { a.mesh(glow, &g, None); }
            return a;
        }
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
        "CeilingLightFactory" => {
            let mut l = CeilingLight::new(rng);
            (l.form, l.form_seed) = draw_form(fs);
            Box::new(l)
        }
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

    /// the lamp is round about z (the stand's control points stand on the axis): the shade hangs from 0.4 of its height below the
    /// stand's end to 0.6 above (its walls extruded 5 mm out), the rack's outer ring (tube rack_t) at its top
    fn head(&self) -> (f64, f64, f64) {
        let p = &self.p;
        let z3 = p.cp[2][2];
        (p.top_radius.max(p.bot_radius) + 0.005 + p.rack_t, z3 - 0.4 * p.shade_height - 0.001, z3 + 0.6 * p.shade_height + p.rack_t)
    }

    /// overall [x extent, y extent, z extent]
    fn extents(&self) -> [f64; 3] {
        let (r, _, top) = self.head();
        let d = 2.0 * r.max(self.p.base_radius).max(self.p.stand_radius);
        [d, d, top]
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
    /// collision: the base disc, the stand on the axis, the shade and its rack
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        let p = &self.p;
        let (r, z0, z1) = self.head();
        let (br, sr, m) = (p.base_radius + 0.002, p.stand_radius + 0.002, 0.002);
        Some(vec![
            Volume::hard([-br, -br, -m], [br, br, p.base_height + m]),
            Volume::hard([-sr, -sr, 0.0], [sr, sr, p.cp[2][2] + m]),
            Volume::hard([-r - m, -r - m, z0 - m], [r + m, r + m, z1 + m]),
        ])
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
