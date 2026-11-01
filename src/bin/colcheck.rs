//! Piece contract check (ig_kit::piece): for every class, over seeds and sizes, the final mesh lies inside the collision.
//! `colcheck [--seeds N] [--class NAME]...` -- one line per class: native or fallback, worst rigid / soft overshoot (cm), leaves without
//! a door part, loose sides (> 3 cm), and FAIL when a rigid vertex is more than 1 mm outside. Exit code 1 if any class fails.
use ig_kit::piece::{rigid_bounds, Prepared, Role, Volume};

const TOL: f64 = 0.001;
const LOOSE: f64 = 0.03;

fn main() {
    indoorgen::assets::install_pieces();
    let mut seeds = 6u64;
    let mut only: Vec<String> = vec![];
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() { "--seeds" => { seeds = args[i + 1].parse().unwrap(); i += 2; } "--class" => { only.push(args[i + 1].clone()); i += 2; } _ => { i += 1; } }
    }
    let classes: Vec<&str> = include_str!("colcheck_classes.txt").lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    println!("{:28} {:8} {:>5} {:>9} {:>9} {:>7} {:>6}  {}", "class", "kind", "runs", "rigid cm", "soft cm", "leaves", "loose", "verdict");
    let mut failed = 0;
    for c in classes {
        if !only.is_empty() && !only.iter().any(|o| o == c) { continue }
        let mut runs = 0;
        let (mut rigid_out, mut soft_out, mut bad_leaf, mut loose_n) = (0.0f64, 0.0f64, 0usize, 0usize);
        let mut native = None;
        let mut loose_note = String::new();
        let mut panic_note = None;
        for s in 1..=seeds {
            let Some(p0) = indoorgen::assets::pieces::any_piece(c, s * 7919 + 13) else { break };
            native = Some(p0.native());
            // its own size, and the class size model's extremes when it resizes
            let mut pieces: Vec<Box<dyn ig_kit::piece::Piece>> = vec![];
            if let Some(m) = indoorgen::furn::sizes::model(c) {
                for x in [m.min, m.max] { if let Some(p) = p0.resize(x) { pieces.push(p); } }
            }
            pieces.insert(0, p0);
            for mut p in pieces {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let vols = p.collision();
                    let mesh = p.mesh();
                    check(&vols, &mesh)
                }));
                match r {
                    Ok((ro, so, bl, lo)) => {
                        runs += 1;
                        rigid_out = rigid_out.max(ro); soft_out = soft_out.max(so); bad_leaf += bl;
                        if !lo.is_empty() { loose_n += 1; if loose_note.is_empty() { loose_note = lo; } }
                    }
                    Err(e) => { panic_note = Some(e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()); }
                }
            }
        }
        let Some(native) = native else { println!("{c:28} {:8}", "no piece"); continue };
        let fail = rigid_out > TOL || soft_out > TOL || bad_leaf > 0 || panic_note.is_some();
        if fail { failed += 1; }
        println!("{c:28} {:8} {runs:5} {:9.1} {:9.1} {bad_leaf:7} {loose_n:6}  {}{}{}", if native { "native" } else { "fallback" }, rigid_out * 100.0, soft_out * 100.0,
            if fail { "FAIL" } else { "ok" }, if loose_n > 0 { format!("  loose: {loose_note}") } else { String::new() }, panic_note.map_or(String::new(), |m| format!("  PANIC: {m}")));
    }
    if failed > 0 { eprintln!("{failed} classes fail"); std::process::exit(1); }
}

/// (worst rigid overshoot, worst soft overshoot, leaves holding no mesh part, loose sides description)
fn check(vols: &[Volume], mesh: &ig_kit::api::Asset) -> (f64, f64, usize, String) {
    let rigid: Vec<Prepared> = vols.iter().filter(|v| v.is_rigid()).map(Prepared::new).collect();
    let softp: Vec<Prepared> = vols.iter().filter(|v| v.is_rigid() || matches!(v.role, Role::Soft)).map(Prepared::new).collect();
    let out = |set: &[Prepared], p: [f64; 3]| -> f64 { set.iter().map(|v| v.outside(p)).fold(f64::INFINITY, f64::min) };
    let (mut ro, mut so) = (0.0f64, 0.0f64);
    let (mut mlo, mut mhi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (i, part) in mesh.parts.iter().enumerate() {
        let soft = mesh.is_soft(i);
        for p in &part.mesh.pos {
            if soft { so = so.max(out(&softp, *p)); } else {
                ro = ro.max(out(&rigid, *p));
                for k in 0..3 { mlo[k] = mlo[k].min(p[k]); mhi[k] = mhi[k].max(p[k]); }
            }
        }
    }
    // each leaf's closed panel holds at least one mesh part (it is a real door or drawer)
    let mut bad = 0;
    for v in vols {
        if let Role::Opening(leaf) = &v.role {
            let panel = match leaf { ig_kit::piece::Leaf::Hinged { panel, .. } | ig_kit::piece::Leaf::Drawer { panel, .. } => *panel };
            let holds = mesh.parts.iter().any(|pt| { let (a, b) = pt.mesh.bounds(); (0..3).all(|k| a[k] >= panel.0[k] - 0.01 && b[k] <= panel.1[k] + 0.01) });
            if !holds { bad += 1; }
        }
    }
    // loose: a side of the rigid bounds more than 3 cm past the rigid mesh
    let (clo, chi) = rigid_bounds(vols);
    let names = ["-x", "-y", "-z", "+x", "+y", "+z"];
    let gaps = [mlo[0] - clo[0], mlo[1] - clo[1], mlo[2] - clo[2], chi[0] - mhi[0], chi[1] - mhi[1], chi[2] - mhi[2]];
    let loose: Vec<String> = (0..6).filter(|&k| gaps[k] > LOOSE).map(|k| format!("{} {:.0}cm", names[k], gaps[k] * 100.0)).collect();
    (ro, so, bad, loose.join(" "))
}
