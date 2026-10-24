use infinigen::assets::cabdoor::*;
use infinigen::assets::cabnodes::*;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut out = std::fs::File::create(&a[1]).unwrap();
    let (h, w, e1, ew, e2, ang) = (1.3181475f64 as f32, 0.41205983f64 as f32, 0.01f32, 0.04622986f64 as f32, 0.006343066f64 as f32, 0.62003433f64 as f32);
    let o = RampedOpts::default();
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    let ramped = ramped_edge(h, e2, ew, e1, ang, o);
    let dr = double_rampled_edge(w, e2, ew, e1, ang, o);
    // panel_edge_frame
    let a_ = ramped_edge(h, e2, ew, e1, ang, o);
    let b_ = ramped_edge(w, e2, ew, e1, ang, o);
    let ma = w.mul_add(0.5, 0.001);
    let value = ma * -1.0;
    let t7 = b_.transformed([0.0, -0.0001, 0.0], z3, [0.9999, 1.0, 1.0]);
    let add_ = ma * 1.0 + -0.0001;
    let t3 = t7.transformed([add_, 0.0, h + 0.0001], [0.0, -A90, 0.0], one);
    let t2_ = t7.transformed([value + 0.0001, 0.0, 0.0], [0.0, A90, 0.0], one);
    let t0 = a_.translated([ma, 0.0, 0.0]);
    let t1_ = t0.transformed(z3, z3, [-1.0, 1.0, 1.0]);
    let frame = GMesh::join(&[t3, t2_, t1_, t0]);
    for (i, (n, g)) in [("ramped", ramped), ("dramped", dr), ("frame", frame)].into_iter().enumerate() {
        let m = g.to_exact();
        let o = serde_json::json!({"kind": "asset", "class": n, "fseed": i + 1, "inst": 0, "verts": m.verts, "polys": m.polys, "face_names": m.names});
        writeln!(out, "{}", o).unwrap();
    }
}
