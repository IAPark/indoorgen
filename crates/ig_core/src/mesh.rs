//! Indexed polygon-mesh toolkit (quads/n-gons kept as faces) + scene container + glTF writer.
use serde_json::json;

pub type V3 = [f64; 3];

/// Indexed polygon mesh. Each face is a CCW polygon (len >= 3) of indices into `pos`.
/// `uv`, when non-empty and the same length as `pos`, is per-vertex.
#[derive(Clone, Default, Debug)]
pub struct Mesh {
    pub pos: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
    pub uv: Vec<[f64; 2]>,
}

impl Mesh {
    /// Catmull-Clark smoothing; `bevel` > 0 first bevels sharp edges (width, segments, 30 degree limit) like Blender's BEVEL then SUBSURF.
    pub fn smoothed(&self, bevel_w: f64, segments: usize, levels: usize) -> Mesh {
        use crate::subdiv::*;
        if self.faces.is_empty() {
            return self.clone();
        }
        let q = QMesh { pos: self.pos.clone(), faces: self.faces.clone() }.weld(1e-6);
        let out = if bevel_w > 0.0 { bevel_then_subsurf(&q, bevel_w, segments.max(1), 30f64.to_radians(), levels) } else { catmull_clark(&q, levels, None) };
        if out.pos.iter().any(|p| !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite()) || out.faces.is_empty() {
            return self.clone();
        }
        // re-expand to per-face vertices so the exporter's smooth-by-angle normals work as before
        let mut m = Mesh::new();
        for f in &out.faces {
            let pts: Vec<V3> = f.iter().map(|&i| out.pos[i as usize]).collect();
            m.add_face(&pts);
        }
        m
    }
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_tri(&mut self, a: V3, b: V3, c: V3) {
        self.add_face(&[a, b, c]);
    }
    /// quad with CCW winding (a,b,c,d) => outward normal by right-hand rule. One 4-vertex face (4 new positions).
    pub fn add_quad(&mut self, a: V3, b: V3, c: V3, d: V3) {
        self.add_face(&[a, b, c, d]);
    }
    /// Add a polygon face with fresh vertices (no welding).
    pub fn add_face(&mut self, pts: &[V3]) {
        assert!(pts.len() >= 3, "face needs >= 3 vertices");
        let i = self.pos.len() as u32;
        self.pos.extend_from_slice(pts);
        self.faces.push((i..i + pts.len() as u32).collect());
    }
    /// Add a face referencing existing vertices.
    pub fn add_face_idx(&mut self, ids: &[u32]) {
        assert!(ids.len() >= 3, "face needs >= 3 vertices");
        self.faces.push(ids.to_vec());
    }
    /// Fan-triangulate every face: (0,k,k+1). Quads give (0,1,2),(0,2,3).
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        let mut out = Vec::with_capacity(self.faces.len() * 2);
        for f in &self.faces {
            for k in 1..f.len() - 1 {
                out.push([f[0], f[k], f[k + 1]]);
            }
        }
        out
    }
    pub fn append(&mut self, o: &Mesh) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&o.pos);
        self.faces.extend(o.faces.iter().map(|f| f.iter().map(|i| i + base).collect::<Vec<u32>>()));
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in self.pos.iter_mut() {
            *p = m.apply(*p);
        }
        if m.det() < 0.0 {
            for f in self.faces.iter_mut() {
                f[1..].reverse();
            }
        }
    }
    pub fn flipped(mut self) -> Mesh {
        for f in self.faces.iter_mut() {
            f[1..].reverse();
        }
        self
    }
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &self.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
    pub fn area(&self) -> f64 {
        self.triangles()
            .iter()
            .map(|t| {
                let (a, b, c) = (self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]);
                let n = cross(sub(b, a), sub(c, a));
                0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
            })
            .sum()
    }

    /// Merge vertices closer than ~eps (hash by rounded position), remap faces, drop degenerate faces
    /// (consecutive duplicate vertices are collapsed first; faces left with < 3 distinct vertices are removed).
    /// Per-vertex uv is kept from the first vertex of each merged group (if present).
    pub fn weld(&mut self, eps: f64) {
        let eps = eps.max(1e-12);
        let key = |p: V3| ((p[0] / eps).round() as i64, (p[1] / eps).round() as i64, (p[2] / eps).round() as i64);
        let has_uv = self.uv.len() == self.pos.len();
        let mut map: crate::dhash::HashMap<(i64, i64, i64), u32> = crate::dhash::HashMap::default();
        let mut remap = vec![0u32; self.pos.len()];
        let mut npos: Vec<V3> = vec![];
        let mut nuv: Vec<[f64; 2]> = vec![];
        for (i, p) in self.pos.iter().enumerate() {
            let id = *map.entry(key(*p)).or_insert_with(|| {
                npos.push(*p);
                if has_uv {
                    nuv.push(self.uv[i]);
                }
                (npos.len() - 1) as u32
            });
            remap[i] = id;
        }
        let mut nfaces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut g: Vec<u32> = Vec::with_capacity(f.len());
            for &v in f {
                let r = remap[v as usize];
                if g.last() != Some(&r) {
                    g.push(r);
                }
            }
            while g.len() > 1 && g.first() == g.last() {
                g.pop();
            }
            if g.len() >= 3 {
                nfaces.push(g);
            }
        }
        self.pos = npos;
        self.uv = nuv;
        self.faces = nfaces;
    }
    pub fn welded(&self, eps: f64) -> Mesh {
        let mut m = self.clone();
        m.weld(eps);
        m
    }
    /// Unique undirected edges (sorted index pairs) of the welded mesh (eps 1e-6), sorted.
    pub fn to_quadmesh_edges(&self) -> Vec<[u32; 2]> {
        let w = self.welded(1e-6);
        let mut set = std::collections::BTreeSet::new();
        for f in &w.faces {
            for k in 0..f.len() {
                let (a, b) = (f[k], f[(k + 1) % f.len()]);
                if a != b {
                    set.insert([a.min(b), a.max(b)]);
                }
            }
        }
        set.into_iter().collect()
    }
    /// Export form: (positions, uv, triangles) where quads/n-gons are fan-triangulated with the fan's shared
    /// vertices duplicated for k >= 2 (matching the old triangle-soup layout, so per-triangle normals/uvs are identical).
    fn export_tris(&self) -> (Vec<V3>, Vec<[f64; 2]>, Vec<[u32; 3]>) {
        // Full triangle soup: every triangle owns its 3 vertices. The exporter computes smooth-by-angle normals per vertex
        // (welding by position), so a vertex index shared by faces at a sharp corner would get the normal of whichever triangle was
        // processed last (dark / flipped-looking triangles on flat panels of indexed meshes such as the exact KitchenSpace slab).
        let has_uv = self.uv.len() == self.pos.len();
        let mut pos: Vec<V3> = Vec::with_capacity(self.faces.len() * 6);
        let mut uv: Vec<[f64; 2]> = vec![];
        let mut tris = Vec::with_capacity(self.faces.len() * 2);
        for f in &self.faces {
            for k in 1..f.len() - 1 {
                let n = pos.len() as u32;
                for &i in &[f[0], f[k], f[k + 1]] {
                    pos.push(self.pos[i as usize]);
                    if has_uv {
                        uv.push(self.uv[i as usize]);
                    }
                }
                tris.push([n, n + 1, n + 2]);
            }
        }
        (pos, uv, tris)
    }
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Row-major 4x4 affine transform.
#[derive(Clone, Copy, Debug)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub fn identity() -> Self {
        Mat4([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]])
    }
    pub fn translation(t: V3) -> Self {
        let mut m = Self::identity();
        m.0[0][3] = t[0];
        m.0[1][3] = t[1];
        m.0[2][3] = t[2];
        m
    }
    pub fn scale(s: V3) -> Self {
        let mut m = Self::identity();
        m.0[0][0] = s[0];
        m.0[1][1] = s[1];
        m.0[2][2] = s[2];
        m
    }
    pub fn rot_z(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[c, -s, 0., 0.], [s, c, 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]])
    }
    pub fn rot_x(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[1., 0., 0., 0.], [0., c, -s, 0.], [0., s, c, 0.], [0., 0., 0., 1.]])
    }
    pub fn rot_y(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Mat4([[c, 0., s, 0.], [0., 1., 0., 0.], [-s, 0., c, 0.], [0., 0., 0., 1.]])
    }
    /// Blender XYZ euler: R = Rz * Ry * Rx
    pub fn euler_xyz(r: V3) -> Self {
        Self::rot_z(r[2]).mul(&Self::rot_y(r[1])).mul(&Self::rot_x(r[0]))
    }
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    r[i][j] += self.0[i][k] * o.0[k][j];
                }
            }
        }
        Mat4(r)
    }
    pub fn apply(&self, p: V3) -> V3 {
        let m = &self.0;
        [
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
            m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
        ]
    }
    pub fn det(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

#[derive(Clone, Debug)]
pub struct Material {
    /// optional procedural-material spec resolved on the Blender side, e.g. {"cls":"ceramic.Plaster","kwargs":{}}
    pub spec: Option<serde_json::Value>,
    pub name: String,
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: [f32; 3],
    pub emissive_strength: f32,
}

impl Material {
    pub fn diffuse(name: &str, rgb: [f32; 3], roughness: f32) -> Self {
        Material {
            spec: None,
            name: name.into(),
            color: [rgb[0], rgb[1], rgb[2], 1.0],
            roughness,
            metallic: 0.0,
            emissive: [0.0; 3],
            emissive_strength: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub mesh: Mesh,
    pub material: usize,
    /// free-form tag for reporting, e.g. generator class name
    pub class: String,
}

#[derive(Clone, Debug)]
pub struct Light {
    pub loc: V3,
    pub watt: f64,
    pub radius: f64,
    pub temperature: f64,
    pub name: String,
}

#[derive(Default, Clone)]
pub struct Scene {
    pub materials: Vec<Material>,
    pub nodes: Vec<Node>,
    pub lights: Vec<Light>,
    /// cloth jobs for a Blender re-simulation (sidecar `cloth`)
    pub cloth: Vec<serde_json::Value>,
}

impl Scene {
    pub fn material(&mut self, m: Material) -> usize {
        if let Some(i) = self.materials.iter().position(|x| x.name == m.name) {
            return i;
        }
        self.materials.push(m);
        self.materials.len() - 1
    }
    pub fn add(&mut self, name: &str, class: &str, mesh: Mesh, material: usize) {
        if !mesh.is_empty() {
            self.nodes.push(Node { name: name.into(), mesh, material, class: class.into() });
        }
    }

    /// Write a binary glTF 2.0 (.glb). Z-up scene is converted to glTF's Y-up with a root rotation node.
    pub fn write_glb(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut bin: Vec<u8> = vec![];
        let mut buffer_views = vec![];
        let mut accessors = vec![];
        let mut meshes = vec![];
        let mut nodes = vec![];
        let mut child_ids = vec![];
        for (ni, n) in self.nodes.iter().enumerate() {
            let (xpos, xuv, xtris) = n.mesh.export_tris();
            let m = &Mesh { pos: xpos, faces: vec![], uv: xuv };
            let tris = &xtris;
            // positions
            let pstart = bin.len();
            let (lo, hi) = m.bounds();
            for p in &m.pos {
                for k in 0..3 {
                    bin.extend_from_slice(&(p[k] as f32).to_le_bytes());
                }
            }
            buffer_views.push(json!({"buffer":0,"byteOffset":pstart,"byteLength":bin.len()-pstart,"target":34962}));
            accessors.push(json!({"bufferView":buffer_views.len()-1,"componentType":5126,"count":m.pos.len(),"type":"VEC3",
                "min":[lo[0],lo[1],lo[2]],"max":[hi[0],hi[1],hi[2]]}));
            let pacc = accessors.len() - 1;
            // smooth-by-angle normals: weld by position, average face normals within 40 degrees of the vertex's own face
            let mut normals = vec![[0f32; 3]; m.pos.len()];
            {
                let key = |p: V3| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64);
                let fnorm: Vec<V3> = tris.iter().map(|t| cross(sub(m.pos[t[1] as usize], m.pos[t[0] as usize]), sub(m.pos[t[2] as usize], m.pos[t[0] as usize]))).collect();
                let mut groups: crate::dhash::HashMap<(i64, i64, i64), Vec<usize>> = crate::dhash::HashMap::default();
                for (ti, t) in tris.iter().enumerate() {
                    for &v in t {
                        groups.entry(key(m.pos[v as usize])).or_default().push(ti);
                    }
                }
                let cos_t = 40f64.to_radians().cos();
                for (ti, t) in tris.iter().enumerate() {
                    let nl = (fnorm[ti][0].powi(2) + fnorm[ti][1].powi(2) + fnorm[ti][2].powi(2)).sqrt().max(1e-18);
                    let nu = [fnorm[ti][0] / nl, fnorm[ti][1] / nl, fnorm[ti][2] / nl];
                    for &v in t {
                        let mut acc = [0f64; 3];
                        for &si in &groups[&key(m.pos[v as usize])] {
                            let sl = (fnorm[si][0].powi(2) + fnorm[si][1].powi(2) + fnorm[si][2].powi(2)).sqrt().max(1e-18);
                            let d = (fnorm[si][0] * nu[0] + fnorm[si][1] * nu[1] + fnorm[si][2] * nu[2]) / sl;
                            if d >= cos_t {
                                for k in 0..3 {
                                    acc[k] += fnorm[si][k];
                                }
                            }
                        }
                        for k in 0..3 {
                            normals[v as usize][k] = acc[k] as f32;
                        }
                    }
                }
            }
            let nstart = bin.len();
            for nv in &normals {
                let l = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt().max(1e-12);
                for k in 0..3 {
                    bin.extend_from_slice(&(nv[k] / l).to_le_bytes());
                }
            }
            buffer_views.push(json!({"buffer":0,"byteOffset":nstart,"byteLength":bin.len()-nstart,"target":34962}));
            accessors.push(json!({"bufferView":buffer_views.len()-1,"componentType":5126,"count":m.pos.len(),"type":"VEC3"}));
            let nacc = accessors.len() - 1;
            let mut attrs = json!({"POSITION":pacc,"NORMAL":nacc});
            let generated_uv: Vec<[f64; 2]>;
            let uvs: &Vec<[f64; 2]> = if m.uv.len() == m.pos.len() {
                &m.uv
            } else {
                // box projection per triangle (1 uv unit = 1 metre)
                let mut uv = vec![[0.0, 0.0]; m.pos.len()];
                for t in tris {
                    let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                    let n = cross(sub(b, a), sub(c, a));
                    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
                    for &i in t {
                        let p = m.pos[i as usize];
                        uv[i as usize] = if az >= ax && az >= ay { [p[0], p[1]] } else if ax >= ay { [p[1], p[2]] } else { [p[0], p[2]] };
                    }
                }
                generated_uv = uv;
                &generated_uv
            };
            if uvs.len() == m.pos.len() {
                let ustart = bin.len();
                for u in uvs {
                    bin.extend_from_slice(&(u[0] as f32).to_le_bytes());
                    bin.extend_from_slice(&(u[1] as f32).to_le_bytes());
                }
                buffer_views.push(json!({"buffer":0,"byteOffset":ustart,"byteLength":bin.len()-ustart,"target":34962}));
                accessors.push(json!({"bufferView":buffer_views.len()-1,"componentType":5126,"count":m.pos.len(),"type":"VEC2"}));
                attrs["TEXCOORD_0"] = json!(accessors.len() - 1);
            }
            let istart = bin.len();
            for t in tris {
                for &i in t {
                    bin.extend_from_slice(&i.to_le_bytes());
                }
            }
            buffer_views.push(json!({"buffer":0,"byteOffset":istart,"byteLength":bin.len()-istart,"target":34963}));
            accessors.push(json!({"bufferView":buffer_views.len()-1,"componentType":5125,"count":tris.len()*3,"type":"SCALAR"}));
            let iacc = accessors.len() - 1;
            meshes.push(json!({"name":n.name,"primitives":[{"attributes":attrs,"indices":iacc,"material":n.material}]}));
            nodes.push(json!({"name":n.name,"mesh":ni,"extras":{"class":n.class}}));
            child_ids.push(ni);
        }
        let root = nodes.len();
        // Z-up -> Y-up: rotate -90deg about X
        let s = std::f64::consts::FRAC_1_SQRT_2;
        nodes.push(json!({"name":"root","children":child_ids,"rotation":[-s,0.0,0.0,s]}));
        let materials: Vec<_> = self
            .materials
            .iter()
            .map(|m| {
                let mut j = json!({"name":m.name,
                    "pbrMetallicRoughness":{"baseColorFactor":m.color,"metallicFactor":m.metallic,"roughnessFactor":m.roughness},
                    "doubleSided": true});
                if m.emissive_strength > 0.0 {
                    j["emissiveFactor"] = json!(m.emissive);
                    j["extensions"] = json!({"KHR_materials_emissive_strength":{"emissiveStrength":m.emissive_strength}});
                }
                j
            })
            .collect();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let nodes_json: Vec<_> = self.nodes.iter().map(|n| json!({"name": n.name, "class": n.class, "material": self.materials[n.material].name, "spec": self.materials[n.material].spec})).collect();
        let lights_json: Vec<_> = self.lights.iter().map(|l| json!({"name": l.name, "loc": l.loc, "watt": l.watt, "radius": l.radius, "temperature": l.temperature})).collect();
        let sidecar = json!({"nodes": nodes_json, "lights": lights_json, "cloth": self.cloth});
        if let Some(sp) = path.parent() {
            let _ = std::fs::write(sp.join(format!("{}.json", path.file_stem().unwrap().to_string_lossy())), serde_json::to_vec_pretty(&sidecar).unwrap());
        }
        let gltf = json!({
            "asset":{"version":"2.0","generator":"infinigen-rs"},
            "extensionsUsed":["KHR_materials_emissive_strength"],
            "scene":0,"scenes":[{"nodes":[root]}],
            "nodes":nodes,"meshes":meshes,"materials":materials,
            "accessors":accessors,"bufferViews":buffer_views,
            "buffers":[{"byteLength":bin.len()}]
        });
        let mut js = serde_json::to_vec(&gltf).unwrap();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        std::fs::write(path, out)
    }
}

impl Mesh {
    pub fn cuboid(lo: V3, hi: V3) -> Mesh {
        let mut m = Mesh::new();
        let c = |x: usize, y: usize, z: usize| [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }];
        m.add_quad(c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0));
        m.add_quad(c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1));
        m.add_quad(c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1));
        m.add_quad(c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0));
        m.add_quad(c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0));
        m.add_quad(c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1));
        m
    }

    /// vertical cylinder (axis z) with optional different top radius (cone frustum), capped
    pub fn frustum(cx: f64, cy: f64, z0: f64, z1: f64, r0: f64, r1: f64, n: usize) -> Mesh {
        let mut m = Mesh::new();
        let pt = |i: usize, r: f64, z: f64| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), z]
        };
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_quad(pt(i, r0, z0), pt(j, r0, z0), pt(j, r1, z1), pt(i, r1, z1));
            if r1 > 0.0 {
                m.add_tri([cx, cy, z1], pt(i, r1, z1), pt(j, r1, z1));
            }
            if r0 > 0.0 {
                m.add_tri([cx, cy, z0], pt(j, r0, z0), pt(i, r0, z0));
            }
        }
        m
    }

    pub fn cylinder(cx: f64, cy: f64, z0: f64, z1: f64, r: f64, n: usize) -> Mesh {
        Mesh::frustum(cx, cy, z0, z1, r, r, n)
    }

    /// UV sphere / ellipsoid centred at c with radii r
    pub fn ellipsoid(c: V3, r: V3, nu: usize, nv: usize) -> Mesh {
        let mut m = Mesh::new();
        let p = |i: usize, j: usize| {
            let th = std::f64::consts::PI * j as f64 / nv as f64;
            let ph = 2.0 * std::f64::consts::PI * i as f64 / nu as f64;
            [c[0] + r[0] * th.sin() * ph.cos(), c[1] + r[1] * th.sin() * ph.sin(), c[2] + r[2] * th.cos()]
        };
        for j in 0..nv {
            for i in 0..nu {
                let i2 = (i + 1) % nu;
                m.add_quad(p(i, j), p(i, j + 1), p(i2, j + 1), p(i2, j));
            }
        }
        m
    }
}

