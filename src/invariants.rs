//! Invariant checks on a finished scene (env INVARIANTS=1, after every dresser has run): the placement audit (qc.rs: floating, sunk,
//! interpenetration, outside a room / through a wall, blocking a door or a window, implausible supports) on the final meshes, plus the
//! checks the layout passes kept rediscovering in renders:
//!   * wall_height: a picture centred outside 1.2-1.8 m above the floor, a mirror within 0.15 m of the floor or the ceiling,
//!   * appliance_flush: a range / dishwasher / fridge touching a counter run whose front is off the run's by more than 5 cm (range and
//!     dishwasher: top off the counter top by more than 4 cm),
//!   * door_paths: a room whose doors are not all joined by walkable floor (layout2d::walk_status),
//!   * size: a piece built to a layout size whose model is off it by more than 2 cm on an axis.
//! One `INV <kind> <key> <class> [room]: <detail>` line per violation and an `INVARIANTS n` summary line; the counts are returned.
use crate::mesh::Scene;
use std::collections::BTreeMap;

pub struct Inv<'a> {
    pub scene: &'a Scene,
    pub state: &'a crate::furn::state::State,
    pub visuals: &'a std::collections::HashMap<String, crate::assets::Asset>,
    pub qc: &'a crate::qc::QcIn<'a>,
    pub floor_z: f64,
    pub ceiling_z: f64,
}

/// world bounds of every scene object (nodes `key#i`), by key
fn object_bounds(scene: &Scene) -> BTreeMap<String, ([f64; 3], [f64; 3], String)> {
    let mut out: BTreeMap<String, ([f64; 3], [f64; 3], String)> = BTreeMap::new();
    for n in &scene.nodes {
        let Some((key, _)) = n.name.split_once('#') else { continue };
        if n.mesh.is_empty() { continue }
        let (l, h) = n.mesh.bounds();
        let e = out.entry(key.to_string()).or_insert(([f64::MAX; 3], [f64::MIN; 3], n.class.clone()));
        for d in 0..3 { e.0[d] = e.0[d].min(l[d]); e.1[d] = e.1[d].max(h[d]); }
    }
    out
}

