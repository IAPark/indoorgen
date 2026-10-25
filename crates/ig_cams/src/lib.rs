//! Photographer-like camera poses for a generated scene (`gen2 --cams K`).
//! Candidates are sampled in the free floor space of each furnished room, aimed at a subject (people first, else the main furniture)
//! with the subject on a rule-of-thirds point, and rejected by ray casting against the scene meshes (parry3d BVH): camera in/near
//! furniture, near occluders, claustrophobic views, subject mostly hidden. Deterministic per seed.
use ig_core::mesh::{Scene, V3};
use ig_core::rng::Rng;
use parry3d_f64::na::{Point3, Vector3};
use parry3d_f64::query::{Ray, RayCast};
use parry3d_f64::shape::{FeatureId, TriMesh};
use serde_json::{json, Value};

pub struct Cfg {
    pub k: usize,
    pub seed: u64,
    pub wall_t: f64,
}

#[derive(Clone)]
struct Subj {
    key: String,
    lo: V3,
    hi: V3,
    w: f64,
    person: bool,
}

struct World {
    tm: TriMesh,
    ntri: usize,
    floor: Vec<bool>,
    door: Vec<bool>,
    owner: Vec<u32>, // per triangle: index into keys
    keys: Vec<String>,
}

impl World {
    /// first hit (distance, triangle index)
    fn cast(&self, o: V3, d: V3, max_t: f64) -> Option<(f64, usize)> {
        let r = Ray::new(Point3::new(o[0], o[1], o[2]), Vector3::new(d[0], d[1], d[2]));
        let h = self.tm.cast_local_ray_and_get_normal(&r, max_t, false)?;
        let tri = match h.feature {
            FeatureId::Face(f) => f as usize % self.ntri,
            _ => 0,
        };
        Some((h.time_of_impact, tri))
    }
}

fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn subject_weight(c: &str) -> f64 {
    if c.contains("Person") { return 6.0 }
    for (s, w) in [
        ("Sofa", 3.5), ("BedFactory", 4.0), ("TableDining", 2.5), ("Bathtub", 2.5), ("Desk", 2.0), ("KitchenIsland", 2.0),
        ("ArmChair", 1.5), ("TVStand", 0.8), ("KitchenCabinet", 1.5), ("Sink", 1.5), ("CoffeeTable", 1.2), ("SideTable", 1.0),
        ("Bookcase", 1.0), ("Shelf", 0.8), ("LargePlant", 0.7), ("Toilet", 0.4), ("Rug", 0.3), ("Dresser", 0.8), ("Wardrobe", 0.6),
    ] {
        if c.contains(s) { return w }
    }
    if ["Light", "Lamp", "Curtain", "Door", "Window", "skirting", "room", "Book", "Bottle", "Cup", "Towel", "Shirt", "Pants", "Plate", "Bowl"].iter().any(|s| c.contains(s)) { return 0.0 }
    0.12
}

