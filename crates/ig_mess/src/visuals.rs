//! States that edit the built visual assets (called through `states::post_visuals`).
use crate::states::V3;
use crate::MessCfg;
use ig_furn::state::State;
use ig_kit::api::Asset;
use std::collections::HashMap;

/// connected components (by shared vertex index) of a mesh: returns component id per vertex
pub fn components(m: &ig_core::mesh::Mesh) -> (Vec<usize>, usize) {
    let n = m.pos.len();
    let mut p: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for f in &m.faces {
        for w in f.windows(2) {
            let (a, b) = (find(&mut p, w[0] as usize), find(&mut p, w[1] as usize));
            if a != b {
                p[a] = b;
            }
        }
    }
    let mut ids: HashMap<usize, usize> = HashMap::new();
    let mut out = vec![0; n];
    for v in 0..n {
        let r = find(&mut p, v);
        let l = ids.len();
        out[v] = *ids.entry(r).or_insert(l);
    }
    (out, ids.len())
}

pub fn dump(name: &str, class: &str, a: &Asset) {
    eprintln!("ASSET {name} {class} parts={}", a.parts.len());
    for (pi, p) in a.parts.iter().enumerate() {
        let (c, n) = components(&p.mesh);
        let mut bb = vec![([f64::MAX; 3], [f64::MIN; 3], 0usize); n];
        for (v, q) in p.mesh.pos.iter().enumerate() {
            let b = &mut bb[c[v]];
            b.2 += 1;
            for k in 0..3 {
                b.0[k] = b.0[k].min(q[k]);
                b.1[k] = b.1[k].max(q[k]);
            }
        }
        eprintln!("  part {pi} verts={} comps={}", p.mesh.pos.len(), n);
        for (i, b) in bb.iter().enumerate().take(40) {
            eprintln!("    c{i} n={} lo={:.3?} hi={:.3?}", b.2, b.0, b.1);
        }
    }
}

pub fn post_visuals(visuals: &mut HashMap<String, Asset>, state: &State, cfg: &MessCfg) {
    let _: Option<V3> = None;
    if std::env::var("MESS_DUMP_ASSETS").is_ok() {
        for (k, a) in visuals.iter() {
            let cls = state.objs[k].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
            if cls.contains("Cabinet") || cls.contains("KitchenSpace") {
                dump(k, &cls, a);
            }
        }
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    let mut rep = crate::states::last_report().unwrap_or_default();
    let mut keys: Vec<String> = visuals.keys().cloned().collect();
    keys.sort();
    let mut rng = cfg.rng("states.beds");
    for k in &keys {
        let cls = state.objs[k].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
        if cls == "BedFactory" {
            let changed = unmake_bed(visuals.get_mut(k).unwrap(), lvl, &mut rng);
            *rep.candidates.entry("bed".into()).or_default() += 1;
            if changed {
                *rep.changed.entry("bed".into()).or_default() += 1;
            }
        }
    }
    rep.print();
}

/// Unmade bed: the sheet / comforter is pulled toward the foot (and slightly askew), the cover thrown crooked. Edits the cloth jobs (Blender
/// re-simulates them with the new placement) and the approximate part meshes the same way.
fn unmake_bed(a: &mut Asset, lvl: f64, rng: &mut ig_core::rng::Rng) -> bool {
    use ig_core::mesh::Mat4;
    let jobs: Vec<usize> = (0..a.cloth.len()).filter(|&i| a.cloth[i].build.is_some()).collect();
    if jobs.is_empty() {
        return false;
    }
    let mut any = false;
    for (n, ji) in jobs.iter().enumerate() {
        // sheet (n = 0): pulled back 10-45 cm toward the foot; cover (n = 1): moved along and across by a smaller amount
        let (u1, u2, u3, u4) = (rng.f64(), rng.f64(), rng.f64(), rng.f64());
        let (dx, dy, yaw) = if n == 0 {
            (lvl * (0.1 + 0.35 * u1), lvl * (u2 * 2.0 - 1.0) * 0.08, lvl * (u3 * 2.0 - 1.0) * 6f64.to_radians())
        } else {
            (lvl * (u1 * 2.0 - 0.6) * 0.3, lvl * (u2 * 2.0 - 1.0) * 0.15, lvl * (u3 * 2.0 - 1.0) * 14f64.to_radians())
        };
        let _ = u4;
        let target = a.cloth[*ji].target;
        let t = Mat4::translation([dx, dy, 0.0]);
        let job = &mut a.cloth[*ji];
        job.pre.transform(&t);
        if let Some(b) = job.build.as_mut() {
            let x = b["x"].as_f64().unwrap_or(0.0);
            b["x"] = serde_json::json!(x + dx);
            b["dy"] = serde_json::json!(dy);
            b["yaw"] = serde_json::json!(yaw);
        }
        if let Some(p) = a.parts.get_mut(target) {
            p.mesh.transform(&t);
        }
        any = true;
    }
    any
}
