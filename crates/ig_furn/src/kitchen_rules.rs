//! Kitchen rules: what a kitchen layout is held to, in one place, for every pass that places kitchen pieces (the L template, the
//! run-end pass, the extra runs, the sink) to score its choices with, and for free_ok to keep loose furniture out of appliance doors.
//!   * door zones: free floor in front of each fridge (its width + `fridge_door_clear`, the door's swing), range and dishwasher
//!     (`appliance_front_clear`, the open door and someone in front of it), inside the room;
//!   * fridge off corners: `fridge_corner_gap` of open floor or in-line counter beside each side of a fridge (a wall or a run
//!     turning the corner there stops its door at 90 degrees, too narrow to pull a drawer out);
//!   * work triangle: sink, range and fridge fronts each `triangle_leg_min`-`triangle_leg_max` apart, the legs summing to
//!     `triangle_sum_min`-`triangle_sum_max`.
//!   * counter line: no slot under `counter_gap_max` beside a range or dishwasher (too narrow for a cabinet, it reads as a
//!     mistake); `close_gaps` slides the appliance across it.
//! `eval` scores a kitchen (0 = every rule met, lower is worse) and names what it breaks.
use super::layout2d::{self, Prism};
use super::state::{ObjState, State};
use super::tags::Tag;

fn class_of(o: &ObjState) -> &str {
    o.generator.as_ref().map(|g| g.class.as_str()).unwrap_or("")
}
fn yaw(p: &super::geom::Pose) -> f64 {
    p.r[1][0].atan2(p.r[0][0])
}
fn room_of(state: &State, k: &str) -> Option<String> { layout2d::room_of(state, k) }

pub fn is_appliance(cls: &str) -> bool { matches!(cls, "FridgeFactory" | "OvenFactory" | "DishwasherFactory") }
/// the kitchen's built-in line (counter runs and what stands in them): held to the rules by score, not kept out of the zones
fn built_in(cls: &str) -> bool { is_appliance(cls) || matches!(cls, "KitchenSpaceFactory" | "KitchenCabinetFactory" | "SinkFactory" | "KitchenIslandFactory") }

/// the floor in front of an appliance's door (world footprint, counter-clockwise); None for anything else
pub fn door_zone(state: &State, k: &str) -> Option<Vec<[f64; 2]>> {
    let o = state.objs.get(k)?;
    let cls = class_of(o);
    if !is_appliance(cls) { return None }
    let lr = super::layout_rules::get();
    let (lo, hi) = (o.geom.lo, o.geom.hi);
    let w = hi[1] - lo[1];
    let depth = if cls == "FridgeFactory" { w + lr.fridge_door_clear } else { lr.appliance_front_clear };
    // (2 cm off the front: the counter run beside it shares the front line)
    let (x0, x1) = (hi[0] + 0.02, hi[0] + depth);
    Some([[x0, lo[1] + 0.02], [x1, lo[1] + 0.02], [x1, hi[1] - 0.02], [x0, hi[1] - 0.02]].iter().map(|p| { let q = o.pose.apply([p[0], p[1], 0.0]); [q[0], q[1]] }).collect())
}

fn hits(a: &[[f64; 2]], prs: &[Prism]) -> bool {
    // (floor-standing parts: above the slab, starting below head height -- not a ceiling light or a wall cabinet over the zone; and solid: a
    // neighbour's door swinging over this floor is every kitchen, they are not both open at once)
    prs.iter().any(|p| p.role == super::geom::ROLE_HARD && p.z[1] > 0.3 && p.z[0] < 1.4 && layout2d::poly_dist(a, &p.poly) <= 0.0)
}

/// a loose piece standing in an appliance's door zone, or an appliance whose zone holds a loose piece (free_ok's hard rule; the
/// built-in line is scored by `eval` instead)
pub fn zone_clash(state: &State, name: &str) -> bool {
    let Some(o) = state.objs.get(name) else { return false };
    let cls = class_of(o);
    if o.generator.is_none() || (built_in(cls) && !is_appliance(cls)) { return false }
    let Some(room) = room_of(state, name) else { return false };
    if !room.starts_with("kitchen") { return false }
    let nc = Tag::sem("NoCollision");
    // (pieces standing on others: a kettle on the counter is not in the way)
    let loose = |k: &str, b: &ObjState| !built_in(class_of(b)) && b.generator.is_some() && !b.tags.contains(nc.clone()) && !layout2d::is_room(b)
        && !b.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())) && k != name;
    if is_appliance(cls) {
        let Some(z) = door_zone(state, name) else { return false };
        return state.objs.iter().any(|(k, b)| loose(k, b) && room_of(state, k).as_deref() == Some(room.as_str()) && hits(&z, &layout2d::prisms(state, k)));
    }
    if !loose("", o) { return false }
    let mine = layout2d::prisms(state, name);
    state.objs.iter().filter(|(k, b)| is_appliance(class_of(b)) && room_of(state, k).as_deref() == Some(room.as_str()))
        .any(|(k, _)| door_zone(state, k).map_or(false, |z| hits(&z, &mine)))
}

