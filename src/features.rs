//! Room features (fireplaces with chimney breasts, bookshelf walls, window seats): chosen per room right after the floor plan and BEFORE the
//! furniture solve, built flush against one straight wall span, then fixed obstacles (with support surfaces) for the solver and exported
//! scene objects. Geometry lives in `ig_features`; this module holds the config, the wall-span extraction, the per-room decisions and the
//! hook into the pipeline (src/pipeline.rs `generate`) and scene export (src/scenegen.rs).
//!
//! Streams: every decision is drawn from `int_hash(seed, "features", room name, ...)` (own stream, never the floor plan or solver stream),
//! so the factor is independent of the other variation factors; with features off nothing is drawn and the output is byte-identical.
use crate::rooms::consts::RoomConstants;
use crate::rooms::solidify::{CutKind, Cutter};
use crate::rng::Rng;
use crate::shp::G;
use ig_features::{Feature, FeatureAsset, FireplaceFeature, Opening, WallSpan};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Probabilities and limits of the features of one room type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RoomFeatures {
    /// Probability that the room gets a fireplace (masonry chimney breast, mantel, hearth) if a wall span fits.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub fireplace: f64,
    /// Probability of a built-in bookshelf wall.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub bookshelf_wall: f64,
    /// Probability of a window seat under a window.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub window_seat: f64,
    /// At most this many features per room.
    #[schemars(range(min = 0, max = 6))]
    pub max: usize,
}
impl Default for RoomFeatures {
    fn default() -> Self { RoomFeatures { fireplace: 0.0, bookshelf_wall: 0.0, window_seat: 0.0, max: 1 } }
}

/// Room features config (section `features`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Features {
    /// Build room features. Off (default): no features, the output is unchanged. `variation.features.mode` can switch them on/off independently.
    pub on: bool,
    /// Seed of the feature decisions (null: house.seed; `variation.features.seed` wins). Own random stream: floor plan and other factors are unaffected.
    pub seed: Option<u64>,
    /// Probabilities per room type (CamelCase names: LivingRoom, Bedroom, DiningRoom, ...). Room types not listed get no features.
    pub rooms: BTreeMap<String, RoomFeatures>,
}
impl Default for Features {
    fn default() -> Self {
        let rf = |fireplace, bookshelf_wall, window_seat, max| RoomFeatures { fireplace, bookshelf_wall, window_seat, max };
        let rooms = [("LivingRoom", rf(0.5, 0.3, 0.15, 3)), ("Bedroom", rf(0.2, 0.2, 0.3, 3)), ("DiningRoom", rf(0.15, 0.0, 0.1, 1))]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        Features { on: false, seed: None, rooms }
    }
}

/// What the `variation.features` mode does to the section above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FeatureMode {
    /// Follow `features.on` and the configured probabilities.
    #[default]
    Default,
    /// No features, whatever `features` says.
    None,
    /// Features on with the configured probabilities (the default table when `features.rooms` is empty), drawn from the variation seed.
    Random,
    /// Every room type of the table gets every feature that fits (probability 1, up to `max`).
    Force,
}

/// Variation factor `features`: independent of the other factors.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FeatureVar {
    /// Seed of the feature stream (null: features.seed, else house.seed).
    pub seed: Option<u64>,
    pub mode: FeatureMode,
}

/// The resolved decision table for one run; `rooms` empty = features off.
#[derive(Clone, Debug, Default)]
pub struct FeatureCfg {
    pub seed: u64,
    pub rooms: BTreeMap<String, RoomFeatures>,
}

