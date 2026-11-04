//! More built-ins on the same terms as the bookshelf wall and window seat: a wet bar and a mudroom bench. Span frame: x along the wall, y into the room, z up.
use crate::bookshelf::{fronts, Out, Style};
use crate::util::*;
use crate::{Feature, FeatureAsset, WallSpan};
use ig_core::materials::spec_material;
use ig_core::mesh::{Mat4, Material, Mesh};
use ig_core::rng::Rng;
use ig_kit::api::Asset;
use serde_json::json;

/// stretches of the wall with no opening at or below `below` m, at least `min` long
fn clear(span: &WallSpan, below: f64, min: f64) -> Vec<[f64; 2]> {
    let mut cuts: Vec<[f64; 2]> = span.openings.iter().filter(|o| o.z[0] < below).map(|o| [o.x[0] - 0.12, o.x[1] + 0.12]).collect();
    cuts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let (mut out, mut x) = (vec![], 0.03f64);
    for c in cuts { if c[0] - x >= min { out.push([x, c[0]]); } x = x.max(c[1]); }
    if span.length - 0.03 - x >= min { out.push([x, span.length - 0.03]); }
    out
}
fn solid(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material { spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}), rgb) }

/// a surface of revolution about z through (x, y) from (radius, height) pairs, closed at the top and bottom
fn lathe(m: &mut Mesh, x: f64, y: f64, z: f64, prof: &[(f64, f64)]) {
    let n = 12;
    let pt = |r: f64, h: f64, i: usize| { let a = i as f64 / n as f64 * std::f64::consts::TAU; [x + r * a.cos(), y + r * a.sin(), z + h] };
    for w in prof.windows(2) { for i in 0..n { m.add_quad(pt(w[0].0, w[0].1, i), pt(w[0].0, w[0].1, i + 1), pt(w[1].0, w[1].1, i + 1), pt(w[1].0, w[1].1, i)); } }
    let (a, b) = (prof[0], *prof.last().unwrap());
    for i in 0..n {
        m.add_tri([x, y, z + a.1], pt(a.0, a.1, i + 1), pt(a.0, a.1, i));
        m.add_tri([x, y, z + b.1], pt(b.0, b.1, i), pt(b.0, b.1, i + 1));
    }
}
fn bottle(m: &mut Mesh, x: f64, y: f64, z: f64, r: f64, h: f64) {
    lathe(m, x, y, z, &[(r, 0.0), (r, h * 0.62), (r * 0.82, h * 0.68), (r * 0.36, h * 0.76), (r * 0.3, h * 0.98), (r * 0.36, h * 0.98), (r * 0.36, h)]);
}
fn tumbler(m: &mut Mesh, x: f64, y: f64, z: f64, r: f64, h: f64) { lathe(m, x, y, z, &[(r * 0.85, 0.0), (r, h)]); }

