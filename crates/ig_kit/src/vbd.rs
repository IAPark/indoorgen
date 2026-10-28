//! Vertex Block Descent cloth (Chen et al. 2024) with offset-geometry contact against static colliders (after OGC, Chen et al. 2025),
//! a drop-in for [`crate::cloth::simulate`]. Same material model and units as the implicit solver (Blender's CLOTH springs: tension /
//! compression / shear / compression-only bending, per-vertex mass, frames as the time unit, uniform pressure, pins, air drag), but:
//! - each substep minimises the implicit-Euler energy with per-vertex Newton steps (Gauss-Seidel), unconditionally stable;
//! - contact is an energy (quadratic within the contact radius, log barrier near the surface) between every cloth vertex and the collider
//!   triangles near it, with lagged IPC friction and gap-rate damping;
//! - every vertex moves at most `gamma` x its distance to the nearest collider triangle per substep (the conservative displacement
//!   bound): a vertex can never cross a collider surface, so the result has no vertex-in-collider penetration by construction.
//! Reference: NVIDIA Newton `solvers/vbd/particle_vbd_kernels.py` (Apache-2.0); this is an independent CPU implementation.
use crate::cloth::{build_springs, closest_on_tri, pressure_forces, Calib, ClothParams, Collider};
use crate::mesh::V3;

#[derive(Clone, Debug)]
pub struct VbdCfg {
    /// substeps per frame
    pub substeps: u32,
    /// Gauss-Seidel sweeps per substep
    pub iters: u32,
    /// contact radius (m): cloth rests about this far from a collider surface
    pub radius: f64,
    /// contact stiffness (force per m, same scale as the spring stiffnesses)
    pub k_contact: f64,
    /// contact damping of the approaching gap rate
    pub kd_contact: f64,
    /// Coulomb friction coefficient against colliders
    pub mu: f64,
    /// friction smoothing velocity (m/frame)
    pub fric_eps: f64,
    /// conservative bound relaxation: a vertex moves at most gamma x (distance to the nearest collider) per substep
    pub gamma: f64,
    /// Chebyshev acceleration of the Gauss-Seidel sweeps (VBD paper 3.5): estimated spectral radius, 0 = off
    pub rho: f64,
}

impl Default for VbdCfg {
    fn default() -> Self {
        let e = |k: &str, d: f64| ig_core::envx::var(k).and_then(|s| s.parse().ok()).unwrap_or(d);
        VbdCfg {
            substeps: e("VBD_SUB", 3.0) as u32,
            iters: e("VBD_IT", 8.0) as u32,
            radius: e("VBD_R", 0.008),
            k_contact: e("VBD_KC", 2000.0),
            kd_contact: e("VBD_KD", 0.0),
            mu: e("VBD_MU", 0.6),
            fric_eps: e("VBD_FEPS", 1e-3),
            gamma: e("VBD_GAMMA", 0.45),
            rho: e("VBD_RHO", 0.0),
        }
    }
}

type M3 = [[f64; 3]; 3];

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn axpy(f: &mut V3, s: f64, a: V3) {
    f[0] += s * a[0];
    f[1] += s * a[1];
    f[2] += s * a[2];
}
#[inline]
fn add_outer(h: &mut M3, s: f64, a: V3) {
    for r in 0..3 {
        for c in 0..3 {
            h[r][c] += s * a[r] * a[c];
        }
    }
}
#[inline]
fn add_diag(h: &mut M3, s: f64) {
    h[0][0] += s;
    h[1][1] += s;
    h[2][2] += s;
}
/// H^-1 f (None when H is singular)
fn solve3(h: &M3, f: V3) -> Option<V3> {
    let det = h[0][0] * (h[1][1] * h[2][2] - h[1][2] * h[2][1]) - h[0][1] * (h[1][0] * h[2][2] - h[1][2] * h[2][0]) + h[0][2] * (h[1][0] * h[2][1] - h[1][1] * h[2][0]);
    if det.abs() < 1e-30 {
        return None;
    }
    let inv = [
        [h[1][1] * h[2][2] - h[1][2] * h[2][1], h[0][2] * h[2][1] - h[0][1] * h[2][2], h[0][1] * h[1][2] - h[0][2] * h[1][1]],
        [h[1][2] * h[2][0] - h[1][0] * h[2][2], h[0][0] * h[2][2] - h[0][2] * h[2][0], h[0][2] * h[1][0] - h[0][0] * h[1][2]],
        [h[1][0] * h[2][1] - h[1][1] * h[2][0], h[0][1] * h[2][0] - h[0][0] * h[2][1], h[0][0] * h[1][1] - h[0][1] * h[1][0]],
    ];
    let d = 1.0 / det;
    Some([0, 1, 2].map(|r| d * (inv[r][0] * f[0] + inv[r][1] * f[1] + inv[r][2] * f[2])))
}

