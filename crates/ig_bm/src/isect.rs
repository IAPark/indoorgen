//! Port of Blender 4.2 `bmesh_intersect.cc` (`BM_mesh_intersect`) as used by the geometry nodes Mesh Boolean FLOAT solver
//! (`use_self=false, use_separate=false, use_dissolve=true, use_island_connect=true, use_partial_connect=false`, boolean mode set).
use super::bvh::{BvhTree, RAYCAST_DIST_MAX};
use super::kern::{FLAG_WALK, FLAG_WALK_ALT};
use super::query::*;
use super::*;
use std::collections::HashMap as StdMap;

pub const BOOLEAN_ISECT: i32 = 0;
pub const BOOLEAN_UNION: i32 = 1;
pub const BOOLEAN_DIFFERENCE: i32 = 2;

struct Epsilon {
    eps: f32,
    eps2x: f32,
    eps_margin: f32,
    eps_sq: f32,
    eps2x_sq: f32,
    eps_margin_sq: f32,
}

// ------------------------------------------------------------------ GHash emulation (iteration order only)
// BLI_ghash.c is built with GHASH_USE_MODULO_BUCKETS: prime bucket counts, `hash % nbuckets`.
const HASH_SIZES: [usize; 27] = [
    5, 11, 17, 37, 67, 131, 257, 521, 1031, 2053, 4099, 8209, 16411, 32771, 65537, 131101, 262147, 524309, 1048583, 2097169, 4194319, 8388617, 16777259, 33554467, 67108879, 134217757, 268435459,
];

pub struct GHash<V> {
    buckets: Vec<Vec<usize>>, // entry indices, chain order (head first)
    keys: Vec<u64>,
    vals: Vec<V>,
    hashfp: fn(u64) -> u32,
    cursize: usize,
    limit_grow: usize,
    nentries: usize,
}

pub fn inthash_p(key: u64) -> u32 {
    let mut key = key;
    key = key.wrapping_add(!(key << 16));
    key ^= key >> 5;
    key = key.wrapping_add(key << 3);
    key ^= key >> 13;
    key = key.wrapping_add(!(key << 9));
    key ^= key >> 17;
    (key & 0xffff_ffff) as u32
}
pub fn ptrhash(key: u64) -> u32 {
    ((key >> 4) as u32) | ((key as u32) << 28)
}

impl<V> GHash<V> {
    pub fn new(hashfp: fn(u64) -> u32) -> GHash<V> {
        GHash { buckets: vec![vec![]; HASH_SIZES[0]], keys: vec![], vals: vec![], hashfp, cursize: 0, limit_grow: (HASH_SIZES[0] * 3) / 4, nentries: 0 }
    }
    fn bucket_index(&self, key: u64) -> usize {
        ((self.hashfp)(key) as usize) % self.buckets.len()
    }
    pub fn find(&self, key: u64) -> Option<usize> {
        let b = self.bucket_index(key);
        self.buckets[b].iter().copied().find(|&i| self.keys[i] == key)
    }
    fn expand(&mut self) {
        if self.nentries < self.limit_grow {
            return;
        }
        let mut new_nbuckets = self.buckets.len();
        while self.nentries > self.limit_grow && self.cursize < HASH_SIZES.len() - 1 {
            self.cursize += 1;
            new_nbuckets = HASH_SIZES[self.cursize];
            self.limit_grow = (new_nbuckets * 3) / 4;
        }
        if new_nbuckets == self.buckets.len() {
            return;
        }
        self.limit_grow = (new_nbuckets * 3) / 4;
        let old = std::mem::replace(&mut self.buckets, vec![vec![]; new_nbuckets]);
        for chain in old {
            for i in chain {
                let b = ((self.hashfp)(self.keys[i]) as usize) % new_nbuckets;
                self.buckets[b].insert(0, i);
            }
        }
    }
    /// BLI_ghash_ensure_p: returns (entry index, was_new)
    pub fn ensure(&mut self, key: u64, make: impl FnOnce() -> V) -> (usize, bool) {
        if let Some(i) = self.find(key) {
            return (i, false);
        }
        let b = self.bucket_index(key);
        let i = self.keys.len();
        self.keys.push(key);
        self.vals.push(make());
        self.buckets[b].insert(0, i);
        self.nentries += 1;
        self.expand();
        (i, true)
    }
    pub fn val_mut(&mut self, i: usize) -> &mut V {
        &mut self.vals[i]
    }
    /// GHASH_ITER order: entry indices
    pub fn iter_order(&self) -> Vec<usize> {
        let mut out = vec![];
        for b in &self.buckets {
            out.extend(b.iter().copied());
        }
        out
    }
    pub fn key(&self, i: usize) -> u64 {
        self.keys[i]
    }
    pub fn val(&self, i: usize) -> &V {
        &self.vals[i]
    }
    pub fn len(&self) -> usize {
        self.nentries
    }
}

fn dbg_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GNBOOL_DUMP").is_some())
}

