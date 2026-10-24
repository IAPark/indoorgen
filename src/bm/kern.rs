//! More BMesh kernel / mods operations needed by the FLOAT-solver mesh boolean (`bmesh_intersect`):
//! edge split, vertex splice, vertex collapse, face kill (loose), face normal flip, face swap.
use super::*;

pub const FLAG_WALK: u8 = 4;
pub const FLAG_WALK_ALT: u8 = 8;
pub const INTERNAL_TAG: u16 = 1 << 7;

impl BMesh {
    // ------------------------------------------------------------------ low level frees (no cascade)
    pub fn kill_only_loop(&mut self, l: usize) {
        self.loops[l].alive = false;
        self.lfree.push(l);
    }
    pub fn kill_only_edge(&mut self, e: usize) {
        self.edges[e].alive = false;
        self.totedge -= 1;
        self.efree.push(e);
    }
    pub fn kill_only_vert(&mut self, v: usize) {
        self.verts[v].alive = false;
        self.totvert -= 1;
        self.vfree.push(v);
    }

    pub fn disk_edge_remove_pub(&mut self, e: usize, v: usize) {
        self.disk_edge_remove(e, v);
    }

    /// bmesh_disk_vert_swap
    fn disk_vert_swap(&mut self, e: usize, v_dst: usize, v_src: usize) {
        if self.edges[e].v1 == v_src {
            self.edges[e].v1 = v_dst;
            self.edges[e].d1 = [NONE, NONE];
        } else if self.edges[e].v2 == v_src {
            self.edges[e].v2 = v_dst;
            self.edges[e].d2 = [NONE, NONE];
        }
    }
    /// bmesh_disk_vert_replace
    pub fn disk_vert_replace(&mut self, e: usize, v_dst: usize, v_src: usize) {
        self.disk_edge_remove(e, v_src);
        self.disk_vert_swap(e, v_dst, v_src);
        self.disk_edge_append(e, v_dst);
    }
    /// bmesh_edge_vert_swap
    pub fn edge_vert_swap(&mut self, e: usize, v_dst: usize, v_src: usize) {
        let first = self.edges[e].l;
        if first != NONE {
            let mut l = first;
            loop {
                if self.loops[l].v == v_src {
                    self.loops[l].v = v_dst;
                } else {
                    let n = self.loops[l].n;
                    if self.loops[n].v == v_src {
                        self.loops[n].v = v_dst;
                    }
                }
                l = self.loops[l].rn;
                if l == first {
                    break;
                }
            }
        }
        self.disk_vert_replace(e, v_dst, v_src);
    }

    // ------------------------------------------------------------------ split edge
    /// bmesh_kernel_split_edge_make_vert: returns (v_new, e_new); `e` keeps (v_new, v_old), `e_new` is (tv, v_new)
    pub fn split_edge_make_vert(&mut self, tv: usize, e: usize) -> (usize, usize) {
        let co = self.verts[tv].co;
        let v_new = self.vert_create(co, Some(tv));
        let e_new = self.edge_create(tv, v_new, Some(e), false);
        self.disk_edge_remove(e_new, tv);
        self.disk_edge_remove(e_new, v_new);
        self.disk_vert_replace(e, v_new, tv);
        self.disk_edge_append(e_new, v_new);
        self.disk_edge_append(e_new, tv);

        let mut l_next = self.edges[e].l;
        self.edges[e].l = NONE;
        if l_next != NONE {
            while l_next != NONE {
                let l = l_next;
                let f = self.loops[l].f;
                self.faces[f].len += 1;
                l_next = if l_next != self.loops[l_next].rn { self.loops[l_next].rn } else { NONE };
                // bmesh_radial_loop_unlink
                if self.loops[l].rn != l {
                    let (rn, rp) = (self.loops[l].rn, self.loops[l].rp);
                    self.loops[rn].rp = rp;
                    self.loops[rp].rn = rn;
                }
                self.loops[l].rn = NONE;
                self.loops[l].rp = NONE;

                let l_new = self.loop_create(NONE, NONE, f);
                // l_new->prev = l; l_new->next = l->next; ...
                self.loops[l_new].p = l;
                let ln = self.loops[l].n;
                self.loops[l_new].n = ln;
                self.loops[l].n = l_new;
                self.loops[ln].p = l_new;
                self.loops[l_new].v = v_new;
                let nxt = self.loops[l_new].n;
                let nv = self.loops[nxt].v;
                let in_e = (v_new == self.edges[e].v1 && nv == self.edges[e].v2) || (v_new == self.edges[e].v2 && nv == self.edges[e].v1);
                let in_enew = (v_new == self.edges[e_new].v1 && nv == self.edges[e_new].v2) || (v_new == self.edges[e_new].v2 && nv == self.edges[e_new].v1);
                if in_e {
                    self.loops[l_new].e = e;
                    self.loops[l].e = e_new;
                    self.loops[l].rn = NONE;
                    self.loops[l].rp = NONE;
                    self.radial_loop_append(e, l_new);
                    self.radial_loop_append(e_new, l);
                } else if in_enew {
                    self.loops[l_new].e = e_new;
                    self.loops[l].e = e;
                    self.loops[l].rn = NONE;
                    self.loops[l].rp = NONE;
                    self.radial_loop_append(e_new, l_new);
                    self.radial_loop_append(e, l);
                }
            }
        }
        (v_new, e_new)
    }

