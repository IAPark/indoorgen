//! SOLIDIFY modifier, simple "EXTRUDE" mode (MOD_solidify_extrude.cc) in float32 with Blender's exact vertex / edge / face / corner order:
//! no even thickness, no clamp, no vertex groups, optional rim. Also `mesh_vert_normals` (Mesh::vert_normals, safe_acos_approx weights).
use crate::bm::query as q;
use crate::bm::NONE;
type V3 = [f32; 3];

/// bke::mesh face normal (tri / quad / Newell) for a face given by vertex indices
pub fn face_normal_v(pos: &[V3], f: &[u32]) -> V3 {
    // Mesh::face_normals = normals_calc_faces: Newell for every face size, zero -> +Z
    let n = f.len();
    let mut nrm = [0.0f32; 3];
    let mut prev = pos[f[n - 1] as usize];
    for i in 0..n {
        let cur = pos[f[i] as usize];
        nrm[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
        nrm[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
        nrm[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
        prev = cur;
    }
    let r = q::normalize(nrm);
    if q::is_zero(r) {
        [0.0, 0.0, 1.0]
    } else {
        r
    }
}

/// blender::math::normalize (true division, zero below 1e-35)
fn normalize_div(a: V3) -> V3 {
    let d = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0; 3]
    }
}

/// Mesh::vert_normals(): angle weighted (safe_acos_approx) face normals; loose vertices get the normalized position
pub fn mesh_vert_normals(pos: &[V3], faces: &[Vec<u32>]) -> Vec<V3> {
    let fnor: Vec<V3> = faces.iter().map(|f| face_normal_v(pos, f)).collect();
    let mut acc = vec![[0.0f32; 3]; pos.len()];
    for (fi, f) in faces.iter().enumerate() {
        let k = f.len();
        for i in 0..k {
            let v = f[i] as usize;
            let p = pos[v];
            let d = |o: V3| normalize_div(q::sub(o, p));
            let (dp, dn) = (d(pos[f[(i + k - 1) % k] as usize]), d(pos[f[(i + 1) % k] as usize]));
            let fac = super::bevmod::safe_acos_approx(q::dot(dp, dn));
            for c in 0..3 {
                acc[v][c] += fnor[fi][c] * fac;
            }
        }
    }
    (0..pos.len()).map(|v| if q::is_zero(acc[v]) { normalize_div(pos[v]) } else { normalize_div(acc[v]) }).collect()
}

pub struct Solid {
    pub pos: Vec<V3>,
    pub edges: Vec<[u32; 2]>,
    pub faces: Vec<Vec<u32>>,
    pub corner_edges: Vec<Vec<u32>>,
}

/// `offset_fac` is the modifier's `offset` (-1..1), `thickness` its thickness. `corner_edges[f][k]` = edge index of the edge from corner k to k+1.
pub fn solidify_extrude(pos: &[V3], edges: &[[u32; 2]], faces: &[Vec<u32>], corner_edges: &[Vec<u32>], thickness: f32, offset_fac: f32, rim: bool) -> Solid {
    let (nv, ne, nf) = (pos.len(), edges.len(), faces.len());
    let ofs_orig = -(((-offset_fac + 1.0) * 0.5) * thickness);
    let ofs_new = thickness + ofs_orig;
    // rim bookkeeping
    const INVALID_UNUSED: usize = usize::MAX;
    const INVALID_PAIR: usize = usize::MAX - 1;
    let mut edge_users = vec![INVALID_UNUSED; ne];
    let mut edge_order = vec![0usize; ne];
    let mut new_edge_arr: Vec<usize> = vec![];
    let mut old_vert_arr = vec![INVALID_UNUSED; nv];
    let mut new_vert_arr: Vec<usize> = vec![];
    if rim {
        for (i, f) in faces.iter().enumerate() {
            let k = f.len();
            let mut prev = k - 1;
            for j in 0..k {
                let (vert_i, prev_vert_i) = (f[j], f[prev]);
                let eidx = corner_edges[i][prev] as usize;
                if edge_users[eidx] == INVALID_UNUSED {
                    let e = edges[eidx];
                    edge_users[eidx] = if (prev_vert_i > vert_i) == (e[0] < e[1]) { i } else { i + nf };
                    edge_order[eidx] = j;
                } else {
                    edge_users[eidx] = INVALID_PAIR;
                }
                prev = j;
            }
        }
        let mut tag = vec![false; nv];
        for eidx in 0..ne {
            if edge_users[eidx] != INVALID_UNUSED && edge_users[eidx] != INVALID_PAIR {
                tag[edges[eidx][0] as usize] = true;
                tag[edges[eidx][1] as usize] = true;
                new_edge_arr.push(eidx);
            }
        }
        for i in 0..nv {
            if tag[i] {
                old_vert_arr[i] = new_vert_arr.len();
                new_vert_arr.push(i);
            }
        }
    }
    let rim_verts = new_vert_arr.len();
    let new_polys = new_edge_arr.len();
    let vnor = mesh_vert_normals(pos, faces);
    // vertices: copy 0 and copy 1, then the offsets (do_flip = false)
    let mut out_pos: Vec<V3> = pos.iter().chain(pos.iter()).cloned().collect();
    let madd = |p: &mut V3, n: V3, f: f32| {
        p[0] += n[0] * f;
        p[1] += n[1] * f;
        p[2] += n[2] * f;
    };
    if ofs_new != 0.0 {
        // INIT_VERT_ARRAY_OFFSETS(test = false): ((ofs_new >= ofs_orig) == do_flip) == test
        let first = ((ofs_new >= ofs_orig) == false) == false;
        for i in 0..nv {
            madd(&mut out_pos[base_idx(first, nv) + i], vnor[i], ofs_new);
        }
    }
    if ofs_orig != 0.0 {
        let first = ((ofs_new >= ofs_orig) == false) == true;
        for i in 0..nv {
            madd(&mut out_pos[base_idx(first, nv) + i], vnor[i], ofs_orig);
        }
    }
    // edges
    let mut out_edges: Vec<[u32; 2]> = edges.to_vec();
    out_edges.extend(edges.iter().map(|e| [e[0] + nv as u32, e[1] + nv as u32]));
    for i in 0..rim_verts {
        out_edges.push([new_vert_arr[i] as u32, new_vert_arr[i] as u32 + nv as u32]);
    }
    // faces
    let mut out_faces: Vec<Vec<u32>> = faces.to_vec();
    let mut out_ce: Vec<Vec<u32>> = corner_edges.to_vec();
    for (i, f) in faces.iter().enumerate() {
        let k = f.len();
        let loop_end = k - 1;
        let mut cv = vec![0u32; k];
        let mut ce = vec![0u32; k];
        let mut j_prev = loop_end;
        for j in 0..k {
            cv[loop_end - j_prev] = f[j];
            ce[loop_end - j_prev] = corner_edges[i][j];
            j_prev = j;
        }
        let e0 = ce[0];
        for j in 0..loop_end {
            ce[j] = ce[j + 1];
        }
        ce[loop_end] = e0;
        out_faces.push(cv.iter().map(|&v| v + nv as u32).collect());
        out_ce.push(ce.iter().map(|&e| e + ne as u32).collect());
    }
    for i in 0..new_polys {
        let eidx = new_edge_arr[i];
        let mut pidx = edge_users[eidx];
        let flip = if pidx >= nf {
            pidx -= nf;
            true
        } else {
            false
        };
        let _ = pidx;
        let e = edges[eidx];
        let rim_e = |v: u32| (ne * 2 + old_vert_arr[v as usize]) as u32;
        let (cv, ce);
        if !flip {
            cv = vec![e[0], e[1], e[1] + nv as u32, e[0] + nv as u32];
            ce = vec![eidx as u32, rim_e(e[1]), (eidx + ne) as u32, rim_e(e[0])];
        } else {
            cv = vec![e[1], e[0], e[0] + nv as u32, e[1] + nv as u32];
            ce = vec![eidx as u32, rim_e(e[0]), (eidx + ne) as u32, rim_e(e[1])];
        }
        out_faces.push(cv);
        out_ce.push(ce);
    }
    let _ = (edge_order, NONE);
    Solid { pos: out_pos, edges: out_edges, faces: out_faces, corner_edges: out_ce }
}

fn base_idx(first: bool, nv: usize) -> usize {
    if first {
        0
    } else {
        nv
    }
}
