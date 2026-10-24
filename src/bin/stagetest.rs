//! stagetest OP DIR TAG [args]: run one ported op on a python stage input (tools/pytrace.py dump) and compare with the python output.
use infinigen::assets::stagefile::*;

fn dir_s(a: &[String]) -> String {
    a[2].clone()
}

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
        "weld" => {
            let th = meta["kwargs"]["merge_threshold"].as_f64().unwrap() as f32;
            let m = infinigen::assets::cabnodes::GMesh { v: inp.v.clone(), p: inp.f.clone(), tag: vec![0; inp.f.len()], e: inp.e.clone() };
            let r = infinigen::assets::gweld::merge_by_distance(&m, th);
            compare(tag, &r.v, &r.p, &out, 1e-6);
        }
        "simple_deform" => {
            use infinigen::assets::bpymod::*;
            let k = &meta["kwargs"];
            let m = if k["deform_method"] == "TWIST" { Deform::Twist } else { Deform::Bend };
            let ax = match k["deform_axis"].as_str().unwrap_or("Z") { "X" => 0, "Y" => 1, _ => 2 };
            let mut v = inp.v.clone();
            simple_deform(&mut v, m, k["angle"].as_f64().unwrap(), ax);
            compare(tag, &v, &inp.f, &out, 1e-6);
        }
        "displace" => {
            use infinigen::assets::bpymod::*;
            use infinigen::assets::legtex::{Tex, TexKind};
            let k = &meta["kwargs"];
            let t = &k["texture"];
            let basis = match t["noise_basis"].as_str().unwrap_or("BLENDER_ORIGINAL") { "BLENDER_ORIGINAL" => 0, "ORIGINAL_PERLIN" => 1, "IMPROVED_PERLIN" => 2, b => panic!("basis {b}") };
            let stype = match t["stucci_type"].as_str().unwrap_or("PLASTIC") { "PLASTIC" => 0, "WALL_IN" => 1, _ => 2 };
            let mut tex = Tex::new(TexKind::Stucci(stype), t["noise_scale"].as_f64().unwrap());
            tex.basis = basis;
            tex.hard = t["noise_type"] == "HARD_NOISE";
            tex.turbul = t["turbulence"].as_f64().unwrap() as f32;
            let dir = match k["direction"].as_str().unwrap_or("NORMAL") { "X" => DispDir::X, "Y" => DispDir::Y, "Z" => DispDir::Z, _ => DispDir::Normal };
            let vg = read_f32(&format!("{dir_}/{tag}_vg.f32", dir_ = dir_s(&a)));
            let mid = k["mid_level"].as_f64().unwrap_or(0.5);
            let mut v = inp.v.clone();
            displace(&mut v, &inp.f, &tex, k["strength"].as_f64().unwrap(), mid, dir, vg.as_deref());
            compare(tag, &v, &inp.f, &out, 1e-6);
        }
        _ => panic!("unknown op"),
    }
}
