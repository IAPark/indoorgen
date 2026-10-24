//! Port of libccd's MPR penetration solver (double precision) and the FCL convex / triangle support glue,
//! which is what python-fcl's `Convex` collision objects (every convex trimesh) go through.
use crate::mesh::V3;

const EPS: f64 = f64::EPSILON;

#[inline]
fn is_zero(v: f64) -> bool {
    v.abs() < EPS
}
fn eq(a: f64, b: f64) -> bool {
    let ab = (a - b).abs();
    if ab < EPS {
        return true;
    }
    let (aa, bb) = (a.abs(), b.abs());
    if bb > aa {
        ab < EPS * bb
    } else {
        ab < EPS * aa
    }
}
fn vec_eq(a: V3, b: V3) -> bool {
    eq(a[0], b[0]) && eq(a[1], b[1]) && eq(a[2], b[2])
}
// The reference python-fcl is an arm64 build: clang contracts `a*b + c*d (+ e*f)` into fused multiply-adds
// (-ffp-contract=on).  libccd's vector helpers are reproduced with the same fusion.
fn dot(a: V3, b: V3) -> f64 {
    a[2].mul_add(b[2], a[1].mul_add(b[1], a[0] * b[0]))
}
fn cross(a: V3, b: V3) -> V3 {
    let f = |p: f64, q: f64, r: f64, s: f64| p.mul_add(q, -(r * s));
    [f(a[1], b[2], a[2], b[1]), f(a[2], b[0], a[0], b[2]), f(a[0], b[1], a[1], b[0])]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn len2(a: V3) -> f64 {
    dot(a, a)
}
fn normalize(a: V3) -> V3 {
    let k = 1.0 / len2(a).sqrt();
    scale(a, k)
}
fn dist2(a: V3, b: V3) -> f64 {
    len2(sub(a, b))
}

pub trait Shape {
    fn support(&self, dir: V3) -> V3;
    fn center(&self) -> V3;
}

/// Eigen::Quaterniond(Matrix3d) -> (x, y, z, w)
fn eigen_quat(m: &[[f64; 3]; 3]) -> [f64; 4] {
    let mut t = m[0][0] + m[1][1] + m[2][2];
    let mut c = [0.0f64; 4]; // x y z w
    if t > 0.0 {
        t = (t + 1.0).sqrt();
        c[3] = 0.5 * t;
        t = 0.5 / t;
        c[0] = (m[2][1] - m[1][2]) * t;
        c[1] = (m[0][2] - m[2][0]) * t;
        c[2] = (m[1][0] - m[0][1]) * t;
    } else {
        let mut i = 0;
        if m[1][1] > m[0][0] {
            i = 1;
        }
        if m[2][2] > m[i][i] {
            i = 2;
        }
        let j = (i + 1) % 3;
        let k = (j + 1) % 3;
        t = (m[i][i] - m[j][j] - m[k][k] + 1.0).sqrt();
        c[i] = 0.5 * t;
        t = 0.5 / t;
        c[3] = (m[k][j] - m[j][k]) * t;
        c[j] = (m[j][i] + m[i][j]) * t;
        c[k] = (m[k][i] + m[i][k]) * t;
    }
    c
}

fn quat_invert(q: [f64; 4]) -> [f64; 4] {
    let l2 = q[3].mul_add(q[3], q[2].mul_add(q[2], q[1].mul_add(q[1], q[0] * q[0])));
    let inv = 1.0 / l2;
    [-q[0] * inv, -q[1] * inv, -q[2] * inv, q[3] * inv]
}

/// ccdQuatRotVec
fn quat_rot(v: V3, q: [f64; 4]) -> V3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let f = |p: f64, q: f64, r: f64, s: f64| p.mul_add(q, -(r * s));
    let c1x = w.mul_add(vx, f(y, vz, z, vy));
    let c1y = w.mul_add(vy, f(z, vx, x, vz));
    let c1z = w.mul_add(vz, f(x, vy, y, vx));
    let c2x = f(y, c1z, z, c1y);
    let c2y = f(z, c1x, x, c1z);
    let c2z = f(x, c1y, y, c1x);
    [vx + 2.0 * c2x, vy + 2.0 * c2y, vz + 2.0 * c2z]
}

/// fcl::Convex<double> collision object (local vertices + transform)
pub struct ConvexShape<'a> {
    pub verts: &'a [V3],
    pub interior: V3,
    rot: [f64; 4],
    rot_inv: [f64; 4],
    pub t: V3,
}

