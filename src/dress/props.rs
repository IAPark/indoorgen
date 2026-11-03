//! Small procedural props the dressers need and the asset library lacks: candlestick, fireplace tool set, log basket / log stack,
//! utensil crock, dish rack, kettle, soap dispenser, toothbrush cup. Each is built in a local frame with z = 0 at its base, centred on
//! the origin, real-world size; materials are Infinigen specs (shaded in Cycles) or plain PBR colours.
use crate::assets::Asset;
use crate::mesh::{Material, Mesh};
use crate::rng::Rng;
use ig_kit::api::Part;
use ig_kit::geomkit::limb;
use serde_json::json;

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, rgb, 0.5);
    m.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}));
    m
}

fn metal(name: &str, rgb: [f32; 3], rough: f32) -> Material {
    let mut m = Material::diffuse(name, rgb, rough);
    m.metallic = 1.0;
    m
}

fn part(a: &mut Asset, mesh: Mesh, mat: &Material) { a.parts.push(Part { mesh, mat: mat.clone(), col: None }); }

/// brass / black / chrome finishes for small metal props
fn finish(r: &mut Rng, tag: &str) -> Material {
    match r.randint(0, 3) {
        0 => metal(&format!("{tag}_brass"), [0.78, 0.60, 0.34], 0.3),
        1 => metal(&format!("{tag}_black"), [0.04, 0.04, 0.04], 0.45),
        _ => metal(&format!("{tag}_chrome"), [0.80, 0.80, 0.82], 0.2),
    }
}

/// candlestick with a candle: 18-32 cm. Same seed = identical pair (mantel symmetry).
pub fn candlestick(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let m = finish(&mut r, "candlestick");
    let h = r.uniform(0.10, 0.20);
    let br = r.uniform(0.035, 0.05);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, 0.0), (br, 0.0), (br, 0.008), (br * 0.5, 0.02), (0.009, 0.03), (0.008, h - 0.01), (0.02, h), (0.02, h + 0.012), (0.0, h + 0.012)], 24), &m);
    let wax = Material::diffuse("candle_wax", [0.93, 0.91, 0.86], 0.6);
    let ch = r.uniform(0.08, 0.14);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, h + 0.012), (0.011, h + 0.012), (0.011, h + 0.012 + ch), (0.0, h + 0.014 + ch)], 16), &wax);
    a
}

/// fireplace tool set on a stand: base, post, top handle, poker / shovel / brush / tongs hung around it (70-80 cm)
pub fn tool_set(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let m = if r.f64() < 0.7 { metal("toolset_black", [0.03, 0.03, 0.03], 0.5) } else { metal("toolset_brass", [0.72, 0.55, 0.32], 0.35) };
    let h = r.uniform(0.68, 0.8);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, 0.0), (0.11, 0.0), (0.11, 0.012), (0.03, 0.02), (0.0, 0.025)], 24), &m);
    part(&mut a, limb([0.0, 0.0, 0.02], [0.0, 0.0, h], 0.008, 0.008, true), &m);
    part(&mut a, Mesh::ellipsoid([0.0, 0.0, h + 0.02], [0.025, 0.025, 0.025], 12, 8), &m);
    // hanging bar and four tools around the post
    part(&mut a, limb([-0.06, 0.0, h - 0.05], [0.06, 0.0, h - 0.05], 0.005, 0.005, true), &m);
    part(&mut a, limb([0.0, -0.06, h - 0.05], [0.0, 0.06, h - 0.05], 0.005, 0.005, true), &m);
    for (i, ang) in [0.0f64, 1.57, 3.14, 4.71].iter().enumerate() {
        let (x, y) = (0.06 * ang.cos(), 0.06 * ang.sin());
        let top = h - 0.06;
        part(&mut a, limb([x, y, 0.06], [x, y, top], 0.005, 0.005, true), &m);
        part(&mut a, Mesh::ellipsoid([x, y, top + 0.01], [0.012, 0.012, 0.018], 10, 6), &m);
        match i {
            0 => part(&mut a, limb([x, y, 0.06], [x + 0.03 * ang.cos(), y + 0.03 * ang.sin(), 0.09], 0.005, 0.003, true), &m), // poker hook
            1 => part(&mut a, Mesh::cuboid([x - 0.05, y - 0.004, 0.035], [x + 0.05, y + 0.004, 0.13]), &m), // shovel
            2 => part(&mut a, Mesh::cylinder(x, y, 0.035, 0.13, 0.03, 12), &Material::diffuse("toolset_bristle", [0.12, 0.09, 0.06], 0.9)), // brush
            _ => part(&mut a, limb([x + 0.01, y, 0.05], [x - 0.01, y, 0.05], 0.006, 0.006, true), &m), // tongs grip
        }
    }
    a
}