fn unit(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn key_of(name: &str) -> &str {
    name.split('#').next().unwrap()
}

/// Camera poses for every furnished room (K each). Sidecar `cameras` entries.
pub fn generate(scene: &Scene, room_polys: &[(String, ig_core::shp::G)], cfg: &Cfg) -> Vec<Value> {
    let bbs: Vec<(V3, V3)> = scene.nodes.iter().map(|n| n.mesh.bounds()).collect();
    let mut out = vec![];
    for (rname, g) in room_polys {
        let inset = ig_core::shp::buffer_mitre(g, -cfg.wall_t / 2.0);
        let poly = ig_core::shp::exterior(&inset);
        let ext = ig_core::shp::exterior(g);
        if poly.len() < 3 || ext.len() < 3 { continue }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &poly { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
        // objects of this room: grouped by key (name before '#'), centre inside the room
        let mut subj: Vec<Subj> = vec![];
        for (i, n) in scene.nodes.iter().enumerate() {
            if n.class == "room" || n.class == "skirting" { continue }
            let key = key_of(&n.name).to_string();
            let (lo, hi) = bbs[i];
            if let Some(s) = subj.iter_mut().find(|s| s.key == key) {
                for d in 0..3 { s.lo[d] = s.lo[d].min(lo[d]); s.hi[d] = s.hi[d].max(hi[d]); }
                continue;
            }
            subj.push(Subj { key, lo, hi, w: subject_weight(&n.class), person: n.class == "Person" });
        }
        subj.retain(|s| in_poly([(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0], &ext));
        if subj.len() < 3 { continue } // shell only (room not furnished in this run)
        let cand: Vec<Subj> = subj.iter().filter(|s| s.w > 0.0 && (s.hi[0] - s.lo[0]).max(s.hi[1] - s.lo[1]).max(s.hi[2] - s.lo[2]) >= 0.5).cloned().collect();
        if cand.is_empty() { continue }
        // keep the strong subjects (people always): weak ones (appliances, small shelves) make dull photos
        let maxw = cand.iter().filter(|s| !s.person).map(|s| s.w).fold(0.0, f64::max);
        let cand: Vec<Subj> = cand.into_iter().filter(|s| s.person || s.w >= 0.4 * maxw).collect();
        // collision world: nodes overlapping the room box (tiny clutter skipped)
        let (mut fz, mut cz) = (f64::NAN, f64::NAN);
        let mut verts: Vec<Point3<f64>> = vec![];
        let mut idx: Vec<[u32; 3]> = vec![];
        let (mut floor, mut owner, mut keys): (Vec<bool>, Vec<u32>, Vec<String>) = (vec![], vec![], vec![]);
        let mut door: Vec<bool> = vec![];
        let (fname, cname) = (format!("{rname}.floor"), format!("{rname}.ceiling"));
        for (i, n) in scene.nodes.iter().enumerate() {
            let (lo, hi) = bbs[i];
            if n.name == fname { fz = hi[2] }
            if n.name == cname { cz = lo[2] }
            if hi[0] < x0 - 0.3 || lo[0] > x1 + 0.3 || hi[1] < y0 - 0.3 || lo[1] > y1 + 0.3 { continue }
            let is_room = n.class == "room";
            if !is_room && (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) < 0.12 { continue }
            let is_floor = n.name == fname || n.class.contains("Rug");
            let key = key_of(&n.name);
            let ki = match keys.iter().position(|k| k == key) { Some(p) => p, None => { keys.push(key.to_string()); keys.len() - 1 } };
            let base = verts.len() as u32;
            if !is_room && n.mesh.faces.len() > 3000 {
                // very detailed assets (plants, ...): their bounding box is a good enough occluder and keeps the BVH small
                for k in 0..8u32 { verts.push(Point3::new(if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] })); }
                for q in [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]] {
                    for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                        idx.push([base + t[0], base + t[1], base + t[2]]);
                        floor.push(is_floor);
                        door.push(n.class.contains("Door"));
                        owner.push(ki as u32);
                    }
                }
                continue;
            }
            for p in &n.mesh.pos { verts.push(Point3::new(p[0], p[1], p[2])); }
            for f in &n.mesh.faces {
                for j in 1..f.len().saturating_sub(1) {
                    idx.push([base + f[0], base + f[j], base + f[j + 1]]);
                    floor.push(is_floor);
                    door.push(n.class.contains("Door"));
                    owner.push(ki as u32);
                }
            }
        }
        if fz.is_nan() || idx.is_empty() { continue }
        if cz.is_nan() { cz = fz + 2.6 }
        let ntri = idx.len();
        let Ok(tm) = TriMesh::new(verts, idx) else { continue };
        let w = World { tm, ntri, floor, door, owner, keys };
        let mut rng = Rng::seeded(ig_core::int_hash(&[&cfg.seed.to_string(), rname, "cams"]));
        let mut got: Vec<(V3, V3, String)> = vec![];
        let mut used = vec![0.0f64; cand.len()];
        let mut tries = 0;
        let has_people = cand.iter().any(|s| s.person);
        // per camera: collect up to NV valid candidates, keep the best-scoring one (open view, subject visible, important subject)
        let mut best: Option<(f64, usize, V3, V3, f64, String)> = None;
        let mut nvalid = 0;
        let mut cam_tries = 0;
        while got.len() < cfg.k && tries < 6000 {
            if nvalid >= 10 || cam_tries >= 900 {
                cam_tries = 0;
                nvalid = 0;
                match best.take() {
                    Some((_, si, o, f, lens, _)) => {
                        let (r, u) = ([f[1], -f[0], 0.0], {
                            let h = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9);
                            [-f[0] / h * f[2], -f[1] / h * f[2], h]
                        });
                        let rn = (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-9);
                        let r = [r[0] / rn, r[1] / rn, 0.0];
                        used[si] += 1.0;
                        got.push((o, f, cand[si].key.clone()));
                        let m = [[r[0], u[0], -f[0], o[0]], [r[1], u[1], -f[1], o[1]], [r[2], u[2], -f[2], o[2]], [0.0, 0.0, 0.0, 1.0]];
                        out.push(json!({"name": format!("cam{}", out.len()), "matrix": m, "lens": lens, "sensor_width": 36, "subject": cand[si].key, "room": rname}));
                    }
                    None => { if tries > 3000 { break } }
                }
                continue;
            }
            tries += 1;
            cam_tries += 1;
            // subject: people first, weights decay with use
            let ws: Vec<f64> = cand.iter().enumerate().map(|(i, s)| if has_people && got.is_empty() && !s.person { 0.0 } else { s.w / (1.0 + 6.0 * used[i]) }).collect();
            let si = rng.choice_p(&ws);
            let s = &cand[si];
            let sc = [(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0];
            let hgt = s.hi[2] - s.lo[2];
            let cz_s = if s.person { s.lo[2] + 0.8 * hgt } else { s.lo[2] + (0.5 * hgt).min(0.9).max(0.3 * hgt) };
            let c = [sc[0], sc[1], cz_s];
            let pos = if rng.f64() < 0.75 {
                let a = rng.uniform(0.0, std::f64::consts::TAU);
                let r = rng.uniform(1.0, 5.5);
                [sc[0] + a.cos() * r, sc[1] + a.sin() * r]
            } else {
                [rng.uniform(x0, x1), rng.uniform(y0, y1)]
            };
            if !in_poly(pos, &poly) { continue }
            let eye = fz + rng.uniform(1.0, 1.75).min(cz - fz - 0.3);
            let o = [pos[0], pos[1], eye];
            // clearance: 8 horizontal rays at 3 heights, floor below must be floor/rug, nothing right above
            let mut bad = false;
            'cl: for hh in [fz + 0.3, fz + 0.8, eye] {
                for k in 0..8 {
                    let a = k as f64 * std::f64::consts::FRAC_PI_4;
                    if w.cast([pos[0], pos[1], hh], [a.cos(), a.sin(), 0.0], 0.3).is_some() { bad = true; break 'cl }
                }
            }
            if bad { continue }
            match w.cast(o, [0.0, 0.0, -1.0], eye - fz + 0.5) {
                Some((_, ti)) if w.floor[ti] => {}
                _ => continue,
            }
            if w.cast(o, [0.0, 0.0, 1.0], 0.3).is_some() { continue }
            let dx = (s.lo[0] - pos[0]).max(0.0).max(pos[0] - s.hi[0]);
            let dy = (s.lo[1] - pos[1]).max(0.0).max(pos[1] - s.hi[1]);
            if dx.hypot(dy) < if cam_tries < 400 { 1.1 } else { 0.6 } { continue }
            let bearing = (c[1] - pos[1]).atan2(c[0] - pos[0]);
            let corners = [[s.lo[0], s.lo[1]], [s.hi[0], s.lo[1]], [s.hi[0], s.hi[1]], [s.lo[0], s.hi[1]]];
            let (mut amin, mut amax) = (f64::MAX, f64::MIN);
            for cc in corners {
                let mut d = (cc[1] - pos[1]).atan2(cc[0] - pos[0]) - bearing;
                while d > std::f64::consts::PI { d -= std::f64::consts::TAU }
                while d < -std::f64::consts::PI { d += std::f64::consts::TAU }
                amin = amin.min(d);
                amax = amax.max(d);
            }
            let span = (amax - amin).max(0.05);
            let frac = rng.uniform(0.3, 0.7);
            let lens = (18.0 / ((span / 2.0).tan() / frac)).clamp(16.0, 35.0);
            let lens = (lens * 2.0).round() / 2.0;
            let tan_h = 18.0 / lens;
            let tan_v = tan_h * 9.0 / 16.0;
            let real_frac = (span / 2.0).tan() / tan_h;
            if !(0.2..=1.0).contains(&real_frac) { continue }
            // rule of thirds
            let tx = rng.uniform(0.2, 0.45) * if rng.bool(0.5) { 1.0 } else { -1.0 };
            let ty = rng.uniform(-0.25, 0.2);
            let yaw = bearing + (tx * tan_h).atan();
            let dh = (c[0] - pos[0]).hypot(c[1] - pos[1]);
            let elev = (c[2] - eye).atan2(dh);
            let pitch = (elev - (ty * tan_v).atan()).clamp(-20f64.to_radians(), 5f64.to_radians());
            let (sy, cy, sp, cp) = (yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos());
            let f = [cy * cp, sy * cp, sp];
            let r = [sy, -cy, 0.0];
            let u = [-cy * sp, -sy * sp, cp];
            let proj = |p: V3| -> Option<(f64, f64)> {
                let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
                let z = d[0] * f[0] + d[1] * f[1] + d[2] * f[2];
                if z < 0.1 { return None }
                Some(((d[0] * r[0] + d[1] * r[1]) / z / tan_h, (d[0] * u[0] + d[1] * u[1] + d[2] * u[2]) / z / tan_v))
            };
            match proj(c) {
                Some((px, py)) if px.abs() < 0.75 && py.abs() < 0.75 => {}
                _ => continue,
            }
            // coarse ray grid (stricter near-occluder test first, the spec's 0.6 m / 15% after 350 failed tries)
            let (near_d, near_f) = if cam_tries < 400 { (0.9, 0.08) } else { (0.6, 0.15) };
            let (gw, gh) = (16, 9);
            let (mut near, mut close, mut tot, mut dsum, mut mid, mut wall, mut doorc) = (0, 0, 0, 0.0f64, 0, 0, 0);
            for iy in 0..gh {
                for ix in 0..gw {
                    let nx = ((ix as f64 + 0.5) / gw as f64) * 2.0 - 1.0;
                    let ny = 1.0 - ((iy as f64 + 0.5) / gh as f64) * 2.0;
                    let d = unit([f[0] + r[0] * nx * tan_h + u[0] * ny * tan_v, f[1] + r[1] * nx * tan_h + u[1] * ny * tan_v, f[2] + u[2] * ny * tan_v]);
                    tot += 1;
                    if let Some((t, ti)) = w.cast(o, d, 6.0) {
                        dsum += t;
                        if t < 1.8 { mid += 1 }
                        if t < 3.0 && w.door[ti] { doorc += 1 }
                        if t < 3.0 && w.keys[w.owner[ti] as usize].contains(".wall") { wall += 1 }
                        if t < 1.2 { close += 1 }
                        if t < near_d { near += 1 }
                    } else {
                        dsum += 6.0;
                    }
                }
            }
            if near as f64 > near_f * tot as f64 || close as f64 > 0.6 * tot as f64 { continue }
            let strict = cam_tries < 400;
            if strict && (doorc as f64 > 0.1 * tot as f64 || mid as f64 > 0.4 * tot as f64 || wall as f64 > 0.4 * tot as f64) { continue }
            // the subject box should be mostly inside the frame
            let inf = (0..8).filter(|k| {
                let q = [if k & 1 == 0 { s.lo[0] } else { s.hi[0] }, if k & 2 == 0 { s.lo[1] } else { s.hi[1] }, if k & 4 == 0 { s.lo[2] } else { s.hi[2] }];
                matches!(proj(q), Some((px, py)) if px.abs() < 1.02 && py.abs() < 1.02)
            }).count();
            if strict && inf < 5 { continue }
            // subject visibility: 3x3x3 sample points
            let (mut vis, mut n) = (0, 0);
            for ix in 0..3 {
                for iy in 0..3 {
                    for iz in 0..3 {
                        let q = [s.lo[0] + (s.hi[0] - s.lo[0]) * (0.1 + 0.4 * ix as f64), s.lo[1] + (s.hi[1] - s.lo[1]) * (0.1 + 0.4 * iy as f64), s.lo[2] + (s.hi[2] - s.lo[2]) * (0.1 + 0.4 * iz as f64)];
                        n += 1;
                        let Some((px, py)) = proj(q) else { continue };
                        if px.abs() > 1.0 || py.abs() > 1.0 { continue }
                        let d = [q[0] - o[0], q[1] - o[1], q[2] - o[2]];
                        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                        let ok = match w.cast(o, unit(d), dist - 0.02) {
                            None => true,
                            Some((_, ti)) => w.keys[w.owner[ti] as usize] == s.key,
                        };
                        if ok { vis += 1 }
                    }
                }
            }
            if (vis as f64) < 0.5 * n as f64 { continue }
            // spread
            if got.iter().any(|(p, ff, sk)| {
                let dd = ((p[0] - o[0]).powi(2) + (p[1] - o[1]).powi(2)).sqrt();
                let ang = (ff[0] * f[0] + ff[1] * f[1]).clamp(-1.0, 1.0).acos().to_degrees();
                dd < 1.2 || (*sk == s.key && ang < 40.0) || ang < 20.0
            }) { continue }
            let score = dsum / tot as f64 / 6.0 + 0.3 * inf as f64 / 8.0 - 0.5 * wall as f64 / tot as f64 - 1.5 * doorc as f64 / tot as f64 - 0.5 * mid as f64 / tot as f64 - if strict { 0.0 } else { 1.0 } + 0.8 * vis as f64 / n as f64 + 0.4 * s.w.min(6.0) / 6.0 + 0.2 * rng.f64();
            nvalid += 1;
            if best.as_ref().map_or(true, |b| score > b.0) { best = Some((score, si, o, f, lens, s.key.clone())) }
        }
    }
    out
}
