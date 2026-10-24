//! Blender 4.2 SOLIDIFY modifier (extrude mode) in float32: the plain variant used for the room prism
//! (`thickness`, `offset=-1`, rim) and the even-thickness / quality-normals shell variant (`MOD_solidify_extrude.cc`).
use super::pyroom::RawMesh;
use crate::bm::query::*;
use crate::bm::V3;

#[derive(Clone, Debug, Default)]
pub struct SMesh {
    pub pos: Vec<V3>,
    pub edges: Vec<[u32; 2]>,
    pub faces: Vec<Vec<u32>>,
    /// corner edges: for corner j of a face, the edge between vert j and vert j+1
    pub cedges: Vec<Vec<u32>>,
}

impl SMesh {
    pub fn from_raw(m: &RawMesh) -> SMesh {
        let mut map: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
        for (i, e) in m.edges.iter().enumerate() {
            map.entry((e[0].min(e[1]), e[0].max(e[1]))).or_insert(i as u32);
        }
        let cedges = m
            .faces
            .iter()
            .map(|f| {
                let k = f.len();
                (0..k).map(|j| map[&(f[j].min(f[(j + 1) % k]), f[j].max(f[(j + 1) % k]))]).collect()
            })
            .collect();
        SMesh { pos: m.pos.clone(), edges: m.edges.clone(), faces: m.faces.clone(), cedges }
    }
}

fn normalize_v3(a: V3) -> V3 {
    normalize_len(a).0
}
/// blender::math::normalize (divides by the length)
fn math_normalize(a: V3) -> V3 {
    let d = dot(a, a);
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0; 3]
    }
}
fn safe_acos_approx(x: f32) -> f32 {
    let f = x.abs();
    let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
    let a = (1.0 - m).sqrt() * (1.570_796_3_f32 + m * (-0.213_300_99_f32 + m * (0.077_980_48_f32 + m * -0.021_640_95_f32)));
    if x < 0.0 {
        std::f32::consts::PI - a
    } else {
        a
    }
}

/// Mesh::face_normals (normal_calc_ngon / Newell for every face)
pub fn face_normals(m: &SMesh) -> Vec<V3> {
    m.faces
        .iter()
        .map(|f| {
            let mut n = [0.0f32; 3];
            let mut vp = m.pos[*f.last().unwrap() as usize];
            for &v in f {
                let vc = m.pos[v as usize];
                n[0] += (vp[1] - vc[1]) * (vp[2] + vc[2]);
                n[1] += (vp[2] - vc[2]) * (vp[0] + vc[0]);
                n[2] += (vp[0] - vc[0]) * (vp[1] + vc[1]);
                vp = vc;
            }
            let (nn, l) = normalize_len(n);
            if l == 0.0 {
                [0.0, 0.0, 1.0]
            } else {
                nn
            }
        })
        .collect()
}

/// Mesh::vert_normals (normals_calc_verts)
pub fn vert_normals(m: &SMesh, fnor: &[V3]) -> Vec<V3> {
    let nv = m.pos.len();
    let mut acc: Vec<Option<V3>> = vec![None; nv];
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.len();
        for j in 0..k {
            let v = f[j] as usize;
            let (p, n) = (f[(j + k - 1) % k] as usize, f[(j + 1) % k] as usize);
            let dir_prev = math_normalize(sub(m.pos[p], m.pos[v]));
            let dir_next = math_normalize(sub(m.pos[n], m.pos[v]));
            let factor = safe_acos_approx(dot(dir_prev, dir_next));
            let a = acc[v].get_or_insert([0.0; 3]);
            a[0] += fnor[fi][0] * factor;
            a[1] += fnor[fi][1] * factor;
            a[2] += fnor[fi][2] * factor;
        }
    }
    (0..nv).map(|v| match acc[v] {
        None => math_normalize(m.pos[v]),
        Some(a) => math_normalize(a),
    }).collect()
}

fn mid_v3_v3v3_angle_weighted(a: V3, b: V3) -> V3 {
    let r = add(a, b);
    let (rn, l) = normalize_len(r);
    let angle = ((std::f64::consts::FRAC_2_PI as f32) * 2.0f32) * (l / 2.0f32).acos();
    mul(rn, angle)
}

