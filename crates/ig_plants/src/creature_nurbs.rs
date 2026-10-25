//! NurbsPart of creatures/parts/generic_nurbs.py + utils/geometry/lofting.py + nurbs.py (geomdl surface evaluation) + part_util.nurbs_to_part.
use super::creature_rng::*;
use ig_core::rng::Rng;

pub type V3 = [f64; 3];

const NPY: [(&str, &[u8]); 15] = [
    ("body_feline_wolf", include_bytes!("../data/nurbs/body_feline_wolf.npy")),
    ("body_feline_housecat", include_bytes!("../data/nurbs/body_feline_housecat.npy")),
    ("body_feline_cheetah", include_bytes!("../data/nurbs/body_feline_cheetah.npy")),
    ("body_feline_tiger_2", include_bytes!("../data/nurbs/body_feline_tiger_2.npy")),
    ("body_feline_tiger", include_bytes!("../data/nurbs/body_feline_tiger.npy")),
    ("body_herbivore_giraffe", include_bytes!("../data/nurbs/body_herbivore_giraffe.npy")),
    ("body_herbivore_goat", include_bytes!("../data/nurbs/body_herbivore_goat.npy")),
    ("body_herbivore_cow", include_bytes!("../data/nurbs/body_herbivore_cow.npy")),
    ("body_herbivore_llama", include_bytes!("../data/nurbs/body_herbivore_llama.npy")),
    ("head_carnivore_wolf", include_bytes!("../data/nurbs/head_carnivore_wolf.npy")),
    ("head_carnivore_tiger", include_bytes!("../data/nurbs/head_carnivore_tiger.npy")),
    ("head_herbivore_goat", include_bytes!("../data/nurbs/head_herbivore_goat.npy")),
    ("head_herbivore_giraffe", include_bytes!("../data/nurbs/head_herbivore_giraffe.npy")),
    ("head_herbivore_cow", include_bytes!("../data/nurbs/head_herbivore_cow.npy")),
    ("head_herbivore_llama", include_bytes!("../data/nurbs/head_herbivore_llama.npy")),
];

/// NURBS_KEYS = [p.stem for p in NURBS_BASE_PATH.iterdir()]: directory order of the reference machine (APFS hash order), filtered by prefix
const KEY_ORDER: [&str; 30] = [
    "body_feline_wolf", "body_feline_housecat", "body_feline_cheetah", "head_herbivore_goat", "body_bird_gull", "body_fish_pufferfish", "body_herbivore_giraffe",
    "body_bird_duck", "head_carnivore_wolf", "body_fish_bluefish", "body_insect_tarantula", "body_insect_bee", "body_fish_eel", "body_fish_pickerel",
    "head_insect_wasp", "head_herbivore_giraffe", "body_fish_crappie", "body_herbivore_goat", "body_insect_beetle", "body_feline_tiger_2", "body_bird_robin",
    "body_llama", "head_herbivore_cow", "head_herbivore_llama", "body_herbivore_cow", "head_carnivore_tiger", "body_feline_tiger", "body_fish_spadefish",
    "head_insect_beetle", "body_herbivore_llama",
];

pub fn keys_with_prefix(prefix: &str) -> Vec<&'static str> {
    KEY_ORDER.iter().copied().filter(|k| k.starts_with(prefix)).collect()
}

/// np.load(...)[..., :3] -> (n, m) points
pub fn load_nurbs(name: &str) -> Vec<Vec<V3>> {
    let b = NPY.iter().find(|(k, _)| *k == name).unwrap_or_else(|| panic!("nurbs {name}")).1;
    let hl = u16::from_le_bytes([b[8], b[9]]) as usize;
    let hdr = std::str::from_utf8(&b[10..10 + hl]).unwrap();
    let sh = hdr.split("'shape': (").nth(1).unwrap().split(')').next().unwrap();
    let dims: Vec<usize> = sh.split(',').filter(|s| !s.trim().is_empty()).map(|s| s.trim().parse().unwrap()).collect();
    let (n, m, d) = (dims[0], dims[1], dims[2]);
    let data = &b[10 + hl..];
    let f = |i: usize| f64::from_le_bytes(data[8 * i..8 * i + 8].try_into().unwrap());
    (0..n).map(|i| (0..m).map(|j| [f((i * m + j) * d), f((i * m + j) * d + 1), f((i * m + j) * d + 2)]).collect()).collect()
}

