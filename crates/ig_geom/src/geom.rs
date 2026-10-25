//! Object geometry for the solver: tagged triangles (planes, projections, centroids) + convex collision proxies.
use super::tags::*;
use crate::mesh::{cross, dot, sub, Mat4, V3};
use crate::shp;
use nalgebra as na;
use parry3d_f64::shape::SharedShape;

pub const VISIBLE: u32 = 1;
pub const SUPPORT: u32 = 2;
pub const INTERIOR: u32 = 4;
pub const BOTTOM: u32 = 8;
pub const TOP: u32 = 16;
pub const SIDE: u32 = 32;
pub const BACK: u32 = 64;
pub const FRONT: u32 = 128;
pub const CEILING: u32 = 256;
pub const WALL: u32 = 512;

pub fn subpart_bit(name: &str) -> u32 {
    match name {
        "Visible" => VISIBLE,
        "SupportSurface" => SUPPORT,
        "Interior" => INTERIOR,
        "Bottom" => BOTTOM,
        "Top" => TOP,
        "Side" => SIDE,
        "Back" => BACK,
        "Front" => FRONT,
        "Ceiling" => CEILING,
        "Wall" => WALL,
        _ => 0,
    }
}

/// (positive mask, negative mask) for a tag set of Subpart tags
pub fn tag_masks(tags: &TagSet) -> (u32, u32) {
    let (mut p, mut n) = (0, 0);
    for t in &tags.0 {
        if t.kind() != Kind::Sub {
            continue;
        }
        let b = subpart_bit(&t.name());
        if t.neg {
            n |= b;
        } else {
            p |= b;
        }
    }
    (p, n)
}

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub v: [V3; 3],
    pub tags: u32,
}

#[derive(Clone, Debug)]
pub enum Proxy {
    /// axis aligned in local space
    Box { center: V3, half: V3 },
    Hull(Vec<V3>),
}

#[derive(Clone, Debug, Default)]
pub struct ObjGeom {
    pub tris: Vec<Tri>,
    pub proxies: Vec<(Proxy, u32)>, // proxy + which face-tag classes it represents (for tag-restricted queries)
    pub lo: V3,
    pub hi: V3,
    /// unique vertices when the mesh is watertight and convex (trimesh: `mesh.is_convex` -> fcl.Convex), else None (BVH)
    pub convex: Option<std::sync::Arc<Vec<V3>>>,
}

impl ObjGeom {
    /// the blender vertex array order (fcl's Convex support search breaks ties by vertex index)
    pub fn with_mesh_verts(mut self, verts: Vec<V3>) -> ObjGeom {
        if let Some(c) = &self.convex {
            if c.len() == verts.len() && c.iter().all(|p| verts.contains(p)) {
                self.convex = Some(std::sync::Arc::new(verts));
            }
        }
        self
    }
}

