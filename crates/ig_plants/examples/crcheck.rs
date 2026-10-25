//! crcheck DUMPDIR : compare Rust creature stages with tools/creaturedump.py output
use ig_plants::creature::*;
use serde_json::Value as J;

fn arr_f32(j: &J) -> Vec<f32> {
    j.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
}

fn cmp(name: &str, a: &CMesh, co: &[f32], sizes: &[i64], idx: &[i64]) {
    let nv = co.len() / 3;
    let mut line = format!("{name}: nv rs {} py {} nf rs {} py {}", a.pos.len(), nv, a.faces.len(), sizes.len());
    if a.pos.len() == nv {
        let mut mx = 0f32;
        for i in 0..nv {
            for k in 0..3 {
                mx = mx.max((a.pos[i][k] - co[i * 3 + k]).abs());
            }
        }
        line += &format!(" exact-order maxerr {:.2e}", mx);
        let bad: Vec<usize> = (0..nv).filter(|&i| (0..3).any(|k| (a.pos[i][k] - co[i * 3 + k]).abs() > 1e-5)).collect();
        if !bad.is_empty() {
            line += &format!(" [{} verts > 1e-5; first {:?}]", bad.len(), &bad[..bad.len().min(6)]);
            for &i in bad.iter().take(2) {
                line += &format!("\n     v{i} rs {:?} py {:?}", a.pos[i], &co[i * 3..i * 3 + 3]);
            }
        }
        let mut fo = 0;
        let mut same = true;
        for (fi, f) in a.faces.iter().enumerate() {
            let n = sizes.get(fi).copied().unwrap_or(-1) as usize;
            if n != f.len() || (0..n).any(|k| f[k] as i64 != idx[fo + k]) {
                same = false;
                break;
            }
            fo += n;
        }
        line += &format!(" faces {}", if same { "identical" } else { "differ" });
    } else {
        // nearest vertex distance (brute force on a coarse sort)
        let mut mx = 0f32;
        let mut sum = 0f64;
        for i in 0..a.pos.len().min(4000) {
            let p = a.pos[i * a.pos.len().max(1) / a.pos.len().min(4000).max(1)];
            let mut best = f32::MAX;
            for j in 0..nv {
                let d = (0..3).map(|k| (p[k] - co[j * 3 + k]).powi(2)).sum::<f32>();
                best = best.min(d);
            }
            mx = mx.max(best.sqrt());
            sum += best.sqrt() as f64;
        }
        line += &format!(" nn(sample) max {:.2e} mean {:.2e}", mx, sum / a.pos.len().min(4000).max(1) as f64);
    }
    println!("{line}");
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let only = std::env::args().nth(2);
    let d: J = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/parts.json")).unwrap()).unwrap();
    for (i, p) in d["parts"].as_array().unwrap().iter().enumerate() {
        let cls = p["cls"].as_str().unwrap();
        let top = match cls {
            "Tail" => "nodegroup_tail",
            "QuadrupedBackLeg" => "nodegroup_quadruped_back_leg",
            "QuadrupedFrontLeg" => "nodegroup_quadruped_front_leg",
            "CarnivoreHead" => "nodegroup_carnivore_head",
            "CarnivoreJaw" => "nodegroup_carnivore_jaw (no gc)",
            "Foot" => "nodegroup_foot",
            "CatEar" => "nodegroup_cat_ear",
            "CatNose" => "nodegroup_cat_nose",
            "MammalEye" => "nodegroup_mammal_eye (no gc)",
            _ => continue,
        };
        if let Some(o) = &only {
            if o != cls && o != &i.to_string() {
                continue;
            }
        }
        let t0 = std::time::Instant::now();
        let out = run_group(top, &p["params"]);
        let m = &p["mesh"];
        let sizes: Vec<i64> = m["sizes"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
        let idx: Vec<i64> = m["idx"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
        let empty = CMesh::default();
        let g = out.mesh("Geometry").unwrap_or(&empty);
        cmp(&format!("part {i} {cls}"), g, &arr_f32(&m["co"]), &sizes, &idx);
        // extras: python extras named like the group outputs (CatNose: the Geometry output becomes the "Nose" extra)
        for ex in p["extras"].as_array().unwrap() {
            let en = ex["name"].as_str().unwrap();
            let key = if cls == "CatNose" { "Geometry" } else { en.trim_end_matches(|c: char| c == '.' || c.is_ascii_digit()) };
            if let Some(g) = out.mesh(key) {
                let esz: Vec<i64> = ex["sizes"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
                let eidx: Vec<i64> = ex["idx"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
                cmp(&format!("   extra {en}"), g, &arr_f32(&ex["co"]), &esz, &eidx);
            }
        }
        if let Some(sk) = out.skeleton() {
            let psk = p["skeleton"].as_array().unwrap();
            println!("   skeleton rs {} py {}  ({:?})", sk.len(), psk.len(), t0.elapsed());
        }
    }
}
