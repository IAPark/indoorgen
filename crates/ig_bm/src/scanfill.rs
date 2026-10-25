//! Port of Blender 4.2 `BLI_scanfill` (scanfill.c, flags CALC_POLYS | CALC_HOLES | CALC_LOOSE) as used by the bmesh
//! `triangle_fill` operator (bmo_triangle_fill_exec).  f32 math, list orders preserved so the triangle order is Blender's.
use super::V3;

const NIL: usize = usize::MAX;
const SF_EPSILON: f32 = 0.00003;
const SF_EPSILON_SQ: f32 = SF_EPSILON * SF_EPSILON;
const SF_POLY_UNSET: u16 = 0xffff;
const SF_VERT_NEW: u8 = 0;
const SF_VERT_AVAILABLE: u8 = 1;
const SF_EDGE_NEW: u8 = 0;
const SF_EDGE_INTERNAL: u8 = 2;
const SF_POLY_NEW: u8 = 0;
const SF_POLY_VALID: u8 = 1;

#[derive(Clone, Copy)]
struct Lb {
    first: usize,
    last: usize,
}
const EMPTY: Lb = Lb { first: NIL, last: NIL };

#[derive(Default)]
struct Links {
    n: Vec<usize>,
    p: Vec<usize>,
}
impl Links {
    fn push(&mut self) {
        self.n.push(NIL);
        self.p.push(NIL);
    }
    fn addtail(&mut self, lb: &mut Lb, id: usize) {
        self.n[id] = NIL;
        self.p[id] = lb.last;
        if lb.last != NIL {
            self.n[lb.last] = id;
        }
        if lb.first == NIL {
            lb.first = id;
        }
        lb.last = id;
    }
    fn remlink(&mut self, lb: &mut Lb, id: usize) {
        if self.n[id] != NIL {
            let n = self.n[id];
            self.p[n] = self.p[id];
        }
        if self.p[id] != NIL {
            let p = self.p[id];
            self.n[p] = self.n[id];
        }
        if lb.last == id {
            lb.last = self.p[id];
        }
        if lb.first == id {
            lb.first = self.n[id];
        }
    }
    fn insert_before(&mut self, lb: &mut Lb, next: usize, new: usize) {
        if next == NIL {
            return self.addtail(lb, new);
        }
        if lb.first == NIL {
            lb.first = new;
            lb.last = new;
            self.n[new] = NIL;
            self.p[new] = NIL;
            return;
        }
        self.p[new] = self.p[next];
        self.n[new] = next;
        if self.p[next] != NIL {
            let p = self.p[next];
            self.n[p] = new;
        }
        self.p[next] = new;
        if lb.first == next {
            lb.first = new;
        }
    }
    /// BLI_movelisttolist(dst, src)
    fn move_to(&mut self, dst: &mut Lb, src: &mut Lb) {
        if src.first == NIL {
            return;
        }
        if dst.first == NIL {
            *dst = *src;
        } else {
            self.n[dst.last] = src.first;
            self.p[src.first] = dst.last;
            dst.last = src.last;
        }
        *src = EMPTY;
    }
}

struct SfVert {
    co: V3,
    xy: [f32; 2],
    tmp: usize,
    poly_nr: u16,
    edge_count: u32,
    f: u8,
}
struct SfEdge {
    v1: usize,
    v2: usize,
    poly_nr: u16,
    f: u8,
}
#[derive(Clone, Copy)]
struct PolyFill {
    edges: u32,
    verts: u32,
    min_xy: [f32; 2],
    max_xy: [f32; 2],
    nr: u16,
    f: u8,
}
#[derive(Clone, Copy)]
struct ScLink {
    vert: usize,
    lb: Lb,
}

struct Ctx {
    v: Vec<SfVert>,
    e: Vec<SfEdge>,
    vl: Links,
    el: Links,
    vb: Lb,
    eb: Lb,
    faces: Vec<[usize; 3]>,
    poly_nr: u16,
}

