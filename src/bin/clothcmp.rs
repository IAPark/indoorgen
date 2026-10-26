//! clothcmp DUMPDIR KEY : run the Rust cloth solver on a python-dumped cloth_sim call (KEY e.g. sim.make_sheet.0) and compare with Blender's result.
use indoorgen::assets::cloth::*;
use indoorgen::assets::pydump::*;
fn stats(a: &[[f64; 3]], b: &[[f64; 3]]) -> (f64, f64) {
    let mut s = 0.0;
    let mut m: f64 = 0.0;
    for (p, q) in a.iter().zip(b) {
        let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        s += d;
        m = m.max(d);
    }
    (s / a.len() as f64, m)
}
fn bounds(a: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in a {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = PyDump::load(&a[1]);
    let key = &a[2];
    let pre = d.mesh(&format!("{key}.pre"));
    let post = d.mesh(&format!("{key}.post"));
    let kw = d.json(&format!("{key}.kw"));
    let mut p = ClothParams::default();
    let f = |k: &str| kw.get(k).and_then(|v| v.as_f64());
    if let Some(x) = f("mass") { p.mass = x; }
    if let Some(x) = f("tension_stiffness") { p.tension = x; }
    if let Some(x) = f("distance_min") { p.distance_min = x; }
    if let Some(x) = f("end_frame") { p.end_frame = x as u32; }
    if let Some(x) = f("gravity") { p.gravity = x; }
    if let Some(x) = f("uniform_pressure_force") { p.pressure = x; }
    p.use_pressure = kw.get("use_pressure").and_then(|v| v.as_bool()).unwrap_or(false);
    p.use_self_collision = kw.get("use_self_collision").and_then(|v| v.as_bool()).unwrap_or(false);
    let pinned = kw.get("vertex_group_mass").and_then(|v| v.as_str()).unwrap_or("") == "pin";
    if pinned { p.pin = pre.pin.clone(); }
    let ncol = kw["ncol"].as_u64().unwrap_or(0);
    let cols: Vec<Collider> = (0..ncol).map(|j| { let m = d.mesh(&format!("{key}.col{j}")); Collider::new(&m.v, &m.faces) }).collect();
    let only: Option<usize> = std::env::var("ONLYCOL").ok().and_then(|s| s.parse().ok());
    let cr: Vec<&Collider> = cols.iter().enumerate().filter(|(i, _)| only.map(|o| o == *i || o == 99 && false).unwrap_or(true)).map(|(_, c)| c).collect();
    if std::env::var("NOPRESS").is_ok() { p.use_pressure = false; }
    let cal = Calib::default();
    let t0 = std::time::Instant::now();
    let out = simulate(&pre.v, &pre.faces, &p, &cr, &cal);
    let (mean, max) = stats(&out, &post.v);
    let (mean0, _) = stats(&pre.v, &post.v);
    let (rl, rh) = bounds(&out);
    let (bl, bh) = bounds(&post.v);
    if let Ok(path) = std::env::var("DUMP") { let j = serde_json::json!({"pre": pre.v, "rust": out, "blender": post.v}); std::fs::write(path, j.to_string()).unwrap(); }
    println!("{key}: sim {:.2}s  |rust-blender| mean {:.4} max {:.4}  (pre-blender mean {:.4})", t0.elapsed().as_secs_f64(), mean, max, mean0);
    let (sm, sp, sx) = surface_distance(&out, &pre.faces, &post.v, &post.faces);
    println!("  surface distance rust<->blender: mean {:.4} p95 {:.4} max {:.4}", sm, sp, sx);
    println!("  bbox rust {:.3?} {:.3?}\n  bbox blnd {:.3?} {:.3?}", rl, rh, bl, bh);
}
