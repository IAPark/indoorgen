use indoorgen::furn::assets::TableAssets;
use indoorgen::furn::solver::{Assets, PlaceholderKind};
use indoorgen::furn::solver::Usage;
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let usage = Usage::from_json(&prog["used_as"]);
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = indoorgen::assets::RealAssets { fallback };
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
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        let real = indoorgen::assets::make_factory(&class, fs).is_some();
        if let (true, Some(g)) = (real, assets.placeholder(&class, fs, inst, kind)) {
            let d = (0..3).map(|k| (g.lo[k] - lo[k]).abs().max((g.hi[k] - hi[k]).abs())).fold(0.0, f64::max);
            if let Some(f) = std::env::args().nth(2) { if class == f && d >= 1e-5 { println!("{} fs={} inst={} d={:.6} rust lo {:?} hi {:?} py lo {:?} hi {:?}", class, fs, inst, d, g.lo, g.hi, lo, hi); } }
            if d < 1e-4 { e.1 += 1; }
            e.2 = e.2.max(d);
        } else { e.3 += 1; }
    }
    println!("{:34} {:>5} {:>6} {:>10} {:>8}", "class", "n", "exact", "max|d|", "noport");
    for (c, (n, ok, mx, np)) in stats { println!("{:34} {:>5} {:>6} {:>10.5} {:>8}", c, n, ok, mx, np); }
}
