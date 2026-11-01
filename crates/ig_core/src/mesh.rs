//! Indexed polygon-mesh toolkit (quads/n-gons kept as faces) + scene container + glTF writer.
use serde_json::json;

pub type V3 = [f64; 3];

/// Indexed polygon mesh. Each face is a CCW polygon (len >= 3) of indices into `pos`.
/// `uv`, when non-empty and the same length as `pos`, is per-vertex.
#[derive(Clone, Default, Debug)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
    pub uv: Vec<[f64; 2]>,
}

impl Mesh {
    /// Catmull-Clark smoothing; `bevel` > 0 first bevels sharp edges (width, segments, 30 degree limit) like Blender's BEVEL then SUBSURF.
    pub fn smoothed(&self, bevel_w: f64, segments: usize, levels: usize) -> Mesh {
        use crate::subdiv::*;
        if self.faces.is_empty() {
            return self.clone();
        }
        let q = QMesh { pos: self.pos.clone(), faces: self.faces.clone() }.weld(1e-6);
        let out = if bevel_w > 0.0 { bevel_then_subsurf(&q, bevel_w, segments.max(1), 30f64.to_radians(), levels) } else { catmull_clark(&q, levels, None) };
        if out.pos.iter().any(|p| !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite()) || out.faces.is_empty() {
            return self.clone();
        }
        // re-expand to per-face vertices so the exporter's smooth-by-angle normals work as before
        let mut m = Mesh::new();
        for f in &out.faces {
            let pts: Vec<V3> = f.iter().map(|&i| out.pos[i as usize]).collect();
            m.add_face(&pts);
        }
        m
    }
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_tri(&mut self, a: V3, b: V3, c: V3) {
        self.add_face(&[a, b, c]);
    }
    /// quad with CCW winding (a,b,c,d) => outward normal by right-hand rule. One 4-vertex face (4 new positions).
    pub fn add_quad(&mut self, a: V3, b: V3, c: V3, d: V3) {
        self.add_face(&[a, b, c, d]);
    }
    /// Add a polygon face with fresh vertices (no welding).
    pub fn add_face(&mut self, pts: &[V3]) {
        assert!(pts.len() >= 3, "face needs >= 3 vertices");
        let i = self.pos.len() as u32;
        self.pos.extend_from_slice(pts);
        self.faces.push((i..i + pts.len() as u32).collect());
    }
    /// Add a face referencing existing vertices.
    pub fn add_face_idx(&mut self, ids: &[u32]) {
        assert!(ids.len() >= 3, "face needs >= 3 vertices");
        self.faces.push(ids.to_vec());
    }
    /// Fan-triangulate every face: (0,k,k+1). Quads give (0,1,2),(0,2,3).
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        let mut out = Vec::with_capacity(self.faces.len() * 2);
        for f in &self.faces {
            for k in 1..f.len() - 1 {
                out.push([f[0], f[k], f[k + 1]]);
            }
        }
        out
    }
    pub fn append(&mut self, o: &Mesh) {
        let base = self.pos.len() as u32;
        // uvs kept when either side has them (missing ones padded with 0); meshes without uvs stay without
        if o.uv.len() == o.pos.len() && !o.pos.is_empty() || (!self.uv.is_empty() && self.uv.len() == self.pos.len()) {
            self.uv.resize(self.pos.len(), [0.0, 0.0]);
            if o.uv.len() == o.pos.len() { self.uv.extend_from_slice(&o.uv); } else { self.uv.resize(self.pos.len() + o.pos.len(), [0.0, 0.0]); }
        }
        self.pos.extend_from_slice(&o.pos);
        self.faces.extend(o.faces.iter().map(|f| f.iter().map(|i| i + base).collect::<Vec<u32>>()));
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in self.pos.iter_mut() {
            *p = m.apply(*p);
        }
        if m.det() < 0.0 {
            for f in self.faces.iter_mut() {
                f[1..].reverse();
            }
        }
    }
    pub fn flipped(mut self) -> Mesh {
        for f in self.faces.iter_mut() {
            f[1..].reverse();
        }
        self
    }
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &self.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
    pub fn area(&self) -> f64 {
        self.triangles()
            .iter()
            .map(|t| {
                let (a, b, c) = (self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]);
                let n = cross(sub(b, a), sub(c, a));
                0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
            })
            .sum()
    }

    /// Merge vertices closer than ~eps (hash by rounded position), remap faces, drop degenerate faces
    /// (consecutive duplicate vertices are collapsed first; faces left with < 3 distinct vertices are removed).
    /// Per-vertex uv is kept from the first vertex of each merged group (if present).
    pub fn weld(&mut self, eps: f64) {
        let eps = eps.max(1e-12);
        let key = |p: V3| ((p[0] / eps).round() as i64, (p[1] / eps).round() as i64, (p[2] / eps).round() as i64);
        let has_uv = self.uv.len() == self.pos.len();
        let mut map: crate::dhash::HashMap<(i64, i64, i64), u32> = crate::dhash::HashMap::default();
        let mut remap = vec![0u32; self.pos.len()];
        let mut npos: Vec<V3> = vec![];
        let mut nuv: Vec<[f64; 2]> = vec![];
        for (i, p) in self.pos.iter().enumerate() {
            let id = *map.entry(key(*p)).or_insert_with(|| {
                npos.push(*p);
                if has_uv {
                    nuv.push(self.uv[i]);
                }
                (npos.len() - 1) as u32
            });
            remap[i] = id;
        }
        let mut nfaces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut g: Vec<u32> = Vec::with_capacity(f.len());
            for &v in f {
                let r = remap[v as usize];
                if g.last() != Some(&r) {
                    g.push(r);
                }
            }
            while g.len() > 1 && g.first() == g.last() {
                g.pop();
            }
            if g.len() >= 3 {
                nfaces.push(g);
            }
        }
        self.pos = npos;
        self.uv = nuv;
        self.faces = nfaces;
    }
    pub fn welded(&self, eps: f64) -> Mesh {
        let mut m = self.clone();
        m.weld(eps);
        m
    }
    /// Unique undirected edges (sorted index pairs) of the welded mesh (eps 1e-6), sorted.
    pub fn to_quadmesh_edges(&self) -> Vec<[u32; 2]> {
        let w = self.welded(1e-6);
        let mut set = std::collections::BTreeSet::new();
        for f in &w.faces {
            for k in 0..f.len() {
                let (a, b) = (f[k], f[(k + 1) % f.len()]);
                if a != b {
                    set.insert([a.min(b), a.max(b)]);
                }
            }
        }
        set.into_iter().collect()
    }
    /// Export form: (positions, uv, triangles) where quads/n-gons are fan-triangulated with the fan's shared
    /// vertices duplicated for k >= 2 (matching the old triangle-soup layout, so per-triangle normals/uvs are identical).
    fn export_tris(&self, tj: bool) -> (Vec<V3>, Vec<[f64; 2]>, Vec<[u32; 3]>, Vec<u32>) {
        // Full triangle soup: every triangle owns its 3 vertices. The exporter computes smooth-by-angle normals per vertex
        // (welding by position), so a vertex index shared by faces at a sharp corner would get the normal of whichever triangle was
        // processed last (dark / flipped-looking triangles on flat panels of indexed meshes such as the exact KitchenSpace slab).
        let fixed = if !tj || std::env::var_os("NO_TJ").is_some() { None } else { self.split_tjunctions() };
        let this = fixed.as_ref().unwrap_or(self);
        let has_uv = this.uv.len() == this.pos.len();
        let mut pos: Vec<V3> = Vec::with_capacity(this.faces.len() * 6);
        let mut uv: Vec<[f64; 2]> = vec![];
        let mut tris = Vec::with_capacity(this.faces.len() * 2);
        let mut tri_face: Vec<u32> = Vec::with_capacity(this.faces.len() * 2);
        let mut seen: std::collections::HashSet<u128, FxBuild> = Default::default();
        for (fi, f) in this.faces.iter().enumerate() {
            for [ia, ib, ic] in triangulate_face(&this.pos, f) {
                // a triangle that coincides with an earlier one (same welded corners, either winding: collapsed cloth layers, merged
                // box-comforter seams, boolean leftovers) would z-fight with it (one side lit, one dark): keep the first only
                // corner = 64-bit mix of the welded 10 um grid cell; the triangle key is the sorted corner triple folded to 128 bits
                let q = |i: u32| -> u64 {
                    let p = this.pos[i as usize];
                    let mut h = 0x9E37_79B9_7F4A_7C15u64;
                    for d in 0..3 { h = (h ^ ((p[d] * 1e5).round() as i64 as u64)).wrapping_mul(0xFF51_AFD7_ED55_8CCD); h ^= h >> 32; }
                    h
                };
                let mut k = [q(ia), q(ib), q(ic)];
                k.sort_unstable();
                if k[0] == k[1] || k[1] == k[2] {
                    continue;
                }
                let key128 = ((k[0] ^ k[1].rotate_left(21) ^ k[2].rotate_left(42)) as u128) << 64 | (k[0].wrapping_mul(3).wrapping_add(k[1].wrapping_mul(5)).wrapping_add(k[2].wrapping_mul(7)) as u128);
                if !seen.insert(key128) {
                    continue;
                }
                // zero-area triangles (collinear corners) draw nothing
                let x = cross(sub(this.pos[ib as usize], this.pos[ia as usize]), sub(this.pos[ic as usize], this.pos[ia as usize]));
                if dot(x, x) < 4e-20 {
                    continue;
                }
                let n = pos.len() as u32;
                for &i in &[ia, ib, ic] {
                    pos.push(this.pos[i as usize]);
                    if has_uv {
                        uv.push(this.uv[i as usize]);
                    }
                }
                tris.push([n, n + 1, n + 2]);
                tri_face.push(fi as u32);
            }
        }
        (pos, uv, tris, tri_face)
    }
}

