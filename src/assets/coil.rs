//! mattress.py make_coiled: poke / tris_convert_to_quads / poke x2 / vertex bevel of the 16-edge vertices / two region extrusions
//! (shrink-fatten along the cap normal) / ScaleElements of the stalks / triangulate / SMOOTH modifier.
use crate::mesh::*;
use crate::dhash::HashMap;

type V3 = [f64; 3];

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len3(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm3(a: V3) -> V3 {
    let l = len3(a);
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { a }
}

pub fn face_normal(m: &Mesh, f: &[u32]) -> V3 {
    // Newell
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    norm3(n)
}

/// BM_face_calc_center_median_weighted
fn center_weighted(m: &Mesh, f: &[u32]) -> V3 {
    let k = f.len();
    let mut c = [0.0; 3];
    let mut tot = 0.0;
    for i in 0..k {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % k] as usize]);
        let w = len3(sub3(a, b));
        for j in 0..3 {
            c[j] += w * 0.5 * (a[j] + b[j]);
        }
        tot += w;
    }
    if tot > 0.0 { [c[0] / tot, c[1] / tot, c[2] / tot] } else { m.pos[f[0] as usize] }
}

/// bpy.ops.mesh.poke on all faces
pub fn poke(m: &Mesh) -> Mesh {
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    for f in &m.faces {
        let c = center_weighted(m, f);
        let ci = o.pos.len() as u32;
        o.pos.push(c);
        let k = f.len();
        for i in 0..k {
            o.faces.push(vec![f[i], f[(i + 1) % k], ci]);
        }
    }
    o
}

fn corner_angles(p: &[V3]) -> Vec<f64> {
    let k = p.len();
    (0..k)
        .map(|i| {
            let (a, b, c) = (p[(i + k - 1) % k], p[i], p[(i + 1) % k]);
            let (u, v) = (norm3(sub3(a, b)), norm3(sub3(c, b)));
            dot3(u, v).clamp(-1.0, 1.0).acos()
        })
        .collect()
}

