//! table_decorations/book.py: BookFactory, BookStackFactory, BookColumnFactory (RNG- and float32-faithful port)
use super::textdraw::{plaster_draws, text_ctor_and_generate};
use super::*;
use crate::bl32;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "BookFactory" | "BookStackFactory" | "BookColumnFactory")
}

#[inline]
fn r32(x: f64) -> f64 {
    x as f32 as f64
}

// ---------------------------------------------------------------- float32 object helpers

/// bpy new_cube() vertex / face layout, scaled by `scale` and moved by `loc` (apply_transform, float32)
fn cube32(loc: V3, scale: V3) -> Mesh {
    let mut m = Mesh::new();
    for i in 0..8 {
        let s = [if i & 4 != 0 { 1.0f32 } else { -1.0 }, if i & 2 != 0 { 1.0f32 } else { -1.0 }, if i & 1 != 0 { 1.0f32 } else { -1.0 }];
        let p: V3 = [0, 1, 2].map(|k| (scale[k] as f32 * s[k] + loc[k] as f32) as f64);
        m.pos.push(p);
    }
    m.faces = vec![vec![0, 1, 3, 2], vec![2, 3, 7, 6], vec![6, 7, 5, 4], vec![4, 5, 1, 0], vec![2, 6, 4, 0], vec![7, 3, 1, 5]];
    m
}

/// utils.object.new_bbox
pub fn new_bbox(x: f64, x_: f64, y: f64, y_: f64, z: f64, z_: f64) -> Mesh {
    cube32([(x + x_) / 2.0, (y + y_) / 2.0, (z + z_) / 2.0], [(x_ - x) / 2.0, (y_ - y) / 2.0, (z_ - z) / 2.0])
}

fn bounds32(ms: &[&Mesh]) -> (V3, V3) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for m in ms {
        for p in &m.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    (lo, hi)
}

/// obj.location = loc ; apply_transform(obj, True) (float32 add)
fn apply_loc(ms: &mut [&mut Mesh], loc: V3) {
    for m in ms.iter_mut() {
        for p in m.pos.iter_mut() {
            for k in 0..3 {
                p[k] = (p[k] as f32 + loc[k] as f32) as f64;
            }
        }
    }
}

/// apply_transform with a rotation (and optional location), Blender XYZ euler, float32
fn apply_rot(ms: &mut [&mut Mesh], euler: V3, loc: V3) {
    let e = [euler[0] as f32, euler[1] as f32, euler[2] as f32];
    let m = bl32::eul_to_mat3(e);
    let l = [loc[0] as f32, loc[1] as f32, loc[2] as f32];
    for mesh in ms.iter_mut() {
        for p in mesh.pos.iter_mut() {
            let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
            for i in 0..3 {
                p[i] = (m[0][i] * x + m[1][i] * y + m[2][i] * z + l[i]) as f64;
            }
        }
    }
}

fn triangles_of(ms: &[&Mesh]) -> Vec<[V3; 3]> {
    let mut out = vec![];
    for m in ms {
        for f in &m.faces {
            let v: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
            if v.len() == 3 {
                out.push([v[0], v[1], v[2]]);
            } else if v.len() == 4 {
                let d = |a: V3, b: V3| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
                if d(v[0], v[2]) <= d(v[1], v[3]) {
                    out.push([v[0], v[1], v[2]]);
                    out.push([v[0], v[2], v[3]]);
                } else {
                    out.push([v[0], v[1], v[3]]);
                    out.push([v[1], v[2], v[3]]);
                }
            } else {
                for k in 1..v.len() - 1 {
                    out.push([v[0], v[k], v[k + 1]]);
                }
            }
        }
    }
    out
}

fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// min over `origins` of the nearest hit (> 1e-5) of the ray origin + t*dir with the mesh; trimesh.proximity.longest_ray (ray_triangle backend)
fn longest_ray_min(tris: &[[V3; 3]], origins: &[V3], dir: V3) -> f64 {
    struct T {
        v0: V3,
        e0: V3,
        e1: V3,
        n: V3,
        d00: f64,
        d01: f64,
        d11: f64,
    }
    let ts: Vec<T> = tris
        .iter()
        .filter_map(|t| {
            let e0 = sub(t[1], t[0]);
            let e1 = sub(t[2], t[0]);
            let c = cross(e0, e1);
            let l = dot3(c, c).sqrt();
            if l == 0.0 {
                return None;
            }
            let n = [c[0] / l, c[1] / l, c[2] / l];
            Some(T { v0: t[0], e0, e1, n, d00: dot3(e0, e0), d01: dot3(e0, e1), d11: dot3(e1, e1) })
        })
        .collect();
    let mut best = f64::INFINITY;
    for &o in origins {
        for t in &ts {
            let pd = dot3(dir, t.n);
            if pd.abs() <= 1e-5 {
                continue;
            }
            let po = dot3(sub(t.v0, o), t.n);
            let dist = po / pd;
            let loc = [dir[0] * dist + o[0], dir[1] * dist + o[1], dir[2] * dist + o[2]];
            let w = sub(loc, t.v0);
            let d02 = dot3(t.e0, w);
            let d12 = dot3(t.e1, w);
            let inv = 1.0 / (t.d00 * t.d11 - t.d01 * t.d01);
            let b2 = (t.d00 * d12 - t.d01 * d02) * inv;
            let b1 = (t.d11 * d02 - t.d01 * d12) * inv;
            let b0 = 1.0 - b1 - b2;
            let tol = 1e-13;
            if !(b0 > -tol && b1 > -tol && b2 > -tol && b0 < 1.0 + tol && b1 < 1.0 + tol && b2 < 1.0 + tol) {
                continue;
            }
            let v = sub(loc, o);
            let fwd = dot3(v, dir);
            if fwd <= -1e-6 {
                continue;
            }
            let d = dot3(v, v).sqrt();
            if d > 1e-5 && d < best {
                best = d;
            }
        }
    }
    best
}

/// Blender SOLIDIFY (simple mode, offset -1): angle-weighted vertex normals, shell on the inner side, rim on open edges
pub fn solidify_simple(m: &Mesh, t: f64, edge_order: Option<&[[u32; 2]]>) -> Mesh {
    let n = m.pos.len();
    type F3 = [f32; 3];
    let pos32: Vec<F3> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let sub3 = |a: F3, b: F3| -> F3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] };
    let dot32 = |a: F3, b: F3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm32 = |a: F3| -> F3 {
        let d = dot32(a, a);
        if d > 1.0e-35f32 {
            let inv = 1.0f32 / d.sqrt();
            [a[0] * inv, a[1] * inv, a[2] * inv]
        } else {
            [0.0; 3]
        }
    };
    let normd32 = |a: F3| -> F3 {
        let d = dot32(a, a);
        if d > 1.0e-35f32 {
            let l = d.sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        } else {
            [0.0; 3]
        }
    };
    let mut nrm = vec![[0.0f32; 3]; n];
    for f in &m.faces {
        let k = f.len();
        let p: Vec<F3> = f.iter().map(|&i| pos32[i as usize]).collect();
        // normals_calc_faces: Newell's method for every face size
        let fnrm: F3 = {
            let mut nn = [0.0f32; 3];
            let mut vp = p[k - 1];
            for i in 0..k {
                let vc = p[i];
                nn[0] += (vp[1] - vc[1]) * (vp[2] + vc[2]);
                nn[1] += (vp[2] - vc[2]) * (vp[0] + vc[0]);
                nn[2] += (vp[0] - vc[0]) * (vp[1] + vc[1]);
                vp = vc;
            }
            let r = norm32(nn);
            if r == [0.0; 3] { [0.0, 0.0, 1.0] } else { r }
        };
        for i in 0..k {
            let prev = normd32(sub3(p[(i + k - 1) % k], p[i]));
            let next = normd32(sub3(p[(i + 1) % k], p[i]));
            let ang = super::legtex::safe_acos_approx(dot32(prev, next));
            for c in 0..3 {
                nrm[f[i] as usize][c] += fnrm[c] * ang;
            }
        }
    }
    let mut out = Mesh::new();
    out.pos = m.pos.clone();
    for i in 0..n {
        let u = normd32(nrm[i]);
        let tf = t as f32;
        out.pos.push([0, 1, 2].map(|c| (m.pos[i][c] as f32 - u[c] * tf) as f64));
    }
    let outward = t < 0.0;
    let flip = |f: &Vec<u32>, off: u32| -> Vec<u32> {
        let mut g = vec![f[0] + off];
        for &v in f[1..].iter().rev() {
            g.push(v + off);
        }
        g
    };
    if !outward {
        out.faces = m.faces.clone();
        for f in &m.faces {
            out.faces.push(flip(f, n as u32));
        }
    } else {
        // offset > 0: the original surface becomes the inner (flipped) shell, the displaced copy keeps the orientation
        for f in &m.faces {
            out.faces.push(flip(f, 0));
        }
        for f in &m.faces {
            out.faces.push(f.iter().map(|&v| v + n as u32).collect());
        }
    }
    let mut cnt: crate::dhash::HashMap<(u32, u32), i32> = crate::dhash::HashMap::default();
    for f in &m.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    // rim quads in edge-index order (python's mesh edge order); loop order if unknown
    let mut dirs: Vec<(u32, u32)> = vec![];
    for f in &m.faces {
        for i in 0..f.len() {
            dirs.push((f[i], f[(i + 1) % f.len()]));
        }
    }
    let ordered: Vec<(u32, u32)> = match edge_order {
        Some(eo) => eo
            .iter()
            .filter_map(|e| dirs.iter().find(|d| (d.0 == e[0] && d.1 == e[1]) || (d.0 == e[1] && d.1 == e[0])).cloned())
            .collect(),
        None => dirs.clone(),
    };
    for (a, b) in ordered {
        if cnt[&(a.min(b), a.max(b))] == 1 {
            if outward {
                out.faces.push(vec![a, b, b + n as u32, a + n as u32]);
            } else {
                out.faces.push(vec![b, a, a + n as u32, b + n as u32]);
            }
        }
    }
    out
}

