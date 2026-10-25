//! tableware/wineglass.py WineglassFactory: spin + SOLIDIFY (exact vertex arrays, faces equal up to ordering)
use super::blops::*;
use super::matdraw;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "WineglassFactory"
}

pub struct Wineglass {
    x_end: f64,
    z_length: f64,
    z_cup: f64,
    z_mid: f64,
    x_neck: f64,
    x_top: f64,
    x_mid: f64,
    thickness: f64,
    scale: f64,
    fs: u64,
}

impl Wineglass {
    fn new(fs: u64) -> Wineglass {
        let mut r = Rng::seeded(fs);
        let x_end = 0.25;
        let z_length = r.log_uniform(0.6, 2.0);
        let z_cup = r.uniform(0.3, 0.6) * z_length;
        let z_mid = z_cup + r.uniform(0.3, 0.5) * (z_length - z_cup);
        let x_neck = r.log_uniform(0.01, 0.02);
        let x_top = x_end * r.log_uniform(1.0, 1.4);
        let x_mid = x_top * r.log_uniform(0.9, 1.2);
        let thickness = r.uniform(0.01, 0.03);
        r.choice_p(&[1.0]); // weighted_sample(material_assignments.glasses)()() : one class, no further draws
        let scale = r.log_uniform(0.1, 0.3);
        if std::env::var("WGDBG").is_ok() { eprintln!("ATTR {} {} {} {} {} {} {} {}", z_length, z_cup, z_mid, x_neck, x_top, x_mid, thickness, scale); }
        Wineglass { x_end, z_length, z_cup, z_mid, x_neck, x_top, x_mid, thickness, scale, fs }
    }
}

impl Factory for Wineglass {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let z_bottom = self.z_length * r.log_uniform(0.01, 0.05);
        let xs = [self.x_end, self.x_end / 2.0, self.x_neck, self.x_neck, self.x_mid, self.x_top];
        let zs = [0.0, z_bottom / 2.0, z_bottom, self.z_cup, self.z_mid, self.z_length];
        let anchors: Vec<[f64; 3]> = (0..6).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[0, 1, 2, 3]);
        let mut s = books::solidify_simple(&m, self.thickness as f32 as f64, None);
        let sc = self.scale as f32;
        for p in s.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc) as f64;
            }
        }
        let glass = spec_material("wineglass", json!({"cls": "ceramic.Glass", "kwargs": {}, "seed": self.fs}), [0.9, 0.95, 1.0]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &glass, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Wineglass::new(fs)))
}
