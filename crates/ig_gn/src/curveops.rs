//! Curve evaluation helpers: evaluated points, lengths, tangents, normals (Blender 4.2 curves_geometry semantics).
use super::geom::*;
use super::math::*;
use super::val::*;
use crate::gnlite as g32;

/// evaluated representation of one curve
pub struct EvalCurve {
    pub pos: Vec<V3>,
    /// evaluated index of every control point (len = ncontrol), for Poly identity
    pub ctrl_at: Vec<usize>,
    /// per evaluated point: (control index a, factor to next control b = a+1 (mod n))
    pub interp: Vec<(usize, f32)>,
    /// catmull-rom: per evaluated point the 4 control indices and weights
    pub cr: Option<Vec<(Vec<usize>, Vec<f32>)>>,
}

pub fn is_vector_handle(t: u8) -> bool {
    t == 2
}

/// number of evaluated points of a bezier segment from control point i to i+1
pub fn bezier_seg_res(c: &Curves, k: usize, i: usize, j: usize) -> usize {
    let r = c.range(k);
    if is_vector_handle(c.htr[r.start + i]) && is_vector_handle(c.htl[r.start + j]) {
        1
    } else {
        c.res[k].max(1) as usize
    }
}

pub fn eval_curve(c: &Curves, k: usize) -> EvalCurve {
    let r = c.range(k);
    let n = r.len();
    let cyc = c.cyclic[k];
    match c.kind[k] {
        CKind::Bezier if n > 1 => {
            let nseg = if cyc { n } else { n - 1 };
            let mut pos = vec![];
            let mut ctrl_at = vec![];
            let mut interp = vec![];
            for i in 0..nseg {
                let j = (i + 1) % n;
                let res = bezier_seg_res(c, k, i, j);
                let (p0, h0, h1, p1) = (c.pos[r.start + i], c.hr[r.start + i], c.hl[r.start + j], c.pos[r.start + j]);
                let pts = bezier_segment_eval(p0, h0, h1, p1, res);
                ctrl_at.push(pos.len());
                for (s, p) in pts.iter().take(res).enumerate() {
                    pos.push(*p);
                    interp.push((i, s as f32 / res as f32));
                }
            }
            if !cyc {
                ctrl_at.push(pos.len());
                pos.push(c.pos[r.end - 1]);
                interp.push((n - 1, 0.0));
            }
            EvalCurve { pos, ctrl_at, interp, cr: None }
        }
        CKind::CatmullRom if n > 1 => {
            let res = c.res[k].max(1) as usize;
            let nseg = if cyc { n } else { n - 1 };
            let ctrl: Vec<V3> = c.pos[r.clone()].to_vec();
            let at = |i: i64| -> V3 {
                if cyc {
                    ctrl[i.rem_euclid(n as i64) as usize]
                } else {
                    ctrl[i.clamp(0, n as i64 - 1) as usize]
                }
            };
            let mut pos = vec![];
            let mut ctrl_at = vec![];
            let mut interp = vec![];
            let mut crw: Vec<(Vec<usize>, Vec<f32>)> = vec![];
            let idx_of = |i: i64| -> usize {
                if cyc {
                    i.rem_euclid(n as i64) as usize
                } else {
                    i.clamp(0, n as i64 - 1) as usize
                }
            };
            for i in 0..nseg {
                let (a, b, cc, d) = (at(i as i64 - 1), at(i as i64), at(i as i64 + 1), at(i as i64 + 2));
                let ids = vec![idx_of(i as i64 - 1), idx_of(i as i64), idx_of(i as i64 + 1), idx_of(i as i64 + 2)];
                ctrl_at.push(pos.len());
                for j in 0..res {
                    let t = j as f32 * (1.0f32 / res as f32);
                    let sm = 1.0f32 - t;
                    let n0 = -t * sm * sm;
                    let n1 = 2.0f32 + t * t * (3.0f32 * t - 5.0f32);
                    let n2 = 2.0f32 + sm * sm * (3.0f32 * sm - 5.0f32);
                    let n3 = -sm * t * t;
                    let mut p = [0.0f32; 3];
                    for q in 0..3 {
                        p[q] = 0.5f32 * (a[q] * n0 + b[q] * n1 + cc[q] * n2 + d[q] * n3);
                    }
                    pos.push(p);
                    interp.push((i, j as f32 / res as f32));
                    crw.push((ids.clone(), vec![0.5 * n0, 0.5 * n1, 0.5 * n2, 0.5 * n3]));
                }
            }
            if !cyc {
                ctrl_at.push(pos.len());
                pos.push(ctrl[n - 1]);
                interp.push((n - 1, 0.0));
                crw.push((vec![n - 1; 4], vec![0.0, 1.0, 0.0, 0.0]));
            }
            EvalCurve { pos, ctrl_at, interp, cr: Some(crw) }
        }
        CKind::Nurbs if n > 1 => {
            let order = (c.nurbs_order(k)).min(n);
            let res = c.res[k].max(1) as usize;
            let nseg = if cyc { n } else { n - 1 };
            let neval = res * nseg;
            let ctrl: Vec<V3> = c.pos[r.clone()].to_vec();
            // knots: cyclic = uniform (periodic); open = clamped (endpoint) knots
            let nk = if cyc { n + 2 * order - 1 } else { n + order };
            let knots: Vec<f32> = if cyc {
                (0..nk).map(|i| i as f32).collect()
            } else {
                (0..nk)
                    .map(|i| {
                        if i < order {
                            0.0
                        } else if i >= n {
                            (n - order + 1) as f32
                        } else {
                            (i - order + 1) as f32
                        }
                    })
                    .collect()
            };
            let (u0, u1) = (knots[order - 1], if cyc { knots[n + order - 1] } else { knots[n] });
            let step = (u1 - u0) / (neval as f32 - if cyc { 0.0 } else { 1.0 }).max(1.0);
            let mut pos = vec![];
            let mut crw: Vec<(Vec<usize>, Vec<f32>)> = vec![];
            for i in 0..neval {
                let u = u0 + step * i as f32;
                let lo = order - 1;
                let hi = if cyc { n + order - 2 } else { n - 1 };
                // span: knots[span] <= u < knots[span+1]
                let mut span = lo;
                while span < hi && knots[span + 1] <= u {
                    span += 1;
                }
                let b = nurbs_basis_knots(order, u, span, &knots);
                let mut idx = vec![];
                for j in 0..order {
                    let ci = span + 1 + j - order;
                    idx.push(if cyc { ci % n } else { ci });
                }
                let mut p = [0.0f32; 3];
                for (j, &ci) in idx.iter().enumerate() {
                    for q in 0..3 {
                        p[q] += ctrl[ci][q] * b[j];
                    }
                }
                pos.push(p);
                crw.push((idx, b));
            }
            let ctrl_at: Vec<usize> = (0..n).map(|j| ((j as f32 / nseg.max(1) as f32) * (neval - 1) as f32).round() as usize).collect();
            let interp: Vec<(usize, f32)> = (0..neval).map(|i| (i.min(n - 1), 0.0)).collect();
            EvalCurve { pos, ctrl_at, interp, cr: Some(crw) }
        }
        _ => EvalCurve {
            pos: c.pos[r.clone()].to_vec(),
            ctrl_at: (0..n).collect(),
            interp: (0..n).map(|i| (i, 0.0)).collect(),
            cr: None,
        },
    }
}