// ------------------------------------------------------------------ math helpers
fn fma_mode() -> u8 {
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| std::env::var("GNBOOL_FMA").ok().and_then(|s| s.parse().ok()).unwrap_or(0))
}
fn dot(a: V3, b: V3) -> f32 {
    if fma_mode() == 0 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    } else {
        a[2].mul_add(b[2], a[1].mul_add(b[1], a[0] * b[0]))
    }
}
fn cross(a: V3, b: V3) -> V3 {
    match fma_mode() {
        0 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]],
        1 => [a[1].mul_add(b[2], -(a[2] * b[1])), a[2].mul_add(b[0], -(a[0] * b[2])), a[0].mul_add(b[1], -(a[1] * b[0]))],
        _ => [(-a[2]).mul_add(b[1], a[1] * b[2]), (-a[0]).mul_add(b[2], a[2] * b[0]), (-a[1]).mul_add(b[0], a[0] * b[1])],
    }
}
fn line_point_factor_v3(p: V3, l1: V3, l2: V3) -> f32 {
    let u = sub(l2, l1);
    let h = sub(p, l1);
    let d = len_sq(u);
    if d > 0.0 {
        dot(u, h) / d
    } else {
        0.0
    }
}
fn interp_v3(a: V3, b: V3, t: f32) -> V3 {
    let s = 1.0f32 - t;
    [s * a[0] + t * b[0], s * a[1] + t * b[1], s * a[2] + t * b[2]]
}
fn mid3(a: V3, b: V3, c: V3) -> V3 {
    [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0]
}
fn tri_v3_scale(v: &mut [V3; 3], t: f32) {
    let p = mid3(v[0], v[1], v[2]);
    for k in 0..3 {
        v[k] = interp_v3(p, v[k], t);
    }
}
fn normal_tri(v1: V3, v2: V3, v3: V3) -> V3 {
    let n1 = sub(v1, v2);
    let n2 = sub(v2, v3);
    let n = [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]];
    normalize(n)
}
fn cross_tri(v1: V3, v2: V3, v3: V3) -> V3 {
    let n1 = sub(v1, v2);
    let n2 = sub(v2, v3);
    [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]]
}
pub fn area_squared_tri(v1: V3, v2: V3, v3: V3) -> f32 {
    let n = mul(cross_tri(v1, v2, v3), 0.5);
    len_sq(n)
}

/// isect_line_line_epsilon_v3: (kind, i1, i2)
fn isect_line_line_epsilon(v1: V3, v2: V3, v3: V3, v4: V3, epsilon: f32) -> (i32, V3, V3) {
    let c = sub(v3, v1);
    let a = sub(v2, v1);
    let b = sub(v4, v3);
    let ab = cross(a, b);
    let d = dot(c, ab);
    let div = dot(ab, ab);
    if div == 0.0 {
        return (0, [0.0; 3], [0.0; 3]);
    }
    if d.abs() <= epsilon {
        let cb = cross(c, b);
        let a2 = mul(a, dot(cb, ab) / div);
        let i1 = add(v1, a2);
        return (1, i1, i1);
    }
    let n = cross(a, b);
    let t0 = sub(v1, v3);
    let t = project_v3_v3v3(t0, n);
    let v3t = add(v3, t);
    let v4t = add(v4, t);
    let c = sub(v3t, v1);
    let a = sub(v2, v1);
    let b = sub(v4t, v3t);
    let ab = cross(a, b);
    let cb = cross(c, b);
    let a2 = mul(a, dot(cb, ab) / dot(ab, ab));
    let i1 = add(v1, a2);
    let i2 = sub(i1, t);
    (2, i1, i2)
}

/// isect_line_segment_tri_epsilon_v3 -> Some(lambda)
fn isect_line_segment_tri_epsilon(p1: V3, p2: V3, v0: V3, v1: V3, v2: V3, epsilon: f32) -> Option<f32> {
    let e1 = sub(v1, v0);
    let e2 = sub(v2, v0);
    let d = sub(p2, p1);
    let p = cross(d, e2);
    let a = dot(e1, p);
    if a == 0.0 {
        return None;
    }
    let f = 1.0f32 / a;
    let s = sub(p1, v0);
    let u = f * dot(s, p);
    if u < -epsilon || u > 1.0 + epsilon {
        return None;
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < -epsilon || (u + v) > 1.0 + epsilon {
        return None;
    }
    let lambda = f * dot(e2, q);
    if lambda < 0.0 || lambda > 1.0 {
        return None;
    }
    Some(lambda)
}

fn point_in_slice(p: V3, v1: V3, l1: V3, l2: V3) -> bool {
    let (_, cp) = closest_to_line(v1, l1, l2);
    let q = sub(cp, v1);
    let rp = sub(p, v1);
    let h = dot(q, rp) / dot(q, q);
    h >= 0.0 && h <= 1.0
}
/// isect_point_tri_v3 -> Some(closest point)
fn isect_point_tri(p: V3, v1: V3, v2: V3, v3: V3) -> Option<V3> {
    if !point_in_slice(p, v1, v2, v3) || !point_in_slice(p, v2, v3, v1) || !point_in_slice(p, v3, v1, v2) {
        return None;
    }
    let no = cross_tri(v1, v2, v3);
    let plane = plane_from_point_normal(v1, no);
    Some(closest_to_plane(plane, p))
}

/// isect_ray_tri_watertight_v3 with the fixed +X ray precalc {kx 1, ky 2, kz 0, sx 0, sy 0, sz 1}; returns lambda
fn isect_ray_tri_watertight_x(ray_origin: V3, v0: V3, v1: V3, v2: V3) -> Option<f32> {
    let (kx, ky, kz) = (1usize, 2usize, 0usize);
    let (sx, sy, sz) = (0.0f32, 0.0f32, 1.0f32);
    let a = [v0[0] - ray_origin[0], v0[1] - ray_origin[1], v0[2] - ray_origin[2]];
    let b = [v1[0] - ray_origin[0], v1[1] - ray_origin[1], v1[2] - ray_origin[2]];
    let c = [v2[0] - ray_origin[0], v2[1] - ray_origin[1], v2[2] - ray_origin[2]];
    let (a_kx, a_ky, a_kz) = (a[kx], a[ky], a[kz]);
    let (b_kx, b_ky, b_kz) = (b[kx], b[ky], b[kz]);
    let (c_kx, c_ky, c_kz) = (c[kx], c[ky], c[kz]);
    let ax = a_kx - sx * a_kz;
    let ay = a_ky - sy * a_kz;
    let bx = b_kx - sx * b_kz;
    let by = b_ky - sy * b_kz;
    let cx = c_kx - sx * c_kz;
    let cy = c_ky - sy * c_kz;
    let u = cx * by - cy * bx;
    let v = ax * cy - ay * cx;
    let w = bx * ay - by * ax;
    if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
        return None;
    }
    let det = u + v + w;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let sign_det = det.to_bits() & 0x8000_0000;
    let t = (u * a_kz + v * b_kz + w * c_kz) * sz;
    let sign_t = f32::from_bits(t.to_bits() ^ sign_det);
    if sign_t < 0.0 {
        return None;
    }
    let inv_det = 1.0f32 / det;
    Some(t * inv_det)
}

