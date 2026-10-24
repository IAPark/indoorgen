//! More geometry nodes: instances, statistics, separate/delete, shading, curve attribute setters.
use super::attr::*;
use super::eval::*;
use super::field::*;
use super::geom::*;
use super::load::*;
use super::math::*;
use super::meshops::*;
use super::nodes::*;
use super::nodes_geo::*;
use super::val::*;
use std::rc::Rc;

fn m4_translate(t: V3) -> M4 {
    let mut m = M4_ID;
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}
fn m4_from_quat(q: [f32; 4]) -> M4 {
    matrix_from_trs([0.0; 3], q, [1.0; 3])
}
fn m4_scale(s: V3) -> M4 {
    let mut m = M4_ID;
    m[0][0] = s[0];
    m[1][1] = s[1];
    m[2][2] = s[2];
    m
}

/// element count of the first component having `dom`
fn domain_n(g: &Geometry, dom: Dom) -> usize {
    let mut n = 0;
    if let Some(m) = &g.mesh {
        n += m.size(dom);
    }
    if let Some(c) = &g.curves {
        n += c.size(dom);
    }
    if let Some(p) = &g.points {
        if dom == Dom::Point {
            n += p.pos.len();
        }
    }
    if let Some(i) = &g.inst {
        if dom == Dom::Instance {
            n += i.n();
        }
    }
    n
}

fn stat_values(g: &Geometry, dom: Dom, sel: &Field, attr: &Field) -> Vec<Val> {
    let mut vals = vec![];
    let mut add = |comp: Comp| {
        if dom_valid(comp, dom) {
            let v = eval_on(attr, comp, dom);
            let s = eval_on(sel, comp, dom);
            for i in 0..v.len() {
                if s[i].b() {
                    vals.push(v[i].clone());
                }
            }
        }
    };
    if let Some(m) = &g.mesh {
        add(Comp::Mesh(m));
    }
    if let Some(p) = &g.points {
        add(Comp::Points(p));
    }
    if let Some(c) = &g.curves {
        add(Comp::Curves(c));
    }
    if let Some(i) = &g.inst {
        add(Comp::Inst(i));
    }
    vals
}

fn stats_f(v: &[f32]) -> [f32; 8] {
    // mean, median, sum, min, max, range, stddev, variance
    if v.is_empty() {
        return [0.0; 8];
    }
    let n = v.len() as f32;
    let mut sum = 0.0f32;
    let mut mn = f32::MAX;
    let mut mx = f32::MIN;
    for &x in v {
        sum += x;
        mn = mn.min(x);
        mx = mx.max(x);
    }
    let mean = sum / n;
    let mut sq = 0.0f32;
    for &x in v {
        let d = x - mean;
        sq += d * d;
    }
    let var = sq / n;
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if s.len() % 2 == 1 { s[s.len() / 2] } else { (s[s.len() / 2 - 1] + s[s.len() / 2]) * 0.5 };
    [mean, median, sum, mn, mx, mx - mn, var.sqrt(), var]
}

pub fn instance_rotation_apply(x: &M4, op: &M4, local: bool) -> M4 {
    if local {
        m4_mul(x, op)
    } else {
        m4_mul(op, x)
    }
}

