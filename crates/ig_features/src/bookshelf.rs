//! BookshelfWallFeature: floor-to-ceiling built-in shelving along a wall span (open shelves, optional closed lower cupboards with a counter ledge,
//! crown fascia, plinth), carving around windows. Also provides the cabinet-building kit reused by the window seat's flanking units.
use crate::util::*;
use crate::{Feature, FeatureAsset, Opening, WallSpan};
use ig_core::mesh::{Material, Mesh, V3};
use ig_core::rng::Rng;
use ig_kit::api::Asset;

/// all parameters of a cabinet run
#[derive(Clone)]
pub struct Style {
    pub d: f64,
    pub tb: f64,
    pub ts: f64,
    pub ph: f64,
    pub pr: f64,
    pub fascia: f64,
    pub fw: f64,
    pub lower_h: f64,
    pub ct: f64,
    pub co: f64,
    pub has_lower: bool,
    pub drawer_row: bool,
    pub upper_closed: bool,
    pub shaker: bool,
    pub bar_pull: bool,
    pub pitch_style: usize,
    pub aligned: bool,
}

#[derive(Default)]
pub struct Out {
    pub case: Mesh,
    pub door: Mesh,
    pub counter: Mesh,
    pub hw: Mesh,
    pub supports: Vec<(V3, V3)>,
}

impl Style {
    pub fn random(rng: &mut Rng, d: f64) -> Style {
        Style {
            d,
            tb: rng.uniform(0.018, 0.026),
            ts: rng.uniform(0.02, 0.03),
            ph: rng.uniform(0.07, 0.12),
            pr: rng.uniform(0.015, 0.04),
            fascia: rng.uniform(0.08, 0.16),
            fw: rng.uniform(0.035, 0.05),
            lower_h: rng.uniform(0.7, 0.9),
            ct: rng.uniform(0.03, 0.04),
            co: rng.uniform(0.02, 0.04),
            has_lower: rng.bool(0.65),
            drawer_row: rng.bool(0.45),
            upper_closed: rng.bool(0.3),
            shaker: rng.bool(0.6),
            bar_pull: rng.bool(0.4),
            pitch_style: rng.choice_idx(3),
            aligned: rng.bool(0.65),
        }
    }
}

pub fn plinth(st: &Style, o: &mut Out, a: f64, b: f64) {
    bx(&mut o.case, a, b, 0.0, st.d - st.pr, 0.0, st.ph);
}

/// crown molding + solid top fascia over [a,b]
pub fn crown(st: &Style, o: &mut Out, a: f64, b: f64, top: f64) {
    let d = st.d;
    bx(&mut o.case, a, b, 0.0, d + 0.012, top - st.fascia, top - 0.09);
    bx(&mut o.case, a, b, 0.0, d + 0.03, top - 0.09, top - 0.05);
    bx(&mut o.case, a, b, 0.0, d + 0.05, top - 0.05, top);
}

fn panel(st: &Style, o: &mut Out, x0: f64, zlo: f64, zhi: f64) {
    bx(&mut o.case, x0, x0 + st.ts, 0.0, st.d, zlo, zhi);
}

fn stile(st: &Style, o: &mut Out, xc: f64, edge: i32, zlo: f64, zhi: f64) {
    // face frame strip in front of a divider (edge: -1 left end, 1 right end, 0 interior)
    let (x0, x1) = match edge {
        -1 => (xc, xc + st.fw),
        1 => (xc - st.fw, xc),
        _ => (xc - st.fw / 2.0, xc + st.fw / 2.0),
    };
    bx(&mut o.case, x0, x1, st.d, st.d + 0.012, zlo, zhi);
}

fn knob(st: &Style, o: &mut Out, x: f64, z: f64, vertical: bool, y0: f64) {
    if st.bar_pull {
        let l = 0.11;
        let (px, pz) = if vertical { (0.0, l * 0.35) } else { (l * 0.35, 0.0) };
        peg(&mut o.hw, x - px, y0, z - pz, 0.028, 0.005);
        peg(&mut o.hw, x + px, y0, z + pz, 0.028, 0.005);
        if vertical {
            bx(&mut o.hw, x - 0.006, x + 0.006, y0 + 0.028, y0 + 0.038, z - l / 2.0 - 0.01, z + l / 2.0 + 0.01);
        } else {
            bx(&mut o.hw, x - l / 2.0 - 0.01, x + l / 2.0 + 0.01, y0 + 0.028, y0 + 0.038, z - 0.006, z + 0.006);
        }
    } else {
        peg(&mut o.hw, x, y0, z, 0.012, 0.004);
        peg(&mut o.hw, x, y0 + 0.012, z, 0.014, 0.013);
    }
}

