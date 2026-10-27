//! Masonry fireplace with a chimney breast, built flush against a wall span (span frame: x along the wall, y into the room, z up from the floor).
//!
//! Geometry (all axis aligned boxes + quads, no booleans; every face has its own vertices, flat normals, metre UVs):
//!   * chimney breast: floor to ceiling, 1.2-2.0 m wide, 0.30-0.50 m deep, as left cheek / right cheek / lintel around the firebox opening
//!   * firebox: 0.6-0.9 m wide, 0.55-0.8 m high opening with a sooty back wall, side walls and roof (soot material), ash bed, logs (+ grate)
//!   * surround: brick | stone | painted plaster breast; optional stone lintel (brick), framed trim (plaster)
//!   * mantel shelf (wood or stone, 1.1-1.4 m high, overhanging the breast on both sides and the front)
//!   * hearth slab (flush or raised), 0.4-0.5 m into the room
//! Collision: breast, mantel and hearth are separate parts with their own boxes. Support surfaces: mantel top, hearth top.
//! The breast extents are returned by [`FireplaceFeature::layout`] so a bookshelf can fill the alcoves on either side.
use crate::boxes::*;
use crate::{Feature, FeatureAsset, WallSpan};
use ig_core::materials::spec_material;
use ig_core::mesh::{Material, Mesh, V3};
use ig_core::rng::Rng;
use ig_kit::api::{Asset, Part};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Brick,
    Stone,
    Plaster,
}

/// Resolved proportions of one fireplace (span frame, metres).
#[derive(Clone, Debug)]
pub struct FireplaceLayout {
    pub style: Style,
    /// breast extent along the wall [x0, x1]
    pub breast: [f64; 2],
    pub depth: f64,
    pub height: f64,
    /// firebox opening [x0, x1] and [z0, z1]
    pub firebox: [f64; 2],
    pub firebox_z: [f64; 2],
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
    /// the proportions `build` will use for (span, interval, seed)
    pub fn layout(span: &WallSpan, x: [f64; 2], seed: u64) -> FireplaceLayout {
        let mut r = Rng::seeded(seed ^ 0xF1AE_91ACE);
        let free = x[1] - x[0];
        let w = r.uniform(1.2, 2.0).min(free - 0.05).max(1.2).min(free);
        let depth = r.uniform(0.30, 0.50);
        let slack = (free - w).max(0.0);
        let cx = x[0] + free / 2.0 + r.uniform(-0.15, 0.15) * slack;
        let cx = cx.clamp(x[0] + w / 2.0, x[1] - w / 2.0);
        let breast = [cx - w / 2.0, cx + w / 2.0];
        let fw = r.uniform(0.60, 0.90).min(w - 0.40);
        let fh = r.uniform(0.55, 0.80);
        let raised = r.bool(0.45);
        let hearth_h = if raised { r.uniform(0.08, 0.28) } else { 0.02 };
        let style = [Style::Brick, Style::Stone, Style::Plaster][r.choice_p(&[0.4, 0.3, 0.3])];
        let hearth_depth = r.uniform(0.40, 0.50);
        let hw = w / 2.0 + r.uniform(0.0, 0.12);
        let mantel_t = r.uniform(0.04, 0.07);
        let mantel_z = r.uniform(1.10, 1.40).max(hearth_h + fh + 0.22).min(span.height - 0.6);
        let mantel_overhang = r.uniform(0.04, 0.12);
        let mantel_depth = r.uniform(0.10, 0.18);
        let logs = r.bool(0.7);
        let grate = r.bool(0.4);
        FireplaceLayout {
            style,
            breast,
            depth,
            height: span.height,
            firebox: [cx - fw / 2.0, cx + fw / 2.0],
            firebox_z: [hearth_h, hearth_h + fh],
            hearth: [cx - hw, cx + hw],
            hearth_depth,
            hearth_h,
            mantel_z,
            mantel_t,
            mantel_overhang,
            mantel_depth,
            alcoves: [[x[0], breast[0]], [breast[1], x[1]]],
            logs,
            grate,
        }
    }
}

