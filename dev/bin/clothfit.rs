//! clothfit : run the Rust cloth solver on every python-dumped cloth_sim call of the cached Bed seeds (parallel) and report the similarity to Blender.
//! env: SEEDS="1 2 3 4 5 6" KINDS="sheet cover pillow wrapped"  + CL_* calibration vars. Output: one line per case + summary.
use indoorgen::assets::cloth::*;
use indoorgen::assets::pydump::*;
use rayon::prelude::*;

fn params_from_kw(kw: &serde_json::Value, pre: &PyMesh) -> ClothParams {
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
    if kw.get("vertex_group_mass").and_then(|v| v.as_str()).unwrap_or("") == "pin" { p.pin = pre.pin.clone(); }
    p
}

fn bounds(a: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in a { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    (lo, hi)
}

fn main() {
    let seeds: Vec<String> = std::env::var("SEEDS").unwrap_or("1 2 3 4 5 6".into()).split_whitespace().map(String::from).collect();
    let kinds: Vec<String> = std::env::var("KINDS").unwrap_or("sheet cover pillow wrapped".into()).split_whitespace().map(String::from).collect();
    let mut jobs = vec![];
    for s in &seeds {
        let dir = format!("{}/work/agents/rng/cache/d_BedFactory_{s}", std::env::var("HOME").unwrap());
        let d = PyDump::load(&dir);
        for k in &kinds {
            let key = match k.as_str() { "sheet" => "sim.make_sheet.0", "cover" => "sim.make_cover.0", "pillow" => "sim.create_asset.0", "wrapped" => "sim.make_wrapped.0", _ => continue };
            if d.has(&format!("{key}.kw")) { jobs.push((s.clone(), k.clone(), dir.clone(), key.to_string())); }
        }
    }
    let cal = Calib::default();
    let res: Vec<(String, String, f64, f64, f64, [f64; 3], [f64; 3], f64)> = jobs.par_iter().map(|(s, k, dir, key)| {
        let d = PyDump::load(dir);
        let pre = d.mesh(&format!("{key}.pre"));
        let post = d.mesh(&format!("{key}.post"));
        let kw = d.json(&format!("{key}.kw"));
        let p = params_from_kw(&kw, &pre);
        let ncol = kw["ncol"].as_u64().unwrap_or(0);
        let cols: Vec<Collider> = (0..ncol).map(|j| { let m = d.mesh(&format!("{key}.col{j}")); Collider::new(&m.v, &m.faces) }).collect();
        let cr: Vec<&Collider> = cols.iter().collect();
        let t = std::time::Instant::now();
        let out = simulate(&pre.v, &pre.faces, &p, &cr, &cal);
        let el = t.elapsed().as_secs_f64();
        let (mean, p95, mx) = surface_distance(&out, &pre.faces, &post.v, &post.faces);
        let (rl, rh) = bounds(&out);
        let (bl, bh) = bounds(&post.v);
        let dl = [(rl[0] - bl[0]).abs().max((rh[0] - bh[0]).abs()), (rl[1] - bl[1]).abs().max((rh[1] - bh[1]).abs()), (rl[2] - bl[2]).abs().max((rh[2] - bh[2]).abs())];
        let _ = mx;
        (s.clone(), k.clone(), mean, p95, mx, dl, [rh[2] - rl[2], bh[2] - bl[2], 0.0], el)
    }).collect();
    let mut tot: std::collections::BTreeMap<String, (f64, usize)> = Default::default();
    for (s, k, mean, p95, mx, dl, zr, el) in &res {
        println!("seed {s} {k:8}: surf mean {:.4} p95 {:.4} max {:.3} | bbox |d| x {:.3} y {:.3} z {:.3} | zrange rs {:.3} bl {:.3} | {:.1}s", mean, p95, mx, dl[0], dl[1], dl[2], zr[0], zr[1], el);
        let e = tot.entry(k.clone()).or_default();
        e.0 += mean; e.1 += 1;
    }
    let mut all = 0.0;
    for (k, (sum, n)) in &tot { println!("MEAN {k}: {:.4} over {n}", sum / *n as f64); all += sum / *n as f64; }
    println!("SCORE {:.4}", all);
}
