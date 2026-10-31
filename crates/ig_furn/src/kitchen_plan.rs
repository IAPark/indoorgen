//! Kitchens planned as sequences of lengths first (the kitchen's room layer; KITCHEN_SEQ=0 keeps the old template and run passes).
//!   1. Lines: every straight wall of the kitchen cut into free stretches (doors, door clearances, built-in features and other fixed
//!      pieces cut it; windows are marked, not cut). Two lines meeting at a room corner share it: one of them owns the corner square.
//!   2. Plan: each line holds a sequence of items -- counter modules (0.3-1.2 m in 0.15 m steps), the sink base, the range, the
//!      dishwasher, the fridge -- packed from one end, with the leftover either open floor at the other end or one filler strip in a
//!      counter. The plan is annealed, scored on the items' footprints with the kitchen rules (kitchen_rules.rs): door zones, the
//!      fridge off corners, the work triangle, landing counter beside the range and the fridge, counter between the sink and the
//!      range, the dishwasher by the sink, the sink under a window, the aisle between facing lines, no narrow slots.
//!   3. Build: each run of adjacent counter items becomes one counter run built to its width (the kitchen's counter depth and
//!      height), the appliances stand at their spots, the sink is set on its run; the next best plan is built when one does not fit.
//!      Planned runs carry `KitchenPlanned`: the built-in fill does not grow them.
use super::groups::{class_of, lohi, yaw, free_ok, new_piece, place_along, match_run, wall_segments, room_type_of};
use super::layout2d::{self, Prism};
use super::solver::Ctx;
use super::state::{ObjState, State};
use super::tags::Tag;
use crate::rng::Rng;
use std::sync::Arc;

thread_local! {
    /// room -> (run, wall point) of each planned kitchen's sink
    static PLANNED_SINK: std::cell::RefCell<std::collections::HashMap<String, (String, [f64; 2])>> = std::cell::RefCell::new(Default::default());
}

/// the run and wall point the plan put this kitchen's sink at
pub fn planned_sink(room: &str) -> Option<(String, [f64; 2])> { PLANNED_SINK.with(|m| m.borrow().get(room).cloned()) }

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind { Counter, Sink, Range, Dish, Fridge }

#[derive(Clone, Copy, Debug)]
struct Item { kind: Kind, w: f64 }

#[derive(Clone, Debug)]
struct LinePlan { items: Vec<Item>, at_end: bool, fill: Option<usize> }

#[derive(Clone, Debug)]
struct Plan { lines: Vec<LinePlan>, owner: Vec<usize> }

/// a free stretch of wall: from `a` along `u` for `len`, room side `n`; `corner0` / `corner1`: the room corner at that end (index
/// into the corner list) when the stretch reaches it; `win`: window intervals along it
#[derive(Clone, Debug)]
struct Line { a: [f64; 2], u: [f64; 2], n: [f64; 2], len: f64, corner: [Option<usize>; 2], win: Vec<(f64, f64)>, intent: Vec<(f64, f64)> }

/// a placed item: its footprint rectangle and front centre
#[derive(Clone, Debug)]
struct Placed { line: usize, idx: usize, kind: Kind, t0: f64, t1: f64, rect: Vec<[f64; 2]>, front: [f64; 2], depth: f64 }

struct Kit {
    lines: Vec<Line>,
    /// lines a built plan could not use (the walkway rule): left empty
    banned: Vec<usize>,
    /// (line, end) pairs meeting at each corner
    corners: Vec<Vec<(usize, usize)>>,
    rings: Vec<Vec<[f64; 2]>>,
    obstacles: Vec<Arc<Vec<Prism>>>,
    d_counter: f64,
    d: [f64; 5],
    w: [f64; 5],
    has: [bool; 5],
    area: f64,
    lr: super::layout_rules::LayoutRules,
}

fn ki(k: Kind) -> usize { match k { Kind::Counter => 0, Kind::Sink => 1, Kind::Range => 2, Kind::Dish => 3, Kind::Fridge => 4 } }
fn is_counter(k: Kind) -> bool { matches!(k, Kind::Counter | Kind::Sink) }
fn built_in(cls: &str) -> bool { matches!(cls, "KitchenSpaceFactory" | "KitchenCabinetFactory" | "SinkFactory" | "FridgeFactory" | "OvenFactory" | "DishwasherFactory") }

