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

pub(crate) fn class_of(o: &ObjState) -> &str {
    o.generator.as_ref().map(|g| g.class.as_str()).unwrap_or("")
}

/// an object's footprint centre (world xy), for complaints
fn at_of(state: &State, k: &str) -> Option<[f64; 2]> {
    let o = state.objs.get(k)?;
    let c = o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]);
    Some([c[0], c[1]])
}
pub(crate) fn yaw(p: &super::geom::Pose) -> f64 {
    p.r[1][0].atan2(p.r[0][0])
}

/// beds with something standing within their bedding's reach (past the head, where it hangs close anyway) tuck the bedding in: a soft
/// volume may overlap its neighbours when laying out, but the cloth then went through them
pub fn tuck_crowded_beds(state: &mut State) -> Vec<String> {
    let beds: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "BedFactory").map(|(k, _)| k.clone()).collect();
    let mut out = vec![];
    for bed in beds {
        let Some(pc) = state.objs[&bed].generator.as_ref().and_then(|g| g.piece.clone()) else { continue };
        let vols = pc.collision();
        let Some((a, b)) = vols.iter().find(|v| matches!(v.role, crate::piece::Role::Soft)).map(|v| v.bounds()) else { continue };
        let p = state.objs[&bed].pose;
        let rect: Vec<[f64; 2]> = [[a[0] + 0.6, a[1]], [b[0], a[1]], [b[0], b[1]], [a[0] + 0.6, b[1]]].iter().map(|q| { let w = p.apply([q[0], q[1], 0.0]); [w[0], w[1]] }).collect();
        let crowded = state.objs.iter().any(|(k, o)| {
            if *k == bed || o.generator.is_none() || o.relations.iter().any(|r| r.target == bed) { return false }
            let c = class_of(o);
            if ["Rug", "Art", "Mirror", "Light", "Curtain", "Fan"].iter().any(|x| c.contains(x)) { return false }
            layout2d::prisms(state, k).iter().any(|x| x.role == super::geom::ROLE_HARD && x.z[0] < p.t[2] + 0.5 && layout2d::poly_dist(&x.poly, &rect) <= 0.0)
        });
        if crowded && super::solver::tuck_piece(state, &bed) { out.push(format!("{bed}: bedding tucked in, a piece stands within its reach")); }
    }
    out
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
    // (windows are named so: they carry no Window tag here, and the rules below never fired)
    let is_win = |k: &str, b: &ObjState| k.starts_with("window") || b.tags.contains(Tag::sem("Window"));
    let by_class = matches!(class_of(o), "BedFactory" | "KitchenSpaceFactory" | "OvenFactory" | "DishwasherFactory" | "SinkFactory");
    // anything standing higher than a sill clears the window in plan (a sideboard's back stood 12 cm in a window's depth; windows carry no
    // collision of their own there)
    if !by_class {
        let pa = layout2d::prisms(state, name);
        for (k, b) in state.objs.iter().filter(|(k, b)| k.as_str() != name && is_win(k.as_str(), b)) {
                    let (wl, wh) = layout2d::lohi_world(state, k);
            if top <= wl[2] + 0.02 { continue }
            // (its box: windows have no prisms)
            let rect = vec![[wl[0], wl[1]], [wh[0], wl[1]], [wh[0], wh[1]], [wl[0], wh[1]]];
            if pa.iter().any(|x| layout2d::poly_dist(&x.poly, &rect) < 0.05) { return false }
        }
    }
    for (k, b) in &state.objs {
        // (low pieces stand under a window only if they clear its sill: a sideboard stood 12 cm into a low window)
        let under_sill = top < 1.0 && top <= b.pose.t[2] + b.geom.lo[2] + 0.02;
        if k == name || b.tags.contains(nc) || layout2d::is_room(b) || (is_win(k.as_str(), b) && skip_win && (by_class || under_sill)) {
            continue;
        }
        // art and mirrors hang on the wall over furniture
        // (but a full-length mirror comes down to the floor: a nightstand stood 2 cm into one)
        let full_length = class_of(b) == "MirrorFactory" && b.geom.hi[2] - b.geom.lo[2] >= 1.2 && b.pose.t[2] + b.geom.lo[2] < top - 0.02;
        if matches!(class_of(b), "WallArtFactory" | "MirrorFactory") && !full_length {
            continue;
        }
        // a door's cutter box reaches 1 m into each room (2 m deep: it cuts the walls); the door clearances (doorclear/..., from the
        // wall face, the opening's width) say what a door needs -- the cutter took a 1.5 x 1.5 m WC's whole floor from the toilet
        if b.tags.contains(Tag::sem("Cutter")) && !is_win(k.as_str(), b) && (k.starts_with("door") || k.starts_with("entrance")) {
            continue;
        }
        // another room's door clearance and feature keep-outs stop at its walls (a bedroom's built-in shelves kept the living
        // room's sofa off the far side of the wall)
        if k.starts_with("doorclear/") || b.tags.contains(Tag::sem("FeatureKeepout")) || class_of(b).ends_with("Feature") {
            // door clearances carry their room in the name (doorclear/<room>/<i>); features and their keep-outs by relation
            let rb = if let Some(rest) = k.strip_prefix("doorclear/") { rest.rsplit_once('/').map(|(r, _)| r.to_string()) }
                else { layout2d::room_of(state, k).or_else(|| k.rsplit_once(".keepout").and_then(|(f, _)| layout2d::room_of(state, f))) };
            if rb.zip(layout2d::room_of(state, name)).map_or(false, |(rb, ra)| rb != ra) { continue }
        }
        // a piece's own children (items on it) are not obstacles
        if b.relations.iter().any(|r| r.target == name) {
            continue;
        }
        if layout2d::depth(state, name, k) > 1e-4 {
            return false;
        }
        // a TV stand clears windows in plan too (its screen would stand in front of the glass, above the sill the 3D test starts at)
        if class_of(o) == "TVStandFactory" && is_win(k.as_str(), b) {
            let (pa, pb) = (layout2d::prisms(state, name), layout2d::prisms(state, k));
            if pa.iter().any(|x| pb.iter().any(|y| layout2d::poly_dist(&x.poly, &y.poly) < 0.05)) {
                return false;
            }
        }
    }
    // the kitchen rules' door zones: no loose piece before an appliance's door
    if super::kitchen_rules::zone_clash(state, name) {
        return false;
    }
    let own = layout2d::room_of(state, name).map_or(0.0, |r| layout2d::outside_room_depth(state, name, &r));
    own <= 1e-3 && layout2d::outside_depth(state, name) <= 1e-4 && (RELAX_WALK.load(std::sync::atomic::Ordering::Relaxed) || layout2d::layout_ok(state, name))
}

/// set while a pass retries with the walkway rule off (its last resort); free_ok then checks overlaps and the room only
static RELAX_WALK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
        if hits.is_empty() && layout2d::outside_depth(state, name) < 1e-3 { let r = layout2d::room_of(state, name).unwrap_or_default(); let (lo, hi) = layout2d::lohi_world(state, name); eprintln!("GROUPDBG {name} at {:.2?}..{:.2?} walk {:?}", lo, hi, layout2d::walk_status(state, &r)); }
        eprintln!("GROUPDBG {name}: hits {hits:?} outside {:.3} own {:.3} zone {} layout {}", layout2d::outside_depth(state, name), layout2d::room_of(state, name).map_or(-1.0, |r| layout2d::outside_room_depth(state, name, &r)), super::kitchen_rules::zone_clash(state, name), layout2d::layout_ok(state, name));
    }
    state.set_pose(name, old);
    state.touch(name);
    false
}

pub(crate) fn lohi(state: &State, name: &str) -> (V3, V3) {
    let g = &state.objs[name].geom;
    (g.lo, g.hi)
}

/// kitchens: fridge and dishwasher at the ends of the counter run
pub fn kitchen(ctx: &mut Ctx, state: &mut State) -> Vec<String> {
    let mut log = vec![];
    // designed kitchens first (an L from a corner: counter + fridge on one leg, counter + range on the other); the rooms it lays out
    // skip the per-run appliance placement below
    // (KITCHEN_SEQ=0: the template and the run passes below; else each kitchen planned as a sequence of lengths, kitchen_plan.rs)
    let seq = std::env::var("KITCHEN_SEQ").map_or(true, |v| v != "0");
    let planned = if seq { super::kitchen_plan::plan_kitchens(ctx, state, &mut log) } else { vec![] };
    let mut done = if super::layout_rules::get().kitchen_template { kitchen_template_only(ctx, state, &mut log, &planned) } else { vec![] };
    done.extend(planned.iter().cloned());
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
                // (a fridge takes the end that leaves it out of a corner, by the kitchen rules, when either end does)
                let rings = layout2d::rings(&layout2d::room_floor(state, &room));
                let order: Vec<f64> = if cls == "FridgeFactory" {
                    let free_end = |state: &mut State, s: f64| -> bool {
                        let at = rp.apply([clo[0], if s > 0.0 { chi[1] + 0.01 } else { clo[1] - 0.01 }, clo[2]]);
                        let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
                        let old = state.objs[&p].pose;
                        let ok = try_place(state, &p, ry, anchor, [at[0], at[1], at[2]]) && !super::kitchen_rules::fridge_cornered(state, &p, &rings);
                        state.set_pose(&p, old); state.touch(&p);
                        ok
                    };
                    if !free_end(state, 1.0) && free_end(state, -1.0) { vec![-1.0, 1.0] } else { vec![1.0, -1.0] }
                } else { vec![1.0, -1.0] };
                for s in order {
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
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen" && !planned.contains(k)).map(|(k, _)| k.clone()).collect();
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
            // (a range goes mid-wall, where counter can meet it on both sides; a fridge into the corner)
            let is_range = class_of(&state.objs[&a]) == "OvenFactory";
            if place_on_wall(state, &a, &room, &move |_, _c, t, len, n| (if is_range { -(t - len / 2.0).abs() } else { -(t - hw).min(len - t - hw) }) - if n[0].abs() > 0.99 || n[1].abs() > 0.99 { 0.0 } else { 10.0 }) {
                log.push(format!("{room}: {a} slid into a corner"));
            } else {
                state.set_pose(&a, start);
                state.touch(&a);
            }
        }
        for i in 0..super::layout_rules::get().extra_runs {
            let others: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            if others.is_empty() { break }
            let name = format!("{room}_run{i}").replace('/', "_");
            if !new_piece(ctx, state, &mut rng, "KitchenSpaceFactory", &room, &name) { break }
            // the first run's floor relation (same class: the same face order); its wall relation names that run's wall
            let floor_rel: Vec<_> = state.objs[&others[0]].relations.iter().filter(|r| r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom")))).cloned().collect();
            if !floor_rel.is_empty() { state.objs.get_mut(&name).unwrap().relations = floor_rel; state.touch(&name); }
            // the shortest run to find a wall; the built-in fill grows it to the span
            let _ = match_run(ctx, state, &name, &others[0], 1.2);
            let n2 = name.clone();
            let os = others.clone();
            let aisle = super::layout_rules::get().kitchen_aisle;
            let aisle_ok = move |st: &State| os.iter().all(|r| { let d = layout2d::dist(st, r, &n2); d < 0.05 || d >= aisle }) && !super::kitchen_rules::run_breaks(st, &n2);
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
            // a short run on its own (not meeting another run or an appliance at a corner) read as a stray counter block
            let joined = state.objs.iter().any(|(k, o)| k != &name && matches!(class_of(o), "KitchenSpaceFactory" | "FridgeFactory" | "OvenFactory" | "DishwasherFactory")
                && layout2d::room_of(state, k).as_deref() == Some(room.as_str()) && layout2d::dist(state, k, &name) < 0.05);
            if w < 1.0 || (!joined && w < 2.0) {
                state.objs.shift_remove(&name);
                log.push(format!("{room}: extra run dropped ({w:.2} m, joined {joined})"));
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
        close_counter_gaps(ctx, state, &room, &mut log);
    }
    log
}

/// the kitchen rules' counter line: a range or dishwasher with a slot narrower than `counter_gap_max` beside it slides across it
/// (the run on its other side then grows after it); kept only when the kitchen scores better by the rules
fn close_counter_gaps(ctx: &mut Ctx, state: &mut State, room: &str, log: &mut Vec<String>) {
    let lr = super::layout_rules::get();
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    let apps: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "OvenFactory" | "DishwasherFactory") && layout2d::room_of(state, k).as_deref() == Some(room)).map(|(k, _)| k.clone()).collect();
    for a in apps {
        let gaps = super::kitchen_rules::side_gaps(state, &a, &rings);
        let Some(&(s, g)) = gaps.iter().find(|(_, g)| *g > 0.03 && *g < lr.counter_gap_max) else { continue };
        let before = super::kitchen_rules::eval(state, room, None).score;
        let saved = state.objs.clone();
        let p0 = state.objs[&a].pose;
        let d = g - 0.01;
        let mut p = p0;
        p.translate([p0.r[0][1] * s * d, p0.r[1][1] * s * d, 0.0]);
        let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == a)).map(|(k, _)| k.clone()).collect();
        move_with_children(state, &a, p);
        let ok = free_ok(state, &a) && { let _ = super::solver::fit_builtins(ctx, state, Some(room)); super::kitchen_rules::eval(state, room, None).score > before + 0.5 };
        if ok {
            log.push(format!("{room}: {a} slid {d:.2} m to close the counter gap"));
        } else {
            state.objs = saved;
            for k in std::iter::once(a.clone()).chain(kids) { state.touch(&k); }
            for k in state.objs.keys().filter(|k| class_of(&state.objs[k.as_str()]) == "KitchenSpaceFactory").cloned().collect::<Vec<_>>() { state.touch(&k); }
        }
    }
}

/// a new counter run sized like the kitchen's first: one counter depth and height through the room
pub(crate) fn match_run(ctx: &mut Ctx, state: &mut State, name: &str, like: &str, w: f64) -> bool {
    let Some(cs) = state.objs.get(like).and_then(|o| o.generator.as_ref()).and_then(super::solver::cur_size) else { return false };
    let base = state.objs[name].clone();
    super::solver::set_size(ctx, state, name, &base, [w, cs[1], cs[2]], 1.0)
}