impl Mesh {
    /// Surface of revolution around the z axis through (cx, cy): `profile` is (radius, z) from bottom to top, n segments.
    pub fn revolve(cx: f64, cy: f64, profile: &[(f64, f64)], n: usize) -> Mesh {
        let mut m = Mesh::new();
        let p = |i: usize, k: usize| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + profile[k].0 * a.cos(), cy + profile[k].0 * a.sin(), profile[k].1]
        };
        for k in 0..profile.len() - 1 {
            for i in 0..n {
                let j = (i + 1) % n;
                m.add_quad(p(i, k), p(j, k), p(j, k + 1), p(i, k + 1));
            }
        }
        m
    }

    /// Thick-walled vessel: outer profile + inner profile (offset inward by `t`), open at the top, closed at the bottom.
    pub fn vessel(cx: f64, cy: f64, outer: &[(f64, f64)], t: f64, n: usize) -> Mesh {
        let mut m = Mesh::revolve(cx, cy, outer, n);
        let top = outer.len() - 1;
        let mut inner: Vec<(f64, f64)> = outer.iter().map(|&(r, z)| ((r - t).max(0.0), z)).collect();
        inner[0].1 = outer[0].1 + t;
        // inner surface faces inward: reverse order so normals point into the cavity
        let rev = Mesh::revolve(cx, cy, &inner, n).flipped();
        m.append(&rev);
        // rim ring
        let ring = |i: usize, r: f64, z: f64| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), z]
        };
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_quad(ring(i, inner[top].0, outer[top].1), ring(j, inner[top].0, outer[top].1), ring(j, outer[top].0, outer[top].1), ring(i, outer[top].0, outer[top].1));
        }
        // bottom disk
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_tri([cx, cy, outer[0].1], ring(j, outer[0].0, outer[0].1), ring(i, outer[0].0, outer[0].1));
        }
        m
    }

    /// Two-sided ribbon along a polyline `spine` with per-point half width `w` and a fixed side direction `side`.
    pub fn ribbon(spine: &[V3], w: &[f64], side: &[V3]) -> Mesh {
        let mut m = Mesh::new();
        for k in 0..spine.len() - 1 {
            let a0 = [spine[k][0] - side[k][0] * w[k], spine[k][1] - side[k][1] * w[k], spine[k][2] - side[k][2] * w[k]];
            let a1 = [spine[k][0] + side[k][0] * w[k], spine[k][1] + side[k][1] * w[k], spine[k][2] + side[k][2] * w[k]];
            let b0 = [spine[k + 1][0] - side[k + 1][0] * w[k + 1], spine[k + 1][1] - side[k + 1][1] * w[k + 1], spine[k + 1][2] - side[k + 1][2] * w[k + 1]];
            let b1 = [spine[k + 1][0] + side[k + 1][0] * w[k + 1], spine[k + 1][1] + side[k + 1][1] * w[k + 1], spine[k + 1][2] + side[k + 1][2] * w[k + 1]];
            m.add_quad(a0, a1, b1, b0);
            m.add_quad(a0, b0, b1, a1);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_is_one_face_and_triangulates_like_before() {
        let mut m = Mesh::new();
        m.add_quad([0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]);
        assert_eq!(m.pos.len(), 4);
        assert_eq!(m.faces, vec![vec![0, 1, 2, 3]]);
        assert_eq!(m.triangles(), vec![[0, 1, 2], [0, 2, 3]]);
        m.add_tri([0., 0., 1.], [1., 0., 1.], [0., 1., 1.]);
        assert_eq!(m.triangles().len(), 3);
        assert!((m.area() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn weld_cube() {
        let m = Mesh::cuboid([0., 0., 0.], [1., 1., 1.]);
        assert_eq!(m.pos.len(), 24);
        let w = m.welded(1e-6);
        assert_eq!(w.pos.len(), 8);
        assert_eq!(w.faces.len(), 6);
        assert_eq!(w.to_quadmesh_edges().len(), 12);
        assert_eq!(m.triangles().len(), 12);
    }

    #[test]
    fn weld_drops_degenerate() {
        let mut m = Mesh::new();
        m.add_tri([0., 0., 0.], [0., 0., 0.], [1., 0., 0.]);
        m.add_quad([0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1e-9]);
        m.weld(1e-6);
        assert_eq!(m.faces.len(), 1);
        assert_eq!(m.faces[0].len(), 3);
    }
}