fn rect(l: &Line, t0: f64, t1: f64, d: f64) -> Vec<[f64; 2]> {
    let p = |t: f64, e: f64| [l.a[0] + l.u[0] * t + l.n[0] * e, l.a[1] + l.u[1] * t + l.n[1] * e];
    vec![p(t0, 0.0), p(t1, 0.0), p(t1, d), p(t0, d)]
}
fn hits(a: &[[f64; 2]], prs: &[Prism], z0: f64, z1: f64) -> bool { prs.iter().any(|p| p.z[1] > z0 && p.z[0] < z1 && layout2d::poly_dist(a, &p.poly) <= 0.0) }
fn ov(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool { layout2d::poly_dist(a, b) <= 0.0 }
fn dist(a: [f64; 2], b: [f64; 2]) -> f64 { (a[0] - b[0]).hypot(a[1] - b[1]) }

impl Kit {
    fn new(state: &State, room: &str, d: [f64; 5], w: [f64; 5], has: [bool; 5]) -> Kit {
        let lr = super::layout_rules::get();
        let rings = layout2d::rings(&layout2d::room_floor(state, room));
        let nc = Tag::sem("NoCollision");
        let win = Tag::sem("Window");
        // fixed pieces: everything in the room but the kitchen's own pieces (and what stands on them), rugs, no-collision markers
        let floor_z = state.objs.iter().filter(|(_, o)| class_of(o) == "KitchenSpaceFactory").map(|(k, _)| layout2d::prisms(state, k).iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min)).fold(f64::INFINITY, f64::min);
        let floor_z = if floor_z.is_finite() { floor_z } else { 0.15 };
        let kitchen_kids = |o: &ObjState| o.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| built_in(class_of(t))));
        let mut obstacles = vec![];
        let mut windows = vec![];
        // door clearances: the line keeps 15 cm more off them along the wall (a run ending at the jamb stood in the door's passage)
        let mut doors: Vec<Arc<Vec<Prism>>> = vec![];
        // windows the wall layer lays out after the furniture (WindowIntent): where the plan had them, a soft pull
        let intents: Vec<Arc<Vec<Prism>>> = {
            let rr = layout2d::rings(&layout2d::room_floor(state, room));
            // (a window lies in the wall: within 0.3 m of the room's outline)
            state.objs.iter().filter(|(_, o)| o.tags.contains(Tag::sem("WindowIntent"))).map(|(k, _)| layout2d::prisms(state, k))
                .filter(|pr| pr.iter().any(|p| layout2d::ring_dist(&rr, [(p.bb[0] + p.bb[2]) / 2.0, (p.bb[1] + p.bb[3]) / 2.0]) < 0.3)).collect()
        };
        for (k, o) in &state.objs {
            if layout2d::is_room(o) || o.tags.contains(nc.clone()) || built_in(class_of(o)) || kitchen_kids(o) || class_of(o).contains("Rug") { continue }
            if matches!(class_of(o), "WallArtFactory" | "MirrorFactory" | "CeilingLightFactory") { continue }
            if k.starts_with("doorclear/") && !k.starts_with(&format!("doorclear/{room}/")) { continue }
            // (a feature's keep-out by its feature's room)
            let rk = layout2d::room_of(state, k).or_else(|| k.rsplit_once(".keepout").and_then(|(f, _)| layout2d::room_of(state, f)));
            // (an opening shared with the next room counts wherever it is filed: a passage filed under the other room was missed)
            let opening = ["door", "window", "passage", "entrance"].iter().any(|p| k.starts_with(p)) && o.generator.is_none();
            let mine = rk.as_deref() == Some(room) || k.starts_with("doorclear/") || (o.generator.is_none() && rk.is_none()) || opening;
            if !mine { continue }
            let prs = layout2d::prisms(state, k);
            if prs.iter().all(|p| (p.bb[0] > 1e8)) { continue }
            // (a window with its sill under the counter top stops the counter like a wall: the floor's height is the room's)
            let low = o.tags.contains(win.clone()) && prs.iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min) < floor_z + 0.97;
            if o.tags.contains(win.clone()) && !low { windows.push(prs) } else if k.starts_with("doorclear/") { doors.push(prs.clone()); obstacles.push(prs) } else { obstacles.push(prs) }
        }
        // lines: the straight walls (axis-aligned) in 5 cm cells, a cell free when a counter-deep strip before it clears the fixed pieces
        let segs: Vec<_> = wall_segments(state, room).into_iter().filter(|(_, _, n)| n[0].abs() > 0.99 || n[1].abs() > 0.99).collect();
        let mut lines = vec![];
        let mut ends: Vec<([f64; 2], usize, usize)> = vec![];
        for (a, b, n) in &segs {
            let len = dist(*a, *b);
            let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            let probe = Line { a: *a, u, n: *n, len, corner: [None, None], win: vec![], intent: vec![] };
            let cell = 0.05;
            let nc_ = (len / cell).floor() as usize;
            let dmax = d.iter().cloned().fold(0.6, f64::max) + 0.02;
            let free: Vec<bool> = (0..nc_).map(|i| {
                let r = rect(&probe, i as f64 * cell + 0.005, (i + 1) as f64 * cell - 0.005, dmax);
                let rw = rect(&probe, i as f64 * cell - 0.15, (i + 1) as f64 * cell + 0.15, dmax + 0.15);
                !obstacles.iter().any(|p| hits(&r, p, -1.0, 2.0)) && !doors.iter().any(|p| hits(&rw, p, -1.0, 2.0))
            }).collect();
            let wcell: Vec<bool> = (0..nc_).map(|i| { let r = rect(&probe, i as f64 * cell, (i + 1) as f64 * cell, 0.2); windows.iter().any(|p| hits(&r, p, -1.0, 9.0)) }).collect();
            let icell: Vec<bool> = (0..nc_).map(|i| { let r = rect(&probe, i as f64 * cell - 0.15, (i + 1) as f64 * cell + 0.15, 0.25); intents.iter().any(|p| hits(&r, p, -1.0, 9.0)) }).collect();
            let mut i = 0;
            while i < nc_ {
                if !free[i] { i += 1; continue }
                let j0 = i;
                while i < nc_ && free[i] { i += 1; }
                // (5 cm off an obstacle that cut the line: a passage's box reached into the next cell)
                let (t0, t1) = (j0 as f64 * cell + if j0 > 0 { 0.05 } else { 0.0 }, if i == nc_ { len } else { i as f64 * cell - 0.05 });
                if t1 - t0 < 0.6 { continue }
                let mut win = vec![];
                let mut k = j0;
                while k < i { if wcell[k] { let s = k; while k < i && wcell[k] { k += 1; } win.push((s as f64 * cell - t0, k as f64 * cell - t0)); } else { k += 1; } }
                let mut intent = vec![];
                let mut k = j0;
                while k < i { if icell[k] { let s = k; while k < i && icell[k] { k += 1; } intent.push((s as f64 * cell - t0, k as f64 * cell - t0)); } else { k += 1; } }
                let li = lines.len();
                lines.push(Line { a: [a[0] + u[0] * t0, a[1] + u[1] * t0], u, n: *n, len: t1 - t0, corner: [None, None], win, intent });
                if j0 == 0 { ends.push((*a, li, 0)); }
                if i == nc_ { ends.push((*b, li, 1)); }
            }
        }
        // corners: line ends at the same room corner
        let mut corners: Vec<Vec<(usize, usize)>> = vec![];
        let mut cpts: Vec<[f64; 2]> = vec![];
        for (p, li, e) in ends {
            let ci = cpts.iter().position(|q| dist(*q, p) < 0.05).unwrap_or_else(|| { cpts.push(p); corners.push(vec![]); cpts.len() - 1 });
            corners[ci].push((li, e));
            lines[li].corner[e] = Some(ci);
        }
        if ig_core::env_on!("KPLANDBG") { for (i, l) in lines.iter().enumerate() { eprintln!("KPLAN line {i}: {:.2} m from {:?} n {:?} corners {:?} windows {:?}", l.len, l.a, l.n, l.corner, l.win); } }
        let area = state.objs[room].polygon.as_ref().map_or(10.0, |p| crate::shp::area(p));
        Kit { lines, banned: vec![], corners, rings, obstacles, d_counter: d[0], d, w, has, area, lr }
    }

    /// the usable stretch of a line under the plan's corner owners: [start, end]
    fn span(&self, plan: &Plan, li: usize) -> (f64, f64) {
        let l = &self.lines[li];
        let mut s = (0.0, l.len);
        for e in 0..2 {
            let Some(ci) = l.corner[e] else { continue };
            // another used line at this corner that owns it: this one starts past its depth
            let used_other = self.corners[ci].iter().any(|(o, _)| *o != li && !plan.lines[*o].items.is_empty());
            if used_other && plan.owner[ci] != li {
                // (butted against the corner leg's front: the pair is built as one L, kitchen_plan::build)
                if e == 0 { s.0 = self.d_counter + 0.003 } else { s.1 = l.len - self.d_counter - 0.003 }
            }
        }
        s
    }

    fn place(&self, plan: &Plan) -> Option<Vec<Placed>> {
        let mut out = vec![];
        for (li, lp) in plan.lines.iter().enumerate() {
            if lp.items.is_empty() { continue }
            let (s0, s1) = self.span(plan, li);
            let sum: f64 = lp.items.iter().map(|i| i.w).sum();
            let left = s1 - s0 - sum;
            if left < -1e-6 { return None }
            let mut ws: Vec<f64> = lp.items.iter().map(|i| i.w).collect();
            let mut t = s0;
            match lp.fill {
                Some(fi) if fi < ws.len() && is_counter(lp.items[fi].kind) => ws[fi] += left,
                _ => if lp.at_end { t += left },
            }
            let l = &self.lines[li];
            for (idx, (it, w)) in lp.items.iter().zip(ws).enumerate() {
                let d = self.d[ki(it.kind)];
                let r = rect(l, t, t + w, d);
                let front = { let m = t + w / 2.0; [l.a[0] + l.u[0] * m + l.n[0] * d, l.a[1] + l.u[1] * m + l.n[1] * d] };
                out.push(Placed { line: li, idx, kind: it.kind, t0: t, t1: t + w, rect: r, front, depth: d });
                t += w;
            }
        }
        Some(out)
    }

    /// the plan's score (higher is better) and what it breaks; None when it does not fit
    fn score(&self, plan: &Plan, why: bool) -> Option<(f64, Vec<String>)> {
        if self.banned.iter().any(|b| !plan.lines[*b].items.is_empty()) { return None }
        let ps = self.place(plan)?;
        let lr = &self.lr;
        let mut s = 0.0;
        let mut br: Vec<String> = vec![];
        let mut pen = |s: &mut f64, v: f64, m: &dyn Fn() -> String| { *s -= v; if why { br.push(m()); } };
        // every wanted piece placed once
        for k in [Kind::Sink, Kind::Range, Kind::Dish, Kind::Fridge] {
            let n = ps.iter().filter(|p| p.kind == k).count();
            if self.has[ki(k)] && n != 1 { return None }
        }
        // counter length up to what the room calls for
        let counter: f64 = ps.iter().filter(|p| is_counter(p.kind)).map(|p| p.t1 - p.t0).sum();
        let target = (1.5 + 0.22 * self.area).min(7.0);
        s += counter.min(target) + 0.15 * (counter - target).max(0.0);
        // runs: adjacent counters on a line; fewer, and none narrower than 0.6 m
        for (li, lp) in plan.lines.iter().enumerate() {
            if lp.items.is_empty() { continue }
            let on: Vec<&Placed> = ps.iter().filter(|p| p.line == li).collect();
            let mut i = 0;
            while i < on.len() {
                if !is_counter(on[i].kind) { i += 1; continue }
                let j0 = i;
                while i < on.len() && is_counter(on[i].kind) { i += 1; }
                let w = on[i - 1].t1 - on[j0].t0;
                s -= 0.5;
                if w < 0.45 { return None }
                if w < 0.6 { pen(&mut s, 2.0, &|| format!("{w:.2} m counter on line {li}")); }
            }
            // the leftover at a wall end: open floor narrower than a cabinet reads as a mistake (beside a fridge: its door's room)
            let (s0, s1) = self.span(plan, li);
            let (first, last) = (on[0], on[on.len() - 1]);
            for (gap, it) in [(first.t0 - s0, first), (s1 - last.t1, last)] {
                if gap > 0.03 && gap < lr.counter_gap_max && !(it.kind == Kind::Fridge && gap >= lr.fridge_corner_gap - 0.01) {
                    pen(&mut s, 2.0, &|| format!("{gap:.2} m slot at the end of line {li}"));
                }
            }
            // a fridge never stands in front of a window; a range under one is poor (the curtain, the draught)
            for p in &on {
                let ov_win = self.lines[li].win.iter().any(|(a, b)| p.t1 - 0.02 > *a && p.t0 + 0.02 < *b);
                if ov_win && p.kind == Kind::Fridge { return None }
                if ov_win && p.kind == Kind::Range { pen(&mut s, 3.0, &|| "range under a window".into()); }
                if ov_win && p.kind == Kind::Sink { s += 2.0; }
                // a window still to come there (the wall layer moves it to the clearest stretch): leave it counter, ideally the sink
                let ov_int = self.lines[li].intent.iter().map(|(a, b)| (p.t1.min(*b) - p.t0.max(*a)).max(0.0)).sum::<f64>();
                if ov_int > 0.05 {
                    match p.kind { Kind::Fridge => pen(&mut s, 3.0, &|| "fridge where a window comes".into()), Kind::Range => pen(&mut s, 2.0, &|| "range where a window comes".into()), Kind::Sink => s += 1.5, _ => {} }
                }
            }
        }
        // footprints: inside the room, clear of the fixed pieces and of each other (corners)
        for (i, p) in ps.iter().enumerate() {
            for q in &p.rect { let c = [q[0] * 0.98 + (p.rect[0][0] + p.rect[2][0]) * 0.01, q[1] * 0.98 + (p.rect[0][1] + p.rect[2][1]) * 0.01]; if !layout2d::in_rings(&self.rings, c) { return None } }
            let shrunk: Vec<[f64; 2]> = { let c = [(p.rect[0][0] + p.rect[2][0]) / 2.0, (p.rect[0][1] + p.rect[2][1]) / 2.0]; p.rect.iter().map(|q| [c[0] + (q[0] - c[0]) * 0.97, c[1] + (q[1] - c[1]) * 0.97]).collect() };
            if self.obstacles.iter().any(|o| hits(&shrunk, o, -1.0, 2.0)) { return None }
            for q in ps.iter().skip(i + 1) { if q.line != p.line && ov(&shrunk, &q.rect) { return None } }
        }
        // door zones: free floor before each appliance
        let zone = |p: &Placed| -> Option<Vec<[f64; 2]>> {
            let l = &self.lines[p.line];
            let depth = match p.kind { Kind::Fridge => (p.t1 - p.t0) + lr.fridge_door_clear, Kind::Range | Kind::Dish => lr.appliance_front_clear, _ => return None };
            let r = rect(l, p.t0 + 0.02, p.t1 - 0.02, p.depth + depth);
            Some(vec![[r[0][0] + l.n[0] * (p.depth + 0.02), r[0][1] + l.n[1] * (p.depth + 0.02)], [r[1][0] + l.n[0] * (p.depth + 0.02), r[1][1] + l.n[1] * (p.depth + 0.02)], r[2], r[3]])
        };
        for p in &ps {
            let Some(z) = zone(p) else { continue };
            let off = z.iter().filter(|q| !layout2d::in_rings(&self.rings, **q)).count();
            let blocked = ps.iter().any(|q| q.line != p.line && ov(&z, &q.rect)) || self.obstacles.iter().any(|o| hits(&z, o, -1.0, 1.4));
            if off > 0 || blocked { pen(&mut s, 4.0, &|| format!("door zone of the {:?}", p.kind)); }
        }
        // aisle: facing lines' fronts at least the aisle apart where they overlap
        for p in &ps {
            for q in &ps {
                if q.line <= p.line { continue }
                let (np, nq) = (self.lines[p.line].n, self.lines[q.line].n);
                if np[0] * nq[0] + np[1] * nq[1] > -0.99 { continue }
                let gap = { let d = [q.front[0] - p.front[0], q.front[1] - p.front[1]]; d[0] * np[0] + d[1] * np[1] };
                let along = { let d = [q.front[0] - p.front[0], q.front[1] - p.front[1]]; (d[0] * np[1] - d[1] * np[0]).abs() };
                if along < ((p.t1 - p.t0) + (q.t1 - q.t0)) / 2.0 && gap < lr.kitchen_aisle { pen(&mut s, 6.0, &|| format!("aisle {gap:.2} m")); }
            }
        }
        // the fridge off corners: open floor or in-line counter beside each side
        for p in ps.iter().filter(|p| matches!(p.kind, Kind::Fridge | Kind::Range)) {
            let l = &self.lines[p.line];
            for (t, sg) in [(p.t0, -1.0), (p.t1, 1.0)] {
                let q = { let tt = t + sg * lr.fridge_corner_gap; let e = p.depth * 0.75; [l.a[0] + l.u[0] * tt + l.n[0] * e, l.a[1] + l.u[1] * tt + l.n[1] * e] };
                let walled = !layout2d::in_rings(&self.rings, q) || self.obstacles.iter().any(|o| o.iter().any(|pr| pr.z[1] > 0.5 && layout2d::in_rings(&[pr.poly.clone()], q)));
                let turned = ps.iter().any(|r| r.line != p.line && layout2d::in_rings(&[r.rect.clone()], q));
                if walled || turned { pen(&mut s, if p.kind == Kind::Fridge { 5.0 } else { 4.0 }, &|| format!("{:?} in a corner", p.kind)); }
            }
        }
        // neighbours along a line
        let next = |p: &Placed, d: i64| -> Option<&Placed> { ps.iter().find(|q| q.line == p.line && q.idx as i64 == p.idx as i64 + d && (if d > 0 { (q.t0 - p.t1).abs() < 0.01 } else { (p.t0 - q.t1).abs() < 0.01 })) };
        let run_len = |p: &Placed, d: i64| -> f64 { let mut w = 0.0; let mut c = p; while let Some(q) = next(c, d) { if !is_counter(q.kind) { break } w += q.t1 - q.t0; c = q; } w };
        for p in &ps {
            match p.kind {
                // landing counter: 0.3 m or more on both sides of the range, 0.4 m on one side of the fridge
                Kind::Range => for d in [-1, 1] { if run_len(p, d) < 0.3 { pen(&mut s, 2.0, &|| "range without counter beside it".into()); } },
                Kind::Fridge => if run_len(p, -1).max(run_len(p, 1)) < 0.4 { pen(&mut s, 1.5, &|| "fridge without landing counter".into()); },
                Kind::Dish => { let by_sink = [-1, 1].iter().any(|d| next(p, *d).map_or(false, |q| q.kind == Kind::Sink)); if by_sink { s += 1.5 } else { pen(&mut s, 1.0, &|| "dishwasher away from the sink".into()); } }
                _ => {}
            }
        }
        // counter between the sink and the range on a line
        if let (Some(sk), Some(rg)) = (ps.iter().find(|p| p.kind == Kind::Sink), ps.iter().find(|p| p.kind == Kind::Range)) {
            if sk.line == rg.line {
                let (a, b) = if sk.t0 < rg.t0 { (sk.t1, rg.t0) } else { (rg.t1, sk.t0) };
                let between_ok = ps.iter().filter(|p| p.line == sk.line && p.t0 >= a - 0.01 && p.t1 <= b + 0.01).all(|p| p.kind == Kind::Counter);
                if b - a < lr.sink_range_gap - 0.01 || !between_ok { pen(&mut s, 3.0, &|| format!("{:.2} m between the sink and the range", b - a)); }
            }
        }
        // the work triangle
        let f = |k: Kind| ps.iter().find(|p| p.kind == k).map(|p| p.front);
        if let (Some(fr), Some(rg), Some(sk)) = (f(Kind::Fridge), f(Kind::Range), f(Kind::Sink)) {
            let legs = [dist(fr, rg), dist(rg, sk), dist(sk, fr)];
            let mut bad = 0.0;
            for l in legs { bad += (lr.triangle_leg_min - l).max(0.0) + (l - lr.triangle_leg_max).max(0.0); }
            let sum: f64 = legs.iter().sum();
            bad += 0.5 * ((lr.triangle_sum_min - sum).max(0.0) + (sum - lr.triangle_sum_max).max(0.0));
            if bad > 0.05 { pen(&mut s, 1.5 * bad, &|| format!("work triangle {:.2}/{:.2}/{:.2}", legs[0], legs[1], legs[2])); }
        }
        // a used line on its own (not meeting another at a corner) short of 1.2 m reads as a stray block
        for (li, lp) in plan.lines.iter().enumerate() {
            if lp.items.is_empty() { continue }
            let w: f64 = ps.iter().filter(|p| p.line == li).map(|p| p.t1 - p.t0).sum();
            let joined = self.lines[li].corner.iter().flatten().any(|ci| self.corners[*ci].iter().any(|(o, _)| *o != li && !plan.lines[*o].items.is_empty()));
            if !joined && w < 1.2 { pen(&mut s, 2.0, &|| format!("short line {li}")); }
            if plan.lines.iter().filter(|l| !l.items.is_empty()).count() > 1 && !joined { s -= 1.0; }
        }
        Some((s, br))
    }

    fn module(rng: &mut Rng) -> f64 { 0.3 + 0.15 * rng.randint(0, 7) as f64 }

    fn random(&self, rng: &mut Rng) -> Plan {
        let n = self.lines.len();
        let mut lines: Vec<LinePlan> = (0..n).map(|_| LinePlan { items: vec![], at_end: rng.f64() < 0.5, fill: None }).collect();
        let longest = (0..n).filter(|i| !self.banned.contains(i)).max_by(|a, b| self.lines[*a].len.partial_cmp(&self.lines[*b].len).unwrap()).unwrap_or(0);
        let mut put = |lines: &mut Vec<LinePlan>, it: Item, rng: &mut Rng| {
            let ok: Vec<usize> = (0..n).filter(|i| !self.banned.contains(i)).collect();
            let li = if rng.f64() < 0.6 && !self.banned.contains(&longest) { longest } else { ok[rng.randint(0, ok.len() as i64) as usize] };
            let pos = rng.randint(0, lines[li].items.len() as i64 + 1) as usize;
            lines[li].items.insert(pos, it);
        };
        for k in [Kind::Sink, Kind::Range, Kind::Dish, Kind::Fridge] {
            if self.has[ki(k)] { put(&mut lines, Item { kind: k, w: self.w[ki(k)] }, rng); }
        }
        for _ in 0..rng.randint(2, 6) { put(&mut lines, Item { kind: Kind::Counter, w: Self::module(rng) }, rng); }
        for l in lines.iter_mut() { if !l.items.is_empty() && rng.f64() < 0.6 { l.fill = l.items.iter().position(|i| i.kind == Kind::Counter); } }
        Plan { lines, owner: self.corners.iter().map(|c| c[rng.randint(0, c.len() as i64) as usize].0).collect() }
    }

    fn mutate(&self, p: &Plan, rng: &mut Rng) -> Plan {
        let mut q = p.clone();
        let n = q.lines.len();
        let used: Vec<usize> = (0..n).filter(|i| !q.lines[*i].items.is_empty()).collect();
        let pick_line = |rng: &mut Rng| rng.randint(0, n as i64) as usize;
        match rng.choice_idx(8) {
            // an item to another spot (any line)
            0 | 1 if !used.is_empty() => {
                let li = used[rng.randint(0, used.len() as i64) as usize];
                let i = rng.randint(0, q.lines[li].items.len() as i64) as usize;
                let it = q.lines[li].items.remove(i);
                if q.lines[li].fill.map_or(false, |f| f >= q.lines[li].items.len()) { q.lines[li].fill = None; }
                let lj = if rng.f64() < 0.5 { li } else { pick_line(rng) };
                let pos = rng.randint(0, q.lines[lj].items.len() as i64 + 1) as usize;
                q.lines[lj].items.insert(pos, it);
            }
            // a counter module more / less / resized
            2 => { let lj = if !used.is_empty() && rng.f64() < 0.8 { used[rng.randint(0, used.len() as i64) as usize] } else { pick_line(rng) }; let pos = rng.randint(0, q.lines[lj].items.len() as i64 + 1) as usize; q.lines[lj].items.insert(pos, Item { kind: Kind::Counter, w: Self::module(rng) }); }
            3 if !used.is_empty() => {
                let li = used[rng.randint(0, used.len() as i64) as usize];
                let cs: Vec<usize> = (0..q.lines[li].items.len()).filter(|i| q.lines[li].items[*i].kind == Kind::Counter).collect();
                if !cs.is_empty() { let i = cs[rng.randint(0, cs.len() as i64) as usize]; q.lines[li].items.remove(i); q.lines[li].fill = None; }
            }
            4 if !used.is_empty() => {
                let li = used[rng.randint(0, used.len() as i64) as usize];
                let cs: Vec<usize> = (0..q.lines[li].items.len()).filter(|i| q.lines[li].items[*i].kind == Kind::Counter).collect();
                if !cs.is_empty() { let i = cs[rng.randint(0, cs.len() as i64) as usize]; let w = q.lines[li].items[i].w + if rng.f64() < 0.5 { 0.15 } else { -0.15 }; q.lines[li].items[i].w = w.clamp(0.3, 1.2); }
            }
            // which end the line packs from, or a filler strip
            5 if !used.is_empty() => { let li = used[rng.randint(0, used.len() as i64) as usize]; q.lines[li].at_end = !q.lines[li].at_end; }
            6 if !used.is_empty() => {
                let li = used[rng.randint(0, used.len() as i64) as usize];
                let cs: Vec<usize> = (0..q.lines[li].items.len()).filter(|i| q.lines[li].items[*i].kind == Kind::Counter).collect();
                q.lines[li].fill = if q.lines[li].fill.is_some() || cs.is_empty() { None } else { Some(cs[rng.randint(0, cs.len() as i64) as usize]) };
            }
            // who owns a corner
            _ => if !q.owner.is_empty() { let ci = rng.randint(0, q.owner.len() as i64) as usize; let c = &self.corners[ci]; q.owner[ci] = c[rng.randint(0, c.len() as i64) as usize].0; },
        }
        q
    }
}

