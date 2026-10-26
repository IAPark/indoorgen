fn main() {
    let s: u64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let f = indoorgen::assets::make_factory("OvenFactory", s).unwrap();
    let a = f.placeholder(0).unwrap();
    println!("{:?}", a.bounds());
}
