//! 2D layout geometry for the solver: every object is a set of vertical prisms (the convex hull of each collision proxy projected
//! to the floor, with the proxy's height range). For prisms the 3D penetration depth is min(footprint depth, height overlap) and the
//! distance is sqrt(footprint distance^2 + height gap^2), so the move validity check needs no 3D contact queries. Rooms are their
//! inner floor polygon (the union of the room mesh's floor triangles): an object must stay inside the floor of the house.
//!
//! On top of the plain collision test, floor moves are checked against two layout rules:
//!   * walkways: the doors of the room stay connected by a 0.6 m clear path and every floor piece (not rugs, chairs, lamps, plants,
//!     side tables) stays within 0.45 m of it. Door points are the door clearance keep-outs (`doorclear/<room>/<i>`).
//!   * hallways: floor pieces at most 0.45 m deep.
//! and `extra_loss` adds the living-room grouping terms (sofa facing the TV stand at 2-4 m, armchairs turned to the coffee table).
use super::state::*;
use super::tags::{Tag, TagSet};
use crate::dhash::HashMap;
use crate::shp;
use std::sync::Arc;

/// runtime switches of the 2D layout (set per run: from the environment natively, by the caller in the browser build)
pub mod knobs {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    static RULES: AtomicBool = AtomicBool::new(true);
    static SIZES: AtomicBool = AtomicBool::new(true);
    static BUILTIN_FIT: AtomicBool = AtomicBool::new(true);
    static GROUPS: AtomicBool = AtomicBool::new(true);
    static FURNITURE_ONLY: AtomicBool = AtomicBool::new(false);
    static GROUP_W: AtomicU64 = AtomicU64::new(0x4008000000000000); // 3.0
    static SIZE_W: AtomicU64 = AtomicU64::new(0x3fe0000000000000); // 0.5
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    pub struct Knobs {
        /// walkway / hallway rules in the move check
        pub rules: bool,
        /// real-world size densities for placeholders (off: assets' own sizes)
        pub sizes: bool,
        /// built-ins extended to their wall span
        pub builtin_fit: bool,
        /// slot groups (fridge / dishwasher at the run, nightstands, desk chairs)
        pub groups: bool,
        /// stop after the furniture stages (no small items on surfaces)
        pub furniture_only: bool,
        /// weight of the living-room grouping loss (sofa / TV / coffee table / armchairs)
        pub group_w: f64,
        /// weight of the size -log density loss
        pub size_w: f64,
    }
    impl Default for Knobs {
        fn default() -> Self { Knobs { rules: true, sizes: true, builtin_fit: true, groups: true, furniture_only: false, group_w: 3.0, size_w: 0.5 } }
    }
    pub fn set(k: &Knobs) {
        RULES.store(k.rules, Relaxed);
        SIZES.store(k.sizes, Relaxed);
        BUILTIN_FIT.store(k.builtin_fit, Relaxed);
        GROUPS.store(k.groups, Relaxed);
        FURNITURE_ONLY.store(k.furniture_only, Relaxed);
        GROUP_W.store(k.group_w.to_bits(), Relaxed);
        SIZE_W.store(k.size_w.to_bits(), Relaxed);
    }
    pub fn get() -> Knobs {
        Knobs { rules: rules(), sizes: sizes(), builtin_fit: builtin_fit(), groups: groups(), furniture_only: furniture_only(), group_w: group_w(), size_w: size_w() }
    }
    /// native runs: NO_LAYOUT_RULES, NO_SIZES, NO_BUILTIN_FIT, NO_GROUPS, LAYOUT_DUMP_STAGE=furniture, LAYOUT_W, LAYOUT_SIZE_W
    pub fn from_env() -> Knobs {
        let on = |k: &str| std::env::var_os(k).is_some();
        let num = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Knobs { rules: !on("NO_LAYOUT_RULES"), sizes: !on("NO_SIZES"), builtin_fit: !on("NO_BUILTIN_FIT"), groups: !on("NO_GROUPS"),
            furniture_only: std::env::var("LAYOUT_DUMP_STAGE").as_deref() == Ok("furniture"), group_w: num("LAYOUT_W", 3.0), size_w: num("LAYOUT_SIZE_W", 0.5) }
    }
    pub fn rules() -> bool { RULES.load(Relaxed) }
    pub fn sizes() -> bool { SIZES.load(Relaxed) }
    pub fn builtin_fit() -> bool { BUILTIN_FIT.load(Relaxed) }
    pub fn groups() -> bool { GROUPS.load(Relaxed) }
    pub fn furniture_only() -> bool { FURNITURE_ONLY.load(Relaxed) }
    pub fn group_w() -> f64 { f64::from_bits(GROUP_W.load(Relaxed)) }
    pub fn size_w() -> f64 { f64::from_bits(SIZE_W.load(Relaxed)) }
}

#[derive(Clone, Debug)]
pub struct Prism {
    /// convex footprint, counter-clockwise
    pub poly: Vec<[f64; 2]>,
    pub z: [f64; 2],
    pub bb: [f64; 4],
}

thread_local! {
    static PRISMS: std::cell::RefCell<crate::dhash::FxMap<String, (u64, Arc<Vec<Prism>>)>> = std::cell::RefCell::new(Default::default());
    static FLOORS: std::cell::RefCell<crate::dhash::FxMap<String, (u64, Arc<shp::G>)>> = std::cell::RefCell::new(Default::default());
    static BASE: std::cell::RefCell<crate::dhash::FxMap<String, bool>> = std::cell::RefCell::new(Default::default());
}

