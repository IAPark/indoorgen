//! WindowSeatFeature: upholstered bench under a window, with drawer / door / open-cubby base, back cushions and throw pillows, optional
//! flanking tall cabinets up to the ceiling.
use crate::bookshelf::{crown, fronts, tall_unit, Out, Style};
use crate::util::*;
use crate::{Feature, FeatureAsset, Opening, WallSpan};
use ig_core::mesh::{Mat4, Mesh};
use ig_core::rng::Rng;
use ig_kit::api::Asset;

pub struct WindowSeatFeature;

fn seat_windows(span: &WallSpan) -> Vec<&Opening> {
    span.openings.iter().filter(|w| w.is_window && w.z[0] >= 0.55 && w.z[0] <= 1.1 && w.x[1] - w.x[0] >= 0.5).collect()
}

/// free x range around window `w`: bounded by the span and neighbouring openings
fn free_range(span: &WallSpan, w: &Opening) -> [f64; 2] {
    let (mut lo, mut hi) = (0.0f64, span.length);
    for o in &span.openings {
        if std::ptr::eq(o, w) {
            continue;
        }
        if o.x[1] <= w.x[0] {
            lo = lo.max(o.x[1] + 0.05);
        } else if o.x[0] >= w.x[1] {
            hi = hi.min(o.x[0] - 0.05);
        }
    }
    [lo, hi]
}

