//! Node group evaluator: demand-driven evaluation with per-node memoization.
use super::field::*;
use super::geom::*;
use super::load::*;
use super::val::*;
use std::cell::RefCell;
use crate::dhash::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub enum Value {
    F(Field),
    G(Rc<Geometry>),
}

impl Value {
    pub fn konst(v: Val) -> Value {
        Value::F(konst(v))
    }
    pub fn empty_geo() -> Value {
        Value::G(Rc::new(Geometry::default()))
    }
    pub fn geo(g: Geometry) -> Value {
        Value::G(Rc::new(g))
    }
}

/// scene object available to Object Info
#[derive(Clone, Default)]
pub struct Object {
    pub geo: Geometry,
    pub matrix: M4,
}

#[derive(Default)]
pub struct Objects {
    pub map: HashMap<String, Object>,
}

pub struct Ev<'a> {
    pub groups: &'a Groups,
    pub g: &'a Group,
    pub objects: &'a Objects,
    pub inputs: Vec<Value>,
    memo: RefCell<HashMap<usize, Rc<Vec<Value>>>>,
    override_geo: RefCell<Option<(usize, String, Rc<Geometry>)>>,
    pub depth: usize,
    /// debugging: remaining path of nested group nodes down to the probed node (see eval_probe)
    pub probe_path: Vec<String>,
    pub probe_sock: String,
}

pub fn default_value(ty: Ty, tname: &str) -> Value {
    let _ = tname;
    if ty == Ty::Geo {
        Value::empty_geo()
    } else {
        Value::konst(Val::zero(ty))
    }
}

/// sockets whose unlinked default is an implicit field (hide_value sockets)
pub fn implicit_default(node_ty: &str, sock: &str) -> Option<FNode> {
    match (node_ty, sock) {
        ("FunctionNodeRandomValue", "ID") => Some(FNode::Id),
        ("GeometryNodeSetPosition", "Position") => Some(FNode::Pos),
        ("GeometryNodeExtrudeMesh", "Offset") => Some(FNode::Normal),
        ("GeometryNodeSampleIndex", "Index") => Some(FNode::Index),
        ("GeometryNodeScaleElements", "Center") => Some(FNode::Pos),
        ("GeometryNodeSampleNearest", "Sample Position") => Some(FNode::Pos),
        ("GeometryNodeInstanceOnPoints", "Position") => Some(FNode::Pos),
        ("ShaderNodeTexNoise", "Vector") | ("ShaderNodeTexVoronoi", "Vector") | ("ShaderNodeTexMusgrave", "Vector") | ("ShaderNodeTexWhiteNoise", "Vector") | ("ShaderNodeTexWave", "Vector") | ("ShaderNodeTexGradient", "Vector") => Some(FNode::Pos),
        _ => None,
    }
}

impl<'a> Ev<'a> {
    pub fn new(groups: &'a Groups, g: &'a Group, objects: &'a Objects, inputs: Vec<Value>, depth: usize) -> Ev<'a> {
        Ev { groups, g, objects, inputs, memo: RefCell::new(HashMap::default()), override_geo: RefCell::new(None), depth, probe_path: vec![], probe_sock: String::new() }
    }

    pub fn out(&self, ni: usize) -> Res<Rc<Vec<Value>>> {
        if let Some(r) = self.memo.borrow().get(&ni) {
            return Ok(r.clone());
        }
        let node = &self.g.nodes[ni];
        let r = self.run(ni, node).map_err(|e| format!("{} ({}): {}", node.name, node.ty, e))?;
        let r = Rc::new(r);
        self.memo.borrow_mut().insert(ni, r.clone());
        Ok(r)
    }

    fn run(&self, ni: usize, node: &Node) -> Res<Vec<Value>> {
        if node.mute && node.ty != "NodeGroupInput" {
            // mute: pass the first same-typed input through
            let mut outs = vec![];
            for o in &node.outs {
                let mut v = default_value(o.ty, &o.tname);
                for i in &node.ins {
                    if i.ty == o.ty && !i.multi {
                        v = self.input(ni, &i.id)?;
                        break;
                    }
                }
                outs.push(v);
            }
            return Ok(outs);
        }
        match node.ty.as_str() {
            "NodeGroupInput" => Ok(node
                .outs
                .iter()
                .map(|o| match self.g.inputs.iter().position(|i| i.id == o.id) {
                    Some(k) => self.inputs[k].clone(),
                    None => default_value(o.ty, &o.tname),
                })
                .collect()),
            "NodeReroute" => Ok(vec![self.input(ni, &node.ins[0].id)?]),
            "GeometryNodeGroup" => {
                let gname = node.ps("group");
                let sub = self.groups.map.get(gname).ok_or_else(|| format!("missing group {}", gname))?;
                if self.depth > 40 {
                    return Err("group recursion".into());
                }
                let mut ins = vec![];
                for ifc in &sub.inputs {
                    if node.in_idx(&ifc.id).is_some() {
                        ins.push(self.input(ni, &ifc.id)?);
                    } else {
                        ins.push(default_value(ifc.ty, ""));
                    }
                }
                let mut ev = Ev::new(self.groups, sub, self.objects, ins, self.depth + 1);
                if !self.probe_path.is_empty() && self.probe_path[0] == node.name {
                    ev.probe_path = self.probe_path[1..].to_vec();
                    ev.probe_sock = self.probe_sock.clone();
                    if ev.probe_path.len() == 1 {
                        let leaf = &ev.probe_path[0];
                        let li = sub.nodes.iter().position(|n| &n.name == leaf).ok_or_else(|| format!("no node {}", leaf))?;
                        let outs = ev.out(li)?;
                        let k = sub.nodes[li].out_idx(&ev.probe_sock).ok_or_else(|| format!("no socket {}", ev.probe_sock))?;
                        return Ok(vec![outs[k].clone(); node.outs.len()]);
                    } else if !ev.probe_path.is_empty() {
                        let next = ev.probe_path[0].clone();
                        let li = sub.nodes.iter().position(|n| n.name == next).ok_or_else(|| format!("no node {}", next))?;
                        let outs = ev.out(li)?;
                        return Ok(vec![outs[0].clone(); node.outs.len()]);
                    }
                }
                let res = ev.outputs()?;
                Ok(node.outs.iter().map(|o| sub.outputs.iter().position(|x| x.id == o.id).map(|k| res[k].clone()).unwrap_or_else(|| default_value(o.ty, ""))).collect())
            }
            _ => super::nodes::dispatch(self, ni, node),
        }
    }

    /// values of the group outputs (aligned with g.outputs)
    pub fn outputs(&self) -> Res<Vec<Value>> {
        let gi = self
            .g
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.ty == "NodeGroupOutput")
            .max_by_key(|(_, n)| n.pb("active") as i32)
            .map(|(i, _)| i);
        let mut res = vec![];
        for ifc in &self.g.outputs {
            match gi {
                Some(gi) if self.g.nodes[gi].in_idx(&ifc.id).is_some() => res.push(self.input(gi, &ifc.id)?),
                _ => res.push(default_value(ifc.ty, "")),
            }
        }
        Ok(res)
    }

