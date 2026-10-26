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
}

/// `max_drop`: largest correction accepted downwards (m); `max_lift`: upwards. Returns the applied corrections.
pub fn snap_children(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, skip: &[String], max_drop: f64, max_lift: f64) -> Vec<Snapped> {
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
    let mut order: Vec<&String> = keys.iter().filter(|k| parent.contains_key(*k)).collect();
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
        let mut gap = f64::MAX;
        for c in &contacts {
            if let Some((z, _, _)) = down_hit(&nodes, &bb, &cand, [c[0], c[1], c[2] + 0.03]) {
                gap = gap.min(c[2] - z);
            }
        }
        if gap == f64::MAX || gap.abs() < 1e-4 {
            continue;
        }
        // accepted: a drop onto a surface within max_drop, or a lift out of a surface by at most max_lift
        let dz = -gap;
        if (dz < 0.0 && -dz > max_drop) || (dz > 0.0 && dz > max_lift) {
            continue;
        }
        let o = state.objs.get_mut(k).unwrap();
        let mut p = o.pose.clone();
        p.t[2] += dz;
        o.pose = p;
        for &n in mine {
            for v in nodes[n].mesh.pos.iter_mut() {
                v[2] += dz;
            }
            bb[n].0[2] += dz;
            bb[n].1[2] += dz;
        }
        out.push(Snapped { key: k.clone(), dz });
    }
    out
}
