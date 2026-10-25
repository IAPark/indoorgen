//! clothes/towel.py TowelFactory: folded / rolled towel, exact parameters and surface-exact geometry.
use super::bevelfull::bevel_angle;
use super::bpyops::solidify;
use super::subsurf::subsurf;
use super::*;
use crate::materials::spec_material;
use crate::mesh::*;
use crate::rng::Rng;
use serde_json::json;
use crate::dhash::HashMap;

pub fn handles(c: &str) -> bool {
    c == "TowelFactory"
}

#[derive(Clone, Debug)]
pub struct TowelSpec {
    pub fseed: u64,
    pub width: f64,
    pub length: f64,
    pub thickness: f64,
    pub roll: bool,
    pub folds: i64,
    pub extra_thickness: f64,
    pub roll_total: f64,
    pub mat_index: usize,
}

const MAT_NAMES: [&str; 7] = ["fabric.CoarseKnitFabric", "fabric.FineKnitFabric", "fabric.Leather", "fabric.SofaFabric", "art.ArtRug", "fabric.Rug", "fabric.Rug"];

impl TowelSpec {
    /// from the constructor draws (width, length, thickness, fold_type, folds, extra_thickness, material index)
    pub fn from_values(fseed: u64, v: &[f64]) -> TowelSpec {
        let (width, length, thickness, extra) = (v[0], v[1], v[2], v[5]);
        let c = length / (thickness + extra) * (4.0 * std::f64::consts::PI);
        // scipy fsolve(f, 0): root of t sqrt(1+t^2) + asinh(t) = c
        let f = |t: f64| t * (1.0 + t * t).sqrt() + (t + (1.0 + t * t).sqrt()).ln() - c;
        let mut t = c.sqrt().max(1.0);
        for _ in 0..200 {
            let ft = f(t);
            let d = 2.0 * (1.0 + t * t).sqrt();
            let nt = t - ft / d;
            if (nt - t).abs() < 1e-14 {
                t = nt;
                break;
            }
            t = nt;
        }
        TowelSpec { fseed, width, length, thickness, roll: v[3] as usize == 1, folds: v[4] as i64, extra_thickness: extra, roll_total: t, mat_index: v[6] as usize }
    }

    /// TowelFactory(fseed) constructed inside an instance stream (BedFactory.towel_factory)
    pub fn from_stream(fseed: u64, r: &mut Rng) -> TowelSpec {
        let w = r.log_uniform(0.3, 0.6);
        let lu = r.log_uniform(1.0, 1.5);
        let t = r.log_uniform(0.003, 0.01);
        let ft = r.choice_p(&[2.0 / 3.0, 1.0 / 3.0]);
        let folds = r.randint(2, 4);
        let eu = r.uniform(0.2, 0.3);
        TowelSpec::from_values(fseed, &[w, w * lu, t, ft as f64, folds as f64, t * eu, 0.0])
    }
}

type Edge = (u32, u32);

fn edges_of(m: &Mesh) -> Vec<Edge> {
    let mut set: HashMap<Edge, ()> = HashMap::default();
    let mut v = vec![];
    for f in &m.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            if set.insert(k, ()).is_none() {
                v.push(k);
            }
        }
    }
    v
}

