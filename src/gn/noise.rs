//! Blender 4.2 Perlin noise / fBm (intern/cycles + blenlib noise.cc) in float32.
use super::math::{hash1, hash2, hash3, hash4};

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
#[inline]
fn negate_if(v: f32, c: u32) -> f32 {
    if c != 0 {
        -v
    } else {
        v
    }
}
#[inline]
fn floor_frac(x: f32) -> (i32, f32) {
    let f = x.floor();
    (f as i32, x - f)
}
#[inline]
fn mixf(a: f32, b: f32, t: f32) -> f32 {
    (1.0 - t) * a + t * b
}
fn grad1(h: u32, x: f32) -> f32 {
    let h = h & 15;
    let g = 1.0 + (h & 7) as f32;
    negate_if(g, h & 8) * x
}
fn grad2(h: u32, x: f32, y: f32) -> f32 {
    let h = h & 7;
    let u = if h < 4 { x } else { y };
    let v = 2.0 * if h < 4 { y } else { x };
    negate_if(u, h & 1) + negate_if(v, h & 2)
}
fn grad3(h: u32, x: f32, y: f32, z: f32) -> f32 {
    let h = h & 15;
    let u = if h < 8 { x } else { y };
    let vt = if h == 12 || h == 14 { x } else { z };
    let v = if h < 4 { y } else { vt };
    negate_if(u, h & 1) + negate_if(v, h & 2)
}
fn grad4(h: u32, x: f32, y: f32, z: f32, w: f32) -> f32 {
    let h = h & 31;
    let u = if h < 24 { x } else { y };
    let v = if h < 16 { y } else { z };
    let s = if h < 8 { z } else { w };
    negate_if(u, h & 1) + negate_if(v, h & 2) + negate_if(s, h & 4)
}
fn bi_mix(v0: f32, v1: f32, v2: f32, v3: f32, x: f32, y: f32) -> f32 {
    let x1 = 1.0 - x;
    (1.0 - y) * (v0 * x1 + v1 * x) + y * (v2 * x1 + v3 * x)
}
#[allow(clippy::too_many_arguments)]
fn tri_mix(v0: f32, v1: f32, v2: f32, v3: f32, v4: f32, v5: f32, v6: f32, v7: f32, x: f32, y: f32, z: f32) -> f32 {
    let x1 = 1.0 - x;
    let y1 = 1.0 - y;
    let z1 = 1.0 - z;
    z1 * (y1 * (v0 * x1 + v1 * x) + y * (v2 * x1 + v3 * x)) + z * (y1 * (v4 * x1 + v5 * x) + y * (v6 * x1 + v7 * x))
}