fn compare_v2v2(a: [f32; 2], b: [f32; 2], limit: f32) -> bool {
    (a[0] - b[0]).abs() < limit && (a[1] - b[1]).abs() < limit
}
fn testedgeside(v1: [f32; 2], v2: [f32; 2], v3: [f32; 2]) -> bool {
    let inp = (v2[0] - v1[0]) * (v1[1] - v3[1]) + (v1[1] - v2[1]) * (v1[0] - v3[0]);
    if inp < 0.0 {
        return false;
    }
    if inp == 0.0 {
        if v1[0] == v3[0] && v1[1] == v3[1] {
            return false;
        }
        if v2[0] == v3[0] && v2[1] == v3[1] {
            return false;
        }
    }
    true
}
fn cos_v2v2v2(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> f32 {
    let mut v1 = [p2[0] - p1[0], p2[1] - p1[1]];
    let mut v2 = [p2[0] - p3[0], p2[1] - p3[1]];
    for v in [&mut v1, &mut v2] {
        let d = v[0] * v[0] + v[1] * v[1];
        if d > 1.0e-35f32 {
            let f = 1.0f32 / d.sqrt();
            v[0] *= f;
            v[1] *= f;
        } else {
            *v = [0.0, 0.0];
        }
    }
    v1[0] * v2[0] + v1[1] * v2[1]
}
fn dist_squared_to_line_v2(l1: [f32; 2], l2: [f32; 2], p: [f32; 2]) -> f32 {
    let u = [l2[0] - l1[0], l2[1] - l1[1]];
    let h = [p[0] - l1[0], p[1] - l1[1]];
    let uu = u[0] * u[0] + u[1] * u[1];
    let lambda = if uu != 0.0 { (u[0] * h[0] + u[1] * h[1]) / uu } else { 0.0 };
    let c = [l1[0] + u[0] * lambda, l1[1] + u[1] * lambda];
    (c[0] - p[0]) * (c[0] - p[0]) + (c[1] - p[1]) * (c[1] - p[1])
}
fn vergscdata(ctx_v: &[SfVert], a: usize, b: usize) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let (x1, x2) = (&ctx_v[a], &ctx_v[b]);
    if x1.xy[1] < x2.xy[1] {
        return Greater;
    }
    if x1.xy[1] > x2.xy[1] {
        return Less;
    }
    if x1.xy[0] > x2.xy[0] {
        return Greater;
    }
    if x1.xy[0] < x2.xy[0] {
        return Less;
    }
    Equal
}

impl Ctx {
    fn vert_add(&mut self, co: V3) -> usize {
        self.v.push(SfVert { co, xy: [0.0; 2], tmp: NIL, poly_nr: self.poly_nr, edge_count: 0, f: SF_VERT_NEW });
        self.vl.push();
        let id = self.v.len() - 1;
        let mut vb = self.vb;
        self.vl.addtail(&mut vb, id);
        self.vb = vb;
        id
    }
    fn edge_add(&mut self, v1: usize, v2: usize) -> usize {
        self.e.push(SfEdge { v1, v2, poly_nr: self.poly_nr, f: SF_EDGE_NEW });
        self.el.push();
        let id = self.e.len() - 1;
        let mut eb = self.eb;
        self.el.addtail(&mut eb, id);
        self.eb = eb;
        id
    }
    fn mergepolys(&mut self, pf1: usize, pf2: usize, pfl: &mut [PolyFill]) {
        let (n1, n2) = (pfl[pf1].nr, pfl[pf2].nr);
        let mut v = self.vb.first;
        while v != NIL {
            if self.v[v].poly_nr == n2 {
                self.v[v].poly_nr = n1;
            }
            v = self.vl.n[v];
        }
        let mut e = self.eb.first;
        while e != NIL {
            if self.e[e].poly_nr == n2 {
                self.e[e].poly_nr = n1;
            }
            e = self.el.n[e];
        }
        let p2 = pfl[pf2];
        let p1 = &mut pfl[pf1];
        p1.verts += p2.verts;
        p1.edges += p2.edges;
        p1.max_xy[0] = p1.max_xy[0].max(p2.max_xy[0]);
        p1.max_xy[1] = p1.max_xy[1].max(p2.max_xy[1]);
        p1.min_xy[0] = p1.min_xy[0].min(p2.min_xy[0]);
        p1.min_xy[1] = p1.min_xy[1].min(p2.min_xy[1]);
        p1.f |= p2.f;
        pfl[pf2].verts = 0;
        pfl[pf2].edges = 0;
    }

