//! Shared helpers for bpy-built bathroom / kitchen assets: edit-mode style extrude / separate / boundary-loop operations on
//! polygon meshes (geometry only, element order is not Blender's), bpy primitive builders, modifiers with python defaults.
use super::cabnodes::GMesh;
use crate::dhash::{HashMap, HashSet};
use crate::mesh::Mesh;

pub type V3 = [f64; 3];

pub fn from_g(g: &GMesh) -> Mesh {
    let mut m = Mesh::new();
    m.pos = g.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    m.faces = g.p.clone();
    m
}

/// drop unreferenced vertices (keeps order)
pub fn compact(m: &Mesh) -> Mesh {
    let mut used = vec![false; m.pos.len()];
    for f in &m.faces {
        for &v in f {
            used[v as usize] = true;
        }
    }
    let mut map = vec![u32::MAX; m.pos.len()];
    let mut o = Mesh::new();
    for (i, p) in m.pos.iter().enumerate() {
        if used[i] {
            map[i] = o.pos.len() as u32;
            o.pos.push(*p);
        }
    }
    o.faces = m.faces.iter().map(|f| f.iter().map(|&v| map[v as usize]).collect()).collect();
    o
}

pub fn ekey(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// unique edges in first-use order of the face corners
pub fn edge_list(faces: &[Vec<u32>]) -> Vec<(u32, u32)> {
    let mut seen: HashSet<(u32, u32)> = HashSet::default();
    let mut out = vec![];
    for f in faces {
        for c in 0..f.len() {
            let k = ekey(f[c], f[(c + 1) % f.len()]);
            if seen.insert(k) {
                out.push(k);
            }
        }
    }
    out
}

/// edge -> faces
pub fn edge_faces(faces: &[Vec<u32>]) -> HashMap<(u32, u32), Vec<u32>> {
    let mut m: HashMap<(u32, u32), Vec<u32>> = HashMap::default();
    for (fi, f) in faces.iter().enumerate() {
        for c in 0..f.len() {
            m.entry(ekey(f[c], f[(c + 1) % f.len()])).or_default().push(fi as u32);
        }
    }
    m
}

/// new mesh made of the faces with `sel` set (vertices compacted); `bpy.ops.mesh.duplicate_move` + `separate(type=SELECTED)`
pub fn separate_faces(m: &Mesh, sel: &[bool]) -> Mesh {
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    for (i, f) in m.faces.iter().enumerate() {
        if sel[i] {
            o.faces.push(f.clone());
        }
    }
    compact(&o)
}

/// boundary of the face region `sel`: edges (a, b) with a selected face on one side and none on the other (or a mesh boundary)
/// assembled in closed loops (vertex lists). `region_to_loop`.
pub fn region_loops(m: &Mesh, sel: &[bool]) -> Vec<Vec<u32>> {
    let ef = edge_faces(&m.faces);
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut order: Vec<u32> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let fs = &ef[&ekey(a, b)];
            if fs.iter().any(|&g| !sel[g as usize]) || fs.len() == 1 {
                for (u, w) in [(a, b), (b, a)] {
                    let e = adj.entry(u).or_insert_with(|| {
                        order.push(u);
                        vec![]
                    });
                    e.push(w);
                }
            }
        }
    }
    loops_from_adj(&adj, &order)
}

/// walk closed loops over an adjacency map (each vertex 2 neighbours); deterministic: start at `order`
pub fn loops_from_adj(adj: &HashMap<u32, Vec<u32>>, order: &[u32]) -> Vec<Vec<u32>> {
    let mut seen: HashSet<u32> = HashSet::default();
    let mut out = vec![];
    for &s in order {
        if seen.contains(&s) {
            continue;
        }
        let mut lp = vec![s];
        seen.insert(s);
        let mut prev = s;
        let mut cur = adj[&s][0];
        while cur != s && !seen.contains(&cur) {
            lp.push(cur);
            seen.insert(cur);
            let nb = &adj[&cur];
            let nxt = if nb[0] != prev { nb[0] } else { nb[1] };
            prev = cur;
            cur = nxt;
        }
        out.push(lp);
    }
    out
}

/// vertices of mesh boundary edges (face count 1) as closed loops
pub fn boundary_loops(m: &Mesh) -> Vec<Vec<u32>> {
    let ef = edge_faces(&m.faces);
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut order = vec![];
    for (a, b) in edge_list(&m.faces) {
        if ef[&(a, b)].len() == 1 {
            for (u, w) in [(a, b), (b, a)] {
                let e = adj.entry(u).or_insert_with(|| {
                    order.push(u);
                    vec![]
                });
                e.push(w);
            }
        }
    }
    loops_from_adj(&adj, &order)
}

