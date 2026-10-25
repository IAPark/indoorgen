//! preview: headless real-time (wgpu/Metal) previewer for infinigen-rs glb scenes.
//! Flat/textured base colors, sun + shadow map, point lights, SSAO, ACES tonemap. Many cameras per process.
mod glb;
use glam::{Mat4, Vec3, Vec4};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use wgpu::util::DeviceExt;

const SHADOW: u32 = 4096;
const SRC: &str = include_str!("shader.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewproj: [[f32; 4]; 4], inv_viewproj: [[f32; 4]; 4], sun_vp: [[f32; 4]; 4],
    sun_dir: [f32; 4], sun_col: [f32; 4], sky: [f32; 4], ground: [f32; 4], cam_pos: [f32; 4], cam_fwd: [f32; 4],
    params: [f32; 4], params2: [f32; 4], lpos: [[f32; 4]; 32], lcol: [[f32; 4]; 32],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatU { color: [f32; 4], emissive: [f32; 4], flags: [f32; 4] }

#[derive(Clone)]
struct Cam { name: String, view: Mat4, fov_x: f32, ortho: Option<f32> }

struct Opts {
    inputs: Vec<PathBuf>, out: PathBuf, cams: Vec<CamSpec>, sheet: bool, res: (u32, u32), ss: u32,
    sun: (f32, f32), exposure: f32, ao: f32, no_ceiling: bool, ncams: usize, verbose: bool, ambient: f32, sun_i: f32, light_scale: f32,
}
#[derive(Clone)]
enum CamSpec { Json(PathBuf), At([f32; 6], f32), Person(usize), Top, Named(String) }

fn parse_args() -> Opts {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts { inputs: vec![], out: PathBuf::from("preview.png"), cams: vec![], sheet: false, res: (960, 540), ss: 2,
        sun: (215.0, 38.0), exposure: 0.5, ao: 1.0, no_ceiling: false, ncams: 3, verbose: false, ambient: 1.0, sun_i: 1.0, light_scale: 1.0 };
    let mut i = 0;
    let nf = |s: &str| -> Vec<f32> { s.split(',').map(|x| x.parse().expect("number")).collect() };
    while i < a.len() {
        let k = a[i].as_str();
        let mut val = || { i += 1; a.get(i).cloned().unwrap_or_else(|| panic!("{k} needs a value")) };
        match k {
            "-o" | "--out" => o.out = PathBuf::from(val()),
            "--cam" => o.cams.push(CamSpec::Json(PathBuf::from(val()))),
            "--cams" => { let v = val(); match v.parse::<usize>() { Ok(n) => o.ncams = n, Err(_) => o.cams.push(CamSpec::Named(v)) } }
            "--view" => o.cams.push(CamSpec::Named(val())),
            "--at" => { let v = nf(&val()); assert!(v.len() >= 6, "--at ex,ey,ez,tx,ty,tz[,lens]"); o.cams.push(CamSpec::At([v[0], v[1], v[2], v[3], v[4], v[5]], *v.get(6).unwrap_or(&18.0))) }
            "--person" => o.cams.push(CamSpec::Person(val().parse().unwrap())),
            "--top" => o.cams.push(CamSpec::Top),
            "--sheet" => o.sheet = true,
            "--res" => { let v = val(); let (w, h) = v.split_once('x').expect("WxH"); o.res = (w.parse().unwrap(), h.parse().unwrap()) }
            "--ss" => o.ss = val().parse().unwrap(),
            "--sun" => { let v = nf(&val()); o.sun = (v[0], v[1]) }
            "--exposure" => o.exposure = val().parse().unwrap(),
            "--ao" => o.ao = val().parse().unwrap(),
            "--ambient" => o.ambient = val().parse().unwrap(),
            "--sun-intensity" => o.sun_i = val().parse().unwrap(),
            "--light-scale" => o.light_scale = val().parse().unwrap(),
            "--no-ceiling" => o.no_ceiling = true,
            "-v" => o.verbose = true,
            _ => o.inputs.push(PathBuf::from(k)),
        }
        i += 1;
    }
    assert!(!o.inputs.is_empty(), "usage: preview [--cam cam.json | --cams N|cam0 | --at ex,ey,ez,tx,ty,tz[,lens] | --person I | --top]... [--sheet] [--res WxH] [--ss N] [--sun az,el] [--exposure E] [--ao A] [--no-ceiling] -o OUT scene.glb...");
    o
}

// ---------------- scene on the GPU ----------------
struct Draw { node: usize, pos: (usize, u64, u64), nrm: Option<(usize, u64, u64)>, uv: Option<(usize, u64, u64)>, idx: (usize, u64, u64), u16: bool, icount: u32, vcount: u32, mat_bg: usize, glass: bool, shadow: bool, bmin: Vec3, bmax: Vec3 }
struct Light { pos: Vec3, color: Vec3 }
struct Scene {
    name: String, bufs: Vec<wgpu::Buffer>, zeros: wgpu::Buffer, draws: Vec<Draw>, mat_bgs: Vec<wgpu::BindGroup>, lights: Vec<Light>,
    bmin: Vec3, bmax: Vec3, side: Value, glb: glb::Glb, sun_vp: Mat4, shadow_view: wgpu::TextureView, shadow_done: bool,
}

fn perspective_rh(fovy: f32, aspect: f32, n: f32, f: f32) -> Mat4 {
    let t = 1.0 / (fovy / 2.0).tan();
    Mat4::from_cols(Vec4::new(t / aspect, 0.0, 0.0, 0.0), Vec4::new(0.0, t, 0.0, 0.0), Vec4::new(0.0, 0.0, f / (n - f), -1.0), Vec4::new(0.0, 0.0, n * f / (n - f), 0.0))
}
fn orthographic_rh(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> Mat4 {
    Mat4::from_cols(Vec4::new(2.0 / (r - l), 0.0, 0.0, 0.0), Vec4::new(0.0, 2.0 / (t - b), 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / (n - f), 0.0), Vec4::new(-(r + l) / (r - l), -(t + b) / (t - b), n / (n - f), 1.0))
}
fn look_at_rh(eye: Vec3, tgt: Vec3, up: Vec3) -> Mat4 {
    let f = (tgt - eye).normalize(); let s = f.cross(up).normalize(); let u = s.cross(f);
    Mat4::from_cols(Vec4::new(s.x, u.x, -f.x, 0.0), Vec4::new(s.y, u.y, -f.y, 0.0), Vec4::new(s.z, u.z, -f.z, 0.0), Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0))
}
fn blackbody(t: f32) -> Vec3 {
    let t = t / 100.0;
    let r = if t <= 66.0 { 255.0 } else { (329.698727446 * (t - 60.0).powf(-0.1332047592)).clamp(0.0, 255.0) };
    let g = if t <= 66.0 { (99.4708025861 * t.ln() - 161.1195681661).clamp(0.0, 255.0) } else { (288.1221695283 * (t - 60.0).powf(-0.0755148492)).clamp(0.0, 255.0) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { (138.5177312231 * (t - 10.0).ln() - 305.0447927307).clamp(0.0, 255.0) };
    Vec3::new(r, g, b) / 255.0
}

fn load_texture(dev: &wgpu::Device, q: &wgpu::Queue, img: image::RgbaImage) -> wgpu::TextureView {
    let mut img = img;
    if img.width().max(img.height()) > 1024 { let s = 1024.0 / img.width().max(img.height()) as f32; img = image::imageops::resize(&img, (img.width() as f32 * s) as u32, (img.height() as f32 * s) as u32, image::imageops::FilterType::Triangle); }
    let (w, h) = img.dimensions();
    let levels = 32 - w.max(h).leading_zeros();
    let tex = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: levels, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let mut cur = img;
    for l in 0..levels {
        let (cw, ch) = cur.dimensions();
        q.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &cur,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(cw * 4), rows_per_image: Some(ch) }, wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 });
        if cw > 1 || ch > 1 { cur = image::imageops::resize(&cur, (cw / 2).max(1), (ch / 2).max(1), image::imageops::FilterType::Triangle); }
    }
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

