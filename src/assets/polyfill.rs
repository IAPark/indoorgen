//! Faithful f32 port of Blender 4.2 ngon triangulation (BM_face_triangulate -> BLI_polyfill_calc_arena + BLI_polyfill_beautify):
//! face normal (Newell), axis_dominant_v3_to_m3_negate projection, ear clipping (USE_CLIP_EVEN/USE_CLIP_SWEEP), beauty edge rotation.

type V3 = [f32; 3];

fn add_newell(n: &mut V3, vp: V3, vc: V3) {
    n[0] += (vp[1] - vc[1]) * (vp[2] + vc[2]);
    n[1] += (vp[2] - vc[2]) * (vp[0] + vc[0]);
    n[2] += (vp[0] - vc[0]) * (vp[1] + vc[1]);
}

fn normalize_v3(n: V3) -> V3 {
    let d = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    if d > 1.0e-35f32 {
        let d = d.sqrt();
        let f = 1.0f32 / d;
        [n[0] * f, n[1] * f, n[2] * f]
    } else {
        [0.0; 3]
    }
}

/// bm_face_calc_poly_normal (len > 4)
fn poly_normal(co: &[V3]) -> V3 {
    let mut n = [0f32; 3];
    let len = co.len();
    let mut vp = co[len - 1];
    for i in 0..len {
        let vc = co[i];
        add_newell(&mut n, vp, vc);
        vp = vc;
    }
    normalize_v3(n)
}

fn ortho_basis(n: V3) -> (V3, V3) {
    let eps = f32::EPSILON;
    let f = n[0] * n[0] + n[1] * n[1];
    if f > eps {
        let d = 1.0f32 / f.sqrt();
        let r1 = [n[1] * d, -n[0] * d, 0.0];
        let r2 = [-n[2] * r1[1], n[2] * r1[0], n[0] * r1[1] - n[1] * r1[0]];
        (r1, r2)
    } else {
        let r1 = [if n[2] < 0.0 { -1.0 } else { 1.0 }, 0.0, 0.0];
        (r1, [0.0, 1.0, 0.0])
    }
}

fn cross_tri_v2(v1: [f32; 2], v2: [f32; 2], v3: [f32; 2]) -> f32 {
    (v1[0] - v2[0]) * (v2[1] - v3[1]) + (v1[1] - v2[1]) * (v3[0] - v2[0])
}

fn len_v2v2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let (x, y) = (a[0] - b[0], a[1] - b[1]);
    (x * x + y * y).sqrt()
}

fn area_alt_2x(v1: [f32; 2], v2: [f32; 2], v3: [f32; 2]) -> f32 {
    let d2 = [v2[0] - v1[0], v2[1] - v1[1]];
    let d3 = [v3[0] - v1[0], v3[1] - v1[1]];
    (d2[0] * d3[1]) - (d3[0] * d2[1])
}

fn sign_of(a: f32) -> i8 {
    if a > 0.0 {
        1
    } else if a == 0.0 {
        0
    } else {
        -1
    }
}

fn span_tri_sign(v1: [f32; 2], v2: [f32; 2], v3: [f32; 2]) -> i8 {
    sign_of(area_alt_2x(v3, v2, v1))
}

struct Pi {
    next: usize,
    prev: usize,
    index: u32,
    sign: i8,
}

struct Pf<'a> {
    coords: &'a [[f32; 2]],
    pi: Vec<Pi>,
    head: usize,
    coords_num: usize,
    concave: usize,
    tris: Vec<[u32; 3]>,
}

