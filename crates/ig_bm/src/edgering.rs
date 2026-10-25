//! bmesh.ops.subdivide_edgering (bmo_subdivide_edgering.cc), interp_mode LINEAR, smooth 0, no profile shape (the bmesh.ops defaults,
//! as used by infinigen's `decorate.subdivide_edge_ring`). Positions of the new vertices come from BM_edge_split_n.
use super::*;
use std::collections::VecDeque;

const E_RING: u8 = 1;
const E_RIM: u8 = 2;
const E_IN_STACK: u8 = 4;
const F_OUT: u8 = 1;
const F_IN_STACK: u8 = 4;

#[derive(Default)]
struct Fl(Vec<u8>);
impl Fl {
    fn get(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }
    fn has(&self, i: usize, f: u8) -> bool {
        self.get(i) & f != 0
    }
    fn on(&mut self, i: usize, f: u8) {
        if i >= self.0.len() {
            self.0.resize(i + 1, 0);
        }
        self.0[i] |= f;
    }
    fn off(&mut self, i: usize, f: u8) {
        if i < self.0.len() {
            self.0[i] &= !f;
        }
    }
}

struct Loop {
    verts: VecDeque<usize>,
    closed: bool,
}

/// BM_mesh_edgeloops_find with the RIM test
fn edgeloops_find(bm: &BMesh, rim: &Fl) -> Vec<Loop> {
    let mut vtag = vec![false; bm.verts.len()];
    let mut etag = vec![false; bm.edges.len()];
    let mut edges = vec![];
    for e in bm.edge_ids() {
        if rim.has(e, E_RIM) {
            etag[e] = true;
            vtag[bm.edges[e].v1] = true;
            vtag[bm.edges[e].v2] = true;
            edges.push(e);
        }
    }
    let mut out = vec![];
    for &e in &edges {
        if !etag[e] {
            continue;
        }
        let mut st = Loop { verts: VecDeque::new(), closed: false };
        let ok1 = loop_build(bm, &mut st, &mut vtag, &mut etag, bm.edges[e].v1, bm.edges[e].v2, 1);
        let ok2 = ok1 && loop_build(bm, &mut st, &mut vtag, &mut etag, bm.edges[e].v2, bm.edges[e].v1, -1);
        if ok2 && st.verts.len() > 1 {
            out.push(st);
        }
    }
    out
}

fn loop_build(bm: &BMesh, st: &mut Loop, vtag: &mut Vec<bool>, etag: &mut Vec<bool>, v_prev: usize, v: usize, dir: i32) -> bool {
    let v_first = v;
    let (mut v_prev, mut v) = (v_prev, Some(v));
    if !vtag[v_first] {
        return true;
    }
    while let Some(cv) = v {
        if dir == 1 {
            st.verts.push_front(cv);
        } else {
            st.verts.push_back(cv);
        }
        vtag[cv] = false;
        // bm_vert_other_tag
        let mut e_next = NONE;
        let mut count = 0;
        for e in bm.edges_of_vert(cv) {
            if etag[e] {
                let vo = bm.edge_other_vert(e, cv);
                if vo != v_prev {
                    e_next = e;
                    count += 1;
                }
            }
        }
        let v_next;
        if count == 1 {
            let vn = bm.edge_other_vert(e_next, cv);
            etag[e_next] = false;
            if vn == v_first {
                st.closed = true;
                v_next = None;
            } else {
                v_next = Some(vn);
            }
        } else if count == 0 {
            v_next = None;
        } else {
            return false;
        }
        v_prev = cv;
        v = v_next;
    }
    true
}

fn check_overlap_all(bm: &BMesh, ring: &Fl, a: &Loop, b: &Loop) -> bool {
    let connect = |tag: &Vec<bool>, v: usize| -> bool {
        for e in bm.edges_of_vert(v) {
            if ring.has(e, E_RING) {
                let vo = bm.edge_other_vert(e, v);
                if tag[vo] {
                    return true;
                }
            }
        }
        false
    };
    let mut tag = vec![false; bm.verts.len()];
    for &v in &b.verts {
        tag[v] = true;
    }
    for &v in &a.verts {
        if !connect(&tag, v) {
            return false;
        }
    }
    let mut tag = vec![false; bm.verts.len()];
    for &v in &a.verts {
        tag[v] = true;
    }
    for &v in &b.verts {
        if !connect(&tag, v) {
            return false;
        }
    }
    true
}

