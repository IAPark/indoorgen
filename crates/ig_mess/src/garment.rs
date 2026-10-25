//! Dropped clothes (owner: mess-clutter worker). Shirt / pants outlines follow python's ShirtFactory / PantsFactory anchors
//! (clothes/shirt.py, pants.py): a polygon, mirrored about x = 0, filled with a 2.5 cm triangulation (CDT with interior points) and
//! solidified. The result is the pre-simulation mesh of a cloth job (python ClothesCover: dropped 0.2 m above the support,
//! cloth_sim(mass=0.05, tension_stiffness=2, distance_min=5e-3), subsurf 2); a height-field drape of it is the default mesh when
//! Blender does not re-simulate.
use ig_core::materials::spec_material;
use ig_core::mesh::{Material, Mesh, V3};
use ig_core::rng::Rng;
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Shirt,
    Pants,
}

pub struct Garment {
    pub kind: Kind,
    /// flat solid in the XY plane, centred on the bounding box, z in [-t/2, t/2]
    pub mesh: Mesh,
    pub thickness: f64,
    pub mat: Material,
}

fn polygon(rng: &mut Rng, kind: Kind) -> (Vec<[f64; 2]>, f64) {
    let pi = std::f64::consts::PI;
    match kind {
        Kind::Shirt => {
            let width = rng.log_uniform(0.45, 0.55);
            let size = width + rng.uniform(0.25, 0.3);
            let size_neck = rng.uniform(0.1, 0.15) * size;
            let long = rng.choice_idx(2) == 1;
            let sleeve_length = size / 2.0 + if long { rng.uniform(-0.05, 0.0) } else { rng.uniform(-0.35, -0.3) };
            let sleeve_width = rng.uniform(0.14, 0.18);
            let ang = rng.uniform(pi / 6.0, pi / 4.0);
            let thickness = rng.log_uniform(0.02, 0.03);
            let xs = [
                0.0,
                width / 2.0,
                width / 2.0,
                width / 2.0 + sleeve_length * ang.sin(),
                width / 2.0 + sleeve_length * ang.sin() + sleeve_width * ang.cos(),
                width / 2.0,
                width / 4.0,
                0.0,
            ];
            let y2 = size - sleeve_width / ang.sin();
            let ys = [
                0.0,
                0.0,
                y2,
                y2 - sleeve_length * ang.cos(),
                y2 - sleeve_length * ang.cos() + sleeve_width * ang.sin(),
                size,
                size + size_neck,
                size + size_neck * rng.uniform(0.3, 0.7),
            ];
            let mut p: Vec<[f64; 2]> = (0..8).map(|i| [xs[i], ys[i]]).collect();
            for i in (1..7).rev() {
                p.push([-xs[i], ys[i]]);
            }
            (p, thickness)
        }
        Kind::Pants => {
            let width = rng.log_uniform(0.45, 0.55);
            let size = width / 2.0 + rng.uniform(0.0, 0.05);
            let length = match rng.choice_idx(3) {
                0 => size + rng.uniform(-0.02, 0.02),
                1 => size + rng.uniform(0.05, 0.1),
                _ => size + rng.uniform(0.5, 0.7),
            };
            let shrink = rng.uniform(0.1, 0.15);
            let thickness = rng.log_uniform(0.02, 0.03);
            let h = [[0.0, 0.0], [width / 2.0, 0.0], [width / 2.0 * (1.0 + shrink), -length], [width / 2.0 * shrink * 2.0, -length], [0.0, -size]];
            let mut p: Vec<[f64; 2]> = h.to_vec();
            for i in (1..4).rev() {
                p.push([-h[i][0], h[i][1]]);
            }
            (p, thickness)
        }
    }
}

fn pt_in(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn edge_dist(p: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    let mut best = f64::MAX;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
        best = best.min(((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt());
    }
    best
}

/// materials.fabrics (+ ArtFabric weight 5): same weights as python's `material_assignments.pants`
fn fabric_material(rng: &mut Rng, name: &str) -> Material {
    let w = [1.0, 1.0, 0.3, 1.0, 5.0];
    let tot: f64 = w.iter().sum();
    let cls = ["fabric.CoarseKnitFabric", "fabric.FineKnitFabric", "fabric.Leather", "fabric.SofaFabric", "art.ArtFabric"][rng.choice_p(&w.iter().map(|x| x / tot).collect::<Vec<_>>())];
    let seed = rng.randint(0, 1_000_000);
    spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.55, 0.5, 0.6])
}

