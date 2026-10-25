//! wall_decorations/balloon.py BalloonFactory: the placeholder (text bounding box) is RNG-exact; the asset (inflated text mesh from a cloth simulation) is
//! approximated by a flattened ellipsoid filling the placeholder.
use super::matdraw;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "BalloonFactory"
}


// ---------------------------------------------------------------- text -> mesh (Blender Bfont curve conversion)

static FONT_JSON: &str = include_str!("../data/bfont_caps.json");

struct Glyph {
    /// per spline: (cyclic, points (co, handle_left, handle_right, left_is_vector, right_is_vector))
    splines: Vec<Vec<([f32; 2], [f32; 2], [f32; 2], bool, bool)>>,
}

fn font() -> &'static std::collections::HashMap<char, (Glyph, f32)> {
    static F: std::sync::OnceLock<std::collections::HashMap<char, (Glyph, f32)>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(FONT_JSON).unwrap();
        let mut out = std::collections::HashMap::new();
        for (k, g) in v["glyphs"].as_object().unwrap() {
            let c = k.chars().next().unwrap();
            let adv = v["adv"][k].as_f64().unwrap() as f32;
            let f2 = |a: &serde_json::Value| [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32];
            let splines = g["splines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    s["pts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| (f2(&p[0]), f2(&p[1]), f2(&p[2]), p[3].as_str() == Some("V"), p[4].as_str() == Some("V")))
                        .collect()
                })
                .collect();
            out.insert(c, (Glyph { splines }, adv));
        }
        out
    })
}

/// BKE_curve_forward_diff_bezier (float32), `it`+1 samples, the last one dropped by the caller
fn forward_diff(q0: f32, q1: f32, q2: f32, q3: f32, it: usize) -> Vec<f32> {
    let itf = it as f32;
    let rt0 = q0;
    let rt1 = 3.0 * (q1 - q0) / itf;
    let f = 1.0 / itf;
    let rt2 = 3.0 * (q0 - 2.0 * q1 + q2) * f * f;
    let rt3 = (q3 - q0 + 3.0 * (q1 - q2)) * f * f * f;
    let (mut a, mut b, mut c, d) = (rt0, rt1 + rt2 + rt3, 2.0 * rt2 + 6.0 * rt3, 6.0 * rt3);
    let mut out = Vec::with_capacity(it + 1);
    for _ in 0..=it {
        out.push(a);
        a += b;
        b += c;
        c += d;
    }
    out
}

/// displist points of one cyclic bezier spline (resolution 12; straight segments between two vector handles give one point)
fn spline_points(pts: &[([f32; 2], [f32; 2], [f32; 2], bool, bool)], off: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    let mut out = vec![];
    for a in 0..n {
        let (p, q) = (&pts[a], &pts[(a + 1) % n]);
        if p.4 && q.3 {
            out.push([p.0[0] + off, p.0[1]]);
        } else {
            let xs = forward_diff(p.0[0] + off, p.2[0] + off, q.1[0] + off, q.0[0] + off, 12);
            let ys = forward_diff(p.0[1], p.2[1], q.1[1], q.0[1], 12);
            for j in 0..12 {
                out.push([xs[j], ys[j]]);
            }
        }
    }
    out
}

fn area2(p: &[[f32; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] as f64 * v[1] as f64 - v[0] as f64 * u[1] as f64;
    }
    a
}

