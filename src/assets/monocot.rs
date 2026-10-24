//! monocot/*.py (MonocotFactory and the growth factories Agave, Banana, Taro, Veratrum, Tussock, Grasses, Maize, Wheat):
//! RNG-exact ports; leaves are built by assets::leafkit (bezier outline, fill, dual-contouring remesh), decorated with float32 ports of the Blender modifiers
//! (assets::bpymod, legacy stucci textures in assets::legtex) and arranged by the exported `geo_flower` node group (data/gn/monocot_flower.json).
use super::bpymod::*;
use super::gnedit::*;
use super::leafkit::*;
use super::legtex::{Tex, TexKind};
use super::stagefile::trace;
use crate::gn;
use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::Ty;
use crate::rng::Rng;
use serde_json::{json, Value as J};
use std::f64::consts::PI;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Agave,
    Banana,
    Taro,
    Veratrum,
    Tussock,
    Grasses,
    Maize,
    Wheat,
}

impl Kind {
    pub fn is_grass(self) -> bool {
        matches!(self, Kind::Grasses | Kind::Maize | Kind::Wheat)
    }
}

/// state of a MonocotGrowthFactory after construction (all numeric attributes the builders read)
#[derive(Clone, Debug)]
pub struct Growth {
    pub kind: Kind,
    pub fs: u64,
    pub count: i64,
    pub perturb: f64,
    pub angle: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub leaf_prob: f64,
    pub leaf_range: (f64, f64),
    pub stem_offset: f64,
    pub scale_curve: Vec<(f64, f64)>,
    pub radius: f64,
    pub bend_angle: f64,
    pub twist_angle: f64,
    pub z_drag: f64,
    pub z_scale: f64,
    pub bud_angle: f64,
    pub cut_prob: f64,
    pub cut_angle: f64,
    pub freq: f64,
    pub n_cuts: i64,
    pub use_distance: bool,
}

fn log_uniform(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    r.log_uniform(lo, hi)
}

