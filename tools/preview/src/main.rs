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
    params: [f32; 4], clip2: [f32; 4], clip: [f32; 4], params2: [f32; 4], lpos: [[f32; 4]; 32], lcol: [[f32; 4]; 32],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatU { color: [f32; 4], emissive: [f32; 4], flags: [f32; 4] }

#[derive(Clone)]
struct Cam { name: String, view: Mat4, fov_x: f32, ortho: Option<f32>, /** a cutaway's clipping distance: what stands before it (a wall) is not drawn */ near: Option<f32>, /** a cutaway's vertical clip plane (normal, offset) */ clip: [f32; 4], clip2: [f32; 4], /** a view from above: only this room is drawn */ only: Option<(Vec3, Vec3, String)>, /** the share of what the room holds in this view, by size */ shown: f32, /** the room's main piece is in the view */ key: bool }

struct Opts {
    inputs: Vec<PathBuf>, out: PathBuf, cams: Vec<CamSpec>, sheet: bool, res: (u32, u32), ss: u32,
    sun: (f32, f32), exposure: f32, ao: f32, no_ceiling: bool, ncams: usize, verbose: bool, ambient: f32, sun_i: f32, light_scale: f32, cloth: Option<PathBuf>, depth: bool, sidecar_lighting: bool,
}
#[derive(Clone)]
enum CamSpec { Json(PathBuf), At([f32; 6], f32), Person(usize), Top, Named(String) }

fn parse_args() -> Opts {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts { inputs: vec![], out: PathBuf::from("preview.png"), cams: vec![], sheet: false, res: (960, 540), ss: 2,
        sun: (215.0, 38.0), exposure: 0.5, ao: 1.0, no_ceiling: false, ncams: 3, verbose: false, ambient: 1.0, sun_i: 1.0, light_scale: 1.0, cloth: None, depth: false, sidecar_lighting: false };
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
            "--sidecar-lighting" => o.sidecar_lighting = true,
            "--cloth" => o.cloth = Some(PathBuf::from(val())),
            "--depth" => o.depth = true, // also write <scene>_<cam>.depth.npy (float32 z-depth in metres along the optical axis, 0 = no surface) next to the pngs (OUT_DIR mode)
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
    /// (azimuth deg, elevation deg, sun intensity, ambient) from the sidecar `variation.lighting` (--sidecar-lighting)
    lit: Option<(f32, f32, f32, f32)>,
    name: String, bufs: Vec<wgpu::Buffer>, zeros: wgpu::Buffer, draws: Vec<Draw>, mat_bgs: Vec<wgpu::BindGroup>, lights: Vec<Light>,
    bmin: Vec3, bmax: Vec3, side: Value, glb: glb::Glb, sun_vp: Mat4, shadow_view: wgpu::TextureView, shadow_done: bool,
}