impl FeatureCfg {
    pub fn resolve(f: &Features, v: &FeatureVar, house_seed: u64) -> FeatureCfg {
        let seed = v.seed.or(f.seed).unwrap_or(house_seed);
        let rooms = match v.mode {
            FeatureMode::None => BTreeMap::new(),
            FeatureMode::Default => if f.on { f.rooms.clone() } else { BTreeMap::new() },
            FeatureMode::Random => if f.rooms.is_empty() { Features::default().rooms } else { f.rooms.clone() },
            FeatureMode::Force => {
                let base = if f.rooms.is_empty() { Features::default().rooms } else { f.rooms.clone() };
                base.into_iter().map(|(k, r)| (k, RoomFeatures { fireplace: 1.0, bookshelf_wall: 1.0, window_seat: 1.0, max: r.max.max(3) })).collect()
            }
        };
        FeatureCfg { seed, rooms }
    }
    pub fn on(&self) -> bool { !self.rooms.is_empty() }
}

/// A feature placed in the world.
pub struct PlacedFeature {
    /// state / node key: `<Class>_<room>_<n>` (no '.', '#', '/')
    pub key: String,
    pub class: &'static str,
    pub room: String,
    /// the built asset in the span frame
    pub asset: ig_kit::api::Asset,
    /// span frame -> world: z rotation `yaw` and translation `origin` (x along the wall, y into the room, z up from the finished floor)
    pub origin: [f64; 3],
    pub yaw: f64,
    /// the span (wall) this feature stands on, its x interval, and the footprint polygon in world xy
    pub span_len: f64,
    pub x: [f64; 2],
    pub footprint: Vec<[f64; 2]>,
    /// keep-out boxes in world space (lo, hi), axis aligned bounds of the rotated boxes
    pub keepout: Vec<([f64; 3], [f64; 3])>,
    /// feature-specific facts for the sidecar (breast extents, alcoves, ...)
    pub info: serde_json::Value,
}

impl PlacedFeature {
    pub fn pose(&self) -> crate::furn::geom::Pose { crate::furn::geom::Pose::from_euler(self.origin, [0.0, 0.0, self.yaw]) }
    pub fn world_point(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, s) = (self.yaw.cos(), self.yaw.sin());
        [self.origin[0] + c * p[0] - s * p[1], self.origin[1] + s * p[0] + c * p[1]]
    }
}

/// a straight inner wall face of a room
pub struct Span {
    pub span: WallSpan,
    pub origin: [f64; 2],
    pub yaw: f64,
}

