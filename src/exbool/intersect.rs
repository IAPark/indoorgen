//! Port of Blender 4.2 `bli_mesh_intersect.cc` (trimesh_nary_intersect and triangulate_polymesh).

use crate::exbool::cdt_q::*;
use crate::exbool::hashtab::*;
use crate::exbool::imesh::*;
use crate::exbool::mpq::*;
use crate::exbool::polyfill;
use num_traits::{One, Signed, Zero};
use std::collections::HashMap;

const DBL_EPSILON: f64 = f64::EPSILON;
const FLT_EPSILON: f32 = f32::EPSILON;
const FLT_MAX: f32 = f32::MAX;

// ---------------------------------------------------------------------------------------------------------------------
// Bounding boxes
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BoundingBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
impl BoundingBox {
    pub fn new() -> Self {
        BoundingBox { min: [FLT_MAX; 3], max: [-FLT_MAX; 3] }
    }
    pub fn combine_d(&mut self, p: &[f64; 3]) {
        for i in 0..3 {
            let f = p[i] as f32;
            if f < self.min[i] {
                self.min[i] = f;
            }
            if f > self.max[i] {
                self.max[i] = f;
            }
        }
    }
    pub fn combine_bb(&mut self, o: &BoundingBox) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(o.min[i]);
            self.max[i] = self.max[i].max(o.max[i]);
        }
    }
    pub fn expand(&mut self, pad: f32) {
        for i in 0..3 {
            self.min[i] -= pad;
            self.max[i] += pad;
        }
    }
}

/// isect_aabb_aabb_v3
pub fn bbs_might_intersect(a: &BoundingBox, b: &BoundingBox) -> bool {
    a.min[0] < b.max[0] && a.max[0] > b.min[0] && a.min[1] < b.max[1] && a.max[1] > b.min[1] && a.min[2] < b.max[2] && a.max[2] > b.min[2]
}

fn calc_face_bounding_boxes(arena: &Arena, tm: &[usize]) -> Vec<BoundingBox> {
    let mut ans = vec![BoundingBox::new(); tm.len()];
    let mut max_abs_val = 0.0f64;
    for (i, &fi) in tm.iter().enumerate() {
        for &v in &arena.faces[fi].vert {
            let d = &arena.verts[v as usize].d;
            ans[i].combine_d(d);
            for k in 0..3 {
                max_abs_val = max_abs_val.max(d[k].abs());
            }
        }
    }
    let pad_factor = 10.0f32;
    let mut pad: f32 = if max_abs_val == 0.0 { FLT_EPSILON } else { ((2.0f32 * FLT_EPSILON) as f64 * max_abs_val) as f32 };
    pad *= pad_factor;
    for bb in ans.iter_mut() {
        bb.expand(pad);
    }
    ans
}

// ---------------------------------------------------------------------------------------------------------------------
// Coplanar clusters
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct CoplanarCluster {
    tris: Vec<usize>,
    bb: BoundingBox,
}
impl CoplanarCluster {
    fn new_one(t: usize, bb: &BoundingBox) -> Self {
        let mut c = CoplanarCluster { tris: vec![], bb: BoundingBox::new() };
        c.add_tri(t, bb);
        c
    }
    fn add_tri(&mut self, t: usize, bb: &BoundingBox) {
        self.tris.push(t);
        self.bb.combine_bb(bb);
    }
}

