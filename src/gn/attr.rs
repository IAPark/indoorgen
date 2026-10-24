//! Attribute access, domain interpolation (Blender "adapt domain" semantics) and built-in geometry fields.
use super::field::*;
use super::geom::*;
use super::math::*;
use super::val::*;

/// Accumulating mixer (Blender attribute_math::DefaultMixer)
pub struct Mixer {
    ty: Ty,
    acc: Vec<[f32; 4]>,
    accd: Vec<f64>,
    w: Vec<f32>,
    any: Vec<bool>,
    first: Vec<Option<Val>>,
}

impl Mixer {
    pub fn new(ty: Ty, n: usize) -> Mixer {
        Mixer { ty, acc: vec![[0.0; 4]; n], accd: vec![0.0; n], w: vec![0.0; n], any: vec![false; n], first: vec![None; n] }
    }
    pub fn mix(&mut self, i: usize, v: &Val) {
        self.mix_w(i, v, 1.0)
    }
    pub fn mix_w(&mut self, i: usize, v: &Val, w: f32) {
        self.w[i] += w;
        match self.ty {
            Ty::F => self.acc[i][0] += v.f() * w,
            Ty::I => self.accd[i] += v.i() as f64 * w as f64,
            Ty::B => self.any[i] |= v.b(),
            Ty::V => {
                let x = v.v();
                for k in 0..3 {
                    self.acc[i][k] += x[k] * w;
                }
            }
            Ty::C | Ty::Q => {
                let x = if self.ty == Ty::C { v.c() } else { v.q() };
                for k in 0..4 {
                    self.acc[i][k] += x[k] * w;
                }
            }
            _ => {
                if self.first[i].is_none() {
                    self.first[i] = Some(v.clone());
                }
            }
        }
    }
    pub fn finish(&self, i: usize) -> Val {
        let t = self.w[i];
        if t == 0.0 {
            return Val::zero(self.ty);
        }
        match self.ty {
            Ty::F => Val::F(self.acc[i][0] / t),
            Ty::I => Val::I((self.accd[i] / t as f64).round() as i32),
            Ty::B => Val::B(self.any[i]),
            Ty::V => Val::V([self.acc[i][0] / t, self.acc[i][1] / t, self.acc[i][2] / t]),
            Ty::C => Val::C([self.acc[i][0] / t, self.acc[i][1] / t, self.acc[i][2] / t, self.acc[i][3] / t]),
            Ty::Q => {
                let q = [self.acc[i][0] / t, self.acc[i][1] / t, self.acc[i][2] / t, self.acc[i][3] / t];
                Val::Q(q)
            }
            _ => self.first[i].clone().unwrap_or(Val::zero(self.ty)),
        }
    }
}