struct Gpu { dev: wgpu::Device, q: wgpu::Queue }

fn load_scene(gpu: &Gpu, path: &Path, mat_bgl: &wgpu::BindGroupLayout, white: &wgpu::TextureView, smp: &wgpu::Sampler, opts: &Opts) -> Scene {
    let t = Instant::now();
    let glb = glb::load(path);
    let side: Value = std::fs::read(path.with_extension("json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if opts.verbose { eprintln!("  glb parse {:.2}s ({} nodes, {} chunks)", t.elapsed().as_secs_f32(), glb.nodes.len(), glb.chunks.len()); }
    let bufs: Vec<wgpu::Buffer> = glb.chunks.iter().map(|c| gpu.dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: c,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX })).collect();
    let maxv = glb.nodes.iter().flat_map(|n| n.prims.iter()).map(|p| p.vcount).max().unwrap_or(1) as u64;
    let zeros = gpu.dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: maxv * 12, usage: wgpu::BufferUsages::VERTEX, mapped_at_creation: false });
    // sidecar specs per node name
    let mut specs: HashMap<String, Value> = HashMap::new();
    if let Some(ns) = side["nodes"].as_array() { for n in ns { if let Some(nm) = n["name"].as_str() { specs.insert(nm.to_string(), n.clone()); } } }
    // textures (people)
    let mut tex_paths: Vec<String> = vec![];
    for n in &glb.nodes { if let Some(s) = specs.get(&n.name) { if let Some(c) = s["spec"]["kwargs"]["color"].as_str() { if s["spec"]["cls"].as_str().map_or(false, |c| c.starts_with("people.")) && !tex_paths.contains(&c.to_string()) { tex_paths.push(c.to_string()); } } } }
    use rayon::prelude::*;
    let decoded: Vec<Option<image::RgbaImage>> = tex_paths.par_iter().map(|p| image::open(p).ok().map(|i| i.to_rgba8())).collect();
    let mut tex_views: HashMap<String, wgpu::TextureView> = HashMap::new();
    for (p, d) in tex_paths.iter().zip(decoded) { if let Some(img) = d { tex_views.insert(p.clone(), load_texture(&gpu.dev, &gpu.q, img)); } }
    // material bind groups keyed by (material, texture path)
    let mut mat_bgs: Vec<wgpu::BindGroup> = vec![]; let mut bg_key: HashMap<(usize, String, u32), usize> = HashMap::new();
    let mut draws = vec![]; let mut bmin = Vec3::splat(1e9); let mut bmax = Vec3::splat(-1e9);
    for (ni, n) in glb.nodes.iter().enumerate() {
        let spec = specs.get(&n.name);
        let cls = spec.and_then(|s| s["spec"]["cls"].as_str()).unwrap_or("");
        let tex = if cls.starts_with("people.") { spec.and_then(|s| s["spec"]["kwargs"]["color"].as_str()).unwrap_or("").to_string() } else { String::new() };
        let lname = n.name.to_lowercase();
        let is_ceil = lname.contains("ceiling");
        let glass = lname.ends_with(".glass") && !lname.contains("wineglass");
        for p in &n.prims {
            let m = &glb.mats[p.mat.min(glb.mats.len().saturating_sub(1))];
            let flags = if cls == "people.hair" { 1u32 } else if glass { 2 } else { 0 };
            let key = (p.mat, tex.clone(), flags);
            let bi = *bg_key.entry(key).or_insert_with(|| {
                let u = MatU { color: m.color, emissive: [m.emissive[0], m.emissive[1], m.emissive[2], 0.0], flags: [flags as f32, 0.0, 0.0, 0.0] };
                let ub = gpu.dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
                let tv = tex_views.get(&tex).unwrap_or(white);
                mat_bgs.push(gpu.dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: mat_bgl, entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(tv) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(smp) } ] }));
                mat_bgs.len() - 1
            });
            let (lo, hi) = (Vec3::from(p.bmin), Vec3::from(p.bmax));
            if !is_ceil && !lname.starts_with("skirting_ceiling") { bmin = bmin.min(lo); bmax = bmax.max(hi); }
            let vw = |v: &glb::View| (v.chunk, v.off, v.len);
            draws.push(Draw { node: ni, pos: vw(&p.pos), nrm: p.nrm.as_ref().map(vw), uv: p.uv.as_ref().map(vw), idx: vw(&p.idx), u16: p.idx_u16, icount: p.icount, vcount: p.vcount,
                mat_bg: bi, glass, shadow: !glass && !(opts.no_ceiling && is_ceil), bmin: lo, bmax: hi });
        }
    }
    let lights: Vec<Light> = side["lights"].as_array().map(|ls| ls.iter().map(|l| {
        let loc = &l["loc"]; let w = l["watt"].as_f64().unwrap_or(50.0) as f32; let c = blackbody(l["temperature"].as_f64().unwrap_or(5000.0) as f32);
        Light { pos: Vec3::new(loc[0].as_f64().unwrap() as f32, loc[1].as_f64().unwrap() as f32, loc[2].as_f64().unwrap() as f32), color: c * (w / (4.0 * std::f32::consts::PI * std::f32::consts::PI)) * opts.light_scale }
    }).collect()).unwrap_or_default();
    // sun
    let (az, el) = (opts.sun.0.to_radians(), opts.sun.1.to_radians());
    let sdir = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
    let c = (bmin + bmax) * 0.5; let r = ((bmax - bmin) * 0.5).length();
    let sv = look_at_rh(c + sdir * r * 2.0, c, Vec3::Z);
    let sp = orthographic_rh(-r, r, -r, r, 0.0, r * 4.0);
    let sh = gpu.dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: SHADOW, height: SHADOW, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
    if opts.verbose { eprintln!("  scene on GPU {:.2}s ({} draws, {} lights, bbox {:?}..{:?})", t.elapsed().as_secs_f32(), draws.len(), lights.len(), bmin, bmax); }
    Scene { name: path.file_stem().unwrap().to_string_lossy().to_string(), bufs, zeros, draws, mat_bgs, lights, bmin, bmax, side, glb,
        sun_vp: sp * sv, shadow_view: sh.create_view(&wgpu::TextureViewDescriptor::default()), shadow_done: false }
}