pub fn perlin1(x: f32) -> f32 {
    let (xi, fx) = floor_frac(x);
    let u = fade(fx);
    let r = mixf(grad1(hash1(xi as u32), fx), grad1(hash1(xi.wrapping_add(1) as u32), fx - 1.0), u);
    0.2500 * r
}
pub fn perlin2(p: [f32; 2]) -> f32 {
    let (xi, fx) = floor_frac(p[0]);
    let (yi, fy) = floor_frac(p[1]);
    let (u, v) = (fade(fx), fade(fy));
    let h = |a: i32, b: i32| hash2(a as u32, b as u32);
    let r = bi_mix(
        grad2(h(xi, yi), fx, fy),
        grad2(h(xi.wrapping_add(1), yi), fx - 1.0, fy),
        grad2(h(xi, yi.wrapping_add(1)), fx, fy - 1.0),
        grad2(h(xi.wrapping_add(1), yi.wrapping_add(1)), fx - 1.0, fy - 1.0),
        u,
        v,
    );
    0.6616 * r
}
pub fn perlin3(p: [f32; 3]) -> f32 {
    let (xi, fx) = floor_frac(p[0]);
    let (yi, fy) = floor_frac(p[1]);
    let (zi, fz) = floor_frac(p[2]);
    let (u, v, w) = (fade(fx), fade(fy), fade(fz));
    let h = |a: i32, b: i32, c: i32| hash3(a as u32, b as u32, c as u32);
    let (x1, y1, z1) = (xi.wrapping_add(1), yi.wrapping_add(1), zi.wrapping_add(1));
    let r = tri_mix(
        grad3(h(xi, yi, zi), fx, fy, fz),
        grad3(h(x1, yi, zi), fx - 1.0, fy, fz),
        grad3(h(xi, y1, zi), fx, fy - 1.0, fz),
        grad3(h(x1, y1, zi), fx - 1.0, fy - 1.0, fz),
        grad3(h(xi, yi, z1), fx, fy, fz - 1.0),
        grad3(h(x1, yi, z1), fx - 1.0, fy, fz - 1.0),
        grad3(h(xi, y1, z1), fx, fy - 1.0, fz - 1.0),
        grad3(h(x1, y1, z1), fx - 1.0, fy - 1.0, fz - 1.0),
        u,
        v,
        w,
    );
    0.9820 * r
}
pub fn perlin4(p: [f32; 4]) -> f32 {
    let (xi, fx) = floor_frac(p[0]);
    let (yi, fy) = floor_frac(p[1]);
    let (zi, fz) = floor_frac(p[2]);
    let (wi, fw) = floor_frac(p[3]);
    let (u, v, t, s) = (fade(fx), fade(fy), fade(fz), fade(fw));
    let h = |a: i32, b: i32, c: i32, d: i32| hash4(a as u32, b as u32, c as u32, d as u32);
    let (x1, y1, z1, w1) = (xi.wrapping_add(1), yi.wrapping_add(1), zi.wrapping_add(1), wi.wrapping_add(1));
    let g = |a: i32, b: i32, c: i32, d: i32, ox: f32, oy: f32, oz: f32, ow: f32| grad4(h(a, b, c, d), fx - ox, fy - oy, fz - oz, fw - ow);
    let w0 = tri_mix(
        g(xi, yi, zi, wi, 0.0, 0.0, 0.0, 0.0),
        g(x1, yi, zi, wi, 1.0, 0.0, 0.0, 0.0),
        g(xi, y1, zi, wi, 0.0, 1.0, 0.0, 0.0),
        g(x1, y1, zi, wi, 1.0, 1.0, 0.0, 0.0),
        g(xi, yi, z1, wi, 0.0, 0.0, 1.0, 0.0),
        g(x1, yi, z1, wi, 1.0, 0.0, 1.0, 0.0),
        g(xi, y1, z1, wi, 0.0, 1.0, 1.0, 0.0),
        g(x1, y1, z1, wi, 1.0, 1.0, 1.0, 0.0),
        u,
        v,
        t,
    );
    let w1v = tri_mix(
        g(xi, yi, zi, w1, 0.0, 0.0, 0.0, 1.0),
        g(x1, yi, zi, w1, 1.0, 0.0, 0.0, 1.0),
        g(xi, y1, zi, w1, 0.0, 1.0, 0.0, 1.0),
        g(x1, y1, zi, w1, 1.0, 1.0, 0.0, 1.0),
        g(xi, yi, z1, w1, 0.0, 0.0, 1.0, 1.0),
        g(x1, yi, z1, w1, 1.0, 0.0, 1.0, 1.0),
        g(xi, y1, z1, w1, 0.0, 1.0, 1.0, 1.0),
        g(x1, y1, z1, w1, 1.0, 1.0, 1.0, 1.0),
        u,
        v,
        t,
    );
    0.8344 * mixf(w0, w1v, s)
}

/// coordinate vector of dimension 1..4
pub fn perlin_dim(dim: u8, p: [f32; 4]) -> f32 {
    match dim {
        1 => perlin1(p[3]),
        2 => perlin2([p[0], p[1]]),
        3 => perlin3([p[0], p[1], p[2]]),
        _ => perlin4(p),
    }
}

fn scale_p(dim: u8, s: f32, p: [f32; 4]) -> [f32; 4] {
    let _ = dim;
    [p[0] * s, p[1] * s, p[2] * s, p[3] * s]
}

pub struct NoiseParams {
    pub dim: u8,
    pub kind: String,
    pub normalize: bool,
    pub detail: f32,
    pub roughness: f32,
    pub lacunarity: f32,
    pub offset: f32,
    pub gain: f32,
}

