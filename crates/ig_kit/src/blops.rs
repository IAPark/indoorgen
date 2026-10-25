//! Ports of the Blender operators the tableware / vase factories rely on (bpy.ops.mesh.spin, remove_doubles, curve.subdivide + convert,
//! SOLIDIFY), validated against python dumps (vertex arrays bit-equal where noted).
use crate::mesh::Mesh;

pub type F3 = [f32; 3];

// ---------------------------------------------------------------- BLI_kdtree_3d + calc_duplicates_fast

#[derive(Clone)]
struct KdNode {
    co: F3,
    index: u32,
    left: i32,
    right: i32,
    d: u8,
}

fn kd_balance(nodes: &mut [KdNode], lo: usize, cnt: usize, axis: usize) -> i32 {
    if cnt == 0 {
        return -1;
    }
    if cnt == 1 {
        nodes[lo].left = -1;
        nodes[lo].right = -1;
        nodes[lo].d = axis as u8;
        return lo as i32;
    }
    let median = cnt / 2;
    let mut neg = lo as isize;
    let mut pos = (lo + cnt - 1) as isize;
    while pos > neg {
        let co = nodes[pos as usize].co[axis];
        let mut i = neg - 1;
        let mut j = pos;
        loop {
            i += 1;
            while nodes[i as usize].co[axis] < co {
                i += 1;
            }
            j -= 1;
            while nodes[j as usize].co[axis] > co && j > neg {
                j -= 1;
            }
            if i >= j {
                break;
            }
            nodes.swap(i as usize, j as usize);
        }
        nodes.swap(i as usize, pos as usize);
        if i >= (lo + median) as isize {
            pos = i - 1;
        }
        if i <= (lo + median) as isize {
            neg = i + 1;
        }
    }
    let m = lo + median;
    nodes[m].d = axis as u8;
    let ax = (axis + 1) % 3;
    let l = kd_balance(nodes, lo, median, ax);
    let r = kd_balance(nodes, m + 1, cnt - (median + 1), ax);
    nodes[m].left = l;
    nodes[m].right = r;
    m as i32
}

struct Dedup<'a> {
    nodes: &'a [KdNode],
    range: f32,
    range_sq: f32,
    dup: Vec<i32>,
    found: usize,
}

impl<'a> Dedup<'a> {
    fn rec(&mut self, ni: usize, q: F3, search: u32) {
        let nd = &self.nodes[ni];
        if search != nd.index {
            let dx = nd.co[0] - q[0];
            let dy = nd.co[1] - q[1];
            let dz = nd.co[2] - q[2];
            let dsq = dx * dx + dy * dy + dz * dz;
            if dsq <= self.range_sq {
                if self.dup[nd.index as usize] == -1 {
                    self.dup[nd.index as usize] = search as i32;
                }
                self.found += 1;
            }
        }
        let d = nd.d as usize;
        let (l, r, c) = (nd.left, nd.right, nd.co[d]);
        if q[d] + self.range >= c && r != -1 {
            self.rec(r as usize, q, search);
        }
        if q[d] - self.range <= c && l != -1 {
            self.rec(l as usize, q, search);
        }
    }
}

/// BLI_kdtree_3d_calc_duplicates_fast over vertices in index order (bmesh remove_doubles without keep_verts); returns the target of every vertex
pub fn remove_doubles_targets(co: &[F3], range: f32) -> Vec<u32> {
    let n = co.len();
    let mut nodes: Vec<KdNode> = (0..n).map(|i| KdNode { co: co[i], index: i as u32, left: -1, right: -1, d: 0 }).collect();
    let root = kd_balance(&mut nodes, 0, n, 0);
    let mut dd = Dedup { nodes: &nodes, range, range_sq: range * range, dup: vec![-1; n], found: 0 };
    if root >= 0 {
        for i in 0..n {
            let index = nodes[i].index;
            let cur = dd.dup[index as usize];
            if cur == -1 || cur == index as i32 {
                let prev = dd.found;
                dd.rec(root as usize, nodes[i].co, index);
                if dd.found != prev {
                    dd.dup[index as usize] = index as i32;
                }
            }
        }
    }
    (0..n)
        .map(|i| {
            let mut t = i as u32;
            let mut guard = 0;
            while dd.dup[t as usize] != -1 && dd.dup[t as usize] != t as i32 && guard < 64 {
                t = dd.dup[t as usize] as u32;
                guard += 1;
            }
            t
        })
        .collect()
}

