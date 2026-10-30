//! BlueprintSolidifier cutter generation (room/solidifier.py): doors, windows, entrance.
//! (Mesh construction lives in `shell.rs`; this module only reproduces the sampling.)
use super::consts::*;
use super::state::*;
use crate::rng::Rng;
use crate::shp::*;
use indexmap::IndexMap;
use std::collections::VecDeque;
use crate::dhash::{HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutKind {
    Door,
    Window,
    Entrance,
    /// wide doorless opening between open-plan rooms (kitchen / dining / living), no door leaf or frame
    Passage,
}

/// An oriented box cutter, centred at `loc`, rotated by `rot_z` about +z. `dims` are full local extents.
#[derive(Clone, Debug)]
pub struct Cutter {
    pub kind: CutKind,
    pub loc: [f64; 3],
    pub rot_z: f64,
    pub dims: [f64; 3],
    pub rooms: Vec<String>,
}

const SNAP: f64 = 0.5;

pub fn canonicalize_ls(line: &G) -> G {
    let mut line = simplify(line, 0.02);
    loop {
        let coords = coords_of(&line);
        let n = coords.len() - 1;
        let mut diff: Vec<P2> = Vec::with_capacity(n);
        for i in 0..n {
            let d = [coords[i + 1][0] - coords[i][0], coords[i + 1][1] - coords[i][1]];
            let nrm = (d[0] * d[0] + d[1] * d[1]).sqrt() + 1e-6;
            diff.push([d[0] / nrm, d[1] / nrm]);
        }
        let mut pts = vec![coords[0]];
        for i in 0..n.saturating_sub(1) {
            let prod = diff[i][0] * diff[i + 1][0] + diff[i][1] * diff[i + 1][1];
            if 1.0 - 1e-6 > prod && prod > -0.8 {
                pts.push(coords[i + 1]);
            }
        }
        pts.push(*coords.last().unwrap());
        let ls = linestring(&pts).unwrap();
        if length(&ls) < length(&line) {
            line = ls;
        } else {
            return ls;
        }
    }
}

pub fn canonicalize_mls(mls: &G) -> G {
    multilinestring(parts(mls).iter().filter(|p| !is_empty(p)).map(canonicalize_ls).collect()).unwrap()
}

/// all segments (x,y,x_,y_) of an (multi)linestring, optionally only those longer than `min_len`
pub fn split_mls(mls: &G, min_len: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut out = vec![];
    for ls in parts(mls) {
        if is_empty(&ls) {
            continue;
        }
        let c = coords_of(&ls);
        for w in c.windows(2) {
            let l = ((w[0][0] - w[1][0]).powi(2) + (w[0][1] - w[1][1]).powi(2)).sqrt();
            if l > min_len {
                out.push((w[0][0], w[0][1], w[1][0], w[1][1]));
            }
        }
    }
    out
}

pub fn max_mls(mls: &G) -> (f64, f64, f64, f64) {
    let segs = split_mls(mls, f64::NEG_INFINITY);
    let mut best = 0;
    let mut bl = -1.0;
    for (i, s) in segs.iter().enumerate() {
        let l = ((s.0 - s.2).powi(2) + (s.1 - s.3).powi(2)).sqrt();
        if l > bl {
            bl = l;
            best = i;
        }
    }
    segs[best]
}

fn mls_ccw(mls: &G, state: &RoomState, i: &str) -> G {
    let ext = exterior(state.poly(i));
    let coords = &ext[..ext.len() - 1];
    let argmin = |p: P2| {
        let mut bi = 0;
        let mut bd = f64::INFINITY;
        for (j, c) in coords.iter().enumerate() {
            let d = ((c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2)).sqrt();
            if d < bd {
                bd = d;
                bi = j;
            }
        }
        bi
    };
    let lines: Vec<G> = parts(mls)
        .into_iter()
        .filter(|l| !is_empty(l))
        .map(|ls| {
            let c = coords_of(&ls);
            let (x, y) = (argmin(c[0]), argmin(c[1]));
            if x < y {
                ls
            } else {
                let mut r = c.clone();
                r.reverse();
                linestring(&r).unwrap()
            }
        })
        .collect();
    multilinestring(lines).unwrap()
}

pub struct Solidified {
    pub cutters: Vec<Cutter>,
}

pub struct Solidifier<'a> {
    pub c: &'a RoomConstants,
    pub graph: &'a RoomGraph,
    pub level: usize,
    pub enable_open: bool,
    /// probability that a door between two of kitchen / dining room / living room becomes a wide passage (0: off, output unchanged)
    pub passage_prob: f64,
}