pub fn noise_value(prm: &NoiseParams, p: [f32; 4]) -> f32 {
    let n = |q: [f32; 4]| perlin_dim(prm.dim, q);
    let detail = prm.detail.clamp(0.0, 15.0);
    let rough = prm.roughness.clamp(0.0, 1.0);
    match prm.kind.as_str() {
        "MULTIFRACTAL" => {
            let mut value = 1.0f32;
            let mut pwr = 1.0f32;
            let mut fs = 1.0f32;
            for _ in 0..=(detail as i32) {
                let t = n(scale_p(prm.dim, fs, p));
                value *= pwr * t + 1.0;
                pwr *= rough;
                fs *= prm.lacunarity;
            }
            let rmd = detail - detail.floor();
            if rmd != 0.0 {
                let t = n(scale_p(prm.dim, fs, p));
                value *= rmd * pwr * t + 1.0;
            }
            value
        }
        "RIDGED_MULTIFRACTAL" => {
            let mut pp = p;
            let mut pwr = rough;
            let mut signal = prm.offset - n(pp).abs();
            signal *= signal;
            let mut value = signal;
            for _ in 1..=(detail as i32) {
                pp = scale_p(prm.dim, prm.lacunarity, pp);
                let weight = (signal * prm.gain).clamp(0.0, 1.0);
                signal = prm.offset - n(pp).abs();
                signal *= signal;
                signal *= weight;
                value += signal * pwr;
                pwr *= rough;
            }
            value
        }
        "HYBRID_MULTIFRACTAL" => {
            let mut pp = p;
            let mut pwr = rough;
            let mut value = n(pp) + prm.offset;
            let mut weight = prm.gain * value;
            pp = scale_p(prm.dim, prm.lacunarity, pp);
            let mut i = 1;
            while weight > 0.001 && i <= detail as i32 {
                if weight > 1.0 {
                    weight = 1.0;
                }
                let signal = (n(pp) + prm.offset) * pwr;
                pwr *= rough;
                value += weight * signal;
                weight *= prm.gain * signal;
                pp = scale_p(prm.dim, prm.lacunarity, pp);
                i += 1;
            }
            let rmd = detail - detail.floor();
            if rmd != 0.0 && weight > 0.001 {
                if weight > 1.0 {
                    weight = 1.0;
                }
                let signal = (n(pp) + prm.offset) * pwr;
                value += rmd * weight * signal;
            }
            value
        }
        "HETERO_TERRAIN" => {
            let mut pp = p;
            let mut pwr = rough;
            let mut value = prm.offset + n(pp);
            pp = scale_p(prm.dim, prm.lacunarity, pp);
            for _ in 1..=(detail as i32) {
                let inc = (n(pp) + prm.offset) * pwr * value;
                value += inc;
                pwr *= rough;
                pp = scale_p(prm.dim, prm.lacunarity, pp);
            }
            let rmd = detail - detail.floor();
            if rmd != 0.0 {
                value += rmd * ((n(pp) + prm.offset) * pwr * value);
            }
            value
        }
        _ => {
            // FBM
            let mut fscale = 1.0f32;
            let mut amp = 1.0f32;
            let mut maxamp = 0.0f32;
            let mut sum = 0.0f32;
            for _ in 0..=(detail as i32) {
                let t = n(scale_p(prm.dim, fscale, p));
                sum += t * amp;
                maxamp += amp;
                amp *= rough;
                fscale *= prm.lacunarity;
            }
            let rmd = detail - detail.floor();
            if rmd != 0.0 {
                let t = n(scale_p(prm.dim, fscale, p));
                let sum2 = sum + t * amp;
                if prm.normalize {
                    mixf(0.5 * sum / maxamp + 0.5, 0.5 * sum2 / (maxamp + amp) + 0.5, rmd)
                } else {
                    mixf(sum, sum2, rmd)
                }
            } else if prm.normalize {
                0.5 * sum / maxamp + 0.5
            } else {
                sum
            }
        }
    }
}

fn fbits(x: f32) -> u32 {
    x.to_bits()
}
/// hash_float_to_float(float2(seed, comp)) based offset, 100..200
fn rand_off(seed: f32, comp: f32) -> f32 {
    100.0 + (hash2(fbits(seed), fbits(comp)) as f32 / 0xFFFFFFFFu32 as f32) * 100.0
}

/// noise texture distortion: p += perlin(p + offset(k)) * dist for every coordinate
pub fn distort(dim: u8, p: [f32; 4], dist: f32) -> [f32; 4] {
    let mut q = p;
    match dim {
        1 => {
            let o = 100.0 + (hash1(fbits(0.0)) as f32 / 0xFFFFFFFFu32 as f32) * 100.0;
            q[3] = p[3] + perlin1(p[3] + o) * dist;
        }
        2 => {
            for k in 0..2 {
                let o = [rand_off(k as f32, 0.0), rand_off(k as f32, 1.0)];
                q[k] = p[k] + perlin2([p[0] + o[0], p[1] + o[1]]) * dist;
            }
        }
        3 => {
            for k in 0..3 {
                let o = [rand_off(k as f32, 0.0), rand_off(k as f32, 1.0), rand_off(k as f32, 2.0)];
                q[k] = p[k] + perlin3([p[0] + o[0], p[1] + o[1], p[2] + o[2]]) * dist;
            }
        }
        _ => {
            for k in 0..4 {
                let o = [rand_off(k as f32, 0.0), rand_off(k as f32, 1.0), rand_off(k as f32, 2.0), rand_off(k as f32, 3.0)];
                q[k] = p[k] + perlin4([p[0] + o[0], p[1] + o[1], p[2] + o[2], p[3] + o[3]]) * dist;
            }
        }
    }
    q
}

