//! Fireplace dressing: two solvers on a FireplaceFeature.
//!
//! * Mantel: a composed arrangement along the mantel shelf, one of three styles -- symmetric (a matched pair at the ends, a centrepiece
//!   between), asymmetric (a tall piece with two candlesticks clustered at one end, one small piece at the other), minimal (one plant
//!   or vase and one small piece). Items are scaled to the shelf depth; over-mantel art / mirrors keep the centre low.
//! * Hearth (fireplace front): a tool set on one side of the firebox and a log basket or log stack on the other, on the hearth slab
//!   beside the opening (never in front of it), or on the floor beside the hearth when the slab has no room.
//! Frames: everything is laid out in the feature's span frame (x along the wall, y into the room, z up) and posed to world.
use super::*;
use crate::rng::Rng;

const MANTEL_TALL: &[&str] = &["VaseFactory", "PlantContainerFactory"];
const MANTEL_SMALL: &[&str] = &["BowlFactory", "NatureShelfTrinketsFactory", "JarFactory", "CupFactory"];

pub fn dress(scene: &mut Scene, state: &State, features: &[crate::features::PlacedFeature], assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let _ = state;
    let mut log = vec![];
    for pf in features.iter().filter(|f| f.class == "FireplaceFeature") {
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "dress_fireplace", &pf.key]));
        let pose = pf.pose();
        let w = |p: [f64; 3]| pose.apply(p);
        let yaw = pf.yaw;
        let mut occ = Occ::from_scene(scene, &[pf.key.clone()]);
        let sups = &pf.asset.supports;
        // ---- mantel: the highest support box above 0.6 m
        if let Some(m) = sups.iter().filter(|s| s.1[2] > 0.6).max_by(|a, b| a.1[2].partial_cmp(&b.1[2]).unwrap()) {
            let (x0, x1, y0, y1, top) = (m.0[0] + 0.04, m.1[0] - 0.04, m.0[1], m.1[1], m.1[2]);
            let depth = y1 - y0;
            let yc = y0 + depth * 0.5;
            // something of the fireplace hung just above the shelf centre (over-mantel mirror / art): keep the centre low
            let xc = (x0 + x1) / 2.0;
            let over = pf.asset.parts.iter().any(|p| { let (l, h) = p.mesh.bounds(); l[2] > top + 0.01 && l[2] < top + 0.5 && l[0] < xc && h[0] > xc });
            let len = x1 - x0;
            let style = if len < 0.7 { 2 } else { [0, 0, 1, 2][r.randint(0, 4) as usize] };
            let mut slots: Vec<(f64, crate::assets::Asset, f64)> = vec![]; // (x, asset, max height)
            let fit = |a: crate::assets::Asset, hmax: f64| -> Option<(crate::assets::Asset, f64)> {
                let (dx, dy, dz) = dims(&a);
                if dx <= 0.0 || dz <= 0.0 { return None }
                let sc = (depth * 0.85 / dx.max(dy)).min(hmax / dz).min(1.0);
                if sc < 0.35 { return None }
                Some((a, sc))
            };
            let pick = |r: &mut Rng, list: &[&str], hmax: f64| -> Option<(crate::assets::Asset, f64)> {
                for _ in 0..6 {
                    let c = list[r.randint(0, list.len() as i64) as usize];
                    if let Some(x) = fit(assets.visual(c, r.next_u64() % 1_000_000, 0), hmax) { return Some(x) }
                }
                None
            };
            let centre_h = if over { 0.18 } else { 0.45 };
            match style {
                0 => {
                    // symmetric: identical pair at both ends + centrepiece
                    let pair_seed = r.next_u64();
                    let pair = if r.f64() < 0.6 { props::candlestick(pair_seed) } else { assets.visual("VaseFactory", pair_seed % 1_000_000, 0) };
                    if let Some((a, sc)) = fit(pair, 0.4) {
                        slots.push((x0 + 0.08, a.clone(), sc));
                        slots.push((x1 - 0.08, a, sc));
                    }
                    if let Some((a, sc)) = pick(&mut r, if over { MANTEL_SMALL } else { MANTEL_TALL }, centre_h) { slots.push((xc, a, sc)); }
                    if len > 1.3 { if let Some((a, sc)) = pick(&mut r, MANTEL_SMALL, 0.15) { slots.push((x0 + len * 0.28, a, sc)); } }
                }
                1 => {
                    // asymmetric: tall piece + two candlesticks at one end, one small piece at the other
                    let left = r.f64() < 0.5;
                    let (e0, sgn) = if left { (x0 + 0.1, 1.0) } else { (x1 - 0.1, -1.0) };
                    if let Some((a, sc)) = pick(&mut r, MANTEL_TALL, if over { 0.35 } else { 0.5 }) { slots.push((e0, a, sc)); }
                    let cs = r.next_u64();
                    if let Some((a, sc)) = fit(props::candlestick(cs), 0.35) { slots.push((e0 + sgn * 0.16, a, sc)); }
                    if let Some((a, sc)) = fit(props::candlestick(cs ^ 0x55), 0.28) { slots.push((e0 + sgn * 0.26, a, sc * 0.85)); }
                    if let Some((a, sc)) = pick(&mut r, MANTEL_SMALL, 0.15) { slots.push((if left { x1 - 0.12 } else { x0 + 0.12 }, a, sc)); }
                }
                _ => {
                    if let Some((a, sc)) = pick(&mut r, MANTEL_TALL, if over { 0.3 } else { 0.45 }) { slots.push((if r.f64() < 0.5 { x0 + 0.12 } else { x1 - 0.12 }, a, sc)); }
                    if let Some((a, sc)) = pick(&mut r, MANTEL_SMALL, 0.15) { slots.push((xc + r.uniform(-0.15, 0.15), a, sc)); }
                }
            }
            let mut n = 0;
            for (i, (x, a, sc)) in slots.iter().enumerate() {
                let at = w([*x, yc, top]);
                let b = box_at(a, at, yaw, *sc);
                if !occ.free(&b) { continue }
                let bb = place(scene, &format!("dress_mantel_{}_{i}", pf.key), "MantelDecor", a, at, yaw, *sc);
                occ.boxes.push(bb);
                n += 1;
            }
            log.push(format!("mantel {} style {style}: {n} items", pf.key));
        }
        // ---- hearth / fireplace front
        let info = &pf.info;
        let fx = info.get("firebox_x").and_then(|v| v.as_array()).map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)]);
        let Some(fx) = fx else { continue };
        // hearth / bench tops (the feature already cut them around the firebox opening): the piece on each side
        let side_piece = |left: bool| sups.iter().filter(|s| s.1[2] < 0.5 && if left { (s.0[0] + s.1[0]) / 2.0 < fx[0] } else { (s.0[0] + s.1[0]) / 2.0 > fx[1] })
            .max_by(|a, b| (a.1[0] - a.0[0]).partial_cmp(&(b.1[0] - b.0[0])).unwrap()).cloned();
        let breast = info.get("breast_x").and_then(|v| v.as_array()).map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)]).unwrap_or(fx);
        let depth_b = info.get("depth").and_then(|v| v.as_f64()).unwrap_or(0.4);
        let tool = props::tool_set(r.next_u64());
        let wood = if r.f64() < 0.6 { props::log_basket(r.next_u64()) } else { props::log_stack(r.next_u64()) };
        let tool_left = r.f64() < 0.5;
        let mut n = 0;
        for (k, item) in [(0, &tool), (1, &wood)] {
            if r.f64() < 0.15 { continue } // not every fireplace has both
            let left = (k == 0) == tool_left;
            let (dx, dy, _) = dims(item);
            let rad = 0.5 * dx.max(dy);
            let mut spots: Vec<[f64; 3]> = vec![];
            if let Some(h) = side_piece(left) {
                // on the slab beside the opening, toward its outer end, clear of the breast face
                let (zx0, zx1) = (h.0[0] + 0.03, h.1[0] - 0.03);
                let (zy0, zy1) = (h.0[1].max(depth_b * 0.0), h.1[1]);
                if zx1 - zx0 >= 2.0 * rad && zy1 - zy0 >= 2.0 * rad * 0.9 {
                    let (sx, sy) = (if left { zx0 + rad } else { zx1 - rad }, (zy0 + zy1) / 2.0);
                    // the slab's mesh top under the footprint (its support box lay 1 cm under a bevelled top: the stand's base sank)
                    let mut zt = h.1[2];
                    for p in &pf.asset.parts { for f in &p.mesh.faces {
                        let (mut l, mut u) = ([f64::MAX; 3], [f64::MIN; 3]);
                        for &i in f { let q = p.mesh.pos[i as usize]; for d in 0..3 { l[d] = l[d].min(q[d]); u[d] = u[d].max(q[d]); } }
                        if l[0] < sx + rad && u[0] > sx - rad && l[1] < sy + rad && u[1] > sy - rad && u[2] > zt && u[2] < h.1[2] + 0.06 { zt = u[2]; }
                    }}
                    spots.push([sx, sy, zt]);
                }
            }
            // floor beside the breast, against the wall-side of the room
            let fxp = if left { breast[0] - rad - 0.06 } else { breast[1] + rad + 0.06 };
            // (on what is really there: the hearth slab runs on past the breast, and a log basket stood 2 cm into it)
            let fy = rad + 0.1;
            // (the median over the footprint: the highest face anywhere under it was the slab's edge, and a basket floated 26 mm)
            let ftris = super::tris_of(&pf.asset);
            let mut zs: Vec<f64> = [[0.0, 0.0], [0.7, 0.0], [-0.7, 0.0], [0.0, 0.7], [0.0, -0.7]].iter().map(|o| super::heights(&ftris, fxp + o[0] * rad, fy + o[1] * rad).into_iter().filter(|z| *z < 0.06).fold(0.0f64, f64::max)).collect();
            zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let fz = zs[2];
            spots.push([fxp, fy, fz]);
            for (j, s) in spots.iter().enumerate() {
                let at = w(*s);
                let yaw_i = yaw + if k == 1 { std::f64::consts::FRAC_PI_2 * if r.f64() < 0.5 { 0.0 } else { 1.0 } } else { 0.0 };
                let b = box_at(item, at, yaw_i, 1.0);
                // (inside the room: the floor spot beside a breast in a corner put a log basket 12 cm into the side wall)
                let inner = crate::furn::layout2d::room_of(state, &pf.key).and_then(|rm| state.objs.get(&rm).and_then(|ro| ro.polygon.clone())).map(|g| crate::shp::buffer_mitre(&g, -crate::bed_decor::wall_half()));
                if inner.as_ref().map_or(false, |g| [[b.0[0], b.0[1]], [b.1[0], b.0[1]], [b.1[0], b.1[1]], [b.0[0], b.1[1]]].iter().any(|q| !crate::shp::contains_pt(g, *q))) { continue }
                if !occ.free(&b) { continue }
                let bb = place(scene, &format!("dress_hearth_{}_{k}_{j}", pf.key), if k == 0 { "FireplaceToolsFactory" } else { "FirewoodFactory" }, item, at, yaw_i, 1.0);
                occ.boxes.push(bb);
                n += 1;
                break;
            }
        }
        log.push(format!("hearth {}: {n} items", pf.key));
    }
    log
}