impl Feature for WindowSeatFeature {
    fn class(&self) -> &'static str {
        "WindowSeatFeature"
    }
    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]> {
        if span.height < 2.2 {
            return vec![];
        }
        let mut v = vec![];
        for w in seat_windows(span) {
            let r = free_range(span, w);
            let a = (w.x[0] - 0.9).max(r[0]);
            let b = (w.x[1] + 0.9).min(r[1]);
            if a <= w.x[0] + 1e-6 && b >= w.x[1] - 1e-6 {
                v.push([a, b]);
            }
        }
        v
    }
    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset {
        let mut rng = Rng::seeded(seed ^ 0x5ea7_0001);
        let win = seat_windows(span)
            .into_iter()
            .max_by(|a, b| {
                let ov = |w: &Opening| (w.x[1].min(x[1]) - w.x[0].max(x[0])).max(0.0);
                ov(a).partial_cmp(&ov(b)).unwrap()
            })
            .cloned()
            .unwrap_or(Opening { is_window: true, x: [(x[0] + x[1]) / 2.0 - 0.5, (x[0] + x[1]) / 2.0 + 0.5], z: [0.8, 1.8] });
        let sill = win.z[0];
        let dp = rng.uniform(0.4, 0.55);
        let c = rng.uniform(0.07, 0.13);
        let hs = rng.uniform(0.42, 0.48).min(sill - 0.1);
        let base_h = hs - c;
        let kick = rng.uniform(0.05, 0.09);
        let kick_r = rng.uniform(0.02, 0.04);
        let tb = 0.025;
        let ts = 0.03;

        // x layout: [x0 | flank | seat | flank | x1]
        let free_l = (win.x[0] - x[0]).max(0.0);
        let free_r = (x[1] - win.x[1]).max(0.0);
        let ext_l = rng.uniform(0.0, 0.3).min(free_l);
        let ext_r = rng.uniform(0.0, 0.3).min(free_r);
        let (sx0, sx1) = (win.x[0] - ext_l, win.x[1] + ext_r);
        let flank_on = rng.bool(0.6);
        let fl_w = |free: f64, ext: f64, rng: &mut Rng| -> f64 {
            let avail = free - ext;
            let w = rng.uniform(0.3, 0.55).min(avail);
            if flank_on && w >= 0.28 {
                w
            } else {
                0.0
            }
        };
        let wl = fl_w(free_l, ext_l, &mut rng);
        let wr = fl_w(free_r, ext_r, &mut rng);
        let (fx0, fx1) = (sx0 - wl, sx1 + wr);

        let mut o = Out::default();
        let case_m = case_material(&mut rng, seed);
        let door_m = if rng.bool(0.75) { case_m.clone() } else { case_material(&mut rng, seed + 1) };
        let hw_m = hardware(&mut rng);
        let fab = fabric(&mut rng, seed + 3, "seat_cushion");
        let accent = fabric(&mut rng, seed + 4, "seat_pillow");
        let mut st = Style::random(&mut rng, dp);
        st.drawer_row = false;
        st.has_lower = rng.bool(0.6);

        // ---- base ----
        let style = rng.choice_p(&[0.4, 0.3, 0.3]);
        let (bx0, bx1) = (sx0, sx1);
        // plinth (solid, recessed)
        bx(&mut o.case, bx0, bx1, 0.0, dp - kick_r, 0.0, kick);
        // top board + back + ends
        bx(&mut o.case, bx0, bx1, 0.0, dp, base_h - tb, base_h);
        bx(&mut o.case, bx0, bx1, 0.0, 0.012, kick, base_h - tb);
        bx(&mut o.case, bx0, bx0 + ts, 0.0, dp, kick, base_h - tb);
        bx(&mut o.case, bx1 - ts, bx1, 0.0, dp, kick, base_h - tb);
        let ix0 = bx0 + ts;
        let ix1 = bx1 - ts;
        let ob = base_h - tb;
        st.d = dp;
        st.ph = kick;
        st.tb = tb;
        match style {
            0 | 1 => {
                // closed: solid carcass behind fronts
                bx(&mut o.case, ix0, ix1, 0.0, dp - 0.004, kick, ob);
                if style == 0 {
                    // drawers: 1 or 2 rows
                    let rows = if ob - kick > 0.32 && rng.bool(0.4) { 2 } else { 1 };
                    let n = (((ix1 - ix0) / 0.55).round() as usize).max(1);
                    let cw = (ix1 - ix0) / n as f64;
                    let rh = (ob - kick) / rows as f64;
                    for r in 0..rows {
                        for i in 0..n {
                            crate::bookshelf::front(&st, &mut o, &mut rng, ix0 + cw * i as f64 + 0.003, ix0 + cw * (i + 1) as f64 - 0.003, kick + rh * r as f64 + 0.003, kick + rh * (r + 1) as f64 - 0.003, dp, true);
                        }
                    }
                } else {
                    fronts(&st, &mut o, &mut rng, ix0, ix1, kick, ob, false);
                }
            }
            _ => {
                // open cubbies with floor boards (clutter / baskets can sit on them)
                let w = ix1 - ix0;
                let n = ((w / rng.uniform(0.4, 0.6)).round() as usize).max(1);
                let cw = (w - ts * (n - 1) as f64) / n as f64;
                bx(&mut o.case, ix0, ix1, 0.0, dp, kick, kick + tb);
                for i in 0..n {
                    let a = ix0 + (cw + ts) * i as f64;
                    o.supports.push(([a, 0.012, kick + tb - 0.001], [a + cw, dp - 0.004, kick + tb]));
                    if i + 1 < n {
                        bx(&mut o.case, a + cw, a + cw + ts, 0.0, dp, kick + tb, ob);
                    }
                }
                // front rail under the cushion
                bx(&mut o.case, ix0, ix1, dp, dp + 0.01, ob - 0.04, ob);
            }
        }

        // ---- cushions ----
        let cush_t = c;
        let puff = rng.uniform(0.01, 0.025);
        let cr = (cush_t * 0.45).min(0.045);
        let two = (sx1 - sx0) > 1.4 && rng.bool(0.5);
        let ncs = if two { 2 } else { 1 };
        let cw = (sx1 - sx0 - 0.0) / ncs as f64;
        let mut cushions = Mesh::new();
        let mut supports = vec![];
        for i in 0..ncs {
            let xa = sx0 + cw * i as f64 + 0.004;
            let xb = sx0 + cw * (i + 1) as f64 - 0.004;
            let (ya, yb) = (0.01, dp + 0.012);
            let hz = (cush_t - puff) / 2.0;
            let mut m = rbox([(xb - xa) / 2.0, (yb - ya) / 2.0, hz], cr, puff);
            m.transform(&Mat4::translation([(xa + xb) / 2.0, (ya + yb) / 2.0, base_h + hz]));
            cushions.append(&m);
            supports.push(([xa + cr, ya + cr, hs - 0.012], [xb - cr, yb - cr, hs - 0.011]));
        }
        // back cushions
        let mut backs = Mesh::new();
        let room_above = (sill - 0.03) - hs;
        if room_above > 0.2 && rng.bool(0.8) {
            let nbk = (((sx1 - sx0) / rng.uniform(0.5, 0.7)).round() as usize).max(1);
            let bw = (sx1 - sx0 - 0.04) / nbk as f64;
            let tk = rng.uniform(0.1, 0.16);
            let hb = rng.uniform(0.32, 0.5).min(room_above / 0.97);
            let ang = rng.uniform(0.12, 0.25);
            for i in 0..nbk {
                let mut m = rbox([(bw - 0.01) / 2.0, tk / 2.0, hb / 2.0], (tk * 0.4).min(0.05), 0.02);
                m.transform(&Mat4::rot_x(ang));
                let (lo, _) = m.bounds();
                m.transform(&Mat4::translation([sx0 + 0.02 + bw * (i as f64 + 0.5), 0.004 - lo[1], hs - 0.012 - lo[2]]));
                backs.append(&m);
            }
        }
        // throw pillows
        let mut pillows = Mesh::new();
        let np = if room_above > 0.2 { rng.choice_idx(4) } else { 0 };
        for _ in 0..np {
            let s = rng.uniform(0.3, 0.45);
            let th = rng.uniform(0.09, 0.14);
            let mut m = rbox([s / 2.0, th / 2.0, s * rng.uniform(0.85, 1.0) / 2.0], th * 0.45, 0.03);
            m.transform(&Mat4::rot_y(rng.uniform(-0.6, 0.6)));
            m.transform(&Mat4::rot_x(rng.uniform(0.25, 0.5)));
            let (lo, hi) = m.bounds();
            let px = rng.uniform(sx0 + 0.05 + (hi[0] - lo[0]) / 2.0, (sx1 - 0.05 - (hi[0] - lo[0]) / 2.0).max(sx0 + 0.06 + (hi[0] - lo[0]) / 2.0));
            let py = rng.uniform(0.12, 0.2);
            m.transform(&Mat4::translation([px, py - lo[1], hs - 0.014 - lo[2]]));
            pillows.append(&m);
        }

        // ---- flanks ----
        let top = span.height;
        let fd = dp.min(0.42);
        let mut fst = Style::random(&mut rng, fd);
        fst.drawer_row = false;
        fst.upper_closed = false;
        fst.has_lower = true;
        fst.lower_h = rng.uniform(0.7, 0.9);
        let mut flanks: Vec<(f64, f64, Out)> = vec![];
        for (a, b) in [(fx0, sx0), (sx1, fx1)] {
            if b - a >= 0.28 {
                let mut fo = Out::default();
                tall_unit(&fst, &mut fo, &mut rng, a, b, top);
                crown(&fst, &mut fo, a, b, top);
                flanks.push((a, b, fo));
            }
        }
        // a slim cap/valance over the whole run is omitted: the window stays clear

        let mut asset = Asset::default();
        let col = ([sx0, 0.0, 0.0], [sx1, dp + 0.012, hs]);
        asset.mesh(o.case, &case_m, Some(col));
        asset.mesh(o.door, &door_m, None);
        if !o.counter.is_empty() {
            asset.mesh(o.counter, &case_m, None);
        }
        if !o.hw.is_empty() {
            asset.mesh(o.hw, &hw_m, None);
        }
        asset.mesh(cushions, &fab, None);
        if !backs.is_empty() {
            asset.mesh(backs, &fab, None);
        }
        if !pillows.is_empty() {
            asset.mesh(pillows, &accent, None);
        }
        asset.supports = o.supports;
        asset.supports.extend(supports);
        for (a, b, fo) in flanks {
            asset.mesh(fo.case, &case_m, Some(([a, 0.0, 0.0], [b, fd + 0.05, top])));
            asset.mesh(fo.door, &door_m, None);
            if !fo.counter.is_empty() {
                asset.mesh(fo.counter, &case_m, None);
            }
            if !fo.hw.is_empty() {
                asset.mesh(fo.hw, &hw_m, None);
            }
            asset.supports.extend(fo.supports);
        }
        let (flo, fhi) = (fx0.min(sx0), fx1.max(sx1));
        let fp = vec![[flo, 0.0], [fhi, 0.0], [fhi, dp + 0.012], [flo, dp + 0.012]];
        let mut keep = vec![([sx0, dp, 0.0], [sx1, dp + 0.45, 0.6])];
        if wl + wr > 0.0 {
            keep.push(([flo, fd, 0.0], [fhi, fd + 0.4, 2.0]));
        }
        FeatureAsset { asset, footprint: fp, keepout: keep, class: "WindowSeatFeature" }
    }
}