/// bpy.ops.mesh.tris_convert_to_quads (face threshold 40, shape threshold 40): greedy pairing by quad shape error
pub fn tris_to_quads(m: &Mesh) -> Mesh {
    let lim = 40f64.to_radians();
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        if f.len() != 3 {
            continue;
        }
        for i in 0..3 {
            let (a, b) = (f[i], f[(i + 1) % 3]);
            edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut cands: Vec<(f64, usize, usize, Vec<u32>)> = vec![];
    let mut keys: Vec<&(u32, u32)> = edge_faces.keys().collect();
    keys.sort();
    for k in keys {
        let fs = &edge_faces[k];
        if fs.len() != 2 || m.faces[fs[0]].len() != 3 || m.faces[fs[1]].len() != 3 {
            continue;
        }
        let (t1, t2) = (&m.faces[fs[0]], &m.faces[fs[1]]);
        // shared edge as A->B in t1
        let mut quad = None;
        for i in 0..3 {
            let (a, b, c) = (t1[i], t1[(i + 1) % 3], t1[(i + 2) % 3]);
            if (a.min(b), a.max(b)) == *k {
                let d = *t2.iter().find(|&&v| v != a && v != b).unwrap();
                quad = Some(vec![a, d, b, c]);
            }
        }
        let q = quad.unwrap();
        let (n1, n2) = (face_normal(m, t1), face_normal(m, t2));
        if dot3(n1, n2).clamp(-1.0, 1.0).acos() > lim {
            continue;
        }
        let pts: Vec<V3> = q.iter().map(|&i| m.pos[i as usize]).collect();
        let ang = corner_angles(&pts);
        if ang.iter().any(|&a| a >= std::f64::consts::PI - 1e-6) {
            continue;
        }
        let dev: Vec<f64> = ang.iter().map(|a| (a - std::f64::consts::FRAC_PI_2).abs()).collect();
        if dev.iter().cloned().fold(0.0, f64::max) > lim {
            continue;
        }
        // convexity: the two triangle normals are consistent with the quad normal
        let err: f64 = dev.iter().sum();
        cands.push((err, fs[0], fs[1], q));
    }
    cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut used = vec![false; m.faces.len()];
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    let mut quads = vec![];
    for (_, a, b, q) in cands {
        if used[a] || used[b] {
            continue;
        }
        used[a] = true;
        used[b] = true;
        quads.push((a.min(b), q));
    }
    let mut qi: HashMap<usize, Vec<u32>> = quads.into_iter().collect();
    for (fi, f) in m.faces.iter().enumerate() {
        if let Some(q) = qi.remove(&fi) {
            o.faces.push(q);
        } else if !used[fi] {
            o.faces.push(f.clone());
        }
    }
    o
}

/// neighbours of v in rotational order with the face that follows each: returns Vec<(w_i, face index containing w_{i-1} -> v -> w_i)>
fn vertex_fan(m: &Mesh, vf: &[Vec<usize>], v: u32) -> Option<Vec<(u32, usize)>> {
    let mut next_of: HashMap<u32, (u32, usize)> = HashMap::default(); // prev -> (next, face)
    for &fi in &vf[v as usize] {
        let f = &m.faces[fi];
        let k = f.len();
        let i = f.iter().position(|&x| x == v)?;
        next_of.insert(f[(i + k - 1) % k], (f[(i + 1) % k], fi));
    }
    let start = *next_of.keys().min()?;
    let mut out = vec![];
    let mut cur = start;
    loop {
        let (nx, fi) = *next_of.get(&cur)?;
        out.push((nx, fi));
        cur = nx;
        if cur == start || out.len() > next_of.len() {
            break;
        }
    }
    if out.len() != next_of.len() || cur != start {
        return None;
    }
    Some(out)
}

pub struct CoilOut {
    pub mesh: Mesh,
    /// faces of the stalk islands (cap + two wall layers) per tuft
    pub tip: Vec<usize>,
}

/// vertex bevel (offset = distance along every edge) of `verts`, then the two region extrusions of the new caps
pub fn bevel_and_extrude(m: &Mesh, verts: &[u32], radius: f64, depth: f64) -> CoilOut {
    let n = m.pos.len();
    let mut vf: Vec<Vec<usize>> = vec![vec![]; n];
    for (fi, f) in m.faces.iter().enumerate() {
        for &v in f {
            vf[v as usize].push(fi);
        }
    }
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    let mut faces: Vec<Vec<u32>> = m.faces.clone();
    let mut caps: Vec<Vec<u32>> = vec![];
    let mut dead = vec![false; n];
    for &v in verts {
        let fan = match vertex_fan(m, &vf, v) {
            Some(f) => f,
            None => continue,
        };
        let k = fan.len();
        let pv = m.pos[v as usize];
        let mut ring: Vec<u32> = vec![];
        for &(w, _) in &fan {
            let d = norm3(sub3(m.pos[w as usize], pv));
            let q = [pv[0] + d[0] * radius, pv[1] + d[1] * radius, pv[2] + d[2] * radius];
            ring.push(o.pos.len() as u32);
            o.pos.push(q);
        }
        // face fan[i].1 contains w_{i-1} -> v -> w_i ; replace v by (p_{i-1}, p_i)
        for i in 0..k {
            let fi = fan[i].1;
            let f = &mut faces[fi];
            let pos = f.iter().position(|&x| x == v).unwrap();
            let (pa, pb) = (ring[(i + k - 1) % k], ring[i]);
            f.splice(pos..pos + 1, [pa, pb]);
        }
        dead[v as usize] = true;
        // cap: edges p_{i-1} -> p_i appear in the faces; the cap runs the other way
        let mut cap: Vec<u32> = ring.clone();
        cap.reverse();
        caps.push(cap);
    }
    // orient caps like the average neighbouring face normal
    for cap in caps.iter_mut() {
        let tmp = Mesh { pos: o.pos.clone(), faces: vec![], uv: vec![] };
        let nrm = face_normal(&tmp, cap);
        let _ = nrm;
    }
    // compact vertices
    let mut map = vec![u32::MAX; o.pos.len()];
    let mut pos = vec![];
    for (i, p) in o.pos.iter().enumerate() {
        if i < n && dead[i] {
            continue;
        }
        map[i] = pos.len() as u32;
        pos.push(*p);
    }
    let mut res = Mesh::new();
    res.pos = pos;
    for f in &faces {
        res.faces.push(f.iter().map(|&v| map[v as usize]).collect());
    }
    let mut tip = vec![];
    for cap in &caps {
        let c: Vec<u32> = cap.iter().map(|&v| map[v as usize]).collect();
        // normal: must agree with the surface normal (away from the mesh): compare with neighbour face normal
        let cn = face_normal(&res, &c);
        let nb = res.faces.iter().find(|f| f.iter().any(|v| c.contains(v)) && f.len() >= 3).cloned().unwrap();
        let nn = face_normal(&res, &nb);
        let c = if dot3(cn, nn) < 0.0 { c.into_iter().rev().collect::<Vec<_>>() } else { c };
        let nrm = face_normal(&res, &c);
        let kk = c.len();
        // ext1: ring L1 = L0 - depth * n ; ext2: ring L2 = L1 + depth * n
        let l1: Vec<u32> = c.iter().map(|&v| {
            let p = res.pos[v as usize];
            res.pos.push([p[0] - nrm[0] * depth, p[1] - nrm[1] * depth, p[2] - nrm[2] * depth]);
            (res.pos.len() - 1) as u32
        }).collect();
        let l2: Vec<u32> = l1.iter().map(|&v| {
            let p = res.pos[v as usize];
            res.pos.push([p[0] + nrm[0] * depth, p[1] + nrm[1] * depth, p[2] + nrm[2] * depth]);
            (res.pos.len() - 1) as u32
        }).collect();
        for i in 0..kk {
            let j = (i + 1) % kk;
            tip.push(res.faces.len());
            res.faces.push(vec![c[i], c[j], l1[j], l1[i]]);
            tip.push(res.faces.len());
            res.faces.push(vec![l1[i], l1[j], l2[j], l2[i]]);
        }
        tip.push(res.faces.len());
        res.faces.push(l2.clone());
    }
    CoilOut { mesh: res, tip }
}

/// vertex degree
pub fn degrees(m: &Mesh) -> Vec<usize> {
    let mut es: crate::dhash::HashSet<(u32, u32)> = crate::dhash::HashSet::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            es.insert((a.min(b), a.max(b)));
        }
    }
    let mut d = vec![0usize; m.pos.len()];
    for (a, b) in es {
        d[a as usize] += 1;
        d[b as usize] += 1;
    }
    d
}

