//! treeshow OUT.glb [seed] [far]: the six Rust tree species (crates/ig_plants/src/rtree.rs) in a row on a ground plane, for the
//! previewer (tools/preview) -- trees iterated without Blender. `far` builds the far level of detail instead.
use indoorgen::assets::rtree::{self, Lod};
use indoorgen::mesh::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&a[1]);
    let seed: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let lod = if a.get(3).map_or(false, |s| s == "far") { Lod::Far } else { Lod::Near };
    let mut scene = Scene::default();
    let ground = scene.material(Material::diffuse("ground_grass", [0.09, 0.16, 0.04], 0.9));
    scene.add("ground#0", "Ground", Mesh::cuboid([-20.0, -20.0, -0.1], [120.0, 30.0, 0.0]), ground);
    let mut x = 0.0;
    for (i, sp) in rtree::ALL.iter().enumerate() {
        let t = std::time::Instant::now();
        let tr = rtree::build(*sp, seed * 100 + i as u64, lod);
        let (bm, lm) = rtree::materials(*sp, 0.0);
        let (bi, li) = (scene.material(bm), scene.material(lm));
        let shift = Mat4::translation([x, 0.0, 0.0]);
        let (mut b, mut l) = (tr.bark, tr.leaves);
        b.transform(&shift); l.transform(&shift);
        let centres: Vec<[f64; 3]> = tr.leaf_centres.iter().map(|c| shift.apply(*c)).collect();
        eprintln!("{sp:?}: height {:.1} m, bark {} faces, {} leaf cards, {:.2}s", tr.height, b.faces.len(), l.faces.len(), t.elapsed().as_secs_f64());
        scene.add(&format!("tree_{sp:?}#0"), "RTree", b, bi);
        scene.add_bent(&format!("tree_{sp:?}#1"), "RTree", l, li, centres);
        x += 14.0;
    }
    scene.write_glb(&out).unwrap();
}