impl Mesh {
    /// Repair T-junctions: a vertex that lies on the interior of another face's boundary edge (within 0.1 mm; both ends welded on a
    /// 10 um grid) is inserted into that edge, so booleans / sub-divided neighbours leave no hairline cracks. Only edges used by a single
    /// face (open boundary) are examined. Returns None when nothing changes (no allocation on the common path).
    pub fn split_tjunctions(&self) -> Option<Mesh> {
        use FxMap as HashMap;
        let key = |p: V3| ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64);
        let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::default();
        let mut wid = Vec::with_capacity(self.pos.len());
        let mut rep: Vec<u32> = vec![]; // welded id -> first vertex index
        for (i, p) in self.pos.iter().enumerate() {
            let n = ids.len() as u32;
            let w = *ids.entry(key(*p)).or_insert(n);
            if w == n {
                rep.push(i as u32);
            }
            wid.push(w);
        }
        let mut cnt: HashMap<(u32, u32), u32> = HashMap::default();
        for f in &self.faces {
            for k in 0..f.len() {
                let (a, b) = (wid[f[k] as usize], wid[f[(k + 1) % f.len()] as usize]);
                if a != b {
                    *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
        }
        let mut bedges: Vec<(u32, u32)> = cnt.iter().filter(|(_, &c)| c == 1).map(|(&e, _)| e).collect();
        if bedges.len() < 2 {
            return None;
        }
        bedges.sort_unstable();
        let mut bv: Vec<u32> = bedges.iter().flat_map(|e| [e.0, e.1]).collect();
        bv.sort_unstable();
        bv.dedup();
        const CELL: f64 = 0.05;
        let cell = |p: V3| ((p[0] / CELL).floor() as i64, (p[1] / CELL).floor() as i64, (p[2] / CELL).floor() as i64);
        let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::default();
        for &w in &bv {
            grid.entry(cell(self.pos[rep[w as usize] as usize])).or_default().push(w);
        }
        let mut ins: HashMap<(u32, u32), Vec<(f64, u32)>> = HashMap::default();
        for &(a, b) in &bedges {
            let (pa, pb) = (self.pos[rep[a as usize] as usize], self.pos[rep[b as usize] as usize]);
            let d = sub(pb, pa);
            let l2 = dot(d, d);
            // only edges >= 1 cm: leaf / foliage meshes have mm-scale open edges everywhere by design (no visible cracks, +40% triangles)
            if l2 < 1e-4 {
                continue;
            }
            let (lo, hi) = ([pa[0].min(pb[0]) - 1e-4, pa[1].min(pb[1]) - 1e-4, pa[2].min(pb[2]) - 1e-4], [pa[0].max(pb[0]) + 1e-4, pa[1].max(pb[1]) + 1e-4, pa[2].max(pb[2]) + 1e-4]);
            let (c0, c1) = (cell(lo), cell(hi));
            if (c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) * (c1.2 - c0.2 + 1) > 20000 {
                continue;
            }
            for cx in c0.0..=c1.0 {
                for cy in c0.1..=c1.1 {
                    for cz in c0.2..=c1.2 {
                        let Some(vs) = grid.get(&(cx, cy, cz)) else { continue };
                        for &w in vs {
                            if w == a || w == b {
                                continue;
                            }
                            let p = self.pos[rep[w as usize] as usize];
                            let t = dot(sub(p, pa), d) / l2;
                            let l = l2.sqrt();
                            if t * l < 2e-5 || (1.0 - t) * l < 2e-5 || t <= 0.0 || t >= 1.0 {
                                continue;
                            }
                            let q = [pa[0] + t * d[0], pa[1] + t * d[1], pa[2] + t * d[2]];
                            let dd = sub(p, q);
                            if dot(dd, dd) < 1e-8 {
                                ins.entry((a, b)).or_default().push((t, rep[w as usize]));
                            }
                        }
                    }
                }
            }
        }
        if ins.is_empty() {
            return None;
        }
        let mut m = self.clone();
        for f in m.faces.iter_mut() {
            let n = f.len();
            let mut nf: Vec<u32> = Vec::with_capacity(n + 2);
            let mut changed = false;
            for k in 0..n {
                let (u, v) = (f[k], f[(k + 1) % n]);
                nf.push(u);
                let (wu, wv) = (wid[u as usize], wid[v as usize]);
                if wu == wv {
                    continue;
                }
                if let Some(list) = ins.get(&(wu.min(wv), wu.max(wv))) {
                    let mut l = list.clone();
                    l.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
                    l.dedup_by(|x, y| wid[x.1 as usize] == wid[y.1 as usize]);
                    if wu > wv {
                        l.reverse();
                    }
                    nf.extend(l.iter().map(|x| x.1));
                    changed = true;
                }
            }
            if changed {
                *f = nf;
            }
        }
        Some(m)
    }
}

/// Triangles of one polygon: a fan for triangles and convex polygons (unchanged output), ear clipping (projected onto the
/// polygon's Newell plane) for non-convex ones, where a fan produces triangles outside the polygon (shards across sink basins).
pub fn triangulate_face(pos: &[V3], f: &[u32]) -> Vec<[u32; 3]> {
    let n = f.len();
    let fan0 = || (1..n - 1).map(|k| [f[0], f[k], f[k + 1]]).collect::<Vec<_>>();
    if n <= 3 {
        return fan0();
    }
    // fan from vertex 0 unless that makes a zero-area triangle (collinear vertices, e.g. T-junction repairs inserted on an edge): then the first apex
    // whose fan has no sliver, so every vertex stays used by a real triangle and the edge it splits is not bridged by a long triangle
    let fan = || {
        let area2 = |a: u32, b: u32, c: u32| { let (pa, pb, pc) = (pos[a as usize], pos[b as usize], pos[c as usize]); let x = cross(sub(pb, pa), sub(pc, pa)); dot(x, x).sqrt() };
        let ok = |a: usize| (1..n - 1).all(|k| area2(f[a], f[(a + k) % n], f[(a + k + 1) % n]) > 1e-12);
        match (0..n).find(|&a| ok(a)) {
            Some(0) => fan0(),
            Some(a) => (1..n - 1).map(|k| [f[a], f[(a + k) % n], f[(a + k + 1) % n]]).collect::<Vec<_>>(),
            None => {
                // collinear vertices on several sides (T-junction repairs): clip ears with a real area, so every vertex stays on a triangle
                let mut rest: Vec<u32> = f.to_vec();
                let mut out = vec![];
                while rest.len() > 3 {
                    let m = rest.len();
                    let Some(i) = (0..m).find(|&i| area2(rest[(i + m - 1) % m], rest[i], rest[(i + 1) % m]) > 1e-12) else { break };
                    out.push([rest[(i + m - 1) % m], rest[i], rest[(i + 1) % m]]);
                    rest.remove(i);
                }
                if rest.len() == 3 {
                    out.push([rest[0], rest[1], rest[2]]);
                    out
                } else {
                    fan0()
                }
            }
        }
    };
    let mut nrm = [0.0; 3];
    for i in 0..n {
        let (a, b) = (pos[f[i] as usize], pos[f[(i + 1) % n] as usize]);
        nrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
        nrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
        nrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = dot(nrm, nrm).sqrt();
    if l < 1e-18 {
        return fan();
    }
    let nrm = [nrm[0] / l, nrm[1] / l, nrm[2] / l];
    // convex if every corner turns the same way as the polygon normal
    let convex = (0..n).all(|i| {
        let (a, b, c) = (pos[f[(i + n - 1) % n] as usize], pos[f[i] as usize], pos[f[(i + 1) % n] as usize]);
        dot(cross(sub(b, a), sub(c, b)), nrm) >= -1e-12
    });
    if convex {
        return fan();
    }
    let ax = if nrm[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let u = cross(nrm, ax);
    let ul = dot(u, u).sqrt();
    let u = [u[0] / ul, u[1] / ul, u[2] / ul];
    let v = cross(nrm, u);
    let flat: Vec<f64> = f.iter().flat_map(|&i| { let p = pos[i as usize]; [dot(p, u), dot(p, v)] }).collect();
    match earcutr::earcut(&flat, &[], 2) {
        Ok(t) if t.len() == 3 * (n - 2) || (t.len() >= 3 && t.len() < 3 * (n - 2) && {
            // earcut drops collinear / zero-area ears: accept when the triangles still cover the polygon (areas add up to the Newell area)
            let tri_area: f64 = t.chunks(3).map(|c| { let x = cross(sub(pos[f[c[1]] as usize], pos[f[c[0]] as usize]), sub(pos[f[c[2]] as usize], pos[f[c[0]] as usize])); 0.5 * dot(x, x).sqrt() }).sum();
            (tri_area - 0.5 * l).abs() <= 1e-7 * (0.5 * l).max(1e-12)
        }) => t
            .chunks(3)
            .map(|c| {
                let (a, b, cc) = (f[c[0]], f[c[1]], f[c[2]]);
                // keep the polygon's winding (earcut may return either orientation)
                let tn = cross(sub(pos[b as usize], pos[a as usize]), sub(pos[cc as usize], pos[a as usize]));
                if dot(tn, nrm) < 0.0 { [a, cc, b] } else { [a, b, cc] }
            })
            .collect(),
        _ => fan(),
    }
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Row-major 4x4 affine transform.
#[derive(Clone, Copy, Debug)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub fn identity() -> Self {
        Mat4([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]])
    }
    pub fn translation(t: V3) -> Self {
        let mut m = Self::identity();
        m.0[0][3] = t[0];
        m.0[1][3] = t[1];
        m.0[2][3] = t[2];
        m
    }
    pub fn scale(s: V3) -> Self {
        let mut m = Self::identity();
        m.0[0][0] = s[0];
        m.0[1][1] = s[1];
        m.0[2][2] = s[2];
        m
    }
    pub fn rot_z(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[c, -s, 0., 0.], [s, c, 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]])
    }
    pub fn rot_x(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[1., 0., 0., 0.], [0., c, -s, 0.], [0., s, c, 0.], [0., 0., 0., 1.]])
    }
    pub fn rot_y(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[c, 0., s, 0.], [0., 1., 0., 0.], [-s, 0., c, 0.], [0., 0., 0., 1.]])
    }
    /// Blender XYZ euler: R = Rz * Ry * Rx
    pub fn euler_xyz(r: V3) -> Self {
        Self::rot_z(r[2]).mul(&Self::rot_y(r[1])).mul(&Self::rot_x(r[0]))
    }
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    r[i][j] += self.0[i][k] * o.0[k][j];
                }
            }
        }
        Mat4(r)
    }
    pub fn apply(&self, p: V3) -> V3 {
        let m = &self.0;
        [
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
            m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
        ]
    }
    pub fn det(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

#[derive(Clone, Debug)]
pub struct Material {
    /// optional procedural-material spec resolved on the Blender side, e.g. {"cls":"ceramic.Plaster","kwargs":{}}
    pub spec: Option<serde_json::Value>,
    pub name: String,
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: [f32; 3],
    pub emissive_strength: f32,
    /// base-colour texture (PNG bytes, shared) with alpha-mask cutoff: leaf cards (glTF alphaMode MASK); None = plain colour
    pub texture: Option<std::sync::Arc<Vec<u8>>>,
    pub alpha_cutoff: f32,
}

impl Material {
    pub fn diffuse(name: &str, rgb: [f32; 3], roughness: f32) -> Self {
        Material {
            spec: None,
            name: name.into(),
            color: [rgb[0], rgb[1], rgb[2], 1.0],
            roughness,
            metallic: 0.0,
            emissive: [0.0; 3],
            emissive_strength: 0.0,
            texture: None,
            alpha_cutoff: 0.5,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub mesh: Mesh,
    pub material: usize,
    /// free-form tag for reporting, e.g. generator class name
    pub class: String,
    /// asset placement (row-major 4x4, Z-up world) the mesh was transformed by; written to the sidecar so the renderer can
    /// restore python-style object-local coordinates (Infinigen shaders use Object texture coordinates)
    pub pose: Option<[[f64; 4]; 4]>,
    /// foliage: a crown centre per mesh face; the exporter bends that face's vertex normals 75 % toward pointing away from it (leaf cards
    /// shade as a volume -- Décoret et al. / game foliage "crown normals" -- instead of as scattered flat quads)
    pub bend: Option<std::sync::Arc<Vec<V3>>>,
    /// instancing: when non-empty the mesh is in a local frame and drawn once per transform (row-major 4x4, Z-up world) -- one
    /// glTF mesh referenced by one node per transform (the outdoor trees: a few dozen builds placed thousands of times)
    pub inst: Vec<[[f64; 4]; 4]>,
}

/// Fast deterministic hasher (FxHash) for lookup-only maps in the exporter whose iteration order never reaches the output.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);
impl std::hash::Hasher for FxHasher {
    fn finish(&self) -> u64 { self.0 }
    fn write(&mut self, bytes: &[u8]) { for &b in bytes { self.write_u64(b as u64); } }
    fn write_u64(&mut self, x: u64) { self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95); }
    fn write_u32(&mut self, x: u32) { self.write_u64(x as u64); }
    fn write_i64(&mut self, x: i64) { self.write_u64(x as u64); }
    fn write_usize(&mut self, x: usize) { self.write_u64(x as u64); }
}
pub type FxBuild = std::hash::BuildHasherDefault<FxHasher>;
pub type FxMap<K, V> = std::collections::HashMap<K, V, FxBuild>;

#[derive(Clone, Debug)]
pub struct Light {
    pub loc: V3,
    pub watt: f64,
    pub radius: f64,
    pub temperature: f64,
    pub name: String,
}

#[derive(Default, Clone)]
pub struct Scene {
    pub materials: Vec<Material>,
    pub nodes: Vec<Node>,
    pub lights: Vec<Light>,
    /// lamps drawn off (sidecar `lights_off`, only written when non-empty): a dataset capture may still switch them on
    pub lights_off: Vec<Light>,
    /// cloth jobs for a Blender re-simulation (sidecar `cloth`)
    pub cloth: Vec<serde_json::Value>,
}

impl Scene {
    pub fn material(&mut self, m: Material) -> usize {
        if let Some(i) = self.materials.iter().position(|x| x.name == m.name) {
            return i;
        }
        self.materials.push(m);
        self.materials.len() - 1
    }
    pub fn add(&mut self, name: &str, class: &str, mesh: Mesh, material: usize) {
        if !mesh.is_empty() {
            self.nodes.push(Node { name: name.into(), mesh, material, class: class.into(), pose: None, bend: None, inst: vec![] });
        }
    }
    /// add a foliage node: `centres` = one crown centre per face of `mesh` (see Node::bend)
    pub fn add_bent(&mut self, name: &str, class: &str, mesh: Mesh, material: usize, centres: Vec<V3>) {
        if !mesh.is_empty() {
            debug_assert_eq!(centres.len(), mesh.faces.len());
            self.nodes.push(Node { name: name.into(), mesh, material, class: class.into(), pose: None, bend: Some(std::sync::Arc::new(centres)), inst: vec![] });
        }
    }
    /// a mesh in its local frame drawn at every transform of `inst` (see Node::inst); `centres` as in add_bent (local frame)
    pub fn add_instanced(&mut self, name: &str, class: &str, mesh: Mesh, material: usize, centres: Option<Vec<V3>>, inst: Vec<[[f64; 4]; 4]>) {
        if !mesh.is_empty() && !inst.is_empty() {
            self.nodes.push(Node { name: name.into(), mesh, material, class: class.into(), pose: None, bend: centres.map(std::sync::Arc::new), inst });
        }
    }
    pub fn has_instances(&self) -> bool { self.nodes.iter().any(|n| !n.inst.is_empty()) }
    pub fn add_posed(&mut self, name: &str, class: &str, mesh: Mesh, material: usize, pose: [[f64; 4]; 4]) {
        if !mesh.is_empty() {
            self.nodes.push(Node { name: name.into(), mesh, material, class: class.into(), pose: Some(pose), bend: None, inst: vec![] });
        }
    }

    /// Write a binary glTF 2.0 (.glb). Z-up scene is converted to glTF's Y-up with a root rotation node.
    pub fn write_glb(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.write_glb_usdc(path, None)
    }

    /// `write_glb`, plus the usdc of `write_usdc` when `usdc` is given, from one set of per-node blobs (triangulation, normals and
    /// welding are the bulk of both writers' time)
    pub fn write_glb_usdc(&self, path: &std::path::Path, usdc: Option<&std::path::Path>) -> std::io::Result<()> {
        let blobs = self.node_blobs(&Default::default());
        let sidecar = self.write_glb_blobs(|| Ok(std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?)), &blobs)?;
        if let Some(u) = usdc { self.write_usdc_blobs(u, &blobs)?; }
        if let Some(sp) = path.parent() {
            let _ = std::fs::write(sp.join(format!("{}.json", path.file_stem().unwrap().to_string_lossy())), serde_json::to_vec_pretty(&sidecar).unwrap());
        }
        Ok(())
    }

    /// The glb bytes of `write_glb` streamed into the writer returned by `open` (called once the size is known); returns the sidecar json
    /// (nodes, lights, cloth jobs) that `write_glb` stores next to the file.
    pub fn write_glb_to<W: std::io::Write>(&self, open: impl FnOnce() -> std::io::Result<W>) -> std::io::Result<serde_json::Value> {
        self.write_glb_blobs(open, &self.node_blobs(&Default::default()))
    }

    fn write_glb_blobs<W: std::io::Write>(&self, open: impl FnOnce() -> std::io::Result<W>, blobs: &[NodeBlob]) -> std::io::Result<serde_json::Value> {
        use rayon::prelude::*;
        let t_glb = web_time::Instant::now();
        // per-node blobs are built in parallel (pure functions of the node), then concatenated in node order: deterministic
        let ph: [std::sync::atomic::AtomicU64; 4] = Default::default();
        if std::env::var_os("GLBPROF").is_some() { eprintln!("GLBPROF blobs {:.1}s (cpu s: export {:.1} normals {:.1} uv {:.1} weld {:.1})", t_glb.elapsed().as_secs_f64(), ph[0].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6, ph[1].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6, ph[2].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6, ph[3].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6); }
        let mut bin: usize = 0; // total bin chunk length; the blobs are streamed to the file in the same order
        let mut buffer_views = vec![];
        let mut accessors = vec![];
        let mut meshes = vec![];
        let mut nodes = vec![];
        let mut child_ids = vec![];
        for (ni, (n, bl)) in self.nodes.iter().zip(blobs).enumerate() {
            let mut put = |data: &[u8], target: u32, bin: &mut usize| { let st = *bin; *bin += data.len(); buffer_views.push(json!({"buffer":0,"byteOffset":st,"byteLength":data.len(),"target":target})); buffer_views.len() - 1 };
            let bv = put(&bl.pos, 34962, &mut bin);
            accessors.push(json!({"bufferView":bv,"componentType":5126,"count":bl.nv,"type":"VEC3","min":[bl.lo[0],bl.lo[1],bl.lo[2]],"max":[bl.hi[0],bl.hi[1],bl.hi[2]]}));
            let pacc = accessors.len() - 1;
            let bv = put(&bl.nrm, 34962, &mut bin);
            accessors.push(json!({"bufferView":bv,"componentType":5126,"count":bl.nv,"type":"VEC3"}));
            let nacc = accessors.len() - 1;
            let mut attrs = json!({"POSITION":pacc,"NORMAL":nacc});
            if let Some(u) = &bl.uv {
                let bv = put(u, 34962, &mut bin);
                accessors.push(json!({"bufferView":bv,"componentType":5126,"count":bl.nv,"type":"VEC2"}));
                attrs["TEXCOORD_0"] = json!(accessors.len() - 1);
            }
            let bv = put(&bl.idx, 34963, &mut bin);
            accessors.push(json!({"bufferView":bv,"componentType":5125,"count":bl.ntri*3,"type":"SCALAR"}));
            let iacc = accessors.len() - 1;
            meshes.push(json!({"name":n.name,"primitives":[{"attributes":attrs,"indices":iacc,"material":n.material}]}));
            if n.inst.is_empty() {
                child_ids.push(nodes.len());
                nodes.push(json!({"name":n.name,"mesh":ni,"extras":{"class":n.class}}));
            } else {
                for (k, m) in n.inst.iter().enumerate() {
                    // glTF matrices are column-major
                    let cm: Vec<f64> = (0..16).map(|i| m[i % 4][i / 4]).collect();
                    child_ids.push(nodes.len());
                    nodes.push(json!({"name":format!("{}.i{k}", n.name),"mesh":ni,"matrix":cm,"extras":{"class":n.class}}));
                }
            }
        }
        let root = nodes.len();
        // Z-up -> Y-up: rotate -90deg about X
        let s = std::f64::consts::FRAC_1_SQRT_2;
        nodes.push(json!({"name":"root","children":child_ids,"rotation":[-s,0.0,0.0,s]}));
        // textures: one image (PNG in its own buffer view after the meshes) per distinct texture
        let mut images: Vec<serde_json::Value> = vec![];
        let mut textures: Vec<serde_json::Value> = vec![];
        let mut img_blobs: Vec<std::sync::Arc<Vec<u8>>> = vec![];
        let mut buffer_views = buffer_views;
        let mut tex_of: Vec<Option<usize>> = vec![];
        for m in &self.materials {
            let Some(t) = &m.texture else { tex_of.push(None); continue };
            let ti = match img_blobs.iter().position(|b| std::sync::Arc::ptr_eq(b, t)) {
                Some(i) => i,
                None => {
                    buffer_views.push(json!({"buffer":0,"byteOffset":bin,"byteLength":t.len()}));
                    images.push(json!({"bufferView": buffer_views.len() - 1, "mimeType": "image/png"}));
                    textures.push(json!({"source": images.len() - 1, "sampler": 0}));
                    bin += t.len();
                    while bin % 4 != 0 { bin += 1; }
                    img_blobs.push(t.clone());
                    img_blobs.len() - 1
                }
            };
            tex_of.push(Some(ti));
        }
        let materials: Vec<_> = self
            .materials
            .iter()
            .enumerate()
            .map(|(mi, m)| {
                let mut j = json!({"name":m.name,
                    "pbrMetallicRoughness":{"baseColorFactor":m.color,"metallicFactor":m.metallic,"roughnessFactor":m.roughness},
                    "doubleSided": true});
                if let Some(ti) = tex_of[mi] {
                    j["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": ti});
                    // (alpha_cutoff 0: an opaque texture -- roof shingles; leaf cards keep the mask)
                    if m.alpha_cutoff > 0.0 {
                        j["alphaMode"] = json!("MASK");
                        j["alphaCutoff"] = json!(m.alpha_cutoff);
                    }
                }
                if m.emissive_strength > 0.0 {
                    j["emissiveFactor"] = json!(m.emissive);
                    j["extensions"] = json!({"KHR_materials_emissive_strength":{"emissiveStrength":m.emissive_strength}});
                }
                j
            })
            .collect();
        assert!(bin % 4 == 0);
        let nodes_json: Vec<_> = self.nodes.iter().map(|n| json!({"name": n.name, "class": n.class, "material": self.materials[n.material].name, "spec": self.materials[n.material].spec, "pose": n.pose})).collect();
        // the plain (non-spec) material values, for importers that take geometry only (usdc)
        let materials_json: Vec<_> = self.materials.iter().map(|m| json!({"name": m.name, "color": m.color, "metallic": m.metallic, "roughness": m.roughness, "emissive": m.emissive, "emissive_strength": m.emissive_strength})).collect();
        let lights_json: Vec<_> = self.lights.iter().map(|l| json!({"name": l.name, "loc": l.loc, "watt": l.watt, "radius": l.radius, "temperature": l.temperature})).collect();
        let mut sidecar = json!({"nodes": nodes_json, "lights": lights_json, "cloth": self.cloth, "materials": materials_json});
        if !self.lights_off.is_empty() {
            sidecar["lights_off"] = self.lights_off.iter().map(|l| json!({"name": l.name, "loc": l.loc, "watt": l.watt, "radius": l.radius, "temperature": l.temperature})).collect();
        }
        let gltf = json!({
            "asset":{"version":"2.0","generator":"infinigen-rs"},
            "extensionsUsed":["KHR_materials_emissive_strength"],
            "scene":0,"scenes":[{"nodes":[root]}],
            "nodes":nodes,"meshes":meshes,"materials":materials,
            "accessors":accessors,"bufferViews":buffer_views,
            "buffers":[{"byteLength":bin}]
        });
        let mut gltf = gltf;
        if !images.is_empty() {
            gltf["images"] = json!(images); gltf["textures"] = json!(textures);
            gltf["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}]);
        }
        let mut js = serde_json::to_vec(&gltf).unwrap();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin;
        if total > u32::MAX as usize { return Err(std::io::Error::new(std::io::ErrorKind::Other, "glb larger than 4 GiB")); }
        use std::io::Write;
        let mut out = open()?;
        out.write_all(b"glTF")?;
        out.write_all(&2u32.to_le_bytes())?;
        out.write_all(&(total as u32).to_le_bytes())?;
        out.write_all(&(js.len() as u32).to_le_bytes())?;
        out.write_all(b"JSON")?;
        out.write_all(&js)?;
        out.write_all(&(bin as u32).to_le_bytes())?;
        out.write_all(b"BIN\0")?;
        for bl in blobs {
            out.write_all(&bl.pos)?;
            out.write_all(&bl.nrm)?;
            if let Some(u) = &bl.uv { out.write_all(u)?; }
            out.write_all(&bl.idx)?;
        }
        for b in &img_blobs {
            out.write_all(b)?;
            let pad = (4 - b.len() % 4) % 4;
            out.write_all(&[0u8; 3][..pad])?;
        }
        out.flush()?;
        Ok(sidecar)
    }
}

impl Mesh {
    pub fn cuboid(lo: V3, hi: V3) -> Mesh {
        let mut m = Mesh::new();
        let c = |x: usize, y: usize, z: usize| [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }];
        m.add_quad(c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0));
        m.add_quad(c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1));
        m.add_quad(c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1));
        m.add_quad(c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0));
        m.add_quad(c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0));
        m.add_quad(c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1));
        m
    }

    /// vertical cylinder (axis z) with optional different top radius (cone frustum), capped
    pub fn frustum(cx: f64, cy: f64, z0: f64, z1: f64, r0: f64, r1: f64, n: usize) -> Mesh {
        let mut m = Mesh::new();
        let pt = |i: usize, r: f64, z: f64| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), z]
        };
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_quad(pt(i, r0, z0), pt(j, r0, z0), pt(j, r1, z1), pt(i, r1, z1));
            if r1 > 0.0 {
                m.add_tri([cx, cy, z1], pt(i, r1, z1), pt(j, r1, z1));
            }
            if r0 > 0.0 {
                m.add_tri([cx, cy, z0], pt(j, r0, z0), pt(i, r0, z0));
            }
        }
        m
    }

    pub fn cylinder(cx: f64, cy: f64, z0: f64, z1: f64, r: f64, n: usize) -> Mesh {
        Mesh::frustum(cx, cy, z0, z1, r, r, n)
    }

    /// UV sphere / ellipsoid centred at c with radii r
    pub fn ellipsoid(c: V3, r: V3, nu: usize, nv: usize) -> Mesh {
        let mut m = Mesh::new();
        let p = |i: usize, j: usize| {
            let th = std::f64::consts::PI * j as f64 / nv as f64;
            let ph = 2.0 * std::f64::consts::PI * i as f64 / nu as f64;
            [c[0] + r[0] * th.sin() * ph.cos(), c[1] + r[1] * th.sin() * ph.sin(), c[2] + r[2] * th.cos()]
        };
        for j in 0..nv {
            for i in 0..nu {
                let i2 = (i + 1) % nu;
                m.add_quad(p(i, j), p(i, j + 1), p(i2, j + 1), p(i2, j));
            }
        }
        m
    }
}

