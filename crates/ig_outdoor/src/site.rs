//! The house's site: a road past the front (a suburban street with curbs and sidewalks, a rural road with gravel shoulders, or a
//! cul-de-sac ending in a turning circle in front of the house), a driveway to the entrance, and neighbour lots along the road
//! with exterior-shell houses (procedural rectangle / L footprints, windows and a front door, src/exterior.rs) on levelled pads.
//! The terrain flattens toward the road's smoothed height profile and onto the neighbours' pads (Terrain::height asks
//! `Site::shape`); the plant community keeps off the road, driveway and lots (`Site::blocked`). Stream: int_hash(seed, "site").
use crate::exterior;
use ig_core::mesh::{Material, Mesh, Scene};
use ig_core::rng::Rng;
use ig_rooms::solidify::{CutKind, Cutter, DoorStyle};
use std::collections::HashMap;

type P2 = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout { Suburban, Rural, CulDeSac }

/// a neighbour house: its outer ring (world), wall top, ground pad height, openings and style
pub struct Neighbour { pub ring: Vec<P2>, pub wall_top: f64, pub pad_z: f64, pub floor_z: f64, pub cutters: Vec<Cutter>, pub style: exterior::Style, pub centre: P2, pub radius: f64 }

pub struct Site {
    pub layout: Layout,
    /// road centreline (x, y, z), ~4 m apart, and half its paved width
    pub road: Vec<[f64; 3]>,
    pub half: f64,
    /// turning circle (centre, radius, z) of a cul-de-sac
    pub bulb: Option<(P2, f64, f64)>,
    /// driveway centreline from the road edge to the house front, and its half width
    pub drive: Vec<P2>,
    pub drive_half: f64,
    pub neighbours: Vec<Neighbour>,
    /// the house centre and the front's outward normal (toward the road)
    pub house_c: P2,
    pub front_n: P2,
    grid: HashMap<(i32, i32), Vec<usize>>,
}

const CELL: f64 = 24.0;
fn key(p: P2) -> (i32, i32) { ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32) }

fn seg_dist(p: P2, a: P2, b: P2) -> (f64, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t), t)
}

impl Site {
    /// nearest road point: (distance to the centreline, road height there); None far from the road (> 40 m)
    pub fn road_near(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (cx, cy) = key([x, y]);
        let mut best: Option<(f64, f64)> = None;
        for gx in cx - 1..=cx + 1 { for gy in cy - 1..=cy + 1 {
            let Some(v) = self.grid.get(&(gx, gy)) else { continue };
            for &i in v {
                let (a, b) = (self.road[i], self.road[i + 1]);
                let (d, t) = seg_dist([x, y], [a[0], a[1]], [b[0], b[1]]);
                if best.map_or(true, |bb| d < bb.0) { best = Some((d, a[2] + (b[2] - a[2]) * t)); }
            }
        }}
        if let Some((c, rad, z)) = self.bulb {
            let d = ((x - c[0]).hypot(y - c[1]) - rad + self.half).max(0.0);
            if best.map_or(true, |bb| d < bb.0) { best = Some((d, z)); }
        }
        best.filter(|b| b.0 < 40.0)
    }

    /// the site's say on the terrain at (x, y): blends `h` toward the neighbours' pads and the road's profile
    pub fn shape(&self, x: f64, y: f64, h: f64) -> f64 {
        let mut h = h;
        for nb in &self.neighbours {
            let d = ((x - nb.centre[0]).hypot(y - nb.centre[1]) - nb.radius).max(0.0);
            if d < 14.0 { let b = smooth(2.0, 14.0, d); h = nb.pad_z * (1.0 - b) + h * b; }
        }
        if let Some((d, rz)) = self.road_near(x, y) {
            let w = 1.0 - smooth(self.half + 1.5, self.half + 9.0, d);
            h = h * (1.0 - w) + (rz - 0.02) * w;
        }
        h
    }

