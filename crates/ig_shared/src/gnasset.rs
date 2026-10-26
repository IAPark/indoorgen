//! Generic "geometry-node asset" support: a JSON template exported by tools/gn_export.py (`template` mode) plus a Rust
//! parameter sampler (RNG-exact) become a Factory. The node graph is evaluated by the `gn` interpreter.
use super::cabnodes::ExactMesh;
use super::*;
use crate::gn;
use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::Ty;
use serde_json::Value as J;
use crate::dhash::HashMap;

pub type Params = HashMap<String, Vec<f64>>;

#[derive(Clone, Debug)]
enum PathEl {
    K(String),
    I(usize),
}

#[derive(Clone, Debug)]
struct Patch {
    group: String,
    node: String,
    at: Vec<PathEl>,
    param: String,
    idx: Option<usize>,
}

#[derive(Clone)]
pub struct GnTemplate {
    raw: J,
    patches: Vec<Patch>,
}

impl GnTemplate {
    pub fn parse(json: &str) -> GnTemplate {
        let raw: J = serde_json::from_str(json).expect("gn template json");
        let mut patches = vec![];
        for p in raw["patches"].as_array().cloned().unwrap_or_default() {
            let at = p["at"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| match x {
                    J::String(s) => PathEl::K(s.clone()),
                    other => PathEl::I(other.as_u64().unwrap() as usize),
                })
                .collect();
            patches.push(Patch {
                group: p["g"].as_str().unwrap().to_string(),
                node: p["n"].as_str().unwrap().to_string(),
                at,
                param: p["param"].as_str().unwrap().to_string(),
                idx: p.get("idx").and_then(|i| i.as_u64()).map(|i| i as usize),
            });
        }
        GnTemplate { raw, patches }
    }

    pub fn clone_template(&self) -> GnTemplate {
        self.clone()
    }

    /// overwrite the default of a group interface input (e.g. values a factory drew when it built the node group)
    pub fn set_group_default(&mut self, group: &str, id: &str, v: J) {
        if let Some(ins) = self.raw["groups"][group]["inputs"].as_array_mut() {
            for i in ins.iter_mut() {
                if i["id"].as_str() == Some(id) {
                    i["default"] = v.clone();
                }
            }
        }
    }

