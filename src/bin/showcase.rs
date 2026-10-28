//! showcase CLASS N out.glb [fseed0]: lay out N instances of a factory in a row (for eyeballing ports)
use indoorgen::assets::*;
use indoorgen::furn::assets::TableAssets;
use indoorgen::mesh::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let class = &a[1];
    let n: usize = a[2].parse().unwrap();
    let out = std::path::PathBuf::from(&a[3]);
    let f0: u64 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = RealAssets { fallback };
    let mut scene = Scene::default();
    let floor = scene.material(Material::diffuse("floor", [0.6, 0.6, 0.6], 0.9));
    let mut x = 0.0;
    for i in 0..n {
        // constructor draws (bed sheet type, ...) as the solver sets them: some ports (BedFactory) only take this path
        indoorgen::assets::set_unseeded(indoorgen::assets::ctor_draws::ctor_draws(class, &mut indoorgen::rng::Rng::seeded(f0 + i as u64)));
        let mut asset = assets.visual(class, f0 + i as u64, 0);
        // FRONT=1: turn the asset's front (+x) toward the showcase camera (-y)
        if std::env::var("FRONT").is_ok() {
            let r = Mat4::rot_z(-std::f64::consts::FRAC_PI_2);
            for p in asset.parts.iter_mut() {
                p.mesh.transform(&r);
            }
        }
        let (lo, hi) = asset.bounds();
        let w = hi[0] - lo[0];
        let shift = Mat4::translation([x - lo[0], 0.0, 0.0]);
        for (pi_, p) in asset.parts.iter().enumerate() {
            let mut m = p.mesh.clone();
            m.transform(&shift);
            let mi = scene.material(p.mat.clone());
            scene.add(&format!("{class}{i}#{pi_}"), class, m, mi);
        }
        eprintln!("{class} seed {} dims {:.2} x {:.2} x {:.2}", f0 + i as u64, hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        x += w.max(0.3) + 0.3;
    }
    let _ = floor; // no floor plane: reference showcase renders have none either
    scene.write_glb(&out).unwrap();
}
