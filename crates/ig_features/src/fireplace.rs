//! Masonry fireplace with a chimney breast, built flush against a wall span (span frame: x along the wall, y into the room, z up from the floor).
//!
//! Every design axis is drawn independently from the seed (see [`FireplaceLayout`]):
//!   * breast shape: plain floor-to-ceiling box | stepped (wider below the mantel) | tapered (chimney narrows and gets shallower above
//!     the mantel) | half height (masonry stops above the mantel, plain wall above)
//!   * firebox opening: rectangular | arched (segmental or semicircular) | Victorian cast-iron insert (iron plate with an arched opening,
//!     optional tiled slips either side); interior sooty or firebrick
//!   * mantel: simple shelf | full surround (legs + frieze + shelf) | shelf on corbels | none (modern opening)
//!   * finish: brick | painted / whitewashed brick | stone (honed or polished) | plaster | plaster with a tiled-slip panel | plaster with a slate panel
//!   * hearth: flush or raised slab | tiled field with a stone border | raised bench hearth running past the breast into the alcove(s)
//!   * extras: fireplace screen, log-store niche in a cheek, over-mantel mirror or art
//! Geometry is boxes and planar polygons (no booleans); every face has its own vertices, flat normals and metre UVs (brick / tile shaders
//! read the UV map). Collision: breast sections, mantel, hearth (+ bench) are separate parts with their own boxes. Support surfaces:
//! mantel top, hearth / bench top. The breast extents are returned by [`FireplaceFeature::layout`] so a bookshelf can fill the alcoves.
use crate::boxes::*;
use crate::{Feature, FeatureAsset, WallSpan};
use ig_core::materials::spec_material;
use ig_core::mesh::{Material, Mesh, V3};
use ig_core::rng::Rng;
use ig_kit::api::{Asset, Part};
use serde_json::json;