// ------------------------------------------------------------------ state
#[derive(PartialEq, Clone, Copy, Debug)]
enum IsectType {
    None = -1,
    Edge0 = 0,
    Edge1 = 1,
    Edge2 = 2,
    Tri = 3,
}

struct State {
    eps: Epsilon,
    edgetri_cache: StdMap<[i32; 4], usize>,
    edge_verts: GHash<Vec<usize>>,
    face_edges: GHash<Vec<usize>>,
    wire_edges: crate::dhash::HashSet<usize>,
    vert_dissolve: Vec<usize>, // push order (iterate in reverse = list order)
}

fn ghash_insert_link(gh: &mut GHash<Vec<usize>>, key: u64, val: usize, use_test: bool) -> bool {
    let (i, _) = gh.ensure(key, Vec::new);
    let list = gh.val_mut(i);
    if use_test && list.contains(&val) {
        return false;
    }
    list.insert(0, val);
    true
}

pub struct EdgePtrModel {
    pub stride: u64,
    pub base: u64,
}

impl State {
    fn edge_verts_add(&mut self, e: usize, v: usize, use_test: bool, model: &EdgePtrModel) {
        ghash_insert_link(&mut self.edge_verts, model.base + (e as u64) * model.stride, v, use_test);
    }
    fn face_edges_add(&mut self, f_index: i32, e: usize, use_test: bool) {
        ghash_insert_link(&mut self.face_edges, f_index as u64, e, use_test);
    }
}

fn intersect_line_tri(p0: V3, p1: V3, t_cos: [V3; 3], t_nor: V3, e: &Epsilon) -> (IsectType, V3) {
    let p_dir = normalize(sub(p0, p1));
    for i_t0 in 0..3usize {
        let i_t1 = (i_t0 + 1) % 3;
        let te_dir = normalize(sub(t_cos[i_t0], t_cos[i_t1]));
        if dot(p_dir, te_dir).abs() >= 1.0 - e.eps {
            // co-linear
        } else {
            let (ty, i0, mut i1) = isect_line_line_epsilon(p0, p1, t_cos[i_t0], t_cos[i_t1], 0.0);
            if ty != 0 {
                if ty == 1 {
                    i1 = i0;
                }
                if ty == 1 || len_squared_v3v3(i0, i1) <= e.eps_margin_sq {
                    let fac = line_point_factor_v3(i1, t_cos[i_t0], t_cos[i_t1]);
                    if fac >= e.eps_margin && fac <= 1.0 - e.eps_margin {
                        let fac = line_point_factor_v3(i0, p0, p1);
                        if fac >= e.eps_margin && fac <= 1.0 - e.eps_margin {
                            let ty = match i_t0 {
                                0 => IsectType::Edge0,
                                1 => IsectType::Edge1,
                                _ => IsectType::Edge2,
                            };
                            return (ty, i0);
                        }
                    }
                }
            }
        }
    }
    if dot(p_dir, t_nor).abs() >= e.eps {
        if let Some(fac) = isect_line_segment_tri_epsilon(p0, p1, t_cos[0], t_cos[1], t_cos[2], 0.0) {
            if fac >= e.eps_margin && fac <= 1.0 - e.eps_margin {
                let ix = interp_v3(p0, p1, fac);
                let m = len_squared_v3v3(t_cos[0], ix).min(len_squared_v3v3(t_cos[1], ix)).min(len_squared_v3v3(t_cos[2], ix));
                // min_fff
                let mut mm = len_squared_v3v3(t_cos[0], ix);
                let b = len_squared_v3v3(t_cos[1], ix);
                if b < mm {
                    mm = b;
                }
                let c = len_squared_v3v3(t_cos[2], ix);
                if c < mm {
                    mm = c;
                }
                let _ = m;
                if mm >= e.eps_margin_sq {
                    return (IsectType::Tri, ix);
                }
            }
        }
    }
    (IsectType::None, [0.0; 3])
}

