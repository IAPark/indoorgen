//! The voxel world of one room. A column of 64 bits per 5 cm floor cell (5 cm layers from the floor: 3.2 m), one grid per channel.
//! Search is optimistic: a piece is rastered inset by a few cm, so a tight fit is never lost to rounding; the prism tests then decide exactly.
use crate::house::{self, Room};
use ig_kit::piece::{Role, Volume};

pub const CELL: f64 = 0.05;
pub const LAYER: f64 = 0.05;
const INSET: f64 = 0.03;
/// half a person: the walk stays this far from what is hard, m
pub const PERSON: f64 = 0.30;

pub type Xy = [f64; 2];

/// a convex prism in world space
#[derive(Clone, Debug, serde::Serialize)]
pub struct Prism { pub hull: Vec<Xy>, pub z: [f64; 2], pub role: u8 }
pub const HARD: u8 = 0;
pub const OPENING: u8 = 1;
pub const SOFT: u8 = 2;
pub const KEEP: u8 = 3;
/// on the floor under everything: only the walls limit it
pub const COVER: u8 = 4;

pub fn role_of(r: &Role) -> u8 { match r { Role::Hard | Role::Support => HARD, Role::Opening(_) => OPENING, Role::Soft => SOFT, Role::Inset => 255 } }

pub fn hull2(mut p: Vec<Xy>) -> Vec<Xy> {
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    p.dedup();
    if p.len() < 3 { return p }
    let cr = |o: Xy, a: Xy, b: Xy| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<Xy> = vec![];
    for &q in &p { while h.len() >= 2 && cr(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 { h.pop(); } h.push(q); }
    let t = h.len() + 1;
    for &q in p.iter().rev().skip(1) { while h.len() >= t && cr(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 { h.pop(); } h.push(q); }
    h.pop();
    h
}

/// separation of two convex polygons along their edge normals: > 0 apart, < 0 how deep
pub fn sat(a: &[Xy], b: &[Xy]) -> f64 {
    let mut best = f64::MIN;
    for (p, q) in [(a, b), (b, a)] {
        for i in 0..p.len() {
            let (s, e) = (p[i], p[(i + 1) % p.len()]);
            let (dx, dy) = (e[0] - s[0], e[1] - s[1]);
            let l = dx.hypot(dy);
            if l < 1e-9 { continue }
            let n = [dy / l, -dx / l];
            let d0 = n[0] * s[0] + n[1] * s[1];
            // (hulls run counter-clockwise: n points out)
            let m = q.iter().map(|v| n[0] * v[0] + n[1] * v[1] - d0).fold(f64::MAX, f64::min);
            best = best.max(m);
        }
    }
    best
}

pub fn in_hull(h: &[Xy], p: Xy, inset: f64) -> bool {
    for i in 0..h.len() {
        let (s, e) = (h[i], h[(i + 1) % h.len()]);
        let (dx, dy) = (e[0] - s[0], e[1] - s[1]);
        let l = dx.hypot(dy);
        if l < 1e-9 { continue }
        if (dx * (p[1] - s[1]) - dy * (p[0] - s[0])) / l < inset { return false }
    }
    true
}

/// a posed piece: yaw about z, then translation
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Pose { pub yaw: f64, pub t: Xy, pub z: f64 }
impl Pose {
    pub fn apply(&self, p: Xy) -> Xy { let (s, c) = self.yaw.sin_cos(); [c * p[0] - s * p[1] + self.t[0], s * p[0] + c * p[1] + self.t[1]] }
}

pub fn prisms(vols: &[Volume], pose: &Pose) -> Vec<Prism> {
    vols.iter().filter_map(|v| {
        let role = role_of(&v.role);
        if role == 255 { return None }
        let pts = v.points();
        let hull = hull2(pts.iter().map(|q| pose.apply([q[0], q[1]])).collect());
        let (mut z0, mut z1) = (f64::MAX, f64::MIN);
        for q in &pts { z0 = z0.min(q[2] + pose.z); z1 = z1.max(q[2] + pose.z); }
        Some(Prism { hull, z: [z0, z1], role })
    }).collect()
}

/// why a pose is refused
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize)]
pub enum Why { Outside, Hard, Door, Opening, Window, Walk, Reach, Exact }

