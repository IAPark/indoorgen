//! mollusk/shell.py (ScallopFactory / ClamFactory / MusselFactory and the shell branch of MolluskFactory):
//! 1024-gon circle -> fill_grid -> dome displacement -> angular profile (scipy quadratic spline) -> groove / hinge -> mirrored lower valve, then
//! decorate_mollusk (SUBSURF to the face size, SOLIDIFY 0.005, DISPLACE with a legacy texture).
use super::bpymod::{displace as _unused_displace_f32, DispDir};
use super::legtex::{displace, Tex, TexKind};
use super::*;
use crate::mesh::Mesh;
use crate::rng::Rng;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    matches!(c, "ScallopFactory" | "ClamFactory" | "MusselFactory" | "MolluskFactory")
}

/// scipy.interpolate.interp1d(x, y, kind='quadratic', bounds_error=False, fill_value=0): degree-2 interpolating B-spline with knots at the data-site midpoints
pub struct QuadSpline {
    t: Vec<f64>,
    c: Vec<f64>,
    x0: f64,
    xn: f64,
}

fn bspline_basis(t: &[f64], k: usize, i: usize, x: f64, last: bool) -> f64 {
    // Cox-de Boor, right-continuous except at the last knot interval end
    if k == 0 {
        let inside = (x >= t[i] && x < t[i + 1]) || (last && x == t[i + 1] && t[i] < t[i + 1] && x == *t.last().unwrap());
        return if inside { 1.0 } else { 0.0 };
    }
    let mut a = 0.0;
    let d1 = t[i + k] - t[i];
    if d1 > 0.0 {
        a += (x - t[i]) / d1 * bspline_basis(t, k - 1, i, x, last);
    }
    let d2 = t[i + k + 1] - t[i + 1];
    if d2 > 0.0 {
        a += (t[i + k + 1] - x) / d2 * bspline_basis(t, k - 1, i + 1, x, last);
    }
    a
}