pub fn make(rng: &mut Rng, kind: Kind) -> Garment {
    let (mut poly, thickness) = polygon(rng, kind);
    let area = ig_core::shp::signed_area(&poly);
    if area < 0.0 {
        poly.reverse();
    }
    // centre on the bounding box
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &poly {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    for p in &mut poly {
        p[0] -= c[0];
        p[1] -= c[1];
    }
    // boundary points every <= 3 cm
    let step = 0.03;
    let mut pts: Vec<[f64; 2]> = vec![];
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let k = (len / step).ceil().max(1.0) as usize;
        for j in 0..k {
            let t = j as f64 / k as f64;
            pts.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let nb = pts.len();
    // interior points: hexagonal grid
    let (hx, hy) = (step, step * 0.866);
    let rows = ((hi[1] - lo[1]) / hy).ceil() as i32 + 1;
    let cols = ((hi[0] - lo[0]) / hx).ceil() as i32 + 1;
    for r in 0..rows {
        for q in 0..cols {
            let p = [lo[0] - c[0] + q as f64 * hx + if r % 2 == 1 { hx / 2.0 } else { 0.0 }, lo[1] - c[1] + r as f64 * hy];
            if pt_in(p, &poly) && edge_dist(p, &poly) > step * 0.6 {
                pts.push(p);
            }
        }
    }
    let loop_idx: Vec<usize> = (0..nb).collect();
    let (v2, _e, f2) = ig_gn::cdt::delaunay_inside_with_holes(&pts, &[loop_idx]);
    let nv = v2.len();
    let t = thickness;
    let mut m = Mesh::new();
    for p in &v2 {
        m.pos.push([p[0], p[1], t / 2.0]);
    }
    for p in &v2 {
        m.pos.push([p[0], p[1], -t / 2.0]);
    }
    let mut edge_count: std::collections::BTreeMap<(u32, u32), (u32, u32, u32)> = Default::default();
    for f in &f2 {
        if f.len() < 3 {
            continue;
        }
        let mut f: Vec<u32> = f.iter().map(|&i| i as u32).collect();
        // CCW seen from +z
        let (a, b, c2) = (v2[f[0] as usize], v2[f[1] as usize], v2[f[2] as usize]);
        if (b[0] - a[0]) * (c2[1] - a[1]) - (b[1] - a[1]) * (c2[0] - a[0]) < 0.0 {
            f.reverse();
        }
        for k in 0..f.len() {
            let (x, y) = (f[k], f[(k + 1) % f.len()]);
            let e = edge_count.entry((x.min(y), x.max(y))).or_insert((x, y, 0));
            e.2 += 1;
        }
        m.faces.push(f.clone());
        m.faces.push(f.iter().rev().map(|&i| i + nv as u32).collect());
    }
    for (_, &(x, y, cnt)) in &edge_count {
        if cnt == 1 {
            // top boundary edge x->y (CCW from above): outward side wall
            m.faces.push(vec![y, x, x + nv as u32, y + nv as u32]);
        }
    }
    let name = match kind {
        Kind::Shirt => "shirt",
        Kind::Pants => "pants",
    };
    let mat = fabric_material(rng, name);
    Garment { kind, mesh: m, thickness, mat }
}

/// Crumple the flat solid a little (low-frequency z waves + a twist about the centre), so the pre-sim state is not a perfect plane.
pub fn crumple(m: &mut Mesh, rng: &mut Rng, amount: f64) {
    let (a1, a2) = (rng.uniform(0.0, 6.28), rng.uniform(0.0, 6.28));
    let (k1, k2) = (rng.uniform(6.0, 14.0), rng.uniform(6.0, 14.0));
    let twist = rng.uniform(-0.6, 0.6) * amount;
    let amp = 0.03 * amount;
    for p in m.pos.iter_mut() {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        let th = twist * (-(r * r) / 0.12).exp();
        let (s, c) = th.sin_cos();
        let (x, y) = (p[0] * c - p[1] * s, p[0] * s + p[1] * c);
        p[0] = x;
        p[1] = y;
        p[2] += amp * (k1 * x + a1).sin() * (k2 * y + a2).sin();
    }
}

/// footprint half extents of a flat mesh
pub fn extents(m: &Mesh) -> (V3, V3) {
    m.bounds()
}