    /// the sidecar's `outdoor.site`: the layout and a street camera -- on the far side of the road (the sidewalk), 10 m along
    /// from the driveway, eye height, looking at the house (tools/render/render_rust.py --view street)
    pub fn record(&self, house_z: f64) -> serde_json::Value {
        // street camera: on the house's side of the road (where the driveway meets it), 6 m along, eye height, aimed at the house
        // a little above its eaves (from across the road the street sat ~1.5 m below the house and the roof was a thin band)
        let e = self.drive.last().copied().unwrap_or(self.house_c);
        let along = [-self.front_n[1], self.front_n[0]];
        let eye = [e[0] - self.front_n[0] * 1.0 + along[0] * 6.0, e[1] - self.front_n[1] * 1.0 + along[1] * 6.0];
        let ez = self.road_near(eye[0], eye[1]).map_or(house_z, |r| r.1) + 1.7;
        let mut rec = serde_json::json!({"layout": format!("{:?}", self.layout), "neighbours": self.neighbours.len(),
            "street_cam": {"eye": [eye[0], eye[1], ez], "target": [self.house_c[0], self.house_c[1], house_z + 2.2]}});
        // neighbour camera: from the road in front of the neighbour nearest the house, aimed at it
        if let Some(nb) = self.neighbours.iter().min_by(|a, b| {
            let da = (a.centre[0] - self.house_c[0]).hypot(a.centre[1] - self.house_c[1]);
            let db = (b.centre[0] - self.house_c[0]).hypot(b.centre[1] - self.house_c[1]);
            da.partial_cmp(&db).unwrap()
        }) {
            // the road point nearest the neighbour, then 3 m toward it and 5 m along
            let mut best = (f64::MAX, [0.0, 0.0, 0.0]);
            for p in &self.road { let d = (p[0] - nb.centre[0]).hypot(p[1] - nb.centre[1]); if d < best.0 { best = (d, *p); } }
            let p = best.1;
            let to = [(nb.centre[0] - p[0]) / best.0.max(1e-6), (nb.centre[1] - p[1]) / best.0.max(1e-6)];
            let eye = [p[0] + to[0] * (self.half + 1.0) + to[1] * 5.0, p[1] + to[1] * (self.half + 1.0) - to[0] * 5.0];
            rec["neighbour_cam"] = serde_json::json!({"eye": [eye[0], eye[1], p[2] + 1.7], "target": [nb.centre[0], nb.centre[1], nb.pad_z + 2.4]});
        }
        rec
    }

    /// nothing grows here: the road and its verges, the driveway, the neighbours' houses and `yard` m around them (a tree's
    /// crown reaches ~7 m: trees ask with ~9 m, shrubs with ~2.5 m)
    pub fn blocked(&self, x: f64, y: f64, yard: f64) -> bool {
        if let Some((d, _)) = self.road_near(x, y) { if d < self.half + 3.5 { return true } }
        for w in self.drive.windows(2) { if seg_dist([x, y], w[0], w[1]).0 < self.drive_half + 1.5 { return true } }
        self.neighbours.iter().any(|nb| (x - nb.centre[0]).hypot(y - nb.centre[1]) < nb.radius + yard)
    }
}

fn smooth(a: f64, b: f64, x: f64) -> f64 { let t = ((x - a) / (b - a)).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) }

