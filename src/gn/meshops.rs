//! Mesh / curve / instance operations shared by the node implementations.
use super::attr::*;
use super::curveops::*;
use super::geom::*;
use super::math::*;
use super::val::*;
use crate::assets::cabnodes::GMesh;
use std::rc::Rc;

// ------------------------------------------------------------------ GMesh adapters
pub fn to_gmesh(m: &Mesh) -> GMesh {
    GMesh { v: m.pos.clone(), p: m.faces_vec(), tag: vec![0; m.nf()], e: m.edges.clone() }
}

/// GMesh -> Mesh (attributes are NOT carried; callers propagate them)
pub fn from_gmesh(g: &GMesh) -> Mesh {
    Mesh::from_faces(g.v.clone(), &g.p, &g.e)
}

// ------------------------------------------------------------------ attribute utilities
pub fn attr_zero_len(ty: Ty, n: usize) -> Vec<Val> {
    vec![Val::zero(ty); n]
}

/// Join attribute maps: `parts` = (attrs, domain sizes lookup, offset-less). Each part provides size per domain.
pub fn join_attrs(parts: &[(&Attrs, &dyn Fn(Dom) -> usize)], total: &dyn Fn(Dom) -> usize) -> Attrs {
    let mut out = Attrs::new();
    // names in order of first appearance
    for (attrs, _) in parts {
        for (name, a) in attrs.iter() {
            if out.contains_key(name) {
                continue;
            }
            // resolve type: highest of all parts with this name
            let mut ty = a.ty;
            for (other, _) in parts {
                if let Some(o) = other.get(name) {
                    ty = higher_ty(ty, o.ty);
                }
            }
            let mut data = Vec::with_capacity(total(a.dom));
            for (p_attrs, size) in parts {
                match p_attrs.get(name) {
                    Some(o) if o.dom == a.dom => data.extend(o.data.iter().map(|v| v.convert(ty))),
                    _ => data.extend(std::iter::repeat(Val::zero(ty)).take(size(a.dom))),
                }
            }
            out.insert(name.clone(), Attr { dom: a.dom, ty, data });
        }
    }
    out
}

