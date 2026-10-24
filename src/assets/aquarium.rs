//! decor/aquarium_tank.py: AquariumTankFactory (tank, belts); the plant/rock/coral inside is not ported (stand-in proxy)
use super::*;

pub fn handles(c: &str) -> bool {
    c == "AquariumTankFactory"
}

pub struct Aquarium {
    pub is_wet: bool,
    pub kind: usize,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub thickness: f64,
    pub belt_t: f64,
}

impl Aquarium {
    pub fn new(rng: &mut Rng) -> Aquarium {
        let mut r = rng.clone();
        let is_wet = r.f64() < 0.5;
        let kind = r.choice_idx(3);
        // the base factory constructor runs inside its own FixedSeed block (state restored) so it does not advance this stream
        let width = r.log_uniform(0.5, 1.0);
        let depth = r.log_uniform(0.5, 0.8);
        let height = r.log_uniform(0.5, 1.0);
        let thickness = r.uniform(0.01, 0.02);
        let belt_t = r.log_uniform(0.02, 0.05);
        Aquarium { is_wet, kind, width, depth, height, thickness, belt_t }
    }
}

impl Factory for Aquarium {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (w, d, h, t) = (self.width, self.depth, self.height, self.thickness);
        let mut a = Asset::default();
        a.cuboid([-t - d, -t, 0.0], [t, w + t, h], &mat("aquarium_ph", [0.6, 0.7, 0.8], 0.1));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        // glass tank walls (open top box shell), two belts, and a stand-in for the decoration
        let (w, d, h, t) = (self.width, self.depth, self.height, self.thickness);
        let glass = mat("aquarium_glass", [0.8, 0.9, 0.95], 0.02);
        let belt = mat("aquarium_belt", [0.1, 0.1, 0.1], 0.4);
        let mut a = Asset::default();
        let (x0, x1, y0, y1) = (-t - d, t, -t, w + t);
        a.cuboid([x0, y0, 0.0], [x1, y1, t], &glass);
        for (lo, hi) in [([x0, y0, 0.0], [x1, y0 + t, h]), ([x0, y1 - t, 0.0], [x1, y1, h]), ([x0, y0, 0.0], [x0 + t, y1, h]), ([x1 - t, y0, 0.0], [x1, y1, h])] {
            a.cuboid(lo, hi, &glass);
        }
        let bt = self.belt_t;
        for z in [0.0, h - bt] {
            for (lo, hi) in [([x0, y0, z], [x1, y0 + t, z + bt]), ([x0, y1 - t, z], [x1, y1, z + bt]), ([x0, y0, z], [x0 + t, y1, z + bt]), ([x1 - t, y0, z], [x1, y1, z + bt])] {
                a.cuboid(lo, hi, &belt);
            }
        }
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    let _ = c;
    Some(Box::new(Aquarium::new(rng)))
}