impl<'a> Pf<'a> {
    fn sign_calc(&mut self, i: usize) {
        let (p, n) = (self.pi[i].prev, self.pi[i].next);
        let s = span_tri_sign(self.coords[self.pi[p].index as usize], self.coords[self.pi[i].index as usize], self.coords[self.pi[n].index as usize]);
        self.pi[i].sign = s;
    }
    fn ear_tip_check(&self, e: usize, sign_accept: i8) -> bool {
        if self.concave == 0 {
            return true;
        }
        if self.pi[e].sign != sign_accept {
            return false;
        }
        let c = self.coords;
        let (pe, ne) = (self.pi[e].prev, self.pi[e].next);
        let v1 = c[self.pi[pe].index as usize];
        let v2 = c[self.pi[e].index as usize];
        let v3 = c[self.pi[ne].index as usize];
        let mut checked = 0;
        let mut cur = self.pi[ne].next;
        while cur != pe {
            if self.pi[cur].sign != 1 {
                let v = c[self.pi[cur].index as usize];
                if span_tri_sign(v3, v1, v) != -1 && span_tri_sign(v1, v2, v) != -1 && span_tri_sign(v2, v3, v) != -1 {
                    return false;
                }
                checked += 1;
                if checked == self.concave {
                    break;
                }
            }
            cur = self.pi[cur].next;
        }
        true
    }
    fn ear_tip_find(&self, init: usize, reverse: bool) -> usize {
        let mut sa = 1i8;
        while sa >= 0 {
            let mut e = init;
            let mut i = self.coords_num;
            while i > 0 {
                i -= 1;
                if self.ear_tip_check(e, sa) {
                    return e;
                }
                e = if reverse { self.pi[e].prev } else { self.pi[e].next };
            }
            sa -= 1;
        }
        let mut e = init;
        let mut i = self.coords_num;
        while i > 0 {
            i -= 1;
            if self.pi[e].sign != -1 {
                return e;
            }
            e = self.pi[e].next;
        }
        e
    }
    fn coord_remove(&mut self, i: usize) {
        let (n, p) = (self.pi[i].next, self.pi[i].prev);
        self.pi[n].prev = p;
        self.pi[p].next = n;
        if self.head == i {
            self.head = n;
        }
        self.coords_num -= 1;
    }
    fn triangulate(&mut self) {
        let mut ear_init = self.head;
        let mut reverse = false;
        while self.coords_num > 3 {
            let ear = self.ear_tip_find(ear_init, reverse);
            if self.pi[ear].sign != 1 {
                self.concave -= 1;
            }
            let (pp, pn) = (self.pi[ear].prev, self.pi[ear].next);
            // cut
            let tri = [self.pi[pp].index, self.pi[ear].index, self.pi[pn].index];
            self.tris.push(tri);
            self.coord_remove(ear);
            let (so_p, so_n) = (self.pi[pp].sign, self.pi[pn].sign);
            if so_p != 1 {
                self.sign_calc(pp);
                if self.pi[pp].sign == 1 {
                    self.concave -= 1;
                }
            }
            if so_n != 1 {
                self.sign_calc(pn);
                if self.pi[pn].sign == 1 {
                    self.concave -= 1;
                }
            }
            ear_init = if reverse { self.pi[pp].prev } else { self.pi[pn].next };
            if self.pi[ear_init].sign != 1 {
                ear_init = if reverse { self.pi[ear_init].prev } else { self.pi[ear_init].next };
                reverse = !reverse;
            }
        }
        if self.coords_num == 3 {
            let a = self.head;
            let b = self.pi[a].next;
            let c = self.pi[b].next;
            self.tris.push([self.pi[a].index, self.pi[b].index, self.pi[c].index]);
        }
    }
}

/// BLI_polyfill_calc_arena with coords_sign = 1
pub fn polyfill_calc(coords: &[[f32; 2]]) -> Vec<[u32; 3]> {
    let n = coords.len();
    let mut pf = Pf { coords, pi: (0..n).map(|i| Pi { next: (i + 1) % n, prev: (i + n - 1) % n, index: i as u32, sign: 0 }).collect(), head: 0, coords_num: n, concave: 0, tris: vec![] };
    for i in 0..n {
        pf.sign_calc(i);
        if pf.pi[i].sign != 1 {
            pf.concave += 1;
        }
    }
    pf.triangulate();
    pf.tris
}

// ---- beautify -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct HalfEdge {
    v: u32,
    e_next: u32,
    e_radial: u32,
    base_index: u32,
}

const UMAX: u32 = u32::MAX;

