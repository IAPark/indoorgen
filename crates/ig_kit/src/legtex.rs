//! Legacy procedural textures (Stucci, Marble, Clouds) and the DISPLACE modifier (direction NORMAL, local texture coordinates).
//! Ported from texture_procedural.cc / MOD_displace.cc (Blender 4.2).
use super::legacynoise::*;
use crate::mesh::Mesh;

#[derive(Clone, Copy, Debug)]
pub enum TexKind {
    /// stype: 0 PLASTIC, 1 WALL_IN, 2 WALL_OUT
    Stucci(i32),
    /// stype: 0 SOFT, 1 SHARP, 2 SHARPER; wave: 0 SIN, 1 SAW, 2 TRI
    Marble(i32, i32),
    Clouds,
}

#[derive(Clone, Copy, Debug)]
pub struct Tex {
    pub kind: TexKind,
    pub noisesize: f32,
    pub noisedepth: i32,
    pub hard: bool,
    pub basis: i32,
    pub turbul: f32,
    pub bright: f32,
    pub contrast: f32,
}

impl Tex {
    /// bpy.data.textures.new(type=...) defaults
    pub fn new(kind: TexKind, noisesize: f64) -> Tex {
        Tex { kind, noisesize: noisesize as f32, noisedepth: 2, hard: false, basis: 0, turbul: 5.0, bright: 1.0, contrast: 1.0 }
    }

    fn bricont(&self, t: f32) -> f32 {
        ((t - 0.5) * self.contrast + self.bright - 0.5).clamp(0.0, 1.0)
    }

    /// intensity (texres.tin)
    pub fn value(&self, x: f32, y: f32, z: f32) -> f32 {
        match self.kind {
            TexKind::Clouds => self.bricont(generic_turbulence(self.noisesize, x, y, z, self.noisedepth, self.hard, self.basis)),
            TexKind::Stucci(stype) => {
                let b2 = generic_noise(self.noisesize, x, y, z, self.hard, self.basis);
                let mut ofs = self.turbul / 200.0;
                if stype != 0 {
                    ofs *= b2 * b2;
                }
                let mut t = generic_noise(self.noisesize, x, y, z + ofs, self.hard, self.basis);
                if stype == 2 {
                    t = 1.0 - t;
                }
                if t < 0.0 {
                    t = 0.0;
                }
                t
            }
            TexKind::Marble(mt, wf) => {
                let n = 5.0 * (x + y + z);
                let mut mi = n + self.turbul * generic_turbulence(self.noisesize, x, y, z, self.noisedepth, self.hard, self.basis);
                let b = 2.0 * std::f32::consts::PI;
                mi = match wf {
                    1 => {
                        let k = (mi / b) as i32;
                        let mut a = mi - k as f32 * b;
                        if a < 0.0 {
                            a += b;
                        }
                        a / b
                    }
                    2 => 1.0 - 2.0 * (((mi * (1.0 / b)) + 0.5).floor() - (mi * (1.0 / b))).abs(),
                    _ => 0.5 + 0.5 * mi.sin(),
                };
                if mt == 1 {
                    mi = mi.sqrt();
                } else if mt == 2 {
                    mi = mi.sqrt().sqrt();
                }
                self.bricont(mi)
            }
        }
    }
}

/// BLI math::safe_acos_approx (max error 4.5e-5): the angle weight of Mesh::vert_normals
pub fn safe_acos_approx(x: f32) -> f32 {
    let f = x.abs();
    let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
    let a = (1.0 - m).sqrt() * (1.570_796_3 + m * (-0.213_300_99 + m * (0.077_980_48 + m * -0.021_640_95)));
    if x < 0.0 {
        std::f32::consts::PI - a
    } else {
        a
    }
}

/// angle-weighted vertex normals in float32 (Mesh::vert_normals)
pub fn vert_normals(m: &Mesh) -> Vec<[f32; 3]> {
    type F3 = [f32; 3];
    let p32: Vec<F3> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let sub = |a: F3, b: F3| -> F3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] };
    let dot = |a: F3, b: F3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: F3, b: F3| -> F3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] };
    let norm = |a: F3| -> F3 {
        let d = dot(a, a);
        if d > 1.0e-35 {
            let inv = 1.0 / d.sqrt();
            [a[0] * inv, a[1] * inv, a[2] * inv]
        } else {
            [0.0; 3]
        }
    };
    // math::normalize divides by the length (normalize_v3 multiplies by the inverse)
    let normd = |a: F3| -> F3 {
        let d = dot(a, a);
        if d > 1.0e-35 {
            let l = d.sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        } else {
            [0.0; 3]
        }
    };
    let mut acc = vec![[0.0f32; 3]; p32.len()];
    for f in &m.faces {
        let k = f.len();
        let p: Vec<F3> = f.iter().map(|&i| p32[i as usize]).collect();
        // normals_calc_faces: Newell's method for every face size
        let fnrm: F3 = {
            let mut n = [0.0f32; 3];
            let mut vp = p[k - 1];
            for i in 0..k {
                let vc = p[i];
                n[0] += (vp[1] - vc[1]) * (vp[2] + vc[2]);
                n[1] += (vp[2] - vc[2]) * (vp[0] + vc[0]);
                n[2] += (vp[0] - vc[0]) * (vp[1] + vc[1]);
                vp = vc;
            }
            let nn = norm(n);
            if nn == [0.0; 3] {
                [0.0, 0.0, 1.0]
            } else {
                nn
            }
        };
        for i in 0..k {
            let prev = normd(sub(p[(i + k - 1) % k], p[i]));
            let next = normd(sub(p[(i + 1) % k], p[i]));
            let ang = safe_acos_approx(dot(prev, next));
            let v = f[i] as usize;
            for c in 0..3 {
                acc[v][c] += fnrm[c] * ang;
            }
        }
    }
    acc.into_iter().enumerate().map(|(i, a)| { let n = normd(a); if n == [0.0; 3] { normd(p32[i]) } else { n } }).collect()
}

/// DISPLACE modifier, direction NORMAL, texture coords = local vertex positions
pub fn displace(m: &mut Mesh, tex: &Tex, strength: f64, midlevel: f64) {
    if strength == 0.0 {
        return;
    }
    let nrm = vert_normals(m);
    let (strength, mid) = (strength as f32, midlevel as f32);
    for (i, p) in m.pos.iter_mut().enumerate() {
        let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let t = tex.value(x, y, z);
        let delta = ((t - mid) * strength).clamp(-10000.0, 10000.0);
        p[0] = (x + nrm[i][0] * delta) as f64;
        p[1] = (y + nrm[i][1] * delta) as f64;
        p[2] = (z + nrm[i][2] * delta) as f64;
    }
}