    /// BM_edge_split (no custom data): returns (v_new, e_new)
    pub fn edge_split(&mut self, e: usize, v: usize, fac: f32) -> (usize, usize) {
        let v_other = self.edge_other_vert(e, v);
        let (v_new, e_new) = self.split_edge_make_vert(v, e);
        let (vco, oco) = (self.verts[v].co, self.verts[v_other].co);
        let d = query::sub(oco, vco);
        self.verts[v_new].co = query::madd(vco, d, fac);
        self.edges[e_new].flag = self.edges[e].flag;
        self.elem_attrs_copy_edge(e, e_new);
        (v_new, e_new)
    }

    // ------------------------------------------------------------------ splice
    pub fn vert_in_face(&self, v: usize, f: usize) -> bool {
        self.face_vert_share_loop(f, v) != NONE
    }
    /// BM_vert_pair_share_face_check
    pub fn vert_pair_share_face_check(&self, a: usize, b: usize) -> bool {
        if self.verts[a].e != NONE && self.verts[b].e != NONE {
            for f in self.faces_of_vert(a) {
                if self.vert_in_face(b, f) {
                    return true;
                }
            }
        }
        false
    }
    /// BM_vert_splice_check_double
    pub fn vert_splice_check_double(&mut self, a: usize, b: usize) -> bool {
        let mut is_double = false;
        if self.verts[a].e != NONE && self.verts[b].e != NONE {
            for e in self.edges_of_vert(a) {
                let vo = self.edge_other_vert(e, a);
                self.verts[vo].api |= FLAG_WALK;
            }
            for e in self.edges_of_vert(b) {
                let vo = self.edge_other_vert(e, b);
                if self.verts[vo].api & FLAG_WALK != 0 {
                    is_double = true;
                    break;
                }
            }
            for e in self.edges_of_vert(a) {
                let vo = self.edge_other_vert(e, a);
                self.verts[vo].api &= !FLAG_WALK;
            }
        }
        is_double
    }
    /// BM_vert_splice: merge `v_src` into `v_dst`
    pub fn vert_splice(&mut self, v_dst: usize, v_src: usize) -> bool {
        if v_src == v_dst {
            return false;
        }
        loop {
            let e = self.verts[v_src].e;
            if e == NONE {
                break;
            }
            self.edge_vert_swap(e, v_dst, v_src);
        }
        self.vert_kill(v_src);
        true
    }
    /// BM_edge_splice
    pub fn edge_splice(&mut self, e_dst: usize, e_src: usize) -> bool {
        if !self.vert_in_edge(e_src, self.edges[e_dst].v1) || !self.vert_in_edge(e_src, self.edges[e_dst].v2) {
            return false;
        }
        while self.edges[e_src].l != NONE {
            let l = self.edges[e_src].l;
            self.radial_loop_remove(e_src, l);
            self.radial_loop_append(e_dst, l);
        }
        self.edge_kill(e_src);
        true
    }

    pub fn disk_count_at_most(&self, v: usize, count_max: usize) -> usize {
        let first = self.verts[v].e;
        if first == NONE {
            return 0;
        }
        let mut n = 0;
        let mut e = first;
        loop {
            n += 1;
            if n == count_max {
                break;
            }
            e = self.disk_edge_next(e, v);
            if e == first {
                break;
            }
        }
        n
    }

    /// bmesh_kernel_join_edge_kill_vert with check_edge_exists=true, kill_degenerate=false, kill_duplicate=false, do_del
    pub fn join_edge_kill_vert(&mut self, e_kill: usize, v_kill: usize, do_del: bool) -> usize {
        if !self.vert_in_edge(e_kill, v_kill) {
            return NONE;
        }
        if self.disk_count_at_most(v_kill, 3) == 2 {
            let e_old = self.disk_edge_next(e_kill, v_kill);
            let v_target = self.edge_other_vert(e_kill, v_kill);
            let v_old = self.edge_other_vert(e_old, v_kill);
            // check for double edges
            if self.vert_in_edge(e_old, v_kill) && self.vert_in_edge(e_old, v_target) {
                return NONE;
            }
            let e_splice = self.edge_exists(v_target, v_old);
            self.disk_vert_replace(e_old, v_target, v_kill);
            self.disk_edge_remove(e_kill, v_target);
            let e_kill_l = self.edges[e_kill].l;
            if e_kill_l != NONE {
                let mut l_kill = e_kill_l;
                loop {
                    let ln = self.loops[l_kill].n;
                    let lp = self.loops[l_kill].p;
                    if self.loops[ln].v == v_kill {
                        self.loops[ln].v = v_target;
                    }
                    self.loops[ln].p = lp;
                    self.loops[lp].n = ln;
                    let f = self.loops[l_kill].f;
                    if self.faces[f].l_first == l_kill {
                        self.faces[f].l_first = ln;
                    }
                    self.faces[f].len -= 1;
                    let l_kill_next = self.loops[l_kill].rn;
                    self.kill_only_loop(l_kill);
                    l_kill = l_kill_next;
                    if l_kill == e_kill_l {
                        break;
                    }
                }
            }
            self.kill_only_edge(e_kill);
            if do_del {
                self.kill_only_vert(v_kill);
            } else {
                self.verts[v_kill].e = NONE;
            }
            if e_splice != NONE {
                self.edge_splice(e_old, e_splice);
            }
            return e_old;
        }
        NONE
    }

