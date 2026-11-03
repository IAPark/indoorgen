//! Small geometry helpers shared by the factory ports.
use crate::mesh::*;

pub fn norm3(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
pub fn unit3(v: V3) -> V3 {
    let n = norm3(v).max(1e-12);
    [v[0] / n, v[1] / n, v[2] / n]
}

/// prism between two points with a round (n sides) or square cross-section of half-size r0 -> r1
pub fn limb(a: V3, b: V3, r0: f64, r1: f64, round: bool) -> Mesh {
    let d = unit3(sub(b, a));
    let up = if d[2].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let u = unit3(cross(d, up));
    let v = cross(d, u);
    let n = if round { 24 } else { 4 }; // (24: at 12 plates, logs and handles showed as polygons)
    let ring = |c: V3, r: f64| -> Vec<V3> {
        (0..n)
            .map(|i| {
                let ang = 2.0 * std::f64::consts::PI * (i as f64 + if round { 0.0 } else { 0.5 }) / n as f64;
                let rr = if round { r } else { r * std::f64::consts::SQRT_2 };
                [c[0] + rr * (ang.cos() * u[0] + ang.sin() * v[0]), c[1] + rr * (ang.cos() * u[1] + ang.sin() * v[1]), c[2] + rr * (ang.cos() * u[2] + ang.sin() * v[2])]
            })
            .collect()
    };
    let (ra, rb) = (ring(a, r0), ring(b, r1));
    let mut m = Mesh::new();
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_quad(ra[i], ra[j], rb[j], rb[i]);
        m.add_tri(a, ra[j], ra[i]);
        m.add_tri(b, rb[i], rb[j]);
    }
    m
}

/// closed polygon (x,y) extruded in z (earcut caps), CCW or CW accepted
pub fn extrude(poly: &[[f64; 2]], z0: f64, z1: f64) -> Mesh {
    let flat: Vec<f64> = poly.iter().flat_map(|p| [p[0], p[1]]).collect();
    let tris = earcutr::earcut(&flat, &[], 2).unwrap_or_default();
    let mut m = Mesh::new();
    let area: f64 = (0..poly.len()).map(|i| {
        let j = (i + 1) % poly.len();
        poly[i][0] * poly[j][1] - poly[j][0] * poly[i][1]
    }).sum();
    let ccw = area > 0.0;
    for t in tris.chunks(3) {
        let (a, b, c) = (poly[t[0]], poly[t[1]], poly[t[2]]);
        if ccw {
            m.add_tri([a[0], a[1], z1], [b[0], b[1], z1], [c[0], c[1], z1]);
            m.add_tri([a[0], a[1], z0], [c[0], c[1], z0], [b[0], b[1], z0]);
        } else {
            m.add_tri([a[0], a[1], z1], [c[0], c[1], z1], [b[0], b[1], z1]);
            m.add_tri([a[0], a[1], z0], [b[0], b[1], z0], [c[0], c[1], z0]);
        }
    }
    for i in 0..poly.len() {
        let j = (i + 1) % poly.len();
        let (a, b) = (poly[i], poly[j]);
        if ccw {
            m.add_quad([a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], z1], [a[0], a[1], z1]);
        } else {
            m.add_quad([a[0], a[1], z0], [a[0], a[1], z1], [b[0], b[1], z1], [b[0], b[1], z0]);
        }
    }
    m
}

/// Chaikin corner cutting on a closed polygon
pub fn chaikin(p: &[[f64; 2]], iters: usize) -> Vec<[f64; 2]> {
    let mut cur = p.to_vec();
    for _ in 0..iters {
        let n = cur.len();
        let mut out = Vec::with_capacity(2 * n);
        for i in 0..n {
            let (a, b) = (cur[i], cur[(i + 1) % n]);
            out.push([0.75 * a[0] + 0.25 * b[0], 0.75 * a[1] + 0.25 * b[1]]);
            out.push([0.25 * a[0] + 0.75 * b[0], 0.25 * a[1] + 0.75 * b[1]]);
        }
        cur = out;
    }
    cur
}

pub fn bbox_of(parts: &[(V3, V3)]) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (a, b) in parts {
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    (lo, hi)
}

pub fn aabb(m: &Mesh) -> (V3, V3) {
    m.bounds()
}