fn clip_range(s: &Scene) -> (f32, f32) {
    let diag = (s.bmax - s.bmin).length();
    if diag <= 200.0 { (0.05, 200.0) } else { (0.1, (diag * 1.1).min(20000.0)) }
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

/// how much of a picture has something in it: the share of its tiles that are not flat, colour counting a little extra
fn picture_detail(im: &image::RgbaImage) -> f32 {
    let (w, h) = im.dimensions(); let (tw, th) = ((w / 24).max(1), (h / 14).max(1));
    let (mut busy, mut n) = (0f32, 0f32);
    for ty in 0..(h / th) { for tx in 0..(w / tw) {
        let (mut sum, mut sq, mut sat, mut c) = (0f32, 0f32, 0f32, 0f32);
        for y in (ty * th..(ty + 1) * th).step_by(2) { for x in (tx * tw..(tx + 1) * tw).step_by(2) {
            let p = im.get_pixel(x, y); let (a, b, d) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
            let l = 0.2126 * a + 0.7152 * b + 0.0722 * d; sum += l; sq += l * l; sat += a.max(b).max(d) - a.min(b).min(d); c += 1.0;
        } }
        let (m, v) = (sum / c, sq / c - (sum / c).powi(2));
        // (edges only: a flat brown wardrobe side filling the frame is colourful, and shows nothing)
        let _ = sat; if v.max(0.0).sqrt() > 0.022 { busy += 1.0; }
        let _ = m; n += 1.0;
    } }
    busy / n
}

/// the share of the frame's width with nothing in it from top to bottom: half a picture of blank wall or a door leaf
fn flat_columns(im: &image::RgbaImage) -> f32 {
    let (w, h) = im.dimensions(); let cols = 24u32; let rows = 14u32;
    let mut flat = 0;
    for cx in 0..cols {
        let (x0, x1) = (cx * w / cols, (cx + 1) * w / cols);
        let mut busy = 0;
        for ry in 2..rows - 2 {
            let (y0, y1) = (ry * h / rows, (ry + 1) * h / rows);
            let (mut sum, mut sq, mut c) = (0f32, 0f32, 0f32);
            for y in (y0..y1).step_by(2) { for x in (x0..x1).step_by(2) { let p = im.get_pixel(x, y); let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0; sum += l; sq += l * l; c += 1.0; } }
            if (sq / c - (sum / c).powi(2)).max(0.0).sqrt() > 0.03 { busy += 1; }
        }
        if busy == 0 { flat += 1; }
    }
    flat as f32 / cols as f32
}

/// a small abstract painting, its own for each frame (by name)
fn painting(name: &str) -> image::RgbaImage { painting_at(name, 0) }
fn painting_at(name: &str, depth: u32) -> image::RgbaImage {
    let mut h = 0xcbf29ce484222325u64; for b in name.bytes() { h = (h ^ b as u64).wrapping_mul(0x100000001b3); }
    let mut next = move || { h ^= h << 13; h ^= h >> 7; h ^= h << 17; (h >> 11) as f32 / (1u64 << 53) as f32 };
    const PAL: [[[f32; 3]; 4]; 6] = [
        [[0.86, 0.80, 0.68], [0.72, 0.40, 0.28], [0.25, 0.33, 0.38], [0.91, 0.72, 0.38]],
        [[0.84, 0.86, 0.84], [0.33, 0.47, 0.50], [0.15, 0.22, 0.30], [0.80, 0.55, 0.45]],
        [[0.93, 0.89, 0.80], [0.52, 0.58, 0.40], [0.28, 0.35, 0.22], [0.78, 0.65, 0.35]],
        [[0.20, 0.23, 0.30], [0.80, 0.42, 0.32], [0.90, 0.78, 0.60], [0.40, 0.55, 0.62]],
        [[0.90, 0.85, 0.82], [0.70, 0.50, 0.55], [0.38, 0.28, 0.40], [0.85, 0.70, 0.60]],
        [[0.78, 0.84, 0.88], [0.40, 0.56, 0.70], [0.92, 0.88, 0.76], [0.20, 0.30, 0.44]]];
    let pal = PAL[(next() * 6.0) as usize % 6];
    let style = (next() * 4.0) as usize;
    let mut shapes: Vec<[f32; 5]> = (0..7).map(|_| [next(), next(), 0.08 + 0.3 * next(), 0.08 + 0.3 * next(), (next() * 3.99).floor()]).collect();
    if style == 1 { shapes.truncate(3); }
    let (ph, amp) = (next() * 6.28, 0.04 + 0.1 * next());
    let n = 768u32;
    let img = image::RgbaImage::from_fn(n, n, |x, y| {
        let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
        let mut c = pal[0];
        match style {
            // a horizon and hills
            0 => { let hz = 0.45 + amp * (u * 7.0 + ph).sin(); let hz2 = 0.65 + amp * (u * 4.0 + ph * 2.0).sin(); if v > hz2 { c = pal[2]; } else if v > hz { c = pal[1]; } else { let k = v / hz.max(0.01); for i in 0..3 { c[i] = pal[0][i] * (1.0 - k) + pal[3][i] * k; } } }
            // colour fields
            1 => { let b = (v * 3.0 + amp * (u * 9.0 + ph).sin()).floor() as usize % 3; c = pal[b + 1]; }
            // blocks
            2 => for s in &shapes { if (u - s[0]).abs() < s[2] && (v - s[1]).abs() < s[3] { c = pal[s[4] as usize]; } },
            // discs
            _ => for s in &shapes { if (u - s[0]).hypot(v - s[1]) < s[2] { c = pal[s[4] as usize]; } },
        }
        image::Rgba([(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255])
    });
    // (the shapes can all fall off the canvas, or a palette's background fill it: a painting that reads as a blank board. Then another)
    let base = img.get_pixel(0, 0).0;
    let other = img.pixels().filter(|p| (0..3).any(|k| (p[k] as i32 - base[k] as i32).abs() > 30)).count() as f32 / (n * n) as f32;
    if (other < 0.2 || other > 0.97) && depth < 4 { return painting_at(&format!("{name}+"), depth + 1) }
    img
}
/// a mirror's look without reflections: cool silver, lighter toward the top, one soft diagonal band
fn mirror_sheen() -> image::RgbaImage {
    let n = 64u32;
    image::RgbaImage::from_fn(n, n, |x, y| {
        let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
        let band = (-((u + v - 0.9) * 7.0).powi(2)).exp() * 0.12;
        let t = 0.52 + 0.22 * (1.0 - v) + band;
        image::Rgba([(t * 235.0) as u8, (t * 245.0) as u8, (t * 255.0).min(255.0) as u8, 255])
    })
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
    let mut glb = glb::load(path);
    // --cloth OVERLAY.glb (tools/cloth_only.py): nodes of the same name are replaced by the overlay's (Blender-simulated cloth, moved people parts)
    if let Some(op) = &opts.cloth {
        let ov = glb::load(op);
        let base = glb.chunks.len();
        glb.chunks.extend(ov.chunks);
        for mut n in ov.nodes {
            let mat = glb.nodes.iter().find(|m| m.name == n.name).and_then(|m| m.prims.first().map(|p| p.mat)).unwrap_or(0);
            for p in n.prims.iter_mut() { p.pos.chunk += base; p.idx.chunk += base; if let Some(v) = p.nrm.as_mut() { v.chunk += base; } if let Some(v) = p.uv.as_mut() { v.chunk += base; } p.mat = mat; }
            for m in glb.nodes.iter_mut().filter(|m| m.name == n.name) { m.prims.clear(); }
            glb.nodes.push(n);
        }
    }
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
    // textures embedded in the glb (leaf cards), keyed "glb:<material index>"
    for (mi, m) in glb.mats.iter().enumerate() {
        if let Some(png) = &m.tex { if let Ok(img) = image::load_from_memory(png) { tex_views.insert(format!("glb:{mi}"), load_texture(&gpu.dev, &gpu.q, img.to_rgba8())); } }
    }
    // material bind groups keyed by (material, texture path)
    let mut mat_bgs: Vec<wgpu::BindGroup> = vec![]; let mut bg_key: HashMap<(usize, String, u32), usize> = HashMap::new();
    let mut draws = vec![]; let mut bmin = Vec3::splat(1e9); let mut bmax = Vec3::splat(-1e9);
    for (ni, n) in glb.nodes.iter().enumerate() {
        let spec = specs.get(&n.name);
        let cls = spec.and_then(|s| s["spec"]["cls"].as_str()).unwrap_or("");
        let tex = if cls.starts_with("people.") { spec.and_then(|s| s["spec"]["kwargs"]["color"].as_str()).unwrap_or("").to_string() } else { String::new() };
        let lname = n.name.to_lowercase();
        let is_ceil = lname.contains("ceiling");
        // (a shower's panes too: drawn solid, a shower looked like a white wardrobe)
        let glass = (lname.ends_with(".glass") && !lname.contains("wineglass")) || n.prims.iter().any(|p| glb.mats[p.mat.min(glb.mats.len().saturating_sub(1))].name.starts_with("shower_glass"));
        for p in &n.prims {
            let m = &glb.mats[p.mat.min(glb.mats.len().saturating_sub(1))];
            let mi = p.mat.min(glb.mats.len().saturating_sub(1));
            let tex = if m.tex.is_some() { format!("glb:{mi}") } else { tex.clone() };
            // (art and mirrors are the renderer's shaders: here a painting of its own for each, and a silver sheen, not flat tan and white boards)
            let is_art = m.name == "art" || m.name.starts_with("gallery_art");
            let shown = is_art || m.name == "mirror" || m.name == "wardrobe_mirror";
            let tex = if shown {
                let k = format!("{}:{}", m.name, n.name);
                if !tex_views.contains_key(&k) { let img = if is_art { painting(&format!("{}:{}", path.display(), n.name)) } else { mirror_sheen() }; tex_views.insert(k.clone(), load_texture(&gpu.dev, &gpu.q, img)); }
                k
            } else { tex };
            let flags = if cls == "people.hair" || m.mask { 1u32 } else if glass { 2 } else { 0 };
            let key = (p.mat, tex.clone(), flags);
            let bi = *bg_key.entry(key).or_insert_with(|| {
                let u = MatU { color: if shown { [1.0, 1.0, 1.0, 1.0] } else { m.color }, emissive: [m.emissive[0], m.emissive[1], m.emissive[2], 0.0], flags: [flags as f32, 0.0, 0.0, 0.0] };
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
    let lit = if opts.sidecar_lighting { side["variation"]["lighting"]["sun_dir"].as_array().map(|d| {
        let f = |i: usize| d[i].as_f64().unwrap() as f32;
        let l = &side["variation"]["lighting"];
        (f(1).atan2(f(0)).to_degrees(), f(2).asin().to_degrees(), l["sun_strength"].as_f64().unwrap_or(4.0) as f32 / 4.0, l["sky_strength"].as_f64().unwrap_or(1.0) as f32)
    }) } else { None };
    let sun = lit.map_or(opts.sun, |l| (l.0, l.1));
    let (az, el) = (sun.0.to_radians(), sun.1.to_radians());
    let sdir = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
    let c = (bmin + bmax) * 0.5; let r = ((bmax - bmin) * 0.5).length();
    let sv = look_at_rh(c + sdir * r * 2.0, c, Vec3::Z);
    let sp = orthographic_rh(-r, r, -r, r, 0.0, r * 4.0);
    let sh = gpu.dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: SHADOW, height: SHADOW, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
    if opts.verbose { eprintln!("  scene on GPU {:.2}s ({} draws, {} lights, bbox {:?}..{:?})", t.elapsed().as_secs_f32(), draws.len(), lights.len(), bmin, bmax); }
    Scene { lit, name: path.file_stem().unwrap().to_string_lossy().to_string(), bufs, zeros, draws, mat_bgs, lights, bmin, bmax, side, glb,
        sun_vp: sp * sv, shadow_view: sh.create_view(&wgpu::TextureViewDescriptor::default()), shadow_done: false }
}

// ---------------- cameras ----------------
fn cam_look(eye: Vec3, tgt: Vec3, lens: f32) -> Cam {
    let f = (tgt - eye).normalize(); let r = f.cross(Vec3::Z).normalize(); let u = r.cross(f);
    let m = Mat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), eye.extend(1.0));
    Cam { name: String::new(), view: m, fov_x: 2.0 * (18.0f32 / lens).atan(), ortho: None, near: None, clip: [0.0; 4], clip2: [0.0; 4], only: None, shown: 0.0, key: true }
}
fn mat_from_json(v: &Value) -> Mat4 {
    let mut c = [0f32; 16];
    for r in 0..4 { for k in 0..4 { c[k * 4 + r] = v[r][k].as_f64().unwrap() as f32; } }
    Mat4::from_cols_array(&c)
}
fn cam_json(v: &Value, name: &str) -> Cam {
    let lens = v["lens"].as_f64().unwrap_or(18.0) as f32; let sw = v["sensor_width"].as_f64().unwrap_or(36.0) as f32;
    Cam { name: name.to_string(), view: mat_from_json(&v["matrix"]), fov_x: 2.0 * (sw / (2.0 * lens)).atan(), ortho: None, near: None, clip: [0.0; 4], clip2: [0.0; 4], only: None, shown: 0.0, key: true }
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
// ---- CPU helpers: wall triangles for camera placement (visibility / clearance), floor triangles for room membership ----
type Tri = [Vec3; 3];
fn node_tris(s: &Scene, pred: impl Fn(&str) -> bool) -> Vec<Tri> {
    let mut out = vec![];
    for d in &s.draws {
        if !pred(&s.glb.nodes[d.node].name) { continue; }
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |i: usize| -> Vec3 { let r = |k: usize| f32::from_le_bytes(pb[(i * 3 + k) * 4..(i * 3 + k) * 4 + 4].try_into().unwrap()); Vec3::new(r(0), r(1), r(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t in 0..d.icount as usize / 3 { out.push([p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2))]); }
    }
    out
}
fn ray_hit(tris: &[Tri], o: Vec3, dir: Vec3, maxd: f32) -> bool {
    for t in tris {
        let e1 = t[1] - t[0]; let e2 = t[2] - t[0]; let pv = dir.cross(e2); let det = e1.dot(pv);
        if det.abs() < 1e-9 { continue; }
        let inv = 1.0 / det; let tv = o - t[0]; let u = tv.dot(pv) * inv; if !(0.0..=1.0).contains(&u) { continue; }
        let qv = tv.cross(e1); let v = dir.dot(qv) * inv; if v < 0.0 || u + v > 1.0 { continue; }
        let tt = e2.dot(qv) * inv; if tt > 0.01 && tt < maxd { return true; }
    }
    false
}
/// distance to the nearest triangle along a ray, up to `maxd`
fn ray_dist(tris: &[Tri], o: Vec3, dir: Vec3, maxd: f32) -> f32 {
    let mut best = maxd;
    for t in tris {
        let e1 = t[1] - t[0]; let e2 = t[2] - t[0]; let pv = dir.cross(e2); let det = e1.dot(pv);
        if det.abs() < 1e-9 { continue; }
        let inv = 1.0 / det; let tv = o - t[0]; let u = tv.dot(pv) * inv; if !(0.0..=1.0).contains(&u) { continue; }
        let qv = tv.cross(e1); let v = dir.dot(qv) * inv; if v < 0.0 || u + v > 1.0 { continue; }
        let tt = e2.dot(qv) * inv; if tt > 0.01 && tt < best { best = tt; }
    }
    best
}
/// how open a view is: rays across the frame, each worth its free depth up to 3 m; a wall or a door leaf close before the lens costs most
fn view_quality(near: &[Tri], low: &[Tri], e: Vec3, f: Vec3, half: f32) -> f32 {
    let (mut sum, n) = (0.0, 15);
    for k in 0..n {
        let a = -half + 2.0 * half * k as f32 / (n - 1) as f32;
        let d = Vec3::new(f.x * a.cos() - f.y * a.sin(), f.x * a.sin() + f.y * a.cos(), 0.0);
        let t = ray_dist(near, e, d, 3.0);
        sum += t - if t < 0.8 { 3.0 } else { 0.0 };
        // (a counter, a bed or a plant right under the lens: seen from above, huge, cut by the frame)
        let lo_o = Vec3::new(e.x, e.y, low.first().map_or(e.z, |t| t[0].z + 0.25));
        if ray_dist(low, lo_o, d, 1.7) < 1.7 { sum -= 5.0; }
    }
    sum / n as f32
}
fn in_tri2(p: Vec3, t: &Tri) -> bool {
    let s = |a: Vec3, b: Vec3, c: Vec3| (a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y);
    let d1 = s(p, t[0], t[1]); let d2 = s(p, t[1], t[2]); let d3 = s(p, t[2], t[0]);
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}
struct Shell { walls: Vec<Tri>, leaves: Vec<Tri>, boxes: Vec<(Vec3, Vec3)> }
fn shell_of(s: &Scene) -> Shell {
    // (the cameras stand in this house: only what is near its floors can block them, not the neighbours' houses)
    let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for d in &s.draws { if s.glb.nodes[d.node].name.ends_with(".floor") { lo = lo.min(d.bmin); hi = hi.max(d.bmax); } }
    let m = Vec3::new(1.0, 1.0, 0.0);
    let mut walls = node_tris(s, |n| n.contains(".wall") || n.contains("exterior"));
    walls.retain(|t| { let tl = t[0].min(t[1]).min(t[2]); let th = t[0].max(t[1]).max(t[2]); th.x > lo.x - m.x && tl.x < hi.x + m.x && th.y > lo.y - m.y && tl.y < hi.y + m.y });
    if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("SHELL {} wall triangles", walls.len()); }
    let skip = |nm: &str| nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("window") || nm.starts_with("door") || nm.contains("exterior") || nm.starts_with("outdoor") || nm.contains("rug");
    let boxes = s.draws.iter().filter(|d| !skip(&s.glb.nodes[d.node].name) && d.bmax.z > 0.3 && d.bmin.z < 1.9 && (d.bmax - d.bmin).length() > 0.5).map(|d| (d.bmin, d.bmax)).collect();
    // (door leaves: walls to a view, though not to a person)
    let leaves = node_tris(s, |n| n.starts_with("door") && !n.contains("frame"));
    Shell { walls, leaves, boxes }
}
impl Shell {
    fn blocked(&self, eye: Vec3, tgt: Vec3) -> bool { let d = tgt - eye; ray_hit(&self.walls, eye, d.normalize(), d.length() - 0.15) }
    fn clearance(&self, e: Vec3) -> f32 {
        let mut m = 3.0f32;
        for k in 0..8 { let a = k as f32 * std::f32::consts::FRAC_PI_4; let d = Vec3::new(a.cos(), a.sin(), 0.0);
            // distance to nearest wall along d (binary probe)
            let mut lo = 0.0f32; let mut hi = 3.0f32; if !ray_hit(&self.walls, e, d, hi) { continue; }
            for _ in 0..7 { let mid = (lo + hi) * 0.5; if ray_hit(&self.walls, e, d, mid) { hi = mid } else { lo = mid } } m = m.min(hi); }
        for (lo, hi) in &self.boxes { let dx = (lo.x - e.x).max(0.0).max(e.x - hi.x); let dy = (lo.y - e.y).max(0.0).max(e.y - hi.y); if e.z > lo.z - 0.2 && e.z < hi.z + 0.2 || true { m = m.min((dx * dx + dy * dy).sqrt()); } }
        m
    }
    /// how many of 9 rays across +-40deg toward the subject travel > `len` before hitting a wall
    fn openness(&self, e: Vec3, tgt: Vec3, len: f32) -> usize {
        let f = (tgt - e).truncate().normalize_or_zero(); let mut n = 0;
        for k in -4..=4 { let a = (k as f32 * 10.0).to_radians(); let d = Vec3::new(f.x * a.cos() - f.y * a.sin(), f.x * a.sin() + f.y * a.cos(), 0.0); if !ray_hit(&self.walls, e, d, len) { n += 1; } }
        n
    }
}
fn auto_cams(s: &Scene, n: usize) -> Vec<Cam> {
    let sh = shell_of(s);
    let skip = |nm: &str| nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("window") || nm.starts_with("door") || nm.contains("exterior") || nm.starts_with("outdoor") || nm.contains("rug");
    let mut floors: HashMap<String, Vec<Tri>> = HashMap::new();
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if nm.ends_with(".floor") { floors.entry(nm.clone()).or_default(); } }
    let names: Vec<String> = floors.keys().cloned().collect();
    for nm in names { let t = node_tris(s, |x| x == nm); floors.insert(nm, t); }
    struct Room { name: String, tris: Vec<Tri>, subj: Vec3, nobj: usize, area: f32 }
    let mut rooms: Vec<Room> = vec![];
    for (nm, tris) in floors.iter() {
        if tris.is_empty() { continue; }
        let area: f32 = tris.iter().map(|t| ((t[1] - t[0]).cross(t[2] - t[0]).z.abs()) * 0.5).sum();
        let (mut acc, mut w, mut cnt) = (Vec3::ZERO, 0.0, 0);
        for d in &s.draws { if skip(&s.glb.nodes[d.node].name) { continue; } let c = (d.bmin + d.bmax) * 0.5;
            if tris.iter().any(|t| in_tri2(Vec3::new(c.x, c.y, 0.0), t)) { let wt = (d.bmax - d.bmin).length(); acc += c * wt; w += wt; cnt += 1; } }
        let fc: Vec3 = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0 * ((t[1] - t[0]).cross(t[2] - t[0]).z.abs())).sum::<Vec3>() / (2.0 * area);
        let subj = if w > 0.0 { acc / w } else { fc };
        rooms.push(Room { name: nm.trim_end_matches(".floor").replace('/', "_"), tris: tris.clone(), subj, nobj: cnt, area });
    }
    rooms.sort_by(|a, b| (b.nobj, b.area as i32).cmp(&(a.nobj, a.area as i32)).then(a.name.cmp(&b.name)));
    let mut out = vec![];
    for rm in rooms.iter() {
        let z0 = rm.tris[0][0].z; let tgt = Vec3::new(rm.subj.x, rm.subj.y, z0 + 0.9);
        let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for t in &rm.tris { for v in t { lo = lo.min(*v); hi = hi.max(*v); } }
        let mut cut_done = false;
        // what the room holds, by size (not its curtains and lights), and the share of it in a view
        let held: Vec<(Vec3, f32)> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; !skip(nm) && !nm.contains("Ceiling") && !nm.contains("urtain") && !nm.contains("_light") && !nm.starts_with("light") && !nm.starts_with("beam") && !nm.contains("Pendant") && (d.bmax - d.bmin).max_element() > 0.2 && d.bmin.z < z0 + 1.9 }).filter_map(|d| { let c = (d.bmin + d.bmax) * 0.5; rm.tris.iter().any(|t| in_tri2(Vec3::new(c.x, c.y, z0), t)).then(|| (c, (d.bmax - d.bmin).truncate().length() * 0.5)) }).collect();
        let held_all: f32 = held.iter().map(|h| h.1).sum();
        // the room's main piece, by floor area: a bedroom's bed, a dining room's table
        let main_piece: Option<Vec3> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; !skip(nm) && !nm.contains("Ceiling") && !nm.contains("urtain") && d.bmin.z < z0 + 1.0 && d.bmax.z > z0 + 0.25 }).filter(|d| { let c = (d.bmin + d.bmax) * 0.5; rm.tris.iter().any(|t| in_tri2(Vec3::new(c.x, c.y, z0), t)) }).map(|d| { let e = d.bmax - d.bmin; (e.x * e.y, (d.bmin + d.bmax) * 0.5) }).filter(|m| m.0 > 0.4).max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).map(|m| m.1);
        let key_seen = |e: Vec3, f: Vec3, half: f32| -> bool { main_piece.map_or(true, |c| { let d = (c - e).truncate(); let l = d.length().max(0.01); (d / l).dot(f.truncate()).clamp(-1.0, 1.0).acos() < half }) };
        let share = |e: Vec3, f: Vec3, half: f32| -> f32 { if held_all <= 0.0 { return 1.0 } held.iter().filter(|(c, _)| { let d = (*c - e).truncate(); let l = d.length().max(0.01); (d / l).dot(f.truncate()).clamp(-1.0, 1.0).acos() < half }).map(|h| h.1).sum::<f32>() / held_all };
        // a narrow rectangular room has nowhere to stand: a cutaway, the camera behind the wall it faces away from, that wall clipped, and the frame at the cut no
        // wider than the room, so all that shows is the room. Far enough back that the far end fills the height.
        let (ex, ey) = (hi.x - lo.x, hi.y - lo.y);
        if rm.area / (ex * ey).max(0.01) > 0.985 && (ex.min(ey) < 2.3 || rm.area < 6.5) {
            let c = (lo + hi) * 0.5;
            let d = (rm.subj - c).truncate();
            // (a long room: down its length, a frame as wide as its long side would be taller than the room; a short one toward what it holds, then the other ways)
            let along_x = if ex.max(ey) > 3.4 || (d.x / ex.max(0.01)).abs().max((d.y / ey.max(0.01)).abs()) < 0.12 { ex >= ey } else { (d.x / ex.max(0.01)).abs() >= (d.y / ey.max(0.01)).abs() };
            let sign = if along_x { if d.x >= 0.0 { 1.0 } else { -1.0 } } else if d.y >= 0.0 { 1.0 } else { -1.0 };
            let ways = [(along_x, sign), (along_x, -sign), (!along_x, 1.0), (!along_x, -1.0)];
            let mut k = 0;
            for (ax, sg) in ways {
                let f = if ax { Vec3::new(sg, 0.0, 0.0) } else { Vec3::new(0.0, sg, 0.0) };
                let (l, w) = if ax { (ex, ey) } else { (ey, ex) };
                // (nor down a long thin one: a long lens stacks its door edges into stripes)
                if w > 3.4 || l / w.max(0.1) > 2.6 { continue }
                // (never through or behind anything: a bookcase against the removed wall showed its inside-out back, a counter its cross-section, a window its frame in mid-air)
                let cut = c.dot(f) - l * 0.5;
                let in_the_way = s.draws.iter().any(|d| {
                    let nm = &s.glb.nodes[d.node].name;
                    if nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("outdoor") || nm.contains("exterior") || nm.contains("Ceiling") || nm.starts_with("door") { return false }
                    if d.bmin.z > z0 + 2.2 || d.bmax.x < lo.x || d.bmin.x > hi.x || d.bmax.y < lo.y || d.bmin.y > hi.y { return false }
                    let near_side = if sg > 0.0 { if ax { d.bmin.x } else { d.bmin.y } } else if ax { -d.bmax.x } else { -d.bmax.y };
                    near_side < cut + 0.25 && near_side > cut - 0.45
                });
                if in_the_way { continue }
                let ratio = (2.7 / (0.5625 * w)).max(1.25);
                let back = (l / (ratio - 1.0)).clamp(0.6, 4.0);
                // (tilted, the frame's lower corners reach wider at the cut than its middle: inside the room still, or the house's outside shows at the edge)
                let half = (0.8 * (w * 0.5 - 0.01) / back).atan().min(60f32.to_radians());
                let e = Vec3::new(c.x, c.y, z0 + 1.3) - f * (l * 0.5 + back);
                // (a raised view, looking down into the room; the cut is a vertical plane, so the removed wall goes down to its foot)
                let e = e + Vec3::new(0.0, 0.0, 0.75);
                let look = e + f * 2.0 + Vec3::new(0.0, 0.0, -0.55);
                let mut cm = cam_look(e, look, 18.0 / half.tan());
                cm.near = Some(0.3);
                { let d = c.dot(f) - l * 0.5 + 0.04; cm.clip = [f.x, f.y, f.z, d]; }
                cm.shown = share(e, f, half); cm.key = key_seen(e, f, half);
                // (doors in the walls beside the cut: seen edge-on down a long lens, their casings and the rooms through them read as stripes and stray blocks)
                {
                    let mut ids: Vec<&str> = vec![];
                    for d in &s.draws {
                        let nm = &s.glb.nodes[d.node].name;
                        if !nm.starts_with("door") { continue }
                        let q = (d.bmin + d.bmax) * 0.5;
                        let side = if ax { Vec3::Y } else { Vec3::X };
                        let (al, la) = ((q - c).dot(f).abs(), (q - c).dot(side).abs());
                        if al < l * 0.5 && la > w * 0.5 - 0.4 && la < w * 0.5 + 0.4 { let id = nm.split('.').next().unwrap_or(nm); if !ids.contains(&id) { ids.push(id); } }
                    }
                    cm.shown -= 0.6 * ids.len() as f32;
                }
                cm.name = if k == 0 { format!("{}_A", rm.name) } else { format!("{}_A~{k}", rm.name) };
                out.push(cm);
                k += 1;
            }
            // (every way cuts something: an ordinary view after all. Else the ordinary spots stand by, should the cut show too little)
            if k > 0 {
                cut_done = true;
            if out.iter().filter(|c| !c.name.contains('~')).count() >= n { return out; }
            }
        }
        let mut cands: Vec<(f32, Vec3)> = vec![];
        let step = if rm.area < 7.0 { 0.3 } else { 0.6 }; let mut y = lo.y + step / 2.0;
        while y < hi.y { let mut x = lo.x + step / 2.0; while x < hi.x {
            let p = Vec3::new(x, y, z0);
            if rm.tris.iter().any(|t| in_tri2(p, t)) {
                let e = Vec3::new(x, y, z0 + 1.4); let dist = (e - tgt).truncate().length();
                if dist > (if rm.area < 7.0 { 0.9 } else { 1.8 }) && !sh.blocked(e, tgt) { cands.push((dist, e)); }
            }
            x += step; } y += step; }
        cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        // (not inside or right behind a piece: cameras stood in a fridge, behind a wardrobe, in a door leaf)
        let boxes: Vec<(Vec3, Vec3)> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; !(nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.contains("exterior") || nm.starts_with("outdoor") || nm.contains("rug") || nm.contains("Ceiling")) && (d.bmax - d.bmin).max_element() > 0.25 }).map(|d| (d.bmin, d.bmax)).collect();
        // (a piece standing on the floor counts all the way up: a camera does not hover over a counter or a bed)
        let inside = |p: Vec3, m: f32| boxes.iter().any(|(l, h)| { let top = if l.z < z0 + 1.0 { f32::MAX } else { h.z + m }; p.x > l.x - m && p.x < h.x + m && p.y > l.y - m && p.y < h.y + m && p.z > l.z - m && p.z < top });
        cands.retain(|(_, e)| !inside(*e, 0.2) && ![0.3f32, 0.6, 0.9].iter().any(|t| { let q = *e + (tgt - *e).normalize_or_zero() * *t; boxes.iter().any(|(l, h)| q.x > l.x && q.x < h.x && q.y > l.y && q.y < h.y && q.z > l.z && q.z < h.z) }));
        // (nor a door leaf filling the view: a hallway's camera saw nothing but the open door)
        let leaves: Vec<(Vec3, Vec3)> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; nm.starts_with("door") && !nm.contains("frame") }).map(|d| (d.bmin, d.bmax)).collect();
        let leaf_blocks = |e: Vec3, t: Vec3| { let f = (t - e).normalize_or_zero(); [0.25f32, 0.5, 0.75, 1.0, 1.5].iter().any(|k| { let q = e + f * *k; leaves.iter().any(|(l, h)| q.x > l.x - 0.03 && q.x < h.x + 0.03 && q.y > l.y - 0.03 && q.y < h.y + 0.03) }) };
        cands.retain(|(_, e)| !leaf_blocks(*e, tgt));
        // the most open views: for each spot the best of a few headings around the subject, a farther spot a little better
        let mut tall: Vec<Tri> = vec![];
        for (l, h) in &boxes { if h.z > z0 + 1.45 && l.z < z0 + 1.55 { let c = [Vec3::new(l.x, l.y, z0 + 1.5), Vec3::new(h.x, l.y, z0 + 1.5), Vec3::new(h.x, h.y, z0 + 1.5), Vec3::new(l.x, h.y, z0 + 1.5)]; let up = Vec3::new(0.0, 0.0, 0.5); let dn = -up; for k in 0..4 { let (a, b) = (c[k], c[(k + 1) % 4]); tall.push([a + dn, b + dn, b + up]); tall.push([a + dn, b + up, a + up]); } } }
        let near: Vec<Tri> = sh.walls.iter().chain(sh.leaves.iter()).chain(tall.iter()).filter(|t| { let (l, h) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2])); h.x > lo.x - 3.5 && l.x < hi.x + 3.5 && h.y > lo.y - 3.5 && l.y < hi.y + 3.5 }).cloned().collect();
        let mut low: Vec<Tri> = vec![];
        for (l, h) in &boxes { if l.z < z0 + 0.7 && h.z > z0 + 0.6 { let z = z0 + 0.45; let c = [Vec3::new(l.x, l.y, z), Vec3::new(h.x, l.y, z), Vec3::new(h.x, h.y, z), Vec3::new(l.x, h.y, z)]; let up = Vec3::new(0.0, 0.0, 0.5); for k in 0..4 { let (a, b) = (c[k], c[(k + 1) % 4]); low.push([a, b, b + up]); low.push([a, b + up, a + up]); } } }
        let near_views = near.clone();
        // what the room holds: a view is for showing it
        let pieces: Vec<(Vec3, f32)> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; !skip(nm) && !nm.contains("Ceiling") && !nm.starts_with("light") && !nm.contains("_light") && (d.bmax - d.bmin).max_element() > 0.2 }).filter_map(|d| { let c = (d.bmin + d.bmax) * 0.5; rm.tris.iter().any(|t| in_tri2(Vec3::new(c.x, c.y, z0), t)).then(|| (c, (d.bmax - d.bmin).truncate().length() * 0.5)) }).collect();
        // (how many of them a heading shows, and through what half angle all of those fit)
        let shows = |e: Vec3, f: Vec3, half: f32| -> (usize, f32) { let (mut n, mut need) = (0, 0f32); for (c, r) in &pieces { let d = (*c - e).truncate(); let l = d.length().max(0.01); let a = (d / l).dot(f.truncate()).clamp(-1.0, 1.0).acos(); if a < half && !sh.blocked(e, Vec3::new(c.x, c.y, e.z)) { n += 1; need = need.max(a + (*r / l).atan().min(0.5)); } } (n, need) };
        let half = 42f32.to_radians();
        let mut scored: Vec<(f32, Vec3, Vec3)> = vec![];
        let n_cands = cands.len(); let every = (n_cands / 40).max(1);
        for (dist, e) in cands.iter().step_by(every) {
            if *dist > 7.5 { continue; }
            let f0 = (tgt - *e).truncate().normalize_or_zero().extend(0.0);
            let mut best: Option<(f32, Vec3)> = None;
            for deg in [0.0f32, -20.0, 20.0, -35.0, 35.0] {
                let a = deg.to_radians();
                let f = Vec3::new(f0.x * a.cos() - f0.y * a.sin(), f0.x * a.sin() + f0.y * a.cos(), 0.0);
                let seen = shows(*e, f, half).0 as f32 / pieces.len().max(1) as f32;
                // (a big piece half out of the frame: a bed or a sofa cut off by its edge)
                let cut_off = pieces.iter().filter(|(c, r)| { if *r < 0.6 { return false } let d = (*c - *e).truncate(); let l = d.length().max(0.01); let a = (d / l).dot(f.truncate()).clamp(-1.0, 1.0).acos(); let w = (*r / l).atan(); a < half + w && a + w > half }).count() as f32;
                let q = view_quality(&near, &low, *e, f, half) - 0.01 * deg.abs() + 0.15 * dist + 3.0 * seen - 0.6 * cut_off;
                if best.map_or(true, |b| q > b.0) { best = Some((q, f)); }
            }
            if let Some((q, f)) = best { scored.push((q, *e, f)); }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let mut chosen: Vec<Vec3> = vec![]; let mut heads: Vec<Vec3> = vec![];
        // (other good spots, should the picture from the best turn out mostly bare wall)
        let mut spare: Vec<(Vec3, Vec3)> = vec![];
        for (_, e, f) in &scored { if spare.len() < 6 && !spare.iter().any(|(s, _)| (*s - *e).truncate().length() < 0.8) && (scored[0].1 - *e).truncate().length() >= 1.0 { spare.push((*e, *f)); } }
        for (q, e, f) in &scored {
            if chosen.len() >= 2 { break; }
            // (a second view only if it is good and elsewhere)
            if !chosen.is_empty() && (*q < 2.2 || (chosen[0] - *e).truncate().length() < 2.5) { continue; }
            chosen.push(*e); heads.push(*f);
        }
        // (every room gets a view: a closet or a hallway fails the rules above, so it takes the farthest clear spot in it and a wider lens)
        let mut lens = if rm.area < 7.0 { 14.0 } else { 18.0 };
        if chosen.is_empty() {
            let mut best: Option<(f32, Vec3)> = None;
            let mut others: Vec<(f32, Vec3)> = vec![];
            let step = 0.2; let mut y = lo.y + step / 2.0;
            while y < hi.y { let mut x = lo.x + step / 2.0; while x < hi.x {
                let p = Vec3::new(x, y, z0);
                if rm.tris.iter().any(|t| in_tri2(p, t)) {
                    let e = Vec3::new(x, y, z0 + 1.4); let dist = (e - tgt).truncate().length();
                    if !sh.blocked(e, tgt) && !inside(e, 0.05) && sh.clearance(e) > 0.12 && !leaf_blocks(e, tgt) { others.push((dist, e)); if best.map_or(true, |b| dist > b.0) { best = Some((dist, e)); } }
                }
                x += step; } y += step; }
            if let Some((_, e)) = best {
                chosen.push(e); lens = 13.0; heads.push((tgt - e).truncate().normalize_or_zero().extend(0.0));
                // (the room has no good spot, so the picture decides: other spots looking at the subject)
                others.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                let mut kept = vec![e];
                for (_, q) in others { if kept.len() < 4 && kept.iter().all(|k| (*k - q).truncate().length() >= 1.0) { kept.push(q); spare.push((q, (tgt - q).truncate().normalize_or_zero().extend(0.0))); } }
            }
        }
        // (the best spot's other headings too: a bare wall one way, the room's contents behind the lens)
        if let (Some(e), Some(f)) = (chosen.first(), heads.first()) { for (a, b) in [(-f.y, f.x), (f.y, -f.x), (-f.x, -f.y)] { spare.push((*e, Vec3::new(a, b, 0.0))); } }
        // (and for a view that is still poor, a wider search: spots across the floor, every heading; rendered only if needed, and the picture decides)
        let n_normal = spare.len();
        {
            let mut wide: Vec<(Vec3, Vec3)> = vec![];
            let step = 0.5; let mut y = lo.y + 0.3;
            while y < hi.y { let mut x = lo.x + 0.3; while x < hi.x {
                let e = Vec3::new(x, y, z0 + 1.4);
                if rm.tris.iter().any(|t| in_tri2(Vec3::new(x, y, z0), t)) && !inside(e, 0.1) && [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].iter().all(|(a, b)| ray_dist(&sh.walls, e, Vec3::new(*a, *b, 0.0), 0.3) >= 0.3) {
                    for h in 0..8 { let a = h as f32 * std::f32::consts::FRAC_PI_4 + 0.2; wide.push((e, Vec3::new(a.cos(), a.sin(), 0.0))); }
                }
                x += step; } y += step; }
            let every = (wide.len() / 48).max(1);
            spare.extend(wide.into_iter().step_by(every));
        }
        let tries: Vec<(usize, usize, Vec3, Option<Vec3>)> = chosen.iter().enumerate().map(|(k, e)| (k, 0, *e, heads.get(k).copied())).chain(spare.iter().filter(|(e, f)| !chosen.iter().zip(heads.iter()).any(|(c, h)| (*c - *e).truncate().length() < 1.0 && h.dot(*f) > 0.9)).enumerate().map(|(i, (e, f))| (0, i + 1, *e, Some(*f)))).collect();
        for &(k, alt, e0, h0) in &tries {
            let e = &e0;
            // (a level view, tilted down 8 degrees whatever the distance: walls stay upright, as an interior photograph's)
            let f = h0.unwrap_or_else(|| (tgt - *e).truncate().normalize_or_zero().extend(0.0));
            let look = *e + f * 2.0 + Vec3::new(0.0, 0.0, -0.28);
            // (the lens to the view: a narrow room's near walls leave the frame on a longer lens, as a photographer would frame it)
            let free_half = { let mut h = 42f32; for deg in [42f32, 38.0, 34.0, 30.0, 26.0] { let side = |sg: f32| { let a = (deg * sg).to_radians(); let d = Vec3::new(f.x * a.cos() - f.y * a.sin(), f.x * a.sin() + f.y * a.cos(), 0.0); ray_dist(&near_views, *e, d, 1.0) }; if side(1.0) >= 1.0 && side(-1.0) >= 1.0 { h = deg; break } h = deg; } h };
            let (seen_n, need) = shows(*e, f, 42f32.to_radians());
            let fit_half = free_half.max((need.to_degrees() + 3.0).min(42.0));
            let view_lens = if lens < 18.0 && fit_half >= 42.0 { lens } else { (18.0 / (fit_half.to_radians().tan())).max(18.0) };
            // (the tilt: 8 degrees at wide angle; on a longer lens down to what the room holds, or its floor would leave the frame)
            let reach = pieces.iter().map(|(c, _)| (*c - *e).truncate().length()).fold(9.0f32, f32::min).max(1.0);
            let tilt = if fit_half >= 40.0 { 0.14f32 } else { ((e.z - z0 - 0.75) / reach).clamp(0.14, 0.42) };
            let look = *e + f * 2.0 + Vec3::new(0.0, 0.0, -2.0 * tilt);
            if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("VIEW {}_{} shows {seen_n} of {} half {fit_half:.0}", rm.name, if k == 0 { "A" } else { "B" }, pieces.len()); }
            // (a view of this room: looking mostly out through a doorway, the next room's bath stood in for a hallway)
            let out_share = { let hv = (18.0 / view_lens).atan(); let mut o = 0; for k in 0..9 { let a = -hv + 2.0 * hv * k as f32 / 8.0; let d = Vec3::new(f.x * a.cos() - f.y * a.sin(), f.x * a.sin() + f.y * a.cos(), 0.0); let t = ray_dist(&sh.walls, *e, d, 9.0); let q = *e + d * (t - 0.1).max(0.0); if !rm.tris.iter().any(|tr| in_tri2(Vec3::new(q.x, q.y, z0), tr)) { o += 1; } } o as f32 / 9.0 };
            let mut cm = cam_look(*e, look, view_lens); cm.shown = share(*e, f, (18.0 / view_lens).atan()) - 2.0 * out_share; cm.key = key_seen(*e, f, (18.0 / view_lens).atan()); cm.name = if alt > n_normal { format!("{}_A~w{alt}", rm.name) } else if cut_done { format!("{}_A~{}", rm.name, 20 + alt + 8 * k) } else if alt > 0 { format!("{}_A~{alt}", rm.name) } else { format!("{}_{}", rm.name, if k == 0 { "A" } else { "B" }) };
            // (PREVIEW_CAMS=1: one line per camera, for rendering the same view in Blender)
            if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("CAM {} {} {} {} {} {} {} {}", cm.name, e.x, e.y, e.z, look.x, look.y, look.z, lens); }
            out.push(cm);
            if out.iter().filter(|c| !c.name.contains('~')).count() >= n { return out; }
        }
        // the last word: a view from above the walls, ceiling and near walls gone, nothing but this room. Not as if standing in it, but the whole room always shows.
        // (true rectangles only: a notch's corner has no plane to cut it away, and stands through the picture as a slab)
        // (and small rooms: a larger one comes out tiny in the frame, and its standing views are better even when they score poorly)
        if rm.area < 9.0 && (hi - lo).truncate().max_element() < 3.8 && rm.area / ((hi.x - lo.x) * (hi.y - lo.y)).max(0.01) > 0.985 {
            let c = (lo + hi) * 0.5;
            let (sx, sy) = (if rm.subj.x >= c.x { 1.0f32 } else { -1.0 }, if rm.subj.y >= c.y { 1.0f32 } else { -1.0 });
            let f = Vec3::new(sx, sy, 0.0).normalize();
            let r = ((hi - lo).truncate().length() * 0.5).max(1.0);
            let dist = 1.15 * r / 0.5625 + 1.6;
            let target = Vec3::new(c.x, c.y, z0 + 1.0);
            let e = target - f * dist * 0.72 + Vec3::new(0.0, 0.0, dist * 0.69);
            let mut cm = cam_look(e, target, 18.0);
            cm.near = Some(0.3);
            cm.clip = [sx, 0.0, 0.0, if sx > 0.0 { lo.x + 0.02 } else { -hi.x + 0.02 }];
            cm.clip2 = [0.0, sy, 0.0, if sy > 0.0 { lo.y + 0.02 } else { -hi.y + 0.02 }];
            cm.only = Some((lo, hi, rm.name.clone()));
            cm.name = format!("{}_A~h", rm.name);
            out.push(cm);
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
                    let sh = shell_of(s);
                    let c0 = (s.bmin + s.bmax) * 0.5; let mut d0 = (c0 - p).truncate().normalize_or_zero(); if d0 == glam::Vec2::ZERO { d0 = glam::Vec2::X; }
                    let base = d0.y.atan2(d0.x); let tgt = p + Vec3::Z * 0.9; let mut best: Option<Vec3> = None;
                    'o: for rad in [3.2f32, 2.6, 2.0, 1.6] {
                        let mut ks: Vec<i32> = (-8..8).collect(); ks.sort_by_key(|k| k.abs());
                        for k in ks { let a = base + k as f32 * std::f32::consts::PI / 8.0; let e = Vec3::new(p.x + a.cos() * rad, p.y + a.sin() * rad, 1.55);
                            if sh.blocked(e, tgt) || sh.blocked(e, p + Vec3::Z * 1.5) || sh.clearance(e) < 0.35 { continue; } best = Some(e); break 'o; }
                    }
                    let eye = best.unwrap_or(p + d0.extend(0.0) * 2.0 + Vec3::Z * 1.6);
                    let mut c = cam_look(eye, tgt, 24.0); c.name = format!("person{i}"); v.push(c);
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
struct Targets { hdr: wgpu::TextureView, nrm: wgpu::TextureView, depth: wgpu::TextureView, depth_tex: wgpu::Texture, dbuf: wgpu::Buffer, dbpr: u32, out: wgpu::Texture, readback: wgpu::Buffer, w: u32, h: u32, bpr: u32 }
struct Renderer {
    gpu: Gpu, ubuf: wgpu::Buffer, g_bgl: wgpu::BindGroupLayout, mat_bgl: wgpu::BindGroupLayout, post_bgl: wgpu::BindGroupLayout,
    shadow_pipe: wgpu::RenderPipeline, main_pipe: wgpu::RenderPipeline, glass_pipe: wgpu::RenderPipeline, post_pipe: wgpu::RenderPipeline,
    cmp_smp: wgpu::Sampler, white: wgpu::TextureView, smp: wgpu::Sampler, tg: Targets, last_depth: Option<Vec<f32>>,
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
    let depth_tex = mk(wgpu::TextureFormat::Depth32Float, rt | wgpu::TextureUsages::COPY_SRC);
    let depth = depth_tex.create_view(&Default::default());
    let dbpr = (w * 4 + 255) / 256 * 256;
    let dbuf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (dbpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let out = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let bpr = (o.res.0 * 4 + 255) / 256 * 256;
    let readback = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * o.res.1) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    Targets { hdr, nrm, depth, depth_tex, dbuf, dbpr, out, readback, w, h, bpr }
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
    Renderer { gpu: Gpu { dev, q }, ubuf, g_bgl, mat_bgl, post_bgl, shadow_pipe, main_pipe, glass_pipe, post_pipe, cmp_smp, white, smp, tg, last_depth: None }
}

fn frustum(vp: Mat4) -> [Vec4; 6] {
    let r = [vp.row(0), vp.row(1), vp.row(2), vp.row(3)];
    [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]]
}
fn visible(f: &[Vec4; 6], lo: Vec3, hi: Vec3) -> bool {
    for p in f { let v = Vec3::new(if p.x >= 0.0 { hi.x } else { lo.x }, if p.y >= 0.0 { hi.y } else { lo.y }, if p.z >= 0.0 { hi.z } else { lo.z }); if p.truncate().dot(v) + p.w < 0.0 { return false; } }
    true
}

