//! table_decorations/vase.py (VaseFactory) and a nature-trinket set for NatureShelfTrinketsFactory (rocks, shells, pinecones, coral twigs)
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    matches!(c, "VaseFactory" | "NatureShelfTrinketsFactory")
}

const VASE: Weighted = &[("ceramic.Marble", 1.0), ("ceramic.MarbleRegular", 0.2), ("ceramic.MarbleVoronoi", 0.2), ("ceramic.Ceramic", 1.0), ("ceramic.Glass", 1.0), ("plastic.Plastic", 1.0), ("metal.MetalBasic", 1.0), ("wood.Wood", 1.0)];

/// material of the python base factory of NatureShelfTrinketsFactory(fs).base_factory (exact RNG replay in render_rust.py)
fn base_spec(name: &str, attr: &str, kwargs: serde_json::Value, fs: u64) -> Material {
    let mut k = kwargs;
    k["attr"] = json!(attr);
    spec_material(name, json!({"cls": "factory.elements.nature_shelf_trinkets.generate.NatureShelfTrinketsFactory", "kwargs": k, "seed": fs}), [0.7, 0.65, 0.6])
}

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.7, 0.65, 0.6])
}

pub struct Decor {
    fs: u64,
    vase: bool,
    p: Vec<f64>,
    seed: u64,
}

impl Decor {
    fn new(c: &str, rng: &mut Rng, fs: u64) -> Decor {
        let mut r = Rng::seeded(rng.clone().randint(0, 1_000_000_000) as u64);
        let r = &mut r;
        if c == "VaseFactory" {
            let z = r.uniform(0.17, 0.5);
            let x = z * r.uniform(0.3, 0.6);
            let neck_scale = r.uniform(0.2, 0.8);
            let top_scale = neck_scale * r.uniform(0.8, 1.2);
            let neck_mid = r.uniform(0.7, 0.95);
            let neck_pos = 0.5 * neck_scale + 0.5 + r.uniform(-0.05, 0.05);
            let shoulder_pos = r.uniform(0.3, 0.7);
            let shoulder_t = r.uniform(0.1, 0.25);
            let foot_scale = r.uniform(0.4, 0.6);
            let foot_h = r.uniform(0.01, 0.1);
            let inner = if r.f64() < 0.5 { 1.0 } else { r.uniform(0.8, 1.0) };
            Decor { fs, vase: true, p: vec![z, x, top_scale, neck_mid, neck_pos, neck_scale, shoulder_pos, shoulder_t, foot_scale, foot_h, inner], seed: r.randint(0, 1_000_000) as u64 }
        } else {
            let size = r.uniform(0.06, 0.15);
            // NatureShelfTrinketsFactory.__init__: np.random.choice(factories, p=probs / probs.sum()) inside FixedSeed(factory_seed)
            let probs = [1.0, 1.0, 1.0, 1.0, 3.0, 2.0, 3.0, 2.0, 2.0, 2.0, 2.0, 5.0, 5.0];
            let tot: f64 = probs.iter().sum();
            let kind = Rng::seeded(fs).choice_p(&probs.map(|x| x / tot));
            Decor { fs, vase: false, p: vec![size, kind as f64, r.f64(), r.f64(), r.f64()], seed: r.randint(0, 1_000_000) as u64 }
        }
    }
}

fn noisy_blob(rx: f64, ry: f64, rz: f64, r: &mut Rng, amp: f64) -> Mesh {
    let (nu, nv) = (18usize, 10usize);
    let ph: Vec<f64> = (0..6).map(|_| r.uniform(0.0, 2.0 * PI)).collect();
    let rad = |u: f64, v: f64| 1.0 + amp * ((3.0 * u + ph[0]).sin() * (2.0 * v + ph[1]).cos() * 0.5 + (5.0 * u + ph[2]).cos() * (3.0 * v + ph[3]).sin() * 0.3);
    let pt = |i: usize, j: usize| -> V3 {
        let (u, v) = (2.0 * PI * (i % nu) as f64 / nu as f64, PI * j as f64 / nv as f64);
        let k = rad(u, v);
        [rx * k * u.cos() * v.sin(), ry * k * u.sin() * v.sin(), -rz * k * v.cos()]
    };
    let mut m = Mesh::new();
    for j in 0..nv {
        for i in 0..nu {
            m.add_quad(pt(i, j), pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1));
        }
    }
    m
}

impl Factory for Decor {
    fn placeholder(&self, i: u64) -> Option<Asset> {
        if self.vase {
            return None;
        }
        // create_placeholder: primitive_cube_add(size=uniform(0.1, 0.15)) as the first draw of FixedSeed(int_hash((factory_seed, i)))
        let size = inst_rng(self.fs, i).uniform(0.1, 0.15);
        let h = (size as f32 / 2.0) as f64;
        let mut a = Asset::default();
        let m = Mesh::cuboid([-h, -h, -h], [h, h, h]);
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }

    fn asset(&self, i: u64) -> Asset {
        let mut a = self.asset_raw(i);
        if !self.vase {
            // create_asset: scale = min(bb / dim), recentre in x/y, bottom at -bb/2 (bb = placeholder box)
            let size = inst_rng(self.fs, i).uniform(0.1, 0.15);
            let (lo, hi) = a.bounds();
            let scale = (0..3).map(|k| size / (hi[k] - lo[k]).max(1e-9)).fold(f64::INFINITY, f64::min);
            let c = [(lo[0] + hi[0]) / 2.0 * scale, (lo[1] + hi[1]) / 2.0 * scale];
            let shift = Mat4::translation([-c[0], -c[1], -size / 2.0 - lo[2] * scale]).mul(&Mat4::scale([scale; 3]));
            a.transform(&shift);
        }
        a
    }
}

