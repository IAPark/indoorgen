//! stagetest OP DIR TAG [args]: run one ported op on a python stage input (tools/pytrace.py dump) and compare with the python output.
use infinigen::assets::stagefile::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (op, dir, tag) = (a[1].as_str(), a[2].as_str(), a[3].as_str());
    let inp = read(&format!("{dir}/{tag}_in.bin")).expect("input");
    let out = read(&format!("{dir}/{tag}_out.bin")).expect("output");
    let meta: serde_json::Value = std::fs::read_to_string(format!("{dir}/{tag}.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
    match op {
        "remesh" => {
            let depth = meta["kwargs"]["octree_depth"].as_i64().unwrap() as i32;
            let (v, q) = infinigen::assets::dualcon::dualcon(&inp.v, &inp.f, 2, depth, 0.9, 1.0);
            let f: Vec<Vec<u32>> = q.iter().map(|x| x.iter().map(|&i| i as u32).collect()).collect();
            compare(tag, &v, &f, &out, 1e-6);
        }
        _ => panic!("unknown op"),
    }
}