/// place `name` with its back on a wall line through `c` (inward normal `n`), covering [start, start + its width] along `u` from `c`
pub(crate) fn place_along(state: &mut State, name: &str, c: [f64; 2], u: [f64; 2], n: [f64; 2], start: f64) -> bool {
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
fn kitchen_template_only(ctx: &mut Ctx, state: &mut State, log: &mut Vec<String>, skip: &[String]) -> Vec<String> {
    let mut done = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "kitchen" && !skip.contains(k)).map(|(k, _)| k.clone()).collect();
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
        let wb0 = state.objs[&leg_b].generator.as_ref().and_then(super::solver::cur_size).map_or(1.2, |c| c[0]);
        let _ = match_run(ctx, state, &leg_b, &proto, wb0);
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
        // the layout scored with the kitchen rules too (door zones, fridge off corners, work triangle: kitchen_rules.rs), the sink
        // taken at the middle of the run the range stands at (where the sinks pass puts it, barring a window)
        let rules = |state: &State| -> f64 {
            let ranged = [&proto, &leg_b].into_iter().filter(|k| state.objs.contains_key(k.as_str())).min_by(|a, b| layout2d::dist(state, a, &range).partial_cmp(&layout2d::dist(state, b, &range)).unwrap());
            let sink_at = ranged.map(|k| { let o = &state.objs[k.as_str()]; let q = o.pose.apply([o.geom.hi[0], (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]); [q[0], q[1]] });
            let e = super::kitchen_rules::eval(state, &room, sink_at);
            if ig_core::env_on!("KRULEDBG") { eprintln!("KRULE {room} {:.2} {:?}", e.score, e.broken); }
            e.score
        };
        // the fridge at each spot along leg A from its far end (0.2 m steps): the far end is often a corner
        let fpos: Vec<usize> = (0..12).collect();
        for ((c, ua, na, la, ub, nb, lb), fk) in cands.clone().into_iter().flat_map(|c| fpos.iter().map(move |k| (c, *k))) {
            state.objs = start_state.clone();
            // the template's pieces out of the room until each is placed: the solver's spots block each other (parked far off they
            // still count as the room's pieces for the walkway check)
            let parked: std::collections::HashMap<String, ObjState> = [&proto, &leg_b, &fridge, &range].into_iter().map(|k| (k.clone(), state.objs.shift_remove(k.as_str()).unwrap())).collect();
            let put = |state: &mut State, k: &str| { if !state.objs.contains_key(k) { state.objs.insert(k.to_string(), parked[k].clone()); state.touch(k); } };
            put(state, &fridge);
            // leg A: the fridge as far along as it fits, the run from the corner up to it
            let mut sf = None;
            {
                let st = la - fw - 0.02 - fk as f64 * 0.2;
                if st >= 1.2 && place_along(state, &fridge, c, ua, na, st) { sf = Some(st); }
            }
            let dbg = ig_core::env_on!("GROUPDBG") && fk == 0;
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
            // the range mid-leg, with counter on both sides (0.6 m or more toward the corner, 1.25 m or more beyond it, which the
            // extra-runs pass fills): at the leg's far end it often sat in another corner, turned along the neighbouring run
            let mut sr = None;
            put(state, &range);
            let mid = depth_a + 0.62 + ((lb - 1.25) - (depth_a + 0.62) - ow).max(0.0) / 2.0;
            let mut cands: Vec<f64> = (0..12).flat_map(|k| { let o = (k as f64 / 2.0).ceil() * 0.2 * if k % 2 == 0 { 1.0 } else { -1.0 }; [mid + o] }).filter(|&st| st >= depth_a + 0.62 && st + ow <= lb - 1.25).collect();
            let n_mid = cands.len();
            cands.extend((0..40).map(|k| lb - ow - 0.02 - k as f64 * 0.2).filter(|&st| st >= depth_a + 0.01));
            let mut range_mid = false;
            for (ci, st) in cands.into_iter().enumerate() {
                if place_along(state, &range, c, ub, nb, st) { sr = Some(st); range_mid = ci < n_mid; break }
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
            // (a range at the leg's end sits in a corner: any layout with it mid-leg wins)
            let score = wa + wb + if wb > 0.0 { 1.0 } else { 0.0 } + if range_mid { 4.0 } else { 0.0 } + rules(state);
            if ig_core::env_on!("KRULEDBG") { eprintln!("KTPL L corner {c:?} fk {fk} wa {wa:.2} wb {wb:.2} mid {range_mid} -> {score:.2}"); }
            if best.as_ref().map_or(true, |(s, _)| score > *s) { best = Some((score, state.objs.clone())); }
        }
        // one wall: from the corner a counter, the range mid-wall, a counter, the fridge at the far end (short second legs left the
        // range in a corner); the extra-runs pass then adds counters on the other walls
        for ((c, ua, na, la, _, _, _), fk) in cands.into_iter().flat_map(|c| fpos.iter().map(move |k| (c, *k))) {
            state.objs = start_state.clone();
            let parked: std::collections::HashMap<String, ObjState> = [&proto, &leg_b, &fridge, &range].into_iter().map(|k| (k.clone(), state.objs.shift_remove(k.as_str()).unwrap())).collect();
            let put = |state: &mut State, k: &str| { if !state.objs.contains_key(k) { state.objs.insert(k.to_string(), parked[k].clone()); state.touch(k); } };
            put(state, &fridge);
            let st = la - fw - 0.02 - fk as f64 * 0.2;
            if st < 1.2 + ow + 1.2 + 0.05 || !place_along(state, &fridge, c, ua, na, st) { continue }
            let sf = st;
            put(state, &range);
            let mid = (sf - ow) / 2.0;
            let mut sr = None;
            for k in 0..12 {
                let st = mid + (k as f64 / 2.0).ceil() * 0.2 * if k % 2 == 0 { 1.0 } else { -1.0 };
                if st < 1.22 || st + ow > sf - 1.22 { continue }
                if place_along(state, &range, c, ua, na, st) { sr = Some(st); break }
            }
            let Some(sr) = sr else { continue };
            // the counter from the corner to the range, and from the range to the fridge
            let (w1, s2, w2) = (sr - 0.02, sr + ow + 0.01, sf - 0.02 - (sr + ow + 0.01));
            put(state, &proto);
            let _ = super::solver::set_width(ctx, state, &proto, &base_a, w1, 1.0);
            if !place_along(state, &proto, c, ua, na, 0.0) { continue }
            put(state, &leg_b);
            let _ = super::solver::set_width(ctx, state, &leg_b, &base_b, w2, 1.0);
            if !place_along(state, &leg_b, c, ua, na, s2) { continue }
            let score = w1 + w2 + 1.0 + 4.0 + rules(state);
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
        let gap = super::layout_rules::get().fixture_gap;
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
                                    if s0 + sw + gap + ww > lb { break }
                                    if place_along(state, &sink, c, ub, nb, s0) && place_along(state, &wc, c, ub, nb, s0 + sw + gap) { placed = true; break }
                                }
                                ok &= placed;
                            } else {
                                let mut placed = false;
                                for k in 0..10 {
                                    let s0 = tw + gap + k as f64 * 0.15;
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
                            // the floor in front of the shower (0.6 m, its full width: the door) or the tub (0.45 m along its middle: getting in)
                            // stays clear of the toilet and the basin
                            let is_shower = class_of(&state.objs[&tub]) == "ShowerFactory";
                            let zone = { let (l, h) = lohi(state, &tub); let tp = state.objs[&tub].pose; let ins = if is_shower { 0.0 } else { (h[1] - l[1]) * 0.2 };
                                let d = if is_shower { 0.6 } else { 0.45 };
                                [[h[0] + 0.01, l[1] + ins], [h[0] + d, l[1] + ins], [h[0] + d, h[1] - ins], [h[0] + 0.01, h[1] - ins]].map(|q| { let w = tp.apply([q[0], q[1], 0.0]); [w[0], w[1]] }).to_vec() };
                            if [&wc, &sink].iter().any(|k| layout2d::prisms(state, k).iter().any(|p| layout2d::poly_dist(&p.poly, &zone) <= 0.0)) { continue }
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
pub(crate) fn new_piece(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, class: &str, room: &str, name: &str) -> bool {
    let Some((geom, gen)) = sample_rand_placeholder(ctx, rng, class) else { return false };
    let mut tags = TagSet::from([Tag::sem("Object"), Tag::sem("Furniture"), Tag::gen(class), Tag::obj(name)]);
    tags.update(&ctx.usage.usages_of(class));
    let mut o = ObjState::new(Arc::new(geom), super::geom::Pose::identity(), tags);
    o.generator = Some(gen);
    // (the room's own floor piece, else any room's, retargeted: a bedroom emptied for its bed had none, and the bed was added with no
    // floor relation and at z 0, half a metre into the floor)
    let floor_rel = |own: bool| state.objs.values().flat_map(|x| x.relations.iter().map(move |r| (x, r))).find(|(_, r)| (!own || r.target == room) && (own || r.target.contains('/')) && r.relation.geo().map_or(false, |g| g.child.contains(Tag::sub("Bottom"))));
    if let Some((ro, r)) = floor_rel(true).or_else(|| floor_rel(false)) {
        let mut r = r.clone(); r.target = room.to_string();
        o.relations.push(r);
        // standing at that piece's floor height (its bottom)
        let z = ro.pose.t[2] + ro.geom.lo[2] - o.geom.lo[2];
        let t = o.pose.t; o.pose.set_loc([t[0], t[1], z]);
    }
    // (no floor piece anywhere to copy: the floor height from the doors, whose bottoms stand on it)
    if o.relations.is_empty() {
        let fz = state.objs.iter().filter(|(k, _)| k.starts_with("door") && !k.starts_with("doorclear")).map(|(k, _)| layout2d::lohi_world(state, k).0[2]).fold(f64::INFINITY, f64::min);
        if fz.is_finite() { let t = o.pose.t; o.pose.set_loc([t[0], t[1], fz.max(0.0) - o.geom.lo[2]]); }
    }
    o.active = true;
    state.objs.insert(name.to_string(), o);
    state.touch(name);
    // the copied relation's child face index belongs to the other piece's geometry: re-matched to this one's downward face
    if let Some(g) = state.objs[name].relations.first().and_then(|r| r.relation.geo().cloned()) {
        let pl = state.planes(name, &g.child);
        // (the lowest of the downward faces: a bed's placeholder has its legs' bottoms and the frame's underside, and snapping the
        // frame's to the floor sank an added bed half a metre)
        let key = |i: usize| { let (n, p) = state.plane_nd(name, pl[i]); ((n[2] * 1000.0).round(), p[2]) };
        let best = (0..pl.len()).min_by(|&i, &j| key(i).partial_cmp(&key(j)).unwrap());
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
        // a pair from 0.9 m wide (one nightstand by a full / double bed read as unfinished in review)
        let want = if bhi[1] - blo[1] < 0.9 { 1 } else { 2 };
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
                    // (the template may have left the room: a nightstand that found no slot; copied from its lifted state)
                    let back = !state.objs.contains_key(t);
                    if back {
                        let Some((_, o)) = lifted.iter().find(|(k, _)| k == t) else { break };
                        state.objs.insert(t.clone(), o.clone());
                    }
                    let old_parent = state.objs[t].relations.iter().map(|r| r.target.clone()).find(|x| state.objs.get(x).map_or(false, |o| class_of(o) == "BedFactory"));
                    clone_obj(state, t, &new, old_parent.as_deref(), &bed);
                    if back { state.objs.shift_remove(t); }
                    new
                }
            };
            // the usual gap, then tighter, then a narrow nightstand (0.35 m): one side of the bed hard against a wall or wardrobe
            // left beds with a single nightstand
            let mut placed_here = false;
            // (a nightstand no deeper than 0.5 m: an oblong side table stood 0.9 m along the bed's side, reaching past its middle)
            if let Some(cs) = state.objs[&name].generator.as_ref().and_then(super::solver::cur_size) {
                if cs[1] > 0.55 {
                    let base = state.objs[&name].clone();
                    let _ = super::solver::set_size(ctx, state, &name, &base, [cs[0].min(0.6), 0.45, cs[2]], 1.0);
                    if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUPDBG nightstand {name}: {cs:.2?} -> depth 0.45"); }
                }
            }
            // (and looser: a bed whose frame flares past its box blocked the tight slot on one side)
            for (shrink, gap) in [(false, 0.04), (false, 0.02), (false, 0.08), (false, 0.12), (true, 0.02), (true, 0.08)] {
                if shrink {
                    let base = state.objs[&name].clone();
                    let _ = super::solver::set_width(ctx, state, &name, &base, 0.35, 1.0);
                }
                let (lo, hi) = lohi(state, &name);
                let at = bp.apply([blo[0], if s > 0.0 { bhi[1] + gap } else { blo[1] - gap }, blo[2]]);
                let anchor = [lo[0], if s > 0.0 { lo[1] } else { hi[1] }, lo[2]];
                if try_place(state, &name, by, anchor, at) { placed_here = true; break }
            }
            if placed_here {
                placed += 1;
                log.push(format!("nightstand {name} on the {} side of {bed}", if s > 0.0 { "+y" } else { "-y" }));
            } else if name.contains("_pair") || name.contains("_nightstand") {
                state.objs.shift_remove(&name);
            } else {
                // out of the room at once: left standing where it was, it blocked the other side's slot
                if state.objs.contains_key(&name) { layout2d::remove_tree(state, &name); }
                failed.push(name);
            }
        }
        // leftover nightstands of this bed found no slot by its headboard: removed (left where they were they stand by the foot)
        for k in stands.into_iter().chain(failed) {
            if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); }
            log.push(format!("nightstand {k}: no slot by the headboard, removed"));
            if let Some(room) = layout2d::room_of(state, &bed) { super::layers::complain(super::layers::Layer::Furniture, &room, "nightstand", at_of(state, &bed), "a free slot beside the headboard", false); }
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

pub(crate) fn room_type_of(room: &str) -> &str {
    room.split('_').next().unwrap_or("")
}

/// the most pieces of a class (or class group) a room keeps, whatever relation the solver used (the program's own counts are per
/// relation: plants placed as free floor pieces escaped the against-wall caps); None = no cap
fn cap(room_type: &str, cls: &str) -> Option<(&'static str, usize)> {
    let storage = ["SingleCabinetFactory", "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory"].contains(&cls) || (cls == "KitchenCabinetFactory" && room_type != "kitchen");
    Some(match cls {
        // (one in a dining room's corner: groups::dining_furnish; dotted around everywhere else)
        // (placed in corners by groups::dining_furnish / room_extras; the solver dotted them around the floor)
        "LargePlantContainerFactory" => ("plants", match room_type { "dining-room" | "bedroom" | "hallway" => 1, "living-room" => 2, _ => 0 }),
        "FloorLampFactory" => ("floor lamps", 0),
        // (letter balloons spelt random words on the walls, one over a window)
        "BalloonFactory" => ("balloons", 0),
        // (bar-height cocktail tables stood beside the dining table)
        "TableCocktailFactory" if room_type == "dining-room" => ("cocktail tables", 0),
        // (a bathroom wardrobe stood in front of the toilet; a bathroom's storage is the basin's)
        "WardrobeFactory" if room_type == "bathroom" => ("wardrobes", 0),
        "BeverageFridgeFactory" => ("beverage fridges", 0),
        "FridgeFactory" => ("fridges", if room_type == "kitchen" { 1 } else { 0 }),
        "OvenFactory" => ("ovens", if room_type == "kitchen" { 1 } else { 0 }),
        "DishwasherFactory" => ("dishwashers", if room_type == "kitchen" { 1 } else { 0 }),
        // (desks read as crammed beside the bed even in large bedrooms: none)
        "SimpleDeskFactory" => ("desks", if room_type == "bedroom" && super::layout_rules::get().desks { 1 } else { 0 }),
        "KitchenCabinetFactory" if room_type == "kitchen" => return None,
        // (outside a kitchen its generator ignores the size asked of it: 1.8 m built for 1.0 m)
        "KitchenCabinetFactory" => ("kitchen cabinets", 0),
        // (a lone cabinet on a living-room wall reads as stray: the TV stand is the room's storage)
        _ if storage => ("storage", match room_type { "utility" => 2, "dining-room" => 2, "living-room" => 1, _ => 0 }),
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
        let lr = super::layout_rules::get();
        if grp == "desks" && area < lr.desk_min_m2 { max = 0; }
        if grp == "tub / shower" && area < lr.tub_and_shower_m2 { max = 1; }
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
pub(crate) fn wall_segments(state: &State, room: &str) -> Vec<([f64; 2], [f64; 2], [f64; 2])> {
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

pub(crate) fn door_points(state: &State, room: &str) -> Vec<[f64; 2]> {
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
            // (a headboard against a window read as a bed blocking the window)
            let win = Tag::sem("Window");
            let on_window = state.objs.iter().any(|(k, o)| o.tags.contains(win.clone()) && layout2d::dist(state, k, bed) < 0.25);
            // (a window the wall layer will lay out on this wall: the headboard prefers another wall)
            let intent = Tag::sem("WindowIntent");
            let on_intent = state.objs.iter().any(|(_, o)| o.tags.contains(intent.clone()) && {
                let q = [o.pose.t[0], o.pose.t[1]];
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l2 = (dx * dx + dy * dy).max(1e-9);
                let tt = (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
                ((a[0] + dx * tt - q[0]).powi(2) + (a[1] + dy * tt - q[1]).powi(2)).sqrt() < 0.4
            });
            // (no slot at all: a bed wedged into a corner with no nightstand read as unfinished, worse than one by a window)
            let score = door_d.min(3.0) + side_room * 1.5 + if side_room >= 2.0 { 2.0 } else { 0.0 } - if side_room == 0.0 { 3.0 } else { 0.0 } + centred * 0.6 + straight * 2.0 - if on_window { 2.5 } else { 0.0 } - if on_intent { 2.0 } else { 0.0 };
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
    // the focal point of each living room: its TV stand, or a fireplace where there is none
    let mut tvs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "TVStandFactory").map(|(k, _)| k.clone()).collect();
    let with_tv: Vec<String> = tvs.iter().filter_map(|t| layout2d::room_of(state, t)).collect();
    tvs.extend(state.objs.iter().filter(|(k, o)| class_of(o) == "FireplaceFeature" && layout2d::room_of(state, k).map_or(false, |r| room_type_of(&r) == "living-room" && !with_tv.contains(&r))).map(|(k, _)| k.clone()));
    for tv in tvs {
        let Some(room) = layout2d::room_of(state, &tv) else { continue };
        let Some(sofa) = state.objs.iter().find(|(k, o)| class_of(o) == "SofaFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()) else { continue };
        // a fireplace is the room's focal point: the sofa faces it (the TV hangs over the mantel, so the stand goes); it is built
        // into its wall, so the searches that move the TV are skipped
        let fire = state.objs.iter().find(|(k, o)| class_of(o) == "FireplaceFeature" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone());
        let fixed = fire.is_some();
        let tv = match fire {
            Some(f) => { if tv != f { layout2d::remove_tree(state, &tv); } log.push(format!("{room}: seating faces the fireplace {f} (TV over the mantel)")); f }
            None => tv,
        };
        let tp = state.objs[&tv].pose;
        let (tlo, thi) = lohi(state, &tv);
        let rings = layout2d::rings(&layout2d::room_floor(state, &room));
        let mut ty = yaw(&tp);
        let mut front = tp.apply([thi[0], (tlo[1] + thi[1]) / 2.0, 0.0]);
        // a fireplace's front is not always its local +x (its depth can run along local y): the face toward the room is the one
        // with floor 0.5 m and 1.5 m in front of it
        if fixed {
            let (cx, cy) = ((tlo[0] + thi[0]) / 2.0, (tlo[1] + thi[1]) / 2.0);
            let y0 = yaw(&tp);
            let mut best_back = f64::INFINITY;
            // (and the opposite face on the wall: the side faces of a fireplace have floor in front of them too)
            for (face, back, ang) in [([thi[0], cy, 0.0], [tlo[0], cy, 0.0], 0.0), ([cx, thi[1], 0.0], [cx, tlo[1], 0.0], std::f64::consts::FRAC_PI_2),
                                      ([cx, tlo[1], 0.0], [cx, thi[1], 0.0], -std::f64::consts::FRAC_PI_2), ([tlo[0], cy, 0.0], [thi[0], cy, 0.0], std::f64::consts::PI)] {
                let (w, b) = (tp.apply(face), tp.apply(back));
                let a = y0 + ang;
                let ahead = |d: f64| layout2d::in_rings(&rings, [w[0] + a.cos() * d, w[1] + a.sin() * d]);
                // (the face whose back is nearest the wall, not the first under 0.25 m: a fireplace by a corner had a side face's
                // back 11 cm off the side wall, and the group was built facing sideways)
                let bd = layout2d::ring_dist(&rings, [b[0], b[1]]);
                if ahead(0.5) && ahead(1.5) && bd < 0.25 && bd < best_back { best_back = bd; ty = a; front = w; }
            }
        }
        let f = [ty.cos(), ty.sin()];
        let (slo, shi) = lohi(state, &sofa);
        let start = state.objs[&sofa].pose;
        // the coffee table and armchairs are placed around the sofa afterwards: out of the room while the sofa looks for its spot
        let mut lifted: Vec<(String, ObjState)> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "CoffeeTableFactory" | "ArmChairFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str()))
            .map(|(k, o)| (k.clone(), o.clone())).collect();
        // a living room of 20 m2 or more without an armchair gets one (placed with the others beside the coffee table, or left out)
        let room_area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        // (30 m2 or more: a pair, one each side of the coffee table, so a large room does not read as half empty)
        let lr = super::layout_rules::get();
        let want = if room_area >= lr.armchair_two_m2 { 2 } else if room_area >= lr.armchair_one_m2 { 1 } else { 0 };
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
        // the annealed group (LIVING_SA=0: the branch chain below); items that stood on the coffee table / armchairs follow them
        if std::env::var("LIVING_SA").map_or(true, |v| v != "0") {
            if let Some(l) = living_sa(ctx, state, rng, &room, &tv, &sofa, fixed, (ty, front), &lifted, want) {
                log.extend(l);
                for (c, co) in &kids {
                    let Some(pk) = co.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.contains_key(t)) else { continue };
                    let Some((_, po)) = lifted.iter().find(|(k, _)| *k == pk) else { continue };
                    let mut co = co.clone();
                    let (d, nt) = ([po.pose.t[0], po.pose.t[1]], state.objs[&pk].pose.t);
                    co.pose.translate([nt[0] - d[0], nt[1] - d[1], 0.0]);
                    state.objs.insert(c.clone(), co);
                    state.touch(c);
                }
                continue;
            }
        }
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
        for (a, b, n) in if fixed { vec![] } else { segs.clone() } {
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
                // (the short views are for narrow rooms: a 45 m2 room seated the sofa 1.4 m off the TV and the coffee table between
                // them closed the walkway)
                let vmin = if room_area >= 18.0 { lr.tv_view_min.max(2.0) } else { lr.tv_view_min };
                if !(vmin..=lr.tv_view_max).contains(&view) { continue }
                if !try_place(state, &sofa, n[1].atan2(n[0]), [slo[0], (slo[1] + shi[1]) / 2.0, slo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]) { continue }
                if !try_place(state, &tv, m[1].atan2(m[0]), [tlo[0], (tlo[1] + thi[1]) / 2.0, tlo[2]], [hp[0] + m[0] * 0.003, hp[1] + m[1] * 0.003, 0.0]) {
                    state.set_pose(&tv, tv_start); state.touch(&tv);
                    continue;
                }
                // the group in the room, not along its edge: midpoint of sofa and TV near the floor's centre
                let mid = [(base[0] + hp[0]) / 2.0, (base[1] + hp[1]) / 2.0];
                // a TV beside a door (the entrance, a room door) reads as misplaced
                let tv_door = ldoors.iter().map(|d| ((d[0] - hp[0]).powi(2) + (d[1] - hp[1]).powi(2)).sqrt()).fold(9.0f64, f64::min);
                let score = -(view - 2.6).abs() - if view < 1.8 { 1.5 } else { 0.0 } - (2.6 - tv_door).max(0.0) * 3.0 - (tt - len / 2.0).abs() / len - ((mid[0] - fc[0]).powi(2) + (mid[1] - fc[1]).powi(2)).sqrt() * 0.6;
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
            for (a, b, n) in if fixed { vec![] } else { segs.clone() } {
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
                    let nd = (((lr.float_max - lr.float_min) / 0.2).round() as usize).min(20);
                    for k in 0..=nd {
                        let d = lr.float_min + k as f64 * 0.2;
                        let at = [tfront[0] + n[0] * d, tfront[1] + n[1] * d, 0.0];
                        // sofa front (local +x) toward the TV
                        if !try_place(state, &sofa, (-n[1]).atan2(-n[0]), [shi[0], (slo[1] + shi[1]) / 2.0, slo[2]], at) { continue }
                        let back = state.objs[&sofa].pose.apply([slo[0], (slo[1] + shi[1]) / 2.0, 0.0]);
                        let on_wall = layout2d::ring_dist(&rings, [back[0], back[1]]) < 0.15;
                        let mid = [(base[0] + back[0]) / 2.0, (base[1] + back[1]) / 2.0];
                        // (a floating sofa reads as unanchored: its back on a wall is worth a longer view)
                        // (raters read a floating sofa as "back to the room" and a TV beside a door as misplaced: both weigh more)
                        let score = if on_wall { 5.0 } else { 0.0 } - (d - 2.6).abs() - (2.6 - tv_door).max(0.0) * 3.0
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
                    if !(1.5..=lr.side_wall_max).contains(&dist) { continue }
                    let cos = (v[0] * n[0] + v[1] * n[1]) / dist;
                    if cos < 0.77 { continue }
                    // and in front of the screen / firebox, not off to its side looking at it edge-on (a sofa against the far wall
                    // of a 45 m2 room watched the fireplace from 60 degrees off its axis)
                    let ahead = -(v[0] * f[0] + v[1] * f[1]) / dist;
                    if ahead < 0.8 { continue }
                    if !try_place(state, &sofa, n[1].atan2(n[0]), [slo[0], (slo[1] + shi[1]) / 2.0, slo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]) { continue }
                    // (centred on a fireplace: it cannot move, so the sofa lines up with it)
                    let score = 100.0 + cos * if fixed { 10.0 } else { 3.0 } - (dist - 2.6).abs();
                    if best.as_ref().map_or(true, |(s, _, _)| score > *s) { best = Some((score, state.objs[&sofa].pose, dist)); walled = true; }
                }
            }
        }
        // otherwise: facing the TV where it stands, 1.3 m (small rooms) to 4.2 m; with the walkway rule relaxed as a last resort
        // (rooms with many doors, e.g. a merged hallway's, left no spot that keeps every door-to-door path, and the sofa stayed
        // wherever the solver put it)
        for relax in [false, true] {
        if relax && (best.is_some() || pair.is_some() || walled) { break }
        RELAX_WALK.store(relax, std::sync::atomic::Ordering::Relaxed);
        for di in 0..=24 {
            if pair.is_some() || walled { break }
            let d = 1.3 + di as f64 * 0.12;
            // (a fireplace cannot move: the sofa may sit further off its axis)
            let lats: &[f64] = if fixed { &[0.0, 0.3, -0.3, 0.6, -0.6, 0.9, -0.9] } else { &[0.0, 0.3, -0.3] };
            for &lat in lats {
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
        }
        RELAX_WALK.store(false, std::sync::atomic::Ordering::Relaxed);
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
                super::layers::complain(super::layers::Layer::Room, &room, "seating", at_of(state, &tv), "a wall or floor spot for the sofa facing the TV / fireplace", true);
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
                let target = ((shi[1] - slo[1]) * lr.coffee_ratio).clamp(0.6, 1.6);
                if (target - (chi[1] - clo[1])).abs() > 0.1 {
                    let base = state.objs[k].clone();
                    let _ = super::solver::set_width(ctx, state, k, &base, target, 1.0);
                }
                // before the sofa: the usual gaps, then shifted sideways, then a smaller table (a 0.6 sofa-width one)
                'c: for (shrink, lats) in [(false, &[0.0f64][..]), (false, &[0.3, -0.3][..]), (true, &[0.0, 0.3, -0.3][..])] {
                    if shrink {
                        let base = state.objs[k].clone();
                        let _ = super::solver::set_width(ctx, state, k, &base, ((shi[1] - slo[1]) * 0.6).clamp(0.6, 1.6), 1.0);
                    }
                    let (clo, chi) = lohi(state, k);
                    for gap in [0.45f64, 0.35, 0.55, 0.3, 0.7] {
                        for &lat in lats {
                            let off = gap + (chi[0] - clo[0]) / 2.0;
                            let at = [sfront[0] + sf[0] * off - sf[1] * lat, sfront[1] + sf[1] * off + sf[0] * lat, 0.0];
                            if try_place(state, k, sy, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], at) {
                                log.push(format!("{room}: coffee table {gap:.2} m before the sofa"));
                                coffee_c = Some([at[0], at[1]]);
                                coffee_hw = (chi[1] - clo[1]) / 2.0;
                                ok = true;
                                break 'c;
                            }
                        }
                    }
                }
                // no spot before the sofa: where the solver had it only when that is in front of the sofa (behind it or at its
                // side it read as detached), else left out
                if !ok {
                    let t = state.objs[k].pose.t;
                    let ahead = (t[0] - sfront[0]) * sf[0] + (t[1] - sfront[1]) * sf[1];
                    ok = ahead > 0.0 && ahead < 1.5 && free_ok(state, k);
                }
            } else {
                let saved = state.objs[k].pose;
                let c = coffee_c.unwrap_or([sfront[0] + sf[0] * 0.8, sfront[1] + sf[1] * 0.8]);
                // beside the coffee table, turned to face it across a 0.45 m gap: the side away from the room's walls first, then
                // the other; else where the solver had it
                let half = (chi[0] - clo[0]) / 2.0;
                // (then slid along the sofa's axis: toward the TV, or back level with the sofa's arm)
                'a: for (gap, fwd) in [(0.45f64, 0.0f64), (0.6, 0.0), (0.35, 0.0), (0.45, 0.35), (0.45, -0.35), (0.45, 0.7), (0.6, -0.35)] {
                    for side in [1.0f64, -1.0] {
                        let dd = coffee_hw + gap + half;
                        let at = [c[0] - sf[1] * side * dd + sf[0] * fwd, c[1] + sf[0] * side * dd + sf[1] * fwd, 0.0];
                        let face = (c[1] - at[1]).atan2(c[0] - at[0]);
                        if try_place(state, k, face, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], at) { ok = true; break 'a; }
                    }
                }
                if !ok {
                    state.set_pose(k, saved);
                    state.touch(k);
                    // (only near the group and on the sofa's front side: one behind the sofa faced its back)
                    let t = state.objs[k].pose.t;
                    let ahead = (t[0] - sfront[0]) * sf[0] + (t[1] - sfront[1]) * sf[1];
                    let near = ((t[0] - c[0]).powi(2) + (t[1] - c[1]).powi(2)).sqrt() < 2.2;
                    // (nor between the sofa and the TV: one in the line of sight blocked the screen)
                    let lateral = ((t[0] - sfront[0]) * -sf[1] + (t[1] - sfront[1]) * sf[0]).abs();
                    let in_view = lateral < (shi[1] - slo[1]) / 2.0 + 0.2 && ahead > 0.9;
                    ok = ahead > 0.1 && near && !in_view && free_ok(state, k);
                    // left where the solver had it: square to the room, turned toward the coffee table (a chair at 45 degrees read
                    // as dropped)
                    if ok {
                        let p0 = state.objs[k].pose;
                        let here = p0.apply([(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, 0.0]);
                        let to = (c[1] - here[1]).atan2(c[0] - here[0]);
                        let snapped = (to / std::f64::consts::FRAC_PI_2).round() * std::f64::consts::FRAC_PI_2;
                        if (yaw(&p0) - snapped).sin().abs() > 0.02 || (yaw(&p0) - snapped).cos() < 0.0 {
                            if !try_place(state, k, snapped, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], [here[0], here[1], 0.0]) {
                                // (no square turn fits there: left out rather than left at a slant)
                                state.set_pose(k, p0);
                                state.touch(k);
                                ok = false;
                            }
                        }
                    }
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
                super::layers::complain(super::layers::Layer::Furniture, &room, "armchair", at_of(state, &sofa), "floor beside the coffee table for an armchair", false);
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
        if room_area < super::layout_rules::get().small_dining_m2 {
            let base = state.objs[&t].clone();
            if let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) {
                if cs[0] > 1.05 && !super::solver::set_width(ctx, state, &t, &base, 1.0, 1.0) && !free_ok(state, &t) {
                    *state.objs.get_mut(&t).unwrap() = base;
                    state.touch(&t);
                }
            }
        }
        // a table up to 1.6 m is round some of the time (layout_rules.round_table_p): squared up to its depth (0.9-1.2 m),
        // one chair per side, tagged for the drawing and the 3D build
        let lr0 = super::layout_rules::get();
        {
            let (l, h) = lohi(state, &t);
            let (w, dp) = (h[1] - l[1], h[0] - l[0]);
            // (a table piece decided itself: round when its top's collision is round -- tagged, its chairs set round it)
            let piece_round = state.objs[&t].generator.as_ref().map_or(false, |g| g.piece.is_some());
            if piece_round {
                if super::solver::round_top(&state.objs[&t]) && (w - dp).abs() < 0.02 {
                    state.objs.get_mut(&t).unwrap().tags.insert(Tag::sem("RoundTable"));
                    state.touch(&t);
                    log.push(format!("{room}: table {t} round"));
                }
            } else if w <= 1.6 && rng.f64() < lr0.round_table_p {
                let base = state.objs[&t].clone();
                // (both sides: squared by width alone, a 0.81 m deep table was laid out 0.9 m wide and built 0.81 m round)
                let dd = dp.clamp(0.9, 1.2);
                let h = base.generator.as_ref().and_then(super::solver::cur_size).map_or(0.75, |c| c[2]);
                if !super::solver::set_size(ctx, state, &t, &base, [dd, dd, h], 1.0) { let _ = super::solver::set_width(ctx, state, &t, &base, dd, 1.0); }
                state.objs.get_mut(&t).unwrap().tags.insert(Tag::sem("RoundTable"));
                state.touch(&t);
                log.push(format!("{room}: table {t} round"));
            }
        }
        // tables in dining rooms go to the middle; eat-in tables (kitchen / living room) keep their spot clear of the seating
        let placed = if room_type_of(&room) == "dining-room" { center_table(state, &t, &room) } else { true };
        for (k, o) in saved { state.objs.insert(k.clone(), o); state.touch(&k); }
        // a full set of chairs: copies of the first chair
        let tw = { let (l2, h2) = lohi(state, &t); let _ = (lo, hi); h2[1] - l2[1] };
        // one chair per 0.6 m on each long side and one at each end: 1.4 m -> 6, 2.1 m -> 8
        // (a small table, under 1.2 m: one chair each side, four; two on adjacent sides read as lopsided)
        let lr = super::layout_rules::get();
        // (eight chairs read as crowded in a dining room under 20 m2: six at most there)
        let room_m2 = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        let want = if tw < lr.small_table { 4 } else { (2 * ((tw / lr.chair_spacing.max(0.4)).floor() as usize).max(1)).clamp(2, 6) + if lr.end_chairs { 2 } else { 0 } };
        let want = if room_m2 < 20.0 { want.min(6) } else { want };
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
        if !placed { super::layers::complain(super::layers::Layer::Room, &room, "dining", at_of(state, &t), "open floor in the middle for the table and its chairs", true); }
    }
    log
}

/// bed placement before the nightstands (see `bedroom_and_desk`)
pub fn beds(state: &mut State) -> Vec<String> {
    let beds: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "BedFactory").map(|(k, _)| k.clone()).collect();
    let mut log = vec![];
    for b in beds {
        match place_bed(state, &b) {
            Some(l) => log.push(l),
            // no wall spot: the bed stays where the solver left it (floating, or across the door)
            None => if let Some(room) = layout2d::room_of(state, &b) { super::layers::complain(super::layers::Layer::Room, &room, "bed", at_of(state, &b), "a wall for the headboard", true); },
        }
    }
    log
}

/// a piece placed against a wall of `room`: every wall spot (0.2 m steps) where the placement is valid is scored by `score(centre,
/// wall position t, wall length)` and the best one kept; false (and the piece back where it was) when none is valid
fn place_on_wall(state: &mut State, name: &str, room: &str, score: &dyn Fn(&State, [f64; 2], f64, f64, [f64; 2]) -> f64) -> bool {
    place_on_wall_step(state, name, room, score, 0.2)
}

/// `place_on_wall` trying spots every `step` along each wall (a single bed's 1.12 m slot between a door clearance and a diagonal wall
/// fell between the 0.2 m spots)
fn place_on_wall_step(state: &mut State, name: &str, room: &str, score: &dyn Fn(&State, [f64; 2], f64, f64, [f64; 2]) -> f64, step: f64) -> bool {
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
        let steps = ((len - w) / step).floor() as usize;
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
        if rt == "living-room" {
            // the solver's plants go before the TV and the sofa look for their walls (room_extras puts plants in free corners
            // afterwards), and so does its storage where room_extras adds a bookcase instead
            let lr = super::layout_rules::get();
            let area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
            let built_in = has(state, "BookshelfWallFeature");
            let gone: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::room_of(state, k).as_deref() == Some(room.as_str())
                && (class_of(o) == "LargePlantContainerFactory" || (lr.living_bookcase && area >= 20.0 && !built_in
                    && matches!(class_of(o), "SingleCabinetFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory"))))
                .map(|(k, _)| k.clone()).collect();
            for k in gone { layout2d::remove_tree(state, &k); log.push(format!("{room}: cleared {k} for the TV and sofa")); }
        }
        if rt == "living-room" && !has(state, "TVStandFactory") {
            let name = format!("{room}_tvstand").replace('/', "_");
            if new_piece(ctx, state, rng, "TVStandFactory", &room, &name) {
                let dd = door_d.clone();
                let score = |_: &State, c: [f64; 2], t: f64, len: f64, _n: [f64; 2]| dd(c).min(3.0) + len * 0.15 - (t - len / 2.0).abs() * 0.5;
                let mut ok = place_on_wall(state, &name, &room, &score);
                // (no wall spot keeps every walkway in a room with many doors: a living room without a TV read as unfinished)
                if !ok {
                    RELAX_WALK.store(true, std::sync::atomic::Ordering::Relaxed);
                    ok = place_on_wall(state, &name, &room, &score);
                    RELAX_WALK.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                if ok { log.push(format!("{room}: added a TV stand")); } else {
                    state.objs.shift_remove(&name);
                    super::layers::complain(super::layers::Layer::Room, &room, "tv", None, "a free wall for the TV stand", true);
                }
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
        // a dining room the solver left without a table: one in the middle of the room, smaller if it must be
        if rt == "dining-room" && !has(state, "TableDiningFactory") {
            let name = format!("{room}_table").replace('/', "_");
            if new_piece(ctx, state, rng, "TableDiningFactory", &room, &name) {
                let mut ok = center_table(state, &name, &room);
                for (w, d) in [(1.6, 0.9), (1.2, 0.8), (0.9, 0.9)] {
                    if ok { break }
                    let base = state.objs[&name].clone();
                    let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) else { break };
                    super::solver::set_size(ctx, state, &name, &base, [w, d, cs[2]], 1.0);
                    ok = center_table(state, &name, &room);
                }
                // (its chairs: the dining pass makes the set)
                if ok { log.push(format!("{room}: added a table")); log.extend(dining(ctx, state, rng)); } else { state.objs.shift_remove(&name); }
            }
        }
        // a kitchen the plan found no layout for: at least a counter, as long as a free wall takes (the sink pass puts a sink on it)
        if rt == "kitchen" && !has(state, "KitchenSpaceFactory") {
            let name = format!("{room}_krun").replace('/', "_");
            if new_piece(ctx, state, rng, "KitchenSpaceFactory", &room, &name) {
                let dd = door_d.clone();
                let score = |_: &State, c: [f64; 2], _t: f64, len: f64, _n: [f64; 2]| dd(c).min(2.0) + len * 0.2;
                let mut ok = false;
                // (what stands on the walls goes if it must: a counter before a cabinet, a range or a fridge)
                for clear in ["", "KitchenCabinetFactory", "OvenFactory", "FridgeFactory"] {
                    if !clear.is_empty() { for k in state.objs.iter().filter(|(k, o)| class_of(o) == clear && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect::<Vec<_>>() { layout2d::remove_tree(state, &k); log.push(format!("{room}: removed {k} for a counter")); } }
                    for w in [1.8, 1.4, 1.1, 0.8, 0.6] {
                        let base = state.objs[&name].clone();
                        let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) else { break };
                        if !super::solver::set_size(ctx, state, &name, &base, [w, cs[1].min(0.65), cs[2]], 1.0) && state.objs[&name].generator.as_ref().and_then(super::solver::cur_size).map_or(true, |c| (c[0] - w).abs() > 0.03) { continue }
                        ok = place_on_wall_step(state, &name, &room, &score, 0.1);
                        if ok { log.push(format!("{room}: a {w:.1} m counter on a free wall")); break }
                    }
                    if ok { break }
                }
                if !ok { state.objs.shift_remove(&name); }
            }
        }
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
                // (its bedding tucked in before anything in the room gives way)
                if !ok && super::solver::tuck_piece(state, &name) { ok = place_on_wall(state, &name, &room, &score); }
                if !ok {
                    // (plants and pictures first: a spot blocked only by a corner plant or a picture over it left the room bedless)
                    let mut crowd: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "LargePlantContainerFactory" | "WallArtFactory" | "MirrorFactory" | "WardrobeFactory" | "LargeShelfFactory" | "SimpleDeskFactory" | "SingleCabinetFactory" | "WindowSeatFeature" | "BookshelfWallFeature")
                        && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
                    // (built-ins last: a window seat on the one wall the doors left)
                    crowd.sort_by_key(|k| match class_of(&state.objs[k.as_str()]) { "LargePlantContainerFactory" | "WallArtFactory" | "MirrorFactory" => 0, "WindowSeatFeature" | "BookshelfWallFeature" => 2, _ => 1 });
                    for k in crowd {
                        layout2d::remove_tree(state, &k);
                        log.push(format!("{room}: removed {k} to fit a bed"));
                        ok = place_on_wall(state, &name, &room, &score);
                        if ok { break; }
                    }
                }
                // (then fresh draws of the bed, the narrower ones only -- a quarter are singles: a 3 x 3 m room took no king)
                if !ok {
                    // (the first bed's floor relation and height: the room was cleared since, and a redrawn bed found nothing to copy them from)
                    let (rel0, floor0) = { let o = &state.objs[&name]; (o.relations.clone(), o.pose.t[2] + o.geom.lo[2]) };
                    for _ in 0..16 {
                        state.objs.shift_remove(&name);
                        super::solver::forget_placeholder("BedFactory");
                        if !new_piece(ctx, state, rng, "BedFactory", &room, &name) { break }
                        if state.objs[&name].relations.is_empty() && !rel0.is_empty() {
                            let o = state.objs.get_mut(&name).unwrap();
                            o.relations = rel0.clone();
                            let t = o.pose.t; let lz = o.geom.lo[2]; o.pose.set_loc([t[0], t[1], floor0 - lz]);
                            state.touch(&name);
                        }
                        super::solver::tuck_piece(state, &name);
                        let bw = { let g = &state.objs[&name].geom; g.hi[1] - g.lo[1] };
                        if bw > 1.55 { continue }
                        ok = place_on_wall_step(state, &name, &room, &score, 0.05);
                        if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUPDBG bedredraw {name}: {bw:.2} wide, placed {ok}"); }
                        if ok { log.push(format!("{room}: bed redrawn {bw:.2} m wide to fit")); break }
                    }
                    if !state.objs.contains_key(&name) { continue }
                    // (the last of them made a compact single, which fits where nothing else does: an angled room's door takes the rest)
                    if !ok && super::solver::tuck_piece(state, &name) {
                        ok = place_on_wall_step(state, &name, &room, &score, 0.05);
                        if ok { log.push(format!("{room}: a compact bed to fit")); }
                    }
                }
                if std::env::var_os("GROUPDBG").is_some() && !ok { let (a, b) = state.objs.get(room.as_str()).and_then(|r| r.polygon.as_ref()).map_or(([0.0; 2], [0.0; 2]), |g| { let b = crate::shp::bounds(g); ([b.0, b.1], [b.2, b.3]) }); eprintln!("GROUPDBG bedless {room} {:.2} x {:.2}", b[0] - a[0], b[1] - a[1]);
                    for (x, y, n) in wall_segments(state, &room) { println!("GROUPDBG  wall {:.2?}-{:.2?} n {:.2?}", x, y, n); }
                    for (k, _) in state.objs.iter().filter(|(k, _)| k.starts_with("door") || k.starts_with("window") || k.starts_with("passage")) { let (l, h) = layout2d::lohi_world(state, k); if (0..2).all(|d| l[d] < b[d] + 0.5 && a[d] - 0.5 < h[d]) { println!("GROUPDBG  {k} {:.2?}..{:.2?}", l, h); } } }
                if ok { log.push(format!("{room}: added a bed")); } else { state.objs.shift_remove(&name); }
            }
        }
        // a living room left without anywhere to sit (the living group seated no sofa in an angled room): a sofa against the longest
        // free wall, else an armchair
        if rt == "living-room" && !has(state, "SofaFactory") && !has(state, "ArmChairFactory") {
            // (what crowds the room gives way, one at a time: a coffee table and a TV left with no seat to go with them, then storage)
            let mut crowd: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "CoffeeTableFactory" | "LargePlantContainerFactory" | "SideTableFactory" | "SingleCabinetFactory" | "SimpleBookcaseFactory" | "CellShelfFactory" | "LargeShelfFactory" | "TVStandFactory")
                && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            crowd.sort_by_key(|k| match class_of(&state.objs[k.as_str()]) { "CoffeeTableFactory" | "LargePlantContainerFactory" | "SideTableFactory" => 0, "TVStandFactory" => 2, _ => 1 });
            let mut crowd = crowd.into_iter();
            'seat: loop {
                for cls in ["SofaFactory", "ArmChairFactory"] {
                    let name = format!("{room}_seat").replace('/', "_");
                    if !new_piece(ctx, state, rng, cls, &room, &name) { continue }
                    let dd = door_d.clone();
                    let score = move |_: &State, c: [f64; 2], t: f64, len: f64, _n: [f64; 2]| len - (t - len / 2.0).abs() + dd(c).min(1.5);
                    if place_on_wall(state, &name, &room, &score) { log.push(format!("{room}: added a {cls}")); break 'seat }
                    state.objs.shift_remove(&name);
                }
                let Some(k) = crowd.next() else { break };
                layout2d::remove_tree(state, &k);
                log.push(format!("{room}: removed {k} to fit a seat"));
            }
        }
        // a bathroom the solver left without a toilet (a 1.5 x 1.5 m room): one on the wall farthest from the door, the shelving and
        // plants that crowd the room giving way if no wall takes it
        if rt == "bathroom" && !has(state, "ToiletFactory") {
            let name = format!("{room}_toilet").replace('/', "_");
            if new_piece(ctx, state, rng, "ToiletFactory", &room, &name) {
                let dd = door_d.clone();
                let score = move |_: &State, c: [f64; 2], _t: f64, _len: f64, _n: [f64; 2]| dd(c);
                let mut ok = place_on_wall(state, &name, &room, &score);
                if !ok {
                    let crowd: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "LargePlantContainerFactory" | "LargeShelfFactory" | "SingleCabinetFactory" | "HardwareFactory" | "WallArtFactory" | "MirrorFactory")
                        && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
                    for k in crowd {
                        layout2d::remove_tree(state, &k);
                        ok = place_on_wall(state, &name, &room, &score);
                        if ok { break; }
                    }
                }
                if ok { log.push(format!("{room}: added a toilet")); } else { state.objs.shift_remove(&name); }
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
        if d0 >= super::layout_rules::get().toilet_door_min && c0 >= 0.15 { continue }
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
    // a bedroom with its own closet (a door between them) keeps its clothes there: a wardrobe in the bedroom too read as a
    // duplicate (each room is solved on its own copy, so the closet's wardrobe is not in this state; the door clearances are)
    let closets: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "closet").map(|(k, _)| k.clone()).collect();
    let bedrooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "bedroom").map(|(k, _)| k.clone()).collect();
    for room in bedrooms {
        let bd = door_points(state, &room);
        // (a closet too small for a wardrobe gets open shelving: the bedroom keeps its wardrobe then)
        let fits = |c: &str| state.objs[c].polygon.as_ref().map_or(false, |p| {
            let r = layout2d::rings(p);
            let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for q in r.first().into_iter().flatten() { x0 = x0.min(q[0]); y0 = y0.min(q[1]); x1 = x1.max(q[0]); y1 = y1.max(q[1]); }
            (x1 - x0).min(y1 - y0) >= 1.5 && (x1 - x0).max(y1 - y0) >= 1.8
        });
        let own = closets.iter().any(|c| fits(c) && door_points(state, c).iter().any(|p| bd.iter().any(|q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt() < 1.5)));
        if !own { continue }
        let ws: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "WardrobeFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        for w in ws { layout2d::remove_tree(state, &w); log.push(format!("{room}: wardrobe {w} removed (the bedroom has its own closet)")); }
    }
    let ws: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| class_of(o) == "WardrobeFactory")
        .filter_map(|(k, _)| layout2d::room_of(state, k).filter(|r| room_type_of(r) == "bedroom").map(|r| (k.clone(), r))).collect();
    for (w, room) in ws {
        let beds: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "BedFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
        let Some(bed) = beds.first().cloned() else { continue };
        let d0 = layout2d::dist(state, &w, &bed);
        if d0 >= super::layout_rules::get().wardrobe_bed_min { continue }
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

/// dining rooms: the solver's storage and plants out before the chairs are seated (a cabinet it left beside the table took a chair's
/// slot; dining_furnish puts a sideboard and a corner plant back afterwards)
pub fn dining_clear(state: &mut State, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let gone: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "SingleCabinetFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory" | "LargePlantContainerFactory")
        && layout2d::room_of(state, k).map_or(false, |r| room_type_of(&r) == "dining-room" && only_room.map_or(true, |o| o == r.as_str()))).map(|(k, _)| k.clone()).collect();
    for k in gone { layout2d::remove_tree(state, &k); log.push(format!("dining: cleared {k} before seating the chairs")); }
    log
}