/// mesh_calc_hq_normal
fn hq_normals(m: &SMesh, fnor: &[V3], vnor: &[V3]) -> Vec<V3> {
    let nv = m.pos.len();
    let ne = m.edges.len();
    let mut p1 = vec![0i64; ne];
    let mut p2 = vec![0i64; ne];
    for (i, ce) in m.cedges.iter().enumerate() {
        for &e in ce {
            let e = e as usize;
            if !(p1[e] == 0 && p2[e] == 0) {
                if p1[e] != -1 && p2[e] == -1 {
                    p2[e] = i as i64;
                } else {
                    p1[e] = -1;
                    p2[e] = -1;
                }
            } else {
                p1[e] = i as i64;
                p2[e] = -1;
            }
        }
    }
    let mut out = vec![[0.0f32; 3]; nv];
    for i in 0..ne {
        if !(p1[i] == 0 && p2[i] == 0) && p1[i] != -1 {
            let en = if p2[i] != -1 { mid_v3_v3v3_angle_weighted(fnor[p1[i] as usize], fnor[p2[i] as usize]) } else { fnor[p1[i] as usize] };
            let e = m.edges[i];
            for &v in &e {
                let o = &mut out[v as usize];
                o[0] += en[0];
                o[1] += en[1];
                o[2] += en[2];
            }
        }
    }
    for i in 0..nv {
        let (n, l) = normalize_len(out[i]);
        out[i] = if l == 0.0 { vnor[i] } else { n };
    }
    out
}

fn shell_v3v3_normalized_to_dist(a: V3, b: V3) -> f32 {
    let c = dot(a, b).abs();
    if c < 1.0e-8f32 {
        1.0
    } else {
        1.0f32 / c
    }
}

fn reversed_copy(f: &[u32], nv: u32) -> Vec<u32> {
    // keep the first vertex, reverse the rest
    let k = f.len();
    let mut out = vec![f[0] + nv];
    for j in (1..k).rev() {
        out.push(f[j] + nv);
    }
    out
}
fn reversed_cedges(c: &[u32], ne: u32) -> Vec<u32> {
    // [e0, e_{k-1}, ..., e_1] rotated left by one => [e_{k-1}, ..., e_1, e_0]
    let k = c.len();
    let mut out: Vec<u32> = (1..k).rev().map(|j| c[j] + ne).collect();
    out.push(c[0] + ne);
    out
}

/// SOLIDIFY(thickness, offset=-1) with rim, no even thickness, no quality normals.
pub fn solidify_simple(m: &SMesh, thickness: f32) -> SMesh {
    let (nv, ne, nf) = (m.pos.len(), m.edges.len(), m.faces.len());
    let fnor = face_normals(m);
    let vnor = vert_normals(m, &fnor);
    const UNUSED: u32 = u32::MAX;
    const PAIR: u32 = u32::MAX - 1;
    let mut edge_users = vec![UNUSED; ne];
    for (i, f) in m.faces.iter().enumerate() {
        let k = f.len();
        let mut prev = k - 1;
        for j in 0..k {
            let (vert, prev_vert) = (f[j], f[prev]);
            let eidx = m.cedges[i][prev] as usize;
            if edge_users[eidx] == UNUSED {
                let e = m.edges[eidx];
                edge_users[eidx] = if (prev_vert > vert) == (e[0] < e[1]) { i as u32 } else { (i + nf) as u32 };
            } else {
                edge_users[eidx] = PAIR;
            }
            prev = j;
        }
    }
    let mut new_edge_arr = vec![];
    let mut tag = vec![false; nv];
    for e in 0..ne {
        if edge_users[e] != UNUSED && edge_users[e] != PAIR {
            tag[m.edges[e][0] as usize] = true;
            tag[m.edges[e][1] as usize] = true;
            new_edge_arr.push(e);
        }
    }
    let mut old_vert_arr = vec![UNUSED; nv];
    let mut new_vert_arr = vec![];
    for i in 0..nv {
        if tag[i] {
            old_vert_arr[i] = new_vert_arr.len() as u32;
            new_vert_arr.push(i as u32);
        }
    }
    let ofs_orig = -(((-(-1.0f32) + 1.0f32) * 0.5f32) * thickness);
    let ofs_new = thickness + ofs_orig;
    assert!(ofs_new == 0.0);
    let mut pos = m.pos.clone();
    for i in 0..nv {
        let p = m.pos[i];
        let n = vnor[i];
        pos.push([p[0] + n[0] * ofs_orig, p[1] + n[1] * ofs_orig, p[2] + n[2] * ofs_orig]);
    }
    let mut edges = m.edges.clone();
    for e in &m.edges {
        edges.push([e[0] + nv as u32, e[1] + nv as u32]);
    }
    for &v in &new_vert_arr {
        edges.push([v, v + nv as u32]);
    }
    let mut faces = m.faces.clone();
    let mut cedges = m.cedges.clone();
    for i in 0..nf {
        faces.push(reversed_copy(&m.faces[i], nv as u32));
        cedges.push(reversed_cedges(&m.cedges[i], ne as u32));
    }
    for &eidx in &new_edge_arr {
        let pidx = edge_users[eidx];
        let (pidx, flip) = if pidx as usize >= nf { (pidx as usize - nf, true) } else { (pidx as usize, false) };
        let _ = pidx;
        let e = m.edges[eidx];
        let (e0, e1) = (e[0], e[1]);
        let off = (2 * ne) as u32;
        if !flip {
            faces.push(vec![e0, e1, e1 + nv as u32, e0 + nv as u32]);
            cedges.push(vec![eidx as u32, off + old_vert_arr[e1 as usize], eidx as u32 + ne as u32, off + old_vert_arr[e0 as usize]]);
        } else {
            faces.push(vec![e1, e0, e0 + nv as u32, e1 + nv as u32]);
            cedges.push(vec![eidx as u32, off + old_vert_arr[e0 as usize], eidx as u32 + ne as u32, off + old_vert_arr[e1 as usize]]);
        }
    }
    SMesh { pos, edges, faces, cedges }
}

