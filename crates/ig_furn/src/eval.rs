//! Constraint evaluator (evaluator/evaluate.py + node_impl/*): values, memoisation, losses and violations.
use super::col;
use super::domain::Domain;
use super::geom::*;
use super::program::*;
use super::state::*;
use super::tags::*;
use crate::mesh::{cross, dot, sub, V3};
use crate::shp;
use crate::dhash::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Val {
    Num(f64),
    Bool(bool),
    Set(Rc<Vec<String>>),
}

impl Val {
    pub fn num(&self) -> f64 {
        match self {
            Val::Num(x) => *x,
            Val::Bool(b) => *b as i32 as f64,
            Val::Set(s) => s.len() as f64,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Val::Num(x) => *x != 0.0,
            Val::Bool(b) => *b,
            Val::Set(s) => !s.is_empty(),
        }
    }
    pub fn set(&self) -> &Rc<Vec<String>> {
        match self {
            Val::Set(s) => s,
            v => panic!("expected set, got {v:?}"),
        }
    }
}

struct Entry {
    val: Val,
    vers: Vec<u64>,
}

/// Memo + static analysis of the program (variable dependence), reusable across evaluations of one stage.
pub struct Memo {
    entries: Vec<Option<Entry>>,
    var_ver: Vec<u64>,
    var_idx: std::rc::Rc<HashMap<String, usize>>,
    deps: std::rc::Rc<Vec<Vec<usize>>>,
    pub relevant: std::rc::Rc<Vec<bool>>,
}

thread_local! {
    /// Memo::new's dependency sets (per program) and relevance (per program + stage filter): rebuilt for every stage of every room
    static MEMO_SHAPE: std::cell::RefCell<crate::dhash::FxMap<(usize, usize, String), (std::rc::Rc<HashMap<String, usize>>, std::rc::Rc<Vec<Vec<usize>>>, std::rc::Rc<Vec<bool>>)>> = std::cell::RefCell::new(Default::default());
}

impl Memo {
    pub fn new(prog: &Program, filter: Option<&Domain>) -> Memo {
        let key = (prog as *const Program as usize, prog.nodes.len(), format!("{filter:?}"));
        let (var_idx, deps, relevant) = match MEMO_SHAPE.with(|c| c.borrow().get(&key).cloned()) {
            Some(x) => x,
            None => {
                let (v, d, r) = Self::shape(prog, filter);
                let x = (std::rc::Rc::new(v), std::rc::Rc::new(d), std::rc::Rc::new(r));
                MEMO_SHAPE.with(|c| c.borrow_mut().insert(key, x.clone()));
                x
            }
        };
        let n = prog.nodes.len();
        let nv = var_idx.len();
        Memo { entries: (0..n).map(|_| None).collect(), var_ver: vec![0; nv], var_idx, deps, relevant }
    }