pub fn adapt_mesh(m: &Mesh, from: Dom, to: Dom, ty: Ty, src: &[Val]) -> Vec<Val> {
    if from == to {
        return src.to_vec();
    }
    let n = m.size(to);
    let mut mx = Mixer::new(ty, n);
    match (from, to) {
        (Dom::Point, Dom::Edge) => {
            for (i, e) in m.edges.iter().enumerate() {
                mx.mix(i, &src[e[0] as usize]);
                mx.mix(i, &src[e[1] as usize]);
            }
        }
        (Dom::Point, Dom::Face) => {
            for f in 0..m.nf() {
                for &v in m.face(f) {
                    mx.mix(f, &src[v as usize]);
                }
            }
        }
        (Dom::Point, Dom::Corner) => return m.cv.iter().map(|&v| src[v as usize].clone()).collect(),
        (Dom::Edge, Dom::Point) => {
            for (i, e) in m.edges.iter().enumerate() {
                mx.mix(e[0] as usize, &src[i]);
                mx.mix(e[1] as usize, &src[i]);
            }
        }
        (Dom::Edge, Dom::Face) => {
            for f in 0..m.nf() {
                for c in m.face_range(f) {
                    mx.mix(f, &src[m.ce[c] as usize]);
                }
            }
        }
        (Dom::Edge, Dom::Corner) => {
            let mut out = vec![Val::zero(ty); m.nc()];
            for f in 0..m.nf() {
                let r = m.face_range(f);
                let k = r.len();
                for c in 0..k {
                    let prev = r.start + (c + k - 1) % k;
                    let mut mm = Mixer::new(ty, 1);
                    mm.mix(0, &src[m.ce[prev] as usize]);
                    mm.mix(0, &src[m.ce[r.start + c] as usize]);
                    out[r.start + c] = mm.finish(0);
                }
            }
            return out;
        }
        (Dom::Face, Dom::Point) => {
            for f in 0..m.nf() {
                for &v in m.face(f) {
                    mx.mix(v as usize, &src[f]);
                }
            }
        }
        (Dom::Face, Dom::Edge) => {
            for f in 0..m.nf() {
                for c in m.face_range(f) {
                    mx.mix(m.ce[c] as usize, &src[f]);
                }
            }
        }
        (Dom::Face, Dom::Corner) => {
            let mut out = Vec::with_capacity(m.nc());
            for f in 0..m.nf() {
                for _ in m.face_range(f) {
                    out.push(src[f].clone());
                }
            }
            return out;
        }
        (Dom::Corner, Dom::Point) => {
            for (c, &v) in m.cv.iter().enumerate() {
                mx.mix(v as usize, &src[c]);
            }
        }
        (Dom::Corner, Dom::Edge) => {
            for f in 0..m.nf() {
                let r = m.face_range(f);
                let k = r.len();
                for c in 0..k {
                    let prev = r.start + (c + k - 1) % k;
                    mx.mix(m.ce[r.start + c] as usize, &src[r.start + c]);
                    let _ = prev;
                }
            }
        }
        (Dom::Corner, Dom::Face) => {
            for f in 0..m.nf() {
                for c in m.face_range(f) {
                    mx.mix(f, &src[c]);
                }
            }
        }
        _ => return vec![Val::zero(ty); n],
    }
    (0..n).map(|i| mx.finish(i)).collect()
}

pub fn adapt_curves(c: &Curves, from: Dom, to: Dom, ty: Ty, src: &[Val]) -> Vec<Val> {
    if from == to {
        return src.to_vec();
    }
    match (from, to) {
        (Dom::Point, Dom::Curve) => {
            let mut mx = Mixer::new(ty, c.nc());
            for k in 0..c.nc() {
                for p in c.range(k) {
                    mx.mix(k, &src[p]);
                }
            }
            (0..c.nc()).map(|i| mx.finish(i)).collect()
        }
        (Dom::Curve, Dom::Point) => {
            let mut out = Vec::with_capacity(c.np());
            for k in 0..c.nc() {
                for _ in c.range(k) {
                    out.push(src[k].clone());
                }
            }
            out
        }
        _ => vec![Val::zero(ty); c.size(to)],
    }
}

pub fn attrs_of<'a>(c: Comp<'a>) -> Option<&'a Attrs> {
    match c {
        Comp::Mesh(m) => Some(&m.attrs),
        Comp::Curves(c) => Some(&c.attrs),
        Comp::Inst(i) => Some(&i.attrs),
        Comp::Points(p) => Some(&p.attrs),
        Comp::None => None,
    }
}

pub fn has_attr(ctx: &Ctx, name: &str) -> bool {
    attrs_of(ctx.comp).map_or(false, |a| a.contains_key(name))
}

