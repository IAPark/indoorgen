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

fn tr(kind: &str, m: &Mesh) {
    if std::env::var("MONOTRACE").is_ok() {
        let v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        super::stagefile::trace(kind, &v, &m.faces, &[]);
    }
}

fn r32(x: f64) -> f64 {
    x as f32 as f64
}

type F3 = [f32; 3];
fn sub3(a: F3, b: F3) -> F3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot3(a: F3, b: F3) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross_tri(v1: F3, v2: F3, v3: F3) -> F3 {
    let (n1, n2) = (sub3(v1, v2), sub3(v2, v3));
    [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]]
}
fn normalize3(a: F3) -> (F3, f32) {
    let d = dot3(a, a);
    if d > 1.0e-35f32 {
        let d = d.sqrt();
        let f = 1.0f32 / d;
        ([a[0] * f, a[1] * f, a[2] * f], d)
    } else {
        ([0.0; 3], 0.0)
    }
}
fn edges_to_normal(a1: F3, a2: F3, b1: F3, b2: F3) -> F3 {
    let da = normalize3(sub3(a2, a1)).0;
    let db = normalize3(sub3(b2, b1)).0;
    normalize3([da[0] + db[0], da[1] + db[1], da[2] + db[2]]).0
}
/// bmo_fill_grid.cc quad_verts_to_barycentric_tri
fn quad_to_tri(a: F3, b: F3, an: F3, bn: F3, prev: Option<(F3, F3)>, flip: bool) -> [F3; 3] {
    let mut no = edges_to_normal(a, an, b, bn);
    if let Some((ap, bp)) = prev {
        let nt = edges_to_normal(ap, a, bp, b);
        no = normalize3([no[0] + nt[0], no[1] + nt[1], no[2] + nt[2]]).0;
    }
    if flip {
        no = [-no[0], -no[1], -no[2]];
    }
    let d = sub3(a, b);
    let l = dot3(d, d).sqrt();
    let no = [no[0] * l, no[1] * l, no[2] * l];
    let mid = [(a[0] + b[0]) * 0.5f32, (a[1] + b[1]) * 0.5f32, (a[2] + b[2]) * 0.5f32];
    [a, b, [mid[0] + no[0], mid[1] + no[1], mid[2] + no[2]]]
}
/// BLI transform_point_by_tri_v3
fn transform_by_tri(pt: F3, tar: &[F3; 3], src: &[F3; 3]) -> F3 {
    let no_tar = normalize3(cross_tri(tar[0], tar[1], tar[2])).0;
    let no_src = normalize3(cross_tri(src[0], src[1], src[2])).0;
    // axis_dominant_v3_to_m3 (rows n1, n2, n after ortho_basis_v3v3_v3)
    let f = no_src[0] * no_src[0] + no_src[1] * no_src[1];
    let (n1, n2) = if f > f32::EPSILON {
        let d = 1.0f32 / f.sqrt();
        let r1 = [no_src[1] * d, -no_src[0] * d, 0.0f32];
        let r2 = [-no_src[2] * r1[1], no_src[2] * r1[0], no_src[0] * r1[1] - no_src[1] * r1[0]];
        (r1, r2)
    } else {
        ([if no_src[2] < 0.0 { -1.0 } else { 1.0 }, 0.0, 0.0], [0.0, 1.0, 0.0])
    };
    let mul = |a: F3| -> F3 {
        [n1[0] * a[0] + n1[1] * a[1] + n1[2] * a[2], n2[0] * a[0] + n2[1] * a[1] + n2[2] * a[2], no_src[0] * a[0] + no_src[1] * a[1] + no_src[2] * a[2]]
    };
    let p = mul(pt);
    let t: Vec<F3> = src.iter().map(|&v| mul(v)).collect();
    let cross2 = |v1: F3, v2: F3, v3: F3| (v1[0] - v2[0]) * (v2[1] - v3[1]) + (v1[1] - v2[1]) * (v3[0] - v2[0]);
    let mut w = [cross2(t[1], t[2], p), cross2(t[2], t[0], p), cross2(t[0], t[1], p)];
    let wtot = w[0] + w[1] + w[2];
    let ok = wtot != 0.0 && {
        let r = 1.0f32 / wtot;
        for x in w.iter_mut() {
            *x *= r;
        }
        w.iter().all(|x| x.is_finite())
    };
    if !ok {
        w = [1.0 / 3.0; 3];
    }
    let mut out = [
        tar[0][0] * w[0] + tar[1][0] * w[1] + tar[2][0] * w[2],
        tar[0][1] * w[0] + tar[1][1] * w[1] + tar[2][1] * w[2],
        tar[0][2] * w[0] + tar[1][2] * w[1] + tar[2][2] * w[2],
    ];
    let cr = cross_tri(tar[0], tar[1], tar[2]);
    let area_tar = (dot3(cr, cr).sqrt() * 0.5f32).sqrt();
    let area_src = ((0.5f32 * ((t[0][0] - t[1][0]) * (t[1][1] - t[2][1]) + (t[0][1] - t[1][1]) * (t[2][0] - t[1][0]))).abs()).sqrt();
    let z = p[2] - t[0][2];
    let k = (z / area_src) * area_tar;
    for c in 0..3 {
        out[c] += no_tar[c] * k;
    }
    out
}

