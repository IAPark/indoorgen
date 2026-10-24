//! bedtable /tmp/f_solall.json out.json : edge order of Blender's SOLIDIFY NON_MANIFOLD output for the bed seat lattice, expressed in
//! the vertex indices of bed::seat_solid_params
use infinigen::assets::bed::seat_solid_params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut out = serde_json::Map::new();
    for (k, v) in d.as_object().unwrap() {
        let (nx, ny): (usize, usize) = { let mut it = k.split(','); (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap()) };
        let f = |n: &str| v[n].as_f64().unwrap();
        let m = seat_solid_params(nx, ny, f("W"), f("S"), f("T"), f("LT"));
        let pv: Vec<[f64; 3]> = v["verts"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()] }).collect();
        assert_eq!(pv.len(), m.pos.len(), "{k} vertex count");
        let map: Vec<usize> = pv.iter().map(|p| {
            let mut best = (1e9, 0);
            for (i, q) in m.pos.iter().enumerate() {
                let d = (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs();
                if d < best.0 { best = (d, i); }
            }
            assert!(best.0 < 1e-5, "{k} no match");
            best.1
        }).collect();
        let edges: Vec<[usize; 2]> = v["edges"].as_array().unwrap().iter().map(|e| { let e = e.as_array().unwrap(); [map[e[0].as_u64().unwrap() as usize], map[e[1].as_u64().unwrap() as usize]] }).collect();
        // faces check
        let mut fa: Vec<Vec<usize>> = v["polys"].as_array().unwrap().iter().map(|p| { let mut q: Vec<usize> = p.as_array().unwrap().iter().map(|i| map[i.as_u64().unwrap() as usize]).collect(); q.sort(); q }).collect();
        let mut fb: Vec<Vec<usize>> = m.faces.iter().map(|p| { let mut q: Vec<usize> = p.iter().map(|&i| i as usize).collect(); q.sort(); q }).collect();
        fa.sort(); fb.sort();
        assert_eq!(fa, fb, "{k} faces differ");
        out.insert(k.clone(), serde_json::json!(edges));
    }
    std::fs::write(&a[2], serde_json::Value::Object(out).to_string()).unwrap();
}
