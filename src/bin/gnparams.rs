//! gnparams CLASS FSEED [INST] : print the parameters sampled by the Rust sampler of a gn-ported asset (see assets/gnports.rs)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (fs, inst): (u64, u64) = (a[2].parse().unwrap(), a.get(3).map(|x| x.parse().unwrap()).unwrap_or(0));
    let p = match a[1].as_str() {
        "TriangleShelfFactory" => infinigen::assets::gnports::triangle_shelf_params(fs, inst),
        _ => panic!("unknown class"),
    };
    let mut k: Vec<_> = p.iter().collect();
    k.sort_by(|x, y| x.0.cmp(y.0));
    for (n, v) in k {
        println!("{} {:?}", n, v);
    }
}