    fn link_value(&self, li: usize, to: &Sock) -> Res<Value> {
        let l = &self.g.links[li];
        let outs = self.out(l.from)?;
        let fnode = &self.g.nodes[l.from];
        let k = fnode.out_idx(&l.fsock).ok_or_else(|| format!("no output socket {} on {}", l.fsock, fnode.name))?;
        let v = outs[k].clone();
        let from_ty = fnode.outs[k].ty;
        Ok(match v {
            Value::F(f) if from_ty != to.ty && to.ty != Ty::Geo && from_ty != Ty::Geo => Value::F(conv(&f, from_ty, to.ty)),
            Value::G(_) if to.ty != Ty::Geo => default_value(to.ty, ""),
            Value::F(_) if to.ty == Ty::Geo => Value::empty_geo(),
            v => v,
        })
    }

    pub fn set_override(&self, o: Option<(usize, String, Rc<Geometry>)>) -> Option<(usize, String, Rc<Geometry>)> {
        std::mem::replace(&mut *self.override_geo.borrow_mut(), o)
    }
    pub fn input(&self, ni: usize, id: &str) -> Res<Value> {
        if let Some((n, i, g)) = &*self.override_geo.borrow() {
            if *n == ni && i == id {
                return Ok(Value::G(g.clone()));
            }
        }
        let node = &self.g.nodes[ni];
        let si = node.in_idx(id).ok_or_else(|| format!("no input socket {}", id))?;
        let sock = &node.ins[si];
        if let Some(ls) = self.g.into.get(&(ni, id.to_string())) {
            if let Some(&li) = ls.last() {
                return self.link_value(li, sock);
            }
        }
        if let Some(f) = implicit_default(&node.ty, id) {
            return Ok(Value::F(Rc::new(f)));
        }
        Ok(match &sock.v {
            Some(v) => Value::konst(v.clone()),
            None => default_value(sock.ty, &sock.tname),
        })
    }

    pub fn has_input(&self, ni: usize, id: &str) -> bool {
        self.g.nodes[ni].in_idx(id).is_some()
    }

    pub fn is_linked(&self, ni: usize, id: &str) -> bool {
        self.g.into.get(&(ni, id.to_string())).map_or(false, |v| !v.is_empty())
    }

    pub fn input_multi(&self, ni: usize, id: &str) -> Res<Vec<Value>> {
        let node = &self.g.nodes[ni];
        let si = node.in_idx(id).ok_or_else(|| format!("no input socket {}", id))?;
        let sock = &node.ins[si];
        let mut v = vec![];
        if let Some(ls) = self.g.into.get(&(ni, id.to_string())) {
            for &li in ls {
                v.push(self.link_value(li, sock)?);
            }
        }
        Ok(v)
    }