/// bmesh.ops.subdivide_edgering (LINEAR, no falloff) on the edges selected by `sel`: returns the new mesh and, per selected edge,
/// the ids of its cut vertices ordered from the lower to the higher vertex index
pub fn subdivide_edgering(m: &Mesh, sel: &dyn Fn(&Mesh, Edge) -> bool, cuts: usize) -> (Mesh, Vec<(Edge, Vec<u32>)>) {
    let edges = edges_of(m);
    let selected0: Vec<Edge> = edges.into_iter().filter(|&e| sel(m, e)).collect();
    // only edges of qualifying faces (quad / tri with >= 2 selected edges) are part of an edge ring
    let sel_set: crate::dhash::HashSet<Edge> = selected0.iter().cloned().collect();
    let mut ring: crate::dhash::HashSet<Edge> = crate::dhash::HashSet::default();
    for f in &m.faces {
        let k = f.len();
        if k > 4 {
            continue;
        }
        let es: Vec<Edge> = (0..k).map(|i| (f[i].min(f[(i + 1) % k]), f[i].max(f[(i + 1) % k]))).filter(|e| sel_set.contains(e)).collect();
        if es.len() >= 2 {
            for e in es {
                ring.insert(e);
            }
        }
    }
    let selected: Vec<Edge> = selected0.into_iter().filter(|e| ring.contains(e)).collect();
    let mut cut_ids: HashMap<Edge, Vec<u32>> = HashMap::default();
    let mut out = m.clone();
    for &(a, b) in &selected {
        let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
        let mut ids = vec![];
        for j in 0..cuts {
            let t = (j + 1) as f64 / (cuts + 1) as f64;
            let t32 = t as f32;
            let lerp = |x: f64, y: f64| -> f64 { ((1.0f32 - t32) * x as f32 + t32 * y as f32) as f64 };
            out.pos.push([lerp(pa[0], pb[0]), lerp(pa[1], pb[1]), lerp(pa[2], pb[2])]);
            ids.push((out.pos.len() - 1) as u32);
        }
        cut_ids.insert((a, b), ids);
    }
    let is_sel = |a: u32, b: u32| cut_ids.contains_key(&(a.min(b), a.max(b)));
    // points along directed edge u -> w (excluding the end points)
    let along = |u: u32, w: u32| -> Vec<u32> {
        let ids = &cut_ids[&(u.min(w), u.max(w))];
        if u < w {
            ids.clone()
        } else {
            ids.iter().rev().cloned().collect()
        }
    };
    let mut faces: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        let k = f.len();
        let sel_k: Vec<bool> = (0..k).map(|i| is_sel(f[i], f[(i + 1) % k])).collect();
        let nsel = sel_k.iter().filter(|&&x| x).count();
        if k == 4 && nsel >= 2 {
            // opposite selected edges i and i+2
            let pair = (0..2).find(|&i| sel_k[i] && sel_k[i + 2]);
            if let Some(i) = pair {
                let (p0, p1, q0, q1) = (f[i], f[(i + 1) % 4], f[(i + 3) % 4], f[(i + 2) % 4]);
                let mut pa = vec![p0];
                pa.extend(along(p0, p1));
                pa.push(p1);
                let mut qa = vec![q0];
                qa.extend(along(q0, q1));
                qa.push(q1);
                for j in 0..=cuts {
                    faces.push(vec![pa[j], pa[j + 1], qa[j + 1], qa[j]]);
                }
                continue;
            }
        }
        // other faces: insert the cut vertices of their selected edges
        let mut nf = vec![];
        for i in 0..k {
            nf.push(f[i]);
            let w = f[(i + 1) % k];
            if sel_k[i] {
                nf.extend(along(f[i], w));
            }
        }
        faces.push(nf);
    }
    out.faces = faces;
    let rep: Vec<(Edge, Vec<u32>)> = selected.into_iter().map(|e| (e, cut_ids[&e].clone())).collect();
    (out, rep)
}

fn dir_n(m: &Mesh, e: Edge) -> V3 {
    let (a, b) = (m.pos[e.0 as usize], m.pos[e.1 as usize]);
    let d = [(b[0] as f32 - a[0] as f32) as f64, (b[1] as f32 - a[1] as f32) as f64, (b[2] as f32 - a[2] as f32) as f64];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l == 0.0 {
        [0.0; 3]
    } else {
        [d[0] / l, d[1] / l, d[2] / l]
    }
}

/// decorate.subdivide_edge_ring(obj, cuts, axis): edges parallel to `axis`
pub fn subdivide_edge_ring(m: &Mesh, cuts: usize, axis: usize) -> Mesh {
    if std::env::var("SDR_OLD").is_ok() {
        let (o, _) = subdivide_edgering(m, &|mm, e| dir_n(mm, e)[axis].abs() > 1.0 - 1e-3, cuts);
        return o;
    }
    subdivide_edge_ring_bm(m, cuts, axis)
}