// ------------------------------------------------------------------------------------------------ lofting helpers (float32 where numpy is)

type F3 = [f32; 3];

fn norm32(a: F3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// lofting.skeleton_to_tangents (float32 array)
pub fn skeleton_to_tangents(sk: &[V3]) -> Vec<F3> {
    let n = sk.len();
    let mut ax = vec![[0f32; 3]; n];
    let d = |a: usize, b: usize| -> F3 { [(sk[a][0] - sk[b][0]) as f32, (sk[a][1] - sk[b][1]) as f32, (sk[a][2] - sk[b][2]) as f32] };
    ax[n - 1] = d(n - 1, n - 2);
    for i in 0..n - 1 {
        ax[i] = d(i + 1, i);
    }
    let old = ax.clone();
    for i in 1..n - 1 {
        for k in 0..3 {
            ax[i][k] = (old[i][k] + old[i - 1][k]) / 2.0;
        }
    }
    for a in ax.iter_mut() {
        let nr = norm32(*a);
        if nr > 0.0 {
            for k in 0..3 {
                a[k] /= nr;
            }
        }
    }
    ax
}

fn cross32(a: F3, b: F3) -> F3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// math.rotate_match_directions (float32 inputs, float64 rotation matrices)
pub fn rotate_match_directions(a: &[F3], b: &[F3]) -> Vec<[[f64; 3]; 3]> {
    a.iter()
        .zip(b.iter())
        .map(|(&a, &b)| {
            let ax = cross32(a, b);
            if norm32(ax) as f64 > 1e-4 {
                let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                dot /= norm32(a) * norm32(b);
                rodrigues(dot.acos(), ax)
            } else {
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            }
        })
        .collect()
}

fn rodrigues(angle: f32, axi: F3) -> [[f64; 3]; 3] {
    let nr = norm32(axi);
    let u = [axi[0] / nr, axi[1] / nr, axi[2] / nr];
    let k: [[f32; 3]; 3] = [[0.0, -u[2], u[1]], [u[2], 0.0, -u[0]], [-u[1], u[0], 0.0]];
    let mut kk = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            kk[i][j] = k[i][0] * k[0][j] + k[i][1] * k[1][j] + k[i][2] * k[2][j];
        }
    }
    let (s, c) = (angle.sin(), angle.cos());
    let mut r = [[0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let id = if i == j { 1.0 } else { 0.0 };
            r[i][j] = id + ((s * k[i][j]) as f64) + (((1.0 - c) * kk[i][j]) as f64);
        }
    }
    r
}

fn mat_vec(m: &[[f64; 3]; 3], v: V3) -> V3 {
    [m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2], m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2], m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2]]
}

// ------------------------------------------------------------------------------------------------ parameters

#[derive(Clone, Debug)]
pub struct NurbsParams {
    pub ts: Vec<f64>,
    pub rads: Vec<f64>,
    pub skeleton_root: V3,
    pub skeleton_yoffs: Vec<f64>,
    pub length: f64,
    pub proportions: Vec<f64>,
    pub thetas: Vec<f64>,
    pub profiles_norm: Vec<Vec<V3>>,
}

