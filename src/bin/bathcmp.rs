use infinigen::assets::bathroom::Bathtub;
use infinigen::assets::bathtub;
use infinigen::assets::stagecmp::surface_dist;
use infinigen::rng::Rng;
use serde_json::Value;

fn load(j: &Value, k: &str) -> Option<(Vec<[f64; 3]>, Vec<Vec<u32>>)> {
    let s = j.get(k)?;
    let v = s["v"].as_array()?.iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()] }).collect();
    let f = s["p"].as_array()?.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    Some((v, f))
}

fn report(name: &str, got: &infinigen::mesh::Mesh, j: &Value, key: &str) {
    if let Ok(d) = std::env::var("DUMP") {
        let js = serde_json::json!({"v": got.pos, "p": got.faces});
        std::fs::write(format!("{d}/{name}.json"), js.to_string()).unwrap();
    }
    let Some((v, f)) = load(j, key) else { println!("{name}: no stage {key}"); return };
    let ok_counts = format!("verts {}/{} faces {}/{}", got.pos.len(), v.len(), got.faces.len(), f.len());
    let vn = infinigen::assets::bathkit::nn_max(&got.pos, &v).max(infinigen::assets::bathkit::nn_max(&v, &got.pos));
    print!("vnn {vn:.2e} ");
    match surface_dist(&got.pos, &got.faces, &v, &f, 3000) {
        Some((mean, mx)) => println!("{name:12} {ok_counts} mean {mean:.3e} max {mx:.3e} {}", if mean < 1e-5 && mx < 1e-4 { "OK" } else { "FAIL" }),
        None => println!("{name:12} {ok_counts} (empty)"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let inst: u64 = a[2].parse().unwrap();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&a[3]).unwrap()).unwrap();
    let mut rng = Rng::seeded(seed);
    let mut t = Bathtub::new(&mut rng);
    t.fseed = seed;
    let mut r = infinigen::assets::inst_rng(seed, inst);
    let t0 = std::time::Instant::now();
    let st = bathtub::build(&t, &mut r);
    println!("build {:.2}s alcove={}", t0.elapsed().as_secs_f64(), t.alcove);
    for (name, key, m) in st.stages.iter() {
        report(name, m, &j, key);
    }
}
