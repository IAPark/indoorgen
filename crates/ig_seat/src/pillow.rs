//! seating/pillow.py PillowFactory: two-layer sheet (grid / disc / annulus) inflated by the native cloth solver (approximate by
//! design), optional rim extrusion, centred, subsurf 2.
use super::bpyops::fill_grid_loop;
use super::bpyx::extrude_region;
use super::cloth::{simulate, Calib, ClothParams};
use super::clothes::{apply_srt_f32, grid, solidify_simple, vertex_normals};
use super::ctor_draws::pillow_ctor;
use super::subsurf::subsurf;
use super::{inst_rng, Asset, Factory, Part};
use crate::materials::spec_material;
use crate::mesh::*;
use crate::rng::Rng;
use serde_json::json;

#[derive(Clone, Debug)]
pub struct PillowSpec {
    /// 0 square 1 rectangle 2 circle 3 torus
    pub shape: usize,
    pub width: f64,
    pub size: f64,
    pub bevel: f64,
    pub thickness: f64,
    pub extrude: f64,
    pub has_seam: bool,
    pub seam_radius: f64,
}

impl PillowSpec {
    pub fn from_ctor(c: &[f64]) -> PillowSpec {
        PillowSpec { shape: c[0] as usize, width: c[1], size: c[2], bevel: c[3], thickness: c[4], extrude: c[5], has_seam: c[6] > 0.5, seam_radius: c[7] }
    }

    fn circle(radius: f64) -> Vec<[f64; 3]> {
        let phid = 2.0f32 * std::f32::consts::PI / 128.0;
        (0..128).map(|a| {
            let phi = phid * a as f32;
            [(-(radius as f32) * phi.sin()) as f64, ((radius as f32) * phi.cos()) as f64, 0.0]
        }).collect()
    }

