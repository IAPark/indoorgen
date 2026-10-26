//! bmesh bevel port vs bpy.ops.mesh.bevel (cases from a Blender dump script)
use indoorgen::bm::bevel::*;
use indoorgen::bm::*;
use serde_json::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let only = a.get(2).cloned();
    let mut ok = 0;
    let mut tot = 0;
    for c in v.as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        if let Some(o) = &only {
            if o != name {
                continue;
            }
        }
        tot += 1;
        let pos: Vec<[f32; 3]> = c["pos"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32, p[2].as_f64().unwrap() as f32] }).collect();
        let edges: Vec<[u32; 2]> = c["edges"].as_array().unwrap().iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32] }).collect();
        let faces: Vec<Vec<u32>> = c["faces"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let ce: Vec<Vec<u32>> = c["ce"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let sel: Vec<usize> = c["sel"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
        let mut bm = BMesh::from_mesh(&pos, &edges, &faces, &ce);
        let o = BevelOpts { offset: c["offset"].as_f64().unwrap() as f32, offset_type: OffsetType::Offset, segments: c["seg"].as_i64().unwrap() as i32, profile: c["profile"].as_f64().unwrap() as f32, limit_offset: c["clamp"].as_bool().unwrap_or(false), loop_slide: true, mat_nr: -1 };
        let r = bevel_edges(&mut bm, &sel, &o);
        let (rp, _re, rf) = bm.to_mesh();
        let bp: Vec<Vec<f64>> = c["rpos"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()).collect();
        let bf: Vec<Vec<u32>> = c["rfaces"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        let mut maxd = 0f64;
        let np = rp.len().min(bp.len());
        for i in 0..np {
            for k in 0..3 {
                maxd = maxd.max((rp[i][k] as f64 - bp[i][k]).abs());
            }
        }
        let same_f = rf == bf;
        let good = r.is_ok() && rp.len() == bp.len() && rf.len() == bf.len() && maxd < 1e-5 && same_f;
        if good { ok += 1; }
        println!("{:24} {} v {}/{} f {}/{} maxd {:.2e} facesame {}{}", name, if good { "OK  " } else { "FAIL" }, rp.len(), bp.len(), rf.len(), bf.len(), maxd, same_f, match &r { Err(e) => format!(" ERR {}", e), _ => String::new() });
    }
    println!("{}/{}", ok, tot);
}
