use indoorgen::assets::cabdoor::*;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut out = std::fs::File::create(&a[2]).unwrap();
    for line in std::fs::read_to_string(&a[1]).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        let q = &j["params"];
        let g = |k: &str| q[k].as_f64().unwrap();
        let p = DrawerParams { board_t: g("drawer_board_thickness"), board_w: g("drawer_board_width"), board_h: g("drawer_board_height"), depth: g("drawer_depth"), side_h: g("drawer_side_height"), width: g("drawer_width"), knob_r: g("knob_radius"), knob_len: g("knob_length") };
        let m = drawer_gmesh(&p).to_exact();
        let o = serde_json::json!({"kind": "asset", "class": "Drawer", "fseed": j["fseed"], "inst": 0, "verts": m.verts, "polys": m.polys, "face_names": m.names});
        writeln!(out, "{}", o).unwrap();
    }
}
