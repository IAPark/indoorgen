//! tableware/plate.py PlateFactory: spin -> SUBSURF(1) -> SOLIDIFY(offset +1) -> subsurf(1)
use super::blops::*;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "PlateFactory"
}

pub struct Plate {
    x_end: f64,
    z_length: f64,
    x_mid: f64,
    z_mid: f64,
    scale: f64,
    thickness: f64,
    fs: u64,
}

impl Plate {
    fn new(fs: u64) -> Plate {
        let mut r = Rng::seeded(fs);
        let x_end = 0.5;
        let z_length = r.log_uniform(0.05, 0.2);
        let x_mid = r.uniform(0.3, 1.0) * x_end;
        let z_mid = r.uniform(0.3, 0.8) * z_length;
        let _has_inside = r.uniform(0.0, 1.0) < 0.2;
        let scale = r.log_uniform(0.2, 0.4);
        let thickness = r.uniform(0.01, 0.03) * scale;
        Plate { x_end, z_length, x_mid, z_mid, scale, thickness, fs }
    }
}

impl Factory for Plate {
    fn asset(&self, _i: u64) -> Asset {
        let sc = self.scale;
        let xs = [0.0, self.x_mid * sc, self.x_mid * sc, self.x_end * sc];
        let zs = [0.0, 0.0, self.z_mid * sc, self.z_length * sc];
        let anchors: Vec<[f64; 3]> = (0..4).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[1, 2]);
        let m = subsurf(&m, 1, false);
        let sign = if std::env::var("PLATESIGN").is_ok() { -1.0 } else { 1.0 };
        let s = books::solidify_simple(&m, -sign * self.thickness as f32 as f64, None);
        let s = subsurf(&s, 1, false);
        let mat = spec_material("plate", json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.fs}), [0.85, 0.85, 0.82]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Plate::new(fs)))
}
