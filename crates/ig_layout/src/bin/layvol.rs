//! layvol CLASS [SEEDS] [--tucked N]: a piece's collision volumes, as `lay` sees them: extents per role, for finding out why it will not stand somewhere.
use ig_kit::piece::{rigid_bounds, Role};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n: u64 = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(6);
    ig_assets::pieces::install_pieces();
    for seed in 0..n {
        let Some(mut p) = ig_kit::piece::make_piece(&a[0], seed * 7919 + 1) else { println!("{} has no piece", a[0]); return };
        let vols = p.collision();
        let (lo, hi) = rigid_bounds(&vols);
        println!("seed {seed}: rigid {:.2?}..{:.2?}", lo, hi);
        for v in &vols {
            let (l, h) = v.bounds();
            let r = match v.role { Role::Hard => "hard", Role::Support => "support", Role::Soft => "soft", Role::Opening(_) => "opening", Role::Inset => "inset" };
            println!("   {r:8} {:.2?}..{:.2?}", l, h);
        }
    }
}