fn quad_rotate_calc(v1: [f32; 2], v2: [f32; 2], v3: [f32; 2], v4: [f32; 2], lock_degenerate: bool) -> (f32, f32) {
    let eps_zero_area = 1e-12f32;
    let area_2x_234 = cross_tri_v2(v2, v3, v4);
    let area_2x_241 = cross_tri_v2(v2, v4, v1);
    let area_2x_123 = cross_tri_v2(v1, v2, v3);
    let area_2x_134 = cross_tri_v2(v1, v3, v4);
    let r_area = (area_2x_234.abs() + area_2x_241.abs() + area_2x_123.abs() + area_2x_134.abs()) / 8.0f32;
    'blk: {
        if (area_2x_123 >= 0.0) != (area_2x_134 >= 0.0) {
            break 'blk;
        }
        if area_2x_123.abs() <= eps_zero_area || area_2x_134.abs() <= eps_zero_area {
            break 'blk;
        }
        if (area_2x_234 >= 0.0) != (area_2x_241 >= 0.0) {
            if lock_degenerate {
                break 'blk;
            }
            return (-f32::MAX, r_area);
        }
        if area_2x_234.abs() <= eps_zero_area || area_2x_241.abs() <= eps_zero_area {
            return (-f32::MAX, r_area);
        }
        let len_12 = len_v2v2(v1, v2);
        let len_23 = len_v2v2(v2, v3);
        let len_34 = len_v2v2(v3, v4);
        let len_41 = len_v2v2(v4, v1);
        let len_13 = len_v2v2(v1, v3);
        let len_24 = len_v2v2(v2, v4);
        let (area_a, area_b) = (area_2x_234.abs(), area_2x_241.abs());
        let prim_a = len_23 + len_34 + len_24;
        let prim_b = len_41 + len_12 + len_24;
        let fac_24 = (area_a / prim_a) + (area_b / prim_b);
        let (area_a, area_b) = (area_2x_123.abs(), area_2x_134.abs());
        let prim_a = len_12 + len_23 + len_13;
        let prim_b = len_34 + len_41 + len_13;
        let fac_13 = (area_a / prim_a) + (area_b / prim_b);
        return (fac_24 - fac_13, r_area);
    }
    (f32::MAX, r_area)
}

struct HeapNode {
    value: f32,
    index: usize,
    ptr: u32, // half-edge index
}

struct Heap {
    tree: Vec<usize>, // node ids
    nodes: Vec<HeapNode>,
}

impl Heap {
    fn swap(&mut self, i: usize, j: usize) {
        let (pi, pj) = (self.tree[i], self.tree[j]);
        self.nodes[pi].index = j;
        self.tree[j] = pi;
        self.nodes[pj].index = i;
        self.tree[i] = pj;
    }
    fn cmp(&self, a: usize, b: usize) -> bool {
        self.nodes[self.tree[a]].value < self.nodes[self.tree[b]].value
    }
    fn down(&mut self, mut i: usize) {
        let size = self.tree.len();
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut smallest = i;
            if l < size && self.cmp(l, smallest) {
                smallest = l;
            }
            if r < size && self.cmp(r, smallest) {
                smallest = r;
            }
            if smallest == i {
                break;
            }
            self.swap(i, smallest);
            i = smallest;
        }
    }
    fn up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) >> 1;
            if self.cmp(p, i) {
                break;
            }
            self.swap(p, i);
            i = p;
        }
    }
    fn insert(&mut self, value: f32, ptr: u32) -> usize {
        let id = self.nodes.len();
        let idx = self.tree.len();
        self.nodes.push(HeapNode { value, index: idx, ptr });
        self.tree.push(id);
        self.up(idx);
        id
    }
    fn update(&mut self, id: usize, value: f32, ptr: u32) {
        self.nodes[id].ptr = ptr;
        if value < self.nodes[id].value {
            self.nodes[id].value = value;
            let i = self.nodes[id].index;
            self.up(i);
        } else if value > self.nodes[id].value {
            self.nodes[id].value = value;
            let i = self.nodes[id].index;
            self.down(i);
        }
    }
    fn pop_min(&mut self) -> u32 {
        let top = self.tree[0];
        let ptr = self.nodes[top].ptr;
        let size = self.tree.len() - 1;
        if size > 0 {
            self.swap(0, size);
            self.tree.pop();
            self.down(0);
        } else {
            self.tree.pop();
        }
        ptr
    }
    fn remove(&mut self, id: usize) {
        let mut i = self.nodes[id].index;
        while i > 0 {
            let p = (i - 1) >> 1;
            self.swap(p, i);
            i = p;
        }
        self.pop_min();
    }
}

