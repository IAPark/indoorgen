//! NatureShelfTrinketsFactory.create_asset for the non-creature branch: base asset -> trimesh stable pose (convex hull + toppling graph,
//! trimesh.poses.compute_stable_poses with the mesh centre of mass) -> rotation (mathutils euler, float32) -> fit into the placeholder cube.
use crate::bl32;
use crate::dhash::HashMap;

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
fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// triangulated convex hull (outward oriented), incremental
pub fn hull_tris(p: &[P3]) -> Vec<[usize; 3]> {
    if p.len() > 4000 {
        // quickhull-style candidate growth: hull of directional extremes, then repeatedly add the farthest outside point of every face;
        // points never outside the final candidate hull cannot be hull vertices, so the plain incremental hull runs on a small subset
        let mut dirs: Vec<P3> = vec![];
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if x != 0 || y != 0 || z != 0 {
                        dirs.push([x as f64, y as f64, z as f64]);
                    }
                }
            }
        }
        let mut cand: Vec<usize> = dirs
            .iter()
            .map(|d| (0..p.len()).max_by(|&a, &b| dot(p[a], *d).partial_cmp(&dot(p[b], *d)).unwrap().then(b.cmp(&a))).unwrap())
            .collect();
        cand.sort();
        cand.dedup();
        for _round in 0..64 {
            let cp: Vec<P3> = cand.iter().map(|&i| p[i]).collect();
            let ch = hull_tris_plain(&cp);
            if ch.is_empty() {
                break;
            }
            let planes: Vec<(P3, f64)> = ch
                .iter()
                .map(|f| {
                    let n = cross(sub(cp[f[1]], cp[f[0]]), sub(cp[f[2]], cp[f[0]]));
                    let l = norm(n);
                    let n = [n[0] / l, n[1] / l, n[2] / l];
                    (n, dot(n, cp[f[0]]))
                })
                .collect();
            let mut far: Vec<(f64, usize)> = vec![(1e-9, usize::MAX); planes.len()];
            for i in 0..p.len() {
                // the face this point is farthest outside of
                let mut bf = usize::MAX;
                let mut bd = 1e-9;
                for (fi, (n, d)) in planes.iter().enumerate() {
                    let h = dot(*n, p[i]) - d;
                    if h > bd {
                        bd = h;
                        bf = fi;
                    }
                }
                if bf != usize::MAX && bd > far[bf].0 {
                    far[bf] = (bd, i);
                }
            }
            let mut added = false;
            for (_, i) in far {
                if i != usize::MAX && !cand.contains(&i) {
                    cand.push(i);
                    added = true;
                }
            }
            if !added {
                break;
            }
            cand.sort();
        }
        let sub_pts: Vec<P3> = cand.iter().map(|&i| p[i]).collect();
        return hull_tris_plain(&sub_pts).into_iter().map(|f| [cand[f[0]], cand[f[1]], cand[f[2]]]).collect();
    }
    hull_tris_plain(p)
}

