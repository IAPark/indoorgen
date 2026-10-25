//! The constraint program: node DAG loaded from `data/home_program.json` (dumped from the python source),
//! plus constraint_domain / constraint_bounds (reasoning/*.py).
use super::domain::*;
use super::relations::*;
use super::tags::*;
use serde_json::Value;
use crate::dhash::HashMap;

pub type NodeId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    TrueDiv,
    Pow,
    FloorDiv,
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
    And,
    Or,
    Not,
    Abs,
    Max,
    Min,
    Log,
    Clip,
    SafeDiv,
}

#[derive(Clone, Debug)]
pub enum Node {
    Scene,
    Tagged { objs: NodeId, tags: TagSet },
    RelatedTo { child: NodeId, parent: NodeId, relation: Relation },
    Item { name: String, member_of: NodeId },
    ForAll { objs: NodeId, var: String, pred: NodeId },
    SumOver { objs: NodeId, var: String, pred: NodeId },
    MeanOver { objs: NodeId, var: String, pred: NodeId },
    Count { objs: NodeId },
    InRange { val: NodeId, low: f64, high: f64 },
    Const(f64),
    ConstBool(bool),
    ScalarOp { op: Op, operands: Vec<NodeId> },
    BoolOp { op: Op, operands: Vec<NodeId> },
    Hinge { val: NodeId, low: f64, high: f64 },
    Volume { objs: NodeId, dims: Vec<usize>, single: Option<usize> },
    Distance { objs: NodeId, others: NodeId, others_tags: TagSet },
    AccessibilityCost { objs: NodeId, others: NodeId, normal: [f64; 3], dist: f64 },
    MinDistanceInternal { objs: NodeId },
    CenterStableSurfaceDist { objs: NodeId },
    AngleAlignmentCost { objs: NodeId, others: NodeId, others_tags: TagSet },
    FocusScore { objs: NodeId, others: NodeId },
}

#[derive(Clone, Debug)]
pub struct Bound {
    pub domain: Domain,
    pub low: Option<f64>,
    pub high: Option<f64>,
}

pub struct Program {
    pub nodes: Vec<Node>,
    pub constraints: Vec<(String, NodeId)>,
    pub score_terms: Vec<(String, NodeId)>,
}

pub type Params = HashMap<String, f64>;

fn parse_tag(v: &Value) -> Tag {
    let o = v.as_object().unwrap();
    if let Some(s) = o.get("sem") {
        Tag::sem(s.as_str().unwrap())
    } else if let Some(s) = o.get("sub") {
        Tag::sub(s.as_str().unwrap())
    } else if let Some(s) = o.get("gen") {
        Tag::gen(s.as_str().unwrap())
    } else if let Some(s) = o.get("var") {
        Tag::var(s.as_str().unwrap())
    } else if let Some(s) = o.get("obj") {
        Tag::obj(s.as_str().unwrap())
    } else if let Some(n) = o.get("neg") {
        parse_tag(n).negated()
    } else {
        panic!("bad tag {v}")
    }
}

pub fn parse_tagset(v: &Value) -> TagSet {
    TagSet::from(v.as_array().unwrap().iter().map(parse_tag))
}

fn num(v: &Value, params: &Params) -> f64 {
    if let Some(p) = v.get("param") {
        return params[p.as_str().unwrap()];
    }
    match v {
        Value::Bool(b) => *b as i32 as f64,
        Value::Number(n) => n.as_f64().unwrap(),
        _ => panic!("not a number {v}"),
    }
}

