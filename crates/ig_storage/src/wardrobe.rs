//! WardrobeFactory (not in infinigen): a bedroom wardrobe -- a tall carcass on a plinth with 2-4 hinged doors (optionally over a row of
//! drawers) or two overlapping sliding doors, bar or knob handles, in wood / painted plywood finishes.
//! Frame: x = depth (front toward +x), y = width, z = height, origin at the back-left-bottom corner (as appliances.rs).
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "WardrobeFactory"
}

const BODY: Weighted = &[("wood.Wood", 3.0), ("wood.WhitePlywood", 2.5), ("wood.BlondePlywood", 1.5), ("wood.BlackPlywood", 0.8), ("plastic.PlasticRough", 0.7)];
const HANDLE_M: Weighted = &[("metal.BrushedMetal", 2.0), ("metal.MetalBasic", 1.0), ("metal.BrushedBlackMetal", 1.5), ("wood.Wood", 0.7)];

pub struct Wardrobe {
    /// overall depth (handles, shaker frames, cornice included), width, height
    depth: f64,
    width: f64,
    height: f64,
    doors: usize,
    sliding: bool,
    drawers: bool,
    knobs: bool,
    plinth: f64,
    contrast_doors: bool,
    /// on tapered legs (the carcass raised to at least 11 cm); a stepped cornice over the doors
    legs: bool,
    cornice: bool,
    /// door style: 0 flat, 1 shaker (frame proud of the panel), 2 a mirror on every other tall door
    style: usize,
    /// the bay standing open (its door swung out ~86 deg, a rail of clothes inside)
    open_bay: Option<usize>,
    seed: u64,
}

const GAP: f64 = 0.003;

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    // wood.Wood stretches its grain along object x (here: depth), so tall door panels showed end-grain swirls: vertical grain instead
    if cls == "wood.Wood" {
        return spec_material(&format!("{name}:{seed}"), json!({"cls": "shader.wood.wood.shader_wood", "kwargs": {"vertical": true}, "seed": seed}), [0.45, 0.3, 0.18]);
    }
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.6, 0.6, 0.62])
}

impl Wardrobe {
    fn new(r: &mut Rng) -> Wardrobe {
        let sliding = r.f64() < 0.3;
        let mut doors = if sliding { 2 } else { r.randint(2, 5) as usize };
        let mut width = if sliding { r.uniform(1.4, 2.2) } else { doors as f64 * r.uniform(0.42, 0.55) };
        let mut depth = r.uniform(0.55, 0.63);
        let mut height = r.uniform(1.9, 2.25);
        // the solver's chosen size (furn::sizes 'exact', built-ins fill their wall span): door count follows the width
        let hinted = crate::api::size_hint().is_some();
        if let Some(h) = crate::api::size_hint() {
            (width, depth, height) = (h[0], h[1], h[2]);
            if !sliding {
                doors = ((width / 0.5).round() as usize).clamp(2, 6);
            }
        }
        let _ = hinted;
        let drawers = !sliding && r.f64() < 0.35;
        let knobs = r.f64() < 0.35;
        let plinth = r.uniform(0.04, 0.1);
        let contrast_doors = r.f64() < 0.2;
        let seed = r.randint(0, 1_000_000) as u64;
        // base, top and doors from their own streams: 30 % stand on tapered legs, 25 % have a stepped cornice over the doors (proud at
        // the front only), doors flat 50 % / shaker 30 % / mirrored 20 %; 25 % of hinged wardrobes stand with one bay open;
        // NO_WARDROBE_STYLES=1: plain, NO_WARDROBE_OPEN=1: closed
        let mut sb = Rng::seeded(crate::int_hash(&["wardrobe_base", &seed.to_string()]));
        let styled = std::env::var_os("NO_WARDROBE_STYLES").is_none();
        let legs = styled && sb.f64() < 0.3;
        let cornice_p = sb.f64() < 0.25;
        let hsh = crate::int_hash(&["wardrobe_open", &seed.to_string()]);
        let open_bay = if !sliding && std::env::var_os("NO_WARDROBE_OPEN").is_none() && !crate::api::closed_hint() && hsh % 4 == 0 { Some((hsh / 4) as usize % doors) } else { None };
        let cornice = styled && cornice_p && open_bay.is_none() && !sliding;
        let mut sq = Rng::seeded(crate::int_hash(&["wardrobe_style", &seed.to_string()]));
        let style = if !styled { 0 } else { sq.choice_p(&[0.5, 0.3, 0.2]) };
        // (WARDROBE_STYLE=0|1|2 forces one: debugging)
        let style = std::env::var("WARDROBE_STYLE").ok().and_then(|v| v.parse().ok()).unwrap_or(style);
        Wardrobe { depth, width, height, doors, sliding, drawers, knobs, plinth, contrast_doors, legs, cornice, style, open_bay, seed }
    }