fn hull_tris_plain(p: &[P3]) -> Vec<[usize; 3]> {
    let n = p.len();
    let i0 = 0;
    let i1 = match (1..n).find(|&i| norm(sub(p[i], p[i0])) > 1e-12) {
        Some(i) => i,
        None => return vec![],
    };
    let i2 = match (1..n).find(|&i| i != i1 && norm(cross(sub(p[i1], p[i0]), sub(p[i], p[i0]))) > 1e-12) {
        Some(i) => i,
        None => return vec![],
    };
    let nrm = cross(sub(p[i1], p[i0]), sub(p[i2], p[i0]));
    let i3 = match (1..n).find(|&i| i != i1 && i != i2 && dot(nrm, sub(p[i], p[i0])).abs() > 1e-12) {
        Some(i) => i,
        None => return vec![],
    };
    let mut faces: Vec<[usize; 3]> = if dot(nrm, sub(p[i3], p[i0])) > 0.0 {
        vec![[i0, i2, i1], [i0, i1, i3], [i1, i2, i3], [i2, i0, i3]]
    } else {
        vec![[i0, i1, i2], [i0, i3, i1], [i1, i3, i2], [i2, i3, i0]]
    };
    let normal = |p: &[P3], f: &[usize; 3]| cross(sub(p[f[1]], p[f[0]]), sub(p[f[2]], p[f[0]]));
    let eps = 1e-12;
    for i in 0..n {
        if [i0, i1, i2, i3].contains(&i) {
            continue;
        }
        let visible: Vec<bool> = faces
            .iter()
            .map(|f| {
                let nn = normal(p, f);
                let l = norm(nn);
                l > 0.0 && dot(nn, sub(p[i], p[f[0]])) / l > 1e-10
            })
            .collect();
        if !visible.iter().any(|&v| v) {
            continue;
        }
        let mut edges: HashMap<(usize, usize), ()> = HashMap::default();
        for (fi, f) in faces.iter().enumerate() {
            if visible[fi] {
                for k in 0..3 {
                    edges.insert((f[k], f[(k + 1) % 3]), ());
                }
            }
        }
        let mut nf: Vec<[usize; 3]> = faces.iter().enumerate().filter(|(fi, _)| !visible[*fi]).map(|(_, f)| *f).collect();
        let mut hor: Vec<(usize, usize)> = edges.keys().filter(|&&(a, b)| !edges.contains_key(&(b, a))).cloned().collect();
        hor.sort();
        for (a, b) in hor {
            nf.push([a, b, i]);
        }
        faces = nf;
    }
    faces
}

/// trimesh mass properties (density 1): centre of mass of a closed triangle mesh
pub fn center_mass(p: &[P3], tris: &[[usize; 3]]) -> P3 {
    let mut vol = 0.0;
    let mut c = [0.0; 3];
    for t in tris {
        let (a, b, d) = (p[t[0]], p[t[1]], p[t[2]]);
        let v = dot(a, cross(b, d)) / 6.0;
        vol += v;
        for k in 0..3 {
            c[k] += v * (a[k] + b[k] + d[k]) / 4.0;
        }
    }
    [c[0] / vol, c[1] / vol, c[2] / vol]
}

fn static_prob(tri: [P3; 3], com: P3) -> f64 {
    let sv: Vec<P3> = tri
        .iter()
        .map(|&v| {
            let d = sub(v, com);
            let l = norm(d);
            [d[0] / l, d[1] / l, d[2] / l]
        })
        .collect();
    let a = dot(sv[0], sv[1]).clamp(-1.0, 1.0).acos();
    let b = dot(sv[1], sv[2]).clamp(-1.0, 1.0).acos();
    let c = dot(sv[2], sv[0]).clamp(-1.0, 1.0).acos();
    let s = (a + b + c) / 2.0;
    1.0 / std::f64::consts::PI * ((s / 2.0).tan() * ((s - a) / 2.0).tan() * ((s - b) / 2.0).tan() * ((s - c) / 2.0).tan()).sqrt().atan()
}

fn orient3d(plane: [P3; 3], pd: P3) -> f64 {
    let (pa, pb, pc) = (plane[0], plane[1], plane[2]);
    let (adx, bdx, cdx) = (pa[0] - pd[0], pb[0] - pd[0], pc[0] - pd[0]);
    let (ady, bdy, cdy) = (pa[1] - pd[1], pb[1] - pd[1], pc[1] - pd[1]);
    let (adz, bdz, cdz) = (pa[2] - pd[2], pb[2] - pd[2], pc[2] - pd[2]);
    adx * (bdy * cdz - bdz * cdy) + bdx * (cdy * adz - cdz * ady) + cdx * (ady * bdz - adz * bdy)
}

