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
    /// oven: add the gas grates (gasgrate.rs) for variant 1
    pub gas: bool,
    /// last built geometry (asset() and exact() of one scene asset both need it; a gas oven takes seconds)
    pub cache: std::sync::Mutex<Option<GGeom>>,
    /// material slot name ("stub.001", "gn", ...) -> python's sampled material for that socket
    pub slot_mats: Vec<(String, Material)>,
    /// built to the layout box [width (y), depth (x), height] (microwaves): the size hint at construction
    pub hint: Option<[f64; 3]>,
}

impl BevFactory {
    pub fn geometry(&self, inst: u64) -> GGeom {
        self.try_geometry(inst).unwrap_or_else(|e| panic!("bevel asset failed: {}", e))
    }

    fn cached_geometry(&self, inst: u64) -> Result<GGeom, String> {
        if let Some(g) = self.cache.lock().unwrap().as_ref() {
            return Ok(g.clone());
        }
        let g = self.try_geometry(inst)?;
        *self.cache.lock().unwrap() = Some(g.clone());
        Ok(g)
    }

    pub fn try_geometry(&self, _inst: u64) -> Result<GGeom, String> {
        let mut s = (self.sample)(&self.rng);
        if let Some(h) = self.hint {
            for (k, v) in appliances::microwave_dims(&self.rng, h) {
                num(&mut s, k, v);
            }
        }
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
        let mut bm = add_bevel(fm, &sel, self.offset, self.segments)?;
        if s.variant == 1 && self.gas {
            bm = add_gas_grates(&bm, &s)?;
        }
        Ok(GGeom::from_mesh(bm))
    }
}

impl BevFactory {
    fn slot_mat(&self, n: &str) -> Material {
        let n = if n.is_empty() { "gn" } else { n };
        if n == "GrateIron" { return mat("grate_iron", [0.035, 0.035, 0.038], 0.55) }
        self.slot_mats.iter().find(|x| x.0 == n).map(|x| x.1.clone()).unwrap_or_else(|| mat(n, [0.6, 0.6, 0.62], 0.4))
    }
}

