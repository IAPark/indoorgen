//! tableware/bottle.py BottleFactory: spin + subsurf(1) + SOLIDIFY, label wrap cylinder, cap cylinder (subsurf or simple subdivision)
use super::blops::*;
use super::matdraw;
use super::subsurf::subsurf;
use super::textdraw::text_ctor_and_generate;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "BottleFactory"
}

/// bpy.ops.mesh.primitive_cylinder_add(vertices=n) with depth 1 moved to z in [0,1] (applied); caps optional
pub fn unit_cylinder(n: usize, caps: bool) -> Mesh {
    let mut m = Mesh::new();
    let step = ((2.0 * std::f64::consts::PI) / n as f64) as f32;
    for i in 0..n {
        let phi = step * i as f32;
        let (s, c) = (phi.sin(), phi.cos());
        m.pos.push([s as f64, c as f64, 0.0]);
        m.pos.push([s as f64, c as f64, 1.0]);
    }
    let q = |i: usize| -> Vec<u32> {
        let j = (i + 1) % n;
        vec![(2 * i) as u32, (2 * i + 1) as u32, (2 * j + 1) as u32, (2 * j) as u32]
    };
    for i in 0..n.saturating_sub(2) {
        m.faces.push(q(i));
    }
    if caps {
        let mut top = vec![3u32, 1];
        top.extend((2..n).rev().map(|k| (2 * k + 1) as u32));
        m.faces.push(top);
    }
    m.faces.push(q(n - 2));
    m.faces.push(q(n - 1));
    if caps {
        m.faces.push((0..n).map(|k| (2 * k) as u32).collect());
    }
    m
}

pub struct Bottle {
    z_length: f64,
    x_length: f64,
    x_cap: f64,
    bottle_width: f64,
    z_neck: f64,
    z_waist: f64,
    z_cap: f64,
    x_anchors: Vec<f64>,
    z_anchors: Vec<f64>,
    is_vector: Vec<usize>,
    cap_subsurf: bool,
    fs: u64,
}

impl Bottle {
    fn new(fs: u64) -> Bottle {
        let mut r = Rng::seeded(fs);
        let z_length = r.uniform(0.15, 0.25);
        let x_length = z_length * r.uniform(0.15, 0.25);
        let x_cap = r.uniform(0.3, 0.35);
        let ty = r.randint(0, 5);
        let bottle_width = r.uniform(0.002, 0.005);
        let mut z_waist = 0.0;
        let (z_neck, z_cap, x_anchors, z_anchors, is_vec): (f64, f64, Vec<f64>, Vec<f64>, Vec<i32>);
        match ty {
            0 => {
                // beer
                let zn = r.uniform(0.5, 0.6);
                let zc = r.uniform(0.05, 0.08);
                let neck_size = r.uniform(0.06, 0.1);
                let neck_ratio = r.uniform(0.4, 0.5);
                x_anchors = vec![0.0, 1.0, 1.0, (neck_ratio + 1.0) / 2.0 + (1.0 - neck_ratio) / 2.0 * x_cap, neck_ratio + (1.0 - neck_ratio) * x_cap, x_cap, x_cap, 0.0];
                z_anchors = vec![0.0, 0.0, zn, zn + r.uniform(0.6, 0.7) * neck_size, zn + neck_size, 1.0 - zc, 1.0, 1.0];
                is_vec = vec![0, 1, 1, 0, 1, 1, 1, 0];
                z_neck = zn;
                z_cap = zc;
            }
            1 => {
                // bordeaux
                let zn = r.uniform(0.6, 0.7);
                let zc = r.uniform(0.1, 0.15);
                let neck_size = r.uniform(0.1, 0.15);
                x_anchors = vec![0.0, 1.0, 1.0, (1.0 + x_cap) / 2.0, x_cap, x_cap, 0.0];
                z_anchors = vec![0.0, 0.0, zn, zn + r.uniform(0.6, 0.7) * neck_size, zn + neck_size, 1.0, 1.0];
                is_vec = vec![0, 1, 1, 0, 1, 1, 0];
                z_neck = zn;
                z_cap = zc;
            }
            2 => {
                // champagne
                let zn = r.uniform(0.4, 0.5);
                let zc = r.uniform(0.05, 0.08);
                x_anchors = vec![0.0, 1.0, 1.0, 1.0, (1.0 + x_cap) / 2.0, x_cap, x_cap, 0.0];
                let a = zn + r.uniform(0.08, 0.1);
                let b = zn + r.uniform(0.15, 0.18);
                z_anchors = vec![0.0, 0.0, zn, a, b, 1.0 - zc, 1.0, 1.0];
                is_vec = vec![0, 1, 1, 0, 0, 1, 1, 0];
                z_neck = zn;
                z_cap = zc;
            }
            3 => {
                // coke
                z_waist = r.uniform(0.4, 0.5);
                let zn = z_waist + r.uniform(0.2, 0.25);
                let zc = r.uniform(0.05, 0.08);
                let x1 = r.uniform(0.85, 0.95);
                let x3 = r.uniform(0.85, 0.95);
                x_anchors = vec![0.0, x1, 1.0, x3, 1.0, 1.0, x_cap, x_cap, 0.0];
                let z2 = r.uniform(0.08, 0.12);
                let z3 = r.uniform(0.18, 0.25);
                z_anchors = vec![0.0, 0.0, z2, z3, z_waist, zn, 1.0 - zc, 1.0, 1.0];
                is_vec = vec![0, 1, 0, 0, 1, 1, 1, 1, 0];
                z_neck = zn;
                z_cap = zc;
            }
            _ => {
                // vintage
                z_waist = r.uniform(0.1, 0.15);
                let zn = r.uniform(0.7, 0.75);
                let zc = r.uniform(0.0, 0.08);
                let x_lower = r.uniform(0.85, 0.95);
                x_anchors = vec![0.0, x_lower, (x_lower + 1.0) / 2.0, 1.0, 1.0, (x_cap + 1.0) / 2.0, x_cap, x_cap, 0.0];
                let z2 = z_waist - r.uniform(0.1, 0.15);
                let z5 = zn + r.uniform(0.1, 0.2);
                z_anchors = vec![0.0, 0.0, z2, z_waist, zn, z5, 1.0 - zc, 1.0, 1.0];
                is_vec = vec![0, 1, 0, 1, 1, 0, 1, 1, 0];
                z_neck = zn;
                z_cap = zc;
            }
        }
        // surface = weighted_sample(plastics)()(); wrap = text.Text()(); cap = weighted_sample(metals)()()
        matdraw::sample_call(&mut r, matdraw::PLASTICS);
        text_ctor_and_generate(&mut r);
        matdraw::sample_call(&mut r, matdraw::METALS);
        r.uniform(0.0, 1.0); // texture_shared
        let cap_subsurf = r.uniform(0.0, 1.0) < 0.5;
        let is_vector: Vec<usize> = is_vec.iter().enumerate().filter(|(_, &v)| v != 0).map(|(i, _)| i).collect();
        Bottle { z_length, x_length, x_cap, bottle_width, z_neck, z_waist, z_cap, x_anchors, z_anchors, is_vector, cap_subsurf, fs }
    }
}

