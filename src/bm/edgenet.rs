//! Port of `bmesh_polygon_edgenet.cc` (Blender 4.2): `BM_face_split_edgenet` and `BM_face_split_edgenet_connect_islands`
//! (without the partial-connect option, which the boolean does not use). Custom data interpolation is skipped.
use super::bvh::BvhTree;
use super::kern::{FLAG_WALK, FLAG_WALK_ALT, INTERNAL_TAG};
use super::query::*;
use super::*;

const FACE_NET: u8 = FLAG_WALK;
const EDGE_NET: u8 = FLAG_WALK;
const VERT_VISIT: u8 = FLAG_WALK;
const VERT_IN_QUEUE: u8 = FLAG_WALK_ALT;

type M3 = [[f32; 3]; 3];

/// axis_dominant_v3_to_m3 (`negate` selects the `_negate` variant); result in Blender's `[col][row]` layout after the transpose
pub fn axis_dominant_v3_to_m3(normal: V3, negate: bool) -> M3 {
    let n = if negate { [-normal[0], -normal[1], -normal[2]] } else { normal };
    let (r0, r1) = crate::exbool::polyfill::ortho_basis(n);
    // m rows: r0, r1, n  -> transpose_m3
    [[r0[0], r1[0], n[0]], [r0[1], r1[1], n[1]], [r0[2], r1[2], n[2]]]
}
/// mul_v2_m3v3
pub fn mul_v2_m3v3(m: &M3, a: V3) -> [f32; 2] {
    [m[0][0] * a[0] + m[1][0] * a[1] + m[2][0] * a[2], m[0][1] * a[0] + m[1][1] * a[1] + m[2][1] * a[2]]
}
fn dot_m3_v3_row_x(m: &M3, a: V3) -> f32 {
    m[0][0] * a[0] + m[1][0] * a[1] + m[2][0] * a[2]
}
fn dot_m3_v3_row_y(m: &M3, a: V3) -> f32 {
    m[0][1] * a[0] + m[1][1] * a[1] + m[2][1] * a[2]
}

fn normalize_v2(a: [f32; 2]) -> [f32; 2] {
    let d = a[0] * a[0] + a[1] * a[1];
    if d > 1.0e-35f32 {
        let d = d.sqrt();
        let f = 1.0f32 / d;
        [a[0] * f, a[1] * f]
    } else {
        [0.0, 0.0]
    }
}
fn dot_v2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross_v2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub_v2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// angle_signed_on_axis_v3v3v3_v3
fn angle_signed_on_axis_v3v3v3_v3(v1: V3, v2: V3, v3: V3, axis: V3) -> f32 {
    let vec1 = sub(v1, v2);
    let vec2 = sub(v3, v2);
    // project_plane_normalized_v3_v3v3
    let proj = |v: V3| {
        let mul = dot(v, axis);
        madd(v, axis, -mul)
    };
    let v1p = proj(vec1);
    let v2p = proj(vec2);
    let mut angle = angle_v3v3(v1p, v2p);
    let t = cross(v2p, v1p);
    if dot(t, axis) < 0.0 {
        angle = (std::f64::consts::PI * 2.0) as f32 - angle;
    }
    angle
}

fn line_point_factor_v2(p: [f32; 2], l1: [f32; 2], l2: [f32; 2]) -> f32 {
    let u = sub_v2(l2, l1);
    let h = sub_v2(p, l1);
    let d = dot_v2(u, u);
    if d > 0.0 {
        dot_v2(u, h) / d
    } else {
        0.0
    }
}

/// isect_seg_seg_v2_point_ex; returns (kind, point)
fn isect_seg_seg_v2_point_ex(v0: [f32; 2], v1: [f32; 2], v2: [f32; 2], v3: [f32; 2], endpoint_bias: f32) -> (i32, [f32; 2]) {
    let eps = 1e-6f32;
    let endpoint_min = -endpoint_bias;
    let endpoint_max = endpoint_bias + 1.0;
    let s10 = sub_v2(v1, v0);
    let s32 = sub_v2(v3, v2);
    let s30 = sub_v2(v3, v0);
    let d = cross_v2(s10, s32);
    if d != 0.0 {
        let u = cross_v2(s30, s32) / d;
        let v = cross_v2(s10, s30) / d;
        if u >= endpoint_min && u <= endpoint_max && v >= endpoint_min && v <= endpoint_max {
            let vi_test = [v0[0] + s10[0] * u, v0[1] + s10[1] * u];
            let s_vi_v2 = sub_v2(vi_test, v2);
            let v = dot_v2(s32, s_vi_v2) / dot_v2(s32, s32);
            if v >= endpoint_min && v <= endpoint_max {
                return (1, vi_test);
            }
        }
        return (-1, [0.0; 2]);
    }
    if cross_v2(s10, s30) == 0.0 && cross_v2(s32, s30) == 0.0 {
        // equal lines
        let (mut v0, mut v1, mut v2, mut v3) = (v0, v1, v2, v3);
        let mut s10 = s10;
        let mut s30 = s30;
        if v0 == v1 {
            let s32b = sub_v2(v3, v2);
            if dot_v2(s32b, s32b) > eps * eps {
                std::mem::swap(&mut v0, &mut v2);
                std::mem::swap(&mut v1, &mut v3);
                s10 = sub_v2(v1, v0);
                s30 = sub_v2(v3, v0);
            } else {
                if v0 == v2 {
                    return (1, v0);
                }
                return (-1, [0.0; 2]);
            }
        }
        let s20 = sub_v2(v2, v0);
        let mut u_a = dot_v2(s20, s10) / dot_v2(s10, s10);
        let mut u_b = dot_v2(s30, s10) / dot_v2(s10, s10);
        if u_a > u_b {
            std::mem::swap(&mut u_a, &mut u_b);
        }
        if u_a > endpoint_max || u_b < endpoint_min {
            return (-1, [0.0; 2]);
        }
        let mx = if 0.0 > u_a { 0.0 } else { u_a };
        let mn = if 1.0 < u_b { 1.0 } else { u_b };
        if mx == mn {
            return (1, [v0[0] + s10[0] * mx, v0[1] + s10[1] * mx]);
        }
    }
    (-1, [0.0; 2])
}