fn builtin(comp: Comp, name: &str) -> Option<(Dom, Ty, Vec<Val>)> {
    match (comp, name) {
        (Comp::Mesh(m), "position") => Some((Dom::Point, Ty::V, m.pos.iter().map(|p| Val::V(*p)).collect())),
        (Comp::Curves(c), "position") => Some((Dom::Point, Ty::V, c.pos.iter().map(|p| Val::V(*p)).collect())),
        (Comp::Curves(c), "radius") => Some((Dom::Point, Ty::F, c.radius.iter().map(|p| Val::F(if c.has_radius { *p } else { 0.0 })).collect())),
        (Comp::Curves(c), "tilt") => Some((Dom::Point, Ty::F, c.tilt.iter().map(|p| Val::F(*p)).collect())),
        (Comp::Curves(c), "handle_left") => Some((Dom::Point, Ty::V, c.hl.iter().map(|p| Val::V(*p)).collect())),
        (Comp::Curves(c), "handle_right") => Some((Dom::Point, Ty::V, c.hr.iter().map(|p| Val::V(*p)).collect())),
        (Comp::Curves(c), "cyclic") => Some((Dom::Curve, Ty::B, c.cyclic.iter().map(|p| Val::B(*p)).collect())),
        (Comp::Curves(c), "resolution") => Some((Dom::Curve, Ty::I, c.res.iter().map(|p| Val::I(*p)).collect())),
        (Comp::Points(p), "position") => Some((Dom::Point, Ty::V, p.pos.iter().map(|p| Val::V(*p)).collect())),
        (Comp::Inst(i), "position") => Some((Dom::Instance, Ty::V, i.xf.iter().map(|m| Val::V([m[3][0], m[3][1], m[3][2]])).collect())),
        _ => None,
    }
}

/// Read an attribute on the context domain (adapting domains); missing -> zero values of `want`
pub fn read_attr(ctx: &Ctx, name: &str, want: Ty) -> Vec<Val> {
    let found: Option<(Dom, Ty, Vec<Val>)> = if let Some(b) = builtin(ctx.comp, name) {
        Some(b)
    } else {
        attrs_of(ctx.comp).and_then(|a| a.get(name)).map(|a| (a.dom, a.ty, a.data.clone()))
    };
    match found {
        None => vec![Val::zero(want)],
        Some((d, ty, data)) => {
            let adapted = match ctx.comp {
                Comp::Mesh(m) => adapt_mesh(m, d, ctx.dom, ty, &data),
                Comp::Curves(c) => adapt_curves(c, d, ctx.dom, ty, &data),
                _ => data,
            };
            if ty == want {
                adapted
            } else {
                adapted.iter().map(|v| v.convert(want)).collect()
            }
        }
    }
}

