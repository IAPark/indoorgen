//! refcmp: compare Rust assets against cached python reference meshes (tools/refcache.py) -- no Blender needed.
//!
//! usage: refcmp CLASS [SEEDS] [--inst I] [--dir DIR] [--tol-mean 1e-5] [--tol-max 1e-4] [--quiet]
//!   SEEDS like 0-5 or 0,3,7 (default: every cached seed). CLASS is the python class name (e.g. ChairFactory).
//! Per case: vertex/face counts, exact-order max coordinate error (when counts match), whether polygon lists are
//! identical, and symmetric nearest-surface distances (probes = vertices + edge midpoints, mean/p95/max both ways).
//! Exit code 1 if any case exceeds the tolerances (surface mean/max) or is missing.
use infinigen::assets::*;
use infinigen::rng::Rng;
use nalgebra as na;
use parry3d_f64::query::PointQuery;
use parry3d_f64::shape::TriMesh;

struct M {
    v: Vec<[f64; 3]>,
    f: Vec<Vec<u32>>,
}

fn read_ref(path: &str) -> Option<(serde_json::Value, M)> {
    let b = std::fs::read(path).ok()?;
    if &b[0..4] != b"RCv1" {
        return None;
    }
    let hl = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let h: serde_json::Value = serde_json::from_slice(&b[8..8 + hl]).ok()?;
    if h["too_big"].as_bool().unwrap_or(false) {
        return Some((h, M { v: vec![], f: vec![] }));
    }
    let nv = h["nv"].as_u64()? as usize;
    let nf = h["nf"].as_u64()? as usize;
    let mut o = 8 + hl;
    let f32at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as f64;
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
    Some((h, M { v, f }))
}

fn trimesh(m: &M) -> Option<TriMesh> {
    let pts: Vec<na::Point3<f64>> = m.v.iter().map(|p| na::Point3::new(p[0], p[1], p[2])).collect();
    let mut idx = vec![];
    for f in &m.f {
        for k in 1..f.len().saturating_sub(1) {
            idx.push([f[0], f[k], f[k + 1]]);
        }
    }
    if idx.is_empty() {
        return None;
    }
    TriMesh::new(pts, idx).ok()
}

fn probes(m: &M) -> Vec<[f64; 3]> {
    let mut p: Vec<[f64; 3]> = m.v.clone();
    for f in &m.f {
        for k in 0..f.len() {
            let (a, b) = (m.v[f[k] as usize], m.v[f[(k + 1) % f.len()] as usize]);
            p.push([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]);
        }
    }
    let cap = 40_000;
    if p.len() > cap {
        let step = (p.len() + cap - 1) / cap;
        p = p.into_iter().step_by(step).collect();
    }
    p
}

fn dists(from: &M, to: &TriMesh) -> (f64, f64, f64) {
    let mut d: Vec<f64> = probes(from).iter().map(|p| {
        let q = na::Point3::new(p[0], p[1], p[2]);
        (to.project_local_point(&q, false).point - q).norm()
    }).collect();
    if d.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mean = d.iter().sum::<f64>() / d.len() as f64;
    d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (mean, d[(d.len() * 95 / 100).min(d.len() - 1)], *d.last().unwrap())
}

