//! JSON (tools/gn_export.py) -> node group data model.
use super::math::euler_to_quat;
use super::val::*;
use serde_json::Value as J;
use crate::dhash::HashMap;

#[derive(Clone, Debug)]
pub struct Sock {
    pub id: String,
    pub name: String,
    pub ty: Ty,
    pub tname: String,
    pub v: Option<Val>,
    pub multi: bool,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub ty: String,
    pub mute: bool,
    pub p: serde_json::Map<String, J>,
    pub ins: Vec<Sock>,
    pub outs: Vec<Sock>,
}

impl Node {
    pub fn ps(&self, k: &str) -> &str {
        self.p.get(k).and_then(|v| v.as_str()).unwrap_or("")
    }
    pub fn pb(&self, k: &str) -> bool {
        self.p.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
    }
    pub fn pi(&self, k: &str) -> i64 {
        self.p.get(k).and_then(|v| v.as_i64()).unwrap_or(0)
    }
    pub fn pf(&self, k: &str) -> f64 {
        self.p.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0)
    }
    pub fn pj(&self, k: &str) -> Option<&J> {
        self.p.get(k)
    }
    pub fn in_idx(&self, id: &str) -> Option<usize> {
        self.ins.iter().position(|s| s.id == id)
    }
    pub fn out_idx(&self, id: &str) -> Option<usize> {
        self.outs.iter().position(|s| s.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct Iface {
    pub id: String,
    pub name: String,
    pub ty: Ty,
    pub default: Option<Val>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub from: usize,
    pub fsock: String,
    pub to: usize,
    pub tsock: String,
    pub sort: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub name: String,
    pub inputs: Vec<Iface>,
    pub outputs: Vec<Iface>,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    /// (to node, to socket id) -> link indices sorted by sort id
    pub into: HashMap<(usize, String), Vec<usize>>,
}

#[derive(Debug, Default)]
pub struct Groups {
    pub map: HashMap<String, Group>,
    pub main: Option<String>,
    pub raw: J,
}

pub fn val_from_json(t: Ty, tname: &str, j: &J) -> Option<Val> {
    let fl = |j: &J| j.as_f64().unwrap_or(0.0) as f32;
    let arr = |j: &J, n: usize| -> Vec<f32> {
        let mut v: Vec<f32> = j.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
        v.resize(n, 0.0);
        v
    };
    Some(match t {
        Ty::F => Val::F(fl(j)),
        Ty::I => Val::I(j.as_i64().unwrap_or_else(|| j.as_f64().unwrap_or(0.0) as i64) as i32),
        Ty::B => Val::B(j.as_bool().unwrap_or_else(|| j.as_i64().unwrap_or(0) != 0)),
        Ty::V => {
            let a = arr(j, 3);
            Val::V([a[0], a[1], a[2]])
        }
        Ty::C => {
            let mut a = arr(j, 4);
            if j.as_array().map_or(0, |x| x.len()) == 3 {
                a[3] = 1.0;
            }
            Val::C([a[0], a[1], a[2], a[3]])
        }
        Ty::Q => {
            let a = arr(j, 3);
            Val::Q(euler_to_quat([a[0], a[1], a[2]]))
        }
        Ty::S => Val::S(j.as_str().unwrap_or("").into()),
        Ty::Ref => {
            let _ = tname;
            let n = j.get("id").or_else(|| j.get("object")).and_then(|x| x.as_str()).unwrap_or("");
            Val::Ref(n.into())
        }
        _ => return None,
    })
}

fn load_sock(j: &J) -> Sock {
    let tname = j["t"].as_str().unwrap_or("").to_string();
    let ty = Ty::from_name(&tname);
    let v = j.get("v").and_then(|x| val_from_json(ty, &tname, x));
    Sock { id: j["id"].as_str().unwrap_or("").into(), name: j["name"].as_str().unwrap_or("").into(), ty, tname, v, multi: j["multi"].as_bool().unwrap_or(false) }
}

fn load_iface(j: &J) -> Iface {
    let tname = j["type"].as_str().unwrap_or("");
    let ty = Ty::from_name(tname);
    Iface { id: j["id"].as_str().unwrap_or("").into(), name: j["name"].as_str().unwrap_or("").into(), ty, default: j.get("default").and_then(|x| val_from_json(ty, tname, x)) }
}

pub fn load_group(name: &str, j: &J) -> Group {
    let mut g = Group { name: name.to_string(), ..Default::default() };
    g.inputs = j["inputs"].as_array().map(|a| a.iter().map(load_iface).collect()).unwrap_or_default();
    g.outputs = j["outputs"].as_array().map(|a| a.iter().map(load_iface).collect()).unwrap_or_default();
    let mut idx: HashMap<String, usize> = HashMap::default();
    for n in j["nodes"].as_array().unwrap_or(&vec![]) {
        let node = Node {
            name: n["name"].as_str().unwrap_or("").into(),
            ty: n["type"].as_str().unwrap_or("").into(),
            mute: n["mute"].as_bool().unwrap_or(false),
            p: n["p"].as_object().cloned().unwrap_or_default(),
            ins: n["in"].as_array().map(|a| a.iter().map(load_sock).collect()).unwrap_or_default(),
            outs: n["out"].as_array().map(|a| a.iter().map(load_sock).collect()).unwrap_or_default(),
        };
        idx.insert(node.name.clone(), g.nodes.len());
        g.nodes.push(node);
    }
    for l in j["links"].as_array().unwrap_or(&vec![]) {
        let (Some(&f), Some(&t)) = (idx.get(l["fn"].as_str().unwrap_or("")), idx.get(l["tn"].as_str().unwrap_or(""))) else { continue };
        let li = g.links.len();
        let link = Link { from: f, fsock: l["fs"].as_str().unwrap_or("").into(), to: t, tsock: l["ts"].as_str().unwrap_or("").into(), sort: l["sort"].as_i64().unwrap_or(0) };
        g.into.entry((t, link.tsock.clone())).or_default().push(li);
        g.links.push(link);
    }
    for v in g.into.values_mut() {
        let links = &g.links;
        v.sort_by_key(|&i| links[i].sort);
    }
    g
}

pub fn load_groups(j: J) -> Groups {
    let mut gs = Groups::default();
    if let Some(m) = j["groups"].as_object() {
        for (k, v) in m {
            gs.map.insert(k.clone(), load_group(k, v));
        }
    }
    gs.main = j["main"].as_str().map(|s| s.to_string());
    gs.raw = j;
    gs
}

pub fn load_file(path: &str) -> anyhow::Result<Groups> {
    let s = std::fs::read_to_string(path)?;
    Ok(load_groups(serde_json::from_str(&s)?))
}

/// Mesh from the JSON used by case files / templates: {"v","e","f","attrs":{name:{dom,ty,data}},"mats":[..]}
pub fn mesh_from_json(j: &J) -> super::geom::Mesh {
    use super::geom::*;
    let v: Vec<[f32; 3]> = j["v"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32, p[2].as_f64().unwrap() as f32]).collect()).unwrap_or_default();
    let f: Vec<Vec<u32>> = j["f"].as_array().map(|a| a.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect()).unwrap_or_default();
    let e: Vec<[u32; 2]> = j["e"].as_array().map(|a| a.iter().map(|p| [p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32]).collect()).unwrap_or_default();
    let mut m = Mesh::from_faces(v, &f, &e);
    if let Some(attrs) = j["attrs"].as_object() {
        for (name, a) in attrs {
            let dom = Dom::from_name(a["dom"].as_str().unwrap_or("POINT"));
            let (ty, tname) = match a["ty"].as_str().unwrap_or("FLOAT") {
                "INT" | "INT8" => (Ty::I, "INT"),
                "BOOLEAN" => (Ty::B, "BOOLEAN"),
                "FLOAT_VECTOR" => (Ty::V, "VECTOR"),
                "FLOAT_COLOR" | "BYTE_COLOR" => (Ty::C, "RGBA"),
                "QUATERNION" => (Ty::Q, "ROTATION"),
                _ => (Ty::F, "FLOAT"),
            };
            let data: Vec<Val> = a["data"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .map(|x| {
                            if ty == Ty::Q {
                                let q: Vec<f32> = x.as_array().unwrap().iter().map(|y| y.as_f64().unwrap() as f32).collect();
                                Val::Q([q[0], q[1], q[2], q[3]])
                            } else {
                                val_from_json(ty, tname, x).unwrap_or(Val::zero(ty))
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            if data.len() == m.size(dom) {
                m.attrs.insert(name.clone(), Attr { dom, ty, data });
            }
        }
    }
    if let Some(ms) = j["mats"].as_array() {
        m.mats = ms.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        if m.mats.len() == 1 && m.mats[0].is_empty() {
            m.mats.clear();
        }
    }
    m
}
