//! tableware/fork.py, spoon.py (and shared guard extrusion): grid -> SOLIDIFY -> subsurf -> geometry-node guard extrusion -> subsurf
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "ForkFactory" | "SpoonFactory" | "KnifeFactory")
}

type F3 = [f32; 3];

fn to_f3(v: V3) -> F3 {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

fn face_normal(m: &Mesh, f: &[u32]) -> F3 {
    let p: Vec<F3> = f.iter().map(|&i| to_f3(m.pos[i as usize])).collect();
    let sub = |a: F3, b: F3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let norm = |a: F3| {
        let d = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
        if d > 1e-35 {
            let inv = 1.0f32 / d.sqrt();
            [a[0] * inv, a[1] * inv, a[2] * inv]
        } else {
            [0.0; 3]
        }
    };
    let cross = |a: F3, b: F3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    match p.len() {
        3 => norm(cross(sub(p[0], p[1]), sub(p[1], p[2]))),
        4 => norm(cross(sub(p[0], p[2]), sub(p[1], p[3]))),
        k => {
            let mut n = [0.0f32; 3];
            let mut prev = p[k - 1];
            for &cur in &p {
                n[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
                n[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
                n[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
                prev = cur;
            }
            norm(n)
        }
    }
}

/// ExtrudeMesh (Faces, Individual off): selected faces are moved to duplicated vertices displaced by the mean of the adjacent selected face normals * scale;
/// side faces close the boundary of the selection
pub fn extrude_regions(m: &Mesh, sel: &[bool], scale: f32) -> Mesh {
    let nf = m.faces.len();
    let nrm: Vec<F3> = (0..nf).map(|i| if sel[i] { face_normal(m, &m.faces[i]) } else { [0.0; 3] }).collect();
    // edge selection counts and boundary vertices
    let mut ecount: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            *ecount.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut boundary = vec![false; m.pos.len()];
    for (&(a, b), &c) in &ecount {
        if c == 1 {
            boundary[a as usize] = true;
            boundary[b as usize] = true;
        }
    }
    // accumulate offsets
    let mut acc = vec![[0.0f32; 3]; m.pos.len()];
    let mut cnt = vec![0u32; m.pos.len()];
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for &v in f {
            let j = v as usize;
            acc[j] = [acc[j][0] + nrm[fi][0], acc[j][1] + nrm[fi][1], acc[j][2] + nrm[fi][2]];
            cnt[j] += 1;
        }
    }
    let moved = |v: usize| -> [f64; 3] {
        let c = cnt[v] as f32;
        let o = [acc[v][0] / c * scale, acc[v][1] / c * scale, acc[v][2] / c * scale];
        let p = to_f3(m.pos[v]);
        [(p[0] + o[0]) as f64, (p[1] + o[1]) as f64, (p[2] + o[2]) as f64]
    };
    let mut out = Mesh::new();
    out.pos = m.pos.clone();
    let mut newidx: crate::dhash::HashMap<u32, u32> = crate::dhash::HashMap::default();
    // first appearance order in selected faces
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for &v in f {
            if newidx.contains_key(&v) {
                continue;
            }
            if boundary[v as usize] {
                out.pos.push(moved(v as usize));
                newidx.insert(v, (out.pos.len() - 1) as u32);
            } else {
                out.pos[v as usize] = moved(v as usize);
                newidx.insert(v, v);
            }
        }
    }
    let mut sides: Vec<Vec<u32>> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            out.faces.push(f.clone());
            continue;
        }
        out.faces.push(f.iter().map(|v| newidx[v]).collect());
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            if ecount[&(a.min(b), a.max(b))] == 1 {
                sides.push(vec![newidx[&b], newidx[&a], a, b]);
            }
        }
    }
    out.faces.extend(sides);
    out
}

/// base.add_guard(obj, selection): geometry-node extrusion of the faces selected by `x < -x_end` (optionally only the +-z facing ones)
fn add_guard(m: &Mesh, has_guard: bool, double: bool, x_end: f32, depth: f32) -> Mesh {
    add_guard_x(m, has_guard, double, -x_end, depth)
}

