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
    WheatEar,
    VeratrumEar,
    Pinecone,
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
    pub primary_stem_offset: f64,
    pub ears: Vec<Growth>,
}

fn log_uniform(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    r.log_uniform(lo, hi)
}

impl Growth {
    /// MonocotGrowthFactory.__init__ only (the factory's own FixedSeed block); the caller fills the remaining attributes
    pub fn new_isolated(kind: Kind, fs: u64) -> Growth {
        Growth::new(kind, fs)
    }
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
            Kind::Banana | Kind::Taro | Kind::Pinecone => r.uniform(0.15, 0.35),
            Kind::WheatEar => r.uniform(0.12, 0.28),
            Kind::VeratrumEar => r.uniform(0.15, 0.35),
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
            primary_stem_offset: 0.0,
            ears: vec![],
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
                        g.scale_curve = vec![(0.0, 1.0), (1.0, 0.6)];
                        g.leaf_range = (0.1, 0.7);
                    }
                    _ => {}
                }
            }
            Kind::Veratrum => {
                g.stem_offset = r.uniform(1.0, 1.5);
                g.angle = r.uniform(PI / 4.0, PI / 3.0);
                g.z_drag = r.uniform(0.4, 0.5);
                g.bend_angle = PI / 2.0;
                g.min_y = r.uniform(PI * 0.25, PI * 0.35);
                g.max_y = r.uniform(PI * 0.6, PI * 0.7);
                g.count = log_uniform(&mut r, 32.0, 64.0) as i64;
                g.scale_curve = vec![(0.0, r.uniform(0.8, 1.0)), (0.4, 0.6), (0.8, r.uniform(0.0, 0.1)), (1.0, 0.0)];
                g.leaf_range = (0.0, r.uniform(0.7, 0.8));
                g.bud_angle = r.uniform(PI / 15.0, PI / 12.0);
                g.freq = r.uniform(25.0, 50.0);
                // VeratrumBranchMonocotFactory (continues this stream)
                let nb = (r.randint(3, 6) + 1) as usize;
                let mut ears: Vec<Growth> = vec![];
                for i in 0..nb {
                    let mut e = Growth::new_isolated(Kind::VeratrumEar, g.fs * 6 + i as u64);
                    e.angle = r.uniform(PI / 4.0, PI / 3.0);
                    e.min_y = r.uniform(PI * 0.25, PI * 0.3);
                    e.max_y = r.uniform(PI * 0.3, PI * 0.35);
                    e.count = r.randint(64, 128);
                    e.leaf_prob = r.uniform(0.6, 0.8);
                    e.leaf_range = (0.0, 0.98);
                    ears.push(e);
                }
                g.primary_stem_offset = r.uniform(0.4, 0.8);
                for (i, e) in ears.iter_mut().enumerate() {
                    let scale = if i > 0 { log_uniform(&mut r, 0.3, 0.6) } else { 1.0 };
                    e.stem_offset = scale * g.primary_stem_offset;
                    e.count = (log_uniform(&mut r, 64.0, 238.0) * scale) as i64;
                }
                g.ears = ears;
            }
            Kind::WheatEar => {
                g.stem_offset = r.uniform(0.4, 0.5);
                g.angle = r.uniform(PI / 6.0, PI / 4.0);
                g.min_y = r.uniform(PI / 4.0, PI / 3.0);
                g.max_y = PI / 2.0;
                g.leaf_prob = r.uniform(0.9, 1.0);
                g.count = log_uniform(&mut r, 96.0, 128.0) as i64;
                g.bend_angle = PI;
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
            Kind::Pinecone => {
                // PineconeFactory.__init__ restarts at the factory seed
                g.angle = 2.0 * PI / (r.randint(4, 8) as f64 + 0.5);
                g.max_y = r.uniform(0.7, 0.8) * PI / 2.0;
                g.leaf_prob = r.uniform(0.9, 0.95);
                g.count = log_uniform(&mut r, 64.0, 96.0) as i64;
                g.stem_offset = r.uniform(0.2, 0.4);
                g.perturb = 0.0;
                g.scale_curve = vec![(0.0, 0.5), (0.5, r.uniform(0.6, 1.0)), (1.0, r.uniform(0.1, 0.2))];
            }
            Kind::VeratrumEar => {}
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
                    // cut_plane: bisect_plane(clear_outer) -- approximated by dropping the vertices (and faces) beyond the plane; the sealing sheet of the python cut is not built
                    let angle = r.uniform(-PI / 3.0, PI / 3.0);
                    let cx = r.uniform(1.0, 1.4);
                    let (nx, ny) = (angle.cos() as f32, angle.sin() as f32);
                    let del: Vec<bool> = obj.v.iter().map(|p| (p[0] - cx as f32) * nx + p[1] * ny > 0.0).collect();
                    obj = obj.delete_verts(&del);
                }
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Pinecone => self.pinecone_leaf(r, face_size),
            Kind::Tussock => {
                let x = [0.0, r.uniform(0.3, 0.7), 1.0];
                let y = [0.0, 0.01, 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                decorate_leaf(&mut obj, r, 4.0, PI / 6.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::Grasses | Kind::Maize | Kind::Wheat => {
                let (x1, x2) = (r.uniform(0.1, 0.2), r.uniform(0.5, 0.7));
                let x = [0.0, x1, x2, 1.0];
                let (lo, hi) = if self.kind == Kind::Maize { (0.03, 0.06) } else { (0.02, 0.03) };
                let (y1, y2) = (r.uniform(lo, hi), r.uniform(lo, hi));
                let y = [0.0, y1, y2, 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                if self.kind != Kind::Maize && r.uniform(0.0, 1.0) < 0.4 {
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
            Kind::Veratrum => {
                let b = self.bud_angle;
                let x = [0.0, 0.2 * b.cos(), r.uniform(0.6, 0.7), 0.8];
                let y = [0.0, 0.2 * b.sin(), r.uniform(0.06, 0.1), 0.0];
                let mut obj = leaf(&x, &y, Some(face_size));
                let distance = obj.distance2boundary();
                let weights: Vec<f32> = distance.iter().map(|&d| (self.freq * d).cos().powi(4) as f32).collect();
                let strength = -r.uniform(5e-3, 8e-3);
                displace(&mut obj.v, &obj.f, None, strength, 0.0, DispDir::Normal, Some(&weights));
                obj.trace("DISPLACE");
                decorate_leaf(&mut obj, r, 8.0, PI / 2.0, PI / 6.0, 0.1, 0.02, true);
                obj
            }
            Kind::VeratrumEar => {
                let x = [0.0, 0.04, 0.06, 0.04, 0.0];
                let y = [0.0, 0.01, 0.0, -0.01, 0.0];
                let z = [0.0, -0.01, -0.01, -0.006, 0.0];
                let anchors: Vec<[f64; 3]> = (0..5).map(|i| [x[i], y[i], z[i]]).collect();
                let steps = r.randint(3, 5) as usize;
                let mut prof = bezier_mesh(&anchors, &[0, 2, 4]);
                prof.trace("WELD");
                prof.trace("WELD");
                // the outline closes on itself (first and last anchor coincide)
                let n = prof.v.len() as u32;
                prof.e.push([n - 1, 0]);
                let filled = fill_outline(&prof);
                filled.trace("fill");
                let plate = remesh_fill(&filled, 0.005);
                // spin(dupli) about the x axis: `steps` copies
                let mut copies: Vec<LMesh> = vec![];
                for k in 0..steps {
                    let ang = (2.0 * PI * k as f64 / steps as f64) as f32;
                    let (c, s) = (ang.cos(), ang.sin());
                    let mut m = plate.clone();
                    for p in m.v.iter_mut() {
                        let (yy, zz) = (p[1], p[2]);
                        p[1] = yy * c - zz * s;
                        p[2] = yy * s + zz * c;
                    }
                    copies.push(m);
                }
                let refs: Vec<&LMesh> = copies.iter().collect();
                let mut o = LMesh::join(&refs).weld(1e-3);
                o.trace("WELD");
                o = o.weld(face_size / 2.0);
                o.trace("WELD");
                o
            }
            Kind::WheatEar => {
                let x = [0.0, 0.05, 0.1];
                let y = [0.0, r.uniform(0.01, 0.015), 0.0];
                let n = r.randint(4, 6) as usize;
                let angles = polygon_angles(r, n, PI / 6.0, PI * 2.0 / 3.0);
                let mut pts: Vec<[f64; 3]> = vec![];
                let mut curves: Vec<LMesh> = vec![];
                for a in angles {
                    let anchors: Vec<[f64; 3]> = (0..3).map(|i| [x[i], a.cos() * y[i], a.sin() * y[i]]).collect();
                    let c = bezier_mesh(&anchors, &[]);
                    c.trace("WELD");
                    curves.push(c);
                }
                let refs: Vec<&LMesh> = curves.iter().collect();
                let j = LMesh::join(&refs);
                for p in &j.v {
                    pts.push([p[0] as f64, p[1] as f64, p[2] as f64]);
                }
                let tris = super::nature::hull_tris(&pts);
                let used: Vec<u32> = {
                    let mut u: Vec<u32> = tris.iter().flat_map(|t| t.iter().map(|&i| i as u32)).collect();
                    u.sort();
                    u.dedup();
                    u
                };
                let mut map = vec![u32::MAX; j.v.len()];
                let mut hull = LMesh::default();
                for (k, &i) in used.iter().enumerate() {
                    map[i as usize] = k as u32;
                    hull.v.push(j.v[i as usize]);
                }
                for t in &tris {
                    hull.f.push(vec![map[t[0]], map[t[1]], map[t[2]]]);
                }
                hull.ensure_face_edges();
                super::voxelremesh::remesh_with_attrs(&hull, face_size / 2.0)
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
        if let Ok(f) = std::env::var("CUTIN") {
            if let Some(st) = super::stagefile::read(&f) {
                if st.v.len() == obj.v.len() {
                    obj.v = st.v;
                }
            }
        }
        // (whole leaves. The tears python cuts here lift wedges of the leaf and delete the stretched edges, along the mesh's grid: every leaf edge
        // came out in stair steps and the blade looked chewed)
        let _ = r;
        obj
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
    T.get_or_init(|| serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/monocot_flower.json"))).unwrap())
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
        // nw.uniform for the perturbation only exists when perturb != 0
        let (s_rot, s_scl) = if self.perturb != 0.0 { (r.randint(0, 100000), r.randint(0, 100000)) } else { (0, 0) };
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
        if self.kind == Kind::Veratrum {
            let mut branches: Vec<LMesh> = self.ears.iter().map(|e| e.create_asset(r, face_size)).collect();
            for i in 1..branches.len() {
                let lz = (self.primary_stem_offset * r.uniform(0.0, 0.6)) as f32;
                let rx = r.uniform(PI * 0.25, PI * 0.4) as f32 as f64;
                let rz = r.uniform(0.0, PI * 2.0) as f32 as f64;
                // matrix = loc * Rz * Rx
                let (cx, sx, cz, sz) = (rx.cos() as f32, rx.sin() as f32, rz.cos() as f32, rz.sin() as f32);
                for p in branches[i].v.iter_mut() {
                    let (x, y, z) = (p[0], p[1], p[2]);
                    let (y1, z1) = (y * cx - z * sx, y * sx + z * cx);
                    p[0] = x * cz - y1 * sz;
                    p[1] = x * sz + y1 * cz;
                    p[2] = z1 + lz;
                }
            }
            let refs: Vec<&LMesh> = branches.iter().collect();
            let mut b = LMesh::join(&refs);
            let lz = (self.stem_offset - 0.02) as f32;
            for p in b.v.iter_mut() {
                p[2] += lz;
            }
            m = LMesh::join(&[&m, &b]);
        }
        if self.kind == Kind::Wheat {
            let ear_g = Growth::new(Kind::WheatEar, self.fs);
            let mut ear = ear_g.create_asset(r, face_size);
            let a = r.uniform(0.0, ear_g.bend_angle);
            simple_deform(&mut ear.v, Deform::Bend, a, 0);
            ear.trace("SIMPLE_DEFORM");
            let lz = (self.stem_offset - 0.02) as f32;
            for p in ear.v.iter_mut() {
                p[2] += lz;
            }
            m = LMesh::join(&[&m, &ear]);
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
/// Indoor pots hold houseplants: only the leafy kinds (Agave, Banana, Taro, Veratrum); the grass kinds (wheat / maize / tussock, 60 % straw
/// hues) read as dried grain in a pot. The renderer's leaf material is patched to match (tools/render/render_rust.py, grass=False).
pub fn kind_for_seed(fs: u64) -> Kind {
    let kinds = [Kind::Agave, Kind::Banana, Kind::Taro, Kind::Veratrum];
    let i = Rng::seeded(fs).choice_p(&[0.25; 4]);
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

/// kinds that have an exact port so far
pub fn supported(fs: u64) -> bool {
    matches!(kind_for_seed(fs), Kind::Agave | Kind::Maize | Kind::Grasses | Kind::Tussock | Kind::Banana | Kind::Taro | Kind::Wheat | Kind::Veratrum)
}

/// PlantContainerFactory.create_asset: origin2lowest(plant, approximate=True), then scale = min([side, side, top] / max|bound_box|), location z = dirt_z.
/// Returns the final vertex positions (float32 math as the object transform apply on join) and the scale.
pub fn fit_in_container(m: &LMesh, side: f64, top: f64, dirt_z: f64) -> (Vec<F3>, f32) {
    let n = m.v.len();
    let zs: Vec<f32> = m.v.iter().map(|p| p[2]).collect();
    let order = np_argsort_f32(&zs);
    let k = n / 10;
    let mut mean = [0f64; 3];
    for &i in &order[..k] {
        for c in 0..3 {
            mean[c] += m.v[i][c] as f64;
        }
    }
    for c in 0..3 {
        mean[c] /= k.max(1) as f64;
    }
    let zmin = m.v[order[0]][2];
    let loc = [(-mean[0]) as f32, (-mean[1]) as f32, -zmin];
    let mut v: Vec<F3> = m.v.iter().map(|p| [p[0] + loc[0], p[1] + loc[1], p[2] + loc[2]]).collect();
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &v {
        for c in 0..3 {
            lo[c] = lo[c].min(p[c]);
            hi[c] = hi[c].max(p[c]);
        }
    }
    let mx = |c: usize| (lo[c].abs()).max(hi[c].abs()) as f64;
    let scale = (side / mx(0)).min(side / mx(1)).min(top / mx(2)) as f32;
    let lz = dirt_z as f32;
    for p in v.iter_mut() {
        *p = [p[0] * scale, p[1] * scale, p[2] * scale + lz];
    }
    (v, scale)
}

impl Growth {
    /// PineconeFactory.build_leaf: filled 128-gon shaped by angles, convex hull, bent along x, scaled / tilted, voxel remeshed, tiny stucci displacement
    fn pinecone_leaf(&self, r: &mut Rng, face_size: f64) -> LMesh {
        use super::shell::{fill_grid_circle_best, QuadSpline};
        let n = 128usize;
        let lp: Vec<[f64; 3]> = (0..n)
            .map(|k| {
                let (sn, cs) = super::cabnodes::sin_cos_from_fraction(k, n);
                [((sn * -1.0f32) + 1.0f32) as f64, (cs * 1.0f32) as f64, 0.0]
            })
            .collect();
        let grid = fill_grid_circle_best(&lp, 114);
        let angles: Vec<f64> = [-1.0, -0.8, -0.5, 0.0, 0.5, 0.8, 1.0].iter().map(|a| a * self.angle / 2.0).collect();
        let scale = r.uniform(0.9, 0.95);
        let scales = [0.0, 0.7, scale, 1.0, scale, 0.7, 0.0];
        let disp = [0.0, 0.0, 0.0, -r.uniform(0.2, 0.3), 0.0, 0.0, 0.0];
        let fs_ = QuadSpline::new(&angles, &scales);
        let fd = QuadSpline::new(&angles, &disp);
        let mut pos: Vec<[f32; 3]> = grid.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let vert_angles: Vec<f64> = pos.iter().map(|p| (p[1] as f64).atan2(p[0] as f64)).collect();
        for (p, &a) in pos.iter_mut().zip(vert_angles.iter()) {
            let s = fs_.eval(a);
            *p = [(s * p[0] as f64) as f32, (s * p[1] as f64) as f32, (s * p[2] as f64) as f32];
        }
        for (p, &a) in pos.iter_mut().zip(vert_angles.iter()) {
            let nrm = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            p[2] = (p[2] as f64 + fd.eval(a) * nrm as f64) as f32;
        }
        // bpy.ops.mesh.convex_hull on the whole mesh (exact bmesh port)
        let faces = grid.faces.clone();
        let edges: Vec<[u32; 2]> = super::bathkit::edge_list(&faces).into_iter().map(|(a, b)| [a, b]).collect();
        let mut ce: Vec<Vec<u32>> = vec![];
        {
            let mut emap: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
            for (i, e) in edges.iter().enumerate() {
                emap.insert((e[0].min(e[1]), e[0].max(e[1])), i as u32);
            }
            for f in &faces {
                ce.push((0..f.len()).map(|k| emap[&(f[k].min(f[(k + 1) % f.len()]), f[k].max(f[(k + 1) % f.len()]))]).collect());
            }
        }
        let mut bm = crate::bm::BMesh::from_mesh(&pos, &edges, &faces, &ce);
        crate::bm::hull::convex_hull(&mut bm, &crate::bm::hull::HullParams::default());
        let (hp, he, hf) = bm.to_mesh();
        let mut v = hp;
        // shape_by_xs: z += quadratic interpolation of [0, 0, 0.5] over x in [0, 2]
        let fx = QuadSpline::new(&[0.0, 1.0, 2.0], &[0.0, 0.0, 0.5]);
        for p in v.iter_mut() {
            p[2] = (p[2] as f64 + fx.eval(p[0] as f64)) as f32;
        }
        // scale 0.1, rotation_euler[1] -= uniform(pi / 18, pi / 12), both applied
        let a = -(r.uniform(PI / 18.0, PI / 12.0) as f32);
        let (c, sn) = (a.cos(), a.sin());
        for p in v.iter_mut() {
            let q = [p[0] * 0.1f32, p[1] * 0.1f32, p[2] * 0.1f32];
            // R_y(a): x' = c x + s z ; z' = -s x + c z
            *p = [c * q[0] + sn * q[2], q[1], -sn * q[0] + c * q[2]];
        }
        let mut m = LMesh { v, f: hf, e: he };
        m.ensure_face_edges();
        let mut m = super::voxelremesh::remesh_with_attrs(&m, face_size);
        let ns = log_uniform(r, 0.002, 0.005);
        let tex = stucci(ns);
        displace(&mut m.v, &m.f, Some(&tex), 0.001, 0.0, DispDir::Normal, None);
        m.trace("DISPLACE");
        m
    }
}
