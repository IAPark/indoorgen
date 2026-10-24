//! clothexp4 [KEYFILTER] : Rust solver on comforter-on-box scenes of tools/clothexp4.py vs Blender
use infinigen::assets::cloth::*;
use infinigen::assets::clothes::*;
fn main() {
    let filt = std::env::args().nth(1).unwrap_or_default();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("/Users/lume/work/agents/rng/cache/clothexp4.json").unwrap()).unwrap();
    let cal = Calib::default();
    let mut pos = vec![];
    for z in [-0.4, 0.0] { for y in [-0.6, 0.6] { for x in [-0.6, 0.6] { pos.push([x, y, z]); } } }
    let faces: Vec<Vec<u32>> = vec![vec![0, 2, 3, 1].into_iter().rev().collect(), vec![4, 5, 7, 6], vec![0, 1, 5, 4], vec![2, 6, 7, 3], vec![0, 4, 6, 2], vec![1, 3, 7, 5]];
    // fix winding so normals point outward: verify via signed volume
    let col = Collider::new(&pos, &faces);
    for (key, val) in j.as_object().unwrap() {
        if !key.contains(&filt) { continue; }
        let mut p = ClothParams::default();
        p.use_pressure = true; p.tension = 2.0; p.distance_min = 5e-3;
        for part in key.split('_') {
            let num: f64 = part[1..].parse().unwrap_or(0.0);
            match &part[..1] { "m" => p.mass = num, "g" => p.gravity = num, "p" => p.pressure = num, _ => {} }
        }
        let (mut g, e) = grid(26, 26);
        apply_srt_f32(&mut g, [0.6, 0.6, 1.0], 0.0, [0.0; 3]);
        let s = solidify_simple(&g, &e, 0.01, -1.0);
        let n = s.pos.len() / 2;
        let mut line = format!("{key}:");
        for f in [10u32, 25, 50] {
            p.end_frame = f;
            let out = simulate(&s.pos, &s.faces, &p, &[&col], &cal);
            let bl: Vec<[f64; 3]> = val[f.to_string()].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
            let topmax = |v: &[[f64; 3]]| v[..n].iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max);
            let botmean = |v: &[[f64; 3]]| v[n..].iter().map(|q| q[2]).sum::<f64>() / n as f64;
            let xr = |v: &[[f64; 3]]| { let a = v[n..].iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max); let b = v[..n].iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max); (a, b) };
            if f == 50 && std::env::var("PROFILE").is_ok() { for (nm, v) in [("rs", &out), ("bl", &bl)] { let row = 13usize; let w = 27usize; let top: Vec<String> = (0..w).step_by(2).map(|c| format!("{:.3}", v[row * w + c][2])).collect(); let bot: Vec<String> = (0..w).step_by(2).map(|c| format!("{:.3}", v[n + row * w + c][2])).collect(); println!("   {nm} top {:?}\n   {nm} bot {:?}", top, bot); } }
            if f == 50 { line += &format!(" [xmax bottom/top rs {:.3?} bl {:.3?}]", xr(&out), xr(&bl)); }
            line += &format!("  f{f}: top zmax rs {:.3} bl {:.3} | bot mean rs {:.3} bl {:.3}", topmax(&out), topmax(&bl), botmean(&out), botmean(&bl));
        }
        println!("{line}");
    }
}
