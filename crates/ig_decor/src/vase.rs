//! table_decorations/vase.py VaseFactory: star-profile loft (Catmull-Rom columns) -> delete top -> SOLIDIFY(0.002) -> SUBSURF(2)
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "VaseFactory"
}

type F3 = [f32; 3];

pub struct Vase {
    inner: f64,
    points: usize,
    height: f64,
    diameter: f64,
    top_scale: f64,
    neck_mid: f64,
    neck_pos: f64,
    neck_scale: f64,
    shoulder_pos: f64,
    shoulder_t: f64,
    foot_scale: f64,
    foot_h: f64,
    fs: u64,
}

impl Vase {
    fn new(fs: u64) -> Vase {
        // dimensions come from the global (unseeded) stream before FixedSeed: [z, x factor]
        let (z, xf) = match super::unseeded() {
            Some(u) if u.len() >= 2 => (u[0], u[1]),
            _ => {
                let mut g = Rng::seeded(fs);
                (g.uniform(0.17, 0.5), g.uniform(0.3, 0.6))
            }
        };
        // vase.py uses this "diameter" as the profile radius: width 0.6-1.2 x height gave 0.56 m wide tabletop bowls-of-vases;
        // 0.6 x keeps real proportions (width 0.36-0.72 x height)
        let x = z * xf * 0.6;
        let mut r = Rng::seeded(fs);
        let neck_scale = r.uniform(0.2, 0.8);
        let u = r.uniform(0.8, 1.0);
        let inner = if r.randint(0, 2) == 0 { 1.0 } else { u };
        let points = r.randint(16, 33) as usize;
        let top_scale = neck_scale * r.uniform(0.8, 1.2);
        let neck_mid = r.uniform(0.7, 0.95);
        let neck_pos = 0.5 * neck_scale + 0.5 + r.uniform(-0.05, 0.05);
        let shoulder_pos = r.uniform(0.3, 0.7);
        let shoulder_t = r.uniform(0.1, 0.25);
        let foot_scale = r.uniform(0.4, 0.6);
        let foot_h = r.uniform(0.01, 0.1);
        Vase { inner, points, height: z, diameter: x, top_scale, neck_mid, neck_pos, neck_scale, shoulder_pos, shoulder_t, foot_scale, foot_h, fs }
    }
}

