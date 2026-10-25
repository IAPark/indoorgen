//! import every avatar (FBX -> avatar.bin) and print a summary
use ig_people::avatar::*;
fn main() {
    let names: Vec<String> = std::env::args().skip(1).collect();
    let names = if names.is_empty() { list_avatars() } else { names };
    for n in names {
        let dir = root_dir().join(&n);
        let t = std::time::Instant::now();
        let a = match import_fbx(&dir, &n) { Ok(a) => a, Err(e) => { println!("{n}: ERR {e}"); continue } };
        let ti = t.elapsed().as_secs_f64();
        save_bin(&a, &dir.join("avatar.bin")).unwrap();
        let t = std::time::Instant::now();
        let b = load_bin(&dir, &n).unwrap();
        let tl = t.elapsed().as_secs_f64();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &b.parts { for v in &p.pos { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } } }
        let parts: Vec<String> = b.parts.iter().map(|p| format!("{}:{:?}[{}|{}|{}] v{} t{}", p.name, p.kind, p.color, p.normal, p.spec, p.pos.len(), p.tris.len())).collect();
        println!("{n}: import {ti:.2}s load {tl:.3}s bones {} lo {:.2?} hi {:.2?}\n   {}", b.bones.len(), lo, hi, parts.join("\n   "));
    }
}
