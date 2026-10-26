use indoorgen::furn::fcl::*;
fn tf(t: &Vec<Vec<f64>>, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| t[i][0] * p[0] + t[i][1] * p[1] + t[i][2] * p[2] + t[i][3])
}
fn main() {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/bl32/fcl_cases.json", std::env::var("INDOORGEN_ORACLES").unwrap_or_else(|_| format!("{}/../local-data/ref", env!("CARGO_MANIFEST_DIR"))))).unwrap()).unwrap();
    let (mut ok, mut bad) = (0, 0);
    for (i, c) in j.as_array().unwrap().iter().enumerate() {
        let ext = |k: &str| { let a = c[k].as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] };
        let mat = |k: &str| -> Vec<Vec<f64>> { c[k].as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()).collect() };
        let (ta, tb) = (mat("Ta"), mat("Tb"));
        let a = trimesh_box_tris(ext("ea"), &|p| tf(&ta, p));
        let b = trimesh_box_tris(ext("eb"), &|p| tf(&tb, p));
        let r = mesh_depth(&a, &b);
        if i == 2 { for (ia, ta) in a.iter().enumerate() { for (ib, tb) in b.iter().enumerate() { if let Some((d, n)) = tri_contact(*ta, *tb) { println!("pair {ia} {ib} depth {d:.5} n {n}"); } } } }
        let want = if c["hit"].as_bool().unwrap() { c["depth"].as_f64() } else { None };
        let same = match (r, want) { (None, None) => true, (Some(x), Some(y)) => (x - y).abs() < 1e-9, _ => false };
        if same { ok += 1 } else { bad += 1; if bad <= 8 { println!("case {i}: rs {r:?} py {want:?} n_py {}", c["n"]); } }
    }
    println!("ok {ok} bad {bad}");
}
