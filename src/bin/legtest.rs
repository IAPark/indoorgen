//! legtest IN.json OUT.json : apply DISPLACE (legacy texture) to a mesh. IN: {"v","f","kind","stype","wave","scale","strength","mid","depth","hard","basis"}
use indoorgen::assets::legtex::*;
use indoorgen::mesh::Mesh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    m.pos = j["v"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect();
    m.faces = j["f"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let st = j["stype"].as_i64().unwrap_or(0) as i32;
    let kind = match j["kind"].as_str().unwrap() {
        "STUCCI" => TexKind::Stucci(st),
        "MARBLE" => TexKind::Marble(st, j["wave"].as_i64().unwrap_or(0) as i32),
        _ => TexKind::Clouds,
    };
    let mut t = Tex::new(kind, j["scale"].as_f64().unwrap());
    t.noisedepth = j["depth"].as_i64().unwrap_or(2) as i32;
    t.hard = j["hard"].as_bool().unwrap_or(false);
    t.basis = j["basis"].as_i64().unwrap_or(0) as i32;
    displace(&mut m, &t, j["strength"].as_f64().unwrap(), j["mid"].as_f64().unwrap());
    std::fs::write(&a[2], serde_json::to_string(&m.pos).unwrap()).unwrap();
}
