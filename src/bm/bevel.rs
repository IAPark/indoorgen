//! Port of Blender 4.2 `bmesh_bevel.cc` (edge bevel; offset types, superellipse profiles, M_POLY / M_ADJ / M_TRI_FAN vertex meshes,
//! sharp miters). Seams, sharp marking, UV layers, harden normals, face strength, custom profiles, vertex bevel and the
//! cutoff vertex-mesh method are not ported (the entry point reports `Err` if they would be needed).
use super::query::*;
use super::*;
use crate::dhash::HashMap;

const BEVEL_EPSILON_D: f64 = 1e-6;
const BEVEL_EPSILON: f32 = 1e-6;
const BEVEL_EPSILON_SQ: f32 = 1e-12;
const BEVEL_EPSILON_BIG: f32 = 1e-4;
const BEVEL_MATCH_SPEC_WEIGHT: f64 = 0.2;
const PRO_SQUARE_R: f32 = 1e4;
const PRO_CIRCLE_R: f32 = 2.0;
const PRO_LINE_R: f32 = 1.0;
const PRO_SQUARE_IN_R: f32 = 0.0;
const SQRT2_F: f32 = std::f32::consts::SQRT_2;

fn deg2radf(d: f64) -> f32 {
    (d * (std::f64::consts::PI / 180.0)) as f32
}
fn eps_ang() -> f32 {
    deg2radf(2.0)
}
fn small_ang() -> f32 {
    deg2radf(10.0)
}
fn small_ang_dot() -> f32 {
    1.0 - small_ang().cos()
}
fn eps_ang_dot() -> f32 {
    1.0 - eps_ang().cos()
}

#[derive(Clone, Copy, Default, Debug)]
pub struct NewVert {
    pub v: usize,
    pub co: V3,
}

