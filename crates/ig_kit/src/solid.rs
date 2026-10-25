//! Closed polyhedra with faces that may carry holes + a "Bevel" (edge chamfer, WIDTH offset) for a set of edges,
//! implemented to be surface-equivalent to bmesh bevel with segments = 1 on (near-)orthogonal edges.
use crate::mesh::*;
use crate::dhash::HashMap;

pub type Loop = Vec<u32>;
/// face = outer loop followed by hole loops. Outer loops are CCW seen from outside (outward normal), holes CW.
pub type Face = Vec<Loop>;

#[derive(Clone, Default)]
pub struct Solid {
    pub pos: Vec<V3>,
    pub faces: Vec<Face>,
    /// optional per-face tag (empty = all 0); propagated by chamfer (strips take the tag of their first face, patches 0)
    pub tags: Vec<u8>,
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm3(a: V3) -> V3 {
    let l = dot3(a, a).sqrt();
    if l == 0.0 { a } else { mul3(a, 1.0 / l) }
}

fn dist3(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Solid {
    pub fn tag(&self, fi: usize) -> u8 {
        self.tags.get(fi).copied().unwrap_or(0)
    }

    pub fn add_vertex(&mut self, p: V3, map: &mut HashMap<[i64; 3], u32>) -> u32 {
        let k = [(p[0] * 1e7).round() as i64, (p[1] * 1e7).round() as i64, (p[2] * 1e7).round() as i64];
        if let Some(&i) = map.get(&k) {
            return i;
        }
        self.pos.push(p);
        let i = (self.pos.len() - 1) as u32;
        map.insert(k, i);
        i
    }

    pub fn from_mesh(m: &Mesh) -> Solid {
        Solid { pos: m.pos.clone(), faces: m.faces.iter().map(|f| vec![f.clone()]).collect(), tags: vec![] }
    }

    pub fn loop_normal(&self, l: &Loop) -> V3 {
        let mut n = [0.0; 3];
        for i in 0..l.len() {
            let (a, b) = (self.pos[l[i] as usize], self.pos[l[(i + 1) % l.len()] as usize]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        norm3(n)
    }
    pub fn face_normal(&self, f: &Face) -> V3 {
        self.loop_normal(&f[0])
    }

    /// bevel every edge whose adjacent face normals differ by more than `angle` (radians); `width` is the chamfer face width
    pub fn chamfer_sharp(&self, width: f64, angle: f64) -> Solid {
        let sel = self.sharp_edges(angle);
        self.chamfer(&sel, width)
    }

    pub fn sharp_edges(&self, angle: f64) -> Vec<(u32, u32)> {
        let mut em: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
        for (fi, f) in self.faces.iter().enumerate() {
            for l in f {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    em.entry((a.min(b), a.max(b))).or_default().push(fi);
                }
            }
        }
        let mut out = vec![];
        for (k, fs) in em {
            if fs.len() == 2 {
                let (n1, n2) = (self.face_normal(&self.faces[fs[0]]), self.face_normal(&self.faces[fs[1]]));
                if dot3(n1, n2).clamp(-1.0, 1.0).acos() > angle {
                    out.push(k);
                }
            }
        }
        out.sort();
        out
    }

    pub fn chamfer(&self, sel_edges: &[(u32, u32)], width: f64) -> Solid {
        let sel: crate::dhash::HashSet<(u32, u32)> = sel_edges.iter().cloned().collect();
        let key = |a: u32, b: u32| (a.min(b), a.max(b));
        // edge -> faces (face index, oriented a->b)
        let mut em: HashMap<(u32, u32), Vec<(usize, u32, u32)>> = HashMap::default();
        for (fi, f) in self.faces.iter().enumerate() {
            for l in f {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    em.entry(key(a, b)).or_default().push((fi, a, b));
                }
            }
        }
        // leg per selected edge
        let mut leg: HashMap<(u32, u32), f64> = HashMap::default();
        for &e in &sel {
            let fs = &em[&e];
            let (n1, n2) = (self.face_normal(&self.faces[fs[0].0]), self.face_normal(&self.faces[fs[1].0]));
            let tau = dot3(n1, n2).clamp(-1.0, 1.0).acos();
            leg.insert(e, width / (2.0 * (tau / 2.0).cos()));
        }
        let mut out = Solid::default();
        let mut vmap: HashMap<[i64; 3], u32> = HashMap::default();
        // pass 1: moved points on unselected edges
        // P[(v, other)] = position of edge (v,other)'s end at v
        let mut pmap: HashMap<(u32, u32), V3> = HashMap::default();
        struct Corner {
            f: usize,
            li: usize,
            i: usize,
            v: u32,
            u: u32,
            w: u32,
        }
        let mut corners: Vec<Corner> = vec![];
        for (fi, f) in self.faces.iter().enumerate() {
            for (li, l) in f.iter().enumerate() {
                for i in 0..l.len() {
                    corners.push(Corner { f: fi, li, i, v: l[i], u: l[(i + l.len() - 1) % l.len()], w: l[(i + 1) % l.len()] });
                }
            }
        }
        // inward directions within a face for edge a->b: n x d
        let inward = |fn_: V3, a: u32, b: u32| norm3(cross3(fn_, sub3(self.pos[b as usize], self.pos[a as usize])));
        let mut single: HashMap<(usize, usize, usize), V3> = HashMap::default();
        for c in &corners {
            let fnrm = self.face_normal(&self.faces[c.f]);
            let s_in = sel.contains(&key(c.u, c.v));
            let s_out = sel.contains(&key(c.v, c.w));
            let pv = self.pos[c.v as usize];
            let d_in = norm3(sub3(self.pos[c.u as usize], pv)); // away from v along e_in
            let d_out = norm3(sub3(self.pos[c.w as usize], pv));
            let n_in = inward(fnrm, c.u, c.v);
            let n_out = inward(fnrm, c.v, c.w);
            if s_in && s_out {
                let (l1, l2) = (leg[&key(c.u, c.v)], leg[&key(c.v, c.w)]);
                // p = pv + a*d_in + b*d_out ; n_in.(p-pv) = l1 ; n_out.(p-pv) = l2
                let (a11, a12, a21, a22) = (dot3(n_in, d_in), dot3(n_in, d_out), dot3(n_out, d_in), dot3(n_out, d_out));
                let det = a11 * a22 - a12 * a21;
                let p = if det.abs() < 1e-9 {
                    add3(pv, add3(mul3(n_in, l1), mul3(n_out, 0.0)))
                } else {
                    let a = (l1 * a22 - a12 * l2) / det;
                    let b = (a11 * l2 - a21 * l1) / det;
                    add3(pv, add3(mul3(d_in, a), mul3(d_out, b)))
                };
                single.insert((c.f, c.li, c.i), p);
            } else if s_in {
                let l1 = leg[&key(c.u, c.v)];
                let t = l1 / dot3(n_in, d_out);
                let elen = dist3(self.pos[c.w as usize], pv);
                let _ = elen;
                let mut p = add3(pv, mul3(d_out, t));
                let old = *pmap.entry((c.v, c.w)).or_insert(p);
                if sub3(old, p).iter().map(|x| x * x).sum::<f64>().sqrt() < 0.2 * l1 {
                    p = old;
                }
                single.insert((c.f, c.li, c.i), p);
            } else if s_out {
                let l2 = leg[&key(c.v, c.w)];
                let t = l2 / dot3(n_out, d_in);
                let elen = dist3(self.pos[c.u as usize], pv);
                let _ = elen;
                let mut p = add3(pv, mul3(d_in, t));
                let old = *pmap.entry((c.v, c.u)).or_insert(p);
                if sub3(old, p).iter().map(|x| x * x).sum::<f64>().sqrt() < 0.2 * l2 {
                    p = old;
                }
                single.insert((c.f, c.li, c.i), p);
            }
        }
        // new faces
        let mut new_faces: Vec<Face> = vec![];
        for (fi, f) in self.faces.iter().enumerate() {
            let mut nf: Face = vec![];
            for (li, l) in f.iter().enumerate() {
                let mut nl: Loop = vec![];
                for i in 0..l.len() {
                    let v = l[i];
                    let (u, w) = (l[(i + l.len() - 1) % l.len()], l[(i + 1) % l.len()]);
                    let mut pts: Vec<V3> = vec![];
                    if let Some(p) = single.get(&(fi, li, i)) {
                        pts.push(*p);
                    } else {
                        let pi = pmap.get(&(v, u)).cloned();
                        let po = pmap.get(&(v, w)).cloned();
                        match (pi, po) {
                            (None, None) => pts.push(self.pos[v as usize]),
                            (a, b) => {
                                pts.push(a.unwrap_or(self.pos[v as usize]));
                                pts.push(b.unwrap_or(self.pos[v as usize]));
                            }
                        }
                    }
                    for p in pts {
                        let id = out.add_vertex(p, &mut vmap);
                        if nl.last() != Some(&id) {
                            nl.push(id);
                        }
                    }
                }
                while nl.len() > 1 && nl[0] == *nl.last().unwrap() {
                    nl.pop();
                }
                nf.push(nl);
            }
            new_faces.push(nf);
        }
        out.faces = new_faces;
        let mut otags: Vec<u8> = (0..self.faces.len()).map(|i| self.tag(i)).collect();
        // strips
        for &e in &sel {
            let fs = &em[&e];
            let (f1, f2) = (fs[0], fs[1]);
            let corner_pt = |f: &(usize, u32, u32), v: u32| -> u32 {
                // find the single point of face f at vertex v
                for (li, l) in self.faces[f.0].iter().enumerate() {
                    for i in 0..l.len() {
                        if l[i] == v {
                            let (u, w) = (l[(i + l.len() - 1) % l.len()], l[(i + 1) % l.len()]);
                            if (key(u, v) == e) || (key(v, w) == e) {
                                let p = single[&(f.0, li, i)];
                                return out.add_vertex(p, &mut vmap.clone());
                            }
                        }
                    }
                }
                panic!("corner not found");
            };
            // vertices must come from the shared map: recompute through vmap lookup without insertion side effects
            let get = |p: V3, out: &mut Solid, vmap: &mut HashMap<[i64; 3], u32>| out.add_vertex(p, vmap);
            let _ = corner_pt;
            let single_pt = |f: &(usize, u32, u32), v: u32| -> V3 {
                for (li, l) in self.faces[f.0].iter().enumerate() {
                    for i in 0..l.len() {
                        if l[i] == v {
                            let (u, w) = (l[(i + l.len() - 1) % l.len()], l[(i + 1) % l.len()]);
                            if key(u, v) == e || key(v, w) == e {
                                return single[&(f.0, li, i)];
                            }
                        }
                    }
                }
                panic!("corner not found");
            };
            let (a, b) = (e.0, e.1);
            let a1 = get(single_pt(&f1, a), &mut out, &mut vmap);
            let b1 = get(single_pt(&f1, b), &mut out, &mut vmap);
            let a2 = get(single_pt(&f2, a), &mut out, &mut vmap);
            let b2 = get(single_pt(&f2, b), &mut out, &mut vmap);
            let nn = norm3(add3(self.face_normal(&self.faces[f1.0]), self.face_normal(&self.faces[f2.0])));
            let mut q: Vec<u32> = vec![a1, b1, b2, a2];
            q.dedup();
            while q.len() > 1 && q[0] == *q.last().unwrap() {
                q.pop();
            }
            if q.len() >= 3 {
                let tmp: Loop = q.clone();
                let n = out.loop_normal(&tmp);
                if dot3(n, nn) < 0.0 {
                    q.reverse();
                }
                out.faces.push(vec![q]);
                otags.push(self.tag(f1.0));
            }
        }
        // vertex patches where >= 3 selected edges meet
        let mut at_v: HashMap<u32, Vec<usize>> = HashMap::default();
        for (ci, c) in corners.iter().enumerate() {
            at_v.entry(c.v).or_default().push(ci);
        }
        for (v, cs) in at_v {
            let nsel = {
                let mut s = crate::dhash::HashSet::default();
                for &ci in &cs {
                    let c = &corners[ci];
                    if sel.contains(&key(c.u, c.v)) {
                        s.insert(key(c.u, c.v));
                    }
                    if sel.contains(&key(c.v, c.w)) {
                        s.insert(key(c.v, c.w));
                    }
                }
                s.len()
            };
            if nsel < 3 {
                continue;
            }
            // chain corners: next corner has e_in == this e_out
            let mut order: Vec<usize> = vec![cs[0]];
            loop {
                let c = &corners[*order.last().unwrap()];
                let nxt = cs.iter().find(|&&cj| corners[cj].u == c.w && corners[cj].v == v && !order.contains(&cj));
                match nxt {
                    Some(&cj) => order.push(cj),
                    None => break,
                }
            }
            if order.len() != cs.len() {
                continue;
            }
            let mut ids: Vec<u32> = vec![];
            let mut expect = [0.0; 3];
            for &ci in &order {
                let c = &corners[ci];
                expect = add3(expect, self.face_normal(&self.faces[c.f]));
                let key3 = (c.f, c.li, c.i);
                let p = if let Some(p) = single.get(&key3) { *p } else { self.pos[v as usize] };
                let id = out.add_vertex(p, &mut vmap);
                if ids.last() != Some(&id) {
                    ids.push(id);
                }
            }
            while ids.len() > 1 && ids[0] == *ids.last().unwrap() {
                ids.pop();
            }
            if ids.len() >= 3 {
                let n = out.loop_normal(&ids);
                if dot3(n, expect) < 0.0 {
                    ids.reverse();
                }
                out.faces.push(vec![ids]);
                otags.push(0);
            }
        }
        out.tags = otags;
        out
    }

    /// one compacted mesh per tag present
    pub fn split_by_tag(&self) -> Vec<(u8, Mesh)> {
        let mut tags: Vec<u8> = (0..self.faces.len()).map(|i| self.tag(i)).collect();
        tags.sort();
        tags.dedup();
        let mut out = vec![];
        for t in tags {
            let sub = Solid { pos: self.pos.clone(), faces: self.faces.iter().enumerate().filter(|(i, _)| self.tag(*i) == t).map(|(_, f)| f.clone()).collect(), tags: vec![] };
            let m = sub.to_mesh();
            let mut map = vec![u32::MAX; m.pos.len()];
            let mut o = Mesh::new();
            for f in &m.faces {
                let nf: Vec<u32> = f
                    .iter()
                    .map(|&v| {
                        if map[v as usize] == u32::MAX {
                            map[v as usize] = o.pos.len() as u32;
                            o.pos.push(m.pos[v as usize]);
                        }
                        map[v as usize]
                    })
                    .collect();
                o.faces.push(nf);
            }
            out.push((t, o));
        }
        out
    }

    /// polygon mesh (faces with holes are triangulated by ear clipping on their plane)
    pub fn to_mesh(&self) -> Mesh {
        let mut m = Mesh::new();
        m.pos = self.pos.clone();
        for f in &self.faces {
            if f.len() == 1 {
                m.faces.push(f[0].clone());
                continue;
            }
            let n = self.face_normal(f);
            let ax = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() { 0 } else if n[1].abs() >= n[2].abs() { 1 } else { 2 };
            let (i0, i1) = match ax { 0 => (1, 2), 1 => (2, 0), _ => (0, 1) };
            let mut flat: Vec<f64> = vec![];
            let mut ids: Vec<u32> = vec![];
            let mut holes: Vec<usize> = vec![];
            for (li, l) in f.iter().enumerate() {
                if li > 0 {
                    holes.push(ids.len());
                }
                for &v in l {
                    let p = self.pos[v as usize];
                    flat.push(p[i0]);
                    flat.push(p[i1]);
                    ids.push(v);
                }
            }
            let tris = earcutr::earcut(&flat, &holes, 2).unwrap_or_default();
            for t in tris.chunks(3) {
                let tri = vec![ids[t[0]], ids[t[1]], ids[t[2]]];
                let a = self.pos[tri[0] as usize];
                let b = self.pos[tri[1] as usize];
                let c = self.pos[tri[2] as usize];
                let tn = cross3(sub3(b, a), sub3(c, a));
                if dot3(tn, n) < 0.0 {
                    m.faces.push(vec![tri[0], tri[2], tri[1]]);
                } else {
                    m.faces.push(tri);
                }
            }
        }
        m
    }
}
