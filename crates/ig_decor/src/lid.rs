//! tableware/lid.py LidFactory: spin -> SOLIDIFY(offset 0) -> BEVEL, glass rim torus, knob (cylinders with BEVEL); the bar handle is not ported
use super::bevel::{bevel, Offset};
use super::blops::*;
use super::bottle::unit_cylinder;
use super::matdraw;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "LidFactory"
}

pub struct Lid {
    x_length: f64,
    z_height: f64,
    thickness: f64,
    is_glass: bool,
    rim_height: f64,
    knob: bool,
    handle_width: f64,
    sub: i64,
    handle_height: f64,
    handle_radius: f64,
    mats: [&'static str; 3],
    fs: u64,
}

impl Lid {
    fn new(fs: u64) -> Lid {
        let mut r = Rng::seeded(fs);
        let x_length = r.uniform(0.08, 0.15);
        let z_height = x_length * r.uniform(0.0, 0.5);
        let thickness = r.uniform(0.003, 0.005);
        let is_glass = r.uniform(0.0, 1.0) < 0.5;
        let rim_height = r.uniform(1.0, 2.0) * thickness;
        let knob = r.randint(0, 2) == 1;
        let handle_height = if knob { x_length * r.uniform(0.1, 0.15) } else { x_length * r.uniform(0.2, 0.25) };
        let handle_radius = x_length * r.uniform(0.15, 0.25);
        let handle_width = x_length * r.uniform(0.25, 0.3);
        let sub = r.randint(0, 3);
        let m0 = if is_glass { matdraw::sample_init(&mut r, matdraw::FRONT_MAYBEGLASS) } else { matdraw::sample_init(&mut r, matdraw::DECORATIVE_HARD) };
        let m1 = matdraw::sample_init(&mut r, matdraw::METALS);
        let m2 = matdraw::sample_init(&mut r, matdraw::DECORATIVE_HARD);
        Lid { x_length, z_height, thickness, is_glass, rim_height, knob, handle_height, handle_radius, handle_width, sub, mats: [m0, m1, m2], fs }
    }

    fn cyl(&self, sx: f64, sz: f64, z0: f64) -> Mesh {
        let mut c = unit_cylinder(32, true);
        for p in c.pos.iter_mut() {
            *p = [(p[0] as f32 * sx as f32) as f64, (p[1] as f32 * sx as f32) as f64, (p[2] as f32 * sz as f32 + z0 as f32) as f64];
        }
        bevel(&c, Offset::Width(self.thickness / 2.0), 4, 30f64.to_radians()).unwrap_or(c)
    }
}

impl Lid {
    /// add_handle: wire arch (linear subdivision of loose edges), edge extrude along y, SOLIDIFY(offset 0), BEVEL, moved onto the lid
    fn handle(&self, body: &Mesh) -> Mesh {
        let f32r = |x: f64| x as f32 as f64;
        let (w, h, t) = (f32r(self.handle_width), f32r(self.handle_height), f32r(self.thickness));
        // z_offset: z of the centre of the face whose centre is closest (L1 in xy) to (handle_width, 0)
        let mut best = (f64::INFINITY, 0.0);
        for f in &body.faces {
            let mut c = [0.0f32; 3];
            for &v in f {
                for k in 0..3 {
                    c[k] += body.pos[v as usize][k] as f32;
                }
            }
            let n = f.len() as f32;
            let c = [c[0] / n, c[1] / n, c[2] / n];
            let d = (c[0] as f64 - w).abs() + (c[1] as f64).abs();
            if d < best.0 {
                best = (d, c[2] as f64);
            }
        }
        let z_off = best.1;
        let pts: Vec<[f64; 3]> = vec![[-w, 0.0, 0.0], [-w, 0.0, h], [w, 0.0, h], [w, 0.0, 0.0]];
        // SUBSURF on a wire: boundary-rule refinement ((a + 6p + b) / 8, midpoints), then the limit position ((a + 4p + b) / 6)
        let mut line: Vec<[f64; 3]> = pts.clone();
        for _ in 0..self.sub {
            let n0 = line.len();
            let mut nl = vec![];
            for i in 0..n0 {
                let p = line[i];
                if i == 0 || i == n0 - 1 {
                    nl.push(p);
                } else {
                    let (a, b) = (line[i - 1], line[i + 1]);
                    nl.push([0, 1, 2].map(|k| (a[k] + 6.0 * p[k] + b[k]) / 8.0));
                }
                if i + 1 < n0 {
                    let q = line[i + 1];
                    nl.push([0, 1, 2].map(|k| (p[k] + q[k]) / 2.0));
                }
            }
            line = nl;
        }
        if self.sub > 0 {
            let src = line.clone();
            for i in 1..src.len() - 1 {
                line[i] = [0, 1, 2].map(|k| (src[i - 1][k] + 4.0 * src[i][k] + src[i + 1][k]) / 6.0);
            }
        }
        let line: Vec<[f64; 3]> = line.iter().map(|p| [f32r(p[0]), f32r(p[1]), f32r(p[2])]).collect();
        // OpenSubdiv vertex order: coarse vertices, then the 2^L - 1 interior points of every coarse edge
        let l = self.sub as usize;
        let seg = 1usize << l;
        let nb = 3 * seg + 1;
        let mut ordered = vec![[0.0; 3]; nb];
        for (si, p) in line.iter().enumerate() {
            let bi = if si % seg == 0 { si / seg } else { 4 + (si / seg) * (seg - 1) + (si % seg - 1) };
            ordered[bi] = *p;
        }
        let n = nb;
        let mut m = Mesh::new();
        m.pos = ordered.clone();
        for p in &ordered {
            m.pos.push([p[0], p[1] + 2.0 * t, p[2]]);
        }
        m.faces = EXT_FACES[l].iter().map(|f| f.to_vec()).collect();
        let _ = n;
        let s = books::solidify_offset(&m, t, 0.0);
        dumpp("hsolid", &s);
        let eo: Vec<(u32, u32)> = SOLID_EDGES[l].iter().map(|e| (e[0], e[1])).collect();
        let mut b = match super::bevelfull::bevel_angle_ex(&s, self.thickness / 2.0, 4, 30f64.to_radians(), false, Some(&eo)) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("lid handle bevel: {e}");
                s
            }
        };
        for p in b.pos.iter_mut() {
            *p = [p[0], (p[1] as f32 - t as f32) as f64, (p[2] as f32 + z_off as f32) as f64];
        }
        b
    }
}

