//! FridgeFactory (not in infinigen): a full-size kitchen refrigerator -- top-freezer, bottom-freezer, side-by-side (with a dispenser
//! recess) or french-door layouts, door gaps, a toe-kick grille and bar handles, in stainless, white or black finishes.
//! Frame: x = depth (front toward +x), y = width, z = height, origin at the back-left-bottom corner (as appliances.rs).
use super::geomkit::*;
use super::*;

pub fn handles(c: &str) -> bool {
    c == "FridgeFactory"
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    TopFreezer,
    BottomFreezer,
    SideBySide,
    French,
}

pub struct Fridge {
    style: Style,
    depth: f64,
    width: f64,
    height: f64,
    door_t: f64,
    kick: f64,
    finish: usize,
    split: f64,
    seed: u64,
}

const GAP: f64 = 0.004;
/// how far the handles stand proud of the door fronts (standoff + bar radius)
const HANDLE_PROUD: f64 = 0.035 + 0.011;

impl Fridge {
    fn new(rng: &mut Rng) -> Fridge {
        let mut f = Fridge::draw(rng);
        // built to the layout box [width (y), depth (x), height]: the bar handles stand proud of the doors
        if let Some(h) = crate::api::size_hint() {
            (f.width, f.depth, f.height) = (h[0], h[1] - HANDLE_PROUD, h[2]);
        }
        f
    }

    fn draw(rng: &mut Rng) -> Fridge {
        let r = rng;
        let u = r.f64();
        let style = if u < 0.3 { Style::TopFreezer } else if u < 0.55 { Style::BottomFreezer } else if u < 0.75 { Style::SideBySide } else { Style::French };
        let width = match style {
            Style::TopFreezer | Style::BottomFreezer => r.uniform(0.6, 0.76),
            _ => r.uniform(0.82, 0.92),
        };
        let height = r.uniform(1.65, 1.85);
        let split = match style {
            Style::TopFreezer => r.uniform(0.66, 0.72),
            Style::BottomFreezer => r.uniform(0.28, 0.34),
            Style::SideBySide => r.uniform(0.4, 0.45),
            Style::French => r.uniform(0.3, 0.36),
        };
        Fridge {
            style,
            depth: r.uniform(0.62, 0.74),
            width,
            height,
            door_t: r.uniform(0.045, 0.065),
            kick: r.uniform(0.05, 0.09),
            // stainless 55%, white 30%, black 15%
            finish: { let f = r.f64(); if f < 0.55 { 0 } else if f < 0.85 { 1 } else { 2 } },
            split,
            seed: r.randint(0, 1_000_000) as u64,
        }
    }

