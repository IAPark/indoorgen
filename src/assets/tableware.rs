//! tableware/*.py: Bowl, Plate, Cup, Wineglass, Can, Jar, Bottle (revolved profiles)
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;

/// Blender SOLIDIFY direction relative to my spin normals (verified by comparison against the Python surface)
const SOLID_SIGN: f64 = 1.0;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    matches!(c, "BowlFactory" | "PlateFactory" | "CupFactory" | "WineglassFactory" | "CanFactory" | "JarFactory" | "BottleFactory")
}

const CUP: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 0.7),
    ("ceramic.VaseCeramic", 4.0),
    ("metal.MetalBasic", 2.0),
    ("plastic.Plastic", 5.0),
    ("plastic.PlasticTranslucent", 5.0),
    ("ceramic.Glass", 3.0),
];
const JAR: Weighted = &[("ceramic.Glass", 8.0), ("plastic.Plastic", 2.0), ("plastic.PlasticTranslucent", 2.0), ("ceramic.VaseCeramic", 2.0), ("ceramic.ColoredGlass", 2.0), ("ceramic.Marble", 1.0), ("ceramic.GlassVolume", 2.0)];
const HANDLE: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("plastic.PlasticRough", 1.0), ("wood.Wood", 3.0), ("metal.WhiteMetal", 2.0)];
const METALS: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3)];

fn spec(name: &str, cls: &str, seed: u64) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), [0.8, 0.8, 0.8])
}

#[derive(Clone, Copy)]
enum Kind {
    Bowl,
    Plate,
    Cup,
    Wine,
    Can,
    Jar,
    Bottle,
}

pub struct Tableware {
    kind: Kind,
    p: Vec<f64>,
    flag: [bool; 4],
    seed: u64,
    fseed: u64,
    extra: Vec<Vec<f64>>,
}

