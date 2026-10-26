//! clothexp3 [KEYFILTER] : Rust solver on the double-layer disc calibration scenes of tools/clothexp3.py vs Blender (z range at frames 10/25/50)
use indoorgen::assets::cloth::*;
use indoorgen::assets::clothes::*;
fn main() {
    let filt = std::env::args().nth(1).unwrap_or_default();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::env::var("EXP3").unwrap_or("/Users/lume/work/agents/rng/cache/clothexp3.json".into())).unwrap()).unwrap();
    let cal = Calib::default();
    for (key, val) in j.as_object().unwrap() {
        if !key.contains(&filt) { continue; }
        let mut p = ClothParams::default();
        p.gravity = 0.0;
        p.use_pressure = true;
        for part in key.split('_') {
            let num: f64 = part[1..].parse().unwrap_or(0.0);
            match &part[..1] {
                "m" => p.mass = num,
                "p" => p.pressure = num,
                "k" => p.tension = num,
                "c" => p.compression = num,
                _ => {}
            }
        }
        let (mut g, e) = grid(16, 16);
        apply_srt_f32(&mut g, [0.2, 0.2, 1.0], 0.0, [0.0; 3]);
        let s = solidify_simple(&g, &e, 0.007, 0.0);
        let mut line = format!("{key}:");
        for f in [10u32, 25, 50] {
            p.end_frame = f;
            let out = simulate(&s.pos, &s.faces, &p, &[], &cal);
            let bl: Vec<[f64; 3]> = val[f.to_string()].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
            let zr = |v: &[[f64; 3]]| v.iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max);
            let (sm, _, _) = surface_distance(&out, &s.faces, &bl, &s.faces);
            line += &format!("  f{f}: zmax rs {:.3} bl {:.3} (surf {:.3})", zr(&out), zr(&bl), sm);
        }
        println!("{line}");
    }
}
