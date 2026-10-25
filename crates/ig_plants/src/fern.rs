//! small_plants/fern.py FernFactory (geometry-node fronds). The parameter draws of create_asset / geo_fern follow the python order (mode, pinna count, per-frond
//! rotations with the vicinity rejection, ages and counts); the geometry is a direct construction of the same structure -- a rachis with growth-scaled spacing and
//! gravity bend, and a pair of feathered leaflet ribbons per pinna -- not the evaluated node graph (which instances a LeafFactory leaf ~10^6 times).
use super::curvekit::FloatCurve;
use super::*;
use crate::mesh::{Mat4, Mesh};
use crate::rng::Rng;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "FernFactory"
}

type V = [f64; 3];

pub struct FernFactory {
    pub fs: u64,
}

fn check_vicinity(r: &[f64; 4], list: &[[f64; 4]]) -> bool {
    list.iter().any(|q| (r[1] - q[1]).abs() < 0.1 && (r[2] - q[2]).abs() < 0.15)
}

fn rot_axis(p: V, axis: usize, a: f64) -> V {
    let (c, s) = (a.cos(), a.sin());
    match axis {
        0 => [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c],
        1 => [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c],
        _ => [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]],
    }
}

impl FernFactory {
    pub fn new(fs: u64) -> FernFactory {
        FernFactory { fs }
    }

