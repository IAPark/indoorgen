fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut v = vec![];
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        if j["fseed"] == 1 { v = j["verts"].as_array().unwrap().iter().take(128).map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32)).collect(); break; }
    }
    let sy = v[0].1;
    let sx = -v[32].0;
    let n = 128usize;
    let pi = std::f64::consts::PI;
    let pif = std::f32::consts::PI;
    let phid_vars: Vec<(&str, f32)> = vec![("f32 2*PI/n", 2.0f32 * pif / n as f32), ("dbl->f32", (2.0 * pi / n as f64) as f32), ("2f*(M_PI/n)", (2.0f32 as f64 * (pi / n as f64)) as f32)];
    for (nm, phid) in &phid_vars {
        for mode in 0..2 {
            for fnmode in 0..3 {
                let mut bad = 0;
                let mut acc = 0.0f32;
                for k in 0..n {
                    let phi = if mode == 0 { acc } else { k as f32 * phid };
                    acc += phid;
                    let (s, c) = match fnmode { 0 => (phi.sin(), phi.cos()), 1 => ((phi as f64).sin() as f32, (phi as f64).cos() as f32), _ => ((phi as f64).sin() as f32, (phi as f64).cos() as f32) };
                    let (x, y) = ((-s) * sx, c * sy);
                    if x != v[k].0 || y != v[k].1 { bad += 1; }
                }
                println!("phid {nm} mode {mode} fn {fnmode}: bad {bad}");
            }
        }
    }
    for k in 0..n {
        let ph = 2.0 * pi * k as f64 / n as f64;
        let (s, c) = (ph.sin() as f32, ph.cos() as f32);
        let (x, y) = ((-s) * sx, c * sy);
        if x != v[k].0 || y != v[k].1 {
            // which neighbours of s,c reproduce the data
            let nb = |a: f32| [f32::from_bits(a.to_bits().wrapping_sub(1)), a, f32::from_bits(a.to_bits().wrapping_add(1))];
            let sok: Vec<i32> = (0..3).filter(|&i| ((-nb(s)[i]) * sx) == v[k].0).map(|i| i as i32 - 1).collect();
            let cok: Vec<i32> = (0..3).filter(|&i| (nb(c)[i] * sy) == v[k].1).map(|i| i as i32 - 1).collect();
            println!("k {k}: exact sin {:.12e} (f32 {:e}) ok-ulp-offsets sin {:?} cos {:?}; exact cos {:.12e} f32 {:e}", ph.sin(), s, sok, cok, ph.cos(), c);
        }
    }
    // octant-symmetric variants
    for var in 0..6 {
        let mut bad = 0;
        for k in 0..n {
            let (mut kk, mut ss, mut sc, mut sw) = (k as i32, 1.0f32, 1.0f32, false);
            let nn = n as i32;
            if kk * 2 > nn { kk = nn - kk; ss = -1.0; }
            if kk * 4 > nn { kk = nn / 2 - kk; sc = -1.0; }
            if kk * 8 > nn { kk = nn / 4 - kk; sw = true; }
            let phi: f32 = match var {
                0 => 2.0f32 * pif * kk as f32 / n as f32,
                1 => (2.0f32 * pif) * (kk as f32 / n as f32),
                2 => (2.0 * pi * kk as f64 / n as f64) as f32,
                3 => kk as f32 * (2.0f32 * pif / n as f32),
                4 => (kk as f32 / n as f32) * (2.0f32 * pif),
                _ => 2.0f32 * (pif * kk as f32 / n as f32),
            };
            let (mut s, mut c) = (phi.sin(), phi.cos());
            if sw { std::mem::swap(&mut s, &mut c); }
            let (s, c) = (ss * s, sc * c);
            let (x, y) = ((-s) * sx, c * sy);
            if x != v[k].0 || y != v[k].1 { bad += 1; }
        }
        println!("octant var {var}: bad {bad}");
    }
    // direct: derived from double angle
    for (nm, f) in [("dbl", 0), ("dbl_via_f32_mult", 1)] {
        let mut bad = 0;
        for k in 0..n {
            let ph = if f == 0 { 2.0 * pi * k as f64 / n as f64 } else { (k as f32 * phid_vars[1].1) as f64 };
            let (s, c) = (ph.sin() as f32, ph.cos() as f32);
            let (x, y) = ((-s) * sx, c * sy);
            if x != v[k].0 || y != v[k].1 { bad += 1; }
        }
        println!("{nm}: bad {bad}");
    }
}
#[allow(dead_code)]
fn unused() {}
