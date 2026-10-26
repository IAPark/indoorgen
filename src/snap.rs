//! Post-solve visual correction of supported objects: the solver places a child on its parent's PLACEHOLDER (python's bounding boxes,
//! kept bit-exact for the lockstep), but the visible parent surface is often a few cm away (KitchenSpace: countertop vs the
//! `bottom_h + 0.13` support board; cabinet shelf boards vs the bbox, ...). After the visuals are built, every child that is
//! related to another object is dropped (or lifted) vertically so its lowest contact points rest on the visible surface below.
use crate::furn::geom::Pose;
use crate::furn::state::State;
use crate::mesh::{Mat4, Node};
use crate::qc::down_hit;
use std::collections::HashMap;

pub struct Snapped {
    pub key: String,
    pub dz: f64,
    pub removed: bool,
}

/// `max_drop`: largest correction accepted downwards (m); `max_lift`: upwards. Returns the applied corrections.
pub fn snap_children(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, skip: &[String], floor_z: f64, max_drop: f64, max_lift: f64) -> Vec<Snapped> {
    let mat_of = |p: &Pose| Mat4([[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]);
    let keys: Vec<String> = state.objs.iter().filter(|(k, o)| o.generator.is_some() && visuals.contains_key(*k)).map(|(k, _)| k.clone()).collect();
    // object parents (relation targets that are generated objects)
    let parent: HashMap<String, String> = keys
        .iter()
        .filter_map(|k| state.objs[k].relations.iter().find(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some()) && r.target != *k).map(|r| (k.clone(), r.target.clone())))
        .collect();
    let depth = |k: &String| {
        let (mut d, mut c) = (0, k.clone());
        while let Some(p) = parent.get(&c) {
            d += 1;
            c = p.clone();
            if d > 8 {
                break;
            }
        }
        d
    };
    // floor-standing objects (no object parent) whose lowest point is within 5 cm of the floor also get snapped down/up onto it
    let on_floor = |k: &String| -> bool {
        visuals[k].parts.iter().any(|_| true) && {
            let m = mat_of(&state.objs[k].pose);
            let z = visuals[k].parts.iter().flat_map(|p| p.mesh.pos.iter().map(|q| m.apply(*q)[2])).fold(f64::MAX, f64::min);
            z > floor_z - 0.03 && z < floor_z + 0.10
        }
    };
    let floor_keys: Vec<String> = keys.iter().filter(|k| on_floor(k)).cloned().collect();
    let mut order: Vec<&String> = keys.iter().filter(|k| parent.contains_key(*k) || floor_keys.contains(k)).collect();
    order.sort_by_key(|k| depth(k));
    if order.is_empty() {
        return vec![];
    }
    // world-space nodes of every generated object
    let mut nodes: Vec<Node> = vec![];
    let mut owner: Vec<usize> = vec![];
    let mut of_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (ki, k) in keys.iter().enumerate() {
        let m = mat_of(&state.objs[k].pose);
        for (pi, part) in visuals[k].parts.iter().enumerate() {
            let mut mesh = part.mesh.clone();
            mesh.transform(&m);
            of_key.entry(k.clone()).or_default().push(nodes.len());
            nodes.push(Node { name: format!("{k}#{pi}"), mesh, material: 0, class: String::new(), pose: None });
            owner.push(ki);
        }
    }
    let bb: Vec<_> = nodes.iter().map(|n| n.mesh.bounds()).collect();
    let mut bb = bb;
    let mut out = vec![];
    let mut drop_keys: Vec<String> = vec![];
    // move an object and everything related to it (its descendants keep their offset)
    fn shift(state: &mut State, nodes: &mut [Node], bb: &mut [([f64; 3], [f64; 3])], of_key: &HashMap<String, Vec<usize>>, parent: &HashMap<String, String>, k: &str, dz: f64) {
        let kids: Vec<String> = std::iter::once(k.to_string()).chain(parent.iter().filter(|(c, _)| is_desc(parent, c, k)).map(|(c, _)| c.clone())).collect();
        for c in kids {
            if let Some(o) = state.objs.get_mut(&c) {
                let mut p = o.pose.clone();
                p.t[2] += dz;
                o.pose = p;
            }
            for &n in of_key.get(&c).into_iter().flatten() {
                for v in nodes[n].mesh.pos.iter_mut() {
                    v[2] += dz;
                }
                bb[n].0[2] += dz;
                bb[n].1[2] += dz;
            }
        }
    }
    fn is_desc(parent: &HashMap<String, String>, c: &str, anc: &str) -> bool {
        let mut x = c;
        for _ in 0..8 {
            match parent.get(x) {
                Some(p) if p == anc => return true,
                Some(p) => x = p,
                None => return false,
            }
        }
        false
    }
    for k in order {
        if skip.contains(k) {
            continue;
        }
        let mine = &of_key[k];
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &n in mine {
            for d in 0..3 {
                lo[d] = lo[d].min(bb[n].0[d]);
                hi[d] = hi[d].max(bb[n].1[d]);
            }
        }
        if lo[0] > hi[0] {
            continue;
        }
        let cand: Vec<usize> = (0..nodes.len())
            .filter(|&n| owner[n] != keys.iter().position(|x| x == k).unwrap() && bb[n].0[0] <= hi[0] && bb[n].1[0] >= lo[0] && bb[n].0[1] <= hi[1] && bb[n].1[1] >= lo[1] && bb[n].0[2] <= hi[2])
            .collect();
        // contact points: lowest vertices, clustered on a 3 cm grid
        let mut vs: Vec<[f64; 3]> = mine.iter().flat_map(|&n| nodes[n].mesh.pos.iter().copied()).filter(|v| v[2] < lo[2] + 0.01).collect();
        vs.sort_by(|a, b| a[2].partial_cmp(&b[2]).unwrap());
        let mut seen = std::collections::HashSet::new();
        let mut contacts = vec![];
        for v in vs {
            if seen.insert(((v[0] / 0.03).floor() as i64, (v[1] / 0.03).floor() as i64)) {
                contacts.push(v);
                if contacts.len() >= 24 {
                    break;
                }
            }
        }
        // contact points buried inside a solid part (an item placed in a shelf board / plinth): lift out of it
        let buried = contacts.iter().any(|c| {
            let q = [c[0], c[1], c[2] + 0.006];
            cand.iter().any(|&n| (0..3).all(|d| q[d] > bb[n].0[d] && q[d] < bb[n].1[d]) && crate::qc::inside(&nodes[n].mesh, q))
        });
        let mut gap = f64::MAX;
        let size_max = (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
        let cls = &state.objs[k].generator.as_ref().unwrap().class;
        // small tabletop-style items that end up with nothing plausible under them are removed
        let removable = parent.contains_key(k) && size_max < 0.6 && !(cls.contains("Lamp") || cls.contains("Light") || cls.contains("Mirror"));
        // lift out of a board first when buried; items inside a closed cabinet body (no board above them to lift onto) just drop onto the
        // surface below like the others
        let mut buried_lift = f64::MAX;
        if buried {
            for c in &contacts {
                if let Some((z, _, _)) = down_hit(&nodes, &bb, &cand, [c[0], c[1], c[2] + 0.25]) {
                    if z > c[2] + 0.005 {
                        buried_lift = buried_lift.min(c[2] - z);
                    }
                }
            }
        }
        if !buried || buried_lift == f64::MAX {
            for c in &contacts {
                let mut best = down_hit(&nodes, &bb, &cand, [c[0], c[1], c[2] + 0.03]).map(|h| h.0);
                // the floor is a support too for anything standing within a few cm of it
                if c[2] - floor_z < 0.12 && c[2] - floor_z > -0.03 && best.map_or(true, |z| z < floor_z) {
                    best = Some(floor_z);
                }
                if let Some(z) = best {
                    gap = gap.min(c[2] - z);
                }
            }
        }
        if buried && buried_lift != f64::MAX { gap = f64::MAX }
        if gap == f64::MAX {
            // nothing below (or buried in a shelf board / plinth: the placeholder bottom lies below the visible board): lift onto the first
            // surface above its contact points within 25 cm
            for c in &contacts {
                if let Some((z, _, _)) = down_hit(&nodes, &bb, &cand, [c[0], c[1], c[2] + 0.25]) {
                    if z > c[2] + 0.005 {
                        gap = gap.min(c[2] - z);
                    }
                }
            }
            if gap < -0.25 || gap == f64::MAX || gap >= 0.0 {
                if removable && !buried {
                    drop_keys.push(k.clone());
                }
                continue;
            }
            let dz = -gap;
            shift(state, &mut nodes, &mut bb, &of_key, &parent, k, dz);
            out.push(Snapped { key: k.clone(), dz, removed: false });
            continue;
        }
        if gap.abs() < 1e-4 {
            continue;
        }
        let dz = -gap;
        if (dz < 0.0 && -dz > max_drop) || (dz > 0.0 && dz > max_lift) {
            if removable && dz < 0.0 {
                drop_keys.push(k.clone());
            }
            continue;
        }
        shift(state, &mut nodes, &mut bb, &of_key, &parent, k, dz);
        out.push(Snapped { key: k.clone(), dz, removed: false });
    }
    // remove the unsupported items and everything resting on them
    for k in &drop_keys {
        let all: Vec<String> = std::iter::once(k.clone()).chain(parent.keys().filter(|c| is_desc(&parent, c, k)).cloned()).collect();
        for c in all {
            if state.objs.shift_remove(&c).is_some() {
                out.push(Snapped { key: c, dz: 0.0, removed: true });
            }
        }
    }
    out
}

/// python builds KitchenCabinetFactory with its first cabinet cell at local y = 0 (the cabinet runs from y = 0 to y = width) while its
/// placeholder / solver box is centred on the origin, so the visible cabinet sits half a width away from the box the solver kept clear
/// (bookcases placed "next to" it end up inside it, items put on its top hover over nothing). Shift the visual so its horizontal centre
/// matches the solver box. Returns the objects moved.
pub fn recenter_visuals(state: &State, visuals: &mut HashMap<String, crate::assets::Asset>, classes: &[&str]) -> Vec<(String, [f64; 2])> {
    let mut out = vec![];
    for (k, o) in state.objs.iter() {
        let Some(g) = &o.generator else { continue };
        if !classes.contains(&g.class.as_str()) {
            continue;
        }
        let Some(a) = visuals.get_mut(k) else { continue };
        if !a.cloth.is_empty() || !a.followers.is_empty() || a.parts.is_empty() {
            continue;
        }
        let (lo, hi) = a.bounds();
        let d = [(o.geom.lo[0] + o.geom.hi[0]) / 2.0 - (lo[0] + hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0 - (lo[1] + hi[1]) / 2.0];
        if d[0].abs().max(d[1].abs()) < 0.03 {
            continue;
        }
        for p in a.parts.iter_mut() {
            for v in p.mesh.pos.iter_mut() {
                v[0] += d[0];
                v[1] += d[1];
            }
        }
        for c in a.cutters.iter_mut() {
            for v in c.pos.iter_mut() {
                v[0] += d[0];
                v[1] += d[1];
            }
        }
        for s in a.supports.iter_mut() {
            for q in [&mut s.0, &mut s.1] {
                q[0] += d[0];
                q[1] += d[1];
            }
        }
        out.push((k.clone(), d));
    }
    out
}

/// Furniture standing inside other furniture (parts of two unrelated objects whose boxes overlap by more than `min_ov` in every axis):
/// the solver's placeholders allow it when the visible shape is bigger than the box it used. The smaller object (and everything
/// resting on it) is removed. Returns the removed keys.
pub fn remove_overlapping(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, skip: &[String], min_ov: f64) -> Vec<(String, String)> {
    let mat_of = |p: &Pose| Mat4([[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]);
    let exempt = |c: &str| ["Chair", "Stool", "Bed", "Sofa", "Rug", "Curtain", "Lamp", "Light", "Mirror", "WallArt", "Plant", "Door", "Window", "Cloth", "Pillow", "Towel"].iter().any(|e| c.contains(e));
    struct B {
        key: String,
        parts: Vec<([f64; 3], [f64; 3])>,
        lo: [f64; 3],
        hi: [f64; 3],
        parent: Option<String>,
    }
    let mut bs: Vec<B> = vec![];
    for (k, o) in state.objs.iter() {
        let Some(g) = &o.generator else { continue };
        let Some(a) = visuals.get(k) else { continue };
        if exempt(&g.class) || skip.contains(k) {
            continue;
        }
        let m = mat_of(&o.pose);
        let parts: Vec<_> = a
            .parts
            .iter()
            .map(|p| {
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for q in &p.mesh.pos {
                    let w = m.apply(*q);
                    for d in 0..3 {
                        lo[d] = lo[d].min(w[d]);
                        hi[d] = hi[d].max(w[d]);
                    }
                }
                (lo, hi)
            })
            .filter(|(l, _)| l[0] != f64::MAX)
            .collect();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (l, h) in &parts {
            for d in 0..3 {
                lo[d] = lo[d].min(l[d]);
                hi[d] = hi[d].max(h[d]);
            }
        }
        let parent = o.relations.iter().find(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())).map(|r| r.target.clone());
        bs.push(B { key: k.clone(), parts, lo, hi, parent });
    }
    let vol = |b: &B| (0..3).map(|d| b.hi[d] - b.lo[d]).product::<f64>();
    let related = |a: &B, b: &B| a.parent.as_deref() == Some(b.key.as_str()) || b.parent.as_deref() == Some(a.key.as_str());
    let mut gone: Vec<(String, String)> = vec![];
    let mut dead: std::collections::HashSet<String> = Default::default();
    for i in 0..bs.len() {
        for j in i + 1..bs.len() {
            let (a, b) = (&bs[i], &bs[j]);
            if dead.contains(&a.key) || dead.contains(&b.key) || related(a, b) {
                continue;
            }
            if (0..3).any(|d| a.lo[d] > b.hi[d] - min_ov || b.lo[d] > a.hi[d] - min_ov) {
                continue;
            }
            let hit = a.parts.iter().any(|(al, ah)| b.parts.iter().any(|(bl, bh)| (0..3).all(|d| ah[d].min(bh[d]) - al[d].max(bl[d]) > min_ov)));
            if hit {
                let (loser, other) = if vol(a) < vol(b) { (a, b) } else { (b, a) };
                dead.insert(loser.key.clone());
                gone.push((loser.key.clone(), other.key.clone()));
            }
        }
    }
    // descendants go too
    let all_keys: Vec<String> = state.objs.keys().cloned().collect();
    let parent_of: HashMap<String, String> = all_keys.iter().filter_map(|k| state.objs[k].relations.iter().find(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())).map(|r| (k.clone(), r.target.clone()))).collect();
    for (k, _) in &gone {
        for c in &all_keys {
            let mut x = c;
            for _ in 0..8 {
                match parent_of.get(x) {
                    Some(p) if p == k => {
                        dead.insert(c.clone());
                        break;
                    }
                    Some(p) => x = p,
                    None => break,
                }
            }
        }
    }
    for k in &dead {
        state.objs.shift_remove(k);
    }
    gone
}
