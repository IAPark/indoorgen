//! Geometry-type nodes (primitives, transforms, attributes, curve/mesh conversion ...).
use super::attr::*;
use super::curveops::*;
use super::eval::*;
use super::field::*;
use super::geom::*;
use super::load::*;
use super::math::*;
use super::meshops::*;
use super::nodes::*;
use super::prim::*;
use super::val::*;
use std::rc::Rc;

pub fn rcgeo(g: Geometry) -> Value {
    Value::G(Rc::new(g))
}

/// bool mask from a selection field on a domain
pub fn selection(ev: &Ev, ni: usize, comp: Comp, dom: Dom) -> Res<Vec<bool>> {
    if !ev.has_input(ni, "Selection") {
        return Ok(vec![true; comp_size(comp, dom)]);
    }
    let f = ev.field(ni, "Selection")?;
    if let Some(c) = as_const(&f) {
        return Ok(vec![c.b(); comp_size(comp, dom)]);
    }
    Ok(eval_on(&f, comp, dom).iter().map(|v| v.b()).collect())
}

pub fn dom_valid(c: Comp, d: Dom) -> bool {
    match c {
        Comp::Mesh(_) => matches!(d, Dom::Point | Dom::Edge | Dom::Face | Dom::Corner),
        Comp::Curves(_) => matches!(d, Dom::Point | Dom::Curve),
        Comp::Inst(_) => d == Dom::Instance,
        Comp::Points(_) => d == Dom::Point,
        Comp::None => false,
    }
}

/// store data (len = domain size) as attribute `name` on the geometry's component
pub fn put_attr(attrs: &mut Attrs, name: &str, dom: Dom, ty: Ty, data: Vec<Val>) {
    attrs.insert(name.to_string(), Attr { dom, ty, data });
}

pub fn set_mesh_attr(m: &mut Mesh, name: &str, dom: Dom, ty: Ty, data: Vec<Val>) {
    if name == "position" && dom == Dom::Point {
        m.vnor = None;
        m.pos = data.iter().map(|v| v.v()).collect();
        return;
    }
    put_attr(&mut m.attrs, name, dom, ty, data);
}

pub fn set_curve_attr(c: &mut Curves, name: &str, dom: Dom, ty: Ty, data: Vec<Val>) {
    match (name, dom) {
        ("position", Dom::Point) => c.pos = data.iter().map(|v| v.v()).collect(),
        ("radius", Dom::Point) => {
            c.radius = data.iter().map(|v| v.f()).collect();
            c.has_radius = true;
        }
        ("tilt", Dom::Point) => c.tilt = data.iter().map(|v| v.f()).collect(),
        ("handle_left", Dom::Point) => c.hl = data.iter().map(|v| v.v()).collect(),
        ("handle_right", Dom::Point) => c.hr = data.iter().map(|v| v.v()).collect(),
        ("cyclic", Dom::Curve) => c.cyclic = data.iter().map(|v| v.b()).collect(),
        ("resolution", Dom::Curve) => c.res = data.iter().map(|v| v.i()).collect(),
        _ => put_attr(&mut c.attrs, name, dom, ty, data),
    }
}

fn read_old(comp: Comp, name: &str, dom: Dom, ty: Ty) -> Vec<Val> {
    let ctx = Ctx::new(comp, dom);
    let f: Field = Rc::new(FNode::Named(name.to_string(), ty));
    ctx.eval_n(&f)
}

