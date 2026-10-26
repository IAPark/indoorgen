//! coiltest out.json [w d t dd ds dp] [stage] : make_coiled on a subdivided box; dumps stage meshes
use indoorgen::assets::coil::*;
use indoorgen::assets::towel::subdivide_edge_ring;
use indoorgen::mesh::Mesh;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p: Vec<f64> = if a.len() > 7 { a[2..8].iter().map(|x| x.parse().unwrap()).collect() } else { vec![0.6, 0.7, 0.3, 0.17, 0.01, 0.06] };
    let (w, d, t, dd, ds, dp) = (p[0], p[1], p[2], p[3], p[4], p[5]);
    let mut m = Mesh::cuboid([-w / 2.0, -d / 2.0, -t / 2.0], [w / 2.0, d / 2.0, t / 2.0]).welded(1e-9);
    let dims = [w, d, t];
    for i in 0..3 {
        m = subdivide_edge_ring(&m, (dims[i] / dd).ceil() as usize, i);
    }
    let stage = std::env::var("STAGE").unwrap_or("all".into());
    let out = match stage.as_str() {
        "grid" => m.clone(),
        "poke1" => poke(&m),
        "quads" => tris_to_quads(&poke(&m)),
        "poke3" => poke(&poke(&tris_to_quads(&poke(&m)))),
        _ => make_coiled(&m, dd, dp, ds, std::env::var("RU").map(|v| v.parse().unwrap()).unwrap_or(0.07), std::env::var("SF").map(|v| v.parse().unwrap()).unwrap_or(0.75)),
    };
    let verts: Vec<_> = out.pos.iter().map(|v| serde_json::json!(v)).collect();
    let polys: Vec<_> = out.faces.iter().map(|f| serde_json::json!(f)).collect();
    std::fs::write(&a[1], serde_json::json!({"verts": verts, "polys": polys}).to_string()).unwrap();
    println!("COIL {} verts {} faces", out.pos.len(), out.faces.len());
}