pub struct WetBarFeature;
impl Feature for WetBarFeature {
    fn class(&self) -> &'static str { "WetBarFeature" }
    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]> { if span.height < 2.2 { vec![] } else { clear(span, 2.0, 1.2) } }
    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset {
        let mut r = Rng::seeded(seed ^ 0xba12_0001);
        let w = r.uniform(1.2, 2.4).min(x[1] - x[0]);
        let a = x[0] + r.f64() * (x[1] - x[0] - w);
        let b = a + w;
        let dp = r.uniform(0.42, 0.56);
        let mut st = Style::random(&mut r, dp);
        st.has_lower = true;
        let (ch, ct, co) = (r.uniform(0.88, 0.93), r.uniform(0.025, 0.04), r.uniform(0.015, 0.035));
        let (kick, kick_r) = (r.uniform(0.07, 0.11), r.uniform(0.02, 0.04));
        let case_m = case_material(&mut r, seed);
        let door_m = if r.bool(0.7) { case_m.clone() } else { case_material(&mut r, seed + 1) };
        let hw_m = hardware(&mut r);
        let mut o = Out::default();
        // base: plinth, carcass, the fronts; one bay may be a wine fridge
        bx(&mut o.case, a, b, 0.0, dp - kick_r, 0.0, kick);
        let zb = ch - ct;
        bx(&mut o.case, a, b, 0.0, dp, kick, zb);
        let fridge = w > 1.3 && r.bool(0.4);
        let fw = 0.46;
        let fa = if r.bool(0.5) { a } else { b - fw };
        let (mut glass, mut steel) = (Mesh::new(), Mesh::new());
        if fridge {
            let (lo, hi) = (fa, fa + fw);
            bx(&mut steel, lo, hi, dp, dp + 0.012, kick, zb);
            bx(&mut glass, lo + 0.03, hi - 0.03, dp + 0.012, dp + 0.016, kick + 0.04, zb - 0.05);
            let (ra, rb) = if fa == a { (a + fw, b) } else { (a, b - fw) };
            fronts(&st, &mut o, &mut r, ra, rb, kick, zb, true);
        } else {
            let n = ((w / 0.75).round() as usize).max(1);
            let cw = w / n as f64;
            for i in 0..n { let (p, q) = (a + cw * i as f64, a + cw * (i + 1) as f64); fronts(&st, &mut o, &mut r, p, q, kick, zb, true); }
        }
        // the counter, with a small sink in it (or none: a dry bar)
        let sink = r.bool(0.7);
        let (sw, sd) = (r.uniform(0.3, 0.38), r.uniform(0.26, 0.32));
        let sx = (a + b) / 2.0 + r.uniform(-0.15, 0.15) * (w - sw - 0.4).max(0.0);
        let (sa, sb, ya, yb) = (sx - sw / 2.0, sx + sw / 2.0, (dp - sd) / 2.0 + 0.03, (dp + sd) / 2.0 + 0.03);
        let mut top = Mesh::new();
        let cf = dp + co;
        if sink {
            bx(&mut top, a - 0.0, sa, 0.0, cf, zb, ch);
            bx(&mut top, sb, b, 0.0, cf, zb, ch);
            bx(&mut top, sa, sb, 0.0, ya, zb, ch);
            bx(&mut top, sa, sb, yb, cf, zb, ch);
            let (bot, t) = (ch - 0.14, 0.003);
            bx(&mut steel, sa, sb, ya, yb, bot - t, bot);
            bx(&mut steel, sa, sa + t, ya, yb, bot, ch);
            bx(&mut steel, sb - t, sb, ya, yb, bot, ch);
            bx(&mut steel, sa, sb, ya, ya + t, bot, ch);
            bx(&mut steel, sa, sb, yb - t, yb, bot, ch);
            // a gooseneck behind it
            let ty = ya - 0.035;
            let rr = 0.009;
            let mut col = Mesh::cylinder(0.0, 0.0, 0.0, 0.26, rr, 12);
            col.transform(&Mat4::translation([sx, ty, ch]));
            steel.append(&col);
            peg(&mut steel, sx, ty, ch + 0.26, 0.15, rr);
            let mut sp = Mesh::cylinder(0.0, 0.0, 0.0, 0.05, rr, 12);
            sp.transform(&Mat4::translation([sx, ty + 0.15, ch + 0.21]));
            steel.append(&sp);
        } else {
            bx(&mut top, a, b, 0.0, cf, zb, ch);
        }
        o.supports.push(([a + 0.05, 0.1, ch], [if sink { sa - 0.05 } else { b - 0.05 }, dp - 0.05, ch + 0.001]));
        // behind it: tile, mirror or boards
        let back_top = r.uniform(1.55, 2.05).min(span.height - 0.25);
        let backing = r.choice_idx(3);
        let mut back = Mesh::new();
        bx(&mut back, a, b, 0.0, 0.012, ch, back_top);
        let back_m = match backing {
            0 => solid("wall_alt", "ceramic.Tile", seed + 5, [0.8, 0.78, 0.72]),
            1 => solid("bar_mirror", "metal.Mirror", seed + 5, [0.9, 0.9, 0.9]),
            _ => case_m.clone(),
        };
        // above: open shelves on the backing, or a wine rack, or closed cabinets at the ends
        let mut bottle_sets = [Mesh::new(), Mesh::new(), Mesh::new()];
        let mut glasses = Mesh::new();
        let mut shelves = Mesh::new();
        let sd2 = r.uniform(0.2, 0.3);
        let upper = r.choice_idx(3);
        let n_sh = if upper == 1 { 1 } else { 2 };
        for k in 0..n_sh {
            let z = ch + 0.42 + k as f64 * r.uniform(0.34, 0.42);
            if z > back_top - 0.02 { continue }
            bx(&mut shelves, a + 0.02, b - 0.02, 0.012, sd2, z - 0.03, z);
            o.supports.push(([a + 0.06, 0.03, z], [b - 0.06, sd2 - 0.02, z + 0.001]));
            let m = ((w - 0.2) / 0.11) as usize;
            for i in 0..m {
                if r.bool(0.35) { continue }
                let px = a + 0.1 + i as f64 * 0.11;
                if k % 2 == 0 && r.bool(0.8) { let (rb, h) = (r.uniform(0.032, 0.04), r.uniform(0.24, 0.31)); let k = r.choice_idx(3); bottle(&mut bottle_sets[k], px, sd2 / 2.0 + 0.01, z, rb, h); }
                else { let rg = r.uniform(0.03, 0.04); tumbler(&mut glasses, px, sd2 / 2.0 + 0.01, z, rg, r.uniform(0.08, 0.11)); }
            }
        }
        if upper == 1 {
            // a wine rack at the wall: a lattice with bottle ends in it
            let (rw, rh) = (r.uniform(0.5, 0.8).min(w - 0.1), 0.42);
            let (ra, rz) = (a + (w - rw) * r.f64(), ch + 0.4 + 0.03);
            let cells = (rw / 0.11) as usize;
            for i in 0..=cells { let px = ra + rw * i as f64 / cells as f64; bx(&mut shelves, px - 0.006, px + 0.006, 0.012, 0.25, rz, rz + rh); }
            for j in 0..=3 { let pz = rz + rh * j as f64 / 3.0; bx(&mut shelves, ra, ra + rw, 0.012, 0.25, pz - 0.006, pz + 0.006); }
            for i in 0..cells { for j in 0..3 {
                if r.bool(0.3) { continue }
                let (px, pz) = (ra + rw * (i as f64 + 0.5) / cells as f64, rz + rh * (j as f64 + 0.5) / 3.0);
                peg(&mut bottle_sets[2], px, 0.03, pz, 0.21, 0.036);
            } }
        }
        // something out on the counter
        for _ in 0..r.randint(2, 5) {
            let (px, py) = (a + 0.1 + r.f64() * (w - 0.2), r.uniform(0.12, dp - 0.1));
            if sink && px > sa - 0.08 && px < sb + 0.08 { continue }
            if r.bool(0.5) { let k = r.choice_idx(3); bottle(&mut bottle_sets[k], px, py, ch, r.uniform(0.032, 0.04), r.uniform(0.24, 0.31)); } else { tumbler(&mut glasses, px, py, ch, 0.036, 0.1); }
        }
        let mut asset = Asset::default();
        asset.mesh(o.case, &case_m, Some(([a, 0.0, 0.0], [b, cf, ch])));
        asset.mesh(o.door, &door_m, None);
        if !o.counter.is_empty() { asset.mesh(o.counter, &case_m, None); }
        if !o.hw.is_empty() { asset.mesh(o.hw, &hw_m, None); }
        asset.mesh(top, &solid("bar_top", ["ceramic.Marble", "wood.Wood", "ceramic.Concrete"][r.choice_idx(3)], seed + 2, [0.82, 0.8, 0.76]), None);
        // (the backing and what is on it fill the wall to its top: art hung on the wall went behind the rack)
        asset.mesh(back, &back_m, Some(([a, 0.0, ch], [b, 0.012, back_top])));
        if !shelves.is_empty() { asset.mesh(shelves, &case_m, Some(([a, 0.0, ch + 0.3], [b, sd2, back_top]))); }
        if !steel.is_empty() { asset.mesh(steel, &solid("sink_metal", "metal.MetalBasic", seed + 3, [0.7, 0.7, 0.72]), None); }
        if !glass.is_empty() { asset.mesh(glass, &solid("bar_fridge", "metal.BrushedBlackMetal", seed + 4, [0.03, 0.03, 0.03]), None); }
        for (k, (m, tint)) in bottle_sets.into_iter().zip([[0.03, 0.12, 0.05], [0.2, 0.08, 0.02], [0.02, 0.02, 0.03]]).enumerate() {
            if !m.is_empty() { asset.mesh(m, &solid("bottle", "ceramic.VaseCeramic", seed + 6 + k as u64, tint), None); }
        }
        if !glasses.is_empty() { asset.mesh(glasses, &solid("glass", "ceramic.Glass", seed + 10, [0.9, 0.9, 0.9]), None); }
        asset.supports = o.supports;
        let fp = vec![[a, 0.0], [b, 0.0], [b, cf], [a, cf]];
        let keep = vec![([a, cf, 0.0], [b, cf + 0.6, 2.0])];
        FeatureAsset { asset, footprint: fp, keepout: keep, class: "WetBarFeature" }
    }
}

