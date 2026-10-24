fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = &a[1];
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j["class"] != "KitchenIslandFactory" { continue; }
        let fs = j["fseed"].as_u64().unwrap();
        let f = infinigen::assets::make_factory("KitchenIslandFactory", fs).unwrap();
        let b = f.bounds_hint(0).unwrap();
        let v = j["verts"].as_array().unwrap();
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in v { for k in 0..3 { let x = p[k].as_f64().unwrap(); lo[k] = lo[k].min(x); hi[k] = hi[k].max(x); } }
        println!("{} cab {:?} {:?} py {:?} {:?}", fs, b.0, b.1, lo, hi);
    }
}
