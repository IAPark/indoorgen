//! Solver state: objects (geometry, pose, tags, relations) and cached derived geometry.
use super::domain::Domain;
use super::geom::*;
use super::relations::*;
use super::tags::*;
use crate::mesh::{cross, dot, sub, V3};
use crate::shp::G;
use indexmap::IndexMap;
use std::cell::RefCell;
use crate::dhash::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct RelState {
    pub relation: Relation,
    pub target: String,
    pub child_plane: usize,
    pub parent_plane: usize,
}

#[derive(Clone, Debug)]
pub struct GenRef {
    pub class: String,
    pub factory_seed: u64,
    pub inst_seed: u64,
    /// constructor draws some factories need for the final asset (BedFactory sheet type / fold / cover)
    pub unseeded: Option<Vec<f64>>,
}

#[derive(Clone)]
pub struct ObjState {
    pub geom: Arc<ObjGeom>,
    pub pose: Pose,
    pub tags: TagSet,
    pub generator: Option<GenRef>,
    pub relations: Vec<RelState>,
    pub dof_t: Option<[[f64; 3]; 3]>,
    pub dof_r: Option<V3>,
    pub active: bool,
    pub polygon: Option<G>,
    pub version: u64,
}

pub struct World {
    pub tris: Vec<[V3; 3]>,
    pub areas: Vec<f64>,
    pub centroid: V3,
    pub lo: V3,
    pub hi: V3,
}

#[derive(Default)]
pub struct Caches {
    world: HashMap<String, (u64, Arc<World>)>,
    planes: HashMap<(String, u32, u32), (u64, Arc<Vec<usize>>)>,
}

pub struct State {
    pub objs: IndexMap<String, ObjState>,
    pub caches: RefCell<Caches>,
    pub next_version: u64,
}

impl ObjState {
    pub fn new(geom: Arc<ObjGeom>, pose: Pose, tags: TagSet) -> ObjState {
        ObjState { geom, pose, tags, generator: None, relations: vec![], dof_t: None, dof_r: None, active: true, polygon: None, version: 0 }
    }
}

impl State {
    pub fn new() -> State {
        State { objs: IndexMap::new(), caches: RefCell::new(Caches::default()), next_version: 1 }
    }

    pub fn touch(&mut self, name: &str) {
        let v = self.next_version;
        self.next_version += 1;
        self.objs.get_mut(name).unwrap().version = v;
    }

    /// bpy.ops.object.transform_apply(scale=True): the (float32) object scale is multiplied into the mesh vertices
    pub fn bake_scale(&mut self, name: &str) {
        let o = self.objs.get_mut(name).unwrap();
        let sc = o.pose.o.scale;
        if sc == [1.0, 1.0, 1.0] {
            return;
        }
        let f = |v: V3| -> V3 { [(v[0] as f32 * sc[0]) as f64, (v[1] as f32 * sc[1]) as f64, (v[2] as f32 * sc[2]) as f64] };
        let mut tris = o.geom.tris.clone();
        for t in tris.iter_mut() {
            for k in 0..3 {
                t.v[k] = f(t.v[k]);
            }
        }
        let mut g = ObjGeom::from_tris(tris, o.geom.proxies.clone());
        if let Some(c) = &o.geom.convex {
            g = g.with_mesh_verts(c.iter().map(|v| f(*v)).collect());
        }
        o.geom = std::sync::Arc::new(g);
        o.pose.o.scale = [1.0, 1.0, 1.0];
        o.pose = Pose::from_o32(o.pose.o);
        self.touch(name);
    }

    /// parse_scene.add_to_scene: `obj.matrix_world = Matrix(obj.matrix_world)` re-derives location / euler / scale from the matrix
    pub fn matrix_roundtrip(&mut self, name: &str) {
        let o = self.objs.get_mut(name).unwrap();
        let w = o.pose.o.matrix_world();
        let mut ob = o.pose.o;
        ob.set_matrix_world(&w);
        o.pose = Pose::from_o32(ob);
        self.touch(name);
    }

