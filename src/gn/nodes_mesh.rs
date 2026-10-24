//! Mesh-modifying nodes: extrude, merge by distance, subdivide, triangulate, split edges, scale elements, ...
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
use crate::dhash::HashMap;
use std::rc::Rc;

/// extend every attribute of a domain by copying source indices
pub fn extend_attrs(m: &mut Mesh, dom: Dom, src: &[usize]) {
    for a in m.attrs.values_mut() {
        if a.dom == dom {
            for &s in src {
                let v = a.data.get(s).cloned().unwrap_or(Val::zero(a.ty));
                a.data.push(v);
            }
        }
    }
}

fn hidden_bool(m: &mut Mesh, name: &str, dom: Dom, mask: Vec<bool>) {
    m.attrs.insert(name.to_string(), Attr { dom, ty: Ty::B, data: mask.into_iter().map(Val::B).collect() });
}

fn avg_offsets(n: usize, contribs: &[(usize, V3)]) -> Vec<V3> {
    let mut acc = vec![[0.0f32; 3]; n];
    let mut cnt = vec![0u32; n];
    for &(v, o) in contribs {
        acc[v] = add3(acc[v], o);
        cnt[v] += 1;
    }
    for i in 0..n {
        if cnt[i] > 1 {
            acc[i] = mul3(acc[i], 1.0 / cnt[i] as f32);
        }
    }
    acc
}

/// ExtrudeMesh. `top`/`side` masks are returned as (Dom, mask) pairs on the result mesh.
pub fn extrude(m: &Mesh, mode: &str, sel: &[bool], off: &[V3], individual: bool) -> (Mesh, Dom, Vec<bool>, Dom, Vec<bool>) {
    match mode {
        "VERTICES" => extrude_vertices(m, sel, off),
        "EDGES" => extrude_edges(m, sel, off),
        _ => {
            if individual {
                extrude_faces_individual(m, sel, off)
            } else {
                extrude_faces_region(m, sel, off)
            }
        }
    }
}

fn extrude_vertices(m: &Mesh, sel: &[bool], off: &[V3]) -> (Mesh, Dom, Vec<bool>, Dom, Vec<bool>) {
    let mut o = m.clone();
    let nv0 = m.nv();
    let ne0 = m.ne();
    let mut srcv = vec![];
    for v in 0..nv0 {
        if sel[v] {
            o.pos.push(add3(m.pos[v], off[v]));
            srcv.push(v);
        }
    }
    for (k, &v) in srcv.iter().enumerate() {
        o.edges.push([v as u32, (nv0 + k) as u32]);
    }
    extend_attrs(&mut o, Dom::Point, &srcv);
    extend_attrs(&mut o, Dom::Edge, &srcv.iter().map(|_| usize::MAX).collect::<Vec<_>>());
    o.vnor = None;
    let mut top = vec![false; o.nv()];
    for k in 0..srcv.len() {
        top[nv0 + k] = true;
    }
    let mut side = vec![false; o.ne()];
    for k in 0..srcv.len() {
        side[ne0 + k] = true;
    }
    (o, Dom::Point, top, Dom::Edge, side)
}

fn extrude_edges(m: &Mesh, sel: &[bool], off: &[V3]) -> (Mesh, Dom, Vec<bool>, Dom, Vec<bool>) {
    let nv0 = m.nv();
    let ne0 = m.ne();
    let nf0 = m.nf();
    let mut vsel = vec![false; nv0];
    let mut contribs = vec![];
    for (e, ed) in m.edges.iter().enumerate() {
        if sel[e] {
            for &v in ed {
                vsel[v as usize] = true;
                contribs.push((v as usize, off[e]));
            }
        }
    }
    let voff = avg_offsets(nv0, &contribs);
    let mut o = m.clone();
    let mut new_of = vec![u32::MAX; nv0];
    let mut srcv = vec![];
    for v in 0..nv0 {
        if vsel[v] {
            new_of[v] = o.pos.len() as u32;
            o.pos.push(add3(m.pos[v], voff[v]));
            srcv.push(v);
        }
    }
    let sel_edges: Vec<usize> = (0..ne0).filter(|&e| sel[e]).collect();
    // new (duplicated) edges
    for &e in &sel_edges {
        let ed = m.edges[e];
        o.edges.push([new_of[ed[0] as usize], new_of[ed[1] as usize]]);
    }
    // connecting edges
    let conn_start = o.edges.len();
    for &v in &srcv {
        o.edges.push([v as u32, new_of[v]]);
    }
    let cn_idx = |v: u32| -> u32 { conn_start as u32 + srcv.iter().position(|&x| x == v as usize).unwrap() as u32 };
    // side faces: one quad per selected edge: (v1, v2, new v2, new v1)
    let mut nfaces = vec![];
    for &e in &sel_edges {
        let ed = m.edges[e];
        nfaces.push(vec![ed[0], new_of[ed[0] as usize], new_of[ed[1] as usize], ed[1]]);
    }
    for f in &nfaces {
        o.cv.extend_from_slice(f);
        o.foff.push(o.cv.len() as u32);
    }
    let _ = cn_idx;
    // attributes
    extend_attrs(&mut o, Dom::Point, &srcv);
    extend_attrs(&mut o, Dom::Edge, &sel_edges);
    extend_attrs(&mut o, Dom::Edge, &srcv.iter().map(|_| usize::MAX).collect::<Vec<_>>());
    o.ce = vec![0; o.cv.len()];
    // rebuild corner edges (matching by vertex pair); new face edges exist already
    o.vnor = None;
    let (ne_before, nf_total) = (o.ne(), o.nf());
    let mut fixed = o.clone();
    fixed.ce = vec![0; fixed.cv.len()];
    // keep first nc entries of ce for old faces
    for c in 0..m.nc() {
        fixed.ce[c] = m.ce[c];
    }
    // generic rebuild for all (new edges are unique pairs)
    let mut map: HashMap<(u32, u32), u32> = HashMap::default();
    for (i, e) in fixed.edges.iter().enumerate() {
        map.entry((e[0].min(e[1]), e[0].max(e[1]))).or_insert(i as u32);
    }
    for f in 0..nf_total {
        let r = fixed.face_range(f);
        let n = r.len();
        for c in 0..n {
            let a = fixed.cv[r.start + c];
            let b = fixed.cv[r.start + (c + 1) % n];
            fixed.ce[r.start + c] = *map.get(&(a.min(b), a.max(b))).unwrap_or(&0);
        }
    }
    let mut o = fixed;
    // face / corner attributes of the side faces: zero
    for a in o.attrs.values_mut() {
        match a.dom {
            Dom::Face => {
                let z = Val::zero(a.ty);
                a.data.resize(nf_total, z);
            }
            Dom::Corner => {
                let z = Val::zero(a.ty);
                a.data.resize(o.cv.len(), z);
            }
            _ => {}
        }
    }
    let _ = (nf0, ne_before);
    let mut top = vec![false; o.ne()];
    for k in 0..sel_edges.len() {
        top[ne0 + k] = true;
    }
    let mut side = vec![false; o.nf()];
    for k in 0..sel_edges.len() {
        side[nf0 + k] = true;
    }
    (o, Dom::Edge, top, Dom::Face, side)
}

fn extrude_faces_region(m: &Mesh, sel: &[bool], off: &[V3]) -> (Mesh, Dom, Vec<bool>, Dom, Vec<bool>) {
    let nv0 = m.nv();
    let ne0 = m.ne();
    let nf0 = m.nf();
    // selected-face counts per edge
    let mut scount = vec![0u32; ne0];
    for f in 0..nf0 {
        if sel[f] {
            for c in m.face_range(f) {
                scount[m.ce[c] as usize] += 1;
            }
        }
    }
    let boundary: Vec<usize> = (0..ne0).filter(|&e| scount[e] == 1).collect();
    let mut dup = vec![false; nv0];
    for &e in &boundary {
        dup[m.edges[e][0] as usize] = true;
        dup[m.edges[e][1] as usize] = true;
    }
    // vertex offsets: mean of the offsets of selected adjacent faces
    let mut contribs = vec![];
    let mut in_region = vec![false; nv0];
    for f in 0..nf0 {
        if sel[f] {
            for &v in m.face(f) {
                contribs.push((v as usize, off[f]));
                in_region[v as usize] = true;
            }
        }
    }
    let voff = avg_offsets(nv0, &contribs);
    let mut o = m.clone();
    o.vnor = None;
    let mut new_of = vec![u32::MAX; nv0];
    let mut srcv = vec![];
    for v in 0..nv0 {
        if dup[v] {
            new_of[v] = o.pos.len() as u32;
            o.pos.push(add3(m.pos[v], voff[v]));
            srcv.push(v);
        }
    }
    // interior region vertices move in place
    for v in 0..nv0 {
        if in_region[v] && !dup[v] {
            o.pos[v] = add3(o.pos[v], voff[v]);
        }
    }
    let map_v = |v: u32| -> u32 { if new_of[v as usize] != u32::MAX { new_of[v as usize] } else { v } };
    // edges: interior edges modified in place, boundary edges duplicated
    let mut edge_new = vec![u32::MAX; ne0];
    for e in 0..ne0 {
        if scount[e] >= 2 {
            let ed = m.edges[e];
            o.edges[e] = [map_v(ed[0]), map_v(ed[1])];
        }
    }
    let mut new_edges_src = vec![];
    for &e in &boundary {
        let ed = m.edges[e];
        edge_new[e] = o.edges.len() as u32;
        o.edges.push([map_v(ed[0]), map_v(ed[1])]);
        new_edges_src.push(e);
    }
    let conn_start = o.edges.len();
    for &v in &srcv {
        o.edges.push([v as u32, new_of[v]]);
    }
    let conn_of = |v: u32| -> u32 { conn_start as u32 + srcv.iter().position(|&x| x == v as usize).unwrap() as u32 };
    // top faces: replace vertices / edges in place
    let mut ce_old = o.ce.clone();
    for f in 0..nf0 {
        if sel[f] {
            for c in m.face_range(f) {
                o.cv[c] = map_v(m.cv[c]);
                let e = m.ce[c] as usize;
                ce_old[c] = if scount[e] == 1 { edge_new[e] } else { e as u32 };
            }
        }
    }
    o.ce = ce_old;
    // side faces (one per boundary edge), orientation from the selected face's loop direction
    let mut side_src_face = vec![];
    for &e in &boundary {
        let ed = m.edges[e];
        let mut forward = true;
        let mut face_of = 0usize;
        'o: for f in 0..nf0 {
            if !sel[f] {
                continue;
            }
            let r = m.face_range(f);
            let n = r.len();
            for c in 0..n {
                if m.ce[r.start + c] as usize == e {
                    let (a, b) = (m.cv[r.start + c], m.cv[r.start + (c + 1) % n]);
                    forward = a == ed[0] && b == ed[1];
                    face_of = f;
                    break 'o;
                }
            }
        }
        side_src_face.push(face_of);
        let (e0, e1) = (ed[0], ed[1]);
        let (n0, n1) = (new_of[e0 as usize], new_of[e1 as usize]);
        let (face, ces): (Vec<u32>, Vec<u32>) = if forward {
            (vec![n0, e0, e1, n1], vec![conn_of(e0), e as u32, conn_of(e1), edge_new[e]])
        } else {
            (vec![n0, n1, e1, e0], vec![edge_new[e], conn_of(e1), e as u32, conn_of(e0)])
        };
        o.cv.extend_from_slice(&face);
        o.ce.extend_from_slice(&ces);
        o.foff.push(o.cv.len() as u32);
    }
    // attribute propagation
    extend_attrs(&mut o, Dom::Point, &srcv);
    extend_attrs(&mut o, Dom::Edge, &new_edges_src);
    extend_attrs(&mut o, Dom::Edge, &srcv.iter().map(|_| usize::MAX).collect::<Vec<_>>());
    extend_attrs(&mut o, Dom::Face, &side_src_face);
    let nc = o.cv.len();
    for a in o.attrs.values_mut() {
        if a.dom == Dom::Corner {
            let z = Val::zero(a.ty);
            a.data.resize(nc, z);
        }
    }
    let mut top = vec![false; o.nf()];
    for f in 0..nf0 {
        top[f] = sel[f];
    }
    let mut side = vec![false; o.nf()];
    for k in 0..boundary.len() {
        side[nf0 + k] = true;
    }
    (o, Dom::Face, top, Dom::Face, side)
}

