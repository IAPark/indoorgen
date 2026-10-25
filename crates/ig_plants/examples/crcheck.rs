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

fn flat(j: &J, out: &mut Vec<f64>) {
    match j {
        J::Number(n) => out.push(n.as_f64().unwrap()),
        J::Array(a) => a.iter().for_each(|x| flat(x, out)),
        J::Object(m) => {
            let mut ks: Vec<&String> = m.keys().collect();
            ks.sort();
            for k in ks {
                flat(&m[k], out);
            }
        }
        _ => {}
    }
}

fn nurbs_json(p: &ig_plants::creature_nurbs::NurbsParams) -> J {
    serde_json::json!({"ts": p.ts, "rads": p.rads, "skeleton_root": p.skeleton_root, "skeleton_yoffs": p.skeleton_yoffs, "length": p.length, "proportions": p.proportions, "thetas": p.thetas, "profiles_norm": p.profiles_norm})
}

/// compare the Rust genome (python draw order) with the dumped part parameters
fn genome_check(dir: &str, fs: u64, herb: bool) {
    use ig_plants::creature_gen::*;
    let d: J = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/parts.json")).unwrap()).unwrap();
    let mut r0 = ig_kit::inst_rng(fs, 0);
    let i2 = r0.randint(0, 10_000_000) as u64;
    let mut r = ig_kit::inst_rng(fs, i2);
    let (g, root) = if herb { herbivore_genome(&mut r) } else { tiger_genome(&mut r) };
    let order = g.preorder(root);
    let parts = d["parts"].as_array().unwrap();
    println!("genome: {} nodes (python {} parts), i2 {}", order.len(), parts.len(), i2);
    for (k, &ni) in order.iter().enumerate() {
        let fac = &g.facs[g.nodes[ni].fac];
        let (cls, mine): (String, J) = match &fac.kind {
            Kind::NurbsBody(p) => ("NurbsBody".into(), nurbs_json(p)),
            Kind::NurbsHead(p) => ("NurbsHead".into(), nurbs_json(p)),
            k => (format!("{:?}", k), fac.params.clone()),
        };
        let pj = &parts[k];
        let pcls = pj["cls"].as_str().unwrap();
        let mut a = vec![];
        let mut b = vec![];
        flat(&mine, &mut a);
        flat(&pj["params"], &mut b);
        let mut mx = 0f64;
        if a.len() == b.len() {
            for (x, y) in a.iter().zip(b.iter()) {
                mx = mx.max((x - y).abs() / (1.0 + y.abs()));
            }
        }
        let att = &g.nodes[ni].att;
        let pa = &d["posed"][k]["att"];
        let mut amx = 0f64;
        if let Some(att) = att {
            let mut pc = vec![];
            flat(&pa["coord"], &mut pc);
            for c in 0..3 {
                amx = amx.max((att.coord[c] - pc[c]).abs());
            }
            if att.side as i64 != pa["side"].as_i64().unwrap() {
                amx = 99.0;
            }
        }
        if let Kind::NurbsBody(np) | Kind::NurbsHead(np) = &fac.kind {
            let t0 = std::time::Instant::now();
            let (m, sk) = ig_plants::creature::make_nurbs_part(np);
            let pm = &pj["mesh"];
            let sizes: Vec<i64> = pm["sizes"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
            let idx: Vec<i64> = pm["idx"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
            cmp(&format!("   nurbs mesh"), &m, &arr_f32(&pm["co"]), &sizes, &idx);
            let psk = pj["skeleton"].as_array().unwrap();
            let mut mx = 0f64;
            for (a, b) in sk.iter().zip(psk.iter()) {
                for c in 0..3 {
                    mx = mx.max((a[c] - b[c].as_f64().unwrap()).abs());
                }
            }
            println!("   skeleton rs {} py {} maxerr {:.2e} ({:?})", sk.len(), psk.len(), mx, t0.elapsed());
        }
        let ok = cls.starts_with(&pcls[..3.min(pcls.len())]) || cls == pcls;
        println!("  {k:2} {cls:14} py {pcls:18} params n {}/{} maxrel {:.1e}  att coord maxerr {:.1e} {}", a.len(), b.len(), mx, amx, if ok { "" } else { "CLASS?" });
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    if std::env::args().nth(2).as_deref() == Some("genome") {
        let fs: u64 = std::env::args().nth(3).unwrap().parse().unwrap();
        genome_check(&dir, fs, std::env::args().nth(4).as_deref() == Some("herb"));
        return;
    }
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
