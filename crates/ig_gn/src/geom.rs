//! Geometry set data model: mesh / curves / instances with typed attributes on domains.
use super::val::*;
use indexmap::IndexMap;
use crate::dhash::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dom {
    Point,
    Edge,
    Face,
    Corner,
    Curve,
    Instance,
}

impl Dom {
    pub fn from_name(s: &str) -> Dom {
        match s {
            "POINT" => Dom::Point,
            "EDGE" => Dom::Edge,
            "FACE" => Dom::Face,
            "CORNER" => Dom::Corner,
            "CURVE" | "SPLINE" => Dom::Curve,
            "INSTANCE" => Dom::Instance,
            _ => Dom::Point,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub dom: Dom,
    pub ty: Ty,
    pub data: Vec<Val>,
}

pub type Attrs = IndexMap<String, Attr>;

pub type M4 = [[f32; 4]; 4];
pub const M4_ID: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

#[derive(Clone, Default, Debug)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub edges: Vec<[u32; 2]>,
    /// face offsets (len = nfaces + 1)
    pub foff: Vec<u32>,
    pub cv: Vec<u32>,
    pub ce: Vec<u32>,
    pub attrs: Attrs,
    /// material slot names (Set Material)
    pub mats: Vec<String>,
    /// cached vertex normals set by primitives (UV sphere); dropped when positions change
    pub vnor: Option<Vec<V3>>,
}

impl Mesh {
    pub fn new() -> Mesh {
        Mesh { foff: vec![0], ..Default::default() }
    }
    pub fn nv(&self) -> usize {
        self.pos.len()
    }
    pub fn ne(&self) -> usize {
        self.edges.len()
    }
    pub fn nf(&self) -> usize {
        self.foff.len().saturating_sub(1)
    }
    pub fn nc(&self) -> usize {
        self.cv.len()
    }
    pub fn face(&self, i: usize) -> &[u32] {
        &self.cv[self.foff[i] as usize..self.foff[i + 1] as usize]
    }
    pub fn face_range(&self, i: usize) -> std::ops::Range<usize> {
        self.foff[i] as usize..self.foff[i + 1] as usize
    }
    pub fn size(&self, d: Dom) -> usize {
        match d {
            Dom::Point => self.nv(),
            Dom::Edge => self.ne(),
            Dom::Face => self.nf(),
            Dom::Corner => self.nc(),
            _ => 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }
    pub fn faces_vec(&self) -> Vec<Vec<u32>> {
        (0..self.nf()).map(|i| self.face(i).to_vec()).collect()
    }
    /// Build from faces; edges: `keep` first (in order), then new ones in corner order.
    pub fn from_faces(pos: Vec<V3>, faces: &[Vec<u32>], keep: &[[u32; 2]]) -> Mesh {
        let mut m = Mesh::new();
        m.pos = pos;
        for f in faces {
            m.cv.extend_from_slice(f);
            m.foff.push(m.cv.len() as u32);
        }
        m.edges = keep.to_vec();
        m.rebuild_corner_edges();
        m
    }
    /// (Re)compute corner edges; add any missing edges after the existing ones (BKE mesh_calc_edges order).
    pub fn rebuild_corner_edges(&mut self) {
        let mut map: HashMap<(u32, u32), u32> = HashMap::with_capacity_and_hasher(self.edges.len() * 2, Default::default());
        for (i, e) in self.edges.iter().enumerate() {
            map.entry((e[0].min(e[1]), e[0].max(e[1]))).or_insert(i as u32);
        }
        self.ce = vec![0; self.cv.len()];
        let new_edges_start = self.edges.len();
        for f in 0..self.nf() {
            let r = self.face_range(f);
            let n = r.len();
            for c in 0..n {
                let v = self.cv[r.start + c];
                let vn = self.cv[r.start + (c + 1) % n];
                let key = (v.min(vn), v.max(vn));
                let id = match map.get(&key) {
                    Some(&id) => id,
                    None => {
                        let id = self.edges.len() as u32;
                        self.edges.push([key.0, key.1]);
                        map.insert(key, id);
                        id
                    }
                };
                self.ce[r.start + c] = id;
            }
        }
        // grow edge attributes for newly added edges
        let ne = self.edges.len();
        if ne > new_edges_start {
            for a in self.attrs.values_mut() {
                if a.dom == Dom::Edge {
                    let z = Val::zero(a.ty);
                    a.data.resize(ne, z);
                }
            }
        }
    }
    /// edge->(faces) adjacency counts
    pub fn edge_face_count(&self) -> Vec<u32> {
        let mut c = vec![0u32; self.ne()];
        for &e in &self.ce {
            c[e as usize] += 1;
        }
        c
    }
    pub fn corner_face(&self) -> Vec<u32> {
        let mut o = vec![0u32; self.nc()];
        for f in 0..self.nf() {
            for c in self.face_range(f) {
                o[c] = f as u32;
            }
        }
        o
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CKind {
    Poly,
    Bezier,
    Nurbs,
    CatmullRom,
}

#[derive(Clone, Default, Debug)]
pub struct Curves {
    pub kind: Vec<CKind>,
    /// point offsets (len = ncurves + 1)
    pub off: Vec<u32>,
    pub cyclic: Vec<bool>,
    pub res: Vec<i32>,
    pub pos: Vec<V3>,
    /// handle positions (valid for all points, used by bezier splines)
    pub hl: Vec<V3>,
    pub hr: Vec<V3>,
    /// handle types per point: 0 free, 1 auto, 2 vector, 3 align (also used for left/right)
    pub htl: Vec<u8>,
    pub htr: Vec<u8>,
    pub radius: Vec<f32>,
    pub tilt: Vec<f32>,
    /// whether a `radius` attribute exists (the Radius field reads 0 otherwise; sweeps use 1)
    pub has_radius: bool,
    pub attrs: Attrs,
}

impl Curves {
    pub fn new() -> Curves {
        Curves { off: vec![0], ..Default::default() }
    }
    pub fn nc(&self) -> usize {
        self.kind.len()
    }
    pub fn np(&self) -> usize {
        self.pos.len()
    }
    pub fn range(&self, c: usize) -> std::ops::Range<usize> {
        self.off[c] as usize..self.off[c + 1] as usize
    }
    pub fn size(&self, d: Dom) -> usize {
        match d {
            Dom::Point => self.np(),
            Dom::Curve => self.nc(),
            _ => 0,
        }
    }
    /// append a spline (points with default radius 1, tilt 0)
    /// NURBS order (Blender default 4, reduced to the point count)
    pub fn nurbs_order(&self, k: usize) -> usize {
        match self.attrs.get("__nurbs_order") {
            Some(a) => a.data[k].i().max(2) as usize,
            None => 4,
        }
    }
    pub fn push_poly(&mut self, pts: &[V3], cyclic: bool) {
        self.push_kind(CKind::Poly, pts, cyclic);
    }
    pub fn push_kind(&mut self, k: CKind, pts: &[V3], cyclic: bool) {
        self.kind.push(k);
        self.cyclic.push(cyclic);
        self.res.push(12);
        for p in pts {
            self.pos.push(*p);
            self.hl.push(*p);
            self.hr.push(*p);
            self.htl.push(1);
            self.htr.push(1);
            self.radius.push(1.0);
            self.tilt.push(0.0);
        }
        self.off.push(self.pos.len() as u32);
    }
}

#[derive(Clone, Default, Debug)]
pub struct Instances {
    pub refs: Vec<Rc<Geometry>>,
    pub idx: Vec<u32>,
    pub xf: Vec<M4>,
    pub attrs: Attrs,
}

impl Instances {
    pub fn n(&self) -> usize {
        self.xf.len()
    }
}

/// point cloud (Mesh to Points, Distribute Points)
#[derive(Clone, Default, Debug)]
pub struct Points {
    pub pos: Vec<V3>,
    pub attrs: Attrs,
}

#[derive(Clone, Default, Debug)]
pub struct Geometry {
    pub mesh: Option<Mesh>,
    pub curves: Option<Curves>,
    pub inst: Option<Instances>,
    pub points: Option<Points>,
}

impl Geometry {
    pub fn from_mesh(m: Mesh) -> Geometry {
        Geometry { mesh: Some(m), ..Default::default() }
    }
    pub fn from_curves(c: Curves) -> Geometry {
        Geometry { curves: Some(c), ..Default::default() }
    }
    pub fn is_empty(&self) -> bool {
        self.mesh.as_ref().map_or(true, |m| m.is_empty())
            && self.curves.as_ref().map_or(true, |c| c.np() == 0)
            && self.inst.as_ref().map_or(true, |i| i.n() == 0)
            && self.points.as_ref().map_or(true, |p| p.pos.is_empty())
    }
}

/// Unit-matrix helpers (column-major M4 like Blender: m[col][row])
pub fn m4_mul(a: &M4, b: &M4) -> M4 {
    // result = a * b (apply b first)
    let mut r = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = b[i][0] * a[0][j] + b[i][1] * a[1][j] + b[i][2] * a[2][j] + b[i][3] * a[3][j];
        }
    }
    r
}

pub fn m4_point(m: &M4, p: V3) -> V3 {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

pub fn m4_dir(m: &M4, p: V3) -> V3 {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2],
    ]
}