impl BMesh {
    #[allow(clippy::too_many_arguments)]
    fn isect_edge_tri(&mut self, s: &mut State, model: &EdgePtrModel, e_v0: usize, e_v1: usize, t: [usize; 3], t_index: i32, t_cos: [V3; 3], t_nor: V3) -> (usize, IsectType) {
        let (mut e_v0, mut e_v1) = (e_v0, e_v1);
        if self.verts[e_v0].idx > self.verts[e_v1].idx {
            std::mem::swap(&mut e_v0, &mut e_v1);
        }
        let ti = [self.verts[t[0]].idx, self.verts[t[1]].idx, self.verts[t[2]].idx];
        let (i0, i1) = (self.verts[e_v0].idx, self.verts[e_v1].idx);
        let mut k_arr = [[0i32; 4]; 4];
        k_arr[3] = [i0, i1, t_index, -1];
        k_arr[0] = [i0, i1, ti[0], ti[1]];
        k_arr[1] = [i0, i1, ti[1], ti[2]];
        k_arr[2] = [i0, i1, ti[2], ti[0]];
        for k in 0..3 {
            let kk = &mut k_arr[k];
            if kk[2] > kk[3] {
                kk.swap(2, 3);
            }
            if kk[0] > kk[2] {
                kk.swap(0, 2);
                kk.swap(1, 3);
            }
        }
        for i in 0..4 {
            if let Some(&iv) = s.edgetri_cache.get(&k_arr[i]) {
                let side = match i {
                    0 => IsectType::Edge0,
                    1 => IsectType::Edge1,
                    2 => IsectType::Edge2,
                    _ => IsectType::Tri,
                };
                return (iv, side);
            }
        }
        let (side, ix) = intersect_line_tri(self.verts[e_v0].co, self.verts[e_v1].co, t_cos, t_nor, &s.eps);
        if side != IsectType::None {
            let iv = self.vert_create(ix, None);
            if dbg_on() {
                eprintln!("NEWV {} side {:?} tri {} edge ({},{}) at {:?}", iv, side, t_index, e_v0, e_v1, ix);
            }
            let e = self.edge_exists(e_v0, e_v1);
            if e != NONE {
                s.edge_verts_add(e, iv, false, model);
            } else {
                // vert_dissolve_add
                self.verts[iv].flag |= ELEM_TAG;
                s.vert_dissolve.push(iv);
            }
            if (side as i32) >= 0 && (side as i32) <= 2 {
                let i = side as usize;
                let e = self.edge_exists(t[i], t[(i + 1) % 3]);
                if e != NONE {
                    s.edge_verts_add(e, iv, false, model);
                }
            }
            s.edgetri_cache.insert(k_arr[(side as i32) as usize], iv);
            return (iv, side);
        }
        (NONE, IsectType::None)
    }

