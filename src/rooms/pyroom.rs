//! Exact port of the python solver room mesh pipeline (RoomSolidifier.make_room): polygon2obj (bpy.ops.mesh.fill = bmesh
//! triangle_fill + beautify), SOLIDIFY (extrude mode), tag, SOLIDIFY (even offset, quality normals), and the solver's
//! `quads_convert_to_tris(BEAUTY, BEAUTY)`.  Vertex / face order and float32 positions follow Blender 4.2.
use crate::bm::{scanfill, BMesh, V3};

#[derive(Clone, Debug, Default)]
pub struct RawMesh {
    pub pos: Vec<V3>,
    /// edges as (v1, v2)
    pub edges: Vec<[u32; 2]>,
    pub faces: Vec<Vec<u32>>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len_sq(a: V3) -> f32 {
    a[0] * a[0] + a[1] * a[1] + a[2] * a[2]
}

/// `rings`: exterior ring first (shapely orient: CCW), then holes (CW), without the closing point.
/// Equivalent to `polygon2obj(p, reversed=True, dissolve=False)` (the polygon is assumed to be oriented already).
pub fn polygon2obj(rings: &[Vec<[f64; 2]>]) -> RawMesh {
    let mut bm = BMesh::new();
    let mut pos: Vec<V3> = vec![];
    let mut edges: Vec<[usize; 2]> = vec![];
    for r in rings {
        let base = pos.len();
        let n = r.len();
        for p in r {
            pos.push([p[0] as f32, p[1] as f32, 0.0]);
        }
        // primitive_circle_add edges: (i+1, i) ..., closing (0, n-1)
        for i in 0..n - 1 {
            edges.push([base + i + 1, base + i]);
        }
        edges.push([base, base + n - 1]);
    }
    for (i, p) in pos.iter().enumerate() {
        let v = bm.vert_create(*p, None);
        bm.verts[v].idx = i as i32;
    }
    for e in &edges {
        bm.edge_create(e[0], e[1], None, false);
    }
    // --- bmo_triangle_fill_exec: normal from vertex/edge pairs
    let normal = fill_normal(&bm, &pos, &edges);
    let tris = scanfill::triangle_fill(&pos, &edges, normal);
    let marked: Vec<usize> = bm.edge_ids();
    let mut new_faces = vec![];
    for t in &tris {
        let f = bm.face_create_verts(&[t[0], t[1], t[2]], None, true);
        new_faces.push(f);
    }
    // beautify_fill faces=new edges=!marked
    let cand: Vec<usize> = bm.edge_ids().into_iter().filter(|e| !marked.contains(e)).collect();
    bm.beautify_fill(&cand);
    let (pos2, edges2, mut faces) = bm.to_mesh();
    // point_normal_up(reversed=True): faces looking up are flipped (loop order reversed keeping the first vertex)
    for f in faces.iter_mut() {
        let (a, b, c) = (pos2[f[0] as usize], pos2[f[1] as usize], pos2[f[2] as usize]);
        let nz = cross(sub(b, a), sub(c, a))[2];
        if nz > 0.0 {
            let first = f[0];
            let mut rest: Vec<u32> = f[1..].to_vec();
            rest.reverse();
            f.clear();
            f.push(first);
            f.extend(rest);
        }
    }
    RawMesh { pos: pos2, edges: edges2, faces }
}

fn fill_normal(bm: &BMesh, pos: &[V3], edges: &[[usize; 2]]) -> V3 {
    // vertices in order of first use
    let mut order: Vec<usize> = vec![];
    let mut seen = vec![false; pos.len()];
    for e in edges {
        for &v in e {
            if !seen[v] {
                seen[v] = true;
                order.push(v);
            }
        }
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SortNormal(f32, V3);
    let mut nors: Vec<SortNormal> = vec![];
    let mut is_degenerate = true;
    for &v in &order {
        let mut value = -1.0f32;
        let mut no = [0.0f32; 3];
        let mut pair = vec![];
        let mut count_ok = true;
        for e in bm.edges_of_vert(v) {
            if pair.len() == 2 {
                count_ok = false;
                break;
            }
            pair.push(e);
        }
        if count_ok && pair.len() == 2 {
            is_degenerate = false;
            let a = sub(pos[v], pos[bm.edge_other_vert(pair[0], v)]);
            let b = sub(pos[v], pos[bm.edge_other_vert(pair[1], v)]);
            no = cross(a, b);
            value = len_sq(no);
            if len_sq(a) > len_sq(b) {
                no = [-no[0], -no[1], -no[2]];
            }
        }
        nors.push(SortNormal(value, no));
    }
    if std::env::var_os("DBGN").is_some() {
        eprintln!("raw {:?}", nors.iter().map(|n| (n.0.to_bits(), if n.1[2] > 0.0 { 1 } else if n.1[2] < 0.0 { -1 } else { 0 })).collect::<Vec<_>>());
    }
    let mut normal = [0.0f32; 3];
    if !is_degenerate {
        // qsort descending by value
        crate::bm::qsort::qsort_f32_reverse(&mut nors);
        normal = nors[0].1;
        for n in nors.iter_mut() {
            if n.0 == -1.0 {
                break;
            }
            if normal[0] * n.1[0] + normal[1] * n.1[1] + normal[2] * n.1[2] < 0.0 {
                n.1 = [-n.1[0], -n.1[1], -n.1[2]];
            }
            normal = [normal[0] + n.1[0], normal[1] + n.1[1], normal[2] + n.1[2]];
        }
    }
    if std::env::var_os("DBGN").is_some() {
        eprintln!("normal {:?} nors {:?}", normal, nors.iter().map(|n| (n.0, n.1[2])).collect::<Vec<_>>());
    }
    if let Ok(sg) = std::env::var("FORCEN") {
        let want: f32 = sg.parse().unwrap();
        if normal[2] * want < 0.0 {
            normal = [-normal[0], -normal[1], -normal[2]];
        }
    }
    let d = len_sq(normal);
    if d > 1.0e-35f32 {
        let f = 1.0f32 / d.sqrt();
        normal = [normal[0] * f, normal[1] * f, normal[2] * f];
    } else {
        normal = [0.0, 0.0, 1.0];
    }
    normal
}
