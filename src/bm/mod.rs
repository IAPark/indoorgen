//! Minimal BMesh (Blender's half-edge mesh) with the same element ordering semantics: disk cycles, radial cycles, and mempool
//! slot reuse (freed slots are reused LIFO, iteration is in slot order), as needed to reproduce Blender's bmesh operators exactly.
pub mod bevel;
pub mod bridge;
pub mod bvh;
pub mod cdata;
pub mod edgering;
pub mod hull;
pub mod edgenet;
pub mod isect;
pub mod kern;
pub mod qsort;
pub mod query;
pub mod rotate;
pub mod scanfill;

pub const NONE: usize = usize::MAX;
pub type V3 = [f32; 3];

pub const ELEM_SELECT: u16 = 1;
pub const ELEM_HIDDEN: u16 = 2;
pub const ELEM_SEAM: u16 = 4;
pub const ELEM_SMOOTH: u16 = 8;
pub const ELEM_TAG: u16 = 16;
pub const ELEM_DRAW: u16 = 32;
pub const ELEM_LONG_TAG: u16 = 1 << 6;

#[derive(Clone, Default, Debug)]
pub struct BMVert {
    pub co: V3,
    pub no: V3,
    pub e: usize,
    pub flag: u16,
    pub api: u8,
    pub idx: i32,
    pub alive: bool,
    /// free user payload (0 = none), copied from the example vertex by `vert_create`
    pub user: usize,
}
#[derive(Clone, Default, Debug)]
pub struct BMEdge {
    /// free user payload (0 = none), copied from the example edge by `edge_create` / attribute copies
    pub user: usize,
    pub v1: usize,
    pub v2: usize,
    /// disk links (next, prev) around v1 / v2
    pub d1: [usize; 2],
    pub d2: [usize; 2],
    pub l: usize,
    pub flag: u16,
    pub api: u8,
    pub idx: i32,
    pub alive: bool,
}
#[derive(Clone, Default, Debug)]
pub struct BMLoop {
    pub v: usize,
    pub e: usize,
    pub f: usize,
    pub rn: usize,
    pub rp: usize,
    pub n: usize,
    pub p: usize,
    pub flag: u16,
    pub idx: i32,
    pub alive: bool,
}
#[derive(Clone, Default, Debug)]
pub struct BMFace {
    pub len: usize,
    pub l_first: usize,
    pub no: V3,
    pub mat_nr: i16,
    pub flag: u16,
    pub api: u8,
    pub idx: i32,
    pub alive: bool,
    /// free user payload (copied by attribute copies, swapped by `face_swap_data`)
    pub user: usize,
}

#[derive(Default)]
pub struct BMesh {
    pub verts: Vec<BMVert>,
    pub edges: Vec<BMEdge>,
    pub loops: Vec<BMLoop>,
    pub faces: Vec<BMFace>,
    vfree: Vec<usize>,
    efree: Vec<usize>,
    lfree: Vec<usize>,
    ffree: Vec<usize>,
    pub totvert: usize,
    pub totedge: usize,
    pub totface: usize,
    /// loop (corner) custom data (empty unless a tool sets layers)
    pub ld: cdata::LoopData,
}

impl BMesh {
    pub fn new() -> BMesh {
        BMesh::default()
    }

    // ------------------------------------------------------------------ create
    pub fn vert_create(&mut self, co: V3, example: Option<usize>) -> usize {
        let mut v = BMVert { co, no: [0.0; 3], e: NONE, flag: 0, api: 0, idx: -1, alive: true, user: 0 };
        if let Some(ex) = example {
            // BM_elem_attrs_copy: hflag (without select) and normal
            v.flag = self.verts[ex].flag & !ELEM_SELECT;
            v.no = self.verts[ex].no;
            v.user = self.verts[ex].user;
        }
        self.totvert += 1;
        match self.vfree.pop() {
            Some(i) => {
                self.verts[i] = v;
                i
            }
            None => {
                self.verts.push(v);
                self.verts.len() - 1
            }
        }
    }