/// trimesh.poses.compute_stable_poses(mesh)[0][0]: rows (x, y, z) of the most probable pose rotation
pub fn stable_pose(verts: &[P3], tris: &[[usize; 3]]) -> [[f64; 3]; 3] {
    let com = center_mass(verts, tris);
    let hp: Vec<[usize; 3]> = hull_tris(verts);
    if std::env::var("TRDBG").is_ok() { eprintln!("COM {:?} hull tris {}", com, hp.len()); }
    // hull vertices are used in place (triangles reference the input vertices)
    let nf = hp.len();
    let tv = |i: usize| -> [P3; 3] { [verts[hp[i][0]], verts[hp[i][1]], verts[hp[i][2]]] };
    let nrm: Vec<P3> = (0..nf)
        .map(|i| {
            let t = tv(i);
            let c = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let l = norm(c);
            [c[0] / l, c[1] / l, c[2] / l]
        })
        .collect();
    let cen: Vec<P3> = (0..nf)
        .map(|i| {
            let t = tv(i);
            [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0, (t[0][2] + t[1][2] + t[2][2]) / 3.0]
        })
        .collect();
    // adjacency through shared edges
    let mut emap: HashMap<(usize, usize), Vec<usize>> = HashMap::default();
    for (fi, f) in hp.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            emap.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut adj: Vec<Vec<(usize, (usize, usize))>> = vec![vec![]; nf];
    let mut keys: Vec<&(usize, usize)> = emap.keys().collect();
    keys.sort();
    for k in keys {
        let fs = &emap[k];
        if fs.len() == 2 {
            adj[fs[0]].push((fs[1], *k));
            adj[fs[1]].push((fs[0], *k));
        }
    }
    let mut prob: Vec<f64> = (0..nf).map(|i| static_prob(tv(i), com)).collect();
    if std::env::var("TRDBG").is_ok() {
        let nonman = emap.values().filter(|v| v.len() != 2).count();
        let nan = prob.iter().filter(|x| !x.is_finite()).count();
        let mut a: Vec<(f64, usize)> = prob.iter().cloned().enumerate().map(|(i, x)| (x, i)).collect();
        a.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
        eprintln!("hull: nonmanifold edges {} nan {} sum prob {} top {:?}", nonman, nan, prob.iter().sum::<f64>(), &a[..4]);
    }
    let mut succ: Vec<Option<usize>> = vec![None; nf];
    for fi in 0..nf {
        let t = tv(fi);
        let d = dot(nrm[fi], sub(com, t[0]));
        let pc = [com[0] - d * nrm[fi][0], com[1] - d * nrm[fi][1], com[2] - d * nrm[fi][2]];
        // barycentric
        let e0 = sub(t[1], t[0]);
        let e1 = sub(t[2], t[0]);
        let w = sub(pc, t[0]);
        let (d00, d01, d11, d02, d12) = (dot(e0, e0), dot(e0, e1), dot(e1, e1), dot(e0, w), dot(e1, w));
        let den = d00 * d11 - d01 * d01;
        let v = (d11 * d02 - d01 * d12) / den;
        let ww = (d00 * d12 - d01 * d02) / den;
        let u = 1.0 - v - ww;
        if !(u < 0.0 || v < 0.0 || ww < 0.0) {
            continue;
        }
        let mut chosen = None;
        for &(tfi, (a, b)) in &adj[fi] {
            let (mut v1, mut v2) = (verts[a], verts[b]);
            if dot(cross(sub(v1, cen[fi]), sub(v2, cen[fi])), nrm[fi]) < 0.0 {
                std::mem::swap(&mut v1, &mut v2);
            }
            let add = |x: P3, y: P3| [x[0] + y[0], x[1] + y[1], x[2] + y[2]];
            let plane1 = [cen[fi], v1, add(v1, nrm[fi])];
            let plane2 = [cen[fi], add(v2, nrm[fi]), v2];
            chosen = Some(tfi);
            if orient3d(plane1, pc) >= 0.0 && orient3d(plane2, pc) >= 0.0 {
                break;
            }
        }
        succ[fi] = chosen;
    }
    let mut indeg = vec![0usize; nf];
    for s in succ.iter().flatten() {
        indeg[*s] += 1;
    }
    let mut nodes: Vec<usize> = (0..nf).filter(|&i| indeg[i] == 0).collect();
    let mut iters = 0;
    while !nodes.is_empty() && iters <= tris.len() {
        let mut new_nodes = vec![];
        for &n in &nodes {
            let Some(s) = succ[n] else { continue };
            prob[s] += prob[n];
            prob[n] = 0.0;
            new_nodes.push(s);
        }
        nodes = new_nodes;
        iters += 1;
    }
    // collect stable poses keyed by the rounded normal
    let mut order: Vec<(i64, i64, i64)> = vec![];
    let mut acc: HashMap<(i64, i64, i64), (f64, P3)> = HashMap::default();
    for n in 0..nf {
        if prob[n] > 0.0 {
            let k = ((nrm[n][0] * 1000.0).round() as i64, (nrm[n][1] * 1000.0).round() as i64, (nrm[n][2] * 1000.0).round() as i64);
            match acc.get_mut(&k) {
                Some(e) => e.0 += prob[n],
                None => {
                    acc.insert(k, (prob[n], nrm[n]));
                    order.push(k);
                }
            }
        }
    }
    let mut best = order[0];
    for k in &order {
        if acc[k].0 > acc[&best].0 {
            best = *k;
        }
    }
    let normal = acc[&best].1;
    if std::env::var("TRDBG").is_ok() {
        let mut v: Vec<f64> = acc.values().map(|e| e.0).collect();
        v.sort_by(|a, b| b.partial_cmp(a).unwrap());
        eprintln!("nposes {} sum {} probs {:?} best normal {:?}", v.len(), v.iter().sum::<f64>(), &v[..6.min(v.len())], normal);
        let mut l: Vec<_> = acc.iter().map(|(k, e)| (e.0, e.1)).collect();
        l.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for (pr, n) in l.iter().take(8) { eprintln!("  {:.4} {:.3?}", pr, n); }
    }
    let z = [-normal[0], -normal[1], -normal[2]];
    let mut x = [-z[1], z[0], 0.0];
    if norm(x) == 0.0 {
        x = [1.0, 0.0, 0.0];
    } else {
        let l = norm(x);
        x = [x[0] / l, x[1] / l, x[2] / l];
    }
    let y0 = cross(z, x);
    let l = norm(y0);
    let y = [y0[0] / l, y0[1] / l, y0[2] / l];
    [x, y, z]
}