// ---------------- cameras ----------------
fn cam_look(eye: Vec3, tgt: Vec3, lens: f32) -> Cam {
    let f = (tgt - eye).normalize(); let r = f.cross(Vec3::Z).normalize(); let u = r.cross(f);
    let m = Mat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), eye.extend(1.0));
    Cam { name: String::new(), view: m, fov_x: 2.0 * (18.0f32 / lens).atan(), ortho: None }
}
fn mat_from_json(v: &Value) -> Mat4 {
    let mut c = [0f32; 16];
    for r in 0..4 { for k in 0..4 { c[k * 4 + r] = v[r][k].as_f64().unwrap() as f32; } }
    Mat4::from_cols_array(&c)
}
fn cam_json(v: &Value, name: &str) -> Cam {
    let lens = v["lens"].as_f64().unwrap_or(18.0) as f32; let sw = v["sensor_width"].as_f64().unwrap_or(36.0) as f32;
    Cam { name: name.to_string(), view: mat_from_json(&v["matrix"]), fov_x: 2.0 * (sw / (2.0 * lens)).atan(), ortho: None }
}
fn person_pos(s: &Scene, idx: usize) -> Option<Vec3> {
    let mut seen: Vec<String> = vec![];
    for n in s.side["nodes"].as_array()? {
        if n["class"] == "Person" && n["pose"].is_array() {
            let nm = n["name"].as_str()?.split('#').next()?.to_string();
            if !seen.contains(&nm) { seen.push(nm); if seen.len() == idx + 1 { let p = &n["pose"]; return Some(Vec3::new(p[0][3].as_f64()? as f32, p[1][3].as_f64()? as f32, p[2][3].as_f64()? as f32)); } }
        }
    }
    None
}
fn node_bbox(s: &Scene, pred: impl Fn(&str) -> bool) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(1e9); let mut hi = Vec3::splat(-1e9); let mut any = false;
    for d in &s.draws { if pred(&s.glb.nodes[d.node].name) { lo = lo.min(d.bmin); hi = hi.max(d.bmax); any = true; } }
    if any { Some((lo, hi)) } else { None }
}
fn auto_cams(s: &Scene, n: usize) -> Vec<Cam> {
    // subject = centre of the furniture/people (everything that is not a room shell or opening)
    let skip = |nm: &str| nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("window") || nm.starts_with("door") || nm.contains("exterior");
    let (mut acc, mut cnt) = (Vec3::ZERO, 0.0);
    for d in &s.draws { if !skip(&s.glb.nodes[d.node].name) { acc += (d.bmin + d.bmax) * 0.5; cnt += 1.0; } }
    let subj = if cnt > 0.0 { acc / cnt } else { (s.bmin + s.bmax) * 0.5 };
    let mut rooms: Vec<(f32, Vec3, Vec3, String)> = vec![];
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if nm.ends_with(".floor") { let a = (d.bmax.x - d.bmin.x) * (d.bmax.y - d.bmin.y); rooms.push((a, d.bmin, d.bmax, nm.clone())); } }
    rooms.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let mut out = vec![];
    for (_, lo, hi, nm) in rooms.iter() {
        let c = (*lo + *hi) * 0.5;
        let mut corners = vec![Vec3::new(lo.x, lo.y, 0.0), Vec3::new(hi.x, lo.y, 0.0), Vec3::new(hi.x, hi.y, 0.0), Vec3::new(lo.x, hi.y, 0.0)];
        corners.sort_by(|a, b| (*b - subj.truncate().extend(0.0)).length().partial_cmp(&(*a - subj.truncate().extend(0.0)).length()).unwrap());
        for k in 0..2 {
            let p = corners[k]; let d = (c - p).truncate().normalize_or_zero().extend(0.0);
            let eye = Vec3::new(p.x, p.y, lo.z + 1.5) + d * 0.5;
            let mut cm = cam_look(eye, Vec3::new(subj.x, subj.y, lo.z + 0.9), 18.0);
            cm.name = format!("{}_{}", nm.trim_end_matches(".floor").replace('/', "_"), if k == 0 { "A" } else { "B" });
            out.push(cm);
            if out.len() >= n { return out; }
        }
    }
    out
}
fn resolve_cams(s: &Scene, o: &Opts) -> Vec<Cam> {
    let mut v = vec![];
    let sidecams = s.side["cameras"].as_array().cloned().unwrap_or_default();
    let mut specs = o.cams.clone();
    if specs.is_empty() { if sidecams.is_empty() { v.extend(auto_cams(s, o.ncams)); } else { for (i, c) in sidecams.iter().enumerate().take(o.ncams) { v.push(cam_json(c, c["name"].as_str().unwrap_or(&format!("cam{i}")))); } } }
    for sp in specs.drain(..) {
        match sp {
            CamSpec::Json(p) => { let j: Value = serde_json::from_slice(&std::fs::read(&p).expect("cam json")).unwrap(); let nm = p.file_stem().unwrap().to_string_lossy().to_string(); v.push(cam_json(&j, &nm)); }
            CamSpec::At(a, lens) => { let mut c = cam_look(Vec3::new(a[0], a[1], a[2]), Vec3::new(a[3], a[4], a[5]), lens); c.name = format!("at{}", v.len()); v.push(c); }
            CamSpec::Named(nm) => {
                if let Some(c) = sidecams.iter().find(|c| c["name"] == nm.as_str()) { v.push(cam_json(c, &nm)); }
                else if nm == "auto" { v.extend(auto_cams(s, o.ncams)); }
                else if let Some(n) = nm.strip_prefix("cam").and_then(|x| x.parse::<usize>().ok()) { let ac = auto_cams(s, n + 1); if let Some(c) = ac.into_iter().nth(n) { v.push(c); } }
                else { eprintln!("camera {nm} not found"); }
            }
            CamSpec::Person(i) => {
                if let Some(p) = person_pos(s, i) {
                    let subj = node_bbox(s, |_| false); let _ = subj;
                    let c0 = (s.bmin + s.bmax) * 0.5; let mut d = (c0 - p).truncate().normalize_or_zero(); if d == glam::Vec2::ZERO { d = glam::Vec2::X; }
                    let eye = p + d.extend(0.0) * 3.0 + Vec3::Z * 1.7; let mut c = cam_look(eye, p + Vec3::Z * 0.9, 24.0); c.name = format!("person{i}"); v.push(c);
                } else { eprintln!("person {i} not found"); }
            }
            CamSpec::Top => {
                let c = (s.bmin + s.bmax) * 0.5; let ext = (s.bmax - s.bmin);
                let mut cm = cam_look(Vec3::new(c.x, c.y, s.bmax.z + 20.0), Vec3::new(c.x, c.y + 1e-3, c.z), 18.0);
                cm.ortho = Some(ext.x.max(ext.y * o.res.0 as f32 / o.res.1 as f32) * 1.05); cm.name = "top".into(); v.push(cm);
            }
        }
    }
    v
}