    pub fn field(&self, ni: usize, id: &str) -> Res<Field> {
        match self.input(ni, id)? {
            Value::F(f) => Ok(f),
            _ => Err(format!("input {} is not a field", id)),
        }
    }
    pub fn geo(&self, ni: usize, id: &str) -> Res<Rc<Geometry>> {
        match self.input(ni, id)? {
            Value::G(g) => Ok(g),
            _ => Ok(Rc::new(Geometry::default())),
        }
    }
    /// constant input value (error if it is a non-constant field)
    pub fn cval(&self, ni: usize, id: &str) -> Res<Val> {
        let f = self.field(ni, id)?;
        match as_const(&f) {
            Some(v) => Ok(v.clone()),
            None => Err(format!("input {} must be constant (single value)", id)),
        }
    }
    pub fn cf(&self, ni: usize, id: &str) -> Res<f32> {
        Ok(self.cval(ni, id)?.f())
    }
    pub fn ci(&self, ni: usize, id: &str) -> Res<i32> {
        Ok(self.cval(ni, id)?.i())
    }
    pub fn cb(&self, ni: usize, id: &str) -> Res<bool> {
        Ok(self.cval(ni, id)?.b())
    }
    pub fn cv(&self, ni: usize, id: &str) -> Res<V3> {
        Ok(self.cval(ni, id)?.v())
    }
}

/// Evaluate group `name` with inputs keyed by interface identifier.
pub fn eval(groups: &Groups, name: &str, inputs: &HashMap<String, Value>, objects: &Objects) -> Res<Vec<(String, Value)>> {
    let g = groups.map.get(name).ok_or_else(|| format!("no group {}", name))?;
    let ins: Vec<Value> = g
        .inputs
        .iter()
        .map(|i| match inputs.get(&i.id).or_else(|| inputs.get(&i.name)) {
            Some(v) => v.clone(),
            None => match &i.default {
                Some(d) => Value::konst(d.clone()),
                None => default_value(i.ty, ""),
            },
        })
        .collect();
    let ev = Ev::new(groups, g, objects, ins, 0);
    let out = ev.outputs()?;
    Ok(g.outputs.iter().zip(out).map(|(i, v)| (i.id.clone(), v)).collect())
}

/// Evaluate one output socket of one node (possibly inside nested group instances: `path` = node names from the top
/// group down to the probed node) for debugging / bisecting.
pub fn eval_probe(groups: &Groups, name: &str, inputs: &HashMap<String, Value>, objects: &Objects, path: &[String], sock: &str) -> Res<Value> {
    let g = groups.map.get(name).ok_or_else(|| format!("no group {}", name))?;
    let ins: Vec<Value> = g
        .inputs
        .iter()
        .map(|i| match inputs.get(&i.id).or_else(|| inputs.get(&i.name)) {
            Some(v) => v.clone(),
            None => match &i.default {
                Some(d) => Value::konst(d.clone()),
                None => default_value(i.ty, ""),
            },
        })
        .collect();
    let mut ev = Ev::new(groups, g, objects, ins, 0);
    ev.probe_path = path.to_vec();
    ev.probe_sock = sock.to_string();
    let top = &path[0];
    let ni = g.nodes.iter().position(|n| &n.name == top).ok_or_else(|| format!("no node {}", top))?;
    let outs = ev.out(ni)?;
    if path.len() == 1 {
        let k = g.nodes[ni].out_idx(sock).ok_or_else(|| format!("no socket {}", sock))?;
        Ok(outs[k].clone())
    } else {
        Ok(outs[0].clone())
    }
}

/// The probe tail used by the Blender side (tools/gn_check.py probe_cases): RealizeInstances, then the first non-empty of
/// mesh / curves (CurveToMesh wire) / points (PointsToVertices)
pub fn probe_mesh(g: &Geometry) -> Mesh {
    use super::meshops::*;
    let real = realize_instances(g, 1000);
    if let Some(m) = &real.mesh {
        if m.nv() > 0 {
            return m.clone();
        }
    }
    if let Some(c) = &real.curves {
        if c.np() > 0 {
            return super::nodes_geo::curve_to_mesh(c, None, false);
        }
    }
    if let Some(p) = &real.points {
        let mut m = Mesh::new();
        m.pos = p.pos.clone();
        for (n, a) in p.attrs.iter() {
            m.attrs.insert(n.clone(), a.clone());
        }
        return m;
    }
    Mesh::new()
}

/// Geometry-modifier output attributes: non-geometry group outputs named by the modifier are stored on the result.
/// `out_attrs`: [{"id": interface id, "name": attribute name, "domain": "POINT"...}]
pub fn apply_out_attrs(g: &Group, outs: &[(String, Value)], geo: Geometry, out_attrs: &serde_json::Value) -> Geometry {
    let mut geo = geo;
    if let Some(a) = out_attrs.as_array() {
        for oa in a {
            let id = oa["id"].as_str().unwrap_or("");
            let name = oa["name"].as_str().unwrap_or("");
            let dom = super::geom::Dom::from_name(oa["domain"].as_str().unwrap_or("POINT"));
            let Some(k) = g.outputs.iter().position(|o| o.id == id) else { continue };
            if let Value::F(f) = &outs[k].1 {
                let ty = g.outputs[k].ty;
                super::nodes_geo::store_field(&mut geo, name, dom, ty, f, &konst(Val::B(true)));
            }
        }
    }
    geo
}