fn rank(t: Ty) -> i32 {
    match t {
        Ty::B => 0,
        Ty::I => 1,
        Ty::F => 2,
        Ty::V => 3,
        Ty::Q => 4,
        Ty::C => 5,
        _ => 6,
    }
}
pub fn higher_ty(a: Ty, b: Ty) -> Ty {
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

pub fn join_meshes(ms: &[&Mesh]) -> Mesh {
    let mut o = Mesh::new();
    for m in ms {
        let bv = o.pos.len() as u32;
        let be = o.edges.len() as u32;
        o.pos.extend_from_slice(&m.pos);
        o.edges.extend(m.edges.iter().map(|e| [e[0] + bv, e[1] + bv]));
        let bc = o.cv.len() as u32;
        o.cv.extend(m.cv.iter().map(|v| v + bv));
        o.ce.extend(m.ce.iter().map(|e| e + be));
        o.foff.extend(m.foff[1..].iter().map(|x| x + bc));
    }
    let parts: Vec<(&Attrs, Box<dyn Fn(Dom) -> usize>)> = ms
        .iter()
        .map(|m| {
            let (a, b, c, d) = (m.nv(), m.ne(), m.nf(), m.nc());
            (&m.attrs, Box::new(move |dm: Dom| match dm {
                Dom::Point => a,
                Dom::Edge => b,
                Dom::Face => c,
                Dom::Corner => d,
                _ => 0,
            }) as Box<dyn Fn(Dom) -> usize>)
        })
        .collect();
    let refs: Vec<(&Attrs, &dyn Fn(Dom) -> usize)> = parts.iter().map(|(a, f)| (*a, &**f)).collect();
    let (a, b, c, d) = (o.nv(), o.ne(), o.nf(), o.nc());
    o.attrs = join_attrs(&refs, &|dm| match dm {
        Dom::Point => a,
        Dom::Edge => b,
        Dom::Face => c,
        Dom::Corner => d,
        _ => 0,
    });
    // merge material slots, remapping material_index
    let mut mats: Vec<String> = vec![];
    for m in ms {
        for n in &m.mats {
            if !mats.contains(n) {
                mats.push(n.clone());
            }
        }
    }
    if !mats.is_empty() {
        let mut mi: Vec<Val> = vec![];
        for m in ms {
            for f in 0..m.nf() {
                let old = m.attrs.get("material_index").map(|a| a.data[f].i()).unwrap_or(0);
                let new = m.mats.get(old.max(0) as usize).and_then(|n| mats.iter().position(|x| x == n)).map(|x| x as i32).unwrap_or(old);
                mi.push(Val::I(new));
            }
        }
        o.attrs.insert("material_index".to_string(), Attr { dom: Dom::Face, ty: Ty::I, data: mi });
        o.mats = mats;
    }
    o
}

pub fn join_curves(cs: &[&Curves]) -> Curves {
    let mut o = Curves::new();
    o.has_radius = cs.iter().any(|c| c.has_radius);
    for c in cs {
        let base = o.pos.len() as u32;
        o.kind.extend_from_slice(&c.kind);
        o.cyclic.extend_from_slice(&c.cyclic);
        o.res.extend_from_slice(&c.res);
        o.off.extend(c.off[1..].iter().map(|x| x + base));
        o.pos.extend_from_slice(&c.pos);
        o.hl.extend_from_slice(&c.hl);
        o.hr.extend_from_slice(&c.hr);
        o.htl.extend_from_slice(&c.htl);
        o.htr.extend_from_slice(&c.htr);
        o.radius.extend_from_slice(&c.radius);
        o.tilt.extend_from_slice(&c.tilt);
    }
    let parts: Vec<(&Attrs, Box<dyn Fn(Dom) -> usize>)> = cs
        .iter()
        .map(|c| {
            let (a, b) = (c.np(), c.nc());
            (&c.attrs, Box::new(move |dm: Dom| match dm {
                Dom::Point => a,
                Dom::Curve => b,
                _ => 0,
            }) as Box<dyn Fn(Dom) -> usize>)
        })
        .collect();
    let refs: Vec<(&Attrs, &dyn Fn(Dom) -> usize)> = parts.iter().map(|(a, f)| (*a, &**f)).collect();
    let (a, b) = (o.np(), o.nc());
    o.attrs = join_attrs(&refs, &|dm| match dm {
        Dom::Point => a,
        Dom::Curve => b,
        _ => 0,
    });
    o
}

pub fn join_instances(is: &[&Instances]) -> Instances {
    let mut o = Instances::default();
    for i in is {
        let base = o.refs.len() as u32;
        o.refs.extend(i.refs.iter().cloned());
        o.idx.extend(i.idx.iter().map(|x| x + base));
        o.xf.extend_from_slice(&i.xf);
    }
    let parts: Vec<(&Attrs, Box<dyn Fn(Dom) -> usize>)> = is
        .iter()
        .map(|i| {
            let n = i.n();
            (&i.attrs, Box::new(move |dm: Dom| if dm == Dom::Instance { n } else { 0 }) as Box<dyn Fn(Dom) -> usize>)
        })
        .collect();
    let refs: Vec<(&Attrs, &dyn Fn(Dom) -> usize)> = parts.iter().map(|(a, f)| (*a, &**f)).collect();
    let n = o.n();
    o.attrs = join_attrs(&refs, &|dm| if dm == Dom::Instance { n } else { 0 });
    o
}

pub fn join_points(ps: &[&Points]) -> Points {
    let mut o = Points::default();
    for p in ps {
        o.pos.extend_from_slice(&p.pos);
    }
    let parts: Vec<(&Attrs, Box<dyn Fn(Dom) -> usize>)> = ps
        .iter()
        .map(|p| {
            let n = p.pos.len();
            (&p.attrs, Box::new(move |dm: Dom| if dm == Dom::Point { n } else { 0 }) as Box<dyn Fn(Dom) -> usize>)
        })
        .collect();
    let refs: Vec<(&Attrs, &dyn Fn(Dom) -> usize)> = parts.iter().map(|(a, f)| (*a, &**f)).collect();
    let n = o.pos.len();
    o.attrs = join_attrs(&refs, &|dm| if dm == Dom::Point { n } else { 0 });
    o
}

/// JoinGeometry: `list` in the order the node processes them
pub fn join_geometries(list: &[Rc<Geometry>]) -> Geometry {
    let ms: Vec<&Mesh> = list.iter().filter_map(|g| g.mesh.as_ref()).collect();
    let cs: Vec<&Curves> = list.iter().filter_map(|g| g.curves.as_ref()).collect();
    let is: Vec<&Instances> = list.iter().filter_map(|g| g.inst.as_ref()).collect();
    let ps: Vec<&Points> = list.iter().filter_map(|g| g.points.as_ref()).collect();
    let mut o = Geometry::default();
    if !ms.is_empty() {
        o.mesh = Some(if ms.len() == 1 { ms[0].clone() } else { join_meshes(&ms) });
    }
    if !cs.is_empty() {
        o.curves = Some(if cs.len() == 1 { cs[0].clone() } else { join_curves(&cs) });
    }
    if !is.is_empty() {
        o.inst = Some(if is.len() == 1 { is[0].clone() } else { join_instances(&is) });
    }
    if !ps.is_empty() {
        o.points = Some(if ps.len() == 1 { ps[0].clone() } else { join_points(&ps) });
    }
    o
}

// ------------------------------------------------------------------ transform
/// matrix from (translation, quaternion rotation, scale) like float4x4::from_loc_rot_scale
pub fn matrix_from_trs(loc: V3, q: [f32; 4], scale: V3) -> M4 {
    let m3 = crate::bl32::quat_to_mat3(q);
    let mut m = M4_ID;
    for c in 0..3 {
        for r in 0..3 {
            m[c][r] = m3[c][r] * scale[c];
        }
    }
    m[3] = [loc[0], loc[1], loc[2], 1.0];
    m
}

/// transform a point the way mesh transform does (x*m0 + y*m1 + z*m2 + loc, no fma)
#[inline]
pub fn xf_point(m: &M4, p: V3) -> V3 {
    let (x, y, z) = (p[0], p[1], p[2]);
    [x * m[0][0] + y * m[1][0] + m[2][0] * z + m[3][0], x * m[0][1] + y * m[1][1] + m[2][1] * z + m[3][1], x * m[0][2] + y * m[1][2] + m[2][2] * z + m[3][2]]
}

pub fn transform_geometry(g: &Geometry, m: &M4, translate_only: Option<V3>) -> Geometry {
    let mut o = g.clone();
    let f = |p: &mut V3| {
        *p = match translate_only {
            Some(t) => [p[0] + t[0], p[1] + t[1], p[2] + t[2]],
            None => xf_point(m, *p),
        }
    };
    if let Some(me) = o.mesh.as_mut() {
        me.vnor = None;
        me.pos.iter_mut().for_each(f);
    }
    if let Some(c) = o.curves.as_mut() {
        c.pos.iter_mut().for_each(f);
        c.hl.iter_mut().for_each(f);
        c.hr.iter_mut().for_each(f);
    }
    if let Some(p) = o.points.as_mut() {
        p.pos.iter_mut().for_each(f);
    }
    if let Some(i) = o.inst.as_mut() {
        for x in i.xf.iter_mut() {
            *x = m4_mul(m, x);
        }
        if let Some(t) = translate_only {
            let _ = t;
        }
    }
    o
}

// ------------------------------------------------------------------ bounding box
pub fn geometry_bounds(g: &Geometry, include_inst: bool) -> Option<(V3, V3)> {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut any = false;
    let mut add = |p: &V3| {
        any = true;
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    if let Some(m) = &g.mesh {
        m.pos.iter().for_each(&mut add);
    }
    if let Some(c) = &g.curves {
        // evaluated positions (bezier handles do not extend bounds beyond the curve)
        for k in 0..c.nc() {
            let e = eval_curve(c, k);
            e.pos.iter().for_each(&mut add);
        }
    }
    if let Some(p) = &g.points {
        p.pos.iter().for_each(&mut add);
    }
    if include_inst {
        if let Some(i) = &g.inst {
            for (k, x) in i.xf.iter().enumerate() {
                if let Some(r) = i.refs.get(i.idx[k] as usize) {
                    if let Some((l, h)) = geometry_bounds(r, true) {
                        for ix in 0..8 {
                            let c = [if ix & 1 == 0 { l[0] } else { h[0] }, if ix & 2 == 0 { l[1] } else { h[1] }, if ix & 4 == 0 { l[2] } else { h[2] }];
                            let p = xf_point(x, c);
                            add(&p);
                        }
                    }
                }
            }
        }
    }
    if any {
        Some((lo, hi))
    } else {
        None
    }
}

/// Blender BoundBox node output: cube mesh of the bounds (lo, hi)
pub fn bbox_mesh(lo: V3, hi: V3) -> Mesh {
    let size = sub3(hi, lo);
    let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let _ = center;
    let g = crate::assets::cabnodes::gcube(size, [2, 2, 2]);
    let mut m = from_gmesh(&g);
    super::prim::set_sharp_all(&mut m);
    // bounding box node: translate the unit-size cube by the center (matrix path)
    let c = [lo[0] + size[0] * 0.5, lo[1] + size[1] * 0.5, lo[2] + size[2] * 0.5];
    for p in m.pos.iter_mut() {
        *p = [p[0] + c[0], p[1] + c[1], p[2] + c[2]];
    }
    m
}

// ------------------------------------------------------------------ realize instances
pub fn realize_instances(g: &Geometry, depth: i32) -> Geometry {
    let mut parts: Vec<Rc<Geometry>> = vec![];
    let mut base = g.clone();
    base.inst = None;
    // Blender gathers realize tasks in component order (mesh, point cloud, instances, volume, curves): the geometry's own
    // curves therefore come after the curves of its instances
    let base_curves = if g.inst.is_some() { base.curves.take() } else { None };
    parts.push(Rc::new(base));
    if let Some(inst) = &g.inst {
        let mut ctx_attrs_names: Vec<String> = vec![];
        for (k, x) in inst.xf.iter().enumerate() {
            let Some(r) = inst.refs.get(inst.idx[k] as usize) else { continue };
            let sub = if depth != 0 && r.inst.is_some() { realize_instances(r, depth - 1) } else { (**r).clone() };
            let mut t = transform_geometry(&sub, x, None);
            // propagate instance attributes to point/face/... domains of realized geometry
            for (name, a) in inst.attrs.iter() {
                if !ctx_attrs_names.contains(name) {
                    ctx_attrs_names.push(name.clone());
                }
                let v = a.data[k].clone();
                if let Some(m) = t.mesh.as_mut() {
                    if !m.attrs.contains_key(name) {
                        m.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: vec![v.clone(); m.nv()] });
                    }
                }
                if let Some(c) = t.curves.as_mut() {
                    if !c.attrs.contains_key(name) {
                        c.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: vec![v.clone(); c.np()] });
                    }
                }
            }
            parts.push(Rc::new(t));
        }
    }
    if let Some(c) = base_curves {
        parts.push(Rc::new(Geometry::from_curves(c)));
    }
    let mut o = join_geometries(&parts);
    o.inst = None;
    o
}

