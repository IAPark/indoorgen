//! Furniture groups placed by slot after the annealing stages (furn::layout2d checks every placement):
//!   * kitchen (after the large stage, before the built-in fill): the fridge flush at one end of the counter run, the dishwasher at the
//!     other, both turned like the run with their backs on its wall line; the run then grows up to them (solver::fit_builtins)
//!   * bedroom (after the medium stage): nightstands in the slots on both sides of the headboard, backs on the headboard line (one is
//!     enough beside a bed under 1.2 m); a missing one is a copy of the bed's other nightstand (a matching pair) or of any in the house
//!   * desk: a desk chair in front of every desk, facing it
//! A slot that collides, leaves the floor or breaks the walkway rule is skipped and the piece keeps its place.
use super::layout2d;
use super::solver::{sample_rand_placeholder, Ctx};
use super::state::*;
use super::tags::{Tag, TagSet};
use crate::mesh::V3;
use crate::rng::Rng;
use std::sync::Arc;

fn class_of(o: &ObjState) -> &str {
    o.generator.as_ref().map(|g| g.class.as_str()).unwrap_or("")
}

fn yaw(p: &super::geom::Pose) -> f64 {
    p.r[1][0].atan2(p.r[0][0])
}

/// collisions (2D prisms), the house floor and the layout rules, without the solver's relation tests
pub fn free_ok(state: &State, name: &str) -> bool {
    let nc = Tag::sem("NoCollision");
    let o = &state.objs[name];
    if o.tags.contains(nc) {
        return true;
    }
    // as the solver's own check (dof::check_post_move_validity): beds and counter runs may stand in front of windows
    // (ranges and dishwashers are counter height: under a window like the run they belong to)
    // anything lower than a sill (sofas, tables, stands) stands under a window too
    let top = layout2d::prisms(state, name).iter().map(|p| p.z[1]).fold(f64::NEG_INFINITY, f64::max);
    let skip_win = (top < 1.0 && class_of(o) != "TVStandFactory") || matches!(class_of(o), "BedFactory" | "KitchenSpaceFactory" | "OvenFactory" | "DishwasherFactory" | "SinkFactory");
    let win = Tag::sem("Window");
    for (k, b) in &state.objs {
        if k == name || b.tags.contains(nc) || layout2d::is_room(b) || (skip_win && b.tags.contains(win)) {
            continue;
        }
        // art and mirrors hang on the wall over furniture
        if matches!(class_of(b), "WallArtFactory" | "MirrorFactory") {
            continue;
        }
        // a piece's own children (items on it) are not obstacles
        if b.relations.iter().any(|r| r.target == name) {
            continue;
        }
        if layout2d::depth(state, name, k) > 1e-4 {
            return false;
        }
        // a TV stand clears windows in plan too (its screen would stand in front of the glass, above the sill the 3D test starts at)
        if class_of(o) == "TVStandFactory" && b.tags.contains(win.clone()) {
            let (pa, pb) = (layout2d::prisms(state, name), layout2d::prisms(state, k));
            if pa.iter().any(|x| pb.iter().any(|y| layout2d::poly_dist(&x.poly, &y.poly) < 0.05)) {
                return false;
            }
        }
    }
    let own = layout2d::room_of(state, name).map_or(0.0, |r| layout2d::outside_room_depth(state, name, &r));
    own <= 1e-3 && layout2d::outside_depth(state, name) <= 1e-4 && layout2d::layout_ok(state, name)
}

/// pose `name` turned like `yaw_` with its local point `anchor` at world point `at`; keeps it if free_ok, else restores. Returns ok.
fn try_place(state: &mut State, name: &str, yaw_: f64, anchor: V3, at: V3) -> bool {
    let old = state.objs[name].pose;
    let (s, c) = yaw_.sin_cos();
    let ra = [c * anchor[0] - s * anchor[1], s * anchor[0] + c * anchor[1], anchor[2]];
    // callers pass z = 0 for "on the floor": the piece keeps its height (floors stand above z = 0, by the slab)
    let tz = if at[2] == 0.0 { old.t[2] } else { at[2] - ra[2] };
    let p = super::geom::Pose::from_euler([at[0] - ra[0], at[1] - ra[1], tz], [0.0, 0.0, yaw_]);
    state.set_pose(name, p);
    state.touch(name);
    if free_ok(state, name) {
        return true;
    }
    if ig_core::env_on!("GROUPDBG") {
        let nc = Tag::sem("NoCollision");
        let hits: Vec<String> = state.objs.iter().filter(|(k, b)| k.as_str() != name && !b.tags.contains(nc) && !layout2d::is_room(b) && !b.relations.iter().any(|r| r.target == name) && layout2d::depth(state, name, k) > 1e-4).map(|(k, _)| k.clone()).collect();
        eprintln!("GROUPDBG {name}: hits {hits:?} outside {:.3} layout {}", layout2d::outside_depth(state, name), layout2d::layout_ok(state, name));
    }
    state.set_pose(name, old);
    state.touch(name);
    false
}

fn lohi(state: &State, name: &str) -> (V3, V3) {
    let g = &state.objs[name].geom;
    (g.lo, g.hi)
}

/// kitchens: fridge and dishwasher at the ends of the counter run
pub fn kitchen(ctx: &mut Ctx, state: &mut State) -> Vec<String> {
    let mut log = vec![];
    // designed kitchens first (an L from a corner: counter + fridge on one leg, counter + range on the other); the rooms it lays out
    // skip the per-run appliance placement below
    let done = kitchen_template(ctx, state, &mut log);
    let runs: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && !layout2d::room_of(state, k).map_or(false, |r| done.contains(&r))).map(|(k, _)| k.clone()).collect();
    // floor-standing kitchen cabinets box the counter run in and read as stray units: removed (wall cabinets stay)
    let _runs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "KitchenSpaceFactory").map(|(k, _)| k.clone()).collect();
    let lone: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenCabinetFactory" && layout2d::room_of(state, k).map_or(false, |r| r.starts_with("kitchen"))
        && layout2d::prisms(state, k).iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min) < 0.25
        ).map(|(k, _)| k.clone()).collect();
    for k in lone {
        layout2d::remove_tree(state, &k);
        log.push(format!("kitchen cabinet {k}: floor-standing, removed (the counter run takes the wall)"));
    }
    // the program stands counter runs 7 cm off their wall (StableAgainst Back, margin 0.07): flush against it instead
    for run in &runs {
        let o = state.objs[run].clone();
        let Some(ri) = o.relations.iter().position(|r| matches!(&r.relation, super::relations::Relation::Geometry(g) if g.child.contains(Tag::sub("Back")) && g.margin > 0.005)) else { continue };
        let super::relations::Relation::Geometry(g) = &o.relations[ri].relation else { continue };
        let m = g.margin;
        let mut g2 = g.clone();
        g2.margin = 0.0;
        let back = o.pose.rot([-1.0, 0.0, 0.0]);
        let oo = state.objs.get_mut(run).unwrap();
        oo.relations[ri].relation = super::relations::Relation::Geometry(g2);
        let mut p = oo.pose;
        p.translate([back[0] * m, back[1] * m, 0.0]);
        state.set_pose(run, p);
        state.touch(run);
        if super::dof::check_post_move_validity(state, run) {
            log.push(format!("counter run {run}: flush against its wall ({:.0} cm closer)", m * 100.0));
            continue;
        }
        // flush against a wall that meets its neighbours at an angle, the run's back corners hit those walls: shorter, from either end
        let flushed = state.objs[run].clone();
        let mut done = false;
        if let Some(cs) = flushed.generator.as_ref().and_then(super::solver::cur_size) {
            'w: for dw in [0.1f64, 0.2, 0.3, 0.4] {
                for side in [1.0f64, -1.0] {
                    if cs[0] - dw < 1.2 { break 'w }
                    if super::solver::set_width(ctx, state, run, &flushed, cs[0] - dw, side) {
                        log.push(format!("counter run {run}: flush against its wall, {:.0} cm shorter", dw * 100.0));
                        done = true;
                        break 'w;
                    }
                }
            }
        }
        if !done {
            *state.objs.get_mut(run).unwrap() = flushed.clone();
            state.touch(run);
            if ig_core::env_on!("GROUPDBG") {
                let hits: Vec<String> = state.objs.keys().filter(|k| k.as_str() != run && !layout2d::is_room(&state.objs[k.as_str()]) && layout2d::depth(state, run, k) > 1e-4).cloned().collect();
                eprintln!("GROUPDBG counter flush failed {run}: relations {} outside {:.3} hits {hits:?} layout {}", super::dof::all_relations_valid(state, run), layout2d::outside_depth(state, run), layout2d::layout_ok(state, run));
            }
            *state.objs.get_mut(run).unwrap() = o;
            state.touch(run);
        }
    }
    for run in runs {
        let Some(room) = layout2d::room_of(state, &run) else { continue };
        let ry = yaw(&state.objs[&run].pose);
        let (clo, chi) = lohi(state, &run);
        let rp = state.objs[&run].pose;
        let mut ends_used: Vec<f64> = vec![];
        // the range (oven) before the dishwasher: a kitchen needs a stove more than a dishwasher
        // the appliances leave the room while they are placed in turn: the solver's oven at a run end otherwise keeps the fridge off it
        let apps: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "FridgeFactory" | "OvenFactory" | "DishwasherFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        let mut parked: Vec<(String, Vec<(String, ObjState)>)> = vec![];
        for a in &apps {
            let mut tree = vec![(a.clone(), state.objs[a].clone())];
            for (c, co) in state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| &r.target == a)) { tree.push((c.clone(), co.clone())); }
            for (k, _) in &tree { state.objs.shift_remove(k); }
            parked.push((a.clone(), tree));
        }
        // with the appliances out, the run first fills its wall; the fridge and range then take their width from its ends
        if !apps.is_empty() {
            let _ = super::solver::fit_builtins(ctx, state, Some(room.as_str()));
        }
        let (clo, chi) = lohi(state, &run);
        let rp = state.objs[&run].pose;
        for cls in ["FridgeFactory", "OvenFactory", "DishwasherFactory"] {
            let pieces: Vec<String> = parked.iter().filter(|(_, t)| class_of(&t[0].1) == cls).map(|(k, _)| k.clone()).collect();
            for p in pieces {
                // back in, where the solver had it, then moved to the run
                if let Some(i) = parked.iter().position(|(k, _)| *k == p) {
                    let (_, tree) = parked.remove(i);
                    for (k, o) in tree { state.objs.insert(k.clone(), o); state.touch(&k); }
                }
                let (lo, hi) = lohi(state, &p);
                let mut done = false;
                for s in [1.0f64, -1.0] {
                    if ends_used.contains(&s) {
                        continue;
                    }
                    // back on the run's back line, 1 cm beside the run's end
                    let at = rp.apply([clo[0], if s > 0.0 { chi[1] + 0.01 } else { clo[1] - 0.01 }, clo[2]]);
                    let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
                    if try_place(state, &p, ry, anchor, [at[0], at[1], at[2]]) {
                        ends_used.push(s);
                        log.push(format!("{cls} {p} at the {} end of {run}", if s > 0.0 { "+y" } else { "-y" }));
                        done = true;
                        break;
                    }
                }
                // no free end: shorten the run at an end by the piece's width (+2 cm) and put the piece there (the built-in fill grows
                // the run's other end afterwards)
                // (in steps past the piece's width: a window over the run's end keeps a tall fridge off it)
                if !done {
                  for extra in [0.0f64, 0.3, 0.6, 0.9, 1.2] {
                    if done { break }
                    let pw = hi[1] - lo[1] + 0.02 + extra;
                    for s in [1.0f64, -1.0] {
                        if ends_used.contains(&s) { continue }
                        let base = state.objs[&run].clone();
                        let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) else { break };
                        // the run may shrink to 0.8 m to take the fridge / range in: one continuous run beats a counter with appliances apart
                        if cs[0] - pw < 1.5 { continue }
                        let saved_piece = state.objs[&p].clone();
                        if !super::solver::set_width(ctx, state, &run, &base, cs[0] - pw, s) {
                            *state.objs.get_mut(&run).unwrap() = base;
                            state.touch(&run);
                            continue;
                        }
                        let rp2 = state.objs[&run].pose;
                        let (clo2, chi2) = lohi(state, &run);
                        let at = rp2.apply([clo2[0], if s > 0.0 { chi2[1] + 0.01 } else { clo2[1] - 0.01 }, clo2[2]]);
                        let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
                        if try_place(state, &p, ry, anchor, at) {
                            ends_used.push(s);
                            log.push(format!("{cls} {p} at the {} end of {run} (run shortened by {pw:.2} m)", if s > 0.0 { "+y" } else { "-y" }));
                            done = true;
                            break;
                        }
                        *state.objs.get_mut(&run).unwrap() = base;
                        state.touch(&run);
                        *state.objs.get_mut(&p).unwrap() = saved_piece;
                        state.touch(&p);
                    }
                  }
                }
                if !done && (cls == "OvenFactory" || cls == "FridgeFactory") {
                    let run2 = run.clone();
                    if place_on_wall(state, &p, &room, &move |st: &State, c: [f64; 2], _t, _len, _n| {
                        // nearest the run's ends: round the corner from it, not across the room
                        let (l, h) = lohi(st, &run2);
                        let rp = st.objs[&run2].pose;
                        [rp.apply([(l[0] + h[0]) / 2.0, l[1], 0.0]), rp.apply([(l[0] + h[0]) / 2.0, h[1], 0.0])].iter()
                            .map(|e| ((c[0] - e[0]).powi(2) + (c[1] - e[1]).powi(2)).sqrt()).fold(f64::INFINITY, f64::min) * -1.0
                    }) {
                        log.push(format!("{cls} {p}: on the wall nearest {run}"));
                        done = true;
                    }
                }
                if !done && (cls == "OvenFactory" || cls == "DishwasherFactory") && state.objs[&p].relations.iter().all(|r| state.objs.get(&r.target).map_or(true, |t| t.generator.is_none())) {
                    // a free-standing oven that cannot join the run reads as a stove dropped in the middle of the kitchen
                    layout2d::remove_tree(state, &p);
                    log.push(format!("{cls} {p}: no free end at {run}, removed"));
                } else if !done && !free_ok(state, &p) {
                    // back where the solver had it but something placed meanwhile stands there
                    layout2d::remove_tree(state, &p);
                    log.push(format!("{cls} {p}: no free end at {run} and its spot taken, removed"));
                } else if !done {
                    log.push(format!("{cls} {p}: no free end at {run}"));
                }
            }
        }
    }
    // counters on every wall with room: more runs along the kitchen's free walls (an L or U with the first), each grown to its span;
    // a run facing another keeps a 1 m aisle (touching at a corner is fine)
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen").map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let mut rng = Rng::seeded(ig_core::int_hash(&["kitchen_runs", &room]));
        // an appliance standing alone mid-wall leaves no stretch long enough for a run: it slides into a corner of a wall first
        // (the new run then grows up to it, in line)
        let lone: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "FridgeFactory" | "OvenFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        for a in lone {
            let runs_now: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            if runs_now.iter().any(|r| layout2d::dist(state, r, &a) < 0.1) { continue }
            let start = state.objs[&a].pose;
            let (lo, hi) = lohi(state, &a);
            let hw = (hi[1] - lo[1]) / 2.0;
            if place_on_wall(state, &a, &room, &move |_, _c, t, len, n| -(t - hw).min(len - t - hw) - if n[0].abs() > 0.99 || n[1].abs() > 0.99 { 0.0 } else { 10.0 }) {
                log.push(format!("{room}: {a} slid into a corner"));
            } else {
                state.set_pose(&a, start);
                state.touch(&a);
            }
        }
        for i in 0..3 {
            let others: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            if others.is_empty() { break }
            let name = format!("{room}_run{i}").replace('/', "_");
            if !new_piece(ctx, state, &mut rng, "KitchenSpaceFactory", &room, &name) { break }
            // the first run's floor relation (same class: the same face order); its wall relation names that run's wall
            let floor_rel: Vec<_> = state.objs[&others[0]].relations.iter().filter(|r| r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))).cloned().collect();
            if !floor_rel.is_empty() { state.objs.get_mut(&name).unwrap().relations = floor_rel; state.touch(&name); }
            // the shortest run to find a wall; the built-in fill grows it to the span
            let base = state.objs[&name].clone();
            let _ = super::solver::set_width(ctx, state, &name, &base, 1.2, 1.0);
            let n2 = name.clone();
            let os = others.clone();
            let aisle_ok = move |st: &State| os.iter().all(|r| { let d = layout2d::dist(st, r, &n2); d < 0.05 || d >= 1.0 });
            let ok2 = aisle_ok.clone();
            // straight walls only (a run on an angled wall reads as a diagonal counter in the middle of the room)
            // (a run meeting another at a corner, an L or U, reads as one counter; parallel runs read as split: corners first)
            let (n3, os3) = (name.clone(), others.clone());
            let placed = place_on_wall(state, &name, &room, &move |st, _c, _t, len, n| {
                let joined = os3.iter().any(|r| layout2d::dist(st, r, &n3) < 0.05);
                len + if joined { 5.0 } else { 0.0 } - if ok2(st) && (n[0].abs() > 0.99 || n[1].abs() > 0.99) { 0.0 } else { 100.0 }
            });
            let ny = yaw(&state.objs[&name].pose);
            let straight = (ny.sin() * ny.cos()).abs() < 0.02;
            if !placed || !aisle_ok(state) || !straight {
                state.objs.shift_remove(&name);
                break;
            }
            let _ = super::solver::fit_builtins(ctx, state, Some(room.as_str()));
            let w = state.objs[&name].generator.as_ref().and_then(super::solver::cur_size).map_or(0.0, |c| c[0]);
            if w < 1.0 {
                state.objs.shift_remove(&name);
                break;
            }
            log.push(format!("{room}: another counter run {name} ({w:.2} m)"));
        }
        // a fridge or range left off every run (no free end on the first) goes to a free end of any run now
        let fridges: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "FridgeFactory" | "OvenFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        let runs: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        for f in fridges {
            // in line with a run (same facing, touching) it stands at its end already; touching one only sideways it does not
            let fy = yaw(&state.objs[&f].pose);
            if runs.iter().any(|r| layout2d::dist(state, r, &f) < 0.1 && (yaw(&state.objs[r].pose) - fy).sin().abs() < 0.05 && (yaw(&state.objs[r].pose) - fy).cos() > 0.0) { continue }
            let (lo, hi) = lohi(state, &f);
            'r: for run in &runs {
                let (clo, chi) = lohi(state, run);
                let rp = state.objs[run].pose;
                let ry = yaw(&rp);
                for s in [1.0f64, -1.0] {
                    let at = rp.apply([clo[0], if s > 0.0 { chi[1] + 0.01 } else { clo[1] - 0.01 }, 0.0]);
                    let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
                    if try_place(state, &f, ry, anchor, [at[0], at[1], 0.0]) {
                        log.push(format!("{room}: {f} moved to the end of {run}"));
                        break 'r;
                    }
                }
            }
        }
    }
    log
}