pub fn accumulate_lengths(pos: &[V3], cyclic: bool) -> Vec<f32> {
    let n = pos.len();
    let mut out = vec![];
    let mut l = 0.0f32;
    for i in 0..n.saturating_sub(1) {
        l += len3(sub3(pos[i + 1], pos[i]));
        out.push(l);
    }
    if cyclic && n > 1 {
        l += len3(sub3(pos[0], pos[n - 1]));
        out.push(l);
    }
    out
}

/// Spline Parameter node, point domain: (factor, length, index) per control point
/// (node_geo_curve_spline_parameter.cc: calculate_point_lengths / convert_lengths_to_factors)
pub fn spline_parameters(c: &Curves) -> (Vec<f32>, Vec<f32>, Vec<i32>) {
    let mut fac = vec![0.0; c.np()];
    let mut len = vec![0.0; c.np()];
    let mut idx = vec![0; c.np()];
    for k in 0..c.nc() {
        let r = c.range(k);
        let n = r.len();
        if n == 0 {
            continue;
        }
        let total;
        if c.kind[k] == CKind::Nurbs {
            let pos = &c.pos[r.clone()];
            let mut length = 0.0f32;
            for i in 0..n.saturating_sub(1) {
                len[r.start + i] = length;
                length += len3(sub3(pos[i + 1], pos[i]));
            }
            len[r.start + n - 1] = length;
            if c.cyclic[k] {
                length += len3(sub3(pos[0], pos[n - 1]));
            }
            total = length;
        } else {
            let e = eval_curve(c, k);
            let l = accumulate_lengths(&e.pos, c.cyclic[k]);
            total = l.last().copied().unwrap_or(0.0);
            for j in 0..n {
                let at = e.ctrl_at[j];
                len[r.start + j] = if j == 0 || at == 0 { 0.0 } else { l[at - 1] };
            }
        }
        for j in 0..n {
            idx[r.start + j] = j as i32;
        }
        // convert lengths to factors
        if total > 0.0 {
            let f = 1.0 / total;
            for j in 0..n {
                fac[r.start + j] = if j == 0 { len[r.start] } else { len[r.start + j] * f };
            }
        } else if n == 1 {
            fac[r.start] = 0.0;
        } else {
            for j in 0..n {
                fac[r.start + j] = j as f32 / (n as f32 - 1.0);
            }
        }
    }
    (fac, len, idx)
}

