//! matcheck CLASS... : for seeds 0..40 print next f64 after Cls()() (compare with tools/matnext.py)
use infinigen::assets::matdraw;
use infinigen::rng::Rng;
fn main() {
    for name in std::env::args().skip(1) {
        for s in 0..40u64 {
            let mut r = Rng::seeded(s);
            matdraw::init(&mut r, &name);
            matdraw::call(&mut r, &name);
            println!("MAT {} {} {:.12}", name, s, r.f64());
        }
    }
}
