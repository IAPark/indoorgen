//! Port of Blender's `delaunay_2d.cc` (constrained Delaunay triangulation, double precision, `CDT_INSIDE_WITH_HOLES` path, no ids)
//! with exact predicates, as used by the Fill Curve node. Vertex / edge / face creation order is kept so that the output order
//! matches Blender.
const NONE: usize = usize::MAX;

// ---------------------------------------------------------------- exact predicates (expansion arithmetic)
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bv = x - a;
    let av = x - bv;
    (x, (a - av) + (b - bv))
}
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}
type Exp = Vec<f64>;
fn grow(e: &Exp, b: f64) -> Exp {
    let mut q = b;
    let mut h = Vec::with_capacity(e.len() + 1);
    for &x in e {
        let (s, t) = two_sum(q, x);
        q = s;
        if t != 0.0 {
            h.push(t);
        }
    }
    if q != 0.0 || h.is_empty() {
        h.push(q);
    }
    h
}
fn exp_add(a: &Exp, b: &Exp) -> Exp {
    let mut r = a.clone();
    for &x in b {
        r = grow(&r, x);
    }
    r
}
fn exp_neg(a: &Exp) -> Exp {
    a.iter().map(|x| -x).collect()
}
fn exp_scale(e: &Exp, b: f64) -> Exp {
    let mut r: Exp = vec![0.0];
    for &x in e {
        let (p, q) = two_prod(x, b);
        r = grow(&r, q);
        r = grow(&r, p);
    }
    r
}
fn exp_mul(a: &Exp, b: &Exp) -> Exp {
    let mut r: Exp = vec![0.0];
    for &x in b {
        r = exp_add(&r, &exp_scale(a, x));
    }
    r
}
fn exp_sign(e: &Exp) -> i32 {
    for &x in e.iter().rev() {
        if x > 0.0 {
            return 1;
        }
        if x < 0.0 {
            return -1;
        }
    }
    0
}
fn two_diff_exp(a: f64, b: f64) -> Exp {
    let (x, y) = two_sum(a, -b);
    if y != 0.0 {
        vec![y, x]
    } else {
        vec![x]
    }
}

const EPS: f64 = 1.1102230246251565e-16;

pub fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> i32 {
    let detleft = (a[0] - c[0]) * (b[1] - c[1]);
    let detright = (a[1] - c[1]) * (b[0] - c[0]);
    let det = detleft - detright;
    let detsum;
    if detleft > 0.0 {
        if detright <= 0.0 {
            return sgn(det);
        }
        detsum = detleft + detright;
    } else if detleft < 0.0 {
        if detright >= 0.0 {
            return sgn(det);
        }
        detsum = -detleft - detright;
    } else {
        return sgn(det);
    }
    let errbound = (3.0 + 16.0 * EPS) * EPS * detsum;
    if det >= errbound || -det >= errbound {
        return sgn(det);
    }
    let acx = two_diff_exp(a[0], c[0]);
    let bcx = two_diff_exp(b[0], c[0]);
    let acy = two_diff_exp(a[1], c[1]);
    let bcy = two_diff_exp(b[1], c[1]);
    let d = exp_add(&exp_mul(&acx, &bcy), &exp_neg(&exp_mul(&acy, &bcx)));
    exp_sign(&d)
}

fn sgn(x: f64) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

pub fn incircle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> i32 {
    let adx = a[0] - d[0];
    let bdx = b[0] - d[0];
    let cdx = c[0] - d[0];
    let ady = a[1] - d[1];
    let bdy = b[1] - d[1];
    let cdy = c[1] - d[1];
    let bdxcdy = bdx * cdy;
    let cdxbdy = cdx * bdy;
    let alift = adx * adx + ady * ady;
    let cdxady = cdx * ady;
    let adxcdy = adx * cdy;
    let blift = bdx * bdx + bdy * bdy;
    let adxbdy = adx * bdy;
    let bdxady = bdx * ady;
    let clift = cdx * cdx + cdy * cdy;
    let det = alift * (bdxcdy - cdxbdy) + blift * (cdxady - adxcdy) + clift * (adxbdy - bdxady);
    let permanent = (bdxcdy.abs() + cdxbdy.abs()) * alift + (cdxady.abs() + adxcdy.abs()) * blift + (adxbdy.abs() + bdxady.abs()) * clift;
    let errbound = (10.0 + 96.0 * EPS) * EPS * permanent;
    if det > errbound || -det > errbound {
        return sgn(det);
    }
    let adx = two_diff_exp(a[0], d[0]);
    let bdx = two_diff_exp(b[0], d[0]);
    let cdx = two_diff_exp(c[0], d[0]);
    let ady = two_diff_exp(a[1], d[1]);
    let bdy = two_diff_exp(b[1], d[1]);
    let cdy = two_diff_exp(c[1], d[1]);
    let alift = exp_add(&exp_mul(&adx, &adx), &exp_mul(&ady, &ady));
    let blift = exp_add(&exp_mul(&bdx, &bdx), &exp_mul(&bdy, &bdy));
    let clift = exp_add(&exp_mul(&cdx, &cdx), &exp_mul(&cdy, &cdy));
    let t1 = exp_add(&exp_mul(&bdx, &cdy), &exp_neg(&exp_mul(&cdx, &bdy)));
    let t2 = exp_add(&exp_mul(&cdx, &ady), &exp_neg(&exp_mul(&adx, &cdy)));
    let t3 = exp_add(&exp_mul(&adx, &bdy), &exp_neg(&exp_mul(&bdx, &ady)));
    let det = exp_add(&exp_add(&exp_mul(&alift, &t1), &exp_mul(&blift, &t2)), &exp_mul(&clift, &t3));
    exp_sign(&det)
}