/// bpy.ops.mesh.remove_doubles(threshold): survivors keep their relative order; faces are remapped, consecutive duplicates dropped, faces with < 3 vertices removed
pub fn remove_doubles(m: &Mesh, th: f32) -> Mesh {
    let co: Vec<F3> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let tgt = remove_doubles_targets(&co, th);
    let n = m.pos.len();
    let mut newidx = vec![u32::MAX; n];
    let mut out = Mesh::new();
    for i in 0..n {
        if tgt[i] == i as u32 {
            newidx[i] = out.pos.len() as u32;
            out.pos.push(m.pos[i]);
        }
    }
    for f in &m.faces {
        let mut u: Vec<u32> = vec![];
        for &v in f {
            let t = tgt[v as usize];
            if u.last() != Some(&t) {
                u.push(t);
            }
        }
        if u.len() > 1 && u[0] == *u.last().unwrap() {
            u.pop();
        }
        let mut uniq = u.clone();
        uniq.sort();
        uniq.dedup();
        if u.len() < 3 || uniq.len() < 3 {
            continue;
        }
        out.faces.push(u.iter().map(|&v| newidx[v as usize]).collect());
    }
    out
}

// ---------------------------------------------------------------- bpy.ops.mesh.spin (full revolution about +z)

/// spin of a polyline profile (loose edges i -- i+1) `steps` times over 2 pi about z: ring k+1 = R * ring k in float32 (Blender duplicates and rotates the previous copy)
pub fn spin_profile(profile: &[F3], steps: usize) -> Mesh {
    let n = profile.len();
    // bpy.ops.mesh.spin: angle property is a float32 (2 pi), phi = angle / steps in float, rotation about -z
    let a = -((2.0 * std::f64::consts::PI) as f32 / steps as f32);
    let c = (a as f64).cos() as f32;
    let s = (a as f64).sin() as f32;
    let mut m = Mesh::new();
    let mut ring: Vec<F3> = profile.to_vec();
    for k in 0..steps {
        for p in &ring {
            m.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
        }
        let next: Vec<F3> = ring.iter().map(|p| [p[0] * c + p[1] * -s, p[0] * s + p[1] * c, p[2]]).collect();
        let _ = k;
        ring = next;
    }
    for k in 0..steps {
        let k1 = (k + 1) % steps;
        for e in 0..n - 1 {
            m.faces.push(vec![(k * n + e) as u32, (k * n + e + 1) as u32, (k1 * n + e + 1) as u32, (k1 * n + e) as u32]);
        }
    }
    m
}

// ---------------------------------------------------------------- bezier_curve(...) of assets/utils/draw.py

#[derive(Clone, Copy, PartialEq)]
enum Ht {
    Auto,
    Vector,
    Align,
    Free,
}

#[derive(Clone)]
struct Bz {
    co: F3,
    h1: F3,
    h2: F3,
    t1: Ht,
    t2: Ht,
}