pub fn clear_cache() {
    PRISMS.with(|c| c.borrow_mut().clear());
    FLOORS.with(|c| c.borrow_mut().clear());
    BASE.with(|c| c.borrow_mut().clear());
}

pub fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap().then(a[1].partial_cmp(&b[1]).unwrap()));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<[f64; 2]> = vec![];
    for p in &pts {
        while lo.len() >= 2 && cross(lo[lo.len() - 2], lo[lo.len() - 1], *p) <= 1e-12 {
            lo.pop();
        }
        lo.push(*p);
    }
    let mut hi: Vec<[f64; 2]> = vec![];
    for p in pts.iter().rev() {
        while hi.len() >= 2 && cross(hi[hi.len() - 2], hi[hi.len() - 1], *p) <= 1e-12 {
            hi.pop();
        }
        hi.push(*p);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo
}

fn bbox(p: &[[f64; 2]]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for q in p {
        b[0] = b[0].min(q[0]);
        b[1] = b[1].min(q[1]);
        b[2] = b[2].max(q[0]);
        b[3] = b[3].max(q[1]);
    }
    b
}

pub fn prisms(state: &State, name: &str) -> Arc<Vec<Prism>> {
    let o = &state.objs[name];
    if let Some(p) = PRISMS.with(|c| c.borrow().get(name).filter(|(v, _)| *v == o.version).map(|(_, p)| p.clone())) {
        return p;
    }
    let mut out = vec![];
    for px in super::col::proxies(state, name).iter() {
        let z0 = px.corners.iter().map(|c| c[2]).fold(f64::INFINITY, f64::min);
        let z1 = px.corners.iter().map(|c| c[2]).fold(f64::NEG_INFINITY, f64::max);
        let poly = hull(px.corners.iter().map(|c| [c[0], c[1]]).collect());
        if poly.len() < 3 {
            continue;
        }
        let bb = bbox(&poly);
        out.push(Prism { poly, z: [z0, z1], bb });
    }
    let arc = Arc::new(out);
    PRISMS.with(|c| c.borrow_mut().insert(name.to_string(), (o.version, arc.clone())));
    arc
}

/// separating-axis overlap of two convex polygons (0 when separated)
fn sat_depth(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    for poly in [a, b] {
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (nx, ny) = (q[1] - p[1], p[0] - q[0]);
            let l = (nx * nx + ny * ny).sqrt();
            if l < 1e-12 {
                continue;
            }
            let (nx, ny) = (nx / l, ny / l);
            let proj = |s: &[[f64; 2]]| s.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| { let d = v[0] * nx + v[1] * ny; (lo.min(d), hi.max(d)) });
            let (a0, a1) = proj(a);
            let (b0, b1) = proj(b);
            let o = a1.min(b1) - a0.max(b0);
            if o <= 0.0 {
                return 0.0;
            }
            best = best.min(o);
        }
    }
    best
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