    pub fn edge_create(&mut self, v1: usize, v2: usize, example: Option<usize>, no_double: bool) -> usize {
        if no_double {
            let e = self.edge_exists(v1, v2);
            if e != NONE {
                return e;
            }
        }
        let mut e = BMEdge { user: 0, v1, v2, d1: [NONE; 2], d2: [NONE; 2], l: NONE, flag: ELEM_SMOOTH | ELEM_DRAW, api: 0, idx: -1, alive: true };
        if let Some(ex) = example {
            e.flag = self.edges[ex].flag & !ELEM_SELECT;
            e.user = self.edges[ex].user;
        }
        self.totedge += 1;
        let id = match self.efree.pop() {
            Some(i) => {
                self.edges[i] = e;
                i
            }
            None => {
                self.edges.push(e);
                self.edges.len() - 1
            }
        };
        self.disk_edge_append(id, v1);
        self.disk_edge_append(id, v2);
        id
    }

    fn loop_create(&mut self, v: usize, e: usize, f: usize) -> usize {
        let l = BMLoop { v, e, f, rn: NONE, rp: NONE, n: NONE, p: NONE, flag: 0, idx: -1, alive: true };
        let i = match self.lfree.pop() {
            Some(i) => {
                self.loops[i] = l;
                i
            }
            None => {
                self.loops.push(l);
                self.loops.len() - 1
            }
        };
        self.ld.reset(i);
        i
    }

    /// BM_face_create (edges given)
    pub fn face_create(&mut self, verts: &[usize], edges: &[usize], example: Option<usize>, no_double: bool) -> usize {
        let len = verts.len();
        if len == 0 {
            return NONE;
        }
        if no_double {
            let f = self.face_exists(verts);
            if f != NONE {
                return f;
            }
        }
        let f = BMFace { len: 0, l_first: NONE, no: [0.0; 3], mat_nr: 0, flag: 0, api: 0, idx: -1, alive: true, user: 0 };
        self.totface += 1;
        let fid = match self.ffree.pop() {
            Some(i) => {
                self.faces[i] = f;
                i
            }
            None => {
                self.faces.push(f);
                self.faces.len() - 1
            }
        };
        let startl = self.loop_create(verts[0], edges[0], fid);
        self.radial_loop_append(edges[0], startl);
        self.faces[fid].l_first = startl;
        let mut lastl = startl;
        for i in 1..len {
            let l = self.loop_create(verts[i], edges[i], fid);
            self.radial_loop_append(edges[i], l);
            self.loops[l].p = lastl;
            self.loops[lastl].n = l;
            lastl = l;
        }
        self.loops[startl].p = lastl;
        self.loops[lastl].n = startl;
        self.faces[fid].len = len;
        if let Some(ex) = example {
            self.elem_attrs_copy_face(ex, fid);
        }
        fid
    }

    /// BM_face_split (bmesh_kernel_split_face_make_edge): returns (new face, loop of the new face on the new edge)
    pub fn face_split(&mut self, f: usize, l_v1: usize, l_v2: usize) -> (usize, usize) {
        self.face_split_ex(f, l_v1, l_v2, false)
    }
    /// BM_face_split with `no_double` (reuses an existing edge between the two verts)
    pub fn face_split_ex(&mut self, f: usize, l_v1: usize, l_v2: usize, no_double: bool) -> (usize, usize) {
        let v1 = self.loops[l_v1].v;
        let v2 = self.loops[l_v2].v;
        let e = self.edge_create(v1, v2, None, no_double);
        // bm_face_create__sfme
        let nf = BMFace { len: 0, l_first: NONE, no: [0.0; 3], mat_nr: 0, flag: 0, api: 0, idx: -1, alive: true, user: 0 };
        self.totface += 1;
        let f2 = match self.ffree.pop() {
            Some(i) => {
                self.faces[i] = nf;
                i
            }
            None => {
                self.faces.push(nf);
                self.faces.len() - 1
            }
        };
        self.elem_attrs_copy_face(f, f2);
        let l_f1 = self.loop_create(v2, e, f);
        let l_f2 = self.loop_create(v1, e, f2);
        let (p2, p1) = (self.loops[l_v2].p, self.loops[l_v1].p);
        self.loops[l_f1].p = p2;
        self.loops[l_f2].p = p1;
        self.loops[p2].n = l_f1;
        self.loops[p1].n = l_f2;
        self.loops[l_f1].n = l_v1;
        self.loops[l_f2].n = l_v2;
        self.loops[l_v1].p = l_f1;
        self.loops[l_v2].p = l_f2;
        // find which of the faces the original first loop is in
        let mut first_loop_f1 = false;
        let mut it = l_f1;
        loop {
            if it == self.faces[f].l_first {
                first_loop_f1 = true;
            }
            it = self.loops[it].n;
            if it == l_f1 {
                break;
            }
        }
        if first_loop_f1 {
            let lf = self.faces[f].l_first;
            if self.loops[lf].p == l_f1 {
                self.faces[f2].l_first = self.loops[l_f2].p;
            } else if self.loops[lf].n == l_f1 {
                self.faces[f2].l_first = self.loops[l_f2].n;
            } else {
                self.faces[f2].l_first = l_f2;
            }
        } else {
            let lf = self.faces[f].l_first;
            self.faces[f2].l_first = lf;
            if self.loops[lf].p == l_f2 {
                self.faces[f].l_first = self.loops[l_f1].p;
            } else if self.loops[lf].n == l_f2 {
                self.faces[f].l_first = self.loops[l_f1].n;
            } else {
                self.faces[f].l_first = l_f1;
            }
        }
        let first2 = self.faces[f2].l_first;
        let mut it = first2;
        let mut f2len = 0;
        loop {
            self.loops[it].f = f2;
            f2len += 1;
            it = self.loops[it].n;
            if it == first2 {
                break;
            }
        }
        self.radial_loop_append(e, l_f1);
        self.radial_loop_append(e, l_f2);
        self.faces[f2].len = f2len;
        let first = self.faces[f].l_first;
        let mut it = first;
        let mut f1len = 0;
        loop {
            f1len += 1;
            it = self.loops[it].n;
            if it == first {
                break;
            }
        }
        self.faces[f].len = f1len;
        (f2, l_f2)
    }