    /// cap the Level input of every SubdivideMesh / SubdivisionSurface node (cheap outline-only evaluation; the shape stays within the control cage)
    pub fn cap_subdivision(&mut self, max_level: i64) {
        let Some(groups) = self.raw["groups"].as_object_mut() else { return };
        for (_, g) in groups.iter_mut() {
            let Some(nodes) = g["nodes"].as_array_mut() else { continue };
            for n in nodes.iter_mut() {
                if !matches!(n["type"].as_str(), Some("GeometryNodeSubdivideMesh") | Some("GeometryNodeSubdivisionSurface")) {
                    continue;
                }
                if let Some(ins) = n["in"].as_array_mut() {
                    for i in ins.iter_mut() {
                        if i["id"].as_str() == Some("Level") {
                            if let Some(v) = i["v"].as_i64() {
                                if v > max_level {
                                    i["v"] = J::from(max_level);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// the groups with every parameter patch applied
    pub fn instantiate(&self, params: &Params) -> gn::Groups {
        let mut j = self.raw.clone();
        for p in &self.patches {
            let Some(vals) = params.get(&p.param) else { continue };
            let v = vals[p.idx.unwrap_or(0)];
            let Some(nodes) = j["groups"][&p.group]["nodes"].as_array_mut() else { continue };
            let Some(node) = nodes.iter_mut().find(|n| n["name"].as_str() == Some(&p.node)) else { continue };
            let mut cur: &mut J = node;
            for el in &p.at {
                cur = match el {
                    PathEl::K(k) => &mut cur[k.as_str()],
                    PathEl::I(i) => &mut cur[*i],
                };
            }
            let is_int = cur.as_i64().is_some() && cur.as_f64().map_or(false, |f| f.fract() == 0.0) && v.fract() == 0.0;
            *cur = if is_int { J::from(v as i64) } else { J::from(v) };
        }
        gn::load::load_groups(j)
    }

    /// evaluate a group with every input at its (patched) interface default; returns the geometry outputs by name (instances realized)
    pub fn run_group(&self, group: &str) -> Result<Vec<(String, GGeom)>, String> {
        let groups = self.instantiate(&Params::default());
        let g = groups.map.get(group).ok_or("missing group")?;
        let objects = gn::Objects::default();
        let outs = gn::eval(&groups, group, &HashMap::default(), &objects)?;
        let mut res = vec![];
        for (k, o) in g.outputs.iter().enumerate() {
            if o.ty != Ty::Geo {
                continue;
            }
            if let gn::Value::G(gg) = &outs[k].1 {
                res.push((o.name.clone(), gn::meshops::realize_instances(&(**gg).clone(), 1000)));
            }
        }
        Ok(res)
    }

    /// evaluate the first recorded step (modifier) with the given parameters; returns the first geometry output
    pub fn run(&self, params: &Params) -> Result<GGeom, String> {
        let groups = self.instantiate(params);
        let step = &self.raw["steps"][0];
        let gname = step["group"].as_str().ok_or("no step")?;
        let g = groups.map.get(gname).ok_or("missing group")?;
        let mut inputs: HashMap<String, gn::Value> = HashMap::default();
        if let Some(i) = g.inputs.iter().find(|i| i.ty == Ty::Geo) {
            let m = &step["mesh"];
            let mesh = if m.is_null() { GMeshT::new() } else { gn::load::mesh_from_json(m) };
            inputs.insert(i.id.clone(), gn::Value::geo(GGeom::from_mesh(mesh)));
        }
        let objects = gn::Objects::default();
        let outs = gn::eval(&groups, gname, &inputs, &objects)?;
        let gi = g.outputs.iter().position(|o| o.ty == Ty::Geo).ok_or("no geometry output")?;
        match &outs[gi].1 {
            gn::Value::G(gg) => {
                let geo = if step.get("out_attrs").map_or(false, |a| a.is_array()) { gn::apply_out_attrs(g, &outs, (**gg).clone(), &step["out_attrs"]) } else { (**gg).clone() };
                Ok(gn::meshops::realize_instances(&geo, 1000))
            }
            _ => Err("output is not geometry".into()),
        }
    }
}

/// gn mesh -> crate::mesh::Mesh parts split by material slot
pub fn parts_of(g: &GGeom, mat_for: &dyn Fn(&str) -> Material) -> Vec<Part> {
    let Some(m) = &g.mesh else { return vec![] };
    let nslots = m.mats.len().max(1);
    let mut meshes: Vec<Mesh> = (0..nslots).map(|_| Mesh::new()).collect();
    for f in 0..m.nf() {
        let mi = m.attrs.get("material_index").map(|a| a.data[f].i().max(0) as usize).unwrap_or(0).min(nslots - 1);
        let pts: Vec<V3> = m.face(f).iter().map(|&v| [m.pos[v as usize][0] as f64, m.pos[v as usize][1] as f64, m.pos[v as usize][2] as f64]).collect();
        meshes[mi].add_face(&pts);
    }
    meshes
        .into_iter()
        .enumerate()
        .filter(|(_, mm)| !mm.faces.is_empty())
        .map(|(i, mm)| {
            let name = m.mats.get(i).cloned().unwrap_or_default();
            Part { mesh: mm, mat: mat_for(&name), col: None }
        })
        .collect()
}

pub fn exact_of(g: &GGeom) -> Option<ExactMesh> {
    let m = g.mesh.as_ref()?;
    Some(ExactMesh {
        verts: m.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(),
        polys: m.faces_vec(),
        names: vec![String::new(); m.nf()],
    })
}

/// Factory built from a template + sampler `fn(factory_seed, inst_seed) -> Params`
pub struct GnFactory {
    pub tpl: &'static GnTemplate,
    pub sample: fn(u64, u64) -> Params,
    pub fseed: u64,
}

impl GnFactory {
    pub fn geometry(&self, inst: u64) -> GGeom {
        let params = (self.sample)(self.fseed, inst);
        self.tpl.run(&params).unwrap_or_else(|e| panic!("gn asset evaluation failed: {}", e))
    }
}

impl Factory for GnFactory {
    fn asset(&self, inst: u64) -> Asset {
        let g = self.geometry(inst);
        let mut a = Asset::default();
        a.parts = parts_of(&g, &|n| mat(if n.is_empty() { "gn" } else { n }, [0.6, 0.55, 0.5], 0.5));
        a
    }
    fn exact(&self, inst: u64) -> Option<ExactMesh> {
        exact_of(&self.geometry(inst))
    }
}