/// bpy.ops.mesh.fill_grid() (use_interp_simple=False, BARYCENTRIC_INTERP) on the 1024-vertex circle of ShellBaseFactory.build_ellipse.
/// Blender picks circle vertex 892 as the grid corner (f32 angle noise; the circle is the same for every shell). Grid x runs along
/// increasing circle index, y along decreasing index; boundary vertices keep their circle index, interior vertices follow row by row.
fn fill_grid_circle(lp: &[[f64; 3]]) -> Mesh {
    const BEST: usize = 892;
    let n = lp.len();
    let (xt, yt) = (n / 4 + 1, n / 4 + 1);
    let (xm, ym) = (xt - 1, yt - 1);
    let c = |i: usize| -> F3 { let p = lp[i % n]; [p[0] as f32, p[1] as f32, p[2] as f32] };
    let bidx = |px: usize, py: usize| -> usize {
        if py == 0 {
            (BEST + px) % n
        } else if px == 0 {
            (BEST + n - py) % n
        } else if py == ym {
            (BEST + 2 * n - n / 4 - px) % n
        } else {
            (BEST + n / 4 + py) % n
        }
    };
    let xy = |x: usize, y: usize| x + y * xt;
    let mut g: Vec<F3> = vec![[0.0; 3]; xt * yt];
    for py in 0..yt {
        for px in 0..xt {
            if px == 0 || py == 0 || px == xm || py == ym {
                g[xy(px, py)] = c(bidx(px, py));
            }
        }
    }
    let tri_a = quad_to_tri(g[xy(0, 0)], g[xy(xm, 0)], g[xy(0, 1)], g[xy(xm, 1)], None, false);
    let tri_b = quad_to_tri(g[xy(0, ym)], g[xy(xm, ym)], g[xy(0, ym - 1)], g[xy(xm, ym - 1)], None, true);
    for y in 1..ym {
        let tri_t = quad_to_tri(g[xy(0, y)], g[xy(xm, y)], g[xy(0, y + 1)], g[xy(xm, y + 1)], Some((g[xy(0, y - 1)], g[xy(xm, y - 1)])), false);
        for x in 1..xm {
            let ca = transform_by_tri(g[xy(x, 0)], &tri_t, &tri_a);
            let cb = transform_by_tri(g[xy(x, ym)], &tri_t, &tri_b);
            let t = y as f32 / (yt as f32 - 1.0f32);
            let s = 1.0f32 - t;
            g[xy(x, y)] = [s * ca[0] + t * cb[0], s * ca[1] + t * cb[1], s * ca[2] + t * cb[2]];
        }
    }
    let pidx = |px: usize, py: usize| -> usize {
        if px == 0 || py == 0 || px == xm || py == ym { bidx(px, py) } else { n + (px - 1) + (py - 1) * (xt - 2) }
    };
    let mut m = Mesh::new();
    m.pos = vec![[0.0; 3]; xt * yt];
    for py in 0..yt {
        for px in 0..xt {
            let q = g[xy(px, py)];
            m.pos[pidx(px, py)] = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
    }
    for px in 0..xt - 1 {
        for py in 0..yt - 1 {
            m.faces.push(vec![pidx(px + 1, py) as u32, pidx(px + 1, py + 1) as u32, pidx(px, py + 1) as u32, pidx(px, py) as u32]);
        }
    }
    m
}

fn build_ellipse(softness: f64) -> Mesh {
    // new_circle(vertices=1024) at x = 1: vertices cos/sin accumulated in float32
    let n = 1024usize;
    let mut lp: Vec<[f64; 3]> = vec![];
    for k in 0..n {
        let (sn, cs) = super::cabnodes::sin_cos_from_fraction(k, n);
        lp.push([((sn * -1.0f32) + 1.0f32) as f64, (cs * 1.0f32) as f64, 0.0]);
    }
    let mut m = fill_grid_circle(&lp);
    tr("geo_geo_shader_vector", &m);
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
    tr("A_ellipse", &m);
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
    if std::env::var("SHELLDBG").is_ok() {
        eprintln!("angles {:?} scales {:?}", angles, scales);
    }
    tr("B_before", m);
    let f = QuadSpline::new(angles, scales);
    for p in m.pos.iter_mut() {
        let a = p[1].atan2(p[0]);
        let s = f.eval(a);
        for k in 0..3 {
            p[k] = r32(s * p[k]);
        }
    }
    tr("C_after", m);
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
    tr("SUBSURF", &o);
    displace(&mut o, &Tex::new(TexKind::Stucci(0), 0.25), 0.2, 0.5);
    tr("DISPLACE", &o);
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

fn maker(kind: ShellKind, r: &mut Rng) -> (Mesh, Option<[f32; 3]>) {
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
            (add_hinge(obj, r), None)
        }
        ShellKind::Clam => {
            let mut obj = build_ellipse(0.5);
            scale_apply(&mut obj, [1.0, 1.2, 1.0]);
            let stale = dims(&obj);
            let s = r.uniform(0.6, 0.7);
            let angles = [-r.uniform(0.4, 0.5), -r.uniform(0.3, 0.35), r.uniform(-0.25, 0.25), r.uniform(0.3, 0.35), r.uniform(0.4, 0.5)];
            let angles: Vec<f64> = angles.iter().map(|a| a * PI).collect();
            shape_by_angles(&mut obj, &angles, &[0.0, s, 1.0, s, 0.0]);
            (obj, Some(stale))
        }
        ShellKind::Mussel => {
            let mut obj = build_ellipse(0.5);
            scale_apply(&mut obj, [1.0, 3.0, 1.0]);
            let stale = dims(&obj);
            let s = r.uniform(0.6, 0.8);
            let a1 = -r.uniform(0.1, 0.15);
            let a2 = r.uniform(0.0, 0.25);
            let angles: Vec<f64> = [-0.5, a1, a2, 0.5].iter().map(|a| a * PI).collect();
            let s3 = r.uniform(0.6, 0.8);
            shape_by_angles(&mut obj, &angles, &[0.0, s, 1.0, s3]);
            (obj, Some(stale))
        }
    }
}

