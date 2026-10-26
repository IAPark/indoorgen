//! ctorcheck CLASS NSEEDS : for global seeds g in 0..N run the emulated constructor draws and print `CLASS words gauss` per seed
//! (compare with tools/ctorlog.py --all, which seeds np.random.seed(g) and measures the real python constructors).
use indoorgen::assets::ctor_draws::{ctor_draws, words_between};
use indoorgen::rng::Rng;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let class = &a[1];
    let n: u64 = a[2].parse().unwrap();
    let mut out = vec![];
    for g in 0..n {
        let mut r = Rng::seeded(g);
        let before = r.clone();
        ctor_draws(class, &mut r);
        let k = words_between(&before, &r).unwrap_or(9999);
        // gaussian cache set? randn() then consumes nothing
        let mut c = r.clone();
        c.randn();
        let cached = words_between(&r, &c) == Some(0);
        out.push(format!("({k}, {})", cached as u8));
    }
    println!("{class} [{}]", out.join(", "));
}