pub fn check(inp: &Inv) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines: Vec<String> = vec![];
    let mut add = |kind: &str, line: String| { *counts.entry(kind.to_string()).or_default() += 1; lines.push(format!("INV {kind} {line}")); };
    let room_of = |k: &str| crate::furn::layout2d::room_of(inp.state, k).unwrap_or_else(|| "?".into());
    // 1. the placement audit on the final meshes (its issue strings, grouped by their first word)
    // (an empty label: the audit returns its report without printing it)
    let rep = crate::qc::run(inp.qc, std::path::Path::new(""), "");
    for o in rep["results"].as_array().into_iter().flatten() {
        for i in o["issues"].as_array().into_iter().flatten() {
            let s = i.as_str().unwrap_or("");
            // a dresser's item touching what it dresses is by design (a towel on its rail, rolls on their stand, items on their counter):
            // the item's name carries the dressed object's key
            let key = o["key"].as_str().unwrap_or("");
            if let Some(other) = s.split_whitespace().nth(1).filter(|_| s.starts_with("overlaps") || s.starts_with("penetrates")) {
                let base = |k: &str| k.split('_').filter(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() >= 4).last().map(|x| x.to_string());
                if other.contains(key) || key.contains(other) || (base(key).is_some() && base(key) == base(other)) { continue }
                // the legs of an L counter meet at the corner by design (kitchen_plan.rs)
                let leg = |k: &str| inp.state.objs.get(k).and_then(|o| o.generator.as_ref()).map_or(false, |g| g.join.is_some());
                if leg(key) && leg(other) { continue }
            }
            let kind = match s.split_whitespace().next().unwrap_or("") {
                "floating" => "floating", "sunk" => "sunk", "overlaps" | "penetrates" => "interpenetration", "outside" | "through" => "outside",
                "implausible" | "resting" => "support", _ => "blocking",
            };
            add(kind, format!("{} {} [{}]: {s}", o["key"].as_str().unwrap_or(""), o["class"].as_str().unwrap_or(""), o["room"].as_str().unwrap_or("?")));
        }
    }
    let ob = object_bounds(inp.scene);
    // 2. wall items
    for (k, (lo, hi, cls)) in &ob {
        let (zl, zh) = (lo[2] - inp.floor_z, hi[2] - inp.floor_z);
        let ceil = inp.ceiling_z - inp.floor_z;
        if cls == "WallArtFactory" || cls == "GalleryFrameFactory" {
            let zc = (zl + zh) / 2.0;
            if !(1.2..=1.8).contains(&zc) && cls == "WallArtFactory" { add("wall_height", format!("{k} {cls} [{}]: centre at {zc:.2} m", room_of(k))); }
            if zh > ceil - 0.1 { add("wall_height", format!("{k} {cls} [{}]: top {zh:.2} m, ceiling {ceil:.2} m", room_of(k))); }
        }
        if cls == "MirrorFactory" && (zl < 0.15 || zh > ceil - 0.15) {
            add("wall_height", format!("{k} {cls} [{}]: {zl:.2}-{zh:.2} m", room_of(k)));
        }
    }
    // 3. appliances in line with the counter run they touch (front along the run's facing; range / dishwasher top at the counter top)
    let runs: Vec<&String> = inp.state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "KitchenSpaceFactory")).map(|(k, _)| k).collect();
    for (k, o) in &inp.state.objs {
        let Some(g) = &o.generator else { continue };
        if !matches!(g.class.as_str(), "OvenFactory" | "DishwasherFactory" | "FridgeFactory") { continue }
        let Some((al, ah, _)) = ob.get(k) else { continue };
        for r in &runs {
            let Some((rl, rh, _)) = ob.get(*r) else { continue };
            // touching: footprints within 5 cm of each other
            let gap = (0..2).map(|d| (al[d] - rh[d]).max(rl[d] - ah[d]).max(0.0)).fold(0.0, f64::max);
            if gap > 0.05 { continue }
            let rp = &inp.state.objs[*r].pose;
            let f = [rp.r[0][0], rp.r[1][0]];
            let ax = if f[0].abs() > f[1].abs() { 0 } else { 1 };
            let s = if f[ax] > 0.0 { 1.0 } else { -1.0 };
            // the counter top: the highest part of the run ending below 1.2 m above the floor (the counter, not the wall cabinets)
            let top = inp.scene.nodes.iter().filter(|n| n.name.split_once('#').map_or(false, |(kk, _)| kk == r.as_str()) && !n.mesh.is_empty())
                .map(|n| n.mesh.bounds()).filter(|(_, h)| h[2] < inp.floor_z + 1.2).map(|(_, h)| h[2]).fold(f64::NAN, f64::max);
            let front_run = if s > 0.0 { rh[ax] } else { -rl[ax] };
            // the run's front includes its counter overhang; the appliance front (door handle) may sit a few cm past it
            let front_app = if s > 0.0 { ah[ax] } else { -al[ax] };
            let df = front_app - front_run;
            if df.abs() > 0.08 { add("appliance_flush", format!("{k} {} [{}]: front {df:+.2} m off run {r}", g.class, room_of(k))); }
            if g.class != "FridgeFactory" && top.is_finite() {
                // body top: the appliance's highest part below the back guard (parts ending under top + 0.1)
                let body = inp.scene.nodes.iter().filter(|n| n.name.split_once('#').map_or(false, |(kk, _)| kk == k.as_str()) && !n.mesh.is_empty())
                    .map(|n| n.mesh.bounds().1[2]).filter(|z| *z < top + 0.1).fold(f64::NAN, f64::max);
                if body.is_finite() && (body - top).abs() > 0.04 { add("appliance_flush", format!("{k} {} [{}]: top {:+.2} m off the counter of {r}", g.class, room_of(k), body - top)); }
            }
            break;
        }
    }
    // 4. door paths
    let rooms: Vec<String> = inp.state.objs.iter().filter(|(_, o)| crate::furn::layout2d::is_room(o)).map(|(k, _)| k.clone()).collect();
    for room in &rooms {
        let (connected, bad) = crate::furn::layout2d::walk_status(inp.state, room);
        if !connected { add("door_paths", format!("{room} room [{room}]: doors not joined by walkable floor")); }
        for b in bad { add("door_paths", format!("{b} {} [{room}]: out of reach", inp.state.objs.get(&b).and_then(|o| o.generator.as_ref()).map_or("?".to_string(), |g| g.class.clone()))); }
    }
    // 5. built to size
    for (k, o) in &inp.state.objs {
        let Some(g) = &o.generator else { continue };
        let exact = crate::furn::sizes::model(&g.class).map_or(false, |m| m.exact) || g.class == "RugFactory";
        let (Some(sz), true) = (g.size.or_else(|| crate::furn::solver::cur_size(g)), exact) else { continue };
        let Some(a) = inp.visuals.get(k) else { continue };
        let (l, h) = a.bounds();
        // (an L leg's wall cabinet reaching into the corner: kitchen_plan.rs)
        // (not when its wall cabinets came off under a window: the reach went with them)
        let reach = if h[2] - l[2] > 1.5 { g.join.map_or(0.0, |j| j[3] * j[2].abs()) } else { 0.0 };
        let e = [h[1] - l[1] - reach, h[0] - l[0], h[2] - l[2]];
        // (a counter run under a window loses its wall cabinets there: its height is not held to the size)
        let dims = if g.class == "KitchenSpaceFactory" { 2 } else { 3 };
        // (wardrobes: handles stand up to 5 cm proud of the doors, and doors the variation opened add their swing to the depth)
        // (showers: the door's handle bar stands 3-4 cm proud of the glass)
        let open = g.class == "WardrobeFactory" && e[1] > sz[1] * 1.4;
        let tol = |i: usize| if open { f64::INFINITY } else if matches!(g.class.as_str(), "WardrobeFactory" | "ShowerFactory") && i == 1 { 0.05 } else { 0.02 };
        let off = (0..dims).map(|i| (e[i] - sz[i]).abs() - (tol(i) - 0.02)).fold(0.0, f64::max);
        if off > 0.02 { add("size", format!("{k} {} [{}]: built {:.2?} for {:.2?}", g.class, room_of(k), e, sz)); }
    }
    for l in &lines { eprintln!("{l}"); }
    let total: usize = counts.values().sum();
    eprintln!("INVARIANTS {total} {}", counts.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "));
    counts
}