/// dining rooms furnished past the table: a sideboard on the best free wall (centred, away from the doors, 0.9-2.5 m from the
/// table so the chairs pull out), a tall cabinet on another wall in large rooms, a rug for the rugs pass to put under the table
/// (layout_rules: dining_sideboard, dining_cabinet_m2, dining_rug)
pub fn dining_furnish(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let lr = super::layout_rules::get();
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && room_type_of(k) == "dining-room" && only_room.map_or(true, |r| r == k.as_str())).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let in_room = |st: &State, f: &dyn Fn(&str) -> bool| st.objs.iter().filter(|(k, o)| f(class_of(o)) && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect::<Vec<_>>();
        let Some(table) = in_room(state, &|c| c == "TableDiningFactory").first().cloned() else { continue };
        let area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        let doors = door_points(state, &room);
        let door_d = move |c: [f64; 2]| doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(4.0f64, f64::min);
        let mut pieces = vec![];
        // (the sideboard is a bookcase placeholder widened along its wall: the cabinet factory's placeholder is its own small asset)
        if lr.dining_sideboard { pieces.push(("SimpleBookcaseFactory", "sideboard")); }
        if area >= lr.dining_cabinet_m2 { pieces.push(("SimpleBookcaseFactory", "cabinet")); }
        // the solver's storage here stands wherever it landed: replaced by the placed pieces
        for k in in_room(state, &|c| matches!(c, "SingleCabinetFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory" | "LargePlantContainerFactory")) { layout2d::remove_tree(state, &k); }
        let mut placed: Vec<String> = vec![];
        for (cls, what) in pieces {
            let name = format!("{room}_{what}").replace('/', "_");
            if !new_piece(ctx, state, rng, cls, &room, &name) { log.push(format!("{room}: no {cls} placeholder")); continue }
            if what == "sideboard" {
                let base = state.objs[&name].clone();
                let _ = super::solver::set_width(ctx, state, &name, &base, 1.3, 1.0);
            }
            // scored on the placed piece (place_on_wall scores after placing): the table gap, the doors, centred on its wall
            // (a second piece on another wall than the first: two cabinets side by side read as a storage wall)
            let (tg, nm, dd, pl) = (table.clone(), name.clone(), door_d.clone(), placed.clone());
            let ok = place_on_wall(state, &name, &room, &move |st, c, t, len, _n| {
                let gap = layout2d::dist(st, &tg, &nm);
                if !(0.9..=2.5).contains(&gap) { return -100.0 }
                let beside = pl.iter().any(|p| layout2d::dist(st, p, &nm) < 0.6);
                dd(c).min(1.5) - (t - len / 2.0).abs() / len.max(0.1) + gap.min(1.5) * 0.5 - if beside { 3.0 } else { 0.0 }
            });
            let gap = layout2d::dist(state, &table, &name);
            if ok && (0.9..=2.5).contains(&gap) { log.push(format!("{room}: {what} added ({gap:.2} m from the table)")); placed.push(name.clone()); } else { state.objs.shift_remove(&name); }
        }
        // a potted plant in a free corner (the far end of a wall, away from the doors and the table)
        if lr.dining_plant {
            let name = format!("{room}_plant").replace('/', "_");
            if new_piece(ctx, state, rng, "LargePlantContainerFactory", &room, &name) {
                let (tg, nm, dd) = (table.clone(), name.clone(), door_d.clone());
                let ok = place_on_wall(state, &name, &room, &move |st, c, t, len, _n| {
                    let corner = t.min(len - t);
                    let gap = layout2d::dist(st, &tg, &nm);
                    if gap < 0.7 { return -100.0 }
                    -corner * 2.0 + dd(c).min(1.5)
                });
                if ok { log.push(format!("{room}: plant in a corner")); } else { state.objs.shift_remove(&name); }
            }
        }
        if lr.dining_rug && in_room(state, &|c| c == "RugFactory").is_empty() {
            let name = format!("{room}_rug").replace('/', "_");
            if new_piece(ctx, state, rng, "RugFactory", &room, &name) { log.push(format!("{room}: rug added for the table")); }
        }
    }
    log
}