/// the kitchen rooms laid out from a plan (the rest keep the old passes)
pub fn plan_kitchens(ctx: &mut Ctx, state: &mut State, log: &mut Vec<String>) -> Vec<String> {
    let mut done = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen").map(|(k, _)| k.clone()).collect();
    for room in rooms {
        if let Some(l) = plan_room(ctx, state, &room) { log.extend(l); done.push(room); }
        else { log.push(format!("{room}: no kitchen plan fits, the old passes lay it out")); }
    }
    done
}

fn plan_room(ctx: &mut Ctx, state: &mut State, room: &str) -> Option<Vec<String>> {
    PLANNED_SINK.with(|m| { m.borrow_mut().remove(room); });
    let dbg = ig_core::env_on!("KPLANDBG");
    let in_room = |st: &State, c: &str| -> Vec<String> { st.objs.iter().filter(|(k, o)| class_of(o) == c && layout2d::room_of(st, k).as_deref() == Some(room)).map(|(k, _)| k.clone()).collect() };
    let runs = in_room(state, "KitchenSpaceFactory");
    let proto = runs.first().cloned()?;
    let fridge = in_room(state, "FridgeFactory").first().cloned();
    let range = in_room(state, "OvenFactory").first().cloned();
    // (a dishwasher whose box does not fit under the counter -- the placeholder comes with its door open, 1 m by 1.2 m -- goes)
    let dish = in_room(state, "DishwasherFactory").first().cloned().filter(|k| { let (l, h) = lohi(state, k); h[1] - l[1] <= 0.7 && h[0] - l[0] <= 0.85 });
    let saved = state.objs.clone();
    // the room's kitchen pieces out: other runs, cabinets, sinks, second appliances, what stands on any of them
    let keep: Vec<String> = [Some(proto.clone()), fridge.clone(), range.clone(), dish.clone()].into_iter().flatten().collect();
    for c in ["KitchenSpaceFactory", "KitchenCabinetFactory", "SinkFactory", "FridgeFactory", "OvenFactory", "DishwasherFactory"] {
        for k in in_room(state, c) { if !keep.contains(&k) { layout2d::remove_tree(state, &k); } }
    }
    let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| keep.contains(&r.target))).map(|(k, _)| k.clone()).collect();
    for k in kids { if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); } }
    let parked: Vec<(String, ObjState)> = keep.iter().map(|k| (k.clone(), state.objs.shift_remove(k).unwrap())).collect();
    let restore = |state: &mut State| { state.objs = saved.clone(); for k in state.objs.keys().cloned().collect::<Vec<_>>() { state.touch(&k); } };
    // widths and depths: the counter's (the first run's size), each appliance's own, the sink base a 0.6 m or 0.8 m module
    let size = |o: &ObjState| { let (l, h) = (o.geom.lo, o.geom.hi); (h[1] - l[1], h[0] - l[0]) };
    let pk = |k: &Option<String>| k.as_ref().and_then(|k| parked.iter().find(|(kk, _)| kk == k)).map(|(_, o)| size(o));
    let (_, dc) = size(&parked[0].1);
    // (the built box can run deeper than the nominal size: the worktop's overhang)
    let dc = proto_depth(state, &parked[0].1).unwrap_or(dc).max(dc);
    let (fw, fd) = pk(&fridge).unwrap_or((0.0, 0.0));
    let (ow, od) = pk(&range).unwrap_or((0.0, 0.0));
    let (dw, dd) = pk(&dish).unwrap_or((0.0, 0.0));
    let mut rng = Rng::seeded(ig_core::int_hash(&["kitchen_plan", room]));
    let sink_w = if rng.f64() < 0.5 { 0.6 } else { 0.8 };
    // (appliance slots 1 cm wider than the appliance: 5 mm play each side)
    let mut kit = Kit::new(state, room, [dc, dc, od, dd, fd], [0.0, sink_w, ow + 0.01, dw + 0.01, fw + 0.01], [true, true, range.is_some(), dish.is_some(), fridge.is_some()]);
    if kit.lines.is_empty() { restore(state); return None }
    // anneal, then build the best plans; a line every built plan fails on (a run cutting the walkway) is banned and the
    // annealing run again (three rounds)
    let mut log = vec![];
    for round in 0..3 {
        if kit.banned.len() >= kit.lines.len() { break }
        let best = anneal(&kit, &mut rng);
        if best.is_empty() { if dbg { eprintln!("KPLAN {room}: {} lines, no plan fits (round {round})", kit.lines.len()); } break }
        let mut fails: Vec<usize> = vec![];
        for (rank, (s, plan)) in best.iter().enumerate() {
            let (_, broken) = kit.score(plan, true).unwrap();
            if dbg { eprintln!("KPLAN {room} #{rank} {s:.2} {broken:?}\n{}", describe(&kit, plan)); }
            restore(state);
            for c in ["KitchenSpaceFactory", "KitchenCabinetFactory", "SinkFactory", "FridgeFactory", "OvenFactory", "DishwasherFactory"] {
                for k in in_room(state, c) { if !keep.contains(&k) { layout2d::remove_tree(state, &k); } }
            }
            let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| keep.contains(&r.target))).map(|(k, _)| k.clone()).collect();
            for k in kids { if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); } }
            for k in &keep { state.objs.shift_remove(k); }
            match build(ctx, state, room, &kit, plan, &parked, &proto, fridge.as_deref(), range.as_deref(), dish.as_deref(), &mut rng, dbg) {
                Ok(()) => {
                    log.push(format!("{room}: kitchen planned (score {s:.2}{}):\n{}", if broken.is_empty() { String::new() } else { format!(", breaks {broken:?}") }, describe(&kit, plan)));
                    return Some(log);
                }
                Err(li) => fails.push(li),
            }
        }
        // the line most plans failed on
        let Some(worst) = (0..kit.lines.len()).max_by_key(|l| fails.iter().filter(|f| *f == l).count()) else { break };
        if !fails.contains(&worst) { break }
        kit.banned.push(worst);
        if dbg { eprintln!("KPLAN {room}: line {worst} banned"); }
    }
    restore(state);
    None
}

