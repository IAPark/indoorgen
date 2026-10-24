//! Topology of Blender's `bpy.ops.mesh.bridge_edge_loops` (bmo_bridge.cc `bridge_loop_pair`, bmesh_edgeloop.cc, bmo_subdivide_edgering.cc
//! edge-ring splitting) on the BMesh port, so vertex and face ORDER (slot reuse, disk cycles) come out as Blender's.
//! Positions of the cut vertices are not computed here: the caller assigns them per ring (see `assets::bridge::bridge_bm`).
//! Supported: two closed or open loops (type SINGLE, no merge, no twist), PATH / LINEAR / SURFACE interpolation (positions are the caller's).
use super::query::*;
use super::*;
use crate::dhash::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct ELoop {
    pub v: Vec<usize>,
    pub closed: bool,
    pub co: V3,
    pub no: V3,
}

impl ELoop {
    fn flip(&mut self) {
        self.no = neg(self.no);
        self.v.reverse();
    }
    fn rotate_first(&mut self, vert: usize) {
        let k = self.v.iter().position(|&x| x == vert).unwrap();
        self.v.rotate_left(k);
    }
    fn calc_center(&mut self, bm: &BMesh) {
        let n = self.v.len();
        let p = |k: usize| bm.verts[self.v[k % n]].co;
        let mut co = [0.0f32; 3];
        let mut totw = 0.0f32;
        let mut w_prev = len_v3v3(p(n - 2 + n), p(n - 1));
        for step in 0..n {
            let k = (n - 1 + step) % n;
            let (cur, next) = (p(k), p(k + 1));
            let w_curr = len_v3v3(cur, next);
            let w = w_curr + w_prev;
            for i in 0..3 {
                co[i] += cur[i] * w;
            }
            totw += w;
            w_prev = w_curr;
        }
        if totw != 0.0 {
            let f = 1.0f32 / totw;
            for i in 0..3 {
                co[i] *= f;
            }
        }
        self.co = co;
    }
    fn calc_normal(&mut self, bm: &BMesh) -> bool {
        let n = self.v.len();
        let mut no = [0.0f32; 3];
        let mut prev = bm.verts[self.v[n - 1]].co;
        for k in 0..n {
            let cur = bm.verts[self.v[k]].co;
            no[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
            no[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
            no[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
            prev = cur;
        }
        let (nn, l) = normalize_len(no);
        if l < 1e-10 {
            self.no = [0.0, 0.0, 1.0];
            return false;
        }
        self.no = nn;
        true
    }
    fn calc_normal_aligned(&mut self, bm: &BMesh, no_align: V3) -> bool {
        let n = self.v.len();
        let mut no = [0.0f32; 3];
        let mut prev = bm.verts[self.v[n - 1]].co;
        for k in 0..n {
            let cur = bm.verts[self.v[k]].co;
            let dir = sub(cur, prev);
            let cr = cross(no_align, dir);
            let nn = cross(dir, cr);
            no = add(no, nn);
            prev = cur;
        }
        let (nn, l) = normalize_len(no);
        if l < 1e-10 {
            self.no = [0.0, 0.0, 1.0];
            return false;
        }
        self.no = nn;
        true
    }
}

/// BM_mesh_edgeloops_find
pub fn edgeloops_find(bm: &BMesh, test: &dyn Fn(usize) -> bool) -> Vec<ELoop> {
    let mut etag: HashSet<usize> = HashSet::default();
    let mut vtag: HashSet<usize> = HashSet::default();
    let mut edges = vec![];
    for e in bm.edge_ids() {
        if test(e) {
            etag.insert(e);
            vtag.insert(bm.edges[e].v1);
            vtag.insert(bm.edges[e].v2);
            edges.push(e);
        }
    }
    let mut out = vec![];
    // bm_loop_build
    fn build(bm: &BMesh, etag: &mut HashSet<usize>, vtag: &mut HashSet<usize>, el: &mut std::collections::VecDeque<usize>, closed: &mut bool, v_prev: usize, v: usize, dir: i32) -> bool {
        if !vtag.contains(&v) {
            return true;
        }
        let v_first = v;
        let (mut v_prev, mut v) = (v_prev, Some(v));
        while let Some(cv) = v {
            if dir == 1 {
                el.push_front(cv);
            } else {
                el.push_back(cv);
            }
            vtag.remove(&cv);
            let mut count = 0;
            let mut e_next = NONE;
            for e in bm.edges_of_vert(cv) {
                if etag.contains(&e) {
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
                etag.remove(&e_next);
                if vn == v_first {
                    *closed = true;
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
    for &e in &edges {
        if etag.contains(&e) {
            let mut el = std::collections::VecDeque::new();
            let mut closed = false;
            let (v1, v2) = (bm.edges[e].v1, bm.edges[e].v2);
            if build(bm, &mut etag, &mut vtag, &mut el, &mut closed, v1, v2, 1) && build(bm, &mut etag, &mut vtag, &mut el, &mut closed, v2, v1, -1) && el.len() > 1 {
                out.push(ELoop { v: el.into_iter().collect(), closed, co: [0.0; 3], no: [0.0; 3] });
            }
        }
    }
    out
}

/// BM_edgeloop_expand (split = false), `v` entries are duplicated
fn expand(v: &mut Vec<usize>, target: usize) {
    while v.len() * 2 < target {
        let mut out = Vec::with_capacity(v.len() * 2);
        for &x in v.iter() {
            out.push(x);
            out.push(x);
        }
        *v = out;
    }
    if v.len() < target {
        let src = v.len() as i64;
        let dst = (target - v.len()) as i64;
        let (src2, dst2) = (src * 2, dst * 2);
        let mut error = dst2 - src;
        let mut i = 0i64;
        let mut cur = 0usize;
        let mut iter_prev = 0i64;
        loop {
            let delta = error.div_euclid(dst2);
            i -= delta;
            if i >= src {
                break;
            }
            while iter_prev < i {
                cur += 1;
                iter_prev += 1;
            }
            let x = v[cur];
            v.insert(cur + 1, x);
            cur += 2;
            iter_prev += 1;
            error -= delta * dst2 + src2;
        }
    }
}

fn is_boundary_edge(bm: &BMesh, e: usize) -> bool {
    let l = bm.edges[e].l;
    l != NONE && l == bm.loops[l].rn
}

/// result of the topology pass
pub struct BridgeTopo {
    /// first / second rim loop of the subdivision step (`a` ring vertices come first in every ring)
    pub rim_a: Vec<usize>,
    pub rim_b: Vec<usize>,
    /// one entry per cross edge in creation order: [a, cut1 .. cutN, b]
    pub rings: Vec<Vec<usize>>,
}

/// `loops`: found loops (first, second) of the selected edges. Creates the bridge faces (+ triangulate / beautify for unequal loops) and
/// returns the set of new faces' edges. `edge_out` = edges of the new faces minus the loop edges.
fn bridge_loop_pair(bm: &mut BMesh, mut a: ELoop, mut b: ELoop, orig_faces: &HashSet<usize>) -> Result<(Vec<usize>, ELoop, ELoop), String> {
    let eps = 0.00001f32;
    let closed = a.closed && b.closed;
    let (mut a_len, mut b_len) = (a.v.len(), b.v.len());
    if a_len < b_len {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut a_len, &mut b_len);
    }
    let el_dir = sub(a.co, b.co);
    if closed {
        a.calc_normal(bm);
        b.calc_normal(bm);
    } else {
        let first_last = |l: &ELoop| sub(bm.verts[l.v[0]].co, bm.verts[*l.v.last().unwrap()].co);
        let (dir_a_orig, dir_b_orig) = (first_last(&a), first_last(&b));
        let no = cross(dir_a_orig, el_dir);
        let dir_a = cross(no, el_dir);
        let no = cross(dir_b_orig, el_dir);
        let dir_b = cross(no, el_dir);
        let (ta, tb) = if !is_zero(dir_a) && !is_zero(dir_b) { (dir_a, dir_b) } else { (dir_a_orig, dir_b_orig) };
        if dot(ta, tb) < 0.0 {
            b.flip();
        }
        let no = normalize(el_dir);
        a.calc_normal_aligned(bm, no);
        b.calc_normal_aligned(bm, no);
    }
    let dot_a = dot(a.no, el_dir);
    let dot_b = dot(b.no, el_dir);
    if len_sq(el_dir) < eps || (dot_a.abs() < eps && dot_b.abs() < eps) {
        if dot(a.no, b.no) < 0.0 {
            b.flip();
        }
    } else if (dot_a < 0.0) != (dot_b < 0.0) {
        b.flip();
    }
    {
        let no = add(a.no, b.no);
        if dot(no, el_dir) < 0.0 {
            a.flip();
            b.flip();
        }
    }
    if bm.totface > 0 {
        let mut votes = [0i32; 2];
        let mut wdir = 1;
        for (i, st) in [&a, &b].iter().enumerate() {
            let n = st.v.len();
            for k in 0..n {
                let nxt = if k + 1 < n { Some(st.v[k + 1]) } else if st.closed { Some(st.v[0]) } else { None };
                if let Some(nv) = nxt {
                    let e = bm.edge_exists(st.v[k], nv);
                    if e != NONE && is_boundary_edge(bm, e) {
                        let l = bm.edges[e].l;
                        votes[i] += if bm.loops[l].v == st.v[k] { wdir } else { -wdir };
                    }
                }
            }
            wdir = -wdir;
        }
        if votes[0] != 0 || votes[1] != 0 {
            let mut fl = [false, false];
            if dot_a.abs() < eps && votes[0] < 0 {
                fl[0] = !fl[0];
                votes[0] = -votes[0];
            }
            if dot_b.abs() < eps && votes[1] < 0 {
                fl[1] = !fl[1];
                votes[1] = -votes[1];
            }
            if votes[0] + votes[1] < 0 {
                fl[0] = !fl[0];
                fl[1] = !fl[1];
            }
            if fl[0] {
                a.flip();
            }
            if fl[1] {
                b.flip();
            }
        }
    }
    let mut bx = b.clone();
    if a_len > b_len {
        expand(&mut bx.v, a_len);
    }
    if closed {
        // bm_bridge_best_rotation
        let n = bx.v.len();
        let mut len_best = f32::MAX;
        let mut best = None;
        for k in 0..n {
            let mut len = 0.0f32;
            for i in 0..n {
                len += len_v3v3(bm.verts[a.v[i]].co, bm.verts[bx.v[(k + i) % n]].co);
                if !(len < len_best) {
                    break;
                }
            }
            if len < len_best {
                len_best = len;
                best = Some(k);
            }
        }
        if let Some(k) = best {
            bx.v.rotate_left(k);
        }
    }
    // faces
    let n = a.v.len();
    let mut i = 0usize;
    loop {
        let next;
        if closed {
            next = (i + 1) % n;
        } else {
            if i + 1 >= n {
                break;
            }
            next = i + 1;
        }
        let (v_a, v_b, v_an, v_bn) = (a.v[i], bx.v[i], a.v[next], bx.v[next]);
        if v_b != v_bn {
            if !(v_b == v_an || v_b == v_a || v_bn == v_an || v_bn == v_a) {
                let arr = [v_b, v_bn, v_an, v_a];
                if bm.face_exists(&arr) == NONE {
                    bm.face_create_verts(&arr, None, true);
                }
            }
        } else if !(v_b == v_an || v_b == v_a) {
            let arr = [v_b, v_an, v_a];
            if bm.face_exists(&arr) == NONE {
                bm.face_create_verts(&arr, None, true);
            }
        }
        if next == 0 {
            break;
        }
        i = next;
    }
    // bridge faces so far = faces that are not original
    let is_new = |bm: &BMesh, f: usize| !orig_faces.contains(&f) && bm.faces[f].alive;
    if a_len != b_len {
        // triangulate the quads of the bridge (BM_mesh_triangulate quad_method BEAUTY, tag_only)
        let snapshot: Vec<usize> = bm.face_ids();
        for f in snapshot {
            if bm.faces[f].len >= 4 && is_new(bm, f) {
                if bm.faces[f].len != 4 {
                    return Err("bridge: n-gon in bridge".into());
                }
                triangulate_quad(bm, f);
            }
        }
        // restrict rotation to edges whose new diagonal joins the two sides
        for v in bm.vert_ids() {
            bm.verts[v].flag &= !ELEM_TAG;
        }
        for &v in &b.v {
            bm.verts[v].flag |= ELEM_TAG;
        }
        for v in bm.vert_ids() {
            let slot = v as i32;
            bm.verts[v].idx = slot;
        }
        let cand: Vec<usize> = bm
            .edge_ids()
            .into_iter()
            .filter(|&e| {
                bm.edge_rotate_check(e) && {
                    let (fa, fb) = bm.edge_face_pair(e).unwrap();
                    is_new(bm, fa) && is_new(bm, fb) && bm.faces[fa].len == 3 && bm.faces[fb].len == 3
                }
            })
            .collect();
        bm.beautify_fill_ex(&cand, true, true);
        for v in bm.vert_ids() {
            bm.verts[v].flag &= !ELEM_TAG;
        }
    }
    // edge_out = edges of the new faces minus the loop edges
    let mut eo: HashSet<usize> = HashSet::default();
    for f in bm.face_ids() {
        if is_new(bm, f) {
            for e in bm.edges_of_face(f) {
                eo.insert(e);
            }
        }
    }
    for st in [&a, &bx] {
        let n = st.v.len();
        for k in 0..n {
            let nxt = if k + 1 < n { Some(st.v[k + 1]) } else if st.closed { Some(st.v[0]) } else { None };
            if let Some(nv) = nxt {
                if nv != st.v[k] {
                    let e = bm.edge_exists(st.v[k], nv);
                    if e != NONE {
                        eo.remove(&e);
                    }
                }
            }
        }
    }
    let mut out: Vec<usize> = bm.edge_ids().into_iter().filter(|e| eo.contains(e)).collect();
    out.dedup();
    Ok((out, a, b))
}

/// BM_face_triangulate for a quad (quad_method BEAUTY), including the swap of the last triangle into the original face
fn triangulate_quad(bm: &mut BMesh, f: usize) {
    use crate::assets::polyfill::quad_split_24_beauty;
    let l_first = bm.faces[f].l_first;
    let l1 = bm.loops[l_first].n; // l_v1
    let l2 = bm.loops[l1].n; // l_v2
    let l3 = bm.loops[l_first].p; // l_v3
    let l4 = l_first; // l_v4
    let co = |l: usize| bm.verts[bm.loops[l].v].co;
    let quad = [co(l_first), co(l1), co(l2), co(l3)];
    let split_24 = quad_split_24_beauty(quad);
    let (lv1, lv2) = if split_24 { (l4, l2) } else { (l1, l3) };
    let loops = [lv1, bm.loops[lv1].n, lv2, bm.loops[lv2].n];
    let tris = [[0usize, 1, 2], [0, 2, 3]];
    let mut f_new = NONE;
    for (i, t) in tris.iter().enumerate() {
        let vt = [bm.loops[loops[t[0]]].v, bm.loops[loops[t[1]]].v, bm.loops[loops[t[2]]].v];
        f_new = bm.face_create_verts(&vt, Some(f), true);
        let _ = i;
    }
    bm.face_swap_data(f, f_new);
    bm.face_kill(f_new);
}

fn pair_order_is_flipped(bm: &BMesh, a: &[usize], b: &[usize]) -> bool {
    let ex = |x: usize, y: usize| bm.edge_exists(x, y) != NONE;
    let (a0, b0) = (a[0], b[0]);
    let mut ai = 0usize;
    loop {
        ai += 1;
        if !(ai < a.len() && (ex(a[ai], b0) || ex(a[ai], b[1]))) {
            break;
        }
    }
    let mut bi = 0usize;
    loop {
        bi += 1;
        if !(bi < b.len() && (ex(b[bi], a0) || ex(b[bi], a[1]))) {
            break;
        }
    }
    let a_step = if ai < a.len() { a[ai - 1] } else { *a.last().unwrap() };
    let b_step = if bi < b.len() { b[bi - 1] } else { *b.last().unwrap() };
    !(ex(a_step, b_step) || ex(a[1], b_step) || ex(b[1], a_step))
}

/// bmo_subdivide_edgering for the two-ring case with `cuts` cuts. `ring_edges`: the bridge's rung edges (edges.out).
fn subdivide_edgering(bm: &mut BMesh, ring_edges: &[usize], cuts: usize) -> Result<BridgeTopo, String> {
    let ring: HashSet<usize> = ring_edges.iter().copied().collect();
    let mut face_out: HashSet<usize> = HashSet::default();
    let mut rim: HashSet<usize> = HashSet::default();
    for &e in ring_edges {
        for f in bm.faces_of_edge(e) {
            if bm.faces[f].len <= 4 && !face_out.contains(&f) {
                let ok = bm.loops_of_face(f).iter().any(|&l| ring.contains(&bm.loops[l].e) && bm.loops[l].e != e);
                if ok {
                    face_out.insert(f);
                    for l in bm.loops_of_face(f) {
                        let le = bm.loops[l].e;
                        if !ring.contains(&le) {
                            rim.insert(le);
                        }
                    }
                }
            }
        }
    }
    let loops = edgeloops_find(bm, &|e| rim.contains(&e));
    if loops.len() != 2 {
        return Err(format!("subdivide_edgering: {} rim loops", loops.len()));
    }
    let (mut la, mut lb) = (loops[0].clone(), loops[1].clone());
    let btag: HashSet<usize> = lb.v.iter().copied().collect();
    // bm_edgering_pair_order
    if !(la.closed && lb.closed) {
        let e = bm.edge_exists(la.v[0], lb.v[0]);
        if e == NONE || !ring.contains(&e) {
            lb.v.reverse();
        }
    } else {
        let node = la.v[0];
        let mut v_other = NONE;
        for e in bm.edges_of_vert(node) {
            if ring.contains(&e) {
                let vo = bm.edge_other_vert(e, node);
                if btag.contains(&vo) {
                    v_other = vo;
                    break;
                }
            }
        }
        if v_other == NONE {
            return Err("subdivide_edgering: no shared edge".into());
        }
        lb.rotate_first(v_other);
        if pair_order_is_flipped(bm, &la.v, &lb.v) {
            lb.v.reverse();
            lb.rotate_first(v_other);
        }
    }
    // bm_edgering_pair_subdiv
    let mut in_stack_e: HashSet<usize> = HashSet::default();
    let mut in_stack_f: HashSet<usize> = HashSet::default();
    let mut edges_stack: Vec<usize> = vec![];
    let mut faces_stack: Vec<usize> = vec![];
    for &node in &la.v {
        for e in bm.edges_of_vert(node) {
            if !in_stack_e.contains(&e) {
                let vo = bm.edge_other_vert(e, node);
                if btag.contains(&vo) {
                    in_stack_e.insert(e);
                    edges_stack.push(e);
                    for f in bm.faces_of_edge(e) {
                        if face_out.contains(&f) && !in_stack_f.contains(&f) {
                            in_stack_f.insert(f);
                            faces_stack.push(f);
                        }
                    }
                }
            }
        }
    }
    let mut rings: Vec<Vec<usize>> = vec![];
    while let Some(e) = edges_stack.pop() {
        let v_a = if btag.contains(&bm.edges[e].v1) { bm.edges[e].v1 } else { bm.edges[e].v2 };
        let v_b = bm.edge_other_vert(e, v_a);
        // BM_edge_split_n
        let mut varr = vec![NONE; cuts + 2];
        for i in 0..cuts {
            let percent = 1.0f32 / (cuts + 1 - i) as f32;
            let v2 = bm.edges[e].v2;
            let (vn, _en) = bm.edge_split(e, v2, percent);
            varr[cuts - i] = vn; // r_varr[cuts - i - 1] sits at varr[1..]
        }
        let e1 = bm.edges[e].v1;
        let mut arr;
        if v_a == e1 {
            varr[0] = v_a;
            varr[cuts + 1] = v_b;
            arr = varr;
            arr.reverse(); // BM_edgeloop_flip
        } else {
            varr[0] = v_b;
            varr[cuts + 1] = v_a;
            arr = varr;
        }
        rings.push(arr);
    }
    // faces: pop, slice
    while let Some(f) = faces_stack.pop() {
        let lf = bm.loops_of_face(f);
        for &l in &lf {
            if rim.contains(&bm.loops[l].e) {
                // bm_face_slice
                let mut l_new = l;
                for _ in 0..cuts {
                    let lp = bm.loops[l_new].p;
                    let lnn = bm.loops[bm.loops[l_new].n].n;
                    let fcur = bm.loops[l_new].f;
                    if lp == lnn || bm.loops[lp].n == lnn || bm.loops[lnn].n == lp {
                        // adjacent loops: BM_face_split would fail (l_new = nullptr)
                        break;
                    }
                    let (_fnew, l_e) = bm.face_split(fcur, lp, lnn);
                    l_new = l_e;
                    let lr = bm.loops[l_new].rn;
                    if bm.faces[bm.loops[l_new].f].len < bm.faces[bm.loops[lr].f].len {
                        l_new = lr;
                    }
                }
                break;
            }
        }
    }
    Ok(BridgeTopo { rim_a: la.v, rim_b: lb.v, rings })
}

/// Full operator on selected edges `sel` (bridge_loops + optional subdivide_edgering). `assign(bm, ring)` sets the coordinates of the
/// inner vertices of every ring (`ring` = [a, cut1.., b]).
pub fn bridge_edge_loops(bm: &mut BMesh, sel: &dyn Fn(usize) -> bool, cuts: usize) -> Result<BridgeTopo, String> {
    let mut loops = edgeloops_find(bm, sel);
    if loops.len() != 2 {
        return Err(format!("bridge: {} loops found (two supported)", loops.len()));
    }
    for l in loops.iter_mut() {
        l.calc_center(bm);
    }
    let orig: HashSet<usize> = bm.face_ids().into_iter().collect();
    let (b, a_loop) = (loops.pop().unwrap(), loops.pop().unwrap());
    let (edge_out, _a, _b) = bridge_loop_pair(bm, a_loop, b, &orig)?;
    if cuts == 0 {
        return Ok(BridgeTopo { rim_a: vec![], rim_b: vec![], rings: vec![] });
    }
    subdivide_edgering(bm, &edge_out, cuts)
}

pub fn unused(_: &HashMap<usize, usize>) {}