impl Decor {
    fn asset_raw(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let mut a = Asset::default();
        if self.vase {
            let p = &self.p;
            let (z, x) = (p[0], p[1]);
            let (top, nmid, npos, ns, sp, st, fs, fh) = (p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9]);
            let rr = x / 2.0;
            // (relative height, relative radius)
            let mut pts: Vec<(f64, f64)> = vec![(0.0, fs), (fh * 0.5, fs), (fh, fs * 1.15), (sp - st, 0.9), (sp, 1.0), (sp + st, 0.9), (nmid.min(npos).min(0.95), ns * 1.1), (npos.max(0.8).min(0.97), ns), (1.0, top)];
            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            pts.dedup_by(|b, a| (b.0 - a.0).abs() < 1e-3);
            let prof: Vec<(f64, f64)> = chaikin_open(&pts.iter().map(|&(h, q)| (q * rr, h * z)).collect::<Vec<_>>(), 2);
            let m = Mesh::vessel(0.0, 0.0, &prof, 0.003, 40);
            let b = m.bounds();
            let mat = spec("vase", weighted(&mut r, VASE), self.seed);
            a.mesh(m, &mat, Some(b));
        } else {
            let size = self.p[0];
            let kind = self.p[1] as usize;
            let mat = |name: &str, cls: &str, s: u64| spec(name, cls, s);
            match kind {
                4..=10 => {
                    // shell: coiled cone
                    let turns = 3.0;
                    let n = 60usize;
                    let mut m = Mesh::new();
                    let ring = 12usize;
                    let pos = |t: f64, k: usize| -> V3 {
                        let ang = turns * 2.0 * PI * t;
                        let rad = size * 0.5 * (0.15 + 0.85 * t);
                        let cx = rad * 0.9 * ang.cos();
                        let cy = rad * 0.9 * ang.sin();
                        let cz = size * 0.8 * t;
                        let a2 = 2.0 * PI * k as f64 / ring as f64;
                        let tube = size * 0.16 * (0.15 + 0.85 * t);
                        [cx + tube * a2.cos() * ang.cos(), cy + tube * a2.cos() * ang.sin(), cz + tube * a2.sin()]
                    };
                    for i in 0..n {
                        for k in 0..ring {
                            let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                            m.add_quad(pos(t0, k), pos(t0, k + 1), pos(t1, k + 1), pos(t1, k));
                        }
                    }
                    let b = m.bounds();
                    a.mesh(m, &mat("shell", "ceramic.Ceramic", self.seed), Some(b));
                }
                3 => {
                    let m = noisy_blob(size * 0.22, size * 0.22, size * 0.4, &mut r, 0.18);
                    let mut m = m;
                    for p in m.pos.iter_mut() {
                        p[2] += size * 0.4;
                    }
                    let b = m.bounds();
                    a.mesh(m, &base_spec("pinecone", "base_factory.material", json!({}), self.fs), Some(b));
                }
                11 | 12 => {
                    // small animal: body, head, four legs
                    let m = &mat("animal", "fabric.FineKnitFabric", self.seed);
                    let (l, h) = (size * 0.7, size * 0.35);
                    let body = noisy_blob(l * 0.5, h * 0.45, h * 0.45, &mut r, 0.05);
                    let mut body = body;
                    for p in body.pos.iter_mut() {
                        p[2] += h * 0.8;
                    }
                    let bb = body.bounds();
                    a.mesh(body, m, Some(bb));
                    let mut head = noisy_blob(h * 0.3, h * 0.25, h * 0.25, &mut r, 0.05);
                    for p in head.pos.iter_mut() {
                        p[0] += l * 0.55;
                        p[2] += h * 1.15;
                    }
                    let hb = head.bounds();
                    a.mesh(head, m, Some(hb));
                    for (sx, sy) in [(-0.3, -1.0), (-0.3, 1.0), (0.3, -1.0), (0.3, 1.0)] {
                        let leg = limb([l * sx, sy * h * 0.25, h * 0.8], [l * sx, sy * h * 0.25, 0.0], h * 0.08, h * 0.06, true);
                        let lb = leg.bounds();
                        a.mesh(leg, m, Some(lb));
                    }
                }
                0 => {
                    // coral: branching twigs
                    let m = &base_spec("coral", "base_factory.material", json!({}), self.fs);
                    for k in 0..5 {
                        let ang = 2.0 * PI * k as f64 / 5.0;
                        let tip = [size * 0.25 * ang.cos(), size * 0.25 * ang.sin(), size * (0.7 + 0.3 * r.f64())];
                        let l = limb([0.0, 0.0, 0.0], tip, size * 0.05, size * 0.025, true);
                        let lb = l.bounds();
                        a.mesh(l, m, Some(lb));
                    }
                }
                _ => {
                    // rock
                    let mut m = noisy_blob(size * 0.5, size * 0.4 * (0.7 + 0.3 * self.p[2]), size * 0.3, &mut r, 0.25);
                    for p in m.pos.iter_mut() {
                        p[2] += size * 0.3;
                    }
                    let b = m.bounds();
                    let rm = if kind == 2 { base_spec("rock", "base_factory.rock_surface", json!({"is_rock": true}), self.fs) } else { mat("rock", "ceramic.Plaster", self.seed) };
                    a.mesh(m, &rm, Some(b));
                }
            }
        }
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Decor::new(c, rng, fs)))
}
