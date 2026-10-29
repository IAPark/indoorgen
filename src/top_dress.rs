//! Top-surface dressing after the solve (program.top_dress): small everyday objects on the level spots of kitchen counters / islands
//! (jars, bottles, bowls, cups, pots, pans, fruit bowls, food boxes, plates, cans), bathtub rims and toilet tanks (toiletries), coffee
//! tables and nightstands (bowls, cups, books, jars). Hypersim kitchens and bathrooms carry dozens of these; ours had few in view.
//!
//! A spot is accepted when the furniture's own visual mesh is level under the item's footprint (within a z band where that furniture's
//! working surface is), nothing of the furniture is above it within `clear` (upper cabinets, taps, backsplash), and no other object
//! (solver objects, clutter, earlier items) occupies the item's box. Items are existing factory assets; scene geometry only; own stream.
use crate::furn::state::State;
use crate::mesh::{Mesh, Scene};
use crate::rng::Rng;
use std::collections::HashMap;

type Box3 = ([f64; 3], [f64; 3]);

fn overlaps(a: &Box3, b: &Box3) -> bool { (0..3).all(|d| a.0[d] < b.1[d] && b.0[d] < a.1[d]) }

/// heights (z) of every triangle covering (x, y)
fn heights(tris: &[[[f64; 3]; 3]], x: f64, y: f64) -> Vec<f64> {
    let mut out = vec![];
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        if x < a[0].min(b[0]).min(c[0]) || x > a[0].max(b[0]).max(c[0]) || y < a[1].min(b[1]).min(c[1]) || y > a[1].max(b[1]).max(c[1]) { continue }
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-12 { continue }
        let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
        let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
        let l3 = 1.0 - l1 - l2;
        if l1 < -1e-9 || l2 < -1e-9 || l3 < -1e-9 { continue }
        out.push(l1 * a[2] + l2 * b[2] + l3 * c[2]);
    }
    out
}

struct Kind { items: &'static [&'static str], per_m: f64, min: usize, max: usize, z: (f64, f64), clear: f64, p: f64 }

fn kind(class: &str) -> Option<Kind> {
    const KITCHEN: &[&str] = &["JarFactory", "JarFactory", "BottleFactory", "BowlFactory", "CupFactory", "CupFactory", "PotFactory", "PanFactory", "FruitContainerFactory", "FoodBoxFactory", "FoodBoxFactory", "PlateFactory", "CanFactory"];
    const BATH: &[&str] = &["BottleFactory", "BottleFactory", "JarFactory", "CupFactory"];
    const DECOR: &[&str] = &["VaseFactory", "JarFactory", "BowlFactory", "BottleFactory", "PlantContainerFactory", "CupFactory"];
    const TABLE: &[&str] = &["BowlFactory", "CupFactory", "CupFactory", "PlateFactory", "JarFactory", "BottleFactory", "FruitContainerFactory"];
    // z band: (from, to) metres above the object's bottom (None-like negative from = relative to its top)
    Some(match class {
        "KitchenSpaceFactory" => Kind { items: KITCHEN, per_m: 1.5, min: 2, max: 7, z: (0.75, 1.05), clear: 0.35, p: 1.0 },
        "KitchenIslandFactory" => Kind { items: KITCHEN, per_m: 1.2, min: 1, max: 4, z: (0.75, 1.05), clear: 0.35, p: 0.9 },
        "BathtubFactory" => Kind { items: BATH, per_m: 3.0, min: 2, max: 6, z: (-0.12, 0.01), clear: 0.3, p: 0.9 },
        "ToiletFactory" => Kind { items: BATH, per_m: 4.0, min: 1, max: 3, z: (-0.08, 0.01), clear: 0.3, p: 0.85 },
        "CoffeeTableFactory" => Kind { items: TABLE, per_m: 3.0, min: 1, max: 4, z: (-0.06, 0.01), clear: 0.3, p: 0.8 },
        "SideTableFactory" => Kind { items: TABLE, per_m: 5.0, min: 2, max: 3, z: (-0.06, 0.01), clear: 0.25, p: 0.9 },
        // bedroom / study furniture: desks, dressers and cabinets
        "SimpleDeskFactory" | "SidetableDeskFactory" => Kind { items: TABLE, per_m: 3.0, min: 2, max: 5, z: (-0.06, 0.01), clear: 0.3, p: 0.9 },
        "SingleCabinetFactory" | "KitchenCabinetFactory" => Kind { items: DECOR, per_m: 3.0, min: 2, max: 5, z: (-0.06, 0.01), clear: 0.3, p: 0.85 },
        _ => return None,
    })
}