impl Tableware {
    fn new(c: &str, rng: &mut Rng, fseed: u64) -> Tableware {
        let mut extra: Vec<Vec<f64>> = vec![];
        let mut r = rng.clone();
        let r = &mut r;
        // note: the subclass __init__ re-seeds FixedSeed(factory_seed), so its parameters start at the beginning of the stream
        let (kind, p, flag): (Kind, Vec<f64>, [bool; 4]) = match c {
            "BowlFactory" => {
                let x_end = 0.5;
                let z_length = r.log_uniform(0.4, 0.8);
                let z_bottom = r.log_uniform(0.02, 0.05);
                let x_bottom = r.uniform(0.2, 0.3) * x_end;
                let x_mid = r.uniform(0.8, 0.95) * x_end;
                let has_inside = r.f64() < 0.5;
                let scale = r.log_uniform(0.15, 0.4);
                let thickness = r.uniform(0.01, 0.03) * scale;
                (Kind::Bowl, vec![x_end, z_length, z_bottom, x_bottom, x_mid, scale, thickness], [has_inside, false, false, false])
            }
            "PlateFactory" => {
                let x_end = 0.5;
                let z_length = r.log_uniform(0.05, 0.2);
                let x_mid = r.uniform(0.3, 1.0) * x_end;
                let z_mid = r.uniform(0.3, 0.8) * z_length;
                let has_inside = r.f64() < 0.2;
                let scale = r.log_uniform(0.2, 0.4);
                let thickness = r.uniform(0.01, 0.03) * scale;
                (Kind::Plate, vec![x_end, z_length, x_mid, z_mid, scale, thickness], [has_inside, false, false, false])
            }
            "CupFactory" => {
                let short = r.f64() < 0.5;
                let (straight, x_lowest, depth, guard);
                if short {
                    straight = r.f64() < 0.2;
                    x_lowest = r.log_uniform(0.6, 0.9);
                    depth = r.log_uniform(0.25, 0.5);
                    guard = r.f64() < 0.8;
                } else {
                    straight = true;
                    x_lowest = r.log_uniform(0.9, 1.0);
                    depth = r.log_uniform(0.5, 1.0);
                    guard = false;
                }
                let handle_loc = if straight { r.uniform(0.45, 0.65) } else { r.uniform(-0.1, 0.3) };
                let round = r.f64() < 0.5;
                let handle_r = depth * r.uniform(0.2, 0.4);
                let _handle_inner = handle_r * r.log_uniform(0.2, 0.3);
                let _tx = r.uniform(0.0, 2.0);
                let _ty = r.uniform(0.0, 2.0);
                let x_lower_ratio = r.log_uniform(0.8, 1.0);
                let thickness = r.log_uniform(0.01, 0.04);
                let _has_wrap = r.f64() < 0.3;
                let _wrap_margin = r.uniform(0.1, 0.2);
                let has_inside = r.f64() < 0.5;
                let scale = r.log_uniform(0.15, 0.3);
                (Kind::Cup, vec![0.25, x_lowest, depth, handle_loc, handle_r, x_lower_ratio, thickness, scale], [straight, guard, round, has_inside])
            }
            "WineglassFactory" => {
                let x_end = 0.25;
                let z_length = r.log_uniform(0.6, 2.0);
                let z_cup = r.uniform(0.3, 0.6) * z_length;
                let z_mid = z_cup + r.uniform(0.3, 0.5) * (z_length - z_cup);
                let x_neck = r.log_uniform(0.01, 0.02);
                let x_top = x_end * r.log_uniform(1.0, 1.4);
                let x_mid = x_top * r.log_uniform(0.9, 1.2);
                let thickness = r.uniform(0.01, 0.03);
                let scale = r.log_uniform(0.1, 0.3);
                let z_bottom = z_length * r.log_uniform(0.01, 0.05);
                (Kind::Wine, vec![x_end, z_length, z_cup, z_mid, x_neck, x_top, x_mid, thickness, scale, z_bottom], [false; 4])
            }
            "CanFactory" => {
                let x_length = r.log_uniform(0.05, 0.1);
                let z_length = x_length * r.log_uniform(0.5, 2.5);
                let rect = r.choice_idx(2) == 1;
                let skew = if r.f64() < 0.5 { r.uniform(1.0, 2.5) } else { 1.0 };
                (Kind::Can, vec![x_length, z_length, skew, r.uniform(0.2, 0.8)], [rect, false, false, false])
            }
            "JarFactory" => {
                let z_length = r.uniform(0.15, 0.2);
                let x_length = r.uniform(0.03, 0.06);
                let thickness = r.uniform(0.002, 0.004);
                let n_base = [4.0, 6.0, 64.0][r.choice_idx(3)];
                let x_cap = r.uniform(0.6, 0.9) * (PI / n_base).cos();
                let z_cap = r.uniform(0.05, 0.08);
                let z_neck = r.uniform(0.15, 0.2);
                (Kind::Jar, vec![x_length, z_length, thickness, n_base, x_cap, z_cap, z_neck], [false; 4])
            }
            _ => {
                let z_length = r.uniform(0.15, 0.25);
                let x_length = z_length * r.uniform(0.15, 0.25);
                let x_cap = r.uniform(0.3, 0.35);
                let ty = r.choice_idx(5);
                let w = r.uniform(0.002, 0.005);
                let (mut z_waist, z_neck, z_cap);
                let (xa, za, vi): (Vec<f64>, Vec<f64>, Vec<f64>);
                match ty {
                    0 => {
                        z_neck = r.uniform(0.5, 0.6);
                        z_cap = r.uniform(0.05, 0.08);
                        let ns = r.uniform(0.06, 0.1);
                        let nr = r.uniform(0.4, 0.5);
                        z_waist = 0.0;
                        xa = vec![0.0, 1.0, 1.0, (nr + 1.0) / 2.0 + (1.0 - nr) / 2.0 * x_cap, nr + (1.0 - nr) * x_cap, x_cap, x_cap, 0.0];
                        za = vec![0.0, 0.0, z_neck, z_neck + r.uniform(0.6, 0.7) * ns, z_neck + ns, 1.0 - z_cap, 1.0, 1.0];
                        vi = vec![1.0, 2.0, 4.0, 5.0, 6.0];
                    }
                    1 => {
                        z_neck = r.uniform(0.6, 0.7);
                        z_cap = r.uniform(0.1, 0.15);
                        let ns = r.uniform(0.1, 0.15);
                        z_waist = 0.0;
                        xa = vec![0.0, 1.0, 1.0, (1.0 + x_cap) / 2.0, x_cap, x_cap, 0.0];
                        za = vec![0.0, 0.0, z_neck, z_neck + r.uniform(0.6, 0.7) * ns, z_neck + ns, 1.0, 1.0];
                        vi = vec![1.0, 2.0, 4.0, 5.0];
                    }
                    2 => {
                        z_neck = r.uniform(0.4, 0.5);
                        z_cap = r.uniform(0.05, 0.08);
                        z_waist = 0.0;
                        xa = vec![0.0, 1.0, 1.0, 1.0, (1.0 + x_cap) / 2.0, x_cap, x_cap, 0.0];
                        let a0 = r.uniform(0.08, 0.1);
                        let a1 = r.uniform(0.15, 0.18);
                        za = vec![0.0, 0.0, z_neck, z_neck + a0, z_neck + a1, 1.0 - z_cap, 1.0, 1.0];
                        vi = vec![1.0, 2.0, 5.0, 6.0];
                    }
                    3 => {
                        z_waist = r.uniform(0.4, 0.5);
                        z_neck = z_waist + r.uniform(0.2, 0.25);
                        z_cap = r.uniform(0.05, 0.08);
                        let (x1, x3) = (r.uniform(0.85, 0.95), r.uniform(0.85, 0.95));
                        xa = vec![0.0, x1, 1.0, x3, 1.0, 1.0, x_cap, x_cap, 0.0];
                        let (z2, z3) = (r.uniform(0.08, 0.12), r.uniform(0.18, 0.25));
                        za = vec![0.0, 0.0, z2, z3, z_waist, z_neck, 1.0 - z_cap, 1.0, 1.0];
                        vi = vec![1.0, 4.0, 5.0, 6.0, 7.0];
                    }
                    _ => {
                        z_waist = r.uniform(0.1, 0.15);
                        z_neck = r.uniform(0.7, 0.75);
                        z_cap = r.uniform(0.0, 0.08);
                        let xl = r.uniform(0.85, 0.95);
                        xa = vec![0.0, xl, (xl + 1.0) / 2.0, 1.0, 1.0, (x_cap + 1.0) / 2.0, x_cap, x_cap, 0.0];
                        let a0 = r.uniform(0.1, 0.15);
                        let a1 = r.uniform(0.1, 0.2);
                        za = vec![0.0, 0.0, z_waist - a0, z_waist, z_neck, z_neck + a1, 1.0 - z_cap, 1.0, 1.0];
                        vi = vec![1.0, 3.0, 4.0, 6.0, 7.0];
                    }
                }
                extra = vec![xa, za, vi];
                (Kind::Bottle, vec![z_length, x_length, x_cap, w, z_neck, z_waist, z_cap], [false; 4])
            }
        };
        Tableware { kind, p, flag, seed: r.randint(0, 1_000_000) as u64, fseed, extra }
    }

