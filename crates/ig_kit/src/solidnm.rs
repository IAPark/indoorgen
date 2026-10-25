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
        3 => normalize(cross(sub(p(0), p(1)), sub(p(1), p(2)))), // normal_tri_v3
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
fn group_offset(mut planes: Vec<[f32; 4]>, fallback: Option<(V3, f32)>, open: bool, edges_dirs: &[V3], face_dirs_for_round: Option<(V3, V3)>, offset_fac: f32, boundary: Boundary) -> V3 {
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
        // fallback case for null faces
        if let Some((fnor, fofs)) = fallback {
            nor = mul(fnor, fofs);
        }
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
    let mut fno: Vec<V3> = faces.iter().map(|f| face_normal32(pos, f)).collect();
    let mut null_face = vec![false; faces.len()];
    for (fi, f) in faces.iter().enumerate() {
        if dot(fno[fi], fno[fi]) < 0.5 {
            let d = sub(pos[f[1] as usize], pos[f[0] as usize]);
            if d[2].abs() < d[1].abs() {
                fno[fi][2] = 1.0;
            } else {
                fno[fi][1] = 1.0;
            }
            null_face[fi] = true;
        }
    }
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
        let pf: Vec<[f32; 4]> = fan.iter().filter(|&&f| !null_face[f]).map(|&f| v4(fno[f], ofs_front_c)).collect();
        let pb: Vec<[f32; 4]> = fan.iter().filter(|&&f| !null_face[f]).map(|&f| v4(neg(fno[f]), ofs_back_c)).collect();
        // last null face of the fan is the fallback (the C loop overwrites fallback_nor for every null face)
        let fb_f = fan.iter().rev().find(|&&f| null_face[f]).map(|&f| (fno[f], ofs_front_c));
        let fb_b = fan.iter().rev().find(|&&f| null_face[f]).map(|&f| (neg(fno[f]), ofs_back_c));
        let round = Some((fno[fan[0]], fno[fan[m - 1]]));
        let roundb = Some((neg(fno[fan[0]]), neg(fno[fan[m - 1]])));
        if std::env::var("SOLDBG").ok().and_then(|x| x.parse::<usize>().ok()) == Some(v) {
            eprintln!("solidnm v {v} open {open} fan {:?} planes {:?} dirs {:?}", fan, pf, dirs);
        }
        let nf = group_offset(pf, fb_f, open, &dirs, round, offset_fac, boundary);
        let nbk = group_offset(pb, fb_b, open, &dirs, roundb, offset_fac, boundary);
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


// ---------------------------------------------------------------------------------------------------------------------------------------
// Faithful port of the edge-group construction of MOD_solidify_nonmanifold.cc (no vertex merging / duplicate-face filtering / self-intersection
// splitting): the ORDER of the constraint planes of a vertex (decides the dedupe / least-squares choice) follows Blender's group walk.

#[derive(Clone, Copy)]
struct NewEdge {
    old_edge: usize,
    /// face-side refs (face*2 + reversed); `None` = open
    faces: [Option<usize>; 2],
}

struct Group {
    edges: Vec<usize>,
    is_orig_closed: bool,
}

fn angle_signed_on_axis(n: V3, ref_n: V3, axis: V3) -> f32 {
    let d = dot(n, ref_n).clamp(-1.0, 1.0);
    let mut angle = d.acos();
    let c = cross(n, ref_n);
    if dot(c, axis) >= 0.0 {
        angle = (2.0 * std::f64::consts::PI - angle as f64) as f32;
    }
    angle
}

/// Blender-like edge list when none is known: first use by the face corners, as (corner vertex, next vertex)
pub fn edges_by_first_use(faces: &[Vec<u32>]) -> Vec<[u32; 2]> {
    let mut seen: HashMap<(u32, u32), ()> = HashMap::default();
    let mut out = vec![];
    for f in faces {
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            if seen.insert((a.min(b), a.max(b)), ()).is_none() {
                out.push([a, b]);
            }
        }
    }
    out
}

