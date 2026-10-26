use indoorgen::assets::hwexact::*;
fn main() {
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/tmp/stor_prims.json").unwrap()).unwrap();
    let t = &d["torus"];
    let g = bpy_torus(128, 12, 0.1, 0.01);
    let mut bad = 0;
    let mut maxd = 0f64;
    for (i, v) in t["v"].as_array().unwrap().iter().enumerate() {
        for k in 0..3 {
            let a = v[k].as_f64().unwrap();
            let b = g.v[i][k] as f64;
            if a != b { bad += 1; maxd = maxd.max((a - b).abs()); if bad < 5 { println!("{} {} {} {}", i, k, a, b); } }
        }
    }
    let pe = t["p"].as_array().unwrap().iter().zip(g.p.iter()).all(|(a, b)| a.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect::<Vec<_>>() == *b);
    println!("bad {} maxd {} polys_eq {}", bad, maxd, pe);
}