impl QuadSpline {
    pub fn new(x: &[f64], y: &[f64]) -> QuadSpline {
        let n = x.len();
        let k = 2usize;
        let mid: Vec<f64> = (0..n - 1).map(|i| (x[i] + x[i + 1]) / 2.0).collect();
        let mut t = vec![x[0]; k + 1];
        if n > 3 {
            t.extend_from_slice(&mid[1..n - 2]);
        }
        t.extend(vec![x[n - 1]; k + 1]);
        // collocation system A c = y
        let mut a = vec![vec![0.0; n + 1]; n];
        for (i, &xi) in x.iter().enumerate() {
            for j in 0..n {
                a[i][j] = bspline_basis(&t, k, j, xi, xi == x[n - 1]);
            }
            a[i][n] = y[i];
        }
        // gaussian elimination with partial pivoting
        for col in 0..n {
            let mut p = col;
            for r in col + 1..n {
                if a[r][col].abs() > a[p][col].abs() {
                    p = r;
                }
            }
            a.swap(col, p);
            let d = a[col][col];
            for cc in col..=n {
                a[col][cc] /= d;
            }
            for r in 0..n {
                if r != col {
                    let f = a[r][col];
                    for cc in col..=n {
                        a[r][cc] -= f * a[col][cc];
                    }
                }
            }
        }
        QuadSpline { t, c: (0..n).map(|i| a[i][n]).collect(), x0: x[0], xn: x[n - 1] }
    }
    pub fn eval(&self, x: f64) -> f64 {
        if x < self.x0 || x > self.xn || x.is_nan() {
            return 0.0;
        }
        (0..self.c.len()).map(|j| self.c[j] * bspline_basis(&self.t, 2, j, x, x == self.xn)).sum()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ShellKind {
    Scallop,
    Clam,
    Mussel,
}

fn r32(x: f64) -> f64 {
    x as f32 as f64
}

fn build_ellipse(softness: f64) -> Mesh {
    // new_circle(vertices=1024) at x = 1: vertices cos/sin accumulated in float32
    let n = 1024usize;
    let phid = (2.0f32 * std::f32::consts::PI) / n as f32;
    let mut phi = 0.0f32;
    let mut lp: Vec<[f64; 3]> = vec![];
    for _ in 0..n {
        lp.push([(phi.cos() * 1.0 + 1.0f32) as f64, (phi.sin() * 1.0) as f64, 0.0]);
        phi += phid;
    }
    let mut m = super::bpyops::fill_grid_loop(&lp);
    // displace: viewpoint (0, 0, 1)
    let vp = [0.0, 0.0, 1.0];
    let co: Vec<[f64; 3]> = m.pos.clone();
    for (p, c) in m.pos.iter_mut().zip(&co) {
        let (x, y, z) = (c[0], c[1], c[2]);
        let r = ((x - 1.0).powi(2) + y * y + z * z).sqrt();
        let t = 1.0 - softness + softness * r.powi(4);
        for k in 0..3 {
            p[k] = r32(c[k] + (1.0 - t) * (vp[k] - c[k]));
        }
    }
    m
}

fn scale_apply(m: &mut Mesh, s: [f32; 3]) {
    for p in m.pos.iter_mut() {
        for k in 0..3 {
            p[k] = ((p[k] as f32) * s[k]) as f64;
        }
    }
}

fn shape_by_angles(m: &mut Mesh, angles: &[f64], scales: &[f64]) {
    let f = QuadSpline::new(angles, scales);
    for p in m.pos.iter_mut() {
        let a = p[1].atan2(p[0]);
        let s = f.eval(a);
        for k in 0..3 {
            p[k] = r32(s * p[k]);
        }
    }
}

fn add_radial_groove(m: &mut Mesh) {
    let freq = 45.0;
    let scale = 0.02;
    let vals: Vec<f64> = m
        .pos
        .iter()
        .map(|p| {
            let (x, y, z) = (p[0], p[1], p[2]);
            let a = (y / (x + 1e-6 * if x >= 0.0 { 1.0 } else { 0.0 })).atan();
            let r = (x * x + y * y + z * z).sqrt();
            scale * (a * freq).cos() * (r - 0.25).max(0.0)
        })
        .collect();
    // displace_vertices(obj, fn) with a 1-D result: `co[:, i] += fn[i]` adds the first three per-vertex values to whole coordinate columns
    for p in m.pos.iter_mut() {
        for k in 0..3 {
            p[k] = r32(p[k] + vals[k]);
        }
    }
}

fn add_hinge(obj: Mesh, r: &mut Rng) -> Mesh {
    let (length, width) = (0.4, 0.1);
    let x = r.uniform(0.8, 1.0);
    let verts = [[0.0, -length, 0.0], [width, -length * x, 0.0], [width, length * x, 0.0], [0.0, length, 0.0]];
    let mut o = Mesh::new();
    o.pos = verts.iter().map(|v| [r32(v[0]), r32(v[1]), r32(v[2])]).collect();
    o.faces.push(vec![0, 1, 2, 3]);
    let mut o = super::subsurf::subsurf(&o, 2, true);
    displace(&mut o, &Tex::new(TexKind::Stucci(0), 0.25), 0.2, 0.5);
    let mut out = obj;
    out.append(&o);
    out
}

fn dims(m: &Mesh) -> [f32; 3] {
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &m.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f32);
            hi[k] = hi[k].max(p[k] as f32);
        }
    }
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

fn maker(kind: ShellKind, r: &mut Rng) -> Mesh {
    match kind {
        ShellKind::Scallop => {
            let mut obj = build_ellipse(0.3);
            scale_apply(&mut obj, [1.0, 1.2, 1.0]);
            let boundary = 0.42;
            let outer = r.uniform(0.28, 0.32);
            let inner = r.uniform(0.18, 0.22);
            let s = r.uniform(0.6, 0.7);
            let angles: Vec<f64> = [-boundary, -outer, -inner, inner, outer, boundary].iter().map(|a| a * PI).collect();
            shape_by_angles(&mut obj, &angles, &[0.0, s, 1.0, 1.0, s, 0.0]);
            add_radial_groove(&mut obj);
            add_hinge(obj, r)
        }
        ShellKind::Clam => {
            let mut obj = build_ellipse(0.5);
            scale_apply(&mut obj, [1.0, 1.2, 1.0]);
            let s = r.uniform(0.6, 0.7);
            let angles = [-r.uniform(0.4, 0.5), -r.uniform(0.3, 0.35), r.uniform(-0.25, 0.25), r.uniform(0.3, 0.35), r.uniform(0.4, 0.5)];
            let angles: Vec<f64> = angles.iter().map(|a| a * PI).collect();
            shape_by_angles(&mut obj, &angles, &[0.0, s, 1.0, s, 0.0]);
            obj
        }
        ShellKind::Mussel => {
            let mut obj = build_ellipse(0.5);
            scale_apply(&mut obj, [1.0, 3.0, 1.0]);
            let s = r.uniform(0.6, 0.8);
            let a1 = -r.uniform(0.1, 0.15);
            let a2 = r.uniform(0.0, 0.25);
            let angles: Vec<f64> = [-0.5, a1, a2, 0.5].iter().map(|a| a * PI).collect();
            let s3 = r.uniform(0.6, 0.8);
            shape_by_angles(&mut obj, &angles, &[0.0, s, 1.0, s3]);
            obj
        }
    }
}

