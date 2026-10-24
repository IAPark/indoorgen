//! Blender SUBSURF modifier (Catmull-Clark with use_limit_surface = True, or SIMPLE): the refined topology of subdiv::catmull_clark
//! with every vertex moved to its limit position (what OpenSubdiv's patch evaluation returns at the grid points).
//! Output vertex/face ORDER is not Blender's (equal only up to relabelling).
use crate::mesh::Mesh;
use crate::subdiv::{catmull_clark, QMesh};
use crate::dhash::HashMap;

pub fn subsurf(m: &Mesh, levels: usize, simple: bool) -> Mesh {
    if levels == 0 {
        return m.clone();
    }
    if simple {
        return simple_subdiv(m, levels);
    }
    let q = QMesh { pos: m.pos.clone(), faces: m.faces.clone() };
    let r = catmull_clark(&q, levels, None);
    let pos = limit_positions(&r);
    Mesh { pos, faces: r.faces, uv: vec![] }
}

fn limit_positions(m: &QMesh) -> Vec<[f64; 3]> {
    let n = m.pos.len();
    // adjacency: per vertex the list of (face index, corner)
    let mut vf: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    let mut ecount: HashMap<(u32, u32), u32> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        for (c, &v) in f.iter().enumerate() {
            vf[v as usize].push((fi, c));
            let (a, b) = (v, f[(c + 1) % f.len()]);
            *ecount.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut out = m.pos.clone();
    for v in 0..n {
        let faces = &vf[v];
        if faces.is_empty() {
            continue;
        }
        let p = m.pos[v];
        // neighbours across edges and diagonals
        let mut edge_nb: Vec<u32> = vec![];
        let mut diag: Vec<u32> = vec![];
        let mut all_quads = true;
        for &(fi, c) in faces {
            let f = &m.faces[fi];
            if f.len() != 4 {
                all_quads = false;
                break;
            }
            let (nx, pv, dg) = (f[(c + 1) % 4], f[(c + 3) % 4], f[(c + 2) % 4]);
            for u in [nx, pv] {
                if !edge_nb.contains(&u) {
                    edge_nb.push(u);
                }
            }
            diag.push(dg);
        }
        if !all_quads {
            continue;
        }
        let boundary: Vec<u32> = edge_nb.iter().cloned().filter(|&u| ecount[&(u.min(v as u32), u.max(v as u32))] == 1).collect();
        if !boundary.is_empty() {
            if boundary.len() == 2 {
                let (a, b) = (m.pos[boundary[0] as usize], m.pos[boundary[1] as usize]);
                out[v] = [0, 1, 2].map(|k| (4.0 * p[k] + a[k] + b[k]) / 6.0);
            }
            continue;
        }
        let nn = edge_nb.len();
        if nn != faces.len() || nn < 3 {
            continue;
        }
        let nf = nn as f64;
        let mut s = [0.0; 3];
        for k in 0..3 {
            let e: f64 = edge_nb.iter().map(|&u| m.pos[u as usize][k]).sum();
            let d: f64 = diag.iter().map(|&u| m.pos[u as usize][k]).sum();
            s[k] = (nf * nf * p[k] + 4.0 * e + d) / (nf * (nf + 5.0));
        }
        out[v] = s;
    }
    out
}

fn simple_subdiv(m: &Mesh, levels: usize) -> Mesh {
    let mut cur = m.clone();
    for _ in 0..levels {
        let mut out = Mesh::new();
        out.pos = cur.pos.clone();
        let mut emid: HashMap<(u32, u32), u32> = HashMap::default();
        let mut mid = |out: &mut Mesh, a: u32, b: u32| -> u32 {
            let k = (a.min(b), a.max(b));
            *emid.entry(k).or_insert_with(|| {
                let (p, q) = (out.pos[k.0 as usize], out.pos[k.1 as usize]);
                out.pos.push([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]);
                (out.pos.len() - 1) as u32
            })
        };
        for f in &cur.faces {
            let k = f.len();
            let mids: Vec<u32> = (0..k).map(|i| mid(&mut out, f[i], f[(i + 1) % k])).collect();
            let mut c = [0.0; 3];
            for &v in f {
                for j in 0..3 {
                    c[j] += cur.pos[v as usize][j] / k as f64;
                }
            }
            out.pos.push(c);
            let ci = (out.pos.len() - 1) as u32;
            for i in 0..k {
                out.faces.push(vec![f[i], mids[i], ci, mids[(i + k - 1) % k]]);
            }
        }
        cur = out;
    }
    cur
}

/// Same surface as `subsurf`, but with Blender's output vertex / face order and corner order (the layout of BKE_subdiv_to_mesh,
/// replicated by gweld::subdivide): the topology comes from gweld, the (limit) positions from `subsurf`, matched through the
/// simple-subdivision positions both layouts share.
pub fn subsurf_bl(m: &Mesh, levels: usize, simple: bool) -> Mesh {
    use super::cabnodes::GMesh;
    if levels == 0 {
        return m.clone();
    }
    let mut g = GMesh::new();
    g.v = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    g.p = m.faces.clone();
    g.tag = vec![0; g.p.len()];
    g.e = super::bathkit::edge_list(&m.faces).into_iter().map(|(a, b)| [a, b]).collect();
    let gb = super::gweld::subdivide(&g, levels as u32);
    let mut out = Mesh::new();
    out.faces = gb.p.clone();
    if simple {
        out.pos = gb.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        return out;
    }
    let q = QMesh { pos: m.pos.clone(), faces: m.faces.clone() };
    let cc = catmull_clark(&q, levels, None);
    let limit = limit_positions(&cc);
    let sp = crate::subdiv::simple_levels(&q, levels).pos;
    assert_eq!(sp.len(), gb.v.len(), "subsurf_bl: layouts differ in size");
    let cell = 1e-4f64;
    let key = |p: [f64; 3]| [(p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::default();
    for (i, p) in sp.iter().enumerate() {
        grid.entry(key(*p)).or_default().push(i as u32);
    }
    let mut pos = Vec::with_capacity(gb.v.len());
    for p in &gb.v {
        let p = [p[0] as f64, p[1] as f64, p[2] as f64];
        let k = key(p);
        let mut best = (f64::INFINITY, u32::MAX);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(v) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &i in v {
                            let s = sp[i as usize];
                            let d = (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2) + (s[2] - p[2]).powi(2);
                            if d < best.0 {
                                best = (d, i);
                            }
                        }
                    }
                }
            }
        }
        assert!(best.1 != u32::MAX && best.0 < 1e-8, "subsurf_bl: vertex match failed");
        pos.push(limit[best.1 as usize]);
    }
    out.pos = pos;
    out
}