// ---------------- renderer ----------------
struct Targets { hdr: wgpu::TextureView, nrm: wgpu::TextureView, depth: wgpu::TextureView, out: wgpu::Texture, readback: wgpu::Buffer, w: u32, h: u32, bpr: u32 }
struct Renderer {
    gpu: Gpu, ubuf: wgpu::Buffer, g_bgl: wgpu::BindGroupLayout, mat_bgl: wgpu::BindGroupLayout, post_bgl: wgpu::BindGroupLayout,
    shadow_pipe: wgpu::RenderPipeline, main_pipe: wgpu::RenderPipeline, glass_pipe: wgpu::RenderPipeline, post_pipe: wgpu::RenderPipeline,
    cmp_smp: wgpu::Sampler, white: wgpu::TextureView, smp: wgpu::Sampler, tg: Targets,
}
fn vbuf_layouts() -> [wgpu::VertexBufferLayout<'static>; 3] {
    [wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3] },
     wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![1 => Float32x3] },
     wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![2 => Float32x2] }]
}
fn make_targets(dev: &wgpu::Device, o: &Opts) -> Targets {
    let (w, h) = (o.res.0 * o.ss, o.res.1 * o.ss);
    let mk = |fmt, usage| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage, view_formats: &[] });
    let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let hdr = mk(wgpu::TextureFormat::Rgba16Float, rt).create_view(&Default::default());
    let nrm = mk(wgpu::TextureFormat::Rgba16Float, rt).create_view(&Default::default());
    let depth = mk(wgpu::TextureFormat::Depth32Float, rt).create_view(&Default::default());
    let out = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let bpr = (o.res.0 * 4 + 255) / 256 * 256;
    let readback = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * o.res.1) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    Targets { hdr, nrm, depth, out, readback, w, h, bpr }
}

