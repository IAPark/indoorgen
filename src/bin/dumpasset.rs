//! dumpasset CLASS fseed out.json : dump the Rust factory asset (inst 0) as {verts, polys}
use infinigen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (class, fs) = (&a[1], a[2].parse::<u64>().unwrap());
    if std::env::var("NOUNSEEDED").is_err() { set_unseeded(infinigen::assets::ctor_draws::ctor_draws(class, &mut infinigen::rng::Rng::seeded(fs))); }
    let f = make_factory(class, fs).expect("no factory");
    let asset = if std::env::var("PH").is_ok() { f.placeholder(0).expect("no placeholder") } else { f.asset(0) };
    let mut verts = vec![];
    let mut polys = vec![];
    if std::env::var("PARTS").is_ok() { let mut o = 0; for p in &asset.parts { eprintln!("part {}..{} ({} verts)", o, o + p.mesh.pos.len(), p.mesh.pos.len()); o += p.mesh.pos.len(); } }
    for p in &asset.parts {
        let off = verts.len();
        for v in &p.mesh.pos { verts.push(serde_json::json!([v[0], v[1], v[2]])); }
        for f in &p.mesh.faces { polys.push(serde_json::json!(f.iter().map(|i| *i as usize + off).collect::<Vec<_>>())); }
    }
    std::fs::write(&a[3], serde_json::json!({"verts": verts, "polys": polys}).to_string()).unwrap();
    let (lo, hi) = asset.bounds();
    println!("RSDUMP {} verts, bounds {:?} {:?}", verts.len(), lo, hi);
}