/// SOLIDIFY(thickness, offset=-1, use_even_offset, use_quality_normals) on a closed mesh (no rim faces are generated).
pub fn solidify_even_hq(m: &SMesh, thickness: f32) -> SMesh {
    let (nv, ne, nf) = (m.pos.len(), m.edges.len(), m.faces.len());
    let fnor = face_normals(m);
    let vnor0 = vert_normals(m, &fnor);
    let vnor = hq_normals(m, &fnor, &vnor0);
    let mut vert_angles = vec![0.0f32; nv];
    let mut vert_accum = vec![0.0f32; nv];
    for (i, f) in m.faces.iter().enumerate() {
        let k = f.len();
        let mut i_curr = k - 1;
        let mut i_next = 0;
        let mut nor_prev = normalize_v3(sub(m.pos[f[i_curr - 1] as usize], m.pos[f[i_curr] as usize]));
        while i_next < k {
            let nor_next = normalize_v3(sub(m.pos[f[i_curr] as usize], m.pos[f[i_next] as usize]));
            let mut angle = angle_normalized_v3v3(nor_prev, nor_next);
            if angle < f32::EPSILON {
                angle = f32::EPSILON;
            }
            let vidx = f[i_curr] as usize;
            vert_accum[vidx] += angle;
            vert_angles[vidx] += shell_v3v3_normalized_to_dist(vnor[vidx], fnor[i]) * angle;
            nor_prev = nor_next;
            i_curr = i_next;
            i_next += 1;
        }
    }
    let ofs_orig = -(((-(-1.0f32) + 1.0f32) * 0.5f32) * thickness);
    let ofs_new = thickness + ofs_orig;
    assert!(ofs_new == 0.0);
    let mut pos = m.pos.clone();
    for i in 0..nv {
        let mut p = m.pos[i];
        if vert_accum[i] != 0.0 {
            let f = ofs_orig * (vert_angles[i] / vert_accum[i]);
            let n = vnor[i];
            p = [p[0] + n[0] * f, p[1] + n[1] * f, p[2] + n[2] * f];
        }
        pos.push(p);
    }
    let mut edges = m.edges.clone();
    for e in &m.edges {
        edges.push([e[0] + nv as u32, e[1] + nv as u32]);
    }
    let mut faces = m.faces.clone();
    let mut cedges = m.cedges.clone();
    for i in 0..nf {
        faces.push(reversed_copy(&m.faces[i], nv as u32));
        cedges.push(reversed_cedges(&m.cedges[i], ne as u32));
    }
    SMesh { pos, edges, faces, cedges }
}
