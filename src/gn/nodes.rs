//! Node dispatch + function-type (field / multi-function) nodes: Math, Vector Math, Compare, Map Range, Switch, ...
use super::rot::*;
use super::eval::*;
use super::field::*;
use super::load::*;
use super::math::*;
use super::val::*;
use super::geom::Geometry;
use std::rc::Rc;

pub type Outs = Vec<Value>;

/// build the output vector of a node from (socket id, value) pairs; missing outputs get defaults
pub fn outs(node: &Node, vals: Vec<(&str, Value)>) -> Outs {
    node.outs
        .iter()
        .map(|o| match vals.iter().find(|(id, _)| *id == o.id) {
            Some((_, v)) => v.clone(),
            None => default_value(o.ty, &o.tname),
        })
        .collect()
}

pub fn fld(f: Field) -> Value {
    Value::F(f)
}

/// nodes that act on each geometry component: Blender applies them recursively to the references of instances
fn inst_input(ty: &str) -> Option<&'static str> {
    match ty {
        "GeometryNodeFillCurve" | "GeometryNodeCurveToMesh" | "GeometryNodeTrimCurve" | "GeometryNodeReverseCurve" | "GeometryNodeSetCurveHandlePositions" | "GeometryNodeSetCurveRadius" | "GeometryNodeSetCurveTilt" | "GeometryNodeCurveSplineType" | "GeometryNodeCurveToPoints" | "GeometryNodeCurveSetHandles" | "GeometryNodeSetCurveNormal" => Some("Curve"),
        "GeometryNodeExtrudeMesh" | "GeometryNodeSubdivideMesh" | "GeometryNodeFlipFaces" | "GeometryNodeTriangulate" | "GeometryNodeSplitEdges" | "GeometryNodeSubdivisionSurface" | "GeometryNodeDualMesh" | "GeometryNodeMeshToCurve" => Some("Mesh"),
        "GeometryNodeSetShadeSmooth" | "GeometryNodeMergeByDistance" | "GeometryNodeResampleCurve" | "GeometryNodeSubdivideCurve" | "GeometryNodeFilletCurve" | "GeometryNodeScaleElements" | "GeometryNodeStoreNamedAttribute" | "GeometryNodeSetMaterial" | "GeometryNodeSetMaterialIndex" => Some("Geometry"),
        _ => None,
    }
}

pub fn dispatch(ev: &Ev, ni: usize, node: &Node) -> Res<Outs> {
    if let Some(id) = inst_input(&node.ty) {
        let id = if node.ty == "GeometryNodeSubdivideCurve" || node.ty == "GeometryNodeFilletCurve" || node.ty == "GeometryNodeResampleCurve" { "Curve" } else { id };
        if node.in_idx(id).is_some() {
            let g = ev.geo(ni, id)?;
            if let Some(inst) = &g.inst {
                let gi = node.outs.iter().position(|o| o.tname.contains("GEOMETRY") || o.tname.contains("Geometry")).unwrap_or(0);
                let mut base = (*g).clone();
                base.inst = None;
                let old = ev.set_override(Some((ni, id.to_string(), Rc::new(base))));
                let mut res = dispatch_inner(ev, ni, node);
                let mut new_refs = vec![];
                if res.is_ok() {
                    for r in &inst.refs {
                        ev.set_override(Some((ni, id.to_string(), r.clone())));
                        match dispatch(ev, ni, node) {
                            Ok(o) => new_refs.push(match &o[gi] {
                                Value::G(x) => x.clone(),
                                _ => Rc::new(Geometry::default()),
                            }),
                            Err(e) => {
                                res = Err(e);
                                break;
                            }
                        }
                    }
                }
                ev.set_override(old);
                let mut o = res?;
                if let Value::G(x) = &o[gi] {
                    let mut ng = (**x).clone();
                    let mut ni_ = inst.clone();
                    ni_.refs = new_refs;
                    ng.inst = Some(ni_);
                    o[gi] = Value::G(Rc::new(ng));
                }
                return Ok(o);
            }
        }
    }
    dispatch_inner(ev, ni, node)
}