pub fn add(scene: &mut Scene, state: &State, visuals: &HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    // occupied space: every solver object's world box (minus the one being dressed) and every clutter / decor node already in the scene
    let world_box = |o: &crate::furn::state::ObjState| -> Box3 {
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let mut taken: Vec<Box3> = scene.nodes.iter().filter(|n| ["mess_", "toiletry_", "cushion_", "towel_", "bathmat_"].iter().any(|p| n.name.starts_with(p))).map(|n| n.mesh.bounds()).collect();
    let objs: Vec<(String, Box3)> = state.objs.iter().filter(|(_, o)| o.generator.is_some()).map(|(k, o)| (k.clone(), world_box(o))).collect();
    let mut n_items = 0usize;
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let Some(kd) = kind(&g.class) else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "top_dress", k]));
        if r.f64() >= kd.p { continue }
        let Some(a) = visuals.get(k) else { continue };
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        let (zb0, zb1) = if kd.z.0 < 0.0 { (hi[2] + kd.z.0, hi[2] + kd.z.1) } else { (lo[2] + kd.z.0, lo[2] + kd.z.1) };
        let tris: Vec<[[f64; 3]; 3]> = a.parts.iter().flat_map(|p| p.mesh.faces.iter().flat_map(move |f| (1..f.len().saturating_sub(1)).map(move |i| [p.mesh.pos[f[0] as usize], p.mesh.pos[f[i] as usize], p.mesh.pos[f[i + 1] as usize]]))).collect();
        let len = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        let want = ((len * kd.per_m) as usize).clamp(kd.min, kd.max);
        let me = world_box(o);
        let others: Vec<&Box3> = objs.iter().filter(|(kk, b)| kk != k && overlaps(&(b.0, b.1), &([me.0[0] - 0.05, me.0[1] - 0.05, me.0[2]], [me.1[0] + 0.05, me.1[1] + 0.05, me.1[2] + 1.0]))).map(|(_, b)| b).collect();
        let mut placed = 0;
        for t in 0..want * 12 {
            if placed >= want { break }
            let cls = kd.items[r.randint(0, kd.items.len() as i64) as usize];
            let item = assets.visual(cls, r.next_u64() % 1_000_000, 0);
            let (ilo, ihi) = item.bounds();
            let (iw, id, ih) = (ihi[0] - ilo[0], ihi[1] - ilo[1], ihi[2] - ilo[2]);
            if iw > 0.45 || id > 0.45 || ih > 0.4 || iw <= 0.0 { continue }
            let rad = 0.5 * iw.max(id) + 0.01;
            let x = r.uniform(lo[0] + rad, (hi[0] - rad).max(lo[0] + rad));
            let y = r.uniform(lo[1] + rad, (hi[1] - rad).max(lo[1] + rad));
            // level surface under the footprint (5 probes inside the z band, within 1.5 cm), nothing of the furniture just above
            let probes = [[x, y], [x - rad * 0.8, y - rad * 0.8], [x + rad * 0.8, y - rad * 0.8], [x - rad * 0.8, y + rad * 0.8], [x + rad * 0.8, y + rad * 0.8]];
            let mut tops = vec![];
            let mut blocked = false;
            for pr in probes {
                let hs = heights(&tris, pr[0], pr[1]);
                let Some(top) = hs.iter().cloned().filter(|z| *z >= zb0 && *z <= zb1).fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.max(z)))) else { blocked = true; break };
                if hs.iter().any(|z| *z > top + 0.01 && *z < top + kd.clear.max(ih + 0.03)) { blocked = true; break }
                tops.push(top);
            }
            if blocked { continue }
            let zt = tops.iter().cloned().fold(f64::MIN, f64::max);
            if tops.iter().any(|z| zt - z > 0.015) { continue }
            // world box of the item, clear of every other object and earlier item
            let c = o.pose.apply([x, y, zt]);
            let yaw = r.uniform(0.0, std::f64::consts::TAU);
            let bx: Box3 = ([c[0] - rad, c[1] - rad, c[2] + 0.002], [c[0] + rad, c[1] + rad, c[2] + ih]);
            if others.iter().any(|b| overlaps(&bx, b)) || taken.iter().any(|b| overlaps(&bx, b)) { continue }
            let (cx, cy) = ((ilo[0] + ihi[0]) / 2.0, (ilo[1] + ihi[1]) / 2.0);
            for (pi, p) in item.parts.iter().enumerate() {
                let mut m: Mesh = p.mesh.clone();
                for q in m.pos.iter_mut() {
                    let (dx, dy) = (q[0] - cx, q[1] - cy);
                    *q = [c[0] + dx * yaw.cos() - dy * yaw.sin(), c[1] + dx * yaw.sin() + dy * yaw.cos(), c[2] + 0.002 + (q[2] - ilo[2])];
                }
                let mi = scene.material(p.mat.clone());
                scene.add(&format!("dress_{}_{t}#{pi}", k.replace('/', "_")), cls, m, mi);
            }
            taken.push(bx);
            placed += 1;
        }
        if placed > 0 { log.push(format!("{placed} items on {k}")); n_items += placed; }
    }
    log.push(format!("total {n_items}"));
    log
}