impl Factory for BevFactory {
    /// collision from the sampled parameters (python's own placeholder formulas: appliances.rs): the body with its door and handle, the
    /// top (an oven: its cooktop, and the control panel turned back over it as its own box), and the door -- dropping down about its bottom
    /// edge (dishwasher, oven) or swinging on its side (beverage fridge, microwave: over the door, not the control panel)
    fn volumes(&self, _inst: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::{Leaf, Volume};
        let mut s = (self.sample)(&self.rng);
        if let Some(h) = self.hint { for (k, v) in appliances::microwave_dims(&self.rng, h) { num(&mut s, k, v); } }
        let n = |k: &str| s.nums.get(k).map(|v| v[0]);
        let (d, w, h, dt) = (n("Depth")?, n("Width")?, n("Height")?, n("DoorThickness")?);
        let g = 0.004;
        let drop = |front: f64, top: f64| Volume::opening(Leaf::Hinged { panel: ([d - 0.01, 0.0, 0.0], [front, w, top]), hinge: [d, 0.0, 0.0], axis: [0.0, 1.0, 0.0], max_angle: std::f64::consts::FRAC_PI_2 });
        let side = |front: f64, y1: f64, top: f64| Volume::opening(Leaf::Hinged { panel: ([d - 0.01, 0.0, 0.0], [front, y1, top]), hinge: [d, 0.0, 0.0], axis: [0.0, 0.0, -1.0], max_angle: 100f64.to_radians() });
        if s.nums.contains_key("Grids") {
            // an oven: python's placeholder bounds, the turned control panel apart so its cooktop stays free
            let (body_hi, pl, pu) = appliances::oven_bounds(d, w, h, dt, n("PanelThickness")?, n("BottonThickness")?, n("BottonRadius")?, n("PanelHeight")?, n("BottonAmount")? as usize);
            let front = body_hi[0] + g;
            // (burner caps stand ~1 cm over the cooktop, a gas range's grates their thickness and a few cm more)
            let over = if s.variant == 1 { n("GrateThickness").unwrap_or(0.03) + 0.04 } else { 0.012 };
            return Some(vec![
                Volume::hard([-g, -g, -g], [front, w + g, body_hi[2] + over + g]),
                Volume::hard([pl[0] - g, pl[1] - g, pl[2] - g], [pu[0] + g, pu[1] + g, pu[2] + g]),
                Volume::support([pu[0] + 0.01, 0.0, h + dt - 0.001], [d, w, h + dt]),
                drop(front, h + dt),
            ]);
        }
        if let (Some(brand), true) = (s.strs.get("BrandName"), s.nums.contains_key("PanelWidth")) {
            // a microwave: the brand text centred over the window part of the door may run past the sides
            let (pw, mz) = (n("PanelWidth")?, n("MarginZ")?);
            let add = (w - pw) + mz * 0.5;
            let cell = w / 9.0;
            let a = add / cell;
            let kmax = (((a + 0.5).ceil() - 1.0).max(a.ceil() - 1.0)).clamp(0.0, 9.0);
            let yc = kmax * cell / 2.0;
            let (t0, t1) = appliances::text_x_bounds(brand);
            // and the "12:01" clock (size 0.05) centred on the control panel
            // (measured as "12001": the metrics have no colon, a digit is wider)
            let (c0, c1) = appliances::text_x_bounds("12001");
            let yk = w - pw / 2.0;
            let (y0, y1) = ((yc + 0.03 * t0).min(yk + 0.05 * c0).min(0.0), (yc + 0.03 * t1).max(yk + 0.05 * c1).max(w));
            let front = d + dt + 0.005 + g;
            return Some(vec![Volume::hard([-g, y0 - g, -g], [front, y1 + g, h + g]), Volume::support([0.0, 0.0, h - 0.001], [d, w, h]), side(front, w - pw, h)]);
        }
        // a dishwasher or a beverage fridge: door slab and a handle 7.5 % of the width on the front
        let front = d + dt + 0.075 * w + g;
        let door = if s.nums.contains_key("RackDAmount") { side(front, w, h + dt) } else { drop(front, h + dt) };
        Some(vec![Volume::hard([-g, -g, -g], [front, w + g, h + dt + g]), Volume::support([0.0, 0.0, h + dt - 0.001], [d, w, h + dt]), door])
    }
    fn asset(&self, inst: u64) -> Asset {
        if (self.sample)(&self.rng).variant != 0 && !self.supports_variants {
            return self.inner.asset(inst);
        }
        let Ok(g) = self.cached_geometry(inst) else { return self.inner.asset(inst) };
        let mut a = Asset::default();
        a.parts = parts_of(&g, &|n| self.slot_mat(n));
        a
    }
    /// each face's material slot: without them the scene mesh goes by the nearest part's box, and a stove's grates, inside the range's box, took the range's material
    fn exact_slots(&self, i: u64) -> Option<(Vec<u16>, Vec<Material>)> {
        if (self.sample)(&self.rng).variant != 0 && !self.supports_variants { return None }
        let g = self.cached_geometry(i).ok()?;
        let m = g.mesh.as_ref()?;
        let n = m.mats.len().max(1);
        let slots = (0..m.nf()).map(|f| m.attrs.get("material_index").map_or(0, |a| a.data[f].i().max(0) as usize).min(n - 1) as u16).collect();
        Some((slots, (0..n).map(|k| self.slot_mat(m.mats.get(k).map_or("", |x| x.as_str()))).collect()))
    }
    fn natural_size(&self, i: u64) -> Option<[f64; 3]> {
        self.inner.natural_size(i)
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
    let supports_variants = c != "OvenFactory" || std::env::var("OVEN_GAS").map_or(true, |v| v != "0");
    let after = match c {
        "DishwasherFactory" => dishwasher_params_r(rng.clone()).1,
        "BeverageFridgeFactory" => fridge_params_r(rng.clone()).1,
        "OvenFactory" => oven_params_r(rng.clone()).1,
        _ => microwave_params_r(rng.clone()).1,
    };
    let slot_mats = super::matpick::gn_appliance_mats(c, after, fseed);
    Some(Box::new(BevFactory { tpls, sample, rng: rng.clone(), offset: if c == "MicrowaveFactory" { 0.03 } else { 0.01 }, segments: 8, inner, supports_variants, gas: c == "OvenFactory", cache: std::sync::Mutex::new(None), slot_mats, hint: if c == "MicrowaveFactory" { crate::api::size_hint() } else { None } }))
}

fn num(s: &mut Sampled, k: &str, v: f64) {
    s.nums.insert(k.to_string(), vec![v]);
}

/// appliances/dishwasher.py sample_parameters (FixedSeed(factory_seed) stream)
fn dishwasher_params(rng: &Rng) -> Sampled {
    dishwasher_params_r(rng.clone()).0
}

/// same, also returning the generator after the draws (material choices continue the FixedSeed stream)
fn dishwasher_params_r(mut r: Rng) -> (Sampled, Rng) {
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
    (s, r)
}

/// appliances/beverage_fridge.py sample_parameters
fn fridge_params(rng: &Rng) -> Sampled {
    fridge_params_r(rng.clone()).0
}

/// same, also returning the generator after the draws (material choices continue the FixedSeed stream)
fn fridge_params_r(mut r: Rng) -> (Sampled, Rng) {
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
    (s, r)
}

/// appliances/oven.py sample_parameters (geometry-node parameters + the gas-grate parameters Grids .. MiddleRatio, which only gas_grates reads)
fn oven_params(rng: &Rng) -> Sampled {
    oven_params_r(rng.clone()).0
}

/// same, also returning the generator after the draws (material choices continue the FixedSeed stream)
fn oven_params_r(mut r: Rng) -> (Sampled, Rng) {
    let mut s = Sampled::default();
    let depth = 1.0 + r.normal(0.0, 0.1);
    let width = 1.0 + r.normal(0.0, 0.1);
    let height = 1.0 + r.normal(0.0, 0.1);
    let door_t = r.uniform(0.05, 0.1) * depth;
    // (the cooktop at the standard 0.90 m, the worktop's: KitchenSpace::new -- body + door over it)
    let height = 0.90 - door_t;
    let rack_r = r.uniform(0.01, 0.02) * depth;
    let rh = r.randint(2, 4);
    let rd = r.randint(4, 6);
    let panel_h = r.uniform(0.2, 0.4) * height;
    let panel_t = r.uniform(0.15, 0.25) * depth;
    let buttons = r.randint(1, 3) * 2;
    let button_r = r.uniform(0.05, 0.1) * width;
    let button_t = r.uniform(0.02, 0.04) * depth;
    // a standard range, as the layout's appliance (appliances.rs): 76 cm wide, 64 cm deep, its body at counter height (0.92 m), a low
    // back guard; the draws above are kept so the stream is unchanged. (The sampled 1 m box overlapped the counter runs beside it and
    // the overlap pass deleted the range.)
    let (sd, sw, sh) = (0.64 / depth, 0.76 / width, 0.92 / height);
    let (depth, width, height) = (0.64, 0.76, 0.92);
    let (door_t, rack_r, panel_t, button_t, button_r) = (door_t * sd, rack_r * sd, panel_t * sd, button_t * sd, button_r * sw);
    let panel_h = (panel_h * sh).clamp(0.1, 0.18);
    let heat = r.uniform(0.1, 0.2);
    let brand = appliances::generate_text(&mut r);
    let use_gas = r.randint(0, 2);
    let n_grids = r.randint(2, 5);
    let grids: Vec<f64> = (0..n_grids).map(|_| r.randint(1, 4) as f64).collect();
    let branches = 2 * r.randint(2, 9);
    s.nums.insert("Grids".into(), grids);
    num(&mut s, "Branches", branches as f64);
    num(&mut s, "GrateThickness", r.uniform(0.01, 0.03));
    num(&mut s, "CenterRatio", r.uniform(0.05, 0.15));
    num(&mut s, "MiddleRatio", r.uniform(0.5, 0.7));
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
    (s, r)
}

/// appliances/microwave.py sample_parameters
fn microwave_params(rng: &Rng) -> Sampled {
    microwave_params_r(rng.clone()).0
}

/// same, also returning the generator after the draws (material choices continue the FixedSeed stream)
fn microwave_params_r(mut r: Rng) -> (Sampled, Rng) {
    let mut s = Sampled::default();
    let (depth, width, height, panel_w) = (r.uniform(0.5, 0.7), r.uniform(0.6, 1.0), r.uniform(0.35, 0.45), r.uniform(0.2, 0.4));
    num(&mut s, "Depth", depth);
    // (the window part at least 0.46 m wide: the turntable, ~0.2 m round, stood through the side of a narrower one)
    num(&mut s, "Width", width.max(panel_w + 0.46));
    num(&mut s, "Height", height);
    num(&mut s, "PanelWidth", panel_w);
    num(&mut s, "MarginZ", r.uniform(0.05, 0.1));
    num(&mut s, "DoorThickness", r.uniform(0.02, 0.04));
    num(&mut s, "DoorMargin", r.uniform(0.03, 0.1));
    num(&mut s, "DoorRotation", 0.0);
    s.strs.insert("BrandName".into(), appliances::generate_text(&mut r));
    (s, r)
}

/// OvenFactory.create_asset tail for use_gas: grates + hollow cut of the body + join (see gasgrate.rs)
/// plain (Send) copy of a finished gas oven mesh: gas ovens take seconds, the result is shared between factory instances (solver / visual asset)
struct GasCached {
    pos: Vec<[f32; 3]>,
    faces: Vec<Vec<u32>>,
    mi: Vec<i32>,
    mats: Vec<String>,
}

fn add_gas_grates(body: &GMesh, s: &Sampled) -> Result<GMesh, String> {
    static GAS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, GasCached>>> = std::sync::OnceLock::new();
    let key = {
        let mut k: Vec<_> = s.nums.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
        k.sort();
        k.join(";")
    };
    if let Some(c) = GAS.get_or_init(Default::default).lock().unwrap().get(&key) {
        let mut out = GMesh::from_faces(c.pos.clone(), &c.faces, &[]);
        out.mats = c.mats.clone();
        out.attrs.insert("material_index".to_string(), Attr { dom: Dom::Face, ty: Ty::I, data: c.mi.iter().map(|&i| Val::I(i)).collect() });
        return Ok(out);
    }
    let out = add_gas_grates_uncached(body, s)?;
    let mi: Vec<i32> = (0..out.nf()).map(|f| out.attrs.get("material_index").map(|a| a.data[f].i()).unwrap_or(0)).collect();
    GAS.get_or_init(Default::default).lock().unwrap().insert(key, GasCached { pos: out.pos.clone(), faces: out.faces_vec(), mi, mats: out.mats.clone() });
    Ok(out)
}

fn add_gas_grates_uncached(body: &GMesh, s: &Sampled) -> Result<GMesh, String> {
    use super::gasgrate::*;
    let n = |k: &str| s.nums.get(k).map(|v| v[0]).ok_or_else(|| format!("missing {k}"));
    let (width, depth0, height0, door) = (n("Width")?, n("Depth")?, n("Height")?, n("DoorThickness")?);
    let t = n("GrateThickness")?;
    let depth = depth0 + 2.0 * door;
    let p = GrateParams {
        width,
        depth,
        grate_width: width * 0.8,
        grate_depth: depth * 0.6,
        height: height0 + door - t,
        thickness: t,
        grids: s.nums.get("Grids").ok_or("missing Grids")?.iter().map(|&g| g as usize).collect(),
        branches: n("Branches")? as usize,
        center_ratio: n("CenterRatio")?,
        middle_ratio: n("MiddleRatio")?,
    };
    let tg = web_time::Instant::now();
    let grates = gas_grates(&p).ok_or("gas grates failed (voxel remesh unavailable?)")?;
    if std::env::var("GASDBG").is_ok() {
        eprintln!("gas_grates built in {:?}", tg.elapsed());
    }
    let hollow = hollow(&p, height0 + door).ok_or("hollow cutter failed")?;
    // body - hollow (BOOLEAN EXACT, use_hole_tolerant), then join the grates
    use crate::exbool::exact_boolean::*;
    let to_in = |pos: &[[f32; 3]], faces: Vec<Vec<u32>>| BoolInput { verts: pos.to_vec(), faces };
    let hollow_in = BoolInput { verts: hollow.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect(), faces: hollow.faces.clone() };
    let r = exact_boolean(&[to_in(&body.pos, body.faces_vec()), hollow_in], BoolOp::Difference, false, true);
    if std::env::var("GASDBG").is_ok() {
        eprintln!("gas oven add_gas_grates total {:?}", tg.elapsed());
    }
    let mut mats = body.mats.clone();
    // (cast iron: in the range's own white the grates did not read at all)
    let white = { mats.push("GrateIron".to_string()); mats.len() - 1 };
    let body_mi: Vec<i32> = (0..body.nf()).map(|f| body.attrs.get("material_index").map(|a| a.data[f].i()).unwrap_or(0)).collect();
    let mut mi: Vec<Val> = r.face_orig.iter().map(|&(_, f)| Val::I(body_mi[f])).collect();
    let mut pos = r.verts.clone();
    let mut faces = r.faces.clone();
    let base = pos.len() as u32;
    pos.extend(grates.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]));
    for f in &grates.faces {
        faces.push(f.iter().map(|&v| v + base).collect());
        mi.push(Val::I(white as i32));
    }
    let mut out = GMesh::from_faces(pos, &faces, &[]);
    out.mats = mats;
    out.attrs.insert("material_index".to_string(), Attr { dom: Dom::Face, ty: Ty::I, data: mi });
    Ok(out)
}
