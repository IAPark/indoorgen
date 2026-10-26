//! rsmesh PY.jsonl : for each (class,fseed,inst) print the Rust asset (parts concatenated) as jsonl {verts,polys}
use indoorgen::assets::*;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        let class = j["class"].as_str().unwrap().to_string();
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let f = match make_factory(&class, fs) { Some(f) => f, None => { println!("{{\"error\":\"nofactory\"}}"); continue; } };
        let a = if std::env::var("RSPH").is_ok() { f.placeholder(inst).unwrap_or_else(|| f.asset(inst)) } else { f.asset(inst) };
        let mut v: Vec<[f64; 3]> = vec![];
        let mut p: Vec<Vec<u32>> = vec![];
        for part in &a.parts {
            let off = v.len() as u32;
            v.extend(part.mesh.pos.iter().cloned());
            p.extend(part.mesh.faces.iter().map(|f| f.iter().map(|&i| i + off).collect::<Vec<_>>()));
        }
        println!("{}", serde_json::json!({"verts": v, "polys": p}));
    }
}