impl<'a> ConvexShape<'a> {
    pub fn new(verts: &'a [V3], interior: V3, r: [[f64; 3]; 3], t: V3) -> ConvexShape<'a> {
        let rot = eigen_quat(&r);
        ConvexShape { verts, interior, rot, rot_inv: quat_invert(rot), t }
    }
}

impl<'a> Shape for ConvexShape<'a> {
    fn support(&self, dir: V3) -> V3 {
        let d = quat_rot(dir, self.rot_inv);
        let mut best = 0usize;
        let mut bv = dot(d, self.verts[0]);
        for i in 1..self.verts.len() {
            let v = dot(d, self.verts[i]);
            if v > bv {
                bv = v;
                best = i;
            }
        }
        add(quat_rot(self.verts[best], self.rot), self.t)
    }
    fn center(&self) -> V3 {
        add(quat_rot(self.interior, self.rot), self.t)
    }
}

/// a mesh triangle (local vertices, mesh transform) used as a convex shape
pub struct TriShape {
    pub p: [V3; 3],
    pub c: V3,
    rot: [f64; 4],
    rot_inv: [f64; 4],
    pub t: V3,
}

impl TriShape {
    pub fn new(p: [V3; 3], r: [[f64; 3]; 3], t: V3) -> TriShape {
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, (p[0][2] + p[1][2] + p[2][2]) / 3.0];
        let rot = eigen_quat(&r);
        TriShape { p, c, rot, rot_inv: quat_invert(rot), t }
    }
}

impl Shape for TriShape {
    fn support(&self, dir: V3) -> V3 {
        let d = quat_rot(dir, self.rot_inv);
        let mut maxdot = -f64::MAX;
        let mut v = self.p[0];
        for i in 0..3 {
            let pp = sub(self.p[i], self.c);
            let dd = dot(d, pp);
            if dd > maxdot {
                v = self.p[i];
                maxdot = dd;
            }
        }
        add(quat_rot(v, self.rot), self.t)
    }
    fn center(&self) -> V3 {
        add(quat_rot(self.c, self.rot), self.t)
    }
}

#[derive(Clone, Copy, Default)]
struct Sup {
    v: V3,
    v1: V3,
    v2: V3,
}

fn support(a: &dyn Shape, b: &dyn Shape, dir: V3) -> Sup {
    let v1 = a.support(dir);
    let v2 = b.support(scale(dir, -1.0));
    Sup { v: sub(v1, v2), v1, v2 }
}

fn point_segment_dist2(p: V3, x0: V3, b: V3) -> (f64, V3) {
    let d = sub(b, x0);
    let a = sub(x0, p);
    let mut t = -1.0 * dot(a, d);
    t /= len2(d);
    if t < 0.0 || is_zero(t) {
        (dist2(x0, p), x0)
    } else if t > 1.0 || eq(t, 1.0) {
        (dist2(b, p), b)
    } else {
        let w = add(scale(d, t), x0);
        (dist2(w, p), w)
    }
}

fn point_tri_dist2(p: V3, x0: V3, b: V3, c: V3) -> (f64, V3) {
    let d1 = sub(b, x0);
    let d2 = sub(c, x0);
    let a = sub(x0, p);
    let u = dot(a, a);
    let _ = u;
    let v = dot(d1, d1);
    let w = dot(d2, d2);
    let pp = dot(a, d1);
    let q = dot(a, d2);
    let r = dot(d1, d2);
    let d = w.mul_add(v, -(r * r));
    let (s, t);
    if is_zero(d) {
        s = -1.0;
        t = -1.0;
    } else {
        s = q.mul_add(r, -(w * pp)) / d;
        t = (-s).mul_add(r, -q) / w;
    }
    if (is_zero(s) || s > 0.0) && (eq(s, 1.0) || s < 1.0) && (is_zero(t) || t > 0.0) && (eq(t, 1.0) || t < 1.0) && (eq(t + s, 1.0) || t + s < 1.0) {
        let wit = add(add(x0, scale(d1, s)), scale(d2, t));
        (dist2(wit, p), wit)
    } else {
        let (mut dist, mut wit) = point_segment_dist2(p, x0, b);
        let (d2_, w2) = point_segment_dist2(p, x0, c);
        if d2_ < dist {
            dist = d2_;
            wit = w2;
        }
        let (d3, w3) = point_segment_dist2(p, b, c);
        if d3 < dist {
            dist = d3;
            wit = w3;
        }
        (dist, wit)
    }
}