/// random starts, then annealing steps from the best (temperature 2 -> 0.03); the best few distinct plans, best first
fn anneal(kit: &Kit, rng: &mut Rng) -> Vec<(f64, Plan)> {
    let mut best: Vec<(f64, Plan)> = vec![];
    let keep_best = |best: &mut Vec<(f64, Plan)>, s: f64, p: &Plan| {
        if best.iter().any(|(b, _)| (b - s).abs() < 1e-6) { return }
        best.push((s, p.clone()));
        best.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        best.truncate(6);
    };
    let mut cur: Option<(f64, Plan)> = None;
    for _ in 0..300 {
        let p = kit.random(rng);
        if let Some((s, _)) = kit.score(&p, false) { keep_best(&mut best, s, &p); if cur.as_ref().map_or(true, |c| s > c.0) { cur = Some((s, p)); } }
    }
    let Some(mut cur) = cur else { return best };
    let steps = 6000;
    for i in 0..steps {
        let temp = 2.0 * (0.015f64).powf(i as f64 / steps as f64);
        let q = kit.mutate(&cur.1, rng);
        let Some((s, _)) = kit.score(&q, false) else { continue };
        if s > cur.0 || rng.f64() < ((s - cur.0) / temp).exp() { cur = (s, q.clone()); keep_best(&mut best, s, &q); }
    }
    best
}

