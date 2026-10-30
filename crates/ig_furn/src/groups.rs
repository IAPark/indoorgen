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
    for (k, b) in &state.objs {
        if k == name || b.tags.contains(nc) || layout2d::is_room(b) {
            continue;
        }
        // a piece's own children (items on it) are not obstacles
        if b.relations.iter().any(|r| r.target == name) {
            continue;
        }
        if layout2d::depth(state, name, k) > 1e-4 {
            return false;
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
    let p = super::geom::Pose::from_euler([at[0] - ra[0], at[1] - ra[1], at[2] - ra[2]], [0.0, 0.0, yaw_]);
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
    let runs: Vec<String> = state.objs.iter().filter(|(_, o)| class_of(o) == "KitchenSpaceFactory").map(|(k, _)| k.clone()).collect();
    for run in runs {
        let Some(room) = layout2d::room_of(state, &run) else { continue };
        let ry = yaw(&state.objs[&run].pose);
        let (clo, chi) = lohi(state, &run);
        let rp = state.objs[&run].pose;
        let mut ends_used: Vec<f64> = vec![];
        for cls in ["FridgeFactory", "DishwasherFactory"] {
            let pieces: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == cls && layout2d::room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            for p in pieces {
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
                if !done {
                    let pw = hi[1] - lo[1] + 0.02;
                    for s in [1.0f64, -1.0] {
                        if ends_used.contains(&s) { continue }
                        let base = state.objs[&run].clone();
                        let Some(cs) = base.generator.as_ref().and_then(super::solver::cur_size) else { break };
                        if cs[0] - pw < super::sizes::model("KitchenSpaceFactory").map_or(1.2, |m| m.min[0]) { continue }
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
                if !done {
                    log.push(format!("{cls} {p}: no free end at {run}"));
                }
            }
        }
    }
    log
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
        let want = if bhi[1] - blo[1] < 1.2 { 1 } else { 2 };
        // a second nightstand: a copy of this bed's (a pair) or of any nightstand in the house
        let template = stands.first().cloned().or_else(|| state.objs.iter().find(|(_, o)| class_of(o) == "SideTableFactory").map(|(k, _)| k.clone()));
        let mut placed = 0;
        for s in [1.0f64, -1.0] {
            if placed >= want {
                break;
            }
            let name = match stands.pop() {
                Some(n) => n,
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
            } else if name.contains("_pair") {
                state.objs.shift_remove(&name);
            }
        }
        // leftover nightstands of this bed that found no slot stay where the solver put them
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
            log.push(format!("desk {desk}: no room for a chair"));
        }
    }
    log
}
