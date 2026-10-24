//! Rotation helpers (quaternion / euler / axis-angle) and color blending used by function nodes.
use super::math::*;
use super::val::*;
use crate::bl32;

pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let t0 = a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3];
    let t1 = a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2];
    let t2 = a[0] * b[2] + a[2] * b[0] + a[3] * b[1] - a[1] * b[3];
    let t3 = a[0] * b[3] + a[3] * b[0] + a[1] * b[2] - a[2] * b[1];
    [t0, t1, t2, t3]
}

pub fn axis_angle_to_quat(axis: V3, angle: f32) -> [f32; 4] {
    let n = norm3(axis);
    let phi = 0.5 * angle;
    let (si, co) = (phi.sin(), phi.cos());
    [co, n[0] * si, n[1] * si, n[2] * si]
}

pub fn quat_rotate(q: [f32; 4], v: V3) -> V3 {
    let mut v = v;
    let t0 = -q[1] * v[0] - q[2] * v[1] - q[3] * v[2];
    let mut t1 = q[0] * v[0] + q[2] * v[2] - q[3] * v[1];
    let mut t2 = q[0] * v[1] + q[3] * v[0] - q[1] * v[2];
    v[2] = q[0] * v[2] + q[1] * v[1] - q[2] * v[0];
    v[0] = t1;
    v[1] = t2;
    t1 = t0 * -q[1] + v[0] * q[0] - v[1] * q[3] + v[2] * q[2];
    t2 = t0 * -q[2] + v[1] * q[0] - v[2] * q[1] + v[0] * q[3];
    v[2] = t0 * -q[3] + v[2] * q[0] - v[0] * q[2] + v[1] * q[1];
    v[0] = t1;
    v[1] = t2;
    v
}

pub fn slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut b = b;
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if d < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        d = -d;
    }
    let (w1, w2);
    if d > 0.9995 {
        w1 = 1.0 - t;
        w2 = t;
    } else {
        let th = d.clamp(-1.0, 1.0).acos();
        let s = th.sin();
        w1 = ((1.0 - t) * th).sin() / s;
        w2 = (t * th).sin() / s;
    }
    let mut r = [0.0; 4];
    for k in 0..4 {
        r[k] = a[k] * w1 + b[k] * w2;
    }
    r
}

/// axis_angle_to_mat3 (normalizes the axis; direct matrix formula, float32)
pub fn axis_angle_to_mat3(axis: V3, angle: f32) -> bl32::M3 {
    let a = bl32::normalize_v3(axis).0;
    let (si, co) = (angle.sin(), angle.cos());
    let ico = 1.0 - co;
    let nsi = [a[0] * si, a[1] * si, a[2] * si];
    let n00 = (a[0] * a[0]) * ico;
    let n01 = (a[0] * a[1]) * ico;
    let n11 = (a[1] * a[1]) * ico;
    let n02 = (a[0] * a[2]) * ico;
    let n12 = (a[1] * a[2]) * ico;
    let n22 = (a[2] * a[2]) * ico;
    let mut m = [[0.0f32; 3]; 3];
    m[0][0] = n00 + co;
    m[0][1] = n01 + nsi[2];
    m[0][2] = n02 - nsi[1];
    m[1][0] = n01 - nsi[2];
    m[1][1] = n11 + co;
    m[1][2] = n12 + nsi[0];
    m[2][0] = n02 + nsi[1];
    m[2][1] = n12 - nsi[0];
    m[2][2] = n22 + co;
    m
}

fn compatible_eul(eul: &mut V3, old: V3) {
    let pi_thresh = 5.1f32;
    let pi_x2 = 2.0 * std::f32::consts::PI;
    let mut deul = [eul[0] - old[0], eul[1] - old[1], eul[2] - old[2]];
    for i in 0..3 {
        if deul[i] > pi_thresh {
            eul[i] -= ((deul[i] / pi_x2) + 0.5).floor() * pi_x2;
            deul[i] = eul[i] - old[i];
        } else if deul[i] < -pi_thresh {
            eul[i] += ((-deul[i] / pi_x2) + 0.5).floor() * pi_x2;
            deul[i] = eul[i] - old[i];
        }
    }
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        if deul[i].abs() > 3.2 && deul[j].abs() < 1.6 && deul[k].abs() < 1.6 {
            if deul[i] > 0.0 {
                eul[i] -= pi_x2;
            } else {
                eul[i] += pi_x2;
            }
        }
    }
}

/// both XYZ-euler solutions of a normalized rotation matrix
fn mat3_to_eul2(m: &bl32::M3) -> (V3, V3) {
    let cy = m[0][0].hypot(m[0][1]);
    if cy > 3.756e-5f32 {
        (
            [m[1][2].atan2(m[2][2]), (-m[0][2]).atan2(cy), m[0][1].atan2(m[0][0])],
            [(-m[1][2]).atan2(-m[2][2]), (-m[0][2]).atan2(-cy), (-m[0][1]).atan2(-m[0][0])],
        )
    } else {
        let e = [(-m[2][1]).atan2(m[1][1]), (-m[0][2]).atan2(cy), 0.0];
        (e, e)
    }
}