/// Spline Parameter node, curve domain: (factor, length) per curve = portion of the total length at the curve's start
pub fn spline_parameters_curve(c: &Curves) -> (Vec<f32>, Vec<f32>) {
    let nc = c.nc();
    let mut lengths = vec![0.0f32; nc];
    let mut length = 0.0f32;
    let mut last_total = 0.0f32;
    for k in 0..nc {
        lengths[k] = length;
        let e = eval_curve(c, k);
        let t = accumulate_lengths(&e.pos, c.cyclic[k]).last().copied().unwrap_or(0.0);
        length += t;
        last_total = t;
    }
    let mut fac = lengths.clone();
    if nc > 0 {
        let total = lengths[nc - 1] + last_total;
        if total > 0.0 {
            let f = 1.0 / total;
            for v in fac.iter_mut() {
                *v *= f;
            }
        } else {
            for (i, v) in fac.iter_mut().enumerate() {
                *v = i as f32 / (nc as f32 - 1.0);
            }
        }
    }
    (fac, lengths)
}

/// evaluated tangents of a curve (poly rule + bezier end correction)
pub fn eval_tangents(c: &Curves, k: usize, e: &EvalCurve) -> Vec<V3> {
    let mut t = g32::tangents(&e.pos, c.cyclic[k]);
    if c.kind[k] == CKind::Bezier && !c.cyclic[k] && e.pos.len() > 1 {
        let r = c.range(k);
        let n = r.len();
        let f = norm3(sub3(c.hr[r.start], c.pos[r.start]));
        if len3(f) > 0.0 {
            t[0] = f;
        }
        let l = norm3(sub3(c.pos[r.end - 1], c.hl[r.end - 1]));
        if len3(l) > 0.0 {
            let last = t.len() - 1;
            t[last] = l;
        }
        let _ = n;
    }
    t
}

pub fn curve_tangents(c: &Curves) -> Vec<V3> {
    let mut out = vec![[0.0; 3]; c.np()];
    for k in 0..c.nc() {
        if c.kind[k] == CKind::Nurbs {
            // NURBS: tangents of the control polygon (bke::curves::poly::calculate_tangents)
            let r = c.range(k);
            if r.len() > 0 {
                let t = g32::tangents(&c.pos[r.clone()], c.cyclic[k]);
                for (j, p) in r.enumerate() {
                    out[p] = t[j];
                }
            }
            continue;
        }
        let e = eval_curve(c, k);
        let t = eval_tangents(c, k, &e);
        for (j, p) in c.range(k).enumerate() {
            out[p] = t[e.ctrl_at[j]];
        }
    }
    out
}

fn angle_signed_on_axis(v1: V3, v2: V3, axis: V3) -> f32 {
    let a = angle_between(v1, v2);
    let cr = cross3(v1, v2);
    if dot3(cr, axis) >= 0.0 {
        a
    } else {
        -a
    }
}

