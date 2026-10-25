//! Field-valued nodes that need geometry data: sampling, mesh inputs, proximity, accumulate, color conversion.
use super::attr::*;
use super::eval::*;
use super::field::*;
use super::geom::*;
use super::load::*;
use super::math::*;
use super::nodes::*;
use super::nodes_geo::*;
use super::val::*;
use std::rc::Rc;

fn kern(f: impl Fn(&[&[Val]], usize) -> Vec<Vec<Val>> + 'static) -> Kernel {
    Rc::new(f)
}

pub fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let k = if g < b { [b, g, -1.0, 2.0 / 3.0] } else { [g, b, 0.0, -1.0 / 3.0] };
    let k2 = if r < k[0] { [k[0], r, k[3], k[0] - 0.0] } else { [r, k[0], k[2], k[1]] };
    let _ = k2;
    // straightforward version (Blender rgb_to_hsv)
    let cmax = r.max(g).max(b);
    let cmin = r.min(g).min(b);
    let cdelta = cmax - cmin;
    let v = cmax;
    if cmax != 0.0 {
        let s = cdelta / cmax;
        let mut h = 0.0;
        if s != 0.0 {
            let (rc, gc, bc) = ((cmax - r) / cdelta, (cmax - g) / cdelta, (cmax - b) / cdelta);
            h = if r == cmax {
                bc - gc
            } else if g == cmax {
                2.0 + rc - bc
            } else {
                4.0 + gc - rc
            };
            h /= 6.0;
            if h < 0.0 {
                h += 1.0;
            }
        }
        [h, s, v]
    } else {
        [0.0, 0.0, v]
    }
}