fn portal_dir(p: &[Sup; 4]) -> V3 {
    let v2v1 = sub(p[2].v, p[1].v);
    let v3v1 = sub(p[3].v, p[1].v);
    normalize(cross(v2v1, v3v1))
}

fn portal_encapsules_origin(p: &[Sup; 4], dir: V3) -> bool {
    let d = dot(dir, p[1].v);
    is_zero(d) || d > 0.0
}

fn portal_reach_tolerance(p: &[Sup; 4], v4: &Sup, dir: V3, tol: f64) -> bool {
    let dv1 = dot(p[1].v, dir);
    let dv2 = dot(p[2].v, dir);
    let dv3 = dot(p[3].v, dir);
    let dv4 = dot(v4.v, dir);
    let mut d1 = dv4 - dv1;
    let d2 = dv4 - dv2;
    let d3 = dv4 - dv3;
    d1 = d1.min(d2);
    d1 = d1.min(d3);
    eq(d1, tol) || d1 < tol
}

fn portal_can_encapsule_origin(v4: &Sup, dir: V3) -> bool {
    let d = dot(v4.v, dir);
    is_zero(d) || d > 0.0
}

fn expand_portal(p: &mut [Sup; 4], v4: &Sup) {
    let v4v0 = cross(v4.v, p[0].v);
    let d = dot(p[1].v, v4v0);
    if d > 0.0 {
        let d2 = dot(p[2].v, v4v0);
        if d2 > 0.0 {
            p[1] = *v4;
        } else {
            p[3] = *v4;
        }
    } else {
        let d3 = dot(p[3].v, v4v0);
        if d3 > 0.0 {
            p[2] = *v4;
        } else {
            p[1] = *v4;
        }
    }
}

/// returns 0 (portal found), 1 (origin on v1), 2 (origin on v0-v1), -1 (no intersection)
fn discover_portal(a: &dyn Shape, b: &dyn Shape, portal: &mut [Sup; 4]) -> i32 {
    // vertex 0 is the center of the portal
    let c1 = a.center();
    let c2 = b.center();
    portal[0] = Sup { v: sub(c1, c2), v1: c1, v2: c2 };
    if vec_eq(portal[0].v, [0.0; 3]) {
        portal[0].v = add(portal[0].v, [EPS * 10.0, 0.0, 0.0]);
    }
    // vertex 1 = support in direction of origin
    let mut dir = normalize(scale(portal[0].v, -1.0));
    portal[1] = support(a, b, dir);
    let d = dot(portal[1].v, dir);
    if is_zero(d) || d < 0.0 {
        return -1;
    }
    // vertex 2
    dir = cross(portal[0].v, portal[1].v);
    if is_zero(len2(dir)) {
        return if vec_eq(portal[1].v, [0.0; 3]) { 1 } else { 2 };
    }
    dir = normalize(dir);
    portal[2] = support(a, b, dir);
    let d = dot(portal[2].v, dir);
    if is_zero(d) || d < 0.0 {
        return -1;
    }
    // vertex 3 direction
    let va = sub(portal[1].v, portal[0].v);
    let vb = sub(portal[2].v, portal[0].v);
    dir = normalize(cross(va, vb));
    // form portal faces oriented outside
    let d = dot(dir, portal[0].v);
    if d > 0.0 {
        portal.swap(1, 2);
        dir = scale(dir, -1.0);
    }
    let mut size = 3;
    while size < 4 {
        portal[3] = support(a, b, dir);
        let d = dot(portal[3].v, dir);
        if is_zero(d) || d < 0.0 {
            return -1;
        }
        let mut cont = false;
        let va = cross(portal[1].v, portal[3].v);
        let d = dot(va, portal[0].v);
        if d < 0.0 && !is_zero(d) {
            portal[2] = portal[3];
            cont = true;
        }
        if !cont {
            let va = cross(portal[3].v, portal[2].v);
            let d = dot(va, portal[0].v);
            if d < 0.0 && !is_zero(d) {
                portal[1] = portal[3];
                cont = true;
            }
        }
        if cont {
            let va = sub(portal[1].v, portal[0].v);
            let vb = sub(portal[2].v, portal[0].v);
            dir = normalize(cross(va, vb));
        } else {
            size = 4;
        }
    }
    0
}

fn refine_portal(a: &dyn Shape, b: &dyn Shape, portal: &mut [Sup; 4], tol: f64) -> i32 {
    loop {
        let dir = portal_dir(portal);
        if portal_encapsules_origin(portal, dir) {
            return 0;
        }
        let v4 = support(a, b, dir);
        if !portal_can_encapsule_origin(&v4, dir) || portal_reach_tolerance(portal, &v4, dir, tol) {
            return -1;
        }
        expand_portal(portal, &v4);
    }
}

