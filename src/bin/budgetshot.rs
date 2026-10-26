//! budgetshot OUT_PREFIX Class:seed ... : builds the assets side by side (1.5 m apart along x) into OUT_PREFIX_full.glb and
//! OUT_PREFIX_budget.glb (per-object triangle budget applied) for before/after close-ups with tools/preview.sh.
use indoorgen::assets::*;
use indoorgen::mesh::{Material, Mesh, Scene};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut full = Scene::default();
    let floor = full.material(Material::diffuse("floor", [0.6, 0.55, 0.5], 0.8));
    full.add("floor", "room", Mesh::cuboid([-1.0, -2.0, -0.05], [1.5 * a.len() as f64 + 1.0, 4.0, 0.0]), floor);
    for (i, spec) in a[2..].iter().enumerate() {
        let (class, seed) = spec.split_once(':').unwrap();
        let fs: u64 = seed.parse().unwrap();
        set_unseeded(indoorgen::assets::ctor_draws::ctor_draws(class, &mut indoorgen::rng::Rng::seeded(fs)));
        let f = make_factory(class, fs).expect("no factory");
        let mut asset = scene_asset(&*f, 0);
        let (lo, hi) = asset.bounds();
        let (dx, dz) = (1.5 * i as f64 - (lo[0] + hi[0]) / 2.0, -lo[2]);
        for (pi, p) in asset.parts.iter().enumerate() {
            let mut m = p.mesh.clone();
            m.transform(&indoorgen::mesh::Mat4::translation([dx, -(lo[1] + hi[1]) / 2.0, dz]));
            let mi = full.material(p.mat.clone());
            full.add(&format!("{class}_{seed}#{pi}"), class, m, mi);
        }
        eprintln!("{spec}: bounds {:?} {:?}", lo, hi);
        let _ = &mut asset;
    }
    let tris = |s: &Scene| s.nodes.iter().map(|n| n.mesh.faces.iter().map(|f| f.len().saturating_sub(2)).sum::<usize>()).sum::<usize>();
    let mut bud = Scene { nodes: full.nodes.clone(), materials: full.materials.clone(), ..Default::default() };
    let (t0, t1, k) = indoorgen::budget::apply(&mut bud);
    eprintln!("tris {} -> {} ({} simplified)", t0, t1, k);
    let _ = tris;
    full.write_glb(std::path::Path::new(&format!("{}_full.glb", a[1]))).unwrap();
    bud.write_glb(std::path::Path::new(&format!("{}_budget.glb", a[1]))).unwrap();
}