/// place `name` with its back on a wall line through `c` (inward normal `n`), covering [start, start + its width] along `u` from `c`
fn place_along(state: &mut State, name: &str, c: [f64; 2], u: [f64; 2], n: [f64; 2], start: f64) -> bool {
    let (lo, hi) = lohi(state, name);
    let yaw_ = n[1].atan2(n[0]);
    let ly = [-n[1], n[0]];
    let anchor = if ly[0] * u[0] + ly[1] * u[1] > 0.0 { [lo[0], lo[1], lo[2]] } else { [lo[0], hi[1], lo[2]] };
    let at = [c[0] + u[0] * start + n[0] * 0.003, c[1] + u[1] * start + n[1] * 0.003, 0.0];
    try_place(state, name, yaw_, anchor, at)
}

/// kitchens laid out from a template: an L at the corner of two straight walls -- leg A a counter run from the corner with the
/// fridge at its far end, leg B a run from the corner (past A's depth) with the range at its far end. Every corner and both leg
/// assignments are tried on a copy of the room's pieces and the layout with the most counter (fridge and range placed) kept.
/// The solver's runs, sinks and dishwashers in the room go; the extra-runs pass then lines the remaining walls (U / galley).
/// Returns the rooms laid out.
fn kitchen_template(ctx: &mut Ctx, state: &mut State, log: &mut Vec<String>) -> Vec<String> {
    let mut done = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen").map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let in_room = |st: &State, c: &str| -> Vec<String> { st.objs.iter().filter(|(k, o)| class_of(o) == c && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect() };
        let runs = in_room(state, "KitchenSpaceFactory");
        let (Some(proto), Some(fridge), Some(range)) = (runs.first().cloned(), in_room(state, "FridgeFactory").first().cloned(), in_room(state, "OvenFactory").first().cloned()) else { continue };
        let saved = state.objs.clone();
        // clear the room's counter pieces: other runs (with what stands on them), dishwashers, loose sinks, items on the prototype
        for r in runs.iter().skip(1) { layout2d::remove_tree(state, r); }
        for k in in_room(state, "DishwasherFactory") { layout2d::remove_tree(state, &k); }
        // wall cabinets too: the runs carry their own upper units
        for k in in_room(state, "KitchenCabinetFactory") { layout2d::remove_tree(state, &k); }
        let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == proto)).map(|(k, _)| k.clone()).collect();
        for k in kids { layout2d::remove_tree(state, &k); }
        // the prototype keeps only its floor relation (its wall relation names the solver's wall)
        let floor_rel: Vec<_> = state.objs[&proto].relations.iter().filter(|r| r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))).cloned().collect();
        state.objs.get_mut(&proto).unwrap().relations = floor_rel.clone();
        state.touch(&proto);
        let leg_b = format!("{room}_legb").replace('/', "_");
        let mut rng = Rng::seeded(ig_core::int_hash(&["kitchen_template", &room]));
        if !new_piece(ctx, state, &mut rng, "KitchenSpaceFactory", &room, &leg_b) {
            state.objs = saved;
            for k in state.objs.keys().cloned().collect::<Vec<_>>() { state.touch(&k); }
            continue;
        }
        state.objs.get_mut(&leg_b).unwrap().relations = floor_rel.clone();
        state.touch(&leg_b);
        let base_a = state.objs[&proto].clone();
        let base_b = state.objs[&leg_b].clone();
        let (flo, fhi) = lohi(state, &fridge);
        let fw = fhi[1] - flo[1];
        let (olo, ohi) = lohi(state, &range);
        let ow = ohi[1] - olo[1];
        let depth_a = { let (l, h) = lohi(state, &proto); h[0] - l[0] };
        // corners: two straight perpendicular wall segments sharing an end
        let segs: Vec<_> = wall_segments(state, &room).into_iter().filter(|(_, _, n)| n[0].abs() > 0.99 || n[1].abs() > 0.99).collect();
        let mut cands = vec![];
        for (i, (a0, a1, na)) in segs.iter().enumerate() {
            for (j, (b0, b1, nb)) in segs.iter().enumerate() {
                if i == j || (na[0] * nb[0] + na[1] * nb[1]).abs() > 0.05 { continue }
                for (ca, fa) in [(*a0, *a1), (*a1, *a0)] {
                    for (cb, fb) in [(*b0, *b1), (*b1, *b0)] {
                        if (ca[0] - cb[0]).hypot(ca[1] - cb[1]) > 0.05 { continue }
                        let la = (fa[0] - ca[0]).hypot(fa[1] - ca[1]);
                        let lb = (fb[0] - cb[0]).hypot(fb[1] - cb[1]);
                        let ua = [(fa[0] - ca[0]) / la, (fa[1] - ca[1]) / la];
                        let ub = [(fb[0] - cb[0]) / lb, (fb[1] - cb[1]) / lb];
                        cands.push((ca, ua, *na, la, ub, *nb, lb));
                    }
                }
            }
        }
        let start_state = state.objs.clone();
        let mut best: Option<(f64, _)> = None;
        for (c, ua, na, la, ub, nb, lb) in cands {
            state.objs = start_state.clone();
            // the template's pieces out of the room until each is placed: the solver's spots block each other (parked far off they
            // still count as the room's pieces for the walkway check)
            let parked: std::collections::HashMap<String, ObjState> = [&proto, &leg_b, &fridge, &range].into_iter().map(|k| (k.clone(), state.objs.shift_remove(k.as_str()).unwrap())).collect();
            let put = |state: &mut State, k: &str| { if !state.objs.contains_key(k) { state.objs.insert(k.to_string(), parked[k].clone()); state.touch(k); } };
            put(state, &fridge);
            // leg A: the fridge as far along as it fits, the run from the corner up to it
            let mut sf = None;
            for k in 0..40 {
                let st = la - fw - 0.02 - k as f64 * 0.2;
                if st < 1.2 { break }
                if place_along(state, &fridge, c, ua, na, st) { sf = Some(st); break }
            }
            let dbg = ig_core::env_on!("GROUPDBG");
            let Some(sf) = sf else { if dbg { eprintln!("KTPL {room} corner {c:?} la {la:.2} lb {lb:.2}: no fridge spot"); } continue };
            let mut wa = 0.0;
            put(state, &proto);
            for k in 0..8 {
                let w = sf - 0.02 - k as f64 * 0.2;
                if w < 1.2 { break }
                *state.objs.get_mut(&proto).unwrap() = base_a.clone();
                state.touch(&proto);
                if !super::solver::set_width(ctx, state, &proto, &base_a, w, 1.0) && state.objs[&proto].generator.as_ref().and_then(super::solver::cur_size).map_or(true, |cs| (cs[0] - w).abs() > 0.05) { continue }
                if place_along(state, &proto, c, ua, na, 0.0) { wa = w; break }
            }
            if wa == 0.0 { if dbg { eprintln!("KTPL {room} corner {c:?}: no run A (fridge at {sf:.2})"); } continue }
            // leg B: past leg A's depth, the range as far along as it fits, the run up to it (none when the leg is short)
            let mut sr = None;
            put(state, &range);
            for k in 0..40 {
                let st = lb - ow - 0.02 - k as f64 * 0.2;
                if st < depth_a + 0.01 { break }
                if place_along(state, &range, c, ub, nb, st) { sr = Some(st); break }
            }
            let Some(sr) = sr else { if dbg { eprintln!("KTPL {room} corner {c:?} lb {lb:.2}: no range spot"); } continue };
            let mut wb = 0.0;
            put(state, &leg_b);
            for k in 0..8 {
                let w = sr - depth_a - 0.03 - k as f64 * 0.2;
                if w < 0.6 { break }
                *state.objs.get_mut(&leg_b).unwrap() = base_b.clone();
                state.touch(&leg_b);
                let _ = super::solver::set_width(ctx, state, &leg_b, &base_b, w, 1.0);
                if place_along(state, &leg_b, c, ub, nb, depth_a + 0.01) { wb = w; break }
            }
            if wb == 0.0 { state.objs.shift_remove(&leg_b); }
            let score = wa + wb + if wb > 0.0 { 1.0 } else { 0.0 };
            if best.as_ref().map_or(true, |(s, _)| score > *s) { best = Some((score, state.objs.clone())); }
        }
        match best {
            Some((sc, objs)) => {
                state.objs = objs;
                for k in [&proto, &leg_b, &fridge, &range] { if state.objs.contains_key(k.as_str()) { state.touch(k); } }
                log.push(format!("{room}: kitchen laid out as an L ({sc:.1} m of counter)"));
                done.push(room.clone());
            }
            None => {
                state.objs = saved;
                for k in state.objs.keys().cloned().collect::<Vec<_>>() { state.touch(&k); }
                log.push(format!("{room}: no corner takes the L template, solver layout kept"));
            }
        }
    }
    done
}