// ---------------------------------------------------------------- structures
#[derive(Clone, Copy)]
struct SE {
    next: usize,
    rot: usize,
    vert: usize,
    edge: usize,
    face: usize,
}
struct Vert {
    co: [f64; 2],
    symedge: usize,
    index: usize,
    merge_to: i64,
    visit: i64,
}
struct Edge {
    constrained: bool,
}
struct Face {
    symedge: usize,
    visit: i64,
    deleted: bool,
    hole: bool,
}

struct Cdt {
    verts: Vec<Vert>,
    edges: Vec<Edge>,
    se: Vec<SE>,
    faces: Vec<Face>,
    outer: usize,
    visit_count: i64,
}

fn sym_of(c: &Cdt, s: usize) -> usize {
    c.se[c.se[s].next].rot
}
fn prev_of(c: &Cdt, s: usize) -> usize {
    c.se[c.se[c.se[s].rot].next].rot
}

impl Cdt {
    fn sym(&self, s: usize) -> usize {
        sym_of(self, s)
    }
    fn prev(&self, s: usize) -> usize {
        prev_of(self, s)
    }
    fn add_vert(&mut self, co: [f64; 2]) -> usize {
        let i = self.verts.len();
        self.verts.push(Vert { co, symedge: NONE, index: i, merge_to: -1, visit: 0 });
        i
    }
    fn add_edge(&mut self, v1: usize, v2: usize, fleft: usize, fright: usize) -> usize {
        let e = self.edges.len();
        self.edges.push(Edge { constrained: false });
        let blank = SE { next: NONE, rot: NONE, vert: 0, edge: e, face: 0 };
        self.se.push(SE { vert: v1, face: fleft, ..blank });
        self.se.push(SE { vert: v2, face: fright, ..blank });
        if self.verts[v1].symedge == NONE {
            self.verts[v1].symedge = 2 * e;
        }
        if self.verts[v2].symedge == NONE {
            self.verts[v2].symedge = 2 * e + 1;
        }
        e
    }
    fn add_face(&mut self) -> usize {
        self.faces.push(Face { symedge: NONE, visit: 0, deleted: false, hole: false });
        self.faces.len() - 1
    }
    fn add_diagonal(&mut self, s1: usize, s2: usize) -> usize {
        let fold = self.se[s1].face;
        let fnew = self.add_face();
        let s1prev = self.prev(s1);
        let s1prevsym = self.sym(s1prev);
        let s2prev = self.prev(s2);
        let s2prevsym = self.sym(s2prev);
        let (v1, v2) = (self.se[s1].vert, self.se[s2].vert);
        let ediag = self.add_edge(v1, v2, fnew, fold);
        let (sdiag, sdiagsym) = (2 * ediag, 2 * ediag + 1);
        self.se[sdiag].next = s2;
        self.se[sdiagsym].next = s1;
        self.se[s2prev].next = sdiagsym;
        self.se[s1prev].next = sdiag;
        self.se[s1].rot = sdiag;
        self.se[sdiag].rot = s1prevsym;
        self.se[s2].rot = sdiagsym;
        self.se[sdiagsym].rot = s2prevsym;
        let mut s = s2;
        while s != sdiag {
            self.se[s].face = fnew;
            s = self.se[s].next;
        }
        ediag
    }
    fn add_vert_to_symedge_edge(&mut self, v: usize, se: usize) -> usize {
        let se_rot = self.se[se].rot;
        let se_rotsym = self.sym(se_rot);
        let sv = self.se[se].vert;
        let sf = self.se[se].face;
        let e = self.add_edge(v, sv, sf, sf);
        let (nse, nsesym) = (2 * e, 2 * e + 1);
        self.se[nse].next = se;
        self.se[nsesym].next = nse;
        self.se[nse].rot = nse;
        self.se[nsesym].rot = se_rot;
        self.se[se].rot = nsesym;
        self.se[se_rotsym].next = nsesym;
        e
    }
    fn connect_separate_parts(&mut self, se1: usize, se2: usize) -> usize {
        let se1_rot = self.se[se1].rot;
        let se1_rotsym = self.sym(se1_rot);
        let se2_rot = self.se[se2].rot;
        let se2_rotsym = self.sym(se2_rot);
        let (v1, v2, o) = (self.se[se1].vert, self.se[se2].vert, self.outer);
        let e = self.add_edge(v1, v2, o, o);
        let (nse, nsesym) = (2 * e, 2 * e + 1);
        self.se[nse].next = se2;
        self.se[nsesym].next = se1;
        self.se[nse].rot = se1_rot;
        self.se[nsesym].rot = se2_rot;
        self.se[se1].rot = nse;
        self.se[se2].rot = nsesym;
        self.se[se1_rotsym].next = nse;
        self.se[se2_rotsym].next = nsesym;
        e
    }
    fn split_edge(&mut self, se: usize, lambda: f64) -> usize {
        let a = self.verts[self.se[se].vert].co;
        let b = self.verts[self.se[self.se[se].next].vert].co;
        let sesym = self.sym(se);
        let sesymprev = self.prev(sesym);
        let sesymprevsym = self.sym(sesymprev);
        let senext = self.se[se].next;
        // interpolate(a, b, lambda) = a * (1 - t) + b * t
        let co = [a[0] * (1.0 - lambda) + b[0] * lambda, a[1] * (1.0 - lambda) + b[1] * lambda];
        let v = self.add_vert(co);
        let (nv, sf, symf) = (self.se[senext].vert, self.se[se].face, self.se[sesym].face);
        let e = self.add_edge(v, nv, sf, symf);
        self.se[sesym].vert = v;
        let (newse, newsesym) = (2 * e, 2 * e + 1);
        self.se[se].next = newse;
        self.se[newsesym].next = sesym;
        self.se[newse].next = senext;
        self.se[newse].rot = sesym;
        self.se[sesym].rot = newse;
        self.se[senext].rot = newsesym;
        self.se[newsesym].rot = sesymprevsym;
        self.se[sesymprev].next = newsesym;
        let nsv = self.se[newsesym].vert;
        if self.verts[nsv].symedge == sesym {
            self.verts[nsv].symedge = newsesym;
        }
        let c = self.edges[self.se[se].edge].constrained;
        self.edges[e].constrained = c;
        e
    }
    fn delete_edge(&mut self, se: usize) {
        let sesym = self.sym(se);
        let v1 = self.se[se].vert;
        let v2 = self.se[sesym].vert;
        let aface = self.se[se].face;
        let bface = self.se[sesym].face;
        let f = self.se[se].next;
        let h = self.prev(se);
        let i = self.se[sesym].next;
        let j = self.prev(sesym);
        let jsym = self.sym(j);
        let hsym = self.sym(h);
        let v1_isolated = i == se;
        let v2_isolated = f == sesym;
        if !v1_isolated {
            self.se[h].next = i;
            self.se[i].rot = hsym;
        }
        if !v2_isolated {
            self.se[j].next = f;
            self.se[f].rot = jsym;
        }
        if !v1_isolated && !v2_isolated && aface != bface {
            let mut k = i;
            while k != f {
                self.se[k].face = aface;
                k = self.se[k].next;
            }
        }
        if v1_isolated {
            self.verts[v1].symedge = NONE;
        } else if self.verts[v1].symedge == se {
            self.verts[v1].symedge = i;
        }
        if v2_isolated {
            self.verts[v2].symedge = NONE;
        } else if self.verts[v2].symedge == sesym {
            self.verts[v2].symedge = f;
        }
        self.se[se].next = NONE;
        self.se[se].rot = NONE;
        self.se[sesym].next = NONE;
        self.se[sesym].rot = NONE;
        if !v1_isolated && !v2_isolated && aface != bface {
            self.faces[bface].deleted = true;
            if self.outer == bface {
                self.outer = aface;
            }
        }
    }
    fn co(&self, v: usize) -> [f64; 2] {
        self.verts[v].co
    }
    fn orient(&self, a: usize, b: usize, c: usize) -> i32 {
        orient2d(self.co(a), self.co(b), self.co(c))
    }
    fn incirc(&self, a: usize, b: usize, c: usize, d: usize) -> i32 {
        incircle(self.co(a), self.co(b), self.co(c), self.co(d))
    }
    fn vert_left_of_symedge(&self, v: usize, se: usize) -> bool {
        self.orient(v, self.se[se].vert, self.se[self.se[se].next].vert) > 0
    }
    fn vert_right_of_symedge(&self, v: usize, se: usize) -> bool {
        self.orient(v, self.se[self.se[se].next].vert, self.se[se].vert) > 0
    }
    fn dc_tri_valid(&self, se: usize, basel: usize, basel_sym: usize) -> bool {
        self.orient(self.se[self.se[se].next].vert, self.se[basel_sym].vert, self.se[basel].vert) > 0
    }