pub fn hsv_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let (h, s, v) = (c[0], c[1], c[2]);
    let nr = ((h * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
    let ng = (2.0 - (h * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
    let nb = (2.0 - (h * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
    [((nr - 1.0) * s + 1.0) * v, ((ng - 1.0) * s + 1.0) * v, ((nb - 1.0) * s + 1.0) * v]
}

pub fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let cmax = r.max(g).max(b);
    let cmin = r.min(g).min(b);
    let l = (cmax + cmin) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if cmax != cmin {
        let d = cmax - cmin;
        s = if l > 0.5 { d / (2.0 - cmax - cmin) } else { d / (cmax + cmin) };
        if cmax == r {
            h = (g - b) / d + if g < b { 6.0 } else { 0.0 };
        } else if cmax == g {
            h = (b - r) / d + 2.0;
        } else {
            h = (r - g) / d + 4.0;
        }
        h /= 6.0;
    }
    [h, s, l]
}

pub fn hsl_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let (h, s, l) = (c[0], c[1], c[2]);
    let nr = ((h * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
    let ng = (2.0 - (h * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
    let nb = (2.0 - (h * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    [(nr - 0.5) * chroma + l, (ng - 0.5) * chroma + l, (nb - 0.5) * chroma + l]
}

/// closest point on triangle (float32 version of closest_on_tri_to_point_v3)
pub fn closest_on_tri(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ap = sub3(p, a);
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub3(p, b);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add3(a, mul3(ab, v));
    }
    let cp = sub3(p, c);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add3(a, mul3(ac, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add3(b, mul3(sub3(c, b), w));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    add3(add3(a, mul3(ab, v)), mul3(ac, w))
}

pub fn closest_on_segment(p: V3, a: V3, b: V3) -> V3 {
    let ab = sub3(b, a);
    let l = dot3(ab, ab);
    let t = if l > 0.0 { (dot3(sub3(p, a), ab) / l).clamp(0.0, 1.0) } else { 0.0 };
    add3(a, mul3(ab, t))
}

/// triangles of a mesh (fan triangulation of each face; ngon via polyfill order)
pub fn mesh_tris(m: &Mesh) -> Vec<(usize, [u32; 3])> {
    let mut t = vec![];
    for f in 0..m.nf() {
        let fv = m.face(f);
        if fv.len() == 3 {
            t.push((f, [fv[0], fv[1], fv[2]]));
        } else if fv.len() == 4 {
            t.push((f, [fv[0], fv[1], fv[2]]));
            t.push((f, [fv[0], fv[2], fv[3]]));
        } else if fv.len() > 4 {
            let co: Vec<[f32; 3]> = fv.iter().map(|&i| m.pos[i as usize]).collect();
            for tr in crate::polyfill::triangulate_ngon(&co) {
                t.push((f, [fv[tr[0] as usize], fv[tr[1] as usize], fv[tr[2] as usize]]));
            }
        }
    }
    t
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Elem {
    Points,
    Edges,
    Faces,
    FaceCenters,
}

/// nearest element: (element index, closest point, distance)
fn nearest(m: &Mesh, tris: &[(usize, [u32; 3])], elem: Elem, p: V3) -> Option<(usize, V3, f32)> {
    let mut best: Option<(usize, V3, f32)> = None;
    let tie_last = false;
    let mut consider = |idx: usize, q: V3| {
        let d = len3(sub3(p, q));
        if best.map_or(true, |b| d < b.2 || (tie_last && d == b.2)) {
            best = Some((idx, q, d));
        }
    };
    match elem {
        Elem::Points => {
            for (i, v) in m.pos.iter().enumerate() {
                consider(i, *v);
            }
        }
        Elem::Edges => {
            for (i, e) in m.edges.iter().enumerate() {
                consider(i, closest_on_segment(p, m.pos[e[0] as usize], m.pos[e[1] as usize]));
            }
        }
        Elem::FaceCenters => {
            for f in 0..m.nf() {
                let r = m.face(f);
                let mut c = [0.0f32; 3];
                for &v in r {
                    c = add3(c, m.pos[v as usize]);
                }
                consider(f, mul3(c, 1.0 / r.len() as f32));
            }
        }
        Elem::Faces => {
            for (f, t) in tris {
                consider(*f, closest_on_tri(p, m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]));
            }
        }
    }
    best
}

pub fn field_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "GeometryNodeSampleIndex" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let idx = ev.field(ni, "Index")?;
            let clamp = node.pb("clamp");
            let mut data: Vec<Val> = vec![];
            if let Some(m) = &g.mesh {
                if dom_valid(Comp::Mesh(m), dom) {
                    data = eval_on(&value, Comp::Mesh(m), dom);
                }
            }
            if data.is_empty() {
                if let Some(c) = &g.curves {
                    if dom_valid(Comp::Curves(c), dom) {
                        data = eval_on(&value, Comp::Curves(c), dom);
                    }
                }
            }
            if data.is_empty() {
                if let Some(p) = &g.points {
                    if dom == Dom::Point {
                        data = eval_on(&value, Comp::Points(p), dom);
                    }
                }
            }
            if data.is_empty() {
                if let Some(i) = &g.inst {
                    if dom == Dom::Instance {
                        data = eval_on(&value, Comp::Inst(i), dom);
                    }
                }
            }
            let data: Vec<Val> = data.iter().map(|v| v.convert(t)).collect();
            let k = kern(move |a, n| {
                vec![(0..n)
                    .map(|i| {
                        let ix = at(a[0], i).i();
                        if data.is_empty() {
                            return Val::zero(t);
                        }
                        let ix = if clamp { ix.clamp(0, data.len() as i32 - 1) } else { ix };
                        if ix < 0 || ix as usize >= data.len() {
                            Val::zero(t)
                        } else {
                            data[ix as usize].clone()
                        }
                    })
                    .collect()]
            });
            outs(node, vec![("Value", fld(mf1(k, vec![idx])))])
        }
        "GeometryNodeFieldAtIndex" => {
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let idx = ev.field(ni, "Index")?;
            let f = Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| {
                let vals = ctx.eval_n(&value);
                let ix = ctx.eval_n(&idx);
                (0..ctx.n)
                    .map(|i| {
                        let k = ix[i].i();
                        if k >= 0 && (k as usize) < vals.len() {
                            vals[k as usize].convert(t)
                        } else {
                            Val::zero(t)
                        }
                    })
                    .collect()
            })));
            outs(node, vec![("Value", fld(f))])
        }
        "GeometryNodeFieldOnDomain" => {
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let dom = Dom::from_name(node.ps("domain"));
            let f = Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| {
                let sub = Ctx::new(ctx.comp, dom);
                let vals = sub.eval_n(&value);
                match ctx.comp {
                    Comp::Mesh(m) => adapt_mesh(m, dom, ctx.dom, t, &vals),
                    Comp::Curves(c) => adapt_curves(c, dom, ctx.dom, t, &vals),
                    _ => vals,
                }
            })));
            outs(node, vec![("Value", fld(f))])
        }
        "GeometryNodeAccumulateField" => {
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let group = ev.field(ni, "Group Index")?;
            let mk = move |which: u8| {
                let value = value.clone();
                let group = group.clone();
                Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| {
                    let vals = ctx.eval_n(&value);
                    let gs = ctx.eval_n(&group);
                    let mut sums: crate::dhash::HashMap<i32, Val> = crate::dhash::HashMap::default();
                    let add = |a: &Val, b: &Val| -> Val {
                        match t {
                            Ty::I => Val::I(a.i().wrapping_add(b.i())),
                            Ty::V => {
                                let (x, y) = (a.v(), b.v());
                                Val::V([x[0] + y[0], x[1] + y[1], x[2] + y[2]])
                            }
                            _ => Val::F(a.f() + b.f()),
                        }
                    };
                    let mut lead = vec![];
                    let mut trail = vec![];
                    for i in 0..ctx.n {
                        let g = gs[i].i();
                        let prev = sums.get(&g).cloned().unwrap_or(Val::zero(t));
                        let v = vals[i].convert(t);
                        let cur = add(&prev, &v);
                        lead.push(cur.clone());
                        trail.push(prev);
                        sums.insert(g, cur);
                    }
                    match which {
                        0 => lead,
                        1 => trail,
                        _ => (0..ctx.n).map(|i| sums[&gs[i].i()].clone()).collect(),
                    }
                })))
            };
            outs(node, vec![("Leading", fld(mk(0))), ("Trailing", fld(mk(1))), ("Total", fld(mk(2)))])
        }
        "GeometryNodeInputMeshEdgeVertices" => {
            let mk = |w: u8| {
                Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| match ctx.comp {
                    Comp::Mesh(m) if ctx.dom == Dom::Edge => m
                        .edges
                        .iter()
                        .map(|e| match w {
                            0 => Val::I(e[0] as i32),
                            1 => Val::I(e[1] as i32),
                            2 => Val::V(m.pos[e[0] as usize]),
                            _ => Val::V(m.pos[e[1] as usize]),
                        })
                        .collect(),
                    _ => vec![if w < 2 { Val::I(0) } else { Val::V([0.0; 3]) }; ctx.n],
                })))
            };
            outs(node, vec![("Vertex Index 1", fld(mk(0))), ("Vertex Index 2", fld(mk(1))), ("Position 1", fld(mk(2))), ("Position 2", fld(mk(3)))])
        }
        "GeometryNodeInputMeshFaceArea" => {
            let f = Rc::new(FNode::Ctx(Rc::new(|ctx: &Ctx| match ctx.comp {
                Comp::Mesh(m) if ctx.dom == Dom::Face => (0..m.nf()).map(|f| Val::F(face_area(m, f))).collect(),
                Comp::Mesh(m) => {
                    let fa: Vec<Val> = (0..m.nf()).map(|f| Val::F(face_area(m, f))).collect();
                    adapt_mesh(m, Dom::Face, ctx.dom, Ty::F, &fa)
                }
                _ => vec![Val::F(0.0); ctx.n],
            })));
            outs(node, vec![("Area", fld(f))])
        }
        "GeometryNodeInputMeshEdgeAngle" => {
            let mk = |signed: bool| {
                Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| match ctx.comp {
                    Comp::Mesh(m) if ctx.dom == Dom::Edge => edge_angles(m, signed).iter().map(|a| Val::F(*a)).collect(),
                    _ => vec![Val::F(0.0); ctx.n],
                })))
            };
            outs(node, vec![("Unsigned Angle", fld(mk(false))), ("Signed Angle", fld(mk(true)))])
        }
        "GeometryNodeInputMeshEdgeNeighbors" | "GeometryNodeInputMeshFaceNeighbors" | "GeometryNodeInputMeshVertexNeighbors" => {
            let which = ty.to_string();
            let mk = |w: u8| {
                let which = which.clone();
                Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| match ctx.comp {
                    Comp::Mesh(m) => mesh_neighbors(m, &which, w, ctx.dom),
                    _ => vec![Val::I(0); ctx.n],
                })))
            };
            if ty.ends_with("EdgeNeighbors") {
                outs(node, vec![("Face Count", fld(mk(1)))])
            } else {
                outs(node, vec![("Vertex Count", fld(mk(0))), ("Face Count", fld(mk(1)))])
            }
        }
        "GeometryNodeInputMaterialIndex" => outs(node, vec![("Material Index", fld(Rc::new(FNode::Named("material_index".into(), Ty::I))))]),
        "GeometryNodeInputShadeSmooth" => {
            let f = Rc::new(FNode::Named("sharp_face".into(), Ty::B));
            outs(node, vec![("Smooth", fld(map1(&f, |v| Val::B(!v.b()))))])
        }
        "GeometryNodeInputEdgeSmooth" => {
            let f = Rc::new(FNode::Named("sharp_edge".into(), Ty::B));
            outs(node, vec![("Smooth", fld(map1(&f, |v| Val::B(!v.b()))))])
        }
        "FunctionNodeFloatToInt" => {
            let mode = node.ps("rounding_mode").to_string();
            let f = ev.field(ni, "Float")?;
            outs(
                node,
                vec![("Integer", fld(map1(&f, move |v| {
                    let x = v.f();
                    Val::I(match mode.as_str() {
                        "FLOOR" => x.floor(),
                        "CEILING" => x.ceil(),
                        "TRUNCATE" => x.trunc(),
                        _ => (x + 0.5).floor(),
                    } as i32)
                })))],
            )
        }
        "FunctionNodeSeparateColor" => {
            let mode = node.ps("mode").to_string();
            let c = ev.field(ni, "Color")?;
            let k = kern(move |a, n| {
                let mut o = vec![vec![]; 4];
                for i in 0..n {
                    let col = at(a[0], i).c();
                    let rgb = [col[0], col[1], col[2]];
                    let v = match mode.as_str() {
                        "HSV" => rgb_to_hsv(rgb),
                        "HSL" => rgb_to_hsl(rgb),
                        _ => rgb,
                    };
                    for k in 0..3 {
                        o[k].push(Val::F(v[k]));
                    }
                    o[3].push(Val::F(col[3]));
                }
                o
            });
            let r = mf(k, vec![c], 4);
            outs(node, vec![("Red", fld(r[0].clone())), ("Green", fld(r[1].clone())), ("Blue", fld(r[2].clone())), ("Alpha", fld(r[3].clone()))])
        }
        "FunctionNodeCombineColor" => {
            let mode = node.ps("mode").to_string();
            let (a, b, c, d) = (ev.field(ni, "Red")?, ev.field(ni, "Green")?, ev.field(ni, "Blue")?, ev.field(ni, "Alpha")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let v = [at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).f()];
                        let rgb = match mode.as_str() {
                            "HSV" => hsv_to_rgb(v),
                            "HSL" => hsl_to_rgb(v),
                            _ => v,
                        };
                        Val::C([rgb[0], rgb[1], rgb[2], at(x[3], i).f()])
                    })
                    .collect()]
            });
            outs(node, vec![("Color", fld(mf1(k, vec![a, b, c, d])))])
        }
        "GeometryNodeProximity" => {
            let tg = ev.geo(ni, "Target")?;
            let elem = match node.ps("target_element") {
                "POINTS" => Elem::Points,
                "EDGES" => Elem::Edges,
                _ => Elem::Faces,
            };
            let src = ev.field(ni, "Source Position")?;
            let mesh = tg.mesh.clone();
            let tris = mesh.as_ref().map(mesh_tris).unwrap_or_default();
            let k = kern(move |a, n| {
                let mut o = vec![vec![], vec![], vec![]];
                for i in 0..n {
                    let p = at(a[0], i).v();
                    match mesh.as_ref().and_then(|m| nearest(m, &tris, elem, p)) {
                        Some((_, q, d)) => {
                            o[0].push(Val::V(q));
                            o[1].push(Val::F(d));
                            o[2].push(Val::B(true));
                        }
                        None => {
                            o[0].push(Val::V([0.0; 3]));
                            o[1].push(Val::F(0.0));
                            o[2].push(Val::B(false));
                        }
                    }
                }
                o
            });
            let r = mf(k, vec![src], 3);
            outs(node, vec![("Position", fld(r[0].clone())), ("Distance", fld(r[1].clone())), ("Is Valid", fld(r[2].clone()))])
        }
        "GeometryNodeSampleNearest" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let src = ev.field(ni, "Sample Position")?;
            let mesh = g.mesh.clone();
            let tris = mesh.as_ref().map(mesh_tris).unwrap_or_default();
            let ptsrc = g.points.clone();
            let k = kern(move |a, n| {
                vec![(0..n)
                    .map(|i| {
                        let p = at(a[0], i).v();
                        if let Some(m) = mesh.as_ref() {
                            let elem = match dom {
                                Dom::Edge => Elem::Edges,
                                Dom::Face => Elem::Faces,
                                _ => Elem::Points,
                            };
                            if let Some((idx, _, _)) = nearest(m, &tris, elem, p) {
                                if dom == Dom::Corner {
                                    // first corner of the nearest vertex
                                    return Val::I(m.cv.iter().position(|&v| v as usize == idx).unwrap_or(0) as i32);
                                }
                                return Val::I(idx as i32);
                            }
                        }
                        if let Some(pc) = ptsrc.as_ref() {
                            let mut best = (0usize, f32::MAX);
                            for (j, q) in pc.pos.iter().enumerate() {
                                let d = len3(sub3(p, *q));
                                if d < best.1 {
                                    best = (j, d);
                                }
                            }
                            return Val::I(best.0 as i32);
                        }
                        Val::I(0)
                    })
                    .collect()]
            });
            outs(node, vec![("Index", fld(mf1(k, vec![src])))])
        }
        "GeometryNodeRaycast" => {
            let g = ev.geo(ni, "Target Geometry")?;
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Attribute")?;
            let (src, dir, len) = (ev.field(ni, "Source Position")?, ev.field(ni, "Ray Direction")?, ev.field(ni, "Ray Length")?);
            let mesh = g.mesh.clone();
            let tris = mesh.as_ref().map(mesh_tris).unwrap_or_default();
            let vals: Vec<Val> = match &mesh {
                Some(m) => eval_on(&value, Comp::Mesh(m), Dom::Point).iter().map(|v| v.convert(t)).collect(),
                None => vec![],
            };
            let k = kern(move |a, n| {
                let mut o = vec![vec![], vec![], vec![], vec![], vec![]];
                for i in 0..n {
                    let origin = at(a[0], i).v();
                    let d = norm3(at(a[1], i).v());
                    let max_len = at(a[2], i).f();
                    let mut best: Option<(f32, usize, [f32; 3])> = None;
                    if let Some(m) = mesh.as_ref() {
                        for (ti, (_, tr)) in tris.iter().enumerate() {
                            let (v0, v1, v2) = (m.pos[tr[0] as usize], m.pos[tr[1] as usize], m.pos[tr[2] as usize]);
                            let e1 = sub3(v1, v0);
                            let e2 = sub3(v2, v0);
                            let p = cross3(d, e2);
                            let det = dot3(e1, p);
                            if det > -1e-12 && det < 1e-12 {
                                continue;
                            }
                            let f = 1.0 / det;
                            let s_ = sub3(origin, v0);
                            let u = f * dot3(s_, p);
                            if u < 0.0 || u > 1.0 {
                                continue;
                            }
                            let q = cross3(s_, e1);
                            let v = f * dot3(d, q);
                            if v < 0.0 || u + v > 1.0 {
                                continue;
                            }
                            let tt = f * dot3(e2, q);
                            if tt < 0.0 || tt > max_len {
                                continue;
                            }
                            if best.map_or(true, |b| tt < b.0) {
                                best = Some((tt, ti, [1.0 - u - v, u, v]));
                            }
                        }
                    }
                    match (best, mesh.as_ref()) {
                        (Some((tt, ti, w)), Some(m)) => {
                            let tr = tris[ti].1;
                            let (v0, v1, v2) = (m.pos[tr[0] as usize], m.pos[tr[1] as usize], m.pos[tr[2] as usize]);
                            let nrm = norm3_c(cross3(sub3(v0, v1), sub3(v1, v2)));
                            o[0].push(Val::B(true));
                            o[1].push(Val::V(add3(origin, mul3(d, tt))));
                            o[2].push(Val::V(nrm));
                            o[3].push(Val::F(tt));
                            let val = match t {
                                Ty::V => {
                                    let (x, y, z) = (vals[tr[0] as usize].v(), vals[tr[1] as usize].v(), vals[tr[2] as usize].v());
                                    Val::V([x[0] * w[0] + y[0] * w[1] + z[0] * w[2], x[1] * w[0] + y[1] * w[1] + z[1] * w[2], x[2] * w[0] + y[2] * w[1] + z[2] * w[2]])
                                }
                                Ty::F => Val::F(vals[tr[0] as usize].f() * w[0] + vals[tr[1] as usize].f() * w[1] + vals[tr[2] as usize].f() * w[2]),
                                _ => vals[tr[0] as usize].clone(),
                            };
                            o[4].push(val);
                        }
                        _ => {
                            o[0].push(Val::B(false));
                            o[1].push(Val::V([0.0; 3]));
                            o[2].push(Val::V([0.0; 3]));
                            o[3].push(Val::F(max_len));
                            o[4].push(Val::zero(t));
                        }
                    }
                }
                o
            });
            let r = mf(k, vec![src, dir, len], 5);
            outs(node, vec![("Is Hit", fld(r[0].clone())), ("Hit Position", fld(r[1].clone())), ("Hit Normal", fld(r[2].clone())), ("Hit Distance", fld(r[3].clone())), ("Attribute", fld(r[4].clone()))])
        }
        "GeometryNodeSampleNearestSurface" => {
            let g = ev.geo(ni, "Mesh")?;
            let t = Ty::from_data_type(node.ps("data_type"));
            let value = ev.field(ni, "Value")?;
            let src = ev.field(ni, "Sample Position")?;
            let mesh = g.mesh.clone();
            let tris = mesh.as_ref().map(mesh_tris).unwrap_or_default();
            // value on corner domain interpolated barycentrically; here: evaluated on the point domain and interpolated
            let vals: Vec<Val> = match &mesh {
                Some(m) => eval_on(&value, Comp::Mesh(m), Dom::Point).iter().map(|v| v.convert(t)).collect(),
                None => vec![],
            };
            let k = kern(move |a, n| {
                let mut o = vec![vec![], vec![]];
                for i in 0..n {
                    let p = at(a[0], i).v();
                    let mut best: Option<(f32, usize)> = None;
                    if let Some(m) = mesh.as_ref() {
                        for (ti, (_, tr)) in tris.iter().enumerate() {
                            let q = closest_on_tri(p, m.pos[tr[0] as usize], m.pos[tr[1] as usize], m.pos[tr[2] as usize]);
                            let d = len3(sub3(p, q));
                            if best.map_or(true, |b| d < b.0) {
                                best = Some((d, ti));
                            }
                        }
                    }
                    match (best, mesh.as_ref()) {
                        (Some((_, ti)), Some(m)) => {
                            let tr = tris[ti].1;
                            let (va, vb, vc) = (m.pos[tr[0] as usize], m.pos[tr[1] as usize], m.pos[tr[2] as usize]);
                            let q = closest_on_tri(p, va, vb, vc);
                            let w = barycentric(q, va, vb, vc);
                            let v = match t {
                                Ty::V => {
                                    let (x, y, z) = (vals[tr[0] as usize].v(), vals[tr[1] as usize].v(), vals[tr[2] as usize].v());
                                    Val::V([x[0] * w[0] + y[0] * w[1] + z[0] * w[2], x[1] * w[0] + y[1] * w[1] + z[1] * w[2], x[2] * w[0] + y[2] * w[1] + z[2] * w[2]])
                                }
                                Ty::F => Val::F(vals[tr[0] as usize].f() * w[0] + vals[tr[1] as usize].f() * w[1] + vals[tr[2] as usize].f() * w[2]),
                                _ => vals[tr[0] as usize].clone(),
                            };
                            o[0].push(v);
                            o[1].push(Val::B(true));
                        }
                        _ => {
                            o[0].push(Val::zero(t));
                            o[1].push(Val::B(false));
                        }
                    }
                }
                o
            });
            let r = mf(k, vec![src], 2);
            outs(node, vec![("Value", fld(r[0].clone())), ("Is Valid", fld(r[1].clone()))])
        }
        "ShaderNodeTexVoronoi" => {
            use super::noise::*;
            let dim: usize = match node.ps("voronoi_dimensions") {
                "1D" => 1,
                "2D" => 2,
                "4D" => 4,
                _ => 3,
            };
            let feature: u8 = match node.ps("feature") {
                "F2" => 1,
                "SMOOTH_F1" => 2,
                "DISTANCE_TO_EDGE" => 3,
                "N_SPHERE_RADIUS" => 4,
                _ => 0,
            };
            let metric: u8 = match node.ps("distance") {
                "MANHATTAN" => 1,
                "CHEBYCHEV" => 2,
                "MINKOWSKI" => 3,
                _ => 0,
            };
            let normalize = node.pb("normalize");
            let get = |id: &str, d: Val| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(d)) } };
            let args = vec![
                get("Vector", Val::V([0.0; 3]))?,
                get("W", Val::F(0.0))?,
                get("Scale", Val::F(5.0))?,
                get("Detail", Val::F(0.0))?,
                get("Roughness", Val::F(0.5))?,
                get("Lacunarity", Val::F(2.0))?,
                get("Smoothness", Val::F(1.0))?,
                get("Exponent", Val::F(0.5))?,
                get("Randomness", Val::F(1.0))?,
            ];
            let k = kern(move |a, n| {
                let mut o = vec![vec![], vec![], vec![], vec![], vec![]];
                for i in 0..n {
                    let v = at(a[0], i).v();
                    let w = at(a[1], i).f();
                    let scale = at(a[2], i).f();
                    let randomness = at(a[8], i).f().clamp(0.0, 1.0);
                    let mut prm = VorP {
                        feature,
                        metric,
                        dim,
                        scale,
                        detail: at(a[3], i).f(),
                        roughness: at(a[4], i).f(),
                        lacunarity: at(a[5], i).f(),
                        smoothness: if feature == 2 { (at(a[6], i).f() / 2.0).clamp(0.0, 0.5) } else { 0.0 },
                        exponent: if metric == 3 && dim != 1 { at(a[7], i).f() } else { 0.0 },
                        randomness,
                        normalize,
                        max_distance: 0.0,
                    };
                    let m = 0.5 + 0.5 * randomness;
                    let coord_raw = match dim {
                        1 => [w, 0.0, 0.0, 0.0],
                        2 => [v[0], v[1], 0.0, 0.0],
                        3 => [v[0], v[1], v[2], 0.0],
                        _ => [v[0], v[1], v[2], w],
                    };
                    let mut coord = [0.0f32; 4];
                    for q in 0..dim {
                        coord[q] = coord_raw[q] * scale;
                    }
                    match feature {
                        3 => {
                            prm.max_distance = 0.5 + 0.5 * randomness;
                            o[0].push(Val::F(vor_fractal_dist_to_edge(&prm, coord)));
                            o[1].push(Val::C([0.0, 0.0, 0.0, 1.0]));
                            o[2].push(Val::V([0.0; 3]));
                            o[3].push(Val::F(0.0));
                            o[4].push(Val::F(0.0));
                        }
                        4 => {
                            o[0].push(Val::F(0.0));
                            o[1].push(Val::C([0.0, 0.0, 0.0, 1.0]));
                            o[2].push(Val::V([0.0; 3]));
                            o[3].push(Val::F(0.0));
                            o[4].push(Val::F(vor_n_sphere(&prm, coord)));
                        }
                        _ => {
                            let md = if dim == 1 {
                                m
                            } else {
                                let mut z = [0.0f32; 4];
                                let mut mm = [0.0f32; 4];
                                for q in 0..dim {
                                    mm[q] = m;
                                }
                                let _ = &mut z;
                                let dd = vdist_pub(dim, z, mm, &prm);
                                dd
                            };
                            prm.max_distance = md * if feature == 1 { 2.0 } else { 1.0 };
                            let r = vor_fractal(&prm, coord, true);
                            o[0].push(Val::F(r.distance));
                            o[1].push(Val::C([r.color[0], r.color[1], r.color[2], 1.0]));
                            o[2].push(Val::V(if dim == 1 { [0.0; 3] } else { [r.position[0], r.position[1], r.position[2]] }));
                            o[3].push(Val::F(r.position[3]));
                            o[4].push(Val::F(0.0));
                        }
                    }
                }
                o
            });
            let r = mf(k, args, 5);
            outs(node, vec![("Distance", fld(r[0].clone())), ("Color", fld(r[1].clone())), ("Position", fld(r[2].clone())), ("W", fld(r[3].clone())), ("Radius", fld(r[4].clone()))])
        }
        "ShaderNodeTexWhiteNoise" => {
            use super::noise::*;
            let dim = node.ps("noise_dimensions").to_string();
            let get = |id: &str, d: Val| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(d)) } };
            let args = vec![get("Vector", Val::V([0.0; 3]))?, get("W", Val::F(0.0))?];
            let k = kern(move |a, n| {
                let mut o = vec![vec![], vec![]];
                for i in 0..n {
                    let v = at(a[0], i).v();
                    let w = at(a[1], i).f();
                    let f = match dim.as_str() {
                        "1D" => hf1(w),
                        "2D" => hf2(v[0], v[1]),
                        "4D" => hf4(v[0], v[1], v[2], w),
                        _ => hf3(v[0], v[1], v[2]),
                    };
                    let c = match dim.as_str() {
                        "1D" => [hf1(w), hf2(w, 1.0), hf2(w, 2.0)],
                        "2D" => [hf2(v[0], v[1]), hf3(v[0], v[1], 1.0), hf3(v[0], v[1], 2.0)],
                        "4D" => [hf4(v[0], v[1], v[2], w), hf4(v[2], v[0], w, v[1]), hf4(w, v[2], v[1], v[0])],
                        _ => hash_f3_to_f3(v),
                    };
                    o[0].push(Val::F(f));
                    o[1].push(Val::C([c[0], c[1], c[2], 1.0]));
                }
                o
            });
            let r = mf(k, args, 2);
            outs(node, vec![("Value", fld(r[0].clone())), ("Color", fld(r[1].clone()))])
        }
        "ShaderNodeTexWave" => {
            use super::noise::*;
            let wave_type = node.ps("wave_type").to_string();
            let bands = node.ps("bands_direction").to_string();
            let rings = node.ps("rings_direction").to_string();
            let profile = node.ps("wave_profile").to_string();
            let get = |id: &str, d: Val| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(d)) } };
            let args = vec![get("Vector", Val::V([0.0; 3]))?, get("Scale", Val::F(5.0))?, get("Distortion", Val::F(0.0))?, get("Detail", Val::F(2.0))?, get("Detail Scale", Val::F(1.0))?, get("Detail Roughness", Val::F(0.5))?, get("Phase Offset", Val::F(0.0))?];
            let k = kern(move |a, n| {
                let mut fac = Vec::with_capacity(n);
                let mut col = Vec::with_capacity(n);
                for i in 0..n {
                    let v = at(a[0], i).v();
                    let sc = at(a[1], i).f();
                    let dist = at(a[2], i).f();
                    let detail = at(a[3], i).f();
                    let dscale = at(a[4], i).f();
                    let drough = at(a[5], i).f();
                    let phase = at(a[6], i).f();
                    let p = [(v[0] * sc + 0.000001) * 0.999999, (v[1] * sc + 0.000001) * 0.999999, (v[2] * sc + 0.000001) * 0.999999];
                    let mut n_ = if wave_type == "BANDS" {
                        match bands.as_str() {
                            "X" => p[0] * 20.0,
                            "Y" => p[1] * 20.0,
                            "Z" => p[2] * 20.0,
                            _ => (p[0] + p[1] + p[2]) * 10.0,
                        }
                    } else {
                        let mut rp = p;
                        match rings.as_str() {
                            "X" => rp[0] = 0.0,
                            "Y" => rp[1] = 0.0,
                            "Z" => rp[2] = 0.0,
                            _ => {}
                        }
                        (rp[0] * rp[0] + rp[1] * rp[1] + rp[2] * rp[2]).sqrt() * 20.0
                    };
                    n_ += phase;
                    if dist != 0.0 {
                        let prm = NoiseParams { dim: 3, kind: "FBM".into(), normalize: true, detail, roughness: drough, lacunarity: 2.0, offset: 0.0, gain: 1.0 };
                        let nv = noise_value(&prm, [p[0] * dscale, p[1] * dscale, p[2] * dscale, 0.0]);
                        n_ += dist * (nv * 2.0 - 1.0);
                    }
                    let f = match profile.as_str() {
                        "SAW" => {
                            let t = n_ / (2.0 * std::f32::consts::PI);
                            t - t.floor()
                        }
                        "TRI" => {
                            let t = n_ / (2.0 * std::f32::consts::PI);
                            (t - (t + 0.5).floor()).abs() * 2.0
                        }
                        _ => 0.5 + 0.5 * (n_ - std::f32::consts::FRAC_PI_2).sin(),
                    };
                    fac.push(Val::F(f));
                    col.push(Val::C([f, f, f, 1.0]));
                }
                vec![col, fac]
            });
            let r = mf(k, args, 2);
            outs(node, vec![("Color", fld(r[0].clone())), ("Fac", fld(r[1].clone()))])
        }
        "ShaderNodeTexNoise" => {
            use super::noise::*;
            let dim = match node.ps("noise_dimensions") {
                "1D" => 1u8,
                "2D" => 2,
                "4D" => 4,
                _ => 3,
            };
            let kind = node.ps("noise_type").to_string();
            let normalize = node.pb("normalize");
            let get = |id: &str, d: Val| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(d)) } };
            let args = vec![
                get("Vector", Val::V([0.0; 3]))?,
                get("W", Val::F(0.0))?,
                get("Scale", Val::F(1.0))?,
                get("Detail", Val::F(2.0))?,
                get("Roughness", Val::F(0.5))?,
                get("Lacunarity", Val::F(2.0))?,
                get("Offset", Val::F(0.0))?,
                get("Gain", Val::F(1.0))?,
                get("Distortion", Val::F(0.0))?,
            ];
            let k = kern(move |a, n| {
                let mut fac = Vec::with_capacity(n);
                let mut col = Vec::with_capacity(n);
                for i in 0..n {
                    let v = at(a[0], i).v();
                    let w = at(a[1], i).f();
                    let sc = at(a[2], i).f();
                    let prm = NoiseParams { dim, kind: kind.clone(), normalize, detail: at(a[3], i).f(), roughness: at(a[4], i).f(), lacunarity: at(a[5], i).f(), offset: at(a[6], i).f(), gain: at(a[7], i).f() };
                    let dist = at(a[8], i).f();
                    let mut p = [v[0] * sc, v[1] * sc, v[2] * sc, w * sc];
                    if dist != 0.0 {
                        p = distort(dim, p, dist);
                    }
                    let f = noise_value(&prm, p);
                    fac.push(Val::F(f));
                    col.push(Val::C([f, f, f, 1.0]));
                }
                vec![fac, col]
            });
            let r = mf(k, args, 2);
            outs(node, vec![("Fac", fld(r[0].clone())), ("Color", fld(r[1].clone()))])
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