pub fn poly_dist(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    if sat_depth(a, b) > 0.0 {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for (s, t) in [(a, b), (b, a)] {
        for p in s {
            for i in 0..t.len() {
                best = best.min(seg_dist(*p, t[i], t[(i + 1) % t.len()]));
            }
        }
    }
    best
}

pub fn prism_depth(a: &Prism, b: &Prism) -> f64 {
    let zo = a.z[1].min(b.z[1]) - a.z[0].max(b.z[0]);
    if zo <= 0.0 || a.bb[0] >= b.bb[2] || b.bb[0] >= a.bb[2] || a.bb[1] >= b.bb[3] || b.bb[1] >= a.bb[3] {
        return 0.0;
    }
    zo.min(sat_depth(&a.poly, &b.poly))
}

pub fn prism_dist(a: &Prism, b: &Prism) -> f64 {
    let dz = (a.z[0].max(b.z[0]) - a.z[1].min(b.z[1])).max(0.0);
    let d = poly_dist(&a.poly, &b.poly);
    (d * d + dz * dz).sqrt()
}

pub fn is_room(o: &ObjState) -> bool {
    o.tags.contains(Tag::sem("Room"))
}

/// object-object penetration depth (rooms excluded: see `outside_depth`). The deepest overlap of any one pair of convex parts: for a
/// piece built of boards (cabinets, shelves) that is at most a board's thickness however far the piece stands into the other (a cabinet
/// 0.35 m into a door keep-out read 0.02); groups::door_keepouts measures world boxes for that reason. The callers that ask "any overlap"
/// (> 1e-4: the solver's hit lists, try_place, the dof moves) are exact; only a threshold above a board's thickness would miss
pub fn depth(state: &State, a: &str, b: &str) -> f64 {
    let (pa, pb) = (prisms(state, a), prisms(state, b));
    let mut best = 0.0f64;
    for x in pa.iter() {
        for y in pb.iter() {
            best = best.max(prism_depth(x, y));
        }
    }
    best
}

/// object-object distance (prisms: exact for the projected proxies)
pub fn dist(state: &State, a: &str, b: &str) -> f64 {
    let (pa, pb) = (prisms(state, a), prisms(state, b));
    let mut best = f64::INFINITY;
    for x in pa.iter() {
        for y in pb.iter() {
            best = best.min(prism_dist(x, y));
        }
    }
    best
}

/// inner floor polygon of a room: the union of its floor (support, upward) triangles
pub fn room_floor(state: &State, room: &str) -> Arc<shp::G> {
    let o = &state.objs[room];
    if let Some(p) = FLOORS.with(|c| c.borrow().get(room).filter(|(v, _)| *v == o.version).map(|(_, p)| p.clone())) {
        return p;
    }
    let w = state.world(room);
    let mut tris = vec![];
    for (i, t) in w.tris.iter().enumerate() {
        let tags = o.geom.tris[i].tags;
        let n = crate::mesh::cross(crate::mesh::sub(t[1], t[0]), crate::mesh::sub(t[2], t[0]));
        if tags & super::geom::SUPPORT != 0 && n[2] > 1e-9 && t.iter().all(|v| v[2].abs() < 0.05) {
            if let Ok(g) = shp::polygon(&[[t[0][0], t[0][1]], [t[1][0], t[1][1]], [t[2][0], t[2][1]], [t[0][0], t[0][1]]], &[]) {
                tris.push(g);
            }
        }
    }
    let g = if tris.is_empty() { o.polygon.clone().unwrap_or_else(|| shp::polygon(&[[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], &[]).unwrap()) } else { shp::buffer(&shp::union_all(&tris), 0.0) };
    // the floor runs under the walls to their centrelines: shrink it to the inner wall faces (half the wall thickness = the smallest
    // distance of a wall-face vertex from the floor outline)
    let outline = shp::boundary(&g);
    let half = w.tris.iter().enumerate().filter(|(i, _)| o.geom.tris[*i].tags & super::geom::WALL != 0).flat_map(|(_, t)| t.iter().map(|v| shp::distance(&outline, &shp::point(v[0], v[1]))).collect::<Vec<_>>()).filter(|d| *d > 1e-3).fold(f64::INFINITY, f64::min);
    let g = if half.is_finite() && half < 0.5 { shp::buffer_mitre(&g, -half) } else { g };
    let arc = Arc::new(g);
    FLOORS.with(|c| c.borrow_mut().insert(room.to_string(), (o.version, arc.clone())));
    arc
}

pub fn rings(g: &shp::G) -> Vec<Vec<[f64; 2]>> {
    let mut out = vec![];
    for p in shp::parts(g) {
        if !shp::is_polygon(&p) {
            continue;
        }
        out.push(shp::exterior(&p));
        out.extend(shp::interiors(&p));
    }
    out
}

pub fn in_rings(rs: &[Vec<[f64; 2]>], p: [f64; 2]) -> bool {
    let mut inside = false;
    for r in rs {
        let n = r.len();
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (r[i], r[j]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

pub fn ring_dist(rs: &[Vec<[f64; 2]>], p: [f64; 2]) -> f64 {
    let mut best = f64::INFINITY;
    for r in rs {
        for i in 0..r.len() {
            best = best.min(seg_dist(p, r[i], r[(i + 1) % r.len()]));
        }
    }
    best
}

pub fn point_in_convex(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    // depth of p inside a ccw convex polygon (<= 0 outside)
    let mut d = f64::INFINITY;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let l = (ex * ex + ey * ey).sqrt().max(1e-12);
        d = d.min(((p[0] - a[0]) * ey * -1.0 + (p[1] - a[1]) * ex) / l);
    }
    d
}

thread_local! {
    static HOUSE: std::cell::RefCell<Option<(Vec<(String, u64)>, Arc<Vec<Vec<[f64; 2]>>>)>> = std::cell::RefCell::new(None);
}

/// rings of the union of every room's floor
fn house_rings(state: &State) -> Arc<Vec<Vec<[f64; 2]>>> {
    let mut key: Vec<(String, u64)> = state.objs.iter().filter(|(_, o)| is_room(o)).map(|(k, o)| (k.clone(), o.version)).collect();
    key.sort();
    if let Some(r) = HOUSE.with(|h| h.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, r)| r.clone())) {
        return r;
    }
    let floors: Vec<shp::G> = key.iter().map(|(k, _)| (*room_floor(state, k)).clone()).collect();
    let u = shp::buffer(&shp::union_all(&floors), 0.0);
    let r = Arc::new(rings(&u));
    HOUSE.with(|h| *h.borrow_mut() = Some((key, r.clone())));
    r
}

/// how far the object's footprint pokes out of the house floor (walls), 0 when inside
pub fn outside_depth(state: &State, name: &str) -> f64 {
    let rs = house_rings(state);
    let mut best = 0.0f64;
    for p in prisms(state, name).iter() {
        for v in &p.poly {
            if !in_rings(&rs, *v) {
                best = best.max(ring_dist(&rs, *v));
            }
        }
        // floor-polygon corners inside the footprint (a wall corner poking into the piece)
        for r in rs.iter() {
            for v in r {
                if v[0] > p.bb[0] && v[0] < p.bb[2] && v[1] > p.bb[1] && v[1] < p.bb[3] {
                    best = best.max(point_in_convex(&p.poly, *v));
                }
            }
        }
    }
    best
}

/// how far the object's footprint pokes out of its own room's floor (rooms are solved on separate copies of the house: a piece moved
/// across a doorway into the next room would meet that room's furniture only when the copies are merged)
pub fn outside_room_depth(state: &State, name: &str, room: &str) -> f64 {
    let rs = rings(&room_floor(state, room));
    let mut best = 0.0f64;
    for p in prisms(state, name).iter() {
        for v in &p.poly {
            if !in_rings(&rs, *v) {
                best = best.max(ring_dist(&rs, *v));
            }
        }
    }
    best
}

/// the room an object stands in: a relation target that is a room, else its parent's room
pub fn room_of(state: &State, name: &str) -> Option<String> {
    let mut cur = name.to_string();
    for _ in 0..4 {
        let o = state.objs.get(&cur)?;
        if let Some(r) = o.relations.iter().find(|r| state.objs.get(&r.target).map_or(false, is_room)) {
            return Some(r.target.clone());
        }
        cur = o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.contains_key(t))?;
    }
    None
}

fn class_of(o: &ObjState) -> &str {
    o.generator.as_ref().map(|g| g.class.as_str()).unwrap_or("")
}

/// pieces that do not block the floor (rugs) or need no access path
fn no_block(cls: &str) -> bool {
    cls.contains("Rug")
}
fn no_reach(cls: &str) -> bool {
    no_block(cls) || cls.contains("Chair") && !cls.contains("ArmChair") || cls.contains("Stool") || cls.contains("Lamp") || cls.contains("Plant") || cls.contains("SideTable") || cls.contains("Hardware")
}

/// floor footprint of a piece standing on the floor (bottom below 0.25 m, no furniture parent except for chairs: dining chairs hang off
/// their table, books / trinkets inside low cabinets also start below 0.25 m), else None
pub fn floor_foot(state: &State, name: &str) -> Option<Vec<Vec<[f64; 2]>>> {
    let o = &state.objs[name];
    let cls = class_of(o);
    if cls.contains("Art") || cls.contains("Mirror") || cls.contains("Light") {
        return None;
    }
    if !(cls.contains("Chair") || cls.contains("Stool")) && o.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())) {
        return None;
    }
    let ps = prisms(state, name);
    let z0 = ps.iter().map(|p| p.z[0]).fold(f64::INFINITY, f64::min);
    if !(z0 < 0.25) {
        return None;
    }
    Some(ps.iter().filter(|p| p.z[0] < 1.2).map(|p| p.poly.clone()).collect())
}

