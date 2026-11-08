//! Dataset camera pool in Rust (`gen2` with DSPOOL=spec.json): a port of tools/dataset/dscap.py's pose search (`make_pool`: fresh
//! look-at-the-room poses aimed at furniture, short trajectory steps and overlap views, best-of-8 fresh candidates with a novelty
//! penalty) and of its view rules (`view_stats`: eye clearance, feet over the floor, a 24 x 18 frustum probe with the composition
//! rules), ray cast against the scene with parry3d's BVH. Writes the cameras_pool.json dscap / the preview engine read, so a
//! preview house needs no Blender process. Random streams differ from numpy's: same distributions and rules, not the same views.
use ig_core::mesh::{Scene, V3};
use ig_core::rng::Rng;
use parry3d_f64::na::{Point3, Vector3};
use parry3d_f64::query::{Ray, RayCast};
use parry3d_f64::shape::{FeatureId, TriMesh};
use serde_json::{json, Value};
use std::collections::HashMap;

/// the furniture a photo of each room type is "of" (dscap.ANCHORS)
fn anchors(room: &str) -> &'static [&'static str] {
    for (k, v) in [
        ("bedroom", &["BedFactory"][..]),
        ("living-room", &["SofaFactory", "TVStandFactory"][..]),
        ("dining-room", &["TableDiningFactory"][..]),
        ("kitchen", &["KitchenSpaceFactory", "FridgeFactory", "KitchenIslandFactory", "OvenFactory", "SinkFactory"][..]),
        ("bathroom", &["BathtubFactory", "ToiletFactory", "BathroomSinkFactory", "StandingSinkFactory"][..]),
    ] {
        if room.starts_with(k) { return v }
    }
    &[]
}

/// 1 wall, 2 floor, 3 ceiling (room shell names "<room>.wall(.alt)(.NNN)"), 0 everything else
fn kind_of(name: &str) -> u8 {
    let mut s = name;
    if let Some(i) = s.rfind('.') { if s[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < s.len() { s = &s[..i] } }
    let s = s.strip_suffix(".alt").unwrap_or(s);
    if s.ends_with(".wall") { 1 } else if s.ends_with(".floor") { 2 } else if s.ends_with(".ceiling") { 3 } else { 0 }
}

fn okey(name: &str) -> String {
    let k = name.split('#').next().unwrap_or(name);
    match k.rfind('.') { Some(i) if k[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < k.len() => k[..i].to_string(), _ => k.to_string() }
}

struct World {
    tm: TriMesh,
    ntri: usize,
    tri_obj: Vec<u32>,
    kind: Vec<u8>,
    okeys: Vec<String>,
    tri_n: Vec<V3>,
}

struct Hit { dist: f64, tri: usize, n: V3 }

impl World {
    fn cast(&self, o: V3, d: V3, max_t: f64) -> Option<Hit> {
        let r = Ray::new(Point3::new(o[0], o[1], o[2]), Vector3::new(d[0], d[1], d[2]));
        let h = self.tm.cast_local_ray_and_get_normal(&r, max_t, false)?;
        let tri = match h.feature { FeatureId::Face(f) => f as usize % self.ntri, _ => 0 };
        Some(Hit { dist: h.time_of_impact, tri, n: self.tri_n[tri] })
    }
    fn tkind(&self, t: usize) -> u8 { self.kind[self.tri_obj[t] as usize] }
    fn tkey(&self, t: usize) -> &str { &self.okeys[self.tri_obj[t] as usize] }
}

fn sub(a: V3, b: V3) -> V3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn dot(a: V3, b: V3) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn norm(a: V3) -> f64 { dot(a, a).sqrt() }
fn unit(a: V3) -> V3 { let l = norm(a).max(1e-12); [a[0] / l, a[1] / l, a[2] / l] }

/// the room being photographed: floor triangles (pose sampling) and their outline (wall margin)
struct Room { name: String, tris: Vec<[V3; 3]>, area: Vec<f64>, total: f64, segs: Vec<([f64; 2], [f64; 2])>, floor_z: f64 }

impl Room {
    fn edge_dist(&self, p: [f64; 2]) -> f64 {
        if self.segs.is_empty() { return 9.0 }
        self.segs.iter().map(|(a, b)| {
            let ab = [b[0] - a[0], b[1] - a[1]];
            let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-12)).clamp(0.0, 1.0);
            (a[0] + ab[0] * t - p[0]).hypot(a[1] + ab[1] * t - p[1])
        }).fold(f64::MAX, f64::min)
    }
    fn sample(&self, r: &mut Rng) -> V3 {
        let mut x = r.f64() * self.total;
        let mut i = 0;
        while i + 1 < self.area.len() && x > self.area[i] { x -= self.area[i]; i += 1; }
        let [a, b, c] = self.tris[i];
        let (mut u, mut v) = (r.f64(), r.f64());
        if u + v > 1.0 { u = 1.0 - u; v = 1.0 - v; }
        [a[0] + u * (b[0] - a[0]) + v * (c[0] - a[0]), a[1] + u * (b[1] - a[1]) + v * (c[1] - a[1]), a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2])]
    }
}

/// c2w (OpenCV axes: x right, y down, z forward) for a camera at pos, azimuth yaw, pitch (dscap.cam_pose_cv)
fn cam_r(yaw: f64, pitch: f64) -> [V3; 3] {
    let fwd = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()];
    let right = unit(cross(fwd, [0.0, 0.0, 1.0]));
    let down = cross(fwd, right);
    [right, down, fwd] // columns
}