fn extrude_faces_individual(m: &Mesh, sel: &[bool], off: &[V3]) -> (Mesh, Dom, Vec<bool>, Dom, Vec<bool>) {
    let nf0 = m.nf();
    let mut o = m.clone();
    o.vnor = None;
    let mut srcv = vec![];
    let mut new_edges_src = vec![];
    let mut side_src = vec![];
    // per selected face: duplicate its vertices
    let mut new_corner_vert: HashMap<usize, Vec<u32>> = HashMap::default();
    for f in 0..nf0 {
        if !sel[f] {
            continue;
        }
        let mut vs = vec![];
        for &v in m.face(f) {
            vs.push(o.pos.len() as u32);
            o.pos.push(add3(m.pos[v as usize], off[f]));
            srcv.push(v as usize);
        }
        new_corner_vert.insert(f, vs);
    }
    // edges: new top edges, then connecting edges, per face in order
    let mut face_new_edges: HashMap<usize, Vec<u32>> = HashMap::default();
    let mut face_conn: HashMap<usize, Vec<u32>> = HashMap::default();
    for f in 0..nf0 {
        if !sel[f] {
            continue;
        }
        let vs = &new_corner_vert[&f];
        let n = vs.len();
        let mut ne = vec![];
        for c in 0..n {
            ne.push(o.edges.len() as u32);
            o.edges.push([vs[c], vs[(c + 1) % n]]);
            new_edges_src.push(m.ce[m.face_range(f).start + c] as usize);
        }
        face_new_edges.insert(f, ne);
    }
    for f in 0..nf0 {
        if !sel[f] {
            continue;
        }
        let vs = &new_corner_vert[&f];
        let r = m.face_range(f);
        let mut cn = vec![];
        for c in 0..vs.len() {
            cn.push(o.edges.len() as u32);
            o.edges.push([m.cv[r.start + c], vs[c]]);
        }
        face_conn.insert(f, cn);
    }
    let mut side_faces = 0;
    for f in 0..nf0 {
        if !sel[f] {
            continue;
        }
        let r = m.face_range(f);
        let n = r.len();
        let vs = new_corner_vert[&f].clone();
        for c in 0..n {
            o.cv[r.start + c] = vs[c];
            o.ce[r.start + c] = face_new_edges[&f][c];
        }
    }
    for f in 0..nf0 {
        if !sel[f] {
            continue;
        }
        let r = m.face_range(f);
        let n = r.len();
        let vs = new_corner_vert[&f].clone();
        for c in 0..n {
            let c1 = (c + 1) % n;
            let face = vec![vs[c], m.cv[r.start + c], m.cv[r.start + c1], vs[c1]];
            let ces = vec![face_conn[&f][c], m.ce[r.start + c], face_conn[&f][c1], face_new_edges[&f][c]];
            o.cv.extend_from_slice(&face);
            o.ce.extend_from_slice(&ces);
            o.foff.push(o.cv.len() as u32);
            side_src.push(f);
            side_faces += 1;
        }
    }
    extend_attrs(&mut o, Dom::Point, &srcv);
    extend_attrs(&mut o, Dom::Edge, &new_edges_src);
    extend_attrs(&mut o, Dom::Edge, &srcv.iter().map(|_| usize::MAX).collect::<Vec<_>>());
    extend_attrs(&mut o, Dom::Face, &side_src);
    let nc = o.cv.len();
    for a in o.attrs.values_mut() {
        if a.dom == Dom::Corner {
            let z = Val::zero(a.ty);
            a.data.resize(nc, z);
        }
    }
    let mut top = vec![false; o.nf()];
    for f in 0..nf0 {
        top[f] = sel[f];
    }
    let mut side = vec![false; o.nf()];
    for k in 0..side_faces {
        side[nf0 + k] = true;
    }
    (o, Dom::Face, top, Dom::Face, side)
}

