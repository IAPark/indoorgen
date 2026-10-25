//! Blender `bpy.ops.mesh.bridge_edge_loops(number_cuts, profile_shape_factor)` for two closed edge loops
//! (bmo_bridge.cc bridge_loop_pair + triangulate/beautify_fill for unequal loops + bmo_subdivide_edgering.cc PATH interpolation).
//! Ported from the Blender 4.2 sources; vertex/face order is not Blender's (surface-equal only).
use crate::mesh::Mesh;
use crate::dhash::HashMap;

type V3 = [f64; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        a
    }
}
fn dist32(a: V3, b: V3) -> f32 {
    let d = [(a[0] as f32) - (b[0] as f32), (a[1] as f32) - (b[1] as f32), (a[2] as f32) - (b[2] as f32)];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

struct Lp {
    v: Vec<u32>,
    no: F3,
}
type F3 = [f32; 3];
fn f3(a: V3) -> F3 {
    [a[0] as f32, a[1] as f32, a[2] as f32]
}
fn dot32(a: F3, b: F3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len32(a: F3, b: F3) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot32(d, d).sqrt()
}
/// BM_edgeloop_calc_center in float32 (vertex visiting order: last, first, second, ...)
fn centre32(pos: &[V3], l: &[u32]) -> F3 {
    let n = l.len();
    let p = |k: usize| f3(pos[l[k % n] as usize]);
    let mut co = [0.0f32; 3];
    let mut totw = 0.0f32;
    let mut w_prev = len32(p(n - 2 + n), p(n - 1));
    for step in 0..n {
        let k = (n - 1 + step) % n;
        let (cur, next) = (p(k), p(k + 1));
        let w_curr = len32(cur, next);
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
    co
}
fn newell32(pos: &[V3], l: &[u32]) -> F3 {
    let n = l.len();
    let mut no = [0.0f32; 3];
    for k in 0..n {
        let (p, q) = (f3(pos[l[(k + n - 1) % n] as usize]), f3(pos[l[k] as usize]));
        no[0] += (p[1] - q[1]) * (p[2] + q[2]);
        no[1] += (p[2] - q[2]) * (p[0] + q[0]);
        no[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let d = dot32(no, no);
    if d > 1.0e-35f32 {
        let ln = d.sqrt();
        if ln < 1e-5 {
            return [0.0, 0.0, 1.0];
        }
        let inv = 1.0f32 / ln;
        [no[0] * inv, no[1] * inv, no[2] * inv]
    } else {
        [0.0, 0.0, 1.0]
    }
}


fn centre(pos: &[V3], l: &[u32]) -> V3 {
    let n = l.len();
    let mut c = [0.0; 3];
    let mut tw = 0.0;
    for k in 0..n {
        let (p, q, r) = (pos[l[(k + n - 1) % n] as usize], pos[l[k] as usize], pos[l[(k + 1) % n] as usize]);
        let w = len(sub(q, p)) + len(sub(r, q));
        c = add(c, mul(q, w));
        tw += w;
    }
    if tw != 0.0 {
        c = mul(c, 1.0 / tw);
    }
    c
}

fn newell(pos: &[V3], l: &[u32]) -> (V3, bool) {
    // BM_edgeloop_calc_normal in float32 (the residual of a degenerate loop decides validity, as in Blender)
    let n = l.len();
    let mut no = [0.0f32; 3];
    for k in 0..n {
        let (p, q) = (f3(pos[l[(k + n - 1) % n] as usize]), f3(pos[l[k] as usize]));
        no[0] += (p[1] - q[1]) * (p[2] + q[2]);
        no[1] += (p[2] - q[2]) * (p[0] + q[0]);
        no[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let d = dot32(no, no);
    if d > 1.0e-35f32 {
        let ln = d.sqrt();
        if ln < 1e-10 {
            return ([0.0, 0.0, 1.0], false);
        }
        let inv = 1.0f32 / ln;
        ([(no[0] * inv) as f64, (no[1] * inv) as f64, (no[2] * inv) as f64], true)
    } else {
        ([0.0, 0.0, 1.0], false)
    }
}

fn flip(l: &mut Lp) {
    l.v.reverse();
    l.no = [-l.no[0], -l.no[1], -l.no[2]];
}

/// BM_edgeloop_expand (split = false)
fn expand(v: &mut Vec<u32>, target: usize) {
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
        let fl = |a: i64, b: i64| a.div_euclid(b);
        let (src2, dst2) = (src * 2, dst * 2);
        let mut error = dst2 - src;
        let mut i = 0i64;
        let mut cur = 0usize;
        let mut iter_prev = 0i64;
        loop {
            let delta = fl(error, dst2);
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

// ---- BLI_heap (min-heap, ties move up) ----
struct Heap {
    tree: Vec<(f32, usize)>, // (value, slot)
    pos: HashMap<usize, usize>,
}
impl Heap {
    fn new() -> Heap {
        Heap { tree: vec![], pos: HashMap::default() }
    }
    fn swap(&mut self, a: usize, b: usize) {
        self.tree.swap(a, b);
        self.pos.insert(self.tree[a].1, a);
        self.pos.insert(self.tree[b].1, b);
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
        self.pos.insert(slot, i);
        self.up(i);
    }
    fn pop_min(&mut self) -> Option<usize> {
        if self.tree.is_empty() {
            return None;
        }
        let top = self.tree[0].1;
        let last = self.tree.len() - 1;
        self.swap(0, last);
        self.tree.pop();
        self.pos.remove(&top);
        if !self.tree.is_empty() {
            self.down(0);
        }
        Some(top)
    }
    fn remove(&mut self, slot: usize) {
        if let Some(&i0) = self.pos.get(&slot) {
            let mut i = i0;
            while i > 0 {
                let p = (i - 1) / 2;
                self.swap(p, i);
                i = p;
            }
            self.pop_min();
        }
    }
}

fn f32v(a: V3) -> [f32; 3] {
    [a[0] as f32, a[1] as f32, a[2] as f32]
}
fn normal_tri(a: V3, b: V3, c: V3) -> Option<[f32; 3]> {
    let (a, b, c) = (f32v(a), f32v(b), f32v(c));
    let n1 = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let n2 = [b[0] - c[0], b[1] - c[1], b[2] - c[2]];
    let n = [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l == 0.0 {
        None
    } else {
        Some([n[0] / l, n[1] / l, n[2] / l])
    }
}
fn angle_nn(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let l = |x: [f32; 3]| (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    if d >= 0.0 {
        2.0 * (l([a[0] - b[0], a[1] - b[1], a[2] - b[2]]) / 2.0).clamp(-1.0, 1.0).asin()
    } else {
        std::f32::consts::PI - 2.0 * (l([a[0] + b[0], a[1] + b[1], a[2] + b[2]]) / 2.0).clamp(-1.0, 1.0).asin()
    }
}

/// bm_edge_calc_rotate_beauty__angle with VERT_RESTRICT_TAG
fn rot_cost(pos: &[V3], side: &HashMap<u32, u8>, v1: u32, v2: u32, v3: u32, v4: u32) -> f32 {
    if side.get(&v1) == side.get(&v3) {
        return f32::MAX;
    }
    if v1 == v3 {
        return f32::MAX;
    }
    let p = |v: u32| pos[v as usize];
    let (a, b) = match (normal_tri(p(v2), p(v3), p(v4)), normal_tri(p(v2), p(v4), p(v1))) {
        (Some(a), Some(b)) => (a, b),
        _ => return f32::MAX,
    };
    let angle_24 = angle_nn(a, b);
    let (c, d) = match (normal_tri(p(v1), p(v2), p(v3)), normal_tri(p(v1), p(v3), p(v4))) {
        (Some(c), Some(d)) => (c, d),
        _ => return f32::MAX,
    };
    angle_nn(c, d) - angle_24
}

fn ekey(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// triangulate (quad: shorter-normal-angle diagonal) + beautify_fill(method angle, restrict tag)
fn triangulate_beautify(pos: &[V3], quads: &[Vec<u32>], side: &HashMap<u32, u8>) -> Vec<[u32; 3]> {
    let mut tris: Vec<[u32; 3]> = vec![];
    for f in quads {
        if f.len() == 3 {
            tris.push([f[0], f[1], f[2]]);
        } else {
            // split along 2-4 (l_v1 = second vertex) or 1-3 by the beauty cost, as BM_face_triangulate
            let (a, b, c, d) = (f[0], f[1], f[2], f[3]);
            let cost = rot_cost(pos, &HashMap::default(), d, a, b, c); // current diag a-c? evaluated below by angle
            let _ = cost;
            let p = |v: u32| pos[v as usize];
            let ang = |t1: [u32; 3], t2: [u32; 3]| -> f32 {
                match (normal_tri(p(t1[0]), p(t1[1]), p(t1[2])), normal_tri(p(t2[0]), p(t2[1]), p(t2[2]))) {
                    (Some(x), Some(y)) => angle_nn(x, y),
                    _ => f32::MAX,
                }
            };
            let a_ac = ang([a, b, c], [a, c, d]);
            let a_bd = ang([a, b, d], [b, c, d]);
            if a_bd < a_ac {
                tris.push([a, b, d]);
                tris.push([b, c, d]);
            } else {
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
    }
    beautify_loop(pos, tris, side, None, false, None)
}

/// generic BM_mesh_beautify_fill on triangles (edge rotation until no improvement): `side` restricts rotations to edges whose new diagonal joins the
/// two sides (VERT_RESTRICT_TAG, angle method as the bridge uses it); with `area` = Some(lock_degenerate) the AREA metric (bpy beautify_fill) is used.
/// `edge_order`: candidate edge order (Blender's mesh edge list), default first use order of the triangle corners.
pub fn beautify_loop(pos: &[V3], tris: Vec<[u32; 3]>, side: &HashMap<u32, u8>, edge_order: Option<&[[u32; 2]]>, area: bool, movable: Option<&[bool]>) -> Vec<[u32; 3]> {
    let bl_order = area; // the bpy op path also reproduces Blender's face slot reuse
    let lock = true;
    let mut tris = tris;
    let mut alive: Vec<bool> = vec![true; tris.len()];
    let mut free: Vec<usize> = vec![];
    let ord: Vec<i32> = std::env::var("BEAUTY_ORD").ok().map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or(vec![1, 1]);
    let cost_of = |v1: u32, v2: u32, v3: u32, v4: u32| -> f32 {
        if area {
            if v1 == v3 {
                return f32::MAX;
            }
            let p = |v: u32| f32v(pos[v as usize]);
            super::polyfill::bm_rotate_beauty_area(p(v1), p(v2), p(v3), p(v4), lock)
        } else {
            rot_cost(pos, side, v1, v2, v3, v4)
        }
    };
    // edge map
    let mut edges: Vec<(u32, u32)> = vec![];
    let mut emap: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let key = ekey(t[k], t[(k + 1) % 3]);
            let e = emap.entry(key).or_insert_with(|| {
                edges.push(key);
                vec![]
            });
            e.push(fi);
        }
    }
    // candidate slots: edges with exactly two triangle faces
    if let Some(eo) = edge_order {
        let have: std::collections::HashSet<(u32, u32)> = edges.iter().cloned().collect();
        let mut ord: Vec<(u32, u32)> = eo.iter().map(|e| ekey(e[0], e[1])).filter(|k| have.contains(k)).collect();
        let seen: std::collections::HashSet<(u32, u32)> = ord.iter().cloned().collect();
        ord.extend(edges.iter().cloned().filter(|k| !seen.contains(k)));
        edges = ord;
    }
    let mut slots: Vec<(u32, u32)> = edges.iter().cloned().filter(|k| emap[k].len() == 2 && movable.map_or(true, |m| emap[k].iter().all(|&f| m[f]))).collect();
    let mut slot_of: HashMap<(u32, u32), usize> = slots.iter().cloned().enumerate().map(|(i, k)| (k, i)).collect();
    let mut states: Vec<Vec<([u32; 2], [u32; 2])>> = vec![vec![]; slots.len()];
    let quad_of = |tris: &Vec<[u32; 3]>, emap: &HashMap<(u32, u32), Vec<usize>>, key: (u32, u32)| -> Option<[u32; 4]> {
        let fs = &emap[&key];
        if fs.len() != 2 {
            return None;
        }
        // f1 contains directed key.0 -> key.1 ; v2 = key.0, v4 = key.1
        let mut res = None;
        for &fi in fs {
            let t = tris[fi];
            for k in 0..3 {
                if t[k] == key.0 && t[(k + 1) % 3] == key.1 {
                    let v1 = t[(k + 2) % 3];
                    // the other face has key.1 -> key.0
                    let o = if fs[0] == fi { fs[1] } else { fs[0] };
                    let u = tris[o];
                    for j in 0..3 {
                        if u[j] == key.1 && u[(j + 1) % 3] == key.0 {
                            let v3 = u[(j + 2) % 3];
                            res = Some([v1, key.0, v3, key.1]);
                        }
                    }
                }
            }
        }
        res
    };
    let mut heap = Heap::new();
    for (i, k) in slots.iter().enumerate() {
        if let Some(q) = quad_of(&tris, &emap, *k) {
            let c = cost_of(q[0], q[1], q[2], q[3]);
            if c < 0.0 {
                heap.insert(c, i);
            }
        }
    }
    while let Some(i) = heap.pop_min() {
        let key = slots[i];
        let q = match quad_of(&tris, &emap, key) {
            Some(q) => q,
            None => continue,
        };
        let (v1, v2, v3, v4) = (q[0], q[1], q[2], q[3]);
        let newkey = ekey(v1, v3);
        if emap.contains_key(&newkey) {
            continue; // BM_EDGEROT_CHECK_EXISTS
        }
        // rotate
        let fs0 = emap.remove(&key).unwrap();
        let mut fs = fs0.clone();
        if bl_order {
            // BM_edge_rotate = join the two triangles into a quad (new slot), kill them, split the quad (new slot from the free list)
            let has_dir = |t: [u32; 3]| (0..3).any(|k| t[k] == key.0 && t[(k + 1) % 3] == key.1);
            let (fa, fb) = if has_dir(tris[fs0[0]]) { (fs0[0], fs0[1]) } else { (fs0[1], fs0[0]) };
            let qs = match free.pop() {
                Some(x) => x,
                None => {
                    tris.push([0; 3]);
                    alive.push(true);
                    tris.len() - 1
                }
            };
            alive[qs] = true;
            let (k1, k2) = if ord[0] == 0 { (fa, fb) } else { (fb, fa) };
            alive[fa] = false;
            alive[fb] = false;
            free.push(k1);
            free.push(k2);
            let ns = free.pop().unwrap();
            alive[ns] = true;
            fs = if ord[1] == 0 { vec![qs, ns] } else { vec![ns, qs] };
            let _ = &fs0;
            // outer edge face lists drop the old faces
            for (a, b) in [(v1, v2), (v2, v3), (v3, v4), (v4, v1)] {
                let l = emap.get_mut(&ekey(a, b)).unwrap();
                l.retain(|f| *f != fa && *f != fb);
            }
        }
        tris[fs[0]] = [v1, v2, v3];
        tris[fs[1]] = [v1, v3, v4];
        // fix the four outer edges' face lists
        for (a, b) in [(v1, v2), (v2, v3), (v3, v4), (v4, v1)] {
            let l = emap.get_mut(&ekey(a, b)).unwrap();
            l.retain(|f| *f != fs[0] && *f != fs[1]);
        }
        emap.get_mut(&ekey(v1, v2)).unwrap().push(fs[0]);
        emap.get_mut(&ekey(v2, v3)).unwrap().push(fs[0]);
        emap.get_mut(&ekey(v3, v4)).unwrap().push(fs[1]);
        emap.get_mut(&ekey(v4, v1)).unwrap().push(fs[1]);
        emap.insert(newkey, vec![fs[0], fs[1]]);
        slot_of.remove(&key);
        slot_of.insert(newkey, i);
        slots[i] = newkey;
        // state memory: current state
        let cur = (ekey_arr(newkey), ekey_arr(ekey(v2, v4)));
        states[i].push(cur);
        // update the four outer edges
        for (a, b) in [(v2, v3), (v1, v2), (v3, v4), (v4, v1)] {
            // order as l->next->e, l->prev->e, ... (order only affects heap ties)
            let k2 = ekey(a, b);
            if let Some(&s) = slot_of.get(&k2) {
                heap.remove(s);
                if let Some(q2) = quad_of(&tris, &emap, k2) {
                    // alternate state check
                    let alt = (ekey_arr(ekey(q2[0], q2[2])), ekey_arr(ekey(q2[1], q2[3])));
                    if states[s].contains(&alt) {
                        continue;
                    }
                    let c = cost_of(q2[0], q2[1], q2[2], q2[3]);
                    if c < 0.0 {
                        heap.insert(c, s);
                    }
                }
            }
        }
    }
    if bl_order {
        return tris.into_iter().zip(alive).filter(|x| x.1).map(|x| x.0).collect();
    }
    tris
}
fn ekey_arr(k: (u32, u32)) -> [u32; 2] {
    [k.0, k.1]
}

fn forward_diff(q: [f64; 4], it: usize) -> Vec<f64> {
    let f = it as f32;
    let (q0, q1, q2, q3) = (q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32);
    let rt0 = q0;
    let rt1 = 3.0f32 * (q1 - q0) / f;
    let f2 = f * f;
    let rt2 = 3.0f32 * (q0 - 2.0f32 * q1 + q2) / f2;
    let f3 = f2 * f;
    let rt3 = (q3 - q0 + 3.0f32 * (q1 - q2)) / f3;
    let (mut a0, mut a1, mut a2, a3) = (rt0, rt1 + rt2 + rt3, 2.0f32 * rt2 + 6.0f32 * rt3, 6.0f32 * rt3);
    let mut out = vec![];
    for _ in 0..=it {
        out.push(a0 as f64);
        a0 += a1;
        a1 += a2;
        a2 += a3;
    }
    out
}

fn rotate_about(axis: V3, angle: f64, v: V3) -> V3 {
    let a = norm(axis);
    let (s, c) = angle.sin_cos();
    add(add(mul(v, c), mul(cross(a, v), s)), mul(a, dot(a, v) * (1.0 - c)))
}

/// bridge two closed loops (given in BMesh edge-loop list order: `la` is eloops.first). Appends faces and cut vertices to `m`.
/// `cuts` >= 0; `shape` = profile_shape_factor (SMOOTH falloff).
pub fn bridge_two_loops(m: &mut Mesh, la: Vec<u32>, lb: Vec<u32>, cuts: usize, shape: f64) {
    bridge_pair(m, la, lb, cuts, shape, &mut None);
}

pub type EdgeMap = HashMap<(u32, u32), (u32, bool)>;
fn edge_map(m: &Mesh) -> EdgeMap {
    let mut ecount: EdgeMap = HashMap::default();
    for f in &m.faces {
        add_face_edges(&mut ecount, f);
    }
    ecount
}
fn add_face_edges(ecount: &mut EdgeMap, f: &[u32]) {
    for k in 0..f.len() {
        let (u, w) = (f[k], f[(k + 1) % f.len()]);
        let e = ecount.entry(ekey(u, w)).or_insert((0, u < w));
        e.0 += 1;
    }
}

/// bezier_handle_calc_length_v3 (bmo_subdivide_edgering.cc)
fn handle_len3(ca: V3, na: V3, cb: V3, nb: V3) -> f64 {
    let d = dot(na, nb);
    let mut fac = 1.333333f64;
    if d < 0.0 {
        let t = 1.0 + d;
        fac = fac * t + 0.75 * (1.0 - t);
    }
    let cr = cross(na, nb);
    let l = if dot(cr, cr) > f32::EPSILON as f64 {
        let q = add(cr, ca);
        let u = sub(q, ca);
        let t = dot(sub(cb, ca), u) / dot(u, u);
        len(sub(cb, add(ca, mul(u, t))))
    } else {
        len(sub(cb, ca))
    };
    (l * 0.5) * fac
}

/// same, returning the two loop lists after the flips / rotation (what a following pair of a multi-loop bridge sees)
pub fn bridge_pair(m: &mut Mesh, la: Vec<u32>, lb: Vec<u32>, cuts: usize, shape: f64, cache: &mut Option<EdgeMap>) -> (Vec<u32>, Vec<u32>) {
    bridge_pair_ex(m, la, lb, cuts, shape, cache, false)
}

/// two-loop bridge with interpolation="SURFACE" (strand handles along the surface tangents of the bridge faces)
pub fn bridge_two_loops_surface(m: &mut Mesh, la: Vec<u32>, lb: Vec<u32>, cuts: usize, shape: f64) {
    bridge_pair_ex(m, la, lb, cuts, shape, &mut None, true);
}

/// `surface`: interpolation="SURFACE" instead of PATH
pub fn bridge_pair_ex(m: &mut Mesh, la: Vec<u32>, lb: Vec<u32>, cuts: usize, shape: f64, cache: &mut Option<EdgeMap>, surface: bool) -> (Vec<u32>, Vec<u32>) {
    bridge_pair_opts(m, la, lb, cuts, shape, cache, if surface { Interp::Surface } else { Interp::Path }, 1.0, "SMOOTH")
}

thread_local! {
    /// extra (vertex on loop a, vertex on loop b) rungs whose cut positions `bridge_pair_opts` should also produce
    static EXTRA_RUNGS: std::cell::RefCell<Vec<(u32, u32)>> = std::cell::RefCell::new(vec![]);
    /// cut vertex positions of the last `bridge_pair_opts` run, per cross edge (vertex on loop a, vertex on loop b), layers 1..cuts from a
    static CUTPOS: std::cell::RefCell<HashMap<(u32, u32), Vec<V3>>> = std::cell::RefCell::new(HashMap::default());
}

/// `interpolation` of bridge_edge_loops
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Interp {
    Linear,
    Path,
    Surface,
}

/// bmesh_subd_falloff_calc (profile_shape enum of bridge_edge_loops)
pub fn subd_falloff(kind: &str, v: f64) -> f64 {
    match kind {
        "SPHERE" => (2.0 * v - v * v).max(0.0).sqrt(),
        "ROOT" => v.max(0.0).sqrt(),
        "SHARP" => v * v,
        "LINEAR" => v,
        "INVERSE_SQUARE" => v * (2.0 - v),
        _ => 3.0 * v * v - 2.0 * v * v * v,
    }
}

/// full bridge_edge_loops options: `smooth` = smoothness (bezier handle scale, Blender default 1.0), `profile_shape` = enum name
pub fn bridge_pair_opts(m: &mut Mesh, la: Vec<u32>, lb: Vec<u32>, cuts: usize, shape: f64, cache: &mut Option<EdgeMap>, mode: Interp, smooth: f64, profile_shape: &str) -> (Vec<u32>, Vec<u32>) {
    let surface = mode == Interp::Surface;
    let pos0 = m.pos.clone();
    let mut a = Lp { v: la, no: [0.0; 3] };
    let mut b = Lp { v: lb, no: [0.0; 3] };
    if a.v.len() < b.v.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let (na_len, nb_len) = (a.v.len(), b.v.len());
    let ca32 = centre32(&pos0, &a.v);
    let cb32 = centre32(&pos0, &b.v);
    let el_dir32 = [ca32[0] - cb32[0], ca32[1] - cb32[1], ca32[2] - cb32[2]];
    a.no = newell32(&pos0, &a.v);
    b.no = newell32(&pos0, &b.v);
    let dot_a = dot32(a.no, el_dir32);
    let dot_b = dot32(b.no, el_dir32);
    let eps = 1e-5f32;
    if dot32(el_dir32, el_dir32) < eps || (dot_a.abs() < eps && dot_b.abs() < eps) {
        if dot32(a.no, b.no) < 0.0 {
            flip(&mut b);
        }
    } else if (dot_a < 0.0) != (dot_b < 0.0) {
        flip(&mut b);
    }
    {
        let no = [a.no[0] + b.no[0], a.no[1] + b.no[1], a.no[2] + b.no[2]];
        if dot32(no, el_dir32) < 0.0 {
            flip(&mut a);
            flip(&mut b);
        }
    }
    // winding votes from existing faces
    if !m.faces.is_empty() {
        if cache.is_none() {
            *cache = Some(edge_map(m));
        }
        let ecount = cache.as_ref().unwrap();
        let mut votes = [0i32; 2];
        let mut dir = 1;
        for (i, l) in [&a, &b].iter().enumerate() {
            let n = l.v.len();
            for k in 0..n {
                let (u, w) = (l.v[k], l.v[(k + 1) % n]);
                if let Some(&(c, fw)) = ecount.get(&ekey(u, w)) {
                    if c == 1 {
                        // e->l->v == el ?  face traverses min->max when fw
                        let l_v_is_u = (u < w) == fw;
                        votes[i] += if l_v_is_u { dir } else { -dir };
                    }
                }
            }
            dir = -dir;
        }
        if votes[0] != 0 || votes[1] != 0 {
            let mut fl = [false, false];
            let (mut v0, mut v1) = (votes[0], votes[1]);
            if dot_a.abs() < eps && v0 < 0 {
                fl[0] = !fl[0];
                v0 = -v0;
            }
            if dot_b.abs() < eps && v1 < 0 {
                fl[1] = !fl[1];
                v1 = -v1;
            }
            if v0 + v1 < 0 {
                fl[0] = !fl[0];
                fl[1] = !fl[1];
            }
            if fl[0] {
                flip(&mut a);
            }
            if fl[1] {
                flip(&mut b);
            }
        }
    }
    let mut bx = b.v.clone();
    if na_len > nb_len {
        expand(&mut bx, na_len);
    }
    // best rotation
    {
        let mut best = f32::MAX;
        let mut kbest = None;
        let n = bx.len();
        for k in 0..n {
            let mut l = 0.0f32;
            for i in 0..n {
                l += dist32(pos0[a.v[i] as usize], pos0[bx[(k + i) % n] as usize]);
                if l >= best {
                    break;
                }
            }
            if l < best {
                best = l;
                kbest = Some(k);
            }
        }
        if let Some(k) = kbest {
            if std::env::var("BRIDGEDBG").is_ok() {
                let mut ls: Vec<(f32, usize)> = (0..n).map(|k| { let mut l = 0.0f32; for i in 0..n { l += dist32(pos0[a.v[i] as usize], pos0[bx[(k + i) % n] as usize]); } (l, k) }).collect();
                ls.sort_by(|x, y| x.partial_cmp(y).unwrap());
                eprintln!("bridge best rotations: {:?}", &ls[..4.min(ls.len())]);
            }
            bx.rotate_left(k);
        }
    }
    // bridge faces [v_b, v_b_next, v_a_next, v_a]
    let n = na_len;
    let mut faces: Vec<Vec<u32>> = vec![];
    for i in 0..n {
        let (va, van) = (a.v[i], a.v[(i + 1) % n]);
        let (vb, vbn) = (bx[i], bx[(i + 1) % n]);
        if vb != vbn {
            faces.push(vec![vb, vbn, van, va]);
        } else {
            faces.push(vec![vb, van, va]);
        }
    }
    let mut side: HashMap<u32, u8> = HashMap::default();
    for &v in &a.v {
        side.insert(v, 0);
    }
    for &v in &b.v {
        side.insert(v, 1);
    }
    let faces: Vec<Vec<u32>> = if na_len != nb_len {
        triangulate_beautify(&pos0, &faces, &side).into_iter().map(|t| t.to_vec()).collect()
    } else {
        faces
    };
    if cuts == 0 {
        if let Some(c) = cache.as_mut() {
            for f in &faces {
                add_face_edges(c, f);
            }
        }
        m.faces.extend(faces);
        return (a.v.clone(), bx);
    }
    // ---- subdivide_edgering, PATH interpolation, smooth = 1 ----
    let resolu = cuts + 2;
    let (ca, cb) = (centre(&pos0, &a.v), centre(&pos0, &b.v));
    let (mut na, va_ok) = newell(&pos0, &a.v);
    let (mut nb, vb_ok) = newell(&pos0, &b.v);
    let el_d = sub(ca, cb);
    let nd = norm(el_d);
    // BM_edgeloop_calc_normal_aligned when the plain normal is degenerate (e.g. open straight polylines)
    let aligned = |l: &[u32]| -> V3 {
        let n = l.len();
        let mut no = [0.0; 3];
        for k in 0..n {
            let (p, q) = (pos0[l[(k + n - 1) % n] as usize], pos0[l[k] as usize]);
            let dir = sub(q, p);
            no = add(no, cross(dir, cross(nd, dir)));
        }
        let ln = len(no);
        if ln < 1e-10 {
            [0.0, 0.0, 1.0]
        } else {
            mul(no, 1.0 / ln)
        }
    };
    if !va_ok {
        na = aligned(&a.v);
    }
    if !vb_ok {
        nb = aligned(&b.v);
    }
    if dot(na, el_d) > 0.0 {
        na = mul(na, -1.0);
    }
    if dot(nb, el_d) < 0.0 {
        nb = mul(nb, -1.0);
    }
    // bezier handle length
    let hl = {
        let d = dot(na, nb);
        let mut fac = 1.333333f64;
        if d < 0.0 {
            let t = 1.0 + d;
            fac = fac * t + 0.75 * (1.0 - t);
        }
        let cr = cross(na, nb);
        let l = if dot(cr, cr) > f32::EPSILON as f64 {
            let q = add(cr, ca);
            // closest point of cb on the line (ca, q)
            let u = sub(q, ca);
            let t = dot(sub(cb, ca), u) / dot(u, u);
            len(sub(cb, add(ca, mul(u, t))))
        } else {
            len(sub(cb, ca))
        };
        (l * 0.5) * fac * smooth
    };
    let ha = add(ca, mul(na, hl));
    let hb = add(cb, mul(nb, hl));
    let mut cm = vec![[0.0; 3]; resolu];
    for k in 0..3 {
        let c = forward_diff([ca[k], ha[k], hb[k], cb[k]], resolu - 1);
        for i in 0..resolu {
            cm[i][k] = c[i];
        }
    }
    let mut dirs = vec![[0.0; 3]; resolu];
    dirs[0] = na;
    dirs[resolu - 1] = mul(nb, -1.0);
    for i in 1..resolu - 1 {
        let d12 = norm(sub(cm[i], cm[i - 1]));
        let d23 = norm(sub(cm[i + 1], cm[i]));
        dirs[i] = norm(add(d12, d23));
    }
    // rotation (as a function) of transported frames
    let mut rots: Vec<Vec<[f64; 3]>> = vec![]; // matrices as three column images of unit vectors
    let apply_id = |v: V3| v;
    let _ = apply_id;
    // keep cumulative rotation as a list of (axis, angle) steps
    let mut steps: Vec<Option<(V3, f64)>> = vec![None; resolu];
    for i in 1..resolu {
        let d = dot(dirs[i - 1], dirs[i]).clamp(-1.0, 1.0);
        let ang = if d >= 0.0 { 2.0 * (len(sub(dirs[i - 1], dirs[i])) / 2.0).min(1.0).asin() } else { std::f64::consts::PI - 2.0 * (len(add(dirs[i - 1], dirs[i])) / 2.0).min(1.0).asin() };
        if ang > 0.0 {
            steps[i] = Some((cross(dirs[i - 1], dirs[i]), ang));
        }
    }
    let _ = &mut rots;
    let cum = |i: usize, v: V3| -> V3 {
        // Delta_i v = step_i(...step_1(v))
        let mut x = v;
        for j in 1..=i {
            if let Some((ax, an)) = steps[j] {
                x = rotate_about(ax, an, x);
            }
        }
        x
    };
    let cum_inv = |i: usize, v: V3| -> V3 {
        let mut x = v;
        for j in (1..=i).rev() {
            if let Some((ax, an)) = steps[j] {
                x = rotate_about(ax, -an, x);
            }
        }
        x
    };
    let falloff: Vec<f64> = (0..resolu)
        .map(|i| {
            let mut s = 1.0;
            if shape != 0.0 {
                let mut fac = i as f64 / (resolu - 1) as f64;
                fac = (1.0 - 2.0 * (0.5 - fac).abs()).abs();
                fac = subd_falloff(profile_shape, fac);
                s += fac * shape;
            }
            s
        })
        .collect();
    // cut vertices per cross edge
    let mut cross_edges: Vec<(u32, u32)> = vec![];
    let mut seen: HashMap<(u32, u32), usize> = HashMap::default();
    let on_a: HashMap<u32, ()> = a.v.iter().map(|&v| (v, ())).collect();
    for f in &faces {
        for k in 0..f.len() {
            let (u, w) = (f[k], f[(k + 1) % f.len()]);
            let (x, y) = if on_a.contains_key(&u) { (u, w) } else { (w, u) };
            if on_a.contains_key(&x) && !on_a.contains_key(&y) {
                seen.entry((x, y)).or_insert_with(|| {
                    cross_edges.push((x, y));
                    cross_edges.len() - 1
                });
            }
        }
    }
    for (x, y) in EXTRA_RUNGS.with(|e| e.borrow().clone()) {
        seen.entry((x, y)).or_insert_with(|| {
            cross_edges.push((x, y));
            cross_edges.len() - 1
        });
    }
    // SURFACE: per-vertex surface tangent = normalized sum of the in-face perpendiculars of the rim edges (bm_vert_calc_surface_tangent)
    let mut tan: HashMap<u32, V3> = HashMap::default();
    if surface {
        let on_b: HashMap<u32, ()> = b.v.iter().map(|&v| (v, ())).collect();
        for f in &faces {
            let n = f.len();
            let p: Vec<V3> = f.iter().map(|&v| pos0[v as usize]).collect();
            let no = if n == 3 { norm(cross(sub(p[1], p[0]), sub(p[2], p[0]))) } else { norm(cross(sub(p[2], p[0]), sub(p[3], p[1]))) };
            for k in 0..n {
                let (u, w) = (f[k], f[(k + 1) % n]);
                if (on_a.contains_key(&u) && on_a.contains_key(&w)) || (on_b.contains_key(&u) && on_b.contains_key(&w)) {
                    let t = norm(cross(sub(pos0[u as usize], pos0[w as usize]), no));
                    for q in [u, w] {
                        let e = tan.entry(q).or_insert([0.0; 3]);
                        *e = add(*e, t);
                    }
                }
            }
        }
        for t in tan.values_mut() {
            *t = norm(*t);
        }
    }
    let base = m.pos.len();
    let mut cut_id: HashMap<(u32, u32), Vec<u32>> = HashMap::default();
    let mut newpos: Vec<V3> = vec![];
    for &(x, y) in &cross_edges {
        let mut ids = vec![];
        if surface {
            let (co_a, co_b) = (pos0[x as usize], pos0[y as usize]);
            let (no_a, no_b) = (tan[&x], tan[&y]);
            let hl2 = handle_len3(co_a, no_a, co_b, no_b) * smooth;
            let (h_a, h_b) = (add(co_a, mul(no_a, hl2)), add(co_b, mul(no_b, hl2)));
            let mut st = vec![[0.0; 3]; resolu];
            for k in 0..3 {
                let c = forward_diff([co_a[k], h_a[k], h_b[k], co_b[k]], resolu - 1);
                for i in 0..resolu {
                    st[i][k] = c[i];
                }
            }
            for i in 1..resolu - 1 {
                let mut p = st[i];
                if shape != 0.0 {
                    p = add(mul(cm[i], 1.0 - falloff[i]), mul(p, falloff[i]));
                }
                ids.push((base + newpos.len()) as u32);
                newpos.push(p);
            }
            cut_id.insert((x, y), ids);
            continue;
        }
        if mode == Interp::Linear {
            for i in 1..resolu - 1 {
                let t = i as f64 / (resolu - 1) as f64;
                let (px, py) = (pos0[x as usize], pos0[y as usize]);
                let mut p = add(mul(px, 1.0 - t), mul(py, t));
                if shape != 0.0 {
                    let lc = add(mul(ca, 1.0 - t), mul(cb, t));
                    p = add(mul(lc, 1.0 - falloff[i]), mul(p, falloff[i]));
                }
                ids.push((base + newpos.len()) as u32);
                newpos.push(p);
            }
            cut_id.insert((x, y), ids);
            continue;
        }
        for i in 1..resolu - 1 {
            // transform_point_by_tri: similarity from the start / end frames to frame i
            let pa = add(cm[i], mul(cum(i, sub(pos0[x as usize], ca)), falloff[i] / falloff[0]));
            let rb = cum(i, cum_inv(resolu - 1, sub(pos0[y as usize], cb)));
            let pb = add(cm[i], mul(rb, falloff[i] / falloff[resolu - 1]));
            let t = i as f64 / (resolu - 1) as f64;
            let p = add(mul(pa, 1.0 - t), mul(pb, t));
            ids.push((base + newpos.len()) as u32);
            newpos.push(p);
        }
        cut_id.insert((x, y), ids);
    }
    m.pos.extend(newpos);
    CUTPOS.with(|c| {
        let mut c = c.borrow_mut();
        c.clear();
        for (k, ids) in &cut_id {
            c.insert(*k, ids.iter().map(|&i| m.pos[i as usize]).collect());
        }
    });
    // slice every bridge face into cuts+1 slabs measured from its rim edge
    for f in &faces {
        emit_slabs(m, f, &on_a, &cut_id, cuts);
    }
    (a.v.clone(), bx)
}

/// emit the slabs of one bridge face (tri or quad) from its a-side to its b-side
fn emit_slabs(m: &mut Mesh, f: &[u32], on_a: &HashMap<u32, ()>, cut_id: &HashMap<(u32, u32), Vec<u32>>, cuts: usize) {
    let n = f.len();
    let side: Vec<bool> = f.iter().map(|v| on_a.contains_key(v)).collect();
    let pt = |x: u32, y: u32, k: usize| -> u32 {
        if k == 0 {
            x
        } else if k == cuts + 1 {
            y
        } else {
            cut_id[&(x, y)][k - 1]
        }
    };
    // find the cyclic position i such that f[i] is on the a side and f[i+1] is on the b side or vice versa
    // tri (2 a, 1 b) or (1 a, 2 b); quad (2 a, 2 b)
    let na = side.iter().filter(|&&s| s).count();
    let nb = n - na;
    // rotate so that the face starts at the first a vertex after a b vertex
    let start = (0..n).find(|&k| side[k] && !side[(k + n - 1) % n]).unwrap();
    let g: Vec<u32> = (0..n).map(|k| f[(start + k) % n]).collect();
    // g = [a0 .. a(na-1), b0 .. b(nb-1)]
    let a_run = &g[..na];
    let b_run = &g[na..];
    // cross edge of each corner: a_last -> b0 (X1), b_last -> a0 (X2)
    let x1 = (a_run[na - 1], b_run[0]);
    let x2 = (a_run[0], b_run[nb - 1]);
    // vertex position on level k: a-run vertices map to (their a vertex) at level 0 only; for na == 2 the second a vertex lies on x1, first on x2
    let level = |k: usize| -> Vec<u32> {
        let mut v = vec![];
        // order around the face at level k: a-side points first (from a0 ... a_last), then b-side
        if na == 2 {
            v.push(pt(x2.0, x2.1, k));
            v.push(pt(x1.0, x1.1, k));
        } else if na == 1 {
            // apex on a: both cross edges share a0
            v.push(pt(x2.0, x2.1, k));
        } else {
            for &a in a_run {
                let _ = a;
            }
        }
        v
    };
    let _ = (nb, level);
    if na == 2 && nb == 1 {
        // tri a0 a1 b : slabs a0 a1 -> (x2 , x1) rings
        for k in 0..=cuts {
            let p = [pt(x2.0, x2.1, k), pt(x1.0, x1.1, k), pt(x1.0, x1.1, k + 1), pt(x2.0, x2.1, k + 1)];
            if k == cuts {
                m.faces.push(vec![p[0], p[1], p[2]]);
            } else {
                m.faces.push(p.to_vec());
            }
        }
    } else if na == 1 && nb == 2 {
        // tri a0 b0 b1 : x1 = (a0,b0), x2 = (a0,b1) ; f = [a0, b0, b1] ; slabs from the apex on a
        for k in 0..=cuts {
            if k == 0 {
                m.faces.push(vec![x1.0, pt(x1.0, x1.1, 1), pt(x2.0, x2.1, 1)]);
            } else {
                m.faces.push(vec![pt(x1.0, x1.1, k), pt(x1.0, x1.1, k + 1), pt(x2.0, x2.1, k + 1), pt(x2.0, x2.1, k)]);
            }
        }
    } else if na == 2 && nb == 2 {
        // quad a0 a1 b0 b1 : x1 = (a1, b0), x2 = (a0, b1)
        for k in 0..=cuts {
            m.faces.push(vec![pt(x2.0, x2.1, k), pt(x1.0, x1.1, k), pt(x1.0, x1.1, k + 1), pt(x2.0, x2.1, k + 1)]);
        }
    } else {
        m.faces.push(f.to_vec());
    }
}


/// bridge_edge_loops with Blender's vertex and face ORDER: the topology comes from the BMesh port (`bm::bridge`: disk cycles, face slot
/// reuse, edge split / face split order), the cut vertex positions from `bridge_pair_opts` run on a copy.
/// `edge_order`: the Blender mesh's edge list (vertex pairs) if known, else first-use order of the face corners; edges between consecutive
/// loop vertices are added when missing. Two loops only. Returns the new mesh (original vertices first, then cut vertices in Blender's order).
pub fn bridge_bm(m: &Mesh, la: &[u32], lb: &[u32], cuts: usize, shape: f64, mode: Interp, smooth: f64, profile_shape: &str, edge_order: Option<&[[u32; 2]]>) -> Result<Mesh, String> {
    bridge_bm_ex(m, la, lb, cuts, shape, mode, smooth, profile_shape, edge_order).map(|r| r.0)
}

/// `bridge_bm` that also returns the Blender edge list of the result (BMesh edge order), as needed by modifiers that depend on it (SOLIDIFY NON_MANIFOLD)
pub fn bridge_bm_ex(m: &Mesh, la: &[u32], lb: &[u32], cuts: usize, shape: f64, mode: Interp, smooth: f64, profile_shape: &str, edge_order: Option<&[[u32; 2]]>) -> Result<(Mesh, Vec<[u32; 2]>), String> {
    use crate::bm::{bridge as bb, BMesh};
    // BMesh input
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut add_edge = |a: u32, b: u32, edges: &mut Vec<[u32; 2]>| -> u32 {
        let k = (a.min(b), a.max(b));
        *emap.entry(k).or_insert_with(|| {
            edges.push(match edge_order.and_then(|eo| eo.iter().find(|e| (e[0].min(e[1]), e[0].max(e[1])) == k)) {
                Some(e) => *e,
                None => [k.0, k.1],
            });
            (edges.len() - 1) as u32
        })
    };
    if let Some(eo) = edge_order {
        for e in eo {
            add_edge(e[0], e[1], &mut edges);
        }
    }
    let mut ce: Vec<Vec<u32>> = vec![];
    for f in &m.faces {
        ce.push((0..f.len()).map(|c| add_edge(f[c], f[(c + 1) % f.len()], &mut edges)).collect());
    }
    let mut sel_set: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for l in [la, lb] {
        // a loop is closed unless the mesh's edge list is known and lacks the closing edge
        let closed = edge_order.map_or(true, |eo| eo.iter().any(|e| (e[0].min(e[1]), e[0].max(e[1])) == (l[0].min(l[l.len() - 1]), l[0].max(l[l.len() - 1]))));
        let n = if closed { l.len() } else { l.len() - 1 };
        for i in 0..n {
            let id = add_edge(l[i], l[(i + 1) % l.len()], &mut edges);
            sel_set.insert(id);
        }
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &m.faces, &ce);
    let eids = bm.edge_ids();
    // BMesh edge slots == creation order == `edges` order
    let sel = |e: usize| sel_set.contains(&(e as u32)) && eids.contains(&e);
    let topo = bb::bridge_edge_loops(&mut bm, &sel, cuts)?;
    let mode_s = match mode {
        Interp::Linear => "LINEAR",
        Interp::Path => "PATH",
        Interp::Surface => "SURFACE",
    };
    if cuts > 0 && !bb::assign_ring_positions(&mut bm, &topo, cuts, mode_s, smooth as f32, shape as f32, profile_shape) {
        // SURFACE: positions from the f64 implementation, asked for exactly the rungs of the BMesh topology
        let (olda, _oldb) = if la.len() < lb.len() { (lb, la) } else { (la, lb) };
        let on_a: std::collections::HashSet<u32> = olda.iter().copied().collect();
        let norm = |p: u32, q: u32| if on_a.contains(&p) { (p, q) } else { (q, p) };
        let extra: Vec<(u32, u32)> = topo.rings.iter().map(|r| norm(r[0] as u32, r[cuts + 1] as u32)).collect();
        EXTRA_RUNGS.with(|e| *e.borrow_mut() = extra);
        let mut mo = m.clone();
        bridge_pair_opts(&mut mo, la.to_vec(), lb.to_vec(), cuts, shape, &mut None, mode, smooth, profile_shape);
        EXTRA_RUNGS.with(|e| e.borrow_mut().clear());
        let cutpos: HashMap<(u32, u32), Vec<V3>> = CUTPOS.with(|c| c.borrow().clone());
        for ring in &topo.rings {
            let (p, q) = norm(ring[0] as u32, ring[cuts + 1] as u32);
            let flipped = p != ring[0] as u32;
            let list = cutpos.get(&(p, q)).ok_or("bridge: no oracle positions for a rung")?;
            for i in 1..=cuts {
                let c = if flipped { list[cuts - i] } else { list[i - 1] };
                bm.verts[ring[i]].co = [c[0] as f32, c[1] as f32, c[2] as f32];
            }
        }
    }
    let (p, e, f) = bm.to_mesh();
    let mut out = Mesh::new();
    out.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    out.faces = f;
    Ok((out, e))
}

/// `bridge_two_loops` with Blender's vertex / face order (see `bridge_bm`): replaces `m` by the bridged mesh. Loops are closed vertex lists;
/// edges of `m` are taken in first-use order of its faces (pass the real Blender edge list through `bridge_bm` when order matters).
pub fn bridge_two_loops_bl(m: &mut Mesh, la: &[u32], lb: &[u32], cuts: usize, shape: f64) -> Result<(), String> {
    *m = bridge_bm(m, la, lb, cuts, shape, Interp::Path, 1.0, "SMOOTH", None)?;
    Ok(())
}
