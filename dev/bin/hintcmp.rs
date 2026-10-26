//! hintcmp CLASS FILE : compare Factory::bounds_hint against "APP seed bbox [lo] [hi]" lines
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut worst: f64 = 0.0;
    for line in std::fs::read_to_string(&a[2]).unwrap().lines() {
        let nums: Vec<f64> = line.replace(['[', ']', ','], " ").split_whitespace().filter_map(|t| t.parse().ok()).collect();
        let seed = nums[0] as u64;
        let (lo, hi) = (&nums[1..4], &nums[4..7]);
        let f = indoorgen::assets::make_factory(&a[1], seed).unwrap();
        let (rl, rh) = match f.bounds_hint(0) { Some(b) => b, None => { println!("seed {seed}: no hint"); continue; } };
        let d = (0..3).map(|k| (rl[k] - lo[k]).abs().max((rh[k] - hi[k]).abs())).fold(0.0, f64::max);
        worst = worst.max(d);
        if d > 1e-5 { println!("seed {seed}: err {d:.2e} py {:?} {:?} rs {:?} {:?}", lo, hi, rl, rh); }
    }
    println!("{} worst {:.2e}", a[1], worst);
}
