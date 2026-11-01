//! Weather (outdoor stage): a seeded state -- clear, partly cloudy, overcast or rain -- with
//!   clouds   a layer at ~1.4 km: a large disc draped toward the horizon carrying a procedural cloud texture (domain-warped fBm,
//!            alpha = density above the state's coverage threshold, greyer where dense), alpha-blended
//!   rain     streak cards (thin, long, translucent) in a box around the house and dark glossy puddles on the road / driveway
//! and the sidecar `weather` record the renderers apply: sun strength, sky ambient, haze, wetness (glossier, darker surfaces).
//! Stream: int_hash(seed, "weather").
use ig_core::mesh::{encode_png, Material, Mesh, Scene};
use ig_core::rng::Rng;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State { Clear, PartlyCloudy, Overcast, Rain }

pub struct Weather { pub state: State, pub cover: f64, pub sun: f64, pub ambient: f64, pub haze: f64, pub wet: f64 }

impl Weather {
    pub fn draw(r: &mut Rng, forced: Option<&str>) -> Weather {
        let state = match forced {
            Some("clear") => State::Clear, Some("partly") | Some("partly_cloudy") => State::PartlyCloudy, Some("overcast") => State::Overcast, Some("rain") => State::Rain,
            _ => { let u = r.f64(); if u < 0.35 { State::Clear } else if u < 0.65 { State::PartlyCloudy } else if u < 0.85 { State::Overcast } else { State::Rain } }
        };
        let (cover, sun, ambient, haze, wet) = match state {
            State::Clear => (r.uniform(0.0, 0.12), 1.0, 1.0, 1.0, 0.0),
            State::PartlyCloudy => (r.uniform(0.3, 0.6), r.uniform(0.8, 1.0), 1.1, 1.2, 0.0),
            State::Overcast => (r.uniform(0.88, 1.0), r.uniform(0.15, 0.3), 1.6, 2.0, r.uniform(0.0, 0.3)),
            State::Rain => (1.0, r.uniform(0.05, 0.15), 1.4, 3.5, 1.0),
        };
        Weather { state, cover, sun, ambient, haze, wet }
    }
    pub fn record(&self) -> Value {
        json!({"state": format!("{:?}", self.state), "cloud_cover": self.cover, "sun_strength": self.sun, "sky_ambient": self.ambient, "haze": self.haze, "wetness": self.wet})
    }
}

fn hash2(x: i64, y: i64, s: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ s.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31; h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9); h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 53) as f64
}
/// tileable value noise on an n x n lattice
fn vnoise(x: f64, y: f64, n: i64, s: u64) -> f64 {
    let (xi, yi) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - xi as f64, y - yi as f64);
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let g = |a: i64, b: i64| hash2(a.rem_euclid(n), b.rem_euclid(n), s);
    let (a, b, c, d) = (g(xi, yi), g(xi + 1, yi), g(xi, yi + 1), g(xi + 1, yi + 1));
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}
fn fbm(x: f64, y: f64, base: i64, s: u64) -> f64 {
    let (mut f, mut a, mut sum, mut norm) = (1.0, 0.5, 0.0, 0.0);
    for o in 0..6 { sum += a * vnoise(x * f, y * f, base * f as i64, s + o); norm += a; f *= 2.0; a *= 0.5; }
    sum / norm
}

/// the cloud texture: n x n RGBA (alpha = density above the coverage threshold), domain-warped fBm, tileable
fn cloud_texture(n: usize, cover: f64, overcast: f64, seed: u64) -> Arc<Vec<u8>> {
    let mut img = vec![0u8; n * n * 4];
    let base = 6i64;
    let srgb = |x: f64| -> u8 { (if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }.clamp(0.0, 1.0) * 255.0) as u8 };
    let th = 1.0 - (0.25 + 0.55 * cover);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f64 / n as f64 * base as f64, y as f64 / n as f64 * base as f64);
            let (wx, wy) = (fbm(u + 3.1, v, base, seed + 11), fbm(u, v + 7.7, base, seed + 23));
            let d = fbm(u + 1.6 * wx, v + 1.6 * wy, base, seed);
            let a = ((d - th) / 0.18).clamp(0.0, 1.0);
            let a = if cover > 0.85 { (0.75 + 0.25 * a).min(1.0) } else { a };
            // denser = greyer underside
            let g = 1.0 - (0.35 + 0.35 * overcast) * a;
            let i = (y * n + x) * 4;
            img[i] = srgb(0.92 * g); img[i + 1] = srgb(0.93 * g); img[i + 2] = srgb(0.96 * g); img[i + 3] = (a * 255.0) as u8;
        }
    }
    Arc::new(encode_png(n as u32, n as u32, &img))
}