    #[allow(clippy::too_many_arguments)]
    fn isect_tri_tri(&mut self, s: &mut State, model: &EdgePtrModel, a_index: i32, b_index: i32, a: [usize; 3], b: [usize; 3], no_shared: bool) {
        let f_a = self.loops[a[0]].f;
        let f_b = self.loops[b[0]].f;
        let fv_a = [self.loops[a[0]].v, self.loops[a[1]].v, self.loops[a[2]].v];
        let fv_b = [self.loops[b[0]].v, self.loops[b[1]].v, self.loops[b[2]].v];
        let co = |bm: &BMesh, v: usize| bm.verts[v].co;
        if no_shared {
            for x in &fv_a {
                if fv_b.contains(x) {
                    return;
                }
            }
        } else if self.face_share_edge_check(f_a, f_b) {
            return;
        }
        let mut iv_ls_a: Vec<usize> = vec![];
        let mut iv_ls_b: Vec<usize> = vec![];
        let eps = &s.eps;
        let (eps_v, eps2x_sq, eps2x) = (eps.eps, eps.eps2x_sq, eps.eps2x);

        macro_rules! push_a {
            ($v:expr) => {{
                let v = $v;
                if self.verts[v].api & FLAG_WALK == 0 {
                    self.verts[v].api |= FLAG_WALK;
                    iv_ls_a.push(v);
                }
            }};
        }
        macro_rules! push_b {
            ($v:expr) => {{
                let v = $v;
                if self.verts[v].api & FLAG_WALK_ALT == 0 {
                    self.verts[v].api |= FLAG_WALK_ALT;
                    iv_ls_b.push(v);
                }
            }};
        }

        // vert-vert
        for i_a in 0..3 {
            for i_b in 0..3 {
                if len_squared_v3v3(co(self, fv_a[i_a]), co(self, fv_b[i_b])) <= eps2x_sq {
                    push_a!(fv_a[i_a]);
                    push_b!(fv_b[i_b]);
                }
            }
        }
        // vert-edge
        for i_a in 0..3 {
            if self.verts[fv_a[i_a]].api & FLAG_WALK == 0 {
                for i_b_e0 in 0..3 {
                    let i_b_e1 = (i_b_e0 + 1) % 3;
                    if self.verts[fv_b[i_b_e0]].api & FLAG_WALK_ALT != 0 || self.verts[fv_b[i_b_e1]].api & FLAG_WALK_ALT != 0 {
                        continue;
                    }
                    let fac = line_point_factor_v3(co(self, fv_a[i_a]), co(self, fv_b[i_b_e0]), co(self, fv_b[i_b_e1]));
                    if fac > 0.0 - eps_v && fac < 1.0 + eps_v {
                        let ix = interp_v3(co(self, fv_b[i_b_e0]), co(self, fv_b[i_b_e1]), fac);
                        if len_squared_v3v3(ix, co(self, fv_a[i_a])) <= eps2x_sq {
                            push_b!(fv_a[i_a]);
                            let e = self.edge_exists(fv_b[i_b_e0], fv_b[i_b_e1]);
                            if e != NONE {
                                s.edge_verts_add(e, fv_a[i_a], true, model);
                            }
                            break;
                        }
                    }
                }
            }
        }
        for i_b in 0..3 {
            if self.verts[fv_b[i_b]].api & FLAG_WALK_ALT == 0 {
                for i_a_e0 in 0..3 {
                    let i_a_e1 = (i_a_e0 + 1) % 3;
                    if self.verts[fv_a[i_a_e0]].api & FLAG_WALK != 0 || self.verts[fv_a[i_a_e1]].api & FLAG_WALK != 0 {
                        continue;
                    }
                    let fac = line_point_factor_v3(co(self, fv_b[i_b]), co(self, fv_a[i_a_e0]), co(self, fv_a[i_a_e1]));
                    if fac > 0.0 - eps_v && fac < 1.0 + eps_v {
                        let ix = interp_v3(co(self, fv_a[i_a_e0]), co(self, fv_a[i_a_e1]), fac);
                        if len_squared_v3v3(ix, co(self, fv_b[i_b])) <= eps2x_sq {
                            push_a!(fv_b[i_b]);
                            let e = self.edge_exists(fv_a[i_a_e0], fv_a[i_a_e1]);
                            if e != NONE {
                                s.edge_verts_add(e, fv_b[i_b], true, model);
                            }
                            break;
                        }
                    }
                }
            }
        }
        // vert-tri
        {
            let mut t_scale = [co(self, fv_b[0]), co(self, fv_b[1]), co(self, fv_b[2])];
            tri_v3_scale(&mut t_scale, 1.0 - eps2x);
            for i_a in 0..3 {
                if self.verts[fv_a[i_a]].api & FLAG_WALK != 0 {
                    continue;
                }
                if let Some(ix) = isect_point_tri(co(self, fv_a[i_a]), t_scale[0], t_scale[1], t_scale[2]) {
                    if len_squared_v3v3(ix, co(self, fv_a[i_a])) <= eps2x_sq {
                        push_a!(fv_a[i_a]);
                        push_b!(fv_a[i_a]);
                    }
                }
            }
        }
        {
            let mut t_scale = [co(self, fv_a[0]), co(self, fv_a[1]), co(self, fv_a[2])];
            tri_v3_scale(&mut t_scale, 1.0 - eps2x);
            for i_b in 0..3 {
                if self.verts[fv_b[i_b]].api & FLAG_WALK_ALT != 0 {
                    continue;
                }
                if let Some(ix) = isect_point_tri(co(self, fv_b[i_b]), t_scale[0], t_scale[1], t_scale[2]) {
                    if len_squared_v3v3(ix, co(self, fv_b[i_b])) <= eps2x_sq {
                        push_a!(fv_b[i_b]);
                        push_b!(fv_b[i_b]);
                    }
                }
            }
        }

        if !(iv_ls_a.len() >= 3 && iv_ls_b.len() >= 3) {
            let f_a_cos = [co(self, fv_a[0]), co(self, fv_a[1]), co(self, fv_a[2])];
            let f_b_cos = [co(self, fv_b[0]), co(self, fv_b[1]), co(self, fv_b[2])];
            let f_a_nor = normal_tri(f_a_cos[0], f_a_cos[1], f_a_cos[2]);
            let f_b_nor = normal_tri(f_b_cos[0], f_b_cos[1], f_b_cos[2]);
            // edge-tri & edge-edge
            for i_a_e0 in 0..3 {
                let i_a_e1 = (i_a_e0 + 1) % 3;
                if self.verts[fv_a[i_a_e0]].api & FLAG_WALK != 0 || self.verts[fv_a[i_a_e1]].api & FLAG_WALK != 0 {
                    continue;
                }
                let (iv, _side) = self.isect_edge_tri(s, model, fv_a[i_a_e0], fv_a[i_a_e1], fv_b, b_index, f_b_cos, f_b_nor);
                if iv != NONE {
                    push_a!(iv);
                    push_b!(iv);
                }
            }
            for i_b_e0 in 0..3 {
                let i_b_e1 = (i_b_e0 + 1) % 3;
                if self.verts[fv_b[i_b_e0]].api & FLAG_WALK_ALT != 0 || self.verts[fv_b[i_b_e1]].api & FLAG_WALK_ALT != 0 {
                    continue;
                }
                let (iv, _side) = self.isect_edge_tri(s, model, fv_b[i_b_e0], fv_b[i_b_e1], fv_a, a_index, f_a_cos, f_a_nor);
                if iv != NONE {
                    push_a!(iv);
                    push_b!(iv);
                }
            }
            for i in 0..2 {
                let (ie_vs, f) = if i == 0 {
                    if iv_ls_a.len() != 2 {
                        continue;
                    }
                    (iv_ls_a.clone(), f_a)
                } else {
                    if iv_ls_b.len() != 2 {
                        continue;
                    }
                    (iv_ls_b.clone(), f_b)
                };
                let mut ie = self.edge_exists(ie_vs[0], ie_vs[1]);
                let ie_exists;
                if ie == NONE {
                    ie_exists = false;
                    ie = self.edge_create(ie_vs[0], ie_vs[1], None, false);
                    s.wire_edges.insert(ie);
                } else {
                    ie_exists = true;
                    s.wire_edges.insert(ie);
                    if self.edge_in_face(ie, f) {
                        continue;
                    }
                }
                let fi = self.faces[f].idx;
                s.face_edges_add(fi, ie, ie_exists);
            }
        }
        for &v in &iv_ls_a {
            self.verts[v].api &= !FLAG_WALK;
        }
        for &v in &iv_ls_b {
            self.verts[v].api &= !FLAG_WALK_ALT;
        }
    }

