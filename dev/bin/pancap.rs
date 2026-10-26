//! pancap n pystages.json : which ring start reproduces python's fill_grid cap (pre-rotation `filled` stage), and angle table
use indoorgen::assets::bpyops::fill_grid_loop_ex;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap();
    let off: usize = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(0);
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let pv: Vec<[f64; 3]> = j["filled"]["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]).collect();
    let inner: Vec<[f64; 3]> = pv.iter().cloned().filter(|p| p[2].abs() < 1e-6 && (p[0] * p[0] + p[1] * p[1]).sqrt() < 0.95).collect();
    let phid = 2.0f32 * std::f32::consts::PI / n as f32;
    let mut phi = 0.0f32;
    let _ = (&mut phi, phid);
    let c32: Vec<[f32; 3]> = (0..n).map(|i| { let (s, c) = indoorgen::assets::cabnodes::sin_cos_from_fraction(i, n); [-s, c, 0.0f32] }).collect();
    let c: Vec<[f64; 3]> = c32.iter().map(|v| [v[0] as f64, v[1] as f64, 0.0]).collect();
    let jv: Vec<[f32; 3]> = j["joined"]["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, 0.0]).collect();
    let bad = (0..n).filter(|&i| jv[i][0] != c32[i][0] || jv[i][1] != c32[i][1]).count();
    println!("ring vertices differing from python joined: {bad}/{n}");
    for s in 0..n {
        let ring: Vec<[f64; 3]> = (0..n).map(|i| c[(i + s) % n]).collect();
        let g = fill_grid_loop_ex(&ring, true);
        let gi: Vec<[f64; 3]> = g.pos.iter().cloned().filter(|p| (p[0] * p[0] + p[1] * p[1]).sqrt() < 0.95).collect();
        let miss = inner.iter().filter(|p| !gi.iter().any(|q| (0..3).all(|k| (q[k] - p[k]).abs() < 1e-5))).count();
        println!("start {s}: unmatched {miss}/{}", inner.len());
    }
    {
        let st: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(2);
        let ring: Vec<[f64; 3]> = (0..n).map(|i| c[(i + st) % n]).collect();
        let g = fill_grid_loop_ex(&ring, true);
        let pyv: Vec<[f32; 3]> = j["filled"]["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32]).collect();
        let mut worst = 0.0f32;
        let mut exact = 0;
        for p in g.pos.iter() {
            let q = [p[0] as f32, p[1] as f32, p[2] as f32];
            if (q[0] * q[0] + q[1] * q[1]).sqrt() > 0.99 { continue; }
            let d = pyv.iter().filter(|v| v[2] == 0.0).map(|v| (v[0] - q[0]).abs().max((v[1] - q[1]).abs())).fold(f32::INFINITY, f32::min);
            if d == 0.0 { exact += 1 }
            worst = worst.max(d);
        }
        println!("grid f32 vs python: exact {exact}, worst {worst:e}");
    }
    let ang: Vec<f32> = (0..n).map(|i| (std::f32::consts::PI - indoorgen::bm::query::angle_v3v3v3(c32[(i + n - 1) % n], c32[i], c32[(i + 1) % n])).abs()).collect();
    for i in 0..n { println!("ang {i} {:e}", ang[i] as f64 - ang[0] as f64); }
    let _ = off;
}