fn polys_g(ps: &[Vec<[f64; 2]>]) -> Vec<shp::G> {
    ps.iter().filter_map(|p| { let mut r = p.clone(); r.push(p[0]); shp::polygon(&r, &[]).ok() }).collect()
}

/// walkway status of a room: (doors connected, unreachable pieces)
pub fn walk_status(state: &State, room: &str) -> (bool, Vec<String>) {
    walk_status_f(state, room, &|_: &str| true, None)
}

/// walkway status counting only the pieces `keep` accepts, without `skip`
pub fn walk_status_f(state: &State, room: &str, keep: &dyn Fn(&str) -> bool, skip: Option<&str>) -> (bool, Vec<String>) {
    let nc = Tag::sem("NoCollision");
    let floor = room_floor(state, room);
    let mut blocks = vec![];
    // reach ignores the pieces that need no path of their own (chairs are pulled out to reach a table): they block door paths only
    let mut reach_blocks = vec![];
    let mut pieces = vec![];
    for (k, o) in &state.objs {
        if o.generator.is_none() || o.tags.contains(nc) || is_room(o) {
            continue;
        }
        let cls = class_of(o);
        if cls == "Door" || cls == "Window" || no_block(cls) {
            continue;
        }
        if skip == Some(k.as_str()) || !keep(k) {
            continue;
        }
        let Some(f) = floor_foot(state, k) else { continue };
        if room_of(state, k).as_deref() != Some(room) {
            continue;
        }
        let g = polys_g(&f);
        if !no_reach(cls) {
            pieces.push((k.clone(), g.clone()));
            reach_blocks.extend(g.clone());
        }
        blocks.extend(g);
    }
    let doors: Vec<[f64; 2]> = state.objs.iter().filter(|(k, _)| k.starts_with(&format!("doorclear/{room}/"))).filter_map(|(k, _)| {
        let p = prisms(state, k);
        p.first().map(|p| { let n = p.poly.len() as f64; [p.poly.iter().map(|v| v[0]).sum::<f64>() / n, p.poly.iter().map(|v| v[1]).sum::<f64>() / n] })
    }).collect();
    if doors.is_empty() {
        return (true, vec![]);
    }
    let free = if blocks.is_empty() { (*floor).clone() } else { shp::difference(&floor, &shp::union_all(&blocks)) };
    let walk = shp::buffer_mitre(&free, -0.3);
    let comps: Vec<shp::G> = if shp::is_empty(&walk) { vec![] } else { shp::parts(&walk).into_iter().filter(|g| !shp::is_empty(g)).collect() };
    let hits: Vec<Vec<usize>> = doors.iter().map(|d| { let p = shp::point(d[0], d[1]); comps.iter().enumerate().filter(|(_, c)| shp::distance(c, &p) < 0.35).map(|(i, _)| i).collect() }).collect();
    let common: Vec<usize> = (0..comps.len()).filter(|i| hits.iter().all(|h| h.contains(i))).collect();
    let connected = !common.is_empty();
    if std::env::var_os("WALKDBG").is_some() && !connected {
        let fb: Vec<shp::G> = pieces.iter().filter(|p| p.0.contains("Feature")).flat_map(|p| p.1.clone()).collect();
        let ff = if fb.is_empty() { (*floor).clone() } else { shp::difference(&floor, &shp::union_all(&fb)) };
        let fw = shp::buffer_mitre(&ff, -0.3);
        let fc: Vec<shp::G> = if shp::is_empty(&fw) { vec![] } else { shp::parts(&fw) };
        let fh: Vec<Vec<usize>> = doors.iter().map(|d| { let p = shp::point(d[0], d[1]); fc.iter().enumerate().filter(|(_, c)| shp::distance(c, &p) < 0.35).map(|(i, _)| i).collect() }).collect();
        eprintln!("WALKDBG {room}: features only: comps {} hits {:?}", fc.len(), fh);
    }
    if std::env::var_os("WALKDBG").is_some() && !connected { eprintln!("WALKDBG {room}: pieces {:?} comps {} doors {:?} hits {:?} comp areas {:?}", pieces.iter().map(|p| p.0.clone()).collect::<Vec<_>>(), comps.len(), doors, hits, comps.iter().map(|c| (shp::area(c) * 100.0).round() / 100.0).collect::<Vec<_>>()); }
    // reach: walkable floor without the chair-like pieces, the components touching a door
    // (no chair-like pieces: the same walkable floor as above)
    let comps2: Vec<shp::G> = if reach_blocks.len() == blocks.len() { comps.clone() } else {
        let free2 = if reach_blocks.is_empty() { (*floor).clone() } else { shp::difference(&floor, &shp::union_all(&reach_blocks)) };
        let walk2 = shp::buffer_mitre(&free2, -0.3);
        if shp::is_empty(&walk2) { vec![] } else { shp::parts(&walk2).into_iter().filter(|g| !shp::is_empty(g)).collect() }
    };
    let reach_set: Vec<usize> = (0..comps2.len()).filter(|&i| doors.iter().any(|d| shp::distance(&comps2[i], &shp::point(d[0], d[1])) < 0.35)).collect();
    let mut bad = vec![];
    if ig_core::env_on!("WALKDBG") { eprintln!("WALKDBG {room}: floor area {:.2} bounds {:?} poly area {:.2}", shp::area(&floor), shp::bounds(&floor), state.objs[room].polygon.as_ref().map_or(0.0, |p| shp::area(p))); }
    if ig_core::env_on!("WALKDBG") { eprintln!("WALKDBG {room}: doors {} comps {} connected {connected} reach comps {} pieces {:?}", doors.len(), comps.len(), reach_set.len(), pieces.iter().map(|p| class_of(&state.objs[&p.0])).collect::<Vec<_>>()); }
    for (k, gs) in &pieces {
        let d = gs.iter().map(|g| reach_set.iter().map(|&i| shp::distance(g, &comps2[i])).fold(f64::INFINITY, f64::min)).fold(f64::INFINITY, f64::min);
        if d > 0.45 {
            bad.push(k.clone());
        }
    }
    (connected, bad)
}