    fn shape(prog: &Program, filter: Option<&Domain>) -> (HashMap<String, usize>, Vec<Vec<usize>>, Vec<bool>) {
        let n = prog.nodes.len();
        let mut var_idx = HashMap::default();
        for node in &prog.nodes {
            if let Node::Item { name, .. } | Node::ForAll { var: name, .. } | Node::SumOver { var: name, .. } | Node::MeanOver { var: name, .. } = node {
                let l = var_idx.len();
                var_idx.entry(name.clone()).or_insert(l);
            }
        }
        // dependence sets by DAG DP (nodes are topologically ordered children-first? not guaranteed: do DFS)
        let mut deps: Vec<Option<Vec<usize>>> = vec![None; n];
        fn go(prog: &Program, id: usize, var_idx: &HashMap<String, usize>, deps: &mut Vec<Option<Vec<usize>>>) -> Vec<usize> {
            if let Some(d) = &deps[id] {
                return d.clone();
            }
            let mut d: Vec<usize> = vec![];
            match &prog.nodes[id] {
                Node::Item { name, .. } => d.push(var_idx[name]),
                n => {
                    for c in prog.children(id) {
                        for x in go(prog, c, var_idx, deps) {
                            if !d.contains(&x) {
                                d.push(x);
                            }
                        }
                    }
                    if let Node::ForAll { var, .. } | Node::SumOver { var, .. } | Node::MeanOver { var, .. } = n {
                        let v = var_idx.get(var).copied();
                        // the variable is bound here; but its member_of dependence stays
                        d.retain(|x| Some(*x) != v);
                    }
                }
            }
            deps[id] = Some(d.clone());
            d
        }
        for id in 0..n {
            go(prog, id, &var_idx, &mut deps);
        }
        let deps: Vec<Vec<usize>> = deps.into_iter().map(|d| d.unwrap()).collect();
        // relevance
        let mut relevant = vec![true; n];
        if let Some(f) = filter {
            let mut cache: Vec<Option<bool>> = vec![None; n];
            fn rel(prog: &Program, id: usize, f: &Domain, cache: &mut Vec<Option<bool>>) -> bool {
                if let Some(r) = cache[id] {
                    return r;
                }
                let r = if prog.is_object_set(id) {
                    let d = prog.constraint_domain(id, true);
                    assert!(d.finalized(false, true), "relevant: unfinalized");
                    d.intersects(f, false, true)
                } else {
                    prog.children(id).into_iter().any(|c| rel(prog, c, f, cache))
                };
                cache[id] = Some(r);
                r
            }
            for id in 0..n {
                relevant[id] = rel(prog, id, f, &mut cache);
            }
        }
        (var_idx, deps, relevant)
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = None;
        }
    }

    /// evict_memo_for_obj
    pub fn evict_for_obj(&mut self, prog: &Program, obj_tags: &TagSet) {
        // the evicted node set depends only on the program and the object's tags
        thread_local! { static EVICT: std::cell::RefCell<crate::dhash::FxMap<(usize, usize, TagSet), std::rc::Rc<Vec<usize>>>> = std::cell::RefCell::new(Default::default()); }
        let key = (prog as *const Program as usize, prog.nodes.len(), obj_tags.clone());
        if let Some(ids) = EVICT.with(|c| c.borrow().get(&key).cloned()) {
            for &id in ids.iter() {
                self.entries[id] = None;
            }
            return;
        }
        let ids = std::rc::Rc::new(self.evict_ids(prog, obj_tags));
        for &id in ids.iter() {
            self.entries[id] = None;
        }
        EVICT.with(|c| c.borrow_mut().insert(key, ids));
    }

    fn evict_ids(&self, prog: &Program, obj_tags: &TagSet) -> Vec<usize> {
        let n = prog.nodes.len();
        let mut res: Vec<Option<bool>> = vec![None; n];
        fn go(prog: &Program, id: usize, tags: &TagSet, res: &mut Vec<Option<bool>>) -> bool {
            if let Some(r) = res[id] {
                return r;
            }
            let mut r = false;
            for c in prog.children(id) {
                r |= go(prog, c, tags, res);
            }
            match &prog.nodes[id] {
                Node::Tagged { tags: t, .. } => {
                    // membership-based (satisfies) rather than the reference's stricter `implies`: avoids stale sets
                    if !satisfies(tags, t) {
                        r = false;
                    }
                }
                Node::Scene => r = true,
                _ => {}
            }
            res[id] = Some(r);
            r
        }
        (0..n).filter(|&id| go(prog, id, obj_tags, &mut res)).collect()
    }
}

pub struct EvalResult {
    pub loss_vals: Vec<(String, f64)>,
    pub violations: Vec<(String, f64)>,
}

impl EvalResult {
    pub fn loss(&self) -> f64 {
        self.loss_vals.iter().map(|x| x.1).sum()
    }
    pub fn viol_count(&self) -> f64 {
        self.violations.iter().map(|x| x.1).sum()
    }
}

pub struct Evaluator<'a> {
    pub prog: &'a Program,
    pub state: &'a State,
    pub memo: &'a mut Memo,
    env: HashMap<usize, Rc<Vec<String>>>,
}

impl<'a> Evaluator<'a> {
    pub fn new(prog: &'a Program, state: &'a State, memo: &'a mut Memo) -> Self {
        Evaluator { prog, state, memo, env: HashMap::default() }
    }

