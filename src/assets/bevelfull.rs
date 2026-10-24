//! Port of Blender 4.2 `bmesh_bevel.cc` as used by the BEVEL modifier (edges, width offset, circular profile, loop slide,
//! clamp overlap, sharp miters). Geometry only (vertex / face order is not Blender's). Source: bsrc/bmesh_bevel.cc.
//! Supported vertex patterns: chains (2 beveled edges), terminal edges, 3-edge cube corners (tri corner / general ADJ patch),
//! pipes are rejected (returns Err).
use crate::mesh::Mesh;
use crate::dhash::HashMap;

type V = [f64; 3];

const EPS: f64 = 1e-6;
const EPS_BIG: f64 = 1e-4;
const EPS_ANG: f64 = 2.0 * std::f64::consts::PI / 180.0;
const SMALL_ANG: f64 = 10.0 * std::f64::consts::PI / 180.0;
const PRO_CIRCLE_R: f64 = 2.0;

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn madd(a: V, b: V, s: f64) -> V {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        a
    }
}
fn is_zero(a: V) -> bool {
    a[0] == 0.0 && a[1] == 0.0 && a[2] == 0.0
}
fn mid(a: V, b: V) -> V {
    mul(add(a, b), 0.5)
}
fn angle_n(a: V, b: V) -> f64 {
    // angle_normalized_v3v3: half-chord formula (robust for zero vectors)
    let saasin = |x: f64| x.clamp(-1.0, 1.0).asin();
    if dot(a, b) >= 0.0 {
        2.0 * saasin(len(sub(a, b)) / 2.0)
    } else {
        std::f64::consts::PI - 2.0 * saasin(len(add(a, b)) / 2.0)
    }
}
fn angle_v(a: V, b: V) -> f64 {
    angle_n(norm(a), norm(b))
}
fn nearly_parallel(d1: V, d2: V) -> bool {
    // inputs normalized
    let a = angle_n(d1, d2);
    a < EPS_ANG || (std::f64::consts::PI - a) < EPS_ANG
}

/// Blender isect_line_line_v3: 0 collinear/parallel, 1 intersect, 2 skew (closest points returned)
fn isect_line_line(v1: V, v2: V, v3: V, v4: V) -> (i32, V, V) {
    let c = sub(v3, v1);
    let a = sub(v2, v1);
    let b = sub(v4, v3);
    let dir1 = norm(a);
    let dir2 = norm(b);
    let d = dot(dir1, dir2);
    if d == 1.0 || d == -1.0 {
        return (0, v1, v1);
    }
    let ab = cross(a, b);
    let dd = dot(c, ab);
    let div = dot(ab, ab);
    if div == 0.0 {
        return (0, v1, v1);
    }
    if dd > -1e-5 && dd < 1e-5 {
        let cb = cross(c, b);
        let i1 = madd(v1, a, dot(cb, ab) / div);
        return (1, i1, i1);
    }
    // skew: closest points
    let cb = cross(c, b);
    let ca = cross(c, a);
    let s = dot(cb, ab) / div;
    let t = dot(ca, ab) / div;
    (2, madd(v1, a, s), madd(v3, b, t))
}

fn isect_line_plane(l1: V, l2: V, plane_co: V, plane_no: V) -> Option<V> {
    let u = sub(l2, l1);
    let h = sub(l1, plane_co);
    let dotv = dot(plane_no, u);
    if dotv.abs() > 1e-9 * len(u).max(1e-30) && dotv != 0.0 {
        let lambda = -dot(plane_no, h) / dotv;
        Some(madd(l1, u, lambda))
    } else {
        None
    }
}

#[derive(Clone, Debug)]
struct Edge {
    v: [u32; 2],
    faces: Vec<usize>,
}

#[derive(Clone, Default, Debug)]
struct Profile {
    super_r: f64,
    start: V,
    middle: V,
    end: V,
    plane_no: V,
    plane_co: V,
    proj_dir: V,
    prof_co: Vec<V>,
    prof_co_2: Vec<V>,
    special_params: bool,
}

#[derive(Clone, Debug)]
struct BoundVert {
    co: V,
    v: Option<u32>,
    efirst: Option<usize>,
    elast: Option<usize>,
    eon: Option<usize>,
    ebev: Option<usize>,
    index: usize,
    sinratio: f64,
    adjchain: Option<(usize, usize)>,
    profile: Profile,
    visited: bool,
}

#[derive(Clone, Debug)]
struct Eh {
    e: usize,
    other: u32,
    fprev: Option<usize>,
    fnext: Option<usize>,
    leftv: Option<usize>,
    rightv: Option<usize>,
    profile_index: i32,
    seg: usize,
    offset_l: f64,
    offset_r: f64,
    offset_l_spec: f64,
    offset_r_spec: f64,
    is_bev: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum MeshKind {
    None,
    Poly,
    Adj,
    TriFan,
}

#[derive(Clone, Debug)]
struct NewVert {
    co: V,
    v: Option<u32>,
}

#[derive(Clone, Debug)]
struct VMesh {
    n: usize,
    ns: usize,
    mesh: Vec<NewVert>,
    bounds: Vec<BoundVert>, // boundary list (index = position); next = (i+1)%n
    kind: MeshKind,
}

#[derive(Clone, Debug)]
struct BevVert {
    v: u32,
    selcount: usize,
    edges: Vec<Eh>,
    vm: VMesh,
}

struct Bevel<'a> {
    m: &'a Mesh,
    fno: Vec<V>,
    edges: Vec<Edge>,
    vedges: Vec<Vec<usize>>,
    vno: Vec<V>,
    etag: Vec<bool>,
    vtag: Vec<bool>,
    offset: f64,
    width_mode: bool,
    seg: usize,
    seg_2: usize,
    pro_xy: Vec<(f64, f64)>,
    pro_xy_2: Vec<(f64, f64)>,
    bvs: Vec<BevVert>,
    bv_of: HashMap<u32, usize>,
    out_pos: Vec<V>,
    out_faces: Vec<Vec<u32>>,
}

fn face_normal(m: &Mesh, f: &[u32]) -> V {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    norm(n)
}

fn even_xy(n: usize) -> Vec<(f64, f64)> {
    // find_even_superellipse_chords for r = 2 (circle): angle spacing
    let t = std::f64::consts::FRAC_PI_2 / n as f64;
    (0..=n).map(|i| ((i as f64 * t).sin(), (i as f64 * t).cos())).collect()
}