/// Plan the site. `ring`: the house's outer face; `cutters`: its openings (the entrance picks the front); `nat(x, y)` the terrain
/// height before the site shapes it; `pad_z` the house's pad.
pub fn plan(seed: u64, ring: &[P2], cutters: &[Cutter], wall_t: f64, nat: &dyn Fn(f64, f64) -> f64, pad_z: f64, layout: Option<Layout>) -> Site {
    let mut r = Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "site"]));
    let layout = layout.unwrap_or_else(|| { let u = r.f64(); if u < 0.5 { Layout::Suburban } else if u < 0.8 { Layout::Rural } else { Layout::CulDeSac } });
    let n = ring.len();
    let area2: f64 = (0..n).map(|k| { let (a, b) = (ring[k], ring[(k + 1) % n]); a[0] * b[1] - b[0] * a[1] }).sum();
    let outward = |u: P2| if area2 > 0.0 { [u[1], -u[0]] } else { [-u[1], u[0]] };
    // front: the facade with the entrance (else the longest), its outward normal and the entrance point
    let mut front: Option<(P2, P2, P2)> = None; // (unit along, outward normal, door point)
    let mut longest = (0.0, [1.0, 0.0], [0.0, -1.0], [0.0, 0.0]);
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        if l < 0.5 { continue }
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let nn = outward(u);
        if l > longest.0 { longest = (l, u, nn, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]); }
        for c in cutters.iter().filter(|c| c.kind == CutKind::Entrance) {
            let rel = [c.loc[0] - a[0], c.loc[1] - a[1]];
            let (al, off) = (rel[0] * u[0] + rel[1] * u[1], rel[0] * nn[0] + rel[1] * nn[1]);
            if al >= 0.0 && al <= l && off < 0.05 && off > -wall_t * 1.2 { front = Some((u, nn, [c.loc[0] + nn[0] * wall_t, c.loc[1] + nn[1] * wall_t])); }
        }
    }
    let (u, nn, door) = front.unwrap_or((longest.1, longest.2, longest.3));
    // the house's extent along the front normal: the road clears its front face
    let front_off = ring.iter().map(|p| (p[0] - door[0]) * nn[0] + (p[1] - door[1]) * nn[1]).fold(f64::MIN, f64::max);
    let (half, setback) = match layout { Layout::Suburban => (r.uniform(3.2, 4.0), r.uniform(9.0, 14.0)), Layout::Rural => (r.uniform(2.6, 3.1), r.uniform(22.0, 45.0)), Layout::CulDeSac => (r.uniform(3.2, 3.8), r.uniform(10.0, 13.0)) };
    let base = [door[0] + nn[0] * (front_off + setback + half), door[1] + nn[1] * (front_off + setback + half)];
    let (amp, wl, ph) = (r.uniform(0.0, 9.0), r.uniform(70.0, 160.0), r.uniform(0.0, 6.28));
    let mut pts: Vec<P2> = vec![];
    let mut bulb = None;
    let len = 700.0;
    match layout {
        Layout::CulDeSac => {
            // from the turning circle in front of the house straight away from it, curving further out
            let rad = r.uniform(10.0, 13.0);
            let c = [base[0] + nn[0] * (rad - half), base[1] + nn[1] * (rad - half)];
            bulb = Some((c, rad, 0.0));
            let mut s = rad - 1.0;
            while s < len { let off = amp * smooth(30.0, 90.0, s) * (s / wl + ph).sin(); pts.push([c[0] + nn[0] * s + u[0] * off, c[1] + nn[1] * s + u[1] * off]); s += 4.0; }
        }
        _ => {
            let mut s = -len;
            while s <= len { let off = amp * smooth(35.0, 100.0, s.abs()) * (s / wl + ph).sin(); pts.push([base[0] + u[0] * s + nn[0] * off, base[1] + u[1] * s + nn[1] * off]); s += 4.0; }
        }
    }
    // height profile: the terrain along the centreline, smoothed over +-24 m; the stretch in front of the house no higher
    // than ~0.6 m below the pad (the driveway climbs to the house)
    let raw: Vec<f64> = pts.iter().map(|p| nat(p[0], p[1])).collect();
    let road: Vec<[f64; 3]> = (0..pts.len()).map(|i| {
        let (lo, hi) = (i.saturating_sub(6), (i + 6).min(pts.len() - 1));
        let z = raw[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64;
        let near = smooth(20.0, 60.0, (pts[i][0] - base[0]).hypot(pts[i][1] - base[1]));
        [pts[i][0], pts[i][1], z.min(pad_z - 0.3) * (1.0 - near) + z * near]
    }).collect();
    if let Some((c, rad, _)) = bulb { bulb = Some((c, rad, road[0][2])); }
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for i in 0..road.len() - 1 {
        let (a, b) = ([road[i][0], road[i][1]], [road[i + 1][0], road[i + 1][1]]);
        let (k0, k1) = (key([a[0].min(b[0]) - 40.0, a[1].min(b[1]) - 40.0]), key([a[0].max(b[0]) + 40.0, a[1].max(b[1]) + 40.0]));
        for gx in k0.0..=k1.0 { for gy in k0.1..=k1.1 { grid.entry((gx, gy)).or_default().push(i); } }
    }
    let drive_half = if layout == Layout::Rural { 1.6 } else { 1.7 };
    let drive_end = [door[0] + nn[0] * (front_off + setback + half - half), door[1] + nn[1] * (front_off + setback + half - half)];
    let drive = vec![[door[0] + nn[0] * 0.8, door[1] + nn[1] * 0.8], drive_end];
    let hc0 = ring.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n as f64, a[1] + p[1] / n as f64]);
    let mut site = Site { layout, road, half, bulb, drive, drive_half, neighbours: vec![], house_c: hc0, front_n: nn, grid };
    // neighbour lots: along both sides of the road (suburban), a few far apart (rural), around the turning circle (cul-de-sac)
    let mut spots: Vec<(P2, P2)> = vec![]; // (house centre, unit vector toward the road)
    match layout {
        Layout::Suburban => {
            let gap = r.uniform(24.0, 34.0);
            for side in [-1.0f64, 1.0] {
                for k in -5i32..=5 {
                    let s = k as f64 * gap + if side > 0.0 { gap * 0.5 } else { 0.0 };
                    if side < 0.0 && k == 0 { continue } // the house's own lot
                    let i = ((s + len) / 4.0).round().clamp(0.0, (site.road.len() - 1) as f64) as usize;
                    let p = [site.road[i][0], site.road[i][1]];
                    let back = r.uniform(14.0, 19.0);
                    let dir = if side < 0.0 { [-nn[0], -nn[1]] } else { nn };
                    spots.push(([p[0] + dir[0] * (half + back), p[1] + dir[1] * (half + back)], [-dir[0], -dir[1]]));
                }
            }
        }
        Layout::Rural => {
            for _ in 0..r.randint(2, 5) {
                let s = r.uniform(70.0, 350.0) * if r.f64() < 0.5 { -1.0 } else { 1.0 };
                let side = if r.f64() < 0.5 { -1.0 } else { 1.0 };
                let i = ((s + len) / 4.0).round().clamp(0.0, (site.road.len() - 1) as f64) as usize;
                let p = [site.road[i][0], site.road[i][1]];
                let back = r.uniform(15.0, 40.0);
                let dir = if side < 0.0 { [-nn[0], -nn[1]] } else { nn };
                spots.push(([p[0] + dir[0] * (half + back), p[1] + dir[1] * (half + back)], [-dir[0], -dir[1]]));
            }
        }
        Layout::CulDeSac => {
            let (c, rad, _) = site.bulb.unwrap();
            let k = r.randint(3, 6) as usize;
            for j in 0..k {
                // around the far side of the circle and along the stem, away from the house
                let a = std::f64::consts::PI * (0.25 + 1.5 * (j as f64 + 0.5) / k as f64);
                let dirn = [nn[0] * a.cos() * -1.0 + u[0] * a.sin(), nn[1] * a.cos() * -1.0 + u[1] * a.sin()];
                let dist = rad + r.uniform(16.0, 22.0);
                spots.push(([c[0] + dirn[0] * dist, c[1] + dirn[1] * dist], [-dirn[0], -dirn[1]]));
            }
            for k in 1..4 {
                for side in [-1.0f64, 1.0] {
                    let i = (k * 8).min(site.road.len() - 1);
                    let p = [site.road[i][0], site.road[i][1]];
                    let dir = [u[0] * side, u[1] * side];
                    spots.push(([p[0] + dir[0] * (half + 16.0), p[1] + dir[1] * (half + 16.0)], [-dir[0], -dir[1]]));
                }
            }
        }
    }
    let hc = ring.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n as f64, a[1] + p[1] / n as f64]);
    for (c, to_road) in spots {
        if (c[0] - hc[0]).hypot(c[1] - hc[1]) < 26.0 { continue }
        if site.neighbours.iter().any(|o| (o.centre[0] - c[0]).hypot(o.centre[1] - c[1]) < o.radius + 12.0) { continue }
        let nb = neighbour(&mut r, c, to_road, nat(c[0], c[1]).max(site.road_near(c[0], c[1]).map_or(f64::MIN, |x| x.1 + 0.2)));
        if site.road_near(c[0], c[1]).map_or(false, |(d, _)| d < site.half + nb.radius + 1.0) { continue }
        site.neighbours.push(nb);
    }
    site
}

