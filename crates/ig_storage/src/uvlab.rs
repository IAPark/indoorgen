//! UV topology bookkeeping for Blender-exact subdivision (BACKLOG K2).
//!
//! Blender refines every mesh that carries a UV map with a face-varying channel, which changes the last float32 bits of the OpenSubdiv limit
//! points (the Rust meshes carry no UVs). A mesh's UV *topology* is a label per face corner: two corners of one vertex share a label iff they have
//! the same UV value (`Osd::new_uv`). This module builds the labels of bpy primitives and propagates them through subdivision, following the layout
//! of `gweld::subdivide_ev` (coarse vertices, then `2^L - 1` vertices per edge, then the interior vertices per face; the children of a coarse
//! face are contiguous).
use crate::dhash::HashMap;
use crate::mesh::Mesh;

pub type Labels = Vec<Vec<i32>>;

/// bpy `primitive_cylinder_add(vertices=n)` (cap fill NGON) polygons in Blender's order, vertex 2i = bottom / 2i+1 = top ring point i, with the corner
/// labels of its UV map (side strip with the seam between the last and the first side face, one island per cap).
pub fn cylinder_bpy(n: usize) -> (Vec<Vec<u32>>, Labels) {
    let m = (2 * n) as u32;
    let mut faces: Vec<Vec<u32>> = vec![];
    // key = (island, column) of the UV vertex; side strip columns 0..=n
    let mut keys: Vec<Vec<(u32, u32)>> = vec![];
    let top_after = (n + 1) / 2;
    let mut push_side = |i: usize, faces: &mut Vec<Vec<u32>>, keys: &mut Vec<Vec<(u32, u32)>>| {
        let a = (2 * i) as u32;
        faces.push(vec![a, a + 1, (a + 3) % m, (a + 2) % m]);
        keys.push(vec![(0, i as u32), (0, i as u32), (0, i as u32 + 1), (0, i as u32 + 1)]);
    };
    for i in 0..n {
        if i == top_after {
            let mut top = vec![3u32, 1];
            let mut k = 2 * n as i64 - 1;
            while k >= 5 {
                top.push(k as u32);
                k -= 2;
            }
            keys.push(top.iter().map(|&v| (1, v)).collect());
            faces.push(top);
        }
        push_side(i, &mut faces, &mut keys);
    }
    let bot: Vec<u32> = (0..n).map(|i| 2 * i as u32).collect();
    keys.push(bot.iter().map(|&v| (2, v)).collect());
    faces.push(bot);
    let lab = label_from_keys(&faces, &keys);
    (faces, lab)
}

/// dense labels from per-corner UV-vertex keys (same vertex + same key = same label)
pub fn label_from_keys<K: std::hash::Hash + Eq + Clone>(faces: &[Vec<u32>], keys: &[Vec<K>]) -> Labels {
    let mut map: HashMap<(u32, K), i32> = HashMap::default();
    let mut out = vec![];
    for (f, k) in faces.iter().zip(keys) {
        let mut row = vec![];
        for (c, &v) in f.iter().enumerate() {
            let n = map.len() as i32;
            row.push(*map.entry((v, k[c].clone())).or_insert(n));
        }
        out.push(row);
    }
    out
}

pub fn flat(l: &Labels) -> Vec<i32> {
    l.iter().flat_map(|r| r.iter().cloned()).collect()
}

/// Labels of the subdivided mesh (`levels` levels at once, layout of `gweld::subdivide_ev`) from the labels of the coarse mesh `m`.
/// `out_faces` are the faces of the subdivided mesh.
pub fn subdivide_labels(m: &Mesh, labels: &Labels, levels: u32, out_faces: &[Vec<u32>], edges: Option<&[[u32; 2]]>) -> Labels {
    let nv = m.pos.len();
    let edges: Vec<(u32, u32)> = match edges {
        Some(e) => e.iter().map(|x| (x[0], x[1])).collect(),
        None => crate::bathkit::edge_list(&m.faces),
    };
    let emap: HashMap<(u32, u32), usize> = edges.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let res = (1usize << levels) + 1;
    let res2 = res - 2;
    let nq_res = (res >> 1) + 1;
    let edge_off = nv;
    let inner_off = nv + edges.len() * res2;
    // children count per coarse face, per-face inner vertex offsets
    let mut first_child = vec![];
    let mut vtx_off = vec![];
    let (mut accf, mut accv) = (0usize, 0usize);
    for f in &m.faces {
        first_child.push(accf);
        vtx_off.push(accv);
        if f.len() == 4 {
            accf += (res - 1) * (res - 1);
            accv += res2 * res2;
        } else {
            accf += f.len() * (res / 2) * (res / 2);
            accv += 1 + f.len() * ((nq_res - 2) * (nq_res - 1));
        }
    }
    assert_eq!(accf, out_faces.len(), "subdivide_labels: face layout mismatch");
    // keys: (vertex, class) -> label; class: 0 = coarse vertex (label of the coarse corner), 1 = on an edge (labels of both end corners), 2 = face interior
    let mut map: HashMap<(u32, i64, i64, i64), i32> = HashMap::default();
    let mut out: Labels = Vec::with_capacity(out_faces.len());
    let mut parent = 0usize;
    for (gi, g) in out_faces.iter().enumerate() {
        while parent + 1 < m.faces.len() && gi >= first_child[parent + 1] {
            parent += 1;
        }
        let f = &m.faces[parent];
        let corner_of = |v: u32| f.iter().position(|&x| x == v).unwrap();
        let mut row = vec![];
        for &w in g {
            let wi = w as usize;
            let key = if wi < nv {
                (w, 0, labels[parent][corner_of(w)] as i64, 0)
            } else if wi < inner_off {
                let ei = (wi - edge_off) / res2;
                let (a, b) = edges[ei];
                debug_assert_eq!(emap[&(a, b)], ei);
                (w, 1, labels[parent][corner_of(a)] as i64, labels[parent][corner_of(b)] as i64)
            } else {
                (w, 2, parent as i64, 0)
            };
            let n = map.len() as i32;
            row.push(*map.entry(key).or_insert(n));
        }
        out.push(row);
    }
    out
}

