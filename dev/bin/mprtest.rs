use indoorgen::furn::mpr::*;
fn main() {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/bl32/fcl_cases.json", std::env::var("INDOORGEN_ORACLES").unwrap_or_else(|_| format!("{}/../local-data/ref", env!("CARGO_MANIFEST_DIR"))))).unwrap()).unwrap();
    let (mut ok_ab, mut ok_ba, mut bad) = (0, 0, 0);
    for (i, c) in j.as_array().unwrap().iter().enumerate() {
        let ext = |k: &str| { let a = c[k].as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] };
        let mat = |k: &str| -> ([[f64; 3]; 3], [f64; 3]) { let m: Vec<Vec<f64>> = c[k].as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()).collect(); ([[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]], [m[0][3], m[1][3], m[2][3]]) };
        let verts = |e: [f64; 3]| -> Vec<[f64; 3]> { (0..8usize).map(|i| [if i & 4 != 0 { e[0] / 2. } else { -e[0] / 2. }, if i & 2 != 0 { e[1] / 2. } else { -e[1] / 2. }, if i & 1 != 0 { e[2] / 2. } else { -e[2] / 2. }]).collect() };
        let (va, vb) = (verts(ext("ea")), verts(ext("eb")));
        let (ra, ta) = mat("Ta");
        let (rb, tb) = mat("Tb");
        let a = ConvexShape::new(&va, [0.0; 3], ra, ta);
        let b = ConvexShape::new(&vb, [0.0; 3], rb, tb);
        let want = if c["hit"].as_bool().unwrap() { c["depth"].as_f64() } else { None };
        let ab = penetration(&a, &b);
        let ba = penetration(&b, &a);
        let same = |x: Option<f64>| match (x, want) { (None, None) => true, (Some(p), Some(q)) => (p - q).abs() < 1e-9, _ => false };
        if same(ab) { ok_ab += 1 }
        if same(ba) { ok_ba += 1 }
        if !same(ab) && !same(ba) { bad += 1; if bad <= 6 { println!("case {i}: ab {ab:?} ba {ba:?} py {want:?}"); } }
    }
    println!("ab ok {ok_ab} ba ok {ok_ba} neither {bad}");
    let (mut dok_ab, mut dok_ba, mut dbad) = (0, 0, 0);
    for (i, c) in j.as_array().unwrap().iter().enumerate() {
        let ext = |k: &str| { let a = c[k].as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] };
        let mat = |k: &str| -> ([[f64; 3]; 3], [f64; 3]) { let m: Vec<Vec<f64>> = c[k].as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()).collect(); ([[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]], [m[0][3], m[1][3], m[2][3]]) };
        let verts = |e: [f64; 3]| -> Vec<[f64; 3]> { (0..8usize).map(|i| [if i & 4 != 0 { e[0] / 2. } else { -e[0] / 2. }, if i & 2 != 0 { e[1] / 2. } else { -e[1] / 2. }, if i & 1 != 0 { e[2] / 2. } else { -e[2] / 2. }]).collect() };
        let (va, vb) = (verts(ext("ea")), verts(ext("eb")));
        let (ra, ta) = mat("Ta");
        let (rb, tb) = mat("Tb");
        let a = ConvexShape::new(&va, [0.0; 3], ra, ta);
        let b = ConvexShape::new(&vb, [0.0; 3], rb, tb);
        let want = c["dist"].as_f64().unwrap();
        if want < 0.0 { continue; }
        let (ab, ba) = (distance(&a, &b), distance(&b, &a));
        let ok = |x: f64| (x - want).abs() < 1e-9;
        if ok(ab) { dok_ab += 1 }
        if ok(ba) { dok_ba += 1 }
        if !ok(ab) && !ok(ba) { dbad += 1; if dbad <= 6 { println!("dist case {i}: ab {ab} ba {ba} py {want}"); } }
    }
    println!("dist: ab ok {dok_ab} ba ok {dok_ba} neither {dbad}");
}
