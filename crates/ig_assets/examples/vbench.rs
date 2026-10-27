// vbench CLASS N [REPS]: build N visual assets of CLASS (factory seeds 1..N), REPS times (memo off), print time per asset
fn main() {
    std::env::set_var("NOMEMO_ASSETS", "1");
    let a: Vec<String> = std::env::args().collect();
    let class = &a[1];
    let n: u64 = a[2].parse().unwrap();
    let reps: usize = a.get(3).map_or(1, |s| s.parse().unwrap());
    let t = std::time::Instant::now();
    let mut tris = 0usize;
    for _ in 0..reps {
        for s in 1..=n {
            let f = ig_assets::make_factory(class, s * 1_000_003).expect("no factory");
            let t1 = std::time::Instant::now();
            let x = ig_kit::api::scene_asset(&*f, s);
            tris = x.parts.iter().map(|p| p.mesh.faces.len()).sum::<usize>();
            if std::env::var_os("EACH").is_some() { eprintln!("  seed {s}: {:?} faces {tris} mat {}", t1.elapsed(), x.parts.first().map_or("", |p| p.mat.name.as_str())); }
        }
    }
    println!("{class}: {:.1} ms/asset (last asset {tris} faces)", t.elapsed().as_secs_f64() * 1000.0 / (n as f64 * reps as f64));
}
