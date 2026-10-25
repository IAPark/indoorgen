//! Dual Mesh node (port of node_geo_dual_mesh.cc, Blender 4.2).
use super::geom::*;
use super::math::*;
use super::val::*;

const LOOSE: u8 = 0;
const NORMAL_V: u8 = 1;
const BOUNDARY_V: u8 = 2;
const NONMANIFOLD: u8 = 3;
// edge types: 0 loose, 1 boundary, 2 normal, 3 non-manifold
const E_BOUNDARY: u8 = 1;

fn edge_add_nb(t: u8) -> u8 {
    match t {
        0 => 1,
        1 => 2,
        _ => 3,
    }
}
fn vert_add_nb(t: u8) -> u8 {
    match t {
        0 => 1,
        1 => 2,
        _ => 3,
    }
}

struct Ctx<'a> {
    m: &'a Mesh,
    et: Vec<u8>,
}

impl<'a> Ctx<'a> {
    #[allow(clippy::too_many_arguments)]
    fn sort_vertex_faces(&self, vertex: usize, boundary_vertex: bool, cf: &mut Vec<usize>, shared: &mut Vec<usize>, sorted: &mut Vec<usize>) -> bool {
        let m = self.m;
        if cf.len() <= 2 && (!boundary_vertex || cf.is_empty()) {
            return true;
        }
        let ev = |e: usize| m.edges[e];
        let mut fvc: Vec<(usize, usize)> = vec![(0, 0); cf.len()];
        for i in 0..cf.len() {
            let mut first_done = false;
            for corner in m.face_range(cf[i]) {
                let edge = m.ce[corner] as usize;
                let [a, b] = ev(edge);
                if a as usize == vertex || b as usize == vertex {
                    if !first_done {
                        fvc[i].0 = corner;
                        first_done = true;
                    } else {
                        fvc[i].1 = corner;
                        break;
                    }
                }
            }
        }
        let cedge = |c: usize| m.ce[c] as usize;
        let cvert = |c: usize| m.cv[c] as usize;
        let mut shared_edge_i: i64 = -1;
        if boundary_vertex {
            for i in 0..cf.len() {
                let (c1, c2) = fvc[i];
                if self.et[cedge(c1)] == E_BOUNDARY && cvert(c1) == vertex {
                    shared_edge_i = cedge(c2) as i64;
                    sorted[0] = fvc[i].0;
                    cf.swap(i, 0);
                    fvc.swap(i, 0);
                    break;
                }
                if self.et[cedge(c2)] == E_BOUNDARY && cvert(c2) == vertex {
                    shared_edge_i = cedge(c1) as i64;
                    sorted[0] = fvc[i].1;
                    cf.swap(i, 0);
                    fvc.swap(i, 0);
                    break;
                }
            }
            if shared_edge_i == -1 {
                for i in 0..cf.len() {
                    let (c1, c2) = fvc[i];
                    if self.et[cedge(c1)] == E_BOUNDARY {
                        shared_edge_i = cedge(c2) as i64;
                        sorted[0] = fvc[i].0;
                        cf.swap(i, 0);
                        fvc.swap(i, 0);
                        break;
                    }
                    if self.et[cedge(c2)] == E_BOUNDARY {
                        shared_edge_i = cedge(c1) as i64;
                        sorted[0] = fvc[i].1;
                        cf.swap(i, 0);
                        fvc.swap(i, 0);
                        break;
                    }
                }
            }
        } else {
            let (c1, c2) = fvc[0];
            if cvert(c1) == vertex {
                shared_edge_i = cedge(c2) as i64;
                sorted[0] = fvc[0].0;
            } else {
                sorted[0] = fvc[0].1;
                shared_edge_i = cedge(c1) as i64;
            }
        }
        if shared_edge_i < 0 {
            return false;
        }
        let mut sh = shared_edge_i as usize;
        for i in 0..cf.len() - 1 {
            shared[i] = sh;
            let mut j = i + 1;
            while j < cf.len() {
                let (c1, c2) = fvc[j];
                if cedge(c1) == sh {
                    sorted[i + 1] = fvc[j].0;
                    sh = cedge(c2);
                    break;
                }
                if cedge(c2) == sh {
                    sorted[i + 1] = fvc[j].1;
                    sh = cedge(c1);
                    break;
                }
                j += 1;
            }
            if j == cf.len() {
                return false;
            }
            cf.swap(i + 1, j);
            fvc.swap(i + 1, j);
        }
        if !boundary_vertex {
            let l = shared.len() - 1;
            shared[l] = sh;
        }
        true
    }