impl Growth {
    fn banana_block(&mut self, r: &mut Rng) {
        self.stem_offset = r.uniform(0.6, 1.0);
        self.angle = r.uniform(PI / 4.0, PI / 3.0);
        self.z_scale = r.uniform(1.0, 1.5);
        self.z_drag = r.uniform(0.1, 0.2);
        self.min_y = r.uniform(PI * 0.05, PI * 0.1);
        self.max_y = r.uniform(PI * 0.25, PI * 0.45);
        self.leaf_range = (r.uniform(0.5, 0.7), 1.0);
        self.count = log_uniform(r, 16.0, 24.0) as i64;
        self.scale_curve = vec![(0.0, r.uniform(0.4, 1.0)), (1.0, r.uniform(0.6, 1.0))];
        self.radius = r.uniform(0.04, 0.06);
        self.bud_angle = r.uniform(PI / 8.0, PI / 6.0);
        self.cut_angle = self.bud_angle + r.uniform(PI / 20.0, PI / 12.0);
        self.freq = log_uniform(r, 100.0, 300.0);
        self.n_cuts = if r.uniform(0.0, 1.0) < 0.8 { r.randint(6, 10) } else { 0 };
    }
    fn grasses_block(&mut self, r: &mut Rng) {
        self.stem_offset = r.uniform(1.5, 2.0);
        self.angle = r.uniform(PI / 6.0, PI / 3.0);
        self.z_drag = r.uniform(0.0, 0.2);
        self.min_y = r.uniform(PI * 0.35, PI * 0.45);
        self.max_y = r.uniform(PI * 0.45, PI * 0.5);
        self.count = log_uniform(r, 16.0, 64.0) as i64;
        self.scale_curve = vec![(0.0, 1.0), (1.0, 0.2)];
        self.bend_angle = PI / 2.0;
    }
    /// MonocotGrowthFactory.__init__ (draws, incl. the colour / shader draws of the material) then the subclass block (each restarts at the factory seed)
    pub fn new(kind: Kind, fs: u64) -> Growth {
        let mut r = Rng::seeded(fs);
        let leaf_prob = r.uniform(0.8, 0.9);
        let z_scale = r.uniform(1.0, 1.2);
        // base hue
        let _hue = match kind {
            Kind::Agave | Kind::Veratrum => r.uniform(0.12, 0.32),
            Kind::Tussock => {
                if r.uniform(0.0, 1.0) < 0.5 {
                    r.uniform(0.1, 0.15)
                } else {
                    r.uniform(0.25, 0.35)
                }
            }
            Kind::Grasses | Kind::Maize => {
                if r.uniform(0.0, 1.0) < 0.6 {
                    r.uniform(0.08, 0.12)
                } else {
                    r.uniform(0.2, 0.25)
                }
            }
            Kind::Wheat => r.uniform(0.08, 0.12),
            Kind::Banana | Kind::Taro => r.uniform(0.15, 0.35),
        };
        r.uniform(0.6, 0.8);
        log_uniform(&mut r, 0.05, 0.1);
        r.uniform(-0.03, 0.03);
        r.uniform(0.8, 1.0);
        log_uniform(&mut r, 0.05, 0.2);
        // shader_monocot
        let use_distance = kind == Kind::Agave;
        r.uniform(0.0, 0.2);
        if r.uniform(0.0, 1.0) >= 0.8 {
            r.uniform(0.2, 0.5);
        }
        if use_distance {
            r.uniform(1.8, 3.5);
        }
        let mut g = Growth {
            kind,
            fs,
            count: 128,
            perturb: 0.05,
            angle: PI / 6.0,
            min_y: 0.0,
            max_y: PI / 2.0,
            leaf_prob,
            leaf_range: (0.0, 1.0),
            stem_offset: 0.2,
            scale_curve: vec![(0.0, 1.0), (1.0, 1.0)],
            radius: 0.01,
            bend_angle: PI / 4.0,
            twist_angle: PI / 6.0,
            z_drag: 0.0,
            z_scale,
            bud_angle: 0.0,
            cut_prob: 0.0,
            cut_angle: 0.0,
            freq: 0.0,
            n_cuts: 0,
            use_distance,
        };
        let mut r = Rng::seeded(fs);
        match kind {
            Kind::Agave => {
                g.stem_offset = r.uniform(0.0, 0.5);
                g.angle = r.uniform(PI / 9.0, PI / 6.0);
                g.z_drag = r.uniform(0.05, 0.1);
                g.min_y = r.uniform(PI * 0.1, PI * 0.15);
                g.max_y = r.uniform(PI * 0.4, PI * 0.52);
                g.count = log_uniform(&mut r, 32.0, 64.0) as i64;
                g.scale_curve = vec![(0.0, r.uniform(0.8, 1.0)), (0.5, 1.0), (1.0, r.uniform(0.6, 1.0))];
                g.bud_angle = r.uniform(PI / 8.0, PI / 4.0);
                g.cut_prob = if r.uniform(0.0, 1.0) < 0.5 { 0.0 } else { r.uniform(0.2, 0.4) };
            }
            Kind::Tussock => {
                g.stem_offset = r.uniform(0.0, 0.2);
                g.angle = r.uniform(PI / 20.0, PI / 18.0);
                g.z_drag = r.uniform(0.1, 0.2);
                g.min_y = r.uniform(PI * 0.2, PI * 0.25);
                g.max_y = PI / 2.0;
                g.count = log_uniform(&mut r, 512.0, 1024.0) as i64;
                g.scale_curve = vec![(0.0, r.uniform(0.6, 1.0)), (1.0, r.uniform(0.6, 1.0))];
            }
            Kind::Grasses | Kind::Maize | Kind::Wheat => {
                g.grasses_block(&mut r);
                match kind {
                    Kind::Maize => {
                        let mut r = Rng::seeded(fs);
                        g.stem_offset = r.uniform(2.0, 2.5);
                        g.scale_curve = vec![(0.0, 1.0), (1.0, 0.6)];
                        g.leaf_range = (0.1, 0.7);
                    }
                    Kind::Wheat => {
                        let mut r = Rng::seeded(fs);
                        g.scale_curve = vec![(0.0, 1.0), (1.0, 0.6)];
                        g.leaf_range = (0.1, 0.7);
                        let _ = &mut r;
                    }
                    _ => {}
                }
            }
            Kind::Banana | Kind::Taro => {
                g.banana_block(&mut r);
                if kind == Kind::Taro {
                    let mut r = Rng::seeded(fs);
                    g.stem_offset = r.uniform(0.05, 0.1);
                    g.radius = r.uniform(0.02, 0.04);
                    g.z_drag = r.uniform(0.2, 0.3);
                    g.bud_angle = r.uniform(PI * 0.6, PI * 0.7);
                    g.freq = log_uniform(&mut r, 10.0, 20.0);
                    g.count = log_uniform(&mut r, 12.0, 16.0) as i64;
                    g.n_cuts = if r.uniform(0.0, 1.0) < 0.5 { r.randint(1, 2) } else { 0 };
                    g.min_y = r.uniform(-PI * 0.25, -PI * 0.05);
                    g.max_y = r.uniform(-PI * 0.05, 0.0);
                }
            }
            _ => unimplemented!("monocot kind {:?}", kind),
        }
        g
    }
}