impl Mesh {
    /// Surface of revolution around the z axis through (cx, cy): `profile` is (radius, z) from bottom to top, n segments.
    pub fn revolve(cx: f64, cy: f64, profile: &[(f64, f64)], n: usize) -> Mesh {
        let mut m = Mesh::new();
        let p = |i: usize, k: usize| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + profile[k].0 * a.cos(), cy + profile[k].0 * a.sin(), profile[k].1]
        };
        for k in 0..profile.len() - 1 {
            for i in 0..n {
                let j = (i + 1) % n;
                m.add_quad(p(i, k), p(j, k), p(j, k + 1), p(i, k + 1));
            }
        }
        m
    }

    /// Thick-walled vessel: outer profile + inner profile (offset inward by `t`), open at the top, closed at the bottom.
    pub fn vessel(cx: f64, cy: f64, outer: &[(f64, f64)], t: f64, n: usize) -> Mesh {
        let mut m = Mesh::revolve(cx, cy, outer, n);
        let top = outer.len() - 1;
        let mut inner: Vec<(f64, f64)> = outer.iter().map(|&(r, z)| ((r - t).max(0.0), z)).collect();
        inner[0].1 = outer[0].1 + t;
        // inner surface faces inward: reverse order so normals point into the cavity
        let rev = Mesh::revolve(cx, cy, &inner, n).flipped();
        m.append(&rev);
        // rim ring
        let ring = |i: usize, r: f64, z: f64| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), z]
        };
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_quad(ring(i, inner[top].0, outer[top].1), ring(j, inner[top].0, outer[top].1), ring(j, outer[top].0, outer[top].1), ring(i, outer[top].0, outer[top].1));
        }
        // bottom disk
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_tri([cx, cy, outer[0].1], ring(j, outer[0].0, outer[0].1), ring(i, outer[0].0, outer[0].1));
        }
        m
    }

    /// Two-sided ribbon along a polyline `spine` with per-point half width `w` and a fixed side direction `side`.
    pub fn ribbon(spine: &[V3], w: &[f64], side: &[V3]) -> Mesh {
        let mut m = Mesh::new();
        for k in 0..spine.len() - 1 {
            let a0 = [spine[k][0] - side[k][0] * w[k], spine[k][1] - side[k][1] * w[k], spine[k][2] - side[k][2] * w[k]];
            let a1 = [spine[k][0] + side[k][0] * w[k], spine[k][1] + side[k][1] * w[k], spine[k][2] + side[k][2] * w[k]];
            let b0 = [spine[k + 1][0] - side[k + 1][0] * w[k + 1], spine[k + 1][1] - side[k + 1][1] * w[k + 1], spine[k + 1][2] - side[k + 1][2] * w[k + 1]];
            let b1 = [spine[k + 1][0] + side[k + 1][0] * w[k + 1], spine[k + 1][1] + side[k + 1][1] * w[k + 1], spine[k + 1][2] + side[k + 1][2] * w[k + 1]];
            m.add_quad(a0, a1, b1, b0);
            m.add_quad(a0, b0, b1, a1);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_is_one_face_and_triangulates_like_before() {
        let mut m = Mesh::new();
        m.add_quad([0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]);
        assert_eq!(m.pos.len(), 4);
        assert_eq!(m.faces, vec![vec![0, 1, 2, 3]]);
        assert_eq!(m.triangles(), vec![[0, 1, 2], [0, 2, 3]]);
        m.add_tri([0., 0., 1.], [1., 0., 1.], [0., 1., 1.]);
        assert_eq!(m.triangles().len(), 3);
        assert!((m.area() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn weld_cube() {
        let m = Mesh::cuboid([0., 0., 0.], [1., 1., 1.]);
        assert_eq!(m.pos.len(), 24);
        let w = m.welded(1e-6);
        assert_eq!(w.pos.len(), 8);
        assert_eq!(w.faces.len(), 6);
        assert_eq!(w.to_quadmesh_edges().len(), 12);
        assert_eq!(m.triangles().len(), 12);
    }

    #[test]
    fn weld_drops_degenerate() {
        let mut m = Mesh::new();
        m.add_tri([0., 0., 0.], [0., 0., 0.], [1., 0., 0.]);
        m.add_quad([0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1e-9]);
        m.weld(1e-6);
        assert_eq!(m.faces.len(), 1);
        assert_eq!(m.faces[0].len(), 3);
    }
}

/// One scene node as exported: welded triangle mesh (f32 little-endian bytes: positions, smooth-by-angle normals, uvs, u32 indices).
pub struct NodeBlob {
    pub lo: V3,
    pub hi: V3,
    pub nv: usize,
    pub ntri: usize,
    pub pos: Vec<u8>,
    pub nrm: Vec<u8>,
    pub uv: Option<Vec<u8>>,
    pub idx: Vec<u8>,
}
type Blob = NodeBlob;

impl Scene {
    /// the exported data of every node (glb and usdc writers), built in parallel, in node order
    pub fn node_blobs(&self, ph: &[std::sync::atomic::AtomicU64; 4]) -> Vec<NodeBlob> {
        use rayon::prelude::*;
        let lapn = |i: usize, t: &mut web_time::Instant| { ph[i].fetch_add(t.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed); *t = web_time::Instant::now(); };
        let lapn = |i: usize, t: &mut web_time::Instant| { ph[i].fetch_add(t.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed); *t = web_time::Instant::now(); };
        self.nodes.par_iter().map(|n| {
            let mut tt = web_time::Instant::now();
            // (no T-junction repair outdoors: terrain grids have none, and the foliage is millions of open leaf-card edges -- the
            // repair took ~55 s of a 90 s outdoor house for nothing)
            let (xpos, xuv, xtris, xface) = n.mesh.export_tris(!n.class.starts_with("Outdoor"));
            lapn(0, &mut tt);
            let m = &Mesh { pos: xpos, faces: vec![], uv: xuv };
            let tris = &xtris;
            let (lo, hi) = m.bounds();
            // smooth-by-angle normals: weld by position, average face normals within 40 degrees of the vertex's own face
            let mut normals = vec![[0f32; 3]; m.pos.len()];
            {
                let key = |p: V3| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64);
                let fnorm: Vec<V3> = tris.iter().map(|t| cross(sub(m.pos[t[1] as usize], m.pos[t[0] as usize]), sub(m.pos[t[2] as usize], m.pos[t[0] as usize]))).collect();
                // vertices welded by position: sort (key, tri, corner) so each group lists its triangles in ascending order
                let mut ent: Vec<((i64, i64, i64), u32, u32)> = Vec::with_capacity(tris.len() * 3);
                for (ti, t) in tris.iter().enumerate() {
                    for (c, &v) in t.iter().enumerate() {
                        ent.push((key(m.pos[v as usize]), ti as u32, c as u32));
                    }
                }
                ent.sort_unstable();
                let flen: Vec<f64> = fnorm.iter().map(|f| (f[0].powi(2) + f[1].powi(2) + f[2].powi(2)).sqrt().max(1e-18)).collect();
                let cos_t = 40f64.to_radians().cos();
                let mut g0 = 0;
                while g0 < ent.len() {
                    let mut g1 = g0 + 1;
                    while g1 < ent.len() && ent[g1].0 == ent[g0].0 { g1 += 1; }
                    for &(_, ti, c) in &ent[g0..g1] {
                        let ti = ti as usize;
                        let nl = flen[ti];
                        let nu = [fnorm[ti][0] / nl, fnorm[ti][1] / nl, fnorm[ti][2] / nl];
                        let mut acc = [0f64; 3];
                        for &(_, si, _) in &ent[g0..g1] {
                            let si = si as usize;
                            let d = (fnorm[si][0] * nu[0] + fnorm[si][1] * nu[1] + fnorm[si][2] * nu[2]) / flen[si];
                            if d >= cos_t {
                                for k in 0..3 {
                                    acc[k] += fnorm[si][k];
                                }
                            }
                        }
                        let v = tris[ti][c as usize] as usize;
                        for k in 0..3 {
                            normals[v][k] = acc[k] as f32;
                        }
                    }
                    g0 = g1;
                }
            }
            if let Some(b) = &n.bend {
                for (ti, t) in tris.iter().enumerate() {
                    let Some(c) = b.get(xface[ti] as usize) else { continue };
                    for &v in t {
                        let v = v as usize;
                        let d = sub(m.pos[v], *c);
                        let dl = dot(d, d).sqrt().max(1e-9);
                        let nv = normals[v];
                        let nl = ((nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]) as f64).sqrt().max(1e-12);
                        // (a card's own normal may face either way: flip it to the outward side before blending)
                        let mut own = [nv[0] as f64 / nl, nv[1] as f64 / nl, nv[2] as f64 / nl];
                        if dot(own, d) < 0.0 { own = [-own[0], -own[1], -own[2]]; }
                        let b2 = [0.25 * own[0] + 0.75 * d[0] / dl, 0.25 * own[1] + 0.75 * d[1] / dl, 0.25 * own[2] + 0.75 * d[2] / dl];
                        normals[v] = [b2[0] as f32, b2[1] as f32, b2[2] as f32];
                    }
                }
            }
            lapn(1, &mut tt);
            let generated_uv: Vec<[f64; 2]>;
            let uvs: &Vec<[f64; 2]> = if m.uv.len() == m.pos.len() {
                &m.uv
            } else {
                // box projection per triangle (1 uv unit = 1 metre)
                let mut uv = vec![[0.0, 0.0]; m.pos.len()];
                for t in tris {
                    let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                    let n = cross(sub(b, a), sub(c, a));
                    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
                    for &i in t {
                        let p = m.pos[i as usize];
                        uv[i as usize] = if az >= ax && az >= ay { [p[0], p[1]] } else if ax >= ay { [p[1], p[2]] } else { [p[0], p[2]] };
                    }
                }
                generated_uv = uv;
                &generated_uv
            };

            lapn(2, &mut tt);
            // lossless weld: soup vertices with identical f32 position / normal / uv share one index
            let has_uv = uvs.len() == m.pos.len();
            let mut map: FxMap<[u32; 8], u32> = FxMap::default();
            let (mut pos, mut nrm, mut uvb, mut idx) = (vec![], vec![], vec![], Vec::with_capacity(tris.len() * 12));
            let mut remap = vec![0u32; m.pos.len()];
            let mut nv = 0u32;
            for (i, p) in m.pos.iter().enumerate() {
                let nn = normals[i];
                let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt().max(1e-12);
                let nn = [nn[0] / l, nn[1] / l, nn[2] / l];
                let pf = [p[0] as f32, p[1] as f32, p[2] as f32];
                let uf = if has_uv { [uvs[i][0] as f32, uvs[i][1] as f32] } else { [0.0, 0.0] };
                let key = [pf[0].to_bits(), pf[1].to_bits(), pf[2].to_bits(), nn[0].to_bits(), nn[1].to_bits(), nn[2].to_bits(), uf[0].to_bits(), uf[1].to_bits()];
                remap[i] = *map.entry(key).or_insert_with(|| {
                    for k in 0..3 { pos.extend_from_slice(&pf[k].to_le_bytes()); }
                    for k in 0..3 { nrm.extend_from_slice(&nn[k].to_le_bytes()); }
                    if has_uv { for k in 0..2 { uvb.extend_from_slice(&uf[k].to_le_bytes()); } }
                    nv += 1;
                    nv - 1
                });
            }
            for t in tris { for &i in t { idx.extend_from_slice(&remap[i as usize].to_le_bytes()); } }
            lapn(3, &mut tt);
            Blob { lo, hi, nv: nv as usize, ntri: tris.len(), pos, nrm, uv: if has_uv { Some(uvb) } else { None }, idx }
        }).collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Scene {
    /// Binary USD (.usdc) of the same meshes as the glb: one Mesh prim per node under /World, named n<index> in node order (the sidecar's
    /// `nodes` give the real names and materials), Z-up metres, points welded by position with face-varying normals and uvs (what a glTF
    /// import with merge_vertices gives). No materials: the renderer assigns them from the sidecar.
    pub fn write_usdc(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.write_usdc_blobs(path, &self.node_blobs(&Default::default()))
    }

    fn write_usdc_blobs(&self, path: &std::path::Path, blobs: &[NodeBlob]) -> std::io::Result<()> {
        use openusd::gf::{Vec2f, Vec3f};
        use openusd::sdf::{self, ChildrenKey, FieldKey, SpecType, Specifier, Value};
        fn err<E: std::fmt::Debug>(e: E) -> std::io::Error { std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}")) }
        let f3 = |b: &[u8], i: usize| -> [f32; 3] { [0, 1, 2].map(|k| f32::from_le_bytes(b[(3 * i + k) * 4..(3 * i + k) * 4 + 4].try_into().unwrap())) };
        let f2 = |b: &[u8], i: usize| -> [f32; 2] { [0, 1].map(|k| f32::from_le_bytes(b[(2 * i + k) * 4..(2 * i + k) * 4 + 4].try_into().unwrap())) };
        let mut d = sdf::Data::new();
        let names: Vec<String> = (0..self.nodes.len()).map(|i| format!("n{i}")).collect();
        let r = d.create_spec(sdf::Path::abs_root(), SpecType::PseudoRoot);
        r.add(ChildrenKey::PrimChildren, Value::TokenVec(vec!["World".into()]));
        r.add(FieldKey::DefaultPrim, Value::Token("World".into()));
        r.add("upAxis", Value::Token("Z".into()));
        r.add("metersPerUnit", Value::Double(1.0));
        let w = d.create_spec(sdf::Path::new("/World").map_err(err)?, SpecType::Prim);
        w.add(FieldKey::Specifier, Value::Specifier(Specifier::Def));
        w.add(FieldKey::TypeName, Value::Token("Xform".into()));
        w.add(ChildrenKey::PrimChildren, Value::TokenVec(names.iter().map(|n| n.as_str().into()).collect()));
        let attr = |d: &mut sdf::Data, path: String, ty: &str, v: Value, interp: Option<&str>| -> std::io::Result<()> {
            let s = d.create_spec(sdf::Path::new(&path).map_err(err)?, SpecType::Attribute);
            s.add(FieldKey::TypeName, Value::Token(ty.into()));
            s.add(FieldKey::Default, v);
            if let Some(i) = interp { s.add("interpolation", Value::Token(i.into())); }
            Ok(())
        };
        for (bl, name) in blobs.iter().zip(&names) {
            let base = format!("/World/{name}");
            let m = d.create_spec(sdf::Path::new(&base).map_err(err)?, SpecType::Prim);
            m.add(FieldKey::Specifier, Value::Specifier(Specifier::Def));
            m.add(FieldKey::TypeName, Value::Token("Mesh".into()));
            let mut props: Vec<openusd::tf::Token> = vec!["points".into(), "faceVertexCounts".into(), "faceVertexIndices".into(), "normals".into(), "subdivisionScheme".into(), "doubleSided".into()];
            if bl.uv.is_some() { props.push("primvars:st".into()); }
            m.add(ChildrenKey::PropertyChildren, Value::TokenVec(props));
            // weld as Blender's glTF importer does with merge_vertices (io_scene_gltf2 merge_duplicate_verts): vertices with the same
            // position and the same normal truncated to 1/50000 (float32) become one, uvs stay per corner; the normal kept per vertex
            // is the truncated one (the importer sets the custom normals from it). -0.0 counts as 0.0, like its byte-wise unique.
            let z = |v: f32| if v == 0.0 { 0.0f32 } else { v };
            let trunc = |v: f32| z((v * 50000.0f32).trunc() * (1.0f64 / 50000.0) as f32);
            let mut map: crate::dhash::FxMap<[u32; 6], i32> = Default::default();
            let (mut pts, mut nrms): (Vec<Vec3f>, Vec<Vec3f>) = (vec![], vec![]);
            let remap: Vec<i32> = (0..bl.nv).map(|i| {
                let p = f3(&bl.pos, i).map(z);
                let n = f3(&bl.nrm, i).map(trunc);
                let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), n[0].to_bits(), n[1].to_bits(), n[2].to_bits()];
                *map.entry(key).or_insert_with(|| { pts.push(Vec3f::from(p)); nrms.push(Vec3f::from(n)); pts.len() as i32 - 1 })
            }).collect();
            let corners: Vec<usize> = bl.idx.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect();
            attr(&mut d, format!("{base}.points"), "point3f[]", Value::Vec3fVec(pts), None)?;
            attr(&mut d, format!("{base}.faceVertexCounts"), "int[]", Value::IntVec(vec![3; bl.ntri]), None)?;
            attr(&mut d, format!("{base}.faceVertexIndices"), "int[]", Value::IntVec(corners.iter().map(|&c| remap[c]).collect()), None)?;
            attr(&mut d, format!("{base}.normals"), "normal3f[]", Value::Vec3fVec(nrms), Some("vertex"))?;
            if let Some(uv) = &bl.uv {
                // the glTF importer flips v (glTF's uv origin is top-left): the same values as after a glb import
                attr(&mut d, format!("{base}.primvars:st"), "texCoord2f[]", Value::Vec2fVec(corners.iter().map(|&c| { let t = f2(uv, c); Vec2f::from([t[0], 1.0 - t[1]]) }).collect()), Some("faceVarying"))?;
            }
            attr(&mut d, format!("{base}.subdivisionScheme"), "token", Value::Token("none".into()), None)?;
            attr(&mut d, format!("{base}.doubleSided"), "bool", Value::Bool(true), None)?;
        }
        openusd::usdc::CrateWriter::write_to_file(&d, path).map_err(err)
    }
}

/// Minimal RGBA8 PNG encoder (zlib via flate2): procedural textures for the glb (leaf cards).
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    use std::io::Write;
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data { c ^= b as u32; for _ in 0..8 { c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 }; } }
        !c
    }
    fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec(); c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize { raw.push(0u8); raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]); }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).unwrap();
    let idat = z.finish().unwrap();
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = vec![]; ihdr.extend_from_slice(&w.to_be_bytes()); ihdr.extend_from_slice(&h.to_be_bytes()); ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr); chunk(&mut out, b"IDAT", &idat); chunk(&mut out, b"IEND", &[]);
    out
}
