//! tableware/cup.py CupFactory (body, wrap, handle: torus -> TAPER x2 -> exact BOOLEAN difference -> keep the outer loose part -> SUBSURF 1 -> join)
use super::bevel::{bevel, Offset};
use super::blops::*;
use super::matdraw;
use super::subsurf::subsurf;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "CupFactory"
}

pub struct Cup {
    x_end: f64,
    straight: bool,
    x_lowest: f64,
    depth: f64,
    has_guard: bool,
    thickness: f64,
    handle_location: f64,
    handle_radius: f64,
    handle_inner: f64,
    taper_x: f64,
    taper_y: f64,
    x_lower_ratio: f64,
    wrap_margin: f64,
    scale: f64,
    fs: u64,
}

impl Cup {
    fn new(fs: u64) -> Cup {
        let mut r = Rng::seeded(fs);
        let x_end = 0.25;
        let is_short = r.uniform(0.0, 1.0) < 0.5;
        let (straight, x_lowest, depth, has_guard);
        if is_short {
            straight = r.uniform(0.0, 1.0) < 0.2;
            x_lowest = r.log_uniform(0.6, 0.9);
            depth = r.log_uniform(0.25, 0.5);
            has_guard = r.uniform(0.0, 1.0) < 0.8;
        } else {
            straight = true;
            x_lowest = r.log_uniform(0.9, 1.0);
            depth = r.log_uniform(0.5, 1.0);
            has_guard = false;
        }
        let handle_location = if straight { r.uniform(0.45, 0.65) } else { r.uniform(-0.1, 0.3) };
        let _handle_type = r.uniform(0.0, 1.0) < 0.5;
        let handle_radius = depth * r.uniform(0.2, 0.4);
        let inner = handle_radius * r.log_uniform(0.2, 0.3);
        let tx = r.uniform(0.0, 2.0);
        let ty = r.uniform(0.0, 2.0);
        let x_lower_ratio = r.log_uniform(0.8, 1.0);
        let thickness = r.log_uniform(0.01, 0.04);
        let _has_wrap = r.uniform(0.0, 1.0) < 0.3;
        let wrap_margin = r.uniform(0.1, 0.2);
        matdraw::sample_call(&mut r, matdraw::GRAPHICDESIGN);
        let _has_inside = r.uniform(0.0, 1.0) < 0.5;
        let scale = r.log_uniform(0.15, 0.3);
        let mut c = Cup { x_end, straight, x_lowest, depth, has_guard, thickness, handle_location, handle_radius, handle_inner: inner, taper_x: tx, taper_y: ty, x_lower_ratio, wrap_margin, scale, fs };
        // built to the layout box [width (y), depth (x), height]: the scale gives the diameter (y), the depth the height; the handle
        // (x side) follows the depth. The x extent is then what the handle makes it.
        if let Some(h) = crate::api::size_hint() {
            for _ in 0..3 {
                let (lo, hi) = c.outline(0);
                c.scale *= h[0] / (hi[1] - lo[1]);
                let (lo, hi) = c.outline(0);
                let zf = h[2] / (hi[2] - lo[2]);
                c.depth *= zf;
                c.handle_radius *= zf;
                c.handle_inner *= zf;
            }
        }
        c
    }
}

