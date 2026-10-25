//! float32 vector math with the exact formulas of Blender's math_vector / math_geom (used by the bmesh bevel port)
use super::{BMesh, NONE, V3};

pub const PI: f32 = std::f32::consts::PI;

#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn mul(a: V3, f: f32) -> V3 {
    [a[0] * f, a[1] * f, a[2] * f]
}
#[inline]
pub fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
/// madd_v3_v3v3fl: a + b * f
#[inline]
pub fn madd(a: V3, b: V3, f: f32) -> V3 {
    [a[0] + b[0] * f, a[1] + b[1] * f, a[2] + b[2] * f]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len_sq(a: V3) -> f32 {
    dot(a, a)
}
#[inline]
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn len_squared_v3v3(a: V3, b: V3) -> f32 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}
pub fn len_v3v3(a: V3, b: V3) -> f32 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}
/// normalize_v3: returns (normalized, original length) ; zero vector stays zero
pub fn normalize_len(a: V3) -> (V3, f32) {
    let d = dot(a, a);
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        (mul(a, 1.0f32 / l), l)
    } else {
        ([0.0; 3], 0.0)
    }
}
pub fn normalize(a: V3) -> V3 {
    normalize_len(a).0
}
pub fn mid(a: V3, b: V3) -> V3 {
    [0.5f32 * (a[0] + b[0]), 0.5f32 * (a[1] + b[1]), 0.5f32 * (a[2] + b[2])]
}
pub fn interp(a: V3, b: V3, t: f32) -> V3 {
    let s = 1.0f32 - t;
    [s * a[0] + t * b[0], s * a[1] + t * b[1], s * a[2] + t * b[2]]
}
pub fn is_zero(a: V3) -> bool {
    a[0] == 0.0 && a[1] == 0.0 && a[2] == 0.0
}
pub fn compare_ff(a: f32, b: f32, max_diff: f32) -> bool {
    (a - b).abs() <= max_diff
}
pub fn compare_v3v3(a: V3, b: V3, limit: f32) -> bool {
    compare_ff(a[0], b[0], limit) && compare_ff(a[1], b[1], limit) && compare_ff(a[2], b[2], limit)
}
pub fn safe_asinf(a: f32) -> f32 {
    if a <= -1.0 {
        -PI / 2.0
    } else if a >= 1.0 {
        PI / 2.0
    } else {
        a.asin()
    }
}
pub fn angle_normalized_v3v3(v1: V3, v2: V3) -> f32 {
    if dot(v1, v2) >= 0.0 {
        return 2.0 * safe_asinf(len_v3v3(v1, v2) / 2.0);
    }
    let v2n = neg(v2);
    PI - 2.0 * safe_asinf(len_v3v3(v1, v2n) / 2.0)
}
pub fn angle_v3v3(a: V3, b: V3) -> f32 {
    angle_normalized_v3v3(normalize(a), normalize(b))
}
pub fn angle_v3v3v3(a: V3, b: V3, c: V3) -> f32 {
    let v1 = normalize(sub(b, a));
    let v2 = normalize(sub(b, c));
    angle_normalized_v3v3(v1, v2)
}
pub fn project_v3_v3v3(p: V3, v_proj: V3) -> V3 {
    if is_zero(v_proj) {
        return [0.0; 3];
    }
    let m = dot(p, v_proj) / dot(v_proj, v_proj);
    mul(v_proj, m)
}
pub fn plane_from_point_normal(co: V3, no: V3) -> [f32; 4] {
    [no[0], no[1], no[2], -dot(no, co)]
}
pub fn plane_point_side(plane: [f32; 4], pt: V3) -> f32 {
    plane[0] * pt[0] + plane[1] * pt[1] + plane[2] * pt[2] + plane[3]
}
pub fn closest_to_plane(plane: [f32; 4], pt: V3) -> V3 {
    let n = [plane[0], plane[1], plane[2]];
    let len_sq_ = len_sq(n);
    let side = plane_point_side(plane, pt);
    madd(pt, n, -side / len_sq_)
}
pub fn closest_to_plane_normalized(plane: [f32; 4], pt: V3) -> V3 {
    let side = plane_point_side(plane, pt);
    madd(pt, [plane[0], plane[1], plane[2]], -side)
}
/// closest_to_ray_v3: (lambda, point)
pub fn closest_to_ray(p: V3, orig: V3, dir: V3) -> (f32, V3) {
    if is_zero(dir) {
        return (0.0, orig);
    }
    let h = sub(p, orig);
    let lambda = dot(dir, h) / dot(dir, dir);
    (lambda, madd(orig, dir, lambda))
}
pub fn closest_to_line(p: V3, l1: V3, l2: V3) -> (f32, V3) {
    closest_to_ray(p, l1, sub(l2, l1))
}
pub fn closest_to_line_segment(p: V3, l1: V3, l2: V3) -> V3 {
    let (lambda, cp) = closest_to_line(p, l1, l2);
    if lambda <= 0.0 {
        l1
    } else if lambda >= 1.0 {
        l2
    } else {
        cp
    }
}
/// isect_line_line_v3: returns (kind, i1, i2): 0 none, 1 one point, 2 nearest points
pub fn isect_line_line(v1: V3, v2: V3, v3: V3, v4: V3) -> (i32, V3, V3) {
    let epsilon = 0.000001f32;
    let c = sub(v3, v1);
    let a = sub(v2, v1);
    let b = sub(v4, v3);
    let ab = cross(a, b);
    let d = dot(c, ab);
    let div = dot(ab, ab);
    if div == 0.0 {
        return (0, [0.0; 3], [0.0; 3]);
    }
    if d.abs() <= epsilon {
        let cb = cross(c, b);
        let a2 = mul(a, dot(cb, ab) / div);
        let i1 = add(v1, a2);
        return (1, i1, i1);
    }
    let n = cross(a, b);
    let t = project_v3_v3v3(sub(v1, v3), n);
    let v3t = add(v3, t);
    let v4t = add(v4, t);
    let c2 = sub(v3t, v1);
    let a2 = sub(v2, v1);
    let b2 = sub(v4t, v3t);
    let ab2 = cross(a2, b2);
    let cb2 = cross(c2, b2);
    let a3 = mul(a2, dot(cb2, ab2) / dot(ab2, ab2));
    let i1 = add(v1, a3);
    let i2 = sub(i1, t);
    (2, i1, i2)
}
pub fn isect_line_plane(l1: V3, l2: V3, plane_co: V3, plane_no: V3) -> Option<V3> {
    let u = sub(l2, l1);
    let h = sub(l1, plane_co);
    let d = dot(plane_no, u);
    if d.abs() > f32::EPSILON {
        let lambda = -dot(plane_no, h) / d;
        return Some(madd(l1, u, lambda));
    }
    None
}
pub fn interp_bilinear_quad(data: [V3; 4], u: f32, v: f32) -> V3 {
    let mut res = mul(data[0], (1.0 - u) * (1.0 - v));
    res = add(res, mul(data[1], u * (1.0 - v)));
    res = add(res, mul(data[2], u * v));
    res = add(res, mul(data[3], (1.0 - u) * v));
    res
}
pub type M4 = [[f32; 4]; 4];
/// mul_v3_m4v3: row-vector convention of Blender (m[col][row])
pub fn mul_v3_m4v3(m: &M4, v: V3) -> V3 {
    let (x, y, z) = (v[0], v[1], v[2]);
    [x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0], x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1], x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2]]
}
pub fn mul_m4_v4(m: &M4, r: [f32; 4]) -> [f32; 4] {
    let (x, y, z, w) = (r[0], r[1], r[2], r[3]);
    [
        x * m[0][0] + y * m[1][0] + z * m[2][0] + w * m[3][0],
        x * m[0][1] + y * m[1][1] + z * m[2][1] + w * m[3][1],
        x * m[0][2] + y * m[1][2] + z * m[2][2] + w * m[3][2],
        x * m[0][3] + y * m[1][3] + z * m[2][3] + w * m[3][3],
    ]
}

/// BM_face_calc_normal
pub fn face_normal(bm: &BMesh, f: usize) -> V3 {
    let ls = bm.loops_of_face(f);
    let co = |i: usize| bm.verts[bm.loops[ls[i]].v].co;
    match ls.len() {
        3 => normalize(cross(sub(co(0), co(1)), sub(co(1), co(2)))),
        4 => normalize(cross(sub(co(2), co(0)), sub(co(3), co(1)))),
        n => {
            let mut nrm = [0.0f32; 3];
            let mut prev = co(n - 1);
            for i in 0..n {
                let cur = co(i);
                nrm[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
                nrm[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
                nrm[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
                prev = cur;
            }
            normalize(nrm)
        }
    }
}
pub fn _unused(_: usize) -> usize {
    NONE
}
