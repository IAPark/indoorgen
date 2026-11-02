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

/// bpy.ops.mesh.inset(thickness, depth, use_individual=True) on the faces with `sel[fi]`: every selected face is shrunk in its plane by
/// `thickness` (even offset, mitred corners), raised by `depth` along its normal, and joined to the old boundary by one quad per edge
/// (quad = [a, b, new_b, new_a] in face order, like Blender). The inner face keeps the original face index.
pub fn inset_individual(m: &Mesh, sel: &[bool], thickness: f64, depth: f64) -> Mesh {
    let mut o = m.clone();
    let mut ring: Vec<Vec<u32>> = vec![];
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |a: V3| {
        let l = dot(a, a).sqrt();
        if l > 1e-300 { [a[0] / l, a[1] / l, a[2] / l] } else { a }
    };
    for (fi, f) in m.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        let n = f.len();
        let p: Vec<V3> = f.iter().map(|&v| m.pos[v as usize]).collect();
        let fnrm = unit(super::coil::face_normal(m, f));
        let mut new_ids = vec![];
        for i in 0..n {
            let (pp, pc, pn) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let (e0, e1) = (unit(sub(pc, pp)), unit(sub(pn, pc)));
            // inward normals of the incoming and outgoing edge
            let (n0, n1) = (unit(cross(fnrm, e0)), unit(cross(fnrm, e1)));
            let bis = [n0[0] + n1[0], n0[1] + n1[1], n0[2] + n1[2]];
            // (a miter limit: at a needle-sharp corner the offset ran to metres -- a bed's headboard panel 12 m tall)
            let s = (thickness / (1.0 + dot(n0, n1)).max(1e-9)).min(2.0 * thickness / dot(bis, bis).sqrt().max(1e-9));
            let q = [pc[0] + bis[0] * s + fnrm[0] * depth, pc[1] + bis[1] * s + fnrm[1] * depth, pc[2] + bis[2] * s + fnrm[2] * depth];
            o.pos.push(q);
            new_ids.push((o.pos.len() - 1) as u32);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            ring.push(vec![f[i], f[j], new_ids[j], new_ids[i]]);
        }
        o.faces[fi] = new_ids;
    }
    o.faces.extend(ring);
    o
}
