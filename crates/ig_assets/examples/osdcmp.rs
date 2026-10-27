// osdcmp CLASS N: max vertex difference between the PrimvarRefiner and stencil OpenSubdiv paths
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: u64 = a[2].parse().unwrap();
    let (mut worst, mut same, mut tot) = (0f64, 0usize, 0usize);
    for s in 1..=n {
        let f = ig_assets::make_factory(&a[1], s * 1_000_003).unwrap();
        std::env::remove_var("OSD_STENCIL");
        let x = ig_kit::api::scene_asset(&*f, s);
        std::env::set_var("OSD_STENCIL", "1");
        let y = ig_kit::api::scene_asset(&*f, s);
        let (lo, hi) = x.bounds();
        let size = (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
        let (ylo, yhi) = y.bounds();
        let bd = (0..3).map(|d| (lo[d] - ylo[d]).abs().max((hi[d] - yhi[d]).abs())).fold(0.0, f64::max);
        let nx: usize = x.parts.iter().map(|p| p.mesh.pos.len()).sum();
        let ny: usize = y.parts.iter().map(|p| p.mesh.pos.len()).sum();
        if nx != ny { println!("  seed {s}: verts {nx} vs {ny}, bounds differ by {:.2e} of the size", bd / size); continue }
        for (p, q) in x.parts.iter().zip(&y.parts) {
            for (u, v) in p.mesh.pos.iter().zip(&q.mesh.pos) {
                let d = (0..3).map(|k| (u[k] - v[k]).abs()).fold(0.0, f64::max);
                worst = worst.max(d / size);
                tot += 1;
                if d == 0.0 { same += 1 }
            }
        }
    }
    println!("{}: {tot} verts, {:.2}% bit-identical, max diff {:.2e} of the object size", a[1], 100.0 * same as f64 / tot as f64, worst);
}