fn log(seed: u64, a: V3, b: V3, rad: f64) -> (Mesh, Material) {
    (limb(a, b, rad, rad * 0.95, true), spec(&format!("log:{seed}"), "wood.Wood", seed, [0.35, 0.25, 0.16]))
}
type V3 = [f64; 3];

/// woven log basket (40-48 cm across) holding 5-7 logs
pub fn log_basket(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let rad = r.uniform(0.2, 0.24);
    let h = r.uniform(0.24, 0.32);
    let weave = spec(&format!("basket:{seed}"), "fabric.CoarseKnitFabric", seed, [0.55, 0.42, 0.28]);
    // open-top wall with thickness + bottom
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, 0.0), (rad * 0.9, 0.0), (rad, h), (rad - 0.015, h), (rad * 0.9 - 0.015, 0.015), (0.0, 0.015)], 28), &weave);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(rad - 0.02, h - 0.01), (rad + 0.012, h - 0.01), (rad + 0.012, h + 0.012), (rad - 0.02, h + 0.012)], 28), &weave);
    let n = r.randint(5, 8);
    for i in 0..n {
        let lr = r.uniform(0.035, 0.05);
        let row = i / 3;
        let y = -rad * 0.55 + (i % 3) as f64 * rad * 0.55 + r.uniform(-0.02, 0.02);
        let z = h * 0.45 + row as f64 * 0.07 + lr;
        // (within the wall at its height: full-length logs came out through the basket's sides)
        let rin = (rad * 0.9 + 0.1 * rad * ((z - lr) / h).clamp(0.0, 1.0)) - 0.018;
        let yy = y.abs() + 0.04 + lr;
        let half = r.uniform(0.17, 0.22).min((rin * rin - yy * yy).max(0.0).sqrt());
        if half < 0.09 { continue }
        let (m, mat) = log(seed + i as u64, [-half, y, z], [half, y + r.uniform(-0.04, 0.04), z + r.uniform(-0.02, 0.03)], lr);
        part(&mut a, m, &mat);
    }
    a
}

/// stacked firewood (3-2-1 pyramid), 40-50 cm long
pub fn log_stack(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let len = r.uniform(0.4, 0.5);
    let mut i = 0u64;
    for (row, n) in [(0, 3), (1, 2), (2, 1)] {
        for j in 0..n {
            let lr = r.uniform(0.04, 0.05);
            let y = (j as f64 - (n - 1) as f64 / 2.0) * 0.1;
            let z = lr + row as f64 * 0.082;
            let (m, mat) = log(seed + i, [0.0, y, z], [len, y + r.uniform(-0.015, 0.015), z], lr);
            part(&mut a, m, &mat);
            i += 1;
        }
    }
    a
}

/// ceramic crock with 3-5 cooking utensils (spoon, spatula, ladle, whisk handle): 28-34 cm overall
pub fn utensil_crock(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let cr = r.uniform(0.05, 0.065);
    let ch = r.uniform(0.13, 0.16);
    let pot = spec(&format!("crock:{seed}"), "ceramic.Ceramic", seed, [0.85, 0.83, 0.78]);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, 0.0), (cr * 0.9, 0.0), (cr, ch * 0.5), (cr * 0.95, ch), (cr * 0.85, ch), (cr * 0.85, 0.01), (0.0, 0.01)], 24), &pot);
    let wood = spec(&format!("utensil_wood:{seed}"), "wood.Wood", seed + 1, [0.6, 0.45, 0.28]);
    let steel = metal("utensil_steel", [0.8, 0.8, 0.82], 0.25);
    let n = r.randint(3, 6);
    for i in 0..n {
        let ang = i as f64 / n as f64 * std::f64::consts::TAU + r.uniform(-0.3, 0.3);
        let lean = r.uniform(0.02, 0.05);
        let (x0, y0) = (0.02 * ang.cos(), 0.02 * ang.sin());
        let top = ch + r.uniform(0.12, 0.19);
        let (x1, y1) = (x0 + lean * ang.cos(), y0 + lean * ang.sin());
        let m = if r.f64() < 0.6 { &wood } else { &steel };
        part(&mut a, limb([x0, y0, 0.02], [x1, y1, top], 0.006, 0.006, true), m);
        match i % 3 {
            0 => part(&mut a, Mesh::ellipsoid([x1, y1, top + 0.03], [0.022, 0.012, 0.035], 10, 6), m), // spoon bowl
            1 => part(&mut a, Mesh::ellipsoid([x1, y1, top + 0.035], [0.032, 0.004, 0.042], 16, 8), m), // spatula blade
            _ => part(&mut a, Mesh::ellipsoid([x1, y1, top + 0.015], [0.035, 0.035, 0.02], 12, 6), m), // ladle
        }
    }
    a
}

