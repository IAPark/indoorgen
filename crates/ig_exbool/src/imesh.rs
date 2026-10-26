//! Port of the IMesh data structures of Blender's BLI_mesh_intersect.hh / bli_mesh_intersect.cc
//! (Vert, Plane, Face, IMeshArena, IMesh helpers).

use crate::exbool::mpq::*;
use num_traits::Zero;
use std::collections::HashMap;
use num_bigint::BigInt;
use std::hash::{BuildHasherDefault, Hash, Hasher};

/// Fx-style hasher for the exact-vertex map (lookup only, never iterated)
#[derive(Default, Clone, Copy)]
pub struct VHasher(u64);
impl Hasher for VHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}
fn vhash(co: &V3q) -> u64 {
    let mut h = VHasher::default();
    VKey::hash_co(co, &mut h);
    h.finish()
}
/// key of the vertex map: rationals are kept reduced (num-rational normalises), so equal values have equal (numer, denom) limbs;
/// the std `Hash for Ratio` does a continued-fraction recursion per hash, which dominated the exact booleans
#[derive(Clone, PartialEq, Eq)]
pub struct VKey(pub V3q);
impl VKey {
    fn hash_co<H: Hasher>(co: &V3q, h: &mut H) {
        for q in co {
            hash_big(q.numer(), h);
            hash_big(q.denom(), h);
        }
    }
}
fn hash_big<H: Hasher>(b: &BigInt, h: &mut H) {
    h.write_u64(b.sign() as i64 as u64);
    for d in b.magnitude().iter_u64_digits() {
        h.write_u64(d);
    }
}
impl Hash for VKey {
    fn hash<H: Hasher>(&self, h: &mut H) {
        for q in &self.0 {
            hash_big(q.numer(), h);
            hash_big(q.denom(), h);
        }
    }
}

pub const NO_INDEX: i32 = -1;

pub type V3q = [Q; 3];

#[derive(Clone)]
pub struct Vert {
    pub co: V3q,
    pub d: [f64; 3],
    pub id: i32,
    pub orig: i32,
}

#[derive(Clone)]
pub struct Plane {
    pub norm_exact: V3q,
    pub d_exact: Q,
    pub norm: [f64; 3],
    pub d: f64,
}

impl Plane {
    pub fn from_exact(norm_exact: V3q, d_exact: Q) -> Plane {
        let norm = q3_get_d(&norm_exact);
        let d = q_get_d(&d_exact);
        Plane { norm_exact, d_exact, norm, d }
    }
    pub fn from_double(norm: [f64; 3], d: f64) -> Plane {
        Plane { norm_exact: [Q::zero(), Q::zero(), Q::zero()], d_exact: Q::zero(), norm, d }
    }
    pub fn exact_populated(&self) -> bool {
        !self.norm_exact[0].is_zero() || !self.norm_exact[1].is_zero() || !self.norm_exact[2].is_zero()
    }
    pub fn make_canonical(&mut self) {
        if !self.norm_exact[0].is_zero() {
            let den = self.norm_exact[0].clone();
            self.norm_exact = [q_one(), &self.norm_exact[1] / &den, &self.norm_exact[2] / &den];
            self.d_exact = &self.d_exact / &den;
        } else if !self.norm_exact[1].is_zero() {
            let den = self.norm_exact[1].clone();
            self.norm_exact = [q_zero(), q_one(), &self.norm_exact[2] / &den];
            self.d_exact = &self.d_exact / &den;
        } else if !self.norm_exact[2].is_zero() {
            let den = self.norm_exact[2].clone();
            self.norm_exact = [q_zero(), q_zero(), q_one()];
            self.d_exact = &self.d_exact / &den;
        } else {
            self.d_exact = q_zero();
        }
        self.norm = q3_get_d(&self.norm_exact);
        self.d = q_get_d(&self.d_exact);
    }
    /// Plane::hash()
    pub fn hash(&self) -> u64 {
        let mut x = self.norm[0].to_bits();
        let mut y = self.norm[1].to_bits();
        let mut z = self.norm[2].to_bits();
        let mut d = self.d.to_bits();
        x = (x >> 56) ^ (x >> 46) ^ x;
        y = (y >> 55) ^ (y >> 45) ^ y;
        z = (z >> 54) ^ (z >> 44) ^ z;
        d = (d >> 53) ^ (d >> 43) ^ d;
        x ^ y ^ z ^ d
    }
}