// ------------------------------------------------------------------ hashing of floats (BLI_hash)
const HM: f32 = 0xFFFFFFFFu32 as f32;
pub fn hf1(k: f32) -> f32 {
    hash1(k.to_bits()) as f32 / HM
}
pub fn hf2(a: f32, b: f32) -> f32 {
    hash2(a.to_bits(), b.to_bits()) as f32 / HM
}
pub fn hf3(a: f32, b: f32, c: f32) -> f32 {
    hash3(a.to_bits(), b.to_bits(), c.to_bits()) as f32 / HM
}
pub fn hf4(a: f32, b: f32, c: f32, d: f32) -> f32 {
    hash4(a.to_bits(), b.to_bits(), c.to_bits(), d.to_bits()) as f32 / HM
}
pub fn hash_f3_to_f3(k: [f32; 3]) -> [f32; 3] {
    [hf3(k[0], k[1], k[2]), hf4(k[0], k[1], k[2], 1.0), hf4(k[0], k[1], k[2], 2.0)]
}

// ------------------------------------------------------------------ Voronoi (3D)
pub struct VoronoiParams {
    pub feature: String,
    pub metric: String,
    pub randomness: f32,
    pub smoothness: f32,
    pub exponent: f32,
}

fn vdist(a: [f32; 3], b: [f32; 3], p: &VoronoiParams) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    match p.metric.as_str() {
        "MANHATTAN" => d[0].abs() + d[1].abs() + d[2].abs(),
        "CHEBYCHEV" => d[0].abs().max(d[1].abs()).max(d[2].abs()),
        "MINKOWSKI" => (d[0].abs().powf(p.exponent) + d[1].abs().powf(p.exponent) + d[2].abs().powf(p.exponent)).powf(1.0 / p.exponent),
        _ => (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
    }
}