/// one door / drawer front in the plane y = y0 (front of the carcass)
pub fn front(st: &Style, o: &mut Out, rng: &mut Rng, x0: f64, x1: f64, z0: f64, z1: f64, y0: f64, drawer: bool) {
    let dt = 0.02;
    let (w, h) = (x1 - x0, z1 - z0);
    let rw = (0.05f64).min(w * 0.3).min(h * 0.3);
    if st.shaker && w > 0.2 && h > 0.14 {
        bx(&mut o.door, x0, x1, y0, y0 + dt, z0, z0 + rw);
        bx(&mut o.door, x0, x1, y0, y0 + dt, z1 - rw, z1);
        bx(&mut o.door, x0, x0 + rw, y0, y0 + dt, z0 + rw, z1 - rw);
        bx(&mut o.door, x1 - rw, x1, y0, y0 + dt, z0 + rw, z1 - rw);
        bx(&mut o.door, x0 + rw * 0.8, x1 - rw * 0.8, y0, y0 + dt * 0.45, z0 + rw * 0.8, z1 - rw * 0.8);
    } else {
        bx(&mut o.door, x0, x1, y0, y0 + dt, z0, z1);
    }
    let yh = y0 + dt;
    if drawer {
        knob(st, o, (x0 + x1) / 2.0, (z0 + z1) / 2.0, false, yh);
    } else {
        let left = rng.bool(0.5);
        let kx = if left { x0 + 0.045 } else { x1 - 0.045 };
        let kz = if h > 0.9 { z0 + h * 0.45 } else if z0 < 0.4 { z1 - 0.08 } else { z0 + 0.08 };
        knob(st, o, kx, kz, true, yh);
    }
}

/// doors (and an optional drawer row) across [a,b] between z0 and z1, in the plane y = d
pub fn fronts(st: &Style, o: &mut Out, rng: &mut Rng, a: f64, b: f64, z0: f64, z1: f64, drawers_ok: bool) {
    let y0 = st.d;
    let g = 0.003;
    let mut zt = z1;
    let w = b - a;
    if drawers_ok && st.drawer_row && z1 - z0 > 0.45 {
        let dh = 0.17;
        let n = ((w / 0.55).round() as usize).max(1);
        let cw = w / n as f64;
        for i in 0..n {
            front(st, o, rng, a + cw * i as f64 + g, a + cw * (i + 1) as f64 - g, z1 - dh + g, z1 - g, y0, true);
        }
        zt = z1 - dh;
    }
    let n = if w > 0.65 { 2 } else { 1 };
    let cw = w / n as f64;
    for i in 0..n {
        front(st, o, rng, a + cw * i as f64 + g, a + cw * (i + 1) as f64 - g, z0 + g, zt - g, y0, false);
    }
}

fn pick_c(st: &Style, rng: &mut Rng, first: bool) -> f64 {
    const SET: [f64; 6] = [0.24, 0.28, 0.32, 0.36, 0.40, 0.44];
    match st.pitch_style {
        0 => 0.30 + 0.0 * rng.f64(),
        1 => {
            if first {
                rng.uniform(0.36, 0.46)
            } else {
                SET[rng.choice_idx(4)]
            }
        }
        _ => SET[rng.choice_idx(6)],
    }
}

/// clear compartment heights from `z` (top of the base board) up to `zi` (underside of the cap); returns shelf board bottoms
fn levels(st: &Style, rng: &mut Rng, zbase: f64, zi: f64) -> Vec<f64> {
    let mut z = zbase;
    let mut v = vec![];
    let mut first = true;
    loop {
        let c = pick_c(st, rng, first);
        first = false;
        let s = z + c;
        if zi - (s + st.tb) < 0.2 {
            break;
        }
        v.push(s);
        z = s + st.tb;
    }
    v
}

/// open compartments across [a,b] (x extents of the segment incl. its end panels) from zb (top of base board) to zi
fn open_bays(st: &Style, o: &mut Out, rng: &mut Rng, a: f64, b: f64, zb: f64, zi: f64, nb: usize, cap: bool) {
    let ts = st.ts;
    let step = (b - a - ts) / nb as f64;
    let shared = levels(st, rng, zb, zi);
    for k in 0..nb {
        let x0 = a + step * k as f64 + ts;
        let x1 = a + step * (k + 1) as f64;
        let lv = if st.aligned { shared.clone() } else { levels(st, rng, zb, zi) };
        for s in lv {
            bx(&mut o.case, x0, x1, 0.012, st.d - 0.004, s, s + st.tb);
            o.supports.push(([x0, 0.012, s], [x1, st.d - 0.004, s + st.tb]));
        }
        let _ = cap;
    }
}