    /// BM_face_calc_point_in_face
    fn face_calc_point_in_face(&self, f: usize) -> V3 {
        let ls = self.loops_of_face(f);
        let co = |l: usize| self.verts[self.loops[l].v].co;
        let ltri: [usize; 3];
        if self.faces[f].len == 3 {
            let l = self.faces[f].l_first;
            ltri = [l, self.loops[l].n, self.loops[l].p];
        } else {
            let tottri = self.faces[f].len - 2;
            let no = self.faces[f].no;
            let m = super::edgenet::axis_dominant_v3_to_m3(no, true);
            let proj: Vec<[f32; 2]> = ls.iter().map(|&l| super::edgenet::mul_v2_m3v3(&m, co(l))).collect();
            let index = crate::exbool::polyfill::polyfill_calc(&proj);
            let mut j_best = 0;
            let mut area_best = -1.0f32;
            for j in 0..tottri {
                let area = area_squared_tri(co(ls[index[j][0] as usize]), co(ls[index[j][1] as usize]), co(ls[index[j][2] as usize]));
                if area > area_best {
                    j_best = j;
                    area_best = area;
                }
            }
            ltri = [ls[index[j_best][0] as usize], ls[index[j_best][1] as usize], ls[index[j_best][2] as usize]];
        }
        mid3(co(ltri[0]), co(ltri[1]), co(ltri[2]))
    }
}

/// bm_loop_filter_fn / face-group walk (BM_mesh_calc_face_groups with htype_step = BM_EDGE)
fn calc_face_groups(bm: &mut BMesh, test_fn: &dyn Fn(&BMesh, usize) -> i32) -> (Vec<i32>, Vec<(usize, usize)>) {
    let fids = bm.face_ids();
    for (i, &f) in fids.iter().enumerate() {
        bm.faces[f].flag &= !ELEM_TAG;
        bm.faces[f].idx = i as i32;
    }
    let tot_faces = fids.len();
    let mut tot_touch = 0;
    let mut groups_array: Vec<i32> = vec![];
    let mut group_index: Vec<(usize, usize)> = vec![];
    let mut iter_pos = 0usize;
    while tot_touch != tot_faces {
        let mut stack: Vec<usize> = vec![];
        while iter_pos < fids.len() {
            let f_next = fids[iter_pos];
            iter_pos += 1;
            if bm.faces[f_next].flag & ELEM_TAG == 0 {
                bm.faces[f_next].flag |= ELEM_TAG;
                stack.push(f_next);
                break;
            }
        }
        let start = groups_array.len();
        let mut count = 0;
        while let Some(f) = stack.pop() {
            groups_array.push(bm.faces[f].idx);
            tot_touch += 1;
            count += 1;
            let l_first = bm.faces[f].l_first;
            let mut l_iter = l_first;
            loop {
                let l_radial_start = bm.loops[l_iter].rn;
                if l_radial_start != l_iter && filter(bm, l_iter, test_fn) {
                    let mut l_radial_iter = l_radial_start;
                    loop {
                        let f_other = bm.loops[l_radial_iter].f;
                        if bm.faces[f_other].flag & ELEM_TAG == 0 {
                            bm.faces[f_other].flag |= ELEM_TAG;
                            stack.push(f_other);
                        }
                        l_radial_iter = bm.loops[l_radial_iter].rn;
                        if l_radial_iter == l_iter {
                            break;
                        }
                    }
                }
                l_iter = bm.loops[l_iter].n;
                if l_iter == l_first {
                    break;
                }
            }
        }
        group_index.push((start, count));
    }
    (groups_array, group_index)
}

fn filter(bm: &BMesh, l: usize, test_fn: &dyn Fn(&BMesh, usize) -> i32) -> bool {
    if bm.edges[bm.loops[l].e].flag & ELEM_TAG != 0 {
        return false;
    }
    if bm.loops[l].rn != l {
        let mut l_iter = bm.loops[l].rn;
        let face_side = test_fn(bm, bm.loops[l].f);
        loop {
            let face_side_other = test_fn(bm, bm.loops[l_iter].f);
            if face_side_other == -1 {
            } else if face_side_other != face_side {
                return false;
            }
            l_iter = bm.loops[l_iter].rn;
            if l_iter == l {
                break;
            }
        }
        return true;
    }
    false
}

