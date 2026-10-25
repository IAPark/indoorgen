//! coilx in.json out.json radius : bevel stage
use ig_seat::mesh::Mesh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut m = Mesh::new();
    m.pos = j["verts"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect();
    m.faces = j["polys"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    if let Some(es) = j["edges"].as_array() {
        *ig_seat::coilx::IN_EDGES.lock().unwrap() = es.iter().map(|e| [e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32]).collect();
    }
    if std::env::var("STAGE").map(|v| v == "scaleonly").unwrap_or(false) {
        let tip: Vec<usize> = j["sel"].as_array().unwrap().iter().enumerate().filter(|(_, b)| b.as_bool().unwrap()).map(|(i, _)| i).collect();
        let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let st = ig_seat::coilx::Stage { pos, faces: m.faces.clone(), tip, edges: vec![] };
        let o = ig_seat::coilx::scale_stage(&st, (a[3].parse::<f64>().unwrap() / a[4].parse::<f64>().unwrap()) as f32);
        let verts: Vec<_> = o.pos.iter().map(|p| serde_json::json!([p[0] as f64, p[1] as f64, p[2] as f64])).collect();
        std::fs::write(&a[2], serde_json::json!({"verts": verts, "polys": o.faces}).to_string()).unwrap();
        return;
    }
    if std::env::var("STAGE").map(|v| v == "full").unwrap_or(false) {
        let f = |i: usize| a[i].parse::<f64>().unwrap();
        // in.json out(unused) dd dep ds ru sf
        let ed: Vec<[u32; 2]> = ig_seat::coilx::IN_EDGES.lock().unwrap().clone();
        let o = ig_seat::coilx::make_coiled_f32(&m, f(3), f(4), f(5), f(6), f(7), if ed.is_empty() { None } else { Some(&ed) });
        println!("{}", o.map(|m| m.pos.len()).unwrap_or(0));
        return;
    }
    if std::env::var("STAGE").map(|v| v == "triq").unwrap_or(false) {
        // triangulate + smooth(fac a[3]) on the given mesh (python 'scale' stage); writes tri to a[2] and final to a[2].final
        let t = if std::env::var("TRI_BEAUTY").is_ok() { ig_seat::coil::triangulate(&m) } else { ig_seat::coilx::triangulate_shortest(&m) };
        let verts: Vec<_> = t.pos.iter().map(|p| serde_json::json!(p)).collect();
        std::fs::write(&a[2], serde_json::json!({"verts": verts, "polys": t.faces}).to_string()).unwrap();
        let f = ig_seat::coil::smooth(&t, a[3].parse().unwrap(), 5);
        let verts: Vec<_> = f.pos.iter().map(|p| serde_json::json!(p)).collect();
        std::fs::write(format!("{}.final", a[2]), serde_json::json!({"verts": verts, "polys": f.faces}).to_string()).unwrap();
        return;
    }
    if std::env::var("STAGE").map(|v| v == "extronly").unwrap_or(false) {
        let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let st = ig_seat::coilx::Stage { pos, faces: m.faces.clone(), tip: vec![], edges: vec![] };
        let o = ig_seat::coilx::extrude_stage(&st, a[3].parse::<f64>().unwrap() as f32);
        let verts: Vec<_> = o.pos.iter().map(|p| serde_json::json!([p[0] as f64, p[1] as f64, p[2] as f64])).collect();
        std::fs::write(&a[2], serde_json::json!({"verts": verts, "polys": o.faces}).to_string()).unwrap();
        return;
    }
    let r: f64 = a[3].parse().unwrap();
    let s = if std::env::var("STAGE").map(|v| v == "poke3").unwrap_or(false) { ig_seat::coilx::poke3_stage(&m) } else { let b = ig_seat::coilx::bevel_stage(&m, r as f32).unwrap(); if std::env::var("STAGE").map(|v| v == "extr").unwrap_or(false) { ig_seat::coilx::extrude_stage(&b, 0.06) } else if std::env::var("STAGE").map(|v| v == "scale").unwrap_or(false) { let e = ig_seat::coilx::extrude_stage(&b, 0.06); ig_seat::coilx::scale_stage(&e, (0.01f64 / r) as f32) } else { b } };
    let verts: Vec<_> = s.pos.iter().map(|p| serde_json::json!([p[0] as f64, p[1] as f64, p[2] as f64])).collect();
    std::fs::write(&a[2], serde_json::json!({"verts": verts, "polys": s.faces, "edges": s.edges}).to_string()).unwrap();
}