fn camel(rt: &str) -> String {
    rt.split('-').map(|p| { let mut c = p.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect()
}

/// Straight spans of the inner wall faces of one room (collinear vertices merged, `corner` trimmed at both ends), with the doors / windows
/// whose cutter lies on the wall mapped to span coordinates. `x` runs along the CCW ring direction, `y` into the room (right-handed frame:
/// seen from inside with the back to the wall, x runs left to right).
pub fn wall_spans(room_name: &str, room_poly: &G, c: &RoomConstants, cutters: &[Cutter]) -> Vec<Span> {
    let wt = c.wall_thickness;
    let inner = crate::shp::buffer_mitre(room_poly, -wt / 2.0);
    let mut ring = crate::shp::exterior(&inner);
    if ring.len() < 4 { return vec![]; }
    if crate::shp::signed_area(&ring) < 0.0 { ring.reverse(); }
    ring.pop(); // closing point
    // merge collinear vertices
    let mut pts: Vec<[f64; 2]> = vec![];
    let n = ring.len();
    for i in 0..n {
        let (a, b, d) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        let cr = (b[0] - a[0]) * (d[1] - b[1]) - (b[1] - a[1]) * (d[0] - b[0]);
        let (l1, l2) = ((b[0] - a[0]).hypot(b[1] - a[1]), (d[0] - b[0]).hypot(d[1] - b[1]));
        if l1 < 1e-6 || l2 < 1e-6 || cr.abs() / (l1 * l2) < 1e-4 { continue }
        pts.push(b);
    }
    let rt = camel(crate::rooms::state::room_type(room_name));
    let floor_z = wt / 2.0;
    let height = c.wall_height - wt;
    let corner = 0.05;
    let mut out = vec![];
    let m = pts.len();
    for i in 0..m {
        let (p, q) = (pts[i], pts[(i + 1) % m]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        if len < 1.0 { continue }
        let u = [(q[0] - p[0]) / len, (q[1] - p[1]) / len];
        let nrm = [-u[1], u[0]];
        let mut openings = vec![];
        for ct in cutters {
            if !ct.rooms.iter().any(|r| r == room_name) { continue }
            // cutter local x = width, y = thickness; it must be parallel to the span and its centre on the wall plane
            let (cx, cy) = (ct.rot_z.cos(), ct.rot_z.sin());
            if (cx * u[0] + cy * u[1]).abs() < 0.99 { continue }
            let rel = [ct.loc[0] - p[0], ct.loc[1] - p[1]];
            let (along, off) = (rel[0] * u[0] + rel[1] * u[1], rel[0] * nrm[0] + rel[1] * nrm[1]);
            if off.abs() > wt + 0.05 { continue }
            let hw = ct.dims[0] / 2.0;
            if along + hw < -0.05 || along - hw > len + 0.05 { continue }
            let z0 = ct.loc[2] - ct.dims[2] / 2.0 - floor_z;
            openings.push(Opening { is_window: ct.kind == CutKind::Window, x: [along - hw - corner, along + hw - corner], z: [z0.max(0.0), z0 + ct.dims[2]] });
        }
        // trim the corners: origin moves along the wall by `corner`
        let o = [p[0] + u[0] * corner, p[1] + u[1] * corner];
        out.push(Span {
            span: WallSpan { length: len - 2.0 * corner, height, wall_thickness: wt, openings, room_type: rt.clone() },
            origin: o,
            yaw: u[1].atan2(u[0]),
        });
    }
    out
}

fn registry() -> Vec<(&'static str, Box<dyn Feature>)> {
    let mut v: Vec<(&'static str, Box<dyn Feature>)> = vec![("fireplace", Box::new(FireplaceFeature))];
    for f in ig_features::builtin_features() {
        let kind = match f.class() { "BookshelfWallFeature" => "bookshelf_wall", "WindowSeatFeature" => "window_seat", _ => continue };
        v.push((kind, f));
    }
    // window seats before bookshelf walls: the seat needs its window wall, the bookshelf takes whatever wall is left
    v.sort_by_key(|(k, _)| match *k { "fireplace" => 0, "window_seat" => 1, _ => 2 });
    v
}

fn prob(rf: &RoomFeatures, kind: &str) -> f64 {
    match kind {
        "fireplace" => rf.fireplace,
        "bookshelf_wall" => rf.bookshelf_wall,
        "window_seat" => rf.window_seat,
        _ => 0.0,
    }
}

/// Decide and build the features of every room. `room_polys`: (room name, floor polygon) as in `Generated.room_polys`.
pub fn plan(cfg: &FeatureCfg, c: &RoomConstants, room_polys: &[(String, G)], cutters: &[Cutter]) -> Vec<PlacedFeature> {
    if !cfg.on() { return vec![] }
    let reg = registry();
    let mut out: Vec<PlacedFeature> = vec![];
    for (room, poly) in room_polys {
        let rt = camel(crate::rooms::state::room_type(room));
        let Some(rf) = cfg.rooms.get(&rt) else { continue };
        let mut spans = wall_spans(room, poly, c, cutters);
        let mut rng = Rng::seeded(crate::int_hash(&[&cfg.seed.to_string(), "features", room]));
        let mut n = 0usize;
        for (kind, feat) in &reg {
            // draws are made for every (room, kind) whether or not the feature fits, so one kind's fit does not shift another's stream
            let roll = rng.f64();
            let pick = rng.f64();
            let fseed = rng.next_u64();
            if n >= rf.max || roll >= prob(rf, kind) { continue }
            // candidates: (span index, interval), weighted by interval length (long walls first)
            let mut cands: Vec<(usize, [f64; 2])> = vec![];
            for (si, s) in spans.iter().enumerate() {
                for iv in feat.fits(&s.span) { cands.push((si, iv)) }
            }
            if cands.is_empty() { continue }
            let w: Vec<f64> = cands.iter().map(|(_, iv)| (iv[1] - iv[0]).powi(2)).collect();
            let tot: f64 = w.iter().sum();
            let mut acc = 0.0;
            let mut ci = cands.len() - 1;
            for (i, wi) in w.iter().enumerate() {
                acc += wi / tot;
                if pick < acc { ci = i; break }
            }
            let (si, iv) = cands[ci];
            let fa: FeatureAsset = feat.build(&spans[si].span, iv, fseed);
            // x extent of the built asset, to block the wall for later features
            let (lo, hi) = fa.asset.bounds();
            let s = &spans[si];
            let (cs, sn) = (s.yaw.cos(), s.yaw.sin());
            let wp = |p: [f64; 2]| [s.origin[0] + cs * p[0] - sn * p[1], s.origin[1] + sn * p[0] + cs * p[1]];
            let footprint: Vec<[f64; 2]> = fa.footprint.iter().map(|p| wp(*p)).collect();
            let keepout: Vec<([f64; 3], [f64; 3])> = fa.keepout.iter().map(|(a, b)| {
                let cs4 = [wp([a[0], a[1]]), wp([b[0], a[1]]), wp([b[0], b[1]]), wp([a[0], b[1]])];
                let (mut l, mut h) = ([f64::MAX, f64::MAX, a[2]], [f64::MIN, f64::MIN, b[2]]);
                for p in cs4 { for d in 0..2 { l[d] = l[d].min(p[d]); h[d] = h[d].max(p[d]); } }
                (l, h)
            }).collect();
            let info = match fa.class {
                "FireplaceFeature" => {
                    let l = FireplaceFeature::layout(&s.span, iv, fseed);
                    serde_json::json!({"style": format!("{:?}", l.style), "breast_x": l.breast, "depth": l.depth, "firebox_x": l.firebox, "firebox_z": l.firebox_z,
                        "hearth_x": l.hearth, "hearth_depth": l.hearth_depth, "hearth_h": l.hearth_h, "mantel_z": l.mantel_z, "alcoves_x": l.alcoves, "span_len": s.span.length})
                }
                _ => serde_json::json!({"span_len": s.span.length, "x": iv}),
            };
            let key = format!("{}_{}_{}", fa.class, room.replace('/', "_"), n);
            out.push(PlacedFeature {
                key,
                class: fa.class,
                room: room.clone(),
                asset: fa.asset,
                origin: [s.origin[0], s.origin[1], c.wall_thickness / 2.0],
                yaw: s.yaw,
                span_len: s.span.length,
                x: [lo[0], hi[0]],
                footprint,
                keepout,
                info,
            });
            // block the used part of the wall for the next feature
            let hh = spans[si].span.height;
            spans[si].span.openings.push(Opening { is_window: false, x: [lo[0] - 0.05, hi[0] + 0.05], z: [0.0, hh] });
            n += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_modes() {
        let mut f = Features::default();
        let v = FeatureVar::default();
        assert!(!FeatureCfg::resolve(&f, &v, 3).on());
        f.on = true;
        assert!(FeatureCfg::resolve(&f, &v, 3).on());
        let none = FeatureVar { mode: FeatureMode::None, ..Default::default() };
        assert!(!FeatureCfg::resolve(&f, &none, 3).on());
        f.on = false;
        let force = FeatureVar { mode: FeatureMode::Force, seed: Some(9) };
        let r = FeatureCfg::resolve(&f, &force, 3);
        assert!(r.on() && r.seed == 9 && r.rooms["LivingRoom"].fireplace == 1.0);
    }
}