#[derive(Clone)]
pub struct World {
    pub ox: f64, pub oy: f64, pub nx: usize, pub ny: usize,
    /// inside the room, clear of the walls
    pub floor: Vec<bool>,
    pub hard: Vec<u64>,
    pub open: Vec<u64>,
    pub soft: Vec<u64>,
    pub keep: Vec<u64>,
    /// the exact shapes behind the grids
    pub placed: Vec<Prism>,
    pub keep_prisms: Vec<Prism>,
    pub inner_d: f64,
}

pub fn mask(z: [f64; 2], inset: f64) -> u64 {
    let (a, b) = (((z[0] + inset) / LAYER).floor().max(0.0) as i64, ((z[1] - inset) / LAYER).ceil().min(64.0) as i64);
    if b <= a { return 0 }
    let n = (b - a) as u32;
    (if n >= 64 { u64::MAX } else { (1u64 << n) - 1 }) << a
}

impl World {
    pub fn new(room: &Room) -> World {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &room.outline { for d in 0..2 { lo[d] = lo[d].min(p[d]); hi[d] = hi[d].max(p[d]); } }
        let (nx, ny) = (((hi[0] - lo[0]) / CELL).ceil() as usize + 1, ((hi[1] - lo[1]) / CELL).ceil() as usize + 1);
        let mut w = World { ox: lo[0], oy: lo[1], nx, ny, floor: vec![false; nx * ny], hard: vec![0; nx * ny], open: vec![0; nx * ny], soft: vec![0; nx * ny], keep: vec![0; nx * ny], placed: vec![], keep_prisms: vec![], inner_d: room.wall_t / 2.0 };
        for j in 0..ny { for i in 0..nx {
            let c = w.center(i, j);
            w.floor[j * nx + i] = house::inside(&room.outline, c) && house::edge_dist(&room.outline, c) >= room.wall_t / 2.0;
        } }
        // a door's clearance: its width by its width into the room (as far as 0.9 m), to door height
        for d in &room.doors {
            let depth = (d.hw * 2.0).min(0.9);
            let e = 0.03;
            let q = |a: f64, b: f64| [d.at[0] + d.u[0] * a + d.n[0] * b, d.at[1] + d.u[1] * a + d.n[1] * b];
            let hull = hull2(vec![q(-d.hw - e, 0.0), q(d.hw + e, 0.0), q(d.hw + e, depth), q(-d.hw - e, depth)]);
            let p = Prism { hull, z: [0.0, 2.1], role: KEEP };
            w.stamp(&p);
            w.keep_prisms.push(p);
        }
        w
    }
    pub fn center(&self, i: usize, j: usize) -> Xy { [self.ox + (i as f64 + 0.5) * CELL, self.oy + (j as f64 + 0.5) * CELL] }
    fn range(&self, h: &[Xy]) -> (usize, usize, usize, usize) {
        let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in h { for d in 0..2 { a[d] = a[d].min(p[d]); b[d] = b[d].max(p[d]); } }
        let f = |v: f64, o: f64, n: usize| (((v - o) / CELL).floor().max(0.0) as usize).min(n);
        (f(a[0], self.ox, self.nx), (f(b[0], self.ox, self.nx) + 1).min(self.nx), f(a[1], self.oy, self.ny), (f(b[1], self.oy, self.ny) + 1).min(self.ny))
    }
    /// writes a prism in the grid of its channel (cells it touches at all: what comes later is tested inset against this)
    pub fn stamp(&mut self, p: &Prism) {
        let m = mask(p.z, 0.0);
        let (i0, i1, j0, j1) = self.range(&p.hull);
        for j in j0..j1 { for i in i0..i1 {
            if !in_hull(&p.hull, self.center(i, j), -CELL * 0.71) { continue }
            let k = j * self.nx + i;
            match p.role { HARD => self.hard[k] |= m, OPENING => self.open[k] |= m, KEEP => self.keep[k] |= m, SOFT => self.soft[k] |= m, _ => {} }
        } }
    }
    /// the voxel test of a piece's prisms; the first refusal
    pub fn voxel(&self, ps: &[Prism]) -> Option<Why> {
        for p in ps {
            // (a layer in from both ends: what stands on a top shares a rounded layer with it; the exact test decides)
            let m = mask(p.z, LAYER + 0.001);
            let (i0, i1, j0, j1) = self.range(&p.hull);
            for j in j0..j1 { for i in i0..i1 {
                let c = self.center(i, j);
                if !in_hull(&p.hull, c, INSET) { continue }
                let k = j * self.nx + i;
                if !self.floor[k] { return Some(Why::Outside) }
                match p.role {
                    HARD => { if self.hard[k] & m != 0 { return Some(Why::Hard) } if self.keep[k] & m != 0 { return Some(Why::Door) } if self.open[k] & m != 0 { return Some(Why::Opening) } }
                    OPENING => { if self.hard[k] & m != 0 { return Some(Why::Opening) } }
                    _ => {}
                }
            } }
        }
        None
    }
    /// the exact test: the prisms against the walls and what is placed
    pub fn exact(&self, ps: &[Prism], room: &Room) -> Option<Why> {
        for p in ps {
            // (a thin thing on the floor keeps a little further off: the skirting's foot, a rug's edge read as through the wall)
            let clear = if p.role == COVER { 0.03 } else { 0.0 };
            for q in &p.hull {
                if !house::inside(&room.outline, *q) || house::edge_dist(&room.outline, *q) < self.inner_d + clear - 0.002 { return Some(Why::Outside) }
            }
            // a wall's corner inside the shape (a notch)
            if room.outline.iter().any(|c| in_hull(&p.hull, *c, 0.002)) { return Some(Why::Outside) }
            for o in self.placed.iter().chain(self.keep_prisms.iter()) {
                let clash = match (p.role, o.role) { (HARD, HARD) | (HARD, OPENING) | (HARD, KEEP) | (OPENING, HARD) => true, _ => false };
                if clash && p.z[0] < o.z[1] - 0.002 && o.z[0] < p.z[1] - 0.002 && sat(&p.hull, &o.hull) < -0.002 {
                    return Some(if o.role == KEEP { Why::Door } else { Why::Exact });
                }
            }
        }
        None
    }
    pub fn add(&mut self, ps: &[Prism]) { for p in ps { self.stamp(p); self.placed.push(p.clone()); } }