    /// how far handles, shaker frames, mirrors and a cornice stand proud of the door fronts: the carcass is that much shallower than the
    /// wardrobe's depth (a wardrobe's handles stood 1.6 cm into the bed beside it, 923)
    fn proud(&self) -> f64 {
        // (the outer door face: a sliding pair's front panel stands 19 mm out; frames and mirrors stand on it)
        let face = if self.sliding { 0.019 } else { 0.0 };
        let mut p: f64 = face + if self.sliding { 0.006 } else if self.knobs { 0.025 } else { 0.037 };
        if self.style == 1 { p = p.max(face + 0.008); }
        if self.style == 2 { p = p.max(face + 0.004); }
        if self.cornice { p = p.max(0.03); }
        p
    }
    /// the carcass's front (the door fronts' plane)
    fn front(&self) -> f64 { self.depth - self.proud() }
    fn base_h(&self) -> f64 { if self.legs { self.plinth.max(0.11) } else { self.plinth } }
    /// the drawer row's top (the doors stand over it)
    fn zd(&self) -> f64 { let (p, h) = (self.base_h(), self.door_top()); if self.drawers { p + (h - p) * 0.2 } else { p } }
    fn door_top(&self) -> f64 { if self.cornice { self.height - 0.065 } else { self.height } }

    /// collision: the body (carcass, base, cornice, handles: its whole depth), the top it carries things on, a swing per hinged door
    /// (about its outer front edge, 100 deg) and a pull-out per drawer; sliding doors stay in the footprint
    fn vols(&self) -> Vec<crate::piece::Volume> {
        use crate::piece::{Leaf, Volume};
        let (d, w, h) = (self.depth, self.width, self.height);
        let f = self.front();
        let t = 0.02;
        let mut v = vec![Volume::hard([0.0, 0.0, -0.002], [d, w, h]), Volume::support([0.0, 0.0, h - 0.001], [f, w, h])];
        if self.sliding { return v }
        let (zd, hz) = (self.zd(), self.door_top());
        let dw = w / self.doors as f64;
        for i in 0..self.doors {
            let (y0, y1) = (dw * i as f64, dw * (i + 1) as f64);
            // the door's hinge: its outer edge (pairs open from the middle); turning out of the carcass, toward +x
            let (hy, axis) = if i % 2 == 0 { (y0, [0.0, 0.0, -1.0]) } else { (y1, [0.0, 0.0, 1.0]) };
            let panel = ([f - t, y0, zd], [d, y1, hz]);
            v.push(Volume::opening(Leaf::Hinged { panel, hinge: [f, hy, zd], axis, max_angle: 100f64.to_radians() }));
        }
        if self.drawers {
            let n = (self.doors / 2).max(1);
            let p = self.base_h();
            for i in 0..n {
                let (y0, y1) = (w * i as f64 / n as f64, w * (i + 1) as f64 / n as f64);
                let panel = ([(f - 0.45).max(0.02), y0, p], [d, y1, zd]);
                v.push(Volume::opening(Leaf::Drawer { panel, dir: [1.0, 0.0, 0.0], travel: 0.75 * (f - 0.02).min(0.45) }));
            }
        }
        v
    }

