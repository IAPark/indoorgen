use indoorgen::assets::curvekit::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cases: Vec<Vec<(f64, f64)>> = serde_json::from_str::<Vec<Vec<(f64, f64)>>>(&a[1]).unwrap();
    let py: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/tmp/f_fc.json").unwrap()).unwrap();
    for (ci, pts) in cases.iter().enumerate() {
        let fc = FloatCurve::new(pts);
        let mut mx = 0.0f64;
        let mut at = 0.0;
        for k in 0..2001 {
            let x = (-0.1f64 + 1.2 * k as f64 / 2000.0) as f32 as f64;
            
            let p = py[ci.to_string()][k].as_f64().unwrap();
            let d = (fc.eval(x) - p).abs();
            if d > mx { mx = d; at = x; }
        }
        for k in (0..2001).step_by(100) { let x = (-0.1f64 + 1.2 * k as f64 / 2000.0) as f32 as f64; if x >= 0.0 && x <= 1.0 { println!("  x {:.4} rs {:.6} py {:.6}", x, fc.eval(x), py[ci.to_string()][k].as_f64().unwrap()); } }
        println!("case {ci} max diff {mx:e} at {at}");
    }
}