pub struct Eval {
    pub score: f64,
    pub broken: Vec<String>,
    /// appliances that cannot be opened (door zone blocked, a fridge in a corner)
    pub unusable: usize,
}

/// the front centre of a piece (world xy)
fn front(state: &State, k: &str) -> [f64; 2] {
    let o = &state.objs[k];
    let q = o.pose.apply([o.geom.hi[0], (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]);
    [q[0], q[1]]
}

/// a run (or cabinet) grown across a fridge's side: built-in growth stops short of it (solver::fit_builtins)
pub fn blocks_fridge(state: &State, run: &str) -> bool {
    let Some(room) = room_of(state, run) else { return false };
    if !room.starts_with("kitchen") { return false }
    let fr: Vec<String> = state.objs.iter().filter(|(k, o)| class_of(o) == "FridgeFactory" && room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
    fr.iter().any(|f| side_probes(state, f).iter().any(|q| {
        let b = &state.objs[run];
        let turned = (yaw(&b.pose) - yaw(&state.objs[f.as_str()].pose)).sin().abs() > 0.3 || (yaw(&b.pose) - yaw(&state.objs[f.as_str()].pose)).cos() < 0.0;
        turned && layout2d::prisms(state, run).iter().any(|p| p.z[1] > 0.5 && p.z[0] < 0.4 && layout2d::in_rings(&[p.poly.clone()], *q))
    }))
}

/// a piece of the built-in line standing in another appliance's door zone (a run facing a fridge across too narrow an aisle)
pub fn in_door_zone(state: &State, k: &str) -> bool {
    let Some(room) = room_of(state, k) else { return false };
    if !room.starts_with("kitchen") { return false }
    let mine = layout2d::prisms(state, k);
    state.objs.iter().filter(|(a, b)| a.as_str() != k && is_appliance(class_of(b)) && room_of(state, a).as_deref() == Some(room.as_str()))
        .any(|(a, _)| door_zone(state, a).map_or(false, |z| hits(&z, &mine)))
}

/// the built-in line's rules a run's placement or growth may not break: across a fridge's side, or into a door zone
pub fn run_breaks(state: &State, run: &str) -> bool { blocks_fridge(state, run) || in_door_zone(state, run) }

/// points `fridge_corner_gap` out from each side of a fridge, beside the front half (where the door's edge swings)
fn side_probes(state: &State, k: &str) -> Vec<[f64; 2]> {
    let lr = super::layout_rules::get();
    let o = &state.objs[k];
    let (lo, hi) = (o.geom.lo, o.geom.hi);
    let mut v = vec![];
    for (y, s) in [(lo[1], -1.0), (hi[1], 1.0)] {
        for fx in [0.55, 0.9] {
            let q = o.pose.apply([lo[0] + (hi[0] - lo[0]) * fx, y + s * lr.fridge_corner_gap, 0.0]);
            v.push([q[0], q[1]]);
        }
    }
    v
}

/// the open floor beside each side of a piece (local -y, +y), measured at its mid-depth to the nearest floor-standing piece or the
/// room's edge, up to 1 m
pub fn side_gaps(state: &State, k: &str, rings: &[Vec<[f64; 2]>]) -> Vec<(f64, f64)> {
    let o = &state.objs[k];
    let (lo, hi) = (o.geom.lo, o.geom.hi);
    let room = room_of(state, k);
    let x = (lo[0] + hi[0]) / 2.0;
    let others: Vec<std::sync::Arc<Vec<Prism>>> = state.objs.iter().filter(|(kk, b)| kk.as_str() != k && b.generator.is_some() && room_of(state, kk) == room
        && !b.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some()))).map(|(kk, _)| layout2d::prisms(state, kk)).collect();
    let mut out = vec![];
    for (y, s) in [(lo[1], -1.0), (hi[1], 1.0)] {
        let mut g = 1.0;
        for i in 1..=100 {
            let d = i as f64 * 0.01;
            let q = o.pose.apply([x, y + s * d, 0.0]);
            let q = [q[0], q[1]];
            if !layout2d::in_rings(rings, q) || others.iter().any(|ps| ps.iter().any(|p| p.z[1] > 0.5 && p.z[0] < 0.4 && layout2d::in_rings(&[p.poly.clone()], q))) { g = d - 0.01; break }
        }
        out.push((s, g));
    }
    out
}