    /// addedgetoscanvert: false when the edge already exists
    fn addedgetoscanvert(&mut self, sc: &mut ScLink, eed: usize) -> bool {
        if sc.lb.first == NIL {
            sc.lb.first = eed;
            sc.lb.last = eed;
            self.el.p[eed] = NIL;
            self.el.n[eed] = NIL;
            return true;
        }
        let (e1, e2) = (self.e[eed].v1, self.e[eed].v2);
        let (x, y) = (self.v[e1].xy[0], self.v[e1].xy[1]);
        let mut fac1 = self.v[e2].xy[1] - y;
        if fac1 == 0.0 {
            fac1 = 1.0e10f32 * (self.v[e2].xy[0] - x);
        } else {
            fac1 = (x - self.v[e2].xy[0]) / fac1;
        }
        let mut ed = sc.lb.first;
        while ed != NIL {
            if self.e[ed].v2 == e2 {
                return false;
            }
            let v2 = self.e[ed].v2;
            let mut fac = self.v[v2].xy[1] - y;
            if fac == 0.0 {
                fac = 1.0e10f32 * (self.v[v2].xy[0] - x);
            } else {
                fac = (x - self.v[v2].xy[0]) / fac;
            }
            if fac > fac1 {
                break;
            }
            ed = self.el.n[ed];
        }
        if ed != NIL {
            self.el.insert_before(&mut sc.lb, ed, eed);
        } else {
            self.el.addtail(&mut sc.lb, eed);
        }
        true
    }

    /// returns the scan index when the edge already exists
    fn addedgetoscanlist(&mut self, scdata: &mut [ScLink], eed: usize) -> Option<usize> {
        let (a, b) = (self.e[eed].v1, self.e[eed].v2);
        if self.v[a].xy[1] == self.v[b].xy[1] {
            if self.v[a].xy[0] > self.v[b].xy[0] {
                self.e[eed].v1 = b;
                self.e[eed].v2 = a;
            }
        } else if self.v[a].xy[1] < self.v[b].xy[1] {
            self.e[eed].v1 = b;
            self.e[eed].v2 = a;
        }
        let key = self.e[eed].v1;
        let idx = scdata.partition_point(|s| vergscdata(&self.v, s.vert, key) == std::cmp::Ordering::Less);
        if idx >= scdata.len() || vergscdata(&self.v, scdata[idx].vert, key) != std::cmp::Ordering::Equal {
            return None;
        }
        let mut sc = scdata[idx];
        let ok = self.addedgetoscanvert(&mut sc, eed);
        scdata[idx] = sc;
        if !ok {
            Some(idx)
        } else {
            None
        }
    }

    fn boundinside_ev(&self, eed: usize, eve: usize) -> bool {
        let (a, b) = (&self.v[self.e[eed].v1], &self.v[self.e[eed].v2]);
        let (minx, maxx) = if a.xy[0] < b.xy[0] { (a.xy[0], b.xy[0]) } else { (b.xy[0], a.xy[0]) };
        let p = &self.v[eve];
        if p.xy[0] >= minx && p.xy[0] <= maxx {
            let (miny, maxy) = if a.xy[1] < b.xy[1] { (a.xy[1], b.xy[1]) } else { (b.xy[1], a.xy[1]) };
            if p.xy[1] >= miny && p.xy[1] <= maxy {
                return true;
            }
        }
        false
    }

