//! One search for every room: items in tier order, a beam of partial layouts, every candidate pose scored by the item's terms. An item with no
//! place is simply not placed (a must is reported); nothing placed is ever taken back.
use crate::house::Room;
use crate::rules::{Item, Place, RoomRules, Variant};
use crate::world::*;
use ig_kit::piece::{rigid_bounds, Piece, Volume};
use std::collections::HashMap;

const BEAM: usize = 6;
const KEEP_PER_STATE: usize = 8;

#[derive(Clone, serde::Serialize)]
pub struct Placed { pub id: String, pub class: String, pub variant: String, pub side: bool, pub pose: Pose, pub lo: [f64; 3], pub hi: [f64; 3], pub prisms: Vec<Prism>, pub score: f64, pub terms: Vec<(String, f64)> }

#[derive(Clone, serde::Serialize)]
pub struct Candidate { pub at: Xy, pub yaw: f64, pub why: Option<Why>, pub score: f64, pub variant: String, pub side: bool, pub prisms: Vec<Prism> }

#[derive(serde::Serialize, Default, Clone)]
pub struct ItemReport { pub id: String, pub tier: u8, pub tried: usize, pub why: HashMap<String, usize>, pub placed: bool, pub variant: String }

#[derive(serde::Serialize)]
pub struct Solution { pub room: Room, pub placed: Vec<Placed>, pub report: Vec<ItemReport>, pub score: f64, pub ms: f64, pub walk: Vec<u8>, pub nx: usize, pub ny: usize, pub ox: f64, pub oy: f64, pub why_item: Vec<Candidate>, pub doors_joined: bool }

struct Shape { vols: Vec<Volume>, lo: [f64; 3], hi: [f64; 3], back: f64, side: f64 }

/// what stands against a wall: its rigid and soft parts (a plant's foliage reaches past its pot), not what swings out
fn shape_of(vols: Vec<Volume>) -> Shape {
    let (lo, hi) = rigid_bounds(&vols);
    let solid: Vec<_> = vols.iter().filter(|v| !matches!(v.role, ig_kit::piece::Role::Opening(_))).map(|v| v.bounds()).collect();
    let back = solid.iter().map(|b| b.0[0]).fold(lo[0], f64::min);
    let side = solid.iter().map(|b| b.0[1]).fold(lo[1], f64::min);
    Shape { vols, lo, hi, back, side }
}

fn hash(s: &str, seed: u64) -> u64 { let mut h = 0xcbf29ce484222325u64 ^ seed; for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); } h }

/// the piece's volumes for a variant (`Piece::tucked` once, twice)
fn shape(it: &Item, seed: u64, v: &Variant, cache: &mut HashMap<(String, u64, String), Option<std::rc::Rc<Shape>>>) -> Option<std::rc::Rc<Shape>> {
    let key = (it.class.clone(), seed, v.piece.clone());
    if let Some(s) = cache.get(&key) { return s.clone() }
    let draws = it.fit.as_ref().map_or(1, |f| f.draws.max(1)) as u64;
    let mut out = None;
    for d in 0..draws {
        let mut p: Option<Box<dyn Piece>> = ig_kit::piece::make_piece(&it.class, seed.wrapping_add(d.wrapping_mul(0x9E3779B97F4A7C15)));
        if v.piece != "as_is" { p = p.and_then(|x| x.tucked()); }
        if v.piece == "compact" { p = p.and_then(|x| x.tucked()); }
        let Some(mut p) = p else { continue };
        let s = std::rc::Rc::new(shape_of(p.collision()));
        let (lo, hi) = (s.lo, s.hi);
        if it.fit.as_ref().map_or(true, |f| hi[1] - lo[1] <= f.max_w && hi[2] - lo[2] <= f.max_h) { out = Some(s); break }
    }
    cache.insert(key, out.clone());
    out
}

/// sized from its parent (Place::Under)
fn sized_under(it: &Item, seed: u64, parent: &Placed) -> Option<std::rc::Rc<Shape>> {
    let Place::Under { add_w, share, w, d, t, .. } = &it.place else { return None };
    let (pl, pw) = (parent.hi[0] - parent.lo[0], parent.hi[1] - parent.lo[1]);
    let (rw, rd) = ((pw + add_w).clamp(w[0], w[1]), (pl * share).clamp(d[0], d[1]));
    let p = ig_kit::piece::make_piece(&it.class, seed)?;
    let mut p = p.resize([rw, rd, *t])?;
    Some(std::rc::Rc::new(shape_of(p.collision())))
}