impl<'a> Bevel<'a> {
    fn new(m: &'a Mesh, offset: f64, seg: usize, width_mode: bool) -> Bevel<'a> {
        let fno: Vec<V> = m.faces.iter().map(|f| face_normal(m, f)).collect();
        let mut edges: Vec<Edge> = vec![];
        let mut emap: HashMap<(u32, u32), usize> = HashMap::default();
        let mut vedges: Vec<Vec<usize>> = vec![vec![]; m.pos.len()];
        for (fi, f) in m.faces.iter().enumerate() {
            for i in 0..f.len() {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                let key = (a.min(b), a.max(b));
                let ei = *emap.entry(key).or_insert_with(|| {
                    edges.push(Edge { v: [key.0, key.1], faces: vec![] });
                    edges.len() - 1
                });
                edges[ei].faces.push(fi);
                if !vedges[a as usize].contains(&ei) {
                    vedges[a as usize].push(ei);
                }
                if !vedges[b as usize].contains(&ei) {
                    vedges[b as usize].push(ei);
                }
            }
        }
        // angle weighted vertex normals
        let mut vno = vec![[0.0; 3]; m.pos.len()];
        for (fi, f) in m.faces.iter().enumerate() {
            let k = f.len();
            for i in 0..k {
                let v = f[i] as usize;
                let (p, n) = (m.pos[f[(i + k - 1) % k] as usize], m.pos[f[(i + 1) % k] as usize]);
                let a = angle_v(sub(p, m.pos[v]), sub(n, m.pos[v]));
                vno[v] = madd(vno[v], fno[fi], a);
            }
        }
        for n in vno.iter_mut() {
            *n = norm(*n);
        }
        let seg_2 = if seg <= 1 { 0 } else { seg.next_power_of_two().max(4) };
        let (pro_xy, pro_xy_2) = if seg > 1 { (even_xy(seg), even_xy(seg_2)) } else { (vec![], vec![]) };
        Bevel { m, width_mode, fno, etag: vec![false; edges.len()], edges, vedges, vno, vtag: vec![false; m.pos.len()], offset, seg, seg_2, pro_xy, pro_xy_2, bvs: vec![], bv_of: HashMap::default(), out_pos: vec![], out_faces: vec![] }
    }

    fn co(&self, v: u32) -> V {
        self.m.pos[v as usize]
    }

    /// face where edge (a->b) is traversed in winding order a then b
    fn face_dir(&self, e: usize, a: u32) -> Option<usize> {
        for &f in &self.edges[e].faces {
            let fv = &self.m.faces[f];
            let k = fv.len();
            for i in 0..k {
                if fv[i] == a && (fv[(i + 1) % k] == self.edges[e].v[0] || fv[(i + 1) % k] == self.edges[e].v[1]) && fv[(i + 1) % k] != a {
                    return Some(f);
                }
            }
        }
        None
    }

    fn other_end(&self, e: usize, v: u32) -> u32 {
        if self.edges[e].v[0] == v {
            self.edges[e].v[1]
        } else {
            self.edges[e].v[0]
        }
    }

    /// bevel_vert_construct
    fn construct(&mut self, v: u32) -> Result<(), String> {
        let inc: Vec<usize> = self.vedges[v as usize].clone();
        let nsel = inc.iter().filter(|&&e| self.etag[e]).count();
        if nsel == 0 {
            self.vtag[v as usize] = false;
            return Ok(());
        }
        for &e in &inc {
            if self.edges[e].faces.len() > 2 {
                return Err("non-manifold edge".into());
            }
            if self.etag[e] && self.edges[e].faces.len() != 2 {
                return Err("beveled edge without two faces".into());
            }
        }
        // out_face(e): face where e is traversed v -> other; in_face(e): other -> v
        let out_f = |s: &Self, e: usize| s.face_dir(e, v);
        let in_f = |s: &Self, e: usize| s.face_dir(e, s.other_end(e, v));
        // start: an edge with no incoming face (boundary), else the first selected edge
        let start = inc.iter().cloned().find(|&e| in_f(self, e).is_none()).unwrap_or_else(|| inc.iter().cloned().find(|&e| self.etag[e]).unwrap_or(inc[0]));
        let mut order = vec![start];
        loop {
            let cur = *order.last().unwrap();
            let of = match out_f(self, cur) {
                Some(f) => f,
                None => break,
            };
            // previous vertex of v in face of
            let fv = &self.m.faces[of];
            let k = fv.len();
            let i = fv.iter().position(|&x| x == v).unwrap();
            let u = fv[(i + k - 1) % k];
            let nxt = *inc.iter().find(|&&e| self.other_end(e, v) == u && e != cur).ok_or("edge order")?;
            if nxt == start {
                break;
            }
            if order.contains(&nxt) {
                return Err("edge order loop".into());
            }
            order.push(nxt);
        }
        if order.len() != inc.len() {
            return Err(format!("edge ordering failed ({} of {})", order.len(), inc.len()));
        }
        let mut ehs: Vec<Eh> = vec![];
        for &e in &order {
            let is_bev = self.etag[e];
            ehs.push(Eh { e, other: self.other_end(e, v), fprev: in_f(self, e), fnext: out_f(self, e), leftv: None, rightv: None, profile_index: 0, seg: if is_bev { self.seg } else { 0 }, offset_l: 0.0, offset_r: 0.0, offset_l_spec: 0.0, offset_r_spec: 0.0, is_bev });
        }
        for eh in ehs.iter_mut() {
            if eh.is_bev {
                let ang = match (eh.fprev, eh.fnext) {
                    (Some(a), Some(b)) => std::f64::consts::PI - angle_n(self.fno[a], self.fno[b]),
                    _ => 0.0,
                };
                let z = (2.0 * (ang / 2.0).sin()).abs();
                eh.offset_l_spec = if !self.width_mode { self.offset } else if z < EPS { 0.01 * self.offset } else { self.offset / z };
                eh.offset_r_spec = eh.offset_l_spec;
            }
            eh.offset_l = eh.offset_l_spec;
            eh.offset_r = eh.offset_r_spec;
        }
        let n = ehs.len();
        self.bv_of.insert(v, self.bvs.len());
        self.bvs.push(BevVert { v, selcount: nsel, edges: ehs, vm: VMesh { n: 0, ns: self.seg, mesh: vec![], bounds: vec![], kind: MeshKind::None } });
        let _ = n;
        Ok(())
    }

    fn find_other_end(&self, bvi: usize, ei: usize) -> Option<(usize, usize)> {
        let eh = &self.bvs[bvi].edges[ei];
        let bvo = *self.bv_of.get(&eh.other)?;
        let eo = self.bvs[bvo].edges.iter().position(|x| x.e == eh.e)?;
        Some((bvo, eo))
    }

    fn geometry_collide_offset(&self, bvi: usize, ebi: usize) -> f64 {
        let no_collide = self.offset + 1e6;
        let mut limit = no_collide;
        if self.offset == 0.0 {
            return no_collide;
        }
        let bv = &self.bvs[bvi];
        let n = bv.edges.len();
        let eb = &bv.edges[ebi];
        let kb = eb.offset_l_spec;
        let ea = &bv.edges[(ebi + 1) % n];
        let ka = ea.offset_r_spec;
        let vb = bv.v;
        let va = ea.other;
        let mut vc = eb.other;
        let mut kc = 0.0;
        let vd: u32;
        let mut ec_e: Option<usize> = None;
        if let Some((bvc, ebo)) = self.find_other_end(bvi, ebi) {
            let nc = self.bvs[bvc].edges.len();
            let ec = &self.bvs[bvc].edges[(ebo + nc - 1) % nc];
            vc = self.bvs[bvc].v;
            kc = ec.offset_l_spec;
            vd = ec.other;
            ec_e = Some(ec.e);
        } else {
            let f = match eb.fnext {
                Some(f) => f,
                None => return no_collide,
            };
            let fv = &self.m.faces[f];
            let k = fv.len();
            let ic = match fv.iter().position(|&x| x == vc) {
                Some(i) => i,
                None => return no_collide,
            };
            // face order: vb -> vc -> vd
            if fv[(ic + k - 1) % k] != vb {
                return no_collide;
            }
            vd = fv[(ic + 1) % k];
        }
        if ea.e == eb.e || ec_e == Some(eb.e) {
            return no_collide;
        }
        let (ka, kb, kc) = (ka / self.offset, kb / self.offset, kc / self.offset);
        let (pa, pb, pc, pd) = (self.co(va), self.co(vb), self.co(vc), self.co(vd));
        let th1 = angle_v(sub(pa, pb), sub(pc, pb));
        let th2 = angle_v(sub(pb, pc), sub(pd, pc));
        let sin1 = th1.sin();
        let sin2 = th2.sin();
        if (ka > 0.0) as i32 + (kb > 0.0) as i32 + (kc > 0.0) as i32 >= 2 {
            let tan1 = th1.tan();
            let tan2 = th2.tan();
            let g = tan1 * tan2;
            let h = sin1 * sin2;
            let den = g * (ka * sin2 + kc * sin1) + kb * h * (tan1 + tan2);
            if den != 0.0 {
                let mut t = len(sub(pb, pc));
                t *= g * h / den;
                if t >= 0.0 {
                    limit = t;
                }
            }
        }
        if kb > 0.0 && ka == 0.0 {
            let t = len(sub(pa, pb)) * sin1 / kb;
            if t >= 0.0 && t < limit {
                limit = t;
            }
        }
        if kb > 0.0 && kc == 0.0 {
            if let Some(_) = ec_e {
                let t = len(sub(pc, pd)) * sin2 / kb;
                if t >= 0.0 && t < limit {
                    limit = t;
                }
            } else {
                let t = len(sub(pc, pd)) * sin2 / kb;
                if t >= 0.0 && t < limit {
                    limit = t;
                }
            }
        }
        limit
    }

    fn limit_offset(&mut self) {
        let mut limited = self.offset;
        for bvi in 0..self.bvs.len() {
            for ei in 0..self.bvs[bvi].edges.len() {
                let c = self.geometry_collide_offset(bvi, ei);
                if c < limited {
                    limited = c;
                }
            }
        }
        if limited < self.offset {
            let f = limited / self.offset;
            for bv in self.bvs.iter_mut() {
                for eh in bv.edges.iter_mut() {
                    eh.offset_l_spec *= f;
                    eh.offset_r_spec *= f;
                    eh.offset_l *= f;
                    eh.offset_r *= f;
                }
            }
            self.offset = limited;
        }
    }
}

impl<'a> Bevel<'a> {
    fn slide_dist(&self, bvi: usize, ei: usize, d: f64) -> V {
        let bv = &self.bvs[bvi];
        let v = self.co(bv.v);
        let dir = norm(sub(v, self.co(bv.edges[ei].other)));
        let l = len(sub(v, self.co(bv.edges[ei].other)));
        let d = if d > l { l - 50.0 * 1e-6 } else { d };
        madd(v, dir, -d)
    }

    fn eh_on_plane(&self, e: &Eh) -> bool {
        if let (Some(a), Some(b)) = (e.fprev, e.fnext) {
            let d = dot(self.fno[a], self.fno[b]);
            return (d + 1.0).abs() <= EPS_BIG || (d - 1.0).abs() <= EPS_BIG;
        }
        false
    }

    /// 1 = ANGLE_LARGER (reflex), 0 straight, -1 smaller
    fn is_outside_edge(&self, bvi: usize, ei: usize, co: V) -> Option<u32> {
        let eh = &self.bvs[bvi].edges[ei];
        let (a, b) = (self.edges[eh.e].v[0], self.edges[eh.e].v[1]);
        let l1 = self.co(a);
        let u = sub(self.co(b), l1);
        let h = sub(co, l1);
        let lenu = len(u);
        let lambda = dot(norm(u), h);
        if lambda <= -EPS_BIG * lenu {
            return Some(a);
        }
        if lambda >= (1.0 + EPS_BIG) * lenu {
            return Some(b);
        }
        None
    }

    fn point_between_edges(&self, bvi: usize, co: V, f: usize, e1: usize, e2: usize) -> bool {
        let bv = &self.bvs[bvi];
        let v = self.co(bv.v);
        let dir1 = norm(sub(v, self.co(bv.edges[e1].other)));
        let dir2 = norm(sub(v, self.co(bv.edges[e2].other)));
        let dirco = norm(sub(v, co));
        let mut ang11 = angle_n(dir1, dir2);
        let mut ang1co = angle_n(dir1, dirco);
        let fno = self.fno[f];
        if dot(cross(dir1, dir2), fno) < 0.0 {
            ang11 = 2.0 * std::f64::consts::PI - ang11;
        }
        if dot(cross(dir1, dirco), fno) < 0.0 {
            ang1co = 2.0 * std::f64::consts::PI - ang1co;
        }
        ang11 - ang1co > -EPS_ANG
    }

    #[allow(clippy::too_many_arguments)]
    fn offset_meet(&self, bvi: usize, e1: usize, e2: usize, f: Option<usize>, edges_between: bool, e_in_plane: Option<usize>) -> V {
        let bv = &self.bvs[bvi];
        let n = bv.edges.len();
        let v = self.co(bv.v);
        let (eh1, eh2) = (&bv.edges[e1], &bv.edges[e2]);
        let mut dir1 = sub(v, self.co(eh1.other));
        let mut dir2 = sub(self.co(eh2.other), v);
        let (mut dir1n, mut dir2p) = ([0.0; 3], [0.0; 3]);
        if edges_between {
            let e1n = &bv.edges[(e1 + 1) % n];
            let e2p = &bv.edges[(e2 + n - 1) % n];
            dir1n = sub(self.co(e1n.other), v);
            dir2p = sub(v, self.co(e2p.other));
        }
        let mut ang = angle_v(dir1, dir2);
        let mut meetco;
        if ang < EPS_ANG {
            let norm_v = if let Some(f) = f {
                self.fno[f]
            } else {
                let mut nv = [0.0; 3];
                let mut cnt = 0;
                let mut i = e1;
                while i != e2 {
                    if let Some(ff) = bv.edges[i].fnext {
                        nv = add(nv, self.fno[ff]);
                        cnt += 1;
                    }
                    i = (i + 1) % n;
                }
                if cnt == 0 {
                    self.vno[bv.v as usize]
                } else {
                    mul(nv, 1.0 / cnt as f64)
                }
            };
            dir1 = add(dir1, dir2);
            let norm_perp1 = norm(cross(dir1, norm_v));
            let d = eh1.offset_r.max(eh2.offset_l) / (ang / 2.0).cos();
            meetco = madd(v, norm_perp1, d);
        } else if (ang - std::f64::consts::PI).abs() < EPS_ANG {
            let d = eh1.offset_r.max(eh2.offset_l);
            meetco = self.slide_dist(bvi, e2, d);
        } else {
            let (norm_v1, norm_v2);
            if f.is_some() && ang < SMALL_ANG {
                norm_v1 = self.fno[f.unwrap()];
                norm_v2 = norm_v1;
            } else if !edges_between {
                let mut nv = norm(cross(dir2, dir1));
                let ref_no = f.map(|f| self.fno[f]).unwrap_or(self.vno[bv.v as usize]);
                if dot(nv, ref_no) < 0.0 {
                    nv = mul(nv, -1.0);
                }
                norm_v1 = nv;
                norm_v2 = nv;
            } else {
                let mut n1 = norm(cross(dir1n, dir1));
                let r1 = eh1.fnext.map(|f| self.fno[f]).unwrap_or(self.vno[bv.v as usize]);
                if dot(n1, r1) < 0.0 {
                    n1 = mul(n1, -1.0);
                }
                let mut n2 = norm(cross(dir2, dir2p));
                let r2 = eh2.fprev.map(|f| self.fno[f]).unwrap_or(self.vno[bv.v as usize]);
                if dot(n2, r2) < 0.0 {
                    n2 = mul(n2, -1.0);
                }
                norm_v1 = n1;
                norm_v2 = n2;
            }
            let norm_perp1 = norm(cross(dir1, norm_v1));
            let norm_perp2 = norm(cross(dir2, norm_v2));
            let off1a = madd(v, norm_perp1, eh1.offset_r);
            let off1b = add(off1a, dir1);
            let off2a = madd(v, norm_perp2, eh2.offset_l);
            let off2b = add(off2a, dir2);
            let (kind, mc, isect2) = isect_line_line(off1a, off1b, off2a, off2b);
            meetco = mc;
            if kind == 0 {
                meetco = off1a;
            } else {
                if eh1.offset_r == 0.0 {
                    if let Some(cv) = self.is_outside_edge(bvi, e1, meetco) {
                        meetco = self.co(cv);
                    }
                }
                if eh2.offset_l == 0.0 {
                    if let Some(cv) = self.is_outside_edge(bvi, e2, meetco) {
                        meetco = self.co(cv);
                    }
                }
                if edges_between && eh1.offset_r > 0.0 && eh2.offset_l > 0.0 {
                    if kind == 2 {
                        meetco = mid(meetco, isect2);
                    }
                    let mut e = e1;
                    while e != e2 {
                        let fnext = bv.edges[e].fnext;
                        if let Some(fnx) = fnext {
                            let fnn = self.fno[fnx];
                            let dropco = {
                                let d = dot(fnn, sub(meetco, v));
                                madd(meetco, fnn, -d)
                            };
                            let mut skip = false;
                            if let Some(ip) = e_in_plane {
                                if let Some(ipf) = bv.edges[ip].fnext {
                                    ang = angle_v(fnn, self.fno[ipf]);
                                    if ang.abs() < SMALL_ANG || (ang - std::f64::consts::PI).abs() < SMALL_ANG {
                                        skip = true;
                                    }
                                }
                            }
                            if !skip && self.point_between_edges(bvi, dropco, fnx, e, (e + 1) % n) {
                                meetco = dropco;
                                break;
                            }
                        }
                        e = (e + 1) % n;
                    }
                }
            }
        }
        meetco
    }