struct Beauty<'a> {
    coords: &'a [[f32; 2]],
    edges: Vec<HalfEdge>,
}

impl<'a> Beauty<'a> {
    fn rotate_calc(&self, ea: usize) -> (f32, f32) {
        let e_a = self.edges[ea];
        let e_b = self.edges[e_a.e_radial as usize];
        let e_a_other = self.edges[self.edges[e_a.e_next as usize].e_next as usize];
        let e_b_other = self.edges[self.edges[e_b.e_next as usize].e_next as usize];
        let c = self.coords;
        let (v1, v2, v3, v4) = (c[e_a_other.v as usize], c[e_a.v as usize], c[e_b_other.v as usize], c[e_b.v as usize]);
        quad_rotate_calc(v1, v2, v3, v4, false)
    }
    fn rotate(&mut self, e: usize) {
        let e0 = e;
        let e1 = self.edges[e0].e_next as usize;
        let e2 = self.edges[e1].e_next as usize;
        let e3 = self.edges[e0].e_radial as usize;
        let e4 = self.edges[e3].e_next as usize;
        let e5 = self.edges[e4].e_next as usize;
        self.edges[e0].e_next = e2 as u32;
        self.edges[e1].e_next = e3 as u32;
        self.edges[e2].e_next = e4 as u32;
        self.edges[e3].e_next = e5 as u32;
        self.edges[e4].e_next = e0 as u32;
        self.edges[e5].e_next = e1 as u32;
        self.edges[e0].v = self.edges[e5].v;
        self.edges[e3].v = self.edges[e2].v;
    }
}

/// BLI_polyfill_beautify (in place on `tris`)
pub fn polyfill_beautify(coords: &[[f32; 2]], tris: &mut Vec<[u32; 3]>) {
    let coords_num = coords.len();
    let coord_last = (coords_num - 1) as u32;
    let tris_len = coords_num - 2;
    let edges_len = tris_len - 1;
    let half_len = 3 * tris_len;
    let mut edges = vec![HalfEdge { v: 0, e_next: 0, e_radial: UMAX, base_index: UMAX }; half_len];
    let mut order: Vec<(u32, u32, u32)> = vec![]; // verts0, verts1, e_half
    for i in 0..tris_len {
        let (mut jp) = 2usize;
        for jc in 0..3usize {
            let ep = i * 3 + jp;
            let ec = i * 3 + jc;
            edges[ep] = HalfEdge { v: tris[i][jp], e_next: ec as u32, e_radial: UMAX, base_index: UMAX };
            let (mut a, mut b) = (tris[i][jp], tris[i][jc]);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            let boundary = a + 1 == b || (a == 0 && b == coord_last);
            if !boundary {
                order.push((a, b, ep as u32));
            }
            jp = jc;
        }
    }
    assert_eq!(order.len(), edges_len * 2);
    order.sort();
    let mut base_index = 0u32;
    let mut i = 0;
    while i < order.len() {
        let (a, b) = (order[i], order[i + 1]);
        i += 2;
        edges[a.2 as usize].e_radial = b.2;
        edges[b.2 as usize].e_radial = a.2;
        edges[a.2 as usize].base_index = base_index;
        edges[b.2 as usize].base_index = base_index;
        base_index += 1;
    }
    let mut bt = Beauty { coords, edges };
    let mut heap = Heap { tree: vec![], nodes: vec![] };
    let mut table: Vec<Option<usize>> = vec![None; edges_len];
    for i in 0..half_len {
        let e = bt.edges[i];
        if e.e_radial < i as u32 {
            let (cost, _) = bt.rotate_calc(i);
            if cost < 0.0 {
                table[e.base_index as usize] = Some(heap.insert(cost, i as u32));
            } else {
                table[e.base_index as usize] = None;
            }
        }
    }
    while !heap.tree.is_empty() {
        let e = heap.pop_min() as usize;
        let bi = bt.edges[e].base_index as usize;
        table[bi] = None;
        bt.rotate(e);
        // cost update
        let e0 = bt.edges[e].e_next as usize;
        let e1 = bt.edges[e0].e_next as usize;
        let er = bt.edges[e].e_radial as usize;
        let e2 = bt.edges[er].e_next as usize;
        let e3 = bt.edges[e2].e_next as usize;
        for ei in [e0, e1, e2, e3] {
            if bt.edges[ei].base_index != UMAX {
                let i = bt.edges[ei].base_index as usize;
                let (cost, area) = bt.rotate_calc(ei);
                if cost < -1e-6f32 * area.max(1.0f32) {
                    match table[i] {
                        None => table[i] = Some(heap.insert(cost, ei as u32)),
                        Some(id) => heap.update(id, cost, ei as u32),
                    }
                } else if let Some(id) = table[i] {
                    heap.remove(id);
                    table[i] = None;
                }
            }
        }
    }
    // collect triangles
    let mut out = vec![];
    for i in 0..half_len {
        if bt.edges[i].v != UMAX {
            let mut e = i;
            let mut tri = [0u32; 3];
            tri[0] = bt.edges[e].v;
            bt.edges[e].v = UMAX;
            e = bt.edges[e].e_next as usize;
            tri[1] = bt.edges[e].v;
            bt.edges[e].v = UMAX;
            e = bt.edges[e].e_next as usize;
            tri[2] = bt.edges[e].v;
            bt.edges[e].v = UMAX;
            out.push(tri);
        }
    }
    *tris = out;
}

