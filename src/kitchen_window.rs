//! Upper kitchen cabinets never hang in front of a window: counter runs may stand under windows (program.counter_over_windows), but
//! their wall units covered the glass. Every part of a KitchenSpace visual that is above counter height and in front of a window (its
//! horizontal extent overlaps the window's, within 0.6 m of the wall plane, and its height range overlaps the window's) is dropped, with
//! the rest of its cabinet.
use crate::furn::state::State;
use crate::furn::tags::Tag;
use std::collections::HashMap;

type Box3 = ([f64; 3], [f64; 3]);

pub fn clear_uppers(state: &State, visuals: &mut HashMap<String, crate::assets::Asset>) -> Vec<String> {
    let mut log = vec![];
    let win = Tag::sem("Window");
    let wins: Vec<&crate::furn::state::ObjState> = state.objs.values().filter(|o| o.tags.contains(win.clone())).collect();
    for (k, o) in &state.objs {
        if o.generator.as_ref().map_or(true, |g| g.class != "KitchenSpaceFactory") { continue }
        let Some(a) = visuals.get_mut(k) else { continue };
        // world -> kitchen local (rigid pose)
        let to_local = |p: [f64; 3]| {
            let d = [p[0] - o.pose.t[0], p[1] - o.pose.t[1], p[2] - o.pose.t[2]];
            let r = &o.pose.r;
            [r[0][0] * d[0] + r[1][0] * d[1] + r[2][0] * d[2], r[0][1] * d[0] + r[1][1] * d[1] + r[2][1] * d[2], r[0][2] * d[0] + r[1][2] * d[1] + r[2][2] * d[2]]
        };
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        let mut boxes: Vec<Box3> = vec![];
        for w in &wins {
            let (l, h) = (w.geom.lo, w.geom.hi);
            let (mut bl, mut bh) = ([f64::MAX; 3], [f64::MIN; 3]);
            for i in 0..8 {
                let p = to_local(w.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]));
                for d in 0..3 { bl[d] = bl[d].min(p[d]); bh[d] = bh[d].max(p[d]); }
            }
            // thin horizontal axis = the wall normal: reach 0.6 m into the room
            let t = if bh[0] - bl[0] < bh[1] - bl[1] { 0 } else { 1 };
            bl[t] -= 0.6; bh[t] += 0.6;
            if (0..3).all(|d| bl[d] < hi[d] + 0.05 && lo[d] - 0.05 < bh[d]) { boxes.push((bl, bh)); }
        }
        if boxes.is_empty() { continue }
        let z_upper = lo[2] + 1.2;
        let n0 = a.parts.len();
        let bb: Vec<Box3> = a.parts.iter().map(|p| p.mesh.bounds()).collect();
        let upper: Vec<usize> = (0..bb.len()).filter(|&i| bb[i].0[2] >= z_upper).collect();
        // over the glass by more than 1 cm along the wall, within reach of it (a cabinet beside the window stays; one over its edge
        // covered part of the glass)
        let in_front = |(pl, ph): &Box3| boxes.iter().any(|(bl, bh)| {
            let ov = |d: usize| (ph[d].min(bh[d]) - pl[d].max(bl[d])).max(0.0);
            ov(0) > 0.01 && ov(1) > 0.01 && pl[2] < bh[2] && bl[2] < ph[2]
        });
        // the upper parts grouped into cabinets (parts whose boxes intersect: frame, doors, handles of one unit); a cabinet goes whole
        // when any of its parts is in front of the glass (part by part left a frame or a door hanging)
        let mut group: Vec<usize> = (0..bb.len()).collect();
        let find = |g: &mut Vec<usize>, mut i: usize| { while g[i] != i { g[i] = g[g[i]]; i = g[i]; } i };
        for (ai, &i) in upper.iter().enumerate() {
            for &j in &upper[ai + 1..] {
                if (0..3).all(|d| bb[i].0[d] < bb[j].1[d] - 0.005 && bb[j].0[d] < bb[i].1[d] - 0.005) {
                    let (ri, rj) = (find(&mut group, i), find(&mut group, j));
                    group[ri] = rj;
                }
            }
        }
        // a knob stands proud of its door, its box only touching the cabinet's: it goes with the cabinet it touches
        for &i in &upper {
            if !a.parts[i].mat.name.starts_with("cab_knob") { continue }
            for &j in &upper {
                if j != i && (0..3).all(|d| bb[i].0[d] < bb[j].1[d] + 0.01 && bb[j].0[d] < bb[i].1[d] + 0.01) {
                    let (ri, rj) = (find(&mut group, i), find(&mut group, j));
                    group[ri] = rj;
                }
            }
        }
        let gone: std::collections::HashSet<usize> = upper.iter().filter(|&&i| in_front(&bb[i])).map(|&i| find(&mut group, i)).collect();
        let drop: Vec<bool> = (0..bb.len()).map(|i| upper.contains(&i) && gone.contains(&find(&mut group, i))).collect();
        let mut idx = 0;
        a.parts.retain(|_| { let keep = !drop[idx]; idx += 1; keep });
        if a.parts.len() < n0 { log.push(format!("{} upper parts in front of a window removed from {k}", n0 - a.parts.len())); }
    }
    log
}

