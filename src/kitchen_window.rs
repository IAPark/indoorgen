//! Upper kitchen cabinets never hang in front of a window: counter runs may stand under windows (program.counter_over_windows), but
//! their wall units covered the glass. Every part of a KitchenSpace visual that is above counter height and in front of a window (its
//! horizontal extent overlaps the window's, within 0.6 m of the wall plane, and its height range overlaps the window's) is dropped.
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
        a.parts.retain(|p| {
            let (pl, ph) = p.mesh.bounds();
            if pl[2] < z_upper { return true }
            !boxes.iter().any(|(bl, bh)| {
                // most of the part's width in front of the glass (a cabinet beside the window stays)
                let ov = |d: usize| (ph[d].min(bh[d]) - pl[d].max(bl[d])).max(0.0) / (ph[d] - pl[d]).max(1e-6);
                ov(0) > 0.4 && ov(1) > 0.4 && pl[2] < bh[2] && bl[2] < ph[2]
            })
        });
        if a.parts.len() < n0 { log.push(format!("{} upper parts in front of a window removed from {k}", n0 - a.parts.len())); }
    }
    log
}
