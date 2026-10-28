//! appliances/{dishwasher,beverage_fridge,microwave,oven}.py and elements/rug.py: boxy appliances with doors, handles, racks, knobs; rug shapes
//! Frame: x = depth (front toward +x), y = width, z = height, origin at the back-left-bottom corner.
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;
use std::f64::consts::PI;

/// infinigen.assets.utils.misc.generate_text: clip_gaussian(3, 7, 2, 15) characters drawn with p = 1.5/0.5/0.5 over a-z A-Z 0-9
pub fn generate_text(r: &mut Rng) -> String {
    let n = clip_gaussian(r, 3.0, 7.0, 2.0, 15.0) as i64;
    let mut w = vec![1.5; 26];
    w.extend(vec![0.5; 36]);
    let tot: f64 = w.iter().sum();
    let mut cdf = vec![];
    let mut acc = 0.0;
    for x in &w {
        acc += x / tot;
        cdf.push(acc);
    }
    let last = *cdf.last().unwrap();
    for c in cdf.iter_mut() {
        *c /= last;
    }
    let chars: Vec<char> = super::fontdata::FONT_CHARS.chars().collect();
    (0..n.max(0)).map(|_| { let u = r.f64(); chars[cdf.partition_point(|&c| c <= u).min(61)] }).collect()
}

/// centred StringToCurves bounds at size 1: (xmin, xmax)
pub fn text_x_bounds(s: &str) -> (f64, f64) {
    let chars: Vec<char> = super::fontdata::FONT_CHARS.chars().collect();
    let m = |c: char| super::fontdata::FONT_METRICS[chars.iter().position(|&x| x == c).unwrap()];
    let cs: Vec<char> = s.chars().collect();
    let total: f64 = cs.iter().map(|&c| m(c)[2]).sum();
    let before_last: f64 = cs[..cs.len() - 1].iter().map(|&c| m(c)[2]).sum();
    (-total / 2.0 + m(cs[0])[0], -total / 2.0 + before_last + m(cs[cs.len() - 1])[1])
}

pub fn handles(c: &str) -> bool {
    matches!(c, "DishwasherFactory" | "BeverageFridgeFactory" | "MicrowaveFactory" | "OvenFactory" | "RugFactory")
}

const METALS: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3)];
const MAYBE_GLASS: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3), ("metal.BlackGlass", 4.0), ("ceramic.Glass", 10.0)];
const GLASS: Weighted = &[("metal.BlackGlass", 2.0), ("ceramic.Glass", 10.0)];
const RUG: Weighted = &[
    ("fabric.CoarseKnitFabric", 1.0),
    ("fabric.FineKnitFabric", 1.0),
    ("fabric.Leather", 0.3),
    ("fabric.SofaFabric", 1.0),
    ("fabric.Rug", 7.0),
    ("art.ArtRug", 5.0),
    ("fabric.CoarseKnitFabric", 1.0),
];

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.6, 0.6, 0.62])
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    Dish,
    Fridge,
    Micro,
    Oven,
    Rug,
}

pub struct Appliance {
    k: K,
    depth: f64,
    width: f64,
    height: f64,
    door_t: f64,
    rack_r: f64,
    racks: usize,
    panel_h: f64,
    panel_t: f64,
    buttons: usize,
    button_r: f64,
    button_t: f64,
    panel_w: f64,
    margin_z: f64,
    door_margin: f64,
    use_gas: bool,
    // rug
    shape: usize,
    thickness: f64,
    rounded: f64,
    seed: u64,
    brand: String,
}