    fn build(&self) -> Asset {
        let mut a = Asset::default();
        let (d, w, h, t, k) = (self.depth, self.width, self.height, self.door_t, self.kick);
        let s = self.seed;
        // plain PBR finishes: infinigen's metal shaders randomise the hue (teal / maroon fridges)
        let pbr = |name: &str, rgb: f32, rough: f32, metal: f32| {
            let mut m = Material::diffuse(&format!("{name}:{s}"), [rgb, rgb, rgb * 1.01], rough);
            m.metallic = metal;
            m
        };
        let j = (s % 7) as f32 * 0.01;
        let (body, door, handle) = match self.finish {
            0 => (pbr("fridge_steel", 0.62 + j, 0.32, 1.0), pbr("fridge_steel_door", 0.66 + j, 0.28, 1.0), pbr("fridge_handle", 0.75, 0.2, 1.0)),
            1 => (pbr("fridge_white", 0.8 + j, 0.3, 0.0), pbr("fridge_white_door", 0.82 + j, 0.22, 0.0), if s % 2 == 0 { pbr("fridge_handle", 0.82, 0.25, 0.0) } else { pbr("fridge_handle", 0.7, 0.2, 1.0) }),
            _ => (pbr("fridge_black", 0.03 + j * 0.3, 0.35, 0.4), pbr("fridge_black_door", 0.035, 0.25, 0.5), pbr("fridge_handle", 0.05, 0.3, 0.8)),
        };
        let dark = Material::diffuse("fridge_grille", [0.03, 0.03, 0.03], 0.6);
        let x0 = d - t; // door back plane
        // cabinet (sides and top a little proud of the doors' back so the gaps read dark)
        let mut cab = Mesh::new();
        cab.append(&Mesh::cuboid([0.0, 0.0, k], [x0, w, h]));
        let cb = cab.bounds();
        a.mesh(cab, &body, Some(cb));
        // toe kick: recessed dark grille
        let grille = Mesh::cuboid([0.0, 0.01, 0.0], [x0 - 0.03, w - 0.01, k]);
        let gb = grille.bounds();
        a.mesh(grille, &dark, Some(gb));
        a.supports.push(([0.0, 0.0, h], [x0, w, h + 0.001]));
        let mut add_door = |a: &mut Asset, y0: f64, y1: f64, z0: f64, z1: f64| {
            let m = Mesh::cuboid([x0, y0 + GAP, z0 + GAP], [d, y1 - GAP, z1 - GAP]);
            let b = m.bounds();
            a.mesh(m, &door, Some(b));
        };
        // bar handles: vertical (at y, from z0 to z1) or horizontal (at z, from y0 to y1), on two standoffs
        let r = 0.011;
        let off = 0.035;
        let vbar = |a: &mut Asset, y: f64, z0: f64, z1: f64| {
            let mut m = limb([d + off, y, z0], [d + off, y, z1], r, r, true);
            for z in [z0 + 0.02, z1 - 0.02] {
                m.append(&limb([d, y, z], [d + off, y, z], 0.007, 0.007, false));
            }
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let hbar = |a: &mut Asset, z: f64, y0: f64, y1: f64| {
            let mut m = limb([d + off, y0, z], [d + off, y1, z], r, r, true);
            for y in [y0 + 0.02, y1 - 0.02] {
                m.append(&limb([d, y, z], [d + off, y, z], 0.007, 0.007, false));
            }
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let zs = k + (h - k) * self.split;
        let hy = 0.06; // handle inset from the door's opening edge
        match self.style {
            Style::TopFreezer => {
                add_door(&mut a, 0.0, w, k, zs);
                add_door(&mut a, 0.0, w, zs, h);
                vbar(&mut a, hy, zs - 0.45, zs - 0.08);
                vbar(&mut a, hy, zs + 0.06, zs + 0.06 + (h - zs) * 0.5);
            }
            Style::BottomFreezer => {
                add_door(&mut a, 0.0, w, k, zs);
                add_door(&mut a, 0.0, w, zs, h);
                hbar(&mut a, zs - 0.06, w * 0.2, w * 0.8);
                vbar(&mut a, hy, zs + 0.1, zs + 0.1 + (h - zs) * 0.6);
            }
            Style::SideBySide => {
                let ys = w * self.split;
                add_door(&mut a, 0.0, ys, k, h);
                add_door(&mut a, ys, w, k, h);
                let (z0, z1) = (k + (h - k) * 0.3, k + (h - k) * 0.85);
                vbar(&mut a, ys - hy, z0, z1);
                vbar(&mut a, ys + hy, z0, z1);
                // ice/water dispenser recess on the freezer door
                let (dz0, dz1) = (k + (h - k) * 0.48, k + (h - k) * 0.66);
                let rec = Mesh::cuboid([d, ys * 0.22, dz0], [d + 0.003, ys * 0.78, dz1]);
                let rb = rec.bounds();
                a.mesh(rec, &dark, Some(rb));
            }
            Style::French => {
                add_door(&mut a, 0.0, w, k, zs);
                add_door(&mut a, 0.0, w / 2.0, zs, h);
                add_door(&mut a, w / 2.0, w, zs, h);
                hbar(&mut a, zs - 0.06, w * 0.15, w * 0.85);
                let (z0, z1) = (zs + 0.1, zs + 0.1 + (h - zs) * 0.6);
                vbar(&mut a, w / 2.0 - hy, z0, z1);
                vbar(&mut a, w / 2.0 + hy, z0, z1);
            }
        }
        a
    }
}

impl Factory for Fridge {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        Some([self.width, self.depth + HANDLE_PROUD, self.height])
    }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([0.0, 0.0, 0.0], [self.depth + HANDLE_PROUD, self.width, self.height]))
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
    /// collision: the body (handles included), the top, a swing per hinged door (hinged away from its handle) and a pull-out for a
    /// bottom freezer drawer (bottom-freezer and french-door fridges)
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::{Leaf, Volume};
        let (d, w, h, t, k) = (self.depth, self.width, self.height, self.door_t, self.kick);
        let x0 = d - t;
        let xf = d + HANDLE_PROUD;
        let mut v = vec![Volume::hard([0.0, 0.0, -0.001], [xf + 0.001, w, h + 0.001]), Volume::support([0.0, 0.0, h - 0.001], [x0, w, h])];
        let zs = k + (h - k) * self.split;
        let door = |v: &mut Vec<Volume>, y0: f64, y1: f64, z0: f64, z1: f64, hinge_hi: bool| {
            let (hy, axis) = if hinge_hi { (y1, [0.0, 0.0, 1.0]) } else { (y0, [0.0, 0.0, -1.0]) };
            v.push(Volume::opening(Leaf::Hinged { panel: ([x0, y0, z0], [xf, y1, z1]), hinge: [d, hy, z0], axis, max_angle: 100f64.to_radians() }));
        };
        let drawer = |v: &mut Vec<Volume>, z0: f64, z1: f64| v.push(Volume::opening(Leaf::Drawer { panel: ([0.05, 0.0, z0], [xf, w, z1]), dir: [1.0, 0.0, 0.0], travel: 0.6 * d }));
        match self.style {
            Style::TopFreezer => { door(&mut v, 0.0, w, k, zs, true); door(&mut v, 0.0, w, zs, h, true); }
            Style::BottomFreezer => { drawer(&mut v, k, zs); door(&mut v, 0.0, w, zs, h, true); }
            Style::SideBySide => { let ys = w * self.split; door(&mut v, 0.0, ys, k, h, false); door(&mut v, ys, w, k, h, true); }
            Style::French => { drawer(&mut v, k, zs); door(&mut v, 0.0, w / 2.0, zs, h, false); door(&mut v, w / 2.0, w, zs, h, true); }
        }
        Some(v)
    }
}

pub fn make(_c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Fridge::new(&mut rng.clone())))
}