/// bathrooms laid out from a template: the tub (or shower) along one wall from a corner, then the sink and toilet along the
/// perpendicular wall past it (or the toilet along the tub's wall after it), each 0.2 m apart; every corner is tried on a copy and
/// the layout with the toilet farthest from the door (and nothing within 0.6 m of it) kept. Returns log lines.
pub fn bathroom_template(state: &mut State, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "bathroom" && only_room.map_or(true, |r| r == k.as_str())).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let first = |st: &State, c: &str| st.objs.iter().find(|(k, o)| class_of(o) == c && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone());
        let Some(wc) = first(state, "ToiletFactory") else { continue };
        let Some(sink) = first(state, "StandingSinkFactory") else { continue };
        let Some(tub) = first(state, "BathtubFactory").or_else(|| first(state, "ShowerFactory")) else { continue };
        let doors = door_points(state, &room);
        let door_d = |st: &State, k: &str| { let t = st.objs[k].pose.t; doors.iter().map(|d| ((d[0] - t[0]).powi(2) + (d[1] - t[1]).powi(2)).sqrt()).fold(9.0f64, f64::min) };
        let w_of = |st: &State, k: &str| { let (l, h) = lohi(st, k); (h[1] - l[1], h[0] - l[0]) };
        let (tw, td) = w_of(state, &tub);
        let (sw, _) = w_of(state, &sink);
        let (ww, _) = w_of(state, &wc);
        let segs: Vec<_> = wall_segments(state, &room).into_iter().filter(|(_, _, n)| n[0].abs() > 0.99 || n[1].abs() > 0.99).collect();
        let start_state = state.objs.clone();
        let mut best: Option<(f64, _)> = None;
        for (i, (a0, a1, na)) in segs.iter().enumerate() {
            for (j, (b0, b1, nb)) in segs.iter().enumerate() {
                if i == j || (na[0] * nb[0] + na[1] * nb[1]).abs() > 0.05 { continue }
                for (c, fa) in [(*a0, *a1), (*a1, *a0)] {
                    for fb in [*b0, *b1] {
                        let cb = if fb == *b0 { *b1 } else { *b0 };
                        if (c[0] - cb[0]).hypot(c[1] - cb[1]) > 0.05 { continue }
                        let la = (fa[0] - c[0]).hypot(fa[1] - c[1]);
                        let lb = (fb[0] - c[0]).hypot(fb[1] - c[1]);
                        if la < tw + 0.01 { continue }
                        let ua = [(fa[0] - c[0]) / la, (fa[1] - c[1]) / la];
                        let ub = [(fb[0] - c[0]) / lb, (fb[1] - c[1]) / lb];
                        let (na, nb) = (*na, *nb);
                        // variants: (sink, wc) along B past the tub; wc along A after the tub and the sink along B
                        for variant in 0..2 {
                            state.objs = start_state.clone();
                            let parked: std::collections::HashMap<String, ObjState> = [&tub, &sink, &wc].into_iter().map(|k| (k.clone(), state.objs.shift_remove(k.as_str()).unwrap())).collect();
                            let put = |state: &mut State, k: &str| { state.objs.insert(k.to_string(), parked[k].clone()); state.touch(k); };
                            put(state, &tub);
                            if !place_along(state, &tub, c, ua, na, 0.0) { continue }
                            let mut ok = true;
                            put(state, &sink);
                            put(state, &wc);
                            if variant == 0 {
                                let mut placed = false;
                                for k in 0..10 {
                                    let s0 = td + 0.15 + k as f64 * 0.15;
                                    if s0 + sw + 0.35 + ww > lb { break }
                                    if place_along(state, &sink, c, ub, nb, s0) && place_along(state, &wc, c, ub, nb, s0 + sw + 0.35) { placed = true; break }
                                }
                                ok &= placed;
                            } else {
                                let mut placed = false;
                                for k in 0..10 {
                                    let s0 = tw + 0.35 + k as f64 * 0.15;
                                    if s0 + ww > la { break }
                                    if place_along(state, &wc, c, ua, na, s0) { placed = true; break }
                                }
                                let mut placed2 = false;
                                for k in 0..10 {
                                    let s0 = td + 0.2 + k as f64 * 0.15;
                                    if s0 + sw > lb { break }
                                    if place_along(state, &sink, c, ub, nb, s0) { placed2 = true; break }
                                }
                                ok &= placed && placed2;
                            }
                            if !ok { continue }
                            // (toilet and sink on different walls read as less crammed)
                            let score = door_d(state, &wc).min(2.5) + if door_d(state, &wc) > door_d(state, &sink) { 0.5 } else { 0.0 } - if door_d(state, &sink) < 0.8 { 1.0 } else { 0.0 } + if variant == 1 { 0.4 } else { 0.0 };
                            if best.as_ref().map_or(true, |(s, _)| score > *s) { best = Some((score, state.objs.clone())); }
                        }
                    }
                }
            }
        }
        match best {
            Some((sc, objs)) => {
                state.objs = objs;
                for k in [&tub, &sink, &wc] { state.touch(k); }
                log.push(format!("{room}: bathroom laid out on a wet wall (toilet {sc:.1})"));
            }
            None => {
                state.objs = start_state;
                for k in state.objs.keys().cloned().collect::<Vec<_>>() { state.touch(&k); }
                log.push(format!("{room}: no corner takes the bathroom template"));
            }
        }
    }
    log
}

/// a new piece of `class` standing in `room`: the solver's placeholder for the class (size from the class density), its usage tags
/// and a floor relation copied from a piece standing in that room; inserted at the origin (the caller places it)
fn new_piece(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, class: &str, room: &str, name: &str) -> bool {
    let Some((geom, gen)) = sample_rand_placeholder(ctx, rng, class) else { return false };
    let mut tags = TagSet::from([Tag::sem("Object"), Tag::sem("Furniture"), Tag::gen(class), Tag::obj(name)]);
    tags.update(&ctx.usage.usages_of(class));
    let mut o = ObjState::new(Arc::new(geom), super::geom::Pose::identity(), tags);
    o.generator = Some(gen);
    if let Some((ro, r)) = state.objs.values().flat_map(|x| x.relations.iter().map(move |r| (x, r))).find(|(_, r)| r.target == room && r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))) {
        o.relations.push(r.clone());
        // standing at that piece's floor height (its bottom)
        let z = ro.pose.t[2] + ro.geom.lo[2] - o.geom.lo[2];
        let t = o.pose.t; o.pose.set_loc([t[0], t[1], z]);
    }
    o.active = true;
    state.objs.insert(name.to_string(), o);
    state.touch(name);
    // the copied relation's child face index belongs to the other piece's geometry: re-matched to this one's downward face
    if let Some(g) = state.objs[name].relations.first().and_then(|r| r.relation.geo().cloned()) {
        let pl = state.planes(name, &g.child);
        let best = (0..pl.len()).min_by(|&i, &j| state.plane_nd(name, pl[i]).0[2].partial_cmp(&state.plane_nd(name, pl[j]).0[2]).unwrap());
        if let Some(bi) = best { state.objs.get_mut(name).unwrap().relations[0].child_plane = bi; }
        state.touch(name);
    }
    true
}

/// a copy of object `src` under a new name, child of `parent` (relations to the old parent retargeted)
fn clone_obj(state: &mut State, src: &str, new: &str, old_parent: Option<&str>, parent: &str) {
    let mut o = state.objs[src].clone();
    for r in o.relations.iter_mut() {
        if Some(r.target.as_str()) == old_parent {
            r.target = parent.to_string();
        }
    }
    o.tags = TagSet(o.tags.0.iter().filter(|t| t.kind() != super::tags::Kind::Obj).cloned().collect());
    o.tags.insert(Tag::obj(new));
    state.objs.insert(new.to_string(), o);
    state.touch(new);
}