/// a neighbour's shell: a rectangle with an optional wing, 1-2 storeys, windows on every facade, a door facing the road
fn neighbour(r: &mut Rng, c: P2, to_road: P2, ground: f64) -> Neighbour {
    let (w, d) = (r.uniform(9.0, 15.0), r.uniform(7.5, 11.0));
    // local frame: +y toward the road (front at y = d/2), x along the front
    let mut rects = vec![[-w / 2.0, -d / 2.0, w / 2.0, d / 2.0]];
    if r.f64() < 0.55 {
        let (ww, wd) = (r.uniform(4.0, 7.0), r.uniform(4.0, 7.0));
        let side = if r.f64() < 0.5 { -1.0 } else { 1.0 };
        rects.push(if r.f64() < 0.5 { [side * w / 2.0 - if side > 0.0 { ww } else { 0.0 }, -d / 2.0 - wd, side * w / 2.0 + if side > 0.0 { 0.0 } else { ww }, -d / 2.0 + 0.01] }
            else { let x0 = if side > 0.0 { w / 2.0 - 0.01 } else { -w / 2.0 - ww }; [x0, -d / 2.0, x0 + ww + 0.01, -d / 2.0 + wd] });
    }
    let polys: Vec<ig_core::shp::G> = rects.iter().map(|q| ig_core::shp::polygon(&[[q[0], q[1]], [q[2], q[1]], [q[2], q[3]], [q[0], q[3]], [q[0], q[1]]], &[]).unwrap()).collect();
    let fp = ig_core::shp::simplify(&ig_core::shp::union_all(&polys), 0.05);
    let local = ig_core::shp::exterior(&fp);
    // rotation: local +y -> to_road
    let (cs, sn) = (to_road[1], -to_road[0]); // rotate local by angle a with (cos a, sin a) such that R*(0,1) = to_road
    let rot = |p: P2| [c[0] + cs * p[0] - sn * p[1], c[1] + sn * p[0] + cs * p[1]];
    let ring: Vec<P2> = local.iter().map(|p| rot(*p)).collect();
    let storeys = if r.f64() < 0.35 { 2 } else { 1 };
    let floor_z = ground + 0.45;
    let wall_top = floor_z + 2.75 * storeys as f64;
    let wall_t = 0.25;
    let mut cutters = vec![];
    let n = local.len();
    let area2: f64 = (0..n).map(|k| { let (a, b) = (local[k], local[(k + 1) % n]); a[0] * b[1] - b[0] * a[1] }).sum();
    let mut door_done = false;
    for k in 0..n {
        let (a, b) = (local[k], local[(k + 1) % n]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        if l < 2.0 { continue }
        let uu = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let on = if area2 > 0.0 { [uu[1], -uu[0]] } else { [-uu[1], uu[0]] };
        let front = on[1] > 0.9;
        let nwin = ((l - 1.0) / 3.2).floor().max(1.0) as usize;
        for st in 0..storeys {
            for j in 0..nwin {
                let al = l * (j as f64 + 0.5) / nwin as f64;
                let is_door = front && st == 0 && !door_done && j == nwin / 2;
                let (ww, hh, zc) = if is_door { (1.0, 2.1, floor_z + 1.05) } else { (r.uniform(0.9, 1.4), 1.35, floor_z + 2.75 * st as f64 + 0.9 + 0.675) };
                if is_door { door_done = true; }
                let pl = [a[0] + uu[0] * al - on[0] * wall_t / 2.0, a[1] + uu[1] * al - on[1] * wall_t / 2.0];
                let pw = rot(pl);
                let ang = (sn * uu[0] + cs * uu[1]).atan2(cs * uu[0] - sn * uu[1]);
                cutters.push(Cutter { kind: if is_door { CutKind::Entrance } else { CutKind::Window }, style: DoorStyle::Swing, loc: [pw[0], pw[1], zc], rot_z: ang, dims: [ww, wall_t * 2.0, hh], rooms: vec![] });
            }
        }
    }
    let radius = local.iter().map(|p| p[0].hypot(p[1])).fold(0.0, f64::max);
    Neighbour { ring, wall_top, pad_z: ground, floor_z, cutters, style: exterior::Style::draw(r), centre: c, radius }
}

/// a strip along a polyline at lateral offsets [o0, o1] from it, `z(i)` the height at point i
fn strip(m: &mut Mesh, pts: &[P2], o0: f64, o1: f64, z: &dyn Fn(usize, P2) -> f64) {
    for i in 0..pts.len().saturating_sub(1) {
        let nrm = |k: usize| {
            let (a, b) = (pts[k.saturating_sub(1).min(pts.len() - 2)], pts[(k + 1).min(pts.len() - 1).max(1)]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]); let l = dx.hypot(dy).max(1e-9); [-dy / l, dx / l]
        };
        let (na, nb) = (nrm(i), nrm(i + 1));
        let p = |k: usize, nv: P2, o: f64| { let q = [pts[k][0] + nv[0] * o, pts[k][1] + nv[1] * o]; [q[0], q[1], z(k, q)] };
        m.add_quad(p(i, na, o0), p(i + 1, nb, o0), p(i + 1, nb, o1), p(i, na, o1));
    }
}

