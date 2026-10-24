//! stagetest OP DIR TAG [args]: run one ported op on a python stage input (tools/pytrace.py dump) and compare with the python output.
use infinigen::assets::stagefile::*;

fn dir_s(a: &[String]) -> String {
    a[2].clone()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "monocot" {
        let fs: u64 = a[2].parse().unwrap();
        let inst: u64 = a[3].parse().unwrap();
        let t0 = std::time::Instant::now();
        let f = infinigen::assets::monocot::MonocotFactory::new(fs);
        let m = f.mesh(inst);
        println!("monocot {fs}: {} verts {} faces in {:.1}s", m.v.len(), m.f.len(), t0.elapsed().as_secs_f64());
        if let Ok(d) = std::env::var("MONOTRACE").or(std::env::var("MONOFINAL")) {
            write(&format!("{d}/999_final_out.bin"), &m.v, &m.f, &m.e);
        }
        return;
    }
    if a[1] == "leaf" {
        let xs: Vec<f64> = a[2].split(',').map(|x| x.parse().unwrap()).collect();
        let ys: Vec<f64> = a[3].split(',').map(|x| x.parse().unwrap()).collect();
        let l = infinigen::assets::leafkit::leaf(&xs, &ys, Some(0.003));
        println!("leaf: {} verts {} faces", l.v.len(), l.f.len());
        return;
    }
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
        "fill" => {
            let m = infinigen::assets::leafkit::LMesh { v: inp.v.clone(), f: inp.f.clone(), e: inp.e.clone() };
            let f = infinigen::assets::leafkit::fill_outline(&m);
            let mut neg = 0;
            let mut tot = 0.0f64;
            for t in &f.f {
                let (a, b, c) = (f.v[t[0] as usize], f.v[t[1] as usize], f.v[t[2] as usize]);
                let ar = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if ar < 0.0 { neg += 1; }
                tot += ar as f64 / 2.0;
            }
            println!("fill: {} tris, neg {}, area {} ; python area:", f.f.len(), neg, tot);
            let s = infinigen::assets::leafkit::solidify(&f, 0.1);
            let zs: std::collections::BTreeSet<i64> = s.v.iter().map(|p| (p[2] * 1000.0).round() as i64).collect();
            println!("solidify z levels (mm): {:?}", zs);
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
