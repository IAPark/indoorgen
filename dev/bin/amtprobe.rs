use indoorgen::rng::Rng;
fn main() {
    // replicate Bathtub::new draws up to hole_radius, then scan
    for (seed, want) in [(1u64, 0.00446161307834657f64), (2, 0.003471448838589538), (6, 0.004682033219057195), (7, 0.0018702638204578278)] {
        let mut r = Rng::seeded(seed);
        r.uniform(1.5, 2.0); r.uniform(0.8, 1.0); r.uniform(0.55, 0.7);
        let alcove = r.choice_p(&[0.5, 0.5]) == 0;
        r.f64(); r.f64();
        if alcove { r.uniform(0.04, 0.08); } else { r.uniform(0.02, 0.04); }
        r.uniform(0.0, 0.2); r.uniform(0.0, 0.2); r.uniform(0.0, 0.1);
        r.uniform(0.2, 0.3); r.uniform(0.05, 0.1); r.uniform(0.02, 0.03); r.f64(); r.randint(0, 3);
        r.uniform(-0.1, 0.1); r.uniform(-0.2, 0.2);
        if alcove { r.randint(1, 3); }
        r.uniform(0.015, 0.02);
        for k in 0..12 {
            let mut c = r.clone();
            for _ in 0..k { c.f64(); }
            let v = c.uniform(0.001, 0.006);
            if (v - want).abs() < 1e-9 { println!("seed {seed}: k={k} alcove={alcove}"); }
        }
    }
}