// ------------------------------------------------------------------------------ mesh normals
pub fn face_normal(m: &Mesh, f: usize) -> V3 {
    let r = m.face(f);
    let p = |i: usize| m.pos[r[i] as usize];
    match r.len() {
        3 => norm3_c(cross3(sub3(p(0), p(1)), sub3(p(1), p(2)))),
        4 => norm3_c(cross3(sub3(p(2), p(0)), sub3(p(3), p(1)))),
        n => {
            // Newell
            let mut nrm = [0.0f32; 3];
            let mut prev = p(n - 1);
            for i in 0..n {
                let cur = p(i);
                nrm[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
                nrm[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
                nrm[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
                prev = cur;
            }
            norm3_c(nrm)
        }
    }
}

pub fn face_normals(m: &Mesh) -> Vec<V3> {
    (0..m.nf()).map(|f| face_normal(m, f)).collect()
}

/// angle-weighted vertex normals (BKE normals_calc_verts)
pub fn vert_normals(m: &Mesh, fnor: &[V3]) -> Vec<V3> {
    let mut vn = vec![[0.0f32; 3]; m.nv()];
    for f in 0..m.nf() {
        let r = m.face(f);
        let n = r.len();
        for i in 0..n {
            let v = r[i] as usize;
            let vp = m.pos[r[(i + n - 1) % n] as usize];
            let vc = m.pos[v];
            let vnx = m.pos[r[(i + 1) % n] as usize];
            let d1 = norm3_c(sub3(vnx, vc));
            let d2 = norm3_c(sub3(vp, vc));
            let ang = dot3(d1, d2).clamp(-1.0, 1.0).acos();
            for k in 0..3 {
                vn[v][k] += fnor[f][k] * ang;
            }
        }
    }
    for v in vn.iter_mut() {
        *v = norm3_c(*v);
    }
    // vertices without faces: normalized position (BKE: loose verts get normalized co)
    let mut used = vec![false; m.nv()];
    for &v in &m.cv {
        used[v as usize] = true;
    }
    for (i, v) in vn.iter_mut().enumerate() {
        if !used[i] {
            *v = norm3_c(m.pos[i]);
        }
    }
    vn
}

pub fn normal_field(ctx: &Ctx) -> Vec<Val> {
    match ctx.comp {
        Comp::Mesh(m) => {
            let fnor = face_normals(m);
            match ctx.dom {
                Dom::Face => fnor.iter().map(|n| Val::V(*n)).collect(),
                Dom::Point => match &m.vnor {
                    Some(v) if v.len() == m.nv() => v.iter().map(|n| Val::V(*n)).collect(),
                    _ => vert_normals(m, &fnor).iter().map(|n| Val::V(*n)).collect(),
                },
                Dom::Corner => {
                    // smooth shading by default unless sharp_face
                    let vn = vert_normals(m, &fnor);
                    let sharp = m.attrs.get("sharp_face");
                    let mut out = Vec::with_capacity(m.nc());
                    for f in 0..m.nf() {
                        let is_sharp = sharp.map_or(false, |a| a.data[f].b());
                        for c in m.face_range(f) {
                            out.push(Val::V(if is_sharp { fnor[f] } else { vn[m.cv[c] as usize] }));
                        }
                    }
                    out
                }
                Dom::Edge => {
                    let vn: Vec<Val> = vert_normals(m, &fnor).iter().map(|n| Val::V(*n)).collect();
                    adapt_mesh(m, Dom::Point, Dom::Edge, Ty::V, &vn).iter().map(|v| Val::V(norm3_c(v.v()))).collect()
                }
                _ => vec![Val::V([0.0; 3]); ctx.n],
            }
        }
        Comp::Curves(c) => {
            if ctx.dom == Dom::Point {
                super::curveops::curve_normals(c).iter().map(|n| Val::V(*n)).collect()
            } else {
                vec![Val::V([0.0; 3]); ctx.n]
            }
        }
        _ => vec![Val::V([0.0; 3]); ctx.n],
    }
}

pub fn curve_tangent_field(ctx: &Ctx) -> Vec<Val> {
    match ctx.comp {
        Comp::Curves(c) => {
            let t = super::curveops::curve_tangents(c);
            let v: Vec<Val> = t.iter().map(|n| Val::V(*n)).collect();
            if ctx.dom == Dom::Point {
                v
            } else {
                adapt_curves(c, Dom::Point, ctx.dom, Ty::V, &v)
            }
        }
        _ => vec![Val::V([0.0; 3]); ctx.n],
    }
}

pub fn spline_param(ctx: &Ctx, which: u8) -> Vec<Val> {
    match ctx.comp {
        Comp::Curves(c) => {
            let ty = if which == 2 { Ty::I } else { Ty::F };
            match ctx.dom {
                Dom::Point => {
                    let (fac, len, idx) = super::curveops::spline_parameters(c);
                    match which {
                        0 => fac.iter().map(|x| Val::F(*x)).collect(),
                        1 => len.iter().map(|x| Val::F(*x)).collect(),
                        _ => idx.iter().map(|x| Val::I(*x)).collect(),
                    }
                }
                Dom::Curve if which < 2 => {
                    let (fac, len) = super::curveops::spline_parameters_curve(c);
                    if which == 0 { fac.iter().map(|x| Val::F(*x)).collect() } else { len.iter().map(|x| Val::F(*x)).collect() }
                }
                _ => vec![Val::zero(ty); ctx.n],
            }
        }
        _ => vec![Val::zero(if which == 2 { Ty::I } else { Ty::F }); ctx.n],
    }
}