/// the counter depth: the first run's size (its depth), else its box
fn proto_depth(_state: &State, o: &ObjState) -> Option<f64> { o.generator.as_ref().and_then(super::solver::cur_size).map(|c| c[1]) }

fn describe(kit: &Kit, plan: &Plan) -> String {
    let mut out = vec![];
    for (li, lp) in plan.lines.iter().enumerate() {
        if lp.items.is_empty() { continue }
        let l = &kit.lines[li];
        let (s0, s1) = kit.span(plan, li);
        let items: Vec<String> = lp.items.iter().enumerate().map(|(i, it)| format!("{}{:?} {:.2}", if lp.fill == Some(i) { "+" } else { "" }, it.kind, it.w)).collect();
        out.push(format!("  line {li} ({:.2} m usable of {:.2}, normal {:?}, from the {}): {}", s1 - s0, l.len, [l.n[0].round(), l.n[1].round()], if lp.at_end { "end" } else { "start" }, items.join(" | ")));
    }
    out.join("\n")
}

#[allow(clippy::too_many_arguments)]
fn build(ctx: &mut Ctx, state: &mut State, room: &str, kit: &Kit, plan: &Plan, parked: &[(String, ObjState)], proto: &str, fridge: Option<&str>, range: Option<&str>,
         dish: Option<&str>, rng: &mut Rng, dbg: bool) -> Result<(), usize> {
    let Some(ps) = kit.place(plan) else { return Err(usize::MAX) };
    let planned = Tag::sem("KitchenPlanned");
    let floor_rel: Vec<_> = parked[0].1.relations.iter().filter(|r| r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))).cloned().collect();
    let put = |state: &mut State, k: &str| { if let Some((_, o)) = parked.iter().find(|(kk, _)| kk == k) { state.objs.insert(k.to_string(), o.clone()); state.touch(k); } };
    let mut run_i = 0;
    let mut sink_on: Option<(String, [f64; 2])> = None;
    // the run each counter item went into
    let mut run_of: std::collections::HashMap<(usize, usize), String> = Default::default();
    for (li, _) in plan.lines.iter().enumerate() {
        let l = &kit.lines[li];
        let on: Vec<&Placed> = ps.iter().filter(|p| p.line == li).collect();
        let mut i = 0;
        while i < on.len() {
            let p = on[i];
            if is_counter(p.kind) {
                let j0 = i;
                while i < on.len() && is_counter(on[i].kind) { i += 1; }
                let (t0, t1) = (on[j0].t0, on[i - 1].t1);
                let name = if run_i == 0 { proto.to_string() } else { format!("{room}_kline{run_i}").replace('/', "_") };
                if run_i == 0 { put(state, proto); } else {
                    if !new_piece(ctx, state, rng, "KitchenSpaceFactory", room, &name) { return Err(li) }
                    state.objs.get_mut(&name).unwrap().relations = floor_rel.clone();
                    state.touch(&name);
                }
                run_i += 1;
                for q in &on[j0..i] { run_of.insert((q.line, q.idx), name.clone()); }
                if !match_run(ctx, state, &name, proto, t1 - t0) && state.objs[&name].generator.as_ref().and_then(super::solver::cur_size).map_or(true, |c| (c[0] - (t1 - t0)).abs() > 0.03) {
                    if dbg { eprintln!("KPLAN build: run {name} not sized to {:.2}", t1 - t0); }
                    return Err(li)
                }
                state.objs.get_mut(&name).unwrap().tags.insert(planned.clone());
                if dbg { let (a, b) = lohi(state, &name); eprintln!("KPLAN build: run {name} asked {:.3} built {:.3} (size {:?})", t1 - t0, b[1] - a[1], state.objs[&name].generator.as_ref().and_then(super::solver::cur_size)); }
                if !place_along(state, &name, l.a, l.u, l.n, t0) { if dbg { eprintln!("KPLAN build: run {name} at line {li} {t0:.2} does not fit"); } return Err(li) }
                if let Some(sk) = on[j0..i].iter().find(|q| q.kind == Kind::Sink) {
                    let m = (sk.t0 + sk.t1) / 2.0;
                    sink_on = Some((name.clone(), [l.a[0] + l.u[0] * m, l.a[1] + l.u[1] * m]));
                }
                continue;
            }
            let k = match p.kind { Kind::Fridge => fridge, Kind::Range => range, Kind::Dish => dish, _ => None };
            let Some(k) = k else { i += 1; continue };
            put(state, k);
            state.objs.get_mut(k).unwrap().tags.insert(planned.clone());
            if !place_along(state, k, l.a, l.u, l.n, p.t0 + 0.005) { if dbg { eprintln!("KPLAN build: {k} at line {li} {:.2} does not fit", p.t0); } return Err(li) }
            // a range or dishwasher in line with the counter front (they are shallower than the counter; a fridge stays on the wall)
            if matches!(p.kind, Kind::Range | Kind::Dish) {
                let fwd = (kit.d_counter - p.depth).max(0.0);
                if fwd > 0.005 {
                    let p0 = state.objs[k].pose;
                    let mut q = p0; q.translate([l.n[0] * fwd, l.n[1] * fwd, 0.0]);
                    state.set_pose(k, q); state.touch(k);
                    if !free_ok(state, k) { state.set_pose(k, p0); state.touch(k); }
                }
            }
            i += 1;
        }
    }
    // L joints: at each corner where the corner leg (the owner) and the other leg both end in counter, the pair is one unit -- the
    // corner leg's base leaves the corner square, the other leg's wall cabinet reaches on to the corner leg's
    let side = |state: &State, run: &str, q: [f64; 2]| -> f64 {
        let o = &state.objs[run];
        let d = [q[0] - o.pose.t[0], q[1] - o.pose.t[1]];
        let ly = o.pose.r[0][1] * d[0] + o.pose.r[1][1] * d[1];
        if ly < (o.geom.lo[1] + o.geom.hi[1]) / 2.0 { -1.0 } else { 1.0 }
    };
    for (ci, ends) in kit.corners.iter().enumerate() {
        let owner = plan.owner[ci];
        let Some(&(_, oe)) = ends.iter().find(|(l, _)| *l == owner) else { continue };
        let lo_ = &kit.lines[owner];
        let at_end = |p: &&Placed, l: &Line, e: usize, s0: f64, s1: f64| if e == 0 { p.t0 - s0 < 0.01 } else { s1 - p.t1 < 0.01 } && p.line == ps.iter().find(|q| std::ptr::eq(*q, *p)).map_or(usize::MAX, |q| q.line) && l.len > 0.0;
        let (os0, os1) = kit.span(plan, owner);
        let Some(op) = ps.iter().filter(|p| p.line == owner && is_counter(p.kind)).find(|p| at_end(p, lo_, oe, os0, os1)) else { continue };
        let Some(orun) = run_of.get(&(op.line, op.idx)).cloned() else { continue };
        for &(lj, je) in ends.iter().filter(|(l, _)| *l != owner) {
            let l = &kit.lines[lj];
            let (s0, s1) = kit.span(plan, lj);
            let Some(lp) = ps.iter().filter(|p| p.line == lj && is_counter(p.kind)).find(|p| at_end(p, l, je, s0, s1)) else { continue };
            let Some(lrun) = run_of.get(&(lp.line, lp.idx)).cloned() else { continue };
            let corner_pt = if oe == 0 { lo_.a } else { [lo_.a[0] + lo_.u[0] * lo_.len, lo_.a[1] + lo_.u[1] * lo_.len] };
            let t = if je == 0 { s0 } else { s1 };
            let joint_pt = [l.a[0] + l.u[0] * t, l.a[1] + l.u[1] * t];
            let (bs, js) = (side(state, &orun, corner_pt), side(state, &lrun, joint_pt));
            let leg_depth = { let (a, b) = lohi(state, &lrun); b[0] - a[0] };
            let own_depth = { let (a, b) = lohi(state, &orun); b[0] - a[0] };
            let set = |state: &mut State, k: &str, f: &dyn Fn(&mut [f64; 4])| { if let Some(g) = state.objs.get_mut(k).and_then(|o| o.generator.as_mut()) { let mut j = g.join.unwrap_or([0.0; 4]); f(&mut j); g.join = Some(j); } state.touch(k); };
            set(state, &orun, &|j| if j[0] == 0.0 { j[0] = bs; j[1] = leg_depth; } else if j[0] != bs { j[0] = 2.0; j[1] = j[1].max(leg_depth); });
            // (to the corner leg's wall-cabinet fronts: half the counter deep, less their doors' 3 cm)
            let reach = own_depth * 0.5 - 0.04;
            set(state, &lrun, &|j| if j[2] == 0.0 { j[2] = js; j[3] = reach; } else if j[2] != js { j[2] = 2.0; j[3] = j[3].min(reach); });
            if dbg { eprintln!("KPLAN L joint at corner {ci}: {orun} blind {bs:+} {leg_depth:.2} m, {lrun} reach {js:+} {reach:.2} m"); }
        }
    }
    // the sink: its run and spot, for the sinks pass (it builds sinks after the furniture stages, which rebuild placeholders)
    if let Some((run, at)) = sink_on { PLANNED_SINK.with(|m| { m.borrow_mut().insert(room.to_string(), (run, at)); }); }
    Ok(())
}