/// Blender's extrude_face_region on a vertex selection (vertex select mode: edges / faces selected when all their vertices are):
/// duplicates the selected vertices (moved by `t`), replaces the fully selected faces by moved copies, and joins region boundary /
/// face-less selected edges to their copies with quads. Original faces of the region are deleted.
pub fn extrude_vsel(m: &mut Mesh, vsel: &[bool], t: V3) {
    let nf = m.faces.len();
    let fsel: Vec<bool> = m.faces.iter().map(|f| f.iter().all(|&v| vsel[v as usize])).collect();
    let ef = edge_faces(&m.faces);
    let mut map: HashMap<u32, u32> = HashMap::default();
    let mut dup = |m: &mut Mesh, v: u32, map: &mut HashMap<u32, u32>| -> u32 {
        *map.entry(v).or_insert_with(|| {
            let p = m.pos[v as usize];
            m.pos.push([p[0] + t[0], p[1] + t[1], p[2] + t[2]]);
            (m.pos.len() - 1) as u32
        })
    };
    let mut sides: Vec<[u32; 4]> = vec![];
    for (a, b) in edge_list(&m.faces) {
        if !(vsel[a as usize] && vsel[b as usize]) {
            continue;
        }
        let fs = &ef[&(a, b)];
        let nin = fs.iter().filter(|&&f| fsel[f as usize]).count();
        if nin >= 2 {
            continue;
        }
        // winding source: an input face if any, else the first adjacent face
        let src = fs.iter().find(|&&f| fsel[f as usize]).copied().unwrap_or(fs[0]);
        let f = &m.faces[src as usize];
        let mut dir = (a, b);
        for c in 0..f.len() {
            if ekey(f[c], f[(c + 1) % f.len()]) == (a, b) {
                dir = (f[c], f[(c + 1) % f.len()]);
            }
        }
        let (a, b) = dir;
        let (na, nb) = (dup(m, a, &mut map), dup(m, b, &mut map));
        let mut q = [a, b, nb, na];
        // extrusion inside the face plane: keep the side face oriented like the source face (Blender's output for flat regions)
        let nf = super::bpyops::face_normal(m, &m.faces[src as usize]);
        let tl = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
        if tl > 0.0 && (t[0] * nf[0] + t[1] * nf[1] + t[2] * nf[2]).abs() < 1e-6 * tl {
            let qm = Mesh { pos: m.pos.clone(), faces: vec![q.to_vec()], uv: vec![] };
            if super::bpyops::face_normal(&qm, &qm.faces[0]).iter().zip(nf.iter()).map(|(x, y)| x * y).sum::<f64>() < 0.0 {
                q = [na, nb, b, a];
            }
        }
        sides.push(q);
    }
    for fi in 0..nf {
        if !fsel[fi] {
            continue;
        }
        for c in 0..m.faces[fi].len() {
            let v = m.faces[fi][c];
            let nv = dup(m, v, &mut map);
            m.faces[fi][c] = nv;
        }
    }
    for s in sides {
        m.faces.push(s.to_vec());
    }
}

pub fn apply_scale_loc(m: &mut Mesh, s: V3, rot_x: f64, loc: V3) {
    // object transform_apply (scale, then rotation about x, then location) in float32
    let (sn, cs) = (rot_x as f32).sin_cos();
    for p in m.pos.iter_mut() {
        let (x, y, z) = ((p[0] as f32) * s[0] as f32, (p[1] as f32) * s[1] as f32, (p[2] as f32) * s[2] as f32);
        let (y2, z2) = (y * cs - z * sn, y * sn + z * cs);
        *p = [(x + loc[0] as f32) as f64, (y2 + loc[1] as f32) as f64, (z2 + loc[2] as f32) as f64];
    }
}

/// BEVEL modifier with python defaults (width 0.1 unless given, angle limit 30 degrees, clamp overlap, profile 0.5)
pub fn bevel_def(m: &Mesh, width: f64, segments: i32) -> Mesh {
    let ang = 30.0f32 * (std::f64::consts::PI / 180.0) as f32;
    super::bevmod::bevel_modifier(m, width, segments, Some(ang), true, 0.5).expect("bevel")
}

/// SUBSURF modifier, SIMPLE type
pub fn simple_subsurf(m: &Mesh, levels: usize) -> Mesh {
    let q = crate::subdiv::QMesh::new(m.pos.clone(), m.faces.clone());
    let r = crate::subdiv::simple_levels(&q, levels);
    let mut o = Mesh::new();
    o.pos = r.pos;
    o.faces = r.faces;
    o
}

/// join in order
pub fn join(list: &[&Mesh]) -> Mesh {
    let mut o = Mesh::new();
    for m in list {
        let b = o.pos.len() as u32;
        o.pos.extend_from_slice(&m.pos);
        o.faces.extend(m.faces.iter().map(|f| f.iter().map(|&v| v + b).collect::<Vec<u32>>()));
    }
    o
}

/// walk the edge loop through `e` over a quad grid: at each valence-4 vertex continue with the edge opposite (not sharing a face)
pub fn edge_loop_quads(m: &Mesh, e: (u32, u32)) -> Vec<u32> {
    let ef = edge_faces(&m.faces);
    let mut vadj: HashMap<u32, Vec<(u32, u32)>> = HashMap::default();
    for (a, b) in edge_list(&m.faces) {
        vadj.entry(a).or_default().push((a, b));
        vadj.entry(b).or_default().push((a, b));
    }
    let share = |x: (u32, u32), y: (u32, u32)| -> bool {
        let fx = &ef[&x];
        let fy = &ef[&y];
        fx.iter().any(|f| fy.contains(f))
    };
    let mut lp = vec![e.0, e.1];
    let mut cur = e;
    let mut v = e.1;
    for _ in 0..m.pos.len() {
        let edges = &vadj[&v];
        if edges.len() != 4 {
            break;
        }
        let nxt = edges.iter().copied().find(|&x| x != cur && !share(x, cur));
        let Some(nx) = nxt else { break };
        let w = if nx.0 == v { nx.1 } else { nx.0 };
        if w == lp[0] {
            return lp;
        }
        lp.push(w);
        cur = nx;
        v = w;
    }
    lp
}