/// bedrooms: nightstands beside the headboard; desks: a chair in front
pub fn bedroom_and_desk(ctx: &mut Ctx, state: &mut State, rng: &mut Rng) -> Vec<String> {
    let mut log = vec![];
    let beds: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "BedFactory").map(|(k, _)| k.clone()).collect();
    for bed in beds {
        let (blo, bhi) = lohi(state, &bed);
        let bp = state.objs[&bed].pose;
        let by = yaw(&bp);
        let mut stands: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "SideTableFactory" && o.relations.iter().any(|r| r.target == bed)).map(|(k, _)| k.clone()).collect();
        // all of this bed's nightstands out of the room while the slots are filled (one standing in the other's slot blocked it);
        // each goes back in as its slot is tried
        let lifted: Vec<(String, ObjState)> = stands.iter().map(|k| (k.clone(), state.objs[k].clone())).collect();
        for k in &stands { state.objs.shift_remove(k); }
        let want = if bhi[1] - blo[1] < 1.2 { 1 } else { 2 };
        // a second nightstand: a copy of this bed's (a pair) or of any nightstand in the house
        let template = stands.first().cloned().or_else(|| state.objs.iter().find(|(_, o)| class_of(o) == "SideTableFactory").map(|(k, _)| k.clone()));
        let mut placed = 0;
        let mut failed: Vec<String> = vec![];
        for s in [1.0f64, -1.0] {
            if placed >= want {
                break;
            }
            let name = match stands.pop() {
                Some(n) => {
                    if let Some((_, o)) = lifted.iter().find(|(k, _)| *k == n) { state.objs.insert(n.clone(), o.clone()); state.touch(&n); }
                    n
                }
                None if template.is_none() => {
                    let Some(room) = layout2d::room_of(state, &bed) else { break };
                    let new = format!("{bed}_nightstand{}", if s > 0.0 { "a" } else { "b" });
                    if state.objs.contains_key(&new) || !new_piece(ctx, state, rng, "SideTableFactory", &room, &new) { break }
                    // the bed is its parent (dress / decor passes look for nightstands by their bed)
                    if let Some(r0) = state.objs[&new].relations.first().cloned() { let mut r = r0; r.target = bed.clone(); state.objs.get_mut(&new).unwrap().relations.push(r); }
                    new
                }
                None => {
                    let Some(t) = &template else { break };
                    let new = format!("{}_pair{}", t, if s > 0.0 { "a" } else { "b" });
                    if state.objs.contains_key(&new) { break }
                    let old_parent = state.objs[t].relations.iter().map(|r| r.target.clone()).find(|x| state.objs.get(x).map_or(false, |o| class_of(o) == "BedFactory"));
                    clone_obj(state, t, &new, old_parent.as_deref(), &bed);
                    new
                }
            };
            let (lo, hi) = lohi(state, &name);
            let at = bp.apply([blo[0], if s > 0.0 { bhi[1] + 0.04 } else { blo[1] - 0.04 }, blo[2]]);
            let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
            if try_place(state, &name, by, anchor, at) {
                placed += 1;
                log.push(format!("nightstand {name} on the {} side of {bed}", if s > 0.0 { "+y" } else { "-y" }));
            } else if name.contains("_pair") || name.contains("_nightstand") {
                state.objs.shift_remove(&name);
            } else {
                failed.push(name);
            }
        }
        // leftover nightstands of this bed found no slot by its headboard: removed (left where they were they stand by the foot)
        for k in stands.into_iter().chain(failed) {
            if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); }
            log.push(format!("nightstand {k}: no slot by the headboard, removed"));
        }
    }
    // a desk jammed against the bed or its nightstands reads as crammed in: removed
    let desks: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "SimpleDeskFactory").map(|(k, _)| k.clone()).collect();
    for desk in &desks {
        let tight = state.objs.iter().any(|(k, o)| (class_of(o) == "BedFactory" && layout2d::dist(state, k, desk) < 0.6)
            || (class_of(o) == "SideTableFactory" && layout2d::dist(state, k, desk) < 0.3));
        if tight {
            layout2d::remove_tree(state, desk);
            log.push(format!("desk {desk}: crammed against the bed, removed"));
        }
    }
    // desk chairs
    let desks: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "SimpleDeskFactory").map(|(k, _)| k.clone()).collect();
    for desk in desks {
        let near = state.objs.iter().any(|(k, o)| class_of(o).contains("Chair") && k != &desk && layout2d::dist(state, k, &desk) < 0.35);
        if near {
            continue;
        }
        let Some((geom, gen)) = sample_rand_placeholder(ctx, rng, "OfficeChairFactory") else { continue };
        let name = format!("{}_deskchair", desk);
        let mut tags = TagSet::from([Tag::sem("Object"), Tag::sem("Furniture"), Tag::gen("OfficeChairFactory"), Tag::obj(&name)]);
        tags.update(&ctx.usage.usages_of("OfficeChairFactory"));
        let mut o = ObjState::new(Arc::new(geom), super::geom::Pose::identity(), tags);
        o.generator = Some(gen);
        // floor relation copied from a piece standing in the desk's room
        if let Some(room) = layout2d::room_of(state, &desk) {
            if let Some(r) = state.objs.values().flat_map(|x| x.relations.iter()).find(|r| r.target == room && r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))) {
                o.relations.push(r.clone());
            }
        }
        o.active = true;
        state.objs.insert(name.clone(), o);
        state.touch(&name);
        let (dlo, dhi) = lohi(state, &desk);
        let dp = state.objs[&desk].pose;
        let (lo, hi) = lohi(state, &name);
        // chair front (local +x) toward the desk: turned half a turn from the desk, its front face 5 cm before the desk front
        let at = dp.apply([dhi[0] + 0.05, (dlo[1] + dhi[1]) / 2.0, dlo[2]]);
        let anchor = [hi[0], (lo[1] + hi[1]) / 2.0, lo[2]];
        if try_place(state, &name, yaw(&dp) + std::f64::consts::PI, anchor, at) {
            log.push(format!("desk chair {name} at {desk}"));
        } else {
            state.objs.shift_remove(&name);
            layout2d::remove_tree(state, &desk);
            log.push(format!("desk {desk}: no room for a chair, removed"));
        }
    }
    log
}

fn room_type_of(room: &str) -> &str {
    room.split('_').next().unwrap_or("")
}

/// the most pieces of a class (or class group) a room keeps, whatever relation the solver used (the program's own counts are per
/// relation: plants placed as free floor pieces escaped the against-wall caps); None = no cap
fn cap(room_type: &str, cls: &str) -> Option<(&'static str, usize)> {
    let storage = ["SingleCabinetFactory", "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory"].contains(&cls) || (cls == "KitchenCabinetFactory" && room_type != "kitchen");
    Some(match cls {
        "LargePlantContainerFactory" => ("plants", 0),
        "FloorLampFactory" => ("floor lamps", 0),
        "BeverageFridgeFactory" => ("beverage fridges", 0),
        "FridgeFactory" => ("fridges", if room_type == "kitchen" { 1 } else { 0 }),
        "OvenFactory" => ("ovens", if room_type == "kitchen" { 1 } else { 0 }),
        "DishwasherFactory" => ("dishwashers", if room_type == "kitchen" { 1 } else { 0 }),
        // (desks read as crammed beside the bed even in large bedrooms: none)
        "SimpleDeskFactory" => ("desks", 0),
        "KitchenCabinetFactory" if room_type == "kitchen" => return None,
        // (a lone cabinet on a living-room wall reads as stray: the TV stand is the room's storage)
        _ if storage => ("storage", match room_type { "utility" => 2, _ => 0 }),
        // a closet is fitted with a wardrobe (missing_pieces), not loose storage; small bathrooms keep a tub or a shower, not both
        "BathtubFactory" | "ShowerFactory" if room_type == "bathroom" => ("tub / shower", 2),
        _ => return None,
    })
}

/// per-room hard caps (see `cap`): extras go, the ones standing farthest from a wall first
pub fn caps(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let mut groups: std::collections::BTreeMap<(String, &'static str), (usize, Vec<String>)> = Default::default();
    for (k, o) in &state.objs {
        if o.generator.is_none() || layout2d::is_room(o) {
            continue;
        }
        // floor / wall pieces only (not items on furniture)
        if o.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())) {
            continue;
        }
        let Some(room) = layout2d::room_of(state, k) else { continue };
        let Some((grp, mut max)) = cap(room_type_of(&room), class_of(o)) else { continue };
        let area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        // a desk only in a bedroom of 18 m2 or more; a tub and a shower together only in a bathroom of 14 m2 or more
        if grp == "desks" && area < 18.0 { max = 0; }
        if grp == "tub / shower" && area < 14.0 { max = 1; }
        let e = groups.entry((room, grp)).or_insert((max, vec![]));
        e.1.push(k.clone());
    }
    for ((room, grp), (max, mut keys)) in groups {
        if keys.len() <= max {
            continue;
        }
        let rings = layout2d::rings(&layout2d::room_floor(state, &room));
        let wall_gap = |k: &str| -> f64 {
            layout2d::prisms(state, k).iter().flat_map(|p| p.poly.iter().map(|v| layout2d::ring_dist(&rings, *v))).fold(f64::INFINITY, f64::min)
        };
        keys.sort_by(|a, b| wall_gap(a).partial_cmp(&wall_gap(b)).unwrap());
        // a bathroom of 7 m2 or more keeps its tub over a shower (a bath without a tub read as under-equipped)
        let area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        if grp == "tub / shower" && area >= 7.0 {
            keys.sort_by_key(|k| class_of(&state.objs[k.as_str()]) != "BathtubFactory");
        }
        for k in keys.split_off(max) {
            let cls = class_of(&state.objs[&k]).to_string();
            layout2d::remove_tree(state, &k);
            log.push(format!("{room}: removed {cls} (over the {max} {grp} cap)"));
        }
    }
    log
}

/// inner wall segments of a room's floor: (start, end, inward normal), longest first
fn wall_segments(state: &State, room: &str) -> Vec<([f64; 2], [f64; 2], [f64; 2])> {
    let floor = layout2d::room_floor(state, room);
    let rings = layout2d::rings(&floor);
    let mut out = vec![];
    for r in &rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt();
            if l < 0.6 {
                continue;
            }
            let mut n = [-dy / l, dx / l];
            let mid = [(a[0] + b[0]) / 2.0 + n[0] * 0.05, (a[1] + b[1]) / 2.0 + n[1] * 0.05];
            if !layout2d::in_rings(&rings, mid) {
                n = [-n[0], -n[1]];
            }
            out.push((a, b, n));
        }
    }
    out.sort_by(|x, y| {
        let l = |s: &([f64; 2], [f64; 2], [f64; 2])| ((s.1[0] - s.0[0]).powi(2) + (s.1[1] - s.0[1]).powi(2)).sqrt();
        l(y).partial_cmp(&l(x)).unwrap()
    });
    out
}

fn door_points(state: &State, room: &str) -> Vec<[f64; 2]> {
    state.objs.keys().filter(|k| k.starts_with(&format!("doorclear/{room}/"))).filter_map(|k| {
        let p = layout2d::prisms(state, k);
        p.first().map(|p| { let n = p.poly.len() as f64; [p.poly.iter().map(|v| v[0]).sum::<f64>() / n, p.poly.iter().map(|v| v[1]).sum::<f64>() / n] })
    }).collect()
}

/// beds: headboard on the wall spot that scores best (valid placement; away from the doors; room for a nightstand on each side;
/// centred on its wall); the bed keeps its place when no wall spot is valid
fn place_bed(state: &mut State, bed: &str) -> Option<String> {
    let room = layout2d::room_of(state, bed)?;
    let (lo, hi) = lohi(state, bed);
    let w = hi[1] - lo[1];
    let doors = door_points(state, &room);
    let start = state.objs[bed].pose;
    let mut best: Option<(f64, super::geom::Pose)> = None;
    for (a, b, n) in wall_segments(state, &room) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < w + 0.1 {
            continue;
        }
        let u = [dx / len, dy / len];
        let yaw_ = n[1].atan2(n[0]);
        let steps = (((len - w) / 0.2).floor() as usize).max(0);
        for i in 0..=steps {
            let t = w / 2.0 + 0.02 + (len - w - 0.04) * if steps == 0 { 0.5 } else { i as f64 / steps as f64 };
            let at = [a[0] + u[0] * t + n[0] * 0.003, a[1] + u[1] * t + n[1] * 0.003, 0.0];
            if !try_place(state, bed, yaw_, [lo[0], (lo[1] + hi[1]) / 2.0, lo[2]], at) {
                continue;
            }
            let c = [at[0] + n[0] * (hi[0] - lo[0]) / 2.0, at[1] + n[1] * (hi[0] - lo[0]) / 2.0];
            let door_d = doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(3.0f64, f64::min);
            // nightstand slots actually free: a 0.5 x 0.45 m footprint on the wall beside the headboard, each side
            let side_room = [1.0f64, -1.0].iter().filter(|s| {
                let s = **s;
                let e0 = t + s * (w / 2.0 + 0.04);
                let e1 = e0 + s * 0.5;
                let q = |tt: f64, dd: f64| [a[0] + u[0] * tt + n[0] * dd, a[1] + u[1] * tt + n[1] * dd];
                slot_free(state, &room, bed, &[q(e0, 0.01), q(e1, 0.01), q(e1, 0.46), q(e0, 0.46)])
            }).count() as f64;
            let centred = 1.0 - ((t - len / 2.0).abs() / (len / 2.0)).min(1.0);
            // a bed square to the room: walls at an angle only when no straight wall works
            let straight = (n[0].abs() < 1e-3 || n[1].abs() < 1e-3) as i32 as f64;
            // (both slots free weighs most: a bed with one nightstand read as unfinished)
            let score = door_d.min(3.0) + side_room * 1.5 + if side_room >= 2.0 { 2.0 } else { 0.0 } + centred * 0.6 + straight * 2.0;
            if best.as_ref().map_or(true, |(s, _)| score > *s) {
                best = Some((score, state.objs[bed].pose));
            }
        }
    }
    match best {
        Some((s, p)) => {
            state.set_pose(bed, p);
            state.touch(bed);
            Some(format!("bed {bed}: headboard on a wall (score {s:.2})"))
        }
        None => {
            state.set_pose(bed, start);
            state.touch(bed);
            None
        }
    }
}