    pub fn evaluate_problem(&mut self, enable_loss: bool, enable_violated: bool) -> EvalResult {
        let mut loss_vals = vec![];
        if enable_loss {
            for (name, id) in &self.prog.score_terms {
                let v = self.eval(*id);
                loss_vals.push((name.clone(), v.num()));
            }
        }
        let mut violations = vec![];
        if enable_violated {
            for (name, id) in &self.prog.constraints {
                violations.push((name.clone(), self.viol_count(*id, true)));
            }
        }
        EvalResult { loss_vals, violations }
    }

    fn scene_set(&self) -> Rc<Vec<String>> {
        Rc::new(self.state.objs.iter().filter(|(_, o)| o.active).map(|(k, _)| k.clone()).collect())
    }

    pub fn eval(&mut self, id: NodeId) -> Val {
        // cache lookup (dependence-aware)
        let dv: Vec<u64> = self.memo.deps[id].iter().map(|&v| self.memo.var_ver[v]).collect();
        if let Some(e) = &self.memo.entries[id] {
            if e.vers == dv {
                return e.val.clone();
            }
        }
        let val = self.compute(id);
        if crate::rng::TRACE_ON.with(|t| t.get()) {
            let kind = match &self.prog.nodes[id] {
                Node::Distance { .. } => "distance",
                Node::Hinge { .. } => "hinge",
                Node::AccessibilityCost { .. } => "accessibility_cost",
                Node::CenterStableSurfaceDist { .. } => "center_stable_surface_dist",
                Node::AngleAlignmentCost { .. } => "angle_alignment_cost",
                Node::MinDistanceInternal { .. } => "min_distance_internal",
                _ => "",
            };
            if !kind.is_empty() {
                if let Val::Num(v) = &val {
                    if kind != "distance" && kind != "accessibility_cost" { eprintln!("RT   NODE {kind} {v:.5}"); }
                }
            }
        }
        let dv: Vec<u64> = self.memo.deps[id].iter().map(|&v| self.memo.var_ver[v]).collect();
        self.memo.entries[id] = Some(Entry { val: val.clone(), vers: dv });
        val
    }

    fn names_of(&mut self, id: NodeId) -> Rc<Vec<String>> {
        self.eval(id).set().clone()
    }

