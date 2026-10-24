use infinigen::assets::cabnodes::*;
fn main() {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/tmp/stor_ngon32.json").unwrap()).unwrap();
    let cases: Vec<(&str, GMesh)> = vec![
        ("a", gcylinder(32, 0.005, 0.001, 1).translated([0.0, 0.0, 0.0]).triangulated()),
        ("b", gcylinder(32, 0.005, 0.001, 1).translated([0.31, 0.02, 0.77]).triangulated()),
        ("c", gcylinder(32, 1.0, 2.0, 1).translated([0.0, 0.0, 0.0]).triangulated()),
        ("d", gcylinder(32, 0.005, 0.001, 1).translated([-0.31, 0.2, 0.7]).triangulated()),
    ];
    for (n, g) in cases {
        let py: Vec<Vec<u32>> = j[n].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let eq = py == g.p;
        println!("{n}: polys equal {eq} (py {} rs {})", py.len(), g.p.len());
        if !eq { for (i, (a, b)) in py.iter().zip(&g.p).enumerate() { if a != b { println!("  first diff {i}: py {:?} rs {:?}", a, b); break; } } }
    }
}
