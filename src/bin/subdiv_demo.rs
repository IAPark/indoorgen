//! Builds a unit cube, applies bevel_then_subsurf(0.05, 3 segments, 30 deg, 1 level) and writes
//! it as OBJ to /tmp/subdiv_demo.obj.
use indoorgen::subdiv::{bevel_then_subsurf, QMesh};

fn main() {
    let cube = QMesh::cuboid([0.0; 3], [1.0; 3]);
    let m = bevel_then_subsurf(&cube, 0.05, 3, 30f64.to_radians(), 1);
    let path = "/tmp/subdiv_demo.obj";
    std::fs::write(path, m.to_obj()).expect("write obj");
    println!(
        "wrote {path}: {} vertices, {} faces, closed={}",
        m.pos.len(),
        m.faces.len(),
        m.is_closed()
    );
}
