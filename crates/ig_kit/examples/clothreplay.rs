// clothreplay DIR [JOB...]: re-run cloth jobs dumped with CLOTH_DUMP=DIR (solver from CL_SOLVER / CL_VBD env as in gen2) and print time
// and shape numbers: comforter-like jobs (gravity, colliders, 50 frames): drop below / loft above the first collider's top (the
// mattress) and quilting (std of the top-surface height over it); throw-like jobs (80 frames): share of vertices above the mattress top.
use ig_kit::cloth::{load_job, simulate};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&a[1]);
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "bin")).collect();
    files.sort();
    let only: Vec<String> = a[2..].to_vec();
    for f in files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        if !only.is_empty() && !only.iter().any(|o| name.contains(o.as_str())) { continue }
        let Some((pos, faces, p, cal, cols)) = load_job(&f) else { continue };
        let refs: Vec<&ig_kit::cloth::Collider> = cols.iter().collect();
        let t = std::time::Instant::now();
        let out = simulate(&pos, &faces, &p, &refs, &cal);
        let dt = t.elapsed().as_secs_f64();
        let kind = if cols.is_empty() { "free" } else if p.end_frame >= 80 { "throw" } else { "comforter" };
        let mut line = format!("{name} {kind:9} {} verts {:.2}s", pos.len(), dt);
        if std::env::var_os("PARAMS").is_some() { eprintln!("{name}: mass {} tension {} compr {} shear {} bend {} pressure {} gravity {} frames {} pins {} press_scale {} thr {} thr_g {}", p.mass, p.tension, p.compression, p.shear, p.bending, p.pressure, p.gravity, p.end_frame, p.pin.iter().filter(|&&w| w >= 0.999).count(), cal.press_scale, cal.press_thresh, cal.press_thresh_g); }
        if cols.is_empty() {
            // inflated closed shells (mattress, pillows): height and enclosed volume, before -> after
            let vol = |x: &[[f64; 3]]| faces.iter().map(|f| (1..f.len() - 1).map(|k| { let (a, b, c) = (x[f[0] as usize], x[f[k] as usize], x[f[k + 1] as usize]); a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]) }).sum::<f64>()).sum::<f64>().abs() / 6.0;
            let hz = |x: &[[f64; 3]]| x.iter().map(|q| q[2]).fold(f64::MIN, f64::max) - x.iter().map(|q| q[2]).fold(f64::MAX, f64::min);
            line += &format!(" | height {:.1} -> {:.1} cm volume {:.4} -> {:.4} m3", hz(&pos) * 100.0, hz(&out) * 100.0, vol(&pos), vol(&out));
        }
        if !cols.is_empty() {
            // first collider = the mattress (bed.rs order)
            let m = if kind == "throw" && cols.len() > 1 { &cols[1] } else { &cols[0] };
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for i in 0..m.len() { for q in m.tri(i) { for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); } } }
            let top = hi[2];
            let zmin = out.iter().map(|q| q[2]).fold(f64::MAX, f64::min);
            let zmax = out.iter().map(|q| q[2]).fold(f64::MIN, f64::max);
            let on_top = out.iter().filter(|q| q[2] > top).count() as f64 / out.len() as f64;
            // quilting: per 2 cm cell over the mattress the highest vertex; std of its height around a quadratic trend
            let mut cells: std::collections::HashMap<(i64, i64), [f64; 3]> = Default::default();
            for q in &out {
                if q[0] > lo[0] && q[0] < hi[0] && q[1] > lo[1] && q[1] < hi[1] && q[2] > top - 0.02 {
                    let k = (((q[0] - lo[0]) / 0.02) as i64, ((q[1] - lo[1]) / 0.02) as i64);
                    let e = cells.entry(k).or_insert(*q);
                    if q[2] > e[2] { *e = *q }
                }
            }
            let pts: Vec<[f64; 3]> = cells.values().copied().collect();
            let quilt = if pts.len() > 50 {
                // least squares z = c0 + c1 x + c2 y + c3 x^2 + c4 y^2 + c5 xy via normal equations
                let row = |q: &[f64; 3]| [1.0, q[0], q[1], q[0] * q[0], q[1] * q[1], q[0] * q[1]];
                let mut ata = [[0.0f64; 6]; 6];
                let mut atb = [0.0f64; 6];
                for q in &pts { let r = row(q); for i in 0..6 { atb[i] += r[i] * q[2]; for j in 0..6 { ata[i][j] += r[i] * r[j] } } }
                // gaussian elimination
                let mut m6 = ata; let mut b6 = atb;
                for c in 0..6 {
                    let piv = (c..6).max_by(|&x, &y| m6[x][c].abs().partial_cmp(&m6[y][c].abs()).unwrap()).unwrap();
                    m6.swap(c, piv); b6.swap(c, piv);
                    for r in 0..6 { if r != c && m6[c][c].abs() > 1e-18 { let fct = m6[r][c] / m6[c][c]; for k in 0..6 { m6[r][k] -= fct * m6[c][k] } b6[r] -= fct * b6[c]; } }
                }
                let coef: Vec<f64> = (0..6).map(|i| if m6[i][i].abs() > 1e-18 { b6[i] / m6[i][i] } else { 0.0 }).collect();
                let res: Vec<f64> = pts.iter().map(|q| { let r = row(q); q[2] - (0..6).map(|i| r[i] * coef[i]).sum::<f64>() }).collect();
                let mean = res.iter().sum::<f64>() / res.len() as f64;
                (res.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / res.len() as f64).sqrt()
            } else { 0.0 };
            // penetration: vertices more than 1 / 3 mm behind the nearest collider face (against its normal)
            let (mut p1, mut p3) = (0usize, 0usize);
            for q in &out {
                let mut best: Option<(f64, [f64; 3], usize, usize)> = None;
                for (ci, c) in cols.iter().enumerate() {
                    if let Some((d, cp, ti)) = c.closest(*q, best.map_or(0.1, |b| b.0)) {
                        if best.map_or(true, |b| d < b.0) { best = Some((d, cp, ti, ci)); }
                    }
                }
                if let Some((d, cp, ti, ci)) = best {
                    let n = cols[ci].normal(ti);
                    let s = (q[0] - cp[0]) * n[0] + (q[1] - cp[1]) * n[1] + (q[2] - cp[2]) * n[2];
                    if s < 0.0 && d > 0.001 { p1 += 1 }
                    if s < 0.0 && d > 0.003 { p3 += 1 }
                }
            }
            line += &format!(" | inside {:.2}% >1mm {:.2}% >3mm", 100.0 * p1 as f64 / out.len() as f64, 100.0 * p3 as f64 / out.len() as f64);
            if kind == "comforter" {
                line += &format!(" | drop {:.1} cm loft {:.1} cm quilt {:.1} mm", (top - zmin) * 100.0, (zmax - top) * 100.0, quilt * 1000.0);
            } else {
                line += &format!(" | on top {:.1}% height {:.1} cm", on_top * 100.0, (zmax - top) * 100.0);
            }
        }
        println!("{line}");
    }
}
