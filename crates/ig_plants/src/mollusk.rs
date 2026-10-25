//! mollusk/snail.py + generate.py (MolluskFactory, snail family): cross-section ring -> ARRAY (object offset) -> bridge_edge_loops of all rings ->
//! scale / random rotation / centre -> geo_affine -> SOLIDIFY -> DISPLACE (legacy Stucci / Marble texture).
use super::bridge::*;
use super::legtex::*;
use super::*;
use crate::bl32;
use crate::dhash::HashMap;
use std::f64::consts::PI;

type M4 = bl32::M4;

fn mul_m4(a: &M4, b: &M4) -> M4 {
    // mul_m4_m4m4(R, A, B): R[j][i] = B[j][0]*A[0][i] + B[j][1]*A[1][i] + B[j][2]*A[2][i] + B[j][3]*A[3][i]
    let mut r = [[0.0f32; 4]; 4];
    for j in 0..4 {
        for i in 0..4 {
            r[j][i] = b[j][0] * a[0][i] + b[j][1] * a[1][i] + b[j][2] * a[2][i] + b[j][3] * a[3][i];
        }
    }
    r
}

fn mul_m4_v3(m: &M4, v: [f32; 3]) -> [f32; 3] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0] + m[3][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1] + m[3][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2] + m[3][2],
    ]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Maker {
    Volute,
    Nautilus,
    Snail,
    Conch,
    Auger,
}

pub struct SnailParams {
    pub lateral: f64,
    pub longitudinal: f64,
    pub freq: usize,
    pub scale: f64,
    pub loop_: usize,
    pub affine: f64,
    pub spike: f64,
}

fn solve_scale(shrink: f64, freq: f64) -> f64 {
    shrink.powf(1.0 / freq)
}
fn solve_lateral(ratio: f64, freq: usize, scale: f64) -> f64 {
    let s: f64 = (0..freq).map(|k| (2.0 * PI / freq as f64 * k as f64).sin() * scale.powi(k as i32)).sum();
    ratio / s
}
fn solve_longitude(ratio: f64, freq: usize, scale: f64) -> f64 {
    ratio * (1.0 + scale.powi(freq as i32)) / freq as f64
}

/// draws of the maker (volute_make, ...) up to and including the arguments of snail_make
pub fn maker_params(maker: Maker, r: &mut Rng) -> (SnailParams, bool) {
    let freq = 256usize;
    match maker {
        Maker::Snail => (SnailParams { lateral: 0.15, longitudinal: 0.04, freq: 28, scale: 0.99, loop_: 8, affine: 1.0, spike: 0.0 }, false),
        Maker::Conch => {
            let scale = solve_scale(r.uniform(0.7, 0.8), freq as f64);
            let lateral = solve_lateral(r.uniform(0.3, 0.4), freq, scale);
            let longitude = solve_longitude(r.uniform(0.7, 0.8), freq, scale);
            let loop_ = r.randint(8, 10) as usize;
            let affine = r.uniform(0.8, 0.9);
            (SnailParams { lateral, longitudinal: longitude, freq, scale, loop_, affine, spike: 0.1 }, false)
        }
        Maker::Auger => {
            let scale = solve_scale(r.uniform(0.7, 0.8), freq as f64);
            let lateral = solve_lateral(r.uniform(0.1, 0.15), freq, scale);
            let longitude = solve_longitude(r.uniform(0.9, 1.0), freq, scale);
            let loop_ = r.randint(8, 12) as usize;
            let affine = r.uniform(0.5, 0.6);
            (SnailParams { lateral, longitudinal: longitude, freq, scale, loop_, affine, spike: 0.0 }, false)
        }
        Maker::Volute => {
            let scale = solve_scale(r.uniform(0.5, 0.6), freq as f64);
            let lateral = solve_lateral(r.uniform(0.4, 0.5), freq, scale);
            let longitude = solve_longitude(r.uniform(0.6, 0.7), freq, scale);
            let loop_ = r.randint(4, 5) as usize;
            (SnailParams { lateral, longitudinal: longitude, freq, scale, loop_, affine: 1.0, spike: 0.0 }, false)
        }
        Maker::Nautilus => {
            let scale = solve_scale(r.uniform(0.4, 0.5), freq as f64);
            let lateral = solve_lateral(r.uniform(1.2, 1.4), freq, scale);
            let longitude = solve_longitude(r.uniform(0.2, 0.3), freq, scale);
            let loop_ = r.randint(4, 5) as usize;
            (SnailParams { lateral, longitudinal: longitude, freq, scale, loop_, affine: 1.0, spike: 0.0 }, false)
        }
    }
}

