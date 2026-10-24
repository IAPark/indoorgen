//! BM_edge_rotate and the `beautify_fill` operator (BM_mesh_beautify_fill, area method, EDGE_RESTRICT_DEGENERATE).
use super::*;
use crate::assets::polyfill::bm_rotate_beauty_area;
use crate::dhash::HashSet;

impl BMesh {
    /// BM_face_other_vert_loop
    pub fn face_other_vert_loop(&self, f: usize, v_prev: usize, v: usize) -> usize {
        let l = self.face_vert_share_loop(f, v);
        let (p, n) = (self.loops[l].p, self.loops[l].n);
        if self.loops[p].v == v_prev {
            n
        } else {
            p
        }
    }
    /// BM_edge_face_pair
    pub fn edge_face_pair(&self, e: usize) -> Option<(usize, usize)> {
        self.edge_loop_pair(e).map(|(la, lb)| (self.loops[la].f, self.loops[lb].f))
    }
    /// BM_edge_rotate_check
    pub fn edge_rotate_check(&self, e: usize) -> bool {
        if let Some((fa, fb)) = self.edge_face_pair(e) {
            let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
            let la = self.face_other_vert_loop(fa, v2, v1);
            let lb = self.face_other_vert_loop(fb, v2, v1);
            if self.loops[la].v == self.loops[lb].v {
                return false;
            }
            let la = self.face_other_vert_loop(fa, v1, v2);
            let lb = self.face_other_vert_loop(fb, v1, v2);
            if self.loops[la].v == self.loops[lb].v {
                return false;
            }
            return true;
        }
        false
    }

    /// BM_faces_join_pair for two faces sharing exactly the edge `e` (do_del = true); returns the new face
    fn faces_join_pair_shared(&mut self, f_a: usize, f_b: usize, e: usize) -> usize {
        let mut chain: Vec<(usize, usize, usize)> = vec![]; // (from vert, to vert, edge)
        for &f in &[f_a, f_b] {
            for l in self.loops_of_face(f) {
                if self.loops[l].e != e {
                    let v = self.loops[l].v;
                    chain.push((v, self.edge_other_vert(self.loops[l].e, v), self.loops[l].e));
                }
            }
        }
        let n = chain.len();
        let mut verts = vec![];
        let mut edges = vec![];
        let mut cur = 0;
        for _ in 0..n {
            verts.push(chain[cur].0);
            edges.push(chain[cur].2);
            let to = chain[cur].1;
            cur = chain.iter().position(|c| c.0 == to).expect("join chain");
        }
        let f_new = self.face_create(&verts, &edges, Some(f_a), false);
        self.edge_kill(e);
        f_new
    }

    /// BM_edge_rotate (ccw = false, check_flag = BM_EDGEROT_CHECK_EXISTS); returns the new edge or NONE
    pub fn edge_rotate(&mut self, e: usize) -> usize {
        if !self.edge_rotate_check(e) {
            return NONE;
        }
        let (mut fa, mut fb) = self.edge_face_pair(e).unwrap();
        // BM_edge_ordered_verts
        let l0 = self.edges[e].l;
        let (v1o, v2o) = {
            let v = self.loops[l0].v;
            if v == self.edges[e].v1 {
                (self.edges[e].v1, self.edges[e].v2)
            } else {
                (self.edges[e].v2, self.edges[e].v1)
            }
        };
        std::mem::swap(&mut fa, &mut fb);
        let l1 = self.face_other_vert_loop(fb, v2o, v1o);
        let l2 = self.face_other_vert_loop(fa, v1o, v2o);
        let (v1, v2) = (self.loops[l1].v, self.loops[l2].v);
        if self.edge_exists(v1, v2) != NONE {
            return NONE;
        }
        let (f1, f2) = (self.loops[l1].f, self.loops[l2].f);
        let e_new = self.edge_create(v1, v2, Some(e), false);
        let la = self.face_edge_share_loop(f1, e);
        let lb = self.face_edge_share_loop(f2, e);
        assert!(self.loops[la].v != self.loops[lb].v, "edge_rotate: non-contiguous winding not supported");
        let f = self.faces_join_pair_shared(f1, f2, e);
        let l1 = self.face_vert_share_loop(f, v1);
        let l2 = self.face_vert_share_loop(f, v2);
        self.face_split_ex(f, l1, l2, true);
        e_new
    }
}

