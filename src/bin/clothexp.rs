//! clothexp CASE FRAME : run the Rust cloth solver on the small calibration scenes of tools/clothexp2.py and compare with Blender.
//! CASE: hang_k2_m005 | hang_k15_m03 | hang_k30_m005 | ... | drop | drop_nodamp
use indoorgen::assets::cloth::*;
use indoorgen::assets::clothes::grid;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let case = a[1].clone();
    let frame: u32 = a[2].parse().unwrap();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::env::var("EXP2").unwrap_or("/Users/lume/work/agents/rng/cache/clothexp2.json".into())).unwrap()).unwrap();
    let cal = Calib::default();
    let mut p = ClothParams::default();
    let (sx, sy, nx, z0);
    let mut cols: Vec<Collider> = vec![];
    let mut pinned = false;
    if case.starts_with("hang") {
        sx = 0.8; sy = 0.8; nx = 16; z0 = 1.0; pinned = true;
        match case.as_str() {
            "hang_k2_m005" => { p.mass = 0.05; p.tension = 2.0; }
            "hang_k15_m03" => { p.mass = 0.3; }
            "hang_k2_m03" => { p.mass = 0.3; p.tension = 2.0; p.compression = 2.0; }
            "hang_k30_m005" => { p.mass = 0.05; p.tension = 30.0; p.compression = 30.0; p.shear = 30.0; }
            "hang_k2_m005_b0" => { p.mass = 0.05; p.tension = 2.0; p.bending = 0.0; }
            "hang_k2_m005_b5" => { p.mass = 0.05; p.tension = 2.0; p.bending = 5.0; }
            _ => panic!("case"),
        }
    } else {
        sx = 1.0; sy = 1.0; nx = 32; z0 = 0.108;
        p.mass = 0.05; p.tension = 2.0; p.distance_min = 5e-3;
        if case == "drop_nodamp" { p.tension_damp = 0.0; p.compression_damp = 0.0; p.shear_damp = 0.0; }
        let (bm, _) = grid(1, 1);
        let mut bm = bm;
        for v in bm.pos.iter_mut() { *v = [v[0] * 0.25, v[1] * 0.25, 0.0]; }
        // box +-0.25 x +-0.25 x +-0.1
        let mut pos = vec![];
        for z in [-0.1, 0.1] { for y in [-0.25, 0.25] { for x in [-0.25, 0.25] { pos.push([x, y, z]); } } }
        let faces: Vec<Vec<u32>> = vec![vec![0, 2, 3, 1].into_iter().rev().collect(), vec![4, 5, 7, 6], vec![0, 1, 5, 4], vec![2, 6, 7, 3], vec![0, 4, 6, 2], vec![1, 3, 7, 5]];
        cols.push(Collider::new(&pos, &faces));
    }
    let (mut m, _) = grid(nx, nx);
    for v in m.pos.iter_mut() { *v = [v[0] * sx / 2.0, v[1] * sy / 2.0, z0]; }
    p.end_frame = frame;
    if let Ok(b) = std::env::var("BEND") { p.bending = b.parse().unwrap(); }
    if let Ok(b) = std::env::var("SHEAR") { p.shear = b.parse().unwrap(); }
    if pinned {
        p.pin = m.pos.iter().map(|v| if v[1] > 0.39 { 1.0 } else { 0.0 }).collect();
    }
    let cr: Vec<&Collider> = cols.iter().collect();
    let out = simulate(&m.pos, &m.faces, &p, &cr, &cal);
    let key = if case.starts_with("hang") { case.clone() } else { case.clone() };
    let bl: Vec<[f64; 3]> = j[&key][frame.to_string()].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
    let mut s = 0.0; let mut mx: f64 = 0.0;
    for (p, q) in out.iter().zip(&bl) { let d = ((p[0]-q[0]).powi(2) + (p[1]-q[1]).powi(2) + (p[2]-q[2]).powi(2)).sqrt(); s += d; mx = mx.max(d); }
    let stat = |v: &[[f64; 3]]| { let zs: Vec<f64> = v.iter().map(|q| q[2]).collect(); (zs.iter().cloned().fold(f64::INFINITY, f64::min), zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), zs[zs.len() / 2], v.iter().map(|q| q[2]).sum::<f64>() / v.len() as f64) };
    let (r0, r1, rc, rm) = stat(&out);
    let (b0, b1, bc, bm2) = stat(&bl);
    if std::env::var("CORNERS").is_ok() { let n = nx + 1; for (nm, idx) in [("corner", 0), ("edge-mid", n / 2), ("quarter", n / 4), ("quarter-diag", (n / 4) * n + n / 4), ("center", n * n / 2)] { println!("  {nm}: rust {:.3?} blender {:.3?}", out[idx as usize], bl[idx as usize]); } }
    println!("{case} f{frame}: vertex err mean {:.4} max {:.4} | z(min,max,center,mean) rust ({:.3},{:.3},{:.3},{:.3}) blender ({:.3},{:.3},{:.3},{:.3})", s / out.len() as f64, mx, r0, r1, rc, rm, b0, b1, bc, bm2);
}