    fn testvertexnearedge(&mut self) {
        let mut eve = self.vb.first;
        while eve != NIL {
            if self.v[eve].edge_count == 1 {
                let mut ed1 = self.eb.first;
                while !(self.e[ed1].v1 == eve || self.e[ed1].v2 == eve) {
                    ed1 = self.el.n[ed1];
                }
                if self.e[ed1].v1 == eve {
                    self.e[ed1].v1 = self.e[ed1].v2;
                    self.e[ed1].v2 = eve;
                }
                let mut eed = self.eb.first;
                while eed != NIL {
                    let (a, b) = (self.e[eed].v1, self.e[eed].v2);
                    if eve != a && eve != b && self.v[eve].poly_nr == self.e[eed].poly_nr {
                        if compare_v2v2(self.v[eve].xy, self.v[a].xy, SF_EPSILON) {
                            self.e[ed1].v2 = a;
                            self.v[a].edge_count += 1;
                            self.v[eve].edge_count = 0;
                            break;
                        }
                        if compare_v2v2(self.v[eve].xy, self.v[b].xy, SF_EPSILON) {
                            self.e[ed1].v2 = b;
                            self.v[b].edge_count += 1;
                            self.v[eve].edge_count = 0;
                            break;
                        }
                        if self.boundinside_ev(eed, eve) {
                            let dist = dist_squared_to_line_v2(self.v[a].xy, self.v[b].xy, self.v[eve].xy);
                            if dist < SF_EPSILON_SQ {
                                let n = self.edge_add(a, eve);
                                ed1 = n;
                                self.e[n].poly_nr = self.e[eed].poly_nr;
                                self.e[eed].v1 = eve;
                                self.v[eve].edge_count = 3;
                                break;
                            }
                        }
                    }
                    eed = self.el.n[eed];
                }
            }
            eve = self.vl.n[eve];
        }
    }

    fn splitlist(&mut self, tempve: &mut Lb, temped: &mut Lb, nr: u16) {
        let mut vb = self.vb;
        self.vl.move_to(tempve, &mut vb);
        self.vb = vb;
        let mut eb = self.eb;
        self.el.move_to(temped, &mut eb);
        self.eb = eb;
        let mut eve = tempve.first;
        while eve != NIL {
            let next = self.vl.n[eve];
            if self.v[eve].poly_nr == nr {
                self.vl.remlink(tempve, eve);
                let mut vb = self.vb;
                self.vl.addtail(&mut vb, eve);
                self.vb = vb;
            }
            eve = next;
        }
        let mut eed = temped.first;
        while eed != NIL {
            let next = self.el.n[eed];
            if self.e[eed].poly_nr == nr {
                self.el.remlink(temped, eed);
                let mut eb = self.eb;
                self.el.addtail(&mut eb, eed);
                self.eb = eb;
            }
            eed = next;
        }
    }

    fn eb_addtail(&mut self, id: usize) {
        let mut eb = self.eb;
        self.el.addtail(&mut eb, id);
        self.eb = eb;
    }
    fn eb_remlink(&mut self, id: usize) {
        let mut eb = self.eb;
        self.el.remlink(&mut eb, id);
        self.eb = eb;
    }