fn smoothstep01(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// (distance, color, position, radius) of the 3D Voronoi texture at coord (already scaled)
pub fn voronoi3(coord: [f32; 3], p: &VoronoiParams) -> (f32, [f32; 3], [f32; 3], f32) {
    let cell = [coord[0].floor(), coord[1].floor(), coord[2].floor()];
    let local = [coord[0] - cell[0], coord[1] - cell[1], coord[2] - cell[2]];
    let pt = |off: [f32; 3]| -> [f32; 3] {
        let h = hash_f3_to_f3([cell[0] + off[0], cell[1] + off[1], cell[2] + off[2]]);
        [off[0] + h[0] * p.randomness, off[1] + h[1] * p.randomness, off[2] + h[2] * p.randomness]
    };
    let add = |a: [f32; 3], b: [f32; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
    match p.feature.as_str() {
        "F2" => {
            let (mut d1, mut d2) = (8.0f32, 8.0f32);
            let (mut off1, mut off2) = ([0.0f32; 3], [0.0f32; 3]);
            let (mut pos1, mut pos2) = ([0.0f32; 3], [0.0f32; 3]);
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        let o = [i as f32, j as f32, k as f32];
                        let pp = pt(o);
                        let d = vdist(pp, local, p);
                        if d < d1 {
                            d2 = d1;
                            d1 = d;
                            off2 = off1;
                            off1 = o;
                            pos2 = pos1;
                            pos1 = pp;
                        } else if d < d2 {
                            d2 = d;
                            off2 = o;
                            pos2 = pp;
                        }
                    }
                }
            }
            let _ = (off1, pos1);
            (d2, hash_f3_to_f3(add(cell, off2)), add(pos2, cell), 0.0)
        }
        "SMOOTH_F1" => {
            let mut sd = 8.0f32;
            let mut sc = [0.0f32; 3];
            let mut sp = [0.0f32; 3];
            let mut h = -1.0f32;
            for k in -2..=2 {
                for j in -2..=2 {
                    for i in -2..=2 {
                        let o = [i as f32, j as f32, k as f32];
                        let pp = pt(o);
                        let d = vdist(pp, local, p);
                        h = if h == -1.0 { 1.0 } else { smoothstep01(0.5 + 0.5 * (sd - d) / p.smoothness) };
                        let mut corr = p.smoothness * h * (1.0 - h);
                        sd = sd * (1.0 - h) + d * h - corr;
                        corr /= 1.0 + 3.0 * p.smoothness;
                        let cc = hash_f3_to_f3(add(cell, o));
                        for q in 0..3 {
                            sc[q] = sc[q] * (1.0 - h) + cc[q] * h - corr;
                            sp[q] = sp[q] * (1.0 - h) + pp[q] * h - corr;
                        }
                    }
                }
            }
            (sd, sc, add(sp, cell), 0.0)
        }
        "DISTANCE_TO_EDGE" => {
            let mut closest = [0.0f32; 3];
            let mut md = 8.0f32;
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        let o = [i as f32, j as f32, k as f32];
                        let v = {
                            let pp = pt(o);
                            [pp[0] - local[0], pp[1] - local[1], pp[2] - local[2]]
                        };
                        let d = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
                        if d < md {
                            md = d;
                            closest = v;
                        }
                    }
                }
            }
            let mut md = 8.0f32;
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        let o = [i as f32, j as f32, k as f32];
                        let pp = pt(o);
                        let v = [pp[0] - local[0], pp[1] - local[1], pp[2] - local[2]];
                        let perp = [v[0] - closest[0], v[1] - closest[1], v[2] - closest[2]];
                        if perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2] > 0.0001 {
                            let mid = [(closest[0] + v[0]) / 2.0, (closest[1] + v[1]) / 2.0, (closest[2] + v[2]) / 2.0];
                            let l = (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt();
                            let nrm = [perp[0] / l, perp[1] / l, perp[2] / l];
                            let de = mid[0] * nrm[0] + mid[1] * nrm[1] + mid[2] * nrm[2];
                            md = md.min(de);
                        }
                    }
                }
            }
            (md, [0.0; 3], [0.0; 3], 0.0)
        }
        "N_SPHERE_RADIUS" => {
            let mut md = 8.0f32;
            let mut closest_off = [0.0f32; 3];
            let mut closest_pt = [0.0f32; 3];
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        let o = [i as f32, j as f32, k as f32];
                        let pp = pt(o);
                        let d = ((pp[0] - local[0]).powi(2) + (pp[1] - local[1]).powi(2) + (pp[2] - local[2]).powi(2)).sqrt();
                        if d < md {
                            md = d;
                            closest_off = o;
                            closest_pt = pp;
                        }
                    }
                }
            }
            let mut md = 8.0f32;
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        if i == 0 && j == 0 && k == 0 {
                            continue;
                        }
                        let o = [i as f32 + closest_off[0], j as f32 + closest_off[1], k as f32 + closest_off[2]];
                        let pp = pt(o);
                        let d = ((closest_pt[0] - pp[0]).powi(2) + (closest_pt[1] - pp[1]).powi(2) + (closest_pt[2] - pp[2]).powi(2)).sqrt();
                        if d < md {
                            md = d;
                        }
                    }
                }
            }
            (0.0, [0.0; 3], [0.0; 3], md / 2.0)
        }
        _ => {
            // F1
            let mut md = 8.0f32;
            let mut toff = [0.0f32; 3];
            let mut tpos = [0.0f32; 3];
            for k in -1..=1 {
                for j in -1..=1 {
                    for i in -1..=1 {
                        let o = [i as f32, j as f32, k as f32];
                        let pp = pt(o);
                        let d = vdist(pp, local, p);
                        if d < md {
                            md = d;
                            toff = o;
                            tpos = pp;
                        }
                    }
                }
            }
            (md, hash_f3_to_f3(add(cell, toff)), add(tpos, cell), 0.0)
        }
    }
}

// ------------------------------------------------------------------ Voronoi (1D-4D, fractal), port of blenlib/intern/noise.cc
pub struct VorP {
    pub feature: u8, // 0 F1, 1 F2, 2 SMOOTH_F1, 3 DISTANCE_TO_EDGE, 4 N_SPHERE_RADIUS
    pub metric: u8,  // 0 euclidean, 1 manhattan, 2 chebychev, 3 minkowski
    pub dim: usize,
    pub scale: f32,
    pub detail: f32,
    pub roughness: f32,
    pub lacunarity: f32,
    pub smoothness: f32,
    pub exponent: f32,
    pub randomness: f32,
    pub normalize: bool,
    pub max_distance: f32,
}

type V4 = [f32; 4];
#[derive(Clone, Copy, Default)]
pub struct VorOut {
    pub distance: f32,
    pub color: [f32; 3],
    pub position: V4,
}