fn dist(a: F3, b: F3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Resample Curve (Count mode) on an evaluated polyline
fn resample(pts: &[F3], cyclic: bool, count: usize) -> Vec<F3> {
    let n = pts.len();
    let nseg = if cyclic { n } else { n - 1 };
    let mut lengths = Vec::with_capacity(nseg);
    let mut acc = 0.0f32;
    for i in 0..nseg {
        acc += dist(pts[i], pts[(i + 1) % n]);
        lengths.push(acc);
    }
    let total = acc;
    (0..count)
        .map(|k| {
            let f = if cyclic { k as f32 / count as f32 } else { k as f32 / (count - 1) as f32 };
            let target = f * total;
            let idx = lengths.partition_point(|&l| l < target).min(nseg - 1);
            let prev = if idx == 0 { 0.0 } else { lengths[idx - 1] };
            let seg = lengths[idx] - prev;
            let t = if seg > 0.0 { (target - prev) / seg } else { 0.0 };
            let (a, b) = (pts[idx], pts[(idx + 1) % n]);
            [a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t, a[2] * (1.0 - t) + b[2] * t]
        })
        .collect()
}

/// Catmull-Rom spline (open, ends mirrored) evaluated with `res` samples per segment
fn catmull_rom_eval(p: &[F3], res: usize) -> Vec<F3> {
    let n = p.len();
    let get = |i: isize| -> F3 {
        let dup = true;
        if i < 0 {
            if dup { p[0] } else { [2.0 * p[0][0] - p[1][0], 2.0 * p[0][1] - p[1][1], 2.0 * p[0][2] - p[1][2]] }
        } else if i as usize >= n {
            if dup { p[n - 1] } else { [2.0 * p[n - 1][0] - p[n - 2][0], 2.0 * p[n - 1][1] - p[n - 2][1], 2.0 * p[n - 1][2] - p[n - 2][2]] }
        } else {
            p[i as usize]
        }
    };
    let mut out = vec![];
    for i in 0..n - 1 {
        let (a, b, c, d) = (get(i as isize - 1), get(i as isize), get(i as isize + 1), get(i as isize + 2));
        for k in 0..res {
            let t = k as f32 / res as f32;
            let (t2, t3) = (t * t, t * t * t);
            let w = [-0.5 * t3 + t2 - 0.5 * t, 1.5 * t3 - 2.5 * t2 + 1.0, -1.5 * t3 + 2.0 * t2 + 0.5 * t, 0.5 * t3 - 0.5 * t2];
            let mut q = [0.0f32; 3];
            for j in 0..3 {
                q[j] = w[0] * a[j] + w[1] * b[j] + w[2] * c[j] + w[3] * d[j];
            }
            out.push(q);
        }
    }
    out.push(p[n - 1]);
    out
}

impl Factory for Vase {
    fn asset(&self, _i: u64) -> Asset {
        let f = |x: f64| x as f32;
        let (h, d) = (f(self.height), f(self.diameter));
        let np = f(self.neck_pos);
        let fh = f(self.foot_h);
        // star profile resampled to 64 points
        let n = self.points;
        let ri = f(self.inner);
        let mut star: Vec<F3> = vec![];
        for k in 0..2 * n {
            let th = (2.0 * std::f64::consts::PI / (2 * n) as f64 * k as f64) as f32;
            let rr = if k % 2 == 0 { 1.0 } else { ri };
            star.push([th.cos() * rr, th.sin() * rr, 0.0]);
        }
        let prof = resample(&star, true, 64);
        // 7 profile curves, top first (Join Geometry reverses the link order)
        let sub = (1.0f32 - np).clamp(0.0, 1.0);
        let mid_z = (sub * f(self.neck_mid) + np) * h;
        let mid_s = d * ((f(self.neck_scale) + f(self.top_scale)) / 2.0);
        let mr = fh + (f(self.shoulder_pos) - 0.0) / 1.0 * (np - fh);
        let m5 = (np - fh) * f(self.shoulder_t);
        let body_top = (mr + m5).min(np) * h;
        let body_bot = (mr - m5).max(fh) * h;
        let curves: [(f32, f32); 7] = [
            (h, f(self.top_scale) * d),
            (mid_z, mid_s),
            (h * np, d * f(self.neck_scale)),
            (body_top, d),
            (body_bot, d),
            (fh * h, d * f(self.foot_scale)),
            (0.0, d * f(self.foot_scale)),
        ];
        let rings = 64usize;
        // column u: Catmull-Rom through the 7 curves' u-th points, resampled to 64
        let mut cols: Vec<Vec<F3>> = vec![];
        for u in 0..64 {
            let ctrl: Vec<F3> = curves.iter().map(|&(z, s)| [prof[u][0] * s, prof[u][1] * s, z]).collect();
            let ev = catmull_rom_eval(&ctrl, 12);
            cols.push(resample(&ev, false, rings));
        }
        let skip_first = true;
        let mut m = Mesh::new();
        let used: Vec<usize> = if skip_first { (1..rings).collect() } else { (0..rings - 1).collect() };
        for &v in &used {
            for u in 0..64 {
                let p = cols[u][v];
                m.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
            }
        }
        let nr = used.len();
        for v in 0..nr - 1 {
            for u in 0..64usize {
                let u1 = (u + 1) % 64;
                m.faces.push(vec![(v * 64 + u) as u32, ((v + 1) * 64 + u) as u32, ((v + 1) * 64 + u1) as u32, (v * 64 + u1) as u32]);
            }
        }
        m.faces.push(((nr - 1) as u32 * 64..nr as u32 * 64).rev().collect());
        let sgn = 1.0;
        let s = books::solidify_simple(&m, sgn * 0.002f32 as f64, None);
        let s = subsurf(&s, 2, false);
        let mat = spec_material("vase", json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.fs}), [0.8, 0.75, 0.7]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Vase::new(fs)))
}