fn init(o: &Opts) -> Renderer {
    let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let ad = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).expect("no adapter");
    if o.verbose { eprintln!("adapter: {:?}", ad.get_info().name); }
    let (dev, q) = pollster::block_on(ad.request_device(&wgpu::DeviceDescriptor { required_limits: ad.limits(), ..Default::default() })).expect("device");
    let uni = |b: u32, vis| wgpu::BindGroupLayoutEntry { binding: b, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
    let tex2d = |b: u32, depth: bool, vis| wgpu::BindGroupLayoutEntry { binding: b, visibility: vis, ty: wgpu::BindingType::Texture { sample_type: if depth { wgpu::TextureSampleType::Depth } else { wgpu::TextureSampleType::Float { filterable: true } }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
    let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT; let f = wgpu::ShaderStages::FRAGMENT;
    let bgl = |e: &[wgpu::BindGroupLayoutEntry]| dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: e });
    let g_bgl = bgl(&[uni(0, vf), tex2d(1, true, f), wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None }]);
    let sh_bgl = bgl(&[uni(0, vf)]);
    let mat_bgl = bgl(&[uni(0, f), tex2d(1, false, f), wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }]);
    let post_bgl = bgl(&[uni(0, f), wgpu::BindGroupLayoutEntry { binding: 3, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        wgpu::BindGroupLayoutEntry { binding: 4, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        wgpu::BindGroupLayoutEntry { binding: 5, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }]);
    let pl = |l: &[&wgpu::BindGroupLayout]| dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &l.iter().map(|x| Some(*x)).collect::<Vec<_>>(), immediate_size: 0 });
    let sm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SRC.into()) });
    let vl = vbuf_layouts();
    let prim = wgpu::PrimitiveState { cull_mode: None, ..Default::default() };
    let ds = |write: bool, bias: bool| wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(write), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(),
        bias: if bias { wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 } } else { Default::default() } };
    let shadow_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl(&[&sh_bgl])),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(vl[0].clone())] },
        primitive: prim, depth_stencil: Some(ds(true, true)), multisample: Default::default(), fragment: None, multiview_mask: None, cache: None });
    let mk_main = |glass: bool| {
        let tgt = [Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: if glass { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None }, write_mask: wgpu::ColorWrites::ALL }),
                   Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: if glass { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL } })];
        dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl(&[&g_bgl, &mat_bgl])),
            vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &vl.iter().cloned().map(Some).collect::<Vec<_>>() },
            primitive: prim, depth_stencil: Some(ds(!glass, false)), multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &sm, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &tgt }), multiview_mask: None, cache: None })
    };
    let main_pipe = mk_main(false); let glass_pipe = mk_main(true);
    let post_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl(&[&post_bgl])),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: &sm, entry_point: Some("fs_post"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())] }), multiview_mask: None, cache: None });
    let ubuf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let cmp_smp = dev.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::LessEqual), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    let smp = dev.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear,
        address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, ..Default::default() });
    let wt = dev.create_texture_with_data(&q, &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::util::TextureDataOrder::LayerMajor, &[255, 255, 255, 255]);
    let white = wt.create_view(&Default::default());
    let tg = make_targets(&dev, o);
    Renderer { gpu: Gpu { dev, q }, ubuf, g_bgl, mat_bgl, post_bgl, shadow_pipe, main_pipe, glass_pipe, post_pipe, cmp_smp, white, smp, tg }
}