/// isect_ray_seg_v2 -> Some(lambda)
fn isect_ray_seg_v2(ray_origin: [f32; 2], ray_direction: [f32; 2], v0: [f32; 2], v1: [f32; 2]) -> Option<f32> {
    let v0l = sub_v2(v0, ray_origin);
    let v1l = sub_v2(v1, ray_origin);
    let s10 = sub_v2(v1l, v0l);
    let det = cross_v2(ray_direction, s10);
    if det != 0.0 {
        let v = cross_v2(v0l, v1l);
        let p = [(ray_direction[0] * v) / det, (ray_direction[1] * v) / det];
        let t = dot_v2(p, ray_direction) / dot_v2(ray_direction, ray_direction);
        if (t >= 0.0) == false {
            return None;
        }
        let h = sub_v2(v1l, p);
        let u = dot_v2(s10, h) / dot_v2(s10, s10);
        if (u >= 0.0 && u <= 1.0) == false {
            return None;
        }
        return Some(t);
    }
    None
}

fn closest_to_line_segment_v2(p: [f32; 2], l1: [f32; 2], l2: [f32; 2]) -> [f32; 2] {
    // closest_to_line_v2
    let u = sub_v2(l2, l1);
    let h = sub_v2(p, l1);
    let denom = dot_v2(u, u);
    let (lambda, cp) = if denom == 0.0 {
        (0.0f32, l1)
    } else {
        let lambda = dot_v2(u, h) / denom;
        (lambda, [l1[0] + u[0] * lambda, l1[1] + u[1] * lambda])
    };
    if lambda <= 0.0 {
        l1
    } else if lambda >= 1.0 {
        l2
    } else {
        cp
    }
}

fn mean_value_half_tan_v2_db(d_curr: &([f64; 2], f64), d_next: &([f64; 2], f64)) -> f64 {
    let area = d_curr.0[0] * d_next.0[1] - d_curr.0[1] * d_next.0[0];
    if area != 0.0 {
        let dot = d_curr.0[0] * d_next.0[0] + d_curr.0[1] * d_next.0[1];
        let len = d_curr.1 * d_next.1;
        let result = (len - dot) / area;
        if result.is_finite() {
            return result;
        }
    }
    0.0
}

/// interp_weights_poly_v2 (mean value coordinates, double precision internals)
pub fn interp_weights_poly_v2(v: &[[f32; 2]], co: [f32; 2]) -> Vec<f32> {
    let n = v.len();
    let mut w = vec![0.0f32; n];
    let mut max_value = 0.0f32;
    for p in v {
        max_value = max_value.max((p[0] - co[0]).abs());
        max_value = max_value.max((p[1] - co[1]).abs());
    }
    let eps = 16.0 * f32::EPSILON * max_value;
    let eps_sq = eps * eps;
    let dir = |a: [f32; 2], b: [f32; 2]| -> ([f64; 2], f64) {
        let d = [a[0] as f64 - b[0] as f64, a[1] as f64 - b[1] as f64];
        (d, (d[0] * d[0] + d[1] * d[1]).sqrt())
    };
    let mut totweight = 0.0f32;
    let mut ix_flag = 0u8;
    let mut i_curr = n - 1;
    let mut i_next = 0usize;
    let mut v_curr = v[i_curr];
    let mut v_next = v[i_next];
    let mut d_curr = dir(v[n - 2], co);
    let mut d_next = dir(v_curr, co);
    let mut ht_prev = mean_value_half_tan_v2_db(&d_curr, &d_next);
    while i_next < n {
        if d_next.1 < eps as f64 {
            ix_flag = 1;
            break;
        }
        let cl = closest_to_line_segment_v2(co, v_curr, v_next);
        let dd = sub_v2(cl, co);
        if dd[0] * dd[0] + dd[1] * dd[1] < eps_sq {
            ix_flag = 2;
            break;
        }
        d_curr = d_next;
        d_next = dir(v_next, co);
        let ht = mean_value_half_tan_v2_db(&d_curr, &d_next);
        w[i_curr] = if d_curr.1 == 0.0 { 0.0 } else { ((ht_prev + ht) / d_curr.1) as f32 };
        totweight += w[i_curr];
        i_curr = i_next;
        i_next += 1;
        v_curr = v_next;
        if i_next < n {
            v_next = v[i_next];
        }
        ht_prev = ht;
    }
    if ix_flag != 0 {
        for x in w.iter_mut() {
            *x = 0.0;
        }
        if ix_flag & 1 != 0 {
            w[i_curr] = 1.0;
        } else {
            let mut fac = line_point_factor_v2(co, v_curr, v_next);
            fac = fac.clamp(0.0, 1.0);
            w[i_curr] = 1.0 - fac;
            w[i_next] = fac;
        }
    } else if totweight != 0.0 {
        for x in w.iter_mut() {
            *x /= totweight;
        }
    }
    w
}