/// unique vertices (first-seen order) if the triangle soup is a closed convex mesh
pub fn convex_vertices(tris: &[Tri]) -> Option<Vec<V3>> {
    if tris.is_empty() {
        return None;
    }
    let mut verts: Vec<V3> = vec![];
    let mut idx = |p: V3, verts: &mut Vec<V3>| -> usize {
        if let Some(i) = verts.iter().position(|q| *q == p) {
            i
        } else {
            verts.push(p);
            verts.len() - 1
        }
    };
    let mut faces: Vec<[usize; 3]> = vec![];
    for t in tris {
        let f = [idx(t.v[0], &mut verts), idx(t.v[1], &mut verts), idx(t.v[2], &mut verts)];
        faces.push(f);
    }
    // watertight: every directed edge has exactly one opposite twin
    let mut edges: crate::dhash::HashMap<(usize, usize), i32> = crate::dhash::HashMap::default();
    for f in &faces {
        for k in 0..3 {
            *edges.entry((f[k], f[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    for (&(a, b), &n) in &edges {
        if n != 1 || edges.get(&(b, a)) != Some(&1) {
            return None;
        }
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in &verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let scale = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
    let tol = 1e-9 * scale;
    for t in tris {
        let n = cross(sub(t.v[1], t.v[0]), sub(t.v[2], t.v[0]));
        let l = dot(n, n).sqrt();
        if l < 1e-12 {
            continue;
        }
        let n = [n[0] / l, n[1] / l, n[2] / l];
        for v in &verts {
            if dot(n, sub(*v, t.v[0])) > tol {
                return None;
            }
        }
    }
    Some(verts)
}

impl ObjGeom {
    pub fn from_tris(tris: Vec<Tri>, proxies: Vec<(Proxy, u32)>) -> ObjGeom {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in &tris {
            for v in &t.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let convex = convex_vertices(&tris).map(std::sync::Arc::new);
        ObjGeom { tris, proxies, lo, hi, convex }
    }

    pub fn dims(&self) -> V3 {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
    }

    /// An axis aligned box [lo,hi] as 12 triangles with canonical tags (back=min x, front=max x, bottom=min z, top=max z)
    pub fn cuboid(lo: V3, hi: V3, extra: u32) -> ObjGeom {
        let mut tris = box_tris(lo, hi, 0);
        canonical_tag(&mut tris, lo, hi);
        for t in tris.iter_mut() {
            t.tags |= extra;
        }
        ObjGeom::from_tris(
            tris,
            vec![(Proxy::Box { center: [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.], half: [(hi[0] - lo[0]) / 2., (hi[1] - lo[1]) / 2., (hi[2] - lo[2]) / 2.] }, u32::MAX)],
        )
    }
}

/// CCW (outward) triangles of an axis-aligned box
pub fn box_tris(lo: V3, hi: V3, tags: u32) -> Vec<Tri> {
    let c = |x: usize, y: usize, z: usize| [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }];
    let quads = [
        [c(0, 0, 0), c(0, 1, 0), c(0, 1, 1), c(0, 0, 1)], // -x
        [c(1, 0, 0), c(1, 0, 1), c(1, 1, 1), c(1, 1, 0)], // +x
        [c(0, 0, 0), c(0, 0, 1), c(1, 0, 1), c(1, 0, 0)], // -y
        [c(0, 1, 0), c(1, 1, 0), c(1, 1, 1), c(0, 1, 1)], // +y
        [c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0)], // -z
        [c(0, 0, 1), c(0, 1, 1), c(1, 1, 1), c(1, 0, 1)], // +z
    ];
    let mut out = vec![];
    for q in quads {
        // quads above are listed clockwise seen from outside; flip to CCW (outward normals)
        out.push(Tri { v: [q[0], q[2], q[1]], tags });
        out.push(Tri { v: [q[0], q[3], q[2]], tags });
    }
    out
}

/// tag_canonical_surfaces: faces whose vertices all lie on the min/max extreme along x (Back/Front) and z (Bottom/Top)
pub fn canonical_tag(tris: &mut [Tri], lo: V3, hi: V3) {
    let dims = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    for t in tris.iter_mut() {
        let on = |axis: usize, val: f64| t.v.iter().all(|v| (v[axis] - val).abs() <= 0.01 * dims[axis] + 1e-8);
        if on(0, lo[0]) {
            t.tags |= BACK;
        }
        if on(0, hi[0]) {
            t.tags |= FRONT;
        }
        if on(2, hi[2]) {
            t.tags |= TOP;
        }
        if on(2, lo[2]) {
            t.tags |= BOTTOM;
        }
    }
}

/// Object pose: rotation matrix + location (Blender "location"/"rotation" of the object origin)
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub r: [[f64; 3]; 3],
    pub t: V3,
    /// authoritative float32 blender transform (location / euler / scale); r and t are its matrix_world widened to f64
    pub o: crate::bl32::Obj32,
}

impl Pose {
    pub fn from_o32(o: crate::bl32::Obj32) -> Pose {
        let w = o.matrix_world();
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = w[j][i] as f64;
            }
        }
        Pose { r, t: [w[3][0] as f64, w[3][1] as f64, w[3][2] as f64], o }
    }
    pub fn identity() -> Pose {
        Pose::from_o32(crate::bl32::Obj32::new([0.0; 3], [0.0; 3]))
    }
    pub fn from_euler(loc: V3, e: V3) -> Pose {
        Pose::from_o32(crate::bl32::Obj32::new([loc[0] as f32, loc[1] as f32, loc[2] as f32], [e[0] as f32, e[1] as f32, e[2] as f32]))
    }
    /// `obj.matrix_world = M` (row-major 4x4)
    pub fn from_matrix(m: [[f64; 4]; 4]) -> Pose {
        let mut o = crate::bl32::Obj32::new([0.0; 3], [0.0; 3]);
        let mut w = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                w[c][r] = m[r][c] as f32;
            }
        }
        o.set_matrix_world(&w);
        Pose::from_o32(o)
    }
    /// obj.location = v
    pub fn set_loc(&mut self, v: V3) {
        self.o.loc = [v[0] as f32, v[1] as f32, v[2] as f32];
        *self = Pose::from_o32(self.o);
    }
    /// obj.location += Vector(v)
    pub fn translate(&mut self, v: V3) {
        self.o.translate(v);
        *self = Pose::from_o32(self.o);
    }
    /// obj.rotation_euler = e (location and scale stay)
    pub fn set_euler(&mut self, e: V3) {
        self.o.eul = [e[0] as f32, e[1] as f32, e[2] as f32];
        *self = Pose::from_o32(self.o);
    }
    pub fn loc32(&self) -> [f32; 3] {
        self.o.loc
    }
    pub fn apply(&self, p: V3) -> V3 {
        let r = &self.r;
        [
            r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2] + self.t[0],
            r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2] + self.t[1],
            r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2] + self.t[2],
        ]
    }
    pub fn rot(&self, v: V3) -> V3 {
        let r = &self.r;
        [
            r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
            r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
            r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2],
        ]
    }
    /// iu.rotate: pre-multiply rotation by axis-angle about the object origin (float32 blender pipeline)
    pub fn rotate_axis(&mut self, axis: V3, angle: f64) {
        self.o.rotate(axis, angle);
        *self = Pose::from_o32(self.o);
    }
    pub fn isometry(&self) -> na::Isometry3<f64> {
        let m = na::Matrix3::new(self.r[0][0], self.r[0][1], self.r[0][2], self.r[1][0], self.r[1][1], self.r[1][2], self.r[2][0], self.r[2][1], self.r[2][2]);
        let rot = na::UnitQuaternion::from_rotation_matrix(&na::Rotation3::from_matrix_unchecked(m));
        na::Isometry3::from_parts(na::Translation3::new(self.t[0], self.t[1], self.t[2]), rot)
    }
}