/// a door leaf close before the lens fills the frame and shows nothing of the room: not drawn for that view
fn leaf_in_the_way(s: &Scene, d: &Draw, eye: Option<Vec3>, cut: Option<(Vec3, f32)>) -> bool {
    let Some(e) = eye else { return false };
    let nm = &s.glb.nodes[d.node].name;
    if !nm.starts_with("door") || nm.contains("frame") || nm.contains("casing") { return false }
    // (a cutaway: the door the cut passes through, or a leaf swung across it, would show its cross-section)
    if let Some((f, near)) = cut {
        let id = nm.split('.').next().unwrap_or(nm);
        if s.draws.iter().any(|o| { let n2 = &s.glb.nodes[o.node].name; if n2.split('.').next() != Some(id) { return false } let c = (o.bmin + o.bmax) * 0.5; let r = (o.bmax - o.bmin) * 0.5; let m = (c - e).dot(f); let h = r.x * f.x.abs() + r.y * f.y.abs() + r.z * f.z.abs(); m - h < near }) { return true }
    }
    // (the whole door goes with its leaf: a handle left behind floated on the wall)
    let id = nm.split('.').next().unwrap_or(nm);
    s.draws.iter().any(|o| { let n2 = &s.glb.nodes[o.node].name; n2.split('.').next() == Some(id) && !n2.contains("frame") && !n2.contains("casing") && { let d = (e.clamp(o.bmin, o.bmax) - e).truncate().length(); let w = (o.bmax - o.bmin).truncate().max_element(); d < 1.5 || (d < 2.6 && w / d > 0.33) } })
}
/// the near walls of a view from above, as (sx, x offset, sy, y offset): what lies under them goes with them
fn cut_plane(eye: Option<Vec3>, lo: &Vec3, hi: &Vec3) -> (f32, f32, f32, f32) {
    let e = eye.unwrap_or(Vec3::ZERO); let c = (*lo + *hi) * 0.5;
    let (sx, sy) = (if e.x < c.x { 1.0 } else { -1.0 }, if e.y < c.y { 1.0 } else { -1.0 });
    (sx, if sx > 0.0 { lo.x } else { -hi.x }, sy, if sy > 0.0 { lo.y } else { -hi.y })
}
fn draw_all<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, glass: bool, shadow: bool, fr: Option<&[Vec4; 6]>, eye: Option<Vec3>, cut: Option<(Vec3, f32)>, only: Option<&(Vec3, Vec3, String)>) {
    for d in &s.draws {
        if shadow { if !d.shadow { continue; } } else if d.glass != glass { continue; }
        if leaf_in_the_way(s, d, eye, cut) { continue; }
        // (PREVIEW_HIDE=a,b: parts not drawn, to see which one a fault is in)
        if let Ok(h) = std::env::var("PREVIEW_HIDE") { let nm = &s.glb.nodes[d.node].name; if h.split(',').any(|x| x == nm) { continue; } }
        // (PREVIEW_NOSHELL=1: no walls, ceilings, openings: nothing stands between an asset close-up and its asset)
        if std::env::var_os("PREVIEW_NOSHELL").is_some() {
            let low = s.glb.nodes[d.node].name.to_lowercase();
            if low.contains(".wall") || low.contains("ceiling") || low.starts_with("door") || (low.starts_with("window") && !low.starts_with("windowseat")) || low.contains("curtain") || low.contains("skirting") || low.starts_with("outdoor") || low.starts_with("exterior") || low.contains(".glass") || low.contains(".rod") { continue; }
        }
        if let Some((lo, hi, room)) = only {
            let nm = s.glb.nodes[d.node].name.replace('/', "_");
            let low = nm.to_lowercase();
            if low.contains(".ceiling") || low.contains("ceiling") || low.starts_with("outdoor") || low.starts_with("exterior") || low.contains("skirting") || low.contains("pendant") || low.contains("_light") || low.starts_with("beam") { continue; }
            // (door leaves too: one swung into the room stood through the picture like a slab of wall. The frames stay, to show where the doors are)
            if low.starts_with("door") && !low.starts_with("doorclear") && !low.contains("frame") && !low.contains("casing") { continue; }
            if low.contains(".wall") || low.contains(".floor") { if !nm.starts_with(&format!("{room}.")) { continue; } }
            else if (low.starts_with("door") || low.starts_with("window") || low.contains("curtain")) && { let c = (d.bmin + d.bmax) * 0.5; let n = [cut_plane(eye, lo, hi)]; n[0].0 * c.x < n[0].1 + 0.3 * 1.0 && n[0].0 != 0.0 || n[0].2 * c.y < n[0].3 + 0.3 && n[0].2 != 0.0 } { continue; }
            else { let c = (d.bmin + d.bmax) * 0.5; if c.x < lo.x - 0.15 || c.x > hi.x + 0.15 || c.y < lo.y - 0.15 || c.y > hi.y + 0.15 { continue; } }
        }
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

fn render(r: &mut Renderer, s: &mut Scene, cam: &Cam, o: &Opts, xp: f32) -> image::RgbaImage {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let aspect = o.res.0 as f32 / o.res.1 as f32;
    // clip range: 0.05-200 m for a house; outdoor scenes (terrain to a few km) push the far plane out and the near plane to 0.1 m
    let (zn, zf) = clip_range(s);
    let zn = cam.near.unwrap_or(zn);
    let proj = match cam.ortho { Some(w) => orthographic_rh(-w / 2.0, w / 2.0, -w / aspect / 2.0, w / aspect / 2.0, zn, zf),
        None => perspective_rh(2.0 * ((cam.fov_x / 2.0).tan() / aspect).atan(), aspect, zn, zf) };
    let view = cam.view.inverse(); let vp = proj * view; let eye = cam.view.w_axis.truncate(); let fwd = -cam.view.z_axis.truncate().normalize();
    let (sun_az, sun_el, sun_i, ambient) = s.lit.map_or((o.sun.0, o.sun.1, o.sun_i, o.ambient), |l| (l.0, l.1, l.2, l.3));
    let (az, el) = (sun_az.to_radians(), sun_el.to_radians());
    let sdir = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
    let sr = ((s.bmax - s.bmin) * 0.5).length() * 2.0;
    let mut gl = Globals { viewproj: vp.to_cols_array_2d(), inv_viewproj: vp.inverse().to_cols_array_2d(), sun_vp: s.sun_vp.to_cols_array_2d(),
        sun_dir: sdir.extend(0.0).into(), sun_col: [3.2 * sun_i, 2.9 * sun_i, 2.5 * sun_i, 0.0], sky: [0.62 * ambient, 0.72 * ambient, 0.9 * ambient, 0.0], ground: [0.28 * ambient, 0.25 * ambient, 0.22 * ambient, 0.0],
        cam_pos: eye.extend(1.0).into(), cam_fwd: fwd.extend(0.0).into(), params: [s.lights.len().min(32) as f32, o.exposure * xp, if cam.near.is_some() { o.ao * 0.45 } else { o.ao }, 0.6], clip2: cam.clip2, clip: cam.clip, params2: [1.0 / SHADOW as f32, o.ss as f32, 1.0, sr],
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
            draw_all(&mut rp, s, false, true, None, None, None, None);
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
        rp.set_pipeline(&r.main_pipe); draw_all(&mut rp, s, false, false, Some(&fr), Some(eye), cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) }), cam.only.as_ref());
        rp.set_pipeline(&r.glass_pipe); draw_all(&mut rp, s, true, false, Some(&fr), Some(eye), cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) }), cam.only.as_ref());
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
    if true {
        enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &tg.depth_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::DepthOnly },
            wgpu::TexelCopyBufferInfo { buffer: &tg.dbuf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tg.dbpr), rows_per_image: Some(tg.h) } }, wgpu::Extent3d { width: tg.w, height: tg.h, depth_or_array_layers: 1 });
    }
    q.submit([enc.finish()]);
    if true {
        let ds = tg.dbuf.slice(..); ds.map_async(wgpu::MapMode::Read, |_| {});
        dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = ds.get_mapped_range().unwrap();
        let (n, f) = clip_range(s); let n = cam.near.unwrap_or(n); let ss = o.ss as usize; let mut out = vec![0f32; (o.res.0 * o.res.1) as usize];
        for y in 0..o.res.1 as usize { for x in 0..o.res.0 as usize {
            // the supersampled texel nearest the pixel centre (ss even: the texel just right/below of the centre)
            let (sx, sy) = (x * ss + ss / 2, y * ss + ss / 2); let off = sy * tg.dbpr as usize + sx * 4;
            let d = f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
            out[y * o.res.0 as usize + x] = if d >= 1.0 { 0.0 } else { n * f / (f - d * (f - n)) };
        } }
        drop(data); tg.dbuf.unmap(); r.last_depth = Some(out);
    }
    let sl = tg.readback.slice(..);
    sl.map_async(wgpu::MapMode::Read, |_| {});
    dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = sl.get_mapped_range().unwrap();
    let mut img = image::RgbaImage::new(o.res.0, o.res.1);
    for y in 0..o.res.1 as usize { let row = &data[y * tg.bpr as usize..y * tg.bpr as usize + o.res.0 as usize * 4]; img.as_mut()[y * o.res.0 as usize * 4..(y + 1) * o.res.0 as usize * 4].copy_from_slice(row); }
    drop(data); tg.readback.unmap();
    img
}