struct CoplanarClusterInfo {
    clusters: Vec<CoplanarCluster>,
    tri_cluster: Vec<i32>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Triangle-triangle intersection
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IKind {
    None,
    Point,
    Segment,
    Coplanar,
}

#[derive(Clone)]
pub struct Itt {
    pub kind: IKind,
    pub p1: V3q,
    pub p2: V3q,
    pub t_source: i32,
}
impl Itt {
    fn none() -> Itt {
        Itt { kind: IKind::None, p1: [q_zero(), q_zero(), q_zero()], p2: [q_zero(), q_zero(), q_zero()], t_source: -1 }
    }
}

fn supremum_dot_cross(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let abs_a = [a[0].abs(), a[1].abs(), a[2].abs()];
    let abs_b = [b[0].abs(), b[1].abs(), b[2].abs()];
    let c0 = abs_a[1] * abs_b[2] + abs_a[2] * abs_b[1];
    let c1 = abs_a[2] * abs_b[0] + abs_a[0] * abs_b[2];
    let c2 = abs_a[0] * abs_b[1] + abs_a[1] * abs_b[0];
    c0 * c0 + c1 * c1 + c2 * c2
}

const INDEX_DOT_CROSS: f64 = 11.0;
const INDEX_PLANE_SIDE: f64 = (3 + 2 * 15) as f64;

fn filter_plane_side(p: &[f64; 3], plane_p: &[f64; 3], plane_no: &[f64; 3], abs_p: &[f64; 3], abs_plane_p: &[f64; 3], abs_plane_no: &[f64; 3]) -> i32 {
    let d = dot3d(&sub3d(p, plane_p), plane_no);
    if d == 0.0 {
        return 0;
    }
    let sum = [abs_p[0] + abs_plane_p[0], abs_p[1] + abs_plane_p[1], abs_p[2] + abs_plane_p[2]];
    let supremum = dot3d(&sum, abs_plane_no);
    let err_bound = supremum * INDEX_PLANE_SIDE * DBL_EPSILON;
    if d.abs() > err_bound {
        return if d > 0.0 { 1 } else { -1 };
    }
    0
}

fn abs3(a: &[f64; 3]) -> [f64; 3] {
    [a[0].abs(), a[1].abs(), a[2].abs()]
}

fn sgn(q: &Q) -> i32 {
    q_sgn(q)
}

fn tti_interp(a: &V3q, b: &V3q, c: &V3q, n: &V3q) -> V3q {
    let ab = q3_sub(a, b);
    let ac = q3_sub(a, c);
    let den = q3_dot(&ab, n);
    let alpha = q3_dot(&ac, n) / den;
    [&a[0] - &alpha * &ab[0], &a[1] - &alpha * &ab[1], &a[2] - &alpha * &ab[2]]
}

fn tti_above(a: &V3q, b: &V3q, c: &V3q, ad: &V3q) -> i32 {
    let ba = q3_sub(b, a);
    let ca = q3_sub(c, a);
    let n = [&ba[1] * &ca[2] - &ba[2] * &ca[1], &ba[2] * &ca[0] - &ba[0] * &ca[2], &ba[0] * &ca[1] - &ba[1] * &ca[0]];
    sgn(&q3_dot(ad, &n))
}

fn itt_canon2(p1: &V3q, q1: &V3q, r1: &V3q, p2: &V3q, q2: &V3q, r2: &V3q, n1: &V3q, n2: &V3q) -> Itt {
    let p1p2 = q3_sub(p2, p1);
    let intersect_1: V3q;
    let intersect_2: V3q;
    if tti_above(p1, q1, r2, &p1p2) > 0 {
        if tti_above(p1, r1, r2, &p1p2) <= 0 {
            if tti_above(p1, r1, q2, &p1p2) > 0 {
                intersect_1 = tti_interp(p1, r1, p2, n2);
                intersect_2 = tti_interp(p2, r2, p1, n1);
            } else {
                intersect_1 = tti_interp(p2, q2, p1, n1);
                intersect_2 = tti_interp(p2, r2, p1, n1);
            }
        } else {
            return Itt::none();
        }
    } else if tti_above(p1, q1, q2, &p1p2) < 0 {
        return Itt::none();
    } else if tti_above(p1, r1, q2, &p1p2) >= 0 {
        intersect_1 = tti_interp(p1, r1, p2, n2);
        intersect_2 = tti_interp(p1, q1, p2, n2);
    } else {
        intersect_1 = tti_interp(p2, q2, p1, n1);
        intersect_2 = tti_interp(p1, q1, p2, n2);
    }
    if intersect_1 == intersect_2 {
        return Itt { kind: IKind::Point, p1: intersect_1, p2: [q_zero(), q_zero(), q_zero()], t_source: -1 };
    }
    Itt { kind: IKind::Segment, p1: intersect_1, p2: intersect_2, t_source: -1 }
}

fn itt_canon1(p1: &V3q, q1: &V3q, r1: &V3q, p2: &V3q, q2: &V3q, r2: &V3q, n1: &V3q, n2: &V3q, sp2: i32, sq2: i32, sr2: i32) -> Itt {
    if sp2 > 0 {
        if sq2 > 0 {
            return itt_canon2(p1, r1, q1, r2, p2, q2, n1, n2);
        }
        if sr2 > 0 {
            return itt_canon2(p1, r1, q1, q2, r2, p2, n1, n2);
        }
        return itt_canon2(p1, q1, r1, p2, q2, r2, n1, n2);
    }
    if sp2 < 0 {
        if sq2 < 0 {
            return itt_canon2(p1, q1, r1, r2, p2, q2, n1, n2);
        }
        if sr2 < 0 {
            return itt_canon2(p1, q1, r1, q2, r2, p2, n1, n2);
        }
        return itt_canon2(p1, r1, q1, p2, q2, r2, n1, n2);
    }
    if sq2 < 0 {
        if sr2 >= 0 {
            return itt_canon2(p1, r1, q1, q2, r2, p2, n1, n2);
        }
        return itt_canon2(p1, q1, r1, p2, q2, r2, n1, n2);
    }
    if sq2 > 0 {
        if sr2 > 0 {
            return itt_canon2(p1, r1, q1, p2, q2, r2, n1, n2);
        }
        return itt_canon2(p1, q1, r1, q2, r2, p2, n1, n2);
    }
    if sr2 > 0 {
        return itt_canon2(p1, q1, r1, r2, p2, q2, n1, n2);
    }
    if sr2 < 0 {
        return itt_canon2(p1, r1, q1, r2, p2, q2, n1, n2);
    }
    Itt { kind: IKind::Coplanar, p1: [q_zero(), q_zero(), q_zero()], p2: [q_zero(), q_zero(), q_zero()], t_source: -1 }
}

fn intersect_tri_tri(arena: &Arena, f1: usize, f2: usize, t2: usize) -> Itt {
    let tri1 = &arena.faces[f1];
    let tri2 = &arena.faces[f2];
    let (vp1, vq1, vr1) = (&arena.verts[tri1.vert[0] as usize], &arena.verts[tri1.vert[1] as usize], &arena.verts[tri1.vert[2] as usize]);
    let (vp2, vq2, vr2) = (&arena.verts[tri2.vert[0] as usize], &arena.verts[tri2.vert[1] as usize], &arena.verts[tri2.vert[2] as usize]);
    let d_p1 = &vp1.d;
    let d_q1 = &vq1.d;
    let d_r1 = &vr1.d;
    let d_p2 = &vp2.d;
    let d_q2 = &vq2.d;
    let d_r2 = &vr2.d;
    let pl1 = tri1.plane.as_ref().unwrap();
    let pl2 = tri2.plane.as_ref().unwrap();
    let d_n2 = &pl2.norm;
    let abs_d_p1 = abs3(d_p1);
    let abs_d_q1 = abs3(d_q1);
    let abs_d_r1 = abs3(d_r1);
    let abs_d_r2 = abs3(d_r2);
    let abs_d_n2 = abs3(d_n2);
    let mut sp1 = filter_plane_side(d_p1, d_r2, d_n2, &abs_d_p1, &abs_d_r2, &abs_d_n2);
    let mut sq1 = filter_plane_side(d_q1, d_r2, d_n2, &abs_d_q1, &abs_d_r2, &abs_d_n2);
    let mut sr1 = filter_plane_side(d_r1, d_r2, d_n2, &abs_d_r1, &abs_d_r2, &abs_d_n2);
    if (sp1 > 0 && sq1 > 0 && sr1 > 0) || (sp1 < 0 && sq1 < 0 && sr1 < 0) {
        return Itt::none();
    }
    let d_n1 = &pl1.norm;
    let abs_d_p2 = abs3(d_p2);
    let abs_d_q2 = abs3(d_q2);
    let abs_d_n1 = abs3(d_n1);
    let mut sp2 = filter_plane_side(d_p2, d_r1, d_n1, &abs_d_p2, &abs_d_r1, &abs_d_n1);
    let mut sq2 = filter_plane_side(d_q2, d_r1, d_n1, &abs_d_q2, &abs_d_r1, &abs_d_n1);
    let mut sr2 = filter_plane_side(d_r2, d_r1, d_n1, &abs_d_r2, &abs_d_r1, &abs_d_n1);
    if (sp2 > 0 && sq2 > 0 && sr2 > 0) || (sp2 < 0 && sq2 < 0 && sr2 < 0) {
        return Itt::none();
    }

    let (p1, q1, r1) = (&vp1.co, &vq1.co, &vr1.co);
    let (p2, q2, r2) = (&vp2.co, &vq2.co, &vr2.co);
    let n2 = &pl2.norm_exact;
    if sp1 == 0 {
        sp1 = sgn(&q3_dot(&q3_sub(p1, r2), n2));
    }
    if sq1 == 0 {
        sq1 = sgn(&q3_dot(&q3_sub(q1, r2), n2));
    }
    if sr1 == 0 {
        sr1 = sgn(&q3_dot(&q3_sub(r1, r2), n2));
    }
    if sp1 * sq1 > 0 && sp1 * sr1 > 0 {
        return Itt::none();
    }
    let n1 = &pl1.norm_exact;
    if sp2 == 0 {
        sp2 = sgn(&q3_dot(&q3_sub(p2, r1), n1));
    }
    if sq2 == 0 {
        sq2 = sgn(&q3_dot(&q3_sub(q2, r1), n1));
    }
    if sr2 == 0 {
        sr2 = sgn(&q3_dot(&q3_sub(r2, r1), n1));
    }
    if sp2 * sq2 > 0 && sp2 * sr2 > 0 {
        return Itt::none();
    }

    let mut ans: Itt;
    if sp1 > 0 {
        if sq1 > 0 {
            ans = itt_canon1(r1, p1, q1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
        } else if sr1 > 0 {
            ans = itt_canon1(q1, r1, p1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
        } else {
            ans = itt_canon1(p1, q1, r1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
        }
    } else if sp1 < 0 {
        if sq1 < 0 {
            ans = itt_canon1(r1, p1, q1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
        } else if sr1 < 0 {
            ans = itt_canon1(q1, r1, p1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
        } else {
            ans = itt_canon1(p1, q1, r1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
        }
    } else if sq1 < 0 {
        if sr1 >= 0 {
            ans = itt_canon1(q1, r1, p1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
        } else {
            ans = itt_canon1(p1, q1, r1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
        }
    } else if sq1 > 0 {
        if sr1 > 0 {
            ans = itt_canon1(p1, q1, r1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
        } else {
            ans = itt_canon1(q1, r1, p1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
        }
    } else if sr1 > 0 {
        ans = itt_canon1(r1, p1, q1, p2, q2, r2, n1, n2, sp2, sq2, sr2);
    } else if sr1 < 0 {
        ans = itt_canon1(r1, p1, q1, p2, r2, q2, n1, n2, sp2, sr2, sq2);
    } else {
        ans = Itt { kind: IKind::Coplanar, p1: [q_zero(), q_zero(), q_zero()], p2: [q_zero(), q_zero(), q_zero()], t_source: -1 };
    }
    if ans.kind == IKind::Coplanar {
        ans.t_source = t2 as i32;
    }
    ans
}

// ---------------------------------------------------------------------------------------------------------------------
// CDT data
// ---------------------------------------------------------------------------------------------------------------------

struct CdtData {
    t_plane: Plane,
    vert: Vec<[Q; 2]>,
    edge: Vec<(usize, usize)>,
    face: Vec<Vec<usize>>,
    input_face: Vec<usize>,
    is_reversed: Vec<bool>,
    cdt_out: CdtOutput,
    verts_to_edge: HashMap<(usize, usize), usize>,
    proj_axis: usize,
}

fn project_3d_to_2d(p: &V3q, axis: usize) -> [Q; 2] {
    match axis {
        0 => [p[1].clone(), p[2].clone()],
        1 => [p[0].clone(), p[2].clone()],
        _ => [p[0].clone(), p[1].clone()],
    }
}

pub fn dominant_axis_q(a: &V3q) -> usize {
    let (x, y, z) = (a[0].abs(), a[1].abs(), a[2].abs());
    if x > y {
        if x > z {
            0
        } else {
            2
        }
    } else if y > z {
        1
    } else {
        2
    }
}

fn dominant_axis_d(a: &[f64; 3]) -> usize {
    let (x, y, z) = (a[0].abs(), a[1].abs(), a[2].abs());
    if x > y {
        if x > z {
            0
        } else {
            2
        }
    } else if y > z {
        1
    } else {
        2
    }
}

impl CdtData {
    fn new(plane: Plane) -> CdtData {
        let proj_axis = dominant_axis_q(&plane.norm_exact);
        CdtData {
            t_plane: plane,
            vert: vec![],
            edge: vec![],
            face: vec![],
            input_face: vec![],
            is_reversed: vec![],
            cdt_out: CdtOutput::default(),
            verts_to_edge: HashMap::new(),
            proj_axis,
        }
    }
    fn prepare_need_vert(&mut self, p3d: &V3q) -> usize {
        let p2d = project_3d_to_2d(p3d, self.proj_axis);
        self.vert.push(p2d);
        self.vert.len() - 1
    }
    fn prepare_need_edge(&mut self, p1: &V3q, p2: &V3q) {
        let v1 = self.prepare_need_vert(p1);
        let v2 = self.prepare_need_vert(p2);
        self.edge.push((v1, v2));
    }
    fn prepare_need_tri(&mut self, arena: &Arena, tm: &[usize], t: usize) {
        let tri = &arena.faces[tm[t]];
        let v0 = self.prepare_need_vert(&arena.verts[tri.vert[0] as usize].co);
        let v1 = self.prepare_need_vert(&arena.verts[tri.vert[1] as usize].co);
        let v2 = self.prepare_need_vert(&arena.verts[tri.vert[2] as usize].co);
        let rev;
        let nn = &tri.plane.as_ref().unwrap().norm_exact[self.proj_axis];
        if q_sgn(nn) >= 0 {
            rev = self.proj_axis == 1;
        } else {
            rev = self.proj_axis != 1;
        }
        let mut f = vec![v0];
        if rev {
            f.push(v2);
            f.push(v1);
        } else {
            f.push(v1);
            f.push(v2);
        }
        self.face.push(f);
        self.input_face.push(t);
        self.is_reversed.push(rev);
    }
}

fn prepare_cdt_input(arena: &Arena, tm: &[usize], t: usize, itts: &[Itt]) -> CdtData {
    let plane = (**arena.faces[tm[t]].plane.as_ref().unwrap()).clone();
    let mut ans = CdtData::new(plane);
    ans.prepare_need_tri(arena, tm, t);
    for itt in itts {
        match itt.kind {
            IKind::None => {}
            IKind::Point => {
                ans.prepare_need_vert(&itt.p1);
            }
            IKind::Segment => ans.prepare_need_edge(&itt.p1, &itt.p2),
            IKind::Coplanar => ans.prepare_need_tri(arena, tm, itt.t_source as usize),
        }
    }
    ans
}

fn prepare_cdt_input_for_cluster(arena: &Arena, tm: &[usize], cl: &CoplanarCluster, itts: &[Itt]) -> CdtData {
    let t0 = cl.tris[0];
    let plane = (**arena.faces[tm[t0]].plane.as_ref().unwrap()).clone();
    let mut ans = CdtData::new(plane);
    for &t in &cl.tris {
        ans.prepare_need_tri(arena, tm, t);
    }
    for itt in itts {
        match itt.kind {
            IKind::Point => {
                ans.prepare_need_vert(&itt.p1);
            }
            IKind::Segment => ans.prepare_need_edge(&itt.p1, &itt.p2),
            _ => {}
        }
    }
    ans
}

fn sorted_pair(p: (usize, usize)) -> (usize, usize) {
    if p.0 <= p.1 {
        p
    } else {
        (p.1, p.0)
    }
}

fn do_cdt(cd: &mut CdtData) {
    let input = CdtInput { vert: cd.vert.clone(), edge: cd.edge.clone(), face: cd.face.clone() };
    if let Ok(path) = std::env::var("BOOL_DUMP_CDT") {
        use std::io::Write;
        let v: Vec<[f64; 2]> = cd.vert.iter().map(|p| [q_get_d(&p[0]), q_get_d(&p[1])]).collect();
        let exact: Vec<[String; 2]> = cd.vert.iter().map(|p| [p[0].to_string(), p[1].to_string()]).collect();
        let j = serde_json::json!({"verts": v, "exact": exact, "edges": cd.edge.iter().map(|e| [e.0, e.1]).collect::<Vec<_>>(), "faces": cd.face});
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        writeln!(f, "{}", j).unwrap();
    }
    cd.cdt_out = delaunay_2d_calc_inside(&input);
    if cd.cdt_out.edge.len() >= 15 {
        for (e, &p) in cd.cdt_out.edge.iter().enumerate() {
            cd.verts_to_edge.entry(sorted_pair(p)).or_insert(e);
        }
    }
}

fn unproject_cdt_vert(cd: &CdtData, p2d: &[Q; 2]) -> V3q {
    let n = &cd.t_plane.norm_exact;
    let d = &cd.t_plane.d_exact;
    match cd.proj_axis {
        0 => {
            let num = -(&n[1] * &p2d[0] + &n[2] * &p2d[1] + d);
            [num / &n[0], p2d[0].clone(), p2d[1].clone()]
        }
        1 => {
            let num = -(&n[0] * &p2d[0] + &n[2] * &p2d[1] + d);
            [p2d[0].clone(), num / &n[1], p2d[1].clone()]
        }
        _ => {
            let num = -(&n[0] * &p2d[0] + &n[1] * &p2d[1] + d);
            [p2d[0].clone(), p2d[1].clone(), num / &n[2]]
        }
    }
}

fn get_cdt_edge_orig(i0: usize, i1: usize, cd: &CdtData, arena: &Arena, tm: &[usize]) -> (i32, bool) {
    let foff = cd.cdt_out.face_edge_offset;
    let mut e: Option<usize> = None;
    if !cd.verts_to_edge.is_empty() {
        e = cd.verts_to_edge.get(&sorted_pair((i0, i1))).copied();
    } else {
        for (ee, &edge) in cd.cdt_out.edge.iter().enumerate() {
            if (edge.0 == i0 && edge.1 == i1) || (edge.0 == i1 && edge.1 == i0) {
                e = Some(ee);
                break;
            }
        }
    }
    let e = match e {
        Some(e) => e,
        None => return (NO_INDEX, false),
    };
    let mut face_eorig = NO_INDEX;
    let mut have_non_face_eorig = false;
    for &orig_index in &cd.cdt_out.edge_orig[e] {
        if orig_index >= foff {
            if face_eorig == NO_INDEX {
                let in_face_index = (orig_index / foff) - 1;
                let pos = (orig_index % foff) as usize;
                let in_tm_face_index = cd.input_face[in_face_index as usize];
                let facep = &arena.faces[tm[in_tm_face_index]];
                let is_rev = cd.is_reversed[in_face_index as usize];
                let eorig = if is_rev { facep.edge_orig[2 - pos] } else { facep.edge_orig[pos] };
                if eorig != NO_INDEX {
                    face_eorig = eorig;
                }
            }
        } else {
            if !have_non_face_eorig {
                have_non_face_eorig = true;
            }
            if face_eorig != NO_INDEX && have_non_face_eorig {
                break;
            }
        }
    }
    if face_eorig != NO_INDEX {
        return (face_eorig, false);
    }
    if have_non_face_eorig {
        return (NO_INDEX, true);
    }
    (NO_INDEX, false)
}

fn cdt_tri_as_imesh_face(cdt_out_t: usize, cdt_in_t: usize, cd: &CdtData, arena: &mut Arena, tm: &[usize]) -> usize {
    let t_orig = arena.faces[tm[cd.input_face[cdt_in_t]]].orig;
    let f = &cd.cdt_out.face[cdt_out_t];
    let (i0, i1, i2) = (f[0], f[1], f[2]);
    let v0co = unproject_cdt_vert(cd, &cd.cdt_out.vert[i0]);
    let v1co = unproject_cdt_vert(cd, &cd.cdt_out.vert[i1]);
    let v2co = unproject_cdt_vert(cd, &cd.cdt_out.vert[i2]);
    let v0 = arena.add_or_find_vert_q(v0co, NO_INDEX);
    let v1 = arena.add_or_find_vert_q(v1co, NO_INDEX);
    let v2 = arena.add_or_find_vert_q(v2co, NO_INDEX);
    let facep;
    if cd.is_reversed[cdt_in_t] {
        let (oe0, s0) = get_cdt_edge_orig(i0, i2, cd, arena, tm);
        let (oe1, s1) = get_cdt_edge_orig(i2, i1, cd, arena, tm);
        let (oe2, s2) = get_cdt_edge_orig(i1, i0, cd, arena, tm);
        facep = arena.add_face(vec![v0, v2, v1], t_orig, vec![oe0, oe1, oe2], vec![s0, s1, s2]);
    } else {
        let (oe0, s0) = get_cdt_edge_orig(i0, i1, cd, arena, tm);
        let (oe1, s1) = get_cdt_edge_orig(i1, i2, cd, arena, tm);
        let (oe2, s2) = get_cdt_edge_orig(i2, i0, cd, arena, tm);
        facep = arena.add_face(vec![v0, v1, v2], t_orig, vec![oe0, oe1, oe2], vec![s0, s1, s2]);
    }
    arena.populate_plane(facep, false);
    facep
}

// ---------------------------------------------------------------------------------------------------------------------
// Triangulation of polygons
// ---------------------------------------------------------------------------------------------------------------------

fn is_quad_flip_first_third(v1: &[f64; 3], v2: &[f64; 3], v3: &[f64; 3], v4: &[f64; 3]) -> bool {
    let d_12 = sub3d(v2, v1);
    let d_13 = sub3d(v3, v1);
    let d_14 = sub3d(v4, v1);
    let cross_a = cross3d(&d_12, &d_13);
    let cross_b = cross3d(&d_14, &d_13);
    dot3d(&cross_a, &cross_b) > 0.0
}

fn polyfill_triangulate_poly(arena: &mut Arena, f: usize) -> Vec<usize> {
    let flen = arena.faces[f].size();
    if arena.faces[f].plane.is_none() {
        arena.populate_plane(f, false);
    }
    let poly_normal = arena.faces[f].plane.as_ref().unwrap().norm;
    let no0 = [poly_normal[0] as f32, poly_normal[1] as f32, poly_normal[2] as f32];
    let no = polyfill::normalize_v3(no0);
    let orig = arena.faces[f].orig;
    let fv = arena.faces[f].vert.clone();
    let feo = arena.faces[f].edge_orig.clone();
    if flen == 4 {
        let (v0, v1, v2, v3) = (fv[0], fv[1], fv[2], fv[3]);
        let (eo_01, eo_12, eo_23, eo_30) = (feo[0], feo[1], feo[2], feo[3]);
        let d = |v: u32| arena.verts[v as usize].d;
        let (c0, c1, c2, c3) = (d(v0), d(v1), d(v2), d(v3));
        let (f0, f1);
        if is_quad_flip_first_third(&c0, &c1, &c2, &c3) {
            f0 = arena.add_face(vec![v0, v1, v3], orig, vec![eo_01, -1, eo_30], vec![false; 3]);
            f1 = arena.add_face(vec![v1, v2, v3], orig, vec![eo_12, eo_23, -1], vec![false; 3]);
        } else {
            f0 = arena.add_face(vec![v0, v1, v2], orig, vec![eo_01, eo_12, -1], vec![false; 3]);
            f1 = arena.add_face(vec![v0, v2, v3], orig, vec![-1, eo_23, eo_30], vec![false; 3]);
        }
        return vec![f0, f1];
    }
    // project along negative normal
    let r2 = [-no[0], -no[1], -no[2]];
    let (r0, r1) = polyfill::ortho_basis(r2);
    let m = [[r0[0], r1[0], r2[0]], [r0[1], r1[1], r2[1]], [r0[2], r1[2], r2[2]]];
    let mut proj: Vec<[f32; 2]> = vec![];
    for j in 0..flen {
        let dco = arena.verts[fv[j] as usize].d;
        let c = [dco[0] as f32, dco[1] as f32, dco[2] as f32];
        proj.push([m[0][0] * c[0] + m[1][0] * c[1] + m[2][0] * c[2], m[0][1] * c[0] + m[1][1] * c[1] + m[2][1] * c[2]]);
    }
    let tris = polyfill::polyfill_calc(&proj);
    let mut ans = vec![];
    for tri in tris {
        let mut eo = [0i32; 3];
        let mut v = [0u32; 3];
        for k in 0..3 {
            v[k] = fv[tri[k] as usize];
            if (tri[k] as usize + 1) % flen == tri[(k + 1) % 3] as usize {
                eo[k] = feo[tri[k] as usize];
            } else {
                eo[k] = NO_INDEX;
            }
        }
        ans.push(arena.add_face(vec![v[0], v[1], v[2]], orig, vec![eo[0], eo[1], eo[2]], vec![false; 3]));
    }
    ans
}

fn exact_triangulate_poly(arena: &mut Arena, f: usize) -> Vec<usize> {
    let flen = arena.faces[f].size();
    let fv = arena.faces[f].vert.clone();
    let feo = arena.faces[f].edge_orig.clone();
    let orig = arena.faces[f].orig;
    if arena.faces[f].plane.is_none() {
        arena.populate_plane(f, false);
    }
    let poly_normal = arena.faces[f].plane.as_ref().unwrap().norm;
    let axis = dominant_axis_d(&poly_normal);
    let rev1 = axis == 1;
    let rev2 = poly_normal[axis] < 0.0;
    let rev = rev1 ^ rev2;
    let mut in_verts: Vec<[Q; 2]> = vec![[q_zero(), q_zero()]; flen];
    for ii in 0..flen {
        let mut k = 0;
        for j in 0..3 {
            if j != axis {
                in_verts[ii][k] = arena.verts[fv[ii] as usize].co[j].clone();
                k += 1;
            }
        }
    }
    let input = CdtInput { vert: in_verts, edge: vec![], face: vec![(0..flen).collect()] };
    let cdt_out = delaunay_2d_calc_inside(&input);
    let n_tris = cdt_out.face.len();
    let mut ans = vec![];
    for t in 0..n_tris {
        let mut i_v_out = [0usize; 3];
        let mut v = [0u32; 3];
        let mut needs_steiner = false;
        for i in 0..3 {
            i_v_out[i] = cdt_out.face[t][i];
            if cdt_out.vert_orig[i_v_out[i]].is_empty() {
                needs_steiner = true;
                break;
            }
            v[i] = fv[cdt_out.vert_orig[i_v_out[i]][0] as usize];
        }
        if needs_steiner {
            return polyfill_triangulate_poly(arena, f);
        }
        let mut verts_to_edge: HashMap<(usize, usize), usize> = HashMap::new();
        for (e, &p) in cdt_out.edge.iter().enumerate() {
            verts_to_edge.entry(sorted_pair(p)).or_insert(e);
        }
        let foff = cdt_out.face_edge_offset;
        let mut eo = [NO_INDEX; 3];
        for i in 0..3 {
            let vpair = (i_v_out[i], i_v_out[(i + 1) % 3]);
            let e_out = *verts_to_edge.get(&sorted_pair(vpair)).unwrap();
            eo[i] = NO_INDEX;
            for &o in &cdt_out.edge_orig[e_out] {
                if o >= foff {
                    let pos = (o % foff) as usize;
                    eo[i] = feo[pos];
                    break;
                }
            }
        }
        if rev {
            ans.push(arena.add_face(vec![v[0], v[2], v[1]], orig, vec![eo[2], eo[1], eo[0]], vec![false; 3]));
        } else {
            ans.push(arena.add_face(vec![v[0], v[1], v[2]], orig, vec![eo[0], eo[1], eo[2]], vec![false; 3]));
        }
    }
    ans
}

fn face_is_degenerate(arena: &Arena, f: usize) -> bool {
    let face = &arena.faces[f];
    let (i0, i1, i2) = (face.vert[0], face.vert[1], face.vert[2]);
    if i0 == i1 || i0 == i2 || i1 == i2 {
        return true;
    }
    let (v0, v1, v2) = (&arena.verts[i0 as usize], &arena.verts[i1 as usize], &arena.verts[i2 as usize]);
    let da = sub3d(&v2.d, &v0.d);
    let db = sub3d(&v2.d, &v1.d);
    let dab = cross3d(&da, &db);
    let dab_length_squared = dot3d(&dab, &dab);
    let err_bound = supremum_dot_cross(&dab, &dab) * INDEX_DOT_CROSS * DBL_EPSILON;
    if dab_length_squared > err_bound {
        return false;
    }
    let a = q3_sub(&v2.co, &v0.co);
    let b = q3_sub(&v2.co, &v1.co);
    let ab = q3_cross(&a, &b);
    ab[0].is_zero() && ab[1].is_zero() && ab[2].is_zero()
}

fn any_degenerate_tris_fast(arena: &Arena, tri: &[usize]) -> bool {
    for &f in tri {
        let face = &arena.faces[f];
        let (i0, i1, i2) = (face.vert[0], face.vert[1], face.vert[2]);
        if i0 == i1 || i0 == i2 || i1 == i2 {
            return true;
        }
        let (v0, v1, v2) = (&arena.verts[i0 as usize], &arena.verts[i1 as usize], &arena.verts[i2 as usize]);
        let da = sub3d(&v2.d, &v0.d);
        let db = sub3d(&v2.d, &v1.d);
        let da_len = dot3d(&da, &da);
        let db_len = dot3d(&db, &db);
        if da_len == 0.0 || db_len == 0.0 {
            return true;
        }
        let dab = cross3d(&da, &db);
        let dab_len = dot3d(&dab, &dab);
        let sin_squared_t = dab_len / (da_len * db_len);
        if sin_squared_t < 1e-8 {
            return true;
        }
    }
    false
}

fn triangulate_poly(arena: &mut Arena, f: usize) -> Vec<usize> {
    let ans = polyfill_triangulate_poly(arena, f);
    if any_degenerate_tris_fast(arena, &ans) {
        return exact_triangulate_poly(arena, f);
    }
    ans
}

/// `triangulate_polymesh`
pub fn triangulate_polymesh(arena: &mut Arena, imesh: &[usize]) -> Vec<usize> {
    for &f in imesh {
        if arena.faces[f].plane.is_none() && arena.faces[f].size() >= 4 {
            arena.populate_plane(f, false);
        }
    }
    let mut face_tris = vec![];
    for &f in imesh {
        if arena.faces[f].size() == 3 {
            face_tris.push(f);
        } else {
            let tris = triangulate_poly(arena, f);
            face_tris.extend(tris);
        }
    }
    face_tris
}

// ---------------------------------------------------------------------------------------------------------------------
// Overlaps
// ---------------------------------------------------------------------------------------------------------------------

/// Pairs (indexA, indexB) of triangles whose (BVH-inflated) bounding boxes overlap and which pass the shape filter,
/// sorted by (A, B) - this is what `TriOverlaps` ends up with after its `std::sort`.
fn compute_overlaps(bbs: &[BoundingBox], nshapes: usize, use_self: bool, shape: &[i32], orig: &[i32]) -> Vec<(u32, u32)> {
    let n = bbs.len();
    let two_trees_no_self = nshapes == 2 && !use_self;
    let lo: Vec<[f32; 3]> = bbs.iter().map(|b| [b.min[0] - FLT_EPSILON, b.min[1] - FLT_EPSILON, b.min[2] - FLT_EPSILON]).collect();
    let hi: Vec<[f32; 3]> = bbs.iter().map(|b| [b.max[0] + FLT_EPSILON, b.max[1] + FLT_EPSILON, b.max[2] + FLT_EPSILON]).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| lo[a][0].partial_cmp(&lo[b][0]).unwrap().then(a.cmp(&b)));
    let mut out: Vec<(u32, u32)> = vec![];
    for ii in 0..n {
        let i = idx[ii];
        for jj in ii + 1..n {
            let j = idx[jj];
            if lo[j][0] > hi[i][0] {
                break;
            }
            if lo[i][1] > hi[j][1] || lo[j][1] > hi[i][1] || lo[i][2] > hi[j][2] || lo[j][2] > hi[i][2] {
                continue;
            }
            // x: lo[i].x <= lo[j].x <= hi[i].x and lo[j].x <= hi[j].x so overlapping.
            let ok = if two_trees_no_self {
                (shape[i] == 0 && shape[j] == 1) || (shape[i] == 1 && shape[j] == 0)
            } else if nshapes == 1 {
                true
            } else {
                orig[i] != orig[j]
            };
            if ok {
                out.push((i as u32, j as u32));
                out.push((j as u32, i as u32));
            }
        }
    }
    out.sort_unstable();
    out
}

/// Itt map: Blender's `Map<std::pair<int,int>, ITT_value>` (inline capacity 0 because ITT_value is big).
type IttMap = BMap<(i32, i32), Itt>;

fn canon_int_pair(a: i32, b: i32) -> (i32, i32) {
    if a > b {
        (b, a)
    } else {
        (a, b)
    }
}

struct PlaneKey {
    plane: Plane,
}
impl PartialEq for PlaneKey {
    fn eq(&self, o: &Self) -> bool {
        self.plane.norm_exact == o.plane.norm_exact && self.plane.d_exact == o.plane.d_exact
    }
}
impl BHash for PlaneKey {
    fn bhash(&self) -> u64 {
        self.plane.hash()
    }
}

fn find_clusters(arena: &Arena, tm: &[usize], tri_bb: &[BoundingBox], itt_map: &IttMap) -> CoplanarClusterInfo {
    let mut ans = CoplanarClusterInfo { clusters: vec![], tri_cluster: vec![-1; tm.len()] };
    // VectorSet: insertion ordered, unique
    let mut maybe: Vec<i32> = vec![];
    let mut maybe_set: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut items_vec: Vec<(&(i32, i32), &Itt)> = itt_map.items().collect();
    match std::env::var("ITT_SORTED").as_deref() {
        Ok("asc") => items_vec.sort_by_key(|x| *x.0),
        Ok("desc") => items_vec.sort_by_key(|x| std::cmp::Reverse(*x.0)),
        Ok("ba") => items_vec.sort_by_key(|x| (x.0 .1, x.0 .0)),
        Ok("ba_desc") => items_vec.sort_by_key(|x| std::cmp::Reverse((x.0 .1, x.0 .0))),
        Ok("rev") => items_vec.reverse(),
        _ => {}
    }
    for (k, v) in items_vec {
        if v.kind == IKind::Coplanar {
            for t in [k.0, k.1] {
                if maybe_set.insert(t) {
                    maybe.push(t);
                }
            }
        }
    }
    if maybe.is_empty() {
        return ans;
    }
    if std::env::var("BOOL_DEBUG_CL").is_ok() {
        eprintln!("itt_map items:");
        for (k, v) in itt_map.items() {
            eprintln!("  {:?} {:?}", k, v.kind);
        }
        eprintln!("maybe {:?}", maybe);
    }
    let mut plane_cls: BMap<PlaneKey, Vec<CoplanarCluster>> = BMap::new(std::env::var("PLANE_INLINE").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    plane_cls.reserve(maybe.len() as i64);
    for &t in &maybe {
        let t = t as usize;
        let mut tplane = (**arena.faces[tm[t]].plane.as_ref().unwrap()).clone();
        tplane.make_canonical();
        if std::env::var("BOOL_DEBUG_CL").is_ok() {
            eprintln!("maybe tri {} plane norm {:?} d {} hash {:#x}", t, tplane.norm, tplane.d, tplane.hash());
        }
        let key = PlaneKey { plane: tplane };
        if plane_cls.contains(&key) {
            let curcls: Vec<CoplanarCluster> = plane_cls.lookup(&key).unwrap().clone();
            let mut int_idx = vec![];
            let mut no_int_idx = vec![];
            for (ci, cl) in curcls.iter().enumerate() {
                if bbs_might_intersect(&tri_bb[t], &cl.bb) {
                    int_idx.push(ci);
                } else {
                    no_int_idx.push(ci);
                }
            }
            if int_idx.is_empty() {
                plane_cls.lookup_mut(&key).unwrap().push(CoplanarCluster::new_one(t, &tri_bb[t]));
            } else if int_idx.len() == 1 {
                plane_cls.lookup_mut(&key).unwrap()[int_idx[0]].add_tri(t, &tri_bb[t]);
            } else {
                let mut mergecl = CoplanarCluster { tris: vec![], bb: BoundingBox::new() };
                mergecl.add_tri(t, &tri_bb[t]);
                for &ci in &int_idx {
                    for &tt in &curcls[ci].tris {
                        mergecl.add_tri(tt, &tri_bb[tt]);
                    }
                }
                let mut newvec = vec![mergecl];
                for &ci in &no_int_idx {
                    newvec.push(curcls[ci].clone());
                }
                *plane_cls.lookup_mut(&key).unwrap() = newvec;
            }
        } else {
            plane_cls.add_new(key, vec![CoplanarCluster::new_one(t, &tri_bb[t])]);
        }
    }
    for (_k, v) in plane_cls.items() {
        for cl in v {
            if cl.tris.len() > 1 {
                let c_index = ans.clusters.len() as i32;
                for &t in &cl.tris {
                    ans.tri_cluster[t] = c_index;
                }
                ans.clusters.push(cl.clone());
            }
        }
    }
    ans
}

// ---------------------------------------------------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------------------------------------------------

/// `trimesh_nary_intersect`.  `shape_fn` maps a face `orig` index to its shape.
pub fn trimesh_nary_intersect(arena: &mut Arena, tm_in: &[usize], nshapes: usize, shape_fn: &dyn Fn(i32) -> i32, use_self: bool) -> Vec<usize> {
    // Remove degenerate triangles.
    let mut has_degenerate = false;
    for &f in tm_in {
        if face_is_degenerate(arena, f) {
            has_degenerate = true;
        }
    }
    let tm: Vec<usize> = if has_degenerate { tm_in.iter().copied().filter(|&f| !face_is_degenerate(arena, f)).collect() } else { tm_in.to_vec() };
    let ntri = tm.len();
    let tri_bb = calc_face_bounding_boxes(arena, &tm);
    let shapes: Vec<i32> = tm.iter().map(|&f| shape_fn(arena.faces[f].orig)).collect();
    let origs: Vec<i32> = tm.iter().map(|&f| arena.faces[f].orig).collect();
    let overlap = compute_overlaps(&tri_bb, nshapes, use_self, &shapes, &origs);
    let mut first_overlap = vec![-1i32; ntri];
    for (i, o) in overlap.iter().enumerate() {
        if first_overlap[o.0 as usize] == -1 {
            first_overlap[o.0 as usize] = i as i32;
        }
    }
    for t in 0..ntri {
        if first_overlap[t] != -1 {
            arena.populate_plane(tm[t], true);
        }
    }

    // itt_map
    let mut itt_map: IttMap = BMap::new(std::env::var("ITT_INLINE").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    if let Ok(v) = std::env::var("ITT_TOTAL") {
        itt_map.forced_total = v.parse().unwrap();
    }
    itt_map.reserve(overlap.len() as i64);
    let mut intersect_pairs: Vec<(i32, i32)> = vec![];
    for o in &overlap {
        let key = canon_int_pair(o.0 as i32, o.1 as i32);
        if !itt_map.contains(&key) {
            itt_map.add_new(key, Itt::none());
            intersect_pairs.push(key);
        }
    }
    for &(a, b) in &intersect_pairs {
        let itt = intersect_tri_tri(arena, tm[a as usize], tm[b as usize], b as usize);
        itt_map.add_overwrite((a, b), itt);
    }

    if let Ok(w) = std::env::var("BOOL_DEBUG_TRI") {
        let w: i32 = w.parse().unwrap();
        for o in &overlap {
            if arena.faces[tm[o.0 as usize]].orig == w {
                let key = canon_int_pair(o.0 as i32, o.1 as i32);
                let itt = itt_map.lookup(&key).unwrap();
                eprintln!("tri {} (orig {}) vs {} (orig {}): {:?}", o.0, w, o.1, arena.faces[tm[o.1 as usize]].orig, itt.kind);
            }
        }
    }
    let clinfo = find_clusters(arena, &tm, &tri_bb, &itt_map);

    // calc_subdivided_non_cluster_tris
    let mut tri_subdivided: Vec<Vec<usize>> = vec![vec![]; ntri];
    {
        struct Otr {
            tri_index: usize,
            start: usize,
            len: usize,
        }
        let mut ranges: Vec<Otr> = vec![];
        let mut oi = 0;
        while oi < overlap.len() {
            let t = overlap[oi].0 as usize;
            let mut i = oi;
            while i + 1 < overlap.len() && overlap[i + 1].0 as usize == t {
                i += 1;
            }
            if clinfo.tri_cluster[t] == -1 {
                let len = i - oi + 1;
                if !(len == 1 && overlap[oi].1 as usize == t) {
                    ranges.push(Otr { tri_index: t, start: oi, len });
                }
            }
            oi = i + 1;
        }
        let mut cd_data: Vec<Option<CdtData>> = (0..ranges.len()).map(|_| None).collect();
        for (k, otr) in ranges.iter().enumerate() {
            let t = otr.tri_index;
            let mut itts: Vec<Itt> = vec![];
            for j in otr.start..otr.start + otr.len {
                let t_other = overlap[j].1 as i32;
                let key = canon_int_pair(t as i32, t_other);
                if let Some(itt) = itt_map.lookup(&key) {
                    if itt.kind != IKind::None {
                        itts.push(itt.clone());
                    }
                }
            }
            // `Vector<ITT_value, 100> itts(otr.len)` is *sized* (otr.len default-constructed INONE entries), so
            // `itts.size() > 0` always holds and the CDT runs for every triangle that has overlaps.
            {
                let mut cd = prepare_cdt_input(arena, &tm, t, &itts);
                do_cdt(&mut cd);
                cd_data[k] = Some(cd);
            }
        }
        for (k, otr) in ranges.iter().enumerate() {
            if let Some(cdd) = &cd_data[k] {
                let t = otr.tri_index;
                // extract_subdivided_tri
                let mut t_in_cdt: i64 = -1;
                for i in 0..cdd.input_face.len() {
                    if cdd.input_face[i] == t {
                        t_in_cdt = i as i64;
                    }
                }
                if t_in_cdt == -1 {
                    continue;
                }
                let mut faces = vec![];
                for f in 0..cdd.cdt_out.face.len() {
                    if cdd.cdt_out.face_orig[f].contains(&(t_in_cdt as i32)) {
                        faces.push(cdt_tri_as_imesh_face(f, t_in_cdt as usize, cdd, arena, &tm));
                    }
                }
                tri_subdivided[t] = faces;
            }
        }
        for t in 0..ntri {
            if tri_subdivided[t].is_empty() && clinfo.tri_cluster[t] == -1 {
                tri_subdivided[t] = vec![tm[t]];
            }
        }
    }

    // clusters
    let mut cluster_subdivided: Vec<CdtData> = vec![];
    for c in 0..clinfo.clusters.len() {
        let cl = &clinfo.clusters[c];
        let mut itts: Vec<Itt> = vec![];
        for &t in &cl.tris {
            let first_i = first_overlap[t];
            if first_i == -1 {
                continue;
            }
            let mut i = first_i as usize;
            while i < overlap.len() && overlap[i].0 as usize == t {
                let t_other = overlap[i].1 as usize;
                if clinfo.tri_cluster[t_other] != c as i32 {
                    let key = canon_int_pair(t as i32, t_other as i32);
                    if let Some(itt) = itt_map.lookup(&key) {
                        if itt.kind != IKind::None && itt.kind != IKind::Coplanar {
                            itts.push(itt.clone());
                        }
                    }
                }
                i += 1;
            }
        }
        let mut cd = prepare_cdt_input_for_cluster(arena, &tm, cl, &itts);
        do_cdt(&mut cd);
        cluster_subdivided.push(cd);
    }
    for c in 0..clinfo.clusters.len() {
        let cd = &cluster_subdivided[c];
        let n_cluster_tris = clinfo.clusters[c].tris.len();
        let mut face_vec: Vec<Vec<usize>> = vec![vec![]; n_cluster_tris];
        for cdt_out_t in 0..cd.cdt_out.face.len() {
            let origs: Vec<i32> = cd.cdt_out.face_orig[cdt_out_t].clone();
            for cdt_in_t in origs {
                let f = cdt_tri_as_imesh_face(cdt_out_t, cdt_in_t as usize, cd, arena, &tm);
                face_vec[cdt_in_t as usize].push(f);
            }
        }
        for cdt_in_t in 0..cd.input_face.len() {
            let tm_t = cd.input_face[cdt_in_t];
            tri_subdivided[tm_t] = std::mem::take(&mut face_vec[cdt_in_t]);
        }
    }

    let mut combined = vec![];
    for t in 0..ntri {
        combined.extend(tri_subdivided[t].iter().copied());
    }
    combined
}

#[allow(dead_code)]
fn _unused() {
    let _ = Q::one();
}