pub fn barycentric(p: V3, a: V3, b: V3, c: V3) -> [f32; 3] {
    let v0 = sub3(b, a);
    let v1 = sub3(c, a);
    let v2 = sub3(p, a);
    let d00 = dot3(v0, v0);
    let d01 = dot3(v0, v1);
    let d11 = dot3(v1, v1);
    let d20 = dot3(v2, v0);
    let d21 = dot3(v2, v1);
    let denom = d00 * d11 - d01 * d01;
    if denom == 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    [1.0 - v - w, v, w]
}

pub fn face_area(m: &Mesh, f: usize) -> f32 {
    let r = m.face(f);
    let n = r.len();
    if n == 3 {
        let (a, b, c) = (m.pos[r[0] as usize], m.pos[r[1] as usize], m.pos[r[2] as usize]);
        return len3(cross3(sub3(b, a), sub3(c, a))) * 0.5;
    }
    // BM_face_calc_area: Newell cross on local coords, sqrt(sum)*0.5
    let mut nrm = [0.0f32; 3];
    let mut prev = m.pos[r[n - 1] as usize];
    for i in 0..n {
        let cur = m.pos[r[i] as usize];
        nrm[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
        nrm[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
        nrm[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
        prev = cur;
    }
    dot3(nrm, nrm).sqrt() * 0.5
}

fn edge_angles(m: &Mesh, signed: bool) -> Vec<f32> {
    let fnor = face_normals(m);
    let mut faces_of: Vec<Vec<(usize, usize)>> = vec![vec![]; m.ne()];
    for f in 0..m.nf() {
        for c in m.face_range(f) {
            faces_of[m.ce[c] as usize].push((f, c));
        }
    }
    (0..m.ne())
        .map(|e| {
            if faces_of[e].len() != 2 {
                return 0.0;
            }
            let (f0, c0) = faces_of[e][0];
            let (f1, _) = faces_of[e][1];
            let (n0, n1) = (fnor[f0], fnor[f1]);
            let angle = {
                let d = dot3(n0, n1).clamp(-1.0, 1.0);
                // angle_normalized_v3v3
                if d >= 0.0 {
                    2.0 * (len3(sub3(n0, n1)) / 2.0).clamp(-1.0, 1.0).asin()
                } else {
                    std::f32::consts::PI - 2.0 * (len3(add3(n0, n1)) / 2.0).clamp(-1.0, 1.0).asin()
                }
            };
            if !signed {
                return angle;
            }
            // sign: edge direction of f0's loop crossed with normals
            let r = m.face_range(f0);
            let k = c0 - r.start;
            let v0 = m.pos[m.cv[c0] as usize];
            let v1 = m.pos[m.cv[r.start + (k + 1) % r.len()] as usize];
            let edge_dir = sub3(v1, v0);
            let cross = cross3(n0, n1);
            if dot3(cross, edge_dir) < 0.0 {
                -angle
            } else {
                angle
            }
        })
        .collect()
}

fn mesh_neighbors(m: &Mesh, which: &str, w: u8, dom: Dom) -> Vec<Val> {
    // w: 0 vertex count, 1 face count
    let src: Vec<i32>;
    let sdom;
    if which.ends_with("VertexNeighbors") {
        sdom = Dom::Point;
        let mut vc = vec![0i32; m.nv()];
        let mut fc = vec![0i32; m.nv()];
        for e in &m.edges {
            vc[e[0] as usize] += 1;
            vc[e[1] as usize] += 1;
        }
        for &v in &m.cv {
            fc[v as usize] += 1;
        }
        src = if w == 0 { vc } else { fc };
    } else if which.ends_with("FaceNeighbors") {
        sdom = Dom::Face;
        // vertex count = number of corners; face count = adjacent faces through edges
        let ecount = m.edge_face_count();
        src = (0..m.nf())
            .map(|f| if w == 0 { m.face_range(f).len() as i32 } else { m.face_range(f).map(|c| ecount[m.ce[c] as usize] as i32 - 1).sum() })
            .collect();
    } else {
        sdom = Dom::Edge;
        src = m.edge_face_count().iter().map(|&x| x as i32).collect();
    }
    let v: Vec<Val> = src.iter().map(|&x| Val::I(x)).collect();
    adapt_mesh(m, sdom, dom, Ty::I, &v)
}
