//! tableware/pan.py make_base (cage -> handle -> SOLIDIFY -> guard ExtrudeMesh -> SUBSURF simple 1 + CC 3), geometric port for the asset
//! bounding box. The cap topology (fill_grid + triangulation) is replaced by a fan: it cannot influence the extremes.
use super::bathfree::solidify;
use super::subsurf::subsurf;
use crate::mesh::Mesh;
use crate::dhash::HashMap;

type V3 = [f64; 3];

pub struct PanParams {
    pub r_expand: f64,
    pub r_mid: f64,
    pub depth: f64,
    pub x_handle: f64,
    pub z_handle: f64,
    pub z_handle_mid: f64,
    pub s_handle: f64,
    pub thickness: f64,
    pub has_guard: bool,
    pub x_guard: f64,
    pub guard_depth: f64,
    pub n: usize,
    pub fill_offset: usize,
    pub has_handle: bool,
    /// handle hole: (centre x, radius)
    pub hole: Option<(f64, f64)>,
}

fn face_normal(m: &Mesh, f: &[u32]) -> V3 {
    let k = f.len();
    let mut n = [0.0; 3];
    for i in 0..k {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % k] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l == 0.0 {
        return [0.0; 3];
    }
    [n[0] / l, n[1] / l, n[2] / l]
}

/// ExtrudeMesh node, mesh mode FACES with individual = false, default (normal) offset times `scale`
pub fn extrude_regions(m: &Mesh, sel: &[bool], scale: f64) -> Mesh {
    let nvo = m.pos.len();
    let mut cnt: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            cnt.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    // vertex offsets: mean of selected face offsets
    let mut off = vec![[0.0f64; 3]; nvo];
    let mut c = vec![0u32; nvo];
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        let n = face_normal(m, f);
        for &v in f {
            for k in 0..3 {
                off[v as usize][k] += n[k] * scale;
            }
            c[v as usize] += 1;
        }
    }
    let mut o = m.clone();
    // boundary edges: exactly one selected face
    let mut bedges: Vec<(u32, u32, usize)> = vec![];
    for f_i in 0..m.faces.len() {
        if !sel[f_i] {
            continue;
        }
        let f = &m.faces[f_i];
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let fs = &cnt[&(a.min(b), a.max(b))];
            if fs.iter().filter(|&&x| sel[x]).count() == 1 {
                bedges.push((a, b, f_i));
            }
        }
    }
    let mut dup: HashMap<u32, u32> = HashMap::default();
    for &(a, b, _) in &bedges {
        for v in [a, b] {
            dup.entry(v).or_insert_with(|| {
                o.pos.push(m.pos[v as usize]);
                (o.pos.len() - 1) as u32
            });
        }
    }
    // edges inside the region that are also used by deselected faces (non-manifold input): their vertices are duplicated too
    let mut inner: Vec<(u32, u32)> = cnt
        .iter()
        .filter(|(_, fs)| fs.iter().filter(|&&x| sel[x]).count() > 1 && fs.iter().any(|&x| !sel[x]))
        .map(|(k, _)| *k)
        .collect();
    inner.sort();
    for (a, b) in inner {
        for v in [a, b] {
            dup.entry(v).or_insert_with(|| {
                o.pos.push(m.pos[v as usize]);
                (o.pos.len() - 1) as u32
            });
        }
    }
    // move: duplicated -> the new vertex; others (interior of the region) -> in place
    for v in 0..nvo {
        if c[v] == 0 {
            continue;
        }
        let t = if let Some(&d) = dup.get(&(v as u32)) { d as usize } else { v };
        for k in 0..3 {
            o.pos[t][k] += off[v][k] / c[v] as f64;
        }
    }
    for fi in 0..o.faces.len() {
        if !sel[fi] {
            continue;
        }
        for v in o.faces[fi].iter_mut() {
            if let Some(&d) = dup.get(v) {
                *v = d;
            }
        }
    }
    for &(a, b, _) in &bedges {
        o.faces.push(vec![a, b, dup[&b], dup[&a]]);
    }
    o
}

