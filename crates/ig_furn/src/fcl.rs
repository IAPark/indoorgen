//! Port of FCL's mesh-mesh contact semantics (Intersect::intersect_Triangle, contact depth = min of the two
//! "deepest vertex below the other plane" values), which the reference solver uses for validity and accessibility.
use crate::mesh::{cross, dot, sub, V3};

const EPS: f64 = 1e-5;

fn project6(ax: V3, p: [V3; 3], q: [V3; 3]) -> bool {
    let pp = [dot(ax, p[0]), dot(ax, p[1]), dot(ax, p[2])];
    let qq = [dot(ax, q[0]), dot(ax, q[1]), dot(ax, q[2])];
    let mn1 = pp[0].min(pp[1]).min(pp[2]);
    let mx2 = qq[0].max(qq[1]).max(qq[2]);
    if mn1 > mx2 {
        return false;
    }
    let mx1 = pp[0].max(pp[1]).max(pp[2]);
    let mn2 = qq[0].min(qq[1]).min(qq[2]);
    !(mn2 > mx1)
}

fn plane(v: [V3; 3]) -> (V3, f64) {
    let mut n = cross(sub(v[1], v[0]), sub(v[2], v[0]));
    let l2 = dot(n, n);
    if l2 > 0.0 {
        let l = l2.sqrt();
        n = [n[0] / l, n[1] / l, n[2] / l];
    }
    (n, dot(n, v[0]))
}

/// (max depth, number of deepest points) of `pts` behind the plane (n, t)
fn deepest(pts: &[V3; 3], n: V3, t: f64) -> (f64, usize) {
    let mut max_depth = -f64::MAX;
    let mut num_deepest = 0usize;
    let (mut neg, mut pos, mut zero) = (0, 0, 0);
    for p in pts {
        let dist = -(dot(n, *p) - t);
        if dist > EPS {
            pos += 1;
        } else if dist < -EPS {
            neg += 1;
        } else {
            zero += 1;
        }
        if dist > max_depth {
            max_depth = dist;
            num_deepest = 1;
        } else if dist + 1e-6 >= max_depth {
            num_deepest += 1;
        }
    }
    if max_depth < -EPS {
        num_deepest = 0;
    }
    if zero == 0 && (neg == 0 || pos == 0) {
        num_deepest = 0;
    }
    (max_depth, num_deepest)
}

/// FCL triangle/triangle test with contact information: Some((penetration depth, contact count)) if the triangles intersect
pub fn tri_contact(pw: [V3; 3], qw: [V3; 3]) -> Option<(f64, usize)> {
    let p1 = [0.0; 3];
    let p2 = sub(pw[1], pw[0]);
    let p3 = sub(pw[2], pw[0]);
    let q1 = sub(qw[0], pw[0]);
    let q2 = sub(qw[1], pw[0]);
    let q3 = sub(qw[2], pw[0]);
    let p = [p1, p2, p3];
    let q = [q1, q2, q3];
    let e1 = sub(p2, p1);
    let e2 = sub(p3, p2);
    let n1 = cross(e1, e2);
    if !project6(n1, p, q) {
        return None;
    }
    let f1 = sub(q2, q1);
    let f2 = sub(q3, q2);
    let m1 = cross(f1, f2);
    if !project6(m1, p, q) {
        return None;
    }
    let f3 = sub(q1, q3);
    let e3 = sub(p1, p3);
    for e in [e1, e2, e3] {
        for f in [f1, f2, f3] {
            if !project6(cross(e, f), p, q) {
                return None;
            }
        }
    }
    for e in [e1, e2, e3] {
        if !project6(cross(e, n1), p, q) {
            return None;
        }
    }
    for f in [f1, f2, f3] {
        if !project6(cross(f, m1), p, q) {
            return None;
        }
    }
    let (n1, t1) = plane(pw);
    let (n2, t2) = plane(qw);
    let (depth2, nd2) = deepest(&qw, n1, t1);
    let (depth1, nd1) = deepest(&pw, n2, t2);
    if depth1 > depth2 {
        Some((depth2, nd2.min(2)))
    } else {
        Some((depth1, nd1.min(2)))
    }
}

fn aabb(t: &[V3; 3]) -> ([f64; 3], [f64; 3]) {
    let mut lo = t[0];
    let mut hi = t[0];
    for v in &t[1..] {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (lo, hi)
}

/// trimesh/FCL `in_collision` between two triangle soups: None if no contact is produced, else the max contact depth
pub fn mesh_depth(a: &[[V3; 3]], b: &[[V3; 3]]) -> Option<f64> {
    let ab: Vec<_> = a.iter().map(aabb).collect();
    let bb: Vec<_> = b.iter().map(aabb).collect();
    // overall culling
    let mut best: Option<f64> = None;
    for (i, ta) in a.iter().enumerate() {
        let (alo, ahi) = ab[i];
        for (j, tb) in b.iter().enumerate() {
            let (blo, bhi) = bb[j];
            if (0..3).any(|k| alo[k] > bhi[k] + 1e-9 || blo[k] > ahi[k] + 1e-9) {
                continue;
            }
            if let Some((d, n)) = tri_contact(*ta, *tb) {
                if n > 0 {
                    best = Some(best.map_or(d, |x: f64| x.max(d)));
                }
            }
        }
    }
    best
}

/// trimesh.creation.box(extents) triangles (same vertex order / winding / diagonals) transformed by `f`
pub fn trimesh_box_tris(ext: V3, f: &dyn Fn(V3) -> V3) -> Vec<[V3; 3]> {
    let h = [ext[0] / 2.0, ext[1] / 2.0, ext[2] / 2.0];
    let v: Vec<V3> = (0..8usize)
        .map(|i| {
            let p = [if i & 4 != 0 { h[0] } else { -h[0] }, if i & 2 != 0 { h[1] } else { -h[1] }, if i & 1 != 0 { h[2] } else { -h[2] }];
            f(p)
        })
        .collect();
    let faces: [[usize; 3]; 12] = [[1, 3, 0], [4, 1, 0], [0, 3, 2], [2, 4, 0], [1, 7, 3], [5, 1, 4], [5, 7, 1], [3, 7, 2], [6, 4, 2], [2, 7, 6], [6, 5, 4], [7, 5, 6]];
    faces.iter().map(|f| [v[f[0]], v[f[1]], v[f[2]]]).collect()
}