struct Heap {
    tree: Vec<(f32, usize)>,
    pos: Vec<usize>,
}
impl Heap {
    fn swap(&mut self, a: usize, b: usize) {
        self.tree.swap(a, b);
        self.pos[self.tree[a].1] = a;
        self.pos[self.tree[b].1] = b;
    }
    fn up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.tree[p].0 < self.tree[i].0 {
                break;
            }
            self.swap(p, i);
            i = p;
        }
    }
    fn down(&mut self, mut i: usize) {
        let size = self.tree.len();
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut s = if l < size && self.tree[l].0 < self.tree[i].0 { l } else { i };
            if r < size && self.tree[r].0 < self.tree[s].0 {
                s = r;
            }
            if s == i {
                break;
            }
            self.swap(i, s);
            i = s;
        }
    }
    fn insert(&mut self, v: f32, slot: usize) {
        self.tree.push((v, slot));
        let i = self.tree.len() - 1;
        self.pos[slot] = i;
        self.up(i);
    }
    fn pop_min(&mut self) -> usize {
        let top = self.tree[0].1;
        let last = self.tree.len() - 1;
        self.swap(0, last);
        self.tree.pop();
        self.pos[top] = usize::MAX;
        if !self.tree.is_empty() {
            self.down(0);
        }
        top
    }
    fn remove(&mut self, slot: usize) {
        let mut i = self.pos[slot];
        while i > 0 {
            let p = (i - 1) / 2;
            self.swap(p, i);
            i = p;
        }
        self.pop_min();
    }
}

type RotState = ([i32; 2], [i32; 2]);

impl BMesh {
    fn erot_state_ex(&self, e: usize) -> ([i32; 2], [i32; 2]) {
        let l = self.edges[e].l;
        let mut v = [self.verts[self.edges[e].v1].idx, self.verts[self.edges[e].v2].idx];
        if v[0] > v[1] {
            v.swap(0, 1);
        }
        let lp = self.loops[l].p;
        let lr = self.loops[l].rn;
        let lrp = self.loops[lr].p;
        let mut f = [self.verts[self.loops[lp].v].idx, self.verts[self.loops[lrp].v].idx];
        if f[0] > f[1] {
            f.swap(0, 1);
        }
        (v, f)
    }
    fn calc_rotate_beauty(&self, e: usize) -> f32 {
        let l = self.edges[e].l;
        let lr = self.loops[l].rn;
        let v1 = self.loops[self.loops[l].p].v;
        let v2 = self.loops[l].v;
        let v3 = self.loops[self.loops[lr].p].v;
        let v4 = self.loops[self.loops[l].n].v;
        if v1 == v3 {
            return f32::MAX;
        }
        bm_rotate_beauty_area(self.verts[v1].co, self.verts[v2].co, self.verts[v3].co, self.verts[v4].co, true)
    }

    /// `beautify_fill` on the given candidate edges (only manifold, rotatable ones are used); returns nothing, the mesh is
    /// modified in place.  Vertex `idx` fields must hold the vertex indices.
    pub fn beautify_fill(&mut self, cand: &[usize]) {
        let mut edge_array: Vec<usize> = cand.iter().copied().filter(|&e| self.edge_rotate_check(e)).collect();
        let n = edge_array.len();
        let mut states: Vec<Option<HashSet<RotState>>> = (0..n).map(|_| None).collect();
        let mut heap = Heap { tree: vec![], pos: vec![usize::MAX; n] };
        for i in 0..n {
            let e = edge_array[i];
            let cost = self.calc_rotate_beauty(e);
            if cost < 0.0 {
                heap.insert(cost, i);
            }
            self.edges[e].idx = i as i32;
        }
        while !heap.tree.is_empty() {
            let i = heap.pop_min();
            let e = edge_array[i];
            let e_new = self.edge_rotate(e);
            if e_new == NONE {
                continue;
            }
            let st = states[i].get_or_insert_with(HashSet::default);
            st.insert(self.erot_state_ex(e_new));
            edge_array[i] = e_new;
            self.edges[e_new].idx = i as i32;
            // bm_edge_update_beauty_cost
            let l = self.edges[e_new].l;
            let lr = self.loops[l].rn;
            let arr = [
                self.loops[self.loops[l].n].e,
                self.loops[self.loops[l].p].e,
                self.loops[self.loops[lr].n].e,
                self.loops[self.loops[lr].p].e,
            ];
            for &ee in &arr {
                let idx = self.edges[ee].idx;
                if idx >= 0 && (idx as usize) < n && edge_array[idx as usize] == ee {
                    let j = idx as usize;
                    if heap.pos[j] != usize::MAX {
                        heap.remove(j);
                    }
                    // alternate state check
                    if let Some(set) = &states[j] {
                        let (v, f) = self.erot_state_ex(ee);
                        if set.contains(&(f, v)) {
                            continue;
                        }
                    }
                    let cost = self.calc_rotate_beauty(ee);
                    if cost < 0.0 {
                        heap.insert(cost, j);
                    }
                }
            }
        }
    }
}