impl Factory for Cup {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let sc = self.scale;
        let (xs, zs): (Vec<f64>, Vec<f64>) = if self.straight {
            (vec![0.0, self.x_lowest * self.x_end, self.x_end], vec![0.0, 0.0, self.depth])
        } else {
            (
                vec![0.0, self.x_lowest * self.x_end, (self.x_lowest + self.x_lower_ratio * (1.0 - self.x_lowest)) * self.x_end, self.x_end],
                vec![0.0, 0.0, self.depth * 0.5, self.depth],
            )
        };
        let anchors: Vec<[f64; 3]> = xs.iter().zip(zs.iter()).map(|(&x, &z)| [x * sc, 0.0, z * sc]).collect();
        let mut m = spin_anchors(&anchors, &[1]);
        let inv = (1.0 / sc) as f32;
        for p in m.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * inv) as f64;
            }
        }
        let pct = r.uniform(10.0, 50.0);
        let m = bevel(&m, Offset::Percent(pct), 8, 30f64.to_radians()).unwrap_or(m);
        // make_wrap: clone, drop vertices (margins / angular window), scale x,y by 1.01
        let thr = r.uniform(0.0, 0.1);
        let keep: Vec<bool> = m
            .pos
            .iter()
            .map(|p| {
                let (x, y, z) = (p[0], p[1], p[2]);
                let bad = (z / self.depth < self.wrap_margin) || (z / self.depth > 1.0 - self.wrap_margin + thr) || (y.atan2(x).abs() < std::f64::consts::PI * self.wrap_margin);
                !bad
            })
            .collect();
        let mut wrap = Mesh::new();
        let mut map = vec![u32::MAX; m.pos.len()];
        for (k, p) in m.pos.iter().enumerate() {
            if keep[k] {
                map[k] = wrap.pos.len() as u32;
                wrap.pos.push([(p[0] as f32 * 1.01f32) as f64, (p[1] as f32 * 1.01f32) as f64, p[2]]);
            }
        }
        for f in &m.faces {
            if f.iter().all(|&v| keep[v as usize]) {
                wrap.faces.push(f.iter().map(|&v| map[v as usize]).collect());
            }
        }
        let s0 = books::solidify_simple(&m, -(self.thickness as f32 as f64), None);
        let s = subsurf(&s0, if crate::dense() { 1 } else { 2 }, false);
        // handle_angle is drawn even without a guard
        let (xl, zl) = (xs[xs.len() - 1] - xs[xs.len() - 2], zs[zs.len() - 1] - zs[zs.len() - 2]);
        let angle_low = (xl / zl).atan();
        let angle_high = ((xs[2] - xs[1]) / (zs[2] - zs[1])).atan();
        let handle_angle = r.uniform(angle_low, angle_high + 1e-3);
        let mut all = Mesh::new();
        all.append(&s);
        if self.has_guard {
            let hl = self.handle_location;
            let n = xs.len();
            let loc = [xs[n - 2] * (1.0 - hl) + xs[n - 1] * hl, 0.0, zs[n - 2] * (1.0 - hl) + zs[n - 1] * hl];
            // dense mode: cut the handle against the level-1 cup (4x fewer faces in the exact boolean; the junction differs by < 1 mm)
            let h = if crate::dense() { self.make_handle(&subsurf(&s0, 1, false), loc, handle_angle) } else { self.make_handle(&s, loc, handle_angle) };
            all.append(&h);
        }
        all.append(&wrap);
        let sc32 = sc as f32;
        for p in all.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * sc32) as f64;
            }
        }
        let mat = spec_material("cup", json!({"cls": "ceramic.Ceramic", "kwargs": {}, "seed": self.fs}), [0.85, 0.85, 0.82]);
        let mut a = Asset::default();
        let b = all.bounds();
        a.mesh(all, &mat, Some(b));
        a
    }

    /// collision: the cup's outline (the bevelled profile, the wrap, the handle ring outside the wall), grown by the wall
    fn volumes(&self, i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = self.outline(i);
        let g = self.thickness * self.scale + 0.003;
        Some(vec![crate::piece::Volume::hard([lo[0] - g, lo[1] - g, lo[2] - g], [hi[0] + g, hi[1] + g, hi[2] + g])])
    }
    fn natural_size(&self, i: u64) -> Option<[f64; 3]> {
        let (lo, hi) = self.outline(i);
        Some([hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]])
    }
    /// dense mode: the solver only needs the outline, so skip solidify / subsurf / the exact handle boolean:
    /// body = bevelled revolve (+ the 1.01 wrap), handle = the torus vertices that lie outside the cup on its +x side
    fn bounds_hint(&self, i: u64) -> Option<(V3, V3)> {
        if !crate::dense() {
            return None;
        }
        Some(self.outline(i))
    }
}

