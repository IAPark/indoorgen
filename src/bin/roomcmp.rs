//! roomcmp: compare the Rust python-exact room mesh pipeline with stage dumps (tools/pyroom.py) / state0 fixtures.
//! usage: roomcmp stage <fixture_state0.json> <room> <pystages.json>
use infinigen::rooms::pyroom::*;
use serde_json::Value;

fn load(p: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}
fn f3(v: &Value) -> [f32; 3] {
    let a = v.as_array().unwrap();
    [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32]
}
fn cmp_stage(name: &str, m: &RawMesh, py: &Value) -> bool {
    let pv: Vec<[f32; 3]> = py["v"].as_array().unwrap().iter().map(f3).collect();
    let pp: Vec<Vec<u32>> = py["p"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    let mut ok = true;
    if pv.len() != m.pos.len() || pp.len() != m.faces.len() {
        println!("{name}: counts differ: verts rust {} py {}, faces rust {} py {}", m.pos.len(), pv.len(), m.faces.len(), pp.len());
        ok = false;
    }
    for (i, (a, b)) in m.pos.iter().zip(&pv).enumerate() {
        if a != b {
            println!("{name}: vert {i} differs: rust {:?} py {:?}", a, b);
            ok = false;
            break;
        }
    }
    for (i, (a, b)) in m.faces.iter().zip(&pp).enumerate() {
        if a != b {
            println!("{name}: face {i} differs: rust {:?} py {:?}", a, b);
            ok = false;
            break;
        }
    }
    if ok {
        println!("{name}: MATCH ({} verts, {} faces)", pv.len(), pp.len());
    }
    ok
}

fn consts(seed: u64) -> infinigen::rooms::consts::RoomConstants {
    let mut rng0 = infinigen::rng::Rng::seeded(seed);
    for _ in 0..42 {
        rng0.next_u32();
    }
    infinigen::rooms::consts::RoomConstants::sample(&mut rng0, 1)
}

fn smesh_to_raw(m: &infinigen::rooms::pysolid::SMesh) -> RawMesh {
    RawMesh { pos: m.pos.clone(), edges: m.edges.clone(), faces: m.faces.clone() }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "stage" => {
            let fx = load(&a[2]);
            let pyall = load(&a[3]);
            for (room, py) in pyall.as_object().unwrap() {
                let o = &fx["objs"][room.as_str()];
                let v = o["verts_world"].as_array().unwrap();
                let n = v.len() / 4;
                let ring: Vec<[f64; 2]> = v[..n].iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()] }).collect();
                println!("== {room}");
                let m = polygon2obj(&[ring]);
                cmp_stage("fill", &m, &py["fill"]);
            }
        }
        "consts" => {
            let c = consts(a[2].parse().unwrap());
            println!("{:?} {:?}", c.wall_thickness, c.wall_height);
        }
        "stages" => {
            // roomcmp stages <seed> <fixture> <pystages.json>
            let c = consts(a[2].parse().unwrap());
            let fx = load(&a[3]);
            let pyall = load(&a[4]);
            for (room, py) in pyall.as_object().unwrap() {
                let o = &fx["objs"][room.as_str()];
                let v = o["verts_world"].as_array().unwrap();
                let n = v.len() / 4;
                let ring: Vec<[f64; 2]> = v[..n].iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()] }).collect();
                println!("== {room}");
                let out = make_room(&[ring], c.wall_thickness, c.wall_height, &[]);
                for (name, m) in &out.stages {
                    cmp_stage(name, &smesh_to_raw(m), &py[*name]);
                }
                let tri: Vec<Vec<u32>> = out.tris.iter().map(|t| t.to_vec()).collect();
                cmp_stage("tri", &RawMesh { pos: out.pos.clone(), edges: vec![], faces: tri }, &py["tri"]);
                // fixture
                let fv: Vec<[f32; 3]> = o["verts_local"].as_array().unwrap().iter().map(f3).collect();
                let fp: Vec<Vec<u32>> = o["polys"].as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
                let mut ok = fv == out.pos;
                if !ok { let i = fv.iter().zip(&out.pos).position(|(a, b)| a != b); println!("fixture: verts differ at {:?} (len {} vs {})", i.map(|i| (i, fv[i], out.pos[i])), fv.len(), out.pos.len()); }
                let rt: Vec<Vec<u32>> = out.tris.iter().map(|t| t.to_vec()).collect();
                if fp != rt { ok = false; let i = fp.iter().zip(&rt).position(|(a, b)| a != b); println!("fixture: polys differ at {:?} (len {} vs {})", i.map(|i| (i, &fp[i], &rt[i])), fp.len(), rt.len()); }
                if ok { println!("fixture: MATCH"); }
            }
        }
        _ => eprintln!("usage"),
    }
}
