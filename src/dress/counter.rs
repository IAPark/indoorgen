//! Kitchen counter dressing (KitchenSpaceFactory runs): items by zone instead of a random scatter.
//!   * sink zone: a soap dispenser at the back corner of the sink, a dish rack on the side away from the stove
//!   * stove zone: a utensil crock beside the cooktop and two oil / vinegar bottles next to it
//!   * free end: a kettle, a canister set (three jars of one design, graded sizes) at the back, sometimes a fruit bowl
//! 4-8 items per run. The counter top, its long axis and the back (wall / backsplash) side are measured on the run's own mesh; the sink
//! and the stove are the SinkFactory / OvenFactory objects over the run. Every item stands on a level spot of the top, clear of the
//! upper cabinets and of everything already there.
use super::*;
use crate::rng::Rng;

pub fn dress(scene: &mut Scene, state: &State, visuals: &std::collections::HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    let world_c = |o: &crate::furn::state::ObjState| o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, (o.geom.lo[2] + o.geom.hi[2]) / 2.0]);
    // one kettle and one canister set per kitchen (each run drew its own: three kettles in a row); the runs by the range go first
    let mut kitchen_has: std::collections::HashSet<(String, &'static str)> = Default::default();
    let near_range = |k: &str| state.objs.iter().any(|(rk, ro)| ro.generator.as_ref().map_or(false, |g| g.class == "OvenFactory") && crate::furn::layout2d::dist(state, k, rk) < 0.1);
    let mut runs: Vec<&String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "KitchenSpaceFactory")).map(|(k, _)| k).collect();
    runs.sort_by_key(|k| !near_range(k));
    for k in runs {
        let o = &state.objs[k.as_str()];
        let room_k = crate::furn::layout2d::room_of(state, k).unwrap_or_default();
        if o.generator.as_ref().map_or(true, |g| g.class != "KitchenSpaceFactory") { continue }
        let Some(a) = visuals.get(k) else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "dress_counter", k]));
        let tris = tris_of(a);
        let (lo, hi) = a.bounds();
        // counter top = the highest plateau between 0.70 and 1.20 m (slab tops vary; a fixed 0.75-1.05 band caught slab undersides)
        let band = {
            let mut levels: std::collections::BTreeMap<i64, usize> = Default::default();
            for gx in 0..16 { for gy in 0..6 {
                let (x, y) = (lo[0] + (hi[0] - lo[0]) * (gx as f64 + 0.5) / 16.0, lo[1] + (hi[1] - lo[1]) * (gy as f64 + 0.5) / 6.0);
                let t = heights(&tris, x, y).into_iter().filter(|z| *z > lo[2] + 0.7 && *z < lo[2] + 1.2).fold(f64::NAN, f64::max);
                if t.is_finite() { *levels.entry((t * 100.0).round() as i64).or_default() += 1 }
            } }
            let z = levels.iter().rev().find(|(_, c)| **c >= 6).map(|(z, _)| *z as f64 / 100.0).unwrap_or(lo[2] + 0.9);
            (z - 0.03, z + 0.02)
        };
        // long axis u, depth axis v
        let ua = if hi[0] - lo[0] >= hi[1] - lo[1] { 0 } else { 1 };
        let va = 1 - ua;
        let to_local = |p: [f64; 3]| {
            let d = [p[0] - o.pose.t[0], p[1] - o.pose.t[1], p[2] - o.pose.t[2]];
            [0, 1, 2].map(|i| o.pose.r[0][i] * d[0] + o.pose.r[1][i] * d[1] + o.pose.r[2][i] * d[2])
        };
        let lp = |u: f64, v: f64| { let mut p = [0.0; 2]; p[ua] = u; p[va] = v; p };
        // counter top height and the back side: the side with geometry rising above the top (backsplash, wall cabinets)
        let (u0, u1, v0, v1) = (lo[ua], hi[ua], lo[va], hi[va]);
        let mut tops = vec![];
        let (mut rise_lo, mut rise_hi) = (0, 0);
        for i in 1..12 {
            let u = u0 + (u1 - u0) * i as f64 / 12.0;
            let c = lp(u, (v0 + v1) / 2.0);
            if let Some(t) = heights(&tris, c[0], c[1]).into_iter().filter(|z| *z >= band.0 && *z <= band.1).fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m: f64| m.max(z)))) { tops.push(t) }
            for (side, cnt) in [(v0 + 0.02, &mut rise_lo), (v1 - 0.02, &mut rise_hi)] {
                let q = lp(u, side);
                if heights(&tris, q[0], q[1]).iter().any(|z| *z > band.1 && *z < band.1 + 0.9) { *cnt += 1 }
            }
        }
        if tops.is_empty() { continue }
        tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ztop = tops[tops.len() / 2];
        let (vb, vf) = if rise_lo >= rise_hi { (v0, v1) } else { (v1, v0) }; // back, front
        let vs = (vf - vb).signum();
        let depth = (vf - vb).abs();
        // sink and stove positions along u (objects whose centre lies over the run)
        let along = |cls: &str| -> Option<f64> {
            state.objs.values().filter(|x| x.generator.as_ref().map_or(false, |g| g.class == cls)).map(|x| to_local(world_c(x))).find(|l| l[ua] > u0 - 0.1 && l[ua] < u1 + 0.1 && l[va] > v0 - 0.4 && l[va] < v1 + 0.4).map(|l| l[ua])
        };
        if std::env::var_os("DRESSDBG").is_some() {
            for x in state.objs.values().filter(|x| x.generator.as_ref().map_or(false, |g| g.class == "OvenFactory" || g.class == "SinkFactory")) {
                eprintln!("DRESSDBG {k} {} local {:.2?} run u {:.2}..{:.2} v {:.2}..{:.2}", x.generator.as_ref().unwrap().class, to_local(world_c(x)), u0, u1, v0, v1);
            }
        }
        let sink = along("SinkFactory");
        // a range stands in line with the runs (at an end, or between two): the one beside this run's end within 0.9 m counts, at
        // that end (the crock and the bottles go on the counter next to it)
        let stove = along("OvenFactory").or_else(|| {
            state.objs.values().filter(|x| x.generator.as_ref().map_or(false, |g| g.class == "OvenFactory")).map(|x| to_local(world_c(x)))
                .filter(|l| l[va] > v0 - 0.3 && l[va] < v1 + 0.3 && (l[ua] < u0 && l[ua] > u0 - 0.9 || l[ua] > u1 && l[ua] < u1 + 0.9))
                .map(|l| l[ua].clamp(u0 - 0.3, u1 + 0.3)).next()
        });
        let mut occ = Occ::from_scene(scene, &[format!("{k}#")]);
        // planned slots: (u, v from the back, asset, scale, class)
        let mut plan: Vec<(f64, f64, crate::assets::Asset, f64, &'static str)> = vec![];
        let clampu = |u: f64| u.clamp(u0 + 0.12, u1 - 0.12);
        if let Some(us) = sink {
            let away = match stove { Some(st) => if st > us { -1.0 } else { 1.0 }, None => if r.f64() < 0.5 { -1.0 } else { 1.0 } };
            plan.push((clampu(us - away * 0.32), 0.07, props::soap_dispenser(r.next_u64()), 1.0, "SoapDispenserFactory"));
            if r.f64() < 0.75 { plan.push((clampu(us + away * 0.62), depth * 0.5, props::dish_rack(r.next_u64()), 1.0, "DishRackFactory")); }
        }
        if let Some(ust) = stove {
            let side = if r.f64() < 0.5 { -1.0 } else { 1.0 };
            let uc = clampu(ust + side * 0.5);
            plan.push((uc, 0.08, props::utensil_crock(r.next_u64()), 1.0, "UtensilCrockFactory"));
            let bs = r.next_u64() % 1_000_000;
            for (j, du) in [0.11f64, 0.19].iter().enumerate() {
                let b = assets.visual("BottleFactory", (bs + j as u64) % 1_000_000, 0);
                let h = dims(&b).2.max(0.05);
                plan.push((clampu(uc + side * du), 0.06, b, (r.uniform(0.24, 0.3) / h).min(1.0), "BottleFactory"));
            }
        }
        // the free end: farthest from sink and stove
        let busy: Vec<f64> = [sink, stove].iter().flatten().cloned().collect();
        let end = if busy.is_empty() { if r.f64() < 0.5 { u0 + 0.2 } else { u1 - 0.2 } } else {
            let d0 = busy.iter().map(|b| (b - (u0 + 0.2)).abs()).fold(f64::MAX, f64::min);
            let d1 = busy.iter().map(|b| (b - (u1 - 0.2)).abs()).fold(f64::MAX, f64::min);
            if d0 > d1 { u0 + 0.2 } else { u1 - 0.2 }
        };
        let inward = if end < (u0 + u1) / 2.0 { 1.0 } else { -1.0 };
        if r.f64() < 0.7 && kitchen_has.insert((room_k.clone(), "kettle")) { plan.push((end, depth * 0.45, props::kettle(r.next_u64()), 1.0, "KettleFactory")); }
        // canister set: one jar design at three sizes, back line
        let js = r.next_u64() % 1_000_000;
        let jar = assets.visual("JarFactory", js, 0);
        let jh = dims(&jar).2.max(0.05);
        for (j, hh) in [0.24f64, 0.2, 0.16].iter().enumerate() {
            if j == 0 && !kitchen_has.insert((room_k.clone(), "jars")) { break }
            plan.push((clampu(end + inward * (0.22 + j as f64 * 0.13)), 0.08, jar.clone(), (hh / jh).min(1.2), "JarFactory"));
        }
        // (bowls at most 0.28 m across: a basin-sized bowl stood on the counter)
        let fit = |a: &crate::assets::Asset| { let (dx, dy, _) = dims(a); (0.28 / dx.max(dy).max(0.05)).min(1.0) };
        if r.f64() < 0.4 { let a = assets.visual("FruitContainerFactory", r.next_u64() % 1_000_000, 0); let sc = fit(&a); plan.push((clampu((u0 + u1) / 2.0 + r.uniform(-0.3, 0.3)), depth * 0.55, a, sc, "FruitContainerFactory")); }
        // a long run (2.4 m and up) holds more: a microwave at the back half the time, and a bowl or a box of food
        if u1 - u0 >= 2.4 {
            let mid = |r: &mut Rng| clampu((u0 + u1) / 2.0 + r.uniform(-0.5, 0.5));
            if r.f64() < 0.5 { let u = mid(&mut r); plan.push((u, 0.05, assets.visual("MicrowaveFactory", r.next_u64() % 1_000_000, 0), 1.0, "MicrowaveFactory")); }
            let cls = ["BowlFactory", "FoodBoxFactory"][r.choice_idx(2)];
            let u = mid(&mut r);
            let a = assets.visual(cls, r.next_u64() % 1_000_000, 0);
            let sc = if cls == "BowlFactory" { fit(&a) } else { 1.0 };
            plan.push((u, depth * 0.5, a, sc, cls));
        }
        let mut n = 0;
        for (i, (u, vb_off, item, sc, cls)) in plan.iter().enumerate() {
            let (dx, dy, dz) = dims(item);
            let rad = 0.5 * dx.max(dy) * sc + 0.005;
            if 2.0 * rad > depth - 0.04 { continue }
            let v = vb + vs * (vb_off + rad).min(depth - rad - 0.02);
            let c = lp(*u, v);
            let dbg = std::env::var_os("DRESSDBG").is_some();
            let Some(zt) = level_spot(&tris, c[0], c[1], rad, band, (dz * sc + 0.03).max(0.1), None) else { if dbg { eprintln!("DRESSDBG {k} item {i} {cls} u {u:.2} v {v:.2}: no level spot (ztop {ztop:.3}, heights {:.3?})", heights(&tris, c[0], c[1])) } continue };
            if (zt - ztop).abs() > 0.03 { if dbg { eprintln!("DRESSDBG {k} item {i}: zt {zt:.3} vs top {ztop:.3}") } continue }
            let at = o.pose.apply([c[0], c[1], zt]);
            // items face the room: yaw from the back toward the front
            let mut fv = [0.0; 3]; fv[va] = vs;
            let f = o.pose.rot(fv);
            let yaw = f[1].atan2(f[0]) + if *cls == "DishRackFactory" { std::f64::consts::FRAC_PI_2 } else { r.uniform(-0.3, 0.3) };
            let b = box_at(item, at, yaw, *sc);
            // (not up into a window over the counter: a microwave stood in front of the glass)
            let in_window = state.objs.iter().any(|(wk, w)| wk.starts_with("window") && w.generator.is_none() && {
                let (l, h) = (w.geom.lo, w.geom.hi);
                let (mut wl, mut wh) = ([f64::MAX; 3], [f64::MIN; 3]);
                for i in 0..8 { let p = w.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]); for d in 0..3 { wl[d] = wl[d].min(p[d]); wh[d] = wh[d].max(p[d]); } }
                (0..2).all(|d| b.0[d] < wh[d] + 0.3 && wl[d] - 0.3 < b.1[d]) && b.1[2] > wl[2]
            });
            if in_window { continue }
            if !occ.free(&b) { if std::env::var_os("DRESSDBG").is_some() { eprintln!("DRESSDBG {k} item {i} {cls}: blocked") } continue }
            let bb = place(scene, &format!("dress_counter_{}_{i}", k.replace('/', "_")), cls, item, at, yaw, *sc);
            occ.boxes.push(bb);
            n += 1;
        }
        log.push(format!("counter {k}: {n} of {} planned (sink {:?}, stove {:?})", plan.len(), sink.map(|x| (x * 100.0).round() / 100.0), stove.map(|x| (x * 100.0).round() / 100.0)));
    }
    // cookware on the ranges: a pot and / or a pan on burners of the cooktop (the plateau under the backsplash panel), one to two
    for (k, o) in &state.objs {
        if o.generator.as_ref().map_or(true, |g| g.class != "OvenFactory") { continue }
        let Some(a) = visuals.get(k) else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "dress_range", k]));
        let tris = tris_of(a);
        let (lo, hi) = a.bounds();
        let Some(ztop) = plateau(a, lo[2] + 0.6, lo[2] + 1.1, 10) else { continue };
        let band = (ztop - 0.02, ztop + 0.02);
        // burners: across the width (local y), toward the front (local +x, the door side)
        let spots = [(0.62, 0.3), (0.62, 0.7), (0.4, 0.3), (0.4, 0.7)];
        let mut occ = Occ::from_scene(scene, &[format!("{k}#")]);
        let want = 1 + r.choice_idx(2);
        let mut n = 0;
        for (i, (fx, fy)) in spots.iter().enumerate() {
            if n >= want { break }
            if r.f64() < 0.3 { continue }
            let cls = ["PotFactory", "PanFactory"][r.choice_idx(2)];
            let item = assets.visual(cls, r.next_u64() % 1_000_000, 0);
            let (dx, dy, _dz) = dims(&item);
            // a burner holds about a 24 cm vessel (the pan's handle overhangs: its body is what counts)
            let sc = (0.24 / dx.min(dy).max(0.05)).min(1.0);
            let (x, y) = (lo[0] + (hi[0] - lo[0]) * fx, lo[1] + (hi[1] - lo[1]) * fy);
            // (level under the vessel's whole base: probed at 6 cm, a pan's rim sat on the grate's sloping edge)
            let rad = 0.5 * dx.min(dy) * sc * 0.85;
            let Some(zt) = level_spot(&tris, x, y, rad, band, 0.15, None) else { continue };
            // (and flat there: the top triangle under each probe within 25 deg of level -- a rim on a rounded grate bar tilted)
            let flat = [[x, y], [x - rad, y], [x + rad, y], [x, y - rad], [x, y + rad]].iter().all(|q| {
                tris.iter().filter(|t| { let (a, b, c) = (t[0], t[1], t[2]);
                    let s = |p: [f64; 3], q2: [f64; 3], r2: [f64; 2]| (q2[0] - p[0]) * (r2[1] - p[1]) - (q2[1] - p[1]) * (r2[0] - p[0]);
                    let (d1, d2, d3) = (s(a, b, *q), s(b, c, *q), s(c, a, *q)); (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0) })
                    .map(|t| { let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]; let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                        let zc = (t[0][2] + t[1][2] + t[2][2]) / 3.0; (zc, (n[2] / l).abs()) })
                    .filter(|(z, _)| *z < zt + 0.02).max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).map_or(false, |(_, nz)| nz > 0.9)
            });
            if !flat { continue }
            let at = o.pose.apply([x, y, zt]);
            let yaw = r.uniform(0.0, std::f64::consts::TAU);
            let b = box_at(&item, at, yaw, sc);
            if !occ.free(&b) { continue }
            let bb = place(scene, &format!("dress_range_{}_{i}", k.replace('/', "_")), cls, &item, at, yaw, sc);
            occ.boxes.push(bb);
            n += 1;
        }
        log.push(format!("range {k}: {n} cookware"));
    }
    log
}
