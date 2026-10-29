//! Bathroom fixture dressing: composed clusters instead of a scatter.
//!   * bathtub: 2-3 toiletries (bottles scaled to 12-22 cm) grouped at one end of the rim, on the solid rim only
//!   * sink: a soap dispenser and a toothbrush cup at the back corners of the basin rim / ledge
//!   * toilet: half of the cisterns carry one small thing (a small plant or a spare roll pair)
use super::*;
use crate::rng::Rng;

pub fn dress(scene: &mut Scene, state: &State, visuals: &std::collections::HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let kind = match g.class.as_str() { "BathtubFactory" => 0, "StandingSinkFactory" | "BathroomSinkFactory" => 1, "ToiletFactory" => 2, _ => continue };
        let Some(a) = visuals.get(k) else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "dress_bath", k]));
        let tris = tris_of(a);
        // the VISUAL mesh's bounds: the solver box can sit centimetres off the built fixture
        let (lo, hi) = a.bounds();
        // the working surface = the highest plateau of the top heights (a raised toilet lid or a tap is higher than the cistern lid /
        // sink ledge, so the bounds' top is not the surface): highest 1 cm level shared by >= 4 cells of a 12 x 8 grid
        let band = {
            let mut levels: std::collections::BTreeMap<i64, usize> = Default::default();
            for gx in 0..12 { for gy in 0..8 {
                let (x, y) = (lo[0] + (hi[0] - lo[0]) * (gx as f64 + 0.5) / 12.0, lo[1] + (hi[1] - lo[1]) * (gy as f64 + 0.5) / 8.0);
                let t = heights(&tris, x, y).into_iter().fold(f64::NAN, f64::max);
                if t.is_finite() { *levels.entry((t * 100.0).round() as i64).or_default() += 1 }
            } }
            let z = levels.iter().rev().find(|(_, c)| **c >= 4).map(|(z, _)| *z as f64 / 100.0).unwrap_or(hi[2]);
            (z - 0.03, z + 0.02)
        };
        let mut occ = Occ::from_scene(scene, &[format!("{k}#")]);
        // items: (asset, scale)
        let toiletry = |r: &mut Rng| { let b = assets.visual("BottleFactory", r.next_u64() % 1_000_000, 0); let h = dims(&b).2.max(0.05); let s = (r.uniform(0.12, 0.22) / h).min(1.0); (b, s) };
        let items: Vec<(crate::assets::Asset, f64, &'static str)> = match kind {
            0 => (0..r.randint(2, 4)).map(|_| { let (b, s) = toiletry(&mut r); (b, s, "BottleFactory") }).collect(),
            1 => vec![(props::soap_dispenser(r.next_u64()), 1.0, "SoapDispenserFactory"), (props::toothbrush_cup(r.next_u64()), 1.0, "ToothbrushCupFactory")],
            _ => {
                if r.f64() < 0.5 { vec![] } else if r.f64() < 0.5 {
                    let p = assets.visual("PlantContainerFactory", r.next_u64() % 1_000_000, 0);
                    let h = dims(&p).2.max(0.05);
                    vec![(p, (0.22 / h).min(1.0), "PlantContainerFactory")]
                } else { vec![(spare_rolls(), 1.0, "ToiletPaperFactory")] }
            }
        };
        if items.is_empty() { continue }
        // anchor: tub = one short end of the rim; sink / toilet = the back (wall) side, both corners
        let ua = if hi[0] - lo[0] >= hi[1] - lo[1] { 0 } else { 1 };
        let va = 1 - ua;
        let lp = |u: f64, v: f64| { let mut p = [0.0; 2]; p[ua] = u; p[va] = v; p };
        let mut cands: Vec<[f64; 2]> = vec![];
        match kind {
            0 => {
                let end = if r.f64() < 0.5 { lo[ua] + 0.06 } else { hi[ua] - 0.06 };
                let inw = if end < (lo[ua] + hi[ua]) / 2.0 { 1.0 } else { -1.0 };
                for du in [0.0, 0.05, 0.1, 0.16, 0.22] { for t in [0.2, 0.35, 0.5, 0.65, 0.8, 0.08, 0.92] { cands.push(lp(end + inw * du, lo[va] + (hi[va] - lo[va]) * t)); } }
            }
            _ => {
                // back = the x side where the tap / cistern rises above the rim; then spots ordered from the back ledge forward
                let rises = |x: f64| (1..8).filter(|i| { let y = lo[1] + (hi[1] - lo[1]) * *i as f64 / 8.0; heights(&tris, x, y).iter().any(|z| *z > band.1 + 0.02) }).count();
                // toilets: the cistern (the highest part) is the back; sinks: the side the tap rises on
                let top_at = |x: f64| (1..8).map(|i| { let y = lo[1] + (hi[1] - lo[1]) * i as f64 / 8.0; heights(&tris, x, y).into_iter().fold(f64::MIN, f64::max) }).fold(f64::MIN, f64::max);
                let back_lo = if kind == 2 { top_at(lo[0] + 0.05) >= top_at(hi[0] - 0.05) } else { rises(lo[0] + 0.05) >= rises(hi[0] - 0.05) };
                for i in 0..10 {
                    let f = 0.03 + 0.045 * i as f64;
                    let x = if back_lo { lo[0] + f } else { hi[0] - f };
                    for t in [0.1, 0.9, 0.2, 0.8, 0.3, 0.7, 0.5] { cands.push([x, lo[1] + (hi[1] - lo[1]) * t]); }
                }
            }
        }
        let mut n = 0;
        for (i, (item, sc, cls)) in items.iter().enumerate() {
            let (dx, dy, dz) = dims(item);
            let rad = (0.5 * dx.max(dy) * sc + 0.004) * if kind == 1 { 0.7 } else { 1.0 };
            let dbg = std::env::var_os("DRESSDBG").is_some();
            let (mut f_spot, mut f_occ) = (0, 0);
            if dbg && kind != 0 {
                for gx in 0..8 { let x = lo[0] + (hi[0] - lo[0]) * (gx as f64 + 0.5) / 8.0;
                    let row: Vec<String> = (0..6).map(|gy| { let y = lo[1] + (hi[1] - lo[1]) * (gy as f64 + 0.5) / 6.0; let h = heights(&tris, x, y); format!("{:.2}", h.into_iter().fold(f64::NAN, f64::max)) }).collect();
                    eprintln!("DRESSDBG {k} grid x {x:.2}: {}", row.join(" ")); }
            }
            if dbg {
                let c = cands[0];
                let hs = heights(&tris, c[0], c[1]);
                eprintln!("DRESSDBG {k} kind {kind} bounds {:.2?} {:.2?} band {:.2?} cand0 {:.2?} heights {:.3?}", lo, hi, band, c, hs);
            }
            for c in &cands {
                let Some(zt) = level_spot(&tris, c[0], c[1], rad, band, (dz * sc + 0.03).max(0.08), if kind == 0 { Some(lo[2]) } else { None }) else { f_spot += 1; if dbg && f_spot == cands.len() { eprintln!("DRESSDBG {k} item {i}: all {} spots fail level_spot (rad {rad:.3})", cands.len()) } continue };
                let at = o.pose.apply([c[0], c[1], zt]);
                let yaw = r.uniform(0.0, std::f64::consts::TAU);
                let b = box_at(item, at, yaw, *sc);
                if !occ.free(&b) { f_occ += 1; if dbg { eprintln!("DRESSDBG {k} item {i}: spot blocked ({f_occ})") } continue }
                let bb = place(scene, &format!("dress_bath_{}_{i}", k.replace('/', "_")), cls, item, at, yaw, *sc);
                occ.boxes.push(bb);
                n += 1;
                break;
            }
        }
        log.push(format!("{} {k}: {n} of {}", ["tub", "sink", "toilet"][kind], items.len()));
    }
    log
}

/// two spare toilet rolls stacked
fn spare_rolls() -> crate::assets::Asset {
    let mut a = crate::assets::Asset::default();
    let paper = crate::mesh::Material::diffuse("tp_paper", [0.95, 0.95, 0.93], 0.9);
    for z in [0.0, 0.1] {
        a.parts.push(ig_kit::api::Part { mesh: crate::mesh::Mesh::revolve(0.0, 0.0, &[(0.0, z), (0.055, z), (0.055, z + 0.098), (0.0, z + 0.098)], 20), mat: paper.clone(), col: None });
    }
    a
}