/// BM_face_triangulate for ngons (len > 4) with ngon_method = BEAUTY: returns triangles as indices into the face's loops.
pub fn triangulate_ngon(co: &[V3]) -> Vec<[u32; 3]> {
    let n = poly_normal(co);
    // axis_dominant_v3_to_m3_negate
    let r2 = [-n[0], -n[1], -n[2]];
    let (r0, r1) = ortho_basis(r2);
    // transpose: m[c][r]; after transpose_m3 the rows become columns of the original
    let m = [[r0[0], r1[0], r2[0]], [r0[1], r1[1], r2[1]], [r0[2], r1[2], r2[2]]];
    let proj: Vec<[f32; 2]> = co.iter().map(|c| [m[0][0] * c[0] + m[1][0] * c[1] + m[2][0] * c[2], m[0][1] * c[0] + m[1][1] * c[1] + m[2][1] * c[2]]).collect();
    let mut tris = polyfill_calc(&proj);
    polyfill_beautify(&proj, &mut tris);
    tris
}

/// mesh_calc_tessellation_for_face (ngon branch): polyfill without beautify
pub fn triangulate_ngon_plain(co: &[V3]) -> Vec<[u32; 3]> {
    let n = poly_normal(co);
    let r2 = [-n[0], -n[1], -n[2]];
    let (r0, r1) = ortho_basis(r2);
    let m = [[r0[0], r1[0], r2[0]], [r0[1], r1[1], r2[1]], [r0[2], r1[2], r2[2]]];
    let proj: Vec<[f32; 2]> = co.iter().map(|c| [m[0][0] * c[0] + m[1][0] * c[1] + m[2][0] * c[2], m[0][1] * c[0] + m[1][1] * c[1] + m[2][1] * c[2]]).collect();
    polyfill_calc(&proj)
}

fn cross_tri_v3(v1: V3, v2: V3, v3: V3) -> V3 {
    let n1 = [v1[0] - v2[0], v1[1] - v2[1], v1[2] - v2[2]];
    let n2 = [v2[0] - v3[0], v2[1] - v3[1], v2[2] - v3[2]];
    [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]]
}

fn is_quad_flip_v3(v1: V3, v2: V3, v3: V3, v4: V3) -> i32 {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (d12, d23, d34, d41) = (sub(v1, v2), sub(v2, v3), sub(v3, v4), sub(v4, v1));
    let mut ret = 0;
    let (ca, cb) = (cross(d12, d23), cross(d34, d41));
    if dot(ca, cb) < 0.0 {
        ret |= 1;
    }
    let (ca, cb) = (cross(d23, d34), cross(d41, d12));
    if dot(ca, cb) < 0.0 {
        ret |= 2;
    }
    ret
}

fn signum_i_ex(a: f32, eps: f32) -> i32 {
    if a > eps {
        1
    } else if a < -eps {
        -1
    } else {
        0
    }
}