/// doors of the empty room connected (cached): rooms too narrow to pass at all are not held to the walkway rule
fn base_connected(state: &State, room: &str) -> bool {
    if let Some(b) = BASE.with(|c| c.borrow().get(room).cloned()) {
        return b;
    }
    let floor = room_floor(state, room);
    let doors: Vec<[f64; 2]> = state.objs.iter().filter(|(k, _)| k.starts_with(&format!("doorclear/{room}/"))).filter_map(|(k, _)| {
        let p = prisms(state, k);
        p.first().map(|p| { let n = p.poly.len() as f64; [p.poly.iter().map(|v| v[0]).sum::<f64>() / n, p.poly.iter().map(|v| v[1]).sum::<f64>() / n] })
    }).collect();
    let walk = shp::buffer_mitre(&floor, -0.3);
    let comps: Vec<shp::G> = if shp::is_empty(&walk) { vec![] } else { shp::parts(&walk) };
    let ok = doors.is_empty() || (0..comps.len()).any(|i| doors.iter().all(|d| shp::distance(&comps[i], &shp::point(d[0], d[1])) < 0.35));
    BASE.with(|c| c.borrow_mut().insert(room.to_string(), ok));
    ok
}

fn min_width(poly: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (nx, ny) = (q[1] - p[1], p[0] - q[0]);
        let l = (nx * nx + ny * ny).sqrt();
        if l < 1e-9 {
            continue;
        }
        let (lo, hi) = poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| { let d = (v[0] * nx + v[1] * ny) / l; (lo.min(d), hi.max(d)) });
        best = best.min(hi - lo);
    }
    best
}

/// layout rules for a floor move of `name` (after the collision test passed)
pub fn layout_ok(state: &State, name: &str) -> bool {
    if !knobs::rules() {
        return true;
    }
    let o = &state.objs[name];
    let cls = class_of(o);
    if o.generator.is_none() || cls == "Door" || cls == "Window" {
        return true;
    }
    let Some(f) = floor_foot(state, name) else { return true };
    let Some(room) = room_of(state, name) else { return true };
    if room.starts_with("hallway") && !no_reach(cls) {
        let all: Vec<[f64; 2]> = f.iter().flatten().cloned().collect();
        if min_width(&hull(all)) > 0.45 {
            return false;
        }
    }
    if no_block(cls) {
        return true;
    }
    // anchors (bed, sofa, counter run, dining table, bathroom fixtures) take precedence: they are held to the walkway rule against the
    // other anchors only (the repair pass then removes lesser pieces in the way); any other piece is rejected when it makes the room
    // worse than it is without it (doors disconnected, a piece cut off, or the piece itself unreachable)
    let base = base_connected(state, &room);
    let (connected, bad, ok);
    if anchor(cls) {
        (connected, bad) = walk_status_f(state, &room, &|k: &str| anchor(class_of(&state.objs[k])), None);
        ok = (connected || !base) && bad.is_empty();
    } else {
        (connected, bad) = walk_status(state, &room);
        ok = if (connected || !base) && bad.is_empty() { true } else {
            let (c0, bad0) = walk_status_f(state, &room, &|_: &str| true, Some(name));
            let cut = (c0 || !base) && !(connected || !base);
            !cut && !bad.iter().any(|k| k == name || !bad0.contains(k))
        };
    }
    if !ok && ig_core::env_str!("LAYOUTDBG").map_or(false, |v| cls.contains(v)) {
        eprintln!("LAYOUTDBG reject {cls} in {room}: doors connected {connected}, unreachable {:?}", bad.iter().map(|k| class_of(&state.objs[k]).to_string()).collect::<Vec<_>>());
    }
    ok
}

