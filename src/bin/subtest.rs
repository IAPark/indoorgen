use infinigen::assets::subsurf::subsurf;
use infinigen::mesh::Mesh;
fn main() {
    let lv: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let c = Mesh::cuboid([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).welded(1e-9);
    let r = subsurf(&c, lv, false);
    println!("{}", serde_json::to_string(&r.pos).unwrap());
}
