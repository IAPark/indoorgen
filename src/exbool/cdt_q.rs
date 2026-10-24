//! Port of Blender 4.2 `blenlib/intern/delaunay_2d.cc` instantiated for `T = mpq_class` (exact rationals),
//! restricted to what mesh_intersect uses: `need_ids = true`, `epsilon = 0`, output type `CDT_INSIDE`.
//!
//! SymEdges are indices: edge `e` owns symedges `2e` and `2e+1`, `sym(s) = s ^ 1`.
//! `blender::Set<int>` id sets are emulated with [`BSet`] so that iteration order matches Blender's.

use crate::exbool::hashtab::BSet;
use crate::exbool::mpq::*;
use num_traits::{One, Signed, Zero};

const NIL: usize = usize::MAX;
const DBL_EPSILON: f64 = f64::EPSILON;

fn qi(x: i64) -> Q {
    q_int(x)
}

/// A coordinate with extra information for fast filtered tests (FatCo<mpq_class>).
#[derive(Clone)]
pub struct Fat {
    pub ex: [Q; 2],
    pub ap: [f64; 2],
    pub ab: [f64; 2],
}
impl Fat {
    pub fn new(p: [Q; 2]) -> Fat {
        let ap = [q_get_d(&p[0]), q_get_d(&p[1])];
        Fat { ex: p, ap, ab: [ap[0].abs(), ap[1].abs()] }
    }
}

fn filtered_orient2d(a: &Fat, b: &Fat, c: &Fat) -> i32 {
    let det = (a.ap[0] - c.ap[0]) * (b.ap[1] - c.ap[1]) - (a.ap[1] - c.ap[1]) * (b.ap[0] - c.ap[0]);
    let supremum = (a.ab[0] + c.ab[0]) * (b.ab[1] + c.ab[1]) + (a.ab[1] + c.ab[1]) * (b.ab[0] + c.ab[0]);
    let err_bound = supremum * 6.0 * DBL_EPSILON;
    if det.abs() > err_bound {
        return if det > 0.0 { 1 } else { -1 };
    }
    q_orient2d(&a.ex, &b.ex, &c.ex)
}

fn filtered_incircle(a: &Fat, b: &Fat, c: &Fat, d: &Fat) -> i32 {
    let adx = a.ap[0] - d.ap[0];
    let bdx = b.ap[0] - d.ap[0];
    let cdx = c.ap[0] - d.ap[0];
    let ady = a.ap[1] - d.ap[1];
    let bdy = b.ap[1] - d.ap[1];
    let cdy = c.ap[1] - d.ap[1];
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

    let sup_adx = a.ab[0] + d.ab[0];
    let sup_bdx = b.ab[0] + d.ab[0];
    let sup_cdx = c.ab[0] + d.ab[0];
    let sup_ady = a.ab[1] + d.ab[1];
    let sup_bdy = b.ab[1] + d.ab[1];
    let sup_cdy = c.ab[1] + d.ab[1];
    let sup_bdxcdy = sup_bdx * sup_cdy;
    let sup_cdxbdy = sup_cdx * sup_bdy;
    let sup_alift = sup_adx * sup_adx + sup_ady * sup_ady;
    let sup_cdxady = sup_cdx * sup_ady;
    let sup_adxcdy = sup_adx * sup_cdy;
    let sup_blift = sup_bdx * sup_bdx + sup_bdy * sup_bdy;
    let sup_adxbdy = sup_adx * sup_bdy;
    let sup_bdxady = sup_bdx * sup_ady;
    let sup_clift = sup_cdx * sup_cdx + sup_cdy * sup_cdy;
    let sup_det = sup_alift * (sup_bdxcdy + sup_cdxbdy) + sup_blift * (sup_cdxady + sup_adxcdy) + sup_clift * (sup_adxbdy + sup_bdxady);
    let index = 14.0;
    let err_bound = sup_det * index * DBL_EPSILON;
    if det.abs() > err_bound {
        return if det < 0.0 {
            -1
        } else if det > 0.0 {
            1
        } else {
            0
        };
    }
    q_incircle(&a.ex, &b.ex, &c.ex, &d.ex)
}

