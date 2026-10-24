//! Curve nodes: primitives, resample, sample, trim, fillet, conversion.
use super::attr::*;
use super::curveops::*;
use super::eval::*;
use super::field::*;
use super::geom::*;
use super::load::*;
use super::math::*;
use super::meshops::*;
use super::nodes::*;
use super::nodes_geo::*;
use super::prim::*;
use super::rot::*;
use super::val::*;
use std::rc::Rc;

pub fn mix_val(a: &Val, b: &Val, f: f32, ty: Ty) -> Val {
    match ty {
        Ty::F => Val::F(a.f() * (1.0 - f) + b.f() * f),
        Ty::V => {
            let (x, y) = (a.v(), b.v());
            Val::V([x[0] * (1.0 - f) + y[0] * f, x[1] * (1.0 - f) + y[1] * f, x[2] * (1.0 - f) + y[2] * f])
        }
        Ty::C => {
            let (x, y) = (a.c(), b.c());
            Val::C([x[0] * (1.0 - f) + y[0] * f, x[1] * (1.0 - f) + y[1] * f, x[2] * (1.0 - f) + y[2] * f, x[3] * (1.0 - f) + y[3] * f])
        }
        Ty::I => Val::I((a.i() as f32 * (1.0 - f) + b.i() as f32 * f) as i32),
        Ty::B => Val::B(a.b() || b.b()),
        _ => a.clone(),
    }
}

/// per-point attributes of curve `k` as evaluated-point value lists (name, type, values)
pub fn eval_point_attrs(c: &Curves, k: usize, e: &EvalCurve) -> Vec<(String, Ty, Vec<Val>)> {
    let r = c.range(k);
    let n = r.len();
    let mut out = vec![];
    let interp = |src: Vec<Val>, ty: Ty| -> Vec<Val> {
        if let Some(cr) = &e.cr {
            if matches!(ty, Ty::F | Ty::V | Ty::I) {
                return cr
                    .iter()
                    .map(|(ids, w)| {
                        if ty == Ty::I {
                            let x = ids.iter().zip(w.iter()).fold(0.0f32, |a, (&i, &wt)| a + src[i].i() as f32 * wt);
                            Val::I((x + 0.5).floor() as i32)
                        } else if ty == Ty::F {
                            Val::F(ids.iter().zip(w.iter()).fold(0.0f32, |a, (&i, &wt)| a + src[i].f() * wt))
                        } else {
                            let mut o = [0.0f32; 3];
                            for (&i, &wt) in ids.iter().zip(w.iter()) {
                                let v = src[i].v();
                                for k in 0..3 {
                                    o[k] += v[k] * wt;
                                }
                            }
                            Val::V(o)
                        }
                    })
                    .collect();
            }
        }
        e.interp
            .iter()
            .map(|&(a, f)| {
                if f == 0.0 {
                    src[a].clone()
                } else {
                    let b = if a + 1 < n { a + 1 } else if c.cyclic[k] { 0 } else { a };
                    mix_val(&src[a], &src[b], f, ty)
                }
            })
            .collect()
    };
    out.push(("radius".to_string(), Ty::F, interp(c.radius[r.clone()].iter().map(|x| Val::F(*x)).collect(), Ty::F)));
    out.push(("tilt".to_string(), Ty::F, interp(c.tilt[r.clone()].iter().map(|x| Val::F(*x)).collect(), Ty::F)));
    for (name, a) in c.attrs.iter() {
        if a.dom == Dom::Point {
            out.push((name.clone(), a.ty, interp(a.data[r.clone()].to_vec(), a.ty)));
        }
    }
    out
}

/// poly curves with samples; returns the new curve and fills its radius/tilt/attrs
pub struct Sampled {
    pub pos: Vec<V3>,
    pub attrs: Vec<(String, Ty, Vec<Val>)>,
    pub tangent: Vec<V3>,
    pub normal: Vec<V3>,
}

pub fn sample_curve_at(c: &Curves, k: usize, samples: &[(usize, f32)]) -> Sampled {
    let e = eval_curve(c, k);
    let cyc = c.cyclic[k];
    let tan = eval_tangents(c, k, &e);
    let nrm = eval_normals_tilt(c, k, &e, &tan);
    let attrs_ev = eval_point_attrs(c, k, &e);
    let n = e.pos.len();
    let mut s = Sampled { pos: vec![], attrs: attrs_ev.iter().map(|(nm, t, _)| (nm.clone(), *t, vec![])).collect(), tangent: vec![], normal: vec![] };
    for &(seg, f) in samples {
        let seg = seg.min(n.saturating_sub(1));
        s.pos.push(lerp_at(&e.pos, cyc, seg, f));
        let bi = if seg + 1 < n { seg + 1 } else if cyc { 0 } else { n - 1 };
        let tl = lerp3(tan[seg], tan[bi], f);
        s.tangent.push(norm3(tl));
        let nl = lerp3(nrm[seg], nrm[bi], f);
        s.normal.push(norm3(nl));
        for (i, (_, t, vals)) in attrs_ev.iter().enumerate() {
            let v = if f == 0.0 { vals[seg].clone() } else { mix_val(&vals[seg], &vals[bi], f, *t) };
            s.attrs[i].2.push(v);
        }
    }
    s
}

pub fn push_sampled(out: &mut Curves, s: &Sampled, cyclic: bool) {
    let _ = cyclic;
    out.push_poly(&s.pos, cyclic);
    let base = out.pos.len() - s.pos.len();
    for (name, ty, vals) in &s.attrs {
        match name.as_str() {
            "radius" => {
                for (i, v) in vals.iter().enumerate() {
                    out.radius[base + i] = v.f();
                }
            }
            "tilt" => {
                for (i, v) in vals.iter().enumerate() {
                    out.tilt[base + i] = v.f();
                }
            }
            _ => {
                let a = out.attrs.entry(name.clone()).or_insert_with(|| Attr { dom: Dom::Point, ty: *ty, data: vec![Val::zero(*ty); base] });
                a.data.resize(base, Val::zero(*ty));
                a.data.extend(vals.iter().cloned());
            }
        }
    }
    // pad attrs that this curve did not provide
    let total = out.pos.len();
    for a in out.attrs.values_mut() {
        if a.dom == Dom::Point {
            let z = Val::zero(a.ty);
            a.data.resize(total, z);
        }
    }
}

fn counts_for(c: &Curves, mode: &str, count_f: &[Val], len_f: &[Val], k: usize) -> usize {
    let e = eval_curve(c, k);
    let acc = accumulate_lengths(&e.pos, c.cyclic[k]);
    let total = acc.last().copied().unwrap_or(0.0);
    match mode {
        "COUNT" => count_f[k].i().max(1) as usize,
        "LENGTH" => {
            let l = len_f[k].f().max(0.0001);
            let c_ = (total / l) as i32;
            (c_ + 1) as usize
        }
        _ => e.pos.len(),
    }
}

