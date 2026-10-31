//! ShowerFactory (not in infinigen): a walk-in shower -- low tray with a drain, glass enclosure (one fixed panel + door, or a corner pair)
//! on slim chrome posts, riser pipe with a round head, and a mixer valve on the back wall. Real bathrooms often have a shower instead of a
//! bathtub; infinigen had none. Frame as appliances.rs: x = depth (back wall at x = 0, front at +x), y = width, z up from the floor.
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "ShowerFactory"
}

pub struct Shower {
    depth: f64,
    width: f64,
    height: f64,
    tray_h: f64,
    corner: bool,
    door_side: bool,
    seed: u64,
}

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

impl Shower {
    fn new(r: &mut Rng) -> Shower {
        let (mut depth, mut width, mut height) = (r.uniform(0.7, 0.95), r.uniform(0.7, 1.1), r.uniform(1.9, 2.0));
        // the solver's chosen size (furn::sizes 'exact': built-ins fill their alcove)
        if let Some(h) = crate::api::size_hint() {
            (width, depth, height) = (h[0], h[1], h[2]);
        }
        Shower {
            depth,
            width,
            height,
            tray_h: r.uniform(0.03, 0.06),
            corner: r.f64() < 0.5,
            door_side: r.f64() < 0.5,
            seed: r.randint(0, 1_000_000) as u64,
        }
    }

    fn build(&self) -> Asset {
        let mut a = Asset::default();
        let (d, w, h, th) = (self.depth, self.width, self.height, self.tray_h);
        let s = self.seed;
        let tray = spec("shower_tray", "ceramic.Ceramic", s, [0.92, 0.92, 0.92]);
        let glass = spec("shower_glass", "ceramic.Glass", s + 1, [0.85, 0.9, 0.9]);
        let chrome = spec("shower_chrome", "metal.MetalBasic", s + 2, [0.8, 0.8, 0.82]);
        let dark = Material::diffuse("shower_drain", [0.05, 0.05, 0.05], 0.4);
        // tray: slight rim, drain grate in the middle
        let t = Mesh::cuboid([0.0, 0.0, 0.0], [d, w, th]).smoothed(0.008, 2, 0);
        let tb = t.bounds();
        a.mesh(t, &tray, Some(tb));
        let drain = Mesh::cuboid([d / 2.0 - 0.06, w / 2.0 - 0.06, th], [d / 2.0 + 0.06, w / 2.0 + 0.06, th + 0.002]);
        a.mesh(drain, &dark, None);
        a.supports.push(([0.0, 0.0, th], [d, w, th + 0.001]));
        let gt = 0.008; // glass thickness
        let post = |a: &mut Asset, x: f64, y: f64| {
            let m = limb([x, y, th], [x, y, h], 0.012, 0.012, true);
            a.mesh(m, &chrome, None);
        };
        // front screen along y at x = d (fixed panel + door), and for corner showers a side screen along x at y = w (or 0)
        let front_x = d - 0.01;
        let (fy0, fy1) = (0.0, w);
        let split = fy0 + (fy1 - fy0) * 0.5;
        let fixed = Mesh::cuboid([front_x - gt / 2.0, fy0 + 0.02, th + 0.01], [front_x + gt / 2.0, split - 0.005, h]);
        a.mesh(fixed, &glass, Some(([front_x - 0.01, fy0, th], [front_x + 0.01, fy1, h])));
        // door, opened a little toward the room
        // (all but closed: ajar at 0.25 rad its glass stood 10-18 cm past the tray into the room)
        let ang: f64 = 0.04;
        let (hx, hy) = (front_x, split);
        let dw = fy1 - split - 0.03;
        let (cx, cy) = (hx + ang.sin() * dw, hy + ang.cos() * dw);
        let mut door = Mesh::new();
        let p = |x: f64, y: f64, z: f64| [x, y, z];
        let (z0, z1) = (th + 0.015, h - 0.01);
        let n = [-(cy - hy), cx - hx];
        let nl = (n[0] * n[0] + n[1] * n[1]).sqrt();
        let o = [n[0] / nl * gt / 2.0, n[1] / nl * gt / 2.0];
        let v = [p(hx - o[0], hy - o[1], z0), p(cx - o[0], cy - o[1], z0), p(cx - o[0], cy - o[1], z1), p(hx - o[0], hy - o[1], z1), p(hx + o[0], hy + o[1], z0), p(cx + o[0], cy + o[1], z0), p(cx + o[0], cy + o[1], z1), p(hx + o[0], hy + o[1], z1)];
        for f in [[0, 1, 2, 3], [7, 6, 5, 4], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]] { door.add_face(&[v[f[0]], v[f[1]], v[f[2]], v[f[3]]]); }
        a.mesh(door, &glass, None);
        // door handle bar
        let hb = limb([cx - 0.02 + 0.03, cy - 0.06, 1.0], [cx - 0.02 + 0.03, cy - 0.06, 1.3], 0.01, 0.01, true);
        a.mesh(hb, &chrome, None);
        post(&mut a, front_x, fy0 + 0.01);
        post(&mut a, front_x, split);
        if self.corner {
            let sy = if self.door_side { w - 0.01 } else { 0.01 };
            let side = Mesh::cuboid([0.02, sy - gt / 2.0, th + 0.01], [front_x - 0.02, sy + gt / 2.0, h]);
            a.mesh(side, &glass, Some(([0.0, sy - 0.01, th], [front_x, sy + 0.01, h])));
            post(&mut a, front_x, sy);
        }
        // top rail on the front screen
        let rail = limb([front_x, fy0, h], [front_x, split, h], 0.01, 0.01, true);
        a.mesh(rail, &chrome, None);
        // riser + head + valve on the back wall (x = 0)
        let ry = w * 0.5;
        // (the arm under the enclosure's top: at a fixed 1.95 m it stood above a 1.92 m enclosure)
        let za = (h - 0.03).min(1.95);
        let riser = limb([0.04, ry, 1.0], [0.04, ry, za], 0.011, 0.011, true);
        a.mesh(riser, &chrome, None);
        let arm = limb([0.04, ry, za], [0.22, ry, za], 0.01, 0.01, true);
        a.mesh(arm, &chrome, None);
        let head = limb([0.22, ry, za], [0.22, ry, za - 0.02], 0.11, 0.11, true);
        a.mesh(head, &chrome, None);
        let valve = limb([0.0, ry, 1.05], [0.06, ry, 1.05], 0.035, 0.03, true);
        a.mesh(valve, &chrome, None);
        let lever = limb([0.06, ry, 1.05], [0.06, ry + 0.09, 1.05], 0.008, 0.008, true);
        a.mesh(lever, &chrome, None);
        a
    }
}

impl Factory for Shower {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([0.0, 0.0, 0.0], [self.depth + 0.25, self.width, self.height]))
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        a.cuboid([0.0, 0.0, 0.0], [self.depth, self.width, self.height], &Material::diffuse("shower_ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}

pub fn make(_c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Shower::new(&mut rng.clone())))
}
