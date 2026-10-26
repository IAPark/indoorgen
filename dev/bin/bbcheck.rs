//! bbcheck PYASSET.jsonl : compare bounds of python spawn_asset meshes (pydump asset) with Rust bbox_of_asset
use indoorgen::assets::*;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut stats: std::collections::BTreeMap<String, (usize, usize, f64)> = Default::default();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j["kind"].as_str().unwrap_or("asset") != "asset" { continue; }
        let class = j["class"].as_str().unwrap().to_string();
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let f = match make_factory(&class, fs) { Some(f) => f, None => continue };
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in j["verts"].as_array().unwrap() { for k in 0..3 { let x = v[k].as_f64().unwrap(); lo[k] = lo[k].min(x); hi[k] = hi[k].max(x); } }
        let g = bbox_of_asset(f.as_ref(), inst);
        let b = (g.lo, g.hi);
        let d = (0..3).map(|k| (b.0[k] - lo[k]).abs().max((b.1[k] - hi[k]).abs())).fold(0.0, f64::max);
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        if d < 1e-5 { e.1 += 1 } else if std::env::var("V").is_ok() { println!("{class} {fs}: d {d:.3e} py {:?} {:?} rs {:?} {:?}", lo, hi, b.0, b.1); }
        e.2 = e.2.max(d);
    }
    for (c, (n, ok, w)) in stats { println!("{c:30} n {n:3} ok(<1e-5) {ok:3} worst {w:.3e}"); }
}
