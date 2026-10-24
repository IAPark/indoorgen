use infinigen::assets::cabnodes::*;
fn main() {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/tmp/stor_probe3.json").unwrap()).unwrap();
    let rot = [0.0f32, 0.0, -1.5708];
    let t = [0.1234f32, 0.0777, 0.3];
    let z = [0f32; 3];
    let o = [1f32; 3];
    let cyl = || gcylinder(32, 0.00317, 0.0027, 2);
    let cases: Vec<(&str, GMesh)> = vec![
        ("cube_rot", gcube([0.0371, 0.2917, 0.8123], [2, 2, 2]).transformed(t, rot, o)),
        ("cube_t_rot", gcube([0.0371, 0.2917, 0.8123], [2, 2, 2]).translated(t).transformed(z, rot, o)),
        ("cyl_plain", cyl()),
        ("cyl_t", cyl().translated(t)),
        ("cyl_trot", cyl().translated(t).transformed(z, rot, o)),
        ("cube5", gcube([0.0371, 0.2917, 0.8123], [5, 5, 5])),
    ];
    for (name, g) in cases {
        let e = j.as_array().unwrap().iter().find(|e| e["name"] == name).unwrap();
        let pv = e["verts"].as_array().unwrap();
        let mut nb = 0;
        let mut first = None;
        for (i, v) in pv.iter().enumerate() {
            let a = v.as_array().unwrap();
            let r = g.v[i];
            if (0..3).any(|k| (a[k].as_f64().unwrap() as f32) != r[k]) { nb += 1; if first.is_none() { first = Some((i, a.iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>(), r)); } }
        }
        if name=="cyl_plain" { for (i, v) in pv.iter().enumerate().take(12) { let a=v.as_array().unwrap(); println!("  v{i} py {:e} {:e} rs {:e} {:e}  ulp {} {}", a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), g.v[i][0], g.v[i][1], (a[0].as_f64().unwrap() as f32).to_bits() as i64 - g.v[i][0].to_bits() as i64, (a[1].as_f64().unwrap() as f32).to_bits() as i64 - g.v[i][1].to_bits() as i64); } }
        println!("{name}: n {} (rs {}) not bit-equal {nb} first {:?}", pv.len(), g.v.len(), first);
    }
}