/// Blender TRIANGULATE modifier (quads: shortest diagonal; first triangle replaces the face in place, the second is appended at the end)
fn triangulate(m: &Mesh) -> Mesh {
    let mut out = Mesh::new();
    out.pos = m.pos.clone();
    let mut second: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        if f.len() == 4 {
            let p = |i: usize| [m.pos[f[i] as usize][0] as f32, m.pos[f[i] as usize][1] as f32, m.pos[f[i] as usize][2] as f32];
            let d = |a: [f32; 3], b: [f32; 3]| -> f32 {
                let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
                x * x + y * y + z * z
            };
            let d02 = d(p(0), p(2));
            let d13 = d(p(1), p(3));
            if d02 < d13 {
                out.faces.push(vec![f[0], f[2], f[3]]);
                second.push(vec![f[0], f[1], f[2]]);
            } else {
                out.faces.push(vec![f[1], f[3], f[0]]);
                second.push(vec![f[1], f[2], f[3]]);
            }
        } else if f.len() == 3 {
            out.faces.push(f.clone());
        } else {
            panic!("ngon triangulate unsupported");
        }
    }
    out.faces.extend(second);
    out
}

fn join(a: &Mesh, b: &Mesh) -> Mesh {
    let mut m = a.clone();
    m.append(b);
    m
}

// ---------------------------------------------------------------- BookFactory

#[derive(Clone)]
struct BookF {
    rel_scale: f64,
    skew: f64,
    paperback: bool,
    margin: f64,
    offset: f64,
    thickness: f64,
}

const UNIT: f64 = 0.0127;

/// BookFactory.make_cover: rotate +90 degrees about x, apply, rotate -90, apply (float32 round trip noise)
fn make_cover(m: &mut Mesh) {
    apply_rot(&mut [m], [std::f64::consts::PI / 2.0, 0.0, 0.0], [0.0; 3]);
    apply_rot(&mut [m], [-std::f64::consts::PI / 2.0, 0.0, 0.0], [0.0; 3]);
}

/// edge index order of python's pre-solidify hardcover mesh (array modifier + bmesh deletes)
const HARDCOVER_EDGES: [[u32; 2]; 31] = [
    [2, 0], [0, 1], [1, 3], [3, 2], [5, 2], [4, 5], [0, 4], [1, 6], [6, 7], [7, 3], [6, 8], [8, 9], [9, 7], [8, 10], [10, 11], [11, 9],
    [10, 12], [12, 13], [13, 11], [12, 14], [14, 15], [15, 13], [14, 16], [16, 17], [17, 15], [16, 18], [18, 19], [19, 17], [19, 21], [21, 20], [20, 18],
];