/// more furnishing past the anchors (layout_rules): potted plants in free corners of living rooms (two from 20 m2), bedrooms
/// (14 m2 and up) and hallways (6 m2 and up), and a bookcase on a free wall of a living room of 20 m2 or more without built-in
/// shelves, clear of the sofa and the TV. The solver's own plants and storage in those rooms go first (dotted around the floor).
pub fn room_extras(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let lr = super::layout_rules::get();
    let rooms: Vec<String> = state.objs.iter().filter(|(k, o)| layout2d::is_room(o) && matches!(room_type_of(k), "living-room" | "bedroom" | "hallway") && only_room.map_or(true, |r| r == k.as_str())).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let rt = room_type_of(&room).to_string();
        let area = state.objs[&room].polygon.as_ref().map_or(0.0, |p| crate::shp::area(p));
        let in_room = |st: &State, f: &dyn Fn(&str) -> bool| st.objs.iter().filter(|(k, o)| f(class_of(o)) && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect::<Vec<_>>();
        for k in in_room(state, &|c| c == "LargePlantContainerFactory") { layout2d::remove_tree(state, &k); }
        let doors = door_points(state, &room);
        let door_d = move |c: [f64; 2]| doors.iter().map(|d| ((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)).sqrt()).fold(4.0f64, f64::min);
        // a bookcase on a free wall (living rooms): away from the sofa and the TV, centred, clear of the doors
        if rt == "living-room" && lr.living_bookcase && area >= 20.0 && in_room(state, &|c| c == "BookshelfWallFeature").is_empty() {
            for k in in_room(state, &|c| matches!(c, "SingleCabinetFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory")) { layout2d::remove_tree(state, &k); }
            let name = format!("{room}_bookcase").replace('/', "_");
            if new_piece(ctx, state, rng, "SimpleBookcaseFactory", &room, &name) {
                let keys = in_room(state, &|c| matches!(c, "SofaFactory" | "TVStandFactory" | "FireplaceFeature" | "ArmChairFactory"));
                let (nm, dd) = (name.clone(), door_d.clone());
                let ok = place_on_wall(state, &name, &room, &move |st, c, t, len, _n| {
                    let gap = keys.iter().map(|k| layout2d::dist(st, k, &nm)).fold(9.0f64, f64::min);
                    if gap < 0.8 { return -100.0 }
                    gap.min(2.0) + dd(c).min(1.5) - (t - len / 2.0).abs() / len.max(0.1)
                });
                if ok { log.push(format!("{room}: bookcase on a free wall")); } else { state.objs.shift_remove(&name); }
            }
        }
        let want = match rt.as_str() {
            "living-room" => if area >= 20.0 { lr.living_plants } else { lr.living_plants.min(1) },
            "bedroom" if area >= 16.0 => lr.bedroom_plants,
            "hallway" if area >= 6.0 => lr.hallway_plants,
            _ => 0,
        };
        for i in 0..want {
            let name = format!("{room}_plant{i}").replace('/', "_");
            if !new_piece(ctx, state, rng, "LargePlantContainerFactory", &room, &name) { break }
            let (nm, dd) = (name.clone(), door_d.clone());
            let ok = place_on_wall(state, &name, &room, &move |st, c, t, len, _n| {
                // in a corner, away from the doors, not hard against another piece (a plant squeezed beside the sofa reads as clutter)
                let near = st.objs.iter().filter(|(k, o)| k.as_str() != nm && o.generator.is_some() && !layout2d::is_room(o) && !matches!(class_of(o), "RugFactory" | "WallArtFactory" | "MirrorFactory" | "CeilingLightFactory"))
                    .map(|(k, _)| layout2d::dist(st, k, &nm)).fold(9.0f64, f64::min);
                // (and 25 cm clear at least: the leaves spread 20-25 cm past the pot's box, into a nightstand beside it)
                if near < 0.25 { return -100.0 }
                -t.min(len - t) * 2.0 + dd(c).min(1.5) + near.min(0.6)
            });
            // (every spot too tight: none)
            let near = state.objs.iter().filter(|(k, o)| k.as_str() != name && o.generator.is_some() && !layout2d::is_room(o) && !matches!(class_of(o), "RugFactory" | "WallArtFactory" | "MirrorFactory" | "CeilingLightFactory"))
                .map(|(k, _)| layout2d::dist(state, k, &name)).fold(9.0f64, f64::min);
            if ok && near >= 0.25 { log.push(format!("{room}: plant in a corner")); } else { state.objs.shift_remove(&name); break }
        }
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
    let lr = super::layout_rules::get();
    // (and the chair ring -- the table's box 0.6 m out all round -- clear of the door swings: chairs there were left out, the rest sat
    // on one side)
    let swings: Vec<std::sync::Arc<Vec<layout2d::Prism>>> = state.objs.keys().filter(|k| k.starts_with(&format!("doorclear/{room}/"))).map(|k| layout2d::prisms(state, k)).collect();
    let roomy = |st: &State| layout2d::prisms(st, t).iter().flat_map(|p| p.poly.iter().copied().collect::<Vec<_>>()).all(|v| {
        layout2d::ring_dist(&rings, v) >= lr.table_wall_clear && doors.iter().all(|d| ((d[0] - v[0]).powi(2) + (d[1] - v[1]).powi(2)).sqrt() >= lr.table_door_clear)
    }) && {
        let (l, h) = layout2d::lohi_world(st, t);
        let ring = vec![[l[0] - 0.6, l[1] - 0.6], [h[0] + 0.6, l[1] - 0.6], [h[0] + 0.6, h[1] + 0.6], [l[0] - 0.6, h[1] + 0.6]];
        !swings.iter().any(|ps| ps.iter().any(|p| layout2d::poly_dist(&ring, &p.poly) <= 0.0))
    };
    // the fallback: the spot with the most table sides whose chair strip (0.6 m) is clear of the walls and the door swings
    let sides_free = |st: &State| -> usize {
        let (l, h) = layout2d::lohi_world(st, t);
        let strips = [[l[0] - 0.6, l[1], l[0], h[1]], [h[0], l[1], h[0] + 0.6, h[1]], [l[0], l[1] - 0.6, h[0], l[1]], [l[0], h[1], h[0], h[1] + 0.6]];
        strips.iter().filter(|b| {
            let q = vec![[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]];
            q.iter().all(|v| layout2d::in_rings(&rings, *v)) && !swings.iter().any(|ps| ps.iter().any(|p| layout2d::poly_dist(&q, &p.poly) <= 0.0))
        }).count()
    };
    let mut first: Option<(usize, super::geom::Pose)> = None;
    for r in [0.0f64, 0.15, 0.3, 0.45, 0.6, 0.75, 0.9, 1.05, 1.2] {
        for (ox, oy) in [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r), (r, r), (-r, -r), (r, -r), (-r, r)] {
            if r == 0.0 && (ox != 0.0 || oy != 0.0) { continue }
            for yy in [yaw_, yaw_ + std::f64::consts::FRAC_PI_2] {
                if try_place(state, t, yy, anchor, [c[0] + ox, c[1] + oy, 0.0]) {
                    if roomy(state) { return true; }
                    let sc = sides_free(state);
                    if first.as_ref().map_or(true, |(b, _)| sc > *b) { first = Some((sc, state.objs[t].pose)); }
                }
            }
        }
    }
    if let Some((_, p)) = first { state.set_pose(t, p); state.touch(t); return true; }
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
            // dining: centred under the table, long side along it
            "dining-room" => in_room(state, "TableDiningFactory").map(|t| {
                let tp = state.objs[&t].pose;
                let (l, h) = lohi(state, &t);
                let w = tp.apply([(l[0] + h[0]) / 2.0, (l[1] + h[1]) / 2.0, 0.0]);
                ([w[0], w[1]], yaw(&tp))
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
        // the rug sized to what it lies under (width across the sofa / bed / along the table, depth the other way): the sofa and the
        // coffee table on it with 0.2 m to spare, the dining chairs pulled out (0.6 m round the table), a bed's lower two thirds and 0.5 m
        // each side; a round table gets a round rug
        let size = match room_type_of(&room) {
            "living-room" => in_room(state, "SofaFactory").map(|s| {
                let (l, h) = lohi(state, &s);
                let cd = in_room(state, "CoffeeTableFactory").map_or(0.6, |ct| { let (l2, h2) = lohi(state, &ct); (h2[0] - l2[0]).min(h2[1] - l2[1]) });
                [((h[1] - l[1]) + 0.4).clamp(1.6, 3.0), (cd + 1.4).clamp(1.4, 2.4), 0.0]
            }),
            "dining-room" => in_room(state, "TableDiningFactory").map(|t| {
                let (l, h) = lohi(state, &t);
                let round = state.objs[&t].tags.contains(Tag::sem("RoundTable"));
                let (tw, td) = (h[1] - l[1], h[0] - l[0]);
                if round { let d = (tw.max(td) + 1.2).clamp(1.6, 2.6); [d, d, 1.0] } else { [(tw + 1.2).clamp(1.6, 3.4), (td + 1.2).clamp(1.6, 2.6), 0.0] }
            }),
            "bedroom" => in_room(state, "BedFactory").map(|b| {
                let (l, h) = lohi(state, &b);
                [((h[1] - l[1]) + 1.0).clamp(1.6, 3.0), ((h[0] - l[0]) * 0.7).clamp(1.2, 2.0), 0.0]
            }),
            _ => None,
        };
        let (lo, hi, anchor) = if let Some([w, d, round]) = size {
            let t = (hi[2] - lo[2]).clamp(0.01, 0.03);
            let o = state.objs.get_mut(&rug).unwrap();
            o.geom = std::sync::Arc::new(super::geom::ObjGeom::cuboid([-d / 2.0, -w / 2.0, lo[2]], [d / 2.0, w / 2.0, lo[2] + t], 0));
            if let Some(g) = o.generator.as_mut() { g.size = Some([w, d, t]); g.fit_box = None; }
            // (a piece: resized to the rug's size, its own geometry; a round one -- under a round table -- is a round rug, its own asset)
            if round > 0.5 && o.generator.as_ref().map_or(false, |g| g.piece.is_some()) {
                let fs = o.generator.as_ref().unwrap().factory_seed;
                if let Some(p) = crate::piece::make_piece("RoundRugFactory", fs) {
                    let g = o.generator.as_mut().unwrap();
                    g.class = "RoundRugFactory".to_string();
                    g.piece = Some(super::state::PieceRef::new(p));
                }
            }
            if let Some(np) = o.generator.as_ref().and_then(|g| g.piece.as_ref()).and_then(|p| p.resize([w, d, t])) {
                o.geom = std::sync::Arc::new(super::solver::piece_geom(&np).0);
                let ps = np.size();
                o.generator.as_mut().unwrap().piece = Some(np);
                o.generator.as_mut().unwrap().size = Some(ps);
            }
            if round > 0.5 { o.tags.insert(Tag::sem("RoundTable")); }
            state.touch(&rug);
            let (l2, h2) = lohi(state, &rug);
            (l2, h2, [0.0, 0.0, lo[2]])
        } else { (lo, hi, anchor) };
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
            super::layers::complain(super::layers::Layer::Furniture, &room, "rug", None, "floor for a rug under the group", false);
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
        // (and room in front of it to open / use it: a wardrobe moved to a straight wall had its doors flush against a bed's side)
        let (lo, hi) = lohi(state, &k);
        let (dep, wid) = (hi[0] - lo[0], hi[1] - lo[1]);
        let reach = if class_of(&state.objs[&k]) == "WardrobeFactory" { 0.6 } else { 0.5 };
        let kk = k.clone();
        let front_free = move |st: &State, c: [f64; 2], n: [f64; 2]| -> bool {
            let u = [-n[1], n[0]];
            let f0 = [c[0] + n[0] * dep / 2.0, c[1] + n[1] * dep / 2.0];
            let strip = vec![[f0[0] - u[0] * wid / 2.0, f0[1] - u[1] * wid / 2.0], [f0[0] + u[0] * wid / 2.0, f0[1] + u[1] * wid / 2.0],
                [f0[0] + u[0] * wid / 2.0 + n[0] * reach, f0[1] + u[1] * wid / 2.0 + n[1] * reach], [f0[0] - u[0] * wid / 2.0 + n[0] * reach, f0[1] - u[1] * wid / 2.0 + n[1] * reach]];
            !st.objs.iter().any(|(o, b)| o != &kk && b.generator.is_some() && !layout2d::is_room(b) && !matches!(class_of(b), "RugFactory" | "WallArtFactory" | "MirrorFactory" | "CeilingLightFactory") && layout2d::floor_foot(st, o).is_some()
                && layout2d::prisms(st, o).iter().any(|p| p.z[0] < 1.0 && layout2d::poly_dist(&strip, &p.poly) <= 0.0))
        };
        let ff = &front_free;
        if place_on_wall(state, &k, &room, &move |st: &State, c: [f64; 2], _t, _len, n: [f64; 2]| if straight_ok(n) && ff(st, c, n) { -((c[0] - p0[0]).powi(2) + (c[1] - p0[1]).powi(2)).sqrt() } else { -1e6 }) {
            let y2 = yaw(&state.objs[&k].pose).to_degrees().rem_euclid(90.0);
            let (c2, n2) = { let p = state.objs[&k].pose; let n = [p.r[0][0], p.r[1][0]]; ([p.t[0] + n[0] * (lo[0] + hi[0]) / 2.0, p.t[1] + n[1] * (lo[0] + hi[0]) / 2.0], n) };
            if y2.min(90.0 - y2) < 3.0 && front_free(state, c2, n2) {
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
    let ok = |(k, o): (&String, &ObjState)| {
        k == owner || layout2d::is_room(o) || o.tags.contains(nc) || o.relations.iter().any(|r| r.target == owner) || k.starts_with("doorclear/") && false
            || layout2d::prisms(state, k).iter().all(|p| layout2d::prism_depth(&probe, p) <= 1e-4)
    };
    if ig_core::env_on!("SLOTDBG") {
        let bad: Vec<&String> = state.objs.iter().filter(|e| !ok(*e)).map(|(k, _)| k).collect();
        eprintln!("SLOTDBG {owner}: blocked by {bad:?}");
    }
    state.objs.iter().all(ok)
}

/// counter runs without a sink get one on their top: under a window that overlooks the run (the usual spot), else at the middle,
/// against the backsplash; further spots along the run when items already stand there
pub fn sinks(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, only_room: Option<&str>) -> Vec<String> {
    let mut log = vec![];
    let mut runs: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "KitchenSpaceFactory" && only_room.map_or(true, |r| layout2d::room_of(state, k).as_deref() == Some(r))).map(|(k, _)| k.clone()).collect();
    // per kitchen, the runs in order of preference: the one the range stands at (else the longest) first; a run without room for
    // the sink hands it to the next
    let rank = |st: &State, r: &str| -> (String, f64, f64) {
        let room = layout2d::room_of(st, r).unwrap_or_default();
        let d = st.objs.iter().filter(|(k, o)| class_of(o) == "OvenFactory" && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| layout2d::dist(st, r, k)).fold(9.0f64, f64::min);
        let w = st.objs[r].generator.as_ref().and_then(super::solver::cur_size).map_or(0.0, |c| c[0]);
        (room, d, -w)
    };
    runs.sort_by(|a, b| rank(state, a).partial_cmp(&rank(state, b)).unwrap());
    for run in runs.clone() {
        let Some(room) = layout2d::room_of(state, &run) else { continue };
        // a planned kitchen (kitchen_plan.rs): one sink, on the plan's run at the plan's spot (the solver's sinks go)
        let plan = super::kitchen_plan::planned_sink(&room).filter(|(r, _)| state.objs.contains_key(r));
        if let Some((pr, _)) = &plan {
            if run != *pr { continue }
            let mine = format!("{pr}_sink");
            let others: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "SinkFactory" && **k != mine && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            for k in others { layout2d::remove_tree(state, &k); }
            if state.objs.contains_key(&mine) { continue }
        }
        // a sink already on this run stays unless it stands against the range (no counter between): then it is placed again
        let on_run: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "SinkFactory" && o.relations.iter().any(|r| r.target == run)).map(|(k, _)| k.clone()).collect();
        if !on_run.is_empty() {
            let ovens0: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "OvenFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            let tight = on_run.iter().any(|sk| ovens0.iter().any(|o| layout2d::dist(state, o, sk) < super::layout_rules::get().sink_range_gap));
            if !tight {
                // (it stays, set on the counter top: the solver stood it on the run's box, the top of the wall cabinets under the ceiling)
                let prs = layout2d::prisms(state, &run);
                let base = prs.iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min);
                let top = prs.iter().map(|p| p.z[1]).filter(|z| *z > base + 0.7 && *z < base + 1.2).fold(f64::NAN, f64::max);
                let top = if top.is_finite() { top } else { base + 0.91 };
                let ry = yaw(&state.objs[&run].pose);
                for sk in &on_run {
                    let bottom = layout2d::prisms(state, sk).iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min);
                    // turned like the run (tap at the back, as the sinks placed below): the solver's own could stand a quarter turn off
                    let (slo, shi) = lohi(state, sk);
                    let c = state.objs[sk].pose.apply([(slo[0] + shi[0]) / 2.0, (slo[1] + shi[1]) / 2.0, slo[2]]);
                    let turned = (yaw(&state.objs[sk].pose) - ry).sin().abs() > 0.05 || (yaw(&state.objs[sk].pose) - ry).cos() < 0.0;
                    if bottom.is_finite() && ((bottom - top).abs() > 0.03 || turned) {
                        let anchor = [(slo[0] + shi[0]) / 2.0, (slo[1] + shi[1]) / 2.0, slo[2]];
                        let (s_, c_) = ry.sin_cos();
                        let ra = [c_ * anchor[0] - s_ * anchor[1], s_ * anchor[0] + c_ * anchor[1]];
                        let p = super::geom::Pose::from_euler([c[0] - ra[0], c[1] - ra[1], top - slo[2]], [0.0, 0.0, ry]);
                        state.set_pose(sk, p);
                        state.touch(sk);
                        log.push(format!("{run}: sink {sk} set on the counter ({:+.2} m{})", top - bottom, if turned { ", turned like the run" } else { "" }));
                    }
                }
                continue
            }
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
        let main = plan.as_ref().map(|(r, _)| r.clone()).or_else(|| in_room.iter().min_by(|a, b| key(a).partial_cmp(&key(b)).unwrap()).map(|r| r.to_string()));
        let first = main.as_deref() == Some(run.as_str());
        if !first && state.objs.iter().any(|(k, o)| class_of(o) == "SinkFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())) { continue }
        // a sink the solver put on another run moves here (removed; the one below takes its place)
        if first {
            let elsewhere: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "SinkFactory" && layout2d::room_of(state, k).as_deref() == Some(room.as_str())
                && !o.relations.iter().any(|r| r.target == run)).map(|(k, _)| k.clone()).collect();
            for k in elsewhere { layout2d::remove_tree(state, &k); log.push(format!("{room}: sink {k} on a run away from the range, moved")); }
        }
        // the solver's loose items on the run go first (the counter dresser removes and redresses them by zone anyway: dress::purge)
        let loose: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == run) && class_of(o) != "SinkFactory").map(|(k, _)| k.clone()).collect();
        for k in loose { if state.objs.contains_key(&k) { layout2d::remove_tree(state, &k); } }
        let rp = state.objs[&run].pose;
        let (rlo, rhi) = lohi(state, &run);
        // the counter top: the top of the run's lowest part
        // (the highest part top 0.7-1.2 m above the run's base: a run whose parts came as one box -- base and wall cabinets together --
        // put the sink on top of the wall cabinets, under the ceiling; else the standard 0.91 m)
        let prs = layout2d::prisms(state, &run);
        let base = prs.iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min);
        let top = prs.iter().map(|p| p.z[1]).filter(|z| *z > base + 0.7 && *z < base + 1.2).fold(f64::NAN, f64::max);
        let top = if top.is_finite() { top } else { base + 0.91 };
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
        if let Some((_, at)) = plan.as_ref().filter(|(r, _)| *r == run) { ys.insert(0, inv([at[0], at[1], 0.0])[1]); }
        let sd = shi[0] - slo[0];
        let mut ok = false;
        // counter to work on between the sink and the range: 0.4 m or more first, then any free spot
        let range_gap = |st: &State| ovens.iter().map(|o| layout2d::dist(st, o, &name)).fold(9.0f64, f64::min);
        'pass: for strict in [true, false] {
            for &y in &ys {
                if y - (shi[1] - slo[1]) / 2.0 < rlo[1] + 0.05 || y + (shi[1] - slo[1]) / 2.0 > rhi[1] - 0.05 { continue }
                let at = rp.apply([rlo[0] + 0.06 + sd / 2.0, y, 0.0]);
                if try_place(state, &name, yaw(&rp), [(slo[0] + shi[0]) / 2.0, (slo[1] + shi[1]) / 2.0, slo[2]], [at[0], at[1], top]) && (!strict || range_gap(state) >= super::layout_rules::get().sink_range_gap) { ok = true; break 'pass; }
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

/// pictures and mirrors after the wall stage: the solver hangs them at any height (frames standing on the floor against the wall, a
/// mirror under the ceiling) and in any number. Per room: up to a few pictures (living / bedroom / hallway 2, dining 1, kitchen /
/// bathroom 0; largest kept), one mirror (bedrooms, bathrooms, hallways only); pictures centred at eye height (1.5 m), small mirrors
/// at 1.55 m, full-length ones (1.2 m and taller) 0.2 m off the floor; nothing within 0.2 m of the ceiling or over a window / door.
pub fn wall_hangings(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let wbox = |st: &State, k: &str| -> ([f64; 3], [f64; 3]) {
        let o = &st.objs[k];
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    // floor level: the bottom of the door openings (doors stand on the floor); the ceiling: the room's wall height above it
    // (window intents of the wall layer are not openings yet)
    // (passages too: a picture hung in mid-air in a half-wall opening)
    let openings: Vec<String> = state.objs.iter().filter(|(k, o)| (k.starts_with("door") || k.starts_with("entrance") || k.starts_with("window") || k.starts_with("passage")) && !k.starts_with("doorclear") && !o.tags.contains(Tag::sem("WindowIntent"))).map(|(k, _)| k.clone()).collect();
    let floor_z = openings.iter().filter(|k| !k.starts_with("window") && !k.starts_with("passage")).map(|k| wbox(state, k).0[2]).fold(f64::INFINITY, f64::min);
    let floor_z = if floor_z.is_finite() { floor_z.max(0.0) } else { 0.14 };
    let rooms: Vec<String> = state.objs.iter().filter(|(_, o)| layout2d::is_room(o)).map(|(k, _)| k.clone()).collect();
    // a wall item's box (world, after a lift of dz) against the floor pieces it would hang over or into, and the door clearances
    // (a towel rail on the side wall of a doorway, a picture inside a wardrobe's box)
    let blocked = |st: &State, k: &str, dz: f64| -> Option<String> {
        let (lo, hi) = wbox(st, k);
        let (lo, hi) = ([lo[0], lo[1], lo[2] + dz], [hi[0], hi[1], hi[2] + dz]);
        for (o, b) in &st.objs {
            if o == k || layout2d::is_room(b) || b.tags.contains(Tag::sem("NoCollision")) { continue }
            let door = o.starts_with("doorclear/");
            let solid = b.generator.as_ref().map_or(false, |g| !matches!(g.class.as_str(), "WallArtFactory" | "MirrorFactory" | "RugFactory" | "CeilingLightFactory" | "HardwareFactory"))
                && !b.relations.iter().any(|r| st.objs.get(&r.target).map_or(false, |t| t.generator.is_some()));
            if !door && !solid { continue }
            let (bl, bh) = wbox(st, o);
            let m = if door { 0.0 } else { 0.03 };
            if (0..2).all(|d| lo[d] < bh[d] + m && bl[d] - m < hi[d]) && (door || (lo[2] < bh[2] && bl[2] < hi[2])) { return Some(o.clone()) }
        }
        None
    };
    for room in rooms {
        let rt = room_type_of(&room).to_string();
        let ceil = state.objs[&room].geom.hi[2] - floor_z;
        let in_room = |st: &State, c: &str| -> Vec<String> { st.objs.iter().filter(|(k, o)| class_of(o) == c && layout2d::room_of(st, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect() };
        // what already hangs in the room (pictures and mirrors overlapped: two frames one over the other)
        let mut hung: Vec<([f64; 3], [f64; 3])> = vec![];
        for (cls, max) in [("WallArtFactory", match rt.as_str() { "living-room" | "bedroom" | "hallway" => 2usize, "dining-room" => 1, _ => 0 }),
                           ("MirrorFactory", if matches!(rt.as_str(), "bedroom" | "bathroom" | "hallway") { 1 } else { 0 })] {
            let mut ks = in_room(state, cls);
            // the largest first (one good picture over a sofa reads better than several small ones)
            ks.sort_by(|a, b| { let ar = |k: &str| { let (l, h) = wbox(state, k); ((h[0] - l[0]).max(h[1] - l[1])) * (h[2] - l[2]) }; ar(b).partial_cmp(&ar(a)).unwrap() });
            let mut kept = 0;
            for k in ks.clone() {
                // nothing stands on a picture or a mirror (the solver used their top edge as a shelf: desk lamps on the frames)
                let on: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == k)).map(|(c, _)| c.clone()).collect();
                for c in on { if state.objs.contains_key(&c) { layout2d::remove_tree(state, &c); log.push(format!("{room}: removed {c} (on {cls} {k})")); } }
            }
            for k in ks {
                if ig_core::env_on!("HANGDBG") { let o = &state.objs[&k]; eprintln!("HANGDBG {k} local {:.2?}..{:.2?} yaw {:.2}", o.geom.lo, o.geom.hi, yaw(&o.pose)); }
                let (lo, hi) = wbox(state, &k);
                let h = hi[2] - lo[2];
                let target_lo = if cls == "MirrorFactory" && h >= 1.2 { floor_z + 0.2 } else { floor_z + if cls == "MirrorFactory" { 1.55 } else { 1.5 } - h / 2.0 };
                let target_lo = target_lo.min(floor_z + ceil - 0.2 - h).max(floor_z + 0.2);
                // over a bed: clear of its headboard (tall headboards hid the lower half of the picture)
                let head_top = state.objs.iter().filter(|(bk, bo)| class_of(bo) == "BedFactory" && layout2d::room_of(state, bk).as_deref() == Some(room.as_str()))
                    .map(|(bk, _)| wbox(state, bk)).filter(|(bl, bh)| (0..2).all(|d| lo[d] < bh[d] + 0.3 && bl[d] - 0.3 < hi[d])).map(|(_, bh)| bh[2]).fold(f64::NEG_INFINITY, f64::max);
                let target_lo = if head_top.is_finite() && cls == "WallArtFactory" { target_lo.max(head_top + 0.1) } else { target_lo };
                // (a canvas taller than 1.1 m or wider than 1.6 m read as a stray panel: dropped, not rescaled; mirrors: a bathroom's
                // at most 1.0 m by 1.0 m -- larger ones read as whiteboards -- a full-length one at most 0.8 m wide, others 1.2 m)
                let w = (hi[0] - lo[0]).max(hi[1] - lo[1]);
                let big = if cls == "WallArtFactory" { h > 1.1 || w > 1.6 } else if rt == "bathroom" { h > 1.0 || w > 1.0 } else if h >= 1.2 { w > 0.8 } else { w > 1.2 };
                // (lifted over a headboard past a 1.8 m centre it reads as hung too high: dropped -- the invariants hold wall art to 1.2-1.8 m)
                let fits = kept < max && !big && target_lo + h <= floor_z + ceil - 0.2 + 1e-6 && !(cls == "WallArtFactory" && target_lo + h / 2.0 - floor_z > 1.8);
                let dz = target_lo - lo[2];
                // over a window or a door at the new height: dropped
                let clash = fits && openings.iter().any(|w| { let (wl, wh) = wbox(state, w); (0..2).all(|d| lo[d] < wh[d] + 0.05 && wl[d] - 0.05 < hi[d]) && lo[2] + dz < wh[2] && wl[2] < hi[2] + dz });
                let into = if fits && !clash { blocked(state, &k, dz) } else { None };
                let clash = clash || into.is_some();
                let over = fits && !clash && hung.iter().any(|(ol, oh)| (0..2).all(|d| lo[d] < oh[d] + 0.05 && ol[d] - 0.05 < hi[d]) && lo[2] + dz < oh[2] + 0.05 && ol[2] - 0.05 < hi[2] + dz);
                if !fits || clash || over {
                    layout2d::remove_tree(state, &k);
                    log.push(format!("{room}: removed {cls} {k} ({})", if clash { "over an opening" } else if over { "over another hanging" } else if big { "too large" } else { "over the count" }));
                    continue;
                }
                hung.push(([lo[0], lo[1], lo[2] + dz], [hi[0], hi[1], hi[2] + dz]));
                if dz.abs() > 0.01 {
                    let mut p = state.objs[&k].pose;
                    p.translate([0.0, 0.0, dz]);
                    state.set_pose(&k, p);
                    state.touch(&k);
                    log.push(format!("{room}: {cls} {k} moved {dz:+.2} m to {:.2}-{:.2} m", target_lo - floor_z, target_lo + h - floor_z));
                }
                kept += 1;
            }
        }
        // bathrooms: the mirror over the washbasin (on the wall behind it, centred, its bottom 0.2 m above the basin), unless a window is
        // there; towel rails centred at 1.15 m, small fittings (paper holder, hooks) at 0.7 m
        if rt == "bathroom" {
            let sink = state.objs.iter().find(|(k, o)| matches!(class_of(o), "StandingSinkFactory" | "BathroomSinkFactory") && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone());
            if let (Some(sink), Some(m)) = (sink, in_room(state, "MirrorFactory").first().cloned()) {
                let (sp, (slo, shi)) = (state.objs[&sink].pose, lohi(state, &sink));
                let (mlo, mhi) = lohi(state, &m);
                let ys = yaw(&sp);
                let back = sp.apply([slo[0], (slo[1] + shi[1]) / 2.0, 0.0]);
                let sink_top = wbox(state, &sink).1[2];
                let h = mhi[2] - mlo[2];
                // (its bottom 8 cm over the basin's box, which holds the tap)
                let zc = (floor_z + 1.55).max(sink_top + 0.08 + h / 2.0).min(floor_z + ceil - 0.25 - h / 2.0);
                let (s_, c_) = ys.sin_cos();
                let a = [mlo[0], (mlo[1] + mhi[1]) / 2.0, (mlo[2] + mhi[2]) / 2.0];
                let ra = [c_ * a[0] - s_ * a[1], s_ * a[0] + c_ * a[1]];
                let old = state.objs[&m].pose;
                // centred over the basin, else slid along the wall (to the side away from a corner or a curve of the wall: the mirror
                // wider than the flat run behind the basin hung past it, through the wall)
                let w = mhi[1] - mlo[1];
                let mut clash = true;
                for t in [0.0, 0.05, -0.05, 0.1, -0.1, 0.15, -0.15, 0.2, -0.2, 0.3, -0.3].iter().map(|f| f * w.max(0.4)) {
                    state.set_pose(&m, super::geom::Pose::from_euler([back[0] - ra[0] - s_ * t, back[1] - ra[1] + c_ * t, zc - a[2]], [0.0, 0.0, ys]));
                    state.touch(&m);
                    let (lo, hi) = wbox(state, &m);
                    clash = openings.iter().any(|w| { let (wl, wh) = wbox(state, w); (0..3).all(|d| lo[d] < wh[d] + 0.02 && wl[d] - 0.02 < hi[d]) }) || layout2d::outside_room_depth(state, &m, &room) > 0.01;
                    if !clash { break }
                }
                // (no spot over the basin: the mirror goes -- left where the solver hung it, it was over the toilet)
                if clash { state.set_pose(&m, old); state.touch(&m); layout2d::remove_tree(state, &m); log.push(format!("{room}: removed mirror {m} (no wall over the washbasin)")); } else { log.push(format!("{room}: mirror {m} over the washbasin")); }
            } else if let Some(m) = in_room(state, "MirrorFactory").first().cloned() {
                layout2d::remove_tree(state, &m);
                log.push(format!("{room}: removed mirror {m} (no washbasin)"));
            }
            for k in in_room(state, "HardwareFactory") {
                let (lo, hi) = wbox(state, &k);
                let long = (hi[0] - lo[0]).max(hi[1] - lo[1]) >= 0.3;
                let zc = floor_z + if long { 1.15 } else { 0.7 };
                let dz = zc - (lo[2] + hi[2]) / 2.0;
                // over a fixture or in a doorway at its height, or into another fitting or the mirror: removed
                let fit_hit = { let (l, h) = wbox(state, &k); state.objs.iter().find(|(o, b)| o.as_str() != k && matches!(class_of(b), "HardwareFactory" | "MirrorFactory") && { let (bl, bh) = wbox(state, o); (0..2).all(|d| l[d] < bh[d] + 0.03 && bl[d] - 0.03 < h[d]) && l[2] + dz < bh[2] + 0.05 && bl[2] - 0.05 < h[2] + dz }).map(|(o, _)| o.clone()) };
                if let Some(b) = blocked(state, &k, dz).or(fit_hit) {
                    layout2d::remove_tree(state, &k);
                    log.push(format!("{room}: removed fitting {k} (into {b})"));
                    continue;
                }
                if dz.abs() > 0.02 {
                    let mut p = state.objs[&k].pose;
                    p.translate([0.0, 0.0, dz]);
                    state.set_pose(&k, p);
                    state.touch(&k);
                    log.push(format!("{room}: {} {k} moved {dz:+.2} m", if long { "towel rail" } else { "fitting" }));
                }
            }
        }
    }
    log
}

/// wall fittings (towel rails, holders) inside a fixture's box: the fixture passes run after the wall hangings, and a washbasin moved
/// under a towel rail had its tap through the rail (the tap stands above the basin's box: 0.3 m clear over a basin)
pub fn fittings_clear(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let fixtures: Vec<String> = state.objs.iter().filter(|(_, o)| matches!(class_of(o), "StandingSinkFactory" | "BathroomSinkFactory" | "ToiletFactory" | "BathtubFactory" | "ShowerFactory")).map(|(k, _)| k.clone()).collect();
    let fits: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "HardwareFactory").map(|(k, _)| k.clone()).collect();
    for k in fits {
        let (lo, hi) = layout2d::lohi_world(state, &k);
        if let Some(f) = fixtures.iter().find(|f| { let (fl, fh) = layout2d::lohi_world(state, f); (0..2).all(|d| lo[d] < fh[d] + 0.02 && fl[d] - 0.02 < hi[d]) && lo[2] < fh[2] + if class_of(&state.objs[f.as_str()]).contains("Sink") { 0.3 } else { 0.02 } && fl[2] < hi[2] }) {
            log.push(format!("removed fitting {k} (in {f})"));
            layout2d::remove_tree(state, &k);
        }
    }
    log
}

/// last word on the door keep-outs: a floor piece (not an anchor: beds, sofas, tables, fixtures stay) more than 5 cm into a door's
/// clearance after every pass is removed (a living-room cabinet stood 44 cm into a doorway's passage)
pub fn door_keepouts(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let kos: Vec<String> = state.objs.keys().filter(|k| k.starts_with("doorclear/")).cloned().collect();
    let pieces: Vec<String> = state.objs.iter().filter(|(k, o)| o.generator.is_some() && !layout2d::is_room(o) && !layout2d::anchor(class_of(o)) && !class_of(o).contains("Rug") && !class_of(o).ends_with("Feature") && layout2d::floor_foot(state, k).is_some()).map(|(k, _)| k.clone()).collect();
    for k in pieces {
        if !state.objs.contains_key(&k) { continue }
        // (world boxes: the prism depth read 2 cm for a cabinet 0.35 m into one)
        let (l, h) = layout2d::lohi_world(state, &k);
        let into = |ko: &String| { let (a, b) = layout2d::lohi_world(state, ko); (0..2).map(|d| (h[d].min(b[d]) - l[d].max(a[d])).max(0.0)).fold(f64::MAX, f64::min) };
        if let Some(ko) = kos.iter().find(|ko| into(ko) > 0.05) {
            log.push(format!("removed {} {k} ({:.2} m into {ko})", class_of(&state.objs[&k]), into(ko)));
            layout2d::remove_tree(state, &k);
        }
    }
    log
}

/// one cabinet style per kitchen: every counter run (and wall cabinet) of a room built with the factory seed of its longest run --
/// each run drew its own, so a short run beside the sink came out white next to dark cabinets and read as a separate unit
pub fn kitchen_style(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let mut by_room: std::collections::BTreeMap<(String, String), Vec<(String, f64)>> = Default::default();
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        if !matches!(g.class.as_str(), "KitchenSpaceFactory" | "KitchenCabinetFactory") { continue }
        let Some(room) = layout2d::room_of(state, k) else { continue };
        let w = super::solver::cur_size(g).map_or(0.0, |c| c[0]);
        by_room.entry((room, g.class.clone())).or_default().push((k.clone(), w));
    }
    for ((room, cls), runs) in by_room {
        if runs.len() < 2 { continue }
        let (lead, _) = runs.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().clone();
        let fs = state.objs[&lead].generator.as_ref().unwrap().factory_seed;
        let mut n = 0;
        for (k, _) in &runs {
            let g = state.objs.get_mut(k).unwrap().generator.as_mut().unwrap();
            if g.factory_seed != fs { g.factory_seed = fs; n += 1; }
        }
        if n > 0 { log.push(format!("{room}: {n} {cls} restyled like {lead}")); }
    }
    log
}

/// items left behind: a piece resting on furniture (relation to a generated parent) whose centre is no longer over that parent -- the
/// group passes move beds, nightstands and tables without what stood on them, and desk lamps floated at the old headboard height
pub fn orphans(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    // a TV centred on its stand (the solver left one at the stand's back edge, its foot 2 cm into the wall)
    let tvs: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| class_of(o) == "TVFactory").filter_map(|(k, o)| o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.get(t).map_or(false, |p| class_of(p) == "TVStandFactory")).map(|t| (k.clone(), t))).collect();
    for (tv, st) in tvs {
        // (a second TV on the stand went with the first's clearing)
        if !state.objs.contains_key(&tv) || !state.objs.contains_key(&st) { continue }
        let (sl, sh) = layout2d::lohi_world(state, &st);
        let (tl, th) = layout2d::lohi_world(state, &tv);
        let d = [(sl[0] + sh[0] - tl[0] - th[0]) / 2.0, (sl[1] + sh[1] - tl[1] - th[1]) / 2.0];
        if d[0].abs() + d[1].abs() > 0.005 { let mut p = state.objs[&tv].pose; p.translate([d[0], d[1], 0.0]); state.set_pose(&tv, p); state.touch(&tv); }
        // what else stands on the stand: slid along it clear of the centred TV, else off (a lamp's shade went into the screen)
        let (tl, th) = layout2d::lohi_world(state, &tv);
        let kids: Vec<String> = state.objs.iter().filter(|(k, o)| **k != tv && o.relations.iter().any(|r| r.target == st)).map(|(k, _)| k.clone()).collect();
        for k in kids {
            // (gone with an earlier one's tree)
            if !state.objs.contains_key(&k) { continue }
            let (kl, kh) = layout2d::lohi_world(state, &k);
            if !(0..2).all(|i| kl[i] < th[i] + 0.02 && tl[i] - 0.02 < kh[i]) { continue }
            // the stand's long axis in world xy, and the shifts that put the piece just past either side of the TV
            let ax = if sh[0] - sl[0] >= sh[1] - sl[1] { 0 } else { 1 };
            let cands = [th[ax] + 0.03 - kl[ax], tl[ax] - 0.03 - kh[ax]];
            let fit = cands.iter().cloned().filter(|s| kl[ax] + s >= sl[ax] && kh[ax] + s <= sh[ax]).min_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());
            if let Some(sv) = fit {
                let mut p = state.objs[&k].pose; let mut t = [0.0; 3]; t[ax] = sv; p.translate(t); state.set_pose(&k, p); state.touch(&k);
            } else { layout2d::remove_tree(state, &k); }
        }
    }
    // pictures and mirrors a piece moved in front of after they were hung (wardrobes grow to their alcove afterwards)
    let hung: Vec<String> = state.objs.iter().filter(|(_, o)| matches!(class_of(o), "WallArtFactory" | "MirrorFactory")).map(|(k, _)| k.clone()).collect();
    for k in hung {
        let (l, h) = layout2d::lohi_world(state, &k);
        let hit = state.objs.iter().find(|(o, b)| o.as_str() != k && matches!(class_of(b), "WardrobeFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "KitchenSpaceFactory" | "BookshelfWallFeature")
            && { let (bl, bh) = layout2d::lohi_world(state, o); (0..2).all(|d| l[d] < bh[d] + 0.04 && bl[d] - 0.04 < h[d]) && l[2] < bh[2] && bl[2] < h[2] }).map(|(o, _)| o.clone());
        if let Some(o) = hit { layout2d::remove_tree(state, &k); log.push(format!("removed {k} (behind {o})")); }
    }
    // one desk lamp per surface, none within 0.5 m of another (the solver stood two side by side on a shelf)
    let lamps: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "DeskLampFactory").map(|(k, _)| k.clone()).collect();
    let mut kept: Vec<(String, [f64; 3])> = vec![];
    for k in lamps {
        let o = &state.objs[&k];
        let parent = o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.get(t).map_or(false, |p| p.generator.is_some()));
        let t = o.pose.t;
        // (not on a coffee table or a dining table: a table lamp stood in the middle of the seating)
        if parent.as_ref().and_then(|p| state.objs.get(p)).map_or(false, |p| matches!(class_of(p), "CoffeeTableFactory" | "TableDiningFactory" | "SofaFactory" | "ArmChairFactory" | "BedFactory")) {
            layout2d::remove_tree(state, &k); log.push(format!("removed DeskLampFactory {k} (on a coffee / dining table)")); continue;
        }
        let dup = kept.iter().any(|(pk, pt)| (parent.is_some() && state.objs[pk.as_str()].relations.iter().any(|r| Some(&r.target) == parent.as_ref())) || ((pt[0] - t[0]).hypot(pt[1] - t[1]) < 0.5 && (pt[2] - t[2]).abs() < 0.3));
        if dup { layout2d::remove_tree(state, &k); log.push(format!("removed DeskLampFactory {k} (a lamp beside it already)")); } else { kept.push((k, t)); }
    }
    let wb = |st: &State, k: &str| -> ([f64; 3], [f64; 3]) {
        let o = &st.objs[k];
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let items: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| o.generator.is_some()).filter_map(|(k, o)| {
        o.relations.iter().find(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some() && !layout2d::is_room(t))).map(|r| (k.clone(), r.target.clone()))
    }).collect();
    if ig_core::env_on!("ORPHDBG") {
        for (k, o) in &state.objs { if class_of(o) == "DeskLampFactory" { eprintln!("ORPHDBG {k} z {:.2} rels {:?}", o.pose.t[2], o.relations.iter().map(|r| (r.target.clone(), format!("{:?}", r.relation).chars().take(40).collect::<String>())).collect::<Vec<_>>()); } }
    }
    for (k, p) in items {
        if !state.objs.contains_key(&k) || !state.objs.contains_key(&p) { continue }
        // dining chairs and nightstands relate to their table / bed but stand beside it
        if matches!(class_of(&state.objs[&k]), "ChairFactory" | "SideTableFactory" | "OfficeChairFactory" | "BarChairFactory") { continue }
        let (il, ih) = wb(state, &k);
        let (pl, ph) = wb(state, &p);
        let c = [(il[0] + ih[0]) / 2.0, (il[1] + ih[1]) / 2.0];
        let over = c[0] > pl[0] - 0.05 && c[0] < ph[0] + 0.05 && c[1] > pl[1] - 0.05 && c[1] < ph[1] + 0.05;
        let level = il[2] > pl[2] - 0.05 && il[2] < ph[2] + 0.1;
        // nothing but bedding on a bed (the solver stood desk lamps on the bed's box, above the real headboard)
        // and nothing at all on a potted plant (lamps stood on the foliage at 1.6 m)
        let pc = class_of(&state.objs[&p]);
        let on_bed = (pc == "BedFactory" && !matches!(class_of(&state.objs[&k]), "PillowFactory" | "BlanketFactory" | "ComforterFactory" | "BoxComforterFactory" | "TowelFactory" | "ClothFactory"))
            || pc.contains("Plant");
        if !(over && level) || on_bed {
            let cls = class_of(&state.objs[&k]).to_string();
            if ig_core::env_on!("ORPHDBG") { eprintln!("ORPHDBG {k} on {p}: t {:?} geom {:?}-{:?} parent {pl:.2?}-{ph:.2?} over {over} level {level}", state.objs[&k].pose.t, state.objs[&k].geom.lo, state.objs[&k].geom.hi); }
            layout2d::remove_tree(state, &k);
            log.push(format!("removed {cls} {k} (left behind by {p})"));
        }
    }
    log
}