fn find_pos(portal: &[Sup; 4]) -> V3 {
    let dir = portal_dir(portal);
    let mut b = [0.0f64; 4];
    let mut vec = cross(portal[1].v, portal[2].v);
    b[0] = dot(vec, portal[3].v);
    vec = cross(portal[3].v, portal[2].v);
    b[1] = dot(vec, portal[0].v);
    vec = cross(portal[0].v, portal[1].v);
    b[2] = dot(vec, portal[3].v);
    vec = cross(portal[2].v, portal[1].v);
    b[3] = dot(vec, portal[0].v);
    let mut sum = b[0] + b[1] + b[2] + b[3];
    if is_zero(sum) || sum < 0.0 {
        b[0] = 0.0;
        vec = cross(portal[2].v, portal[3].v);
        b[1] = dot(vec, dir);
        vec = cross(portal[3].v, portal[1].v);
        b[2] = dot(vec, dir);
        vec = cross(portal[1].v, portal[2].v);
        b[3] = dot(vec, dir);
        sum = b[1] + b[2] + b[3];
    }
    let inv = 1.0 / sum;
    let (mut p1, mut p2) = ([0.0; 3], [0.0; 3]);
    for i in 0..4 {
        p1 = add(p1, scale(portal[i].v1, b[i]));
        p2 = add(p2, scale(portal[i].v2, b[i]));
    }
    p1 = scale(p1, inv);
    p2 = scale(p2, inv);
    scale(add(p1, p2), 0.5)
}

/// ccdMPRPenetration: Some((depth, dir, pos)) when the shapes intersect
pub fn mpr_penetration(a: &dyn Shape, b: &dyn Shape, max_iterations: u64, tol: f64) -> Option<(f64, V3, V3)> {
    let mut portal = [Sup::default(); 4];
    let res = discover_portal(a, b, &mut portal);
    if res < 0 {
        return None;
    }
    if res == 1 {
        let pos = scale(add(portal[1].v1, portal[1].v2), 0.5);
        return Some((0.0, [0.0; 3], pos));
    }
    if res == 2 {
        let pos = scale(add(portal[1].v1, portal[1].v2), 0.5);
        let dir = portal[1].v;
        let depth = len2(dir).sqrt();
        return Some((depth, normalize(dir), pos));
    }
    if refine_portal(a, b, &mut portal, tol) < 0 {
        return None;
    }
    let mut iterations = 0u64;
    loop {
        let dir = portal_dir(&portal);
        let v4 = support(a, b, dir);
        if portal_reach_tolerance(&portal, &v4, dir, tol) || iterations > max_iterations {
            let (d2, w) = point_tri_dist2([0.0; 3], portal[1].v, portal[2].v, portal[3].v);
            let depth = d2.sqrt();
            let pdir = if is_zero(depth) { [0.0; 3] } else { normalize(w) };
            let pos = find_pos(&portal);
            return Some((depth, pdir, pos));
        }
        expand_portal(&mut portal, &v4);
        iterations += 1;
    }
}

/// default fcl::GJKSolver_libccd settings
pub const MAX_COLLISION_ITERATIONS: u64 = 500;
pub const COLLISION_TOLERANCE: f64 = 1e-6;

pub fn penetration(a: &dyn Shape, b: &dyn Shape) -> Option<f64> {
    mpr_penetration(a, b, MAX_COLLISION_ITERATIONS, COLLISION_TOLERANCE).map(|r| r.0)
}

// ---------------------------------------------------------------------------------------------
// GJK distance (fcl::detail::libccd_extension::ccdGJKSignedDist for separated shapes)
// ---------------------------------------------------------------------------------------------

fn eigen_normalized(v: V3) -> V3 {
    let z = dot(v, v);
    if z > 0.0 {
        let s = z.sqrt();
        [v[0] / s, v[1] / s, v[2] / s]
    } else {
        v
    }
}

#[derive(Clone, Copy)]
struct Simplex {
    ps: [Sup; 4],
    last: i32,
}

impl Simplex {
    fn size(&self) -> i32 {
        self.last + 1
    }
    fn add(&mut self, s: Sup) {
        self.last += 1;
        self.ps[self.last as usize] = s;
    }
    fn set(&mut self, pos: usize, s: Sup) {
        self.ps[pos] = s;
    }
    fn set_size(&mut self, size: i32) {
        self.last = size - 1;
    }
}