impl BookF {
    /// BookFactory.__init__ (draws from the current global stream)
    fn new(r: &mut Rng) -> BookF {
        let rel_scale = r.log_uniform(1.0, 1.5);
        let skew = r.log_uniform(1.3, 1.8);
        let paperback = r.uniform(0.0, 1.0) < 0.5;
        let margin = r.uniform(0.005, 0.01);
        let offset = if r.uniform(0.0, 1.0) < 0.5 { 0.0 } else { r.log_uniform(0.002, 0.008) };
        let thickness = r.uniform(0.002, 0.003);
        text_ctor_and_generate(r);
        r.uniform(0.0, 1.0); // scratch
        r.uniform(0.0, 1.0); // edge wear
        r.uniform(0.0, 1.0); // texture_shared
        BookF { rel_scale, skew, paperback, margin, offset, thickness }
    }

    /// create_asset: (paper, cover) meshes in the book's local frame
    fn create(&self, r: &mut Rng) -> (Mesh, Mesh) {
        plaster_draws(r);
        let width = ((r.log_uniform(0.08, 0.15) * self.rel_scale / UNIT) as i64) as f64 * UNIT;
        let height = (((width * self.skew) / UNIT) as i64) as f64 * UNIT;
        let depth = r.uniform(0.01, 0.02) * self.rel_scale;
        let paper = cube32([width / 2.0, height / 2.0, depth / 2.0], [width / 2.0 - 1e-4, height / 2.0, depth / 2.0 - 1e-4]);
        let cover = if self.paperback { self.paperback_cover(width, height, depth) } else { self.hardcover(width, height, depth) };
        (paper, cover)
    }

    fn paperback_cover(&self, w: f64, h: f64, d: f64) -> Mesh {
        let mut m = cube32([w / 2.0, h / 2.0, d / 2.0], [w / 2.0, h / 2.0, d / 2.0]);
        let keep = [0usize, 4, 5];
        m.faces = keep.iter().map(|&i| m.faces[i].clone()).collect();
        make_cover(&mut m);
        m
    }

    fn hardcover(&self, w: f64, h: f64, d: f64) -> Mesh {
        let count = 8usize;
        let sx = ((w + self.margin) / 2.0) as f32;
        let sy = (h / 2.0 + self.margin) as f32;
        let sz = (d / 2.0 / count as f64) as f32;
        let ly = (-self.margin) as f32;
        let xs = [0.0f32, 2.0 * sx];
        let ys = [ly, 2.0 * sy + ly];
        let zs: Vec<f32> = (0..=count).map(|k| sz * (2 * k) as f32).collect();
        let mut m = Mesh::new();
        let mut push = |x: f32, y: f32, z: f32| m.pos.push([x as f64, y as f64, z as f64]);
        push(xs[0], ys[0], zs[0]);
        push(xs[0], ys[0], zs[1]);
        push(xs[0], ys[1], zs[0]);
        push(xs[0], ys[1], zs[1]);
        push(xs[1], ys[0], zs[0]);
        push(xs[1], ys[1], zs[0]);
        for k in 2..=count {
            push(xs[0], ys[0], zs[k]);
            push(xs[0], ys[1], zs[k]);
        }
        push(xs[1], ys[0], zs[count]);
        push(xs[1], ys[1], zs[count]);
        m.faces = vec![vec![0, 1, 3, 2], vec![2, 5, 4, 0], vec![1, 6, 7, 3]];
        for k in 2..count {
            let a = 6 + 2 * (k - 2) as u32;
            m.faces.push(vec![a, a + 2, a + 3, a + 1]);
        }
        let last = 6 + 2 * (count - 2) as u32; // z_count pair
        m.faces.push(vec![last + 3, last + 1, last, last + 2]);
        // spine bulge
        let df = d as f32;
        let off = self.offset as f32;
        for p in m.pos.iter_mut() {
            let z = p[2] as f32;
            let a = z / df;
            let b = 1.0f32 - z / df;
            let ratio = a.min(b);
            let x = p[0] as f32 - 4.0f32 * ratio * (1.0f32 - ratio) * off;
            p[0] = x as f64;
        }
        make_cover(&mut m);
        solidify_simple(&m, r32(self.thickness), Some(&HARDCOVER_EDGES))
    }
}