/// generic_nurbs.decompose_nurbs_handles
pub fn decompose(handles: &[Vec<V3>]) -> NurbsParams {
    let n = handles.len();
    let m = handles[0].len();
    let sk: Vec<V3> = handles
        .iter()
        .map(|r| {
            let mut s = [0.0; 3];
            for p in r {
                for k in 0..3 {
                    s[k] += p[k];
                }
            }
            [s[0] / m as f64, s[1] / m as f64, s[2] / m as f64]
        })
        .collect();
    let tang = skeleton_to_tangents(&sk);
    let fwd = vec![[1f32, 0.0, 0.0]; n];
    let rots = rotate_match_directions(&tang, &fwd);
    let ts: Vec<f64> = (0..n).map(|i| if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 }).collect();
    let mut profiles: Vec<Vec<V3>> = vec![];
    for i in 0..n {
        profiles.push(handles[i].iter().map(|p| mat_vec(&rots[i], [p[0] - sk[i][0], p[1] - sk[i][1], p[2] - sk[i][2]])).collect());
    }
    let rads: Vec<f64> = profiles
        .iter()
        .map(|r| {
            let s: f64 = r.iter().map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()).sum();
            (s / m as f64).clamp(1e-3, 1e5)
        })
        .collect();
    let profiles_norm: Vec<Vec<V3>> = profiles.iter().zip(rads.iter()).map(|(r, &d)| r.iter().map(|p| [p[0] / d, p[1] / d, p[2] / d]).collect()).collect();
    let dirs: Vec<V3> = (0..n - 1).map(|i| [sk[i + 1][0] - sk[i][0], sk[i + 1][1] - sk[i][1], sk[i + 1][2] - sk[i][2]]).collect();
    let lens: Vec<f64> = dirs.iter().map(|d| (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()).collect();
    let length: f64 = lens.iter().sum();
    NurbsParams {
        ts,
        rads,
        skeleton_root: sk[0],
        skeleton_yoffs: dirs.iter().zip(lens.iter()).map(|(d, l)| d[1] / l).collect(),
        length,
        proportions: lens.iter().map(|l| l / length).collect(),
        thetas: dirs.iter().map(|d| d[2].atan2(d[0]).to_degrees()).collect(),
        profiles_norm,
    }
}

/// generic_nurbs.NurbsPart.sample_params (np.random draw order); `temps[k]` = dirichlet concentration of key k
pub fn sample_nurbs(r: &mut Rng, prefix: &str, temps: &dyn Fn(&str) -> f64, var: f64) -> NurbsParams {
    let keys = keys_with_prefix(prefix);
    let alpha: Vec<f64> = keys.iter().map(|k| temps(k)).collect();
    let w = dirichlet(r, &alpha);
    let mut handles: Vec<Vec<V3>> = vec![];
    for (k, wk) in keys.iter().zip(w.iter()) {
        let h = load_nurbs(k);
        if handles.is_empty() {
            handles = h.iter().map(|row| row.iter().map(|p| [0.0 + wk * p[0], 0.0 + wk * p[1], 0.0 + wk * p[2]]).collect()).collect();
        } else {
            for (row, hr) in handles.iter_mut().zip(h.iter()) {
                for (p, q) in row.iter_mut().zip(hr.iter()) {
                    for c in 0..3 {
                        p[c] += wk * q[c];
                    }
                }
            }
        }
    }
    let mut d = decompose(&handles);
    let sz = r.normal(1.0, 0.1 * var);
    let a = r.normal(1.0, 0.1 * var);
    d.length *= sz * a;
    let n_rad = d.rads.len();
    let b = r.normal(1.0, 0.1 * var);
    let noise: Vec<f64> = (0..n_rad).map(|_| r.normal(1.0, 0.15 * var)).collect();
    for (i, rd) in d.rads.iter_mut().enumerate() {
        *rd *= sz * b * noise[i];
    }
    let pr = r.normal(1.0, 0.15 * var);
    for p in d.proportions.iter_mut() {
        *p *= pr;
    }
    let nt = d.thetas.len();
    let mut ang: Vec<f64> = (0..nt).map(|_| r.normal(0.0, 7.0 * var)).collect();
    let mean = ang.iter().sum::<f64>() / nt as f64;
    for a in ang.iter_mut() {
        *a -= mean;
    }
    for (t, a) in d.thetas.iter_mut().zip(ang.iter()) {
        *t += a;
    }
    let n = d.profiles_norm.len();
    let m = d.profiles_norm[0].len();
    let n1: Vec<f64> = (0..m).map(|_| r.normal(1.0, 0.07 * var)).collect();
    let n2: Vec<f64> = (0..n * m).map(|_| r.normal(1.0, 0.15 * var)).collect();
    let mut pn: Vec<Vec<f64>> = (0..n).map(|i| (0..m).map(|j| n1[j] * n2[i * m + j]).collect()).collect();
    for row in pn.iter_mut() {
        let old = row.clone();
        for i in 0..(m / 2 - 1) {
            row[i] = old[m - 2 - i];
        }
    }
    for i in 0..n {
        for j in 0..m {
            for c in 0..3 {
                d.profiles_norm[i][j][c] *= pn[i][j];
            }
        }
    }
    d
}