/// a fridge side blocked within the corner gap: a wall, or a run / tall piece turned across it
pub fn fridge_cornered(state: &State, k: &str, rings: &[Vec<[f64; 2]>]) -> bool {
    let o = &state.objs[k];
    let fy = yaw(&o.pose);
    let room = room_of(state, k);
    side_probes(state, k).into_iter().any(|q| {
        if !layout2d::in_rings(rings, q) {
            if ig_core::env_on!("KRULEDBG") { eprintln!("KRULE corner {k}: probe {q:?} off the floor"); }
            return true
        }
        let by = state.objs.iter().find(|(kk, b)| kk.as_str() != k && b.generator.is_some() && room_of(state, kk) == room && {
            let turned = (yaw(&b.pose) - fy).sin().abs() > 0.3 || (yaw(&b.pose) - fy).cos() < 0.0;
            (turned || class_of(b) == "FridgeFactory" || class_of(b) == "KitchenCabinetFactory") && layout2d::prisms(state, kk).iter().any(|p| p.z[1] > 0.5 && p.z[0] < 0.4 && layout2d::in_rings(&[p.poly.clone()], q))
        }).map(|(kk, _)| kk.clone());
        if let Some(b) = &by { if ig_core::env_on!("KRULEDBG") { eprintln!("KRULE corner {k}: probe {q:?} in {b}"); } }
        by.is_some()
    })
}

/// the kitchen in `room` against the rules; `sink_at` stands in for a sink not placed yet (the template)
pub fn eval(state: &State, room: &str, sink_at: Option<[f64; 2]>) -> Eval {
    let lr = super::layout_rules::get();
    let mut score = 0.0;
    let mut broken = vec![];
    let mut unusable = 0;
    let rings = layout2d::rings(&layout2d::room_floor(state, room));
    let in_room = |c: &str| -> Vec<String> { state.objs.iter().filter(|(k, o)| class_of(o) == c && room_of(state, k).as_deref() == Some(room)).map(|(k, _)| k.clone()).collect() };
    let (fridges, ranges, sinks) = (in_room("FridgeFactory"), in_room("OvenFactory"), in_room("SinkFactory"));
    let nc = Tag::sem("NoCollision");
    // door zones: inside the room and clear of every other piece standing on the floor (the built-in line included)
    for k in fridges.iter().chain(ranges.iter()).chain(in_room("DishwasherFactory").iter()) {
        let Some(z) = door_zone(state, k) else { continue };
        let n = 5;
        let out = (0..n * n).filter(|i| {
            let (u, v) = ((i % n) as f64 / (n - 1) as f64, (i / n) as f64 / (n - 1) as f64);
            let a = [z[0][0] + (z[1][0] - z[0][0]) * u, z[0][1] + (z[1][1] - z[0][1]) * u];
            let b = [z[3][0] + (z[2][0] - z[3][0]) * u, z[3][1] + (z[2][1] - z[3][1]) * u];
            !layout2d::in_rings(&rings, [a[0] + (b[0] - a[0]) * v, a[1] + (b[1] - a[1]) * v])
        }).count();
        let blocker = state.objs.iter().find(|(kk, b)| kk.as_str() != k.as_str() && b.generator.is_some() && !b.tags.contains(nc.clone()) && !layout2d::is_room(b)
            && room_of(state, kk).as_deref() == Some(room) && !b.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some()))
            && hits(&z, &layout2d::prisms(state, kk))).map(|(kk, _)| kk.clone());
        if out > 2 || blocker.is_some() {
            score -= 4.0;
            unusable += 1;
            broken.push(format!("door zone of {k}: {}", blocker.map_or("outside the room".to_string(), |b| format!("{b} in it"))));
        }
    }
    for f in &fridges {
        if fridge_cornered(state, f, &rings) { score -= 5.0; unusable += 1; broken.push(format!("{f} in a corner")); }
    }
    // the counter line unbroken: no slot narrower than `counter_gap_max` beside a range or dishwasher
    for k in ranges.iter().chain(in_room("DishwasherFactory").iter()) {
        for (s, g) in side_gaps(state, k, &rings) {
            if g > 0.03 && g < lr.counter_gap_max { score -= 2.0; broken.push(format!("{g:.2} m gap at the {} side of {k}", if s > 0.0 { "+y" } else { "-y" })); }
        }
    }
    // the work triangle (the first of each)
    let sink = sinks.first().map(|s| front(state, s)).or(sink_at);
    if let (Some(f), Some(r), Some(s)) = (fridges.first(), ranges.first(), sink) {
        let (f, r) = (front(state, f), front(state, r));
        let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let legs = [d(f, r), d(r, s), d(s, f)];
        let mut bad = 0.0;
        for l in legs { bad += (lr.triangle_leg_min - l).max(0.0) + (l - lr.triangle_leg_max).max(0.0); }
        let sum: f64 = legs.iter().sum();
        bad += 0.5 * ((lr.triangle_sum_min - sum).max(0.0) + (sum - lr.triangle_sum_max).max(0.0));
        if bad > 0.05 { broken.push(format!("work triangle {:.2}/{:.2}/{:.2} m", legs[0], legs[1], legs[2])); }
        score -= 1.5 * bad;
    }
    Eval { score, broken, unusable }
}