/// bridge_edge_loops over every ring (`select_all` edges): loops ordered by BM_mesh_edgeloops_calc_order, bridged pairwise
pub fn bridge_all_rings(m: &mut Mesh, rings: Vec<Vec<u32>>) {
    // centres (float32, edge-length weighted)
    let centre = |l: &Vec<u32>| -> [f32; 3] {
        let n = l.len();
        let p = |k: usize| -> [f32; 3] {
            let q = m.pos[l[k % n] as usize];
            [q[0] as f32, q[1] as f32, q[2] as f32]
        };
        let len = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2])).sqrt();
        let mut co = [0.0f32; 3];
        let mut totw = 0.0f32;
        let mut w_prev = len(p(n - 2 + n), p(n - 1));
        for step in 0..n {
            let k = (n - 1 + step) % n;
            let (cur, next) = (p(k), p(k + 1));
            let w_curr = len(cur, next);
            let w = w_curr + w_prev;
            for i in 0..3 {
                co[i] += cur[i] * w;
            }
            totw += w;
            w_prev = w_curr;
        }
        if totw != 0.0 {
            let f = 1.0f32 / totw;
            for i in 0..3 {
                co[i] *= f;
            }
        }
        co
    };
    let cents: Vec<[f32; 3]> = rings.iter().map(|l| centre(l)).collect();
    let tot = cents.len();
    let mut cent = [0.0f32; 3];
    for c in &cents {
        for i in 0..3 {
            cent[i] += c[i];
        }
    }
    for i in 0..3 {
        cent[i] *= 1.0f32 / tot as f32;
    }
    let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2]);
    let mut remaining: Vec<usize> = (0..tot).collect();
    let mut best = 0usize;
    let mut best_d = -1.0f32;
    for (k, &i) in remaining.iter().enumerate() {
        let d = d2(cent, cents[i]);
        if d > best_d {
            best_d = d;
            best = k;
        }
    }
    let mut order = vec![remaining.remove(best)];
    while !remaining.is_empty() {
        let last = cents[*order.last().unwrap()];
        let mut bk = 0usize;
        let mut bd = f32::MAX;
        for (k, &i) in remaining.iter().enumerate() {
            let d = d2(last, cents[i]);
            if d < bd {
                bd = d;
                bk = k;
            }
        }
        order.push(remaining.remove(bk));
    }
    let mut loops: Vec<Vec<u32>> = order.iter().map(|&i| rings[i].clone()).collect();
    let mut cache = None;
    for k in 0..loops.len() - 1 {
        let a = std::mem::take(&mut loops[k]);
        let b = std::mem::take(&mut loops[k + 1]);
        let (na, nb) = bridge_pair(m, a, b, 0, 0.0, &mut cache);
        loops[k] = na;
        loops[k + 1] = nb;
    }
}

/// snail_make + the first lines of SnailBaseFactory.create_asset (before SOLIDIFY)
pub fn snail_mesh(maker: Maker, r: &mut Rng) -> Mesh {
    let (p, _) = maker_params(maker, r);
    let n = 40usize;
    let resolution = p.loop_ * p.freq;
    let concave = r.uniform(1.9, 2.1);
    // build_cross_section
    let perturb = 1.0 / (5.0 * n as f64);
    let pert: Vec<f64> = (0..n).map(|_| r.uniform(-perturb, perturb)).collect();
    let sp: Vec<f64> = (0..n).map(|_| r.uniform(0.0, p.spike)).collect();
    let gate: Vec<f64> = (0..n).map(|_| r.uniform(0.0, 1.0)).collect();
    let mut ring = Mesh::new();
    for i in 0..n {
        let ang = (i as f64 / n as f64 + pert[i]) * 2.0 * PI;
        let mut rad = ang.cos().abs().powf(concave) + ang.sin().abs().powf(concave);
        rad *= 1.0 + sp[i] * if gate[i] < 0.2 { 1.0 } else { 0.0 };
        ring.pos.push([(ang.cos() * rad) as f32 as f64, (ang.sin() * rad * p.affine) as f32 as f64, 0.0]);
    }
    let rz = r.uniform(0.0, PI / 12.0);
    {
        let m = bl32::eul_to_mat3([0.0, 0.0, rz as f32]);
        for q in ring.pos.iter_mut() {
            let (x, y, z) = (q[0] as f32, q[1] as f32, q[2] as f32);
            for i in 0..3 {
                q[i] = (x * m[0][i] + y * m[1][i] + z * m[2][i]) as f64;
            }
        }
    }
    let sign = if r.choice_idx(2) == 0 { -1.0 } else { 1.0 };
    // ARRAY: constant offset (0,0,lateral) and object offset (empty)
    let mut offset: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    offset[3][2] += p.lateral as f32;
    let mut empty = bl32::Obj32::new([(p.longitudinal * sign) as f32, 0.0, 0.0], [(2.0 * PI / p.freq as f64) as f32, 0.0, 0.0]);
    empty.scale = [p.scale as f32; 3];
    let ident: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let result_mat = mul_m4(&mul_m4(&offset, &ident), &empty.matrix_world());
    let mut cur = ident;
    let mut out = Mesh::new();
    out.pos = ring.pos.clone();
    for c in 1..resolution {
        cur = mul_m4(&cur, &result_mat);
        for q in &ring.pos {
            let v = mul_m4_v3(&cur, [q[0] as f32, q[1] as f32, q[2] as f32]);
            out.pos.push([v[0] as f64, v[1] as f64, v[2] as f64]);
        }
        let _ = c;
    }
    // rings in edgeloops_find order: [v0, v(n-1), ..., v1] per copy
    let rings: Vec<Vec<u32>> = (0..resolution)
        .map(|c| {
            let base = (c * n) as u32;
            let mut l = vec![base];
            l.extend((1..n).rev().map(|k| base + k as u32));
            l
        })
        .collect();
    bridge_all_rings(&mut out, rings);
    out
}