    /// BM_face_create_verts with create_edges = true
    pub fn face_create_verts(&mut self, verts: &[usize], example: Option<usize>, create_edges: bool) -> usize {
        let len = verts.len();
        let mut edges = vec![NONE; len];
        if create_edges {
            let mut i_prev = len - 1;
            for i in 0..len {
                edges[i_prev] = self.edge_create(verts[i_prev], verts[i], None, true);
                i_prev = i;
            }
        } else {
            let mut i_prev = len - 1;
            for i in 0..len {
                let e = self.edge_exists(verts[i_prev], verts[i]);
                if e == NONE {
                    return NONE;
                }
                edges[i_prev] = e;
                i_prev = i;
            }
        }
        self.face_create(verts, &edges, example, false)
    }

    pub fn elem_attrs_copy_face(&mut self, src: usize, dst: usize) {
        let (flag, no, mat) = (self.faces[src].flag, self.faces[src].no, self.faces[src].mat_nr);
        let user = self.faces[src].user;
        let d = &mut self.faces[dst];
        d.user = user;
        d.flag = (d.flag & ELEM_SELECT) | (flag & !ELEM_SELECT);
        d.no = no;
        d.mat_nr = mat;
    }
    pub fn elem_attrs_copy_vert(&mut self, src: usize, dst: usize) {
        let (flag, no) = (self.verts[src].flag, self.verts[src].no);
        let d = &mut self.verts[dst];
        d.flag = (d.flag & ELEM_SELECT) | (flag & !ELEM_SELECT);
        d.no = no;
    }
    pub fn elem_attrs_copy_edge(&mut self, src: usize, dst: usize) {
        let (flag, user) = (self.edges[src].flag, self.edges[src].user);
        let d = &mut self.edges[dst];
        d.flag = (d.flag & ELEM_SELECT) | (flag & !ELEM_SELECT);
        d.user = user;
    }

    /// bmesh_face_swap_data: swap the loop cycles / len / normal / flags of two faces (the element slots stay put)
    pub fn face_swap_data(&mut self, a: usize, b: usize) {
        for l in self.loops_of_face(a) {
            self.loops[l].f = b;
        }
        for l in self.loops_of_face(b) {
            self.loops[l].f = a;
        }
        let idx_a = self.faces[a].idx;
        let idx_b = self.faces[b].idx;
        self.faces.swap(a, b);
        self.faces[a].idx = idx_a;
        self.faces[b].idx = idx_b;
    }