/// Add the weather's geometry around `centre` (ground height `gz`); `paved(x, y)` gives the height of road / driveway at a point
/// (None off it) for the puddles.
pub fn add(scene: &mut Scene, w: &Weather, seed: u64, centre: [f64; 2], gz: f64, paved: &dyn Fn(f64, f64) -> Option<f64>) -> usize {
    let mut r = Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "weather_geom"]));
    let mut n = 0;
    if w.cover > 0.02 {
        // cloud layer: a disc of radius 9 km at 1.4 km, sinking to 300 m at its rim (reaches the horizon), uv tiled 3x
        let (rings, segs, rad, alt) = (24usize, 96usize, 9000.0, 1400.0);
        let mut m = Mesh::new();
        let pt = |k: usize, s: usize| -> ([f64; 3], [f64; 2]) {
            let rr = rad * (k as f64 / rings as f64).powf(1.5);
            let a = std::f64::consts::TAU * s as f64 / segs as f64;
            let (x, y) = (centre[0] + rr * a.cos(), centre[1] + rr * a.sin());
            let z = gz + alt - 1100.0 * (rr / rad).powi(2);
            ([x, y, z], [(x - centre[0]) / (2.0 * rad / 3.0), (y - centre[1]) / (2.0 * rad / 3.0)])
        };
        for k in 0..rings {
            for s in 0..segs {
                let q = [pt(k, s), pt(k, s + 1), pt(k + 1, s + 1), pt(k + 1, s)];
                let base = m.pos.len() as u32;
                if m.uv.len() != m.pos.len() { m.uv.resize(m.pos.len(), [0.0, 0.0]); }
                for (p, uv) in q { m.pos.push(p); m.uv.push(uv); }
                m.faces.push(vec![base, base + 1, base + 2, base + 3]);
            }
        }
        let mut mat = Material::diffuse(&format!("sky_clouds:{:?}", w.state), [1.0, 1.0, 1.0], 1.0);
        mat.texture = Some(cloud_texture(1024, w.cover, if matches!(w.state, State::Overcast | State::Rain) { 1.0 } else { 0.0 }, r.next_u64()));
        mat.alpha_cutoff = -1.0; // alpha blend
        mat.emissive = [0.0; 3];
        let mi = scene.material(mat);
        n += m.faces.len();
        scene.add("outdoor_sky_clouds#0", "OutdoorSky", m, mi);
    }
    if w.state == State::Rain {
        // rain streaks: 0.004 x 0.5 m cards, slightly slanted, in a 70 x 70 x 14 m box around the house
        let mut m = Mesh::new();
        let (sx, sy) = (r.uniform(-0.12, 0.12), r.uniform(-0.12, 0.12));
        for _ in 0..40000 {
            let (x, y, z) = (centre[0] + r.uniform(-35.0, 35.0), centre[1] + r.uniform(-35.0, 35.0), gz + r.uniform(0.0, 14.0));
            let l = r.uniform(0.35, 0.6);
            let a = r.uniform(0.0, std::f64::consts::PI);
            let (dx, dy) = (a.cos() * 0.002, a.sin() * 0.002);
            let top = [x + sx * l, y + sy * l, z + l];
            m.add_quad([x - dx, y - dy, z], [x + dx, y + dy, z], [top[0] + dx, top[1] + dy, top[2]], [top[0] - dx, top[1] - dy, top[2]]);
        }
        let mut mat = Material::diffuse("weather_rain", [0.75, 0.78, 0.82], 0.2);
        mat.color[3] = 0.35;
        let mi = scene.material(mat);
        n += m.faces.len();
        scene.add("outdoor_rain#0", "OutdoorSky", m, mi);
    }
    if w.wet > 0.5 {
        // puddles: irregular discs on the paving, mirror-dark
        let mut m = Mesh::new();
        let mut placed = 0;
        for _ in 0..4000 {
            if placed >= 60 { break }
            let (x, y) = (centre[0] + r.uniform(-60.0, 60.0), centre[1] + r.uniform(-60.0, 60.0));
            let Some(z) = paved(x, y) else { continue };
            let (rx, ry, rot) = (r.uniform(0.3, 1.4), r.uniform(0.2, 0.8), r.uniform(0.0, 3.14));
            let k = 14;
            let wob: Vec<f64> = (0..k).map(|_| r.uniform(0.75, 1.15)).collect();
            for j in 0..k {
                let (a0, a1) = (std::f64::consts::TAU * j as f64 / k as f64, std::f64::consts::TAU * (j + 1) as f64 / k as f64);
                let p = |a: f64, s: f64| { let (lx, ly) = (rx * s * a.cos(), ry * s * a.sin()); [x + lx * rot.cos() - ly * rot.sin(), y + lx * rot.sin() + ly * rot.cos(), z + 0.045] };
                m.add_tri([x, y, z + 0.045], p(a0, wob[j]), p(a1, wob[(j + 1) % k]));
            }
            placed += 1;
        }
        if !m.is_empty() {
            let mut mat = Material::diffuse("weather_puddle", [0.01, 0.012, 0.015], 0.02);
            mat.metallic = 0.0;
            let mi = scene.material(mat);
            n += m.faces.len();
            scene.add("outdoor_puddles#0", "OutdoorTerrain", m, mi);
        }
    }
    n
}