/// ShellBaseFactory.create_asset + MolluskFactory.decorate_mollusk
pub fn shell_asset(kind: ShellKind, r: &mut Rng, face_size: f64) -> Mesh {
    let (mut upper, stale) = maker(kind, r);
    // obj.dimensions is read from the bounding box cached before shape_by_angles rewrote the coordinates (foreach_set does not refresh it)
    let d = stale.unwrap_or_else(|| dims(&upper));
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
    // python's apply_transform with scale z=-1 leaves the winding unchanged (lower valve normals face inward)
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
            tr("PRE", &obj);
            obj = super::subsurf::subsurf(&obj, levels as usize, false);
            tr("SUBSURF", &obj);
        }
    }
    if let Ok(f) = std::env::var("PRESOL") {
        let v: Vec<[f32; 3]> = obj.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        super::stagefile::write(&f, &v, &obj.faces, &[]);
    }
    obj = solidify_exact(&obj, 0.005);
    tr("SOLIDIFY", &obj);
    let marble = r.choice_idx(2) == 1;
    let scale = r.log_uniform(0.1, 0.2);
    let kind = if marble { TexKind::Marble(0, 0) } else { TexKind::Stucci(0) };
    displace(&mut obj, &Tex::new(kind, scale), 0.02, 0.0);
    tr("DISPLACE", &obj);
    obj
}

/// Blender SOLIDIFY (simple/extrude mode, offset -1): exact port solidx::solidify_extrude; edges numbered in first-seen face order
fn solidify_exact(m: &Mesh, th: f64) -> Mesh {
    let mut emap: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut ce = vec![];
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([k.0, k.1]);
                (edges.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let s = super::solidx::solidify_extrude(&pos, &edges, &m.faces, &ce, th as f32, -1.0, true);
    let mut o = Mesh::new();
    o.pos = s.pos.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    o.faces = s.faces;
    o
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
