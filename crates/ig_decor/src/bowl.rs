//! tableware/bowl.py BowlFactory: spin -> SOLIDIFY(offset +1) -> BEVEL(thickness/2, 2..4 segments) -> subsurf(1)
use super::bevel::{bevel, Offset};
use super::blops::*;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "BowlFactory"
}

pub struct Bowl {
    x_end: f64,
    z_length: f64,
    z_bottom: f64,
    x_bottom: f64,
    x_mid: f64,
    scale: f64,
    thickness: f64,
    fs: u64,
}

impl Bowl {
    fn new(fs: u64) -> Bowl {
        let mut r = Rng::seeded(fs);
        let x_end = 0.5;
        let z_length = r.log_uniform(0.4, 0.8);
        let z_bottom = r.log_uniform(0.02, 0.05);
        let x_bottom = r.uniform(0.2, 0.3) * x_end;
        let x_mid = r.uniform(0.8, 0.95) * x_end;
        let _has_inside = r.uniform(0.0, 1.0) < 0.5;
        let scale = r.log_uniform(0.15, 0.4);
        let thickness = r.uniform(0.01, 0.03) * scale;
        Bowl { x_end, z_length, z_bottom, x_bottom, x_mid, scale, thickness, fs }
    }
}

impl Factory for Bowl {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let r = self.x_end * self.scale;
        let m = books::new_bbox(-r, r, -r, r, 0.0, self.z_length * self.scale);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let sc = self.scale;
        let xs = [0.0, self.x_bottom, self.x_bottom + 1e-3, self.x_bottom, self.x_mid, self.x_end];
        let zs = [0.0, 0.0, 0.0, self.z_bottom, self.z_length / 2.0, self.z_length];
        let anchors: Vec<[f64; 3]> = (0..6).map(|k| [xs[k] * sc, 0.0, zs[k] * sc]).collect();
        let m = spin_anchors(&anchors, &[2, 3]);
        let s = books::solidify_simple(&m, -(self.thickness as f32 as f64), None);
        if std::env::var("BDBG").is_ok() { eprintln!("prebevel verts {} faces {}", s.pos.len(), s.faces.len()); }
        let seg = r.randint(2, 5) as usize;
        let s = bevel(&s, Offset::Width(self.thickness / 2.0), seg, 30f64.to_radians()).unwrap_or(s);
        if std::env::var("BDBG").is_ok() { eprintln!("bevel verts {} nan {}", s.pos.len(), s.pos.iter().filter(|p| p.iter().any(|x| x.is_nan())).count()); }
        let s = subsurf(&s, 1, false);
        let mat = spec_material("bowl", json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.fs}), [0.85, 0.85, 0.82]);
        let mut a = Asset::default();
        let b = s.bounds();
        a.mesh(s, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Bowl::new(fs)))
}

/// tableware/fruit_container.py: FixedSeed(fs): np.random.choice([Bowl, Pot], p=[.5,.5]) then base(fs); fruit scattering happens in finalize_assets
pub struct FruitContainer {
    base: Box<dyn Factory>,
}

pub fn fruit_handles(c: &str) -> bool {
    c == "FruitContainerFactory"
}

pub fn fruit_make(fs: u64) -> Option<Box<dyn Factory>> {
    let mut r = Rng::seeded(fs);
    let u = r.uniform(0.0, 1.0);
    let base: Box<dyn Factory> = if u < 0.5 {
        Box::new(Bowl::new(fs))
    } else {
        super::cookware::make("PotFactory", &mut Rng::seeded(fs), fs)?
    };
    Some(Box::new(FruitContainer { base }))
}

impl Factory for FruitContainer {
    fn asset(&self, i: u64) -> Asset {
        self.base.asset(i)
    }
    fn placeholder(&self, i: u64) -> Option<Asset> {
        let p = self.base.placeholder(i)?;
        let (mut lo, mut hi) = p.bounds();
        for z in [&mut lo[2], &mut hi[2]] {
            if *z > 0.02 {
                *z += 0.05;
            } else if *z < 0.02 {
                *z -= 0.01;
            }
        }
        let mut a = Asset::default();
        let m = books::new_bbox(lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }
}
