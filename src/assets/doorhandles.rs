//! Door handles (BaseDoorFactory.make_knobs / make_levers / make_pulls and nodegroup_push_bar_handle).
//! Geometry follows the python constructions (bezier profile + spin, spun lever + solidified arm, mirrored pull path
//! swept with a circle/square, capsule-profile push bar) but not Blender's exact meshing (spin/subsurf/solidify indices).
use super::curvekit::*;
use super::doors::{DoorSpec, HandleDraws};
use super::solid::Solid;
use crate::mesh::*;

type P2 = (f64, f64);

/// Blender bezier curve through `anchors` (VECTOR handles at `vec_idx`, AUTO elsewhere), `res` points per segment
fn bezier_profile(anchors: &[P2], vec_idx: &[usize], res: usize) -> Vec<P2> {
    let n = anchors.len();
    let sub = |a: P2, b: P2| (a.0 - b.0, a.1 - b.1);
    let len = |a: P2| (a.0 * a.0 + a.1 * a.1).sqrt();
    let mut hl = vec![(0.0, 0.0); n];
    let mut hr = vec![(0.0, 0.0); n];
    for i in 0..n {
        let p = anchors[i];
        let prev = if i > 0 { anchors[i - 1] } else { (2.0 * p.0 - anchors[1].0, 2.0 * p.1 - anchors[1].1) };
        let next = if i + 1 < n { anchors[i + 1] } else { (2.0 * p.0 - anchors[n - 2].0, 2.0 * p.1 - anchors[n - 2].1) };
        if vec_idx.contains(&i) {
            hl[i] = (p.0 + (prev.0 - p.0) / 3.0, p.1 + (prev.1 - p.1) / 3.0);
            hr[i] = (p.0 + (next.0 - p.0) / 3.0, p.1 + (next.1 - p.1) / 3.0);
        } else {
            let da = sub(p, prev);
            let db = sub(next, p);
            let (la, lb) = (len(da), len(db));
            let t = (db.0 / lb + da.0 / la, db.1 / lb + da.1 / la);
            let l = len(t) * 2.5614;
            if l != 0.0 {
                let (a, b) = (la / l, lb / l);
                hl[i] = (p.0 - t.0 * a, p.1 - t.1 * a);
                hr[i] = (p.0 + t.0 * b, p.1 + t.1 * b);
            } else {
                hl[i] = p;
                hr[i] = p;
            }
        }
    }
    let _ = res;
    let mut out: Vec<P2> = vec![];
    for i in 0..n - 1 {
        let (p0, h0, h1, p1) = (anchors[i], hr[i], hl[i + 1], anchors[i + 1]);
        // draw.curve2mesh: `number_cuts = min(int(chord / 5e-3) - 1, 64)` equal-parameter subdivisions, then a polyline through the control points
        let chord = len(sub(p1, p0));
        let cuts = ((chord / 5e-3) as i64 - 1).min(64);
        out.push(p0);
        for k in 1..=cuts.max(0) {
            let t = k as f64 / (cuts + 1) as f64;
            let s = 1.0 - t;
            let f = |a: f64, b: f64, c: f64, d: f64| s * s * s * a + 3.0 * s * s * t * b + 3.0 * s * t * t * c + t * t * t * d;
            out.push((f(p0.0, h0.0, h1.0, p1.0), f(p0.1, h0.1, h1.1, p1.1)));
        }
    }
    out.push(anchors[n - 1]);
    // WELD modifier (1e-3)
    let mut w: Vec<P2> = vec![];
    for p in out {
        if let Some(l) = w.last() {
            if len(sub(p, *l)) < 1e-3 {
                continue;
            }
        }
        w.push(p);
    }
    w
}