/// BM_face_triangulate quad with MOD_TRIANGULATE_QUAD_BEAUTY: true => split along corners (0, 2) ("split_24"), false => along (1, 3)
pub fn quad_split_24_beauty(co: [V3; 4]) -> bool {
    // l_v1 = corner 1, l_v2 = corner 2, l_v3 = corner 3, l_v4 = corner 0
    let (v1, v2, v3, v4) = (co[1], co[2], co[3], co[0]);
    let flip = is_quad_flip_v3(v1, v2, v3, v4);
    if flip & 1 != 0 {
        return true;
    }
    if flip & 2 != 0 {
        return false;
    }
    // bm_edge_calc_rotate_beauty__area (lock_degenerate = false)
    let no_a = cross_tri_v3(v2, v3, v4);
    let no_b = cross_tri_v3(v2, v4, v1);
    let mut no = [no_a[0] + no_b[0], no_a[1] + no_b[1], no_a[2] + no_b[2]];
    let d = no[0] * no[0] + no[1] * no[1] + no[2] * no[2];
    let no_scale = if d > 1.0e-35f32 { d.sqrt() } else { 0.0 };
    if no_scale == 0.0 {
        return false; // FLT_MAX > 0 => split_24 = true actually
    }
    let f = 1.0f32 / no_scale;
    no = [no[0] * f, no[1] * f, no[2] * f];
    let (r0, r1) = ortho_basis(no);
    let m = [[r0[0], r1[0], no[0]], [r0[1], r1[1], no[1]], [r0[2], r1[2], no[2]]];
    let proj = |c: V3| [m[0][0] * c[0] + m[1][0] * c[1] + m[2][0] * c[2], m[0][1] * c[0] + m[1][1] * c[1] + m[2][1] * c[2]];
    let (v1p, v2p, v3p, v4p) = (proj(v1), proj(v2), proj(v3), proj(v4));
    let eps = 1e-5f32;
    if signum_i_ex(cross_tri_v2(v2p, v3p, v4p) / no_scale, eps) + signum_i_ex(cross_tri_v2(v2p, v4p, v1p) / no_scale, eps) == 0 {
        return true; // FLT_MAX
    }
    let (cost, _) = quad_rotate_calc(v1p, v2p, v3p, v4p, false);
    cost > 0.0
}


/// bm_edge_calc_rotate_beauty__area (bmesh_beautify.cc): cost of rotating the edge (v2, v4) of the quad v1 v2 v3 v4 into (v1, v3);
/// negative = the rotated state is better, FLT_MAX = not allowed
pub fn rotate_beauty_area(v1: V3, v2: V3, v3: V3, v4: V3, lock_degenerate: bool) -> f32 {
    let no_a = cross_tri_v3(v2, v3, v4);
    let no_b = cross_tri_v3(v2, v4, v1);
    let mut no = [no_a[0] + no_b[0], no_a[1] + no_b[1], no_a[2] + no_b[2]];
    let d = no[0] * no[0] + no[1] * no[1] + no[2] * no[2];
    let no_scale = if d > 1.0e-35f32 { d.sqrt() } else { 0.0 };
    if no_scale == 0.0 {
        return f32::MAX;
    }
    let f = 1.0f32 / no_scale;
    no = [no[0] * f, no[1] * f, no[2] * f];
    let (r0, r1) = ortho_basis(no);
    let m = [[r0[0], r1[0], no[0]], [r0[1], r1[1], no[1]], [r0[2], r1[2], no[2]]];
    let proj = |c: V3| [m[0][0] * c[0] + m[1][0] * c[1] + m[2][0] * c[2], m[0][1] * c[0] + m[1][1] * c[1] + m[2][1] * c[2]];
    let (v1p, v2p, v3p, v4p) = (proj(v1), proj(v2), proj(v3), proj(v4));
    let eps = 1e-5f32;
    if signum_i_ex(cross_tri_v2(v2p, v3p, v4p) / no_scale, eps) + signum_i_ex(cross_tri_v2(v2p, v4p, v1p) / no_scale, eps) == 0 {
        return f32::MAX;
    }
    quad_rotate_calc(v1p, v2p, v3p, v4p, lock_degenerate).0
}