    /// returns (meetco, angle, ok)
    fn offset_meet_edge(&self, bvi: usize, e1: usize, e2: usize) -> (V, f64, bool) {
        let bv = &self.bvs[bvi];
        let v = self.co(bv.v);
        let dir1 = norm(sub(self.co(bv.edges[e1].other), v));
        let dir2 = norm(sub(self.co(bv.edges[e2].other), v));
        let mut ang = angle_n(dir1, dir2);
        if ang.abs() < 0.1 {
            return (v, 0.0, false);
        }
        let fno = cross(dir1, dir2);
        if dot(fno, self.vno[bv.v as usize]) < 0.0 {
            ang = 2.0 * std::f64::consts::PI - ang;
            return (v, ang, false);
        }
        if (ang - std::f64::consts::PI).abs() < 0.1 {
            return (v, ang, false);
        }
        let sinang = ang.sin();
        let meet = if bv.edges[e1].offset_r == 0.0 { madd(v, dir1, bv.edges[e2].offset_l / sinang) } else { madd(v, dir2, bv.edges[e1].offset_r / sinang) };
        (meet, ang, true)
    }

    fn good_offset_on_edge_between(&self, bvi: usize, e1: usize, e2: usize, emid: usize) -> bool {
        self.offset_meet_edge(bvi, e1, emid).2 && self.offset_meet_edge(bvi, emid, e2).2
    }

    fn offset_on_edge_between(&self, bvi: usize, e1: usize, e2: usize, emid: usize) -> (V, f64, bool) {
        let (meet1, ang1, ok1) = self.offset_meet_edge(bvi, e1, emid);
        let (meet2, ang2, ok2) = self.offset_meet_edge(bvi, emid, e2);
        if ok1 && ok2 {
            let sr = if ang1 == 0.0 { 1.0 } else { ang2.sin() / ang1.sin() };
            return (mid(meet1, meet2), sr, true);
        } else if ok1 {
            return (meet1, 1.0, false);
        } else if ok2 {
            return (meet2, 1.0, false);
        }
        (self.slide_dist(bvi, emid, self.bvs[bvi].edges[e1].offset_r), 1.0, false)
    }

    fn offset_in_plane(&self, bvi: usize, ei: usize, plane_no: Option<V>, left: bool) -> V {
        let bv = &self.bvs[bvi];
        let eh = &bv.edges[ei];
        let v = self.co(bv.v);
        let dir = norm(sub(self.co(eh.other), v));
        let no = match plane_no {
            Some(n) => n,
            None => {
                if dir[0].abs() < dir[1].abs() {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                }
            }
        };
        let fdir = if left { norm(cross(dir, no)) } else { norm(cross(no, dir)) };
        madd(v, fdir, if left { eh.offset_l } else { eh.offset_r })
    }

    fn add_bound(&mut self, bvi: usize, co: V) -> usize {
        let n = self.bvs[bvi].vm.bounds.len();
        self.bvs[bvi].vm.bounds.push(BoundVert { co, v: None, efirst: None, elast: None, eon: None, ebev: None, index: n, sinratio: 1.0, adjchain: None, profile: Profile { super_r: 1.0, ..Default::default() }, visited: false });
        self.bvs[bvi].vm.n = n + 1;
        n
    }

    fn next_bev(&self, bvi: usize, from: usize) -> Option<usize> {
        let n = self.bvs[bvi].edges.len();
        let mut e = from;
        loop {
            if self.bvs[bvi].edges[e].is_bev {
                return Some(e);
            }
            e = (e + 1) % n;
            if e == from {
                return None;
            }
        }
    }

