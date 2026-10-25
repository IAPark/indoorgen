//! mushroom/*.py (MushroomFactory: cap + stem + gills, clusters of 1-5). Constructor and instance draws follow the python order; the cap / stem are spun
//! from the same Bezier profiles (blops::spin_anchors) and decorated with the same modifiers, but the voxel remeshes, the stem band / web geometry nodes and the
//! morel pattern are not reproduced (approximation, not surface-equal).
use super::bpymod::{simple_deform, Deform};
use super::legtex::{displace, Tex, TexKind};
use super::*;
use crate::mesh::{Mat4, Mesh};
use crate::rng::Rng;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "MushroomFactory"
}

type V = [f64; 3];

#[derive(Clone, Debug)]
struct CapCfg {
    x: Vec<f64>,
    z: Vec<f64>,
    vec: Vec<usize>,
    has_gill: bool,
}

#[derive(Clone, Debug)]
struct GillCfg {
    x: Vec<f64>,
    z: Vec<f64>,
    vec: Vec<usize>,
}

pub struct MushroomFactory {
    fs: u64,
    directional: bool,
    lowered: bool,
    tolerant: f64,
    cap: CapCfg,
    radius: f64,
    inner_radius: f64,
    gill: Option<GillCfg>,
    web: u8, // 0 hollow, 1 solid, 2 none
    has_band: bool,
}

fn cap_configs(r: &mut Rng) -> Vec<CapCfg> {
    let mut v = vec![];
    // campanulate
    let x = r.uniform(0.12, 0.15);
    let z = vec![0.0, 0.0, r.uniform(0.03, 0.05), r.uniform(0.1, 0.12), r.uniform(0.16, 0.2), 0.2];
    v.push(CapCfg { x: vec![0.0, x, x, 0.08, 0.04, 0.0], z, vec: vec![], has_gill: true });
    // conical
    let z = r.uniform(0.2, 0.3);
    let x1 = r.uniform(0.12, 0.15);
    v.push(CapCfg { x: vec![0.0, x1, 0.01, 0.0], z: vec![0.0, 0.0, z, z], vec: vec![1], has_gill: true });
    // convex
    let z = r.uniform(0.14, 0.16);
    let z2 = r.uniform(0.04, 0.06);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.01, 0.0], z: vec![0.0, 0.0, z2, z, z], vec: vec![1], has_gill: true });
    // depressed
    let z = r.uniform(0.03, 0.05);
    let z2 = r.uniform(0.06, 0.08);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.0], z: vec![0.0, 0.0, z2, z], vec: vec![1], has_gill: true });
    // flat
    let z = r.uniform(0.05, 0.07);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.0], z: vec![0.0, 0.0, z, z], vec: vec![1], has_gill: true });
    // infundiuliform
    let z = r.uniform(0.08, 0.12);
    let x = r.uniform(0.12, 0.15);
    let zz = z + r.uniform(0.005, 0.01);
    v.push(CapCfg { x: vec![0.0, 0.03, x, x - 0.01, 0.0], z: vec![0.0, 0.0, z, zz, 0.02], vec: vec![], has_gill: false });
    // ovate
    let z = r.uniform(0.2, 0.3);
    let x1 = r.uniform(0.12, 0.15);
    v.push(CapCfg { x: vec![0.0, x1, 0.08, 0.01, 0.0], z: vec![0.0, 0.0, 0.8 * z, z, z], vec: vec![1], has_gill: true });
    // umbillicate
    let z = r.uniform(0.03, 0.05);
    let z2 = r.uniform(0.06, 0.08);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.02, 0.0], z: vec![0.0, 0.04, z2, z + 0.02, z], vec: vec![], has_gill: false });
    // umbonate
    let z = r.uniform(0.05, 0.07);
    let z_ = z + r.uniform(0.02, 0.04);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.06, 0.02, 0.0], z: vec![0.0, 0.0, z - 0.01, z, z_, z_], vec: vec![1], has_gill: true });
    v
}

