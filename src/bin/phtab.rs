//! phtab oracle.jsonl [Class,...] : placeholder bounds check for my classes; Coffee/Side unseeded dims are fitted from the oracle bbox
use indoorgen::furn::assets::TableAssets;
use indoorgen::furn::solver::{Assets, PlaceholderKind, Usage};
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let filter: Vec<String> = a.get(2).map(|s| s.split(',').map(|x| x.to_string()).collect()).unwrap_or_default();
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let usage = Usage::from_json(&prog["used_as"]);
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = indoorgen::assets::RealAssets { fallback };
    let mut stats: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    for line in std::fs::read_to_string(&a[1]).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j.get("kind").is_some() { continue; }
        let class = j["class"].as_str().unwrap().to_string();
        if !filter.is_empty() && !filter.contains(&class) { continue; }
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in j["verts"].as_array().unwrap() { for k in 0..3 { let x = v[k].as_f64().unwrap(); lo[k] = lo[k].min(x); hi[k] = hi[k].max(x); } }
        let kind = if usage.has_usage(&class, "RealPlaceholder") { PlaceholderKind::Real } else if usage.has_usage(&class, "PlaceholderBBox") { PlaceholderKind::BboxOfPlaceholder } else { PlaceholderKind::BboxOfAsset };
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2]];
        let measure = |u: &Vec<f64>| -> Option<([f64; 3], [f64; 3])> { indoorgen::assets::set_unseeded(Some(u.clone())); let g = assets.placeholder(&class, fs, inst, kind)?; Some((g.lo, g.hi)) };
        let g = match class.as_str() {
            "SideTableFactory" | "CoffeeTableFactory" => {
                let side = class == "SideTableFactory";
                let mut u = if side { vec![1.0, 1.0] } else { vec![1.2, 0.7, 0.45] };
                for _ in 0..30 {
                    let (l, h) = measure(&u).unwrap();
                    let e = [h[0] - l[0], h[1] - l[1], h[2]];
                    if side {
                        u[0] *= ext[0] / e[0];
                        u[1] *= ext[2] / e[2];
                    } else {
                        for k in 0..3 { u[k] *= ext[k] / e[k]; }
                    }
                }
                measure(&u).map(|(l, h)| indoorgen::furn::geom::ObjGeom::cuboid(l, h, 0))
            }
            _ => { indoorgen::assets::set_unseeded(None); assets.placeholder(&class, fs, inst, kind) }
        };
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        if let Some(g) = g {
            let d = (0..3).map(|k| (g.lo[k] - lo[k]).abs().max((g.hi[k] - hi[k]).abs())).fold(0.0, f64::max);
            if d < 1e-5 { e.1 += 1; } else if std::env::var("VERBOSE").is_ok() { println!("BAD {class} {fs} {inst} d={d:.5} ext_py {:?} lo {:?} hi {:?} rs lo {:?} hi {:?}", ext, lo, hi, g.lo, g.hi); }
            e.2 = e.2.max(d);
        }
    }
    println!("{:34} {:>5} {:>6} {:>10}", "class", "n", "exact", "max|d|");
    for (c, (n, ok, mx)) in stats { println!("{:34} {:>5} {:>6} {:>10.6}", c, n, ok, mx); }
}
