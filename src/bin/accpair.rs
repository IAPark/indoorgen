use infinigen::furn::mpr::*;
fn v3(v: &serde_json::Value) -> [f64; 3] { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }
fn m3(v: &serde_json::Value) -> [[f64; 3]; 3] { let a = v.as_array().unwrap(); [v3(&a[0]), v3(&a[1]), v3(&a[2])] }
fn main() {
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/tmp/accjson.txt").unwrap()).unwrap();
    let fv: Vec<[f64; 3]> = d["fverts"].as_array().unwrap().iter().map(v3).collect();
    let bv: Vec<[f64; 3]> = d["bverts"].as_array().unwrap().iter().map(v3).collect();
    let mean = |v: &[[f64; 3]]| { let n = v.len() as f64; let mut s = [0.0; 3]; for p in v { for k in 0..3 { s[k] += p[k]; } } [s[0] / n, s[1] / n, s[2] / n] };
    let f = ConvexShape::new(&fv, mean(&fv), m3(&d["fr"]), v3(&d["ft"]));
    let b = ConvexShape::new(&bv, mean(&bv), m3(&d["br"]), v3(&d["bt"]));
    println!("b,f {:?}", mpr_penetration(&b, &f, 500, 1e-6));
    println!("f,b {:?}", mpr_penetration(&f, &b, 500, 1e-6));
    println!("fverts {:?}", fv);
}