/// full-height unit over [a,b]: panels at both ends and between bays, lower cupboard (optional), open shelves, optional closed top row
pub fn tall_unit(st: &Style, o: &mut Out, rng: &mut Rng, a: f64, b: f64, top: f64) {
    let (ts, tb, d) = (st.ts, st.tb, st.d);
    let zi = top - st.fascia;
    let w = b - a;
    if w < 0.3 {
        bx(&mut o.case, a, b, 0.0, d, st.ph, zi);
        plinth(st, o, a, b);
        return;
    }
    let nb = ((w / rng.uniform(0.6, 1.0)).round() as usize).max(1);
    let step = (w - ts) / nb as f64;
    plinth(st, o, a, b);
    // back panel
    bx(&mut o.case, a, b, 0.0, 0.012, st.ph, zi);
    for k in 0..=nb {
        let x = a + step * k as f64;
        panel(st, o, x, st.ph, zi);
        let edge = if k == 0 { -1 } else if k == nb { 1 } else { 0 };
        let xc = match edge {
            -1 => x,
            1 => x + ts,
            _ => x + ts / 2.0,
        };
        stile(st, o, xc, edge, st.ph, zi);
    }
    // base board + lower cupboard
    let mut zb = st.ph + tb;
    bx(&mut o.case, a + ts, b - ts, 0.0, d, st.ph, st.ph + tb);
    if st.has_lower {
        let lh = st.lower_h;
        // (the doors of each bay between its own panels: spaced on their own, a divider's face strip came through a door's panel)
        for k in 0..nb {
            fronts(st, o, rng, a + step * k as f64 + ts, a + step * (k + 1) as f64, st.ph + tb, lh, true);
        }
        zb = lh + st.ct;
        bx(&mut o.counter, a - 0.003, b + 0.003, 0.0, d + st.co, lh, lh + st.ct);
        o.supports.push(([a, 0.0, lh], [b, d + st.co, lh + st.ct]));
    } else {
        o.supports.push(([a + ts, 0.012, st.ph], [b - ts, d - 0.004, st.ph + tb]));
    }
    let mut zcap = zi;
    if st.upper_closed && zi - zb > 1.2 {
        let uh = rng.uniform(0.45, 0.7);
        let zu = zi - uh;
        zcap = zu - tb;
        bx(&mut o.case, a + ts, b - ts, 0.012, d, zu - tb, zu);
        for k in 0..nb {
            let x0 = a + step * k as f64 + ts;
            let x1 = a + step * (k + 1) as f64;
            fronts(st, o, rng, x0, x1, zu, zi, false);
        }
    }
    open_bays(st, o, rng, a, b, zb, zcap, nb, true);
}

/// closed cupboard front between the panels: doors across [a,b] from the base board to height `lh` (counter added by the caller if wanted)
pub fn under_block(st: &Style, o: &mut Out, rng: &mut Rng, a: f64, b: f64, lh: f64, _tall: bool) {
    let nb = (((b - a) / 0.7).round() as usize).max(1);
    let cw = (b - a) / nb as f64;
    for i in 0..nb {
        fronts(st, o, rng, a + cw * i as f64, a + cw * (i + 1) as f64, st.ph + st.tb, lh, true);
    }
}

pub struct BookshelfWallFeature;

fn blocked(span: &WallSpan) -> Vec<[f64; 2]> {
    let mut v: Vec<[f64; 2]> = vec![];
    for o in &span.openings {
        if !o.is_window || o.z[0] < 0.55 || o.z[1] > span.height - 0.35 {
            v.push([o.x[0] - 0.06, o.x[1] + 0.06]);
        }
    }
    v.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    v
}

impl Feature for BookshelfWallFeature {
    fn class(&self) -> &'static str {
        "BookshelfWallFeature"
    }
    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]> {
        if span.height < 2.2 || span.height > 3.8 {
            return vec![];
        }
        let mut out = vec![];
        let mut x = 0.0;
        let mut push = |a: f64, b: f64| {
            if b - a >= 1.2 {
                out.push([a, b]);
            }
        };
        for bl in blocked(span) {
            if bl[0] > x {
                push(x, bl[0].min(span.length));
            }
            x = x.max(bl[1]);
        }
        if x < span.length {
            push(x, span.length);
        }
        out
    }
    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset {
        self.build_max_depth(span, x, seed, f64::MAX)
    }
}