pub fn mesh_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "GeometryNodeExtrudeMesh" => {
            let g = ev.geo(ni, "Mesh")?;
            let mode = node.ps("mode").to_string();
            let dom = match mode.as_str() {
                "VERTICES" => Dom::Point,
                "EDGES" => Dom::Edge,
                _ => Dom::Face,
            };
            let indiv = ev.cb(ni, "Individual").unwrap_or(false);
            let mut o = (*g).clone();
            let mut top_name = String::new();
            let mut side_name = String::new();
            if let Some(m) = &g.mesh {
                let comp = Comp::Mesh(m);
                let sel = selection(ev, ni, comp, dom)?;
                let offf = ev.field(ni, "Offset")?;
                let scl = ev.field(ni, "Offset Scale")?;
                let prod = map2(&offf, &scl, |a, b| {
                    let (v, s) = (a.v(), b.f());
                    Val::V([v[0] * s, v[1] * s, v[2] * s])
                });
                let off: Vec<V3> = eval_on(&prod, comp, dom).iter().map(|v| v.v()).collect();
                let (mut nm, td, top, sd, side) = extrude(m, &mode, &sel, &off, indiv);
                top_name = anon_name("ext_top");
                side_name = anon_name("ext_side");
                hidden_bool(&mut nm, &top_name, td, top);
                hidden_bool(&mut nm, &side_name, sd, side);
                o.mesh = Some(nm);
            }
            outs(
                node,
                vec![
                    ("Mesh", rcgeo(o)),
                    ("Top", fld(Rc::new(FNode::Named(top_name, Ty::B)))),
                    ("Side", fld(Rc::new(FNode::Named(side_name, Ty::B)))),
                ],
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

// ====================================================================================== more mesh nodes

fn ty_is_multi(node: &Node) -> bool {
    node.in_idx("Mesh 2").is_some()
}

fn dsq(a: V3, b: V3) -> f32 {
    let d = sub3(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Triangulate selected faces with >= min_verts corners. Returns the new mesh (first triangle in place, others appended).
pub fn triangulate(m: &Mesh, sel: &[bool], min_verts: usize, quad_method: &str) -> Mesh {
    let mut faces: Vec<Vec<u32>> = vec![];
    let mut src: Vec<usize> = vec![];
    let mut extra: Vec<(Vec<u32>, usize)> = vec![];
    for f in 0..m.nf() {
        let fv = m.face(f).to_vec();
        if !sel[f] || fv.len() < min_verts.max(4) {
            faces.push(fv);
            src.push(f);
        } else if fv.len() == 4 {
            let (a, b, c, d) = (fv[0], fv[1], fv[2], fv[3]);
            let p = |i: u32| m.pos[i as usize];
            let d1 = dsq(p(a), p(c));
            let d2 = dsq(p(b), p(d));
            let split_ac = match quad_method {
                "LONGEST_DIAGONAL" => d1 > d2,
                "FIXED" => true,
                "FIXED_ALTERNATE" => false,
                "BEAUTY" => crate::assets::polyfill::quad_split_24_beauty([p(a), p(b), p(c), p(d)]),
                _ => d1 < d2,
            };
            if split_ac {
                faces.push(vec![a, c, d]);
                src.push(f);
                extra.push((vec![a, b, c], f));
            } else {
                faces.push(vec![b, d, a]);
                src.push(f);
                extra.push((vec![b, c, d], f));
            }
        } else {
            let co: Vec<[f32; 3]> = fv.iter().map(|&i| m.pos[i as usize]).collect();
            let tris = crate::assets::polyfill::triangulate_ngon(&co);
            let last = tris.len() - 1;
            for (k, t) in tris.iter().enumerate() {
                let face = vec![fv[t[0] as usize], fv[t[1] as usize], fv[t[2] as usize]];
                if k == last {
                    faces.push(face);
                    src.push(f);
                } else {
                    extra.push((face, f));
                }
            }
        }
    }
    for (f, s) in extra {
        faces.push(f);
        src.push(s);
    }
    let mut o = Mesh::from_faces(m.pos.clone(), &faces, &m.edges);
    o.mats = m.mats.clone();
    for (name, a) in m.attrs.iter() {
        match a.dom {
            Dom::Point => {
                o.attrs.insert(name.clone(), a.clone());
            }
            Dom::Face => {
                o.attrs.insert(name.clone(), Attr { dom: Dom::Face, ty: a.ty, data: src.iter().map(|&i| a.data[i].clone()).collect() });
            }
            Dom::Edge => {
                let mut d = a.data.clone();
                d.resize(o.ne(), Val::zero(a.ty));
                o.attrs.insert(name.clone(), Attr { dom: Dom::Edge, ty: a.ty, data: d });
            }
            _ => {}
        }
    }
    o
}

/// Merge by distance (mode ALL, all vertices selected) with point/face attribute propagation
pub fn merge_by_distance_mesh(m: &Mesh, dist: f32) -> Mesh {
    use crate::assets::gweld::merge_by_distance;
    let g = to_gmesh(m);
    let mg = merge_by_distance(&g, dist);
    let mut o = from_gmesh(&mg);
    o.mats = m.mats.clone();
    // vertex mapping: replicate weld target selection to find the source of every output vertex
    let n = m.nv();
    let range_sq = dist * dist;
    let mut dup = vec![-2i32; n];
    for i in 0..n {
        if dup[i] == -2 || dup[i] == i as i32 {
            let mut found = false;
            for j in 0..n {
                if j != i && dup[j] == -2 && dsq(m.pos[j], m.pos[i]) <= range_sq {
                    dup[j] = i as i32;
                    found = true;
                }
            }
            if found {
                dup[i] = i as i32;
            }
        }
    }
    let kept: Vec<usize> = (0..n).filter(|&i| dup[i] == -2 || dup[i] == i as i32).collect();
    if kept.len() == o.nv() {
        for (name, a) in m.attrs.iter() {
            if a.dom == Dom::Point {
                o.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data: kept.iter().map(|&i| a.data[i].clone()).collect() });
            }
        }
    }
    // face mapping by greedy matching of vertex cycles
    let mut newidx = vec![0u32; n];
    for (k, &i) in kept.iter().enumerate() {
        newidx[i] = k as u32;
    }
    for i in 0..n {
        if dup[i] >= 0 && dup[i] != i as i32 {
            newidx[i] = newidx[dup[i] as usize];
        }
    }
    // edge attributes: first original edge mapping onto each surviving edge
    if m.attrs.values().any(|a| a.dom == Dom::Edge) {
        let mut first: HashMap<(u32, u32), usize> = HashMap::default();
        for (i, e) in m.edges.iter().enumerate() {
            let (a, b) = (newidx[e[0] as usize], newidx[e[1] as usize]);
            if a != b {
                first.entry((a.min(b), a.max(b))).or_insert(i);
            }
        }
        let src: Vec<Option<usize>> = o.edges.iter().map(|e| first.get(&(e[0].min(e[1]), e[0].max(e[1]))).copied()).collect();
        let eidx: HashMap<(u32, u32), usize> = o.edges.iter().enumerate().map(|(i, e)| ((e[0].min(e[1]), e[0].max(e[1])), i)).collect();
        for (name, a) in m.attrs.iter() {
            if a.dom == Dom::Edge {
                let mut data: Vec<Val> = src.iter().map(|s| s.map(|i| a.data[i].clone()).unwrap_or(Val::zero(a.ty))).collect();
                if a.ty == Ty::B {
                    // booleans mix with OR over all merged edges
                    for (i, e) in m.edges.iter().enumerate() {
                        let (x, y) = (newidx[e[0] as usize], newidx[e[1] as usize]);
                        if x != y {
                            if let Some(&k) = eidx.get(&(x.min(y), x.max(y))) {
                                if a.data[i].b() {
                                    data[k] = Val::B(true);
                                }
                            }
                        }
                    }
                }
                o.attrs.insert(name.clone(), Attr { dom: Dom::Edge, ty: a.ty, data });
            }
        }
    }
    let same_cycle = |a: &[u32], b: &[u32]| a.len() == b.len() && (0..b.len()).any(|s| (0..b.len()).all(|k| a[k] == b[(k + s) % b.len()]));
    let mut fsrc = vec![];
    let mut of = 0usize;
    for f in 0..m.nf() {
        if of >= o.nf() {
            break;
        }
        let mut mapped: Vec<u32> = m.face(f).iter().map(|&v| newidx[v as usize]).collect();
        mapped.dedup();
        while mapped.len() > 1 && mapped.first() == mapped.last() {
            mapped.pop();
        }
        if mapped.len() >= 3 && same_cycle(&mapped, o.face(of)) {
            fsrc.push(f);
            of += 1;
        }
    }
    if fsrc.len() == o.nf() {
        for (name, a) in m.attrs.iter() {
            if a.dom == Dom::Face {
                o.attrs.insert(name.clone(), Attr { dom: Dom::Face, ty: a.ty, data: fsrc.iter().map(|&i| a.data[i].clone()).collect() });
            }
        }
    }
    o
}

/// run the simple subdivision on float channels (each coarse vertex carries `chans[c][i]`), returning the channels of
/// the subdivided vertices (bilinear interpolation = how Blender interpolates vertex / corner data)
fn subdiv_channels(g: &crate::assets::cabnodes::GMesh, level: u32, chans: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let mut out: Vec<Vec<f32>> = vec![];
    let mut c = 0;
    while c < chans.len() {
        let mut gg = g.clone();
        for (i, p) in gg.v.iter_mut().enumerate() {
            *p = [chans[c][i], if c + 1 < chans.len() { chans[c + 1][i] } else { 0.0 }, if c + 2 < chans.len() { chans[c + 2][i] } else { 0.0 }];
        }
        let sg = crate::assets::gweld::subdivide(&gg, level);
        for k in 0..3.min(chans.len() - c) {
            out.push(sg.v.iter().map(|p| p[k]).collect());
        }
        c += 3;
    }
    out
}

fn val_to_chans(ty: Ty, v: &Val, out: &mut Vec<f32>) {
    match ty {
        Ty::F => out.push(v.f()),
        Ty::I => out.push(v.i() as f32),
        Ty::B => out.push(if v.b() { 1.0 } else { 0.0 }),
        Ty::V => out.extend(v.v()),
        Ty::C => out.extend(v.c()),
        _ => {}
    }
}
fn chans_to_val(ty: Ty, c: &[f32]) -> Val {
    match ty {
        Ty::F => Val::F(c[0]),
        Ty::I => Val::I(c[0].round() as i32),
        Ty::B => Val::B(c[0] >= 0.5),
        Ty::V => Val::V([c[0], c[1], c[2]]),
        Ty::C => Val::C([c[0], c[1], c[2], c[3]]),
        _ => Val::zero(ty),
    }
}
fn nchan(ty: Ty) -> usize {
    match ty {
        Ty::F | Ty::I | Ty::B => 1,
        Ty::V => 3,
        Ty::C => 4,
        _ => 0,
    }
}

/// propagate all attributes of `m` onto the subdivided mesh `o` (BKE subdiv_mesh.cc semantics)
fn subdiv_propagate(m: &Mesh, level: u32, o: &mut Mesh) {
    use crate::assets::gweld::{subdivide, subdivide_edges};
    let g = to_gmesh(m);
    let (_, esrc) = subdivide_edges(&g, level);
    // face sources
    let mut fsrc = vec![];
    for f in 0..m.nf() {
        let n = m.face_range(f).len();
        let kids = if n == 4 { 4usize.pow(level) } else { n * 4usize.pow(level - 1) };
        for _ in 0..kids {
            fsrc.push(f);
        }
    }
    // corner provenance via a split mesh (every face with private vertices)
    let mut split = crate::assets::cabnodes::GMesh::new();
    let mut sv: Vec<usize> = vec![];
    let mut sp: Vec<Vec<u32>> = vec![];
    for f in 0..m.nf() {
        let mut face = vec![];
        for c in m.face_range(f) {
            face.push(split.v.len() as u32);
            split.v.push(m.pos[m.cv[c] as usize]);
            sv.push(c);
        }
        sp.push(face);
    }
    for face in &sp {
        for i in 0..face.len() {
            split.e.push([face[i], face[(i + 1) % face.len()]]);
        }
    }
    split.p = sp;
    split.tag = vec![0; m.nf()];
    for (name, a) in m.attrs.iter() {
        let nc = nchan(a.ty);
        match a.dom {
            Dom::Face => {
                if fsrc.len() == o.nf() {
                    o.attrs.insert(name.clone(), Attr { dom: Dom::Face, ty: a.ty, data: fsrc.iter().map(|&i| a.data[i].clone()).collect() });
                }
            }
            Dom::Edge => {
                let data: Vec<Val> = (0..o.ne()).map(|i| if i < esrc.len() && esrc[i] >= 0 { a.data[esrc[i] as usize].clone() } else { Val::zero(a.ty) }).collect();
                o.attrs.insert(name.clone(), Attr { dom: Dom::Edge, ty: a.ty, data });
            }
            Dom::Point if nc > 0 => {
                let mut chans = vec![vec![0.0f32; m.nv()]; nc];
                for (i, v) in a.data.iter().enumerate() {
                    let mut t = vec![];
                    val_to_chans(a.ty, v, &mut t);
                    for k in 0..nc {
                        chans[k][i] = t[k];
                    }
                }
                let res = subdiv_channels(&g, level, &chans);
                let n = o.nv();
                let data: Vec<Val> = (0..n).map(|i| chans_to_val(a.ty, &(0..nc).map(|k| res[k][i]).collect::<Vec<f32>>())).collect();
                o.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data });
            }
            Dom::Corner if nc > 0 => {
                let mut chans = vec![vec![0.0f32; split.v.len()]; nc];
                for (i, v) in a.data.iter().enumerate() {
                    let mut t = vec![];
                    val_to_chans(a.ty, v, &mut t);
                    for k in 0..nc {
                        chans[k][i] = t[k];
                    }
                }
                let res = subdiv_channels(&split, level, &chans);
                let ssub = subdivide(&split, level);
                let mut data = vec![];
                for q in &ssub.p {
                    for &vi in q {
                        data.push(chans_to_val(a.ty, &(0..nc).map(|k| res[k][vi as usize]).collect::<Vec<f32>>()));
                    }
                }
                if data.len() == o.nc() {
                    o.attrs.insert(name.clone(), Attr { dom: Dom::Corner, ty: a.ty, data });
                }
            }
            _ => {}
        }
    }
    let _ = sv;
}

