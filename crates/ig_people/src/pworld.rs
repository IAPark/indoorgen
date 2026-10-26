//! World model for people placement (owner: people-place worker): furniture colliders (parry trimeshes from the visual meshes), room
//! polygons, seat / mattress analysis, and the collision tests of a posed body (a few oriented boxes) against furniture, walls and other people.
use ig_core::mesh::V3;
use ig_furn::state::State;
use parry3d_f64::na::{Isometry3, Point3, Translation3, UnitQuaternion, Vector3};
use parry3d_f64::query;
use parry3d_f64::shape::{Cuboid, TriMesh};
use std::cell::OnceCell;

pub type Tri = [V3; 3];

pub struct Obj {
    pub key: String,
    pub class: String,
    pub pose: ig_furn::geom::Pose,
    pub tris: Vec<Tri>,
    /// triangle index ranges of the bed cover parts (cloth jobs that collide with other parts: comforter, blanket)
    pub cover: Vec<(usize, usize)>,
    /// (first tri, end tri, part index) of the parts that rest on the cloth (pillows, towels: cloth followers)
    pub pillows: Vec<(usize, usize, usize)>,
    /// world AABB of the triangles
    pub lo: V3,
    pub hi: V3,
    pub room: usize,
    tm: OnceCell<Option<TriMesh>>,
}

impl Obj {
    /// local point -> world
    pub fn w(&self, p: V3) -> V3 {
        self.pose.apply(p)
    }
    /// world direction (unit, xy) of a local xy direction (x = front, y = side)
    pub fn dir(&self, local: [f64; 2]) -> [f64; 2] {
        let v = self.pose.rot([local[0], local[1], 0.0]);
        let l = v[0].hypot(v[1]).max(1e-9);
        [v[0] / l, v[1] / l]
    }
    pub fn trimesh(&self) -> Option<&TriMesh> {
        self.tm
            .get_or_init(|| {
                if self.tris.is_empty() {
                    return None;
                }
                let mut v = Vec::with_capacity(self.tris.len() * 3);
                let mut ix = Vec::with_capacity(self.tris.len());
                for (i, t) in self.tris.iter().enumerate() {
                    for p in t {
                        v.push(Point3::new(p[0], p[1], p[2]));
                    }
                    ix.push([3 * i as u32, 3 * i as u32 + 1, 3 * i as u32 + 2]);
                }
                TriMesh::new(v, ix).ok()
            })
            .as_ref()
    }
    /// the triangles without the cover parts
    pub fn tris_nocover(&self) -> Vec<Tri> {
        if self.cover.is_empty() {
            return self.tris.clone();
        }
        self.tris.iter().enumerate().filter(|(i, _)| !self.cover.iter().any(|&(a, b)| *i >= a && *i < b)).map(|(_, t)| *t).collect()
    }
    /// triangles with the cover parts optionally dropped and pillow parts translated by `moves` (part index, world offset)
    pub fn tris_with(&self, skip_cover: bool, moves: &[(usize, V3)]) -> Vec<Tri> {
        let mut out = Vec::with_capacity(self.tris.len());
        for (i, t) in self.tris.iter().enumerate() {
            if skip_cover && self.cover.iter().any(|&(a, b)| i >= a && i < b) {
                continue;
            }
            let mv = self.pillows.iter().find(|&&(a, b, _)| i >= a && i < b).and_then(|&(_, _, pi)| moves.iter().find(|m| m.0 == pi));
            match mv {
                Some((_, d)) => out.push([0, 1, 2].map(|k| [t[k][0] + d[0], t[k][1] + d[1], t[k][2] + d[2]])),
                None => out.push(*t),
            }
        }
        out
    }
    /// world xy bounding box (lo, hi) and z range of one pillow part
    pub fn part_bounds(&self, a: usize, b: usize) -> (V3, V3) {
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for t in &self.tris[a..b] {
            for q in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
        (lo, hi)
    }
    /// highest crossing of the vertical line at world (x, y) that is <= zmax, ignoring the cover parts (the mattress / pillows top)
    pub fn top_xy_nocover(&self, x: f64, y: f64, zmax: f64) -> Option<f64> {
        let o = Obj { key: String::new(), class: String::new(), pose: self.pose, tris: self.tris_nocover(), cover: vec![], pillows: vec![], lo: self.lo, hi: self.hi, room: self.room, tm: OnceCell::new() };
        o.column(x, y).into_iter().filter(|z| *z <= zmax).last()
    }
    /// z values where the vertical line (x, y) crosses the triangles, ascending
    pub fn column(&self, x: f64, y: f64) -> Vec<f64> {
        let mut zs = vec![];
        for t in &self.tris {
            let (a, b, c) = (t[0], t[1], t[2]);
            if x < a[0].min(b[0]).min(c[0]) || x > a[0].max(b[0]).max(c[0]) || y < a[1].min(b[1]).min(c[1]) || y > a[1].max(b[1]).max(c[1]) {
                continue;
            }
            let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if d.abs() < 1e-14 {
                continue;
            }
            let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
            let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9 {
                zs.push(l1 * a[2] + l2 * b[2] + l3 * c[2]);
            }
        }
        zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        zs
    }
    /// highest crossing of the vertical line at local (lx, ly) that is <= zmax (world z)
    pub fn top_at(&self, lx: f64, ly: f64, zmax: f64) -> Option<f64> {
        let w = self.w([lx, ly, 0.0]);
        self.column(w[0], w[1]).into_iter().filter(|z| *z <= zmax).last()
    }
}

pub struct Room {
    pub name: String,
    pub ty: String,
    pub poly: Vec<[f64; 2]>,
    pub bbox: [f64; 4],
    pub doors: Vec<[f64; 2]>,
    pub n_furn: usize,
}

pub struct World {
    pub objs: Vec<Obj>,
    pub rooms: Vec<Room>,
    pub floor_z: f64,
    /// half wall thickness: the walkable area is the room polygon inset by this
    pub wall_in: f64,
    /// extra keep-out boxes (door swing areas)
    pub extra: Vec<WBox>,
}

pub fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
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

pub fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

pub fn edge_dist(p: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| seg_dist(p, poly[i], poly[(i + 1) % n])).fold(f64::MAX, f64::min)
}

