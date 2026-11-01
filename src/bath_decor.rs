//! Bathroom dressing after the solve (program.bath_decor): towels folded over the towel bars (HardwareFactory rails >= 0.3 m), a bath mat
//! in front of the bathtub or toilet, and 2-4 toiletry bottles on the sink's back rim. Bathroom views showed ~6 objects (Hypersim
//! bathrooms: towels, mats, toiletries everywhere); the clutter pass put its items on shelves the cramped views rarely see.
//! Scene geometry only (bottles are BottleFactory assets); own stream.
use crate::furn::state::State;
use crate::mesh::{Material, Mesh, Scene};
use crate::rng::Rng;
use serde_json::json;
use std::collections::HashMap;

fn fabric(scene: &mut Scene, name: &str, r: &mut Rng) -> usize {
    // towels: whites, greys, beiges, some muted colours
    let pal: [[f32; 3]; 7] = [[0.92, 0.92, 0.9], [0.85, 0.83, 0.78], [0.6, 0.6, 0.6], [0.75, 0.68, 0.58], [0.35, 0.45, 0.55], [0.55, 0.62, 0.55], [0.7, 0.55, 0.5]];
    let rgb = pal[r.randint(0, 7) as usize];
    let mut m = Material::diffuse(name, rgb, 0.95);
    m.spec = None;
    scene.material(m)
}

/// world axis-aligned box of a local box under `pose`
fn wbox(pose: &crate::furn::geom::Pose, l: [f64; 3], h: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for i in 0..8 {
        let w = pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
        for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
    }
    (lo, hi)
}

fn bx(lo: [f64; 3], hi: [f64; 3], pose: &crate::furn::geom::Pose) -> Mesh {
    let mut m = Mesh::cuboid(lo, hi);
    for q in m.pos.iter_mut() { *q = pose.apply(*q) }
    m
}