    // ------------------------------------------------------------------ disk / radial cycles
    fn disk_link(&self, e: usize, v: usize) -> [usize; 2] {
        if v == self.edges[e].v1 {
            self.edges[e].d1
        } else {
            self.edges[e].d2
        }
    }
    fn disk_link_mut(&mut self, e: usize, v: usize) -> &mut [usize; 2] {
        if v == self.edges[e].v1 {
            &mut self.edges[e].d1
        } else {
            &mut self.edges[e].d2
        }
    }
    pub fn disk_edge_next(&self, e: usize, v: usize) -> usize {
        self.disk_link(e, v)[0]
    }
    pub fn disk_edge_prev(&self, e: usize, v: usize) -> usize {
        self.disk_link(e, v)[1]
    }
    fn disk_edge_append(&mut self, e: usize, v: usize) {
        if self.verts[v].e == NONE {
            self.verts[v].e = e;
            *self.disk_link_mut(e, v) = [e, e];
        } else {
            let ve = self.verts[v].e;
            let dl2 = self.disk_link(ve, v);
            let dl3_edge = dl2[1];
            // dl1 = link of e: next = v->e, prev = dl2.prev
            *self.disk_link_mut(e, v) = [ve, dl2[1]];
            self.disk_link_mut(ve, v)[1] = e;
            if dl3_edge != NONE {
                self.disk_link_mut(dl3_edge, v)[0] = e;
            }
        }
    }
    fn disk_edge_remove(&mut self, e: usize, v: usize) {
        let dl1 = self.disk_link(e, v);
        if dl1[1] != NONE {
            self.disk_link_mut(dl1[1], v)[0] = dl1[0];
        }
        if dl1[0] != NONE {
            self.disk_link_mut(dl1[0], v)[1] = dl1[1];
        }
        if self.verts[v].e == e {
            self.verts[v].e = if e != dl1[0] { dl1[0] } else { NONE };
        }
        *self.disk_link_mut(e, v) = [NONE, NONE];
    }
    fn radial_loop_append(&mut self, e: usize, l: usize) {
        let el = self.edges[e].l;
        if el == NONE {
            self.edges[e].l = l;
            self.loops[l].rn = l;
            self.loops[l].rp = l;
        } else {
            self.loops[l].rp = el;
            let eln = self.loops[el].rn;
            self.loops[l].rn = eln;
            self.loops[eln].rp = l;
            self.loops[el].rn = l;
            self.edges[e].l = l;
        }
        self.loops[l].e = e;
    }
    fn radial_loop_remove(&mut self, e: usize, l: usize) {
        if self.loops[l].rn != l {
            if l == self.edges[e].l {
                self.edges[e].l = self.loops[l].rn;
            }
            let (rn, rp) = (self.loops[l].rn, self.loops[l].rp);
            self.loops[rn].rp = rp;
            self.loops[rp].rn = rn;
        } else if l == self.edges[e].l {
            self.edges[e].l = NONE;
        }
        self.loops[l].rn = NONE;
        self.loops[l].rp = NONE;
        self.loops[l].e = NONE;
    }

