//! clothstand CLASS SEED DUMP.json : standalone Rust factory (stub-material draw order) vs python pydump (verts/polys)
use infinigen::assets::clothes::*;
use infinigen::rng::Rng;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[2].parse().unwrap();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[3]).unwrap()).unwrap();
    let mut r = Rng::seeded(seed);
    let kind = match a[1].as_str() { "BlanketFactory" => BlanketKind::Blanket, "ComforterFactory" => BlanketKind::Comforter, _ => BlanketKind::BoxComforter };
    let mut b = Blanket::from_draws(kind, &mut r);
    if kind == BlanketKind::BoxComforter { b.draw_margin(&mut r); }
    let m = b.create_asset();
    let pv = j["verts"].as_array().unwrap();
    let pp = j["polys"].as_array().unwrap();
    println!("{} {seed}: py verts {} polys {} | rs verts {} polys {}", a[1], pv.len(), pp.len(), m.pos.len(), m.faces.len());
    if pv.len() == m.pos.len() {
        let mut e: f64 = 0.0;
        for (p, q) in pv.iter().zip(&m.pos) { for k in 0..3 { e = e.max((p[k].as_f64().unwrap() - q[k]).abs()); } }
        let same = pp.len() == m.faces.len() && pp.iter().zip(&m.faces).all(|(x, y)| x.as_array().unwrap().iter().map(|t| t.as_u64().unwrap() as u32).collect::<Vec<_>>() == *y);
        println!("  max coord err {:.2e}  polys identical {}", e, same);
    }
}