// ---------------------------------------------------------------- column / stack

pub struct Books {
    kind: usize, // 0 single 1 stack 2 column
    bases: Vec<BookF>,
    n_books: usize,
    max_angle: f64,
    max_rel_scale: f64,
    max_skew: f64,
    fseed: u64,
}

impl Books {
    fn new(c: &str, rng: &mut Rng, fseed: u64) -> Books {
        let mut r = rng.clone();
        let kind = match c {
            "BookFactory" => 0,
            "BookStackFactory" => 1,
            _ => 2,
        };
        if kind == 0 {
            let b = BookF::new(&mut r);
            return Books { kind, max_rel_scale: b.rel_scale, max_skew: b.skew, bases: vec![b], n_books: 1, max_angle: 0.0, fseed };
        }
        let nb = r.randint(1, 4) as usize;
        let mut bases = vec![];
        for _ in 0..nb {
            r.randint(0, 100_000);
            bases.push(BookF::new(&mut r));
        }
        let (n_books, max_angle) = if kind == 1 {
            let n = r.log_uniform(5.0, 15.0) as usize;
            let a = if r.uniform(0.0, 1.0) < 0.7 { r.uniform(std::f64::consts::PI / 9.0, std::f64::consts::PI / 6.0) } else { 0.0 };
            (n, a)
        } else {
            let n = r.randint(10, 20) as usize;
            let a = if r.uniform(0.0, 1.0) < 0.7 { r.uniform(0.0, std::f64::consts::PI / 9.0) } else { 0.0 };
            (n, a)
        };
        let max_rel_scale = bases.iter().map(|b| b.rel_scale).fold(f64::MIN, f64::max);
        let max_skew = bases.iter().map(|b| b.skew).fold(f64::MIN, f64::max);
        Books { kind, bases, n_books, max_angle, max_rel_scale, max_skew, fseed }
    }

    fn column(&self, r: &mut Rng) -> Vec<Mesh> {
        let pi = std::f64::consts::PI;
        let mut books: Vec<Mesh> = vec![];
        for i in 0..self.n_books {
            let bi = r.randint(0, self.bases.len() as i64) as usize;
            let (paper, cover) = self.bases[bi].create(r);
            let mut obj = join(&paper, &cover);
            let (lo, hi) = bounds32(&[&obj]);
            apply_loc(&mut [&mut obj], [-hi[0], -lo[1], -lo[2]]);
            let euler;
            if r.uniform(0.0, 1.0) < 0.5 {
                euler = [pi / 2.0 - r.uniform(0.0, self.max_angle), 0.0, pi / 2.0];
            } else {
                apply_loc(&mut [&mut obj], [0.0, 0.0, -hi[2]]);
                euler = [pi / 2.0 + r.uniform(0.0, self.max_angle), 0.0, pi / 2.0];
            }
            apply_rot(&mut [&mut obj], euler, [0.0; 3]);
            if i > 0 {
                apply_loc(&mut [&mut obj], [10.0, 0.0, 0.0]);
                // obj2trimesh triangulates the object in place, at its current (x = 10) position
                obj = triangulate(&obj);
                let prev = &books[i - 1];
                let tp = triangles_of(&[prev]);
                let to = triangles_of(&[&obj]);
                let d1 = longest_ray_min(&tp, &obj.pos, [-1.0, 0.0, 0.0]);
                let d2 = longest_ray_min(&to, &prev.pos, [1.0, 0.0, 0.0]);
                let offset = d1.min(d2);
                apply_loc(&mut [&mut obj], [-offset, 0.0, 0.0]);
            } else {
                // triangulated while serving as books[-1] in the next iteration (same position)
                obj = triangulate(&obj);
            }
            books.push(obj);
        }
        let mn = books.iter().map(|b| bounds32(&[b]).0[0]).fold(f64::INFINITY, f64::min);
        for b in books.iter_mut() {
            apply_loc(&mut [b], [-mn, 0.0, 0.0]);
        }
        books
    }

