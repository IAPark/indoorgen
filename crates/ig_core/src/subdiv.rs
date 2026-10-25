//! Blender-compatible Catmull-Clark subdivision and edge bevel on an indexed polygon mesh.
//!
//! Self-contained: defines its own mesh type ([`QMesh`]) and depends only on `std`.
//!
//! * [`catmull_clark`]  - SUBSURF modifier semantics (boundary "smooth", no limit surface),
//!   optional per-edge crease (sharpness in [0,1]; a crease keeps its sharpness at every level).
//! * [`bevel`]          - BEVEL modifier (edges, "Angle" limit method, offset type Offset).
//! * [`bevel_then_subsurf`] - convenience.

use std::collections::BTreeMap;
use crate::dhash::{HashMap, HashSet};

pub type V3 = [f64; 3];

#[derive(Clone, Debug, Default)]
pub struct QMesh {
    pub pos: Vec<V3>,
    /// CCW polygons (len >= 3), vertices shared.
    pub faces: Vec<Vec<u32>>,
}

// ---------------------------------------------------------------- vector helpers
#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
fn normalize(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-300 && l.is_finite() {
        scale(a, 1.0 / l)
    } else {
        [0.0; 3]
    }
}
#[inline]
fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(scale(a, 1.0 - t), scale(b, t))
}
#[inline]
fn ekey(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}
#[inline]
fn ekey64(a: u32, b: u32) -> u64 {
    let (a, b) = ekey(a, b);
    ((a as u64) << 32) | b as u64
}

/// Remove consecutive duplicate ids (cyclically).
fn dedupe_cyclic(v: &mut Vec<u32>) {
    v.dedup();
    while v.len() > 1 && v[0] == *v.last().unwrap() {
        v.pop();
    }
}

fn newell(pos: &[V3], f: &[u32]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let a = pos[f[i] as usize];
        let b = pos[f[(i + 1) % f.len()] as usize];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

// ---------------------------------------------------------------- QMesh
impl QMesh {
    pub fn new(pos: Vec<V3>, faces: Vec<Vec<u32>>) -> QMesh {
        QMesh { pos, faces }
    }

    /// Axis aligned box, outward CCW quads.
    pub fn cuboid(lo: V3, hi: V3) -> QMesh {
        let p = |x: usize, y: usize, z: usize| -> V3 {
            [
                if x == 0 { lo[0] } else { hi[0] },
                if y == 0 { lo[1] } else { hi[1] },
                if z == 0 { lo[2] } else { hi[2] },
            ]
        };
        let mut pos = Vec::new();
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    pos.push(p(x, y, z));
                }
            }
        }
        // index = x + 2y + 4z
        let faces = vec![
            vec![0, 2, 3, 1], // -z
            vec![4, 5, 7, 6], // +z
            vec![0, 1, 5, 4], // -y
            vec![2, 6, 7, 3], // +y
            vec![0, 4, 6, 2], // -x
            vec![1, 3, 7, 5], // +x
        ];
        QMesh { pos, faces }
    }

    /// Capped cylinder along z (n-gon caps, quad sides), outward CCW.
    pub fn cylinder(n: usize, radius: f64, z0: f64, z1: f64) -> QMesh {
        let n = n.max(3);
        let mut pos = Vec::new();
        for z in [z0, z1] {
            for i in 0..n {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                pos.push([radius * a.cos(), radius * a.sin(), z]);
            }
        }
        let mut faces = Vec::new();
        faces.push((0..n as u32).rev().collect());
        faces.push((n as u32..2 * n as u32).collect());
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            faces.push(vec![i, j, j + n as u32, i + n as u32]);
        }
        QMesh { pos, faces }
    }

    /// Append another mesh (no welding).
    pub fn append(&mut self, o: &QMesh) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&o.pos);
        for f in &o.faces {
            self.faces.push(f.iter().map(|&i| i + base).collect());
        }
    }

    /// Merge vertices closer than `eps` (per-axis cell hashing with neighbour search),
    /// then drop consecutive duplicate corners and faces with < 3 corners.
    pub fn weld(&self, eps: f64) -> QMesh {
        let eps = eps.max(1e-300);
        let cell = |p: V3| -> (i64, i64, i64) {
            (
                (p[0] / eps).floor() as i64,
                (p[1] / eps).floor() as i64,
                (p[2] / eps).floor() as i64,
            )
        };
        let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::default();
        let mut npos: Vec<V3> = Vec::new();
        let mut remap = vec![0u32; self.pos.len()];
        for (i, &p) in self.pos.iter().enumerate() {
            let c = cell(p);
            let mut found = None;
            'outer: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(list) = grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                            for &j in list {
                                if len(sub(npos[j as usize], p)) <= eps {
                                    found = Some(j);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
            remap[i] = match found {
                Some(j) => j,
                None => {
                    let j = npos.len() as u32;
                    npos.push(p);
                    grid.entry(c).or_default().push(j);
                    j
                }
            };
        }
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut g: Vec<u32> = f.iter().map(|&i| remap[i as usize]).collect();
            dedupe_cyclic(&mut g);
            if g.len() >= 3 {
                faces.push(g);
            }
        }
        QMesh { pos: npos, faces }
    }

    /// Unique undirected edges as sorted pairs (sorted list).
    pub fn edges(&self) -> Vec<(u32, u32)> {
        let mut e: Vec<(u32, u32)> = Vec::new();
        for f in &self.faces {
            for i in 0..f.len() {
                e.push(ekey(f[i], f[(i + 1) % f.len()]));
            }
        }
        e.sort_unstable();
        e.dedup();
        e
    }

    /// Fan triangulation.
    pub fn triangulate(&self) -> Vec<[u32; 3]> {
        let mut t = Vec::new();
        for f in &self.faces {
            for i in 1..f.len().saturating_sub(1) {
                t.push([f[0], f[i], f[i + 1]]);
            }
        }
        t
    }

    /// Every edge has exactly two faces.
    pub fn is_closed(&self) -> bool {
        let mut c: HashMap<u64, u32> = HashMap::default();
        for f in &self.faces {
            for i in 0..f.len() {
                *c.entry(ekey64(f[i], f[(i + 1) % f.len()])).or_insert(0) += 1;
            }
        }
        !c.is_empty() && c.values().all(|&n| n == 2)
    }

    /// Wavefront OBJ text (1-based indices).
    pub fn to_obj(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        for p in &self.pos {
            let _ = writeln!(s, "v {:.9} {:.9} {:.9}", p[0], p[1], p[2]);
        }
        for f in &self.faces {
            s.push('f');
            for &i in f {
                let _ = write!(s, " {}", i + 1);
            }
            s.push('\n');
        }
        s
    }
}