impl Cup {
    /// bounding box of the cup, handle ring included (the instance's bevel draws taken for instance `i`)
    fn outline(&self, i: u64) -> (V3, V3) {
        let mut r = inst_rng(self.fs, i);
        let sc = self.scale;
        let (xs, zs): (Vec<f64>, Vec<f64>) = if self.straight {
            (vec![0.0, self.x_lowest * self.x_end, self.x_end], vec![0.0, 0.0, self.depth])
        } else {
            (
                vec![0.0, self.x_lowest * self.x_end, (self.x_lowest + self.x_lower_ratio * (1.0 - self.x_lowest)) * self.x_end, self.x_end],
                vec![0.0, 0.0, self.depth * 0.5, self.depth],
            )
        };
        let anchors: Vec<[f64; 3]> = xs.iter().zip(zs.iter()).map(|(&x, &z)| [x * sc, 0.0, z * sc]).collect();
        let mut m = spin_anchors(&anchors, &[1]);
        let inv = (1.0 / sc) as f32;
        for p in m.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 * inv) as f64;
            }
        }
        let pct = r.uniform(10.0, 50.0);
        let m = bevel(&m, Offset::Percent(pct), 8, 30f64.to_radians()).unwrap_or(m);
        let thr = r.uniform(0.0, 0.1);
        let (mut lo, mut hi) = m.bounds();
        for p in &m.pos {
            let (x, y, z) = (p[0], p[1], p[2]);
            let bad = (z / self.depth < self.wrap_margin) || (z / self.depth > 1.0 - self.wrap_margin + thr) || (y.atan2(x).abs() < std::f64::consts::PI * self.wrap_margin);
            if !bad {
                for (k, v) in [x * 1.01, y * 1.01, z].into_iter().enumerate() {
                    lo[k] = lo[k].min(v);
                    hi[k] = hi[k].max(v);
                }
            }
        }
        let (xl, zl) = (xs[xs.len() - 1] - xs[xs.len() - 2], zs[zs.len() - 1] - zs[zs.len() - 2]);
        let angle_low = (xl / zl).atan();
        let angle_high = ((xs[2] - xs[1]) / (zs[2] - zs[1])).atan();
        let handle_angle = r.uniform(angle_low, angle_high + 1e-3);
        if self.has_guard {
            let hl = self.handle_location;
            let n = xs.len();
            let loc = [xs[n - 2] * (1.0 - hl) + xs[n - 1] * hl, 0.0, zs[n - 2] * (1.0 - hl) + zs[n - 1] * hl];
            let (t, w) = self.handle_ring(loc, handle_angle);
            // outer wall radius at height z
            let radius_at = |z: f64| -> f64 {
                for k in 1..xs.len() {
                    if z <= zs[k] && zs[k] > zs[k - 1] {
                        let f = ((z - zs[k - 1]) / (zs[k] - zs[k - 1])).clamp(0.0, 1.0);
                        return xs[k - 1] + f * (xs[k] - xs[k - 1]);
                    }
                }
                xs[xs.len() - 1]
            };
            for p in &t.pos {
                let q = mat_mul_v(&w, [p[0] as f32, p[1] as f32, p[2] as f32]);
                let (x, y, z) = (q[0] as f64, q[1] as f64, q[2] as f64);
                let outside = z > self.depth || z < 0.0 || x.hypot(y) > radius_at(z);
                if x > 0.0 && outside {
                    for (k, v) in [x, y, z].into_iter().enumerate() {
                        lo[k] = lo[k].min(v);
                        hi[k] = hi[k].max(v);
                    }
                }
            }
        }
        let sc32 = sc as f32;
        let f = |v: f64| (v as f32 * sc32) as f64;
        ([f(lo[0]), f(lo[1]), f(lo[2])], [f(hi[0]), f(hi[1]), f(hi[2])])
    }
}

