use indoorgen::bl32::*;
fn f(v: &serde_json::Value) -> f32 { v.as_f64().unwrap() as f32 }
fn v3(v: &serde_json::Value) -> [f32; 3] { let a = v.as_array().unwrap(); [f(&a[0]), f(&a[1]), f(&a[2])] }
fn main() {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/Users/lume/work/ref/bl32/vec.json").unwrap()).unwrap();
    let mut bad = std::collections::BTreeMap::<&str, (usize, String)>::new();
    for (i, c) in j.as_array().unwrap().iter().enumerate() {
        let (a, b) = (v3(&c["a"]), v3(&c["b"]));
        let s = c["s"].as_f64().unwrap();
        let mut chk = |n: &'static str, ok: bool, d: String| if !ok { let e = bad.entry(n).or_insert((0, format!("case {i}: {d}"))); e.0 += 1; };
        chk("dot", vdot(a, b) == c["dot"].as_f64().unwrap(), format!("{:e} vs {:e}", vdot(a, b), c["dot"].as_f64().unwrap()));
        chk("sub", vsub(a, b) == v3(&c["sub"]), String::new());
        chk("add", vadd(a, b) == v3(&c["add"]), String::new());
        chk("neg", vneg(a) == v3(&c["neg"]), String::new());
        chk("mul", vscale(a, s) == v3(&c["mul"]), format!("{:?} vs {:?}", vscale(a, s), v3(&c["mul"])));
        chk("normalized", vnormalized(b) == v3(&c["normalized"]), format!("{:?} vs {:?}", vnormalized(b), v3(&c["normalized"])));
        if !c["div"].is_null() {
            let n = ((a[0] as f64).powi(2) + (a[1] as f64).powi(2) + (a[2] as f64).powi(2)).sqrt();
            chk("div", vdiv(a, n) == v3(&c["div"]), format!("{:?} vs {:?}", vdiv(a, n), v3(&c["div"])));
        }
        chk("divc", vdiv(a, 1e-4) == v3(&c["divc"]), format!("{:?} vs {:?}", vdiv(a, 1e-4), v3(&c["divc"])));
        let npd = (0..3).fold(0.0f64, |acc, k| acc + a[k] as f64 * b[k] as f64);
        chk("npdot", npd == c["npdot"].as_f64().unwrap(), format!("{npd} vs {}", c["npdot"]));
    }
    println!("done");
    for (k, (n, d)) in bad { println!("MISMATCH {k}: {n} e.g. {d}"); }
}