fn is_abs_less_eps_sq(v: f64) -> bool {
    v.abs() < EPS * EPS
}

fn do_simplex2(sx: &Simplex, dir: &mut V3) -> i32 {
    let p_oa = sx.ps[sx.last as usize].v;
    let p_ob = sx.ps[0].v;
    let p_ab = sub(p_ob, p_oa);
    let plane_normal = cross(p_ob, p_ab);
    if len2(plane_normal) < len2(p_ab) * len2(p_ob) * EPS * EPS {
        return 1;
    }
    let nd = cross(eigen_normalized(plane_normal), p_ab);
    *dir = nd;
    0
}

fn triple_cross(a: V3, b: V3, c: V3) -> V3 {
    cross(cross(a, b), c)
}

fn do_simplex3(sx: &mut Simplex, dir: &mut V3) -> i32 {
    let a = sx.ps[sx.last as usize];
    let b = sx.ps[1];
    let c = sx.ps[0];
    let (d2, _) = point_tri_dist2([0.0; 3], a.v, b.v, c.v);
    if is_abs_less_eps_sq(d2) {
        return 1;
    }
    if vec_eq(a.v, b.v) || vec_eq(a.v, c.v) {
        return -1;
    }
    let ao = scale(a.v, -1.0);
    let ab = sub(b.v, a.v);
    let ac = sub(c.v, a.v);
    let abc = cross(ab, ac);
    let tmp = cross(abc, ac);
    let mut d = dot(tmp, ao);
    // goto-style control flow of the C code
    let mut goto45 = false;
    if is_zero(d) || d > 0.0 {
        d = dot(ac, ao);
        if is_zero(d) || d > 0.0 {
            sx.set(1, a);
            sx.set_size(2);
            *dir = triple_cross(ac, ao, ac);
        } else {
            goto45 = true;
        }
    } else {
        let tmp = cross(ab, abc);
        d = dot(tmp, ao);
        if is_zero(d) || d > 0.0 {
            goto45 = true;
        } else {
            d = dot(abc, ao);
            if is_zero(d) || d > 0.0 {
                *dir = abc;
            } else {
                let ctmp = c;
                sx.set(0, b);
                sx.set(1, ctmp);
                *dir = scale(abc, -1.0);
            }
        }
    }
    if goto45 {
        d = dot(ab, ao);
        if is_zero(d) || d > 0.0 {
            sx.set(0, b);
            sx.set(1, a);
            sx.set_size(2);
            *dir = triple_cross(ab, ao, ab);
        } else {
            sx.set(0, a);
            sx.set_size(1);
            *dir = ao;
        }
    }
    0
}

fn do_simplex4(sx: &mut Simplex, dir: &mut V3) -> i32 {
    let a = sx.ps[sx.last as usize];
    let b = sx.ps[2];
    let c = sx.ps[1];
    let d = sx.ps[0];
    let (p_sa, p_sb, p_sc, p_sd) = (a.v, b.v, c.v, d.v);
    let p_ab = sub(p_sb, p_sa);
    let p_ac = sub(p_sc, p_sa);
    let p_ad = sub(p_sd, p_sa);
    let p_as = scale(p_sa, -1.0);
    let k_eps = EPS;
    let nhat_abc = eigen_normalized(cross(p_ab, p_ac));
    let so_to_abc = dot(nhat_abc, p_as);
    let d_to_abc = dot(nhat_abc, p_ad);
    if d_to_abc.abs() < k_eps {
        return -1;
    }
    if so_to_abc.abs() < k_eps {
        let (d2, _) = point_tri_dist2([0.0; 3], a.v, b.v, c.v);
        if is_abs_less_eps_sq(d2) {
            return 1;
        }
    }
    let nhat_acd = eigen_normalized(cross(p_ac, p_ad));
    let so_to_acd = dot(nhat_acd, p_as);
    if so_to_acd.abs() < k_eps {
        let (d2, _) = point_tri_dist2([0.0; 3], a.v, c.v, d.v);
        if is_abs_less_eps_sq(d2) {
            return 1;
        }
    }
    let nhat_adb = eigen_normalized(cross(p_ad, p_ab));
    let so_to_adb = dot(nhat_adb, p_as);
    if so_to_adb.abs() < k_eps {
        let (d2, _) = point_tri_dist2([0.0; 3], a.v, d.v, b.v);
        if is_abs_less_eps_sq(d2) {
            return 1;
        }
    }
    let c_to_adb = dot(nhat_adb, p_ac);
    let b_to_acd = dot(nhat_acd, p_ab);
    let so_in_abc = (so_to_abc * d_to_abc) > 0.0;
    let so_in_adb = (so_to_adb * c_to_adb) > 0.0;
    let so_in_acd = (so_to_acd * b_to_acd) > 0.0;
    if so_in_abc && so_in_adb && so_in_acd {
        return 1;
    } else if !so_in_acd {
        sx.set(2, a);
    } else if !so_in_adb {
        sx.set(1, d);
        sx.set(0, b);
        sx.set(2, a);
    } else {
        sx.set(0, c);
        sx.set(1, b);
        sx.set(2, a);
    }
    sx.set_size(3);
    do_simplex3(sx, dir)
}