fn vhash_point(d: usize, k: V4) -> V4 {
    match d {
        1 => [hf1(k[0]), 0.0, 0.0, 0.0],
        2 => [hf2(k[0], k[1]), hf3(k[0], k[1], 1.0), 0.0, 0.0],
        3 => {
            let h = hash_f3_to_f3([k[0], k[1], k[2]]);
            [h[0], h[1], h[2], 0.0]
        }
        _ => [hf4(k[0], k[1], k[2], k[3]), hf4(k[3], k[0], k[1], k[2]), hf4(k[2], k[3], k[0], k[1]), hf4(k[1], k[2], k[3], k[0])],
    }
}
fn vhash_color(d: usize, k: V4) -> [f32; 3] {
    match d {
        1 => [hf1(k[0]), hf2(k[0], 1.0), hf2(k[0], 2.0)],
        2 => [hf2(k[0], k[1]), hf3(k[0], k[1], 1.0), hf3(k[0], k[1], 2.0)],
        3 => hash_f3_to_f3([k[0], k[1], k[2]]),
        _ => [hf4(k[0], k[1], k[2], k[3]), hf4(k[2], k[0], k[3], k[1]), hf4(k[3], k[2], k[1], k[0])],
    }
}
pub fn vdist_pub(d: usize, a: V4, b: V4, p: &VorP) -> f32 {
    vdist_nd(d, a, b, p)
}
fn vdist_nd(d: usize, a: V4, b: V4, p: &VorP) -> f32 {
    if d == 1 {
        return (b[0] - a[0]).abs();
    }
    let df: Vec<f32> = (0..d).map(|i| (a[i] - b[i]).abs()).collect();
    match p.metric {
        1 => {
            let mut s = df[0];
            for x in &df[1..] {
                s += *x;
            }
            s
        }
        2 => {
            let mut m = df[d - 1];
            for i in (0..d - 1).rev() {
                m = df[i].max(m);
            }
            m
        }
        3 => {
            let mut s = df[0].powf(p.exponent);
            for x in &df[1..] {
                s += x.powf(p.exponent);
            }
            s.powf(1.0 / p.exponent)
        }
        _ => {
            let mut s = 0.0f32;
            for i in 0..d {
                let t = a[i] - b[i];
                s += t * t;
            }
            s.sqrt()
        }
    }
}
fn euclid(d: usize, a: V4, b: V4) -> f32 {
    let mut s = 0.0f32;
    for i in 0..d {
        let t = a[i] - b[i];
        s += t * t;
    }
    s.sqrt()
}
fn vfloor(d: usize, c: V4) -> V4 {
    let mut o = [0.0; 4];
    for i in 0..d {
        o[i] = c[i].floor();
    }
    o
}
fn vsub(d: usize, a: V4, b: V4) -> V4 {
    let mut o = [0.0; 4];
    for i in 0..d {
        o[i] = a[i] - b[i];
    }
    o
}
fn vadd(d: usize, a: V4, b: V4) -> V4 {
    let mut o = [0.0; 4];
    for i in 0..d {
        o[i] = a[i] + b[i];
    }
    o
}
/// offsets of the scan window in Blender's loop order (first axis fastest)
fn window(d: usize, lo: i32, hi: i32) -> Vec<V4> {
    let n = (hi - lo + 1) as usize;
    let total = n.pow(d as u32);
    (0..total)
        .map(|mut idx| {
            let mut o = [0.0f32; 4];
            for a in 0..d {
                o[a] = (lo + (idx % n) as i32) as f32;
                idx /= n;
            }
            o
        })
        .collect()
}
fn point_at(d: usize, cell: V4, off: V4, rnd: f32) -> V4 {
    let h = vhash_point(d, vadd(d, cell, off));
    let mut o = [0.0; 4];
    for i in 0..d {
        o[i] = off[i] + h[i] * rnd;
    }
    o
}
fn vpos_out(d: usize, c: V4) -> V4 {
    // voronoi_position: 1D puts the coordinate into w, others keep their components
    if d == 1 {
        [0.0, 0.0, 0.0, c[0]]
    } else {
        let mut o = [0.0; 4];
        for i in 0..d {
            o[i] = c[i];
        }
        o
    }
}
fn smooth01(x: f32) -> f32 {
    if x >= 1.0 {
        1.0
    } else if x <= 0.0 {
        0.0
    } else {
        x * x * (3.0 - 2.0 * x)
    }
}

