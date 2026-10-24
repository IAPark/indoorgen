//! bevsub in.json out.json [width segments]: the BEVEL modifier (python defaults) on a {"v","p"} mesh with the exact bmesh port
use infinigen::assets::bathkit::bevel_def;
use infinigen::mesh::Mesh;
use serde_json::Value;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    m.pos = j["v"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap() as f32 as f64, p[1].as_f64().unwrap() as f32 as f64, p[2].as_f64().unwrap() as f32 as f64] }).collect();
    m.faces = j["p"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let w: f64 = a.get(3).map_or(0.1, |s| s.parse().unwrap());
    let seg: i32 = a.get(4).map_or(2, |s| s.parse().unwrap());
    let b = bevel_def(&m, w, seg);
    std::fs::write(&a[2], serde_json::json!({"v": b.pos, "p": b.faces}).to_string()).unwrap();
    println!("{} {}", b.pos.len(), b.faces.len());
}