fn angle_between(a: V3, b: V3) -> f32 {
    let (an, bn) = (norm3_c(a), norm3_c(b));
    if dot3(an, bn) >= 0.0 {
        2.0 * (len3(sub3(an, bn)) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (len3(add3(an, bn)) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

/// bke::curves::poly::calculate_normals_minimum (Blender 4.2): forward-transported normals, normalized after every step; cyclic curves spread the
/// remaining twist over all points (angle_signed_on_axis_v3v3_v3 returns an angle in [0, 2 pi)).
pub fn normals_min_twist(t: &[V3], cyclic: bool) -> Vec<V3> {
    let n = t.len();
    if n == 0 {
        return vec![];
    }
    let is_zero = |a: V3| a[0] == 0.0 && a[1] == 0.0 && a[2] == 0.0;
    let next_normal = |last_n: V3, last_t: V3, cur_t: V3| -> V3 {
        if is_zero(last_t) || is_zero(cur_t) {
            return last_n;
        }
        let angle = angle_normalized_pub(last_t, cur_t);
        if angle != 0.0 {
            let axis = norm3(cross3(last_t, cur_t));
            if !is_zero(axis) {
                return norm3(g32::rotate_dir_pub(last_n, axis, angle));
            }
        }
        last_n
    };
    let mut out = vec![[0.0f32; 3]; n];
    let first = t[0];
    out[0] = if first[0].abs() + first[1].abs() < 1e-4 { [1.0, 0.0, 0.0] } else { norm3([first[1], -first[0], 0.0]) };
    for i in 1..n {
        out[i] = next_normal(out[i - 1], t[i - 1], t[i]);
    }
    if cyclic {
        let unc = next_normal(out[n - 1], t[n - 1], t[0]);
        let axis = t[0];
        let proj = |v: V3| -> V3 {
            let m = dot3(v, axis);
            [v[0] + axis[0] * (-m), v[1] + axis[1] * (-m), v[2] + axis[2] * (-m)]
        };
        let (v1p, v2p) = (proj(out[0]), proj(unc));
        let mut corr = angle_between(v1p, v2p);
        if dot3(cross3(v2p, v1p), axis) < 0.0 {
            corr = 2.0 * std::f32::consts::PI - corr;
        }
        if corr > std::f32::consts::PI {
            corr -= 2.0 * std::f32::consts::PI;
        }
        let step = corr / n as f32;
        for i in 0..n {
            let ax = t[i];
            if is_zero(ax) {
                continue;
            }
            out[i] = g32::rotate_dir_pub(out[i], ax, step * i as f32);
        }
    }
    out
}

fn angle_normalized_pub(a: V3, b: V3) -> f32 {
    if dot3(a, b) >= 0.0 {
        2.0 * (len3(sub3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (len3(add3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

pub fn normals_z_up(t: &[V3]) -> Vec<V3> {
    t.iter()
        .map(|t| {
            let n = [t[1], -t[0], 0.0];
            if len3(n) == 0.0 {
                [1.0, 0.0, 0.0]
            } else {
                norm3(n)
            }
        })
        .collect()
}

/// evaluated-point normals for a curve with the given mode ("MINIMUM_TWIST"/"Z_UP")
pub fn eval_normals(c: &Curves, k: usize, t: &[V3]) -> Vec<V3> {
    let mode = c.attrs.get("__normal_mode").map(|a| a.data[k].i()).unwrap_or(0);
    if mode == 1 {
        normals_z_up(t)
    } else {
        normals_min_twist(t, c.cyclic[k])
    }
}

/// evaluated normals including the tilt rotation (CurvesGeometry::evaluated_normals)
pub fn eval_normals_tilt(c: &Curves, k: usize, e: &EvalCurve, t: &[V3]) -> Vec<V3> {
    let mut n = eval_normals(c, k, t);
    let tl = interp_to_eval(e, &c.tilt[c.range(k)], c.cyclic[k]);
    for i in 0..n.len() {
        if tl[i] != 0.0 {
            n[i] = g32::rotate_dir_pub(n[i], t[i], tl[i]);
        }
    }
    n
}

pub fn curve_normals(c: &Curves) -> Vec<V3> {
    let mut out = vec![[0.0; 3]; c.np()];
    for k in 0..c.nc() {
        if c.kind[k] == CKind::Nurbs {
            // no control-point <-> evaluated-point correspondence: treat the control polygon as a poly curve (no tilt)
            let r = c.range(k);
            if r.len() > 0 {
                let t = g32::tangents(&c.pos[r.clone()], c.cyclic[k]);
                let n = eval_normals(c, k, &t);
                for (j, p) in r.enumerate() {
                    out[p] = n[j];
                }
            }
            continue;
        }
        let e = eval_curve(c, k);
        let t = eval_tangents(c, k, &e);
        let n = eval_normals_tilt(c, k, &e, &t);
        for (j, p) in c.range(k).enumerate() {
            out[p] = n[e.ctrl_at[j]];
        }
    }
    out
}

/// interpolate a per-control-point float attribute to evaluated points
pub fn interp_to_eval(e: &EvalCurve, src: &[f32], cyclic: bool) -> Vec<f32> {
    let n = src.len();
    if let Some(cr) = &e.cr {
        return cr.iter().map(|(ids, w)| ids.iter().zip(w.iter()).fold(0.0f32, |acc, (&i, &wt)| acc + src[i] * wt)).collect();
    }
    e.interp
        .iter()
        .map(|&(a, f)| {
            if f == 0.0 {
                src[a]
            } else {
                let b = if a + 1 < n { a + 1 } else if cyclic { 0 } else { a };
                src[a] * (1.0 - f) + src[b] * f
            }
        })
        .collect()
}

/// Blender bezier::evaluate_segment: `res` points (the end point is not included)
pub fn bezier_segment_eval(p0: V3, h0: V3, h1: V3, p1: V3, res: usize) -> Vec<V3> {
    let inv = 1.0f32 / res as f32;
    let inv2 = inv * inv;
    let inv3 = inv2 * inv;
    let mut q0 = p0;
    let mut q1 = [0.0f32; 3];
    let mut q2 = [0.0f32; 3];
    let mut q3 = [0.0f32; 3];
    for k in 0..3 {
        let rt1 = 3.0 * (h0[k] - p0[k]) * inv;
        let rt2 = 3.0 * (p0[k] - 2.0 * h0[k] + h1[k]) * inv2;
        let rt3 = (p1[k] - p0[k] + 3.0 * (h0[k] - h1[k])) * inv3;
        q1[k] = rt1 + rt2 + rt3;
        q2[k] = 2.0 * rt2 + 6.0 * rt3;
        q3[k] = 6.0 * rt3;
    }
    let mut out = Vec::with_capacity(res);
    out.push(q0);
    for _ in 1..res {
        for k in 0..3 {
            q0[k] += q1[k];
            q1[k] += q2[k];
            q2[k] += q3[k];
        }
        out.push(q0);
    }
    out
}

/// sample positions: for every requested (segment index, factor) lerp evaluated points (segment i -> i+1, wrapping if cyclic)
pub fn lerp_at(pos: &[V3], cyclic: bool, idx: usize, f: f32) -> V3 {
    let n = pos.len();
    let a = pos[idx.min(n - 1)];
    let bi = if idx + 1 < n { idx + 1 } else if cyclic { 0 } else { n - 1 };
    let b = pos[bi];
    [a[0] * (1.0 - f) + b[0] * f, a[1] * (1.0 - f) + b[1] * f, a[2] * (1.0 - f) + b[2] * f]
}

/// length_parameterize::sample_uniform: (segment index, factor) for `count` samples
pub fn sample_uniform(acc: &[f32], include_last: bool, count: usize) -> Vec<(usize, f32)> {
    if acc.is_empty() || count == 0 {
        return vec![(0, 0.0); count];
    }
    if count == 1 {
        return vec![(0, 0.0)];
    }
    let total = *acc.last().unwrap();
    let step = total / ((count as i32 - include_last as i32) as f32);
    (0..count).map(|i| sample_at_length(acc, total.min(i as f32 * step))).collect()
}

/// length_parameterize::sample_at_length: (segment, factor) for a length along the curve
pub fn sample_at_length(acc: &[f32], length: f32) -> (usize, f32) {
    if acc.is_empty() {
        return (0, 0.0);
    }
    let total = *acc.last().unwrap();
    let l = length.max(0.0);
    if l >= total {
        return (acc.len() - 1, 1.0);
    }
    let seg = acc.partition_point(|&a| a <= l);
    let start = if seg == 0 { 0.0 } else { acc[seg - 1] };
    let end = acc[seg];
    let seglen = end - start;
    let inv = if seglen != 0.0 { 1.0 / seglen } else { 0.0 };
    (seg, (l - start) * inv)
}

/// uniform-knot B-spline basis values of the `order` control points ending at `span` (Cox-de Boor)
pub fn nurbs_basis(order: usize, u: f32, span: usize) -> Vec<f32> {
    let mut b = vec![0.0f32; order];
    b[0] = 1.0;
    let mut left = vec![0.0f32; order];
    let mut right = vec![0.0f32; order];
    for j in 1..order {
        left[j] = u - (span + 1 - j) as f32;
        right[j] = (span + j) as f32 - u;
        let mut saved = 0.0f32;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den != 0.0 { b[r] / den } else { 0.0 };
            b[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        b[j] = saved;
    }
    b
}

/// B-spline basis for arbitrary knots (Cox-de Boor, NURBS book A2.2)
pub fn nurbs_basis_knots(order: usize, u: f32, span: usize, knots: &[f32]) -> Vec<f32> {
    let mut b = vec![0.0f32; order];
    b[0] = 1.0;
    let mut left = vec![0.0f32; order];
    let mut right = vec![0.0f32; order];
    for j in 1..order {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0f32;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den != 0.0 { b[r] / den } else { 0.0 };
            b[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        b[j] = saved;
    }
    b
}

// ---------------------------------------------------------------- bezier handles (curve_bezier.cc / curves_geometry.cc)
fn vec_handle(p: V3, other: V3) -> V3 {
    let t = 1.0f32 / 3.0;
    [p[0] * (1.0 - t) + other[0] * t, p[1] * (1.0 - t) + other[1] * t, p[2] * (1.0 - t) + other[2] * t]
}

fn aligned_handle(position: V3, other: V3, aligned: V3) -> V3 {
    let length = len3(sub3(aligned, position));
    let dir = norm3(sub3(other, position));
    sub3(position, mul3(dir, length))
}

fn point_handles(tl: u8, tr: u8, position: V3, prev: V3, next: V3, left: &mut V3, right: &mut V3) {
    if tl == 1 || tr == 1 {
        let prev_diff = sub3(position, prev);
        let next_diff = sub3(next, position);
        let mut prev_len = len3(prev_diff);
        let mut next_len = len3(next_diff);
        if prev_len == 0.0 {
            prev_len = 1.0;
        }
        if next_len == 0.0 {
            next_len = 1.0;
        }
        let nd = [next_diff[0] / next_len, next_diff[1] / next_len, next_diff[2] / next_len];
        let pd = [prev_diff[0] / prev_len, prev_diff[1] / prev_len, prev_diff[2] / prev_len];
        let dir = add3(nd, pd);
        let len = len3(dir) * 2.5614f32;
        if len != 0.0 {
            if tl == 1 {
                let pc = prev_len.min(next_len * 5.0);
                *left = add3(position, mul3(dir, -(pc / len)));
            }
            if tr == 1 {
                let nc = next_len.min(prev_len * 5.0);
                *right = add3(position, mul3(dir, nc / len));
            }
        }
    }
    if tl == 2 {
        *left = vec_handle(position, prev);
    }
    if tr == 2 {
        *right = vec_handle(position, next);
    }
    if tl == 3 && tr != 3 {
        *left = aligned_handle(position, *right, *left);
    } else if tl != 3 && tr == 3 {
        *right = aligned_handle(position, *left, *right);
    }
}

/// CurvesGeometry::calculate_bezier_auto_handles
pub fn calculate_bezier_auto_handles(c: &mut Curves) {
    for k in 0..c.nc() {
        if c.kind[k] != CKind::Bezier {
            continue;
        }
        let r = c.range(k);
        let n = r.len();
        if n <= 1 {
            continue;
        }
        let cyc = c.cyclic[k];
        let s = r.start;
        let pos: Vec<V3> = c.pos[r.clone()].to_vec();
        let (mut hl, mut hr) = (c.hl[s], c.hr[s]);
        let prev = if cyc { pos[n - 1] } else { sub3(mul3(pos[0], 2.0), pos[1]) };
        point_handles(c.htl[s], c.htr[s], pos[0], prev, pos[1], &mut hl, &mut hr);
        c.hl[s] = hl;
        c.hr[s] = hr;
        for i in 1..n - 1 {
            let (mut hl, mut hr) = (c.hl[s + i], c.hr[s + i]);
            point_handles(c.htl[s + i], c.htr[s + i], pos[i], pos[i - 1], pos[i + 1], &mut hl, &mut hr);
            c.hl[s + i] = hl;
            c.hr[s + i] = hr;
        }
        let l = n - 1;
        let (mut hl, mut hr) = (c.hl[s + l], c.hr[s + l]);
        let next = if cyc { pos[0] } else { sub3(mul3(pos[l], 2.0), pos[l - 1]) };
        point_handles(c.htl[s + l], c.htr[s + l], pos[l], pos[l - 1], next, &mut hl, &mut hr);
        c.hl[s + l] = hl;
        c.hr[s + l] = hr;
    }
}

/// bezier::set_handle_position
pub fn set_handle_position(position: V3, ty: u8, ty_other: u8, new_handle: V3, handle: &mut V3, handle_other: &mut V3) {
    if ty == 1 || ty == 2 {
        return;
    }
    *handle = new_handle;
    if ty_other == 3 {
        *handle_other = aligned_handle(position, *handle, *handle_other);
    }
}
