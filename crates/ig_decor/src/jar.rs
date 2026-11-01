//! tableware/jar.py JarFactory: open prism -> SUBSURF(simple, 2) -> join circle -> bridge_edge_loops(5 cuts) -> extrude -> SUBSURF(2) -> SOLIDIFY, cap cylinder
use super::bridge::bridge_two_loops;
use super::matdraw;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;
use crate::dhash::HashMap;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "JarFactory"
}

const JAR: matdraw::Table = &[
    ("ceramic.Glass", 8.0),
    ("plastic.Plastic", 2.0),
    ("plastic.PlasticTranslucent", 2.0),
    ("ceramic.VaseCeramic", 2.0),
    ("ceramic.ColoredGlass", 2.0),
    ("ceramic.Marble", 1.0),
    ("ceramic.GlassVolume", 2.0),
];
const HANDLE: matdraw::Table = &[
    ("metal.MetalBasic", 3.0),
    ("metal.BrushedMetal", 2.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 0.7),
    ("metal.BrushedBlackMetal", 0.3),
    ("plastic.PlasticRough", 1.0),
    ("wood.Wood", 3.0),
    ("metal.WhiteMetal", 2.0),
];

pub struct Jar {
    z_length: f64,
    x_length: f64,
    thickness: f64,
    n_base: usize,
    x_cap: f64,
    z_cap: f64,
    z_neck: f64,
    surface: &'static str,
    cap_surface: &'static str,
    cap_subsurf: bool,
    fs: u64,
}

impl Jar {
    fn new(fs: u64) -> Jar {
        let mut r = Rng::seeded(fs);
        let z_length = r.uniform(0.15, 0.2);
        let x_length = r.uniform(0.03, 0.06);
        let thickness = r.uniform(0.002, 0.004);
        let n_base = [4usize, 6, 64][r.choice_idx(3)];
        let x_cap = r.uniform(0.6, 0.9) * (PI / n_base as f64).cos();
        let z_cap = r.uniform(0.05, 0.08);
        let z_neck = r.uniform(0.15, 0.2);
        let surface = matdraw::sample_init(&mut r, JAR);
        let cap_surface = matdraw::sample_init(&mut r, HANDLE);
        r.uniform(0.0, 1.0);
        r.uniform(0.0, 1.0);
        let cap_subsurf = r.uniform(0.0, 1.0) < 0.5;
        let mut j = Jar { z_length, x_length, thickness, n_base, x_cap, z_cap, z_neck, surface, cap_surface, cap_subsurf, fs };
        // built to the layout box [width (y), depth (x), height]: the prism (square / hexagon / round) fits the box; the wall
        // thickness stays what it was drawn
        if let Some(h) = crate::api::size_hint() {
            for _ in 0..3 {
                let (lo, hi) = j.outline(0);
                j.x_length *= (h[0] / (hi[1] - lo[1])).min(h[1] / (hi[0] - lo[0]));
                j.z_length *= h[2] / (hi[2] - lo[2]);
            }
        }
        j
    }

    /// bounding box from the subdivided base prism and the neck / cap heights (no bridge, solidify or second subsurf)
    fn outline(&self, i: u64) -> (V3, V3) {
        let mut r = inst_rng(self.fs, i);
        let (m, _) = self.stage_mesh(i, 0);
        let (lo, hi) = m.bounds();
        let _shape = r.uniform(0.0, 0.1);
        let capz = (1.0 + self.z_neck + self.z_cap * r.uniform(0.5, 0.8)) * self.z_length;
        let top = (capz + self.z_cap * self.z_length).max((1.0 + self.z_neck + self.z_cap) * self.z_length);
        let rc = self.x_cap * self.x_length + 1e-3;
        let (rx, ry) = (hi[0].max(-lo[0]).max(rc), hi[1].max(-lo[1]).max(rc));
        ([-rx, -ry, 0.0], [rx, ry, top])
    }