fn in_line(a: &Fat, b: &Fat, c: &Fat) -> bool {
    let ab = [b.ap[0] - a.ap[0], b.ap[1] - a.ap[1]];
    let bc = [c.ap[0] - b.ap[0], c.ap[1] - b.ap[1]];
    let ac = [c.ap[0] - a.ap[0], c.ap[1] - a.ap[1]];
    let sab = [b.ab[0] + a.ab[0], b.ab[1] + a.ab[1]];
    let sbc = [c.ab[0] + b.ab[0], c.ab[1] + b.ab[1]];
    let sac = [c.ab[0] + a.ab[0], c.ab[1] + a.ab[1]];
    let dot_ab_ac = ab[0] * ac[0] + ab[1] * ac[1];
    let sup_dot_ab_ac = sab[0] * sac[0] + sab[1] * sac[1];
    let index = 6.0;
    let mut err_bound = sup_dot_ab_ac * index * DBL_EPSILON;
    if dot_ab_ac < -err_bound {
        return false;
    }
    let dot_bc_ac = bc[0] * ac[0] + bc[1] * ac[1];
    let sup_dot_bc_ac = sbc[0] * sac[0] + sbc[1] * sac[1];
    err_bound = sup_dot_bc_ac * index * DBL_EPSILON;
    if dot_bc_ac < -err_bound {
        return false;
    }
    let eab = [&b.ex[0] - &a.ex[0], &b.ex[1] - &a.ex[1]];
    let eac = [&c.ex[0] - &a.ex[0], &c.ex[1] - &a.ex[1]];
    if &eab[0] * &eac[0] + &eab[1] * &eac[1] < Q::zero() {
        return false;
    }
    let ebc = [&c.ex[0] - &b.ex[0], &c.ex[1] - &b.ex[1]];
    &ebc[0] * &eac[0] + &ebc[1] * &eac[1] >= Q::zero()
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum IKind {
    Colinear,
    None,
    Exact,
    Cross,
}

/// `isect_seg_seg<mpq2>` (math_vector.cc).
fn isect_seg_seg(v1: &[Q; 2], v2: &[Q; 2], v3: &[Q; 2], v4: &[Q; 2]) -> (IKind, Q) {
    let div = (&v2[0] - &v1[0]) * (&v4[1] - &v3[1]) - (&v2[1] - &v1[1]) * (&v4[0] - &v3[0]);
    if div.is_zero() {
        return (IKind::Colinear, Q::zero());
    }
    let lambda = ((&v1[1] - &v3[1]) * (&v4[0] - &v3[0]) - (&v1[0] - &v3[0]) * (&v4[1] - &v3[1])) / &div;
    let mudiv = (&v1[1] - &v3[1]) * (&v2[0] - &v1[0]) - (&v1[0] - &v3[0]) * (&v2[1] - &v1[1]);
    let zero = Q::zero();
    let one = Q::one();
    let ds = q_sgn(&div);
    if lambda >= zero && lambda <= one && ((ds > 0 && mudiv >= zero && mudiv <= div) || (ds < 0 && mudiv <= zero && mudiv >= div)) {
        if lambda.is_zero() || lambda == one || mudiv.is_zero() || mudiv == div {
            (IKind::Exact, lambda)
        } else {
            (IKind::Cross, lambda)
        }
    } else {
        (IKind::None, lambda)
    }
}

#[derive(Clone)]
struct CrossData {
    lambda: Q,
    vert: usize,
    inn: usize,
    out: usize,
}
impl CrossData {
    fn new() -> Self {
        CrossData { lambda: Q::zero(), vert: NIL, inn: NIL, out: NIL }
    }
}

pub struct CdtInput {
    pub vert: Vec<[Q; 2]>,
    pub edge: Vec<(usize, usize)>,
    pub face: Vec<Vec<usize>>,
}

#[derive(Default)]
pub struct CdtOutput {
    pub vert: Vec<[Q; 2]>,
    pub vert_orig: Vec<Vec<i32>>,
    pub edge: Vec<(usize, usize)>,
    pub edge_orig: Vec<Vec<i32>>,
    pub face: Vec<Vec<usize>>,
    pub face_orig: Vec<Vec<i32>>,
    pub face_edge_offset: i32,
}

fn new_set() -> BSet<i32> {
    BSet::new(8)
}

struct Cdt {
    vfat: Vec<Fat>,
    vsym: Vec<usize>,
    vmerge: Vec<i64>,
    vvisit: Vec<i32>,
    vids: Vec<BSet<i32>>,
    next: Vec<usize>,
    rot: Vec<usize>,
    svert: Vec<usize>,
    sface: Vec<usize>,
    eids: Vec<BSet<i32>>,
    fsym: Vec<usize>,
    fids: Vec<BSet<i32>>,
    fvisit: Vec<i32>,
    fdel: Vec<bool>,
    outer: usize,
    visit_count: i32,
    input_vert_num: usize,
    face_edge_offset: i32,
}

fn sym(s: usize) -> usize {
    s ^ 1
}
fn eof(s: usize) -> usize {
    s >> 1
}

fn add_list(dst: &mut BSet<i32>, src: &BSet<i32>) {
    let items: Vec<i32> = src.iter().copied().collect();
    for v in items {
        dst.add(v);
    }
}

impl Cdt {
    fn prev(&self, s: usize) -> usize {
        self.rot[self.next[self.rot[s]]]
    }
    fn add_vert(&mut self, p: [Q; 2]) -> usize {
        self.vfat.push(Fat::new(p));
        self.vsym.push(NIL);
        self.vmerge.push(-1);
        self.vvisit.push(0);
        self.vids.push(new_set());
        self.vfat.len() - 1
    }
    fn add_face(&mut self) -> usize {
        self.fsym.push(NIL);
        self.fids.push(new_set());
        self.fvisit.push(0);
        self.fdel.push(false);
        self.fsym.len() - 1
    }
    fn add_edge(&mut self, v1: usize, v2: usize, fleft: usize, fright: usize) -> usize {
        let e = self.eids.len();
        self.eids.push(new_set());
        self.next.extend([NIL, NIL]);
        self.rot.extend([NIL, NIL]);
        self.svert.extend([v1, v2]);
        self.sface.extend([fleft, fright]);
        if self.vsym[v1] == NIL {
            self.vsym[v1] = 2 * e;
        }
        if self.vsym[v2] == NIL {
            self.vsym[v2] = 2 * e + 1;
        }
        e
    }
    fn add_diagonal(&mut self, s1: usize, s2: usize) -> usize {
        let fold = self.sface[s1];
        let fnew = self.add_face();
        let s1prev = self.prev(s1);
        let s1prevsym = sym(s1prev);
        let s2prev = self.prev(s2);
        let s2prevsym = sym(s2prev);
        let ed = self.add_edge(self.svert[s1], self.svert[s2], fnew, fold);
        let sd = 2 * ed;
        let sds = 2 * ed + 1;
        self.next[sd] = s2;
        self.next[sds] = s1;
        self.next[s2prev] = sds;
        self.next[s1prev] = sd;
        self.rot[s1] = sd;
        self.rot[sd] = s1prevsym;
        self.rot[s2] = sds;
        self.rot[sds] = s2prevsym;
        let mut se = s2;
        while se != sd {
            self.sface[se] = fnew;
            se = self.next[se];
        }
        let src = self.fids[fold].clone();
        add_list(&mut self.fids[fnew], &src);
        ed
    }
    fn add_vert_to_symedge_edge(&mut self, v: usize, se: usize) -> usize {
        let se_rot = self.rot[se];
        let se_rotsym = sym(se_rot);
        let e = self.add_edge(v, self.svert[se], self.sface[se], self.sface[se]);
        let (ns, nss) = (2 * e, 2 * e + 1);
        self.next[ns] = se;
        self.next[nss] = ns;
        self.rot[ns] = ns;
        self.rot[nss] = se_rot;
        self.rot[se] = nss;
        self.next[se_rotsym] = nss;
        e
    }
    fn connect_separate_parts(&mut self, se1: usize, se2: usize) -> usize {
        let se1_rot = self.rot[se1];
        let se1_rotsym = sym(se1_rot);
        let se2_rot = self.rot[se2];
        let se2_rotsym = sym(se2_rot);
        let e = self.add_edge(self.svert[se1], self.svert[se2], self.outer, self.outer);
        let (ns, nss) = (2 * e, 2 * e + 1);
        self.next[ns] = se2;
        self.next[nss] = se1;
        self.rot[ns] = se1_rot;
        self.rot[nss] = se2_rot;
        self.rot[se1] = ns;
        self.rot[se2] = nss;
        self.next[se1_rotsym] = ns;
        self.next[se2_rotsym] = nss;
        e
    }
    fn split_edge(&mut self, se: usize, lambda: &Q) -> usize {
        let a = self.vfat[self.svert[se]].ex.clone();
        let b = self.vfat[self.svert[self.next[se]]].ex.clone();
        let sesym = sym(se);
        let sesymprev = self.prev(sesym);
        let sesymprevsym = sym(sesymprev);
        let senext = self.next[se];
        let one_m = Q::one() - lambda;
        let p = [&a[0] * &one_m + &b[0] * lambda, &a[1] * &one_m + &b[1] * lambda];
        let v = self.add_vert(p);
        let e = self.add_edge(v, self.svert[self.next[se]], self.sface[se], self.sface[sesym]);
        self.svert[sesym] = v;
        let (newse, newsesym) = (2 * e, 2 * e + 1);
        self.next[se] = newse;
        self.next[newsesym] = sesym;
        self.next[newse] = senext;
        self.rot[newse] = sesym;
        self.rot[sesym] = newse;
        self.rot[senext] = newsesym;
        self.rot[newsesym] = sesymprevsym;
        self.next[sesymprev] = newsesym;
        let nv = self.svert[newsesym];
        if self.vsym[nv] == sesym {
            self.vsym[nv] = newsesym;
        }
        let src = self.eids[eof(se)].clone();
        add_list(&mut self.eids[e], &src);
        e
    }
    fn delete_edge(&mut self, se: usize) {
        let sesym = sym(se);
        let v1 = self.svert[se];
        let v2 = self.svert[sesym];
        let aface = self.sface[se];
        let bface = self.sface[sesym];
        let f = self.next[se];
        let h = self.prev(se);
        let i = self.next[sesym];
        let j = self.prev(sesym);
        let jsym = sym(j);
        let hsym = sym(h);
        let v1_isolated = i == se;
        let v2_isolated = f == sesym;
        if !v1_isolated {
            self.next[h] = i;
            self.rot[i] = hsym;
        }
        if !v2_isolated {
            self.next[j] = f;
            self.rot[f] = jsym;
        }
        if !v1_isolated && !v2_isolated && aface != bface {
            let mut k = i;
            while k != f {
                self.sface[k] = aface;
                k = self.next[k];
            }
        }
        if v1_isolated {
            self.vsym[v1] = NIL;
        } else if self.vsym[v1] == se {
            self.vsym[v1] = i;
        }
        if v2_isolated {
            self.vsym[v2] = NIL;
        } else if self.vsym[v2] == sesym {
            self.vsym[v2] = f;
        }
        self.next[se] = NIL;
        self.rot[se] = NIL;
        self.next[sesym] = NIL;
        self.rot[sesym] = NIL;
        if !v1_isolated && !v2_isolated && aface != bface {
            self.fdel[bface] = true;
            if self.outer == bface {
                self.outer = aface;
            }
        }
    }

    fn is_constrained(&self, e: usize) -> bool {
        !self.eids[e].is_empty()
    }
    fn is_deleted(&self, e: usize) -> bool {
        self.next[2 * e] == NIL
    }
    fn find_symedge_between(&self, v1: usize, v2: usize) -> usize {
        let start = self.vsym[v1];
        let mut t = start;
        loop {
            if self.svert[self.next[t]] == v2 {
                return t;
            }
            t = self.rot[t];
            if t == start {
                return NIL;
            }
        }
    }
    fn find_symedge_with_face(&self, v: usize, f: usize) -> usize {
        let start = self.vsym[v];
        let mut t = start;
        loop {
            if self.sface[t] == f {
                return t;
            }
            t = self.rot[t];
            if t == start {
                return NIL;
            }
        }
    }

    // ---------------------------------------------------------------- divide and conquer
    fn fat(&self, v: usize) -> &Fat {
        &self.vfat[v]
    }
    fn vert_left_of(&self, v: usize, se: usize) -> bool {
        filtered_orient2d(self.fat(v), self.fat(self.svert[se]), self.fat(self.svert[self.next[se]])) > 0
    }
    fn vert_right_of(&self, v: usize, se: usize) -> bool {
        filtered_orient2d(self.fat(v), self.fat(self.svert[self.next[se]]), self.fat(self.svert[se])) > 0
    }
    fn dc_tri_valid(&self, se: usize, basel: usize, basel_sym: usize) -> bool {
        filtered_orient2d(self.fat(self.svert[self.next[se]]), self.fat(self.svert[basel_sym]), self.fat(self.svert[basel])) > 0
    }
    fn vf(&self, se: usize) -> &Fat {
        &self.vfat[self.svert[se]]
    }

    fn dc_tri(&mut self, sites: &[usize], start: usize, end: usize) -> (usize, usize) {
        let n = end - start;
        if n <= 1 {
            return (NIL, NIL);
        }
        if n <= 3 {
            let v1 = sites[start];
            let v2 = sites[start + 1];
            let ea = self.add_edge(v1, v2, self.outer, self.outer);
            self.next[2 * ea] = 2 * ea + 1;
            self.next[2 * ea + 1] = 2 * ea;
            self.rot[2 * ea] = 2 * ea;
            self.rot[2 * ea + 1] = 2 * ea + 1;
            if n == 2 {
                return (2 * ea, 2 * ea + 1);
            }
            let v3 = sites[start + 2];
            let eb = self.add_vert_to_symedge_edge(v3, 2 * ea + 1);
            let o = filtered_orient2d(self.fat(v1), self.fat(v2), self.fat(v3));
            if o > 0 {
                self.add_diagonal(2 * eb, 2 * ea);
                return (2 * ea, 2 * eb);
            } else if o < 0 {
                self.add_diagonal(2 * ea, 2 * eb);
                return (self.rot[2 * ea], self.rot[2 * eb]);
            } else {
                return (2 * ea, 2 * eb);
            }
        }
        let n2 = n / 2;
        let (mut ldo, mut ldi) = self.dc_tri(sites, start, start + n2);
        let (mut rdi, mut rdo) = self.dc_tri(sites, start + n2, end);
        loop {
            if self.vert_left_of(self.svert[rdi], ldi) {
                ldi = self.next[ldi];
            } else if self.vert_right_of(self.svert[ldi], rdi) {
                rdi = self.rot[sym(rdi)];
            } else {
                break;
            }
        }
        let ebasel = self.connect_separate_parts(self.next[sym(rdi)], ldi);
        let mut basel = 2 * ebasel;
        let mut basel_sym = 2 * ebasel + 1;
        if self.svert[ldi] == self.svert[ldo] {
            ldo = basel_sym;
        }
        if self.svert[rdi] == self.svert[rdo] {
            rdo = basel;
        }
        loop {
            let mut lcand = self.rot[basel_sym];
            let mut rcand = self.next[basel_sym];
            if self.dc_tri_valid(lcand, basel, basel_sym) {
                while filtered_incircle(self.vf(basel_sym), self.vf(basel), self.vf(self.next[lcand]), self.vf(self.next[self.rot[lcand]])) > 0 {
                    let t = self.rot[lcand];
                    self.delete_edge(sym(lcand));
                    lcand = t;
                }
            }
            if self.dc_tri_valid(rcand, basel, basel_sym) {
                while filtered_incircle(self.vf(basel_sym), self.vf(basel), self.vf(self.next[rcand]), self.vf(self.next[self.next[sym(rcand)]])) > 0 {
                    let t = self.next[sym(rcand)];
                    self.delete_edge(rcand);
                    rcand = t;
                }
            }
            let valid_l = self.dc_tri_valid(lcand, basel, basel_sym);
            let valid_r = self.dc_tri_valid(rcand, basel, basel_sym);
            if !valid_l && !valid_r {
                break;
            }
            let eb = if !valid_l || (valid_r && filtered_incircle(self.vf(self.next[lcand]), self.vf(lcand), self.vf(rcand), self.vf(self.next[rcand])) > 0) {
                self.add_diagonal(self.next[rcand], basel_sym)
            } else {
                self.add_diagonal(self.next[basel_sym], sym(lcand))
            };
            basel = 2 * eb;
            basel_sym = 2 * eb + 1;
        }
        (ldo, rdo)
    }

    fn initial_triangulation(&mut self) {
        let n = self.vfat.len();
        if n <= 1 {
            return;
        }
        let mut sites: Vec<usize> = (0..n).collect();
        {
            let vf = &self.vfat;
            sites.sort_by(|&a, &b| {
                let (ca, cb) = (&vf[a].ex, &vf[b].ex);
                ca[0].cmp(&cb[0]).then(ca[1].cmp(&cb[1])).then(a.cmp(&b))
            });
        }
        let ns = sites.len();
        let mut i = 0;
        while i + 1 < ns {
            let mut j = i + 1;
            while j < ns && self.vfat[sites[j]].ex == self.vfat[sites[i]].ex {
                self.vmerge[sites[j]] = sites[i] as i64;
                j += 1;
            }
            if j - i > 1 {
                i = j - 1;
            }
            i += 1;
        }
        let comp: Vec<usize> = sites.iter().copied().filter(|&v| self.vmerge[v] < 0).collect();
        if comp.is_empty() {
            return;
        }
        self.dc_tri(&comp, 0, comp.len());
    }

    // ---------------------------------------------------------------- constraints
    fn re_delaunay_triangulate(&mut self, se: usize) {
        if self.sface[se] == self.outer || self.sface[sym(se)] == self.outer {
            return;
        }
        let mut count = 1;
        let mut ss = self.next[se];
        while ss != se {
            count += 1;
            ss = self.next[ss];
        }
        if count <= 3 {
            return;
        }
        let first = self.next[self.next[se]];
        let a = self.svert[se];
        let b = self.svert[self.next[se]];
        let mut cv = self.svert[first];
        let mut cse = first;
        let mut ss = self.next[first];
        while ss != se {
            let v = self.svert[ss];
            if filtered_incircle(self.fat(a), self.fat(b), self.fat(cv), self.fat(v)) > 0 {
                cv = v;
                cse = ss;
            }
            ss = self.next[ss];
        }
        let mut ebc = NIL;
        let mut eca = NIL;
        if self.find_symedge_between(b, cv) == NIL {
            ebc = self.add_diagonal(self.next[se], cse);
        }
        if self.find_symedge_between(cv, a) == NIL {
            eca = self.add_diagonal(cse, se);
        }
        if ebc != NIL {
            self.re_delaunay_triangulate(2 * ebc + 1);
        }
        if eca != NIL {
            self.re_delaunay_triangulate(2 * eca + 1);
        }
    }

    fn fill_through_vert(&self, v: usize, cd_out: usize, cd: &mut CrossData, cd_next: &mut CrossData) {
        cd_next.lambda = Q::zero();
        cd_next.vert = v;
        cd_next.inn = NIL;
        cd_next.out = NIL;
        if cd.lambda.is_zero() {
            cd.out = cd_out;
        } else {
            let mut se = sym(cd.inn);
            if self.svert[se] != v {
                se = self.next[se];
                if self.svert[se] != v {
                    se = self.next[se];
                }
            }
            cd_next.inn = se;
        }
    }

    fn fill_for_intersect(&self, curco: &Fat, v2: usize, t: usize, cd: &mut CrossData, cd_next: &mut CrossData) {
        let va = self.svert[t];
        let vb = self.svert[self.next[t]];
        let se_vcvb = sym(self.next[t]);
        let se_vcva = self.next[self.next[t]];
        let (kind, lambda) = isect_seg_seg(&self.vfat[va].ex, &self.vfat[vb].ex, &curco.ex, &self.vfat[v2].ex);
        match kind {
            IKind::Cross => {
                *cd_next = CrossData { lambda, vert: NIL, inn: t, out: NIL };
                if cd.lambda.is_zero() {
                    cd.out = se_vcva;
                }
            }
            IKind::Exact => {
                if lambda.is_zero() {
                    self.fill_through_vert(va, se_vcva, cd, cd_next);
                } else if lambda == Q::one() {
                    self.fill_through_vert(vb, se_vcvb, cd, cd_next);
                } else {
                    *cd_next = CrossData { lambda, vert: NIL, inn: t, out: NIL };
                    if cd.lambda.is_zero() {
                        cd.out = se_vcva;
                    }
                }
            }
            IKind::None => {
                let half = Q::new(1.into(), 2.into());
                if lambda <= half {
                    self.fill_through_vert(va, se_vcva, cd, cd_next);
                } else {
                    self.fill_through_vert(vb, se_vcvb, cd, cd_next);
                }
            }
            IKind::Colinear => {
                let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]);
                if d2(self.vfat[va].ap, self.vfat[v2].ap) <= d2(self.vfat[vb].ap, self.vfat[v2].ap) {
                    self.fill_through_vert(va, se_vcva, cd, cd_next);
                } else {
                    self.fill_through_vert(vb, se_vcvb, cd, cd_next);
                }
            }
        }
    }

    fn next_crossing_from_vert(&self, cd: &mut CrossData, cd_next: &mut CrossData, v2: usize) -> bool {
        let tstart = self.vsym[cd.vert];
        let mut t = tstart;
        let vcur = cd.vert;
        loop {
            let va = self.svert[self.next[t]];
            let vb = self.svert[self.next[self.next[t]]];
            let orient1 = filtered_orient2d(self.fat(self.svert[t]), self.fat(va), self.fat(v2));
            if orient1 == 0 && in_line(self.fat(vcur), self.fat(va), self.fat(v2)) {
                self.fill_through_vert(va, t, cd, cd_next);
                return true;
            }
            if self.sface[t] != self.outer {
                let orient2 = filtered_orient2d(self.fat(vcur), self.fat(vb), self.fat(v2));
                if orient1 > 0 && orient2 < 0 {
                    let tt = self.next[t];
                    let cur = self.vfat[vcur].clone();
                    self.fill_for_intersect(&cur, v2, tt, cd, cd_next);
                    return true;
                }
            }
            t = self.rot[t];
            if t == tstart {
                return false;
            }
        }
    }

    fn next_crossing_from_edge(&self, cd: &mut CrossData, cd_next: &mut CrossData, v2: usize) {
        let va = self.svert[cd.inn];
        let vb = self.svert[self.next[cd.inn]];
        let (a, b) = (&self.vfat[va].ex, &self.vfat[vb].ex);
        let l = &cd.lambda;
        let one_m = Q::one() - l;
        let curco = [&a[0] * &one_m + &b[0] * l, &a[1] * &one_m + &b[1] * l];
        let fat_cur = Fat::new(curco);
        let se_ac = self.next[sym(cd.inn)];
        let vc = self.svert[self.next[se_ac]];
        let o = filtered_orient2d(&fat_cur, self.fat(v2), self.fat(vc));
        if o < 0 {
            self.fill_for_intersect(&fat_cur, v2, self.next[se_ac], cd, cd_next);
        } else if o > 0 {
            self.fill_for_intersect(&fat_cur, v2, se_ac, cd, cd_next);
        } else {
            *cd_next = CrossData { lambda: Q::zero(), vert: vc, inn: self.next[se_ac], out: NIL };
        }
    }

    /// Returns the list of edges (CDT edge indices) from v1 to v2 in order.
    fn add_edge_constraint(&mut self, v1: usize, v2: usize, input_id: i32) -> Vec<usize> {
        let mut edge_list: Vec<usize> = vec![];
        if v1 == v2 {
            return edge_list;
        }
        let t0 = self.find_symedge_between(v1, v2);
        if t0 != NIL {
            let e = eof(t0);
            self.eids[e].add(input_id);
            edge_list.push(e);
            return edge_list;
        }
        self.visit_count += 1;
        let visit = self.visit_count;
        let mut crossings: Vec<CrossData> = vec![CrossData { lambda: Q::zero(), vert: v1, inn: NIL, out: NIL }];
        loop {
            let n = crossings.len();
            if crossings[n - 1].vert == v2 {
                break;
            }
            let mut cd = crossings[n - 1].clone();
            let mut cd_next = CrossData::new();
            let ok;
            if cd.lambda.is_zero() {
                ok = self.next_crossing_from_vert(&mut cd, &mut cd_next, v2);
            } else {
                self.next_crossing_from_edge(&mut cd, &mut cd_next, v2);
                ok = true;
            }
            crossings[n - 1] = cd;
            crossings.push(cd_next);
            if !ok || crossings.len() == 100000 {
                return edge_list;
            }
            if crossings[n].lambda.is_zero() {
                let v = crossings[n].vert;
                if self.vvisit[v] == visit {
                    return edge_list;
                }
                self.vvisit[v] = visit;
            }
        }
        let nc = crossings.len();
        let m1 = qi(-1);
        for i in 2..nc {
            if crossings[i].lambda.is_zero() {
                let v = crossings[i].vert;
                let mut j = i - 1;
                while j > 0 {
                    let cp = &crossings[j];
                    if (cp.lambda.is_zero() && cp.vert != v) || (!cp.lambda.is_zero() && self.svert[cp.inn] != v && self.svert[self.next[cp.inn]] != v) {
                        break;
                    }
                    crossings[j].lambda = m1.clone();
                    j -= 1;
                }
                if j < i - 1 {
                    let (cp_lambda_zero, cp_vert, cp_inn) = (crossings[j].lambda.is_zero(), crossings[j].vert, crossings[j].inn);
                    if cp_lambda_zero {
                        let se = self.find_symedge_between(cp_vert, v);
                        if se == NIL {
                            return edge_list;
                        }
                        crossings[j].out = se;
                        crossings[i].inn = NIL;
                    } else {
                        let se = self.find_symedge_with_face(v, self.sface[sym(cp_inn)]);
                        if se == NIL {
                            return edge_list;
                        }
                        crossings[i].inn = se;
                    }
                }
            }
        }
        // split constrained edges
        for i in 0..nc {
            if !crossings[i].lambda.is_zero() && crossings[i].lambda != m1 && self.is_constrained(eof(crossings[i].inn)) {
                let lam = crossings[i].lambda.clone();
                let e = self.split_edge(crossings[i].inn, &lam);
                crossings[i].vert = self.svert[2 * e];
            }
        }
        // remove crossed non-constrained edges
        for i in 0..nc {
            if !crossings[i].lambda.is_zero() && crossings[i].lambda != m1 && !self.is_constrained(eof(crossings[i].inn)) {
                self.delete_edge(crossings[i].inn);
            }
        }
        // insert segments
        let mut tstart = crossings[0].out;
        for i in 1..nc {
            if crossings[i].lambda == m1 {
                continue;
            }
            let mut t = NIL;
            let mut tnext = NIL;
            let mut edge;
            if !crossings[i].lambda.is_zero() {
                if self.is_constrained(eof(crossings[i].inn)) {
                    t = self.vsym[crossings[i].vert];
                    tnext = self.next[sym(t)];
                }
            } else {
                t = crossings[i].inn;
                tnext = crossings[i].out;
                if t == NIL {
                    let mut j = i as i64 - 1;
                    while j >= 0 {
                        if crossings[j as usize].lambda != m1 {
                            break;
                        }
                        j -= 1;
                    }
                    let cp_out = crossings[j.max(0) as usize].out;
                    edge = eof(cp_out);
                    self.eids[edge].add(input_id);
                    edge_list.push(edge);
                }
            }
            if t != NIL {
                if self.svert[self.next[tstart]] == self.svert[t] {
                    edge = eof(tstart);
                } else {
                    edge = self.add_diagonal(tstart, t);
                }
                self.eids[edge].add(input_id);
                edge_list.push(edge);
                self.re_delaunay_triangulate(2 * edge);
                self.re_delaunay_triangulate(2 * edge + 1);
            }
            if i < nc - 1 && tnext != NIL {
                tstart = tnext;
            }
        }
        edge_list
    }

    fn add_edge_constraints(&mut self, input: &CdtInput) {
        let ne = input.edge.len();
        let nv = input.vert.len();
        for (i, &(iv1, iv2)) in input.edge.iter().enumerate() {
            if iv1 >= nv || iv2 >= nv {
                continue;
            }
            let v1 = self.resolve(iv1);
            let v2 = self.resolve(iv2);
            self.add_edge_constraint(v1, v2, i as i32);
        }
        self.face_edge_offset = ne as i32;
    }

    fn add_face_ids(&mut self, face_symedge: usize, face_id: i32, fedge_start: i32, fedge_end: i32) {
        self.visit_count += 1;
        let visit = self.visit_count;
        let mut stack = vec![face_symedge];
        while let Some(se0) = stack.pop() {
            let face = self.sface[se0];
            if self.fvisit[face] == visit {
                continue;
            }
            self.fvisit[face] = visit;
            self.fids[face].add(face_id);
            let mut se = self.next[se0];
            while se != se0 {
                let in_range = self.eids[eof(se)].iter().any(|&id| id >= fedge_start && id <= fedge_end);
                if !in_range {
                    let ss = sym(se);
                    if self.fvisit[self.sface[ss]] != visit {
                        stack.push(ss);
                    }
                }
                se = self.next[se];
            }
        }
    }

    fn add_face_constraints(&mut self, input: &CdtInput) -> usize {
        let nv = input.vert.len();
        let mut maxflen = 0;
        for f in &input.face {
            maxflen = maxflen.max(f.len());
        }
        let want = (maxflen as i32).max(self.face_edge_offset);
        let mut off = 1;
        if want > 0 {
            while off < want {
                off *= 10;
            }
        }
        self.face_edge_offset = off;
        let mut added = 0;
        let mut face_symedge0 = NIL;
        for (f, face) in input.face.iter().enumerate() {
            if face.len() <= 2 {
                continue;
            }
            let fedge_start = (f as i32 + 1) * self.face_edge_offset;
            for i in 0..face.len() {
                let face_edge_id = fedge_start + i as i32;
                let iv1 = face[i];
                let iv2 = face[(i + 1) % face.len()];
                if iv1 >= nv || iv2 >= nv {
                    continue;
                }
                added += 1;
                let v1 = self.resolve(iv1);
                let v2 = self.resolve(iv2);
                let el = self.add_edge_constraint(v1, v2, face_edge_id);
                if let Some(&fe) = el.first() {
                    face_symedge0 = 2 * fe;
                    if self.svert[face_symedge0] != v1 {
                        face_symedge0 = 2 * fe + 1;
                    }
                }
            }
            let fedge_end = fedge_start + face.len() as i32 - 1;
            if face_symedge0 != NIL {
                self.add_face_ids(face_symedge0, f as i32, fedge_start, fedge_end);
                self.add_face_ids(face_symedge0, f as i32, fedge_start, fedge_end);
            }
        }
        added
    }
    fn resolve(&self, i: usize) -> usize {
        if self.vmerge[i] != -1 {
            self.vmerge[i] as usize
        } else {
            i
        }
    }

    // ---------------------------------------------------------------- output preparation (CDT_INSIDE)
    fn dissolve_symedge(&mut self, se0: usize) {
        let mut se = se0;
        let mut symse = sym(se);
        if self.sface[symse] == self.outer {
            se = sym(se);
            symse = sym(se);
        }
        let of = self.outer;
        if self.fsym[of] == se || self.fsym[of] == symse {
            if self.next[self.next[se]] == se {
                self.fsym[of] = NIL;
            } else {
                self.fsym[of] = self.next[self.next[se]];
            }
        } else {
            let f1 = self.sface[se];
            if self.fsym[f1] == se {
                self.fsym[f1] = self.next[se];
            }
            let f2 = self.sface[symse];
            if self.fsym[f2] == symse {
                self.fsym[f2] = self.next[symse];
            }
        }
        self.delete_edge(se);
    }

    fn remove_outer_edges_until_constraints(&mut self) {
        self.visit_count += 1;
        let visit = self.visit_count;
        let of = self.outer;
        self.fvisit[of] = visit;
        let mut fstack: Vec<usize> = vec![];
        let se_start = self.fsym[of];
        let mut se = se_start;
        loop {
            if !self.is_constrained(eof(se)) {
                let fs = self.sface[sym(se)];
                if self.fvisit[fs] != visit {
                    fstack.push(fs);
                }
            }
            se = self.next[se];
            if se == se_start {
                break;
            }
        }
        while let Some(f) = fstack.pop() {
            if self.fvisit[f] == visit {
                continue;
            }
            self.fvisit[f] = visit;
            let se_start = self.fsym[f];
            let mut se = se_start;
            let mut to_dissolve: Vec<usize> = vec![];
            loop {
                if !self.is_constrained(eof(se)) {
                    let fs = self.sface[sym(se)];
                    if self.fvisit[fs] != visit {
                        fstack.push(fs);
                    } else {
                        to_dissolve.push(se);
                    }
                }
                se = self.next[se];
                if se == se_start {
                    break;
                }
            }
            while let Some(se) = to_dissolve.pop() {
                if self.next[se] != NIL {
                    self.dissolve_symedge(se);
                }
            }
        }
    }

    fn prepare_for_output(&mut self) {
        if self.eids.is_empty() {
            return;
        }
        for e in 0..self.eids.len() {
            if !self.is_deleted(e) {
                for k in 0..2 {
                    let f = self.sface[2 * e + k];
                    if self.fsym[f] == NIL {
                        self.fsym[f] = 2 * e + k;
                    }
                }
            }
        }
        self.remove_outer_edges_until_constraints();
    }
}

