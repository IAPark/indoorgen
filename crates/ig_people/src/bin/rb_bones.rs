use ig_people::avatar::*;
fn main() {
    let n = std::env::args().nth(1).unwrap();
    let a = load(&n).unwrap();
    for (i, b) in a.bones.iter().enumerate() {
        if b.name.contains("Finger") && !b.name.contains("Finger0") && !b.name.ends_with("1") && !b.name.ends_with("2") { }
        if b.name.contains("Finger") || b.name.contains("Eye") || b.name.contains("Mouth") || b.name.contains("Brow") || b.name.contains("Cheek")|| b.name.contains("Lip") { continue }
        let p = b.pos;
        println!("{i:2} {:<22} parent {:2} pos [{:6.3} {:6.3} {:6.3}]", b.name, b.parent, p[0], p[1], p[2]);
    }
}
