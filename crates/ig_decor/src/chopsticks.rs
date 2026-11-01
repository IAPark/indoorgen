//! tableware/chopsticks.py ChopsticksFactory: grid -> SOLIDIFY -> tapered bar (write_co) -> SUBSURF(2, simple if square) -> optional guard extrude
//! (depth 0), scaled; two sticks joined with join_objects (the first object keeps its local coordinates, the second is placed relative to it).
use super::cutlery::extrude_regions;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "ChopsticksFactory"
}

pub struct Chopsticks {
    y_length: f64,
    y_shrink: f64,
    is_square: bool,
    has_guard: bool,
    x_guard: f64,
    scale: f64,
    fs: u64,
}

type M = ([[f64; 3]; 3], [f64; 3]);

fn rotz(a: f64) -> [[f64; 3]; 3] {
    let m = crate::bl32::eul_to_mat3([0.0, 0.0, a as f32]);
    // bl32 matrices are m[col][row]
    let mut r = [[0.0; 3]; 3];
    for c in 0..3 {
        for row in 0..3 {
            r[row][c] = m[c][row] as f64;
        }
    }
    r
}

fn apply(m: &M, p: [f64; 3]) -> [f64; 3] {
    let mut o = [0.0; 3];
    for i in 0..3 {
        o[i] = m.0[i][0] * p[0] + m.0[i][1] * p[1] + m.0[i][2] * p[2] + m.1[i];
    }
    o
}

fn inv(m: &M) -> M {
    let r = m.0;
    let rt = [[r[0][0], r[1][0], r[2][0]], [r[0][1], r[1][1], r[2][1]], [r[0][2], r[1][2], r[2][2]]];
    let t = [-(rt[0][0] * m.1[0] + rt[0][1] * m.1[1] + rt[0][2] * m.1[2]), -(rt[1][0] * m.1[0] + rt[1][1] * m.1[1] + rt[1][2] * m.1[2]), -(rt[2][0] * m.1[0] + rt[2][1] * m.1[1] + rt[2][2] * m.1[2])];
    (rt, t)
}

fn compose(a: &M, b: &M) -> M {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a.0[i][k] * b.0[k][j]).sum();
        }
    }
    let t = apply(a, b.1);
    (r, t)
}

impl Chopsticks {
    fn new(fs: u64) -> Chopsticks {
        let mut r = Rng::seeded(fs);
        let y_length = r.uniform(0.01, 0.02);
        let y_shrink = r.log_uniform(0.2, 0.8);
        let is_square = r.uniform(0.0, 1.0) < 0.5;
        let has_guard = r.uniform(0.0, 1.0) < 0.4;
        let x_guard = r.uniform(0.4, 0.9);
        let scale = r.log_uniform(0.2, 0.4);
        Chopsticks { y_length, y_shrink, is_square, has_guard, x_guard, scale, fs }
    }

    fn make_single(&self) -> Mesh {
        let n = (1.0 / self.y_length) as usize;
        let mut g = Mesh::new();
        for _ in 0..2 * n {
            g.pos.push([0.0; 3]);
        }
        for i in 0..n - 1 {
            g.faces.push(vec![i as u32, (i + 1) as u32, (n + i + 1) as u32, (n + i) as u32]);
        }
        let mut m = books::solidify_simple(&g, self.y_length * 2.0, None);
        for b in 0..4 {
            for i in 0..n {
                let l = (self.y_shrink + (1.0 - self.y_shrink) * i as f64 / (n - 1) as f64) * self.y_length;
                let x = i as f64 / (n - 1) as f64;
                let y = if b % 2 == 0 { -l } else { l };
                let z = if b < 2 { l } else { -l };
                m.pos[b * n + i] = [x as f32 as f64, y as f32 as f64, z as f32 as f64];
            }
        }
        let mut m = subsurf(&m, 2, self.is_square);
        if self.has_guard {
            let sel: Vec<bool> = m
                .faces
                .iter()
                .map(|f| {
                    let mut sx = 0.0f32;
                    for &v in f {
                        sx += m.pos[v as usize][0] as f32;
                    }
                    (sx / f.len() as f32) as f64 > self.x_guard
                })
                .collect();
            if sel.iter().any(|&s| s) {
                m = extrude_regions(&m, &sel, 0.0);
            }
        }
        for p in m.pos.iter_mut() {
            for c in 0..3 {
                p[c] = (p[c] as f32 * self.scale as f32) as f64;
            }
        }
        m
    }
}