fn rotz_dir(d: P2) -> P2 {
    d
}

impl<'a> Solidifier<'a> {
    fn compute_dist2entrance(&self) -> HashMap<String, usize> {
        let root = self.graph.root().to_string();
        let mut q = VecDeque::new();
        let mut dist = HashMap::default();
        dist.insert(root.clone(), 0usize);
        q.push_back(root);
        while let Some(node) = q.pop_front() {
            let d = dist[&node];
            for n in self.graph.valid_neighbours(&node) {
                if !dist.contains_key(n) {
                    dist.insert(n.to_string(), d + 1);
                    q.push_back(n.to_string());
                }
            }
        }
        dist
    }

    pub fn solidify(&self, rng: &mut Rng, state: &RoomState) -> Solidified {
        let ext_name = state.objs.keys().find(|k| room_type(k) == EXTERIOR).unwrap().clone();
        let shared_edges: IndexMap<String, IndexMap<String, G>> = state
            .valid_rooms()
            .map(|(k, o)| {
                (k.clone(), o.relations.iter().map(|r| (r.target.clone(), canonicalize_mls(&r.value))).collect())
            })
            .collect();
        let exterior_edges: IndexMap<String, G> = state.objs[&ext_name]
            .relations
            .iter()
            .map(|r| (r.target.clone(), mls_ccw(&canonicalize_mls(&r.value), state, &r.target)))
            .collect();
        let exterior_shape = state.poly(&ext_name).clone();

        let (door_cutters, _open) = self.make_interior_cutters(rng, state, &shared_edges);
        let (window_cutters, entrance_cutters) = self.make_exterior_cutters(rng, &exterior_edges, &exterior_shape);

        // creation order = naming order in the reference: doors, windows, entrance (unroll order)
        let mut cutters = vec![];
        cutters.extend(door_cutters);
        cutters.extend(window_cutters);
        cutters.extend(entrance_cutters);
        Solidified { cutters }
    }