    fn boundary_edge_on_face(&self, face: usize, vertex: usize) -> usize {
        let m = self.m;
        let mut r = 0;
        for c in m.face_range(face) {
            let e = m.ce[c] as usize;
            if self.et[e] == E_BOUNDARY {
                let [a, b] = m.edges[e];
                if a as usize == vertex || b as usize == vertex {
                    r = e;
                    return r;
                }
            }
        }
        r
    }

    fn boundary_edges_on_face(&self, face: usize, vertex: usize) -> (usize, usize) {
        let m = self.m;
        let mut edge1_done = false;
        let mut needs_swap = false;
        let (mut r1, mut r2) = (0usize, 0usize);
        for c in m.face_range(face) {
            let e = m.ce[c] as usize;
            if self.et[e] == E_BOUNDARY {
                let [a, b] = m.edges[e];
                if a as usize == vertex || b as usize == vertex {
                    if edge1_done {
                        if needs_swap {
                            r2 = r1;
                            r1 = e;
                        } else {
                            r2 = e;
                        }
                        return (r1, r2);
                    }
                    r1 = e;
                    edge1_done = true;
                    if m.cv[c] as usize == vertex {
                        needs_swap = true;
                    }
                }
            }
        }
        (r1, r2)
    }
}

pub fn dual_mesh(m: &Mesh, keep: bool) -> Mesh {
    let nv = m.pos.len();
    let ne = m.edges.len();
    let nf = m.nf();
    let mut vt = vec![LOOSE; nv];
    let mut et = vec![0u8; ne];
    for f in 0..nf {
        for c in m.face_range(f) {
            let e = m.ce[c] as usize;
            et[e] = edge_add_nb(et[e]);
        }
    }
    for i in 0..ne {
        let t = et[i];
        if t == 0 {
            continue;
        }
        let [a, b] = m.edges[i];
        let (a, b) = (a as usize, b as usize);
        if t == 1 {
            vt[a] = vert_add_nb(vt[a]);
            vt[b] = vert_add_nb(vt[b]);
        } else if t >= 3 {
            vt[a] = NONMANIFOLD;
            vt[b] = NONMANIFOLD;
        }
    }
    for i in 0..ne {
        if et[i] == 2 {
            let [a, b] = m.edges[i];
            if vt[a as usize] == LOOSE {
                vt[a as usize] = NORMAL_V;
            }
            if vt[b as usize] == LOOSE {
                vt[b as usize] = NORMAL_V;
            }
        }
    }
    let mut v2f: Vec<Vec<usize>> = vec![vec![]; nv];
    for f in 0..nf {
        for c in m.face_range(f) {
            v2f[m.cv[c] as usize].push(f);
        }
    }
    let cx = Ctx { m, et: et.clone() };
    let mut vshared: Vec<Vec<usize>> = vec![vec![]; nv];
    let mut vcorners: Vec<Vec<usize>> = vec![vec![]; nv];
    for i in 0..nv {
        if vt[i] == LOOSE || vt[i] >= NONMANIFOLD || (!keep && vt[i] == BOUNDARY_V) {
            continue;
        }
        let n = v2f[i].len();
        let mut sorted = vec![0usize; n];
        let ok;
        if vt[i] == NORMAL_V {
            let mut shared = vec![0usize; n];
            let mut cf = std::mem::take(&mut v2f[i]);
            ok = cx.sort_vertex_faces(i, false, &mut cf, &mut shared, &mut sorted);
            v2f[i] = cf;
            vshared[i] = shared;
        } else {
            let mut shared = vec![0usize; n.saturating_sub(1)];
            let mut cf = std::mem::take(&mut v2f[i]);
            ok = cx.sort_vertex_faces(i, true, &mut cf, &mut shared, &mut sorted);
            v2f[i] = cf;
            vshared[i] = shared;
        }
        if !ok {
            vt[i] = NONMANIFOLD;
            continue;
        }
        vcorners[i] = sorted;
    }
    // face centers
    let mut vpos: Vec<V3> = Vec::with_capacity(nf);
    for f in 0..nf {
        let r = m.face_range(f);
        let mut s = [0.0f32; 3];
        for c in r.clone() {
            let p = m.pos[m.cv[c] as usize];
            s = [s[0] + p[0], s[1] + p[1], s[2] + p[2]];
        }
        let k = 1.0f32 / r.len() as f32;
        vpos.push([s[0] * k, s[1] * k, s[2] * k]);
    }
    let mut bmid: Vec<usize> = vec![];
    if keep {
        bmid = vec![0; ne];
        for i in 0..ne {
            if et[i] == E_BOUNDARY {
                let [a, b] = m.edges[i];
                let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
                bmid[i] = vpos.len();
                vpos.push([(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5, (pa[2] + pb[2]) * 0.5]);
            }
        }
    }
    let mut face_sizes: Vec<usize> = vec![];
    let mut corner_verts: Vec<u32> = vec![];
    let mut corner_edges: Vec<u32> = vec![];
    let mut new_edges: Vec<[u32; 2]> = vec![];
    let mut new_to_old_corners: Vec<usize> = vec![];
    let mut new_to_old_edges: Vec<usize> = vec![];
    let mut bvert_face: Vec<(usize, usize)> = vec![];
    let mut old_to_new_edges: Vec<i64> = vec![-1; ne];
    // dissolve redundant verts
    for vi in 0..nv {
        if v2f[vi].len() != 2 || vt[vi] != NORMAL_V {
            continue;
        }
        let first = v2f[vi][0];
        let second = v2f[vi][1];
        let new_edge_index = new_edges.len();
        let mut edge_created = false;
        let needs = |vt: &Vec<u8>, v: usize| vt[v] != BOUNDARY_V && v2f[v].len() == 2 && v2f[v][0] == first && v2f[v][1] == second;
        for c in m.face_range(first) {
            let ei = m.ce[c] as usize;
            let [a, b] = m.edges[ei];
            let mut mark = false;
            if needs(&vt, a as usize) {
                vt[a as usize] = LOOSE;
                mark = true;
            }
            if needs(&vt, b as usize) {
                vt[b as usize] = LOOSE;
                mark = true;
            }
            if mark {
                if !edge_created {
                    new_to_old_edges.push(ei);
                    new_edges.push([first as u32, second as u32]);
                    edge_created = true;
                }
                old_to_new_edges[ei] = new_edge_index as i64;
            }
        }
    }
    for i in 0..nv {
        if vt[i] == LOOSE || vt[i] >= NONMANIFOLD || (!keep && vt[i] == BOUNDARY_V) {
            continue;
        }
        let mut corner_indices: Vec<usize> = v2f[i].clone();
        let shared = &vshared[i];
        let sorted = &vcorners[i];
        if vt[i] == NORMAL_V {
            if corner_indices.len() <= 2 {
                continue;
            }
            for (k, &old_edge) in shared.iter().enumerate() {
                if old_to_new_edges[old_edge] == -1 {
                    new_to_old_edges.push(old_edge);
                    old_to_new_edges[old_edge] = new_edges.len() as i64;
                    new_edges.push([corner_indices[k] as u32, corner_indices[(k + 1) % corner_indices.len()] as u32]);
                }
                corner_edges.push(old_to_new_edges[old_edge] as u32);
            }
            new_to_old_corners.extend(sorted.iter().copied());
        } else {
            for (k, &old_edge) in shared.iter().enumerate() {
                if old_to_new_edges[old_edge] == -1 {
                    new_to_old_edges.push(old_edge);
                    old_to_new_edges[old_edge] = new_edges.len() as i64;
                    new_edges.push([corner_indices[k] as u32, corner_indices[k + 1] as u32]);
                }
                corner_edges.push(old_to_new_edges[old_edge] as u32);
            }
            new_to_old_corners.extend(sorted.iter().copied());
            let (edge1, edge2);
            if corner_indices.len() >= 2 {
                edge1 = cx.boundary_edge_on_face(*corner_indices.last().unwrap(), i);
                edge2 = cx.boundary_edge_on_face(corner_indices[0], i);
            } else {
                let r = cx.boundary_edges_on_face(corner_indices[0], i);
                edge1 = r.0;
                edge2 = r.1;
            }
            let last_face_center = *corner_indices.last().unwrap();
            corner_indices.push(bmid[edge1]);
            new_to_old_corners.push(*sorted.last().unwrap());
            let first_midpoint = *corner_indices.last().unwrap();
            let mut add_edge = |old: usize, v1: usize, v2: usize, ne_: &mut Vec<[u32; 2]>, n2o: &mut Vec<usize>, ce: &mut Vec<u32>| {
                let idx = ne_.len();
                n2o.push(old);
                ne_.push([v1 as u32, v2 as u32]);
                ce.push(idx as u32);
            };
            if old_to_new_edges[edge1] == -1 {
                add_edge(edge1, last_face_center, first_midpoint, &mut new_edges, &mut new_to_old_edges, &mut corner_edges);
                old_to_new_edges[edge1] = new_edges.len() as i64 - 1;
                bvert_face.push((first_midpoint, last_face_center));
            } else {
                corner_edges.push(old_to_new_edges[edge1] as u32);
            }
            corner_indices.push(vpos.len());
            new_to_old_corners.push(sorted[0]);
            bvert_face.push((*corner_indices.last().unwrap(), last_face_center));
            vpos.push(m.pos[i]);
            let boundary_vertex = *corner_indices.last().unwrap();
            add_edge(edge1, first_midpoint, boundary_vertex, &mut new_edges, &mut new_to_old_edges, &mut corner_edges);
            corner_indices.push(bmid[edge2]);
            new_to_old_corners.push(sorted[0]);
            let second_midpoint = *corner_indices.last().unwrap();
            add_edge(edge2, boundary_vertex, second_midpoint, &mut new_edges, &mut new_to_old_edges, &mut corner_edges);
            if old_to_new_edges[edge2] == -1 {
                let first_face_center = corner_indices[0];
                add_edge(edge2, second_midpoint, first_face_center, &mut new_edges, &mut new_to_old_edges, &mut corner_edges);
                old_to_new_edges[edge2] = new_edges.len() as i64 - 1;
                bvert_face.push((second_midpoint, first_face_center));
            } else {
                corner_edges.push(old_to_new_edges[edge2] as u32);
            }
        }
        face_sizes.push(corner_indices.len());
        for j in corner_indices {
            corner_verts.push(j as u32);
        }
    }
    let mut o = Mesh::new();
    o.pos = vpos;
    o.edges = new_edges;
    o.cv = corner_verts;
    o.ce = corner_edges;
    o.foff = vec![0];
    let mut acc = 0u32;
    for s in &face_sizes {
        acc += *s as u32;
        o.foff.push(acc);
    }
    let nfo = face_sizes.len();
    let nvo = o.pos.len();
    // attributes
    for (name, a) in m.attrs.iter() {
        if name == "sharp_face" {
            continue;
        }
        let zero = Val::zero(a.ty);
        match a.dom {
            Dom::Point => {
                let mut data = vec![];
                for i in 0..nv {
                    if vt[i] == NORMAL_V || (keep && vt[i] == BOUNDARY_V) {
                        data.push(a.data[i].clone());
                    }
                }
                data.resize(nfo, zero);
                data.truncate(nfo);
                o.attrs.insert(name.clone(), Attr { dom: Dom::Face, ty: a.ty, data });
            }
            Dom::Face => {
                let mut data = a.data.clone();
                data.resize(nvo, zero);
                if keep {
                    for &(d, s) in &bvert_face {
                        data[d] = a.data[s].clone();
                    }
                }
                o.attrs.insert(name.clone(), Attr { dom: Dom::Point, ty: a.ty, data });
            }
            Dom::Edge => {
                o.attrs.insert(name.clone(), Attr { dom: Dom::Edge, ty: a.ty, data: new_to_old_edges.iter().map(|&e| a.data[e].clone()).collect() });
            }
            Dom::Corner => {
                o.attrs.insert(name.clone(), Attr { dom: Dom::Corner, ty: a.ty, data: new_to_old_corners.iter().map(|&c| a.data[c].clone()).collect() });
            }
            _ => {}
        }
    }
    if nfo > 0 {
        o.attrs.insert("sharp_face".into(), Attr { dom: Dom::Face, ty: Ty::B, data: vec![Val::B(true); nfo] });
    }
    o.mats = m.mats.clone();
    o
}
