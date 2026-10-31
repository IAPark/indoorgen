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
    let is_rug = |k: &String| state.objs[k].generator.as_ref().map_or(false, |g| g.class.contains("Rug"));
    let mut order: Vec<&String> = keys.iter().filter(|k| parent.contains_key(*k) || floor_keys.contains(k) || is_rug(k)).collect();
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
        if state.objs[k].generator.as_ref().map_or(false, |g| g.class.contains("Rug")) {
            // rugs lie flat on the floor; a tilted rug (the solver sometimes leans one against a bed / wall) is removed
            if hi[2] - lo[2] > 0.06 {
                drop_keys.push(k.clone());
                continue;
            }
            let dz = floor_z - lo[2];
            if dz.abs() > 1e-4 && dz.abs() < 1.0 {
                shift(state, &mut nodes, &mut bb, &of_key, &parent, k, dz);
                out.push(Snapped { key: k.clone(), dz, removed: false });
            }
            continue;
        }
        let cand: Vec<usize> = (0..nodes.len())
            .filter(|&n| !state.objs[&keys[owner[n]]].generator.as_ref().map_or(false, |g| g.class.contains("Rug")) && owner[n] != keys.iter().position(|x| x == k).unwrap() && bb[n].0[0] <= hi[0] && bb[n].1[0] >= lo[0] && bb[n].0[1] <= hi[1] && bb[n].1[1] >= lo[1] && bb[n].0[2] <= hi[2])
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
pub fn remove_overlapping(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, skip: &[String], min_ov: f64, rooms: &[(String, crate::shp::G)]) -> Vec<(String, String)> {
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
    let classes: std::collections::HashMap<String, (String, bool)> = state.objs.iter().filter_map(|(k, o)| o.generator.as_ref().map(|g| (k.clone(), (g.class.clone(), g.join.is_some())))).collect();
    let class_of = |k: &str| classes.get(k).map(|c| c.0.clone()).unwrap_or_default();
    // (the two legs of an L meet at the corner by design: kitchen_plan.rs)
    let l_leg = |k: &str| classes.get(k).map_or(false, |c| c.1);
    let related = |a: &B, b: &B| a.parent.as_deref() == Some(b.key.as_str()) || b.parent.as_deref() == Some(a.key.as_str()) || (l_leg(&a.key) && l_leg(&b.key));
    let mut gone: Vec<(String, String)> = vec![];
    let mut shifted: Vec<(String, f64)> = vec![];
    // (by class: pieces the group passes added carry other names, a planned kitchen's runs `<room>_kline<n>`)
    let ess_key = |k: &str| { let c = class_of(k); ["ToiletFactory", "BathtubFactory", "ShowerFactory", "StandingSinkFactory", "BathroomSinkFactory", "KitchenSpaceFactory", "FridgeFactory", "WardrobeFactory", "TableDiningFactory", "TVStandFactory", "CoffeeTableFactory"].iter().any(|e| k.ends_with(e) || c == *e) };
    let ess_pair = |a: &B, b: &B| ess_key(&a.key) && ess_key(&b.key);
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
            if hit && std::env::var_os("OVDBG").is_some() {
                for (al, ah) in &a.parts { for (bl, bh) in &b.parts { if (0..3).all(|d| ah[d].min(bh[d]) - al[d].max(bl[d]) > min_ov) { eprintln!("OVDBG {} {:.2?}..{:.2?} x {} {:.2?}..{:.2?}", a.key, al, ah, b.key, bl, bh); } } }
            }
            if hit && ess_pair(a, b) {
                // two essential fixtures (toilet against a pedestal sink): slide one of them clear along the axis of least overlap; a shift
                // must keep its footprint inside the room (0.18 m in from the wall centre line: half a wall + 3 cm) and off every other object
                let mut best = (f64::MAX, 0usize, 0usize, 0.0f64);
                for (mv, other) in [(i, j), (j, i)] {
                    let (m, o) = (&bs[mv], &bs[other]);
                    let room = rooms.iter().find(|(_, g)| crate::shp::contains_pt(g, [(m.lo[0] + m.hi[0]) / 2.0, (m.lo[1] + m.hi[1]) / 2.0])).map(|(_, g)| crate::shp::buffer_mitre(g, -0.18));
                    for d in 0..2 {
                        for p in [o.hi[d] - m.lo[d] + 0.02, -(m.hi[d] - o.lo[d] + 0.02)] {
                            if p.abs() >= best.0 { continue }
                            let (mut lo, mut hi) = (m.lo, m.hi);
                            lo[d] += p; hi[d] += p;
                            let inside = room.as_ref().map_or(true, |g| [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]].iter().all(|q| crate::shp::contains_pt(g, *q)));
                            // only along a wall the fixture is backed against (its other axis within 0.25 m of the room edge), so it stays on it
                            let e = 1 - d;
                            let c = [(m.lo[0] + m.hi[0]) / 2.0, (m.lo[1] + m.hi[1]) / 2.0];
                            let along_wall = room.as_ref().map_or(false, |g| [m.lo[e] - 0.25, m.hi[e] + 0.25].iter().any(|&v| { let mut q = c; q[e] = v; !crate::shp::contains_pt(g, q) }));
                            let inside = inside && along_wall;
                            let free = bs.iter().all(|x| x.key == m.key || dead.contains(&x.key) || (0..3).any(|e| x.lo[e] > hi[e] - min_ov || lo[e] > x.hi[e] - min_ov));
                            if std::env::var_os("SHIFT_DEBUG").is_some() { eprintln!("SHIFT {} d{d} {p:.2} inside {inside} free {free} blockers {:?}", m.key, bs.iter().filter(|x| !(x.key == m.key || dead.contains(&x.key) || (0..3).any(|e| x.lo[e] > hi[e] - min_ov || lo[e] > x.hi[e] - min_ov))).map(|x| x.key.clone()).collect::<Vec<_>>()); }
                            if inside && free { best = (p.abs(), mv, d, p) }
                        }
                    }
                }
                if best.0 < 1.5 {
                    let key = bs[best.1].key.clone();
                    // the fixture and everything resting on it (bottles on a sink)
                    let mut group = vec![key.clone()];
                    loop {
                        let more: Vec<String> = state.objs.iter().filter(|(k, o)| !group.contains(*k) && o.relations.iter().any(|r| group.contains(&r.target))).map(|(k, _)| k.clone()).collect();
                        if more.is_empty() { break }
                        group.extend(more);
                    }
                    for gk in &group { if let Some(ob) = state.objs.get_mut(gk) { ob.pose.t[best.2] += best.3; } }
                    shifted.push((key, best.3));
                    continue;
                }
            }
            if hit {
                // the room's essential fixtures lose only to each other (a toilet "inside" a standing sink's box went, the sink stayed)
                let ess = |x: &B| ess_key(&x.key);
                let (loser, other) = match (ess(a), ess(b)) {
                    (true, false) => (b, a),
                    (false, true) => (a, b),
                    _ => if vol(a) < vol(b) { (a, b) } else { (b, a) },
                };
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
    for (k, d) in &shifted { eprintln!("OVERLAP shifted {k} by {d:.2} m (two essential fixtures)"); }
    gone
}

