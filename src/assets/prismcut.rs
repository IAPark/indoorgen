//! Boolean DIFFERENCE of a surface mesh minus a convex vertical prism (the Pan handle hole: bpy cylinder cutter spanning all z).
//! Reproduces what Blender's EXACT solver + dissolve produces for this configuration: every face (quads are split along the
//! `is_quad_flip_first_third` diagonal) is clipped against the prism in the xy projection, the outside parts of one input face become one
//! polygon (diagonal dissolved), prism wall faces are the regions of each side facet inside the solid (one polygon per connected region),
//! and non-input vertices of valence 2 that are straight within 1e-4 are dissolved (only possible on a quad diagonal).
use crate::dhash::{HashMap, HashSet};
use crate::mesh::Mesh;

type V3 = [f64; 3];
type V2 = [f64; 2];

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Key {
    Orig(u32),
    /// crossing of the mesh edge (lo, hi) with prism facet i
    Cross(u32, u32, u8),
    /// vertical line through prism corner k pierces triangle tid
    Corner(u8, u32),
}

fn sub2(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross2(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn xy(p: V3) -> V2 {
    [p[0], p[1]]
}

/// raw parameters (t along p->q, u along a->b) of the line intersection
fn isect_raw(p: V2, q: V2, a: V2, b: V2) -> Option<(f64, f64)> {
    let d = sub2(q, p);
    let e = sub2(b, a);
    let den = cross2(d, e);
    if den == 0.0 {
        return None;
    }
    let w = sub2(a, p);
    Some((cross2(w, e) / den, cross2(w, d) / den))
}
fn seg_isect(p: V2, q: V2, a: V2, b: V2) -> Option<(f64, f64)> {
    let (t, u) = isect_raw(p, q, a, b)?;
    if t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0 {
        Some((t, u))
    } else {
        None
    }
}

struct Ctx<'a> {
    m: &'a Mesh,
    c: Vec<V2>,
    tris: Vec<[u32; 3]>,
    pos: Vec<V3>,
    map: HashMap<Key, u32>,
}

impl<'a> Ctx<'a> {
    fn inside_c(&self, p: V2) -> bool {
        let n = self.c.len();
        (0..n).all(|i| cross2(sub2(self.c[(i + 1) % n], self.c[i]), sub2(p, self.c[i])) > 0.0)
    }
    fn txy(&self, tid: u32) -> [V2; 3] {
        let t = self.tris[tid as usize];
        [xy(self.m.pos[t[0] as usize]), xy(self.m.pos[t[1] as usize]), xy(self.m.pos[t[2] as usize])]
    }
    fn key_pos(&mut self, k: Key) -> V3 {
        if let Some(&i) = self.map.get(&k) {
            return self.pos[i as usize];
        }
        let p = match k {
            Key::Orig(v) => self.m.pos[v as usize],
            Key::Cross(lo, hi, i) => {
                let (p, q) = (self.m.pos[lo as usize], self.m.pos[hi as usize]);
                let n = self.c.len();
                let t = isect_raw(xy(p), xy(q), self.c[i as usize], self.c[(i as usize + 1) % n]).map(|x| x.0).unwrap_or(0.5);
                [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t]
            }
            Key::Corner(ci, tid) => {
                let t = self.tris[tid as usize];
                let (a, b, c) = (self.m.pos[t[0] as usize], self.m.pos[t[1] as usize], self.m.pos[t[2] as usize]);
                let p = self.c[ci as usize];
                let den = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / den;
                let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / den;
                [p[0], p[1], l1 * a[2] + l2 * b[2] + (1.0 - l1 - l2) * c[2]]
            }
        };
        let p = [p[0] as f32 as f64, p[1] as f32 as f64, p[2] as f32 as f64];
        self.map.insert(k, self.pos.len() as u32);
        self.pos.push(p);
        p
    }

    /// directed boundary edges (in the winding of the triangle) of tri minus prism
    fn tri_region(&mut self, tid: u32) -> Vec<(Key, Key)> {
        let t = self.tris[tid as usize];
        let p = self.txy(tid);
        let area = cross2(sub2(p[1], p[0]), sub2(p[2], p[0]));
        let mut out = vec![];
        if area.abs() < 1e-18 {
            return out;
        }
        let sg = area.signum();
        let m = self.c.len();
        for k in 0..3 {
            let (u, w) = (t[k], t[(k + 1) % 3]);
            let (lo, hi) = if u < w { (u, w) } else { (w, u) };
            let (plo, phi) = (xy(self.m.pos[lo as usize]), xy(self.m.pos[hi as usize]));
            let mut cr: Vec<(f64, u8)> = vec![];
            for i in 0..m {
                if let Some((tt, _)) = seg_isect(plo, phi, self.c[i], self.c[(i + 1) % m]) {
                    cr.push((if u < w { tt } else { 1.0 - tt }, i as u8));
                }
            }
            cr.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut pts: Vec<(f64, Key)> = vec![(0.0, Key::Orig(u))];
            for (tt, i) in cr {
                pts.push((tt, Key::Cross(lo, hi, i)));
            }
            pts.push((1.0, Key::Orig(w)));
            let (pu, pw) = (xy(self.m.pos[u as usize]), xy(self.m.pos[w as usize]));
            for j in 0..pts.len() - 1 {
                let tm = (pts[j].0 + pts[j + 1].0) / 2.0;
                let pm = [pu[0] + (pw[0] - pu[0]) * tm, pu[1] + (pw[1] - pu[1]) * tm];
                if !self.inside_c(pm) {
                    out.push((pts[j].1, pts[j + 1].1));
                }
            }
        }
        // prism boundary pieces inside the triangle, traversed opposite to the triangle
        let mut nodes: Vec<(Key, V2)> = vec![];
        let seq: Vec<(usize, bool)> = if sg > 0.0 { (0..m).rev().map(|i| (i, false)).collect() } else { (0..m).map(|i| (i, true)).collect() };
        for (i, fwd) in seq {
            let (a, b) = (self.c[i], self.c[(i + 1) % m]);
            let startk = if fwd { i } else { (i + 1) % m };
            nodes.push((Key::Corner(startk as u8, tid), if fwd { a } else { b }));
            let mut ev: Vec<(f64, Key)> = vec![];
            for k in 0..3 {
                let (u, w) = (t[k], t[(k + 1) % 3]);
                let (lo, hi) = if u < w { (u, w) } else { (w, u) };
                if let Some((_, uu)) = seg_isect(xy(self.m.pos[lo as usize]), xy(self.m.pos[hi as usize]), a, b) {
                    ev.push((if fwd { uu } else { 1.0 - uu }, Key::Cross(lo, hi, i as u8)));
                }
            }
            ev.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            for (uu, key) in ev {
                let (s, e) = if fwd { (a, b) } else { (b, a) };
                nodes.push((key, [s[0] + (e[0] - s[0]) * uu, s[1] + (e[1] - s[1]) * uu]));
            }
        }
        let nn = nodes.len();
        for j in 0..nn {
            let (n0, n1) = (nodes[j], nodes[(j + 1) % nn]);
            let pm = [(n0.1[0] + n1.1[0]) / 2.0, (n0.1[1] + n1.1[1]) / 2.0];
            let inside = (0..3).all(|k| cross2(sub2(p[(k + 1) % 3], p[k]), sub2(pm, p[k])) * sg > 0.0);
            if inside {
                out.push((n0.0, n1.0));
            }
        }
        out
    }
}

fn trace(edges: Vec<(Key, Key)>) -> Vec<Vec<Key>> {
    let mut set: HashSet<(Key, Key)> = HashSet::default();
    for &e in &edges {
        set.insert(e);
    }
    // cancel opposite pairs (dissolved diagonal)
    let edges: Vec<(Key, Key)> = edges.into_iter().filter(|&(a, b)| !set.contains(&(b, a))).collect();
    let mut next: HashMap<Key, Vec<usize>> = HashMap::default();
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

fn newell(p: &[V3]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// `corners`: the prism polygon in the xy plane (any orientation, convex)
pub fn prism_difference(m: &Mesh, corners: &[V2]) -> Mesh {
    let mut c = corners.to_vec();
    let area: f64 = (0..c.len()).map(|i| cross2(c[i], c[(i + 1) % c.len()])).sum();
    if area < 0.0 {
        c.reverse();
    }
    let nc = c.len();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &c {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let ctr = [c.iter().map(|p| p[0]).sum::<f64>() / nc as f64, c.iter().map(|p| p[1]).sum::<f64>() / nc as f64];
    let mut cx = Ctx { m, c: c.clone(), tris: vec![], pos: vec![], map: HashMap::default() };
    // faces touching the prism bbox
    let mut cut_faces: Vec<usize> = vec![];
    let mut keep: Vec<bool> = vec![true; m.faces.len()];
    let mut face_tids: Vec<Vec<u32>> = vec![vec![]; m.faces.len()];
    let mut diag: HashSet<(u32, u32)> = HashSet::default();
    for (fi, f) in m.faces.iter().enumerate() {
        if f.len() != 3 && f.len() != 4 {
            continue;
        }
        let (mut fl, mut fh) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for &v in f {
            for k in 0..2 {
                fl[k] = fl[k].min(m.pos[v as usize][k]);
                fh[k] = fh[k].max(m.pos[v as usize][k]);
            }
        }
        if fl[0] > hi[0] || fh[0] < lo[0] || fl[1] > hi[1] || fh[1] < lo[1] {
            continue;
        }
        keep[fi] = false;
        cut_faces.push(fi);
        let tris: Vec<[u32; 3]> = if f.len() == 3 {
            vec![[f[0], f[1], f[2]]]
        } else {
            let p = |i: usize| m.pos[f[i] as usize];
            let sub3 = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let cr = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let (d12, d13, d14) = (sub3(p(1), p(0)), sub3(p(2), p(0)), sub3(p(3), p(0)));
            let (ca, cb) = (cr(d12, d13), cr(d14, d13));
            let flip = ca[0] * cb[0] + ca[1] * cb[1] + ca[2] * cb[2] > 0.0;
            if flip {
                diag.insert((f[1].min(f[3]), f[1].max(f[3])));
                vec![[f[0], f[1], f[3]], [f[1], f[2], f[3]]]
            } else {
                diag.insert((f[0].min(f[2]), f[0].max(f[2])));
                vec![[f[0], f[1], f[2]], [f[0], f[2], f[3]]]
            }
        };
        for t in tris {
            face_tids[fi].push(cx.tris.len() as u32);
            cx.tris.push(t);
        }
    }
    let mut loops: Vec<Vec<Key>> = vec![];
    let mut orig_faces: Vec<Vec<u32>> = vec![];
    for &fi in &cut_faces {
        let mut edges = vec![];
        for &tid in &face_tids[fi] {
            edges.extend(cx.tri_region(tid));
        }
        for lp in trace(edges) {
            if lp.len() >= 3 && lp.iter().all(|k| matches!(k, Key::Orig(_))) && lp.len() == m.faces[fi].len() {
                orig_faces.push(m.faces[fi].clone());
            } else if lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    // wall faces
    let mut walls: Vec<Vec<Key>> = vec![];
    let all_tids: Vec<u32> = (0..cx.tris.len() as u32).collect();
    let mut seg_by_facet: Vec<Vec<(Key, Key)>> = vec![vec![]; nc];
    let mut pierce: Vec<Vec<Key>> = vec![vec![]; nc];
    for i in 0..nc {
        let (a, b) = (c[i], c[(i + 1) % nc]);
        let d = sub2(b, a);
        for &tid in &all_tids {
            let t = cx.tris[tid as usize];
            let p = cx.txy(tid);
            let area = cross2(sub2(p[1], p[0]), sub2(p[2], p[0]));
            if area.abs() < 1e-18 {
                continue;
            }
            let sg = area.signum();
            let (mut t0, mut t1, mut k0, mut k1) = (0.0f64, 1.0f64, None, None);
            let mut ok = true;
            for k in 0..3 {
                let (u, w) = (p[k], p[(k + 1) % 3]);
                let e = sub2(w, u);
                let f0 = sg * cross2(e, sub2(a, u));
                let f1 = sg * cross2(e, d);
                if f1 == 0.0 {
                    if f0 < 0.0 {
                        ok = false;
                    }
                    continue;
                }
                let tc = -f0 / f1;
                if f1 > 0.0 {
                    if tc > t0 {
                        t0 = tc;
                        k0 = Some(k);
                    }
                } else if tc < t1 {
                    t1 = tc;
                    k1 = Some(k);
                }
            }
            if !ok || t1 - t0 <= 1e-12 {
                continue;
            }
            let ek = |k: usize| {
                let (u, w) = (t[k], t[(k + 1) % 3]);
                Key::Cross(u.min(w), u.max(w), i as u8)
            };
            let ka = match k0 {
                Some(k) => ek(k),
                None => {
                    let key = Key::Corner(i as u8, tid);
                    pierce[i].push(key);
                    key
                }
            };
            let kb = match k1 {
                Some(k) => ek(k),
                None => {
                    let key = Key::Corner(((i + 1) % nc) as u8, tid);
                    pierce[(i + 1) % nc].push(key);
                    key
                }
            };
            seg_by_facet[i].push((ka, kb));
        }
    }
    for pl in pierce.iter_mut() {
        let mut v: Vec<(f64, Key)> = pl.iter().map(|&k| (cx.key_pos(k)[2], k)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        *pl = v.into_iter().map(|x| x.1).collect();
    }
    for i in 0..nc {
        let mut edges = std::mem::take(&mut seg_by_facet[i]);
        for corner in [i, (i + 1) % nc] {
            for pr in pierce[corner].chunks(2) {
                if pr.len() == 2 {
                    edges.push((pr[0], pr[1]));
                }
            }
        }
        // undirected cycles
        let mut adj: HashMap<Key, Vec<usize>> = HashMap::default();
        for (j, e) in edges.iter().enumerate() {
            adj.entry(e.0).or_default().push(j);
            adj.entry(e.1).or_default().push(j);
        }
        let mut used = vec![false; edges.len()];
        for s in 0..edges.len() {
            if used[s] {
                continue;
            }
            let start = edges[s].0;
            let mut lp = vec![start];
            let mut cur = s;
            let mut at = edges[s].1;
            used[s] = true;
            let mut closed = false;
            loop {
                if at == start {
                    closed = true;
                    break;
                }
                lp.push(at);
                match adj.get(&at).and_then(|v| v.iter().find(|&&j| !used[j])) {
                    Some(&j) => {
                        used[j] = true;
                        cur = j;
                        at = if edges[j].0 == at { edges[j].1 } else { edges[j].0 };
                    }
                    None => break,
                }
            }
            let _ = cur;
            if !closed || lp.len() < 3 {
                if std::env::var("PRISMDBG").is_ok() {
                    eprintln!("prismcut: open wall chain facet {i} len {}", lp.len());
                }
                continue;
            }
            let ps: Vec<V3> = lp.iter().map(|&k| cx.key_pos(k)).collect();
            let n = newell(&ps);
            let mid = [(c[i][0] + c[(i + 1) % nc][0]) / 2.0, (c[i][1] + c[(i + 1) % nc][1]) / 2.0];
            if n[0] * (ctr[0] - mid[0]) + n[1] * (ctr[1] - mid[1]) < 0.0 {
                lp.reverse();
            }
            walls.push(lp);
        }
    }
    // dissolve straight diagonal-crossing vertices
    let mut dissolved: HashSet<Key> = HashSet::default();
    for lp in loops.iter().chain(walls.iter()) {
        let n = lp.len();
        for j in 0..n {
            if let Key::Cross(lo, hi, _) = lp[j] {
                if !diag.contains(&(lo, hi)) {
                    continue;
                }
                let (a, b, d) = (cx.key_pos(lp[(j + n - 1) % n]), cx.key_pos(lp[j]), cx.key_pos(lp[(j + 1) % n]));
                let v1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v2 = [d[0] - b[0], d[1] - b[1], d[2] - b[2]];
                let (l1, l2) = ((v1[0] * v1[0] + v1[1] * v1[1] + v1[2] * v1[2]).sqrt(), (v2[0] * v2[0] + v2[1] * v2[1] + v2[2] * v2[2]).sqrt());
                let cs = (v1[0] * v2[0] + v1[1] * v2[1] + v1[2] * v2[2]) / (l1 * l2);
                if (cs - 1.0).abs() < 1e-4 {
                    dissolved.insert(lp[j]);
                }
            }
        }
    }
    // assemble
    let mut out = Mesh::new();
    let mut remap: HashMap<Key, u32> = HashMap::default();
    let mut faces_k: Vec<Vec<Key>> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        if keep[fi] {
            faces_k.push(f.iter().map(|&v| Key::Orig(v)).collect());
        }
    }
    for f in orig_faces {
        faces_k.push(f.iter().map(|&v| Key::Orig(v)).collect());
    }
    for lp in loops.into_iter().chain(walls.into_iter()) {
        let l: Vec<Key> = lp.into_iter().filter(|k| !dissolved.contains(k)).collect();
        if l.len() >= 3 {
            faces_k.push(l);
        }
    }
    // vertices: originals in index order, then new ones in creation order
    let mut used: HashSet<Key> = HashSet::default();
    for f in &faces_k {
        for &k in f {
            used.insert(k);
        }
    }
    for v in 0..m.pos.len() as u32 {
        if used.contains(&Key::Orig(v)) {
            remap.insert(Key::Orig(v), out.pos.len() as u32);
            out.pos.push(m.pos[v as usize]);
        }
    }
    for f in &faces_k {
        let mut idx = vec![];
        for &k in f {
            let i = match remap.get(&k) {
                Some(&i) => i,
                None => {
                    let p = cx.key_pos(k);
                    out.pos.push(p);
                    remap.insert(k, (out.pos.len() - 1) as u32);
                    (out.pos.len() - 1) as u32
                }
            };
            idx.push(i);
        }
        out.faces.push(idx);
    }
    out
}

/// bpy primitive_cylinder_add (32 vertices, radius 1) scaled to `r`, moved to (cx, 0): corners in the xy plane
pub fn cylinder_corners(cx: f64, r: f64) -> Vec<V2> {
    (0..32)
        .map(|i| {
            let t = 2.0 * std::f64::consts::PI * i as f64 / 32.0;
            [(cx + r * t.cos()) as f32 as f64, (r * t.sin()) as f32 as f64]
        })
        .collect()
}
