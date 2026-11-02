//! seating/mattress.py: MattressFactory. Coiled = poke/quads/dots (coil.rs, exact stages); wrapped = bevelled box inflated by the
//! native cloth solver (approximate by design).
use super::bevelfull::bevel_angle_ex;
use super::cloth::{simulate_job, Calib, ClothParams};
use super::coil::make_coiled;
use super::matdraw::*;
use super::towel::subdivide_edge_ring;
use super::{inst_rng, Asset, Factory, Part};
use crate::materials::spec_material;
use crate::mesh::*;
use crate::rng::Rng;
use serde_json::json;

pub const FABRICS_W: Table = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];

/// BED_SIM=1: bedding and wrapped mattresses from the cloth simulation (else from formulas)
pub fn sim_bedding() -> bool {
    std::env::var_os("BED_SIM").is_some()
}

#[derive(Clone, Debug)]
pub struct MattressSpec {
    pub width: f64,
    pub size: f64,
    pub thickness: f64,
    pub dot_distance: f64,
    pub dot_size: f64,
    pub dot_depth: f64,
    pub coiled: bool,
    pub mat_cls: &'static str,
}

impl MattressSpec {
    /// MattressFactory.__init__ (FixedSeed(fseed))
    pub fn new(fseed: u64) -> MattressSpec {
        let mut mr = Rng::seeded(fseed);
        let width = mr.log_uniform(0.9, 2.0);
        let size = mr.uniform(2.0, 2.4);
        let thickness = mr.uniform(0.2, 0.35);
        let dot_distance = mr.log_uniform(0.16, 0.2);
        let dot_size = mr.uniform(0.005, 0.02);
        let dot_depth = mr.uniform(0.04, 0.08);
        let coiled = mr.choice_p(&[0.5, 0.5]) == 0;
        let mat_cls = sample_call(&mut mr, FABRICS_W);
        MattressSpec { width, size, thickness, dot_distance, dot_size, dot_depth, coiled, mat_cls }
    }

    /// create_asset: box centred at the origin, `r` = the spawn_asset stream
    /// a wrapped mattress's fill: (how far the sides belly, how far the top domes); the simulated one's is larger
    pub fn puff(&self) -> (f64, f64) {
        let (w, s, t) = (self.width, self.size, self.thickness);
        if self.coiled { return (0.0, 0.0) }
        if sim_bedding() { (0.06 * t + 0.003, 0.075 * w.min(s)) } else { (0.03 * t + 0.002, 0.03 * w.min(s)) }
    }

    /// the top's dome over (x, y) in the mattress's frame: 1 in the middle, 0 at the edges
    pub fn dome(&self, x: f64, y: f64) -> f64 {
        let (u, v) = ((2.0 * x / self.width).clamp(-1.0, 1.0), (2.0 * y / self.size).clamp(-1.0, 1.0));
        (1.0 - u.powi(4)) * (1.0 - v.powi(4))
    }