// ---------------------------------------------------------------- Catmull-Clark
thread_local! {
    static CORNER_FIXED: std::cell::Cell<bool> = std::cell::Cell::new(true);
}
/// boundary corners (valence-2 boundary vertices) stay fixed (SUBSURF modifier / node boundary_smooth PRESERVE_CORNERS) or are smoothed (node boundary_smooth ALL)
pub fn set_corner_fixed(b: bool) -> bool {
    CORNER_FIXED.with(|c| c.replace(b))
}
/// Blender SUBSURF modifier semantics (Catmull-Clark, `use_limit_surface` off, boundary 'smooth').
/// `crease`: per-edge OpenSubdiv sharpness keyed by sorted (u,v) (Blender: 10 * crease^2, so 0..10; use a large value for "sharp at every level").
/// Uniform creasing like OpenSubdiv: an edge with sharpness s >= 1 is fully sharp, 0 < s < 1 blends the smooth and the sharp rule by s, and the
/// child edges get max(s - 1, 0). Open boundaries use the boundary rule.
pub fn catmull_clark(
    m: &QMesh,
    levels: usize,
    crease: Option<&HashMap<(u32, u32), f64>>,
) -> QMesh {
    let mut cur = m.clone();
    let mut cr: HashMap<(u32, u32), f64> = HashMap::default();
    if let Some(c) = crease {
        for (&(a, b), &s) in c {
            if s > 0.0 {
                cr.insert(ekey(a, b), s);
            }
        }
    }
    for _ in 0..levels {
        if cur.faces.is_empty() {
            break;
        }
        let (n, nc) = cc_once(&cur, &cr, false);
        cur = n;
        cr = nc;
    }
    cur
}

/// the same refinement with linear (simple-subdivision) point rules: identical topology and vertex order
pub fn simple_levels(m: &QMesh, levels: usize) -> QMesh {
    let mut cur = m.clone();
    let cr = HashMap::default();
    for _ in 0..levels {
        if cur.faces.is_empty() {
            break;
        }
        cur = cc_once(&cur, &cr, true).0;
    }
    cur
}

fn cc_once(m: &QMesh, cr: &HashMap<(u32, u32), f64>, simple: bool) -> (QMesh, HashMap<(u32, u32), f64>) {
    let nv = m.pos.len();
    let nf = m.faces.len();
    let total: usize = m.faces.iter().map(|f| f.len()).sum();

    // edges
    let mut emap: HashMap<u64, u32> = HashMap::with_capacity_and_hasher(total, Default::default());
    let mut ev: Vec<(u32, u32)> = Vec::with_capacity(total / 2 + 8);
    let mut ecount: Vec<u32> = Vec::with_capacity(total / 2 + 8);
    let mut ef: Vec<[u32; 2]> = Vec::with_capacity(total / 2 + 8);
    let mut foff: Vec<usize> = Vec::with_capacity(nf + 1);
    let mut fedge: Vec<u32> = Vec::with_capacity(total);
    for (fi, f) in m.faces.iter().enumerate() {
        foff.push(fedge.len());
        for i in 0..f.len() {
            let (a, b) = ekey(f[i], f[(i + 1) % f.len()]);
            let key = ((a as u64) << 32) | b as u64;
            let e = *emap.entry(key).or_insert_with(|| {
                ev.push((a, b));
                ecount.push(0);
                ef.push([0; 2]);
                (ev.len() - 1) as u32
            });
            let c = ecount[e as usize];
            if c < 2 {
                ef[e as usize][c as usize] = fi as u32;
            }
            ecount[e as usize] = c + 1;
            fedge.push(e);
        }
    }
    foff.push(fedge.len());
    let ne = ev.len();

    // face points
    let mut fp: Vec<V3> = Vec::with_capacity(nf);
    for f in &m.faces {
        let mut s = [0.0; 3];
        for &i in f {
            s = add(s, m.pos[i as usize]);
        }
        fp.push(scale(s, 1.0 / f.len() as f64));
    }

    // edge points
    let mut ep: Vec<V3> = Vec::with_capacity(ne);
    let mut esharp: Vec<f64> = vec![0.0; ne];
    let mut emid: Vec<V3> = Vec::with_capacity(ne);
    for e in 0..ne {
        let (a, b) = ev[e];
        let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
        let mid = scale(add(pa, pb), 0.5);
        emid.push(mid);
        let s = if ecount[e] != 2 {
            1.0
        } else {
            cr.get(&(a, b)).copied().unwrap_or(0.0).min(1.0)
        };
        esharp[e] = s;
        if ecount[e] == 2 {
            let smooth = scale(
                add(add(pa, pb), add(fp[ef[e][0] as usize], fp[ef[e][1] as usize])),
                0.25,
            );
            ep.push(if simple { mid } else if s > 0.0 { lerp(smooth, mid, s) } else { smooth });
        } else {
            ep.push(mid);
        }
    }

    // vertex accumulators
    let mut fsum = vec![[0.0; 3]; nv];
    let mut fcnt = vec![0u32; nv];
    for (fi, f) in m.faces.iter().enumerate() {
        for &v in f {
            fsum[v as usize] = add(fsum[v as usize], fp[fi]);
            fcnt[v as usize] += 1;
        }
    }
    let mut esum = vec![[0.0; 3]; nv];
    let mut ecnt = vec![0u32; nv];
    let mut cn = vec![0u32; nv];
    let mut ca = vec![0u32; nv];
    let mut cb = vec![0u32; nv];
    let mut csum = vec![0.0f64; nv];
    for e in 0..ne {
        let (a, b) = ev[e];
        for (v, o) in [(a, b), (b, a)] {
            let vi = v as usize;
            esum[vi] = add(esum[vi], emid[e]);
            ecnt[vi] += 1;
            if esharp[e] > 0.0 {
                cn[vi] += 1;
                if cn[vi] == 1 {
                    ca[vi] = o;
                } else if cn[vi] == 2 {
                    cb[vi] = o;
                }
                csum[vi] += esharp[e];
            }
        }
    }

    let mut out = QMesh {
        pos: Vec::with_capacity(nv + ne + nf),
        faces: Vec::with_capacity(total),
    };
    for v in 0..nv {
        let p = m.pos[v];
        if simple || fcnt[v] == 0 || ecnt[v] == 0 {
            out.pos.push(p);
            continue;
        }
        let n = fcnt[v] as f64;
        let f_avg = scale(fsum[v], 1.0 / n);
        let r_avg = scale(esum[v], 1.0 / ecnt[v] as f64);
        let smooth = scale(
            add(add(f_avg, scale(r_avg, 2.0)), scale(p, n - 3.0)),
            1.0 / n,
        );
        let c = cn[v];
        let avg_s = if c > 0 { csum[v] / c as f64 } else { 0.0 };
        let q = match c {
            0 | 1 => smooth,
            2 if ecnt[v] <= 2 && CORNER_FIXED.with(|c| c.get()) => p, // corner (valence 2): fixed, like OpenSubdiv
            2 => {
                let crease_pt = scale(
                    add(
                        scale(p, 6.0),
                        add(m.pos[ca[v] as usize], m.pos[cb[v] as usize]),
                    ),
                    1.0 / 8.0,
                );
                if avg_s >= 1.0 {
                    crease_pt
                } else {
                    lerp(smooth, crease_pt, avg_s)
                }
            }
            _ => {
                if avg_s >= 1.0 {
                    p
                } else {
                    lerp(smooth, p, avg_s)
                }
            }
        };
        out.pos.push(q);
    }
    out.pos.extend_from_slice(&ep);
    out.pos.extend_from_slice(&fp);

    // faces
    for (fi, f) in m.faces.iter().enumerate() {
        let n = f.len();
        let fpi = (nv + ne + fi) as u32;
        let es = &fedge[foff[fi]..foff[fi + 1]];
        for i in 0..n {
            let e_next = nv as u32 + es[i];
            let e_prev = nv as u32 + es[(i + n - 1) % n];
            out.faces.push(vec![f[i], e_next, fpi, e_prev]);
        }
    }

    // propagate creases onto child edges
    let mut ncr = HashMap::default();
    for e in 0..ne {
        if ecount[e] == 2 {
            if let Some(&s) = cr.get(&ev[e]) {
                // uniform creasing: the sharpness drops by one per level
                let child = if s > 1.0 { s - 1.0 } else { 0.0 };
                if child > 0.0 {
                    let id = (nv + e) as u32;
                    ncr.insert(ekey(ev[e].0, id), child);
                    ncr.insert(ekey(ev[e].1, id), child);
                }
            }
        }
    }
    (out, ncr)
}