    fn build_boundary(&mut self, bvi: usize, construct: bool) -> Result<(), String> {
        let n = self.bvs[bvi].edges.len();
        if n <= 1 {
            return Ok(());
        }
        let efirst = self.next_bev(bvi, n - 1).ok_or("no bevel edge")?;
        if self.bvs[bvi].selcount == 1 {
            return self.build_boundary_terminal(bvi, efirst, construct);
        }
        let mut e = efirst;
        loop {
            // analyse edges until next beveled edge
            let mut in_plane = 0;
            let mut not_in_plane = 0;
            let mut enip = None;
            let mut eip = None;
            let mut e2 = (e + 1) % n;
            while !self.bvs[bvi].edges[e2].is_bev {
                if self.eh_on_plane(&self.bvs[bvi].edges[e2]) {
                    in_plane += 1;
                    eip = Some(e2);
                } else {
                    not_in_plane += 1;
                    enip = Some(e2);
                }
                e2 = (e2 + 1) % n;
            }
            let mut eon = None;
            let mut sinratio = 1.0;
            let co;
            let fnext = self.bvs[bvi].edges[e].fnext;
            if in_plane == 0 && not_in_plane == 0 {
                co = self.offset_meet(bvi, e, e2, fnext, false, None);
            } else if not_in_plane > 0 {
                if not_in_plane == 1 && self.good_offset_on_edge_between(bvi, e, e2, enip.unwrap()) {
                    let (c, r, ok) = self.offset_on_edge_between(bvi, e, e2, enip.unwrap());
                    co = c;
                    if ok {
                        eon = enip;
                        sinratio = r;
                    }
                } else {
                    co = self.offset_meet(bvi, e, e2, None, true, eip);
                }
            } else if in_plane == 1 && self.good_offset_on_edge_between(bvi, e, e2, eip.unwrap()) {
                let (c, r, ok) = self.offset_on_edge_between(bvi, e, e2, eip.unwrap());
                co = c;
                if ok {
                    eon = eip;
                    sinratio = r;
                }
            } else {
                co = self.offset_meet(bvi, e, e2, fnext, false, None);
            }
            if construct {
                let bi = self.add_bound(bvi, co);
                {
                    let b = &mut self.bvs[bvi].vm.bounds[bi];
                    b.efirst = Some(e);
                    b.elast = Some(e2);
                    b.ebev = Some(e2);
                    b.eon = eon;
                    if eon.is_some() {
                        b.sinratio = sinratio;
                    }
                }
                self.bvs[bvi].edges[e].rightv = Some(bi);
                self.bvs[bvi].edges[e2].leftv = Some(bi);
                let mut e3 = (e + 1) % n;
                while e3 != e2 {
                    self.bvs[bvi].edges[e3].leftv = Some(bi);
                    self.bvs[bvi].edges[e3].rightv = Some(bi);
                    e3 = (e3 + 1) % n;
                }
            } else {
                let bi = self.bvs[bvi].edges[e].rightv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
            e = e2;
            if e == efirst {
                break;
            }
        }
        if construct {
            let cnt = self.bvs[bvi].vm.bounds.len();
            self.bvs[bvi].vm.kind = if cnt == 2 {
                MeshKind::None
            } else if self.bvs[bvi].edges[efirst].seg == 1 {
                MeshKind::Poly
            } else {
                MeshKind::Adj
            };
        }
        Ok(())
    }

    fn build_boundary_terminal(&mut self, bvi: usize, efirst: usize, construct: bool) -> Result<(), String> {
        let n = self.bvs[bvi].edges.len();
        let e = efirst;
        if n == 2 {
            let (fp, fnx) = (self.bvs[bvi].edges[e].fprev, self.bvs[bvi].edges[e].fnext);
            let no = fp.or(fnx).map(|f| self.fno[f]);
            let co = self.offset_in_plane(bvi, e, no, true);
            if construct {
                let bi = self.add_bound(bvi, co);
                let b = &mut self.bvs[bvi].vm.bounds[bi];
                b.efirst = Some(e);
                b.elast = Some(e);
                b.ebev = Some(e);
                self.bvs[bvi].edges[e].leftv = Some(bi);
            } else {
                let bi = self.bvs[bvi].edges[e].leftv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
            let no = fnx.or(fp).map(|f| self.fno[f]);
            let co = self.offset_in_plane(bvi, e, no, false);
            if construct {
                let bi = self.add_bound(bvi, co);
                let b = &mut self.bvs[bvi].vm.bounds[bi];
                b.efirst = Some(e);
                b.elast = Some(e);
                self.bvs[bvi].edges[e].rightv = Some(bi);
            } else {
                let bi = self.bvs[bvi].edges[e].rightv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
            let en = (e + 1) % n;
            let co = self.slide_dist(bvi, en, self.bvs[bvi].edges[e].offset_l);
            if construct {
                let bi = self.add_bound(bvi, co);
                let b = &mut self.bvs[bvi].vm.bounds[bi];
                b.efirst = Some(en);
                b.elast = Some(en);
                self.bvs[bvi].edges[en].leftv = Some(bi);
                self.bvs[bvi].edges[en].rightv = Some(bi);
            } else {
                let bi = self.bvs[bvi].edges[en].leftv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
        } else {
            let ep = (e + n - 1) % n;
            let fprev = self.bvs[bvi].edges[e].fprev;
            let co = self.offset_meet(bvi, ep, e, fprev, false, None);
            if construct {
                let bi = self.add_bound(bvi, co);
                let b = &mut self.bvs[bvi].vm.bounds[bi];
                b.efirst = Some(ep);
                b.elast = Some(e);
                b.ebev = Some(e);
                self.bvs[bvi].edges[e].leftv = Some(bi);
                self.bvs[bvi].edges[ep].leftv = Some(bi);
                self.bvs[bvi].edges[ep].rightv = Some(bi);
            } else {
                let bi = self.bvs[bvi].edges[e].leftv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
            let e1 = (e + 1) % n;
            let fprev1 = self.bvs[bvi].edges[e1].fprev;
            let co = self.offset_meet(bvi, (e1 + n - 1) % n, e1, fprev1, false, None);
            if construct {
                let bi = self.add_bound(bvi, co);
                let b = &mut self.bvs[bvi].vm.bounds[bi];
                b.efirst = Some((e1 + n - 1) % n);
                b.elast = Some(e1);
                self.bvs[bvi].edges[e1].leftv = Some(bi);
                self.bvs[bvi].edges[e1].rightv = Some(bi);
                self.bvs[bvi].edges[(e1 + n - 1) % n].rightv = Some(bi);
            } else {
                let bi = self.bvs[bvi].edges[e1].leftv.unwrap();
                self.bvs[bvi].vm.bounds[bi].co = co;
            }
            let d = self.bvs[bvi].edges[efirst].offset_l_spec;
            let mut ee = (e1 + 1) % n;
            while (ee + 1) % n != efirst {
                let co = self.slide_dist(bvi, ee, d);
                if construct {
                    let bi = self.add_bound(bvi, co);
                    let b = &mut self.bvs[bvi].vm.bounds[bi];
                    b.efirst = Some(ee);
                    b.elast = Some(ee);
                    self.bvs[bvi].edges[ee].leftv = Some(bi);
                    self.bvs[bvi].edges[ee].rightv = Some(bi);
                } else {
                    let bi = self.bvs[bvi].edges[ee].leftv.unwrap();
                    self.bvs[bvi].vm.bounds[bi].co = co;
                }
                ee = (ee + 1) % n;
            }
        }
        if n >= 3 {
            // snap profile to the plane of the adjacent two edges
            let b0 = 0;
            self.set_profile_params(bvi, b0);
            self.move_profile_plane(bvi, b0);
        }
        if construct {
            let cnt = self.bvs[bvi].vm.bounds.len();
            self.bvs[bvi].vm.kind = if cnt == 2 && n == 3 {
                MeshKind::None
            } else if cnt == 3 {
                MeshKind::TriFan
            } else {
                MeshKind::Poly
            };
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------ profiles
fn make_unit_square_map(va: V, vmid: V, vb: V) -> Option<[V; 4]> {
    let vb_vmid = sub(vmid, vb);
    let va_vmid = sub(vmid, va);
    if is_zero(va_vmid) || is_zero(vb_vmid) {
        return None;
    }
    if (angle_v(va_vmid, vb_vmid) - std::f64::consts::PI).abs() <= EPS_ANG {
        return None;
    }
    let vo = sub(va, vb_vmid);
    let vddir = norm(cross(vb_vmid, va_vmid));
    let vd = add(vo, vddir);
    let c0 = sub(vmid, va);
    let c1 = sub(vmid, vb);
    let c2 = sub(sub(add(vmid, vd), va), vb);
    let c3 = sub(add(va, vb), vmid);
    Some([c0, c1, c2, c3])
}

fn map_apply(m: &[V; 4], p: V) -> V {
    [m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0], m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1], m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2]]
}

fn map_apply_(m: &[V; 4], p: V) -> V {
    map_apply(m, p)
}

fn make_unit_cube_map(va: V, vb: V, vc: V, vd: V) -> [V; 4] {
    let c0 = mul(add(sub(sub(va, vb), vc), vd), 0.5);
    let c1 = mul(add(sub(sub(vb, va), vc), vd), 0.5);
    let c2 = mul(add(sub(sub(vc, va), vb), vd), 0.5);
    let c3 = mul(sub(add(add(va, vb), vc), vd), 0.5);
    [c0, c1, c2, c3]
}

impl<'a> Bevel<'a> {
    fn calc_profile(&self, pro: &mut Profile) {
        if self.seg == 1 {
            return;
        }
        let use_map_opt = if pro.super_r == 1.0 { None } else { make_unit_square_map(pro.start, pro.middle, pro.end) };
        let mut gen = |ns: usize, xy: &Vec<(f64, f64)>| -> Vec<V> {
            let mut out = vec![];
            for k in 0..=ns {
                let co = if k == 0 {
                    pro.start
                } else if k == ns {
                    pro.end
                } else if let Some(m) = &use_map_opt {
                    map_apply(m, [xy[k].0, xy[k].1, 0.0])
                } else {
                    let t = k as f64 / ns as f64;
                    add(mul(pro.start, 1.0 - t), mul(pro.end, t))
                };
                let c = if !is_zero(pro.proj_dir) {
                    isect_line_plane(co, add(co, pro.proj_dir), pro.plane_co, pro.plane_no).unwrap_or(co)
                } else {
                    co
                };
                out.push(c);
            }
            out
        };
        pro.prof_co = gen(self.seg, &self.pro_xy);
        pro.prof_co_2 = if self.seg_2 != self.seg { gen(self.seg_2, &self.pro_xy_2) } else { pro.prof_co.clone() };
    }

    fn get_profile_point(&self, pro: &Profile, i: usize, nseg: usize) -> V {
        if self.seg == 1 {
            return if i == 0 { pro.start } else { pro.end };
        }
        if nseg == self.seg {
            pro.prof_co[i]
        } else {
            let sub = self.seg_2 / nseg;
            pro.prof_co_2[i * sub]
        }
    }

    fn project_to_edge(&self, e: usize, a: V, b: V) -> V {
        let (v1, v2) = (self.co(self.edges[e].v[0]), self.co(self.edges[e].v[1]));
        let (k, p, _) = isect_line_line(v1, v2, a, b);
        if k == 0 {
            v1
        } else {
            p
        }
    }

    fn set_profile_params(&mut self, bvi: usize, bi: usize) {
        let bv = &self.bvs[bvi];
        let nb = bv.vm.bounds.len();
        let start = bv.vm.bounds[bi].co;
        let end = bv.vm.bounds[(bi + 1) % nb].co;
        let mut pro = bv.vm.bounds[bi].profile.clone();
        let mut do_linear = true;
        let v = self.co(bv.v);
        if let Some(ei) = bv.vm.bounds[bi].ebev {
            do_linear = false;
            let eh = &bv.edges[ei];
            pro.super_r = PRO_CIRCLE_R;
            pro.proj_dir = norm(sub(v, self.co(eh.other)));
            pro.middle = self.project_to_edge(eh.e, start, end);
            pro.start = start;
            pro.end = end;
            let mut d1 = norm(sub(pro.middle, start));
            let mut d2 = norm(sub(pro.middle, end));
            pro.plane_no = norm(cross(d1, d2));
            if nearly_parallel(d1, d2) {
                pro.middle = v;
                let ne = bv.edges.len();
                let (ep, en) = (&bv.edges[(ei + ne - 1) % ne], &bv.edges[(ei + 1) % ne]);
                if ep.is_bev && en.is_bev && bv.selcount >= 3 {
                    let d3 = norm(sub(self.co(ep.other), v));
                    let d4 = norm(sub(self.co(en.other), v));
                    // directions of the offset lines (edge directions)
                    let d3 = mul(d3, -1.0);
                    let d4 = mul(d4, -1.0);
                    let _ = (d3, d4);
                    let d3 = norm(sub(self.co(self.edges[ep.e].v[0]), self.co(self.edges[ep.e].v[1])));
                    let d4 = norm(sub(self.co(self.edges[en.e].v[0]), self.co(self.edges[en.e].v[1])));
                    if nearly_parallel(d3, d4) {
                        pro.middle = mid(start, end);
                        do_linear = true;
                    } else {
                        let co3 = add(start, d3);
                        let co4 = add(end, d4);
                        let (k, meet, _) = isect_line_line(start, co3, end, co4);
                        if k != 0 {
                            pro.middle = meet;
                        } else {
                            pro.middle = mid(start, end);
                            do_linear = true;
                        }
                    }
                }
                pro.end = end;
                d1 = norm(sub(pro.middle, start));
                d2 = norm(sub(pro.middle, end));
                pro.plane_no = norm(cross(d1, d2));
                if nearly_parallel(d1, d2) {
                    do_linear = true;
                } else {
                    pro.plane_co = v;
                    pro.proj_dir = pro.plane_no;
                }
            }
            pro.plane_co = start;
        }
        if do_linear {
            pro.super_r = 1.0;
            pro.start = start;
            pro.end = end;
            pro.middle = mid(start, end);
            pro.plane_co = [0.0; 3];
            pro.plane_no = [0.0; 3];
            pro.proj_dir = [0.0; 3];
        }
        self.bvs[bvi].vm.bounds[bi].profile = pro;
    }

    fn move_profile_plane(&mut self, bvi: usize, bi: usize) {
        let v = self.co(self.bvs[bvi].v);
        let pro = &mut self.bvs[bvi].vm.bounds[bi].profile;
        if is_zero(pro.proj_dir) {
            return;
        }
        let d1 = norm(sub(v, pro.start));
        let d2 = norm(sub(v, pro.end));
        let no = cross(d1, d2);
        let no2 = cross(d1, pro.proj_dir);
        let no3 = cross(d2, pro.proj_dir);
        let (l1, l2, l3) = (len(no), len(no2), len(no3));
        if l1 > EPS_BIG && l2 > EPS_BIG && l3 > EPS_BIG {
            let (no, no2, no3) = (norm(no), norm(no2), norm(no3));
            let dot2 = dot(no, no2);
            let dot3 = dot(no, no3);
            if dot2.abs() < 1.0 - EPS_BIG && dot3.abs() < 1.0 - EPS_BIG {
                pro.plane_no = no;
            }
        }
        pro.special_params = true;
    }

    fn move_weld_profile_planes(&mut self, bvi: usize, b1: usize, b2: usize) {
        let v = self.co(self.bvs[bvi].v);
        let (p1, p2) = (self.bvs[bvi].vm.bounds[b1].clone(), self.bvs[bvi].vm.bounds[b2].clone());
        if is_zero(p1.profile.proj_dir) || is_zero(p2.profile.proj_dir) {
            return;
        }
        let d1 = sub(v, p1.co);
        let d2 = sub(v, p2.co);
        let no = cross(d1, d2);
        let l1 = len(no);
        let no = norm(no);
        let no2 = cross(d1, p1.profile.proj_dir);
        let l2 = len(no2);
        let no2 = norm(no2);
        let no3 = cross(d2, p2.profile.proj_dir);
        let l3 = len(no3);
        let no3 = norm(no3);
        if l1 > EPS && (l2 > EPS || l3 > EPS) {
            let dot1 = dot(no, no2).abs();
            let dot2 = dot(no, no3).abs();
            if (dot1 - 1.0).abs() > EPS {
                self.bvs[bvi].vm.bounds[b1].profile.plane_no = no;
            }
            if (dot2 - 1.0).abs() > EPS {
                self.bvs[bvi].vm.bounds[b2].profile.plane_no = no;
            }
        }
        self.bvs[bvi].vm.bounds[b1].profile.special_params = true;
        self.bvs[bvi].vm.bounds[b2].profile.special_params = true;
    }
}

// ------------------------------------------------------------------------------------------ vertex mesh helpers
fn vm_idx(ns: usize, i: usize, j: usize, k: usize) -> usize {
    let nj = ns / 2 + 1;
    let nk = ns + 1;
    i * nk * nj + j * nk + k
}

impl VMesh {
    fn new_adj(n: usize, ns: usize, bounds: Vec<BoundVert>) -> VMesh {
        VMesh { n, ns, mesh: vec![NewVert { co: [0.0; 3], v: None }; n * (ns / 2 + 1) * (ns + 1)], bounds, kind: MeshKind::Adj }
    }
    fn mv(&self, i: usize, j: usize, k: usize) -> usize {
        vm_idx(self.ns, i, j, k)
    }
    fn co(&self, i: usize, j: usize, k: usize) -> V {
        self.mesh[self.mv(i, j, k)].co
    }
    fn set_co(&mut self, i: usize, j: usize, k: usize, c: V) {
        let ix = self.mv(i, j, k);
        self.mesh[ix].co = c;
    }
    fn canon_idx(&self, i: usize, j: usize, k: usize) -> usize {
        let n = self.n;
        let ns = self.ns;
        let ns2 = ns / 2;
        let odd = ns % 2;
        if odd == 0 && j == ns2 && k == ns2 {
            return self.mv(0, j, k);
        }
        if j + 1 <= ns2 + odd && k <= ns2 {
            return self.mv(i, j, k);
        }
        if k <= ns2 {
            return self.mv((i + n - 1) % n, k, ns - j);
        }
        self.mv((i + 1) % n, ns - k, j)
    }
    fn canon_co(&self, i: usize, j: usize, k: usize) -> V {
        self.mesh[self.canon_idx(i, j, k)].co
    }
    fn is_canon(&self, i: usize, j: usize, k: usize) -> bool {
        let ns2 = self.ns / 2;
        if self.ns % 2 == 1 {
            return j <= ns2 && k <= ns2;
        }
        (j < ns2 && k <= ns2) || (j == ns2 && k == ns2 && i == 0)
    }
    fn copy_equiv(&mut self) {
        let ns2 = self.ns / 2;
        for i in 0..self.n {
            for j in 0..=ns2 {
                for k in 0..=self.ns {
                    if self.is_canon(i, j, k) {
                        continue;
                    }
                    let c = self.canon_idx(i, j, k);
                    let (co, v) = (self.mesh[c].co, self.mesh[c].v);
                    let ix = self.mv(i, j, k);
                    self.mesh[ix].co = co;
                    self.mesh[ix].v = v;
                }
            }
        }
    }
    fn center(&self) -> V {
        let ns2 = self.ns / 2;
        if self.ns % 2 == 1 {
            let mut c = [0.0; 3];
            for i in 0..self.n {
                c = add(c, self.co(i, ns2, ns2));
            }
            mul(c, 1.0 / self.n as f64)
        } else {
            self.co(0, ns2, ns2)
        }
    }
}

fn avg4(a: V, b: V, c: V, d: V) -> V {
    mul(add(add(a, b), add(c, d)), 0.25)
}

fn sabin_gamma(n: usize) -> f64 {
    if n < 3 {
        return 0.0;
    }
    match n {
        3 => 0.065247584,
        4 => 0.25,
        5 => 0.401983447,
        6 => 0.523423277,
        _ => {
            let k = (std::f64::consts::PI / n as f64).cos();
            let k2 = k * k;
            let k4 = k2 * k2;
            let k6 = k4 * k2;
            let y = (3f64.sqrt() * (64.0 * k6 - 144.0 * k4 + 135.0 * k2 - 27.0).sqrt() + 9.0 * k).powf(1.0 / 3.0);
            let x = 0.480749856769136 * y - (0.231120424783545 * (12.0 * k2 - 9.0)) / y;
            (k * x + 2.0 * k2 - 1.0) / (x * x * (k * x + 1.0))
        }
    }
}

fn fill_vmesh_fracs(vm: &VMesh, i: usize) -> Vec<f64> {
    let ns = vm.ns;
    let mut frac = vec![0.0; ns + 1];
    let mut total = 0.0;
    for k in 0..ns {
        total += len(sub(vm.co(i, 0, k), vm.co(i, 0, k + 1)));
        frac[k + 1] = total;
    }
    if total > 0.0 {
        for k in 1..=ns {
            frac[k] /= total;
        }
    } else {
        frac[ns] = 1.0;
    }
    frac
}

fn interp_range(frac: &[f64], n: usize, f: f64) -> (usize, f64) {
    for i in 0..n {
        if f <= frac[i + 1] {
            let rest = f - frac[i];
            let mut r = if rest == 0.0 { 0.0 } else { rest / (frac[i + 1] - frac[i]) };
            let mut ii = i;
            if i == n - 1 && r == 1.0 {
                ii = n;
                r = 0.0;
            }
            return (ii, r);
        }
    }
    (n, 0.0)
}

impl<'a> Bevel<'a> {
    fn fill_profile_fracs(&self, b: &BoundVert, ns: usize) -> Vec<f64> {
        let mut frac = vec![0.0; ns + 1];
        let mut total = 0.0;
        let mut co = b.co;
        for k in 0..ns {
            let next = self.get_profile_point(&b.profile, k + 1, ns);
            total += len(sub(co, next));
            frac[k + 1] = total;
            co = next;
        }
        if total > 0.0 {
            for k in 1..=ns {
                frac[k] /= total;
            }
        } else {
            frac[ns] = 1.0;
        }
        frac
    }

    fn interp_vmesh(&self, vm_in: &VMesh, nseg: usize) -> VMesh {
        let n = vm_in.n;
        let ns_in = vm_in.ns;
        let nseg2 = nseg / 2;
        let odd = nseg % 2;
        let mut out = VMesh::new_adj(n, nseg, vm_in.bounds.clone());
        let mut prev_frac = fill_vmesh_fracs(vm_in, n - 1);
        let mut prev_new = self.fill_profile_fracs(&vm_in.bounds[n - 1], nseg);
        for i in 0..n {
            let frac = fill_vmesh_fracs(vm_in, i);
            let new_frac = self.fill_profile_fracs(&vm_in.bounds[i], nseg);
            for j in 0..=(nseg2 + odd - 1) {
                for k in 0..=nseg2 {
                    let fraction = new_frac[k];
                    let (k_in, restk) = interp_range(&frac, ns_in, fraction);
                    let fraction = prev_new[nseg - j];
                    let (k_in_prev, restkprev) = interp_range(&prev_frac, ns_in, fraction);
                    let mut j_in = ns_in - k_in_prev;
                    let mut restj = -restkprev;
                    if restj > -EPS {
                        restj = 0.0;
                    } else {
                        j_in -= 1;
                        restj += 1.0;
                    }
                    let co;
                    if restj < EPS && restk < EPS {
                        co = vm_in.canon_co(i, j_in, k_in);
                    } else {
                        let j0inc = if restj < EPS || j_in == ns_in { 0 } else { 1 };
                        let k0inc = if restk < EPS || k_in == ns_in { 0 } else { 1 };
                        let q0 = vm_in.canon_co(i, j_in, k_in);
                        let q1 = vm_in.canon_co(i, j_in, k_in + k0inc);
                        let q2 = vm_in.canon_co(i, j_in + j0inc, k_in + k0inc);
                        let q3 = vm_in.canon_co(i, j_in + j0inc, k_in);
                        // interp_bilinear_quad_v3(quad, u=restk, v=restj)
                        let (u, v) = (restk, restj);
                        co = add(add(mul(q0, (1.0 - u) * (1.0 - v)), mul(q1, u * (1.0 - v))), add(mul(q2, u * v), mul(q3, (1.0 - u) * v)));
                    }
                    out.set_co(i, j, k, co);
                }
            }
            prev_frac = frac;
            prev_new = new_frac;
        }
        if odd == 0 {
            let c = vm_in.center();
            out.set_co(0, nseg2, nseg2, c);
        }
        out.copy_equiv();
        out
    }

    fn cubic_subdiv(&self, vm_in: &mut VMesh) -> VMesh {
        let n = vm_in.n;
        let ns_in = vm_in.ns;
        let ns_in2 = ns_in / 2;
        let ns_out = 2 * ns_in;
        let mut out = VMesh::new_adj(n, ns_out, vm_in.bounds.clone());
        for i in 0..n {
            out.set_co(i, 0, 0, vm_in.co(i, 0, 0));
            for k in 1..ns_in {
                let mut co = vm_in.co(i, 0, k);
                let co1 = vm_in.co(i, 0, k - 1);
                let co2 = vm_in.co(i, 0, k + 1);
                let acc = madd(add(co1, co2), co, -2.0);
                co = madd(co, acc, -1.0 / 6.0);
                let ix = out.canon_idx(i, 0, 2 * k);
                out.mesh[ix].co = co;
            }
        }
        for i in 0..n {
            let mut k = 1;
            while k < ns_out {
                let mut co = self.get_profile_point(&out.bounds[i].profile, k, ns_out);
                let co1 = out.canon_co(i, 0, k - 1);
                let co2 = out.canon_co(i, 0, k + 1);
                let acc = madd(add(co1, co2), co, -2.0);
                co = madd(co, acc, -1.0 / 6.0);
                let ix = out.canon_idx(i, 0, k);
                out.mesh[ix].co = co;
                k += 2;
            }
        }
        out.copy_equiv();
        for i in 0..n {
            for k in 0..ns_in {
                let c = out.co(i, 0, 2 * k);
                vm_in.set_co(i, 0, k, c);
            }
        }
        vm_in.copy_equiv();
        // face vertices
        for i in 0..n {
            for j in 0..ns_in2 {
                for k in 0..ns_in2 {
                    let co = avg4(vm_in.co(i, j, k), vm_in.co(i, j, k + 1), vm_in.co(i, j + 1, k), vm_in.co(i, j + 1, k + 1));
                    out.set_co(i, 2 * j + 1, 2 * k + 1, co);
                }
            }
        }
        for i in 0..n {
            for j in 0..ns_in2 {
                for k in 1..=ns_in2 {
                    let co = avg4(vm_in.co(i, j, k), vm_in.co(i, j + 1, k), out.canon_co(i, 2 * j + 1, 2 * k - 1), out.canon_co(i, 2 * j + 1, 2 * k + 1));
                    out.set_co(i, 2 * j + 1, 2 * k, co);
                }
            }
        }
        for i in 0..n {
            for j in 1..ns_in2 {
                for k in 0..ns_in2 {
                    let co = avg4(vm_in.co(i, j, k), vm_in.co(i, j, k + 1), out.canon_co(i, 2 * j - 1, 2 * k + 1), out.canon_co(i, 2 * j + 1, 2 * k + 1));
                    out.set_co(i, 2 * j, 2 * k + 1, co);
                }
            }
        }
        let gamma = 0.25;
        let beta = -gamma;
        for i in 0..n {
            for j in 1..ns_in2 {
                for k in 1..=ns_in2 {
                    let co1 = avg4(out.canon_co(i, 2 * j, 2 * k - 1), out.canon_co(i, 2 * j, 2 * k + 1), out.canon_co(i, 2 * j - 1, 2 * k), out.canon_co(i, 2 * j + 1, 2 * k));
                    let co2 = avg4(out.canon_co(i, 2 * j - 1, 2 * k - 1), out.canon_co(i, 2 * j + 1, 2 * k - 1), out.canon_co(i, 2 * j - 1, 2 * k + 1), out.canon_co(i, 2 * j + 1, 2 * k + 1));
                    let mut co = co1;
                    co = madd(co, co2, beta);
                    co = madd(co, vm_in.co(i, j, k), gamma);
                    out.set_co(i, 2 * j, 2 * k, co);
                }
            }
        }
        out.copy_equiv();
        // centre vertex
        let gamma = sabin_gamma(n);
        let beta = -gamma;
        let mut co1 = [0.0; 3];
        let mut co2 = [0.0; 3];
        for i in 0..n {
            co1 = add(co1, out.co(i, ns_in, ns_in - 1));
            co2 = add(co2, out.co(i, ns_in - 1, ns_in - 1));
            co2 = add(co2, out.co(i, ns_in - 1, ns_in + 1));
        }
        let mut co = mul(co1, 1.0 / n as f64);
        co = madd(co, co2, beta / (2.0 * n as f64));
        co = madd(co, vm_in.co(0, ns_in2, ns_in2), gamma);
        for i in 0..n {
            out.set_co(i, ns_in, ns_in, co);
        }
        // profile vertices on the boundary
        for i in 0..n {
            let inext = (i + 1) % n;
            for k in 0..=ns_out {
                let c = self.get_profile_point(&out.bounds[i].profile, k, ns_out);
                out.set_co(i, 0, k, c);
                if k >= ns_in && k < ns_out {
                    out.set_co(inext, ns_out - k, 0, c);
                }
            }
        }
        out
    }

    fn make_cube_corner_adj_vmesh(&self) -> VMesh {
        let nseg = self.seg;
        let r = PRO_CIRCLE_R;
        let mut bounds = vec![];
        for i in 0..3 {
            let mut co = [0.0; 3];
            co[i] = 1.0;
            bounds.push(BoundVert { co, v: None, efirst: None, elast: None, eon: None, ebev: None, index: i, sinratio: 1.0, adjchain: None, profile: Profile { super_r: 1.0, ..Default::default() }, visited: false });
        }
        let mut vm0 = VMesh::new_adj(3, 2, bounds);
        for i in 0..3 {
            let mut coc = [0.0; 3];
            coc[i] = 1.0;
            coc[(i + 1) % 3] = 1.0;
            coc[(i + 2) % 3] = 0.0;
            let start = vm0.bounds[i].co;
            let end = vm0.bounds[(i + 1) % 3].co;
            let mut pro = Profile { super_r: r, start, end, middle: coc, ..Default::default() };
            pro.plane_co = start;
            pro.plane_no = cross(start, end);
            pro.proj_dir = pro.plane_no;
            self.calc_profile(&mut pro);
            vm0.bounds[i].profile = pro.clone();
            vm0.set_co(i, 0, 0, start);
            // sample the profile halfway
            let half = self.get_profile_point(&pro, 1, 2);
            vm0.set_co(i, 0, 1, half);
        }
        let mut co = [1.0 / 3f64.sqrt(); 3];
        if nseg > 2 && r > 1.5 {
            co = mul(co, 1.4);
        }
        vm0.set_co(0, 1, 1, co);
        vm0.copy_equiv();
        let mut vm1 = vm0;
        while vm1.ns < nseg {
            let mut vin = vm1;
            vm1 = self.cubic_subdiv(&mut vin);
        }
        if vm1.ns != nseg {
            vm1 = self.interp_vmesh(&vm1, nseg);
        }
        let ns2 = nseg / 2;
        for i in 0..3 {
            for j in 0..=ns2 {
                for k in 0..=nseg {
                    let c = norm(vm1.co(i, j, k));
                    vm1.set_co(i, j, k, c);
                }
            }
        }
        vm1
    }

    fn tri_corner_test(&self, bvi: usize) -> i32 {
        let bv = &self.bvs[bvi];
        if bv.vm.n != 3 {
            return 0;
        }
        let offset = bv.edges[0].offset_l;
        let mut in_plane = 0;
        let mut totang = 0.0;
        for eh in &bv.edges {
            let ang = self.signed_face_angle(eh.e);
            let absang = ang.abs();
            if absang <= std::f64::consts::FRAC_PI_4 {
                in_plane += 1;
            } else if absang >= 3.0 * std::f64::consts::FRAC_PI_4 {
                return -1;
            }
            if eh.is_bev && (eh.offset_l - offset).abs() > EPS {
                return -1;
            }
            totang += ang;
        }
        if in_plane != bv.edges.len() as i32 - 3 {
            return -1;
        }
        let angdiff = (totang.abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs();
        if angdiff > std::f64::consts::FRAC_PI_4 {
            return -1;
        }
        if bv.edges.len() != 3 || bv.selcount != 3 {
            return 0;
        }
        1
    }

    /// BM_edge_calc_face_angle_signed_ex: angle between face normals, positive for convex edges
    fn signed_face_angle(&self, e: usize) -> f64 {
        let ed = &self.edges[e];
        if ed.faces.len() != 2 {
            return 0.0;
        }
        let (f1, f2) = (ed.faces[0], ed.faces[1]);
        let (n1, n2) = (self.fno[f1], self.fno[f2]);
        let ang = angle_n(n1, n2);
        // convexity: the vertex of f2 opposite to the edge lies below the plane of f1 for a convex edge
        let fv2 = &self.m.faces[f2];
        let c2: V = {
            let mut s = [0.0; 3];
            for &v in fv2 {
                s = add(s, self.co(v));
            }
            mul(s, 1.0 / fv2.len() as f64)
        };
        let p = self.co(ed.v[0]);
        if dot(n1, sub(c2, p)) > 0.0 {
            -ang
        } else {
            ang
        }
    }

    fn tri_corner_adj_vmesh(&self, bvi: usize) -> VMesh {
        let bv = &self.bvs[bvi];
        let co0 = bv.vm.bounds[0].co;
        let co1 = bv.vm.bounds[1].co;
        let co2 = bv.vm.bounds[2].co;
        let mat = make_unit_cube_map(co0, co1, co2, self.co(bv.v));
        let ns = self.seg;
        let ns2 = ns / 2;
        let mut vm = self.make_cube_corner_adj_vmesh();
        for i in 0..3 {
            for j in 0..=ns2 {
                for k in 0..=ns {
                    let c = map_apply_(&mat, vm.co(i, j, k));
                    vm.set_co(i, j, k, c);
                }
            }
        }
        vm
    }

    fn adj_vmesh(&self, bvi: usize) -> Result<VMesh, String> {
        let bv = &self.bvs[bvi];
        let n_bndv = bv.vm.n;
        if n_bndv == 3 && self.tri_corner_test(bvi) != -1 {
            return Ok(self.tri_corner_adj_vmesh(bvi));
        }
        let nseg = bv.vm.ns;
        let mut vm0 = VMesh::new_adj(n_bndv, 2, bv.vm.bounds.clone());
        let mut center = [0.0; 3];
        for i in 0..n_bndv {
            vm0.set_co(i, 0, 0, bv.vm.bounds[i].co);
            let h = self.get_profile_point(&bv.vm.bounds[i].profile, 1, 2);
            vm0.set_co(i, 0, 1, h);
            center = add(center, bv.vm.bounds[i].co);
        }
        center = mul(center, 1.0 / n_bndv as f64);
        let orig = self.co(bv.v);
        let fullness = self.fullness();
        let cd = sub(orig, center);
        if dot(cd, cd) > 1e-12 {
            vm0.set_co(0, 1, 1, madd(center, cd, fullness));
        } else {
            vm0.set_co(0, 1, 1, center);
        }
        vm0.copy_equiv();
        let mut vm1 = vm0;
        loop {
            let mut vin = vm1;
            vm1 = self.cubic_subdiv(&mut vin);
            if vm1.ns >= nseg {
                break;
            }
        }
        if vm1.ns != nseg {
            vm1 = self.interp_vmesh(&vm1, nseg);
        }
        Ok(vm1)
    }

    fn fullness(&self) -> f64 {
        const CF: [f64; 11] = [0.0, 0.559, 0.642, 0.551, 0.646, 0.624, 0.646, 0.619, 0.647, 0.639, 0.647];
        if self.seg > 0 && self.seg <= 11 {
            CF[self.seg - 1]
        } else if self.seg % 2 == 0 {
            2.4506 * 0.5 - 0.000003 * self.seg as f64 - 0.6266
        } else {
            2.3635 * 0.5 + 0.000152 * self.seg as f64 - 0.6060
        }
    }
}

// ------------------------------------------------------------------------------------------ building
impl<'a> Bevel<'a> {
    fn new_out_vert(&mut self, co: V) -> u32 {
        self.out_pos.push(co);
        (self.out_pos.len() - 1) as u32
    }

    fn pipe_test(&self, bvi: usize) -> bool {
        let bv = &self.bvs[bvi];
        let n = bv.vm.n;
        if n < 3 || n > 4 || bv.selcount < 3 || bv.selcount > 4 {
            return false;
        }
        let v = self.co(bv.v);
        let mut epipe = None;
        let mut dir1 = [0.0; 3];
        for i in 0..n {
            let (b1, b2, b3) = (&bv.vm.bounds[i], &bv.vm.bounds[(i + 1) % n], &bv.vm.bounds[(i + 2) % n]);
            if let (Some(e1), Some(_), Some(e3)) = (b1.ebev, b2.ebev, b3.ebev) {
                let d1 = norm(sub(v, self.co(bv.edges[e1].other)));
                let d3 = norm(sub(self.co(bv.edges[e3].other), v));
                if angle_n(d1, d3) < EPS_ANG {
                    epipe = Some(e1);
                    dir1 = d1;
                    break;
                }
            }
        }
        if epipe.is_none() {
            return false;
        }
        for eh in &bv.edges {
            if let Some(f) = eh.fnext {
                if dot(dir1, self.fno[f]).abs() > EPS_BIG {
                    return false;
                }
            }
        }
        true
    }

    fn build_vmesh(&mut self, bvi: usize) -> Result<(), String> {
        let ns = self.seg;
        let ns2 = ns / 2;
        let n = self.bvs[bvi].vm.bounds.len();
        self.bvs[bvi].vm.n = n;
        self.bvs[bvi].vm.ns = ns;
        self.bvs[bvi].vm.mesh = vec![NewVert { co: [0.0; 3], v: None }; n * (ns2 + 1) * (ns + 1)];
        let weld = self.bvs[bvi].selcount == 2 && n == 2;
        let mut weld1 = None;
        let mut weld2 = None;
        for i in 0..n {
            let co = self.bvs[bvi].vm.bounds[i].co;
            let id = self.new_out_vert(co);
            self.bvs[bvi].vm.bounds[i].v = Some(id);
            let ix = vm_idx(ns, i, 0, 0);
            self.bvs[bvi].vm.mesh[ix] = NewVert { co, v: Some(id) };
            if weld && self.bvs[bvi].vm.bounds[i].ebev.is_some() {
                if weld1.is_none() {
                    weld1 = Some(i);
                } else {
                    weld2 = Some(i);
                    self.set_profile_params(bvi, weld1.unwrap());
                    self.set_profile_params(bvi, i);
                    self.move_weld_profile_planes(bvi, weld1.unwrap(), i);
                }
            }
        }
        // calculate_vm_profiles
        for i in 0..n {
            if !self.bvs[bvi].vm.bounds[i].profile.special_params {
                self.set_profile_params(bvi, i);
            }
            let mut pro = self.bvs[bvi].vm.bounds[i].profile.clone();
            self.calc_profile(&mut pro);
            self.bvs[bvi].vm.bounds[i].profile = pro;
        }
        for i in 0..n {
            let nx = (i + 1) % n;
            let src = self.bvs[bvi].vm.mesh[vm_idx(ns, nx, 0, 0)].clone();
            self.bvs[bvi].vm.mesh[vm_idx(ns, i, 0, ns)] = src;
            if self.bvs[bvi].vm.kind != MeshKind::Adj {
                for k in 1..ns {
                    if self.bvs[bvi].vm.bounds[i].ebev.is_some() {
                        let co = self.get_profile_point(&self.bvs[bvi].vm.bounds[i].profile, k, ns);
                        let id = if !weld { Some(self.new_out_vert(co)) } else { None };
                        self.bvs[bvi].vm.mesh[vm_idx(ns, i, 0, k)] = NewVert { co, v: id };
                    } else if n == 2 {
                        let src = self.bvs[bvi].vm.mesh[vm_idx(ns, 1 - i, 0, ns - k)].clone();
                        self.bvs[bvi].vm.mesh[vm_idx(ns, i, 0, k)] = src;
                    }
                }
            }
        }
        if weld {
            let (w1, w2) = (weld1.ok_or("weld1")?, weld2.ok_or("weld2")?);
            self.bvs[bvi].vm.kind = MeshKind::None;
            for k in 1..ns {
                let c1 = self.bvs[bvi].vm.mesh[vm_idx(ns, w1, 0, k)].co;
                let c2 = self.bvs[bvi].vm.mesh[vm_idx(ns, w2, 0, ns - k)].co;
                let (s1, s2) = (self.bvs[bvi].vm.bounds[w1].profile.super_r, self.bvs[bvi].vm.bounds[w2].profile.super_r);
                let co = if s1 == 1.0 && s2 != 1.0 {
                    c2
                } else if s2 == 1.0 && s1 != 1.0 {
                    c1
                } else {
                    mid(c1, c2)
                };
                let id = self.new_out_vert(co);
                self.bvs[bvi].vm.mesh[vm_idx(ns, w1, 0, k)] = NewVert { co, v: Some(id) };
            }
            for k in 1..ns {
                let src = self.bvs[bvi].vm.mesh[vm_idx(ns, w1, 0, k)].clone();
                self.bvs[bvi].vm.mesh[vm_idx(ns, w2, 0, ns - k)] = src;
            }
        }
        if (n == 3 || n == 4) && ns > 1 && self.pipe_test(bvi) {
            return Err("pipe vertex pattern not supported".into());
        }
        match self.bvs[bvi].vm.kind {
            MeshKind::None => {}
            MeshKind::Poly => {
                self.build_poly(bvi);
            }
            MeshKind::TriFan => {
                if let Some(f) = self.build_poly(bvi) {
                    // fan from the last vertex
                    let fv = self.out_faces[f].clone();
                    self.out_faces.truncate(f);
                    let last = *fv.last().unwrap();
                    for i in 0..fv.len() - 2 {
                        self.out_faces.push(vec![last, fv[i], fv[i + 1]]);
                    }
                }
            }
            MeshKind::Adj => self.build_rings(bvi)?,
        }
        Ok(())
    }

    fn build_poly(&mut self, bvi: usize) -> Option<usize> {
        let bv = &self.bvs[bvi];
        let n = bv.vm.n;
        let ns = bv.vm.ns;
        let mut verts = vec![];
        for i in 0..n {
            verts.push(bv.vm.bounds[i].v.unwrap());
            if let Some(eb) = bv.vm.bounds[i].ebev {
                if bv.edges[eb].seg > 1 {
                    for k in 1..bv.edges[eb].seg {
                        verts.push(bv.vm.mesh[vm_idx(ns, i, 0, k)].v.unwrap());
                    }
                }
            }
        }
        if verts.len() > 2 {
            self.out_faces.push(verts);
            Some(self.out_faces.len() - 1)
        } else {
            None
        }
    }

    fn build_rings(&mut self, bvi: usize) -> Result<(), String> {
        let n = self.bvs[bvi].vm.n;
        let ns = self.seg;
        let ns2 = ns / 2;
        let odd = ns % 2;
        let vm1 = if self.tri_corner_test(bvi) == 1 { self.tri_corner_adj_vmesh(bvi) } else { self.adj_vmesh(bvi)? };
        for i in 0..n {
            for j in 0..=ns2 {
                for k in 0..=ns {
                    if j == 0 && (k == 0 || k == ns) {
                        continue;
                    }
                    if !self.bvs[bvi].vm.is_canon(i, j, k) {
                        continue;
                    }
                    let co = vm1.co(i, j, k);
                    let id = self.new_out_vert(co);
                    self.bvs[bvi].vm.mesh[vm_idx(ns, i, j, k)] = NewVert { co, v: Some(id) };
                }
            }
        }
        self.bvs[bvi].vm.copy_equiv();
        let vm = &self.bvs[bvi].vm;
        let mut faces = vec![];
        for i in 0..n {
            for j in 0..ns2 {
                for k in 0..(ns2 + odd) {
                    let q = [vm.mesh[vm.mv(i, j, k)].v.unwrap(), vm.mesh[vm.mv(i, j, k + 1)].v.unwrap(), vm.mesh[vm.mv(i, j + 1, k + 1)].v.unwrap(), vm.mesh[vm.mv(i, j + 1, k)].v.unwrap()];
                    faces.push(q.to_vec());
                }
            }
        }
        if odd == 1 {
            let c: Vec<u32> = (0..n).map(|i| vm.mesh[vm.mv(i, ns2, ns2)].v.unwrap()).collect();
            faces.push(c);
        }
        self.out_faces.extend(faces);
        Ok(())
    }

    fn build_edge_polygons(&mut self, e: usize) {
        if self.edges[e].faces.len() != 2 {
            return;
        }
        let (va, vb) = (self.edges[e].v[0], self.edges[e].v[1]);
        let (bv1, bv2) = (self.bv_of[&va], self.bv_of[&vb]);
        let e1 = self.bvs[bv1].edges.iter().position(|x| x.e == e).unwrap();
        let e2 = self.bvs[bv2].edges.iter().position(|x| x.e == e).unwrap();
        let nseg = self.bvs[bv1].edges[e1].seg;
        let (l1, r1) = (self.bvs[bv1].edges[e1].leftv.unwrap(), self.bvs[bv1].edges[e1].rightv.unwrap());
        let (l2, r2) = (self.bvs[bv2].edges[e2].leftv.unwrap(), self.bvs[bv2].edges[e2].rightv.unwrap());
        let ns = self.seg;
        let b = |s: &Self, bvi: usize, i: usize, k: usize| s.bvs[bvi].vm.mesh[vm_idx(ns, i, 0, k)].v.unwrap();
        let mut verts = [self.bvs[bv1].vm.bounds[l1].v.unwrap(), self.bvs[bv2].vm.bounds[r2].v.unwrap(), 0, 0];
        let _ = r1;
        for k in 1..=nseg {
            verts[3] = b(self, bv1, l1, k);
            verts[2] = b(self, bv2, l2, nseg - k);
            self.out_faces.push(verts.to_vec());
            verts[0] = verts[3];
            verts[1] = verts[2];
        }
    }

    fn rebuild_face(&mut self, fi: usize) -> Option<Vec<u32>> {
        let f = self.m.faces[fi].clone();
        let k = f.len();
        let mut any = false;
        let mut vv: Vec<u32> = vec![];
        for i in 0..k {
            let v = f[i];
            if !self.vtag[v as usize] || !self.bv_of.contains_key(&v) {
                vv.push(v);
                continue;
            }
            any = true;
            let bvi = self.bv_of[&v];
            let w = f[(i + 1) % k];
            let u = f[(i + k - 1) % k];
            let bv = &self.bvs[bvi];
            let n = bv.edges.len();
            let ei = bv.edges.iter().position(|x| x.other == w).unwrap();
            let epi = bv.edges.iter().position(|x| x.other == u).unwrap();
            let go_ccw;
            if (ei + n - 1) % n == epi {
                if (epi + n - 1) % n == ei {
                    go_ccw = bv.edges[ei].fnext != Some(fi);
                } else {
                    go_ccw = true;
                }
            } else if (epi + n - 1) % n == ei {
                go_ccw = false;
            } else {
                let cnt = |a: usize, b: usize| -> usize {
                    let mut c = 0;
                    let mut e = a;
                    loop {
                        if e == b {
                            break;
                        }
                        e = (e + 1) % n;
                        c += 1;
                        if e == a {
                            break;
                        }
                    }
                    c
                };
                go_ccw = cnt(epi, ei) < cnt(ei, epi);
            }
            let vm = &bv.vm;
            let nb = vm.bounds.len();
            let mut on_profile_start = false;
            let (mut vstart, vend);
            if go_ccw {
                vstart = bv.edges[epi].rightv.unwrap();
                vend = bv.edges[ei].leftv.unwrap();
                if bv.edges[ei].profile_index > 0 {
                    vstart = (vstart + nb - 1) % nb;
                    on_profile_start = true;
                }
            } else {
                vstart = bv.edges[epi].leftv.unwrap();
                vend = bv.edges[ei].rightv.unwrap();
                if bv.edges[epi].profile_index > 0 {
                    vstart = (vstart + 1) % nb;
                    on_profile_start = true;
                }
            }
            let mut b = vstart;
            if !on_profile_start {
                vv.push(vm.bounds[b].v.unwrap());
            }
            let ns = vm.ns;
            while b != vend {
                if go_ccw {
                    let kstart = if on_profile_start {
                        on_profile_start = false;
                        bv.edges[ei].profile_index as usize
                    } else {
                        1
                    };
                    let kend = if bv.edges[epi].rightv == Some(b) && bv.edges[epi].profile_index > 0 { bv.edges[epi].profile_index as usize } else { ns };
                    for kk in kstart..=kend {
                        if let Some(id) = vm.mesh[vm_idx(ns, b, 0, kk)].v {
                            vv.push(id);
                        }
                    }
                    b = (b + 1) % nb;
                } else {
                    let i2 = (b + nb - 1) % nb;
                    let kstart: i64 = if on_profile_start {
                        on_profile_start = false;
                        bv.edges[epi].profile_index as i64
                    } else {
                        ns as i64 - 1
                    };
                    let kend: i64 = if bv.edges[ei].rightv == Some(i2) && bv.edges[ei].profile_index > 0 { bv.edges[ei].profile_index as i64 } else { 0 };
                    let mut kk = kstart;
                    while kk >= kend {
                        if let Some(id) = vm.mesh[vm_idx(ns, i2, 0, kk as usize)].v {
                            vv.push(id);
                        }
                        kk -= 1;
                    }
                    b = i2;
                }
            }
        }
        if any {
            Some(vv)
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------------------------------ offset adjustment
fn solve_lsq(a: &Vec<Vec<f64>>, b: &Vec<f64>, np: usize) -> Vec<f64> {
    // normal equations with Gaussian elimination (np is small)
    let nrows = a.len();
    let mut m = vec![vec![0.0; np + 1]; np];
    for i in 0..np {
        for j in 0..np {
            let mut s = 0.0;
            for r in 0..nrows {
                s += a[r][i] * a[r][j];
            }
            m[i][j] = s;
        }
        let mut s = 0.0;
        for r in 0..nrows {
            s += a[r][i] * b[r];
        }
        m[i][np] = s;
    }
    for i in 0..np {
        let mut p = i;
        for r in i + 1..np {
            if m[r][i].abs() > m[p][i].abs() {
                p = r;
            }
        }
        m.swap(i, p);
        let d = m[i][i];
        if d.abs() < 1e-18 {
            continue;
        }
        for c in i..=np {
            m[i][c] /= d;
        }
        for r in 0..np {
            if r != i {
                let f = m[r][i];
                if f != 0.0 {
                    for c in i..=np {
                        m[r][c] -= f * m[i][c];
                    }
                }
            }
        }
    }
    (0..np).map(|i| m[i][np]).collect()
}

impl<'a> Bevel<'a> {
    fn adjust_chain(&mut self, vstart: (usize, usize), iscycle: bool) {
        // collect the chain
        let mut chain = vec![];
        let mut v = Some(vstart);
        while let Some(x) = v {
            chain.push(x);
            v = self.bvs[x.0].vm.bounds[x.1].adjchain;
            if v == Some(vstart) {
                break;
            }
        }
        let np = chain.len();
        let nrows = if iscycle { 3 * np } else { 3 * np - 3 };
        let mut a = vec![vec![0.0; np]; nrows];
        let mut b = vec![0.0; nrows];
        let weight = 0.2;
        for (i, &(bi, vi)) in chain.iter().enumerate() {
            let bnd = &self.bvs[bi].vm.bounds[vi];
            if iscycle || i < np - 1 {
                let eright = bnd.efirst.unwrap();
                let nxt = bnd.adjchain.unwrap();
                let enextleft = self.bvs[nxt.0].vm.bounds[nxt.1].elast.unwrap();
                a[i][i] += 1.0;
                if iscycle {
                    a[if i > 0 { i - 1 } else { np - 1 }][i] += -bnd.sinratio;
                } else if i > 0 {
                    a[i - 1][i] += -bnd.sinratio;
                }
                let row = if iscycle { np + 2 * i } else { np - 1 + 2 * i };
                a[row][i] += weight;
                b[row] += weight * self.bvs[bi].edges[eright].offset_r;
                let row = row + 1;
                let col = if i == np - 1 { 0 } else { i + 1 };
                a[row][col] += weight * self.bvs[nxt.0].vm.bounds[nxt.1].sinratio;
                b[row] += weight * self.bvs[nxt.0].edges[enextleft].offset_l;
            } else {
                a[i - 1][i] += -1.0;
            }
        }
        let sol = solve_lsq(&a, &b, np);
        for (i, &(bi, vi)) in chain.iter().enumerate() {
            let val = sol[i];
            let (efirst, elast, sr) = {
                let bnd = &self.bvs[bi].vm.bounds[vi];
                (bnd.efirst, bnd.elast, bnd.sinratio)
            };
            if iscycle || i < np - 1 {
                self.bvs[bi].edges[efirst.unwrap()].offset_r = val;
                if iscycle || i > 0 {
                    self.bvs[bi].edges[elast.unwrap()].offset_l = sr * val;
                }
            } else {
                self.bvs[bi].edges[elast.unwrap()].offset_l = val;
            }
        }
    }

    fn adjust_offsets(&mut self) -> Result<(), String> {
        let nbv = self.bvs.len();
        // iterate bevverts in vertex index order
        let mut order: Vec<usize> = (0..nbv).collect();
        order.sort_by_key(|&i| self.bvs[i].v);
        for &bvi in &order {
            let nb = self.bvs[bvi].vm.bounds.len();
            for ai in 0..nb {
                {
                    let a = &self.bvs[bvi].vm.bounds[ai];
                    if a.visited || a.eon.is_none() {
                        continue;
                    }
                }
                let anchor = (bvi, ai);
                let mut v = anchor;
                let mut vchainstart = anchor;
                let mut vchainend = anchor;
                let mut iscycle = false;
                let mut chainlen = 1;
                loop {
                    let (bcur, vi) = v;
                    {
                        let b = &self.bvs[bcur].vm.bounds[vi];
                        if !(b.eon.is_some() && !b.visited && !iscycle) {
                            break;
                        }
                    }
                    self.bvs[bcur].vm.bounds[vi].visited = true;
                    let ef = match self.bvs[bcur].vm.bounds[vi].efirst {
                        Some(e) => e,
                        None => break,
                    };
                    let (bo, eo) = match self.find_other_end(bcur, ef) {
                        Some(x) => x,
                        None => break,
                    };
                    let vnext = (bo, self.bvs[bo].edges[eo].leftv.unwrap());
                    self.bvs[bcur].vm.bounds[vi].adjchain = Some(vnext);
                    vchainend = vnext;
                    chainlen += 1;
                    if self.bvs[vnext.0].vm.bounds[vnext.1].visited {
                        if vnext != vchainstart {
                            break;
                        }
                        self.adjust_chain(vchainstart, true);
                        iscycle = true;
                    }
                    v = vnext;
                }
                if !iscycle {
                    self.bvs[v.0].vm.bounds[v.1].adjchain = None;
                    v = vchainstart;
                    loop {
                        let (bcur, vi) = v;
                        self.bvs[bcur].vm.bounds[vi].visited = true;
                        let el = match self.bvs[bcur].vm.bounds[vi].elast {
                            Some(e) => e,
                            None => break,
                        };
                        let (bo, eo) = match self.find_other_end(bcur, el) {
                            Some(x) => x,
                            None => break,
                        };
                        let vnext = (bo, self.bvs[bo].edges[eo].rightv.unwrap());
                        self.bvs[vnext.0].vm.bounds[vnext.1].adjchain = Some(v);
                        chainlen += 1;
                        vchainstart = vnext;
                        v = vnext;
                        let b = &self.bvs[v.0].vm.bounds[v.1];
                        if !(!b.visited && b.eon.is_some()) {
                            break;
                        }
                    }
                    let s = &self.bvs[vchainstart.0].vm.bounds[vchainstart.1];
                    let e = &self.bvs[vchainend.0].vm.bounds[vchainend.1];
                    if chainlen >= 3 && s.eon.is_none() && e.eon.is_none() {
                        self.adjust_chain(vchainstart, false);
                    }
                }
            }
        }
        for bvi in 0..nbv {
            self.build_boundary(bvi, false)?;
        }
        Ok(())
    }
}

fn map_apply_unused() {}

/// BEVEL modifier (limit method ANGLE, width offset, circular profile): `angle_limit` in radians
pub fn bevel_angle(m: &Mesh, width: f64, segments: usize, angle_limit: f64, width_mode: bool) -> Result<Mesh, String> {
    let mut b = Bevel::new(m, width, segments.max(1), width_mode);
    // tag edges
    for e in 0..b.edges.len() {
        if b.edges[e].faces.len() == 2 {
            let ang = angle_n(b.fno[b.edges[e].faces[0]], b.fno[b.edges[e].faces[1]]);
            if ang.abs() > angle_limit {
                b.etag[e] = true;
                for &v in &b.edges[e].v.clone() {
                    b.vtag[v as usize] = true;
                }
            }
        }
    }
    bevel_tagged(b)
}

fn bevel_tagged(mut b: Bevel) -> Result<Mesh, String> {
    let nv = b.m.pos.len();
    b.out_pos = b.m.pos.clone();
    for v in 0..nv as u32 {
        if b.vtag[v as usize] {
            b.construct(v)?;
        }
    }
    if b.bvs.is_empty() {
        return Ok(b.m.clone());
    }
    b.limit_offset();
    for bvi in 0..b.bvs.len() {
        b.build_boundary(bvi, true)?;
    }
    b.adjust_offsets()?;
    // vertex meshes in vertex order
    let mut order: Vec<usize> = (0..b.bvs.len()).collect();
    order.sort_by_key(|&i| b.bvs[i].v);
    for &bvi in &order {
        b.build_vmesh(bvi)?;
    }
    for e in 0..b.edges.len() {
        if b.etag[e] {
            b.build_edge_polygons(e);
        }
    }
    // rebuilt faces
    let mut faces = vec![];
    for fi in 0..b.m.faces.len() {
        match b.rebuild_face(fi) {
            Some(f) => faces.push(f),
            None => faces.push(b.m.faces[fi].clone()),
        }
    }
    faces.extend(b.out_faces.drain(..));
    // compact: drop original tagged vertices
    let mut keep = vec![false; b.out_pos.len()];
    for f in &faces {
        for &v in f {
            keep[v as usize] = true;
        }
    }
    let mut map = vec![u32::MAX; b.out_pos.len()];
    let mut out = Mesh::new();
    for (i, p) in b.out_pos.iter().enumerate() {
        if keep[i] {
            map[i] = out.pos.len() as u32;
            out.pos.push(*p);
        }
    }
    for f in faces {
        out.faces.push(f.iter().map(|&v| map[v as usize]).collect());
    }
    Ok(out)
}