// ------------------------------------------------------------------ face / vertex selection helpers
/// mask of domain elements -> mask of mesh vertices (used by delete / separate)
pub fn tri_count(m: &Mesh) -> usize {
    (0..m.nf()).map(|f| m.face_range(f).len().saturating_sub(2)).sum()
}

/// Build a new mesh keeping the selected vertices / edges / faces (delete: keep = !selected).
/// `dom`: domain of `keep`. Order is preserved. Edges/faces that reference removed verts are dropped.
/// `keep_all_verts`: Delete Geometry mode EDGE_FACE (vertices are never removed)
pub fn mesh_keep(m: &Mesh, dom: Dom, keep: &[bool], keep_all_verts: bool) -> Mesh {
    let (vkeep, ekeep, fkeep): (Vec<bool>, Vec<bool>, Vec<bool>) = match dom {
        Dom::Point => {
            let vk = keep.to_vec();
            let ek: Vec<bool> = m.edges.iter().map(|e| vk[e[0] as usize] && vk[e[1] as usize]).collect();
            let fk: Vec<bool> = (0..m.nf()).map(|f| m.face(f).iter().all(|&v| vk[v as usize])).collect();
            (vk, ek, fk)
        }
        Dom::Edge => {
            // delete edges: faces using a deleted edge are removed; vertices remain
            let ek = keep.to_vec();
            let fk: Vec<bool> = (0..m.nf()).map(|f| m.face_range(f).all(|c| ek[m.ce[c] as usize])).collect();
            let mut vk = vec![keep_all_verts; m.nv()];
            if !keep_all_verts {
                for (i, e) in m.edges.iter().enumerate() {
                    if ek[i] {
                        vk[e[0] as usize] = true;
                        vk[e[1] as usize] = true;
                    }
                }
            }
            (vk, ek, fk)
        }
        _ => {
            let fk = keep.to_vec();
            // vertices/edges stay if used by kept faces... Blender "delete faces" only removes faces and
            // the edges/verts only used by deleted faces
            let mut ek = vec![false; m.ne()];
            let mut vk = vec![false; m.nv()];
            for f in 0..m.nf() {
                if fk[f] {
                    for c in m.face_range(f) {
                        ek[m.ce[c] as usize] = true;
                        vk[m.cv[c] as usize] = true;
                    }
                }
            }
            if keep_all_verts {
                vk = vec![true; m.nv()];
            }
            // loose edges/vertices that were not part of any face are kept
            let mut in_face_e = vec![false; m.ne()];
            let mut in_face_v = vec![false; m.nv()];
            for c in 0..m.nc() {
                in_face_e[m.ce[c] as usize] = true;
                in_face_v[m.cv[c] as usize] = true;
            }
            for e in 0..m.ne() {
                if !in_face_e[e] {
                    ek[e] = true;
                    vk[m.edges[e][0] as usize] = true;
                    vk[m.edges[e][1] as usize] = true;
                }
            }
            for v in 0..m.nv() {
                if !in_face_v[v] {
                    vk[v] = true;
                }
            }
            (vk, ek, fk)
        }
    };
    // vertices that are kept but belong to removed edges/faces only: for Point domain they're already removed.
    let mut vmap = vec![u32::MAX; m.nv()];
    let mut o = Mesh::new();
    let mut vsrc = vec![];
    for v in 0..m.nv() {
        if vkeep[v] {
            vmap[v] = o.pos.len() as u32;
            o.pos.push(m.pos[v]);
            vsrc.push(v);
        }
    }
    let mut esrc = vec![];
    let mut emap = vec![u32::MAX; m.ne()];
    for (i, e) in m.edges.iter().enumerate() {
        if ekeep[i] && vmap[e[0] as usize] != u32::MAX && vmap[e[1] as usize] != u32::MAX {
            emap[i] = o.edges.len() as u32;
            o.edges.push([vmap[e[0] as usize], vmap[e[1] as usize]]);
            esrc.push(i);
        }
    }
    let mut fsrc = vec![];
    let mut csrc = vec![];
    for f in 0..m.nf() {
        if fkeep[f] {
            for c in m.face_range(f) {
                o.cv.push(vmap[m.cv[c] as usize]);
                o.ce.push(emap[m.ce[c] as usize]);
                csrc.push(c);
            }
            o.foff.push(o.cv.len() as u32);
            fsrc.push(f);
        }
    }
    for (name, a) in m.attrs.iter() {
        let src: &Vec<usize> = match a.dom {
            Dom::Point => &vsrc,
            Dom::Edge => &esrc,
            Dom::Face => &fsrc,
            _ => &csrc,
        };
        o.attrs.insert(name.clone(), Attr { dom: a.dom, ty: a.ty, data: src.iter().map(|&i| a.data[i].clone()).collect() });
    }
    o
}