fn rotate_y(v: &mut [F3], angle: f32) {
    // Euler (0, angle, 0) -> eul_to_mat3 in double, applied with mul_m4_v3 in float32
    let a = angle as f64;
    let (cj, sj) = (a.cos(), a.sin());
    let m = [[cj as f32, 0.0f32, (-sj) as f32], [0.0, 1.0, 0.0], [sj as f32, 0.0, cj as f32]];
    // mat[col][row] layout of the C matrix: x' = x*m[0][0] + y*m[1][0] + z*m[2][0]
    let c = [[cj as f32, 0.0, (-sj) as f32], [0.0, 1.0, 0.0], [sj as f32, 0.0, cj as f32]];
    let _ = m;
    for p in v.iter_mut() {
        let (x, y, z) = (p[0], p[1], p[2]);
        p[0] = x * c[0][0] + y * c[1][0] + z * c[2][0];
        p[1] = x * c[0][1] + y * c[1][1] + z * c[2][1];
        p[2] = x * c[0][2] + y * c[1][2] + z * c[2][2];
    }
}

fn stucci(noise_scale: f64) -> Tex {
    Tex::new(TexKind::Stucci(0), noise_scale)
}

fn geo_ext(m: &mut LMesh, r: &mut Rng, noise_strength: f64, noise_scale: f64) {
    // decorate.geo_extension draws
    let ns = r.uniform(noise_strength / 2.0, noise_strength);
    let sc = r.uniform(noise_scale * 0.7, noise_scale * 1.4);
    let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
    let mesh = crate::mesh::Mesh { pos: m.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.f.clone(), uv: vec![] };
    let out = super::gnhelpers::geo_extension(&mesh, off, sc, ns);
    for (i, p) in out.pos.iter().enumerate() {
        m.v[i] = [p[0] as f32, p[1] as f32, p[2] as f32];
    }
}

/// MonocotGrowthFactory.decorate_leaf
pub fn decorate_leaf(m: &mut LMesh, r: &mut Rng, y_ratio: f64, y_bend: f64, z_bend: f64, noise_scale: f64, strength: f64, leftmost: bool) {
    rotate_y(&mut m.v, (-PI / 2.0) as f32);
    let a = r.uniform(0.5, 1.0) * y_bend;
    simple_deform(&mut m.v, Deform::Bend, a, 1);
    m.trace("SIMPLE_DEFORM");
    rotate_y(&mut m.v, (PI / 2.0) as f32);
    let a = r.uniform(-1.0, 1.0) * z_bend;
    simple_deform(&mut m.v, Deform::Bend, a, 2);
    m.trace("SIMPLE_DEFORM");
    let k = r.uniform(0.0, 1.0);
    let c = (y_ratio * k) as f32;
    for p in m.v.iter_mut() {
        let y = p[1];
        let d = c * y * y;
        p[2] = (p[2] as f64 + d as f64) as f32;
    }
    geo_ext(m, r, 0.2, 2.0);
    m.trace("geo_geo_extension");
    let tex = stucci(noise_scale);
    displace(&mut m.v, &m.f, Some(&tex), strength, 0.5, DispDir::Normal, None);
    m.trace("DISPLACE");
    let dims = m.dimensions();
    for (d, w) in [(DispDir::X, dims[0]), (DispDir::Y, dims[1])] {
        let tex = stucci(noise_scale);
        let s = r.uniform(0.01, 0.02) * w as f64;
        displace(&mut m.v, &m.f, Some(&tex), s, 0.5, d, None);
        m.trace("DISPLACE");
    }
    if leftmost {
        origin2leftmost(m);
    }
}

pub fn origin2leftmost(m: &mut LMesh) {
    let mut i = 0;
    for (k, p) in m.v.iter().enumerate() {
        if p[0] < m.v[i][0] {
            i = k;
        }
    }
    let l = [-m.v[i][0], -m.v[i][1], -m.v[i][2]];
    for p in m.v.iter_mut() {
        p[0] += l[0];
        p[1] += l[1];
        p[2] += l[2];
    }
}

