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

// ------------------------------------------------------------------------------------------------------------------
pub const T_INTERIOR: u8 = 1;
pub const T_CEILING: u8 = 2;
pub const T_SUPPORT: u8 = 4;
pub const T_WALL: u8 = 8;
pub const T_VISIBLE: u8 = 16;

/// `bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")` on a tri / quad mesh (every face selected).
/// Returns the triangles in Blender's face order and the index of the source face of every triangle.
pub fn quads_to_tris(pos: &[V3], faces: &[Vec<u32>]) -> (Vec<[u32; 3]>, Vec<usize>) {
    use crate::assets::polyfill::{bm_rotate_beauty_area, is_quad_flip_v3};
    let mut bm = BMesh::new();
    for p in pos {
        bm.vert_create(*p, None);
    }
    for (i, f) in faces.iter().enumerate() {
        let vs: Vec<usize> = f.iter().map(|&v| v as usize).collect();
        let id = bm.face_create_verts(&vs, None, true);
        bm.faces[id].user = i;
        assert_eq!(id, i);
    }
    let n0 = bm.faces.len();
    for f in 0..n0 {
        if !bm.faces[f].alive || bm.faces[f].len < 4 {
            continue;
        }
        assert_eq!(bm.faces[f].len, 4, "ngon triangulation not needed for rooms");
        let ls = bm.loops_of_face(f);
        let v: Vec<usize> = ls.iter().map(|&l| bm.loops[l].v).collect();
        let co = |i: usize| bm.verts[v[i]].co;
        // l_v1 = first->next, l_v2 = first->next->next, l_v3 = first->prev, l_v4 = first
        let (c1, c2, c3, c4) = (co(1), co(2), co(3), co(0));
        let flip = is_quad_flip_v3(c1, c2, c3, c4);
        let split_24 = if flip & 1 != 0 {
            true
        } else if flip & 2 != 0 {
            false
        } else {
            bm_rotate_beauty_area(c1, c2, c3, c4, false) > 0.0
        };
        let tris: [[usize; 3]; 2] = if split_24 { [[v[0], v[1], v[2]], [v[0], v[2], v[3]]] } else { [[v[1], v[2], v[3]], [v[1], v[3], v[0]]] };
        let mut last = usize::MAX;
        for t in &tris {
            last = bm.face_create_verts(t, Some(f), true);
        }
        bm.face_swap_data(f, last);
        bm.face_kill(last);
    }
    let mut out = vec![];
    let mut src = vec![];
    // vertex ids are 0..n (no vertex was killed)
    for f in bm.face_ids() {
        let vs = bm.verts_of_face(f);
        assert_eq!(vs.len(), 3);
        out.push([vs[0] as u32, vs[1] as u32, vs[2] as u32]);
        src.push(bm.faces[f].user);
    }
    (out, src)
}

pub struct RoomOut {
    pub pos: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub tags: Vec<u8>,
    /// raw stage meshes (for debugging / stage comparison)
    pub stages: Vec<(&'static str, super::pysolid::SMesh)>,
}

/// RoomSolidifier.make_room + the solver's triangulation.  `rings`: oriented polygon rings (exterior first), `ext`: exterior
/// edge centres of this room (empty => everything interior).
pub fn make_room(rings: &[Vec<[f64; 2]>], wall_thickness: f64, wall_height: f64, ext: &[[f64; 2]]) -> RoomOut {
    use super::pysolid::*;
    let raw = polygon2obj(rings);
    let m0 = SMesh::from_raw(&raw);
    let s1 = solidify_simple(&m0, wall_height as f32);
    // tag() on the prism faces
    let (wt, h) = (wall_thickness, wall_height);
    let (eps, snap) = (1e-5f64, 0.5f64);
    let high = h - wt / 2.0;
    let low = wt / 2.0;
    let mut tags1: Vec<u8> = vec![];
    for f in &s1.faces {
        let k = f.len() as f32;
        let mut c = [0.0f32; 3];
        for &v in f {
            let p = s1.pos[v as usize];
            c[0] += p[0];
            c[1] += p[1];
            c[2] += p[2];
        }
        let c = [c[0] / k, c[1] / k, c[2] / k];
        let z = c[2] as f64;
        let ceiling = z > high - eps || (z - high + snap).abs() < eps;
        let floor = z < low + eps || (z - low - snap).abs() < eps;
        let wall = !(ceiling || floor);
        let exterior = ext.iter().any(|e| (c[0] as f64 - e[0]).abs() + (c[1] as f64 - e[1]).abs() < wt * 4.0);
        let mut t = 0u8;
        if !exterior {
            t |= T_INTERIOR;
        }
        if ceiling {
            t |= T_CEILING;
        }
        if floor {
            t |= T_SUPPORT;
        }
        if wall {
            t |= T_WALL;
        }
        tags1.push(t);
    }
    let s2 = solidify_even_hq(&s1, (wt / 2.0) as f32);
    let nf1 = s1.faces.len();
    let mut tags2: Vec<u8> = tags1.clone();
    tags2.extend(tags1.iter().map(|t| t | T_VISIBLE));
    let (tris, src) = quads_to_tris(&s2.pos, &s2.faces);
    let tags = src.iter().map(|&i| tags2[i]).collect();
    let _ = nf1;
    RoomOut { pos: s2.pos.clone(), tris, tags, stages: vec![("fill", m0.clone()), ("sol1", s1), ("sol2", s2)] }
}