impl BMesh {
    fn edge_flagged_radial_count(&self, e: usize) -> usize {
        let first = self.edges[e].l;
        let mut count = 0;
        if first != NONE {
            let mut l = first;
            loop {
                if self.faces[self.loops[l].f].api & FACE_NET != 0 {
                    count += 1;
                }
                l = self.loops[l].rn;
                if l == first {
                    break;
                }
            }
        }
        count
    }
    fn edge_flagged_radial_first(&self, e: usize) -> usize {
        let first = self.edges[e].l;
        if first != NONE {
            let mut l = first;
            loop {
                if self.faces[self.loops[l].f].api & FACE_NET != 0 {
                    return l;
                }
                l = self.loops[l].rn;
                if l == first {
                    break;
                }
            }
        }
        NONE
    }

    fn edgenet_find_loop_pair(&self, v_init: usize, f: usize, fnm: &M3, e_pair: &mut [usize; 2]) -> bool {
        let mut edges_boundary: Vec<usize> = vec![];
        let mut edges_wire: Vec<usize> = vec![];
        {
            let e_first = self.verts[v_init].e;
            let mut e = e_first;
            loop {
                if self.edges[e].api & EDGE_NET != 0 {
                    let count = self.edge_flagged_radial_count(e);
                    if count == 1 {
                        edges_boundary.push(e);
                    } else if count == 0 {
                        edges_wire.push(e);
                    }
                }
                e = self.disk_edge_next(e, v_init);
                if e == e_first {
                    break;
                }
            }
        }
        let (edges_boundary_len, edges_wire_len) = (edges_boundary.len(), edges_wire.len());
        if edges_boundary_len == 0 {
            return false;
        }
        e_pair[0] = edges_boundary.pop().unwrap();
        let mut edges_search: Vec<usize> = vec![];
        if edges_wire_len == 0 {
            if edges_boundary_len > 1 {
                e_pair[1] = edges_boundary.pop().unwrap();
                if edges_boundary_len > 2 {
                    std::mem::swap(&mut edges_search, &mut edges_boundary);
                }
            } else {
                return false;
            }
        } else {
            e_pair[1] = edges_wire.pop().unwrap();
            if edges_wire_len > 1 {
                std::mem::swap(&mut edges_search, &mut edges_wire);
            }
        }
        if !edges_search.is_empty() {
            let v_prev = self.edge_other_vert(e_pair[0], v_init);
            let v_next = self.edge_other_vert(e_pair[1], v_init);
            let nrm2 = |a: usize, b: usize| {
                let d = sub(self.verts[a].co, self.verts[b].co);
                normalize_v2(mul_v2_m3v3(fnm, d))
            };
            let dir_prev = nrm2(v_prev, v_init);
            let dir_next = nrm2(v_next, v_init);
            let mut angle_best_cos = dot_v2(dir_next, dir_prev);
            while let Some(e) = edges_search.pop() {
                let v_next = self.edge_other_vert(e, v_init);
                let dir_test = nrm2(v_next, v_init);
                let angle_test_cos = dot_v2(dir_prev, dir_test);
                if angle_test_cos > angle_best_cos {
                    angle_best_cos = angle_test_cos;
                    e_pair[1] = e;
                }
            }
        }
        // flip based on winding
        let l_walk = self.edge_flagged_radial_first(e_pair[0]);
        let mut swap = false;
        if self.loops[l_walk].f == f {
            swap = !swap;
        }
        if self.loops[l_walk].v != v_init {
            swap = !swap;
        }
        if swap {
            e_pair.swap(0, 1);
        }
        true
    }

    fn edgenet_find_loop_pair_exists(&self, v_init: usize) -> bool {
        let mut edges_boundary_len = 0;
        let mut edges_wire_len = 0;
        let e_first = self.verts[v_init].e;
        let mut e = e_first;
        loop {
            if self.edges[e].api & EDGE_NET != 0 {
                let count = self.edge_flagged_radial_count(e);
                if count == 1 {
                    edges_boundary_len += 1;
                } else if count == 0 {
                    edges_wire_len += 1;
                }
            }
            e = self.disk_edge_next(e, v_init);
            if e == e_first {
                break;
            }
        }
        if edges_boundary_len == 0 {
            return false;
        }
        if edges_wire_len == 0 && edges_boundary_len < 2 {
            return false;
        }
        true
    }

