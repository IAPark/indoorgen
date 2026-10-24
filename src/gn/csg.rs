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

/// split `p` into the cells of the BSP tree (no classification)
fn split_cells(node: &Node, p: Poly, out: &mut Vec<Poly>, eps: f64) {
    let Some(pl) = node.plane else {
        out.push(p);
        return;
    };
    let (mut cf, mut cb, mut fr, mut bk) = (vec![], vec![], vec![], vec![]);
    split_polygon(&pl, &p, &mut cf, &mut cb, &mut fr, &mut bk, eps);
    fr.extend(cf);
    fr.extend(cb);
    for f in fr {
        match &node.front {
            Some(n) => split_cells(n, f, out, eps),
            None => out.push(f),
        }
    }
    for b in bk {
        match &node.back {
            Some(n) => split_cells(n, b, out, eps),
            None => out.push(b),
        }
    }
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
            // split p by the planes of the other solid's triangles that really cross it, then classify the cells
            let side = |pl: &Plane, q: &Poly| -> (bool, bool) {
                let (mut pos_, mut neg) = (false, false);
                for v in &q.v {
                    let d = dot(pl.n, *v) - pl.w;
                    if d > eps {
                        pos_ = true;
                    } else if d < -eps {
                        neg = true;
                    }
                }
                (pos_, neg)
            };
            let mut cand: Vec<Poly> = vec![];
            for (bi, (l, h)) in bb.iter().enumerate() {
                if !(0..3).all(|k| lo[k] <= h[k] + eps * 10.0 && hi[k] >= l[k] - eps * 10.0) {
                    continue;
                }
                let q = &b[bi];
                let (sp, sn) = side(&p.plane, q);
                if !(sp && sn) {
                    continue;
                }
                let (tp, tn) = side(&q.plane, p);
                if !(tp && tn) {
                    continue;
                }
                cand.push(q.clone());
            }
            let mut frags = vec![];
            if cand.is_empty() {
                frags.push(p.clone());
            } else {
                let local = Node::new(cand, eps);
                split_cells(&local, p.clone(), &mut frags, eps);
            }
            for f in frags {
                let n = f.v.len() as f64;
                let mut c = [0.0; 3];
                for v in &f.v {
                    for k in 0..3 {
                        c[k] += v[k] / n;
                    }
                }
                if point_outside(&tree, c, eps) {
                    out.push(f);
                } else {
                    inn.push(f);
                }
            }
        }
    }
    if std::env::var("GNDUMP").is_ok() {
        let t = a.iter().filter(|p| { let (lo, hi) = bbox(p); bb.iter().any(|(l, h)| (0..3).all(|k| lo[k] <= h[k] + eps * 10.0 && hi[k] >= l[k] - eps * 10.0)) }).count();
        eprintln!("classify: {} polys, {} touching -> out {} in {}", a.len(), t, out.len(), inn.len());
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
    let tolw = 2e-7 * scale.max(1e-3);
    let q = 1.0 / tolw;
    let mut map: HashMap<[i64; 3], Vec<u32>> = HashMap::default();
    let mut pos: Vec<V3> = vec![];
    let mut posd: Vec<[f64; 3]> = vec![];
    let mut faces: Vec<Vec<u32>> = vec![];
    let mut src: Vec<(u8, u32)> = vec![];
    for p in &result {
        let ids: Vec<u32> = p
            .v
            .iter()
            .map(|v| {
                let c = [(v[0] * q).floor() as i64, (v[1] * q).floor() as i64, (v[2] * q).floor() as i64];
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            if let Some(list) = map.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                                for &id in list {
                                    let w = posd[id as usize];
                                    if (w[0] - v[0]).abs() <= tolw && (w[1] - v[1]).abs() <= tolw && (w[2] - v[2]).abs() <= tolw {
                                        return id;
                                    }
                                }
                            }
                        }
                    }
                }
                pos.push([v[0] as f32, v[1] as f32, v[2] as f32]);
                posd.push(*v);
                let id = pos.len() as u32 - 1;
                map.entry(c).or_default().push(id);
                id
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
    if std::env::var("GNDUMP").is_ok() {
        eprintln!("csg {}: A {} verts {} faces, others {:?}, result tris {} verts {}", op, first.nv(), first.nf(), others.iter().map(|o| (o.nv(), o.nf())).collect::<Vec<_>>(), faces.len(), pos.len());
    }
    let faces = if std::env::var("GNNOMERGE").is_ok() { faces } else { resolve_tjunctions(&pos, faces, scale) };
    let (faces, src) = if std::env::var("GNNOMERGE").is_ok() { (faces, src) } else { merge_coplanar(&pos, faces, src) };
    if std::env::var("GNDUMP").is_ok() {
        eprintln!("  merged faces {}", faces.len());
    }
    let (pos, faces) = compact(pos, faces);
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