fn front_xy(state: &State, name: &str) -> Option<[f64; 2]> {
    let pl = state.planes(name, &TagSet::from([Tag::sub("Front")]));
    let &i = pl.first()?;
    let (n, _) = state.plane_nd(name, i);
    let l = (n[0] * n[0] + n[1] * n[1]).sqrt();
    if l < 1e-6 { None } else { Some([n[0] / l, n[1] / l]) }
}

fn centre(state: &State, name: &str) -> [f64; 2] {
    let ps = prisms(state, name);
    let (mut x, mut y, mut n) = (0.0, 0.0, 0.0);
    for p in ps.iter() {
        for v in &p.poly {
            x += v[0];
            y += v[1];
            n += 1.0;
        }
    }
    if n == 0.0 { let t = state.objs[name].pose.t; [t[0], t[1]] } else { [x / n, y / n] }
}

fn angle_off(f: [f64; 2], from: [f64; 2], to: [f64; 2]) -> (f64, f64) {
    let (vx, vy) = (to[0] - from[0], to[1] - from[1]);
    let d = (vx * vx + vy * vy).sqrt().max(1e-9);
    (((f[0] * vx + f[1] * vy) / d).clamp(-1.0, 1.0).acos().to_degrees(), d)
}

/// living-room grouping: sofa faces the TV stand (within 20 deg, 2-4 m) and the stand faces the sofa; armchairs turn to the coffee
/// table (or the sofa); each degree beyond the limit / metre outside the range costs (x / 30)^2 / (x / 1 m)^2, times LAYOUT_W
pub fn extra_loss(state: &State) -> f64 {
    let w: f64 = knobs::group_w();
    if w == 0.0 {
        return 0.0;
    }
    let by = |c: &str| -> Vec<&String> { state.objs.iter().filter(|(_, o)| class_of(o) == c).map(|(k, _)| k).collect() };
    let (sofas, tvs, coffees, arms) = (by("SofaFactory"), by("TVStandFactory"), by("CoffeeTableFactory"), by("ArmChairFactory"));

    let mut loss = 0.0;
    // sizes: -log density of every sized piece relative to its class's most likely size (furn::sizes), weight LAYOUT_SIZE_W
    let ws: f64 = knobs::size_w();
    for o in state.objs.values() {
        let Some(g) = &o.generator else { continue };
        let (Some((lo, hi)), Some(m)) = (g.fit_box, super::sizes::model(&g.class)) else { continue };
        loss += ws / w * super::sizes::nll(m, [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]]);
    }
    let same_room = |a: &str, b: &str| room_of(state, a).is_some() && room_of(state, a) == room_of(state, b);
    for s in &sofas {
        let Some(tv) = tvs.iter().find(|t| same_room(s, t)) else { continue };
        let (cs, ct) = (centre(state, s), centre(state, tv));
        if let Some(f) = front_xy(state, s) {
            let (a, d) = angle_off(f, cs, ct);
            loss += ((a - 20.0).max(0.0) / 30.0).powi(2) + ((2.0 - d).max(0.0) + (d - 4.0).max(0.0)).powi(2);
        }
        if let Some(f) = front_xy(state, tv) {
            let (a, _) = angle_off(f, ct, cs);
            loss += ((a - 30.0).max(0.0) / 30.0).powi(2);
        }
    }
    for a in &arms {
        let target = coffees.iter().find(|c| same_room(a, c)).or_else(|| sofas.iter().find(|s| same_room(a, s)));
        let Some(t) = target else { continue };
        let Some(f) = front_xy(state, a) else { continue };
        let (ang, d) = angle_off(f, centre(state, a), centre(state, t));
        loss += ((ang - 35.0).max(0.0) / 30.0).powi(2) + (d - 3.0).max(0.0).powi(2);
    }
    w * loss
}

/// prism of a world-space point set (free boxes)
pub fn prism_of(w: &[crate::mesh::V3]) -> Prism {
    let z0 = w.iter().map(|c| c[2]).fold(f64::INFINITY, f64::min);
    let z1 = w.iter().map(|c| c[2]).fold(f64::NEG_INFINITY, f64::max);
    let poly = hull(w.iter().map(|c| [c[0], c[1]]).collect());
    let bb = bbox(&poly);
    Prism { poly, z: [z0, z1], bb }
}

