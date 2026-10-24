//! bevtest in.json width segs out.json [angle_deg] : run the bevelfull port on {"in": mesh}
use infinigen::mesh::Mesh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    for v in d["in"]["verts"].as_array().unwrap() {
        let v = v.as_array().unwrap();
        m.pos.push([v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]);
    }
    for f in d["in"]["polys"].as_array().unwrap() {
        m.faces.push(f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect());
    }
    let width: f64 = a[2].parse().unwrap();
    let segs: usize = a[3].parse().unwrap();
    let ang: f64 = a.get(5).map(|x| x.parse().unwrap()).unwrap_or(30.0);
    match infinigen::assets::bevelfull::bevel_angle(&m, width, segs, ang.to_radians(), std::env::var("WIDTH").is_ok()) {
        Ok(o) => {
            let verts: Vec<_> = o.pos.iter().map(|v| serde_json::json!([v[0], v[1], v[2]])).collect();
            let polys: Vec<_> = o.faces.iter().map(|f| serde_json::json!(f)).collect();
            std::fs::write(&a[4], serde_json::json!({"verts": verts, "polys": polys}).to_string()).unwrap();
            eprintln!("RSBEVEL {} {}", o.pos.len(), o.faces.len());
        }
        Err(e) => eprintln!("ERR {e}"),
    }
}
