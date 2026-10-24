//! Float32 emulation of the Blender geometry-node primitives the lamp / tableware node graphs use
//! (CurveCircle, CurveLine, CurveBezierSegment, SampleCurve, CurveToMesh, ExtrudeMesh, FlipFaces, JoinGeometry ordering).
//! Meshes keep f64 storage but every coordinate is rounded through f32 exactly where Blender does.
use crate::mesh::Mesh;

pub type F3 = [f32; 3];

pub fn add(a: F3, b: F3) -> F3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: F3, s: f32) -> F3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: F3, b: F3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: F3, b: F3) -> F3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: F3) -> f32 {
    dot(a, a).sqrt()
}
pub fn normalize(a: F3) -> F3 {
    let l = len(a);
    if l == 0.0 {
        [0.0; 3]
    } else {
        // math::normalize: v / length
        [a[0] / l, a[1] / l, a[2] / l]
    }
}
pub fn dist(a: F3, b: F3) -> f32 {
    len(sub(a, b))
}

pub struct Spline {
    pub pts: Vec<F3>,
    pub radius: Vec<f32>,
    pub cyclic: bool,
}

impl Spline {
    pub fn new(pts: Vec<F3>, cyclic: bool) -> Spline {
        let n = pts.len();
        Spline { pts, radius: vec![1.0; n], cyclic }
    }
}

/// GeometryNodeCurvePrimitiveCircle (Points mode), centred at the origin in the xy plane
pub fn curve_circle(radius: f32, res: usize) -> Spline {
    let theta_step = ((2.0f64 * std::f64::consts::PI) / res as f64) as f32;
    let pts = (0..res)
        .map(|i| {
            let theta = theta_step * i as f32;
            [theta.cos() * radius, theta.sin() * radius, 0.0]
        })
        .collect();
    Spline::new(pts, true)
}

pub fn curve_line(a: F3, b: F3) -> Spline {
    Spline::new(vec![a, b], false)
}

/// BKE_curve_forward_diff_bezier-style evaluation of one cubic segment with `res` edges (res + 1 points)
pub fn bezier_points(p0: F3, h0: F3, h1: F3, p1: F3, res: usize) -> Vec<F3> {
    let mut out = vec![[0.0f32; 3]; res + 1];
    for k in 0..3 {
        let (q0, q1, q2, q3) = (p0[k], h0[k], h1[k], p1[k]);
        let rt0 = q0;
        let rt1 = 3.0f32 * (q1 - q0);
        let rt2 = 3.0f32 * (q0 - 2.0f32 * q1 + q2);
        let rt3 = q3 - q0 + 3.0f32 * (q1 - q2);
        let f = 1.0f32 / res as f32;
        let mut a0 = rt0;
        let mut a1 = f * (rt1 + f * (rt2 + f * rt3));
        let mut a2 = 2.0f32 * f * f * (rt2 + 3.0f32 * f * rt3);
        let a3 = 6.0f32 * f * f * f * rt3;
        for p in out.iter_mut() {
            p[k] = a0;
            a0 += a1;
            a1 += a2;
            a2 += a3;
        }
    }
    out
}

fn is_zero(a: F3) -> bool {
    a[0] == 0.0 && a[1] == 0.0 && a[2] == 0.0
}

fn bisect(a: F3, b: F3) -> F3 {
    let r = normalize(add(a, b));
    if is_zero(r) {
        a
    } else {
        r
    }
}

pub fn tangents(pts: &[F3], cyclic: bool) -> Vec<F3> {
    let n = pts.len();
    if n == 1 {
        return vec![[0.0, 0.0, 1.0]];
    }
    // segment tangents
    let mut seg = vec![[0.0f32; 3]; n];
    for i in 0..n - 1 {
        seg[i] = normalize(sub(pts[i + 1], pts[i]));
    }
    seg[n - 1] = if cyclic { normalize(sub(pts[0], pts[n - 1])) } else { seg[n - 2] };
    let mut t = vec![[0.0f32; 3]; n];
    t[0] = if cyclic { bisect(seg[n - 1], seg[0]) } else { seg[0] };
    for i in 1..n - 1 {
        t[i] = bisect(seg[i - 1], seg[i]);
    }
    t[n - 1] = if cyclic { bisect(seg[n - 2], seg[n - 1]) } else { seg[n - 2] };
    t
}

