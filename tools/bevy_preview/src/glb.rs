//! Minimal glb reader for infinigen-rs scenes (same layout as tools/preview/src/glb.rs): nodes -> meshes -> primitives
//! (world-space, Z-up mesh data under a single rotated root node), materials. Keeps the whole file in memory; views are
//! absolute byte ranges into it.
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct View { pub off: usize, pub len: usize }

pub struct Prim {
    pub pos: View, pub nrm: Option<View>, pub uv: Option<View>, pub idx: View,
    pub idx_u16: bool, pub icount: u32, pub vcount: u32, pub mat: usize,
    pub bmin: [f32; 3], pub bmax: [f32; 3],
}
pub struct Node { pub name: String, pub prims: Vec<Prim> }
/// `tex`: embedded base-colour image bytes (PNG/JPEG) when the material has a baseColorTexture; `mask`: alphaMode MASK cutoff
pub struct Mat {
    pub name: String, pub color: [f32; 4], pub emissive: [f32; 3], pub tex: Option<Arc<Vec<u8>>>, pub mask: Option<f32>,
    pub blend: bool, pub metallic: f32, pub roughness: f32,
}
pub struct Glb { pub nodes: Vec<Node>, pub mats: Vec<Mat>, pub data: Vec<u8> }

fn f3(v: &Value) -> [f32; 3] {
    let a = v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>()).unwrap_or(vec![0.0; 3]);
    [a[0], a[1], a[2]]
}

impl Glb {
    pub fn bytes(&self, v: &View) -> &[u8] { &self.data[v.off..v.off + v.len] }
}

pub fn load(path: &Path) -> Glb {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(&data[0..4] == b"glTF", "not a glb");
    let jl = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let j: Value = serde_json::from_slice(&data[20..20 + jl]).expect("glb json");
    let bin = 20 + jl + 8;
    let views = j["bufferViews"].as_array().cloned().unwrap_or_default();
    let vview = |i: usize| View { off: bin + views[i]["byteOffset"].as_u64().unwrap_or(0) as usize, len: views[i]["byteLength"].as_u64().unwrap() as usize };
    let acc = j["accessors"].as_array().cloned().unwrap_or_default();
    let aview = |a: usize| -> View {
        let ac = &acc[a];
        let v = vview(ac["bufferView"].as_u64().unwrap() as usize);
        let ao = ac["byteOffset"].as_u64().unwrap_or(0) as usize;
        View { off: v.off + ao, len: v.len - ao }
    };
    let imgs: Vec<Option<Arc<Vec<u8>>>> = j["images"].as_array().map(|a| a.iter().map(|im| {
        let v = vview(im["bufferView"].as_u64()? as usize);
        Some(Arc::new(data[v.off..v.off + v.len].to_vec()))
    }).collect()).unwrap_or_default();
    let tex_img = |ti: u64| -> Option<Arc<Vec<u8>>> { let src = j["textures"][ti as usize]["source"].as_u64()? as usize; imgs.get(src).cloned().flatten() };
    let mats: Vec<Mat> = j["materials"].as_array().map(|m| m.iter().map(|m| {
        let pbr = &m["pbrMetallicRoughness"];
        let c = pbr["baseColorFactor"].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect::<Vec<_>>()).unwrap_or(vec![0.8, 0.8, 0.8, 1.0]);
        let es = m["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"].as_f64().unwrap_or(1.0) as f32;
        let e = if m["emissiveFactor"].is_array() { f3(&m["emissiveFactor"]) } else { [0.0; 3] };
        let tex = pbr["baseColorTexture"]["index"].as_u64().and_then(tex_img);
        let mode = m["alphaMode"].as_str().unwrap_or("OPAQUE");
        Mat {
            name: m["name"].as_str().unwrap_or("").to_string(), color: [c[0], c[1], c[2], c[3]], emissive: [e[0] * es, e[1] * es, e[2] * es], tex,
            mask: if mode == "MASK" { Some(m["alphaCutoff"].as_f64().unwrap_or(0.5) as f32) } else { None }, blend: mode == "BLEND",
            metallic: pbr["metallicFactor"].as_f64().unwrap_or(1.0) as f32, roughness: pbr["roughnessFactor"].as_f64().unwrap_or(1.0) as f32,
        }
    }).collect()).unwrap_or_default();
    let meshes = j["meshes"].as_array().cloned().unwrap_or_default();
    let nodes = j["nodes"].as_array().cloned().unwrap_or_default().iter().filter_map(|n| {
        let mi = n["mesh"].as_u64()? as usize;
        let prims = meshes[mi]["primitives"].as_array().unwrap().iter().filter_map(|p| {
            let at = &p["attributes"];
            let pa = at["POSITION"].as_u64()? as usize;
            let ia = p["indices"].as_u64()? as usize;
            Some(Prim {
                pos: aview(pa), nrm: at["NORMAL"].as_u64().map(|a| aview(a as usize)), uv: at["TEXCOORD_0"].as_u64().map(|a| aview(a as usize)),
                idx: aview(ia), idx_u16: acc[ia]["componentType"].as_u64().unwrap() == 5123,
                icount: acc[ia]["count"].as_u64().unwrap() as u32, vcount: acc[pa]["count"].as_u64().unwrap() as u32,
                mat: p["material"].as_u64().unwrap_or(0) as usize, bmin: f3(&acc[pa]["min"]), bmax: f3(&acc[pa]["max"]),
            })
        }).collect();
        Some(Node { name: n["name"].as_str().unwrap_or("").to_string(), prims })
    }).collect();
    Glb { nodes, mats, data }
}