    fn stack(&self, r: &mut Rng) -> Vec<(Mesh, Mesh)> {
        let mut books: Vec<(Mesh, Mesh)> = vec![];
        let mut offset = 0.0f64;
        for _ in 0..self.n_books {
            let bi = r.randint(0, self.bases.len() as i64) as usize;
            let (mut paper, mut cover) = self.bases[bi].create(r);
            let (lo, hi) = bounds32(&[&paper, &cover]);
            let c0 = ((lo[0] as f32 + hi[0] as f32) / 2.0f32) as f64;
            let c1 = ((lo[1] as f32 + hi[1] as f32) / 2.0f32) as f64;
            let loc = [-c0, -c1, offset - lo[2]];
            let rz = r.uniform(-self.max_angle, self.max_angle);
            apply_rot(&mut [&mut paper, &mut cover], [0.0, 0.0, rz], loc);
            offset = bounds32(&[&paper, &cover]).1[2];
            books.push((paper, cover));
        }
        books
    }
}

impl Factory for Books {
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fseed, i);
        let (books, joined) = match self.kind {
            0 => (vec![self.bases[0].create(&mut r)], vec![]),
            1 => (self.stack(&mut r), vec![]),
            _ => (vec![], self.column(&mut r)),
        };
        let paper_m = spec_material("book_pages", json!({"cls": "ceramic.Plaster", "kwargs": {}, "seed": self.fseed}), [0.9, 0.88, 0.8]);
        let cover_m = spec_material("book_cover", json!({"cls": "art.Art", "kwargs": {}, "seed": self.fseed + 1}), [0.5, 0.3, 0.3]);
        let mut a = Asset::default();
        for j in joined {
            let jb = j.bounds();
            a.mesh(j, &cover_m, Some(jb));
        }
        for (p, c) in books {
            let pb = p.bounds();
            a.mesh(p, &paper_m, Some(pb));
            let cb = c.bounds();
            a.mesh(c, &cover_m, Some(cb));
        }
        a
    }

    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mesh = match self.kind {
            2 => {
                let height = 0.15 * self.max_rel_scale * self.max_skew;
                new_bbox(0.0, (0.02 + self.max_angle.sin() * height) * self.n_books as f64 * self.max_rel_scale, -0.15 * self.max_rel_scale, 0.0, 0.0, height)
            }
            1 => {
                let x_lo = -0.15 * self.max_rel_scale / 2.0;
                let x_hi = 0.15 * self.max_rel_scale / 2.0;
                let y_lo = -0.15 * self.max_rel_scale / 2.0 * self.max_skew;
                let y_hi = 0.15 * self.max_rel_scale / 2.0 * self.max_skew;
                let th = self.max_angle;
                let rot = |x: f64, y: f64| (x * th.cos() - y * th.sin(), x * th.sin() + y * th.cos());
                let c = [rot(x_lo, y_lo), rot(x_lo, y_hi), rot(x_hi, y_lo), rot(x_hi, y_hi)];
                let mnx = c.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).min(x_lo);
                let mxx = c.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).max(x_hi);
                let mny = c.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).min(y_lo);
                let mxy = c.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).max(y_hi);
                new_bbox(mnx, mxx, mny, mxy, 0.0, self.n_books as f64 * 0.02 * self.max_rel_scale * 0.8)
            }
            _ => return None,
        };
        let mut a = Asset::default();
        let m = mat("ph", [0.5, 0.5, 0.5], 0.5);
        let b = mesh.bounds();
        a.mesh(mesh, &m, Some(b));
        Some(a)
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Books::new(c, rng, fseed)))
}

/// SOLIDIFY with an arbitrary offset in [-1, 1] (-1: shell inside, 0: centred, +1: outside): original vertices move by (offset+1)/2 of the shell vector
/// (vertex order: original copies, then shell copies; the original faces keep their orientation)
pub fn solidify_offset(m: &Mesh, t: f64, offset: f64) -> Mesh {
    let mut s = solidify_simple(m, t, None);
    let n = m.pos.len();
    let fr = ((offset + 1.0) / 2.0) as f32;
    for i in 0..n {
        let (p, q) = (s.pos[i], s.pos[i + n]);
        for k in 0..3 {
            let d = (p[k] as f32 - q[k] as f32) * fr;
            s.pos[i][k] = (p[k] as f32 + d) as f64;
            s.pos[i + n][k] = (q[k] as f32 + d) as f64;
        }
    }
    s
}
