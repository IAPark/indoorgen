//! Furniture states on the solved layout (owner: mess-states worker).
//!
//! `apply` runs on the solved `State` before the visual assets are built: it re-poses chairs, rugs and small decor objects.
//! `post_visuals` (in `visuals.rs`) runs after the visuals are built and edits the assets: bed cloth, cabinet doors / drawers.
//! Every move is checked with the solver's own collision queries (`ig_furn::col::depth`) against all other objects and the room
//! shells; a move is only kept when no penetration depth grows beyond the original layout's (+4 mm).
//! All randomness comes from `MessCfg::rng("states.<kind>")`.
use crate::MessCfg;
use geos::Geom;
use ig_furn::col;
use ig_furn::geom::Pose;
use ig_furn::state::State;
use std::collections::BTreeMap;

pub type V3 = [f64; 3];

/// counts of changed objects per kind, printed to stderr as `MESS states ...`
#[derive(Default, Clone, Debug)]
pub struct Report {
    pub changed: BTreeMap<String, usize>,
    pub candidates: BTreeMap<String, usize>,
    /// worst penetration depth of a changed group vs any other object, before / after its move
    pub worst_before: f64,
    pub worst_after: f64,
    pub violations: usize,
}

impl Report {
    pub fn print(&self) {
        eprintln!(
            "MESS states changed={:?} of={:?} worst_depth before={:.4} after={:.4} new_interpenetrations={}",
            self.changed, self.candidates, self.worst_before, self.worst_after, self.violations
        );
    }
}

pub fn is_chair(c: &str) -> bool {
    matches!(c, "ChairFactory" | "OfficeChairFactory" | "BarChairFactory")
}
fn is_table(c: &str) -> bool {
    matches!(c, "TableDiningFactory" | "SimpleDeskFactory" | "TableCocktailFactory" | "CoffeeTableFactory" | "SideTableFactory")
}
/// classes that hang on walls / ceilings, are fixtures or big furniture: never jittered
fn jitter_skip(c: &str) -> bool {
    matches!(
        c,
        "WallArtFactory" | "MirrorFactory" | "HardwareFactory" | "CeilingLightFactory" | "CeilingClassicLampFactory" | "BalloonFactory" | "SinkFactory"
            | "StandingSinkFactory" | "ToiletFactory" | "BathtubFactory" | "TVFactory" | "MonitorFactory" | "OvenFactory" | "DishwasherFactory"
            | "BeverageFridgeFactory" | "MicrowaveFactory" | "BedFactory" | "RugFactory"
    ) || is_chair(c)
}

fn rot_z(p: V3, a: f64) -> V3 {
    let (s, c) = a.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

/// pose with `dyaw` applied about the world point `pivot`, then translated by `d`
fn moved(p: &Pose, pivot: V3, dyaw: f64, d: V3) -> Pose {
    let mut o = p.o;
    let rel = [p.t[0] - pivot[0], p.t[1] - pivot[1], p.t[2] - pivot[2]];
    let r = rot_z(rel, dyaw);
    o.loc = [(pivot[0] + r[0] + d[0]) as f32, (pivot[1] + r[1] + d[1]) as f32, (pivot[2] + r[2] + d[2]) as f32];
    o.eul[2] = (o.eul[2] as f64 + dyaw) as f32;
    Pose::from_o32(o)
}

fn is_room(state: &State, k: &str) -> bool {
    state.objs[k].polygon.is_some()
}

/// generated objects that rest on `parent` (relation target), transitively
fn children(state: &State, parent: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut stack = vec![parent.to_string()];
    while let Some(p) = stack.pop() {
        for (k, o) in &state.objs {
            if o.generator.is_some() && k != parent && !out.contains(k) && o.relations.iter().any(|r| r.target == p) {
                out.push(k.clone());
                stack.push(k.clone());
            }
        }
    }
    out
}

/// penetration depth against every other generated object and room shell near `name` (cutters excluded), only overlapping ones
pub fn depths(state: &State, name: &str, skip: &[String]) -> BTreeMap<String, f64> {
    let w = state.world(name);
    let mut m = BTreeMap::new();
    for (k, o) in &state.objs {
        if k == name || skip.contains(k) || !(o.generator.is_some() || o.polygon.is_some()) {
            continue;
        }
        let ow = state.world(k);
        if !(0..3).all(|a| ow.lo[a] <= w.hi[a] + 0.02 && ow.hi[a] >= w.lo[a] - 0.02) {
            continue;
        }
        let d = col::depth(state, name, k);
        if d > 0.0 {
            m.insert(k.clone(), d);
        }
    }
    m
}

/// true when every depth after the move is no worse than before (+4 mm)
pub fn ok_vs(base: &BTreeMap<String, f64>, now: &BTreeMap<String, f64>) -> bool {
    now.iter().all(|(k, d)| *d <= base.get(k).copied().unwrap_or(0.0) + 0.004)
}

struct Mover<'a> {
    state: &'a mut State,
    rep: Report,
}