fn compact(pos: Vec<V3>, faces: Vec<Vec<u32>>) -> (Vec<V3>, Vec<Vec<u32>>) {
    let mut map = vec![u32::MAX; pos.len()];
    let mut np = vec![];
    for f in &faces {
        for &v in f {
            if map[v as usize] == u32::MAX {
                map[v as usize] = 0;
            }
        }
    }
    for (i, p) in pos.iter().enumerate() {
        if map[i] != u32::MAX {
            map[i] = np.len() as u32;
            np.push(*p);
        }
    }
    (np, faces.into_iter().map(|f| f.into_iter().map(|v| map[v as usize]).collect()).collect())
}

/// Merge edge-adjacent coplanar triangles with the same source face into polygons; vertices that are collinear in every
/// polygon they belong to are dropped (removes the T-junction vertices the BSP leaves behind). Regions with holes stay triangles.
pub fn merge_coplanar(pos: &[V3], tris: Vec<Vec<u32>>, src: Vec<(u8, u32)>) -> (Vec<Vec<u32>>, Vec<(u8, u32)>) {
    let n = tris.len();
    let p64 = |i: u32| {
        let q = pos[i as usize];
        [q[0] as f64, q[1] as f64, q[2] as f64]
    };
    let normals: Vec<P3> = tris
        .iter()
        .map(|t| {
            let c = cross(sub(p64(t[1]), p64(t[0])), sub(p64(t[2]), p64(t[0])));
            let l = dot(c, c).sqrt();
            if l > 0.0 { [c[0] / l, c[1] / l, c[2] / l] } else { c }
        })
        .collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let nx = p[c];
            p[c] = r;
            c = nx;
        }
        r
    }
    let mut emap: HashMap<(u32, u32), usize> = HashMap::default();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            emap.insert((t[k], t[(k + 1) % 3]), i);
        }
    }
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            if let Some(&j) = emap.get(&(t[(k + 1) % 3], t[k])) {
                if src[i] == src[j] {
                    let d = sub(normals[i], normals[j]);
                    if dot(d, d).sqrt() < 1e-5 {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        parent[a] = b;
                    }
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::default();
    let mut order = vec![];
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_insert_with(|| {
            order.push(r);
            vec![]
        }).push(i);
    }
    let mut needed = vec![false; pos.len()];
    // (loop, src) for merged groups; triangles for the rest
    let mut loops: Vec<(Vec<u32>, (u8, u32))> = vec![];
    let mut keep_tris: Vec<usize> = vec![];
    let mut tri_out: Vec<([u32; 3], (u8, u32))> = vec![];
    for r in &order {
        let g = &groups[r];
        if g.len() == 1 {
            keep_tris.push(g[0]);
            for &v in &tris[g[0]] {
                needed[v as usize] = true;
            }
            continue;
        }
        let mut set: HashMap<(u32, u32), ()> = HashMap::default();
        for &i in g {
            for k in 0..3 {
                set.insert((tris[i][k], tris[i][(k + 1) % 3]), ());
            }
        }
        let mut next: HashMap<u32, Vec<u32>> = HashMap::default();
        for &(a, b) in set.keys() {
            if !set.contains_key(&(b, a)) {
                next.entry(a).or_default().push(b);
            }
        }
        // boundary loops
        let simple = next.values().all(|v| v.len() == 1);
        let mut gl: Vec<Vec<u32>> = vec![];
        if simple {
            let mut seen: HashMap<u32, ()> = HashMap::default();
            let mut starts: Vec<u32> = next.keys().copied().collect();
            starts.sort();
            for st in starts {
                if seen.contains_key(&st) {
                    continue;
                }
                let mut lp = vec![st];
                seen.insert(st, ());
                let mut cur = next[&st][0];
                let mut guard = 0;
                while cur != st && guard < 100000 {
                    lp.push(cur);
                    seen.insert(cur, ());
                    cur = match next.get(&cur) {
                        Some(v) => v[0],
                        None => break,
                    };
                    guard += 1;
                }
                if cur != st {
                    gl.clear();
                    break;
                }
                gl.push(lp);
            }
        }
        let keep_group = |keep_tris: &mut Vec<usize>, needed: &mut Vec<bool>| {
            for &i in g {
                keep_tris.push(i);
                for &v in &tris[i] {
                    needed[v as usize] = true;
                }
            }
        };
        if gl.is_empty() || gl.iter().any(|l| l.len() < 3) {
            keep_group(&mut keep_tris, &mut needed);
            continue;
        }
        // drop collinear vertices (they stay only if some polygon needs them)
        let mut gn = normals[g[0]];
        let _ = &mut gn;
        for lp in &gl {
            let m = lp.len();
            for k in 0..m {
                let (a, b, c) = (p64(lp[(k + m - 1) % m]), p64(lp[k]), p64(lp[(k + 1) % m]));
                let (u, v) = (sub(b, a), sub(c, b));
                let cr = cross(u, v);
                if dot(cr, cr).sqrt() > 1e-5 * dot(u, u).sqrt() * dot(v, v).sqrt() {
                    needed[lp[k] as usize] = true;
                }
            }
        }
        let signed_area = |lp: &Vec<u32>| -> f64 {
            let mut nn = [0.0f64; 3];
            for k in 0..lp.len() {
                let (a, b) = (p64(lp[k]), p64(lp[(k + 1) % lp.len()]));
                let c = cross(a, b);
                for q in 0..3 {
                    nn[q] += c[q];
                }
            }
            dot(nn, gn) * 0.5
        };
        let outers: Vec<usize> = (0..gl.len()).filter(|&k| signed_area(&gl[k]) > 0.0).collect();
        if gl.len() == 1 {
            loops.push((gl[0].clone(), src[g[0]]));
        } else if outers.len() == 1 {
            // one outer loop with holes: earcut in the plane
            let o = outers[0];
            let ax = (0..3).max_by(|&a, &b| gn[a].abs().partial_cmp(&gn[b].abs()).unwrap()).unwrap();
            let (u, w) = ((ax + 1) % 3, (ax + 2) % 3);
            let mut ids: Vec<u32> = vec![];
            let mut data: Vec<f64> = vec![];
            let mut holes: Vec<usize> = vec![];
            let order_loops: Vec<usize> = std::iter::once(o).chain((0..gl.len()).filter(|&k| k != o)).collect();
            for (n_, &k) in order_loops.iter().enumerate() {
                if n_ > 0 {
                    holes.push(ids.len());
                }
                let lp: Vec<u32> = gl[k].iter().copied().filter(|&v| needed[v as usize]).collect();
                // earcut expects the outer ring CCW in its (u, w) frame; flip the frame sign if the normal axis is negative
                let flip = gn[ax] < 0.0;
                for &v in &lp {
                    let q = p64(v);
                    data.push(q[u]);
                    data.push(if flip { -q[w] } else { q[w] });
                    ids.push(v);
                }
            }
            let tri = earcutr::earcut(&data, &holes, 2).unwrap_or_default();
            if tri.is_empty() {
                keep_group(&mut keep_tris, &mut needed);
                continue;
            }
            let _ = ();
            for t in tri.chunks(3) {
                tri_out.push(([ids[t[0]], ids[t[1]], ids[t[2]]], src[g[0]]));
            }
        } else {
            keep_group(&mut keep_tris, &mut needed);
            continue;
        }
    }
    let mut faces = vec![];
    let mut osrc = vec![];
    for (lp, s) in loops {
        let f: Vec<u32> = lp.into_iter().filter(|&v| needed[v as usize]).collect();
        if f.len() >= 3 {
            faces.push(f);
            osrc.push(s);
        }
    }
    for (t, sr) in tri_out {
        faces.push(t.to_vec());
        osrc.push(sr);
    }
    for i in keep_tris {
        faces.push(tris[i].clone());
        osrc.push(src[i]);
    }
    (faces, osrc)
}