/// Store a field as an attribute on every valid component of `g`.
pub fn store_field(g: &mut Geometry, name: &str, dom: Dom, ty: Ty, field: &Field, sel: &Field) {
    let merge = |comp: Comp, n: usize| -> Option<Vec<Val>> {
        if !dom_valid(comp, dom) {
            return None;
        }
        let vals = eval_on(field, comp, dom);
        let mask: Vec<bool> = match as_const(sel) {
            Some(c) => vec![c.b(); n],
            None => eval_on(sel, comp, dom).iter().map(|v| v.b()).collect(),
        };
        let all = mask.iter().all(|&b| b);
        let old = if all { vec![] } else { read_old(comp, name, dom, ty) };
        Some((0..n).map(|i| if mask[i] { vals[i].convert(ty) } else { old[i].convert(ty) }).collect())
    };
    if let Some(m) = g.mesh.as_ref() {
        let n = m.size(dom);
        if let Some(d) = merge(Comp::Mesh(m), n) {
            set_mesh_attr(g.mesh.as_mut().unwrap(), name, dom, ty, d);
        }
    }
    if let Some(c) = g.curves.as_ref() {
        let n = c.size(dom);
        if let Some(d) = merge(Comp::Curves(c), n) {
            set_curve_attr(g.curves.as_mut().unwrap(), name, dom, ty, d);
        }
    }
    if let Some(p) = g.points.as_ref() {
        let n = if dom == Dom::Point { p.pos.len() } else { 0 };
        if let Some(d) = merge(Comp::Points(p), n) {
            if name == "position" {
                g.points.as_mut().unwrap().pos = d.iter().map(|v| v.v()).collect();
            } else {
                put_attr(&mut g.points.as_mut().unwrap().attrs, name, dom, ty, d);
            }
        }
    }
    if let Some(i) = g.inst.as_ref() {
        let n = if dom == Dom::Instance { i.n() } else { 0 };
        if let Some(d) = merge(Comp::Inst(i), n) {
            put_attr(&mut g.inst.as_mut().unwrap().attrs, name, dom, ty, d);
        }
    }
}

fn mat_index(m: &mut Mesh, name: &str) -> i32 {
    if m.mats.is_empty() {
        // evaluated meshes always carry an empty first slot
        m.mats.push(String::new());
    }
    if let Some(i) = m.mats.iter().position(|x| x == name) {
        return i as i32;
    }
    m.mats.push(name.to_string());
    m.mats.len() as i32 - 1
}

fn curve_geo(c: Curves) -> Value {
    rcgeo(Geometry::from_curves(c))
}