// ---------------------------------------------------------------- Bevel
struct Fan {
    closed: bool,
    /// (face, index of v in face), walking around v in face-order (clockwise seen from outside)
    corners: Vec<(u32, u32)>,
    /// neighbour vertex of every edge slot; corner j has incoming slot j and outgoing slot (j+1)%nslots
    nbr: Vec<u32>,
}

type HeMap = HashMap<(u32, u32), (u32, u32)>;

fn build_fan(v: u32, faces: &[Vec<u32>], he: &HeMap, vcorners: &[(u32, u32)]) -> Option<Fan> {
    if vcorners.is_empty() {
        return None;
    }
    let mut start = vcorners[0];
    let mut closed = false;
    let mut cur = start;
    let mut steps = 0;
    loop {
        let f = &faces[cur.0 as usize];
        let n = f.len();
        let p = f[(cur.1 as usize + n - 1) % n];
        match he.get(&(v, p)) {
            Some(&(g, j)) => {
                cur = (g, j);
                if cur == start {
                    closed = true;
                    break;
                }
            }
            None => break,
        }
        steps += 1;
        if steps > vcorners.len() {
            return None;
        }
    }
    if !closed {
        start = cur;
    }
    let mut corners = vec![start];
    let mut nbr = Vec::new();
    {
        let f = &faces[start.0 as usize];
        let n = f.len();
        nbr.push(f[(start.1 as usize + n - 1) % n]);
    }
    let mut closed_out = false;
    loop {
        let (fi, i) = *corners.last().unwrap();
        let f = &faces[fi as usize];
        let n = f.len();
        let nx = f[(i as usize + 1) % n];
        nbr.push(nx);
        match he.get(&(nx, v)) {
            Some(&(g, j)) => {
                let gl = faces[g as usize].len();
                let c = (g, ((j as usize + 1) % gl) as u32);
                if c == start {
                    closed_out = true;
                    break;
                }
                corners.push(c);
                if corners.len() > vcorners.len() {
                    return None;
                }
            }
            None => break,
        }
    }
    if corners.len() != vcorners.len() || closed_out != closed {
        return None;
    }
    if closed_out {
        nbr.pop();
    }
    // neighbours must be distinct (otherwise the edge map is ambiguous)
    let mut s: Vec<u32> = nbr.clone();
    s.sort_unstable();
    s.dedup();
    if s.len() != nbr.len() {
        return None;
    }
    Some(Fan {
        closed,
        corners,
        nbr,
    })
}

struct VData {
    v: u32,
    fan: Fan,
    bev: Vec<bool>,
    uc: Vec<u32>,
    us: Vec<V3>,
    cq: Vec<Option<V3>>,
}

#[inline]
fn sgn_clamp(s: f64) -> f64 {
    if s.abs() < 0.1 {
        if s < 0.0 {
            -0.1
        } else {
            0.1
        }
    } else {
        s
    }
}

impl VData {
    fn q_disp(&self, j: usize) -> Option<V3> {
        let k = self.fan.nbr.len();
        let (inc, out) = (j, (j + 1) % k);
        if let Some(d) = self.cq[j] {
            return Some(d);
        }
        match (self.bev[inc], self.bev[out]) {
            (true, false) => Some(scale(self.us[out], 1.0 / self.uc[out].max(1) as f64)),
            (false, true) => Some(scale(self.us[inc], 1.0 / self.uc[inc].max(1) as f64)),
            _ => None,
        }
    }
}

fn arc_points(l: V3, r: V3, mid: V3, wt: f64, seg: usize) -> Vec<V3> {
    let mut v = Vec::with_capacity(seg.saturating_sub(1));
    for j in 1..seg {
        let t = j as f64 / seg as f64;
        let b0 = (1.0 - t) * (1.0 - t);
        let b1 = 2.0 * wt * t * (1.0 - t);
        let b2 = t * t;
        let den = b0 + b1 + b2;
        let p = add(add(scale(l, b0), scale(mid, b1)), scale(r, b2));
        v.push(scale(p, 1.0 / den));
    }
    v
}

