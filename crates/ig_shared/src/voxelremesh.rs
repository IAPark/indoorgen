//! REMESH modifier VOXEL mode (BKE_mesh_remesh_voxel: OpenVDB meshToLevelSet(halfWidth 1) + volumeToMesh(isovalue 0, adaptivity 0)),
//! reimplemented with an exact point-triangle distance field (the narrow band of a level set holds the true distance, so the surface-nets vertices
//! land where OpenVDB's edge interpolation puts them) and signs from angle-weighted pseudo-normals (closed meshes). One vertex per mixed cube
//! (OpenVDB splits the ambiguous cube configurations into several points; those are not reproduced), quads over the sign-changing grid edges.
//! Vertex / face ORDER is not OpenVDB's. `remesh_with_attrs` additionally snaps the result onto the source surface (transfer_all moves the
//! position attribute), see leafkit::sample_nearest_surface.
use super::leafkit::{closest_on_tri_f32, F3};
use crate::dhash::HashMap;

type D3 = [f64; 3];

fn sub(a: D3, b: D3) -> D3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: D3, b: D3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: D3, b: D3) -> D3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: D3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: D3) -> D3 {
    let n = norm(a);
    if n > 0.0 {
        [a[0] / n, a[1] / n, a[2] / n]
    } else {
        a
    }
}

/// closest point on triangle with the feature it lies on: 0..2 vertex, 3..5 edge (ab, ac, bc), 6 face
fn closest(p: D3, a: D3, b: D3, c: D3) -> (D3, u8) {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, 0);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, 1);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return ([a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v], 3);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, 2);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return ([a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w], 4);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return ([b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w], 5);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    ([a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w], 6)
}

pub struct SurfaceNets {
    pub v: Vec<F3>,
    pub f: Vec<Vec<u32>>,
}

/// BKE_mesh_remesh_voxel(mesh, voxel_size, adaptivity 0, isovalue 0) on the corner triangles of a mesh
pub fn voxel_remesh(pos: &[F3], tris: &[[u32; 3]], voxel: f32) -> SurfaceNets {
    let voxel = voxel * crate::leafkit::voxel_scale() as f32;
    if std::env::var("NOVDB").is_err() {
        if let Some((v, f)) = crate::vdb::remesh_voxel(pos, tris, voxel) {
            return SurfaceNets { v, f };
        }
    }
    voxel_remesh_nets(pos, tris, voxel)
}