/// spin an (r, y) profile about the y axis; first ring is capped with an n-gon (edge_face_add / fill)
fn revolve_y(profile: &[P2]) -> Mesh {
    let mean_r = profile.iter().map(|p| p.0.abs()).sum::<f64>() / profile.len() as f64;
    let n = ((2.0 * std::f64::consts::PI * mean_r / 5e-3) as usize).min(128).max(3);
    let mut m = Mesh::new();
    let mut rings: Vec<Vec<u32>> = vec![];
    for &(r, y) in profile {
        let mut ring = vec![];
        if r.abs() < 1e-3 {
            m.pos.push([0.0, y, 0.0]);
            ring.push((m.pos.len() - 1) as u32);
        } else {
            for k in 0..n {
                let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                m.pos.push([r * a.cos(), y, r * a.sin()]);
                ring.push((m.pos.len() - 1) as u32);
            }
        }
        rings.push(ring);
    }
    for w in rings.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        for k in 0..n {
            let k2 = (k + 1) % n;
            match (a.len() == 1, b.len() == 1) {
                (false, false) => m.faces.push(vec![a[k], a[k2], b[k2], b[k]]),
                (false, true) => m.faces.push(vec![a[k], a[k2], b[0]]),
                (true, false) => m.faces.push(vec![a[0], b[k2], b[k]]),
                _ => {}
            }
        }
    }
    if rings[0].len() > 1 {
        m.faces.push(rings[0].iter().rev().cloned().collect());
    }
    orient_outward(&mut m);
    m
}

/// orient faces of a surface of revolution about y: radial outward for the side, -y for the base cap
fn orient_outward(m: &mut Mesh) {
    for f in m.faces.iter_mut() {
        let p: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
        let mut n = [0.0; 3];
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let k = p.len() as f64;
        let fc = [p.iter().map(|q| q[0]).sum::<f64>() / k, p.iter().map(|q| q[1]).sum::<f64>() / k, p.iter().map(|q| q[2]).sum::<f64>() / k];
        let flat = p.iter().all(|q| (q[1] - p[0][1]).abs() < 1e-12);
        let expect = if flat { [0.0, -1.0, 0.0] } else { [fc[0], 0.0, fc[2]] };
        if n[0] * expect[0] + n[1] * expect[1] + n[2] * expect[2] < 0.0 {
            f.reverse();
        }
    }
}

fn mesh_chamfer(m: &Mesh) -> Mesh {
    Solid::from_mesh(&m.welded(1e-9)).chamfer_sharp(0.001, 70f64.to_radians()).to_mesh()
}

fn shift(m: &mut Mesh, t: V3) {
    for p in m.pos.iter_mut() {
        *p = [p[0] + t[0], p[1] + t[1], p[2] + t[2]];
    }
}

pub fn knob_mesh(s: &DoorSpec) -> Mesh {
    let anchors: Vec<P2> = (0..7).map(|i| (s.knob_radius_mid[i] * s.knob_radius, s.knob_depth_mid[i] * s.knob_depth)).collect();
    let prof = bezier_profile(&anchors, &[0, 2, 3], 12);
    let mut m = revolve_y(&prof);
    shift(&mut m, [0.0, s.depth / 2.0, 0.0]);
    mesh_chamfer(&m)
}

