use indoorgen::assets::curvekit::*;
fn rd(p: &str) -> Vec<[f64; 3]> {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let v = if j.is_array() { j.clone() } else { j["verts"].clone() };
    v.as_array().unwrap().iter().map(|x| [x[0].as_f64().unwrap(), x[1].as_f64().unwrap(), x[2].as_f64().unwrap()]).collect()
}
fn cmp(name: &str, a: &[[f64; 3]], b: &[[f64; 3]]) {
    let mut mx = 0f32; let mut ex = 0; let mut tot = 0;
    for (p, q) in a.iter().zip(b) { for k in 0..3 { let d = (p[k] as f32 - q[k] as f32).abs(); mx = mx.max(d); tot += 1; if d == 0.0 { ex += 1; } } }
    println!("{name}: n {} {} max {:e} exact {}/{}", a.len(), b.len(), mx, ex, tot);
}
fn main() {
    let pts = rd("/tmp/f_pts.json");
    let ev = catmull_rom_eval(&pts, 12);
    cmp("eval", &ev.pts, &rd("/tmp/f_cr_eval.json"));
    cmp("res128", &ev.resample(128).pts, &rd("/tmp/f_cr_res128.json"));
}