    fn compute(&mut self, id: NodeId) -> Val {
        let prog = self.prog;
        match &prog.nodes[id] {
            Node::Scene => Val::Set(self.scene_set()),
            Node::Tagged { objs, tags } => {
                let s = self.names_of(*objs);
                Val::Set(Rc::new(s.iter().filter(|k| satisfies(&self.state.objs.get(*k).unwrap_or_else(|| panic!("stale name {k} in tagged set (node {id})")).tags, tags)).cloned().collect()))
            }
            Node::RelatedTo { child, parent, relation } => {
                let children = self.names_of(*child);
                let parents = self.names_of(*parent);
                Val::Set(Rc::new(
                    children
                        .iter()
                        .filter(|o| self.state.objs.get(*o).unwrap_or_else(|| panic!("stale name {o} in related_to children (node {id}); state has {}", self.state.objs.len())).relations.iter().any(|rs| rs.relation.implies(relation) && parents.contains(&rs.target)))
                        .cloned()
                        .collect(),
                ))
            }
            Node::Item { name, .. } => {
                let vi = self.memo.var_idx[name];
                Val::Set(self.env.get(&vi).unwrap_or_else(|| panic!("unbound var {name}")).clone())
            }
            Node::ForAll { objs, var, pred } | Node::SumOver { objs, var, pred } | Node::MeanOver { objs, var, pred } => {
                let set = self.names_of(*objs);
                let vi = self.memo.var_idx[var];
                let mut results = Vec::with_capacity(set.len());
                let saved = self.env.get(&vi).cloned();
                for o in set.iter() {
                    self.memo.var_ver[vi] += 1;
                    self.env.insert(vi, Rc::new(vec![o.clone()]));
                    results.push(self.eval(*pred));
                }
                match saved {
                    Some(s) => {
                        self.env.insert(vi, s);
                    }
                    None => {
                        self.env.remove(&vi);
                    }
                }
                self.memo.var_ver[vi] += 1;
                match &prog.nodes[id] {
                    Node::ForAll { .. } => Val::Bool(results.iter().all(|v| v.truthy())),
                    Node::SumOver { .. } => Val::Num(results.iter().map(|v| v.num()).sum()),
                    _ => Val::Num(if results.is_empty() { 0.0 } else { results.iter().map(|v| v.num()).sum::<f64>() / results.len() as f64 }),
                }
            }
            Node::Count { objs } => Val::Num(self.names_of(*objs).len() as f64),
            Node::InRange { val, low, high } => {
                let x = self.eval(*val).num();
                Val::Bool(x <= *high && x >= *low)
            }
            Node::Const(v) => Val::Num(*v),
            Node::ConstBool(b) => Val::Bool(*b),
            Node::ScalarOp { op, operands } => {
                let v: Vec<f64> = operands.iter().map(|&o| self.eval(o).num()).collect();
                let r = match op {
                    Op::Add | Op::Mul | Op::Max | Op::Min | Op::Sub | Op::TrueDiv | Op::Pow | Op::SafeDiv | Op::FloorDiv => {
                        // functools.reduce over operands
                        let mut acc = v[0];
                        for x in &v[1..] {
                            acc = apply_scalar(*op, &[acc, *x]);
                        }
                        if v.len() == 1 && matches!(op, Op::Max | Op::Min | Op::Add | Op::Mul) { v[0] } else { acc }
                    }
                    _ => apply_scalar(*op, &v),
                };
                Val::Num(r)
            }
            Node::BoolOp { op, operands } => {
                let v: Vec<Val> = operands.iter().map(|&o| self.eval(o)).collect();
                match op {
                    Op::Not => Val::Bool(!v[0].truthy()),
                    Op::And => Val::Bool(v.iter().all(|x| x.truthy())),
                    Op::Or => Val::Bool(v.iter().any(|x| x.truthy())),
                    _ => {
                        let (a, b) = (v[0].num(), v[1].num());
                        Val::Bool(match op {
                            Op::Eq => a == b,
                            Op::Ne => a != b,
                            Op::Ge => a >= b,
                            Op::Le => a <= b,
                            Op::Gt => a > b,
                            Op::Lt => a < b,
                            _ => panic!("boolop {op:?}"),
                        })
                    }
                }
            }
            Node::Hinge { val, low, high } => {
                let x = self.eval(*val).num();
                Val::Num(if x < *low { *low - x } else if x > *high { x - *high } else { 0.0 })
            }
            Node::Volume { objs, dims, single } => {
                let s = self.names_of(*objs);
                let mut res = 0.0;
                for o in s.iter() {
                    let mut d = self.state.dims(o).to_vec();
                    d.sort_by(|a, b| b.partial_cmp(a).unwrap());
                    let chosen: Vec<f64> = match single {
                        Some(n) => d[..*n].to_vec(),
                        None => dims.iter().map(|&i| d[i]).collect(),
                    };
                    res += chosen.iter().product::<f64>() + 1e-10;
                }
                Val::Num(res)
            }
            Node::Distance { objs, others, others_tags } => {
                let a = self.names_of(*objs);
                let b = self.names_of(*others);
                if a.is_empty() || b.is_empty() {
                    return Val::Num(0.0);
                }
                let d = col::min_dist(self.state, &a, Some(&b), others_tags);
                if crate::rng::TRACE_ON.with(|t| t.get()) { let mut aa: Vec<_> = a.iter().cloned().collect(); aa.sort(); let mut bb: Vec<_> = b.iter().cloned().collect(); bb.sort(); eprintln!("RT   NODE distance {:.5}  A={} B={}", if d < 0.0 { 0.0 } else { d }, aa.join(",").chars().take(80).collect::<String>(), bb.join(",").chars().take(60).collect::<String>()); }
                Val::Num(if d < 0.0 { 0.0 } else { d })
            }
            Node::MinDistanceInternal { objs } => {
                let a = self.names_of(*objs);
                if a.len() <= 1 {
                    return Val::Num(0.0);
                }
                Val::Num(col::min_dist(self.state, &a, None, &TagSet::new()))
            }
            Node::AccessibilityCost { objs, others, normal, dist } => {
                let a = self.names_of(*objs);
                let b = self.names_of(*others);
                if a.is_empty() {
                    return Val::Num(0.0);
                }
                let v = self.accessibility(&a, &b, *normal, *dist);
                if crate::rng::TRACE_ON.with(|t| t.get()) { let mut aa: Vec<_> = a.iter().cloned().collect(); aa.sort(); let mut bb: Vec<_> = b.iter().cloned().collect(); bb.sort(); eprintln!("RT   NODE accessibility_cost {:.5}  A={} B={}", v, aa.join(",").chars().take(80).collect::<String>(), bb.join(",").chars().take(60).collect::<String>()); }
                Val::Num(v)
            }
            Node::CenterStableSurfaceDist { objs } => {
                let a = self.names_of(*objs);
                Val::Num(self.center_stable(&a))
            }
            Node::AngleAlignmentCost { objs, others, others_tags } => {
                let a = self.names_of(*objs);
                let b = self.names_of(*others);
                if a.is_empty() || b.is_empty() {
                    return Val::Num(0.0);
                }
                Val::Num(self.angle_alignment(&a, &b, others_tags))
            }
            Node::FocusScore { objs, others } => {
                let a = self.names_of(*objs);
                let b = self.names_of(*others);
                if a.is_empty() || b.is_empty() {
                    return Val::Num(0.0);
                }
                // python hands `b` over as a list built from a set of strings: its first element decides
                let ord = crate::pyhash::str_set_order(&b);
                let first = b[ord[0]].clone();
                Val::Num(self.focus_score(&a, &first))
            }
        }
    }