/// `edges`: the Blender mesh's edge list (order and vertex order inside each edge matter for the plane order), None = `edges_by_first_use`.
pub fn solidify_nonmanifold_ex(pos: &[V3], faces: &[Vec<u32>], edges: Option<&[[u32; 2]]>, thickness: f32, offset_fac: f32, boundary: Boundary) -> Result<SolidifyOut, String> {
    let n = pos.len();
    let owned;
    let edges: &[[u32; 2]] = match edges {
        Some(e) => e,
        None => {
            owned = edges_by_first_use(faces);
            &owned
        }
    };
    let ofs_front = (offset_fac + 1.0f32) * 0.5f32 * thickness;
    let ofs_back = ofs_front - thickness * offset_fac;
    let ofs_front_c = clamp_nonzero(ofs_front, 1e-5);
    let ofs_back_c = clamp_nonzero(ofs_back, 1e-5);
    // Mesh::face_normals() as the modifier sees it: Newell for every face size (verified against Blender 4.2; python's polygon.normal of tri / quad
    // faces differs in the last bits, which decides near-parallel plane merges)
    let mut fno: Vec<V3> = faces.iter().map(|f| crate::solidx::face_normal_v(pos, f)).collect();
    let mut null_face = vec![false; faces.len()];
    let mut emap: HashMap<(u32, u32), usize> = HashMap::default();
    for (i, e) in edges.iter().enumerate() {
        emap.insert((e[0].min(e[1]), e[0].max(e[1])), i);
    }
    let corner_edge = |f: &Vec<u32>, c: usize| -> Result<usize, String> {
        let (a, b) = (f[c], f[(c + 1) % f.len()]);
        emap.get(&(a.min(b), a.max(b))).copied().ok_or_else(|| "face edge missing in edge list".to_string())
    };
    for (fi, f) in faces.iter().enumerate() {
        if dot(fno[fi], fno[fi]) < 0.5 {
            let e = edges[corner_edge(f, 0)?];
            let d = sub(pos[e[1] as usize], pos[e[0] as usize]);
            if d[2].abs() < d[1].abs() {
                fno[fi][2] = 1.0;
            } else {
                fno[fi][1] = 1.0;
            }
            null_face[fi] = true;
        }
    }
    // edge -> adjacent (face, reversed) in face order
    let mut adj: Vec<Vec<(usize, bool)>> = vec![vec![]; edges.len()];
    for (fi, f) in faces.iter().enumerate() {
        for c in 0..f.len() {
            let e = corner_edge(f, c)?;
            adj[e].push((fi, edges[e][1] != f[c]));
        }
    }
    // new edges
    let mut new_edges: Vec<NewEdge> = vec![];
    let mut edge_new: Vec<Vec<usize>> = vec![vec![]; edges.len()];
    for (i, e) in edges.iter().enumerate() {
        if adj[i].is_empty() {
            continue;
        }
        let (v1, v2) = (e[0] as usize, e[1] as usize);
        let (a, b) = if v2 < v1 { (v2, v1) } else { (v1, v2) };
        let d = sub(pos[b], pos[a]);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len * len <= 1e-8 || len <= f32::EPSILON {
            return Err("degenerate edge (merge path not ported)".into());
        }
        let edgedir = mul(sub(pos[v2], pos[v1]), 1.0f32 / len);
        let al = adj[i].len();
        let mut sorted: Vec<(f32, usize)> = vec![];
        if al > 1 {
            let mut ref_nor = [0.0f32; 3];
            for (j, &(fi, rev)) in adj[i].iter().enumerate() {
                let mut nor = if rev { neg(fno[fi]) } else { fno[fi] };
                let mut dd = 1.0f32;
                if faces[fi].len() > 3 {
                    dd = dot(nor, edgedir);
                    nor = sub(nor, mul(edgedir, dd));
                    if dd != 0.0 {
                        let (nn, l) = normalize_len(nor);
                        nor = nn;
                        dd = l;
                    } else {
                        dd = 1.0;
                    }
                }
                let angle = if dd == 0.0 {
                    0.0
                } else if j == 0 {
                    ref_nor = nor;
                    0.0
                } else {
                    -angle_signed_on_axis(nor, ref_nor, edgedir)
                };
                sorted.push((angle, fi * 2 + rev as usize));
            }
            crate::bpyops::libc_qsort(&mut sorted, &|x: &(f32, usize), y: &(f32, usize)| (x.0 > y.0) as i32 - (x.0 < y.0) as i32);
        } else {
            sorted.push((0.0, adj[i][0].0 * 2 + adj[i][0].1 as usize));
        }
        let cnt = if al > 1 { al } else { 2 };
        for j in 0..cnt {
            let ne = if al > 1 {
                let nj = if j + 1 == al { 0 } else { j + 1 };
                NewEdge { old_edge: i, faces: [Some(sorted[j].1), Some(sorted[nj].1 ^ 1)] }
            } else {
                NewEdge { old_edge: i, faces: [Some(sorted[0].1 ^ j), None] }
            };
            edge_new[i].push(new_edges.len());
            new_edges.push(ne);
        }
    }
    // vertex -> incident edges (edge index order)
    let mut vedges: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, e) in edges.iter().enumerate() {
        if adj[i].is_empty() {
            continue;
        }
        vedges[e[0] as usize].push(i);
        vedges[e[1] as usize].push(i);
    }
    let mut front = vec![[0.0f32; 3]; n];
    let mut back = vec![[0.0f32; 3]; n];
    let opp = |r: usize| r ^ 1;
    for v in 0..n {
        if vedges[v].len() < 2 {
            front[v] = pos[v];
            back[v] = pos[v];
            continue;
        }
        let un: Vec<usize> = vedges[v].iter().flat_map(|&e| edge_new[e].iter().copied()).collect();
        let mut unassigned: Vec<Option<usize>> = un.iter().map(|&x| Some(x)).collect();
        let mut groups: Vec<Group> = vec![];
        let mut track: [Option<usize>; 2] = [None, None];
        let mut last_open: Option<usize> = None;
        let mut assigned = 0;
        let mut eg: isize = -1;
        while assigned < unassigned.len() {
            let mut found: Option<(usize, usize)> = None;
            let mut at_start = false;
            if eg >= 0 && groups[eg as usize].edges.is_empty() {
                let mut j = 0;
                let mut edge: Option<usize> = None;
                while edge.is_none() && j < unassigned.len() {
                    edge = unassigned[j];
                    j += 1;
                    if let (Some(e), Some(lo)) = (edge, last_open) {
                        if new_edges[e].faces[0] != Some(lo) || new_edges[e].faces[1].is_some() {
                            edge = None;
                        }
                    }
                }
                if edge.is_none() && last_open.is_some() {
                    last_open = None;
                    j = 0;
                    while edge.is_none() && j < unassigned.len() {
                        edge = unassigned[j];
                        j += 1;
                    }
                }
                let e = edge.ok_or("edge group walk: no start edge")?;
                found = Some((e, j - 1));
                let ne = new_edges[e];
                if last_open.is_none() && edges[ne.old_edge][0] as usize == v {
                    track = [ne.faces[0], ne.faces[1]];
                    if ne.faces[1].is_none() {
                        last_open = Some(opp(ne.faces[0].unwrap()));
                    }
                } else {
                    track = [ne.faces[1], ne.faces[0]];
                }
            } else if eg >= 0 {
                for idx in 0..unassigned.len() {
                    let Some(e) = unassigned[idx] else { continue };
                    let ne = new_edges[e];
                    if ne.faces[0].is_some() && ne.faces[0] == track[1] {
                        at_start = false;
                        track[1] = ne.faces[1];
                        found = Some((e, idx));
                        if ne.faces[1].is_none() {
                            groups[eg as usize].is_orig_closed = false;
                            last_open = Some(opp(ne.faces[0].unwrap()));
                        }
                        break;
                    }
                    if ne.faces[0].is_some() && ne.faces[0] == track[0] {
                        at_start = true;
                        track[0] = ne.faces[1];
                        found = Some((e, idx));
                        if ne.faces[1].is_none() {
                            groups[eg as usize].is_orig_closed = false;
                        }
                        break;
                    }
                    if ne.faces[1].is_some() {
                        if ne.faces[1] == track[1] {
                            at_start = false;
                            track[1] = ne.faces[0];
                            found = Some((e, idx));
                            break;
                        }
                        if ne.faces[1] == track[0] {
                            at_start = true;
                            track[0] = ne.faces[0];
                            found = Some((e, idx));
                            break;
                        }
                    }
                }
            }
            if let Some((e, idx)) = found {
                unassigned[idx] = None;
                assigned += 1;
                let g = &mut groups[eg as usize];
                if at_start {
                    g.edges.insert(0, e);
                } else {
                    g.edges.push(e);
                }
                if new_edges[*g.edges.last().unwrap()].faces[1].is_some() {
                    last_open = None;
                }
            } else {
                eg += 1;
                groups.push(Group { edges: vec![], is_orig_closed: true });
                track = [None, None];
            }
        }
        // long groups containing the same old edge twice need the split logic (not ported)
        for g in &groups {
            if g.edges.len() > 3 {
                for a in 0..g.edges.len() {
                    for b in a + 1..g.edges.len() {
                        if new_edges[g.edges[a]].old_edge == new_edges[g.edges[b]].old_edge {
                            return Err("self-intersecting edge group (split not ported)".into());
                        }
                    }
                }
            }
        }
        let mut got_front = false;
        let mut got_back = false;
        for g in &groups {
            if g.edges.is_empty() {
                continue;
            }
            let glen = g.edges.len();
            let cycle = g.is_orig_closed;
            let mut planes: Vec<[f32; 4]> = vec![];
            let mut fallback: Option<(V3, f32)> = None;
            let mut first_edge: Option<usize> = None;
            for k in 0..glen {
                if k & 1 == 0 || (!cycle && k == glen - 1) {
                    let e = new_edges[g.edges[k]];
                    for l in 0..2 {
                        if let Some(face) = e.faces[l] {
                            let skip = match first_edge {
                                Some(fe) => new_edges[fe].faces[0] == Some(face) || new_edges[fe].faces[1] == Some(face),
                                None => false,
                            };
                            if !skip {
                                let rev = face & 1 == 1;
                                let ofs = if rev { ofs_back_c } else { ofs_front_c };
                                let fi = face >> 1;
                                let nn = if rev { neg(fno[fi]) } else { fno[fi] };
                                if !null_face[fi] {
                                    planes.push(v4(nn, ofs));
                                } else {
                                    fallback = Some((nn, ofs));
                                }
                            }
                        }
                    }
                    if (cycle && k == 0) || (!cycle && k + 3 >= glen) {
                        first_edge = Some(g.edges[k]);
                    }
                }
            }
            let dirs: Vec<V3> = g
                .edges
                .iter()
                .map(|&ne| {
                    let e = edges[new_edges[ne].old_edge];
                    let o = if e[0] as usize == v { e[1] } else { e[0] };
                    sub(pos[o as usize], pos[v])
                })
                .collect();
            let side = |ne: usize| -> V3 {
                let r = new_edges[ne].faces[0].unwrap();
                if r & 1 == 1 {
                    neg(fno[r >> 1])
                } else {
                    fno[r >> 1]
                }
            };
            let round = Some((side(g.edges[0]), side(g.edges[glen - 1])));
            let nor = group_offset(planes, fallback, !g.is_orig_closed, &dirs, round, offset_fac, boundary);
            let p = add(nor, pos[v]);
            if new_edges[g.edges[0]].faces[0].unwrap() & 1 == 1 {
                if got_back {
                    return Err("several back groups".into());
                }
                got_back = true;
                back[v] = p;
            } else {
                if got_front {
                    return Err("several front groups".into());
                }
                got_front = true;
                front[v] = p;
            }
        }
        if !got_front || !got_back {
            return Err("vertex without front/back group".into());
        }
    }
    let mut out_pos = front;
    out_pos.extend(back);
    let mut out_faces: Vec<Vec<u32>> = faces.to_vec();
    for f in faces {
        out_faces.push(f.iter().rev().map(|&v| v + n as u32).collect());
    }
    for (i, e) in edges.iter().enumerate() {
        let _ = e;
        if adj[i].len() == 1 {
            let (fi, _) = adj[i][0];
            let f = &faces[fi];
            for c in 0..f.len() {
                let (a, b) = (f[c], f[(c + 1) % f.len()]);
                if a.min(b) == edges[i][0].min(edges[i][1]) && a.max(b) == edges[i][0].max(edges[i][1]) {
                    out_faces.push(vec![b, a, a + n as u32, b + n as u32]);
                }
            }
        }
    }
    Ok(SolidifyOut { pos: out_pos, faces: out_faces })
}