impl Growth {
    fn build_leaf(&self, r: &mut Rng, face_size: f64) -> LMesh {
        match self.kind {
            Kind::Agave => {
                let b = self.bud_angle;
                let x = [0.0, 0.2 * b.cos(), r.uniform(1.0, 1.4), 1.5];
                let y = [0.0, 0.2 * b.sin(), r.uniform(0.1, 0.15), 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                let distance = obj.distance2boundary();
                let z_offset = -log_uniform(r, 0.08, 0.16);
                let z_ratio = r.uniform(1.5, 2.5);
                let mut lower = obj.clone();
                for (i, p) in lower.v.iter_mut().enumerate() {
                    let d = (1.0 - (1.0 - distance[i]).powf(z_ratio)) * z_offset;
                    p[2] = (p[2] as f64 + d) as f32;
                }
                obj = LMesh::join(&[&lower, &obj]);
                obj = obj.weld(2e-4);
                obj.trace("WELD");
                if r.uniform(0.0, 1.0) < self.cut_prob {
                    unimplemented!("agave cut_plane");
                }
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Tussock => {
                let x = [0.0, r.uniform(0.3, 0.7), 1.0];
                let y = [0.0, 0.01, 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Grasses | Kind::Maize => {
                let (x1, x2) = (r.uniform(0.1, 0.2), r.uniform(0.5, 0.7));
                let x = [0.0, x1, x2, 1.0];
                let (lo, hi) = if self.kind == Kind::Maize { (0.03, 0.06) } else { (0.02, 0.03) };
                let (y1, y2) = (r.uniform(lo, hi), r.uniform(lo, hi));
                let y = [0.0, y1, y2, 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                if self.kind == Kind::Grasses && r.uniform(0.0, 1.0) < 0.4 {
                    let x_cut = r.uniform(0.5, 1.0);
                    let ang = r.uniform(-PI / 3.0, PI / 3.0);
                    let (ca, sa) = (ang.cos() as f32, ang.sin() as f32);
                    let xc = x_cut as f32;
                    let del: Vec<bool> = obj.v.iter().map(|p| (p[0] - xc) * ca + p[1] * sa > 0.0).collect();
                    obj = obj.delete_verts(&del);
                }
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Banana => {
                let b = self.bud_angle;
                let x = [0.0, 0.2 * b.cos(), r.uniform(0.8, 1.2), 2.0];
                let y = [0.0, 0.2 * b.sin(), r.uniform(0.2, 0.25), 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                obj = self.cut_leaf(obj, r);
                self.displace_veins(&mut obj, r);
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Taro => {
                let b = self.bud_angle;
                let x = [0.0, 0.2 * b.cos(), r.uniform(0.4, 1.0), r.uniform(0.8, 1.0)];
                let y = [0.0, 0.2 * b.sin(), r.uniform(0.25, 0.3), 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                obj = self.cut_leaf(obj, r);
                self.displace_veins(&mut obj, r);
                decorate_leaf(&mut obj, r, 2.0, PI / 6.0, PI / 6.0, 0.1, 0.02, false);
                let branch = self.build_branch(r);
                let mut o = LMesh::join(&[&obj, &branch]);
                // origin2lowest
                let mut i = 0;
                for (k, p) in o.v.iter().enumerate() {
                    if p[2] < o.v[i][2] {
                        i = k;
                    }
                }
                let l = [-o.v[i][0], -o.v[i][1], -o.v[i][2]];
                for p in o.v.iter_mut() {
                    p[0] += l[0];
                    p[1] += l[1];
                    p[2] += l[2];
                }
                o
            }
            _ => unimplemented!("leaf of {:?}", self.kind),
        }
    }

    fn build_branch(&self, r: &mut Rng) -> LMesh {
        let offset = r.uniform(0.2, 0.3);
        let length = r.uniform(1.0, 2.0);
        let x = [0.0, -0.05, -offset - r.uniform(0.01, 0.02), -offset];
        let z = [0.0, 0.0, -length + 0.1, -length];
        let anchors: Vec<[f64; 3]> = (0..4).map(|i| [x[i], 0.0, z[i]]).collect();
        let bm = bezier_mesh(&anchors, &[]);
        bm.trace("WELD");
        let line: Vec<[f64; 3]> = bm.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        let rad = r.uniform(0.02, 0.03);
        let tube = super::gnhelpers::geo_radius(&[line], rad, 32, 0.004);
        let mut m = LMesh { v: tube.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: tube.faces.clone(), e: vec![] };
        m.ensure_face_edges();
        m.trace("geo_geo_radius");
        m
    }

    /// banana / taro cut_leaf
    fn cut_leaf(&self, mut obj: LMesh, r: &mut Rng) -> LMesh {
        let n = self.n_cuts as usize;
        let sel: Vec<usize> = (0..obj.v.len()).filter(|&i| obj.v[i][1].abs() < 0.08 && obj.v[i][1].abs() > 0.01).collect();
        let mut pos: Vec<usize> = sel.iter().copied().filter(|&i| obj.v[i][1] > 0.0).collect();
        let mut neg: Vec<usize> = sel.iter().copied().filter(|&i| obj.v[i][1] < 0.0).collect();
        let keys: Vec<f32> = pos.iter().map(|&i| obj.v[i][0]).collect();
        pos = np_argsort_f32(&keys).into_iter().map(|k| pos[k]).collect();
        let keys: Vec<f32> = neg.iter().map(|&i| obj.v[i][0]).collect();
        neg = np_argsort_f32(&keys).into_iter().map(|k| neg[k]).collect();
        let choose = |r: &mut Rng, src: &Vec<usize>| -> Vec<usize> {
            let mut perm: Vec<usize> = (0..src.len()).collect();
            r.shuffle(&mut perm);
            perm.iter().take(n).map(|&k| src[k]).collect()
        };
        let pc = choose(r, &pos);
        let nc = choose(r, &neg);
        if std::env::var("LEAFDBG").is_ok() {
            eprintln!("cut_leaf: pos {} neg {} chosen pos {:?}", pos.len(), neg.len(), pc.iter().map(|&i| (obj.v[i][0], obj.v[i][1])).collect::<Vec<_>>());
        }
        let pts: Vec<[f32; 3]> = obj.v.clone();
        let mut pairs: Vec<(usize, usize)> = vec![];
        for k in 0..pc.len().saturating_sub(1) {
            pairs.push((pc[k], pc[k + 1]));
        }
        for k in 0..nc.len().saturating_sub(1) {
            pairs.push((nc[k], nc[k + 1]));
        }
        let (sa, ca) = (self.cut_angle.sin(), self.cut_angle.cos());
        for (i1, i2) in pairs {
            let (x1, y1) = (pts[i1][0] as f64, pts[i1][1] as f64);
            let (x2, y2) = (pts[i2][0] as f64, pts[i2][1] as f64);
            let coeff = if y1 > 0.0 { 1.0f64 } else { -1.0 };
            let ratio = r.uniform(-2.0, 0.4);
            let exponent = r.uniform(1.2, 1.6);
            if std::env::var("LEAFDBG").is_ok() {
                eprintln!("  cut ratio {ratio} exp {exponent} x1 {x1} y1 {y1}");
            }
            let m1 = x1 * sa - y1 * ca * coeff;
            let m2 = x2 * sa - y2 * ca * coeff;
            let den = ((x1 - x2).powi(2) + (y1 - y2).powi(2) + 0.1).sqrt();
            for p in obj.v.iter_mut() {
                let (x, y) = (p[0] as f64, p[1] as f64);
                let m = x * sa - y * ca * coeff;
                let dist = ((x - x1) * (y1 - y2) + (y - y1) * (x1 - x2)) / den;
                if m1 < m && m < m2 && dist * coeff < 0.0 {
                    p[2] = (p[2] as f64 + ratio * dist.abs().powf(exponent)) as f32;
                }
            }
        }
        let del: Vec<bool> = obj
            .e
            .iter()
            .map(|e| {
                let (a, b) = (obj.v[e[0] as usize], obj.v[e[1] as usize]);
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() > 0.02
            })
            .collect();
        obj.delete_edges(&del)
    }

    fn displace_veins(&self, obj: &mut LMesh, r: &mut Rng) {
        let n = obj.v.len();
        let weights: Vec<f32>;
        if self.kind == Kind::Taro {
            // point_normal_up: flip faces whose normal points down
            for fc in obj.f.iter_mut() {
                let mut nz = 0.0f32;
                let k = fc.len();
                for i in 0..k {
                    let (a, b) = (obj.v[fc[i] as usize], obj.v[fc[(i + 1) % k] as usize]);
                    nz += (a[0] - b[0]) * (a[1] + b[1]);
                }
                // Newell z = sum (x_i - x_{i+1}) (y_i + y_{i+1}) ... sign matches the polygon normal
                if nz < 0.0 {
                    fc[1..].reverse();
                }
            }
            let ph = r.uniform(0.0, PI * 2.0);
            let oy = r.uniform(0.1, 0.2);
            let ox = r.uniform(0.1, 0.4);
            let thr: Vec<f64> = (0..n).map(|_| r.uniform(0.98, 0.99)).collect();
            let thr2: Vec<f64> = (0..n).map(|_| r.uniform(0.002, 0.008)).collect();
            weights = (0..n)
                .map(|i| {
                    let (x, y) = (obj.v[i][0] as f64, obj.v[i][1] as f64);
                    let a = (y - if y > 0.0 { -1.0 } else { 1.0 } * oy).atan2(x - ox);
                    let branch = (ph + a * self.freq).cos() > thr[i];
                    let lf = y.abs() < thr2[i];
                    if branch || lf { 1.0 } else { 0.0 }
                })
                .collect();
        } else {
            let thr: Vec<f64> = (0..n).map(|_| r.uniform(0.85, 0.9)).collect();
            let thr2: Vec<f64> = (0..n).map(|_| r.uniform(0.002, 0.008)).collect();
            let (sa, ca) = (self.cut_angle.sin(), self.cut_angle.cos());
            weights = (0..n)
                .map(|i| {
                    let (x, y) = (obj.v[i][0] as f64, obj.v[i][1] as f64);
                    let branch = ((y.abs() * ca - x * sa) * self.freq).cos() > thr[i];
                    let lf = y.abs() < thr2[i];
                    if branch || lf { 1.0 } else { 0.0 }
                })
                .collect();
        }
        let strength = -r.uniform(5e-3, 8e-3);
        displace(&mut obj.v, &obj.f, None, strength, 0.0, DispDir::Normal, Some(&weights));
        obj.trace("DISPLACE");
    }

    fn build_instance(&self, r: &mut Rng, face_size: f64) -> LMesh {
        if self.kind == Kind::Taro {
            return self.build_leaf(r, face_size);
        }
        let mut o = self.build_leaf(r, face_size);
        origin2leftmost(&mut o);
        let dx = -(0.01f32);
        for p in o.v.iter_mut() {
            p[0] += dx;
        }
        o
    }
}

fn tpl() -> &'static J {
    static T: OnceLock<J> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/monocot_flower.json")).unwrap())
}

fn gmesh_of(m: &LMesh) -> GMeshT {
    GMeshT::from_faces(m.v.clone(), &m.f, &m.e)
}

impl Growth {
    /// create_raw: leaves collection, stem, geo_flower. Returns the joined stem + leaves.
    pub fn create_raw(&self, r: &mut Rng, face_size: f64) -> LMesh {
        let mut g = self.clone();
        if g.angle != 0.0 {
            let mut frequency = 2.0 * PI / g.angle;
            let fr = frequency - frequency.trunc();
            if 0.01 < fr && fr < 0.05 {
                frequency += 0.05;
            } else if -0.05 < fr && fr < -0.01 {
                frequency -= 0.05;
            }
            g.angle = 2.0 * PI / frequency;
        }
        // make_collection: 10 leaves, each preceded by choice([0], p=[1.]) (one double)
        let mut leaves: Vec<LMesh> = vec![];
        for _ in 0..10 {
            r.f64();
            leaves.push(g.build_instance(r, face_size));
        }
        let stem = g.build_stem(face_size);
        g.flower(r, &stem, &leaves)
    }

    fn build_stem(&self, face_size: f64) -> LMesh {
        // SUBSURF levels 9 on a single edge: repeated midpoint insertion
        let mut zs: Vec<f32> = vec![0.0, self.stem_offset as f32];
        for _ in 0..9 {
            let mut n = vec![];
            for i in 0..zs.len() - 1 {
                n.push(zs[i]);
                n.push((zs[i] + zs[i + 1]) / 2.0);
            }
            n.push(*zs.last().unwrap());
            zs = n;
        }
        let line: Vec<[f64; 3]> = zs.iter().map(|&z| [0.0, 0.0, z as f64]).collect();
        let lm = LMesh { v: line.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: vec![], e: (0..line.len() as u32 - 1).map(|i| [i, i + 1]).collect() };
        lm.trace("SUBSURF");
        let tube = super::gnhelpers::geo_radius(&[line], self.radius, 16, 0.004);
        let mut m = LMesh { v: tube.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: tube.faces.clone(), e: vec![] };
        m.ensure_face_edges();
        m.trace("geo_geo_radius");
        // adapt_mesh_resolution(face_size, "subdivide")
        if !m.f.is_empty() {
            let mut lens: Vec<f32> = m
                .e
                .iter()
                .map(|e| {
                    let (a, b) = (m.v[e[0] as usize], m.v[e[1] as usize]);
                    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
                })
                .collect();
            lens.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = lens.len();
            let lmax = if n <= 4 { lens[n - 1] } else { lens[n - ((n + 3) / 4)] } as f64;
            if lmax > face_size {
                let levels = ((lmax / face_size).log2().ceil() as i64).min(6);
                if levels > 0 {
                    let mesh = crate::mesh::Mesh { pos: m.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.f.clone(), uv: vec![] };
                    let s = super::subsurf::subsurf(&mesh, levels as usize, false);
                    m = LMesh { v: s.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: s.faces, e: vec![] };
                    m.ensure_face_edges();
                    m.trace("SUBSURF");
                }
            }
        }
        let tex = stucci(0.1);
        displace(&mut m.v, &m.f, Some(&tex), 0.01, 0.5, DispDir::Normal, None);
        m.trace("DISPLACE");
        m
    }

    fn flower(&self, r: &mut Rng, stem: &LMesh, leaves: &[LMesh]) -> LMesh {
        let mut j = tpl().clone();
        let top = j["main"].as_str().unwrap().to_string();
        let top = top.as_str();
        // node construction draws: accumulate-field angle seed, rotation / scale perturbation seeds, bernoulli seed
        let s_acc = r.randint(0, 100000);
        let s_rot = r.randint(0, 100000);
        let s_scl = r.randint(0, 100000);
        let s_ber = r.randint(0, 100000);
        set_in(&mut j, top, "Curve Line", "End", json!([0.0, 0.0, self.stem_offset]));
        set_in(&mut j, top, "Resample Curve", "Count", json!(self.count));
        set_in(&mut j, top, "Random Value", "Min_001", json!(self.angle * 0.95));
        set_in(&mut j, top, "Random Value", "Max_001", json!(self.angle * 1.05));
        set_in(&mut j, top, "Random Value", "Seed", json!(s_acc));
        for (n, s) in [("Random Value.001", s_rot), ("Random Value.002", s_scl)] {
            set_in(&mut j, top, n, "Min", json!([-self.perturb, -self.perturb, -self.perturb]));
            set_in(&mut j, top, n, "Max", json!([self.perturb, self.perturb, self.perturb]));
            set_in(&mut j, top, n, "Seed", json!(s));
        }
        set_in(&mut j, top, "Random Value.003", "Probability", json!(self.leaf_prob));
        set_in(&mut j, top, "Random Value.003", "Seed", json!(s_ber));
        set_in(&mut j, top, "Compare", "B", json!(self.leaf_range.0));
        set_in(&mut j, top, "Compare.001", "B", json!(self.leaf_range.1));
        node_mut(&mut j, top, "Float Curve")["p"]["curve"]["pts"] = json!([[0.0, -self.min_y, "VECTOR"], [1.0, -self.max_y, "VECTOR"]]);
        let pts: Vec<J> = self.scale_curve.iter().map(|&(x, y)| json!([x, y, "AUTO"])).collect();
        node_mut(&mut j, top, "Float Curve.001")["p"]["curve"]["pts"] = J::Array(pts);
        // scene objects: the leaves collection
        j["collections"] = json!({"assets:leaves": {"objects": (0..leaves.len()).map(|i| format!("leaf{:02}", i)).collect::<Vec<_>>(), "children": [], "offset": [0.0, 0.0, 0.0]}});
        let groups = gn::load::load_groups(j);
        let mut objects = gn::Objects::default();
        let mut names = vec![];
        for (i, l) in leaves.iter().enumerate() {
            let name = format!("leaf{:02}", i);
            objects.map.insert(name.clone(), gn::eval::Object { geo: GGeom::from_mesh(gmesh_of(l)), matrix: gn::M4_ID });
            names.push(name);
        }
        objects.collections.insert("assets:leaves".to_string(), gn::eval::Collection { objects: names, children: vec![], offset: [0.0; 3] });
        let gdef = &groups.map[top];
        let gi = gdef.inputs.iter().find(|i| i.ty == Ty::Geo).unwrap();
        let mut inputs: crate::dhash::HashMap<String, gn::Value> = crate::dhash::HashMap::default();
        inputs.insert(gi.id.clone(), gn::Value::geo(GGeom::from_mesh(gmesh_of(stem))));
        let outs = gn::eval(&groups, top, &inputs, &objects).expect("geo_flower eval");
        let oi = gdef.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
        let g = match &outs[oi].1 {
            gn::Value::G(gg) => gn::meshops::realize_instances(gg, 1000),
            _ => panic!("geo_flower output"),
        };
        let gm = g.mesh.as_ref().unwrap();
        let mut m = LMesh { v: gm.pos.clone(), f: gm.faces_vec(), e: gm.edges.clone() };
        m.trace("geo_geo_flower");
        m
    }

    /// decorate_monocot
    pub fn decorate_monocot(&self, m: &mut LMesh, r: &mut Rng) {
        let zd = (-self.z_drag) as f32;
        for p in m.v.iter_mut() {
            let d = zd * (p[0] * p[0] + p[1] * p[1]);
            p[2] += d;
        }
        geo_ext(m, r, 0.4, 2.0);
        m.trace("geo_geo_extension");
        let a = r.uniform(-self.twist_angle, self.twist_angle);
        simple_deform(&mut m.v, Deform::Twist, a, 2);
        m.trace("SIMPLE_DEFORM");
        let a = r.uniform(0.0, self.bend_angle);
        simple_deform(&mut m.v, Deform::Bend, a, 0);
        m.trace("SIMPLE_DEFORM");
        let s = [r.uniform(0.8, 1.2) as f32, r.uniform(0.8, 1.2) as f32, self.z_scale as f32];
        let rz = r.uniform(0.0, PI * 2.0) as f32 as f64;
        let (c, sn) = (rz.cos(), rz.sin());
        // M = R * S ; columns of R scaled; x' = x*M[0][0] + y*M[1][0] + z*M[2][0]
        let m00 = c as f32 * s[0];
        let m01 = sn as f32 * s[0];
        let m10 = (-sn) as f32 * s[1];
        let m11 = c as f32 * s[1];
        let m22 = s[2];
        for p in m.v.iter_mut() {
            let (x, y, z) = (p[0], p[1], p[2]);
            p[0] = x * m00 + y * m10 + z * 0.0;
            p[1] = x * m01 + y * m11 + z * 0.0;
            p[2] = x * 0.0 + y * 0.0 + z * m22;
        }
    }

    /// MonocotGrowthFactory.create_asset for the non-grass kinds (the instance RNG `r` is already seeded)
    pub fn create_asset(&self, r: &mut Rng, face_size: f64) -> LMesh {
        let mut m = self.create_raw(r, face_size);
        if self.kind == Kind::Maize {
            let husk = self.build_husk(r);
            m = LMesh::join(&[&m, &husk]);
        }
        self.decorate_monocot(&mut m, r);
        m
    }

    fn build_husk(&self, r: &mut Rng) -> LMesh {
        let x = [0.0, r.uniform(0.04, 0.05), r.uniform(0.03, 0.03), 0.0];
        let z = [0.0, 0.01, r.uniform(0.24, 0.3), r.uniform(0.35, 0.4)];
        let anchors: Vec<[f64; 3]> = (0..4).map(|i| [x[i], 0.0, z[i]]).collect();
        let bm = bezier_mesh(&anchors, &[]);
        bm.trace("WELD");
        bm.trace("WELD");
        let sp = super::blops::spin_anchors(&anchors, &[]);
        let mut m = LMesh { v: sp.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: sp.faces.clone(), e: vec![] };
        m.ensure_face_edges();
        let tex = stucci(0.01);
        displace(&mut m.v, &m.f, Some(&tex), 0.02, 0.5, DispDir::Normal, None);
        m.trace("DISPLACE");
        let rx = r.uniform(0.0, PI * 0.2) as f32 as f64;
        let (c, s) = (rx.cos() as f32, rx.sin() as f32);
        let lz = (self.stem_offset - 0.02) as f32;
        for p in m.v.iter_mut() {
            let (x, y, z) = (p[0], p[1], p[2]);
            p[0] = x * 1.0 + y * 0.0 + z * 0.0 + 0.0;
            p[1] = x * 0.0 + y * c + z * -s + 0.0;
            p[2] = x * 0.0 + y * s + z * c + lz;
        }
        m
    }
}

pub fn run_trace_unused() {
    let _ = trace;
}

use super::{inst_rng, Asset, Factory};
use crate::mesh::Mesh;

pub const FACE_SIZE: f64 = 0.003;

pub fn handles(c: &str) -> bool {
    c == "MonocotFactory"
}

pub struct MonocotFactory {
    pub fs: u64,
    pub g: Growth,
}

/// kind chosen by MonocotFactory.__init__: np.random.choice over [Tussock, Grasses, Wheat, Maize, Agave, Banana, Taro, Veratrum], uniform p
pub fn kind_for_seed(fs: u64) -> Kind {
    let kinds = [Kind::Tussock, Kind::Grasses, Kind::Wheat, Kind::Maize, Kind::Agave, Kind::Banana, Kind::Taro, Kind::Veratrum];
    let i = Rng::seeded(fs).choice_p(&[0.125; 8]);
    kinds[i]
}

impl MonocotFactory {
    pub fn new(fs: u64) -> MonocotFactory {
        MonocotFactory { fs, g: Growth::new(kind_for_seed(fs), fs) }
    }
    pub fn mesh(&self, inst: u64) -> LMesh {
        let mut r = inst_rng(self.fs, inst);
        if self.g.kind.is_grass() {
            let n = r.randint(1, 6) as usize;
            let angles = polygon_angles(&mut r, n, PI / 4.0, PI * 2.0);
            let radius: Vec<f64> = (0..n).map(|_| r.uniform(0.08, 0.16)).collect();
            let mut parts: Vec<LMesh> = vec![];
            // join_objects keeps the first object as the active one: every other part is placed relative to it
            let loc = |j: usize| ((radius[j] * angles[j].cos()) as f32, (radius[j] * angles[j].sin()) as f32);
            for j in 0..n {
                let mut m = self.g.create_asset(&mut r, FACE_SIZE);
                let (lx, ly) = if j == 0 { (0.0, 0.0) } else { (loc(j).0 - loc(0).0, loc(j).1 - loc(0).1) };
                for p in m.v.iter_mut() {
                    p[0] += lx;
                    p[1] += ly;
                }
                parts.push(m);
            }
            let refs: Vec<&LMesh> = parts.iter().collect();
            return LMesh::join(&refs);
        }
        self.g.create_asset(&mut r, FACE_SIZE)
    }
}

impl Factory for MonocotFactory {
    fn asset(&self, inst: u64) -> Asset {
        let m = self.mesh(inst);
        let mesh = Mesh { pos: m.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(), faces: m.f.clone(), uv: vec![] };
        let mut a = Asset::default();
        let b = mesh.bounds();
        a.mesh(mesh, &super::mat("monocot", [0.15, 0.3, 0.1], 0.55), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(MonocotFactory::new(fs)))
}

/// assets/utils/mesh.polygon_angles
pub fn polygon_angles(r: &mut Rng, n: usize, min_angle: f64, max_angle: f64) -> Vec<f64> {
    for _ in 0..100 {
        let mut a: Vec<f64> = (0..n).map(|_| r.uniform(0.0, 2.0 * PI)).collect();
        a.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let ok = (0..n).all(|i| {
            let d = (a[i] - a[(i + n - 1) % n]).rem_euclid(2.0 * PI);
            d >= min_angle && d <= max_angle
        });
        if ok {
            return a;
        }
    }
    let off = r.uniform(0.0, 2.0 * PI);
    let mut a: Vec<f64> = (0..n).map(|i| (i as f64 * (2.0 * PI / n as f64) + off).rem_euclid(2.0 * PI)).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    a
}