/// open-polyline Chaikin smoothing that keeps the end points
pub fn chaikin_open(p: &[(f64, f64)], iters: usize) -> Vec<(f64, f64)> {
    let mut cur = p.to_vec();
    for _ in 0..iters {
        if cur.len() < 3 {
            break;
        }
        let mut out = vec![cur[0]];
        for i in 0..cur.len() - 1 {
            let (a, b) = (cur[i], cur[i + 1]);
            if i > 0 {
                out.push((0.75 * a.0 + 0.25 * b.0, 0.75 * a.1 + 0.25 * b.1));
            }
            if i + 1 < cur.len() - 1 {
                out.push((0.25 * a.0 + 0.75 * b.0, 0.25 * a.1 + 0.75 * b.1));
            }
        }
        out.push(*cur.last().unwrap());
        cur = out;
    }
    cur
}

/// infinigen.core.util.math.clip_gaussian: retry np.random.normal until inside [lo, hi] (max 20 tries then clip)
pub fn clip_gaussian(r: &mut crate::rng::Rng, mean: f64, std: f64, lo: f64, hi: f64) -> f64 {
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

/// infinigen.assets.utils.misc.generate_text: consumes clip_gaussian(3,7,2,15) then that many weighted choices
pub fn skip_text(r: &mut crate::rng::Rng) {
    let n = clip_gaussian(r, 3.0, 7.0, 2.0, 15.0) as i64;
    for _ in 0..n.max(0) {
        r.f64();
    }
}

// ---- Blender-compatible bezier profile + spin (infinigen.assets.utils.draw.bezier_curve / spin) ----

/// Handles of an open Blender bezier spline through `pts` (x,z): AUTO handles use BKE_nurb_handle_calc, VECTOR handles point at the neighbours.
/// Returns (left, right) handle per point.
fn bezier_handles(pts: &[(f64, f64)], vector: &[usize]) -> Vec<((f64, f64), (f64, f64))> {
    let n = pts.len();
    let sub = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0, a.1 - b.1);
    let len = |a: (f64, f64)| (a.0 * a.0 + a.1 * a.1).sqrt();
    (0..n)
        .map(|i| {
            let p2 = pts[i];
            let p1 = if i > 0 { pts[i - 1] } else { (2.0 * p2.0 - pts[1].0, 2.0 * p2.1 - pts[1].1) };
            let p3 = if i + 1 < n { pts[i + 1] } else { (2.0 * p2.0 - pts[n - 2].0, 2.0 * p2.1 - pts[n - 2].1) };
            if vector.contains(&i) {
                let (d1, d3) = (sub(p1, p2), sub(p3, p2));
                return (((p2.0 + d1.0 / 3.0), (p2.1 + d1.1 / 3.0)), ((p2.0 + d3.0 / 3.0), (p2.1 + d3.1 / 3.0)));
            }
            let (da, db) = (sub(p2, p1), sub(p3, p2));
            let (mut la, mut lb) = (len(da), len(db));
            if la == 0.0 {
                la = 1.0;
            }
            if lb == 0.0 {
                lb = 1.0;
            }
            let t = (db.0 / lb + da.0 / la, db.1 / lb + da.1 / la);
            let l = len(t) * 2.5614;
            if l == 0.0 {
                return (p2, p2);
            }
            let (la, lb) = (la / l, lb / l);
            ((p2.0 - t.0 * la, p2.1 - t.1 * la), (p2.0 + t.0 * lb, p2.1 + t.1 * lb))
        })
        .collect()
}

/// Sample the spline with `res` points per segment (Blender resolution_u), end points included
pub fn bezier_profile(pts: &[(f64, f64)], vector: &[usize], res: usize) -> Vec<(f64, f64)> {
    let h = bezier_handles(pts, vector);
    let mut out = vec![pts[0]];
    for i in 0..pts.len() - 1 {
        let (p0, p1, p2, p3) = (pts[i], h[i].1, h[i + 1].0, pts[i + 1]);
        for k in 1..=res {
            let t = k as f64 / res as f64;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            out.push((w[0] * p0.0 + w[1] * p1.0 + w[2] * p2.0 + w[3] * p3.0, w[0] * p0.1 + w[1] * p1.1 + w[2] * p2.1 + w[3] * p3.1));
        }
    }
    out
}

