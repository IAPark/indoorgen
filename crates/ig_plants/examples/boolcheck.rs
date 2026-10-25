//! boolcheck CASEFILE a b c [--swap] : DIFFERENCE of probe a minus b, c (float solver) vs the expected mesh of a following probe
use ig_gn::geom::Mesh as GM;
use serde_json::Value as J;

fn mesh(j: &J) -> GM {
    let pos: Vec<[f32; 3]> = j["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32]).collect();
    let faces: Vec<Vec<u32>> = j["f"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let ed: Vec<[u32; 2]> = j["e"].as_array().map(|a| a.iter().map(|x| [x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32]).collect()).unwrap_or_default();
    GM::from_faces(pos, &faces, &ed)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: J = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let cs = j["cases"].as_array().unwrap();
    let idx: Vec<usize> = a[2..5].iter().map(|s| s.parse().unwrap()).collect();
    let ms: Vec<GM> = idx.iter().map(|&i| mesh(&cs[i]["expect"])).collect();
    let id: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let swap = a.iter().any(|s| s == "--swap");
    let ms: Vec<GM> = if std::env::var("CHAIN").is_ok() {
        // mesh 1 = Rust union of probes 0 and 9 (Rust edge order) instead of Blender's probe 10
        let u = ig_gn::fbool::mesh_boolean_float("UNION", &[(&mesh(&cs[0]["expect"]), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]), (&mesh(&cs[9]["expect"]), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]])]);
        let mut v = ms;
        v[0] = u;
        v
    } else { ms };
    let ops: Vec<(&GM, [[f32; 4]; 4])> = if swap { vec![(&ms[0], id), (&ms[2], id), (&ms[1], id)] } else { vec![(&ms[0], id), (&ms[1], id), (&ms[2], id)] };
    let opn = std::env::var("BOP").unwrap_or("DIFFERENCE".into());
    let ops: Vec<(&GM, [[f32; 4]; 4])> = if opn == "UNION" { ops[..2].to_vec() } else { ops };
    let r = ig_gn::fbool::mesh_boolean_float(&opn, &ops);
    println!("result {} verts {} faces", r.nv(), r.nf());
    if a.len() > 5 && !a[5].starts_with("--") {
        let e = mesh(&cs[a[5].parse::<usize>().unwrap()]["expect"]);
        println!("blender {} verts {} faces", e.nv(), e.nf());
        let be: Vec<(u32, u32)> = cs[a[5].parse::<usize>().unwrap()]["expect"]["e"].as_array().unwrap().iter().map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32)).collect();
        let re: Vec<(u32, u32)> = r.edges.iter().map(|x| (x[0], x[1])).collect();
        let n = |x: &(u32, u32)| (x.0.min(x.1), x.0.max(x.1));
        let first = be.iter().zip(re.iter()).position(|(x, y)| n(x) != n(y));
        let mut sa: Vec<_> = be.iter().map(n).collect();
        let mut sb: Vec<_> = re.iter().map(n).collect();
        sa.sort();
        sb.sort();
        println!("edges blender {} rust {} same set {} first order diff {:?}", be.len(), re.len(), sa == sb, first);
        if let Some(i) = first { println!("  blender {:?}\n  rust    {:?}", &be[i..(i + 6).min(be.len())], &re[i..(i + 6).min(re.len())]); }
    }
}
