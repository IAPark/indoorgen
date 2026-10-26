// messbench CLASS N : build time / size of the first N factory seeds (clutter asset cost survey)
use indoorgen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: u64 = a[2].parse().unwrap();
    for s in 0..n {
        let t = std::time::Instant::now();
        let tc = std::time::Instant::now();
        let dr = indoorgen::assets::ctor_draws::ctor_draws(&a[1], &mut indoorgen::rng::Rng::seeded(s));
        let dt = tc.elapsed().as_secs_f64();
        indoorgen::assets::ctor_draws::set_unseeded(dr);
        let t = std::time::Instant::now();
        let Some(f) = make_factory(&a[1], s) else { println!("{} none", a[1]); return };
        let asset = scene_asset(&*f, s + 1000);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        let mut nv = 0;
        for p in &asset.parts {
            nv += p.mesh.pos.len();
            for q in &p.mesh.pos { for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); } }
        }
        println!("{} seed {s}: ctor {dt:.3}s build {:.3}s parts {} verts {nv} size {:.2} {:.2} {:.2} lo z {:.3}", a[1], t.elapsed().as_secs_f64(), asset.parts.len(), hi[0]-lo[0], hi[1]-lo[1], hi[2]-lo[2], lo[2]);
    }
}