/// pure-Rust approximation (used when OpenVDB is not linked)
pub fn voxel_remesh_nets(pos: &[F3], tris: &[[u32; 3]], voxel: f32) -> SurfaceNets {
    let vs = voxel as f64;
    let p: Vec<D3> = pos.iter().map(|q| [q[0] as f64 / vs, q[1] as f64 / vs, q[2] as f64 / vs]).collect();
    // pseudo normals
    let nt = tris.len();
    let mut fnorm: Vec<D3> = Vec::with_capacity(nt);
    for t in tris {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        fnorm.push(unit(cross(sub(b, a), sub(c, a))));
    }
    let mut vnorm = vec![[0.0f64; 3]; p.len()];
    let mut enorm: HashMap<(u32, u32), D3> = HashMap::default();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (i0, i1, i2) = (t[k] as usize, t[(k + 1) % 3] as usize, t[(k + 2) % 3] as usize);
            let e1 = unit(sub(p[i1], p[i0]));
            let e2 = unit(sub(p[i2], p[i0]));
            let ang = dot(e1, e2).clamp(-1.0, 1.0).acos();
            for c in 0..3 {
                vnorm[i0][c] += fnorm[ti][c] * ang;
            }
            let key = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
            let e = enorm.entry(key).or_insert([0.0; 3]);
            for c in 0..3 {
                e[c] += fnorm[ti][c];
            }
        }
    }
    // distance field in index space within R voxels of the surface
    const R: f64 = 2.0;
    let mut field: HashMap<[i32; 3], (f64, f64)> = HashMap::default(); // (dist^2, signed pseudo-normal dot)
    for (ti, t) in tris.iter().enumerate() {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for q in [a, b, c] {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let (l, h) = ([0, 1, 2].map(|k| (lo[k] - R).ceil() as i32), [0, 1, 2].map(|k| (hi[k] + R).floor() as i32));
        for x in l[0]..=h[0] {
            for y in l[1]..=h[1] {
                for z in l[2]..=h[2] {
                    let q = [x as f64, y as f64, z as f64];
                    let (cp, feat) = closest(q, a, b, c);
                    let d = sub(q, cp);
                    let d2 = dot(d, d);
                    if d2 > R * R {
                        continue;
                    }
                    let e = field.entry([x, y, z]).or_insert((f64::MAX, 0.0));
                    if d2 < e.0 - 1e-12 {
                        let n = match feat {
                            6 => fnorm[ti],
                            0 => vnorm[t[0] as usize],
                            1 => vnorm[t[1] as usize],
                            2 => vnorm[t[2] as usize],
                            3 => enorm[&(t[0].min(t[1]), t[0].max(t[1]))],
                            4 => enorm[&(t[0].min(t[2]), t[0].max(t[2]))],
                            _ => enorm[&(t[1].min(t[2]), t[1].max(t[2]))],
                        };
                        *e = (d2, dot(d, n));
                    }
                }
            }
        }
    }
    // signed values clamped to the narrow band (+-1 voxel); missing corners are outside the band
    let value = |k: [i32; 3]| -> Option<f64> {
        field.get(&k).map(|&(d2, s)| {
            let d = d2.sqrt().min(1.0);
            if s > 0.0 { d } else if s < 0.0 { -d } else if d2 == 0.0 { 0.0 } else { d }
        })
    };
    // corner signs of cubes: unknown corners take the sign of any known corner's side only if all known agree (they are > 2 voxels away: cube is not mixed)
    let mut cube_vertex: HashMap<[i32; 3], u32> = HashMap::default();
    let mut verts: Vec<F3> = vec![];
    let mut keys: Vec<[i32; 3]> = field.keys().copied().collect();
    keys.sort();
    const CORNER: [[i32; 3]; 8] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
    const EDGES: [[usize; 2]; 12] = [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];
    let mut cubes_seen: HashMap<[i32; 3], ()> = HashMap::default();
    for k in &keys {
        // cubes whose lower corner is k - offset: visit the 8 cubes touching voxel k
        for o in CORNER {
            let base = [k[0] - o[0], k[1] - o[1], k[2] - o[2]];
            if cubes_seen.insert(base, ()).is_some() {
                continue;
            }
            let mut vals = [0f64; 8];
            let mut ok = true;
            for (ci, c) in CORNER.iter().enumerate() {
                match value([base[0] + c[0], base[1] + c[1], base[2] + c[2]]) {
                    Some(v) => vals[ci] = v,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let inside: Vec<bool> = vals.iter().map(|&v| v < 0.0).collect();
            if inside.iter().all(|&b| b) || inside.iter().all(|&b| !b) {
                continue;
            }
            let mut acc = [0f64; 3];
            let mut cnt = 0;
            for e in EDGES {
                let (a, b) = (e[0], e[1]);
                if inside[a] != inside[b] {
                    let t = vals[a] / (vals[a] - vals[b]);
                    for c in 0..3 {
                        acc[c] += CORNER[a][c] as f64 + t * (CORNER[b][c] - CORNER[a][c]) as f64;
                    }
                    cnt += 1;
                }
            }
            let q = [0, 1, 2].map(|c| ((base[c] as f64 + acc[c] / cnt as f64) * vs) as f32);
            cube_vertex.insert(base, 0);
            verts.push(q);
            let last = verts.len() as u32 - 1;
            cube_vertex.insert(base, last);
        }
    }
    // quads over sign-changing grid edges: edge from voxel k along axis a; the four cubes around it
    let mut faces: Vec<Vec<u32>> = vec![];
    let sign_in = |k: [i32; 3]| value(k).map(|v| v < 0.0);
    let mut ek: Vec<[i32; 3]> = cube_vertex.keys().copied().collect();
    ek.sort();
    let _ = ek;
    let mut vox: Vec<[i32; 3]> = keys.clone();
    vox.sort();
    for k in vox {
        let Some(s0) = sign_in(k) else { continue };
        for axis in 0..3 {
            let mut k1 = k;
            k1[axis] += 1;
            let Some(s1) = sign_in(k1) else { continue };
            if s0 == s1 {
                continue;
            }
            // the four cubes containing this edge, in cyclic order around the axis
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut ids = [0u32; 4];
            let mut good = true;
            for (i, (du, dw)) in [(-1, -1), (0, -1), (0, 0), (-1, 0)].iter().enumerate() {
                let mut b = k;
                b[u] += du;
                b[w] += dw;
                match cube_vertex.get(&b) {
                    Some(&id) => ids[i] = id,
                    None => {
                        good = false;
                        break;
                    }
                }
            }
            if !good {
                continue;
            }
            // outward orientation: inside -> outside along +axis gives counter-clockwise about +axis when seen from +axis
            if s0 {
                faces.push(vec![ids[0], ids[1], ids[2], ids[3]]);
            } else {
                faces.push(vec![ids[3], ids[2], ids[1], ids[0]]);
            }
        }
    }
    let _ = closest_on_tri_f32;
    SurfaceNets { v: verts, f: faces }
}

use super::leafkit::LMesh;

/// detail.remesh_with_attrs(obj, face_size): voxel remesh, then transfer_all moves every vertex onto the nearest point of the source surface
pub fn remesh_with_attrs(m: &LMesh, face_size: f64) -> LMesh {
    let tris = super::dualcon::corner_tris(&m.v, &m.f);
    let r = voxel_remesh(&m.v, &tris, face_size as f32);
    let mut o = LMesh { v: r.v, f: r.f, e: vec![] };
    let dummy = vec![0f32; m.v.len()];
    let snapped = super::leafkit::sample_nearest_surface(m, &dummy, &o.v);
    for (i, s) in snapped.iter().enumerate() {
        o.v[i] = s.1;
    }
    o.ensure_face_edges();
    o
}
