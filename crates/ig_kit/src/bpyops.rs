//! Ports of Blender operators / modifiers used by bpy-built assets (chairs, beds, towels): fill_grid, SOLIDIFY (extrude mode),
//! MIRROR, vertex normals. Geometry only (vertex order is not Blender's).
use crate::mesh::Mesh;

type V = [f64; 3];
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a);
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        a
    }
}

fn safe_acos_approx(x: f64) -> f64 {
    let x = x as f32;
    let f = x.abs();
    let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
    let a = (1.0 - m).sqrt() * (1.570_796_326_7 + m * (-0.213_300_99 + m * (0.077_980_48 + m * -0.021_640_95)));
    (if x < 0.0 { std::f32::consts::PI - a } else { a }) as f64
}

pub fn face_normal(m: &Mesh, f: &[u32]) -> V {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    norm(n)
}

/// Mesh::vert_normals(): face normals weighted by the corner angle (safe_acos_approx)
pub fn vert_normals(m: &Mesh) -> Vec<V> {
    let mut vn = vec![[0.0; 3]; m.pos.len()];
    for f in &m.faces {
        let n = face_normal(m, f);
        let k = f.len();
        for i in 0..k {
            let v = f[i] as usize;
            let p = m.pos[v];
            let dp = norm(sub(m.pos[f[(i + k - 1) % k] as usize], p));
            let dn = norm(sub(m.pos[f[(i + 1) % k] as usize], p));
            let w = safe_acos_approx(dot(dp, dn));
            for c in 0..3 {
                vn[v][c] += n[c] * w;
            }
        }
    }
    for (i, n) in vn.iter_mut().enumerate() {
        if len(*n) == 0.0 {
            *n = norm(m.pos[i]);
        } else {
            *n = norm(*n);
        }
    }
    vn
}

/// SOLIDIFY modifier, extrude mode, no even thickness, rim on: `thickness`, `offset` in [-1, 1]
pub fn solidify(m: &Mesh, thickness: f64, offset: f64) -> Mesh {
    let n = m.pos.len();
    let vn = vert_normals(m);
    let ofs_orig = -(((-offset + 1.0) * 0.5) * thickness);
    let ofs_new = thickness + ofs_orig;
    let mut out = Mesh::new();
    // original vertices are moved by ofs_new (original winding), the copy by ofs_orig (flipped winding)
    for i in 0..n {
        let p = m.pos[i];
        out.pos.push([p[0] + vn[i][0] * ofs_new, p[1] + vn[i][1] * ofs_new, p[2] + vn[i][2] * ofs_new]);
    }
    for i in 0..n {
        let p = m.pos[i];
        out.pos.push([p[0] + vn[i][0] * ofs_orig, p[1] + vn[i][1] * ofs_orig, p[2] + vn[i][2] * ofs_orig]);
    }
    for f in &m.faces {
        out.faces.push(f.clone());
        out.faces.push(f.iter().rev().map(|&v| v + n as u32).collect());
    }
    // rim: boundary edges
    use crate::dhash::HashMap;
    let mut cnt: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let e = cnt.entry((a.min(b), a.max(b))).or_insert((0, a, b));
            e.0 += 1;
        }
    }
    let mut keys: Vec<_> = cnt.iter().filter(|(_, v)| v.0 == 1).map(|(k, v)| (*k, v.1, v.2)).collect();
    keys.sort();
    for (_, a, b) in keys {
        // boundary edge a->b in the original face: side quad between the two shells (outward)
        out.faces.push(vec![a, a + n as u32, b + n as u32, b]);
    }
    out
}

/// barycentric_weights_v2_quad in float32 (blenlib math_geom, no FMA contraction in the shipped binary)
fn mean_value_weights32(c: [[f32; 2]; 4], co: [f32; 2]) -> [f32; 4] {
    let dirs: Vec<[f32; 2]> = c.iter().map(|v| [v[0] - co[0], v[1] - co[1]]).collect();
    let lens: Vec<f32> = dirs.iter().map(|d| (d[0] * d[0] + d[1] * d[1]).sqrt()).collect();
    for i in 0..4 {
        if lens[i] < f32::EPSILON {
            let mut w = [0.0; 4];
            w[i] = 1.0;
            return w;
        }
    }
    let t = |i1: usize, i2: usize| -> f32 {
        let area = dirs[i1][0] * dirs[i2][1] - dirs[i1][1] * dirs[i2][0];
        if area != 0.0 {
            (((lens[i1] * lens[i2]) - (dirs[i1][0] * dirs[i2][0] + dirs[i1][1] * dirs[i2][1])) / area).abs()
        } else {
            0.0
        }
    };
    let tt = [t(0, 1), t(1, 2), t(2, 3), t(3, 0)];
    let mut w = [(tt[3] + tt[0]) / lens[0], (tt[0] + tt[1]) / lens[1], (tt[1] + tt[2]) / lens[2], (tt[2] + tt[3]) / lens[3]];
    let wtot = ((w[0] + w[1]) + w[2]) + w[3];
    let r = 1.0f32 / wtot;
    for x in w.iter_mut() {
        *x *= r;
    }
    if w.iter().all(|x| x.is_finite()) {
        return w;
    }
    [0.25; 4]
}

