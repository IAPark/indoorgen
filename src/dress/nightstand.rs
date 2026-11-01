//! Nightstand dressing: a bedside lamp on every nightstand of a bed that has none on it, toward the back of the top, and half the
//! time a book stack beside it. (The solver's lamps land on any bedroom surface; the second nightstand of a pair is a copy made after
//! the solve and came up bare.)
use super::*;
use crate::rng::Rng;

pub fn dress(scene: &mut Scene, state: &State, visuals: &std::collections::HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    for (k, o) in &state.objs {
        if o.generator.as_ref().map_or(true, |g| g.class != "SideTableFactory") { continue }
        // nightstands: a bed among the relation targets
        if !o.relations.iter().any(|r| state.objs.get(&r.target).and_then(|t| t.generator.as_ref()).map_or(false, |g| g.class == "BedFactory")) { continue }
        let has_lamp = state.objs.values().any(|x| x.generator.as_ref().map_or(false, |g| g.class.contains("Lamp")) && x.relations.iter().any(|r| &r.target == k));
        let Some(a) = visuals.get(k) else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "dress_nightstand", k]));
        let tris = tris_of(a);
        let (lo, hi) = a.bounds();
        let Some(ztop) = plateau(a, lo[2] + 0.3, lo[2] + 0.9, 8) else { continue };
        let band = (ztop - 0.02, ztop + 0.02);
        // (the bed's own boxes are left out: its bedding's bounds reach over the nightstand top though the cover does not)
        let beds: Vec<String> = o.relations.iter().filter(|r| state.objs.get(&r.target).and_then(|t| t.generator.as_ref()).map_or(false, |g| g.class == "BedFactory")).map(|r| format!("{}#", r.target)).collect();
        let mut skip = vec![format!("{k}#")];
        skip.extend(beds);
        let mut occ = Occ::from_scene(scene, &skip);
        occ.add_window_zones(scene);
        let mut items: Vec<(&str, f64, f64)> = vec![];
        // local frame: the back is -x (the nightstand faces +x like the bed), across is y
        if !has_lamp { items.push(("DeskLampFactory", 0.35, if r.f64() < 0.5 { 0.35 } else { 0.65 })); }
        if r.f64() < 0.5 { items.push(("BookStackFactory", 0.6, if items.first().map_or(0.5, |i| i.2) < 0.5 { 0.7 } else { 0.3 })); }
        let mut n = 0;
        for (i, (cls, fx, fy)) in items.iter().enumerate() {
            let item = assets.visual(cls, r.next_u64() % 1_000_000, 0);
            let (dx, dy, dz) = dims(&item);
            let top_w = (hi[0] - lo[0]).min(hi[1] - lo[1]);
            let sc = ((top_w * 0.45) / dx.max(dy).max(0.05)).min(1.0);
            let rad = 0.5 * dx.max(dy) * sc;
            let dbg = std::env::var_os("DRESSDBG").is_some();
            let yaw = o.pose.r[1][0].atan2(o.pose.r[0][0]) + r.uniform(-0.4, 0.4);
            // the planned spot, then the other side and further forward (a window beside the bed takes the back corner on its side)
            let tries = [(*fx, *fy), (*fx, 1.0 - fy), (fx + 0.2, *fy), (fx + 0.2, 1.0 - fy)];
            let mut found = None;
            for (tx, ty) in tries {
                let (x, y) = (lo[0] + (hi[0] - lo[0]) * tx, lo[1] + (hi[1] - lo[1]) * ty);
                let Some(zt) = level_spot(&tris, x, y, rad.min(0.08), band, (dz * sc).min(0.6), None) else { continue };
                let at = o.pose.apply([x, y, zt]);
                let b = box_at(&item, at, yaw, sc);
                if occ.free(&b) { found = Some(at); break }
            }
            let Some(at) = found else { if dbg { eprintln!("DRESSDBG nightstand {k} {cls}: no free level spot (dims {dx:.2} {dy:.2} {dz:.2} sc {sc:.2})") } continue };
            let bb = place(scene, &format!("dress_nightstand_{}_{i}", k.replace('/', "_")), cls, &item, at, yaw, sc);
            occ.boxes.push(bb);
            n += 1;
        }
        log.push(format!("nightstand {k}: {n} of {}", items.len()));
    }
    log
}