    /// geometry_pinnae_nodes for one frond (local frame: rachis along +z)
    fn frond(&self, r: &mut Rng, leaf_num: usize, age: f64, pinna_num: usize, gravity: f64) -> Mesh {
        // contour (two modes)
        let mode = r.randint(0, 2);
        let base: [f64; 8] = if mode == 1 { [0.0, 0.2, 0.6, 1.4, 3.0, 4.0, 5.0, 6.0] } else { [0.0, 0.2, 0.6, 1.4, 3.0, 4.0, 5.0, 4.2] };
        let mut contour = [0.0; 8];
        for i in 0..8 {
            contour[i] = (base[i] + r.normal(0.0, 0.04 * i as f64)) / 6.0;
        }
        let xs = [0.0, 0.2, 0.4, 0.55, 0.7, 0.8, 0.9, 1.0];
        let fc = FloatCurve::new(&xs.iter().zip(contour.iter()).map(|(x, y)| (*x, *y)).collect::<Vec<_>>());
        // two random version bits for the left / right selections
        let _ = (r.randint(0, 2), r.randint(0, 2));
        // pinna version contour + curvature draws (5 versions)
        let mut pinna_k = vec![];
        for _ in 0..5 {
            let k = r.uniform(0.5, 0.58);
            let mut c = [0.0; 6];
            for j in 0..6 {
                c[j] = k * (j as f64 * (1.0 + r.normal(0.0, 0.1)) / 5.0 + 0.08).clamp(0.0, 0.7);
            }
            let ln = (leaf_num as i64 + r.randint(-1, 2)) as usize;
            let zmax = r.uniform(0.3, 0.45);
            let ycn = r.normal(0.0, 0.2).abs().clamp(0.0, 0.3);
            let ykk = r.uniform(-0.04, 0.2);
            pinna_k.push((c, ln.max(4), zmax, ycn, ykk));
        }
        let curv_y = r.normal(0.0, 0.3).clamp(-0.4, 0.4);
        let curv_z = r.normal(0.0, 0.2);
        let curv_g = r.uniform(0.25, 0.42) * gravity;
        // rachis positions: accumulate contour * age scale along z
        let amap = 0.3 + (4.5 - 0.3) * age;
        let mut z = 0.0;
        let mut rachis: Vec<V> = vec![];
        for k in 0..pinna_num {
            let t = k as f64 / (pinna_num - 1).max(1) as f64;
            rachis.push([0.0, 0.0, z]);
            z += fc.eval(1.0 - t).max(0.0).max(contour[7] * 0.0) * amap * 0.0 + fc.eval(t) * amap;
        }
        let total = rachis.last().map_or(1.0, |p| p[2]).max(1e-6);
        // rotation of every point about the origin with angles growing along the rachis
        let mut pts: Vec<V> = vec![];
        for (k, p) in rachis.iter().enumerate() {
            let t = k as f64 / (pinna_num - 1).max(1) as f64;
            let mut q = *p;
            q = rot_axis(q, 2, curv_z * t.powf(1.2));
            q = rot_axis(q, 1, curv_y * t.powf(1.2));
            q = rot_axis(q, 0, curv_g * (0.2 + t.powf(1.5)).min(1.0));
            pts.push(q);
        }
        let mut m = Mesh::new();
        // rachis stem as a thin 2-gon strip
        let w = 0.04 * amap * 0.15;
        for k in 0..pts.len() - 1 {
            m.add_quad([pts[k][0] - w, pts[k][1], pts[k][2]], [pts[k][0] + w, pts[k][1], pts[k][2]], [pts[k + 1][0] + w, pts[k + 1][1], pts[k + 1][2]], [pts[k + 1][0] - w, pts[k + 1][1], pts[k + 1][2]]);
        }
        // pinnae
        for k in 0..pts.len() {
            let t = k as f64 / (pinna_num - 1).max(1) as f64;
            let ver = &pinna_k[k % 5];
            let (c, ln, zmax, _ycn, _ykk) = (&ver.0, ver.1, ver.2, ver.3, ver.4);
            let pcont = FloatCurve::new(&[(0.0, c[0]), (0.38, c[1]), (0.55, c[2]), (0.75, c[3]), (0.9, c[4]), (1.0, c[5])]);
            let lmap = 0.5 + 1.5 * age;
            let size = fc.eval(t).max(0.02) / 0.6;
            for side in [-1.0f64, 1.0] {
                // pinna axis: perpendicular to the rachis, swept toward the tip and drooping
                let sweep = 0.3 + 0.2 * zmax;
                let droop = 0.15 + curv_g.abs() * 0.5;
                let dir = {
                    let d = [side * 1.0, -droop, sweep];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    [d[0] / l, d[1] / l, d[2] / l]
                };
                let mut s = 0.0;
                let mut prev = pts[k];
                let mut ribbon: Vec<(V, f64)> = vec![(prev, 0.0)];
                for j in 0..ln {
                    let u = j as f64 / (ln - 1).max(1) as f64;
                    s += pcont.eval(u).max(0.0) * lmap * size;
                    let p = [pts[k][0] + dir[0] * s, pts[k][1] + dir[1] * s - 0.05 * s * s * (0.5 + u), pts[k][2] + dir[2] * s];
                    let leaflet = pcont.eval(u).max(0.0) * 7.0 * 0.3 * 0.45 * size.sqrt();
                    ribbon.push((p, leaflet));
                    prev = p;
                }
                let _ = prev;
                // feather: leaflets as small diamonds on both sides of the pinna axis
                for q in 1..ribbon.len() {
                    let (p0, _) = ribbon[q - 1];
                    let (p1, wd) = ribbon[q];
                    let axis = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                    // side vector: perpendicular to the pinna axis within the frond plane (rachis direction)
                    let rd = [0.0, 0.0, 1.0];
                    let cr = [axis[1] * rd[2] - axis[2] * rd[1], axis[2] * rd[0] - axis[0] * rd[2], axis[0] * rd[1] - axis[1] * rd[0]];
                    let cl = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt().max(1e-9);
                    let up = [cr[0] / cl, cr[1] / cl, cr[2] / cl];
                    let fwd = [0.0, 0.0, 1.0];
                    let tip = [p1[0] + fwd[0] * wd * 1.2, p1[1] + fwd[1] * wd * 1.2, p1[2] + fwd[2] * wd * 1.2];
                    let a = [p0[0], p0[1], p0[2]];
                    m.add_tri(a, [p1[0] + up[0] * wd * 0.5, p1[1] + up[1] * wd * 0.5, p1[2] + up[2] * wd * 0.5], tip);
                    m.add_tri(a, tip, [p1[0] - up[0] * wd * 0.5, p1[1] - up[1] * wd * 0.5, p1[2] - up[2] * wd * 0.5]);
                }
            }
        }
        m
    }