    // ---- violation counting (evaluate.viol_count) ----
    pub fn viol_count(&mut self, id: NodeId, use_filter: bool) -> f64 {
        let prog = self.prog;
        let relevant = |m: &Memo, i: NodeId| !use_filter || m.relevant[i];
        match &prog.nodes[id] {
            Node::BoolOp { op: Op::And, operands } => {
                let mut s = 0.0;
                for &o in operands {
                    s += self.viol_count(o, use_filter);
                }
                s
            }
            Node::InRange { val, low, high } => {
                let v = self.eval(*val).num();
                let mut res = if v < *low { *low - v } else if v > *high { v - *high } else { 0.0 };
                if !relevant(self.memo, *val) {
                    res = 0.0;
                }
                res
            }
            Node::BoolOp { op: Op::Eq, operands } => {
                let (l, r) = (operands[0], operands[1]);
                let res = (self.eval(l).num() - self.eval(r).num()).abs();
                if !relevant(self.memo, l) && !relevant(self.memo, r) {
                    0.0
                } else {
                    res
                }
            }
            Node::ForAll { objs, var, pred } => {
                let set = self.names_of(*objs);
                let vi = self.memo.var_idx[var];
                let saved = self.env.get(&vi).cloned();
                let mut viol = 0.0;
                for o in set.iter() {
                    self.memo.var_ver[vi] += 1;
                    self.env.insert(vi, Rc::new(vec![o.clone()]));
                    viol += self.viol_count(*pred, use_filter);
                }
                match saved {
                    Some(s) => {
                        self.env.insert(vi, s);
                    }
                    None => {
                        self.env.remove(&vi);
                    }
                }
                self.memo.var_ver[vi] += 1;
                viol
            }
            Node::BoolOp { op: op @ (Op::Ge | Op::Le | Op::Gt | Op::Lt), operands } => {
                let (l, r) = (operands[0], operands[1]);
                if relevant(self.memo, l) || relevant(self.memo, r) {
                    let (a, b) = (self.eval(l), self.eval(r));
                    let both_int = |v: &Val| match v {
                        Val::Num(x) => x.fract() == 0.0 && is_int_node(prog, if std::ptr::eq(v, &a) { l } else { r }),
                        _ => false,
                    };
                    let _ = both_int;
                    // integer comparison only when both sides are python ints (counts / int constants)
                    let ints = is_int_expr(prog, l) && is_int_expr(prog, r);
                    let (x, y) = (a.num(), b.num());
                    if ints {
                        let err = match op {
                            Op::Ge => y - x,
                            Op::Le => x - y,
                            Op::Gt => y - x + 1.0,
                            _ => x - y + 1.0,
                        };
                        err.max(0.0)
                    } else {
                        let sat = match op {
                            Op::Ge => x >= y,
                            Op::Le => x <= y,
                            Op::Gt => x > y,
                            _ => x < y,
                        };
                        if sat { 0.0 } else { 1.0 }
                    }
                } else {
                    0.0
                }
            }
            Node::ConstBool(b) => if *b { 0.0 } else { 1.0 },
            Node::BoolOp { op: Op::Or, operands } => {
                let a = self.viol_count(operands[0], false);
                let b = self.viol_count(operands[1], false);
                a.min(b)
            }
            Node::BoolOp { op: Op::Not, operands } => {
                let v = self.eval(operands[0]);
                if matches!(v, Val::Bool(true)) { 1.0 } else { 0.0 }
            }
            _ => self.eval(id).num(),
        }
    }