/// the bbox of Pan.create_asset before the final scale: (lo, hi) in cage units after origin2lowest (z shifted so min z = 0)
pub fn pan_unit_mesh(p: &PanParams) -> Option<Mesh> {
    let n = p.n;
    let mut m = Mesh::new();
    let rings = [(0.0, 1.0), (p.depth / 2.0, p.r_mid), (p.depth, p.r_expand)];
    let rot = std::f64::consts::PI / n as f64;
    for &(z, r) in &rings {
        for i in 0..n {
            let phi = 2.0 * std::f64::consts::PI * i as f64 / n as f64 + rot;
            m.pos.push([-r * phi.sin(), r * phi.cos(), z]);
        }
    }
    let id = |k: usize, i: usize| (k * n + (i % n)) as u32;
    let pv = |i: usize| (i + n - 1) % n;
    for k in 0..2 {
        for i in 0..n {
            m.faces.push(vec![id(k + 1, i), id(k + 1, pv(i)), id(k, pv(i)), id(k, i)]);
        }
    }
    // bottom cap: fill_grid on the base ring (loop rotated by `offset`), quads split by BEAUTY, normals pointing down
    {
        // python fills the unrotated ring (bpy circle: (-sin phi, cos phi), phi accumulated in f32); the start vertex is the first one with
        // the largest f32 angle deviation, then rotated by `offset`
        // BM_mesh_edgeloops_find starts the loop list at e->v1 of the first mesh edge (vertex 1) and walks forward: 1, 2, .., n-1, 0
        let c32: Vec<[f32; 3]> = (0..n).map(|i| { let (s, c) = super::cabnodes::sin_cos_from_fraction(i, n); [-s, c, 0.0f32] }).collect();
        let mut best = (-1.0f32, 0usize);
        for j in 0..n {
            let i = (1 + j) % n;
            let a = (std::f32::consts::PI - crate::bm::query::angle_v3v3v3(c32[(i + n - 1) % n], c32[i], c32[(i + 1) % n])).abs();
            if a > best.0 || j == 0 {
                best = (a, j);
            }
        }
        // `best.1` counts along the loop list; the ring below is indexed by vertex: list position j <-> vertex (1 + j) % n
        best.1 = (1 + best.1) % n;
        let start = (best.1 + p.fill_offset) % n;
        let ring: Vec<V3> = (0..n).map(|i| m.pos[id(0, (i + start) % n) as usize]).collect();
        let idx: Vec<u32> = (0..n).map(|i| id(0, (i + start) % n)).collect();
        let mut grid = super::bpyops::fill_grid_loop_ex(&ring, true);
        let nz: f64 = grid.faces.iter().map(|f| super::bpyops::face_normal(&grid, f)[2]).sum();
        if nz > 0.0 {
            for f in grid.faces.iter_mut() {
                f.reverse();
            }
        }
        // grid vertices on the ring map to the ring vertices; the rest are new
        let mut gmap = vec![0u32; grid.pos.len()];
        for (gi, gp) in grid.pos.iter().enumerate() {
            let hit = ring.iter().position(|rp| rp == gp);
            gmap[gi] = match hit {
                Some(k) => idx[k],
                None => {
                    m.pos.push(*gp);
                    (m.pos.len() - 1) as u32
                }
            };
        }
        for f in &grid.faces {
            let q = [m.pos[gmap[f[0] as usize] as usize], m.pos[gmap[f[1] as usize] as usize], m.pos[gmap[f[2] as usize] as usize], m.pos[gmap[f[3] as usize] as usize]];
            let q32 = q.map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]);
            let v = |k: usize| gmap[f[k] as usize];
            if super::polyfill::quad_split_24_beauty(q32) {
                m.faces.push(vec![v(0), v(1), v(2)]);
                m.faces.push(vec![v(0), v(2), v(3)]);
            } else {
                m.faces.push(vec![v(0), v(1), v(3)]);
                m.faces.push(vec![v(1), v(2), v(3)]);
            }
        }
    }
    dbg_bbox("filled", &m);
    if p.has_handle {
    // handle: edge with the largest u.x + v.x + u.z + v.z
    let mut best = (f64::NEG_INFINITY, 0u32, 0u32, 0usize);
    let mut ecount: HashMap<(u32, u32), (u32, u32, usize)> = HashMap::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            // BMesh e->l is the most recently attached loop: its direction defines the extrusion orientation
            let e = ecount.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
            e.0 = a;
            e.1 = b;
            e.2 += 1;
        }
    }
    let mut keys: Vec<_> = ecount.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let e = ecount[&key];
        let (u, v) = (m.pos[e.0 as usize], m.pos[e.1 as usize]);
        let s = u[0] + v[0] + u[2] + v[2];
        if s > best.0 {
            best = (s, e.0, e.1, e.2);
        }
    }
    let (mut a, mut b) = (best.1, best.2);
    let mut extrude = |m: &mut Mesh, a: &mut u32, b: &mut u32, t: V3| {
        let (pa, pb) = (m.pos[*a as usize], m.pos[*b as usize]);
        let a1 = m.pos.len() as u32;
        m.pos.push([pa[0] + t[0], pa[1] + t[1], pa[2] + t[2]]);
        let b1 = m.pos.len() as u32;
        m.pos.push([pb[0] + t[0], pb[1] + t[1], pb[2] + t[2]]);
        m.faces.push(vec![*b, *a, a1, b1]);
        *a = a1;
        *b = b1;
    };
    extrude(&mut m, &mut a, &mut b, [p.x_handle * 0.5, 0.0, p.z_handle_mid]);
    extrude(&mut m, &mut a, &mut b, [p.x_handle * 0.5, 0.0, p.z_handle - p.z_handle_mid]);
    // resize(s_handle) about the median of the selected front edge
    let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
    let med = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0];
    for v in [a, b] {
        for k in 0..3 {
            m.pos[v as usize][k] = med[k] + (m.pos[v as usize][k] - med[k]) * p.s_handle;
        }
    }
    extrude(&mut m, &mut a, &mut b, [1e-3, 0.0, 0.0]);
    }
    round32(&mut m);
    dbg_bbox("handle", &m);
    let mut m = solidify(&m, p.thickness, 1.0);
    round32(&mut m);
    dbg_bbox("solid", &m);
    if p.has_guard {
        let sel: Vec<bool> = m
            .faces
            .iter()
            .map(|f| f.iter().map(|&v| m.pos[v as usize][0]).sum::<f64>() / f.len() as f64 > p.x_guard)
            .collect();
        m = extrude_regions(&m, &sel, p.guard_depth);
        round32(&mut m);
    }
    dbg_bbox("guard", &m);
    let m = subsurf(&m, 1, true);
    dbg_bbox("simple1", &m);
    let mut m = subsurf(&m, 3, false);
    dbg_bbox("cc3", &m);
    if let Some((cx, r)) = p.hole {
        round32(&mut m);
        m = super::prismcut::prism_difference(&m, &super::prismcut::cylinder_corners(cx, r));
        dbg_bbox("hole", &m);
    }
    Some(m)
}

