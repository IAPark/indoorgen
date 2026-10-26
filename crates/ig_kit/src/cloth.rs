//! Native cloth simulation (XPBD) approximating Blender's CLOTH modifier as driven by infinigen `clothes.cloth_sim`:
//! structural/shear/bending springs, uniform pressure, vertex-group pins, gravity, static mesh colliders (BVH, sweep test,
//! friction) and optional self collision. Bit-exactness with Blender is not a goal; see DONE.md for the similarity numbers.
use crate::mesh::V3;

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
fn unit(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-300 { mul(a, 1.0 / l) } else { [0.0, 0.0, 1.0] }
}

// ---------------------------------------------------------------- BVH over static triangles

#[derive(Clone)]
struct Node {
    lo: V3,
    hi: V3,
    /// leaf: first triangle in `order`, count>0; inner: left child = self+1, right child = `right`
    first: u32,
    count: u32,
    right: u32,
}

pub struct Collider {
    tris: Vec<[V3; 3]>,
    normals: Vec<V3>,
    order: Vec<u32>,
    nodes: Vec<Node>,
}

fn closest_on_tri(p: V3, t: &[V3; 3]) -> V3 {
    // Ericson, Real-Time Collision Detection
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add(a, mul(ab, v));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add(a, mul(ac, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, mul(sub(c, b), w));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    add(a, add(mul(ab, v), mul(ac, w)))
}

impl Collider {
    /// `faces` may be n-gons (fan-triangulated); winding is expected outward (Blender convention).
    pub fn new(pos: &[V3], faces: &[Vec<u32>]) -> Collider {
        let mut tris = vec![];
        for f in faces {
            for k in 1..f.len().saturating_sub(1) {
                tris.push([pos[f[0] as usize], pos[f[k] as usize], pos[f[k + 1] as usize]]);
            }
        }
        let normals: Vec<V3> = tris.iter().map(|t| unit(cross(sub(t[1], t[0]), sub(t[2], t[0])))).collect();
        let mut c = Collider { tris, normals, order: vec![], nodes: vec![] };
        let n = c.tris.len();
        c.order = (0..n as u32).collect();
        if n > 0 {
            let cent: Vec<V3> = c.tris.iter().map(|t| mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)).collect();
            c.build(0, n, &cent);
        }
        c
    }
    fn build(&mut self, s: usize, e: usize, cent: &[V3]) -> usize {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &ti in &self.order[s..e] {
            for v in &self.tris[ti as usize] {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let id = self.nodes.len();
        self.nodes.push(Node { lo, hi, first: s as u32, count: (e - s) as u32, right: 0 });
        if e - s <= 4 {
            return id;
        }
        let ext = sub(hi, lo);
        let ax = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
        self.order[s..e].sort_unstable_by(|&a, &b| cent[a as usize][ax].partial_cmp(&cent[b as usize][ax]).unwrap());
        let m = (s + e) / 2;
        self.nodes[id].count = 0;
        self.build(s, m, cent);
        let r = self.build(m, e, cent);
        self.nodes[id].right = r as u32;
        id
    }
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }
    /// closest point on the surface within radius `r`: (dist, point, triangle index)
    pub fn closest(&self, p: V3, r: f64) -> Option<(f64, V3, usize)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best = r * r;
        let mut res = None;
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let n = &self.nodes[ni];
            let mut d2 = 0.0;
            for k in 0..3 {
                let d = (n.lo[k] - p[k]).max(p[k] - n.hi[k]).max(0.0);
                d2 += d * d;
            }
            if d2 > best {
                continue;
            }
            if n.count > 0 {
                for j in n.first..n.first + n.count {
                    let ti = self.order[j as usize] as usize;
                    let q = closest_on_tri(p, &self.tris[ti]);
                    let dd = len(sub(p, q));
                    if dd * dd < best || (res.is_none() && dd * dd <= best) {
                        best = dd * dd;
                        res = Some((dd, q, ti));
                    }
                }
            } else {
                stack.push(ni + 1);
                stack.push(n.right as usize);
            }
        }
        res
    }
    /// first front-face hit of segment a->b: (t, triangle index)
    pub fn segment_hit(&self, a: V3, b: V3) -> Option<(f64, usize)> {
        if self.nodes.is_empty() {
            return None;
        }
        let d = sub(b, a);
        let mut best = 1.0f64;
        let mut res = None;
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let n = &self.nodes[ni];
            // slab test
            let (mut t0, mut t1) = (0.0f64, best);
            let mut ok = true;
            for k in 0..3 {
                if d[k].abs() < 1e-18 {
                    if a[k] < n.lo[k] || a[k] > n.hi[k] {
                        ok = false;
                        break;
                    }
                } else {
                    let inv = 1.0 / d[k];
                    let (mut ta, mut tb) = ((n.lo[k] - a[k]) * inv, (n.hi[k] - a[k]) * inv);
                    if ta > tb {
                        std::mem::swap(&mut ta, &mut tb);
                    }
                    t0 = t0.max(ta);
                    t1 = t1.min(tb);
                    if t0 > t1 {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            if n.count > 0 {
                for j in n.first..n.first + n.count {
                    let ti = self.order[j as usize] as usize;
                    let nrm = self.normals[ti];
                    let den = dot(nrm, d);
                    if den >= 0.0 {
                        continue; // only entering the front side
                    }
                    let t = &self.tris[ti];
                    let tt = dot(nrm, sub(t[0], a)) / den;
                    if !(0.0..best).contains(&tt) {
                        continue;
                    }
                    let q = add(a, mul(d, tt));
                    // barycentric inside test
                    let (v0, v1, v2) = (sub(t[1], t[0]), sub(t[2], t[0]), sub(q, t[0]));
                    let (d00, d01, d11, d20, d21) = (dot(v0, v0), dot(v0, v1), dot(v1, v1), dot(v2, v0), dot(v2, v1));
                    let den2 = d00 * d11 - d01 * d01;
                    if den2.abs() < 1e-30 {
                        continue;
                    }
                    let v = (d11 * d20 - d01 * d21) / den2;
                    let w = (d00 * d21 - d01 * d20) / den2;
                    if v >= -1e-9 && w >= -1e-9 && v + w <= 1.0 + 1e-9 {
                        best = tt;
                        res = Some((tt, ti));
                    }
                }
            } else {
                stack.push(ni + 1);
                stack.push(n.right as usize);
            }
        }
        res
    }
    pub fn normal(&self, ti: usize) -> V3 {
        self.normals[ti]
    }
}

// ---------------------------------------------------------------- parameters

/// Blender CLOTH modifier settings used by infinigen (units: simulation time is measured in frames, 1 m, N-like forces)
#[derive(Clone, Debug)]
pub struct ClothParams {
    pub mass: f64,
    pub tension: f64,
    pub compression: f64,
    pub shear: f64,
    pub bending: f64,
    pub tension_damp: f64,
    pub compression_damp: f64,
    pub shear_damp: f64,
    pub air_damping: f64,
    /// effector gravity weight (0 disables gravity)
    pub gravity: f64,
    pub distance_min: f64,
    pub use_self_collision: bool,
    pub self_distance: f64,
    pub use_pressure: bool,
    pub pressure: f64,
    /// > 0: the pressure fades out linearly as the enclosed volume approaches this value (m^3), i.e. a bounded gas pocket instead of an unbounded push
    pub volume_cap: f64,
    pub end_frame: u32,
    pub quality: u32,
    /// per vertex pin weight (>= 0.999 = fixed); empty = none
    pub pin: Vec<f64>,
}

impl Default for ClothParams {
    /// Blender 4.2 cloth defaults
    fn default() -> Self {
        ClothParams {
            mass: 0.3,
            tension: 15.0,
            compression: 15.0,
            shear: 5.0,
            bending: 0.5,
            tension_damp: 5.0,
            compression_damp: 5.0,
            shear_damp: 5.0,
            air_damping: 1.0,
            gravity: 1.0,
            distance_min: 0.015,
            use_self_collision: false,
            self_distance: 0.015,
            use_pressure: false,
            pressure: 0.0,
            volume_cap: 0.0,
            end_frame: 50,
            quality: 5,
            pin: vec![],
        }
    }
}

/// tunables found by calibrating against Blender (see tools/clothexp*.py and the clothcmp bin)
#[derive(Clone, Debug)]
pub struct Calib {
    /// acceleration of gravity in m/frame^2 (Blender: 9.81 * 0.001)
    pub g: f64,
    /// air drag force per velocity = Cvi * drag_scale
    pub drag_scale: f64,
    pub k_scale: f64,
    pub press_scale: f64,
    /// pressure shaping: P_eff = press_scale * max(P - press_thresh, 0)^press_pow (pow 1, thresh 0 = plain pressure)
    pub press_thresh: f64,
    /// extra pressure threshold when gravity is on (calibrated on Blender comforter-on-box scenes)
    pub press_thresh_g: f64,
    pub press_pow: f64,
    pub d_scale: f64,
    /// collision object outer thickness (Blender default 0.02)
    pub col_thickness: f64,
    pub friction: f64,
    /// max speed (m/frame) of the penetration resolution
    pub push_speed: f64,
    /// fraction of the outgoing (separating) normal velocity removed per step inside the contact zone (collision damping + stickiness)
    pub sticky: f64,
    pub cg_iters: usize,
    pub cg_tol: f64,
    pub substeps_mult: u32,
    /// pressure only acts on vertices with a collider surface this far below them (m); <= 0 disables
    pub support_reach: f64,
    pub sweep: bool,
    pub edge_probes: bool,
    pub final_project: bool,
    pub revert_behind: bool,
    /// substeps per frame (0 = cloth quality x substeps_mult)
    pub steps: u32,
}

impl Default for Calib {
    fn default() -> Self {
        let e = |k: &str, d: f64| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
        Calib {
            g: e("CL_G", 0.00981),
            drag_scale: e("CL_DRAG", 0.01),
            k_scale: e("CL_K", 1.0),
            press_scale: e("CL_PRESS", 1.0),
            press_thresh: e("CL_PTHR", 0.0),
            press_thresh_g: e("CL_PTHRG", 0.6),
            press_pow: e("CL_PPOW", 1.0),
            d_scale: e("CL_D", 1.0),
            col_thickness: e("CL_COL", 0.035),
            friction: e("CL_FRIC", 10.0),
            push_speed: e("CL_PUSH", 0.05),
            sticky: e("CL_STICKY", 0.9),
            cg_iters: e("CL_CG", 40.0) as usize,
            cg_tol: e("CL_TOL", 1e-4),
            substeps_mult: e("CL_SUBM", 1.0) as u32,
            support_reach: e("CL_SUPPORT", 0.12),
            sweep: e("CL_SWEEP", 0.0) > 0.5,
            edge_probes: e("CL_EDGE", 0.0) > 0.5,
            final_project: e("CL_FINAL", 1.0) > 0.5,
            revert_behind: e("CL_REVERT", 0.0) > 0.5,
            steps: e("CL_STEPS", 2.0) as u32,
        }
    }
}

struct Spring {
    i: u32,
    j: u32,
    l0: f64,
    kind: u8, // 0 structural 1 shear 2 bending
}

fn build_springs(pos: &[V3], faces: &[Vec<u32>]) -> Vec<Spring> {
    use crate::dhash::{HashMap, HashSet};
    let mut seen: HashSet<(u32, u32)> = HashSet::default();
    let mut out = vec![];
    let push = |out: &mut Vec<Spring>, seen: &mut HashSet<(u32, u32)>, a: u32, b: u32, kind: u8| {
        if a == b {
            return;
        }
        let k = (a.min(b), a.max(b));
        if seen.insert(k) {
            out.push(Spring { i: k.0, j: k.1, l0: len(sub(pos[a as usize], pos[b as usize])), kind });
        }
    };
    for f in faces {
        let n = f.len();
        for k in 0..n {
            push(&mut out, &mut seen, f[k], f[(k + 1) % n], 0);
        }
    }
    for f in faces {
        if f.len() == 4 {
            push(&mut out, &mut seen, f[0], f[2], 1);
            push(&mut out, &mut seen, f[1], f[3], 1);
        }
    }
    // bending: link opposite neighbours of every vertex (anti-parallel edges at rest) -> skip-one springs on grids
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::default();
    for s in out.iter().filter(|s| s.kind == 0) {
        adj.entry(s.i).or_default().push(s.j);
        adj.entry(s.j).or_default().push(s.i);
    }
    let mut keys: Vec<u32> = adj.keys().copied().collect();
    keys.sort();
    for v in keys {
        let nb = &adj[&v];
        for a in 0..nb.len() {
            for b in a + 1..nb.len() {
                let (da, db) = (sub(pos[nb[a] as usize], pos[v as usize]), sub(pos[nb[b] as usize], pos[v as usize]));
                let (la, lb) = (len(da), len(db));
                if la > 1e-12 && lb > 1e-12 && dot(da, db) / (la * lb) < -0.98 {
                    push(&mut out, &mut seen, nb[a], nb[b], 2);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- simulation

/// vertex-spread pressure forces: every polygon contributes (vector area * pressure) split equally between its vertices
fn pressure_forces(x: &[V3], faces: &[Vec<u32>], p: f64, out: &mut [V3]) {
    for o in out.iter_mut() {
        *o = [0.0; 3];
    }
    for f in faces {
        let n = f.len();
        let mut a = [0.0; 3];
        for k in 0..n {
            a = add(a, cross(x[f[k] as usize], x[f[(k + 1) % n] as usize]));
        }
        let a = mul(a, 0.5 * p / n as f64);
        for &v in f {
            out[v as usize] = add(out[v as usize], a);
        }
    }
}

/// When set (gen2 default), `simulate_job` returns the pre-sim positions: the cloth is exported as a sidecar job and re-simulated in Blender at render time.
pub static SKIP_JOB_SIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `simulate` for cloth that is also exported as a sidecar job (mattress, sheet, cover); skipped when SKIP_JOB_SIM is set
pub fn simulate_job(pos0: &[V3], faces: &[Vec<u32>], p: &ClothParams, colliders: &[&Collider], cal: &Calib) -> Vec<V3> {
    if SKIP_JOB_SIM.load(std::sync::atomic::Ordering::Relaxed) {
        return pos0.to_vec();
    }
    simulate(pos0, faces, p, colliders, cal)
}

pub fn simulate(pos0: &[V3], faces: &[Vec<u32>], p: &ClothParams, colliders: &[&Collider], cal: &Calib) -> Vec<V3> {
    let t0 = std::time::Instant::now();
    let r = simulate_inner(pos0, faces, p, colliders, cal);
    if std::env::var_os("CLOTH_T").is_some() {
        eprintln!("CLOTH_T {} verts {} frames {:.2}s", pos0.len(), p.end_frame, t0.elapsed().as_secs_f64());
    }
    r
}

fn simulate_inner(pos0: &[V3], faces: &[Vec<u32>], p: &ClothParams, colliders: &[&Collider], cal: &Calib) -> Vec<V3> {
    let n = pos0.len();
    let springs = build_springs(pos0, faces);
    let avg_len = {
        let s: Vec<f64> = springs.iter().filter(|s| s.kind == 0).map(|s| s.l0).collect();
        s.iter().sum::<f64>() / s.len().max(1) as f64
    };
    let steps_per_frame = if cal.steps > 0 { cal.steps } else { (p.quality * cal.substeps_mult).max(1) };
    let dt = 1.0 / steps_per_frame as f64;
    let frames = p.end_frame.saturating_sub(1);
    let mut x: Vec<V3> = pos0.to_vec();
    let mut v: Vec<V3> = vec![[0.0; 3]; n];
    let pinned: Vec<bool> = (0..n).map(|i| p.pin.get(i).copied().unwrap_or(0.0) >= 0.999).collect();
    let m = p.mass;
    let g = [0.0, 0.0, -cal.g * p.gravity];
    let drag = p.air_damping * cal.drag_scale;
    let r_col = p.distance_min + cal.col_thickness;
    let ns = springs.len();
    // per spring scratch
    let mut dirs = vec![[0.0f64; 3]; ns];
    let mut alpha = vec![0.0f64; ns];
    let mut beta = vec![0.0f64; ns];
    let mut forces = vec![[0.0f64; 3]; n];
    let mut pf = vec![[0.0f64; 3]; n];
    let mut pw = vec![1.0f64; n];
    let mut b = vec![[0.0f64; 3]; n];
    let mut dv = vec![[0.0f64; 3]; n];
    let mut r = vec![[0.0f64; 3]; n];
    let mut z = vec![[0.0f64; 3]; n];
    let mut pdir = vec![[0.0f64; 3]; n];
    let mut ap = vec![[0.0f64; 3]; n];
    let mut diag = vec![0.0f64; n];
    let mut act: Vec<(u32, u32, f64, f64, V3, u8)> = Vec::with_capacity(ns);
    for _step in 0..(frames * steps_per_frame) {
        for fo in forces.iter_mut() {
            *fo = [0.0; 3];
        }
        for i in 0..n {
            forces[i] = add(mul(g, m), mul(v[i], -drag));
        }
        if p.use_pressure && p.pressure != 0.0 && p.gravity > 0.0 && cal.support_reach > 0.0 && !colliders.is_empty() && (_step as u32) % steps_per_frame == 0 {
            // an inflated comforter only stays puffy where it is supported from below: the overhang deflates and hangs
            for i in 0..n {
                let a = x[i];
                let b = [a[0], a[1], a[2] - cal.support_reach];
                let hit = colliders.iter().any(|c| c.segment_hit(a, b).is_some());
                let target = if hit { 1.0 } else { 0.0 };
                pw[i] += (target - pw[i]) * 0.5;
            }
            // spatially smooth (pockets of the comforter must not deflate one by one: that leaves round pits)
            for _ in 0..3 {
                let mut acc = pw.clone();
                let mut cnt = vec![1.0f64; n];
                for s in springs.iter().filter(|s| s.kind == 0) {
                    acc[s.i as usize] += pw[s.j as usize];
                    acc[s.j as usize] += pw[s.i as usize];
                    cnt[s.i as usize] += 1.0;
                    cnt[s.j as usize] += 1.0;
                }
                for i in 0..n {
                    pw[i] = acc[i] / cnt[i];
                }
            }
        }
        if p.use_pressure && p.pressure != 0.0 {
            let vfac = if p.volume_cap > 0.0 {
                let mut vol = 0.0;
                for f in faces {
                    for k in 1..f.len() - 1 {
                        vol += dot(x[f[0] as usize], cross(x[f[k] as usize], x[f[k + 1] as usize]));
                    }
                }
                (1.0 - (vol / 6.0) / p.volume_cap).clamp(0.0, 1.0)
            } else {
                1.0
            };
            pressure_forces(&x, faces, vfac * cal.press_scale * (p.pressure.abs() - cal.press_thresh - if p.gravity > 0.0 { cal.press_thresh_g } else { 0.0 }).max(0.0).powf(cal.press_pow) * p.pressure.signum(), &mut pf);
            for i in 0..n {
                forces[i] = add(forces[i], mul(pf[i], pw[i]));
            }
        }
        for b_ in b.iter_mut() {
            *b_ = [0.0; 3];
        }
        for d_ in diag.iter_mut() {
            *d_ = m;
        }
        for (si, s) in springs.iter().enumerate() {
            let (i, j) = (s.i as usize, s.j as usize);
            let e = sub(x[j], x[i]);
            let l = len(e);
            if l < 1e-12 {
                alpha[si] = 0.0;
                beta[si] = 0.0;
                continue;
            }
            let dir = mul(e, 1.0 / l);
            dirs[si] = dir;
            let vrel = dot(sub(v[j], v[i]), dir);
            let ak = 1.0 / avg_len.max(1e-9) * cal.k_scale;
            let (k, d, bend) = match s.kind {
                0 => if l > s.l0 { (p.tension * ak, p.tension_damp * cal.d_scale, false) } else { (p.compression * ak, p.compression_damp * cal.d_scale, false) },
                1 => (p.shear * ak, p.shear_damp * cal.d_scale, false),
                _ => (p.bending * ak, 0.0, true),
            };
            if bend && l >= s.l0 {
                alpha[si] = 0.0;
                beta[si] = 0.0;
                continue;
            }
            let gfac = (1.0 - s.l0 / l).max(0.0);
            let f = (k * (l - s.l0) + d * vrel) * 1.0;
            let fv = mul(dir, f);
            forces[i] = add(forces[i], fv);
            forces[j] = sub(forces[j], fv);
            // stiffness jacobian K = k (g I + (1-g) dd^T); damping D = d dd^T
            let (ka, kb) = (k * gfac, k * (1.0 - gfac));
            alpha[si] = dt * dt * ka;
            beta[si] = dt * dt * kb + dt * d;
            let uv = sub(v[i], v[j]);
            let ku = add(mul(uv, ka), mul(dir, kb * dot(dir, uv)));
            b[i] = sub(b[i], mul(ku, dt * dt));
            b[j] = add(b[j], mul(ku, dt * dt));
            diag[i] += alpha[si] + beta[si];
            diag[j] += alpha[si] + beta[si];
        }
        for i in 0..n {
            if pinned[i] {
                b[i] = [0.0; 3];
            } else {
                b[i] = add(b[i], mul(forces[i], dt));
            }
        }
        // PCG for A dv = b, A = M + sum (alpha I + beta dd^T) [[1,-1],[-1,1]]
        // active springs of this step, compacted once (same order => bit-identical sums)
        act.clear();
        for (si, s) in springs.iter().enumerate() {
            if alpha[si] != 0.0 || beta[si] != 0.0 {
                act.push((s.i, s.j, alpha[si], beta[si], dirs[si], (pinned[s.i as usize] as u8) | ((pinned[s.j as usize] as u8) << 1)));
            }
        }
        let matvec = |xv: &[V3], out: &mut [V3]| {
            for i in 0..n {
                out[i] = if pinned[i] { [0.0; 3] } else { mul(xv[i], m) };
            }
            for &(i, j, al, be, dr, pf) in act.iter() {
                let (i, j) = (i as usize, j as usize);
                let (xi, xj) = (xv[i], xv[j]);
                let u = [xi[0] - xj[0], xi[1] - xj[1], xi[2] - xj[2]];
                let k = be * (dr[0] * u[0] + dr[1] * u[1] + dr[2] * u[2]);
                let w = [u[0] * al + dr[0] * k, u[1] * al + dr[1] * k, u[2] * al + dr[2] * k];
                if pf & 1 == 0 {
                    let o = &mut out[i];
                    o[0] += w[0];
                    o[1] += w[1];
                    o[2] += w[2];
                }
                if pf & 2 == 0 {
                    let o = &mut out[j];
                    o[0] -= w[0];
                    o[1] -= w[1];
                    o[2] -= w[2];
                }
            }
        };
        for i in 0..n {
            dv[i] = [0.0; 3];
            r[i] = b[i];
        }
        let bnorm: f64 = b.iter().map(|q| dot(*q, *q)).sum::<f64>().sqrt();
        if bnorm > 0.0 {
            for i in 0..n {
                z[i] = mul(r[i], 1.0 / diag[i]);
                pdir[i] = z[i];
            }
            let mut rz: f64 = (0..n).map(|i| dot(r[i], z[i])).sum();
            for _it in 0..cal.cg_iters {
                matvec(&pdir, &mut ap);
                let pap: f64 = (0..n).map(|i| dot(pdir[i], ap[i])).sum();
                if pap.abs() < 1e-300 {
                    break;
                }
                let a = rz / pap;
                let mut rn = 0.0;
                for i in 0..n {
                    dv[i] = add(dv[i], mul(pdir[i], a));
                    r[i] = sub(r[i], mul(ap[i], a));
                    rn += dot(r[i], r[i]);
                }
                if rn.sqrt() < cal.cg_tol * bnorm {
                    break;
                }
                let mut rz2 = 0.0;
                for i in 0..n {
                    z[i] = mul(r[i], 1.0 / diag[i]);
                    rz2 += dot(r[i], z[i]);
                }
                let bta = rz2 / rz;
                rz = rz2;
                for i in 0..n {
                    pdir[i] = add(z[i], mul(pdir[i], bta));
                }
            }
        }
        for i in 0..n {
            if !pinned[i] {
                v[i] = add(v[i], dv[i]);
            } else {
                v[i] = [0.0; 3];
            }
        }
        // collisions on the predicted positions: inelastic normal response, Coulomb friction on the tangential part,
        // penetration resolved by a limited position correction
        let mut corr = vec![[0.0f64; 3]; n];
        for i in 0..n {
            if pinned[i] {
                continue;
            }
            // no tunnelling through thin colliders: a vertex moves at most 0.4 collision radii per step
            let vl = len(v[i]) * dt;
            if vl > 0.4 * r_col {
                v[i] = mul(v[i], 0.4 * r_col / vl);
            }
            let mut xp = add(x[i], mul(v[i], dt));
            for c in colliders {
                if c.is_empty() {
                    continue;
                }
                // sweep: a step that would cross a front face stops in front of it
                if let Some((t, ti)) = if cal.sweep { c.segment_hit(x[i], xp) } else { None } {
                    let nrm = c.normal(ti);
                    let hit = add(x[i], mul(sub(xp, x[i]), t));
                    xp = add(hit, mul(nrm, r_col * 0.9));
                    let vn = dot(v[i], nrm);
                    if vn < 0.0 {
                        v[i] = sub(v[i], mul(nrm, vn));
                    }
                    corr[i] = add(corr[i], sub(xp, add(x[i], mul(v[i], dt))));
                }
                if let Some((d, q, ti)) = c.closest(xp, r_col) {
                    let nrm = c.normal(ti);
                    let side = dot(sub(xp, q), nrm);
                    let dir = if side >= 0.0 && d > 1e-9 { mul(sub(xp, q), 1.0 / d) } else { nrm };
                    let dd = if side >= 0.0 { d } else { -d };
                    let pen = (r_col - dd).max(0.0);
                    if side < 0.0 && cal.revert_behind {
                        // went behind the surface (crease / edge pass): undo the move instead of popping out on a possibly wrong side
                        let vn = dot(v[i], nrm);
                        v[i] = sub(v[i], mul(nrm, vn.min(0.0)));
                        let back = sub(x[i], xp);
                        corr[i] = add(corr[i], back);
                        continue;
                    }
                    let vn = dot(v[i], dir);
                    let approach = (-vn).max(0.0);
                    let vt = sub(v[i], mul(dir, vn));
                    let vtl = len(vt);
                    let vt2 = if vtl > 1e-15 { mul(vt, ((vtl - cal.friction * approach).max(0.0)) / vtl) } else { vt };
                    v[i] = add(vt2, mul(dir, vn.max(0.0) * (1.0 - cal.sticky)));
                    corr[i] = add(corr[i], mul(dir, pen.min(cal.push_speed * dt)));
                }
            }
        }
        if cal.edge_probes && !colliders.is_empty() {
            // edge midpoints catch collider apexes poking between two cloth vertices
            for s in springs.iter().filter(|s| s.kind == 0) {
                let (i, j) = (s.i as usize, s.j as usize);
                if pinned[i] && pinned[j] {
                    continue;
                }
                let pm = mul(add(add(x[i], mul(v[i], dt)), add(x[j], mul(v[j], dt))), 0.5);
                for c in colliders {
                    if c.is_empty() {
                        continue;
                    }
                    if let Some((d, q, ti)) = c.closest(pm, r_col) {
                        let nrm = c.normal(ti);
                        let side = dot(sub(pm, q), nrm);
                        let dir = if side >= 0.0 && d > 1e-9 { mul(sub(pm, q), 1.0 / d) } else { nrm };
                        let dd = if side >= 0.0 { d } else { -d };
                        let pen = (r_col - dd).max(0.0).min(cal.push_speed * dt);
                        for k in [i, j] {
                            if pinned[k] {
                                continue;
                            }
                            let vn = dot(v[k], dir);
                            if vn < 0.0 {
                                v[k] = sub(v[k], mul(dir, vn));
                            }
                            corr[k] = add(corr[k], mul(dir, pen));
                        }
                    }
                }
            }
        }
        for i in 0..n {
            x[i] = add(add(x[i], mul(v[i], dt)), corr[i]);
        }
        if std::env::var("CL_DBG").is_ok() && (_step as u32) % steps_per_frame == 0 {
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for q in &x { for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); } }
            let vmax = v.iter().map(|q| len(*q)).fold(0.0, f64::max);
            let mut behind = 0;
            for q in &x { for c in colliders { if let Some((d, qq, ti)) = c.closest(*q, 0.05) { if dot(sub(*q, qq), c.normal(ti)) < 0.0 && d > 1e-6 { behind += 1; } } } }
            eprintln!("  behind {}", behind);
            eprintln!("frame {} z [{:.3},{:.3}] y [{:.3},{:.3}] x [{:.3},{:.3}] vmax {:.4}", _step as u32 / steps_per_frame + 2, lo[2], hi[2], lo[1], hi[1], lo[0], hi[0], vmax);
        }
    }
    // final clean-up: vertices that ended behind a collider face (slipped through a crease) are put back on its front side
    if cal.final_project {
        for i in 0..n {
            if pinned[i] {
                continue;
            }
            for c in colliders {
                if let Some((d, q, ti)) = c.closest(x[i], 0.12) {
                    let nrm = c.normal(ti);
                    if dot(sub(x[i], q), nrm) < 0.0 && d > 1e-9 {
                        x[i] = add(q, mul(nrm, r_col * 0.8));
                    }
                }
            }
        }
    }
    x
}

/// Rest a soft body (pillow, towel) on the surfaces of `col`: the body is moved vertically until its lowest region touches the highest
/// support point below it (gap `skin`), then the bottom `depth` metres of the body follow the surface below (shrink-wrap, at most `max_gap`
/// per vertex), so the body drapes over bumps instead of hovering over the low spots. Vertices without a surface below are left alone.
pub fn rest_and_wrap(pos: &mut [V3], col: &Collider, depth: f64, skin: f64, max_gap: f64) {
    if pos.is_empty() || col.is_empty() {
        return;
    }
    let lo = pos.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let below = |p: V3| -> Option<f64> {
        let a = [p[0], p[1], p[2] + 0.12];
        let b = [p[0], p[1], p[2] - 0.6];
        col.segment_hit(a, b).map(|(t, _)| a[2] + (b[2] - a[2]) * t)
    };
    let mut zs: Vec<Option<f64>> = vec![None; pos.len()];
    let mut min_gap = f64::INFINITY;
    let mut z_ref = 0.0;
    for (i, p) in pos.iter().enumerate() {
        if p[2] - lo < depth {
            zs[i] = below(*p);
            if let Some(z) = zs[i] {
                if p[2] - z - skin < min_gap {
                    min_gap = p[2] - z - skin;
                    z_ref = z;
                }
            }
        }
    }
    if !min_gap.is_finite() {
        return;
    }
    // rigid move so the lowest contact has exactly `skin` clearance
    for p in pos.iter_mut() {
        p[2] -= min_gap;
    }
    for (i, p) in pos.iter_mut().enumerate() {
        if let Some(z) = zs[i] {
            let h = p[2] - (lo - min_gap);
            let w = (1.0 - h / depth).clamp(0.0, 1.0);
            let w = w * w * (3.0 - 2.0 * w);
            // follow the surface profile below the contact point (a depression of the surface pulls the soft body down into it)
            let drop = (z_ref - z).clamp(0.0, max_gap);
            p[2] -= w * drop;
        }
    }
}

// ---------------------------------------------------------------- surface comparison

/// symmetric surface distance between two meshes: probes = vertices + 3 points per edge, distance to the other surface.
/// returns (mean, p95, max) over both directions combined
pub fn surface_distance(pa: &[V3], fa: &[Vec<u32>], pb: &[V3], fb: &[Vec<u32>]) -> (f64, f64, f64) {
    let ca = Collider::new(pa, fa);
    let cb = Collider::new(pb, fb);
    let mut d: Vec<f64> = vec![];
    let probes = |p: &[V3], f: &[Vec<u32>]| -> Vec<V3> {
        let mut out: Vec<V3> = p.to_vec();
        let mut seen = crate::dhash::HashSet::default();
        for face in f {
            for k in 0..face.len() {
                let (a, b) = (face[k], face[(k + 1) % face.len()]);
                if seen.insert((a.min(b), a.max(b))) {
                    for t in [0.25, 0.5, 0.75] {
                        out.push(add(mul(p[a as usize], 1.0 - t), mul(p[b as usize], t)));
                    }
                }
            }
        }
        let stride = (out.len() / 20000).max(1);
        out.into_iter().step_by(stride).collect()
    };
    for (pts, other) in [(probes(pa, fa), &cb), (probes(pb, fb), &ca)] {
        for q in pts {
            let mut r = 0.05;
            loop {
                if let Some((dd, _, _)) = other.closest(q, r) {
                    d.push(dd);
                    break;
                }
                r *= 4.0;
                if r > 100.0 {
                    d.push(r);
                    break;
                }
            }
        }
    }
    d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = d.iter().sum::<f64>() / d.len().max(1) as f64;
    (mean, d[((d.len() as f64 * 0.95) as usize).min(d.len() - 1)], *d.last().unwrap_or(&0.0))
}
