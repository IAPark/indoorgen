//! SOLIDIFY modifier, mode NON_MANIFOLD, thickness mode CONSTRAINTS, boundary FLAT/ROUND/NONE (MOD_solidify_nonmanifold.cc) for surfaces whose
//! every vertex has a single face fan (manifold interior or boundary vertices, consistently oriented faces). No vertex merging, no
//! self-intersection splitting, no vertex groups, no clamping. float32 arithmetic as in Blender.
//! Output: front-shell vertex i = original i displaced by thickness*offset, back-shell vertex n+i; faces: shell, reversed shell, rim.
use crate::bm::query::*;
use crate::dhash::HashMap;

type V3 = [f32; 3];

pub struct SolidifyOut {
    pub pos: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
}

fn clamp_nonzero(v: f32, eps: f32) -> f32 {
    if v < 0.0 {
        v.min(-eps)
    } else {
        v.max(eps)
    }
}

pub fn face_normal32(pos: &[V3], f: &[u32]) -> V3 {
    let p = |i: usize| pos[f[i] as usize];
    match f.len() {
        3 => normalize(cross(sub(p(1), p(0)), sub(p(2), p(0)))),
        4 => normalize(cross(sub(p(2), p(0)), sub(p(3), p(1)))),
        n => {
            // newell
            let mut no = [0.0f32; 3];
            let mut v_prev = p(n - 1);
            for i in 0..n {
                let v_curr = p(i);
                no[0] += (v_prev[1] - v_curr[1]) * (v_prev[2] + v_curr[2]);
                no[1] += (v_prev[2] - v_curr[2]) * (v_prev[0] + v_curr[0]);
                no[2] += (v_prev[0] - v_curr[0]) * (v_prev[1] + v_curr[1]);
                v_prev = v_curr;
            }
            normalize(no)
        }
    }
}

fn invert_m3(m: &mut [[f32; 3]; 3]) -> bool {
    let a = *m;
    let mut r = [[0.0f32; 3]; 3];
    r[0][0] = a[1][1] * a[2][2] - a[1][2] * a[2][1];
    r[0][1] = -a[0][1] * a[2][2] + a[0][2] * a[2][1];
    r[0][2] = a[0][1] * a[1][2] - a[0][2] * a[1][1];
    r[1][0] = -a[1][0] * a[2][2] + a[1][2] * a[2][0];
    r[1][1] = a[0][0] * a[2][2] - a[0][2] * a[2][0];
    r[1][2] = -a[0][0] * a[1][2] + a[0][2] * a[1][0];
    r[2][0] = a[1][0] * a[2][1] - a[1][1] * a[2][0];
    r[2][1] = -a[0][0] * a[2][1] + a[0][1] * a[2][0];
    r[2][2] = a[0][0] * a[1][1] - a[0][1] * a[1][0];
    let det = a[0][0] * r[0][0] + a[0][1] * r[1][0] + a[0][2] * r[2][0];
    if det == 0.0 {
        return false;
    }
    let id = 1.0f32 / det;
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] *= id;
        }
    }
    *m = r;
    true
}

fn v4(n: V3, o: f32) -> [f32; 4] {
    [n[0], n[1], n[2], o]
}
fn n3(p: &[f32; 4]) -> V3 {
    [p[0], p[1], p[2]]
}

#[derive(Clone, Copy, PartialEq)]
pub enum Boundary {
    None,
    Round,
    Flat,
}