/// prisms of an object outside a State (proxy boxes / hulls under its pose)
pub fn prisms_of_obj(o: &ObjState) -> Vec<Prism> {
    use super::geom::Proxy;
    let mut out = vec![];
    for (p, _) in &o.geom.proxies {
        let pts: Vec<crate::mesh::V3> = match p {
            Proxy::Box { center, half } => {
                let mut v = vec![];
                for sx in [-1.0, 1.0] { for sy in [-1.0, 1.0] { for sz in [-1.0, 1.0] {
                    v.push(o.pose.apply([center[0] + sx * half[0], center[1] + sy * half[1], center[2] + sz * half[2]]));
                } } }
                v
            }
            Proxy::Hull(pts) => pts.iter().map(|q| o.pose.apply(*q)).collect(),
        };
        let pr = prism_of(&pts);
        if pr.poly.len() >= 3 { out.push(pr); }
    }
    out
}

pub fn anchor(cls: &str) -> bool {
    ["Bed", "Sofa", "TVStand", "KitchenSpace", "Toilet", "StandingSink", "BathroomSink", "Bathtub", "Shower", "TableDining", "Fridge", "Oven", "Chair", "Dishwasher", "Sink"].iter().any(|a| cls.starts_with(a))
}

pub fn remove_tree(state: &mut State, k: &str) -> Vec<String> {
    let mut gone = vec![k.to_string()];
    let mut i = 0;
    while i < gone.len() {
        let t = gone[i].clone();
        let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == t)).map(|(c, _)| c.clone()).collect();
        for c in kids { if !gone.contains(&c) { gone.push(c); } }
        i += 1;
    }
    for g in &gone { state.objs.shift_remove(g); }
    gone
}