/// Built-in dishwashers: infinigen's DishwasherFactory stands at its own height (8-9 cm proud of the counter in the gal22 review,
/// "taller than and in front of the counter, not built in"). A dishwasher within 0.3 m of a counter run is scaled vertically (about its
/// base) so its top sits 5 mm under that run's counter top.
pub fn fit_dishwashers(state: &State, visuals: &mut HashMap<String, crate::assets::Asset>) -> Vec<String> {
    let mut log = vec![];
    let wbox = |o: &crate::furn::state::ObjState, a: &crate::assets::Asset| -> Box3 {
        let (l, h) = a.bounds();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let p = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(p[d]); hi[d] = hi[d].max(p[d]); }
        }
        (lo, hi)
    };
    let counters: Vec<(Box3, f64)> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "KitchenSpaceFactory")).filter_map(|(k, o)| {
        let a = visuals.get(k)?;
        let (lo, _) = a.bounds();
        let top = crate::dress::plateau(a, lo[2] + 0.7, lo[2] + 1.2, 6)?;
        Some((wbox(o, a), o.pose.t[2] + top))
    }).collect();
    // (ranges too: their cooktop at the worktop, which runs 0.8-1.05 m by the counter's style; the range is a fixed 0.92 m)
    let dws: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "DishwasherFactory" || g.class == "OvenFactory")).map(|(k, _)| k.clone()).collect();
    for k in dws {
        let o = &state.objs[&k];
        let Some(a) = visuals.get(&k) else { continue };
        let b = wbox(o, a);
        let near = counters.iter().filter(|(cb, _)| (0..2).all(|d| b.0[d] < cb.1[d] + 0.3 && cb.0[d] - 0.3 < b.1[d])).map(|(_, t)| *t).fold(f64::NAN, f64::min);
        if !near.is_finite() { continue }
        let (lo, hi) = a.bounds();
        // (a range's body top, under its back guard: the highest part ending below the worktop + 0.1)
        let is_range = o.generator.as_ref().map_or(false, |g| g.class == "OvenFactory");
        let body = if is_range { a.parts.iter().map(|p| p.mesh.bounds().1[2]).filter(|z| o.pose.t[2] + *z < near + 0.1).fold(f64::NAN, f64::max) } else { hi[2] };
        let body = if body.is_finite() { body } else { hi[2] };
        let (z0, top) = (o.pose.t[2] + lo[2], o.pose.t[2] + body);
        let want = if is_range { near } else { near - 0.005 };
        if (top - want).abs() < 0.005 || want - z0 < 0.5 { continue }
        let s = (want - z0) / (top - z0);
        let a = visuals.get_mut(&k).unwrap();
        for p in a.parts.iter_mut() { for q in p.mesh.pos.iter_mut() { q[2] = lo[2] + (q[2] - lo[2]) * s; } }
        log.push(format!("dishwasher {k}: top {top:.3} -> {want:.3} (x{s:.3})"));
    }
    log
}