impl Factory for Chopsticks {
    /// collision: the two sticks' boxes (x along the stick, scale long from 0 or from -1, the section y_length about it), the second one
    /// placed as the build's draws put it relative to the first
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let mut r = inst_rng(self.fs, i);
        let ident: M = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]);
        let mut x0 = 0.0;
        let (ma, mb): (M, M);
        if r.uniform(0.0, 1.0) < 0.6 {
            let distance = r.log_uniform(self.y_length, 0.04);
            let flip = r.uniform(0.0, 1.0) >= 0.5;
            if flip { x0 = -1.0; }
            let a1 = r.uniform(0.0, PI / 8.0);
            let a2 = r.uniform(0.0, PI / 8.0);
            let s = if flip { -1.0 } else { 1.0 };
            ma = (rotz(s * a1), [0.0, distance, 0.0]);
            mb = (rotz(-s * a2), [0.0, -distance, 0.0]);
        } else {
            let lx = r.uniform(-0.1, 0.2);
            let ly = r.uniform(-0.2, 0.2);
            let sign = if ly > 0.0 { 1.0 } else if ly < 0.0 { -1.0 } else { 0.0 };
            let ang = r.log_uniform(PI / 8.0, PI / 4.0);
            ma = ident;
            mb = (rotz(-sign * ang), [lx, ly, self.y_length]);
        }
        let rel = compose(&inv(&ma), &mb);
        let (sc, yl, m) = (self.scale, self.y_length * self.scale * 1.02, 0.002);
        // (the stick is scaled first, then the flipped pair moved back by a whole unit)
        let (lo, hi) = ([x0 - m, -yl - m, -yl - m], [x0 + sc + m, yl + m, yl + m]);
        let mut pts = vec![];
        for x in [lo[0], hi[0]] { for y in [lo[1], hi[1]] { for z in [lo[2], hi[2]] { pts.push(apply(&rel, [x, y, z])); } } }
        Some(vec![crate::piece::Volume::hard(lo, hi), crate::piece::Volume::hard_hull(pts)])
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let single = self.make_single();
        let ident: M = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]);
        let mut a_mesh = single.clone();
        let mut b_mesh = single.clone();
        let (ma, mb): (M, M);
        if r.uniform(0.0, 1.0) < 0.6 {
            let distance = r.log_uniform(self.y_length, 0.04);
            if r.uniform(0.0, 1.0) < 0.5 {
                let a1 = r.uniform(0.0, PI / 8.0);
                let a2 = r.uniform(0.0, PI / 8.0);
                ma = (rotz(a1), [0.0, distance, 0.0]);
                mb = (rotz(-a2), [0.0, -distance, 0.0]);
            } else {
                for m in [&mut a_mesh, &mut b_mesh] {
                    for p in m.pos.iter_mut() {
                        p[0] = (p[0] as f32 - 1.0) as f64;
                    }
                }
                let a1 = r.uniform(0.0, PI / 8.0);
                let a2 = r.uniform(0.0, PI / 8.0);
                ma = (rotz(-a1), [0.0, distance, 0.0]);
                mb = (rotz(a2), [0.0, -distance, 0.0]);
            }
        } else {
            let lx = r.uniform(-0.1, 0.2);
            let ly = r.uniform(-0.2, 0.2);
            let sign = if ly > 0.0 { 1.0 } else if ly < 0.0 { -1.0 } else { 0.0 };
            let ang = r.log_uniform(PI / 8.0, PI / 4.0);
            ma = ident;
            mb = (rotz(-sign * ang), [lx, ly, self.y_length]);
        }
        // join_objects: the first object keeps local coordinates; the second goes through inv(first) * second
        let rel = compose(&inv(&ma), &mb);
        let _ = &ma;
        let mut m = a_mesh;
        let off = m.pos.len() as u32;
        m.pos.extend(b_mesh.pos.iter().map(|&p| {
            let q = apply(&rel, p);
            [q[0] as f32 as f64, q[1] as f32 as f64, q[2] as f32 as f64]
        }));
        m.faces.extend(b_mesh.faces.iter().map(|f| f.iter().map(|&v| v + off).collect::<Vec<_>>()));
        let mat = spec_material("chopsticks", json!({"cls": "wood.Wood", "kwargs": {}, "seed": self.fs}), [0.6, 0.45, 0.3]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Chopsticks::new(fs)))
}
