//! diffasset file.jsonl : build each (class, fseed, inst) with the Rust factory and diff vertices/polygons against the python dump
use infinigen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut n = 0;
    let (mut vcount_ok, mut poly_ok, mut worst) = (0, 0, 0.0f64);
    let mut cls = String::new();
    for line in std::fs::read_to_string(&a[1]).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        cls = j["class"].as_str().unwrap().to_string();
        let fs = j["fseed"].as_u64().unwrap();
        let inst = j["inst"].as_u64().unwrap();
        n += 1;
        if j.get("error").is_some() { println!("{cls} {fs}: python error {}", j["error"]); continue; }
        let f = match make_factory(&cls, fs) { Some(f) => f, None => { println!("{cls}: no factory"); return; } };
        let asset = if std::env::var("PH").is_ok() { f.placeholder(inst).expect("no placeholder") } else { f.asset(inst) };
        let mut verts: Vec<[f64; 3]> = vec![];
        let mut polys: Vec<Vec<u32>> = vec![];
        for p in &asset.parts {
            let off = verts.len() as u32;
            verts.extend(p.mesh.pos.iter().cloned());
            for fc in &p.mesh.faces { polys.push(fc.iter().map(|i| i + off).collect()); }
        }
        let pv = j["verts"].as_array().unwrap();
        let pp = j["polys"].as_array().unwrap();
        let vok = pv.len() == verts.len();
        let mut err = f64::INFINITY;
        if vok {
            err = 0.0;
            for (i, v) in pv.iter().enumerate() { for k in 0..3 { err = err.max((v[k].as_f64().unwrap() - verts[i][k]).abs()); } }
        }
        let pok = pp.len() == polys.len() && pp.iter().zip(&polys).all(|(x, y)| x.as_array().unwrap().iter().map(|t| t.as_u64().unwrap() as u32).collect::<Vec<_>>() == *y);
        if vok { vcount_ok += 1; if err.is_finite() { worst = worst.max(err); } }
        if pok { poly_ok += 1; }
        println!("{cls} {fs} inst {inst}: verts py {} rs {} {}  maxerr {:.2e}  polys {} (py {} rs {})", pv.len(), verts.len(), if vok { "OK" } else { "MISMATCH" }, err, if pok { "identical" } else { "DIFFER" }, pp.len(), polys.len());
    }
    println!("SUMMARY {cls}: n={n} vertex-count match {vcount_ok}, polys identical {poly_ok}, worst coord err (count-matching) {worst:.2e}");
}