    // ---- geometry nodes ----

    fn front_axis_xy(&self, name: &str) -> [f64; 2] {
        let planes = self.state.planes(name, &TagSet::from([Tag::sub("Front")]));
        let (n, _) = self.state.plane_nd(name, planes[0]);
        [n[0], n[1]]
    }

    fn accessibility(&mut self, a: &[String], b: &[String], normal: V3, dist: f64) -> f64 {
        if b.is_empty() {
            return 0.0;
        }
        let axis = (0..3).max_by(|&i, &j| normal[i].abs().partial_cmp(&normal[j].abs()).unwrap()).unwrap();
        let sign = normal[axis].signum();
        let mut best = 0.0f64;
        for name in a {
            let o = &self.state.objs[name];
            let (lo, hi) = (o.geom.lo, o.geom.hi);
            let sc = o.pose.o.scale;
            // bpy_obj.dimensions = local bbox dims * scale
            let mut ext = [(hi[0] - lo[0]) * sc[0] as f64, (hi[1] - lo[1]) * sc[1] as f64, (hi[2] - lo[2]) * sc[2] as f64];
            ext[axis] = dist;
            let center = [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.];
            let extent = if sign < 0.0 { lo[axis] } else { hi[axis] };
            let mut off = [0.0; 3];
            off[axis] = sign * (dist / 2.0 + extent - center[axis]);
            let total = [center[0] + off[0], center[1] + off[1], center[2] + off[2]];
            let h = [ext[0] / 2.0, ext[1] / 2.0, ext[2] / 2.0];
            let bverts: Vec<V3> = (0..8usize).map(|i| [if i & 4 != 0 { h[0] } else { -h[0] }, if i & 2 != 0 { h[1] } else { -h[1] }, if i & 1 != 0 { h[2] } else { -h[2] }]).collect();
            let mut fpose = o.pose;
            fpose.t = o.pose.apply(total);
            // trimesh CollisionManager.add_object casts the transform to float32
            fpose.t = [fpose.t[0] as f32 as f64, fpose.t[1] as f32 as f64, fpose.t[2] as f32 as f64];
            if crate::rng::TRACE_ON.with(|t| t.get()) {
                let r8 = |x: f64| (x * 1e8).round() / 1e8;
                eprintln!("RT   FREEBOX {name} extents {:?} T {:?}", ext.map(|x| x), [[r8(fpose.r[0][0]), r8(fpose.r[0][1]), r8(fpose.r[0][2]), r8(fpose.t[0])], [r8(fpose.r[1][0]), r8(fpose.r[1][1]), r8(fpose.r[1][2]), r8(fpose.t[1])], [r8(fpose.r[2][0]), r8(fpose.r[2][1]), r8(fpose.r[2][2]), r8(fpose.t[2])], [0.0, 0.0, 0.0, 1.0]]);
            }
            for bn in b {
                let ob = &self.state.objs[bn];
                let d = col::free_box_depth(&bverts, &fpose, ob);
                if ig_core::env_str!("ACCPAIR").map_or(false, |p| p == "ALL" || p == format!("{name}|{bn}")) && crate::rng::TRACE_ON.with(|t| t.get()) && std::env::var("ACCAT").map_or(true, |a| { let c = crate::rng::CUR.with(|c| c.get()); a == format!("{}:{}", c.0, c.1) }) {
                    let tris: Vec<[[f64; 3]; 3]> = ob.geom.tris.iter().map(|t| t.v).collect();
                    eprintln!("RT   ACCJSON {}", serde_json::json!({"a": name, "b": bn, "fverts": bverts, "fr": fpose.r, "ft": fpose.t, "bconvex": ob.geom.convex.as_ref().map(|v| (**v).clone()), "btris": tris, "br": ob.pose.r, "bt": ob.pose.t, "depth": d, "objA": obj_desc(o), "objB": obj_desc(ob), "normal": normal, "dist": dist}));
                }
                if let Some(d) = d {
                    if ig_core::env_on!("ACCDBG") && crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   ACC {name} vs {bn} depth {d:.5} ext={ext:?} axis={axis} sign={sign}"); }
                    best = best.max(d);
                }
            }
        }
        best
    }

