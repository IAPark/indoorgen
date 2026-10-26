//! stagetest OP DIR TAG [args]: run one ported op on a python stage input (tools/pytrace.py dump) and compare with the python output.
use indoorgen::assets::stagefile::*;

fn dir_s(a: &[String]) -> String {
    a[2].clone()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "monocot" {
        let fs: u64 = a[2].parse().unwrap();
        let inst: u64 = a[3].parse().unwrap();
        let t0 = std::time::Instant::now();
        let f = indoorgen::assets::monocot::MonocotFactory::new(fs);
        let m = f.mesh(inst);
        println!("monocot {fs}: {} verts {} faces in {:.1}s", m.v.len(), m.f.len(), t0.elapsed().as_secs_f64());
        if let Ok(d) = std::env::var("MONOTRACE").or(std::env::var("MONOFINAL")) {
            write(&format!("{d}/999_final_out.bin"), &m.v, &m.f, &m.e);
        }
        return;
    }
    if a[1] == "argsort" {
        // argsort FILE_A.f32 (raw) -> prints first indices
        let raw = std::fs::read(&a[2]).unwrap();
        let v: Vec<f32> = raw.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let idx = indoorgen::assets::leafkit::np_argsort_f32(&v);
        println!("{:?}", &idx[..12.min(idx.len())]);
        if let Ok(o) = std::env::var("ARGOUT") { std::fs::write(o, idx.iter().flat_map(|&i| (i as i64).to_le_bytes()).collect::<Vec<u8>>()).unwrap(); }
        return;
    }
    if a[1] == "transfer" {
        // transfer DIR SOLID_TAG REMESH_TAG EXPECT_TAG
        let d = &a[2];
        let src = read(&format!("{d}/{}_out.bin", a[3])).unwrap();
        let at = read(&format!("{d}/{}_out.bin", a[4])).unwrap();
        let ex = read(&format!("{d}/{}_out.bin", a[5])).unwrap();
        let lm = indoorgen::assets::leafkit::LMesh { v: src.v.clone(), f: src.f.clone(), e: vec![] };
        let n = lm.v.len() / 2;
        let top: Vec<f32> = (0..lm.v.len()).map(|i| if i >= n { 1.0 } else { 0.0 }).collect();
        let r = indoorgen::assets::leafkit::sample_nearest_surface(&lm, &top, &at.v);
        let bad: Vec<usize> = (0..r.len()).filter(|&i| r[i].1 != ex.v[i]).take(6).collect();
        let tris0 = indoorgen::assets::dualcon::corner_tris(&lm.v, &lm.f);
        for &bi in &bad {
            let q = at.v[bi];
            let mut cands: Vec<(f32, usize)> = vec![];
            for (ti, t) in tris0.iter().enumerate() {
                let cp = indoorgen::assets::leafkit::closest_on_tri_f32(q, lm.v[t[0] as usize], lm.v[t[1] as usize], lm.v[t[2] as usize]);
                let d = (cp[0]-q[0]).powi(2)+(cp[1]-q[1]).powi(2)+(cp[2]-q[2]).powi(2);
                cands.push((d, ti));
            }
            cands.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("BAD {bi}: q {:?} mine {:?} blender {:?}; best tris {:?}", q, r[bi].1, ex.v[bi], &cands[..3.min(cands.len())]);
        }
        let mut worst = (0.0f32, 0usize);
        for i in 0..r.len() {
            let e = (0..3).map(|k| (r[i].1[k] - ex.v[i][k]).abs()).fold(0.0, f32::max);
            if e > worst.0 { worst = (e, i); }
        }
        let nex = (0..r.len()).filter(|&i| r[i].1 == ex.v[i]).count();
        println!("exact {} of {}", nex, r.len());
        println!("worst {} at {} p {:?} mine {:?} blender {:?}", worst.0, worst.1, at.v[worst.1], r[worst.1].1, ex.v[worst.1]);
        let tris = indoorgen::assets::dualcon::corner_tris(&lm.v, &lm.f);
        let q = at.v[worst.1];
        for (ti, t) in tris.iter().enumerate() {
            let (a, b, c) = (lm.v[t[0] as usize], lm.v[t[1] as usize], lm.v[t[2] as usize]);
            let cp = indoorgen::assets::leafkit::closest_on_tri_f32(q, a, b, c);
            let d = ((cp[0]-q[0]).powi(2)+(cp[1]-q[1]).powi(2)+(cp[2]-q[2]).powi(2)).sqrt();
            if d < 3e-4 { println!("tri {ti} {:?} d {d:e} cp {:?} verts {:?} {:?} {:?}", t, cp, a, b, c); }
        }
        return;
    }
    if a[1] == "container" {
        let fs: u64 = a[2].parse().unwrap();
        let inst: u64 = a[3].parse().unwrap();
        let f = indoorgen::assets::make_factory("LargePlantContainerFactory", fs).unwrap();
        let asset = f.asset(inst);
        let d = &a[4];
        for (k, pi) in [(0usize, 0usize), (2, 1), (1, 2)] {
            if let Some(p) = asset.parts.get(pi) {
                let v: Vec<[f32; 3]> = p.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
                write(&format!("{d}/90{k}_part_out.bin"), &v, &p.mesh.faces, &[]);
            }
        }
        return;
    }
    if a[1] == "bbox" {
        // bbox CLASS SEEDS
        for sd in a[3].split(',') {
            let fs: u64 = sd.parse().unwrap();
            let t0 = std::time::Instant::now();
            let f = indoorgen::assets::make_factory(&a[2], fs).unwrap();
            let asset = f.asset(0);
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            let mut nv = 0;
            for p in &asset.parts {
                nv += p.mesh.pos.len();
                for q in &p.mesh.pos {
                    for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); }
                }
            }
            println!("BBOX {} {} {} {:.3?} {:.3?} {:.1}s", a[2], fs, nv, lo, hi, t0.elapsed().as_secs_f64());
        }
        return;
    }
    if a[1] == "leaf" {
        let xs: Vec<f64> = a[2].split(',').map(|x| x.parse().unwrap()).collect();
        let ys: Vec<f64> = a[3].split(',').map(|x| x.parse().unwrap()).collect();
        let l = indoorgen::assets::leafkit::leaf(&xs, &ys, Some(0.003));
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
            let (v, q) = indoorgen::assets::dualcon::dualcon(&inp.v, &inp.f, 2, depth, 0.9, 1.0);
            let f: Vec<Vec<u32>> = q.iter().map(|x| x.iter().map(|&i| i as u32).collect()).collect();
            if let Ok(d) = std::env::var("DUMPOUT") { write(&d, &v, &f, &[]); }
            compare(tag, &v, &f, &out, 1e-6);
        }
        "fillcmp" => {
            // fillcmp DIR 002_WELD(out=welded outline) : compare fill_outline(outline) with the python SOLIDIFY input
            let outline = read(&format!("{dir}/002_WELD_out.bin")).unwrap();
            let want = read(&format!("{dir}/003_SOLIDIFY_in.bin")).unwrap();
            let m = indoorgen::assets::leafkit::LMesh { v: outline.v.clone(), f: vec![], e: outline.e.clone() };
            let f = indoorgen::assets::leafkit::fill_outline(&m);
            println!("faces {} vs {}; identical lists {}", f.f.len(), want.f.len(), f.f == want.f);
            let first = f.f.iter().zip(&want.f).position(|(a, b)| a != b);
            println!("first diff {:?} {:?}", first, first.map(|i| (&f.f[i], &want.f[i])));
        }
        "solidcmp" => {
            let outline = read(&format!("{dir}/003_SOLIDIFY_in.bin")).unwrap();
            let want = read(&format!("{dir}/003_SOLIDIFY_out.bin")).unwrap();
            let m = indoorgen::assets::leafkit::LMesh { v: outline.v.clone(), f: outline.f.clone(), e: outline.e.clone() };
            let s = indoorgen::assets::leafkit::solidify(&m, 0.1);
            println!("verts {} vs {} exact {}", s.v.len(), want.v.len(), s.v == want.v);
            println!("faces {} vs {} identical {}", s.f.len(), want.f.len(), s.f == want.f);
            let first = s.f.iter().zip(&want.f).position(|(a, b)| a != b);
            println!("first diff {:?} {:?}", first, first.map(|i| (&s.f[i], &want.f[i])));
            let mut a: Vec<Vec<u32>> = s.f.clone(); let mut b: Vec<Vec<u32>> = want.f.clone();
            a.sort(); b.sort();
            println!("same sets (as stored) {}", a == b);
        }
        "fill" => {
            let m = indoorgen::assets::leafkit::LMesh { v: inp.v.clone(), f: inp.f.clone(), e: inp.e.clone() };
            let f = indoorgen::assets::leafkit::fill_outline(&m);
            let mut neg = 0;
            let mut tot = 0.0f64;
            for t in &f.f {
                let (a, b, c) = (f.v[t[0] as usize], f.v[t[1] as usize], f.v[t[2] as usize]);
                let ar = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if ar < 0.0 { neg += 1; }
                tot += ar as f64 / 2.0;
            }
            if let Ok(vs) = std::env::var("FILLV") {
                let vi: u32 = vs.parse().unwrap();
                for t in &f.f {
                    if t.contains(&vi) {
                        let (a, b, c) = (f.v[t[0] as usize], f.v[t[1] as usize], f.v[t[2] as usize]);
                        let ar = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                        println!("tri {:?} area2 {:e}", t, ar);
                    }
                }
            }
            println!("fill: {} tris, neg {}, area {} ; python area:", f.f.len(), neg, tot);
            let s = indoorgen::assets::leafkit::solidify(&f, 0.1);
            let zs: std::collections::BTreeSet<i64> = s.v.iter().map(|p| (p[2] * 1000.0).round() as i64).collect();
            println!("solidify z levels (mm): {:?}", zs);
        }
        "voxel" => {
            let vs: f64 = a.get(4).map(|x| x.parse().unwrap()).unwrap_or(0.02);
            let m = indoorgen::assets::leafkit::LMesh { v: inp.v.clone(), f: inp.f.clone(), e: vec![] };
            let tris = indoorgen::assets::dualcon::corner_tris(&m.v, &m.f);
            let r = indoorgen::assets::voxelremesh::voxel_remesh(&m.v, &tris, vs as f32);
            compare(tag, &r.v, &r.f, &out, 1e-4);
        }
        "rwa" => {
            let vs: f64 = a.get(4).map(|x| x.parse().unwrap()).unwrap_or(0.02);
            let m = indoorgen::assets::leafkit::LMesh { v: inp.v.clone(), f: inp.f.clone(), e: vec![] };
            let r = indoorgen::assets::voxelremesh::remesh_with_attrs(&m, vs);
            compare(tag, &r.v, &r.f, &out, 1e-4);
        }
        "weld" => {
            let th = meta["kwargs"]["merge_threshold"].as_f64().unwrap() as f32;
            let m = indoorgen::assets::cabnodes::GMesh { v: inp.v.clone(), p: inp.f.clone(), tag: vec![0; inp.f.len()], e: inp.e.clone() };
            let r = indoorgen::assets::gweld::merge_by_distance(&m, th);
            compare(tag, &r.v, &r.p, &out, 1e-6);
        }
        "simple_deform" => {
            use indoorgen::assets::bpymod::*;
            let k = &meta["kwargs"];
            let m = if k["deform_method"] == "TWIST" { Deform::Twist } else { Deform::Bend };
            let ax = match k["deform_axis"].as_str().unwrap_or("Z") { "X" => 0, "Y" => 1, _ => 2 };
            let mut v = inp.v.clone();
            simple_deform(&mut v, m, k["angle"].as_f64().unwrap(), ax);
            compare(tag, &v, &inp.f, &out, 1e-6);
        }
        "displace" => {
            use indoorgen::assets::bpymod::*;
            use indoorgen::assets::legtex::{Tex, TexKind};
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
            displace(&mut v, &inp.f, Some(&tex), k["strength"].as_f64().unwrap(), mid, dir, vg.as_deref());
            compare(tag, &v, &inp.f, &out, 1e-6);
        }
        _ => panic!("unknown op"),
    }
}
