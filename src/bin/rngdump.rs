fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let n: usize = a[2].parse().unwrap();
    let mut r = infinigen::rng::Rng::seeded(seed);
    for i in 0..n {
        println!("{i} {:.6}", r.f64());
    }
}