    /// returns (le, re)
    fn dc_tri(&mut self, sites: &[(usize, usize)], start: usize, end: usize) -> (usize, usize) {
        let n = end - start;
        if n <= 1 {
            return (NONE, NONE);
        }
        if n <= 3 {
            let v1 = sites[start].0;
            let v2 = sites[start + 1].0;
            let o = self.outer;
            let ea = self.add_edge(v1, v2, o, o);
            self.se[2 * ea].next = 2 * ea + 1;
            self.se[2 * ea + 1].next = 2 * ea;
            self.se[2 * ea].rot = 2 * ea;
            self.se[2 * ea + 1].rot = 2 * ea + 1;
            if n == 2 {
                return (2 * ea, 2 * ea + 1);
            }
            let v3 = sites[start + 2].0;
            let eb = self.add_vert_to_symedge_edge(v3, 2 * ea + 1);
            let orient = self.orient(v1, v2, v3);
            if orient > 0 {
                self.add_diagonal(2 * eb, 2 * ea);
                return (2 * ea, 2 * eb);
            } else if orient < 0 {
                self.add_diagonal(2 * ea, 2 * eb);
                return (self.se[2 * ea].rot, self.se[2 * eb].rot);
            }
            return (2 * ea, 2 * eb);
        }
        let n2 = n / 2;
        let (mut ldo, mut ldi) = self.dc_tri(sites, start, start + n2);
        let (mut rdi, mut rdo) = self.dc_tri(sites, start + n2, end);
        // lower common tangent
        loop {
            if self.vert_left_of_symedge(self.se[rdi].vert, ldi) {
                ldi = self.se[ldi].next;
            } else if self.vert_right_of_symedge(self.se[ldi].vert, rdi) {
                let s = self.sym(rdi);
                rdi = self.se[s].rot;
            } else {
                break;
            }
        }
        let a = self.sym(rdi);
        let a = self.se[a].next;
        let mut ebasel = self.connect_separate_parts(a, ldi);
        let mut basel = 2 * ebasel;
        let mut basel_sym = 2 * ebasel + 1;
        if self.se[ldi].vert == self.se[ldo].vert {
            ldo = basel_sym;
        }
        if self.se[rdi].vert == self.se[rdo].vert {
            rdo = basel;
        }
        loop {
            let mut lcand = self.se[basel_sym].rot;
            let mut rcand = self.se[basel_sym].next;
            if self.dc_tri_valid(lcand, basel, basel_sym) {
                loop {
                    let lr = self.se[lcand].rot;
                    let c = self.se[self.se[lr].next].vert;
                    if self.incirc(self.se[basel_sym].vert, self.se[basel].vert, self.se[self.se[lcand].next].vert, c) > 0 {
                        let t = self.se[lcand].rot;
                        let s = self.sym(lcand);
                        self.delete_edge(s);
                        lcand = t;
                    } else {
                        break;
                    }
                }
            }
            if self.dc_tri_valid(rcand, basel, basel_sym) {
                loop {
                    let s = self.sym(rcand);
                    let c = self.se[self.se[self.se[s].next].next].vert;
                    if self.incirc(self.se[basel_sym].vert, self.se[basel].vert, self.se[self.se[rcand].next].vert, c) > 0 {
                        let t = self.se[s].next;
                        self.delete_edge(rcand);
                        rcand = t;
                    } else {
                        break;
                    }
                }
            }
            let valid_l = self.dc_tri_valid(lcand, basel, basel_sym);
            let valid_r = self.dc_tri_valid(rcand, basel, basel_sym);
            if !valid_l && !valid_r {
                break;
            }
            let use_r = !valid_l
                || (valid_r
                    && self.incirc(
                        self.se[self.se[lcand].next].vert,
                        self.se[lcand].vert,
                        self.se[rcand].vert,
                        self.se[self.se[rcand].next].vert,
                    ) > 0);
            if use_r {
                let a = self.se[rcand].next;
                ebasel = self.add_diagonal(a, basel_sym);
            } else {
                let a = self.se[basel_sym].next;
                let b = self.sym(lcand);
                ebasel = self.add_diagonal(a, b);
            }
            basel = 2 * ebasel;
            basel_sym = 2 * ebasel + 1;
        }
        (ldo, rdo)
    }