/// Two rugs lying on top of each other read as a thick slab: keep the larger one. Returns the removed keys.
pub fn remove_overlapping_rugs(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, min_ov: f64) -> Vec<String> {
    let mat_of = |p: &Pose| Mat4([[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]);
    let mut rugs: Vec<(String, [f64; 2], [f64; 2])> = vec![];
    for (k, o) in state.objs.iter() {
        if !o.generator.as_ref().map_or(false, |g| g.class.contains("Rug")) { continue }
        let Some(a) = visuals.get(k) else { continue };
        let m = mat_of(&o.pose);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &a.parts { for q in &p.mesh.pos { let w = m.apply(*q); for d in 0..2 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); } } }
        if lo[0] < hi[0] { rugs.push((k.clone(), lo, hi)); }
    }
    let area = |r: &(String, [f64; 2], [f64; 2])| (r.2[0] - r.1[0]) * (r.2[1] - r.1[1]);
    let mut dead: Vec<String> = vec![];
    for i in 0..rugs.len() {
        for j in i + 1..rugs.len() {
            let (a, b) = (&rugs[i], &rugs[j]);
            if dead.contains(&a.0) || dead.contains(&b.0) { continue }
            if (0..2).all(|d| a.2[d].min(b.2[d]) - a.1[d].max(b.1[d]) > min_ov) {
                dead.push(if area(a) < area(b) { a.0.clone() } else { b.0.clone() });
            }
        }
    }
    for k in &dead { state.objs.shift_remove(k); }
    dead
}