/// GN ScaleElements (face domain, uniform) with islands connected through shared vertices; centre = mean of the face centres
pub fn scale_islands(m: &Mesh, tip: &[usize], s: f64) -> Mesh {
    let n = m.pos.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for &fi in tip {
        let f = &m.faces[fi];
        for w in f.windows(2) {
            let (a, b) = (find(&mut parent, w[0] as usize), find(&mut parent, w[1] as usize));
            parent[a] = b;
        }
    }
    let mut acc: HashMap<usize, ([f64; 3], usize)> = HashMap::default();
    let mut used = vec![false; n];
    for &fi in tip {
        let f = &m.faces[fi];
        let mut c = [0.0; 3];
        for &v in f {
            used[v as usize] = true;
            for j in 0..3 {
                c[j] += m.pos[v as usize][j] / f.len() as f64;
            }
        }
        let r = find(&mut parent, f[0] as usize);
        let e = acc.entry(r).or_insert(([0.0; 3], 0));
        for j in 0..3 {
            e.0[j] += c[j];
        }
        e.1 += 1;
    }
    let mut o = m.clone();
    for v in 0..n {
        if !used[v] {
            continue;
        }
        let r = find(&mut parent, v);
        let (c, k) = acc[&r];
        let c = [c[0] / k as f64, c[1] / k as f64, c[2] / k as f64];
        for j in 0..3 {
            o.pos[v][j] = c[j] + (m.pos[v][j] - c[j]) * s;
        }
    }
    o
}