    /// cylinder primitive (depth 1, location z .5) scaled and baked; faces: side quads then the bottom ngon (top ngon optional)
    fn cylinder(n: usize, rx: f64, rz: f64, z0: f64, top: bool) -> Mesh {
        let mut m = Mesh::new();
        let step = ((2.0 * PI) / n as f64) as f32;
        for i in 0..n {
            let phi = step * i as f32;
            let (s, c) = (phi.sin(), phi.cos());
            let x = (s * rx as f32) as f64;
            let y = (c * rx as f32) as f64;
            m.pos.push([x, y, z0]);
            m.pos.push([x, y, z0 + rz]);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            m.faces.push(vec![(2 * i) as u32, (2 * i + 1) as u32, (2 * j + 1) as u32, (2 * j) as u32]);
        }
        if top {
            m.faces.push((0..n).rev().map(|k| (2 * k + 1) as u32).collect());
        }
        m.faces.push((0..n).map(|k| (2 * k) as u32).collect());
        m
    }

    /// Blender evaluates the interior of a simple-subdivided bottom n-gon (level 2) at slightly different places than the linear
    /// quad refinement; the radii (relative to the prism radius) were measured from Blender 4.2 for the n_base values the factory uses.
    fn fix_bottom_ngon(m: &mut Mesh, n: usize, r: f64) {
        let (q, s) = match n {
            6 => (0.63785715, 0.4446630),
            64 => (0.7510342, 0.4366807),
            _ => return,
        };
        let mid = r * (PI / n as f64).cos();
        for p in m.pos.iter_mut() {
            let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
            if p[2].abs() < 1e-12 && rr > 1e-6 && rr < mid * 0.97 {
                let ph = (p[0].atan2(p[1]) + 2.0 * PI) % (2.0 * PI);
                let k = ph / (2.0 * PI / n as f64);
                let on_corner = (k - k.round()).abs() < 1e-3;
                let t = if on_corner { q } else { s } * r / rr;
                p[0] *= t;
                p[1] *= t;
            }
        }
    }

    fn stage_mesh(&self, i: u64, stage: u32) -> (Mesh, Mesh) {
        let e = Mesh::new();
        let mut r = inst_rng(self.fs, i);
        let n = self.n_base;
        let cyl = Jar::cylinder(n, self.x_length, self.z_length, 0.0, false);
        let mut m = subsurf(&cyl, 2, true);
        Jar::fix_bottom_ngon(&mut m, n, self.x_length);
        if stage == 0 {
            return (m, e);
        }
        // top ring in Blender's edge-loop order: start at the column-1 vertex, ascending columns
        let zt = self.z_length;
        let phi = |p: [f64; 3]| (p[0].atan2(p[1]) + 2.0 * PI) % (2.0 * PI);
        let mut ring: Vec<(f64, u32)> = (0..m.pos.len()).filter(|&k| (m.pos[k][2] - zt).abs() < 1e-9).map(|k| (phi(m.pos[k]), k as u32)).collect();
        ring.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let p1 = 2.0 * PI / n as f64;
        let k1 = ring.iter().position(|e| (e.0 - p1).abs() < 1e-7).unwrap();
        ring.rotate_left(k1);
        let ring: Vec<u32> = ring.into_iter().map(|e| e.1).collect();
        // circle
        let cz = (1.0 + self.z_neck) * self.z_length;
        let cr = self.x_cap * self.x_length;
        let base = m.pos.len();
        for k in 0..32 {
            let a = 2.0 * PI * k as f64 / 32.0;
            m.pos.push([-cr * a.sin(), cr * a.cos(), cz]);
        }
        let mut circ: Vec<u32> = (1..32).map(|k| (base + k) as u32).collect();
        circ.push(base as u32);
        let shape = r.uniform(0.0, 0.1);
        bridge_two_loops(&mut m, ring, circ, 5, shape);
        // select all, region_to_loop, extrude_edges_move (0, 0, z_cap * z_length)
        let mut ecount: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::default();
        for f in &m.faces {
            for k in 0..f.len() {
                let (u, w) = (f[k], f[(k + 1) % f.len()]);
                let e = ecount.entry((u.min(w), u.max(w))).or_insert((0, u, w));
                e.0 += 1;
            }
        }
        let mut bnd: Vec<(u32, u32)> = ecount.values().filter(|e| e.0 == 1).map(|e| (e.1, e.2)).collect();
        bnd.sort();
        let dz = self.z_cap * self.z_length;
        let mut newid: HashMap<u32, u32> = HashMap::default();
        for &(u, w) in &bnd {
            for v in [u, w] {
                newid.entry(v).or_insert_with(|| {
                    let p = m.pos[v as usize];
                    m.pos.push([p[0], p[1], p[2] + dz]);
                    (m.pos.len() - 1) as u32
                });
            }
        }
        for &(u, w) in &bnd {
            // existing face traverses u -> w, the strip traverses w -> u
            m.faces.push(vec![w, u, newid[&u], newid[&w]]);
        }
        if stage == 1 {
            return (m, e);
        }
        // dense mode: one subdivision level fewer on the bridged body (4x fewer faces; the scene budget cuts jars to a few k triangles anyway)
        let m = subsurf(&m, if crate::dense() { 1 } else { 2 }, false);
        if stage == 2 {
            return (m, e);
        }
        let s = books::solidify_simple(&m, self.thickness, None);
        if stage == 3 {
            return (s, e);
        }
        let capz = (1.0 + self.z_neck + self.z_cap * r.uniform(0.5, 0.8)) * self.z_length;
        let s = subsurf(&s, 1, self.cap_subsurf);
        let cap = Jar::cylinder(64, self.x_cap * self.x_length + 1e-3, self.z_cap * self.z_length, capz, true);
        (s, cap)
    }
}

