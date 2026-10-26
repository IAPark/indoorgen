use indoorgen::assets::cabdoor::*;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut out = std::fs::File::create(&a[2]).unwrap();
    for line in std::fs::read_to_string(&a[1]).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        let q = &j["params"];
        let g = |k: &str| q[k].as_f64().unwrap();
        let p = DoorParams { height: g("door_height"), width: g("door_width"), e1: g("edge_thickness_1"), ew: g("edge_width"), e2: g("edge_thickness_2"), ang: g("edge_ramp_angle"), knob_r: g("knob_R"), knob_len: g("knob_length"), has_mid: q["has_mid_ramp"].as_bool().unwrap(), left: q["door_left_hinge"].as_bool().unwrap() };
        let m = door_gmesh(&p, RampedOpts::default()).to_exact();
        let o = serde_json::json!({"kind": "asset", "class": "Door", "fseed": j["fseed"], "inst": 0, "verts": m.verts, "polys": m.polys, "face_names": m.names});
        writeln!(out, "{}", o).unwrap();
    }
}