pub fn add(scene: &mut Scene, state: &State, visuals: &HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    let in_bath = |o: &crate::furn::state::ObjState| o.relations.iter().any(|r| r.target.starts_with("bathroom"));
    // the mats laid so far (world xy boxes): a toilet's mat and a tub's laid one over the other
    let mut mats: Vec<([f64; 3], [f64; 3])> = vec![];
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "bath_decor", k]));
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        match g.class.as_str() {
            // towel bar: a towel folded over it, hanging 0.35-0.55 m in front of the wall on both sides of the rail
            "HardwareFactory" if hi[1] - lo[1] >= 0.3 => {
                if r.f64() >= 0.85 { continue }
                let m = fabric(scene, &format!("towel:{k}"), &mut r);
                let w = (hi[1] - lo[1]) * r.uniform(0.6, 0.85);
                let yc = (lo[1] + hi[1]) / 2.0 + r.uniform(-0.05, 0.05);
                let drop = r.uniform(0.35, 0.55);
                let (x0, x1) = (lo[0] - 0.005, hi[0] + 0.01);
                let zt = hi[2] + 0.01;
                let t = 0.012;
                let mut mesh = bx([x0, yc - w / 2.0, zt - t], [x1, yc + w / 2.0, zt], &o.pose);
                mesh.append(&bx([x0, yc - w / 2.0, zt - drop], [x0 + t, yc + w / 2.0, zt], &o.pose));
                mesh.append(&bx([x1 - t, yc - w / 2.0, zt - drop * r.uniform(0.6, 0.95)], [x1, yc + w / 2.0, zt], &o.pose));
                // (a towel hanging into the basin or the cistern below the rail: none)
                let (ml, mh) = mesh.bounds();
                // (nor over another fitting lower on the wall)
                if state.objs.iter().any(|(kk, oo)| kk != k && oo.generator.as_ref().map_or(false, |gg| matches!(gg.class.as_str(), "BathtubFactory" | "ShowerFactory" | "ToiletFactory" | "StandingSinkFactory" | "HardwareFactory")) && {
                    // (over a basin 0.3 m clear: its tap stands above the box)
                    let b = wbox(&oo.pose, oo.geom.lo, oo.geom.hi); let up = |d: usize| if d == 2 && oo.generator.as_ref().map_or(false, |gg| gg.class.contains("Sink")) { 0.3 } else { 0.02 };
                    (0..3).all(|d| ml[d] < b.1[d] + up(d) && b.0[d] - 0.02 < mh[d]) }) { continue }
                // (nor over a window: a towel hung 12 cm into the glass's zone)
                if state.objs.iter().any(|(wk, w)| wk.starts_with("window") && w.generator.is_none() && { let b = wbox(&w.pose, w.geom.lo, w.geom.hi); (0..3).all(|d| ml[d] < b.1[d] + 0.15 && b.0[d] - 0.15 < mh[d]) }) { continue }
                scene.add(&format!("towel_{}#0", k.replace('/', "_")), "TowelFactory", mesh, m);
                log.push(format!("towel on {k}"));
            }
            // bath mat on the floor in front of the bathtub / toilet (front = local +x)
            "BathtubFactory" | "ToiletFactory" if in_bath(o) => {
                if g.class == "ToiletFactory" {
                    let mut r2 = Rng::seeded(crate::int_hash(&[&seed.to_string(), "bath_decor_wc", k]));
                    // wall shelf above the cistern (on the back wall, lo x) with toiletries and a folded towel, 70 %
                    // (not under a window: the wall layer can put one over the cistern)
                    let under_win = state.objs.iter().any(|(wk, w)| w.generator.is_none() && wk.starts_with("window") && {
                        let wc = w.pose.t;
                        let d = [wc[0] - o.pose.t[0], wc[1] - o.pose.t[1]];
                        let l = [o.pose.r[0][0] * d[0] + o.pose.r[1][0] * d[1], o.pose.r[0][1] * d[0] + o.pose.r[1][1] * d[1]];
                        let half = (w.geom.hi[0] - w.geom.lo[0]).max(w.geom.hi[1] - w.geom.lo[1]) / 2.0 + 0.4;
                        l[0] < lo[0] + 0.2 && l[0] > lo[0] - 0.5 && l[1].abs() < half
                    });
                    // the wall behind the cistern, marched to on the room's floor (the toilet's box reaches into thick walls: shelves
                    // stood 3-9 cm in the wall)
                    let floor = crate::furn::layout2d::room_of(state, k).and_then(|rm| state.objs.get(&rm).and_then(|ro| ro.polygon.clone())).map(|g| crate::shp::buffer_mitre(&g, -(crate::bed_decor::wall_half() + 0.005)));
                    let wall_x = {
                        let ym = (lo[1] + hi[1]) / 2.0;
                        let mut x = (lo[0] + hi[0]) / 2.0;
                        if let Some(f) = &floor { while x > lo[0] - 0.3 { let q = o.pose.apply([x - 0.01, ym, 0.0]); if !crate::shp::contains_pt(f, [q[0], q[1]]) { break } x -= 0.01; } }
                        x + 0.005
                    };
                    if r2.f64() < 0.7 && !under_win {
                        let sz = hi[2] + r2.uniform(0.3, 0.45);
                        let mut sw = r2.uniform(0.5, 0.7);
                        let yc = (lo[1] + hi[1]) / 2.0;
                        let sd = 0.16;
                        // (within the room: a toilet in a corner had its shelf run on into the side wall)
                        let inside = |w: f64| floor.as_ref().map_or(true, |f| [[wall_x + sd, yc - w / 2.0 - 0.03], [wall_x + sd, yc + w / 2.0 + 0.03], [wall_x + 0.01, yc - w / 2.0 - 0.03], [wall_x + 0.01, yc + w / 2.0 + 0.03]].iter().all(|q| { let p = o.pose.apply([q[0], q[1], 0.0]); crate::shp::contains_pt(f, [p[0], p[1]]) }));
                        while sw > 0.35 && !inside(sw) { sw -= 0.05; }
                        // (not over a towel rail or holder on the same wall: the shelf rested on one; nor into the mirror's frame)
                        let rail_under = state.objs.iter().any(|(hk, ho)| ho.generator.as_ref().map_or(false, |g| matches!(g.class.as_str(), "HardwareFactory" | "MirrorFactory" | "WallArtFactory")) && {
                            let b = wbox(&ho.pose, ho.geom.lo, ho.geom.hi);
                            let s0 = o.pose.apply([wall_x, yc - sw / 2.0 - 0.05, sz - 0.35]); let s1 = o.pose.apply([wall_x + sd + 0.05, yc + sw / 2.0 + 0.05, sz + 0.45]);
                            let (l, h) = ([s0[0].min(s1[0]), s0[1].min(s1[1]), s0[2].min(s1[2])], [s0[0].max(s1[0]), s0[1].max(s1[1]), s0[2].max(s1[2])]);
                            hk != k && (0..3).all(|d| b.0[d] < h[d] && l[d] < b.1[d]) });
                        if rail_under || !inside(sw) { /* no shelf */ } else {
                        let wood = scene.material(Material::diffuse(&format!("wc_shelf:{k}"), [0.85, 0.83, 0.8], 0.5));
                        scene.add(&format!("wcshelf_{}#0", k.replace('/', "_")), "WallShelfFactory", bx([wall_x, yc - sw / 2.0, sz], [wall_x + sd, yc + sw / 2.0, sz + 0.02], &o.pose), wood);
                        let n = r2.randint(2, 5) as usize;
                        for i in 0..n {
                            let y = yc - sw / 2.0 + sw * (i as f64 + 0.5) / n as f64;
                            // (no folded towel: it landed on a bottle)
                            let b = assets.visual("BottleFactory", r2.next_u64() % 1_000_000, 0);
                            let (blo, bhi) = b.bounds();
                            let sc = (0.17 / (bhi[2] - blo[2]).max(0.05)).min(1.0);
                            for (pi, p) in b.parts.iter().enumerate() {
                                let mut m = p.mesh.clone();
                                for q in m.pos.iter_mut() {
                                    *q = o.pose.apply([wall_x + 0.08 + (q[0] - (blo[0] + bhi[0]) / 2.0) * sc, y + (q[1] - (blo[1] + bhi[1]) / 2.0) * sc, sz + 0.02 + (q[2] - blo[2]) * sc]);
                                }
                                let mi = scene.material(p.mat.clone());
                                scene.add(&format!("wcbottle_{}_{i}#{pi}", k.replace('/', "_")), "BottleFactory", m, mi);
                            }
                        }
                        log.push(format!("wall shelf over {k}"));
                        }
                    }
                    // toilet-paper stand beside the bowl (chrome post + roll), 80 %
                    // on a side that is clear of other furniture (it stood inside a bathtub next to the toilet)
                    let x = (lo[0] + hi[0]) / 2.0;
                    // (and inside the room, clear of its walls: a toilet in a corner had its stand through the wall)
                    let floor = crate::furn::layout2d::room_of(state, k).and_then(|r| state.objs.get(&r).and_then(|ro| ro.polygon.clone())).map(|g| crate::shp::buffer_mitre(&g, -0.12));
                    let free = |side: f64| {
                        let wb = wbox(&o.pose, [x - 0.1, side - 0.06, lo[2]], [x + 0.1, side + 0.06, lo[2] + 0.62]);
                        let inside = floor.as_ref().map_or(true, |g| [[wb.0[0], wb.0[1]], [wb.1[0], wb.0[1]], [wb.1[0], wb.1[1]], [wb.0[0], wb.1[1]]].iter().all(|q| crate::shp::contains_pt(g, *q)));
                        inside && !state.objs.iter().any(|(kk, oo)| kk != k && oo.generator.is_some() && { let b = wbox(&oo.pose, oo.geom.lo, oo.geom.hi); (0..3).all(|d| wb.0[d] < b.1[d] && b.0[d] < wb.1[d]) })
                    };
                    let mut sides = [lo[1] - 0.14, hi[1] + 0.14];
                    if r2.f64() < 0.5 { sides.swap(0, 1) }
                    let side = sides.iter().cloned().find(|s| free(*s));
                    if let (true, Some(side)) = (r2.f64() < 0.8, side) {
                        // the floor itself (its top lies half a wall thickness up), not the toilet's box bottom: the stand floated 8 mm
                        let zf = crate::bed_decor::wall_half() - o.pose.t[2];
                        let chrome = scene.material(Material::diffuse("tp_chrome", [0.78, 0.78, 0.8], 0.2));
                        let paper = scene.material(Material::diffuse("tp_paper", [0.95, 0.95, 0.93], 0.9));
                        let mut post = ig_kit::geomkit::limb([x, side, zf], [x, side, zf + 0.62], 0.012, 0.012, true);
                        // (the base bar's axis at its radius above the floor: at 4 mm it sank 2.6 cm into it)
                        post.append(&ig_kit::geomkit::limb([x - 0.1, side, zf + 0.031], [x + 0.1, side, zf + 0.031], 0.03, 0.03, false));
                        for q in post.pos.iter_mut() { *q = o.pose.apply(*q) }
                        scene.add(&format!("tpstand_{}#0", k.replace('/', "_")), "ToiletPaperFactory", post, chrome);
                        for (j, zz) in [zf + 0.5, zf + 0.3, zf + 0.18].iter().enumerate() {
                            let mut roll = ig_kit::geomkit::limb([x, side, *zz], [x, side, *zz + 0.1], 0.055, 0.055, true);
                            for q in roll.pos.iter_mut() { *q = o.pose.apply(*q) }
                            scene.add(&format!("tproll_{}_{j}#0", k.replace('/', "_")), "ToiletPaperFactory", roll, paper);
                        }
                        log.push(format!("paper stand at {k}"));
                    }
                }
                if r.f64() >= if g.class == "BathtubFactory" { 0.8 } else { 0.4 } { continue }
                let m = fabric(scene, &format!("bathmat:{k}"), &mut r);
                let (mw, md) = (r.uniform(0.5, 0.8).min(hi[1] - lo[1]), r.uniform(0.4, 0.55));
                let yc = (lo[1] + hi[1]) / 2.0;
                // (on the floor itself, not the fixture's box bottom: a mat floated 2 cm)
                let zf = crate::bed_decor::wall_half() - o.pose.t[2];
                let mesh = bx([hi[0] + 0.03, yc - mw / 2.0, zf], [hi[0] + 0.03 + md, yc + mw / 2.0, zf + 0.012], &o.pose);
                // (clear of the other fixtures' footprints: a toilet's mat ran under the shower tray)
                let (ml, mh) = mesh.bounds();
                let hit = state.objs.iter().any(|(kk, oo)| kk != k && oo.generator.as_ref().map_or(false, |gg| matches!(gg.class.as_str(), "BathtubFactory" | "ShowerFactory" | "ToiletFactory" | "StandingSinkFactory")) && {
                    let b = wbox(&oo.pose, oo.geom.lo, oo.geom.hi); (0..2).all(|d| ml[d] < b.1[d] && b.0[d] < mh[d]) });
                // (on the room's own floor: a toilet facing a doorway had its mat out in the hall)
                let floor = crate::furn::layout2d::room_of(state, k).and_then(|rm| state.objs.get(&rm).and_then(|ro| ro.polygon.clone())).map(|g| crate::shp::buffer_mitre(&g, -crate::bed_decor::wall_half()));
                let off_floor = floor.as_ref().map_or(false, |f| [[hi[0] + 0.03, yc - mw / 2.0], [hi[0] + 0.03, yc + mw / 2.0], [hi[0] + 0.03 + md, yc - mw / 2.0], [hi[0] + 0.03 + md, yc + mw / 2.0]].iter().any(|q| { let p = o.pose.apply([q[0], q[1], 0.0]); !crate::shp::contains_pt(f, [p[0], p[1]]) }));
                if hit || off_floor || mats.iter().any(|(l, h)| (0..2).all(|d| ml[d] < h[d] + 0.03 && l[d] - 0.03 < mh[d])) { continue }
                mats.push((ml, mh));
                scene.add(&format!("bathmat_{}#0", k.replace('/', "_")), "BathMatFactory", mesh, m);
                log.push(format!("mat at {k}"));
            }
            // toiletries on the back rim of the sink (highest visual surface in the back 20 % of its depth)
            // sinks: dressed by crate::dress::bath (soap dispenser + toothbrush cup)
            "StandingSinkFactory" | "BathroomSinkFactory" if !crate::dress::ACTIVE.load(std::sync::atomic::Ordering::Relaxed) => {
                let Some(a) = visuals.get(k) else { continue };
                let d = hi[0] - lo[0];
                let rim: Vec<[f64; 3]> = a.parts.iter().flat_map(|p| p.mesh.pos.iter().copied()).filter(|q| q[0] < lo[0] + 0.2 * d && q[2] > lo[2] + 0.5 * (hi[2] - lo[2])).collect();
                if std::env::var_os("BATH_DEBUG").is_some() { eprintln!("BATHDBG {k} lo {lo:?} hi {hi:?} rim {} vis z {:?}", rim.len(), a.parts.iter().flat_map(|p| p.mesh.pos.iter()).map(|q| q[2]).fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)))); }
                if rim.is_empty() { continue }
                let n = r.randint(2, 5) as usize;
                let mut placed = 0;
                let mut taken: Vec<f64> = vec![];
                for i in 0..n * 4 {
                    if placed >= n { break }
                    // a flat spot: the visual's top surface within 4 cm of (x, y) is level (no tap, no basin edge)
                    let x = lo[0] + d * r.uniform(0.04, 0.18);
                    let y = lo[1] + (hi[1] - lo[1]) * r.uniform(0.06, 0.94);
                    if taken.iter().any(|t| (t - y).abs() < 0.09) { continue }
                    let near: Vec<f64> = rim.iter().filter(|q| (q[0] - x).hypot(q[1] - y) < 0.04).map(|q| q[2]).collect();
                    if near.len() < 3 { continue }
                    let zt = near.iter().cloned().fold(f64::MIN, f64::max);
                    let flat = near.iter().filter(|&&z| z > zt - 0.03).count() as f64 / near.len() as f64;
                    let around = rim.iter().filter(|q| (q[0] - x).hypot(q[1] - y) < 0.09 && q[2] > zt + 0.02).count();
                    if flat < 0.3 || around > 0 { continue }
                    taken.push(y);
                    let b = assets.visual("BottleFactory", r.next_u64() % 1_000_000, 0);
                    let (blo, bhi) = b.bounds();
                    let s = (0.16 / (bhi[2] - blo[2]).max(0.05)).min(1.0);
                    for (pi, p) in b.parts.iter().enumerate() {
                        let mut m = p.mesh.clone();
                        for q in m.pos.iter_mut() {
                            let l = [x + (q[0] - (blo[0] + bhi[0]) / 2.0) * s, y + (q[1] - (blo[1] + bhi[1]) / 2.0) * s, zt + (q[2] - blo[2]) * s];
                            *q = o.pose.apply(l);
                        }
                        let mi = scene.material(p.mat.clone());
                        scene.add(&format!("toiletry_{}_{i}#{pi}", k.replace('/', "_")), "BottleFactory", m, mi);
                    }
                    placed += 1;
                }
                if placed > 0 { log.push(format!("{placed} toiletries on {k}")) }
            }
            _ => {}
        }
    }
    log
}
