fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s: u64 = a[1].parse().unwrap();
    let m = indoorgen::assets::taps::tap_mesh(s, 0);
    println!("verts {} faces {}", m.pos.len(), m.faces.len());
}
