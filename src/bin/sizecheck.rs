//! sizecheck CLASS[,CLASS..] [nseeds] : for each factory seed, the natural size (constructor draws only) and the extents of the asset
//! built to the natural size and to a perturbed size ([width (y), depth (x), height] = [hi.y-lo.y, hi.x-lo.x, hi.z-lo.z]); reports the
//! worst axis error per class (build-to-size check without running the scene generator)
use indoorgen::assets::*;
fn ext(a: &Asset) -> [f64; 3] {
    let (lo, hi) = a.bounds();
    [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]]
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: u64 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(12);
    let thr: f64 = std::env::var("THR").ok().and_then(|t| t.parse().ok()).unwrap_or(0.02);
    for class in a[1].split(',') {
        let mut worst = [0.0f64; 3];
        let (mut nlo, mut nhi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        let mut nonat = 0;
        let mut natmis = [0.0f64; 3];
        let fs0: u64 = std::env::var("FS0").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        for fs in fs0..fs0 + n {
            for inst in [0u64, 7] {
                ig_kit::api::set_size_hint(None);
                set_unseeded(indoorgen::assets::ctor_draws::ctor_draws(class, &mut indoorgen::rng::Rng::seeded(fs)));
                let Some(f) = make_factory(class, fs) else { println!("{class}: no factory"); return };
                let Some(nat) = f.natural_size(inst) else { nonat += 1; continue };
                let natonly = std::env::var_os("NATONLY").is_some();
                let free = if natonly { nat } else { ext(&scene_asset(&*f, inst)) };
                for k in 0..3 { nlo[k] = nlo[k].min(nat[k]); nhi[k] = nhi[k].max(nat[k]); natmis[k] = natmis[k].max((nat[k] - free[k]).abs()); }
                // decor classes are resized uniformly by the solver (their natural proportions stay); furniture axes move independently
                let uniform = std::env::var_os("UNIFORM").is_some() || matches!(class, "BottleFactory" | "CanFactory" | "CupFactory" | "JarFactory" | "PlateFactory" | "WineglassFactory" | "VaseFactory" | "DeskLampFactory" | "FloorLampFactory" | "MicrowaveFactory" | "OfficeChairFactory");
                if natonly { for k in 0..3 { nlo[k] = nlo[k].min(nat[k]); nhi[k] = nhi[k].max(nat[k]); } continue; }
                let fixed: Option<[f64; 3]> = std::env::var("HINT").ok().map(|v| { let p: Vec<f64> = v.split(',').map(|x| x.parse().unwrap()).collect(); [p[0], p[1], p[2]] });
                let hints = if let Some(f) = fixed { [f, f, f, f] } else if uniform { [nat, nat.map(|v| v * 1.15), nat.map(|v| v * 0.85), nat.map(|v| v * 1.3)] } else { [nat, [nat[0] * 1.15, nat[1] * 0.9, nat[2] * 1.05], [nat[0] * 0.8, nat[1] * 1.1, nat[2] * 0.95], [nat[0] * 1.1, nat[1] * 1.1, nat[2] * 1.1]] };
                for (hi_, h) in hints.iter().enumerate() {
                    ig_kit::api::set_size_hint(Some(*h));
                    ig_kit::api::set_round_hint(std::env::var_os("ROUND").is_some());
                    let f = make_factory(class, fs).unwrap();
                    let asset = scene_asset(&*f, inst);
                    let e = ext(&asset);
                    if std::env::var("VERBOSE").map_or(false, |v| v == "2") { println!("  {class} fs {fs} hint {:.3?} bounds {:.3?}", h, asset.bounds()); }
                    ig_kit::api::set_size_hint(None);
                    let err = [0, 1, 2].map(|k| (e[k] - h[k]).abs());
                    if err.iter().any(|v| *v > thr) && std::env::var_os("VERBOSE").is_some() { println!("  {class} fs {fs} inst {inst} hint#{hi_} {:.3?} built {:.3?}", h, e); }
                    for k in 0..3 { worst[k] = worst[k].max(err[k]); }
                }
            }
        }
        println!("{class}: natural range w {:.2}-{:.2} d {:.2}-{:.2} h {:.2}-{:.2}; natural vs free-built max err {:.3?}; build-to-size max err {:.3?}{}", nlo[0], nhi[0], nlo[1], nhi[1], nlo[2], nhi[2], natmis, worst, if nonat > 0 { format!(" (no natural size on {nonat})") } else { String::new() });
    }
}