    fn make_interior_cutters(
        &self,
        rng: &mut Rng,
        state: &RoomState,
        shared_edges: &IndexMap<String, IndexMap<String, G>>,
    ) -> (Vec<Cutter>, Vec<Cutter>) {
        let dist2 = self.compute_dist2entrance();
        let centroids: HashMap<String, P2> =
            state.valid_rooms().map(|(k, o)| (k.clone(), centroid(o.polygon.as_ref().unwrap()))).collect();
        // door cutters keyed in insertion order: (k, l) pairs, emitted in unroll order
        let mut door_map: IndexMap<String, IndexMap<String, Cutter>> = IndexMap::new();
        for (k, ses) in shared_edges {
            for (l, se) in ses {
                if k >= l || length(se) <= self.c.segment_margin {
                    continue;
                }
                let sign = if dist2[k] > dist2[l] { 1.0 } else { -1.0 };
                let direction = rotz_dir([(centroids[k][0] - centroids[l][0]) * sign, (centroids[k][1] - centroids[l][1]) * sign]);
                let adjacent = self.graph.valid_neighbours(l).contains(k.as_str());
                // enable_open=false path: door iff graph-adjacent
                let mut fn_ = if adjacent { "door" } else { "none" };
                // more rooms with two doors: public rooms that share a wall but are not graph neighbours (kitchen / dining / living /
                // hallway) get a second way through, on their own stream (EXTRA_DOOR_P, default 0.7)
                let public = |t: &str| matches!(t, "kitchen" | "dining-room" | "living-room" | "hallway");
                let mut extra_rng = None;
                if !adjacent && public(room_type(k)) && public(room_type(l)) && !(room_type(k) == "hallway" && room_type(l) == "hallway") && length(se) >= 1.5 {
                    let p = std::env::var("EXTRA_DOOR_P").ok().and_then(|v| v.parse().ok()).unwrap_or(crate::plan_rules::get().extra_door_p);
                    let mut er = Rng::seeded(crate::int_hash(&["extra_door", k, l]));
                    if er.f64() < p { fn_ = "door"; extra_rng = Some(er); }
                }
                if let Some(mut er) = extra_rng {
                    let mut cut = self.make_door_cutter(&mut er, se, direction);
                    cut.rooms = vec![k.clone(), l.clone()];
                    door_map.entry(k.clone()).or_default().insert(l.clone(), cut.clone());
                    door_map.entry(l.clone()).or_default().insert(k.clone(), cut);
                    continue;
                }
                assert!(!self.enable_open, "open cutters not ported");
                // open plan: kitchen / dining / living doors on a long enough shared wall become wide passages; own stream, drawn only
                // when passages are on so the door draws (and everything after them) are unchanged otherwise
                let open_pair = |t: &str| matches!(t, "kitchen" | "dining-room" | "living-room");
                if std::env::var_os("PASSAGE_DEBUG").is_some() && self.passage_prob > 0.0 { eprintln!("PASSAGEDBG {k} {l} fn {fn_} len {:.2}", length(se)); }
                if fn_ == "door" && self.passage_prob > 0.0 && open_pair(room_type(k)) && open_pair(room_type(l)) && length(se) >= 2.2 {
                    let mut pr = Rng::seeded(crate::int_hash(&["passage", &format!("{:?}", rng.peek()), k, l]));
                    if pr.f64() < self.passage_prob {
                        let mut cut = self.make_passage_cutter(&mut pr, se);
                        cut.rooms = vec![k.clone(), l.clone()];
                        door_map.entry(k.clone()).or_default().insert(l.clone(), cut.clone());
                        door_map.entry(l.clone()).or_default().insert(k.clone(), cut);
                        continue;
                    }
                }
                if fn_ == "door" {
                    let mut cut = self.make_door_cutter(rng, se, direction);
                    cut.rooms = vec![k.clone(), l.clone()];
                    door_map.entry(k.clone()).or_default().insert(l.clone(), cut.clone());
                    door_map.entry(l.clone()).or_default().insert(k.clone(), cut);
                }
            }
        }
        let mut out = vec![];
        for (k, cs) in &door_map {
            for (l, c) in cs {
                if k < l {
                    out.push(c.clone());
                }
            }
        }
        (out, vec![])
    }

