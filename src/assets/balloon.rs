//! wall_decorations/balloon.py BalloonFactory: the placeholder (text bounding box) is RNG-exact; the asset (inflated text mesh from a cloth simulation) is
//! approximated by a flattened ellipsoid filling the placeholder.
use super::matdraw;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "BalloonFactory"
}

pub struct Balloon {
    thickness: f64,
    rel_scale: f64,
    mat: &'static str,
    fs: u64,
}

impl Balloon {
    fn new(fs: u64) -> Balloon {
        let mut r = Rng::seeded(fs);
        let thickness = r.uniform(0.06, 0.1);
        let mat = matdraw::sample_init(&mut r, matdraw::DECORATIVE_METAL);
        matdraw::call(&mut r, mat);
        let rel_scale = r.uniform(0.2, 0.3) * 4.0;
        let _displace = r.uniform(0.02, 0.04);
        Balloon { thickness, rel_scale, mat, fs }
    }

    /// len(generate_text()): int(clip_gaussian(3, 7, 2, 15)) characters
    fn text_len(&self, inst: u64) -> usize {
        let mut r = inst_rng(self.fs, inst);
        let mut i = 0;
        loop {
            let v = r.normal(3.0, 7.0);
            if (2.0..=15.0).contains(&v) {
                return v as usize;
            }
            if i == 20 {
                return v.clamp(2.0, 15.0) as usize;
            }
            i += 1;
        }
    }

    fn bounds(&self, inst: u64) -> (V3, V3) {
        let n = self.text_len(inst) as f64;
        ([-self.thickness / 2.0, 0.0, 0.0], [self.thickness / 2.0, self.rel_scale * n * 0.8, self.rel_scale * 0.8])
    }
}

impl Factory for Balloon {
    fn placeholder(&self, inst: u64) -> Option<Asset> {
        let (lo, hi) = self.bounds(inst);
        let m = books::new_bbox(lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }
    fn asset(&self, inst: u64) -> Asset {
        let (lo, hi) = self.bounds(inst);
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let m = Mesh::ellipsoid(c, [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0, (hi[2] - lo[2]) / 2.0], 12, 8);
        let material = spec_material("balloon", json!({"cls": self.mat, "kwargs": {}, "seed": self.fs}), [0.8, 0.8, 0.8]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &material, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Balloon::new(fs)))
}
