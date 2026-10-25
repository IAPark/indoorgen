// rdtgt CASE.json TH : print remove_doubles targets (one per line)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let co: Vec<[f32; 3]> = j["verts"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32]).collect();
    let t = ig_kit::blops::remove_doubles_targets(&co, a[2].parse().unwrap());
    println!("{}", serde_json::to_string(&t).unwrap());
}