/// wire dish rack (40 x 30 cm) on a drip tray with 2-4 plates standing and 1-2 cups
pub fn dish_rack(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let (w, d) = (r.uniform(0.38, 0.45), r.uniform(0.28, 0.32));
    let tray = Material::diffuse("rack_tray", if r.f64() < 0.5 { [0.9, 0.9, 0.9] } else { [0.15, 0.15, 0.15] }, 0.35);
    let wire = metal("rack_wire", [0.78, 0.78, 0.8], 0.3);
    part(&mut a, Mesh::cuboid([-w / 2.0, -d / 2.0, 0.0], [w / 2.0, d / 2.0, 0.018]).smoothed(0.006, 1, 0), &tray);
    // frame: 4 posts, top rails, plate dividers
    for (x, y) in [(-w / 2.0 + 0.01, -d / 2.0 + 0.01), (w / 2.0 - 0.01, -d / 2.0 + 0.01), (-w / 2.0 + 0.01, d / 2.0 - 0.01), (w / 2.0 - 0.01, d / 2.0 - 0.01)] {
        part(&mut a, limb([x, y, 0.018], [x, y, 0.12], 0.003, 0.003, false), &wire);
    }
    for y in [-d / 2.0 + 0.01, d / 2.0 - 0.01] { part(&mut a, limb([-w / 2.0 + 0.01, y, 0.12], [w / 2.0 - 0.01, y, 0.12], 0.003, 0.003, false), &wire); }
    let np = r.randint(2, 5);
    let pr = r.uniform(0.1, 0.12);
    let plate = spec(&format!("rack_plate:{seed}"), "ceramic.Ceramic", seed, [0.92, 0.92, 0.9]);
    for i in 0..np {
        let x = -w / 2.0 + 0.06 + i as f64 * 0.045;
        part(&mut a, limb([x - 0.006, 0.0, pr + 0.02], [x + 0.006, 0.0, pr + 0.02], pr, pr, true), &plate);
        part(&mut a, limb([x + 0.022, -d / 2.0 + 0.02, 0.018], [x + 0.022, -d / 2.0 + 0.02, 0.1], 0.0025, 0.0025, false), &wire);
    }
    let cup = spec(&format!("rack_cup:{seed}"), "ceramic.Ceramic", seed + 7, [0.8, 0.8, 0.78]);
    for i in 0..r.randint(1, 3) {
        let x = w / 2.0 - 0.06 - i as f64 * 0.09;
        part(&mut a, Mesh::revolve(x, d / 4.0 - 0.05, &[(0.0, 0.11), (0.04, 0.11), (0.035, 0.02), (0.0, 0.02)], 16), &cup);
    }
    a
}