/// TRIANGULATE (quads / ngons, min_vertices 4) as fans from the shortest-diagonal split (beauty is not reproduced)
pub fn triangulate(m: &Mesh) -> Mesh {
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    for f in &m.faces {
        if f.len() <= 3 {
            o.faces.push(f.clone());
        } else if f.len() == 4 {
            let c = |i: usize| { let p = m.pos[f[i] as usize]; [p[0] as f32, p[1] as f32, p[2] as f32] };
            if super::polyfill::quad_split_24_beauty([c(0), c(1), c(2), c(3)]) {
                o.faces.push(vec![f[0], f[1], f[2]]);
                o.faces.push(vec![f[0], f[2], f[3]]);
            } else {
                o.faces.push(vec![f[0], f[1], f[3]]);
                o.faces.push(vec![f[1], f[2], f[3]]);
            }
        } else {
            let co: Vec<[f32; 3]> = f.iter().map(|&v| { let p = m.pos[v as usize]; [p[0] as f32, p[1] as f32, p[2] as f32] }).collect();
            for t in super::polyfill::triangulate_ngon(&co) {
                o.faces.push(vec![f[t[0] as usize], f[t[1] as usize], f[t[2] as usize]]);
            }
        }
    }
    o
}

/// SMOOTH modifier (all axes): each iteration moves every vertex towards the mean of its edge neighbours by `fac`
pub fn smooth(m: &Mesh, fac: f64, iterations: usize) -> Mesh {
    let mut es: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            es.insert((a.min(b), a.max(b)));
        }
    }
    let mut o = m.clone();
    for _ in 0..iterations {
        let mut acc = vec![[0.0f64; 3]; o.pos.len()];
        let mut cnt = vec![0usize; o.pos.len()];
        for &(a, b) in &es {
            let (pa, pb) = (o.pos[a as usize], o.pos[b as usize]);
            for j in 0..3 {
                let mid = (pa[j] + pb[j]) * 0.5;
                acc[a as usize][j] += mid;
                acc[b as usize][j] += mid;
            }
            cnt[a as usize] += 1;
            cnt[b as usize] += 1;
        }
        for i in 0..o.pos.len() {
            if cnt[i] == 0 {
                continue;
            }
            for j in 0..3 {
                let avg = acc[i][j] / cnt[i] as f64;
                o.pos[i][j] = o.pos[i][j] * (1.0 - fac) + avg * fac;
            }
        }
    }
    o
}

/// mattress.make_coiled; `radius_u` / `smooth_f` are the two stream draws (uniform(.06,.08), uniform(.5,1))
pub fn make_coiled(m: &Mesh, dot_distance: f64, dot_depth: f64, dot_size: f64, radius_u: f64, smooth_f: f64) -> Mesh {
    let d = if std::env::var("COIL_OLD").is_ok() {
        let a = poke(m);
        let b = tris_to_quads(&a);
        let c = poke(&b);
        poke(&c)
    } else {
        poke_chain_bm(m)
    };
    let deg = degrees(&d);
    let verts: Vec<u32> = (0..d.pos.len() as u32).filter(|&v| deg[v as usize] == 16).collect();
    let radius = dot_distance * radius_u;
    let out = bevel_and_extrude(&d, &verts, radius, dot_depth);
    if std::env::var("COIL_STAGE").map(|v| v == "extr").unwrap_or(false) {
        return out.mesh;
    }
    if std::env::var("COIL_DBG").is_ok() { eprintln!("tufts {} tipfaces {} verts {} deg16 {}", verts.len(), out.tip.len(), out.mesh.pos.len(), verts.len()); }
    let scaled = scale_islands(&out.mesh, &out.tip, dot_size / radius);
    if std::env::var("COIL_STAGE").map(|v| v == "scale").unwrap_or(false) {
        return scaled;
    }
    let t = triangulate(&scaled);
    if std::env::var("COIL_STAGE").map(|v| v == "tri").unwrap_or(false) {
        return t;
    }
    smooth(&t, smooth_f, 5)
}

/// poke / tris_convert_to_quads / poke / poke on a BMesh in one edit session (float32, Blender's pairing and element order)
pub fn poke_chain_bm(m: &Mesh) -> Mesh {
    use crate::bm::BMesh;
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut ce: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([a, b]);
                (edges.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    crate::bm::hull::poke_all(&mut bm);
    crate::bm::hull::tris_to_quads_all(&mut bm);
    crate::bm::hull::poke_all(&mut bm);
    crate::bm::hull::poke_all(&mut bm);
    let (p, _e, f) = bm.to_mesh();
    let mut o = Mesh::new();
    o.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    o.faces = f;
    o
}