impl MushroomFactory {
    pub fn new(fs: u64) -> MushroomFactory {
        // MushroomFactory.__init__
        let mut r = Rng::seeded(fs);
        let directional = r.randint(0, 2) == 0;
        let lowered = r.uniform(0.0, 1.0) < 0.5;
        // MushroomGrowthFactory / cap / stem each restart at the factory seed
        let mut rc = Rng::seeded(fs);
        // growth: base hue (the cap factory receives it but only uses it for the shader)
        let _ = (rc.uniform(0.0, 1.0),);
        let mut rc = Rng::seeded(fs);
        let xs = rc.uniform(0.7, 1.4);
        let zs = rc.uniform(0.7, 1.4);
        let cfgs = cap_configs(&mut rc);
        let idx = rc.choice_p(&[2.0, 2.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 1.0]);
        let mut cap = cfgs[idx].clone();
        cap.x = cap.x.iter().map(|v| v * xs).collect();
        cap.z = cap.z.iter().map(|v| v * zs).collect();
        let radius = cap.x.iter().cloned().fold(f64::MIN, f64::max);
        let inner_radius = rc.log_uniform(0.2, 0.35) * radius;
        let adn = GillCfg {
            x: vec![radius, (radius + inner_radius) / 2.0, inner_radius, inner_radius, radius],
            z: vec![0.0, -rc.uniform(0.05, 0.08), -rc.uniform(0.0, 0.02), 0.0, 0.0],
            vec: vec![3],
        };
        let dec = GillCfg {
            x: vec![radius, (radius + inner_radius) / 2.0, inner_radius, 0.0, radius],
            z: vec![0.0, -rc.uniform(0.05, 0.08), -rc.uniform(0.08, 0.1), 0.0, 0.0],
            vec: vec![2],
        };
        let gi = rc.choice_p(&[1.0, 1.0, 1.0]);
        let gill = if !cap.has_gill { None } else { [Some(adn), Some(dec), None][gi].clone() };
        let mut rs = Rng::seeded(fs);
        let web = rs.choice_p(&[1.0, 1.0, 2.0]) as u8;
        let has_band = rs.uniform(0.0, 1.0) < 0.75;
        let tolerant = r.uniform(0.0, 0.2);
        MushroomFactory { fs, directional, lowered, tolerant, cap, radius, inner_radius, gill, web, has_band }
    }

    fn spin(&self, x: &[f64], z: &[f64], vec: &[usize]) -> Mesh {
        let anchors: Vec<[f64; 3]> = (0..x.len()).map(|i| [x[i], 0.0, z[i]]).collect();
        super::blops::spin_anchors(&anchors, vec)
    }

    fn deform(m: &mut Mesh, method: Deform, angle: f64, axis: usize) {
        let mut v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        simple_deform(&mut v, method, angle, axis);
        for (p, q) in m.pos.iter_mut().zip(&v) {
            *p = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
    }

    fn cap_mesh(&self, i: u64) -> Mesh {
        let mut r = inst_rng(self.fs, i);
        let mut obj = self.spin(&self.cap.x, &self.cap.z, &self.cap.vec);
        if let Some(g) = &self.gill {
            let steps = r.randint(8, 20) as usize;
            // closed outline in the xz plane -> flat sheet, repeated around z
            let pts: Vec<[f64; 3]> = (0..g.x.len()).map(|k| [g.x[k], 0.0, g.z[k]]).collect();
            let c = super::blops::bezier_curve_points(&pts, &g.vec);
            let n = c.len();
            let mut sheet = Mesh::new();
            let ring: Vec<[f64; 3]> = c.iter().map(|p| [p[0] as f64, 0.0, p[2] as f64]).collect();
            // fan from the centroid (the outline is a thin convex-ish sliver)
            let mut cen = [0.0; 3];
            for p in &ring {
                for k in 0..3 {
                    cen[k] += p[k] / n as f64;
                }
            }
            for k in 0..n {
                sheet.add_face(&[cen, ring[k], ring[(k + 1) % n]]);
            }
            let mut gill = Mesh::new();
            for s in 0..steps {
                let mut sh = sheet.clone();
                sh.transform(&Mat4::rot_z(2.0 * PI * s as f64 / steps as f64));
                gill.append(&sh);
            }
            obj.append(&gill);
        }
        let marble = r.choice_idx(2) == 1;
        let scale = r.log_uniform(0.01, 0.05);
        let kind = if marble { TexKind::Marble(0, 0) } else { TexKind::Stucci(0) };
        displace(&mut obj, &Tex::new(kind, scale), 0.008, 0.0);
        // geo_extension(0.1)
        let ns = r.uniform(0.05, 0.1);
        let sc = r.uniform(1.4, 2.8);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        obj = super::gnhelpers::geo_extension(&obj, off, sc, ns);
        let a = r.uniform(-PI / 4.0, PI / 4.0);
        Self::deform(&mut obj, Deform::Twist, a, 0);
        let rr = [r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25)];
        for p in obj.pos.iter_mut() {
            p[0] += if p[0] > 0.0 { rr[0] } else { rr[1] } * p[0];
            p[1] += if p[1] > 0.0 { rr[2] } else { rr[3] } * p[1];
        }
        obj
    }