fn point_in(poly: &[[f32; 2]], q: [f32; 2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// bpy text object (Bfont, size 1, default settings) converted to a mesh: one filled triangle set, z = 0, +z facing
pub fn text_mesh(text: &str) -> Mesh {
    let mut m = Mesh::new();
    let mut x = 0f32;
    for ch in text.chars() {
        let Some((g, adv)) = font().get(&ch) else { continue };
        let conts: Vec<Vec<[f32; 2]>> = g.splines.iter().map(|s| spline_points(s, x)).collect();
        x += adv;
        if conts.is_empty() {
            continue;
        }
        let areas: Vec<f64> = conts.iter().map(|c| area2(c)).collect();
        let big = (0..conts.len()).max_by(|&a, &b| areas[a].abs().partial_cmp(&areas[b].abs()).unwrap()).unwrap();
        let outer_sign = areas[big].signum();
        let outers: Vec<usize> = (0..conts.len()).filter(|&i| areas[i].signum() == outer_sign).collect();
        let base = m.pos.len() as u32;
        let mut starts = vec![];
        let mut acc = 0u32;
        for c in &conts {
            starts.push(acc);
            acc += c.len() as u32;
            for p in c {
                m.pos.push([p[0] as f64, p[1] as f64, 0.0]);
            }
        }
        for &o in &outers {
            let holes: Vec<usize> = (0..conts.len())
                .filter(|&h| {
                    areas[h].signum() != outer_sign
                        && point_in(&conts[o], conts[h][0])
                        && !outers.iter().any(|&o2| o2 != o && areas[o2].abs() < areas[o].abs() && point_in(&conts[o2], conts[h][0]) && point_in(&conts[o], conts[o2][0]))
                })
                .collect();
            let mut flat: Vec<f64> = vec![];
            let mut idx: Vec<u32> = vec![];
            let mut hole_starts = vec![];
            for p in &conts[o] {
                flat.extend([p[0] as f64, p[1] as f64]);
            }
            for k in 0..conts[o].len() as u32 {
                idx.push(starts[o] + k);
            }
            for &h in &holes {
                hole_starts.push(idx.len());
                for p in &conts[h] {
                    flat.extend([p[0] as f64, p[1] as f64]);
                }
                for k in 0..conts[h].len() as u32 {
                    idx.push(starts[h] + k);
                }
            }
            let tris = earcutr::earcut(&flat, &hole_starts, 2).unwrap_or_default();
            for t in tris.chunks(3) {
                let (a, b, c) = (idx[t[0]], idx[t[1]], idx[t[2]]);
                let (pa, pb, pc) = (m.pos[(base + a) as usize], m.pos[(base + b) as usize], m.pos[(base + c) as usize]);
                let cr = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
                if cr >= 0.0 {
                    m.faces.push(vec![base + a, base + b, base + c]);
                } else {
                    m.faces.push(vec![base + a, base + c, base + b]);
                }
            }
        }
    }
    m
}


type V3f = [f64; 3];
fn vsub(a: V3f, b: V3f) -> V3f {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vdot(a: V3f, b: V3f) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// closest point on triangle (Ericson), returns (distance^2, barycentric weights)
fn closest_tri(p: V3f, a: V3f, b: V3f, c: V3f) -> (f64, [f64; 3]) {
    let (ab, ac, ap) = (vsub(b, a), vsub(c, a), vsub(p, a));
    let (d1, d2) = (vdot(ab, ap), vdot(ac, ap));
    let w = |u: f64, v: f64, ww: f64| -> (f64, [f64; 3]) {
        let q = [a[0] * u + b[0] * v + c[0] * ww, a[1] * u + b[1] * v + c[1] * ww, a[2] * u + b[2] * v + c[2] * ww];
        let d = vsub(p, q);
        (vdot(d, d), [u, v, ww])
    };
    if d1 <= 0.0 && d2 <= 0.0 {
        return w(1.0, 0.0, 0.0);
    }
    let bp = vsub(p, b);
    let (d3, d4) = (vdot(ab, bp), vdot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return w(0.0, 1.0, 0.0);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return w(1.0 - v, v, 0.0);
    }
    let cp = vsub(p, c);
    let (d5, d6) = (vdot(ab, cp), vdot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return w(0.0, 0.0, 1.0);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let ww = d2 / (d2 - d6);
        return w(1.0 - ww, 0.0, ww);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let ww = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return w(0.0, 1.0 - ww, ww);
    }
    let den = 1.0 / (va + vb + vc);
    let (v, ww) = (vb * den, vc * den);
    w(1.0 - v - ww, v, ww)
}

/// utils.draw.remesh_fill(obj, 0.02): SOLIDIFY(0.1), SHARP REMESH at 0.02, drop the far shell (nearest-surface "top" attribute)
pub fn remesh_fill(text: &Mesh, res: f64) -> Mesh {
    let sol = bpyops::solidify(text, 0.10000000149011612, -1.0);
    let n = text.pos.len() as u32;
    let pos32: Vec<[f32; 3]> = sol.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &pos32 {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let dim = (0..3).map(|k| (hi[k] - lo[k]) as f64).fold(0.0, f64::max);
    let depth = (((dim + 0.01) / res).log2()).ceil() as i32;
    let (v, q) = dualcon::dualcon(&pos32, &sol.faces, 2, depth, 0.9, 1.0);
    // source triangles of the solidified mesh
    let mut tris: Vec<[u32; 3]> = vec![];
    for f in &sol.faces {
        for k in 1..f.len() - 1 {
            tris.push([f[0], f[k], f[k + 1]]);
        }
    }
    let tb: Vec<(V3f, V3f)> = tris
        .iter()
        .map(|t| {
            let ps = [pos32[t[0] as usize], pos32[t[1] as usize], pos32[t[2] as usize]];
            let mut l = [f64::INFINITY; 3];
            let mut h = [f64::NEG_INFINITY; 3];
            for p in ps {
                for k in 0..3 {
                    l[k] = l[k].min(p[k] as f64);
                    h[k] = h[k].max(p[k] as f64);
                }
            }
            (l, h)
        })
        .collect();
    let keep: Vec<bool> = v
        .iter()
        .map(|p| {
            let p = [p[0] as f64, p[1] as f64, p[2] as f64];
            let mut best = (f64::INFINITY, 0usize, [0.0; 3]);
            for (ti, t) in tris.iter().enumerate() {
                let (l, h) = tb[ti];
                let mut d2 = 0.0;
                for k in 0..3 {
                    let e = (l[k] - p[k]).max(p[k] - h[k]).max(0.0);
                    d2 += e * e;
                }
                if d2 > best.0 {
                    continue;
                }
                let g = |i: u32| {
                    let q = pos32[i as usize];
                    [q[0] as f64, q[1] as f64, q[2] as f64]
                };
                let (d, w) = closest_tri(p, g(t[0]), g(t[1]), g(t[2]));
                if d < best.0 {
                    best = (d, ti, w);
                }
            }
            let t = tris[best.1];
            let top: f64 = (0..3).map(|k| if t[k] >= n { best.2[k] } else { 0.0 }).sum();
            !(top > 1e-3)
        })
        .collect();
    let mut map = vec![u32::MAX; v.len()];
    let mut out = Mesh::new();
    for (i, p) in v.iter().enumerate() {
        if keep[i] {
            map[i] = out.pos.len() as u32;
            out.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
        }
    }
    for f in &q {
        if f.iter().all(|&i| (i as usize) < keep.len() && keep[i as usize]) {
            out.faces.push(f.iter().map(|&i| map[i as usize]).collect());
        }
    }
    out
}

/// decorate.subdivide_edge_ring(obj, cuts, z): every vertical edge (|dz| > 1-1e-3 after normalisation) gets `cuts` new vertices (linear: the
/// ring between the two solidify shells is straight), the rim quads become cuts+1 strips
pub fn subdivide_vertical(m: &Mesh, cuts: usize) -> Mesh {
    use crate::dhash::HashMap;
    let mut out = m.clone();
    let mut cut: HashMap<(u32, u32), Vec<u32>> = HashMap::default();
    let mut sel = |out: &mut Mesh, a: u32, b: u32| -> Option<Vec<u32>> {
        let k = (a.min(b), a.max(b));
        if let Some(v) = cut.get(&k) {
            return Some(if a < b { v.clone() } else { v.iter().rev().copied().collect() });
        }
        let (pa, pb) = (m.pos[k.0 as usize], m.pos[k.1 as usize]);
        let d = [(pb[0] as f32 - pa[0] as f32) as f64, (pb[1] as f32 - pa[1] as f32) as f64, (pb[2] as f32 - pa[2] as f32) as f64];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l == 0.0 || (d[2] / l).abs() <= 1.0 - 1e-3 {
            return None;
        }
        let mut ids = vec![];
        for i in 1..=cuts {
            let t = i as f64 / (cuts + 1) as f64;
            ids.push(out.pos.len() as u32);
            out.pos.push([pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t, pa[2] + (pb[2] - pa[2]) * t]);
        }
        cut.insert(k, ids.clone());
        Some(if a < b { ids } else { ids.iter().rev().copied().collect() })
    };
    let mut faces = vec![];
    for f in &m.faces {
        if f.len() == 4 {
            // quad with two opposite vertical edges: (f0,f1) and (f2,f3)
            for r in 0..2 {
                let (a, b, c, d) = (f[r], f[(r + 1) % 4], f[(r + 2) % 4], f[(r + 3) % 4]);
                // edges (a,b) and (c,d) vertical?
                let e1 = sel(&mut out, a, b);
                let e2 = sel(&mut out, c, d);
                if let (Some(v1), Some(v2)) = (e1, e2) {
                    // a -> b along v1, c -> d along v2
                    let mut la = vec![a];
                    la.extend(&v1);
                    la.push(b);
                    let mut lc = vec![c];
                    lc.extend(&v2);
                    lc.push(d);
                    lc.reverse();
                    for i in 0..=cuts {
                        faces.push(vec![la[i], la[i + 1], lc[i + 1], lc[i]]);
                    }
                    break;
                }
                if r == 1 {
                    faces.push(f.clone());
                }
            }
        } else {
            faces.push(f.clone());
        }
    }
    out.faces = faces;
    out
}

/// Blender SMOOTH modifier (factor, iterations; all axes): v = v*(1-f) + f*mean(edge midpoints)
pub fn smooth_modifier(m: &mut Mesh, factor: f32, iters: usize) {
    use crate::dhash::HashSet;
    let mut seen: HashSet<(u32, u32)> = HashSet::default();
    let mut edges = vec![];
    for f in &m.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            if seen.insert((a.min(b), a.max(b))) {
                edges.push((a.min(b), a.max(b)));
            }
        }
    }
    let mut v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    for _ in 0..iters {
        let mut acc = vec![[0f32; 3]; v.len()];
        let mut cnt = vec![0u32; v.len()];
        for &(a, b) in &edges {
            let mid = [(v[a as usize][0] + v[b as usize][0]) * 0.5, (v[a as usize][1] + v[b as usize][1]) * 0.5, (v[a as usize][2] + v[b as usize][2]) * 0.5];
            for &i in &[a, b] {
                for k in 0..3 {
                    acc[i as usize][k] += mid[k];
                }
                cnt[i as usize] += 1;
            }
        }
        for i in 0..v.len() {
            if cnt[i] > 0 {
                let f = 1.0 / cnt[i] as f32;
                for k in 0..3 {
                    v[i][k] = v[i][k] * (1.0 - factor) + acc[i][k] * f * factor;
                }
            }
        }
    }
    m.pos = v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
}

/// Rust stand-in for `cloth_sim(gravity 0, pressure)` on the solidified, ring-subdivided text: Blender's result barely inflates the slab
/// (fitted on seed 0: the two shells move apart by 0.022*(1-exp(-d/0.03))-0.002 each, d = xy distance to the rim)
pub fn inflate(pre: &Mesh, rim_src: &Mesh) -> Mesh {
    // rim = vertices of the text shell that lie on boundary edges
    use crate::dhash::HashMap;
    let mut cnt: HashMap<(u32, u32), u32> = HashMap::default();
    for f in &rim_src.faces {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut rim = vec![];
    let mut isr = vec![false; rim_src.pos.len()];
    for (k, c) in &cnt {
        if *c == 1 {
            isr[k.0 as usize] = true;
            isr[k.1 as usize] = true;
        }
    }
    for (i, r) in isr.iter().enumerate() {
        if *r {
            rim.push(rim_src.pos[i]);
        }
    }
    let (zlo, zhi) = pre.pos.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p[2]), a.1.max(p[2])));
    let mut out = pre.clone();
    for p in out.pos.iter_mut() {
        let mut d2 = f64::INFINITY;
        for r in &rim {
            d2 = d2.min((r[0] - p[0]).powi(2) + (r[1] - p[1]).powi(2));
        }
        let h = 0.022 * (1.0 - (-d2.sqrt() / 0.03).exp()) - 0.002;
        let frac = (p[2] - zlo) / (zhi - zlo);
        p[2] += h * (2.0 * frac - 1.0);
    }
    out
}