/// living rooms: the sofa facing the TV stand at a viewing distance (2-4 m, ideally about 2.6 m, or with its back on the wall
/// opposite the TV), centred on the TV
pub fn living(ctx: &mut Ctx, state: &mut State, rng: &mut Rng) -> Vec<String> {
    let mut log = vec![];
    let tvs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "TVStandFactory").map(|(k, _)| k.clone()).collect();
    for tv in tvs {
        let Some(room) = layout2d::room_of(state, &tv) else { continue };
        let Some(sofa) = state.objs.iter().find(|(k, o)| class_of(o) == "SofaFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()) else { continue };
        let tp = state.objs[&tv].pose;
        let (tlo, thi) = lohi(state, &tv);
        let ty = yaw(&tp);
        let f = [ty.cos(), ty.sin()];
        let front = tp.apply([thi[0], (tlo[1] + thi[1]) / 2.0, 0.0]);
        let (slo, shi) = lohi(state, &sofa);
        let start = state.objs[&sofa].pose;
        let rings = layout2d::rings(&layout2d::room_floor(state, &room));
        // the coffee table and armchairs are placed around the sofa afterwards: out of the room while the sofa looks for its spot
        let mut lifted: Vec<(String, ObjState)> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "CoffeeTableFactory" | "ArmChairFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str()))
            .map(|(k, o)| (k.clone(), o.clone())).collect();
        // a living room of 20 m2 or more without an armchair gets one (placed with the others beside the coffee table, or left out)
        let room_area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        // (30 m2 or more: a pair, one each side of the coffee table, so a large room does not read as half empty)
        let want = if room_area >= 30.0 { 2 } else if room_area >= 20.0 { 1 } else { 0 };
        for i in lifted.iter().filter(|(_, o)| class_of(o) == "ArmChairFactory").count()..want {
            let an = format!("{room}_armchair{i}").replace('/', "_");
            if new_piece(ctx, state, rng, "ArmChairFactory", &room, &an) {
                let o = state.objs.shift_remove(&an).unwrap();
                lifted.push((an, o));
            }
        }
        let mut kids: Vec<(String, ObjState)> = vec![];
        for (k, _) in &lifted {
            for (c, co) in state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| &r.target == k)) { kids.push((c.clone(), co.clone())); }
        }
        for (k, _) in lifted.iter().chain(kids.iter()) { state.objs.shift_remove(k); }
        let mut best: Option<(f64, super::geom::Pose, f64)> = None;
        // first choice: the sofa's back on a wall and the TV stand on the opposite (parallel) wall facing it, 1.8-4.2 m apart
        let tv_start = state.objs[&tv].pose;
        let mut pair: Option<(f64, super::geom::Pose, super::geom::Pose, f64)> = None;
        let segs = wall_segments(state, &room);
        let ldoors = door_points(state, &room);
        // the floor's area centroid (a vertex mean leans toward the short leg of an L-shaped room)
        let fc = {
            let r = &rings[0];
            let (mut a, mut cx, mut cy) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..r.len() {
                let (p, q) = (r[i], r[(i + 1) % r.len()]);
                let cr = p[0] * q[1] - q[0] * p[1];
                a += cr; cx += (p[0] + q[0]) * cr; cy += (p[1] + q[1]) * cr;
            }
            if a.abs() < 1e-9 { let n = r.len() as f64; [r.iter().map(|p| p[0]).sum::<f64>() / n, r.iter().map(|p| p[1]).sum::<f64>() / n] } else { [cx / (3.0 * a), cy / (3.0 * a)] }
        };
        let sw = shi[1] - slo[1];
        let sdep = shi[0] - slo[0];
        for (a, b, n) in segs.clone() {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len < sw + 0.1 { continue }
            let u = [dx / len, dy / len];
            let steps = ((len - sw) / 0.25).floor() as usize;
            for i in 0..=steps {
                let tt = sw / 2.0 + 0.02 + (len - sw - 0.04) * if steps == 0 { 0.5 } else { i as f64 / steps as f64 };
                let base = [a[0] + u[0] * tt, a[1] + u[1] * tt];
                // the opposite wall: the nearest parallel wall segment the ray from the sofa's centre line meets
                let mut hit: Option<(f64, [f64; 2], [f64; 2])> = None;
                for (c0, c1, m) in &segs {
                    if m[0] * n[0] + m[1] * n[1] > -0.98 { continue }
                    let (ex, ey) = (c1[0] - c0[0], c1[1] - c0[1]);
                    let den = n[0] * ey - n[1] * ex;
                    if den.abs() < 1e-9 { continue }
                    let tr = ((c0[0] - base[0]) * ey - (c0[1] - base[1]) * ex) / den;
                    let s2 = ((c0[0] - base[0]) * n[1] - (c0[1] - base[1]) * n[0]) / den;
                    if tr > 0.5 && (0.0..=1.0).contains(&s2) && hit.as_ref().map_or(true, |h| tr < h.0) { hit = Some((tr, [base[0] + n[0] * tr, base[1] + n[1] * tr], *m)); }
                }
                let Some((dist, hp, m)) = hit else { continue };
                let view = dist - sdep - (thi[0] - tlo[0]);
                // narrow living rooms (2.6 m) seat the sofa across from the TV at 1.3 m
                // (raters read more than ~4 m between sofa and TV as "far"; past that the sofa faces the TV from mid-room)
                if !(1.3..=4.0).contains(&view) { continue }
                if !try_place(state, &sofa, n[1].atan2(n[0]), [slo[0], (slo[1] + shi[1]) / 2.0, slo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]) { continue }
                if !try_place(state, &tv, m[1].atan2(m[0]), [tlo[0], (tlo[1] + thi[1]) / 2.0, tlo[2]], [hp[0] + m[0] * 0.003, hp[1] + m[1] * 0.003, 0.0]) {
                    state.set_pose(&tv, tv_start); state.touch(&tv);
                    continue;
                }
                // the group in the room, not along its edge: midpoint of sofa and TV near the floor's centre
                let mid = [(base[0] + hp[0]) / 2.0, (base[1] + hp[1]) / 2.0];
                // a TV beside a door (the entrance, a room door) reads as misplaced
                let tv_door = ldoors.iter().map(|d| ((d[0] - hp[0]).powi(2) + (d[1] - hp[1]).powi(2)).sqrt()).fold(9.0f64, f64::min);
                let score = -(view - 2.6).abs() - if view < 1.8 { 1.5 } else { 0.0 } - (1.6 - tv_door).max(0.0) * 1.5 - (tt - len / 2.0).abs() / len - ((mid[0] - fc[0]).powi(2) + (mid[1] - fc[1]).powi(2)).sqrt() * 0.6;
                if pair.as_ref().map_or(true, |p| score > p.0) { pair = Some((score, state.objs[&sofa].pose, state.objs[&tv].pose, view)); }
                state.set_pose(&tv, tv_start); state.touch(&tv);
            }
        }
        if let Some((_, sp, tvp, view)) = pair {
            move_with_children(state, &tv, tvp);
            state.set_pose(&sofa, sp);
            state.touch(&sofa);
            best = Some((1e9, sp, view));
        }
        // otherwise the TV moves too: every TV spot on a wall (clear of windows by free_ok, away from the doors) with the sofa
        // facing it 2.2-3.2 m off, its back on a wall when that distance meets one; the group near the room's middle
        let mut walled = false;
        if pair.is_none() {
            let mut tvbest: Option<(f64, super::geom::Pose, super::geom::Pose, f64)> = None;
            for (a, b, n) in segs.clone() {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = (dx * dx + dy * dy).sqrt();
                let tw = thi[1] - tlo[1];
                if len < tw + 0.1 { continue }
                let u = [dx / len, dy / len];
                let steps = ((len - tw) / 0.25).floor() as usize;
                for i in 0..=steps {
                    let tt = tw / 2.0 + 0.02 + (len - tw - 0.04) * if steps == 0 { 0.5 } else { i as f64 / steps as f64 };
                    let base = [a[0] + u[0] * tt, a[1] + u[1] * tt];
                    // the sofa out of the room while the TV tries the spot (wherever its last try left it, it blocks)
                    let so = state.objs.shift_remove(&sofa).unwrap();
                    let ok = try_place(state, &tv, n[1].atan2(n[0]), [tlo[0], (tlo[1] + thi[1]) / 2.0, tlo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]);
                    state.objs.insert(sofa.clone(), so);
                    state.touch(&sofa);
                    if !ok { continue }
                    let tv_door = ldoors.iter().map(|d| ((d[0] - base[0]).powi(2) + (d[1] - base[1]).powi(2)).sqrt()).fold(9.0f64, f64::min);
                    let tfront = [base[0] + n[0] * (thi[0] - tlo[0]), base[1] + n[1] * (thi[0] - tlo[0])];
                    for k in 0..6 {
                        let d = 2.2 + k as f64 * 0.2;
                        let at = [tfront[0] + n[0] * d, tfront[1] + n[1] * d, 0.0];
                        // sofa front (local +x) toward the TV
                        if !try_place(state, &sofa, (-n[1]).atan2(-n[0]), [shi[0], (slo[1] + shi[1]) / 2.0, slo[2]], at) { continue }
                        let back = state.objs[&sofa].pose.apply([slo[0], (slo[1] + shi[1]) / 2.0, 0.0]);
                        let on_wall = layout2d::ring_dist(&rings, [back[0], back[1]]) < 0.15;
                        let mid = [(base[0] + back[0]) / 2.0, (base[1] + back[1]) / 2.0];
                        let score = if on_wall { 2.0 } else { 0.0 } - (d - 2.6).abs() - (1.6 - tv_door).max(0.0) * 1.5
                            - ((mid[0] - fc[0]).powi(2) + (mid[1] - fc[1]).powi(2)).sqrt() * 0.3 - (tt - len / 2.0).abs() / len;
                        if tvbest.as_ref().map_or(true, |b| score > b.0) { tvbest = Some((score, state.objs[&tv].pose, state.objs[&sofa].pose, d)); }
                    }
                }
            }
            state.set_pose(&tv, tv_start); state.touch(&tv);
            if let Some((_, tvp, sp, d)) = tvbest {
                move_with_children(state, &tv, tvp);
                state.set_pose(&sofa, sp);
                state.touch(&sofa);
                best = Some((1e8, sp, d));
                walled = true;
            }
        }
        // otherwise: back on any wall that looks toward the TV where it stands (1.5-4.5 m, within ~40 degrees)
        if pair.is_none() && !walled {
            let tc = { let (a, b) = (tlo, thi); state.objs[&tv].pose.apply([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0]) };
            for (a, b, n) in segs.clone() {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = (dx * dx + dy * dy).sqrt();
                if len < sw + 0.1 { continue }
                let u = [dx / len, dy / len];
                let steps = ((len - sw) / 0.2).floor() as usize;
                for i in 0..=steps {
                    let tt = sw / 2.0 + 0.02 + (len - sw - 0.04) * if steps == 0 { 0.5 } else { i as f64 / steps as f64 };
                    let base = [a[0] + u[0] * tt, a[1] + u[1] * tt];
                    let fc2 = [base[0] + n[0] * sdep, base[1] + n[1] * sdep];
                    let v = [tc[0] - fc2[0], tc[1] - fc2[1]];
                    let dist = (v[0] * v[0] + v[1] * v[1]).sqrt();
                    if !(1.5..=3.6).contains(&dist) { continue }
                    let cos = (v[0] * n[0] + v[1] * n[1]) / dist;
                    if cos < 0.77 { continue }
                    if !try_place(state, &sofa, n[1].atan2(n[0]), [slo[0], (slo[1] + shi[1]) / 2.0, slo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]) { continue }
                    let score = 100.0 + cos * 3.0 - (dist - 2.6).abs();
                    if best.as_ref().map_or(true, |(s, _, _)| score > *s) { best = Some((score, state.objs[&sofa].pose, dist)); walled = true; }
                }
            }
        }
        // otherwise: facing the TV where it stands, 1.3 m (small rooms) to 4.2 m
        for di in 0..=24 {
            if pair.is_some() || walled { break }
            let d = 1.3 + di as f64 * 0.12;
            for lat in [0.0f64, 0.3, -0.3] {
                let at = [front[0] + f[0] * d - f[1] * lat, front[1] + f[1] * d + f[0] * lat, 0.0];
                // sofa front (local +x) toward the TV: turned half a turn from the stand, its front face at distance d
                if !try_place(state, &sofa, ty + std::f64::consts::PI, [shi[0], (slo[1] + shi[1]) / 2.0, slo[2]], at) {
                    continue;
                }
                let back = state.objs[&sofa].pose.apply([slo[0], (slo[1] + shi[1]) / 2.0, 0.0]);
                let on_wall = layout2d::ring_dist(&rings, [back[0], back[1]]) < 0.15;
                // a sofa with its back on a wall (opposite the TV) reads as placed; floating mid-room only when no wall spot works
                let score = -(d - 2.4).abs() * 0.5 - lat.abs() + if on_wall { 2.5 } else { 0.0 };
                if best.as_ref().map_or(true, |(s, _, _)| score > *s) {
                    best = Some((score, state.objs[&sofa].pose, d));
                }
            }
        }
        match best {
            Some((_, p, d)) => {
                state.set_pose(&sofa, p);
                state.touch(&sofa);
                log.push(format!("{room}: sofa {sofa} faces the TV at {d:.1} m"));
            }
            None => {
                state.set_pose(&sofa, start);
                state.touch(&sofa);
                log.push(format!("{room}: no spot for the sofa facing the TV"));
            }
        }
        // put the lifted pieces back: the coffee table in front of the sofa, armchairs where they were or beside the coffee table
        // turned toward it; a piece with no valid spot is left out (with whatever stood on it)
        let sp = state.objs[&sofa].pose;
        let sy = yaw(&sp);
        let sf = [sy.cos(), sy.sin()];
        let sfront = sp.apply([shi[0], (slo[1] + shi[1]) / 2.0, 0.0]);
        let mut coffee_c: Option<[f64; 2]> = None;
        let mut coffee_hw = 0.5f64;
        for (k, o) in lifted.iter().filter(|(_, o)| class_of(o) == "CoffeeTableFactory").chain(lifted.iter().filter(|(_, o)| class_of(o) == "ArmChairFactory")) {
            state.objs.insert(k.clone(), o.clone());
            state.touch(k);
            let (clo, chi) = lohi(state, k);
            let mut ok = false;
            if class_of(o) == "CoffeeTableFactory" {
                // about the sofa's seat width, three quarters of it (a 0.8 m table before a 2.6 m sofa reads as too small)
                let target = ((shi[1] - slo[1]) * 0.75).clamp(0.6, 1.6);
                if (target - (chi[1] - clo[1])).abs() > 0.1 {
                    let base = state.objs[k].clone();
                    let _ = super::solver::set_width(ctx, state, k, &base, target, 1.0);
                }
                let (clo, chi) = lohi(state, k);
                for gap in [0.45f64, 0.35, 0.55, 0.3, 0.7] {
                    let off = gap + (chi[0] - clo[0]) / 2.0;
                    let at = [sfront[0] + sf[0] * off, sfront[1] + sf[1] * off, 0.0];
                    if try_place(state, k, sy, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], at) {
                        log.push(format!("{room}: coffee table {gap:.2} m before the sofa"));
                        coffee_c = Some([at[0], at[1]]);
                        coffee_hw = (chi[1] - clo[1]) / 2.0;
                        ok = true;
                        break;
                    }
                }
                // no spot before the sofa: back where the solver had it, if that is still free
                if !ok { ok = free_ok(state, k); }
            } else {
                let saved = state.objs[k].pose;
                let c = coffee_c.unwrap_or([sfront[0] + sf[0] * 0.8, sfront[1] + sf[1] * 0.8]);
                // beside the coffee table, turned to face it across a 0.45 m gap: the side away from the room's walls first, then
                // the other; else where the solver had it
                let half = (chi[0] - clo[0]) / 2.0;
                'a: for gap in [0.45f64, 0.6, 0.35] {
                    for side in [1.0f64, -1.0] {
                        let dd = coffee_hw + gap + half;
                        let at = [c[0] - sf[1] * side * dd, c[1] + sf[0] * side * dd, 0.0];
                        let face = (c[1] - at[1]).atan2(c[0] - at[0]);
                        if try_place(state, k, face, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], at) { ok = true; break 'a; }
                    }
                }
                if !ok {
                    state.set_pose(k, saved);
                    state.touch(k);
                    ok = free_ok(state, k);
                }
            }
            if ok {
                for (c, co) in kids.iter().filter(|(_, co)| co.relations.iter().any(|r| &r.target == k)) {
                    // items on it follow it
                    let mut co = co.clone();
                    let d = [o.pose.t[0], o.pose.t[1]];
                    let nt = state.objs[k].pose.t;
                    co.pose.translate([nt[0] - d[0], nt[1] - d[1], 0.0]);
                    state.objs.insert(c.clone(), co);
                    state.touch(c);
                }
            } else {
                state.objs.shift_remove(k);
                log.push(format!("{room}: {} left out (no spot around the sofa)", class_of(o)));
            }
        }
    }
    log
}