fn vor_f1(p: &VorP, coord: V4) -> VorOut {
    let d = p.dim;
    let cell = vfloor(d, coord);
    let local = vsub(d, coord, cell);
    let mut md = f32::MAX;
    let mut toff = [0.0; 4];
    let mut tpos = [0.0; 4];
    for off in window(d, -1, 1) {
        let pp = point_at(d, cell, off, p.randomness);
        let dd = vdist_nd(d, pp, local, p);
        if dd < md {
            toff = off;
            md = dd;
            tpos = pp;
        }
    }
    VorOut { distance: md, color: vhash_color(d, vadd(d, cell, toff)), position: vpos_out(d, vadd(d, tpos, cell)) }
}
fn vor_f2(p: &VorP, coord: V4) -> VorOut {
    let d = p.dim;
    let cell = vfloor(d, coord);
    let local = vsub(d, coord, cell);
    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
    let (mut o1, mut o2) = ([0.0; 4], [0.0; 4]);
    let (mut p1, mut p2) = ([0.0; 4], [0.0; 4]);
    for off in window(d, -1, 1) {
        let pp = point_at(d, cell, off, p.randomness);
        let dd = vdist_nd(d, pp, local, p);
        if dd < d1 {
            d2 = d1;
            d1 = dd;
            o2 = o1;
            o1 = off;
            p2 = p1;
            p1 = pp;
        } else if dd < d2 {
            d2 = dd;
            o2 = off;
            p2 = pp;
        }
    }
    let _ = (o1, p1);
    VorOut { distance: d2, color: vhash_color(d, vadd(d, cell, o2)), position: vpos_out(d, vadd(d, p2, cell)) }
}
fn vor_smooth_f1(p: &VorP, coord: V4, calc_color: bool) -> VorOut {
    let d = p.dim;
    let cell = vfloor(d, coord);
    let local = vsub(d, coord, cell);
    let mut sd = 0.0f32;
    let mut sc = [0.0f32; 3];
    let mut sp = [0.0f32; 4];
    let mut h = -1.0f32;
    for off in window(d, -2, 2) {
        let pp = point_at(d, cell, off, p.randomness);
        let dd = vdist_nd(d, pp, local, p);
        h = if h == -1.0 { 1.0 } else { smooth01(0.5 + 0.5 * (sd - dd) / p.smoothness) };
        let mut corr = p.smoothness * h * (1.0 - h);
        sd = ((1.0 - h) * sd + h * dd) - corr;
        corr /= 1.0 + 3.0 * p.smoothness;
        if calc_color {
            let cc = vhash_color(d, vadd(d, cell, off));
            for q in 0..3 {
                sc[q] = ((1.0 - h) * sc[q] + h * cc[q]) - corr;
            }
        }
        for q in 0..d {
            sp[q] = ((1.0 - h) * sp[q] + h * pp[q]) - corr;
        }
    }
    VorOut { distance: sd, color: sc, position: vpos_out(d, vadd(d, cell, sp)) }
}
fn vor_dist_to_edge(p: &VorP, coord: V4) -> f32 {
    let d = p.dim;
    let cell = vfloor(d, coord);
    let local = vsub(d, coord, cell);
    if d == 1 {
        let mid = hf1(cell[0]) * p.randomness;
        let left = -1.0 + hf1(cell[0] - 1.0) * p.randomness;
        let right = 1.0 + hf1(cell[0] + 1.0) * p.randomness;
        let dl = ((mid + left) / 2.0 - local[0]).abs();
        let dr = ((mid + right) / 2.0 - local[0]).abs();
        return dl.min(dr);
    }
    let dot = |a: V4, b: V4| {
        let mut s = a[0] * b[0];
        for i in 1..d {
            s += a[i] * b[i];
        }
        s
    };
    let mut closest = [0.0; 4];
    let mut md = f32::MAX;
    let offs = window(d, -1, 1);
    for off in &offs {
        let pp = point_at(d, cell, *off, p.randomness);
        let v = vsub(d, pp, local);
        let dd = dot(v, v);
        if dd < md {
            md = dd;
            closest = v;
        }
    }
    md = f32::MAX;
    for off in &offs {
        let pp = point_at(d, cell, *off, p.randomness);
        let v = vsub(d, pp, local);
        let perp = vsub(d, v, closest);
        if dot(perp, perp) > 0.0001 {
            let len = dot(perp, perp).sqrt();
            let mut nrm = [0.0; 4];
            for i in 0..d {
                nrm[i] = perp[i] / len;
            }
            let mut mid = [0.0; 4];
            for i in 0..d {
                mid[i] = (closest[i] + v[i]) / 2.0;
            }
            md = md.min(dot(mid, nrm));
        }
    }
    md
}
pub fn vor_n_sphere(p: &VorP, coord: V4) -> f32 {
    let d = p.dim;
    let cell = vfloor(d, coord);
    let local = vsub(d, coord, cell);
    let offs = window(d, -1, 1);
    let mut closest = [0.0; 4];
    let mut closest_off = [0.0; 4];
    let mut md = f32::MAX;
    for off in &offs {
        let pp = point_at(d, cell, *off, p.randomness);
        let dd = if d == 1 { (pp[0] - local[0]).abs() } else { euclid(d, pp, local) };
        if dd < md {
            md = dd;
            closest = pp;
            closest_off = *off;
        }
    }
    md = f32::MAX;
    let mut cc = [0.0; 4];
    for off in &offs {
        if (0..d).all(|i| off[i] == 0.0) {
            continue;
        }
        let o2 = vadd(d, *off, closest_off);
        let pp = point_at(d, cell, o2, p.randomness);
        let dd = if d == 1 { (closest[0] - pp[0]).abs() } else { euclid(d, closest, pp) };
        if dd < md {
            md = dd;
            cc = pp;
        }
    }
    if d == 1 {
        (cc[0] - closest[0]).abs() / 2.0
    } else {
        euclid(d, cc, closest) / 2.0
    }
}

