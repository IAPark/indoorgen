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
    let a = (1.0 - m).sqrt() * (1.570_796_3 + m * (-0.213_300_99 + m * (0.077_980_48 + m * -0.021_640_95)));
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

fn mean_value_weights(c: [[f64; 2]; 4], co: [f64; 2]) -> [f64; 4] {
    let dirs: Vec<[f64; 2]> = c.iter().map(|v| [v[0] - co[0], v[1] - co[1]]).collect();
    let lens: Vec<f64> = dirs.iter().map(|d| (d[0] * d[0] + d[1] * d[1]).sqrt()).collect();
    let eps = f32::EPSILON as f64;
    for i in 0..4 {
        if lens[i] < eps {
            let mut w = [0.0; 4];
            w[i] = 1.0;
            return w;
        }
    }
    let t = |i1: usize, i2: usize| -> f64 {
        let area = dirs[i1][0] * dirs[i2][1] - dirs[i1][1] * dirs[i2][0];
        if area != 0.0 {
            (((lens[i1] * lens[i2]) - (dirs[i1][0] * dirs[i2][0] + dirs[i1][1] * dirs[i2][1])) / area).abs()
        } else {
            0.0
        }
    };
    let tt = [t(0, 1), t(1, 2), t(2, 3), t(3, 0)];
    let mut w = [(tt[3] + tt[0]) / lens[0], (tt[0] + tt[1]) / lens[1], (tt[1] + tt[2]) / lens[2], (tt[2] + tt[3]) / lens[3]];
    let wtot: f64 = w.iter().sum();
    if wtot != 0.0 {
        for x in w.iter_mut() {
            *x /= wtot;
        }
        if w.iter().all(|x| x.is_finite()) {
            return w;
        }
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
    // corner angles |pi - angle(prev, v, next)|
    let ang: Vec<f64> = (0..n)
        .map(|i| {
            let (p, v, q) = (lp[(i + n - 1) % n], lp[i], lp[(i + 1) % n]);
            let a = norm(sub(p, v));
            let b = norm(sub(q, v));
            let d = dot(a, b).clamp(-1.0, 1.0);
            (std::f64::consts::PI - d.acos()).abs()
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
    sorted.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap().then(x.1.cmp(&y.1)));
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
        grid[xy(0, y)] = Some(l[span - y]);
        grid[xy(xtot - 1, y)] = Some(l[half + y]);
    }
    for x in 0..xtot {
        grid[xy(x, 0)] = Some(l[span + x]);
        grid[xy(x, ytot - 1)] = Some(l[(n - x) % n]);
    }
    let xs = 1.0 / (xtot - 1) as f64;
    let ys = 1.0 / (ytot - 1) as f64;
    for y in 1..ytot - 1 {
        for x in 1..xtot - 1 {
            let (xf, yf) = (xs * x as f64, ys * y as f64);
            let w = mean_value_weights([[xf, 0.0], [0.0, yf], [xf, 1.0], [1.0, yf]], [xf, yf]);
            let p = [grid[xy(x, 0)].unwrap(), grid[xy(0, y)].unwrap(), grid[xy(x, ytot - 1)].unwrap(), grid[xy(xtot - 1, y)].unwrap()];
            let mut co = [0.0; 3];
            for k in 0..4 {
                for c in 0..3 {
                    co[c] += p[k][c] * w[k];
                }
            }
            grid[xy(x, y)] = Some(co);
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