    fn exists_edge(&self, v1: usize, v2: usize) -> bool {
        self.find_symedge_between_verts(v1, v2) != NONE
    }
    fn find_symedge_between_verts(&self, v1: usize, v2: usize) -> usize {
        let tstart = self.verts[v1].symedge;
        let mut t = tstart;
        loop {
            if self.se[self.se[t].next].vert == v2 {
                return t;
            }
            t = self.se[t].rot;
            if t == tstart {
                return NONE;
            }
        }
    }
    fn find_symedge_with_face(&self, v: usize, f: usize) -> usize {
        let tstart = self.verts[v].symedge;
        let mut t = tstart;
        loop {
            if self.se[t].face == f {
                return t;
            }
            t = self.se[t].rot;
            if t == tstart {
                return NONE;
            }
        }
    }

    fn re_delaunay_triangulate(&mut self, se: usize) {
        if self.se[se].face == self.outer || self.se[self.sym(se)].face == self.outer {
            return;
        }
        let mut count = 1;
        let mut ss = self.se[se].next;
        while ss != se {
            count += 1;
            ss = self.se[ss].next;
        }
        if count <= 3 {
            return;
        }
        let first = self.se[self.se[se].next].next;
        let a = self.se[se].vert;
        let b = self.se[self.se[se].next].vert;
        let mut c = self.se[first].vert;
        let mut cse = first;
        let mut ss = self.se[first].next;
        while ss != se {
            let v = self.se[ss].vert;
            if self.incirc(a, b, c, v) > 0 {
                c = v;
                cse = ss;
            }
            ss = self.se[ss].next;
        }
        let mut ebc = NONE;
        let mut eca = NONE;
        if !self.exists_edge(b, c) {
            let s = self.se[se].next;
            ebc = self.add_diagonal(s, cse);
        }
        if !self.exists_edge(c, a) {
            eca = self.add_diagonal(cse, se);
        }
        if ebc != NONE {
            self.re_delaunay_triangulate(2 * ebc + 1);
        }
        if eca != NONE {
            self.re_delaunay_triangulate(2 * eca + 1);
        }
    }
}

