//! vox: the scene in a coarse voxel grid, for choosing views. A candidate view is a small picture and depth found by walking rays through the grid, in a fraction of a millisecond
//! against the GPU's whole render pass: the search scores these, and only what it chooses is drawn properly.
use glam::Vec3;

pub const CELL: f32 = 0.05;
pub struct Grid { pub lo: Vec3, pub n: [usize; 3], pub cells: Vec<u16> }

impl Grid {
    pub fn new(lo: Vec3, hi: Vec3) -> Grid {
        let e = (hi - lo) / CELL;
        let n = [(e.x.ceil() as usize).max(1), (e.y.ceil() as usize).max(1), (e.z.ceil() as usize).max(1)];
        Grid { lo, n, cells: vec![0; n[0] * n[1] * n[2]] }
    }
    #[inline] fn at(&self, i: usize, j: usize, k: usize) -> usize { (k * self.n[1] + j) * self.n[0] + i }
    /// writes a triangle into every cell it passes through, by points closer together than a cell
    pub fn tri(&mut self, t: &[Vec3; 3], id: u16) {
        let (a, b, c) = (t[0], t[1], t[2]);
        let longest = (b - a).length().max((c - a).length()).max((c - b).length());
        let steps = ((longest / (CELL * 0.7)).ceil() as usize).clamp(1, 2000);
        let s = steps as f32;
        for i in 0..=steps { for j in 0..=(steps - i) {
            let (u, v) = (i as f32 / s, j as f32 / s);
            let p = a + (b - a) * u + (c - a) * v;
            let q = (p - self.lo) / CELL;
            if q.x < 0.0 || q.y < 0.0 || q.z < 0.0 { continue }
            let (x, y, z) = (q.x as usize, q.y as usize, q.z as usize);
            if x < self.n[0] && y < self.n[1] && z < self.n[2] { let at = self.at(x, y, z); self.cells[at] = id; }
        } }
    }
    /// along a ray from `o`: the first filled cell `pass` does not let through, past `t0`: (distance, id, axis of the face it was entered by)
    pub fn march(&self, o: Vec3, d: Vec3, t0: f32, tmax: f32, pass: &(dyn Fn(u16, Vec3) -> bool + Sync)) -> Option<(f32, u16, usize)> {
        let hi = self.lo + Vec3::new(self.n[0] as f32, self.n[1] as f32, self.n[2] as f32) * CELL;
        // into the grid
        let inv = d.recip();
        let (ta, tb) = ((self.lo - o) * inv, (hi - o) * inv);
        let (mn, mx) = (ta.min(tb), ta.max(tb));
        let mut t = mn.max_element().max(t0).max(0.0);
        let end = mx.min_element().min(tmax);
        if t >= end { return None }
        let start = o + d * (t + 1e-4);
        let q = (start - self.lo) / CELL;
        let mut c = [(q.x as i64).clamp(0, self.n[0] as i64 - 1), (q.y as i64).clamp(0, self.n[1] as i64 - 1), (q.z as i64).clamp(0, self.n[2] as i64 - 1)];
        let step = [if d.x > 0.0 { 1i64 } else { -1 }, if d.y > 0.0 { 1 } else { -1 }, if d.z > 0.0 { 1 } else { -1 }];
        let delta = (Vec3::splat(CELL) * inv).abs();
        let mut side = Vec3::ZERO;
        for a in 0..3 {
            let edge = self.lo[a] + (c[a] + if step[a] > 0 { 1 } else { 0 }) as f32 * CELL;
            side[a] = if d[a].abs() < 1e-9 { f32::MAX } else { (edge - o[a]) * inv[a] };
        }
        let mut axis = 2usize;
        while t < end {
            let id = self.cells[self.at(c[0] as usize, c[1] as usize, c[2] as usize)];
            if id != 0 && !pass(id, o + d * t) { return Some((t, id, axis)) }
            axis = if side.x < side.y && side.x < side.z { 0 } else if side.y < side.z { 1 } else { 2 };
            t = side[axis];
            side[axis] += delta[axis];
            c[axis] += step[axis];
            if c[axis] < 0 || c[axis] >= self.n[axis] as i64 { return None }
        }
        None
    }
}