    /// create_asset on the spawn stream `r`
    pub fn build(&self, r: &mut Rng) -> Mesh {
        let t = self.thickness;
        // base sheet
        let (mut base, edges): (Mesh, Vec<(u32, u32)>) = match self.shape {
            2 | 3 => {
                let outer = Self::circle(1.0);
                let mut m = Mesh::new();
                m.pos.extend(outer.iter().cloned());
                let lp: Vec<[f64; 3]> = outer.clone();
                if self.shape == 2 {
                    let g = fill_grid_loop(&lp);
                    let mut mm = g;
                    // boundary edges of the fill, in face order
                    let e = boundary_order(&mm);
                    let _ = &mut mm;
                    (mm, e)
                } else {
                    let ri = r.uniform(0.2, 0.4);
                    let inner = Self::circle(ri);
                    m.pos.extend(inner.iter().cloned());
                    super::bridge::bridge_pair_opts(&mut m, (0..128).collect(), (128..256).collect(), 12, 0.0, &mut None, super::bridge::Interp::Linear, 1.0, "SMOOTH"); // python: interpolation="LINEAR"
                    let e = boundary_order(&m);
                    (m, e)
                }
            }
            _ => grid(32, 32),
        };
        apply_srt_f32(&mut base, [(self.width / 2.0) as f32, (self.size / 2.0) as f32, 1.0], 0.0, [0.0; 3]);
        let mut m = solidify_simple(&base, &edges, t, 0.0);
        let nf_base = base.faces.len();
        let rim: Vec<bool> = (0..m.faces.len()).map(|fi| {
            let f = &m.faces[fi];
            let n = super::coil::face_normal(&m, f);
            fi >= 2 * nf_base || n[2].abs() < 0.1
        }).collect();
        // faces that are neither rim by construction nor flat keep the python rule: |nz| < 0.1
        let rim: Vec<bool> = (0..m.faces.len()).map(|fi| {
            let n = super::coil::face_normal(&m, &m.faces[fi]);
            let _ = &rim;
            n[2].abs() < 0.1
        }).collect();
        let mut pin = vec![0.0; m.pos.len()];
        if self.has_seam {
            let sel: Vec<bool> = m.faces.iter().map(|f| {
                let mut c = [0.0; 3];
                for &v in f {
                    for k in 0..3 {
                        c[k] += m.pos[v as usize][k];
                    }
                }
                let n = f.len() as f64;
                let c = [c[0] / n, c[1] / n, c[2] / n];
                c[0] * c[0] + c[1] * c[1] < self.seam_radius * self.seam_radius && c[2] > 0.0
            }).collect();
            let mut ecount: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
            for (fi, f) in m.faces.iter().enumerate() {
                if !sel[fi] {
                    continue;
                }
                for k in 0..f.len() {
                    let (a, b) = (f[k], f[(k + 1) % f.len()]);
                    *ecount.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
            for (&(a, b), &c) in ecount.iter() {
                if c == 1 {
                    pin[a as usize] = 1.0;
                    pin[b as usize] = 1.0;
                }
            }
        }
        let pressure = if self.shape == 3 { r.uniform(8.0, 12.0) } else { r.uniform(1.0, 2.0) };
        let mut p = ClothParams::default();
        p.tension = r.uniform(0.0, 5.0);
        p.gravity = 0.0;
        p.use_pressure = true;
        p.pressure = pressure;
        if self.has_seam {
            p.pin = pin;
        }
        let out = simulate(&m.pos, &m.faces, &p, &[], &Calib::default());
        m.pos = out;
        if self.extrude > 0.0 {
            let nrm = region_normals(&m, &rim);
            let map = extrude_region(&mut m, &rim, [0.0; 3]);
            let mut keys: Vec<u32> = map.keys().copied().collect();
            keys.sort();
            for k in keys {
                let nv = map[&k] as usize;
                for c in 0..3 {
                    m.pos[nv][c] += nrm[k as usize][c] * self.extrude;
                }
            }
        }
        let (lo, hi) = m.bounds();
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        for q in m.pos.iter_mut() {
            *q = [q[0] - c[0], q[1] - c[1], q[2] - c[2]];
        }
        subsurf(&m, 2, false)
    }
}

/// unit vertex normals from the selected faces only
fn region_normals(m: &Mesh, sel: &[bool]) -> Vec<[f64; 3]> {
    let mut sub = Mesh::new();
    sub.pos = m.pos.clone();
    for (i, f) in m.faces.iter().enumerate() {
        if sel[i] {
            sub.faces.push(f.clone());
        }
    }
    vertex_normals(&sub)
}

/// boundary edges (used by exactly one face) in face order, as solidify wants them
fn boundary_order(m: &Mesh) -> Vec<(u32, u32)> {
    let mut cnt: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    for f in &m.faces {
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut out = vec![];
    for f in &m.faces {
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            if cnt[&(a.min(b), a.max(b))] == 1 {
                out.push((a, b));
            }
        }
    }
    out
}

pub struct Pillow {
    pub s: PillowSpec,
    pub fseed: u64,
}

pub fn make(fseed: u64) -> Option<Box<dyn Factory>> {
    let v = super::unseeded()?;
    if v.len() < 8 {
        return None;
    }
    Some(Box::new(Pillow { s: PillowSpec::from_ctor(&v), fseed }))
}

#[allow(dead_code)]
fn _unused(r: &mut Rng) -> [f64; 8] {
    pillow_ctor(r)
}

impl Factory for Pillow {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let s = &self.s;
        let mut a = Asset::default();
        a.cuboid([-s.width / 2.0, -s.size / 2.0, -0.05], [s.width / 2.0, s.size / 2.0, 0.05], &super::mat("ph", [0.5; 3], 0.5));
        Some(a)
    }

    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fseed, inst);
        let m = self.s.build(&mut r);
        let mt = spec_material("pillow", json!({"cls": "fabric.FineKnitFabric", "kwargs": {}, "seed": self.fseed as i64 + 10}), [0.9, 0.9, 0.92]);
        let b = m.bounds();
        let mut a = Asset::default();
        a.parts.push(Part { mesh: m, mat: mt, col: Some((b.0, b.1)) });
        a
    }
}