/// insert into every face edge the vertices lying on it (so adjacent fragments share identical edges)
pub fn resolve_tjunctions(pos: &[V3], faces: Vec<Vec<u32>>, scale: f64) -> Vec<Vec<u32>> {
    if pos.is_empty() {
        return faces;
    }
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f64);
            hi[k] = hi[k].max(p[k] as f64);
        }
    }
    let ext = (0..3).map(|k| hi[k] - lo[k]).fold(1e-9, f64::max);
    let cell = (ext / ((pos.len() as f64).cbrt() * 1.5 + 1.0)).max(1e-9);
    let key = |p: [f64; 3]| [((p[0] - lo[0]) / cell).floor() as i64, ((p[1] - lo[1]) / cell).floor() as i64, ((p[2] - lo[2]) / cell).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::default();
    for (i, p) in pos.iter().enumerate() {
        grid.entry(key([p[0] as f64, p[1] as f64, p[2] as f64])).or_default().push(i as u32);
    }
    let tol = 1e-6 * scale.max(1e-6);
    let mut out = Vec::with_capacity(faces.len());
    for f in faces {
        let n = f.len();
        let mut nf: Vec<u32> = vec![];
        for k in 0..n {
            let (a, b) = (f[k], f[(k + 1) % n]);
            nf.push(a);
            let (pa, pb) = ([pos[a as usize][0] as f64, pos[a as usize][1] as f64, pos[a as usize][2] as f64], [pos[b as usize][0] as f64, pos[b as usize][1] as f64, pos[b as usize][2] as f64]);
            let (ka, kb) = (key(pa), key(pb));
            let (mut cl, mut ch) = ([0i64; 3], [0i64; 3]);
            for q in 0..3 {
                cl[q] = ka[q].min(kb[q]) - 0;
                ch[q] = ka[q].max(kb[q]) + 0;
            }
            let ncells = (0..3).map(|q| (ch[q] - cl[q] + 1) as i64).product::<i64>();
            if ncells > 4000 {
                continue;
            }
            let d = sub(pb, pa);
            let l2 = dot(d, d);
            if l2 < 1e-30 {
                continue;
            }
            let mut found: Vec<(f64, u32)> = vec![];
            for x in cl[0] - 1..=ch[0] + 1 {
                for y in cl[1] - 1..=ch[1] + 1 {
                    for z in cl[2] - 1..=ch[2] + 1 {
                        if let Some(vs) = grid.get(&[x, y, z]) {
                            for &v in vs {
                                if v == a || v == b {
                                    continue;
                                }
                                let pv = [pos[v as usize][0] as f64, pos[v as usize][1] as f64, pos[v as usize][2] as f64];
                                let t = dot(sub(pv, pa), d) / l2;
                                if t <= 1e-9 || t >= 1.0 - 1e-9 {
                                    continue;
                                }
                                let proj = [pa[0] + d[0] * t, pa[1] + d[1] * t, pa[2] + d[2] * t];
                                let e = sub(pv, proj);
                                if dot(e, e).sqrt() <= tol {
                                    found.push((t, v));
                                }
                            }
                        }
                    }
                }
            }
            found.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            for (_, v) in found {
                nf.push(v);
            }
        }
        out.push(nf);
    }
    out
}
