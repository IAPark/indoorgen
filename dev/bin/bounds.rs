use indoorgen::furn::program::*;
use indoorgen::dhash::HashMap;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/home_program.json")).unwrap()).unwrap();
    let variant = &args[1];
    let mut params: Params = HashMap::default();
    for (k, v) in [("furniture_fullness_pct", 0.75), ("obj_interior_obj_pct", 0.75), ("obj_on_storage_pct", 0.75), ("obj_on_nonstorage_pct", 0.6), ("painting_area_per_room_area", 1.5), ("sofa_back_margin", 0.2), ("sofa_side_margin", 0.2)] {
        params.insert(k.into(), v);
    }
    let prog = Program::load(&data, variant, &params);
    let bounds = prog.constraint_bounds();
    let out: Vec<_> = bounds.iter().map(|b| serde_json::json!({"domain": domain_json(&b.domain), "low": b.low, "high": b.high})).collect();
    println!("{}", serde_json::to_string(&out).unwrap());
}