impl<'a> Mover<'a> {
    /// apply (pivot, dyaw, d) to `name` and everything resting on it; keep it when collisions stay acceptable and `extra_ok`, else revert
    fn try_move(&mut self, name: &str, pivot: V3, dyaw: f64, d: V3, skip_extra: &[String], extra_ok: &dyn Fn(&State, &str) -> bool) -> bool {
        let mut group = vec![name.to_string()];
        group.extend(children(self.state, name));
        let mut skip = group.clone();
        skip.extend(skip_extra.iter().cloned());
        let base: Vec<BTreeMap<String, f64>> = group.iter().map(|g| depths(self.state, g, &skip)).collect();
        let old: Vec<Pose> = group.iter().map(|g| self.state.objs[g].pose).collect();
        for (g, p) in group.iter().zip(&old) {
            self.state.set_pose(g, moved(p, pivot, dyaw, d));
        }
        let now: Vec<BTreeMap<String, f64>> = group.iter().map(|g| depths(self.state, g, &skip)).collect();
        if group.iter().zip(base.iter().zip(&now)).all(|(_, (b, n))| ok_vs(b, n)) && extra_ok(self.state, name) {
            let mx = |v: &[BTreeMap<String, f64>]| v.iter().flat_map(|b| b.values().cloned()).fold(0.0, f64::max);
            self.rep.worst_before = self.rep.worst_before.max(mx(&base));
            self.rep.worst_after = self.rep.worst_after.max(mx(&now));
            return true;
        }
        for (g, p) in group.iter().zip(old) {
            self.state.set_pose(g, p);
        }
        false
    }
    fn count(&mut self, kind: &str, changed: bool) {
        *self.rep.candidates.entry(kind.into()).or_default() += 1;
        if changed {
            *self.rep.changed.entry(kind.into()).or_default() += 1;
        }
    }
}

fn center(state: &State, k: &str) -> V3 {
    let o = &state.objs[k];
    o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, (o.geom.lo[2] + o.geom.hi[2]) / 2.0])
}

thread_local! { static LAST: std::cell::RefCell<Option<Report>> = std::cell::RefCell::new(None); }
pub fn last_report() -> Option<Report> {
    LAST.with(|l| l.borrow().clone())
}