pub fn curve_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "GeometryNodeCurveLength" => {
            let g = ev.geo(ni, "Curve")?;
            let mut l = 0.0f32;
            if let Some(c) = &g.curves {
                for k in 0..c.nc() {
                    let e = eval_curve(c, k);
                    l += accumulate_lengths(&e.pos, c.cyclic[k]).last().copied().unwrap_or(0.0);
                }
            }
            outs(node, vec![("Length", Value::konst(Val::F(l)))])
        }
        "GeometryNodeInputRadius" => outs(node, vec![("Radius", fld(Rc::new(FNode::Named("radius".into(), Ty::F))))]),
        "GeometryNodeInputCurveTilt" => outs(node, vec![("Tilt", fld(Rc::new(FNode::Named("tilt".into(), Ty::F))))]),
        "GeometryNodeCurveEndpointSelection" => {
            let s = ev.ci(ni, "Start Size")?.max(0) as usize;
            let e = ev.ci(ni, "End Size")?.max(0) as usize;
            let f = Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| match ctx.comp {
                Comp::Curves(c) if ctx.dom == Dom::Point => {
                    let mut v = vec![Val::B(false); c.np()];
                    for k in 0..c.nc() {
                        let r = c.range(k);
                        let n = r.len();
                        for (j, p) in r.enumerate() {
                            if j < s || j + e >= n {
                                v[p] = Val::B(true);
                            }
                        }
                    }
                    v
                }
                _ => vec![Val::B(false); ctx.n],
            })));
            outs(node, vec![("Selection", fld(f))])
        }
        "GeometryNodeCurveStar" => {
            let n = ev.ci(ni, "Points")?.max(3) as usize;
            let (ir, or_, tw) = (ev.cf(ni, "Inner Radius")?, ev.cf(ni, "Outer Radius")?, ev.cf(ni, "Twist")?);
            let step = (2.0 * std::f64::consts::PI / n as f64) as f32;
            let mut pts = vec![];
            for i in 0..n {
                let a = step * i as f32;
                pts.push([or_ * a.cos(), or_ * a.sin(), 0.0]);
                let a2 = a + step * 0.5 + tw;
                pts.push([ir * a2.cos(), ir * a2.sin(), 0.0]);
            }
            let mut c = Curves::new();
            c.push_poly(&pts, true);
            let outer = Rc::new(FNode::Ctx(Rc::new(|ctx: &Ctx| (0..ctx.n).map(|i| Val::B(i % 2 == 0)).collect())));
            outs(node, vec![("Curve", rcgeo(Geometry::from_curves(c))), ("Outer Points", fld(outer))])
        }
        "GeometryNodeSplineLength" => {
            let f = Rc::new(FNode::Ctx(Rc::new(|ctx: &Ctx| match ctx.comp {
                Comp::Curves(c) => {
                    let lens: Vec<Val> = (0..c.nc())
                        .map(|k| {
                            let e = eval_curve(c, k);
                            Val::F(accumulate_lengths(&e.pos, c.cyclic[k]).last().copied().unwrap_or(0.0))
                        })
                        .collect();
                    if ctx.dom == Dom::Curve {
                        lens
                    } else {
                        adapt_curves(c, Dom::Curve, ctx.dom, Ty::F, &lens)
                    }
                }
                _ => vec![Val::F(0.0); ctx.n],
            })));
            let pc = Rc::new(FNode::Ctx(Rc::new(|ctx: &Ctx| match ctx.comp {
                Comp::Curves(c) => {
                    let v: Vec<Val> = (0..c.nc()).map(|k| Val::I(c.range(k).len() as i32)).collect();
                    if ctx.dom == Dom::Curve {
                        v
                    } else {
                        adapt_curves(c, Dom::Curve, ctx.dom, Ty::I, &v)
                    }
                }
                _ => vec![Val::I(0); ctx.n],
            })));
            outs(node, vec![("Length", fld(f)), ("Point Count", fld(pc))])
        }
        "GeometryNodeCurveSplineType" => {
            let g = ev.geo(ni, "Curve")?;
            let kind = match node.ps("spline_type") {
                "BEZIER" => CKind::Bezier,
                "CATMULL_ROM" => CKind::CatmullRom,
                "NURBS" => CKind::Nurbs,
                _ => CKind::Poly,
            };
            let mut o = (*g).clone();
            if let Some(c) = o.curves.as_mut() {
                let sel = selection(ev, ni, Comp::Curves(g.curves.as_ref().unwrap()), Dom::Curve)?;
                for k in 0..c.nc() {
                    if !sel[k] || c.kind[k] == kind {
                        continue;
                    }
                    if kind == CKind::Bezier && c.kind[k] != CKind::Bezier {
                        // poly -> bezier: vector handles one third of the way to the neighbours
                        let r = c.range(k);
                        let n = r.len();
                        for (j, p) in r.clone().enumerate() {
                            let cyc = c.cyclic[k];
                            let prev = if j > 0 { Some(c.pos[r.start + j - 1]) } else if cyc { Some(c.pos[r.start + n - 1]) } else { None };
                            let next = if j + 1 < n { Some(c.pos[r.start + j + 1]) } else if cyc { Some(c.pos[r.start]) } else { None };
                            let pos = c.pos[p];
                            c.hl[p] = match prev {
                                Some(q) => add3(pos, mul3(sub3(q, pos), 1.0 / 3.0)),
                                None => pos,
                            };
                            c.hr[p] = match next {
                                Some(q) => add3(pos, mul3(sub3(q, pos), 1.0 / 3.0)),
                                None => pos,
                            };
                            c.htl[p] = 2;
                            c.htr[p] = 2;
                        }
                    }
                    c.kind[k] = kind;
                }
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeCurveSetHandles" => {
            let g = ev.geo(ni, "Curve")?;
            let ty: u8 = match node.ps("handle_type") {
                "FREE" => 0,
                "VECTOR" => 2,
                "ALIGN" => 3,
                _ => 1,
            };
            let (mut left, mut right) = (false, false);
            if let Some(serde_json::Value::Array(a)) = node.pj("mode") {
                for m in a {
                    match m.as_str() {
                        Some("LEFT") => left = true,
                        Some("RIGHT") => right = true,
                        _ => {}
                    }
                }
            } else {
                left = true;
                right = true;
            }
            let mut o = (*g).clone();
            if let Some(c) = o.curves.as_mut() {
                let sel = selection(ev, ni, Comp::Curves(g.curves.as_ref().unwrap()), Dom::Point)?;
                for i in 0..c.np() {
                    if sel[i] {
                        if left {
                            c.htl[i] = ty;
                        }
                        if right {
                            c.htr[i] = ty;
                        }
                    }
                }
                calculate_bezier_auto_handles(c);
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeSetCurveHandlePositions" => {
            let g = ev.geo(ni, "Curve")?;
            let use_left = node.ps("mode") != "RIGHT";
            let mut o = (*g).clone();
            if let Some(c) = o.curves.as_mut() {
                let comp = Comp::Curves(g.curves.as_ref().unwrap());
                let sel = selection(ev, ni, comp, Dom::Point)?;
                let pos = eval_on(&ev.field(ni, "Position")?, comp, Dom::Point);
                let off = eval_on(&ev.field(ni, "Offset")?, comp, Dom::Point);
                for i in 0..c.np() {
                    if !sel[i] {
                        continue;
                    }
                    let (mut t, mut t_other) = if use_left { (c.htl[i], c.htr[i]) } else { (c.htr[i], c.htl[i]) };
                    // update_handle_types_for_movement
                    match t {
                        1 => {
                            if t_other == 1 {
                                t = 3;
                                t_other = 3;
                            } else {
                                t = 0;
                            }
                        }
                        2 => t = 0,
                        3 => {
                            if t_other != 3 {
                                t = 0;
                            }
                        }
                        _ => {}
                    }
                    if use_left {
                        c.htl[i] = t;
                        c.htr[i] = t_other;
                    } else {
                        c.htr[i] = t;
                        c.htl[i] = t_other;
                    }
                    let newp = add3(pos[i].v(), off[i].v());
                    let position = c.pos[i];
                    if use_left {
                        let (mut h, mut ho) = (c.hl[i], c.hr[i]);
                        set_handle_position(position, t, t_other, newp, &mut h, &mut ho);
                        c.hl[i] = h;
                        c.hr[i] = ho;
                    } else {
                        let (mut h, mut ho) = (c.hr[i], c.hl[i]);
                        set_handle_position(position, t, t_other, newp, &mut h, &mut ho);
                        c.hr[i] = h;
                        c.hl[i] = ho;
                    }
                }
                calculate_bezier_auto_handles(c);
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeCurvePrimitiveBezierSegment" => {
            let res = ev.ci(ni, "Resolution")?.max(1);
            let (p0, mut h0, mut h1, p1) = (ev.cv(ni, "Start")?, ev.cv(ni, "Start Handle")?, ev.cv(ni, "End Handle")?, ev.cv(ni, "End")?);
            if node.ps("mode") == "OFFSET" {
                h0 = add3(p0, h0);
                h1 = add3(p1, h1);
            }
            let mut c = Curves::new();
            c.push_kind(CKind::Bezier, &[p0, p1], false);
            c.res[0] = res;
            c.hr[0] = h0;
            c.hl[1] = h1;
            c.hl[0] = sub3(p0, sub3(h0, p0));
            c.hr[1] = sub3(p1, sub3(h1, p1));
            c.htl = vec![0, 0];
            c.htr = vec![0, 0];
            outs(node, vec![("Curve", rcgeo(Geometry::from_curves(c)))])
        }
        "GeometryNodeCurveArc" => {
            if node.ps("mode") == "POINTS" {
                return Err("CurveArc POINTS mode unsupported".into());
            }
            let res = ev.ci(ni, "Resolution")?.max(2) as usize;
            let radius = ev.cf(ni, "Radius")?;
            let start = ev.cf(ni, "Start Angle")?;
            let mut sweep = ev.cf(ni, "Sweep Angle")?;
            let connect = ev.cb(ni, "Connect Center")?;
            let invert = ev.cb(ni, "Invert Arc")?;
            if invert {
                sweep = -(2.0 * std::f32::consts::PI - sweep.abs()) * sweep.signum();
            }
            let step = sweep / (res - 1) as f32;
            let mut pts: Vec<V3> = (0..res)
                .map(|i| {
                    let t = step * i as f32 + start;
                    [radius * t.cos(), radius * t.sin(), 0.0]
                })
                .collect();
            if connect {
                pts.push([0.0; 3]);
            }
            let mut c = Curves::new();
            c.push_poly(&pts, connect);
            outs(node, vec![("Curve", rcgeo(Geometry::from_curves(c)))])
        }
        "GeometryNodeCurveSpiral" => {
            let res = ev.ci(ni, "Resolution")?.max(1);
            let rot = ev.cf(ni, "Rotations")?;
            let (sr, er, h) = (ev.cf(ni, "Start Radius")?, ev.cf(ni, "End Radius")?, ev.cf(ni, "Height")?);
            let dir = ev.cb(ni, "Reverse")?;
            let total = ((res as f32 * rot) as i32).max(1);
            let dr = (er - sr) / total as f32;
            let dh = h / total as f32;
            let dt = (std::f32::consts::PI * 2.0 * rot) / total as f32 * if dir { 1.0 } else { -1.0 };
            let pts: Vec<V3> = (0..=total)
                .map(|i| {
                    let th = i as f32 * dt;
                    let rad = sr + i as f32 * dr;
                    [rad * th.cos(), rad * th.sin(), dh * i as f32]
                })
                .collect();
            let mut c = Curves::new();
            c.push_poly(&pts, false);
            outs(node, vec![("Curve", rcgeo(Geometry::from_curves(c)))])
        }
        "GeometryNodeReverseCurve" => {
            let g = ev.geo(ni, "Curve")?;
            let mut o = (*g).clone();
            if let Some(c) = o.curves.as_mut() {
                let sel = selection(ev, ni, Comp::Curves(g.curves.as_ref().unwrap()), Dom::Curve)?;
                for k in 0..c.nc() {
                    if !sel[k] {
                        continue;
                    }
                    let r = c.range(k);
                    c.pos[r.clone()].reverse();
                    c.radius[r.clone()].reverse();
                    c.tilt[r.clone()].reverse();
                    let (mut hl, mut hr) = (c.hl[r.clone()].to_vec(), c.hr[r.clone()].to_vec());
                    hl.reverse();
                    hr.reverse();
                    c.hl[r.clone()].copy_from_slice(&hr);
                    c.hr[r.clone()].copy_from_slice(&hl);
                    let (mut tl, mut tr) = (c.htl[r.clone()].to_vec(), c.htr[r.clone()].to_vec());
                    tl.reverse();
                    tr.reverse();
                    c.htl[r.clone()].copy_from_slice(&tr);
                    c.htr[r.clone()].copy_from_slice(&tl);
                    for a in c.attrs.values_mut() {
                        if a.dom == Dom::Point {
                            a.data[r.clone()].reverse();
                        }
                    }
                }
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeSetCurveNormal" => {
            let g = ev.geo(ni, "Curve")?;
            let mut o = (*g).clone();
            if let Some(c) = o.curves.as_mut() {
                let mode = match node.ps("mode") {
                    "Z_UP" => 1,
                    _ => 0,
                };
                let sel = selection(ev, ni, Comp::Curves(g.curves.as_ref().unwrap()), Dom::Curve)?;
                let nc = c.nc();
                let a = c.attrs.entry("__normal_mode".into()).or_insert_with(|| Attr { dom: Dom::Curve, ty: Ty::I, data: vec![Val::I(0); nc] });
                for k in 0..nc {
                    if sel[k] {
                        a.data[k] = Val::I(mode);
                    }
                }
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeResampleCurve" | "GeometryNodeCurveToPoints" => {
            let is_points = ty.ends_with("ToPoints");
            let g = ev.geo(ni, if is_points { "Curve" } else { "Curve" })?;
            let mode = node.ps("mode").to_string();
            let mut out_c = Curves::new();
            out_c.has_radius = g.curves.as_ref().map_or(false, |c| c.has_radius);
            let mut pts = Points::default();
            let (mut tang, mut norm, mut rotq) = (vec![], vec![], vec![]);
            if let Some(c) = &g.curves {
                let comp = Comp::Curves(c);
                let sel = if is_points { vec![true; c.nc()] } else { selection(ev, ni, comp, Dom::Curve)? };
                let count_f = if mode == "COUNT" { eval_on(&ev.field(ni, "Count")?, comp, Dom::Curve) } else { vec![] };
                let len_f = if mode == "LENGTH" { eval_on(&ev.field(ni, "Length")?, comp, Dom::Curve) } else { vec![] };
                let mut copied_curve_attrs: Vec<usize> = vec![];
                for k in 0..c.nc() {
                    if !sel[k] {
                        // unselected curves are copied unchanged
                        let r = c.range(k);
                        let ctrl: Vec<V3> = c.pos[r.clone()].to_vec();
                        let _ = ctrl;
                        let e = eval_curve(c, k);
                        let samples: Vec<(usize, f32)> = (0..e.pos.len()).map(|i| (i, 0.0)).collect();
                        let s = sample_curve_at(c, k, &samples);
                        push_sampled(&mut out_c, &s, c.cyclic[k]);
                        copied_curve_attrs.push(k);
                        continue;
                    }
                    let n = counts_for(c, &mode, &count_f, &len_f, k);
                    let e = eval_curve(c, k);
                    let samples: Vec<(usize, f32)> = if mode == "EVALUATED" {
                        (0..e.pos.len()).map(|i| (i, 0.0)).collect()
                    } else {
                        let acc = accumulate_lengths(&e.pos, c.cyclic[k]);
                        sample_uniform(&acc, !c.cyclic[k], n)
                    };
                    let s = sample_curve_at(c, k, &samples);
                    if is_points {
                        pts.pos.extend(s.pos.iter().cloned());
                        tang.extend(s.tangent.iter().cloned());
                        norm.extend(s.normal.iter().cloned());
                        for i in 0..s.pos.len() {
                            rotq.push(rotation_from_axes(s.normal[i], s.tangent[i]));
                        }
                        for (name, ty_, vals) in &s.attrs {
                            let a = pts.attrs.entry(name.clone()).or_insert_with(|| Attr { dom: Dom::Point, ty: *ty_, data: vec![] });
                            a.data.extend(vals.iter().cloned());
                        }
                    } else {
                        push_sampled(&mut out_c, &s, c.cyclic[k]);
                        copied_curve_attrs.push(k);
                    }
                }
                if !is_points {
                    // curve-domain attributes
                    for (name, a) in c.attrs.iter() {
                        if a.dom == Dom::Curve && !name.starts_with("__normal") || name.starts_with("__nurbs") {
                            out_c.attrs.insert(name.clone(), Attr { dom: Dom::Curve, ty: a.ty, data: copied_curve_attrs.iter().map(|&k| a.data[k].clone()).collect() });
                        }
                    }
                }
            }
            if is_points {
                pts.attrs.insert("tangent".into(), Attr { dom: Dom::Point, ty: Ty::V, data: tang.iter().map(|v| Val::V(*v)).collect() });
                pts.attrs.insert("normal".into(), Attr { dom: Dom::Point, ty: Ty::V, data: norm.iter().map(|v| Val::V(*v)).collect() });
                pts.attrs.insert("rotation".into(), Attr { dom: Dom::Point, ty: Ty::Q, data: rotq.iter().map(|v| Val::Q(*v)).collect() });
                let mut o = Geometry::default();
                o.points = Some(pts);
                outs(
                    node,
                    vec![
                        ("Points", rcgeo(o)),
                        ("Tangent", fld(Rc::new(FNode::Named("tangent".into(), Ty::V)))),
                        ("Normal", fld(Rc::new(FNode::Named("normal".into(), Ty::V)))),
                        ("Rotation", fld(Rc::new(FNode::Named("rotation".into(), Ty::Q)))),
                    ],
                )
            } else {
                outs(node, vec![("Curve", rcgeo(Geometry::from_curves(out_c)))])
            }
        }
        "GeometryNodeSampleCurve" => {
            let g = ev.geo(ni, "Curves")?;
            let by_length = node.ps("mode") == "LENGTH";
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let fac = ev.field(ni, if by_length { "Length" } else { "Factor" })?;
            let cidx = if ev.has_input(ni, "Curve Index") { ev.field(ni, "Curve Index")? } else { konst(Val::I(0)) };
            let all = node.pb("use_all_curves");
            let mut pos_tab = vec![];
            // precompute per-curve evaluated data
            struct CD {
                e: EvalCurve,
                acc: Vec<f32>,
                tan: Vec<V3>,
                nrm: Vec<V3>,
                cyc: bool,
                vals: Vec<Val>,
            }
            let mut cds: Vec<CD> = vec![];
            if let Some(c) = &g.curves {
                let vals_pt = eval_on(&value, Comp::Curves(c), Dom::Point);
                for k in 0..c.nc() {
                    let e = eval_curve(c, k);
                    let acc = accumulate_lengths(&e.pos, c.cyclic[k]);
                    let tan = eval_tangents(c, k, &e);
                    let nrm = eval_normals_tilt(c, k, &e, &tan);
                    let r = c.range(k);
                    let n = r.len();
                    let src: Vec<Val> = r.map(|p| vals_pt[p].convert(t)).collect();
                    let vals: Vec<Val> = e
                        .interp
                        .iter()
                        .map(|&(a, f)| {
                            if f == 0.0 {
                                src[a].clone()
                            } else {
                                let b = if a + 1 < n { a + 1 } else if c.cyclic[k] { 0 } else { a };
                                mix_val(&src[a], &src[b], f, t)
                            }
                        })
                        .collect();
                    cds.push(CD { cyc: c.cyclic[k], e, acc, tan, nrm, vals });
                }
            }
            let cds = Rc::new(cds);
            let cds2 = cds.clone();
            let k = Rc::new(move |a: &[&[Val]], n: usize| -> Vec<Vec<Val>> {
                let mut outv = vec![Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
                for i in 0..n {
                    let f = at(a[0], i).f();
                    let ci = at(a[1], i).i();
                    let (cd_idx, total_len);
                    if all {
                        let tl: f32 = cds2.iter().map(|c| c.acc.last().copied().unwrap_or(0.0)).sum();
                        total_len = tl;
                        cd_idx = 0usize;
                    } else {
                        cd_idx = ci.clamp(0, cds2.len() as i32 - 1).max(0) as usize;
                        total_len = 0.0;
                    }
                    let _ = total_len;
                    if cds2.is_empty() {
                        for o in outv.iter_mut() {
                            o.push(Val::zero(Ty::V));
                        }
                        outv[0][i] = Val::zero(t);
                        continue;
                    }
                    let c = &cds2[cd_idx];
                    let total = c.acc.last().copied().unwrap_or(0.0);
                    let len = if by_length { f } else { f.clamp(0.0, 1.0) * total };
                    let (seg, ff) = sample_at_length(&c.acc, len);
                    let n_ = c.e.pos.len();
                    let bi = if seg + 1 < n_ { seg + 1 } else if c.cyc { 0 } else { n_ - 1 };
                    outv[0].push(mix_val(&c.vals[seg.min(n_ - 1)], &c.vals[bi], ff, t));
                    outv[1].push(Val::V(lerp_at(&c.e.pos, c.cyc, seg, ff)));
                    outv[2].push(Val::V(norm3(lerp3(c.tan[seg], c.tan[bi], ff))));
                    outv[3].push(Val::V(norm3(lerp3(c.nrm[seg], c.nrm[bi], ff))));
                }
                outv
            });
            let r = mf(k, vec![fac, cidx], 4);
            pos_tab.push(());
            outs(node, vec![("Value", fld(r[0].clone())), ("Position", fld(r[1].clone())), ("Tangent", fld(r[2].clone())), ("Normal", fld(r[3].clone()))])
        }
        "GeometryNodeMeshToCurve" => {
            let g = ev.geo(ni, "Mesh")?;
            let mut o = Geometry::default();
            if let Some(m) = &g.mesh {
                let sel = selection(ev, ni, Comp::Mesh(m), Dom::Edge)?;
                o.curves = Some(mesh_to_curve(m, &sel));
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeFillCurve" => {
            let g = ev.geo(ni, "Curve")?;
            let mut o = Geometry::default();
            if let Some(c) = &g.curves {
                o.mesh = Some(fill_curve(c, node.ps("mode") == "NGONS"));
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeSubdivideCurve" => {
            let g = ev.geo(ni, "Curve")?;
            let cuts = ev.field(ni, "Cuts")?;
            let mut o = Geometry::default();
            if let Some(c) = &g.curves {
                let cv = eval_on(&cuts, Comp::Curves(c), Dom::Point);
                o.curves = Some(subdivide_curve(c, &cv));
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeTrimCurve" => {
            let g = ev.geo(ni, "Curve")?;
            let by_len = node.ps("mode") == "LENGTH";
            let (sid, eid) = if by_len { ("Start_001", "End_001") } else { ("Start", "End") };
            let mut o = (*g).clone();
            if let Some(c) = &g.curves {
                let comp = Comp::Curves(c);
                let sel = selection(ev, ni, comp, Dom::Curve)?;
                let sv = eval_on(&ev.field(ni, sid)?, comp, Dom::Curve);
                let evv = eval_on(&ev.field(ni, eid)?, comp, Dom::Curve);
                o.curves = Some(trim_curve(c, &sel, &sv, &evv, by_len));
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeFilletCurve" => {
            let g = ev.geo(ni, "Curve")?;
            let rad = ev.field(ni, "Radius")?;
            let limit = ev.cb(ni, "Limit Radius")?;
            let count = if ev.has_input(ni, "Count") { Some(ev.field(ni, "Count")?) } else { None };
            let mut o = (*g).clone();
            if let Some(c) = &g.curves {
                let rv = eval_on(&rad, Comp::Curves(c), Dom::Point);
                let cv = match &count {
                    Some(f) => eval_on(f, Comp::Curves(c), Dom::Point),
                    None => vec![Val::I(1); c.np()],
                };
                o.curves = Some(fillet_curve(c, &rv, &cv, limit, node.ps("mode") == "BEZIER"));
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

/// rotation (quaternion) from normal (x axis) and tangent (z axis): math::from_orthonormal_axes
pub fn rotation_from_axes(normal: V3, tangent: V3) -> [f32; 4] {
    let x = normal;
    let z = tangent;
    let y = cross3(z, x);
    let m: crate::bl32::M3 = [x, y, z];
    crate::bl32::mat3_normalized_to_quat(&m)
}

pub fn mesh_to_curve(m: &Mesh, sel: &[bool]) -> Curves {
    let nv = m.nv();
    let mut adj: Vec<Vec<usize>> = vec![vec![]; nv];
    for (i, e) in m.edges.iter().enumerate() {
        if sel[i] {
            adj[e[0] as usize].push(i);
            adj[e[1] as usize].push(i);
        }
    }
    let mut used = vec![false; m.ne()];
    let mut c = Curves::new();
    let mut chains: Vec<(Vec<u32>, bool)> = vec![];
    let walk = |start: usize, used: &mut Vec<bool>| -> (Vec<u32>, bool) {
        let mut chain = vec![start as u32];
        let mut cur = start;
        loop {
            let next = adj[cur].iter().find(|&&e| !used[e]).copied();
            match next {
                None => return (chain, false),
                Some(e) => {
                    used[e] = true;
                    let ed = m.edges[e];
                    let nx = if ed[0] as usize == cur { ed[1] } else { ed[0] } as usize;
                    if nx == start {
                        return (chain, true);
                    }
                    chain.push(nx as u32);
                    cur = nx;
                }
            }
        }
    };
    for v in 0..nv {
        if adj[v].len() == 1 && !used[adj[v][0]] {
            chains.push(walk(v, &mut used));
        } else if adj[v].len() > 2 {
            while adj[v].iter().any(|&e| !used[e]) {
                chains.push(walk(v, &mut used));
            }
        }
    }
    for v in 0..nv {
        if adj[v].iter().any(|&e| !used[e]) {
            chains.push(walk(v, &mut used));
        }
    }
    let mut src = vec![];
    for (chain, cyc) in &chains {
        let pts: Vec<V3> = chain.iter().map(|&v| m.pos[v as usize]).collect();
        c.push_poly(&pts, *cyc);
        src.extend(chain.iter().map(|&v| v as usize));
    }
    for (name, a) in m.attrs.iter() {
        if name.starts_with("sharp") || name == "material_index" {
            continue;
        }
        let d = adapt_mesh(m, a.dom, Dom::Point, a.ty, &a.data);
        if a.dom == Dom::Point || true {
            c.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: src.iter().map(|&i| d[i].clone()).collect() });
        }
    }
    c
}

fn area2(p: &[[f32; 2]]) -> f64 {
    let mut a = 0.0f64;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] as f64 * v[1] as f64 - v[0] as f64 * u[1] as f64;
    }
    a
}

fn pip(pt: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > pt[1]) != (b[1] > pt[1]) && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// triangulate a CCW outer loop with CW hole loops (indices into `pts`) by bridging the holes (keyhole) and ear clipping
fn triangulate_with_holes(pts: &[[f32; 2]], outer: Vec<usize>, holes: Vec<Vec<usize>>) -> Vec<[usize; 3]> {
    let mut ring = outer;
    let mut holes = holes;
    // rightmost hole first
    holes.sort_by(|a, b| {
        let ma = a.iter().map(|&i| pts[i][0]).fold(f32::MIN, f32::max);
        let mb = b.iter().map(|&i| pts[i][0]).fold(f32::MIN, f32::max);
        mb.partial_cmp(&ma).unwrap()
    });
    for h in holes {
        // hole vertex with max x
        let (hk, _) = h.iter().enumerate().max_by(|a, b| pts[*a.1][0].partial_cmp(&pts[*b.1][0]).unwrap()).unwrap();
        let hp = pts[h[hk]];
        // nearest visible ring vertex to the right: cast ray +x, take the closest crossing edge, pick its endpoint with max x
        let n = ring.len();
        let mut best: Option<(f32, usize)> = None;
        for i in 0..n {
            let (a, b) = (pts[ring[i]], pts[ring[(i + 1) % n]]);
            if (a[1] <= hp[1]) != (b[1] <= hp[1]) || a[1] == hp[1] || b[1] == hp[1] {
                if a[1] == b[1] {
                    continue;
                }
                let t = (hp[1] - a[1]) / (b[1] - a[1]);
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let x = a[0] + t * (b[0] - a[0]);
                if x >= hp[0] && best.map_or(true, |(bx, _)| x < bx) {
                    let k = if a[0] > b[0] { i } else { (i + 1) % n };
                    best = Some((x, k));
                }
            }
        }
        let mut k = match best {
            Some((_, k)) => k,
            None => 0,
        };
        // among ring vertices inside the triangle (hp, crossing, k) pick the one with the smallest angle (earcut)
        let m = pts[ring[k]];
        let cross_x = best.map_or(m[0], |b| b.0);
        let (mut tan_min, mut found) = (f32::MAX, k);
        for i in 0..n {
            let q = pts[ring[i]];
            if q[0] >= hp[0].min(cross_x) && q[0] <= m[0].max(cross_x) && q != m {
                let tri = [hp, [cross_x, hp[1]], m];
                let s0 = ((tri[1][0] - tri[0][0]) * (q[1] - tri[0][1]) - (tri[1][1] - tri[0][1]) * (q[0] - tri[0][0])) as f64;
                let s1 = ((tri[2][0] - tri[1][0]) * (q[1] - tri[1][1]) - (tri[2][1] - tri[1][1]) * (q[0] - tri[1][0])) as f64;
                let s2 = ((tri[0][0] - tri[2][0]) * (q[1] - tri[2][1]) - (tri[0][1] - tri[2][1]) * (q[0] - tri[2][0])) as f64;
                let inside = (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0);
                if inside {
                    let t = ((q[1] - hp[1]).abs()) / (q[0] - hp[0]).abs().max(1e-30);
                    if t < tan_min {
                        tan_min = t;
                        found = i;
                    }
                }
            }
        }
        k = found;
        // splice: ring[..=k], hole starting at hk (full cycle), hole[hk] again, ring[k..]
        let mut nr: Vec<usize> = ring[..=k].to_vec();
        for j in 0..=h.len() {
            nr.push(h[(hk + j) % h.len()]);
        }
        nr.push(ring[k]);
        nr.extend_from_slice(&ring[k + 1..]);
        ring = nr;
    }
    // ear clipping
    let mut tris = vec![];
    let mut idx: Vec<usize> = ring;
    let mut guard = 0;
    while idx.len() > 3 && guard < 100000 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            let cr = (b[0] - a[0]) as f64 * (c[1] - a[1]) as f64 - (b[1] - a[1]) as f64 * (c[0] - a[0]) as f64;
            if cr <= 0.0 {
                continue;
            }
            let mut ok = true;
            for &j in &idx {
                let q = pts[j];
                if q == a || q == b || q == c {
                    continue;
                }
                let s0 = (b[0] - a[0]) as f64 * (q[1] - a[1]) as f64 - (b[1] - a[1]) as f64 * (q[0] - a[0]) as f64;
                let s1 = (c[0] - b[0]) as f64 * (q[1] - b[1]) as f64 - (c[1] - b[1]) as f64 * (q[0] - b[0]) as f64;
                let s2 = (a[0] - c[0]) as f64 * (q[1] - c[1]) as f64 - (a[1] - c[1]) as f64 * (q[0] - c[0]) as f64;
                if s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0 {
                    ok = false;
                    break;
                }
            }
            if ok {
                tris.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // degenerate: drop a vertex
            idx.remove(0);
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    tris
}

pub fn fill_curve(c: &Curves, ngons: bool) -> Mesh {
    if !ngons && c.nc() > 0 {
        // Blender: CDT (INSIDE_WITH_HOLES) over all curves of the geometry, positions in double
        let mut pts: Vec<[f64; 2]> = vec![];
        let mut faces: Vec<Vec<usize>> = vec![];
        for k in 0..c.nc() {
            let e = eval_curve(c, k);
            let base = pts.len();
            for p in &e.pos {
                pts.push([p[0] as f64, p[1] as f64]);
            }
            faces.push((base..pts.len()).collect());
        }
        let (v, ed, f) = super::cdt::delaunay_inside_with_holes(&pts, &faces);
        let pos: Vec<V3> = v.iter().map(|p| [p[0] as f32, p[1] as f32, 0.0]).collect();
        let fl: Vec<Vec<u32>> = f.iter().map(|r| r.iter().map(|&x| x as u32).collect()).collect();
        let el: Vec<[u32; 2]> = ed.iter().map(|e| [e.0 as u32, e.1 as u32]).collect();
        let mut mm = Mesh::from_faces(pos, &fl, &el);
        set_sharp_all(&mut mm);
        return mm;
    }
    fill_curve_legacy(c, ngons)
}

fn fill_curve_legacy(c: &Curves, ngons: bool) -> Mesh {
    let mut m = Mesh::new();
    let mut faces: Vec<Vec<u32>> = vec![];
    let mut loops: Vec<(u32, Vec<[f32; 2]>)> = vec![];
    for k in 0..c.nc() {
        let e = eval_curve(c, k);
        let base = m.pos.len() as u32;
        // coincident points (within the CDT epsilon) are merged
        let mut up: Vec<V3> = vec![];
        for p in &e.pos {
            if up.last().map_or(true, |q| len3(sub3(*p, *q)) > 1e-5) {
                up.push(*p);
            }
        }
        while up.len() > 1 && len3(sub3(up[0], *up.last().unwrap())) <= 1e-5 {
            up.pop();
        }
        m.pos.extend_from_slice(&up);
        if up.len() < 3 {
            continue;
        }
        loops.push((base, up.iter().map(|p| [p[0], p[1]]).collect()));
    }
    if ngons {
        for (base, l) in &loops {
            faces.push((0..l.len() as u32).map(|i| base + i).collect());
        }
    } else {
        // nesting (even-odd): depth = number of other loops containing the first vertex; parent = smallest containing loop
        let nl = loops.len();
        let mut depth = vec![0usize; nl];
        let mut parent = vec![usize::MAX; nl];
        for i in 0..nl {
            let mut best_area = f64::MAX;
            for j in 0..nl {
                if i != j && pip(loops[i].1[0], &loops[j].1) {
                    depth[i] += 1;
                    let a = area2(&loops[j].1).abs();
                    if a < best_area {
                        best_area = a;
                        parent[i] = j;
                    }
                }
            }
        }
        for o in 0..nl {
            if depth[o] % 2 != 0 {
                continue;
            }
            // local point table
            let mut pts: Vec<[f32; 2]> = vec![];
            let mut gid: Vec<u32> = vec![];
            let mut add = |l: usize, rev: bool, pts: &mut Vec<[f32; 2]>, gid: &mut Vec<u32>| -> Vec<usize> {
                let n = loops[l].1.len();
                let mut ids = vec![];
                let order: Vec<usize> = if rev { (0..n).rev().collect() } else { (0..n).collect() };
                for i in order {
                    ids.push(pts.len());
                    pts.push(loops[l].1[i]);
                    gid.push(loops[l].0 + i as u32);
                }
                ids
            };
            let ccw0 = area2(&loops[o].1) > 0.0;
            let outer = add(o, !ccw0, &mut pts, &mut gid);
            let mut holes = vec![];
            for h in 0..nl {
                if parent[h] == o && depth[h] == depth[o] + 1 {
                    let hccw = area2(&loops[h].1) > 0.0;
                    holes.push(add(h, hccw, &mut pts, &mut gid));
                }
            }
            let ccw = area2(&loops[o].1) > 0.0;
            if holes.is_empty() {
                let co: Vec<[f32; 2]> = outer.iter().map(|&i| pts[i]).collect();
                let mut tris = crate::assets::polyfill::polyfill_calc(&co);
                let orig = tris.clone();
                delaunay_flip(&co, &mut tris);
                let valid = |t: &Vec<[u32; 3]>| t.iter().all(|t| {
                    let (a, b, c) = (co[t[0] as usize], co[t[1] as usize], co[t[2] as usize]);
                    (b[0] - a[0]) as f64 * (c[1] - a[1]) as f64 - (b[1] - a[1]) as f64 * (c[0] - a[0]) as f64 > 0.0
                });
                let parea = area2(&co) / 2.0;
                let tarea = |t: &Vec<[u32; 3]>| -> f64 {
                    t.iter().map(|t| {
                        let (a, b, c) = (co[t[0] as usize], co[t[1] as usize], co[t[2] as usize]);
                        ((b[0] - a[0]) as f64 * (c[1] - a[1]) as f64 - (b[1] - a[1]) as f64 * (c[0] - a[0]) as f64) / 2.0
                    }).sum()
                };
                if !(valid(&tris) && (tarea(&tris) - parea).abs() <= 1e-6 * parea.abs()) {
                    tris = orig;
                    if !(valid(&tris) && (tarea(&tris) - parea).abs() <= 1e-6 * parea.abs()) {
                        let all: Vec<usize> = (0..co.len()).collect();
                        tris = triangulate_with_holes(&co, all, vec![]).into_iter().map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect();
                    }
                }
                for t in tris {
                    faces.push(vec![gid[outer[t[0] as usize]], gid[outer[t[1] as usize]], gid[outer[t[2] as usize]]]);
                }
            } else {
                for t in triangulate_with_holes(&pts, outer, holes) {
                    faces.push(vec![gid[t[0]], gid[t[1]], gid[t[2]]]);
                }
            }
        }
    }
    let pos = m.pos.clone();
    let mut mm = Mesh::from_faces(pos, &faces, &[]);
    set_sharp_all(&mut mm);
    mm
}

pub fn subdivide_curve(c: &Curves, cuts: &[Val]) -> Curves {
    let mut o = Curves::new();
    o.has_radius = c.has_radius;
    for k in 0..c.nc() {
        let r = c.range(k);
        let n = r.len();
        let nseg = if c.cyclic[k] { n } else { n.saturating_sub(1) };
        let mut pts = vec![];
        let mut rad = vec![];
        let mut til = vec![];
        let mut at: Vec<Vec<Val>> = c.attrs.values().filter(|a| a.dom == Dom::Point).map(|_| vec![]).collect();
        let anames: Vec<(&String, &Attr)> = c.attrs.iter().filter(|(_, a)| a.dom == Dom::Point).collect();
        for i in 0..n {
            let p = r.start + i;
            pts.push(c.pos[p]);
            rad.push(c.radius[p]);
            til.push(c.tilt[p]);
            for (ai, (_, a)) in anames.iter().enumerate() {
                at[ai].push(a.data[p].clone());
            }
            if i < nseg {
                let q = r.start + (i + 1) % n;
                let nc = cuts[p].i().max(0);
                for j in 0..nc {
                    let f = (j + 1) as f32 / (nc + 1) as f32;
                    pts.push(lerp3(c.pos[p], c.pos[q], f));
                    rad.push(c.radius[p] * (1.0 - f) + c.radius[q] * f);
                    til.push(c.tilt[p] * (1.0 - f) + c.tilt[q] * f);
                    for (ai, (_, a)) in anames.iter().enumerate() {
                        at[ai].push(mix_val(&a.data[p], &a.data[q], f, a.ty));
                    }
                }
            }
        }
        o.push_poly(&pts, c.cyclic[k]);
        let base = o.pos.len() - pts.len();
        for (i, v) in rad.iter().enumerate() {
            o.radius[base + i] = *v;
            o.tilt[base + i] = til[i];
        }
        for (ai, (name, a)) in anames.iter().enumerate() {
            let e = o.attrs.entry((*name).clone()).or_insert_with(|| Attr { dom: Dom::Point, ty: a.ty, data: vec![] });
            e.data.extend(at[ai].drain(..));
        }
    }
    o
}

/// Trim of an open Bezier spline keeps it a Bezier spline: the cut points are located on the evaluated polyline (index + factor),
/// converted to segment parameters and the control points / handles are split with de Casteljau (geometry/trim_curves.cc).
fn trim_bezier(c: &Curves, k: usize, e: &EvalCurve, sa: usize, fa: f32, sb: usize, fb: f32, o: &mut Curves) {
    let r = c.range(k);
    let lerp = |a: V3, b: V3, t: f32| -> V3 { [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t] };
    let seg_param = |idx: usize, f: f32| -> (usize, f32) {
        let idx = idx.min(e.interp.len() - 1);
        let (seg, t0) = e.interp[idx];
        let res = bezier_seg_res(c, k, seg, seg + 1) as f32;
        let nseg = r.len() - 1;
        let (seg, t) = (seg, t0 + f / res);
        if seg >= nseg {
            (nseg - 1, 1.0)
        } else {
            (seg, t.min(1.0))
        }
    };
    let (seg_a, ta) = seg_param(sa, fa);
    let (seg_b, tb) = seg_param(sb, fb);
    let cub = |i: usize| (c.pos[r.start + i], c.hr[r.start + i], c.hl[r.start + i + 1], c.pos[r.start + i + 1]);
    // split cubic (p0,h0,h1,p1) at t -> left (p0,a,b,m), right (m,c,e,p1)
    let split = |q: (V3, V3, V3, V3), t: f32| -> ((V3, V3, V3, V3), (V3, V3, V3, V3)) {
        let (p0, h0, h1, p1) = q;
        let a = lerp(p0, h0, t);
        let qq = lerp(h0, h1, t);
        let ee = lerp(h1, p1, t);
        let b = lerp(a, qq, t);
        let cc = lerp(qq, ee, t);
        let m = lerp(b, cc, t);
        ((p0, a, b, m), (m, cc, ee, p1))
    };
    // (pos, handle_left, handle_right, radius, tilt)
    let mut pts: Vec<(V3, V3, V3, f32, f32)> = vec![];
    let rad = |i: usize, j: usize, t: f32| (c.radius[r.start + i] + (c.radius[r.start + j] - c.radius[r.start + i]) * t, c.tilt[r.start + i] + (c.tilt[r.start + j] - c.tilt[r.start + i]) * t);
    if seg_a == seg_b {
        let (left, _) = split(cub(seg_a), tb);
        let tt = if tb > 0.0 { ta / tb } else { 0.0 };
        let (_, right) = split(left, tt);
        let (ra, rb) = (rad(seg_a, seg_a + 1, ta), rad(seg_a, seg_a + 1, tb));
        pts.push((right.0, right.0, right.1, ra.0, ra.1));
        pts.push((right.3, right.2, right.3, rb.0, rb.1));
    } else {
        let (_, right) = split(cub(seg_a), ta);
        let ra = rad(seg_a, seg_a + 1, ta);
        pts.push((right.0, right.0, right.1, ra.0, ra.1));
        for i in seg_a + 1..=seg_b {
            let (pos, mut hl, mut hr) = (c.pos[r.start + i], c.hl[r.start + i], c.hr[r.start + i]);
            if i == seg_a + 1 {
                hl = right.2;
            }
            let mut rd = (c.radius[r.start + i], c.tilt[r.start + i]);
            if i == seg_b {
                let (left, _) = split(cub(seg_b), tb);
                // this point keeps its position; the trimmed end is a new point at tb
                pts.push((pos, hl, hr, rd.0, rd.1));
                let re = rad(seg_b, seg_b + 1, tb);
                if let Some(l) = pts.last_mut() {
                    l.2 = left.1;
                }
                let _ = &mut hr;
                pts.push((left.3, left.2, left.3, re.0, re.1));
                rd = re;
                let _ = rd;
            } else {
                pts.push((pos, hl, hr, rd.0, rd.1));
            }
        }
    }
    for (p, hl, hr, rd, tl) in &pts {
        o.pos.push(*p);
        o.hl.push(*hl);
        o.hr.push(*hr);
        o.htl.push(0);
        o.htr.push(0);
        o.radius.push(*rd);
        o.tilt.push(*tl);
    }
    o.kind.push(CKind::Bezier);
    o.cyclic.push(false);
    o.res.push(c.res[k]);
    o.off.push(o.pos.len() as u32);
}

pub fn trim_curve(c: &Curves, sel: &[bool], start: &[Val], end: &[Val], by_len: bool) -> Curves {
    let mut o = Curves::new();
    o.has_radius = c.has_radius;
    for k in 0..c.nc() {
        let r = c.range(k);
        if !sel[k] || c.cyclic[k] && false {
            let pts: Vec<V3> = c.pos[r.clone()].to_vec();
            o.push_kind(c.kind[k], &pts, c.cyclic[k]);
            let base = o.pos.len() - pts.len();
            for (i, p) in r.clone().enumerate() {
                o.radius[base + i] = c.radius[p];
                o.tilt[base + i] = c.tilt[p];
            }
            continue;
        }
        let e = eval_curve(c, k);
        let acc = accumulate_lengths(&e.pos, c.cyclic[k]);
        let total = acc.last().copied().unwrap_or(0.0);
        let (mut a, mut b) = if by_len { (start[k].f(), end[k].f()) } else { (start[k].f() * total, end[k].f() * total) };
        a = a.clamp(0.0, total);
        b = b.clamp(0.0, total);
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        let (sa, fa) = sample_at_length(&acc, a);
        let (sb, fb) = sample_at_length(&acc, b);
        if c.kind[k] == CKind::Bezier && !c.cyclic[k] && r.len() > 1 {
            trim_bezier(c, k, &e, sa, fa, sb, fb, &mut o);
            continue;
        }
        // samples: start, interior evaluated points, end
        let mut samples = vec![(sa, fa)];
        for i in (sa + 1)..=sb {
            if i < e.pos.len() && !(i == sb && fb == 0.0) {
                samples.push((i, 0.0));
            }
        }
        samples.push((sb, fb));
        let s = sample_curve_at(c, k, &samples);
        push_sampled(&mut o, &s, false);
    }
    o
}

/// angle_v3v3 (normalizes both inputs with normalize_v3, then angle_normalized_v3v3)
fn angle_v3v3(a: V3, b: V3) -> f32 {
    let (a, b) = (norm3_c(a), norm3_c(b));
    let saasin = |x: f32| x.clamp(-1.0, 1.0).asin();
    if dot3(a, b) >= 0.0 {
        2.0 * saasin(len3(sub3(b, a)) / 2.0)
    } else {
        let bn = [-b[0], -b[1], -b[2]];
        (std::f64::consts::PI as f32) - 2.0 * saasin(len3(sub3(bn, a)) / 2.0)
    }
}

fn fillet_limit_radius(pp: V3, p: V3, pn: V3, ap: f32, a: f32, an: f32, rp: f32, r: f32, rn: f32) -> f32 {
    let disp = r * (a / 2.0).tan();
    let disp_p = rp * (ap / 2.0).tan();
    let seg_p = len3(sub3(p, pp));
    let tot_p = disp_p + disp;
    let f_p = safe_div(seg_p, tot_p).clamp(0.0, 1.0);
    let disp_n = rn * (an / 2.0).tan();
    let seg_n = len3(sub3(p, pn));
    let tot_n = disp_n + disp;
    let f_n = safe_div(seg_n, tot_n).clamp(0.0, 1.0);
    r * f_p.min(f_n)
}

/// vector handle: interpolate(p, other, 1/3)
fn vector_handle(p: V3, other: V3) -> V3 {
    let t = 1.0f32 / 3.0;
    [p[0] * (1.0 - t) + other[0] * t, p[1] * (1.0 - t) + other[1] * t, p[2] * (1.0 - t) + other[2] * t]
}

/// Port of blender::geometry::fillet_curves (geometry/intern/fillet_curves.cc).  `counts` = per point Count field
/// (ignored in Bezier mode, where it is 1).
pub fn fillet_curve(c: &Curves, radius: &[Val], counts: &[Val], limit: bool, bezier: bool) -> Curves {
    let mut o = Curves::new();
    o.has_radius = c.has_radius;
    let pi32 = std::f64::consts::PI as f32;
    let _ = pi32;
    let has_bezier = c.kind.iter().any(|k| *k == CKind::Bezier);
    // destination point <- source point, for attribute gathering
    let mut gather: Vec<usize> = vec![];
    for k in 0..c.nc() {
        let r = c.range(k);
        let n = r.len();
        let cyc = c.cyclic[k];
        let pos: Vec<V3> = c.pos[r.clone()].to_vec();
        if n == 0 {
            o.push_kind(c.kind[k], &[], cyc);
            continue;
        }
        let mut cnt: Vec<usize> = (0..n).map(|i| if bezier { 2 } else { (counts[r.start + i].i().max(0) + 1) as usize }).collect();
        if !cyc {
            cnt[0] = 1;
            cnt[n - 1] = 1;
        }
        for i in 0..n {
            if radius[r.start + i].f() == 0.0 {
                cnt[i] = 1;
            }
        }
        // directions and angles
        let mut dirs = vec![[0.0f32; 3]; n];
        for i in 0..n.saturating_sub(1) {
            dirs[i] = norm3(sub3(pos[i + 1], pos[i]));
        }
        dirs[n - 1] = norm3(sub3(pos[0], pos[n - 1]));
        let neg = |v: V3| [-v[0], -v[1], -v[2]];
        let mut angles = vec![0.0f32; n];
        angles[0] = (std::f64::consts::PI - angle_v3v3(neg(dirs[n - 1]), dirs[0]) as f64) as f32;
        for i in 1..n {
            angles[i] = (std::f64::consts::PI - angle_v3v3(neg(dirs[i - 1]), dirs[i]) as f64) as f32;
        }
        let rin: Vec<f32> = (0..n).map(|i| radius[r.start + i].f()).collect();
        let radii: Vec<f32> = if limit {
            let mut rc = vec![0.0f32; n];
            if cyc {
                rc[0] = fillet_limit_radius(pos[n - 1], pos[0], pos[1 % n], angles[n - 1], angles[0], angles[1 % n], rin[n - 1], rin[0], rin[1 % n]);
                for i in 1..n.saturating_sub(1) {
                    rc[i] = fillet_limit_radius(pos[i - 1], pos[i], pos[i + 1], angles[i - 1], angles[i], angles[i + 1], rin[i - 1], rin[i], rin[i + 1]);
                }
                if n >= 2 {
                    rc[n - 1] = fillet_limit_radius(pos[n - 2], pos[n - 1], pos[0], angles[n - 2], angles[n - 1], angles[0], rin[n - 2], rin[n - 1], rin[0]);
                }
            } else {
                let last = n - 1;
                rc[0] = 0.0;
                for i in 1..last.max(1) {
                    let (ip, in_) = (i - 1, i + 1);
                    let rp = if ip == 0 { 0.0 } else { rin[ip] };
                    let rn = if in_ == last { 0.0 } else { rin[in_] };
                    rc[i] = fillet_limit_radius(pos[ip], pos[i], pos[in_], angles[ip], angles[i], angles[in_], rp, rin[i], rn);
                }
                rc[last] = 0.0;
            }
            rc
        } else {
            rin.clone()
        };
        // positions
        let mut dst: Vec<V3> = vec![];
        let mut arcs: Vec<std::ops::Range<usize>> = vec![];
        for i in 0..n {
            let start = dst.len();
            if cnt[i] == 1 {
                dst.push(pos[i]);
                gather.push(r.start + i);
                arcs.push(start..start + 1);
                continue;
            }
            let ip = if i == 0 { n - 1 } else { i - 1 };
            let angle = angles[i];
            let rad = radii[i];
            let displacement = rad * (angle / 2.0).tan();
            let prev_dir = neg(dirs[ip]);
            let next_dir = dirs[i];
            let arc_start = add3(pos[i], mul3(prev_dir, displacement));
            let arc_end = add3(pos[i], mul3(next_dir, displacement));
            let nmid = cnt[i] - 2;
            dst.push(arc_start);
            if nmid > 0 {
                let axis = neg(norm3(cross3(prev_dir, next_dir)));
                let sum = add3(next_dir, prev_dir);
                let center_direction = norm3(mul3(sum, 0.5));
                let distance_to_center = (rad * rad + displacement * displacement).sqrt();
                let center = add3(pos[i], mul3(center_direction, distance_to_center));
                let segment_angle = angle / (nmid + 1) as f32;
                for j in 0..nmid {
                    let ang = segment_angle * (j + 1) as f32;
                    let (si, co) = (ang.sin(), ang.cos());
                    let ico = 1.0 - co;
                    let nsi = [axis[0] * si, axis[1] * si, axis[2] * si];
                    let n00 = (axis[0] * axis[0]) * ico;
                    let n01 = (axis[0] * axis[1]) * ico;
                    let n11 = (axis[1] * axis[1]) * ico;
                    let n02 = (axis[0] * axis[2]) * ico;
                    let n12 = (axis[1] * axis[2]) * ico;
                    let n22 = (axis[2] * axis[2]) * ico;
                    let m = [[n00 + co, n01 + nsi[2], n02 - nsi[1]], [n01 - nsi[2], n11 + co, n12 + nsi[0]], [n02 + nsi[1], n12 - nsi[0], n22 + co]];
                    let v = sub3(arc_start, center);
                    let rv = [m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2], m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2], m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2]];
                    dst.push(add3(rv, center));
                }
            }
            dst.push(arc_end);
            for _ in 0..cnt[i] {
                gather.push(r.start + i);
            }
            arcs.push(start..dst.len());
        }
        let first_dst = o.pos.len();
        o.push_kind(c.kind[k], &dst, cyc);
        let kk = o.nc() - 1;
        o.res[kk] = c.res[k];
        // handles
        if c.kind[k] == CKind::Bezier && has_bezier {
            let nd = dst.len();
            let last = nd - 1;
            for i in 0..n {
                let arc = arcs[i].clone();
                let rs = r.start + i;
                if arc.len() == 1 {
                    o.hl[first_dst + arc.start] = c.hl[rs];
                    o.hr[first_dst + arc.start] = c.hr[rs];
                    o.htl[first_dst + arc.start] = c.htl[rs];
                    o.htr[first_dst + arc.start] = c.htr[rs];
                    continue;
                }
                let (a, b) = (arc.start, arc.end - 1);
                let prev = if a == 0 { last } else { a - 1 };
                let next = if b == last { 0 } else { b + 1 };
                if bezier {
                    let ip = if i == 0 { n - 1 } else { i - 1 };
                    let prev_dir = neg(dirs[ip]);
                    let next_dir = dirs[i];
                    o.hl[first_dst + a] = vector_handle(dst[a], dst[prev]);
                    o.hr[first_dst + b] = vector_handle(dst[b], dst[next]);
                    o.htl[first_dst + a] = 2;
                    o.htr[first_dst + b] = 2;
                    let handle_length = (4.0f32 / 3.0) * radii[i] * (angles[i] / 4.0).tan();
                    o.hr[first_dst + a] = sub3(dst[a], mul3(prev_dir, handle_length));
                    o.hl[first_dst + b] = sub3(dst[b], mul3(next_dir, handle_length));
                    o.htr[first_dst + a] = 3;
                    o.htl[first_dst + b] = 3;
                } else {
                    for j in arc.clone() {
                        o.htl[first_dst + j] = 2;
                        o.htr[first_dst + j] = 2;
                    }
                    o.hl[first_dst + a] = vector_handle(dst[a], dst[prev]);
                    o.hr[first_dst + b] = vector_handle(dst[b], dst[next]);
                    for j in (a + 1)..b {
                        o.hr[first_dst + j] = vector_handle(dst[j], dst[j - 1]);
                        o.hl[first_dst + j] = vector_handle(dst[j], dst[j + 1]);
                    }
                }
            }
        }
    }
    // point attributes
    for j in 0..gather.len() {
        let s = gather[j];
        o.radius[j] = c.radius[s];
        o.tilt[j] = c.tilt[s];
    }
    for (name, a) in c.attrs.iter() {
        if a.dom == Dom::Point {
            o.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: gather.iter().map(|&s| a.data[s].clone()).collect() });
        } else {
            o.attrs.insert(name.clone(), a.clone());
        }
    }
    o
}

fn angle_norm(a: V3, b: V3) -> f32 {
    if dot3(a, b) >= 0.0 {
        2.0 * (len3(sub3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (len3(add3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

fn rotate_around(dir: V3, axis: V3, angle: f32) -> V3 {
    let axis_scaled = mul3(axis, dot3(dir, axis));
    let diff = sub3(dir, axis_scaled);
    let c = cross3(axis, diff);
    add3(add3(axis_scaled, mul3(diff, angle.cos())), mul3(c, angle.sin()))
}


/// constrained Delaunay by Lawson flips: polygon boundary edges (i, i+1) are constraints
pub fn delaunay_flip(co: &[[f32; 2]], tris: &mut Vec<[u32; 3]>) {
    let n = co.len() as u32;
    let is_boundary = |a: u32, b: u32| -> bool { (a + 1) % n == b || (b + 1) % n == a };
    let p = |i: u32| [co[i as usize][0] as f64, co[i as usize][1] as f64];
    let orient = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let incircle = |a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]| -> f64 {
        let (ax, ay) = (a[0] - d[0], a[1] - d[1]);
        let (bx, by) = (b[0] - d[0], b[1] - d[1]);
        let (cx, cy) = (c[0] - d[0], c[1] - d[1]);
        let det = (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay) + (cx * cx + cy * cy) * (ax * by - bx * ay);
        if orient(a, b, c) > 0.0 { det } else { -det }
    };
    for _ in 0..(tris.len() * tris.len() + 10) {
        let mut flipped = false;
        'outer: for i in 0..tris.len() {
            for k in 0..3 {
                let (a, b) = (tris[i][k], tris[i][(k + 1) % 3]);
                if is_boundary(a, b) {
                    continue;
                }
                let c = tris[i][(k + 2) % 3];
                // neighbour triangle sharing edge b-a
                for j in 0..tris.len() {
                    if j == i {
                        continue;
                    }
                    for m in 0..3 {
                        if tris[j][m] == b && tris[j][(m + 1) % 3] == a {
                            let d = tris[j][(m + 2) % 3];
                            // flip a-b to c-d if d lies inside circumcircle(a,b,c) and the quad is convex
                            if incircle(p(a), p(b), p(c), p(d)) > 1e-12 && orient(p(c), p(a), p(d)) * orient(p(c), p(d), p(b)) > 0.0 {
                                tris[i] = [c, a, d];
                                tris[j] = [d, b, c];
                                flipped = true;
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        if !flipped {
            break;
        }
    }
}