pub struct MudroomFeature;
impl Feature for MudroomFeature {
    fn class(&self) -> &'static str { "MudroomFeature" }
    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]> { if span.height < 2.1 { vec![] } else { clear(span, 2.0, 0.9) } }
    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset {
        let mut r = Rng::seeded(seed ^ 0x3ad0_0001);
        let w = r.uniform(0.9, 2.0).min(x[1] - x[0]);
        let a = x[0] + r.f64() * (x[1] - x[0] - w);
        let b = a + w;
        let dp = r.uniform(0.34, 0.42);
        let hs = r.uniform(0.43, 0.48);
        let tb = 0.03;
        let top_z = r.uniform(1.75, 1.95).min(span.height - 0.2);
        let (sd, hook_z) = (r.uniform(0.22, 0.3), top_z - r.uniform(0.2, 0.3));
        let case_m = case_material(&mut r, seed);
        let hw_m = hardware(&mut r);
        let mut case = Mesh::new();
        // the bench, open cubbies under it
        bx(&mut case, a, b, 0.0, dp, hs - tb, hs);
        bx(&mut case, a, b, 0.0, 0.015, 0.0, top_z);
        let n = ((w / 0.42).round() as usize).max(2);
        for i in 0..=n { let px = a + (w - 0.022) * i as f64 / n as f64; bx(&mut case, px, px + 0.022, 0.0, dp, 0.0, hs - tb); }
        bx(&mut case, a, b, 0.0, dp, 0.0, 0.03);
        // the ends rise to the shelf, or not
        if r.bool(0.5) { bx(&mut case, a, a + 0.022, 0.0, dp, hs, top_z); bx(&mut case, b - 0.022, b, 0.0, dp.min(sd), hs, top_z); }
        // the shelf over the hooks
        bx(&mut case, a, b, 0.0, sd, top_z - 0.03, top_z);
        let mut supports = vec![([a + 0.05, 0.03, top_z], [b - 0.05, sd - 0.02, top_z + 0.001])];
        supports.push(([a + 0.05, 0.05, hs], [b - 0.05, dp - 0.03, hs + 0.001]));
        // hooks, with coats and bags on some
        let mut hooks = Mesh::new();
        let (mut coats, mut boots, mut cushion) = (Mesh::new(), Mesh::new(), Mesh::new());
        let m = ((w / 0.28) as usize).max(2);
        for i in 0..m {
            let px = a + (w - 0.12) * (i as f64 + 0.5) / m as f64 + 0.06;
            peg(&mut hooks, px, 0.015, hook_z, 0.07, 0.008);
            if r.bool(0.4) {
                let (hw, hh) = (r.uniform(0.17, 0.24), r.uniform(0.3, 0.42).min((hook_z - hs - 0.06) / 2.0));
                let mut c = rbox([hw, r.uniform(0.04, 0.07), hh], 0.03, 0.0);
                // (hanging from a hook: gathered at the top, folds down the length)
                let (ph, fold) = (r.uniform(0.0, 6.0), r.uniform(0.008, 0.018));
                for p in c.pos.iter_mut() {
                    let t = (p[2] + hh) / (2.0 * hh);
                    p[0] *= 0.35 + 0.65 * (1.0 - t * t);
                    p[1] += fold * (p[0] * 38.0 + ph).sin() * (1.0 - t) + 0.02 * t * t;
                }
                c.transform(&Mat4::translation([px, 0.075, hook_z - hh - 0.01]));
                coats.append(&c);
            }
        }
        // boots in the cubbies
        for i in 0..n {
            if !r.bool(0.5) { continue }
            let cw = (w - 0.022) / n as f64;
            let cx = a + cw * (i as f64 + 0.5);
            for s in [-1.0, 1.0] {
                let hz = r.uniform(0.05, 0.1);
                let mut bo = rbox([0.045, r.uniform(0.11, 0.14), hz], 0.03, 0.0);
                // (standing on the cubby's floor)
                bo.transform(&Mat4::translation([cx + s * 0.06, dp * 0.55, 0.03 + hz]));
                boots.append(&bo);
            }
        }
        if r.bool(0.5) { let mut c = rbox([(w - 0.08) / 2.0, (dp - 0.04) / 2.0, 0.03], 0.025, 0.012); c.transform(&Mat4::translation([(a + b) / 2.0, dp / 2.0, hs + 0.03])); cushion.append(&c); }
        let mut asset = Asset::default();
        asset.mesh(case, &case_m, Some(([a, 0.0, 0.0], [b, dp, hs])));
        asset.mesh(hooks, &hw_m, None);
        if !coats.is_empty() { asset.mesh(coats, &fabric(&mut r, seed + 2, "garment"), None); }
        if !boots.is_empty() { asset.mesh(boots, &solid("slipper", "fabric.Leather", seed + 3, [0.18, 0.11, 0.07]), None); }
        if !cushion.is_empty() { asset.mesh(cushion, &fabric(&mut r, seed + 4, "seat_cushion"), None); }
        asset.supports = supports;
        let fp = vec![[a, 0.0], [b, 0.0], [b, dp], [a, dp]];
        FeatureAsset { asset, footprint: fp, keepout: vec![([a, dp, 0.0], [b, dp + 0.4, 1.0])], class: "MudroomFeature" }
    }
}
