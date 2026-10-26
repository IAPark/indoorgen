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
            z > floor_z - 0.03 && z < floor_z + 0.05
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
        if !buried {
            for c in &contacts {
                let mut best = down_hit(&nodes, &bb, &cand, [c[0], c[1], c[2] + 0.03]).map(|h| h.0);
                // the floor is a support too for anything standing within a few cm of it
                if c[2] - floor_z < 0.08 && c[2] - floor_z > -0.03 && best.map_or(true, |z| z < floor_z) {
                    best = Some(floor_z);
                }
                if let Some(z) = best {
                    gap = gap.min(c[2] - z);
                }
            }
        }
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
