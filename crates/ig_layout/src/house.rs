//! Rooms from a `*.house.json` (what gen2 writes next to a glb).
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct Cutter { pub kind: String, pub loc: [f64; 3], pub rot_z: f64, pub dims: [f64; 3], pub rooms: Vec<String> }

#[derive(Deserialize)]
struct HouseFile { wall_thickness: f64, wall_height: f64, rooms: Vec<(String, String)>, cutters: Vec<Cutter> }

/// a wall face inside the room: `a`-`b` on the inner face, `n` the unit normal into the room
#[derive(Clone, Debug, serde::Serialize)]
pub struct Wall { pub a: [f64; 2], pub b: [f64; 2], pub n: [f64; 2], pub len: f64 }

/// an opening seen from this room: its centre on the wall, along-the-wall half width, unit normal into the room
#[derive(Clone, Debug, serde::Serialize)]
pub struct Opening { pub at: [f64; 2], pub u: [f64; 2], pub n: [f64; 2], pub hw: f64, pub z: [f64; 2], pub kind: String }

#[derive(Clone, Debug, serde::Serialize)]
pub struct Room { pub name: String, pub kind: String, pub outline: Vec<[f64; 2]>, pub wall_t: f64, pub height: f64, pub area: f64, pub walls: Vec<Wall>, pub doors: Vec<Opening>, pub windows: Vec<Opening> }

/// the first ring of a WKT polygon, without its closing point
fn ring(wkt: &str) -> Vec<[f64; 2]> {
    let s = wkt.find('(').map_or("", |i| &wkt[i..]);
    let s = s.trim_start_matches(|c| c == '(' || c == ' ');
    let s = s.split(')').next().unwrap_or("");
    let mut r: Vec<[f64; 2]> = s.split(',').filter_map(|p| { let mut t = p.split_whitespace().map(|x| x.parse::<f64>().ok()); Some([t.next()??, t.next()??]) }).collect();
    if r.len() > 1 && r[0] == r[r.len() - 1] { r.pop(); }
    r
}

pub fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) { c = !c; }
    }
    c
}

/// distance from `p` to the ring's edges
pub fn edge_dist(ring: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let mut m = f64::MAX;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
        m = m.min((a[0] + dx * t - p[0]).hypot(a[1] + dy * t - p[1]));
    }
    m
}

pub fn load(path: &str) -> Vec<Room> {
    let h: HouseFile = serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| panic!("{path}: {e}"));
    h.rooms.iter().map(|(name, wkt)| room_from(name, ring(wkt), &h.cutters, h.wall_thickness, h.wall_height)).collect()
}

/// one room from the plan's outline and openings
pub fn room_from(name: &str, outline: Vec<[f64; 2]>, cutters: &[Cutter], wall_thickness: f64, wall_height: f64) -> Room {
    {
        struct H<'a> { wall_thickness: f64, wall_height: f64, cutters: &'a [Cutter] }
        let h = H { wall_thickness, wall_height, cutters };
        let area = (0..outline.len()).map(|i| { let (a, b) = (outline[i], outline[(i + 1) % outline.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f64>().abs() / 2.0;
        let half = h.wall_thickness / 2.0;
        let mut walls = vec![];
        for i in 0..outline.len() {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if len < 0.3 { continue }
            let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            let mut n = [-u[1], u[0]];
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            if !inside(&outline, [m[0] + n[0] * 0.02, m[1] + n[1] * 0.02]) { n = [-n[0], -n[1]]; }
            // (the face is the outline moved in half a wall; its ends in by the same, the neighbouring walls' faces at a square corner)
            let (a2, b2) = ([a[0] + n[0] * half + u[0] * half, a[1] + n[1] * half + u[1] * half], [b[0] + n[0] * half - u[0] * half, b[1] + n[1] * half - u[1] * half]);
            walls.push(Wall { a: a2, b: b2, n, len: (len - 2.0 * half).max(0.0) });
        }
        let (mut doors, mut windows) = (vec![], vec![]);
        for c in h.cutters.iter().filter(|c| c.rooms.iter().any(|r| r == name)) {
            let u = [c.rot_z.cos(), c.rot_z.sin()];
            let mut n = [-u[1], u[0]];
            if !inside(&outline, [c.loc[0] + n[0] * (half + 0.05), c.loc[1] + n[1] * (half + 0.05)]) { n = [-n[0], -n[1]]; }
            let o = Opening { at: [c.loc[0] + n[0] * half, c.loc[1] + n[1] * half], u, n, hw: c.dims[0] / 2.0, z: [c.loc[2] - c.dims[2] / 2.0, c.loc[2] + c.dims[2] / 2.0], kind: c.kind.clone() };
            if c.kind == "Window" { windows.push(o) } else { doors.push(o) }
        }
        let kind = name.split('_').next().unwrap_or("").to_string();
        Room { name: name.to_string(), kind, outline, wall_t: h.wall_thickness, height: h.wall_height, area, walls, doors, windows }
    }
}
