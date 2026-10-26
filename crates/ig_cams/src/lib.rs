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
    /// target room type (e.g. "Bedroom", "LivingRoom"): its rooms get `k` cameras, every other furnished room (k + 1) / 2
    pub target: Option<String>,
}

#[derive(Clone)]
struct Subj {
    key: String,
    lo: V3,
    hi: V3,
    w: f64,
    person: bool,
    group: bool,
    /// people: world facing direction (xy), seated/lying flags
    face: Option<[f64; 2]>,
    low: bool,
    lie: bool,
    /// people: sampled mesh vertices (framing/crop tests use the real body, not the axis-aligned box)
    pts: Vec<V3>,
}

fn sample_pts(pos: &[V3]) -> Vec<V3> {
    let st = (pos.len() / 40).max(1);
    pos.iter().step_by(st).copied().collect()
}

struct World {
    tm: TriMesh,
    ntri: usize,
    floor: Vec<bool>,
    door: Vec<bool>,
    shell: Vec<bool>, // walls/floor/ceiling/windows/doors/skirting: not 'furniture'
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

/// per-room-type multiplier of a subject class weight (bed in bedrooms, sofa group in living rooms, counter run / island in kitchens, ...)
fn prior(rt: &str, c: &str) -> f64 {
    let any = |l: &[&str]| l.iter().any(|s| c.contains(s));
    match rt {
        "bedroom" => if any(&["Bed"]) { 2.0 } else if any(&["Desk", "Dresser", "Wardrobe", "Plant", "Chair"]) { 1.0 } else { 0.6 },
        "living-room" => if any(&["Sofa", "ArmChair", "CoffeeTable", "TVStand"]) { 1.8 } else if any(&["Bookcase", "Shelf"]) { 0.6 } else { 1.0 },
        "dining-room" => if any(&["TableDining"]) { 3.0 } else if any(&["Chair"]) { 0.3 } else { 0.5 },
        "kitchen" => if any(&["Cabinet", "Island", "Sink"]) { 3.0 } else if any(&["TableDining"]) { 1.5 } else if any(&["Oven", "Fridge", "Dishwasher", "Microwave"]) { 0.4 } else { 0.7 },
        "bathroom" => if any(&["Bathtub", "Toilet", "Sink", "Shower"]) { 2.0 } else if any(&["Shelf", "Cabinet", "Bookcase"]) { 0.4 } else { 0.8 },
        _ => 1.0,
    }
}

fn subject_weight(c: &str) -> f64 {
    if c.contains("Person") { return 6.0 }
    for (s, w) in [
        ("Sofa", 3.5), ("BedFactory", 4.0), ("TableDining", 2.5), ("Bathtub", 2.5), ("Desk", 2.0), ("KitchenIsland", 2.0),
        ("ArmChair", 1.5), ("TVStand", 0.8), ("KitchenCabinet", 1.5), ("Sink", 1.5), ("CoffeeTable", 1.2), ("SideTable", 1.0),
        ("SingleCabinet", 1.2), ("Oven", 0.6), ("Bookcase", 1.0), ("Shelf", 0.8), ("LargePlant", 0.7), ("Toilet", 0.4), ("Rug", 0.3), ("Dresser", 0.8), ("Wardrobe", 0.6),
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

/// nearest of a few random free spots to a wanted position (falls back to the wanted position)
fn near_valid(valid: &[[f64; 2]], want: [f64; 2], rng: &mut Rng) -> [f64; 2] {
    if valid.is_empty() { return want }
    let mut best = valid[rng.choice_idx(valid.len())];
    for _ in 0..11 {
        let q = valid[rng.choice_idx(valid.len())];
        if (q[0] - want[0]).hypot(q[1] - want[1]) < (best[0] - want[0]).hypot(best[1] - want[1]) { best = q }
    }
    [best[0] + rng.uniform(-0.1, 0.1), best[1] + rng.uniform(-0.1, 0.1)]
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
                if s.person { s.pts.extend(sample_pts(&n.mesh.pos)) }
                continue;
            }
            let person = n.class == "Person";
            let face = n.pose.filter(|_| person).map(|p| { let v = [-p[0][1], p[1][1]]; let l = v[0].hypot(v[1]).max(1e-9); [v[0] / l, v[1] / l] });
            let lie = person && key.contains("_Lie");
            let low = lie || (person && key.contains("_Sit"));
            subj.push(Subj { key, lo, hi, w: subject_weight(&n.class), person, group: false, face, low, lie, pts: if person { sample_pts(&n.mesh.pos) } else { vec![] } });
        }
        subj.retain(|s| in_poly([(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0], &ext));
        if subj.len() < 2 { continue } // shell only (room not furnished in this run)
        let mut cand: Vec<Subj> = subj.iter().filter(|s| s.w > 0.0 && (s.hi[0] - s.lo[0]).max(s.hi[1] - s.lo[1]).max(s.hi[2] - s.lo[2]) >= 0.5).cloned().collect();
        if cand.is_empty() {
            // nothing big: any non-trivial object will do
            cand = subj.iter().filter(|s| s.w > 0.0 && (s.hi[0] - s.lo[0]).max(s.hi[1] - s.lo[1]).max(s.hi[2] - s.lo[2]) >= 0.25).cloned().map(|mut s| { s.w = s.w.max(0.3); s }).collect();
        }
        if cand.is_empty() { continue }
        // keep the strong subjects (people always): weak ones (appliances, small shelves) make dull photos
        let _maxw = cand.iter().filter(|s| !s.person).map(|s| s.w).fold(0.0, f64::max);
        let rtype = rname.split('_').next().unwrap_or("");
        let cand: Vec<Subj> = cand.into_iter().map(|mut s| { if !s.person { s.w *= prior(rtype, &s.key) } s }).collect();
        let maxw = cand.iter().filter(|s| !s.person).map(|s| s.w).fold(0.0, f64::max);
        let all = cand.clone();
        let mut cand: Vec<Subj> = cand.into_iter().filter(|s| s.person || s.w >= 0.4 * maxw).collect();
        // 'group' subject (establishing shots): every candidate together; for the per-slot weights it only counts in sparse rooms
        let sparse = maxw < 4.0 && all.iter().filter(|s| !s.person).count() >= 2;
        let gi = cand.len();
        {
            let mut g = Subj { key: "group".into(), lo: [f64::MAX; 3], hi: [f64::MIN; 3], w: 0.9 * maxw.max(1.0), person: false, group: true, face: None, low: false, lie: false, pts: vec![] };
            for s in all.iter() { for d in 0..3 { g.lo[d] = g.lo[d].min(s.lo[d]); g.hi[d] = g.hi[d].max(s.hi[d]); } }
            cand.push(g);
        }
        // collision world: nodes overlapping the room box (tiny clutter skipped)
        let (mut fz, mut cz) = (f64::NAN, f64::NAN);
        let mut verts: Vec<Point3<f64>> = vec![];
        let mut idx: Vec<[u32; 3]> = vec![];
        let (mut floor, mut owner, mut keys): (Vec<bool>, Vec<u32>, Vec<String>) = (vec![], vec![], vec![]);
        let mut door: Vec<bool> = vec![];
        let mut shell: Vec<bool> = vec![];
        let (fname, cname) = (format!("{rname}.floor"), format!("{rname}.ceiling"));
        for (i, n) in scene.nodes.iter().enumerate() {
            let (lo, hi) = bbs[i];
            if n.name == fname { fz = hi[2] }
            if n.name == cname { cz = lo[2] }
            if hi[0] < x0 - 0.3 || lo[0] > x1 + 0.3 || hi[1] < y0 - 0.3 || lo[1] > y1 + 0.3 { continue }
            let is_room = n.class == "room";
            if !is_room && (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) < 0.12 { continue }
            let is_shell = is_room || n.class.contains("Door") || n.class.contains("Window") || n.class == "skirting";
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
                        shell.push(is_shell);
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
                    shell.push(is_shell);
                    owner.push(ki as u32);
                }
            }
        }
        if fz.is_nan() || idx.is_empty() { continue }
        if cz.is_nan() { cz = fz + 2.6 }
        let ntri = idx.len();
        let Ok(tm) = TriMesh::new(verts, idx) else { continue };
        let w = World { tm, ntri, floor, door, shell, owner, keys };        let mut rng = Rng::seeded(ig_core::int_hash(&[&cfg.seed.to_string(), rname, "cams"]));
        // seating/beds/TV stands have a back against a wall: never shoot them from behind
        let backs: Vec<Option<[f64; 2]>> = cand.iter().map(|s| {
            if s.group || s.person || !["Sofa", "ArmChair", "BedFactory", "TVStand"].iter().any(|k| s.key.contains(k)) { return None }
            let z = s.lo[2] + 0.5 * (s.hi[2] - s.lo[2]);
            let (cx, cy) = ((s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0);
            // the high part (backrest/headboard) is off-centre: its mean offset along the dominant axis gives the back direction
            let zt = s.lo[2] + 0.6 * (s.hi[2] - s.lo[2]);
            let (mut mx, mut my, mut cnt) = (0.0, 0.0, 0usize);
            for n in scene.nodes.iter().filter(|n| key_of(&n.name) == s.key) {
                for p in n.mesh.pos.iter().filter(|p| p[2] > zt) { mx += p[0] - cx; my += p[1] - cy; cnt += 1 }
            }
            if cnt > 20 {
                let (mx, my) = (mx / cnt as f64, my / cnt as f64);
                let (ex, ey) = (s.hi[0] - s.lo[0], s.hi[1] - s.lo[1]);
                if (mx.abs() / ex).max(my.abs() / ey) > 0.06 {
                    return Some(if mx.abs() / ex > my.abs() / ey { [mx.signum(), 0.0] } else { [0.0, my.signum()] });
                }
            }
            let mut best: Option<(f64, [f64; 2])> = None;
            for (d, o) in [([1.0, 0.0], s.hi[0] + 0.02), ([-1.0, 0.0], s.lo[0] - 0.02), ([0.0, 1.0], s.hi[1] + 0.02), ([0.0, -1.0], s.lo[1] - 0.02)] {
                let org = if d[0] != 0.0 { [o, cy, z] } else { [cx, o, z] };
                if let Some((t, _)) = w.cast(org, [d[0], d[1], 0.0], 0.6) { if best.map_or(true, |b| t < b.0) { best = Some((t, d)) } }
            }
            best.map(|b| b.1)
        }).collect();
        // room centroid / area (polygon)
        let (mut ar, mut ccx, mut ccy) = (0.0, 0.0, 0.0);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let cr = a[0] * b[1] - b[0] * a[1];
            ar += cr; ccx += (a[0] + b[0]) * cr; ccy += (a[1] + b[1]) * cr;
        }
        let area = ar.abs() / 2.0;
        let cen = if ar.abs() > 1e-9 && in_poly([ccx / (3.0 * ar), ccy / (3.0 * ar)], &poly) { [ccx / (3.0 * ar), ccy / (3.0 * ar)] } else { [(x0 + x1) / 2.0, (y0 + y1) / 2.0] };
        // vantage pool for establishing shots: room corners pulled inwards, and doorways (just inside / just outside the room)
        let mut pool: Vec<([f64; 2], bool)> = vec![];
        for p in &poly {
            let (dx, dy) = (cen[0] - p[0], cen[1] - p[1]);
            let l = dx.hypot(dy).max(1e-9);
            for off in [0.55, 1.0] { pool.push(([p[0] + dx / l * off, p[1] + dy / l * off], false)); }
        }
        let mut doors: Vec<[f64; 2]> = vec![];
        for (i, n) in scene.nodes.iter().enumerate() {
            if !n.class.contains("Door") { continue }
            let (lo, hi) = bbs[i];
            let (ex, ey) = (hi[0] - lo[0], hi[1] - lo[1]);
            if ex.max(ey) < 0.6 || ex.max(ey) > 2.2 || hi[2] - lo[2] < 1.6 { continue }
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            // near the room outline?
            let mut dmin = f64::MAX;
            for k in 0..ext.len() {
                let (a, b) = (ext[k], ext[(k + 1) % ext.len()]);
                let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
                let t = (((c[0] - a[0]) * abx + (c[1] - a[1]) * aby) / (abx * abx + aby * aby).max(1e-12)).clamp(0.0, 1.0);
                dmin = dmin.min((c[0] - a[0] - t * abx).hypot(c[1] - a[1] - t * aby));
            }
            if dmin > 0.5 || doors.iter().any(|d| (d[0] - c[0]).hypot(d[1] - c[1]) < 0.8) { continue }
            doors.push(c);
            let nrm = if ex < ey { [1.0, 0.0] } else { [0.0, 1.0] };
            for (sg, off) in [(1.0, 0.3), (-1.0, 0.3), (1.0, 0.8), (-1.0, 0.8)] {
                let pp = [c[0] + sg * nrm[0] * off, c[1] + sg * nrm[1] * off];
                pool.push((pp, !in_poly(pp, &poly)));
            }
        }
        let small = area < 9.0;
        // free standing spots on a grid (nothing within 25 cm at knee/chest/eye height, floor below, nothing right above)
        let mut valid: Vec<[f64; 2]> = vec![];
        {
            let step = 0.25;
            let (mut gx, mut gy);
            gx = x0 + step / 2.0;
            while gx < x1 {
                gy = y0 + step / 2.0;
                while gy < y1 {
                    let pp = [gx, gy];
                    if in_poly(pp, &poly) {
                        let mut ok = true;
                        'g: for (hh, cl) in [(fz + 0.5, 0.15), (fz + 0.95, 0.25), (fz + 1.45, 0.25)] {
                            for k in 0..8 {
                                let a = k as f64 * std::f64::consts::FRAC_PI_4;
                                if w.cast([pp[0], pp[1], hh], [a.cos(), a.sin(), 0.0], cl).is_some() { ok = false; break 'g }
                            }
                        }
                        if ok { ok = matches!(w.cast([pp[0], pp[1], fz + 1.45], [0.0, 0.0, -1.0], 1.95), Some((_, ti)) if w.floor[ti]) && w.cast([pp[0], pp[1], fz + 1.45], [0.0, 0.0, 1.0], 0.3).is_none() }
                        if ok { valid.push(pp) }
                    }
                    gy += step;
                }
                gx += step;
            }
        }
        // people nobody can see from the front (facing a wall in a tight room): shot only small / incidentally
        let faceless: Vec<bool> = cand.iter().map(|s| {
            let Some(fc) = s.face else { return false };
            if !s.person || s.lie { return false }
            let sc = [(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0];
            let head = [sc[0], sc[1], s.lo[2] + 0.85 * (s.hi[2] - s.lo[2])];
            valid.iter().filter(|p| {
                let (vx, vy) = (p[0] - sc[0], p[1] - sc[1]);
                let d = vx.hypot(vy);
                if !(d > 1.3 && d < 4.5 && (vx * fc[0] + vy * fc[1]) / d > 0.3) { return false }
                let o = [p[0], p[1], fz + 1.4];
                let dv = [head[0] - o[0], head[1] - o[1], head[2] - o[2]];
                let dl = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
                match w.cast(o, unit(dv), dl - 0.05) { None => true, Some((_, ti)) => w.keys[w.owner[ti] as usize] == s.key }
            }).take(3).count() < 3
        }).collect();
        let mut got: Vec<(V3, V3, String)> = vec![];
        let mut used = vec![0.0f64; cand.len()];
        let mut rej = [0u32; 24];
        let has_people = cand.iter().any(|s| s.person);
        let mut lvl_hist = [0usize; 7];
        let tau = std::f64::consts::TAU;
        let wrap = |mut d: f64| { while d > std::f64::consts::PI { d -= tau } while d < -std::f64::consts::PI { d += tau } d };
        let norm = |x: &str| x.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
        let k_room = match &cfg.target { Some(t) if norm(t) != norm(rtype) => (cfg.k + 1) / 2, _ => cfg.k };
        for slot in 0..k_room {
            // 0 = establishing wide, 1 = medium (people first), 2 = detail
            let kind = if slot == 0 { 0 } else if slot % 2 == 1 { 1 } else { 2 };
            let mut chosen: Option<(f64, usize, V3, V3, f64)> = None;
            'lv: for lv in 0..6usize {
                // thresholds per relaxation level
                let (face_min, near_d, near_f, wall_max, key_max, door_max, blk_max, vis_min, inf_min, dist_min, dist_min_p, sp_d, sp_a, mid_max, furn_min, clr) = match lv {
                    0 => (0.45, 0.9, 0.08, 0.40, 0.32, 0.03, 0.08, 0.5, 8, 1.2, 1.3, 1.2, 25.0, 0.4, 0.12, 0.3),
                    1 => (0.0, 0.7, 0.12, 0.50, 0.40, 0.05, 0.12, 0.45, 8, 1.0, 1.15, 1.0, 22.0, 0.5, 0.08, 0.28),
                    2 => (-0.3, 0.6, 0.15, 0.50, 0.50, 0.08, 0.15, 0.35, 7, 0.8, 1.0, 0.9, 20.0, 0.6, 0.04, 0.2),
                    3 => (-0.5, 0.45, 0.25, 0.50, 0.45, 0.12, 0.20, 0.25, 7, 0.6, 0.8, 0.8, 18.0, 0.7, 0.02, 0.16),
                    4 => (-1.1, 0.4, 0.30, 0.50, 0.50, 0.15, 0.25, 0.25, 7, 0.6, 0.8, 0.8, 16.0, 0.8, 0.0, 0.16),
                    _ => (-1.1, 0.4, 0.30, 0.60, 0.60, 0.20, 0.40, 0.2, 0, 0.5, 0.7, 0.7, 14.0, 0.8, 0.0, 0.14),
                };
                let (budget, want) = ([260, 170, 170, 130, 130, 130][lv], if lv == 0 { 10 } else { 6 });
                let mut best: Option<(f64, usize, V3, V3, f64)> = None;
                let mut nvalid = 0;
                for _ in 0..budget {
                    if nvalid >= want { break }
                    // subject
                    let ws: Vec<f64> = cand.iter().enumerate().map(|(i, s)| {
                        if s.group {
                            return if kind == 0 || lv >= 3 || (sparse && kind > 0) { if kind == 0 { 1.0 } else { 0.5 * s.w } } else { 0.0 };
                        }
                        if kind == 0 && lv < 3 { return 0.0 }
                        if lv >= 4 && s.person && cand.iter().any(|q| !q.person && !q.group) { return 0.0 }
                        let mut w0 = s.w;
                        if kind == 2 && s.person { w0 *= 0.3 }
                        if kind == 1 && s.person { w0 *= 1.5 }
                        if faceless[i] { w0 *= 0.12 }
                        w0 / (1.0 + 6.0 * used[i])
                    }).collect();
                    if ws.iter().sum::<f64>() <= 0.0 { rej[13] += 1; break }
                    let si = rng.choice_p(&ws);
                    let s = &cand[si];
                    let sc = [(s.lo[0] + s.hi[0]) / 2.0, (s.lo[1] + s.hi[1]) / 2.0];
                    let hgt = s.hi[2] - s.lo[2];
                    let cz_s = if s.group { fz + 0.9 } else if s.person { s.lo[2] + 0.55 * hgt } else { s.lo[2] + (0.5 * hgt).min(0.9).max(0.3 * hgt) };
                    let c = [if s.group { cen[0] * 0.5 + sc[0] * 0.5 } else { sc[0] }, if s.group { cen[1] * 0.5 + sc[1] * 0.5 } else { sc[1] }, cz_s];
                    // candidate camera position
                    let pu = rng.f64();
                    let mut outside_ok = false;
                    let pos = if s.group || kind == 0 {
                        if pu < 0.6 && !pool.is_empty() {
                            let (p, o) = pool[rng.choice_idx(pool.len())];
                            outside_ok = o;
                            p
                        } else { near_valid(&valid, [rng.uniform(x0, x1), rng.uniform(y0, y1)], &mut rng) }
                    } else if pu < 0.75 {
                        // in front of people / the open side of seating; else anywhere around
                        let base = if let (Some(fc), true) = (s.face, s.person && !s.lie) { Some(fc[1].atan2(fc[0])) } else { backs[si].map(|b| (-b[1]).atan2(-b[0])) };
                        let a = match base { Some(b) if rng.bool(0.8) => b + rng.uniform(-1.1, 1.1), _ => rng.uniform(0.0, tau) };
                        let rmax = (0.7 * area.sqrt() + 0.8).clamp(2.0, 4.2);
                        let r = if s.person { rng.uniform(1.2, rmax) } else if pu < 0.35 { rng.uniform(0.8, 3.0) } else { rng.uniform(1.2, 5.5) };
                        let want = [sc[0] + a.cos() * r, sc[1] + a.sin() * r];
                        near_valid(&valid, want, &mut rng)
                    } else if pu < 0.85 && !pool.is_empty() {
                        let (p, o) = pool[rng.choice_idx(pool.len())];
                        outside_ok = o;
                        p
                    } else {
                        near_valid(&valid, [rng.uniform(x0, x1), rng.uniform(y0, y1)], &mut rng)
                    };
                    if !outside_ok && !in_poly(pos, &poly) { rej[0] += 1; continue }
                    // eye height: 1.2-1.6 m standing, lower for seated subjects and low objects
                    let eye = if kind == 0 { fz + rng.uniform(1.3, 1.6) } else if s.low || (!s.person && hgt < 0.8) { fz + rng.uniform(1.0, 1.35) } else { fz + rng.uniform(1.2, 1.6) };
                    let eye = eye.min(cz - 0.35);
                    let o = [pos[0], pos[1], eye];
                    // clearance: 8 horizontal rays at 3 heights; floor below
                    let mut bad = false;
                    'cl: for (hh, cl) in [(fz + 0.5, clr * 0.5), (fz + 0.95, clr), (eye, clr)] {
                        for k in 0..8 {
                            let a = k as f64 * std::f64::consts::FRAC_PI_4;
                            match w.cast([pos[0], pos[1], hh], [a.cos(), a.sin(), 0.0], cl) {
                                Some((_, ti)) if outside_ok && w.shell[ti] => {}
                                Some(_) => { bad = true; break 'cl }
                                None => {}
                            }
                        }
                    }
                    if bad { rej[1] += 1; continue }
                    match w.cast(o, [0.0, 0.0, -1.0], eye - fz + 0.5) {
                        Some((_, ti)) if w.floor[ti] || outside_ok => { let _ = ti; }
                        _ => { rej[2] += 1; continue }
                    }
                    if w.cast(o, [0.0, 0.0, 1.0], 0.3).is_some() { rej[3] += 1; continue }
                    if lv < 4 { if let Some(bk) = backs[si] {
                        let (vx, vy) = (pos[0] - sc[0], pos[1] - sc[1]);
                        let vl = vx.hypot(vy).max(1e-9);
                        if (vx * bk[0] + vy * bk[1]) / vl > if lv == 0 { 0.3 } else { 0.6 } { rej[12] += 1; continue }
                    } }
                    // people: face visible (camera in front or 3/4)
                    let mut face_cos = 1.0;
                    if let (Some(fc), true) = (s.face, s.person && !s.lie) {
                        let (vx, vy) = (pos[0] - sc[0], pos[1] - sc[1]);
                        face_cos = (vx * fc[0] + vy * fc[1]) / vx.hypot(vy).max(1e-9);
                        if face_cos < face_min { rej[14] += 1; continue }
                    }
                    let dx = (s.lo[0] - pos[0]).max(0.0).max(pos[0] - s.hi[0]);
                    let dy = (s.lo[1] - pos[1]).max(0.0).max(pos[1] - s.hi[1]);
                    if !s.group && dx.hypot(dy) < if s.person { if faceless[si] && lv < 5 { f64::max(dist_min_p, 2.0) } else { dist_min_p } } else { dist_min } { rej[4] += 1; continue }
                    let bearing = (c[1] - pos[1]).atan2(c[0] - pos[0]);
                    // lens: size the subject box to the frame fraction wanted by the shot type
                    let (flo, fhi) = match (kind, s.group) { (0, _) | (_, true) => (0.6, 1.0), (1, _) => (0.4, 0.72), _ => (0.62, 0.92) };
                    let frac = rng.uniform(flo, fhi);
                    let (lmin, lmax) = if kind == 0 || s.group { (if small { 12.0 } else { 16.0 }, if small { 20.0 } else { 24.0 }) } else { (if small { 12.0 } else { 14.0 }, if kind == 1 { 32.0 } else { 40.0 }) };
                    let lens = if s.group {
                        rng.uniform(lmin, lmax)
                    } else {
                        let corners = [[s.lo[0], s.lo[1]], [s.hi[0], s.lo[1]], [s.hi[0], s.hi[1]], [s.lo[0], s.hi[1]]];
                        let (mut amin, mut amax) = (f64::MAX, f64::MIN);
                        let (mut emin, mut emax) = (f64::MAX, f64::MIN);
                        for cc in corners {
                            let d = wrap((cc[1] - pos[1]).atan2(cc[0] - pos[0]) - bearing);
                            amin = amin.min(d); amax = amax.max(d);
                            let h = (cc[0] - pos[0]).hypot(cc[1] - pos[1]).max(0.05);
                            for z in [s.lo[2], s.hi[2]] { let e = (z - eye).atan2(h); emin = emin.min(e); emax = emax.max(e); }
                        }
                        let th_h = ((amax - amin).max(0.05) / 2.0).tan() / frac;
                        let th_v = ((emax - emin).max(0.05) / 2.0).tan() / frac * 16.0 / 9.0;
                        18.0 / th_h.max(th_v)
                    };
                    let lens = ((lens.clamp(lmin, lmax)) * 2.0).round() / 2.0;
                    let tan_h = 18.0 / lens;
                    let tan_v = tan_h * 9.0 / 16.0;
                    // rule of thirds, lead room: person looks into the frame
                    let mut tx = rng.uniform(0.12, 0.38) * if rng.bool(0.5) { 1.0 } else { -1.0 };
                    if kind == 0 { tx *= 0.5 }
                    let ty = rng.uniform(-0.12, 0.12);
                    let yaw = bearing + (tx * tan_h).atan();
                    let dh = (c[0] - pos[0]).hypot(c[1] - pos[1]);
                    let elev = (c[2] - eye).atan2(dh);
                    let pitch = (elev - (ty * tan_v).atan()).clamp(-5f64.to_radians(), 4f64.to_radians());
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
                        Some((px, py)) if px.abs() < 0.8 && py.abs() < 0.8 => {}
                        _ => { rej[6] += 1; continue }
                    }
                    // subject box in the frame: fully inside (people with headroom), size within the shot's range
                    let (mut inf, mut allp) = (0, 0);
                    let (mut bx0, mut bx1, mut by0, mut by1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                    if s.pts.is_empty() {
                        for k in 0..8 {
                            let q = [if k & 1 == 0 { s.lo[0] } else { s.hi[0] }, if k & 2 == 0 { s.lo[1] } else { s.hi[1] }, if k & 4 == 0 { s.lo[2] } else { s.hi[2] }];
                            if let Some((px, py)) = proj(q) {
                                allp += 1;
                                bx0 = bx0.min(px); bx1 = bx1.max(px); by0 = by0.min(py); by1 = by1.max(py);
                                if px.abs() < 0.98 && py.abs() < 0.98 { inf += 1 }
                            }
                        }
                    } else {
                        // real body vertices; counts scaled to the 8-corner convention (8 = everything)
                        let n = s.pts.len();
                        let (mut i2, mut a2) = (0usize, 0usize);
                        for &q in &s.pts {
                            if let Some((px, py)) = proj(q) {
                                a2 += 1;
                                bx0 = bx0.min(px); bx1 = bx1.max(px); by0 = by0.min(py); by1 = by1.max(py);
                                if px.abs() < 0.98 && py.abs() < 0.98 { i2 += 1 }
                            }
                        }
                        inf = if i2 == n { 8 } else { (i2 * 8 / n).min(7) };
                        allp = if a2 == n { 8 } else { (a2 * 8 / n).min(7) };
                    }
                    if !s.group {
                        if inf < inf_min { rej[9] += 1; continue }
                        let sz = ((bx1 - bx0) / 2.0).max((by1 - by0) / 2.0 * 1.0);
                        if lv < 4 && allp == 8 {
                            let lo_ok = match (kind, lv) { (1, 0) => 0.33, (1, _) => 0.25, (_, 0) => 0.5, _ => 0.35 };
                            if sz < lo_ok && !(lv > 1) { rej[5] += 1; continue }
                        }
                        if s.person && lv < 5 && (by1 > 0.93 || by0 < if lv < 2 { -0.92 } else { -0.97 } || bx0 < -0.97 || bx1 > 0.97) { rej[15] += 1; continue }
                        if s.person && lv < 5 && (by1 - by0) / 2.0 < if lv == 0 { 0.42 } else { 0.28 } { rej[16] += 1; continue }
                    }
                    // other people must not be cut by the frame edge
                    if lv < 6 {
                        let mut cut = false;
                        for q in cand.iter().filter(|q| q.person && q.key != s.key) {
                            let (mut ins, mut tot_p) = (0usize, 0usize);
                            for &pq in &q.pts {
                                tot_p += 1;
                                match proj(pq) { Some((px, py)) => if px.abs() < 1.0 && py.abs() < 1.0 { ins += 1 }, None => {} }
                            }
                            if ins > 0 && ins * 100 < tot_p * 97 { cut = true }
                            if ins * 100 >= tot_p * 97 {
                                let mut blocked = 0;
                                for fz_ in [0.85, 0.6, 0.35] {
                                    let tq = [(q.lo[0] + q.hi[0]) / 2.0, (q.lo[1] + q.hi[1]) / 2.0, q.lo[2] + fz_ * (q.hi[2] - q.lo[2])];
                                    let dq = [tq[0] - o[0], tq[1] - o[1], tq[2] - o[2]];
                                    let dl = (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt();
                                    if let Some((_, ti)) = w.cast(o, unit(dq), dl - 0.15) { if w.keys[w.owner[ti] as usize] != q.key { blocked += 1 } }
                                }
                                if blocked >= 2 { cut = true }
                            }
                        }
                        if cut { rej[23] += 1; continue }
                    }
                    // coarse ray grid
                    let (gw, gh) = (16, 9);
                    let (mut near, mut close, mut tot, mut dsum, mut mid, mut doorc) = (0, 0, 0, 0.0f64, 0, 0);
                    let (mut furn, mut sa, mut fg, mut bg, mut blk, mut wallall) = (0, 0, 0, 0, 0, 0);
                    let mut kc = vec![0u32; w.keys.len()];
                    for iy in 0..gh {
                        for ix in 0..gw {
                            let nx = ((ix as f64 + 0.5) / gw as f64) * 2.0 - 1.0;
                            let ny = 1.0 - ((iy as f64 + 0.5) / gh as f64) * 2.0;
                            let d = unit([f[0] + r[0] * nx * tan_h + u[0] * ny * tan_v, f[1] + r[1] * nx * tan_h + u[1] * ny * tan_v, f[2] + u[2] * ny * tan_v]);
                            tot += 1;
                            if let Some((t, ti)) = w.cast(o, d, 6.0) {
                                dsum += t;
                                let ko = w.owner[ti] as usize;
                                if t < 1.8 { mid += 1 }
                                if t < 4.5 && t > 0.7 && w.door[ti] { doorc += 1 }
                                if !w.shell[ti] { furn += 1; kc[ko] += 1 }
                                if s.group { if !w.shell[ti] { sa += 1 } } else if w.keys[ko] == s.key { sa += 1 }
                                if t < 1.8 && !w.floor[ti] && !w.keys[ko].contains(".wall") && !w.keys[ko].contains(".ceiling") && if s.group { w.shell[ti] } else { w.keys[ko] != s.key } { blk += if nx.abs() < 0.6 { 2 } else { 1 } }
                                if w.keys[ko].contains(".wall") { wallall += 1 }
                                if t < 2.0 { fg += 1 } else if t > 3.5 { bg += 1 }
                                if t < 1.2 { close += 1 }
                                if t < near_d { near += 1 }
                            } else {
                                dsum += 6.0;
                            }
                        }
                    }
                    let tf = tot as f64;
                    if near as f64 > near_f * tf || close as f64 > 0.6 * tf { rej[7] += 1; continue }
                    let (furn_f, sa_f, doorf, blk_f, wall_f) = (furn as f64 / tf, sa as f64 / tf, doorc as f64 / tf, blk as f64 / tf, wallall as f64 / tf);
                    let sa_min = if s.person { 0.015 } else if s.group { 0.12 } else { 0.04 } * [1.0, 0.6, 0.4, 0.3, 0.2][lv.min(4)];
                    let big_other = w.keys.iter().enumerate().any(|(ki, kk)| kc[ki] as f64 / tf > key_max && !(s.group || *kk == s.key));
                    let big_subj = !s.group && !s.person && sa_f > [0.5, 0.55, 0.6, 0.7, 0.7, 0.8][lv.min(5)];
                    let outs_bad = outside_ok && (sa_f < 0.08 || furn_f < 0.1 || wall_f > 0.4);
                    let fails = [outs_bad, doorf > door_max, blk_f > blk_max, wall_f > wall_max, big_other, big_subj, furn_f < furn_min, sa_f < sa_min, mid as f64 > mid_max * tf];
                    if fails.iter().any(|&b| b) { rej[8] += 1; for (i, b) in fails.iter().enumerate() { if *b && i < 7 { rej[16 + i] += 1 } } continue }
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
                                    Some(_) if s.group => true,
                                    Some((_, ti)) => w.keys[w.owner[ti] as usize] == s.key,
                                };
                                if ok { vis += 1 }
                            }
                        }
                    }
                    if (vis as f64) < vis_min * n as f64 { rej[10] += 1; continue }
                    // spread against the cameras already chosen
                    if got.iter().any(|(p, ff, sk)| {
                        let dd = ((p[0] - o[0]).powi(2) + (p[1] - o[1]).powi(2)).sqrt();
                        let ang = (ff[0] * f[0] + ff[1] * f[1]).clamp(-1.0, 1.0).acos().to_degrees();
                        dd < sp_d && ang < sp_a || (*sk == s.key && ang < 25.0 && dd < 1.5) || ang < 12.0
                    }) { rej[11] += 1; continue }
                    let px = proj(c).map_or(0.0, |p| p.0);
                    let lead = match s.face { Some(fc) if s.person && !s.lie => if px * (fc[0] * r[0] + fc[1] * r[1]) < 0.0 { 0.3 } else { 0.0 }, _ => 0.0 };
                    let score = dsum / tf / 6.0 + 0.8 * inf as f64 / 8.0 - 3.0 * doorf - 3.0 * blk_f - 2.0 * (wall_f - 0.2).max(0.0) - 0.8 * pitch.abs() / 0.1 - 1.5 * (sa_f - 0.4).max(0.0) + 1.0 * (sa_f / 0.2).min(1.0)
                        + 0.8 * (furn_f / 0.4).min(1.0) + if fg as f64 > 0.1 * tf && bg as f64 > 0.1 * tf { 0.3 } else { 0.0 } - 0.5 * mid as f64 / tf + 0.8 * vis as f64 / n as f64
                        + 0.4 * s.w.min(6.0) / 6.0 + 0.2 * rng.f64() - 0.6 * lv as f64 + if s.person { 1.2 * face_cos.max(-0.5) + lead } else { 0.0 }
                        + if kind == 0 { 0.5 * (dsum / tf / 6.0) } else { 0.0 };
                    nvalid += 1;
                    if best.as_ref().map_or(true, |b| score > b.0) { best = Some((score, si, o, f, lens)) }
                }
                if std::env::var("CAMDBG").map_or(false, |v| v == "2") { eprintln!("  slot{slot} lv{lv} valid={nvalid} rej={:?}", &rej[..24]); }
                if best.is_some() { chosen = best; lvl_hist[lv] += 1; break 'lv }
            }
            let (si, o, f, lens) = match chosen {
                Some((_, si, o, f, lens)) => (si, o, f, lens),
                None => {
                    // last resort: a corner wide shot from the polygon vertex pulled in, farthest from earlier cameras
                    lvl_hist[6] += 1;
                    let g = &cand[gi];
                    let gt = [(g.lo[0] + g.hi[0]) / 2.0, (g.lo[1] + g.hi[1]) / 2.0];
                    let ez = (fz + 1.4).min(cz - 0.35);
                    let flens = if small { 12.0 } else { 14.0 };
                    let mut bestp: Option<(f64, [f64; 2], V3)> = None;
                    for (p, outs) in pool.iter().filter(|(_, o)| !*o) {
                        let o = [p[0], p[1], ez];
                        if *outs {
                            let free = (0..8).all(|k| { let a = k as f64 * std::f64::consts::FRAC_PI_4; w.cast(o, [a.cos(), a.sin(), 0.0], 0.2).map_or(true, |(_, ti)| w.shell[ti]) });
                            if !free || w.cast(o, [0.0, 0.0, -1.0], ez - fz + 0.4).is_none() { continue }
                        }
                        for (tgt, yo) in [(gt, 0.0), (cen, 0.0), (gt, -0.45), (gt, 0.45), (cen, -0.45), (cen, 0.45)] {
                            let (dx0, dy0) = (tgt[0] - p[0], tgt[1] - p[1]);
                            let dh = dx0.hypot(dy0);
                            if dh < 0.8 { continue }
                            let (dx, dy) = (dx0 * f64::cos(yo) - dy0 * f64::sin(yo), dx0 * f64::sin(yo) + dy0 * f64::cos(yo));
                            let pitch = ((fz + 0.9 - ez).atan2(dh)).clamp(-5f64.to_radians(), 4f64.to_radians());
                            let f = [dx / dh * pitch.cos(), dy / dh * pitch.cos(), pitch.sin()];
                            let (r, u) = ([f[1], -f[0], 0.0], [-f[0] / (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9) * f[2], -f[1] / (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9) * f[2], (f[0] * f[0] + f[1] * f[1]).sqrt()]);
                            let rn = (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-9);
                            let r = [r[0] / rn, r[1] / rn, 0.0];
                            let (th, tv) = (18.0 / flens, 18.0 / flens * 9.0 / 16.0);
                            let (mut wl, mut nr, mut ds, mut fu) = (0, 0, 0.0, 0);
                            let mut cutp = 0.0;
                            for q in cand.iter().filter(|q| q.person) {
                                let (mut ins, mut vis_n) = (0usize, 0usize);
                                for &pq in &q.pts {
                                    let dq = [pq[0] - o[0], pq[1] - o[1], pq[2] - o[2]];
                                    let zq = dq[0] * f[0] + dq[1] * f[1] + dq[2] * f[2];
                                    if zq < 0.1 { continue }
                                    vis_n += 1;
                                    let (px, py) = ((dq[0] * r[0] + dq[1] * r[1]) / zq / th, (dq[0] * u[0] + dq[1] * u[1] + dq[2] * u[2]) / zq / tv);
                                    if px.abs() < 1.0 && py.abs() < 1.0 { ins += 1 }
                                }
                                let nn = q.pts.len();
                                if (ins > 0 && ins * 100 < nn * 97) || (vis_n > 0 && vis_n * 100 < nn * 97) { cutp += 1.0 }
                            }
                            for iy in 0..5 { for ix in 0..8 {
                                let nx = ((ix as f64 + 0.5) / 8.0) * 2.0 - 1.0;
                                let ny = 1.0 - ((iy as f64 + 0.5) / 5.0) * 2.0;
                                let d = unit([f[0] + r[0] * nx * th + u[0] * ny * tv, f[1] + r[1] * nx * th + u[1] * ny * tv, f[2] + u[2] * ny * tv]);
                                if let Some((t, ti)) = w.cast(o, d, 6.0) {
                                    ds += t;
                                    if w.keys[w.owner[ti] as usize].contains(".wall") { wl += 1 }
                                    if t < 0.9 { nr += 1 }
                                    if !w.shell[ti] { fu += 1 }
                                } else { ds += 6.0 }
                            } }
                            let dmin = got.iter().map(|(q, ff, _)| if (q[0] - p[0]).hypot(q[1] - p[1]) < 0.8 && (ff[0] * f[0] + ff[1] * f[1]).clamp(-1.0, 1.0).acos().to_degrees() < 25.0 { 0.0 } else { 1.0 }).fold(1.0, f64::min);
                            if dmin < 0.5 { continue }
                            let kmax = { let mut kc = vec![0u32; w.keys.len()]; for iy in 0..5 { for ix in 0..8 { let nx = ((ix as f64 + 0.5) / 8.0) * 2.0 - 1.0; let ny = 1.0 - ((iy as f64 + 0.5) / 5.0) * 2.0; let d = unit([f[0] + r[0] * nx * th + u[0] * ny * tv, f[1] + r[1] * nx * th + u[1] * ny * tv, f[2] + u[2] * ny * tv]); if let Some((t, ti)) = w.cast(o, d, 6.0) { if !w.shell[ti] && t < 3.0 { kc[w.owner[ti] as usize] += 1 } } } } kc.into_iter().max().unwrap_or(0) as f64 / 40.0 };
                            let sc = -20.0 * cutp - 6.0 * (kmax - 0.22).max(0.0) - 3.0 * wl as f64 / 40.0 - 3.0 * nr as f64 / 40.0 + ds / 40.0 / 6.0 + 0.8 * (fu as f64 / 40.0).min(0.4) - 2.0 * (1.0 - dmin);
                            if bestp.map_or(true, |b| sc > b.0) { bestp = Some((sc, [p[0], p[1]], f)) }
                        }
                    }
                    let (o, f) = match bestp { Some((_, p, f)) => ([p[0], p[1], ez], f), None => {
                        let (dx, dy) = (gt[0] - cen[0], gt[1] - cen[1]);
                        let dh = dx.hypot(dy).max(1e-6);
                        ([cen[0], cen[1], ez], [dx / dh, dy / dh, 0.0])
                    } };
                    (gi, o, f, flens)
                }
            };
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
        let _ = has_people;
        if std::env::var("CAMDBG").is_ok() { eprintln!("CAMDBG {rname} area={area:.1} got={} levels={:?} rej={:?}", got.len(), lvl_hist, &rej[..24]); }
    }
    out
}
