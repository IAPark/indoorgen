//! Mesh boolean via BSP-tree CSG (csg.js algorithm) in f64. Output is triangle soup welded into a mesh.
//! Surface-equal to Blender's exact boolean for closed manifold inputs; vertex/face order is not Blender's.
use super::geom::*;
use super::val::*;
use crate::dhash::HashMap;

const EPS: f64 = 1e-9;

type P3 = [f64; 3];

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[derive(Clone, Copy)]
struct Plane {
    n: P3,
    w: f64,
}

#[derive(Clone)]
struct Poly {
    v: Vec<P3>,
    plane: Plane,
    /// (mesh id, face index) of the source face
    src: (u8, u32),
}

impl Poly {
    fn new(v: Vec<P3>, src: (u8, u32)) -> Option<Poly> {
        let n = cross(sub(v[1], v[0]), sub(v[2], v[0]));
        let l = dot(n, n).sqrt();
        if l < 1e-18 {
            return None;
        }
        let n = [n[0] / l, n[1] / l, n[2] / l];
        Some(Poly { plane: Plane { n, w: dot(n, v[0]) }, v, src })
    }
    fn flip(&mut self) {
        self.v.reverse();
        self.plane.n = [-self.plane.n[0], -self.plane.n[1], -self.plane.n[2]];
        self.plane.w = -self.plane.w;
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

fn split_polygon(pl: &Plane, poly: &Poly, cf: &mut Vec<Poly>, cb: &mut Vec<Poly>, f: &mut Vec<Poly>, b: &mut Vec<Poly>, eps: f64) {
    let mut ptype = 0u8;
    let mut types = vec![];
    for v in &poly.v {
        let t = dot(pl.n, *v) - pl.w;
        let ty = if t < -eps {
            BACK
        } else if t > eps {
            FRONT
        } else {
            COPLANAR
        };
        ptype |= ty;
        types.push(ty);
    }
    match ptype {
        COPLANAR => {
            if dot(pl.n, poly.plane.n) > 0.0 {
                cf.push(poly.clone());
            } else {
                cb.push(poly.clone());
            }
        }
        FRONT => f.push(poly.clone()),
        BACK => b.push(poly.clone()),
        _ => {
            let mut fv = vec![];
            let mut bv = vec![];
            let n = poly.v.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.v[i], poly.v[j]);
                if ti != BACK {
                    fv.push(vi);
                }
                if ti != FRONT {
                    bv.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (pl.w - dot(pl.n, vi)) / dot(pl.n, sub(vj, vi));
                    let vv = lerp(vi, vj, t);
                    fv.push(vv);
                    bv.push(vv);
                }
            }
            if fv.len() >= 3 {
                f.push(Poly { v: fv, plane: poly.plane, src: poly.src });
            }
            if bv.len() >= 3 {
                b.push(Poly { v: bv, plane: poly.plane, src: poly.src });
            }
        }
    }
}

struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<Poly>,
}

