//! matslots CLASS FSEED INST : per-slot material spec + face count of Factory::exact_slots (compare with python's material slots of the same instance)
use infinigen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (class, s, inst): (&str, u64, u64) = (&a[1], a[2].parse().unwrap(), a[3].parse().unwrap());
    let f = make_factory(class, s).unwrap();
    let Some((slots, mats)) = f.exact_slots(inst) else { println!("no exact_slots"); return };
    for (k, m) in mats.iter().enumerate() {
        let n = slots.iter().filter(|&&x| x as usize == k).count();
        if n > 0 {
            println!("slot {k:3} faces {n:6} {}", m.spec.as_ref().map(|s| s.to_string()).unwrap_or_default());
        }
    }
}