/// bpy.ops.mesh.fill_grid(use_interp_simple=True) on a single closed edge loop (cyclic vertex list `lp`, even length)
pub fn fill_grid_loop(lp: &[V]) -> Mesh {
    fill_grid_loop_ex(lp, false)
}

/// `first_is_best`: the loop is already rotated so that its first vertex is the corner Blender selects (bpy fill_grid: first vertex with
/// the maximum f32 angle deviation, offset applied); the f64 angle search below is skipped
pub fn fill_grid_loop_ex(lp: &[V], first_is_best: bool) -> Mesh {
    let n = lp.len();
    let half = n / 2;
    // corner angles |pi - angle(prev, v, next)| in float32 (edbm_fill_grid_vert_tag_angle)
    let f3 = |p: V| [p[0] as f32, p[1] as f32, p[2] as f32];
    let ang: Vec<f64> = (0..n)
        .map(|i| {
            let (p, v, q) = (lp[(i + n - 1) % n], lp[i], lp[(i + 1) % n]);
            (std::f32::consts::PI - crate::bm::query::angle_v3v3v3(f3(p), f3(v), f3(q))).abs() as f64
        })
        .collect();
    let mut best = 0;
    for i in 0..n {
        if !first_is_best && ang[i] > ang[best] {
            best = i;
        }
    }
    // rotate so that the best corner is first
    let rot: Vec<usize> = (0..n).map(|i| (best + i) % n).collect();
    let l: Vec<V> = rot.iter().map(|&i| lp[i]).collect();
    let a2: Vec<f64> = rot.iter().map(|&i| ang[i]).collect();
    let mut span = n / 4;
    let mut sorted: Vec<(f64, usize)> = a2.iter().cloned().enumerate().map(|(i, a)| (a, i)).collect();
    // C qsort of macOS libc (FreeBSD derived, not stable) with BLI_sortutil_cmp_float_reverse: ties between corners go by its partitioning
    libc_qsort(&mut sorted, &|a: &(f64, usize), b: &(f64, usize)| {
        let (x, y) = (a.0 as f32, b.0 as f32);
        if x < y { 1 } else if x > y { -1 } else { 0 }
    });
    if std::env::var("FGDBG").is_ok() {
        eprintln!("fill_grid n {n} best {best} top {:?} last {:?}", &sorted[..8.min(n)], sorted[n - 1]);
    }
    if sorted[2].0 - sorted[n - 1].0 > 1e-3 {
        let tagged: Vec<usize> = sorted.iter().take(4).map(|x| x.1).collect();
        for i in 0..half {
            if tagged.contains(&i) && i != 0 {
                span = i;
                break;
            }
        }
    }
    let xtot = half - span + 1;
    let ytot = span + 1;
    let mut grid: Vec<Option<V>> = vec![None; xtot * ytot];
    let xy = |x: usize, y: usize| x + y * xtot;
    for y in 0..ytot {
        // bm_grid_fill: left rail = second half of the loop, right rail = first `ytot` vertices (bit-exact vs python, see tools/pancap)
        grid[xy(0, y)] = Some(l[half + (ytot - 1 - y)]);
        grid[xy(xtot - 1, y)] = Some(l[y]);
    }
    for x in 0..xtot {
        grid[xy(x, 0)] = Some(l[(n - (xtot - 1 - x)) % n]);
        grid[xy(x, ytot - 1)] = Some(l[span + xtot - 1 - x]);
    }
    let xs = 1.0f32 / (xtot - 1) as f32;
    let ys = 1.0f32 / (ytot - 1) as f32;
    for y in 1..ytot - 1 {
        for x in 1..xtot - 1 {
            let (xf, yf) = (xs * x as f32, ys * y as f32);
            let w = mean_value_weights32([[xf, 0.0], [0.0, yf], [xf, 1.0], [1.0, yf]], [xf, yf]);
            let p = [grid[xy(x, 0)].unwrap(), grid[xy(0, y)].unwrap(), grid[xy(x, ytot - 1)].unwrap(), grid[xy(xtot - 1, y)].unwrap()];
            let mut co = [0.0f32; 3];
            for k in 0..4 {
                for c in 0..3 {
                    co[c] += (p[k][c] as f32) * w[k];
                }
            }
            grid[xy(x, y)] = Some([co[0] as f64, co[1] as f64, co[2] as f64]);
        }
    }
    let mut m = Mesh::new();
    for g in &grid {
        m.pos.push(g.unwrap());
    }
    for x in 0..xtot - 1 {
        for y in 0..ytot - 1 {
            m.faces.push(vec![xy(x + 1, y) as u32, xy(x + 1, y + 1) as u32, xy(x, y + 1) as u32, xy(x, y) as u32]);
        }
    }
    // orient so that the normals agree with the loop's own winding (+z for a counter-clockwise loop seen from above)
    let mut area = 0.0;
    for i in 0..n {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        area += a[0] * b[1] - b[0] * a[1];
    }
    let fnz: f64 = m.faces.iter().map(|f| face_normal(&m, f)[2]).sum();
    if (area > 0.0) != (fnz > 0.0) {
        for f in m.faces.iter_mut() {
            f.reverse();
        }
    }
    m
}

