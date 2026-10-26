use indoorgen::assets::polyfill::quad_split_24_beauty;
fn main() {
    let c: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("tmp/sd/005_mesh_bridge_edge_loops.case.json").unwrap()).unwrap();
    let v = |i: usize| -> [f32; 3] { let a = &c["verts"][i]; [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32] };
    for q in [[390usize, 391, 320, 138], [391, 389, 375, 347], [389, 388, 162, 368], [388, 390, 145, 189]] {
        let mut out = vec![];
        for r in 0..4 {
            let qq = [q[r], q[(r + 1) % 4], q[(r + 2) % 4], q[(r + 3) % 4]];
            out.push(quad_split_24_beauty([v(qq[0]), v(qq[1]), v(qq[2]), v(qq[3])]));
        }
        println!("{:?} rotations split24 {:?}", q, out);
    }
}