/// position offset `nor` of one edge group (CONSTRAINTS method). `open`: the fan ends in boundary edges.
#[allow(clippy::too_many_arguments)]
fn group_offset(mut planes: Vec<[f32; 4]>, open: bool, edges_dirs: &[V3], face_dirs_for_round: Option<(V3, V3)>, offset_fac: f32, boundary: Boundary) -> V3 {
    let edges_len = edges_dirs.len();
    let mut nor = [0.0f32; 3];
    let mut move_nor = [0.0f32; 3];
    let mut disable_boundary_fix = boundary == Boundary::None || !open;
    let mut approximate_free_direction = false;
    let mut queue_index = planes.len();
    if queue_index > 2 {
        let mut min_p = 2.0f32;
        let (mut min_n0, mut min_n1) = (0usize, 0usize);
        for k in 0..queue_index {
            for m in k + 1..queue_index {
                let p = dot(n3(&planes[k]), n3(&planes[m]));
                if p < min_p {
                    min_p = p;
                    min_n0 = k;
                    min_n1 = m;
                }
            }
        }
        if min_n1 != 0 {
            planes.swap(min_n0, 0);
            planes.swap(min_n1, 1);
        } else {
            planes.swap(min_n0, 1);
        }
        min_p = 1.0;
        min_n1 = 2;
        for k in 2..queue_index {
            let max_p = dot(n3(&planes[0]), n3(&planes[k])).max(dot(n3(&planes[1]), n3(&planes[k])));
            if max_p <= min_p {
                min_p = max_p;
                min_n1 = k;
            }
        }
        planes.swap(min_n1, 2);
    }
    while queue_index > 2 {
        let (mut best_n0, mut best_n1) = (0usize, 0usize);
        let mut best_p = -1.0f32;
        let mut best_ofs_diff = 0.0f32;
        for k in 0..queue_index {
            for m in k + 1..queue_index {
                let p = dot(n3(&planes[m]), n3(&planes[k]));
                let ofs_diff = (planes[m][3] - planes[k][3]).abs();
                if p > best_p + f32::EPSILON || (p >= best_p && ofs_diff < best_ofs_diff) {
                    best_p = p;
                    best_ofs_diff = ofs_diff;
                    best_n0 = k;
                    best_n1 = m;
                }
            }
        }
        if best_p < 0.98 {
            break;
        }
        let s = add(n3(&planes[best_n0]), n3(&planes[best_n1]));
        let sn = normalize(s);
        let ofs = (planes[best_n0][3] + planes[best_n1][3]) * 0.5f32;
        planes[best_n0] = v4(sn, ofs);
        queue_index -= 1;
        planes.remove(best_n1);
    }
    planes.truncate(queue_index);
    let size = queue_index;
    let boundary_fix_threshold = 0.7f32;
    if size > 3 {
        let mut mat = [[0.0f32; 3]; 3];
        for k in 0..3 {
            for m in 0..size {
                mat[k] = madd(mat[k], n3(&planes[m]), planes[m][k]);
            }
            mat[k][k] += 5e-5f32;
        }
        invert_m3(&mut mat);
        nor = [0.0; 3];
        for k in 0..size {
            nor = madd(nor, n3(&planes[k]), planes[k][3]);
        }
        // mul_v3_m3v3: r[i] = M[0][i]*a[0] + M[1][i]*a[1] + M[2][i]*a[2]
        let a = nor;
        nor = [mat[0][0] * a[0] + mat[1][0] * a[1] + mat[2][0] * a[2], mat[0][1] * a[0] + mat[1][1] * a[1] + mat[2][1] * a[2], mat[0][2] * a[0] + mat[1][2] * a[1] + mat[2][2] * a[2]];
        if !disable_boundary_fix {
            let mut greatest_angle_cos = 1.0f32;
            for k in 0..2 {
                for m in 2..size {
                    let p = dot(n3(&planes[m]), n3(&planes[k]));
                    if p < greatest_angle_cos {
                        greatest_angle_cos = p;
                    }
                }
            }
            if greatest_angle_cos > boundary_fix_threshold {
                approximate_free_direction = true;
            } else {
                disable_boundary_fix = true;
            }
        }
    } else if size > 1 {
        let stop_explosion = 0.999f32 - offset_fac.abs() * 0.05f32;
        let q = dot(n3(&planes[0]), n3(&planes[1]));
        let mut d = 1.0f32 - q * q;
        move_nor = normalize(cross(n3(&planes[0]), n3(&planes[1])));
        if d > f32::EPSILON * 10.0 && q < stop_explosion {
            d = 1.0f32 / d;
            let (p0, p1) = (planes[0], planes[1]);
            let a = mul(n3(&p0), (p0[3] - p1[3] * q) * d);
            let b = mul(n3(&p1), (p1[3] - p0[3] * q) * d);
            planes[0] = v4(a, p0[3]);
            planes[1] = v4(b, p1[3]);
        } else {
            d = 1.0f32 / (q.abs() + 1.0f32);
            let (p0, p1) = (planes[0], planes[1]);
            planes[0] = v4(mul(n3(&p0), p0[3] * d), p0[3]);
            planes[1] = v4(mul(n3(&p1), p1[3] * d), p1[3]);
        }
        nor = add(n3(&planes[0]), n3(&planes[1]));
        if size == 3 {
            d = dot(n3(&planes[2]), move_nor);
            if d.abs() > 0.02 {
                let tmp = madd(nor, n3(&planes[2]), -planes[2][3]);
                let tmp = mul(move_nor, dot(n3(&planes[2]), tmp) / d);
                nor = sub(nor, tmp);
                if d.abs() > 1.0 - boundary_fix_threshold {
                    disable_boundary_fix = true;
                }
            }
        }
        approximate_free_direction = false;
    } else if size == 1 {
        nor = mul(n3(&planes[0]), planes[0][3]);
        if edges_len > 2 {
            disable_boundary_fix = true;
            approximate_free_direction = true;
        }
    } else {
        disable_boundary_fix = true;
    }
    if approximate_free_direction {
        let mut k = 1;
        while k + 1 < edges_len {
            move_nor = add(move_nor, edges_dirs[k]);
            k += 1;
        }
        if k == 1 {
            disable_boundary_fix = true;
        } else {
            let (mn, l) = normalize_len(move_nor);
            move_nor = mn;
            disable_boundary_fix = l == 0.0;
        }
    }
    if !disable_boundary_fix {
        let e0 = edges_dirs[0];
        let e1 = edges_dirs[edges_len - 1];
        let constr_nor;
        if boundary == Boundary::Flat {
            constr_nor = normalize(cross(e0, e1));
        } else {
            let (f0, f1) = face_dirs_for_round.unwrap_or(([0.0; 3], [0.0; 3]));
            let n0 = normalize(cross(e0, f0));
            let n1 = normalize(cross(f1, e1));
            constr_nor = normalize(add(n0, n1));
        }
        let d = dot(constr_nor, move_nor);
        if d.abs() > 0.1 {
            move_nor = mul(move_nor, dot(constr_nor, nor) / d);
            nor = sub(nor, move_nor);
        }
    }
    nor
}