/// Blender BEVEL modifier semantics for an edge bevel (Angle limit method, offset type Offset,
/// circular profile). See module docs / the report for known limitations.
pub fn bevel(m: &QMesh, width: f64, segments: usize, angle_limit: f64) -> QMesh {
    let seg = segments.max(1);
    let nv = m.pos.len();
    let nf = m.faces.len();
    if !(width > 0.0)
        || !width.is_finite()
        || nf == 0
        || m.faces.iter().any(|f| f.len() < 3 || f.iter().any(|&i| i as usize >= nv))
        || m.pos.iter().any(|p| p.iter().any(|c| !c.is_finite()))
    {
        return m.clone();
    }
    let fnorm: Vec<V3> = m.faces.iter().map(|f| normalize(newell(&m.pos, f))).collect();
    let fcent: Vec<V3> = m
        .faces
        .iter()
        .map(|f| {
            let mut s = [0.0; 3];
            for &i in f {
                s = add(s, m.pos[i as usize]);
            }
            scale(s, 1.0 / f.len() as f64)
        })
        .collect();

    // half edges
    let mut he: HeMap = HashMap::with_capacity_and_hasher(m.faces.iter().map(|f| f.len()).sum(), Default::default());
    let mut bad = vec![false; nv];
    let mut vcorners: Vec<Vec<(u32, u32)>> = vec![Vec::new(); nv];
    for (fi, f) in m.faces.iter().enumerate() {
        for i in 0..f.len() {
            let (x, y) = (f[i], f[(i + 1) % f.len()]);
            vcorners[x as usize].push((fi as u32, i as u32));
            if x == y || he.insert((x, y), (fi as u32, i as u32)).is_some() {
                bad[x as usize] = true;
                bad[y as usize] = true;
            }
        }
    }

    // candidate edges
    let lim = angle_limit.max(0.0) + 1e-7;
    let mut bevset: HashSet<(u32, u32)> = HashSet::default();
    for (&(x, y), &(f, _)) in he.iter() {
        if x >= y || bad[x as usize] || bad[y as usize] {
            continue;
        }
        if let Some(&(g, _)) = he.get(&(y, x)) {
            let (n1, n2) = (fnorm[f as usize], fnorm[g as usize]);
            if len(n1) < 0.5 || len(n2) < 0.5 {
                continue;
            }
            let ang = dot(n1, n2).clamp(-1.0, 1.0).acos();
            if ang > lim {
                bevset.insert((x, y));
            }
        }
    }
    if bevset.is_empty() {
        return m.clone();
    }

    // fans of endpoint vertices; drop edges touching non-manifold vertices
    let mut ends: Vec<u32> = bevset.iter().flat_map(|&(a, b)| [a, b]).collect();
    ends.sort_unstable();
    ends.dedup();
    let mut fans: HashMap<u32, Fan> = HashMap::default();
    for &v in &ends {
        if let Some(f) = build_fan(v, &m.faces, &he, &vcorners[v as usize]) {
            fans.insert(v, f);
        }
    }
    bevset.retain(|&(a, b)| fans.contains_key(&a) && fans.contains_key(&b));
    if bevset.is_empty() {
        return m.clone();
    }
    let mut touched: Vec<u32> = bevset.iter().flat_map(|&(a, b)| [a, b]).collect();
    touched.sort_unstable();
    touched.dedup();

    // ---- stage 1: displacements at unit width
    let mut vdata: Vec<VData> = Vec::with_capacity(touched.len());
    for &v in &touched {
        let fan = fans.remove(&v).unwrap();
        let k = fan.nbr.len();
        let pv = m.pos[v as usize];
        let bev: Vec<bool> = fan.nbr.iter().map(|&n| bevset.contains(&ekey(v, n))).collect();
        let mut uc = vec![0u32; k];
        let mut us = vec![[0.0; 3]; k];
        let mut cq = vec![None; fan.corners.len()];
        for (j, &(f, _)) in fan.corners.iter().enumerate() {
            let (inc, out) = (j, (j + 1) % k);
            let (bi, bo) = (bev[inc], bev[out]);
            if !bi && !bo {
                continue;
            }
            let pp = m.pos[fan.nbr[inc] as usize];
            let pn = m.pos[fan.nbr[out] as usize];
            let a = normalize(sub(pp, pv));
            let b = normalize(sub(pn, pv));
            let nrm = fnorm[f as usize];
            let in_p = normalize(cross(nrm, sub(pv, pp)));
            let in_n = normalize(cross(nrm, sub(pn, pv)));
            let (sb, sa) = (dot(b, in_p), dot(a, in_n));
            if bi && bo {
                if sa.abs() < 0.05 && sb.abs() < 0.05 {
                    cq[j] = Some(in_p);
                } else {
                    cq[j] = Some(add(scale(a, 1.0 / sgn_clamp(sa)), scale(b, 1.0 / sgn_clamp(sb))));
                }
            } else if bi {
                us[out] = add(us[out], scale(b, 1.0 / sgn_clamp(sb)));
                uc[out] += 1;
            } else {
                us[inc] = add(us[inc], scale(a, 1.0 / sgn_clamp(sa)));
                uc[inc] += 1;
            }
        }
        vdata.push(VData {
            v,
            fan,
            bev,
            uc,
            us,
            cq,
        });
    }

    // ---- width clamp: slide at both ends of every edge must fit in the edge
    let mut shifts: HashMap<(u32, u32), f64> = HashMap::default();
    for vd in &vdata {
        let pv = m.pos[vd.v as usize];
        let k = vd.fan.nbr.len();
        let nc = vd.fan.corners.len();
        for s in 0..k {
            let d = normalize(sub(m.pos[vd.fan.nbr[s] as usize], pv));
            let sh = if vd.bev[s] {
                let c0 = (s + nc - 1) % nc;
                let mut best: f64 = 0.0;
                for c in [c0, s] {
                    if let Some(q) = vd.q_disp(c) {
                        best = best.max(dot(q, d));
                    }
                }
                best
            } else if vd.uc[s] > 0 {
                dot(vd.us[s], d).abs() / vd.uc[s] as f64
            } else {
                0.0
            };
            *shifts.entry(ekey(vd.v, vd.fan.nbr[s])).or_insert(0.0) += sh;
        }
    }
    let mut w = width;
    for (&(a, b), &sh) in &shifts {
        if sh > 1e-12 {
            let l = len(sub(m.pos[a as usize], m.pos[b as usize]));
            w = w.min(0.98 * l / sh);
        }
    }
    if !(w > 0.0) || !w.is_finite() {
        return m.clone();
    }

    // ---- stage 2: build geometry
    let mut npos: Vec<V3> = m.pos.clone();
    let mut foff: Vec<usize> = Vec::with_capacity(nf + 1);
    let mut tot = 0;
    for f in &m.faces {
        foff.push(tot);
        tot += f.len();
    }
    let mut cout: Vec<[u32; 2]> = vec![[0; 2]; tot];
    let mut clen: Vec<u8> = vec![0; tot];
    let mut chains: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    let mut extra_faces: Vec<Vec<u32>> = Vec::new();

    macro_rules! push_pos {
        ($p:expr) => {{
            npos.push($p);
            (npos.len() - 1) as u32
        }};
    }

    for vd in &vdata {
        let v = vd.v;
        let pv = m.pos[v as usize];
        let k = vd.fan.nbr.len();
        let nc = vd.fan.corners.len();
        let mut uid = vec![u32::MAX; k];
        for s in 0..k {
            if !vd.bev[s] {
                uid[s] = if vd.uc[s] > 0 {
                    push_pos!(add(pv, scale(vd.us[s], w / vd.uc[s] as f64)))
                } else {
                    v
                };
            }
        }
        let mut qid = vec![u32::MAX; nc];
        for j in 0..nc {
            let (inc, out) = (j, (j + 1) % k);
            match (vd.bev[inc], vd.bev[out]) {
                (true, true) => qid[j] = push_pos!(add(pv, scale(vd.cq[j].unwrap(), w))),
                (true, false) => qid[j] = uid[out],
                (false, true) => qid[j] = uid[inc],
                _ => {}
            }
        }
        // profile interior points per beveled slot
        let bev_slots: Vec<usize> = (0..k).filter(|&s| vd.bev[s]).collect();
        let mut interior: Vec<Vec<V3>> = Vec::new();
        let mut convex_all = true;
        for &s in &bev_slots {
            let fc = (s + nc - 1) % nc;
            let (l, r) = (npos[qid[fc] as usize], npos[qid[s] as usize]);
            let d = normalize(sub(m.pos[vd.fan.nbr[s] as usize], pv));
            let t = dot(sub(scale(add(l, r), 0.5), pv), d);
            let mid = add(pv, scale(d, t));
            let f1 = vd.fan.corners[fc].0 as usize;
            let f2 = vd.fan.corners[s].0 as usize;
            let cosd = dot(fnorm[f1], fnorm[f2]).clamp(-1.0, 1.0);
            let wt = ((1.0 + cosd) * 0.5).sqrt();
            if dot(fnorm[f1], sub(fcent[f2], fcent[f1])) >= 0.0 {
                convex_all = false;
            }
            interior.push(arc_points(l, r, mid, wt, seg));
        }
        // two beveled edges sharing both ends (e.g. cylinder rim vertex): share the profile
        let mut shared: Option<usize> = None; // second slot's interior reuses first's reversed
        if bev_slots.len() == 2 && seg > 1 {
            let (x, y) = (bev_slots[0], bev_slots[1]);
            let (fx, lx) = (qid[(x + nc - 1) % nc], qid[x]);
            let (fy, ly) = (qid[(y + nc - 1) % nc], qid[y]);
            if fx == ly && lx == fy {
                let n = seg - 1;
                for j in 0..n {
                    let a = interior[0][j];
                    let b = interior[1][n - 1 - j];
                    interior[0][j] = scale(add(a, b), 0.5);
                }
                shared = Some(0);
            }
        }
        let mut chain_of: Vec<Vec<u32>> = Vec::new();
        for (bi, &s) in bev_slots.iter().enumerate() {
            let fc = (s + nc - 1) % nc;
            let mut ch = vec![qid[fc]];
            if shared.is_some() && bi == 1 {
                let first = &chain_of[0];
                for j in (1..seg).rev() {
                    ch.push(first[j]);
                }
            } else {
                for p in &interior[bi] {
                    ch.push(push_pos!(*p));
                }
            }
            ch.push(qid[s]);
            chain_of.push(ch);
        }
        let mut chain_ix = vec![usize::MAX; k];
        for (bi, &s) in bev_slots.iter().enumerate() {
            chain_ix[s] = bi;
            chains.insert((v, vd.fan.nbr[s]), chain_of[bi].clone());
        }

        // face corner replacements
        for (j, &(f, i)) in vd.fan.corners.iter().enumerate() {
            let (inc, out) = (j, (j + 1) % k);
            let a = if vd.bev[inc] {
                *chain_of[chain_ix[inc]].last().unwrap()
            } else {
                uid[inc]
            };
            let b = if vd.bev[out] {
                chain_of[chain_ix[out]][0]
            } else {
                uid[out]
            };
            let c = foff[f as usize] + i as usize;
            if a == b {
                cout[c] = [a, a];
                clen[c] = 1;
            } else {
                cout[c] = [a, b];
                clen[c] = 2;
            }
        }

        // corner patch ring (walk order = clockwise from outside)
        let mut ring: Vec<u32> = Vec::new();
        for s in 0..k {
            if vd.bev[s] {
                ring.extend_from_slice(&chain_of[chain_ix[s]]);
            } else {
                ring.push(uid[s]);
            }
        }
        if !vd.fan.closed {
            ring.push(v);
        }
        dedupe_cyclic(&mut ring);
        if shared.is_some() {
            // the two shared profiles retrace each other: cancel the spur a,b,a -> a
            loop {
                let n = ring.len();
                if n < 3 {
                    break;
                }
                match (0..n).find(|&i| ring[(i + n - 1) % n] == ring[(i + 1) % n]) {
                    Some(i) => {
                        let j = (i + 1) % n;
                        let (hi, lo) = if i > j { (i, j) } else { (j, i) };
                        ring.remove(hi);
                        ring.remove(lo);
                    }
                    None => break,
                }
            }
        }
        if ring.len() >= 3 {
            ring.reverse(); // CCW from outside
            let all_bev = vd.bev.iter().all(|&b| b) && vd.fan.closed;
            if all_bev && seg >= 2 && ring.len() > 4 {
                let mut cen = [0.0; 3];
                for &r in &ring {
                    cen = add(cen, npos[r as usize]);
                }
                cen = scale(cen, 1.0 / ring.len() as f64);
                if convex_all {
                    let to_v = sub(pv, cen);
                    let dist = len(to_v);
                    if dist > 1e-12 {
                        let axis = scale(to_v, 1.0 / dist);
                        let mut rho = 0.0;
                        for &r in &ring {
                            let dvec = sub(npos[r as usize], cen);
                            let perp = sub(dvec, scale(axis, dot(dvec, axis)));
                            rho += len(perp);
                        }
                        rho /= ring.len() as f64;
                        if rho < w {
                            let bulge = (w - (w * w - rho * rho).sqrt()).min(0.9 * dist);
                            cen = add(cen, scale(axis, bulge));
                        }
                    }
                }
                let c = push_pos!(cen);
                let n = ring.len();
                if n % 2 == 0 {
                    for i in 0..n / 2 {
                        extra_faces.push(vec![c, ring[2 * i], ring[2 * i + 1], ring[(2 * i + 2) % n]]);
                    }
                } else {
                    for i in 0..n {
                        extra_faces.push(vec![c, ring[i], ring[(i + 1) % n]]);
                    }
                }
            } else {
                extra_faces.push(ring);
            }
        }
    }

    // ---- assemble faces
    let mut tgt = vdata.iter().map(|d| d.v).collect::<Vec<_>>();
    tgt.sort_unstable();
    let mut is_touched = vec![false; nv];
    for &v in &tgt {
        is_touched[v as usize] = true;
    }
    let mut faces: Vec<Vec<u32>> = Vec::with_capacity(nf + extra_faces.len());
    for (fi, f) in m.faces.iter().enumerate() {
        let mut g: Vec<u32> = Vec::with_capacity(f.len() + 4);
        for (i, &v) in f.iter().enumerate() {
            if is_touched[v as usize] {
                let c = foff[fi] + i;
                g.extend_from_slice(&cout[c][..clen[c] as usize]);
            } else {
                g.push(v);
            }
        }
        dedupe_cyclic(&mut g);
        if g.len() >= 3 {
            faces.push(g);
        }
    }
    // edge strips
    for (&(a, b), ca) in chains.iter() {
        if a >= b {
            continue;
        }
        if let Some(cb) = chains.get(&(b, a)) {
            let n = ca.len();
            for j in 0..n - 1 {
                let bj = cb[n - 1 - j];
                let bj1 = cb[n - 2 - j];
                let mut q = vec![bj, ca[j], ca[j + 1], bj1];
                dedupe_cyclic(&mut q);
                if q.len() >= 3 {
                    faces.push(q);
                }
            }
        }
    }
    faces.extend(extra_faces);

    // compact
    let mut used = vec![u32::MAX; npos.len()];
    let mut out = QMesh::default();
    for f in &mut faces {
        for v in f.iter_mut() {
            if used[*v as usize] == u32::MAX {
                used[*v as usize] = out.pos.len() as u32;
                out.pos.push(npos[*v as usize]);
            }
            *v = used[*v as usize];
        }
    }
    out.faces = faces;
    out
}