/// labels of the faces kept by a face filter that keeps the face order
pub fn keep_faces(labels: &Labels, keep: &[bool]) -> Labels {
    labels.iter().zip(keep).filter(|(_, &k)| k).map(|(l, _)| l.clone()).collect()
}

/// labels of `bathfree::solidify` (SOLIDIFY, simple mode): original faces, reversed copies (same UV values at the copied vertices), then the rim quads
/// `[b, a, a', b']` of the boundary edges `a -> b` (sorted like bathfree::solidify), whose corners carry the UV of the adjacent original face.
pub fn solidify_labels(m: &Mesh, labels: &Labels) -> Labels {
    let mut out: Labels = labels.clone();
    for r in labels {
        out.push(r.iter().rev().cloned().collect());
    }
    let mut cnt: HashMap<(u32, u32), (u32, u32, usize, usize, usize)> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let e = cnt.entry((a.min(b), a.max(b))).or_insert((a, b, 0, fi, i));
            e.2 += 1;
        }
    }
    let mut be: Vec<(u32, u32, usize, usize)> = cnt.values().filter(|e| e.2 == 1).map(|e| (e.0, e.1, e.3, e.4)).collect();
    be.sort();
    for (_, _, fi, i) in be {
        let k = m.faces[fi].len();
        let (la, lb) = (labels[fi][i], labels[fi][(i + 1) % k]);
        out.push(vec![lb, la, la, lb]);
    }
    out
}

/// OpenSubdiv subdivision of a UV-carrying mesh as Blender does it (Catmull-Clark or SIMPLE, limit surface, quality 3) + the labels of the result
pub fn subsurf_uv(m: &Mesh, labels: &Labels, levels: usize, simple: bool, edges: Option<&[[u32; 2]]>) -> Option<(Mesh, Labels)> {
    if levels == 0 {
        return Some((m.clone(), labels.clone()));
    }
    let out = crate::subsurf::subsurf_osd_uv(m, levels, simple, true, 3, edges, Some(&flat(labels)))?;
    let l = subdivide_labels(m, labels, levels as u32, &out.faces, edges);
    Some((out, l))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// partition of corners per vertex as a canonical form
    fn canon(faces: &[Vec<u32>], lab: &Labels) -> Vec<Vec<usize>> {
        let mut seen: HashMap<i32, usize> = HashMap::default();
        lab.iter().map(|r| r.iter().map(|&l| { let n = seen.len(); *seen.entry(l).or_insert(n) }).collect()).collect()
    }

    fn real_labels(case: &Value) -> (Mesh, Labels) {
        let mut m = Mesh::new();
        m.pos = case["verts"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect();
        m.faces = case["polys"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let uv = case["uv"].as_array().unwrap();
        let mut keys = vec![];
        let mut li = 0;
        for f in &m.faces {
            let mut row = vec![];
            for _ in f {
                row.push((uv[li][0].as_f64().unwrap().to_bits(), uv[li][1].as_f64().unwrap().to_bits()));
                li += 1;
            }
            keys.push(row);
        }
        let l = label_from_keys(&m.faces, &keys);
        (m, l)
    }

    /// needs tmp/sdu (python stage dump with UVs of the bathroom sink seed 0); run with `cargo test -p ig_storage uvlab -- --ignored`
    #[test]
    #[ignore]
    fn cylinder_and_propagation_match_blender() {
        let dir = std::env::var("SDU").unwrap_or("/Users/lume/work/wt/bath/tmp/sdu".into());
        let rd = |n: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{n}")).unwrap()).unwrap() };
        let c2 = rd("002_object_modifier_apply_SUBSURF.case.json");
        let c3 = rd("003_object_modifier_apply_SUBSURF.case.json");
        let (m2, l2) = real_labels(&c2);
        let (m3, l3) = real_labels(&c3);
        // 1. the bpy cylinder model
        let (cf, cl) = cylinder_bpy(4);
        assert_eq!(cf, m2.faces);
        assert_eq!(canon(&cf, &cl), canon(&m2.faces, &l2));
        // 2. propagation through the SIMPLE subdivision (level 2): labels of the result partition like the real UV of the next stage
        let (out, ol) = subsurf_uv(&m2, &l2, 2, true, Some(&c2["edges"].as_array().unwrap().iter().map(|e| [e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32]).collect::<Vec<_>>())).unwrap();
        assert_eq!(out.faces, m3.faces);
        assert_eq!(canon(&out.faces, &ol), canon(&m3.faces, &l3));
    }
}