impl Factory for Lid {
    /// collision: the dome (x_length round, z_height high, its shell thickness about the surface), a glass lid's rim torus, the knob
    /// (a stem handle_height up, a cap up to two thicknesses) or the arched handle on top
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        let t = self.thickness;
        let r = self.x_length + t + 0.002;
        let z0 = -(t / 2.0).max(self.rim_height / 2.0) - 0.002;
        let z1 = self.z_height + self.handle_height + 3.0 * t + 0.002;
        Some(vec![crate::piece::Volume::hard([-r, -r, z0], [r, r, z1])])
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        for k in 0..3 {
            matdraw::call(&mut r, self.mats[k]);
        }
        let xs = [0.0, 0.01, self.x_length / 2.0, self.x_length];
        let zh = self.z_height;
        let zs = [zh, zh, zh * r.uniform(0.7, 0.8), 0.0];
        let anchors: Vec<[f64; 3]> = (0..4).map(|k| [xs[k], 0.0, zs[k]]).collect();
        let m = spin_anchors(&anchors, &[]);
        let s = books::solidify_offset(&m, self.thickness as f32 as f64, 0.0);
        let body = bevel(&s, Offset::Width(self.thickness / 2.0), 4, 30f64.to_radians()).unwrap_or(s);
        dumpp("part0", &body);
        let mut all = Mesh::new();
        all.append(&body);
        if self.is_glass {
            // torus: major radius x_length, minor radius thickness / 2, 128 x 12 segments, z scaled by rim_height / thickness
            let (maj, mnr) = (128usize, 12usize);
            let zs = (self.rim_height / self.thickness) as f32;
            let mut t = Mesh::new();
            for a in 0..maj {
                let th = (2.0 * std::f64::consts::PI * a as f64 / maj as f64) as f32;
                for b in 0..mnr {
                    let ph = (2.0 * std::f64::consts::PI * b as f64 / mnr as f64) as f32;
                    let rad = self.x_length as f32 + (self.thickness / 2.0) as f32 * ph.cos();
                    t.pos.push([(rad * th.cos()) as f64, (rad * th.sin()) as f64, ((self.thickness / 2.0) as f32 * ph.sin() * zs) as f64]);
                }
            }
            for a in 0..maj {
                let a1 = (a + 1) % maj;
                for b in 0..mnr {
                    let b1 = (b + 1) % mnr;
                    t.faces.push(vec![(a * mnr + b) as u32, (a1 * mnr + b) as u32, (a1 * mnr + b1) as u32, (a * mnr + b1) as u32]);
                }
            }
            dumpp("part1", &t);
            all.append(&t);
        }
        if !self.knob {
            let h = self.handle(&body);
            dumpp("part2", &h);
            all.append(&h);
        }
        if self.knob {
            let u1 = r.uniform(1.0, 2.0);
            let stem = self.cyl(self.thickness * u1, self.handle_height, self.z_height);
            let u2 = r.uniform(1.0, 2.0);
            let top = self.cyl(self.handle_radius, self.thickness * u2, self.z_height + self.handle_height);
            { let mut k = Mesh::new(); k.append(&stem); k.append(&top); dumpp("part1", &k); }
            all.append(&stem);
            all.append(&top);
        }
        let mat = spec_material("lid", json!({"cls": self.mats[0], "kwargs": {}, "seed": self.fs}), [0.8, 0.8, 0.8]);
        let mut a = Asset::default();
        let b = all.bounds();
        a.mesh(all, &mat, Some(b));
        a
    }
}