/// contact energy derivative and second derivative at distance `dis` (Newton's C2 quadratic + log-barrier profile)
#[inline]
fn barrier(dis: f64, r: f64, k: f64) -> (f64, f64) {
    let tau = 0.5 * r;
    let dmin = (1e-5f64).min(0.5 * tau);
    if dis >= tau {
        (-k * (r - dis), k)
    } else if dis > dmin {
        let k2 = tau * tau * k;
        (-k2 / dis, k2 / (dis * dis))
    } else {
        let k2 = tau * tau * k;
        (k2 * (dis - 2.0 * dmin) / (dmin * dmin), k2 / (dmin * dmin))
    }
}

pub fn simulate(pos0: &[V3], faces: &[Vec<u32>], p: &ClothParams, colliders: &[&Collider], cal: &Calib, cfg: &VbdCfg) -> Vec<V3> {
    let n = pos0.len();
    let springs = build_springs(pos0, faces);
    let avg_len = {
        let s: Vec<f64> = springs.iter().filter(|s| s.kind == 0).map(|s| s.l0).collect();
        s.iter().sum::<f64>() / s.len().max(1) as f64
    };
    let ak = cal.k_scale / avg_len.max(1e-9);
    let mut adj: Vec<Vec<u32>> = vec![vec![]; n];
    for (si, s) in springs.iter().enumerate() {
        adj[s.i as usize].push(si as u32);
        adj[s.j as usize].push(si as u32);
    }
    let pinned: Vec<bool> = (0..n).map(|i| p.pin.get(i).copied().unwrap_or(0.0) >= 0.999).collect();
    let sub_n = cfg.substeps.max(1);
    let h = 1.0 / sub_n as f64;
    let frames = p.end_frame.saturating_sub(1);
    let m = p.mass;
    let g = [0.0, 0.0, -cal.g * p.gravity];
    let drag = p.air_damping * cal.drag_scale;
    let press = if p.use_pressure && p.pressure != 0.0 {
        cal.press_scale * (p.pressure.abs() - cal.press_thresh - if p.gravity > 0.0 { cal.press_thresh_g } else { 0.0 }).max(0.0).powf(cal.press_pow) * p.pressure.signum()
    } else {
        0.0
    };
    let r = cfg.radius;
    // candidate collider triangles per vertex: everything within the radius plus the bound reach of this substep
    let query = r * 3.0;
    let mut x: Vec<V3> = pos0.to_vec();
    let mut v: Vec<V3> = vec![[0.0; 3]; n];
    let mut xp: Vec<V3> = x.clone();
    let mut y: Vec<V3> = x.clone();
    let mut bound = vec![f64::MAX; n];
    // per vertex: nearest collider triangle at detection and whether the vertex is behind it (inside a closed collider: the pre-sim
    // cloth often starts there, e.g. a comforter centred on the mattress top); such a vertex is pushed out along the face normal
    let mut nearest: Vec<Option<(u16, u32, bool)>> = vec![None; n];
    let mut cand: Vec<Vec<(u16, u32)>> = vec![vec![]; n];
    let mut pf = vec![[0.0f64; 3]; n];
    let mut scratch: Vec<u32> = vec![];
    let mut truncated = 0usize;
    for _step in 0..frames * sub_n {
        // contact detection at the start of the substep: nearest collider distance (-> displacement bound) and nearby triangles
        for i in 0..n {
            cand[i].clear();
            if pinned[i] || colliders.is_empty() {
                bound[i] = f64::MAX;
                continue;
            }
            let mut dmin = 1.0f64;
            nearest[i] = None;
            for (ci, c) in colliders.iter().enumerate() {
                if let Some((d, q, ti)) = c.closest(x[i], dmin) {
                    if d < dmin || nearest[i].is_none() {
                        let behind = d > 1e-9 && dot(sub(x[i], q), c.normal(ti)) < -0.3 * d;
                        nearest[i] = Some((ci as u16, ti as u32, behind));
                    }
                    dmin = dmin.min(d);
                }
                scratch.clear();
                c.near(x[i], query, &mut scratch);
                let xi = x[i];
                cand[i].extend(scratch.iter().filter(|&&t| {
                    let q = closest_on_tri(xi, c.tri(t as usize));
                    let d = sub(xi, q);
                    dot(d, d) < query * query
                }).map(|&t| (ci as u16, t)));
            }
            let behind = nearest[i].map_or(false, |n| n.2);
            bound[i] = if behind { f64::MAX } else { cfg.gamma * dmin };
        }
        // inertia target, also the initial guess (Gauss-Seidel only corrects the elastic / contact deviation from it; starting from the
        // previous position it would barely move stiff cloth in a few sweeps); the initial move is stopped short of collider surfaces
        for i in 0..n {
            xp[i] = x[i];
            y[i] = if pinned[i] { x[i] } else { [0, 1, 2].map(|k| x[i][k] + h * v[i][k] + h * h * g[k]) };
            if !pinned[i] {
                let disp = sub(y[i], xp[i]);
                let dl = dot(disp, disp).sqrt();
                let mut xn = y[i];
                if dl > bound[i] {
                    let mut t = 1.0f64;
                    for c in colliders {
                        if let Some((th, _)) = c.segment_hit(xp[i], y[i]) {
                            t = t.min(th);
                        }
                    }
                    if t < 1.0 {
                        let keep = ((t * dl - 0.5 * r) / dl).max(0.0);
                        xn = [xp[i][0] + disp[0] * keep, xp[i][1] + disp[1] * keep, xp[i][2] + disp[2] * keep];
                    }
                }
                x[i] = xn;
            }
        }
        let mh2 = m / (h * h);
        let mut omega = 1.0f64;
        let mut x_prev_it = x.clone(); // the previous iterate
        let mut x_prev2 = x.clone(); // the one before
        for _it in 0..cfg.iters {
            if press != 0.0 {
                pressure_forces(&x, faces, press, &mut pf);
            }
            for i in 0..n {
                if pinned[i] {
                    continue;
                }
                let xi = x[i];
                let mut f = [0.0; 3];
                axpy(&mut f, mh2, sub(y[i], xi));
                let mut hm: M3 = [[0.0; 3]; 3];
                add_diag(&mut hm, mh2);
                // air drag
                axpy(&mut f, -drag / h, sub(xi, xp[i]));
                add_diag(&mut hm, drag / h);
                if press != 0.0 {
                    axpy(&mut f, 1.0, pf[i]);
                }
                // springs
                for &si in &adj[i] {
                    let s = &springs[si as usize];
                    let j = if s.i as usize == i { s.j as usize } else { s.i as usize };
                    let e = sub(x[j], xi);
                    let l = dot(e, e).sqrt();
                    if l < 1e-12 {
                        continue;
                    }
                    let (k, d, bend) = match s.kind {
                        0 => if l > s.l0 { (p.tension * ak, p.tension_damp * cal.d_scale, false) } else { (p.compression * ak, p.compression_damp * cal.d_scale, false) },
                        1 => (p.shear * ak, p.shear_damp * cal.d_scale, false),
                        _ => (p.bending * ak, 0.0, true),
                    };
                    if bend && l >= s.l0 {
                        continue;
                    }
                    let dir = [e[0] / l, e[1] / l, e[2] / l];
                    axpy(&mut f, k * (l - s.l0), dir);
                    let gfac = (1.0 - s.l0 / l).max(0.0);
                    add_outer(&mut hm, k * (1.0 - gfac), dir);
                    add_diag(&mut hm, k * gfac);
                    if d > 0.0 {
                        let vrel = dot(sub(sub(x[j], xp[j]), sub(xi, xp[i])), dir) / h;
                        axpy(&mut f, d * vrel, dir);
                        add_outer(&mut hm, d / h, dir);
                    }
                }
                // inside a collider: one-sided penalty out along the nearest face normal (to the contact radius in front of it)
                let inside = match nearest[i] {
                    Some((ci, ti, true)) => {
                        let c = colliders[ci as usize];
                        let q = closest_on_tri(xi, c.tri(ti as usize));
                        let nt = c.normal(ti as usize);
                        let sd = dot(sub(xi, q), nt);
                        if sd < r {
                            axpy(&mut f, cfg.k_contact * (r - sd), nt);
                            add_outer(&mut hm, cfg.k_contact, nt);
                        }
                        true
                    }
                    _ => false,
                };
                // contact with the collider triangles near this vertex (two-sided offset surfaces)
                for &(ci, ti) in if inside { &[][..] } else { &cand[i][..] } {
                    let tri = colliders[ci as usize].tri(ti as usize);
                    let q = closest_on_tri(xi, tri);
                    let diff = sub(xi, q);
                    let dis = dot(diff, diff).sqrt();
                    if dis >= r || dis < 1e-12 {
                        continue;
                    }
                    let nrm = [diff[0] / dis, diff[1] / dis, diff[2] / dis];
                    let (de, d2e) = barrier(dis, r, cfg.k_contact);
                    axpy(&mut f, -de, nrm);
                    add_outer(&mut hm, d2e, nrm);
                    let dx = sub(xi, xp[i]);
                    let gap_rate = dot(nrm, dx) / h;
                    if cfg.kd_contact > 0.0 && gap_rate < -1e-9 {
                        axpy(&mut f, -cfg.kd_contact * gap_rate, nrm);
                        add_outer(&mut hm, cfg.kd_contact / h, nrm);
                    }
                    // lagged IPC friction: tangential motion of this substep against the static surface
                    if cfg.mu > 0.0 {
                        let un = dot(dx, nrm);
                        let u = [dx[0] - un * nrm[0], dx[1] - un * nrm[1], dx[2] - un * nrm[2]];
                        let ul = dot(u, u).sqrt();
                        if ul > 0.0 {
                            let eps = cfg.fric_eps * h;
                            let f1 = if ul > eps { 1.0 / ul } else { (-ul / eps + 2.0) / eps };
                            let fn_ = -de;
                            axpy(&mut f, -cfg.mu * fn_ * f1, u);
                            add_diag(&mut hm, cfg.mu * fn_ * f1);
                            add_outer(&mut hm, -cfg.mu * fn_ * f1, nrm);
                        }
                    }
                }
                let Some(dx) = solve3(&hm, f) else { continue };
                let mut xn = [xi[0] + dx[0], xi[1] + dx[1], xi[2] + dx[2]];
                // displacement bound: within gamma x (nearest collider distance at detection) of the substep start the move is safe; a
                // longer one (sliding along a surface, falling past an edge) is checked as a segment and stopped short of any collider
                // surface it would enter, so a vertex never crosses a collider
                let disp = sub(xn, xp[i]);
                let dl = dot(disp, disp).sqrt();
                if dl > bound[i] {
                    let mut t = 1.0f64;
                    for c in colliders {
                        if let Some((th, _)) = c.segment_hit(xp[i], xn) {
                            t = t.min(th);
                        }
                    }
                    if t < 1.0 {
                        let keep = ((t * dl - 0.5 * r) / dl).max(0.0);
                        xn = [xp[i][0] + disp[0] * keep, xp[i][1] + disp[1] * keep, xp[i][2] + disp[2] * keep];
                        truncated += 1;
                    }
                }
                x[i] = xn;
            }
            // Chebyshev semi-iterative acceleration: x = omega (x_gs - x_{n-1}) + x_{n-1}; only for vertices with no collider near
            // them (an extrapolated move is not collision-checked), and never past the displacement bound
            if cfg.rho > 0.0 {
                let rho2 = cfg.rho * cfg.rho;
                omega = if _it == 0 { 1.0 } else if _it == 1 { 2.0 / (2.0 - rho2) } else { 4.0 / (4.0 - rho2 * omega) };
                if _it >= 1 {
                    for i in 0..n {
                        if pinned[i] || !cand[i].is_empty() || nearest[i].map_or(false, |q| q.2) {
                            continue;
                        }
                        let xa = [0, 1, 2].map(|k| omega * (x[i][k] - x_prev2[i][k]) + x_prev2[i][k]);
                        let d = sub(xa, xp[i]);
                        if dot(d, d) <= bound[i] * bound[i] {
                            x[i] = xa;
                        }
                    }
                }
                std::mem::swap(&mut x_prev2, &mut x_prev_it);
                x_prev_it.copy_from_slice(&x);
            }
        }
        for i in 0..n {
            v[i] = if pinned[i] { [0.0; 3] } else { [0, 1, 2].map(|k| (x[i][k] - xp[i][k]) / h) };
        }
    }
    if std::env::var_os("VBD_LOG").is_some() {
        eprintln!("VBD {n} verts {} springs {frames} frames x {sub_n} substeps x {} iters, {truncated} bound truncations", springs.len(), cfg.iters);
    }
    x
}
