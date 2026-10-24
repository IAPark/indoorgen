use infinigen::assets::bathroom::Toilet;
use infinigen::assets::stagecmp::surface_dist;
use infinigen::assets::toiletx;
use infinigen::rng::Rng;
use serde_json::Value;

fn load(j: &Value, k: &str) -> Option<(Vec<[f64; 3]>, Vec<Vec<u32>>)> {
    let s = j.get(k)?;
    let v = s["v"].as_array()?.iter().map(|p| { let p = p.as_array().unwrap(); [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()] }).collect();
    let f = s["p"].as_array()?.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    Some((v, f))
}

fn report(name: &str, got: (&[[f64; 3]], &[Vec<u32>]), j: &Value, key: &str) {
    let Some((v, f)) = load(j, key) else { println!("{name}: no stage {key}"); return };
    let ok_counts = format!("verts {}/{} faces {}/{}", got.0.len(), v.len(), got.1.len(), f.len());
    // wire stages (no faces): compare vertices directly
    if f.is_empty() && got.1.is_empty() {
        let mut md = 0.0f64;
        for (a, b) in got.0.iter().zip(v.iter()) {
            for k in 0..3 {
                md = md.max((a[k] - b[k]).abs());
            }
        }
        println!("{name:10} {ok_counts} wire maxdiff {md:.3e}");
        return;
    }
    match surface_dist(got.0, got.1, &v, &f, 3000) {
        Some((mean, mx)) => println!("{name:10} {ok_counts} mean {mean:.3e} max {mx:.3e} {}", if mean < 1e-5 && mx < 1e-4 { "OK" } else { "FAIL" }),
        None => println!("{name:10} {ok_counts} (empty)"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let inst: u64 = a[2].parse().unwrap();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&a[3]).unwrap()).unwrap();
    let mut rng = Rng::seeded(seed);
    let t = Toilet::new(&mut rng);
    let mut r = infinigen::assets::inst_rng(seed, inst);
    let st = toiletx::tube_bridge(&t, &mut r);
    report("upper", (&st.upper, &[]), &j, "upper");
    report("lower", (&st.lower, &[]), &j, "lower");
    report("bottom", (&st.bottom, &[]), &j, "bottom");
    report("bridge", (&st.bridged.pos, &st.bridged.faces), &j, "e05_edit");
    let sol = toiletx::solidify_tube(&t, &st.bridged);
    report("solidify", (&sol.pos, &sol.faces), &j, "m06_SOLIDIFY");
    let tube = toiletx::extrude_top(&t, &sol);
    if std::env::var("VDBG").is_ok() {
        let (pv, _) = load(&j, "tube").unwrap();
        let mut worst: Vec<(f64, usize)> = pv.iter().enumerate().map(|(i, p)| (tube.pos.iter().map(|q| ((q[0]-p[0]).powi(2)+(q[1]-p[1]).powi(2)+(q[2]-p[2]).powi(2)).sqrt()).fold(f64::MAX, f64::min), i)).collect();
        worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for (d, i) in worst.iter().take(8) { println!("py vertex {} {:?} nearest rust vertex dist {:.4e}", i, pv[*i], d); }
        let nz = tube.pos.iter().filter(|p| (p[2] - t.extrude_height).abs() < 1e-6).count();
        let nzp = pv.iter().filter(|p| (p[2] - t.extrude_height).abs() < 1e-6).count();
        println!("verts at clip z: rust {} py {}", nz, nzp);
    }
    report("tube", (&tube.pos, &tube.faces), &j, "tube");
}
