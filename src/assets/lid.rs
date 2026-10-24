//! tableware/lid.py LidFactory: spin -> SOLIDIFY(offset 0) -> BEVEL, glass rim torus, knob (cylinders with BEVEL); the bar handle is not ported
use super::bevel::{bevel, Offset};
use super::blops::*;
use super::bottle::unit_cylinder;
use super::matdraw;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "LidFactory"
}

pub struct Lid {
    x_length: f64,
    z_height: f64,
    thickness: f64,
    is_glass: bool,
    rim_height: f64,
    knob: bool,
    handle_height: f64,
    handle_radius: f64,
    mats: [&'static str; 3],
    fs: u64,
}

impl Lid {
    fn new(fs: u64) -> Lid {
        let mut r = Rng::seeded(fs);
        let x_length = r.uniform(0.08, 0.15);
        let z_height = x_length * r.uniform(0.0, 0.5);
        let thickness = r.uniform(0.003, 0.005);
        let is_glass = r.uniform(0.0, 1.0) < 0.5;
        let rim_height = r.uniform(1.0, 2.0) * thickness;
        let knob = r.randint(0, 2) == 1;
        let handle_height = if knob { x_length * r.uniform(0.1, 0.15) } else { x_length * r.uniform(0.2, 0.25) };
        let handle_radius = x_length * r.uniform(0.15, 0.25);
        let _handle_width = x_length * r.uniform(0.25, 0.3);
        let _sub = r.randint(0, 3);
        let m0 = if is_glass { matdraw::sample_init(&mut r, matdraw::FRONT_MAYBEGLASS) } else { matdraw::sample_init(&mut r, matdraw::DECORATIVE_HARD) };
        let m1 = matdraw::sample_init(&mut r, matdraw::METALS);
        let m2 = matdraw::sample_init(&mut r, matdraw::DECORATIVE_HARD);
        Lid { x_length, z_height, thickness, is_glass, rim_height, knob, handle_height, handle_radius, mats: [m0, m1, m2], fs }
    }

    fn cyl(&self, sx: f64, sz: f64, z0: f64) -> Mesh {
        let mut c = unit_cylinder(32, true);
        for p in c.pos.iter_mut() {
            *p = [(p[0] as f32 * sx as f32) as f64, (p[1] as f32 * sx as f32) as f64, (p[2] as f32 * sz as f32 + z0 as f32) as f64];
        }
        bevel(&c, Offset::Width(self.thickness / 2.0), 4, 30f64.to_radians()).unwrap_or(c)
    }
}

impl Factory for Lid {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        for k in 0..3 {
            matdraw::call(&mut r, self.mats[k]);
        }
        let xs = [0.0, 0.01, self.x_length / 2.0, self.x_length];
        let zh = self.z_height;
        let zs = [zh, zh, zh * r.uniform(0.7, 0.8), 0.0];
        let anchors: Vec<[f64; 3]> = (0..4).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[]);
        let s = books::solidify_offset(&m, self.thickness as f32 as f64, 0.0);
        let body = bevel(&s, Offset::Width(self.thickness / 2.0), 4, 30f64.to_radians()).unwrap_or(s);
        let mut all = Mesh::new();
        all.append(&body);
        if self.is_glass {
            // torus: major radius x_length, minor radius thickness / 2, 128 x 12 segments, z scaled by rim_height / thickness
            let (maj, mnr) = (128usize, 12usize);
            let zs = (self.rim_height / self.thickness) as f32;
            let mut t = Mesh::new();
            for a in 0..maj {
                let th = (2.0 * std::f64::consts::PI * a as f64 / maj as f64) as f32;
                for b in 0..mnr {
                    let ph = (2.0 * std::f64::consts::PI * b as f64 / mnr as f64) as f32;
                    let rad = self.x_length as f32 + (self.thickness / 2.0) as f32 * ph.cos();
                    t.pos.push([(rad * th.cos()) as f64, (rad * th.sin()) as f64, ((self.thickness / 2.0) as f32 * ph.sin() * zs) as f64]);
                }
            }
            for a in 0..maj {
                let a1 = (a + 1) % maj;
                for b in 0..mnr {
                    let b1 = (b + 1) % mnr;
                    t.faces.push(vec![(a * mnr + b) as u32, (a1 * mnr + b) as u32, (a1 * mnr + b1) as u32, (a * mnr + b1) as u32]);
                }
            }
            all.append(&t);
        }
        if self.knob {
            let u1 = r.uniform(1.0, 2.0);
            let stem = self.cyl(self.thickness * u1, self.handle_height, self.z_height);
            let u2 = r.uniform(1.0, 2.0);
            let top = self.cyl(self.handle_radius, self.thickness * u2, self.z_height + self.handle_height);
            all.append(&stem);
            all.append(&top);
        }
        let mat = spec_material("lid", json!({"cls": self.mats[0], "kwargs": {}, "seed": self.fs}), [0.8, 0.8, 0.8]);
        let mut a = Asset::default();
        let b = all.bounds();
        a.mesh(all, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Lid::new(fs)))
}