pub fn add_guard_x(m: &Mesh, has_guard: bool, double: bool, x_thr: f32, depth: f32) -> Mesh {
    let sel: Vec<bool> = m
        .faces
        .iter()
        .map(|f| {
            if !has_guard {
                return false;
            }
            let mut sx = 0.0f32;
            for &v in f {
                sx += m.pos[v as usize][0] as f32;
            }
            let cx = sx / f.len() as f32;
            let mut s = cx < x_thr;
            if double {
                let n = face_normal(m, f);
                s = s && n[2].abs() > 0.8;
            }
            s
        })
        .collect();
    if !sel.iter().any(|&s| s) {
        return m.clone();
    }
    extrude_regions(m, &sel, depth)
}

pub fn grid(nx: usize, ny: usize) -> Vec<Vec<u32>> {
    let w = nx as u32;
    let mut faces = vec![];
    for j in 0..ny as u32 - 1 {
        for i in 0..w - 1 {
            let a = j * w + i;
            faces.push(vec![a, a + 1, a + 1 + w, a + w]);
        }
    }
    faces
}

pub struct Cutlery {
    fork: bool,
    x_length: f64,
    x_tip: f64,
    y_length: f64,
    z_depth: f64,
    z_offset: f64,
    thickness: f64,
    has_guard: bool,
    double: bool,
    n_cuts: usize,
    guard_depth: f64,
    scale: f64,
    fs: u64,
}

impl Cutlery {
    fn new(fork: bool, fs: u64) -> Cutlery {
        let mut r = Rng::seeded(fs);
        if fork {
            let x_length = r.log_uniform(0.4, 0.8);
            let x_tip = r.uniform(0.15, 0.2);
            let y_length = r.log_uniform(0.05, 0.08);
            let z_depth = r.log_uniform(0.02, 0.04);
            let z_offset = r.uniform(0.0, 0.05);
            let thickness = r.log_uniform(0.008, 0.015);
            let has_guard = r.uniform(0.0, 1.0) < 0.4;
            let double = !(r.uniform(0.0, 1.0) < 0.6);
            let n_cuts = if r.uniform(0.0, 1.0) < 0.3 { r.randint(1, 3) as usize } else { 3 };
            let guard_depth = r.log_uniform(0.2, 1.0) * thickness;
            let scale = r.log_uniform(0.15, 0.25);
            Cutlery { fork, x_length, x_tip, y_length, z_depth, z_offset, thickness, has_guard, double, n_cuts, guard_depth, scale, fs }
        } else {
            let x_length = r.log_uniform(0.2, 0.8);
            let y_length = r.log_uniform(0.06, 0.12);
            let z_depth = r.log_uniform(0.08, 0.25);
            let z_offset = r.uniform(0.0, 0.05);
            let thickness = r.log_uniform(0.008, 0.015);
            let has_guard = r.uniform(0.0, 1.0) < 0.4;
            let double = !(r.uniform(0.0, 1.0) < 0.6);
            let guard_depth = r.log_uniform(0.2, 1.0) * thickness;
            let scale = r.log_uniform(0.15, 0.25);
            Cutlery { fork, x_length, x_tip: 0.0, y_length, z_depth, z_offset, thickness, has_guard, double, n_cuts: 0, guard_depth, scale, fs }
        }
    }
}