/// Mutates poses of the solved scene before visuals are built. No-op when `!cfg.on()`.
pub fn apply(state: &mut State, cfg: &MessCfg) {
    if !cfg.on() {
        return;
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    if std::env::var("MESS_DUMP").is_ok() {
        for (k, o) in &state.objs {
            eprintln!("OBJ {k} cls={:?} t={:.2?} lo={:.2?} hi={:.2?} nproxy={}", o.generator.as_ref().map(|g| g.class.clone()), o.pose.t, o.geom.lo, o.geom.hi, o.geom.proxies.len());
        }
    }
    let names: Vec<(String, String)> = state.objs.iter().filter_map(|(k, o)| o.generator.as_ref().map(|g| (k.clone(), g.class.clone()))).collect();
    let mut mv = Mover { state, rep: Report::default() };

    // ---- 1. chairs: pulled out from the table / desk 0.1-0.5 m and yawed 5-30 degrees (scaled by level)
    let mut rng = cfg.rng("states.chairs");
    for (k, _) in names.iter().filter(|(_, c)| is_chair(c)) {
        let (u_hit, u_pull, u_yaw, u_sign) = (rng.f64(), rng.f64(), rng.f64(), rng.f64());
        let cc = center(mv.state, k);
        let mut best: Option<(f64, V3)> = None;
        for (t, _) in names.iter().filter(|(_, c)| is_table(c)) {
            let w = mv.state.world(t);
            let cp = [cc[0].clamp(w.lo[0], w.hi[0]), cc[1].clamp(w.lo[1], w.hi[1])];
            let dist = ((cc[0] - cp[0]).powi(2) + (cc[1] - cp[1]).powi(2)).sqrt();
            if dist < 0.9 && best.map_or(true, |b| dist < b.0) {
                best = Some((dist, [cc[0] - cp[0], cc[1] - cp[1], 0.0]));
            }
        }
        let mut done = false;
        if let (Some((dist, dir)), true) = (best, u_hit < 0.35 + 0.65 * lvl) {
            let n = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
            // chair centre inside the table's footprint (tucked in): pull along the chair's own back axis (local -x is the seat front)
            let dir = if n < 1e-6 || dist < 1e-6 { let r = mv.state.objs[k].pose.r; [r[0][0], r[1][0], 0.0] } else { [dir[0] / n, dir[1] / n, 0.0] };
            let pull = lvl * (0.2 + 0.8 * u_pull) * 0.5;
            let yaw = lvl * (0.17 + 0.83 * u_yaw) * 30f64.to_radians() * if u_sign < 0.5 { -1.0 } else { 1.0 };
            for (f, yf) in [(1.0, 1.0), (0.7, 1.0), (0.7, -1.0), (0.45, 0.6), (0.3, -0.6), (0.2, 0.3)] {
                if mv.try_move(k, cc, yaw * yf, [dir[0] * pull * f, dir[1] * pull * f, 0.0], &[], &|_, _| true) {
                    done = true;
                    break;
                }
            }
        }
        mv.count("chair", done);
    }

    // ---- 4b. rugs: slightly rotated / offset, kept inside the room polygon; furniture standing on them is not checked
    let mut rng = cfg.rng("states.rugs");
    let furn: Vec<String> = names.iter().map(|n| n.0.clone()).collect();
    for (k, _) in names.iter().filter(|(_, c)| c == "RugFactory") {
        let (a, b, c) = (rng.f64(), rng.f64(), rng.f64());
        let cc = center(mv.state, k);
        let yaw = lvl * (a * 2.0 - 1.0) * 12f64.to_radians();
        let off = [lvl * (b * 2.0 - 1.0) * 0.25, lvl * (c * 2.0 - 1.0) * 0.25];
        let mut done = false;
        for f in [1.0, 0.6, 0.3] {
            if mv.try_move(k, cc, yaw * f, [off[0] * f, off[1] * f, 0.0], &furn, &|s, n| inside_rooms(s, n, 0.05)) {
                done = true;
                break;
            }
        }
        mv.count("rug", done);
    }

    // ---- 5. small decor: 1-10 degrees, 1-3 cm, staying inside the parent's footprint
    let mut rng = cfg.rng("states.decor");
    for (k, c) in &names {
        if jitter_skip(c) {
            continue;
        }
        let o = &mv.state.objs[k];
        let Some(parent) = o.relations.iter().map(|r| r.target.clone()).find(|t| mv.state.objs.get(t).map_or(false, |p| p.generator.is_some())) else { continue };
        let (hl, hh) = (o.geom.lo, o.geom.hi);
        if hh[2] - hl[2] > 0.7 || (hh[0] - hl[0]).max(hh[1] - hl[1]) > 0.8 {
            continue;
        }
        let (a, b, ca, d) = (rng.f64(), rng.f64(), rng.f64(), rng.f64());
        let cc = center(mv.state, k);
        let yaw = lvl * (1.0 + 9.0 * a) * 1f64.to_radians() * if b < 0.5 { -1.0 } else { 1.0 };
        let ang = ca * std::f64::consts::TAU;
        let dist = lvl * (0.01 + 0.02 * d);
        let before = footprint_in_parent(mv.state, k, &parent);
        let done = mv.try_move(k, cc, yaw, [ang.cos() * dist, ang.sin() * dist, 0.0], &[], &|s, n| footprint_in_parent(s, n, &parent) >= before);
        mv.count("decor", done);
    }
    LAST.with(|l| *l.borrow_mut() = Some(mv.rep.clone()));
}

/// world xy corners of the object's local bbox
fn footprint(state: &State, k: &str) -> [[f64; 2]; 4] {
    let o = &state.objs[k];
    let (lo, hi) = (o.geom.lo, o.geom.hi);
    let mut out = [[0.0; 2]; 4];
    for (i, (x, y)) in [(lo[0], lo[1]), (hi[0], lo[1]), (hi[0], hi[1]), (lo[0], hi[1])].iter().enumerate() {
        let w = o.pose.apply([*x, *y, lo[2]]);
        out[i] = [w[0], w[1]];
    }
    out
}

/// number of footprint corners of `k` inside the parent's world AABB (xy)
fn footprint_in_parent(state: &State, k: &str, parent: &str) -> usize {
    let w = state.world(parent);
    footprint(state, k).iter().filter(|c| c[0] >= w.lo[0] && c[0] <= w.hi[0] && c[1] >= w.lo[1] && c[1] <= w.hi[1]).count()
}

/// every footprint corner of `k` lies inside some room polygon, `margin` m away from its boundary
fn inside_rooms(state: &State, k: &str, margin: f64) -> bool {
    footprint(state, k).iter().all(|c| {
        let sq = [[c[0] - margin, c[1] - margin], [c[0] + margin, c[1] - margin], [c[0] + margin, c[1] + margin], [c[0] - margin, c[1] + margin]];
        let Ok(sq) = ig_core::shp::polygon(&sq, &[]) else { return false };
        state.objs.values().any(|o| o.polygon.as_ref().map_or(false, |p| p.contains(&sq).unwrap_or(false)))
    })
}

/// Edits the built visual assets (bed cloth, cabinet doors / drawers). No-op when `!cfg.on()`.
pub fn post_visuals(visuals: &mut std::collections::HashMap<String, ig_kit::api::Asset>, state: &State, cfg: &MessCfg) {
    if !cfg.on() {
        return;
    }
    crate::visuals::post_visuals(visuals, state, cfg);
}
