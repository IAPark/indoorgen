//! Per-object triangle budget (Q6): quadric-error edge-collapse simplification (meshoptimizer) of every scene node whose
//! triangle count is far above what its size can show. Applied right before the glb export. Deterministic (pure function of the node).
//! Exempt: people, room shells, nodes that cloth jobs target or collide with. Env: MESH_BUDGET=0 disables, MESH_BUDGET_SCALE=f scales budgets.
use ig_core::mesh::{triangulate_face, Mesh, Scene};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// triangle budget for an object of bounding-box diagonal `d` metres
pub fn budget_for(d: f64, scale: f64) -> usize {
    let b = if d < 0.1 { 1000.0 } else if d < 0.2 { 2500.0 } else if d < 0.4 { 6000.0 } else if d < 0.8 { 15000.0 } else if d < 1.5 { 40000.0 } else { 100000.0 };
    (b * scale) as usize
}

fn cloth_names(scene: &Scene) -> HashSet<String> {
    let mut s = HashSet::new();
    fn walk(v: &serde_json::Value, s: &mut HashSet<String>) {
        match v {
            serde_json::Value::String(x) => { s.insert(x.clone()); }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, s)),
            serde_json::Value::Object(o) => o.iter().filter(|(k, _)| matches!(k.as_str(), "target" | "collide" | "part" | "bases")).for_each(|(_, x)| walk(x, s)),
            _ => {}
        }
    }
    scene.cloth.iter().for_each(|j| walk(j, &mut s));
    s
}

/// Simplify `m` to about `target` triangles. Returns None when it is already within budget or simplification fails.
pub fn simplify_mesh(m: &Mesh, target: usize) -> Option<Mesh> {
    let ntri: usize = m.faces.iter().map(|f| f.len().saturating_sub(2)).sum();
    if ntri <= target {
        return None;
    }
    let (lo, hi) = m.bounds();
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let has_uv = m.uv.len() == m.pos.len();
    // weld by (position 0.1 um, uv): uv seams stay separate vertices, so they stay borders for the simplifier
    let mut ids: HashMap<[i64; 5], u32> = HashMap::default();
    let mut remap = Vec::with_capacity(m.pos.len());
    let mut vpos: Vec<[f32; 3]> = vec![];
    let mut vorig: Vec<u32> = vec![];
    for (i, p) in m.pos.iter().enumerate() {
        let (u, v) = if has_uv { (m.uv[i][0], m.uv[i][1]) } else { (0.0, 0.0) };
        let k = [(p[0] * 1e7).round() as i64, (p[1] * 1e7).round() as i64, (p[2] * 1e7).round() as i64, (u * 1e5).round() as i64, (v * 1e5).round() as i64];
        let n = vpos.len() as u32;
        let id = *ids.entry(k).or_insert_with(|| {
            vpos.push([(p[0] - c[0]) as f32, (p[1] - c[1]) as f32, (p[2] - c[2]) as f32]);
            vorig.push(i as u32);
            n
        });
        remap.push(id);
    }
    let mut idx: Vec<u32> = Vec::with_capacity(ntri * 3);
    for f in &m.faces {
        for t in triangulate_face(&m.pos, f) {
            let (a, b, cc) = (remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]);
            if a != b && b != cc && a != cc {
                idx.extend_from_slice(&[a, b, cc]);
            }
        }
    }
    if idx.len() / 3 <= target {
        return None;
    }
    let bytes: Vec<u8> = vpos.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect();
    let ad = meshopt::VertexDataAdapter::new(&bytes, 12, 0).ok()?;
    let out = meshopt::simplify(&idx, &ad, target * 3, 1.0, meshopt::SimplifyOptions::Prune | meshopt::SimplifyOptions::ErrorAbsolute, None);
    if out.is_empty() {
        return None;
    }
    // compact
    let mut nid: HashMap<u32, u32> = HashMap::default();
    let mut r = Mesh::default();
    let mut faces = Vec::with_capacity(out.len() / 3);
    for t in out.chunks(3) {
        let mut f = Vec::with_capacity(3);
        for &v in t {
            let n = nid.len() as u32;
            let id = *nid.entry(v).or_insert_with(|| {
                let o = vorig[v as usize] as usize;
                r.pos.push(m.pos[o]);
                if has_uv { r.uv.push(m.uv[o]); }
                n
            });
            f.push(id);
        }
        faces.push(f);
    }
    r.faces = faces;
    Some(r)
}

/// Apply the budget to all nodes; returns (triangles before, after, nodes simplified).
pub fn apply(scene: &mut Scene, geo: &crate::config::Geometry) -> (usize, usize, usize) {
    let scale: f64 = geo.budget_scale;
    if !geo.mesh_budget {
        return (0, 0, 0);
    }
    let keep = cloth_names(scene);
    let res: Vec<(usize, usize, Option<Mesh>)> = scene.nodes.par_iter().map(|n| {
        let ntri: usize = n.mesh.faces.iter().map(|f| f.len().saturating_sub(2)).sum();
        if n.class == "Person" || n.class == "room" || keep.contains(&n.name) {
            return (ntri, ntri, None);
        }
        let (lo, hi) = n.mesh.bounds();
        let d = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
        let b = budget_for(d, scale);
        match simplify_mesh(&n.mesh, b) {
            Some(m) => { let t = m.faces.len(); (ntri, t, Some(m)) }
            None => (ntri, ntri, None),
        }
    }).collect();
    let (mut a, mut b, mut k) = (0, 0, 0);
    for (n, (t0, t1, m)) in scene.nodes.iter_mut().zip(res) {
        a += t0; b += t1;
        if let Some(m) = m { n.mesh = m; k += 1; }
    }
    if ig_core::envx::var_os("BUDGETSTAT").is_some() {
        let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for n in &scene.nodes { let e = by.entry(n.class.clone()).or_default(); e.0 += 1; e.1 += n.mesh.faces.iter().map(|f| f.len().saturating_sub(2)).sum::<usize>(); }
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse((x.1).1));
        for (c, (n, t)) in v.iter().take(18) { eprintln!("BUDGETSTAT {c} nodes {n} tris {t}"); }
    }
    (a, b, k)
}

/// Budget for the parts of one built asset (applied right after the build: later stages and memory get the small meshes).
/// Assets with cloth jobs / followers are left alone (Blender re-simulates them against these parts).
pub fn apply_asset(a: &mut ig_kit::api::Asset, geo: &crate::config::Geometry) {
    if !geo.mesh_budget {
        return;
    }
    let mut keep: HashSet<usize> = HashSet::new();
    for j in &a.cloth {
        keep.insert(j.target);
        keep.extend(j.collide.iter().copied());
        if let Some((p, _)) = j.z_from { keep.insert(p); }
    }
    for f in &a.followers {
        keep.insert(f.part);
        keep.extend(f.bases.iter().copied());
    }
    let scale: f64 = geo.budget_scale;
    for (pi, p) in a.parts.iter_mut().enumerate() {
        if keep.contains(&pi) { continue; }
        let (lo, hi) = p.mesh.bounds();
        let d = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
        if let Some(m) = simplify_mesh(&p.mesh, budget_for(d, scale)) {
            p.mesh = m;
        }
    }
}