impl Cutlery {
    /// the profile's anchors (x, half-width y, z) from the instance's draws
    fn anchors(&self, r: &mut Rng) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let x_end = 0.15f64;
        let (xa, ya, za): (Vec<f64>, Vec<f64>, Vec<f64>);
        if self.fork {
            let a1 = r.uniform(-0.04, -0.02);
            let a6 = -x_end - self.x_length * r.log_uniform(1.2, 1.4);
            xa = vec![self.x_tip, a1, -0.08, -0.12, -x_end, -x_end - self.x_length, a6];
            let y = &self.y_length;
            let mut v = vec![];
            v.push(y * r.log_uniform(0.8, 1.0));
            v.push(y * r.log_uniform(1.0, 1.2));
            v.push(y * r.log_uniform(0.6, 1.0));
            v.push(y * r.log_uniform(0.2, 0.4));
            v.push(r.log_uniform(0.01, 0.02));
            v.push(r.log_uniform(0.02, 0.05));
            v.push(r.log_uniform(0.01, 0.02));
            ya = v;
            let z5 = self.z_offset + r.uniform(-0.02, 0.04);
            let z6 = self.z_offset + r.uniform(-0.02, 0.0);
            za = vec![0.0, -self.z_depth, -self.z_depth, 0.0, self.z_offset, z5, z6];
        } else {
            let a0 = r.log_uniform(0.07, 0.25);
            let a6 = -x_end - self.x_length * r.log_uniform(1.2, 1.4);
            xa = vec![a0, 0.0, -0.08, -0.12, -x_end, -x_end - self.x_length, a6];
            let y = &self.y_length;
            let mut v = vec![];
            v.push(y * r.log_uniform(0.1, 0.8));
            v.push(y * r.log_uniform(1.0, 1.2));
            v.push(y * r.log_uniform(0.6, 1.0));
            v.push(y * r.log_uniform(0.2, 0.4));
            v.push(r.log_uniform(0.01, 0.02));
            v.push(r.log_uniform(0.02, 0.05));
            v.push(r.log_uniform(0.01, 0.02));
            ya = v;
            let z5 = self.z_offset + r.uniform(-0.02, 0.04);
            let z6 = self.z_offset + r.uniform(-0.02, 0.0);
            za = vec![0.0, 0.0, 0.0, 0.0, self.z_offset, z5, z6];
        }
        (xa, ya, za)
    }
}

impl Factory for Cutlery {
    /// collision: the profile's anchors (the grid the solidified, subdivided surface stays inside), the solidify's thickness and the
    /// guard's extrusion round them, scaled
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (xa, ya, za) = self.anchors(&mut inst_rng(self.fs, i));
        let g = self.thickness + self.guard_depth;
        let mn = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let z0 = mn(&za).min(-self.z_depth);
        let (sc, m) = (self.scale, 0.002);
        let y = mx(&ya) + g;
        Some(vec![crate::piece::Volume::hard([(mn(&xa) - g) * sc - m, -y * sc - m, (z0 - g) * sc - m], [(mx(&xa) + 0.02 + g) * sc + m, y * sc + m, (mx(&za) + g) * sc + m])])
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let x_end = 0.15f64;
        let (xa, ya, za) = self.anchors(&mut r);
        let nx = xa.len();
        let rows = if self.fork { 2 * (self.n_cuts + 1) } else { 3 };
        let mut m = Mesh::new();
        let lin: Vec<f64> = (0..rows).map(|j| if rows == 1 { 1.0 } else { 1.0 + (-2.0) * j as f64 / (rows - 1) as f64 }).collect();
        for j in 0..rows {
            for k in 0..nx {
                let (mut x, mut y, mut z) = (xa[k], 0.0, za[k]);
                if self.fork {
                    y = ya[k] * lin[j];
                } else {
                    y = match j {
                        0 => ya[k],
                        1 => 0.0,
                        _ => -ya[k],
                    };
                    if j == 1 && k == 0 {
                        x += 0.02;
                    }
                    if j == 1 && k == 1 {
                        z = -self.z_depth;
                    }
                }
                m.pos.push([x as f32 as f64, y as f32 as f64, z as f32 as f64]);
            }
        }
        m.faces = grid(nx, rows);
        if self.fork {
            // make_cuts: delete the first-column faces whose front-vertex index is odd
            let mut keep = vec![];
            for (fi, f) in m.faces.iter().enumerate() {
                let j = fi / (nx - 1);
                let col = fi % (nx - 1);
                if col == 0 && j % 2 == 1 {
                    let _ = f;
                    continue;
                }
                keep.push(f.clone());
            }
            m.faces = keep;
        }
        let s = books::solidify_simple(&m, self.thickness as f32 as f64, None);
        let s = subsurf(&s, 1, false);
        let s = add_guard(&s, self.has_guard, self.double, x_end as f32, self.guard_depth as f32);
        let mut s = subsurf(&s, if self.fork { 1 } else { 2 }, false);
        let sc = self.scale as f32;
        for p in s.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc) as f64;
            }
        }
        let mat = spec_material("cutlery", json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.fs}), [0.75, 0.75, 0.75]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &mat, Some(b));
        a
    }
}