impl Factory for Bottle {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        // --- bottle body
        let anchors: Vec<[f64; 3]> = (0..self.x_anchors.len()).map(|k| [self.x_anchors[k] * self.x_length, 0.0, self.z_anchors[k] * self.z_length]).collect();
        let m = spin_anchors(&anchors, &self.is_vector);
        let m = subsurf(&m, 1, false);
        let body = books::solidify_simple(&m, self.bottle_width as f32 as f64, None);
        // --- label wrap: 128-gon open cylinder with 16 extra rings
        let z_max = self.z_neck - r.uniform(0.02, 0.05) * (self.z_neck - self.z_waist);
        let z_min = self.z_waist + r.uniform(0.02, 0.15) * (self.z_neck - self.z_waist);
        let max_x = body.pos.iter().map(|p| p[0] as f32).fold(f32::MIN, f32::max);
        let radius = (max_x as f64 + 2e-3) as f32;
        let h = ((z_max - z_min) * self.z_length) as f32;
        let loc = (z_min * self.z_length) as f32;
        let mut wrap = Mesh::new();
        let n = 128usize;
        let cyl = unit_cylinder(n, false);
        let rows = 18usize; // top, bottom and 16 cuts
        for k in 0..n {
            for row in 0..rows {
                let t = row as f32 / (rows - 1) as f32;
                let p = cyl.pos[2 * k];
                wrap.pos.push([(p[0] as f32 * radius) as f64, (p[1] as f32 * radius) as f64, ((t * h) + loc) as f64]);
            }
        }
        for k in 0..n {
            let k1 = (k + 1) % n;
            for row in 0..rows - 1 {
                wrap.faces.push(vec![(k * rows + row) as u32, (k * rows + row + 1) as u32, (k1 * rows + row + 1) as u32, (k1 * rows + row) as u32]);
            }
        }
        // --- cap
        let mut cap = unit_cylinder(128, true);
        let sx = ((self.x_cap + 0.1) * self.x_length) as f32;
        let sz = ((self.z_cap + 0.01) * self.z_length) as f32;
        let lz = ((1.0 - self.z_cap) * self.z_length) as f32;
        for p in cap.pos.iter_mut() {
            *p = [(p[0] as f32 * sx) as f64, (p[1] as f32 * sx) as f64, (p[2] as f32 * sz + lz) as f64];
        }
        let cap = subsurf(&cap, 1, self.cap_subsurf);
        let plastic = spec_material("bottle", json!({"cls": "plastic.Plastic", "kwargs": {}, "seed": self.fs}), [0.3, 0.5, 0.3]);
        let label = spec_material("bottle_label", json!({"cls": "text.Text", "kwargs": {}, "seed": self.fs}), [0.8, 0.7, 0.5]);
        let metal = spec_material("bottle_cap", json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.fs}), [0.6, 0.6, 0.6]);
        let mut a = Asset::default();
        for (m, mt) in [(body, &plastic), (wrap, &label), (cap, &metal)] {
            let b = m.bounds();
            a.mesh(m, mt, Some(b));
        }
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Bottle::new(fs)))
}
