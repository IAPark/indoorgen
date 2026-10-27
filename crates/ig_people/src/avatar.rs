//! Rocketbox avatar loading: FBX (assimp) -> compact binary cache (`avatar.bin`). Body frame: metres, z up, facing +y, +x = the
//! avatar's right hand side, origin at the pelvis rest height below the pelvis (feet soles ~ z=0 in the rest pose).
#[cfg(feature = "assimp")]
use russimp::node::Node;
#[cfg(feature = "assimp")]
use russimp::scene::{PostProcess, Scene};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub type M4 = [[f64; 4]; 4];

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                r[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    r
}
pub fn ident() -> M4 {
    [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]
}
/// inverse of an affine matrix (rotation+scale part inverted by cofactors)
pub fn inv_affine(m: &M4) -> M4 {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    let id = 1.0 / det;
    let mut r = ident();
    r[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * id;
    r[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * id;
    r[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * id;
    r[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * id;
    r[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * id;
    r[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * id;
    r[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * id;
    r[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * id;
    r[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * id;
    for i in 0..3 {
        r[i][3] = -(r[i][0] * m[0][3] + r[i][1] * m[1][3] + r[i][2] * m[2][3]);
    }
    r
}
#[cfg(feature = "assimp")]
fn conv(m: &russimp::Matrix4x4) -> M4 {
    [[m.a1 as f64, m.a2 as f64, m.a3 as f64, m.a4 as f64], [m.b1 as f64, m.b2 as f64, m.b3 as f64, m.b4 as f64], [m.c1 as f64, m.c2 as f64, m.c3 as f64, m.c4 as f64], [m.d1 as f64, m.d2 as f64, m.d3 as f64, m.d4 as f64]]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Body = 0,
    Head = 1,
    Hair = 2,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    /// rest pivot (joint position) in body frame
    pub pos: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct AvPart {
    pub name: String,
    pub kind: PartKind,
    /// texture file names (relative to `<avatar>/Textures/`), empty when absent
    pub color: String,
    pub normal: String,
    pub spec: String,
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub tris: Vec<[u32; 3]>,
}

#[derive(Clone, Debug)]
pub struct Avatar {
    pub name: String,
    pub dir: PathBuf,
    pub bones: Vec<Bone>,
    pub parts: Vec<AvPart>,
}

impl Avatar {
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}

pub fn root_dir() -> PathBuf {
    // INDOORGEN_ASSETS: the rocketbox directory (ROCKETBOX_DIR is the older name); default <repo>/assets/rocketbox
    for k in ["INDOORGEN_ASSETS", "ROCKETBOX_DIR"] {
        if let Ok(p) = std::env::var(k) {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/rocketbox")
}

/// avatar names that have an fbx on disk (sorted)
pub fn list_avatars() -> Vec<String> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(root_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if e.path().join("Export").join(format!("{n}.fbx")).exists() || e.path().join("avatar.bin").exists() {
                v.push(n);
            }
        }
    }
    v.sort();
    v
}

// ---------------------------------------------------------------- FBX import
#[cfg(feature = "assimp")]
fn collect_nodes(n: &Rc<Node>, parent: Option<usize>, parent_g: &M4, out: &mut Vec<(String, Option<usize>, M4)>) {
    let g = mul(parent_g, &conv(&n.transformation));
    let idx = out.len();
    out.push((n.name.clone(), parent, g));
    for c in n.children.borrow().iter() {
        collect_nodes(c, Some(idx), &g, out);
    }
}

#[cfg(feature = "assimp")]
fn tex_name(files: &[String], key: &str, ext: &str, dir: &Path) -> String {
    for f in files {
        let base = f.rsplit(['\\', '/']).next().unwrap_or(f);
        let lo = base.to_lowercase();
        if lo.contains(key) && !lo.contains("wrinkle") {
            let stem = base.rsplit_once('.').map(|x| x.0).unwrap_or(base);
            for e in [ext, "png", "jpg"] {
                let c = format!("{stem}.{e}");
                if dir.join("Textures").join(&c).exists() {
                    return c;
                }
            }
        }
    }
    String::new()
}

#[cfg(feature = "assimp")]
pub fn import_fbx(dir: &Path, name: &str) -> Result<Avatar, String> {
    let path = dir.join("Export").join(format!("{name}.fbx"));
    let sc = Scene::from_file(path.to_str().unwrap(), vec![PostProcess::Triangulate, PostProcess::LimitBoneWeights]).map_err(|e| format!("{e:?}"))?;
    let root = sc.root.as_ref().ok_or("no root")?;
    let mut nodes = vec![];
    collect_nodes(root, None, &ident(), &mut nodes);
    let by_name: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.0.as_str(), i)).collect();
    // M: node-global -> mesh-local, from the first weighted bone
    let mut rest_world: HashMap<usize, M4> = HashMap::new();
    let mut mfix: Option<M4> = None;
    for m in &sc.meshes {
        for b in &m.bones {
            if let Some(&ni) = by_name.get(b.name.as_str()) {
                let w = inv_affine(&conv(&b.offset_matrix));
                rest_world.insert(ni, w);
                if mfix.is_none() {
                    mfix = Some(mul(&w, &inv_affine(&nodes[ni].2)));
                }
            }
        }
    }
    let mfix = mfix.ok_or("no bones")?;
    // skeleton = nodes whose name starts with "Bip01" and are not the assimp helper nodes
    let skel: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].0.starts_with("Bip01") && !nodes[i].0.contains("$AssimpFbx$") && !nodes[i].0.contains("Footsteps")).collect();
    let gidx: HashMap<usize, usize> = skel.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    // body frame from the skeleton: up = pelvis->head, right = L thigh -> R thigh
    let wp = |n: &str| -> Result<[f64; 3], String> {
        let ni = *by_name.get(n).ok_or(format!("missing node {n}"))?;
        let w = rest_world.get(&ni).copied().unwrap_or_else(|| mul(&mfix, &nodes[ni].2));
        Ok([w[0][3], w[1][3], w[2][3]])
    };
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let nrm = |a: [f64; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let up = nrm(sub(wp("Bip01 Head")?, wp("Bip01 Pelvis")?));
    let mut right = sub(wp("Bip01 R Thigh")?, wp("Bip01 L Thigh")?);
    let d = dot(right, up);
    right = nrm([right[0] - d * up[0], right[1] - d * up[1], right[2] - d * up[2]]);
    let fwd = cross(up, right);
    // facing check against the nose
    let nose = sub(wp("Bip01 MNose")?, wp("Bip01 Head")?);
    let (fwd, right) = if dot(nose, fwd) < 0.0 { ([-fwd[0], -fwd[1], -fwd[2]], [-right[0], -right[1], -right[2]]) } else { (fwd, right) };
    // rows: x=right, y=fwd, z=up (this is a proper rotation: right x fwd = up must hold)
    let c = cross(right, fwd);
    let (right, fwd) = if dot(c, up) < 0.0 { return Err("left handed skeleton".into()) } else { (right, fwd) };
    let rows = [right, fwd, up];
    let s = 0.01;
    let tp = |p: [f64; 3]| -> [f32; 3] { [(dot(rows[0], p) * s) as f32, (dot(rows[1], p) * s) as f32, (dot(rows[2], p) * s) as f32] };
    let tv = |p: [f64; 3]| -> [f32; 3] {
        let q = [dot(rows[0], p), dot(rows[1], p), dot(rows[2], p)];
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt().max(1e-12);
        [(q[0] / l) as f32, (q[1] / l) as f32, (q[2] / l) as f32]
    };
    let mut bones: Vec<Bone> = vec![];
    for &ni in &skel {
        let w = rest_world.get(&ni).copied().unwrap_or_else(|| mul(&mfix, &nodes[ni].2));
        let mut p = nodes[ni].1;
        let mut parent = -1i32;
        while let Some(pi) = p {
            if let Some(&g) = gidx.get(&pi) {
                parent = g as i32;
                break;
            }
            p = nodes[pi].1;
        }
        bones.push(Bone { name: nodes[ni].0.clone(), parent, pos: tp([w[0][3], w[1][3], w[2][3]]) });
    }
    // floor offset: feet soles at z = 0 in rest: use the lowest vertex of all meshes
    let mut zmin = f32::MAX;
    let mut parts = vec![];
    for m in &sc.meshes {
        let mat = &sc.materials[m.material_index as usize];
        let mut files: Vec<String> = vec![];
        for p in &mat.properties {
            if p.key.ends_with("file") {
                if let russimp::material::PropertyTypeInfo::String(s) = &p.data {
                    files.push(s.clone());
                }
            }
        }
        let mname = mat.properties.iter().find(|p| p.key == "?mat.name").and_then(|p| if let russimp::material::PropertyTypeInfo::String(s) = &p.data { Some(s.clone()) } else { None }).unwrap_or_default();
        let lname = mname.to_lowercase();
        let kind = if lname.contains("opacity") { PartKind::Hair } else if lname.contains("head") { PartKind::Head } else { PartKind::Body };
        let pos: Vec<[f32; 3]> = m.vertices.iter().map(|v| tp([v.x as f64, v.y as f64, v.z as f64])).collect();
        for p in &pos {
            zmin = zmin.min(p[2]);
        }
        let nrmv: Vec<[f32; 3]> = m.normals.iter().map(|v| tv([v.x as f64, v.y as f64, v.z as f64])).collect();
        let uv: Vec<[f32; 2]> = match m.texture_coords.first() {
            Some(Some(t)) => t.iter().map(|v| [v.x, v.y]).collect(),
            _ => vec![[0.0, 0.0]; pos.len()],
        };
        let mut jw: Vec<Vec<(u16, f32)>> = vec![vec![]; pos.len()];
        for b in &m.bones {
            let Some(&ni) = by_name.get(b.name.as_str()) else { continue };
            let Some(&g) = gidx.get(&ni) else { continue };
            for w in &b.weights {
                if w.weight > 1e-4 {
                    jw[w.vertex_id as usize].push((g as u16, w.weight));
                }
            }
        }
        let mut joints = vec![[0u16; 4]; pos.len()];
        let mut weights = vec![[0f32; 4]; pos.len()];
        for (i, l) in jw.iter_mut().enumerate() {
            l.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            l.truncate(4);
            let sum: f32 = l.iter().map(|x| x.1).sum();
            for (k, (j, w)) in l.iter().enumerate() {
                joints[i][k] = *j;
                weights[i][k] = *w / sum.max(1e-9);
            }
        }
        let tris: Vec<[u32; 3]> = m.faces.iter().filter(|f| f.0.len() == 3).map(|f| [f.0[0], f.0[1], f.0[2]]).collect();
        parts.push(AvPart {
            name: mname,
            kind,
            color: tex_name(&files, "color", if kind == PartKind::Hair { "png" } else { "jpg" }, dir),
            normal: tex_name(&files, "normal", "png", dir),
            spec: tex_name(&files, "specular", "jpg", dir),
            pos,
            nrm: nrmv,
            uv,
            joints,
            weights,
            tris,
        });
    }
    // shift so the lowest vertex is at z = 0
    for b in bones.iter_mut() {
        b.pos[2] -= zmin;
    }
    for p in parts.iter_mut() {
        for v in p.pos.iter_mut() {
            v[2] -= zmin;
        }
    }
    Ok(Avatar { name: name.to_string(), dir: dir.to_path_buf(), bones, parts })
}

// ---------------------------------------------------------------- binary cache
const MAGIC: &[u8; 8] = b"IGAVAT02";

struct W(Vec<u8>);
impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn s(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
}
struct R<'a>(&'a [u8], usize);
impl<'a> R<'a> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.0[self.1..self.1 + 4].try_into().unwrap());
        self.1 += 4;
        v
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    fn s(&mut self) -> String {
        let n = self.u32() as usize;
        let s = String::from_utf8_lossy(&self.0[self.1..self.1 + n]).to_string();
        self.1 += n;
        s
    }
}

pub fn save_bin(a: &Avatar, path: &Path) -> std::io::Result<()> {
    let mut w = W(MAGIC.to_vec());
    w.u32(a.bones.len() as u32);
    for b in &a.bones {
        w.s(&b.name);
        w.u32(b.parent as u32);
        w.f32s(&b.pos);
    }
    w.u32(a.parts.len() as u32);
    for p in &a.parts {
        w.s(&p.name);
        w.u32(p.kind as u32);
        w.s(&p.color);
        w.s(&p.normal);
        w.s(&p.spec);
        w.u32(p.pos.len() as u32);
        for i in 0..p.pos.len() {
            w.f32s(&p.pos[i]);
            w.f32s(&p.nrm[i]);
            w.f32s(&p.uv[i]);
            for k in 0..4 {
                w.u32(p.joints[i][k] as u32);
            }
            w.f32s(&p.weights[i]);
        }
        w.u32(p.tris.len() as u32);
        for t in &p.tris {
            for k in 0..3 {
                w.u32(t[k]);
            }
        }
    }
    std::fs::write(path, w.0)
}

pub fn load_bin(dir: &Path, name: &str) -> Option<Avatar> {
    let data = std::fs::read(dir.join("avatar.bin")).ok()?;
    if data.len() < 8 || &data[..8] != MAGIC {
        return None;
    }
    let mut r = R(&data, 8);
    let nb = r.u32() as usize;
    let mut bones = vec![];
    for _ in 0..nb {
        let name = r.s();
        let parent = r.u32() as i32;
        let pos = [r.f32(), r.f32(), r.f32()];
        bones.push(Bone { name, parent, pos });
    }
    let np = r.u32() as usize;
    let mut parts = vec![];
    for _ in 0..np {
        let pname = r.s();
        let kind = match r.u32() {
            1 => PartKind::Head,
            2 => PartKind::Hair,
            _ => PartKind::Body,
        };
        let (color, normal, spec) = (r.s(), r.s(), r.s());
        let nv = r.u32() as usize;
        let (mut pos, mut nrm, mut uv, mut joints, mut weights) = (Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv));
        for _ in 0..nv {
            pos.push([r.f32(), r.f32(), r.f32()]);
            nrm.push([r.f32(), r.f32(), r.f32()]);
            uv.push([r.f32(), r.f32()]);
            joints.push([r.u32() as u16, r.u32() as u16, r.u32() as u16, r.u32() as u16]);
            weights.push([r.f32(), r.f32(), r.f32(), r.f32()]);
        }
        let nt = r.u32() as usize;
        let tris = (0..nt).map(|_| [r.u32(), r.u32(), r.u32()]).collect();
        parts.push(AvPart { name: pname, kind, color, normal, spec, pos, nrm, uv, joints, weights, tris });
    }
    Some(Avatar { name: name.to_string(), dir: dir.to_path_buf(), bones, parts })
}

#[cfg(not(feature = "assimp"))]
pub fn import_fbx(_dir: &Path, _name: &str) -> Result<Avatar, String> {
    Err("built without the assimp feature (no FBX import)".into())
}

/// load from the cache, importing the FBX (and writing the cache) on a miss
pub fn load(name: &str) -> Option<Avatar> {
    let dir = root_dir().join(name);
    if let Some(a) = load_bin(&dir, name) {
        return Some(a);
    }
    let a = import_fbx(&dir, name).map_err(|e| eprintln!("avatar {name}: {e}")).ok()?;
    let _ = save_bin(&a, &dir.join("avatar.bin"));
    Some(a)
}