/// `delaunay_2d_calc(input, CDT_INSIDE)` for exact rationals with `need_ids = true`.
pub fn delaunay_2d_calc_inside(input: &CdtInput) -> CdtOutput {
    let mut d = Cdt {
        vfat: vec![],
        vsym: vec![],
        vmerge: vec![],
        vvisit: vec![],
        vids: vec![],
        next: vec![],
        rot: vec![],
        svert: vec![],
        sface: vec![],
        eids: vec![],
        fsym: vec![],
        fids: vec![],
        fvisit: vec![],
        fdel: vec![],
        outer: 0,
        visit_count: 0,
        input_vert_num: input.vert.len(),
        face_edge_offset: 0,
    };
    d.outer = d.add_face();
    for p in &input.vert {
        d.add_vert(p.clone());
    }
    d.initial_triangulation();
    d.add_edge_constraints(input);
    let _added = d.add_face_constraints(input);
    // (output type stays CDT_INSIDE)
    d.prepare_for_output();

    let mut res = CdtOutput::default();
    res.face_edge_offset = d.face_edge_offset;
    let nvt = d.vfat.len();
    let mut map = vec![0usize; nvt];
    let mut nv = 0;
    for i in 0..nvt {
        if d.vmerge[i] == -1 {
            map[i] = nv;
            nv += 1;
        }
    }
    if nv == 0 {
        return res;
    }
    if nv < nvt {
        for i in 0..nvt {
            if d.vmerge[i] != -1 {
                let to = d.vmerge[i] as usize;
                if i < d.input_vert_num {
                    d.vids[to].add(i as i32);
                }
                map[i] = map[to];
            }
        }
    }
    for i in 0..nvt {
        if d.vmerge[i] == -1 {
            res.vert.push(d.vfat[i].ex.clone());
            let mut vo: Vec<i32> = vec![];
            if i < d.input_vert_num {
                vo.push(i as i32);
            }
            for &x in d.vids[i].iter() {
                vo.push(x);
            }
            res.vert_orig.push(vo);
        }
    }
    for e in 0..d.eids.len() {
        if !d.is_deleted(e) {
            res.edge.push((map[d.svert[2 * e]], map[d.svert[2 * e + 1]]));
            res.edge_orig.push(d.eids[e].iter().copied().collect());
        }
    }
    for f in 0..d.fsym.len() {
        if !d.fdel[f] && f != d.outer {
            let se_start = d.fsym[f];
            let mut face = vec![];
            let mut se = se_start;
            loop {
                face.push(map[d.svert[se]]);
                se = d.next[se];
                if se == se_start {
                    break;
                }
            }
            res.face.push(face);
            res.face_orig.push(d.fids[f].iter().copied().collect());
        }
    }
    res
}

#[allow(dead_code)]
fn _unused() {
    let _ = Q::abs;
    let _ = <Q as Signed>::is_positive;
}