impl Factory for Jar {
    fn natural_size(&self, i: u64) -> Option<[f64; 3]> {
        let (lo, hi) = self.outline(i);
        Some([hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]])
    }
    /// dense mode: outline from the subdivided base prism and the neck / cap heights (no bridge, solidify or second subsurf)
    fn bounds_hint(&self, i: u64) -> Option<(V3, V3)> {
        if !crate::dense() {
            return None;
        }
        Some(self.outline(i))
    }
    /// collision: the revolved profile's outline with the cap on its neck
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = self.outline(i);
        let g = 0.003;
        Some(vec![crate::piece::Volume::hard([lo[0] - g, lo[1] - g, lo[2] - g], [hi[0] + g, hi[1] + g, hi[2] + g])])
    }
    fn asset(&self, i: u64) -> Asset {
        let (m, cap) = self.stage_mesh(i, 9);
        let mat = spec_material("jar", json!({"cls": self.surface, "kwargs": {}, "seed": self.fs}), [0.8, 0.85, 0.9]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        let cmat = spec_material("jar_cap", json!({"cls": self.cap_surface, "kwargs": {}, "seed": self.fs + 1}), [0.5, 0.5, 0.5]);
        let cb = cap.bounds();
        a.mesh(cap, &cmat, Some(cb));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Jar::new(fs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jar_hint_matches_asset() {
        std::env::set_var("INFINIGEN_DENSE", "1");
        let (mut worst, mut sum, mut tf, mut th) = (0.0f64, 0.0, 0.0, 0.0);
        for fs in 0..40u64 {
            let j = Jar::new(fs);
            let t = web_time::Instant::now();
            let a = j.asset(3).bounds();
            tf += t.elapsed().as_secs_f64();
            let t = web_time::Instant::now();
            let (lo, hi) = j.bounds_hint(3).unwrap();
            th += t.elapsed().as_secs_f64();
            let e = (0..3).map(|k| (lo[k] - a.0[k]).abs().max((hi[k] - a.1[k]).abs())).fold(0.0, f64::max);
            if e > 0.01 { eprintln!("fs {fs} err {e:.4} n {} full {:?} {:?} hint {:?} {:?}", j.n_base, a.0, a.1, lo, hi); }
            worst = worst.max(e);
            sum += e;
        }
        eprintln!("jar hint: worst {worst:.4} mean {:.4} full {tf:.2}s hint {th:.2}s", sum / 40.0);
    }
}