pub fn parse_relation(v: &Value, params: &Params) -> Relation {
    let o = v.as_object().unwrap();
    match o["_"].as_str().unwrap() {
        "AnyRelation" => Relation::Any,
        "NegatedRelation" => Relation::Negated(Box::new(parse_relation(&o["rel"], params))),
        "RoomNeighbour" => Relation::RoomNeighbour(o["connector_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()),
        "CutFrom" => Relation::CutFrom,
        "SharedEdge" => Relation::SharedEdge,
        "Traverse" => Relation::Traverse,
        k @ ("Touching" | "SupportedBy" | "CoPlanar" | "StableAgainst") => {
            let kind = match k {
                "Touching" => GeoKind::Touching,
                "SupportedBy" => GeoKind::SupportedBy,
                "CoPlanar" => GeoKind::CoPlanar,
                _ => GeoKind::StableAgainst,
            };
            Relation::Geometry(Geo {
                kind,
                child: parse_tagset(&o["child_tags"]),
                parent: parse_tagset(&o["parent_tags"]),
                margin: o.get("margin").map_or(0.0, |m| num(m, params)),
                check_z: o.get("check_z").map_or(true, |b| b.as_bool().unwrap()),
                rev_normal: o.get("rev_normal").map_or(false, |b| b.as_bool().unwrap()),
            })
        }
        k => panic!("unknown relation {k}"),
    }
}

pub fn parse_domain(v: &Value, params: &Params) -> Domain {
    let o = v.as_object().unwrap();
    Domain {
        tags: parse_tagset(&o["tags"]),
        relations: o["relations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let p = p.as_array().unwrap();
                (parse_relation(&p[0], params), parse_domain(&p[1], params))
            })
            .collect(),
    }
}

fn parse_op(s: &str) -> Op {
    match s {
        "add" => Op::Add,
        "sub" => Op::Sub,
        "mul" => Op::Mul,
        "truediv" => Op::TrueDiv,
        "pow" => Op::Pow,
        "floordiv" => Op::FloorDiv,
        "eq" => Op::Eq,
        "ne" => Op::Ne,
        "ge" => Op::Ge,
        "le" => Op::Le,
        "gt" => Op::Gt,
        "lt" => Op::Lt,
        "and" => Op::And,
        "or" => Op::Or,
        "not" => Op::Not,
        "abs" => Op::Abs,
        "max" => Op::Max,
        "min" => Op::Min,
        "log" => Op::Log,
        "clip" => Op::Clip,
        "safediv" => Op::SafeDiv,
        _ => panic!("op {s}"),
    }
}

impl Program {
    /// `variant` is the 5-char bitstring of (has_tv, has_aquarium_tank, has_birthday_balloons, has_cocktail_tables, has_kitchen_barstools)
    pub fn load(data: &Value, variant: &str, params: &Params) -> Program {
        let v = &data["variants"][variant];
        let rid = |x: &Value| x["ref"].as_u64().unwrap() as usize;
        let nodes_json = v["nodes"].as_array().unwrap();
        let mut nodes = vec![];
        for n in nodes_json {
            let o = n.as_object().unwrap();
            let cls = o["_"].as_str().unwrap();
            let node = match cls {
                "scene" => Node::Scene,
                "tagged" => Node::Tagged { objs: rid(&o["objs"]), tags: parse_tagset(&o["tags"]["tags"]) },
                "related_to" => Node::RelatedTo {
                    child: rid(&o["child"]),
                    parent: rid(&o["parent"]),
                    relation: parse_relation(&o["relation"]["rel"], params),
                },
                "item" => Node::Item { name: o["name"].as_str().unwrap().into(), member_of: rid(&o["member_of"]) },
                "ForAll" => Node::ForAll { objs: rid(&o["objs"]), var: o["var"].as_str().unwrap().into(), pred: rid(&o["pred"]) },
                "SumOver" => Node::SumOver { objs: rid(&o["objs"]), var: o["var"].as_str().unwrap().into(), pred: rid(&o["pred"]) },
                "MeanOver" => Node::MeanOver { objs: rid(&o["objs"]), var: o["var"].as_str().unwrap().into(), pred: rid(&o["pred"]) },
                "count" => Node::Count { objs: rid(&o["objs"]) },
                "in_range" => Node::InRange { val: rid(&o["val"]), low: num(&o["low"], params), high: num(&o["high"], params) },
                "constant" => match &o["value"] {
                    Value::Bool(b) => Node::ConstBool(*b),
                    x => Node::Const(num(x, params)),
                },
                "ScalarOperatorExpression" | "BoolOperatorExpression" => {
                    let op = parse_op(o["func"]["op"].as_str().unwrap());
                    let operands: Vec<NodeId> = o["operands"]["list"].as_array().unwrap().iter().map(rid).collect();
                    if cls == "ScalarOperatorExpression" {
                        Node::ScalarOp { op, operands }
                    } else {
                        Node::BoolOp { op, operands }
                    }
                }
                "hinge" => Node::Hinge { val: rid(&o["val"]), low: num(&o["low"], params), high: num(&o["high"], params) },
                "volume" => {
                    let (dims, single) = match &o["dims"] {
                        Value::Number(n) => (vec![], Some(n.as_u64().unwrap() as usize)),
                        d => (d["list"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect(), None),
                    };
                    Node::Volume { objs: rid(&o["objs"]), dims, single }
                }
                "distance" => Node::Distance { objs: rid(&o["objs"]), others: rid(&o["others"]), others_tags: parse_tagset(&o["others_tags"]["tags"]) },
                "accessibility_cost" => {
                    let nv = o["normal"]["vec"].as_array().unwrap();
                    Node::AccessibilityCost {
                        objs: rid(&o["objs"]),
                        others: rid(&o["others"]),
                        normal: [nv[0].as_f64().unwrap(), nv[1].as_f64().unwrap(), nv[2].as_f64().unwrap()],
                        dist: num(&o["dist"], params),
                    }
                }
                "min_distance_internal" => Node::MinDistanceInternal { objs: rid(&o["objs"]) },
                "center_stable_surface_dist" => Node::CenterStableSurfaceDist { objs: rid(&o["objs"]) },
                "angle_alignment_cost" => Node::AngleAlignmentCost { objs: rid(&o["objs"]), others: rid(&o["others"]), others_tags: parse_tagset(&o["others_tags"]["tags"]) },
                "focus_score" => Node::FocusScore { objs: rid(&o["objs"]), others: rid(&o["others"]) },
                c => panic!("unknown node class {c}"),
            };
            nodes.push(node);
        }
        let side = |m: &Value| -> Vec<(String, NodeId)> { m.as_object().unwrap().iter().map(|(k, x)| (k.clone(), rid(x))).collect() };
        Program { nodes, constraints: side(&v["constraints"]), score_terms: side(&v["score_terms"]) }
    }

    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        use Node::*;
        match &self.nodes[id] {
            Scene | Const(_) | ConstBool(_) => vec![],
            Tagged { objs, .. } | Count { objs } | MinDistanceInternal { objs } | CenterStableSurfaceDist { objs } | Volume { objs, .. } => vec![*objs],
            RelatedTo { child, parent, .. } => vec![*child, *parent],
            Item { member_of, .. } => vec![*member_of],
            ForAll { objs, pred, .. } | SumOver { objs, pred, .. } | MeanOver { objs, pred, .. } => vec![*objs, *pred],
            InRange { val, .. } | Hinge { val, .. } => vec![*val],
            ScalarOp { operands, .. } | BoolOp { operands, .. } => operands.clone(),
            Distance { objs, others, .. } | AccessibilityCost { objs, others, .. } | AngleAlignmentCost { objs, others, .. } | FocusScore { objs, others } => vec![*objs, *others],
        }
    }

    pub fn is_object_set(&self, id: NodeId) -> bool {
        matches!(self.nodes[id], Node::Scene | Node::Tagged { .. } | Node::RelatedTo { .. } | Node::Item { .. })
    }

    pub fn is_constant(&self, id: NodeId) -> bool {
        match &self.nodes[id] {
            Node::Const(_) | Node::ConstBool(_) => true,
            Node::ScalarOp { operands, .. } | Node::BoolOp { operands, .. } => operands.iter().all(|&o| self.is_constant(o)),
            _ => false,
        }
    }

    pub fn eval_const(&self, id: NodeId) -> f64 {
        match &self.nodes[id] {
            Node::Const(v) => *v,
            Node::ConstBool(b) => *b as i32 as f64,
            Node::ScalarOp { op, operands } => {
                let v: Vec<f64> = operands.iter().map(|&o| self.eval_const(o)).collect();
                apply_scalar(*op, &v)
            }
            n => panic!("not const: {n:?}"),
        }
    }

    // ---- reasoning/constraint_domain.py ----
    pub fn constraint_domain(&self, id: NodeId, finalize_variables: bool) -> Domain {
        match &self.nodes[id] {
            Node::Tagged { objs, tags } => {
                let mut d = self.constraint_domain(*objs, finalize_variables);
                d.tags.update(tags);
                assert!(!contradiction(&d.tags), "Contradictory tags {tags:?}");
                d
            }
            Node::RelatedTo { child, parent, relation } => {
                let mut c = self.constraint_domain(*child, finalize_variables);
                let p = self.constraint_domain(*parent, finalize_variables);
                c.add_relation(relation.clone(), p);
                c
            }
            Node::Scene => Domain::default(),
            Node::Item { name, member_of } => {
                if finalize_variables {
                    self.constraint_domain(*member_of, finalize_variables)
                } else {
                    Domain::new(TagSet::from([Tag::var(name)]))
                }
            }
            n => panic!("constraint_domain of {n:?}"),
        }
    }

    // ---- reasoning/constraint_bounding.py ----
    pub fn constraint_bounds(&self) -> Vec<Bound> {
        let mut out = vec![];
        for (_, c) in &self.constraints {
            out.extend(self.bounds_of(*c));
        }
        out
    }

    fn bounds_of(&self, id: NodeId) -> Vec<Bound> {
        match &self.nodes[id] {
            Node::BoolOp { op: Op::And, operands } => operands.iter().flat_map(|&o| self.bounds_of(o)).collect(),
            Node::InRange { val, low, high } => self.map_bound(*val, Some(*low), Some(*high)),
            Node::BoolOp { op, operands } if matches!(op, Op::Eq | Op::Le | Op::Ge | Op::Lt | Op::Gt) && operands.len() == 2 => {
                let (l, r) = (operands[0], operands[1]);
                let (lc, rc) = (self.is_constant(l), self.is_constant(r));
                if !lc && !rc {
                    return vec![];
                }
                let lhs = if lc { Some(self.eval_const(l)) } else { None };
                let rhs = if rc { Some(self.eval_const(r)) } else { None };
                let right_const = rhs.is_some();
                let val = if right_const { rhs.unwrap() } else { lhs.unwrap() };
                let (low, high) = match (op, right_const) {
                    (Op::Eq, _) => (Some(val), Some(val)),
                    (Op::Le, false) | (Op::Ge, true) => (Some(val), None),
                    (Op::Le, true) | (Op::Ge, false) => (None, Some(val)),
                    (Op::Lt, true) | (Op::Gt, false) => (None, Some(val - 1.0)),
                    (Op::Lt, false) | (Op::Gt, true) => (Some(val + 1.0), None),
                    _ => unreachable!(),
                };
                let expr = if lc { r } else { l };
                self.map_bound(expr, low, high)
            }
            Node::ForAll { objs, var, pred } => {
                let o_domain = self.constraint_domain(*objs, false);
                let mut bounds = self.bounds_of(*pred);
                for b in bounds.iter_mut() {
                    b.domain = domain_tag_substitute(&b.domain, Tag::var(var), &o_domain).0;
                }
                bounds
            }
            _ => vec![],
        }
    }

    fn map_bound(&self, id: NodeId, low: Option<f64>, high: Option<f64>) -> Vec<Bound> {
        match &self.nodes[id] {
            Node::ScalarOp { op, operands } if operands.len() == 2 && matches!(op, Op::Add | Op::Mul | Op::TrueDiv | Op::Sub | Op::FloorDiv) => {
                // int_inverse_op: add<->sub, mul->truediv (overwritten), truediv->mul, floordiv->mul
                let inv = |o: &Op| match o {
                    Op::Add => Op::Sub,
                    Op::Sub => Op::Add,
                    Op::Mul => Op::TrueDiv,
                    Op::TrueDiv => Op::Mul,
                    Op::FloorDiv => Op::Mul,
                    _ => unreachable!(),
                };
                let (l, r) = (operands[0], operands[1]);
                let f = inv(op);
                match (self.is_constant(l), self.is_constant(r)) {
                    (true, false) => {
                        let c = self.eval_const(l);
                        self.map_bound(r, low.map(|x| apply_scalar(f, &[c, x])), high.map(|x| apply_scalar(f, &[c, x])))
                    }
                    (false, true) => {
                        let c = self.eval_const(r);
                        self.map_bound(l, low.map(|x| apply_scalar(f, &[x, c])), high.map(|x| apply_scalar(f, &[x, c])))
                    }
                    _ => vec![],
                }
            }
            Node::Count { objs } => self.map_bound(*objs, low, high),
            _ if self.is_object_set(id) => vec![Bound { domain: self.constraint_domain(id, false), low, high }],
            _ => vec![],
        }
    }
}

pub fn apply_scalar(op: Op, v: &[f64]) -> f64 {
    match op {
        Op::Add => v.iter().sum(),
        Op::Sub => v[0] - v[1],
        Op::Mul => v.iter().product(),
        Op::TrueDiv => v[0] / v[1],
        Op::FloorDiv => (v[0] / v[1]).floor(),
        Op::Pow => v[0].powf(v[1]),
        Op::Abs => v[0].abs(),
        Op::Max => v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        Op::Min => v.iter().cloned().fold(f64::INFINITY, f64::min),
        Op::Log => v[0].ln(),
        Op::Clip => v[0].max(v[1]).min(v[2]),
        Op::SafeDiv => {
            if v[1] == 0.0 {
                if v[0] == 0.0 {
                    0.0
                } else {
                    1.0
                }
            } else {
                v[0] / v[1]
            }
        }
        _ => panic!("apply_scalar {op:?}"),
    }
}

// ---- JSON emission of domains for cross-checking against python ----
pub fn tag_json(t: &Tag) -> Value {
    let base = match t.kind() {
        Kind::Sem => serde_json::json!({"sem": t.name()}),
        Kind::Sub => serde_json::json!({"sub": t.name()}),
        Kind::Gen => serde_json::json!({"gen": t.name()}),
        Kind::Var => serde_json::json!({"var": t.name()}),
        Kind::Obj => serde_json::json!({"obj": t.name()}),
    };
    if t.neg {
        serde_json::json!({ "neg": base })
    } else {
        base
    }
}

pub fn rel_json(r: &Relation) -> Value {
    match r {
        Relation::Any => serde_json::json!({"_": "AnyRelation"}),
        Relation::Negated(x) => serde_json::json!({"_": "NegatedRelation", "rel": rel_json(x)}),
        Relation::Geometry(g) => {
            let mut j = serde_json::json!({"_": format!("{:?}", g.kind),
                "child_tags": g.child.0.iter().map(tag_json).collect::<Vec<_>>(),
                "parent_tags": g.parent.0.iter().map(tag_json).collect::<Vec<_>>()});
            if matches!(g.kind, GeoKind::CoPlanar | GeoKind::StableAgainst) {
                j["margin"] = g.margin.into();
                j["rev_normal"] = g.rev_normal.into();
            }
            if g.kind == GeoKind::StableAgainst {
                j["check_z"] = g.check_z.into();
            }
            j
        }
        Relation::RoomNeighbour(c) => serde_json::json!({"_": "RoomNeighbour", "connector_types": c}),
        Relation::CutFrom => serde_json::json!({"_": "CutFrom"}),
        Relation::SharedEdge => serde_json::json!({"_": "SharedEdge"}),
        Relation::Traverse => serde_json::json!({"_": "Traverse"}),
    }
}

pub fn domain_json(d: &Domain) -> Value {
    serde_json::json!({"_": "Domain", "tags": d.tags.0.iter().map(tag_json).collect::<Vec<_>>(),
        "relations": d.relations.iter().map(|(r, x)| serde_json::json!([rel_json(r), domain_json(x)])).collect::<Vec<_>>()})
}
