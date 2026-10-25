//! Mesh boolean of two closed polygon meshes in generic position, producing what Blender's EXACT solver produces up to vertex order:
//! every input face is triangulated (quads along the `is_quad_flip_first_third` diagonal), tri-tri intersection segments split each
//! triangle into the faces of its planar arrangement, faces are kept or dropped by inside/outside tests, and the kept pieces of one input
//! face are merged back (diagonals dissolved) into one polygon per connected piece.
//! Not handled (Blender's exact arithmetic is): coplanar overlapping faces, vertices exactly on the other surface, shared edges.
//! Callers with symmetric inputs should perturb one operand by a hair (see assets/pangeom.rs bars).
use crate::dhash::{HashMap, HashSet};
use crate::mesh::Mesh;

type V3 = [f64; 3];

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum K {
    /// vertex of mesh m
    V(u8, u32),
    /// edge (lo, hi) of mesh m pierces triangle `tri` of the other mesh
    E(u8, u32, u32, u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Difference,
    Union,
    Intersect,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

struct Side<'a> {
    m: &'a Mesh,
    tris: Vec<[u32; 3]>,
    tri_face: Vec<u32>,
    diag: HashSet<(u32, u32)>,
}

fn triangulate(m: &Mesh) -> Side<'_> {
    let mut tris = vec![];
    let mut tri_face = vec![];
    let mut diag: HashSet<(u32, u32)> = HashSet::default();
    for (fi, f) in m.faces.iter().enumerate() {
        match f.len() {
            3 => {
                tris.push([f[0], f[1], f[2]]);
                tri_face.push(fi as u32);
            }
            4 => {
                let p = |i: usize| m.pos[f[i] as usize];
                let (d12, d13, d14) = (sub(p(1), p(0)), sub(p(2), p(0)), sub(p(3), p(0)));
                let flip = dot(cross(d12, d13), cross(d14, d13)) > 0.0;
                if flip {
                    diag.insert((f[1].min(f[3]), f[1].max(f[3])));
                    tris.push([f[0], f[1], f[3]]);
                    tris.push([f[1], f[2], f[3]]);
                } else {
                    diag.insert((f[0].min(f[2]), f[0].max(f[2])));
                    tris.push([f[0], f[1], f[2]]);
                    tris.push([f[0], f[2], f[3]]);
                }
                tri_face.push(fi as u32);
                tri_face.push(fi as u32);
            }
            n if n > 4 => {
                for i in 1..n - 1 {
                    tris.push([f[0], f[i], f[i + 1]]);
                    tri_face.push(fi as u32);
                    if i > 1 {
                        diag.insert((f[0].min(f[i]), f[0].max(f[i])));
                    }
                }
            }
            _ => {}
        }
    }
    Side { m, tris, tri_face, diag }
}

impl<'a> Side<'a> {
    fn tp(&self, t: usize) -> [V3; 3] {
        let v = self.tris[t];
        [self.m.pos[v[0] as usize], self.m.pos[v[1] as usize], self.m.pos[v[2] as usize]]
    }
}

/// 3-axis uniform grid over triangle bounding boxes
struct Grid {
    cell: f64,
    map: HashMap<(i32, i32, i32), Vec<u32>>,
}

impl Grid {
    fn build(s: &Side) -> Grid {
        let mut ext = 0.0f64;
        for t in 0..s.tris.len() {
            let p = s.tp(t);
            for k in 0..3 {
                let (lo, hi) = (p[0][k].min(p[1][k]).min(p[2][k]), p[0][k].max(p[1][k]).max(p[2][k]));
                ext = ext.max(hi - lo);
            }
        }
        let cell = (ext * 1.5).max(1e-9);
        let mut g = Grid { cell, map: HashMap::default() };
        for t in 0..s.tris.len() {
            let (lo, hi) = bbox(&s.tp(t));
            for c in g.cells(lo, hi) {
                g.map.entry(c).or_default().push(t as u32);
            }
        }
        g
    }
    fn cells(&self, lo: V3, hi: V3) -> Vec<(i32, i32, i32)> {
        let f = |x: f64| (x / self.cell).floor() as i32;
        let mut v = vec![];
        for i in f(lo[0])..=f(hi[0]) {
            for j in f(lo[1])..=f(hi[1]) {
                for k in f(lo[2])..=f(hi[2]) {
                    v.push((i, j, k));
                }
            }
        }
        v
    }
}

