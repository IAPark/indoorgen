//! Float32 ports of Blender 4.2 modifiers on plain position arrays: SIMPLE_DEFORM (bend / twist), DISPLACE (axis directions, vertex group, legacy texture).
use super::legtex::{vert_normals, Tex};

type F3 = [f32; 3];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Deform {
    Bend,
    Twist,
}

/// SimpleDeformModifier (local space, limits 0..1, no lock axes). `axis` 0/1/2 = X/Y/Z, `factor` = the modifier's angle.
pub fn simple_deform(v: &mut [F3], method: Deform, angle: f64, axis: usize) {
    let factor_in = angle as f32;
    let limit_axis = if method == Deform::Bend {
        match axis {
            0 | 1 => 2,
            _ => 0,
        }
    } else {
        axis
    };
    let mut lower = f32::MAX;
    let mut upper = -f32::MAX;
    for p in v.iter() {
        lower = lower.min(p[limit_axis]);
        upper = upper.max(p[limit_axis]);
    }
    let lim1 = lower + (upper - lower) * 1.0f32;
    let lim0 = lower + (upper - lower) * 0.0f32;
    let factor = factor_in / f32::EPSILON.max(lim1 - lim0);
    if method == Deform::Bend && factor.abs() < 0.000001 {
        return;
    }
    const AXIS_MAP: [[usize; 3]; 3] = [[1, 2, 0], [2, 0, 1], [0, 1, 2]];
    let map = AXIS_MAP[if method != Deform::Bend { axis } else { 2 }];
    for p in v.iter_mut() {
        let mut co = *p;
        let mut dcut = [0f32; 3];
        let val = co[limit_axis].max(lim0).min(lim1);
        // axis_limit: if (limits[0] > val) val = limits[0]; if (limits[1] < val) val = limits[1]
        let mut vv = co[limit_axis];
        if lim0 > vv {
            vv = lim0;
        }
        if lim1 < vv {
            vv = lim1;
        }
        let _ = val;
        dcut[limit_axis] = co[limit_axis] - vv;
        co[limit_axis] = vv;
        let mut c = [co[map[0]], co[map[1]], co[map[2]]];
        let d = [dcut[map[0]], dcut[map[1]], dcut[map[2]]];
        match method {
            Deform::Twist => {
                let (x, y, z) = (c[0], c[1], c[2]);
                let theta = z * factor;
                let (s, co_) = (theta.sin(), theta.cos());
                c[0] = x * co_ - y * s;
                c[1] = x * s + y * co_;
                c[2] = z;
                c[0] += d[0];
                c[1] += d[1];
                c[2] += d[2];
            }
            Deform::Bend => {
                let (x, y, z) = (c[0], c[1], c[2]);
                let theta = match axis {
                    0 | 1 => z * factor,
                    _ => x * factor,
                };
                let (s, co_) = (theta.sin(), theta.cos());
                match axis {
                    0 => {
                        c[0] = x;
                        c[1] = y * co_ + (1.0 - co_) / factor;
                        c[2] = -(y - 1.0 / factor) * s;
                        c[0] += d[0];
                        c[1] += s * d[2];
                        c[2] += co_ * d[2];
                    }
                    1 => {
                        c[0] = x * co_ + (1.0 - co_) / factor;
                        c[1] = y;
                        c[2] = -(x - 1.0 / factor) * s;
                        c[0] += s * d[2];
                        c[1] += d[1];
                        c[2] += co_ * d[2];
                    }
                    _ => {
                        c[0] = -(y - 1.0 / factor) * s;
                        c[1] = y * co_ + (1.0 - co_) / factor;
                        c[2] = z;
                        c[0] += co_ * d[0];
                        c[1] += s * d[0];
                        c[2] += d[2];
                    }
                }
            }
        }
        let mut out = [0f32; 3];
        out[map[0]] = c[0];
        out[map[1]] = c[1];
        out[map[2]] = c[2];
        // interp_v3_v3v3(cos, cos, co, weight = 1): a + (b - a) * 1
        for k in 0..3 {
            p[k] = p[k] + (out[k] - p[k]) * 1.0;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DispDir {
    X,
    Y,
    Z,
    Normal,
}

/// DISPLACE modifier with a legacy texture (local coordinates); `vg` = vertex group weights (None = no group), `faces` needed for NORMAL.
pub fn displace(v: &mut [F3], faces: &[Vec<u32>], tex: &Tex, strength: f64, midlevel: f64, dir: DispDir, vg: Option<&[f32]>) {
    if strength == 0.0 {
        return;
    }
    let strength = strength as f32;
    let mid = midlevel as f32;
    let nrm = if dir == DispDir::Normal {
        let m = crate::mesh::Mesh { pos: v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: faces.to_vec(), uv: vec![] };
        vert_normals(&m)
    } else {
        vec![]
    };
    let tex_co: Vec<F3> = v.to_vec();
    for (i, p) in v.iter_mut().enumerate() {
        let mut s = strength;
        if let Some(w) = vg {
            if w[i] == 0.0 {
                continue;
            }
            s *= w[i];
        }
        let t = tex.value(tex_co[i][0], tex_co[i][1], tex_co[i][2]);
        let mut delta = (t - mid) * s;
        delta = delta.max(-10000.0).min(10000.0);
        match dir {
            DispDir::X => p[0] += delta,
            DispDir::Y => p[1] += delta,
            DispDir::Z => p[2] += delta,
            DispDir::Normal => {
                for k in 0..3 {
                    p[k] += nrm[i][k] * delta;
                }
            }
        }
    }
}