/// utils.draw.spin: bezier profile (x = radius, z = height) revolved about z with Blender's rotation resolution, welding the axis
pub fn spin(pts: &[(f64, f64)], vector: &[usize], res: usize) -> Mesh {
    let prof = bezier_profile(pts, vector, res);
    // weld consecutive profile points closer than 1e-3
    let mut p: Vec<(f64, f64)> = vec![];
    for q in prof {
        if p.last().map_or(true, |l| ((l.0 - q.0).powi(2) + (l.1 - q.1).powi(2)).sqrt() > 1e-3) {
            p.push(q);
        }
    }
    let mean_r = p.iter().map(|q| q.0.abs()).sum::<f64>() / p.len() as f64;
    let steps = (((2.0 * std::f64::consts::PI * mean_r / 5e-3) as usize).min(128)).max(3);
    let mut m = Mesh::new();
    let at = |i: usize, k: usize| -> V3 {
        let a = 2.0 * std::f64::consts::PI * (k % steps) as f64 / steps as f64;
        [p[i].0 * a.cos(), p[i].0 * a.sin(), p[i].1]
    };
    for i in 0..p.len() - 1 {
        for k in 0..steps {
            let (a, b, c, d) = (at(i, k), at(i, k + 1), at(i + 1, k + 1), at(i + 1, k));
            let on_axis = |q: V3| q[0].abs() < 1e-3 && q[1].abs() < 1e-3;
            if on_axis(a) && on_axis(b) {
                m.add_tri(a, c, d);
            } else if on_axis(c) && on_axis(d) {
                m.add_tri(a, b, c);
            } else {
                m.add_quad(a, b, c, d);
            }
        }
    }
    m
}

/// blender SOLIDIFY on a closed/open surface: duplicate inward/outward (sign of `t`) along averaged vertex normals, flipped, plus a rim where the surface is open
pub fn solidify(m: &Mesh, t: f64) -> Mesh {
    use crate::subdiv::QMesh;
    let q = QMesh { pos: m.pos.clone(), faces: m.faces.clone() }.weld(1e-5);
    let n = q.pos.len();
    let mut nrm = vec![[0.0f64; 3]; n];
    for f in &q.faces {
        // Newell normal
        let mut nn = [0.0; 3];
        for i in 0..f.len() {
            let (a, b) = (q.pos[f[i] as usize], q.pos[f[(i + 1) % f.len()] as usize]);
            nn[0] += (a[1] - b[1]) * (a[2] + b[2]);
            nn[1] += (a[2] - b[2]) * (a[0] + b[0]);
            nn[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        for &v in f {
            for k in 0..3 {
                nrm[v as usize][k] += nn[k];
            }
        }
    }
    let inner: Vec<V3> = (0..n).map(|i| {
        let l = (nrm[i][0].powi(2) + nrm[i][1].powi(2) + nrm[i][2].powi(2)).sqrt().max(1e-12);
        [q.pos[i][0] + nrm[i][0] / l * t, q.pos[i][1] + nrm[i][1] / l * t, q.pos[i][2] + nrm[i][2] / l * t]
    }).collect();
    let mut out = Mesh::new();
    for f in &q.faces {
        let pts: Vec<V3> = f.iter().map(|&i| q.pos[i as usize]).collect();
        out.add_face(&pts);
        let ip: Vec<V3> = f.iter().rev().map(|&i| inner[i as usize]).collect();
        out.add_face(&ip);
    }
    // rim quads along open boundary edges
    let mut cnt: crate::dhash::HashMap<(u32, u32), i32> = crate::dhash::HashMap::default();
    for f in &q.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    for f in &q.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            if cnt[&(a.min(b), a.max(b))] == 1 {
                out.add_quad(q.pos[b as usize], q.pos[a as usize], inner[a as usize], inner[b as usize]);
            }
        }
    }
    out
}

/// assets/utils/object.new_bbox: unit cube with float32 object scale / location, applied as x * s + l in float32 -> (lo, hi) with float32-exact values
pub fn new_bbox_f32(x: f64, x_: f64, y: f64, y_: f64, z: f64, z_: f64) -> (V3, V3) {
    let ax = |a: f64, b: f64| -> (f64, f64) {
        let (l, s) = (((a + b) / 2.0) as f32, ((b - a) / 2.0) as f32);
        (((-1.0f32) * s + l) as f64, (1.0f32 * s + l) as f64)
    };
    let (x0, x1) = ax(x, x_);
    let (y0, y1) = ax(y, y_);
    let (z0, z1) = ax(z, z_);
    ([x0, y0, z0], [x1, y1, z1])
}
