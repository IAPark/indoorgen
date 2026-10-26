//! meshdiff DUMP.jsonl [kind=asset]: compare python spawn_asset meshes (pydump.py) against Rust `Factory::exact`.
use indoorgen::assets::*;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let kind = std::env::args().nth(2).unwrap_or("asset".into());
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let usage = indoorgen::furn::solver::Usage::from_json(&prog["used_as"]);
    let verbose = std::env::var("V").is_ok();
    let mut stats: std::collections::BTreeMap<String, (usize, usize, usize, usize, usize, f64, usize)> = Default::default();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j["kind"].as_str().unwrap_or("ph") != kind { continue; }
        let class = j["class"].as_str().unwrap().to_string();
        let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        let e = stats.entry(class.clone()).or_default();
        e.0 += 1;
        let f = match make_factory(&class, fs) { Some(f) => f, None => { println!("{class} {fs}: no factory"); continue; } };
        let ex = if kind == "ph" && f.placeholder_exact(inst).is_some() { f.placeholder_exact(inst) } else if kind == "ph" {
            let real = usage.has_usage(&class, "RealPlaceholder");
            let phb = usage.has_usage(&class, "PlaceholderBBox");
            let eb = f.bounds_hint(inst).or_else(|| None).or_else(|| f.exact(inst).map(|e| { let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]); for v in &e.verts { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } } (lo, hi) }));
            let a = if real || phb { f.placeholder(inst) } else { Some(f.asset(inst)) };
            a.map(|a| { let (lo, hi) = if !real && !phb && eb.is_some() { eb.unwrap() } else { a.bounds() }; if real { indoorgen::assets::cabnodes::new_bbox_exact(lo, hi) } else { indoorgen::assets::cabnodes::box_exact(lo, hi) } })
        } else { f.exact(inst).map(|e| if kind == "populated" { e.triangulated().with_canonical_tags() } else { e }) };
        let ex = match ex { Some(x) => x, None => { println!("{class} {fs}: no exact()"); continue; } };
        let pv: Vec<[f64; 3]> = j["verts"].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
        let pp: Vec<Vec<u32>> = j["polys"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let pn: Vec<String> = j["face_names"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        let (pv, pp, pn) = if kind == "ph" && pv.len() != ex.verts.len() && usage.has_usage(&class, "PlaceholderBBox") {
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for v in &pv { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
            let b = indoorgen::assets::cabnodes::box_exact(lo, hi);
            (b.verts, b.polys, b.names)
        } else { (pv, pp, pn) };
        let vcount = pv.len() == ex.verts.len();
        let mut maxd: f64 = f64::NAN;
        if vcount {
            maxd = 0.0;
            for (a, b) in pv.iter().zip(&ex.verts) { for k in 0..3 { maxd = maxd.max((a[k] - b[k]).abs()); } }
        }
        let nbit = if vcount { pv.iter().zip(&ex.verts).filter(|(a, b)| (0..3).any(|k| (a[k] as f32) != (b[k] as f32))).count() } else { 0 };
        if verbose || std::env::var("BITS").is_ok() { println!("  {class} {fs}: verts not bit-equal: {nbit}/{}", pv.len()); if std::env::var("BITS").is_ok() { let mut c = 0; for (i, (a, b)) in pv.iter().zip(&ex.verts).enumerate() { if (0..3).any(|k| (a[k] as f32) != (b[k] as f32)) { println!("    v{i} py {:?} rs {:?}", a, b); c += 1; if c > 60 { break; } } } } }
        let polys_eq = pp == ex.polys;
        let names_eq = pn == ex.names;
        if vcount { e.1 += 1; }
        if polys_eq { e.2 += 1; }
        if names_eq { e.3 += 1; }
        if vcount && maxd < 1e-6 { e.4 += 1; }
        if maxd.is_finite() { e.5 = e.5.max(maxd); }
        let shown = e.6; 
        if verbose || (!(vcount && polys_eq && names_eq && maxd < 1e-6) && shown < 2) {
            e.6 += 1;
            println!("{class} fseed {fs} inst {inst}: verts py {} rs {} maxd {:.2e} polys_eq {} (py {} rs {}) names_eq {}", pv.len(), ex.verts.len(), maxd, polys_eq, pp.len(), ex.polys.len(), names_eq);
            if vcount && !polys_eq { for (i, (a, b)) in pp.iter().zip(&ex.polys).enumerate() { if a != b { println!("  first poly diff at {i}: py {:?} rs {:?}", a, b); break; } } }
        }
    }
    println!("{:30} {:>4} {:>8} {:>8} {:>8} {:>10} {:>10}", "class", "n", "vcount", "polys", "names", "coord<1e-6", "max coord err");
    for (c, (n, v, p, nm, ok, m, _)) in stats { println!("{:30} {:>4} {:>8} {:>8} {:>8} {:>10} {:>10.2e}", c, n, v, p, nm, ok, m); }
}