fn isect_bvhtree_point(tree: &BvhTree, tri_verts: &[[usize; 3]], bm: &BMesh, co: V3) -> i32 {
    let mut z_buffer: Vec<f32> = vec![];
    let mut hit_index = -1i32;
    let mut hit_dist = RAYCAST_DIST_MAX;
    if tree.leaf_num > 0 {
        tree.ray_cast(co, [1.0, 0.0, 0.0], &mut hit_index, &mut hit_dist, &mut |index: i32, _cur: f32| {
            let tv = tri_verts[index as usize];
            let (v0, v1, v2) = (bm.verts[tv[0]].co, bm.verts[tv[1]].co, bm.verts[tv[2]].co);
            if let Some(dist) = isect_ray_tri_watertight_x(co, v0, v1, v2) {
                if dist >= 0.0 {
                    z_buffer.push(dist);
                }
            }
            None
        });
    }
    if z_buffer.is_empty() {
        0
    } else if z_buffer.len() == 1 {
        1
    } else {
        let eps = f32::EPSILON * 10.0;
        let mut num_isect = 1;
        super::qsort::qsort(&mut z_buffer, &|a: &f32, b: &f32| {
            if a > b {
                1
            } else if a < b {
                -1
            } else {
                0
            }
        });
        let mut depth_last = z_buffer[0];
        for i in 1..z_buffer.len() {
            if z_buffer[i] - depth_last > eps {
                depth_last = z_buffer[i];
                num_isect += 1;
            }
        }
        num_isect
    }
}

pub fn edge_ptr_model() -> EdgePtrModel {
    let stride = std::env::var("GNBOOL_PTR_STRIDE").ok().and_then(|s| s.parse().ok()).unwrap_or(72);
    let base = std::env::var("GNBOOL_PTR_BASE").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    EdgePtrModel { stride, base }
}