fn dispatch_inner(ev: &Ev, ni: usize, node: &Node) -> Res<Outs> {
    if let Some(r) = fn_nodes(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_geo::geo_nodes(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_geo2::geo_nodes2(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_curves::curve_nodes(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_fields::field_nodes(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_mesh::mesh_nodes(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_mesh::mesh_nodes2(ev, ni, node)? {
        return Ok(r);
    }
    if let Some(r) = super::nodes_text::text_nodes(ev, ni, node)? {
        return Ok(r);
    }
    Err(format!("unsupported node type {}", node.ty))
}

fn kern(f: impl Fn(&[&[Val]], usize) -> Vec<Vec<Val>> + 'static) -> Kernel {
    Rc::new(f)
}

fn bool_math(op: &str, a: bool, b: bool) -> bool {
    match op {
        "AND" => a && b,
        "OR" => a || b,
        "NOT" => !a,
        "NAND" => !(a && b),
        "NOR" => !(a || b),
        "XNOR" => a == b,
        "XOR" => a != b,
        "IMPLY" => !a || b,
        "NIMPLY" => a && !b,
        _ => false,
    }
}

fn map_range_f(mode: &str, clamp: bool, v: f32, fmin: f32, fmax: f32, tmin: f32, tmax: f32, steps: f32) -> f32 {
    let mut result;
    match mode {
        "STEPPED" => {
            let mut factor = safe_div(v - fmin, fmax - fmin);
            factor = safe_div((factor * (steps + 1.0)).floor(), steps);
            result = tmin + factor * (tmax - tmin);
        }
        "SMOOTHSTEP" => {
            let mut factor = if fmin > fmax { 1.0 - safe_div(v - fmax, fmin - fmax).clamp(0.0, 1.0) } else { safe_div(v - fmin, fmax - fmin).clamp(0.0, 1.0) };
            factor = factor * factor * (3.0 - 2.0 * factor);
            result = tmin + factor * (tmax - tmin);
        }
        "SMOOTHERSTEP" => {
            let mut factor = if fmin > fmax { 1.0 - safe_div(v - fmax, fmin - fmax).clamp(0.0, 1.0) } else { safe_div(v - fmin, fmax - fmin).clamp(0.0, 1.0) };
            factor = factor * factor * factor * (factor * (factor * 6.0 - 15.0) + 10.0);
            result = tmin + factor * (tmax - tmin);
        }
        _ => {
            let factor = safe_div(v - fmin, fmax - fmin);
            result = tmin + factor * (tmax - tmin);
        }
    }
    if clamp && (mode == "LINEAR" || mode == "STEPPED") {
        result = if tmin > tmax { result.clamp(tmax, tmin) } else { result.clamp(tmin, tmax) };
    }
    result
}

fn color_ramp_eval(ramp: &serde_json::Value, fac: f32) -> [f32; 4] {
    let els: Vec<(f32, [f32; 4])> = ramp["els"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    let c: Vec<f32> = e[1].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
                    (e[0].as_f64().unwrap() as f32, [c[0], c[1], c[2], c[3]])
                })
                .collect()
        })
        .unwrap_or_default();
    if els.is_empty() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let interp = ramp["interp"].as_str().unwrap_or("LINEAR");
    let n = els.len();
    if fac < els[0].0 || n == 1 {
        return els[0].1;
    }
    if fac >= els[n - 1].0 {
        return els[n - 1].1;
    }
    let mut i = 0;
    while i + 1 < n && els[i + 1].0 <= fac {
        i += 1;
    }
    if interp == "CONSTANT" {
        return els[i].1;
    }
    let (a, b) = (els[i], els[i + 1]);
    let mut t = if b.0 != a.0 { (fac - a.0) / (b.0 - a.0) } else { 0.0 };
    if interp == "EASE" {
        t = t * t * (3.0 - 2.0 * t);
    }
    let mut o = [0.0; 4];
    for k in 0..4 {
        o[k] = a.1[k] * (1.0 - t) + b.1[k] * t;
    }
    o
}

fn fn_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "ShaderNodeValue" => outs(node, vec![("Value", fld(kf(node.pf("value") as f32)))]),
        "FunctionNodeInputInt" => outs(node, vec![("Integer", fld(konst(Val::I(node.pi("integer") as i32))))]),
        "FunctionNodeInputBool" => outs(node, vec![("Boolean", fld(konst(Val::B(node.pb("boolean")))))]),
        "GeometryNodeStringJoin" => {
            let delim = ev.cval(ni, "Delimiter")?.s().to_string();
            let mut parts: Vec<String> = vec![];
            for v in ev.input_multi(ni, "Strings")? {
                if let Value::F(f) = v {
                    if let Some(c) = as_const(&f) {
                        parts.push(c.s().to_string());
                    }
                }
            }
            parts.reverse();
            outs(node, vec![("String", fld(konst(Val::S(parts.join(&delim).into()))))])
        }
        "FunctionNodeInputString" => outs(node, vec![("String", fld(konst(Val::S(node.ps("string").into()))))]),
        "FunctionNodeInputVector" => {
            let v: Vec<f32> = node.pj("vector").and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or(vec![0.0; 3]);
            outs(node, vec![("Vector", fld(konst(Val::V([v[0], v[1], v[2]]))))])
        }
        "FunctionNodeInputColor" => {
            let v: Vec<f32> = node.pj("value").and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or(vec![0.0, 0.0, 0.0, 1.0]);
            outs(node, vec![("Color", fld(konst(Val::C([v[0], v[1], v[2], v[3]]))))])
        }
        "ShaderNodeMath" => {
            let op = node.ps("operation").to_string();
            let clamp = node.pb("use_clamp");
            let a = ev.field(ni, "Value")?;
            let b = if ev.has_input(ni, "Value_001") { ev.field(ni, "Value_001")? } else { kf(0.0) };
            let c = if ev.has_input(ni, "Value_002") { ev.field(ni, "Value_002")? } else { kf(0.0) };
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let mut r = math_op(&op, at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).f());
                        if clamp {
                            r = r.clamp(0.0, 1.0);
                        }
                        Val::F(r)
                    })
                    .collect()]
            });
            outs(node, vec![("Value", fld(mf1(k, vec![a, b, c])))])
        }
        "ShaderNodeVectorMath" => {
            let op = node.ps("operation").to_string();
            let a = ev.field(ni, "Vector")?;
            let b = if ev.has_input(ni, "Vector_001") { ev.field(ni, "Vector_001")? } else { konst(Val::V([0.0; 3])) };
            let c = if ev.has_input(ni, "Vector_002") { ev.field(ni, "Vector_002")? } else { konst(Val::V([0.0; 3])) };
            let s = if ev.has_input(ni, "Scale") { ev.field(ni, "Scale")? } else { kf(1.0) };
            let k = kern(move |x, n| {
                let mut vs = Vec::with_capacity(n);
                let mut fs = Vec::with_capacity(n);
                for i in 0..n {
                    let (v, f) = vec_op(&op, at(x[0], i).v(), at(x[1], i).v(), at(x[2], i).v(), at(x[3], i).f());
                    vs.push(Val::V(v));
                    fs.push(Val::F(f));
                }
                vec![vs, fs]
            });
            let r = mf(k, vec![a, b, c, s], 2);
            outs(node, vec![("Vector", fld(r[0].clone())), ("Value", fld(r[1].clone()))])
        }
        "ShaderNodeCombineXYZ" => {
            let (x, y, z) = (ev.field(ni, "X")?, ev.field(ni, "Y")?, ev.field(ni, "Z")?);
            let k = kern(|a, n| vec![(0..n).map(|i| Val::V([at(a[0], i).f(), at(a[1], i).f(), at(a[2], i).f()])).collect()]);
            outs(node, vec![("Vector", fld(mf1(k, vec![x, y, z])))])
        }
        "FunctionNodeCombineMatrix" => {
            let mut args = vec![];
            for c in 1..=4 {
                for r in 1..=4 {
                    args.push(ev.field(ni, &format!("Column {} Row {}", c, r))?);
                }
            }
            let k = kern(|a, n| {
                vec![(0..n)
                    .map(|i| {
                        let mut m = [[0.0f32; 4]; 4];
                        for c in 0..4 {
                            for r in 0..4 {
                                m[c][r] = at(a[c * 4 + r], i).f();
                            }
                        }
                        Val::M(Box::new(m))
                    })
                    .collect()]
            });
            outs(node, vec![("Matrix", fld(mf1(k, args)))])
        }
        "FunctionNodeSeparateMatrix" => {
            let m = ev.field(ni, "Matrix")?;
            let k = kern(|a, n| (0..16).map(|q| (0..n).map(|i| Val::F(at(a[0], i).m()[q / 4][q % 4])).collect()).collect());
            let r = mf(k, vec![m], 16);
            let names: Vec<String> = (1..=4).flat_map(|c| (1..=4).map(move |rr| format!("Column {} Row {}", c, rr))).collect();
            outs(node, names.iter().enumerate().map(|(q, nm)| (nm.as_str(), fld(r[q].clone()))).collect())
        }
        "FunctionNodeTransposeMatrix" => {
            let m = ev.field(ni, "Matrix")?;
            let k = kern(|a, n| vec![(0..n).map(|i| { let m = at(a[0], i).m(); let mut t = m; for c in 0..4 { for r in 0..4 { t[c][r] = m[r][c]; } } Val::M(Box::new(t)) }).collect()]);
            outs(node, vec![("Matrix", fld(mf1(k, vec![m])))])
        }
        "FunctionNodeMatrixDeterminant" => {
            let m = ev.field(ni, "Matrix")?;
            let k = kern(|a, n| vec![(0..n).map(|i| Val::F(m4_det(&at(a[0], i).m()))).collect()]);
            outs(node, vec![("Determinant", fld(mf1(k, vec![m])))])
        }
        "FunctionNodeInvertMatrix" => {
            let m = ev.field(ni, "Matrix")?;
            let k = kern(|a, n| {
                let mut o = vec![vec![], vec![]];
                for i in 0..n {
                    let (inv, ok) = m4_invert(&at(a[0], i).m());
                    o[0].push(Val::M(Box::new(inv)));
                    o[1].push(Val::B(ok));
                }
                o
            });
            let r = mf(k, vec![m], 2);
            outs(node, vec![("Matrix", fld(r[0].clone())), ("Invertible", fld(r[1].clone()))])
        }
        "FunctionNodeMatrixMultiply" => {
            let (a, b) = (ev.field(ni, "Matrix")?, ev.field(ni, "Matrix_001")?);
            let k = kern(|x, n| vec![(0..n).map(|i| Val::M(Box::new(super::geom::m4_mul(&at(x[0], i).m(), &at(x[1], i).m())))).collect()]);
            outs(node, vec![("Matrix", fld(mf1(k, vec![a, b])))])
        }
        "FunctionNodeTransformPoint" => {
            let (v, m) = (ev.field(ni, "Vector")?, ev.field(ni, "Transform")?);
            let k = kern(|x, n| vec![(0..n).map(|i| Val::V(super::geom::m4_point(&at(x[1], i).m(), at(x[0], i).v()))).collect()]);
            outs(node, vec![("Vector", fld(mf1(k, vec![v, m])))])
        }
        "FunctionNodeTransformDirection" => {
            let (v, m) = (ev.field(ni, "Direction")?, ev.field(ni, "Transform")?);
            let k = kern(|x, n| vec![(0..n).map(|i| Val::V(super::geom::m4_dir(&at(x[1], i).m(), at(x[0], i).v()))).collect()]);
            outs(node, vec![("Direction", fld(mf1(k, vec![v, m])))])
        }
        "ShaderNodeSeparateXYZ" => {
            let v = ev.field(ni, "Vector")?;
            let k = kern(|a, n| (0..3).map(|c| (0..n).map(|i| Val::F(at(a[0], i).v()[c])).collect()).collect());
            let r = mf(k, vec![v], 3);
            outs(node, vec![("X", fld(r[0].clone())), ("Y", fld(r[1].clone())), ("Z", fld(r[2].clone()))])
        }
        "FunctionNodeBooleanMath" => {
            let op = node.ps("operation").to_string();
            let a = ev.field(ni, "Boolean")?;
            let b = if ev.has_input(ni, "Boolean_001") { ev.field(ni, "Boolean_001")? } else { konst(Val::B(false)) };
            let k = kern(move |x, n| vec![(0..n).map(|i| Val::B(bool_math(&op, at(x[0], i).b(), at(x[1], i).b()))).collect()]);
            outs(node, vec![("Boolean", fld(mf1(k, vec![a, b])))])
        }
        "ShaderNodeClamp" => {
            let range = node.ps("clamp_type") == "RANGE";
            let (v, lo, hi) = (ev.field(ni, "Value")?, ev.field(ni, "Min")?, ev.field(ni, "Max")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let (v, lo, hi) = (at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).f());
                        Val::F(if range {
                            if lo < hi {
                                v.clamp(lo, hi)
                            } else {
                                v.clamp(hi, lo)
                            }
                        } else {
                            v.max(lo).min(hi)
                        })
                    })
                    .collect()]
            });
            outs(node, vec![("Result", fld(mf1(k, vec![v, lo, hi])))])
        }
        "ShaderNodeMapRange" => {
            let mode = node.ps("interpolation_type").to_string();
            let clamp = node.pb("clamp");
            if node.ps("data_type") == "FLOAT_VECTOR" {
                let ids = ["Vector", "From_Min_FLOAT3", "From_Max_FLOAT3", "To_Min_FLOAT3", "To_Max_FLOAT3", "Steps_FLOAT3"];
                let args: Vec<Field> = ids.iter().map(|i| if ev.has_input(ni, i) { ev.field(ni, i) } else { Ok(konst(Val::V([0.0; 3]))) }).collect::<Res<_>>()?;
                let k = kern(move |x, n| {
                    vec![(0..n)
                        .map(|i| {
                            let a: Vec<V3> = (0..6).map(|j| at(x[j], i).v()).collect();
                            let mut o = [0.0f32; 3];
                            for c in 0..3 {
                                o[c] = map_range_f(&mode, clamp, a[0][c], a[1][c], a[2][c], a[3][c], a[4][c], a[5][c]);
                            }
                            Val::V(o)
                        })
                        .collect()]
                });
                outs(node, vec![("Vector", fld(mf1(k, args)))])
            } else {
                let ids = ["Value", "From Min", "From Max", "To Min", "To Max", "Steps"];
                let args: Vec<Field> = ids.iter().map(|i| if ev.has_input(ni, i) { ev.field(ni, i) } else { Ok(kf(0.0)) }).collect::<Res<_>>()?;
                let k = kern(move |x, n| vec![(0..n).map(|i| Val::F(map_range_f(&mode, clamp, at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).f(), at(x[3], i).f(), at(x[4], i).f(), at(x[5], i).f()))).collect()]);
                outs(node, vec![("Result", fld(mf1(k, args)))])
            }
        }
        "FunctionNodeCompare" => {
            let op = node.ps("operation").to_string();
            let dt = node.ps("data_type").to_string();
            let mode = node.ps("mode").to_string();
            let get = |id: &str, d: Val| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(d)) } };
            let (a, b, c, ang, eps) = match dt.as_str() {
                "INT" => (get("A_INT", Val::I(0))?, get("B_INT", Val::I(0))?, kf(0.0), kf(0.0), kf(0.0)),
                "VECTOR" => (get("A_VEC3", Val::V([0.0; 3]))?, get("B_VEC3", Val::V([0.0; 3]))?, get("C", Val::F(0.0))?, get("Angle", Val::F(0.0))?, get("Epsilon", Val::F(0.0))?),
                "RGBA" => (get("A_COL", Val::C([0.0; 4]))?, get("B_COL", Val::C([0.0; 4]))?, kf(0.0), kf(0.0), get("Epsilon", Val::F(0.0))?),
                "STRING" => (get("A_STR", Val::S("".into()))?, get("B_STR", Val::S("".into()))?, kf(0.0), kf(0.0), kf(0.0)),
                _ => (get("A", Val::F(0.0))?, get("B", Val::F(0.0))?, get("C", Val::F(0.0))?, kf(0.0), kf(0.0)),
            };
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let r = match dt.as_str() {
                            "INT" => {
                                let (a, b) = (at(x[0], i).i(), at(x[1], i).i());
                                match op.as_str() {
                                    "LESS_THAN" => a < b,
                                    "LESS_EQUAL" => a <= b,
                                    "GREATER_THAN" => a > b,
                                    "GREATER_EQUAL" => a >= b,
                                    "EQUAL" => a == b,
                                    "NOT_EQUAL" => a != b,
                                    _ => false,
                                }
                            }
                            "STRING" => {
                                let (a, b) = (at(x[0], i).s(), at(x[1], i).s());
                                match op.as_str() {
                                    "EQUAL" => a == b,
                                    "NOT_EQUAL" => a != b,
                                    _ => false,
                                }
                            }
                            "RGBA" => {
                                let (a, b) = (at(x[0], i).c(), at(x[1], i).c());
                                let e = at(x[4], i).f();
                                match op.as_str() {
                                    "EQUAL" => (a[0] - b[0]).abs() <= e && (a[1] - b[1]).abs() <= e && (a[2] - b[2]).abs() <= e,
                                    "NOT_EQUAL" => (a[0] - b[0]).abs() > e || (a[1] - b[1]).abs() > e || (a[2] - b[2]).abs() > e,
                                    "BRIGHTER" => rgb_to_grayscale(&a) > rgb_to_grayscale(&b),
                                    "DARKER" => rgb_to_grayscale(&a) < rgb_to_grayscale(&b),
                                    _ => false,
                                }
                            }
                            "VECTOR" => {
                                let (a, b) = (at(x[0], i).v(), at(x[1], i).v());
                                let (cc, an, e) = (at(x[2], i).f(), at(x[3], i).f(), at(x[4], i).f());
                                match mode.as_str() {
                                    "ELEMENT" => {
                                        let cmp = |p: f32, q: f32| match op.as_str() {
                                            "LESS_THAN" => p < q,
                                            "LESS_EQUAL" => p <= q,
                                            "GREATER_THAN" => p > q,
                                            "GREATER_EQUAL" => p >= q,
                                            "EQUAL" => (p - q).abs() <= e,
                                            "NOT_EQUAL" => (p - q).abs() > e,
                                            _ => false,
                                        };
                                        match op.as_str() {
                                            "NOT_EQUAL" => cmp(a[0], b[0]) || cmp(a[1], b[1]) || cmp(a[2], b[2]),
                                            _ => cmp(a[0], b[0]) && cmp(a[1], b[1]) && cmp(a[2], b[2]),
                                        }
                                    }
                                    "LENGTH" => cmp_f(&op, len3(a), len3(b), e),
                                    "AVERAGE" => cmp_f(&op, (a[0] + a[1] + a[2]) / 3.0, (b[0] + b[1] + b[2]) / 3.0, e),
                                    "DOT_PRODUCT" => cmp_f(&op, dot3(a, b), cc, e),
                                    _ => {
                                        let (an_, bn_) = (norm3(a), norm3(b));
                                        let angle = dot3(an_, bn_).clamp(-1.0, 1.0).acos();
                                        match op.as_str() {
                                            "EQUAL" => angle <= an,
                                            "NOT_EQUAL" => angle > an,
                                            _ => cmp_f(&op, angle, an, e),
                                        }
                                    }
                                }
                            }
                            _ => {
                                let (a, b, cc) = (at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).f());
                                cmp_f(&op, a, b, cc)
                            }
                        };
                        Val::B(r)
                    })
                    .collect()]
            });
            outs(node, vec![("Result", fld(mf1(k, vec![a, b, c, ang, eps])))])
        }
        "GeometryNodeSwitch" => {
            let t = node.ins.iter().find(|s| s.id == "True").map(|s| s.ty).unwrap_or(Ty::Geo);
            let sw = ev.field(ni, "Switch")?;
            if t == Ty::Geo {
                let b = ev.cb(ni, "Switch")?;
                let g = ev.input(ni, if b { "True" } else { "False" })?;
                outs(node, vec![("Output", g)])
            } else if let Some(c) = as_const(&sw) {
                let g = ev.input(ni, if c.b() { "True" } else { "False" })?;
                outs(node, vec![("Output", g)])
            } else {
                let (f, tr) = (ev.field(ni, "False")?, ev.field(ni, "True")?);
                let k = kern(|x, n| vec![(0..n).map(|i| if at(x[0], i).b() { at(x[2], i).clone() } else { at(x[1], i).clone() }).collect()]);
                outs(node, vec![("Output", fld(mf1(k, vec![sw, f, tr])))])
            }
        }
        "FunctionNodeRandomValue" => {
            let dt = node.ps("data_type").to_string();
            let id = ev.field(ni, "ID")?;
            let seed = ev.field(ni, "Seed")?;
            let get = |i: &str, d: Val| -> Res<Field> { if ev.has_input(ni, i) { ev.field(ni, i) } else { Ok(konst(d)) } };
            match dt.as_str() {
                "FLOAT" => {
                    let (lo, hi) = (get("Min_001", Val::F(0.0))?, get("Max_001", Val::F(1.0))?);
                    let k = kern(|x, n| {
                        vec![(0..n)
                            .map(|i| {
                                let (lo, hi, id, seed) = (at(x[0], i).f(), at(x[1], i).f(), at(x[2], i).i(), at(x[3], i).i());
                                Val::F(hash_to_float2(seed as u32, id as u32) * (hi - lo) + lo)
                            })
                            .collect()]
                    });
                    outs(node, vec![("Value_001", fld(mf1(k, vec![lo, hi, id, seed])))])
                }
                "INT" => {
                    let (lo, hi) = (get("Min_002", Val::I(0))?, get("Max_002", Val::I(100))?);
                    let k = kern(|x, n| {
                        vec![(0..n)
                            .map(|i| {
                                let (lo, hi, id, seed) = (at(x[0], i).i(), at(x[1], i).i(), at(x[2], i).i(), at(x[3], i).i());
                                let v = hash_to_float2(id as u32, seed as u32);
                                Val::I(((v * (hi - lo + 1) as f32 + lo as f32 - 0.5) + 0.5).floor() as i32)
                            })
                            .collect()]
                    });
                    outs(node, vec![("Value_002", fld(mf1(k, vec![lo, hi, id, seed])))])
                }
                "FLOAT_VECTOR" => {
                    let (lo, hi) = (get("Min", Val::V([0.0; 3]))?, get("Max", Val::V([1.0; 3]))?);
                    let k = kern(|x, n| {
                        vec![(0..n)
                            .map(|i| {
                                let (lo, hi, id, seed) = (at(x[0], i).v(), at(x[1], i).v(), at(x[2], i).i() as u32, at(x[3], i).i() as u32);
                                let r = [hash_to_float3(seed, id, 0), hash_to_float3(seed, id, 1), hash_to_float3(seed, id, 2)];
                                Val::V([r[0] * (hi[0] - lo[0]) + lo[0], r[1] * (hi[1] - lo[1]) + lo[1], r[2] * (hi[2] - lo[2]) + lo[2]])
                            })
                            .collect()]
                    });
                    outs(node, vec![("Value", fld(mf1(k, vec![lo, hi, id, seed])))])
                }
                "BOOLEAN" => {
                    let p = get("Probability", Val::F(0.5))?;
                    let k = kern(|x, n| {
                        vec![(0..n)
                            .map(|i| {
                                let (p, id, seed) = (at(x[0], i).f(), at(x[1], i).i(), at(x[2], i).i());
                                Val::B(hash_to_float2(id as u32, seed as u32) <= p)
                            })
                            .collect()]
                    });
                    outs(node, vec![("Value_003", fld(mf1(k, vec![p, id, seed])))])
                }
                _ => return Err(format!("RandomValue data_type {} unsupported", dt)),
            }
        }
        "ShaderNodeFloatCurve" => {
            let c = node.pj("curve").ok_or("curve missing")?;
            let pts: Vec<(f64, f64)> = c["pts"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
            let vecs: Vec<bool> = c["pts"].as_array().unwrap().iter().map(|p| p[2].as_str() == Some("VECTOR")).collect();
            let clip = c["clip"].as_array().cloned().unwrap_or_default();
            let (cmin, cmax) = (clip.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0), clip.get(4).and_then(|x| x.as_f64()).unwrap_or(1.0));
            let (xmin, xmax) = (clip.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0), clip.get(3).and_then(|x| x.as_f64()).unwrap_or(1.0));
            let extrap = c["ext"].as_str().map_or(true, |e| e != "HORIZONTAL");
            let use_clip = clip.get(0).and_then(|x| x.as_bool()).unwrap_or(true);
            let fc = crate::assets::curvekit::FloatCurve::new_full(&pts, &vecs, xmin as f32, xmax as f32, extrap);
            let (fac, v) = (ev.field(ni, "Factor")?, ev.field(ni, "Value")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let (f, v) = (at(x[0], i).f(), at(x[1], i).f());
                        let mut cv = fc.eval(v as f64) as f32;
                        if use_clip {
                            cv = cv.clamp(cmin as f32, cmax as f32);
                        }
                        Val::F(v * (1.0 - f) + cv * f)
                    })
                    .collect()]
            });
            outs(node, vec![("Value", fld(mf1(k, vec![fac, v])))])
        }
        "ShaderNodeVectorCurve" => {
            let c = node.pj("curves3").ok_or("curves3 missing")?;
            let clip = c["clip"].as_array().cloned().unwrap_or_default();
            let g = |i: usize, d: f64| clip.get(i).and_then(|x| x.as_f64()).unwrap_or(d);
            let (xmin, xmax) = (g(1, -1.0) as f32, g(3, 1.0) as f32);
            let extrap = c["ext"].as_str().map_or(true, |e| e != "HORIZONTAL");
            let mut fcs = vec![];
            for k in 0..3 {
                let arr = c["pts"][k].as_array().cloned().unwrap_or_default();
                let pts: Vec<(f64, f64)> = arr.iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
                let vecs: Vec<bool> = arr.iter().map(|p| p[2].as_str() == Some("VECTOR")).collect();
                fcs.push(crate::assets::curvekit::FloatCurve::new_full(&pts, &vecs, xmin, xmax, extrap));
            }
            let (fac, v) = (ev.field(ni, "Fac")?, ev.field(ni, "Vector")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let f = at(x[0], i).f();
                        let v = at(x[1], i).v();
                        let mut o = [0.0f32; 3];
                        for c in 0..3 {
                            let cv = fcs[c].eval(v[c] as f64) as f32;
                            o[c] = v[c] * (1.0 - f) + cv * f;
                        }
                        Val::V(o)
                    })
                    .collect()]
            });
            outs(node, vec![("Vector", fld(mf1(k, vec![fac, v])))])
        }
        "ShaderNodeMapping" => {
            let vt = node.ps("vector_type").to_string();
            let (v, loc, rot, sc) = (ev.field(ni, "Vector")?, ev.field(ni, "Location")?, ev.field(ni, "Rotation")?, ev.field(ni, "Scale")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let (v, loc, rot, sc) = (at(x[0], i).v(), at(x[1], i).v(), at(x[2], i).v(), at(x[3], i).v());
                        let m = crate::bl32::eul_to_mat3(rot);
                        let mulm = |mm: &crate::bl32::M3, p: V3| [mm[0][0] * p[0] + mm[1][0] * p[1] + mm[2][0] * p[2], mm[0][1] * p[0] + mm[1][1] * p[1] + mm[2][1] * p[2], mm[0][2] * p[0] + mm[1][2] * p[1] + mm[2][2] * p[2]];
                        let mult = |mm: &crate::bl32::M3, p: V3| [mm[0][0] * p[0] + mm[0][1] * p[1] + mm[0][2] * p[2], mm[1][0] * p[0] + mm[1][1] * p[1] + mm[1][2] * p[2], mm[2][0] * p[0] + mm[2][1] * p[1] + mm[2][2] * p[2]];
                        let sd = |a: V3, b: V3| [safe_div(a[0], b[0]), safe_div(a[1], b[1]), safe_div(a[2], b[2])];
                        Val::V(match vt.as_str() {
                            "TEXTURE" => sd(mult(&m, sub3(v, loc)), sc),
                            "VECTOR" => mulm(&m, [v[0] * sc[0], v[1] * sc[1], v[2] * sc[2]]),
                            "NORMAL" => norm3(mulm(&m, sd(v, sc))),
                            _ => add3(mulm(&m, [v[0] * sc[0], v[1] * sc[1], v[2] * sc[2]]), loc),
                        })
                    })
                    .collect()]
            });
            outs(node, vec![("Vector", fld(mf1(k, vec![v, loc, rot, sc])))])
        }
        "ShaderNodeValToRGB" => {
            let ramp = node.pj("ramp").ok_or("ramp missing")?.clone();
            let f = ev.field(ni, "Fac")?;
            let k = kern(move |x, n| {
                let cs: Vec<[f32; 4]> = (0..n).map(|i| color_ramp_eval(&ramp, at(x[0], i).f())).collect();
                vec![cs.iter().map(|c| Val::C([c[0], c[1], c[2], 1.0])).collect(), cs.iter().map(|c| Val::F(c[3])).collect()]
            });
            let r = mf(k, vec![f], 2);
            outs(node, vec![("Color", fld(r[0].clone())), ("Alpha", fld(r[1].clone()))])
        }
        "ShaderNodeMix" => {
            let dt = node.ps("data_type").to_string();
            let clamp_f = node.pb("clamp_factor");
            let clamp_r = node.pb("clamp_result");
            let blend = node.ps("blend_type").to_string();
            let fac = ev.field(ni, "Factor_Float")?;
            let (ia, ib, oid) = match dt.as_str() {
                "VECTOR" => ("A_Vector", "B_Vector", "Result_Vector"),
                "RGBA" => ("A_Color", "B_Color", "Result_Color"),
                "ROTATION" => ("A_Rotation", "B_Rotation", "Result_Rotation"),
                _ => ("A_Float", "B_Float", "Result_Float"),
            };
            let (a, b) = (ev.field(ni, ia)?, ev.field(ni, ib)?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let mut f = at(x[0], i).f();
                        if clamp_f {
                            f = f.clamp(0.0, 1.0);
                        }
                        match dt.as_str() {
                            "VECTOR" => {
                                let (a, b) = (at(x[1], i).v(), at(x[2], i).v());
                                Val::V([a[0] * (1.0 - f) + b[0] * f, a[1] * (1.0 - f) + b[1] * f, a[2] * (1.0 - f) + b[2] * f])
                            }
                            "RGBA" => {
                                let (a, b) = (at(x[1], i).c(), at(x[2], i).c());
                                let mut c = blend_color(&blend, f, a, b);
                                if clamp_r {
                                    for k in 0..4 {
                                        c[k] = c[k].clamp(0.0, 1.0);
                                    }
                                }
                                Val::C(c)
                            }
                            "ROTATION" => {
                                let (a, b) = (at(x[1], i).q(), at(x[2], i).q());
                                Val::Q(slerp(a, b, f))
                            }
                            _ => {
                                let (a, b) = (at(x[1], i).f(), at(x[2], i).f());
                                let mut r = a * (1.0 - f) + b * f;
                                if clamp_r {
                                    r = r.clamp(0.0, 1.0);
                                }
                                Val::F(r)
                            }
                        }
                    })
                    .collect()]
            });
            outs(node, vec![(oid, fld(mf1(k, vec![fac, a, b])))])
        }
        "ShaderNodeMixRGB" => {
            let blend = node.ps("blend_type").to_string();
            let clamp = node.pb("use_clamp");
            let (f, a, b) = (ev.field(ni, "Fac")?, ev.field(ni, "Color1")?, ev.field(ni, "Color2")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let f = at(x[0], i).f().clamp(0.0, 1.0);
                        let mut c = blend_color(&blend, f, at(x[1], i).c(), at(x[2], i).c());
                        if clamp {
                            for k in 0..3 {
                                c[k] = c[k].clamp(0.0, 1.0);
                            }
                        }
                        Val::C(c)
                    })
                    .collect()]
            });
            outs(node, vec![("Color", fld(mf1(k, vec![f, a, b])))])
        }
        "FunctionNodeEulerToRotation" => {
            let e = ev.field(ni, "Euler")?;
            outs(node, vec![("Rotation", fld(map1(&e, |v| Val::Q(euler_to_quat(v.v())))))])
        }
        "FunctionNodeRotationToEuler" => {
            let e = ev.field(ni, "Rotation")?;
            outs(node, vec![("Euler", fld(map1(&e, |v| Val::V(quat_to_euler(v.q())))))])
        }
        "FunctionNodeInvertRotation" => {
            let e = ev.field(ni, "Rotation")?;
            outs(node, vec![("Rotation", fld(map1(&e, |v| {
                let q = v.q();
                Val::Q([q[0], -q[1], -q[2], -q[3]])
            })))])
        }
        "FunctionNodeAxisAngleToRotation" => {
            let (a, ang) = (ev.field(ni, "Axis")?, ev.field(ni, "Angle")?);
            outs(node, vec![("Rotation", fld(map2(&a, &ang, |a, ang| Val::Q(axis_angle_to_quat(a.v(), ang.f())))))])
        }
        "FunctionNodeRotateVector" => {
            let (v, r) = (ev.field(ni, "Vector")?, ev.field(ni, "Rotation")?);
            outs(node, vec![("Vector", fld(map2(&v, &r, |v, r| Val::V(quat_rotate(r.q(), v.v())))))])
        }
        "FunctionNodeAlignEulerToVector" => {
            let axis = node.ps("axis").to_string();
            let pivot = node.ps("pivot_axis").to_string();
            let (r, f, v) = (ev.field(ni, "Rotation")?, ev.field(ni, "Factor")?, ev.field(ni, "Vector")?);
            let k = kern(move |x, n| vec![(0..n).map(|i| Val::V(align_euler_to_vector(at(x[0], i).v(), at(x[1], i).f(), at(x[2], i).v(), &axis, &pivot))).collect()]);
            outs(node, vec![("Rotation", fld(mf1(k, vec![r, f, v])))])
        }
        "FunctionNodeAlignRotationToVector" => {
            let axis = node.ps("axis").to_string();
            let pivot = node.ps("pivot_axis").to_string();
            let (r, f, v) = (ev.field(ni, "Rotation")?, ev.field(ni, "Factor")?, ev.field(ni, "Vector")?);
            let k = kern(move |x, n| vec![(0..n).map(|i| Val::Q(align_rotation_to_vector(at(x[0], i).q(), at(x[1], i).f(), at(x[2], i).v(), &axis, &pivot))).collect()]);
            outs(node, vec![("Rotation", fld(mf1(k, vec![r, f, v])))])
        }
        "FunctionNodeRotateEuler" => {
            let axis_angle = node.ps("rotation_type") == "AXIS_ANGLE";
            let local = node.ps("space") == "LOCAL";
            let r = ev.field(ni, "Rotation")?;
            let (a, b, c) = if axis_angle {
                (ev.field(ni, "Axis")?, ev.field(ni, "Angle")?, kf(0.0))
            } else {
                (ev.field(ni, "Rotate By")?, kf(0.0), kf(0.0))
            };
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let rot = euler_to_quat(at(x[0], i).v());
                        let by = if axis_angle { axis_angle_to_quat(at(x[1], i).v(), at(x[2], i).f()) } else { euler_to_quat(at(x[1], i).v()) };
                        let res = if local { quat_mul(rot, by) } else { quat_mul(by, rot) };
                        Val::V(quat_to_euler(res))
                    })
                    .collect()]
            });
            outs(node, vec![("Rotation", fld(mf1(k, vec![r, a, b, c])))])
        }
        "ShaderNodeVectorRotate" => {
            let mode = node.ps("rotation_type").to_string();
            let inv = node.pb("invert");
            let get = |id: &str| -> Res<Field> { if ev.has_input(ni, id) { ev.field(ni, id) } else { Ok(konst(Val::V([0.0; 3]))) } };
            let (v, c, ax, ang, rot) = (ev.field(ni, "Vector")?, ev.field(ni, "Center")?, get("Axis")?, if ev.has_input(ni, "Angle") { ev.field(ni, "Angle")? } else { kf(0.0) }, get("Rotation")?);
            let k = kern(move |x, n| {
                vec![(0..n)
                    .map(|i| {
                        let (v, c, ax, ang, rot) = (at(x[0], i).v(), at(x[1], i).v(), at(x[2], i).v(), at(x[3], i).f(), at(x[4], i).v());
                        Val::V(vector_rotate(&mode, inv, v, c, ax, ang, rot))
                    })
                    .collect()]
            });
            outs(node, vec![("Vector", fld(mf1(k, vec![v, c, ax, ang, rot])))])
        }
        "GeometryNodeInputPosition" => outs(node, vec![("Position", fld(Rc::new(FNode::Pos)))]),
        "GeometryNodeInputNormal" => outs(node, vec![("Normal", fld(Rc::new(FNode::Normal)))]),
        "GeometryNodeInputIndex" => outs(node, vec![("Index", fld(Rc::new(FNode::Index)))]),
        "GeometryNodeInputID" => outs(node, vec![("ID", fld(Rc::new(FNode::Id)))]),
        "GeometryNodeInputTangent" => outs(node, vec![("Tangent", fld(Rc::new(FNode::Tangent)))]),
        "GeometryNodeSplineParameter" => outs(
            node,
            vec![("Factor", fld(Rc::new(FNode::SplineParam(0)))), ("Length", fld(Rc::new(FNode::SplineParam(1)))), ("Index", fld(Rc::new(FNode::SplineParam(2))))],
        ),
        "GeometryNodeInputNamedAttribute" => {
            let name = ev.cval(ni, "Name")?.s().to_string();
            let t = Ty::from_data_type(node.ps("data_type"));
            let nm = name.clone();
            let exists = Rc::new(FNode::Ctx(Rc::new(move |c: &Ctx| {
                let e = super::attr::has_attr(c, &nm) || (nm == "position" && !matches!(c.comp, Comp::None));
                vec![Val::B(e); c.n.max(1)]
            })));
            outs(node, vec![("Attribute", fld(Rc::new(FNode::Named(name, t)))), ("Exists", fld(exists))])
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

fn cmp_f(op: &str, a: f32, b: f32, c: f32) -> bool {
    match op {
        "LESS_THAN" => a < b,
        "LESS_EQUAL" => a <= b,
        "GREATER_THAN" => a > b,
        "GREATER_EQUAL" => a >= b,
        "EQUAL" => (a - b).abs() <= c,
        "NOT_EQUAL" => (a - b).abs() > c,
        _ => false,
    }
}

fn m4_det(m: &[[f32; 4]; 4]) -> f32 {
    let a: Vec<f64> = m.iter().flat_map(|c| c.iter().map(|x| *x as f64)).collect();
    det4(&a) as f32
}
fn det4(a: &[f64]) -> f64 {
    // a[c*4+r]
    let g = |r: usize, c: usize| a[c * 4 + r];
    let mut d = 0.0;
    for c in 0..4 {
        let mut mm = [[0.0f64; 3]; 3];
        for rr in 1..4 {
            let mut cc2 = 0;
            for cc in 0..4 {
                if cc == c {
                    continue;
                }
                mm[rr - 1][cc2] = g(rr, cc);
                cc2 += 1;
            }
        }
        let m3 = mm[0][0] * (mm[1][1] * mm[2][2] - mm[1][2] * mm[2][1]) - mm[0][1] * (mm[1][0] * mm[2][2] - mm[1][2] * mm[2][0]) + mm[0][2] * (mm[1][0] * mm[2][1] - mm[1][1] * mm[2][0]);
        d += if c % 2 == 0 { 1.0 } else { -1.0 } * g(0, c) * m3;
    }
    d
}
fn m4_invert(m: &[[f32; 4]; 4]) -> ([[f32; 4]; 4], bool) {
    // Gauss-Jordan in f64
    let mut a = [[0.0f64; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            a[r][c] = m[c][r] as f64;
        }
        a[r][4 + r] = 1.0;
    }
    for i in 0..4 {
        let mut p = i;
        for r in i + 1..4 {
            if a[r][i].abs() > a[p][i].abs() {
                p = r;
            }
        }
        if a[p][i].abs() < 1e-30 {
            return ([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]], false);
        }
        a.swap(i, p);
        let d = a[i][i];
        for c in 0..8 {
            a[i][c] /= d;
        }
        for r in 0..4 {
            if r != i {
                let f = a[r][i];
                for c in 0..8 {
                    a[r][c] -= f * a[i][c];
                }
            }
        }
    }
    let mut o = [[0.0f32; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            o[c][r] = a[r][4 + c] as f32;
        }
    }
    (o, true)
}