/// Bevel then subdivide (the order Blender assets in this codebase use).
pub fn bevel_then_subsurf(
    m: &QMesh,
    width: f64,
    segments: usize,
    angle_limit: f64,
    levels: usize,
) -> QMesh {
    catmull_clark(&bevel(m, width, segments, angle_limit), levels, None)
}

// ---------------------------------------------------------------- tests
#[cfg(test)]
mod tests {
    use super::*;

    fn euler(m: &QMesh) -> i64 {
        // count only referenced vertices
        let mut used = HashSet::default();
        for f in &m.faces {
            for &v in f {
                used.insert(v);
            }
        }
        used.len() as i64 - m.edges().len() as i64 + m.faces.len() as i64
    }

    /// every directed half edge appears exactly once and its reverse exists
    fn oriented_manifold(m: &QMesh) -> bool {
        let mut he: HashSet<(u32, u32)> = HashSet::default();
        for f in &m.faces {
            for i in 0..f.len() {
                if !he.insert((f[i], f[(i + 1) % f.len()])) {
                    return false;
                }
            }
        }
        he.iter().all(|&(a, b)| he.contains(&(b, a)))
    }

    fn volume(m: &QMesh) -> f64 {
        let mut v = 0.0;
        for t in m.triangulate() {
            let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
            v += dot(a, cross(b, c)) / 6.0;
        }
        v
    }

    fn face_area(m: &QMesh, f: &[u32]) -> f64 {
        0.5 * len(newell(&m.pos, f))
    }

    fn no_nan(m: &QMesh) -> bool {
        m.pos.iter().all(|p| p.iter().all(|c| c.is_finite()))
            && m.faces.iter().all(|f| f.iter().all(|&i| (i as usize) < m.pos.len()))
    }