    fn scanfill(&mut self, pf: &PolyFill) -> u32 {
        let nr = pf.nr;
        // STEP 1
        let mut scdata: Vec<ScLink> = vec![];
        let mut eve = self.vb.first;
        while eve != NIL {
            if self.v[eve].poly_nr == nr {
                self.v[eve].f = SF_VERT_NEW;
                scdata.push(ScLink { vert: eve, lb: EMPTY });
            }
            eve = self.vl.n[eve];
        }
        let verts = scdata.len();
        {
            let vv = &self.v;
            scdata.sort_by(|a, b| vergscdata(vv, a.vert, b.vert));
        }
        let mut eed = self.eb.first;
        while eed != NIL {
            let next = self.el.n[eed];
            self.eb_remlink(eed);
            if self.e[eed].v1 != self.e[eed].v2 {
                self.addedgetoscanlist(&mut scdata, eed);
            }
            eed = next;
        }
        // STEP 2
        let twoconnected = pf.f == SF_POLY_NEW;
        let mut totface: u32 = 0;
        let maxface = (2 * verts) as u32;
        let mut a = 0usize;
        while a < verts {
            // set connect flags
            let mut ed1 = scdata[a].lb.first;
            while ed1 != NIL {
                let nxt = self.el.n[ed1];
                let (v1, v2) = (self.e[ed1].v1, self.e[ed1].v2);
                if self.v[v1].edge_count == 1 || self.v[v2].edge_count == 1 {
                    let mut lb = scdata[a].lb;
                    self.el.remlink(&mut lb, ed1);
                    scdata[a].lb = lb;
                    self.eb_addtail(ed1);
                    if self.v[v1].edge_count > 1 {
                        self.v[v1].edge_count -= 1;
                    }
                    if self.v[v2].edge_count > 1 {
                        self.v[v2].edge_count -= 1;
                    }
                } else {
                    self.v[v2].f = SF_VERT_AVAILABLE;
                }
                ed1 = nxt;
            }
            while scdata[a].lb.first != NIL {
                let ed1 = scdata[a].lb.first;
                let ed2 = self.el.n[ed1];
                if totface >= maxface {
                    a = verts;
                    break;
                }
                if ed2 == NIL {
                    scdata[a].lb = EMPTY;
                    self.eb_addtail(ed1);
                    let (v1, v2) = (self.e[ed1].v1, self.e[ed1].v2);
                    self.v[v2].f = SF_VERT_NEW;
                    self.v[v1].edge_count = self.v[v1].edge_count.wrapping_sub(1);
                    self.v[v2].edge_count = self.v[v2].edge_count.wrapping_sub(1);
                } else {
                    let mut best_sc: Option<usize> = None;
                    let mut angle_best_cos = -1.0f32;
                    let mut firsttime = false;
                    let v1 = self.e[ed1].v2;
                    let v2 = self.e[ed1].v1;
                    let v3 = self.e[ed2].v2;
                    if v1 == v2 || v2 == v3 {
                        break;
                    }
                    let (x1, x2, x3) = (self.v[v1].xy, self.v[v2].xy, self.v[v3].xy);
                    let miny = x1[1].min(x3[1]);
                    for b in (a + 1)..verts {
                        let sv = scdata[b].vert;
                        if self.v[sv].f == SF_VERT_NEW {
                            let p = self.v[sv].xy;
                            if p[1] <= miny {
                                break;
                            }
                            if testedgeside(x1, x2, p) && testedgeside(x2, x3, p) && testedgeside(x3, x1, p) {
                                match best_sc {
                                    None => best_sc = Some(b),
                                    Some(bs) => {
                                        let bp = self.v[scdata[bs].vert].xy;
                                        if !firsttime {
                                            angle_best_cos = cos_v2v2v2(x2, x1, bp);
                                            firsttime = true;
                                        }
                                        let angle_test_cos = cos_v2v2v2(x2, x1, p);
                                        if angle_test_cos > angle_best_cos {
                                            best_sc = Some(b);
                                            angle_best_cos = angle_test_cos;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(bs) = best_sc {
                        let ed3 = self.edge_add(v2, scdata[bs].vert);
                        self.eb_remlink(ed3);
                        let mut lb = scdata[a].lb;
                        self.el.insert_before(&mut lb, ed2, ed3);
                        scdata[a].lb = lb;
                        let t2 = self.e[ed3].v2;
                        self.v[t2].f = SF_VERT_AVAILABLE;
                        self.e[ed3].f = SF_EDGE_INTERNAL;
                        let t1 = self.e[ed3].v1;
                        self.v[t1].edge_count += 1;
                        self.v[t2].edge_count += 1;
                    } else {
                        self.faces.push([v1, v2, v3]);
                        totface += 1;
                        let mut lb = scdata[a].lb;
                        self.el.remlink(&mut lb, ed1);
                        scdata[a].lb = lb;
                        self.eb_addtail(ed1);
                        let (e1v1, e1v2) = (self.e[ed1].v1, self.e[ed1].v2);
                        self.v[e1v2].f = SF_VERT_NEW;
                        self.v[e1v1].edge_count = self.v[e1v1].edge_count.wrapping_sub(1);
                        self.v[e1v2].edge_count = self.v[e1v2].edge_count.wrapping_sub(1);
                        if self.e[ed2].f == SF_EDGE_NEW && twoconnected {
                            let mut lb = scdata[a].lb;
                            self.el.remlink(&mut lb, ed2);
                            scdata[a].lb = lb;
                            self.eb_addtail(ed2);
                            let (a2, b2) = (self.e[ed2].v1, self.e[ed2].v2);
                            self.v[b2].f = SF_VERT_NEW;
                            self.v[a2].edge_count = self.v[a2].edge_count.wrapping_sub(1);
                            self.v[b2].edge_count = self.v[b2].edge_count.wrapping_sub(1);
                        }
                        let ed3 = self.edge_add(v1, v3);
                        self.eb_remlink(ed3);
                        self.e[ed3].f = SF_EDGE_INTERNAL;
                        self.v[v1].edge_count += 1;
                        self.v[v3].edge_count += 1;
                        if let Some(sc1) = self.addedgetoscanlist(&mut scdata, ed3) {
                            self.v[v1].edge_count -= 1;
                            self.v[v3].edge_count -= 1;
                            let mut e3 = scdata[sc1].lb.first;
                            while e3 != NIL {
                                let (p, q) = (self.e[e3].v1, self.e[e3].v2);
                                if (p == v1 && q == v3) || (p == v3 && q == v1) {
                                    if twoconnected {
                                        let mut lb = scdata[sc1].lb;
                                        self.el.remlink(&mut lb, e3);
                                        scdata[sc1].lb = lb;
                                        self.eb_addtail(e3);
                                        self.v[p].edge_count = self.v[p].edge_count.wrapping_sub(1);
                                        self.v[q].edge_count = self.v[q].edge_count.wrapping_sub(1);
                                    }
                                    break;
                                }
                                e3 = self.el.n[e3];
                            }
                        }
                    }
                }
                // loose edges
                let mut ed1 = scdata[a].lb.first;
                while ed1 != NIL {
                    let nxt = self.el.n[ed1];
                    let (v1, v2) = (self.e[ed1].v1, self.e[ed1].v2);
                    if self.v[v1].edge_count < 2 || self.v[v2].edge_count < 2 {
                        let mut lb = scdata[a].lb;
                        self.el.remlink(&mut lb, ed1);
                        scdata[a].lb = lb;
                        self.eb_addtail(ed1);
                        if self.v[v1].edge_count > 1 {
                            self.v[v1].edge_count -= 1;
                        }
                        if self.v[v2].edge_count > 1 {
                            self.v[v2].edge_count -= 1;
                        }
                    }
                    ed1 = nxt;
                }
            }
            a += 1;
        }
        totface
    }

    fn calc(&mut self, nor_proj: V3) -> u32 {
        let flag_polys = true;
        // first test vertices if they are in edges, reset flags
        let mut eed = self.eb.first;
        while eed != NIL {
            let (a, b) = (self.e[eed].v1, self.e[eed].v2);
            self.v[a].f = SF_VERT_AVAILABLE;
            self.v[b].f = SF_VERT_AVAILABLE;
            eed = self.el.n[eed];
        }
        let mut eve = self.vb.first;
        while eve != NIL {
            if self.v[eve].f == SF_VERT_AVAILABLE {
                break;
            }
            eve = self.vl.n[eve];
        }
        if eve == NIL {
            return 0;
        }
        let mut n = nor_proj;
        let d = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        if d > 1.0e-35f32 {
            let f = 1.0f32 / d.sqrt();
            n = [n[0] * f, n[1] * f, n[2] * f];
        } else {
            return 0;
        }
        // axis_dominant_v3_to_m3_negate
        let r2 = [-n[0], -n[1], -n[2]];
        let (r0, r1) = crate::polyfill::ortho_basis(r2);
        let proj = |c: V3| -> [f32; 2] { [r0[0] * c[0] + r0[1] * c[1] + r0[2] * c[2], r1[0] * c[0] + r1[1] * c[1] + r1[2] * c[2]] };
        // STEP 1: count polys
        let mut poly: u16 = 0;
        if self.poly_nr != SF_POLY_UNSET {
            poly = self.poly_nr + 1;
            self.poly_nr = SF_POLY_UNSET;
        }
        if flag_polys && poly == 0 {
            let mut eve = self.vb.first;
            while eve != NIL {
                self.v[eve].xy = proj(self.v[eve].co);
                if self.v[eve].poly_nr == SF_POLY_UNSET {
                    let mut toggle = 0u32;
                    let mut ok = true;
                    self.v[eve].poly_nr = poly;
                    while ok {
                        ok = false;
                        toggle += 1;
                        let mut eed = if toggle & 1 != 0 { self.eb.first } else { self.eb.last };
                        while eed != NIL {
                            let (a, b) = (self.e[eed].v1, self.e[eed].v2);
                            if self.v[a].poly_nr == SF_POLY_UNSET && self.v[b].poly_nr == poly {
                                self.v[a].poly_nr = poly;
                                self.e[eed].poly_nr = poly;
                                ok = true;
                            } else if self.v[b].poly_nr == SF_POLY_UNSET && self.v[a].poly_nr == poly {
                                self.v[b].poly_nr = poly;
                                self.e[eed].poly_nr = poly;
                                ok = true;
                            } else if self.e[eed].poly_nr == SF_POLY_UNSET && self.v[a].poly_nr == poly && self.v[b].poly_nr == poly {
                                self.e[eed].poly_nr = poly;
                                ok = true;
                            }
                            eed = if toggle & 1 != 0 { self.el.n[eed] } else { self.el.p[eed] };
                        }
                    }
                    poly += 1;
                }
                eve = self.vl.n[eve];
            }
        }
        // STEP 2: remove loose edges and strings of edges
        {
            let mut toggle = 0u32;
            let mut eed = self.eb.first;
            let mut broke = false;
            while eed != NIL {
                let (a, b) = (self.e[eed].v1, self.e[eed].v2);
                let c = self.v[a].edge_count;
                self.v[a].edge_count += 1;
                if c > 250 {
                    broke = true;
                    break;
                }
                let c = self.v[b].edge_count;
                self.v[b].edge_count += 1;
                if c > 250 {
                    broke = true;
                    break;
                }
                eed = self.el.n[eed];
            }
            if broke {
                return 0;
            }
            self.testvertexnearedge();
            let mut ok = true;
            while ok {
                ok = false;
                toggle += 1;
                let mut eed = if toggle & 1 != 0 { self.eb.first } else { self.eb.last };
                while eed != NIL {
                    let nxt = if toggle & 1 != 0 { self.el.n[eed] } else { self.el.p[eed] };
                    let (a, b) = (self.e[eed].v1, self.e[eed].v2);
                    if self.v[a].edge_count == 1 {
                        self.v[b].edge_count -= 1;
                        let mut vb = self.vb;
                        self.vl.remlink(&mut vb, a);
                        self.vb = vb;
                        self.eb_remlink(eed);
                        ok = true;
                    } else if self.v[b].edge_count == 1 {
                        self.v[a].edge_count -= 1;
                        let mut vb = self.vb;
                        self.vl.remlink(&mut vb, b);
                        self.vb = vb;
                        self.eb_remlink(eed);
                        ok = true;
                    }
                    eed = nxt;
                }
            }
            if self.eb.first == NIL {
                return 0;
            }
        }
        // STEP 3: polyfill structs
        let mut pflist: Vec<PolyFill> = (0..poly)
            .map(|a| PolyFill { edges: 0, verts: 0, min_xy: [1.0e20; 2], max_xy: [-1.0e20; 2], nr: a, f: SF_POLY_NEW })
            .collect();
        let mut eed = self.eb.first;
        while eed != NIL {
            pflist[self.e[eed].poly_nr as usize].edges += 1;
            eed = self.el.n[eed];
        }
        let mut eve = self.vb.first;
        while eve != NIL {
            let p = &mut pflist[self.v[eve].poly_nr as usize];
            p.verts += 1;
            let xy = self.v[eve].xy;
            p.min_xy[0] = if p.min_xy[0] < xy[0] { p.min_xy[0] } else { xy[0] };
            p.min_xy[1] = if p.min_xy[1] < xy[1] { p.min_xy[1] } else { xy[1] };
            p.max_xy[0] = if p.max_xy[0] > xy[0] { p.max_xy[0] } else { xy[0] };
            p.max_xy[1] = if p.max_xy[1] > xy[1] { p.max_xy[1] } else { xy[1] };
            if self.v[eve].edge_count > 2 {
                p.f = SF_POLY_VALID;
            }
            eve = self.vl.n[eve];
        }
        // STEP 4: find holes or bounds, join them
        if poly > 1 {
            let pl = poly as usize;
            let mut target: Vec<usize> = (0..pl).collect();
            fn boundisect(pf2: &PolyFill, pf1: &PolyFill) -> bool {
                if pf1.edges == 0 || pf2.edges == 0 {
                    return false;
                }
                if pf2.max_xy[0] < pf1.min_xy[0] || pf2.max_xy[1] < pf1.min_xy[1] || pf2.min_xy[0] > pf1.max_xy[0] || pf2.min_xy[1] > pf1.max_xy[1] {
                    return false;
                }
                true
            }
            fn rec(pfl: &[PolyFill], tgt: usize, test: usize, map: &mut [usize]) {
                let pf_a = &pfl[test];
                for b in (tgt + 1)..pfl.len() {
                    if map[b] != b {
                        continue;
                    }
                    if boundisect(pf_a, &pfl[b]) {
                        map[b] = tgt;
                        rec(pfl, tgt, b, map);
                    }
                }
            }
            for a in 0..pl {
                if target[a] != a {
                    continue;
                }
                rec(&pflist, a, a, &mut target);
            }
            for a in 0..pl {
                if target[a] != a {
                    let (dst, src) = (target[a], a);
                    self.mergepolys(dst, src, &mut pflist);
                }
            }
        }
        // STEP 5: make triangles
        let mut tempve = self.vb;
        let mut temped = self.eb;
        self.vb = EMPTY;
        self.eb = EMPTY;
        let mut totfaces = 0;
        for a in 0..poly as usize {
            let pf = pflist[a];
            if pf.edges > 1 {
                self.splitlist(&mut tempve, &mut temped, pf.nr);
                totfaces += self.scanfill(&pf);
            }
        }
        let mut vb = self.vb;
        self.vl.move_to(&mut vb, &mut tempve);
        self.vb = vb;
        let mut eb = self.eb;
        self.el.move_to(&mut eb, &mut temped);
        self.eb = eb;
        totfaces
    }
}

/// `BLI_scanfill_calc_ex(CALC_HOLES | CALC_POLYS | CALC_LOOSE, normal)` over the edge list (vertices are added in order of
/// first use, as bmo_triangle_fill_exec does).  Returns triangles (v1, v2, v3) as indices into `co`, in `fillfacebase` order.
pub fn triangle_fill(co: &[V3], edges: &[[usize; 2]], normal: V3) -> Vec<[usize; 3]> {
    let mut ctx = Ctx { v: vec![], e: vec![], vl: Links::default(), el: Links::default(), vb: EMPTY, eb: EMPTY, faces: vec![], poly_nr: SF_POLY_UNSET };
    let mut map: Vec<usize> = vec![NIL; co.len()];
    for ed in edges {
        let mut sv = [0usize; 2];
        for i in 0..2 {
            if map[ed[i]] == NIL {
                let id = ctx.vert_add(co[ed[i]]);
                ctx.v[id].tmp = ed[i];
                map[ed[i]] = id;
            }
            sv[i] = map[ed[i]];
        }
        ctx.edge_add(sv[0], sv[1]);
    }
    ctx.calc(normal);
    ctx.faces.iter().map(|f| [ctx.v[f[0]].tmp, ctx.v[f[1]].tmp, ctx.v[f[2]].tmp]).collect()
}