pub fn geo_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        // ---------------------------------------------------------------- primitives
        "GeometryNodeMeshCube" => {
            let size = ev.cv(ni, "Size")?;
            let n = [ev.ci(ni, "Vertices X")?, ev.ci(ni, "Vertices Y")?, ev.ci(ni, "Vertices Z")?];
            let mut m = mesh_cube(size, n);
            let un = anon_name("cube_uv");
            m.attrs.insert(un.clone(), Attr { dom: Dom::Corner, ty: Ty::V, data: cube_uv(n).iter().map(|u| Val::V([u[0], u[1], 0.0])).collect() });
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m))), ("UV Map", fld(Rc::new(FNode::Named(un, Ty::V))))])
        }
        "GeometryNodeMeshCylinder" | "GeometryNodeMeshCone" => {
            let fill = match node.ps("fill_type") {
                "NONE" => 0,
                "TRIANGLE_FAN" => 2,
                _ => 1,
            };
            let cyl = ty.ends_with("Cylinder");
            let (rt, rb) = if cyl {
                let r = ev.cf(ni, "Radius")?;
                (r, r)
            } else {
                (ev.cf(ni, "Radius Top")?, ev.cf(ni, "Radius Bottom")?)
            };
            let depth = ev.cf(ni, "Depth")?;
            let n = ev.ci(ni, "Vertices")?.max(3) as usize;
            let ss = ev.ci(ni, "Side Segments")?.max(1) as usize;
            let fs = if ev.has_input(ni, "Fill Segments") { ev.ci(ni, "Fill Segments")?.max(1) as usize } else { 1 };
            let co = mesh_cone(n, rt, rb, depth, ss, fs, fill, !cyl);
            let mut m = co.mesh;
            let (tn, bn, sn, un) = (anon_name("cone_top"), anon_name("cone_bottom"), anon_name("cone_side"), anon_name("cone_uv"));
            let put = |m: &mut Mesh, name: &str, dom: Dom, v: &Vec<bool>| {
                m.attrs.insert(name.to_string(), Attr { dom, ty: Ty::B, data: v.iter().map(|b| Val::B(*b)).collect() });
            };
            put(&mut m, &tn, co.top.0, &co.top.1);
            put(&mut m, &bn, co.bottom.0, &co.bottom.1);
            if !co.side.is_empty() {
                put(&mut m, &sn, Dom::Face, &co.side);
            }
            if !co.uv.is_empty() {
                m.attrs.insert(un.clone(), Attr { dom: Dom::Corner, ty: Ty::V, data: co.uv.iter().map(|u| Val::V([u[0], u[1], 0.0])).collect() });
            }
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m))), ("Top", fld(Rc::new(FNode::Named(tn, Ty::B)))), ("Bottom", fld(Rc::new(FNode::Named(bn, Ty::B)))), ("Side", fld(Rc::new(FNode::Named(sn, Ty::B)))), ("UV Map", fld(Rc::new(FNode::Named(un, Ty::V))))])
        }
        "GeometryNodeMeshUVSphere" => {
            let (mut m, uv) = mesh_uv_sphere_uv(ev.ci(ni, "Segments")?.max(3) as usize, ev.ci(ni, "Rings")?.max(2) as usize, ev.cf(ni, "Radius")?);
            let un = anon_name("sphere_uv");
            m.attrs.insert(un.clone(), Attr { dom: Dom::Corner, ty: Ty::V, data: uv.iter().map(|u| Val::V([u[0], u[1], 0.0])).collect() });
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m))), ("UV Map", fld(Rc::new(FNode::Named(un, Ty::V))))])
        }
        "GeometryNodeMeshLine" => {
            let start = ev.cv(ni, "Start Location")?;
            let off = ev.cv(ni, "Offset")?;
            let (count, delta) = if node.ps("mode") == "END_POINTS" {
                let d = sub3(off, start);
                if node.ps("count_mode") == "RESOLUTION" {
                    let res = ev.cf(ni, "Resolution")?;
                    (((len3(d) / res) as i32 + 1).max(1), mul3(norm3(d), res))
                } else {
                    let c = ev.ci(ni, "Count")?;
                    (c, if c > 1 { mul3(d, 1.0 / (c - 1) as f32) } else { [0.0; 3] })
                }
            } else {
                (ev.ci(ni, "Count")?, off)
            };
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(mesh_line(count.max(0) as usize, start, delta))))])
        }
        "GeometryNodeMeshGrid" => {
            let (sx, sy) = (ev.cf(ni, "Size X")?, ev.cf(ni, "Size Y")?);
            let mut m = mesh_grid(sx, sy, ev.ci(ni, "Vertices X")?.max(1) as usize, ev.ci(ni, "Vertices Y")?.max(1) as usize);
            let un = anon_name("grid_uv");
            if m.nf() > 0 {
                m.attrs.insert(un.clone(), Attr { dom: Dom::Corner, ty: Ty::V, data: grid_uv(&m, sx, sy).iter().map(|u| Val::V([u[0], u[1], 0.0])).collect() });
            }
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m))), ("UV Map", fld(Rc::new(FNode::Named(un, Ty::V))))])
        }
        "GeometryNodeMeshCircle" => {
            let fill = match node.ps("fill_type") {
                "NGON" => 1,
                "TRIANGLE_FAN" => 2,
                _ => 0,
            };
            let m = mesh_circle(ev.ci(ni, "Vertices")?.max(3) as usize, ev.cf(ni, "Radius")?, fill);
            outs(node, vec![("Mesh", rcgeo(Geometry::from_mesh(m)))])
        }
        "GeometryNodeCurvePrimitiveLine" => {
            let a = ev.cv(ni, "Start")?;
            let b = if node.ps("mode") == "DIRECTION" {
                let d = ev.cv(ni, "Direction")?;
                let l = ev.cf(ni, "Length")?;
                add3(a, mul3(norm3(d), l))
            } else {
                ev.cv(ni, "End")?
            };
            outs(node, vec![("Curve", curve_geo(curve_line(a, b)))])
        }
        "GeometryNodeCurvePrimitiveCircle" => {
            if node.ps("mode") == "POINTS" {
                return Err("CurveCircle POINTS mode unsupported".into());
            }
            let c = curve_circle(ev.ci(ni, "Resolution")?.max(3) as usize, ev.cf(ni, "Radius")?);
            outs(node, vec![("Curve", curve_geo(c)), ("Center", Value::konst(Val::V([0.0; 3])))])
        }
        "GeometryNodeCurvePrimitiveQuadrilateral" => {
            if node.ps("mode") != "RECTANGLE" {
                return Err("Quadrilateral mode unsupported".into());
            }
            outs(node, vec![("Curve", curve_geo(curve_quadrilateral_rect(ev.cf(ni, "Width")?, ev.cf(ni, "Height")?)))])
        }
        "GeometryNodeCurveQuadraticBezier" => {
            let c = curve_quadratic_bezier(ev.ci(ni, "Resolution")?.max(3) as usize, ev.cv(ni, "Start")?, ev.cv(ni, "Middle")?, ev.cv(ni, "End")?);
            outs(node, vec![("Curve", curve_geo(c))])
        }
        // ---------------------------------------------------------------- transform / join
        "GeometryNodeTransform" => {
            let g = ev.geo(ni, "Geometry")?;
            let (loc, rot, scale) = (ev.cv(ni, "Translation")?, ev.cval(ni, "Rotation")?.q(), ev.cv(ni, "Scale")?);
            let identity = rot == [1.0, 0.0, 0.0, 0.0] && scale == [1.0; 3];
            let m = matrix_from_trs(loc, rot, scale);
            let o = if identity { transform_geometry(&g, &m, Some(loc)) } else { transform_geometry(&g, &m, None) };
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeJoinGeometry" => {
            let mut gs: Vec<Rc<Geometry>> = vec![];
            for v in ev.input_multi(ni, "Geometry")? {
                if let Value::G(g) = v {
                    gs.push(g);
                }
            }
            gs.reverse();
            outs(node, vec![("Geometry", rcgeo(join_geometries(&gs)))])
        }
        "GeometryNodeSetPosition" => {
            let g = ev.geo(ni, "Geometry")?;
            let sel = ev.field(ni, "Selection")?;
            let pos = ev.field(ni, "Position")?;
            let off = ev.field(ni, "Offset")?;
            let mut o = (*g).clone();
            set_position_geo(&mut o, &sel, &pos, &off);
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeStoreNamedAttribute" => {
            let g = ev.geo(ni, "Geometry")?;
            let name = ev.cval(ni, "Name")?.s().to_string();
            let mut o = (*g).clone();
            if !name.is_empty() {
                let sel = ev.field(ni, "Selection")?;
                let val = ev.field(ni, "Value")?;
                let dom = Dom::from_name(node.ps("domain"));
                let t = Ty::from_data_type(node.ps("data_type"));
                store_field(&mut o, &name, dom, t, &val, &sel);
            }
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeCaptureAttribute" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let mut o = (*g).clone();
            let items_in: Vec<&Sock> = node.ins.iter().filter(|s| s.id != "Geometry" && s.id != "__extend__").collect();
            let items_out: Vec<&Sock> = node.outs.iter().filter(|s| s.id != "Geometry" && s.id != "__extend__").collect();
            let mut res: Vec<(&str, Value)> = vec![];
            let all = konst(Val::B(true));
            for (k, si) in items_in.iter().enumerate() {
                let f = ev.field(ni, &si.id)?;
                let anon = anon_name("anon");
                store_field(&mut o, &anon, dom, si.ty, &f, &all);
                if let Some(so) = items_out.get(k) {
                    res.push((so.id.as_str(), Value::F(Rc::new(FNode::Named(anon, si.ty)))));
                }
            }
            res.push(("Geometry", rcgeo(o)));
            outs(node, res)
        }
        "GeometryNodeSetMaterial" => {
            let g = ev.geo(ni, "Geometry")?;
            let name = ev.cval(ni, "Material")?.s().to_string();
            let mut o = (*g).clone();
            if let Some(m) = o.mesh.as_mut() {
                let sel = selection(ev, ni, Comp::Mesh(m), Dom::Face)?;
                let idx = mat_index(m, &name);
                let mut mi: Vec<Val> = match m.attrs.get("material_index") {
                    Some(a) => a.data.clone(),
                    None => vec![Val::I(0); m.nf()],
                };
                for f in 0..m.nf() {
                    if sel[f] {
                        mi[f] = Val::I(idx);
                    }
                }
                put_attr(&mut m.attrs, "material_index", Dom::Face, Ty::I, mi);
            }
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeSetMaterialIndex" => {
            let g = ev.geo(ni, "Geometry")?;
            let idx = ev.field(ni, "Material Index")?;
            let sel = ev.field(ni, "Selection")?;
            let mut o = (*g).clone();
            store_field(&mut o, "material_index", Dom::Face, Ty::I, &idx, &sel);
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeRealizeInstances" => {
            let g = ev.geo(ni, "Geometry")?;
            let depth = if ev.has_input(ni, "Depth") && !ev.cb(ni, "Realize All").unwrap_or(true) { ev.ci(ni, "Depth")? } else { -1 };
            outs(node, vec![("Geometry", rcgeo(realize_instances(&g, if depth < 0 { 1000 } else { depth - 1 })))])
        }
        "GeometryNodeBoundBox" => {
            let g = ev.geo(ni, "Geometry")?;
            // Min / Max: bounds of the realized geometry; Bounding Box: every geometry set (also the references of instances) is
            // replaced by the box of its own components, the instance structure stays
            fn box_sets(g: &Geometry) -> Geometry {
                let mut o = Geometry::default();
                if let Some((lo, hi)) = geometry_bounds(g, false) {
                    o.mesh = Some(bbox_mesh(lo, hi));
                }
                if let Some(i) = &g.inst {
                    let mut ni_ = i.clone();
                    ni_.refs = i.refs.iter().map(|r| Rc::new(box_sets(r))).collect();
                    o.inst = Some(ni_);
                }
                o
            }
            match geometry_bounds(&g, true) {
                Some((lo, hi)) => outs(node, vec![("Bounding Box", rcgeo(box_sets(&g))), ("Min", Value::konst(Val::V(lo))), ("Max", Value::konst(Val::V(hi)))]),
                None => outs(node, vec![("Bounding Box", Value::empty_geo())]),
            }
        }
        "GeometryNodeCurveToMesh" => {
            let c = ev.geo(ni, "Curve")?;
            let prof = ev.geo(ni, "Profile Curve")?;
            let caps = ev.cb(ni, "Fill Caps")?;
            if std::env::var("GNDUMP").is_ok() {
                eprintln!("CurveToMesh {}: main curves {:?} pts {:?}, profile curves {:?} pts {:?}, has mesh {}", node.name, c.curves.as_ref().map(|x| x.nc()), c.curves.as_ref().map(|x| x.np()), prof.curves.as_ref().map(|x| x.nc()), prof.curves.as_ref().map(|x| x.np()), c.mesh.is_some());
            }
            let mut o = Geometry::default();
            o.inst = c.inst.clone();
            // keep_only_during_modify({MESH}): a mesh component without curves passes through
            o.mesh = c.mesh.clone();
            if let Some(cv) = &c.curves {
                o.mesh = Some(curve_to_mesh(cv, prof.curves.as_ref(), caps));
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

fn set_position_geo(g: &mut Geometry, sel: &Field, pos: &Field, off: &Field) {
    if let Some(m) = g.mesh.as_ref() {
        let comp = Comp::Mesh(m);
        let s = eval_on(sel, comp, Dom::Point);
        let p = eval_on(pos, comp, Dom::Point);
        let o = eval_on(off, comp, Dom::Point);
        let m = g.mesh.as_mut().unwrap();
        m.vnor = None;
        for i in 0..m.nv() {
            if s[i].b() {
                m.pos[i] = add3(p[i].v(), o[i].v());
            }
        }
    }
    if let Some(c) = g.curves.as_ref() {
        let comp = Comp::Curves(c);
        let s = eval_on(sel, comp, Dom::Point);
        let p = eval_on(pos, comp, Dom::Point);
        let o = eval_on(off, comp, Dom::Point);
        let c = g.curves.as_mut().unwrap();
        for i in 0..c.np() {
            if s[i].b() {
                let new = add3(p[i].v(), o[i].v());
                let d = sub3(new, c.pos[i]);
                c.pos[i] = new;
                c.hl[i] = add3(c.hl[i], d);
                c.hr[i] = add3(c.hr[i], d);
            }
        }
        super::curveops::calculate_bezier_auto_handles(c);
    }
    if let Some(pt) = g.points.as_ref() {
        let comp = Comp::Points(pt);
        let s = eval_on(sel, comp, Dom::Point);
        let p = eval_on(pos, comp, Dom::Point);
        let o = eval_on(off, comp, Dom::Point);
        let pt = g.points.as_mut().unwrap();
        for i in 0..pt.pos.len() {
            if s[i].b() {
                pt.pos[i] = add3(p[i].v(), o[i].v());
            }
        }
    }
    if let Some(inst) = g.inst.as_ref() {
        // Instances component: the instance translations are the positions (Blender set_instances_position)
        let comp = Comp::Inst(inst);
        let s = eval_on(sel, comp, Dom::Instance);
        let p = eval_on(pos, comp, Dom::Instance);
        let o = eval_on(off, comp, Dom::Instance);
        let inst = g.inst.as_mut().unwrap();
        for i in 0..inst.n() {
            if s[i].b() {
                let new = add3(p[i].v(), o[i].v());
                inst.xf[i][3][0] = new[0];
                inst.xf[i][3][1] = new[1];
                inst.xf[i][3][2] = new[2];
            }
        }
    }
}

/// CurveToMesh
pub fn curve_to_mesh(c: &Curves, prof: Option<&Curves>, caps: bool) -> Mesh {
    let mut m = Mesh::new();
    let prof = match prof {
        Some(p) if p.np() > 0 => p,
        _ => {
            // wire mesh
            for k in 0..c.nc() {
                let e = eval_curve(c, k);
                let base = m.pos.len() as u32;
                m.pos.extend_from_slice(&e.pos);
                for (name, ty, vals) in super::nodes_curves::eval_point_attrs(c, k, &e) {
                    if name == "radius" || name == "tilt" || name.starts_with("__normal") || name.starts_with("__nurbs") {
                        continue;
                    }
                    let a = m.attrs.entry(name).or_insert_with(|| Attr { dom: Dom::Point, ty, data: vec![] });
                    a.data.resize(base as usize, Val::zero(ty));
                    a.data.extend(vals);
                }
                let n = e.pos.len() as u32;
                for i in 0..n.saturating_sub(1) {
                    m.edges.push([base + i, base + i + 1]);
                }
                if c.cyclic[k] && n > 2 {
                    m.edges.push([base + n - 1, base]);
                }
            }
            return m;
        }
    };
    let mut cap_edges: Vec<usize> = vec![];
    let mut cap_faces: Vec<usize> = vec![];
    // profile evaluated curves
    let profs: Vec<(EvalCurve, bool)> = (0..prof.nc()).map(|k| (eval_curve(prof, k), prof.cyclic[k])).collect();
    for k in 0..c.nc() {
        let e = eval_curve(c, k);
        let tan = eval_tangents(c, k, &e);
        let nrm = eval_normals_tilt(c, k, &e, &tan);
        let r: Vec<f32> = interp_to_eval(&e, &c.radius[c.range(k)], c.cyclic[k]);
        let n = e.pos.len();
        for (pe, pcyc) in &profs {
            let pn = pe.pos.len();
            let base = m.pos.len() as u32;
            for (name, ty, vals) in super::nodes_curves::eval_point_attrs(c, k, &e) {
                if name == "radius" || name == "tilt" || name.starts_with("__normal") || name.starts_with("__nurbs") {
                    continue;
                }
                let a = m.attrs.entry(name).or_insert_with(|| Attr { dom: Dom::Point, ty, data: vec![] });
                a.data.resize(base as usize, Val::zero(ty));
                for v in &vals {
                    for _ in 0..pn {
                        a.data.push(v.clone());
                    }
                }
            }
            for i in 0..n {
                let t = tan[i];
                let nn = nrm[i];
                let b = cross3(t, nn);
                let (cx, cy, cz) = (nn, b, t);
                let (cx, cy, cz) = (mul3(cx, r[i]), mul3(cy, r[i]), mul3(cz, r[i]));
                for p in &pe.pos {
                    let mut v = [0.0f32; 3];
                    for q in 0..3 {
                        v[q] = cx[q] * p[0] + cy[q] * p[1] + cz[q] * p[2] + e.pos[i][q];
                    }
                    m.pos.push(v);
                }
            }
            let nseg = if n <= 2 { n.saturating_sub(1) } else if c.cyclic[k] { n } else { n - 1 };
            let pseg = if *pcyc { pn } else { pn.saturating_sub(1) };
            let mut faces: Vec<Vec<u32>> = vec![];
            let mut fedges: Vec<Vec<u32>> = vec![];
            let main_start = m.edges.len();
            if pn == 1 {
                // profile with a single point: wire along the curve
                for i in 0..n.saturating_sub(1) {
                    m.edges.push([base + i as u32, base + i as u32 + 1]);
                }
                if c.cyclic[k] && nseg > 2 {
                    m.edges.push([base + n as u32 - 1, base]);
                }
                continue;
            }
            // Blender edge order: edges along the curve for each profile vertex, then the profile rings
            for ip in 0..pn {
                for ir in 0..nseg {
                    let nx = if ir == n - 1 { 0 } else { ir + 1 };
                    m.edges.push([base + (pn * ir + ip) as u32, base + (pn * nx + ip) as u32]);
                }
            }
            let ring_edges_start = m.edges.len();
            for ir in 0..n {
                for ip in 0..pseg {
                    let nx = if ip == pn - 1 { 0 } else { ip + 1 };
                    m.edges.push([base + (pn * ir + ip) as u32, base + (pn * ir + nx) as u32]);
                }
            }
            for ir in 0..nseg {
                let nx = if ir == n - 1 { 0 } else { ir + 1 };
                let (r0, r1) = (base + (ir * pn) as u32, base + (nx * pn) as u32);
                for ip in 0..pseg {
                    let j1 = if ip == pn - 1 { 0 } else { ip + 1 };
                    faces.push(vec![r0 + ip as u32, r0 + j1 as u32, r1 + j1 as u32, r1 + ip as u32]);
                    let ring_e = ring_edges_start + pseg * ir;
                    let next_ring_e = ring_edges_start + pseg * nx;
                    fedges.push(vec![
                        (ring_e + ip) as u32,
                        (main_start + nseg * j1 + ir) as u32,
                        (next_ring_e + ip) as u32,
                        (main_start + nseg * ip + ir) as u32,
                    ]);
                }
            }
            let nside = faces.len();
            if caps && !c.cyclic[k] && *pcyc && pn > 2 {
                let last = base + ((n - 1) * pn) as u32;
                faces.push((0..pn as u32).rev().map(|j| base + j).collect());
                faces.push((0..pn as u32).map(|j| last + j).collect());
                let last_ring_e = ring_edges_start + pseg * (n - 1);
                fedges.push((0..pseg).map(|i| (ring_edges_start + if i == pseg - 1 { pseg - 1 } else { pseg - i - 2 }) as u32).collect());
                fedges.push((0..pseg).map(|i| (last_ring_e + i) as u32).collect());
                for i in [0usize, n - 1] {
                    for j in 0..pseg {
                        cap_edges.push(ring_edges_start + i * pseg + j);
                    }
                }
                cap_faces.push(m.nf() + nside);
                cap_faces.push(m.nf() + nside + 1);
            }
            for fe in &fedges {
                m.ce.extend_from_slice(fe);
            }
            for f in &faces {
                m.cv.extend_from_slice(f);
                m.foff.push(m.cv.len() as u32);
            }
        }
    }
    let nvv = m.nv();
    for a in m.attrs.values_mut() {
        if a.dom == Dom::Point {
            let z = Val::zero(a.ty);
            a.data.resize(nvv, z);
        }
    }
    if !cap_faces.is_empty() {
        let mut se = vec![Val::B(false); m.ne()];
        for &e in &cap_edges {
            se[e] = Val::B(true);
        }
        m.attrs.insert("sharp_edge".into(), Attr { dom: Dom::Edge, ty: Ty::B, data: se });
        let mut sf = vec![Val::B(false); m.nf()];
        for &f in &cap_faces {
            sf[f] = Val::B(true);
        }
        m.attrs.insert("sharp_face".into(), Attr { dom: Dom::Face, ty: Ty::B, data: sf });
    }
    // wire edges for profiles / curves without faces are implied by faces; add ring/longitudinal edges for faceless output
    m
}
