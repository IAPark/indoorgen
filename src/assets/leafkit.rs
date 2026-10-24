//! Shared bpy-style mesh toolkit for plant leaves (infinigen assets/utils/draw.py `leaf`, `remesh_fill`, decorate.py helpers):
//! float32 polygon mesh with explicit edges, WELD, bmesh vertex delete, separate loose, SOLIDIFY (offset -1), SampleNearestSurface transfer,
//! the sharp REMESH (dualcon), fill of a closed planar outline, distance-to-boundary.
use super::cabnodes::GMesh;
use super::stagefile::trace;
use crate::dhash::HashMap;

pub type F3 = [f32; 3];

#[derive(Clone, Default, Debug)]
pub struct LMesh {
    pub v: Vec<F3>,
    pub f: Vec<Vec<u32>>,
    pub e: Vec<[u32; 2]>,
}

impl LMesh {
    pub fn to_g(&self) -> GMesh {
        GMesh { v: self.v.clone(), p: self.f.clone(), tag: vec![0; self.f.len()], e: self.e.clone() }
    }
    pub fn from_g(g: GMesh) -> LMesh {
        LMesh { v: g.v, f: g.p, e: g.e }
    }
    pub fn trace(&self, kind: &str) {
        trace(kind, &self.v, &self.f, &self.e);
    }
    /// edges of the faces in order of first appearance, appended after the explicit ones that are not yet present
    pub fn ensure_face_edges(&mut self) {
        let mut seen: HashMap<(u32, u32), ()> = HashMap::default();
        for e in &self.e {
            seen.insert((e[0].min(e[1]), e[0].max(e[1])), ());
        }
        for fc in &self.f {
            let n = fc.len();
            for i in 0..n {
                let (a, b) = (fc[i], fc[(i + 1) % n]);
                let k = (a.min(b), a.max(b));
                if seen.insert(k, ()).is_none() {
                    self.e.push([a, b]);
                }
            }
        }
    }
    pub fn bounds(&self) -> (F3, F3) {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &self.v {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
    /// obj.dimensions (float32)
    pub fn dimensions(&self) -> F3 {
        let (lo, hi) = self.bounds();
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }
    pub fn weld(&self, th: f64) -> LMesh {
        LMesh::from_g(super::gweld::merge_by_distance(&self.to_g(), th as f32))
    }
    pub fn join(parts: &[&LMesh]) -> LMesh {
        let mut o = LMesh::default();
        for p in parts {
            let off = o.v.len() as u32;
            o.v.extend_from_slice(&p.v);
            for fc in &p.f {
                o.f.push(fc.iter().map(|&i| i + off).collect());
            }
            for e in &p.e {
                o.e.push([e[0] + off, e[1] + off]);
            }
        }
        o
    }
    /// bmesh.ops.delete(geom=verts): faces and edges touching a deleted vertex go; remaining order is kept
    pub fn delete_verts(&self, del: &[bool]) -> LMesh {
        let mut map = vec![u32::MAX; self.v.len()];
        let mut o = LMesh::default();
        for (i, p) in self.v.iter().enumerate() {
            if !del[i] {
                map[i] = o.v.len() as u32;
                o.v.push(*p);
            }
        }
        for fc in &self.f {
            if fc.iter().all(|&i| !del[i as usize]) {
                o.f.push(fc.iter().map(|&i| map[i as usize]).collect());
            }
        }
        for e in &self.e {
            if !del[e[0] as usize] && !del[e[1] as usize] {
                o.e.push([map[e[0] as usize], map[e[1] as usize]]);
            }
        }
        o
    }
    /// bmesh.ops.delete(context=EDGES): the edges and their faces go, then endpoint vertices left without edges
    pub fn delete_edges(&self, del: &[bool]) -> LMesh {
        let key = |a: u32, b: u32| (a.min(b), a.max(b));
        let mut dead: HashMap<(u32, u32), ()> = HashMap::default();
        for (i, e) in self.e.iter().enumerate() {
            if del[i] {
                dead.insert(key(e[0], e[1]), ());
            }
        }
        let mut o = LMesh::default();
        let mut tagged = vec![false; self.v.len()];
        for (i, e) in self.e.iter().enumerate() {
            if del[i] {
                tagged[e[0] as usize] = true;
                tagged[e[1] as usize] = true;
            }
        }
        let mut deg = vec![0u32; self.v.len()];
        let mut ekeep: Vec<[u32; 2]> = vec![];
        for (i, e) in self.e.iter().enumerate() {
            if !del[i] {
                ekeep.push(*e);
                deg[e[0] as usize] += 1;
                deg[e[1] as usize] += 1;
            }
        }
        let mut fkeep: Vec<&Vec<u32>> = vec![];
        for fc in &self.f {
            let k = fc.len();
            if (0..k).all(|i| !dead.contains_key(&key(fc[i], fc[(i + 1) % k]))) {
                fkeep.push(fc);
            }
        }
        let mut map = vec![u32::MAX; self.v.len()];
        for (i, p) in self.v.iter().enumerate() {
            if tagged[i] && deg[i] == 0 {
                continue;
            }
            map[i] = o.v.len() as u32;
            o.v.push(*p);
        }
        for fc in fkeep {
            o.f.push(fc.iter().map(|&i| map[i as usize]).collect());
        }
        for e in ekeep {
            o.e.push([map[e[0] as usize], map[e[1] as usize]]);
        }
        o
    }
    /// keep only the listed vertices (order kept); faces and edges fully inside survive
    pub fn keep_verts(&self, keep: &[bool]) -> LMesh {
        let del: Vec<bool> = keep.iter().map(|k| !k).collect();
        self.delete_verts(&del)
    }
    /// butil.split_object + argmax over [original (= last component), pieces in creation order]: the component with the most vertices
    pub fn largest_component(&self) -> LMesh {
        let n = self.v.len();
        let mut uf: Vec<u32> = (0..n as u32).collect();
        fn find(uf: &mut Vec<u32>, mut x: u32) -> u32 {
            while uf[x as usize] != x {
                uf[x as usize] = uf[uf[x as usize] as usize];
                x = uf[x as usize];
            }
            x
        }
        let mut union = |uf: &mut Vec<u32>, a: u32, b: u32| {
            let (ra, rb) = (find(uf, a), find(uf, b));
            if ra != rb {
                uf[ra.max(rb) as usize] = ra.min(rb);
            }
        };
        for fc in &self.f {
            for k in 1..fc.len() {
                union(&mut uf, fc[0], fc[k]);
            }
        }
        for e in &self.e {
            union(&mut uf, e[0], e[1]);
        }
        // components ordered by lowest vertex index (root = min index thanks to the union direction)
        let mut roots: Vec<u32> = vec![];
        let mut count: HashMap<u32, usize> = HashMap::default();
        for i in 0..n as u32 {
            let r = find(&mut uf, i);
            if !count.contains_key(&r) {
                roots.push(r);
            }
            *count.entry(r).or_insert(0) += 1;
        }
        if roots.len() <= 1 {
            return self.clone();
        }
        let mut order: Vec<u32> = vec![*roots.last().unwrap()];
        order.extend(roots[..roots.len() - 1].iter().copied());
        let mut best = order[0];
        for &r in &order {
            if count[&r] > count[&best] {
                best = r;
            }
        }
        let keep: Vec<bool> = (0..n as u32).map(|i| find(&mut uf, i) == best).collect();
        self.keep_verts(&keep)
    }
    /// decorate.distance2boundary: BFS hop distance from the boundary-loop vertices, normalised by the deepest level
    pub fn distance2boundary(&self) -> Vec<f64> {
        let n = self.v.len();
        // region_to_loop on the fully selected mesh: edges used by exactly one face
        let mut cnt: HashMap<(u32, u32), u32> = HashMap::default();
        for fc in &self.f {
            let k = fc.len();
            for i in 0..k {
                let (a, b) = (fc[i], fc[(i + 1) % k]);
                *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let mut sel = vec![false; n];
        for e in &self.e {
            let key = (e[0].min(e[1]), e[0].max(e[1]));
            let c = cnt.get(&key).copied().unwrap_or(0);
            if c == 1 {
                sel[e[0] as usize] = true;
                sel[e[1] as usize] = true;
            }
        }
        let mut adj: Vec<Vec<u32>> = vec![vec![]; n];
        for e in &self.e {
            adj[e[0] as usize].push(e[1]);
            adj[e[1] as usize].push(e[0]);
        }
        let mut dist = vec![-100.0f64; n];
        let mut queue: Vec<u32> = (0..n as u32).filter(|&i| sel[i as usize]).collect();
        let mut d = 0.0f64;
        let mut dmax = 0usize;
        loop {
            for &i in &queue {
                dist[i as usize] = d;
            }
            let mut next: Vec<u32> = vec![];
            let mut inq = vec![false; n];
            for &i in &queue {
                for &j in &adj[i as usize] {
                    if dist[j as usize] < 0.0 && !inq[j as usize] {
                        inq[j as usize] = true;
                        next.push(j);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            queue = next;
            d += 1.0;
            dmax += 1;
        }
        let _ = dmax;
        for x in dist.iter_mut() {
            if *x < 0.0 {
                *x = 0.0;
            }
        }
        let div = d.max(1.0);
        for x in dist.iter_mut() {
            *x /= div;
        }
        dist
    }
}

/// draw.bezier_curve(anchors) as a mesh (polyline vertices, chain edges); anchors rows are x, y, z (z may be a constant)
pub fn bezier_mesh(anchors: &[[f64; 3]], vector: &[usize]) -> LMesh {
    let v = super::blops::bezier_curve_points(anchors, vector);
    let n = v.len();
    LMesh { e: (0..n.saturating_sub(1) as u32).map(|i| [i, i + 1]).collect(), v, f: vec![] }
}

fn signed_area_xy(p: &[F3], lp: &[u32]) -> f32 {
    let mut a = 0.0f32;
    for i in 0..lp.len() {
        let (p0, p1) = (p[lp[i] as usize], p[lp[(i + 1) % lp.len()] as usize]);
        a += p0[0] * p1[1] - p1[0] * p0[1];
    }
    a * 0.5
}

/// bpy.ops.mesh.fill() for a single closed planar outline (z constant): triangles facing +z, new diagonals appended as edges
pub fn fill_outline(m: &LMesh) -> LMesh {
    let n = m.v.len();
    let mut adj: Vec<Vec<u32>> = vec![vec![]; n];
    for e in &m.e {
        adj[e[0] as usize].push(e[1]);
        adj[e[1] as usize].push(e[0]);
    }
    // prune dangling wire chains (welded curve stems): the fill covers the 2-core loop only
    let mut deg: Vec<usize> = adj.iter().map(|a| a.len()).collect();
    let mut removed = vec![false; n];
    let mut stack: Vec<usize> = (0..n).filter(|&i| deg[i] == 1).collect();
    while let Some(v) = stack.pop() {
        if removed[v] {
            continue;
        }
        removed[v] = true;
        for &u in &adj[v] {
            let u = u as usize;
            if !removed[u] {
                deg[u] -= 1;
                if deg[u] == 1 {
                    stack.push(u);
                }
            }
        }
    }
    let start = (0..n).find(|&i| !removed[i] && !adj[i].is_empty()).unwrap_or(0) as u32;
    let mut lp = vec![start];
    let mut prev = u32::MAX;
    let mut cur = start;
    loop {
        let nxt = adj[cur as usize].iter().copied().find(|&x| x != prev && x != cur && !removed[x as usize]);
        let Some(nx) = nxt else { break };
        if nx == start || lp.contains(&nx) {
            break;
        }
        lp.push(nx);
        prev = cur;
        cur = nx;
    }
    if signed_area_xy(&m.v, &lp) < 0.0 {
        lp.reverse();
    }
    // bmo_triangle_fill: the fill normal is that of the vertex with the largest edge-pair cross product (edge pair = its first two edges in
    // mesh edge order, sign negated when the first leg is the longer one); the triangles wind counter-clockwise about it
    let mut inc: Vec<Vec<u32>> = vec![vec![]; n];
    for (ei, e) in m.e.iter().enumerate() {
        inc[e[0] as usize].push(ei as u32);
        inc[e[1] as usize].push(ei as u32);
    }
    // scanfill vertex order = first appearance in the edge list; qsort (BSD / macOS) over (value, normal.z), descending by value
    let mut order: Vec<u32> = vec![];
    {
        let mut seen = vec![false; n];
        for e in &m.e {
            for &v in e {
                if !seen[v as usize] {
                    seen[v as usize] = true;
                    order.push(v);
                }
            }
        }
    }
    let mut nors: Vec<(f32, f32)> = vec![];
    for &vv in &order {
        let v = vv as usize;
        if inc[v].len() != 2 {
            nors.push((-1.0, 0.0));
            continue;
        }
        let other = |ei: u32| {
            let e = m.e[ei as usize];
            if e[0] as usize == v { e[1] } else { e[0] }
        };
        let pv = m.v[v];
        let (pa, pb) = (m.v[other(inc[v][0]) as usize], m.v[other(inc[v][1]) as usize]);
        let da = [pv[0] - pa[0], pv[1] - pa[1], pv[2] - pa[2]];
        let db = [pv[0] - pb[0], pv[1] - pb[1], pv[2] - pb[2]];
        // arm64 clang contracts a*b - c*d into fma(a, b, -(c*d)); the mirrored vertex of a symmetric leaf then rounds differently, which decides the sort
        let fm = |a: f32, b: f32, c: f32, d: f32| a.mul_add(b, -(c * d));
        let cr = [fm(da[1], db[2], da[2], db[1]), fm(da[2], db[0], da[0], db[2]), fm(da[0], db[1], da[1], db[0])];
        let value = cr[2].mul_add(cr[2], cr[1].mul_add(cr[1], cr[0] * cr[0]));
        let sq = |d: [f32; 3]| d[2].mul_add(d[2], d[1].mul_add(d[1], d[0] * d[0]));
        let (la, lb) = (sq(da), sq(db));
        let nz = if la > lb { -cr[2] } else { cr[2] };
        nors.push((value, nz));
    }
    let pre = nors.clone();
    bsd_qsort(&mut nors, &|a: &(f32, f32), b: &(f32, f32)| if a.0 < b.0 { 1 } else if a.0 > b.0 { -1 } else { 0 });
    let best = nors[0];
    if std::env::var("LEAFDBG").is_ok() {
        let mx = pre.iter().map(|x| x.0).fold(-1.0f32, f32::max);
        let ties: Vec<(usize, f32)> = pre.iter().enumerate().filter(|(_, x)| x.0 == mx).map(|(i, x)| (i, x.1)).collect();
        eprintln!("fill normal: max {mx} ties {:?} chosen nz {}", ties, best.1);
    }
    let want_ccw = best.1 >= 0.0;
    // polyfill_calc(coords_sign = 1) expects the winding that axis_dominant_v3_to_m3_negate yields for a CCW face: clockwise in x/y
    let lpr: Vec<u32> = lp.iter().rev().copied().collect();
    let proj: Vec<[f32; 2]> = lpr.iter().map(|&i| [m.v[i as usize][0], m.v[i as usize][1]]).collect();
    let tris = super::polyfill::polyfill_calc(&proj);
    let lp = lpr;
    let mut o = m.clone();
    for t in tris {
        let (a, b, c) = (lp[t[0] as usize], lp[t[1] as usize], lp[t[2] as usize]);
        let (pa, pb, pc) = (m.v[a as usize], m.v[b as usize], m.v[c as usize]);
        let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
        if (area < 0.0) == want_ccw {
            o.f.push(vec![a, c, b]);
        } else {
            o.f.push(vec![a, b, c]);
        }
    }
    if std::env::var("LEAFDBG").is_ok() {
        let pa = signed_area_xy(&m.v, &lp).abs();
        let ta: f32 = o.f.iter().map(|t| {
            let (a, b, c) = (o.v[t[0] as usize], o.v[t[1] as usize], o.v[t[2] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
        }).sum();
        eprintln!("fill_outline: loop {} of {} verts, polygon area {pa} tri area {ta}", lp.len(), n);
    }
    o.ensure_face_edges();
    o
}

/// SOLIDIFY modifier (simple mode, offset -1, rim on): originals stay, the copy moves by -thickness along the vertex normal
pub fn solidify(m: &LMesh, thickness: f32) -> LMesh {
    let n = m.v.len();
    // zero-area triangles (collinear outline vertices) would contribute the (0,0,1) fallback normal: drop them for the normal calculation only
    let nondeg: Vec<Vec<u32>> = m
        .f
        .iter()
        .filter(|fc| {
            if fc.len() != 3 {
                return true;
            }
            let (a, b, c) = (m.v[fc[0] as usize], m.v[fc[1] as usize], m.v[fc[2] as usize]);
            let cr = [(b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]), (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]), (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])];
            cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2] > 0.0
        })
        .cloned()
        .collect();
    let mesh = crate::mesh::Mesh { pos: m.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: nondeg, uv: vec![] };
    let mut nrm = super::legtex::vert_normals(&mesh);
    // planar xy fill: sliver triangles make the angle-weighted normal sign noisy; Blender's triangulation has none, so use the fill orientation
    if m.v.iter().all(|p| p[2] == m.v[0][2]) && !m.f.is_empty() {
        let mut area = 0.0f64;
        for fc in &m.f {
            for k in 1..fc.len() - 1 {
                let (a, b, c) = (m.v[fc[0] as usize], m.v[fc[k] as usize], m.v[fc[k + 1] as usize]);
                area += ((b[0] - a[0]) as f64) * ((c[1] - a[1]) as f64) - ((b[1] - a[1]) as f64) * ((c[0] - a[0]) as f64);
            }
        }
        let sg = if area >= 0.0 { 1.0f32 } else { -1.0 };
        let mut used = vec![false; n];
        for fc in &m.f {
            for &i in fc {
                used[i as usize] = true;
            }
        }
        for i in 0..n {
            if used[i] {
                nrm[i] = [0.0, 0.0, sg];
            }
        }
    }
    let ofs_orig = -(((-(-1.0f32) + 1.0) * 0.5) * thickness);
    let ofs_new = thickness + ofs_orig;
    let mut o = LMesh::default();
    for i in 0..n {
        let p = m.v[i];
        o.v.push([p[0] + nrm[i][0] * ofs_new, p[1] + nrm[i][1] * ofs_new, p[2] + nrm[i][2] * ofs_new]);
    }
    for i in 0..n {
        let p = m.v[i];
        o.v.push([p[0] + nrm[i][0] * ofs_orig, p[1] + nrm[i][1] * ofs_orig, p[2] + nrm[i][2] * ofs_orig]);
    }
    for fc in &m.f {
        o.f.push(fc.clone());
    }
    for fc in &m.f {
        let mut r = vec![fc[0] + n as u32];
        for &i in fc[1..].iter().rev() {
            r.push(i + n as u32);
        }
        o.f.push(r);
    }
    // rim quads over boundary edges, in edge order
    let mut cnt: HashMap<(u32, u32), u32> = HashMap::default();
    for fc in &m.f {
        let k = fc.len();
        for i in 0..k {
            let (a, b) = (fc[i], fc[(i + 1) % k]);
            *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    for e in &m.e {
        if cnt.get(&(e[0].min(e[1]), e[0].max(e[1]))).copied().unwrap_or(0) == 1 {
            o.f.push(vec![e[1], e[0], e[0] + n as u32, e[1] + n as u32]);
        }
    }
    o.e = m.e.clone();
    o.ensure_face_edges();
    o
}

/// geo node SampleNearestSurface of a point attribute (barycentric interpolation at the closest point of the corner triangles)
pub fn sample_nearest_surface(src: &LMesh, val: &[f32], at: &[F3]) -> Vec<(f32, F3)> {
    let tris = super::dualcon::corner_tris(&src.v, &src.f);
    // uniform grid over triangle bounding boxes
    let (lo, hi) = src.bounds();
    let ext = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max).max(1e-6);
    let cell = (ext / 24.0).max(0.01);
    let key = |p: F3| -> [i64; 3] { [0, 1, 2].map(|k| ((p[k] - lo[k]) / cell).floor() as i64) };
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::default();
    for (ti, t) in tris.iter().enumerate() {
        let ps = [src.v[t[0] as usize], src.v[t[1] as usize], src.v[t[2] as usize]];
        let mut a = ps[0];
        let mut b = ps[0];
        for p in &ps {
            for k in 0..3 {
                a[k] = a[k].min(p[k]);
                b[k] = b[k].max(p[k]);
            }
        }
        let (ka, kb) = (key(a), key(b));
        for x in ka[0]..=kb[0] {
            for y in ka[1]..=kb[1] {
                for z in ka[2]..=kb[2] {
                    grid.entry([x, y, z]).or_default().push(ti as u32);
                }
            }
        }
    }
    // per-triangle bounding boxes: a triangle whose box is farther than the best distance so far can neither win nor tie
    let tri_box: Vec<(F3, F3)> = tris
        .iter()
        .map(|t| {
            let ps = [src.v[t[0] as usize], src.v[t[1] as usize], src.v[t[2] as usize]];
            let mut a = ps[0];
            let mut b = ps[0];
            for p in &ps {
                for k in 0..3 {
                    a[k] = a[k].min(p[k]);
                    b[k] = b[k].max(p[k]);
                }
            }
            (a, b)
        })
        .collect();
    use rayon::prelude::*;
    at.par_iter()
        .map(|q| {
            let kq = key(*q);
            let mut best = (f32::MAX, usize::MAX, [0f32; 3]);
            let mut rr = 1i64;
            loop {
                for x in kq[0] - rr..=kq[0] + rr {
                    for y in kq[1] - rr..=kq[1] + rr {
                        for z in kq[2] - rr..=kq[2] + rr {
                            if rr > 1 && (x - kq[0]).abs().max((y - kq[1]).abs()).max((z - kq[2]).abs()) < rr {
                                continue;
                            }
                            if let Some(l) = grid.get(&[x, y, z]) {
                                for &ti in l {
                                    if best.1 != usize::MAX {
                                        let (bl, bh) = &tri_box[ti as usize];
                                        let mut lb = 0.0f32;
                                        for k in 0..3 {
                                            let d = (bl[k] - q[k]).max(q[k] - bh[k]).max(0.0);
                                            lb += d * d;
                                        }
                                        if lb > best.0 * 1.001 + 1e-9 {
                                            continue;
                                        }
                                    }
                                    let t = tris[ti as usize];
                                    let r = closest_on_tri_f32(*q, src.v[t[0] as usize], src.v[t[1] as usize], src.v[t[2] as usize]);
                                    let d = sub3(r, *q);
                                    let d2 = dot3(d, d);
                                    let area = |t: [u32; 3]| {
                                        let (a, b, c) = (src.v[t[0] as usize], src.v[t[1] as usize], src.v[t[2] as usize]);
                                        let (u, v) = (sub3(b, a), sub3(c, a));
                                        let cr = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                                        dot3(cr, cr)
                                    };
                                    // equal distance (point on a shared edge / in a coplanar region): prefer the larger triangle, slivers give noisy weights
                                    let tie = best.1 != usize::MAX && (d2 - best.0).abs() <= 1e-13;
                                    if (!tie && d2 < best.0) || (tie && area(t) > area(tris[best.1])) {
                                        best = (d2, ti as usize, r);
                                    }
                                }
                            }
                        }
                    }
                }
                if best.1 != usize::MAX && best.0.sqrt() <= cell * (rr as f32) {
                    break;
                }
                rr += 1;
                if rr > 40 {
                    break;
                }
            }
            let t = tris[best.1];
            let (v1, v2, v3) = (src.v[t[0] as usize], src.v[t[1] as usize], src.v[t[2] as usize]);
            let w = interp_weights_tri_f32(v1, v2, v3, best.2);
            let pos = [0, 1, 2].map(|k| v1[k] * w[0] + v2[k] * w[1] + v3[k] * w[2]);
            let v = val[t[0] as usize] * w[0] + val[t[1] as usize] * w[1] + val[t[2] as usize] * w[2];
            (v, pos)
        })
        .collect()
}

fn sub3(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot3(a: F3, b: F3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// BLI closest_on_tri_to_point_v3 (float32)
pub fn closest_on_tri_f32(p: F3, v1: F3, v2: F3, v3: F3) -> F3 {
    let ab = sub3(v2, v1);
    let mut ac = sub3(v3, v1);
    let ap = sub3(p, v1);
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return v1;
    }
    let bp = sub3(p, v2);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return v2;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [v1[0] + ab[0] * v, v1[1] + ab[1] * v, v1[2] + ab[2] * v];
    }
    let cp = sub3(p, v3);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return v3;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [v1[0] + ac[0] * w, v1[1] + ac[1] * w, v1[2] + ac[2] * w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let mut r = sub3(v3, v2);
        for k in 0..3 {
            r[k] *= w;
            r[k] += v2[k];
        }
        return r;
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    for k in 0..3 {
        ac[k] *= w;
    }
    [v1[0] + ab[0] * v + ac[0], v1[1] + ab[1] * v + ac[1], v1[2] + ab[2] * v + ac[2]]
}

/// BLI interp_weights_tri_v3 (float32)
pub fn interp_weights_tri_f32(v1: F3, v2: F3, v3: F3, co: F3) -> [f32; 3] {
    let n1 = sub3(v1, v2);
    let n2 = sub3(v2, v3);
    let mut n = [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]];
    let d = dot3(n, n);
    if d > 1.0e-35 {
        let inv = 1.0 / d.sqrt();
        for k in 0..3 {
            n[k] *= inv;
        }
    } else {
        n = [0.0; 3];
    }
    let (xn, yn, zn) = (n[0].abs(), n[1].abs(), n[2].abs());
    let (i, j) = if zn >= xn && zn >= yn { (0, 1) } else if yn >= xn && yn >= zn { (0, 2) } else { (1, 2) };
    let area = |a: F3, b: F3, c: F3| 0.5 * ((a[i] - b[i]) * (b[j] - c[j]) + (a[j] - b[j]) * (c[i] - b[i]));
    let mut w = [area(v2, v3, co), area(v3, v1, co), area(v1, v2, co)];
    let wtot = w[0] + w[1] + w[2];
    if wtot != 0.0 {
        let inv = 1.0 / wtot;
        for k in 0..3 {
            w[k] *= inv;
        }
        if w.iter().all(|x| x.is_finite()) {
            return w;
        }
    }
    [1.0 / 3.0; 3]
}

/// draw.remesh_fill(obj, resolution): solidify, sharp dual-contouring remesh, drop the vertices nearest the offset copy
pub fn remesh_fill(m: &LMesh, resolution: f64) -> LMesh {
    let n = m.v.len();
    let s = solidify(m, 0.1);
    s.trace("SOLIDIFY");
    s.trace("geo_geo_attribute");
    let top: Vec<f32> = (0..s.v.len()).map(|i| if i >= n { 1.0 } else { 0.0 }).collect();
    // sharp_remesh_with_attrs
    let dims = s.dimensions();
    let maxd = dims[0].max(dims[1]).max(dims[2]) as f64;
    let depth = (((maxd + 0.01) / resolution).log2()).ceil() as i32;
    let (rv, rq) = super::dualcon::dualcon(&s.v, &s.f, 2, depth, 0.9, 1.0);
    let rf: Vec<Vec<u32>> = rq.iter().map(|q| q.iter().map(|&i| i as u32).collect()).collect();
    let mut r = LMesh { v: rv, f: rf, e: vec![] };
    r.ensure_face_edges();
    r.trace("REMESH");
    // transfer_all copies every non-internal attribute -- `top` and the vertex `position` itself: each vertex snaps onto the nearest point of the solid
    let samp = sample_nearest_surface(&s, &top, &r.v);
    let tv: Vec<f32> = samp.iter().map(|x| x.0).collect();
    for (i, x) in samp.iter().enumerate() {
        r.v[i] = x.1;
    }
    r.trace("geo_transfer_att_node");
    r.trace("geo_copy_geom_info");
    let del: Vec<bool> = tv.iter().map(|&t| t > 1e-3).collect();
    if std::env::var("LEAFDBG").is_ok() {
        let kept: Vec<usize> = (0..del.len()).filter(|&i| !del[i]).collect();
        eprintln!("remesh_fill: kept {} of {}", kept.len(), del.len());
        let amb = tv.iter().filter(|&&t| t > 1e-9 && t <= 0.5).count();
        eprintln!("  ambiguous (1e-9<t<=0.5): {}", amb);
        let mut o = r.delete_verts(&del);
        o.trace("DBGKEPT");
        return o;
    }
    r.delete_verts(&del)
}

/// draw.leaf(x_anchors, y_anchors, face_size)
pub fn leaf(x: &[f64], y: &[f64], face_size: Option<f64>) -> LMesh {
    let mut parts: Vec<LMesh> = vec![];
    for s in [-1.0f64, 1.0] {
        let anchors: Vec<[f64; 3]> = (0..x.len()).map(|i| [x[i], s * y[i], 0.0]).collect();
        let c = bezier_mesh(&anchors, &[]);
        c.trace("WELD");
        parts.push(c);
    }
    let j = LMesh::join(&[&parts[0], &parts[1]]);
    let j = j.weld(0.001);
    j.trace("WELD");
    let filled = fill_outline(&j);
    let mut o = remesh_fill(&filled, 0.005);
    if let Some(fs) = face_size {
        o = o.weld(fs / 2.0);
        o.trace("WELD");
    }
    o.largest_component()
}

/// Apple / FreeBSD libc qsort (Bentley-McIlroy 3-way partition, insertion sort for n < 7 or when a pass swapped nothing), needed for tie behaviour
pub fn bsd_qsort<T: Copy>(a: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
    let n = a.len();
    if n < 2 {
        return;
    }
    let med3 = |a: &[T], x: usize, y: usize, z: usize| -> usize {
        if cmp(&a[x], &a[y]) < 0 {
            if cmp(&a[y], &a[z]) < 0 { y } else if cmp(&a[x], &a[z]) < 0 { z } else { x }
        } else if cmp(&a[y], &a[z]) > 0 {
            y
        } else if cmp(&a[x], &a[z]) < 0 {
            x
        } else {
            z
        }
    };
    let insertion = |a: &mut [T]| {
        for pm in 1..a.len() {
            let mut pl = pm;
            while pl > 0 && cmp(&a[pl - 1], &a[pl]) > 0 {
                a.swap(pl, pl - 1);
                pl -= 1;
            }
        }
    };
    if n < 7 {
        insertion(a);
        return;
    }
    let mut pm = n / 2;
    if n > 7 {
        let mut pl = 0;
        let mut pn = n - 1;
        if n > 40 {
            let d = n / 8;
            pl = med3(a, pl, pl + d, pl + 2 * d);
            pm = med3(a, pm - d, pm, pm + d);
            pn = med3(a, pn - 2 * d, pn - d, pn);
        }
        pm = med3(a, pl, pm, pn);
    }
    a.swap(0, pm);
    let (mut pa, mut pb) = (1usize, 1usize);
    let (mut pc, mut pd) = (n - 1, n - 1);
    let mut swap_cnt = false;
    loop {
        while pb <= pc {
            let r = cmp(&a[pb], &a[0]);
            if r > 0 {
                break;
            }
            if r == 0 {
                swap_cnt = true;
                a.swap(pa, pb);
                pa += 1;
            }
            pb += 1;
        }
        while pb <= pc {
            let r = cmp(&a[pc], &a[0]);
            if r < 0 {
                break;
            }
            if r == 0 {
                swap_cnt = true;
                a.swap(pc, pd);
                pd -= 1;
            }
            pc -= 1;
        }
        if pb > pc {
            break;
        }
        a.swap(pb, pc);
        swap_cnt = true;
        pb += 1;
        pc -= 1;
    }
    if !swap_cnt && std::env::var("QSORT_NOINS").is_err() {
        insertion(a);
        return;
    }
    let pn = n;
    let vecswap = |a: &mut [T], mut i: usize, mut j: usize, cnt: usize| {
        for _ in 0..cnt {
            a.swap(i, j);
            i += 1;
            j += 1;
        }
    };
    let d1 = pa.min(pb - pa);
    vecswap(a, 0, pb - d1, d1);
    let d1b = (pd - pc).min(pn - pd - 1);
    vecswap(a, pb, pn - d1b, d1b);
    let d1 = pb - pa;
    let d2 = pd - pc;
    if d1 > 1 {
        bsd_qsort(&mut a[..d1], cmp);
    }
    if d2 > 1 {
        let s = pn - d2;
        bsd_qsort(&mut a[s..], cmp);
    }
}

/// numpy 1.26 `np.argsort(float32 array)` (kind quicksort = introsort on the index array; the tie order matters for lattice data)
pub fn np_argsort_f32(v: &[f32]) -> Vec<usize> {
    let num = v.len();
    let mut idx: Vec<usize> = (0..num).collect();
    if num < 2 {
        return idx;
    }
    let less = |a: f32, b: f32| a < b || (b != b && a == a);
    let mut stack: Vec<(usize, usize)> = vec![];
    let (mut pl, mut pr) = (0usize, num - 1);
    loop {
        while pr as i64 - pl as i64 > 16 {
            let pm = pl + ((pr - pl) >> 1);
            if less(v[idx[pm]], v[idx[pl]]) {
                idx.swap(pm, pl);
            }
            if less(v[idx[pr]], v[idx[pm]]) {
                idx.swap(pr, pm);
            }
            if less(v[idx[pm]], v[idx[pl]]) {
                idx.swap(pm, pl);
            }
            let vp = v[idx[pm]];
            let mut pi = pl;
            let mut pj = pr - 1;
            idx.swap(pm, pj);
            loop {
                loop {
                    pi += 1;
                    if !less(v[idx[pi]], vp) {
                        break;
                    }
                }
                loop {
                    pj -= 1;
                    if !less(vp, v[idx[pj]]) {
                        break;
                    }
                }
                if pi >= pj {
                    break;
                }
                idx.swap(pi, pj);
            }
            let pk = pr - 1;
            idx.swap(pi, pk);
            if pi - pl < pr - pi {
                stack.push((pi + 1, pr));
                pr = pi - 1;
            } else {
                stack.push((pl, pi - 1));
                pl = pi + 1;
            }
        }
        for pi in pl + 1..=pr {
            let vi = idx[pi];
            let vp = v[vi];
            let mut pj = pi;
            while pj > pl && less(vp, v[idx[pj - 1]]) {
                idx[pj] = idx[pj - 1];
                pj -= 1;
            }
            idx[pj] = vi;
        }
        match stack.pop() {
            None => break,
            Some((l, r)) => {
                pl = l;
                pr = r;
            }
        }
    }
    idx
}
