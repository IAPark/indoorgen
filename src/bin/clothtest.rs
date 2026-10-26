//! clothtest out.json : drop a cloth strip over a box (bed-like) with the Rust cloth sim; writes {verts, polys} + out.json.parts
use indoorgen::assets::cloth::*;
fn grid(nx: usize, ny: usize, w: f64, h: f64, c: [f64; 3], off: usize) -> (Vec<[f64; 3]>, Vec<Vec<u32>>) {
    let mut p = vec![];
    for j in 0..=ny { for i in 0..=nx { p.push([c[0] + (j as f64 / ny as f64 - 0.5) * h, c[1] + (i as f64 / nx as f64 - 0.5) * w, c[2]]); } }
    let mut f = vec![];
    for j in 0..ny { for i in 0..nx { let a = (j * (nx + 1) + i + off) as u32; f.push(vec![a, a + 1, a + 1 + (nx as u32 + 1), a + (nx as u32 + 1)]); } }
    (p, f)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let env = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (w, h) = (env("W", 2.7), env("H", 0.7));
    let n = (w / 0.05) as usize;
    let m = (h / 0.05) as usize;
    let (pos, faces) = grid(n, m, w, h, [1.0, 0.0, 0.7], 0);
    // box with outward faces
    let (lo, hi) = ([0.0, -0.8, 0.0], [2.0, 0.8, 0.5]);
    let mut bp = vec![];
    for k in 0..8 { bp.push([if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }]); }
    let bf: Vec<Vec<u32>> = vec![vec![0, 2, 3, 1], vec![4, 5, 7, 6], vec![0, 1, 5, 4], vec![2, 6, 7, 3], vec![0, 4, 6, 2], vec![1, 3, 7, 5]];
    let col = Collider::new(&bp, &bf);
    let mut p = ClothParams::default();
    p.mass = 0.05; p.tension = 2.0; p.distance_min = env("DMIN", 5e-3); p.end_frame = 80;
    let t0 = std::time::Instant::now();
    let out = simulate(&pos, &faces, &p, &[&col], &Calib::default());
    eprintln!("sim {:.2}s", t0.elapsed().as_secs_f64());
    let (mut lo2, mut hi2) = ([1e9; 3], [-1e9; 3]);
    for q in &out { for k in 0..3 { lo2[k] = f64::min(lo2[k], q[k]); hi2[k] = f64::max(hi2[k], q[k]); } }
    eprintln!("cloth bounds {:?} {:?}", lo2, hi2);
    let mut verts = vec![]; let mut polys = vec![];
    for q in &bp { verts.push(serde_json::json!(q)); }
    for f in &bf { polys.push(serde_json::json!(f)); }
    for q in &out { verts.push(serde_json::json!(q)); }
    for f in &faces { polys.push(serde_json::json!(f.iter().map(|i| i + 8).collect::<Vec<_>>())); }
    std::fs::write(&a[1], serde_json::json!({"verts": verts, "polys": polys}).to_string()).unwrap();
    std::fs::write(format!("{}.parts", a[1]), format!("part 0..8 (8 verts)\npart 8..{} (x)\n", 8 + out.len())).unwrap();
}
