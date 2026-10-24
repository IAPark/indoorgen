use infinigen::rng::Rng;
#[test]
fn matches_numpy() {
    // python: np.random.seed(42); [np.random.uniform(), np.random.randint(0,10), np.random.randint(1000000), np.random.randn(), np.random.choice(5,p=[.1,.2,.3,.2,.2])]
    let mut r = Rng::seeded(42);
    println!("{} {} {} {} {}", r.f64(), r.randint(0, 10), r.randint(0, 1000000), r.randn(), r.choice_p(&[0.1, 0.2, 0.3, 0.2, 0.2]));
}
