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
    /// dense mode: outline of the revolved profile (no subsurf / solidify), padded by the shell thickness
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        if !crate::dense() {
            return None;
        }
        let sc = self.scale;
        let xs = [0.0, self.x_mid * sc, self.x_mid * sc, self.x_end * sc];
        let zs = [0.0, 0.0, self.z_mid * sc, self.z_length * sc];
        let anchors: Vec<[f64; 3]> = (0..4).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[1, 2]);
        let (mut lo, mut hi) = m.bounds();
        let t = self.thickness as f32 as f64;
        lo[2] -= t;
        hi[2] += 0.0;
        Some((lo, hi))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plate_hint_matches_asset() {
        std::env::set_var("INFINIGEN_DENSE", "1");
        let (mut worst, mut sum) = (0.0f64, 0.0);
        for fs in 0..40u64 {
            let j = Plate::new(fs);
            let a = j.asset(3).bounds();
            let (lo, hi) = j.bounds_hint(3).unwrap();
            let e = (0..3).map(|k| (lo[k] - a.0[k]).abs().max((hi[k] - a.1[k]).abs())).fold(0.0, f64::max);
            if e > 0.005 { eprintln!("fs {fs} err {e:.4} full {:?} {:?} hint {:?} {:?}", a.0, a.1, lo, hi); }
            worst = worst.max(e);
            sum += e;
        }
        eprintln!("plate hint: worst {worst:.4} mean {:.4}", sum / 40.0);
    }
}