pub fn norm(v: V3) -> f64 {
    dot(v, v).sqrt()
}
pub fn unit(v: V3) -> V3 {
    let n = norm(v);
    if n > 0.0 {
        [v[0] / n, v[1] / n, v[2] / n]
    } else {
        v
    }
}
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// triangle in world space
pub fn world_tri(p: &Pose, t: &Tri) -> [V3; 3] {
    [p.apply(t.v[0]), p.apply(t.v[1]), p.apply(t.v[2])]
}

pub fn tri_normal(w: &[V3; 3]) -> V3 {
    unit(cross(sub(w[1], w[0]), sub(w[2], w[0])))
}

pub fn tri_area(w: &[V3; 3]) -> f64 {
    0.5 * norm(cross(sub(w[1], w[0]), sub(w[2], w[0])))
}

/// Blender polygon normal of a triangle in float32 (normal_tri_v3: cross(v1-v2, v2-v3) then normalize_v3)
pub fn normal_f32(w: &[V3; 3]) -> [f32; 3] {
    let f = |p: V3| [p[0] as f32, p[1] as f32, p[2] as f32];
    let (a, b, c) = (f(w[0]), f(w[1]), f(w[2]));
    let n1 = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let n2 = [b[0] - c[0], b[1] - c[1], b[2] - c[2]];
    let n = [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]];
    let d = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    if d > 1e-35 {
        let inv = 1.0f32 / d.sqrt();
        [n[0] * inv, n[1] * inv, n[2] * inv]
    } else {
        [0.0; 3]
    }
}

fn round_half_even(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 {
        (2.0 * (x / 2.0).round()) as i64
    } else {
        r as i64
    }
}

fn f32v(p: V3) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// planes.hash_plane(global_polygon_normal, global_vertex_coordinates, tol) with blender's float32 vectors
fn plane_hash(pose: &Pose, t: &Tri, tol: f64) -> Option<([i64; 3], i64)> {
    use crate::bl32::*;
    let m = pose.o.matrix_world();
    let ln = poly_normal(f32v(t.v[0]), f32v(t.v[1]), f32v(t.v[2]));
    let gn = global_polygon_normal(&m, ln, false);
    let p = mat4_mul_point(&m, f32v(t.v[0]));
    let l = ((gn[0] as f64).powi(2) + (gn[1] as f64).powi(2) + (gn[2] as f64).powi(2)).sqrt();
    if l < 1e-6 {
        return None;
    }
    let nn = vdiv(gn, l);
    let dist = (nn[0] as f64) * (p[0] as f64) + (nn[1] as f64) * (p[1] as f64) + (nn[2] as f64) * (p[2] as f64);
    let q = vdiv(nn, tol);
    Some(([round_half_even(q[0] as f64), round_half_even(q[1] as f64), round_half_even(q[2] as f64)], round_half_even(dist / tol)))
}