pub fn make(c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    if c == "KnifeFactory" {
        return Some(Box::new(Knife::new(fs)));
    }
    Some(Box::new(Cutlery::new(c == "ForkFactory", fs)))
}


pub struct Knife {
    x_length: f64,
    has_guard: bool,
    y_length: f64,
    y_guard: f64,
    x_guard: f64,
    has_tip: bool,
    thickness: f64,
    y_offset: f64,
    double: bool,
    guard_depth: f64,
    scale: f64,
    fs: u64,
}

impl Knife {
    fn new(fs: u64) -> Knife {
        let mut r = Rng::seeded(fs);
        let x_length = r.log_uniform(0.4, 0.7);
        let has_guard = r.uniform(0.0, 1.0) < 0.7;
        let (y_length, y_guard);
        if has_guard {
            y_length = r.log_uniform(0.1, 0.5);
            y_guard = y_length * r.log_uniform(0.2, 0.4);
        } else {
            y_length = r.log_uniform(0.1, 0.2);
            y_guard = y_length * r.log_uniform(0.3, 0.5);
        }
        let x_guard = r.uniform(0.0, 0.2);
        let has_tip = r.uniform(0.0, 1.0) < 0.7;
        let thickness = r.log_uniform(0.02, 0.03);
        let y_off_rand = r.uniform(0.0, 1.0);
        let y_offset = if y_off_rand < 1.0 / 8.0 {
            0.2
        } else if y_off_rand < 1.0 / 4.0 {
            0.5
        } else {
            r.uniform(0.2, 0.6)
        };
        let double = !(r.uniform(0.0, 1.0) < 0.6);
        let guard_depth = r.log_uniform(0.2, 1.0) * thickness;
        let scale = r.log_uniform(0.2, 0.3);
        Knife { x_length, has_guard, y_length, y_guard, x_guard, has_tip, thickness, y_offset, double, guard_depth, scale, fs }
    }
}