/// Rugs under room features (fireplace hearths, built-in shelves, window seats): the solver never saw the features, so a rug could lie
/// under the hearth with the fireplace drawn on top of it. Each such rug slides out along the axis of least overlap (at most `max_shift`)
/// or is removed. Footprints are the world xy boxes of the features' parts within 0.3 m of the floor. Returns (key, "moved" | "removed").
pub fn rugs_clear_of(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, keys: &[String], max_shift: f64) -> Vec<(String, &'static str)> {
    let mat_of = |p: &Pose| Mat4([[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]);
    let mut feet: Vec<([f64; 2], [f64; 2])> = vec![];
    for k in keys {
        let (Some(o), Some(a)) = (state.objs.get(k), visuals.get(k)) else { continue };
        let m = mat_of(&o.pose);
        let zmin = a.parts.iter().flat_map(|p| p.mesh.pos.iter().map(|q| m.apply(*q)[2])).fold(f64::MAX, f64::min);
        for p in &a.parts {
            let w: Vec<[f64; 3]> = p.mesh.pos.iter().map(|q| m.apply(*q)).collect();
            if w.is_empty() || w.iter().map(|q| q[2]).fold(f64::MAX, f64::min) > zmin + 0.3 { continue }
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for q in &w { for d in 0..2 { lo[d] = lo[d].min(q[d]); hi[d] = hi[d].max(q[d]); } }
            feet.push((lo, hi));
        }
    }
    let mut out = vec![];
    if feet.is_empty() { return out }
    let rugs: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class.contains("Rug"))).map(|(k, _)| k.clone()).collect();
    for k in rugs {
        let Some(a) = visuals.get(&k) else { continue };
        let m = mat_of(&state.objs[&k].pose);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &a.parts { for q in &p.mesh.pos { let w = m.apply(*q); for d in 0..2 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); } } }
        let mut shift = [0.0f64; 2];
        let mut dead = false;
        for _ in 0..4 {
            let (l, h) = ([lo[0] + shift[0], lo[1] + shift[1]], [hi[0] + shift[0], hi[1] + shift[1]]);
            let Some((fl, fh)) = feet.iter().find(|(fl, fh)| (0..2).all(|d| l[d].max(fl[d]) < h[d].min(fh[d]) - 0.02)) else { break };
            // smallest move that clears this footprint
            let cands = [(0, fh[0] - l[0] + 0.03), (0, fl[0] - h[0] - 0.03), (1, fh[1] - l[1] + 0.03), (1, fl[1] - h[1] - 0.03)];
            let (d, v) = cands.iter().cloned().min_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap()).unwrap();
            shift[d] += v;
            if shift[0].hypot(shift[1]) > max_shift { dead = true; break }
        }
        if dead { state.objs.shift_remove(&k); out.push((k, "removed")); continue }
        if shift != [0.0, 0.0] {
            let o = state.objs.get_mut(&k).unwrap();
            let t = o.pose.t;
            o.pose.set_loc([t[0] + shift[0], t[1] + shift[1], t[2]]);
            out.push((k, "moved"));
        }
    }
    out
}

