//! tableware/cup.py CupFactory (handle-less path exact; the handle boolean is not ported)
use super::bevel::{bevel, Offset};
use super::blops::*;
use super::matdraw;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "CupFactory"
}

pub struct Cup {
    x_end: f64,
    straight: bool,
    x_lowest: f64,
    depth: f64,
    has_guard: bool,
    thickness: f64,
    x_lower_ratio: f64,
    wrap_margin: f64,
    scale: f64,
    fs: u64,
}

impl Cup {
    fn new(fs: u64) -> Cup {
        let mut r = Rng::seeded(fs);
        let x_end = 0.25;
        let is_short = r.uniform(0.0, 1.0) < 0.5;
        let (straight, x_lowest, depth, has_guard);
        if is_short {
            straight = r.uniform(0.0, 1.0) < 0.2;
            x_lowest = r.log_uniform(0.6, 0.9);
            depth = r.log_uniform(0.25, 0.5);
            has_guard = r.uniform(0.0, 1.0) < 0.8;
        } else {
            straight = true;
            x_lowest = r.log_uniform(0.9, 1.0);
            depth = r.log_uniform(0.5, 1.0);
            has_guard = false;
        }
        let _handle_location = if straight { r.uniform(0.45, 0.65) } else { r.uniform(-0.1, 0.3) };
        let _handle_type = r.uniform(0.0, 1.0) < 0.5;
        let handle_radius = depth * r.uniform(0.2, 0.4);
        let _inner = handle_radius * r.log_uniform(0.2, 0.3);
        let _tx = r.uniform(0.0, 2.0);
        let _ty = r.uniform(0.0, 2.0);
        let x_lower_ratio = r.log_uniform(0.8, 1.0);
        let thickness = r.log_uniform(0.01, 0.04);
        let _has_wrap = r.uniform(0.0, 1.0) < 0.3;
        let wrap_margin = r.uniform(0.1, 0.2);
        matdraw::sample_call(&mut r, matdraw::GRAPHICDESIGN);
        let _has_inside = r.uniform(0.0, 1.0) < 0.5;
        let scale = r.log_uniform(0.15, 0.3);
        Cup { x_end, straight, x_lowest, depth, has_guard, thickness, x_lower_ratio, wrap_margin, scale, fs }
    }
}

impl Factory for Cup {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let sc = self.scale;
        let (xs, zs): (Vec<f64>, Vec<f64>) = if self.straight {
            (vec![0.0, self.x_lowest * self.x_end, self.x_end], vec![0.0, 0.0, self.depth])
        } else {
            (
                vec![0.0, self.x_lowest * self.x_end, (self.x_lowest + self.x_lower_ratio * (1.0 - self.x_lowest)) * self.x_end, self.x_end],
                vec![0.0, 0.0, self.depth * 0.5, self.depth],
            )
        };
        let anchors: Vec<[f64; 3]> = xs.iter().zip(zs.iter()).map(|(&x, &z)| [x * sc, 0.0, z * sc]).collect();
        let mut m = spin_anchors(&anchors, &[1]);
        let inv = (1.0 / sc) as f32;
        for p in m.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * inv) as f64;
            }
        }
        let pct = r.uniform(10.0, 50.0);
        let m = bevel(&m, Offset::Percent(pct), 8, 30f64.to_radians()).unwrap_or(m);
        // make_wrap: clone, drop vertices (margins / angular window), scale x,y by 1.01
        let thr = r.uniform(0.0, 0.1);
        let keep: Vec<bool> = m
            .pos
            .iter()
            .map(|p| {
                let (x, y, z) = (p[0], p[1], p[2]);
                let bad = (z / self.depth < self.wrap_margin) || (z / self.depth > 1.0 - self.wrap_margin + thr) || (y.atan2(x).abs() < std::f64::consts::PI * self.wrap_margin);
                !bad
            })
            .collect();
        let mut wrap = Mesh::new();
        let mut map = vec![u32::MAX; m.pos.len()];
        for (k, p) in m.pos.iter().enumerate() {
            if keep[k] {
                map[k] = wrap.pos.len() as u32;
                wrap.pos.push([(p[0] as f32 * 1.01f32) as f64, (p[1] as f32 * 1.01f32) as f64, p[2]]);
            }
        }
        for f in &m.faces {
            if f.iter().all(|&v| keep[v as usize]) {
                wrap.faces.push(f.iter().map(|&v| map[v as usize]).collect());
            }
        }
        let s = books::solidify_simple(&m, -(self.thickness as f32 as f64), None);
        let s = subsurf(&s, 2, false);
        let mut all = Mesh::new();
        all.append(&s);
        all.append(&wrap);
        let sc32 = sc as f32;
        for p in all.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc32) as f64;
            }
        }
        let mat = spec_material("cup", json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.fs}), [0.85, 0.85, 0.82]);
        let mut a = Asset::default();
        let b = all.bounds();
        a.mesh(all, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Cup::new(fs)))
}
