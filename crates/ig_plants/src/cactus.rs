//! cactus/*.py (CactusFactory: globular / columnar / prickly pear bodies + spikes). Parameter draws follow the python order; the geometry is rebuilt with
//! swept star profiles (no voxel remesh: python remeshes the body at face_size 0.003, and its tree / spike instancing is Blender-order dependent), so this
//! is a faithful approximation, not a surface-equal port.
use super::curvekit::{Curve, FloatCurve};
use super::*;
use crate::mesh::{Mat4, Mesh};
use crate::rng::Rng;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "CactusFactory"
}

type V = [f64; 3];

fn vsub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vadd(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn vmul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn vdot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn vcross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn vlen(a: V) -> f64 {
    vdot(a, a).sqrt()
}
fn vunit(a: V) -> V {
    let l = vlen(a);
    if l > 0.0 {
        vmul(a, 1.0 / l)
    } else {
        a
    }
}

/// Rodrigues rotation of v about axis by angle
fn rodrigues(v: V, axis: V, ang: f64) -> V {
    let k = vunit(axis);
    let (c, s) = (ang.cos(), ang.sin());
    vadd(vadd(vmul(v, c), vmul(vcross(k, v), s)), vmul(k, vdot(k, v) * (1.0 - c)))
}

fn randn3(r: &mut Rng) -> V {
    [r.randn(), r.randn(), r.randn()]
}

/// trees/tree.py rand_path
#[allow(clippy::too_many_arguments)]
fn rand_path(r: &mut Rng, n_pts: usize, sz: f64, std: f64, momentum: f64, init_vec: V, init_pt: V, pull_dir: Option<V>, pull_init: f64, pull_factor: f64) -> Vec<V> {
    let mut iv = init_vec;
    if let Some(p) = pull_dir {
        iv = vadd(iv, vmul(p, pull_init));
    }
    iv = vunit(iv);
    let mut path = vec![init_pt];
    for i in 1..n_pts {
        let prev_delta = if i == 1 { vmul(iv, sz) } else { vsub(path[i - 1], path[i - 2]) };
        let prev_sz = vlen(prev_delta);
        let mut nd = vadd(prev_delta, vmul(randn3(r), std));
        if let Some(p) = pull_dir {
            nd = vadd(nd, vmul(p, pull_factor));
        }
        nd = vmul(vunit(nd), prev_sz);
        let tm = 1.0 - (1.0 - momentum) * (i as f64 + 1.0) / n_pts as f64;
        let delta = vadd(vmul(prev_delta, tm), vmul(nd, 1.0 - tm));
        let delta = vmul(vunit(delta), sz);
        path.push(vadd(path[i - 1], delta));
    }
    path
}

/// quadratic-ish smooth resampling of a polyline (scipy interp1d quadratic through the control points) with `res` samples per segment
fn smooth_path(pts: &[V], res: usize) -> Vec<V> {
    let n = pts.len();
    let mut out = vec![];
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(n - 1)];
        for k in 0..res {
            let t = k as f64 / res as f64;
            // Catmull-Rom
            let f = |a: f64, b: f64, c: f64, d: f64| 0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t + (-a + 3.0 * b - 3.0 * c + d) * t * t * t);
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1]), f(p0[2], p1[2], p2[2], p3[2])]);
        }
    }
    out.push(pts[n - 1]);
    out
}

/// curve2mesh with a per-point radius and a closed 2D profile (x -> normal, y -> binormal), end caps included
fn sweep(path: &[V], radius: &[f64], tilt: &[f64], profile: &[[f64; 2]], caps: bool) -> Mesh {
    let mut c = Curve::new(path.to_vec(), false);
    c.tilt = tilt.to_vec();
    let tan = c.tangents();
    let nor = c.normals(&tan);
    let np = profile.len();
    let mut m = Mesh::new();
    for i in 0..path.len() {
        let b = vcross(tan[i], nor[i]);
        for p in profile {
            let o = vadd(vmul(nor[i], p[0] * radius[i]), vmul(b, p[1] * radius[i]));
            m.pos.push(vadd(path[i], o));
        }
    }
    for i in 0..path.len() - 1 {
        for j in 0..np {
            let j1 = (j + 1) % np;
            m.add_face_idx(&[(i * np + j) as u32, (i * np + j1) as u32, ((i + 1) * np + j1) as u32, ((i + 1) * np + j) as u32]);
        }
    }
    if caps {
        m.add_face_idx(&(0..np as u32).rev().collect::<Vec<_>>());
        let base = ((path.len() - 1) * np) as u32;
        m.add_face_idx(&(0..np as u32).map(|j| base + j).collect::<Vec<_>>());
    }
    m
}

