//! Blender geometry-node curve semantics (float32 where it matters): FloatCurve (CurveMapping), poly curves, fillet, resample, curve-to-mesh.
use crate::mesh::*;

// ---------------------------------------------------------------- FloatCurve (blenkernel colortools.cc)
const CM_RESOL: usize = 32;
const CM_TABLE: usize = 256;

#[derive(Clone, Copy, Default)]
struct Bez {
    v: [[f32; 3]; 3], // handle1, point, handle2
}

fn len3(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn handle_calc(bezt: &mut Bez, prev: Option<&Bez>, next: Option<&Bez>, vector: bool) {
    let p2 = bezt.v[1];
    let p1 = match prev {
        Some(p) => p.v[1],
        None => {
            let p3 = next.unwrap().v[1];
            [2.0 * p2[0] - p3[0], 2.0 * p2[1] - p3[1], 2.0 * p2[2] - p3[2]]
        }
    };
    let p3 = match next {
        Some(n) => n.v[1],
        None => [2.0 * p2[0] - p1[0], 2.0 * p2[1] - p1[1], 2.0 * p2[2] - p1[2]],
    };
    let dvec_a = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let dvec_b = [p3[0] - p2[0], p3[1] - p2[1], p3[2] - p2[2]];
    if vector {
        for k in 0..3 {
            bezt.v[0][k] = p2[k] - dvec_a[k] / 3.0;
            bezt.v[2][k] = p2[k] + dvec_b[k] / 3.0;
        }
        return;
    }
    let mut len_a = len3(dvec_a);
    let mut len_b = len3(dvec_b);
    if len_a == 0.0 {
        len_a = 1.0;
    }
    if len_b == 0.0 {
        len_b = 1.0;
    }
    let tvec = [dvec_b[0] / len_b + dvec_a[0] / len_a, dvec_b[1] / len_b + dvec_a[1] / len_a, dvec_b[2] / len_b + dvec_a[2] / len_a];
    let len = len3(tvec) * 2.5614f32;
    if len != 0.0 {
        // (the 5x handle-length clamp of F-Curves does not apply to curve mappings: measured against bpy)
        len_a /= len;
        for k in 0..3 {
            bezt.v[0][k] = p2[k] + tvec[k] * -len_a;
        }
        len_b /= len;
        for k in 0..3 {
            bezt.v[2][k] = p2[k] + tvec[k] * len_b;
        }
    }
}

fn forward_diff(q0: f32, q1: f32, q2: f32, q3: f32, it: usize, out: &mut [f32]) {
    let f = it as f32;
    let rt0 = q0;
    let rt1 = 3.0 * (q1 - q0) / f;
    let f2 = f * f;
    let rt2 = 3.0 * (q0 - 2.0 * q1 + q2) / f2;
    let f3 = f2 * f;
    let rt3 = (q3 - q0 + 3.0 * (q1 - q2)) / f3;
    let mut q0 = rt0;
    let mut q1 = rt1 + rt2 + rt3;
    let mut q2 = 2.0 * rt2 + 6.0 * rt3;
    let q3 = 6.0 * rt3;
    for a in 0..=it {
        out[a] = q0;
        q0 += q1;
        q1 += q2;
        q2 += q3;
    }
}

fn correct_bezpart(v1: [f32; 3], v2: &mut [f32; 3], v3: &mut [f32; 3], v4: [f32; 3]) {
    let h1 = [v1[0] - v2[0], v1[1] - v2[1]];
    let h2 = [v4[0] - v3[0], v4[1] - v3[1]];
    let len = v4[0] - v1[0];
    let len1 = h1[0].abs();
    let len2 = h2[0].abs();
    if len1 + len2 == 0.0 {
        return;
    }
    if len1 + len2 > len {
        let fac = len / (len1 + len2);
        v2[0] = v1[0] - fac * h1[0];
        v2[1] = v1[1] - fac * h1[1];
        v3[0] = v4[0] - fac * h2[0];
        v3[1] = v4[1] - fac * h2[1];
    }
}

/// Blender FloatCurve / CurveMapping with AUTO handles, evaluated through the 257-entry lookup table (clip rect [0,1])
#[derive(Clone)]
pub struct FloatCurve {
    table: Vec<[f32; 2]>,
    mintable: f32,
    range: f32,
    ext_in: [f32; 2],
    ext_out: [f32; 2],
    extrapolate: bool,
}

impl FloatCurve {
    pub fn new(pts: &[(f64, f64)]) -> FloatCurve {
        FloatCurve::new_ex(pts, &[], 0.0, 1.0)
    }

    /// `vector[i]` = point i has a VECTOR handle; clip rectangle x range [xmin, xmax]
    pub fn new_ex(pts: &[(f64, f64)], vector: &[bool], xmin: f32, xmax: f32) -> FloatCurve {
        FloatCurve::new_full(pts, vector, xmin, xmax, true)
    }

    /// `extrapolate`: CUMA_EXTEND_EXTRAPOLATE (false = horizontal extension)
    pub fn new_full(pts: &[(f64, f64)], vector: &[bool], xmin: f32, xmax: f32, extrapolate: bool) -> FloatCurve {
        let n = pts.len();
        let table_range = xmax - xmin;
        let mut mintable = xmin;
        let mut maxtable = xmax;
        let mut bezt = vec![Bez::default(); n];
        for a in 0..n {
            let (x, y) = (pts[a].0 as f32, pts[a].1 as f32);
            mintable = mintable.min(x);
            maxtable = maxtable.max(x);
            bezt[a].v[1] = [x, y, 0.0];
        }
        for a in 0..n {
            let prev = if a > 0 { Some(bezt[a - 1]) } else { None };
            let next = if a + 1 < n { Some(bezt[a + 1]) } else { None };
            let mut b = bezt[a];
            handle_calc(&mut b, prev.as_ref(), next.as_ref(), vector.get(a).copied().unwrap_or(false));
            bezt[a] = b;
        }
        if n > 2 && !vector.get(0).copied().unwrap_or(false) {
            let hlen = len3([bezt[0].v[1][0] - bezt[0].v[2][0], bezt[0].v[1][1] - bezt[0].v[2][1], bezt[0].v[1][2] - bezt[0].v[2][2]]);
            let mut vec = bezt[1].v[0];
            if vec[0] < bezt[0].v[1][0] {
                vec[0] = bezt[0].v[1][0];
            }
            for k in 0..3 {
                vec[k] -= bezt[0].v[1][k];
            }
            let nlen = len3(vec);
            if nlen > f32::EPSILON {
                for k in 0..3 {
                    vec[k] *= hlen / nlen;
                }
                for k in 0..3 {
                    bezt[0].v[2][k] = vec[k] + bezt[0].v[1][k];
                    bezt[0].v[0][k] = bezt[0].v[1][k] - vec[k];
                }
            }
        }
        if n > 2 && !vector.get(n - 1).copied().unwrap_or(false) {
            let a = n - 1;
            let hlen = len3([bezt[a].v[1][0] - bezt[a].v[0][0], bezt[a].v[1][1] - bezt[a].v[0][1], bezt[a].v[1][2] - bezt[a].v[0][2]]);
            let mut vec = bezt[a - 1].v[2];
            if vec[0] > bezt[a].v[1][0] {
                vec[0] = bezt[a].v[1][0];
            }
            for k in 0..3 {
                vec[k] -= bezt[a].v[1][k];
            }
            let nlen = len3(vec);
            if nlen > f32::EPSILON {
                for k in 0..3 {
                    vec[k] *= hlen / nlen;
                }
                for k in 0..3 {
                    bezt[a].v[0][k] = vec[k] + bezt[a].v[1][k];
                    bezt[a].v[2][k] = bezt[a].v[1][k] - vec[k];
                }
            }
        }
        let totpoint = (n - 1) * CM_RESOL;
        let mut all = vec![[0.0f32; 2]; totpoint];
        for a in 0..n - 1 {
            let (mut h2, mut h3) = (bezt[a].v[2], bezt[a + 1].v[0]);
            correct_bezpart(bezt[a].v[1], &mut h2, &mut h3, bezt[a + 1].v[1]);
            let (mut xs, mut ys) = ([0.0f32; CM_RESOL], [0.0f32; CM_RESOL]);
            forward_diff(bezt[a].v[1][0], h2[0], h3[0], bezt[a + 1].v[1][0], CM_RESOL - 1, &mut xs);
            forward_diff(bezt[a].v[1][1], h2[1], h3[1], bezt[a + 1].v[1][1], CM_RESOL - 1, &mut ys);
            for k in 0..CM_RESOL {
                all[a * CM_RESOL + k] = [xs[k], ys[k]];
            }
        }
        let mut ext_in = [bezt[0].v[0][0] - bezt[0].v[1][0], bezt[0].v[0][1] - bezt[0].v[1][1]];
        let r = (ext_in[0] * ext_in[0] + ext_in[1] * ext_in[1]).sqrt();
        ext_in = [ext_in[0] / r, ext_in[1] / r];
        let a = n - 1;
        let mut ext_out = [bezt[a].v[1][0] - bezt[a].v[2][0], bezt[a].v[1][1] - bezt[a].v[2][1]];
        let r = (ext_out[0] * ext_out[0] + ext_out[1] * ext_out[1]).sqrt();
        ext_out = [ext_out[0] / r, ext_out[1] / r];
        // curvemap_make_table: CM_TABLE equal x distances, linear between the forward-differenced points
        let range = (1.0f32 / 256.0f32) * table_range;
        let calc_extend = |x: f32, first: [f32; 2], last: [f32; 2]| -> f32 {
            if x <= first[0] {
                if !extrapolate {
                    return first[1];
                }
                if ext_in[0] == 0.0 {
                    return first[1] + ext_in[1] * 10000.0;
                }
                return first[1] + ext_in[1] * (x - first[0]) / ext_in[0];
            }
            if x >= last[0] {
                if !extrapolate {
                    return last[1];
                }
                if ext_out[0] == 0.0 {
                    return last[1] - ext_out[1] * 10000.0;
                }
                return last[1] + ext_out[1] * (x - last[0]) / ext_out[0];
            }
            0.0
        };
        let first = all[0];
        let last = all[totpoint - 1];
        let mut table = vec![[0.0f32; 2]; CM_TABLE + 1];
        let mut pi = 0usize;
        for a in 0..=CM_TABLE {
            let cur_x = mintable + range * (a as f32);
            while cur_x >= all[pi][0] && pi != totpoint - 1 {
                pi += 1;
            }
            let y = if pi == 0 || (pi == totpoint - 1 && cur_x >= all[pi][0]) {
                if (cur_x - all[pi][0]).abs() <= 1e-6 {
                    all[pi][1]
                } else {
                    calc_extend(cur_x, first, last)
                }
            } else {
                let mut fac1 = all[pi][0] - all[pi - 1][0];
                let fac2 = all[pi][0] - cur_x;
                if fac1 > f32::EPSILON {
                    fac1 = fac2 / fac1;
                } else {
                    fac1 = 0.0;
                }
                fac1 * all[pi - 1][1] + (1.0 - fac1) * all[pi][1]
            };
            table[a] = [cur_x, y];
        }
        FloatCurve { table, mintable, range: 1.0 / range, ext_in, ext_out, extrapolate }
    }

    /// value as the FloatCurve node returns it: the mapping's clip rectangle limits y to [0, 1]
    pub fn eval_node(&self, x: f64) -> f64 {
        self.eval(x).clamp(0.0, 1.0)
    }

    pub fn eval(&self, x: f64) -> f64 {
        let value = x as f32;
        let mut fi = (value - self.mintable) * self.range;
        let i = fi as i32;
        if fi < 0.0 || fi > CM_TABLE as f32 {
            let (first, last) = (self.table[0], self.table[CM_TABLE]);
            if value <= first[0] {
                if !self.extrapolate {
                    return first[1] as f64;
                }
                return if self.ext_in[0] == 0.0 { (first[1] + self.ext_in[1] * 10000.0) as f64 } else { (first[1] + self.ext_in[1] * (value - first[0]) / self.ext_in[0]) as f64 };
            }
            if !self.extrapolate {
                return last[1] as f64;
            }
            return if self.ext_out[0] == 0.0 { (last[1] - self.ext_out[1] * 10000.0) as f64 } else { (last[1] + self.ext_out[1] * (value - last[0]) / self.ext_out[0]) as f64 };
        }
        if i < 0 {
            return self.table[0][1] as f64;
        }
        if i as usize >= CM_TABLE {
            return self.table[CM_TABLE][1] as f64;
        }
        fi -= i as f32;
        ((1.0 - fi) * self.table[i as usize][1] + fi * self.table[i as usize + 1][1]) as f64
    }
}

/// Blender 4.2 builds the Transform Geometry matrix through a quaternion: euler -> quat (float) -> mat3
pub fn trs_rot(eul: [f32; 3]) -> [[f32; 3]; 3] {
    let (ti, tj, th) = (eul[0] * 0.5f32, eul[1] * 0.5f32, eul[2] * 0.5f32);
    let (ci, cj, ch) = (ti.cos(), tj.cos(), th.cos());
    let (si, sj, sh) = (ti.sin(), tj.sin(), th.sin());
    let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
    let q = [cj * cc + sj * ss, cj * sc - sj * cs, cj * ss + sj * cc, cj * cs - sj * sc];
    crate::bl32::quat_to_mat3(q)
}

/// Transform Geometry on mesh positions in float32 (see Curve::transform_trs)
pub fn mesh_trs(m: &mut Mesh, loc: [f32; 3], eul: [f32; 3], scale: [f32; 3]) {
    let r = trs_rot(eul);
    let mm = [
        [r[0][0] * scale[0], r[0][1] * scale[0], r[0][2] * scale[0]],
        [r[1][0] * scale[1], r[1][1] * scale[1], r[1][2] * scale[1]],
        [r[2][0] * scale[2], r[2][1] * scale[2], r[2][2] * scale[2]],
    ];
    for p in m.pos.iter_mut() {
        let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let o = [
            mm[0][0] * x + mm[1][0] * y + mm[2][0] * z + loc[0],
            mm[0][1] * x + mm[1][1] * y + mm[2][1] * z + loc[1],
            mm[0][2] * x + mm[1][2] * y + mm[2][2] * z + loc[2],
        ];
        *p = [o[0] as f64, o[1] as f64, o[2] as f64];
    }
}

// ---------------------------------------------------------------- poly curves
#[derive(Clone, Debug, Default)]
pub struct Curve {
    pub pts: Vec<V3>,
    pub cyclic: bool,
    pub tilt: Vec<f64>,
}

fn vlen(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn vnorm(a: V3) -> V3 {
    let l = vlen(a);
    if l == 0.0 {
        a
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}
fn vadd(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn vmul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn lerp3(a: V3, b: V3, t: f64) -> V3 {
    [a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t, a[2] * (1.0 - t) + b[2] * t]
}

impl Curve {
    pub fn new(pts: Vec<V3>, cyclic: bool) -> Curve {
        let n = pts.len();
        Curve { pts, cyclic, tilt: vec![0.0; n] }
    }
    pub fn line(a: V3, b: V3) -> Curve {
        Curve::new(vec![a, b], false)
    }
    /// GeometryNodeCurvePrimitiveCircle (radius mode): points at 2*pi*i/n
    pub fn circle(n: usize, radius: f64) -> Curve {
        let step = (2.0f32 * std::f32::consts::PI) / n as f32;
        Curve::new((0..n).map(|i| { let th = (step * i as f32) as f64; [((radius * th.cos()) as f32) as f64, ((radius * th.sin()) as f32) as f64, 0.0] }).collect(), true)
    }
    /// GeometryNodeCurveArc (radius mode, not connected)
    pub fn arc(res: usize, radius: f64, start: f64, sweep: f64) -> Curve {
        let step = sweep / (res - 1) as f64;
        Curve::new((0..res).map(|i| { let t = step * i as f64 + start; [radius * t.cos(), radius * t.sin(), 0.0] }).collect(), false)
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in self.pts.iter_mut() {
            *p = m.apply(*p);
        }
    }
    /// Transform Geometry node (components mode) in float32: loc/euler/scale -> matrix, points = M * p
    pub fn transform_trs(&mut self, loc: [f32; 3], eul: [f32; 3], scale: [f32; 3]) {
        let r = trs_rot(eul);
        // columns of the rotation scaled per axis, translation in the last column
        let m = [
            [r[0][0] * scale[0], r[0][1] * scale[0], r[0][2] * scale[0]],
            [r[1][0] * scale[1], r[1][1] * scale[1], r[1][2] * scale[1]],
            [r[2][0] * scale[2], r[2][1] * scale[2], r[2][2] * scale[2]],
        ];
        for p in self.pts.iter_mut() {
            let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
            let o = [
                m[0][0] * x + m[1][0] * y + m[2][0] * z + loc[0],
                m[0][1] * x + m[1][1] * y + m[2][1] * z + loc[1],
                m[0][2] * x + m[1][2] * y + m[2][2] * z + loc[2],
            ];
            *p = [o[0] as f64, o[1] as f64, o[2] as f64];
        }
    }
    pub fn set_tilt(&mut self, t: f64) {
        self.tilt = vec![t; self.pts.len()];
    }
    fn nseg(&self) -> usize {
        if self.cyclic { self.pts.len() } else { self.pts.len() - 1 }
    }
    /// accumulated evaluated lengths per segment end (float32 like Blender)
    fn acc_lengths(&self) -> Vec<f64> {
        let n = self.pts.len();
        let mut acc = vec![];
        let mut s = 0.0f32;
        for i in 0..self.nseg() {
            let a = self.pts[i];
            let b = self.pts[(i + 1) % n];
            let d = [b[0] as f32 - a[0] as f32, b[1] as f32 - a[1] as f32, b[2] as f32 - a[2] as f32];
            s += (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            acc.push(s as f64);
        }
        acc
    }
    pub fn total_length(&self) -> f64 {
        *self.acc_lengths().last().unwrap_or(&0.0)
    }
    /// position/tilt at accumulated length
    fn at_length(&self, acc: &[f64], len: f64) -> (V3, f64) {
        let n = self.pts.len();
        let mut seg = acc.partition_point(|&l| l <= len);
        if seg >= acc.len() {
            seg = acc.len() - 1;
        }
        let start = if seg == 0 { 0.0 } else { acc[seg - 1] };
        let f = if acc[seg] > start { ((len - start) / (acc[seg] - start)).clamp(0.0, 1.0) } else { 0.0 };
        let (i0, i1) = (seg, (seg + 1) % n);
        (lerp3(self.pts[i0], self.pts[i1], f), self.tilt[i0] * (1.0 - f) + self.tilt[i1] * f)
    }
    /// ResampleCurve (count mode), float32 arithmetic
    pub fn resample(&self, count: usize) -> Curve {
        let acc: Vec<f32> = self.acc_lengths().iter().map(|&x| x as f32).collect();
        let total = *acc.last().unwrap();
        let sample_len = total / if self.cyclic { count as f32 } else { (count - 1) as f32 };
        let n = self.pts.len();
        let mut pts = vec![];
        let mut tilt = vec![];
        let mut prev = 0.0f32;
        let mut i_dst = 0usize;
        let lerp = |a: f64, b: f64, f: f32| -> f64 { ((a as f32) * (1.0 - f) + (b as f32) * f) as f64 };
        for i_src in 0..acc.len() {
            let length = acc[i_src];
            while i_dst < count && sample_len * (i_dst as f32) < length {
                let f = (sample_len * i_dst as f32 - prev) * (1.0f32 / (length - prev));
                let (i0, i1) = (i_src, (i_src + 1) % n);
                pts.push([lerp(self.pts[i0][0], self.pts[i1][0], f), lerp(self.pts[i0][1], self.pts[i1][1], f), lerp(self.pts[i0][2], self.pts[i1][2], f)]);
                tilt.push(lerp(self.tilt[i0], self.tilt[i1], f));
                i_dst += 1;
            }
            prev = length;
        }
        if !self.cyclic {
            while pts.len() < count {
                pts.push(*self.pts.last().unwrap());
                tilt.push(*self.tilt.last().unwrap());
            }
            *pts.last_mut().unwrap() = *self.pts.last().unwrap();
            *tilt.last_mut().unwrap() = *self.tilt.last().unwrap();
        }
        Curve { pts, cyclic: self.cyclic, tilt }
    }
    /// SampleCurve in factor mode (single curve)
    pub fn sample_factor(&self, factor: f64) -> V3 {
        let acc = self.acc_lengths();
        let total = *acc.last().unwrap();
        self.at_length(&acc, factor.clamp(0.0, 1.0) * total).0
    }
    /// GeometryNodeFilletCurve, poly mode, uniform radius, limit radius on
    pub fn fillet_poly(&self, radius: f64, count: usize) -> Curve {
        let n = self.pts.len();
        if radius <= 0.0 || n < 3 {
            return self.clone();
        }
        // directions and interior angles
        let seg = |i: usize| -> (V3, f64) {
            let d = sub(self.pts[(i + 1) % n], self.pts[i]);
            (vnorm(d), vlen(d))
        };
        let mut disp = vec![0.0; n];
        let mut angle = vec![0.0; n];
        let mut has = vec![false; n];
        for i in 0..n {
            if !self.cyclic && (i == 0 || i == n - 1) {
                continue;
            }
            let ip = (i + n - 1) % n;
            let (dp, _) = seg(ip);
            let (dn, _) = seg(i);
            let back = vmul(dp, -1.0);
            let c = dot(back, dn).clamp(-1.0, 1.0);
            let a = c.acos();
            angle[i] = a;
            if a < 1e-4 || (std::f64::consts::PI - a) < 1e-4 {
                continue;
            }
            has[i] = true;
            disp[i] = radius / (a / 2.0).tan();
        }
        // limit: each segment is shared by its two end corners
        let lim = disp.clone();
        for i in 0..n {
            if !has[i] {
                continue;
            }
            let ip = (i + n - 1) % n;
            let (_, lp) = seg(ip);
            let (_, ln) = seg(i);
            let share = |j: usize, l: f64| -> f64 {
                // fraction of segment available: half if the other end also has a fillet, whole otherwise
                if has[j] { l / 2.0 } else { l }
            };
            let m = share(ip, lp).min(share((i + 1) % n, ln));
            disp[i] = lim[i].min(m);
        }
        let mut out = vec![];
        let mut tilt = vec![];
        for i in 0..n {
            if !has[i] {
                out.push(self.pts[i]);
                tilt.push(self.tilt[i]);
                continue;
            }
            let ip = (i + n - 1) % n;
            let (dp, _) = seg(ip);
            let (dn, _) = seg(i);
            let b = self.pts[i];
            let back = vmul(dp, -1.0);
            let d = disp[i];
            let t0 = vadd(b, vmul(back, d));
            let t1 = vadd(b, vmul(dn, d));
            let a = angle[i];
            let r = d * (a / 2.0).tan();
            let bis = vnorm(vadd(back, dn));
            let cen = vadd(b, vmul(bis, r / (a / 2.0).sin()));
            let (v0, v1) = (sub(t0, cen), sub(t1, cen));
            let axis = vnorm(cross(v0, v1));
            let sweep = vlen(cross(v0, v1)).atan2(dot(v0, v1));
            for k in 0..=count {
                let th = sweep * k as f64 / count as f64;
                // rotate v0 around axis by th
                let (s, c) = th.sin_cos();
                let ax = vmul(axis, dot(v0, axis));
                let df = sub(v0, ax);
                let cr = cross(axis, df);
                let v = vadd(vadd(ax, vmul(df, c)), vmul(cr, s));
                out.push(vadd(cen, v));
                tilt.push(self.tilt[i]);
            }
        }
        Curve { pts: out, cyclic: self.cyclic, tilt }
    }

    pub fn tangents(&self) -> Vec<V3> {
        let n = self.pts.len();
        if n == 1 {
            return vec![[0.0, 0.0, 1.0]];
        }
        let nseg = self.nseg();
        let segd: Vec<V3> = (0..nseg).map(|i| vnorm(sub(self.pts[(i + 1) % n], self.pts[i]))).collect();
        (0..n)
            .map(|i| {
                if self.cyclic {
                    vnorm(vadd(segd[(i + n - 1) % n], segd[i]))
                } else if i == 0 {
                    segd[0]
                } else if i == n - 1 {
                    segd[n - 2]
                } else {
                    vnorm(vadd(segd[i - 1], segd[i]))
                }
            })
            .collect()
    }
    /// minimum-twist normals including tilt
    pub fn normals(&self, tan: &[V3]) -> Vec<V3> {
        let n = self.pts.len();
        let t0 = tan[0];
        let mut n0 = vnorm([t0[1], -t0[0], 0.0]);
        if t0[0].abs() + t0[1].abs() < 1e-5 {
            n0 = [1.0, 0.0, 0.0];
        }
        let rot = |d: V3, ax: V3, ang: f64| -> V3 {
            let a = vmul(ax, dot(d, ax));
            let df = sub(d, a);
            let cr = cross(ax, df);
            vadd(vadd(a, vmul(df, ang.cos())), vmul(cr, ang.sin()))
        };
        let mut nor = vec![n0];
        for i in 1..n {
            let (lt, ct) = (tan[i - 1], tan[i]);
            let ang = dot(lt, ct).clamp(-1.0, 1.0).acos();
            let nn = if ang != 0.0 && vlen(lt) > 0.0 && vlen(ct) > 0.0 { rot(nor[i - 1], vnorm(cross(lt, ct)), ang) } else { nor[i - 1] };
            nor.push(nn);
        }
        if self.cyclic && n > 1 {
            let (lt, ft) = (tan[n - 1], tan[0]);
            let ang = dot(lt, ft).clamp(-1.0, 1.0).acos();
            let unc = if ang != 0.0 { rot(nor[n - 1], vnorm(cross(lt, ft)), ang) } else { nor[n - 1] };
            // signed angle from unc to first normal about first tangent
            let (a, b) = (unc, nor[0]);
            let mut corr = dot(cross(a, b), ft).atan2(dot(a, b));
            if corr > std::f64::consts::PI {
                corr -= 2.0 * std::f64::consts::PI;
            }
            {
                for i in 1..n {
                    nor[i] = rot(nor[i], tan[i], corr * (i as f64 / n as f64));
                }
            }
        }
        (0..n).map(|i| if self.tilt[i] != 0.0 { rot(nor[i], tan[i], self.tilt[i]) } else { nor[i] }).collect()
    }
}

/// CurveToMesh: sweep `prof` (x -> normal, y -> tangent x normal, z -> tangent) along `main`. Vertex index = ring*profile_len + j.
pub fn curve_to_mesh(main: &Curve, prof: &Curve, fill_caps: bool) -> Mesh {
    let tan = main.tangents();
    let nor = main.normals(&tan);
    let (nm, np) = (main.pts.len(), prof.pts.len());
    let mut m = Mesh::new();
    for i in 0..nm {
        let b = cross(tan[i], nor[i]);
        for p in &prof.pts {
            let o = [nor[i][0] * p[0] + b[0] * p[1] + tan[i][0] * p[2], nor[i][1] * p[0] + b[1] * p[1] + tan[i][1] * p[2], nor[i][2] * p[0] + b[2] * p[1] + tan[i][2] * p[2]];
            m.pos.push(vadd(main.pts[i], o));
        }
    }
    let rings = if main.cyclic { nm } else { nm - 1 };
    let segs = if prof.cyclic { np } else { np - 1 };
    for i in 0..rings {
        let i1 = (i + 1) % nm;
        for j in 0..segs {
            let j1 = (j + 1) % np;
            if prof.cyclic {
                m.add_face_idx(&[(i * np + j) as u32, (i * np + j1) as u32, (i1 * np + j1) as u32, (i1 * np + j) as u32]);
            } else {
                m.add_face_idx(&[(i * np + j) as u32, (i1 * np + j) as u32, (i1 * np + j1) as u32, (i * np + j1) as u32]);
            }
        }
    }
    if fill_caps && prof.cyclic && !main.cyclic {
        m.add_face_idx(&(0..np as u32).rev().collect::<Vec<_>>());
        let base = ((nm - 1) * np) as u32;
        m.add_face_idx(&(0..np as u32).map(|j| base + j).collect::<Vec<_>>());
    }
    m
}


// ---------------------------------------------------------------- spline evaluation
/// Catmull-Rom spline (open) evaluated to a poly curve with `res` points per segment (Blender default resolution 12)
pub fn catmull_rom_eval(ctrl: &[V3], res: usize) -> Curve {
    let n = ctrl.len();
    let mut out = vec![];
    for i in 0..n - 1 {
        let a = ctrl[if i == 0 { 0 } else { i - 1 }];
        let b = ctrl[i];
        let c = ctrl[i + 1];
        let d = ctrl[(i + 2).min(n - 1)];
        for j in 0..res {
            let t = j as f32 * (1.0f32 / res as f32);
            let sm = 1.0f32 - t;
            let n0 = -t * sm * sm;
            let n1 = 2.0f32 + t * t * (3.0f32 * t - 5.0f32);
            let n2 = 2.0f32 + sm * sm * (3.0f32 * sm - 5.0f32);
            let n3 = -sm * t * t;
            let mut p = [0.0; 3];
            for k in 0..3 {
                p[k] = (0.5f32 * ((a[k] as f32) * n0 + (b[k] as f32) * n1 + (c[k] as f32) * n2 + (d[k] as f32) * n3)) as f64;
            }
            out.push(p);
        }
    }
    out.push(ctrl[n - 1]);
    Curve::new(out, false)
}

/// Bezier segment (Position mode handles) evaluated with Blender's forward differencing in float32; `res` points on the segment (+ end point)
pub fn bezier_segment_eval(p0: V3, h0: V3, h1: V3, p1: V3, res: usize) -> Curve {
    let f = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32];
    let (p0, h0, h1, p1) = (f(p0), f(h0), f(h1), f(p1));
    let inv = 1.0f32 / res as f32;
    let inv2 = inv * inv;
    let inv3 = inv2 * inv;
    let mut pts = vec![];
    let mut q0 = p0;
    let mut rt1 = [0.0f32; 3];
    let mut rt2 = [0.0f32; 3];
    let mut rt3 = [0.0f32; 3];
    for k in 0..3 {
        rt1[k] = 3.0 * (h0[k] - p0[k]) * inv;
        rt2[k] = 3.0 * (p0[k] - 2.0 * h0[k] + h1[k]) * inv2;
        rt3[k] = (p1[k] - p0[k] + 3.0 * (h0[k] - h1[k])) * inv3;
    }
    let mut q1 = [0.0f32; 3];
    let mut q2 = [0.0f32; 3];
    let mut q3 = [0.0f32; 3];
    for k in 0..3 {
        q1[k] = rt1[k] + rt2[k] + rt3[k];
        q2[k] = 2.0 * rt2[k] + 6.0 * rt3[k];
        q3[k] = 6.0 * rt3[k];
    }
    for _ in 0..res {
        pts.push([q0[0] as f64, q0[1] as f64, q0[2] as f64]);
        for k in 0..3 {
            q0[k] += q1[k];
            q1[k] += q2[k];
            q2[k] += q3[k];
        }
    }
    pts.push([p1[0] as f64, p1[1] as f64, p1[2] as f64]);
    Curve::new(pts, false)
}