fn poses(room: &Room, it: &Item, v: &Variant, s: &Shape, state: &State) -> Vec<(Pose, f64, f64)> {
    let mut out = vec![];
    let z = -s.lo[2];
    let (l, w) = (s.hi[0] - s.lo[0], s.hi[1] - s.lo[1]);
    match &it.place {
        Place::Wall => for wl in &room.walls {
            let span = if v.side { l } else { w };
            if wl.len < span { continue }
            let u = [(wl.b[0] - wl.a[0]) / wl.len, (wl.b[1] - wl.a[1]) / wl.len];
            // back to the wall (+x out), or its -y side to the wall
            let yaw = if v.side { (-wl.n[0]).atan2(wl.n[1]) } else { wl.n[1].atan2(wl.n[0]) };
            let (sn, cs) = yaw.sin_cos();
            let anchor = if v.side { [(s.lo[0] + s.hi[0]) / 2.0, s.side] } else { [s.back, (s.lo[1] + s.hi[1]) / 2.0] };
            let ra = [cs * anchor[0] - sn * anchor[1], sn * anchor[0] + cs * anchor[1]];
            let steps = ((wl.len - span) / 0.05).floor() as usize;
            for i in 0..=steps {
                let t = span / 2.0 + if steps == 0 { (wl.len - span) / 2.0 } else { (wl.len - span) * i as f64 / steps as f64 };
                let at = [wl.a[0] + u[0] * t + wl.n[0] * 0.003, wl.a[1] + u[1] * t + wl.n[1] * 0.003];
                out.push((Pose { yaw, t: [at[0] - ra[0], at[1] - ra[1]], z }, t, wl.len));
            }
        },
        Place::Hang { centers, tall_from, tall_bottom } => {
            let h = s.hi[2] - s.lo[2];
            let zs: Vec<f64> = if h >= *tall_from { vec![tall_bottom - s.lo[2]] } else { centers.iter().map(|c| c - h / 2.0 - s.lo[2]).collect() };
            for z in zs { for wl in &room.walls {
                if wl.len < w { continue }
                let u = [(wl.b[0] - wl.a[0]) / wl.len, (wl.b[1] - wl.a[1]) / wl.len];
                let yaw = wl.n[1].atan2(wl.n[0]);
                let (sn, cs) = yaw.sin_cos();
                let anchor = [s.lo[0], (s.lo[1] + s.hi[1]) / 2.0];
                let ra = [cs * anchor[0] - sn * anchor[1], sn * anchor[0] + cs * anchor[1]];
                let steps = ((wl.len - w) / 0.1).floor() as usize;
                for i in 0..=steps {
                    let t = w / 2.0 + if steps == 0 { (wl.len - w) / 2.0 } else { (wl.len - w) * i as f64 / steps as f64 };
                    let at = [wl.a[0] + u[0] * t + wl.n[0] * 0.003, wl.a[1] + u[1] * t + wl.n[1] * 0.003];
                    out.push((Pose { yaw, t: [at[0] - ra[0], at[1] - ra[1]], z }, t, wl.len));
                }
            } }
        },
        Place::Under { parent, past, .. } => if let Some(p) = state.items.iter().find(|p| &p.id == parent) {
            // (less far past the foot, then off centre, when a wall is near)
            for k in 0..6 { for off in [0.0, 0.1, -0.1, 0.2, -0.2, 0.35, -0.35] {
                let at = p.pose.apply([p.hi[0] + past - k as f64 * 0.1 - s.hi[0], (p.lo[1] + p.hi[1]) / 2.0 - (s.lo[1] + s.hi[1]) / 2.0 + off]);
                out.push((Pose { yaw: p.pose.yaw, t: at, z }, off.abs() + k as f64 * 0.1, 1.0));
            } }
        },
        Place::Beside { parent, side, gap } => if let Some(p) = state.items.iter().find(|p| &p.id == parent) {
            // in the parent's frame: back in line, next to its side
            let y = if *side > 0 { p.hi[1] + gap - s.lo[1] } else { p.lo[1] - gap - s.hi[1] };
            let x = p.lo[0] - s.lo[0];
            let at = p.pose.apply([x, y]);
            out.push((Pose { yaw: p.pose.yaw, t: at, z }, 0.0, 1.0));
        },
    }
    out
}

#[derive(Clone)]
struct State { world: World, items: Vec<Placed>, score: f64, skipped: Vec<(String, u8)> }

fn front_prism(s: &Shape, pose: &Pose, d: f64) -> Prism {
    let q = |x: f64, y: f64| pose.apply([x, y]);
    Prism { hull: hull2(vec![q(s.hi[0], s.lo[1]), q(s.hi[0] + d, s.lo[1]), q(s.hi[0] + d, s.hi[1]), q(s.hi[0], s.hi[1])]), z: [0.0, 1.9], role: OPENING }
}