/// Unique planes among faces selected by `mask`; returns the triangle index representing each plane.
pub fn unique_planes(g: &ObjGeom, pose: &Pose, pos: u32, neg: u32) -> Vec<usize> {
    let mut seen: Vec<([i64; 3], i64)> = vec![];
    let mut out = vec![];
    for (i, t) in g.tris.iter().enumerate() {
        if t.tags & pos != pos || t.tags & neg != 0 {
            continue;
        }
        let Some(h) = plane_hash(pose, t, 1e-4) else { continue };
        if !seen.contains(&h) {
            seen.push(h);
            out.push(i);
        }
    }
    out
}

/// face indices coplanar with triangle `ti` among those matching the masks (tolerance 1e-2 like the reference)
pub fn plane_mask(g: &ObjGeom, pose: &Pose, pos: u32, neg: u32, ti: usize) -> Vec<usize> {
    use crate::bl32;
    let rw = world_tri(pose, &g.tris[ti]);
    let rn = tri_normal(&rw);
    let w = pose.o.matrix_world();
    let (_, quat, _) = bl32::decompose(&w);
    let rot = bl32::quat_to_mat3(quat);
    let f32v3 = |p: V3| [p[0] as f32, p[1] as f32, p[2] as f32];
    // python (_compute_tagged_plane_mask): float32 mathutils vertices / normals, double numpy dots
    let gnormal = |t: &Tri| -> [f32; 3] {
        let ln = bl32::poly_normal(f32v3(t.v[0]), f32v3(t.v[1]), f32v3(t.v[2]));
        let mut n = [0.0f32; 3];
        for row in 0..3 {
            let mut d = 0.0f64;
            d += (rot[0][row] * ln[0]) as f64;
            d += (rot[1][row] * ln[1]) as f64;
            d += (rot[2][row] * ln[2]) as f64;
            n[row] = d as f32;
        }
        let nn = (n[0] as f64 * n[0] as f64 + n[1] as f64 * n[1] as f64 + n[2] as f64 * n[2] as f64).sqrt();
        let inv = 1.0f32 / (nn as f32);
        [n[0] * inv, n[1] * inv, n[2] * inv]
    };
    let gvert = |t: &Tri| bl32::mat4_mul_point(&w, f32v3(t.v[0]));
    let ref_n = gnormal(&g.tris[ti]);
    let ref_v = gvert(&g.tris[ti]);
    let d3 = |a: [f32; 3], b: [f32; 3]| a[0] as f64 * b[0] as f64 + a[1] as f64 * b[1] as f64 + a[2] as f64 * b[2] as f64;
    let mut out = vec![];
    for (i, t) in g.tris.iter().enumerate() {
        if t.tags & pos != pos || t.tags & neg != 0 {
            continue;
        }
        // cheap double prefilter with a wide margin
        let w2 = world_tri(pose, t);
        let n2 = tri_normal(&w2);
        if (dot(rn, n2) - 1.0).abs() > 0.05 {
            continue;
        }
        let cn = gnormal(t);
        let cv = gvert(t);
        let mut diff = [ref_v[0] - cv[0], ref_v[1] - cv[1], ref_v[2] - cv[2]];
        let dn = d3(diff, diff).sqrt();
        if !(dn.abs() <= 1e-8) {
            let inv = 1.0f32 / (dn as f32);
            diff = [diff[0] * inv, diff[1] * inv, diff[2] * inv];
        }
        let ndot = d3(ref_n, cn);
        let pdot = d3(diff, cn);
        if (ndot - 1.0).abs() <= 1e-2 + 1e-5 && pdot.abs() <= 1e-2 {
            out.push(i);
        }
    }
    out
}