impl Node {
    fn new(polys: Vec<Poly>, eps: f64) -> Node {
        let mut n = Node { plane: None, front: None, back: None, polys: vec![] };
        if !polys.is_empty() {
            n.build(polys, eps);
        }
        n
    }
    fn invert(&mut self) {
        for p in self.polys.iter_mut() {
            p.flip();
        }
        if let Some(pl) = self.plane.as_mut() {
            pl.n = [-pl.n[0], -pl.n[1], -pl.n[2]];
            pl.w = -pl.w;
        }
        if let Some(f) = self.front.as_mut() {
            f.invert();
        }
        if let Some(b) = self.back.as_mut() {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip_polygons(&self, polys: Vec<Poly>, eps: f64) -> Vec<Poly> {
        let Some(pl) = self.plane else { return polys };
        let mut front = vec![];
        let mut back = vec![];
        for p in &polys {
            let (mut cf, mut cb) = (vec![], vec![]);
            split_polygon(&pl, p, &mut cf, &mut cb, &mut front, &mut back, eps);
            front.extend(cf);
            back.extend(cb);
        }
        let mut front = match &self.front {
            Some(f) => f.clip_polygons(front, eps),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back, eps),
            None => vec![],
        };
        front.extend(back);
        front
    }
    fn clip_to(&mut self, bsp: &Node, eps: f64) {
        let p = std::mem::take(&mut self.polys);
        self.polys = bsp.clip_polygons(p, eps);
        if let Some(f) = self.front.as_mut() {
            f.clip_to(bsp, eps);
        }
        if let Some(b) = self.back.as_mut() {
            b.clip_to(bsp, eps);
        }
    }
    fn all_polygons(&self) -> Vec<Poly> {
        let mut p = self.polys.clone();
        if let Some(f) = &self.front {
            p.extend(f.all_polygons());
        }
        if let Some(b) = &self.back {
            p.extend(b.all_polygons());
        }
        p
    }
    fn build(&mut self, polys: Vec<Poly>, eps: f64) {
        if polys.is_empty() {
            return;
        }
        let pl = *self.plane.get_or_insert(polys[0].plane);
        let mut front = vec![];
        let mut back = vec![];
        let mut coplanar_f = vec![];
        let mut coplanar_b = vec![];
        for p in &polys {
            split_polygon(&pl, p, &mut coplanar_f, &mut coplanar_b, &mut front, &mut back, eps);
        }
        self.polys.extend(coplanar_f);
        self.polys.extend(coplanar_b);
        if !front.is_empty() {
            self.front.get_or_insert_with(|| Box::new(Node { plane: None, front: None, back: None, polys: vec![] })).build(front, eps);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(|| Box::new(Node { plane: None, front: None, back: None, polys: vec![] })).build(back, eps);
        }
    }
}

fn csg_union(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let mut na = Node::new(a, eps);
    let mut nb = Node::new(b, eps);
    na.clip_to(&nb, eps);
    nb.clip_to(&na, eps);
    nb.invert();
    nb.clip_to(&na, eps);
    nb.invert();
    na.build(nb.all_polygons(), eps);
    na.all_polygons()
}

fn csg_subtract(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let mut na = Node::new(a, eps);
    let mut nb = Node::new(b, eps);
    na.invert();
    na.clip_to(&nb, eps);
    nb.clip_to(&na, eps);
    nb.invert();
    nb.clip_to(&na, eps);
    nb.invert();
    na.build(nb.all_polygons(), eps);
    na.invert();
    na.all_polygons()
}

fn csg_intersect(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let mut na = Node::new(a, eps);
    let mut nb = Node::new(b, eps);
    na.invert();
    nb.clip_to(&na, eps);
    nb.invert();
    na.clip_to(&nb, eps);
    nb.clip_to(&na, eps);
    na.build(nb.all_polygons(), eps);
    na.invert();
    na.all_polygons()
}

fn bbox(p: &Poly) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for v in &p.v {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (lo, hi)
}

fn point_outside(node: &Node, p: P3, eps: f64) -> bool {
    let Some(pl) = node.plane else { return true };
    let d = dot(pl.n, p) - pl.w;
    if d >= -eps {
        match &node.front {
            Some(f) => point_outside(f, p, eps),
            None => true,
        }
    } else {
        match &node.back {
            Some(b) => point_outside(b, p, eps),
            None => false,
        }
    }
}

fn flipped(ps: &[Poly]) -> Vec<Poly> {
    ps.iter()
        .map(|p| {
            let mut q = p.clone();
            q.flip();
            q
        })
        .collect()
}

/// Boolean of two polygon soups that only splits polygons which touch the other solid:
/// returns (parts of a outside b, parts of a inside b)
fn classify(a: &[Poly], b: &[Poly], eps: f64) -> (Vec<Poly>, Vec<Poly>) {
    let bb: Vec<([f64; 3], [f64; 3])> = b.iter().map(bbox).collect();
    let tree = Node::new(b.to_vec(), eps);
    let inv = Node::new(flipped(b), eps);
    let (mut out, mut inn) = (vec![], vec![]);
    for p in a {
        let (lo, hi) = bbox(p);
        let touches = bb.iter().any(|(l, h)| (0..3).all(|k| lo[k] <= h[k] + eps * 10.0 && hi[k] >= l[k] - eps * 10.0));
        if !touches {
            let c = {
                let n = p.v.len() as f64;
                let mut s = [0.0; 3];
                for v in &p.v {
                    for k in 0..3 {
                        s[k] += v[k] / n;
                    }
                }
                s
            };
            if point_outside(&tree, c, eps) {
                out.push(p.clone());
            } else {
                inn.push(p.clone());
            }
        } else {
            out.extend(tree.clip_polygons(vec![p.clone()], eps));
            inn.extend(inv.clip_polygons(vec![p.clone()], eps));
        }
    }
    (out, inn)
}

fn bin_union(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let (ao, _) = classify(&a, &b, eps);
    let (bo, _) = classify(&b, &a, eps);
    let mut r = ao;
    r.extend(bo);
    r
}

fn bin_intersect(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let (_, ai) = classify(&a, &b, eps);
    let (_, bi) = classify(&b, &a, eps);
    let mut r = ai;
    r.extend(bi);
    r
}

fn bin_subtract(a: Vec<Poly>, b: Vec<Poly>, eps: f64) -> Vec<Poly> {
    let (ao, _) = classify(&a, &b, eps);
    let (_, bi) = classify(&b, &a, eps);
    let mut r = ao;
    r.extend(flipped(&bi));
    r
}

fn to_polys(m: &Mesh, id: u8) -> Vec<Poly> {
    let mut out = vec![];
    for f in 0..m.nf() {
        let fv = m.face(f);
        for k in 1..fv.len().saturating_sub(1) {
            let p = |i: u32| {
                let q = m.pos[i as usize];
                [q[0] as f64, q[1] as f64, q[2] as f64]
            };
            if let Some(pl) = Poly::new(vec![p(fv[0]), p(fv[k]), p(fv[k + 1])], (id, f as u32)) {
                out.push(pl);
            }
        }
    }
    out
}

/// op: "UNION" | "INTERSECT" | "DIFFERENCE"
pub fn mesh_boolean(op: &str, first: &Mesh, others: &[&Mesh]) -> Mesh {
    let mut all = first.pos.clone();
    for o in others {
        all.extend(o.pos.iter().cloned());
    }
    let scale = all.iter().fold(1e-9f32, |m, p| m.max(p[0].abs()).max(p[1].abs()).max(p[2].abs())) as f64;
    let eps = (EPS * scale).max(1e-12) * 10.0;
    let a = to_polys(first, 0);
    let mut result;
    match op {
        "DIFFERENCE" => {
            // subtract the union of all other meshes
            let mut u: Vec<Poly> = vec![];
            for (i, o) in others.iter().enumerate() {
                let p = to_polys(o, (i + 1) as u8);
                u = if u.is_empty() { p } else { bin_union(u, p, eps) };
            }
            result = if u.is_empty() { a } else { bin_subtract(a, u, eps) };
        }
        "INTERSECT" => {
            result = a;
            for (i, o) in others.iter().enumerate() {
                result = bin_intersect(result, to_polys(o, (i + 1) as u8), eps);
            }
        }
        _ => {
            result = a;
            for (i, o) in others.iter().enumerate() {
                result = bin_union(result, to_polys(o, (i + 1) as u8), eps);
            }
        }
    }
    // weld vertices and triangulate (fan) the convex fragments
    let q = 1.0 / (1e-7 * scale.max(1e-3));
    let mut map: HashMap<[i64; 3], u32> = HashMap::default();
    let mut pos: Vec<V3> = vec![];
    let mut faces: Vec<Vec<u32>> = vec![];
    let mut src: Vec<(u8, u32)> = vec![];
    for p in &result {
        let ids: Vec<u32> = p
            .v
            .iter()
            .map(|v| {
                let key = [(v[0] * q).round() as i64, (v[1] * q).round() as i64, (v[2] * q).round() as i64];
                *map.entry(key).or_insert_with(|| {
                    pos.push([v[0] as f32, v[1] as f32, v[2] as f32]);
                    pos.len() as u32 - 1
                })
            })
            .collect();
        let mut ids = ids;
        ids.dedup();
        while ids.len() > 1 && ids[0] == *ids.last().unwrap() {
            ids.pop();
        }
        for k in 1..ids.len().saturating_sub(1) {
            if ids[0] != ids[k] && ids[k] != ids[k + 1] && ids[0] != ids[k + 1] {
                faces.push(vec![ids[0], ids[k], ids[k + 1]]);
                src.push(p.src);
            }
        }
    }
    let mut m = Mesh::from_faces(pos, &faces, &[]);
    // face attribute propagation from the source faces
    let metas: Vec<&Mesh> = std::iter::once(first).chain(others.iter().copied()).collect();
    let mut names: Vec<String> = vec![];
    for mm in &metas {
        for n in mm.attrs.keys() {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
    }
    for n in names {
        let mut ty = Ty::B;
        let mut dom = Dom::Face;
        for mm in &metas {
            if let Some(a) = mm.attrs.get(&n) {
                ty = a.ty;
                dom = a.dom;
            }
        }
        if dom != Dom::Face {
            continue;
        }
        let data: Vec<Val> = src
            .iter()
            .map(|&(id, f)| match metas[id as usize].attrs.get(&n) {
                Some(a) => a.data[f as usize].clone(),
                None => Val::zero(ty),
            })
            .collect();
        m.attrs.insert(n, Attr { dom: Dom::Face, ty, data });
    }
    m.mats = first.mats.clone();
    m
}