    /// the mattress's bounds from its parameters: the block (width x size x thickness, centred), puffed by the build -- a coiled one
    /// by its coils, a wrapped one by the pressure inflating its cover
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let (w, s, t) = (self.width, self.size, self.thickness);
        // (a coiled block stays within its box -- the coils dent it; a wrapped one's cover fills out: its top and sides, the bottom stays)
        if self.coiled { return ([-w / 2.0, -s / 2.0, -t / 2.0], [w / 2.0, s / 2.0, t / 2.0]) }
        let (side, top) = self.puff();
        ([-w / 2.0 - side, -s / 2.0 - side, -t / 2.0], [w / 2.0 + side, s / 2.0 + side, t / 2.0 + top])
    }

    pub fn build(&self, r: &mut Rng) -> Mesh {
        self.build_job(r).0
    }

    /// like `build`, plus the cloth job inputs of the wrapped type: (pre mesh, pressure, pin vertices)
    pub fn build_job(&self, r: &mut Rng) -> (Mesh, Option<(Mesh, f64, Vec<u32>)>) {
        let (w, s, t) = (self.width, self.size, self.thickness);
        let mut m = Mesh::cuboid([-w / 2.0, -s / 2.0, -t / 2.0], [w / 2.0, s / 2.0, t / 2.0]).welded(1e-9);
        if self.coiled {
            for (i, d) in [w, s, t].iter().enumerate() {
                m = subdivide_edge_ring(&m, (d / self.dot_distance).ceil() as usize, i);
            }
            let ru = r.uniform(0.06, 0.08);
            let sf = r.uniform(0.5, 1.0);
            let out = if std::env::var("COIL_F64").is_err() {
                super::coilx::make_coiled_f32(&m, self.dot_distance, self.dot_depth, self.dot_size, ru, sf, None)
            } else {
                None
            };
            (out.unwrap_or_else(|| make_coiled(&m, self.dot_distance, self.dot_depth, self.dot_size, ru, sf)), None)
        } else {
            let wd = 0.05;
            for (i, d) in [w, s, t].iter().enumerate() {
                m = subdivide_edge_ring(&m, (d / wd).ceil() as usize, i);
            }
            let m = bevel_angle_ex(&m, wd / 3.0, 2, 30f64.to_radians(), false, None).unwrap_or(m);
            let pressure = r.uniform(0.1, 0.2);
            if !sim_bedding() {
                // the cover filled out: the top domes, the sides belly, the bottom stays (no simulation)
                let mut m = m;
                let (side, top) = self.puff();
                for q in m.pos.iter_mut() {
                    let f = ((q[2] + t / 2.0) / t).clamp(0.0, 1.0);
                    q[2] += top * f * f * self.dome(q[0], q[1]);
                    let b = side * (std::f64::consts::PI * f).sin();
                    q[0] += b * (2.0 * q[0] / w).powi(5);
                    q[1] += b * (2.0 * q[1] / s).powi(5);
                }
                return (m, None);
            }
            let mut p = ClothParams::default();
            p.gravity = 0.0;
            p.use_pressure = true;
            p.pressure = pressure;
            let pin: Vec<u32> = (0..m.pos.len() as u32).filter(|&i| m.pos[i as usize][2] < 0.1 - t / 2.0).collect();
            p.pin = m.pos.iter().map(|q| if q[2] < 0.1 - t / 2.0 { 1.0 } else { 0.0 }).collect();
            // pressure calibrated against the python mattress bake (seeds 4, 5): the generic scale over-inflates the sealed box
            let mut cal = Calib::default();
            cal.press_scale *= 0.2;
            let out = simulate_job(&m.pos, &m.faces, &p, &[], &cal);
            let pre = m.clone();
            let mut o = m;
            o.pos = out;
            (o, Some((pre, pressure, pin)))
        }
    }
}

pub struct Mattress {
    pub s: MattressSpec,
    pub fseed: u64,
}

pub fn make(fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Mattress { s: MattressSpec::new(fseed), fseed }))
}

impl Factory for Mattress {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let s = &self.s;
        let mut a = Asset::default();
        a.cuboid([-s.width / 2.0, -s.size / 2.0, -s.thickness / 2.0], [s.width / 2.0, s.size / 2.0, s.thickness / 2.0], &super::mat("ph", [0.5; 3], 0.5));
        Some(a)
    }

    /// collision: measured on its build (the coils puff the block out by up to 9 cm, not inward)
    fn volumes(&self, _inst: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = self.s.bounds();
        let g = 0.003;
        Some(vec![crate::piece::Volume::hard([lo[0] - g, lo[1] - g, lo[2] - g], [hi[0] + g, hi[1] + g, hi[2] + g]), crate::piece::Volume::support([lo[0] + 0.05, lo[1] + 0.05, hi[2] - 0.001], [hi[0] - 0.05, hi[1] - 0.05, hi[2]])])
    }
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fseed, inst);
        let m = self.s.build(&mut r);
        let mt = spec_material("mattress", json!({"cls": self.s.mat_cls, "kwargs": {}, "seed": self.fseed as i64 + 3}), [0.92, 0.92, 0.9]);
        let b = m.bounds();
        let mut a = Asset::default();
        a.parts.push(Part { mesh: m, mat: mt, col: Some((b.0, b.1)) });
        a
    }
}
