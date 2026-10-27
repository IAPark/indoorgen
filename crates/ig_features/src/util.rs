//! Small geometry / material helpers shared by the built-in features.
use ig_core::materials::spec_material;
use ig_core::mesh::{Material, Mat4, Mesh, V3};
use ig_core::rng::Rng;

/// append a closed box (skipped when degenerate)
pub fn bx(m: &mut Mesh, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64) {
    if x1 - x0 < 1e-6 || y1 - y0 < 1e-6 || z1 - z0 < 1e-6 {
        return;
    }
    m.append(&Mesh::cuboid([x0, y0, z0], [x1, y1, z1]));
}

/// rounded (soft) box centred at the origin with half extents `h`, corner radius `r`; `puff` raises the upper centre like a stuffed cushion
pub fn rbox(h: V3, r: f64, puff: f64) -> Mesh {
    let r = r.min(h[0] * 0.95).min(h[1] * 0.95).min(h[2] * 0.95).max(1e-4);
    let axis = |hh: f64| -> Vec<f64> {
        let mut v = vec![];
        for k in 0..=4 {
            v.push(-hh + r * k as f64 / 4.0);
        }
        for k in 0..=4 {
            v.push(hh - r + r * k as f64 / 4.0);
        }
        v
    };
    let ax = [axis(h[0]), axis(h[1]), axis(h[2])];
    let mut m = Mesh::new();
    let pt = |q: V3| -> V3 {
        let mut p = [0.0; 3];
        let mut d = [0.0; 3];
        for k in 0..3 {
            let inner = q[k].max(-(h[k] - r)).min(h[k] - r);
            p[k] = inner;
            d[k] = q[k] - inner;
        }
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
        for k in 0..3 {
            p[k] += d[k] / l * r;
        }
        if puff != 0.0 {
            let u = 1.0 - (p[0] / h[0]).powi(2);
            let v = 1.0 - (p[1] / h[1]).powi(2);
            p[2] += puff * u.max(0.0) * v.max(0.0) * ((p[2] / h[2] + 1.0) * 0.5).max(0.0);
        }
        p
    };
    for a in 0..3 {
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        for s in [1.0, -1.0] {
            let n = ax[u].len();
            let nv = ax[v].len();
            for i in 0..n - 1 {
                for j in 0..nv - 1 {
                    let q = |ii: usize, jj: usize| {
                        let mut p = [0.0; 3];
                        p[a] = s * h[a];
                        p[u] = ax[u][ii];
                        p[v] = ax[v][jj];
                        pt(p)
                    };
                    let (p00, p10, p11, p01) = (q(i, j), q(i + 1, j), q(i + 1, j + 1), q(i, j + 1));
                    if (ax[u][i + 1] - ax[u][i]).abs() < 1e-9 || (ax[v][j + 1] - ax[v][j]).abs() < 1e-9 {
                        continue;
                    }
                    if s > 0.0 {
                        m.add_quad(p00, p10, p11, p01);
                    } else {
                        m.add_quad(p00, p01, p11, p10);
                    }
                }
            }
        }
    }
    m
}

/// horizontal knob / post along +y starting at y0
pub fn peg(m: &mut Mesh, x: f64, y0: f64, z: f64, len: f64, r: f64) {
    let mut c = Mesh::cylinder(0.0, 0.0, 0.0, len, r, 14);
    c.transform(&Mat4::rot_x(-std::f64::consts::FRAC_PI_2));
    c.transform(&Mat4::translation([x, y0, z]));
    m.append(&c);
}

pub fn paint(name: &str, rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, rgb, 0.42);
    m.metallic = 0.0;
    m
}

pub const PAINTS: &[(&str, [f32; 3])] = &[
    ("paint_warm_white", [0.9, 0.88, 0.82]),
    ("paint_cream", [0.85, 0.8, 0.66]),
    ("paint_sage", [0.52, 0.6, 0.48]),
    ("paint_navy", [0.1, 0.16, 0.28]),
    ("paint_charcoal", [0.17, 0.18, 0.19]),
    ("paint_dusty_blue", [0.48, 0.58, 0.66]),
    ("paint_forest", [0.13, 0.26, 0.19]),
    ("paint_greige", [0.62, 0.58, 0.52]),
    ("paint_clay", [0.6, 0.38, 0.3]),
];

/// Infinigen plywood-family wood material spec (same classes as the storage assets)
pub fn wood(kind: &str, seed: u64) -> Material {
    let (cls, rgb, name): (&str, [f32; 3], &str) = match kind {
        "white" => ("wood.WhitePlywood", [0.82, 0.8, 0.76], "bi_white_wood"),
        "black" => ("wood.BlackPlywood", [0.08, 0.06, 0.05], "bi_black_wood"),
        "wood2" => ("wood.Wood", [0.5, 0.33, 0.2], "bi_wood"),
        _ => ("wood.BlondePlywood", [0.55, 0.4, 0.24], "bi_blonde_wood"),
    };
    let mut m = spec_material(name, serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}), rgb);
    m.roughness = 0.5;
    m
}

/// case / carcass material: (material, is_natural_wood)
pub fn case_material(rng: &mut Rng, seed: u64) -> Material {
    let k = rng.choice_p(&[0.2, 0.4, 0.28, 0.12]);
    match k {
        0 => wood("white", seed),
        1 => {
            let (n, c) = PAINTS[rng.choice_idx(PAINTS.len())];
            paint(n, c)
        }
        2 => wood(if rng.bool(0.5) { "blonde" } else { "wood2" }, seed),
        _ => wood("black", seed),
    }
}

pub fn fabric(rng: &mut Rng, seed: u64, name: &str) -> Material {
    const FAB: &[&str] = &["fabric.CoarseKnitFabric", "fabric.FineKnitFabric", "fabric.SofaFabric", "fabric.Leather"];
    const RGB: &[[f32; 3]] = &[[0.62, 0.58, 0.5], [0.55, 0.6, 0.66], [0.42, 0.28, 0.24], [0.28, 0.4, 0.36], [0.75, 0.7, 0.6], [0.2, 0.25, 0.36]];
    let cls = FAB[rng.choice_p(&[0.35, 0.35, 0.25, 0.05])];
    let rgb = RGB[rng.choice_idx(RGB.len())];
    let mut m = spec_material(name, serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}), rgb);
    m.roughness = 0.9;
    m
}

pub fn hardware(rng: &mut Rng) -> Material {
    let k = rng.choice_idx(3);
    let (cls, rgb, n): (&str, [f32; 3], &str) = match k {
        0 => ("metal.BrushedMetal", [0.7, 0.7, 0.72], "bi_hw_steel"),
        1 => ("metal.MetalBasic", [0.75, 0.6, 0.3], "bi_hw_brass"),
        _ => ("metal.MetalBasic", [0.05, 0.05, 0.05], "bi_hw_black"),
    };
    let mut m = spec_material(n, serde_json::json!({"cls": cls, "kwargs": {}, "seed": rng.next_u32() as u64 % 1_000_000}), rgb);
    m.metallic = 1.0;
    m.roughness = 0.35;
    m
}