/// subsurf(1), scale, rotate (pi/2, 0, pi/2) and apply, DISPLACE (normal, mid 0.5), SMOOTH x5
pub fn post_cloth(m: &Mesh, rel_scale: f64, displace: f64) -> Mesh {
    let mut o = ig_shared::subsurf::subsurf(m, 1, false);
    let (a, c) = (std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
    // R = Rz(c) * Rx(a)
    let (sa, ca, sc, cc) = (a.sin(), a.cos(), c.sin(), c.cos());
    let r = [[cc, -sc * ca, sc * sa], [sc, cc * ca, -cc * sa], [0.0, sa, ca]];
    let sf = rel_scale as f32;
    for p in o.pos.iter_mut() {
        let q = [(p[0] as f32 * sf) as f64, (p[1] as f32 * sf) as f64, (p[2] as f32 * sf) as f64];
        let v = [0, 1, 2].map(|i| (r[i][0] * q[0] + r[i][1] * q[1] + r[i][2] * q[2]) as f32 as f64);
        *p = v;
    }
    let mut v32: Vec<[f32; 3]> = o.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    bpymod::displace(&mut v32, &o.faces, None, displace, 0.5, bpymod::DispDir::Normal, None);
    o.pos = v32.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    smooth_modifier(&mut o, 0.5, 5);
    o
}

/// generate_text(): the letters (random draws after the length draw), uppercased
fn gen_text(r: &mut Rng) -> String {
    let n = text_len_rng(r);
    let mut w = vec![];
    w.extend(std::iter::repeat(1.5).take(26));
    w.extend(std::iter::repeat(0.5).take(26));
    w.extend(std::iter::repeat(0.5).take(10));
    let set: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().collect();
    // np.random.choice(size=n, p=) = n uniforms searched in the normalised cdf
    let tot: f64 = w.iter().sum();
    let mut cdf = vec![];
    let mut acc = 0.0;
    for x in &w {
        acc += x / tot;
        cdf.push(acc);
    }
    let last = *cdf.last().unwrap();
    for c in cdf.iter_mut() {
        *c /= last;
    }
    let mut s = String::new();
    for _ in 0..n {
        let u = r.f64();
        let i = cdf.partition_point(|&c| c <= u).min(set.len() - 1);
        s.push(set[i].to_ascii_uppercase());
    }
    s
}

/// int(clip_gaussian(3, 7, 2, 15))
fn text_len_rng(r: &mut Rng) -> usize {
    let mut i = 0;
    loop {
        let v = r.normal(3.0, 7.0);
        if (2.0..=15.0).contains(&v) {
            return v as usize;
        }
        if i == 20 {
            return v.clamp(2.0, 15.0) as usize;
        }
        i += 1;
    }
}

pub struct Balloon {
    thickness: f64,
    rel_scale: f64,
    displace: f64,
    mat: &'static str,
    fs: u64,
}

impl Balloon {
    fn new(fs: u64) -> Balloon {
        let mut r = Rng::seeded(fs);
        let thickness = r.uniform(0.06, 0.1);
        let mat = matdraw::sample_call(&mut r, matdraw::DECORATIVE_METAL);
        let rel_scale = r.uniform(0.2, 0.3) * 4.0;
        let displace = r.uniform(0.02, 0.04);
        Balloon { thickness, rel_scale, displace, mat, fs }
    }

    fn text(&self, inst: u64) -> String {
        gen_text(&mut inst_rng(self.fs, inst))
    }
    fn text_len(&self, inst: u64) -> usize {
        text_len_rng(&mut inst_rng(self.fs, inst))
    }

    /// text mesh -> remesh_fill -> solidify -> ring subdivision (the cloth input, local text units)
    fn pre_cloth(&self, inst: u64) -> (Mesh, Mesh) {
        let rm = remesh_fill(&text_mesh(&self.text(inst)), 0.02);
        let sol = bpyops::solidify(&rm, self.thickness, 0.5);
        (subdivide_vertical(&sol, 8), rm)
    }

    fn bounds(&self, inst: u64) -> (V3, V3) {
        let n = self.text_len(inst) as f64;
        ([-self.thickness / 2.0, 0.0, 0.0], [self.thickness / 2.0, self.rel_scale * n * 0.8, self.rel_scale * 0.8])
    }
}

impl Factory for Balloon {
    fn placeholder(&self, inst: u64) -> Option<Asset> {
        let (lo, hi) = self.bounds(inst);
        let m = books::new_bbox(lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }
    fn asset(&self, inst: u64) -> Asset {
        let (pre, rm) = self.pre_cloth(inst);
        let m = post_cloth(&inflate(&pre, &rm), self.rel_scale, self.displace);
        let material = spec_material("balloon", json!({"cls": self.mat, "kwargs": {}, "seed": self.fs}), [0.8, 0.8, 0.8]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &material, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Balloon::new(fs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn load(p: &str) -> Option<(Vec<V3>, Vec<Vec<u32>>)> {
        let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        let pos = d["verts"].as_array()?.iter().map(|v| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]).collect();
        let faces = d["polys"].as_array()?.iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
        Some((pos, faces))
    }
    #[test]
    fn text_stage() {
        let dir = std::env::var("SD").unwrap_or("../../tmp/sd".into());
        let Some((rp, rf)) = load(&format!("{dir}/000_mesh_primitive_cube_add.json")) else { return };
        let mut r = inst_rng(0, 0);
        let t = gen_text(&mut r);
        let m = text_mesh(&t);
        eprintln!("text {t} verts {} (ref {}) faces {} (ref {})", m.pos.len(), rp.len(), m.faces.len(), rf.len());
        let (a, b, c) = ig_kit::cloth::surface_distance(&m.pos, &m.faces, &rp, &rf);
        eprintln!("surface dist mean {a} p95 {b} max {c}");
        let rm = remesh_fill(&m, 0.02);
        let sub = subdivide_vertical(&bpyops::solidify(&rm, 0.0819525420665741, 0.5), 8);
        eprintln!("pre-cloth verts {} faces {}", sub.pos.len(), sub.faces.len());
        if let Some((sp, sf)) = load(&format!("{dir}/007_object_modifier_apply_CLOTH.case.json")) {
            let (a, b, c) = ig_kit::cloth::surface_distance(&sub.pos, &sub.faces, &sp, &sf);
            eprintln!("pre-cloth vs stage007 input ({} verts {} faces): mean {a} p95 {b} max {c}", sp.len(), sf.len());
        }
        if let (Some((cp, cf)), Some((fp, ff))) = (load(&format!("{dir}/007_object_modifier_apply_CLOTH.json")), load(&format!("{dir}/final.json"))) {
            let mm = Mesh { pos: cp, faces: cf, uv: vec![] };
            std::env::set_var("REFSTUB", "2");
            let bl = Balloon::new(0);
            eprintln!("rel_scale {} displace {} thickness {}", bl.rel_scale, bl.displace, bl.thickness);
            let bz = |p: &Vec<V3>| Mesh { pos: p.clone(), faces: vec![], uv: vec![] }.bounds();
            let (flo, fhi) = bz(&fp);
            let o1 = post_cloth(&mm, 1.0, 0.0);
            let (olo, ohi) = bz(&o1.pos);
            let mut rs = (fhi[1] - flo[1]) / (ohi[1] - olo[1]);
            for _ in 0..4 {
                let t = post_cloth(&mm, rs, 0.03089766390621662);
                let (tl, th) = bz(&t.pos);
                rs *= (fhi[1] - flo[1]) / (th[1] - tl[1]);
            }
            eprintln!("fitted rel_scale {rs} (ref bounds {flo:?} {fhi:?})");
            let o = post_cloth(&mm, rs, 0.03089766390621662);
            let (a, b, c) = ig_kit::cloth::surface_distance(&o.pos, &o.faces, &fp, &ff);
            eprintln!("post_cloth(blender cloth) vs final: verts {} ({}) mean {a} p95 {b} max {c}", o.pos.len(), fp.len());
        }
        {
            use crate::dhash::HashMap;
            let mut c: HashMap<(u32, u32), u32> = HashMap::default();
            let mut tri = 0;
            for f in &sub.faces {
                if f.len() != 4 { tri += 1; }
                for i in 0..f.len() { let (a, b) = (f[i], f[(i + 1) % f.len()]); *c.entry((a.min(b), a.max(b))).or_insert(0) += 1; }
            }
            let used: std::collections::HashSet<u32> = sub.faces.iter().flatten().copied().collect();
            eprintln!("pre edges {} boundary {} nonmanifold {} nonquad {} used verts {}", c.len(), c.values().filter(|&&x| x == 1).count(), c.values().filter(|&&x| x > 2).count(), tri, used.len());
        }
        eprintln!("remesh_fill verts {} faces {}", rm.pos.len(), rm.faces.len());
        if let Some((sp, sf)) = load(&format!("{dir}/006_object_modifier_apply_SOLIDIFY.case.json")) {
            let (a, b, c) = ig_kit::cloth::surface_distance(&rm.pos, &rm.faces, &sp, &sf);
            eprintln!("vs stage006 input ({} verts {} faces): mean {a} p95 {b} max {c}", sp.len(), sf.len());
        }
    }
}