fn mat_mul_v(m: &crate::bl32::M4, v: [f32; 3]) -> [f32; 3] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0] + m[3][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1] + m[3][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2] + m[3][2],
    ]
}

/// bpy_operators add_torus (48 x 12): mathutils float32 rotation matrix times a float32 vector (products in float, sum in double)
fn torus(major: f64, minor: f64) -> Mesh {
    torus_seg(major, minor, 48, 12)
}

fn torus_seg(major: f64, minor: f64, maj_seg: usize, min_seg: usize) -> Mesh {
    let pi2 = std::f64::consts::PI * 2.0;
    let mut m = Mesh::new();
    let tot = maj_seg * min_seg;
    let mut i1 = 0usize;
    for mi in 0..maj_seg {
        let a = ((mi as f64 / maj_seg as f64) * pi2) as f32;
        let (c, s) = (a.cos(), a.sin());
        // Matrix.Rotation(angle, 3, 'Z'): [[c, -s, 0], [s, c, 0], [0, 0, 1]]
        for ni in 0..min_seg {
            let ang = pi2 * ni as f64 / min_seg as f64;
            let v = [(major + ang.cos() * minor) as f32, 0.0f32, (ang.sin() * minor) as f32];
            let row = |r: [f32; 3]| ((r[0] * v[0]) as f64 + (r[1] * v[1]) as f64 + (r[2] * v[2]) as f64) as f32;
            let p = [row([c, -s, 0.0]), row([s, c, 0.0]), row([0.0, 0.0, 1.0])];
            m.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
            let (i2, i3, i4);
            if ni + 1 == min_seg {
                i2 = mi * min_seg;
                i3 = i1 + min_seg;
                i4 = i2 + min_seg;
            } else {
                i2 = i1 + 1;
                i3 = i1 + min_seg;
                i4 = i3 + 1;
            }
            let w = |x: usize| (if x >= tot { x - tot } else { x }) as u32;
            m.faces.push(vec![i1 as u32, w(i3), w(i4), w(i2)]);
            i1 += 1;
        }
    }
    m
}

impl Cup {
    /// the tapered handle torus (object space) and its world matrix
    fn handle_ring(&self, loc: [f64; 3], angle: f64) -> (Mesh, crate::bl32::M4) {
        use crate::bl32::Obj32;
        let mut t = if crate::dense() { torus_seg(self.handle_radius, self.handle_inner, 32, 8) } else { torus(self.handle_radius, self.handle_inner) };
        let ob = Obj32::new([loc[0] as f32, loc[1] as f32, loc[2] as f32], [std::f64::consts::FRAC_PI_2 as f32, angle as f32, 0.0]);
        let w = ob.matrix_world();
        // two SIMPLE_DEFORM TAPER modifiers in object space
        let mut v: Vec<[f32; 3]> = t.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        super::bpymod::simple_deform(&mut v, super::bpymod::Deform::Taper, self.taper_x, 0);
        super::bpymod::simple_deform(&mut v, super::bpymod::Deform::Taper, self.taper_y, 1);
        t.pos = v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        (t, w)
    }