/// nodegroup star profile: MeshCircle(3 n) with every even vertex pushed out by `ridge`
fn star_profile(n3: usize, ridge: f64, perturb: Option<(&mut Rng, f64)>, scale: [f64; 2]) -> Vec<[f64; 2]> {
    let mut jit: Vec<[f64; 2]> = vec![[0.0; 2]; n3];
    if let Some((r, p)) = perturb {
        for j in jit.iter_mut() {
            *j = [r.uniform(-p, p), r.uniform(-p, p)];
            let _ = r.uniform(-p, p);
        }
    }
    (0..n3)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n3 as f64;
            let mut p = [a.cos() + jit[i][0], a.sin() + jit[i][1]];
            if i % 2 == 0 {
                p = [p[0] * ridge, p[1] * ridge];
            }
            [p[0] * scale[0], p[1] * scale[1]]
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CKind {
    Globular,
    Columnar,
    PrickyPear,
}

pub struct CactusFactory {
    pub fs: u64,
    pub kind: CKind,
    pub spike_distance: f64,
}

pub struct Body {
    pub mesh: Mesh,
    /// ridge points (position, outward normal) used for spike placement
    pub ridge: Vec<(V, V)>,
}

impl CactusFactory {
    pub fn new(fs: u64) -> CactusFactory {
        let mut r = Rng::seeded(fs);
        let kind = [CKind::Globular, CKind::Columnar, CKind::PrickyPear][r.choice_p(&[1.0, 1.0, 1.0])];
        let spike_distance = 0.08;
        CactusFactory { fs, kind, spike_distance }
    }

    fn globular(&self, r: &mut Rng) -> Body {
        let star = r.randint(6, 12) as usize;
        let frequency = r.uniform(-0.2, 0.2);
        let ridge = r.uniform(1.1, 1.2);
        let anchors = [(0.0, r.uniform(0.2, 0.4)), (r.uniform(0.4, 0.6), r.log_uniform(0.5, 0.8)), (r.uniform(0.8, 0.85), r.uniform(0.4, 0.6)), (1.0, 0.05)];
        let mult = r.log_uniform(0.5, 1.0);
        let fc = FloatCurve::new(&anchors);
        let res = 64;
        let path: Vec<V> = (0..res).map(|i| [0.0, 0.0, i as f64 / (res - 1) as f64]).collect();
        let radius: Vec<f64> = (0..res).map(|i| fc.eval(i as f64 / (res - 1) as f64) * mult).collect();
        let tilt: Vec<f64> = (0..res).map(|i| i as f64 / (res - 1) as f64 * 2.0 * PI * frequency).collect();
        let prof = star_profile(star * 3, ridge, None, [1.0, 1.0]);
        let mut m = sweep(&path, &radius, &tilt, &prof, true);
        // geo_extension draws + noise
        let ne = r.uniform(0.1, 0.2);
        let ns = r.uniform(1.4, 2.8);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        m = subdivide_for_noise(&m);
        let m2 = super::gnhelpers::geo_extension(&m, off, ns, ne);
        m = m2;
        let s = [r.uniform(0.8, 1.5), r.uniform(0.8, 1.5), r.uniform(0.8, 1.5)];
        let rz = r.uniform(0.0, 2.0 * PI);
        m.transform(&Mat4::rot_z(rz).mul(&Mat4::scale(s)));
        // ridge points: even profile vertices
        let mut rp = vec![];
        let np = prof.len();
        let mx = Mat4::rot_z(rz).mul(&Mat4::scale(s));
        let _ = mx;
        for i in 0..res {
            for j in (0..np).step_by(2) {
                let k = i * np + j;
                let p = m.pos[k];
                // normal: from the axis
                let n = vunit([p[0], p[1], 0.0]);
                rp.push((p, n));
            }
        }
        Body { mesh: m, ridge: rp }
    }

    fn pad(&self, r: &mut Rng) -> Mesh {
        let res = 64;
        let anchors = [(0.0, r.uniform(0.15, 0.2)), (r.uniform(0.4, 0.6), r.log_uniform(0.4, 0.5)), (1.0, 0.05)];
        let mult = r.log_uniform(0.5, 1.5);
        let fc = FloatCurve::new(&anchors);
        let path: Vec<V> = (0..res).map(|i| [0.0, 0.0, i as f64 / (res - 1) as f64]).collect();
        let radius: Vec<f64> = (0..res).map(|i| fc.eval(i as f64 / (res - 1) as f64) * mult).collect();
        let prof: Vec<[f64; 2]> = (0..32).map(|i| [(2.0 * PI * i as f64 / 32.0).cos(), (2.0 * PI * i as f64 / 32.0).sin()]).collect();
        let mut m = sweep(&path, &radius, &vec![0.0; res], &prof, true);
        let ne = r.uniform(0.1, 0.2);
        let ns = r.uniform(1.4, 2.8);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        m = subdivide_for_noise(&m);
        m = super::gnhelpers::geo_extension(&m, off, ns, ne);
        let s = [r.uniform(0.8, 1.2), r.uniform(0.2, 0.25), r.uniform(0.8, 1.2)];
        m.transform(&Mat4::scale(s));
        m
    }

    fn pear_leaves(&self, r: &mut Rng, level: u32) -> Mesh {
        if level == 0 {
            return self.pad(r);
        }
        let n = r.randint(1, 3) as usize;
        let leaves: Vec<Mesh> = (0..n).map(|_| self.pear_leaves(r, level - 1)).collect();
        let base = self.pad(r);
        let mut angles = vec![-r.uniform(PI / 3.0, PI / 2.0), r.uniform(-PI / 16.0, PI / 16.0), r.uniform(PI / 3.0, PI / 2.0)];
        r.shuffle(&mut angles);
        let mut out = base.clone();
        for (k, leaf) in leaves.into_iter().enumerate() {
            let a = angles[k];
            let v = [a.sin(), 0.0, a.cos() + 0.5];
            let mut best = 0;
            for (i, p) in base.pos.iter().enumerate() {
                if vdot(*p, v) > vdot(base.pos[best], v) {
                    best = i;
                }
            }
            let mut l = leaf;
            for p in l.pos.iter_mut() {
                p[2] -= 0.15;
            }
            let sc = r.uniform(0.5, 0.75);
            let rz = r.uniform(-PI / 3.0, PI / 3.0);
            l.transform(&Mat4::translation(base.pos[best]).mul(&Mat4::euler_xyz([0.0, a, rz])).mul(&Mat4::scale([sc; 3])));
            out.append(&l);
        }
        out
    }

    fn columnar(&self, r: &mut Rng) -> Body {
        let resolution = 16usize;
        let base_radius = 0.25;
        // branch_config draws
        let n_minor = r.randint(10, 14) as usize;
        let b_minor = r.randint(2, 4) as usize;
        let angles = loop {
            let a: Vec<f64> = (0..b_minor).map(|_| r.uniform(0.0, 2.0 * PI)).collect();
            let mut s = a.clone();
            s.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let ok = (0..b_minor).all(|i| {
                let nxt = if i + 1 < b_minor { s[i + 1] } else { s[0] + 2.0 * PI };
                nxt - s[i] > PI / 3.0
            });
            if ok {
                break a;
            }
        };
        let mut paths: Vec<(Vec<V>, Vec<f64>)> = vec![];
        // major
        let radius_fn = |r: &mut Rng, base: f64, size: usize| -> Vec<f64> {
            let decay = r.uniform(0.5, 0.8);
            let root = r.uniform(0.7, 0.9);
            let alpha = r.uniform(2.0, 3.0);
            let total = size * resolution;
            let mut rad = vec![base * decay; total];
            for k in 0..resolution {
                rad[k] *= root.powf(1.0 - k as f64 / resolution as f64);
            }
            for k in 0..resolution {
                let idx = total - resolution + k;
                rad[idx] *= (1.0 - (k as f64 / resolution as f64).powf(alpha)).powf(1.0 / alpha);
            }
            rad
        };
        let major = rand_path(r, 16, 0.3, 0.4, 0.99, [0.0, 0.0, 1.0], [0.0; 3], None, 1.0, 0.0);
        let mut pts = major.clone();
        let det = smooth_path(&pts, resolution);
        let rad0 = radius_fn(r, 1.0, 15);
        let mut rad_major = vec![1.0];
        rad_major.extend(rad0);
        paths.push((det, rad_major.clone()));
        let _ = &mut pts;
        for idx in 0..b_minor {
            let n = major.len();
            let lo = (n as f64 * 0.2) as i64;
            let hi = (n as f64 * 0.6) as i64;
            let rnd_idx = r.randint(lo, hi) as usize;
            let curr = vsub(major[rnd_idx], major[rnd_idx - 1]);
            let axis2 = [angles[idx].cos(), angles[idx].sin(), 0.0];
            let rnd_ang = r.f64() * (PI / 2.0 - PI / 2.5) + PI / 2.5;
            let sign = if r.randn() >= 0.0 { 1.0 } else { -1.0 };
            let init_vec = rodrigues(curr, axis2, rnd_ang * sign);
            let path = rand_path(r, n_minor, 0.2, 0.4, 0.1, init_vec, major[rnd_idx], Some([0.0, 0.0, 1.0]), 0.0, 4.0);
            let det = smooth_path(&path, resolution);
            let base = rad_major[rnd_idx * resolution];
            let rad = radius_fn(r, base, n_minor - 1);
            let mut rv = vec![base];
            rv.extend(rad);
            paths.push((det, rv));
        }
        let star = r.randint(5, 8) as usize;
        let sc = [r.uniform(0.8, 1.0), r.uniform(0.8, 1.0)];
        let ridge = r.uniform(1.15, 1.25);
        let prof = star_profile(star * 3, ridge, None, sc);
        let mut mesh = Mesh::new();
        let mut rp = vec![];
        for (path, rad) in paths {
            let radius: Vec<f64> = rad.iter().take(path.len()).map(|x| x * base_radius).collect();
            let radius = if radius.len() < path.len() { let mut v = radius.clone(); while v.len() < path.len() { v.push(*v.last().unwrap()); } v } else { radius };
            let m = sweep(&path, &radius, &vec![0.0; path.len()], &prof, true);
            let np = prof.len();
            for i in (0..path.len()).step_by(2) {
                for j in (0..np).step_by(2) {
                    let p = m.pos[i * np + j];
                    let c = path[i];
                    rp.push((p, vunit(vsub(p, c))));
                }
            }
            mesh.append(&m);
        }
        let ne = r.uniform(0.1, 0.2);
        let ns = r.uniform(1.4, 2.8);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        let mesh = super::gnhelpers::geo_extension(&mesh, off, ns, ne);
        Body { mesh, ridge: rp }
    }

    /// CactusFactory.create_asset
    pub fn mesh(&self, inst: u64, face_size: f64) -> Mesh {
        let mut r = inst_rng(self.fs, inst);
        let body = match self.kind {
            CKind::Globular => self.globular(&mut r),
            CKind::Columnar => self.columnar(&mut r),
            CKind::PrickyPear => {
                let m = self.pear_leaves(&mut r, 2);
                let rp: Vec<(V, V)> = m.pos.iter().step_by(7).map(|p| (*p, vunit(*p))).collect();
                Body { mesh: m, ridge: rp }
            }
        };
        let mut mesh = body.mesh;
        // noise displacement
        let noise_strength = 0.02;
        let marble = r.choice_idx(2) == 1;
        let scale = r.log_uniform(0.1, 0.15);
        let kind = if marble { super::legtex::TexKind::Marble(0, 0) } else { super::legtex::TexKind::Stucci(0) };
        super::legtex::displace(&mut mesh, &super::legtex::Tex::new(kind, scale), noise_strength, 0.0);
        // spikes
        if face_size <= 0.05 {
            let sp = self.spikes(&mut r, &body.ridge);
            mesh.append(&sp);
        }
        mesh
    }

    fn spike_variant(&self, r: &mut Rng) -> Mesh {
        let mut out = Mesh::new();
        for _ in 0..4 {
            let sz = r.uniform(0.005, 0.01);
            // sample_direction(0.8)
            let mut dir = [0.0, 0.0, 1.0];
            for _ in 0..100 {
                let x = randn3(r);
                let y = vunit(x);
                if y[2] > 0.8 {
                    dir = y;
                    break;
                }
            }
            let path = rand_path(r, 9, sz, 0.5, 0.85, dir, [0.0; 3], None, 1.0, 0.0);
            let radius: Vec<f64> = (0..9).map(|k| 0.002 * 0.5f64.powf(k as f64 / 9.0)).collect();
            let prof: Vec<[f64; 2]> = (0..5).map(|i| [(2.0 * PI * i as f64 / 5.0).cos(), (2.0 * PI * i as f64 / 5.0).sin()]).collect();
            out.append(&sweep(&path, &radius, &vec![0.0; 9], &prof, true));
        }
        out
    }

    fn spikes(&self, r: &mut Rng, ridge: &[(V, V)]) -> Mesh {
        let variants: Vec<Mesh> = (0..5)
            .map(|_| {
                r.f64();
                self.spike_variant(r)
            })
            .collect();
        // merge-by-distance style thinning of the ridge points
        let d = self.spike_distance;
        let mut kept: Vec<(V, V)> = vec![];
        let mut grid: crate::dhash::HashMap<[i64; 3], Vec<usize>> = crate::dhash::HashMap::default();
        for &(p, n) in ridge {
            if !r.bool(0.9) {
                continue;
            }
            let k = [(p[0] / d).floor() as i64, (p[1] / d).floor() as i64, (p[2] / d).floor() as i64];
            let mut near = false;
            'o: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(l) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                            for &i in l {
                                if vlen(vsub(kept[i].0, p)) < d {
                                    near = true;
                                    break 'o;
                                }
                            }
                        }
                    }
                }
            }
            if !near {
                grid.entry(k).or_default().push(kept.len());
                kept.push((p, n));
            }
        }
        let mut out = Mesh::new();
        for (p, n) in kept {
            let v = &variants[r.choice_idx(variants.len())];
            // align z to the normal, random spin, partially pulled toward +z
            let spin = r.uniform(0.0, 2.0 * PI);
            let pull = r.uniform(0.2, 0.5);
            let dir = vunit(vadd(vmul(n, 1.0 - pull), vmul([0.0, 0.0, 1.0], pull)));
            let tilt = dir[2].clamp(-1.0, 1.0).acos();
            let az = dir[1].atan2(dir[0]);
            let sc = [r.uniform(0.5, 1.0), r.uniform(0.5, 1.0), r.uniform(0.5, 1.0)];
            let mut m = v.clone();
            m.transform(&Mat4::translation(p).mul(&Mat4::euler_xyz([0.0, tilt, az])).mul(&Mat4::rot_z(spin)).mul(&Mat4::scale(sc)));
            out.append(&m);
        }
        out
    }
}

/// 1 level of linear subdivision so the noise displacement has vertices to move
fn subdivide_for_noise(m: &Mesh) -> Mesh {
    super::subsurf::subsurf(m, 2, true)
}

impl Factory for CactusFactory {
    fn asset(&self, inst: u64) -> Asset {
        let mesh = self.mesh(inst, 0.003);
        let b = mesh.bounds();
        let mut a = Asset::default();
        a.mesh(mesh, &mat("cactus", [0.1, 0.3, 0.12], 0.5), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(CactusFactory::new(fs)))
}