    fn build(&self, inst: u64) -> Asset {
        let mut a = Asset::default();
        let mut r = Rng::seeded(self.seed);
        let p = &self.p;
        let body = |r: &mut Rng, cls: Weighted| spec("tableware", weighted(r, cls), self.seed);
        match self.kind {
            Kind::Bowl => {
                let (x_end, zl, zb, xb, xm, sc, th) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
                let xs = [0.0, xb, xb + 1e-3, xb, xm, x_end];
                let zs = [0.0, 0.0, 0.0, zb, zl / 2.0, zl];
                let pts: Vec<(f64, f64)> = xs.iter().zip(zs.iter()).map(|(x, z)| (x * sc, z * sc)).collect();
                let m = solidify(&spin(&pts, &[2, 3], 12), th * SOLID_SIGN).smoothed(th / 2.0, 3, 1);
                let b = m.bounds();
                let mat = body(&mut r, CUP);
                a.mesh(m, &mat, Some(b));
            }
            Kind::Plate => {
                let (x_end, zl, xm, zm, sc, th) = (p[0], p[1], p[2], p[3], p[4], p[5]);
                let xs = [0.0, xm, xm, x_end];
                let zs = [0.0, 0.0, zm, zl];
                let pts: Vec<(f64, f64)> = xs.iter().zip(zs.iter()).map(|(x, z)| (x * sc, z * sc)).collect();
                let m = solidify(&spin(&pts, &[1, 2], 12).smoothed(0.0, 1, 1), th * SOLID_SIGN).smoothed(0.0, 1, 1);
                let b = m.bounds();
                let mat = body(&mut r, CUP);
                a.mesh(m, &mat, Some(b));
            }
            Kind::Cup => {
                let (x_end, xl, depth, hl, hr, xlr, th, sc) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);
                let straight = self.flag[0];
                let prof: Vec<(f64, f64)> = if straight {
                    vec![(0.0, 0.0), (xl * x_end * sc, 0.0), (x_end * sc, depth * sc)]
                } else {
                    vec![(0.0, 0.0), (xl * x_end * sc, 0.0), ((xl + xlr * (1.0 - xl)) * x_end * sc, depth * 0.5 * sc), (x_end * sc, depth * sc)]
                };
                let m = Mesh::vessel(0.0, 0.0, &chaikin_open(&prof, if straight { 0 } else { 2 }), th * sc.min(1.0) * 0.5 + 0.002, 32);
                let b = m.bounds();
                let mat = body(&mut r, CUP);
                a.mesh(m, &mat, Some(b));
                if self.flag[1] {
                    // handle: arc attached at hl along the wall, reaching outward
                    let (x0, z0) = (prof[prof.len() - 2].0, prof[prof.len() - 2].1);
                    let (x1, z1) = (prof[prof.len() - 1].0, prof[prof.len() - 1].1);
                    let hx = x0 * (1.0 - hl) + x1 * hl;
                    let hz = z0 * (1.0 - hl) + z1 * hl;
                    let rad = hr * sc;
                    let hm = spec("cup_handle", weighted(&mut r, HANDLE), self.seed + 1);
                    let n = 10;
                    let pts: Vec<V3> = (0..=n).map(|k| { let t = PI * (k as f64 / n as f64) - PI / 2.0; [hx + rad * 0.9 * t.cos() * if self.flag[2] { 1.0 } else { 0.7 }, 0.0, hz + rad * t.sin() * 1.3] }).collect();
                    for k in 0..n {
                        let l = limb(pts[k], pts[k + 1], rad * 0.18, rad * 0.18, true);
                        let lb = l.bounds();
                        a.mesh(l, &hm, Some(lb));
                    }
                }
            }
            Kind::Wine => {
                let (x_end, zl, zc, zm, xn, xt, xmid, th, sc) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8]);
                let mut ir = inst_rng(self.fseed, inst);
                let zb = zl * ir.log_uniform(0.01, 0.05);
                let xs = [x_end, x_end / 2.0, xn, xn, xmid, xt];
                let zs = [0.0, zb / 2.0, zb, zc, zm, zl];
                let pts: Vec<(f64, f64)> = xs.iter().zip(zs.iter()).map(|(x, z)| (*x, *z)).collect();
                let mut m = solidify(&spin(&pts, &[0, 1, 2, 3], 12), -th);
                m.transform(&Mat4::scale([sc, sc, sc]));
                let b = m.bounds();
                let mat = spec("wineglass", "ceramic.Glass", self.seed);
                a.mesh(m, &mat, Some(b));
            }
            Kind::Can => {
                let (xl, zl, skew) = (p[0], p[1], p[2]);
                let rect = self.flag[0];
                let n = 40;
                let side = xl * p[3];
                let pts: Vec<[f64; 2]> = (0..n).map(|i| {
                    let t = 2.0 * PI * i as f64 / n as f64;
                    let (c, s) = (t.cos(), t.sin());
                    if rect {
                        let e = 0.25;
                        let kx = c.abs().powf(2.0 / (2.0 + 4.0 * e)).copysign(c);
                        let ky = s.abs().powf(2.0 / (2.0 + 4.0 * e)).copysign(s);
                        [xl * kx * (1.0 + side * 0.0), xl * ky / skew]
                    } else {
                        [xl * c, xl * s / skew]
                    }
                }).collect();
                let m = extrude(&pts, 0.0, zl);
                let b = m.bounds();
                let mat = spec("can", weighted(&mut r, METALS), self.seed);
                a.mesh(m, &mat, Some(b));
                let label = {
                    let mut lm = Mesh::new();
                    let (z0, z1) = (zl * 0.1, zl * 0.92);
                    for i in 0..n {
                        let j = (i + 1) % n;
                        let (q0, q1) = (pts[i], pts[j]);
                        let sc = 1.002;
                        lm.add_quad([q0[0] * sc, q0[1] * sc, z0], [q1[0] * sc, q1[1] * sc, z0], [q1[0] * sc, q1[1] * sc, z1], [q0[0] * sc, q0[1] * sc, z1]);
                    }
                    lm
                };
                let lb = label.bounds();
                a.mesh(label, &spec("can_label", "art.Art", self.seed + 3), Some(lb));
            }
            Kind::Jar => {
                let (xl, zl, th, nb, xc, zc, zn) = (p[0], p[1], p[2], p[3] as usize, p[4], p[5], p[6]);
                // body: n_base prism lofted into a rounded neck and cap rim
                let ring = |rr: f64, z: f64| -> Vec<V3> { (0..nb.max(4)).map(|i| { let t = 2.0 * PI * i as f64 / nb.max(4) as f64; [rr * t.cos(), rr * t.sin(), z] }).collect() };
                let profile = [(1.0, 0.0), (1.0, 1.0), (1.0 - 0.0 * xl, 1.0 + zn * 0.4), (xc, 1.0 + zn)];
                let rings: Vec<Vec<V3>> = profile.iter().map(|&(rs, zz)| ring(rs * xl, zz * zl)).collect();
                let mut m = Mesh::new();
                for k in 0..rings.len() - 1 {
                    let n = rings[k].len();
                    for i in 0..n {
                        let j = (i + 1) % n;
                        m.add_quad(rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]);
                    }
                }
                let n = rings[0].len();
                for i in 0..n {
                    m.add_tri([0.0, 0.0, 0.0], rings[0][(i + 1) % n], rings[0][i]);
                }
                let _ = th;
                let b = m.bounds();
                let mat = spec("jar", weighted(&mut r, JAR), self.seed);
                a.mesh(m, &mat, Some(b));
                let cz0 = (1.0 + zn) * zl;
                let cap = Mesh::cylinder(0.0, 0.0, cz0, cz0 + zc * zl, xc * xl + 0.001, 32);
                let cb = cap.bounds();
                a.mesh(cap, &spec("jar_cap", weighted(&mut r, HANDLE), self.seed + 2), Some(cb));
            }
            Kind::Bottle => {
                let (zl, xl, xcap, bw, zneck, zwaist, zcap) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
                let (xa, za, vi) = (&self.extra[0], &self.extra[1], &self.extra[2]);
                let pts: Vec<(f64, f64)> = xa.iter().zip(za.iter()).map(|(x, z)| (x * xl, z * zl)).collect();
                let vidx: Vec<usize> = vi.iter().map(|v| *v as usize).collect();
                let body_m = spin(&pts, &vidx, 12).smoothed(0.0, 1, 1);
                let rad = body_m.pos.iter().map(|q| q[0]).fold(0.0f64, f64::max) + 2e-3;
                let m = if bw > 0.0 { solidify(&body_m, -bw) } else { body_m };
                let b = m.bounds();
                let mat = spec("bottle", "ceramic.Glass", self.seed);
                a.mesh(m, &mat, Some(b));
                // label wrap: open cylinder between the waist and the neck (instance draws)
                let mut ir = inst_rng(self.fseed, inst);
                let z_max = zneck - ir.uniform(0.02, 0.05) * (zneck - zwaist);
                let z_min = zwaist + ir.uniform(0.02, 0.15) * (zneck - zwaist);
                let (z0, z1) = (z_min * zl, z_max * zl);
                let n = 128;
                let mut wrap = Mesh::new();
                for i in 0..n {
                    let (t0, t1) = (2.0 * PI * i as f64 / n as f64, 2.0 * PI * (i + 1) as f64 / n as f64);
                    wrap.add_quad([rad * t0.cos(), rad * t0.sin(), z0], [rad * t1.cos(), rad * t1.sin(), z0], [rad * t1.cos(), rad * t1.sin(), z1], [rad * t0.cos(), rad * t0.sin(), z1]);
                }
                let wb = wrap.bounds();
                a.mesh(wrap, &spec("bottle_label", "art.Art", self.seed + 1), Some(wb));
                // cap
                let cz0 = (1.0 - zcap) * zl;
                let cap = Mesh::cylinder(0.0, 0.0, cz0, cz0 + (zcap + 0.01) * zl, (xcap + 0.1) * xl, 64);
                let cb = cap.bounds();
                a.mesh(cap, &spec("bottle_cap", weighted(&mut r, METALS), self.seed + 2), Some(cb));
            }
        }
        let _ = self.flag;
        a
    }
}

impl Factory for Tableware {
    fn asset(&self, i: u64) -> Asset {
        self.build(i)
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Tableware::new(c, rng, fseed)))
}
