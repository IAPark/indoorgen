//! Small ports of edit-mode operators used by bpy-built assets (geometry only; element order is not Blender's):
//! extrude_region_move, separate-by-selection, polygon normals / centres as `read_normal` / `read_center` return them.
use crate::mesh::Mesh;
use crate::dhash::HashMap;

pub type V3 = [f64; 3];

fn f32c(p: V3) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// polygon normals as Blender stores them (float32; quads by diagonal cross product, ngons by Newell)
pub fn face_normals(m: &Mesh) -> Vec<V3> {
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| f32c(*p)).collect();
    m.faces.iter().map(|f| { let n = super::solidnm::face_normal32(&pos, f); [n[0] as f64, n[1] as f64, n[2] as f64] }).collect()
}

/// polygon centres (mean of the corner vertices, float32 like `poly.center`)
pub fn face_centers(m: &Mesh) -> Vec<V3> {
    m.faces
        .iter()
        .map(|f| {
            let mut c = [0.0f32; 3];
            for &v in f {
                let p = f32c(m.pos[v as usize]);
                for k in 0..3 {
                    c[k] += p[k];
                }
            }
            let n = f.len() as f32;
            [(c[0] / n) as f64, (c[1] / n) as f64, (c[2] / n) as f64]
        })
        .collect()
}

/// bpy.ops.mesh.extrude_region_move on a face selection: the selected faces move by `t`, side faces connect them to the rest.
/// Returns the new vertex id of every old vertex that was duplicated.
pub fn extrude_region(m: &mut Mesh, sel: &[bool], t: V3) -> HashMap<u32, u32> {
    let mut map: HashMap<u32, u32> = HashMap::default();
    // edge -> (selected faces count, total faces count)
    let mut ecount: HashMap<(u32, u32), (u32, u32)> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let e = ecount.entry((a.min(b), a.max(b))).or_insert((0, 0));
            e.1 += 1;
            if sel[fi] {
                e.0 += 1;
            }
        }
    }
    let mut sides: Vec<(u32, u32)> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let e = ecount[&(a.min(b), a.max(b))];
            if e.0 == 1 {
                sides.push((a, b));
            }
        }
    }
    let nfaces = m.faces.len();
    for fi in 0..nfaces {
        if !sel[fi] {
            continue;
        }
        for c in 0..m.faces[fi].len() {
            let v = m.faces[fi][c];
            let nv = *map.entry(v).or_insert_with(|| {
                let p = m.pos[v as usize];
                m.pos.push([p[0] + t[0], p[1] + t[1], p[2] + t[2]]);
                (m.pos.len() - 1) as u32
            });
            m.faces[fi][c] = nv;
        }
    }
    for (a, b) in sides {
        m.faces.push(vec![a, b, map[&b], map[&a]]);
    }
    map
}
