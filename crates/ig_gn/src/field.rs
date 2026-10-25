//! Lazy fields: evaluated per domain element in the context of a geometry component.
use super::attr::*;
use super::geom::*;
use super::val::*;
use std::cell::RefCell;
use crate::dhash::HashMap;
use std::rc::Rc;

pub type Kernel = Rc<dyn Fn(&[&[Val]], usize) -> Vec<Vec<Val>>>;

pub enum FNode {
    Const(Val),
    Pos,
    Normal,
    Index,
    /// "id" attribute if present else index
    Id,
    Named(String, Ty),
    /// 0 factor, 1 length, 2 index
    SplineParam(u8),
    Tangent,
    Mf { k: Kernel, args: Vec<Field>, out: usize },
    /// face area / edge verts / etc: generic context readers
    Ctx(Rc<dyn Fn(&Ctx) -> Vec<Val>>),
}

pub type Field = Rc<FNode>;

#[derive(Clone, Copy)]
pub enum Comp<'a> {
    None,
    Mesh(&'a Mesh),
    Curves(&'a Curves),
    Inst(&'a Instances),
    Points(&'a Points),
}

pub struct Ctx<'a> {
    pub comp: Comp<'a>,
    pub dom: Dom,
    pub n: usize,
    cache: RefCell<HashMap<*const FNode, Rc<Vec<Val>>>>,
}

pub fn comp_size(c: Comp, d: Dom) -> usize {
    match c {
        Comp::Mesh(m) => m.size(d),
        Comp::Curves(c) => c.size(d),
        Comp::Inst(i) => {
            if d == Dom::Instance {
                i.n()
            } else {
                0
            }
        }
        Comp::Points(p) => {
            if d == Dom::Point {
                p.pos.len()
            } else {
                0
            }
        }
        Comp::None => 0,
    }
}

impl<'a> Ctx<'a> {
    pub fn new(comp: Comp<'a>, dom: Dom) -> Ctx<'a> {
        Ctx { comp, dom, n: comp_size(comp, dom), cache: RefCell::new(HashMap::default()) }
    }
}

pub fn konst(v: Val) -> Field {
    Rc::new(FNode::Const(v))
}
pub fn kf(x: f32) -> Field {
    konst(Val::F(x))
}

pub fn as_const(f: &Field) -> Option<&Val> {
    match &**f {
        FNode::Const(v) => Some(v),
        _ => None,
    }
}

/// Build Mf outputs; constant-fold if all args are constants.
pub fn mf(k: Kernel, args: Vec<Field>, nout: usize) -> Vec<Field> {
    if args.iter().all(|a| as_const(a).is_some()) {
        let cols: Vec<[Val; 1]> = args.iter().map(|a| [as_const(a).unwrap().clone()]).collect();
        let sl: Vec<&[Val]> = cols.iter().map(|c| &c[..]).collect();
        let r = k(&sl, 1);
        return r.into_iter().map(|v| konst(v[0].clone())).collect();
    }
    (0..nout).map(|o| Rc::new(FNode::Mf { k: k.clone(), args: args.clone(), out: o })).collect()
}

pub fn mf1(k: Kernel, args: Vec<Field>) -> Field {
    mf(k, args, 1).remove(0)
}

pub fn conv(f: &Field, from: Ty, to: Ty) -> Field {
    if from == to {
        return f.clone();
    }
    mf1(Rc::new(move |a, n| vec![(0..n).map(|i| at(a[0], i).convert(to)).collect()]), vec![f.clone()])
}

impl Ctx<'_> {
    /// evaluate; result has length 1 (broadcast) or n
    pub fn eval(&self, f: &Field) -> Rc<Vec<Val>> {
        let key = Rc::as_ptr(f);
        if let Some(r) = self.cache.borrow().get(&key) {
            return r.clone();
        }
        let r: Vec<Val> = match &**f {
            FNode::Const(v) => vec![v.clone()],
            FNode::Pos => read_attr(self, "position", Ty::V),
            FNode::Normal => normal_field(self),
            FNode::Index => (0..self.n).map(|i| Val::I(i as i32)).collect(),
            FNode::Id => {
                if has_attr(self, "id") {
                    read_attr(self, "id", Ty::I)
                } else {
                    (0..self.n).map(|i| Val::I(i as i32)).collect()
                }
            }
            FNode::Named(name, ty) => read_attr(self, name, *ty),
            FNode::SplineParam(w) => spline_param(self, *w),
            FNode::Tangent => curve_tangent_field(self),
            FNode::Ctx(g) => g(self),
            FNode::Mf { k, args, out } => {
                let vals: Vec<Rc<Vec<Val>>> = args.iter().map(|a| self.eval(a)).collect();
                let all1 = vals.iter().all(|v| v.len() == 1) && self.n != 1;
                let nn = if all1 { 1 } else { self.n };
                let sl: Vec<&[Val]> = vals.iter().map(|v| &v[..]).collect();
                let mut r = k(&sl, nn);
                r.swap_remove(*out)
            }
        };
        let r = Rc::new(r);
        self.cache.borrow_mut().insert(key, r.clone());
        r
    }
    /// evaluate and expand to exactly n elements
    pub fn eval_n(&self, f: &Field) -> Vec<Val> {
        let r = self.eval(f);
        if r.len() == self.n {
            (*r).clone()
        } else if r.len() == 1 {
            vec![r[0].clone(); self.n]
        } else {
            let mut v = (*r).clone();
            v.resize(self.n, Val::F(0.0));
            v
        }
    }
}

pub fn eval_on(f: &Field, comp: Comp, dom: Dom) -> Vec<Val> {
    let ctx = Ctx::new(comp, dom);
    ctx.eval_n(f)
}

/// helpers for kernels: element access with broadcast
#[inline]
pub fn at<'a>(a: &'a [Val], i: usize) -> &'a Val {
    if a.len() == 1 {
        &a[0]
    } else {
        &a[i]
    }
}

pub fn map1(f: &Field, g: impl Fn(&Val) -> Val + 'static) -> Field {
    mf1(Rc::new(move |a, n| vec![(0..n).map(|i| g(at(a[0], i))).collect()]), vec![f.clone()])
}
pub fn map2(f: &Field, h: &Field, g: impl Fn(&Val, &Val) -> Val + 'static) -> Field {
    mf1(Rc::new(move |a, n| vec![(0..n).map(|i| g(at(a[0], i), at(a[1], i))).collect()]), vec![f.clone(), h.clone()])
}