/// distance along the ray (origin o, unit dir d) to the first crossing of the polygon boundary
pub fn ray_poly(o: [f64; 2], d: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let mut best = f64::MAX;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let den = d[0] * e[1] - d[1] * e[0];
        if den.abs() < 1e-12 {
            continue;
        }
        let ao = [a[0] - o[0], a[1] - o[1]];
        let t = (ao[0] * e[1] - ao[1] * e[0]) / den;
        let u = (ao[0] * d[1] - ao[1] * d[0]) / den;
        if t > 0.0 && (0.0..=1.0).contains(&u) {
            best = best.min(t);
        }
    }
    best
}

/// distance along the ray to a 2D AABB (slab test), None when missed
pub fn ray_aabb(o: [f64; 2], d: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::MAX);
    for k in 0..2 {
        if d[k].abs() < 1e-12 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
        } else {
            let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    if t0 <= t1 {
        Some(t0)
    } else {
        None
    }
}

/// signed distance to the polygon boundary: > 0 inside
pub fn inside_dist(p: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let d = edge_dist(p, poly);
    if in_poly(p, poly) {
        d
    } else {
        -d
    }
}

/// world oriented box: centre, half extents, z rotation
#[derive(Clone, Copy, Debug)]
pub struct WBox {
    pub c: V3,
    pub h: V3,
    pub yaw: f64,
}

