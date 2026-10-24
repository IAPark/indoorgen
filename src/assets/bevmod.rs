//! Blender BEVEL modifier (edges, limit method ANGLE or NONE, offset type width, superellipse profile 0.5, clamp overlap, loop slide) on a
//! polygon mesh, via the bit-exact bmesh bevel port (src/bm). Edge / vertex order of the result follows Blender's for a Blender-ordered input.
use crate::bm::bevel::{bm_mesh_bevel, BevelOpts, OffsetType};
use crate::bm::query::dot;
use crate::bm::*;
use crate::mesh::Mesh;
use crate::dhash::HashMap;

/// `angle`: Some(radians) for limit_method ANGLE (default 30 degrees), None for NONE (all manifold edges)
pub fn bevel_modifier(m: &Mesh, width: f64, segments: i32, angle: Option<f32>, clamp: bool, profile: f32) -> Result<Mesh, String> {
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    // edges in first-use order of the face corners (BKE mesh edge calculation order)
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges: Vec<[u32; 2]> = vec![];
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
    let (p, _e, f) = bm.to_mesh();
    let mut out = Mesh::new();
    out.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    out.faces = f;
    Ok(out)
}