    /// the walkable cells: on the floor, nothing hard from the ankles to head height within half a person
    pub fn walk(&self, extra: &[Prism]) -> Vec<bool> {
        let body = mask([0.05, 1.9], 0.0);
        let (nx, ny) = (self.nx, self.ny);
        // (soft parts -- bedding, foliage -- may be walked through: user, 2026-11-03; an item with soft_blocks counts its soft parts as hard)
        let mut blocked: Vec<bool> = (0..nx * ny).map(|k| !self.floor[k] || self.hard[k] & body != 0).collect();
        for p in extra.iter().filter(|p| p.role == HARD) {
            if p.z[0] >= 1.9 || p.z[1] <= 0.05 { continue }
            let (i0, i1, j0, j1) = self.range(&p.hull);
            for j in j0..j1 { for i in i0..i1 { if in_hull(&p.hull, self.center(i, j), 0.0) { blocked[j * nx + i] = true; } } }
        }
        // two-pass chamfer distance, in cells
        const FAR: f32 = 1e6;
        let mut d: Vec<f32> = blocked.iter().map(|b| if *b { 0.0 } else { FAR }).collect();
        let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
        for j in 0..ny { for i in 0..nx {
            let k = j * nx + i;
            if i > 0 { d[k] = d[k].min(d[k - 1] + a); }
            if j > 0 { d[k] = d[k].min(d[k - nx] + a); if i > 0 { d[k] = d[k].min(d[k - nx - 1] + b); } if i + 1 < nx { d[k] = d[k].min(d[k - nx + 1] + b); } }
        } }
        for j in (0..ny).rev() { for i in (0..nx).rev() {
            let k = j * nx + i;
            if i + 1 < nx { d[k] = d[k].min(d[k + 1] + a); }
            if j + 1 < ny { d[k] = d[k].min(d[k + nx] + a); if i + 1 < nx { d[k] = d[k].min(d[k + nx + 1] + b); } if i > 0 { d[k] = d[k].min(d[k + nx - 1] + b); } }
        } }
        // (measured from cell centres: a cell's width more, or a corridor passes a few cm under what the exact check wants)
        let r = (PERSON / CELL) as f32 + 1.0;
        d.iter().map(|v| *v >= r).collect()
    }
    /// the walkable cells reached from the first door; whether every door reaches them
    pub fn flood(&self, walk: &[bool], room: &Room) -> (Vec<bool>, bool) {
        let (nx, ny) = (self.nx, self.ny);
        // a door's spot: a walkable cell within 0.35 m of the middle of its clearance (the room is joined up at the door, not somewhere past it)
        let start = |d: &house::Opening| -> Option<usize> {
            let dep = (d.hw * 2.0).min(0.9) / 2.0;
            let c = [d.at[0] + d.n[0] * dep, d.at[1] + d.n[1] * dep];
            let mut best: Option<(f64, usize)> = None;
            let r = (0.35 / CELL) as i64 + 1;
            let (ci, cj) = (((c[0] - self.ox) / CELL) as i64, ((c[1] - self.oy) / CELL) as i64);
            for dj in -r..=r { for di in -r..=r {
                let (i, j) = (ci + di, cj + dj);
                if i < 0 || j < 0 || i as usize >= nx || j as usize >= ny { continue }
                let k = j as usize * nx + i as usize;
                if !walk[k] { continue }
                let q = self.center(i as usize, j as usize);
                let e = (q[0] - c[0]).hypot(q[1] - c[1]);
                if e < 0.35 && best.map_or(true, |b| e < b.0) { best = Some((e, k)); }
            } }
            best.map(|b| b.1)
        };
        let mut seen = vec![false; nx * ny];
        let Some(d0) = room.doors.first() else { return (walk.to_vec(), true) };
        let Some(s) = start(d0) else { return (seen, false) };
        let mut stack = vec![s];
        seen[s] = true;
        while let Some(k) = stack.pop() {
            let (i, j) = (k % nx, k / nx);
            let mut go = |n: usize| { if walk[n] && !seen[n] { seen[n] = true; stack.push(n); } };
            if i > 0 { go(k - 1); } if i + 1 < nx { go(k + 1); } if j > 0 { go(k - nx); } if j + 1 < ny { go(k + nx); }
        }
        let all = room.doors.iter().all(|d| start(d).map_or(false, |k| seen[k]));
        (seen, all)
    }
    /// any of `seen` within `d` of a shape
    pub fn near(&self, seen: &[bool], hull: &[Xy], d: f64) -> bool {
        let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in hull { for k in 0..2 { a[k] = a[k].min(p[k]); b[k] = b[k].max(p[k]); } }
        let (i0, i1, j0, j1) = self.range(&[[a[0] - d, a[1] - d], [b[0] + d, b[1] + d]]);
        for j in j0..j1 { for i in i0..i1 { if seen[j * self.nx + i] && in_hull(hull, self.center(i, j), -d) { return true } } }
        false
    }
    pub fn cell_of(&self, p: Xy) -> Option<usize> {
        let (i, j) = (((p[0] - self.ox) / CELL) as i64, ((p[1] - self.oy) / CELL) as i64);
        (i >= 0 && j >= 0 && (i as usize) < self.nx && (j as usize) < self.ny).then(|| j as usize * self.nx + i as usize)
    }
}
