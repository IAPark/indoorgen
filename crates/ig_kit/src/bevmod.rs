//! Blender BEVEL modifier (edges, limit method ANGLE or NONE, offset type width, superellipse profile 0.5, clamp overlap, loop slide) on a
//! polygon mesh, via the bit-exact bmesh bevel port (src/bm). Edge / vertex order of the result follows Blender's for a Blender-ordered input.
use crate::bm::bevel::{bm_mesh_bevel, BevelOpts, OffsetType};
use crate::bm::query::dot;
use crate::bm::*;
use crate::mesh::Mesh;
use crate::dhash::HashMap;

/// `angle`: Some(radians) for limit_method ANGLE (default 30 degrees), None for NONE (all manifold edges)
pub fn bevel_modifier(m: &Mesh, width: f64, segments: i32, angle: Option<f32>, clamp: bool, profile: f32) -> Result<Mesh, String> {
    bevel_modifier_ex(m, width, segments, angle, clamp, profile, None)
}

/// like `bevel_modifier`; `edge_order`: the Blender mesh's edge list (vertex pairs in edge index order, e.g. `mesh.edges`), used instead of the
/// first-use order of the face corners (matters for the vertex/face order of the result)
pub fn bevel_modifier_ex(m: &Mesh, width: f64, segments: i32, angle: Option<f32>, clamp: bool, profile: f32, edge_order: Option<&[[u32; 2]]>) -> Result<Mesh, String> {
    bevel_modifier_edges(m, width, segments, angle, clamp, profile, edge_order).map(|r| r.0)
}

/// like `bevel_modifier_ex`, also returning the result's edge list in Blender's (BMesh) order with the stored vertex order (v1, v2)
pub fn bevel_modifier_edges(m: &Mesh, width: f64, segments: i32, angle: Option<f32>, clamp: bool, profile: f32, edge_order: Option<&[[u32; 2]]>) -> Result<(Mesh, Vec<[u32; 2]>), String> {
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    // edges in first-use order of the face corners (BKE mesh edge calculation order)
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    if let Some(eo) = edge_order {
        for e in eo {
            emap.insert((e[0].min(e[1]), e[0].max(e[1])), edges.len() as u32);
            edges.push(*e);
        }
    }
    let mut ce: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([k.0, k.1]);
                (edges.len() - 1) as u32
            });
            fe.push(id);
        }
        ce.push(fe);
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    set_mesh_vert_normals(&mut bm, &pos, &m.faces);
    for v in bm.vert_ids() {
        bm.verts[v].flag &= !ELEM_TAG;
    }
    for e in bm.edge_ids() {
        bm.edges[e].flag &= !ELEM_TAG;
    }
    let threshold = angle.map(|a| (a + 0.000000175f32).cos());
    for e in bm.edge_ids() {
        match threshold {
            Some(th) => {
                if let Some((la, lb)) = bm.edge_loop_pair(e) {
                    let (fa, fb) = (bm.loops[la].f, bm.loops[lb].f);
                    if dot(bm.faces[fa].no, bm.faces[fb].no) < th {
                        bm.edges[e].flag |= ELEM_TAG;
                        let (a, b) = (bm.edges[e].v1, bm.edges[e].v2);
                        bm.verts[a].flag |= ELEM_TAG;
                        bm.verts[b].flag |= ELEM_TAG;
                    }
                }
            }
            None => {
                if bm.edge_is_manifold(e) {
                    bm.edges[e].flag |= ELEM_TAG;
                    let (a, b) = (bm.edges[e].v1, bm.edges[e].v2);
                    bm.verts[a].flag |= ELEM_TAG;
                    bm.verts[b].flag |= ELEM_TAG;
                }
            }
        }
    }
    let o = BevelOpts { offset: width as f32, offset_type: OffsetType::Offset, segments, profile, limit_offset: clamp, loop_slide: true, mat_nr: -1 };
    bm_mesh_bevel(&mut bm, &o)?;
    let (p, e, f) = bm.to_mesh();
    let mut out = Mesh::new();
    out.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    out.faces = f;
    Ok((out, e))
}

