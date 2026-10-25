use infinigen::furn::assets::TableAssets;
use infinigen::furn::solver::{Assets, PlaceholderKind};
use infinigen::furn::solver::Usage;
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let usage = Usage::from_json(&prog["used_as"]);
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = infinigen::assets::RealAssets { fallback };
    let mut stats: BTreeMap<String, (usize, usize, f64, usize)> = BTreeMap::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j.get("kind").is_some() { continue; }
        let class = j["class"].as_str().unwrap().to_string();
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let verts: Vec<[f64; 3]> = j["verts"].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &verts { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
        let kind = if usage.has_usage(&class, "RealPlaceholder") { PlaceholderKind::Real } else if usage.has_usage(&class, "AssetAsPlaceholder") { PlaceholderKind::AssetAsPlaceholder } else if usage.has_usage(&class, "PlaceholderBBox") { PlaceholderKind::BboxOfPlaceholder } else { PlaceholderKind::BboxOfAsset };
        if std::env::var_os("PHKIND").is_some() { eprintln!("KIND {} {:?}", class, kind as u8); }
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        let real = infinigen::assets::make_factory(&class, fs).is_some();
        // Side/Coffee tables take their dimensions from constructor draws on the global stream (not in the oracle): fit them to the python bounds
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2]];
        let fitted = if real && (class == "SideTableFactory" || class == "CoffeeTableFactory") {
            let side = class == "SideTableFactory";
            let mut u = if side { vec![1.0, 1.0] } else { vec![1.2, 0.7, 0.45] };
            for _ in 0..40 {
                infinigen::assets::set_unseeded(Some(u.clone()));
                let Some(g) = assets.placeholder(&class, fs, inst, kind) else { break };
                let e = [g.hi[0] - g.lo[0], g.hi[1] - g.lo[1], g.hi[2]];
                if side { u[0] *= ext[0] / e[0]; u[1] *= ext[2] / e[2]; } else { for k in 0..3 { u[k] *= ext[k] / e[k]; } }
            }
            infinigen::assets::set_unseeded(Some(u));
            let g = assets.placeholder(&class, fs, inst, kind);
            infinigen::assets::set_unseeded(None);
            g
        } else { None };
        let gopt = if fitted.is_some() { fitted } else { assets.placeholder(&class, fs, inst, kind) };
        if let (true, Some(g)) = (real, gopt) {
            let d = (0..3).map(|k| (g.lo[k] - lo[k]).abs().max((g.hi[k] - hi[k]).abs())).fold(0.0, f64::max);
            if std::env::var_os("PHTAGS").is_some() {
                let names = j["face_names"].as_array().cloned().unwrap_or_default();
                let polys = j["polys"].as_array().unwrap();
                let mut bad = 0;
                for (pi, poly) in polys.iter().enumerate() {
                    let ix: Vec<usize> = poly.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
                    let c: [f64; 3] = std::array::from_fn(|k| (verts[ix[0]][k] + verts[ix[1]][k] + verts[ix[2]][k]) / 3.0);
                    let mut bits = 0u32;
                    if !names.is_empty() { for part in names[pi].as_str().unwrap().split('.') { let mut cc = part.to_string(); if let Some(f) = cc.get_mut(0..1) { f.make_ascii_uppercase(); } bits |= infinigen::furn::geom::subpart_bit(&cc); } }
                    let m = bits & (infinigen::furn::geom::BACK | infinigen::furn::geom::FRONT | infinigen::furn::geom::TOP | infinigen::furn::geom::BOTTOM | infinigen::furn::geom::SIDE);
                    let found = g.tris.iter().find(|t| (0..3).all(|k| ((t.v[0][k] + t.v[1][k] + t.v[2][k]) / 3.0 - c[k]).abs() < 1e-5));
                    match found { Some(t) => { if t.tags & (infinigen::furn::geom::BACK | infinigen::furn::geom::FRONT | infinigen::furn::geom::TOP | infinigen::furn::geom::BOTTOM | infinigen::furn::geom::SIDE) != m { bad += 1; } } None => bad += 1 }
                }
                if bad > 0 && std::env::var_os("PHTAGS").unwrap() == "2" { for (pi, poly) in polys.iter().enumerate() { let ix: Vec<usize> = poly.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect(); let c: [f64; 3] = std::array::from_fn(|k| (verts[ix[0]][k] + verts[ix[1]][k] + verts[ix[2]][k]) / 3.0); let f = g.tris.iter().find(|t| (0..3).all(|k| ((t.v[0][k] + t.v[1][k] + t.v[2][k]) / 3.0 - c[k]).abs() < 1e-5)); eprintln!("  py {:?} c {:?} rust {:?}", names.get(pi), c, f.map(|t| t.tags)); } }
                if bad > 0 { eprintln!("TAGDIFF {} fs {} inst {} bad {}/{}", class, fs, inst, bad, polys.len()); }
            }
            if std::env::var("PHV").ok().as_deref() == Some(class.as_str()) && d > 1e-12 { eprintln!("{} fs {} inst {} d {:e} lo {:?} {:?} hi {:?} {:?}", class, fs, inst, d, lo, g.lo, hi, g.hi); }
            if d < std::env::var("PHTOL").ok().and_then(|s| s.parse().ok()).unwrap_or(1e-4) { e.1 += 1; }
            e.2 = e.2.max(d);
        } else { e.3 += 1; }
    }
    println!("{:34} {:>5} {:>6} {:>10} {:>8}", "class", "n", "exact", "max|d|", "noport");
    for (c, (n, ok, mx, np)) in stats { println!("{:34} {:>5} {:>6} {:>12.3e} {:>8}", c, n, ok, mx, np); }
}