    pub fn mesh(&self, inst: u64) -> Mesh {
        let mut r = inst_rng(self.fs, inst);
        let young = r.randint(0, 2) == 1;
        let scale = 0.02;
        let pinnae_num = r.randint(12, 30) as usize;
        let _lf_seed = r.randint(0, 1000);
        let mut out = Mesh::new();
        let mut frond_params: Vec<(f64, f64, f64, f64, f64, usize, usize)> = vec![]; // rz, rx, rz2, age, gravity, leaf_num, pinna_num
        if young {
            let mut rotates: Vec<[f64; 4]> = vec![];
            for _ in 0..pinnae_num {
                let flip = r.randint(0, 3);
                let rz = if flip != 0 { r.uniform(2.74, 3.54) } else { r.uniform(-0.4, 0.4) };
                let rx = r.uniform(0.8, 1.1);
                let rz2 = r.uniform(0.0, 6.28);
                let g = if flip != 0 { 1.0 } else { -1.0 };
                let rot = [rz, rx, rz2, g];
                if !check_vicinity(&rot, &rotates) {
                    rotates.push(rot);
                }
            }
            for rot in &rotates {
                let age = r.uniform(0.7, 0.95);
                let ln = r.randint(15, 25) as usize;
                let pn = r.randint(60, 80) as usize;
                frond_params.push((rot[0], rot[1], rot[2], age, rot[3], ln, pn));
            }
            let nyoung = r.randint(0, 5);
            for _ in 0..nyoung {
                let age = r.uniform(0.2, 0.5);
                let ln = r.randint(14, 20) as usize;
                let pn = r.randint(60, 100) as usize;
                let rz = r.uniform(0.0, 6.28);
                let rx = r.uniform(0.0, 0.4);
                let rz2 = r.uniform(0.0, 6.28);
                frond_params.push((rz, rx, rz2, age, 0.0, ln, pn));
            }
        } else {
            let mut rotates: Vec<[f64; 4]> = vec![];
            for _ in 0..pinnae_num {
                let rz = r.normal(3.14, 0.2);
                let rx = r.uniform(0.5, 1.1);
                let rz2 = r.uniform(0.0, 6.28);
                let rot = [rz, rx, rz2, 1.0];
                if !check_vicinity(&rot, &rotates) {
                    rotates.push(rot);
                }
            }
            for rot in &rotates {
                let age = r.uniform(0.7, 0.9);
                let ln = r.randint(16, 25) as usize;
                let pn = r.randint(60, 80) as usize;
                frond_params.push((rot[0], rot[1], rot[2], age, rot[3], ln, pn));
            }
        }
        for (rz, rx, rz2, age, g, ln, pn) in frond_params {
            let mut f = self.frond(&mut r, ln, age, pn, if g == 0.0 { 0.0 } else { g });
            f.transform(&Mat4::rot_z(rz2).mul(&Mat4::euler_xyz([-rx, 0.0, 0.0])).mul(&Mat4::rot_z(rz)));
            out.append(&f);
        }
        out.transform(&Mat4::scale([scale; 3]));
        out
    }
}

impl Factory for FernFactory {
    fn asset(&self, inst: u64) -> Asset {
        let mesh = self.mesh(inst);
        let b = mesh.bounds();
        let mut a = Asset::default();
        a.mesh(mesh, &mat("fern", [0.1, 0.35, 0.1], 0.6), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(FernFactory::new(fs)))
}