/// dining tables: moved to the open middle of their room (long side along the room's long axis), with a matching set of chairs
/// (copies of the table's chair, up to the table's slots: 4-6); table_setting::gather_chairs then seats them
pub fn dining(ctx: &mut Ctx, state: &mut State, rng: &mut Rng) -> Vec<String> {
    let mut log = vec![];
    let tables: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "TableDiningFactory").map(|(k, _)| k.clone()).collect();
    for t in tables {
        let Some(room) = layout2d::room_of(state, &t) else { continue };
        let rings = layout2d::rings(&layout2d::room_floor(state, &room));
        let Some(r0) = rings.first() else { continue };
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in r0 { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
        let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let (lo, hi) = lohi(state, &t);
        // table width (local y) along the room's long axis
        let yaw_ = if x1 - x0 >= y1 - y0 { std::f64::consts::FRAC_PI_2 } else { 0.0 };
        let anchor = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
        // its chairs out of the way while the table moves (gather_chairs re-seats them)
        let chairs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "ChairFactory" && o.relations.iter().any(|r| r.target == t)).map(|(k, _)| k.clone()).collect();
        let saved: Vec<(String, ObjState)> = chairs.iter().map(|k| (k.clone(), state.objs[k].clone())).collect();
        for k in &chairs { state.objs.shift_remove(k); }
        let _ = (c, yaw_, anchor);
        // a small dining room gets a small table (a centred 1.6 m table with its chairs closed the path between the doors)
        let room_area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        if room_area < 15.0 {
            let base = state.objs[&t].clone();
            if let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) {
                if cs[0] > 1.05 && !super::solver::set_width(ctx, state, &t, &base, 1.0, 1.0) && !free_ok(state, &t) {
                    *state.objs.get_mut(&t).unwrap() = base;
                    state.touch(&t);
                }
            }
        }
        // tables in dining rooms go to the middle; eat-in tables (kitchen / living room) keep their spot clear of the seating
        let placed = if room_type_of(&room) == "dining-room" { center_table(state, &t, &room) } else { true };
        for (k, o) in saved { state.objs.insert(k.clone(), o); state.touch(&k); }
        // a full set of chairs: copies of the first chair
        let tw = { let (l2, h2) = lohi(state, &t); let _ = (lo, hi); h2[1] - l2[1] };
        // one chair per 0.6 m on each long side and one at each end: 1.4 m -> 6, 2.1 m -> 8
        // (a small table, under 1.2 m: one chair each side, four; two on adjacent sides read as lopsided)
        let want = if tw < 1.2 { 4 } else { (2 * ((tw / 0.6).floor() as usize).max(1)).clamp(2, 6) + 2 };
        let mut chairs = chairs;
        if chairs.is_empty() {
            // a table without chairs: a first chair from scratch, the copies below make the set
            let cname = format!("{t}_chair0");
            if new_piece(ctx, state, rng, "ChairFactory", &room, &cname) {
                if let Some(r0) = state.objs[&cname].relations.first().cloned() { let mut r = r0; r.target = t.clone(); state.objs.get_mut(&cname).unwrap().relations.push(r); }
                let tp = state.objs[&t].pose.t;
                let mut p = state.objs[&cname].pose; p.set_loc([tp[0], tp[1], 0.0]); state.set_pose(&cname, p); state.touch(&cname);
                chairs.push(cname);
            }
        }
        let have = chairs.len();
        if let Some(first) = chairs.first().cloned() {
            for i in have..want {
                let new = format!("{first}_set{i}");
                if state.objs.contains_key(&new) { continue }
                clone_obj(state, &first, &new, None, &t);
            }
        }
        log.push(format!("{room}: table {t} {} with {} chairs wanted", if placed { "in the middle" } else { "kept in place" }, want.max(have)));
    }
    log
}

/// bed placement before the nightstands (see `bedroom_and_desk`)
pub fn beds(state: &mut State) -> Vec<String> {
    let beds: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "BedFactory").map(|(k, _)| k.clone()).collect();
    beds.iter().filter_map(|b| place_bed(state, b)).collect()
}

/// a piece placed against a wall of `room`: every wall spot (0.2 m steps) where the placement is valid is scored by `score(centre,
/// wall position t, wall length)` and the best one kept; false (and the piece back where it was) when none is valid
fn place_on_wall(state: &mut State, name: &str, room: &str, score: &dyn Fn(&State, [f64; 2], f64, f64, [f64; 2]) -> f64) -> bool {
    let (lo, hi) = lohi(state, name);
    let w = hi[1] - lo[1];
    let start = state.objs[name].pose;
    let mut best: Option<(f64, super::geom::Pose)> = None;
    for (a, b, n) in wall_segments(state, room) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < w + 0.05 { continue }
        let u = [dx / len, dy / len];
        let yaw_ = n[1].atan2(n[0]);
        let steps = ((len - w) / 0.2).floor() as usize;
        for i in 0..=steps {
            let t = w / 2.0 + 0.01 + (len - w - 0.02) * if steps == 0 { 0.5 } else { i as f64 / steps as f64 };
            let at = [a[0] + u[0] * t + n[0] * 0.003, a[1] + u[1] * t + n[1] * 0.003, 0.0];
            if !try_place(state, name, yaw_, [lo[0], (lo[1] + hi[1]) / 2.0, lo[2]], at) { continue }
            let c = [at[0] + n[0] * (hi[0] - lo[0]) / 2.0, at[1] + n[1] * (hi[0] - lo[0]) / 2.0];
            let sc = score(state, c, t, len, n);
            if best.as_ref().map_or(true, |(s, _)| sc > *s) { best = Some((sc, state.objs[name].pose)); }
        }
    }
    let ok = best.is_some();
    let p = best.map(|b| b.1).unwrap_or(start);
    state.set_pose(name, p);
    state.touch(name);
    ok
}