    fn stem_mesh(&self, i: u64) -> Mesh {
        let mut r = inst_rng(self.fs, i);
        let ir = self.inner_radius;
        let length = r.log_uniform(0.4, 0.8);
        let x = [0.0, ir, r.log_uniform(1.0, 2.0) * ir, ir * r.uniform(1.0, 1.2), 0.0];
        let z = [0.0, 0.0, -length * r.uniform(0.3, 0.7), -length, -length];
        let mut obj = self.spin(&x, &z, &[1, 4]);
        if self.has_band {
            let _ = (r.log_uniform(5.0, 10.0), r.uniform(5.0, 10.0), r.uniform(0.3, 0.7), r.uniform(0.008, 0.01));
        }
        if self.web != 2 {
            // skirt (solid approximation of the hollow / solid web)
            let outer = ir * if self.web == 0 { r.uniform(2.0, 3.5) } else { r.uniform(1.5, 3.5) };
            let zt = r.uniform(0.0, 0.05);
            let len = if self.web == 0 { r.log_uniform(0.2, 0.4) } else { r.uniform(0.15, 0.2) };
            let xa = [ir, (outer + ir) / 2.0, outer];
            let za = [-zt, -zt - r.uniform(0.3, 0.4) * len, -zt - len];
            let web = self.spin(&xa, &za, &[]);
            obj.append(&web);
        }
        let marble_scale = r.uniform(0.005, 0.01);
        displace(&mut obj, &Tex::new(TexKind::Stucci(0), marble_scale), 0.008, 0.0);
        let a = -r.uniform(0.0, PI / 2.0);
        Self::deform(&mut obj, Deform::Bend, a, 1);
        obj
    }

    /// MushroomGrowthFactory.create_asset: cap + stem joined, lowered to z = 0 at the lowest vertex
    fn growth(&self, i: u64) -> Mesh {
        let mut cap = self.cap_mesh(i);
        let stem = self.stem_mesh(i);
        cap.append(&stem);
        let mut lo = 0;
        for (k, p) in cap.pos.iter().enumerate() {
            if p[2] < cap.pos[lo][2] {
                lo = k;
            }
        }
        let l = [-cap.pos[lo][0], -cap.pos[lo][1], -cap.pos[lo][2]];
        for p in cap.pos.iter_mut() {
            for k in 0..3 {
                p[k] += l[k];
            }
        }
        cap
    }

