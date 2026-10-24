//! Port of Bullet's btConvexHullComputer (LinearMath/btConvexHullComputer.cpp) as used by Blender's
//! Convex Hull node through intern/rigidbody/rb_convex_hull_api.cpp.
//!
//! NOTE: Blender's Bullet build uses btScalar == double; the reference data only matches with f64
//! arithmetic internally (f32 input widened, f32 output narrowed in plConvexHullGetVertex).
//! Shrink (shrink == 0 in Blender) is never reached and is omitted; so are Rational128/PointR128/Face.
//! Pointers are replaced by indices (NIL == NULL); the edge pool free list is replicated (LIFO reuse).

/// result of plConvexHullCompute
pub struct Hull {
    /// btConvexHullComputer::vertices
    pub vertices: Vec<[f32; 3]>,
    /// btConvexHullComputer::original_vertex_index
    pub original_vertex_index: Vec<i32>,
    /// btConvexHullComputer::edges, as (source_vertex, target_vertex, reverse_edge_index, next_edge_of_face_index)
    pub edges: Vec<HullEdge>,
    /// btConvexHullComputer::faces (index of one edge of each face)
    pub faces: Vec<i32>,
}

#[derive(Clone, Copy, Debug)]
pub struct HullEdge {
    pub source_vertex: i32,
    pub target_vertex: i32,
    pub reverse_edge: i32,
    pub next_edge_of_face: i32,
}

const NIL: usize = usize::MAX;

#[derive(Clone, Copy, Default, Debug)]
struct P64 {
    x: i64,
    y: i64,
    z: i64,
}