fn mat(name: String, cls: &str, kwargs: serde_json::Value, seed: u64, rgb: [f32; 3], rough: f32) -> Material {
    let mut m = spec_material(&name, json!({"cls": cls, "kwargs": kwargs, "seed": seed}), rgb);
    m.roughness = rough;
    m
}

fn plain(name: &str, rgb: [f32; 3], rough: f32) -> Material {
    Material::diffuse(name, rgb, rough)
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
        let (fx0, fx1) = (l.firebox[0], l.firebox[1]);
        let (fz0, fz1) = (l.firebox_z[0], l.firebox_z[1]);
        let (d, h) = (l.depth, l.height);
        let cd = d - 0.03; // firebox depth (it cuts into the breast, the wall stays behind)
        let hz = l.hearth_h;
        let y_back = d - cd;

        // ---- materials ----
        let (surround, trim, hearth_mat, mantel_mat): (Material, Option<Material>, Material, Material) = match l.style {
            Style::Brick => (
                mat("fp_brick".into(), "ceramic.Brick", json!({}), ms, [0.42, 0.16, 0.11], 0.9),
                if r.bool(0.6) { Some(mat("fp_stone".into(), "ceramic.Concrete", json!({}), ms + 1, [0.55, 0.53, 0.5], 0.8)) } else { None },
                mat("fp_hearth".into(), "ceramic.Concrete", json!({}), ms + 2, [0.25, 0.25, 0.26], 0.8),
                if r.bool(0.6) { mat("fp_mantel_wood".into(), "wood.Wood", json!({}), ms + 3, [0.35, 0.2, 0.1], 0.5) } else { mat("fp_mantel_stone".into(), "ceramic.Concrete", json!({}), ms + 3, [0.55, 0.53, 0.5], 0.8) },
            ),
            Style::Stone => {
                let polished = r.bool(0.5);
                let (cls, rgb) = if polished { ("ceramic.MarbleRegular", [0.75, 0.74, 0.72]) } else { ("ceramic.Concrete", [0.58, 0.56, 0.52]) };
                (
                    mat("fp_stone_breast".into(), cls, json!({}), ms, rgb, if polished { 0.35 } else { 0.85 }),
                    None,
                    mat("fp_hearth".into(), cls, json!({}), ms + 2, rgb, if polished { 0.35 } else { 0.85 }),
                    mat("fp_mantel_stone".into(), cls, json!({}), ms + 3, rgb, if polished { 0.35 } else { 0.85 }),
                )
            }
            Style::Plaster => {
                let wood = r.bool(0.6);
                (
                    mat("fp_plaster".into(), "ceramic.Plaster", json!({}), ms, [0.88, 0.86, 0.82], 0.9),
                    Some(if wood { mat("fp_trim_wood".into(), "wood.Wood", json!({}), ms + 1, [0.4, 0.25, 0.13], 0.5) } else { mat("fp_trim_marble".into(), "ceramic.MarbleRegular", json!({}), ms + 1, [0.8, 0.8, 0.78], 0.35) }),
                    mat("fp_hearth".into(), "ceramic.MarbleRegular", json!({}), ms + 2, [0.7, 0.7, 0.68], 0.35),
                    if wood { mat("fp_mantel_wood".into(), "wood.Wood", json!({}), ms + 3, [0.4, 0.25, 0.13], 0.5) } else { mat("fp_mantel_stone".into(), "ceramic.MarbleRegular", json!({}), ms + 3, [0.8, 0.8, 0.78], 0.35) },
                )
            }
        };
        let soot = plain("fp_soot", [0.025, 0.022, 0.02], 0.95);

        // ---- breast ----
        let mut breast = Mesh::new();
        // left / right cheeks: outer side + front (their inner sides and the top are hidden or belong to the firebox)
        add_box(&mut breast, [bx0, 0.0, 0.0], [fx0, d, h], NX | PY);
        add_box(&mut breast, [fx1, 0.0, 0.0], [bx1, d, h], PX | PY);
        // lintel over the opening
        add_box(&mut breast, [fx0, 0.0, fz1], [fx1, d, h], PY);
        // (below the opening the hearth slab covers the front)

        // ---- firebox (soot) ----
        let mut fire = Mesh::new();
        add_quad_uv(&mut fire, [[fx0, y_back, fz0], [fx1, y_back, fz0], [fx1, y_back, fz1], [fx0, y_back, fz1]], 0, 2); // back wall (faces +y)
        add_quad_uv(&mut fire, [[fx0, y_back, fz0], [fx0, d, fz0], [fx0, d, fz1], [fx0, y_back, fz1]], 1, 2); // left side (faces +x)
        add_quad_uv(&mut fire, [[fx1, y_back, fz0], [fx1, y_back, fz1], [fx1, d, fz1], [fx1, d, fz0]], 1, 2); // right side (faces -x)
        add_quad_uv(&mut fire, [[fx0, y_back, fz1], [fx0, d, fz1], [fx1, d, fz1], [fx1, y_back, fz1]], 0, 1); // roof (faces -z)
        // ash bed, slightly proud of the hearth
        add_box(&mut fire, [fx0 + 0.02, y_back + 0.01, fz0], [fx1 - 0.02, d - 0.05, fz0 + 0.03], PX | NX | PY | NY | PZ);
        // sooty smear on the front face above the opening is part of the material; nothing to model

        // ---- trim / surround ----
        let mut trim_m = Mesh::new();
        match (l.style, &trim) {
            (Style::Brick, Some(_)) => {
                // stone lintel slab + two jamb blocks flush with the cheeks' front
                let t = 0.02;
                add_box(&mut trim_m, [fx0 - 0.10, d, fz1], [fx1 + 0.10, d + t, fz1 + 0.12], NX | PX | PY | PZ | NZ);
            }
            (Style::Plaster, Some(_)) => {
                let (t, wd) = (0.025, 0.11);
                add_box(&mut trim_m, [fx0 - wd, d, fz0], [fx0, d + t, fz1 + wd], NX | PX | PY | PZ);
                add_box(&mut trim_m, [fx1, d, fz0], [fx1 + wd, d + t, fz1 + wd], NX | PX | PY | PZ);
                add_box(&mut trim_m, [fx0, d, fz1], [fx1, d + t, fz1 + wd], PY | NZ);
            }
            _ => {}
        }

        // ---- mantel ----
        let (mx0, mx1) = (bx0 - l.mantel_overhang, bx1 + l.mantel_overhang);
        let (mz0, mz1) = (l.mantel_z, l.mantel_z + l.mantel_t);
        let my1 = d + l.mantel_depth;
        let mantel_box = ([mx0, d - 0.001, mz0], [mx1, my1, mz1]);
        let mut mantel = Mesh::new();
        add_box(&mut mantel, mantel_box.0, mantel_box.1, ALL & !NY);
        // ---- hearth ----
        let (hx0, hx1) = (l.hearth[0], l.hearth[1]);
        let hy1 = d + l.hearth_depth;
        let mut hearth = Mesh::new();
        add_box(&mut hearth, [hx0, 0.0, 0.0], [hx1, hy1, hz], ALL & !NY & !NZ);

        // ---- logs and grate ----
        let mut logs = Mesh::new();
        let mut iron = Mesh::new();
        if l.logs {
            let fw = fx1 - fx0;
            let yc = y_back + 0.03 + (cd - 0.09) * 0.5;
            let rad = r.uniform(0.04, 0.06).min((cd - 0.09) / 4.0 - 0.003);
            let len = (fw - 0.12).min(r.uniform(0.40, 0.55));
            let z0 = fz0 + 0.03 + if l.grate { 0.06 } else { 0.0 };
            let xc = (fx0 + fx1) / 2.0 + r.uniform(-0.04, 0.04);
            for (i, dy) in [-1.0f64, 1.0].iter().enumerate() {
                let yy = yc + dy * (rad + 0.003);
                add_cylinder(&mut logs, [xc - len / 2.0 + i as f64 * 0.02, yy, z0 + rad], [xc + len / 2.0 - i as f64 * 0.02, yy, z0 + rad], rad, 12);
            }
            add_cylinder(&mut logs, [xc - len / 2.0, yc, z0 + rad * 2.6], [xc + len / 2.0, yc, z0 + rad * 2.6], rad * 0.95, 12);
        }
        if l.grate {
            // low iron grate: two side rails and cross bars
            let (xa, xb) = (fx0 + 0.06, fx1 - 0.06);
            let (ya, yb) = (y_back + 0.04, d - 0.06);
            let zz = fz0 + 0.03 + 0.05;
            for yy in [ya, yb] { add_box(&mut iron, [xa, yy - 0.01, zz], [xb, yy + 0.01, zz + 0.02], ALL); }
            let n = 6;
            for i in 0..n {
                let xx = xa + (xb - xa) * i as f64 / (n - 1) as f64;
                add_box(&mut iron, [xx - 0.008, ya, zz], [xx + 0.008, yb, zz + 0.015], ALL);
            }
            for xx in [xa, xb] { for yy in [ya, yb] { add_box(&mut iron, [xx - 0.012, yy - 0.012, fz0 + 0.03], [xx + 0.012, yy + 0.012, zz], ALL); } }
        }

        // ---- assemble ----
        let breast_box: (V3, V3) = ([bx0, 0.0, 0.0], [bx1, d, h]);
        let mut parts: Vec<Part> = vec![
            Part { mesh: breast, mat: surround, col: Some(breast_box) },
            Part { mesh: fire, mat: soot, col: None },
            Part { mesh: mantel, mat: mantel_mat, col: Some(mantel_box) },
            Part { mesh: hearth, mat: hearth_mat, col: Some(([hx0, 0.0, 0.0], [hx1, hy1, hz])) },
        ];
        if let Some(t) = trim {
            if !trim_m.is_empty() { parts.push(Part { mesh: trim_m, mat: t, col: None }); }
        }
        if !logs.is_empty() {
            parts.push(Part { mesh: logs, mat: mat("fp_logs".into(), "wood.Wood", json!({}), ms + 4, [0.3, 0.18, 0.09], 0.8), col: None });
        }
        if !iron.is_empty() {
            parts.push(Part { mesh: iron, mat: plain("fp_iron", [0.03, 0.03, 0.03], 0.5), col: None });
        }
        let supports = vec![
            ([mx0, d, mz0], [mx1, my1, mz1]),
            ([hx0, d, 0.0], [hx1, hy1, hz]),
        ];
        let asset = Asset { cloth: vec![], followers: vec![], parts, supports, cutters: vec![] };
        let clear = 0.35; // keep-out in front of the hearth
        let footprint = vec![[hx0, 0.0], [hx1, 0.0], [hx1, hy1], [hx0, hy1]];
        let keepout = vec![([hx0 - 0.10, hy1, 0.0], [hx1 + 0.10, hy1 + clear, 1.2])];
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
        for seed in 0..40 {
            let a = f.build(&s, iv[0], seed);
            let (lo, hi) = a.asset.bounds();
            assert!(lo[1] >= -1e-9 && hi[1] <= 0.5 + 0.5 + 0.2, "{lo:?} {hi:?}");
            assert!(lo[0] >= iv[0][0] - 0.2 && hi[0] <= iv[0][1] + 0.01, "x {lo:?} {hi:?} {:?}", iv[0]);
            let l = FireplaceFeature::layout(&s, iv[0], seed);
            assert!((1.2..=2.0 + 1e-9).contains(&(l.breast[1] - l.breast[0])));
            assert!((0.3..=0.5).contains(&l.depth));
            assert!(a.asset.parts.iter().all(|p| p.mesh.uv.len() == p.mesh.pos.len()));
            let g = a.asset.solver_geom();
            assert!(!g.tris.is_empty());
        }
    }
}
