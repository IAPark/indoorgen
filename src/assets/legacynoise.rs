include!("legacynoise_tables.rs");

fn h(i: i32) -> usize {
    HASH[(i as usize) & 511] as usize
}

#[inline]
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}
fn npfade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
fn grad(hv: usize, x: f32, y: f32, z: f32) -> f32 {
    let hh = hv & 15;
    let u = if hh < 8 { x } else { y };
    let v = if hh < 4 { y } else if hh == 12 || hh == 14 { x } else { z };
    (if hh & 1 == 0 { u } else { -u }) + (if hh & 2 == 0 { v } else { -v })
}

pub fn new_perlin(x: f32, y: f32, z: f32) -> f32 {
    let (mut x, mut y, mut z) = (x, y, z);
    let (mut u, mut v, mut w) = (x.floor(), y.floor(), z.floor());
    let xi = (u as i32) & 255;
    let yi = (v as i32) & 255;
    let zi = (w as i32) & 255;
    x -= u;
    y -= v;
    z -= w;
    u = npfade(x);
    v = npfade(y);
    w = npfade(z);
    let a = HASH[xi as usize] as i32 + yi;
    let aa = h(a) as i32 + zi;
    let ab = h(a + 1) as i32 + zi;
    let b = HASH[(xi + 1) as usize] as i32 + yi;
    let ba = h(b) as i32 + zi;
    let bb = h(b + 1) as i32 + zi;
    lerp(
        w,
        lerp(
            v,
            lerp(u, grad(h(aa), x, y, z), grad(h(ba), x - 1.0, y, z)),
            lerp(u, grad(h(ab), x, y - 1.0, z), grad(h(bb), x - 1.0, y - 1.0, z)),
        ),
        lerp(
            v,
            lerp(u, grad(h(aa + 1), x, y, z - 1.0), grad(h(ba + 1), x - 1.0, y, z - 1.0)),
            lerp(u, grad(h(ab + 1), x, y - 1.0, z - 1.0), grad(h(bb + 1), x - 1.0, y - 1.0, z - 1.0)),
        ),
    )
}

pub fn new_perlin_u(x: f32, y: f32, z: f32) -> f32 {
    0.5 + 0.5 * new_perlin(x, y, z)
}

pub fn org_blender_noise(x: f32, y: f32, z: f32) -> f32 {
    let mut n = 0.5f32;
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (ox, oy, oz) = (x - fx, y - fy, z - fz);
    let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
    let (jx, jy, jz) = (ox - 1.0, oy - 1.0, oz - 1.0);
    let (mut cn1, mut cn2, mut cn3) = (ox * ox, oy * oy, oz * oz);
    let (mut cn4, mut cn5, mut cn6) = (jx * jx, jy * jy, jz * jz);
    cn1 = 1.0 - 3.0 * cn1 + 2.0 * cn1 * ox;
    cn2 = 1.0 - 3.0 * cn2 + 2.0 * cn2 * oy;
    cn3 = 1.0 - 3.0 * cn3 + 2.0 * cn3 * oz;
    cn4 = 1.0 - 3.0 * cn4 - 2.0 * cn4 * jx;
    cn5 = 1.0 - 3.0 * cn5 - 2.0 * cn5 * jy;
    cn6 = 1.0 - 3.0 * cn6 - 2.0 * cn6 * jz;
    let hs = |i: i32| HASH[(i as usize) & 511] as i32;
    let b00 = hs(hs(ix & 255) + (iy & 255));
    let b10 = hs(hs((ix + 1) & 255) + (iy & 255));
    let b01 = hs(hs(ix & 255) + ((iy + 1) & 255));
    let b11 = hs(hs((ix + 1) & 255) + ((iy + 1) & 255));
    let b20 = iz & 255;
    let b21 = (iz + 1) & 255;
    let hv = |idx: i32| -> [f32; 3] {
        let o = 3 * (hs(idx) as usize);
        [HASHVECTF[o], HASHVECTF[o + 1], HASHVECTF[o + 2]]
    };
    let mut add = |i: f32, hh: [f32; 3], a: f32, b: f32, c: f32| {
        n += i * (hh[0] * a + hh[1] * b + hh[2] * c);
    };
    add(cn1 * cn2 * cn3, hv(b20 + b00), ox, oy, oz);
    add(cn1 * cn2 * cn6, hv(b21 + b00), ox, oy, jz);
    add(cn1 * cn5 * cn3, hv(b20 + b01), ox, jy, oz);
    add(cn1 * cn5 * cn6, hv(b21 + b01), ox, jy, jz);
    add(cn4 * cn2 * cn3, hv(b20 + b10), jx, oy, oz);
    add(cn4 * cn2 * cn6, hv(b21 + b10), jx, oy, jz);
    add(cn4 * cn5 * cn3, hv(b20 + b11), jx, jy, oz);
    add(cn4 * cn5 * cn6, hv(b21 + b11), jx, jy, jz);
    n.clamp(0.0, 1.0)
}

fn basis_fn(basis: i32) -> (fn(f32, f32, f32) -> f32, f32) {
    match basis {
        2 => (new_perlin_u, 0.0),
        0 => (org_blender_noise, 1.0),
        _ => panic!("legacy noise basis {} not ported", basis),
    }
}

/// BLI_noise_generic_noise
pub fn generic_noise(noisesize: f32, x: f32, y: f32, z: f32, hard: bool, basis: i32) -> f32 {
    let (f, add) = basis_fn(basis);
    let (mut x, mut y, mut z) = (x + add, y + add, z + add);
    if noisesize != 0.0 {
        let ns = 1.0 / noisesize;
        x *= ns;
        y *= ns;
        z *= ns;
    }
    if hard {
        return (2.0 * f(x, y, z) - 1.0).abs();
    }
    f(x, y, z)
}

/// BLI_noise_generic_turbulence
pub fn generic_turbulence(noisesize: f32, x: f32, y: f32, z: f32, oct: i32, hard: bool, basis: i32) -> f32 {
    let (f, add) = basis_fn(basis);
    let (mut x, mut y, mut z) = (x + add, y + add, z + add);
    if noisesize != 0.0 {
        let ns = 1.0 / noisesize;
        x *= ns;
        y *= ns;
        z *= ns;
    }
    let (mut sum, mut amp, mut fscale) = (0.0f32, 1.0f32, 1.0f32);
    for _ in 0..=oct {
        let mut t = f(fscale * x, fscale * y, fscale * z);
        if hard {
            t = (2.0 * t - 1.0).abs();
        }
        sum += t * amp;
        amp *= 0.5;
        fscale *= 2.0;
    }
    sum * ((1i32 << oct) as f32 / ((1i32 << (oct + 1)) - 1) as f32)
}