fn write_npy(p: &Path, res: (u32, u32), d: &[f32]) {
    use std::io::Write;
    if let Some(pp) = p.parent() { std::fs::create_dir_all(pp).ok(); }
    let mut hdr = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({}, {}), }}", res.1, res.0);
    while (10 + hdr.len() + 1) % 64 != 0 { hdr.push(' '); } hdr.push('\n');
    let mut f = std::io::BufWriter::new(std::fs::File::create(p).unwrap());
    f.write_all(b"\x93NUMPY\x01\x00").unwrap(); f.write_all(&(hdr.len() as u16).to_le_bytes()).unwrap(); f.write_all(hdr.as_bytes()).unwrap();
    f.write_all(bytemuck::cast_slice(d)).unwrap();
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
        let mut best_detail: HashMap<String, f32> = HashMap::new();
        for (i, c) in cams.iter().enumerate() {
            // (only a poor view looks further)
            if let Some((base, alt)) = c.name.split_once('~') { let enough = if alt.starts_with('w') { 0.5 } else if alt == "h" { 0.45 } else { 1.0 }; if best_detail.get(&format!("{}_{}", s.name, base)).map_or(true, |d| *d >= enough) { continue } }
            let t = Instant::now();
            let mut img = render(&mut r, &mut s, c, &o, 1.0);
            // a view too bright or too dark for its lights: exposed again to suit, as a camera would
            let luma = |im: &image::RgbaImage| { let (mut sum, mut hi, n) = (0f32, 0u32, im.pixels().len() as f32); for p in im.pixels() { let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0; sum += l; if l > 0.97 { hi += 1; } } (sum / n, hi as f32 / n) };
            let (m0, h0) = luma(&img);
            if c.ortho.is_none() && (m0 > 0.74 || h0 > 0.10 || m0 < 0.38) {
                // (the tonemap's middle is steep: a little goes a long way)
                let xp = if m0 < 0.38 { (0.5 / m0.max(0.1)).min(1.8) } else { (0.66 / m0).powf(2.2).clamp(0.45, 0.95) };
                img = render(&mut r, &mut s, c, &o, xp);
            }
            if o.verbose { eprintln!("  cam {} {:.3}s", c.name, t.elapsed().as_secs_f32()); }
            let nm = if c.name.is_empty() { format!("cam{i}") } else { c.name.clone() };
            // a view that is mostly bare: other spots for it are tried, the one with most to see kept; a second view only if it has something to show
            // (what stands right before the lens counts against it: a wall, a door, a bottle on a shelf filling the frame)
            let near = r.last_depth.as_ref().map_or(0.0, |d| { let (mut n, mut c) = (0f32, 0f32); for v in d.iter().step_by(11) { c += 1.0; if *v > 0.0 && *v < 0.9 { n += 1.0; } } n / c.max(1.0) });
            // (and what matters most is that it shows the room: a detailed view of a curtain is still no view of a bathroom)
            // (a bare picture earns none of that: what the room holds may be behind the wall it shows)
            let raw = picture_detail(&img);
            // (nor a close-up: book spines filling the frame are detail, and show no room)
            let close = r.last_depth.as_ref().map_or(0.0, |d| { let (mut n, mut c) = (0f32, 0f32); for v in d.iter().step_by(11) { c += 1.0; if *v > 0.0 && *v < 1.5 { n += 1.0; } } n / c.max(1.0) });
            let detail = raw - 1.2 * near + 0.8 * c.shown * (raw / 0.5).min(1.0) - 1.2 * flat_columns(&img) - if c.key { 0.0 } else { 0.8 } - 2.0 * (close - 0.45).max(0.0);
            if let Some((base, alt)) = nm.split_once('~') {
                let key = format!("{}_{}", s.name, base);
                if let Some(fr) = frames.iter_mut().find(|(k, _)| *k == key) {
                    if alt == "h" { eprintln!("OVERHEAD {key}"); } if alt == "h" || detail > best_detail.get(&key).copied().unwrap_or(0.0) + 0.03 { fr.1 = img; best_detail.insert(key, detail); if o.depth { write_npy(&o.out.join(format!("{}_{}.depth.npy", s.name, base)), o.res, r.last_depth.as_ref().unwrap()); } }
                }
                if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("DETAIL {nm} {detail:.2}"); }
                continue;
            }
            if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("DETAIL {nm} {detail:.2}"); }
            if nm.ends_with("_B") && detail < 0.75 { continue }
            best_detail.insert(format!("{}_{}", s.name, nm), detail);
            if o.depth { write_npy(&o.out.join(format!("{}_{}.depth.npy", s.name, nm)), o.res, r.last_depth.as_ref().unwrap()); }
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