fn term_value(tm: &crate::rules::Term, room: &Room, c: Xy, t: f64, len: f64, st: &State) -> f64 {
    let name = tm.name.as_str();
    let door = room.doors.iter().map(|d| (d.at[0] - c[0]).hypot(d.at[1] - c[1])).fold(4.0f64, f64::min);
    match name {
        "far_from_door" => (door / 4.0).min(1.0),
        "center_on_wall" => 1.0 - ((t - len / 2.0).abs() / (len / 2.0).max(0.1)).min(1.0),
        "long_wall" => (len / 5.0).min(1.0),
        "corner" => 1.0 - (t.min(len - t) / (len / 2.0).max(0.1)).min(1.0),
        // away from what is placed
        // the first candidates of a list (`t` the cost of straying from the ideal)
        "ideal" => 1.0 - t.min(1.0),
        "apart" => st.items.iter().map(|p| { let q = p.pose.apply([(p.lo[0] + p.hi[0]) / 2.0, (p.lo[1] + p.hi[1]) / 2.0]); (q[0] - c[0]).hypot(q[1] - c[1]) }).fold(3.0f64, f64::min) / 3.0,
        // near another item (`of`): 1 at it, 0 from 3 m
        "near" => tm.of.as_ref().and_then(|o| st.items.iter().find(|p| &p.id == o)).map_or(0.0, |p| { let q = p.pose.apply([(p.lo[0] + p.hi[0]) / 2.0, (p.lo[1] + p.hi[1]) / 2.0]); 1.0 - ((q[0] - c[0]).hypot(q[1] - c[1]) / 3.0).min(1.0) }),
        _ => panic!("unknown term {name}"),
    }
}

/// a low-discrepancy stream for the variety noise
fn noise(seed: u64, i: u64) -> f64 { let x = hash(&i.to_string(), seed); (x >> 11) as f64 / (1u64 << 53) as f64 }