fn dumpp(name: &str, m: &Mesh) {
    if let Ok(dir) = std::env::var("PANDBG") {
        let j = serde_json::json!({"v": m.pos, "p": m.faces});
        let _ = std::fs::write(format!("{dir}/rs_{name}.json"), j.to_string());
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Lid::new(fs)))
}

// generated from tmp/ext2.py: Blender's extrude_edges_move face order / SOLIDIFY edge order for the lid handle wire (subdivision level 0..2)
const EXT_FACES_0: &[&[u32]] = &[&[3,2,6,7],&[2,1,5,6],&[1,0,4,5]];
const SOLID_EDGES_0: &[&[u32]] = &[&[0,1],&[1,2],&[2,3],&[4,5],&[5,6],&[6,7],&[7,3],&[2,6],&[1,5],&[0,4],&[8,9],&[9,10],&[10,11],&[12,13],&[13,14],&[14,15],&[15,11],&[10,14],&[9,13],&[8,12],&[0,8],&[1,9],&[2,10],&[3,11],&[4,12],&[5,13],&[6,14],&[7,15]];
const EXT_FACES_1: &[&[u32]] = &[&[6,2,9,13],&[5,1,8,12],&[4,0,7,11],&[3,6,13,10],&[2,5,12,9],&[1,4,11,8]];
const SOLID_EDGES_1: &[&[u32]] = &[&[0,4],&[4,1],&[1,5],&[5,2],&[2,6],&[6,3],&[7,11],&[11,8],&[8,12],&[12,9],&[9,13],&[13,10],&[13,6],&[2,9],&[12,5],&[1,8],&[11,4],&[0,7],&[10,3],&[14,18],&[18,15],&[15,19],&[19,16],&[16,20],&[20,17],&[21,25],&[25,22],&[22,26],&[26,23],&[23,27],&[27,24],&[27,20],&[16,23],&[26,19],&[15,22],&[25,18],&[14,21],&[24,17],&[0,14],&[1,15],&[2,16],&[3,17],&[4,18],&[5,19],&[6,20],&[7,21],&[8,22],&[9,23],&[10,24],&[11,25],&[12,26],&[13,27]];
const EXT_FACES_2: &[&[u32]] = &[&[6,5,18,19],&[11,10,23,24],&[9,8,21,22],&[1,6,19,14],&[12,11,24,25],&[4,0,13,17],&[2,9,22,15],&[7,1,14,20],&[3,12,25,16],&[5,4,17,18],&[10,2,15,23],&[8,7,20,21]];
const SOLID_EDGES_2: &[&[u32]] = &[&[0,4],&[4,5],&[5,6],&[6,1],&[1,7],&[7,8],&[8,9],&[9,2],&[2,10],&[10,11],&[11,12],&[12,3],&[13,17],&[17,18],&[18,19],&[19,14],&[14,20],&[20,21],&[21,22],&[22,15],&[15,23],&[23,24],&[24,25],&[25,16],&[19,6],&[5,18],&[24,11],&[10,23],&[22,9],&[8,21],&[14,1],&[25,12],&[17,4],&[0,13],&[15,2],&[20,7],&[16,3],&[26,30],&[30,31],&[31,32],&[32,27],&[27,33],&[33,34],&[34,35],&[35,28],&[28,36],&[36,37],&[37,38],&[38,29],&[39,43],&[43,44],&[44,45],&[45,40],&[40,46],&[46,47],&[47,48],&[48,41],&[41,49],&[49,50],&[50,51],&[51,42],&[45,32],&[31,44],&[50,37],&[36,49],&[48,35],&[34,47],&[40,27],&[51,38],&[43,30],&[26,39],&[41,28],&[46,33],&[42,29],&[0,26],&[1,27],&[2,28],&[3,29],&[4,30],&[5,31],&[6,32],&[7,33],&[8,34],&[9,35],&[10,36],&[11,37],&[12,38],&[13,39],&[14,40],&[15,41],&[16,42],&[17,43],&[18,44],&[19,45],&[20,46],&[21,47],&[22,48],&[23,49],&[24,50],&[25,51]];
const EXT_FACES: [&[&[u32]]; 3] = [EXT_FACES_0, EXT_FACES_1, EXT_FACES_2];
const SOLID_EDGES: [&[&[u32]]; 3] = [SOLID_EDGES_0, SOLID_EDGES_1, SOLID_EDGES_2];
