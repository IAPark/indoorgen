//! Wall material alternatives of decorate.room_walls: "half" (bisect at a z threshold, upper part gets the alternative
//! material) and "aligned" (faces touching an edge on the line of the longest horizontal edge).
use crate::mesh::*;

/// bpy.ops.mesh.bisect(plane_no=(0,0,1)) on a mesh of convex polygons: every face crossing z = `z` is cut in two.
pub fn bisect_z(m: &Mesh, z: f64) -> Mesh {
    let mut out = Mesh::new();
    for f in &m.faces {
        let pts: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
        let d: Vec<f64> = pts.iter().map(|p| p[2] - z).collect();
        if !(d.iter().any(|&x| x > 1e-9) && d.iter().any(|&x| x < -1e-9)) {
            out.add_face(&pts);
            continue;
        }
        let (mut lo, mut hi): (Vec<V3>, Vec<V3>) = (vec![], vec![]);
        for k in 0..pts.len() {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            let (da, db) = (d[k], d[(k + 1) % pts.len()]);
            if da <= 1e-9 { lo.push(a) }
            if da >= -1e-9 { hi.push(a) }
            if (da > 1e-9 && db < -1e-9) || (da < -1e-9 && db > 1e-9) {
                let t = da / (da - db);
                let c = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z];
                lo.push(c);
                hi.push(c);
            }
        }
        for poly in [lo, hi] {
            if poly.len() >= 3 {
                out.add_face(&poly);
            }
        }
    }
    out
}

pub fn face_center_z(m: &Mesh, f: &[u32]) -> f64 {
    f.iter().map(|&i| m.pos[i as usize][2]).sum::<f64>() / f.len() as f64
}

/// `alternative` mask of the "aligned" branch: edges collinear (in xy) with the longest horizontal edge; a face is
/// selected when any of its edges is aligned.  Mirrors the python expressions (un-normalised cross products, 1e-4).
pub fn aligned_faces(m: &Mesh) -> Vec<bool> {
    let mut edges: Vec<(V3, V3)> = vec![];
    for f in &m.faces {
        for k in 0..f.len() {
            edges.push((m.pos[f[k] as usize], m.pos[f[(k + 1) % f.len()] as usize]));
        }
    }
    let mut best = (f64::NEG_INFINITY, 0usize);
    for (i, (a, b)) in edges.iter().enumerate() {
        let len = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let s = len - if (a[2] - b[2]).abs() > 0.1 { 100.0 } else { 0.0 };
        if s > best.0 + 1e-12 {
            best = (s, i);
        }
    }
    if edges.is_empty() {
        return vec![];
    }
    let (u_, v_) = edges[best.1];
    let dir = [u_[0] - v_[0], u_[1] - v_[1]];
    let cr = |a: [f64; 2], b: [f64; 2]| (a[0] * b[1] - a[1] * b[0]).abs();
    let aligned: Vec<bool> = edges
        .iter()
        .map(|(u, v)| {
            let d = [u[0] - v[0], u[1] - v[1]];
            (d[0] * d[0] + d[1] * d[1]).sqrt() > 1e-2
                && cr(d, dir) < 1e-4
                && cr([u[0] - v_[0], u[1] - v_[1]], dir) < 1e-4
                && cr([u[0] - u_[0], u[1] - u_[1]], dir) < 1e-4
        })
        .collect();
    let mut k = 0;
    m.faces
        .iter()
        .map(|f| {
            let any = (0..f.len()).any(|j| aligned[k + j]);
            k += f.len();
            any
        })
        .collect()
}

/// split faces by mask into (mask == false, mask == true)
pub fn split_by_mask(m: &Mesh, mask: &[bool]) -> (Mesh, Mesh) {
    let (mut a, mut b) = (Mesh::new(), Mesh::new());
    for (f, &sel) in m.faces.iter().zip(mask) {
        let pts: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
        if sel { b.add_face(&pts) } else { a.add_face(&pts) }
    }
    (a, b)
}

/// Apply a wall alternative: returns (primary part, alternative part, possibly bisected full mesh is consumed)
pub fn apply_half(m: &Mesh, wall_height: f64, frac: f64) -> (Mesh, Mesh) {
    let minz = m.pos.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let z = minz + wall_height * frac;
    let cut = bisect_z(m, z);
    let mask: Vec<bool> = cut.faces.iter().map(|f| face_center_z(&cut, f) > z).collect();
    split_by_mask(&cut, &mask)
}

pub fn apply_aligned(m: &Mesh) -> (Mesh, Mesh) {
    let mask = aligned_faces(m);
    split_by_mask(m, &mask)
}
