use indoorgen::assets::bathkit::nn_max;
use indoorgen::assets::cookware::Tv;
use indoorgen::rng::Rng;
use serde_json::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let mut rng = Rng::seeded(seed);
    let mut t = Tv::new(&mut rng, false);
    t.fseed = seed;
    let mut r = indoorgen::assets::inst_rng(seed, 0);
    let b = indoorgen::assets::tvx::build(&t, &mut r);
    for (name, key, m) in b.stages.iter() {
        let v: Vec<[f64; 3]> = j[key.as_str()]["v"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()] }).collect();
        println!("{name}: verts {}/{} vnn {:.2e}", m.pos.len(), v.len(), nn_max(&m.pos, &v).max(nn_max(&v, &m.pos)));
        if let Ok(d) = std::env::var("DUMP") {
            std::fs::write(format!("{d}/tv_{name}.json"), serde_json::json!({"v": m.pos, "p": m.faces}).to_string()).unwrap();
        }
    }
}