fn do_simplex(sx: &mut Simplex, dir: &mut V3) -> i32 {
    match sx.size() {
        2 => do_simplex2(sx, dir),
        3 => do_simplex3(sx, dir),
        _ => do_simplex4(sx, dir),
    }
}

/// __ccdGJK: 0 when the shapes intersect (simplex holds the final state), -1 otherwise
fn ccd_gjk(a: &dyn Shape, b: &dyn Shape, max_iterations: u64, sx: &mut Simplex) -> i32 {
    sx.last = -1;
    let dir0 = [1.0, 0.0, 0.0];
    let mut last = support(a, b, dir0);
    sx.add(last);
    let mut dir = scale(last.v, -1.0);
    for _ in 0..max_iterations {
        last = support(a, b, dir);
        if dot(last.v, dir) < 0.0 {
            return -1;
        }
        sx.add(last);
        let r = do_simplex(sx, &mut dir);
        if r == 1 {
            return 0;
        } else if r == -1 {
            return -1;
        }
        if is_zero(len2(dir)) {
            return -1;
        }
    }
    -1
}

fn simplex_reduce_to_triangle(sx: &mut Simplex, mut dist: f64, best_witness: &mut V3) -> f64 {
    let mut best: i32 = -1;
    for i in 0..3usize {
        let p0 = sx.ps[if i == 0 { 3 } else { 0 }].v;
        let p1 = sx.ps[if i == 1 { 3 } else { 1 }].v;
        let p2 = sx.ps[if i == 2 { 3 } else { 2 }].v;
        let (d2, w) = point_tri_dist2([0.0; 3], p0, p1, p2);
        let nd = d2.sqrt();
        if nd < dist {
            dist = nd;
            best = i as i32;
            *best_witness = w;
        }
    }
    if best >= 0 {
        let s3 = sx.ps[3];
        sx.set(best as usize, s3);
    }
    sx.set_size(3);
    dist
}

fn ccd_dist(a: &dyn Shape, b: &dyn Shape, max_iterations: u64, tol: f64, sx: &mut Simplex) -> f64 {
    let mut last_dist = f64::MAX;
    for _ in 0..max_iterations {
        let mut closest_p = [0.0; 3];
        let dist;
        match sx.size() {
            1 => {
                closest_p = sx.ps[0].v;
                dist = len2(sx.ps[0].v).sqrt();
            }
            2 => {
                let (d2, w) = point_segment_dist2([0.0; 3], sx.ps[0].v, sx.ps[1].v);
                closest_p = w;
                dist = d2.sqrt();
            }
            3 => {
                let (d2, w) = point_tri_dist2([0.0; 3], sx.ps[0].v, sx.ps[1].v, sx.ps[2].v);
                closest_p = w;
                dist = d2.sqrt();
            }
            _ => {
                dist = simplex_reduce_to_triangle(sx, last_dist, &mut closest_p);
            }
        }
        if (last_dist - dist) < tol {
            return dist;
        }
        let dir = normalize(scale(closest_p, -1.0));
        let last = support(a, b, dir);
        last_dist = dist;
        let d = len2(last.v).sqrt();
        if (last_dist - d).abs() < tol {
            return last_dist;
        }
        sx.add(last);
    }
    -1.0
}

/// signed distance between two convex shapes as fcl reports it for separated shapes (negative: colliding)
pub fn distance(a: &dyn Shape, b: &dyn Shape) -> f64 {
    let mut sx = Simplex { ps: [Sup::default(); 4], last: -1 };
    if ccd_gjk(a, b, 1000, &mut sx) == 0 {
        return -1.0;
    }
    ccd_dist(a, b, 1000, 1e-6, &mut sx)
}
