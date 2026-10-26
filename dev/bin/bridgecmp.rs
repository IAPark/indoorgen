//! bridgecmp in.json out.json : run the Rust bridge_two_loops on a {v,p,loops,cuts,shape} case (compare with tools/bridgetest.py)
use indoorgen::mesh::Mesh;
use serde_json::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    m.pos = j["v"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()] }).collect();
    m.faces = j["p"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let lp = |i: usize| -> Vec<u32> { j["loops"][i].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect() };
    indoorgen::assets::bridge::bridge_two_loops(&mut m, lp(0), lp(1), j["cuts"].as_u64().unwrap() as usize, j["shape"].as_f64().unwrap());
    std::fs::write(&a[2], serde_json::json!({"v": m.pos, "p": m.faces}).to_string()).unwrap();
    println!("{} {}", m.pos.len(), m.faces.len());
}