impl Factory for Knife {
    /// collision: the blade and handle outline (x from the handle's end to the tip, y from the spine to the widest of blade and guard,
    /// the edge offset in), the solidify's thickness and the guard's extrusion round it, scaled
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let mut r = inst_rng(self.fs, i);
        let x_end = 0.5f64;
        r.uniform(0.5, 0.8);
        r.uniform(0.3, 0.4);
        let ya1 = self.y_length * r.log_uniform(0.75, 0.95);
        let off = self.y_offset * self.y_length;
        let g = self.thickness + self.guard_depth + 0.004;
        let y1 = self.y_length.max(self.y_guard).max(ya1 + 3e-3) + off.max(0.0);
        let y0 = off.min(0.0);
        let (sc, m) = (self.scale, 0.002);
        Some(vec![crate::piece::Volume::hard([(-x_end * self.x_length - g) * sc - m, (y0 - g) * sc - m, -g * sc - m], [(x_end + 1e-3 + g) * sc + m, (y1 + g) * sc + m, g * sc + m])])
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let x_end = 0.5f64;
        let xa1 = r.uniform(0.5, 0.8) * x_end;
        let xa2 = r.uniform(0.3, 0.4) * x_end;
        let mut xa = vec![x_end, xa1, xa2, 1e-3, 0.0, -1e-3, -2e-3, -x_end * self.x_length + 1e-3, -x_end * self.x_length];
        let ya1 = self.y_length * r.log_uniform(0.75, 0.95);
        let mut ya = vec![1e-3, ya1, self.y_length, self.y_length, self.y_length, self.y_guard, self.y_guard, self.y_guard, self.y_guard];
        if !self.has_guard {
            let idx = [0usize, 1, 2, 4, 5, 7, 8];
            xa = idx.iter().map(|&k| xa[k]).collect();
            ya = idx.iter().map(|&k| ya[k]).collect();
        }
        if self.has_tip {
            let mut nx = vec![xa[0]];
            nx.extend(xa.iter());
            let mut ny = vec![ya[0]];
            ny.extend(ya.iter());
            xa = nx;
            ya = ny;
            xa[0] += 1e-3;
            ya[1] += 3e-3;
        }
        let n = xa.len();
        let mut x: Vec<f64> = vec![];
        let mut y: Vec<f64> = vec![];
        for row in 0..2 {
            for k in 0..n {
                x.push(xa[k]);
                y.push(if row == 0 { ya[k] } else { 0.0 });
            }
        }
        let off = self.y_offset * self.y_length;
        y[0] += off;
        y[n] += off;
        if self.has_tip {
            y[1] += off;
            y[n + 1] += off;
            let d = self.y_offset * (self.y_length - ya[2]);
            y[2] += d;
            y[n + 2] += d;
        } else {
            let d = self.y_offset * (self.y_length - ya[1]);
            y[1] += d;
            y[n + 1] += d;
        }
        let mut m = Mesh::new();
        for k in 0..2 * n {
            m.pos.push([x[k] as f32 as f64, y[k] as f32 as f64, 0.0]);
        }
        m.faces = grid(n, 2);
        let mut s = books::solidify_simple(&m, self.thickness as f32 as f64, None);
        // make_knife_tip: merge the top / bottom copies of every blade-edge vertex with x >= 0 and y > y_offset * y_length
        let thr = off as f32;
        let nn = 2 * n;
        let mut target: Vec<u32> = (0..s.pos.len() as u32).collect();
        for v in 0..nn {
            let (xv, yv) = (s.pos[v][0] as f32, s.pos[v][1] as f32);
            if xv >= 0.0 && yv > thr {
                let w = v + nn;
                let (a, b) = (s.pos[v], s.pos[w]);
                let mid = [((a[0] as f32 + b[0] as f32) / 2.0) as f64, ((a[1] as f32 + b[1] as f32) / 2.0) as f64, ((a[2] as f32 + b[2] as f32) / 2.0) as f64];
                s.pos[v] = mid;
                target[w] = v as u32;
            }
        }
        let mut faces = vec![];
        for f in &s.faces {
            let mut u: Vec<u32> = vec![];
            for &q in f {
                let t = target[q as usize];
                if u.last() != Some(&t) {
                    u.push(t);
                }
            }
            if u.len() > 1 && u[0] == *u.last().unwrap() {
                u.pop();
            }
            let mut uq = u.clone();
            uq.sort();
            uq.dedup();
            if u.len() >= 3 && uq.len() >= 3 {
                faces.push(u);
            }
        }
        s.faces = faces;
        // drop vertices no longer used (merged copies)
        let mut used = vec![false; s.pos.len()];
        for f in &s.faces {
            for &q in f {
                used[q as usize] = true;
            }
        }
        let mut remap = vec![u32::MAX; s.pos.len()];
        let mut np = vec![];
        for (k, p) in s.pos.iter().enumerate() {
            if used[k] {
                remap[k] = np.len() as u32;
                np.push(*p);
            }
        }
        for f in s.faces.iter_mut() {
            for q in f.iter_mut() {
                *q = remap[*q as usize];
            }
        }
        s.pos = np;
        let s = subsurf(&s, 1, false);
        let s = add_guard_x(&s, self.has_guard, self.double, (-self.x_guard * self.x_length * x_end) as f32, self.guard_depth as f32);
        let s = subsurf(&s, 1, false);
        let mut s = subsurf(&s, 1, true);
        let sc = self.scale as f32;
        for p in s.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc) as f64;
            }
        }
        let mat = spec_material("knife", json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.fs}), [0.75, 0.75, 0.75]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &mat, Some(b));
        a
    }
}