fn frustum(vp: Mat4) -> [Vec4; 6] {
    let r = [vp.row(0), vp.row(1), vp.row(2), vp.row(3)];
    [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]]
}
fn visible(f: &[Vec4; 6], lo: Vec3, hi: Vec3) -> bool {
    for p in f { let v = Vec3::new(if p.x >= 0.0 { hi.x } else { lo.x }, if p.y >= 0.0 { hi.y } else { lo.y }, if p.z >= 0.0 { hi.z } else { lo.z }); if p.truncate().dot(v) + p.w < 0.0 { return false; } }
    true
}

fn draw_all<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, glass: bool, shadow: bool, fr: Option<&[Vec4; 6]>) {
    for d in &s.draws {
        if shadow { if !d.shadow { continue; } } else if d.glass != glass { continue; }
        if let Some(f) = fr { if !visible(f, d.bmin, d.bmax) { continue; } }
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        if !shadow {
            match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
            match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
            rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
        }
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, 0..1);
    }
}

fn render(r: &mut Renderer, s: &mut Scene, cam: &Cam, o: &Opts) -> image::RgbaImage {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let aspect = o.res.0 as f32 / o.res.1 as f32;
    let proj = match cam.ortho { Some(w) => orthographic_rh(-w / 2.0, w / 2.0, -w / aspect / 2.0, w / aspect / 2.0, 0.05, 200.0),
        None => perspective_rh(2.0 * ((cam.fov_x / 2.0).tan() / aspect).atan(), aspect, 0.05, 200.0) };
    let view = cam.view.inverse(); let vp = proj * view; let eye = cam.view.w_axis.truncate(); let fwd = -cam.view.z_axis.truncate().normalize();
    let (az, el) = (o.sun.0.to_radians(), o.sun.1.to_radians());
    let sdir = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
    let sr = ((s.bmax - s.bmin) * 0.5).length() * 2.0;
    let mut gl = Globals { viewproj: vp.to_cols_array_2d(), inv_viewproj: vp.inverse().to_cols_array_2d(), sun_vp: s.sun_vp.to_cols_array_2d(),
        sun_dir: sdir.extend(0.0).into(), sun_col: [3.2 * o.sun_i, 2.9 * o.sun_i, 2.5 * o.sun_i, 0.0], sky: [0.62 * o.ambient, 0.72 * o.ambient, 0.9 * o.ambient, 0.0], ground: [0.28 * o.ambient, 0.25 * o.ambient, 0.22 * o.ambient, 0.0],
        cam_pos: eye.extend(1.0).into(), cam_fwd: fwd.extend(0.0).into(), params: [s.lights.len().min(32) as f32, o.exposure, o.ao, 0.6], params2: [1.0 / SHADOW as f32, o.ss as f32, 1.0, sr],
        lpos: [[0.0; 4]; 32], lcol: [[0.0; 4]; 32] };
    for (i, l) in s.lights.iter().take(32).enumerate() { gl.lpos[i] = l.pos.extend(1.0).into(); gl.lcol[i] = l.color.extend(1.0).into(); }
    q.write_buffer(&r.ubuf, 0, bytemuck::bytes_of(&gl));
    let sh_bg = |l: &wgpu::BindGroupLayout| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: l, entries: &[wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() }] });
    let mut enc = dev.create_command_encoder(&Default::default());
    if !s.shadow_done {
        // sun_vp is camera independent: render the shadow map once per scene (needs its own uniform since ubuf holds the camera)
        let sl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
        let bg = sh_bg(&sl);
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[], timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &s.shadow_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
            rp.set_pipeline(&r.shadow_pipe); rp.set_bind_group(0, &bg, &[]);
            draw_all(&mut rp, s, false, true, None);
        }
        s.shadow_done = true;
    }
    let g_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.g_bgl, entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&s.shadow_view) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.cmp_smp) }] });
    let tg = &r.tg; let fr = frustum(vp);
    {
        let sky = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &tg.hdr, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } }),
                                 Some(wgpu::RenderPassColorAttachment { view: &tg.nrm, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &tg.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &g_bg, &[]);
        rp.set_pipeline(&r.main_pipe); draw_all(&mut rp, s, false, false, Some(&fr));
        rp.set_pipeline(&r.glass_pipe); draw_all(&mut rp, s, true, false, Some(&fr));
    }
    let post_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.post_bgl, entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tg.hdr) },
        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&tg.nrm) },
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&tg.depth) }] });
    let ov = tg.out.create_view(&Default::default());
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None, depth_stencil_attachment: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &ov, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })] });
        rp.set_pipeline(&r.post_pipe); rp.set_bind_group(0, &post_bg, &[]); rp.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &tg.out, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &tg.readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tg.bpr), rows_per_image: Some(o.res.1) } }, wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 });
    q.submit([enc.finish()]);
    let sl = tg.readback.slice(..);
    sl.map_async(wgpu::MapMode::Read, |_| {});
    dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = sl.get_mapped_range().unwrap();
    let mut img = image::RgbaImage::new(o.res.0, o.res.1);
    for y in 0..o.res.1 as usize { let row = &data[y * tg.bpr as usize..y * tg.bpr as usize + o.res.0 as usize * 4]; img.as_mut()[y * o.res.0 as usize * 4..(y + 1) * o.res.0 as usize * 4].copy_from_slice(row); }
    drop(data); tg.readback.unmap();
    img
}