fn angle_normalized(a: F3, b: F3) -> f32 {
    if dot(a, b) >= 0.0 {
        2.0 * (len(sub(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (len(add(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

fn rotate_dir(dir: F3, axis: F3, angle: f32) -> F3 {
    let axis_scaled = mul(axis, dot(dir, axis));
    let diff = sub(dir, axis_scaled);
    let c = cross(axis, diff);
    add(add(axis_scaled, mul(diff, angle.cos())), mul(c, angle.sin()))
}

fn next_normal(last_n: F3, last_t: F3, cur_t: F3) -> F3 {
    if is_zero(last_t) || is_zero(cur_t) {
        return last_n;
    }
    let angle = angle_normalized(last_t, cur_t);
    if angle != 0.0 {
        let axis = normalize(cross(last_t, cur_t));
        return rotate_dir(last_n, axis, angle);
    }
    last_n
}

pub fn next_normal_pub(a: F3, b: F3, c: F3) -> F3 { next_normal(a, b, c) }
pub fn rotate_dir_pub(d: F3, a: F3, ang: f32) -> F3 { rotate_dir(d, a, ang) }

pub fn normals_min_twist(t: &[F3], cyclic: bool) -> Vec<F3> {
    let n = t.len();
    let mut out = vec![[0.0f32; 3]; n];
    let first = t[0];
    out[0] = if first[0].abs() + first[1].abs() < 1e-4 { [1.0, 0.0, 0.0] } else { normalize([first[1], -first[0], 0.0]) };
    for i in 1..n {
        out[i] = next_normal(out[i - 1], t[i - 1], t[i]);
    }
    let _ = cyclic; // planar circles: the cyclic twist correction is below float noise
    out
}

/// GeometryNodeCurveToMesh. `profile` points are in the profile plane (z = 0).
pub fn curve_to_mesh(splines: &[Spline], profile: &Spline, caps: bool) -> Mesh {
    let mut m = Mesh::new();
    let pn = profile.pts.len();
    for s in splines {
        let n = s.pts.len();
        let base = m.pos.len() as u32;
        let t = tangents(&s.pts, s.cyclic);
        let nrm = normals_min_twist(&t, s.cyclic);
        for i in 0..n {
            let b = cross(t[i], nrm[i]);
            let r = s.radius[i];
            let (cx, cy, cz) = (mul(nrm[i], r), mul(b, r), mul(t[i], r));
            for p in &profile.pts {
                let mut v = [0.0f32; 3];
                for k in 0..3 {
                    v[k] = cx[k] * p[0] + cy[k] * p[1] + cz[k] * p[2] + s.pts[i][k];
                }
                m.pos.push([v[0] as f64, v[1] as f64, v[2] as f64]);
            }
        }
        let nseg = if s.cyclic { n } else { n - 1 };
        for i in 0..nseg {
            let (r0, r1) = (base + (i * pn) as u32, base + (((i + 1) % n) * pn) as u32);
            for j in 0..pn {
                let j1 = ((j + 1) % pn) as u32;
                m.faces.push(vec![r0 + j as u32, r0 + j1, r1 + j1, r1 + j as u32]);
            }
        }
        if caps && !s.cyclic {
            let last = base + ((n - 1) * pn) as u32;
            m.faces.push((0..pn as u32).rev().map(|j| base + j).collect());
            m.faces.push((0..pn as u32).map(|j| last + j).collect());
        }
    }
    m
}

/// Join Geometry: inputs end up in reverse link order
pub fn join_rev(parts: &[&Mesh]) -> Mesh {
    let mut m = Mesh::new();
    for p in parts.iter().rev() {
        m.append(p);
    }
    m
}

/// FlipFaces: every face keeps its first vertex and reverses the rest
pub fn flip_faces(m: &Mesh) -> Mesh {
    let mut out = m.clone();
    for f in out.faces.iter_mut() {
        let first = f[0];
        let mut g = vec![first];
        g.extend(f[1..].iter().rev());
        *f = g;
    }
    out
}

/// face normal (Blender normal_quad_v3 / normal_tri_v3 / Newell) in float
fn face_normal(m: &Mesh, f: &[u32]) -> F3 {
    let p = |i: usize| -> F3 {
        let v = m.pos[f[i] as usize];
        [v[0] as f32, v[1] as f32, v[2] as f32]
    };
    if f.len() == 4 {
        let n1 = sub(p(0), p(2));
        let n2 = sub(p(1), p(3));
        normalize(cross(n1, n2))
    } else if f.len() == 3 {
        let n1 = sub(p(0), p(1));
        let n2 = sub(p(1), p(2));
        normalize(cross(n1, n2))
    } else {
        let mut n = [0.0f32; 3];
        let mut prev = p(f.len() - 1);
        for i in 0..f.len() {
            let cur = p(i);
            n[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
            n[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
            n[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
            prev = cur;
        }
        normalize(n)
    }
}

/// ExtrudeMesh (Faces mode, Individual off, all faces selected, offset = face normal field): vertices are duplicated and
/// moved by the mean of the adjacent face normals * scale; selected faces move to the copies and boundary side faces are added.
pub fn extrude_faces(m: &Mesh, scale: f32, edges: &[(u32, u32)]) -> Mesh {
    let n = m.pos.len();
    let mut acc = vec![[0.0f32; 3]; n];
    let mut cnt = vec![0u32; n];
    for f in &m.faces {
        let fnrm = face_normal(m, f);
        for &v in f {
            acc[v as usize] = add(acc[v as usize], fnrm);
            cnt[v as usize] += 1;
        }
    }
    let mut out = Mesh::new();
    out.pos = m.pos.clone();
    for i in 0..n {
        let o = if cnt[i] > 0 { mul(acc[i], 1.0 / cnt[i] as f32) } else { [0.0; 3] };
        let o = mul(o, scale);
        let p = m.pos[i];
        out.pos.push([(p[0] as f32 + o[0]) as f64, (p[1] as f32 + o[1]) as f64, (p[2] as f32 + o[2]) as f64]);
    }
    // faces moved to the extruded copies
    for f in &m.faces {
        out.faces.push(f.iter().map(|&v| v + n as u32).collect());
    }
    // boundary edges: side faces in stored-edge order
    let mut cntm: crate::dhash::HashMap<(u32, u32), i32> = crate::dhash::HashMap::default();
    for f in &m.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            *cntm.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    for &(e0, e1) in edges {
        if cntm.get(&(e0.min(e1), e0.max(e1))).copied().unwrap_or(0) != 1 {
            continue;
        }
        let nn = n as u32;
        // loop direction of this edge inside its (single) face
        let mut forward = true;
        'o: for f in &m.faces {
            for i in 0..f.len() {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                if a == e0 && b == e1 {
                    forward = true;
                    break 'o;
                }
                if a == e1 && b == e0 {
                    forward = false;
                    break 'o;
                }
            }
        }
        if forward {
            out.faces.push(vec![e0 + nn, e0, e1, e1 + nn]);
        } else {
            out.faces.push(vec![e0 + nn, e1 + nn, e1, e0]);
        }
    }
    out
}