#[derive(Clone, Copy)]
struct Cross {
    lambda: f64,
    vert: usize,
    in_: usize,
    out: usize,
}

fn isect_seg_seg(v1: [f64; 2], v2: [f64; 2], v3: [f64; 2], v4: [f64; 2]) -> (i32, f64) {
    // kinds: -1 colinear, 0 none, 1 exact, 2 cross
    let div = (v2[0] - v1[0]) * (v4[1] - v3[1]) - (v2[1] - v1[1]) * (v4[0] - v3[0]);
    if div == 0.0 {
        return (-1, 0.0);
    }
    let lambda = ((v1[1] - v3[1]) * (v4[0] - v3[0]) - (v1[0] - v3[0]) * (v4[1] - v3[1])) / div;
    let mu = ((v1[1] - v3[1]) * (v2[0] - v1[0]) - (v1[0] - v3[0]) * (v2[1] - v1[1])) / div;
    if (0.0..=1.0).contains(&lambda) && (0.0..=1.0).contains(&mu) {
        if lambda == 0.0 || lambda == 1.0 || mu == 0.0 || mu == 1.0 {
            (1, lambda)
        } else {
            (2, lambda)
        }
    } else {
        (0, lambda)
    }
}

fn in_line(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ac = [c[0] - a[0], c[1] - a[1]];
    if ab[0] * ac[0] + ab[1] * ac[1] < 0.0 {
        return false;
    }
    let bc = [c[0] - b[0], c[1] - b[1]];
    bc[0] * ac[0] + bc[1] * ac[1] >= 0.0
}

impl Cdt {
    fn fill_crossdata_for_through_vert(&self, v: usize, cd_out: usize, cd: &mut Cross, cd_next: &mut Cross) {
        cd_next.lambda = 0.0;
        cd_next.vert = v;
        cd_next.in_ = NONE;
        cd_next.out = NONE;
        if cd.lambda == 0.0 {
            cd.out = cd_out;
        } else {
            let mut se = self.sym(cd.in_);
            if self.se[se].vert != v {
                se = self.se[se].next;
                if self.se[se].vert != v {
                    se = self.se[se].next;
                }
            }
            cd_next.in_ = se;
        }
    }
    fn fill_crossdata_for_intersect(&self, curco: [f64; 2], v2: usize, t: usize, cd: &mut Cross, cd_next: &mut Cross) {
        let va = self.se[t].vert;
        let tn = self.se[t].next;
        let vb = self.se[tn].vert;
        let se_vcvb = self.sym(tn);
        let se_vcva = self.se[tn].next;
        let (kind, lambda) = isect_seg_seg(self.co(va), self.co(vb), curco, self.co(v2));
        match kind {
            2 => {
                let (a, b) = (self.co(va), self.co(vb));
                let len_ab = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                // epsilon is 0 for Fill Curve
                if lambda * len_ab <= 0.0 {
                    self.fill_crossdata_for_through_vert(va, se_vcva, cd, cd_next);
                } else if (1.0 - lambda) * len_ab <= 0.0 {
                    self.fill_crossdata_for_through_vert(vb, se_vcvb, cd, cd_next);
                } else {
                    *cd_next = Cross { lambda, vert: NONE, in_: t, out: NONE };
                    if cd.lambda == 0.0 {
                        cd.out = se_vcva;
                    }
                }
            }
            1 => {
                if lambda == 0.0 {
                    self.fill_crossdata_for_through_vert(va, se_vcva, cd, cd_next);
                } else if lambda == 1.0 {
                    self.fill_crossdata_for_through_vert(vb, se_vcvb, cd, cd_next);
                } else {
                    *cd_next = Cross { lambda, vert: NONE, in_: t, out: NONE };
                    if cd.lambda == 0.0 {
                        cd.out = se_vcva;
                    }
                }
            }
            0 => {
                if lambda <= 0.5 {
                    self.fill_crossdata_for_through_vert(va, se_vcva, cd, cd_next);
                } else {
                    self.fill_crossdata_for_through_vert(vb, se_vcvb, cd, cd_next);
                }
            }
            _ => {
                let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
                if d2(self.co(va), self.co(v2)) <= d2(self.co(vb), self.co(v2)) {
                    self.fill_crossdata_for_through_vert(va, se_vcva, cd, cd_next);
                } else {
                    self.fill_crossdata_for_through_vert(vb, se_vcvb, cd, cd_next);
                }
            }
        }
    }
    fn get_next_crossing_from_vert(&self, cd: &mut Cross, cd_next: &mut Cross, v2: usize) -> bool {
        let tstart = self.verts[cd.vert].symedge;
        let mut t = tstart;
        let vcur = cd.vert;
        loop {
            let tn = self.se[t].next;
            let va = self.se[tn].vert;
            let vb = self.se[self.se[tn].next].vert;
            let orient1 = self.orient(self.se[t].vert, va, v2);
            if orient1 == 0 && in_line(self.co(vcur), self.co(va), self.co(v2)) {
                self.fill_crossdata_for_through_vert(va, t, cd, cd_next);
                return true;
            }
            if self.se[t].face != self.outer {
                let orient2 = self.orient(vcur, vb, v2);
                if orient1 > 0 && orient2 < 0 {
                    let t2 = self.se[t].next;
                    self.fill_crossdata_for_intersect(self.co(vcur), v2, t2, cd, cd_next);
                    return true;
                }
            }
            t = self.se[t].rot;
            if t == tstart {
                return false;
            }
        }
    }
    fn get_next_crossing_from_edge(&self, cd: &Cross, cd_next: &mut Cross, v2: usize, cdm: &mut Cross) {
        let va = self.se[cd.in_].vert;
        let vb = self.se[self.se[cd.in_].next].vert;
        let (a, b) = (self.co(va), self.co(vb));
        let curco = [a[0] * (1.0 - cd.lambda) + b[0] * cd.lambda, a[1] * (1.0 - cd.lambda) + b[1] * cd.lambda];
        let se_ac = self.se[self.sym(cd.in_)].next;
        let vc = self.se[self.se[se_ac].next].vert;
        let orient = orient2d(curco, self.co(v2), self.co(vc));
        let mut cdc = *cd;
        if orient < 0 {
            let t = self.se[se_ac].next;
            self.fill_crossdata_for_intersect(curco, v2, t, &mut cdc, cd_next);
        } else if orient > 0 {
            self.fill_crossdata_for_intersect(curco, v2, se_ac, &mut cdc, cd_next);
        } else {
            *cd_next = Cross { lambda: 0.0, vert: vc, in_: self.se[se_ac].next, out: NONE };
        }
        *cdm = cdc;
    }

