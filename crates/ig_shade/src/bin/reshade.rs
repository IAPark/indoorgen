//! reshade IN.json OUT.json RULES.json [--seed N] [--explain NODE]: re-apply material rules to a generated house's sidecar, without generating it again. Put OUT.json
//! beside a link to the house's .glb and preview that. Needs a sidecar from a gen2 that writes each node's "ext".
use serde_json::{json, Value};
use std::collections::HashMap;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 { eprintln!("reshade IN.json OUT.json RULES.json [--seed N] [--explain NODE]"); std::process::exit(2) }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let mut side: Value = serde_json::from_slice(&std::fs::read(&a[0]).expect("sidecar")).expect("sidecar json");
    let cfg = ig_shade::rules::Config::parse(&std::fs::read_to_string(&a[2]).expect("rules")).unwrap_or_else(|e| { eprintln!("{}: {e}", a[2]); std::process::exit(2) });
    let house = opt("--seed").and_then(|s| s.parse().ok()).or_else(|| side["house_seed"].as_u64()).unwrap_or(0);
    let explain = opt("--explain");
    let mut graphs: Vec<Value> = vec![];
    let mut ids: HashMap<String, usize> = HashMap::new();
    let (mut n_changed, mut n_nodes) = (0, 0);
    for n in side["nodes"].as_array_mut().expect("nodes") {
        let (Some(spec0), Some(rgb0), Some(ext)) = (n.get("spec0").cloned().filter(|s| !s.is_null()), n.get("rgb0").cloned(), n.get("ext").cloned()) else { n.as_object_mut().unwrap().remove("graph"); continue };
        let f = |v: &Value, i: usize| v[i].as_f64().unwrap_or(0.0);
        let (rgb, e) = ([f(&rgb0, 0) as f32, f(&rgb0, 1) as f32, f(&rgb0, 2) as f32], [f(&ext, 0), f(&ext, 1), f(&ext, 2)]);
        let (name, mat, class) = (n["name"].as_str().unwrap_or("").to_string(), n["material"].as_str().unwrap_or("").to_string(), n["class"].as_str().unwrap_or("").to_string());
        let (spec, col, tr) = ig_shade::rules::resolve(&cfg, house, &spec0, &mat, &name, &class, rgb);
        if explain.as_deref().map_or(false, |x| name.starts_with(x)) { println!("{name} slot {} room {:?} style {:?}\n  was {} now {}\n  odds {:?}\n  rules {:?}", ig_shade::rules::slot_of(&mat), ig_shade::rules::room_of(&name), cfg.style_of(house), spec0["cls"], spec["cls"], tr.odds, tr.rules); }
        n_nodes += 1;
        if spec != spec0 || col != rgb { n_changed += 1; }
        let o = n.as_object_mut().unwrap();
        o.remove("graph");
        if let Some(g) = ig_shade::for_material(&spec, &mat, &name, e, col) {
            let id = *ids.entry(g.to_string()).or_insert_with(|| { graphs.push(g); graphs.len() - 1 });
            o.insert("graph".into(), json!(id));
        }
        o.insert("spec".into(), spec);
    }
    side["graphs"] = Value::Array(graphs);
    std::fs::write(&a[1], serde_json::to_vec(&side).unwrap()).expect("write");
    eprintln!("{n_changed} of {n_nodes} parts changed, style {:?}", cfg.style_of(house));
}