/// Add the road, sidewalks, curbs, markings, driveway and the neighbours' houses. `ground(x, y)` the shaped terrain height.
pub fn add_meshes(scene: &mut Scene, site: &Site, ground: &dyn Fn(f64, f64) -> f64, seed: u64) -> usize {
    let pts: Vec<P2> = site.road.iter().map(|p| [p[0], p[1]]).collect();
    let rz = |i: usize, _q: P2| site.road[i][2];
    let (mut asphalt, mut concrete, mut paint, mut gravel) = (Mesh::new(), Mesh::new(), Mesh::new(), Mesh::new());
    let h = site.half;
    strip(&mut asphalt, &pts, -h, h, &|i, q| rz(i, q) + 0.03);
    if let Some((c, rad, z)) = site.bulb {
        for k in 0..48 {
            let (a0, a1) = (std::f64::consts::TAU * k as f64 / 48.0, std::f64::consts::TAU * (k + 1) as f64 / 48.0);
            asphalt.add_tri([c[0], c[1], z + 0.03], [c[0] + rad * a0.cos(), c[1] + rad * a0.sin(), z + 0.03], [c[0] + rad * a1.cos(), c[1] + rad * a1.sin(), z + 0.03]);
        }
    }
    match site.layout {
        Layout::Rural => {
            strip(&mut gravel, &pts, h, h + 1.0, &|i, q| rz(i, q) + 0.01);
            strip(&mut gravel, &pts, -h - 1.0, -h, &|i, q| rz(i, q) + 0.01);
            // centre dashes (3 m on, 9 m off) and edge lines
            let mut k = 0;
            while k + 1 < pts.len() { strip(&mut paint, &pts[k..k + 2], -0.06, 0.06, &|i, q| rz(k + i, q) + 0.035); k += 3; }
            strip(&mut paint, &pts, h - 0.25, h - 0.15, &|i, q| rz(i, q) + 0.035);
            strip(&mut paint, &pts, -h + 0.15, -h + 0.25, &|i, q| rz(i, q) + 0.035);
        }
        _ => {
            // curbs (0.15 m), a grass verge, 1.5 m sidewalks
            for s in [-1.0f64, 1.0] {
                strip(&mut concrete, &pts, s * h, s * (h + 0.15), &|i, q| rz(i, q) + 0.15);
                strip(&mut concrete, &pts, s * h, s * h, &|i, q| rz(i, q) + 0.03);
                strip(&mut concrete, &pts, s * (h + 2.0), s * (h + 3.5), &|i, q| rz(i, q) + 0.1);
            }
        }
    }
    // driveway: a concrete slab on the ground from the house to the road
    if site.drive.len() == 2 {
        let (a, b) = (site.drive[0], site.drive[1]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        let nseg = (l / 1.5).ceil().max(1.0) as usize;
        let dp: Vec<P2> = (0..=nseg).map(|k| { let t = k as f64 / nseg as f64; [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] }).collect();
        strip(&mut concrete, &dp, -site.drive_half, site.drive_half, &|_, q| ground(q[0], q[1]) + 0.05);
    }
    let mut n = 0;
    let am = scene.material(Material::diffuse("site_asphalt", [0.045, 0.045, 0.047], 0.92));
    let cm = scene.material(Material::diffuse("site_concrete", [0.42, 0.41, 0.39], 0.9));
    let pm = scene.material(Material::diffuse("site_road_paint", [0.75, 0.62, 0.15], 0.7));
    let gm = scene.material(Material::diffuse("site_gravel", [0.30, 0.28, 0.25], 1.0));
    for (nm, m, mi) in [("asphalt", asphalt, am), ("concrete", concrete, cm), ("paint", paint, pm), ("gravel", gravel, gm)] {
        if m.is_empty() { continue }
        n += m.faces.len();
        scene.add(&format!("outdoor_road.{nm}"), "OutdoorTerrain", m, mi);
    }
    for (k, nb) in site.neighbours.iter().enumerate() {
        n += exterior::add_house(scene, &format!("nb{k}"), &nb.ring, &nb.cutters, 0.25, nb.floor_z, nb.wall_top, nb.pad_z, &nb.style, seed ^ (k as u64 * 7919), true);
    }
    n
}
