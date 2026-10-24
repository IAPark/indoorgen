use infinigen::assets::doors::apply_draws;
use infinigen::assets::matdraw::init;
use infinigen::rng::Rng;
fn main() {
    let names = ["metal.MetalBasic", "metal.BrushedMetal", "metal.GalvanizedMetal", "metal.GrainedMetal", "metal.HammeredMetal", "metal.BrushedBlackMetal", "wood.Wood", "wood.WhitePlywood", "wood.BlackPlywood", "wood.BlondePlywood", "plastic.Plastic", "plastic.PlasticRough", "plastic.PlasticTranslucent", "ceramic.Glass"];
    let mut out = serde_json::Map::new();
    for n in names {
        for s in 1..7u64 {
            let mut r = Rng::seeded(s);
            init(&mut r, n);
            let p = r.f64();
            let mut r = Rng::seeded(s);
            init(&mut r, n);
            apply_draws(&mut r, n);
            let q = r.f64();
            for v in ["empty_mc", "obj", "glass"] {
                out.insert(format!("{n}/{s}/{v}"), serde_json::json!([p, q]));
            }
        }
    }
    std::fs::write("/tmp/f_apply_rs.json", serde_json::Value::Object(out).to_string()).unwrap();
}
