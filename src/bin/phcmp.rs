//! phcmp PH.jsonl : bounds of Factory::placeholder(inst) vs python placeholder meshes (pydump ph)
use indoorgen::assets::*;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut stats: std::collections::BTreeMap<String, (usize, usize, f64)> = Default::default();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j["kind"].as_str().unwrap_or("ph") != "ph" { continue; }
        let class = j["class"].as_str().unwrap().to_string();
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let f = match make_factory(&class, fs) { Some(f) => f, None => continue };
        let a = match f.placeholder(inst) { Some(a) => a, None => continue };
        let (lo, hi) = a.bounds();
        let (mut plo, mut phi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in j["verts"].as_array().unwrap() { for k in 0..3 { let x = v[k].as_f64().unwrap(); plo[k] = plo[k].min(x); phi[k] = phi[k].max(x); } }
        let d = (0..3).map(|k| (lo[k] - plo[k]).abs().max((hi[k] - phi[k]).abs())).fold(0.0, f64::max);
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        if d < 1e-5 { e.1 += 1 } else if std::env::var("V").is_ok() { println!("{class} {fs}: d {d:.3e} py {:?} {:?} rs {:?} {:?}", plo, phi, lo, hi); }
        e.2 = e.2.max(d);
    }
    for (c, (n, ok, w)) in stats { println!("{c:30} n {n:3} ok(<1e-5) {ok:3} worst {w:.3e}"); }
}