/// 2D frame (u, v) of trimesh.geometry.plane_transform: the rotation R = align_vectors(n, +z) maps world -> plane coords,
/// so u = R^T e_x and v = R^T e_y (rows 0 and 1 of R). The exact frame matters because python's `is_vertically_contained` is frame-dependent.
pub fn plane_frame(n: V3) -> (V3, V3) {
    let n = unit(n);
    let z = [0.0, 0.0, 1.0];
    let c = dot(n, z).clamp(-1.0, 1.0);
    let axis = cross(n, z);
    let s = norm(axis);
    let r: [[f64; 3]; 3] = if s < 1e-12 {
        if c > 0.0 {
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        } else {
            // antiparallel: rotation by pi about x
            [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]
        }
    } else {
        let k = [axis[0] / s, axis[1] / s, axis[2] / s];
        let th = c.acos();
        let (sn, cs) = (th.sin(), th.cos());
        let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let kk = k[i] * k[j];
                m[i][j] = (if i == j { cs } else { 0.0 }) + sn * kx[i][j] + (1.0 - cs) * kk;
            }
        }
        m
    };
    ([r[0][0], r[0][1], r[0][2]], [r[1][0], r[1][1], r[1][2]])
}

/// trimesh.path.polygons.projected: union of the (front side) triangles projected onto the plane (origin, normal)
pub fn projected(tris: &[[V3; 3]], normal: V3, origin: V3) -> Option<shp::G> {
    // memo on the exact input bits: the solver projects the same (static parent / unchanged child) triangles again and again
    thread_local! {
        static MEMO: std::cell::RefCell<(std::collections::HashMap<u64, Vec<(Vec<u64>, Option<shp::G>)>>, usize)> = std::cell::RefCell::new((Default::default(), 0));
    }
    let mut key: Vec<u64> = Vec::with_capacity(6 + tris.len() * 9);
    for k in 0..3 {
        key.push(normal[k].to_bits());
    }
    for k in 0..3 {
        key.push(origin[k].to_bits());
    }
    for t in tris {
        for p in t {
            for k in 0..3 {
                key.push(p[k].to_bits());
            }
        }
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for w in &key {
        h = (h ^ w).wrapping_mul(0x100000001b3);
        h ^= h >> 29;
    }
    if let Some(r) = MEMO.with(|m| m.borrow().0.get(&h).and_then(|b| b.iter().find(|e| e.0 == key).map(|e| e.1.clone()))) {
        return r;
    }
    let r = projected_raw(tris, normal, origin);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.1 > 2500 {
            m.0.clear();
            m.1 = 0;
        }
        m.1 += 1;
        m.0.entry(h).or_default().push((key, r.clone()));
    });
    r
}

fn projected_raw(tris: &[[V3; 3]], normal: V3, origin: V3) -> Option<shp::G> {
    let n = unit(normal);
    let (u, v) = plane_frame(n);
    let mut front = vec![];
    let mut back = vec![];
    for w in tris {
        let d = dot(tri_normal(w), n);
        if d > 1e-10 {
            front.push(w);
        } else if d < -1e-10 {
            back.push(w);
        }
    }
    let side = if front.is_empty() {
        back
    } else if back.is_empty() {
        front
    } else if front.len() <= back.len() {
        // trimesh picks the side with fewer faces
        front
    } else {
        back
    };
    let mut polys = vec![];
    let mut scale_ = 0.0f64;
    for w in side {
        let pts: Vec<[f64; 2]> = w.iter().map(|p| {
            let r = sub(*p, origin);
            [dot(r, u), dot(r, v)]
        }).collect();
        for p in &pts {
            scale_ = scale_.max(p[0].abs()).max(p[1].abs());
        }
        let a = 0.5 * ((pts[1][0] - pts[0][0]) * (pts[2][1] - pts[0][1]) - (pts[2][0] - pts[0][0]) * (pts[1][1] - pts[0][1]));
        if a.abs() < 1e-14 {
            continue;
        }
        if let Ok(p) = shp::polygon(&pts, &[]) {
            polys.push(p);
        }
    }
    if polys.is_empty() {
        return None;
    }
    let u_ = shp::union_all(&polys);
    let pad = 1e-5 * (2.0 * scale_);
    Some(shp::buffer(&shp::buffer(&u_, pad), -pad))
}

pub fn parry_shape(p: &Proxy) -> SharedShape {
    match p {
        Proxy::Box { half, .. } => SharedShape::cuboid(half[0].max(1e-6), half[1].max(1e-6), half[2].max(1e-6)),
        Proxy::Hull(pts) => {
            let pts: Vec<na::Point3<f64>> = pts.iter().map(|p| na::Point3::new(p[0], p[1], p[2])).collect();
            SharedShape::convex_hull(&pts).expect("hull")
        }
    }
}

