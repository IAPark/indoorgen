//! rsdump CLASS FSEED_START N INST OUT.jsonl : dump Factory::exact (or placeholder) meshes as pydump-compatible jsonl
use infinigen::assets::*;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let class = &a[1];
    let (s0, n, inst): (u64, u64, u64) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let mut out = std::fs::File::create(&a[5]).unwrap();
    for s in s0..s0 + n {
        let f = make_factory(class, s).unwrap();
        let e = match f.exact(inst) {
            Some(e) => e,
            None => {
                // fall back to the approximate asset surface (triangulated)
                let asset = f.asset(inst);
                let mut e = cabnodes::ExactMesh::default();
                for p in &asset.parts {
                    for t in p.mesh.triangles() {
                        let base = e.verts.len() as u32;
                        for k in 0..3 { e.verts.push(p.mesh.pos[t[k] as usize]); }
                        e.polys.push(vec![base, base + 1, base + 2]);
                    }
                }
                e
            }
        };
        let j = serde_json::json!({"kind": "asset", "class": class, "fseed": s, "inst": inst, "verts": e.verts, "polys": e.polys, "face_names": e.names});
        writeln!(out, "{}", j).unwrap();
    }
}
