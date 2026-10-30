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