/// mathutils.Matrix(rows).to_euler(): float32 euler of the 3x3 given as rows
pub fn rows_to_euler(r: [[f64; 3]; 3]) -> [f32; 3] {
    let mut m = [[0.0f32; 3]; 3];
    for col in 0..3 {
        for row in 0..3 {
            m[col][row] = r[row][col] as f32;
        }
    }
    crate::gn::rot::mat3_to_eul_plain(&m)
}

/// the tail of create_asset: pose, fit into the cube of side `size`, place the bottom at -size/2
pub fn finish_trinket(mut pos: Vec<[f32; 3]>, tris: &[[usize; 3]], size: f64) -> Vec<[f32; 3]> {
    let pv: Vec<P3> = pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    let rows = stable_pose(&pv, tris);
    let eul = rows_to_euler(rows);
    let m = bl32::eul_to_mat3(eul);
    for p in pos.iter_mut() {
        let (x, y, z) = (p[0], p[1], p[2]);
        *p = [x * m[0][0] + y * m[1][0] + z * m[2][0], x * m[0][1] + y * m[1][1] + z * m[2][1], x * m[0][2] + y * m[1][2] + z * m[2][2]];
    }
    let bounds = |pos: &Vec<[f32; 3]>| {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    };
    let sz = size as f32;
    let (lo, hi) = bounds(&pos);
    let scale = (0..3).map(|k| (sz as f64) / ((hi[k] - lo[k]) as f64)).fold(f64::INFINITY, f64::min) as f32;
    for p in pos.iter_mut() {
        for k in 0..3 {
            p[k] *= scale;
        }
    }
    let (lo, hi) = bounds(&pos);
    let loc = [-(lo[0] + hi[0]) / 2.0, -(lo[1] + hi[1]) / 2.0, (0.0 - (lo[2] as f64 + size / 2.0)) as f32];
    for p in pos.iter_mut() {
        for k in 0..3 {
            p[k] += loc[k];
        }
    }
    pos
}