    fn bbox(m: &QMesh) -> (V3, V3) {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for p in &m.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn unit_cube() -> QMesh {
        QMesh::cuboid([0.0; 3], [1.0; 3])
    }

    fn check_solid(m: &QMesh, expect_euler: i64) {
        assert!(no_nan(m));
        assert!(m.is_closed());
        assert!(oriented_manifold(m));
        assert_eq!(euler(m), expect_euler);
        assert!(volume(m) > 0.0, "outward orientation");
    }

    #[test]
    fn cube_is_sane() {
        let c = unit_cube();
        check_solid(&c, 2);
        assert!((volume(&c) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cc_cube_level1_counts() {
        let c = catmull_clark(&unit_cube(), 1, None);
        assert_eq!(c.pos.len(), 26);
        assert_eq!(c.faces.len(), 24);
        assert!(c.faces.iter().all(|f| f.len() == 4));
        check_solid(&c, 2);
        // known value: Catmull-Clark corner of a unit cube = (5/9*... ) corner (1/3-ish inset)
        // corner vertex moves inward: (F + 2R)/3 = 2/9 per axis
        assert!(c.pos[0].iter().all(|x| (x - 2.0 / 9.0).abs() < 1e-12), "{:?}", c.pos[0]);
        assert!(c.pos[7].iter().all(|x| (x - 7.0 / 9.0).abs() < 1e-12));
    }

    #[test]
    fn cc_cube_levels_shrink_and_stay_closed() {
        let mut prev_vol = 1.0;
        for lv in 1..=3 {
            let c = catmull_clark(&unit_cube(), lv, None);
            check_solid(&c, 2);
            assert_eq!(c.faces.len(), 6 * 4usize.pow(lv as u32));
            let v = volume(&c);
            assert!(v < prev_vol && v > 0.3, "level {lv} volume {v}");
            prev_vol = v;
        }
        let c3 = catmull_clark(&unit_cube(), 3, None);
        let (lo, hi) = bbox(&c3);
        let ext = hi[0] - lo[0];
        // sphere-like cube: smaller than the cube, but not collapsed
        assert!(ext > 0.8 && ext < 0.9, "bbox extent {ext}");
        let v = volume(&c3);
        assert!(v > 0.3 && v < 0.5, "vol {v}");
        eprintln!("cc level3 extent {ext} volume {v}");
    }

    #[test]
    fn cc_open_plane_keeps_boundary() {
        // 3x3 quad plane
        let mut pos = vec![];
        for y in 0..4 {
            for x in 0..4 {
                pos.push([x as f64, y as f64, 0.0]);
            }
        }
        let mut faces = vec![];
        for y in 0..3u32 {
            for x in 0..3u32 {
                let i = y * 4 + x;
                faces.push(vec![i, i + 1, i + 5, i + 4]);
            }
        }
        let m = QMesh { pos, faces };
        let c = catmull_clark(&m, 2, None);
        assert!(no_nan(&c));
        assert!(!c.is_closed());
        // all points inside the original square, z = 0, boundary stays on the square
        let (lo, hi) = bbox(&c);
        assert!(lo[0].abs() < 1e-12 && lo[1].abs() < 1e-12 && (hi[0] - 3.0).abs() < 1e-12);
        assert!(c.pos.iter().all(|p| p[2].abs() < 1e-12));
        // boundary edge vertices all lie on the border of the square
        let mut cnt: HashMap<u64, u32> = HashMap::default();
        for f in &c.faces {
            for i in 0..f.len() {
                *cnt.entry(ekey64(f[i], f[(i + 1) % 4])).or_default() += 1;
            }
        }
        let mut nb = 0;
        for (k, n) in cnt {
            if n == 1 {
                nb += 1;
                for v in [(k >> 32) as usize, (k & 0xffff_ffff) as usize] {
                    let p = c.pos[v];
                    let on = p[0].abs() < 1e-12
                        || p[1].abs() < 1e-12
                        || (p[0] - 3.0).abs() < 1e-12
                        || (p[1] - 3.0).abs() < 1e-12;
                    assert!(on, "{:?}", p);
                }
            }
        }
        assert_eq!(nb, 3 * 4 * 4);
        // boundary rule check at one level: edge point of a boundary edge = midpoint
        let c1 = catmull_clark(&m, 1, None);
        assert!(c1.pos.iter().any(|p| (p[0] - 0.5).abs() < 1e-12 && p[1].abs() < 1e-12));
        // boundary vertex (0,1) -> (6*(0,1)+(0,0)+(0,2))/8 = (0,1)
        assert!(c1.pos[4].iter().zip([0.0, 1.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-12));
    }

    #[test]
    fn cc_creases_stay_sharp() {
        let cube = unit_cube();
        // crease a single edge (0,1): edge point is the midpoint, uncreased is pulled off the line
        let mut cr = HashMap::default();
        cr.insert((0u32, 1u32), 10.0);
        let a = catmull_clark(&cube, 1, Some(&cr));
        let b = catmull_clark(&cube, 1, None);
        let nv = 8;
        // edge ids are assigned in discovery order; find the new vertex at the midpoint of (0,1)
        let on_line = |m: &QMesh| {
            m.pos[nv..].iter().any(|p| {
                (p[0] - 0.5).abs() < 1e-12 && p[1].abs() < 1e-12 && p[2].abs() < 1e-12
            })
        };
        assert!(on_line(&a));
        assert!(!on_line(&b));
        // crease the whole top loop: top vertices stay in the plane z=1 at every level
        let mut cr = HashMap::default();
        for e in [(4u32, 5u32), (5, 7), (6, 7), (4, 6)] {
            cr.insert(e, 10.0);
        }
        let c = catmull_clark(&cube, 3, Some(&cr));
        check_solid(&c, 2);
        let (_, hi) = bbox(&c);
        assert!((hi[2] - 1.0).abs() < 1e-12, "top stays flat/sharp: {}", hi[2]);
        let top: Vec<_> = c.pos.iter().filter(|p| (p[2] - 1.0).abs() < 1e-12).collect();
        assert!(top.len() >= 81, "{}", top.len());
        // a fully creased cube (all 12 edges) is the cube itself
        let mut all = HashMap::default();
        for e in cube.edges() {
            all.insert(e, 10.0);
        }
        let c = catmull_clark(&cube, 2, Some(&all));
        let (lo, hi) = bbox(&c);
        assert!(lo.iter().all(|x| x.abs() < 1e-12) && hi.iter().all(|x| (x - 1.0).abs() < 1e-12));
        assert!((volume(&c) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cc_semi_sharp_creases_decay_per_level() {
        // measured in Blender 4.2 (cube size 1, every edge crease c, one level): sharpness = 10 c^2; edge point y = -0.375 - 0.125 * min(s, 1)
        let cube = unit_cube();
        let mut all = HashMap::default();
        for e in cube.edges() {
            all.insert(e, 10.0 * 0.19f64 * 0.19);
        }
        let c = catmull_clark(&cube, 1, Some(&all));
        let w = 10.0 * 0.19f64 * 0.19;
        // unit cube [0,1]^3: edge (0,1) midpoint (0.5,0,0); smooth edge point (0.5, 0.125, 0.125) -> blended towards the midpoint
        let want = [0.5, 0.125 * (1.0 - w), 0.125 * (1.0 - w)];
        assert!(c.pos[8..].iter().any(|p| (0..3).all(|k| (p[k] - want[k]).abs() < 1e-12)), "{:?}", &c.pos[8..20]);
        // sharpness 0.36 < 1 is spent after the first level: level 2 equals subdividing the level-1 mesh without creases
        let c2 = catmull_clark(&cube, 2, Some(&all));
        let c1_then = catmull_clark(&c, 1, None);
        for (a, b) in c2.pos.iter().zip(c1_then.pos.iter()) {
            assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-12));
        }
    }

    #[test]
    fn bevel_cube_s1() {
        let c = unit_cube();
        let b = bevel(&c, 0.1, 1, 30f64.to_radians());
        assert_eq!(b.pos.len(), 24);
        assert_eq!(b.faces.len(), 6 + 12 + 8);
        check_solid(&b, 2);
        assert_eq!(b.edges().len(), 48);
        // 6 shrunken originals (area 0.8^2), 12 strips, 8 triangles
        let mut big = 0;
        for f in &b.faces {
            if f.len() == 4 && (face_area(&b, f) - 0.64).abs() < 1e-9 {
                big += 1;
            }
        }
        assert_eq!(big, 6, "all original faces shrank to 0.8x0.8");
        assert_eq!(b.faces.iter().filter(|f| f.len() == 3).count(), 8);
        // chamfer volume: 1 - 12 * (0.5*0.1*0.1*1) + 8 corner correction (small): check range
        let v = volume(&b);
        assert!(v < 1.0 && v > 0.9, "{v}");
        // bbox unchanged
        let (lo, hi) = bbox(&b);
        assert!(lo.iter().all(|x| x.abs() < 1e-12) && hi.iter().all(|x| (x - 1.0).abs() < 1e-12));
    }

    #[test]
    fn bevel_cube_s3_and_more() {
        let c = unit_cube();
        for s in 2..=5 {
            let b = bevel(&c, 0.1, s, 30f64.to_radians());
            check_solid(&b, 2);
            let v = volume(&b);
            assert!(v < 1.0 && v > 0.9, "s={s} vol {v}");
            for f in &b.faces {
                assert!(face_area(&b, f) > 0.0);
            }
        }
        let b = bevel(&c, 0.1, 3, 30f64.to_radians());
        assert_eq!(b.pos.len(), 80);
        assert_eq!(b.faces.len(), 6 + 36 + 72);
        // circular profile: volume of rounded edges: 12 quarter-circle cut-outs
        // (1-pi/4)*r^2 * L each is removed along edges; cube-corner correction is small
        let expected = 1.0 - 12.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 0.01 * 0.8 - 12.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 0.01 * 0.2;
        let v = volume(&b);
        assert!((v - expected).abs() < 5e-3, "vol {v} vs ~{expected}");
    }

    #[test]
    fn bevel_clamps_width() {
        let c = unit_cube();
        let b = bevel(&c, 5.0, 2, 30f64.to_radians());
        check_solid(&b, 2);
        let (lo, hi) = bbox(&b);
        assert!(lo.iter().all(|x| *x >= -1e-12) && hi.iter().all(|x| *x <= 1.0 + 1e-12));
        let b = bevel(&c, 0.5, 1, 30f64.to_radians());
        assert!(no_nan(&b));
    }

    fn open_edges(m: &QMesh) -> Vec<(u32, u32, u32)> {
        let mut c: HashMap<(u32, u32), u32> = HashMap::default();
        for f in &m.faces {
            for i in 0..f.len() {
                *c.entry(ekey(f[i], f[(i + 1) % f.len()])).or_default() += 1;
            }
        }
        c.into_iter().filter(|x| x.1 != 2).map(|((a, b), n)| (a, b, n)).collect()
    }

    #[test]
    fn bevel_cylinder_only_rims() {
        let n = 16;
        let cyl = QMesh::cylinder(n, 1.0, 0.0, 2.0);
        check_solid(&cyl, 2);
        let w = 0.05;
        let b = bevel(&cyl, w, 1, 30f64.to_radians());
        check_solid(&b, 2);
        assert_eq!(b.pos.len(), 4 * n);
        for p in &b.pos {
            let ok = [0.0, w, 2.0 - w, 2.0].iter().any(|z| (p[2] - z).abs() < 1e-9);
            assert!(ok, "unexpected z {}", p[2]);
        }
        // no bevel in the mid-height region: side edges (vertical) are not beveled:
        // side faces are quads spanning w..2-w
        let side_quads = b
            .faces
            .iter()
            .filter(|f| f.len() == 4 && f.iter().all(|&i| b.pos[i as usize][2] >= w - 1e-9 && b.pos[i as usize][2] <= 2.0 - w + 1e-9))
            .count();
        assert_eq!(side_quads, n);
        // multi-segment
        let b3 = bevel(&cyl, w, 3, 30f64.to_radians());
        check_solid(&b3, 2);
        let b3 = bevel(&QMesh::cylinder(32, 1.0, 0.0, 0.5), 0.1, 4, 30f64.to_radians());
        check_solid(&b3, 2);
    }

    /// slab 3x3 grid with the middle cell removed, walls everywhere
    fn slab_with_hole() -> QMesh {
        // vertex grid 4x4 at z = 0 and z = h
        let h = 0.3;
        let mut pos = vec![];
        let xs = [0.0, 1.0, 2.0, 3.0];
        for z in [0.0, h] {
            for y in xs {
                for x in xs {
                    pos.push([x, y, z]);
                }
            }
        }
        let id = |x: u32, y: u32, z: u32| z * 16 + y * 4 + x;
        let mut faces = vec![];
        for y in 0..3u32 {
            for x in 0..3u32 {
                if x == 1 && y == 1 {
                    continue;
                }
                // top (+z) CCW, bottom (-z)
                faces.push(vec![id(x, y, 1), id(x + 1, y, 1), id(x + 1, y + 1, 1), id(x, y + 1, 1)]);
                faces.push(vec![id(x, y + 1, 0), id(x + 1, y + 1, 0), id(x + 1, y, 0), id(x, y, 0)]);
            }
        }
        // outer walls
        for i in 0..3u32 {
            faces.push(vec![id(i, 0, 0), id(i + 1, 0, 0), id(i + 1, 0, 1), id(i, 0, 1)]); // -y
            faces.push(vec![id(i + 1, 3, 0), id(i, 3, 0), id(i, 3, 1), id(i + 1, 3, 1)]); // +y
            faces.push(vec![id(0, i + 1, 0), id(0, i, 0), id(0, i, 1), id(0, i + 1, 1)]); // -x
            faces.push(vec![id(3, i, 0), id(3, i + 1, 0), id(3, i + 1, 1), id(3, i, 1)]); // +x
        }
        // hole walls (normals point into the hole)
        faces.push(vec![id(1, 1, 0), id(1, 2, 0), id(1, 2, 1), id(1, 1, 1)]); // x=1 face, normal +x
        faces.push(vec![id(2, 2, 0), id(2, 1, 0), id(2, 1, 1), id(2, 2, 1)]); // x=2, normal -x
        faces.push(vec![id(2, 1, 0), id(1, 1, 0), id(1, 1, 1), id(2, 1, 1)]); // y=1, normal +y
        faces.push(vec![id(1, 2, 0), id(2, 2, 0), id(2, 2, 1), id(1, 2, 1)]); // y=2, normal -y
        QMesh { pos, faces }
    }

    #[test]
    fn bevel_slab_with_hole() {
        let slab = slab_with_hole();
        assert!(slab.is_closed());
        assert!(oriented_manifold(&slab));
        assert!(volume(&slab) > 0.0);
        assert_eq!(euler(&slab), 0, "torus");
        for s in [1usize, 2, 3] {
            let b = bevel(&slab, 0.05, s, 30f64.to_radians());
            assert!(no_nan(&b));
            assert!(b.is_closed(), "s={s}");
            assert!(oriented_manifold(&b), "s={s}");
            assert_eq!(euler(&b), 0, "s={s}");
            assert!(volume(&b) > 0.0 && volume(&b) < volume(&slab) + 0.02);
            // subsurf afterwards stays closed
            let c = catmull_clark(&b, 1, None);
            assert!(c.is_closed() && no_nan(&c));
        }
    }

    #[test]
    fn bevel_merged_boxes() {
        let mut m = QMesh::default();
        m.append(&QMesh::cuboid([0.0; 3], [1.0; 3]));
        m.append(&QMesh::cuboid([2.0, 0.0, 0.0], [3.0, 2.0, 0.5]));
        m.append(&QMesh::cuboid([-3.0, -1.0, 0.0], [-2.0, -0.5, 4.0]));
        let m = m.weld(1e-9);
        assert_eq!(m.pos.len(), 24);
        for s in [1usize, 2, 3] {
            let b = bevel(&m, 0.05, s, 30f64.to_radians());
            assert!(no_nan(&b));
            assert!(b.is_closed() && oriented_manifold(&b));
            assert_eq!(euler(&b), 6, "three spheres");
            let c = bevel_then_subsurf(&m, 0.05, s, 30f64.to_radians(), 2);
            assert!(c.is_closed() && no_nan(&c));
        }
        // boxes sharing an edge (non-manifold vertices) must not panic or produce NaN
        let mut t = QMesh::default();
        t.append(&QMesh::cuboid([0.0; 3], [1.0; 3]));
        t.append(&QMesh::cuboid([1.0, 1.0, 0.0], [2.0, 2.0, 1.0]));
        let t = t.weld(1e-9);
        let b = bevel(&t, 0.05, 2, 30f64.to_radians());
        assert!(no_nan(&b));
    }

    #[test]
    fn bevel_random_boxes_no_nan() {
        let mut s = 0x1234_5678_9abc_def0u64;
        let mut rnd = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..60 {
            let lo = [rnd() * 4.0 - 2.0, rnd() * 4.0 - 2.0, rnd() * 4.0 - 2.0];
            let d = [0.01 + rnd() * 3.0, 0.01 + rnd() * 3.0, 0.01 + rnd() * 3.0];
            let hi = [lo[0] + d[0], lo[1] + d[1], lo[2] + d[2]];
            let m = QMesh::cuboid(lo, hi);
            let w = rnd() * 1.5;
            let sg = 1 + (rnd() * 5.0) as usize;
            let b = bevel(&m, w, sg, 30f64.to_radians());
            assert!(no_nan(&b), "box {lo:?} {hi:?} w {w} s {sg}");
            assert!(b.is_closed() && oriented_manifold(&b));
            assert_eq!(euler(&b), 2);
            let c = bevel_then_subsurf(&m, w, sg, 30f64.to_radians(), 1);
            assert!(no_nan(&c) && c.is_closed());
        }
        // degenerate inputs
        let m = QMesh::cuboid([0.0; 3], [1.0, 1.0, 0.0]);
        let b = bevel(&m, 0.1, 2, 0.5);
        assert!(no_nan(&b));
        let b = bevel(&unit_cube(), 0.0, 2, 0.5);
        assert_eq!(b.pos.len(), 8);
        let b = bevel(&unit_cube(), f64::NAN, 2, 0.5);
        assert_eq!(b.pos.len(), 8);
    }

    #[test]
    fn bevel_flat_plane_untouched() {
        let mut pos = vec![];
        for y in 0..4 {
            for x in 0..4 {
                pos.push([x as f64, y as f64, 0.0]);
            }
        }
        let mut faces = vec![];
        for y in 0..3u32 {
            for x in 0..3u32 {
                let i = y * 4 + x;
                faces.push(vec![i, i + 1, i + 5, i + 4]);
            }
        }
        let m = QMesh { pos, faces };
        let b = bevel(&m, 0.1, 3, 30f64.to_radians());
        assert_eq!(b.pos, m.pos);
        assert_eq!(b.faces, m.faces);
    }

    #[test]
    fn weld_edges_triangulate() {
        let mut m = QMesh::default();
        m.append(&unit_cube());
        m.append(&unit_cube());
        let w = m.weld(1e-9);
        assert_eq!(w.pos.len(), 8);
        assert_eq!(w.faces.len(), 12);
        assert_eq!(unit_cube().edges().len(), 12);
        assert_eq!(unit_cube().triangulate().len(), 12);
        assert!(!w.is_closed(), "duplicated faces give 4 faces per edge");
    }

    #[test]
    fn bevel_then_subsurf_is_closed() {
        let c = bevel_then_subsurf(&unit_cube(), 0.05, 3, 30f64.to_radians(), 1);
        check_solid(&c, 2);
    }

    fn parse_obj(s: &str) -> QMesh {
        let mut m = QMesh::default();
        for l in s.lines() {
            let mut it = l.split_whitespace();
            match it.next() {
                Some("v") => {
                    let p: Vec<f64> = it.map(|x| x.parse().unwrap()).collect();
                    assert_eq!(p.len(), 3);
                    m.pos.push([p[0], p[1], p[2]]);
                }
                Some("f") => {
                    let f: Vec<u32> = it.map(|x| x.parse::<u32>().unwrap() - 1).collect();
                    assert!(f.len() >= 3);
                    m.faces.push(f);
                }
                None => {}
                Some(o) => panic!("unexpected obj line {o}"),
            }
        }
        m
    }

    #[test]
    fn obj_roundtrip_demo_mesh() {
        let m = bevel_then_subsurf(&unit_cube(), 0.05, 3, 30f64.to_radians(), 1);
        let r = parse_obj(&m.to_obj());
        assert_eq!(r.pos.len(), m.pos.len());
        assert_eq!(r.faces, m.faces);
        assert!(r.faces.iter().flatten().all(|&i| (i as usize) < r.pos.len()));
        check_solid(&r, 2);
    }

    #[test]
    fn perf_50k_quads() {
        // 224 x 224 = 50176 quads
        let n = 224u32;
        let mut pos = Vec::new();
        for y in 0..=n {
            for x in 0..=n {
                pos.push([x as f64, y as f64, ((x * y) % 7) as f64 * 0.01]);
            }
        }
        let mut faces = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let i = y * (n + 1) + x;
                faces.push(vec![i, i + 1, i + n + 2, i + n + 1]);
            }
        }
        let m = QMesh { pos, faces };
        assert!(m.faces.len() > 50_000);
        let t = std::time::Instant::now();
        let c = catmull_clark(&m, 1, None);
        let dt = t.elapsed().as_secs_f64();
        eprintln!("catmull_clark 1 level of {} quads: {:.3}s -> {} quads", m.faces.len(), dt, c.faces.len());
        assert_eq!(c.faces.len(), m.faces.len() * 4);
        if !cfg!(debug_assertions) {
            assert!(dt < 1.0, "took {dt}s");
        }
    }
}