#[derive(Clone, Debug)]
pub struct EdgeHalf {
    pub next: usize,
    pub prev: usize,
    pub e: usize,
    pub fprev: usize,
    pub fnext: usize,
    pub leftv: usize,
    pub rightv: usize,
    pub profile_index: i32,
    pub seg: i32,
    pub offset_l: f32,
    pub offset_r: f32,
    pub offset_l_spec: f32,
    pub offset_r_spec: f32,
    pub is_bev: bool,
    pub is_rev: bool,
    pub is_seam: bool,
    pub visited_rpo: bool,
}
impl EdgeHalf {
    fn blank() -> EdgeHalf {
        EdgeHalf {
            next: NONE,
            prev: NONE,
            e: NONE,
            fprev: NONE,
            fnext: NONE,
            leftv: NONE,
            rightv: NONE,
            profile_index: 0,
            seg: 0,
            offset_l: 0.0,
            offset_r: 0.0,
            offset_l_spec: 0.0,
            offset_r_spec: 0.0,
            is_bev: false,
            is_rev: false,
            is_seam: false,
            visited_rpo: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub super_r: f32,
    pub height: f32,
    pub start: V3,
    pub middle: V3,
    pub end: V3,
    pub plane_no: V3,
    pub plane_co: V3,
    pub proj_dir: V3,
    pub prof_co: Vec<f32>,
    pub prof_co_2: Vec<f32>,
    pub special_params: bool,
}

#[derive(Clone, Debug)]
pub struct BoundVert {
    pub next: usize,
    pub prev: usize,
    pub nv: NewVert,
    pub efirst: usize,
    pub elast: usize,
    pub eon: usize,
    pub ebev: usize,
    pub index: i32,
    pub sinratio: f32,
    pub adjchain: usize,
    pub profile: Profile,
    pub any_seam: bool,
    pub visited: bool,
    pub is_arc_start: bool,
    pub is_patch_start: bool,
    pub is_profile_start: bool,
    pub seam_len: i32,
    pub sharp_len: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeshKind {
    None,
    Poly,
    Adj,
    TriFan,
    Cutoff,
}

#[derive(Clone, Debug)]
pub struct VMesh {
    pub mesh: Vec<NewVert>,
    pub boundstart: usize,
    pub count: i32,
    pub seg: i32,
    pub mesh_kind: MeshKind,
}

#[derive(Clone, Debug)]
pub struct BevVert {
    pub v: usize,
    pub edgecount: usize,
    pub selcount: usize,
    pub wirecount: usize,
    pub offset: f32,
    pub any_seam: bool,
    pub visited: bool,
    /// index of the first EdgeHalf in the arena (edges are contiguous)
    pub e0: usize,
    pub wire_edges: Vec<usize>,
    pub vmesh: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FKind {
    None,
    Orig,
    Vert,
    Edge,
    Recon,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AngleKind {
    Smaller,
    Straight,
    Larger,
}

#[derive(Default, Clone)]
pub struct ProfileSpacing {
    pub xvals: Vec<f64>,
    pub yvals: Vec<f64>,
    pub xvals_2: Vec<f64>,
    pub yvals_2: Vec<f64>,
    pub seg_2: i32,
    pub fullness: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffsetType {
    Offset,
    Width,
    Depth,
    Percent,
    Absolute,
}

pub struct BevelOpts {
    pub offset: f32,
    pub offset_type: OffsetType,
    pub segments: i32,
    pub profile: f32,
    pub limit_offset: bool,
    pub loop_slide: bool,
    pub mat_nr: i32,
}

pub struct Bp<'a> {
    pub bm: &'a mut BMesh,
    pub offset: f32,
    pub offset_type: OffsetType,
    pub seg: i32,
    pub profile: f32,
    pub pro_super_r: f32,
    pub loop_slide: bool,
    pub limit_offset: bool,
    pub offset_adjust: bool,
    /// bevel affect = VERTICES (bmo_bevel with vertices selected; verts tagged, edges not)
    pub vertex_only: bool,
    pub mat_nr: i32,
    pub pro_spacing: ProfileSpacing,
    pub ehs: Vec<EdgeHalf>,
    pub bvs: Vec<BevVert>,
    pub bounds: Vec<BoundVert>,
    pub vms: Vec<VMesh>,
    pub vert_hash: HashMap<usize, usize>,
    pub face_hash: HashMap<usize, FKind>,
    pub unsupported: Option<String>,
}

pub fn power_of_2_max_i(n: i32) -> i32 {
    if n <= 0 {
        return 1;
    }
    let mut p = 1;
    while p < n {
        p <<= 1;
    }
    p
}

impl<'a> Bp<'a> {
    // ---------------------------------------------------------------- small helpers
    fn co(&self, v: usize) -> V3 {
        self.bm.verts[v].co
    }
    fn vno(&self, v: usize) -> V3 {
        self.bm.verts[v].no
    }
    fn fno(&self, f: usize) -> V3 {
        self.bm.faces[f].no
    }
    fn tag(&self, v: usize) -> bool {
        self.bm.verts[v].flag & ELEM_TAG != 0
    }
    fn record_face_kind(&mut self, f: usize, k: FKind) {
        self.face_hash.insert(f, k);
    }
    fn get_face_kind(&self, f: usize) -> FKind {
        *self.face_hash.get(&f).unwrap_or(&FKind::Orig)
    }
    fn nearly_parallel(d1: V3, d2: V3) -> bool {
        let ang = angle_v3v3(d1, d2);
        ang.abs() < eps_ang() || (ang - PI).abs() < eps_ang()
    }
    fn nearly_parallel_normalized(d1: V3, d2: V3) -> bool {
        let direction_dot = dot(d1, d2);
        compare_ff(direction_dot.abs(), 1.0, eps_ang_dot())
    }

    fn add_new_bound_vert(&mut self, vm: usize, co: V3) -> usize {
        let id = self.bounds.len();
        let mut bv = BoundVert {
            next: id,
            prev: id,
            nv: NewVert { v: NONE, co },
            efirst: NONE,
            elast: NONE,
            eon: NONE,
            ebev: NONE,
            index: 0,
            sinratio: 1.0,
            adjchain: NONE,
            profile: Profile { super_r: PRO_LINE_R, ..Default::default() },
            any_seam: false,
            visited: false,
            is_arc_start: false,
            is_patch_start: false,
            is_profile_start: false,
            seam_len: 0,
            sharp_len: 0,
        };
        let bs = self.vms[vm].boundstart;
        if bs == NONE {
            bv.index = 0;
            self.bounds.push(bv);
            self.vms[vm].boundstart = id;
        } else {
            let tail = self.bounds[bs].prev;
            bv.index = self.bounds[tail].index + 1;
            bv.prev = tail;
            bv.next = bs;
            self.bounds.push(bv);
            self.bounds[tail].next = id;
            self.bounds[bs].prev = id;
        }
        self.vms[vm].count += 1;
        id
    }
    fn adjust_bound_vert(&mut self, bv: usize, co: V3) {
        self.bounds[bv].nv.co = co;
    }
    fn mesh_idx(&self, vm: usize, i: i32, j: i32, k: i32) -> usize {
        let v = &self.vms[vm];
        let nj = v.seg / 2 + 1;
        let nk = v.seg + 1;
        (i * nk * nj + j * nk + k) as usize
    }
    fn mv(&self, vm: usize, i: i32, j: i32, k: i32) -> NewVert {
        self.vms[vm].mesh[self.mesh_idx(vm, i, j, k)]
    }
    fn mv_co(&self, vm: usize, i: i32, j: i32, k: i32) -> V3 {
        self.mv(vm, i, j, k).co
    }
    fn set_mv_co(&mut self, vm: usize, i: i32, j: i32, k: i32, co: V3) {
        let ix = self.mesh_idx(vm, i, j, k);
        self.vms[vm].mesh[ix].co = co;
    }
    fn create_mesh_bmvert(&mut self, vm: usize, i: i32, j: i32, k: i32, eg: usize) {
        let ix = self.mesh_idx(vm, i, j, k);
        let co = self.vms[vm].mesh[ix].co;
        let v = self.bm.vert_create(co, Some(eg));
        self.bm.verts[v].flag &= !ELEM_TAG;
        self.vms[vm].mesh[ix].v = v;
    }
    fn copy_mesh_vert(&mut self, vm: usize, ito: i32, jto: i32, kto: i32, ifrom: i32, jfrom: i32, kfrom: i32) {
        let a = self.mesh_idx(vm, ito, jto, kto);
        let b = self.mesh_idx(vm, ifrom, jfrom, kfrom);
        let nv = self.vms[vm].mesh[b];
        self.vms[vm].mesh[a] = nv;
    }
    fn find_edge_half(&self, bv: usize, bme: usize) -> usize {
        let b = &self.bvs[bv];
        for i in 0..b.edgecount {
            if self.ehs[b.e0 + i].e == bme {
                return b.e0 + i;
            }
        }
        NONE
    }
    fn find_bevvert(&self, v: usize) -> usize {
        *self.vert_hash.get(&v).unwrap_or(&NONE)
    }
    /// returns (EdgeHalf of the other end, its BevVert)
    fn find_other_end_edge_half(&self, e: usize) -> (usize, usize) {
        let eh = &self.ehs[e];
        let bme = &self.bm.edges[eh.e];
        let bvo = self.find_bevvert(if eh.is_rev { bme.v1 } else { bme.v2 });
        if bvo != NONE {
            let eother = self.find_edge_half(bvo, eh.e);
            return (eother, bvo);
        }
        (NONE, NONE)
    }
    fn bv_of_eh(&self, bv: usize, i: usize) -> usize {
        self.bvs[bv].e0 + i
    }
    fn next_bev(&self, bv: usize, from_e: usize) -> usize {
        let from = if from_e == NONE { self.bvs[bv].e0 + self.bvs[bv].edgecount - 1 } else { from_e };
        let mut e = from;
        loop {
            if self.ehs[e].is_bev {
                return e;
            }
            e = self.ehs[e].next;
            if e == from {
                return NONE;
            }
        }
    }
    fn count_ccw_edges_between(&self, e1: usize, e2: usize) -> i32 {
        let mut count = 0;
        let mut e = e1;
        loop {
            if e == e2 {
                break;
            }
            e = self.ehs[e].next;
            count += 1;
            if e == e1 {
                break;
            }
        }
        count
    }
    fn edges_face_connected_at_vert(&self, bme1: usize, bme2: usize) -> bool {
        for l in self.bm.loops_of_edge(bme1) {
            let lp = self.bm.loops[l].p;
            let ln = self.bm.loops[l].n;
            if self.bm.loops[lp].e == bme2 || self.bm.loops[ln].e == bme2 {
                return true;
            }
        }
        false
    }
    /// boundvert_rep_face
    fn boundvert_rep_face(&self, v: usize) -> usize {
        let b = &self.bounds[v];
        let mut frep;
        if b.ebev != NONE {
            frep = self.ehs[b.ebev].fprev;
        } else if b.efirst != NONE {
            frep = self.ehs[b.efirst].fprev;
            if frep == NONE {
                if self.ehs[b.efirst].fnext != NONE {
                    frep = self.ehs[b.efirst].fnext;
                } else if self.ehs[b.elast].fprev != NONE {
                    frep = self.ehs[b.elast].fprev;
                }
            }
        } else if self.bounds[b.prev].elast != NONE {
            frep = self.ehs[self.bounds[b.prev].elast].fnext;
            let nx = self.bounds[b.next].efirst;
            if nx != NONE && frep == NONE {
                frep = self.ehs[nx].fprev;
            }
        } else {
            frep = NONE;
        }
        frep
    }

    /// bev_create_ngon (no custom data: only the attribute copy from the representative face)
    fn bev_create_ngon(&mut self, verts: &[usize], face_arr: Option<&[usize]>, facerep: usize, mat_nr: i32) -> usize {
        let ex = if facerep != NONE { Some(facerep) } else { None };
        let f = self.bm.face_create_verts(verts, ex, true);
        if f == NONE {
            return NONE;
        }
        if facerep == NONE {
            if let Some(fa) = face_arr {
                if !fa.is_empty() && fa[0] != NONE {
                    self.bm.elem_attrs_copy_face(fa[0], f);
                }
            }
        }
        self.bm.faces[f].flag |= ELEM_TAG;
        if mat_nr >= 0 {
            self.bm.faces[f].mat_nr = mat_nr as i16;
        }
        f
    }
}

impl<'a> Bp<'a> {
    fn slide_dist(&self, e: usize, v: usize, d: f32) -> V3 {
        let ve = self.co(v);
        let other = self.bm.edge_other_vert(self.ehs[e].e, v);
        let mut dir = sub(ve, self.co(other));
        let (n, l) = normalize_len(dir);
        dir = n;
        let mut d = d;
        if d > l {
            d = l - (50.0 * BEVEL_EPSILON_D) as f32;
        }
        madd(ve, dir, -d)
    }
    /// returns Some(closer vertex) if co is not on the edge
    fn is_outside_edge(&self, e: usize, co: V3) -> Option<usize> {
        let be = &self.bm.edges[self.ehs[e].e];
        let l1 = self.co(be.v1);
        let u0 = sub(self.co(be.v2), l1);
        let h = sub(co, l1);
        let (u, lenu) = normalize_len(u0);
        let lambda = dot(u, h);
        if lambda <= -BEVEL_EPSILON_BIG * lenu {
            return Some(be.v1);
        }
        if lambda >= (1.0 + BEVEL_EPSILON_BIG) * lenu {
            return Some(be.v2);
        }
        None
    }
    fn edges_angle_kind(&self, e1: usize, e2: usize, v: usize) -> AngleKind {
        let v1 = self.bm.edge_other_vert(self.ehs[e1].e, v);
        let v2 = self.bm.edge_other_vert(self.ehs[e2].e, v);
        let dir1 = normalize(sub(self.co(v), self.co(v1)));
        let dir2 = normalize(sub(self.co(v), self.co(v2)));
        if Self::nearly_parallel_normalized(dir1, dir2) {
            return AngleKind::Straight;
        }
        let cr = normalize(cross(dir1, dir2));
        let no = if self.ehs[e1].fnext != NONE {
            self.fno(self.ehs[e1].fnext)
        } else if self.ehs[e2].fprev != NONE {
            self.fno(self.ehs[e2].fprev)
        } else {
            self.vno(v)
        };
        if dot(cr, no) < 0.0 {
            AngleKind::Larger
        } else {
            AngleKind::Smaller
        }
    }
    fn point_between_edges(&self, co: V3, v: usize, f: usize, e1: usize, e2: usize) -> bool {
        let v1 = self.bm.edge_other_vert(self.ehs[e1].e, v);
        let v2 = self.bm.edge_other_vert(self.ehs[e2].e, v);
        let dir1 = normalize(sub(self.co(v), self.co(v1)));
        let dir2 = normalize(sub(self.co(v), self.co(v2)));
        let dirco = normalize(sub(self.co(v), co));
        let mut ang11 = angle_normalized_v3v3(dir1, dir2);
        let mut ang1co = angle_normalized_v3v3(dir1, dirco);
        let fnv = self.fno(f);
        let no = cross(dir1, dir2);
        if dot(no, fnv) < 0.0 {
            ang11 = (std::f64::consts::PI * 2.0) as f32 - ang11;
        }
        let no = cross(dir1, dirco);
        if dot(no, fnv) < 0.0 {
            ang1co = (std::f64::consts::PI * 2.0) as f32 - ang1co;
        }
        ang11 - ang1co > -eps_ang()
    }
    fn edge_edge_angle_less_than_180(&self, e1: usize, e2: usize, f: usize) -> bool {
        let (a, b) = (&self.bm.edges[e1], &self.bm.edges[e2]);
        let (v, v1, v2);
        if a.v1 == b.v1 {
            v = a.v1;
            v1 = a.v2;
            v2 = b.v2;
        } else if a.v1 == b.v2 {
            v = a.v1;
            v1 = a.v2;
            v2 = b.v1;
        } else if a.v2 == b.v1 {
            v = a.v2;
            v1 = a.v1;
            v2 = b.v2;
        } else if a.v2 == b.v2 {
            v = a.v2;
            v1 = a.v1;
            v2 = b.v1;
        } else {
            return false;
        }
        let dir1 = sub(self.co(v1), self.co(v));
        let dir2 = sub(self.co(v2), self.co(v));
        dot(cross(dir1, dir2), self.fno(f)) > 0.0
    }

    fn offset_meet_lines_percent_or_absolute(&self, e1: usize, e2: usize, v: usize) -> (V3, V3, V3, V3) {
        let v1 = self.bm.edge_other_vert(self.ehs[e1].e, v);
        let v2 = self.bm.edge_other_vert(self.ehs[e2].e, v);
        let f1 = self.ehs[e1].fnext;
        let f2 = self.ehs[e2].fprev;
        let mut no_offsets = f1 == NONE || f2 == NONE;
        let (mut l1a, mut l1b, mut l2a, mut l2b) = (self.co(v), self.co(v1), self.co(v), self.co(v2));
        if !no_offsets {
            let l = self.bm.face_vert_share_loop(f1, v1);
            let e0 = self.bm.loops[l].e;
            let l = self.bm.face_vert_share_loop(f2, v2);
            let e3 = self.bm.loops[self.bm.loops[l].p].e;
            let l = self.bm.face_vert_share_loop(f1, v);
            let e4 = self.bm.loops[self.bm.loops[l].p].e;
            let l = self.bm.face_vert_share_loop(f2, v);
            let e5 = self.bm.loops[l].e;
            no_offsets = !self.edge_edge_angle_less_than_180(e0, self.ehs[e1].e, f1)
                || !self.edge_edge_angle_less_than_180(self.ehs[e1].e, e4, f1)
                || !self.edge_edge_angle_less_than_180(self.ehs[e2].e, e3, f2)
                || !self.edge_edge_angle_less_than_180(e5, self.ehs[e2].e, f1);
            if !no_offsets {
                let (d0, d3, d4, d5);
                if self.offset_type == OffsetType::Absolute {
                    d0 = self.offset;
                    d3 = self.offset;
                    d4 = self.offset;
                    d5 = self.offset;
                } else {
                    d0 = self.offset * self.bm.edge_calc_length(e0) / 100.0;
                    d3 = self.offset * self.bm.edge_calc_length(e3) / 100.0;
                    d4 = self.offset * self.bm.edge_calc_length(e4) / 100.0;
                    d5 = self.offset * self.bm.edge_calc_length(e5) / 100.0;
                }
                let (e1_wt, e2_wt) = (1.0f32, 1.0f32);
                // slide_dist uses an EdgeHalf only for its edge; emulate with raw edges
                l1a = self.slide_dist_edge(e4, v, d4 * e1_wt);
                l1b = self.slide_dist_edge(e0, v1, d0 * e1_wt);
                l2a = self.slide_dist_edge(e5, v, d5 * e2_wt);
                l2b = self.slide_dist_edge(e3, v2, d3 * e2_wt);
            }
        }
        (l1a, l1b, l2a, l2b)
    }
    fn slide_dist_edge(&self, bme: usize, v: usize, d: f32) -> V3 {
        let ve = self.co(v);
        let other = self.bm.edge_other_vert(bme, v);
        let (dir, l) = normalize_len(sub(ve, self.co(other)));
        let mut d = d;
        if d > l {
            d = l - (50.0 * BEVEL_EPSILON_D) as f32;
        }
        madd(ve, dir, -d)
    }

    /// offset_meet: returns meetco
    fn offset_meet(&self, e1: usize, e2: usize, v: usize, f: usize, edges_between: bool, e_in_plane: usize) -> V3 {
        let ve = self.co(v);
        let dir1 = sub(ve, self.co(self.bm.edge_other_vert(self.ehs[e1].e, v)));
        let dir2 = sub(self.co(self.bm.edge_other_vert(self.ehs[e2].e, v)), ve);
        let (dir1n, dir2p);
        if edges_between {
            let e1next = self.ehs[e1].next;
            let e2prev = self.ehs[e2].prev;
            dir1n = sub(self.co(self.bm.edge_other_vert(self.ehs[e1next].e, v)), ve);
            dir2p = sub(ve, self.co(self.bm.edge_other_vert(self.ehs[e2prev].e, v)));
        } else {
            dir1n = [0.0; 3];
            dir2p = [0.0; 3];
        }
        let mut ang = angle_v3v3(dir1, dir2);
        let mut meetco;
        if ang < eps_ang() {
            // e1 and e2 are parallel
            let norm_v;
            if f != NONE {
                norm_v = self.fno(f);
            } else {
                let mut fcount = 0;
                let mut nv = [0.0f32; 3];
                let mut eloop = e1;
                while eloop != e2 {
                    if self.ehs[eloop].fnext != NONE {
                        nv = add(nv, self.fno(self.ehs[eloop].fnext));
                        fcount += 1;
                    }
                    eloop = self.ehs[eloop].next;
                }
                if fcount == 0 {
                    norm_v = self.vno(v);
                } else {
                    norm_v = mul(nv, 1.0 / fcount as f32);
                }
            }
            let d1 = add(dir1, dir2);
            let norm_perp1 = normalize(cross(d1, norm_v));
            let mut off1a = ve;
            let mut d = self.ehs[e1].offset_r.max(self.ehs[e2].offset_l);
            d = d / (ang / 2.0).cos();
            off1a = madd(off1a, norm_perp1, d);
            meetco = off1a;
        } else if (ang - PI).abs() < eps_ang() {
            let d = self.ehs[e1].offset_r.max(self.ehs[e2].offset_l);
            meetco = self.slide_dist(e2, v, d);
        } else {
            let (norm_v1, norm_v2);
            if f != NONE && ang < small_ang() {
                norm_v1 = self.fno(f);
                norm_v2 = self.fno(f);
            } else if !edges_between {
                let mut n1 = normalize(cross(dir2, dir1));
                let ref_no = if f != NONE { self.fno(f) } else { self.vno(v) };
                if dot(n1, ref_no) < 0.0 {
                    n1 = neg(n1);
                }
                norm_v1 = n1;
                norm_v2 = n1;
            } else {
                let mut n1 = normalize(cross(dir1n, dir1));
                let f1 = self.ehs[e1].fnext;
                let ref1 = if f1 != NONE { self.fno(f1) } else { self.vno(v) };
                if dot(n1, ref1) < 0.0 {
                    n1 = neg(n1);
                }
                let mut n2 = normalize(cross(dir2, dir2p));
                let f2 = self.ehs[e2].fprev;
                let ref2 = if f2 != NONE { self.fno(f2) } else { self.vno(v) };
                if dot(n2, ref2) < 0.0 {
                    n2 = neg(n2);
                }
                norm_v1 = n1;
                norm_v2 = n2;
            }
            let norm_perp1 = normalize(cross(dir1, norm_v1));
            let norm_perp2 = normalize(cross(dir2, norm_v2));
            let (off1a, off1b, off2a, off2b);
            if matches!(self.offset_type, OffsetType::Percent | OffsetType::Absolute) {
                let (a, b, c, d) = self.offset_meet_lines_percent_or_absolute(e1, e2, v);
                off1a = a;
                off1b = b;
                off2a = c;
                off2b = d;
            } else {
                off1a = madd(ve, norm_perp1, self.ehs[e1].offset_r);
                off1b = add(off1a, dir1);
                off2a = madd(ve, norm_perp2, self.ehs[e2].offset_l);
                off2b = add(off2a, dir2);
            }
            let (isect_kind, m, isect2) = isect_line_line(off1a, off1b, off2a, off2b);
            meetco = m;
            if isect_kind == 0 {
                meetco = off1a;
            } else {
                if self.ehs[e1].offset_r == 0.0 {
                    if let Some(cv) = self.is_outside_edge(e1, meetco) {
                        meetco = self.co(cv);
                    }
                }
                if self.ehs[e2].offset_l == 0.0 {
                    if let Some(cv) = self.is_outside_edge(e2, meetco) {
                        meetco = self.co(cv);
                    }
                }
                if edges_between && self.ehs[e1].offset_r > 0.0 && self.ehs[e2].offset_l > 0.0 {
                    if isect_kind == 2 {
                        meetco = mid(meetco, isect2);
                    }
                    let mut e = e1;
                    while e != e2 {
                        let fnext = self.ehs[e].fnext;
                        if fnext == NONE {
                            e = self.ehs[e].next;
                            continue;
                        }
                        let plane = plane_from_point_normal(ve, self.fno(fnext));
                        let dropco = closest_to_plane_normalized(plane, meetco);
                        if e_in_plane != NONE {
                            ang = angle_v3v3(self.fno(fnext), self.fno(self.ehs[e_in_plane].fnext));
                            if ang.abs() < small_ang() || (ang - PI).abs() < small_ang() {
                                e = self.ehs[e].next;
                                continue;
                            }
                        }
                        if self.point_between_edges(dropco, v, fnext, e, self.ehs[e].next) {
                            meetco = dropco;
                            break;
                        }
                        e = self.ehs[e].next;
                    }
                }
            }
        }
        meetco
    }

    /// offset_meet_edge: Some(meetco) if a good meet point exists; always returns the angle
    fn offset_meet_edge(&self, e1: usize, e2: usize, v: usize) -> (Option<V3>, f32) {
        let ve = self.co(v);
        let dir1 = normalize(sub(self.co(self.bm.edge_other_vert(self.ehs[e1].e, v)), ve));
        let dir2 = normalize(sub(self.co(self.bm.edge_other_vert(self.ehs[e2].e, v)), ve));
        let mut ang = angle_normalized_v3v3(dir1, dir2);
        let good = 0.1f32;
        if ang.abs() < good {
            return (None, 0.0);
        }
        let fno = cross(dir1, dir2);
        if dot(fno, self.vno(v)) < 0.0 {
            ang = 2.0 * PI - ang;
            return (None, ang);
        }
        if (ang - PI).abs() < good {
            return (None, ang);
        }
        let sinang = ang.sin();
        let mut meetco = ve;
        if self.ehs[e1].offset_r == 0.0 {
            meetco = madd(meetco, dir1, self.ehs[e2].offset_l / sinang);
        } else {
            meetco = madd(meetco, dir2, self.ehs[e1].offset_r / sinang);
        }
        (Some(meetco), ang)
    }
    fn good_offset_on_edge_between(&self, e1: usize, e2: usize, emid: usize, v: usize) -> bool {
        self.offset_meet_edge(e1, emid, v).0.is_some() && self.offset_meet_edge(emid, e2, v).0.is_some()
    }
    /// returns (retval, meetco, sinratio)
    fn offset_on_edge_between(&self, e1: usize, e2: usize, emid: usize, v: usize) -> (bool, V3, f32) {
        let mut retval = false;
        let (m1, ang1) = self.offset_meet_edge(e1, emid, v);
        let (m2, ang2) = self.offset_meet_edge(emid, e2, v);
        let mut sinratio = 1.0f32;
        let meetco;
        if matches!(self.offset_type, OffsetType::Percent | OffsetType::Absolute) {
            let v2 = self.bm.edge_other_vert(self.ehs[emid].e, v);
            if self.offset_type == OffsetType::Percent {
                meetco = interp(self.co(v), self.co(v2), self.offset / 100.0);
            } else {
                let dir = normalize(sub(self.co(v2), self.co(v)));
                meetco = madd(self.co(v), dir, self.offset);
            }
            sinratio = if ang1 == 0.0 { 1.0 } else { ang2.sin() / ang1.sin() };
            return (true, meetco, sinratio);
        }
        match (m1, m2) {
            (Some(a), Some(b)) => {
                meetco = mid(a, b);
                sinratio = if ang1 == 0.0 { 1.0 } else { ang2.sin() / ang1.sin() };
                retval = true;
            }
            (Some(a), None) => meetco = a,
            (None, Some(b)) => meetco = b,
            (None, None) => meetco = self.slide_dist(emid, v, self.ehs[e1].offset_r),
        }
        (retval, meetco, sinratio)
    }
    fn offset_in_plane(&self, e: usize, plane_no: Option<V3>, left: bool) -> V3 {
        let eh = &self.ehs[e];
        let be = &self.bm.edges[eh.e];
        let v = if eh.is_rev { be.v2 } else { be.v1 };
        let dir = normalize(sub(self.co(self.bm.edge_other_vert(eh.e, v)), self.co(v)));
        let no = match plane_no {
            Some(p) => p,
            None => {
                let mut n = [0.0f32; 3];
                if dir[0].abs() < dir[1].abs() {
                    n[0] = 1.0;
                } else {
                    n[1] = 1.0;
                }
                n
            }
        };
        let fdir = if left { normalize(cross(dir, no)) } else { normalize(cross(no, dir)) };
        madd(self.co(v), fdir, if left { eh.offset_l } else { eh.offset_r })
    }
    fn project_to_edge(&self, bme: usize, co_a: V3, co_b: V3) -> V3 {
        let e = &self.bm.edges[bme];
        let (k, p, _) = isect_line_line(self.co(e.v1), self.co(e.v2), co_a, co_b);
        if k == 0 {
            self.co(e.v1)
        } else {
            p
        }
    }
}

fn make_unit_square_map(va: V3, vmid: V3, vb: V3) -> Option<M4> {
    let va_vmid = sub(vmid, va);
    let vb_vmid = sub(vmid, vb);
    if is_zero(va_vmid) || is_zero(vb_vmid) {
        return None;
    }
    if (angle_v3v3(va_vmid, vb_vmid) - PI).abs() <= eps_ang() {
        return None;
    }
    let vo = sub(va, vb_vmid);
    let vddir = normalize(cross(vb_vmid, va_vmid));
    let vd = add(vo, vddir);
    let mut m = [[0.0f32; 4]; 4];
    let r0 = sub(vmid, va);
    m[0] = [r0[0], r0[1], r0[2], 0.0];
    let r1 = sub(vmid, vb);
    m[1] = [r1[0], r1[1], r1[2], 0.0];
    let mut r2 = add(vmid, vd);
    r2 = sub(r2, va);
    r2 = sub(r2, vb);
    m[2] = [r2[0], r2[1], r2[2], 0.0];
    let mut r3 = add(va, vb);
    r3 = sub(r3, vmid);
    m[3] = [r3[0], r3[1], r3[2], 1.0];
    Some(m)
}

fn make_unit_cube_map(va: V3, vb: V3, vc: V3, vd: V3) -> M4 {
    let mut m = [[0.0f32; 4]; 4];
    let mut r = va;
    r = sub(r, vb);
    r = sub(r, vc);
    r = add(r, vd);
    r = mul(r, 0.5);
    m[0] = [r[0], r[1], r[2], 0.0];
    let mut r = vb;
    r = sub(r, va);
    r = sub(r, vc);
    r = add(r, vd);
    r = mul(r, 0.5);
    m[1] = [r[0], r[1], r[2], 0.0];
    let mut r = vc;
    r = sub(r, va);
    r = sub(r, vb);
    r = add(r, vd);
    r = mul(r, 0.5);
    m[2] = [r[0], r[1], r[2], 0.0];
    let mut r = va;
    r = add(r, vb);
    r = add(r, vc);
    r = sub(r, vd);
    r = mul(r, 0.5);
    m[3] = [r[0], r[1], r[2], 1.0];
    m
}

fn superellipse_co(x: f64, r: f32, rbig: bool) -> f64 {
    let r = r as f64;
    if rbig {
        return (1.0 - x.powf(r)).powf(1.0 / r);
    }
    1.0 - (1.0 - (1.0 - x).powf(r)).powf(1.0 / r)
}

fn snap_to_superellipsoid(co: V3, super_r: f32, midline: bool) -> V3 {
    let r = super_r;
    if r == PRO_CIRCLE_R {
        return normalize(co);
    }
    let a = co[0].max(0.0);
    let b = co[1].max(0.0);
    let c = co[2].max(0.0);
    let (mut x, mut y, mut z) = (a, b, c);
    if r == PRO_SQUARE_R || r == PRO_SQUARE_IN_R {
        z = 0.0;
        x = x.min(1.0);
        y = y.min(1.0);
        if r == PRO_SQUARE_R {
            let dx = 1.0 - x;
            let dy = 1.0 - y;
            if dx < dy {
                x = 1.0;
                y = if midline { 1.0 } else { y };
            } else {
                y = 1.0;
                x = if midline { 1.0 } else { x };
            }
        } else if x < y {
            x = 0.0;
            y = if midline { 0.0 } else { y };
        } else {
            y = 0.0;
            x = if midline { 0.0 } else { x };
        }
    } else {
        let rinv = 1.0f32 / r;
        if a == 0.0 {
            if b == 0.0 {
                x = 0.0;
                y = 0.0;
                z = c.powf(rinv);
            } else {
                x = 0.0;
                y = (1.0f32 / (1.0 + (c / b).powf(r))).powf(rinv);
                z = c * y / b;
            }
        } else {
            x = (1.0f32 / (1.0 + (b / a).powf(r) + (c / a).powf(r))).powf(rinv);
            y = b * x / a;
            z = c * x / a;
        }
    }
    [x, y, z]
}

impl<'a> Bp<'a> {
    fn set_profile_params(&mut self, bv: usize, bndv: usize) {
        let mut do_linear_interp = true;
        let e = self.bounds[bndv].ebev;
        let start = self.bounds[bndv].nv.co;
        let nxt = self.bounds[bndv].next;
        let end = self.bounds[nxt].nv.co;
        let mut pro = self.bounds[bndv].profile.clone();
        if e != NONE {
            do_linear_interp = false;
            pro.super_r = self.pro_super_r;
            let be = self.ehs[e].e;
            let (v1, v2) = (self.bm.edges[be].v1, self.bm.edges[be].v2);
            pro.proj_dir = sub(self.co(v1), self.co(v2));
            if self.ehs[e].is_rev {
                pro.proj_dir = neg(pro.proj_dir);
            }
            pro.proj_dir = normalize(pro.proj_dir);
            pro.middle = self.project_to_edge(be, start, end);
            pro.start = start;
            pro.end = end;
            let mut d1 = normalize(sub(pro.middle, start));
            let mut d2 = normalize(sub(pro.middle, end));
            pro.plane_no = normalize(cross(d1, d2));
            if Self::nearly_parallel(d1, d2) {
                let bvv = self.bvs[bv].v;
                pro.middle = self.co(bvv);
                let (eprev, enext) = (self.ehs[e].prev, self.ehs[e].next);
                if self.ehs[eprev].is_bev && self.ehs[enext].is_bev && self.bvs[bv].selcount >= 3 {
                    let pe = &self.bm.edges[self.ehs[eprev].e];
                    let ne = &self.bm.edges[self.ehs[enext].e];
                    let d3 = normalize(sub(self.co(pe.v1), self.co(pe.v2)));
                    let d4 = normalize(sub(self.co(ne.v1), self.co(ne.v2)));
                    if Self::nearly_parallel(d3, d4) {
                        pro.middle = mid(start, end);
                        do_linear_interp = true;
                    } else {
                        let co3 = add(start, d3);
                        let co4 = add(end, d4);
                        let (k, meetco, _) = isect_line_line(start, co3, end, co4);
                        if k != 0 {
                            pro.middle = meetco;
                        } else {
                            pro.middle = mid(start, end);
                            do_linear_interp = true;
                        }
                    }
                }
                pro.end = end;
                d1 = normalize(sub(pro.middle, start));
                d2 = normalize(sub(pro.middle, end));
                pro.plane_no = normalize(cross(d1, d2));
                if Self::nearly_parallel(d1, d2) {
                    do_linear_interp = true;
                } else {
                    pro.plane_co = self.co(bvv);
                    pro.proj_dir = pro.plane_no;
                }
            }
            pro.plane_co = start;
        } else if self.bounds[bndv].is_arc_start {
            pro.start = start;
            pro.end = end;
            pro.super_r = PRO_CIRCLE_R;
            pro.plane_co = [0.0; 3];
            pro.plane_no = [0.0; 3];
            pro.proj_dir = [0.0; 3];
            do_linear_interp = false;
        } else if self.vertex_only {
            pro.start = start;
            pro.middle = self.co(self.bvs[bv].v);
            pro.end = end;
            pro.super_r = self.pro_super_r;
            pro.plane_co = [0.0; 3];
            pro.plane_no = [0.0; 3];
            pro.proj_dir = [0.0; 3];
            do_linear_interp = false;
        }
        if do_linear_interp {
            pro.super_r = PRO_LINE_R;
            pro.start = start;
            pro.end = end;
            pro.middle = mid(start, end);
            pro.plane_co = [0.0; 3];
            pro.plane_no = [0.0; 3];
            pro.proj_dir = [0.0; 3];
        }
        self.bounds[bndv].profile = pro;
    }

    fn move_profile_plane(&mut self, bndv: usize, bmvert: usize) {
        let pro = self.bounds[bndv].profile.clone();
        if is_zero(pro.proj_dir) {
            return;
        }
        let d1 = normalize(sub(self.co(bmvert), pro.start));
        let d2 = normalize(sub(self.co(bmvert), pro.end));
        let (no, l1) = normalize_len(cross(d1, d2));
        let (no2, l2) = normalize_len(cross(d1, pro.proj_dir));
        let (no3, l3) = normalize_len(cross(d2, pro.proj_dir));
        if l1 > BEVEL_EPSILON_BIG && l2 > BEVEL_EPSILON_BIG && l3 > BEVEL_EPSILON_BIG {
            let dot2 = dot(no, no2);
            let dot3 = dot(no, no3);
            if dot2.abs() < (1.0 - BEVEL_EPSILON_BIG) && dot3.abs() < (1.0 - BEVEL_EPSILON_BIG) {
                self.bounds[bndv].profile.plane_no = no;
            }
        }
        self.bounds[bndv].profile.special_params = true;
    }

    fn move_weld_profile_planes(&mut self, bv: usize, b1: usize, b2: usize) {
        let (p1, p2) = (self.bounds[b1].profile.clone(), self.bounds[b2].profile.clone());
        if is_zero(p1.proj_dir) || is_zero(p2.proj_dir) {
            return;
        }
        let bvv = self.co(self.bvs[bv].v);
        let d1 = sub(bvv, self.bounds[b1].nv.co);
        let d2 = sub(bvv, self.bounds[b2].nv.co);
        let (no, l1) = normalize_len(cross(d1, d2));
        let (no2, l2) = normalize_len(cross(d1, p1.proj_dir));
        let (no3, l3) = normalize_len(cross(d2, p2.proj_dir));
        if l1 > BEVEL_EPSILON && (l2 > BEVEL_EPSILON || l3 > BEVEL_EPSILON) {
            let dot1 = dot(no, no2).abs();
            let dot2 = dot(no, no3).abs();
            if (dot1 - 1.0).abs() > BEVEL_EPSILON {
                self.bounds[b1].profile.plane_no = no;
            }
            if (dot2 - 1.0).abs() > BEVEL_EPSILON {
                self.bounds[b2].profile.plane_no = no;
            }
        }
        self.bounds[b1].profile.special_params = true;
        self.bounds[b2].profile.special_params = true;
    }

    fn bev_ccw_test(&self, a: usize, b: usize, f: usize) -> i32 {
        if f == NONE {
            return 0;
        }
        let la = self.bm.face_edge_share_loop(f, a);
        let lb = self.bm.face_edge_share_loop(f, b);
        if la == NONE || lb == NONE {
            return 0;
        }
        if self.bm.loops[lb].n == la {
            1
        } else {
            -1
        }
    }

    fn get_profile_point(&self, pro: &Profile, i: i32, nseg: i32) -> V3 {
        if self.seg == 1 {
            return if i == 0 { pro.start } else { pro.end };
        }
        let get = |v: &Vec<f32>, idx: usize| -> V3 { [v[3 * idx], v[3 * idx + 1], v[3 * idx + 2]] };
        if nseg == self.seg {
            get(&pro.prof_co, i as usize)
        } else {
            let sub_sp = self.pro_spacing.seg_2 / nseg;
            let src = if pro.prof_co_2.is_empty() { &pro.prof_co } else { &pro.prof_co_2 };
            get(src, (i * sub_sp) as usize)
        }
    }

    fn calculate_profile_segments(&self, profile: &Profile, map: &M4, use_map: bool, reversed: bool, ns: i32, xvals: &[f64], yvals: &[f64]) -> Vec<f32> {
        let mut out = vec![0.0f32; 3 * (ns as usize + 1)];
        for k in 0..=ns {
            let co: V3;
            if k == 0 {
                co = profile.start;
            } else if k == ns {
                co = profile.end;
            } else if use_map {
                let ku = k as usize;
                let p = [
                    if reversed { yvals[(ns - k) as usize] as f32 } else { xvals[ku] as f32 },
                    if reversed { xvals[(ns - k) as usize] as f32 } else { yvals[ku] as f32 },
                    0.0,
                ];
                co = mul_v3_m4v3(map, p);
            } else {
                co = interp(profile.start, profile.end, k as f32 / ns as f32);
            }
            let mut res = co;
            if !is_zero(profile.proj_dir) {
                let co2 = add(co, profile.proj_dir);
                res = match isect_line_plane(co, co2, profile.plane_co, profile.plane_no) {
                    Some(p) => p,
                    None => co,
                };
            }
            out[3 * k as usize] = res[0];
            out[3 * k as usize + 1] = res[1];
            out[3 * k as usize + 2] = res[2];
        }
        out
    }

    fn calculate_profile(&mut self, bndv: usize, reversed: bool) {
        if self.seg == 1 {
            return;
        }
        let pro = self.bounds[bndv].profile.clone();
        let need_2 = self.seg != self.pro_spacing.seg_2;
        let (use_map, map);
        if pro.super_r == PRO_LINE_R {
            use_map = false;
            map = [[0.0; 4]; 4];
        } else {
            match make_unit_square_map(pro.start, pro.middle, pro.end) {
                Some(m) => {
                    use_map = true;
                    map = m;
                }
                None => {
                    use_map = false;
                    map = [[0.0; 4]; 4];
                }
            }
        }
        let ps = self.pro_spacing.clone();
        let c1 = self.calculate_profile_segments(&pro, &map, use_map, reversed, self.seg, &ps.xvals, &ps.yvals);
        let c2 = if need_2 { self.calculate_profile_segments(&pro, &map, use_map, reversed, ps.seg_2, &ps.xvals_2, &ps.yvals_2) } else { vec![] };
        let p = &mut self.bounds[bndv].profile;
        p.prof_co = c1;
        p.prof_co_2 = c2;
    }

    fn eh_on_plane(&self, e: usize) -> bool {
        let eh = &self.ehs[e];
        if eh.fprev != NONE && eh.fnext != NONE {
            let d = dot(self.fno(eh.fprev), self.fno(eh.fnext));
            if (d + 1.0).abs() <= BEVEL_EPSILON_BIG || (d - 1.0).abs() <= BEVEL_EPSILON_BIG {
                return true;
            }
        }
        false
    }

    fn calculate_vm_profiles(&mut self, bv: usize, vm: usize) {
        let start = self.vms[vm].boundstart;
        let mut bndv = start;
        loop {
            if !self.bounds[bndv].profile.special_params {
                self.set_profile_params(bv, bndv);
            }
            self.calculate_profile(bndv, false);
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
    }
}

impl<'a> Bp<'a> {
    fn new_vmesh(&mut self, count: i32, seg: i32, bounds: usize) -> usize {
        let n = (count * (1 + seg / 2) * (1 + seg)) as usize;
        self.vms.push(VMesh { mesh: vec![NewVert { v: NONE, co: [0.0; 3] }; n], boundstart: bounds, count, seg, mesh_kind: MeshKind::Adj });
        self.vms.len() - 1
    }

    fn build_boundary_terminal_edge(&mut self, bv: usize, efirst: usize, construct: bool) {
        let vm = self.bvs[bv].vmesh;
        let bvv = self.bvs[bv].v;
        let mut e = efirst;
        let edgecount = self.bvs[bv].edgecount;
        if edgecount == 2 {
            let no = if self.ehs[e].fprev != NONE {
                Some(self.fno(self.ehs[e].fprev))
            } else if self.ehs[e].fnext != NONE {
                Some(self.fno(self.ehs[e].fnext))
            } else {
                None
            };
            let co = self.offset_in_plane(e, no, true);
            if construct {
                let bndv = self.add_new_bound_vert(vm, co);
                self.bounds[bndv].efirst = e;
                self.bounds[bndv].elast = e;
                self.bounds[bndv].ebev = e;
                self.ehs[e].leftv = bndv;
            } else {
                let lv = self.ehs[e].leftv;
                self.adjust_bound_vert(lv, co);
            }
            let no = if self.ehs[e].fnext != NONE {
                Some(self.fno(self.ehs[e].fnext))
            } else if self.ehs[e].fprev != NONE {
                Some(self.fno(self.ehs[e].fprev))
            } else {
                None
            };
            let co = self.offset_in_plane(e, no, false);
            if construct {
                let bndv = self.add_new_bound_vert(vm, co);
                self.bounds[bndv].efirst = e;
                self.bounds[bndv].elast = e;
                self.ehs[e].rightv = bndv;
            } else {
                let rv = self.ehs[e].rightv;
                self.adjust_bound_vert(rv, co);
            }
            let en = self.ehs[e].next;
            let co = self.slide_dist(en, bvv, self.ehs[e].offset_l);
            if construct {
                let bndv = self.add_new_bound_vert(vm, co);
                self.bounds[bndv].efirst = en;
                self.bounds[bndv].elast = en;
                self.ehs[en].leftv = bndv;
                self.ehs[en].rightv = bndv;
            } else {
                let lv = self.ehs[en].leftv;
                self.adjust_bound_vert(lv, co);
            }
        } else {
            let leg_slide = matches!(self.offset_type, OffsetType::Percent | OffsetType::Absolute);
            let ep = self.ehs[e].prev;
            let co = if leg_slide { self.slide_dist(ep, bvv, self.ehs[e].offset_l) } else { self.offset_meet(ep, e, bvv, self.ehs[e].fprev, false, NONE) };
            if construct {
                let bndv = self.add_new_bound_vert(vm, co);
                self.bounds[bndv].efirst = ep;
                self.bounds[bndv].elast = e;
                self.bounds[bndv].ebev = e;
                self.ehs[e].leftv = bndv;
                self.ehs[ep].leftv = bndv;
                self.ehs[ep].rightv = bndv;
            } else {
                let lv = self.ehs[e].leftv;
                self.adjust_bound_vert(lv, co);
            }
            e = self.ehs[e].next;
            let ep = self.ehs[e].prev;
            let co = if leg_slide { self.slide_dist(e, bvv, self.ehs[ep].offset_r) } else { self.offset_meet(ep, e, bvv, self.ehs[e].fprev, false, NONE) };
            if construct {
                let bndv = self.add_new_bound_vert(vm, co);
                self.bounds[bndv].efirst = ep;
                self.bounds[bndv].elast = e;
                self.ehs[e].leftv = bndv;
                self.ehs[e].rightv = bndv;
                self.ehs[ep].rightv = bndv;
            } else {
                let lv = self.ehs[e].leftv;
                self.adjust_bound_vert(lv, co);
            }
            let d = self.ehs[efirst].offset_l_spec;
            e = self.ehs[e].next;
            while self.ehs[e].next != efirst {
                let co = self.slide_dist(e, bvv, d);
                if construct {
                    let bndv = self.add_new_bound_vert(vm, co);
                    self.bounds[bndv].efirst = e;
                    self.bounds[bndv].elast = e;
                    self.ehs[e].leftv = bndv;
                    self.ehs[e].rightv = bndv;
                } else {
                    let lv = self.ehs[e].leftv;
                    self.adjust_bound_vert(lv, co);
                }
                e = self.ehs[e].next;
            }
        }
        if edgecount >= 3 {
            let bndv = self.vms[vm].boundstart;
            self.set_profile_params(bv, bndv);
            self.move_profile_plane(bndv, bvv);
        }
        if construct {
            let count = self.vms[vm].count;
            if count == 2 && edgecount == 3 {
                self.vms[vm].mesh_kind = MeshKind::None;
            } else if count == 3 {
                self.vms[vm].mesh_kind = MeshKind::TriFan;
            } else {
                self.vms[vm].mesh_kind = MeshKind::Poly;
            }
        }
    }

    /// build_boundary_vertex_only
    fn build_boundary_vertex_only(&mut self, bv: usize, construct: bool) {
        let vm = self.bvs[bv].vmesh;
        let bvv = self.bvs[bv].v;
        let efirst = self.bvs[bv].e0;
        let mut e = efirst;
        loop {
            let co = self.slide_dist(e, bvv, self.ehs[e].offset_l);
            if construct {
                let v = self.add_new_bound_vert(vm, co);
                self.bounds[v].efirst = e;
                self.bounds[v].elast = e;
                self.ehs[e].leftv = v;
                self.ehs[e].rightv = v;
            } else {
                let lv = self.ehs[e].leftv;
                self.adjust_bound_vert(lv, co);
            }
            e = self.ehs[e].next;
            if e == efirst {
                break;
            }
        }
        if construct {
            // set_bound_vert_seams only matters for mark_seam / mark_sharp (not ported)
            if self.vms[vm].count == 2 {
                self.vms[vm].mesh_kind = MeshKind::None;
            } else if self.seg == 1 {
                self.vms[vm].mesh_kind = MeshKind::Poly;
            } else {
                self.vms[vm].mesh_kind = MeshKind::Adj;
            }
        }
    }

    fn build_boundary(&mut self, bv: usize, construct: bool) {
        if self.bvs[bv].edgecount <= 1 {
            return;
        }
        if self.vertex_only {
            self.build_boundary_vertex_only(bv, construct);
            return;
        }
        let vm = self.bvs[bv].vmesh;
        let bvv = self.bvs[bv].v;
        let efirst = self.next_bev(bv, NONE);
        if self.bvs[bv].selcount == 1 {
            self.build_boundary_terminal_edge(bv, efirst, construct);
            return;
        }
        let mut e = efirst;
        loop {
            let mut eon = NONE;
            let mut in_plane = 0;
            let mut not_in_plane = 0;
            let mut enip = NONE;
            let mut eip = NONE;
            let mut e2 = self.ehs[e].next;
            while !self.ehs[e2].is_bev {
                if self.eh_on_plane(e2) {
                    in_plane += 1;
                    eip = e2;
                } else {
                    not_in_plane += 1;
                    enip = e2;
                }
                e2 = self.ehs[e2].next;
            }
            let mut r = 1.0f32;
            let co;
            if in_plane == 0 && not_in_plane == 0 {
                co = self.offset_meet(e, e2, bvv, self.ehs[e].fnext, false, NONE);
            } else if not_in_plane > 0 {
                if self.loop_slide && not_in_plane == 1 && self.good_offset_on_edge_between(e, e2, enip, bvv) {
                    let (ok, c, rr) = self.offset_on_edge_between(e, e2, enip, bvv);
                    co = c;
                    r = rr;
                    if ok {
                        eon = enip;
                    }
                } else {
                    co = self.offset_meet(e, e2, bvv, NONE, true, eip);
                }
            } else if self.loop_slide && in_plane == 1 && self.good_offset_on_edge_between(e, e2, eip, bvv) {
                let (ok, c, rr) = self.offset_on_edge_between(e, e2, eip, bvv);
                co = c;
                r = rr;
                if ok {
                    eon = eip;
                }
            } else {
                co = self.offset_meet(e, e2, bvv, self.ehs[e].fnext, false, NONE);
            }
            if construct {
                let v = self.add_new_bound_vert(vm, co);
                self.bounds[v].efirst = e;
                self.bounds[v].elast = e2;
                self.bounds[v].ebev = e2;
                self.bounds[v].eon = eon;
                if eon != NONE {
                    self.bounds[v].sinratio = r;
                }
                self.ehs[e].rightv = v;
                self.ehs[e2].leftv = v;
                let mut e3 = self.ehs[e].next;
                while e3 != e2 {
                    self.ehs[e3].leftv = v;
                    self.ehs[e3].rightv = v;
                    e3 = self.ehs[e3].next;
                }
            } else {
                let rv = self.ehs[e].rightv;
                self.adjust_bound_vert(rv, co);
            }
            e = e2;
            if e == efirst {
                break;
            }
        }
        if construct {
            let count = self.vms[vm].count;
            if count == 2 {
                self.vms[vm].mesh_kind = MeshKind::None;
            } else if self.ehs[efirst].seg == 1 {
                self.vms[vm].mesh_kind = MeshKind::Poly;
            } else {
                self.vms[vm].mesh_kind = MeshKind::Adj;
            }
        }
    }
}

/// dense normal-equations least squares (stands in for Eigen's SparseLU on A^T A)
fn solve_least_squares(nrows: usize, ncols: usize, entries: &[(usize, usize, f64)], rhs: &[f64]) -> Vec<f64> {
    let mut a = vec![vec![0.0f64; ncols]; nrows];
    for &(r, c, v) in entries {
        a[r][c] += v;
    }
    let mut ata = vec![vec![0.0f64; ncols + 1]; ncols];
    for i in 0..ncols {
        for j in 0..ncols {
            let mut s = 0.0;
            for k in 0..nrows {
                s += a[k][i] * a[k][j];
            }
            ata[i][j] = s;
        }
        let mut s = 0.0;
        for k in 0..nrows {
            s += a[k][i] * rhs[k];
        }
        ata[i][ncols] = s;
    }
    // gaussian elimination with partial pivoting
    for col in 0..ncols {
        let mut piv = col;
        for r in col + 1..ncols {
            if ata[r][col].abs() > ata[piv][col].abs() {
                piv = r;
            }
        }
        ata.swap(col, piv);
        let d = ata[col][col];
        if d == 0.0 {
            continue;
        }
        for r in col + 1..ncols {
            let f = ata[r][col] / d;
            for c in col..=ncols {
                let t = ata[col][c];
                ata[r][c] -= f * t;
            }
        }
    }
    let mut x = vec![0.0f64; ncols];
    for i in (0..ncols).rev() {
        let mut s = ata[i][ncols];
        for j in i + 1..ncols {
            s -= ata[i][j] * x[j];
        }
        x[i] = if ata[i][i] != 0.0 { s / ata[i][i] } else { 0.0 };
    }
    x
}

impl<'a> Bp<'a> {
    fn adjust_the_cycle_or_chain(&mut self, vstart: usize, iscycle: bool) {
        let mut np = 0usize;
        let mut v = vstart;
        loop {
            np += 1;
            v = self.bounds[v].adjchain;
            if v == NONE || v == vstart {
                break;
            }
        }
        let nrows = if iscycle { 3 * np } else { 3 * np - 3 };
        let mut entries: Vec<(usize, usize, f64)> = vec![];
        let mut rhs = vec![0.0f64; nrows];
        let weight = BEVEL_MATCH_SPEC_WEIGHT;
        let mut v = vstart;
        let mut i = 0usize;
        loop {
            if iscycle || i < np - 1 {
                let eright = self.bounds[v].efirst;
                let adj = self.bounds[v].adjchain;
                let enextleft = self.bounds[adj].elast;
                entries.push((i, i, 1.0));
                rhs[i] += 0.0;
                if iscycle {
                    entries.push((if i > 0 { i - 1 } else { np - 1 }, i, -(self.bounds[v].sinratio as f64)));
                } else if i > 0 {
                    entries.push((i - 1, i, -(self.bounds[v].sinratio as f64)));
                }
                let row = if iscycle { np + 2 * i } else { np - 1 + 2 * i };
                entries.push((row, i, weight));
                rhs[row] += weight * self.ehs[eright].offset_r as f64;
                let row = row + 1;
                entries.push((row, if i == np - 1 { 0 } else { i + 1 }, weight * self.bounds[adj].sinratio as f64));
                rhs[row] += weight * self.ehs[enextleft].offset_l as f64;
            } else {
                entries.push((i - 1, i, -1.0));
            }
            i += 1;
            v = self.bounds[v].adjchain;
            if v == NONE || v == vstart {
                break;
            }
        }
        let x = solve_least_squares(nrows, np, &entries, &rhs);
        let mut v = vstart;
        let mut i = 0usize;
        loop {
            let val = x[i];
            if iscycle || i < np - 1 {
                let eright = self.bounds[v].efirst;
                let eleft = self.bounds[v].elast;
                self.ehs[eright].offset_r = val as f32;
                if iscycle || v != vstart {
                    self.ehs[eleft].offset_l = (self.bounds[v].sinratio as f64 * val) as f32;
                }
            } else {
                let eleft = self.bounds[v].elast;
                self.ehs[eleft].offset_l = val as f32;
            }
            i += 1;
            v = self.bounds[v].adjchain;
            if v == NONE || v == vstart {
                break;
            }
        }
    }

    fn adjust_offsets(&mut self) {
        let vids = self.bm.vert_ids();
        for &bmv in &vids {
            if !self.tag(bmv) {
                continue;
            }
            let bv = self.find_bevvert(bmv);
            if bv == NONE {
                continue;
            }
            let mut bvcur;
            let start = self.vms[self.bvs[bv].vmesh].boundstart;
            let mut vanchor = start;
            loop {
                if self.bounds[vanchor].visited || self.bounds[vanchor].eon == NONE {
                    vanchor = self.bounds[vanchor].next;
                    if vanchor == start {
                        break;
                    }
                    continue;
                }
                let (mut v, mut vchainstart, mut vchainend) = (vanchor, vanchor, vanchor);
                let mut iscycle = false;
                let mut chainlen = 1;
                bvcur = bv;
                let _ = bvcur;
                while self.bounds[v].eon != NONE && !self.bounds[v].visited && !iscycle {
                    self.bounds[v].visited = true;
                    if self.bounds[v].efirst == NONE {
                        break;
                    }
                    let (enext, bvn) = self.find_other_end_edge_half(self.bounds[v].efirst);
                    bvcur = bvn;
                    let _ = bvcur;
                    if enext == NONE {
                        break;
                    }
                    let vnext = self.ehs[enext].leftv;
                    self.bounds[v].adjchain = vnext;
                    vchainend = vnext;
                    chainlen += 1;
                    if self.bounds[vnext].visited {
                        if vnext != vchainstart {
                            break;
                        }
                        self.adjust_the_cycle_or_chain(vchainstart, true);
                        iscycle = true;
                    }
                    v = vnext;
                }
                if !iscycle {
                    self.bounds[v].adjchain = NONE;
                    v = vchainstart;
                    loop {
                        self.bounds[v].visited = true;
                        if self.bounds[v].elast == NONE {
                            break;
                        }
                        let (enext, _) = self.find_other_end_edge_half(self.bounds[v].elast);
                        if enext == NONE {
                            break;
                        }
                        let vnext = self.ehs[enext].rightv;
                        self.bounds[vnext].adjchain = v;
                        chainlen += 1;
                        vchainstart = vnext;
                        v = vnext;
                        if self.bounds[v].visited || self.bounds[v].eon == NONE {
                            break;
                        }
                    }
                    if chainlen >= 3 && self.bounds[vchainstart].eon == NONE && self.bounds[vchainend].eon == NONE {
                        self.adjust_the_cycle_or_chain(vchainstart, false);
                    }
                }
                vanchor = self.bounds[vanchor].next;
                if vanchor == start {
                    break;
                }
            }
        }
        for &bmv in &vids {
            if self.tag(bmv) {
                let bv = self.find_bevvert(bmv);
                if bv != NONE {
                    self.build_boundary(bv, false);
                }
            }
        }
    }

    fn pipe_test(&self, bv: usize) -> usize {
        let vm = self.bvs[bv].vmesh;
        let count = self.vms[vm].count;
        let sel = self.bvs[bv].selcount;
        if !(3..=4).contains(&count) || !(3..=4).contains(&sel) {
            return NONE;
        }
        let bvv = self.bvs[bv].v;
        let start = self.vms[vm].boundstart;
        let mut v1 = start;
        let mut epipe = NONE;
        let mut dir1 = [0.0f32; 3];
        loop {
            let v2 = self.bounds[v1].next;
            let v3 = self.bounds[v2].next;
            if self.bounds[v1].ebev != NONE && self.bounds[v2].ebev != NONE && self.bounds[v3].ebev != NONE {
                dir1 = normalize(sub(self.co(bvv), self.co(self.bm.edge_other_vert(self.ehs[self.bounds[v1].ebev].e, bvv))));
                let dir3 = normalize(sub(self.co(self.bm.edge_other_vert(self.ehs[self.bounds[v3].ebev].e, bvv)), self.co(bvv)));
                if angle_normalized_v3v3(dir1, dir3) < eps_ang() {
                    epipe = self.bounds[v1].ebev;
                    break;
                }
            }
            v1 = self.bounds[v1].next;
            if v1 == start {
                break;
            }
        }
        if epipe == NONE {
            return NONE;
        }
        let b = &self.bvs[bv];
        for i in 0..b.edgecount {
            let e = &self.ehs[b.e0 + i];
            if e.fnext != NONE && dot(dir1, self.fno(e.fnext)).abs() > BEVEL_EPSILON_BIG {
                return NONE;
            }
        }
        v1
    }
}

fn sabin_gamma(n: i32) -> f32 {
    if n < 3 {
        return 0.0;
    }
    if n == 3 {
        return 0.065247584;
    }
    if n == 4 {
        return 0.25;
    }
    if n == 5 {
        return 0.401983447;
    }
    if n == 6 {
        return 0.523423277;
    }
    let k = (std::f64::consts::PI / n as f64).cos();
    let k2 = k * k;
    let k4 = k2 * k2;
    let k6 = k4 * k2;
    let y = (3.0f64.sqrt() * (64.0 * k6 - 144.0 * k4 + 135.0 * k2 - 27.0).sqrt() + 9.0 * k).powf(1.0 / 3.0);
    let x = 0.480749856769136 * y - (0.231120424783545 * (12.0 * k2 - 9.0)) / y;
    ((k * x + 2.0 * k2 - 1.0) / (x * x * (k * x + 1.0))) as f32
}

fn interp_range(frac: &[f32], n: i32, f: f32) -> (i32, f32) {
    for i in 0..n {
        if f <= frac[(i + 1) as usize] {
            let rest = f - frac[i as usize];
            let mut r_rest = if rest == 0.0 { 0.0 } else { rest / (frac[(i + 1) as usize] - frac[i as usize]) };
            let mut ii = i;
            if i == n - 1 && r_rest == 1.0 {
                ii = n;
                r_rest = 0.0;
            }
            return (ii, r_rest);
        }
    }
    (n, 0.0)
}

impl<'a> Bp<'a> {
    fn mesh_vert_canon_idx(&self, vm: usize, i: i32, j: i32, k: i32) -> usize {
        let n = self.vms[vm].count;
        let ns = self.vms[vm].seg;
        let ns2 = ns / 2;
        let odd = ns % 2;
        if odd == 0 && j == ns2 && k == ns2 {
            return self.mesh_idx(vm, 0, j, k);
        }
        if j <= ns2 - 1 + odd && k <= ns2 {
            return self.mesh_idx(vm, i, j, k);
        }
        if k <= ns2 {
            return self.mesh_idx(vm, (i + n - 1) % n, k, ns - j);
        }
        self.mesh_idx(vm, (i + 1) % n, ns - k, j)
    }
    fn mv_canon(&self, vm: usize, i: i32, j: i32, k: i32) -> NewVert {
        self.vms[vm].mesh[self.mesh_vert_canon_idx(vm, i, j, k)]
    }
    fn is_canon(&self, vm: usize, i: i32, j: i32, k: i32) -> bool {
        let ns2 = self.vms[vm].seg / 2;
        if self.vms[vm].seg % 2 == 1 {
            return j <= ns2 && k <= ns2;
        }
        (j < ns2 && k <= ns2) || (j == ns2 && k == ns2 && i == 0)
    }
    fn vmesh_copy_equiv_verts(&mut self, vm: usize) {
        let n = self.vms[vm].count;
        let ns = self.vms[vm].seg;
        let ns2 = ns / 2;
        for i in 0..n {
            for j in 0..=ns2 {
                for k in 0..=ns {
                    if self.is_canon(vm, i, j, k) {
                        continue;
                    }
                    let v0 = self.mv_canon(vm, i, j, k);
                    let ix = self.mesh_idx(vm, i, j, k);
                    self.vms[vm].mesh[ix].co = v0.co;
                    self.vms[vm].mesh[ix].v = v0.v;
                }
            }
        }
    }
    fn vmesh_center(&self, vm: usize) -> V3 {
        let n = self.vms[vm].count;
        let ns2 = self.vms[vm].seg / 2;
        if self.vms[vm].seg % 2 != 0 {
            let mut c = [0.0f32; 3];
            for i in 0..n {
                c = add(c, self.mv_co(vm, i, ns2, ns2));
            }
            mul(c, 1.0 / n as f32)
        } else {
            self.mv_co(vm, 0, ns2, ns2)
        }
    }
    fn fill_vmesh_fracs(&self, vm: usize, i: i32) -> Vec<f32> {
        let ns = self.vms[vm].seg;
        let mut frac = vec![0.0f32; ns as usize + 1];
        let mut total = 0.0f32;
        for k in 0..ns {
            total += len_v3v3(self.mv_co(vm, i, 0, k), self.mv_co(vm, i, 0, k + 1));
            frac[k as usize + 1] = total;
        }
        if total > 0.0 {
            for k in 1..=ns as usize {
                frac[k] /= total;
            }
        } else {
            frac[ns as usize] = 1.0;
        }
        frac
    }
    fn fill_profile_fracs(&self, bndv: usize, ns: i32) -> Vec<f32> {
        let mut frac = vec![0.0f32; ns as usize + 1];
        let mut co = self.bounds[bndv].nv.co;
        let mut total = 0.0f32;
        let pro = &self.bounds[bndv].profile;
        for k in 0..ns {
            let nextco = self.get_profile_point(pro, k + 1, ns);
            total += len_v3v3(co, nextco);
            frac[k as usize + 1] = total;
            co = nextco;
        }
        if total > 0.0 {
            for k in 1..=ns as usize {
                frac[k] /= total;
            }
        } else {
            frac[ns as usize] = 1.0;
        }
        frac
    }

    fn interp_vmesh(&mut self, vm_in: usize, nseg: i32) -> usize {
        let n_bndv = self.vms[vm_in].count;
        let ns_in = self.vms[vm_in].seg;
        let nseg2 = nseg / 2;
        let odd = nseg % 2;
        let boundstart = self.vms[vm_in].boundstart;
        let vm_out = self.new_vmesh(n_bndv, nseg, boundstart);
        let mut prev_frac = self.fill_vmesh_fracs(vm_in, n_bndv - 1);
        let mut bndv = boundstart;
        let mut prev_new_frac = self.fill_profile_fracs(self.bounds[bndv].prev, nseg);
        for i in 0..n_bndv {
            let frac = self.fill_vmesh_fracs(vm_in, i);
            let new_frac = self.fill_profile_fracs(bndv, nseg);
            for j in 0..=nseg2 - 1 + odd {
                for k in 0..=nseg2 {
                    let fraction = new_frac[k as usize];
                    let (k_in, restk) = interp_range(&frac, ns_in, fraction);
                    let fraction = prev_new_frac[(nseg - j) as usize];
                    let (k_in_prev, restkprev) = interp_range(&prev_frac, ns_in, fraction);
                    let mut j_in = ns_in - k_in_prev;
                    let mut restj = -restkprev;
                    if restj > -BEVEL_EPSILON {
                        restj = 0.0;
                    } else {
                        j_in -= 1;
                        restj = 1.0 + restj;
                    }
                    let co;
                    if restj < BEVEL_EPSILON && restk < BEVEL_EPSILON {
                        co = self.mv_canon(vm_in, i, j_in, k_in).co;
                    } else {
                        let j0inc = if restj < BEVEL_EPSILON || j_in == ns_in { 0 } else { 1 };
                        let k0inc = if restk < BEVEL_EPSILON || k_in == ns_in { 0 } else { 1 };
                        let quad = [
                            self.mv_canon(vm_in, i, j_in, k_in).co,
                            self.mv_canon(vm_in, i, j_in, k_in + k0inc).co,
                            self.mv_canon(vm_in, i, j_in + j0inc, k_in + k0inc).co,
                            self.mv_canon(vm_in, i, j_in + j0inc, k_in).co,
                        ];
                        co = interp_bilinear_quad(quad, restk, restj);
                    }
                    self.set_mv_co(vm_out, i, j, k, co);
                }
            }
            bndv = self.bounds[bndv].next;
            prev_frac = frac;
            prev_new_frac = new_frac;
        }
        if odd == 0 {
            let center = self.vmesh_center(vm_in);
            self.set_mv_co(vm_out, 0, nseg2, nseg2, center);
        }
        self.vmesh_copy_equiv_verts(vm_out);
        vm_out
    }

    fn avg4(&self, a: NewVert, b: NewVert, c: NewVert, d: NewVert) -> V3 {
        let mut co = add(a.co, b.co);
        co = add(co, c.co);
        co = add(co, d.co);
        mul(co, 0.25)
    }

    fn cubic_subdiv(&mut self, vm_in: usize) -> usize {
        let n_boundary = self.vms[vm_in].count;
        let ns_in = self.vms[vm_in].seg;
        let ns_in2 = ns_in / 2;
        let ns_out = 2 * ns_in;
        let boundstart = self.vms[vm_in].boundstart;
        let vm_out = self.new_vmesh(n_boundary, ns_out, boundstart);
        for i in 0..n_boundary {
            let c0 = self.mv_co(vm_in, i, 0, 0);
            self.set_mv_co(vm_out, i, 0, 0, c0);
            for k in 1..ns_in {
                let mut co = self.mv_co(vm_in, i, 0, k);
                let co1 = self.mv_co(vm_in, i, 0, k - 1);
                let co2 = self.mv_co(vm_in, i, 0, k + 1);
                let mut acc = add(co1, co2);
                acc = madd(acc, co, -2.0);
                co = madd(co, acc, -1.0 / 6.0);
                let ix = self.mesh_vert_canon_idx(vm_out, i, 0, 2 * k);
                self.vms[vm_out].mesh[ix].co = co;
            }
        }
        let mut bndv = self.vms[vm_out].boundstart;
        for i in 0..n_boundary {
            let mut k = 1;
            while k < ns_out {
                let mut co = self.get_profile_point(&self.bounds[bndv].profile, k, ns_out);
                let co1 = self.mv_canon(vm_out, i, 0, k - 1).co;
                let co2 = self.mv_canon(vm_out, i, 0, k + 1).co;
                let mut acc = add(co1, co2);
                acc = madd(acc, co, -2.0);
                co = madd(co, acc, -1.0 / 6.0);
                let ix = self.mesh_vert_canon_idx(vm_out, i, 0, k);
                self.vms[vm_out].mesh[ix].co = co;
                k += 2;
            }
            bndv = self.bounds[bndv].next;
        }
        self.vmesh_copy_equiv_verts(vm_out);
        for i in 0..n_boundary {
            for k in 0..ns_in {
                let c = self.mv_co(vm_out, i, 0, 2 * k);
                self.set_mv_co(vm_in, i, 0, k, c);
            }
        }
        self.vmesh_copy_equiv_verts(vm_in);
        for i in 0..n_boundary {
            for j in 0..ns_in2 {
                for k in 0..ns_in2 {
                    let co = self.avg4(self.mv(vm_in, i, j, k), self.mv(vm_in, i, j, k + 1), self.mv(vm_in, i, j + 1, k), self.mv(vm_in, i, j + 1, k + 1));
                    self.set_mv_co(vm_out, i, 2 * j + 1, 2 * k + 1, co);
                }
            }
        }
        for i in 0..n_boundary {
            for j in 0..ns_in2 {
                for k in 1..=ns_in2 {
                    let co = self.avg4(self.mv(vm_in, i, j, k), self.mv(vm_in, i, j + 1, k), self.mv_canon(vm_out, i, 2 * j + 1, 2 * k - 1), self.mv_canon(vm_out, i, 2 * j + 1, 2 * k + 1));
                    self.set_mv_co(vm_out, i, 2 * j + 1, 2 * k, co);
                }
            }
        }
        for i in 0..n_boundary {
            for j in 1..ns_in2 {
                for k in 0..ns_in2 {
                    let co = self.avg4(self.mv(vm_in, i, j, k), self.mv(vm_in, i, j, k + 1), self.mv_canon(vm_out, i, 2 * j - 1, 2 * k + 1), self.mv_canon(vm_out, i, 2 * j + 1, 2 * k + 1));
                    self.set_mv_co(vm_out, i, 2 * j, 2 * k + 1, co);
                }
            }
        }
        let mut gamma = 0.25f32;
        let mut beta = -gamma;
        for i in 0..n_boundary {
            for j in 1..ns_in2 {
                for k in 1..=ns_in2 {
                    let co1 = self.avg4(self.mv_canon(vm_out, i, 2 * j, 2 * k - 1), self.mv_canon(vm_out, i, 2 * j, 2 * k + 1), self.mv_canon(vm_out, i, 2 * j - 1, 2 * k), self.mv_canon(vm_out, i, 2 * j + 1, 2 * k));
                    let co2 = self.avg4(
                        self.mv_canon(vm_out, i, 2 * j - 1, 2 * k - 1),
                        self.mv_canon(vm_out, i, 2 * j + 1, 2 * k - 1),
                        self.mv_canon(vm_out, i, 2 * j - 1, 2 * k + 1),
                        self.mv_canon(vm_out, i, 2 * j + 1, 2 * k + 1),
                    );
                    let mut co = co1;
                    co = madd(co, co2, beta);
                    co = madd(co, self.mv_co(vm_in, i, j, k), gamma);
                    self.set_mv_co(vm_out, i, 2 * j, 2 * k, co);
                }
            }
        }
        self.vmesh_copy_equiv_verts(vm_out);
        gamma = sabin_gamma(n_boundary);
        beta = -gamma;
        let mut co1 = [0.0f32; 3];
        let mut co2 = [0.0f32; 3];
        for i in 0..n_boundary {
            co1 = add(co1, self.mv_co(vm_out, i, ns_in, ns_in - 1));
            co2 = add(co2, self.mv_co(vm_out, i, ns_in - 1, ns_in - 1));
            co2 = add(co2, self.mv_co(vm_out, i, ns_in - 1, ns_in + 1));
        }
        let mut co = co1;
        co = mul(co, 1.0 / n_boundary as f32);
        co = madd(co, co2, beta / (2.0 * n_boundary as f32));
        co = madd(co, self.mv_co(vm_in, 0, ns_in2, ns_in2), gamma);
        for i in 0..n_boundary {
            self.set_mv_co(vm_out, i, ns_in, ns_in, co);
        }
        let mut bndv = self.vms[vm_out].boundstart;
        for i in 0..n_boundary {
            let inext = (i + 1) % n_boundary;
            for k in 0..=ns_out {
                let co = self.get_profile_point(&self.bounds[bndv].profile, k, ns_out);
                self.set_mv_co(vm_out, i, 0, k, co);
                if k >= ns_in && k < ns_out {
                    self.set_mv_co(vm_out, inext, ns_out - k, 0, co);
                }
            }
            bndv = self.bounds[bndv].next;
        }
        vm_out
    }
}

impl<'a> Bp<'a> {
    fn make_cube_corner_adj_vmesh(&mut self) -> Result<usize, String> {
        let nseg = self.seg;
        let r = self.pro_super_r;
        if r == PRO_SQUARE_R || r == PRO_SQUARE_IN_R {
            return Err("square profile cube corner not ported".into());
        }
        let vm0 = self.new_vmesh(3, 2, NONE);
        self.vms[vm0].count = 0;
        for i in 0..3 {
            let mut co = [0.0f32; 3];
            co[i] = 1.0;
            self.add_new_bound_vert(vm0, co);
        }
        let mut bndv = self.vms[vm0].boundstart;
        for i in 0..3 {
            let mut coc = [0.0f32; 3];
            coc[i] = 1.0;
            coc[(i + 1) % 3] = 1.0;
            coc[(i + 2) % 3] = 0.0;
            let nxt = self.bounds[bndv].next;
            let start = self.bounds[bndv].nv.co;
            let end = self.bounds[nxt].nv.co;
            {
                let p = &mut self.bounds[bndv].profile;
                p.super_r = r;
                p.start = start;
                p.end = end;
                p.middle = coc;
                p.plane_co = start;
                p.plane_no = cross(start, end);
                p.proj_dir = p.plane_no;
            }
            self.set_mv_co(vm0, i as i32, 0, 0, start);
            self.calculate_profile(bndv, false);
            let pt = self.get_profile_point(&self.bounds[bndv].profile.clone(), 1, 2);
            self.set_mv_co(vm0, i as i32, 0, 1, pt);
            bndv = nxt;
        }
        let c = std::f32::consts::FRAC_1_SQRT_2 * 0.0 + (1.0f64 / 3.0f64.sqrt()) as f32;
        let mut co = [c, c, c];
        if nseg > 2 {
            if r > 1.5 {
                co = mul(co, 1.4);
            } else if r < 0.75 {
                co = mul(co, 0.6);
            }
        }
        self.set_mv_co(vm0, 0, 1, 1, co);
        self.vmesh_copy_equiv_verts(vm0);
        let mut vm1 = vm0;
        while self.vms[vm1].seg < nseg {
            vm1 = self.cubic_subdiv(vm1);
        }
        if self.vms[vm1].seg != nseg {
            vm1 = self.interp_vmesh(vm1, nseg);
        }
        let ns2 = nseg / 2;
        for i in 0..3 {
            for j in 0..=ns2 {
                for k in 0..=nseg {
                    let c = self.mv_co(vm1, i, j, k);
                    self.set_mv_co(vm1, i, j, k, snap_to_superellipsoid(c, r, false));
                }
            }
        }
        Ok(vm1)
    }

    fn tri_corner_test(&self, bv: usize) -> i32 {
        if self.vertex_only {
            return -1;
        }
        let mut in_plane_e = 0;
        let vm = self.bvs[bv].vmesh;
        if self.vms[vm].count != 3 {
            return 0;
        }
        let b = &self.bvs[bv];
        let offset = self.ehs[b.e0].offset_l;
        let mut totang = 0.0f32;
        for i in 0..b.edgecount {
            let e = &self.ehs[b.e0 + i];
            let ang = self.edge_face_angle_signed(e.e, 0.0);
            let absang = ang.abs();
            if absang <= std::f32::consts::FRAC_PI_4 {
                in_plane_e += 1;
            } else if absang >= 3.0 * std::f32::consts::FRAC_PI_4 {
                return -1;
            }
            if e.is_bev && !compare_ff(e.offset_l, offset, BEVEL_EPSILON) {
                return -1;
            }
            totang += ang;
        }
        if in_plane_e != b.edgecount as i32 - 3 {
            return -1;
        }
        let angdiff = (totang.abs() - 3.0 * std::f32::consts::FRAC_PI_2).abs();
        if (self.pro_super_r == PRO_SQUARE_R && angdiff > PI / 16.0) || angdiff > std::f32::consts::FRAC_PI_4 {
            return -1;
        }
        if b.edgecount != 3 || b.selcount != 3 {
            return 0;
        }
        1
    }
    /// BM_edge_calc_face_angle_signed_ex
    fn edge_face_angle_signed(&self, e: usize, fallback: f32) -> f32 {
        if self.bm.edge_is_manifold(e) {
            let l1 = self.bm.edges[e].l;
            let l2 = self.bm.loops[l1].rn;
            let (f1, f2) = (self.bm.loops[l1].f, self.bm.loops[l2].f);
            let angle = angle_normalized_v3v3(self.fno(f1), self.fno(f2));
            if self.edge_is_convex(e) {
                angle
            } else {
                -angle
            }
        } else {
            fallback
        }
    }
    /// BM_edge_is_convex
    fn edge_is_convex(&self, e: usize) -> bool {
        if self.bm.edge_is_manifold(e) {
            let l1 = self.bm.edges[e].l;
            let l2 = self.bm.loops[l1].rn;
            let (f1, f2) = (self.bm.loops[l1].f, self.bm.loops[l2].f);
            if self.fno(f1) != self.fno(f2) {
                let cr = cross(self.fno(f1), self.fno(f2));
                let l_dir = sub(self.co(self.bm.loops[self.bm.loops[l1].n].v), self.co(self.bm.loops[l1].v));
                return dot(l_dir, cr) > 0.0;
            }
        }
        true
    }

    fn tri_corner_adj_vmesh(&mut self, bv: usize) -> Result<usize, String> {
        let vm = self.bvs[bv].vmesh;
        let mut bndv = self.vms[vm].boundstart;
        let co0 = self.bounds[bndv].nv.co;
        bndv = self.bounds[bndv].next;
        let co1 = self.bounds[bndv].nv.co;
        bndv = self.bounds[bndv].next;
        let co2 = self.bounds[bndv].nv.co;
        let bvv = self.co(self.bvs[bv].v);
        let mat = make_unit_cube_map(co0, co1, co2, bvv);
        let ns = self.seg;
        let ns2 = ns / 2;
        let vm1 = self.make_cube_corner_adj_vmesh()?;
        for i in 0..3 {
            for j in 0..=ns2 {
                for k in 0..=ns {
                    let c = self.mv_co(vm1, i, j, k);
                    let r = mul_m4_v4(&mat, [c[0], c[1], c[2], 1.0]);
                    self.set_mv_co(vm1, i, j, k, [r[0], r[1], r[2]]);
                }
            }
        }
        Ok(vm1)
    }

    fn adj_vmesh(&mut self, bv: usize) -> Result<usize, String> {
        let vmb = self.bvs[bv].vmesh;
        let n_bndv = self.vms[vmb].count;
        if n_bndv == 3 && self.tri_corner_test(bv) != -1 && self.pro_super_r != PRO_SQUARE_IN_R {
            return self.tri_corner_adj_vmesh(bv);
        }
        let nseg = self.vms[vmb].seg;
        let boundstart = self.vms[vmb].boundstart;
        let vm0 = self.new_vmesh(n_bndv, 2, boundstart);
        let mut bndv = self.vms[vm0].boundstart;
        let mut boundverts_center = [0.0f32; 3];
        for i in 0..n_bndv {
            let c = self.bounds[bndv].nv.co;
            self.set_mv_co(vm0, i, 0, 0, c);
            let p = self.get_profile_point(&self.bounds[bndv].profile, 1, 2);
            self.set_mv_co(vm0, i, 0, 1, p);
            boundverts_center = add(boundverts_center, c);
            bndv = self.bounds[bndv].next;
        }
        boundverts_center = mul(boundverts_center, 1.0 / n_bndv as f32);
        let original_vertex = self.co(self.bvs[bv].v);
        let fullness = self.pro_spacing.fullness;
        let center_direction = sub(original_vertex, boundverts_center);
        if len_sq(center_direction) > BEVEL_EPSILON_SQ {
            let c = madd(boundverts_center, center_direction, fullness);
            self.set_mv_co(vm0, 0, 1, 1, c);
        } else {
            self.set_mv_co(vm0, 0, 1, 1, boundverts_center);
        }
        self.vmesh_copy_equiv_verts(vm0);
        let mut vm1 = vm0;
        loop {
            vm1 = self.cubic_subdiv(vm1);
            if self.vms[vm1].seg >= nseg {
                break;
            }
        }
        if self.vms[vm1].seg != nseg {
            vm1 = self.interp_vmesh(vm1, nseg);
        }
        Ok(vm1)
    }

    fn snap_to_pipe_profile(&self, vpipe: usize, midline: bool, co: V3) -> V3 {
        let pro = &self.bounds[vpipe].profile;
        let e = self.ehs[self.bounds[vpipe].ebev].e;
        if compare_v3v3(pro.start, pro.end, BEVEL_EPSILON_D as f32) {
            return pro.start;
        }
        let be = &self.bm.edges[e];
        let edir = sub(self.co(be.v1), self.co(be.v2));
        let plane = plane_from_point_normal(co, edir);
        let start_plane = closest_to_plane(plane, pro.start);
        let end_plane = closest_to_plane(plane, pro.end);
        let middle_plane = closest_to_plane(plane, pro.middle);
        if let Some(m) = make_unit_square_map(start_plane, middle_plane, end_plane) {
            if let Some(minv) = invert_m4(&m) {
                let p = mul_v3_m4v3(&minv, co);
                let p = snap_to_superellipsoid(p, pro.super_r, midline);
                return mul_v3_m4v3(&m, p);
            }
        }
        closest_to_line_segment(co, start_plane, end_plane)
    }
    fn pipe_adj_vmesh(&mut self, bv: usize, vpipe: usize) -> Result<usize, String> {
        let vm = self.adj_vmesh(bv)?;
        let vmb = self.bvs[bv].vmesh;
        let n_bndv = self.vms[vmb].count;
        let ns = self.vms[vmb].seg;
        let half_ns = ns / 2;
        let ipipe1 = self.bounds[vpipe].index;
        let nn = self.bounds[self.bounds[vpipe].next].next;
        let ipipe2 = self.bounds[nn].index;
        for i in 0..n_bndv {
            for j in 1..=half_ns {
                for k in 0..=half_ns {
                    if !self.is_canon(vm, i, j, k) {
                        continue;
                    }
                    let even = ns % 2 == 0;
                    let midline = even && k == half_ns && ((i == 0 && j == half_ns) || i == ipipe1 || i == ipipe2);
                    let c = self.mv_co(vm, i, j, k);
                    let r = self.snap_to_pipe_profile(vpipe, midline, c);
                    self.set_mv_co(vm, i, j, k, r);
                }
            }
        }
        Ok(vm)
    }
}

fn invert_m4(m: &M4) -> Option<M4> {
    // Gauss-Jordan in f64 (Blender uses the same Gauss-Jordan in float; differences are float noise)
    let mut a = [[0.0f64; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = m[i][j] as f64;
        }
        a[i][4 + i] = 1.0;
    }
    for c in 0..4 {
        let mut p = c;
        for r in c + 1..4 {
            if a[r][c].abs() > a[p][c].abs() {
                p = r;
            }
        }
        if a[p][c].abs() < 1e-30 {
            return None;
        }
        a.swap(c, p);
        let d = a[c][c];
        for j in 0..8 {
            a[c][j] /= d;
        }
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                for j in 0..8 {
                    let t = a[c][j];
                    a[r][j] -= f * t;
                }
            }
        }
    }
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = a[i][4 + j] as f32;
        }
    }
    Some(out)
}

impl<'a> Bp<'a> {
    fn bevel_build_rings(&mut self, bv: usize, vpipe: usize) -> Result<(), String> {
        let mat_nr = self.mat_nr;
        let vmb = self.bvs[bv].vmesh;
        let n_bndv = self.vms[vmb].count;
        let ns = self.vms[vmb].seg;
        let ns2 = ns / 2;
        let odd = ns % 2;
        if self.pro_super_r == PRO_SQUARE_R && self.bvs[bv].selcount >= 3 && odd == 0 {
            return Err("square profile vertex mesh not ported".into());
        }
        let vm1;
        if vpipe != NONE {
            vm1 = self.pipe_adj_vmesh(bv, vpipe)?;
        } else if self.tri_corner_test(bv) == 1 {
            vm1 = self.tri_corner_adj_vmesh(bv)?;
            if self.pro_super_r == PRO_SQUARE_IN_R {
                return Err("square-in profile not ported".into());
            }
        } else {
            vm1 = self.adj_vmesh(bv)?;
        }
        let vm = vmb;
        let bvv = self.bvs[bv].v;
        for i in 0..n_bndv {
            for j in 0..=ns2 {
                for k in 0..=ns {
                    if j == 0 && (k == 0 || k == ns) {
                        continue;
                    }
                    if !self.is_canon(vm, i, j, k) {
                        continue;
                    }
                    let c = self.mv_co(vm1, i, j, k);
                    self.set_mv_co(vm, i, j, k, c);
                    self.create_mesh_bmvert(vm, i, j, k, bvv);
                }
            }
        }
        self.vmesh_copy_equiv_verts(vm);
        let start = self.vms[vm].boundstart;
        let mut bndv_rep_faces = vec![NONE; n_bndv as usize];
        let mut bndv = start;
        loop {
            let i = self.bounds[bndv].index;
            bndv_rep_faces[i as usize] = self.boundvert_rep_face(bndv);
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
        let mut center_verts = vec![NONE; n_bndv as usize];
        let mut center_face_interps = vec![NONE; n_bndv as usize];
        let mut frep_beats_next = vec![false; n_bndv as usize];
        if odd == 1 {
            for i in 0..n_bndv {
                let inext = (i + 1) % n_bndv;
                let fchoices = [bndv_rep_faces[i as usize], bndv_rep_faces[inext as usize]];
                let fwinner = self.choose_rep_face(&fchoices);
                frep_beats_next[i as usize] = fwinner == bndv_rep_faces[i as usize];
            }
        }
        let mut bndv = start;
        loop {
            let i = self.bounds[bndv].index;
            let inext = self.bounds[self.bounds[bndv].next].index;
            let f = bndv_rep_faces[i as usize];
            let f2 = bndv_rep_faces[inext as usize];
            let mut fc = NONE;
            if odd == 1 {
                fc = if frep_beats_next[i as usize] { f } else { f2 };
            }
            let _ = fc;
            for j in 0..ns2 {
                for k in 0..(ns2 + odd) {
                    let bmvs = [self.mv(vm, i, j, k).v, self.mv(vm, i, j, k + 1).v, self.mv(vm, i, j + 1, k + 1).v, self.mv(vm, i, j + 1, k).v];
                    let mut fr = [f, f, f, f];
                    if odd == 1 && k == ns2 {
                        // not a seam: left half interpolates in f, right half in f2
                        fr = [f, f, f, f];
                        if self.bounds[bndv].ebev == NONE {
                            fr = [fc, fc, fc, fc];
                        } else {
                            fr[0] = f;
                            fr[3] = f;
                            fr[1] = f2;
                            fr[2] = f2;
                        }
                        if j == ns2 - 1 {
                            center_verts[i as usize] = bmvs[3];
                            center_face_interps[i as usize] = f;
                        }
                    }
                    let r_f = self.bev_create_ngon(&bmvs, Some(&fr), NONE, mat_nr);
                    self.record_face_kind(r_f, FKind::Vert);
                }
            }
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
        if odd == 1 {
            let cen_f = self.bev_create_ngon(&center_verts, Some(&center_face_interps), NONE, mat_nr);
            self.record_face_kind(cen_f, FKind::Vert);
        }
        Ok(())
    }

    /// choose_rep_face without math layers: lowest selected/material/center-z/x/y value wins
    fn choose_rep_face(&self, face: &[usize]) -> usize {
        let nfaces = face.len();
        let mut value_vecs = vec![[0.0f32; 6]; nfaces];
        let mut still_viable = vec![false; nfaces];
        let mut num_viable = 0;
        for f in 0..nfaces {
            let bmf = face[f];
            if bmf == NONE {
                still_viable[f] = false;
                continue;
            }
            still_viable[f] = true;
            num_viable += 1;
            let fl = &self.bm.faces[bmf];
            let cent = self.bm.face_calc_center_bounds(bmf);
            value_vecs[f] = [0.0, if fl.flag & ELEM_SELECT != 0 { 0.0 } else { 1.0 }, if fl.mat_nr >= 0 { fl.mat_nr as f32 } else { 0.0 }, cent[2], cent[0], cent[1]];
        }
        let mut best_f: i32 = -1;
        let mut value_index = 0;
        while num_viable > 1 && value_index < 6 {
            for f in 0..nfaces as i32 {
                if !still_viable[f as usize] || f == best_f {
                    continue;
                }
                if best_f == -1 {
                    best_f = f;
                    continue;
                }
                let (vf, vb) = (value_vecs[f as usize][value_index], value_vecs[best_f as usize][value_index]);
                if vf < vb {
                    best_f = f;
                    for i in (0..f).rev() {
                        if still_viable[i as usize] {
                            still_viable[i as usize] = false;
                            num_viable -= 1;
                        }
                    }
                } else if vf > vb {
                    still_viable[f as usize] = false;
                    num_viable -= 1;
                }
            }
            value_index += 1;
        }
        if best_f == -1 {
            best_f = 0;
        }
        face[best_f as usize]
    }
}

impl<'a> Bp<'a> {
    fn bevel_build_poly(&mut self, bv: usize) -> usize {
        let vm = self.bvs[bv].vmesh;
        let mut bmverts: Vec<usize> = vec![];
        let mut bmfaces: Vec<usize> = vec![];
        let start = self.vms[vm].boundstart;
        let mut bndv = start;
        let mut n = 0;
        loop {
            bmverts.push(self.bounds[bndv].nv.v);
            bmfaces.push(self.boundvert_rep_face(bndv));
            n += 1;
            let ebev = self.bounds[bndv].ebev;
            if ebev != NONE && self.ehs[ebev].seg > 1 {
                for k in 1..self.ehs[ebev].seg {
                    bmverts.push(self.mv(vm, self.bounds[bndv].index, 0, k).v);
                    bmfaces.push(self.boundvert_rep_face(bndv));
                    n += 1;
                }
            }
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
        if n > 2 {
            let mat = self.mat_nr;
            let f = self.bev_create_ngon(&bmverts, Some(&bmfaces), NONE, mat);
            self.record_face_kind(f, FKind::Vert);
            f
        } else {
            NONE
        }
    }

    fn bevel_build_trifan(&mut self, bv: usize) {
        let mut f = self.bevel_build_poly(bv);
        if f == NONE {
            return;
        }
        let mut l_fan = self.bm.loops[self.bm.faces[f].l_first].p;
        let v_fan = self.bm.loops[l_fan].v;
        while self.bm.faces[f].len > 3 {
            let ln = self.bm.loops[l_fan].n;
            let lnn = self.bm.loops[ln].n;
            let (f_new, l_new) = self.bm.face_split(f, l_fan, lnn);
            if self.bm.faces[f_new].len > self.bm.faces[f].len {
                f = f_new;
                if self.bm.loops[l_new].v == v_fan {
                    l_fan = l_new;
                } else if self.bm.loops[self.bm.loops[l_new].n].v == v_fan {
                    l_fan = self.bm.loops[l_new].n;
                } else if self.bm.loops[self.bm.loops[l_new].p].v == v_fan {
                    l_fan = self.bm.loops[l_new].p;
                }
            } else if self.bm.loops[l_fan].v == v_fan {
            } else if self.bm.loops[self.bm.loops[l_fan].n].v == v_fan {
                l_fan = self.bm.loops[l_fan].n;
            } else if self.bm.loops[self.bm.loops[l_fan].p].v == v_fan {
                l_fan = self.bm.loops[l_fan].p;
            }
            self.record_face_kind(f_new, FKind::Vert);
        }
    }

    /// vertex bevel with only two boundary verts: a curved edge between them when seg > 1 (or the plain edge if the vertex had no faces)
    fn bevel_vert_two_edges(&mut self, bv: usize) {
        let vm = self.bvs[bv].vmesh;
        let bvv = self.bvs[bv].v;
        let (v1, v2) = (self.mv(vm, 0, 0, 0).v, self.mv(vm, 1, 0, 0).v);
        let ns = self.vms[vm].seg;
        if ns > 1 {
            let bndv = self.vms[vm].boundstart;
            let mut pro = self.bounds[bndv].profile.clone();
            pro.super_r = self.pro_super_r;
            pro.start = self.co(v1);
            pro.end = self.co(v2);
            pro.middle = self.co(bvv);
            pro.plane_co = [0.0; 3];
            pro.plane_no = [0.0; 3];
            pro.proj_dir = [0.0; 3];
            for k in 1..ns {
                let co = self.get_profile_point(&pro, k, ns);
                self.set_mv_co(vm, 0, 0, k, co);
                self.create_mesh_bmvert(vm, 0, 0, k, bvv);
            }
            let c2 = self.co(v2);
            self.set_mv_co(vm, 0, 0, ns, c2);
            for k in 1..ns {
                self.copy_mesh_vert(vm, 1, 0, ns - k, 0, 0, k);
            }
            self.bounds[bndv].profile = pro;
        }
        if !self.bm.vert_face_check(bvv) {
            let e_eg = self.ehs[self.bvs[bv].e0].e;
            for k in 0..ns {
                let a = self.mv(vm, 0, 0, k).v;
                let b = self.mv(vm, 0, 0, k + 1).v;
                self.bm.edge_create(a, b, Some(e_eg), true);
            }
        }
    }

    fn build_vmesh(&mut self, bv: usize) -> Result<(), String> {
        let vm = self.bvs[bv].vmesh;
        let n = self.vms[vm].count;
        let ns = self.vms[vm].seg;
        let ns2 = ns / 2;
        let bvv = self.bvs[bv].v;
        self.vms[vm].mesh = vec![NewVert { v: NONE, co: [0.0; 3] }; (n * (ns2 + 1) * (ns + 1)) as usize];
        let weld = self.bvs[bv].selcount == 2 && self.vms[vm].count == 2;
        let (mut weld1, mut weld2) = (NONE, NONE);
        let start = self.vms[vm].boundstart;
        let mut bndv = start;
        loop {
            let i = self.bounds[bndv].index;
            let c = self.bounds[bndv].nv.co;
            self.set_mv_co(vm, i, 0, 0, c);
            self.create_mesh_bmvert(vm, i, 0, 0, bvv);
            self.bounds[bndv].nv.v = self.mv(vm, i, 0, 0).v;
            if weld && self.bounds[bndv].ebev != NONE {
                if weld1 == NONE {
                    weld1 = bndv;
                } else {
                    weld2 = bndv;
                    self.set_profile_params(bv, weld1);
                    self.set_profile_params(bv, weld2);
                    self.move_weld_profile_planes(bv, weld1, weld2);
                }
            }
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
        self.calculate_vm_profiles(bv, vm);
        let mut bndv = start;
        loop {
            let i = self.bounds[bndv].index;
            let inext = self.bounds[self.bounds[bndv].next].index;
            self.copy_mesh_vert(vm, i, 0, ns, inext, 0, 0);
            if self.vms[vm].mesh_kind != MeshKind::Adj {
                for k in 1..ns {
                    if self.bounds[bndv].ebev != NONE {
                        let co = self.get_profile_point(&self.bounds[bndv].profile.clone(), k, ns);
                        self.set_mv_co(vm, i, 0, k, co);
                        if !weld {
                            self.create_mesh_bmvert(vm, i, 0, k, bvv);
                        }
                    } else if n == 2 && self.bounds[bndv].ebev == NONE {
                        self.copy_mesh_vert(vm, i, 0, k, 1 - i, 0, ns - k);
                    }
                }
            }
            bndv = self.bounds[bndv].next;
            if bndv == start {
                break;
            }
        }
        if weld {
            self.vms[vm].mesh_kind = MeshKind::None;
            for k in 1..ns {
                let i1 = self.bounds[weld1].index;
                let i2 = self.bounds[weld2].index;
                let v_weld1 = self.mv_co(vm, i1, 0, k);
                let v_weld2 = self.mv_co(vm, i2, 0, ns - k);
                let co;
                if self.bounds[weld1].profile.super_r == PRO_LINE_R && self.bounds[weld2].profile.super_r != PRO_LINE_R {
                    co = v_weld2;
                } else if self.bounds[weld2].profile.super_r == PRO_LINE_R && self.bounds[weld1].profile.super_r != PRO_LINE_R {
                    co = v_weld1;
                } else {
                    co = mid(v_weld1, v_weld2);
                }
                self.set_mv_co(vm, i1, 0, k, co);
                self.create_mesh_bmvert(vm, i1, 0, k, bvv);
            }
            for k in 1..ns {
                let (i1, i2) = (self.bounds[weld1].index, self.bounds[weld2].index);
                self.copy_mesh_vert(vm, i2, 0, ns - k, i1, 0, k);
            }
        }
        let mut vpipe = NONE;
        if (self.vms[vm].count == 3 || self.vms[vm].count == 4) && self.seg > 1 {
            vpipe = self.pipe_test(bv);
            if vpipe != NONE {
                self.vms[vm].mesh_kind = MeshKind::Adj;
            }
        }
        match self.vms[vm].mesh_kind {
            MeshKind::None => {
                if n == 2 && self.vertex_only {
                    self.bevel_vert_two_edges(bv);
                }
            }
            MeshKind::Poly => {
                self.bevel_build_poly(bv);
            }
            MeshKind::Adj => {
                self.bevel_build_rings(bv, vpipe)?;
            }
            MeshKind::TriFan => {
                self.bevel_build_trifan(bv);
            }
            MeshKind::Cutoff => return Err("cutoff vertex mesh not ported".into()),
        }
        Ok(())
    }
}

const API_OVERLAP: u8 = 1;

impl<'a> Bp<'a> {
    fn edge_tag_enable(&mut self, e: usize) {
        self.bm.edges[e].api |= API_OVERLAP;
    }
    fn edge_tag_disable(&mut self, e: usize) {
        self.bm.edges[e].api &= !API_OVERLAP;
    }
    fn edge_tag_test(&self, e: usize) -> bool {
        self.bm.edges[e].api & API_OVERLAP != 0
    }

    fn bevel_edge_order_extend(&mut self, bv: usize, i: usize) -> usize {
        let e0 = self.bvs[bv].e0;
        let bvv = self.bvs[bv].v;
        let mut sucs: Vec<usize> = vec![];
        let bme = self.ehs[e0 + i].e;
        for l in self.bm.loops_of_edge(bme) {
            let bme2 = if self.bm.loops[l].v == bvv { self.bm.loops[self.bm.loops[l].p].e } else { self.bm.loops[self.bm.loops[l].n].e };
            if !self.edge_tag_test(bme2) {
                sucs.push(bme2);
            }
        }
        let nsucs = sucs.len();
        let mut bestj = i;
        let mut j = i;
        let mut save_path: Vec<usize> = vec![];
        for sucindex in 0..nsucs {
            let nextbme = sucs[sucindex];
            self.ehs[e0 + j + 1].e = nextbme;
            self.edge_tag_enable(nextbme);
            let tryj = self.bevel_edge_order_extend(bv, j + 1);
            if tryj > bestj || (tryj == bestj && self.edges_face_connected_at_vert(self.ehs[e0 + tryj].e, self.ehs[e0].e)) {
                bestj = tryj;
                save_path.clear();
                for k in j + 1..=bestj {
                    save_path.push(self.ehs[e0 + k].e);
                }
            }
            for k in j + 1..=tryj {
                let ee = self.ehs[e0 + k].e;
                self.edge_tag_disable(ee);
                self.ehs[e0 + k].e = NONE;
            }
        }
        if bestj > j {
            for k in j + 1..=bestj {
                self.ehs[e0 + k].e = save_path[k - (j + 1)];
                let ee = self.ehs[e0 + k].e;
                self.edge_tag_enable(ee);
            }
        }
        j = bestj;
        j
    }

    fn fast_bevel_edge_order(&mut self, bv: usize) -> bool {
        let ntot = self.bvs[bv].edgecount;
        let e0 = self.bvs[bv].e0;
        let bvv = self.bvs[bv].v;
        let mut bme = self.ehs[e0].e;
        if self.bm.edges[bme].l == NONE {
            return false;
        }
        for i in 1..ntot {
            let mut num_shared_face = 0;
            let mut first_suc = NONE;
            for bme2 in self.bm.edges_of_vert(bvv) {
                if self.edge_tag_test(bme2) {
                    continue;
                }
                for f in self.bm.faces_of_edge(bme2) {
                    if self.bm.face_edge_share_loop(f, bme) != NONE {
                        num_shared_face += 1;
                        if first_suc == NONE {
                            first_suc = bme2;
                        }
                    }
                }
                if num_shared_face >= 3 {
                    break;
                }
            }
            if num_shared_face == 1 || (i == 1 && num_shared_face == 2) {
                self.ehs[e0 + i].e = first_suc;
                bme = first_suc;
                self.edge_tag_enable(bme);
            } else {
                for k in 1..i {
                    let ee = self.ehs[e0 + k].e;
                    self.edge_tag_disable(ee);
                    self.ehs[e0 + k].e = NONE;
                }
                return false;
            }
        }
        true
    }

    fn find_bevel_edge_order(&mut self, bv: usize, first_bme: usize) {
        let ntot = self.bvs[bv].edgecount;
        let e0 = self.bvs[bv].e0;
        let bvv = self.bvs[bv].v;
        let mut first_bme = first_bme;
        let mut i = 0usize;
        loop {
            self.ehs[e0 + i].e = first_bme;
            self.edge_tag_enable(first_bme);
            if i == 0 && self.fast_bevel_edge_order(bv) {
                break;
            }
            i = self.bevel_edge_order_extend(bv, i);
            i += 1;
            if i >= ntot {
                break;
            }
            first_bme = NONE;
            for bme in self.bm.edges_of_vert(bvv) {
                if self.edge_tag_test(bme) {
                    continue;
                }
                if first_bme == NONE {
                    first_bme = bme;
                }
                if self.bm.edge_face_count(bme) == 1 {
                    first_bme = bme;
                    break;
                }
            }
        }
        for i in 0..ntot {
            let e = e0 + i;
            let e2 = if i == ntot - 1 { e0 } else { e0 + i + 1 };
            let bme = self.ehs[e].e;
            let bme2 = self.ehs[e2].e;
            if self.ehs[e].fnext != NONE || self.ehs[e2].fprev != NONE {
                continue;
            }
            let mut bestf = NONE;
            for l in self.bm.loops_of_edge(bme) {
                let f = self.bm.loops[l].f;
                let (lp, ln) = (self.bm.loops[l].p, self.bm.loops[l].n);
                if self.bm.loops[lp].e == bme2 || self.bm.loops[ln].e == bme2 {
                    if bestf == NONE || self.bm.loops[l].v == bvv {
                        bestf = f;
                    }
                }
                if bestf != NONE {
                    self.ehs[e].fnext = bestf;
                    self.ehs[e2].fprev = bestf;
                }
            }
        }
    }

    fn edge_face_angle(&self, e: usize) -> f32 {
        let eh = &self.ehs[e];
        if eh.fprev != NONE && eh.fnext != NONE {
            return PI - angle_normalized_v3v3(self.fno(eh.fprev), self.fno(eh.fnext));
        }
        0.0
    }

    fn bevel_vert_construct(&mut self, v: usize) -> usize {
        let mut nsel = 0;
        let mut tot_edges = 0;
        let mut tot_wire = 0;
        let mut first_bme = NONE;
        let edges = self.bm.edges_of_vert(v);
        for &bme in &edges {
            let face_count = self.bm.edge_face_count(bme);
            self.edge_tag_disable(bme);
            if self.bm.edges[bme].flag & ELEM_TAG != 0 && !self.vertex_only {
                nsel += 1;
                if first_bme == NONE {
                    first_bme = bme;
                }
            }
            if face_count == 1 {
                first_bme = bme;
            }
            if face_count > 0 || self.vertex_only {
                tot_edges += 1;
            }
            if self.bm.edge_is_wire(bme) {
                tot_wire += 1;
                if !self.vertex_only {
                    self.edge_tag_enable(bme);
                }
            }
        }
        if first_bme == NONE {
            first_bme = self.bm.verts[v].e;
        }
        if (nsel == 0 && !self.vertex_only) || (tot_edges < 2 && self.vertex_only) {
            self.bm.verts[v].flag &= !ELEM_TAG;
            return NONE;
        }
        let e0 = self.ehs.len();
        for _ in 0..tot_edges {
            self.ehs.push(EdgeHalf::blank());
        }
        let vm = self.vms.len();
        self.vms.push(VMesh { mesh: vec![], boundstart: NONE, count: 0, seg: self.seg, mesh_kind: MeshKind::None });
        let bv = self.bvs.len();
        self.bvs.push(BevVert { v, edgecount: tot_edges, selcount: nsel, wirecount: tot_wire, offset: self.offset, any_seam: false, visited: false, e0, wire_edges: vec![], vmesh: vm });
        self.vert_hash.insert(v, bv);
        self.find_bevel_edge_order(bv, first_bme);
        for i in 0..tot_edges {
            let bme = self.ehs[e0 + i].e;
            if self.bm.edges[bme].flag & ELEM_TAG != 0 && !self.vertex_only {
                self.ehs[e0 + i].is_bev = true;
                self.ehs[e0 + i].seg = self.seg;
            } else {
                self.ehs[e0 + i].is_bev = false;
                self.ehs[e0 + i].seg = 0;
            }
            self.ehs[e0 + i].is_rev = self.bm.edges[bme].v2 == v;
            self.ehs[e0 + i].leftv = NONE;
            self.ehs[e0 + i].rightv = NONE;
            self.ehs[e0 + i].profile_index = 0;
        }
        for &bme in &edges {
            self.edge_tag_disable(bme);
        }
        if tot_edges > 1 {
            let mut ccw_test_sum = 0;
            for i in 0..tot_edges {
                ccw_test_sum += self.bev_ccw_test(self.ehs[e0 + i].e, self.ehs[e0 + (i + 1) % tot_edges].e, self.ehs[e0 + i].fnext);
            }
            if ccw_test_sum < 0 {
                for i in 0..=(tot_edges / 2).saturating_sub(1) {
                    if tot_edges / 2 == 0 {
                        break;
                    }
                    let (a, b) = (e0 + i, e0 + tot_edges - i - 1);
                    self.ehs.swap(a, b);
                    let t = self.ehs[a].fprev;
                    self.ehs[a].fprev = self.ehs[a].fnext;
                    self.ehs[a].fnext = t;
                    let t = self.ehs[b].fprev;
                    self.ehs[b].fprev = self.ehs[b].fnext;
                    self.ehs[b].fnext = t;
                }
                if tot_edges % 2 == 1 {
                    let i = e0 + tot_edges / 2;
                    let t = self.ehs[i].fprev;
                    self.ehs[i].fprev = self.ehs[i].fnext;
                    self.ehs[i].fnext = t;
                }
            }
        }
        let mut vert_axis = [0.0f32; 3];
        let bv_offset = self.offset;
        if self.vertex_only && matches!(self.offset_type, OffsetType::Width | OffsetType::Depth) {
            for i in 0..tot_edges {
                let bme = self.ehs[e0 + i].e;
                let v2 = self.bm.edge_other_vert(bme, v);
                let edge_dir = normalize(sub(self.co(v), self.co(v2)));
                vert_axis = add(vert_axis, edge_dir);
            }
        }
        for i in 0..tot_edges {
            let e = e0 + i;
            self.ehs[e].next = e0 + (i + 1) % tot_edges;
            self.ehs[e].prev = e0 + (i + tot_edges - 1) % tot_edges;
            if self.ehs[e].is_bev {
                match self.offset_type {
                    OffsetType::Offset => self.ehs[e].offset_l_spec = self.offset,
                    OffsetType::Width => {
                        let z = (2.0 * (self.edge_face_angle(e) / 2.0).sin()).abs();
                        self.ehs[e].offset_l_spec = if z < BEVEL_EPSILON { 0.01 * self.offset } else { self.offset / z };
                    }
                    OffsetType::Depth => {
                        let z = ((self.edge_face_angle(e) / 2.0).cos()).abs();
                        self.ehs[e].offset_l_spec = if z < BEVEL_EPSILON { 0.01 * self.offset } else { self.offset / z };
                    }
                    OffsetType::Percent => {
                        self.ehs[e].offset_l_spec = self.bm.edge_calc_length(self.ehs[self.ehs[e].prev].e) * self.offset / 100.0;
                        self.ehs[e].offset_r_spec = self.bm.edge_calc_length(self.ehs[self.ehs[e].next].e) * self.offset / 100.0;
                    }
                    OffsetType::Absolute => {
                        self.ehs[e].offset_l_spec = self.offset;
                        self.ehs[e].offset_r_spec = self.offset;
                    }
                }
                if !matches!(self.offset_type, OffsetType::Percent | OffsetType::Absolute) {
                    self.ehs[e].offset_r_spec = self.ehs[e].offset_l_spec;
                }
            } else if self.vertex_only {
                let bme = self.ehs[e].e;
                match self.offset_type {
                    OffsetType::Offset => self.ehs[e].offset_l_spec = bv_offset,
                    OffsetType::Width => {
                        let v2 = self.bm.edge_other_vert(bme, v);
                        let edge_dir = sub(self.co(v), self.co(v2));
                        let z = (2.0 * angle_v3v3(vert_axis, edge_dir).sin()).abs();
                        self.ehs[e].offset_l_spec = if z < BEVEL_EPSILON { 0.01 * self.offset } else { self.offset / z };
                    }
                    OffsetType::Depth => {
                        let v2 = self.bm.edge_other_vert(bme, v);
                        let edge_dir = sub(self.co(v), self.co(v2));
                        let z = angle_v3v3(vert_axis, edge_dir).cos().abs();
                        self.ehs[e].offset_l_spec = if z < BEVEL_EPSILON { 0.01 * self.offset } else { self.offset / z };
                    }
                    OffsetType::Percent => self.ehs[e].offset_l_spec = self.bm.edge_calc_length(bme) * bv_offset / 100.0,
                    OffsetType::Absolute => self.ehs[e].offset_l_spec = bv_offset,
                }
                self.ehs[e].offset_r_spec = self.ehs[e].offset_l_spec;
            } else {
                self.ehs[e].offset_l_spec = 0.0;
                self.ehs[e].offset_r_spec = 0.0;
            }
            self.ehs[e].offset_l = self.ehs[e].offset_l_spec;
            self.ehs[e].offset_r = self.ehs[e].offset_r_spec;
            // contig_ldata_across_edge is true without loop layers: seam only when a face is missing
            self.ehs[e].is_seam = !(self.ehs[e].fprev != NONE && self.ehs[e].fnext != NONE);
        }
        if tot_wire != 0 {
            let mut w = vec![];
            for &bme in &edges {
                if self.bm.edge_is_wire(bme) {
                    w.push(bme);
                }
            }
            self.bvs[bv].wire_edges = w;
        }
        bv
    }
}

// ------------------------------------------------------------------ rebuild, edge polygons, profile spacing, entry
impl<'a> Bp<'a> {
    fn bev_rebuild_polygon(&mut self, f: usize) -> bool {
        let mut do_rebuild = false;
        let mut vv: Vec<usize> = vec![];
        let mut ee: Vec<usize> = vec![];
        for l in self.bm.loops_of_face(f) {
            let lv = self.bm.loops[l].v;
            if self.tag(lv) {
                let lprev = self.bm.loops[l].p;
                let bv = self.find_bevvert(lv);
                let vm = self.bvs[bv].vmesh;
                let e = self.find_edge_half(bv, self.bm.loops[l].e);
                let bme = self.ehs[e].e;
                let eprev = self.find_edge_half(bv, self.bm.loops[lprev].e);
                let go_ccw;
                if self.ehs[e].prev == eprev {
                    if self.ehs[eprev].prev == e {
                        go_ccw = self.ehs[e].fnext != f;
                    } else {
                        go_ccw = true;
                    }
                } else if self.ehs[eprev].prev == e {
                    go_ccw = false;
                } else {
                    go_ccw = self.count_ccw_edges_between(eprev, e) < self.count_ccw_edges_between(e, eprev);
                }
                let mut on_profile_start = false;
                let mut vstart;
                let vend;
                if go_ccw {
                    vstart = self.ehs[eprev].rightv;
                    vend = self.ehs[e].leftv;
                    if self.ehs[e].profile_index > 0 {
                        vstart = self.bounds[vstart].prev;
                        on_profile_start = true;
                    }
                } else {
                    vstart = self.ehs[eprev].leftv;
                    vend = self.ehs[e].rightv;
                    if self.ehs[eprev].profile_index > 0 {
                        vstart = self.bounds[vstart].next;
                        on_profile_start = true;
                    }
                }
                let mut v = vstart;
                if !on_profile_start {
                    vv.push(self.bounds[v].nv.v);
                    ee.push(bme);
                }
                while v != vend {
                    let vmseg = self.vms[vm].seg;
                    if go_ccw {
                        let i = self.bounds[v].index;
                        let kstart;
                        if on_profile_start {
                            kstart = self.ehs[e].profile_index;
                            on_profile_start = false;
                        } else {
                            kstart = 1;
                        }
                        let kend = if self.ehs[eprev].rightv == v && self.ehs[eprev].profile_index > 0 { self.ehs[eprev].profile_index } else { vmseg };
                        for k in kstart..=kend {
                            let bmv = self.mv(vm, i, 0, k).v;
                            if bmv != NONE {
                                vv.push(bmv);
                                ee.push(bme);
                            }
                        }
                        v = self.bounds[v].next;
                    } else {
                        let i = self.bounds[self.bounds[v].prev].index;
                        let kstart;
                        if on_profile_start {
                            kstart = self.ehs[eprev].profile_index;
                            on_profile_start = false;
                        } else {
                            kstart = vmseg - 1;
                        }
                        let kend = if self.ehs[e].rightv == self.bounds[v].prev && self.ehs[e].profile_index > 0 { self.ehs[e].profile_index } else { 0 };
                        let mut k = kstart;
                        while k >= kend {
                            let bmv = self.mv(vm, i, 0, k).v;
                            if bmv != NONE {
                                vv.push(bmv);
                                ee.push(bme);
                            }
                            k -= 1;
                        }
                        v = self.bounds[v].prev;
                    }
                }
                do_rebuild = true;
            } else {
                vv.push(lv);
                ee.push(self.bm.loops[l].e);
            }
        }
        if do_rebuild {
            let n = vv.len();
            let f_new = self.bev_create_ngon(&vv, None, f, -1);
            for k in 0..n {
                let bme_new = self.bm.edge_exists(vv[k], vv[(k + 1) % n]);
                if bme_new != NONE && ee[k] != bme_new {
                    self.bm.elem_attrs_copy_edge(ee[k], bme_new);
                }
            }
            if f_new != NONE {
                self.record_face_kind(f_new, FKind::Recon);
                self.bm.faces[f_new].flag &= !ELEM_TAG;
            }
        }
        do_rebuild
    }

    fn bevel_rebuild_existing_polygons(&mut self, v: usize) {
        let faces = self.bm.faces_of_vert(v);
        for f in faces {
            if self.bev_rebuild_polygon(f) {
                self.bm.face_kill(f);
            }
        }
    }

    fn bevel_reattach_wires(&mut self, v: usize) {
        let bv = self.find_bevvert(v);
        if bv == NONE || self.bvs[bv].wirecount == 0 {
            return;
        }
        let wires = self.bvs[bv].wire_edges.clone();
        let vm = self.bvs[bv].vmesh;
        for e in wires {
            let mut vclosest = NONE;
            let mut dclosest = f32::MAX;
            let mut votherclosest = NONE;
            let vother = self.bm.edge_other_vert(e, v);
            let mut bvother = NONE;
            if self.tag(vother) {
                bvother = self.find_bevvert(vother);
                if bvother == NONE {
                    return;
                }
            }
            let start = self.vms[vm].boundstart;
            let mut bndv = start;
            loop {
                if bvother != NONE {
                    let vmo = self.bvs[bvother].vmesh;
                    let so = self.vms[vmo].boundstart;
                    let mut bo = so;
                    loop {
                        let d = len_sq(sub(self.bounds[bo].nv.co, self.bounds[bndv].nv.co));
                        if d < dclosest {
                            vclosest = self.bounds[bndv].nv.v;
                            votherclosest = self.bounds[bo].nv.v;
                            dclosest = d;
                        }
                        bo = self.bounds[bo].next;
                        if bo == so {
                            break;
                        }
                    }
                } else {
                    let d = len_sq(sub(self.co(vother), self.bounds[bndv].nv.co));
                    if d < dclosest {
                        vclosest = self.bounds[bndv].nv.v;
                        votherclosest = vother;
                        dclosest = d;
                    }
                }
                bndv = self.bounds[bndv].next;
                if bndv == start {
                    break;
                }
            }
            if vclosest != NONE {
                self.bm.edge_create(vclosest, votherclosest, Some(e), true);
            }
        }
    }

    fn bevel_build_edge_polygons(&mut self, bme: usize) {
        let mat_nr = self.mat_nr;
        if !self.bm.edge_is_manifold(bme) {
            return;
        }
        let bv1 = self.find_bevvert(self.bm.edges[bme].v1);
        let bv2 = self.find_bevvert(self.bm.edges[bme].v2);
        let e1 = self.find_edge_half(bv1, bme);
        let e2 = self.find_edge_half(bv2, bme);
        let nseg = self.ehs[e1].seg;
        let bmv1 = self.bounds[self.ehs[e1].leftv].nv.v;
        let bmv4 = self.bounds[self.ehs[e1].rightv].nv.v;
        let bmv2 = self.bounds[self.ehs[e2].rightv].nv.v;
        let bmv3 = self.bounds[self.ehs[e2].leftv].nv.v;
        let f1 = self.ehs[e1].fprev;
        let f2 = self.ehs[e1].fnext;
        let faces = [f1, f1, f2, f2];
        let i1 = self.bounds[self.ehs[e1].leftv].index;
        let i2 = self.bounds[self.ehs[e2].leftv].index;
        let vm1 = self.bvs[bv1].vmesh;
        let vm2 = self.bvs[bv2].vmesh;
        let mut verts = [bmv1, bmv2, NONE, NONE];
        let odd = nseg % 2;
        let mid = nseg / 2;
        // no seams / UV layers: is_seam paths are not reachable for manifold edges
        for k in 1..=nseg {
            verts[3] = self.mv(vm1, i1, 0, k).v;
            verts[2] = self.mv(vm2, i2, 0, nseg - k).v;
            let r_f;
            if odd == 1 && k == mid + 1 {
                r_f = self.bev_create_ngon(&verts, Some(&faces), NONE, mat_nr);
            } else if odd == 0 && k == mid {
                r_f = self.bev_create_ngon(&verts, None, f1, mat_nr);
            } else if odd == 0 && k == mid + 1 {
                r_f = self.bev_create_ngon(&verts, None, f2, mat_nr);
            } else {
                let f = if k <= mid { f1 } else { f2 };
                r_f = self.bev_create_ngon(&verts, None, f, mat_nr);
            }
            if r_f != NONE {
                self.record_face_kind(r_f, FKind::Edge);
            }
            verts[0] = verts[3];
            verts[1] = verts[2];
        }
        let bme1 = self.bm.edge_exists(bmv1, bmv2);
        let bme2 = self.bm.edge_exists(bmv3, bmv4);
        if bme1 != NONE {
            self.bm.elem_attrs_copy_edge(bme, bme1);
        }
        if bme2 != NONE {
            self.bm.elem_attrs_copy_edge(bme, bme2);
        }
    }

    fn find_superellipse_chord_endpoint(x0: f64, dtarget: f64, r: f32, rbig: bool) -> f64 {
        let y0 = superellipse_co(x0, r, rbig);
        let tol = 1e-13;
        let maxiter = 10;
        let mut xmin = x0 + std::f64::consts::SQRT_2 / 2.0 * dtarget;
        if xmin > 1.0 {
            xmin = 1.0;
        }
        let mut xmax = x0 + dtarget;
        if xmax > 1.0 {
            xmax = 1.0;
        }
        let mut ymin = superellipse_co(xmin, r, rbig);
        let mut ymax = superellipse_co(xmax, r, rbig);
        let mut dmaxerr = ((xmax - x0).powi(2) + (ymax - y0).powi(2)).sqrt() - dtarget;
        let mut dminerr = ((xmin - x0).powi(2) + (ymin - y0).powi(2)).sqrt() - dtarget;
        let mut xnew = xmax - dmaxerr * (xmax - xmin) / (dmaxerr - dminerr);
        let mut lastupdated_upper = true;
        for _ in 0..maxiter {
            let ynew = superellipse_co(xnew, r, rbig);
            let dnewerr = ((xnew - x0).powi(2) + (ynew - y0).powi(2)).sqrt() - dtarget;
            if dnewerr.abs() < tol {
                break;
            }
            if dnewerr < 0.0 {
                xmin = xnew;
                ymin = ynew;
                dminerr = dnewerr;
                if !lastupdated_upper {
                    xnew = (dmaxerr / 2.0 * xmin - dminerr * xmax) / (dmaxerr / 2.0 - dminerr);
                } else {
                    xnew = xmax - dmaxerr * (xmax - xmin) / (dmaxerr - dminerr);
                }
                lastupdated_upper = false;
            } else {
                xmax = xnew;
                ymax = ynew;
                dmaxerr = dnewerr;
                if lastupdated_upper {
                    xnew = (dmaxerr * xmin - dminerr / 2.0 * xmax) / (dmaxerr - dminerr / 2.0);
                } else {
                    xnew = xmax - dmaxerr * (xmax - xmin) / (dmaxerr - dminerr);
                }
                lastupdated_upper = true;
            }
        }
        let _ = (ymin, ymax);
        xnew
    }

    fn find_even_superellipse_chords_general(seg: i32, r: f32, xvals: &mut [f64], yvals: &mut [f64]) {
        let smoothitermax = 10;
        let error_tol = 1e-7;
        let imax = ((seg + 1) / 2 - 1) as usize;
        let seg_odd = seg % 2 == 1;
        let rbig;
        let mx: f64;
        if r > 1.0 {
            rbig = true;
            mx = 0.5f64.powf(1.0 / r as f64);
        } else {
            rbig = false;
            mx = 1.0 - 0.5f64.powf(1.0 / r as f64);
        }
        for i in 0..=imax {
            xvals[i] = i as f64 * mx / seg as f64 * 2.0;
            yvals[i] = superellipse_co(xvals[i], r, rbig);
        }
        yvals[0] = 1.0;
        for _ in 0..smoothitermax {
            let mut sum = 0.0;
            let mut dmin = 2.0f64;
            let mut dmax = 0.0f64;
            for i in 0..imax {
                let d = ((xvals[i + 1] - xvals[i]).powi(2) + (yvals[i + 1] - yvals[i]).powi(2)).sqrt();
                sum += d;
                if d > dmax {
                    dmax = d;
                }
                if d < dmin {
                    dmin = d;
                }
            }
            let davg;
            if seg_odd {
                sum += std::f64::consts::SQRT_2 / 2.0 * (yvals[imax] - xvals[imax]);
                davg = sum / (imax as f64 + 0.5);
            } else {
                sum += ((xvals[imax] - mx).powi(2) + (yvals[imax] - mx).powi(2)).sqrt();
                davg = sum / (imax as f64 + 1.0);
            }
            let mut precision_reached = true;
            if dmax - davg > error_tol {
                precision_reached = false;
            }
            if dmin - davg < error_tol {
                precision_reached = false;
            }
            if precision_reached {
                break;
            }
            for i in 1..=imax {
                xvals[i] = Self::find_superellipse_chord_endpoint(xvals[i - 1], davg, r, rbig);
                yvals[i] = superellipse_co(xvals[i], r, rbig);
            }
        }
        if !seg_odd {
            xvals[imax + 1] = mx;
            yvals[imax + 1] = mx;
        }
        let seg = seg as usize;
        for i in imax + 1..=seg {
            yvals[i] = xvals[seg - i];
            xvals[i] = yvals[seg - i];
        }
        if !rbig {
            for i in 0..=seg {
                let temp = xvals[i];
                xvals[i] = 1.0 - yvals[i];
                yvals[i] = 1.0 - temp;
            }
        }
    }

    fn find_even_superellipse_chords(n: i32, r: f32, xvals: &mut [f64], yvals: &mut [f64]) {
        let seg_odd = n % 2 == 1;
        let n2 = (n / 2) as usize;
        let nn = n as usize;
        let sq = std::f64::consts::SQRT_2;
        if r == PRO_LINE_R {
            for i in 0..=nn {
                xvals[i] = i as f64 / n as f64;
                yvals[i] = 1.0 - i as f64 / n as f64;
            }
            return;
        }
        if r == PRO_CIRCLE_R {
            let temp = std::f64::consts::FRAC_PI_2 / n as f64;
            for i in 0..=nn {
                xvals[i] = (i as f64 * temp).sin();
                yvals[i] = (i as f64 * temp).cos();
            }
            return;
        }
        if r == PRO_SQUARE_IN_R {
            if !seg_odd {
                for i in 0..=n2 {
                    xvals[i] = 0.0;
                    yvals[i] = 1.0 - i as f64 / n2 as f64;
                    xvals[nn - i] = yvals[i];
                    yvals[nn - i] = xvals[i];
                }
            } else {
                let temp = 1.0 / (n2 as f64 + sq / 2.0);
                for i in 0..=n2 {
                    xvals[i] = 0.0;
                    yvals[i] = 1.0 - i as f64 * temp;
                    xvals[nn - i] = yvals[i];
                    yvals[nn - i] = xvals[i];
                }
            }
            return;
        }
        if r == PRO_SQUARE_R {
            if !seg_odd {
                for i in 0..=n2 {
                    xvals[i] = i as f64 / n2 as f64;
                    yvals[i] = 1.0;
                    xvals[nn - i] = yvals[i];
                    yvals[nn - i] = xvals[i];
                }
            } else {
                let temp = 1.0 / (n2 as f64 + sq / 2.0);
                for i in 0..=n2 {
                    xvals[i] = i as f64 * temp;
                    yvals[i] = 1.0;
                    xvals[nn - i] = yvals[i];
                    yvals[nn - i] = xvals[i];
                }
            }
            return;
        }
        Self::find_even_superellipse_chords_general(n, r, xvals, yvals);
    }

    fn find_profile_fullness(&self) -> f32 {
        let nseg = self.seg;
        const CF: [f32; 11] = [0.0, 0.559, 0.642, 0.551, 0.646, 0.624, 0.646, 0.619, 0.647, 0.639, 0.647];
        if self.pro_super_r == PRO_LINE_R {
            0.0
        } else if self.pro_super_r == PRO_CIRCLE_R && nseg > 0 && nseg <= 11 {
            CF[(nseg - 1) as usize]
        } else if nseg % 2 == 0 {
            2.4506f32 * self.profile - 0.00000300f32 * nseg as f32 - 0.6266f32
        } else {
            2.3635f32 * self.profile + 0.000152f32 * nseg as f32 - 0.6060f32
        }
    }

    fn set_profile_spacing(&mut self) {
        let seg = self.seg;
        if seg <= 1 {
            return;
        }
        let seg_2 = power_of_2_max_i(seg).max(4);
        self.pro_spacing.seg_2 = seg_2;
        let mut x2 = vec![0.0f64; seg_2 as usize + 1];
        let mut y2 = vec![0.0f64; seg_2 as usize + 1];
        Self::find_even_superellipse_chords(seg_2, self.pro_super_r, &mut x2, &mut y2);
        self.pro_spacing.xvals_2 = x2;
        self.pro_spacing.yvals_2 = y2;
        let mut x = vec![0.0f64; seg as usize + 1];
        let mut y = vec![0.0f64; seg as usize + 1];
        Self::find_even_superellipse_chords(seg, self.pro_super_r, &mut x, &mut y);
        self.pro_spacing.xvals = x;
        self.pro_spacing.yvals = y;
    }

    fn run(&mut self) -> Result<(), String> {
        let vids = self.bm.vert_ids();
        for &v in &vids {
            if self.tag(v) {
                let bv = self.bevel_vert_construct(v);
                if bv != NONE && !self.limit_offset {
                    self.build_boundary(bv, true);
                }
            }
        }
        if self.limit_offset {
            self.bevel_limit_offset();
            for &v in &vids {
                if self.tag(v) {
                    let bv = self.find_bevvert(v);
                    if bv != NONE {
                        self.build_boundary(bv, true);
                    }
                }
            }
        }
        if self.offset_adjust {
            self.adjust_offsets();
        }
        let vids = self.bm.vert_ids();
        for &v in &vids {
            if self.tag(v) {
                let bv = self.find_bevvert(v);
                if bv != NONE {
                    self.build_vmesh(bv)?;
                }
            }
        }
        for e in self.bm.edge_ids() {
            if self.bm.edges[e].flag & ELEM_TAG != 0 && !self.vertex_only {
                self.bevel_build_edge_polygons(e);
            }
        }
        let vids = self.bm.vert_ids();
        for &v in &vids {
            if self.tag(v) {
                self.bevel_rebuild_existing_polygons(v);
                self.bevel_reattach_wires(v);
            }
        }
        for v in self.bm.vert_ids() {
            if self.tag(v) {
                self.bm.vert_kill(v);
            }
        }
        if let Some(u) = &self.unsupported {
            return Err(u.clone());
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ clamp overlap (bevel_limit_offset)
impl<'a> Bp<'a> {
    fn eh_vb_vc(&self, eb: usize) -> (usize, usize) {
        let e = &self.bm.edges[self.ehs[eb].e];
        if self.ehs[eb].is_rev {
            (e.v2, e.v1)
        } else {
            (e.v1, e.v2)
        }
    }

    fn vertex_collide_offset(&self, ea: usize) -> f32 {
        let no_collide_offset = self.offset + 1e6f32;
        if self.offset == 0.0 {
            return no_collide_offset;
        }
        let ka = self.ehs[ea].offset_l_spec / self.offset;
        let (eb, _) = self.find_other_end_edge_half(ea);
        let kb = if eb != NONE { self.ehs[eb].offset_l_spec / self.offset } else { 0.0 };
        let kab = ka + kb;
        let la = self.bm.edge_calc_length(self.ehs[ea].e);
        if kab <= 0.0 {
            return no_collide_offset;
        }
        la / kab
    }

    fn geometry_collide_offset(&self, eb: usize) -> f32 {
        let no_collide_offset = self.offset + 1e6f32;
        let mut limit = no_collide_offset;
        if self.offset == 0.0 {
            return no_collide_offset;
        }
        let kb0 = self.ehs[eb].offset_l_spec;
        let ea = self.ehs[eb].next;
        let ka0 = self.ehs[ea].offset_r_spec;
        let (vb, mut vc) = self.eh_vb_vc(eb);
        let ea_e = &self.bm.edges[self.ehs[ea].e];
        let va = if self.ehs[ea].is_rev { ea_e.v1 } else { ea_e.v2 };
        let (ebother, bvc) = self.find_other_end_edge_half(eb);
        let (ec, vd, mut kc);
        if matches!(self.offset_type, OffsetType::Percent | OffsetType::Absolute) {
            if self.ehs[ea].is_bev && ebother != NONE && self.ehs[self.ehs[ebother].prev].is_bev {
                if self.offset_type == OffsetType::Percent {
                    return 50.0;
                }
                let blen = self.bm.edge_calc_length(self.ehs[eb].e);
                return if self.offset > blen / 2.0 { blen / 2.0 } else { blen };
            }
            return no_collide_offset;
        }
        if ebother != NONE {
            ec = self.ehs[ebother].prev;
            vc = self.bvs[bvc].v;
            kc = self.ehs[ec].offset_l_spec;
            let ece = &self.bm.edges[self.ehs[ec].e];
            vd = if self.ehs[ec].is_rev { ece.v1 } else { ece.v2 };
        } else {
            kc = 0.0;
            ec = NONE;
            let fnext = self.ehs[eb].fnext;
            if fnext == NONE {
                return no_collide_offset;
            }
            let lb = self.bm.face_edge_share_loop(fnext, self.ehs[eb].e);
            if lb == NONE {
                return no_collide_offset;
            }
            let lbn = self.bm.loops[lb].n;
            if self.bm.loops[lbn].v == vc {
                vd = self.bm.loops[self.bm.loops[lbn].n].v;
            } else if self.bm.loops[lb].v == vc {
                vd = self.bm.loops[self.bm.loops[lb].p].v;
            } else {
                return no_collide_offset;
            }
        }
        if self.ehs[ea].e == self.ehs[eb].e || (ec != NONE && self.ehs[ec].e == self.ehs[eb].e) {
            return no_collide_offset;
        }
        let ka = ka0 / self.offset;
        let kb = kb0 / self.offset;
        kc /= self.offset;
        let th1 = angle_v3v3v3(self.co(va), self.co(vb), self.co(vc));
        let th2 = angle_v3v3v3(self.co(vb), self.co(vc), self.co(vd));
        let sin1 = th1.sin();
        let sin2 = th2.sin();
        if std::env::var("BEVDBG2").is_ok() {
            eprintln!("  gco eb={} ka={} kb={} kc={} th1={:e} th2={:e} tan1={:e} tan2={:e} lenB={:e}", eb, ka, kb, kc, th1, th2, th1.tan(), th2.tan(), self.bm.edge_calc_length(self.ehs[eb].e));
        }
        if (ka > 0.0) as i32 + (kb > 0.0) as i32 + (kc > 0.0) as i32 >= 2 {
            let tan1 = th1.tan();
            let tan2 = th2.tan();
            let g = tan1 * tan2;
            let h = sin1 * sin2;
            let den = g * (ka * sin2 + kc * sin1) + kb * h * (tan1 + tan2);
            if den != 0.0 {
                let mut t = self.bm.edge_calc_length(self.ehs[eb].e);
                t *= g * h / den;
                if t >= 0.0 {
                    limit = t;
                }
            }
        }
        if kb > 0.0 && ka == 0.0 {
            let mut t = self.bm.edge_calc_length(self.ehs[ea].e);
            t *= sin1 / kb;
            if t >= 0.0 && t < limit {
                limit = t;
            }
        }
        if kb > 0.0 && kc == 0.0 {
            let mut t = self.bm.edge_calc_length(self.ehs[ec].e);
            t *= sin2 / kb;
            if t >= 0.0 && t < limit {
                limit = t;
            }
        }
        limit
    }

    fn bevel_limit_offset(&mut self) {
        let mut limited_offset = self.offset;
        let vids = self.bm.vert_ids();
        for &v in &vids {
            if !self.tag(v) {
                continue;
            }
            let bv = self.find_bevvert(v);
            if bv == NONE {
                continue;
            }
            for i in 0..self.bvs[bv].edgecount {
                let c = if self.vertex_only { self.vertex_collide_offset(self.bvs[bv].e0 + i) } else { self.geometry_collide_offset(self.bvs[bv].e0 + i) };
                if std::env::var("BEVDBG").is_ok() && c < self.offset {
                    eprintln!("collide v{} i{} c={:e} co={:?}", v, i, c, self.bm.verts[v].co);
                }
                if c < limited_offset {
                    limited_offset = c;
                }
            }
        }
        if limited_offset < self.offset {
            let f = limited_offset / self.offset;
            for &v in &vids {
                if !self.tag(v) {
                    continue;
                }
                let bv = self.find_bevvert(v);
                if bv == NONE {
                    continue;
                }
                for i in 0..self.bvs[bv].edgecount {
                    let e = self.bvs[bv].e0 + i;
                    self.ehs[e].offset_l_spec *= f;
                    self.ehs[e].offset_r_spec *= f;
                    self.ehs[e].offset_l *= f;
                    self.ehs[e].offset_r *= f;
                }
            }
            self.offset = limited_offset;
        }
    }
}

/// Tag-based entry (BM_mesh_bevel). Callers set ELEM_TAG on verts/edges first.
pub fn bm_mesh_bevel(bm: &mut BMesh, o: &BevelOpts) -> Result<(), String> {
    bm_mesh_bevel_ex(bm, o, false)
}

/// `vertex_only`: affect = VERTICES (callers tag the vertices to bevel, no edges)
pub fn bm_mesh_bevel_ex(bm: &mut BMesh, o: &BevelOpts, vertex_only: bool) -> Result<(), String> {
    if o.offset <= 0.0 {
        return Ok(());
    }
    let mut pro_super_r = -(2.0f32).ln() / o.profile.sqrt().ln();
    if o.profile >= 0.950 {
        pro_super_r = PRO_SQUARE_R;
    } else if (pro_super_r - PRO_CIRCLE_R).abs() < 1e-4 {
        pro_super_r = PRO_CIRCLE_R;
    } else if (pro_super_r - PRO_LINE_R).abs() < 1e-4 {
        pro_super_r = PRO_LINE_R;
    } else if pro_super_r < 1e-4 {
        pro_super_r = PRO_SQUARE_IN_R;
    }
    let mut bp = Bp {
        bm,
        offset: o.offset,
        offset_type: o.offset_type,
        seg: o.segments.max(1),
        profile: o.profile,
        pro_super_r,
        loop_slide: o.loop_slide,
        limit_offset: o.limit_offset,
        offset_adjust: !vertex_only && !matches!(o.offset_type, OffsetType::Percent | OffsetType::Absolute),
        vertex_only,
        mat_nr: o.mat_nr,
        pro_spacing: ProfileSpacing::default(),
        ehs: vec![],
        bvs: vec![],
        bounds: vec![],
        vms: vec![],
        vert_hash: HashMap::default(),
        face_hash: HashMap::default(),
        unsupported: None,
    };
    bp.set_profile_spacing();
    if bp.seg > 1 {
        bp.pro_spacing.fullness = bp.find_profile_fullness();
    }
    bp.run()
}

/// bmo_bevel_exec tagging: `edges` selected (affect EDGES); verts are the selected edges' verts.
pub fn bevel_edges(bm: &mut BMesh, edges: &[usize], o: &BevelOpts) -> Result<(), String> {
    for v in bm.vert_ids() {
        bm.verts[v].flag &= !ELEM_TAG;
    }
    for e in bm.edge_ids() {
        bm.edges[e].flag &= !ELEM_TAG;
    }
    for f in bm.face_ids() {
        bm.faces[f].flag &= !ELEM_TAG;
    }
    for &e in edges {
        let (a, b) = (bm.edges[e].v1, bm.edges[e].v2);
        bm.verts[a].flag |= ELEM_TAG;
        bm.verts[b].flag |= ELEM_TAG;
    }
    for &e in edges {
        if bm.edge_is_manifold(e) {
            bm.edges[e].flag |= ELEM_TAG;
        }
    }
    bm_mesh_bevel(bm, o)
}