/// fractal_voronoi_x_fx (F1/F2/Smooth F1)
pub fn vor_fractal(p: &VorP, coord: V4, calc_color: bool) -> VorOut {
    let d = p.dim;
    let mut amplitude = 1.0f32;
    let mut max_amp = 0.0f32;
    let mut scale = 1.0f32;
    let mut out = VorOut::default();
    let zero_input = p.detail == 0.0 || p.roughness == 0.0;
    let sc = |c: V4, s: f32| -> V4 {
        let mut o = [0.0; 4];
        for i in 0..d {
            o[i] = c[i] * s;
        }
        o
    };
    let n = p.detail.ceil() as i32;
    for i in 0..=n {
        let cs = sc(coord, scale);
        let oct = if p.feature == 1 {
            vor_f2(p, cs)
        } else if p.feature == 2 && p.smoothness != 0.0 {
            vor_smooth_f1(p, cs, calc_color)
        } else {
            vor_f1(p, cs)
        };
        // position components beyond `dim` are zero except 1D w, which lives in component 3
        let comps: [usize; 4] = if d == 1 { [3, 3, 3, 3] } else { [0, 1, 2, 3] };
        let ncomp = if d == 1 { 1 } else { d };
        if zero_input {
            max_amp = 1.0;
            out = oct;
            break;
        }
        if (i as f32) <= p.detail {
            max_amp += amplitude;
            out.distance += oct.distance * amplitude;
            for q in 0..3 {
                out.color[q] += oct.color[q] * amplitude;
            }
            for q in 0..ncomp {
                let c = comps[q];
                out.position[c] = mixf(out.position[c], oct.position[c] / scale, amplitude);
            }
            scale *= p.lacunarity;
            amplitude *= p.roughness;
        } else {
            let rem = p.detail - p.detail.floor();
            if rem != 0.0 {
                max_amp = mixf(max_amp, max_amp + amplitude, rem);
                out.distance = mixf(out.distance, out.distance + oct.distance * amplitude, rem);
                for q in 0..3 {
                    out.color[q] = mixf(out.color[q], out.color[q] + oct.color[q] * amplitude, rem);
                }
                for q in 0..ncomp {
                    let c = comps[q];
                    out.position[c] = mixf(out.position[c], mixf(out.position[c], oct.position[c] / scale, amplitude), rem);
                }
            }
        }
    }
    if p.normalize {
        out.distance /= max_amp * p.max_distance;
        for q in 0..3 {
            out.color[q] /= max_amp;
        }
    }
    if p.scale != 0.0 {
        for q in 0..4 {
            out.position[q] /= p.scale;
        }
    } else {
        out.position = [0.0; 4];
    }
    out
}

pub fn vor_fractal_dist_to_edge(p: &VorP, coord: V4) -> f32 {
    let d = p.dim;
    let mut amplitude = 1.0f32;
    let mut max_amp = p.max_distance;
    let mut scale = 1.0f32;
    let mut distance = 8.0f32;
    let zero_input = p.detail == 0.0 || p.roughness == 0.0;
    let n = p.detail.ceil() as i32;
    for i in 0..=n {
        let mut cs = [0.0; 4];
        for q in 0..d {
            cs[q] = coord[q] * scale;
        }
        let od = vor_dist_to_edge(p, cs);
        if zero_input {
            distance = od;
            break;
        }
        if (i as f32) <= p.detail {
            max_amp = mixf(max_amp, p.max_distance / scale, amplitude);
            distance = mixf(distance, distance.min(od / scale), amplitude);
            scale *= p.lacunarity;
            amplitude *= p.roughness;
        } else {
            let rem = p.detail - p.detail.floor();
            if rem != 0.0 {
                let la = mixf(max_amp, p.max_distance / scale, amplitude);
                max_amp = mixf(max_amp, la, rem);
                let ld = mixf(distance, distance.min(od / scale), amplitude);
                distance = mixf(distance, distance.min(ld), rem);
            }
        }
    }
    if p.normalize {
        distance /= max_amp;
    }
    distance
}