/// blender::math::safe_acos_approx with Blender's exact f32 constants (the previous abbreviated literals were different floats; SOLIDIFY of a
/// garbled hull is bit-exact only with these).
pub fn safe_acos_approx(x: f32) -> f32 {
    let f = x.abs();
    let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
    let p = 1.5707963267f32 + m * (-0.213300989f32 + m * (0.077980478f32 + m * -0.02164095f32));
    let a = (1.0 - m).sqrt() * p;
    if x < 0.0 {
        std::f32::consts::PI - a
    } else {
        a
    }
}

/// `BKE_mesh_to_bmesh_ex(calc_vert_normal)`: BMVert.no = Mesh::vert_normals() (angle weighted (safe_acos_approx) sum of the Newell
/// face normals), not BMesh's own vertex normals. Matters for the vertex-normal sign tests of the bevel (folded geometry).
pub fn set_mesh_vert_normals(bm: &mut BMesh, pos: &[[f32; 3]], faces: &[Vec<u32>]) {
    let norm = |v: [f32; 3]| -> [f32; 3] {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.0 {
            [v[0] / l, v[1] / l, v[2] / l]
        } else {
            [0.0; 3]
        }
    };
    let mut fnor: Vec<[f32; 3]> = vec![];
    for f in faces {
        let mut n = [0.0f32; 3];
        let mut prev = pos[*f.last().unwrap() as usize];
        for &v in f {
            let cur = pos[v as usize];
            n[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
            n[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
            n[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
            prev = cur;
        }
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        fnor.push(if l == 0.0 { [0.0, 0.0, 1.0] } else { [n[0] / l, n[1] / l, n[2] / l] });
    }
    let mut acc = vec![[0.0f32; 3]; pos.len()];
    let mut has = vec![false; pos.len()];
    for (fi, f) in faces.iter().enumerate() {
        let k = f.len();
        for i in 0..k {
            let v = f[i] as usize;
            let p = pos[v];
            let d = |q: [f32; 3]| norm([q[0] - p[0], q[1] - p[1], q[2] - p[2]]);
            let (dp, dn) = (d(pos[f[(i + k - 1) % k] as usize]), d(pos[f[(i + 1) % k] as usize]));
            let fac = safe_acos_approx(dp[0] * dn[0] + dp[1] * dn[1] + dp[2] * dn[2]);
            for c in 0..3 {
                acc[v][c] += fnor[fi][c] * fac;
            }
            has[v] = true;
        }
    }
    for v in 0..pos.len() {
        bm.verts[v].no = if has[v] { norm(acc[v]) } else { norm(pos[v]) };
    }
}

/// `bpy.ops.mesh.bevel(affect=EDGES, offset_type=WIDTH|OFFSET, ...)` on the edges whose two face normals differ by more than `sharp`
/// radians (`edges_select_sharp`). Operator defaults: clamp_overlap False, loop_slide True, profile 0.5.
pub fn bevel_sharp_op(m: &Mesh, sharp: f32, width: f64, segments: i32, width_mode: bool) -> Result<Mesh, String> {
    use crate::bm::bevel::{bevel_edges, BevelOpts, OffsetType};
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut ce: Vec<Vec<u32>> = vec![];
    let mut efaces: Vec<Vec<usize>> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([k.0, k.1]);
                efaces.push(vec![]);
                (edges.len() - 1) as u32
            });
            efaces[id as usize].push(fi);
            fe.push(id);
        }
        ce.push(fe);
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    set_mesh_vert_normals(&mut bm, &pos, &m.faces);
    let mut sel = vec![];
    for (ei, fs) in efaces.iter().enumerate() {
        if fs.len() == 2 {
            let a = bm.faces[fs[0]].no;
            let b = bm.faces[fs[1]].no;
            if crate::bm::query::angle_normalized_v3v3(a, b).abs() > sharp {
                sel.push(ei);
            }
        }
    }
    let o = BevelOpts { offset: width as f32, offset_type: if width_mode { OffsetType::Width } else { OffsetType::Offset }, segments, profile: 0.5, limit_offset: false, loop_slide: true, mat_nr: -1 };
    bevel_edges(&mut bm, &sel, &o)?;
    let (p, _e, f) = bm.to_mesh();
    let mut out = Mesh::new();
    out.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    out.faces = f;
    Ok(out)
}