pub fn geo_nodes2(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "GeometryNodeInstanceOnPoints" => {
            let pts = ev.geo(ni, "Points")?;
            let inst_geo = ev.geo(ni, "Instance")?;
            let sel = ev.field(ni, "Selection")?;
            let rot = ev.field(ni, "Rotation")?;
            let scl = ev.field(ni, "Scale")?;
            let pick = ev.field(ni, "Pick Instance")?;
            let iidx = if ev.is_linked(ni, "Instance Index") { ev.field(ni, "Instance Index")? } else { Rc::new(FNode::Id) }; // implicit default: id or index
            let mut out = Instances::default();
            // instance references
            let (refs, multi): (Vec<Rc<Geometry>>, bool) = match &inst_geo.inst {
                Some(i) if !i.refs.is_empty() => (i.refs.clone(), true),
                _ => (vec![inst_geo.clone()], false),
            };
            out.refs = refs;
            let nrefs = out.refs.len() as i32;
            let mut sources: Vec<Comp> = vec![];
            if let Some(m) = &pts.mesh {
                sources.push(Comp::Mesh(m));
            }
            if let Some(p) = &pts.points {
                sources.push(Comp::Points(p));
            }
            if let Some(c) = &pts.curves {
                sources.push(Comp::Curves(c));
            }
            let mut inst_attrs: Vec<(Attrs, usize)> = vec![];
            for comp in sources {
                let n = comp_size(comp, Dom::Point);
                let s = eval_on(&sel, comp, Dom::Point);
                let p = eval_on(&Rc::new(FNode::Pos), comp, Dom::Point);
                let rq = eval_on(&rot, comp, Dom::Point);
                let sc = eval_on(&scl, comp, Dom::Point);
                let pk = eval_on(&pick, comp, Dom::Point);
                let ix = eval_on(&iidx, comp, Dom::Point);
                let mut selected = vec![];
                for i in 0..n {
                    if !s[i].b() {
                        continue;
                    }
                    out.xf.push(matrix_from_trs(p[i].v(), rq[i].q(), sc[i].v()));
                    let k = if multi && pk[i].b() { ix[i].i().rem_euclid(nrefs.max(1)) } else { 0 };
                    out.idx.push(k as u32);
                    selected.push(i);
                }
                // propagate point attributes to the instance domain
                if let Some(attrs) = attrs_of(comp) {
                    let mut sub = Attrs::new();
                    for (name, a) in attrs.iter() {
                        let data: Vec<Val> = match comp {
                            Comp::Mesh(m) => adapt_mesh(m, a.dom, Dom::Point, a.ty, &a.data),
                            Comp::Curves(c) => adapt_curves(c, a.dom, Dom::Point, a.ty, &a.data),
                            _ => a.data.clone(),
                        };
                        sub.insert(name.clone(), Attr { dom: Dom::Instance, ty: a.ty, data: selected.iter().map(|&i| data[i].clone()).collect() });
                    }
                    inst_attrs.push((sub, selected.len()));
                }
            }
            if inst_attrs.len() == 1 {
                out.attrs = inst_attrs.pop().unwrap().0;
            } else if inst_attrs.len() > 1 {
                let parts: Vec<(&Attrs, Box<dyn Fn(Dom) -> usize>)> = inst_attrs.iter().map(|(a, n)| { let n = *n; (a, Box::new(move |_d: Dom| n) as Box<dyn Fn(Dom) -> usize>) }).collect();
                let refs: Vec<(&Attrs, &dyn Fn(Dom) -> usize)> = parts.iter().map(|(a, f)| (*a, &**f)).collect();
                let total = out.n();
                out.attrs = join_attrs(&refs, &|_| total);
            }
            let mut g = Geometry::default();
            g.inst = Some(out);
            outs(node, vec![("Instances", rcgeo(g))])
        }
        "GeometryNodeRotateInstances" | "GeometryNodeScaleInstances" | "GeometryNodeTranslateInstances" => {
            let g = ev.geo(ni, "Instances")?;
            let sel = ev.field(ni, "Selection")?;
            let mut o = (*g).clone();
            if let Some(inst) = o.inst.as_mut() {
                let comp = Comp::Inst(&g.inst.as_ref().unwrap());
                let s = eval_on(&sel, comp, Dom::Instance);
                let local = eval_on(&ev.field(ni, "Local Space")?, comp, Dom::Instance);
                match ty {
                    "GeometryNodeRotateInstances" => {
                        let rot = eval_on(&ev.field(ni, "Rotation")?, comp, Dom::Instance);
                        let piv = eval_on(&ev.field(ni, "Pivot Point")?, comp, Dom::Instance);
                        for i in 0..inst.n() {
                            if !s[i].b() {
                                continue;
                            }
                            let p = piv[i].v();
                            if std::env::var("ROTDBG").is_ok() {
                                eprintln!("ROT {:?} piv {:?} local {:?} xf {:?}", rot[i], p, local[i].b(), inst.xf[i]);
                            }
                            let rm = m4_from_quat(rot[i].q());
                            let op = m4_mul(&m4_mul(&m4_translate(p), &rm), &m4_translate([-p[0], -p[1], -p[2]]));
                            inst.xf[i] = instance_rotation_apply(&inst.xf[i], &op, local[i].b());
                        }
                    }
                    "GeometryNodeScaleInstances" => {
                        let sc = eval_on(&ev.field(ni, "Scale")?, comp, Dom::Instance);
                        let ce = eval_on(&ev.field(ni, "Center")?, comp, Dom::Instance);
                        for i in 0..inst.n() {
                            if !s[i].b() {
                                continue;
                            }
                            let c = ce[i].v();
                            let sm = m4_scale(sc[i].v());
                            let op = m4_mul(&m4_mul(&m4_translate(c), &sm), &m4_translate([-c[0], -c[1], -c[2]]));
                            inst.xf[i] = instance_rotation_apply(&inst.xf[i], &op, local[i].b());
                        }
                    }
                    _ => {
                        let tr = eval_on(&ev.field(ni, "Translation")?, comp, Dom::Instance);
                        for i in 0..inst.n() {
                            if !s[i].b() {
                                continue;
                            }
                            let t = tr[i].v();
                            if local[i].b() {
                                inst.xf[i] = m4_mul(&inst.xf[i], &m4_translate(t));
                            } else {
                                inst.xf[i][3][0] += t[0];
                                inst.xf[i][3][1] += t[1];
                                inst.xf[i][3][2] += t[2];
                            }
                        }
                    }
                }
            }
            outs(node, vec![("Instances", rcgeo(o))])
        }
        "GeometryNodeAttributeStatistic" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let sel = ev.field(ni, "Selection")?;
            let attr = ev.field(ni, "Attribute")?;
            let vals = stat_values(&g, dom, &sel, &attr);
            let names = ["Mean", "Median", "Sum", "Min", "Max", "Range", "Standard Deviation", "Variance"];
            if node.ps("data_type") == "FLOAT_VECTOR" {
                let mut res = vec![[0.0f32; 3]; 8];
                for c in 0..3 {
                    let v: Vec<f32> = vals.iter().map(|x| x.v()[c]).collect();
                    let st = stats_f(&v);
                    for k in 0..8 {
                        res[k][c] = st[k];
                    }
                }
                outs(node, names.iter().enumerate().map(|(k, n)| (*n, Value::konst(Val::V(res[k])))).collect())
            } else {
                let v: Vec<f32> = vals.iter().map(|x| x.f()).collect();
                let st = stats_f(&v);
                outs(node, names.iter().enumerate().map(|(k, n)| (*n, Value::konst(Val::F(st[k])))).collect())
            }
        }
        "GeometryNodeAttributeDomainSize" => {
            let g = ev.geo(ni, "Geometry")?;
            let comp = node.ps("component");
            let mut v: Vec<(&str, Value)> = vec![];
            match comp {
                "MESH" => {
                    let m = g.mesh.as_ref();
                    v.push(("Point Count", Value::konst(Val::I(m.map_or(0, |m| m.nv()) as i32))));
                    v.push(("Edge Count", Value::konst(Val::I(m.map_or(0, |m| m.ne()) as i32))));
                    v.push(("Face Count", Value::konst(Val::I(m.map_or(0, |m| m.nf()) as i32))));
                    v.push(("Face Corner Count", Value::konst(Val::I(m.map_or(0, |m| m.nc()) as i32))));
                }
                "CURVE" => {
                    let c = g.curves.as_ref();
                    v.push(("Point Count", Value::konst(Val::I(c.map_or(0, |c| c.np()) as i32))));
                    v.push(("Spline Count", Value::konst(Val::I(c.map_or(0, |c| c.nc()) as i32))));
                }
                "POINTCLOUD" => v.push(("Point Count", Value::konst(Val::I(g.points.as_ref().map_or(0, |p| p.pos.len()) as i32)))),
                _ => v.push(("Instance Count", Value::konst(Val::I(g.inst.as_ref().map_or(0, |i| i.n()) as i32)))),
            }
            outs(node, v)
        }
        "GeometryNodeSeparateGeometry" | "GeometryNodeDeleteGeometry" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let sel = ev.field(ni, "Selection")?;
            let is_delete = ty.ends_with("DeleteGeometry");
            let keep_mode = |invert: bool| -> Geometry { separate(&g, dom, &sel, invert, node.ps("mode")) };
            if is_delete {
                outs(node, vec![("Geometry", rcgeo(keep_mode(true)))])
            } else {
                outs(node, vec![("Selection", rcgeo(keep_mode(false))), ("Inverted", rcgeo(keep_mode(true)))])
            }
        }
        "GeometryNodeSetShadeSmooth" => {
            let g = ev.geo(ni, "Geometry")?;
            let sel = ev.field(ni, "Selection")?;
            let smooth = ev.field(ni, "Shade Smooth")?;
            let dom = Dom::from_name(node.ps("domain"));
            let mut o = (*g).clone();
            if let Some(m) = o.mesh.as_mut() {
                let inv = map1(&smooth, |v| Val::B(!v.b()));
                let name = if dom == Dom::Edge { "sharp_edge" } else { "sharp_face" };
                let mut tmp = Geometry::from_mesh(std::mem::take(m));
                store_field(&mut tmp, name, dom, Ty::B, &inv, &sel);
                *m = tmp.mesh.unwrap();
            }
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeFlipFaces" => {
            let g = ev.geo(ni, "Mesh")?;
            let mut o = (*g).clone();
            if let Some(m) = o.mesh.as_mut() {
                let sel = selection(ev, ni, Comp::Mesh(&g.mesh.as_ref().unwrap()), Dom::Face)?;
                flip_faces(m, &sel);
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeSetCurveRadius" | "GeometryNodeSetCurveTilt" => {
            let g = ev.geo(ni, "Curve")?;
            let sel = ev.field(ni, "Selection")?;
            let (name, id) = if ty.ends_with("Radius") { ("radius", "Radius") } else { ("tilt", "Tilt") };
            let f = ev.field(ni, id)?;
            let mut o = (*g).clone();
            if o.curves.is_some() {
                let mut tmp = Geometry::from_curves(o.curves.take().unwrap());
                store_field(&mut tmp, name, Dom::Point, Ty::F, &f, &sel);
                o.curves = tmp.curves;
            }
            outs(node, vec![("Curve", rcgeo(o))])
        }
        "GeometryNodeGeometryToInstance" => {
            let mut gs: Vec<Rc<Geometry>> = vec![];
            for v in ev.input_multi(ni, "Geometry")? {
                if let Value::G(g) = v {
                    gs.push(g);
                }
            }
            gs.reverse();
            let mut inst = Instances::default();
            for (i, g) in gs.into_iter().enumerate() {
                inst.refs.push(g);
                inst.idx.push(i as u32);
                inst.xf.push(M4_ID);
            }
            outs(node, vec![("Instances", rcgeo(Geometry { inst: Some(inst), ..Default::default() }))])
        }
        "GeometryNodePoints" => {
            let n = ev.ci(ni, "Count")?.max(0) as usize;
            let mut p = Points::default();
            p.pos = vec![[0.0; 3]; n];
            let comp = Comp::Points(&p);
            let pos: Vec<V3> = eval_on(&ev.field(ni, "Position")?, comp, Dom::Point).iter().map(|v| v.v()).collect();
            let rad: Vec<Val> = eval_on(&ev.field(ni, "Radius")?, comp, Dom::Point);
            p.pos = pos;
            p.attrs.insert("radius".into(), Attr { dom: Dom::Point, ty: Ty::F, data: rad });
            outs(node, vec![("Geometry", rcgeo(Geometry { points: Some(p), ..Default::default() }))])
        }
        "GeometryNodeMeshIcoSphere" => {
            let r = ev.cf(ni, "Radius")?;
            let sub = ev.ci(ni, "Subdivisions")?.clamp(1, 8);
            let mut m = super::prim::mesh_ico_sphere(r, sub as usize);
            m.vnor = None;
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m)))])
        }
        "GeometryNodeSetPointRadius" => {
            let g = ev.geo(ni, "Points")?;
            let sel = ev.field(ni, "Selection")?;
            let f = ev.field(ni, "Radius")?;
            let mut o = (*g).clone();
            if o.points.is_some() {
                let mut tmp = Geometry { points: o.points.take(), ..Default::default() };
                store_field(&mut tmp, "radius", Dom::Point, Ty::F, &f, &sel);
                o.points = tmp.points;
            }
            outs(node, vec![("Points", rcgeo(o))])
        }
        "GeometryNodeMeshToPoints" => {
            let g = ev.geo(ni, "Mesh")?;
            let mut o = Geometry::default();
            if let Some(m) = &g.mesh {
                let mode = node.ps("mode");
                let dom = match mode {
                    "EDGES" => Dom::Edge,
                    "FACES" => Dom::Face,
                    "CORNERS" => Dom::Corner,
                    _ => Dom::Point,
                };
                let comp = Comp::Mesh(m);
                let sel = selection(ev, ni, comp, dom)?;
                let pos = eval_on(&ev.field(ni, "Position")?, comp, dom);
                let mut p = Points::default();
                let mut idx = vec![];
                for i in 0..m.size(dom) {
                    if sel[i] {
                        p.pos.push(pos[i].v());
                        idx.push(i);
                    }
                }
                for (name, a) in m.attrs.iter() {
                    let data = adapt_mesh(m, a.dom, dom, a.ty, &a.data);
                    p.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: idx.iter().map(|&i| data[i].clone()).collect() });
                }
                if ev.has_input(ni, "Radius") {
                    let rad = eval_on(&ev.field(ni, "Radius")?, comp, dom);
                    p.attrs.insert("radius".into(), Attr { dom: Dom::Point, ty: Ty::F, data: idx.iter().map(|&i| rad[i].clone()).collect() });
                }
                o.points = Some(p);
            }
            outs(node, vec![("Points", rcgeo(o))])
        }
        "GeometryNodePointsToVertices" => {
            let g = ev.geo(ni, "Points")?;
            let mut o = Geometry::default();
            let mut mesh = Mesh::new();
            if let Some(p) = &g.points {
                let sel = selection(ev, ni, Comp::Points(p), Dom::Point)?;
                let keep: Vec<usize> = (0..p.pos.len()).filter(|&i| sel[i]).collect();
                mesh.pos = keep.iter().map(|&i| p.pos[i]).collect();
                for (name, a) in p.attrs.iter() {
                    mesh.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: keep.iter().map(|&i| a.data[i].clone()).collect() });
                }
            }
            o.mesh = Some(mesh);
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeObjectInfo" => {
            let name = ev.cval(ni, "Object")?.s().to_string();
            let as_inst = ev.cb(ni, "As Instance").unwrap_or(false);
            let relative = node.ps("transform_space") == "RELATIVE";
            match ev.objects.map.get(&name) {
                None => outs(node, vec![("Geometry", Value::empty_geo())]),
                Some(o) => {
                    let m = o.matrix;
                    let (loc, q, sc) = crate::bl32::decompose(&m);
                    let geo = if as_inst {
                        let mut inst = Instances::default();
                        inst.refs.push(Rc::new(o.geo.clone()));
                        inst.idx.push(0);
                        inst.xf.push(if relative { m } else { M4_ID });
                        Geometry { inst: Some(inst), ..Default::default() }
                    } else if relative {
                        transform_geometry(&o.geo, &m, None)
                    } else {
                        o.geo.clone()
                    };
                    let (l, qq, s) = if relative { (loc, q, sc) } else { ([0.0; 3], [1.0, 0.0, 0.0, 0.0], [1.0; 3]) };
                    outs(node, vec![("Location", Value::konst(Val::V(l))), ("Rotation", Value::konst(Val::Q(qq))), ("Scale", Value::konst(Val::V(s))), ("Geometry", rcgeo(geo))])
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

pub fn flip_faces(m: &mut Mesh, sel: &[bool]) {
    for f in 0..m.nf() {
        if !sel[f] {
            continue;
        }
        let r = m.face_range(f);
        let n = r.len();
        let old_v: Vec<u32> = m.cv[r.clone()].to_vec();
        let old_e: Vec<u32> = m.ce[r.clone()].to_vec();
        let perm: Vec<usize> = (0..n).map(|i| if i == 0 { 0 } else { n - i }).collect();
        for i in 0..n {
            m.cv[r.start + i] = old_v[perm[i]];
            m.ce[r.start + i] = old_e[n - 1 - i];
        }
        for a in m.attrs.values_mut() {
            if a.dom == Dom::Corner {
                let old: Vec<Val> = a.data[r.clone()].to_vec();
                for i in 0..n {
                    a.data[r.start + i] = old[perm[i]].clone();
                }
            }
        }
    }
}

/// keep the selected (or unselected if invert) elements of domain `dom` in every component
pub fn separate(g: &Geometry, dom: Dom, sel: &Field, invert: bool, mode: &str) -> Geometry {
    let mut o = Geometry::default();
    let mask = |comp: Comp, d: Dom| -> Vec<bool> { eval_on(sel, comp, d).iter().map(|v| v.b() != invert).collect() };
    if let Some(m) = &g.mesh {
        let d = match dom {
            Dom::Curve | Dom::Instance => Dom::Point,
            d => d,
        };
        if matches!(dom, Dom::Point | Dom::Edge | Dom::Face) {
            let k = mask(Comp::Mesh(m), d);
            let mut nm = mesh_keep(m, d, &k, mode == "EDGE_FACE");
            if mode == "ONLY_FACE" && d == Dom::Face {
                // only faces are removed; keep every vertex / edge
                nm = mesh_keep_faces_only(m, &k);
            }
            o.mesh = Some(nm);
        }
    }
    if let Some(c) = &g.curves {
        match dom {
            Dom::Point | Dom::Edge | Dom::Face => {
                let k = mask(Comp::Curves(c), Dom::Point);
                o.curves = Some(curves_keep(c, Dom::Point, &k));
            }
            Dom::Curve => {
                let k = mask(Comp::Curves(c), Dom::Curve);
                o.curves = Some(curves_keep(c, Dom::Curve, &k));
            }
            _ => {}
        }
    }
    if let Some(p) = &g.points {
        if dom == Dom::Point {
            let k = mask(Comp::Points(p), Dom::Point);
            let idx: Vec<usize> = (0..p.pos.len()).filter(|&i| k[i]).collect();
            let mut np = Points::default();
            np.pos = idx.iter().map(|&i| p.pos[i]).collect();
            for (name, a) in p.attrs.iter() {
                np.attrs.insert(name.clone(), Attr { dom: a.dom, ty: a.ty, data: idx.iter().map(|&i| a.data[i].clone()).collect() });
            }
            o.points = Some(np);
        }
    }
    if let Some(i) = &g.inst {
        if dom == Dom::Instance || dom == Dom::Point {
            let k = mask(Comp::Inst(i), Dom::Instance);
            let idx: Vec<usize> = (0..i.n()).filter(|&j| k[j]).collect();
            let mut ni = Instances::default();
            ni.refs = i.refs.clone();
            ni.idx = idx.iter().map(|&j| i.idx[j]).collect();
            ni.xf = idx.iter().map(|&j| i.xf[j]).collect();
            for (name, a) in i.attrs.iter() {
                ni.attrs.insert(name.clone(), Attr { dom: a.dom, ty: a.ty, data: idx.iter().map(|&j| a.data[j].clone()).collect() });
            }
            o.inst = Some(ni);
        } else {
            o.inst = Some(i.clone());
        }
    }
    o
}

pub fn mesh_keep_faces_only(m: &Mesh, keep: &[bool]) -> Mesh {
    let mut o = m.clone();
    // remove faces, keep verts and edges
    let mut nm = Mesh::new();
    nm.pos = m.pos.clone();
    nm.edges = m.edges.clone();
    nm.mats = m.mats.clone();
    let mut fsrc = vec![];
    let mut csrc = vec![];
    for f in 0..m.nf() {
        if keep[f] {
            for c in m.face_range(f) {
                nm.cv.push(m.cv[c]);
                nm.ce.push(m.ce[c]);
                csrc.push(c);
            }
            nm.foff.push(nm.cv.len() as u32);
            fsrc.push(f);
        }
    }
    for (name, a) in m.attrs.iter() {
        let data = match a.dom {
            Dom::Face => fsrc.iter().map(|&i| a.data[i].clone()).collect(),
            Dom::Corner => csrc.iter().map(|&i| a.data[i].clone()).collect(),
            _ => a.data.clone(),
        };
        nm.attrs.insert(name.clone(), Attr { dom: a.dom, ty: a.ty, data });
    }
    std::mem::swap(&mut o, &mut nm);
    o
}