impl WBox {
    pub fn iso(&self) -> Isometry3<f64> {
        Isometry3::from_parts(Translation3::new(self.c[0], self.c[1], self.c[2]), UnitQuaternion::from_axis_angle(&Vector3::z_axis(), self.yaw))
    }
    pub fn cuboid(&self) -> Cuboid {
        Cuboid::new(Vector3::new(self.h[0], self.h[1], self.h[2]))
    }
    pub fn corners_xy(&self) -> [[f64; 2]; 4] {
        let (s, c) = self.yaw.sin_cos();
        let mut out = [[0.0; 2]; 4];
        for (i, (sx, sy)) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().enumerate() {
            let (lx, ly) = (sx * self.h[0], sy * self.h[1]);
            out[i] = [self.c[0] + c * lx - s * ly, self.c[1] + s * lx + c * ly];
        }
        out
    }
    pub fn aabb(&self) -> (V3, V3) {
        let cs = self.corners_xy();
        let mut lo = [f64::MAX, f64::MAX, self.c[2] - self.h[2]];
        let mut hi = [f64::MIN, f64::MIN, self.c[2] + self.h[2]];
        for p in cs {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
}

/// classes that never block a person
fn ignored(class: &str) -> bool {
    class.contains("CeilingLight") || class.contains("Rug") || class.contains("WallArt") || class.contains("Curtain")
}

impl World {
    pub fn new(state: &State, room_polys: &[(String, ig_core::shp::G)], visuals: Option<&std::collections::HashMap<String, ig_kit::api::Asset>>, floor_z: f64, wall_t: f64) -> World {
        let mut rooms: Vec<Room> = vec![];
        for (name, g) in room_polys {
            let poly: Vec<[f64; 2]> = ig_core::shp::exterior(g);
            if poly.len() < 3 {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in &poly {
                x0 = x0.min(p[0]);
                y0 = y0.min(p[1]);
                x1 = x1.max(p[0]);
                y1 = y1.max(p[1]);
            }
            let ty = name.split('_').next().unwrap_or("").to_string();
            rooms.push(Room { name: name.clone(), ty, poly, bbox: [x0, y0, x1, y1], doors: vec![], n_furn: 0 });
        }
        let room_of = |xy: [f64; 2], rooms: &[Room]| -> usize {
            if let Some(i) = rooms.iter().position(|r| in_poly(xy, &r.poly)) {
                return i;
            }
            (0..rooms.len()).min_by(|&a, &b| edge_dist(xy, &rooms[a].poly).partial_cmp(&edge_dist(xy, &rooms[b].poly)).unwrap()).unwrap_or(0)
        };
        let mut objs = vec![];
        if rooms.is_empty() {
            return World { objs, rooms, floor_z, wall_in: wall_t / 2.0, extra: vec![] };
        }
        for (k, o) in &state.objs {
            let xy = [o.pose.t[0], o.pose.t[1]];
            let Some(gen) = &o.generator else {
                if k.starts_with("door") || k.starts_with("entrance") || k.starts_with("open") {
                    for r in rooms.iter_mut() {
                        if edge_dist(xy, &r.poly) < 0.6 {
                            r.doors.push(xy);
                        }
                    }
                }
                continue;
            };
            let mut tris: Vec<Tri> = vec![];
            let mut cover: Vec<(usize, usize)> = vec![];
            let mut pillows: Vec<(usize, usize, usize)> = vec![];
            if let Some(v) = visuals.and_then(|v| v.get(k)) {
                for (pi, p) in v.parts.iter().enumerate() {
                    let a0 = tris.len();
                    if v.cloth.iter().any(|j| j.target == pi && !j.collide.is_empty()) {
                        // (range filled after the part is appended)
                        cover.push((a0, usize::MAX));
                    }
                    for t in p.mesh.triangles() {
                        tris.push([o.pose.apply(p.mesh.pos[t[0] as usize]), o.pose.apply(p.mesh.pos[t[1] as usize]), o.pose.apply(p.mesh.pos[t[2] as usize])]);
                    }
                    if let Some(c) = cover.last_mut() {
                        if c.1 == usize::MAX {
                            c.1 = tris.len();
                        }
                    }
                    if v.followers.iter().any(|f| f.part == pi) {
                        pillows.push((a0, tris.len(), pi));
                    }
                }
            }
            if tris.is_empty() {
                tris = state.world(k).tris.clone();
            }
            if tris.is_empty() {
                continue;
            }
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            for t in &tris {
                for p in t {
                    for d in 0..3 {
                        lo[d] = lo[d].min(p[d]);
                        hi[d] = hi[d].max(p[d]);
                    }
                }
            }
            let room = room_of(xy, &rooms);
            if !ignored(&gen.class) {
                rooms[room].n_furn += 1;
            }
            objs.push(Obj { key: k.clone(), class: gen.class.clone(), pose: o.pose, tris, cover, pillows, lo, hi, room, tm: OnceCell::new() });
        }
        World { objs, rooms, floor_z, wall_in: wall_t / 2.0, extra: vec![] }
    }

    /// objects the physics of a person must respect (everything solid except ceiling lights, wall art, curtains; rugs count)
    pub fn blocks_phys(&self, o: &Obj) -> bool {
        !(o.class.contains("CeilingLight") || o.class.contains("WallArt") || o.class.contains("Curtain")) && o.hi[2] - self.floor_z > 0.005
    }

    pub fn blocks(&self, i: usize) -> bool {
        let o = &self.objs[i];
        !ignored(&o.class) && o.hi[2] - self.floor_z > 0.03
    }

    /// worst penetration depth of the boxes into furniture (skipping `skip`)
    pub fn pen_furniture(&self, boxes: &[WBox], skip: Option<usize>) -> f64 {
        let mut worst = 0.0f64;
        let id = Isometry3::identity();
        for b in boxes {
            let (blo, bhi) = b.aabb();
            let cu = b.cuboid();
            let iso = b.iso();
            for (i, o) in self.objs.iter().enumerate() {
                if Some(i) == skip || !self.blocks(i) {
                    continue;
                }
                if (0..3).any(|d| o.hi[d] < blo[d] || o.lo[d] > bhi[d]) {
                    continue;
                }
                let Some(tm) = o.trimesh() else { continue };
                if let Ok(Some(c)) = query::contact(&id, tm, &iso, &cu, 0.0) {
                    if c.dist < 0.0 {
                        if std::env::var_os("PEOPLE_DEBUG2").is_some() && -c.dist > 0.012 {
                            eprintln!("   hit {} {} depth {:.3} box c={:.2?} h={:.2?}", o.key, o.class, -c.dist, b.c, b.h);
                        }
                        worst = worst.max(-c.dist);
                    }
                }
            }
        }
        worst
    }

    /// worst wall penetration: how far a box corner is beyond the inner wall face of `room`
    pub fn pen_wall(&self, boxes: &[WBox], room: usize) -> f64 {
        let r = &self.rooms[room];
        let mut worst = 0.0f64;
        for b in boxes {
            for p in b.corners_xy() {
                let d = inside_dist(p, &r.poly);
                worst = worst.max(self.wall_in - d);
            }
        }
        worst
    }
}

/// worst penetration between two sets of boxes
pub fn pen_boxes(a: &[WBox], b: &[WBox]) -> f64 {
    let mut worst = 0.0f64;
    for x in a {
        let (xl, xh) = x.aabb();
        for y in b {
            let (yl, yh) = y.aabb();
            if (0..3).any(|d| xh[d] < yl[d] || xl[d] > yh[d]) {
                continue;
            }
            if let Ok(Some(c)) = query::contact(&x.iso(), &x.cuboid(), &y.iso(), &y.cuboid(), 0.0) {
                if c.dist < 0.0 {
                    worst = worst.max(-c.dist);
                }
            }
        }
    }
    worst
}

/// what a seat (chair, stool, sofa cushion) offers, in the object's local frame
#[derive(Clone, Debug)]
pub struct Seat {
    /// world z of the seat top
    pub top: f64,
    /// local x of the backrest front face (or the back edge for backless seats) and of the front edge
    pub x0: f64,
    pub x1: f64,
    pub has_back: bool,
    /// free lateral span (between armrests) at the seat middle
    pub ya: f64,
    pub yb: f64,
}

pub fn analyze_seat(o: &Obj, lo: V3, hi: V3, floor_z: f64) -> Option<Seat> {
    let zmax = floor_z + 0.62;
    let step = 0.02;
    let n = (((hi[0] - lo[0]) / step) as usize).max(1);
    let mut tops: Vec<(f64, f64, f64)> = vec![]; // lx, low top, tall top
    for i in 0..=n {
        let lx = lo[0] + i as f64 * step;
        let w = o.w([lx, 0.0, 0.0]);
        let col = o.column(w[0], w[1]);
        let low = col.iter().copied().filter(|z| *z <= zmax).last();
        let tall = col.last().copied();
        if let (Some(l), Some(t)) = (low, tall) {
            tops.push((lx, l, t));
        }
    }
    if tops.is_empty() {
        return None;
    }
    // mode of the low tops (2 cm bins), ignoring things below knee height
    let mut hist: std::collections::BTreeMap<i64, usize> = Default::default();
    for t in &tops {
        if t.1 > floor_z + 0.2 {
            *hist.entry((t.1 * 50.0).round() as i64).or_default() += 1;
        }
    }
    let (&bin, _) = hist.iter().max_by_key(|(b, c)| (**c, std::cmp::Reverse(**b)))?;
    let top = bin as f64 / 50.0;
    let seat_cols: Vec<&(f64, f64, f64)> = tops.iter().filter(|t| (t.1 - top).abs() < 0.04).collect();
    let x1 = seat_cols.iter().map(|t| t.0).fold(f64::MIN, f64::max);
    // backrest: walking back from the front, the first column that rises well above the seat
    let mut x0 = seat_cols.iter().map(|t| t.0).fold(f64::MAX, f64::min);
    let mut has_back = false;
    for t in tops.iter().rev().filter(|t| t.0 <= x1) {
        if t.2 > top + 0.2 {
            x0 = t.0 + step;
            has_back = true;
            break;
        }
    }
    // lateral free span at the seat middle
    let xm = 0.5 * (x0.max(lo[0]) + x1);
    let span = |sign: f64| -> f64 {
        let mut y = 0.0;
        loop {
            let ny = y + sign * 0.02;
            if ny.abs() > 1.6 {
                break;
            }
            let w = o.w([xm, ny, 0.0]);
            let col = o.column(w[0], w[1]);
            let low = col.iter().copied().filter(|z| *z <= zmax).last();
            let tall = col.last().copied();
            match (low, tall) {
                (Some(l), Some(t)) if (l - top).abs() < 0.07 && t < top + 0.25 => y = ny,
                _ => break,
            }
        }
        y
    };
    Some(Seat { top, x0, x1, has_back, ya: span(-1.0), yb: span(1.0) })
}
