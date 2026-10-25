//! sanity table over all avatars x poses: seat heights, contact values, timings
use ig_people::*;
fn main() {
    let n = avatar_count();
    let t = std::time::Instant::now();
    let mut tot = 0;
    for &k in ALL_POSES.iter() {
        let (mut seat_lo, mut seat_hi) = (9.0f64, -9.0f64);
        let (mut h_lo, mut h_hi) = (9.0f64, -9.0f64);
        let (mut fz_hi, mut bad) = (-9.0f64, 0);
        let mut hz = (9.0f64, -9.0f64);
        for a in 0..n {
            let i = pose_info(a, k);
            tot += 1;
            let all: Vec<f64> = i.lo.iter().chain(i.hi.iter()).cloned().chain(i.seat.iter().flatten().cloned()).collect();
            if all.iter().any(|x| !x.is_finite()) { bad += 1; }
            if let Some(s) = i.seat { seat_lo = seat_lo.min(s[2]); seat_hi = seat_hi.max(s[2]); }
            if std::env::var("VERBOSE").is_ok() { println!("  {:?} av{a} {:?} top {:.2} seat {:?} feety {:?}", k, ig_people::body::avatar_name(a), i.hi[2], i.seat.map(|s| (s[1]*100.0).round()/100.0), i.feet.map(|f| f[0][1])); }
            h_lo = h_lo.min(i.hi[2]); h_hi = h_hi.max(i.hi[2]);
            if let Some(f) = i.feet { fz_hi = fz_hi.max(f[0][2]).max(f[1][2]); }
            if let Some(h) = i.hands { hz.0 = hz.0.min(h[0][2].min(h[1][2])); hz.1 = hz.1.max(h[0][2].max(h[1][2])); }
        }
        println!("{k:?}: seat z [{seat_lo:.2},{seat_hi:.2}] top z [{h_lo:.2},{h_hi:.2}] sole z max {fz_hi:.3} hands z [{:.2},{:.2}] bad {bad}", hz.0, hz.1);
    }
    println!("pose_info cold: {:.1} ms each ({tot} calls)", t.elapsed().as_secs_f64() * 1000.0 / tot as f64);
    let t = std::time::Instant::now();
    let mut cnt = 0;
    for a in 0..n { for &k in ALL_POSES.iter() { let _ = build_person(a, k, 7); cnt += 1; } }
    println!("build_person: {:.1} ms each ({cnt} calls)", t.elapsed().as_secs_f64() * 1000.0 / cnt as f64);
}