    fn add_edge_constraint(&mut self, v1: usize, v2: usize) {
        if v1 == v2 {
            return;
        }
        let t = self.find_symedge_between_verts(v1, v2);
        if t != NONE {
            let e = self.se[t].edge;
            self.edges[e].constrained = true;
            return;
        }
        self.visit_count += 1;
        let visit = self.visit_count;
        let mut crossings: Vec<Cross> = vec![Cross { lambda: 0.0, vert: v1, in_: NONE, out: NONE }];
        loop {
            let n = crossings.len();
            if crossings[n - 1].vert == v2 && n > 0 {
                break;
            }
            let mut cd = crossings[n - 1];
            let mut cd_next = Cross { lambda: 0.0, vert: NONE, in_: NONE, out: NONE };
            let ok;
            if cd.lambda == 0.0 {
                ok = self.get_next_crossing_from_vert(&mut cd, &mut cd_next, v2);
            } else {
                let mut cdm = cd;
                self.get_next_crossing_from_edge(&cd, &mut cd_next, v2, &mut cdm);
                cd = cdm;
                ok = true;
            }
            crossings[n - 1] = cd;
            crossings.push(cd_next);
            if !ok || crossings.len() == 100000 {
                return;
            }
            if crossings[n].lambda == 0.0 {
                let v = crossings[n].vert;
                if self.verts[v].visit == visit {
                    return;
                }
                self.verts[v].visit = visit;
            }
        }
        let ncross = crossings.len();
        // eliminate duplicate vertex crossings
        for i in 2..ncross {
            if crossings[i].lambda == 0.0 {
                let v = crossings[i].vert;
                let mut j = i - 1;
                while j > 0 {
                    let cp = crossings[j];
                    let brk = (cp.lambda == 0.0 && cp.vert != v)
                        || (cp.lambda != 0.0 && self.se[cp.in_].vert != v && self.se[self.se[cp.in_].next].vert != v);
                    if brk {
                        break;
                    }
                    crossings[j].lambda = -1.0;
                    j -= 1;
                }
                if j + 1 < i {
                    let cp = crossings[j];
                    if cp.lambda == 0.0 {
                        let se = self.find_symedge_between_verts(cp.vert, v);
                        if se == NONE {
                            return;
                        }
                        crossings[j].out = se;
                        crossings[i].in_ = NONE;
                    } else {
                        let f = self.se[self.sym(cp.in_)].face;
                        let se = self.find_symedge_with_face(v, f);
                        if se == NONE {
                            return;
                        }
                        crossings[i].in_ = se;
                    }
                }
            }
        }
        // split constrained edges that are crossed
        for i in 0..ncross {
            let cd = crossings[i];
            if cd.lambda != 0.0 && cd.lambda != -1.0 && self.edges[self.se[cd.in_].edge].constrained {
                let e = self.split_edge(cd.in_, cd.lambda);
                crossings[i].vert = self.se[2 * e].vert;
            }
        }
        // remove crossed unconstrained edges
        for i in 0..ncross {
            let cd = crossings[i];
            if cd.lambda != 0.0 && cd.lambda != -1.0 && !self.edges[self.se[cd.in_].edge].constrained {
                self.delete_edge(cd.in_);
            }
        }
        // add diagonals and retriangulate
        let mut tstart = crossings[0].out;
        for i in 1..ncross {
            let cd = crossings[i];
            if cd.lambda == -1.0 {
                continue;
            }
            let mut t = NONE;
            let mut tnext = t;
            let mut edge = NONE;
            if cd.lambda != 0.0 {
                if self.edges[self.se[cd.in_].edge].constrained {
                    t = self.verts[cd.vert].symedge;
                    tnext = self.se[self.sym(t)].next;
                }
            } else {
                t = cd.in_;
                tnext = cd.out;
                if t == NONE {
                    let mut j = i as i64 - 1;
                    while j >= 0 {
                        if crossings[j as usize].lambda != -1.0 {
                            break;
                        }
                        j -= 1;
                    }
                    let cp = crossings[j as usize];
                    edge = self.se[cp.out].edge;
                    self.edges[edge].constrained = true;
                }
            }
            if t != NONE {
                if self.se[self.se[tstart].next].vert == self.se[t].vert {
                    edge = self.se[tstart].edge;
                } else {
                    edge = self.add_diagonal(tstart, t);
                }
                self.edges[edge].constrained = true;
                self.re_delaunay_triangulate(2 * edge);
                self.re_delaunay_triangulate(2 * edge + 1);
            }
            let _ = edge;
            if i < ncross - 1 && tnext != NONE {
                tstart = tnext;
            }
        }
    }