/// living rooms without a TV stand get one (centred on a long wall away from the doors); bathrooms with neither a tub nor a shower
/// get a shower in a corner
/// (`only_room`: the room this solve furnishes; every room is solved on its own copy of the house, so the others look empty here)
pub fn missing_pieces(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && only_room.map_or(true, |r| r == k.as_str())).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let rt = room_type_of(&room).to_string();
        let has = |state: &State, c: &str| state.objs.iter().any(|(k, o)| class_of(o) == c && layout2d::room_of(state, k).as_deref() == Some(room.as_str()));
        let doors = door_points(state, &room);
        let door_d = move |c: [f64; 2]| doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(4.0f64, f64::min);
        if rt == "living-room" && !has(state, "TVStandFactory") {
            let name = format!("{room}_tvstand").replace('/', "_");
            if new_piece(ctx, state, rng, "TVStandFactory", &room, &name) {
                let dd = door_d.clone();
                if place_on_wall(state, &name, &room, &|_, c, t, len, _n| dd(c).min(3.0) + len * 0.15 - (t - len / 2.0).abs() * 0.5) {
                    log.push(format!("{room}: added a TV stand"));
                } else { state.objs.shift_remove(&name); }
            }
        }
        // closets: a wardrobe along the longest wall (the built-in fill then sizes it to the span)
        if rt == "closet" && !has(state, "WardrobeFactory") {
            let name = format!("{room}_wardrobe").replace('/', "_");
            if new_piece(ctx, state, rng, "WardrobeFactory", &room, &name) {
                let dd = door_d.clone();
                if place_on_wall(state, &name, &room, &|_, c, _t, len, _n| len + dd(c).min(1.5)) {
                    log.push(format!("{room}: added a wardrobe"));
                } else {
                    state.objs.shift_remove(&name);
                    // too tight for a wardrobe: open shelving
                    let sname = format!("{room}_shelf").replace('/', "_");
                    if new_piece(ctx, state, rng, "LargeShelfFactory", &room, &sname) {
                        let dd = door_d.clone();
                        if place_on_wall(state, &sname, &room, &|_, c, _t, len, _n| len + dd(c).min(1.5)) { log.push(format!("{room}: added shelving")); } else { state.objs.shift_remove(&sname); }
                    }
                }
            }
        }
        // kitchens without a range get one (the kitchen pass puts it at the counter run)
        if rt == "kitchen" && !has(state, "OvenFactory") {
            let name = format!("{room}_range").replace('/', "_");
            if new_piece(ctx, state, rng, "OvenFactory", &room, &name) {
                if place_on_wall(state, &name, &room, &|_, _c, _t, len, _n| len) { log.push(format!("{room}: added a range")); } else { state.objs.shift_remove(&name); }
            }
        }
        // a bedroom the solver left without a bed (angled or tight rooms): one against the longest free wall (the beds pass then
        // settles it); if no wall takes it, the storage that crowds the room gives way
        if rt == "bedroom" && !has(state, "BedFactory") {
            let name = format!("{room}_bed").replace('/', "_");
            if new_piece(ctx, state, rng, "BedFactory", &room, &name) {
                let score = |_: &State, _c: [f64; 2], t: f64, len: f64, _n: [f64; 2]| len - (t - len / 2.0).abs();
                let mut ok = place_on_wall(state, &name, &room, &score);
                if !ok {
                    let crowd: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "WardrobeFactory" | "LargeShelfFactory" | "SimpleDeskFactory" | "SingleCabinetFactory")
                        && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
                    for k in crowd {
                        layout2d::remove_tree(state, &k);
                        log.push(format!("{room}: removed {k} to fit a bed"));
                        ok = place_on_wall(state, &name, &room, &score);
                        if ok { break; }
                    }
                }
                if ok { log.push(format!("{room}: added a bed")); } else { state.objs.shift_remove(&name); }
            }
        }
        if rt == "bathroom" && !has(state, "BathtubFactory") && !has(state, "ShowerFactory") {
            let name = format!("{room}_shower").replace('/', "_");
            if new_piece(ctx, state, rng, "ShowerFactory", &room, &name) {
                let dd = door_d.clone();
                if place_on_wall(state, &name, &room, &|_, c, t, len, _n| dd(c).min(2.0) * 0.5 - t.min(len - t)) {
                    log.push(format!("{room}: added a shower"));
                } else { state.objs.shift_remove(&name); }
            }
        }
    }
    log
}

/// bathrooms: a toilet within 1 m of the door moves to the wall spot farthest from the doors (kept where it is if none is free)
pub fn toilets(state: &mut State, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let wcs: Vec<(String, String)> = state.objs.iter().filter(|(k, o)| class_of(o) == "ToiletFactory")
        .filter_map(|(k, _)| layout2d::room_of(state, k).map(|r| (k.clone(), r))).filter(|(_, r)| only_room.map_or(true, |o| o == r)).collect();
    for (wc, room) in wcs {
        let doors = door_points(state, &room);
        let door_d = move |c: [f64; 2]| doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(4.0f64, f64::min);
        // clearance to the other fixtures (a toilet touching the tub reads as jammed in)
        let wc2 = wc.clone();
        let room2 = room.clone();
        let clr = move |st: &State| st.objs.iter().filter(|(k, o)| matches!(class_of(o), "BathtubFactory" | "ShowerFactory" | "StandingSinkFactory")
            && layout2d::room_of(st, k).as_deref() == Some(room2.as_str())).map(|(k, _)| layout2d::dist(st, k, &wc2)).fold(1.0f64, f64::min);
        let t = state.objs[&wc].pose.t;
        let d0 = door_d([t[0], t[1]]);
        let c0 = clr(state);
        if d0 >= 1.0 && c0 >= 0.15 { continue }
        let s0 = d0.min(2.5) + 2.0 * c0.min(0.3);
        let start = state.objs[&wc].pose;
        let dd = door_d.clone();
        let clr2 = clr.clone();
        if place_on_wall(state, &wc, &room, &move |st, c, _t, _len, _n| dd(c).min(2.5) + 2.0 * clr2(st).min(0.3)) {
            let t1 = state.objs[&wc].pose.t;
            let s1 = door_d([t1[0], t1[1]]).min(2.5) + 2.0 * clr(state).min(0.3);
            if s1 > s0 + 0.2 {
                log.push(format!("{room}: toilet moved (door {d0:.1} -> {:.1} m, clearance {c0:.2} -> {:.2} m)", door_d([t1[0], t1[1]]), clr(state)));
                continue;
            }
        }
        state.set_pose(&wc, start);
        state.touch(&wc);
    }
    log
}

/// bedrooms: a wardrobe within 0.7 m of the bed (at its foot, against its side) moves to the wall spot farthest from the bed and
/// clear of the doors; kept where it is when no spot does better
pub fn wardrobes(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let ws: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| class_of(o) == "WardrobeFactory")
        .filter_map(|(k, _)| layout2d::room_of(state, k).filter(|r| room_type_of(r) == "bedroom").map(|r| (k.clone(), r))).collect();
    for (w, room) in ws {
        let beds: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "BedFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        let Some(bed) = beds.first().cloned() else { continue };
        let d0 = layout2d::dist(state, &w, &bed);
        if d0 >= 0.7 { continue }
        let doors = door_points(state, &room);
        let door_d = move |c: [f64; 2]| doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(4.0f64, f64::min);
        let score = |st: &State, c: [f64; 2], w2: &str, bed: &str| layout2d::dist(st, w2, bed).min(2.0) + door_d(c).min(1.2) * 0.5;
        let t = state.objs[&w].pose.t;
        let s0 = score(state, [t[0], t[1]], &w, &bed);
        let start = state.objs[&w].pose;
        let (w2, b2) = (w.clone(), bed.clone());
        let sc = &score;
        if place_on_wall(state, &w, &room, &move |st, c, _t, _len, _n| sc(st, c, &w2, &b2)) {
            let t1 = state.objs[&w].pose.t;
            if score(state, [t1[0], t1[1]], &w, &bed) > s0 + 0.3 && layout2d::dist(state, &w, &bed) >= 0.6 {
                log.push(format!("{room}: wardrobe {w} moved off the bed ({d0:.2} -> {:.2} m)", layout2d::dist(state, &w, &bed)));
                continue;
            }
        }
        state.set_pose(&w, start);
        state.touch(&w);
    }
    log
}

/// a table in the open middle of `room` (long side along the room's long axis), tried at the floor's bbox centre then rings of
/// offsets; true when placed
fn center_table(state: &mut State, t: &str, room: &str) -> bool {
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    let Some(r0) = rings.first() else { return false };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in r0 { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
    let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
    let (lo, hi) = lohi(state, t);
    let yaw_ = if x1 - x0 >= y1 - y0 { std::f64::consts::FRAC_PI_2 } else { 0.0 };
    let anchor = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
    // room for the chairs: 0.75 m from the walls and 1 m from the doors all round (the first free spot without it lost chairs to
    // blocked slots); else the first free spot
    let doors = door_points(state, room);
    let roomy = |st: &State| layout2d::prisms(st, t).iter().flat_map(|p| p.poly.iter().copied().collect::<Vec<_>>()).all(|v| {
        layout2d::ring_dist(&rings, v) >= 0.75 && doors.iter().all(|d| ((d[0] - v[0]).powi(2) + (d[1] - v[1]).powi(2)).sqrt() >= 1.0)
    });
    let mut first: Option<super::geom::Pose> = None;
    for r in [0.0f64, 0.3, 0.6, 0.9, 1.2] {
        for (ox, oy) in [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r), (r, r), (-r, -r), (r, -r), (-r, r)] {
            if r == 0.0 && (ox != 0.0 || oy != 0.0) { continue }
            for yy in [yaw_, yaw_ + std::f64::consts::FRAC_PI_2] {
                if try_place(state, t, yy, anchor, [c[0] + ox, c[1] + oy, 0.0]) {
                    if roomy(state) { return true; }
                    if first.is_none() { first = Some(state.objs[t].pose); }
                }
            }
        }
    }
    if let Some(p) = first { state.set_pose(t, p); state.touch(t); return true; }
    false
}

/// rugs under their group: in a living room centred on the coffee table (else between the sofa and the TV), turned with the sofa; in
/// a bedroom under the foot two thirds of the bed. A rug that would leave its room stays where it is
pub fn rugs(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let rugs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "RugFactory").map(|(k, _)| k.clone()).collect();
    for rug in rugs {
        let Some(room) = layout2d::room_of(state, &rug) else { continue };
        let in_room = |state: &State, c: &str| state.objs.iter().find(|(k, o)| class_of(o) == c && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone());
        let (lo, hi) = lohi(state, &rug);
        let anchor = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
        let target: Option<([f64; 2], f64)> = match room_type_of(&room) {
            "living-room" => in_room(state, "SofaFactory").map(|s| {
                let sp = state.objs[&s].pose;
                let c = in_room(state, "CoffeeTableFactory").map(|ct| { let t = state.objs[&ct].pose; let (l, h) = lohi(state, &ct); let w = t.apply([(l[0] + h[0]) / 2.0, (l[1] + h[1]) / 2.0, 0.0]); [w[0], w[1]] })
                    .unwrap_or_else(|| { let (l, h) = lohi(state, &s); let w = sp.apply([h[0] + 0.8, (l[1] + h[1]) / 2.0, 0.0]); [w[0], w[1]] });
                (c, yaw(&sp))
            }),
            "bedroom" => in_room(state, "BedFactory").map(|b| {
                let bp = state.objs[&b].pose;
                let (l, h) = lohi(state, &b);
                let w = bp.apply([l[0] + (h[0] - l[0]) * 0.62, (l[1] + h[1]) / 2.0, 0.0]);
                ([w[0], w[1]], yaw(&bp))
            }),
            _ => None,
        };
        let Some((c, y)) = target else { continue };
        let old = state.objs[&rug].pose;
        // the rug's long side across the sofa's / bed's facing direction reads best in both rooms
        let long_y = hi[1] - lo[1] >= hi[0] - lo[0];
        let y0 = if long_y { y } else { y + std::f64::consts::FRAC_PI_2 };
        let mut placed = false;
        // turned the other way when the room is too narrow for it across; a rug that fits under neither way reads as stray: removed
        for yy in [y0, y0 + std::f64::consts::FRAC_PI_2] {
            let (s_, c_) = yy.sin_cos();
            let ra = [c_ * anchor[0] - s_ * anchor[1], s_ * anchor[0] + c_ * anchor[1]];
            state.set_pose(&rug, super::geom::Pose::from_euler([c[0] - ra[0], c[1] - ra[1], old.t[2]], [0.0, 0.0, yy]));
            state.touch(&rug);
            if layout2d::outside_room_depth(state, &rug, &room) <= 0.02 { placed = true; break; }
        }
        if placed {
            log.push(format!("{room}: rug under the group"));
        } else {
            layout2d::remove_tree(state, &rug);
            log.push(format!("{room}: rug {rug} fits under no group, removed"));
        }
    }
    log
}

