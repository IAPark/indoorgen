//! Readers / comparators for python stage dumps (tools/pytrace.py, RCv1 format) used to validate ported bpy ops one by one.
pub struct Stage {
    pub v: Vec<[f32; 3]>,
    pub f: Vec<Vec<u32>>,
}

pub fn read(path: &str) -> Option<Stage> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 8 || &b[0..4] != b"RCv1" {
        return None;
    }
    let hl = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let h: serde_json::Value = serde_json::from_slice(&b[8..8 + hl]).ok()?;
    let nv = h["nv"].as_u64()? as usize;
    let nf = h["nf"].as_u64()? as usize;
    let mut o = 8 + hl;
    let f32at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut v = Vec::with_capacity(nv);
    for i in 0..nv {
        v.push([f32at(o + 12 * i), f32at(o + 12 * i + 4), f32at(o + 12 * i + 8)]);
    }
    o += 12 * nv;
    let sizes: Vec<usize> = (0..nf).map(|i| u32at(o + 4 * i) as usize).collect();
    o += 4 * nf;
    let mut f = Vec::with_capacity(nf);
    for s in sizes {
        f.push((0..s).map(|k| u32at(o + 4 * k)).collect());
        o += 4 * s;
    }
    Some(Stage { v, f })
}

pub fn read_f32(path: &str) -> Option<Vec<f32>> {
    let b = std::fs::read(path).ok()?;
    Some(b.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// Compare a Rust result against a reference stage; prints one line and returns true if (vertex-order) exact within `tol`.
pub fn compare(name: &str, rv: &[[f32; 3]], rf: &[Vec<u32>], r: &Stage, tol: f64) -> bool {
    let mut line = format!("{name}: verts {} vs {} faces {} vs {}", rv.len(), r.v.len(), rf.len(), r.f.len());
    let mut ok = false;
    if rv.len() == r.v.len() {
        let mut me = 0f64;
        for (a, b) in rv.iter().zip(&r.v) {
            for k in 0..3 {
                me = me.max((a[k] as f64 - b[k] as f64).abs());
            }
        }
        let same_f = rf.len() == r.f.len() && rf.iter().zip(&r.f).all(|(a, b)| a == b);
        line += &format!(" | order err {:.3e} faces identical {}", me, same_f);
        ok = me <= tol && same_f;
    }
    let va: Vec<[f64; 3]> = rv.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    let vb: Vec<[f64; 3]> = r.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    if let Some((mean, mx)) = super::stagecmp::surface_dist(&va, rf, &vb, &r.f, 30000) {
        line += &format!(" | surface mean {:.3e} max {:.3e}", mean, mx);
        if !ok && mx <= tol.max(1e-6) {
            ok = true;
        }
    }
    println!("{} {}", if ok { "PASS" } else { "FAIL" }, line);
    ok
}