impl Appliance {
    fn new(c: &str, rng: &mut Rng) -> Appliance {
        // exact python draw order (materials stubbed): see appliances/*.py sample_parameters, elements/rug.py
        let mut r = rng.clone();
        let r = &mut r;
        let k = match c {
            "DishwasherFactory" => K::Dish,
            "BeverageFridgeFactory" => K::Fridge,
            "MicrowaveFactory" => K::Micro,
            "OvenFactory" => K::Oven,
            _ => K::Rug,
        };
        let mut a = Appliance { k, depth: 1.0, width: 1.0, height: 1.0, door_t: 0.07, rack_r: 0.015, racks: 2, panel_h: 0.0, panel_t: 0.0, buttons: 0, button_r: 0.0, button_t: 0.0, panel_w: 0.0, margin_z: 0.0, door_margin: 0.0, use_gas: false, shape: 0, thickness: 0.015, rounded: 0.3, seed: 0, brand: String::new() };
        match k {
            K::Dish | K::Fridge | K::Oven => {
                a.depth = 1.0 + r.normal(0.0, 0.1);
                a.width = 1.0 + r.normal(0.0, 0.1);
                a.height = 1.0 + r.normal(0.0, 0.1);
                a.door_t = r.uniform(0.05, 0.1) * a.depth;
                a.rack_r = r.uniform(0.01, 0.02) * a.depth;
                match k {
                    K::Dish => {
                        a.racks = r.randint(2, 3) as usize;
                    }
                    K::Fridge => {
                        a.racks = r.randint(2, 4) as usize;
                        let _rack_d = r.randint(4, 6);
                    }
                    _ => {
                        a.racks = r.randint(2, 4) as usize;
                        let _rack_d = r.randint(4, 6);
                        a.panel_h = r.uniform(0.2, 0.4) * a.height;
                        a.panel_t = r.uniform(0.15, 0.25) * a.depth;
                        a.buttons = r.randint(1, 3) as usize * 2;
                        a.button_r = r.uniform(0.05, 0.1) * a.width;
                        a.button_t = r.uniform(0.02, 0.04) * a.depth;
                        let _heat = r.uniform(0.1, 0.2);
                        skip_text(r);
                        a.use_gas = r.randint(0, 2) == 1;
                        let n_grids = r.randint(2, 5);
                        for _ in 0..n_grids {
                            r.randint(1, 4);
                        }
                        let _branches = 2 * r.randint(2, 9);
                        let _grate = r.uniform(0.01, 0.03);
                        let _center = r.uniform(0.05, 0.15);
                        let _middle = r.uniform(0.5, 0.7);
                    }
                }
            }
            K::Micro => {
                a.depth = r.uniform(0.5, 0.7);
                a.width = r.uniform(0.6, 1.0);
                a.height = r.uniform(0.35, 0.45);
                a.panel_w = r.uniform(0.2, 0.4);
                a.margin_z = r.uniform(0.05, 0.1);
                a.door_t = r.uniform(0.02, 0.04);
                a.door_margin = r.uniform(0.03, 0.1);
                a.brand = generate_text(r);
            }
            K::Rug => {
                let w = clip_gaussian(r, 3.0, 1.0, 2.0, 6.0);
                let len = w * r.uniform(1.0, 1.5);
                a.shape = r.choice_idx(4); // rectangle, circle, rounded, ellipse
                a.width = w;
                a.depth = if a.shape == 1 { w } else { len };
                a.rounded = w * r.uniform(0.1, 0.5);
                a.thickness = r.uniform(0.01, 0.02);
            }
        }
        a.seed = r.randint(0, 1_000_000) as u64;
        a
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let mut a = Asset::default();
        let (d, w, h) = (self.depth, self.width, self.height);
        if self.k == K::Rug {
            let m = spec("rug", weighted(&mut r, RUG), self.seed);
            let n = 128;
            let (hl, hw) = (d / 2.0, w / 2.0);
            let pts: Vec<[f64; 2]> = match self.shape {
                0 => vec![[-hl, -hw], [hl, -hw], [hl, hw], [-hl, hw]],
                // python's BEVEL modifier does not round a single flat quad (boundary edges are not beveled): 'rounded' is a plain rectangle
                2 => vec![[-hl, -hw], [hl, -hw], [hl, hw], [-hl, hw]],
                _ => (0..n).map(|i| { let t = 2.0 * PI * i as f64 / n as f64; [hl * t.cos(), hw * t.sin()] }).collect(),
            };
            let mesh = extrude(&pts, 0.0, self.thickness);
            let b = mesh.bounds();
            a.mesh(mesh, &m, Some(b));
            return a;
        }
        let body = spec("app_body", weighted(&mut r, crate::matpick::APPLIANCE_METALS), self.seed);
        let back = spec("app_back", weighted(&mut r, crate::matpick::APPLIANCE_METALS), self.seed + 1);
        let glass = spec("app_glass", weighted(&mut r, GLASS), self.seed + 2);
        let front = spec("app_front", weighted(&mut r, MAYBE_GLASS), self.seed + 3);
        let handle = spec("app_handle", weighted(&mut r, crate::matpick::APPLIANCE_METALS), self.seed + 4);
        let t = self.door_t;
        let bodyh = h;
        // carcass: open at the front so the door reads as separate
        let mut shell = Mesh::new();
        let wall = 0.03;
        shell.append(&Mesh::cuboid([0.0, 0.0, 0.0], [d - t, w, wall]));
        shell.append(&Mesh::cuboid([0.0, 0.0, bodyh - wall], [d - t, w, bodyh]));
        shell.append(&Mesh::cuboid([0.0, 0.0, 0.0], [wall, w, bodyh]));
        shell.append(&Mesh::cuboid([0.0, 0.0, 0.0], [d - t, wall, bodyh]));
        shell.append(&Mesh::cuboid([0.0, w - wall, 0.0], [d - t, w, bodyh]));
        let sb = shell.bounds();
        a.mesh(shell, &back, Some(sb));
        a.supports.push(([0.0, 0.0, bodyh], [d, w, bodyh + 0.001]));
        match self.k {
            K::Dish => {
                let door = Mesh::cuboid([d - t, 0.0, 0.0], [d, w, bodyh]);
                let db = door.bounds();
                a.mesh(door, &front, Some(db));
                // top control strip + handle bar
                let strip = Mesh::cuboid([d, w * 0.05, bodyh * 0.88], [d + 0.015, w * 0.95, bodyh * 0.96]);
                let sb2 = strip.bounds();
                a.mesh(strip, &handle, Some(sb2));
                let bar = limb([d + 0.04, w * 0.12, bodyh * 0.8], [d + 0.04, w * 0.88, bodyh * 0.8], 0.012, 0.012, true);
                let bb = bar.bounds();
                a.mesh(bar, &handle, Some(bb));
                for ix in [0.12, 0.88] {
                    let post = limb([d, w * ix, bodyh * 0.8], [d + 0.04, w * ix, bodyh * 0.8], 0.008, 0.008, true);
                    let pb = post.bounds();
                    a.mesh(post, &handle, Some(pb));
                }
            }
            K::Fridge => {
                // glass door in a frame, vertical handle, wire racks inside
                let fr = 0.05;
                let mut frame = Mesh::new();
                frame.append(&Mesh::cuboid([d - t, 0.0, 0.0], [d, fr, bodyh]));
                frame.append(&Mesh::cuboid([d - t, w - fr, 0.0], [d, w, bodyh]));
                frame.append(&Mesh::cuboid([d - t, fr, 0.0], [d, w - fr, fr]));
                frame.append(&Mesh::cuboid([d - t, fr, bodyh - fr], [d, w - fr, bodyh]));
                let fb = frame.bounds();
                a.mesh(frame, &front, Some(fb));
                let mut gl = Mesh::new();
                gl.add_quad([d - t * 0.5, fr, fr], [d - t * 0.5, w - fr, fr], [d - t * 0.5, w - fr, bodyh - fr], [d - t * 0.5, fr, bodyh - fr]);
                a.mesh(gl, &glass, Some(([d - t * 0.5, fr, fr], [d - t * 0.5 + 0.001, w - fr, bodyh - fr])));
                let bar = limb([d + 0.045, w * 0.9, bodyh * 0.25], [d + 0.045, w * 0.9, bodyh * 0.75], 0.014, 0.014, true);
                let bb = bar.bounds();
                a.mesh(bar, &handle, Some(bb));
                for z in [bodyh * 0.28, bodyh * 0.9] {
                    let _ = z;
                }
                for i in 1..=self.racks + 1 {
                    let z = bodyh * i as f64 / (self.racks as f64 + 2.0);
                    let rack = Mesh::cuboid([wall, wall, z], [d - t - 0.02, w - wall, z + self.rack_r]);
                    let rb = rack.bounds();
                    a.mesh(rack, &handle, Some(rb));
                }
            }
            K::Micro => {
                let pw = self.panel_w;
                let (dm, mz) = (self.door_margin, self.margin_z);
                // door with window on the left, control panel on the right
                let mut door = Mesh::new();
                let win_y0 = dm * 0.5;
                let win_y1 = w - pw - dm * 0.5;
                let (z0, z1) = (mz, bodyh - mz);
                door.append(&Mesh::cuboid([d - t, 0.0, 0.0], [d, win_y0, bodyh]));
                door.append(&Mesh::cuboid([d - t, win_y1, 0.0], [d, w - pw, bodyh]));
                door.append(&Mesh::cuboid([d - t, win_y0, 0.0], [d, win_y1, z0]));
                door.append(&Mesh::cuboid([d - t, win_y0, z1], [d, win_y1, bodyh]));
                let db = door.bounds();
                a.mesh(door, &front, Some(db));
                let mut gl = Mesh::new();
                gl.add_quad([d - t * 0.4, win_y0, z0], [d - t * 0.4, win_y1, z0], [d - t * 0.4, win_y1, z1], [d - t * 0.4, win_y0, z1]);
                a.mesh(gl, &glass, Some(([d - t * 0.4, win_y0, z0], [d - t * 0.4 + 0.001, win_y1, z1])));
                let panel = Mesh::cuboid([d - t, w - pw, 0.0], [d + 0.01, w, bodyh]);
                let pb = panel.bounds();
                a.mesh(panel, &front, Some(pb));
                for k in 0..4 {
                    let z = bodyh * (0.2 + 0.18 * k as f64);
                    let btn = Mesh::cylinder(0.0, 0.0, 0.0, 0.012, 0.014, 12);
                    let mut btn = btn;
                    btn.transform(&Mat4::rot_y(PI / 2.0));
                    btn.transform(&Mat4::translation([d + 0.01, w - pw / 2.0, z]));
                    let bb = btn.bounds();
                    a.mesh(btn, &handle, Some(bb));
                }
                let hbar = limb([d + 0.03, w - pw - 0.05, bodyh * 0.2], [d + 0.03, w - pw - 0.05, bodyh * 0.8], 0.01, 0.01, true);
                let hb = hbar.bounds();
                a.mesh(hbar, &handle, Some(hb));
            }
            K::Oven => {
                // door with glass, handle bar, cooktop + backsplash with knobs
                let mut door = Mesh::new();
                let fr = 0.07;
                door.append(&Mesh::cuboid([d - t, 0.0, 0.0], [d, fr, bodyh]));
                door.append(&Mesh::cuboid([d - t, w - fr, 0.0], [d, w, bodyh]));
                door.append(&Mesh::cuboid([d - t, fr, 0.0], [d, w - fr, fr]));
                door.append(&Mesh::cuboid([d - t, fr, bodyh - fr], [d, w - fr, bodyh]));
                let db = door.bounds();
                a.mesh(door, &front, Some(db));
                let mut gl = Mesh::new();
                gl.add_quad([d - t * 0.5, fr, fr], [d - t * 0.5, w - fr, fr], [d - t * 0.5, w - fr, bodyh - fr], [d - t * 0.5, fr, bodyh - fr]);
                a.mesh(gl, &glass, Some(([d - t * 0.5, fr, fr], [d - t * 0.5 + 0.001, w - fr, bodyh - fr])));
                let bar = limb([d + 0.05, w * 0.1, bodyh * 0.86], [d + 0.05, w * 0.9, bodyh * 0.86], 0.016, 0.016, true);
                let bb = bar.bounds();
                a.mesh(bar, &handle, Some(bb));
                for ix in [0.1, 0.9] {
                    let post = limb([d, w * ix, bodyh * 0.86], [d + 0.05, w * ix, bodyh * 0.86], 0.01, 0.01, true);
                    let pb = post.bounds();
                    a.mesh(post, &handle, Some(pb));
                }
                // cooktop slab
                let top = Mesh::cuboid([0.0, 0.0, bodyh], [d, w, bodyh + 0.02]);
                let tb = top.bounds();
                a.mesh(top, &front, Some(tb));
                a.supports.push(([0.0, 0.0, bodyh + 0.02], [d, w, bodyh + 0.021]));
                let burner_z = bodyh + 0.02;
                for (cx, cy) in [(0.3, 0.27), (0.3, 0.73), (0.7, 0.27), (0.7, 0.73)] {
                    let br = 0.07 + 0.02 * (cx + cy);
                    if self.use_gas {
                        let ring = Mesh::cylinder(d * cx, w * cy, burner_z, burner_z + 0.012, br, 20);
                        let rb = ring.bounds();
                        a.mesh(ring, &handle, Some(rb));
                        for k in 0..4 {
                            let ang = PI * k as f64 / 4.0;
                            let (dx, dy) = (ang.cos() * br * 1.5, ang.sin() * br * 1.5);
                            let g = limb([d * cx - dx, w * cy - dy, burner_z + 0.03], [d * cx + dx, w * cy + dy, burner_z + 0.03], 0.006, 0.006, true);
                            let gb = g.bounds();
                            a.mesh(g, &back, Some(gb));
                        }
                    } else {
                        let ring = Mesh::cylinder(d * cx, w * cy, burner_z, burner_z + 0.006, br, 24);
                        let rb = ring.bounds();
                        a.mesh(ring, &back, Some(rb));
                    }
                }
                // backsplash panel with knobs
                let (py0, py1) = (0.0, w);
                let pz1 = bodyh + 0.02 + self.panel_h;
                let panel = Mesh::cuboid([0.0, py0, bodyh + 0.02], [self.panel_t.min(d * 0.25), py1, pz1]);
                let pb = panel.bounds();
                a.mesh(panel, &front, Some(pb));
                let nb = self.buttons.max(2);
                for k in 0..nb {
                    let yy = w * (k as f64 + 1.0) / (nb as f64 + 1.0);
                    let mut kn = Mesh::cylinder(0.0, 0.0, 0.0, 0.02, self.button_r.min(0.07), 14);
                    kn.transform(&Mat4::rot_y(PI / 2.0));
                    kn.transform(&Mat4::translation([self.panel_t.min(d * 0.25), yy, bodyh + 0.02 + self.panel_h * 0.5]));
                    let kb = kn.bounds();
                    a.mesh(kn, &handle, Some(kb));
                }
                let _ = (self.racks, self.rack_r);
            }
            K::Rug => {}
        }
        a
    }
}