/// after the post-solve moves (chairs, coffee tables): rooms whose doors are disconnected or with unreachable pieces lose their
/// smallest blocking non-anchor pieces (with whatever stands on them) until they pass or nothing helps
pub fn repair(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let rooms: Vec<String> = state.objs.iter().filter(|(_, o)| is_room(o)).map(|(k, _)| k.clone()).collect();
    for room in rooms {
        let base = base_connected(state, &room);
        let score = |st: &State| { let (c, bad) = walk_status(st, &room); (if c || !base { 0 } else { 1 }, bad.len()) };
        let mut cur = score(state);
        while cur != (0, 0) {
            // non-anchor pieces (smallest first); dining chairs never (they tuck under the table: a path past them is fine, and
            // removing them left tables with one chair)
            let pick = |chairs: bool| -> Vec<(f64, String)> {
                let mut c: Vec<(f64, String)> = state.objs.iter().filter(|(k, o)| {
                    let cls = class_of(o);
                    // (built-in features neither: a fireplace or window seat is part of the wall)
                    let ok_cls = if chairs { false } else { !anchor(cls) && !cls.starts_with("Chair") && !cls.ends_with("Feature") };
                    o.generator.is_some() && !is_room(o) && ok_cls && !no_block(cls) && cls != "Door" && cls != "Window" && floor_foot(state, k).is_some() && room_of(state, k).as_deref() == Some(room.as_str())
                }).map(|(k, _)| (prisms(state, k).iter().map(|p| (p.bb[2] - p.bb[0]) * (p.bb[3] - p.bb[1])).sum::<f64>(), k.clone())).collect();
                c.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                c
            };
            let cands = pick(false);
            // remove candidates cumulatively (smallest first) until the room improves (two pieces can close a gap together), then put
            // back every removed piece the improvement does not need
            let mut trial = State { objs: state.objs.clone(), caches: Default::default(), next_version: state.next_version };
            let mut removed: Vec<String> = vec![];
            let mut s = cur;
            for (_, k) in &cands {
                if !trial.objs.contains_key(k) { continue }
                remove_tree(&mut trial, k);
                removed.push(k.clone());
                s = score(&trial);
                if s < cur { break; }
            }
            if !(s < cur) { break }
            let mut keep_out = removed.clone();
            for k in removed.iter().rev().skip(1) {
                let mut t2 = State { objs: state.objs.clone(), caches: Default::default(), next_version: state.next_version };
                for r in keep_out.iter().filter(|r| *r != k) { if t2.objs.contains_key(r) { remove_tree(&mut t2, r); } }
                if score(&t2) <= s { keep_out.retain(|r| r != k); }
            }
            for k in &keep_out {
                if !state.objs.contains_key(k) { continue }
                let cls = class_of(&state.objs[k]).to_string();
                let gone = remove_tree(state, k);
                log.push(format!("{room}: removed {cls} ({} objects)", gone.len()));
            }
            let s = score(state);
            log.push(format!("{room}: -> doors {} unreachable {}", if s.0 == 0 { "ok" } else { "cut" }, s.1));
            if !(s < cur) { break }
            cur = s;
        }
        // last resort: the secondary anchors (a TV stand, a sofa, a dining table with its chairs), smallest first, each kept out only
        // when the room improves -- never a bed, a counter run or a bathroom fixture (narrow rooms: a sofa and TV stand across a 2.5 m
        // living room cut its doors apart)
        let mut cur = score(state);
        if cur != (0, 0) {
            // (with the other movable pieces: a room can need a shelf and a sofa out together)
            // (built-ins last of all -- alcove shelves, a bookshelf wall, a window seat, a fireplace: a fireplace and a bookshelf wall cut a
            // five-door living room in two)
            let fixed = |c: &str| ["Bed", "KitchenSpace", "KitchenIsland", "Toilet", "StandingSink", "BathroomSink", "Bathtub", "Shower", "Fridge", "Oven", "Dishwasher", "Sink", "Chair"].iter().any(|a| c.starts_with(a));
            let mut sec: Vec<(f64, String)> = state.objs.iter().filter(|(k, o)| o.generator.is_some() && !is_room(o) && !fixed(class_of(o)) && !no_block(class_of(o)) && floor_foot(state, k).is_some() && room_of(state, k).as_deref() == Some(room.as_str()))
                .map(|(k, o)| { let c = class_of(o); let last = if k.contains("alcove") { 1000.0 } else if c == "BookshelfWallFeature" { 2000.0 } else if c == "WindowSeatFeature" { 3000.0 } else if c.ends_with("Feature") { 4000.0 } else { 0.0 };
                    (last + prisms(state, k).iter().map(|p| (p.bb[2] - p.bb[0]) * (p.bb[3] - p.bb[1])).sum::<f64>(), k.clone()) }).collect();
            sec.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            // cumulatively, smallest first, until the room improves; then the ones the improvement does not need go back
            let mut trial = State { objs: state.objs.clone(), caches: Default::default(), next_version: state.next_version };
            let mut removed: Vec<String> = vec![];
            let mut s2 = cur;
            for (_, k) in &sec {
                if !trial.objs.contains_key(k) { continue }
                remove_tree(&mut trial, k);
                removed.push(k.clone());
                s2 = score(&trial);
                if s2 == (0, 0) { break }
            }
            if s2 < cur {
                let mut keep_out = removed.clone();
                for k in removed.iter().rev() {
                    let mut t2 = State { objs: state.objs.clone(), caches: Default::default(), next_version: state.next_version };
                    for r in keep_out.iter().filter(|r| *r != k) { if t2.objs.contains_key(r) { remove_tree(&mut t2, r); } }
                    if score(&t2) <= s2 { keep_out.retain(|r| r != k); }
                }
                for k in &keep_out {
                    if !state.objs.contains_key(k) { continue }
                    let gone = remove_tree(state, k);
                    log.push(format!("{room}: removed {k} and {} more (last resort)", gone.len().saturating_sub(1)));
                }
                cur = score(state);
            }
        }
        // and a bed (never removed) slid along its headboard wall, with what is related to it (nightstands, what stands on them), to the
        // nearest spot that joins the room up, inside it and clear of everything else (a bed across a narrow room cut its doors apart)
        if cur != (0, 0) {
            // (and a sofa or a TV stand the same way: a living room's only sofa is not removed for a path)
            let beds: Vec<String> = state.objs.iter().filter(|(k, o)| matches!(class_of(o), "BedFactory" | "SofaFactory" | "TVStandFactory") && room_of(state, k).as_deref() == Some(room.as_str())).map(|(k, _)| k.clone()).collect();
            'beds: for b in beds {
                let mut tree = vec![b.clone()];
                loop {
                    let more: Vec<String> = state.objs.iter().filter(|(k, o)| !tree.contains(k) && o.relations.iter().any(|r| tree.contains(&r.target))).map(|(k, _)| k.clone()).collect();
                    if more.is_empty() { break }
                    tree.extend(more);
                }
                let ax = [state.objs[&b].pose.r[0][1], state.objs[&b].pose.r[1][1]];
                let old: Vec<(String, super::geom::Pose)> = tree.iter().map(|k| (k.clone(), state.objs[k].pose)).collect();
                for step in 1..=12 { for sgn in [1.0, -1.0] {
                    let d = sgn * 0.1 * step as f64;
                    for (k, p) in &old { let mut q = *p; let t = q.t; q.set_loc([t[0] + ax[0] * d, t[1] + ax[1] * d, t[2]]); state.set_pose(k, q); state.touch(k); }
                    let clear = outside_room_depth(state, &b, &room) < 0.01 && tree.iter().all(|k| !state.objs.iter().any(|(o, ob)| !tree.contains(o) && !is_room(ob) && ((ob.generator.is_some() && floor_foot(state, o).is_some()) || o.starts_with("doorclear/") || o.contains(".keepout")) && depth(state, k, o) > 1e-4));
                    if clear && score(state) < cur {
                        log.push(format!("{room}: {b} slid {d:+.1} m along its wall (last resort)"));
                        cur = score(state);
                        if cur == (0, 0) { break 'beds }
                        continue 'beds;
                    }
                    for (k, p) in &old { state.set_pose(k, *p); state.touch(k); }
                }}
            }
        }
        let (c, bad) = walk_status(state, &room);
        if !(c || !base) || !bad.is_empty() { log.push(format!("{room}: still failing (doors connected {c}, unreachable {:?})", bad.iter().map(|k| class_of(&state.objs[k]).to_string()).collect::<Vec<_>>())); }
    }
    log
}

/// per-house state: prisms, floors and the house outline are keyed by object name and version, which repeat between houses
pub fn reset_caches() {
    // (a geometry first, so the geometry library's thread state is set up -- and torn down at exit -- before / after these maps:
    // created first, the floors map outlived it and dropping its shapes at exit aborted the process)
    let _ = shp::point(0.0, 0.0);
    PRISMS.with(|c| c.borrow_mut().clear());
    FLOORS.with(|c| c.borrow_mut().clear());
    HOUSE.with(|c| *c.borrow_mut() = None);
}

/// an object's world-space axis-aligned box (its local box through its pose)
pub fn lohi_world(state: &State, k: &str) -> ([f64; 3], [f64; 3]) {
    let o = &state.objs[k];
    let (l, h) = (o.geom.lo, o.geom.hi);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for i in 0..8 {
        let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
        for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
    }
    (lo, hi)
}
