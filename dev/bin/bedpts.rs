//! bedpts SEED: prints the leg / back post vertices of BedFrameFactory(SEED) as JSON (float32 values) to compare with tools/stagedump.py dumps.
use indoorgen::assets::bed::BedSpec;
fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let s = BedSpec::frame(seed);
    let f = |ls: Vec<Vec<[f64; 3]>>| ls.into_iter().map(|l| l.into_iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect::<Vec<_>>()).collect::<Vec<_>>();
    println!("{}", serde_json::json!({"leg_decor": s.leg_decor, "wrapped": s.leg_decor_wrapped, "bed_back": s.c.bed_back, "has_all_legs": s.has_all_legs, "legs": f(s.legs_pub()), "backs": f(s.c.make_backs())}));
}
