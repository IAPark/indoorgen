//! boolcheck CASEFILE a b c [--swap] : DIFFERENCE of probe a minus b, c (float solver) vs the expected mesh of a following probe
use ig_gn::geom::Mesh as GM;
use serde_json::Value as J;

fn mesh(j: &J) -> GM {
    let pos: Vec<[f32; 3]> = j["v"].as_array().unwrap().iter().map(|v| [v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32]).collect();
    let faces: Vec<Vec<u32>> = j["f"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    GM::from_faces(pos, &faces, &[])
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: J = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let cs = j["cases"].as_array().unwrap();
    let idx: Vec<usize> = a[2..5].iter().map(|s| s.parse().unwrap()).collect();
    let ms: Vec<GM> = idx.iter().map(|&i| mesh(&cs[i]["expect"])).collect();
    let id: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let swap = a.iter().any(|s| s == "--swap");
    let ops: Vec<(&GM, [[f32; 4]; 4])> = if swap { vec![(&ms[0], id), (&ms[2], id), (&ms[1], id)] } else { vec![(&ms[0], id), (&ms[1], id), (&ms[2], id)] };
    let r = ig_gn::fbool::mesh_boolean_float("DIFFERENCE", &ops);
    println!("result {} verts {} faces", r.nv(), r.nf());
    if a.len() > 5 && !a[5].starts_with("--") {
        let e = mesh(&cs[a[5].parse::<usize>().unwrap()]["expect"]);
        println!("blender {} verts {} faces", e.nv(), e.nf());
    }
}