    /// BM_vert_collapse_edge(bm, e_kill, v_kill, do_del=true, kill_degenerate=false, kill_duplicate=false)
    pub fn vert_collapse_edge(&mut self, e_kill: usize, v_kill: usize) -> usize {
        self.join_edge_kill_vert(e_kill, v_kill, true)
    }

    /// BM_vert_is_edge_pair
    pub fn vert_is_edge_pair(&self, v: usize) -> bool {
        let e = self.verts[v].e;
        if e != NONE {
            let e_other = self.disk_edge_next(e, v);
            return e_other != e && self.disk_edge_next(e_other, v) == e;
        }
        false
    }

    // ------------------------------------------------------------------ face ops
    /// BM_face_kill_loose
    pub fn face_kill_loose(&mut self, f: usize) {
        if self.faces[f].l_first != NONE {
            let l_first = self.faces[f].l_first;
            let mut l_iter = l_first;
            loop {
                let l_next = self.loops[l_iter].n;
                let e = self.loops[l_iter].e;
                self.radial_loop_remove(e, l_iter);
                self.kill_only_loop(l_iter);
                if self.edges[e].l == NONE {
                    let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
                    self.disk_edge_remove(e, v1);
                    self.disk_edge_remove(e, v2);
                    self.kill_only_edge(e);
                    if self.verts[v1].e == NONE {
                        self.kill_only_vert(v1);
                    }
                    if self.verts[v2].e == NONE {
                        self.kill_only_vert(v2);
                    }
                }
                l_iter = l_next;
                if l_iter == l_first {
                    break;
                }
            }
        }
        self.faces[f].alive = false;
        self.totface -= 1;
        self.ffree.push(f);
    }

    /// bmesh_kernel_loop_reverse
    pub fn loop_reverse(&mut self, f: usize) {
        let l_first = self.faces[f].l_first;
        let lp0 = self.loops[l_first].p;
        let mut e_prev = self.loops[lp0].e;
        let mut l_prev_radial_next = self.loops[lp0].rn;
        let mut l_prev_radial_prev = self.loops[lp0].rp;
        let mut is_prev_boundary = l_prev_radial_next == self.loops[l_prev_radial_next].rn;
        let mut l_iter = l_first;
        loop {
            let e_iter = self.loops[l_iter].e;
            let l_iter_radial_next = self.loops[l_iter].rn;
            let l_iter_radial_prev = self.loops[l_iter].rp;
            let is_iter_boundary = l_iter_radial_next == self.loops[l_iter_radial_next].rn;
            if is_prev_boundary {
                self.loops[l_iter].rn = l_iter;
                self.loops[l_iter].rp = l_iter;
            } else {
                self.loops[l_iter].rn = l_prev_radial_next;
                self.loops[l_iter].rp = l_prev_radial_prev;
                self.loops[l_prev_radial_next].rp = l_iter;
                self.loops[l_prev_radial_prev].rn = l_iter;
            }
            if self.edges[e_iter].l == l_iter {
                self.edges[e_iter].l = self.loops[l_iter].n;
            }
            self.loops[l_iter].e = e_prev;
            let (n, p) = (self.loops[l_iter].n, self.loops[l_iter].p);
            self.loops[l_iter].n = p;
            self.loops[l_iter].p = n;
            e_prev = e_iter;
            l_prev_radial_next = l_iter_radial_next;
            l_prev_radial_prev = l_iter_radial_prev;
            is_prev_boundary = is_iter_boundary;
            l_iter = self.loops[l_iter].p;
            if l_iter == l_first {
                break;
            }
        }
    }
    /// BM_face_normal_flip
    pub fn face_normal_flip(&mut self, f: usize) {
        self.loop_reverse(f);
        let n = self.faces[f].no;
        self.faces[f].no = [-n[0], -n[1], -n[2]];
    }

    /// BM_edge_in_face
    pub fn edge_in_face(&self, e: usize, f: usize) -> bool {
        let first = self.edges[e].l;
        if first == NONE {
            return false;
        }
        let mut l = first;
        loop {
            if self.loops[l].f == f {
                return true;
            }
            l = self.loops[l].rn;
            if l == first {
                return false;
            }
        }
    }
    /// BM_face_share_edge_check
    pub fn face_share_edge_check(&self, f1: usize, f2: usize) -> bool {
        let first = self.faces[f1].l_first;
        let mut l = first;
        loop {
            if self.edge_in_face(self.loops[l].e, f2) {
                return true;
            }
            l = self.loops[l].n;
            if l == first {
                return false;
            }
        }
    }
}
