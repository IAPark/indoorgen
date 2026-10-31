//! Minimal glb reader for infinigen-rs scenes: nodes -> meshes -> primitives (world-space, Z-up mesh data), materials.
//! Binary data is split into <=CHUNK byte chunks at bufferView boundaries so every GPU buffer stays under device limits.
use serde_json::Value;
use std::path::Path;

pub const CHUNK: usize = 192 << 20;

#[derive(Clone, Copy, Debug)]
pub struct View { pub chunk: usize, pub off: u64, pub len: u64 }

pub struct Prim {
    pub pos: View, pub nrm: Option<View>, pub uv: Option<View>, pub idx: View,
    pub idx_u16: bool, pub icount: u32, pub vcount: u32, pub mat: usize,
    pub bmin: [f32; 3], pub bmax: [f32; 3],
}
pub struct Node { pub name: String, pub prims: Vec<Prim> }
/// `tex`: embedded base-colour image (PNG bytes) when the material has a baseColorTexture; `mask`: alphaMode MASK
pub struct Mat { pub name: String, pub color: [f32; 4], pub emissive: [f32; 3], pub tex: Option<std::sync::Arc<Vec<u8>>>, pub mask: bool }
pub struct Glb { pub nodes: Vec<Node>, pub mats: Vec<Mat>, pub chunks: Vec<Vec<u8>> }

fn f3(v: &Value) -> [f32; 3] {
    let a = v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>()).unwrap_or(vec![0.0; 3]);
    [a[0], a[1], a[2]]
}

pub fn load(path: &Path) -> Glb {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(&data[0..4] == b"glTF", "not a glb");
    let jl = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let j: Value = serde_json::from_slice(&data[20..20 + jl]).expect("glb json");
    let bin_start = 20 + jl + 8;
    let bin = &data[bin_start..];
    let views = j["bufferViews"].as_array().unwrap();
    // chunk assignment: sequential greedy grouping by byteOffset
    let mut order: Vec<usize> = (0..views.len()).collect();
    order.sort_by_key(|&i| views[i]["byteOffset"].as_u64().unwrap_or(0));
    let mut vmap = vec![View { chunk: 0, off: 0, len: 0 }; views.len()];
    let mut chunks: Vec<Vec<u8>> = vec![Vec::new()];
    for &i in &order {
        let o = views[i]["byteOffset"].as_u64().unwrap_or(0) as usize;
        let l = views[i]["byteLength"].as_u64().unwrap() as usize;
        if !chunks.last().unwrap().is_empty() && chunks.last().unwrap().len() + l + 4 > CHUNK { chunks.push(Vec::new()); }
        let c = chunks.last_mut().unwrap();
        while c.len() % 4 != 0 { c.push(0); }
        let off = c.len();
        c.extend_from_slice(&bin[o..o + l]);
        vmap[i] = View { chunk: chunks.len() - 1, off: off as u64, len: l as u64 };
    }
    let acc = j["accessors"].as_array().unwrap();
    let aview = |a: usize| -> View {
        let ac = &acc[a];
        let v = vmap[ac["bufferView"].as_u64().unwrap() as usize];
        let ao = ac["byteOffset"].as_u64().unwrap_or(0);
        View { chunk: v.chunk, off: v.off + ao, len: v.len - ao }
    };
    // embedded images (bufferView PNGs), shared by every material that uses them
    let imgs: Vec<Option<std::sync::Arc<Vec<u8>>>> = j["images"].as_array().map(|a| a.iter().map(|im| {
        let bv = im["bufferView"].as_u64()? as usize;
        let o = views[bv]["byteOffset"].as_u64().unwrap_or(0) as usize; let l = views[bv]["byteLength"].as_u64()? as usize;
        Some(std::sync::Arc::new(bin[o..o + l].to_vec()))
    }).collect()).unwrap_or_default();
    let tex_img = |ti: u64| -> Option<std::sync::Arc<Vec<u8>>> { let src = j["textures"][ti as usize]["source"].as_u64()? as usize; imgs.get(src).cloned().flatten() };
    let mats: Vec<Mat> = j["materials"].as_array().map(|m| m.iter().map(|m| {
        let c = m["pbrMetallicRoughness"]["baseColorFactor"].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect::<Vec<_>>()).unwrap_or(vec![0.8, 0.8, 0.8, 1.0]);
        let es = m["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"].as_f64().unwrap_or(1.0) as f32;
        let e = if m["emissiveFactor"].is_array() { f3(&m["emissiveFactor"]) } else { [0.0; 3] };
        let tex = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().and_then(tex_img);
        Mat { name: m["name"].as_str().unwrap_or("").to_string(), color: [c[0], c[1], c[2], c[3]], emissive: [e[0] * es, e[1] * es, e[2] * es], tex, mask: m["alphaMode"].as_str() == Some("MASK") }
    }).collect()).unwrap_or_default();
    let meshes = j["meshes"].as_array().unwrap();
    let nodes = j["nodes"].as_array().unwrap().iter().filter_map(|n| {
        let mi = n["mesh"].as_u64()? as usize;
        let prims = meshes[mi]["primitives"].as_array().unwrap().iter().filter_map(|p| {
            let at = &p["attributes"];
            let pa = at["POSITION"].as_u64()? as usize;
            let ia = p["indices"].as_u64()? as usize;
            let vcount = acc[pa]["count"].as_u64().unwrap() as u32;
            let icount = acc[ia]["count"].as_u64().unwrap() as u32;
            let u16i = acc[ia]["componentType"].as_u64().unwrap() == 5123;
            Some(Prim {
                pos: aview(pa), nrm: at["NORMAL"].as_u64().map(|a| aview(a as usize)), uv: at["TEXCOORD_0"].as_u64().map(|a| aview(a as usize)),
                idx: aview(ia), idx_u16: u16i, icount, vcount, mat: p["material"].as_u64().unwrap_or(0) as usize,
                bmin: f3(&acc[pa]["min"]), bmax: f3(&acc[pa]["max"]),
            })
        }).collect();
        Some(Node { name: n["name"].as_str().unwrap_or("").to_string(), prims })
    }).collect();
    Glb { nodes, mats, chunks }
}