#[derive(Clone)]
pub struct Face {
    pub vert: Vec<u32>,
    pub edge_orig: Vec<i32>,
    pub is_intersect: Vec<bool>,
    pub plane: Option<Box<Plane>>,
    pub orig: i32,
}

impl Face {
    pub fn size(&self) -> usize {
        self.vert.len()
    }
}

#[derive(Default)]
pub struct Arena {
    pub verts: Vec<Vert>,
    vmap: HashMap<u64, Vec<u32>, BuildHasherDefault<VHasher>>,
    pub faces: Vec<Face>,
}

pub fn dot3d(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross3d(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn sub3d(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl Arena {
    pub fn new() -> Arena {
        Arena::default()
    }

    /// add_or_find_vert(mpq3, orig): double coordinates derived by get_d (truncation).
    pub fn add_or_find_vert_q(&mut self, co: V3q, orig: i32) -> u32 {
        if let Some(i) = self.find_vert(&co) {
            return i;
        }
        let d = q3_get_d(&co);
        self.insert_vert(co, d, orig)
    }

    /// add_or_find_vert(Vert*) with an already computed double coordinate.
    pub fn add_or_find_vert_with_d(&mut self, co: V3q, d: [f64; 3], orig: i32) -> u32 {
        if let Some(i) = self.find_vert(&co) {
            return i;
        }
        self.insert_vert(co, d, orig)
    }

    fn insert_vert(&mut self, co: V3q, d: [f64; 3], orig: i32) -> u32 {
        let id = self.verts.len() as u32;
        self.vmap.entry(vhash(&co)).or_default().push(id);
        self.verts.push(Vert { co, d, id: id as i32, orig });
        id
    }

    pub fn find_vert(&self, co: &V3q) -> Option<u32> {
        self.vmap.get(&vhash(co)).and_then(|l| l.iter().copied().find(|&i| self.verts[i as usize].co == *co))
    }

    pub fn add_face(&mut self, vert: Vec<u32>, orig: i32, edge_orig: Vec<i32>, is_intersect: Vec<bool>) -> usize {
        self.faces.push(Face { vert, edge_orig, is_intersect, plane: None, orig });
        self.faces.len() - 1
    }

    /// Face::populate_plane
    pub fn populate_plane(&mut self, fi: usize, need_exact: bool) {
        if let Some(p) = &self.faces[fi].plane {
            if !need_exact || p.exact_populated() {
                return;
            }
        }
        let vs: Vec<u32> = self.faces[fi].vert.clone();
        let plane = if need_exact {
            let normal_exact: V3q = if vs.len() > 3 {
                // cross_poly (Newell) in exact arithmetic
                let nv = vs.len();
                let mut n = [q_zero(), q_zero(), q_zero()];
                let mut prev = &self.verts[vs[nv - 1] as usize].co;
                for i in 0..nv {
                    let cur = &self.verts[vs[i] as usize].co;
                    n[0] = &n[0] + (&prev[1] - &cur[1]) * (&prev[2] + &cur[2]);
                    n[1] = &n[1] + (&prev[2] - &cur[2]) * (&prev[0] + &cur[0]);
                    n[2] = &n[2] + (&prev[0] - &cur[0]) * (&prev[1] + &cur[1]);
                    prev = cur;
                }
                n
            } else {
                let tr02 = q3_sub(&self.verts[vs[0] as usize].co, &self.verts[vs[2] as usize].co);
                let tr12 = q3_sub(&self.verts[vs[1] as usize].co, &self.verts[vs[2] as usize].co);
                q3_cross(&tr02, &tr12)
            };
            let d_exact = -q3_dot(&normal_exact, &self.verts[vs[0] as usize].co);
            Plane::from_exact(normal_exact, d_exact)
        } else {
            let normal: [f64; 3] = if vs.len() > 3 {
                let nv = vs.len();
                let mut n = [0.0f64; 3];
                let mut prev = self.verts[vs[nv - 1] as usize].d;
                for i in 0..nv {
                    let cur = self.verts[vs[i] as usize].d;
                    n[0] = n[0] + (prev[1] - cur[1]) * (prev[2] + cur[2]);
                    n[1] = n[1] + (prev[2] - cur[2]) * (prev[0] + cur[0]);
                    n[2] = n[2] + (prev[0] - cur[0]) * (prev[1] + cur[1]);
                    prev = cur;
                }
                n
            } else {
                let tr02 = sub3d(&self.verts[vs[0] as usize].d, &self.verts[vs[2] as usize].d);
                let tr12 = sub3d(&self.verts[vs[1] as usize].d, &self.verts[vs[2] as usize].d);
                cross3d(&tr02, &tr12)
            };
            let d = -dot3d(&normal, &self.verts[vs[0] as usize].d);
            Plane::from_double(normal, d)
        };
        self.faces[fi].plane = Some(Box::new(plane));
    }
}

/// Face::cyclic_equal on vertex-id lists.
pub fn cyclic_equal(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let n = a.len();
    for s in 0..n {
        for so in 0..n {
            let mut ok = true;
            for i in 0..n {
                if a[(s + i) % n] != b[(so + i) % n] {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
        }
    }
    false
}

/// `IMesh::erase_face_positions`: returns (new face index or None when the face disappears).
pub fn erase_face_positions(arena: &mut Arena, face: usize, erase: &[bool]) -> Option<usize> {
    let cur_len = arena.faces[face].size();
    let to_erase = erase.iter().filter(|&&b| b).count();
    if to_erase == 0 {
        return Some(face);
    }
    let new_len = cur_len - to_erase;
    if new_len < 3 {
        return None;
    }
    let f = arena.faces[face].clone();
    let mut nv = vec![];
    let mut ne = vec![];
    let mut ni = vec![];
    for i in 0..cur_len {
        if !erase[i] {
            nv.push(f.vert[i]);
            ne.push(f.edge_orig[i]);
            ni.push(f.is_intersect[i]);
        }
    }
    Some(arena.add_face(nv, f.orig, ne, ni))
}

/// `IMesh::populate_vert`: returns the vertices (arena ids) in output order, and a lookup table id -> index.
pub fn populate_vert(arena: &Arena, faces: &[usize]) -> (Vec<u32>, HashMap<u32, usize>) {
    let mut seen: HashMap<u32, usize> = HashMap::new();
    let mut order: Vec<u32> = vec![];
    for &f in faces {
        for &v in &arena.faces[f].vert {
            if !seen.contains_key(&v) {
                seen.insert(v, order.len());
                order.push(v);
            }
        }
    }
    // fix_order: blender::parallel_sort (std::sort) with this comparator; ties (equal `orig`) are resolved the way
    // libc++'s introsort resolves them.
    crate::exbool::libcxx_sort::blender_parallel_sort(&mut order, |a: &u32, b: &u32| {
        let (va, vb) = (&arena.verts[*a as usize], &arena.verts[*b as usize]);
        if va.orig != NO_INDEX && vb.orig != NO_INDEX {
            return va.orig < vb.orig;
        }
        if va.orig != NO_INDEX {
            return true;
        }
        if vb.orig != NO_INDEX {
            return false;
        }
        va.id < vb.id
    });
    let mut idx = HashMap::new();
    for (i, &v) in order.iter().enumerate() {
        idx.insert(v, i);
    }
    (order, idx)
}