    pub fn mesh(&self, inst: u64) -> Mesh {
        let mut r = inst_rng(self.fs, inst);
        let n = r.randint(1, 6) as usize;
        let mut mushrooms: Vec<Mesh> = vec![];
        let mut keypoints: Vec<Vec<V>> = vec![];
        for j in 0..n {
            let m = self.growth(j as u64 + inst * 10);
            let mut k: Vec<V> = m.pos.iter().step_by(40).copied().filter(|p| p[2] > self.tolerant).collect();
            if k.is_empty() {
                k = m.pos.iter().step_by(40).copied().collect();
            }
            if k.is_empty() {
                k = vec![[0.0; 3]];
            }
            keypoints.push(k);
            mushrooms.push(m);
        }
        let (rotations, start_locs, directions): (Vec<V>, Vec<V>, Vec<V>) = if self.directional {
            let rot_y: Vec<f64> = if self.lowered { (0..n).map(|_| r.uniform(0.0, PI / 6.0)).collect() } else { vec![0.0; n] };
            let rot_z: Vec<f64> = (0..n).map(|_| -PI / 2.0 + r.uniform(-PI / 8.0, PI / 8.0)).collect();
            (
                (0..n).map(|i| [0.0, rot_y[i], rot_z[i]]).collect(),
                (0..n).map(|i| [if n > 1 { self.radius * n as f64 * 0.4 * i as f64 / (n - 1) as f64 } else { 0.0 }, 0.0, 0.0]).collect(),
                vec![[0.0, 1.0, 0.0]; n],
            )
        } else {
            let angles = super::monocot::polygon_angles(&mut r, n, PI / 10.0, PI * 2.0);
            let rot_y: Vec<f64> = if self.lowered { (0..n).map(|_| r.uniform(0.0, PI / 6.0)).collect() } else { vec![0.0; n] };
            let rot_z: Vec<f64> = (0..n).map(|i| angles[i] + r.uniform(-PI / 8.0, PI / 8.0)).collect();
            (
                (0..n).map(|i| [0.0, rot_y[i], rot_z[i]]).collect(),
                vec![[0.0; 3]; n],
                (0..n).map(|i| [angles[i].cos(), angles[i].sin(), 0.0]).collect(),
            )
        };
        // find_closest
        let sc: Vec<f64> = (0..n).map(|_| r.uniform(0.3, 1.2)).collect();
        let mats: Vec<Mat4> = (0..n).map(|i| Mat4::euler_xyz(rotations[i]).mul(&Mat4::scale([sc[i]; 3]))).collect();
        let mut verts: Vec<Vec<V>> = (0..n).map(|i| keypoints[i].iter().map(|p| mat_apply(&mats[i], *p)).collect()).collect();
        let mut locations: Vec<V> = vec![[0.0; 3]];
        for i in 1..n {
            let basis: Vec<V> = verts[..i].iter().flatten().copied().collect();
            let mut chosen = None;
            for k in 0..20 {
                let d = 4.0 * k as f64 / 19.0 * self.radius;
                let off = [start_locs[i][0] + directions[i][0] * d, start_locs[i][1] + directions[i][1] * d, start_locs[i][2] + directions[i][2] * d];
                let mind = verts[i]
                    .iter()
                    .map(|v| {
                        let q = [v[0] + off[0], v[1] + off[1], v[2] + off[2]];
                        basis.iter().map(|b| ((b[0] - q[0]).powi(2) + (b[1] - q[1]).powi(2) + (b[2] - q[2]).powi(2)).sqrt()).fold(f64::MAX, f64::min)
                    })
                    .fold(f64::MAX, f64::min);
                if mind > 0.008 {
                    chosen = Some(off);
                    break;
                }
            }
            let off = chosen.unwrap_or([start_locs[i][0] + directions[i][0] * 4.0 * self.radius, start_locs[i][1] + directions[i][1] * 4.0 * self.radius, start_locs[i][2] + directions[i][2] * 4.0 * self.radius]);
            for v in verts[i].iter_mut() {
                for k in 0..3 {
                    v[k] += off[k];
                }
            }
            locations.push(off);
        }
        let mut out = Mesh::new();
        for i in 0..n {
            let mut m = mushrooms[i].clone();
            m.transform(&Mat4::translation(locations[i]).mul(&mats[i]));
            out.append(&m);
        }
        let a = r.uniform(-PI / 8.0, PI / 8.0);
        let axis = r.choice_idx(2);
        Self::deform(&mut out, Deform::Bend, a, axis);
        out
    }
}

fn mat_apply(m: &Mat4, p: V) -> V {
    let a = &m.0;
    [a[0][0] * p[0] + a[0][1] * p[1] + a[0][2] * p[2] + a[0][3], a[1][0] * p[0] + a[1][1] * p[1] + a[1][2] * p[2] + a[1][3], a[2][0] * p[0] + a[2][1] * p[1] + a[2][2] * p[2] + a[2][3]]
}

impl Factory for MushroomFactory {
    fn asset(&self, inst: u64) -> Asset {
        let mesh = self.mesh(inst);
        let b = mesh.bounds();
        let mut a = Asset::default();
        a.mesh(mesh, &mat("mushroom", [0.5, 0.35, 0.25], 0.6), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(MushroomFactory::new(fs)))
}