#[derive(Clone)]
struct Stats { med: f64, nobj: usize, ncls: usize, objfrac: f64, wall: f64, anchor: f64, person: f64, win: f64, p10: f64, miss: f64, near_low: f64 }

fn view_score(s: &Stats) -> f64 {
    1.5 * s.ncls as f64 + 0.3 * (s.nobj.min(60) as f64) + 12.0 * s.objfrac - 6.0 * s.wall + 40.0 * s.anchor.min(0.15) - 45.0 * s.person + 15.0 * s.win.min(0.2)
}

const EYE: [V3; 18] = [[1., 0., 0.], [-1., 0., 0.], [0., 1., 0.], [0., -1., 0.], [0., 0., 1.], [0., 0., -1.], [0.7, 0.7, 0.], [-0.7, 0.7, 0.], [0.7, -0.7, 0.], [-0.7, -0.7, 0.],
    [0.7, 0., 0.7], [-0.7, 0., 0.7], [0., 0.7, 0.7], [0., -0.7, 0.7], [0.7, 0., -0.7], [-0.7, 0., -0.7], [0., 0.7, -0.7], [0., -0.7, -0.7]];

fn view_stats(w: &World, rm: &Room, has_anchor: bool, pos: V3, yaw: f64, pitch: f64, f: f64, wd: usize, ht: usize, strict: bool) -> Result<Stats, &'static str> {
    view_stats_mode(w, rm, has_anchor, pos, yaw, pitch, f, wd, ht, strict, false)
}

/// `snap`: a family snapshot -- people may fill the frame and stand close, the room is background (no anchor, fewer objects needed)
fn view_stats_mode(w: &World, rm: &Room, has_anchor: bool, pos: V3, yaw: f64, pitch: f64, f: f64, wd: usize, ht: usize, strict: bool, snap: bool) -> Result<Stats, &'static str> {
    for d in EYE {
        if let Some(h) = w.cast(pos, unit(d), 0.3) {
            if h.dist < 0.22 || (w.tkind(h.tri) == 0 && dot(h.n, d) > 0.0) { return Err(if w.tkind(h.tri) == 0 { "eye-in-object" } else { "eye-near-wall" }) }
        }
    }
    if let Some(h) = w.cast(pos, [0.0, 0.0, -1.0], 100.0) {
        if w.tkind(h.tri) == 0 && h.dist < (pos[2] - rm.floor_z) - 0.12 && !w.tkey(h.tri).contains("Rug") { return Err("over-furniture") }
    }
    let (gw, gh) = (24usize, 18usize);
    let r = cam_r(yaw, pitch);
    let anc = anchors(&rm.name);
    let lin = |n: usize, m: usize| -> Vec<usize> { (0..m).map(|i| ((n - 1) as f64 * i as f64 / (m - 1) as f64) as usize).collect() };
    let (ys, xs) = (lin(ht, gh), lin(wd, gw));
    let (mut ds, mut miss, mut wall, mut nobjpix, mut person, mut anchor, mut pnear, mut near_low, mut n_low, mut pend, mut door, mut win) = (vec![], 0, 0, 0, 0, 0, 1e9f64, 0, 0, 0, 0, 0);
    let mut objs: std::collections::BTreeSet<String> = Default::default();
    let mut per: HashMap<String, usize> = HashMap::new();
    let mut near_obj: HashMap<String, usize> = HashMap::new();
    let (mut edge_n, mut edge_near) = ([0usize; 2], [0usize; 2]);
    let (xl, xr) = (xs[gw / 5], xs[gw - 1 - gw / 5]);
    let ylow = ys[gh * 2 / 3];
    for &y in &ys {
        for &x in &xs {
            let (u, v) = ((x as f64 + 0.5 - wd as f64 / 2.0) / f, (y as f64 + 0.5 - ht as f64 / 2.0) / f);
            let d = unit([r[0][0] * u + r[1][0] * v + r[2][0], r[0][1] * u + r[1][1] * v + r[2][1], r[0][2] * u + r[1][2] * v + r[2][2]]);
            let Some(h) = w.cast(pos, d, 100.0) else { miss += 1; continue };
            ds.push(h.dist);
            let kd = w.tkind(h.tri);
            if kd == 1 || kd == 3 { wall += 1 }
            if y >= ylow { n_low += 1; if kd == 0 && h.dist < 1.1 { near_low += 1 } }
            if h.dist < 1.1 { *near_obj.entry(if kd == 0 { w.tkey(h.tri).to_string() } else { "_wall".into() }).or_default() += 1 }
            if kd == 0 && h.dist < 1.2 && w.tkey(h.tri).starts_with("pendant") { pend += 1 }
            if x <= xl || x >= xr { let s = if x <= xl { 0 } else { 1 }; edge_n[s] += 1; if h.dist < 0.8 { edge_near[s] += 1 } }
            if kd == 0 {
                let k = w.tkey(h.tri);
                if (k.contains("Door") || k.starts_with("door")) && h.dist < 2.0 { door += 1 }
                if k.contains("Window") || k.starts_with("window") { win += 1 }
                if !(k.contains("Window") || k.contains("Door") || k.starts_with("window") || k.starts_with("door")) {
                    objs.insert(k.to_string()); nobjpix += 1; *per.entry(k.to_string()).or_default() += 1;
                    if k.starts_with("person") { person += 1; pnear = pnear.min(h.dist) }
                    if anc.iter().any(|a| k.contains(a)) { anchor += 1 }
                }
            }
        }
    }
    let n = (gw * gh) as f64;
    if ds.is_empty() { return Err("all-miss") }
    ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| { let i = p * (ds.len() - 1) as f64; let (a, b) = (i.floor() as usize, i.ceil() as usize); ds[a] + (ds[b] - ds[a]) * (i - a as f64) };
    let ncls = objs.iter().map(|k| { let s = k.trim_start_matches(|c: char| c.is_ascii_digit()); let s = s.strip_prefix('_').unwrap_or(s); s.split('.').next().unwrap_or("").to_string() }).collect::<std::collections::BTreeSet<_>>().len();
    let st = Stats { med: q(0.5), p10: q(0.1), miss: miss as f64 / n, wall: wall as f64 / n, nobj: objs.len(), ncls, objfrac: nobjpix as f64 / n, person: person as f64 / n, anchor: anchor as f64 / n, near_low: near_low as f64 / n_low.max(1) as f64, win: win as f64 / n };
    let maxper = per.values().cloned().max().unwrap_or(0) as f64 / n;
    if snap {
        // (the photo is of the people: the room only has to be there behind them)
        if st.p10 < 0.3 { return Err("too-close") }
        if st.miss > 0.2 { return Err("sees-void") }
        if st.wall > 0.6 { return Err("mostly-wall") }
        if pend > 0 { return Err("pendant-at-eye") }
        if st.person < 0.12 { return Err("person-small") }
        if st.person > 0.6 { return Err("person-fills-frame") }
        if pnear < 1.0 { return Err("person-too-close") }
        if maxper > 0.45 { return Err("one-object-fills-frame") }
        return Ok(st)
    }
    if st.p10 < 0.35 { return Err("too-close") }
    if strict && st.near_low > 0.3 { return Err("foreground-blocked") }
    if strict && near_obj.values().cloned().max().unwrap_or(0) as f64 / n > 0.16 { return Err("near-object-fills-frame") }
    if st.med < (0.38 * rm.total.max(1.0).sqrt()).clamp(1.0, 1.3) { return Err("near-median") }
    if st.miss > 0.2 { return Err("sees-void") }
    if st.wall > 0.45 { return Err("mostly-wall") }
    if st.ncls < 5 { return Err("few-classes") }
    if pend > 0 { return Err("pendant-at-eye") }
    if !anc.is_empty() && has_anchor && st.anchor < 0.03 { return Err("no-anchor") }
    if strict && (0..2).any(|i| edge_n[i] > 0 && edge_near[i] as f64 / edge_n[i] as f64 > 0.6) { return Err("edge-occluded") }
    if st.nobj < 4 { return Err("few-objects") }
    if st.person > 0.15 { return Err("person-fills-frame") }
    if maxper > 0.3 { return Err("one-object-fills-frame") }
    if door as f64 / n > 0.12 { return Err("door-fills-frame") }
    if door as f64 / n + maxper > 0.4 { return Err("door-and-object-fill-frame") }
    if pnear < 1.6 { return Err("person-too-close") }
    Ok(st)
}