    fn make_exterior_cutters(
        &self,
        rng: &mut Rng,
        exterior_edges: &IndexMap<String, G>,
        exterior: &G,
    ) -> (Vec<Cutter>, Vec<Cutter>) {
        let entrance = if self.graph.entrance.is_some() { Some(self.graph.root().to_string()) } else { None };
        let mut window_cutters: IndexMap<String, Vec<Cutter>> = IndexMap::new();
        let mut entrance_cutters: IndexMap<String, Vec<Cutter>> = IndexMap::new();
        for (k, mls) in exterior_edges {
            if Some(k) == entrance.as_ref() && self.level == 0 {
                continue;
            }
            for ls in parts(mls) {
                if is_empty(&ls) {
                    continue;
                }
                let mut ls = segmentize(&ls, self.c.max_window_length);
                let buffered = buffer_full(&ls, 0.1, Join::Round, Cap::Round, 16, true);
                if area(&intersection(&buffered, exterior)) < area(&buffered) / 2.0 {
                    let mut c = coords_of(&ls);
                    c.reverse();
                    ls = linestring(&c).unwrap();
                }
                // floor-to-ceiling glass on some living / dining room walls (Hypersim's living spaces; python passes 0): own stream
                let pano_p = if k.starts_with("living-room") || k.starts_with("dining-room") { std::env::var("PANO_P").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3) } else { 0.0 };
                let pano = pano_p > 0.0 && Rng::seeded(ig_core::int_hash(&["pano", k, &format!("{:.3}", length(&ls))])).f64() < pano_p;
                if pano && std::env::var_os("PANO_DEBUG").is_some() { eprintln!("PANO {k} wall {:.2} m", length(&ls)); }
                let mut cs = self.make_window_cutter(rng, &ls, if pano { -1.0 } else { 0.0 });
                for c in cs.iter_mut() {
                    c.rooms = vec![k.clone()];
                }
                window_cutters.entry(k.clone()).or_default().extend(cs);
            }
        }
        for (k, mls) in exterior_edges {
            if Some(k) == entrance.as_ref() && self.level == 0 {
                let (x, y, x_, y_) = max_mls(mls);
                let ls = linestring(&[[x, y], [x_, y_]]).unwrap();
                let mut cutter = self.make_entrance_cutter(rng, &ls);
                cutter.rooms = vec![k.clone()];
                entrance_cutters.entry(k.clone()).or_default().push(cutter);
                let rest = difference(mls, &ls);
                if length(&rest) > 0.0 {
                    let pano_p = if k.starts_with("living-room") || k.starts_with("dining-room") { std::env::var("PANO_P").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3) } else { 0.0 };
                    let pano = pano_p > 0.0 && Rng::seeded(ig_core::int_hash(&["pano", k, &format!("{:.3}", length(&rest))])).f64() < pano_p;
                    let mut cs = self.make_window_cutter(rng, &rest, if pano { -1.0 } else { 0.0 });
                    for c in cs.iter_mut() {
                        c.rooms = vec![k.clone()];
                    }
                    window_cutters.entry(k.clone()).or_default().extend(cs);
                }
            }
        }
        // fewer windows where privacy or wall space matters: closets none, bathrooms their widest one, bedrooms their widest two
        // (filtered after generation, so the random stream and every other room's windows stay as they were)
        for (k, cs) in window_cutters.iter_mut() {
            let pr = crate::plan_rules::get();
            let keep = if k.starts_with("closet") { pr.closet_windows } else if k.starts_with("bathroom") { pr.bathroom_windows } else if k.starts_with("bedroom") { pr.bedroom_windows } else { usize::MAX };
            if cs.len() > keep {
                cs.sort_by(|a, b| b.dims[0].partial_cmp(&a.dims[0]).unwrap());
                cs.truncate(keep);
            }
        }
        (window_cutters.into_values().flatten().collect(), entrance_cutters.into_values().flatten().collect())
    }

    fn make_door_cutter(&self, rng: &mut Rng, mls: &G, direction: P2) -> Cutter {
        let m = self.c.door_margin + self.c.door_width / 2.0;
        let (mut x, mut y, x_, y_) = max_mls(mls);
        let vertical = (x - x_).abs() < 0.1;
        let wt = self.c.wall_thickness;
        let z_rot;
        if vertical {
            y = rng.uniform(y.min(y_) + m, y.max(y_) - m);
            z_rot = if direction[0] > 0.0 { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
        } else {
            x = rng.uniform(x.min(x_) + m, x.max(x_) - m);
            z_rot = if direction[1] > 0.0 { 0.0 } else { std::f64::consts::PI };
        }
        Cutter {
            kind: CutKind::Door,
            loc: [x, y, self.c.door_size / 2.0 + wt / 2.0],
            rot_z: z_rot,
            dims: [self.c.door_width, 2.0 * (self.c.door_width + wt / 2.0), self.c.door_size],
            rooms: vec![],
        }
    }

    /// a wide opening centred on the longest piece of the shared wall: 1.6-2.6 m wide (0.35 m of wall kept at each end), up to
    /// 0.25 m below the ceiling
    fn make_passage_cutter(&self, rng: &mut Rng, mls: &G) -> Cutter {
        let (x, y, x_, y_) = max_mls(mls);
        let wt = self.c.wall_thickness;
        let len = ((x_ - x).powi(2) + (y_ - y).powi(2)).sqrt();
        let w = (len - 2.0 * (wt / 2.0 + 0.35)).min(rng.uniform(1.6, 2.6)).max(self.c.door_width);
        let h = (self.c.wall_height - wt - rng.uniform(0.25, 0.45)).max(self.c.door_size);
        Cutter {
            kind: CutKind::Passage,
            loc: [(x + x_) / 2.0, (y + y_) / 2.0, h / 2.0 + wt / 2.0],
            rot_z: (y_ - y).atan2(x_ - x),
            dims: [w, 2.0 * wt + 0.2, h],
            rooms: vec![],
        }
    }

    fn make_entrance_cutter(&self, rng: &mut Rng, mls: &G) -> Cutter {
        let (x, y, x_, y_) = max_mls(mls);
        let length = ((y_ - y).powi(2) + (x_ - x).powi(2)).sqrt();
        let m = self.c.door_margin + self.c.door_width / 2.0;
        let lam = rng.uniform(m / length, 1.0 - m / length);
        let wt = self.c.wall_thickness;
        Cutter {
            kind: CutKind::Entrance,
            loc: [lam * x + (1.0 - lam) * x_, lam * y + (1.0 - lam) * y_, self.c.door_size / 2.0 + wt / 2.0],
            rot_z: (y_ - y).atan2(x_ - x),
            dims: [self.c.door_width, self.c.door_width + 2.0 * wt, self.c.door_size],
            rooms: vec![],
        }
    }

    /// `panoramic` is the room's panoramic probability (0 => no draw, like rg(0.0))
    fn make_window_cutter(&self, rng: &mut Rng, mls: &G, panoramic: f64) -> Vec<Cutter> {
        let mut cutters = vec![];
        let (wt, wm) = (self.c.wall_thickness, self.c.window_margin);
        for (x, y, x_, y_) in split_mls(mls, self.c.door_width) {
            let length = ((y_ - y).powi(2) + (x_ - x).powi(2)).sqrt();
            // panoramic < 0: decided by the caller's own stream (|panoramic| = 1 means panoramic), the plan's rng untouched
            let pano = if panoramic < 0.0 { true } else { panoramic > 0.0 && rng.bool(panoramic) };
            let (x_scale, lam, z_scale, z_loc);
            if pano && self.c.wall_height < 4.0 {
                x_scale = length / 2.0 - wm;
                lam = 0.5;
                z_scale = (self.c.wall_height - wt) / 2.0 - SNAP;
                z_loc = z_scale + wt / 2.0 + SNAP;
            } else {
                x_scale = rng.uniform(self.c.door_width / 2.0, length / 2.0 - wm);
                let m = (x_scale + wm) / length;
                lam = rng.uniform(m, 1.0 - m);
                z_scale = self.c.window_size / 2.0;
                z_loc = z_scale + self.c.window_height + wt / 2.0;
            }
            cutters.push(Cutter {
                kind: CutKind::Window,
                loc: [lam * x + (1.0 - lam) * x_, lam * y + (1.0 - lam) * y_, z_loc],
                rot_z: (y - y_).atan2(x - x_),
                dims: [2.0 * x_scale, 2.0 * wt, 2.0 * z_scale],
                rooms: vec![],
            });
        }
        cutters
    }
}

#[allow(dead_code)]
fn _unused(_: GeometryTypes) {}
