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
        let cr = [da[1] * db[2] - da[2] * db[1], da[2] * db[0] - da[0] * db[2], da[0] * db[1] - da[1] * db[0]];
        let value = cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2];
        let (la, lb) = (da[0] * da[0] + da[1] * da[1] + da[2] * da[2], db[0] * db[0] + db[1] * db[1] + db[2] * db[2]);
        let nz = if la > lb { -cr[2] } else { cr[2] };
        nors.push((value, nz));
    }
    bsd_qsort(&mut nors, &|a: &(f32, f32), b: &(f32, f32)| if a.0 < b.0 { 1 } else if a.0 > b.0 { -1 } else { 0 });
    let best = nors[0];
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
    let mesh = crate::mesh::Mesh { pos: m.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.f.clone(), uv: vec![] };
    let nrm = super::legtex::vert_normals(&mesh);
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
    use parry3d_f64::math::Point;
    use parry3d_f64::query::PointQuery;
    use parry3d_f64::shape::TriMesh;
    let tris = super::dualcon::corner_tris(&src.v, &src.f);
    let pts: Vec<Point<f64>> = src.v.iter().map(|p| Point::new(p[0] as f64, p[1] as f64, p[2] as f64)).collect();
    let tm = TriMesh::new(pts.clone(), tris.clone()).unwrap();
    at.iter()
        .map(|q| {
            let qp = Point::new(q[0] as f64, q[1] as f64, q[2] as f64);
            let (proj, feat) = tm.project_local_point_and_get_feature(&qp);
            let fi = match feat {
                parry3d_f64::shape::FeatureId::Face(i) => (i % tris.len() as u32) as usize,
                _ => {
                    // vertex / edge features: find a triangle holding the closest point
                    let mut best = (f64::MAX, 0usize);
                    for (ti, t) in tris.iter().enumerate() {
                        let tri = parry3d_f64::shape::Triangle::new(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
                        let pr = tri.project_local_point(&qp, true);
                        let d = (pr.point - proj.point).norm_squared();
                        if d < best.0 {
                            best = (d, ti);
                        }
                    }
                    best.1
                }
            };
            let t = tris[fi];
            let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let p = proj.point;
            let (v0, v1, v2) = (b - a, c - a, p - a);
            let d00 = v0.dot(&v0);
            let d01 = v0.dot(&v1);
            let d11 = v1.dot(&v1);
            let d20 = v2.dot(&v0);
            let d21 = v2.dot(&v1);
            let den = d00 * d11 - d01 * d01;
            let (w1, w2) = if den.abs() < 1e-30 { (0.0, 0.0) } else { ((d11 * d20 - d01 * d21) / den, (d00 * d21 - d01 * d20) / den) };
            let w0 = 1.0 - w1 - w2;
            let pa = src.v[t[0] as usize];
            let pb = src.v[t[1] as usize];
            let pc = src.v[t[2] as usize];
            let pos = [0, 1, 2].map(|k| (w0 * pa[k] as f64 + w1 * pb[k] as f64 + w2 * pc[k] as f64) as f32);
            ((w0 * val[t[0] as usize] as f64 + w1 * val[t[1] as usize] as f64 + w2 * val[t[2] as usize] as f64) as f32, pos)
        })
        .collect()
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
    if !swap_cnt {
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
