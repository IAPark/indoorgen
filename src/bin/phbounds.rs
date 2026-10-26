fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = indoorgen::assets::make_factory(&a[1], a[2].parse().unwrap()).unwrap();
    let p = f.placeholder(0).unwrap();
    println!("{:?}", p.bounds());
}
