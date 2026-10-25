//! Carnivore / Herbivore creatures (NatureShelfTrinkets draws them with hair=False): genome sampling in python RNG order, nurbs / node-group parts,
//! raycast attachment, join (remove_doubles 0.01 + BEAUTY triangulation). See tools/creaturedump.py for the python stage dumps.
use super::*;
use crate::gn;
use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
use crate::gn::val::{Ty, Val};
use serde_json::Value as J;
use std::sync::OnceLock;

thread_local! {
    static GROUPS: std::rc::Rc<gn::Groups> = std::rc::Rc::new(gn::load::load_groups(serde_json::from_str(include_str!("../data/creature_gn.json")).unwrap()));
}
pub fn groups() -> std::rc::Rc<gn::Groups> {
    GROUPS.with(|g| g.clone())
}

/// plain mesh in f32 (python vertex order)
#[derive(Clone, Default, Debug)]
pub struct CMesh {
    pub pos: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u32>>,
}

impl CMesh {
    pub fn from_gn(m: &GMeshT) -> CMesh {
        CMesh { pos: m.pos.clone(), faces: (0..m.nf()).map(|i| m.face(i).to_vec()).collect() }
    }
}

/// result of one node-group part: every output (geometry realized to a mesh; curve outputs as their evaluated points)
pub struct GnOut {
    pub names: Vec<String>,
    pub meshes: Vec<CMesh>,
    pub curve_pts: Vec<Vec<[f32; 3]>>,
}

impl GnOut {
    pub fn mesh(&self, name: &str) -> Option<&CMesh> {
        self.names.iter().position(|n| n == name).map(|i| &self.meshes[i])
    }
    pub fn skeleton(&self) -> Option<Vec<[f64; 3]>> {
        let i = self.names.iter().position(|n| n == "Skeleton Curve")?;
        Some(self.curve_pts[i].iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect())
    }
}

fn coerce(t: Ty, v: &J) -> Option<Val> {
    let f = |x: &J| x.as_f64().unwrap_or(0.0) as f32;
    Some(match t {
        Ty::F => Val::F(f(v)),
        Ty::I => Val::I(v.as_f64().unwrap_or(0.0) as i32),
        Ty::B => Val::B(v.as_bool().unwrap_or(v.as_f64().map_or(false, |x| x != 0.0))),
        Ty::V => {
            let a = v.as_array()?;
            Val::V([f(&a[0]), f(&a[1]), f(&a[2])])
        }
        _ => return None,
    })
}

/// evaluate node group `top` with python-side params (json object keyed by group input name)
pub fn run_group(top: &str, params: &J) -> GnOut {
    let gs_rc = groups();
    let gs = &*gs_rc;
    let g = gs.map.get(top).unwrap_or_else(|| panic!("no group {top}"));
    let mut inputs: crate::dhash::HashMap<String, gn::Value> = crate::dhash::HashMap::default();
    for i in &g.inputs {
        if let Some(v) = params.get(&i.name) {
            if let Some(val) = coerce(i.ty, v) {
                inputs.insert(i.id.clone(), gn::Value::konst(val));
            }
        }
    }
    let objects = gn::Objects::default();
    let outs = gn::eval(gs, top, &inputs, &objects).unwrap_or_else(|e| panic!("{top}: {e}"));
    let mut r = GnOut { names: vec![], meshes: vec![], curve_pts: vec![] };
    for (o, (_, v)) in g.outputs.iter().zip(outs.iter()) {
        if o.ty != Ty::Geo {
            continue;
        }
        if let gn::Value::G(geo) = v {
            let real = gn::meshops::realize_instances(geo, 1000);
            r.names.push(o.name.clone());
            r.meshes.push(real.mesh.as_ref().map(CMesh::from_gn).unwrap_or_default());
            let mut pts = vec![];
            if let Some(c) = &real.curves {
                for k in 0..c.nc() {
                    pts.extend(gn::curveops::eval_curve(c, k).pos);
                }
            }
            r.curve_pts.push(pts);
        }
    }
    r
}