/// Surface finish of the breast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Brick,
    /// brick under paint (opaque) or whitewash (partial)
    PaintedBrick,
    Stone,
    Plaster,
    /// plaster breast with a panel of glazed tile slips around the opening
    TiledSlips,
    /// plaster breast with a dark slate panel around the opening
    Slate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreastShape {
    Plain,
    /// the part below the mantel is wider than the chimney above
    Stepped,
    /// above the mantel the chimney is narrower and shallower
    Tapered,
    /// masonry only up to `top_z`; plain wall above
    Half,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireboxShape {
    Rect,
    Arch,
    /// Victorian cast-iron insert: iron plate with an arched opening in front of the firebox
    Insert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MantelKind {
    Shelf,
    /// legs either side of the opening, a frieze across, shelf on top
    Surround,
    /// shelf carried by two corbel blocks
    Corbel,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HearthKind {
    Slab,
    /// tile field inside a stone border
    Tiled,
    /// raised seat-height hearth running along the wall past the breast
    Bench,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverMantel {
    None,
    Mirror,
    Art,
}

/// Resolved proportions and design choices of one fireplace (span frame, metres).
#[derive(Clone, Debug)]
pub struct FireplaceLayout {
    pub style: Style,
    pub shape: BreastShape,
    pub firebox_shape: FireboxShape,
    pub mantel_kind: MantelKind,
    pub hearth_kind: HearthKind,
    pub over_mantel: OverMantel,
    /// outer breast extent along the wall [x0, x1] (the widest section)
    pub breast: [f64; 2],
    /// extent of the section above `split_z` (= `breast` for plain / half)
    pub upper: [f64; 2],
    pub depth: f64,
    /// depth of the section above `split_z`
    pub upper_depth: f64,
    /// height where the lower section ends (mantel height, or above the opening without a mantel)
    pub split_z: f64,
    /// top of the masonry (ceiling, or lower for a half-height breast)
    pub top_z: f64,
    pub height: f64,
    /// firebox opening [x0, x1] and [z0, z1] (z1 = crown of the arch for arched openings)
    pub firebox: [f64; 2],
    pub firebox_z: [f64; 2],
    /// rise of the arch above its springing line (0 for a rectangular opening)
    pub arch_rise: f64,
    pub hearth: [f64; 2],
    pub hearth_depth: f64,
    pub hearth_h: f64,
    pub mantel_z: f64,
    pub mantel_t: f64,
    pub mantel_overhang: f64,
    pub mantel_depth: f64,
    /// free wall either side of the breast inside the interval given to `build` ([x0, x1] each, may be empty/narrow), for built-in units
    pub alcoves: [[f64; 2]; 2],
    pub logs: bool,
    pub grate: bool,
    pub screen: bool,
    pub firebrick: bool,
    /// log-store niche in the left (-1) or right (+1) cheek, 0 = none
    pub log_niche: i32,
    /// bench hearth extent along the wall (only for `HearthKind::Bench`)
    pub bench: [f64; 2],
}

pub struct FireplaceFeature;

fn room_ok(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    t.contains("living") || t.contains("bedroom") || t.contains("dining")
}

/// x intervals of the span clear of openings (a window or door breaks the floor-to-ceiling breast)
fn free_intervals(span: &WallSpan) -> Vec<[f64; 2]> {
    let mut blocked: Vec<[f64; 2]> = span
        .openings
        .iter()
        .map(|o| {
            let m = if o.is_window { 0.20 } else { 0.45 }; // doors need landing room beside them
            [o.x[0] - m, o.x[1] + m]
        })
        .collect();
    blocked.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let mut out = vec![];
    let mut cur = 0.0;
    for b in blocked {
        if b[0] > cur { out.push([cur, b[0].min(span.length)]); }
        cur = cur.max(b[1]);
    }
    if cur < span.length { out.push([cur, span.length]); }
    out.into_iter().filter(|i| i[1] - i[0] >= 1.4).collect()
}

impl FireplaceFeature {
    /// the proportions and design choices `build` will use for (span, interval, seed)
    pub fn layout(span: &WallSpan, x: [f64; 2], seed: u64) -> FireplaceLayout {
        let mut r = Rng::seeded(seed ^ 0xF1AE_91ACE);
        let free = x[1] - x[0];
        let h = span.height;
        // ---- design axes (drawn first so proportions can depend on them) ----
        let style = [Style::Brick, Style::PaintedBrick, Style::Stone, Style::Plaster, Style::TiledSlips, Style::Slate][r.choice_p(&[0.26, 0.14, 0.18, 0.16, 0.14, 0.12])];
        let shape = [BreastShape::Plain, BreastShape::Stepped, BreastShape::Tapered, BreastShape::Half][r.choice_p(&[0.40, 0.20, 0.25, 0.15])];
        let firebox_shape = [FireboxShape::Rect, FireboxShape::Arch, FireboxShape::Insert][r.choice_p(&[0.45, 0.30, 0.25])];
        let mantel_kind = [MantelKind::Shelf, MantelKind::Surround, MantelKind::Corbel, MantelKind::None][r.choice_p(&[0.38, 0.27, 0.20, 0.15])];
        let hearth_kind = [HearthKind::Slab, HearthKind::Tiled, HearthKind::Bench][r.choice_p(&[0.55, 0.25, 0.20])];
        // ---- breast footprint ----
        let w = r.uniform(1.2, 2.0).min(free - 0.05).max(1.2).min(free);
        let depth = r.uniform(0.30, 0.50);
        let slack = (free - w).max(0.0);
        let cx = x[0] + free / 2.0 + r.uniform(-0.15, 0.15) * slack;
        let cx = cx.clamp(x[0] + w / 2.0, x[1] - w / 2.0);
        // ---- firebox ----
        let fw = r.uniform(0.60, 0.90).min(w - 0.40);
        let fh = r.uniform(0.55, 0.80);
        let hearth_h = match hearth_kind {
            HearthKind::Bench => r.uniform(0.30, 0.45),
            _ => if r.bool(0.45) { r.uniform(0.08, 0.28) } else { 0.02 },
        };
        let arch_rise = match firebox_shape {
            FireboxShape::Rect => 0.0,
            // semicircle (rise = half the span) or a flatter segmental arch
            _ => if r.bool(0.4) { fw / 2.0 } else { fw / 2.0 * r.uniform(0.35, 0.7) },
        };
        let fz1 = hearth_h + fh.max(arch_rise + 0.35);
        // ---- mantel ----
        let mantel_t = r.uniform(0.04, 0.07);
        let mut mantel_z = r.uniform(1.10, 1.40).max(fz1 + 0.22).min(h - 0.6);
        if mantel_kind == MantelKind::Surround { mantel_z = mantel_z.max(fz1 + 0.28); }
        let mantel_overhang = r.uniform(0.04, 0.12);
        let mantel_depth = r.uniform(0.10, 0.18);
        // ---- sections ----
        let split_z = match mantel_kind {
            MantelKind::None => (fz1 + r.uniform(0.35, 0.6)).min(h - 0.5),
            _ => mantel_z,
        };
        let (breast, upper, upper_depth) = match shape {
            BreastShape::Stepped => {
                // widen below the mantel, limited by the interval
                let s = r.uniform(0.10, 0.22);
                let b = [(cx - w / 2.0 - s).max(x[0]), (cx + w / 2.0 + s).min(x[1])];
                (b, [cx - w / 2.0, cx + w / 2.0], depth)
            }
            BreastShape::Tapered => {
                let s = r.uniform(0.08, 0.2).min((w - 0.8) / 2.0).max(0.04);
                let ud = (depth - r.uniform(0.08, 0.15)).max(0.18);
                ([cx - w / 2.0, cx + w / 2.0], [cx - w / 2.0 + s, cx + w / 2.0 - s], ud)
            }
            _ => ([cx - w / 2.0, cx + w / 2.0], [cx - w / 2.0, cx + w / 2.0], depth),
        };
        let top_z = match shape {
            BreastShape::Half => (split_z + r.uniform(0.25, 0.7)).min(h - 0.3),
            _ => h,
        };
        // ---- hearth ----
        let hearth_depth = match hearth_kind { HearthKind::Bench => r.uniform(0.35, 0.50), _ => r.uniform(0.40, 0.50) };
        let hw = (breast[1] - breast[0]) / 2.0 + r.uniform(0.0, 0.12);
        let hearth = [cx - hw, cx + hw];
        let bench = if hearth_kind == HearthKind::Bench {
            // run past the breast on one or both sides into the alcove(s)
            let (l, rr) = match r.choice_p(&[0.4, 0.3, 0.3]) { 0 => (true, true), 1 => (true, false), _ => (false, true) };
            let ext = |r: &mut Rng| r.uniform(0.3, 0.9);
            let b0 = if l { (breast[0] - ext(&mut r)).max(x[0]) } else { breast[0] };
            let b1 = if rr { (breast[1] + ext(&mut r)).min(x[1]) } else { breast[1] };
            [b0.min(hearth[0]), b1.max(hearth[1])]
        } else {
            hearth
        };
        // ---- extras ----
        let logs = r.bool(0.7);
        let grate = r.bool(0.4) || firebox_shape == FireboxShape::Insert;
        let screen = hearth_depth >= 0.38 && hearth_kind != HearthKind::Bench && r.bool(0.3);
        let firebrick = r.bool(0.4);
        let cheek = ((cx - fw / 2.0) - breast[0]).min(breast[1] - (cx + fw / 2.0));
        let log_niche = if cheek >= 0.34 && mantel_z - hearth_h > 0.75 && r.bool(0.25) { if r.bool(0.5) { -1 } else { 1 } } else { 0 };
        let over_space = top_z - (mantel_z + mantel_t);
        let over_mantel = if mantel_kind != MantelKind::None && over_space >= 0.8 {
            [OverMantel::None, OverMantel::Mirror, OverMantel::Art][r.choice_p(&[0.5, 0.25, 0.25])]
        } else {
            OverMantel::None
        };
        FireplaceLayout {
            style,
            shape,
            firebox_shape,
            mantel_kind,
            hearth_kind,
            over_mantel,
            breast,
            upper,
            depth,
            upper_depth,
            split_z,
            top_z,
            height: h,
            firebox: [cx - fw / 2.0, cx + fw / 2.0],
            firebox_z: [hearth_h, fz1],
            arch_rise,
            hearth,
            hearth_depth,
            hearth_h,
            mantel_z,
            mantel_t,
            mantel_overhang,
            mantel_depth,
            alcoves: [[x[0], breast[0].min(bench[0])], [breast[1].max(bench[1]), x[1]]],
            logs,
            grate,
            screen,
            firebrick,
            log_niche,
            bench,
        }
    }
}

fn mat(name: String, cls: &str, kwargs: serde_json::Value, seed: u64, rgb: [f32; 3], rough: f32) -> Material {
    let mut m = spec_material(&name, json!({"cls": cls, "kwargs": kwargs, "seed": seed}), rgb);
    m.roughness = rough;
    m
}

/// procedural material whose base colour the renderer mixes toward `paint` by `amount` (render_rust.py: spec "paint")
fn painted(name: String, cls: &str, seed: u64, paint: [f32; 3], amount: f32, rough: f32) -> Material {
    let base = [0.42f32, 0.16, 0.11];
    let rgb = [0, 1, 2].map(|i| base[i] * (1.0 - amount) + paint[i] * amount);
    let mut m = spec_material(&name, json!({"cls": cls, "kwargs": {}, "seed": seed, "paint": {"rgb": paint, "amount": amount}}), rgb);
    m.roughness = rough;
    m
}

fn plain(name: &str, rgb: [f32; 3], rough: f32) -> Material {
    Material::diffuse(name, rgb, rough)
}

fn iron(seed: u64) -> Material {
    mat("fp_iron".into(), "metal.MetalBasic", json!({"color_hsv": [0.0, 0.0, 0.03]}), seed, [0.03, 0.03, 0.03], 0.45)
}

fn tile(name: &str, seed: u64, shape: &str, rgb: [f32; 3]) -> Material {
    mat(name.into(), "ceramic.Tile", json!({"shape": shape, "shader": "ceramic"}), seed, rgb, 0.3)
}

fn sub(a: V3, b: V3) -> V3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }

/// planar polygon with fresh vertices, wound so its normal points along `n`; UV = metres in the two axes most parallel to the plane
fn poly(m: &mut Mesh, pts: &[V3], n: V3) {
    let mut nn = [0.0f64; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        nn[0] += (a[1] - b[1]) * (a[2] + b[2]);
        nn[1] += (a[2] - b[2]) * (a[0] + b[0]);
        nn[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let mut p: Vec<V3> = pts.to_vec();
    if nn[0] * n[0] + nn[1] * n[1] + nn[2] * n[2] < 0.0 { p.reverse(); }
    let ax = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() { (1, 2) } else if n[1].abs() >= n[2].abs() { (0, 2) } else { (0, 1) };
    pad_uv(m);
    m.add_face(&p);
    for q in &p { m.uv.push([q[ax.0], q[ax.1]]); }
}

/// points of the opening's top edge from left to right: an arch (n segments) or the straight head
fn head_curve(x0: f64, x1: f64, spring: f64, rise: f64, n: usize) -> Vec<[f64; 2]> {
    if rise <= 1e-6 { return vec![[x0, spring], [x1, spring]]; }
    let half = (x1 - x0) / 2.0;
    // circle through (x0, spring), (x1, spring) and the crown (cx, spring + rise)
    let rad = (half * half + rise * rise) / (2.0 * rise);
    let (cx, cz) = ((x0 + x1) / 2.0, spring + rise - rad);
    // the arc runs over the top from the left springing point (angle in [pi/2, pi]) to the right one (angle in [0, pi/2]); for a semicircle
    // the centre lies on the springing line and atan2(-0.0, -r) would give -pi (the bottom half), so clamp both into the upper half
    let a0 = (spring - cz).max(0.0).atan2(x0 - cx);
    let a1 = (spring - cz).max(0.0).atan2(x1 - cx);
    (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        [cx + rad * a.cos(), cz + rad * a.sin()]
    }).collect()
}

/// front band between the opening head and z = `top` (at plane y), facing +y; spandrels of an arch or a plain strip
fn spandrel(m: &mut Mesh, curve: &[[f64; 2]], y: f64, top: f64) {
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        if top - a[1].max(b[1]) < 1e-6 && (top - a[1]).abs() < 1e-6 && (top - b[1]).abs() < 1e-6 { continue }
        poly(m, &[[a[0], y, a[1]], [b[0], y, b[1]], [b[0], y, top], [a[0], y, top]], [0.0, 1.0, 0.0]);
    }
}

/// soffit of the opening head between planes y0 (back) and y1 (front), facing down / toward the opening
fn soffit(m: &mut Mesh, curve: &[[f64; 2]], y0: f64, y1: f64) {
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        let t = sub([b[0], 0.0, b[1]], [a[0], 0.0, a[1]]);
        let n = [t[2], 0.0, -t[0]]; // tangent rotated: points below the curve
        poly(m, &[[a[0], y0, a[1]], [b[0], y0, b[1]], [b[0], y1, b[1]], [a[0], y1, a[1]]], n);
    }
}

impl Feature for FireplaceFeature {
    fn class(&self) -> &'static str { "FireplaceFeature" }

    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]> {
        if !room_ok(&span.room_type) || span.height < 2.2 { return vec![]; }
        free_intervals(span)
    }

    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset {
        let l = Self::layout(span, x, seed);
        let mut r = Rng::seeded(seed ^ 0x57A1_E5);
        let ms = r.randint(0, 1_000_000) as u64;
        let (bx0, bx1) = (l.breast[0], l.breast[1]);
        let (ux0, ux1) = (l.upper[0], l.upper[1]);
        let (fx0, fx1) = (l.firebox[0], l.firebox[1]);
        let (fz0, fz1) = (l.firebox_z[0], l.firebox_z[1]);
        let spring = fz1 - l.arch_rise;
        let (d, du) = (l.depth, l.upper_depth);
        let (zs, zt) = (l.split_z, l.top_z);
        let cd = d - 0.03; // firebox depth (it cuts into the breast, the wall stays behind)
        let hz = l.hearth_h;
        let y_back = d - cd;
        let curve = head_curve(fx0, fx1, spring, l.arch_rise, 16);

        // ---- materials ----
        const PAINTS: [[f32; 3]; 6] = [[0.92, 0.91, 0.88], [0.86, 0.84, 0.78], [0.08, 0.08, 0.09], [0.27, 0.35, 0.30], [0.55, 0.58, 0.60], [0.70, 0.62, 0.52]];
        let stone_mat = |r: &mut Rng, name: &str, s: u64| -> Material {
            if r.bool(0.5) { mat(name.into(), "ceramic.MarbleRegular", json!({}), s, [0.75, 0.74, 0.72], 0.35) } else { mat(name.into(), "ceramic.Concrete", json!({}), s, [0.58, 0.56, 0.52], 0.85) }
        };
        let wood_mat = |name: &str, s: u64| mat(name.into(), "wood.Wood", json!({}), s, [0.38, 0.23, 0.12], 0.5);
        let plaster = |s: u64| mat("fp_plaster".into(), "ceramic.Plaster", json!({"plaster_colored": false}), s, [0.88, 0.86, 0.82], 0.9);
        let (surround, panel): (Material, Option<Material>) = match l.style {
            Style::Brick => (mat("fp_brick".into(), "ceramic.Brick", json!({}), ms, [0.42, 0.16, 0.11], 0.9), None),
            Style::PaintedBrick => {
                let c = PAINTS[r.choice_p(&[0.35, 0.2, 0.15, 0.1, 0.1, 0.1])];
                let amount = if r.bool(0.35) { r.uniform(0.45, 0.65) as f32 } else { r.uniform(0.85, 0.95) as f32 }; // whitewash vs paint
                (painted("fp_brick_painted".into(), "ceramic.Brick", ms, c, amount, 0.85), None)
            }
            Style::Stone => (stone_mat(&mut r, "fp_stone_breast", ms), None),
            Style::Plaster => (plaster(ms), None),
            Style::TiledSlips => {
                let shapes = ["rectangle", "square", "staggered", "hexagon"];
                let sh = shapes[r.choice_p(&[0.4, 0.25, 0.2, 0.15])];
                (plaster(ms), Some(tile("fp_slips", ms + 5, sh, [0.25, 0.4, 0.42])))
            }
            Style::Slate => (plaster(ms), Some(painted("fp_slate".into(), "ceramic.Concrete", ms + 5, [0.12, 0.13, 0.14], 0.85, 0.6))),
        };
        // the mantel / surround / trim material
        let mantel_mat = match (l.style, r.f64()) {
            (Style::Stone, _) => surround.clone(),
            (_, u) if u < 0.5 => wood_mat("fp_mantel_wood", ms + 3),
            (_, u) if u < 0.65 => painted("fp_mantel_painted".into(), "wood.Wood", ms + 3, PAINTS[0], 0.92, 0.5),
            _ => stone_mat(&mut r, "fp_mantel_stone", ms + 3),
        };
        let border_mat = stone_mat(&mut r, "fp_hearth", ms + 2);
        let interior = if l.firebrick { painted("fp_firebrick".into(), "ceramic.Brick", ms + 6, [0.42, 0.33, 0.25], 0.55, 0.95) } else { plain("fp_soot", [0.025, 0.022, 0.02], 0.95) };

        // ---- breast: lower section (0 .. zs) around the opening, upper section (zs .. zt) ----
        let mut breast = Mesh::new();
        let top_l = if l.shape == BreastShape::Plain { 0 } else { PZ };
        let niche_x = if l.log_niche != 0 {
            let (c0, c1) = if l.log_niche < 0 { (bx0, fx0) } else { (fx1, bx1) };
            let nw = ((c1 - c0) - 0.16).min(0.55);
            let m = (c0 + c1) / 2.0;
            Some([m - nw / 2.0, m + nw / 2.0])
        } else { None };
        let niche_z = [hz + 0.06, (hz + 0.06 + r.uniform(0.40, 0.55)).min(zs - 0.15)];
        let cheek = |m: &mut Mesh, a: f64, b: f64, side: u8| {
            // a cheek [a, b], optionally with the log niche cut out of it (split into four boxes around the hole)
            match niche_x {
                Some(n) if n[0] >= a - 1e-9 && n[1] <= b + 1e-9 => {
                    add_box(m, [a, 0.0, 0.0], [b, d, niche_z[0]], side | PY);
                    add_box(m, [a, 0.0, niche_z[1]], [b, d, zs], side | PY | top_l);
                    add_box(m, [a, 0.0, niche_z[0]], [n[0], d, niche_z[1]], (side & NX) | PY);
                    add_box(m, [n[1], 0.0, niche_z[0]], [b, d, niche_z[1]], (side & PX) | PY);
                }
                _ => add_box(m, [a, 0.0, 0.0], [b, d, zs], side | PY | top_l),
            }
        };
        cheek(&mut breast, bx0, fx0, NX);
        cheek(&mut breast, fx1, bx1, PX);
        // over the opening: plain lintel from the crown up, spandrels between the head curve and the crown
        add_box(&mut breast, [fx0, 0.0, fz1], [fx1, d, zs], PY | top_l);
        spandrel(&mut breast, &curve, d, fz1);
        let mut upper = Mesh::new();
        if zt > zs + 1e-6 {
            let top = if zt < l.height - 1e-6 { PZ } else { 0 };
            add_box(&mut upper, [ux0, 0.0, zs], [ux1, du, zt], NX | PX | PY | top);
        }

        // ---- firebox interior ----
        let mut fire = Mesh::new();
        poly(&mut fire, &[[fx0, y_back, fz0], [fx1, y_back, fz0], [fx1, y_back, spring], [fx0, y_back, spring]], [0.0, 1.0, 0.0]);
        for w in curve.windows(2) {
            let (a, b) = (w[0], w[1]);
            if l.arch_rise > 0.0 { poly(&mut fire, &[[a[0], y_back, spring], [b[0], y_back, spring], [b[0], y_back, b[1]], [a[0], y_back, a[1]]], [0.0, 1.0, 0.0]); }
        }
        poly(&mut fire, &[[fx0, y_back, fz0], [fx0, d, fz0], [fx0, d, spring], [fx0, y_back, spring]], [1.0, 0.0, 0.0]);
        poly(&mut fire, &[[fx1, y_back, fz0], [fx1, d, fz0], [fx1, d, spring], [fx1, y_back, spring]], [-1.0, 0.0, 0.0]);
        soffit(&mut fire, &curve, y_back, d);
        let mut ash = Mesh::new();
        add_box(&mut ash, [fx0 + 0.02, y_back + 0.01, fz0], [fx1 - 0.02, d - 0.05, fz0 + 0.03], PX | NX | PY | NY | PZ);

        // ---- insert plate / surround panel ----
        let mut ironm = Mesh::new();
        let mut panel_m = Mesh::new();
        if l.firebox_shape == FireboxShape::Insert {
            // iron plate in front of the opening: side stiles, a head band over the arch, a hood lip
            let (m, t) = (r.uniform(0.08, 0.14), 0.015);
            let (px0, px1, pz1) = (fx0 - m, fx1 + m, fz1 + r.uniform(0.08, 0.15));
            add_box(&mut ironm, [px0, d, fz0], [fx0, d + t, pz1], NX | PX | PY | PZ);
            add_box(&mut ironm, [fx1, d, fz0], [px1, d + t, pz1], NX | PX | PY | PZ);
            spandrel(&mut ironm, &curve, d + t, pz1);
            add_box(&mut ironm, [fx0, d, fz1], [fx1, d + t, pz1], PZ);
            soffit(&mut ironm, &curve, d, d + t);
            add_box(&mut ironm, [px0 - 0.01, d, fz0 + 0.02], [px1 + 0.01, d + 0.05, fz0 + 0.05], ALL & !NY); // hood lip / fret
            // tiled slips either side of the plate (if the finish has a panel, or 50% of inserts)
            if panel.is_some() || r.bool(0.5) {
                let sw = r.uniform(0.10, 0.18);
                add_box(&mut panel_m, [px0 - sw, d, fz0], [px0, d + 0.008, pz1], NX | PY | PZ);
                add_box(&mut panel_m, [px1, d, fz0], [px1 + sw, d + 0.008, pz1], PX | PY | PZ);
            }
        } else if panel.is_some() {
            // a panel framing the opening (slips or slate), flush-mounted 8 mm proud
            let pw = r.uniform(0.12, 0.30);
            let pz1 = (fz1 + pw).min(zs - 0.02);
            add_box(&mut panel_m, [fx0 - pw, d, fz0], [fx0, d + 0.008, pz1], NX | PY | PZ);
            add_box(&mut panel_m, [fx1, d, fz0], [fx1 + pw, d + 0.008, pz1], PX | PY | PZ);
            spandrel(&mut panel_m, &curve, d + 0.008, pz1);
            add_box(&mut panel_m, [fx0, d, fz1], [fx1, d + 0.008, pz1], PZ);
            soffit(&mut panel_m, &curve, d, d + 0.008);
        }

        // ---- mantel ----
        let (mz0, mz1) = (l.mantel_z, l.mantel_z + l.mantel_t);
        let mut mantel = Mesh::new();
        let mut mantel_box: Option<(V3, V3)> = None;
        let mut legs_box: Vec<(V3, V3)> = vec![];
        let (mut mx0, mut mx1, mut my1) = (bx0 - l.mantel_overhang, bx1 + l.mantel_overhang, d + l.mantel_depth);
        match l.mantel_kind {
            MantelKind::None => {}
            MantelKind::Shelf => {
                add_box(&mut mantel, [mx0, d - 0.001, mz0], [mx1, my1, mz1], ALL & !NY);
                mantel_box = Some(([mx0, d - 0.001, mz0], [mx1, my1, mz1]));
            }
            MantelKind::Corbel => {
                add_box(&mut mantel, [mx0, d - 0.001, mz0], [mx1, my1, mz1], ALL & !NY);
                mantel_box = Some(([mx0, d - 0.001, mz0], [mx1, my1, mz1]));
                let (cw, ch) = (r.uniform(0.06, 0.10), r.uniform(0.14, 0.24));
                for cxx in [bx0 + 0.05, bx1 - 0.05 - cw] {
                    // stepped corbel: deep upper block, shallower lower block
                    add_box(&mut mantel, [cxx, d - 0.001, mz0 - ch * 0.5], [cxx + cw, d + l.mantel_depth * 0.8, mz0], ALL & !NY & !PZ);
                    add_box(&mut mantel, [cxx, d - 0.001, mz0 - ch], [cxx + cw, d + l.mantel_depth * 0.45, mz0 - ch * 0.5], ALL & !NY & !PZ);
                }
            }
            MantelKind::Surround => {
                // legs (pilasters) either side of the opening, frieze across, shelf on top
                let lw = r.uniform(0.12, 0.20).min(fx0 - bx0 - 0.02).max(0.06);
                let gap = r.uniform(0.04, 0.10);
                let (l0, l1) = ((fx0 - gap - lw).max(bx0), (fx1 + gap + lw).min(bx1));
                let proud = r.uniform(0.04, 0.07);
                let fh = r.uniform(0.14, 0.24).min(mz0 - fz1 - 0.02).max(0.08);
                add_box(&mut mantel, [l0, d - 0.001, hz], [l0 + lw, d + proud, mz0 - fh], ALL & !NY & !PZ);
                add_box(&mut mantel, [l1 - lw, d - 0.001, hz], [l1, d + proud, mz0 - fh], ALL & !NY & !PZ);
                add_box(&mut mantel, [l0, d - 0.001, mz0 - fh], [l1, d + proud, mz0], ALL & !NY & !PZ);
                legs_box.push(([l0, d - 0.001, hz], [l1, d + proud, mz0]));
                // plinth blocks at the foot of each leg
                for lx in [l0, l1 - lw] { add_box(&mut mantel, [lx - 0.01, d - 0.001, hz], [lx + lw + 0.01, d + proud + 0.01, hz + 0.10], ALL & !NY & !NZ); }
                mx0 = l0 - l.mantel_overhang;
                mx1 = l1 + l.mantel_overhang;
                my1 = d + proud + r.uniform(0.06, 0.12);
                add_box(&mut mantel, [mx0, d - 0.001, mz0], [mx1, my1, mz1], ALL & !NY);
                mantel_box = Some(([mx0, d - 0.001, mz0], [mx1, my1, mz1]));
            }
        }

        // ---- hearth ----
        let (hx0, hx1) = (l.hearth[0], l.hearth[1]);
        let hy1 = d + l.hearth_depth;
        let mut hearth = Mesh::new();
        let mut hearth_tiles = Mesh::new();
        let mut supports: Vec<(V3, V3)> = vec![];
        let mut cols: Vec<(V3, V3)> = vec![];
        match l.hearth_kind {
            HearthKind::Slab => {
                add_box(&mut hearth, [hx0, 0.0, 0.0], [hx1, hy1, hz], ALL & !NY & !NZ);
                supports.push(([hx0, d, 0.0], [hx1, hy1, hz]));
                cols.push(([hx0, 0.0, 0.0], [hx1, hy1, hz]));
            }
            HearthKind::Tiled => {
                let b = r.uniform(0.04, 0.08);
                add_box(&mut hearth, [hx0, 0.0, 0.0], [hx1, hy1, hz], NX | PX | PY);
                // border ring on top, tile field inside it
                add_box(&mut hearth, [hx0, d, hz - 0.001], [hx1, hy1, hz], PZ);
                add_box(&mut hearth_tiles, [hx0 + b, d, hz], [hx1 - b, hy1 - b, hz + 0.004], PZ | PY | NX | PX);
                supports.push(([hx0, d, 0.0], [hx1, hy1, hz + 0.004]));
                cols.push(([hx0, 0.0, 0.0], [hx1, hy1, hz]));
            }
            HearthKind::Bench => {
                let (b0, b1) = (l.bench[0], l.bench[1]);
                let by1 = d.max(0.0) + l.hearth_depth;
                // bench body: in front of the breast and along the wall in the alcoves (behind = wall)
                add_box(&mut hearth, [b0, 0.0, 0.0], [b1, by1, hz], NX | PX | PY | PZ);
                let top = r.uniform(0.04, 0.06);
                add_box(&mut hearth, [b0 - 0.02, 0.0, hz], [b1 + 0.02, by1 + 0.03, hz + top], ALL & !NY & !NZ);
                supports.push(([b0, 0.0, 0.0], [b1, by1 + 0.03, hz + top]));
                cols.push(([b0, 0.0, 0.0], [b1 + 0.02, by1 + 0.03, hz + top]));
            }
        }
        // nothing is set down in front of the fire: the hearth / bench top only supports items beside the firebox opening
        let keep = [l.firebox[0] - 0.08, l.firebox[1] + 0.08];
        supports = supports.into_iter().flat_map(|(lo, hi)| {
            [[lo[0], hi[0].min(keep[0])], [lo[0].max(keep[1]), hi[0]]].into_iter()
                .filter(|x| x[1] - x[0] >= 0.12)
                .map(move |x| ([x[0], lo[1], lo[2]], [x[1], hi[1], hi[2]]))
        }).collect();

        // ---- logs, grate, screen, niche, over-mantel ----
        let mut logs = Mesh::new();
        let mut grate = Mesh::new();
        let log_z = fz0 + 0.03 + if l.grate { 0.06 } else { 0.0 };
        if l.logs {
            let fw = fx1 - fx0;
            let yc = y_back + 0.03 + (cd - 0.09) * 0.5;
            let rad = r.uniform(0.04, 0.06).min((cd - 0.09) / 4.0 - 0.003);
            let len = (fw - 0.12).min(r.uniform(0.40, 0.55));
            let xc = (fx0 + fx1) / 2.0 + r.uniform(-0.04, 0.04);
            for (i, dy) in [-1.0f64, 1.0].iter().enumerate() {
                let yy = yc + dy * (rad + 0.003);
                add_cylinder(&mut logs, [xc - len / 2.0 + i as f64 * 0.02, yy, log_z + rad], [xc + len / 2.0 - i as f64 * 0.02, yy, log_z + rad], rad, 12);
            }
            add_cylinder(&mut logs, [xc - len / 2.0, yc, log_z + rad * 2.6], [xc + len / 2.0, yc, log_z + rad * 2.6], rad * 0.95, 12);
        }
        if l.grate {
            let (xa, xb) = (fx0 + 0.06, fx1 - 0.06);
            let (ya, yb) = (y_back + 0.04, d - 0.06);
            let zz = fz0 + 0.03 + 0.05;
            for yy in [ya, yb] { add_box(&mut grate, [xa, yy - 0.01, zz], [xb, yy + 0.01, zz + 0.02], ALL); }
            let n = 6;
            for i in 0..n {
                let xx = xa + (xb - xa) * i as f64 / (n - 1) as f64;
                add_box(&mut grate, [xx - 0.008, ya, zz], [xx + 0.008, yb, zz + 0.015], ALL);
            }
            for xx in [xa, xb] { for yy in [ya, yb] { add_box(&mut grate, [xx - 0.012, yy - 0.012, fz0 + 0.03], [xx + 0.012, yy + 0.012, zz], ALL); } }
        }
        let mut screen = Mesh::new();
        let mut mesh_panel = Mesh::new();
        if l.screen {
            // flat screen standing on the hearth: iron frame + dark mesh panel + two feet
            let (sx0, sx1) = (fx0 - 0.06, fx1 + 0.06);
            let sy = d + r.uniform(0.12, 0.2).min(l.hearth_depth - 0.08);
            let sz1 = hz + (fz1 - fz0) * r.uniform(0.55, 0.75);
            let (t, f) = (0.012, 0.02);
            add_box(&mut screen, [sx0, sy, hz + 0.03], [sx0 + f, sy + t, sz1], ALL);
            add_box(&mut screen, [sx1 - f, sy, hz + 0.03], [sx1, sy + t, sz1], ALL);
            add_box(&mut screen, [sx0, sy, sz1 - f], [sx1, sy + t, sz1], ALL);
            add_box(&mut screen, [sx0, sy, hz + 0.03], [sx1, sy + t, hz + 0.03 + f], ALL);
            for fxx in [sx0 + 0.05, sx1 - 0.08] { add_box(&mut screen, [fxx, sy - 0.07, hz], [fxx + 0.03, sy + t + 0.07, hz + 0.03], ALL & !NZ); }
            add_box(&mut mesh_panel, [sx0 + f, sy + 0.004, hz + 0.03 + f], [sx1 - f, sy + 0.008, sz1 - f], PY | NY);
        }
        let mut niche = Mesh::new();
        let mut niche_logs = Mesh::new();
        if let Some(n) = niche_x {
            let ny0 = (d - 0.26).max(0.03);
            let (z0, z1) = (niche_z[0], niche_z[1]);
            poly(&mut niche, &[[n[0], ny0, z0], [n[1], ny0, z0], [n[1], ny0, z1], [n[0], ny0, z1]], [0.0, 1.0, 0.0]);
            poly(&mut niche, &[[n[0], ny0, z0], [n[0], d, z0], [n[0], d, z1], [n[0], ny0, z1]], [1.0, 0.0, 0.0]);
            poly(&mut niche, &[[n[1], ny0, z0], [n[1], d, z0], [n[1], d, z1], [n[1], ny0, z1]], [-1.0, 0.0, 0.0]);
            poly(&mut niche, &[[n[0], ny0, z1], [n[1], ny0, z1], [n[1], d, z1], [n[0], d, z1]], [0.0, 0.0, -1.0]);
            poly(&mut niche, &[[n[0], ny0, z0], [n[1], ny0, z0], [n[1], d, z0], [n[0], d, z0]], [0.0, 0.0, 1.0]);
            // stacked split logs along y (end grain to the room)
            let rad = r.uniform(0.035, 0.05);
            let cols_n = (((n[1] - n[0]) - 0.02) / (2.0 * rad + 0.004)).floor().max(1.0) as usize;
            let rows = (((z1 - z0) * r.uniform(0.6, 0.95)) / (2.0 * rad + 0.004)).floor().max(1.0) as usize;
            for row in 0..rows {
                for c in 0..cols_n {
                    let xx = n[0] + 0.01 + rad + c as f64 * (2.0 * rad + 0.004) + if row % 2 == 1 { rad * 0.5 } else { 0.0 };
                    if xx + rad > n[1] - 0.005 { continue }
                    let zz = z0 + rad + row as f64 * (2.0 * rad + 0.002);
                    add_cylinder(&mut niche_logs, [xx, ny0 + 0.01, zz], [xx, d - 0.015 - r.uniform(0.0, 0.04), zz], rad * r.uniform(0.85, 1.0), 8);
                }
            }
        }
        let mut frame = Mesh::new();
        let mut glass = Mesh::new();
        if l.over_mantel != OverMantel::None {
            let wall_y = if l.shape == BreastShape::Half && mz1 + 0.15 > zt { 0.0 } else { du };
            let (ax0, ax1) = (ux0, ux1);
            let aw = ((ax1 - ax0) * r.uniform(0.5, 0.75)).max(0.4);
            let space = (zt.min(l.height) - mz1 - 0.15).max(0.5);
            let ah = (aw * r.uniform(0.7, 1.3)).min(space * 0.8).max(0.35);
            let z0 = mz1 + r.uniform(0.08, 0.22);
            let acx = (ax0 + ax1) / 2.0;
            let (x0, x1, z1) = (acx - aw / 2.0, acx + aw / 2.0, z0 + ah);
            let ft = r.uniform(0.03, 0.06);
            let fd = 0.03;
            add_box(&mut frame, [x0, wall_y, z0], [x0 + ft, wall_y + fd, z1], ALL & !NY);
            add_box(&mut frame, [x1 - ft, wall_y, z0], [x1, wall_y + fd, z1], ALL & !NY);
            add_box(&mut frame, [x0 + ft, wall_y, z1 - ft], [x1 - ft, wall_y + fd, z1], ALL & !NY);
            add_box(&mut frame, [x0 + ft, wall_y, z0], [x1 - ft, wall_y + fd, z0 + ft], ALL & !NY);
            add_box(&mut glass, [x0 + ft, wall_y, z0 + ft], [x1 - ft, wall_y + fd * 0.5, z1 - ft], PY);
        }

        // ---- assemble ----
        let mut parts: Vec<Part> = vec![Part { mesh: breast, mat: surround.clone(), col: Some(([bx0, 0.0, 0.0], [bx1, d, zs])) }];
        if !upper.is_empty() { parts.push(Part { mesh: upper, mat: surround, col: Some(([ux0, 0.0, zs], [ux1, du, zt])) }); }
        parts.push(Part { mesh: fire, mat: interior, col: None });
        parts.push(Part { mesh: ash, mat: plain("fp_ash", [0.07, 0.065, 0.06], 0.95), col: None });
        if !mantel.is_empty() {
            let col = match (mantel_box, legs_box.first()) {
                (Some(m), Some(lb)) => Some(([m.0[0].min(lb.0[0]), d - 0.001, lb.0[2]], [m.1[0].max(lb.1[0]), m.1[1].max(lb.1[1]), m.1[2]])),
                (Some(m), None) => Some(m),
                _ => None,
            };
            parts.push(Part { mesh: mantel, mat: mantel_mat, col });
        }
        let hearth_col = cols.first().copied();
        parts.push(Part { mesh: hearth, mat: border_mat, col: hearth_col });
        if !hearth_tiles.is_empty() {
            let shapes = ["square", "hexagon", "staggered", "crossed"];
            let sh = shapes[r.choice_p(&[0.35, 0.25, 0.2, 0.2])];
            parts.push(Part { mesh: hearth_tiles, mat: tile("fp_hearth_tiles", ms + 7, sh, [0.55, 0.3, 0.2]), col: None });
        }
        if !panel_m.is_empty() {
            let pm = panel.clone().unwrap_or_else(|| tile("fp_slips", ms + 5, "rectangle", [0.25, 0.4, 0.42]));
            parts.push(Part { mesh: panel_m, mat: pm, col: None });
        }
        if !ironm.is_empty() { parts.push(Part { mesh: ironm, mat: iron(ms + 8), col: None }); }
        if !logs.is_empty() { parts.push(Part { mesh: logs, mat: plain("fp_logs_charred", [0.07, 0.045, 0.03], 0.9), col: None }); } // bark / char (a procedural wood can come out pale)
        if !grate.is_empty() { parts.push(Part { mesh: grate, mat: iron(ms + 9), col: None }); }
        if !screen.is_empty() {
            parts.push(Part { mesh: screen, mat: iron(ms + 10), col: None });
            parts.push(Part { mesh: mesh_panel, mat: mat("fp_screen_mesh".into(), "metal.MetalBasic", json!({"color_hsv": [0.0, 0.0, 0.15]}), ms + 14, [0.16, 0.16, 0.16], 0.6), col: None });
        }
        if !niche.is_empty() {
            parts.push(Part { mesh: niche, mat: plain("fp_niche", [0.06, 0.055, 0.05], 0.95), col: None });
            parts.push(Part { mesh: niche_logs, mat: mat("fp_store_logs".into(), "wood.Wood", json!({}), ms + 11, [0.45, 0.3, 0.17], 0.8), col: None });
        }
        if !frame.is_empty() {
            parts.push(Part { mesh: frame, mat: if r.bool(0.5) { wood_mat("fp_frame", ms + 12) } else { painted("fp_frame_gilt".into(), "wood.Wood", ms + 12, [0.62, 0.48, 0.22], 0.8, 0.35) }, col: None });
            let inner = if l.over_mantel == OverMantel::Mirror {
                spec_material("fp_mirror", json!({"cls": "metal.Mirror", "kwargs": {}, "seed": ms + 13}), [0.8, 0.85, 0.9])
            } else {
                spec_material("fp_art", json!({"cls": "art.Art", "kwargs": {}, "seed": ms + 13}), [0.6, 0.5, 0.4])
            };
            parts.push(Part { mesh: glass, mat: inner, col: None });
        }
        for p in parts.iter_mut() { pad_uv(&mut p.mesh); }
        if let Some((lo, hi)) = mantel_box { supports.insert(0, ([lo[0], d, lo[2]], hi)); }
        let asset = Asset { cloth: vec![], followers: vec![], parts, supports, cutters: vec![], cols: vec![] };
        let (fx_0, fx_1) = (l.hearth[0].min(l.bench[0]).min(bx0), l.hearth[1].max(l.bench[1]).max(bx1));
        let fy1 = if l.hearth_kind == HearthKind::Bench { d + l.hearth_depth + 0.03 } else { hy1 };
        let clear = 0.35; // keep-out in front of the hearth
        let footprint = vec![[fx_0, 0.0], [fx_1, 0.0], [fx_1, fy1], [fx_0, fy1]];
        let keepout = vec![([hx0 - 0.10, fy1, 0.0], [hx1 + 0.10, fy1 + clear, 1.2])];
        FeatureAsset { asset, footprint, keepout, class: "FireplaceFeature" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opening;
    fn span(len: f64, rt: &str, op: Vec<Opening>) -> WallSpan {
        WallSpan { length: len, height: 2.7, wall_thickness: 0.2, openings: op, room_type: rt.into() }
    }
    #[test]
    fn fits_and_builds() {
        let f = FireplaceFeature;
        assert!(f.fits(&span(1.0, "LivingRoom", vec![])).is_empty());
        assert!(f.fits(&span(4.0, "Kitchen", vec![])).is_empty());
        let s = span(5.0, "LivingRoom", vec![Opening { is_window: false, x: [0.2, 1.1], z: [0.0, 2.1] }]);
        let iv = f.fits(&s);
        assert_eq!(iv.len(), 1);
        assert!(iv[0][0] >= 1.5);
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..200 {
            let a = f.build(&s, iv[0], seed);
            let (lo, hi) = a.asset.bounds();
            assert!(lo[1] >= -1e-9 && hi[1] <= 0.5 + 0.6 + 0.2, "{lo:?} {hi:?}");
            assert!(lo[0] >= iv[0][0] - 0.2 && hi[0] <= iv[0][1] + 0.2, "x {lo:?} {hi:?} {:?}", iv[0]);
            assert!(hi[2] <= 2.7 + 1e-6, "z {hi:?}");
            let l = FireplaceFeature::layout(&s, iv[0], seed);
            assert!(l.breast[1] - l.breast[0] >= 1.2 - 1e-9);
            assert!((0.3..=0.5).contains(&l.depth));
            assert!(l.firebox_z[1] < l.split_z && l.split_z <= l.top_z + 1e-9);
            assert!(a.asset.parts.iter().all(|p| p.mesh.uv.len() == p.mesh.pos.len()));
            assert!(!a.asset.solver_geom().tris.is_empty());
            seen.insert(format!("{:?}{:?}{:?}{:?}{:?}", l.style, l.shape, l.firebox_shape, l.mantel_kind, l.hearth_kind));
        }
        // arch heads never dip below the springing line
        for (rise, half) in [(0.4, 0.4), (0.2, 0.4), (0.39999999999, 0.4)] {
            let c = head_curve(1.0 - half, 1.0 + half, 0.5, rise, 16);
            assert!(c.iter().all(|p| p[1] >= 0.5 - 1e-9), "rise {rise}: {c:?}");
            assert!((c[8][1] - (0.5 + rise)).abs() < 1e-6);
        }
        // every axis takes several values across seeds
        assert!(seen.len() > 60, "only {} combinations", seen.len());
    }
}