fn rotate_first(lb: &mut VecDeque<usize>, v: usize) {
    let k = lb.iter().position(|&x| x == v).unwrap();
    lb.rotate_left(k);
}

fn pair_order_is_flipped(bm: &BMesh, a: &VecDeque<usize>, b: &VecDeque<usize>) -> bool {
    let (af, bf) = (a[0], b[0]);
    let ex = |x: usize, y: usize| bm.edge_exists(x, y) != NONE;
    let (a1, b1) = (a[1], b[1]);
    let mut ia = 0;
    loop {
        ia += 1;
        if !(ia < a.len() && (ex(a[ia], bf) || ex(a[ia], b1))) {
            break;
        }
    }
    let mut ib = 0;
    loop {
        ib += 1;
        if !(ib < b.len() && (ex(b[ib], af) || ex(b[ib], a1))) {
            break;
        }
    }
    let a_step = if ia < a.len() { a[ia - 1] } else { a[a.len() - 1] };
    let b_step = if ib < b.len() { b[ib - 1] } else { b[b.len() - 1] };
    !(ex(a_step, b_step) || ex(a1, b_step) || ex(b1, a_step))
}

fn face_slice(bm: &mut BMesh, fo: &mut Fl, l: usize, cuts: usize) {
    let mut l_new = l;
    for _ in 0..cuts {
        let f = bm.loops[l_new].f;
        let l_a = bm.loops[l_new].p;
        let l_b = bm.loops[bm.loops[l_new].n].n;
        // BM_loop_is_adjacent
        if l_a == l_b || bm.loops[l_b].n == l_a || bm.loops[l_b].p == l_a {
            break;
        }
        let (_f2, ln) = bm.face_split(f, l_a, l_b);
        l_new = ln;
        let rn = bm.loops[l_new].rn;
        if bm.faces[bm.loops[l_new].f].len < bm.faces[bm.loops[rn].f].len {
            l_new = rn;
        }
        fo.on(bm.loops[l_new].f, F_OUT);
        let rn = bm.loops[l_new].rn;
        fo.on(bm.loops[rn].f, F_OUT);
    }
}

/// BM_edge_split_n
fn edge_split_n(bm: &mut BMesh, e: usize, cuts: usize) {
    for i in 0..cuts {
        let percent = 1.0f32 / (cuts + 1 - i) as f32;
        let v2 = bm.edges[e].v2;
        bm.edge_split(e, v2, percent);
    }
}

fn ringsubd(bm: &mut BMesh, ef: &mut Fl, fo: &mut Fl, a: &mut Loop, b: &mut Loop, cuts: usize) {
    // bm_edgering_pair_order
    {
        let mut tag = vec![false; bm.verts.len()];
        for &v in &b.verts {
            tag[v] = true;
        }
        if a.closed && b.closed {
            let node = a.verts[0];
            let mut v_other = NONE;
            for e in bm.edges_of_vert(node) {
                if ef.has(e, E_RING) {
                    v_other = bm.edge_other_vert(e, node);
                    if tag.get(v_other).copied().unwrap_or(false) {
                        break;
                    }
                    v_other = NONE;
                }
            }
            if v_other != NONE {
                rotate_first(&mut b.verts, v_other);
                if pair_order_is_flipped(bm, &a.verts, &b.verts) {
                    b.verts.make_contiguous().reverse();
                    rotate_first(&mut b.verts, v_other);
                }
            }
        } else {
            let e = bm.edge_exists(a.verts[0], b.verts[0]);
            if e == NONE || !ef.has(e, E_RING) {
                b.verts.make_contiguous().reverse();
            }
        }
    }
    // bm_edgering_pair_subdiv
    let mut btag = vec![false; bm.verts.len()];
    for &v in &b.verts {
        btag[v] = true;
    }
    let mut edges_stack: Vec<usize> = vec![];
    let mut faces_stack: Vec<usize> = vec![];
    for &node in &a.verts {
        for e in bm.edges_of_vert(node) {
            if !ef.has(e, E_IN_STACK) {
                let vo = bm.edge_other_vert(e, node);
                if btag.get(vo).copied().unwrap_or(false) {
                    ef.on(e, E_IN_STACK);
                    edges_stack.push(e);
                    for f in bm.faces_of_edge(e) {
                        if fo.has(f, F_OUT) && !fo.has(f, F_IN_STACK) {
                            fo.on(f, F_IN_STACK);
                            faces_stack.push(f);
                        }
                    }
                }
            }
        }
    }
    while let Some(e) = edges_stack.pop() {
        ef.off(e, E_IN_STACK);
        ef.off(e, E_RING);
        edge_split_n(bm, e, cuts);
    }
    while let Some(f) = faces_stack.pop() {
        fo.off(f, F_IN_STACK);
        let first = bm.faces[f].l_first;
        let mut l = first;
        loop {
            let le = bm.loops[l].e;
            if ef.has(le, E_RIM) {
                face_slice(bm, fo, l, cuts);
                break;
            }
            l = bm.loops[l].n;
            if l == first {
                break;
            }
        }
    }
}