fn dbg_bbox(name: &str, m: &Mesh) {
    if std::env::var("PANDBG").is_err() {
        return;
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for q in &m.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    if let Ok(dir) = std::env::var("PANDBG") {
        let j = serde_json::json!({"v": m.pos, "p": m.faces});
        let _ = std::fs::write(format!("{dir}/rs_{name}.json"), j.to_string());
    }
    eprintln!("{name}: nv {} nf {} lo {:?} hi {:?}", m.pos.len(), m.faces.len(), lo, hi);
}

fn round32(m: &mut Mesh) {
    for p in m.pos.iter_mut() {
        for c in p.iter_mut() {
            *c = *c as f32 as f64;
        }
    }
}

/// bounds of `m` minus a vertical 32-gon prism (bpy cylinder, vertices at multiples of 11.25 degrees, radius `r`, centre (cx, 0)),
/// i.e. of the boolean DIFFERENCE with a cutter spanning all z: vertices outside + crossings of mesh edges with the prism facets +
/// crossings of the prism corner lines with the mesh faces.
pub fn bounds_minus_prism(m: &Mesh, cx: f64, r: f64) -> (V3, V3) {
    let n = 32usize;
    let corner = |i: usize| -> [f64; 2] {
        let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        [cx + r * t.sin(), r * t.cos()]
    };
    let poly: Vec<[f64; 2]> = (0..n).map(corner).collect();
    let inside = |p: &[f64; 3]| -> bool {
        // point in convex polygon
        let mut sgn = 0i32;
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            let s = if c > 0.0 { 1 } else { -1 };
            if sgn == 0 {
                sgn = s;
            } else if s != sgn {
                return false;
            }
        }
        true
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut add = |p: V3| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    let ins: Vec<bool> = m.pos.iter().map(inside).collect();
    for (i, p) in m.pos.iter().enumerate() {
        if !ins[i] {
            add(*p);
        }
    }
    // unique edges
    let mut edges: crate::dhash::HashSet<(u32, u32)> = Default::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            edges.insert((a.min(b), a.max(b)));
        }
    }
    for &(a, b) in &edges {
        if ins[a as usize] == ins[b as usize] {
            // both inside/outside: a convex prism can still be crossed twice, but cc3 edges are far smaller than the hole
            continue;
        }
        let (p, q) = (m.pos[a as usize], m.pos[b as usize]);
        for i in 0..n {
            let (s0, s1) = (poly[i], poly[(i + 1) % n]);
            // segment p-q (xy) vs s0-s1
            let d = [q[0] - p[0], q[1] - p[1]];
            let e = [s1[0] - s0[0], s1[1] - s0[1]];
            let den = d[0] * e[1] - d[1] * e[0];
            if den.abs() < 1e-18 {
                continue;
            }
            let w = [s0[0] - p[0], s0[1] - p[1]];
            let t = (w[0] * e[1] - w[1] * e[0]) / den;
            let u = (w[0] * d[1] - w[1] * d[0]) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                add([p[0] + t * d[0], p[1] + t * d[1], p[2] + t * (q[2] - p[2])]);
            }
        }
    }
    // prism corner lines through the faces (quads split along a diagonal; non-planarity is negligible at this resolution)
    for f in &m.faces {
        let tris: Vec<[u32; 3]> = (1..f.len() - 1).map(|i| [f[0], f[i], f[i + 1]]).collect();
        for t in tris {
            let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
            let (lx, hx) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
            let (ly, hy) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
            for pc in &poly {
                if pc[0] < lx || pc[0] > hx || pc[1] < ly || pc[1] > hy {
                    continue;
                }
                let den = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if den.abs() < 1e-18 {
                    continue;
                }
                let l1 = ((b[1] - c[1]) * (pc[0] - c[0]) + (c[0] - b[0]) * (pc[1] - c[1])) / den;
                let l2 = ((c[1] - a[1]) * (pc[0] - c[0]) + (a[0] - c[0]) * (pc[1] - c[1])) / den;
                let l3 = 1.0 - l1 - l2;
                if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                    add([pc[0], pc[1], l1 * a[2] + l2 * b[2] + l3 * c[2]]);
                }
            }
        }
    }
    (lo, hi)
}
