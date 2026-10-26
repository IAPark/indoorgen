//! coilrun in.json OUT.json dot_distance dot_depth dot_size radius smooth_factor : Rust make_coiled on a mesh json {verts, polys}
use indoorgen::mesh::Mesh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    m.pos = j["verts"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap() as f32 as f64, p[1].as_f64().unwrap() as f32 as f64, p[2].as_f64().unwrap() as f32 as f64]).collect();
    m.faces = j["polys"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let f = |i: usize| a[i].parse::<f64>().unwrap();
    let (dd, dep, ds, rad, sm) = (f(3), f(4), f(5), f(6), f(7));
    let o = indoorgen::assets::coil::make_coiled(&m, dd, dep, ds, rad / dd, sm);
    std::fs::write(&a[2], serde_json::json!({"verts": o.pos, "polys": o.faces}).to_string()).unwrap();
}