pub fn subdivide_mesh_simple(m: &Mesh, level: u32) -> Mesh {
    let g = to_gmesh(m);
    let sg = crate::assets::gweld::subdivide(&g, level);
    let mut o = from_gmesh(&sg);
    o.mats = m.mats.clone();
    if level > 0 {
        subdiv_propagate(m, level, &mut o);
    }
    o
}

pub fn subsurf_mesh(m: &Mesh, level: usize) -> Mesh {
    subsurf_mesh_cr(m, level, None)
}

pub fn subsurf_mesh_cr(m: &Mesh, level: usize, crease: Option<&crate::dhash::HashMap<(u32, u32), f64>>) -> Mesh {
    let mm = crate::mesh::Mesh {
        pos: m.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(),
        faces: m.faces_vec(),
        uv: vec![],
    };
    let q = crate::subdiv::QMesh { pos: mm.pos.clone(), faces: mm.faces.clone() };
    if level == 0 {
        return m.clone();
    }
    let t_cc = crate::subdiv::catmull_clark(&q, level, crease);
    let t_simple = crate::subdiv::simple_levels(&q, level);
    // Blender's vertex / edge / face layout (BKE subdiv_foreach), with Catmull-Clark positions mapped onto it
    let g = to_gmesh(m);
    let b = crate::assets::gweld::subdivide(&g, level as u32);
    let mut bpos: Vec<V3> = b.v.clone();
    let mut mapped = vec![false; b.v.len()];
    let mut ok = t_simple.pos.len() == b.v.len();
    if ok {
        // per coarse face: match B vertices to T vertices by (simple) position
        let mut bstart = 0usize;
        let mut tstart = 0usize;
        for f in 0..m.nf() {
            let n = m.face_range(f).len();
            let kids = if n == 4 { 4usize.pow(level as u32) } else { n * 4usize.pow(level as u32 - 1) };
            let bq = &b.p[bstart..bstart + kids];
            let tq = &t_simple.faces[tstart..tstart + kids];
            bstart += kids;
            tstart += kids;
            let mut bs: Vec<u32> = bq.iter().flatten().copied().collect();
            bs.sort();
            bs.dedup();
            let mut ts: Vec<u32> = tq.iter().flatten().copied().collect();
            ts.sort();
            ts.dedup();
            if bs.len() != ts.len() {
                ok = false;
                break;
            }
            let mut used = vec![false; ts.len()];
            for &bi in &bs {
                let bp = b.v[bi as usize];
                let mut best = (f64::MAX, usize::MAX);
                for (k, &ti) in ts.iter().enumerate() {
                    if used[k] {
                        continue;
                    }
                    let tp = t_simple.pos[ti as usize];
                    let d = (bp[0] as f64 - tp[0]).powi(2) + (bp[1] as f64 - tp[1]).powi(2) + (bp[2] as f64 - tp[2]).powi(2);
                    if d < best.0 {
                        best = (d, k);
                    }
                }
                if best.1 == usize::MAX {
                    ok = false;
                    break;
                }
                if mapped[bi as usize] {
                    continue; // shared vertex, already placed through the neighbouring face
                }
                used[best.1] = true;
                let tp = t_cc.pos[ts[best.1] as usize];
                bpos[bi as usize] = [tp[0] as f32, tp[1] as f32, tp[2] as f32];
                mapped[bi as usize] = true;
            }
            if !ok {
                break;
            }
        }
    }
    let mut o;
    if ok {
        let mut bb = b.clone();
        bb.v = bpos;
        o = from_gmesh(&bb);
    } else {
        let pos: Vec<V3> = t_cc.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        o = Mesh::from_faces(pos, &t_cc.faces, &[]);
    }
    o.mats = m.mats.clone();
    if ok {
        subdiv_propagate(m, level as u32, &mut o);
    }
    o
}