pub fn _unused(_: V) -> V {
    cross([0.0; 3], [0.0; 3])
}

/// macOS libc `qsort` (Libc/stdlib/FreeBSD/qsort.c): median-of-three quicksort with insertion sort for n < 7 and when a partitioning pass swapped nothing
pub fn libc_qsort<T: Clone>(a: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
    qs(a, 0, a.len(), cmp);
}

fn qs<T: Clone>(a: &mut [T], start: usize, n0: usize, cmp: &dyn Fn(&T, &T) -> i32) {
    let (mut base, mut n) = (start, n0);
    loop {
        let mut swap_cnt = false;
        if n < 7 {
            insertion(a, base, n, cmp);
            return;
        }
        let med3 = |a: &[T], x: usize, y: usize, z: usize| -> usize {
            if cmp(&a[x], &a[y]) < 0 {
                if cmp(&a[y], &a[z]) < 0 { y } else if cmp(&a[x], &a[z]) < 0 { z } else { x }
            } else if cmp(&a[y], &a[z]) > 0 {
                y
            } else if cmp(&a[x], &a[z]) < 0 {
                x
            } else {
                z
            }
        };
        let mut pm = base + n / 2;
        if n > 7 {
            let mut pl = base;
            let mut pn = base + n - 1;
            if n > 40 {
                let d = n / 8;
                pl = med3(a, pl, pl + d, pl + 2 * d);
                pm = med3(a, pm - d, pm, pm + d);
                pn = med3(a, pn - 2 * d, pn - d, pn);
            }
            pm = med3(a, pl, pm, pn);
        }
        a.swap(base, pm);
        let mut pa = base + 1;
        let mut pb = pa;
        let mut pc = base + n - 1;
        let mut pd = pc;
        loop {
            while pb <= pc {
                let r = cmp(&a[pb], &a[base]);
                if r > 0 {
                    break;
                }
                if r == 0 {
                    swap_cnt = true;
                    a.swap(pa, pb);
                    pa += 1;
                }
                pb += 1;
            }
            while pb <= pc {
                let r = cmp(&a[pc], &a[base]);
                if r < 0 {
                    break;
                }
                if r == 0 {
                    swap_cnt = true;
                    a.swap(pc, pd);
                    pd -= 1;
                }
                pc -= 1;
            }
            if pb > pc {
                break;
            }
            a.swap(pb, pc);
            swap_cnt = true;
            pb += 1;
            pc -= 1;
        }
        if !swap_cnt {
            insertion(a, base, n, cmp);
            return;
        }
        let pn = base + n;
        let mut d1 = (pa - base).min(pb - pa);
        vecswap(a, base, pb - d1, d1);
        d1 = (pd - pc).min(pn - pd - 1);
        vecswap(a, pb, pn - d1, d1);
        let d1 = pb - pa;
        let d2 = pd - pc;
        if d1 <= d2 {
            if d1 > 1 {
                qs(a, base, d1, cmp);
            }
            if d2 > 1 {
                base = pn - d2;
                n = d2;
                continue;
            }
        } else {
            if d2 > 1 {
                qs(a, pn - d2, d2, cmp);
            }
            if d1 > 1 {
                n = d1;
                continue;
            }
        }
        return;
    }
}

fn insertion<T: Clone>(a: &mut [T], base: usize, n: usize, cmp: &dyn Fn(&T, &T) -> i32) {
    for pm in base + 1..base + n {
        let mut pl = pm;
        while pl > base && cmp(&a[pl - 1], &a[pl]) > 0 {
            a.swap(pl, pl - 1);
            pl -= 1;
        }
    }
}

fn vecswap<T: Clone>(a: &mut [T], i: usize, j: usize, n: usize) {
    for k in 0..n {
        a.swap(i + k, j + k);
    }
}
