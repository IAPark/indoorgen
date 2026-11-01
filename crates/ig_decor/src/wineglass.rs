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
        let mut w = Wineglass { x_end, z_length, z_cup, z_mid, x_neck, x_top, x_mid, thickness, scale, fs };
        // built to the layout box [width (y), depth (x), height]: round, the smaller of width and depth is the diameter. The scale
        // gives the width; the stem and bowl heights (z_length, z_cup, z_mid) follow the height.
        if let Some(h) = crate::api::size_hint() {
            let d = h[0].min(h[1]);
            for _ in 0..3 {
                let e = w.extents();
                w.scale *= d / e[0].max(e[1]);
                let e = w.extents();
                let zf = (h[2] / e[2]).max(0.05);
                w.z_length *= zf;
                w.z_cup *= zf;
                w.z_mid *= zf;
            }
        }
        w
    }

    /// bounding box of the revolved profile (the solidify shell only adds the wall thickness below); the stem-foot height is the
    /// instance's draw, taken here for instance 0
    fn outline(&self, i: u64) -> (V3, V3) {
        let mut r = inst_rng(self.fs, i);
        let z_bottom = self.z_length * r.log_uniform(0.01, 0.05);
        let xs = [self.x_end, self.x_end / 2.0, self.x_neck, self.x_neck, self.x_mid, self.x_top];
        let zs = [0.0, z_bottom / 2.0, z_bottom, self.z_cup, self.z_mid, self.z_length];
        let anchors: Vec<[f64; 3]> = (0..6).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[0, 1, 2, 3]);
        let (mut lo, mut hi) = m.bounds();
        let t = self.thickness as f32 as f64;
        lo[2] -= t;
        let sc = self.scale as f32;
        let f = |v: f64| (v as f32 * sc) as f64;
        ([f(lo[0]), f(lo[1]), f(lo[2])], [f(hi[0]), f(hi[1]), f(hi[2])])
    }

    fn extents(&self) -> [f64; 3] {
        let (lo, hi) = self.outline(0);
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }
}

impl Factory for Wineglass {
    fn natural_size(&self, i: u64) -> Option<[f64; 3]> {
        let (lo, hi) = self.outline(i);
        Some([hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]])
    }
    /// dense mode: outline of the revolved profile (the solidify shell only adds the wall thickness)
    fn bounds_hint(&self, i: u64) -> Option<(V3, V3)> {
        if !crate::dense() {
            return None;
        }
        Some(self.outline(i))
    }
    /// collision: the revolved profile's outline, grown by the glass
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = self.outline(i);
        let g = self.thickness * self.scale + 0.002;
        Some(vec![crate::piece::Volume::hard([lo[0] - g, lo[1] - g, lo[2] - g], [hi[0] + g, hi[1] + g, hi[2] + g])])
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wineglass_hint_matches_asset() {
        std::env::set_var("INFINIGEN_DENSE", "1");
        let (mut worst, mut sum) = (0.0f64, 0.0);
        for fs in 0..40u64 {
            let j = Wineglass::new(fs);
            let a = j.asset(3).bounds();
            let (lo, hi) = j.bounds_hint(3).unwrap();
            let e = (0..3).map(|k| (lo[k] - a.0[k]).abs().max((hi[k] - a.1[k]).abs())).fold(0.0, f64::max);
            if e > 0.005 { eprintln!("fs {fs} err {e:.4} full {:?} {:?} hint {:?} {:?}", a.0, a.1, lo, hi); }
            worst = worst.max(e);
            sum += e;
        }
        eprintln!("wineglass hint: worst {worst:.4} mean {:.4}", sum / 40.0);
    }
}
