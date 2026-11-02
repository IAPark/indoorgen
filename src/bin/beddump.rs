//! beddump SEED DIR [INST]: a bed's parts (DIR/part.K.json {verts, polys}) and its cloth jobs (DIR/jobs.json), for tools/bedref.py
use indoorgen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (fs, dir) = (a[1].parse::<u64>().unwrap(), &a[2]);
    let inst: u64 = a.get(3).map_or(0, |s| s.parse().unwrap());
    std::fs::create_dir_all(dir).unwrap();
    set_unseeded(indoorgen::assets::ctor_draws::ctor_draws("BedFactory", &mut indoorgen::rng::Rng::seeded(fs)));
    let f = make_factory("BedFactory", fs).expect("no factory");
    let asset = f.asset(inst);
    let id = indoorgen::mesh::Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    std::fs::write(format!("{dir}/jobs.json"), serde_json::to_vec(&asset.cloth_json("bed", &id)).unwrap()).unwrap();
    for (k, p) in asset.parts.iter().enumerate() {
        std::fs::write(format!("{dir}/part.{k}.json"), serde_json::json!({"verts": p.mesh.pos, "polys": p.mesh.faces, "mat": p.mat.name}).to_string()).unwrap();
    }
}