    fn center_stable(&mut self, a: &[String]) -> f64 {
        let mut score = 0.0;
        for name in a {
            let o = &self.state.objs[name];
            let w = self.state.world(name);
            for rs in &o.relations {
                let Some(g) = rs.relation.geo() else { continue };
                let parent_planes = self.state.planes(&rs.target, &g.parent);
                let obj_planes = self.state.planes(name, &g.child);
                if rs.parent_plane >= parent_planes.len() || rs.child_plane >= obj_planes.len() {
                    return 0.0;
                }
                let po = &self.state.objs[&rs.target];
                let (pos, neg) = tag_masks(&g.parent);
                let idxs = plane_mask(&po.geom, &po.pose, pos, neg, parent_planes[rs.parent_plane]);
                let pw = self.state.world(&rs.target);
                let mut c = [0.0; 3];
                let mut tot = 0.0;
                for &i in &idxs {
                    let t = &pw.tris[i];
                    for k in 0..3 {
                        c[k] += pw.areas[i] * (t[0][k] + t[1][k] + t[2][k]) / 3.0;
                    }
                    tot += pw.areas[i];
                }
                if tot > 0.0 {
                    for k in 0..3 {
                        c[k] /= tot;
                    }
                }
                let d = sub(w.centroid, c);
                score += dot(d, d).sqrt();
            }
        }
        score
    }

    fn xy_outline(&self, name: &str) -> Option<shp::G> {
        let w = self.state.world(name);
        projected(&w.tris, [0., 0., 1.], [0., 0., 0.])
    }