/// `pos`: float32 positions, `faces`: consistently oriented polygons. offset_fac = modifier `offset` (+1 / -1 / 0), thickness = `thickness`.
pub fn solidify_nonmanifold(pos: &[V3], faces: &[Vec<u32>], thickness: f32, offset_fac: f32, boundary: Boundary) -> Result<SolidifyOut, String> {
    let n = pos.len();
    let ofs_front = (offset_fac + 1.0f32) * 0.5f32 * thickness;
    let ofs_back = ofs_front - thickness * offset_fac;
    let ofs_front_c = clamp_nonzero(ofs_front, 1e-5);
    let ofs_back_c = clamp_nonzero(ofs_back, 1e-5);
    let fno: Vec<V3> = faces.iter().map(|f| face_normal32(pos, f)).collect();
    // edge -> faces
    let mut efaces: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, f) in faces.iter().enumerate() {
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            efaces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    if efaces.values().any(|v| v.len() > 2) {
        return Err("non-manifold edge".into());
    }
    // vertex -> (face, corner)
    let mut vcorners: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (fi, f) in faces.iter().enumerate() {
        for (c, &v) in f.iter().enumerate() {
            vcorners[v as usize].push((fi, c));
        }
    }
    let mut front = vec![[0.0f32; 3]; n];
    let mut back = vec![[0.0f32; 3]; n];
    for v in 0..n {
        let corners = &vcorners[v];
        if corners.is_empty() {
            front[v] = pos[v];
            back[v] = pos[v];
            continue;
        }
        let prev_of = |fi: usize, c: usize| faces[fi][(c + faces[fi].len() - 1) % faces[fi].len()];
        let next_of = |fi: usize, c: usize| faces[fi][(c + 1) % faces[fi].len()];
        let corner_of = |fi: usize| corners.iter().find(|x| x.0 == fi).unwrap().1;
        // start: open chain -> the face whose previous edge is a boundary edge
        let mut start = None;
        for &(fi, c) in corners {
            let p = prev_of(fi, c);
            if efaces[&(p.min(v as u32), p.max(v as u32))].len() == 1 {
                if start.is_some() {
                    return Err("vertex with several fans".into());
                }
                start = Some(fi);
            }
        }
        let open = start.is_some();
        let s = start.unwrap_or(corners[0].0);
        let mut fan = vec![s];
        let mut cur = s;
        loop {
            let c = corner_of(cur);
            let nx = next_of(cur, c);
            let l = &efaces[&(nx.min(v as u32), nx.max(v as u32))];
            let other = l.iter().copied().find(|&g| g != cur);
            match other {
                None => break,
                Some(g) => {
                    if g == s {
                        break;
                    }
                    fan.push(g);
                    cur = g;
                }
            }
        }
        if fan.len() != corners.len() {
            return Err("vertex with several fans".into());
        }
        let m = fan.len();
        // neighbours n_0 = prev(F0), n_k = next(F_{k-1})
        let mut nb: Vec<u32> = vec![prev_of(fan[0], corner_of(fan[0]))];
        for k in 1..=m {
            let f = fan[k - 1];
            nb.push(next_of(f, corner_of(f)));
        }
        let dirs: Vec<V3> = if open { nb.iter().map(|&q| sub(pos[q as usize], pos[v])).collect() } else { nb[..m].iter().map(|&q| sub(pos[q as usize], pos[v])).collect() };
        let pf: Vec<[f32; 4]> = fan.iter().map(|&f| v4(fno[f], ofs_front_c)).collect();
        let pb: Vec<[f32; 4]> = fan.iter().map(|&f| v4(neg(fno[f]), ofs_back_c)).collect();
        let round = Some((fno[fan[0]], fno[fan[m - 1]]));
        let roundb = Some((neg(fno[fan[0]]), neg(fno[fan[m - 1]])));
        let nf = group_offset(pf, open, &dirs, round, offset_fac, boundary);
        let nbk = group_offset(pb, open, &dirs, roundb, offset_fac, boundary);
        front[v] = add(nf, pos[v]);
        back[v] = add(nbk, pos[v]);
    }
    let mut out_pos = front;
    out_pos.extend(back);
    let mut out_faces: Vec<Vec<u32>> = vec![];
    for f in faces {
        out_faces.push(f.clone());
    }
    for f in faces {
        out_faces.push(f.iter().rev().map(|&v| v + n as u32).collect());
    }
    // rim: boundary edges
    for (fi, f) in faces.iter().enumerate() {
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            if efaces[&(a.min(b), a.max(b))].len() == 1 {
                let _ = fi;
                out_faces.push(vec![b, a, a + n as u32, b + n as u32]);
            }
        }
    }
    Ok(SolidifyOut { pos: out_pos, faces: out_faces })
}