fn save_png(img: &image::RgbaImage, p: &Path) {
    use image::ImageEncoder;
    let f = std::fs::File::create(p).unwrap_or_else(|e| panic!("create {}: {e}", p.display()));
    let enc = image::codecs::png::PngEncoder::new_with_quality(std::io::BufWriter::new(f), image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Sub);
    enc.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8).unwrap();
}

fn main() {
    let t0 = Instant::now();
    let o = parse_args();
    let mut r = init(&o);
    if o.verbose { eprintln!("gpu init {:.2}s", t0.elapsed().as_secs_f32()); }
    let mut frames: Vec<(String, image::RgbaImage)> = vec![];
    for path in &o.inputs {
        let t = Instant::now();
        let mut s = load_scene(&r.gpu, path, &r.mat_bgl, &r.white, &r.smp, &o);
        let cams = resolve_cams(&s, &o);
        if o.verbose { eprintln!("scene {} ready {:.2}s, {} cams", s.name, t.elapsed().as_secs_f32(), cams.len()); }
        for (i, c) in cams.iter().enumerate() {
            let t = Instant::now();
            let img = render(&mut r, &mut s, c, &o);
            if o.verbose { eprintln!("  cam {} {:.3}s", c.name, t.elapsed().as_secs_f32()); }
            let nm = if c.name.is_empty() { format!("cam{i}") } else { c.name.clone() };
            frames.push((format!("{}_{}", s.name, nm), img));
        }
        r.gpu.dev.poll(wgpu::PollType::wait_indefinitely()).ok();
    }
    if frames.is_empty() { eprintln!("no frames"); std::process::exit(2); }
    let is_png = o.out.extension().map_or(false, |e| e == "png");
    if o.sheet || (is_png && frames.len() > 1) {
        let n = frames.len(); let cols = (n as f32).sqrt().ceil() as u32; let rows = (n as u32 + cols - 1) / cols;
        let (w, h) = o.res; let gap = 4;
        let mut sheet = image::RgbaImage::from_pixel(cols * (w + gap) + gap, rows * (h + gap) + gap, image::Rgba([20, 20, 20, 255]));
        for (i, (_, im)) in frames.iter().enumerate() { image::imageops::replace(&mut sheet, im, (gap + (i as u32 % cols) * (w + gap)) as i64, (gap + (i as u32 / cols) * (h + gap)) as i64); }
        if let Some(p) = o.out.parent() { std::fs::create_dir_all(p).ok(); }
        let sp = if is_png { o.out.clone() } else { std::fs::create_dir_all(&o.out).ok(); o.out.join("sheet.png") };
        save_png(&sheet, &sp); println!("{}", sp.display());
    } else if is_png {
        save_png(&frames[0].1, &o.out); println!("{}", o.out.display());
    } else {
        std::fs::create_dir_all(&o.out).ok();
        for (nm, im) in &frames { let p = o.out.join(format!("{nm}.png")); save_png(im, &p); println!("{}", p.display()); }
    }
    if o.verbose { eprintln!("total {:.2}s", t0.elapsed().as_secs_f32()); }
}