    fn angle_alignment(&mut self, a: &[String], b: &[String], b_tags: &TagSet) -> f64 {
        let (pos, neg) = tag_masks(b_tags);
        // b edges (name, p, q)
        let mut b_edges: Vec<(String, [f64; 2], [f64; 2])> = vec![];
        for bn in b {
            let o = &self.state.objs[bn];
            let w = self.state.world(bn);
            let sel: Vec<[V3; 3]> = o.geom.tris.iter().zip(&w.tris).filter(|(t, _)| t.tags & pos == pos && t.tags & neg == 0).map(|(_, w)| *w).collect();
            if sel.is_empty() {
                continue;
            }
            let poly = projected(&sel, [0., 0., 1.], [0., 0., 0.]);
            match poly {
                Some(p) if !shp::is_empty(&p) => {
                    for part in shp::parts(&p) {
                        let ext = shp::exterior(&part);
                        for wn in ext.windows(2) {
                            if isclose2(wn[0], wn[1]) {
                                continue;
                            }
                            b_edges.push((bn.clone(), wn[0], wn[1]));
                        }
                    }
                }
                _ => {
                    for t in &sel {
                        for k in 0..3 {
                            let (p, q) = (t[k], t[(k + 1) % 3]);
                            if isclose2([p[0], p[1]], [q[0], q[1]]) {
                                continue;
                            }
                            b_edges.push((bn.clone(), [p[0], p[1]], [q[0], q[1]]));
                        }
                    }
                }
            }
        }
        let mut score = 0.0;
        for an in a {
            let axis = self.front_axis_xy(an);
            let centroid = match self.xy_outline(an) {
                Some(p) => shp::centroid(&p),
                None => {
                    let w = self.state.world(an);
                    [w.centroid[0], w.centroid[1]]
                }
            };
            let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
            for (bn, p, q) in &b_edges {
                // tagged path: copies of b are never named like a
                let _ = bn;
                let d = seg_dist(centroid, *p, *q);
                if best.map_or(true, |(bd, _, _)| d < bd) {
                    best = Some((d, *p, *q));
                }
            }
            let Some((_, p, q)) = best else { continue };
            if crate::rng::TRACE_ON.with(|t| t.get()) && ig_core::env_on!("ANGDBG") { eprintln!("RT   ANG {an} axis={axis:?} centroid={centroid:?} edge={p:?}->{q:?} nedges={}", b_edges.len()); }
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let n1 = [dy, -dx];
            let l = (n1[0] * n1[0] + n1[1] * n1[1]).sqrt();
            let n1 = [n1[0] / l, n1[1] / l];
            let d1 = axis[0] * n1[0] + axis[1] * n1[1];
            let s1 = -d1 / 2.0 + 0.5;
            let s2 = d1 / 2.0 + 0.5;
            score += s1.min(s2);
        }
        score
    }

    fn focus_score(&mut self, a: &[String], b: &str) -> f64 {
        let Some(bp) = self.xy_outline(b) else { return 0.0 };
        let bc = shp::centroid(&bp);
        let mut score = 0.0;
        for an in a {
            let Some(ap) = self.xy_outline(an) else { continue };
            let ac = shp::centroid(&ap);
            let axis = self.front_axis_xy(an);
            let fv = [bc[0] - ac[0], bc[1] - ac[1]];
            let l = (fv[0] * fv[0] + fv[1] * fv[1]).sqrt();
            let fv = [fv[0] / l, fv[1] / l];
            score += -(axis[0] * fv[0] + axis[1] * fv[1]) / 2.0 + 0.5;
        }
        score
    }
}

/// np.isclose(start, end).all() for 2-vectors (atol 1e-8, rtol 1e-5 relative to `end`)
fn isclose2(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() <= 1e-8 + 1e-5 * b[0].abs() && (a[1] - b[1]).abs() <= 1e-8 + 1e-5 * b[1].abs()
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + t * ab[0], a[1] + t * ab[1]];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

fn is_int_node(_p: &Program, _id: NodeId) -> bool {
    false
}

/// python-int-valued expression (counts, int constants and their sums): decides integer vs boolean violation counting
fn is_int_expr(prog: &Program, id: NodeId) -> bool {
    match &prog.nodes[id] {
        Node::Count { .. } => true,
        Node::Const(v) => v.fract() == 0.0,
        Node::ScalarOp { op: Op::Add | Op::Sub | Op::Mul, operands } => operands.iter().all(|&o| is_int_expr(prog, o)),
        _ => false,
    }
}

#[allow(dead_code)]
fn _unused(_: V3) -> V3 {
    cross([0.; 3], [0.; 3])
}

/// full description of an object (local mesh in blender vertex order + blender transform) for python-side replay
pub fn obj_desc(o: &ObjState) -> serde_json::Value {
    let mut verts: Vec<[f64; 3]> = match &o.geom.convex {
        Some(c) => (**c).clone(),
        None => vec![],
    };
    let mut faces = vec![];
    for t in &o.geom.tris {
        let mut f = [0usize; 3];
        for k in 0..3 {
            f[k] = match verts.iter().position(|q| *q == t.v[k]) {
                Some(i) => i,
                None => {
                    verts.push(t.v[k]);
                    verts.len() - 1
                }
            };
        }
        faces.push(f);
    }
    serde_json::json!({"verts": verts, "faces": faces, "loc": o.pose.o.loc, "euler": o.pose.o.eul, "scale": o.pose.o.scale})
}
