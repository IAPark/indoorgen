//! Node-graph assets finished with `add_bevel` (appliances): two exported graph steps (preprocess, final), python's
//! `get_bevel_edges` selection on the preprocess mesh, then the ported bmesh bevel (src/bm) on the final mesh.
use super::gnasset::*;
use super::*;
use crate::bm::bevel::{bevel_edges, BevelOpts, OffsetType};
use crate::bm::BMesh;
use crate::gn;
use crate::gn::geom::{Attr, Dom, Geometry as GGeom, Mesh as GMesh};
use crate::gn::load::val_from_json;
use crate::gn::val::{Ty, Val};
use serde_json::Value as J;
use crate::dhash::HashMap;
use std::sync::OnceLock;

#[derive(Default, Clone)]
pub struct Sampled {
    pub nums: Params,
    pub strs: HashMap<String, String>,
    /// template variant (oven: 1 = gas)
    pub variant: usize,
}

pub struct BevTpl {
    raw: J,
}

impl BevTpl {
    pub fn parse(json: &str) -> BevTpl {
        let raw: J = serde_json::from_str(json).expect("bevel template json");
        BevTpl { raw }
    }

    /// evaluate exported step `i` (0 = preprocess, 1 = final) with the sampled group inputs; realized
    pub fn step(&self, i: usize, s: &Sampled) -> Result<GGeom, String> {
        let groups = gn::load::load_groups(self.raw.clone());
        let step = &self.raw["steps"][i];
        let gname = step["group"].as_str().ok_or("no step")?;
        let g = groups.map.get(gname).ok_or("missing group")?;
        let mut inputs: HashMap<String, gn::Value> = HashMap::default();
        for ifc in &g.inputs {
            if ifc.ty == Ty::Geo {
                let m = &step["mesh"];
                let mesh = if m.is_null() { GMesh::new() } else { gn::load::mesh_from_json(m) };
                inputs.insert(ifc.id.clone(), gn::Value::geo(GGeom::from_mesh(mesh)));
                continue;
            }
            let v: Option<Val> = if let Some(st) = s.strs.get(&ifc.name) {
                Some(Val::S(st.as_str().into()))
            } else if let Some(n) = s.nums.get(&ifc.name) {
                match ifc.ty {
                    Ty::F => Some(Val::F(n[0] as f32)),
                    Ty::I => Some(Val::I(n[0] as i32)),
                    Ty::B => Some(Val::B(n[0] != 0.0)),
                    Ty::V => Some(Val::V([n[0] as f32, n[1] as f32, n[2] as f32])),
                    _ => None,
                }
            } else {
                step["inputs"].get(&ifc.id).and_then(|v| val_from_json(ifc.ty, "", v))
            };
            if let Some(v) = v {
                inputs.insert(ifc.id.clone(), gn::Value::konst(v));
            }
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

/// infinigen.core.util.bevelling.get_bevel_edges on the preprocess mesh (edge indices in mesh edge order)
pub fn get_bevel_edges(m: &GMesh) -> Vec<usize> {
    let inf = 1e5f64;
    let pts: Vec<[f64; 3]> = m.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    let mag = |p: &[f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    let mut mn = [f64::INFINITY; 3];
    let mut mx = [f64::NEG_INFINITY; 3];
    for p in &pts {
        if mag(p) < 0.5 * inf {
            for k in 0..3 {
                mn[k] = mn[k].min(p[k]);
                mx[k] = mx[k].max(p[k]);
            }
        }
    }
    let flags = |p: &[f64; 3]| {
        let mut f = [0i32; 3];
        for k in 0..3 {
            if (p[k] - mn[k]).abs() < 1e-4 {
                f[k] = -1;
            } else if (p[k] - mx[k]).abs() < 1e-4 {
                f[k] = 1;
            }
        }
        f
    };
    let mut out = vec![];
    for (i, e) in m.edges.iter().enumerate() {
        let p0 = pts[e[0] as usize];
        let p1 = pts[e[1] as usize];
        let (f0, f1) = (flags(&p0), flags(&p1));
        let mut cnt = 0;
        for k in 0..3 {
            if f0[k] != 0 && f0[k] == f1[k] {
                cnt += 1;
            }
        }
        let m0 = mag(&p0);
        if cnt >= 2 {
            out.push(i);
        } else if m0 > 0.5 * inf && m0 < 1.5 * inf {
            out.push(i);
        }
    }
    out
}

/// `add_bevel(obj, edges, offset, segments)`: bpy.ops.mesh.bevel with the operator defaults
pub fn add_bevel(m: &GMesh, edges: &[usize], offset: f32, segments: i32) -> Result<GMesh, String> {
    let faces = m.faces_vec();
    let ce: Vec<Vec<u32>> = (0..m.nf()).map(|f| m.face_range(f).map(|c| m.ce[c]).collect()).collect();
    let mut bm = BMesh::from_mesh(&m.pos, &m.edges, &faces, &ce);
    let mats: Vec<i32> = (0..m.nf()).map(|f| m.attrs.get("material_index").map(|a| a.data[f].i()).unwrap_or(0)).collect();
    for (i, fid) in bm.face_ids().into_iter().enumerate() {
        bm.faces[fid].mat_nr = mats[i] as i16;
    }
    let sharp_e: Vec<bool> = (0..m.ne()).map(|e| m.attrs.get("sharp_edge").map(|a| a.dom == Dom::Edge && a.data[e].b()).unwrap_or(false)).collect();
    let sharp_f: Vec<bool> = (0..m.nf()).map(|f| m.attrs.get("sharp_face").map(|a| a.dom == Dom::Face && a.data[f].b()).unwrap_or(false)).collect();
    for (i, eid) in bm.edge_ids().into_iter().enumerate() {
        if sharp_e[i] {
            bm.edges[eid].flag &= !crate::bm::ELEM_SMOOTH;
        }
    }
    for (i, fid) in bm.face_ids().into_iter().enumerate() {
        if !sharp_f[i] {
            bm.faces[fid].flag |= crate::bm::ELEM_SMOOTH;
        }
    }
    let o = BevelOpts { offset, offset_type: OffsetType::Offset, segments, profile: 0.5, limit_offset: false, loop_slide: true, mat_nr: -1 };
    bevel_edges(&mut bm, edges, &o)?;
    let (pos, edges2, faces2) = bm.to_mesh();
    let mi: Vec<Val> = bm.face_ids().iter().map(|&f| Val::I(bm.faces[f].mat_nr as i32)).collect();
    let mut out = GMesh::from_faces(pos, &faces2, &edges2);
    out.mats = m.mats.clone();
    out.attrs.insert("material_index".to_string(), Attr { dom: Dom::Face, ty: Ty::I, data: mi });
    let se: Vec<Val> = bm.edge_ids().iter().map(|&e| Val::B(bm.edges[e].flag & crate::bm::ELEM_SMOOTH == 0)).collect();
    if se.iter().any(|v| v.b()) {
        out.attrs.insert("sharp_edge".to_string(), Attr { dom: Dom::Edge, ty: Ty::B, data: se });
    }
    let sf: Vec<Val> = bm.face_ids().iter().map(|&f| Val::B(bm.faces[f].flag & crate::bm::ELEM_SMOOTH == 0)).collect();
    if sf.iter().any(|v| v.b()) {
        out.attrs.insert("sharp_face".to_string(), Attr { dom: Dom::Face, ty: Ty::B, data: sf });
    }
    Ok(out)
}

/// two-step bevelled node-graph factory. `inner` supplies placeholder / bounds hints (kept exact).
pub struct BevFactory {
    pub tpls: Vec<&'static BevTpl>,
    pub sample: fn(&Rng) -> Sampled,
    pub rng: Rng,
    pub offset: f32,
    pub segments: i32,
    pub inner: Box<dyn Factory>,
    /// false: template variants != 0 are not finished (gas oven grates), fall back to `inner`
    pub supports_variants: bool,
}

impl BevFactory {
    pub fn geometry(&self, inst: u64) -> GGeom {
        self.try_geometry(inst).unwrap_or_else(|e| panic!("bevel asset failed: {}", e))
    }

    pub fn try_geometry(&self, _inst: u64) -> Result<GGeom, String> {
        let s = (self.sample)(&self.rng);
        let tpl = self.tpls[s.variant];
        let pre = tpl.step(0, &s)?;
        let fin = tpl.step(1, &s)?;
        let pm = pre.mesh.as_ref().ok_or("no preprocess mesh")?;
        let fm = fin.mesh.as_ref().ok_or("no final mesh")?;
        let sel = get_bevel_edges(pm);
        if std::env::var("BEVDBG").is_ok() {
            { use std::io::Write; let mut f = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/bevdbg.txt").unwrap(); let _ = writeln!(f, "bevel: pre nv {} ne {} nf {}; fin nv {} ne {} nf {}; sel {}", pm.nv(), pm.ne(), pm.nf(), fm.nv(), fm.ne(), fm.nf(), sel.len()); }
        }
        let sel: Vec<usize> = if pm.edges.len() == fm.edges.len() { sel } else { vec![] };
        let bm = add_bevel(fm, &sel, self.offset, self.segments)?;
        Ok(GGeom::from_mesh(bm))
    }
}

impl Factory for BevFactory {
    fn asset(&self, inst: u64) -> Asset {
        if (self.sample)(&self.rng).variant != 0 && !self.supports_variants {
            return self.inner.asset(inst);
        }
        let Ok(g) = self.try_geometry(inst) else { return self.inner.asset(inst) };
        let mut a = Asset::default();
        a.parts = parts_of(&g, &|n| mat(if n.is_empty() { "gn" } else { n }, [0.6, 0.6, 0.62], 0.4));
        a
    }
    fn placeholder(&self, i: u64) -> Option<Asset> {
        self.inner.placeholder(i)
    }
    fn bounds_hint(&self, i: u64) -> Option<(V3, V3)> {
        self.inner.bounds_hint(i)
    }
    fn placeholder_exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        self.inner.placeholder_exact(i)
    }
    fn exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        if (self.sample)(&self.rng).variant != 0 && !self.supports_variants {
            return None;
        }
        exact_of(&self.geometry(i))
    }
}

pub fn handles(c: &str) -> bool {
    matches!(c, "DishwasherFactory" | "BeverageFridgeFactory" | "OvenFactory" | "MicrowaveFactory")
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    let inner = appliances::make(c, &mut rng.clone(), fseed)?;
    let (tpls, sample): (Vec<&'static BevTpl>, fn(&Rng) -> Sampled) = match c {
        "DishwasherFactory" => {
            static T: OnceLock<BevTpl> = OnceLock::new();
            (vec![T.get_or_init(|| BevTpl::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/dishwasher.json"))))], dishwasher_params)
        }
        "BeverageFridgeFactory" => {
            static T: OnceLock<BevTpl> = OnceLock::new();
            (vec![T.get_or_init(|| BevTpl::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/beverage_fridge.json"))))], fridge_params)
        }
        "OvenFactory" => {
            static E: OnceLock<BevTpl> = OnceLock::new();
            static G: OnceLock<BevTpl> = OnceLock::new();
            (
                vec![E.get_or_init(|| BevTpl::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/oven_electric.json")))), G.get_or_init(|| BevTpl::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/oven_gas.json"))))],
                oven_params,
            )
        }
        "MicrowaveFactory" => {
            static T: OnceLock<BevTpl> = OnceLock::new();
            (vec![T.get_or_init(|| BevTpl::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/microwave.json"))))], microwave_params)
        }
        _ => return None,
    };
    let supports_variants = c != "OvenFactory";
    Some(Box::new(BevFactory { tpls, sample, rng: rng.clone(), offset: if c == "MicrowaveFactory" { 0.03 } else { 0.01 }, segments: 8, inner, supports_variants }))
}

fn num(s: &mut Sampled, k: &str, v: f64) {
    s.nums.insert(k.to_string(), vec![v]);
}

/// appliances/dishwasher.py sample_parameters (FixedSeed(factory_seed) stream)
fn dishwasher_params(rng: &Rng) -> Sampled {
    let mut r = rng.clone();
    let mut s = Sampled::default();
    let depth = 1.0 + r.normal(0.0, 0.1);
    let width = 1.0 + r.normal(0.0, 0.1);
    let height = 1.0 + r.normal(0.0, 0.1);
    let door_t = r.uniform(0.05, 0.1) * depth;
    let rack_r = r.uniform(0.01, 0.02) * depth;
    let racks = r.randint(2, 3);
    num(&mut s, "Depth", depth);
    num(&mut s, "Width", width);
    num(&mut s, "Height", height);
    num(&mut s, "DoorThickness", door_t);
    num(&mut s, "DoorRotation", 0.0);
    num(&mut s, "RackRadius", rack_r);
    num(&mut s, "RackAmount", racks as f64);
    s.strs.insert("BrandName".into(), "BrandName".into());
    s
}

/// appliances/beverage_fridge.py sample_parameters
fn fridge_params(rng: &Rng) -> Sampled {
    let mut r = rng.clone();
    let mut s = Sampled::default();
    let depth = 1.0 + r.normal(0.0, 0.1);
    let width = 1.0 + r.normal(0.0, 0.1);
    let height = 1.0 + r.normal(0.0, 0.1);
    let door_t = r.uniform(0.05, 0.1) * depth;
    let rack_r = r.uniform(0.01, 0.02) * depth;
    let rh = r.randint(2, 4);
    let rd = r.randint(4, 6);
    num(&mut s, "Depth", depth);
    num(&mut s, "Width", width);
    num(&mut s, "Height", height);
    num(&mut s, "DoorThickness", door_t);
    num(&mut s, "DoorRotation", 0.0);
    num(&mut s, "RackRadius", rack_r);
    num(&mut s, "RackHAmount", rh as f64);
    num(&mut s, "RackDAmount", rd as f64);
    s.strs.insert("BrandName".into(), "BrandName".into());
    s
}

/// appliances/oven.py sample_parameters (geometry-node parameters; the gas-grate parameters are drawn but unused here)
fn oven_params(rng: &Rng) -> Sampled {
    let mut r = rng.clone();
    let mut s = Sampled::default();
    let depth = 1.0 + r.normal(0.0, 0.1);
    let width = 1.0 + r.normal(0.0, 0.1);
    let height = 1.0 + r.normal(0.0, 0.1);
    let door_t = r.uniform(0.05, 0.1) * depth;
    let rack_r = r.uniform(0.01, 0.02) * depth;
    let rh = r.randint(2, 4);
    let rd = r.randint(4, 6);
    let panel_h = r.uniform(0.2, 0.4) * height;
    let panel_t = r.uniform(0.15, 0.25) * depth;
    let buttons = r.randint(1, 3) * 2;
    let button_r = r.uniform(0.05, 0.1) * width;
    let button_t = r.uniform(0.02, 0.04) * depth;
    let heat = r.uniform(0.1, 0.2);
    let brand = appliances::generate_text(&mut r);
    let use_gas = r.randint(0, 2);
    num(&mut s, "Depth", depth);
    num(&mut s, "Width", width);
    num(&mut s, "Height", height);
    num(&mut s, "DoorThickness", door_t);
    num(&mut s, "DoorRotation", 0.0);
    num(&mut s, "RackRadius", rack_r);
    num(&mut s, "RackHAmount", rh as f64);
    num(&mut s, "RackDAmount", rd as f64);
    num(&mut s, "PanelHeight", panel_h);
    num(&mut s, "PanelThickness", panel_t);
    num(&mut s, "BottonAmount", buttons as f64);
    num(&mut s, "BottonRadius", button_r);
    num(&mut s, "BottonThickness", button_t);
    num(&mut s, "HeaterRadiusRatio", heat);
    s.strs.insert("BrandName".into(), brand);
    s.variant = use_gas as usize;
    s
}

/// appliances/microwave.py sample_parameters
fn microwave_params(rng: &Rng) -> Sampled {
    let mut r = rng.clone();
    let mut s = Sampled::default();
    num(&mut s, "Depth", r.uniform(0.5, 0.7));
    num(&mut s, "Width", r.uniform(0.6, 1.0));
    num(&mut s, "Height", r.uniform(0.35, 0.45));
    num(&mut s, "PanelWidth", r.uniform(0.2, 0.4));
    num(&mut s, "MarginZ", r.uniform(0.05, 0.1));
    num(&mut s, "DoorThickness", r.uniform(0.02, 0.04));
    num(&mut s, "DoorMargin", r.uniform(0.03, 0.1));
    num(&mut s, "DoorRotation", 0.0);
    s.strs.insert("BrandName".into(), appliances::generate_text(&mut r));
    s
}