    fn edgenet_find_loop_walk(&mut self, v_init: usize, face_normal: V3, e_pair: [usize; 2]) -> bool {
        struct VertOrder {
            angle: f32,
            v: usize,
        }
        let mut found = false;
        let mut edge_order: Vec<VertOrder> = vec![];
        let mut vert_visit: Vec<usize> = vec![];
        let mut vert_stack: Vec<usize> = vec![];
        let mut vert_stack_next: Vec<usize> = vec![];

        let v0 = self.edge_other_vert(e_pair[0], v_init);
        self.verts[v0].e = e_pair[0];
        vert_stack.push(v0);
        let v_dst = self.edge_other_vert(e_pair[1], v_init);

        'outer: while let Some(mut v) = vert_stack.pop() {
            loop {
                // walk_nofork
                if v == v_dst {
                    found = true;
                    break 'outer;
                }
                let e_first = self.verts[v].e;
                let mut e_next = self.disk_edge_next(e_first, v);
                if e_next != e_first {
                    loop {
                        if self.edges[e_next].api & EDGE_NET != 0 && self.edge_flagged_radial_count(e_next) < 2 {
                            let v_next = self.edge_other_vert(e_next, v);
                            if self.verts[v_next].api & VERT_VISIT == 0 {
                                edge_order.push(VertOrder { angle: 0.0, v: v_next });
                                self.verts[v_next].e = e_next;
                            }
                        }
                        e_next = self.disk_edge_next(e_next, v);
                        if e_next == e_first {
                            break;
                        }
                    }
                }
                if edge_order.len() == 1 {
                    let eo = edge_order.pop().unwrap();
                    v = eo.v;
                    continue;
                }
                if edge_order.len() > 1 {
                    let v_prev = self.edge_other_vert(self.verts[v].e, v);
                    for eo in edge_order.iter_mut() {
                        eo.angle = angle_signed_on_axis_v3v3v3_v3(self.verts[v_prev].co, self.verts[v].co, self.verts[eo.v].co, face_normal);
                    }
                    qsort::qsort(&mut edge_order, &|a: &VertOrder, b: &VertOrder| {
                        // BLI_sortutil_cmp_float_reverse
                        if a.angle < b.angle {
                            1
                        } else if a.angle > b.angle {
                            -1
                        } else {
                            0
                        }
                    });
                    vert_visit.push(v);
                    self.verts[v].api |= VERT_VISIT;
                }
                while let Some(eo) = edge_order.pop() {
                    vert_stack_next.push(eo.v);
                }
                if !vert_stack_next.is_empty() {
                    std::mem::swap(&mut vert_stack, &mut vert_stack_next);
                }
                break;
            }
        }
        for v in vert_visit {
            self.verts[v].api &= !VERT_VISIT;
        }
        found
    }

    fn edgenet_find_loop(&mut self, v_init: usize, f: usize, face_normal: V3, fnm: &M3) -> Option<Vec<usize>> {
        let mut e_pair = [NONE; 2];
        if !self.edgenet_find_loop_pair(v_init, f, fnm, &mut e_pair) {
            return None;
        }
        if self.edgenet_find_loop_walk(v_init, face_normal, e_pair) {
            let mut out = vec![v_init];
            let mut v = self.edge_other_vert(e_pair[1], v_init);
            loop {
                out.push(v);
                v = self.edge_other_vert(self.verts[v].e, v);
                if v == v_init {
                    break;
                }
            }
            return if out.len() > 2 { Some(out) } else { None };
        }
        None
    }

    /// BM_face_split_edgenet (no custom data): returns true when executed; `r_faces` receives the resulting faces
    pub fn face_split_edgenet(&mut self, f: usize, edge_net: &[usize], r_faces: Option<&mut Vec<usize>>) -> bool {
        if edge_net.is_empty() {
            if let Some(r) = r_faces {
                r.clear();
            }
            return false;
        }
        let mut face_arr: Vec<usize> = vec![];
        let mut vert_queue: Vec<usize> = vec![];
        self.faces[f].api |= FACE_NET;
        for &e in edge_net {
            self.edges[e].api |= EDGE_NET;
            let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
            self.verts[v1].api &= !VERT_IN_QUEUE;
            self.verts[v2].api &= !VERT_IN_QUEUE;
        }
        let l_first = self.faces[f].l_first;
        {
            let mut l = l_first;
            loop {
                let e = self.loops[l].e;
                self.edges[e].api |= EDGE_NET;
                let v = self.loops[l].v;
                self.verts[v].api &= !VERT_IN_QUEUE;
                l = self.loops[l].n;
                if l == l_first {
                    break;
                }
            }
        }
        let f_no = self.faces[f].no;
        let fnm = axis_dominant_v3_to_m3(f_no, false);
        let v_first = self.loops[l_first].v;
        vert_queue.push(v_first);
        self.verts[v_first].api |= VERT_IN_QUEUE;
        while let Some(v) = vert_queue.pop() {
            self.verts[v].api &= !VERT_IN_QUEUE;
            if let Some(face_verts) = self.edgenet_find_loop(v, f, f_no, &fnm) {
                let f_new = self.face_create_verts(&face_verts, Some(f), false);
                if f_new != NONE {
                    face_arr.push(f_new);
                    self.faces[f_new].no = f_no;
                    self.faces[f_new].flag = self.faces[f].flag;
                    self.faces[f_new].api |= FACE_NET;
                    let ls = self.loops_of_face(f_new);
                    for l in ls {
                        let lv = self.loops[l].v;
                        if self.verts[lv].api & VERT_IN_QUEUE == 0 && self.edgenet_find_loop_pair_exists(lv) {
                            vert_queue.push(lv);
                            self.verts[lv].api |= VERT_IN_QUEUE;
                        }
                    }
                }
            }
        }
        // loop custom data of the new faces (only when some layer interpolates)
        if self.ld.has_math() {
            let axis_mat = fnm;
            let mut blocks: Vec<usize> = vec![];
            let mut cos_2d: Vec<[f32; 2]> = vec![];
            let first = self.faces[f].l_first;
            let mut l_iter = first;
            loop {
                let lv = self.loops[l_iter].v;
                for l_other in self.loops_of_vert(lv) {
                    let fo = self.loops[l_other].f;
                    if fo != f && self.faces[fo].api & FACE_NET != 0 {
                        self.ld.copy(l_iter, l_other);
                    }
                }
                self.verts[lv].api |= VERT_VISIT;
                cos_2d.push(mul_v2_m3v3(&axis_mat, self.verts[lv].co));
                blocks.push(l_iter);
                l_iter = self.loops[l_iter].n;
                if l_iter == first {
                    break;
                }
            }
            for &e in edge_net {
                for v in [self.edges[e].v1, self.edges[e].v2] {
                    if self.verts[v].api & VERT_VISIT == 0 {
                        self.verts[v].api |= VERT_VISIT;
                        let mut l_first_net = NONE;
                        for l_it in self.loops_of_vert(v) {
                            if self.faces[self.loops[l_it].f].api & FACE_NET != 0 {
                                if l_first_net == NONE {
                                    let co = mul_v2_m3v3(&axis_mat, self.verts[v].co);
                                    let w = interp_weights_poly_v2(&cos_2d, co);
                                    self.ld.interp(&blocks, &w, l_it);
                                    l_first_net = l_it;
                                } else {
                                    self.ld.copy(l_first_net, l_it);
                                }
                            }
                        }
                    }
                }
            }
        }
        // cleanup
        for &e in edge_net {
            self.edges[e].api &= !EDGE_NET;
            let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
            self.verts[v1].api &= !VERT_VISIT;
            self.verts[v2].api &= !VERT_VISIT;
        }
        {
            let mut l = self.faces[f].l_first;
            let first = l;
            loop {
                let e = self.loops[l].e;
                self.edges[e].api &= !EDGE_NET;
                let v = self.loops[l].v;
                self.verts[v].api &= !VERT_VISIT;
                l = self.loops[l].n;
                if l == first {
                    break;
                }
            }
        }
        if !face_arr.is_empty() {
            let f0 = face_arr[0];
            self.face_swap_data(f, f0);
            self.face_kill(f0);
            face_arr[0] = f;
        } else {
            self.faces[f].api &= !FACE_NET;
        }
        for &fa in &face_arr {
            self.faces[fa].api &= !FACE_NET;
        }
        if let Some(r) = r_faces {
            *r = face_arr;
        }
        true
    }

    /// BM_face_split_edgenet_connect_islands (use_partial_connect = false). Returns the new edge net when holes were found.
    pub fn face_split_edgenet_connect_islands(&mut self, f: usize, edge_net_init: &[usize]) -> Option<Vec<usize>> {
        const EDGE_NOT_IN_STACK: u16 = INTERNAL_TAG;
        const VERT_NOT_IN_STACK: u16 = INTERNAL_TAG;
        const VERT_IN_ARRAY: u16 = INTERNAL_TAG;
        let init_len = edge_net_init.len();
        let mut edge_arr: Vec<usize> = edge_net_init.to_vec();
        {
            let first = self.faces[f].l_first;
            let mut l = first;
            loop {
                edge_arr.push(self.loops[l].e);
                l = self.loops[l].n;
                if l == first {
                    break;
                }
            }
        }
        let edge_arr_len = edge_arr.len();
        for &e in &edge_arr {
            self.edges[e].flag |= EDGE_NOT_IN_STACK;
            let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
            self.verts[v1].flag |= VERT_NOT_IN_STACK;
            self.verts[v2].flag |= VERT_NOT_IN_STACK;
        }

        // groups
        struct Group {
            edge_links: Vec<usize>, // list order (most recent first)
            vert_len: usize,
            has_prev_edge: bool,
            vmin: usize,
            vmax: usize,
            min_axis: [f32; 2],
            max_axis: [f32; 2],
        }
        let mut groups_list: Vec<Group> = vec![]; // creation order
        {
            let mut edge_index = edge_arr_len - 1;
            let mut edge_in_group_tot = 0;
            let mut vstack: Vec<usize> = vec![];
            loop {
                let mut edge_links: Vec<usize> = vec![]; // we prepend: collect in push order then reverse
                let mut unique_verts_in_group = 0;
                let mut unique_edges_in_group = 0;
                let v1 = self.edges[edge_arr[edge_index]].v1;
                vstack.push(v1);
                self.verts[v1].flag &= !VERT_NOT_IN_STACK;
                while let Some(v_iter) = vstack.pop() {
                    unique_verts_in_group += 1;
                    let e_first = self.verts[v_iter].e;
                    let mut e_iter = e_first;
                    loop {
                        if self.edges[e_iter].flag & EDGE_NOT_IN_STACK != 0 {
                            self.edges[e_iter].flag &= !EDGE_NOT_IN_STACK;
                            unique_edges_in_group += 1;
                            edge_links.push(e_iter);
                            let v_other = self.edge_other_vert(e_iter, v_iter);
                            if self.verts[v_other].flag & VERT_NOT_IN_STACK != 0 {
                                vstack.push(v_other);
                                self.verts[v_other].flag &= !VERT_NOT_IN_STACK;
                            }
                        }
                        e_iter = self.disk_edge_next(e_iter, v_iter);
                        if e_iter == e_first {
                            break;
                        }
                    }
                }
                edge_links.reverse();
                groups_list.push(Group { edge_links, vert_len: unique_verts_in_group, has_prev_edge: false, vmin: NONE, vmax: NONE, min_axis: [0.0; 2], max_axis: [0.0; 2] });
                edge_in_group_tot += unique_edges_in_group;
                if edge_in_group_tot == edge_arr_len {
                    break;
                }
                while self.edges[edge_arr[edge_index]].flag & EDGE_NOT_IN_STACK == 0 {
                    edge_index -= 1;
                }
            }
        }

        let mut result: Option<Vec<usize>> = None;
        let group_arr_len = groups_list.len();
        if group_arr_len > 1 {
            let axis_mat = axis_dominant_v3_to_m3(self.faces[f].no, false);
            // group_head is a list with the most recently created group first; group_arr is filled in reverse of that
            // traversal, i.e. creation order (boundary face first)
            let mut group_arr: Vec<Group> = groups_list;
            let mut vert_arr_len = 0;
            for g in group_arr.iter_mut() {
                let first = g.edge_links[0];
                g.vmin = self.edges[first].v1;
                g.vmax = self.edges[first].v2;
                let mut min_axis = [f32::MAX, f32::MAX];
                let mut max_axis = [-f32::MAX, -f32::MAX];
                for &e in &g.edge_links {
                    for j in 0..2 {
                        let v_iter = if j == 0 { self.edges[e].v1 } else { self.edges[e].v2 };
                        let co = self.verts[v_iter].co;
                        let axis_value = [dot_m3_v3_row_x(&axis_mat, co), dot_m3_v3_row_y(&axis_mat, co)];
                        if axis_pt_cmp(axis_value, min_axis) == -1 {
                            g.vmin = v_iter;
                            min_axis = axis_value;
                        }
                        if axis_pt_cmp(axis_value, max_axis) == 1 {
                            g.vmax = v_iter;
                            max_axis = axis_value;
                        }
                    }
                }
                g.min_axis = min_axis;
                g.max_axis = max_axis;
                g.has_prev_edge = false;
                vert_arr_len += g.vert_len;
            }
            qsort::qsort(&mut group_arr, &|g1: &Group, g2: &Group| {
                let mut test = axis_pt_cmp(g1.min_axis, g2.min_axis);
                if test == 0 {
                    test = axis_pt_cmp(g1.max_axis, g2.max_axis);
                }
                test
            });

            let mut vert_arr: Vec<usize> = vec![NONE; vert_arr_len];
            let mut verts_group_table: Vec<usize> = vec![0; vert_arr_len];
            let mut vert_coords_backup: Vec<V3> = vec![[0.0; 3]; vert_arr_len];
            let f_co_ref = self.verts[self.loops[self.faces[f].l_first].v].co;
            let mut v_index = 0usize;
            for (g_index, g) in group_arr.iter().enumerate() {
                for &e in &g.edge_links {
                    for j in 0..2 {
                        let v_iter = if j == 0 { self.edges[e].v1 } else { self.edges[e].v2 };
                        if self.verts[v_iter].flag & VERT_IN_ARRAY == 0 {
                            self.verts[v_iter].flag |= VERT_IN_ARRAY;
                            vert_coords_backup[v_index] = self.verts[v_iter].co;
                            let rel = sub(self.verts[v_iter].co, f_co_ref);
                            let co_2d = mul_v2_m3v3(&axis_mat, rel);
                            self.verts[v_iter].co = [co_2d[0], co_2d[1], 0.0];
                            self.verts[v_iter].idx = v_index as i32;
                            vert_arr[v_index] = v_iter;
                            verts_group_table[v_index] = g_index;
                            v_index += 1;
                        }
                    }
                }
            }

            let mut bvh = BvhTree::new(edge_arr_len, 1e-4, 8, 8);
            for i in 0..edge_arr_len {
                let e = edge_arr[i];
                let (c1, c2) = (self.verts[self.edges[e].v1].co, self.verts[self.edges[e].v2].co);
                bvh.insert(i as i32, &[[c1[0], c1[1], 0.0], [c2[0], c2[1], 0.0]]);
            }
            bvh.balance();

            let mut edge_net_new: Vec<usize> = edge_net_init.to_vec();
            let mut edge_arr_new: Vec<usize> = vec![];
            let mut vert_range = [0usize, group_arr[0].vert_len];
            for g_index in 1..group_arr_len {
                vert_range[0] = vert_range[1];
                vert_range[1] += group_arr[g_index].vert_len;
                if !group_arr[g_index].has_prev_edge {
                    let v_origin = group_arr[g_index].vmin;
                    let index_other = self.edgenet_find_connection(&bvh, &edge_arr, &edge_arr_new, vert_range, v_origin, false);
                    if index_other != -1 {
                        let v_end = vert_arr[index_other as usize];
                        let ne = self.edge_create(v_origin, v_end, None, false);
                        edge_net_new.push(ne);
                        edge_arr_new.push(ne);
                    }
                }
                {
                    let v_origin = group_arr[g_index].vmax;
                    let index_other = self.edgenet_find_connection(&bvh, &edge_arr, &edge_arr_new, vert_range, v_origin, true);
                    if index_other != -1 {
                        let v_end = vert_arr[index_other as usize];
                        let ne = self.edge_create(v_origin, v_end, None, false);
                        edge_net_new.push(ne);
                        edge_arr_new.push(ne);
                        let g_other = verts_group_table[index_other as usize];
                        group_arr[g_other].has_prev_edge = true;
                    }
                }
            }
            for i in 0..vert_arr_len {
                self.verts[vert_arr[i]].co = vert_coords_backup[i];
            }
            // (clear VERT_IN_ARRAY: shares the INTERNAL_TAG bit with VERT_NOT_IN_STACK, cleared below for edge_arr verts)
            result = Some(edge_net_new);
        }
        for &e in &edge_arr {
            self.edges[e].flag &= !EDGE_NOT_IN_STACK;
            let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
            self.verts[v1].flag &= !VERT_NOT_IN_STACK;
            self.verts[v2].flag &= !VERT_NOT_IN_STACK;
        }
        let _ = init_len;
        result
    }

    fn edgenet_find_connection(&mut self, bvh: &BvhTree, edge_arr: &[usize], edge_arr_new: &[usize], vert_range: [usize; 2], v_origin: usize, direction_sign: bool) -> i32 {
        const VERT_IS_VALID: u16 = INTERNAL_TAG;
        let mut dir = [0f32; 3];
        dir[0] = if direction_sign { 1.0 } else { -1.0 };
        let e_hit = self.test_edges_isect_2d_ray(bvh, edge_arr, edge_arr_new, vert_range, v_origin, dir);
        let mut v_other = NONE;
        if e_hit != NONE {
            let mut e_hit = e_hit;
            let mut v_other_fallback;
            let mut vert_search: Vec<usize> = vec![];
            let mut vert_blacklist: Vec<usize> = vec![];
            loop {
                let (e1, e2) = (self.edges[e_hit].v1, self.edges[e_hit].v2);
                let oc = self.verts[v_origin].co;
                let d1 = len_squared_v2v2([oc[0], oc[1]], [self.verts[e1].co[0], self.verts[e1].co[1]]);
                let d2 = len_squared_v2v2([oc[0], oc[1]], [self.verts[e2].co[0], self.verts[e2].co[1]]);
                let v_pair = if d1 > d2 { [e1, e2] } else { [e2, e1] };
                for j in 0..2 {
                    let v_iter = v_pair[j];
                    if self.verts[v_iter].flag & VERT_IS_VALID != 0 {
                        let ok = if direction_sign { self.verts[v_iter].co[0] > self.verts[v_origin].co[0] } else { self.verts[v_iter].co[0] < self.verts[v_origin].co[0] };
                        if ok {
                            vert_search.push(v_iter);
                            vert_blacklist.push(v_iter);
                            self.verts[v_iter].flag &= !VERT_IS_VALID;
                        }
                    }
                }
                v_other_fallback = v_other;
                v_other = vert_search.pop().unwrap_or(NONE);
                if v_other == NONE {
                    break;
                }
                let eh = self.test_edges_isect_2d_vert(bvh, edge_arr, edge_arr_new, vert_range, v_origin, v_other);
                if eh == NONE {
                    break;
                }
                e_hit = eh;
            }
            if v_other == NONE {
                v_other = v_other_fallback;
            }
            while let Some(v) = vert_blacklist.pop() {
                self.verts[v].flag |= VERT_IS_VALID;
            }
        }
        if v_other != NONE {
            self.verts[v_other].idx
        } else {
            -1
        }
    }

    fn edge_isect_verts_point_2d(&self, e: usize, v_a: usize, v_b: usize) -> Option<[f32; 2]> {
        let endpoint_bias = 1e-4f32;
        let (e1, e2) = (self.edges[e].v1, self.edges[e].v2);
        let p = |v: usize| [self.verts[v].co[0], self.verts[v].co[1]];
        let (k, pt) = isect_seg_seg_v2_point_ex(p(v_a), p(v_b), p(e1), p(e2), endpoint_bias);
        if k == 1 && e1 != v_a && e2 != v_a && e1 != v_b && e2 != v_b {
            Some(pt)
        } else {
            None
        }
    }

    fn test_edges_isect_2d_vert(&self, bvh: &BvhTree, edge_arr: &[usize], edge_arr_new: &[usize], vert_range: [usize; 2], v_origin: usize, v_other: usize) -> usize {
        let oc = self.verts[v_origin].co;
        let tc = self.verts[v_other].co;
        let d = [tc[0] - oc[0], tc[1] - oc[1]];
        let hd = (d[0] * d[0] + d[1] * d[1]).sqrt();
        // normalize_v2 (with its zero handling)
        let dn = normalize_v2(d);
        let dir = [dn[0], dn[1], 0.0];
        let dist_orig = if (d[0] * d[0] + d[1] * d[1]) > 1.0e-35f32 { hd } else { 0.0 };
        let mut hit_index: i32 = -1;
        let mut hit_dist = dist_orig;
        let p2 = |v: usize| [self.verts[v].co[0], self.verts[v].co[1]];
        bvh.ray_cast(oc, dir, &mut hit_index, &mut hit_dist, &mut |index: i32, cur: f32| {
            let e = edge_arr[index as usize];
            let v1_index = self.verts[self.edges[e].v1].idx;
            if let Some(co_isect) = self.edge_isect_verts_point_2d(e, v_origin, v_other) {
                let t = line_point_factor_v2(co_isect, p2(v_origin), p2(v_other));
                let dist_new = dist_orig * t;
                if dist_new < cur && dist_new > 0.0 && (v1_index < vert_range[0] as i32 || v1_index >= vert_range[1] as i32) {
                    return Some((index, dist_new));
                }
            }
            None
        });
        let mut e_hit = if hit_index != -1 { edge_arr[hit_index as usize] } else { NONE };
        if hit_index == -1 {
            let mut t_best = 1.0f32;
            for &e in edge_arr_new {
                if let Some(co_isect) = self.edge_isect_verts_point_2d(e, v_origin, v_other) {
                    let t_test = line_point_factor_v2(co_isect, p2(v_origin), p2(v_other));
                    if t_test < t_best {
                        t_best = t_test;
                        e_hit = e;
                    }
                }
            }
        }
        e_hit
    }

    fn test_edges_isect_2d_ray(&self, bvh: &BvhTree, edge_arr: &[usize], edge_arr_new: &[usize], vert_range: [usize; 2], v_origin: usize, dir: [f32; 3]) -> usize {
        let oc = self.verts[v_origin].co;
        let mut hit_index: i32 = -1;
        let mut hit_dist = super::bvh::RAYCAST_DIST_MAX;
        let p2 = |v: usize| [self.verts[v].co[0], self.verts[v].co[1]];
        let dir2 = [dir[0], dir[1]];
        bvh.ray_cast(oc, dir, &mut hit_index, &mut hit_dist, &mut |index: i32, cur: f32| {
            let e = edge_arr[index as usize];
            if let Some(dist_new) = isect_ray_seg_v2(p2(v_origin), dir2, p2(self.edges[e].v1), p2(self.edges[e].v2)) {
                if dist_new < cur && dist_new > 0.0 && self.edges[e].v1 != v_origin && self.edges[e].v2 != v_origin {
                    let v1_index = self.verts[self.edges[e].v1].idx;
                    if v1_index < vert_range[0] as i32 || v1_index >= vert_range[1] as i32 {
                        return Some((index, dist_new));
                    }
                }
            }
            None
        });
        let mut e_hit = if hit_index != -1 { edge_arr[hit_index as usize] } else { NONE };
        if hit_index != -1 {
            for &e in edge_arr_new {
                if let Some(dist_new) = isect_ray_seg_v2(p2(v_origin), dir2, p2(self.edges[e].v1), p2(self.edges[e].v2)) {
                    if self.edges[e].v1 != v_origin && self.edges[e].v2 != v_origin && dist_new < hit_dist {
                        hit_dist = dist_new;
                        e_hit = e;
                    }
                }
            }
        }
        e_hit
    }
}

fn len_squared_v2v2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = sub_v2(a, b);
    d[0] * d[0] + d[1] * d[1]
}

fn axis_pt_cmp(a: [f32; 2], b: [f32; 2]) -> i32 {
    if a[0] < b[0] {
        return -1;
    }
    if a[0] > b[0] {
        return 1;
    }
    if a[1] < b[1] {
        return -1;
    }
    if a[1] > b[1] {
        return 1;
    }
    0
}