    /// add_handle: returns the selected loose part in the cup's space
    fn make_handle(&self, cup: &Mesh, loc: [f64; 3], angle: f64) -> Mesh {
        let (t, w) = self.handle_ring(loc, angle);
        // operand (cup) into the torus' object space: inverse of the (rigid) world matrix
        let rt = |i: usize, j: usize| w[j][i];
        let mut inv = [[0.0f32; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                inv[i][j] = rt(i, j);
            }
        }
        for j in 0..3 {
            inv[3][j] = -(w[3][0] * inv[0][j] + w[3][1] * inv[1][j] + w[3][2] * inv[2][j]);
        }
        inv[3][3] = 1.0;
        let mut c2 = cup.clone();
        for p in c2.pos.iter_mut() {
            let q = mat_mul_v(&inv, [p[0] as f32, p[1] as f32, p[2] as f32]);
            *p = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
        let d = super::bathkit::boolean_exact("DIFFERENCE", &t, &c2);
        // separate loose parts, keep the one with the largest local x
        let nv = d.pos.len();
        let mut parent: Vec<usize> = (0..nv).collect();
        fn find(p: &mut Vec<usize>, x: usize) -> usize {
            let mut r = x;
            while p[r] != r {
                r = p[r];
            }
            let mut y = x;
            while p[y] != r {
                let nx = p[y];
                p[y] = r;
                y = nx;
            }
            r
        }
        for f in &d.faces {
            for k in 1..f.len() {
                let (a, b) = (find(&mut parent, f[0] as usize), find(&mut parent, f[k] as usize));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        let mut used = vec![false; nv];
        for f in &d.faces {
            for &v in f {
                used[v as usize] = true;
            }
        }
        // parts in order of their lowest vertex; argmax of max x (first on ties)
        let mut best: Option<(usize, f64)> = None;
        let mut roots: Vec<usize> = vec![];
        for i in 0..nv {
            if used[i] {
                let r = find(&mut parent, i);
                if !roots.contains(&r) {
                    roots.push(r);
                }
            }
        }
        for &r in &roots {
            let mx = (0..nv).filter(|&i| used[i] && find(&mut parent, i) == r).map(|i| d.pos[i][0] as f32 as f64).fold(f64::MIN, f64::max);
            if best.map_or(true, |(_, b)| mx > b) {
                best = Some((r, mx));
            }
        }
        let r = best.unwrap().0;
        let mut part = Mesh::new();
        let mut map = vec![u32::MAX; nv];
        for i in 0..nv {
            if used[i] && find(&mut parent, i) == r {
                map[i] = part.pos.len() as u32;
                part.pos.push(d.pos[i]);
            }
        }
        for f in &d.faces {
            if map[f[0] as usize] != u32::MAX {
                part.faces.push(f.iter().map(|&v| map[v as usize]).collect());
            }
        }
        let mut part = subsurf(&part, 1, false);
        for p in part.pos.iter_mut() {
            let q = mat_mul_v(&w, [p[0] as f32, p[1] as f32, p[2] as f32]);
            *p = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
        part
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Cup::new(fs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// dense bounds hint vs the full asset (boolean handle): max bbox error over many seeds
    #[test]
    fn cup_asset_dense_vs_exact_timing() {
        std::env::set_var("INFINIGEN_DENSE", "1");
        let t = web_time::Instant::now();
        let mut tris = 0;
        for fs in 0..12u64 {
            let a = Cup::new(fs).asset(7);
            tris += a.parts.iter().map(|p| p.mesh.faces.len()).sum::<usize>();
        }
        eprintln!("cup asset x12 (dense): {:?}, {} faces", t.elapsed(), tris);
    }
    #[test]
    fn cup_hint_matches_asset() {
        std::env::set_var("INFINIGEN_DENSE", "1");
        let (mut worst, mut sum, mut n) = (0.0f64, 0.0f64, 0);
        let t_full = web_time::Instant::now();
        let mut full = vec![];
        for fs in 0..40u64 {
            let c = Cup::new(fs);
            full.push(c.asset(7).bounds());
        }
        let tf = t_full.elapsed();
        let t_h = web_time::Instant::now();
        for fs in 0..40u64 {
            let c = Cup::new(fs);
            let (lo, hi) = c.bounds_hint(7).unwrap();
            let (flo, fhi) = full[fs as usize];
            let e = (0..3).map(|k| (lo[k] - flo[k]).abs().max((hi[k] - fhi[k]).abs())).fold(0.0, f64::max);
            worst = worst.max(e);
            sum += e;
            n += 1;
            if e > 0.01 {
                eprintln!("fs {fs} err {e:.4} full {flo:?} {fhi:?} hint {lo:?} {hi:?} guard {}", c.has_guard);
            }
        }
        eprintln!("cup hint: worst {worst:.4} mean {:.4}; full {:?} hint {:?}", sum / n as f64, tf, t_h.elapsed());
        assert!(worst < 0.02);
    }
}