pub fn lever_mesh(s: &DoorSpec, hd: &HandleDraws) -> Mesh {
    let xs = [s.lever_radius, s.lever_radius, s.lever_mid_radius, s.lever_mid_radius, 0.0];
    let ys: Vec<f64> = [0.0, s.lever_mid_depth, s.lever_mid_depth, 1.0, 1.001].iter().map(|v| v * s.lever_depth).collect();
    let anchors: Vec<P2> = (0..5).map(|i| (xs[i], ys[i])).collect();
    let prof = bezier_profile(&anchors, &[0, 1, 2, 3], 12);
    let mut base = revolve_y(&prof);
    // arm: new_line(4) scaled by lever_length, extruded 2*mid_radius in z, solidified by mid_radius
    let l = s.lever_length;
    let mut c: Vec<V3> = (0..5).map(|k| [k as f64 / 4.0, 0.0, 0.0]).collect();
    match s.level_type {
        0 => {
            c[1][2] = -hd.wave[0];
            c[3][2] = hd.wave[1];
        }
        2 => c[4][1] = -hd.bent,
        _ => {}
    }
    for p in c.iter_mut() {
        *p = [p[0] * l, p[1] * l, p[2] * l];
    }
    let (mr, th) = (s.lever_mid_radius, s.lever_mid_radius);
    let normal = |k: usize| -> V3 {
        if s.level_type == 0 {
            return [0.0, 1.0, 0.0];
        }
        let seg = |a: usize, b: usize| {
            let d = [c[b][0] - c[a][0], c[b][1] - c[a][1]];
            let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
            [-d[1] / n, d[0] / n]
        };
        let n = if k == 0 { seg(0, 1) } else if k == 4 { seg(3, 4) } else {
            let (a, b) = (seg(k - 1, k), seg(k, k + 1));
            let s = [a[0] + b[0], a[1] + b[1]];
            let l = (s[0] * s[0] + s[1] * s[1]).sqrt();
            [s[0] / l, s[1] / l]
        };
        [n[0], n[1], 0.0]
    };
    let mut arm = Mesh::new();
    let mut idx = [[[0u32; 2]; 2]; 5]; // [k][side][edge]
    for k in 0..5 {
        let n = normal(k);
        for (si, sgn) in [-1.0, 1.0].iter().enumerate() {
            for e in 0..2 {
                arm.pos.push([c[k][0] + sgn * th / 2.0 * n[0], c[k][1] + sgn * th / 2.0 * n[1], c[k][2] + e as f64 * 2.0 * mr + sgn * th / 2.0 * n[2]]);
                idx[k][si][e] = (arm.pos.len() - 1) as u32;
            }
        }
    }
    for k in 0..4 {
        for si in 0..2 {
            arm.faces.push(vec![idx[k][si][0], idx[k + 1][si][0], idx[k + 1][si][1], idx[k][si][1]]);
        }
        for e in 0..2 {
            arm.faces.push(vec![idx[k][0][e], idx[k + 1][0][e], idx[k + 1][1][e], idx[k][1][e]]);
        }
    }
    arm.faces.push(vec![idx[0][0][0], idx[0][1][0], idx[0][1][1], idx[0][0][1]]);
    arm.faces.push(vec![idx[4][0][0], idx[4][0][1], idx[4][1][1], idx[4][1][0]]);
    shift(&mut arm, [-s.lever_mid_radius, s.lever_depth, -s.lever_mid_radius]);
    base.append(&arm);
    shift(&mut base, [0.0, s.depth / 2.0, 0.0]);
    base
}

pub fn pull_mesh(s: &DoorSpec) -> Mesh {
    let (sz, dp, ex, w) = (s.pull_size, s.pull_depth, s.pull_extension, s.pull_width);
    // mirrored (z) polyline(s)
    let paths: Vec<Vec<V3>> = match s.pull_type {
        0 => vec![vec![[0.0, 0.0, -sz], [0.0, dp, -sz], [0.0, dp, sz], [0.0, 0.0, sz]]],
        1 => vec![vec![[0.0, 0.0, -sz], [0.0, dp, -sz], [0.0, dp, sz], [0.0, 0.0, sz]], vec![[0.0, dp, sz], [0.0, dp, sz + ex]], vec![[0.0, dp, -sz], [0.0, dp, -sz - ex]]],
        _ => vec![vec![[0.0, 0.0, sz], [0.0, dp, sz], [w, dp, sz], [w, dp, 0.0]], vec![[0.0, 0.0, -sz], [0.0, dp, -sz], [w, dp, -sz], [w, dp, 0.0]]],
    };
    let mut out = Mesh::new();
    let r = s.pull_radius;
    for path in paths {
        let mut c = Curve::new(path, false);
        if s.to_pull_bevel && c.pts.len() >= 3 {
            c = c.fillet_poly(s.pull_bevel_width * 0.9, 4);
        }
        let prof = if s.is_pull_circular {
            Curve::circle(32, r)
        } else {
            Curve::new(vec![[r, r, 0.0], [-r, r, 0.0], [-r, -r, 0.0], [r, -r, 0.0]], true)
        };
        if !s.is_pull_circular {
            for p in c.pts.iter_mut() {
                p[1] -= r;
                p[2] -= r;
            }
        }
        out.append(&curve_to_mesh(&c, &prof, true));
    }
    shift(&mut out, [s.handle_offset - s.width, s.depth / 2.0, 0.0]);
    mesh_chamfer(&out)
}