    pub fn set_pose(&mut self, name: &str, pose: Pose) {
        self.objs.get_mut(name).unwrap().pose = pose;
        self.touch(name);
    }

    pub fn world(&self, name: &str) -> Arc<World> {
        let o = &self.objs[name];
        {
            let c = self.caches.borrow();
            if let Some((v, w)) = c.world.get(name) {
                if *v == o.version {
                    return w.clone();
                }
            }
        }
        let tris: Vec<[V3; 3]> = o.geom.tris.iter().map(|t| world_tri(&o.pose, t)).collect();
        let areas: Vec<f64> = tris.iter().map(tri_area).collect();
        let total: f64 = areas.iter().sum();
        let mut c = [0.0; 3];
        for (t, a) in tris.iter().zip(&areas) {
            for k in 0..3 {
                c[k] += a * (t[0][k] + t[1][k] + t[2][k]) / 3.0;
            }
        }
        if total > 0.0 {
            for k in 0..3 {
                c[k] /= total;
            }
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in &tris {
            for v in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let w = Arc::new(World { tris, areas, centroid: c, lo, hi });
        self.caches.borrow_mut().world.insert(name.to_string(), (o.version, w.clone()));
        w
    }

    /// get_tagged_planes: indices of representative triangles
    pub fn planes(&self, name: &str, tags: &TagSet) -> Arc<Vec<usize>> {
        let (pos, neg) = tag_masks(tags);
        let o = &self.objs[name];
        let key = (name.to_string(), pos, neg);
        {
            let c = self.caches.borrow();
            if let Some((v, p)) = c.planes.get(&key) {
                if *v == o.version {
                    return p.clone();
                }
            }
        }
        let p = Arc::new(unique_planes(&o.geom, &o.pose, pos, neg));
        if std::env::var("PLANEDUMP").map_or(false, |v| name.contains(&v)) && o.geom.tris.len() > 100 {
            let mask: Vec<usize> = o.geom.tris.iter().enumerate().filter(|(_, t)| t.tags & pos == pos && t.tags & neg == 0).map(|(i, _)| i).collect();
            eprintln!("PLANEJSON {}", serde_json::json!({"name": name, "mask": mask, "planes": *p, "desc": super::eval::obj_desc(o)}));
        }
        if std::env::var("PLANEDBG").is_ok() && name.contains("living") { eprintln!("PLANES {name} pos={pos} neg={neg} -> {:?}", p); }
        self.caches.borrow_mut().planes.insert(key, (o.version, p.clone()));
        p
    }

    /// world-space normal & a point of plane `ti` of object `name`
    pub fn plane_nd(&self, name: &str, ti: usize) -> (V3, V3) {
        let w = self.world(name);
        let t = &w.tris[ti];
        (unit(cross(sub(t[1], t[0]), sub(t[2], t[0]))), t[0])
    }

    pub fn dims(&self, name: &str) -> V3 {
        self.objs[name].geom.dims()
    }

    pub fn domain_contains(&self, dom: &Domain, o: &ObjState) -> bool {
        if !satisfies(&o.tags, &dom.tags) {
            return false;
        }
        for (rel, d) in &dom.relations {
            if let Relation::Negated(inner) = rel {
                if o.relations.iter().any(|rs| rs.relation.intersects(inner, false) && self.domain_contains(d, &self.objs[&rs.target])) {
                    return false;
                }
            } else if !o.relations.iter().any(|rs| rs.relation.intersects(rel, false) && self.domain_contains(d, &self.objs[&rs.target])) {
                return false;
            }
        }
        true
    }

    pub fn objkeys_in_dom(&self, dom: &Domain) -> Vec<String> {
        self.objs.iter().filter(|(_, o)| o.active && self.domain_contains(dom, o)).map(|(k, _)| k.clone()).collect()
    }
}

pub fn dist_to_plane(p: V3, origin: V3, n: V3) -> f64 {
    dot(sub(p, origin), n).abs()
}