/// pose `name` like try_place (local point `anchor` at world `at`, turned `yaw_`, keeping its height when at.z is 0) without testing it
fn pose_at(state: &mut State, name: &str, yaw_: f64, anchor: V3, at: V3) {
    let old = state.objs[name].pose;
    let (s, c) = yaw_.sin_cos();
    let ra = [c * anchor[0] - s * anchor[1], s * anchor[0] + c * anchor[1], anchor[2]];
    let tz = if at[2] == 0.0 { old.t[2] } else { at[2] - ra[2] };
    state.set_pose(name, super::geom::Pose::from_euler([at[0] - ra[0], at[1] - ra[1], tz], [0.0, 0.0, yaw_]));
    state.touch(name);
}

/// The living-room group as one annealed layout (the room layer's first real solver; LIVING_SA=0 keeps the old branch chain).
/// Parameters: the TV's wall and position along it (fixed for a fireplace), the viewing distance, the sofa's sideways offset, whether
/// it backs onto the wall behind it when one is near, and where the armchairs go (none / left / right / both, beside the coffee table,
/// facing it). Each candidate builds the whole group -- TV, sofa, coffee table, armchairs -- and is scored as one: every piece placed
/// legally (free_ok: no overlaps, the room, the walkways), the sofa's back on a wall, a 2.6 m view, the TV clear of the doors, the
/// group near the room's middle, the wanted armchairs placed. Returns None (nothing changed) when no candidate seats the sofa.
thread_local! { static FIRE_FAILS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[allow(clippy::too_many_arguments)]
fn living_sa(ctx: &mut Ctx, state: &mut State, rng: &mut Rng, room: &str, tv: &str, sofa: &str, fixed: bool, fixed_front: (f64, [f64; 3]),
             lifted: &[(String, ObjState)], want: usize) -> Option<Vec<String>> {
    let mut log = vec![];
    let lr = super::layout_rules::get();
    let segs = wall_segments(state, room);
    let doors = door_points(state, room);
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    if rings.is_empty() { return None }
    let fc = { let r = &rings[0]; let n = r.len() as f64; [r.iter().map(|p| p[0]).sum::<f64>() / n, r.iter().map(|p| p[1]).sum::<f64>() / n] };
    let (tlo, thi) = lohi(state, tv);
    let (slo, shi) = lohi(state, sofa);
    let (tv0, sofa0) = (state.objs[tv].pose, state.objs[sofa].pose);
    let sw = shi[1] - slo[1];
    // the coffee table sized to the sofa once (three quarters of its width)
    let mut coffee: Option<(String, ObjState)> = lifted.iter().find(|(_, o)| class_of(o) == "CoffeeTableFactory").cloned();
    if let Some((k, o)) = coffee.as_mut() {
        state.objs.insert(k.clone(), o.clone());
        let target = (sw * lr.coffee_ratio).clamp(0.6, 1.6);
        let base = state.objs[k.as_str()].clone();
        let _ = super::solver::set_width(ctx, state, k, &base, target, 1.0);
        *o = state.objs.shift_remove(k.as_str()).unwrap();
    }
    let arms: Vec<(String, ObjState)> = lifted.iter().filter(|(_, o)| class_of(o) == "ArmChairFactory").take(want.max(0)).cloned().collect();
    // a fireplace moves with the group (FIRE_MOVE=0 keeps it where the plan built it): it, its keep-outs, the built-in shelves in
    // its alcoves (bookshelf walls beside it) and what stands on them move as one rigid piece along any wall long enough
    let fire_move = fixed && std::env::var("FIRE_MOVE").map_or(true, |v| v != "0");
    let wbox = |st: &State, k: &str| -> ([f64; 3], [f64; 3]) {
        let o = &st.objs[k];
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let mut members: Vec<(String, super::geom::Pose)> = vec![];
    let (fire_w, fire_depth) = if fire_move {
        let (fl, fh) = wbox(state, tv);
        let mut ks = vec![tv.to_string()];
        for (k, o) in &state.objs {
            if class_of(o) == "BookshelfWallFeature" && layout2d::room_of(state, k).as_deref() == Some(room) {
                let (l, h) = wbox(state, k);
                if (0..2).all(|d| l[d] < fh[d] + 0.15 && fl[d] - 0.15 < h[d]) { ks.push(k.clone()); }
            }
        }
        let base_ks = ks.clone();
        for k in &base_ks {
            ks.extend(state.objs.keys().filter(|kk| kk.starts_with(&format!("{k}.keepout"))).cloned());
            ks.extend(state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| &r.target == k) && o.generator.is_some()).map(|(kk, _)| kk.clone()));
        }
        ks.sort(); ks.dedup();
        members = ks.into_iter().map(|k| { let p = state.objs[&k].pose; (k, p) }).collect();
        let all: Vec<([f64; 3], [f64; 3])> = members.iter().filter(|(k, _)| !k.contains(".keepout") && (k == tv || state.objs[k].generator.as_ref().map_or(false, |g| g.class.ends_with("Feature")))).map(|(k, _)| wbox(state, k)).collect();
        let w = all.iter().map(|(l, h)| (h[0] - l[0]).max(h[1] - l[1])).fold(0.0, f64::max);
        let (fty, ffront) = fixed_front;
        let span_w = { let perp = [-fty.sin(), fty.cos()]; let pr: Vec<f64> = all.iter().flat_map(|(l, h)| [[l[0], l[1]], [h[0], l[1]], [h[0], h[1]], [l[0], h[1]]]).map(|q| (q[0] - ffront[0]) * perp[0] + (q[1] - ffront[1]) * perp[1]).collect(); pr.iter().cloned().fold(f64::MIN, f64::max) - pr.iter().cloned().fold(f64::MAX, f64::min) };
        (span_w.max(w * 0.0), layout2d::ring_dist(&rings, [ffront[0], ffront[1]]))
    } else { (0.0, 0.0) };
    let member_keys: Vec<String> = members.iter().map(|(k, _)| k.clone()).collect();
    // the fireplace group at a wall spot: rigid move of every member from where the plan put it (turned about its face's centre)
    let place_fire = |state: &mut State, wall: usize, t: f64| -> Option<(f64, [f64; 3])> {
        let (a, b, n) = segs[wall];
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if len < fire_w + 0.2 { return None }
        let tt = fire_w / 2.0 + 0.1 + (len - fire_w - 0.2) * t;
        let base = [a[0] + (b[0] - a[0]) / len * tt, a[1] + (b[1] - a[1]) / len * tt];
        let ny = n[1].atan2(n[0]);
        let (oy, of) = fixed_front;
        let nf = [base[0] + n[0] * fire_depth, base[1] + n[1] * fire_depth];
        let dy = ny - oy;
        let (s_, c_) = dy.sin_cos();
        for (k, p0) in &members {
            let d = [p0.t[0] - of[0], p0.t[1] - of[1]];
            let w = [nf[0] + c_ * d[0] - s_ * d[1], nf[1] + s_ * d[0] + c_ * d[1]];
            state.set_pose(k, super::geom::Pose::from_euler([w[0], w[1], p0.t[2]], [0.0, 0.0, yaw(p0) + dy]));
            state.touch(k);
        }
        // the moved pieces clear of everything else (doors, windows, door clearances, furniture) and inside the room
        let nc = Tag::sem("NoCollision");
        for (k, _) in members.iter().filter(|(k, _)| !k.contains(".keepout") && state.objs[k.as_str()].generator.as_ref().map_or(false, |g| g.class.ends_with("Feature"))) {
            let od = layout2d::outside_room_depth(state, k, room);
            if od > 1e-3 { if ig_core::env_on!("SADBG") { eprintln!("SADBG fire {k} outside {od:.3}"); } return None }
            for (o, b) in &state.objs {
                if member_keys.contains(o) || b.tags.contains(nc) || layout2d::is_room(b) { continue }
                if o.starts_with("doorclear/") && !o.starts_with(&format!("doorclear/{room}/")) { continue }
                if layout2d::depth(state, k, o) > 1e-4 { if ig_core::env_on!("SADBG") { eprintln!("SADBG fire {k} hits {o}"); } return None }
            }
        }
        Some((ny, [nf[0], nf[1], 0.0]))
    };
    let intents: Vec<([f64; 3], [f64; 3])> = state.objs.iter().filter(|(_, o)| o.tags.contains(Tag::sem("WindowIntent"))).map(|(k, _)| layout2d::lohi_world(state, k))
        .filter(|(l, h)| layout2d::ring_dist(&rings, [(l[0] + h[0]) / 2.0, (l[1] + h[1]) / 2.0]) < 0.4).collect();
    #[derive(Clone, Copy, Debug)]
    struct P { wall: usize, t: f64, d: f64, lat: f64, back: bool, arms: u8 }
    let tv_walls: Vec<usize> = (0..segs.len()).filter(|&i| { let (a, b, _) = segs[i]; ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt() > (thi[1] - tlo[1]) + 0.2 }).collect();
    let tv_walls: Vec<usize> = if fire_move { (0..segs.len()).filter(|&i| { let (a, b, _) = segs[i]; ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt() > fire_w + 0.2 }).collect() } else { tv_walls };
    if !fixed && tv_walls.is_empty() { return None }
    // build a candidate into the state; returns its score (None: the TV or the sofa cannot stand there) and which optional pieces
    // were placed. The optional pieces are inserted only while they are tried.
    let build = |state: &mut State, p: &P, keep: bool| -> Option<(f64, Vec<String>)> {
        // (the sofa out of the way while the TV finds its spot: it is placed after, facing it)
        let sofa_i = state.objs.get_index_of(sofa)?;
        let sofa_o = state.objs.shift_remove(sofa)?;
        // the TV and the direction it faces
        let tv_r: Option<(f64, [f64; 3])> = if fire_move { let r = place_fire(state, p.wall, p.t); if r.is_none() { FIRE_FAILS.with(|c| c.set(c.get() + 1)); } r } else if fixed { Some(fixed_front) } else {
            let (a, b, n) = segs[p.wall];
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let tw = thi[1] - tlo[1];
            let tt = tw / 2.0 + 0.05 + (len - tw - 0.1).max(0.0) * p.t;
            let base = [a[0] + (b[0] - a[0]) / len * tt, a[1] + (b[1] - a[1]) / len * tt];
            pose_at(state, tv, n[1].atan2(n[0]), [tlo[0], (tlo[1] + thi[1]) / 2.0, tlo[2]], [base[0] + n[0] * 0.003, base[1] + n[1] * 0.003, 0.0]);
            let td = thi[0] - tlo[0];
            free_ok(state, tv).then(|| (n[1].atan2(n[0]), [base[0] + n[0] * td, base[1] + n[1] * td, 0.0]))
        };
        state.objs.shift_insert(sofa_i, sofa.to_string(), sofa_o);
        state.touch(sofa);
        let (ty, tfront) = tv_r?;
        let f = [ty.cos(), ty.sin()];
        let perp = [-f[1], f[0]];
        // the sofa facing it at distance d, shifted sideways; backed onto a wall behind it when one is within 0.6 m
        let mut d = p.d;
        if p.back {
            let sdep = shi[0] - slo[0];
            let back_at = |d: f64| [tfront[0] + f[0] * (d + sdep) + perp[0] * p.lat, tfront[1] + f[1] * (d + sdep) + perp[1] * p.lat];
            for k in 0..=12 {
                let q = back_at(d + k as f64 * 0.05);
                if !layout2d::in_rings(&rings, q) { d += (k as f64 * 0.05 - 0.02).max(0.0); break }
            }
        }
        if d > lr.tv_view_max + 0.4 { return None }
        let at = [tfront[0] + f[0] * d + perp[0] * p.lat, tfront[1] + f[1] * d + perp[1] * p.lat, 0.0];
        pose_at(state, sofa, ty + std::f64::consts::PI, [shi[0], (slo[1] + shi[1]) / 2.0, slo[2]], at);
        if !free_ok(state, sofa) {
            if ig_core::env_on!("SADBG") { let hits: Vec<String> = state.objs.keys().filter(|k| k.as_str() != sofa && layout2d::depth(state, sofa, k) > 1e-4).cloned().collect(); eprintln!("SADBG sofa hits {hits:?} outside {:.3} layout {}", layout2d::outside_room_depth(state, sofa, room), layout2d::layout_ok(state, sofa)); }
            return None
        }
        let back = state.objs[sofa].pose.apply([slo[0], (slo[1] + shi[1]) / 2.0, 0.0]);
        let on_wall = layout2d::ring_dist(&rings, [back[0], back[1]]) < 0.15;
        let tv_door = doors.iter().map(|q| ((q[0] - tfront[0]).powi(2) + (q[1] - tfront[1]).powi(2)).sqrt()).fold(9.0f64, f64::min);
        let mid = [(tfront[0] + at[0]) / 2.0, (tfront[1] + at[1]) / 2.0];
        // the TV / fireplace off a window still to come (the room's windows went behind a fireplace moved onto its one outside wall)
        let over_window = { let (l, h) = layout2d::lohi_world(state, tv); intents.iter().any(|(il, ih)| (0..2).all(|dd| l[dd] < ih[dd] + 0.3 && il[dd] - 0.3 < h[dd])) };
        let mut score = if on_wall { 5.0 } else { 0.0 } - if over_window { 6.0 } else { 0.0 } - (d - 2.6).abs() * 1.5 - (2.6 - tv_door).max(0.0) * 3.0 - ((mid[0] - fc[0]).powi(2) + (mid[1] - fc[1]).powi(2)).sqrt() * 0.3 - p.lat.abs() * 0.5;
        if d < 1.8 { score -= 3.0; }
        // the coffee table before the sofa, 0.45 m off its front
        let sy = ty + std::f64::consts::PI;
        let sf = [sy.cos(), sy.sin()];
        let sfront = [at[0], at[1]];
        let mut placed = vec![];
        let mut coffee_c = [sfront[0] + sf[0] * 0.8, sfront[1] + sf[1] * 0.8];
        let mut coffee_hw = 0.4;
        if let Some((k, o)) = &coffee {
            state.objs.insert(k.clone(), o.clone());
            state.touch(k);
            let (clo, chi) = lohi(state, k);
            let off = 0.45 + (chi[0] - clo[0]) / 2.0;
            let c = [sfront[0] + sf[0] * off, sfront[1] + sf[1] * off, 0.0];
            pose_at(state, k, sy, [(clo[0] + chi[0]) / 2.0, (clo[1] + chi[1]) / 2.0, clo[2]], c);
            if free_ok(state, k) { score += 2.0; placed.push(k.clone()); coffee_c = [c[0], c[1]]; coffee_hw = (chi[1] - clo[1]) / 2.0; }
            else { state.objs.shift_remove(k); score -= 1.0; }
        }
        // armchairs beside the coffee table, facing it
        let sides: &[f64] = match p.arms { 1 => &[1.0], 2 => &[-1.0], 3 => &[1.0, -1.0], _ => &[] };
        let mut n_arm = 0;
        for (i, side) in sides.iter().enumerate() {
            let Some((k, o)) = arms.get(i) else { break };
            state.objs.insert(k.clone(), o.clone());
            state.touch(k);
            let (alo, ahi) = lohi(state, k);
            let dd = coffee_hw + 0.45 + (ahi[0] - alo[0]) / 2.0;
            let c = [coffee_c[0] + perp[0] * side * dd, coffee_c[1] + perp[1] * side * dd, 0.0];
            let face = (coffee_c[1] - c[1]).atan2(coffee_c[0] - c[0]);
            pose_at(state, k, face, [(alo[0] + ahi[0]) / 2.0, (alo[1] + ahi[1]) / 2.0, alo[2]], c);
            if free_ok(state, k) { n_arm += 1; placed.push(k.clone()); } else { state.objs.shift_remove(k); }
        }
        score += 3.0 * n_arm.min(want) as f64 - 3.0 * (want.saturating_sub(n_arm)) as f64;
        if !keep { for k in &placed { state.objs.shift_remove(k); } }
        Some((score, placed))
    };
    // annealing over the parameters: 120 random starts, then 400 steps from the best (temperature 2 -> 0.05)
    let rnd_p = |rng: &mut Rng| P { wall: if tv_walls.is_empty() { 0 } else { tv_walls[rng.randint(0, tv_walls.len() as i64) as usize] }, t: rng.uniform(0.0, 1.0),
        d: rng.uniform(lr.tv_view_min.max(1.6), lr.tv_view_max), lat: rng.uniform(-0.6, 0.6), back: rng.f64() < 0.7, arms: [0u8, 1, 2, 3][rng.choice_idx(4)] };
    let mut cur: Option<(f64, P)> = None;
    for _ in 0..120 {
        let p = rnd_p(rng);
        if let Some((s, _)) = build(state, &p, false) { if cur.map_or(true, |c| s > c.0) { cur = Some((s, p)); } }
    }
    if ig_core::env_on!("SADBG") {
        let mut why = [0usize; 3];
        for _ in 0..60 {
            let p = rnd_p(rng);
            match build(state, &p, false) { Some(_) => why[2] += 1, None => { if state.objs.contains_key(tv) && !fire_move && !fixed && !free_ok(state, tv) { why[0] += 1 } else { why[1] += 1 } } }
        }
        eprintln!("SADBG {room}: fire fails {} intents {:?}", FIRE_FAILS.with(|c| c.get()), intents);
        eprintln!("SADBG {room}: tv_walls {} fire_move {fire_move} fire_w {fire_w:.2} members {} -> tv-fail {} sofa-fail {} ok {}", tv_walls.len(), members.len(), why[0], why[1], why[2]);
    }
    let Some(mut cur) = cur else {
        for (k, p0) in &members { state.set_pose(k, *p0); state.touch(k); }
        state.set_pose(tv, tv0); state.touch(tv);
        state.set_pose(sofa, sofa0); state.touch(sofa);
        return None;
    };
    let mut best = cur;
    let steps = 400;
    for i in 0..steps {
        let temp = 2.0 * (0.025f64).powf(i as f64 / steps as f64);
        let mut p = cur.1;
        match rng.choice_idx(6) {
            0 if (!fixed || fire_move) && !tv_walls.is_empty() => { p.wall = tv_walls[rng.randint(0, tv_walls.len() as i64) as usize]; }
            1 => { p.t = (p.t + rng.normal(0.0, 0.1)).clamp(0.0, 1.0); }
            2 => { p.d = (p.d + rng.normal(0.0, 0.2)).clamp(lr.tv_view_min, lr.tv_view_max); }
            3 => { p.lat = (p.lat + rng.normal(0.0, 0.2)).clamp(-1.0, 1.0); }
            4 => { p.back = !p.back; }
            _ => { p.arms = [0u8, 1, 2, 3][rng.choice_idx(4)]; }
        }
        let Some((s, _)) = build(state, &p, false) else { continue };
        if s > cur.0 || rng.f64() < ((s - cur.0) / temp).exp() { cur = (s, p); }
        if cur.0 > best.0 { best = cur; }
    }
    // the best layout, built for good
    let (score, placed) = build(state, &best.1, true)?;
    log.push(format!("{room}: living group annealed (score {score:.2}: TV wall {} at {:.2}, view {:.2} m, lateral {:+.2}, backed {}, armchairs {} of {want})",
        best.1.wall, best.1.t, best.1.d, best.1.lat, best.1.back, placed.iter().filter(|k| k.contains("ArmChair") || k.contains("armchair")).count()));
    for (k, _) in lifted {
        if !placed.contains(k) {
            log.push(format!("{room}: {} left out (no spot in the annealed group)", class_of(&lifted.iter().find(|(kk, _)| kk == k).unwrap().1)));
            if class_of(&lifted.iter().find(|(kk, _)| kk == k).unwrap().1) == "ArmChairFactory" && lifted.iter().filter(|(_, o)| class_of(o) == "ArmChairFactory").position(|(kk, _)| kk == k).map_or(false, |i| i < want) {
                super::layers::complain(super::layers::Layer::Furniture, room, "armchair", at_of(state, sofa), "floor beside the coffee table for an armchair", false);
            }
        }
    }
    Some(log)
}