pub fn proxy_pose(p: &Proxy, pose: &Pose) -> na::Isometry3<f64> {
    match p {
        Proxy::Box { center, .. } => {
            let c = pose.apply(*center);
            let mut q = *pose;
            q.t = c;
            q.isometry()
        }
        Proxy::Hull(_) => pose.isometry(),
    }
}

pub fn hull_ok(pts: &[V3]) -> bool {
    let p: Vec<na::Point3<f64>> = pts.iter().map(|p| na::Point3::new(p[0], p[1], p[2])).collect();
    SharedShape::convex_hull(&p).is_some()
}

/// true when the triangle normal points up (+z component positive)
pub fn tri_nz(v: &[V3; 3]) -> bool {
    let a = [v[1][0] - v[0][0], v[1][1] - v[0][1]];
    let b = [v[2][0] - v[0][0], v[2][1] - v[0][1]];
    a[0] * b[1] - a[1] * b[0] > 0.0
}

/// python placeholder boxes: exact vertex / triangle order of the reference meshes.
/// variant 0: created placeholders (vertex i = 4x+2y+z), variant 1: bounding box of the asset (vertex i = x+2y+4z)
pub fn cuboid_py(lo: V3, hi: V3, variant: u8, extra: u32) -> ObjGeom {
    let vert = |i: usize| -> V3 {
        let (x, y, z) = if variant == 0 { ((i >> 2) & 1, (i >> 1) & 1, i & 1) } else { (i & 1, (i >> 1) & 1, (i >> 2) & 1) };
        [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }]
    };
    let verts: [V3; 8] = std::array::from_fn(vert);
    cuboid_py_verts(verts, variant, extra)
}

/// like cuboid_py with the 8 vertex positions given explicitly (python vertex order of `variant`)
pub fn cuboid_py_verts(verts: [V3; 8], variant: u8, extra: u32) -> ObjGeom {
    let vert = |i: usize| -> V3 { verts[i] };
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in &verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let (polys, names): ([[usize; 3]; 12], [&str; 12]) = if variant == 0 {
        (
            [[1, 2, 0], [3, 6, 2], [7, 4, 6], [5, 0, 4], [6, 0, 2], [3, 5, 7], [1, 3, 2], [3, 7, 6], [7, 5, 4], [5, 1, 0], [6, 4, 0], [3, 1, 5]],
            ["back", "", "front", "", "bottom", "top", "back", "", "front", "", "bottom", "top"],
        )
    } else {
        (
            [[2, 1, 0], [1, 4, 0], [5, 6, 4], [6, 3, 2], [4, 2, 0], [3, 5, 1], [2, 3, 1], [1, 5, 4], [5, 7, 6], [6, 7, 3], [4, 6, 2], [3, 7, 5]],
            ["bottom", "", "top", "", "back", "front", "bottom", "", "top", "", "back", "front"],
        )
    };
    let tris: Vec<Tri> = polys
        .iter()
        .zip(names.iter())
        .map(|(p, n)| Tri {
            v: [vert(p[0]), vert(p[1]), vert(p[2])],
            tags: extra
                | match *n {
                    "back" => BACK,
                    "front" => FRONT,
                    "bottom" => BOTTOM,
                    "top" => TOP,
                    _ => 0,
                },
        })
        .collect();
    let verts: Vec<V3> = (0..8).map(vert).collect();
    ObjGeom::from_tris(
        tris,
        vec![(Proxy::Box { center: [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.], half: [(hi[0] - lo[0]) / 2., (hi[1] - lo[1]) / 2., (hi[2] - lo[2]) / 2.] }, u32::MAX)],
    )
    .with_mesh_verts(verts)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlaceholderKind {
    Real,
    AssetAsPlaceholder,
    BboxOfPlaceholder,
    BboxOfAsset,
}

/// Source of placeholder geometry (the asset factories).
pub trait Assets {
    fn placeholder(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom>;
    /// real asset geometry for solver purposes (shelves populated before small objects are placed)
    fn solver_asset(&self, _class: &str, _factory_seed: u64, _inst_seed: u64) -> Option<ObjGeom> {
        None
    }
}

