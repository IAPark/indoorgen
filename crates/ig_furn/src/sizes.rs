//! Real-world furniture sizes as probability densities (data/size_model.json): the solver samples a piece's size from its class
//! density when it adds the piece, a resize move nudges it, and the annealing loss adds -log density relative to the most likely size,
//! so sizes are chosen together with the layout. Placeholders are plain boxes of the chosen size (no asset build); the final asset is
//! built with its own seed and fitted to the box (`fit_matrix`).
//!
//! Sizes are [width, depth, height] = the local [y, x, z] extents of the canonical frame (back = min x, front = max x, bottom = min z).
use crate::mesh::V3;
use crate::rng::Rng;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Comp {
    pub p: f64,
    pub mean: [f64; 3],
    pub sd: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Model {
    pub uniform: bool,
    /// 'exact': the factory builds to the size itself (ig_kit::api::set_size_hint), no fitting
    pub exact: bool,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub comps: Vec<Comp>,
    /// -log density at the most likely component mean (the loss is relative to it)
    pub nll0: f64,
}

static MODELS: OnceLock<crate::dhash::FxMap<String, Model>> = OnceLock::new();

fn a3(v: &serde_json::Value) -> [f64; 3] {
    let a = v.as_array().unwrap();
    [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
}

pub fn models() -> &'static crate::dhash::FxMap<String, Model> {
    MODELS.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../data/size_model.json")).unwrap();
        let mut out = crate::dhash::FxMap::default();
        for (k, m) in v.as_object().unwrap() {
            if k.starts_with('_') {
                continue;
            }
            let comps: Vec<Comp> = m["components"].as_array().unwrap().iter().map(|c| Comp { p: c["p"].as_f64().unwrap(), mean: a3(&c["mean"]), sd: a3(&c["sd"]) }).collect();
            let mut md = Model { uniform: m["policy"].as_str() == Some("uniform"), exact: m["policy"].as_str() == Some("exact"), min: a3(&m["min"]), max: a3(&m["max"]), comps, nll0: 0.0 };
            md.nll0 = md.comps.iter().map(|c| nll_raw(&md, c.mean)).fold(f64::INFINITY, f64::min);
            out.insert(k.clone(), md);
        }
        out
    })
}

pub fn model(class: &str) -> Option<&'static Model> {
    if ig_core::env_on!("NO_SIZES") {
        return None;
    }
    models().get(class)
}

fn nll_raw(m: &Model, x: [f64; 3]) -> f64 {
    let mut p = 0.0;
    for c in &m.comps {
        let mut q = c.p;
        for i in 0..3 {
            let z = (x[i] - c.mean[i]) / c.sd[i];
            q *= (-0.5 * z * z).exp() / (c.sd[i] * (2.0 * std::f64::consts::PI).sqrt());
        }
        p += q;
    }
    -(p.max(1e-300)).ln()
}

/// -log density relative to the most likely size (>= 0)
pub fn nll(m: &Model, x: [f64; 3]) -> f64 {
    (nll_raw(m, x) - m.nll0).max(0.0)
}

fn clamp(m: &Model, mut x: [f64; 3]) -> [f64; 3] {
    for i in 0..3 {
        x[i] = x[i].clamp(m.min[i], m.max[i]);
    }
    x
}

pub fn sample(m: &Model, rng: &mut Rng) -> [f64; 3] {
    let k = rng.choice_p(&m.comps.iter().map(|c| c.p).collect::<Vec<_>>());
    let c = &m.comps[k];
    let mut x = [0.0; 3];
    for i in 0..3 {
        let mut v = c.mean[i];
        for _ in 0..20 {
            v = rng.normal(c.mean[i], c.sd[i]);
            if v >= m.min[i] && v <= m.max[i] {
                break;
            }
        }
        x[i] = v;
    }
    clamp(m, x)
}

/// resize move: a fresh sample (1 in 4), else one axis (uniform classes: all axes) scaled by exp(N(0, 0.08))
pub fn perturb(m: &Model, x: [f64; 3], rng: &mut Rng) -> [f64; 3] {
    if rng.f64() < 0.25 {
        return sample(m, rng);
    }
    let f = rng.normal(0.0, 0.08).exp();
    let mut y = x;
    if m.uniform {
        for v in y.iter_mut() {
            *v *= f;
        }
    } else {
        y[rng.choice_idx(3)] *= f;
    }
    clamp(m, y)
}

/// the box of size `x` placed like the reference box: back face (min x) and bottom (min z) kept, centred in y
pub fn target_box(ref_lo: V3, ref_hi: V3, x: [f64; 3]) -> (V3, V3) {
    let cy = (ref_lo[1] + ref_hi[1]) / 2.0;
    ([ref_lo[0], cy - x[0] / 2.0, ref_lo[2]], [ref_lo[0] + x[1], cy + x[0] / 2.0, ref_lo[2] + x[2]])
}

/// axis-aligned map (per-axis scale + offset) from an asset's natural bounds to the chosen box: back, bottom and the y centre line
/// up; 'axes' classes stretch each axis by at most +-15% (then shrink uniformly if still too big), 'uniform' classes scale uniformly to
/// fit inside the box. Returns (scale, offset): p' = p * scale + offset.
pub fn fit_matrix(uniform: bool, n_lo: V3, n_hi: V3, t_lo: V3, t_hi: V3) -> (V3, V3) {
    let nd = [(n_hi[0] - n_lo[0]).max(1e-6), (n_hi[1] - n_lo[1]).max(1e-6), (n_hi[2] - n_lo[2]).max(1e-6)];
    let td = [t_hi[0] - t_lo[0], t_hi[1] - t_lo[1], t_hi[2] - t_lo[2]];
    let mut s = [td[0] / nd[0], td[1] / nd[1], td[2] / nd[2]];
    if uniform {
        let u = s[0].min(s[1]).min(s[2]);
        s = [u, u, u];
    } else {
        for v in s.iter_mut() {
            *v = v.clamp(0.85, 1.15);
        }
        let over = (0..3).map(|i| td[i] / (nd[i] * s[i])).fold(f64::INFINITY, f64::min);
        if over < 1.0 {
            for v in s.iter_mut() {
                *v *= over;
            }
        }
    }
    let ncy = (n_lo[1] + n_hi[1]) / 2.0;
    let tcy = (t_lo[1] + t_hi[1]) / 2.0;
    (s, [t_lo[0] - n_lo[0] * s[0], tcy - ncy * s[1], t_lo[2] - n_lo[2] * s[2]])
}

/// a solver geometry mapped by `fit_matrix` (tris, proxies, bounds, convex hull)
pub fn fit_geom(g: &super::geom::ObjGeom, s: V3, o: V3) -> super::geom::ObjGeom {
    use super::geom::Proxy;
    let m = |p: V3| [p[0] * s[0] + o[0], p[1] * s[1] + o[1], p[2] * s[2] + o[2]];
    let mut g = g.clone();
    for t in g.tris.iter_mut() {
        t.v = [m(t.v[0]), m(t.v[1]), m(t.v[2])];
    }
    for (p, _) in g.proxies.iter_mut() {
        match p {
            Proxy::Box { center, half } => {
                *center = m(*center);
                *half = [half[0] * s[0], half[1] * s[1], half[2] * s[2]];
            }
            Proxy::Hull(pts) => {
                for q in pts.iter_mut() {
                    *q = m(*q);
                }
            }
        }
    }
    g.lo = m(g.lo);
    g.hi = m(g.hi);
    if let Some(c) = &g.convex {
        g.convex = Some(std::sync::Arc::new(c.iter().map(|p| m(*p)).collect()));
    }
    g
}