/// `seed`: the house's and the run's (the same room in another house draws other pieces)
pub fn solve(room: &Room, rules: &RoomRules, seed: u64, why_id: Option<&str>) -> Solution {
    let t0 = std::time::Instant::now();
    let mut order: Vec<&Item> = rules.program.iter().filter(|i| room.area >= i.min_area).collect();
    order.sort_by_key(|i| i.tier);
    let mut cache = HashMap::new();
    let mut beam = vec![State { world: World::new(room), items: vec![], score: 0.0, skipped: vec![] }];
    let mut report: Vec<ItemReport> = vec![];
    let mut why_item = vec![];
    let mut draw = 0u64;
    for it in &order {
        let mut next: Vec<State> = vec![];
        let mut rep = ItemReport { id: it.id.clone(), tier: it.tier, ..Default::default() };
        let pseed = hash(&format!("{}/{}", room.name, it.id), seed);
        for st in &beam {
            let mut any = false;
            for v in &it.variants {
                let sized = if let Place::Under { parent, .. } = &it.place { st.items.iter().find(|p| &p.id == parent).and_then(|p| sized_under(it, pseed, p)) } else { shape(it, pseed, v, &mut cache) };
                let Some(s) = sized else { *rep.why.entry("no piece".into()).or_default() += 1; continue };
                let mut cands: Vec<(f64, Pose, Vec<Prism>, Vec<(String, f64)>)> = vec![];
                for (pose, t, len) in poses(room, it, v, &s, st) {
                    rep.tried += 1;
                    let mut ps = prisms(&s.vols, &pose);
                    if it.cover { for p in ps.iter_mut() { p.role = COVER; } }
                    let mut fail = st.world.voxel(&ps).or_else(|| st.world.exact(&ps, room));
                    if fail.is_none() && it.avoid_window {
                        let z_of = |i: usize| ps.iter().filter(|p| p.role == HARD).map(move |p| p.z[i]);
                        let (bot, top) = (z_of(0).fold(f64::MAX, f64::min), z_of(1).fold(f64::MIN, f64::max));
                        let blocks = room.windows.iter().any(|w| top > w.z[0] + 0.02 && bot < w.z[1] - 0.02 && { let hull = hull2(vec![[w.at[0] - w.u[0] * w.hw, w.at[1] - w.u[1] * w.hw], [w.at[0] + w.u[0] * w.hw, w.at[1] + w.u[1] * w.hw], [w.at[0] + w.u[0] * w.hw + w.n[0] * 0.1, w.at[1] + w.u[1] * w.hw + w.n[1] * 0.1], [w.at[0] - w.u[0] * w.hw + w.n[0] * 0.1, w.at[1] - w.u[1] * w.hw + w.n[1] * 0.1]]); ps.iter().any(|p| p.role == HARD && sat(&p.hull, &hull) < 0.0) });
                        if blocks { fail = Some(Why::Window); }
                    }
                    // the floor before its front is kept (blocks furniture: it stands in the walk's way below)
                    if fail.is_none() && it.front > 0.0 {
                        let f = front_prism(&s, &pose, it.front);
                        if st.world.placed.iter().any(|o| o.role == HARD && o.z[0] < 1.9 && sat(&f.hull, &o.hull) < -0.002) { fail = Some(Why::Hard); }
                    }
                    let c = pose.apply([(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0]);
                    let terms: Vec<(String, f64)> = it.terms.iter().map(|tm| (tm.name.clone(), tm.w * term_value(tm, room, c, t, len, st))).collect();
                    let sc: f64 = terms.iter().map(|x| x.1).sum::<f64>() + 0.15 * noise(pseed, { draw += 1; draw });
                    if fail.is_none() { cands.push((sc, pose, ps, terms)); } else {
                        let w = fail.unwrap();
                        *rep.why.entry(format!("{w:?}")).or_default() += 1;
                        if why_id == Some(it.id.as_str()) && next.is_empty() { why_item.push(Candidate { at: c, yaw: pose.yaw, why: Some(w), score: sc, variant: v.piece.clone(), side: v.side, prisms: ps.clone() }); }
                    }
                }
                cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                // the walk checked best first: the slowest test, on what could win
                let mut kept = 0;
                for (sc, pose, ps, terms) in cands {
                    let mut w2 = st.world.clone();
                    w2.add(&ps);
                    if it.front > 0.0 { let f = front_prism(&s, &pose, it.front); w2.add(&[f]); }
                    let walk = w2.walk(&[]);
                    let (seen, joined) = w2.flood(&walk, room);
                    let mut fail = (!joined && room.doors.len() > 1).then_some(Why::Walk);
                    if fail.is_none() && it.reach {
                        let q = pose.apply([s.hi[0] + PERSON + 0.1, (s.lo[1] + s.hi[1]) / 2.0]);
                        let q2 = pose.apply([(s.lo[0] + s.hi[0]) / 2.0, s.hi[1] + PERSON + 0.1]);
                        let q3 = pose.apply([(s.lo[0] + s.hi[0]) / 2.0, s.lo[1] - PERSON - 0.1]);
                        if ![q, q2, q3].iter().any(|p| w2.cell_of(*p).map_or(false, |k| seen[k])) { fail = Some(Why::Reach); }
                    }
                    if why_id == Some(it.id.as_str()) && next.is_empty() { let c = pose.apply([(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0]); why_item.push(Candidate { at: c, yaw: pose.yaw, why: fail, score: sc, variant: v.piece.clone(), side: v.side, prisms: ps.clone() }); }
                    if let Some(w) = fail { *rep.why.entry(format!("{w:?}")).or_default() += 1; continue }
                    any = true;
                    rep.placed = true;
                    rep.variant = format!("{}{}", v.piece, if v.side { "+side" } else { "" });
                    let mut ns = State { world: w2, items: st.items.clone(), score: st.score + sc, skipped: st.skipped.clone() };
                    ns.items.push(Placed { id: it.id.clone(), class: it.class.clone(), variant: v.piece.clone(), side: v.side, pose, lo: s.lo, hi: s.hi, prisms: ps, score: sc, terms });
                    next.push(ns);
                    kept += 1;
                    if kept >= KEEP_PER_STATE { break }
                }
                if any { break }
            }
            if !any {
                let mut ns = st.clone();
                ns.score -= [100.0, 5.0, 1.0][it.tier.min(2) as usize];
                ns.skipped.push((it.id.clone(), it.tier));
                next.push(ns);
            }
        }
        next.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        next.truncate(BEAM);
        beam = next;
        report.push(rep);
    }
    let best = beam.into_iter().next().unwrap();
    for r in report.iter_mut() { r.placed = best.items.iter().any(|p| p.id == r.id); if let Some(p) = best.items.iter().find(|p| p.id == r.id) { r.variant = format!("{}{}", p.variant, if p.side { "+side" } else { "" }); } }
    let walk = best.world.walk(&[]);
    let (seen, joined) = best.world.flood(&walk, room);
    let w: Vec<u8> = (0..walk.len()).map(|k| if seen[k] { 2 } else if walk[k] { 1 } else { 0 }).collect();
    Solution { room: room.clone(), placed: best.items, report, score: best.score, ms: t0.elapsed().as_secs_f64() * 1000.0, walk: w, nx: best.world.nx, ny: best.world.ny, ox: best.world.ox, oy: best.world.oy, why_item, doors_joined: joined }
}