/// houses without a dining room: an eat-in table (see the block below), placed after the living-room group so it keeps clear of
/// the seating
pub fn eat_in(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && only_room.map_or(true, |r| r == k.as_str())).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let rt = room_type_of(&room).to_string();
        let has = |state: &State, c: &str| state.objs.iter().any(|(k, o)| class_of(o) == c && layout2d::room_of(state, k).as_deref() == Some(room.as_str()));
        // no dining room in the house: an eat-in table, in the kitchen when it has 13 m2 or more, else in the living room (decided from
        // the floor plan alone: each room is solved on its own copy of the house). One chair now; `dining` centres the table and
        // completes the set
        let area_of = |state: &State, r: &str| state.objs[r].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        let no_dining = !state.objs.iter().any(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "dining-room");
        let kitchen_big = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen").any(|(k, _)| area_of(state, k) >= 18.0);
        // a living room takes the table only when it is large enough for a separate dining corner
        // (a living room no longer takes one: a table among the sofa group reads as misplaced)
        // (a kitchen no longer takes one either: its counters line every free wall, and a table among them read as in the way)
        let host = false && rt == "kitchen" && kitchen_big;
        if no_dining && host && !has(state, "TableDiningFactory") {
            let tname = format!("{room}_eatin").replace('/', "_");
            if new_piece(ctx, state, rng, "TableDiningFactory", &room, &tname) {
                if place_away_from_seating(state, &tname, &room) {
                    let cname = format!("{tname}_chair");
                    if new_piece(ctx, state, rng, "ChairFactory", &room, &cname) {
                        // dining chairs belong to their table (gather_chairs seats the chairs related to it)
                        if let Some(r0) = state.objs[&cname].relations.first().cloned() { let mut r = r0; r.target = tname.clone(); state.objs.get_mut(&cname).unwrap().relations.push(r); }
                    }
                    log.push(format!("{room}: added an eat-in dining table"));
                } else { state.objs.shift_remove(&tname); }
            }
        }
    }
    log
}

/// a table at the valid spot of `room` (0.3 m grid, square to the room) farthest from the seating group (sofa / TV stand / coffee
/// table) and at least 0.6 m from the walls' doors; true when placed
fn place_away_from_seating(state: &mut State, t: &str, room: &str) -> bool {
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    let Some(r0) = rings.first() else { return false };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in r0 { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
    let seat: Vec<[f64; 2]> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "SofaFactory" | "TVStandFactory" | "CoffeeTableFactory") && layout2d::room_of(state, k).as_deref() == Some(room)).map(|(_, o)| [o.pose.t[0], o.pose.t[1]]).collect();
    let (lo, hi) = lohi(state, t);
    let anchor = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
    let start = state.objs[t].pose;
    let mut best: Option<(f64, super::geom::Pose)> = None;
    let mut x = x0 + 0.6;
    while x < x1 - 0.6 {
        let mut y = y0 + 0.6;
        while y < y1 - 0.6 {
            for yy in [0.0f64, std::f64::consts::FRAC_PI_2] {
                if try_place(state, t, yy, anchor, [x, y, 0.0]) {
                    let d = seat.iter().map(|s| ((s[0] - x).powi(2) + (s[1] - y).powi(2)).sqrt()).fold(4.0f64, f64::min);
                    // far from the seating, but with room around it for chairs (distance to the walls)
                    let wall = layout2d::ring_dist(&rings, [x, y]).min(1.6);
                    if !seat.is_empty() && d < 1.8 { continue }
                    let sc = d + wall;
                    if best.as_ref().map_or(true, |(b, _)| sc > *b) { best = Some((sc, state.objs[t].pose)); }
                }
            }
            y += 0.3;
        }
        x += 0.3;
    }
    let ok = best.is_some();
    state.set_pose(t, best.map(|b| b.1).unwrap_or(start));
    state.touch(t);
    ok
}

/// wall pieces (toilets, cabinets, bookcases, shelves, wardrobes, desks) standing on a wall at an angle move to a straight wall of
/// their room when one has a valid spot (nearest to where they were)
pub fn square_up(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let names: Vec<String> = state.objs.iter().filter(|(_, o)| matches!(class_of(o), "ToiletFactory" | "SingleCabinetFactory" | "KitchenCabinetFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory" | "WardrobeFactory" | "SimpleDeskFactory" | "TVStandFactory" | "StandingSinkFactory")
        && o.relations.iter().all(|r| state.objs.get(&r.target).map_or(true, |t| t.generator.is_none()))).map(|(k, _)| k.clone()).collect();
    for k in names {
        let y = yaw(&state.objs[&k].pose).to_degrees().rem_euclid(90.0);
        if y.min(90.0 - y) < 3.0 { continue }
        let Some(room) = layout2d::room_of(state, &k) else { continue };
        let p0 = [state.objs[&k].pose.t[0], state.objs[&k].pose.t[1]];
        let straight_ok = |n: [f64; 2]| n[0].abs() < 1e-3 || n[1].abs() < 1e-3;
        // score: straight walls only, nearest to the old spot
        let segs = wall_segments(state, &room);
        if !segs.iter().any(|s| straight_ok(s.2)) { continue }
        let start = state.objs[&k].pose;
        if place_on_wall(state, &k, &room, &move |_st: &State, c: [f64; 2], _t, _len, n: [f64; 2]| if straight_ok(n) { -((c[0] - p0[0]).powi(2) + (c[1] - p0[1]).powi(2)).sqrt() } else { -1e6 }) {
            let y2 = yaw(&state.objs[&k].pose).to_degrees().rem_euclid(90.0);
            if y2.min(90.0 - y2) < 3.0 {
                log.push(format!("{} {k}: moved to a straight wall", class_of(&state.objs[&k])));
            } else {
                // only angled spots were free: it stays where it was
                state.set_pose(&k, start);
                state.touch(&k);
            }
        }
    }
    log
}

/// a footprint (world corners, any order around) inside `room`'s floor and clear of every piece up to 0.7 m high except `owner` and
/// its children
fn slot_free(state: &State, room: &str, owner: &str, corners: &[[f64; 2]]) -> bool {
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    if !corners.iter().all(|c| layout2d::in_rings(&rings, *c)) { return false }
    let probe = layout2d::prism_of(&corners.iter().map(|c| [c[0], c[1], 0.0]).chain(corners.iter().map(|c| [c[0], c[1], 0.7])).collect::<Vec<_>>());
    let nc = Tag::sem("NoCollision");
    state.objs.iter().all(|(k, o)| {
        k == owner || layout2d::is_room(o) || o.tags.contains(nc) || o.relations.iter().any(|r| r.target == owner) || k.starts_with("doorclear/") && false
            || layout2d::prisms(state, k).iter().all(|p| layout2d::prism_depth(&probe, p) <= 1e-4)
    })
}

/// counter runs without a sink get one on their top: under a window that overlooks the run (the usual spot), else at the middle,
/// against the backsplash; further spots along the run when items already stand there
pub fn sinks(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let runs: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && only_room.map_or(true, |r| layout2d::room_of(state, k).as_deref() == Some(r))).map(|(k, _)| k.clone()).collect();
    for run in runs.clone() {
        let Some(room) = layout2d::room_of(state, &run) else { continue };
        // a sink already on this run stays unless it stands against the range (no counter between): then it is placed again
        let on_run: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "SinkFactory" && o.relations.iter().any(|r| r.target == run)).map(|(k, _)| k.clone()).collect();
        if !on_run.is_empty() {
            let ovens0: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "OvenFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            let tight = on_run.iter().any(|sk| ovens0.iter().any(|o| layout2d::dist(state, o, sk) < 0.4));
            if !tight { continue }
            for k in on_run { layout2d::remove_tree(state, &k); log.push(format!("{run}: sink against the range, placed again")); }
        }
        // one sink per kitchen, on the run the range stands at (else the longest): a sink on a separate counter read as disjoint
        let in_room: Vec<&String> = runs.iter().filter(|r| layout2d::room_of(state, r).as_deref() == Some(room.as_str())).collect();
        let ovens: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "OvenFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        let key = |r: &str| -> (f64, f64) {
            let d = ovens.iter().map(|o| layout2d::dist(state, r, o)).fold(9.0f64, f64::min);
            let w = state.objs[r].generator.as_ref().and_then(super::solver::cur_size).map_or(0.0, |c| c[0]);
            (d, -w)
        };
        let main = in_room.iter().min_by(|a, b| key(a).partial_cmp(&key(b)).unwrap()).map(|r| r.to_string());
        if main.as_deref() != Some(run.as_str()) { continue }
        // a sink the solver put on another run moves here (removed; the one below takes its place)
        let elsewhere: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "SinkFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())
            && !o.relations.iter().any(|r| r.target == run)).map(|(k, _)| k.clone()).collect();
        for k in elsewhere { layout2d::remove_tree(state, &k); log.push(format!("{room}: sink {k} on a run away from the range, moved")); }
        // the solver's loose items on the run go first (the counter dresser removes and redresses them by zone anyway: dress::purge)
        let loose: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == run) && class_of(o) != "SinkFactory").map(|(k, _)| k.clone()).collect();
        for k in loose { if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); } }
        let rp = state.objs[&run].pose;
        let (rlo, rhi) = lohi(state, &run);
        // the counter top: the top of the run's lowest part
        let top = layout2d::prisms(state, &run).iter().map(|p| (p.z[0], p.z[1])).fold((f64::INFINITY, 0.0), |a, b| if b.0 < a.0 { b } else { a }).1;
        let name = format!("{run}_sink");
        if !new_piece(ctx, state, rng, "SinkFactory", &room, &name) { continue }
        if let Some(r) = state.objs[&run].relations.iter().find(|r| r.target == room && r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))).cloned() {
            let mut r = r;
            r.target = run.clone();
            let o = state.objs.get_mut(&name).unwrap();
            o.relations = vec![r];
        }
        let (slo, shi) = lohi(state, &name);
        // positions along the run (local y): under a window over the run first, then the middle, then steps out from it
        let inv = |p: [f64; 3]| { let d = [p[0] - rp.t[0], p[1] - rp.t[1]]; [rp.r[0][0] * d[0] + rp.r[1][0] * d[1], rp.r[0][1] * d[0] + rp.r[1][1] * d[1]] };
        let mut ys: Vec<f64> = state.objs.iter().filter(|(_, o)| o.tags.contains(Tag::sem("Window"))).filter_map(|(k, _)| {
            let p = layout2d::prisms(state, k);
            let c = p.first().map(|p| { let n = p.poly.len() as f64; [p.poly.iter().map(|v| v[0]).sum::<f64>() / n, p.poly.iter().map(|v| v[1]).sum::<f64>() / n, 0.0] })?;
            let l = inv(c);
            (l[0] > rlo[0] - 0.6 && l[0] < rhi[0] + 0.3 && l[1] > rlo[1] + 0.35 && l[1] < rhi[1] - 0.35).then_some(l[1])
        }).collect();
        let mid = (rlo[1] + rhi[1]) / 2.0;
        for k in 0..8 { let off = (k as f64 / 2.0).ceil() * 0.3 * if k % 2 == 0 { 1.0 } else { -1.0 }; ys.push(mid + off); }
        let sd = shi[0] - slo[0];
        let mut ok = false;
        // counter to work on between the sink and the range: 0.4 m or more first, then any free spot
        let range_gap = |st: &State| ovens.iter().map(|o| layout2d::dist(st, o, &name)).fold(9.0f64, f64::min);
        'pass: for strict in [true, false] {
            for &y in &ys {
                if y - (shi[1] - slo[1]) / 2.0 < rlo[1] + 0.05 || y + (shi[1] - slo[1]) / 2.0 > rhi[1] - 0.05 { continue }
                let at = rp.apply([rlo[0] + 0.06 + sd / 2.0, y, 0.0]);
                if try_place(state, &name, yaw(&rp), [(slo[0] + shi[0]) / 2.0, (slo[1] + shi[1]) / 2.0, slo[2]], [at[0], at[1], top]) && (!strict || range_gap(state) >= 0.4) { ok = true; break 'pass; }
            }
        }
        if ok { log.push(format!("{run}: sink added")); } else { state.objs.shift_remove(&name); log.push(format!("{run}: no room for a sink")); }
    }
    log
}

/// move `name` to `p`, taking every piece that stands on it (its children) along rigidly
fn move_with_children(state: &mut State, name: &str, p: super::geom::Pose) {
    let old = state.objs[name].pose;
    let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == name)).map(|(k, _)| k.clone()).collect();
    let (dy, oy) = (yaw(&p), yaw(&old));
    for k in kids {
        let c = state.objs[&k].pose;
        // child position in the parent's old frame, then into the new one
        let d = [c.t[0] - old.t[0], c.t[1] - old.t[1]];
        let (so, co) = (-oy).sin_cos();
        let l = [co * d[0] - so * d[1], so * d[0] + co * d[1]];
        let (sn, cn) = dy.sin_cos();
        let w = [p.t[0] + cn * l[0] - sn * l[1], p.t[1] + sn * l[0] + cn * l[1]];
        let ny = yaw(&c) + dy - oy;
        state.set_pose(&k, super::geom::Pose::from_euler([w[0], w[1], c.t[2]], [0.0, 0.0, ny]));
        state.touch(&k);
    }
    state.set_pose(name, p);
    state.touch(name);
}
