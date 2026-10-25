use serde_json::Value as J;
fn main() {
    let j: J = serde_json::from_str(&std::fs::read_to_string("tmp/fg128.json").unwrap()).unwrap();
    let n = 128;
    let lp: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            let (sn, cs) = ig_plants::cabnodes::sin_cos_from_fraction(k, n);
            [((sn * -1.0f32) + 1.0f32) as f64, (cs * 1.0f32) as f64, 0.0]
        })
        .collect();
    let bv: Vec<[f32; 3]> = j["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32]).collect();
    for best in 0..n {
        let m = ig_plants::shell::fill_grid_circle_best(&lp, best);
        if m.pos.len() != bv.len() {
            continue;
        }
        let mx = m.pos.iter().zip(bv.iter()).map(|(a, b)| (0..3).map(|k| (a[k] as f32 - b[k]).abs()).fold(0f32, f32::max)).fold(0f32, f32::max);
        if mx < 1e-3 {
            println!("best {best} maxerr {mx:e}");
        }
    }
}