    fn build(&self) -> Asset {
        let mut a = Asset::default();
        let mut r = Rng::seeded(self.seed);
        let d = self.front();
        let w = self.width;
        let s = self.seed;
        let legs = self.legs;
        let p = self.base_h();
        let body_cls = weighted(&mut r, BODY);
        let door_cls = if self.contrast_doors { weighted(&mut r, BODY) } else { body_cls };
        let body = spec("wardrobe_body", body_cls, s);
        let door = spec("wardrobe_door", door_cls, s + 1);
        // 20 % painted (carcass and doors one colour; own stream, NO_PAINT=1: off)
        let (body, door) = if std::env::var_os("NO_PAINT").is_none() && crate::int_hash(&["wardrobe_paint", &self.seed.to_string()]) % 100 < 20 { let m = painted_material(&format!("wardrobe_paint:{s}"), s, [0.6, 0.6, 0.62]); (m.clone(), m) } else { (body, door) };
        let handle = spec("wardrobe_handle", weighted(&mut r, HANDLE_M), s + 2);
        let t = 0.02; // door thickness
        let x0 = d - t;
        // one bay open (door swung out): a hollow compartment with a rail of hanging clothes and folded stacks on a shelf above; the
        // cabinet body is built around it
        let dw0 = w / self.doors as f64;
        let open_bay = self.open_bay;
        let cornice = self.cornice;
        let (h, h_full) = (self.door_top(), self.height);
        let mut cab = Mesh::new();
        match open_bay {
            Some(bi) => {
                let (by0, by1) = (dw0 * bi as f64, dw0 * (bi + 1) as f64);
                if by0 > 0.0 { cab.append(&Mesh::cuboid([0.0, 0.0, p], [x0, by0, h])); }
                if by1 < w { cab.append(&Mesh::cuboid([0.0, by1, p], [x0, w, h])); }
                // (under the lining's floor and behind its front edge: level with them, the two fought in a black and white pattern)
                cab.append(&Mesh::cuboid([0.0, by0, p], [x0 - 0.001, by1, self.zd() + 0.016])); // the bay's floor, down to the plinth behind any drawers
                // (a carcass round it: at an end bay only the white lining stood there, a pale side stopping in mid air over the drawers' level)
                const SKIN: f64 = 0.012;
                cab.append(&Mesh::cuboid([0.0, by0, p], [SKIN, by1, h]));
                cab.append(&Mesh::cuboid([0.0, by0, h - SKIN], [x0 - 0.001, by1, h]));
                cab.append(&Mesh::cuboid([0.0, by0, p], [x0 - 0.001, by0 + SKIN, h]));
                cab.append(&Mesh::cuboid([0.0, by1 - SKIN, p], [x0 - 0.001, by1, h]));
            }
            None => cab.append(&Mesh::cuboid([0.0, 0.0, p], [x0, w, h])),
        }
        // recessed plinth, or legs set in from the corners (a middle pair under a wide one)
        if legs {
            let mut ys = vec![0.05, w - 0.05];
            if w > 1.4 { ys.push(w / 2.0); }
            for y in ys { for x in [0.05, x0 - 0.05] {
                let sx = if x > x0 / 2.0 { 0.012 } else { -0.012 };
                let m = limb([x, y, p], [x + sx, y, 0.0], 0.022, 0.014, true); let b = m.bounds(); a.mesh(m, &body, Some(b));
            }}
        } else {
            cab.append(&Mesh::cuboid([0.0, 0.02, 0.0], [x0 - 0.04, w - 0.02, p]));
        }
        if cornice {
            cab.append(&Mesh::cuboid([0.0, 0.0, h], [d + 0.012, w, h + 0.025]));
            cab.append(&Mesh::cuboid([0.0, 0.0, h + 0.025], [d + 0.03, w, h_full]));
        }
        let cb = cab.bounds();
        a.mesh(cab, &body, Some(cb));
        a.supports.push(([0.0, 0.0, h_full], [d, w, h_full + 0.001]));
        let style = self.style;
        let mirror_m = spec("wardrobe_mirror", "metal.Mirror", s + 7);
        let n_panel = std::cell::Cell::new(0usize);
        let panel = |a: &mut Asset, xa: f64, xb: f64, y0: f64, y1: f64, z0: f64, z1: f64| {
            let m = Mesh::cuboid([xa, y0 + GAP, z0 + GAP], [xb, y1 - GAP, z1 - GAP]);
            let b = m.bounds();
            a.mesh(m, &door, Some(b));
            let (dh, dw) = (z1 - z0, y1 - y0);
            let k = n_panel.get(); n_panel.set(k + 1);
            if style == 1 {
                // shaker: four rails 8 mm proud round the panel (narrower on drawers)
                let fw = 0.07f64.min(0.22 * dh).min(0.22 * dw);
                let (ya, yb, za, zb) = (y0 + GAP, y1 - GAP, z0 + GAP, z1 - GAP);
                for (lo, hi) in [([xb, ya, za], [xb + 0.008, yb, za + fw]), ([xb, ya, zb - fw], [xb + 0.008, yb, zb]),
                                 ([xb, ya, za + fw], [xb + 0.008, ya + fw, zb - fw]), ([xb, yb - fw, za + fw], [xb + 0.008, yb, zb - fw])] {
                    let m = Mesh::cuboid(lo, hi); let b = m.bounds(); a.mesh(m, &door, Some(b));
                }
            } else if style == 2 && dh > 1.2 && k % 2 == 0 {
                // a full-length mirror inset on the door
                let m = Mesh::cuboid([xb, y0 + 0.07, z0 + 0.12], [xb + 0.004, y1 - 0.07, z1 - 0.12]); let b = m.bounds(); a.mesh(m, &mirror_m, Some(b));
            }
        };
        let off = 0.03;
        let vbar = |a: &mut Asset, x: f64, y: f64, zc: f64, len: f64| {
            let mut m = limb([x + off, y, zc - len / 2.0], [x + off, y, zc + len / 2.0], 0.007, 0.007, true);
            for z in [zc - len / 2.0 + 0.015, zc + len / 2.0 - 0.015] {
                m.append(&limb([x, y, z], [x + off, y, z], 0.005, 0.005, false));
            }
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let knob = |a: &mut Asset, x: f64, y: f64, z: f64| {
            let m = limb([x, y, z], [x + 0.025, y, z], 0.012, 0.012, true);
            let b = m.bounds();
            a.mesh(m, &handle, Some(b));
        };
        let zc = p + (h - p) * 0.5;
        if self.sliding {
            // two panels, the left one in front, overlapping by 4 cm
            let half = w / 2.0;
            panel(&mut a, d + 0.001, d + 0.019, 0.0, half + 0.02, p, h);
            panel(&mut a, x0 + 0.001, d - 0.001, half - 0.02, w, p, h);
            // flat vertical pulls near the outer edges
            for (y, x) in [(0.05, d + 0.019), (w - 0.05, d - 0.001)] {
                let m = Mesh::cuboid([x, y - 0.012, zc - 0.2], [x + 0.006, y + 0.012, zc + 0.2]);
                let b = m.bounds();
                a.mesh(m, &handle, Some(b));
            }
            return a;
        }
        let zd = self.zd();
        if self.drawers {
            let n = (self.doors / 2).max(1);
            for i in 0..n {
                let (y0, y1) = (w * i as f64 / n as f64, w * (i + 1) as f64 / n as f64);
                panel(&mut a, x0, d, y0, y1, p, zd);
                let ym = (y0 + y1) / 2.0;
                let zm = (p + zd) / 2.0;
                if self.knobs {
                    knob(&mut a, d, ym, zm);
                } else {
                    let m = limb([d + off, ym - 0.08, zm], [d + off, ym + 0.08, zm], 0.007, 0.007, true);
                    let b = m.bounds();
                    a.mesh(m, &handle, Some(b));
                }
            }
        }
        let dw = w / self.doors as f64;
        let hz = zd + (h - zd) * 0.45;
        if let Some(bi) = open_bay {
            let (y0, y1) = (dw * bi as f64, dw * (bi + 1) as f64);
            self.open_bay(&mut a, &mut r, y0, y1, zd, x0);
        }
        for i in 0..self.doors {
            let (y0, y1) = (dw * i as f64, dw * (i + 1) as f64);
            if open_bay == Some(i) {
                // the door stands open ~86 deg on its hinge (the bay's outer edge), swung out into the room
                // (not past square: at 100 deg an end bay's door stood 10 cm out beside the carcass, into a door's swing next to it)
                let hinge_y = if i % 2 == 0 { y0 } else { y1 };
                let sgn = if i % 2 == 0 { 1.0 } else { -1.0 }; // +1: door spans toward +y when closed
                let ang: f64 = 1.5;
                let wd = y1 - y0 - 2.0 * GAP;
                // closed direction (0, sgn); rotated by ang about z toward +x (out of the cabinet)
                let dir = [ang.sin(), sgn * ang.cos()];
                let nrm = [dir[1] * -sgn, dir[0] * sgn];
                let v = |u: f64, tt: f64, z: f64| [d + dir[0] * u + nrm[0] * tt, hinge_y + dir[1] * u + nrm[1] * tt, z];
                let vs = [v(0.0, 0.0, zd + GAP), v(wd, 0.0, zd + GAP), v(wd, 0.0, h - GAP), v(0.0, 0.0, h - GAP), v(0.0, t, zd + GAP), v(wd, t, zd + GAP), v(wd, t, h - GAP), v(0.0, t, h - GAP)];
                let mut m = Mesh::new();
                for f in [[0, 1, 2, 3], [7, 6, 5, 4], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]] { m.add_face(&[vs[f[0]], vs[f[1]], vs[f[2]], vs[f[3]]]); }
                a.mesh(m, &door, None);
                continue;
            }
            panel(&mut a, x0, d, y0, y1, zd, h);
            // handle on the opening edge: doors pair up (left door opens at its right edge)
            let y = if i % 2 == 0 { y1 - 0.04 } else { y0 + 0.04 };
            if self.knobs {
                knob(&mut a, d, y, hz);
            } else {
                vbar(&mut a, d, y, hz, 0.3);
            }
        }
        a
    }
}