/// ShellBaseFactory.create_asset + MolluskFactory.decorate_mollusk
pub fn shell_asset(kind: ShellKind, r: &mut Rng, face_size: f64) -> Mesh {
    let mut upper = maker(kind, r);
    let d = dims(&upper);
    let dim = ((d[0] as f64) * (d[1] as f64) + 0.01).sqrt();
    let sc = (1.0 / dim) as f32;
    scale_apply(&mut upper, [sc, sc, sc]);
    for p in upper.pos.iter_mut() {
        p[2] = (p[2] as f32 + 0.005f32) as f64;
    }
    let mut lower = upper.clone();
    for p in lower.pos.iter_mut() {
        p[2] = -p[2];
    }
    for f in lower.faces.iter_mut() {
        f[1..].reverse();
    }
    let base = r.uniform(0.0, PI / 4.0);
    let delta = r.uniform(PI / 6.0, PI / 3.0);
    // join: the lower valve keeps its mesh coordinates, the upper one is rotated by -delta about y relative to it
    let ang = (-delta) as f32;
    let _ = base;
    let (c, s) = (ang.cos(), ang.sin());
    for p in upper.pos.iter_mut() {
        let (x, z) = (p[0] as f32, p[2] as f32);
        p[0] = (x * c + z * s) as f64;
        p[2] = (-x * s + z * c) as f64;
    }
    let mut obj = lower;
    obj.append(&upper);
    // decorate_mollusk
    let areas: f64 = obj.faces.iter().map(|f| face_area(&obj, f)).sum::<f64>() / obj.faces.len() as f64;
    if areas >= 1e-6 {
        let levels = (areas / face_size).log2().ceil() as i64;
        if levels > 0 {
            obj = super::subsurf::subsurf(&obj, levels as usize, false);
        }
    }
    obj = books::solidify_simple(&obj, 0.005, None);
    let marble = r.choice_idx(2) == 1;
    let scale = r.log_uniform(0.1, 0.2);
    let kind = if marble { TexKind::Marble(0, 0) } else { TexKind::Stucci(0) };
    displace(&mut obj, &Tex::new(kind, scale), 0.02, 0.0);
    obj
}

fn face_area(m: &Mesh, f: &[u32]) -> f64 {
    let n = f.len();
    let mut a = [0.0f64; 3];
    for i in 0..n {
        let (p, q) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % n] as usize]);
        a[0] += p[1] * q[2] - p[2] * q[1];
        a[1] += p[2] * q[0] - p[0] * q[2];
        a[2] += p[0] * q[1] - p[1] * q[0];
    }
    0.5 * (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

pub struct Shell {
    kind: ShellKind,
    fs: u64,
}

impl Factory for Shell {
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fs, inst);
        let m = shell_asset(self.kind, &mut r, 0.003);
        let b = m.bounds();
        let mut a = Asset::default();
        a.mesh(m, &mat("mollusk", [0.8, 0.7, 0.6], 0.5), Some(b));
        a
    }
}

pub fn make(c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    let kind = match c {
        "ScallopFactory" => ShellKind::Scallop,
        "ClamFactory" => ShellKind::Clam,
        "MusselFactory" => ShellKind::Mussel,
        _ => return None,
    };
    let _ = DispDir::X;
    Some(Box::new(Shell { kind, fs }))
}
