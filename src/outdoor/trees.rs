//! Rust trees (crates/ig_plants/src/rtree.rs) as scene geometry for the outdoor stage's tree spots (its own module: an edit here
//! recompiles only this file's code).
use crate::mesh::{Mat4, Mesh, Scene};
use serde_json::Value;

/// Build the tree spots in `props` as geometry: oak / maple near the house one by one among their neighbours, the rest instances of
/// shared builds. Empties `props` (the sidecar's spots are for trees = "infinigen"); returns the log line.
pub fn build(scene: &mut Scene, props: &mut Vec<Value>, seed: u64, centre: [f64; 2], e: f64) -> String {
    use crate::assets::rtree::{self, Lod};
    let species = rtree::ALL;
    let mut built: std::collections::HashMap<(u8, u64), (Mesh, Mesh, Vec<[f64; 3]>)> = Default::default();
    let mut mats: std::collections::HashMap<u8, (usize, usize)> = Default::default();
    let (mut bark_all, mut leaf_all): (std::collections::HashMap<u8, Mesh>, std::collections::HashMap<u8, Mesh>) = (Default::default(), Default::default());
    let mut cent_all: std::collections::HashMap<u8, Vec<[f64; 3]>> = Default::default();
    let mut inst: std::collections::HashMap<(u8, u64), Vec<[[f64; 4]; 4]>> = Default::default();
    let tree_at: Vec<([f64; 2], f64)> = props.iter().filter(|p| p["kind"] == "tree")
        .map(|p| ([p["loc"][0].as_f64().unwrap(), p["loc"][1].as_f64().unwrap()], p["crown"].as_f64().unwrap())).collect();
    // the near trees' crowded builds, then the props in order (shared builds for the rest)
    let near_of = |pr: &Value| {
        let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap()];
        // (crowded builds for the space-colonization broadleaves -- oak, maple -- within 45 m: Weber-Penn crowns change little
        // when crowded, and past 45 m the asymmetry is not visible from the windows; the rest are instances)
        let si = pr["species"].as_u64().unwrap_or(5) as usize;
        pr["kind"] == "tree" && si <= 1 && (loc[0] - centre[0]).abs().max((loc[1] - centre[1]).abs()) <= e.min(45.0)
    };
    let crowded: Vec<Option<(Mesh, Mesh, Vec<[f64; 3]>, bool)>> = {
        props.iter().enumerate().map(|(pi, pr)| {
            if !near_of(pr) { return None }
            let variant = pr["variant"].as_u64().unwrap_or(1000);
            let si = pr["species"].as_u64().unwrap_or(5) as usize;
            let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap()];
            let (rot, sc) = (pr["rot"].as_f64().unwrap_or(0.0), pr["scale"].as_f64().unwrap_or(1.0));
            let bseed = seed ^ (variant * 7919) ^ (si as u64) << 20;
            // neighbours in the tree's local frame (unrotated, unscaled)
            let cr = pr["crown"].as_f64().unwrap_or(1.0);
            let (cs, sn) = ((-rot).cos(), (-rot).sin());
            let nb: Vec<([f64; 2], f64)> = tree_at.iter().filter(|(p, rn)| {
                let dd = (p[0] - loc[0]).hypot(p[1] - loc[1]);
                dd > 0.05 && dd < cr + rn
            }).map(|(p, rn)| {
                let (dx, dy) = ((p[0] - loc[0]) / sc, (p[1] - loc[1]) / sc);
                ([dx * cs - dy * sn, dx * sn + dy * cs], rn / sc)
            }).collect();
            let t = rtree::build_crowded(species[si], bseed ^ (pi as u64) << 32, Lod::Near, cr / sc, &nb);
            Some((t.bark, t.leaves, t.leaf_centres, !nb.is_empty()))
        }).collect()
    };
    let n_crowded = crowded.iter().filter(|c| c.as_ref().map_or(false, |c| c.3)).count();
    for (pi, pr) in props.iter().enumerate() {
        let variant = pr["variant"].as_u64().unwrap_or(1000);
        let si = pr["species"].as_u64().unwrap_or(5) as usize;
        let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap(), pr["loc"][2].as_f64().unwrap()];
        let (rot, sc) = (pr["rot"].as_f64().unwrap_or(0.0), pr["scale"].as_f64().unwrap_or(1.0));
        let d = (loc[0] - centre[0]).abs().max((loc[1] - centre[1]).abs());
        let lod = if d > e { Lod::Far } else { Lod::Near };
        let bseed = seed ^ (variant * 7919) ^ (si as u64) << 20;
        let m = Mat4::translation(loc).mul(&Mat4::rot_z(rot)).mul(&Mat4::scale([sc; 3]));
        mats.entry(si as u8).or_insert_with(|| { let (bm, lm) = rtree::materials(species[si], 0.0); (scene.material(bm), scene.material(lm)) });
        if let Some(c) = &crowded[pi] {
            // a one-off build: merged into its species' mesh in world space
            let (mut bb, mut ll) = (c.0.clone(), c.1.clone());
            bb.transform(&m); ll.transform(&m);
            bark_all.entry(si as u8).or_default().append(&bb);
            let le = leaf_all.entry(si as u8).or_default();
            if le.uv.len() != le.pos.len() { le.uv.resize(le.pos.len(), [0.0, 0.0]); }
            le.append(&ll);
            cent_all.entry(si as u8).or_default().extend(c.2.iter().map(|p| m.apply(*p)));
        } else {
            // a shared build: one more instance of it
            let key = (si as u8, variant * 2 + (lod == Lod::Far) as u64);
            built.entry(key).or_insert_with(|| { let t = rtree::build(species[si], bseed, lod); (t.bark, t.leaves, t.leaf_centres) });
            inst.entry(key).or_default().push(m.0);
        }
    }
    let mut keys: Vec<u8> = bark_all.keys().copied().collect(); keys.sort();
    for k in keys {
        let (bi, li) = mats[&k];
        scene.add(&format!("outdoor_rtree_{:?}#0", species[k as usize]), "OutdoorTree", bark_all.remove(&k).unwrap(), bi);
        scene.add_bent(&format!("outdoor_rtree_{:?}#1", species[k as usize]), "OutdoorTree", leaf_all.remove(&k).unwrap(), li, cent_all.remove(&k).unwrap_or_default());
    }
    // shared builds as instanced nodes (one mesh, a transform per tree), in key order
    let mut bkeys: Vec<(u8, u64)> = built.keys().copied().collect(); bkeys.sort();
    let n_shared = bkeys.len();
    let n_inst: usize = inst.values().map(|v| v.len()).sum();
    for k in bkeys {
        let (b, l, lc) = built.remove(&k).unwrap();
        let (bi, li) = mats[&k.0];
        let xs = inst.remove(&k).unwrap_or_default();
        let nm = format!("outdoor_rtree_{:?}_v{}", species[k.0 as usize], k.1);
        scene.add_instanced(&format!("{nm}#0"), "OutdoorTree", b, bi, None, xs.clone());
        scene.add_instanced(&format!("{nm}#1"), "OutdoorTree", l, li, Some(lc), xs);
    }
    let msg = format!("rust trees: {n_crowded} near trees built among neighbours, {n_shared} shared builds placed {n_inst} times");
    props.clear();
    msg
}
