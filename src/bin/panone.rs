use infinigen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s: u64 = a[1].parse().unwrap();
    let f = make_factory("PanFactory", s).unwrap();
    println!("{:?}", f.bounds_hint(0));
}