fn v_sub(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v_len(a: F3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn v_madd(a: F3, d: F3, f: f32) -> F3 {
    [a[0] + d[0] * f, a[1] + d[1] * f, a[2] + d[2] * f]
}
fn v_lerp(a: F3, b: F3, t: f32) -> F3 {
    let s = 1.0f32 - t;
    [s * a[0] + t * b[0], s * a[1] + t * b[1], s * a[2] + t * b[2]]
}

/// BKE_nurb_handles_calc (calchandleNurb_intern for every point of an open spline)
fn calc_handles(pts: &mut [Bz]) {
    let n = pts.len();
    for i in 0..n {
        let p2 = pts[i].co;
        let p1 = if i > 0 { pts[i - 1].co } else { v_sub([2.0 * p2[0], 2.0 * p2[1], 2.0 * p2[2]], pts[i + 1].co) };
        let p3 = if i + 1 < n { pts[i + 1].co } else { v_sub([2.0 * p2[0], 2.0 * p2[1], 2.0 * p2[2]], p1) };
        let da = v_sub(p2, p1);
        let db = v_sub(p3, p2);
        let mut la = v_len(da);
        let mut lb = v_len(db);
        if la == 0.0 {
            la = 1.0;
        }
        if lb == 0.0 {
            lb = 1.0;
        }
        let b = &mut pts[i];
        if b.t1 == Ht::Free && b.t2 == Ht::Free {
            continue;
        }
        if b.t1 == Ht::Auto || b.t2 == Ht::Auto {
            let tv = [db[0] / lb + da[0] / la, db[1] / lb + da[1] / la, db[2] / lb + da[2] / la];
            let len = v_len(tv) * 2.5614f32;
            if len != 0.0 {
                if la > 5.0 * lb {
                    la = 5.0 * lb;
                }
                if lb > 5.0 * la {
                    lb = 5.0 * la;
                }
                if b.t1 == Ht::Auto {
                    b.h1 = v_madd(p2, tv, -(la / len));
                }
                if b.t2 == Ht::Auto {
                    b.h2 = v_madd(p2, tv, lb / len);
                }
            }
        }
        if b.t1 == Ht::Vector {
            b.h1 = v_madd(p2, da, -1.0 / 3.0);
        }
        if b.t2 == Ht::Vector {
            b.h2 = v_madd(p2, db, 1.0 / 3.0);
        }
        // calchandleNurb_intern, HD_ALIGN part (handle_sel_flag = 0: h1 is re-aligned from h2 first, then h2 from the new h1)
        if (b.t1 == Ht::Align || b.t2 == Ht::Align) && b.t1 != Ht::Free && b.t2 != Ht::Free {
            let mut len_a = v_len(v_sub(p2, b.h1));
            let mut len_b = v_len(v_sub(p2, b.h2));
            if len_a == 0.0 {
                len_a = 1.0;
            }
            if len_b == 0.0 {
                len_b = 1.0;
            }
            let ratio = len_a / len_b;
            let eps = 1e-5f32;
            if b.t1 == Ht::Align && len_b > eps {
                let l = ratio;
                b.h1 = [p2[0] + l * (p2[0] - b.h2[0]), p2[1] + l * (p2[1] - b.h2[1]), p2[2] + l * (p2[2] - b.h2[2])];
            }
            if b.t2 == Ht::Align && len_a > eps {
                let l = 1.0f32 / ratio;
                b.h2 = [p2[0] + l * (p2[0] - b.h1[0]), p2[1] + l * (p2[1] - b.h1[1]), p2[2] + l * (p2[2] - b.h1[2])];
            }
        }
    }
}

/// draw.bezier_curve(anchors, vector_locations) mesh vertices: Blender curve with AUTO/VECTOR handles, then curve2mesh's reversed per-segment
/// `curve.subdivide(number_cuts)` (de Casteljau splits with handle recalculation after every subdivide), then the WELD modifier (cluster centroid)
pub fn bezier_curve_points(anchors: &[[f64; 3]], vector: &[usize]) -> Vec<F3> {
    bezier_core(anchors, vector, None)
}

/// draw.align_bezier(anchors, axes, scale): handles of points with an axis are projected on it and scaled (FREE -> ALIGNED)
pub fn align_bezier_points(anchors: &[[f64; 3]], axes: &[Option<[f64; 3]>], scale: &[f64]) -> Vec<F3> {
    bezier_core(anchors, &[], Some((axes, scale)))
}

fn bezier_core(anchors: &[[f64; 3]], vector: &[usize], edit: Option<(&[Option<[f64; 3]>], &[f64])>) -> Vec<F3> {
    let a32: Vec<F3> = anchors.iter().map(|a| [a[0] as f32, a[1] as f32, a[2] as f32]).collect();
    let mut pts: Vec<Bz> = (0..a32.len())
        .map(|i| {
            let t = if vector.contains(&i) { Ht::Vector } else { Ht::Auto };
            Bz { co: a32[i], h1: a32[i], h2: a32[i], t1: t, t2: t }
        })
        .collect();
    calc_handles(&mut pts);
    if let Some((axes, scale)) = edit {
        let mut sc = vec![1.0f64];
        sc.extend_from_slice(scale);
        sc.push(1.0);
        for (i, p) in pts.iter_mut().enumerate() {
            if let Some(a) = axes[i] {
                // numpy float64 arithmetic on the float32 handle / anchor values, one rounding to float32 when the handle is stored
                let hnew = |h: F3, co: F3, s: f64| -> F3 {
                    // `p.handle_left - p.co` is a mathutils Vector subtraction: float32
                    let d = [(h[0] - co[0]) as f64, (h[1] - co[1]) as f64, (h[2] - co[2]) as f64];
                    let k = d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
                    let pv = [k * a[0], k * a[1], k * a[2]];
                    let pl = (pv[0] * pv[0] + pv[1] * pv[1] + pv[2] * pv[2]).sqrt();
                    let dl = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let r = |i: usize| (co[i] as f64 + pv[i] / pl * dl * s) as f32;
                    [r(0), r(1), r(2)]
                };
                let (h1, h2) = (hnew(p.h1, p.co, sc[2 * i]), hnew(p.h2, p.co, sc[2 * i + 1]));
                p.h1 = h1;
                p.h2 = h2;
                // draw.align_bezier leaves the edited handles FREE; curve2mesh's retyping to ALIGNED goes through point references taken before
                // entering edit mode and has no effect, so the handles are never re-aligned (BKE_nurb_handles_calc returns early for FREE / FREE)
                p.t1 = Ht::Free;
                p.t2 = Ht::Free;
            }
        }
    }
    let lens: Vec<f64> = (0..a32.len() - 1).map(|i| (0..3).map(|k| (a32[i][k] as f64 - a32[i + 1][k] as f64).powi(2)).sum::<f64>().sqrt()).collect();
    for i in (0..a32.len() - 1).rev() {
        let k = ((lens[i] / 5e-3) as i64 - 1).min(64);
        if k < 0 {
            continue;
        }
        // bpy.ops.curve.subdivide(number_cuts=0) clamps to one cut
        let k = (k as usize).max(1);
        let (mut c0, mut h0, mut h1, c1) = (pts[i].co, pts[i].h2, pts[i + 1].h1, pts[i + 1].co);
        let mut new: Vec<Bz> = vec![];
        for j in 0..k {
            let f = 1.0f32 / (k + 1 - j) as f32;
            let a = v_lerp(c0, h0, f);
            let b = v_lerp(h0, h1, f);
            let c = v_lerp(h1, c1, f);
            let ab = v_lerp(a, b, f);
            let bc = v_lerp(b, c, f);
            let nn = v_lerp(ab, bc, f);
            if j == 0 {
                pts[i].h2 = a;
            } else {
                let last = new.len() - 1;
                new[last].h2 = a;
            }
            new.push(Bz { co: nn, h1: ab, h2: bc, t1: pts[i + 1].t1, t2: pts[i + 1].t2 });
            c0 = nn;
            h0 = bc;
            h1 = c;
        }
        pts[i + 1].h1 = h1;
        let at = i + 1;
        for (o, q) in new.into_iter().enumerate() {
            pts.insert(at + o, q);
        }
        calc_handles(&mut pts);
    }
    // WELD modifier: clusters of vertices closer than 1e-3 collapse to their centroid
    let co: Vec<F3> = pts.iter().map(|p| p.co).collect();
    if std::env::var("BLDBG").is_ok() { for q in &co { eprintln!("PRE {:?}", q); } }
    weld_polyline(&co, 1e-3)
}

/// WELD modifier (mode all) on a vertex list: greedy in index order, an unmerged vertex absorbs the later unmerged vertices within `th`;
/// the survivor moves to the centroid of its cluster
pub fn weld_polyline(pts: &[F3], th: f32) -> Vec<F3> {
    let n = pts.len();
    let mut merged = vec![false; n];
    let mut out: Vec<F3> = vec![];
    for i in 0..n {
        if merged[i] {
            continue;
        }
        let mut members = vec![i];
        for j in i + 1..n {
            if !merged[j] && v_len(v_sub(pts[j], pts[i])) <= th {
                merged[j] = true;
                members.push(j);
            }
        }
        if members.len() == 1 {
            out.push(pts[i]);
        } else {
            let m = members.len() as f32;
            let mut c = [0.0f32; 3];
            for &q in &members {
                for k in 0..3 {
                    c[k] += pts[q][k];
                }
            }
            out.push([c[0] / m, c[1] / m, c[2] / m]);
        }
    }
    out
}

/// draw.spin(anchors, vector_locations) for an axis-z profile: bezier mesh -> mean radius -> steps -> spin -> remove_doubles(1e-3)
pub fn spin_anchors(anchors: &[[f64; 3]], vector: &[usize]) -> Mesh {
    let prof = bezier_curve_points(anchors, vector);
    let mean_r: f64 = prof.iter().map(|p| ((p[0] * p[0] + p[1] * p[1]) as f64).sqrt()).sum::<f64>() / prof.len() as f64;
    let steps = ((2.0 * std::f64::consts::PI * mean_r / 5e-3) as usize).min(128);
    if std::env::var("BLDBG").is_ok() { eprintln!("PROF {} steps {} mean_r {}", prof.len(), steps, mean_r); for p in &prof { eprintln!("P {:?}", p); } }
    let m = spin_profile(&prof, steps);
    let r = remove_doubles(&m, 1e-3);
    if std::env::var("BLDBG").is_ok() { eprintln!("SPIN verts {} faces {} (before {} {})", r.pos.len(), r.faces.len(), m.pos.len(), m.faces.len()); }
    r
}
