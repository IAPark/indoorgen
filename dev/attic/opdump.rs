//! opdump SEED out.json : rooms-only generation, dumps constants + cutters and the openings pieces (world coordinates)
use indoorgen::furn::assets::TableAssets;
use indoorgen::pipeline::*;
use indoorgen::rooms::solidify::CutKind;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/home_program.json")).unwrap()).unwrap();
    let assets = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/asset_bbox_table.json"));
    let g = generate(&Options { seed, restrict_room: None, steps_large: 0, steps_medium: 0, steps_small: 0 }, &assets, &prog);
    let c = &g.c;
    let cut: Vec<_> = g.cutters.iter().map(|k| serde_json::json!({"kind": match k.kind { CutKind::Door => "door", CutKind::Window => "window", CutKind::Entrance => "entrance" }, "loc": k.loc, "rot_z": k.rot_z, "dims": k.dims, "rooms": k.rooms})).collect();
    let t0 = std::time::Instant::now();
    let ps = indoorgen::openings::make_openings(c, &g.cutters, seed);
    eprintln!("make_openings {:?}", t0.elapsed());
    let pieces: Vec<_> = ps.iter().map(|p| {
        serde_json::json!({"name": p.name, "verts": p.mesh.pos, "polys": p.mesh.faces, "cls": p.mat.spec})
    }).collect();
    let consts = serde_json::json!({"wall_thickness": c.wall_thickness, "wall_height": c.wall_height, "door_width": c.door_width, "door_size": c.door_size, "door_margin": c.door_margin, "n_stories": c.n_stories});
    std::fs::write(&a[2], serde_json::json!({"consts": consts, "cutters": cut, "pieces": pieces}).to_string()).unwrap();
    eprintln!("cutters {} pieces {}", g.cutters.len(), pieces.len());
}