impl BookshelfWallFeature {
    /// alcove shelving beside a chimney breast: any width >= this (the free-wall minimum of [`Feature::fits`] is 1.2 m)
    pub const MIN_ALCOVE: f64 = 0.5;
    /// [`Feature::build`] with the shelf depth capped at `max_depth` (flush with a chimney breast); the random stream is the same
    pub fn build_max_depth(&self, span: &WallSpan, x: [f64; 2], seed: u64, max_depth: f64) -> FeatureAsset {
        let mut rng = Rng::seeded(seed ^ 0xb00c_5e1f);
        let d = rng.uniform(0.28, 0.4).min(max_depth);
        let st = Style::random(&mut rng, d);
        let top = if span.height > 2.6 && rng.bool(0.15) { rng.uniform(2.0, 2.4) } else { span.height };
        let (x0, x1) = (x[0], x[1]);
        let mut o = Out::default();
        let case_m = case_material(&mut rng, seed);
        let door_m = if rng.bool(0.7) { case_m.clone() } else { case_material(&mut rng, seed + 1) };
        let counter_m = match rng.choice_idx(3) {
            0 => case_m.clone(),
            1 => wood("blonde", seed + 2),
            _ => paint("counter_stone", [0.82, 0.8, 0.78]),
        };
        let hw_m = hardware(&mut rng);

        // carve-outs for windows
        let mut carves: Vec<(f64, f64, f64, f64)> = vec![];
        for w in span.openings.iter().filter(|w: &&Opening| w.is_window && w.x[1] > x0 + 0.05 && w.x[0] < x1 - 0.05) {
            let (a, b) = ((w.x[0] - 0.06).max(x0), (w.x[1] + 0.06).min(x1));
            if let Some(l) = carves.last_mut() {
                if a <= l.1 + 0.2 {
                    l.1 = l.1.max(b);
                    l.2 = l.2.min(w.z[0]);
                    l.3 = l.3.max(w.z[1]);
                    continue;
                }
            }
            carves.push((a, b, w.z[0], w.z[1]));
        }
        let mut cur = x0;
        let mut segs: Vec<(f64, f64)> = vec![];
        for c in &carves {
            if c.0 > cur {
                segs.push((cur, c.0));
            }
            cur = c.1;
        }
        if cur < x1 {
            segs.push((cur, x1));
        }
        for s in &segs {
            tall_unit(&st, &mut o, &mut rng, s.0, s.1, top);
        }
        let zi = top - st.fascia;
        for c in &carves {
            let (a, b, sill, head) = (c.0, c.1, c.2, c.3);
            // cupboard under the window
            let lh = (sill - st.ct - 0.03).min(0.95);
            plinth(&st, &mut o, a, b);
            bx(&mut o.case, a, b, 0.0, 0.012, st.ph, lh);
            bx(&mut o.case, a, b, 0.0, st.d, st.ph, st.ph + st.tb);
            if lh > 0.4 {
                under_block(&st, &mut o, &mut rng, a, b, lh, false);
                // (3 mm past the panels: flush, its free end fought with the end panel in a dashed line)
                bx(&mut o.counter, a - 0.003, b + 0.003, 0.0, st.d + st.co, lh, lh + st.ct);
                o.supports.push(([a, 0.0, lh], [b, st.d + st.co, lh + st.ct]));
            }
            // shelf row above the window head
            let zb = head + 0.03;
            if zi - zb > 0.3 {
                bx(&mut o.case, a, b, 0.0, 0.012, zb, zi);
                bx(&mut o.case, a, b, 0.0, st.d, zb - 0.0, zb + st.tb);
                o.supports.push(([a, 0.012, zb], [b, st.d - 0.004, zb + st.tb]));
                if zi - zb > 0.75 {
                    let s = zb + (zi - zb) * 0.5;
                    bx(&mut o.case, a, b, 0.012, st.d - 0.004, s, s + st.tb);
                    o.supports.push(([a, 0.012, s], [b, st.d - 0.004, s + st.tb]));
                }
            }
        }
        crown(&st, &mut o, x0, x1, top);

        let mut asset = Asset::default();
        let col = ([x0, 0.0, 0.0], [x1, st.d + 0.05, top]);
        asset.mesh(o.case, &case_m, Some(col));
        asset.mesh(o.door, &door_m, None);
        if !o.counter.is_empty() {
            asset.mesh(o.counter, &counter_m, None);
        }
        if !o.hw.is_empty() {
            asset.mesh(o.hw, &hw_m, None);
        }
        asset.supports = o.supports;
        let fp = vec![[x0, 0.0], [x1, 0.0], [x1, st.d + 0.05], [x0, st.d + 0.05]];
        FeatureAsset { asset, footprint: fp, keepout: vec![([x0, st.d, 0.0], [x1, st.d + 0.5, 2.0])], class: "BookshelfWallFeature" }
    }
}

#[allow(dead_code)]
fn _unused(_: &Material) {}