impl P64 {
    fn is_zero(&self) -> bool {
        self.x == 0 && self.y == 0 && self.z == 0
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct P32 {
    x: i32,
    y: i32,
    z: i32,
    index: i32,
}

impl P32 {
    fn new(x: i32, y: i32, z: i32) -> P32 {
        P32 { x, y, z, index: -1 }
    }
    fn eq3(&self, b: &P32) -> bool {
        self.x == b.x && self.y == b.y && self.z == b.z
    }
    // Point32::cross(Point32): int32 arithmetic, widened to int64
    fn cross32(&self, b: &P32) -> P64 {
        P64 {
            x: self.y.wrapping_mul(b.z).wrapping_sub(self.z.wrapping_mul(b.y)) as i64,
            y: self.z.wrapping_mul(b.x).wrapping_sub(self.x.wrapping_mul(b.z)) as i64,
            z: self.x.wrapping_mul(b.y).wrapping_sub(self.y.wrapping_mul(b.x)) as i64,
        }
    }
    // Point32::cross(Point64)
    fn cross64(&self, b: &P64) -> P64 {
        let (x, y, z) = (self.x as i64, self.y as i64, self.z as i64);
        P64 {
            x: y.wrapping_mul(b.z).wrapping_sub(z.wrapping_mul(b.y)),
            y: z.wrapping_mul(b.x).wrapping_sub(x.wrapping_mul(b.z)),
            z: x.wrapping_mul(b.y).wrapping_sub(y.wrapping_mul(b.x)),
        }
    }
    // Point32::dot(Point32): int32 arithmetic, widened
    fn dot32(&self, b: &P32) -> i64 {
        self.x
            .wrapping_mul(b.x)
            .wrapping_add(self.y.wrapping_mul(b.y))
            .wrapping_add(self.z.wrapping_mul(b.z)) as i64
    }
    fn dot64(&self, b: &P64) -> i64 {
        (self.x as i64)
            .wrapping_mul(b.x)
            .wrapping_add((self.y as i64).wrapping_mul(b.y))
            .wrapping_add((self.z as i64).wrapping_mul(b.z))
    }
    fn sub(&self, b: &P32) -> P32 {
        P32::new(self.x.wrapping_sub(b.x), self.y.wrapping_sub(b.y), self.z.wrapping_sub(b.z))
    }
}

#[inline]
fn dot_64_64(a: &P64, b: &P64) -> i64 {
    a.x.wrapping_mul(b.x).wrapping_add(a.y.wrapping_mul(b.y)).wrapping_add(a.z.wrapping_mul(b.z))
}
#[allow(dead_code)]
fn _unused(a: &P64) -> i64 {
    dot_64_64(a, a)
}

/// Rational64
#[derive(Clone, Copy, Debug)]
struct Rational64 {
    numerator: u64,
    denominator: u64,
    sign: i32,
}

impl Rational64 {
    fn new(numerator: i64, denominator: i64) -> Rational64 {
        let sign;
        let n;
        let mut s;
        if numerator > 0 {
            s = 1;
            n = numerator as u64;
        } else if numerator < 0 {
            s = -1;
            n = numerator.wrapping_neg() as u64;
        } else {
            s = 0;
            n = 0;
        }
        let d;
        if denominator > 0 {
            d = denominator as u64;
        } else if denominator < 0 {
            s = -s;
            d = denominator.wrapping_neg() as u64;
        } else {
            d = 0;
        }
        sign = s;
        Rational64 { numerator: n, denominator: d, sign }
    }
    fn is_negative_infinity(&self) -> bool {
        self.sign < 0 && self.denominator == 0
    }
    fn is_nan(&self) -> bool {
        self.sign == 0 && self.denominator == 0
    }
    fn compare(&self, b: &Rational64) -> i32 {
        if self.sign != b.sign {
            return self.sign - b.sign;
        } else if self.sign == 0 {
            return 0;
        }
        // Int128::mul(m_numerator, b.m_denominator).ucmp(Int128::mul(m_denominator, b.m_numerator))
        let l = (self.numerator as u128) * (b.denominator as u128);
        let r = (self.denominator as u128) * (b.numerator as u128);
        let u = if l < r {
            -1
        } else if l > r {
            1
        } else {
            0
        };
        self.sign * u
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    next: usize,
    prev: usize,
    edges: usize,
    point: P32,
    copy: i32,
}

#[derive(Clone, Copy)]
struct Edge {
    next: usize,
    prev: usize,
    reverse: usize,
    target: usize,
    copy: i32,
}

#[derive(Clone, Copy)]
struct IntermediateHull {
    min_xy: usize,
    max_xy: usize,
    min_yx: usize,
    max_yx: usize,
}

impl IntermediateHull {
    fn new() -> Self {
        IntermediateHull { min_xy: NIL, max_xy: NIL, min_yx: NIL, max_yx: NIL }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Orientation {
    None,
    Clockwise,
    CounterClockwise,
}

struct Internal {
    v: Vec<Vertex>,
    e: Vec<Edge>,
    free: Vec<usize>,
    merge_stamp: i32,
    scaling: [f64; 3],
    center: [f64; 3],
    med_axis: usize,
    max_axis: usize,
    min_axis: usize,
    vertex_list: usize,
}

fn max_axis3(f: &[f64; 3]) -> usize {
    if f[0] < f[1] {
        if f[1] < f[2] {
            2
        } else {
            1
        }
    } else if f[0] < f[2] {
        2
    } else {
        0
    }
}

fn min_axis3(f: &[f64; 3]) -> usize {
    if f[0] < f[1] {
        if f[0] < f[2] {
            0
        } else {
            2
        }
    } else if f[1] < f[2] {
        1
    } else {
        2
    }
}

// pointCmp
fn point_cmp(p: &P32, q: &P32) -> bool {
    p.y < q.y || (p.y == q.y && (p.x < q.x || (p.x == q.x && p.z < q.z)))
}

// btAlignedObjectArray::quickSortInternal
fn quick_sort_internal(a: &mut [P32], lo: i32, hi: i32) {
    let mut i = lo;
    let mut j = hi;
    let x = a[((lo + hi) / 2) as usize];
    loop {
        while point_cmp(&a[i as usize], &x) {
            i += 1;
        }
        while point_cmp(&x, &a[j as usize]) {
            j -= 1;
        }
        if i <= j {
            a.swap(i as usize, j as usize);
            i += 1;
            j -= 1;
        }
        if i > j {
            break;
        }
    }
    if lo < j {
        quick_sort_internal(a, lo, j);
    }
    if i < hi {
        quick_sort_internal(a, i, hi);
    }
}

impl Internal {
    #[inline]
    fn px(&self, v: usize) -> i32 {
        self.v[v].point.x
    }
    #[inline]
    fn py(&self, v: usize) -> i32 {
        self.v[v].point.y
    }
    #[inline]
    fn pt(&self, v: usize) -> P32 {
        self.v[v].point
    }

    // Edge::link
    fn link(&mut self, e: usize, n: usize) {
        self.e[e].next = n;
        self.e[n].prev = e;
    }

    fn new_edge_object(&mut self) -> usize {
        let blank = Edge { next: NIL, prev: NIL, reverse: NIL, target: NIL, copy: 0 };
        if let Some(i) = self.free.pop() {
            self.e[i] = blank;
            i
        } else {
            self.e.push(blank);
            self.e.len() - 1
        }
    }

    fn new_edge_pair(&mut self, from: usize, to: usize) -> usize {
        let e = self.new_edge_object();
        let r = self.new_edge_object();
        self.e[e].reverse = r;
        self.e[r].reverse = e;
        self.e[e].copy = self.merge_stamp;
        self.e[r].copy = self.merge_stamp;
        self.e[e].target = to;
        self.e[r].target = from;
        e
    }

    fn remove_edge_pair(&mut self, edge: usize) {
        let mut n = self.e[edge].next;
        let r = self.e[edge].reverse;
        if n != edge {
            let p = self.e[edge].prev;
            self.e[n].prev = p;
            self.e[p].next = n;
            let rt = self.e[r].target;
            self.v[rt].edges = n;
        } else {
            let rt = self.e[r].target;
            self.v[rt].edges = NIL;
        }
        n = self.e[r].next;
        if n != r {
            let p = self.e[r].prev;
            self.e[n].prev = p;
            self.e[p].next = n;
            let et = self.e[edge].target;
            self.v[et].edges = n;
        } else {
            let et = self.e[edge].target;
            self.v[et].edges = NIL;
        }
        // freeObject (LIFO)
        self.free.push(edge);
        self.free.push(r);
    }

    fn merge_projection(&mut self, h0: &mut IntermediateHull, h1: &mut IntermediateHull, c0: &mut usize, c1: &mut usize) -> bool {
        let mut v0 = h0.max_yx;
        let mut v1 = h1.min_yx;
        if self.px(v0) == self.px(v1) && self.py(v0) == self.py(v1) {
            let v1p = self.v[v1].prev;
            if v1p == v1 {
                *c0 = v0;
                if self.v[v1].edges != NIL {
                    v1 = self.e[self.v[v1].edges].target;
                }
                *c1 = v1;
                return false;
            }
            let v1n = self.v[v1].next;
            self.v[v1p].next = v1n;
            self.v[v1n].prev = v1p;
            if v1 == h1.min_xy {
                if self.px(v1n) < self.px(v1p) || (self.px(v1n) == self.px(v1p) && self.py(v1n) < self.py(v1p)) {
                    h1.min_xy = v1n;
                } else {
                    h1.min_xy = v1p;
                }
            }
            if v1 == h1.max_xy {
                if self.px(v1n) > self.px(v1p) || (self.px(v1n) == self.px(v1p) && self.py(v1n) > self.py(v1p)) {
                    h1.max_xy = v1n;
                } else {
                    h1.max_xy = v1p;
                }
            }
        }

        v0 = h0.max_xy;
        v1 = h1.max_xy;
        let mut v00 = NIL;
        let mut v10 = NIL;
        let mut sign: i32 = 1;

        for side in 0..=1 {
            let mut dx = self.px(v1).wrapping_sub(self.px(v0)).wrapping_mul(sign);
            if dx > 0 {
                loop {
                    let dy = self.py(v1).wrapping_sub(self.py(v0));

                    let w0 = if side != 0 { self.v[v0].next } else { self.v[v0].prev };
                    if w0 != v0 {
                        let dx0 = self.px(w0).wrapping_sub(self.px(v0)).wrapping_mul(sign);
                        let dy0 = self.py(w0).wrapping_sub(self.py(v0));
                        if dy0 <= 0 && (dx0 == 0 || (dx0 < 0 && dy0.wrapping_mul(dx) <= dy.wrapping_mul(dx0))) {
                            v0 = w0;
                            dx = self.px(v1).wrapping_sub(self.px(v0)).wrapping_mul(sign);
                            continue;
                        }
                    }

                    let w1 = if side != 0 { self.v[v1].next } else { self.v[v1].prev };
                    if w1 != v1 {
                        let dx1 = self.px(w1).wrapping_sub(self.px(v1)).wrapping_mul(sign);
                        let dy1 = self.py(w1).wrapping_sub(self.py(v1));
                        let dxn = self.px(w1).wrapping_sub(self.px(v0)).wrapping_mul(sign);
                        if dxn > 0 && dy1 < 0 && (dx1 == 0 || (dx1 < 0 && dy1.wrapping_mul(dx) < dy.wrapping_mul(dx1))) {
                            v1 = w1;
                            dx = dxn;
                            continue;
                        }
                    }
                    break;
                }
            } else if dx < 0 {
                loop {
                    let dy = self.py(v1).wrapping_sub(self.py(v0));

                    let w1 = if side != 0 { self.v[v1].prev } else { self.v[v1].next };
                    if w1 != v1 {
                        let dx1 = self.px(w1).wrapping_sub(self.px(v1)).wrapping_mul(sign);
                        let dy1 = self.py(w1).wrapping_sub(self.py(v1));
                        if dy1 >= 0 && (dx1 == 0 || (dx1 < 0 && dy1.wrapping_mul(dx) <= dy.wrapping_mul(dx1))) {
                            v1 = w1;
                            dx = self.px(v1).wrapping_sub(self.px(v0)).wrapping_mul(sign);
                            continue;
                        }
                    }

                    let w0 = if side != 0 { self.v[v0].prev } else { self.v[v0].next };
                    if w0 != v0 {
                        let dx0 = self.px(w0).wrapping_sub(self.px(v0)).wrapping_mul(sign);
                        let dy0 = self.py(w0).wrapping_sub(self.py(v0));
                        let dxn = self.px(v1).wrapping_sub(self.px(w0)).wrapping_mul(sign);
                        if dxn < 0 && dy0 > 0 && (dx0 == 0 || (dx0 < 0 && dy0.wrapping_mul(dx) < dy.wrapping_mul(dx0))) {
                            v0 = w0;
                            dx = dxn;
                            continue;
                        }
                    }
                    break;
                }
            } else {
                let x = self.px(v0);
                let mut y0 = self.py(v0);
                let mut w0 = v0;
                loop {
                    let t = if side != 0 { self.v[w0].next } else { self.v[w0].prev };
                    if t != v0 && self.px(t) == x && self.py(t) <= y0 {
                        w0 = t;
                        y0 = self.py(t);
                    } else {
                        break;
                    }
                }
                v0 = w0;

                let mut y1 = self.py(v1);
                let mut w1 = v1;
                loop {
                    let t = if side != 0 { self.v[w1].prev } else { self.v[w1].next };
                    if t != v1 && self.px(t) == x && self.py(t) >= y1 {
                        w1 = t;
                        y1 = self.py(t);
                    } else {
                        break;
                    }
                }
                v1 = w1;
            }

            if side == 0 {
                v00 = v0;
                v10 = v1;
                v0 = h0.min_xy;
                v1 = h1.min_xy;
                sign = -1;
            }
        }

        self.v[v0].prev = v1;
        self.v[v1].next = v0;

        self.v[v00].next = v10;
        self.v[v10].prev = v00;

        if self.px(h1.min_xy) < self.px(h0.min_xy) {
            h0.min_xy = h1.min_xy;
        }
        if self.px(h1.max_xy) >= self.px(h0.max_xy) {
            h0.max_xy = h1.max_xy;
        }
        h0.max_yx = h1.max_yx;

        *c0 = v00;
        *c1 = v10;
        true
    }

    fn compute_internal(&mut self, ov: &[usize], start: i32, end: i32, result: &mut IntermediateHull) {
        let n = end - start;
        match n {
            0 => {
                *result = IntermediateHull::new();
                return;
            }
            2 => {
                let mut v = ov[start as usize];
                let mut w = v + 1;
                if !self.pt(v).eq3(&self.pt(w)) {
                    let dx = self.px(v).wrapping_sub(self.px(w));
                    let dy = self.py(v).wrapping_sub(self.py(w));

                    if dx == 0 && dy == 0 {
                        if self.pt(v).z > self.pt(w).z {
                            std::mem::swap(&mut v, &mut w);
                        }
                        self.v[v].next = v;
                        self.v[v].prev = v;
                        result.min_xy = v;
                        result.max_xy = v;
                        result.min_yx = v;
                        result.max_yx = v;
                    } else {
                        self.v[v].next = w;
                        self.v[v].prev = w;
                        self.v[w].next = v;
                        self.v[w].prev = v;

                        if dx < 0 || (dx == 0 && dy < 0) {
                            result.min_xy = v;
                            result.max_xy = w;
                        } else {
                            result.min_xy = w;
                            result.max_xy = v;
                        }

                        if dy < 0 || (dy == 0 && dx < 0) {
                            result.min_yx = v;
                            result.max_yx = w;
                        } else {
                            result.min_yx = w;
                            result.max_yx = v;
                        }
                    }

                    let mut e = self.new_edge_pair(v, w);
                    self.link(e, e);
                    self.v[v].edges = e;

                    e = self.e[e].reverse;
                    self.link(e, e);
                    self.v[w].edges = e;
                    return;
                }
                {
                    let v = ov[start as usize];
                    self.v[v].edges = NIL;
                    self.v[v].next = v;
                    self.v[v].prev = v;
                    result.min_xy = v;
                    result.max_xy = v;
                    result.min_yx = v;
                    result.max_yx = v;
                }
                return;
            }
            1 => {
                let v = ov[start as usize];
                self.v[v].edges = NIL;
                self.v[v].next = v;
                self.v[v].prev = v;
                result.min_xy = v;
                result.max_xy = v;
                result.min_yx = v;
                result.max_yx = v;
                return;
            }
            _ => {}
        }

        let split0 = start + n / 2;
        let p = self.pt(ov[(split0 - 1) as usize]);
        let mut split1 = split0;
        while split1 < end && self.pt(ov[split1 as usize]).eq3(&p) {
            split1 += 1;
        }
        self.compute_internal(ov, start, split0, result);
        let mut hull1 = IntermediateHull::new();
        self.compute_internal(ov, split1, end, &mut hull1);
        self.merge(result, &mut hull1);
    }

    fn get_orientation(&self, prev: usize, next: usize, s: &P32, t: &P32) -> Orientation {
        if self.e[prev].next == next {
            if self.e[prev].prev == next {
                let n = t.cross32(s);
                let nrt = self.e[self.e[next].reverse].target;
                let m = self
                    .pt(self.e[prev].target)
                    .sub(&self.pt(nrt))
                    .cross32(&self.pt(self.e[next].target).sub(&self.pt(nrt)));
                let _ = m.is_zero();
                let dot = dot_64_64(&n, &m);
                return if dot > 0 { Orientation::CounterClockwise } else { Orientation::Clockwise };
            }
            return Orientation::CounterClockwise;
        } else if self.e[prev].prev == next {
            return Orientation::Clockwise;
        }
        Orientation::None
    }

    fn find_max_angle(&self, ccw: bool, start: usize, s: &P32, rxs: &P64, sxrxs: &P64, min_cot: &mut Rational64) -> usize {
        let mut min_edge = NIL;
        let first = self.v[start].edges;
        let mut e = first;
        if e != NIL {
            loop {
                if self.e[e].copy > self.merge_stamp {
                    let t = self.pt(self.e[e].target).sub(&self.pt(start));
                    let cot = Rational64::new(t.dot64(sxrxs), t.dot64(rxs));
                    if cot.is_nan() {
                        // assert only
                    } else {
                        let cmp;
                        if min_edge == NIL {
                            *min_cot = cot;
                            min_edge = e;
                        } else {
                            cmp = cot.compare(min_cot);
                            if cmp < 0 {
                                *min_cot = cot;
                                min_edge = e;
                            } else if cmp == 0 && (ccw == (self.get_orientation(min_edge, e, s, &t) == Orientation::CounterClockwise)) {
                                min_edge = e;
                            }
                        }
                    }
                }
                e = self.e[e].next;
                if e == first {
                    break;
                }
            }
        }
        min_edge
    }

    fn find_edge_for_coplanar_faces(&self, c0: usize, c1: usize, e0: &mut usize, e1: &mut usize, stop0: usize, stop1: usize) {
        let start0 = *e0;
        let start1 = *e1;
        let mut et0 = if start0 != NIL { self.pt(self.e[start0].target) } else { self.pt(c0) };
        let mut et1 = if start1 != NIL { self.pt(self.e[start1].target) } else { self.pt(c1) };
        let s = self.pt(c1).sub(&self.pt(c0));
        let ref_e = if start0 != NIL { start0 } else { start1 };
        let normal = self.pt(self.e[ref_e].target).sub(&self.pt(c0)).cross32(&s);
        let dist = self.pt(c0).dot64(&normal);
        let perp = s.cross64(&normal);

        let mut max_dot0 = et0.dot64(&perp);
        if *e0 != NIL {
            while self.e[*e0].target != stop0 {
                let e = self.e[self.e[*e0].reverse].prev;
                if self.pt(self.e[e].target).dot64(&normal) < dist {
                    break;
                }
                if self.e[e].copy == self.merge_stamp {
                    break;
                }
                let dot = self.pt(self.e[e].target).dot64(&perp);
                if dot <= max_dot0 {
                    break;
                }
                max_dot0 = dot;
                *e0 = e;
                et0 = self.pt(self.e[e].target);
            }
        }

        let mut max_dot1 = et1.dot64(&perp);
        if *e1 != NIL {
            while self.e[*e1].target != stop1 {
                let e = self.e[self.e[*e1].reverse].next;
                if self.pt(self.e[e].target).dot64(&normal) < dist {
                    break;
                }
                if self.e[e].copy == self.merge_stamp {
                    break;
                }
                let dot = self.pt(self.e[e].target).dot64(&perp);
                if dot <= max_dot1 {
                    break;
                }
                max_dot1 = dot;
                *e1 = e;
                et1 = self.pt(self.e[e].target);
            }
        }

        let mut dx = max_dot1.wrapping_sub(max_dot0);
        if dx > 0 {
            loop {
                let dy = et1.sub(&et0).dot32(&s);

                if *e0 != NIL && self.e[*e0].target != stop0 {
                    let f0 = self.e[self.e[*e0].next].reverse;
                    if self.e[f0].copy > self.merge_stamp {
                        let d = self.pt(self.e[f0].target).sub(&et0);
                        let dx0 = d.dot64(&perp);
                        let dy0 = d.dot32(&s);
                        if if dx0 == 0 {
                            dy0 < 0
                        } else {
                            dx0 < 0 && Rational64::new(dy0, dx0).compare(&Rational64::new(dy, dx)) >= 0
                        } {
                            et0 = self.pt(self.e[f0].target);
                            dx = et1.sub(&et0).dot64(&perp);
                            *e0 = if *e0 == start0 { NIL } else { f0 };
                            continue;
                        }
                    }
                }

                if *e1 != NIL && self.e[*e1].target != stop1 {
                    let f1 = self.e[self.e[*e1].reverse].next;
                    if self.e[f1].copy > self.merge_stamp {
                        let d1 = self.pt(self.e[f1].target).sub(&et1);
                        if d1.dot64(&normal) == 0 {
                            let dx1 = d1.dot64(&perp);
                            let dy1 = d1.dot32(&s);
                            let dxn = self.pt(self.e[f1].target).sub(&et0).dot64(&perp);
                            if dxn > 0
                                && (if dx1 == 0 {
                                    dy1 < 0
                                } else {
                                    dx1 < 0 && Rational64::new(dy1, dx1).compare(&Rational64::new(dy, dx)) > 0
                                })
                            {
                                *e1 = f1;
                                et1 = self.pt(self.e[*e1].target);
                                dx = dxn;
                                continue;
                            }
                        }
                    }
                }
                break;
            }
        } else if dx < 0 {
            loop {
                let dy = et1.sub(&et0).dot32(&s);

                if *e1 != NIL && self.e[*e1].target != stop1 {
                    let f1 = self.e[self.e[*e1].prev].reverse;
                    if self.e[f1].copy > self.merge_stamp {
                        let d = self.pt(self.e[f1].target).sub(&et1);
                        let dx1 = d.dot64(&perp);
                        let dy1 = d.dot32(&s);
                        if if dx1 == 0 {
                            dy1 > 0
                        } else {
                            dx1 < 0 && Rational64::new(dy1, dx1).compare(&Rational64::new(dy, dx)) <= 0
                        } {
                            et1 = self.pt(self.e[f1].target);
                            dx = et1.sub(&et0).dot64(&perp);
                            *e1 = if *e1 == start1 { NIL } else { f1 };
                            continue;
                        }
                    }
                }

                if *e0 != NIL && self.e[*e0].target != stop0 {
                    let f0 = self.e[self.e[*e0].reverse].prev;
                    if self.e[f0].copy > self.merge_stamp {
                        let d0 = self.pt(self.e[f0].target).sub(&et0);
                        if d0.dot64(&normal) == 0 {
                            let dx0 = d0.dot64(&perp);
                            let dy0 = d0.dot32(&s);
                            let dxn = et1.sub(&self.pt(self.e[f0].target)).dot64(&perp);
                            if dxn < 0
                                && (if dx0 == 0 {
                                    dy0 > 0
                                } else {
                                    dx0 < 0 && Rational64::new(dy0, dx0).compare(&Rational64::new(dy, dx)) < 0
                                })
                            {
                                *e0 = f0;
                                et0 = self.pt(self.e[*e0].target);
                                dx = dxn;
                                continue;
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    fn merge(&mut self, h0: &mut IntermediateHull, h1: &mut IntermediateHull) {
        if h1.max_xy == NIL {
            return;
        }
        if h0.max_xy == NIL {
            *h0 = *h1;
            return;
        }

        self.merge_stamp -= 1;

        let mut c0 = NIL;
        let mut to_prev0 = NIL;
        let mut first_new0 = NIL;
        let mut pending_head0 = NIL;
        let mut pending_tail0 = NIL;
        let mut c1 = NIL;
        let mut to_prev1 = NIL;
        let mut first_new1 = NIL;
        let mut pending_head1 = NIL;
        let mut pending_tail1 = NIL;
        let mut prev_point;

        if self.merge_projection(h0, h1, &mut c0, &mut c1) {
            let s = self.pt(c1).sub(&self.pt(c0));
            let normal = P32::new(0, 0, -1).cross32(&s);
            let t = s.cross64(&normal);

            let mut e = self.v[c0].edges;
            let mut start0 = NIL;
            if e != NIL {
                let first = e;
                loop {
                    let d = self.pt(self.e[e].target).sub(&self.pt(c0));
                    let dot = d.dot64(&normal);
                    if dot == 0 && d.dot64(&t) > 0 {
                        if start0 == NIL || self.get_orientation(start0, e, &s, &P32::new(0, 0, -1)) == Orientation::Clockwise {
                            start0 = e;
                        }
                    }
                    e = self.e[e].next;
                    if e == first {
                        break;
                    }
                }
            }

            e = self.v[c1].edges;
            let mut start1 = NIL;
            if e != NIL {
                let first = e;
                loop {
                    let d = self.pt(self.e[e].target).sub(&self.pt(c1));
                    let dot = d.dot64(&normal);
                    if dot == 0 && d.dot64(&t) > 0 {
                        if start1 == NIL || self.get_orientation(start1, e, &s, &P32::new(0, 0, -1)) == Orientation::CounterClockwise {
                            start1 = e;
                        }
                    }
                    e = self.e[e].next;
                    if e == first {
                        break;
                    }
                }
            }

            if start0 != NIL || start1 != NIL {
                self.find_edge_for_coplanar_faces(c0, c1, &mut start0, &mut start1, NIL, NIL);
                if start0 != NIL {
                    c0 = self.e[start0].target;
                }
                if start1 != NIL {
                    c1 = self.e[start1].target;
                }
            }

            prev_point = self.pt(c1);
            prev_point.z = prev_point.z.wrapping_add(1);
        } else {
            prev_point = self.pt(c1);
            prev_point.x = prev_point.x.wrapping_add(1);
        }

        let first0 = c0;
        let first1 = c1;
        let mut first_run = true;

        loop {
            let s = self.pt(c1).sub(&self.pt(c0));
            let r = prev_point.sub(&self.pt(c0));
            let rxs = r.cross32(&s);
            let sxrxs = s.cross64(&rxs);

            let mut min_cot0 = Rational64::new(0, 0);
            let min0 = self.find_max_angle(false, c0, &s, &rxs, &sxrxs, &mut min_cot0);
            let mut min_cot1 = Rational64::new(0, 0);
            let min1 = self.find_max_angle(true, c1, &s, &rxs, &sxrxs, &mut min_cot1);
            if min0 == NIL && min1 == NIL {
                let mut e = self.new_edge_pair(c0, c1);
                self.link(e, e);
                self.v[c0].edges = e;

                e = self.e[e].reverse;
                self.link(e, e);
                self.v[c1].edges = e;
                return;
            } else {
                let cmp: i32 = if min0 == NIL {
                    1
                } else if min1 == NIL {
                    -1
                } else {
                    min_cot0.compare(&min_cot1)
                };
                if first_run
                    || (if cmp >= 0 {
                        !min_cot1.is_negative_infinity()
                    } else {
                        !min_cot0.is_negative_infinity()
                    })
                {
                    let mut e = self.new_edge_pair(c0, c1);
                    if pending_tail0 != NIL {
                        self.e[pending_tail0].prev = e;
                    } else {
                        pending_head0 = e;
                    }
                    self.e[e].next = pending_tail0;
                    pending_tail0 = e;

                    e = self.e[e].reverse;
                    if pending_tail1 != NIL {
                        self.e[pending_tail1].next = e;
                    } else {
                        pending_head1 = e;
                    }
                    self.e[e].prev = pending_tail1;
                    pending_tail1 = e;
                }

                let mut e0 = min0;
                let mut e1 = min1;

                if cmp == 0 {
                    self.find_edge_for_coplanar_faces(c0, c1, &mut e0, &mut e1, NIL, NIL);
                }

                if cmp >= 0 && e1 != NIL {
                    if to_prev1 != NIL {
                        let mut e = self.e[to_prev1].next;
                        while e != min1 {
                            let n = self.e[e].next;
                            self.remove_edge_pair(e);
                            e = n;
                        }
                    }

                    if pending_tail1 != NIL {
                        if to_prev1 != NIL {
                            self.link(to_prev1, pending_head1);
                        } else {
                            let mp = self.e[min1].prev;
                            self.link(mp, pending_head1);
                            first_new1 = pending_head1;
                        }
                        self.link(pending_tail1, min1);
                        pending_head1 = NIL;
                        pending_tail1 = NIL;
                    } else if to_prev1 == NIL {
                        first_new1 = min1;
                    }

                    prev_point = self.pt(c1);
                    c1 = self.e[e1].target;
                    to_prev1 = self.e[e1].reverse;
                }

                if cmp <= 0 && e0 != NIL {
                    if to_prev0 != NIL {
                        let mut e = self.e[to_prev0].prev;
                        while e != min0 {
                            let n = self.e[e].prev;
                            self.remove_edge_pair(e);
                            e = n;
                        }
                    }

                    if pending_tail0 != NIL {
                        if to_prev0 != NIL {
                            self.link(pending_head0, to_prev0);
                        } else {
                            let mn = self.e[min0].next;
                            self.link(pending_head0, mn);
                            first_new0 = pending_head0;
                        }
                        self.link(min0, pending_tail0);
                        pending_head0 = NIL;
                        pending_tail0 = NIL;
                    } else if to_prev0 == NIL {
                        first_new0 = min0;
                    }

                    prev_point = self.pt(c0);
                    c0 = self.e[e0].target;
                    to_prev0 = self.e[e0].reverse;
                }
            }

            if c0 == first0 && c1 == first1 {
                if to_prev0 == NIL {
                    self.link(pending_head0, pending_tail0);
                    self.v[c0].edges = pending_tail0;
                } else {
                    let mut e = self.e[to_prev0].prev;
                    while e != first_new0 {
                        let n = self.e[e].prev;
                        self.remove_edge_pair(e);
                        e = n;
                    }
                    if pending_tail0 != NIL {
                        self.link(pending_head0, to_prev0);
                        self.link(first_new0, pending_tail0);
                    }
                }

                if to_prev1 == NIL {
                    self.link(pending_tail1, pending_head1);
                    self.v[c1].edges = pending_tail1;
                } else {
                    let mut e = self.e[to_prev1].next;
                    while e != first_new1 {
                        let n = self.e[e].next;
                        self.remove_edge_pair(e);
                        e = n;
                    }
                    if pending_tail1 != NIL {
                        self.link(to_prev1, pending_head1);
                        self.link(pending_tail1, first_new1);
                    }
                }
                return;
            }

            first_run = false;
        }
    }

    fn get_coordinates(&self, v: usize) -> [f32; 3] {
        let pt = self.v[v].point;
        // index >= 0 always (no shrink): btScalar(point.x) ...
        let mut p = [0f64; 3];
        p[self.med_axis] = pt.x as f64;
        p[self.max_axis] = pt.y as f64;
        p[self.min_axis] = pt.z as f64;
        // p * scaling + center, narrowed to float by plConvexHullGetVertex
        let r = [p[0] * self.scaling[0] + self.center[0], p[1] * self.scaling[1] + self.center[1], p[2] * self.scaling[2] + self.center[2]];
        [r[0] as f32, r[1] as f32, r[2] as f32]
    }
}

fn internal_compute(coords: &[[f32; 3]]) -> Internal {
    // NOTE: Blender's Bullet build uses btScalar == double (BT_USE_DOUBLE_PRECISION); the reference
    // data only matches with double arithmetic here.  Float coords are widened to double.
    let count = coords.len();
    let mut min = [1e30f64; 3];
    let mut max = [-1e30f64; 3];
    for v in coords {
        for k in 0..3 {
            let x = v[k] as f64;
            if x < min[k] {
                min[k] = x;
            }
            if max[k] < x {
                max[k] = x;
            }
        }
    }
    let mut s = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let max_axis = max_axis3(&s);
    let mut min_axis = min_axis3(&s);
    if min_axis == max_axis {
        min_axis = (max_axis + 1) % 3;
    }
    let med_axis = 3 - max_axis - min_axis;

    // s /= 10216  ==>  s *= 1/10216
    let inv = 1.0f64 / 10216.0f64;
    for k in 0..3 {
        s[k] *= inv;
    }
    if (med_axis + 1) % 3 != max_axis {
        for k in 0..3 {
            s[k] *= -1.0f64;
        }
    }
    let scaling = s;
    for k in 0..3 {
        if s[k] != 0.0 {
            s[k] = 1.0f64 / s[k];
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5f64,
        (min[1] + max[1]) * 0.5f64,
        (min[2] + max[2]) * 0.5f64,
    ];

    let mut points: Vec<P32> = Vec::with_capacity(count);
    for (i, v) in coords.iter().enumerate() {
        let p = [
            (v[0] as f64 - center[0]) * s[0],
            (v[1] as f64 - center[1]) * s[1],
            (v[2] as f64 - center[2]) * s[2],
        ];
        points.push(P32 { x: p[med_axis] as i32, y: p[max_axis] as i32, z: p[min_axis] as i32, index: i as i32 });
    }
    if points.len() > 1 {
        let n = points.len() as i32;
        quick_sort_internal(&mut points, 0, n - 1);
    }

    let mut this = Internal {
        v: Vec::with_capacity(count),
        e: Vec::with_capacity(6 * count),
        free: Vec::new(),
        merge_stamp: -3,
        scaling,
        center,
        med_axis,
        max_axis,
        min_axis,
        vertex_list: NIL,
    };
    let mut original_vertices = Vec::with_capacity(count);
    for (i, p) in points.iter().enumerate() {
        this.v.push(Vertex { next: NIL, prev: NIL, edges: NIL, point: *p, copy: -1 });
        original_vertices.push(i);
    }

    let mut hull = IntermediateHull::new();
    this.compute_internal(&original_vertices, 0, count as i32, &mut hull);
    this.vertex_list = hull.min_xy;
    this
}

fn get_vertex_copy(vertex: usize, v: &mut Vec<Vertex>, vertices: &mut Vec<usize>) -> i32 {
    let mut index = v[vertex].copy;
    if index < 0 {
        index = vertices.len() as i32;
        v[vertex].copy = index;
        vertices.push(vertex);
    }
    index
}

/// plConvexHullCompute(coords, count): btConvexHullComputer::compute(coords, 12, count, 0, 0)
pub fn convex_hull_compute(coords: &[[f32; 3]]) -> Hull {
    let mut out = Hull { vertices: vec![], original_vertex_index: vec![], edges: vec![], faces: vec![] };
    if coords.is_empty() {
        return out;
    }
    let mut hull = internal_compute(coords);

    // relative-offset edges as in btConvexHullComputer::Edge
    let mut rnext: Vec<i32> = vec![];
    let mut rrev: Vec<i32> = vec![];
    let mut rtarget: Vec<i32> = vec![];

    let mut old_vertices: Vec<usize> = vec![];
    let vl = hull.vertex_list;
    get_vertex_copy(vl, &mut hull.v, &mut old_vertices);
    let mut copied = 0usize;
    while copied < old_vertices.len() {
        let v = old_vertices[copied];
        out.vertices.push(hull.get_coordinates(v));
        out.original_vertex_index.push(hull.v[v].point.index);
        let first_edge = hull.v[v].edges;
        if first_edge != NIL {
            let mut first_copy: i32 = -1;
            let mut prev_copy: i32 = -1;
            let mut e = first_edge;
            loop {
                if hull.e[e].copy < 0 {
                    let s = rnext.len() as i32;
                    rnext.push(0);
                    rnext.push(0);
                    rrev.push(1);
                    rrev.push(-1);
                    hull.e[e].copy = s;
                    let er = hull.e[e].reverse;
                    hull.e[er].copy = s + 1;
                    let et = hull.e[e].target;
                    let tv = get_vertex_copy(et, &mut hull.v, &mut old_vertices);
                    rtarget.push(tv);
                    rtarget.push(copied as i32);
                }
                let ec = hull.e[e].copy;
                if prev_copy >= 0 {
                    rnext[ec as usize] = prev_copy - ec;
                } else {
                    first_copy = ec;
                }
                prev_copy = ec;
                e = hull.e[e].next;
                if e == first_edge {
                    break;
                }
            }
            rnext[first_copy as usize] = prev_copy - first_copy;
        }
        copied += 1;
    }

    for i in 0..copied {
        let v = old_vertices[i];
        let first_edge = hull.v[v].edges;
        if first_edge != NIL {
            let mut e = first_edge;
            loop {
                if hull.e[e].copy >= 0 {
                    out.faces.push(hull.e[e].copy);
                    let mut f = e;
                    loop {
                        hull.e[f].copy = -1;
                        let fr = hull.e[f].reverse;
                        f = hull.e[fr].prev;
                        if f == e {
                            break;
                        }
                    }
                }
                e = hull.e[e].next;
                if e == first_edge {
                    break;
                }
            }
        }
    }

    for i in 0..rnext.len() {
        let rev = i as i32 + rrev[i];
        let src = rtarget[rev as usize];
        // getNextEdgeOfFace: (this+reverse)->getNextEdgeOfVertex() = rev + next[rev]
        let nf = rev + rnext[rev as usize];
        out.edges.push(HullEdge { source_vertex: src, target_vertex: rtarget[i], reverse_edge: rev, next_edge_of_face: nf });
    }
    out
}