/// decorate.subdivide_edge_ring on the exact bmesh operator port (float32 vertex positions, Blender's element order)
pub fn subdivide_edge_ring_bm(m: &Mesh, cuts: usize, axis: usize) -> Mesh {
    use crate::bm::BMesh;
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    // mesh edges in first-use order of the face corners
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut ce: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([a, b]);
                (edges.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    // selected = |normalize(co[v2] - co[v1]) . axis| > 1 - 1e-3 (float64 on the float32 coordinates)
    let mut sel = vec![];
    for (i, e) in edges.iter().enumerate() {
        let (a, b) = (pos[e[0] as usize], pos[e[1] as usize]);
        let d = [b[0] as f64 - a[0] as f64, b[1] as f64 - a[1] as f64, b[2] as f64 - a[2] as f64];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > 0.0 && (d[axis] / l).abs() > 1.0 - 1e-3 {
            sel.push(i);
        }
    }
    crate::bm::edgering::subdivide_edgering(&mut bm, &sel, cuts);
    let (p, _e, f) = bm.to_mesh();
    let mut o = Mesh::new();
    o.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    o.faces = f;
    o
}

fn bounds_of(m: &Mesh) -> (V3, V3) {
    m.bounds()
}

impl TowelSpec {
    fn fold(&self, m: &mut Mesh, r: &mut Rng) {
        let t = self.thickness;
        let (lo, hi) = bounds_of(m);
        let rz = if hi[0] - lo[0] > hi[1] - lo[1] {
            std::f64::consts::PI * if r.uniform(0.0, 1.0) < 0.5 { 1.0 } else { 0.0 }
        } else {
            std::f64::consts::PI * if r.uniform(0.0, 1.0) < 0.5 { 1.0 } else { 0.0 } + std::f64::consts::FRAC_PI_2
        };
        m.transform(&Mat4::rot_z(rz));
        let (lo, hi) = bounds_of(m);
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let shift = [-c[0] + r.uniform(-t, t), -c[1], 0.0];
        m.transform(&Mat4::translation(shift));
        let n = m.pos.len();
        let (mut sub, rep) = subdivide_edgering(m, &|mm, e| dir_n(mm, e)[0].abs() > 1.0 - 1e-3, 15);
        let _ = n;
        for (_, ids) in &rep {
            // cut vertices of one edge get x = linspace(-t, t, 15) in the order they run along x
            let mut order: Vec<(f64, u32)> = ids.iter().map(|&i| (sub.pos[i as usize][0], i)).collect();
            order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for (k, (_, i)) in order.iter().enumerate() {
                sub.pos[*i as usize][0] = -t + 2.0 * t * k as f64 / 14.0;
            }
        }
        let max_z = sub.pos.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max) + self.extra_thickness;
        for p in sub.pos.iter_mut() {
            let (x, z) = (p[0], p[2]);
            let theta = x / t * std::f64::consts::FRAC_PI_2;
            let (nx, nz) = if x < -t {
                (x, z)
            } else if x > t {
                (-x, max_z * 2.0 - z)
            } else {
                (-t + (max_z - z) * theta.cos(), max_z + (max_z - z) * theta.sin())
            };
            p[0] = nx;
            p[2] = nz;
        }
        *m = sub;
        if r.uniform(0.0, 1.0) < 0.5 {
            // mirror(obj): scale x = -1 and flip the normals
            for p in m.pos.iter_mut() {
                p[0] = -p[0];
            }
            // scaling by -1 already reverses the winding; flip_normals reverses it back
        }
    }

    /// create_asset() geometry (before materials)
    pub fn build(&self, inst: u64) -> Mesh {
        let mut r = super::inst_rng(self.fseed, inst);
        let t = self.thickness;
        let (hx, hy) = if self.roll { (self.length / 2.0, self.width / 2.0) } else { (self.width / 2.0, self.length / 2.0) };
        let mut m = Mesh::new();
        m.pos = vec![[-hx, -hy, 0.0], [hx, -hy, 0.0], [-hx, hy, 0.0], [hx, hy, 0.0]];
        m.faces = vec![vec![0, 1, 3, 2]];
        let mut roll_i: Vec<usize> = vec![];
        if self.roll {
            m = subdivide_edge_ring(&m, 256, 0);
            let tt: Vec<f64> = (0..=256).map(|i| self.roll_total * i as f64 / 256.0).collect();
            for p in m.pos.iter_mut() {
                let i = round_half_even((p[0] / self.length + 0.5) * 256.0) as usize;
                roll_i.push(i);
                let tv = tt[i];
                let len = (tv * (1.0 + tv * tv).sqrt() + (tv + (1.0 + tv * tv).sqrt()).ln()) * (t + self.extra_thickness) / (4.0 * std::f64::consts::PI);
                p[0] = len;
            }
        }
        if std::env::var("TW_STAGE").ok().as_deref() == Some("preroll") {
            return m;
        }
        let _strength = r.uniform(0.2, 0.4); // wrap_sides(strength=...)
        m = solidify(&m, t, 1.0);
        if self.roll {
            let tt: Vec<f64> = (0..=256).map(|i| self.roll_total * i as f64 / 256.0).collect();
            for (vi, p) in m.pos.iter_mut().enumerate() {
                let i = roll_i[vi % roll_i.len()];
                let tv = tt[i];
                let rad = (t + self.extra_thickness) / (2.0 * std::f64::consts::PI) * tv + if p[2] > t / 2.0 { -t / 2.0 } else { t / 2.0 };
                *p = [rad * tv.cos(), p[1], rad * tv.sin()];
            }
            if std::env::var("TW_STAGE").ok().as_deref() == Some("roll") {
                return m;
            }
            m = subdivide_edge_ring(&m, 16, 1);
        } else {
            for fi in 0..self.folds {
                self.fold(&mut m, &mut r);
                if std::env::var("TW_STAGE").ok().and_then(|v| v.parse::<i64>().ok()) == Some(fi + 1) {
                    return m;
                }
            }
            m = subdivide_edge_ring(&m, 16, 0);
            m = subdivide_edge_ring(&m, 16, 1);
        }
        let bw = t * r.uniform(0.4, 0.8);
        if std::env::var("TW_STAGE").ok().as_deref() == Some("prebevel") {
            eprintln!("BW {}", bw);
            return m;
        }
        m = match super::bevmod::bevel_modifier_ex(&m, bw, 2, Some(0.5235987901687622f32), true, 0.5, None) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("towel bevel failed: {e}");
                m
            }
        };
        if std::env::var("TW_STAGE").ok().as_deref() == Some("bevel") {
            return m;
        }
        // geo_extension(noise_strength=uniform(.05,.1)) draws
        let ns0 = r.uniform(0.05, 0.1);
        let strength = r.uniform(ns0 / 2.0, ns0);
        let scale = r.uniform(2.0 * 0.7, 2.0 * 1.4);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        m = super::gnhelpers::geo_extension(&m, off, scale, strength);
        subsurf(&m, 1, false)
    }
}

fn round_half_even(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

pub struct Towel {
    pub spec: TowelSpec,
}

pub fn make(_c: &str, _rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    let v = unseeded()?;
    if v.len() < 7 {
        return None;
    }
    Some(Box::new(Towel { spec: TowelSpec::from_values(fseed, &v) }))
}

impl Factory for Towel {
    fn asset(&self, i: u64) -> Asset {
        let m = self.spec.build(i);
        let mt = spec_material("towel", json!({"cls": MAT_NAMES[self.spec.mat_index.min(6)], "kwargs": {}, "seed": self.spec.fseed}), [0.8, 0.8, 0.85]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.parts.push(Part { mesh: m, mat: mt, col: Some((b.0, b.1)) });
        a
    }
}