/// Copy a subset of curves/points
pub fn curves_keep(c: &Curves, dom: Dom, keep: &[bool]) -> Curves {
    let mut o = Curves::new();
    o.has_radius = c.has_radius;
    let mut psrc = vec![];
    let mut csrc = vec![];
    match dom {
        Dom::Curve => {
            for k in 0..c.nc() {
                if keep[k] {
                    csrc.push(k);
                    for p in c.range(k) {
                        psrc.push(p);
                    }
                }
            }
            for &k in &csrc {
                o.kind.push(c.kind[k]);
                o.cyclic.push(c.cyclic[k]);
                o.res.push(c.res[k]);
                o.off.push(o.off.last().unwrap() + c.range(k).len() as u32);
            }
        }
        _ => {
            // points: each curve keeps its kept points; curves with no points are removed
            for k in 0..c.nc() {
                let kept: Vec<usize> = c.range(k).filter(|&p| keep[p]).collect();
                if kept.is_empty() {
                    continue;
                }
                csrc.push(k);
                o.kind.push(c.kind[k]);
                o.cyclic.push(c.cyclic[k]);
                o.res.push(c.res[k]);
                o.off.push(o.off.last().unwrap() + kept.len() as u32);
                psrc.extend(kept);
            }
        }
    }
    for &p in &psrc {
        o.pos.push(c.pos[p]);
        o.hl.push(c.hl[p]);
        o.hr.push(c.hr[p]);
        o.htl.push(c.htl[p]);
        o.htr.push(c.htr[p]);
        o.radius.push(c.radius[p]);
        o.tilt.push(c.tilt[p]);
    }
    for (name, a) in c.attrs.iter() {
        let src = if a.dom == Dom::Point { &psrc } else { &csrc };
        o.attrs.insert(name.clone(), Attr { dom: a.dom, ty: a.ty, data: src.iter().map(|&i| a.data[i].clone()).collect() });
    }
    o
}
