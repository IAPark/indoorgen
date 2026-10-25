//! Float32 math used by the Math / Vector Math / Compare / Map Range / rotation nodes (Blender 4.2 semantics).
use super::val::*;
use crate::bl32;

pub fn euler_to_quat(e: V3) -> [f32; 4] {
    let (ti, tj, th) = (e[0] as f64 * 0.5, e[1] as f64 * 0.5, e[2] as f64 * 0.5);
    let (ci, cj, ch) = (ti.cos(), tj.cos(), th.cos());
    let (si, sj, sh) = (ti.sin(), tj.sin(), th.sin());
    let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
    [(cj * cc + sj * ss) as f32, (cj * sc - sj * cs) as f32, (cj * ss + sj * cc) as f32, (cj * cs - sj * sc) as f32]
}

pub fn quat_to_euler(q: [f32; 4]) -> V3 {
    let m = bl32::quat_to_mat3(q);
    bl32::mat3_normalized_to_eul(&m)
}

pub fn dot3(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul3(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len3(a: V3) -> f32 {
    dot3(a, a).sqrt()
}
/// math::normalize (zero-safe; v / length)
pub fn norm3(a: V3) -> V3 {
    let l = len3(a);
    if l != 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0; 3]
    }
}
/// normalize_v3 (multiply by 1/len, threshold)
pub fn norm3_c(a: V3) -> V3 {
    bl32::normalize_v3(a).0
}

pub fn safe_div(a: f32, b: f32) -> f32 {
    if b != 0.0 {
        a / b
    } else {
        0.0
    }
}

pub fn wrapf(v: f32, max: f32, min: f32) -> f32 {
    let range = max - min;
    if range != 0.0 {
        v - range * ((v - min) / range).floor()
    } else {
        min
    }
}

pub fn fractf(a: f32) -> f32 {
    a - a.floor()
}

pub fn pingpong(v: f32, s: f32) -> f32 {
    if s != 0.0 {
        (fractf((v - s) / (s * 2.0)) * s * 2.0 - s).abs()
    } else {
        0.0
    }
}

pub fn smoothmin(a: f32, b: f32, c: f32) -> f32 {
    if c != 0.0 {
        let h = (c - (a - b).abs()).max(0.0) / c;
        a.min(b) - h * h * h * c * (1.0 / 6.0)
    } else {
        a.min(b)
    }
}

pub fn safe_pow(a: f32, b: f32) -> f32 {
    if a < 0.0 && b != (b as i32) as f32 {
        return 0.0;
    }
    a.powf(b)
}

pub fn compatible_sign(a: f32) -> f32 {
    if a < 0.0 {
        -1.0
    } else if a > 0.0 {
        1.0
    } else {
        0.0
    }
}