impl Factory for Appliance {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        match self.k {
            K::Dish | K::Fridge => {
                // door slab + handle (0.075 W) on the front, body from the origin
                let (d, w, h, dt) = (self.depth, self.width, self.height, self.door_t);
                Some(([0.0, 0.0, 0.0], [d + dt + 0.075 * w, w, h + dt]))
            }
            K::Micro => {
                let (d, w, h, dt) = (self.depth, self.width, self.height, self.door_t);
                // brand text (size 0.03) is centred on the y-range of the window part of the door
                let add = (w - self.panel_w) + self.margin_z * 0.5;
                let cell = w / 9.0;
                let a = add / cell;
                let kmax = (((a + 0.5).ceil() - 1.0).max((a).ceil() - 1.0)).clamp(0.0, 9.0);
                let yc = kmax * cell / 2.0;
                let (t0, t1) = text_x_bounds(&self.brand);
                let ymin = (yc + 0.03 * t0).min(0.0);
                let ymax = (yc + 0.03 * t1).max(w);
                Some(([0.0, ymin, 0.0], [d + dt + 0.005, ymax, h]))
            }
            _ => None,
        }
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
    fn exact(&self, _i: u64) -> Option<cabnodes::ExactMesh> {
        if self.k != K::Rug {
            return None;
        }
        // elements/rug.py create_asset: shape (plane / 128-gon circle scaled by (length/2, width/2)), SOLIDIFY thickness offset=1
        let (sx, sy, t) = ((self.depth / 2.0) as f32, (self.width / 2.0) as f32, self.thickness as f32);
        let mut e = cabnodes::ExactMesh::default();
        if self.shape == 0 || self.shape == 2 {
            let ring = [[-sx, -sy], [sx, -sy], [-sx, sy], [sx, sy]];
            for z in [t, 0.0f32] {
                for p in ring {
                    e.verts.push([p[0] as f64, p[1] as f64, z as f64]);
                }
            }
            e.polys = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 2, 6, 4], [1, 0, 4, 5], [3, 1, 5, 7], [2, 3, 7, 6]].iter().map(|p| p.to_vec()).collect();
        } else {
            let n = 128usize;
            let mut ring = vec![];
            for a in 0..n {
                let (sn, c) = cabnodes::sin_cos_from_fraction(a, n);
                ring.push([(-sn) * sx, c * sy]);
            }
            for z in [t, 0.0f32] {
                for p in &ring {
                    e.verts.push([p[0] as f64, p[1] as f64, z as f64]);
                }
            }
            let nn = n as u32;
            let mut top: Vec<u32> = (1..nn).collect();
            top.push(0);
            e.polys.push(top);
            let mut bot = vec![nn + 1, nn];
            bot.extend((nn + 2..2 * nn).rev());
            e.polys.push(bot);
            for i in 0..nn {
                let j = (i + 1) % nn;
                e.polys.push(vec![j, i, nn + i, nn + j]);
            }
        }
        Some(e)
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        if self.k == K::Oven {
            // oven.py create_placeholder: bbox of [door, racks, heater, body] + bbox of the control panel rotated by -0.1745 about (0,0,H)
            let (d, w, h, dt) = (self.depth, self.width, self.height, self.door_t);
            let hw = 0.05 * w;
            let handle_x = d + dt + hw + hw * 0.5;
            let body_hi = [handle_x.max(d + dt), w, h + dt];
            // control panel bbox (pre-rotation): cube + buttons + text glyph boxes (StringToCurves bounds at size 1: [xmin, ymin, xmax, ymax])
            let (pt, bt, rr, ph, bamount) = (self.panel_t, self.button_t, self.button_r, self.panel_h, self.buttons.max(1));
            let z0 = h + dt;
            let zc = z0 + ph / 2.0; // bbox centre of the panel cube
            let (mut plo, mut phi): ([f64; 3], [f64; 3]) = ([0.0, 0.0, z0], [pt, w, z0 + ph]);
            let mut ext = |lo: [f64; 3], hi: [f64; 3]| {
                for k in 0..3 {
                    plo[k] = plo[k].min(lo[k]);
                    phi[k] = phi[k].max(hi[k]);
                }
            };
            // text: translation T, glyph box g (size 1), size sz, extrude off: world (x, y, z) = (local z, local x, local y)
            let text = |t: [f64; 3], g: [f64; 4], sz: f64, off: f64| -> ([f64; 3], [f64; 3]) { ([t[0], t[1] + sz * g[0], t[2] + sz * g[1]], [t[0] + off, t[1] + sz * g[2], t[2] + sz * g[3]]) };
            const G_1201: [f64; 4] = [-0.9075, -0.009, 0.9095, 0.691];
            const G_OFF: [f64; 4] = [-0.6165, -0.009, 0.7075, 0.6914074];
            const G_HIGH: [f64; 4] = [-0.8625, -0.23, 0.8765, 0.682];
            const G_LOW: [f64; 4] = [-0.8065, -0.009, 0.8805, 0.682];
            const G_1: [f64; 4] = [-0.048, 0.0, 0.05, 0.682];
            let (tl, th) = text([pt, w / 2.0, zc], G_1201, 0.2 * ph, 0.002);
            ext(tl, th);
            let nd = bamount + 2;
            for idx in 0..nd {
                let fi = idx as f64;
                let half = nd as f64 * 0.5;
                if fi > half - 1.01 && fi < half + 0.99 {
                    continue;
                }
                let y = (fi + 1.0) * (w / (nd as f64 + 1.0));
                // button disc and ring (axis along x)
                ext([pt, y - rr - 0.008, zc - rr - 0.008], [pt + bt, y + rr + 0.008, zc + rr + 0.008]);
                let sz = 0.25 * rr;
                let (l, h2) = text([pt, y, zc + rr + 0.05 * ph], G_OFF, sz, 0.002);
                ext(l, h2);
                let (l, h2) = text([pt, y + rr * 0.7 + 0.05 * ph, zc + rr * 0.7 + 0.05 * ph], G_HIGH, sz, 0.002);
                ext(l, h2);
                let (l, h2) = text([pt, y + rr * -0.7 + -0.05 * ph, zc + rr * 0.7 + 0.05 * ph], G_LOW, sz, 0.002);
                ext(l, h2);
                let (l, h2) = text([pt + bt, y, zc], G_1, rr, 0.1 * bt);
                ext(l, h2);
            }
            let th = 0.1745f64;
            let (c, s) = (th.cos(), th.sin());
            let mut lo: [f64; 3] = [0.0, 0.0, 0.0];
            let mut hi = body_hi;
            for x in [plo[0], phi[0]] {
                for z in [plo[2], phi[2]] {
                    let (xr, zr) = (x * c - (z - h) * s, x * s + (z - h) * c + h);
                    lo[0] = lo[0].min(xr);
                    hi[0] = hi[0].max(xr);
                    hi[2] = hi[2].max(zr);
                    lo[2] = lo[2].min(zr);
                }
            }
            lo[1] = lo[1].min(plo[1]);
            hi[1] = hi[1].max(phi[1]);
            let mut a = Asset::default();
            a.cuboid(lo, hi, &Material::diffuse("oven_ph", [0.5; 3], 0.5));
            return Some(a);
        }
        if self.k == K::Rug {
            // elements/rug.py create_placeholder: new_bbox(-L/2, L/2, -W/2, W/2, 0, thickness)
            let mut a = Asset::default();
            a.cuboid([-self.depth / 2.0, -self.width / 2.0, 0.0], [self.depth / 2.0, self.width / 2.0, self.thickness], &Material::diffuse("rug", [0.5; 3], 0.9));
            return Some(a);
        }
        None
    }
}

pub fn make(c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Appliance::new(c, rng)))
}