fn bbox(p: &[V3; 3]) -> (V3, V3) {
    let mut lo = p[0];
    let mut hi = p[0];
    for q in &p[1..] {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    (lo, hi)
}

/// endpoints of the intersection segment of two triangles: (mesh of the piercing edge, edge index, position along the line)
fn tri_tri(a: &[V3; 3], b: &[V3; 3]) -> Option<[(u8, usize, f64); 2]> {
    let nb = cross(sub(b[1], b[0]), sub(b[2], b[0]));
    let na = cross(sub(a[1], a[0]), sub(a[2], a[0]));
    let da = [dot(nb, sub(a[0], b[0])), dot(nb, sub(a[1], b[0])), dot(nb, sub(a[2], b[0]))];
    let db = [dot(na, sub(b[0], a[0])), dot(na, sub(b[1], a[0])), dot(na, sub(b[2], a[0]))];
    let pos = |d: &[f64; 3]| [d[0] >= 0.0, d[1] >= 0.0, d[2] >= 0.0];
    let (sa, sb) = (pos(&da), pos(&db));
    if sa[0] == sa[1] && sa[1] == sa[2] {
        return None;
    }
    if sb[0] == sb[1] && sb[1] == sb[2] {
        return None;
    }
    let dir = cross(na, nb);
    let along = |p: V3| dot(p, dir);
    let crossings = |t: &[V3; 3], d: &[f64; 3], s: &[bool; 3], mesh: u8| -> Vec<(u8, usize, f64)> {
        let mut v = vec![];
        for k in 0..3 {
            let k1 = (k + 1) % 3;
            if s[k] != s[k1] {
                let tt = d[k] / (d[k] - d[k1]);
                let p = [t[k][0] + (t[k1][0] - t[k][0]) * tt, t[k][1] + (t[k1][1] - t[k][1]) * tt, t[k][2] + (t[k1][2] - t[k][2]) * tt];
                v.push((mesh, k, along(p)));
            }
        }
        v
    };
    let ca = crossings(a, &da, &sa, 0);
    let cb = crossings(b, &db, &sb, 1);
    if ca.len() != 2 || cb.len() != 2 {
        return None;
    }
    let (amin, amax) = if ca[0].2 <= ca[1].2 { (ca[0], ca[1]) } else { (ca[1], ca[0]) };
    let (bmin, bmax) = if cb[0].2 <= cb[1].2 { (cb[0], cb[1]) } else { (cb[1], cb[0]) };
    let lo = if amin.2 >= bmin.2 { amin } else { bmin };
    let hi = if amax.2 <= bmax.2 { amax } else { bmax };
    if lo.2 >= hi.2 {
        return None;
    }
    Some([lo, hi])
}

struct Ctx<'a> {
    s: [Side<'a>; 2],
    pos: HashMap<K, V3>,
}

impl<'a> Ctx<'a> {
    fn key_pos(&mut self, k: K) -> V3 {
        if let Some(&p) = self.pos.get(&k) {
            return p;
        }
        let p = match k {
            K::V(m, v) => self.s[m as usize].m.pos[v as usize],
            K::E(m, lo, hi, tri) => {
                let (me, ot) = (&self.s[m as usize], &self.s[1 - m as usize]);
                let (p, q) = (me.m.pos[lo as usize], me.m.pos[hi as usize]);
                let b = ot.tp(tri as usize);
                let nb = cross(sub(b[1], b[0]), sub(b[2], b[0]));
                let (dp, dq) = (dot(nb, sub(p, b[0])), dot(nb, sub(q, b[0])));
                let t = dp / (dp - dq);
                let r = [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t];
                [r[0] as f32 as f64, r[1] as f32 as f64, r[2] as f32 as f64]
            }
        };
        self.pos.insert(k, p);
        p
    }
}

fn edge_key(m: u8, side: &Side, tri: usize, k: usize, other_tri: usize) -> K {
    let v = side.tris[tri];
    let (u, w) = (v[k], v[(k + 1) % 3]);
    K::E(m, u.min(w), u.max(w), other_tri as u32)
}

/// arrangement faces (as key loops in the triangle's winding) of triangle `t` of side `m` cut by `segs`
fn subdivide_tri(cx: &mut Ctx, m: u8, t: usize, segs: &[(K, K)]) -> Vec<Vec<K>> {
    let tri = cx.s[m as usize].tris[t];
    let corners = [K::V(m, tri[0]), K::V(m, tri[1]), K::V(m, tri[2])];
    let cp = [cx.key_pos(corners[0]), cx.key_pos(corners[1]), cx.key_pos(corners[2])];
    let n = cross(sub(cp[1], cp[0]), sub(cp[2], cp[0]));
    let ax = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() {
        0
    } else if n[1].abs() >= n[2].abs() {
        1
    } else {
        2
    };
    let (a1, a2) = ((ax + 1) % 3, (ax + 2) % 3);
    // 2D with the triangle counter-clockwise
    let mut proj = |cx: &mut Ctx, k: K| -> [f64; 2] {
        let p = cx.key_pos(k);
        [p[a1], p[a2]]
    };
    let c2 = [proj(cx, corners[0]), proj(cx, corners[1]), proj(cx, corners[2])];
    let area = (c2[1][0] - c2[0][0]) * (c2[2][1] - c2[0][1]) - (c2[1][1] - c2[0][1]) * (c2[2][0] - c2[0][0]);
    let mir = if area < 0.0 { -1.0 } else { 1.0 };
    let mut p2: HashMap<K, [f64; 2]> = HashMap::default();
    let mut nodes: Vec<K> = vec![];
    let mut add = |cx: &mut Ctx, k: K, nodes: &mut Vec<K>, p2: &mut HashMap<K, [f64; 2]>| {
        if !p2.contains_key(&k) {
            let q = proj(cx, k);
            p2.insert(k, [q[0] * mir, q[1]]);
            nodes.push(k);
        }
    };
    for &c in &corners {
        add(cx, c, &mut nodes, &mut p2);
    }
    for &(a, b) in segs {
        add(cx, a, &mut nodes, &mut p2);
        add(cx, b, &mut nodes, &mut p2);
    }
    // boundary chain
    let mut edges: Vec<(K, K)> = vec![];
    let mut chain: Vec<K> = vec![];
    for k in 0..3 {
        let (u, w) = (tri[k], tri[(k + 1) % 3]);
        let (lo, hi) = (u.min(w), u.max(w));
        let mut on: Vec<(f64, K)> = nodes
            .iter()
            .filter_map(|&q| match q {
                K::E(mm, l, h, _) if mm == m && l == lo && h == hi => {
                    let p = p2[&q];
                    let (s, e) = (p2[&corners[k]], p2[&corners[(k + 1) % 3]]);
                    let d = [e[0] - s[0], e[1] - s[1]];
                    Some((((p[0] - s[0]) * d[0] + (p[1] - s[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]), q))
                }
                _ => None,
            })
            .collect();
        on.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        chain.push(corners[k]);
        chain.extend(on.into_iter().map(|x| x.1));
    }
    for i in 0..chain.len() {
        edges.push((chain[i], chain[(i + 1) % chain.len()]));
    }
    let mut und: HashSet<(K, K)> = HashSet::default();
    for &(a, b) in segs {
        if a == b {
            continue;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        if und.insert(key) {
            edges.push((a, b));
        }
    }
    // adjacency (undirected)
    let mut adj: HashMap<K, Vec<K>> = HashMap::default();
    let mut seen: HashSet<(K, K)> = HashSet::default();
    for &(a, b) in &edges {
        let key = if a < b { (a, b) } else { (b, a) };
        if !seen.insert(key) {
            continue;
        }
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    // prune dangling nodes
    loop {
        let dead: Vec<K> = adj.iter().filter(|(_, v)| v.len() < 2).map(|(k, _)| *k).collect();
        if dead.is_empty() {
            break;
        }
        for d in dead {
            if let Some(nb) = adj.remove(&d) {
                for u in nb {
                    if let Some(l) = adj.get_mut(&u) {
                        l.retain(|&x| x != d);
                    }
                }
            }
        }
    }
    // sort neighbours by angle
    let ang = |a: K, b: K| -> f64 {
        let (pa, pb) = (p2[&a], p2[&b]);
        (pb[1] - pa[1]).atan2(pb[0] - pa[0])
    };
    let mut order: HashMap<K, Vec<K>> = HashMap::default();
    for (&k, v) in &adj {
        let mut l = v.clone();
        l.sort_by(|&x, &y| ang(k, x).partial_cmp(&ang(k, y)).unwrap());
        order.insert(k, l);
    }
    let mut visited: HashSet<(K, K)> = HashSet::default();
    let mut faces = vec![];
    let mut starts: Vec<(K, K)> = vec![];
    for (&k, v) in &order {
        for &w in v {
            starts.push((k, w));
        }
    }
    starts.sort();
    for (s0, s1) in starts {
        if visited.contains(&(s0, s1)) {
            continue;
        }
        let mut lp = vec![];
        let (mut u, mut v) = (s0, s1);
        let mut guard = 0;
        loop {
            visited.insert((u, v));
            lp.push(u);
            // next: neighbour of v that is the first clockwise from the direction v->u
            let nb = &order[&v];
            let a_back = ang(v, u);
            let i = nb.iter().position(|&x| x == u).unwrap();
            let _ = a_back;
            // nb sorted ascending by angle: the clockwise neighbour of u is the previous one (cyclic)
            let w = nb[(i + nb.len() - 1) % nb.len()];
            u = v;
            v = w;
            guard += 1;
            if (u, v) == (s0, s1) || guard > 10_000 {
                break;
            }
        }
        let mut a = 0.0;
        for i in 0..lp.len() {
            let (p, q) = (p2[&lp[i]], p2[&lp[(i + 1) % lp.len()]]);
            a += p[0] * q[1] - q[0] * p[1];
        }
        if a > 1e-24 {
            faces.push(lp);
        }
    }
    faces
}

/// ray (+x) parity test against a mesh, with a (y, z) grid over its triangles
struct RayGrid {
    cell: f64,
    map: HashMap<(i32, i32), Vec<u32>>,
    lo: V3,
    hi: V3,
}

impl RayGrid {
    fn build(s: &Side) -> RayGrid {
        let mut ext = 0.0f64;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in 0..s.tris.len() {
            let p = s.tp(t);
            let (l, h) = bbox(&p);
            ext = ext.max(h[1] - l[1]).max(h[2] - l[2]);
            for k in 0..3 {
                lo[k] = lo[k].min(l[k]);
                hi[k] = hi[k].max(h[k]);
            }
        }
        let cell = (ext * 2.0).max(1e-9);
        let mut g = RayGrid { cell, map: HashMap::default(), lo, hi };
        for t in 0..s.tris.len() {
            let (l, h) = bbox(&s.tp(t));
            let f = |x: f64| (x / cell).floor() as i32;
            for j in f(l[1])..=f(h[1]) {
                for k in f(l[2])..=f(h[2]) {
                    g.map.entry((j, k)).or_default().push(t as u32);
                }
            }
        }
        g
    }
    fn inside(&self, s: &Side, p: V3) -> bool {
        if p[0] > self.hi[0] || p[1] < self.lo[1] || p[1] > self.hi[1] || p[2] < self.lo[2] || p[2] > self.hi[2] {
            return false;
        }
        let (py, pz) = (p[1] + 1.37e-9, p[2] + 0.83e-9);
        let c = ((py / self.cell).floor() as i32, (pz / self.cell).floor() as i32);
        let mut count = 0;
        if let Some(l) = self.map.get(&c) {
            for &t in l {
                let q = s.tp(t as usize);
                // barycentric in (y, z)
                let d = (q[1][1] - q[0][1]) * (q[2][2] - q[0][2]) - (q[2][1] - q[0][1]) * (q[1][2] - q[0][2]);
                if d == 0.0 {
                    continue;
                }
                let u = ((py - q[0][1]) * (q[2][2] - q[0][2]) - (q[2][1] - q[0][1]) * (pz - q[0][2])) / d;
                let v = ((q[1][1] - q[0][1]) * (pz - q[0][2]) - (py - q[0][1]) * (q[1][2] - q[0][2])) / d;
                if u < 0.0 || v < 0.0 || u + v >= 1.0 {
                    continue;
                }
                let x = q[0][0] + u * (q[1][0] - q[0][0]) + v * (q[2][0] - q[0][0]);
                if x > p[0] {
                    count += 1;
                }
            }
        }
        count % 2 == 1
    }
}

fn trace_keys(edges: Vec<(K, K)>) -> Vec<Vec<K>> {
    let mut set: HashSet<(K, K)> = HashSet::default();
    for &e in &edges {
        set.insert(e);
    }
    let edges: Vec<(K, K)> = edges.into_iter().filter(|&(a, b)| !set.contains(&(b, a))).collect();
    let mut next: HashMap<K, Vec<usize>> = HashMap::default();
    for (i, e) in edges.iter().enumerate() {
        next.entry(e.0).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut loops = vec![];
    for s in 0..edges.len() {
        if used[s] {
            continue;
        }
        let mut lp = vec![edges[s].0];
        let mut cur = s;
        loop {
            used[cur] = true;
            let to = edges[cur].1;
            if to == edges[s].0 {
                break;
            }
            lp.push(to);
            match next.get(&to).and_then(|v| v.iter().find(|&&i| !used[i])) {
                Some(&n) => cur = n,
                None => break,
            }
        }
        loops.push(lp);
    }
    loops
}

pub fn boolean(op: Op, a: &Mesh, b: &Mesh) -> Mesh {
    let sa = triangulate(a);
    let sb = triangulate(b);
    let grid = Grid::build(&sb);
    let mut cx = Ctx { s: [sa, sb], pos: HashMap::default() };
    // intersection segments per triangle
    let mut segs: [HashMap<u32, Vec<(K, K)>>; 2] = [HashMap::default(), HashMap::default()];
    {
        let mut stamp = vec![u32::MAX; cx.s[1].tris.len()];
        for ta in 0..cx.s[0].tris.len() {
            let pa = cx.s[0].tp(ta);
            let (lo, hi) = bbox(&pa);
            for c in grid.cells(lo, hi) {
                if let Some(l) = grid.map.get(&c) {
                    for &tb in l {
                        if stamp[tb as usize] == ta as u32 {
                            continue;
                        }
                        stamp[tb as usize] = ta as u32;
                        let pb = cx.s[1].tp(tb as usize);
                        let (l2, h2) = bbox(&pb);
                        if (0..3).any(|k| l2[k] > hi[k] || h2[k] < lo[k]) {
                            continue;
                        }
                        if let Some(ends) = tri_tt(&pa, &pb) {
                            let mut ks = [K::V(0, 0); 2];
                            for (i, &(mesh, k, _)) in ends.iter().enumerate() {
                                ks[i] = if mesh == 0 { edge_key(0, &cx.s[0], ta, k, tb as usize) } else { edge_key(1, &cx.s[1], tb as usize, k, ta) };
                            }
                            if ks[0] != ks[1] {
                                segs[0].entry(ta as u32).or_default().push((ks[0], ks[1]));
                                segs[1].entry(tb).or_default().push((ks[0], ks[1]));
                            }
                        }
                    }
                }
            }
        }
    }
    let rg = [RayGrid::build(&cx.s[0]), RayGrid::build(&cx.s[1])];
    let mut vclass: HashMap<K, bool> = HashMap::default();
    // per original face: kept edges
    let mut kept: [HashMap<u32, Vec<(K, K)>>; 2] = [HashMap::default(), HashMap::default()];
    let want_inside = |m: usize| match op {
        Op::Difference => m == 1,
        Op::Union => false,
        Op::Intersect => true,
    };
    for m in 0..2usize {
        let nt = cx.s[m].tris.len();
        for t in 0..nt {
            let faces: Vec<Vec<K>> = match segs[m].get(&(t as u32)) {
                Some(sg) => {
                    let sg = sg.clone();
                    subdivide_tri(&mut cx, m as u8, t, &sg)
                }
                None => {
                    let v = cx.s[m].tris[t];
                    vec![vec![K::V(m as u8, v[0]), K::V(m as u8, v[1]), K::V(m as u8, v[2])]]
                }
            };
            for f in faces {
                // class
                let corner = f.iter().find(|k| matches!(k, K::V(..))).cloned();
                let inside = match corner {
                    Some(k) => {
                        if let Some(&c) = vclass.get(&k) {
                            c
                        } else {
                            let p = cx.key_pos(k);
                            let c = rg[1 - m].inside(&cx.s[1 - m], p);
                            vclass.insert(k, c);
                            c
                        }
                    }
                    None => {
                        let ps: Vec<V3> = f.iter().map(|&k| cx.key_pos(k)).collect();
                        let c = interior_point(&ps);
                        rg[1 - m].inside(&cx.s[1 - m], c)
                    }
                };
                if inside == want_inside(m) {
                    let face = cx.s[m].tri_face[t];
                    let l = kept[m].entry(face).or_default();
                    for i in 0..f.len() {
                        l.push((f[i], f[(i + 1) % f.len()]));
                    }
                }
            }
        }
    }
    // output loops
    let mut loops: Vec<(usize, Vec<K>)> = vec![];
    for m in 0..2usize {
        let mut faces: Vec<u32> = kept[m].keys().cloned().collect();
        faces.sort();
        for fi in faces {
            let edges = kept[m].remove(&fi).unwrap();
            for mut lp in trace_keys(edges) {
                if lp.len() < 3 {
                    continue;
                }
                if op == Op::Difference && m == 1 {
                    lp.reverse();
                }
                loops.push((m, lp));
            }
        }
    }
    // find_dissolve_verts: non-input vertices of valence 2 (the same two neighbours in every face) that are straight within cos 1e-4
    let mut nbrs: HashMap<K, (K, K)> = HashMap::default();
    let mut bad: HashSet<K> = HashSet::default();
    for (_, lp) in &loops {
        let n = lp.len();
        for i in 0..n {
            let k = lp[i];
            if matches!(k, K::V(..)) {
                continue;
            }
            let (a, b) = (lp[(i + n - 1) % n], lp[(i + 1) % n]);
            match nbrs.get(&k) {
                Some(&(x, y)) => {
                    if !((a == y && b == x) || (a == x && b == y)) {
                        bad.insert(k);
                    }
                }
                None => {
                    nbrs.insert(k, (a, b));
                }
            }
        }
    }
    let mut dissolved: HashSet<K> = HashSet::default();
    let mut order: Vec<K> = nbrs.keys().cloned().collect();
    order.sort();
    for k in order {
        if bad.contains(&k) {
            continue;
        }
        let (a, b) = nbrs[&k];
        let (pa, pk, pb) = (cx.key_pos(a), cx.key_pos(k), cx.key_pos(b));
        let (v1, v2) = (sub(pk, pa), sub(pb, pk));
        let (l1, l2) = (dot(v1, v1).sqrt(), dot(v2, v2).sqrt());
        if l1 == 0.0 || l2 == 0.0 {
            continue;
        }
        if (dot(v1, v2) / (l1 * l2) - 1.0).abs() < 1e-4 {
            dissolved.insert(k);
        }
    }
    // assemble
    let mut out = Mesh::new();
    let mut idx: HashMap<K, u32> = HashMap::default();
    for (_, lp) in loops {
        let mut f = vec![];
        for k in lp {
            if dissolved.contains(&k) {
                continue;
            }
            let i = match idx.get(&k) {
                Some(&i) => i,
                None => {
                    let p = cx.key_pos(k);
                    out.pos.push(p);
                    idx.insert(k, (out.pos.len() - 1) as u32);
                    (out.pos.len() - 1) as u32
                }
            };
            f.push(i);
        }
        if f.len() >= 3 {
            out.faces.push(f);
        }
    }
    out
}

fn tri_tt(a: &[V3; 3], b: &[V3; 3]) -> Option<[(u8, usize, f64); 2]> {
    tri_tri(a, b)
}

/// a point strictly inside the polygon: centroid of the largest ear
fn interior_point(p: &[V3]) -> V3 {
    let n = p.len();
    let mut best = (-1.0, [0.0; 3]);
    for i in 0..n {
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let ar = {
            let x = cross(sub(b, a), sub(c, a));
            dot(x, x).sqrt()
        };
        // convex corner test against the polygon normal
        let mut nrm = [0.0; 3];
        for j in 0..n {
            let (u, v) = (p[j], p[(j + 1) % n]);
            nrm[0] += (u[1] - v[1]) * (u[2] + v[2]);
            nrm[1] += (u[2] - v[2]) * (u[0] + v[0]);
            nrm[2] += (u[0] - v[0]) * (u[1] + v[1]);
        }
        let x = cross(sub(b, a), sub(c, b));
        if dot(x, nrm) <= 0.0 {
            continue;
        }
        // no other vertex inside the ear
        let mut ok = true;
        for j in 0..n {
            if j == i || j == (i + n - 1) % n || j == (i + 1) % n {
                continue;
            }
            let q = p[j];
            let s0 = dot(cross(sub(b, a), sub(q, a)), nrm);
            let s1 = dot(cross(sub(c, b), sub(q, b)), nrm);
            let s2 = dot(cross(sub(a, c), sub(q, c)), nrm);
            if s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0 {
                ok = false;
                break;
            }
        }
        if ok && ar > best.0 {
            best = (ar, [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0]);
        }
    }
    if best.0 < 0.0 {
        let mut c = [0.0; 3];
        for q in p {
            for k in 0..3 {
                c[k] += q[k] / n as f64;
            }
        }
        return c;
    }
    best.1
}

/// connected components by shared vertices
pub fn components(m: &Mesh) -> Vec<Mesh> {
    let n = m.pos.len();
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
    for f in &m.faces {
        for w in f.windows(2) {
            let (a, b) = (find(&mut parent, w[0] as usize), find(&mut parent, w[1] as usize));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        let r = find(&mut parent, f[0] as usize);
        groups.entry(r).or_default().push(fi);
    }
    let mut keys: Vec<usize> = groups.keys().cloned().collect();
    keys.sort_by_key(|k| groups[k][0]);
    keys.into_iter()
        .map(|k| {
            let mut o = Mesh::new();
            let mut map: HashMap<u32, u32> = HashMap::default();
            for &fi in &groups[&k] {
                let f: Vec<u32> = m.faces[fi]
                    .iter()
                    .map(|&v| {
                        *map.entry(v).or_insert_with(|| {
                            o.pos.push(m.pos[v as usize]);
                            (o.pos.len() - 1) as u32
                        })
                    })
                    .collect();
                o.faces.push(f);
            }
            o
        })
        .collect()
}