/// box-projected uvs (metres) per corner (meshes with per-face vertices, as Mesh::smoothed returns)
fn box_uv(m: &mut Mesh) {
    m.uv = vec![[0.0, 0.0]; m.pos.len()];
    for f in &m.faces {
        if f.len() < 3 { continue }
        let (a, b, c) = (m.pos[f[0] as usize], m.pos[f[1] as usize], m.pos[f[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let ax = (0..3).max_by(|&i, &j| n[i].abs().partial_cmp(&n[j].abs()).unwrap()).unwrap();
        for &i in f { let p = m.pos[i as usize]; m.uv[i as usize] = match ax { 0 => [p[1], p[2]], 1 => [p[0], p[2]], _ => [p[0], p[1]] }; }
    }
}

impl Wardrobe {
    /// hollow bay [y0, y1] x [0, x0] above zd: back + side boards + floor, a hanging rail with 6-14 garments (shirts / dresses / coats:
    /// thin rounded slabs hung from the rail at slightly different depths, lengths and colours, each on a hanger) and a top shelf with
    /// 2-3 folded stacks
    fn open_bay(&self, a: &mut Asset, r: &mut Rng, y0: f64, y1: f64, zd: f64, x0: f64) {
        let h = self.height;
        let wall = 0.018;
        let inside = Material::diffuse("wardrobe_inside", [0.82, 0.8, 0.76], 0.8);
        // carve: the cabinet body is solid; paint its interior faces with an inner shell slightly inside the bay
        let mut shell = Mesh::new();
        // (a millimetre inside the carcass, so only its inner faces and front edges show)
        const IN: f64 = 0.001;
        let (hi_z, xb) = (h - IN, IN);
        shell.append(&Mesh::cuboid([xb, y0 + wall, zd + wall], [0.02, y1 - wall, h - wall])); // back
        shell.append(&Mesh::cuboid([xb, y0 + IN, zd], [x0, y0 + wall, hi_z])); // sides
        shell.append(&Mesh::cuboid([xb, y1 - wall, zd], [x0, y1 - IN, hi_z]));
        shell.append(&Mesh::cuboid([xb, y0 + IN, zd], [x0, y1 - IN, zd + wall])); // floor
        shell.append(&Mesh::cuboid([xb, y0 + IN, h - wall], [x0, y1 - IN, hi_z])); // top
        a.mesh(shell, &inside, None);
        let shelf_z = h - 0.32;
        a.mesh(Mesh::cuboid([0.02, y0 + wall, shelf_z], [x0 - 0.01, y1 - wall, shelf_z + 0.018]), &inside, None);
        let rail_z = shelf_z - 0.06;
        let chrome = Material::diffuse("wardrobe_rail", [0.75, 0.75, 0.77], 0.25);
        a.mesh(ig_kit::geomkit::limb([x0 * 0.5, y0 + wall, rail_z], [x0 * 0.5, y1 - wall, rail_z], 0.012, 0.012, true), &chrome, None);
        // garments
        let pal: [[f32; 3]; 10] = [[0.08, 0.1, 0.18], [0.85, 0.85, 0.82], [0.25, 0.25, 0.27], [0.55, 0.42, 0.3], [0.6, 0.15, 0.15], [0.35, 0.45, 0.6], [0.15, 0.3, 0.2], [0.75, 0.65, 0.5], [0.45, 0.3, 0.45], [0.9, 0.75, 0.55]];
        let n = ((y1 - y0 - 2.0 * wall) / 0.045) as usize;
        let n = n.clamp(4, 14);
        let mut y = y0 + wall + 0.03;
        let step = (y1 - y0 - 2.0 * wall - 0.06) / n as f64;
        for k in 0..n {
            let len = [0.7f64, 0.75, 0.95, 1.15, 0.6][r.randint(0, 5) as usize].min(rail_z - zd - 0.05);
            let gw = r.uniform(0.44, 0.52).min(x0 - 0.06);
            let th = r.uniform(0.018, 0.04);
            let xc = x0 * 0.5 + r.uniform(-0.015, 0.015);
            let col = pal[r.randint(0, 10) as usize];
            let fab = spec_material(&format!("garment:{}:{k}", self.seed), json!({"cls": "fabric.FineKnitFabric", "kwargs": {}, "seed": self.seed + 100 + k as u64}), col);
            // body: a thin slab tapering at the shoulders
            let top = rail_z - 0.07;
            let mut g = Mesh::cuboid([xc - gw / 2.0, y - th / 2.0, top - len], [xc + gw / 2.0, y + th / 2.0, top]).smoothed(th * 0.45, 2, 1);
            for p in g.pos.iter_mut() {
                if p[2] > top - 0.08 { let f = 1.0 - (p[2] - (top - 0.08)) / 0.08 * 0.35; p[0] = xc + (p[0] - xc) * f; }
            }
            box_uv(&mut g);
            a.mesh(g, &fab, None);
            // hanger: hook + bar
            a.mesh(ig_kit::geomkit::limb([xc, y, rail_z], [xc, y, top + 0.005], 0.003, 0.003, false), &chrome, None);
            a.mesh(ig_kit::geomkit::limb([xc - gw * 0.42, y, top - 0.01], [xc + gw * 0.42, y, top - 0.01], 0.006, 0.006, true), &Material::diffuse("hanger", [0.5, 0.38, 0.25], 0.6), None);
            y += step;
        }
        // folded stacks on the shelf
        let ns = if y1 - y0 > 0.55 { 3 } else { 2 };
        for k in 0..ns {
            let yc = y0 + (y1 - y0) * (k as f64 + 0.5) / ns as f64;
            let mut z = shelf_z + 0.018;
            let layers = r.randint(3, 7);
            for l in 0..layers {
                let col = pal[r.randint(0, 10) as usize];
                let tt = r.uniform(0.025, 0.045);
                // (each stack within its share of the bay: two 0.3 m stacks in a 0.4 m bay stood through its side)
                let (sw, sd) = (r.uniform(0.25, 0.3).min((y1 - y0 - 2.0 * wall) / ns as f64 - 0.02), r.uniform(0.25, 0.3).min(x0 - 0.05));
                let jx = r.uniform(-0.01, 0.01);
                let mut m = Mesh::cuboid([x0 - sd - 0.02 + jx, yc - sw / 2.0, z], [x0 - 0.02 + jx, yc + sw / 2.0, z + tt]).smoothed(tt * 0.4, 2, 1);
                box_uv(&mut m);
                let fab = spec_material(&format!("folded:{}:{k}:{l}", self.seed), json!({"cls": "fabric.CoarseKnitFabric", "kwargs": {}, "seed": self.seed + 300 + (k * 10 + l as usize) as u64}), col);
                a.mesh(m, &fab, None);
                z += tt;
            }
        }
    }
}

impl Factory for Wardrobe {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        Some(([0.0, 0.0, 0.0], [self.depth + 0.045, self.width, self.height]))
    }
    fn asset(&self, _i: u64) -> Asset { self.build() }
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> { Some(self.vols()) }
}

pub fn make(_c: &str, rng: &mut Rng, _fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Wardrobe::new(&mut rng.clone())))
}