/// nodegroup_push_bar_handle, rotated (0, pi/2, 0) and placed at (-0.95 w, depth/2, z_offset); returns None for non-bar doors
pub fn bar_mesh(s: &DoorSpec) -> Mesh {
    let b = match &s.bar {
        Some(b) => b,
        None => return Mesh::new(),
    };
    let capsule = |aspect: f64, scale: f64| -> Curve {
        let q = Curve::new(vec![[0.5, aspect / 2.0, 0.0], [-0.5, aspect / 2.0, 0.0], [-0.5, -aspect / 2.0, 0.0], [0.5, -aspect / 2.0, 0.0]], true);
        let mut c = q.resample(32).fillet_poly(1.0, 8).resample(64);
        for p in c.pts.iter_mut() {
            *p = [p[0] * scale, p[1] * scale, p[2] * scale];
        }
        c
    };
    let cyl = |width: f64, aspect: f64, height: f64| -> Mesh { curve_to_mesh(&Curve::line([0.0, 0.0, 0.01], [0.0, 0.0, height]), &capsule(aspect, width), true) };
    let (l, t, asp) = (b.bar_length, b.bar_thickness, b.aspect);
    let mut parent = cyl(t, asp, l);
    // handle_end: capsule-ish profile 3x1 with a bump, swept along z over l*end_length_ratio
    let end_h = l * b.end_length_ratio;
    let end_w = b.end_height_ratio * t;
    {
        let q = Curve::new(vec![[1.5, 0.5, 0.0], [-1.5, 0.5, 0.0], [-1.5, -0.5, 0.0], [1.5, -0.5, 0.0]], true).resample(8);
        let mut c = q;
        for (i, p) in c.pts.iter_mut().enumerate() {
            if (1..=2).contains(&i) {
                p[1] += 0.75;
            }
        }
        for p in c.pts.iter_mut() {
            *p = [p[0], p[1] * asp, p[2]];
        }
        let c = c.resample(16).fillet_poly(0.5, 8).resample(64);
        let mut c = c;
        let sc = end_w * 0.3333;
        for p in c.pts.iter_mut() {
            *p = [p[0] * sc, p[1] * sc, p[2] * sc];
        }
        parent.append(&curve_to_mesh(&Curve::line([0.0, 0.0, 0.0], [0.0, 0.0, end_h]), &c, true));
    }
    let mut child = cyl(t * b.height_ratio, asp, l * b.length_ratio);
    shift(&mut child, [0.0, t * asp * 0.4, (-l / 2.0 + end_h) * 0.95]);
    // sliding joint: the child is positioned relative to the parent's bounding box centre
    let (plo, phi) = parent.bounds();
    shift(&mut child, [(plo[0] + phi[0]) / 2.0, (plo[1] + phi[1]) / 2.0, (plo[2] + phi[2]) / 2.0]);
    parent.append(&child);
    let rot = Mat4::euler_xyz([0.0, 1.5708, 0.0]);
    for p in parent.pos.iter_mut() {
        *p = rot.apply(*p);
    }
    shift(&mut parent, [0.0, t * asp * 0.5, 0.0]);
    shift(&mut parent, [-s.width * 0.95, s.depth / 2.0, b.z_offset]);
    parent
}

fn mirror_y(m: &Mesh) -> Mesh {
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        p[1] = -p[1];
    }
    o.flipped()
}

/// the two-sided handle (symmetry along y); None for none/bar
pub fn handle_geometry(s: &DoorSpec, hd: &HandleDraws) -> Option<Mesh> {
    let h = match s.handle_type {
        0 => knob_mesh(s),
        1 => lever_mesh(s, hd),
        2 => pull_mesh(s),
        _ => return None,
    };
    let mut both = h.clone();
    both.append(&mirror_y(&h));
    Some(both)
}