    fn dissolve_symedge(&mut self, mut se: usize) {
        let mut symse = self.sym(se);
        if self.se[symse].face == self.outer {
            se = self.sym(se);
            symse = self.sym(se);
        }
        let ofs = self.faces[self.outer].symedge;
        if ofs == se || ofs == symse {
            if self.se[self.se[se].next].next == se {
                self.faces[self.outer].symedge = NONE;
            } else {
                self.faces[self.outer].symedge = self.se[self.se[se].next].next;
            }
        } else {
            let f = self.se[se].face;
            if self.faces[f].symedge == se {
                self.faces[f].symedge = self.se[se].next;
            }
            let g = self.se[symse].face;
            if self.faces[g].symedge == symse {
                self.faces[g].symedge = self.se[symse].next;
            }
        }
        self.delete_edge(se);
    }

    fn remove_outer_edges_until_constraints(&mut self) {
        self.visit_count += 1;
        let visit = self.visit_count;
        let o = self.outer;
        self.faces[o].visit = visit;
        let mut fstack: Vec<usize> = vec![];
        let se_start = self.faces[o].symedge;
        let mut se = se_start;
        loop {
            if !self.edges[self.se[se].edge].constrained {
                let fsym = self.se[self.sym(se)].face;
                if self.faces[fsym].visit != visit {
                    fstack.push(fsym);
                }
            }
            se = self.se[se].next;
            if se == se_start {
                break;
            }
        }
        while let Some(f) = fstack.pop() {
            if self.faces[f].visit == visit {
                continue;
            }
            self.faces[f].visit = visit;
            let se_start = self.faces[f].symedge;
            let mut se = se_start;
            let mut to_dissolve: Vec<usize> = vec![];
            loop {
                let dissolvable = !self.edges[self.se[se].edge].constrained;
                if dissolvable {
                    let fsym = self.se[self.sym(se)].face;
                    if self.faces[fsym].visit != visit {
                        fstack.push(fsym);
                    } else {
                        to_dissolve.push(se);
                    }
                }
                se = self.se[se].next;
                if se == se_start {
                    break;
                }
            }
            while let Some(se) = to_dissolve.pop() {
                if self.se[se].next != NONE {
                    self.dissolve_symedge(se);
                }
            }
        }
    }

    fn remove_faces_in_holes(&mut self) {
        for i in 0..self.faces.len() {
            if !self.faces[i].deleted && self.faces[i].hole {
                self.faces[i].deleted = true;
                let se_start = self.faces[i].symedge;
                let mut se = se_start;
                loop {
                    let se_next = self.se[se].next;
                    if self.se[se].edge != NONE && !self.edges[self.se[se].edge].constrained {
                        self.se[se].next = NONE;
                    }
                    se = se_next;
                    if se == se_start {
                        break;
                    }
                }
            }
        }
    }

    fn detect_holes(&mut self) {
        for f in self.faces.iter_mut() {
            f.visit = -1;
        }
        let mut cur_region: i64 = -1;
        let o = self.outer;
        self.faces[o].visit = -2;
        let mut region_rep: Vec<usize> = vec![];
        for i in 0..self.faces.len() {
            if !self.faces[i].deleted && self.faces[i].symedge != NONE && self.faces[i].visit == -1 {
                let mut fstack = vec![i];
                cur_region += 1;
                region_rep.push(i);
                while let Some(f) = fstack.pop() {
                    if self.faces[f].visit != -1 {
                        continue;
                    }
                    self.faces[f].visit = cur_region;
                    let se_start = self.faces[f].symedge;
                    let mut se = se_start;
                    loop {
                        if !self.edges[self.se[se].edge].constrained {
                            let fsym = self.se[self.sym(se)].face;
                            if !self.faces[fsym].deleted && self.faces[fsym].visit == -1 {
                                fstack.push(fsym);
                            }
                        }
                        se = self.se[se].next;
                        if se == se_start {
                            break;
                        }
                    }
                }
            }
        }
        cur_region += 1;
        self.visit_count = cur_region;
        let ray_end = [123456.0f64, 654321.0f64];
        for &f in &region_rep {
            let s0 = self.faces[f].symedge;
            let a = self.co(self.se[s0].vert);
            let b = self.co(self.se[self.se[s0].next].vert);
            let c = self.co(self.se[self.se[self.se[s0].next].next].vert);
            let mid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            let mut hits = 0;
            for e in 0..self.edges.len() {
                if self.se[2 * e].next == NONE || !self.edges[e].constrained {
                    continue;
                }
                let (f0, f1) = (self.se[2 * e].face, self.se[2 * e + 1].face);
                if self.faces[f0].visit == self.faces[f1].visit {
                    continue;
                }
                let (k, _) = isect_seg_seg(ray_end, mid, self.co(self.se[2 * e].vert), self.co(self.se[2 * e + 1].vert));
                if k == 2 {
                    hits += 1;
                }
            }
            self.faces[f].hole = hits % 2 == 0;
        }
        for i in 0..self.faces.len() {
            let region = self.faces[i].visit;
            if region < 0 {
                continue;
            }
            let rep = region_rep[region as usize];
            self.faces[i].hole = self.faces[rep].hole;
        }
    }
}