/// bmo_subdivide_edgering_exec; `edges` = the "edges" slot (BMesh edge ids). Returns false when the operator cancels (nothing changed).
pub fn subdivide_edgering(bm: &mut BMesh, edges: &[usize], cuts: usize) -> bool {
    let mut ef = Fl::default();
    let mut fo = Fl::default();
    for &e in edges {
        ef.on(e, E_RING);
    }
    for &e in edges {
        for f in bm.faces_of_edge(e) {
            if bm.faces[f].len <= 4 && !fo.has(f, F_OUT) {
                let ls = bm.loops_of_face(f);
                let ok = ls.iter().any(|&l| ef.has(bm.loops[l].e, E_RING) && bm.loops[l].e != e);
                if ok {
                    fo.on(f, F_OUT);
                    for &l in &ls {
                        let le = bm.loops[l].e;
                        if !ef.has(le, E_RING) {
                            ef.on(le, E_RIM);
                        }
                    }
                }
            }
        }
    }
    let mut loops = edgeloops_find(bm, &ef);
    let n = loops.len();
    if n < 2 {
        return false;
    }
    if n == 2 {
        let (l0, l1) = loops.split_at_mut(1);
        let (a, b) = (&mut l0[0], &mut l1[0]);
        if !check_overlap_all(bm, &ef, a, b) {
            return false;
        }
        ringsubd(bm, &mut ef, &mut fo, a, b, cuts);
        return true;
    }
    // bm_edgering_pair_calc: vert -> loop map, pairs in discovery order (the GSet iteration order is pointer dependent in Blender)
    let mut owner = vec![usize::MAX; bm.verts.len()];
    for (i, l) in loops.iter().enumerate() {
        for &v in &l.verts {
            owner[v] = i;
        }
    }
    let mut pairs: Vec<(usize, usize)> = vec![];
    for (i, l) in loops.iter().enumerate() {
        let v = l.verts[0];
        for e in bm.edges_of_vert(v) {
            if ef.has(e, E_RING) {
                let vo = bm.edge_other_vert(e, v);
                let j = owner[vo];
                if j != usize::MAX {
                    let p = (i.min(j), i.max(j));
                    if !pairs.contains(&p) {
                        pairs.push(p);
                    }
                }
            }
        }
    }
    if pairs.is_empty() {
        return false;
    }
    let ok: Vec<bool> = pairs.iter().map(|&(i, j)| check_overlap_all(bm, &ef, &loops[i], &loops[j])).collect();
    let mut changed = false;
    for (k, &(i, j)) in pairs.iter().enumerate() {
        if !ok[k] {
            continue;
        }
        // distinct loops (i != j can fail only for a loop paired with itself)
        if i == j {
            continue;
        }
        let (lo, hi) = (i.min(j), i.max(j));
        let (l0, l1) = loops.split_at_mut(hi);
        let (a, b) = (&mut l0[lo], &mut l1[0]);
        ringsubd(bm, &mut ef, &mut fo, a, b, cuts);
        changed = true;
    }
    changed
}
