use indoorgen::bl32::*;
fn f(v: &serde_json::Value) -> f32 { v.as_f64().unwrap() as f32 }
fn v3(v: &serde_json::Value) -> [f32; 3] { let a = v.as_array().unwrap(); [f(&a[0]), f(&a[1]), f(&a[2])] }
fn main() {
    let path = std::env::args().nth(1).unwrap_or("/Users/lume/work/ref/bl32/cases.json".into());
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let verts: Vec<[f32; 3]> = j["verts"].as_array().unwrap().iter().map(v3).collect();
    let polys: Vec<Vec<usize>> = j["polys"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect()).collect();
    let ln_py: Vec<[f32; 3]> = j["local_normals"].as_array().unwrap().iter().map(v3).collect();
    for (i, p) in polys.iter().enumerate() { let ln = poly_normal(verts[p[0]], verts[p[1]], verts[p[2]]); if ln != ln_py[i] { println!("LOCAL NORMAL MISMATCH poly {i}: rs {ln:?} py {:?}", ln_py[i]); } }
    let mut bad = std::collections::BTreeMap::<String, (usize, String)>::new();
    let mut total = 0usize;
    for (ci, case) in j["cases"].as_array().unwrap().iter().enumerate() {
        let mut o = Obj32::new([0.0; 3], [0.0; 3]);
        let ops = case["ops"].as_array().unwrap();
        let res = case["res"].as_array().unwrap();
        for (k, op) in ops.iter().enumerate() {
            let name = op[0].as_str().unwrap();
            if k > 0 { let pr = &res[k - 1]; o = Obj32 { loc: v3(&pr["loc"]), eul: v3(&pr["euler"]), scale: v3(&pr["scale"]) }; }
            match name {
                "setloc" => o.loc = v3(&op[1]),
                "seteul" => o.eul = v3(&op[1]),
                "translate" => { let a = op[1].as_array().unwrap(); o.translate([a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]) }
                "rotate" => {
                    let a = op[1].as_array().unwrap();
                    let d = o.rotate_dbg([a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()], op[2].as_f64().unwrap());
                    let pd = &res[k]["dbg"];
                    let m3 = |m: &[[f32; 3]; 3], key: &str| -> bool { let p = pd[key].as_array().unwrap(); (0..3).all(|r| (0..3).all(|c| m[c][r] == f(&p[r][c]))) };
                    let q = |q: &[f32; 4], key: &str| -> bool { let p = pd[key].as_array().unwrap(); (0..4).all(|i| q[i] == f(&p[i])) };
                    let m4 = |m: &[[f32; 4]; 4], key: &str| -> bool { let p = pd[key].as_array().unwrap(); (0..4).all(|r| (0..4).all(|c| m[c][r] == f(&p[r][c]))) };
                    let mut sb = |n: &str, ok: bool| if !ok { let e = bad.entry(format!("rot-stage {n}")).or_insert((0, format!("case {ci} op {k}"))); e.0 += 1; };
                    sb("tm", m3(&d.tm, "tm")); if ci == 0 && k == 3 { eprintln!("DBG quat rs {:?} py {:?}\n rm rs {:?} py {:?}\n prev state {:?}", d.quat, pd["quat"], d.rm, pd["rm"], o); }
                    sb("quat", q(&d.quat, "quat")); sb("rm", m3(&d.rm, "rm")); sb("prod", m3(&d.prod, "prod")); sb("q2", q(&d.q2, "q2")); sb("lrs", m4(&d.lrs, "lrs"));
                }
                _ => panic!(),
            }
            let r = &res[k];
            total += 1;
            let mut chk = |field: &str, ok: bool, detail: String| {
                if !ok {
                    let e = bad.entry(format!("{name}/{field}")).or_insert((0, detail.clone()));
                    e.0 += 1;
                }
            };
            chk("loc", o.loc == v3(&r["loc"]), format!("case {ci} op {k}: rs {:?} py {:?}", o.loc, v3(&r["loc"])));
            chk("euler", o.eul == v3(&r["euler"]), format!("case {ci} op {k}: rs {:?} py {:?}", o.eul, v3(&r["euler"])));
            chk("scale", o.scale == v3(&r["scale"]), format!("case {ci} op {k}: rs {:?} py {:?}", o.scale, v3(&r["scale"])));
            let m = o.matrix_world();
            let mp = r["M"].as_array().unwrap();
            let mut mok = true;
            for row in 0..4 { for col in 0..4 { if m[col][row] != f(&mp[row][col]) { mok = false; } } }
            chk("matrix", mok, format!("case {ci} op {k}: rs {:?}", m));
            let gv = r["gv"].as_array().unwrap();
            let mut vok = true;
            for (i, v) in verts.iter().enumerate() { if mat4_mul_point(&m, *v) != v3(&gv[i]) { vok = false; } }
            chk("gv", vok, format!("case {ci} op {k}"));
            let gn = r["gn"].as_array().unwrap();
            let mut nok = true;
            for (i, p) in polys.iter().enumerate() {
                let ln = poly_normal(verts[p[0]], verts[p[1]], verts[p[2]]);
                let gg = global_polygon_normal(&m, ln, false);
                if gg != v3(&gn[i]) { nok = false; if ci == 0 && k == 0 && std::env::var("GNDBG").is_ok() { eprintln!("poly {i} local {:?} rs {:?} py {:?}", ln, gg, v3(&gn[i])); } }
            }
            chk("gn", nok, format!("case {ci} op {k}"));
        }
    }
    println!("ops checked {total}");
    for (k, (n, d)) in bad { println!("MISMATCH {k}: {n}  e.g. {d}"); }
}