/// generic_nurbs.recompose_nurbs_handles
pub fn recompose(p: &NurbsParams) -> Vec<Vec<V3>> {
    let lens: Vec<f64> = p.proportions.iter().map(|x| p.length * x).collect();
    let mut sk: Vec<V3> = vec![p.skeleton_root];
    for (i, l) in lens.iter().enumerate() {
        let th = p.thetas[i].to_radians();
        let off = [l * th.cos(), l * p.skeleton_yoffs[i], l * th.sin()];
        let last = *sk.last().unwrap();
        sk.push([last[0] + off[0], last[1] + off[1], last[2] + off[2]]);
    }
    let n = p.ts.len();
    let k = sk.len();
    let tang = skeleton_to_tangents(&sk);
    // lerp_sample at ts * (k - 1): float32 arithmetic on a float32 copy
    let sk32: Vec<F3> = sk.iter().map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]).collect();
    let lerp = |vec: &[F3], t: f32| -> F3 {
        let idx = (t.floor() as i64).clamp(0, vec.len() as i64 - 1) as usize;
        let rem = t - idx as f32;
        let mut res = vec[idx];
        if idx < vec.len() - 1 {
            for c in 0..3 {
                res[c] = (1.0 - rem) * res[c] + rem * vec[idx + 1][c];
            }
        }
        res
    };
    let axes: Vec<F3> = p.ts.iter().map(|&t| lerp(&tang, (t as f32) * ((k - 1) as f32))).collect();
    let pos: Vec<F3> = p.ts.iter().map(|&t| lerp(&sk32, (t as f32) * ((k - 1) as f32))).collect();
    let fwd = vec![[1f32, 0.0, 0.0]; n];
    let rots = rotate_match_directions(&fwd, &axes);
    (0..n)
        .map(|i| {
            p.profiles_norm[i]
                .iter()
                .map(|q| {
                    let v = mat_vec(&rots[i], [q[0] * p.rads[i], q[1] * p.rads[i], q[2] * p.rads[i]]);
                    [v[0] + pos[i][0] as f64, v[1] + pos[i][1] as f64, v[2] + pos[i][2] as f64]
                })
                .collect()
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------ geomdl surface

fn linspace(start: f64, stop: f64, num: usize) -> Vec<f64> {
    if (start - stop).abs() <= 10e-8 {
        return vec![start];
    }
    if num > 1 {
        let div = (num - 1) as f64;
        let delta = stop - start;
        return (0..num).map(|x| start + (x as f64) * delta / div).collect();
    }
    vec![start]
}

fn find_span_linear(degree: usize, kv: &[f64], num: usize, knot: f64) -> usize {
    let mut span = degree + 1;
    while span < num && kv[span] <= knot {
        span += 1;
    }
    span - 1
}

fn basis_function(degree: usize, kv: &[f64], span: usize, knot: f64) -> Vec<f64> {
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    let mut nn = vec![1.0; degree + 1];
    for j in 1..=degree {
        left[j] = knot - kv[span + 1 - j];
        right[j] = kv[span + j] - knot;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = nn[r] / (right[r + 1] + left[j - r]);
            nn[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        nn[j] = saved;
    }
    nn
}

fn generate_knotvector(degree: usize, n: usize, clamped: bool) -> Vec<f64> {
    // mode "uniform"
    let mut knot: Vec<f64>;
    if clamped {
        // np.linspace(0, n, n - degree + 1)[1:-1]
        let cnt = n - degree + 1;
        let mid: Vec<f64> = (1..cnt - 1).map(|i| np_linspace_at(0.0, n as f64, cnt, i)).collect();
        knot = vec![0.0; degree + 1];
        knot.extend(mid);
        knot.extend(vec![n as f64; degree + 1]);
    } else {
        knot = (0..n + degree + 1).map(|i| i as f64).collect();
    }
    let mx = knot.iter().cloned().fold(f64::MIN, f64::max);
    knot.iter().map(|k| k / mx).collect()
}

fn np_linspace_at(start: f64, stop: f64, num: usize, i: usize) -> f64 {
    let div = (num - 1) as f64;
    let step = (stop - start) / div;
    if i == num - 1 {
        stop
    } else {
        start + (i as f64) * step
    }
}

/// nurbs.geomdl_nurbs: cyclic-v cubic surface over (n, m) control points sampled with `delta`; returns (points (nu*(nv-1)), nu, nv-1)
pub fn geomdl_nurbs(ctrl: &[Vec<V3>], delta: f64) -> (Vec<V3>, usize, usize) {
    let n = ctrl.len();
    let m = ctrl[0].len();
    let (du, dv) = (3usize, 3usize);
    let kv_u = generate_knotvector(du, n, true);
    let mut kv_v = generate_knotvector(dv, m, false);
    let first = kv_v[0];
    let last = *kv_v.last().unwrap();
    for i in 1..=dv {
        kv_v.push(kv_v[i] + last - first);
    }
    let size_v = m + dv;
    // control points with the first dv columns repeated; flat index idx_v + l + size_v * (idx_u + k)
    let cp = |i: usize, j: usize| ctrl[i][j % m];
    let ss = (1.0 / delta + 0.5).floor() as usize;
    let (su, sv) = (ss, ss);
    let (start_u, stop_u) = (kv_u[du], kv_u[kv_u.len() - (du + 1)]);
    let (start_v, stop_v) = (kv_v[dv], kv_v[kv_v.len() - (dv + 1)]);
    let ku = linspace(start_u, stop_u, su);
    let kvv = linspace(start_v, stop_v, sv);
    let spans_u: Vec<usize> = ku.iter().map(|&k| find_span_linear(du, &kv_u, n, k)).collect();
    let spans_v: Vec<usize> = kvv.iter().map(|&k| find_span_linear(dv, &kv_v, size_v, k)).collect();
    let bu: Vec<Vec<f64>> = spans_u.iter().zip(ku.iter()).map(|(&s, &k)| basis_function(du, &kv_u, s, k)).collect();
    let bv: Vec<Vec<f64>> = spans_v.iter().zip(kvv.iter()).map(|(&s, &k)| basis_function(dv, &kv_v, s, k)).collect();
    let mut pts = vec![];
    let (nu, nvv) = (spans_u.len(), spans_v.len());
    for i in 0..nu {
        let idx_u = spans_u[i] - du;
        for j in 0..nvv {
            let idx_v = spans_v[j] - dv;
            let mut spt = [0.0f64; 4];
            for k in 0..=du {
                let mut temp = [0.0f64; 4];
                for l in 0..=dv {
                    let c = cp(idx_u + k, idx_v + l);
                    let w = [c[0], c[1], c[2], 1.0];
                    for q in 0..4 {
                        temp[q] += bv[j][l] * w[q];
                    }
                }
                for q in 0..4 {
                    spt[q] += bu[i][k] * temp[q];
                }
            }
            pts.push([spt[0] / spt[3], spt[1] / spt[3], spt[2] / spt[3]]);
        }
    }
    // drop the duplicated last column of every row (cyclic v)
    let keep = nvv - 1;
    let mut out = Vec::with_capacity(nu * keep);
    for i in 0..nu {
        for j in 0..keep {
            out.push(pts[i * nvv + j]);
        }
    }
    (out, nu, keep)
}

/// nurbs.nurbs(method="geomdl", face_size): mesh points + quad faces of compute_cylinder_topology(nu, nv, cyclic)
pub fn nurbs_mesh(ctrl: &[Vec<V3>], face_size: f64) -> (Vec<V3>, Vec<Vec<u32>>) {
    let n = ctrl.len();
    let m = ctrl[0].len();
    let dist = |a: V3, b: V3| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let mut ulength: f64 = 0.0;
    for j in 0..m {
        let s: f64 = (0..n - 1).map(|i| dist(ctrl[i + 1][j], ctrl[i][j])).sum();
        ulength = ulength.max(s);
    }
    let mut vlength: f64 = 0.0;
    for i in 0..n {
        let s: f64 = (0..m - 1).map(|j| dist(ctrl[i][j + 1], ctrl[i][j])).sum();
        vlength = vlength.max(s);
    }
    let steps = face_size / ulength.max(vlength);
    let (pts, nu, nv) = geomdl_nurbs(ctrl, steps);
    let mut faces = vec![];
    for i in 0..nu - 1 {
        for j in 0..nv {
            let j1 = (j + 1) % nv;
            faces.push(vec![(i * nv + j) as u32, (i * nv + j1) as u32, (i * nv + j1 + nv) as u32, (i * nv + j + nv) as u32]);
        }
    }
    (pts, faces)
}

/// part_util.nurbs_to_part skeleton: handles.mean(axis=1)[1:-1] as a wire mesh subdivided twice (linear on loose edges), walked from vertex 0 with the first point duplicated
pub fn part_skeleton(handles: &[Vec<V3>]) -> Vec<V3> {
    let n = handles.len();
    let m = handles[0].len();
    let sk: Vec<V3> = handles
        .iter()
        .map(|r| {
            let mut s = [0.0; 3];
            for p in r {
                for k in 0..3 {
                    s[k] += p[k];
                }
            }
            [(s[0] / m as f64) as f32 as f64, (s[1] / m as f64) as f32 as f64, (s[2] / m as f64) as f32 as f64]
        })
        .collect();
    let sk = &sk[1..n - 1];
    // SUBSURF levels=2 on a wire mesh with use_limit_surface: boundary-curve Catmull-Clark (corner ends) twice, then the limit mask (P- + 4P + P+) / 6
    let cc = |p: &[V3]| -> Vec<V3> {
        let n = p.len();
        let mut out = vec![];
        for i in 0..n {
            if i == 0 || i == n - 1 {
                out.push(p[i]);
            } else {
                out.push([0, 1, 2].map(|c| (6.0 * p[i][c] + p[i - 1][c] + p[i + 1][c]) / 8.0));
            }
            if i < n - 1 {
                out.push([0, 1, 2].map(|c| (p[i][c] + p[i + 1][c]) / 2.0));
            }
        }
        out
    };
    let r = cc(&cc(sk));
    let mut fine = r.clone();
    for j in 1..r.len() - 1 {
        fine[j] = [0, 1, 2].map(|c| (r[j - 1][c] + 4.0 * r[j][c] + r[j + 1][c]) / 6.0);
    }
    let fine: Vec<V3> = fine.iter().map(|p| [p[0] as f32 as f64, p[1] as f32 as f64, p[2] as f32 as f64]).collect();
    // first point repeated, last dropped
    let mut out = vec![fine[0]];
    out.extend_from_slice(&fine[..fine.len() - 1]);
    out
}
