//! Floor-plan nudges: a shared interior wall between two rooms moved by `d` metres (positive: room `a` grows into `b`). Used by
//! the plan refinement (tools/plan_refine.py): the house is laid out again with the moved wall and the change kept when the
//! furnished layout scores better. Only the longest shared axis-aligned piece of the wall moves; the strip it sweeps must lie
//! entirely in the room that shrinks, and both rooms must stay single polygons of 2 m2 or more.

use super::state::*;
use crate::shp::*;

/// the longest shared axis-aligned edge piece of `a` and `b`: (vertical, coordinate, lo, hi)
fn shared_edge(pa: &G, pb: &G) -> Option<(bool, f64, f64, f64)> {
    let (ea, eb) = (exterior(pa), exterior(pb));
    let mut best: Option<(bool, f64, f64, f64)> = None;
    for i in 0..ea.len().saturating_sub(1) {
        let (p, q) = (ea[i], ea[i + 1]);
        for j in 0..eb.len().saturating_sub(1) {
            let (r, s) = (eb[j], eb[j + 1]);
            for vertical in [true, false] {
                let (c, k) = if vertical { (0, 1) } else { (1, 0) };
                if (p[c] - q[c]).abs() > 1e-6 || (r[c] - s[c]).abs() > 1e-6 || (p[c] - r[c]).abs() > 1e-6 { continue }
                let lo = p[k].min(q[k]).max(r[k].min(s[k]));
                let hi = p[k].max(q[k]).min(r[k].max(s[k]));
                if hi - lo > best.map_or(0.3, |b| b.3 - b.2) { best = Some((vertical, p[c], lo, hi)); }
            }
        }
    }
    best
}

/// move the wall between rooms `a` and `b` by `d` (a grows when positive); false (state unchanged) when it does not fit
pub fn nudge(state: &mut RoomState, a: &str, b: &str, d: f64) -> bool {
    if !state.objs.contains_key(a) || !state.objs.contains_key(b) || d.abs() < 1e-3 { return false }
    let (pa, pb) = (state.poly(a).clone(), state.poly(b).clone());
    let Some((vertical, x0, lo, hi)) = shared_edge(&pa, &pb) else { return false };
    // which side of the wall b is on
    let cb = centroid(&pb);
    let side = if vertical { (cb[0] - x0).signum() } else { (cb[1] - x0).signum() };
    // the swept strip: into b when a grows, into a when it shrinks
    let (s0, s1) = if d > 0.0 { (x0, x0 + side * d) } else { (x0 + side * d, x0) };
    let (s0, s1) = (s0.min(s1), s0.max(s1));
    let strip = if vertical { bbox(s0, lo, s1, hi) } else { bbox(lo, s0, hi, s1) };
    let (from, to) = if d > 0.0 { (&pb, &pa) } else { (&pa, &pb) };
    if area(&intersection(&strip, from)) < area(&strip) * 0.99 { return false }
    let nf = difference(from, &strip);
    let nt = union(to, &strip);
    let single = |g: &G| is_polygon(g) && parts(g).len() <= 1 && area(g) >= 2.0;
    if !single(&nf) || !single(&nt) { return false }
    let (nf, nt) = (simplify(&nf, 1e-3), simplify(&nt, 1e-3));
    let (na, nb) = if d > 0.0 { (nt, nf) } else { (nf, nt) };
    state.objs.get_mut(a).unwrap().polygon = Some(na);
    state.objs.get_mut(b).unwrap().polygon = Some(nb);
    update_shared(state, a);
    update_shared(state, b);
    update_exterior(state, a);
    update_exterior(state, b);
    true
}

/// hallways that serve fewer than two private rooms (bedrooms, bathrooms), and stubs under 6 m2, do no work as hallways: each is merged into the
/// neighbour it shares the longest wall with (public rooms first), which takes over its doors; an entrance hallway stays, and a
/// merge that would not leave one polygon is skipped. Returns the merges made.
pub fn merge_useless_hallways(state: &mut RoomState, graph: &mut RoomGraph) -> Vec<String> {
    let mut log = vec![];
    let halls: Vec<String> = state.valid_rooms().filter(|(k, _)| room_type(k) == "hallway").map(|(k, _)| k.clone()).collect();
    for h in halls {
        let Some(hi) = graph.names.iter().position(|n| *n == h) else { continue };
        if graph.entrance == Some(hi) || graph.invalid.contains(&hi) { continue }
        let ns: Vec<String> = graph.valid_neighbours(&h).into_iter().map(|s| s.to_string()).collect();
        let private = ns.iter().filter(|n| matches!(room_type(n), "bedroom" | "bathroom")).count();
        // (a stub under 6 m2 is a patch of floor in front of doors: the room it opens off can take those doors itself)
        if private >= 2 && area(state.poly(&h)) >= 6.0 { continue }
        let ph = state.poly(&h).clone();
        // the neighbour to merge into: public rooms first, then the longest shared wall
        let mut cands: Vec<(i32, f64, String)> = ns.iter().filter(|n| state.objs.contains_key(n.as_str())).map(|n| {
            let pub_ = matches!(room_type(n), "living-room" | "dining-room" | "kitchen") as i32;
            (pub_, length(&shared(&ph, state.poly(n))), n.clone())
        }).filter(|c| c.1 >= 1.0).collect();
        cands.sort_by(|a, b| (b.0, b.1).partial_cmp(&(a.0, a.1)).unwrap());
        let Some((_, _, t)) = cands.into_iter().next() else { continue };
        let merged = simplify(&union(state.poly(&t), &ph), 1e-3);
        if !(is_polygon(&merged) && parts(&merged).len() <= 1) { continue }
        state.objs.get_mut(&t).unwrap().polygon = Some(merged);
        state.objs.shift_remove(&h);
        for o in state.objs.values_mut() { o.relations.retain(|r| r.target != h); }
        update_shared(state, &t);
        update_exterior(state, &t);
        // the hallway's neighbours now open off the merged room
        let ti = graph.names.iter().position(|n| *n == t).unwrap();
        for n in ns.iter().filter(|n| **n != t) {
            if let Some(ni) = graph.names.iter().position(|x| x == n) {
                if !graph.ns[ti].contains(&ni) { graph.ns[ti].push(ni); }
                if !graph.ns[ni].contains(&ti) { graph.ns[ni].push(ti); }
            }
        }
        for v in graph.ns.iter_mut() { v.retain(|&j| j != hi); }
        graph.ns[hi].clear();
        graph.invalid.insert(hi);
        log.push(format!("hallway {h} ({private} private rooms off it) merged into {t}"));
    }
    log
}