/// Convex hull (triangulated), incremental algorithm in f64
pub fn convex_hull(points: &[V3]) -> Mesh {
    let n = points.len();
    if n < 4 {
        return Mesh::new();
    }
    let p: Vec<[f64; 3]> = points.iter().map(|v| [v[0] as f64, v[1] as f64, v[2] as f64]).collect();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // initial tetrahedron
    let i0 = 0;
    let mut i1 = None;
    for i in 1..n {
        if dot(sub(p[i], p[i0]), sub(p[i], p[i0])) > 1e-18 {
            i1 = Some(i);
            break;
        }
    }
    let Some(i1) = i1 else { return Mesh::new() };
    let mut i2 = None;
    for i in 1..n {
        if i != i1 {
            let c = cross(sub(p[i1], p[i0]), sub(p[i], p[i0]));
            if dot(c, c) > 1e-18 {
                i2 = Some(i);
                break;
            }
        }
    }
    let Some(i2) = i2 else { return Mesh::new() };
    let nrm = cross(sub(p[i1], p[i0]), sub(p[i2], p[i0]));
    let mut i3 = None;
    for i in 1..n {
        if i != i1 && i != i2 && dot(nrm, sub(p[i], p[i0])).abs() > 1e-12 {
            i3 = Some(i);
            break;
        }
    }
    let Some(i3) = i3 else { return Mesh::new() };
    let mut faces: Vec<[usize; 3]> = vec![];
    let side = dot(nrm, sub(p[i3], p[i0]));
    if side > 0.0 {
        faces.push([i0, i2, i1]);
        faces.push([i0, i1, i3]);
        faces.push([i1, i2, i3]);
        faces.push([i2, i0, i3]);
    } else {
        faces.push([i0, i1, i2]);
        faces.push([i0, i3, i1]);
        faces.push([i1, i3, i2]);
        faces.push([i2, i3, i0]);
    }
    let normal = |f: &[usize; 3]| cross(sub(p[f[1]], p[f[0]]), sub(p[f[2]], p[f[0]]));
    for i in 0..n {
        if [i0, i1, i2, i3].contains(&i) {
            continue;
        }
        let visible: Vec<bool> = faces.iter().map(|f| dot(normal(f), sub(p[i], p[f[0]])) > 1e-12).collect();
        if !visible.iter().any(|&v| v) {
            continue;
        }
        // horizon edges
        let mut edges: HashMap<(usize, usize), (usize, usize)> = HashMap::default();
        for (fi, f) in faces.iter().enumerate() {
            if visible[fi] {
                for k in 0..3 {
                    let (a, b) = (f[k], f[(k + 1) % 3]);
                    edges.insert((a, b), (a, b));
                }
            }
        }
        let mut nf: Vec<[usize; 3]> = faces.iter().enumerate().filter(|(fi, _)| !visible[*fi]).map(|(_, f)| *f).collect();
        for (&(a, b), _) in edges.iter() {
            if !edges.contains_key(&(b, a)) {
                nf.push([a, b, i]);
            }
        }
        faces = nf;
    }
    // merge coplanar triangles into polygons and drop non-extreme vertices
    let scale = p.iter().fold(0.0f64, |m, q| m.max(q[0].abs()).max(q[1].abs()).max(q[2].abs())).max(1e-9);
    let nf = faces.len();
    let norms: Vec<[f64; 3]> = faces
        .iter()
        .map(|f| {
            let c = normal(f);
            let l = dot(c, c).sqrt();
            if l > 0.0 { [c[0] / l, c[1] / l, c[2] / l] } else { c }
        })
        .collect();
    let mut parent: Vec<usize> = (0..nf).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    let mut edge_face: HashMap<(usize, usize), usize> = HashMap::default();
    for (fi, f) in faces.iter().enumerate() {
        for k in 0..3 {
            edge_face.insert((f[k], f[(k + 1) % 3]), fi);
        }
    }
    for (fi, f) in faces.iter().enumerate() {
        for k in 0..3 {
            if let Some(&gj) = edge_face.get(&(f[(k + 1) % 3], f[k])) {
                let d = sub(norms[fi], norms[gj]);
                if dot(d, d).sqrt() < 2e-5 {
                    let (a, b) = (find(&mut parent, fi), find(&mut parent, gj));
                    parent[a] = b;
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut gidx: HashMap<usize, usize> = HashMap::default();
    for fi in 0..nf {
        let r = find(&mut parent, fi);
        let gi = *gidx.entry(r).or_insert_with(|| {
            groups.push(vec![]);
            groups.len() - 1
        });
        groups[gi].push(fi);
    }
    let mut loops: Vec<Vec<usize>> = vec![];
    for g in &groups {
        let mut next: HashMap<usize, usize> = HashMap::default();
        let mut set: crate::dhash::HashSet<(usize, usize)> = crate::dhash::HashSet::default();
        for &fi in g {
            for k in 0..3 {
                set.insert((faces[fi][k], faces[fi][(k + 1) % 3]));
            }
        }
        for &(a, b) in &set {
            if !set.contains(&(b, a)) {
                next.insert(a, b);
            }
        }
        if let Some((&start, _)) = next.iter().min_by_key(|(k, _)| **k) {
            let mut lp = vec![start];
            let mut cur = next[&start];
            let mut guard = 0;
            while cur != start && guard < 10000 {
                lp.push(cur);
                cur = match next.get(&cur) {
                    Some(&c) => c,
                    None => break,
                };
                guard += 1;
            }
            loops.push(lp);
        }
    }
    let collinear = |a: usize, b: usize, c: usize| -> bool {
        let u = sub(p[b], p[a]);
        let v = sub(p[c], p[b]);
        let cr = cross(u, v);
        dot(cr, cr).sqrt() <= 2e-5 * dot(u, u).sqrt() * dot(v, v).sqrt()
    };
    let mut needed = vec![false; n];
    for lp in &loops {
        let m = lp.len();
        for k in 0..m {
            if !collinear(lp[(k + m - 1) % m], lp[k], lp[(k + 1) % m]) {
                needed[lp[k]] = true;
            }
        }
    }
    let mut map = vec![u32::MAX; n];
    let mut pos = vec![];
    for i in 0..n {
        if needed[i] {
            map[i] = pos.len() as u32;
            pos.push(points[i]);
        }
    }
    let fl: Vec<Vec<u32>> = loops.iter().map(|lp| lp.iter().filter(|&&v| needed[v]).map(|&v| map[v]).collect::<Vec<u32>>()).filter(|f| f.len() >= 3).collect();
    let mut m = Mesh::from_faces(pos, &fl, &[]);
    super::prim::set_sharp_all(&mut m);
    m
}

pub fn mesh_nodes2(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    let ty = node.ty.as_str();
    let r: Outs = match ty {
        "GeometryNodeMergeByDistance" => {
            let g = ev.geo(ni, "Geometry")?;
            let d = ev.cf(ni, "Distance")?;
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                let sel = selection(ev, ni, Comp::Mesh(m), Dom::Point)?;
                if sel.iter().all(|&s| s) {
                    o.mesh = Some(merge_by_distance_mesh(m, d));
                } else {
                    return Err("MergeByDistance with partial selection unsupported".into());
                }
            }
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        "GeometryNodeSubdivideMesh" => {
            let g = ev.geo(ni, "Mesh")?;
            let lvl = ev.ci(ni, "Level")?.clamp(0, 11) as u32;
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                o.mesh = Some(subdivide_mesh_simple(m, lvl));
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeSubdivisionSurface" => {
            let g = ev.geo(ni, "Mesh")?;
            let lvl = ev.ci(ni, "Level")?.clamp(0, 6) as usize;
            let mut crease: Option<crate::dhash::HashMap<(u32, u32), f64>> = None;
            if let Some(m) = &g.mesh {
                let comp = Comp::Mesh(m);
                if ev.has_input(ni, "Edge Crease") {
                    let ec = eval_on(&ev.field(ni, "Edge Crease")?, comp, Dom::Edge);
                    let mut mp = crate::dhash::HashMap::default();
                    for (i, e) in m.edges.iter().enumerate() {
                        let c = ec[i].f() as f64;
                        if c > 0.0 {
                            mp.insert((e[0].min(e[1]), e[0].max(e[1])), c.min(1.0));
                        }
                    }
                    if !mp.is_empty() {
                        crease = Some(mp);
                    }
                }
                if ev.has_input(ni, "Vertex Crease") {
                    let vc = eval_on(&ev.field(ni, "Vertex Crease")?, comp, Dom::Point);
                    if vc.iter().any(|v| v.f() != 0.0) {
                        return Err("SubdivisionSurface vertex creases unsupported".into());
                    }
                }
            }
            // like geometry_set.modify_geometry_sets: also applies to the meshes inside instance references
            let prev = crate::subdiv::set_corner_fixed(node.ps("boundary_smooth") != "ALL");
            fn rec(g: &Geometry, lvl: usize, cr: Option<&crate::dhash::HashMap<(u32, u32), f64>>) -> Geometry {
                let mut o = g.clone();
                if let Some(m) = &g.mesh {
                    o.mesh = Some(if lvl == 0 { m.clone() } else { subsurf_mesh_cr(m, lvl, cr) });
                }
                if let Some(i) = &g.inst {
                    let mut ni2 = i.clone();
                    ni2.refs = i.refs.iter().map(|r| Rc::new(rec(r, lvl, None))).collect();
                    o.inst = Some(ni2);
                }
                o
            }
            let res = rec(&g, lvl, crease.as_ref());
            crate::subdiv::set_corner_fixed(prev);
            outs(node, vec![("Mesh", rcgeo(res))])
        }
        "GeometryNodeMeshBoolean" => {
            let op = node.ps("operation");
            let mut o = Geometry::default();
            let ident: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
            let mut geos: Vec<Rc<Geometry>> = vec![];
            let mut first_geo: Option<Rc<Geometry>> = None;
            if op == "DIFFERENCE" {
                first_geo = Some(ev.geo(ni, "Mesh 1")?);
            }
            for v in ev.input_multi(ni, "Mesh 2")? {
                if let Value::G(g) = v {
                    geos.push(g);
                }
            }
            // the multi-input is processed in reverse link order, like Join Geometry
            geos.reverse();
            // operands: Mesh 1 (mesh only), then for each Mesh 2 geometry its mesh followed by the meshes of its instances
            let mut operands: Vec<(&Mesh, M4)> = vec![];
            if let Some(g) = &first_geo {
                if let Some(m) = &g.mesh {
                    operands.push((m, ident));
                }
            }
            for g in &geos {
                if let Some(m) = &g.mesh {
                    operands.push((m, ident));
                }
                if let Some(inst) = &g.inst {
                    for i in 0..inst.n() {
                        if let Some(Some(m)) = inst.refs.get(inst.idx[i] as usize).map(|r| r.mesh.as_ref()) {
                            operands.push((m, inst.xf[i]));
                        }
                    }
                }
            }
            if !operands.is_empty() {
                let float = node.ps("solver") == "FLOAT" && std::env::var_os("GNBOOLEXACT").is_none();
                let exact = node.ps("solver") != "FLOAT" || std::env::var_os("GNBOOLEXACT").is_some();
                o.mesh = Some(if float && std::env::var_os("GNBOOLBSP").is_none() {
                    super::fbool::mesh_boolean_float(op, &operands)
                } else {
                    // the other solvers take the transformed operands
                    let owned: Vec<Option<Mesh>> = operands.iter().map(|(m, x)| if *x == ident { None } else { Some(super::fbool::transform_mesh(m, x)) }).collect();
                    let refs: Vec<&Mesh> = operands.iter().zip(owned.iter()).map(|((m, _), ow)| ow.as_ref().unwrap_or(*m)).collect();
                    let (m1, ms) = (refs[0], &refs[1..]);
                    if exact && std::env::var_os("GNBOOLBSP").is_none() {
                        let (us, ht) = (ev.cb(ni, "Self Intersection").unwrap_or(false), ev.cb(ni, "Hole Tolerant").unwrap_or(false));
                        super::csg::mesh_boolean_exact(op, m1, ms, us, ht)
                    } else {
                        super::csg::mesh_boolean(op, m1, ms)
                    }
                });
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeDistributePointsOnFaces" => {
            let g = ev.geo(ni, "Mesh")?;
            let poisson = node.ps("distribute_method") == "POISSON";
            let seed = ev.ci(ni, "Seed")?;
            let mut o = Geometry::default();
            let nname = anon_name("dist_n");
            let rname = anon_name("dist_r");
            if let Some(m) = &g.mesh {
                let comp = Comp::Mesh(m);
                let sel = selection(ev, ni, comp, Dom::Face)?;
                let (mut pts, nrm, rot) = if poisson {
                    let fac: Vec<f32> = eval_on(&ev.field(ni, "Density Factor")?, comp, Dom::Corner).iter().map(|v| v.f()).collect();
                    distribute_poisson(m, ev.cf(ni, "Distance Min")?, ev.cf(ni, "Density Max")?, &fac, &sel, seed)
                } else {
                    let dens: Vec<f32> = eval_on(&ev.field(ni, "Density")?, comp, Dom::Corner).iter().map(|v| v.f()).collect();
                    distribute_random(m, &dens, &sel, seed)
                };
                pts.attrs.insert(nname.clone(), Attr { dom: Dom::Point, ty: Ty::V, data: nrm.iter().map(|v| Val::V(*v)).collect() });
                pts.attrs.insert(rname.clone(), Attr { dom: Dom::Point, ty: Ty::Q, data: rot.iter().map(|v| Val::Q(*v)).collect() });
                o.points = Some(pts);
            }
            outs(node, vec![("Points", rcgeo(o)), ("Normal", fld(Rc::new(FNode::Named(nname, Ty::V)))), ("Rotation", fld(Rc::new(FNode::Named(rname, Ty::Q))))])
        }
        "GeometryNodeInputShortestEdgePaths" => {
            let end = ev.field(ni, "End Vertex")?;
            let cost = ev.field(ni, "Edge Cost")?;
            let mk = |which: u8| {
                let (end, cost) = (end.clone(), cost.clone());
                Rc::new(FNode::Ctx(Rc::new(move |ctx: &Ctx| match ctx.comp {
                    Comp::Mesh(m) => {
                        let ends: Vec<bool> = eval_on(&end, ctx.comp, Dom::Point).iter().map(|v| v.b()).collect();
                        let costs: Vec<f32> = eval_on(&cost, ctx.comp, Dom::Edge).iter().map(|v| v.f()).collect();
                        let (next, total) = shortest_paths(m, &ends, &costs);
                        let v: Vec<Val> = if which == 0 { next.iter().map(|&x| Val::I(x)).collect() } else { total.iter().map(|&x| Val::F(x)).collect() };
                        if ctx.dom == Dom::Point {
                            v
                        } else {
                            adapt_mesh(m, Dom::Point, ctx.dom, if which == 0 { Ty::I } else { Ty::F }, &v)
                        }
                    }
                    _ => vec![Val::zero(if which == 0 { Ty::I } else { Ty::F }); ctx.n],
                })))
            };
            outs(node, vec![("Next Vertex Index", fld(mk(0))), ("Total Cost", fld(mk(1)))])
        }
        "GeometryNodeEdgePathsToCurves" => {
            let g = ev.geo(ni, "Mesh")?;
            let mut o = Geometry::default();
            if let Some(m) = &g.mesh {
                let comp = Comp::Mesh(m);
                let start: Vec<bool> = eval_on(&ev.field(ni, "Start Vertices")?, comp, Dom::Point).iter().map(|v| v.b()).collect();
                let next: Vec<i32> = eval_on(&ev.field(ni, "Next Vertex Index")?, comp, Dom::Point).iter().map(|v| v.i()).collect();
                let mut c = Curves::new();
                for v in 0..m.nv() {
                    if !start[v] {
                        continue;
                    }
                    let mut chain = vec![v];
                    let mut cur = v;
                    let mut guard = 0;
                    while next[cur] >= 0 && next[cur] as usize != cur && guard < m.nv() {
                        cur = next[cur] as usize;
                        chain.push(cur);
                        guard += 1;
                    }
                    if chain.len() >= 2 {
                        let pts: Vec<V3> = chain.iter().map(|&i| m.pos[i]).collect();
                        c.push_poly(&pts, false);
                    }
                }
                o.curves = Some(c);
            }
            outs(node, vec![("Curves", rcgeo(o))])
        }
        "GeometryNodeTriangulate" => {
            let g = ev.geo(ni, "Mesh")?;
            let minv = ev.ci(ni, "Minimum Vertices")?.max(4) as usize;
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                let sel = selection(ev, ni, Comp::Mesh(m), Dom::Face)?;
                o.mesh = Some(triangulate(m, &sel, minv, node.ps("quad_method")));
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeConvexHull" => {
            let g = ev.geo(ni, "Geometry")?;
            let mut pts: Vec<V3> = vec![];
            if let Some(m) = &g.mesh {
                pts.extend(m.pos.iter().cloned());
            }
            if let Some(p) = &g.points {
                pts.extend(p.pos.iter().cloned());
            }
            if let Some(c) = &g.curves {
                for k in 0..c.nc() {
                    pts.extend(super::curveops::eval_curve(c, k).pos);
                }
            }
            if let Some(i) = &g.inst {
                let real = realize_instances(&Geometry { inst: Some(i.clone()), ..Default::default() }, 100);
                if let Some(m) = &real.mesh {
                    pts.extend(m.pos.iter().cloned());
                }
            }
            outs(node, vec![("Convex Hull", rcgeo(Geometry::from_mesh(hull_mesh(&pts))))])
        }
        "GeometryNodeDuplicateElements" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let amt = ev.field(ni, "Amount")?;
            let mut o = Geometry::default();
            let anon = anon_name("dupidx");
            if let Some(m) = &g.mesh {
                if matches!(dom, Dom::Point | Dom::Edge | Dom::Face) {
                    let comp = Comp::Mesh(m);
                    let sel = selection(ev, ni, comp, dom)?;
                    let am = eval_on(&amt, comp, dom);
                    o.mesh = Some(duplicate_mesh(m, dom, &sel, &am, &anon));
                }
            }
            if let Some(c) = &g.curves {
                if dom == Dom::Curve {
                    let comp = Comp::Curves(c);
                    let sel = selection(ev, ni, comp, dom)?;
                    let am = eval_on(&amt, comp, dom);
                    let mut oc = Curves::new();
                    oc.has_radius = c.has_radius;
                    let mut idx = vec![];
                    for k in 0..c.nc() {
                        if !sel[k] {
                            continue;
                        }
                        for j in 0..am[k].i().max(0) {
                            let r = c.range(k);
                            let pts: Vec<V3> = c.pos[r.clone()].to_vec();
                            oc.push_kind(c.kind[k], &pts, c.cyclic[k]);
                            let base = oc.pos.len() - pts.len();
                            for (i, p) in r.enumerate() {
                                oc.radius[base + i] = c.radius[p];
                                oc.tilt[base + i] = c.tilt[p];
                                oc.hl[base + i] = c.hl[p];
                                oc.hr[base + i] = c.hr[p];
                            }
                            idx.push(Val::I(j));
                        }
                    }
                    oc.attrs.insert(anon.clone(), Attr { dom: Dom::Curve, ty: Ty::I, data: idx });
                    o.curves = Some(oc);
                }
            }
            if let Some(i) = &g.inst {
                if dom == Dom::Instance {
                    let comp = Comp::Inst(i);
                    let sel = selection(ev, ni, comp, dom)?;
                    let am = eval_on(&amt, comp, dom);
                    let mut oi = Instances::default();
                    oi.refs = i.refs.clone();
                    let mut idx = vec![];
                    for k in 0..i.n() {
                        if !sel[k] {
                            continue;
                        }
                        for j in 0..am[k].i().max(0) {
                            oi.idx.push(i.idx[k]);
                            oi.xf.push(i.xf[k]);
                            idx.push(Val::I(j));
                        }
                    }
                    if std::env::var("GNDUMP").is_ok() {
                        eprintln!("dup inst: n {} amounts {:?} -> {}", i.n(), am.iter().map(|v| v.i()).collect::<Vec<_>>(), oi.idx.len());
                    }
                    oi.attrs.insert(anon.clone(), Attr { dom: Dom::Instance, ty: Ty::I, data: idx });
                    o.inst = Some(oi);
                }
            }
            outs(node, vec![("Geometry", rcgeo(o)), ("Duplicate Index", fld(Rc::new(FNode::Named(anon, Ty::I))))])
        }
        "GeometryNodeDualMesh" => {
            let g = ev.geo(ni, "Mesh")?;
            let keep = ev.cb(ni, "Keep Boundaries")?;
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                o.mesh = Some(super::dual::dual_mesh(m, keep));
            }
            outs(node, vec![("Dual Mesh", rcgeo(o))])
        }
        "GeometryNodeSplitEdges" => {
            let g = ev.geo(ni, "Mesh")?;
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                let sel = selection(ev, ni, Comp::Mesh(m), Dom::Edge)?;
                o.mesh = Some(split_edges(m, &sel));
            }
            outs(node, vec![("Mesh", rcgeo(o))])
        }
        "GeometryNodeScaleElements" => {
            let g = ev.geo(ni, "Geometry")?;
            let dom = Dom::from_name(node.ps("domain"));
            let mut o = (*g).clone();
            if let Some(m) = &g.mesh {
                let comp = Comp::Mesh(m);
                let sel = selection(ev, ni, comp, dom)?;
                let sc: Vec<f32> = eval_on(&ev.field(ni, "Scale")?, comp, dom).iter().map(|v| v.f()).collect();
                let single_axis = node.ps("scale_mode") == "SINGLE_AXIS";
                let axis: Vec<V3> = if single_axis { eval_on(&ev.field(ni, "Axis")?, comp, dom).iter().map(|v| v.v()).collect() } else { vec![] };
                let centers: Vec<V3> = eval_on(&ev.field(ni, "Center")?, comp, dom).iter().map(|v| v.v()).collect();
                o.mesh = Some(scale_elements(m, dom, &sel, &sc, &centers, &axis));
            }
            outs(node, vec![("Geometry", rcgeo(o))])
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

fn duplicate_mesh(m: &Mesh, dom: Dom, sel: &[bool], amt: &[Val], anon: &str) -> Mesh {
    let mut o = Mesh::new();
    o.mats = m.mats.clone();
    let mut vsrc: Vec<usize> = vec![];
    let mut esrc: Vec<usize> = vec![];
    let mut fsrc: Vec<usize> = vec![];
    let mut csrc: Vec<usize> = vec![];
    let mut dupidx: Vec<i32> = vec![];
    let n = m.size(dom);
    for i in 0..n {
        if !sel[i] {
            continue;
        }
        for j in 0..amt[i].i().max(0) {
            dupidx.push(j);
            match dom {
                Dom::Point => {
                    o.pos.push(m.pos[i]);
                    vsrc.push(i);
                }
                Dom::Edge => {
                    let e = m.edges[i];
                    let b = o.pos.len() as u32;
                    o.pos.push(m.pos[e[0] as usize]);
                    o.pos.push(m.pos[e[1] as usize]);
                    vsrc.push(e[0] as usize);
                    vsrc.push(e[1] as usize);
                    o.edges.push([b, b + 1]);
                    esrc.push(i);
                }
                _ => {
                    let b = o.pos.len() as u32;
                    let r = m.face_range(i);
                    for c in r.clone() {
                        o.pos.push(m.pos[m.cv[c] as usize]);
                        vsrc.push(m.cv[c] as usize);
                        csrc.push(c);
                    }
                    let k = r.len() as u32;
                    for q in 0..k {
                        o.edges.push([b + q, b + (q + 1) % k]);
                        esrc.push(m.ce[r.start + q as usize] as usize);
                    }
                    o.cv.extend((0..k).map(|q| b + q));
                    o.foff.push(o.cv.len() as u32);
                    fsrc.push(i);
                }
            }
        }
    }
    if dom != Dom::Face {
        o.foff = vec![0];
    } else {
        o.ce = (0..o.cv.len()).map(|c| c as u32).collect();
        // corner edge: face f corner q -> edge base+q
        let mut ce = vec![];
        let mut eb = 0u32;
        for f in 0..o.nf() {
            let k = o.face_range(f).len() as u32;
            for q in 0..k {
                ce.push(eb + q);
            }
            eb += k;
        }
        o.ce = ce;
    }
    for (name, a) in m.attrs.iter() {
        let (src, d) = match a.dom {
            Dom::Point => (&vsrc, Dom::Point),
            Dom::Edge => (&esrc, Dom::Edge),
            Dom::Face => (&fsrc, Dom::Face),
            _ => (&csrc, Dom::Corner),
        };
        if (d == Dom::Edge && dom == Dom::Point) || (d == Dom::Face && dom != Dom::Face) || (d == Dom::Corner && dom != Dom::Face) || (d == Dom::Edge && src.len() != o.ne()) {
            continue;
        }
        o.attrs.insert(name.clone(), Attr { dom: d, ty: a.ty, data: src.iter().map(|&i| a.data[i].clone()).collect() });
    }
    // duplicate index on the working domain
    let ddom = dom;
    let ddata: Vec<Val> = match dom {
        Dom::Point => dupidx.iter().map(|&x| Val::I(x)).collect(),
        Dom::Edge => dupidx.iter().map(|&x| Val::I(x)).collect(),
        _ => dupidx.iter().map(|&x| Val::I(x)).collect(),
    };
    o.attrs.insert(anon.to_string(), Attr { dom: ddom, ty: Ty::I, data: ddata });
    o
}

fn split_edges(m: &Mesh, sel: &[bool]) -> Mesh {
    // corner groups per vertex connected through unselected edges
    let mut o = m.clone();
    o.vnor = None;
    let nv = m.nv();
    let mut corners_of: Vec<Vec<usize>> = vec![vec![]; nv];
    for c in 0..m.nc() {
        corners_of[m.cv[c] as usize].push(c);
    }
    let cf = m.corner_face();
    let mut newv_of_corner: Vec<u32> = m.cv.clone();
    let mut vsrc = vec![];
    for v in 0..nv {
        let cs = &corners_of[v];
        if cs.len() < 2 {
            continue;
        }
        // union-find over corners at this vertex: two corners are joined if their faces share an unselected edge at v
        let mut parent: Vec<usize> = (0..cs.len()).collect();
        fn find(p: &mut Vec<usize>, x: usize) -> usize {
            if p[x] != x {
                let r = find(p, p[x]);
                p[x] = r;
            }
            p[x]
        }
        for (a, &ca) in cs.iter().enumerate() {
            for (b, &cb) in cs.iter().enumerate().skip(a + 1) {
                // edges at v of corner ca: the edge going out (ce[ca]) and the one coming in (ce[prev])
                let edges_at = |c: usize| -> [u32; 2] {
                    let f = cf[c] as usize;
                    let r = m.face_range(f);
                    let k = c - r.start;
                    let prev = r.start + (k + r.len() - 1) % r.len();
                    [m.ce[c], m.ce[prev]]
                };
                let (ea, eb) = (edges_at(ca), edges_at(cb));
                let mut shared = false;
                for x in ea {
                    for y in eb {
                        if x == y && !sel[x as usize] {
                            shared = true;
                        }
                    }
                }
                if shared {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    parent[ra] = rb;
                }
            }
        }
        let mut roots: Vec<usize> = vec![];
        for a in 0..cs.len() {
            let r = find(&mut parent, a);
            if !roots.contains(&r) {
                roots.push(r);
            }
        }
        for (k, &r) in roots.iter().enumerate() {
            if k == 0 {
                continue;
            }
            let nvid = o.pos.len() as u32;
            o.pos.push(m.pos[v]);
            vsrc.push(v);
            for (a, &ca) in cs.iter().enumerate() {
                if find(&mut parent, a) == r {
                    newv_of_corner[ca] = nvid;
                }
            }
        }
    }
    o.cv = newv_of_corner;
    extend_attrs(&mut o, Dom::Point, &vsrc);
    // edges: rebuild from faces, keeping attribute data by matching old edge ids
    let old_ce = m.ce.clone();
    let mut new_edges: Vec<[u32; 2]> = vec![];
    let mut esrc: Vec<usize> = vec![];
    let mut map: HashMap<(u32, u32), u32> = HashMap::default();
    let mut ce = vec![0u32; o.cv.len()];
    for f in 0..o.nf() {
        let r = o.face_range(f);
        let n = r.len();
        for c in 0..n {
            let (a, b) = (o.cv[r.start + c], o.cv[r.start + (c + 1) % n]);
            let key = (a.min(b), a.max(b));
            let id = *map.entry(key).or_insert_with(|| {
                new_edges.push([key.0, key.1]);
                esrc.push(old_ce[r.start + c] as usize);
                new_edges.len() as u32 - 1
            });
            ce[r.start + c] = id;
        }
    }
    // loose original edges stay
    for (i, e) in m.edges.iter().enumerate() {
        if !m.ce.iter().any(|&x| x as usize == i) {
            new_edges.push(*e);
            esrc.push(i);
        }
    }
    o.edges = new_edges;
    o.ce = ce;
    for a in o.attrs.values_mut() {
        if a.dom == Dom::Edge {
            let old = a.data.clone();
            a.data = esrc.iter().map(|&i| old.get(i).cloned().unwrap_or(Val::zero(a.ty))).collect();
        }
    }
    o
}

fn scale_elements(m: &Mesh, dom: Dom, sel: &[bool], scale: &[f32], centers: &[V3], axis: &[V3]) -> Mesh {
    // selected elements sharing vertices form islands; an island is scaled about the mean of its element centers
    let _ = axis;
    let mut o = m.clone();
    o.vnor = None;
    let nv = m.nv();
    let n = m.size(dom);
    let verts_of = |i: usize| -> Vec<u32> { if dom == Dom::Face { m.face(i).to_vec() } else { m.edges[i].to_vec() } };
    let mut parent: Vec<usize> = (0..nv).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for i in 0..n {
        if !sel[i] {
            continue;
        }
        let vs = verts_of(i);
        for w in vs.windows(2) {
            let (a, b) = (find(&mut parent, w[0] as usize), find(&mut parent, w[1] as usize));
            parent[a] = b;
        }
    }
    let mut acc_c: HashMap<usize, ([f32; 3], f32, u32)> = HashMap::default();
    for i in 0..n {
        if !sel[i] {
            continue;
        }
        let vs = verts_of(i);
        let r = find(&mut parent, vs[0] as usize);
        let e = acc_c.entry(r).or_insert(([0.0; 3], 0.0, 0));
        e.0 = add3(e.0, centers[i]);
        e.1 += scale[i];
        e.2 += 1;
    }
    let mut used = vec![false; nv];
    for i in 0..n {
        if sel[i] {
            for v in verts_of(i) {
                used[v as usize] = true;
            }
        }
    }
    for v in 0..nv {
        if !used[v] {
            continue;
        }
        let r = find(&mut parent, v);
        let (c, s, k) = acc_c[&r];
        let kf = k as f32;
        let c = mul3(c, 1.0 / kf);
        let s = s / kf;
        let rel = sub3(m.pos[v], c);
        o.pos[v] = add3(c, mul3(rel, s));
    }
    o
}

// ====================================================================================== Distribute Points on Faces

/// Blender RandomNumberGenerator (drand48 variant)
pub struct BlRng {
    x: u64,
}
impl BlRng {
    pub fn new(seed: u32) -> BlRng {
        BlRng { x: ((seed as u64) << 16) | 0x330E }
    }
    fn step(&mut self) {
        self.x = (0x5DEECE66Du64.wrapping_mul(self.x).wrapping_add(0xB)) & 0x0000_FFFF_FFFF_FFFF;
    }
    pub fn get_uint32(&mut self) -> u32 {
        self.step();
        (self.x >> 17) as u32
    }
    pub fn get_float(&mut self) -> f32 {
        (self.get_uint32() as i32) as f32 / 2147483648.0
    }
    pub fn bary(&mut self) -> [f32; 3] {
        let mut a = self.get_float();
        let mut b = self.get_float();
        if a + b > 1.0 {
            a = 1.0 - a;
            b = 1.0 - b;
        }
        [a, b, 1.0 - a - b]
    }
}

/// mesh loop triangles (corner indices) with owning face, Blender mesh_calc_tessellation order
pub fn looptris(m: &Mesh) -> Vec<([usize; 3], usize)> {
    let mut out = vec![];
    for f in 0..m.nf() {
        let r = m.face_range(f);
        let s = r.start;
        match r.len() {
            3 => out.push(([s, s + 1, s + 2], f)),
            4 => {
                let p = |k: usize| m.pos[m.cv[s + k] as usize];
                let d12 = sub3(p(1), p(0));
                let d13 = sub3(p(2), p(0));
                let d14 = sub3(p(3), p(0));
                let ca = cross3(d12, d13);
                let cb = cross3(d14, d13);
                if dot3(ca, cb) > 0.0 {
                    out.push(([s, s + 1, s + 3], f));
                    out.push(([s + 1, s + 2, s + 3], f));
                } else {
                    out.push(([s, s + 1, s + 2], f));
                    out.push(([s, s + 2, s + 3], f));
                }
            }
            n if n > 4 => {
                let co: Vec<[f32; 3]> = (0..n).map(|k| m.pos[m.cv[s + k] as usize]).collect();
                for t in crate::assets::polyfill::triangulate_ngon(&co) {
                    out.push(([s + t[0] as usize, s + t[1] as usize, s + t[2] as usize], f));
                }
            }
            _ => {}
        }
    }
    out
}

fn area_tri(a: V3, b: V3, c: V3) -> f32 {
    len3(cross3(sub3(a, b), sub3(b, c))) * 0.5
}

pub fn rotation_between(v1: V3, v2: V3) -> [f32; 4] {
    let axis = cross3(v1, v2);
    let (an, l) = crate::bl32::normalize_v3(axis);
    if l > f32::EPSILON {
        let ang = {
            let (a, b) = (norm3_c(v1), norm3_c(v2));
            if dot3(a, b) >= 0.0 {
                2.0 * (len3(sub3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
            } else {
                std::f32::consts::PI - 2.0 * (len3(add3(a, b)) / 2.0).clamp(-1.0, 1.0).asin()
            }
        };
        super::rot::axis_angle_to_quat(an, ang)
    } else if dot3(v1, v2) > 0.0 {
        [1.0, 0.0, 0.0, 0.0]
    } else {
        // orthogonal axis, rotate by pi
        let ax = if v1[0].abs() < 0.5 { cross3(v1, [1.0, 0.0, 0.0]) } else { cross3(v1, [0.0, 1.0, 0.0]) };
        super::rot::axis_angle_to_quat(norm3(ax), std::f32::consts::PI)
    }
}

/// Distribute Points rotation: Z = normal, X = normalize(cross(up, normal)) (fallback cross(normal, y)), Y = cross(Z, X)
pub fn normal_frame_quat(n: V3) -> [f32; 4] {
    let mut x = cross3([0.0, 0.0, 1.0], n);
    if len3(x) < 1e-5 {
        x = cross3(n, [0.0, 1.0, 0.0]);
    }
    let x = norm3(x);
    let y = cross3(n, x);
    crate::bl32::mat3_normalized_to_quat(&[x, y, n])
}

pub fn distribute_random(m: &Mesh, density: &[f32], sel: &[bool], seed: i32) -> (Points, Vec<V3>, Vec<[f32; 4]>) {
    let seed = seed.wrapping_mul(5383843);
    let mut ids: Vec<Val> = vec![];
    let tris = looptris(m);
    let fnor = face_normals(m);
    let mut pts = Points::default();
    let mut normals = vec![];
    let mut rots = vec![];
    for (ti, (c, f)) in tris.iter().enumerate() {
        if !sel[*f] {
            continue;
        }
        let v = |k: usize| m.pos[m.cv[c[k]] as usize];
        let (p0, p1, p2) = (v(0), v(1), v(2));
        let d = (density[c[0]].max(0.0) + density[c[1]].max(0.0) + density[c[2]].max(0.0)) / 3.0;
        let area = area_tri(p0, p1, p2);
        let tri_seed = hash2(ti as u32, seed as u32);
        let mut rng = BlRng::new(tri_seed);
        let amount_fl = area * d;
        let fract = amount_fl - amount_fl.floor();
        let add = fract > rng.get_float();
        let n = amount_fl as i32 + add as i32;
        for _ in 0..n {
            let w = rng.bary();
            pts.pos.push([p0[0] * w[0] + p1[0] * w[1] + p2[0] * w[2], p0[1] * w[0] + p1[1] * w[1] + p2[1] * w[2], p0[2] * w[0] + p1[2] * w[1] + p2[2] * w[2]]);
            normals.push(fnor[*f]);
            rots.push(normal_frame_quat(fnor[*f]));
            ids.push(Val::I(hash2(hash3(w[0].to_bits(), w[1].to_bits(), w[2].to_bits()), ti as u32) as i32));
        }
    }
    pts.attrs.insert("id".into(), Attr { dom: Dom::Point, ty: Ty::I, data: ids });
    (pts, normals, rots)
}


/// Distribute Points on Faces, Poisson disk mode (distribute_points_poisson_disk)
pub fn distribute_poisson(m: &Mesh, min_dist: f32, max_density: f32, factor: &[f32], sel: &[bool], seed: i32) -> (Points, Vec<V3>, Vec<[f32; 4]>) {
    let seed = seed.wrapping_mul(5383843);
    let tris = looptris(m);
    let fnor = face_normals(m);
    let mut pos: Vec<V3> = vec![];
    let mut bary: Vec<V3> = vec![];
    let mut tri_idx: Vec<usize> = vec![];
    for (ti, (c, _f)) in tris.iter().enumerate() {
        let v = |k: usize| m.pos[m.cv[c[k]] as usize];
        let (p0, p1, p2) = (v(0), v(1), v(2));
        let area = area_tri(p0, p1, p2);
        let mut rng = BlRng::new(hash2(ti as u32, seed as u32));
        let amount_fl = area * max_density * 1.0;
        let fract = amount_fl - amount_fl.floor();
        let add = fract > rng.get_float();
        let n = amount_fl as i32 + add as i32;
        for _ in 0..n {
            let w = rng.bary();
            pos.push([p0[0] * w[0] + p1[0] * w[1] + p2[0] * w[2], p0[1] * w[0] + p1[1] * w[1] + p2[1] * w[2], p0[2] * w[0] + p1[2] * w[1] + p2[2] * w[2]]);
            bary.push(w);
            tri_idx.push(ti);
        }
    }
    let n = pos.len();
    let mut elim = vec![false; n];
    if min_dist > 0.0 {
        let cell = min_dist;
        let key = |p: V3| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64);
        let mut grid: crate::dhash::HashMap<(i64, i64, i64), Vec<usize>> = crate::dhash::HashMap::default();
        for (i, p) in pos.iter().enumerate() {
            grid.entry(key(*p)).or_default().push(i);
        }
        let r2 = min_dist * min_dist;
        for i in 0..n {
            if elim[i] {
                continue;
            }
            let k = key(pos[i]);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(l) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                            for &j in l {
                                if j != i {
                                    let d = sub3(pos[j], pos[i]);
                                    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2 {
                                        elim[j] = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for i in 0..n {
        if elim[i] {
            continue;
        }
        let (c, f) = &tris[tri_idx[i]];
        let fct = |k: usize| if sel[*f] { factor[c[k]].max(0.0) } else { 0.0 };
        let w = bary[i];
        let prob = fct(0) * w[0] + fct(1) * w[1] + fct(2) * w[2];
        let h = super::noise::hf3(w[0], w[1], w[2]);
        if h > prob {
            elim[i] = true;
        }
    }
    for i in (0..n).rev() {
        if elim[i] {
            pos.swap_remove(i);
            bary.swap_remove(i);
            tri_idx.swap_remove(i);
        }
    }
    let mut pts = Points::default();
    let mut normals = vec![];
    let mut rots = vec![];
    let mut ids = vec![];
    for i in 0..pos.len() {
        let (_, f) = &tris[tri_idx[i]];
        pts.pos.push(pos[i]);
        normals.push(fnor[*f]);
        rots.push(normal_frame_quat(fnor[*f]));
        let w = bary[i];
        ids.push(Val::I(hash2(hash3(w[0].to_bits(), w[1].to_bits(), w[2].to_bits()), tri_idx[i] as u32) as i32));
    }
    pts.attrs.insert("id".into(), Attr { dom: Dom::Point, ty: Ty::I, data: ids });
    (pts, normals, rots)
}

/// Dijkstra from the end vertices over mesh edges: (next vertex towards the end, total cost); unreachable: (-1, 0)
pub fn shortest_paths(m: &Mesh, ends: &[bool], cost: &[f32]) -> (Vec<i32>, Vec<f32>) {
    let n = m.nv();
    let mut adj: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (i, e) in m.edges.iter().enumerate() {
        adj[e[0] as usize].push((e[1] as usize, i));
        adj[e[1] as usize].push((e[0] as usize, i));
    }
    let mut next = vec![-1i32; n];
    let mut total = vec![f32::MAX; n];
    // binary heap keyed by (cost, vertex) with total ordering on f32 bits
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u32, usize)>> = std::collections::BinaryHeap::new();
    let key = |c: f32| c.to_bits();
    for v in 0..n {
        if ends[v] {
            total[v] = 0.0;
            next[v] = v as i32;
            heap.push(std::cmp::Reverse((key(0.0), v)));
        }
    }
    while let Some(std::cmp::Reverse((k, v))) = heap.pop() {
        if f32::from_bits(k) > total[v] {
            continue;
        }
        for &(u, e) in &adj[v] {
            let nc = total[v] + cost[e];
            if nc < total[u] {
                total[u] = nc;
                next[u] = v as i32;
                heap.push(std::cmp::Reverse((key(nc), u)));
            }
        }
    }
    for v in 0..n {
        if total[v] == f32::MAX {
            total[v] = 0.0;
        }
    }
    (next, total)
}

/// hull_from_bullet (node_geo_convex_hull.cc) on top of the Bullet btConvexHullComputer port
pub fn hull_mesh(pts: &[V3]) -> Mesh {
    let h = super::bullet_hull::convex_hull_compute(pts);
    let nv = h.vertices.len();
    let mut m = Mesh::new();
    m.pos = h.vertices.clone();
    if nv <= 2 {
        if nv == 2 {
            m.edges.push([0, 1]);
        }
        return m;
    }
    let loops = h.edges.len();
    let mut corner_verts = vec![0u32; loops];
    let mut corner_edges = vec![0u32; loops];
    let mut edge_index = 0u32;
    for i in 0..loops {
        let (vf, vt) = (h.edges[i].source_vertex, h.edges[i].target_vertex);
        corner_verts[i] = vf as u32;
        if vf < vt {
            m.edges.push([vf as u32, vt as u32]);
            corner_edges[i] = edge_index;
            corner_edges[h.edges[i].reverse_edge as usize] = edge_index;
            edge_index += 1;
        }
    }
    m.foff = vec![0];
    for &f in &h.faces {
        let e0 = f as usize;
        let mut e = e0;
        let mut cnt = 0;
        loop {
            m.cv.push(corner_verts[e]);
            m.ce.push(corner_edges[e]);
            cnt += 1;
            e = h.edges[e].next_edge_of_face as usize;
            if e == e0 {
                break;
            }
        }
        let last = *m.foff.last().unwrap();
        m.foff.push(last + cnt);
    }
    super::prim::set_sharp_all(&mut m);
    m
}