#[derive(Clone)]
struct View { pos: V3, yaw: f64, pitch: f64, f_mm: f64, kind: &'static str, hit: V3, st: Stats, relaxed: bool }

fn make_pool(w: &World, rm: &Room, targets: &[(V3, f64)], has_anchor: bool, cfg: &Value, wd: usize, ht: usize, n: usize, rng: &mut Rng, strict: bool, rejects: &mut HashMap<String, usize>) -> (Vec<View>, usize) {
    let g = |k: &str, i: usize, d: f64| cfg.get(k).and_then(|v| v.get(i)).and_then(|v| v.as_f64()).unwrap_or(d);
    let (mut lo_f, mut hi_f, mut hlo, hhi) = (g("focal_mm", 0, 18.0), g("focal_mm", 1, 35.0), g("eye_height_m", 0, 1.0), g("eye_height_m", 1, 1.8));
    let mut margin = cfg.get("wall_margin_m").and_then(|v| v.as_f64()).unwrap_or(0.45);
    let rr = rm.total.max(1.0).sqrt();
    margin = margin.min((0.12 * rr).max(0.25));
    let lens_min = std::env::var("LENS_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(24.0);
    lo_f = lo_f.max(lens_min); hi_f = hi_f.max(lo_f + 6.0);
    if rm.total < 10.0 { lo_f = lo_f.min(20.0); hi_f = hi_f.min(28.0); } else if rm.name.starts_with("bedroom") { hlo = hlo.max(1.35); }
    let d_tgt = (0.5 * rr).clamp(1.2, 2.0);
    let traj = cfg.get("traj_frac").and_then(|v| v.as_f64()).unwrap_or(0.4);
    let cluster = cfg.get("cluster_len").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let best_of = cfg.get("fresh_candidates").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
    let tw: f64 = targets.iter().map(|t| t.1).sum();
    let mut pool: Vec<View> = vec![];
    let (mut prev, mut run, mut tries): (Option<View>, usize, usize) = (None, 0, 0);
    let mut fresh_best: Vec<View> = vec![];
    let mut rej = |r: &str| *rejects.entry(r.to_string()).or_default() += 1;
    while pool.len() < n && tries < n * 400 {
        tries += 1;
        let fresh = prev.is_none() || run >= cluster || !fresh_best.is_empty();
        let kind = if fresh { "fresh" } else if rng.f64() < traj { "traj" } else { "overlap" };
        let (mut pos, yaw, pitch, f);
        if kind == "traj" {
            let p = prev.as_ref().unwrap();
            let (ang, step) = (rng.uniform(0.0, std::f64::consts::TAU), rng.uniform(0.3, 0.7));
            pos = [p.pos[0] + ang.cos() * step, p.pos[1] + ang.sin() * step, p.pos[2] + rng.uniform(-0.1, 0.1)];
            yaw = p.yaw + rng.uniform(-20.0, 20.0).to_radians();
            pitch = p.pitch + rng.normal(0.0, 3.0).to_radians();
            f = (p.f_mm * rng.uniform(0.9, 1.1)).clamp(lo_f, hi_f);
            pos[2] = pos[2].clamp(rm.floor_z + hlo, rm.floor_z + hhi);
        } else {
            let p2 = rm.sample(rng);
            pos = [p2[0], p2[1], 0.0];
            let tgt: V3;
            if kind == "overlap" {
                let h = prev.as_ref().unwrap().hit;
                tgt = [h[0] + rng.normal(0.0, 0.3), h[1] + rng.normal(0.0, 0.3), h[2] + rng.normal(0.0, 0.3)];
            } else if !targets.is_empty() && rng.f64() < 0.85 {
                let mut x = rng.f64() * tw; let mut i = 0;
                while i + 1 < targets.len() && x > targets[i].1 { x -= targets[i].1; i += 1; }
                let mut t = targets[i].0; t[2] = t[2].clamp(rm.floor_z + 0.3, rm.floor_z + 1.5);
                tgt = t;
                let mut q = rm.sample(rng);
                for _ in 0..20 {
                    q = rm.sample(rng);
                    if rm.edge_dist([q[0], q[1]]) < 1.3 && (tgt[0] - q[0]).hypot(tgt[1] - q[1]) > d_tgt { break }
                }
                pos = [q[0], q[1], 0.0];
            } else {
                let q = rm.sample(rng);
                tgt = [q[0], q[1], rng.uniform(0.4, 1.4) + rm.floor_z];
            }
            if (tgt[0] - pos[0]).hypot(tgt[1] - pos[1]) < 1.5f64.min(d_tgt) { continue }
            f = (rng.uniform(lo_f.ln(), hi_f.ln())).exp();
            pos[2] = rm.floor_z + rng.uniform(hlo, hhi);
            let dv = sub(tgt, pos);
            yaw = dv[1].atan2(dv[0]) + rng.normal(0.0, 10.0).to_radians();
            pitch = dv[2].atan2(dv[0].hypot(dv[1]) + 1e-9);
        }
        let pitch = pitch.clamp((-22.0f64).to_radians(), 10.0f64.to_radians());
        if rm.edge_dist([pos[0], pos[1]]) < margin { rej("wall-margin"); continue }
        let fpx = f / 36.0 * wd as f64;
        let st = match view_stats(w, rm, has_anchor, pos, yaw, pitch, fpx, wd, ht, strict) { Ok(s) => s, Err(e) => { rej(e); continue } };
        let fwd = cam_r(yaw, pitch)[2];
        let hit = match w.cast(pos, fwd, 100.0) { Some(h) => [pos[0] + fwd[0] * h.dist, pos[1] + fwd[1] * h.dist, pos[2] + fwd[2] * h.dist], None => [pos[0] + fwd[0] * st.med, pos[1] + fwd[1] * st.med, pos[2] + fwd[2] * st.med] };
        let mut v = View { pos, yaw, pitch, f_mm: f, kind, hit, st, relaxed: !strict };
        if kind == "fresh" {
            fresh_best.push(v.clone());
            let need = if tries < n * 100 { best_of } else if tries < n * 250 { 2 } else { 1 };
            if fresh_best.len() < need && tries < n * 400 - 1 { continue }
            let seen: Vec<&View> = pool.iter().filter(|u| u.kind == "fresh").collect();
            let nov = |c: &View| -> f64 {
                let mut pen = 0.0;
                for u in &seen {
                    pen += 4.0 * (-((c.hit[0] - u.hit[0]).powi(2) + (c.hit[1] - u.hit[1]).powi(2)) / 1.5).exp();
                    let dyaw = ((c.yaw - u.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
                    pen += 4.0 * (-((c.pos[0] - u.pos[0]).powi(2) + (c.pos[1] - u.pos[1]).powi(2)) / 1.0).exp() * (-(dyaw / 0.6).powi(2)).exp();
                }
                -pen
            };
            v = fresh_best.iter().max_by(|a, b| (view_score(&a.st) + nov(a)).partial_cmp(&(view_score(&b.st) + nov(b))).unwrap()).unwrap().clone();
            fresh_best.clear();
        }
        run = if v.kind == "fresh" { 1 } else { run + 1 };
        prev = Some(v.clone());
        pool.push(v);
    }
    (pool, tries)
}

/// family-snapshot poses: aimed at a person (or the middle of two near each other) in the room, from 1.2-2.8 m, at standing or
/// sitting eye height, phone-like lens; the person must be the first thing the ray to them meets
fn snapshot_views(w: &World, rm: &Room, people: &[(V3, V3, Option<f64>)], wd: usize, ht: usize, n: usize, rng: &mut Rng, rejects: &mut HashMap<String, usize>) -> (Vec<View>, usize) {
    let mut out: Vec<View> = vec![]; let mut tries = 0;
    if people.is_empty() || n == 0 { return (out, 0) }
    while out.len() < n && tries < n * 300 {
        tries += 1;
        let (lo, hi, face) = people[(rng.f64() * people.len() as f64) as usize % people.len()];
        let mut t = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, hi[2] - 0.22];
        // (two people within 1.6 m: frame them together half the time)
        if people.len() > 1 && rng.f64() < 0.5 {
            let (lo2, hi2, _) = people[(rng.f64() * people.len() as f64) as usize % people.len()];
            let t2 = [(lo2[0] + hi2[0]) / 2.0, (lo2[1] + hi2[1]) / 2.0, hi2[2] - 0.22];
            if (t2[0] - t[0]).hypot(t2[1] - t[1]) < 1.6 { t = [(t[0] + t2[0]) / 2.0, (t[1] + t2[1]) / 2.0, (t[2] + t2[2]) / 2.0] }
        }
        // (in front of the person, as a family photo is taken: within 50 degrees of the way they face; anywhere for someone lying down)
        let az = match face { Some(f) => f + rng.uniform(-50.0, 50.0).to_radians(), None => rng.uniform(0.0, std::f64::consts::TAU) };
        let d = rng.uniform(1.2, 2.2);
        let eye = if rng.f64() < 0.7 { rng.uniform(1.25, 1.65) } else { rng.uniform(0.9, 1.1) };
        let pos = [t[0] + d * az.cos(), t[1] + d * az.sin(), rm.floor_z + eye];
        if rm.edge_dist([pos[0], pos[1]]) < 0.3 { *rejects.entry("snap-wall-margin".into()).or_default() += 1; continue }
        let dv = sub(t, pos); let dist = norm(dv);
        match w.cast(pos, unit(dv), dist + 0.3) {
            Some(h) if w.tkey(h.tri).starts_with("person") => {}
            _ => { *rejects.entry("snap-person-hidden".into()).or_default() += 1; continue }
        }
        let yaw = dv[1].atan2(dv[0]) + rng.normal(0.0, 4.0).to_radians();
        let pitch = (dv[2].atan2(dv[0].hypot(dv[1]))).clamp((-25.0f64).to_radians(), 15.0f64.to_radians());
        let f = rng.uniform(24.0, 35.0);
        let fpx = f / 36.0 * wd as f64;
        match view_stats_mode(w, rm, false, pos, yaw, pitch, fpx, wd, ht, false, true) {
            Ok(st) => out.push(View { pos, yaw, pitch, f_mm: f, kind: "snapshot", hit: t, st, relaxed: false }),
            Err(e) => { *rejects.entry(format!("snap-{e}")).or_default() += 1; }
        }
    }
    (out, tries)
}

/// Build the pool for `room` (e.g. "living-room_0/0") and return the cameras_pool.json value. `spec`: width, height, n_views,
/// camera_seed, cameras {focal_mm, eye_height_m, wall_margin_m, traj_frac, cluster_len, fresh_candidates, pool_factor}.
pub fn make(scene: &Scene, room: &str, spec: &Value) -> Value {
    let t0 = std::time::Instant::now();
    // one BVH over every visible mesh (dscap: every render-visible Blender mesh)
    let (mut pts, mut idx, mut tri_obj, mut tri_n) = (vec![], vec![], vec![], vec![]);
    let (mut kind, mut okeys) = (vec![], vec![]);
    let mut floor: Vec<[V3; 3]> = vec![];
    for n in &scene.nodes {
        if n.mesh.is_empty() { continue }
        let oi = okeys.len() as u32;
        kind.push(kind_of(&n.name)); okeys.push(okey(&n.name));
        let is_floor = n.name.starts_with(room) && n.name.contains(".floor");
        for t in n.mesh.triangles() {
            let (a, b, c) = (n.mesh.pos[t[0] as usize], n.mesh.pos[t[1] as usize], n.mesh.pos[t[2] as usize]);
            let nn = cross(sub(b, a), sub(c, a));
            if norm(nn) < 1e-14 { continue }
            let base = pts.len() as u32;
            for p in [a, b, c] { pts.push(Point3::new(p[0], p[1], p[2])) }
            idx.push([base, base + 1, base + 2]); tri_obj.push(oi); tri_n.push(unit(nn));
            if is_floor { floor.push([a, b, c]) }
        }
    }
    let ntri = idx.len();
    let tm = TriMesh::new(pts, idx).expect("pool: scene triangle mesh");
    let w = World { tm, ntri, tri_obj, kind, okeys, tri_n };
    // room floor: area-weighted sampling, outline edges (matched by position: floor meshes have per-face vertices)
    let area: Vec<f64> = floor.iter().map(|t| norm(cross(sub(t[1], t[0]), sub(t[2], t[0]))) / 2.0).collect();
    let total: f64 = area.iter().sum();
    let q = |p: V3| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64);
    let mut ec: HashMap<((i64, i64), (i64, i64)), (usize, [f64; 2], [f64; 2])> = HashMap::new();
    for t in &floor {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (q(a), q(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            ec.entry(k).or_insert((0, [a[0], a[1]], [b[0], b[1]])).0 += 1;
        }
    }
    let segs: Vec<([f64; 2], [f64; 2])> = ec.values().filter(|e| e.0 == 1).map(|e| (e.1, e.2)).collect();
    let floor_z = if floor.is_empty() { 0.0 } else { floor.iter().map(|t| (t[0][2] + t[1][2] + t[2][2]) / 3.0).sum::<f64>() / floor.len() as f64 };
    let rm = Room { name: room.to_string(), tris: floor, area, total, segs, floor_z };
    // furniture targets for fresh views (dscap: footprint-weighted, anchors x4)
    let mut boxes: HashMap<String, (V3, V3)> = HashMap::new();
    for n in &scene.nodes {
        if n.mesh.is_empty() || kind_of(&n.name) != 0 { continue }
        let (lo, hi) = n.mesh.bounds();
        let b = boxes.entry(okey(&n.name)).or_insert((lo, hi));
        for d in 0..3 { b.0[d] = b.0[d].min(lo[d]); b.1[d] = b.1[d].max(hi[d]); }
    }
    let (mut rlo, mut rhi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in &rm.tris { for p in t { for d in 0..2 { rlo[d] = rlo[d].min(p[d]); rhi[d] = rhi[d].max(p[d]); } } }
    let anc = anchors(room);
    let mut keys: Vec<&String> = boxes.keys().collect();
    keys.sort();
    let mut targets = vec![];
    for k in keys {
        if k.starts_with("person") || k.contains("Window") || k.contains("Door") { continue }
        let (lo, hi) = boxes[k];
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        if !(rlo[0] - 0.1 <= c[0] && c[0] <= rhi[0] + 0.1 && rlo[1] - 0.1 <= c[1] && c[1] <= rhi[1] + 0.1) { continue }
        let fp = (hi[0] - lo[0]) * (hi[1] - lo[1]);
        if fp < 0.12 { continue }
        targets.push((c, fp * if anc.iter().any(|a| k.contains(a)) { 4.0 } else { 1.0 }));
    }
    // spec.aim_file: points a target video of this home sees (make_eras.py): fresh views aim at them as much as at the furniture,
    // so photos of this era can show what the video shows. Read when the pool is made: a file not written yet means none
    if let Some(af) = spec.get("aim_file").and_then(|v| v.as_str()) {
        if let Some(pts) = std::fs::read_to_string(af).ok().and_then(|t| serde_json::from_str::<Vec<V3>>(&t).ok()).filter(|p| !p.is_empty()) {
            let tw: f64 = targets.iter().map(|t| t.1).sum::<f64>().max(1.0);
            let wgt = tw * spec.get("aim_share").and_then(|v| v.as_f64()).unwrap_or(1.0) / pts.len() as f64;
            for p in &pts { targets.push((*p, wgt)) }
            eprintln!("POOL aim: {} video points, weight {:.0} %", pts.len(), 100.0 * wgt * pts.len() as f64 / (tw + wgt * pts.len() as f64));
        }
    }
    let has_anchor = !anc.is_empty() && w.okeys.iter().any(|k| anc.iter().any(|a| k.contains(a)));
    let (wd, ht, k) = (spec["width"].as_u64().unwrap_or(960) as usize, spec["height"].as_u64().unwrap_or(720) as usize, spec["n_views"].as_u64().unwrap_or(4) as usize);
    let cams = spec.get("cameras").cloned().unwrap_or(json!({}));
    let pf = cams.get("pool_factor").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let seed = spec["camera_seed"].as_u64().unwrap_or(1);
    let mut rej: HashMap<String, usize> = HashMap::new();
    let (mut pool, mut tries) = make_pool(&w, &rm, &targets, has_anchor, &cams, wd, ht, pf * k, &mut Rng::seeded(seed), true, &mut rej);
    let mut rej2: HashMap<String, usize> = HashMap::new();
    if pool.len() < k {
        let (more, t2) = make_pool(&w, &rm, &targets, has_anchor, &cams, wd, ht, pf * k - pool.len(), &mut Rng::seeded(seed + 7919), false, &mut rej2);
        pool.extend(more); tries += t2;
    }
    // cameras.snapshot_share: that share of the pool again as family snapshots of the people in the room (make_eras reserves a share of
    // each era's photos for them)
    let share = cams.get("snapshot_share").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if share > 0.0 {
        // (each person's facing from the placement pose: local +y turned by its yaw -- ig_people yaw_of; lying people face no camera)
        let mut faces: HashMap<String, f64> = HashMap::new();
        for n in &scene.nodes {
            if !n.name.starts_with("person") { continue }
            if let Some(p) = n.pose { faces.entry(okey(&n.name)).or_insert(p[1][1].atan2(p[0][1])); }
        }
        let mut people: Vec<(V3, V3, Option<f64>)> = vec![];
        for (k, (lo, hi)) in &boxes {
            if !k.starts_with("person") { continue }
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let face = if k.contains("Lie") { None } else { faces.get(k).copied() };
            if rlo[0] - 0.1 <= c[0] && c[0] <= rhi[0] + 0.1 && rlo[1] - 0.1 <= c[1] && c[1] <= rhi[1] + 0.1 { people.push((*lo, *hi, face)) }
        }
        people.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let ns = ((share * pool.len() as f64).ceil() as usize).max(if people.is_empty() { 0 } else { 2 });
        let (snaps, t3) = snapshot_views(&w, &rm, &people, wd, ht, ns, &mut Rng::seeded(seed ^ 0x5a17), &mut rej);
        eprintln!("POOL snapshots: {} of {} wanted from {} people in {} tries", snaps.len(), ns, people.len(), t3);
        pool.extend(snaps);
    }
    let views: Vec<Value> = pool.iter().map(|v| json!({"pos": v.pos, "yaw": v.yaw, "pitch": v.pitch, "f_mm": v.f_mm, "kind": v.kind, "hit": v.hit, "relaxed": v.relaxed,
        "stats": {"med": v.st.med, "p10": v.st.p10, "miss": v.st.miss, "wall": v.st.wall, "nobj": v.st.nobj, "ncls": v.st.ncls, "objfrac": v.st.objfrac, "person": v.st.person, "anchor": v.st.anchor, "near_low": v.st.near_low, "win": v.st.win}})).collect();
    eprintln!("POOL {room}: {} views from {tries} tries in {:.2?} ({} tris), rejects {:?}", views.len(), t0.elapsed(), ntri, rej);
    let mut out = json!({"camera_seed": seed, "width": wd, "height": ht, "views": views, "rejects": rej, "relaxed_rejects": rej2, "tries": tries, "room": room, "engine": "rust"});
    // spec.video: the target video's candidate walks (make_eras.py)
    if let Some(vs) = spec.get("video").filter(|v| v.is_object()) { out["video_paths"] = { let room_views: Vec<View> = pool.iter().filter(|v| v.kind != "snapshot").cloned().collect(); video_paths(&w, &room_views, vs, &mut Rng::seeded(seed ^ 0x5eed_7a7)) }; }
    out
}

// ------------------------------------------------------------------------------------------------ target video paths
// (tools/dataset/make_eras.py: the walk of a target video, chosen on the CPU from the scene's BVH while gen2 builds -- the same
// candidates and score as eras_geom.py's GPU depth check, but every frame's clearance and every pool view as a start)

type Key = (V3, f64, f64);

fn fwd_of(yaw: f64, pitch: f64) -> V3 { [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()] }
fn wrap(a: f64) -> f64 { (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI }
fn add(a: V3, b: V3) -> V3 { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn mul(a: V3, s: f64) -> V3 { [a[0] * s, a[1] * s, a[2] * s] }

/// camera moves from one pose (eras_geom.move_paths): push, pull, pan, strafe, walk-turn, orbit; `s` = +-1 picks the side
fn move_keys(v: &View, s: f64) -> Vec<(&'static str, Vec<Key>)> {
    let (p, y, pt) = (v.pos, v.yaw, v.pitch);
    let f = fwd_of(y, 0.0); let r = [y.sin(), -y.cos(), 0.0];
    let d = (v.hit[0] - p[0]).hypot(v.hit[1] - p[1]).max(0.5);
    let rad = |x: f64| x.to_radians();
    let mut out = vec![
        ("push", vec![(p, y, pt), (add(p, mul(f, (0.55 * d).min(1.6))), y, pt)]),
        ("pull", vec![(add(p, mul(f, (0.45 * d).min(1.2))), y, pt), (p, y, pt)]),
        ("pan", vec![(p, y - s * rad(35.0), pt), (p, y + s * rad(35.0), pt)]),
        ("strafe", vec![(add(p, mul(r, -0.7)), y, pt), (add(p, mul(r, 0.7)), y, pt)]),
        ("walkturn", vec![(p, y, pt), (add(p, mul(f, 0.8)), y + s * rad(25.0), pt), (add(add(p, mul(f, 1.3)), mul(r, s * 0.4)), y + s * rad(55.0), pt)]),
    ];
    let c = [v.hit[0], v.hit[1], p[2]];
    let a0 = (p[1] - c[1]).atan2(p[0] - c[0]);
    let keys: Vec<Key> = [-0.45, 0.0, 0.45].iter().map(|da| {
        let a = a0 + s * da; let q = [c[0] + d * a.cos(), c[1] + d * a.sin(), p[2]];
        (q, (c[1] - q[1]).atan2(c[0] - q[0]), pt)
    }).collect();
    out.push(("orbit", keys));
    out
}

/// Catmull-Rom through the key positions, angles eased between keys, yaw unwrapped (eras_geom.sample_keys: the builder redraws the
/// frames from the keys, so the two must agree)
fn sample_keys(keys: &[Key], frames: usize) -> Vec<Key> {
    let p: Vec<V3> = keys.iter().map(|k| k.0).collect();
    let mut ys = vec![keys[0].1];
    for k in &keys[1..] { let l = *ys.last().unwrap(); ys.push(l + wrap(k.1 - l)); }
    let a: Vec<(f64, f64)> = ys.iter().zip(keys).map(|(y, k)| (*y, k.2)).collect();
    let seg = p.len() - 1;
    let g = |j: isize| -> usize { j.clamp(0, p.len() as isize - 1) as usize };
    (0..frames).map(|i| {
        let u = i as f64 / (frames - 1).max(1) as f64 * seg as f64; let k = (u as usize).min(seg - 1); let t = u - k as f64;
        let (p0, p1, p2, p3) = (p[g(k as isize - 1)], p[g(k as isize)], p[g(k as isize + 1)], p[g(k as isize + 2)]);
        let (t2, t3) = (t * t, t * t * t);
        let pos = [0, 1, 2].map(|j| 0.5 * (2.0 * p1[j] + (-p0[j] + p2[j]) * t + (2.0 * p0[j] - 5.0 * p1[j] + 4.0 * p2[j] - p3[j]) * t2 + (-p0[j] + 3.0 * p1[j] - 3.0 * p2[j] + p3[j]) * t3));
        let te = t * t * (3.0 - 2.0 * t);
        let (a0, a1) = (a[g(k as isize)], a[g(k as isize + 1)]);
        (pos, a0.0 + (a1.0 - a0.0) * te, a0.1 + (a1.1 - a0.1) * te)
    }).collect()
}

/// pool chains (make_eras.video_paths): views that follow each other, steps 0.4-2.6 m and under 70 degrees
fn chain_keys(pool: &[View], n_views: usize) -> Vec<Vec<Key>> {
    let mut out = vec![];
    for s in 0..pool.len() {
        let mut ch = vec![&pool[s]];
        for v in &pool[s + 1..] {
            if ch.len() >= n_views { break }
            let p = ch[ch.len() - 1];
            let d = norm(sub(p.pos, v.pos)); let dy = wrap(v.yaw - p.yaw).abs();
            if d > 0.4 && d < 2.6 && dy < 70f64.to_radians() { ch.push(v) }
        }
        if ch.len() >= 2 { out.push(ch.iter().map(|v| (v.pos, v.yaw, v.pitch)).collect()) }
    }
    out
}

struct PathStats { near: f64, close: f64, cells: usize, pts: Vec<V3> }

/// clearance at every frame (18 directions) and along every step, and per 8th frame a 24 x 14 ray probe of the frustum: the distinct
/// 25 cm cells hit, the share of rays nearer than 1.2 m
fn path_stats(w: &World, frames: &[Key], f_mm: f64, wd: usize, ht: usize, want_pts: bool) -> PathStats {
    let mut near = f64::MAX;
    for (i, fr) in frames.iter().enumerate() {
        for d in EYE.iter() { if let Some(h) = w.cast(fr.0, unit(*d), 0.6) { near = near.min(h.dist) } }
        if i + 1 < frames.len() {
            let step = sub(frames[i + 1].0, fr.0); let l = norm(step);
            if l > 1e-6 { if let Some(h) = w.cast(fr.0, unit(step), l + 0.3) { near = near.min((h.dist - l).max(0.0)) } }
        }
    }
    let (nx, ny) = (24usize, 14usize);
    let fx = f_mm / 36.0 * wd as f64;
    let mut cells = std::collections::HashSet::new(); let (mut close_n, mut tot) = (0usize, 0usize); let mut pts = vec![];
    for fr in frames.iter().step_by(8) {
        let r = cam_r(fr.1, fr.2);
        for j in 0..ny { for i in 0..nx {
            let u = ((i as f64 + 0.5) / nx as f64 - 0.5) * wd as f64 / fx; let v = ((j as f64 + 0.5) / ny as f64 - 0.5) * ht as f64 / fx;
            let d = unit(add(add(r[2], mul(r[0], u)), mul(r[1], v)));
            tot += 1;
            if let Some(h) = w.cast(fr.0, d, 15.0) {
                // (z-depth, as the GPU check measured it)
                if h.dist * dot(d, r[2]) < 1.2 { close_n += 1 }
                let q = add(fr.0, mul(d, h.dist));
                cells.insert(((q[0] / 0.25).floor() as i64, (q[1] / 0.25).floor() as i64, (q[2] / 0.25).floor() as i64));
                if want_pts { pts.push(q) }
            }
        } }
    }
    PathStats { near, close: close_n as f64 / tot.max(1) as f64, cells: cells.len(), pts }
}

/// candidate walks from every pool view and the pool chains, scored like eras_geom.path_score (cells seen, less for near-wall
/// frames, zero under min_depth); returns the best few with their keys (the builder samples the frames from them) and the points
/// the best one sees (the reference photos are picked partly for them)
fn video_paths(w: &World, pool: &[View], spec: &Value, rng: &mut Rng) -> Value {
    let t0 = std::time::Instant::now();
    let frames = spec["frames"].as_u64().unwrap_or(80) as usize;
    let (wd, ht) = (spec["width"].as_u64().unwrap_or(832) as usize, spec["height"].as_u64().unwrap_or(480) as usize);
    let min_d = spec["min_depth_m"].as_f64().unwrap_or(0.3);
    let starts = spec["starts"].as_u64().unwrap_or(12) as usize;
    let mut cands: Vec<(&'static str, Vec<Key>, f64)> = vec![];
    for ch in chain_keys(pool, spec["path_views"].as_u64().unwrap_or(3) as usize).into_iter().take(4) { let f = pool.iter().find(|v| v.pos == ch[0].0).map_or(24.0, |v| v.f_mm); cands.push(("pool-chain", ch, f)) }
    for v in pool.iter().take(starts) {
        let s = if rng.f64() < 0.5 { -1.0 } else { 1.0 };
        for (fam, k) in move_keys(v, s) { cands.push((fam, k, v.f_mm)) }
    }
    let mut scored: Vec<(f64, usize, PathStats)> = cands.iter().enumerate().map(|(i, (_, k, f))| {
        let fr = sample_keys(k, frames);
        let st = path_stats(w, &fr, *f, wd, ht, false);
        let base = if st.near < min_d { 0.0 } else { st.cells as f64 * (1.0 - 0.8 * (st.close / 0.6).min(1.0)) };
        (base * rng.uniform(0.85, 1.15), i, st)
    }).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let best: Vec<Value> = scored.iter().take(3).filter(|s| s.0 > 0.0).enumerate().map(|(rank, (sc, i, st))| {
        let (fam, k, f) = &cands[*i];
        let pts = if rank == 0 { let fr = sample_keys(k, frames); let mut p = path_stats(w, &fr, *f, wd, ht, true).pts; let step = (p.len() / 4000).max(1); p = p.into_iter().step_by(step).collect(); p } else { vec![] };
        json!({"family": fam, "keys": k.iter().map(|q| json!([q.0, q.1, q.2])).collect::<Vec<_>>(), "f_mm": f, "score": sc, "near_m": st.near, "close_share": st.close, "cells": st.cells, "points": pts})
    }).collect();
    eprintln!("VIDEO paths: {} candidates in {:.2?}, best {:?}", cands.len(), t0.elapsed(), best.first().map(|b| (b["family"].clone(), b["score"].clone())));
    json!(best)
}