    // ------------------------------------------------------------------ kill
    pub fn face_kill(&mut self, f: usize) {
        if self.faces[f].l_first != NONE {
            let l_first = self.faces[f].l_first;
            let mut l_iter = l_first;
            loop {
                let l_next = self.loops[l_iter].n;
                let e = self.loops[l_iter].e;
                self.radial_loop_remove(e, l_iter);
                self.loops[l_iter].alive = false;
                self.lfree.push(l_iter);
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
    pub fn edge_kill(&mut self, e: usize) {
        while self.edges[e].l != NONE {
            let f = self.loops[self.edges[e].l].f;
            self.face_kill(f);
        }
        let (v1, v2) = (self.edges[e].v1, self.edges[e].v2);
        self.disk_edge_remove(e, v1);
        self.disk_edge_remove(e, v2);
        self.edges[e].alive = false;
        self.totedge -= 1;
        self.efree.push(e);
    }
    pub fn vert_kill(&mut self, v: usize) {
        while self.verts[v].e != NONE {
            let e = self.verts[v].e;
            self.edge_kill(e);
        }
        self.verts[v].alive = false;
        self.totvert -= 1;
        self.vfree.push(v);
    }

    // ------------------------------------------------------------------ iteration (pool order)
    pub fn vert_ids(&self) -> Vec<usize> {
        (0..self.verts.len()).filter(|&i| self.verts[i].alive).collect()
    }
    pub fn edge_ids(&self) -> Vec<usize> {
        (0..self.edges.len()).filter(|&i| self.edges[i].alive).collect()
    }
    pub fn face_ids(&self) -> Vec<usize> {
        (0..self.faces.len()).filter(|&i| self.faces[i].alive).collect()
    }
    /// BM_EDGES_OF_VERT
    pub fn edges_of_vert(&self, v: usize) -> Vec<usize> {
        let mut out = vec![];
        let first = self.verts[v].e;
        if first == NONE {
            return out;
        }
        let mut e = first;
        loop {
            out.push(e);
            e = self.disk_edge_next(e, v);
            if e == first {
                break;
            }
        }
        out
    }
    /// BM_LOOPS_OF_EDGE
    pub fn loops_of_edge(&self, e: usize) -> Vec<usize> {
        let mut out = vec![];
        let first = self.edges[e].l;
        if first == NONE {
            return out;
        }
        let mut l = first;
        loop {
            out.push(l);
            l = self.loops[l].rn;
            if l == first {
                break;
            }
        }
        out
    }
    pub fn faces_of_edge(&self, e: usize) -> Vec<usize> {
        self.loops_of_edge(e).into_iter().map(|l| self.loops[l].f).collect()
    }
    /// BM_LOOPS_OF_FACE
    pub fn loops_of_face(&self, f: usize) -> Vec<usize> {
        let mut out = vec![];
        let first = self.faces[f].l_first;
        let mut l = first;
        loop {
            out.push(l);
            l = self.loops[l].n;
            if l == first {
                break;
            }
        }
        out
    }
    pub fn verts_of_face(&self, f: usize) -> Vec<usize> {
        self.loops_of_face(f).into_iter().map(|l| self.loops[l].v).collect()
    }
    pub fn edges_of_face(&self, f: usize) -> Vec<usize> {
        self.loops_of_face(f).into_iter().map(|l| self.loops[l].e).collect()
    }
    /// BM_LOOPS_OF_VERT / BM_FACES_OF_VERT order (disk edges, radial loops whose vert is v)
    pub fn loops_of_vert(&self, v: usize) -> Vec<usize> {
        let mut out = vec![];
        // bmiter__loop_of_vert: count = facevert count; start from the first edge in the disk that has a loop
        let count = self.disk_facevert_count(v);
        if count == 0 {
            return out;
        }
        let mut l_first = self.disk_faceloop_find_first(self.verts[v].e, v);
        let mut e_next = self.loops[l_first].e;
        let mut l_next = l_first;
        let mut cnt = count;
        loop {
            out.push(l_next);
            cnt -= 1;
            if cnt == 0 {
                break;
            }
            l_next = self.radial_faceloop_find_next(l_next, v);
            if l_next == l_first {
                e_next = self.disk_faceedge_find_next(e_next, v);
                l_first = self.radial_faceloop_find_first(self.edges[e_next].l, v);
                l_next = l_first;
            }
        }
        out
    }
    pub fn faces_of_vert(&self, v: usize) -> Vec<usize> {
        self.loops_of_vert(v).into_iter().map(|l| self.loops[l].f).collect()
    }
    fn radial_facevert_count(&self, l: usize, v: usize) -> usize {
        let mut c = 0;
        let mut it = l;
        loop {
            if self.loops[it].v == v {
                c += 1;
            }
            it = self.loops[it].rn;
            if it == l {
                break;
            }
        }
        c
    }
    fn disk_facevert_count(&self, v: usize) -> usize {
        let mut count = 0;
        for e in self.edges_of_vert(v) {
            if self.edges[e].l != NONE {
                count += self.radial_facevert_count(self.edges[e].l, v);
            }
        }
        count
    }
    fn disk_faceloop_find_first(&self, e: usize, v: usize) -> usize {
        let mut it = e;
        loop {
            let l = self.edges[it].l;
            if l != NONE {
                return if self.loops[l].v == v { l } else { self.loops[l].n };
            }
            it = self.disk_edge_next(it, v);
            if it == e {
                return NONE;
            }
        }
    }
    fn radial_faceloop_find_first(&self, l: usize, v: usize) -> usize {
        let mut it = l;
        loop {
            if self.loops[it].v == v {
                return it;
            }
            it = self.loops[it].rn;
            if it == l {
                return NONE;
            }
        }
    }
    fn radial_faceloop_find_next(&self, l: usize, v: usize) -> usize {
        let mut it = self.loops[l].rn;
        loop {
            if self.loops[it].v == v {
                return it;
            }
            it = self.loops[it].rn;
            if it == l {
                return l;
            }
        }
    }
    fn radial_facevert_check(&self, l: usize, v: usize) -> bool {
        let mut it = l;
        loop {
            if self.loops[it].v == v {
                return true;
            }
            it = self.loops[it].rn;
            if it == l {
                return false;
            }
        }
    }
    fn disk_faceedge_find_next(&self, e: usize, v: usize) -> usize {
        let mut find = self.disk_edge_next(e, v);
        loop {
            if self.edges[find].l != NONE && self.radial_facevert_check(self.edges[find].l, v) {
                return find;
            }
            find = self.disk_edge_next(find, v);
            if find == e {
                return e;
            }
        }
    }

    // ------------------------------------------------------------------ queries
    pub fn edge_other_vert(&self, e: usize, v: usize) -> usize {
        let ed = &self.edges[e];
        if v == ed.v1 {
            ed.v2
        } else if v == ed.v2 {
            ed.v1
        } else {
            NONE
        }
    }
    pub fn vert_in_edge(&self, e: usize, v: usize) -> bool {
        self.edges[e].v1 == v || self.edges[e].v2 == v
    }
    pub fn edge_exists(&self, a: usize, b: usize) -> usize {
        let (ea, eb) = (self.verts[a].e, self.verts[b].e);
        if ea == NONE || eb == NONE {
            return NONE;
        }
        let (mut ia, mut ib) = (ea, eb);
        loop {
            if self.vert_in_edge(ia, b) {
                return ia;
            }
            if self.vert_in_edge(ib, a) {
                return ib;
            }
            ia = self.disk_edge_next(ia, a);
            ib = self.disk_edge_next(ib, b);
            if ia == ea || ib == eb {
                return NONE;
            }
        }
    }
    pub fn face_exists(&self, varr: &[usize]) -> usize {
        let len = varr.len();
        if self.verts[varr[0]].e == NONE {
            return NONE;
        }
        let e_first = self.verts[varr[0]].e;
        let mut e_iter = e_first;
        loop {
            let lf = self.edges[e_iter].l;
            if lf != NONE {
                let mut lr = lf;
                loop {
                    if self.loops[lr].v == varr[0] && self.faces[self.loops[lr].f].len == len {
                        let mut i_walk = 2;
                        if self.loops[self.loops[lr].n].v == varr[1] {
                            let mut lw = self.loops[self.loops[lr].n].n;
                            while i_walk != len {
                                if self.loops[lw].v != varr[i_walk] {
                                    break;
                                }
                                lw = self.loops[lw].n;
                                i_walk += 1;
                            }
                        } else if self.loops[self.loops[lr].p].v == varr[1] {
                            let mut lw = self.loops[self.loops[lr].p].p;
                            while i_walk != len {
                                if self.loops[lw].v != varr[i_walk] {
                                    break;
                                }
                                lw = self.loops[lw].p;
                                i_walk += 1;
                            }
                        }
                        if i_walk == len {
                            return self.loops[lr].f;
                        }
                    }
                    lr = self.loops[lr].rn;
                    if lr == lf {
                        break;
                    }
                }
            }
            e_iter = self.disk_edge_next(e_iter, varr[0]);
            if e_iter == e_first {
                break;
            }
        }
        NONE
    }
    pub fn edge_is_manifold(&self, e: usize) -> bool {
        let l = self.edges[e].l;
        l != NONE && self.loops[l].rn != l && self.loops[self.loops[l].rn].rn == l
    }
    pub fn edge_is_wire(&self, e: usize) -> bool {
        self.edges[e].l == NONE
    }
    pub fn edge_face_count(&self, e: usize) -> usize {
        self.loops_of_edge(e).len()
    }
    pub fn edge_loop_pair(&self, e: usize) -> Option<(usize, usize)> {
        let la = self.edges[e].l;
        if la == NONE {
            return None;
        }
        let lb = self.loops[la].rn;
        if la != lb && self.loops[lb].rn == la {
            Some((la, lb))
        } else {
            None
        }
    }
    pub fn edge_calc_length(&self, e: usize) -> f32 {
        let (a, b) = (self.verts[self.edges[e].v1].co, self.verts[self.edges[e].v2].co);
        crate::bm::query::len_v3v3(a, b)
    }
    pub fn face_vert_share_loop(&self, f: usize, v: usize) -> usize {
        let first = self.faces[f].l_first;
        let mut l = first;
        loop {
            if self.loops[l].v == v {
                return l;
            }
            l = self.loops[l].n;
            if l == first {
                return NONE;
            }
        }
    }
    pub fn face_edge_share_loop(&self, f: usize, e: usize) -> usize {
        let first = self.edges[e].l;
        if first == NONE {
            return NONE;
        }
        let mut l = first;
        loop {
            if self.loops[l].f == f {
                return l;
            }
            l = self.loops[l].rn;
            if l == first {
                return NONE;
            }
        }
    }
    pub fn vert_face_check(&self, v: usize) -> bool {
        self.edges_of_vert(v).iter().any(|&e| self.edges[e].l != NONE)
    }
    /// BM_edge_other_loop
    pub fn edge_other_loop(&self, e: usize, l: usize) -> usize {
        let mut l_other = if self.loops[l].e == e { l } else { self.loops[l].p };
        l_other = self.loops[l_other].rn;
        if self.loops[l_other].v == self.loops[l].v {
        } else if self.loops[self.loops[l_other].n].v == self.loops[l].v {
            l_other = self.loops[l_other].n;
        }
        l_other
    }
    /// BM_vert_step_fan_loop
    pub fn vert_step_fan_loop(&self, l: usize, e_step: &mut usize) -> usize {
        let e_prev = *e_step;
        let e_next;
        if self.loops[l].e == e_prev {
            e_next = self.loops[self.loops[l].p].e;
        } else if self.loops[self.loops[l].p].e == e_prev {
            e_next = self.loops[l].e;
        } else {
            return NONE;
        }
        if self.edge_is_manifold(e_next) {
            *e_step = e_next;
            return self.edge_other_loop(e_next, l);
        }
        NONE
    }
    pub fn face_calc_normal(&self, f: usize) -> V3 {
        query::face_normal(self, f)
    }
    pub fn face_calc_center_bounds(&self, f: usize) -> V3 {
        let vs = self.verts_of_face(f);
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in vs {
            for k in 0..3 {
                lo[k] = lo[k].min(self.verts[v].co[k]);
                hi[k] = hi[k].max(self.verts[v].co[k]);
            }
        }
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]
    }

    /// recompute all face normals
    pub fn normals_update(&mut self) {
        for f in self.face_ids() {
            self.faces[f].no = self.face_calc_normal(f);
        }
    }

    // ------------------------------------------------------------------ conversion
    /// BM_mesh_bm_from_me: verts, edges in array order, faces from corner lists (edges looked up by corner edge ids)
    pub fn from_mesh(pos: &[V3], edges: &[[u32; 2]], faces: &[Vec<u32>], corner_edges: &[Vec<u32>]) -> BMesh {
        let mut bm = BMesh::new();
        for p in pos {
            bm.vert_create(*p, None);
        }
        for e in edges {
            bm.edge_create(e[0] as usize, e[1] as usize, None, false);
        }
        for (fi, f) in faces.iter().enumerate() {
            let vs: Vec<usize> = f.iter().map(|&v| v as usize).collect();
            let es: Vec<usize> = corner_edges[fi].iter().map(|&e| e as usize).collect();
            let id = bm.face_create(&vs, &es, None, false);
            // faces created from a mesh are smooth-less flat; keep defaults
            let _ = id;
        }
        bm.normals_update();
        bm
    }
    /// BM_mesh_bm_to_me: (positions, edges (v1, v2 as stored), faces (vertex index lists))
    pub fn to_mesh(&self) -> (Vec<V3>, Vec<[u32; 2]>, Vec<Vec<u32>>) {
        let vids = self.vert_ids();
        let mut vmap = vec![u32::MAX; self.verts.len()];
        for (i, &v) in vids.iter().enumerate() {
            vmap[v] = i as u32;
        }
        let pos = vids.iter().map(|&v| self.verts[v].co).collect();
        let edges = self.edge_ids().iter().map(|&e| [vmap[self.edges[e].v1], vmap[self.edges[e].v2]]).collect();
        let faces = self.face_ids().iter().map(|&f| self.verts_of_face(f).iter().map(|&v| vmap[v]).collect()).collect();
        (pos, edges, faces)
    }
}