/// Furniture whose VISIBLE mesh pokes through a wall (the solver placed the python placeholder flush with the wall, the real asset is
/// deeper, e.g. a bed headboard): move it back into its room along the wall normals, at most `max_push`, unless the move makes it
/// overlap other furniture. Objects resting on it move along. `rooms` = inner room polygons (wall faces). Returns (key, dx, dy).
pub fn push_out_of_walls(state: &mut State, visuals: &HashMap<String, crate::assets::Asset>, rooms: &[Vec<[f64; 2]>], max_push: f64) -> Vec<(String, f64, f64)> {
    let mat_of = |p: &Pose| Mat4([[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]);
    let wall_bound = |c: &str| ["Rug", "Door", "Window", "Curtain", "WallArt", "Mirror", "Light", "Lamp", "Shelf", "Cabinet", "KitchenSpace", "Bookcase", "Feature"].iter().any(|e| c.contains(e));
    let in_poly = |p: [f64; 2], poly: &[[f64; 2]]| -> bool {
        let (mut c, n) = (false, poly.len());
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (poly[i], poly[j]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] { c = !c; }
            j = i;
        }
        c
    };
    let mut keys: Vec<String> = state.objs.iter().filter(|(k, o)| o.generator.is_some() && visuals.contains_key(*k)).map(|(k, _)| k.clone()).collect();
    keys.sort();
    let parent: HashMap<String, String> = keys.iter().filter_map(|k| state.objs[k].relations.iter().find(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some()) && r.target != *k).map(|r| (k.clone(), r.target.clone()))).collect();
    let world_box = |state: &State, k: &str| -> ([f64; 3], [f64; 3]) {
        let m = mat_of(&state.objs[k].pose);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for p in &visuals[k].parts { for q in &p.mesh.pos { let w = m.apply(*q); for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); } } }
        (lo, hi)
    };
    let mut out = vec![];
    for k in &keys {
        let cls = state.objs[k].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
        if parent.contains_key(k) || wall_bound(&cls) { continue }
        let m = mat_of(&state.objs[k].pose);
        let verts: Vec<[f64; 3]> = visuals[k].parts.iter().flat_map(|p| p.mesh.pos.iter().map(|q| m.apply(*q))).filter(|v| v[2] < 2.6).collect();
        if verts.is_empty() { continue }
        let (cx, cy) = verts.iter().fold((0.0, 0.0), |a, v| (a.0 + v[0], a.1 + v[1]));
        let c = [cx / verts.len() as f64, cy / verts.len() as f64];
        let Some(poly) = rooms.iter().find(|p| p.len() > 2 && in_poly(c, p)) else { continue };
        // per wall edge: the deepest vertex beyond it
        let n = poly.len();
        let mut depth = vec![0.0f64; n];
        for v in &verts {
            let p = [v[0], v[1]];
            if in_poly(p, poly) { continue }
            let (mut best, mut bi) = (f64::MAX, 0);
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
                let l2 = ab[0] * ab[0] + ab[1] * ab[1];
                let t = if l2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = (p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1]);
                if d < best { best = d; bi = i; }
            }
            depth[bi] = depth[bi].max(best);
        }
        if depth.iter().all(|d| *d <= 0.02) { continue }
        // push inwards: sum over the penetrated edges of (inward normal * depth + 5 mm)
        let area2: f64 = (0..n).map(|i| poly[i][0] * poly[(i + 1) % n][1] - poly[(i + 1) % n][0] * poly[i][1]).sum();
        let (mut dx, mut dy) = (0.0, 0.0);
        for i in 0..n {
            if depth[i] <= 0.02 { continue }
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let l = ex.hypot(ey).max(1e-9);
            // inward normal: left of the edge for a counter-clockwise polygon
            let (nx, ny) = if area2 > 0.0 { (-ey / l, ex / l) } else { (ey / l, -ex / l) };
            dx += nx * (depth[i] + 0.005);
            dy += ny * (depth[i] + 0.005);
        }
        let dbg = std::env::var_os("WALLDBG").is_some();
        if dbg { eprintln!("WALLDBG {k} depth {:?} push ({dx:.3},{dy:.3})", depth.iter().filter(|d| **d > 0.02).collect::<Vec<_>>()); }
        if dx.hypot(dy) > max_push { continue }
        // the move must not make it overlap other floor furniture
        let (lo, hi) = world_box(state, k);
        let (nlo, nhi) = ([lo[0] + dx, lo[1] + dy, lo[2]], [hi[0] + dx, hi[1] + dy, hi[2]]);
        let ov = |a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])| (0..3).map(|d| (a.1[d].min(b.1[d]) - a.0[d].max(b.0[d])).max(0.0)).fold(f64::MAX, f64::min);
        let blocked = keys.iter().any(|o| {
            if o == k || parent.contains_key(o) { return false }
            let oc = state.objs[o].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
            if oc.contains("Rug") { return false }
            let ob = world_box(state, o);
            ov((nlo, nhi), ob) > ov((lo, hi), ob) + 0.02
        });
        if blocked { if dbg { eprintln!("WALLDBG {k} blocked"); } continue }
        let kids: Vec<String> = std::iter::once(k.clone()).chain(keys.iter().filter(|c| { let mut x = c.as_str(); for _ in 0..8 { match parent.get(x) { Some(p) if p == k => return true, Some(p) => x = p, None => return false } } false }).cloned()).collect();
        for c in kids {
            if let Some(o) = state.objs.get_mut(&c) { let mut p = o.pose.clone(); p.t[0] += dx; p.t[1] += dy; o.pose = p; }
        }
        out.push((k.clone(), dx, dy));
    }
    out
}
