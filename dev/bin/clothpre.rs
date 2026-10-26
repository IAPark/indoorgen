//! clothpre DUMPDIR KEY : rebuild the pre-sim sheet/cover mesh (blanket family) from the bounds of the python dump and compare vertex by vertex.
use indoorgen::assets::clothes::*;
use indoorgen::assets::pydump::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = PyDump::load(&a[1]);
    let key = &a[2];
    let pre = d.mesh(&format!("{key}.pre"));
    let kw = d.json(&format!("{key}.kw"));
    let name = kw["name"].as_str().unwrap().to_string();
    let kind = if name.starts_with("BoxComforter") { BlanketKind::BoxComforter } else if name.starts_with("Comforter") { BlanketKind::Comforter } else { BlanketKind::Blanket };
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in &pre.v { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    let width = hi[1] - lo[1];
    let size = hi[0] - lo[0];
    let b = Blanket { kind, width, size, thickness: 0.0, margin: 0.35 };
    let mut m = b.create_asset();
    let loc = [(hi[0] + lo[0]) / 2.0, 0.0, if kind == BlanketKind::Blanket { lo[2] } else { hi[2] }];
    apply_srt_f32(&mut m, [1.0; 3], std::f32::consts::FRAC_PI_2, [loc[0] as f32, loc[1] as f32, loc[2] as f32]);
    println!("{key} {name}: py verts {} faces {} | rs verts {} faces {}", pre.v.len(), pre.faces.len(), m.pos.len(), m.faces.len());
    if m.pos.len() == pre.v.len() {
        let mut e: f64 = 0.0;
        for (p, q) in pre.v.iter().zip(&m.pos) { for k in 0..3 { e = e.max((p[k] - q[k]).abs()); } }
        println!("  max coord err {:.2e}  faces identical: {}", e, pre.faces == m.faces);
    }
}