pub fn mat3_to_eul_plain(m: &bl32::M3) -> V3 {
    let mut n = *m;
    for i in 0..3 {
        n[i] = bl32::normalize_v3(m[i]).0;
    }
    bl32::mat3_normalized_to_eul(&n)
}

pub fn mat3_to_compatible_eul(old: V3, m: &bl32::M3) -> V3 {
    // normalize the matrix columns first (mat3_to_compatible_eul)
    let mut n = *m;
    for i in 0..3 {
        n[i] = bl32::normalize_v3(m[i]).0;
    }
    let (mut e1, mut e2) = mat3_to_eul2(&n);
    compatible_eul(&mut e1, old);
    compatible_eul(&mut e2, old);
    let d1 = (e1[0] - old[0]).abs() + (e1[1] - old[1]).abs() + (e1[2] - old[2]).abs();
    let d2 = (e2[0] - old[0]).abs() + (e2[1] - old[1]).abs() + (e2[2] - old[2]).abs();
    if d1 > d2 {
        e2
    } else {
        e1
    }
}

fn angle_normalized(a: V3, b: V3) -> f32 {
    if dot3(a, b) >= 0.0 {
        2.0 * (len3(sub3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (len3(add3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

fn angle_v3v3(a: V3, b: V3) -> f32 {
    angle_normalized(norm3_c(a), norm3_c(b))
}

fn project_plane(v: V3, n: V3) -> V3 {
    let l = dot3(n, n);
    let mul = dot3(v, n) / l;
    sub3(v, mul3(n, mul))
}

fn angle_signed_on_axis(v1: V3, v2: V3, axis: V3) -> f32 {
    let v1p = project_plane(v1, axis);
    let v2p = project_plane(v2, axis);
    let tproj = cross3(v1p, v2p);
    let a = angle_v3v3(v1p, v2p);
    if dot3(tproj, axis) < 0.0 {
        -a
    } else {
        a
    }
}

fn mul_m3_v3(m: &bl32::M3, v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

pub fn align_euler_to_vector(rot: V3, factor: f32, vector: V3, axis: &str, pivot: &str) -> V3 {
    if vector == [0.0; 3] {
        return rot;
    }
    let main: V3 = match axis {
        "Y" => [0.0, 1.0, 0.0],
        "Z" => [0.0, 0.0, 1.0],
        _ => [1.0, 0.0, 0.0],
    };
    let old = bl32::eul_to_mat3(rot);
    let old_axis = mul_m3_v3(&old, main);
    if pivot == "AUTO" {
        let new_axis = norm3(vector);
        let cx = |a: V3, b: V3| -> V3 {
            let (x, y) = ([a[0] as f64, a[1] as f64, a[2] as f64], [b[0] as f64, b[1] as f64, b[2] as f64]);
            [(x[1] * y[2] - x[2] * y[1]) as f32, (x[2] * y[0] - x[0] * y[2]) as f32, (x[0] * y[1] - x[1] * y[0]) as f32]
        };
        let mut raxis = cx(old_axis, new_axis);
        if raxis == [0.0; 3] {
            raxis = cx(old_axis, [1.0, 0.0, 0.0]);
            if raxis == [0.0; 3] {
                raxis = cx(old_axis, [0.0, 1.0, 0.0]);
            }
        }
        let full = angle_normalized(old_axis, new_axis);
        let angle = factor * full;
        let rm = axis_angle_to_mat3(raxis, angle);
        let nr = bl32::mul_m3_m3m3(&rm, &old);
        mat3_to_eul_plain(&nr)
    } else {
        let local_pivot: V3 = match pivot {
            "Y" => [0.0, 1.0, 0.0],
            "Z" => [0.0, 0.0, 1.0],
            _ => [1.0, 0.0, 0.0],
        };
        if local_pivot == main {
            return rot;
        }
        let pivot_axis = mul_m3_v3(&old, local_pivot);
        let full = angle_signed_on_axis(old_axis, vector, pivot_axis);
        let angle = factor * full;
        let rm = axis_angle_to_mat3(pivot_axis, angle);
        let nr = bl32::mul_m3_m3m3(&rm, &old);
        mat3_to_eul_plain(&nr)
    }
}

pub fn vector_rotate(mode: &str, invert: bool, v: V3, center: V3, axis: V3, angle: f32, euler: V3) -> V3 {
    let rel = sub3(v, center);
    let r = match mode {
        "X_AXIS" | "Y_AXIS" | "Z_AXIS" => {
            let ax = match mode {
                "X_AXIS" => [1.0, 0.0, 0.0],
                "Y_AXIS" => [0.0, 1.0, 0.0],
                _ => [0.0, 0.0, 1.0],
            };
            let a = if invert { -angle } else { angle };
            let m = axis_angle_to_mat3(ax, a);
            mul_m3_v3(&m, rel)
        }
        "EULER_XYZ" => {
            let m = bl32::eul_to_mat3(euler);
            if invert {
                // transpose
                let t = [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]];
                mul_m3_v3(&t, rel)
            } else {
                mul_m3_v3(&m, rel)
            }
        }
        _ => {
            let a = if invert { -angle } else { angle };
            let m = axis_angle_to_mat3(axis, a);
            mul_m3_v3(&m, rel)
        }
    };
    add3(r, center)
}

pub fn blend_color(mode: &str, f: f32, a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let facm = 1.0 - f;
    let mut o = [0.0f32; 4];
    for k in 0..3 {
        let (x, y) = (a[k], b[k]);
        o[k] = match mode {
            "ADD" => x + f * y,
            "MULTIPLY" => facm * x + f * x * y,
            "SUBTRACT" => x - f * y,
            "SCREEN" => 1.0 - (facm + f * (1.0 - y)) * (1.0 - x),
            "DIVIDE" => {
                if y != 0.0 {
                    facm * x + f * x / y
                } else {
                    x
                }
            }
            "DIFFERENCE" => facm * x + f * (x - y).abs(),
            "DARKEN" => x * facm + x.min(y) * f,
            "LIGHTEN" => x.max(y * f),
            "OVERLAY" => {
                if x < 0.5 {
                    x * (facm + 2.0 * f * y)
                } else {
                    1.0 - (facm + 2.0 * f * (1.0 - y)) * (1.0 - x)
                }
            }
            "EXCLUSION" => facm * x + f * (x + y - 2.0 * x * y),
            "LINEAR_LIGHT" => {
                if y > 0.5 {
                    x + f * (2.0 * (y - 0.5))
                } else {
                    x + f * (2.0 * y - 1.0)
                }
            }
            _ => x * facm + y * f,
        };
    }
    o[3] = a[3] * facm + b[3] * f;
    o
}

/// angle_signed_on_axis_v3v3_v3 (C version: negative angles are returned as 2*pi - angle)
fn angle_signed_on_axis_c(v1: V3, v2: V3, axis: V3) -> f32 {
    let v1p = project_plane(v1, axis);
    let v2p = project_plane(v2, axis);
    let mut a = angle_v3v3(v1p, v2p);
    let tproj = cross3(v2p, v1p);
    if dot3(tproj, axis) < 0.0 {
        a = (std::f64::consts::PI * 2.0) as f32 - a;
    }
    a
}

/// Align Rotation to Vector node on quaternions
pub fn align_rotation_to_vector(old: [f32; 4], factor: f32, vector: V3, axis: &str, pivot: &str) -> [f32; 4] {
    if vector == [0.0; 3] {
        return old;
    }
    let unit = |a: &str| -> V3 {
        match a {
            "Y" => [0.0, 1.0, 0.0],
            "Z" => [0.0, 0.0, 1.0],
            _ => [1.0, 0.0, 0.0],
        }
    };
    let main = unit(axis);
    let old_axis = quat_rotate(old, main);
    if pivot == "AUTO" {
        let new_axis = norm3(vector);
        let cx = |a: V3, b: V3| -> V3 {
            let (x, y) = ([a[0] as f64, a[1] as f64, a[2] as f64], [b[0] as f64, b[1] as f64, b[2] as f64]);
            [(x[1] * y[2] - x[2] * y[1]) as f32, (x[2] * y[0] - x[0] * y[2]) as f32, (x[0] * y[1] - x[1] * y[0]) as f32]
        };
        let mut raxis = cx(old_axis, new_axis);
        if raxis == [0.0; 3] {
            raxis = cx(old_axis, [1.0, 0.0, 0.0]);
            if raxis == [0.0; 3] {
                raxis = cx(old_axis, [0.0, 1.0, 0.0]);
            }
        }
        let full = angle_normalized(old_axis, new_axis);
        let angle = factor * full;
        quat_mul(axis_angle_to_quat(norm3(raxis), angle), old)
    } else {
        let lp = unit(pivot);
        if lp == main {
            return old;
        }
        let pivot_axis = quat_rotate(old, lp);
        let mut full = angle_signed_on_axis_c(vector, old_axis, pivot_axis);
        if full > std::f32::consts::PI {
            full -= (2.0 * std::f64::consts::PI) as f32;
        }
        let angle = factor * full;
        quat_mul(axis_angle_to_quat(norm3(pivot_axis), angle), old)
    }
}