/// delaunay_2d_calc(CDT_input{vert, face}, CDT_INSIDE_WITH_HOLES) -> (vertices, edges, faces)
pub fn delaunay_inside_with_holes(input: &[[f64; 2]], faces: &[Vec<usize>]) -> (Vec<[f64; 2]>, Vec<(usize, usize)>, Vec<Vec<usize>>) {
    let mut c = Cdt { verts: vec![], edges: vec![], se: vec![], faces: vec![], outer: 0, visit_count: 0 };
    c.outer = c.add_face();
    for &p in input {
        c.add_vert(p);
    }
    let nv = input.len();
    // initial triangulation
    if c.verts.len() > 1 {
        let mut sites: Vec<(usize, usize)> = (0..nv).map(|i| (i, i)).collect();
        sites.sort_by(|a, b| {
            let (ca, cb) = (c.verts[a.0].co, c.verts[b.0].co);
            ca[0].partial_cmp(&cb[0]).unwrap().then(ca[1].partial_cmp(&cb[1]).unwrap()).then(a.1.cmp(&b.1))
        });
        // find_site_merges
        let n = sites.len();
        let mut i = 0;
        while i + 1 < n {
            let mut j = i + 1;
            while j < n && c.verts[sites[j].0].co == c.verts[sites[i].0].co {
                c.verts[sites[j].0].merge_to = sites[i].1 as i64;
                j += 1;
            }
            if j - i > 1 {
                i = j - 1;
            }
            i += 1;
        }
        // compress
        let mut kept: Vec<(usize, usize)> = vec![];
        for s in &sites {
            if c.verts[s.0].merge_to < 0 {
                kept.push(*s);
            }
        }
        if !kept.is_empty() {
            let n = kept.len();
            c.dc_tri(&kept, 0, n);
        }
    }
    // face constraints
    let mut faces_added = 0;
    for f in faces {
        if f.len() <= 2 {
            continue;
        }
        for i in 0..f.len() {
            let (iv1, iv2) = (f[i], f[(i + 1) % f.len()]);
            if iv1 >= nv || iv2 >= nv {
                continue;
            }
            faces_added += 1;
            let resolve = |c: &Cdt, i: usize| -> usize {
                let v = &c.verts[i];
                if v.merge_to != -1 {
                    v.merge_to as usize
                } else {
                    i
                }
            };
            let v1 = resolve(&c, iv1);
            let v2 = resolve(&c, iv2);
            c.add_edge_constraint(v1, v2);
        }
    }
    let inside_only = faces_added == 0;
    // prepare output
    if c.edges.is_empty() {
        return (vec![], vec![], vec![]);
    }
    for e in 0..c.edges.len() {
        if c.se[2 * e].next != NONE {
            for k in 0..2 {
                let f = c.se[2 * e + k].face;
                if c.faces[f].symedge == NONE {
                    c.faces[f].symedge = 2 * e + k;
                }
            }
        }
    }
    if !inside_only {
        c.detect_holes();
    }
    c.remove_outer_edges_until_constraints();
    if !inside_only {
        c.remove_faces_in_holes();
    }
    // output
    let mut map = vec![0usize; c.verts.len()];
    let mut nvo = 0;
    for i in 0..c.verts.len() {
        if c.verts[i].merge_to == -1 {
            map[i] = nvo;
            nvo += 1;
        }
    }
    for i in 0..c.verts.len() {
        if c.verts[i].merge_to != -1 {
            map[i] = map[c.verts[i].merge_to as usize];
        }
    }
    let verts: Vec<[f64; 2]> = c.verts.iter().filter(|v| v.merge_to == -1).map(|v| v.co).collect();
    let mut edges = vec![];
    for e in 0..c.edges.len() {
        if c.se[2 * e].next != NONE {
            edges.push((map[c.se[2 * e].vert], map[c.se[2 * e + 1].vert]));
        }
    }
    let mut out_faces = vec![];
    for (fi, f) in c.faces.iter().enumerate() {
        if !f.deleted && fi != c.outer {
            let s0 = f.symedge;
            let mut s = s0;
            let mut ring = vec![];
            loop {
                ring.push(map[c.se[s].vert]);
                s = c.se[s].next;
                if s == s0 {
                    break;
                }
            }
            out_faces.push(ring);
        }
    }
    (verts, edges, out_faces)
}