pub fn math_op(op: &str, a: f32, b: f32, c: f32) -> f32 {
    match op {
        "ADD" => a + b,
        "SUBTRACT" => a - b,
        "MULTIPLY" => a * b,
        "DIVIDE" => safe_div(a, b),
        "MULTIPLY_ADD" => a * b + c,
        "POWER" => safe_pow(a, b),
        "LOGARITHM" => {
            if a > 0.0 && b > 0.0 {
                a.ln() / b.ln()
            } else {
                0.0
            }
        }
        "SQRT" => {
            if a > 0.0 {
                a.sqrt()
            } else {
                0.0
            }
        }
        "INVERSE_SQRT" => {
            if a > 0.0 {
                1.0 / a.sqrt()
            } else {
                0.0
            }
        }
        "ABSOLUTE" => a.abs(),
        "EXPONENT" => a.exp(),
        "MINIMUM" => a.min(b),
        "MAXIMUM" => a.max(b),
        "LESS_THAN" => (a < b) as i32 as f32,
        "GREATER_THAN" => (a > b) as i32 as f32,
        "SIGN" => compatible_sign(a),
        "COMPARE" => ((a - b).abs() <= c.max(1e-5)) as i32 as f32,
        "SMOOTH_MIN" => smoothmin(a, b, c),
        "SMOOTH_MAX" => -smoothmin(-a, -b, c),
        "ROUND" => (a + 0.5).floor(),
        "FLOOR" => a.floor(),
        "CEIL" => a.ceil(),
        "TRUNC" => a.trunc(),
        "FRACT" => fractf(a),
        "MODULO" | "TRUNCATED_MODULO" => {
            if b == 0.0 {
                0.0
            } else {
                a % b
            }
        }
        "FLOORED_MODULO" => {
            if b == 0.0 {
                0.0
            } else {
                a - (a / b).floor() * b
            }
        }
        "WRAP" => wrapf(a, b, c),
        "SNAP" => {
            if b == 0.0 {
                0.0
            } else {
                (a / b).floor() * b
            }
        }
        "PINGPONG" => pingpong(a, b),
        "SINE" => a.sin(),
        "COSINE" => a.cos(),
        "TANGENT" => a.tan(),
        "ARCSINE" => a.clamp(-1.0, 1.0).asin(),
        "ARCCOSINE" => a.clamp(-1.0, 1.0).acos(),
        "ARCTANGENT" => a.atan(),
        "ARCTAN2" => a.atan2(b),
        "SINH" => a.sinh(),
        "COSH" => a.cosh(),
        "TANH" => a.tanh(),
        "RADIANS" => a * ((std::f64::consts::PI / 180.0) as f32),
        "DEGREES" => a * ((180.0 / std::f64::consts::PI) as f32),
        _ => 0.0,
    }
}

pub fn vec_op(op: &str, a: V3, b: V3, c: V3, s: f32) -> (V3, f32) {
    let m3 = |f: &dyn Fn(f32, f32) -> f32| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])];
    let v = match op {
        "ADD" => m3(&|x, y| x + y),
        "SUBTRACT" => m3(&|x, y| x - y),
        "MULTIPLY" => m3(&|x, y| x * y),
        "DIVIDE" => m3(&|x, y| safe_div(x, y)),
        "MULTIPLY_ADD" => [a[0] * b[0] + c[0], a[1] * b[1] + c[1], a[2] * b[2] + c[2]],
        "CROSS_PRODUCT" => {
            let (x, y) = ([a[0] as f64, a[1] as f64, a[2] as f64], [b[0] as f64, b[1] as f64, b[2] as f64]);
            [(x[1] * y[2] - x[2] * y[1]) as f32, (x[2] * y[0] - x[0] * y[2]) as f32, (x[0] * y[1] - x[1] * y[0]) as f32]
        }
        "PROJECT" => {
            let l = dot3(b, b);
            if l != 0.0 {
                mul3(b, dot3(a, b) / l)
            } else {
                [0.0; 3]
            }
        }
        "REFLECT" => {
            let n = norm3(b);
            sub3(a, mul3(n, 2.0 * dot3(n, a)))
        }
        "REFRACT" => {
            // refract_v3_v3v3(r, a, normalize(b), s)
            let n = norm3(b);
            let eta = s;
            let mut dv = dot3(n, a);
            let k = 1.0 - eta * eta * (1.0 - dv * dv);
            if k < 0.0 {
                [0.0; 3]
            } else {
                dv = dv * eta + k.sqrt();
                sub3(mul3(a, eta), mul3(n, dv))
            }
        }
        "FACEFORWARD" => {
            if dot3(c, b) < 0.0 {
                a
            } else {
                [-a[0], -a[1], -a[2]]
            }
        }
        "SCALE" => mul3(a, s),
        "NORMALIZE" => norm3(a),
        "ABSOLUTE" => [a[0].abs(), a[1].abs(), a[2].abs()],
        "POWER" => m3(&|x, y| safe_pow(x, y)),
        "SIGN" => [compatible_sign(a[0]), compatible_sign(a[1]), compatible_sign(a[2])],
        "MINIMUM" => m3(&|x, y| x.min(y)),
        "MAXIMUM" => m3(&|x, y| x.max(y)),
        "FLOOR" => [a[0].floor(), a[1].floor(), a[2].floor()],
        "CEIL" => [a[0].ceil(), a[1].ceil(), a[2].ceil()],
        "FRACTION" => [fractf(a[0]), fractf(a[1]), fractf(a[2])],
        "MODULO" => m3(&|x, y| if y == 0.0 { 0.0 } else { x % y }),
        "WRAP" => [wrapf(a[0], b[0], c[0]), wrapf(a[1], b[1], c[1]), wrapf(a[2], b[2], c[2])],
        "SNAP" => m3(&|x, y| if y == 0.0 { 0.0 } else { (x / y).floor() * y }),
        "SINE" => [a[0].sin(), a[1].sin(), a[2].sin()],
        "COSINE" => [a[0].cos(), a[1].cos(), a[2].cos()],
        "TANGENT" => [a[0].tan(), a[1].tan(), a[2].tan()],
        _ => [0.0; 3],
    };
    let f = match op {
        "DOT_PRODUCT" => dot3(a, b),
        "DISTANCE" => len3(sub3(a, b)),
        "LENGTH" => len3(a),
        _ => 0.0,
    };
    (v, f)
}