/// kettle: stainless or enamel body, lid knob, spout, arched handle (22-28 cm)
pub fn kettle(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let body = if r.f64() < 0.5 { metal("kettle_steel", [0.82, 0.82, 0.84], 0.18) } else {
        let c = [[0.85, 0.12, 0.1], [0.92, 0.92, 0.9], [0.08, 0.08, 0.08], [0.55, 0.7, 0.75]][r.randint(0, 4) as usize];
        Material::diffuse("kettle_enamel", c, 0.25)
    };
    let black = Material::diffuse("kettle_black", [0.03, 0.03, 0.03], 0.5);
    let (rb, h) = (r.uniform(0.085, 0.1), r.uniform(0.15, 0.19));
    // (a round belly rising to the lid: five straight sections of profile showed as hard rings round the body)
    let mut prof = vec![(0.0, 0.0), (rb * 0.94, 0.0), (rb * 0.985, 0.004)];
    for k in 0..=16 {
        let t = k as f64 / 16.0;
        let r_ = if t < 0.35 { rb * (1.0 + 0.06 * (t / 0.35 * std::f64::consts::FRAC_PI_2).sin()) } else { (rb * 1.06 * ((t - 0.35) / 0.65 * std::f64::consts::FRAC_PI_2).cos().powf(0.75)).max(rb * 0.42) };
        prof.push((r_, 0.008 + t * (h - 0.008)));
    }
    for k in 1..=5 {
        let a_ = k as f64 / 5.0 * std::f64::consts::FRAC_PI_2;
        prof.push((rb * 0.42 * a_.cos(), h + 0.012 * a_.sin()));
    }
    part(&mut a, Mesh::revolve(0.0, 0.0, &prof, 40), &body);
    part(&mut a, Mesh::ellipsoid([0.0, 0.0, h + 0.015], [0.02, 0.02, 0.015], 12, 6), &black);
    part(&mut a, limb([rb * 0.8, 0.0, h * 0.35], [rb + 0.07, 0.0, h * 0.85], 0.02, 0.008, true), &body);
    let n = 16;
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let p = |t: f64| { let a_ = std::f64::consts::PI * t; [-rb * 0.42 * a_.cos(), 0.0, h * 0.9 + 0.08 * a_.sin()] };
        part(&mut a, limb(p(t0), p(t1), 0.009, 0.009, true), &black);
    }
    a
}

/// a dispenser's body: rounded foot, round shoulders up to the neck
fn soap_profile(rb: f64, h: f64) -> Vec<(f64, f64)> {
    let mut p = vec![(0.0, 0.0), (rb * 0.9, 0.0), (rb * 0.98, 0.002), (rb, 0.006)];
    for k in 0..=8 {
        let a = k as f64 / 8.0 * std::f64::consts::FRAC_PI_2;
        p.push((rb * 0.42 + rb * 0.58 * a.cos(), h * 0.82 + h * 0.18 * a.sin()));
    }
    p.push((0.0, h));
    p
}

/// pump soap dispenser (15-19 cm)
pub fn soap_dispenser(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let c = [[0.95, 0.95, 0.93], [0.25, 0.3, 0.28], [0.75, 0.62, 0.45], [0.2, 0.2, 0.22]][r.randint(0, 4) as usize];
    let body = Material::diffuse("soap_body", c, 0.3);
    let pump = metal("soap_pump", [0.82, 0.82, 0.84], 0.2);
    let (rb, h) = (r.uniform(0.03, 0.04), r.uniform(0.1, 0.13));
    part(&mut a, Mesh::revolve(0.0, 0.0, &soap_profile(rb, h), 32), &body);
    part(&mut a, limb([0.0, 0.0, h], [0.0, 0.0, h + 0.035], 0.008, 0.008, true), &pump);
    part(&mut a, limb([0.0, 0.0, h + 0.035], [0.035, 0.0, h + 0.03], 0.006, 0.005, true), &pump);
    a
}

/// toothbrush cup with two toothbrushes (cup 10 cm, brushes to 19 cm)
pub fn toothbrush_cup(seed: u64) -> Asset {
    let mut r = Rng::seeded(seed);
    let mut a = Asset::default();
    let cup = spec(&format!("tbcup:{seed}"), "ceramic.Ceramic", seed, [0.9, 0.9, 0.88]);
    part(&mut a, Mesh::revolve(0.0, 0.0, &[(0.0, 0.0), (0.032, 0.0), (0.036, 0.1), (0.031, 0.1), (0.028, 0.008), (0.0, 0.008)], 20), &cup);
    let bristle = Material::diffuse("tb_bristle", [0.95, 0.95, 0.95], 0.8);
    for i in 0..2 {
        let col = [[0.1, 0.4, 0.8], [0.85, 0.2, 0.3], [0.2, 0.7, 0.4], [0.9, 0.7, 0.1]][r.randint(0, 4) as usize];
        let hm = Material::diffuse("tb_handle", col, 0.35);
        let s = if i == 0 { 1.0 } else { -1.0 };
        let (x1, z1) = (s * 0.03, 0.19);
        part(&mut a, limb([s * 0.008, 0.0, 0.01], [x1, 0.0, z1], 0.005, 0.004, true), &hm);
        part(&mut a, Mesh::cuboid([x1 - 0.006, -0.012, z1], [x1 + 0.006, 0.0, z1 + 0.025]), &bristle);
    }
    a
}