fn bounds32(m: &Mesh) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &m.pos {
        for k in 0..3 {
            let v = p[k] as f32;
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }
    (lo, hi)
}

fn apply_m4(m: &mut Mesh, mat: &M4) {
    for q in m.pos.iter_mut() {
        let v = mul_m4_v3(mat, [q[0] as f32, q[1] as f32, q[2] as f32]);
        *q = [v[0] as f64, v[1] as f64, v[2] as f64];
    }
}

/// SnailBaseFactory.create_asset (maker, normalise, random rotation, centre, geo_affine)
pub fn snail_create_asset(maker: Maker, r: &mut Rng) -> Mesh {
    let mut m = snail_mesh(maker, r);
    // obj.scale = 1 / max(dimensions); obj.rotation_euler = uniform(0, 2 pi, 3); apply
    let (lo, hi) = bounds32(&m);
    let dmax = (0..3).map(|k| (hi[k] - lo[k]) as f64).fold(f64::MIN, f64::max);
    let s = (1.0 / dmax) as f32;
    let eul = [r.uniform(0.0, 2.0 * PI) as f32, r.uniform(0.0, 2.0 * PI) as f32, r.uniform(0.0, 2.0 * PI) as f32];
    let mut ob = bl32::Obj32::new([0.0; 3], eul);
    ob.scale = [s; 3];
    apply_m4(&mut m, &ob.matrix_world());
    // obj.location = -center(obj); location.z += dimensions.z * 0.4; apply
    let (lo, hi) = bounds32(&m);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let mut loc = [-c[0], -c[1], -c[2]];
    loc[2] = (loc[2] as f64 + (hi[2] - lo[2]) as f64 * 0.4) as f32;
    for q in m.pos.iter_mut() {
        for k in 0..3 {
            q[k] = (q[k] as f32 + loc[k]) as f64;
        }
    }
    // geo_affine
    let rows: Vec<[f32; 3]> = (0..3).map(|_| [r.uniform(-0.1, 0.1) as f32, r.uniform(-0.1, 0.1) as f32, r.uniform(-0.1, 0.1) as f32]).collect();
    // (the offset links from the DOT_PRODUCT 'Vector' output, which is disabled: the node tree adds a zero offset)
    let _ = rows;
    m
}

pub struct Snail {
    maker: Maker,
    fs: u64,
}

/// SnailBaseFactory.create_asset + MolluskFactory.decorate_mollusk
pub fn snail_final(maker: Maker, r: &mut Rng) -> Mesh {
    let stage: i32 = std::env::var("MOLSTAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(9);
    let mut m = if stage < 0 { snail_mesh(maker, r) } else { snail_create_asset(maker, r) };
    if stage >= 1 {
        // decorate_mollusk: SOLIDIFY(0.005) then DISPLACE
        m = books::solidify_simple(&m, 0.005, None);
    }
    if stage >= 2 {
        let st = if r.choice_idx(2) == 0 { TexKind::Stucci(0) } else { TexKind::Marble(0, 0) };
        let scale = r.log_uniform(0.1, 0.2);
        displace(&mut m, &Tex::new(st, scale), 0.02, 0.0);
    }
    m
}

impl Factory for Snail {
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fs, inst);
        let m = snail_final(self.maker, &mut r);
        let cn = match self.maker { Maker::Volute => "VoluteFactory", Maker::Nautilus => "NautilusFactory", Maker::Conch => "ConchFactory", Maker::Auger => "AugerFactory", Maker::Snail => "MolluskFactory" };
        let mat = crate::trinkets::mollusk_mat(&format!("mollusk.generate.{cn}"), "material", self.fs);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn handles(c: &str) -> bool {
    matches!(c, "AugerFactory" | "ConchFactory" | "VoluteFactory")
}

pub fn make(c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    let maker = match c {
        "AugerFactory" => Maker::Auger,
        "ConchFactory" => Maker::Conch,
        "VoluteFactory" => Maker::Volute,
        _ => return None,
    };
    Some(Box::new(Snail { maker, fs }))
}