fn bbox(m: &M) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in &m.v {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

fn parse_ints(s: &str) -> Vec<u64> {
    let mut out = vec![];
    for part in s.split(',') {
        if let Some((a, b)) = part.split_once('-') {
            out.extend(a.parse::<u64>().unwrap()..=b.parse::<u64>().unwrap());
        } else if !part.is_empty() {
            out.push(part.parse().unwrap());
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        eprintln!("usage: refcmp CLASS [SEEDS] [--inst I] [--dir DIR] [--tol-mean X] [--tol-max X] [--quiet]");
        std::process::exit(2);
    }
    std::env::set_var("REFSTUB", "1");
    let opt = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
    let cls = &a[1];
    let dir = opt("--dir").unwrap_or_else(|| format!("{}/work/refcache", std::env::var("HOME").unwrap()));
    let tol_mean: f64 = opt("--tol-mean").map_or(1e-5, |s| s.parse().unwrap());
    let tol_max: f64 = opt("--tol-max").map_or(1e-4, |s| s.parse().unwrap());
    let inst_filter: Option<u64> = opt("--inst").map(|s| s.parse().unwrap());
    let quiet = a.iter().any(|x| x == "--quiet");
    let seeds: Option<Vec<u64>> = a.get(2).filter(|s| !s.starts_with("--")).map(|s| parse_ints(s));
    let mut files: Vec<(u64, u64, String)> = vec![];
    for e in std::fs::read_dir(format!("{dir}/{cls}")).unwrap_or_else(|_| panic!("no cache dir {dir}/{cls}: run tools/refcache.py first")) {
        let p = e.unwrap().path();
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if let Some((s, i)) = name.split_once('_') {
            let (s, i): (u64, u64) = (s.parse().unwrap(), i.parse().unwrap());
            if seeds.as_ref().map_or(true, |v| v.contains(&s)) && inst_filter.map_or(true, |x| x == i) {
                files.push((s, i, p.to_string_lossy().to_string()));
            }
        }
    }
    files.sort();
    let (mut n, mut ok, mut exact) = (0, 0, 0);
    let mut worst = (0.0f64, 0.0f64);
    for (s, i, path) in &files {
        n += 1;
        let (h, py) = read_ref(path).expect("bad cache file");
        let t0 = std::time::Instant::now();
        let mut rng = Rng::seeded(*s);
        set_unseeded(ctor_draws::ctor_draws(cls, &mut rng));
        let f = match make_factory(cls, *s) {
            Some(f) => f,
            None => {
                println!("{cls} {s}_{i}: NO RUST FACTORY");
                continue;
            }
        };
        let asset = f.asset(*i);
        if let Some(d) = opt("--dump") {
            std::fs::create_dir_all(&d).unwrap();
            for (k, p) in asset.parts.iter().enumerate() {
                let v: Vec<_> = p.mesh.pos.iter().map(|v| serde_json::json!(v)).collect();
                let f: Vec<_> = p.mesh.faces.iter().map(|f| serde_json::json!(f)).collect();
                std::fs::write(format!("{d}/{cls}_{s}.{k}.json"), serde_json::json!({"verts": v, "polys": f}).to_string()).unwrap();
            }
        }
        let secs = t0.elapsed().as_secs_f64();
        let mut rs = M { v: vec![], f: vec![] };
        for p in &asset.parts {
            let off = rs.v.len() as u32;
            rs.v.extend(p.mesh.pos.iter().cloned());
            rs.f.extend(p.mesh.faces.iter().map(|fc| fc.iter().map(|x| x + off).collect::<Vec<u32>>()));
        }
        if h["too_big"].as_bool().unwrap_or(false) {
            println!("{cls} {s}_{i}: reference too big to cache (nv {}); rust nv {}", h["nv"], rs.v.len());
            continue;
        }
        let same_n = py.v.len() == rs.v.len();
        let maxerr = if same_n {
            py.v.iter().zip(&rs.v).map(|(p, q)| (0..3).map(|k| (p[k] - q[k]).abs()).fold(0.0, f64::max)).fold(0.0, f64::max)
        } else {
            f64::NAN
        };
        let polys_same = py.f == rs.f;
        let (pt, rt) = (trimesh(&py), trimesh(&rs));
        let (a2b, b2a) = match (&pt, &rt) {
            (Some(pt), Some(rt)) => (dists(&rs, pt), dists(&py, rt)),
            _ => ((f64::NAN, f64::NAN, f64::NAN), (f64::NAN, f64::NAN, f64::NAN)),
        };
        let mean = a2b.0.max(b2a.0);
        let mx = a2b.2.max(b2a.2);
        let (plo, phi) = bbox(&py);
        let (rlo, rhi) = bbox(&rs);
        let bb = (0..3).map(|k| (plo[k] - rlo[k]).abs().max((phi[k] - rhi[k]).abs())).fold(0.0, f64::max);
        let pass = mean <= tol_mean && mx <= tol_max;
        if pass {
            ok += 1;
        }
        if same_n && polys_same && maxerr <= 1e-6 {
            exact += 1;
        }
        if mean.is_finite() {
            worst = (worst.0.max(mean), worst.1.max(mx));
        }
        if !quiet || !pass {
            println!(
                "{cls} {s}_{i}: {} nv py {} rs {}{} nf py {} rs {} polys {} | surf mean {:.1e} p95 {:.1e} max {:.1e} | bbox {:.1e} | rust {:.2}s py {}s",
                if pass { "PASS" } else { "FAIL" },
                py.v.len(),
                rs.v.len(),
                if same_n { format!(" (order maxerr {maxerr:.1e})") } else { String::new() },
                py.f.len(),
                rs.f.len(),
                if polys_same { "identical" } else { "differ" },
                mean,
                a2b.1.max(b2a.1),
                mx,
                bb,
                secs,
                h["secs"]
            );
        }
    }
    println!("SUMMARY {cls}: {ok}/{n} surface-pass (mean<={tol_mean:e}, max<={tol_max:e}), {exact}/{n} exact order; worst mean {:.1e} max {:.1e}", worst.0, worst.1);
    if ok < n || n == 0 {
        std::process::exit(1);
    }
}