// ---------------------------------------------------------------- hashing (BLI_hash / noise)
#[inline]
fn rot(x: u32, k: u32) -> u32 {
    x.rotate_left(k)
}
#[inline]
fn final_mix(mut a: u32, mut b: u32, mut c: u32) -> u32 {
    c ^= b;
    c = c.wrapping_sub(rot(b, 14));
    a ^= c;
    a = a.wrapping_sub(rot(c, 11));
    b ^= a;
    b = b.wrapping_sub(rot(a, 25));
    c ^= b;
    c = c.wrapping_sub(rot(b, 16));
    a ^= c;
    a = a.wrapping_sub(rot(c, 4));
    b ^= a;
    b = b.wrapping_sub(rot(a, 14));
    c ^= b;
    c = c.wrapping_sub(rot(b, 24));
    let _ = a;
    c
}
pub fn hash1(kx: u32) -> u32 {
    let a = 0xdeadbeefu32.wrapping_add(1 << 2).wrapping_add(13);
    final_mix(a.wrapping_add(kx), a, a)
}
pub fn hash2(kx: u32, ky: u32) -> u32 {
    let a = 0xdeadbeefu32.wrapping_add(2 << 2).wrapping_add(13);
    final_mix(a.wrapping_add(kx), a.wrapping_add(ky), a)
}
pub fn hash3(kx: u32, ky: u32, kz: u32) -> u32 {
    let a = 0xdeadbeefu32.wrapping_add(3 << 2).wrapping_add(13);
    final_mix(a.wrapping_add(kx), a.wrapping_add(ky), a.wrapping_add(kz))
}
pub fn hash4(kx: u32, ky: u32, kz: u32, kw: u32) -> u32 {
    let mut a = 0xdeadbeefu32.wrapping_add(4 << 2).wrapping_add(13);
    let mut b = a;
    let mut c = a;
    a = a.wrapping_add(kx);
    b = b.wrapping_add(ky);
    c = c.wrapping_add(kz);
    // mix(a,b,c)
    a = a.wrapping_sub(c);
    a ^= rot(c, 4);
    c = c.wrapping_add(b);
    b = b.wrapping_sub(a);
    b ^= rot(a, 6);
    a = a.wrapping_add(c);
    c = c.wrapping_sub(b);
    c ^= rot(b, 8);
    b = b.wrapping_add(a);
    a = a.wrapping_sub(c);
    a ^= rot(c, 16);
    c = c.wrapping_add(b);
    b = b.wrapping_sub(a);
    b ^= rot(a, 19);
    a = a.wrapping_add(c);
    c = c.wrapping_sub(b);
    c ^= rot(b, 4);
    b = b.wrapping_add(a);
    a = a.wrapping_add(kw);
    final_mix(a, b, c)
}
pub fn hash_to_float2(kx: u32, ky: u32) -> f32 {
    hash2(kx, ky) as f32 / 0xFFFFFFFFu32 as f32
}
pub fn hash_to_float3(kx: u32, ky: u32, kz: u32) -> f32 {
    hash3(kx, ky, kz) as f32 / 0xFFFFFFFFu32 as f32
}
