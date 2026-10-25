//! Blender BEVEL modifier (edges, limit method ANGLE) for meshes whose beveled edges form loops of 2-edge vertices
//! (valence-4 vertices with two opposite beveled edges): the case of spun / solidified bowls, lids, cups.
//! Validated against python (1554 new vertices within 3.3e-7). Other vertex patterns return None.
//! Offset: the boundary points sit on the unbeveled edges at the mean of w/sin(theta) of both adjacent faces, the profile is a
//! quarter ellipse through the parallelogram (A1, v, A2) and the offset is globally clamped to half the shortest unbeveled
//! edge joining two beveled vertices (clamp overlap).
use crate::mesh::Mesh;
use crate::dhash::HashMap;

type V3 = [f64; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub enum Offset {
    /// distance along the faces
    Width(f64),
    /// percentage (0..100) of the unbeveled edge length
    Percent(f64),
}

fn face_normal(m: &Mesh, f: &[u32]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = len(n);
    if l > 0.0 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        n
    }
}

pub fn bevel(m: &Mesh, offset: Offset, segments: usize, angle_limit: f64) -> Option<Mesh> {
    let nf = m.faces.len();
    let normals: Vec<V3> = m.faces.iter().map(|f| face_normal(m, f)).collect();
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            edges.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut bev: crate::dhash::HashSet<(u32, u32)> = Default::default();
    for (&e, fs) in &edges {
        if fs.len() == 2 {
            let ang = dot(normals[fs[0]], normals[fs[1]]).clamp(-1.0, 1.0).acos();
            if ang > angle_limit + 1e-9 {
                bev.insert(e);
            }
        }
    }
    if bev.is_empty() {
        return Some(m.clone());
    }
    let n = m.pos.len();
    let mut nb: Vec<Vec<u32>> = vec![vec![]; n];
    for &(a, b) in edges.keys() {
        nb[a as usize].push(b);
        nb[b as usize].push(a);
    }
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let mut bv: Vec<bool> = vec![false; n];
    for &(a, b) in &bev {
        bv[a as usize] = true;
        bv[b as usize] = true;
    }
    // classify
    let mut info: HashMap<u32, ([u32; 2], [Option<u32>; 2])> = HashMap::default(); // (beveled neighbours, unbeveled neighbours)
    for v in 0..n as u32 {
        if !bv[v as usize] {
            continue;
        }
        let bn: Vec<u32> = nb[v as usize].iter().cloned().filter(|&u| bev.contains(&key(u, v))).collect();
        let un: Vec<u32> = nb[v as usize].iter().cloned().filter(|&u| !bev.contains(&key(u, v))).collect();
        if bn.len() != 2 || !(un.len() == 2 || un.len() == 1) {
            return None;
        }
        info.insert(v, ([bn[0], bn[1]], if un.len() == 2 { [Some(un[0]), Some(un[1])] } else { [Some(un[0]), None] }));
    }
    // clamp
    let w0 = match offset {
        Offset::Width(w) => w,
        Offset::Percent(_) => f64::INFINITY,
    };
    // distance along the unbeveled edge u from v per unit width: mean of 1/sin(theta) towards the two beveled edges
    let factor = |v: u32, u: u32| -> f64 {
        let (bn, _) = info[&v];
        let pv = m.pos[v as usize];
        let du = sub(m.pos[u as usize], pv);
        let lu = len(du);
        let mut s = 0.0;
        for &e in &bn {
            let de = sub(m.pos[e as usize], pv);
            let c = dot(du, de) / (lu * len(de));
            s += 1.0 / c.clamp(-1.0, 1.0).acos().sin();
        }
        s / 2.0
    };
    let mut w = w0;
    for (&(a, b), _) in &edges {
        if bev.contains(&(a, b)) {
            continue;
        }
        let l = len(sub(m.pos[a as usize], m.pos[b as usize]));
        match (bv[a as usize], bv[b as usize]) {
            (true, true) => w = w.min(l / (factor(a, b) + factor(b, a))),
            (true, false) => w = w.min(l / factor(a, b)),
            (false, true) => w = w.min(l / factor(b, a)),
            _ => {}
        }
    }
    // boundary points of every beveled vertex: ends[v][0], ends[v][1] (one per side)
    let mut ends: HashMap<u32, [V3; 2]> = HashMap::default();
    for (&v, &(bn, sides)) in &info {
        let pv = m.pos[v as usize];
        let mut pts = [pv; 2];
        for (si, side) in sides.iter().enumerate() {
            if let Some(u) = side {
                let du = sub(m.pos[*u as usize], pv);
                let lu = len(du);
                let dir = [du[0] / lu, du[1] / lu, du[2] / lu];
                let dist = match offset {
                    Offset::Width(_) => {
                        let mut s = 0.0;
                        for &e in &bn {
                            let de = sub(m.pos[e as usize], pv);
                            let c = dot(dir, de) / len(de);
                            s += w / c.clamp(-1.0, 1.0).acos().sin();
                        }
                        s / 2.0
                    }
                    Offset::Percent(p) => lu * p / 100.0,
                };
                pts[si] = [pv[0] + dir[0] * dist, pv[1] + dir[1] * dist, pv[2] + dir[2] * dist];
            } else {
                // the two beveled edges bound a common face: offset lines meet on the bisector
                let d1 = sub(m.pos[bn[0] as usize], pv);
                let d2 = sub(m.pos[bn[1] as usize], pv);
                let (l1, l2) = (len(d1), len(d2));
                let (u1, u2) = ([d1[0] / l1, d1[1] / l1, d1[2] / l1], [d2[0] / l2, d2[1] / l2, d2[2] / l2]);
                let th = dot(u1, u2).clamp(-1.0, 1.0).acos();
                let bis = [u1[0] + u2[0], u1[1] + u2[1], u1[2] + u2[2]];
                let lb = len(bis);
                let h = w / (th / 2.0).sin();
                pts[si] = [pv[0] + bis[0] / lb * h, pv[1] + bis[1] / lb * h, pv[2] + bis[2] / lb * h];
            }
        }
        ends.insert(v, pts);
    }
    // which side (0/1) of vertex v does face f (given its neighbours p, q around v) lie on
    let side_of = |v: u32, p: u32, q: u32| -> Option<usize> {
        let (_, sides) = info[&v];
        for (si, side) in sides.iter().enumerate() {
            match side {
                Some(u) if *u == p || *u == q => return Some(si),
                _ => {}
            }
        }
        // both neighbours beveled: the cap side
        sides.iter().position(|s| s.is_none())
    };
    let mut out = Mesh::new();
    let mut map_plain: Vec<u32> = vec![u32::MAX; n];
    let mut rows: HashMap<u32, Vec<u32>> = HashMap::default();
    for v in 0..n as u32 {
        if !bv[v as usize] {
            map_plain[v as usize] = out.pos.len() as u32;
            out.pos.push(m.pos[v as usize]);
        } else {
            let [a1, a2] = ends[&v];
            let pv = m.pos[v as usize];
            let c = [a1[0] + a2[0] - pv[0], a1[1] + a2[1] - pv[1], a1[2] + a2[2] - pv[2]];
            let mut r = vec![];
            for k in 0..=segments {
                let ph = std::f64::consts::FRAC_PI_2 * k as f64 / segments as f64;
                let (cs, sn) = (ph.cos(), ph.sin());
                let p = [0, 1, 2].map(|j| c[j] + (a1[j] - c[j]) * cs + (a2[j] - c[j]) * sn);
                r.push(out.pos.len() as u32);
                out.pos.push(p);
            }
            rows.insert(v, r);
        }
    }
    for f in &m.faces {
        let k = f.len();
        let mut g = vec![];
        for i in 0..k {
            let v = f[i];
            if !bv[v as usize] {
                g.push(map_plain[v as usize]);
            } else {
                let (p, q) = (f[(i + k - 1) % k], f[(i + 1) % k]);
                let si = side_of(v, p, q)?;
                g.push(if si == 0 { rows[&v][0] } else { rows[&v][segments] });
            }
        }
        out.faces.push(g);
    }
    let mut done: crate::dhash::HashSet<(u32, u32)> = Default::default();
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let e = key(a, b);
            if !bev.contains(&e) || edges[&e][0] != fi || !done.insert(e) {
                continue;
            }
            // strip rows run from the side of face fa (row 0) to the side of the other face
            let nbr = |v: u32| -> (u32, u32) {
                let idx = f.iter().position(|&x| x == v).unwrap();
                (f[(idx + k - 1) % k], f[(idx + 1) % k])
            };
            let (pa, qa) = nbr(a);
            let (pb, qb) = nbr(b);
            let sa = side_of(a, pa, qa)?;
            let sb = side_of(b, pb, qb)?;
            let ra = &rows[&a];
            let rb = &rows[&b];
            let ia = |j: usize| if sa == 0 { ra[j] } else { ra[segments - j] };
            let ib = |j: usize| if sb == 0 { rb[j] } else { rb[segments - j] };
            for j in 0..segments {
                out.faces.push(vec![ib(j), ia(j), ia(j + 1), ib(j + 1)]);
            }
        }
    }
    let _ = nf;
    Some(out)
}