/// BM_mesh_intersect for the boolean FLOAT solver. `looptris` are loops (indices into bm.loops).
/// `test_fn(bm, face) -> 0 | 1 | -1`.
pub fn mesh_intersect_boolean(bm: &mut BMesh, looptris: &[[usize; 3]], test_fn: &dyn Fn(&BMesh, usize) -> i32, boolean_mode: i32, eps: f32) {
    let model = edge_ptr_model();
    let mut s = State {
        eps: Epsilon { eps, eps2x: eps * 2.0, eps_margin: eps * 2.0 * 10.0, eps_sq: eps * eps, eps2x_sq: (eps * 2.0) * (eps * 2.0), eps_margin_sq: (eps * 2.0 * 10.0) * (eps * 2.0 * 10.0) },
        edgetri_cache: StdMap::new(),
        edge_verts: GHash::new(ptrhash),
        face_edges: GHash::new(inthash_p),
        wire_edges: Default::default(),
        vert_dissolve: vec![],
    };
    let _ = (s.eps.eps_sq,);
    // BM_mesh_elem_index_ensure(VERT | EDGE | FACE)
    for (i, v) in bm.vert_ids().into_iter().enumerate() {
        bm.verts[v].idx = i as i32;
    }
    for (i, e) in bm.edge_ids().into_iter().enumerate() {
        bm.edges[e].idx = i as i32;
    }
    for (i, f) in bm.face_ids().into_iter().enumerate() {
        bm.faces[f].idx = i as i32;
    }
    let totface_orig = bm.totface;
    let ftable_orig: Vec<usize> = bm.face_ids();
    // BM_mesh_elem_hflag_disable_all(EDGE | VERT, TAG)
    for v in bm.vert_ids() {
        bm.verts[v].flag &= !ELEM_TAG;
    }
    for e in bm.edge_ids() {
        bm.edges[e].flag &= !ELEM_TAG;
    }
    let tri_verts: Vec<[usize; 3]> = looptris.iter().map(|t| [bm.loops[t[0]].v, bm.loops[t[1]].v, bm.loops[t[2]].v]).collect();

    let mut tree_a = BvhTree::new(looptris.len(), s.eps.eps_margin, 8, 8);
    let mut tree_b = BvhTree::new(looptris.len(), s.eps.eps_margin, 8, 8);
    for (i, lt) in looptris.iter().enumerate() {
        let side = test_fn(bm, bm.loops[lt[0]].f);
        let t_cos = [bm.verts[bm.loops[lt[0]].v].co, bm.verts[bm.loops[lt[1]].v].co, bm.verts[bm.loops[lt[2]].v].co];
        if side == 0 {
            tree_a.insert(i as i32, &t_cos);
        }
    }
    for (i, lt) in looptris.iter().enumerate() {
        let side = test_fn(bm, bm.loops[lt[0]].f);
        let t_cos = [bm.verts[bm.loops[lt[0]].v].co, bm.verts[bm.loops[lt[1]].v].co, bm.verts[bm.loops[lt[2]].v].co];
        if side == 1 {
            tree_b.insert(i as i32, &t_cos);
        }
    }
    if tree_a.leaf_num > 0 {
        tree_a.balance();
    }
    if tree_b.leaf_num > 0 {
        tree_b.balance();
    }
    let isect_tri_tri_no_shared = true;
    if tree_a.leaf_num > 0 && tree_b.leaf_num > 0 {
        let overlap = tree_b.overlap_ex(&tree_a);
        for (ia, ib) in overlap {
            let a = looptris[ia as usize];
            let b = looptris[ib as usize];
            if dbg_on() {
                eprintln!("PAIR {} {}", ia, ib);
            }
            bm.isect_tri_tri(&mut s, &model, ia, ib, a, b, isect_tri_tri_no_shared);
        }
    }

    // ---- USE_SPLICE
    for gi in s.edge_verts.iter_order() {
        let e = s.edge_verts.key(gi);
        let e = ((e - model.base) / model.stride) as usize;
        let mut list = s.edge_verts.val(gi).clone();
        let v_start = bm.edges[e].v1;
        if list.len() > 1 {
            // edge_verts_sort
            let co = bm.verts[v_start].co;
            let mut vs: Vec<(f32, usize)> = list.iter().map(|&v| (len_squared_v3v3(co, bm.verts[v].co), v)).collect();
            super::qsort::qsort(&mut vs, &|a: &(f32, usize), b: &(f32, usize)| {
                if a.0 > b.0 {
                    1
                } else if a.0 < b.0 {
                    -1
                } else {
                    0
                }
            });
            list = vs.into_iter().map(|x| x.1).collect();
        }
        let is_wire = s.wire_edges.contains(&e);
        let mut v_prev = v_start;
        for &vi in &list {
            let fac = line_point_factor_v3(bm.verts[vi].co, bm.verts[bm.edges[e].v1].co, bm.verts[bm.edges[e].v2].co);
            if bm.vert_in_edge(e, v_prev) {
                let fac = fac.clamp(0.0, 1.0);
                let (v_new, e_split) = bm.edge_split(e, v_prev, fac);
                v_prev = v_new;
                if bm.edge_exists(v_prev, vi) == NONE && !bm.vert_splice_check_double(v_prev, vi) && !bm.vert_pair_share_face_check(v_prev, vi) {
                    bm.vert_splice(vi, v_prev);
                } else {
                    bm.verts[v_prev].co = bm.verts[vi].co;
                }
                v_prev = vi;
                if is_wire {
                    s.wire_edges.insert(e_split);
                }
            }
        }
    }

    // ---- USE_NET
    for gi in s.face_edges.iter_order() {
        let f_index = s.face_edges.key(gi) as usize;
        let list = s.face_edges.val(gi).clone();
        let f = ftable_orig[f_index];
        let mut edge_arr = list;
        if let Some(new_net) = bm.face_split_edgenet_connect_islands(f, &edge_arr) {
            edge_arr = new_net;
        }
        let mut rf = vec![];
        bm.face_split_edgenet(f, &edge_arr, Some(&mut rf));
        if dbg_on() {
            eprintln!("net key {} f {} edges {} -> faces {:?}", f_index, f, edge_arr.len(), rf);
        }
    }
    let _ = totface_orig;

    // tag wire edges
    let mut wires: Vec<usize> = s.wire_edges.iter().copied().collect();
    wires.sort();
    for e in wires {
        bm.edges[e].flag |= ELEM_TAG;
    }

    // ---- boolean
    let ftable: Vec<usize> = bm.face_ids();
    let (groups_array, group_index) = calc_face_groups(bm, test_fn);
    let tree_pair = [&tree_a, &tree_b];
    for &(fg0, cnt) in &group_index {
        let f = ftable[groups_array[fg0] as usize];
        let mut side = test_fn(bm, f);
        if side == -1 {
            continue;
        }
        side = if side == 0 { 1 } else { 0 };
        let co = bm.face_calc_point_in_face(f);
        let hits = isect_bvhtree_point(tree_pair[side as usize], &tri_verts, bm, co);
        let (do_remove, do_flip) = match boolean_mode {
            BOOLEAN_ISECT => ((hits & 1) != 1, false),
            BOOLEAN_UNION => ((hits & 1) == 1, false),
            _ => ((((hits & 1) == 1) as i32) == side, side == 0),
        };
        if dbg_on() {
            eprintln!("group start {} cnt {} f {} side {} co {:?} hits {} remove {} flip {}", fg0, cnt, f, side, co, hits, do_remove, do_flip);
        }
        if do_remove {
            for k in fg0..fg0 + cnt {
                bm.faces[ftable[groups_array[k] as usize]].mat_nr = -1;
            }
        } else if do_flip {
            for k in fg0..fg0 + cnt {
                bm.face_normal_flip(ftable[groups_array[k] as usize]);
            }
        }
    }
    // dissolve
    for &v in s.vert_dissolve.iter().rev() {
        if !bm.verts[v].alive {
            continue;
        }
        if bm.vert_is_edge_pair(v) {
            let mut ok = true;
            let ve = bm.verts[v].e;
            let first = bm.edges[ve].l;
            if first != NONE {
                let mut l = first;
                loop {
                    if bm.faces[bm.loops[l].f].len == 3 {
                        ok = false;
                        break;
                    }
                    l = bm.loops[l].rn;
                    if l == first {
                        break;
                    }
                }
            }
            if ok {
                bm.vert_collapse_edge(ve, v);
            }
        }
    }
    for &f in &ftable {
        if bm.faces[f].alive && bm.faces[f].mat_nr == -1 {
            bm.face_kill_loose(f);
        }
    }
}
