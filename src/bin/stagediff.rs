//! stagediff DIR_RUST DIR_PY: compare every NNN_kind_out.bin of the Rust trace (MONOTRACE) with the python trace (tools/pytrace.py), in order.
use indoorgen::assets::stagefile::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (da, db) = (&a[1], &a[2]);
    let tol: f64 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(1e-6);
    let mut names: Vec<String> = std::fs::read_dir(da).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with("_out.bin")).collect();
    names.sort();
    let mut bad = 0;
    for n in names {
        let ra = read(&format!("{da}/{n}")).unwrap();
        let Some(rb) = read(&format!("{db}/{n}")) else {
            println!("MISSING in python: {n}");
            continue;
        };
        if !compare(&n, &ra.v, &ra.f, &rb, tol) {
            bad += 1;
            if a.iter().any(|x| x == "--first") {
                break;
            }
        }
    }
    println!("{} stages differ", bad);
}
