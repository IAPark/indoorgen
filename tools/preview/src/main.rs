//! preview: headless real-time (wgpu/Metal) previewer for infinigen-rs glb scenes.
//! Flat/textured base colors, sun + shadow map, point lights, SSAO, ACES tonemap. Many cameras per process.
mod bake;
mod glb;
mod rt;
mod vox;
use glam::{Mat4, Vec3, Vec4, Vec2};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use wgpu::util::DeviceExt;

const SHADOW: u32 = 4096;
/// the size of a candidate view on the voxel grid
const VOX_W: u32 = 120; const VOX_H: u32 = 68;
const SRC: &str = include_str!("shader.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewproj: [[f32; 4]; 4], inv_viewproj: [[f32; 4]; 4], sun_vp: [[f32; 4]; 4],
    sun_dir: [f32; 4], sun_col: [f32; 4], sky: [f32; 4], ground: [f32; 4], cam_pos: [f32; 4], cam_fwd: [f32; 4],
    params: [f32; 4], clip2: [f32; 4], clip: [f32; 4], params2: [f32; 4], lpos: [[f32; 4]; 32], lcol: [[f32; 4]; 32],
}
/// The index of refraction of a part's glass (0: none given).
fn sn_ior(side: &serde_json::Value, sn: Option<&serde_json::Value>) -> f64 { sn.and_then(|x| x["graph"].as_u64()).and_then(|i| side["graphs"][i as usize]["out"]["ior"].as_f64()).unwrap_or(0.0) }
/// A flat mirror's pane, by its material.
fn is_mirror(name: &str) -> bool { name.to_lowercase().contains("mirror") }
/// Cloth, by the shader names `tools/render/render_rust.py` treats as cloth (the shader's, not the part's: a "sofa_fabric" can be leather): there it gets a soft sheen, which is why dark cushions are never near black.
fn is_cloth(name: &str) -> bool {
    let n = name.to_lowercase();
    !["glass", "mirror", "lamp", "bulb", "people", "window"].iter().any(|x| n.contains(x))
        && ["fabric", "knit", "velvet", "rug", "towel", "cloth", "bedding", "linen", "comforter", "pillow", "curtain", "sheet", "mattress", "duvet", "throw", "blanket", "cushion", "bed_panel", "upholster", "headboard"].iter().any(|x| n.contains(x))
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatU { color: [f32; 4], emissive: [f32; 4], flags: [f32; 4], obj: [[f32; 4]; 4], p: [[f32; 4]; 48] }

#[derive(Clone)]
struct Cam { name: String, view: Mat4, fov_x: f32, ortho: Option<f32>, /** a cutaway's clipping distance: what stands before it (a wall) is not drawn */ near: Option<f32>, /** a cutaway's vertical clip plane (normal, offset) */ clip: [f32; 4], clip2: [f32; 4], /** a view from above: only this room is drawn */ only: Option<(Vec3, Vec3, String, [[f32; 4]; 2])>, /** the share of what the room holds in this view, by size */ shown: f32, /** the room's main piece is in the view */ key: bool, /** share of the room's floor in view, walls considered */ cover: f32, /** share of the frame's width taken by a large piece cut off by its edge */ slab: f32 }

struct Opts {
    inputs: Vec<PathBuf>, out: PathBuf, cams: Vec<CamSpec>, sheet: bool, res: (u32, u32), ss: u32,
    sun: (f32, f32), exposure: f32, ao: f32, no_ceiling: bool, ncams: usize, verbose: bool, ambient: f32, sun_i: f32, light_scale: f32, cloth: Option<PathBuf>, depth: bool, sidecar_lighting: bool, rt: Option<rt::Quality>, seed: u64, bake: Option<String>,
}
#[derive(Clone)]
enum CamSpec { Json(PathBuf), At([f32; 6], f32), Person(usize), Top, Named(String) }

fn parse_args() -> Opts {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts { inputs: vec![], out: PathBuf::from("preview.png"), cams: vec![], sheet: false, res: (960, 540), ss: 2,
        sun: (215.0, 38.0), exposure: 0.5, ao: 1.0, no_ceiling: false, ncams: 3, verbose: false, ambient: 1.0, sun_i: 1.0, light_scale: 1.0, cloth: None, depth: false, rt: None, seed: 0, bake: None, sidecar_lighting: false };
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
            "--rt" => { let q = rt::Quality::parse(&val()); o.ss = q.ss; o.rt = Some(q); }
            "--seed" => o.seed = val().parse().unwrap(),
            "--bake" => { o.bake = Some(val()); bake::ON.store(true, std::sync::atomic::Ordering::Relaxed); o.rt = Some(rt::Quality::parse("full")); o.ss = 1; }
            "-v" => o.verbose = true,
            _ => o.inputs.push(PathBuf::from(k)),
        }
        i += 1;
    }
    assert!(!o.inputs.is_empty(), "usage: preview [--cam cam.json | --cams N|cam0 | --at ex,ey,ez,tx,ty,tz[,lens] | --person I | --top]... [--sheet] [--res WxH] [--ss N] [--sun az,el] [--exposure E] [--ao A] [--no-ceiling] -o OUT scene.glb...");
    o
}

// ---------------- scene on the GPU ----------------
struct Draw { pane: bool, mask: bool, mat: usize, node: usize, pos: (usize, u64, u64), nrm: Option<(usize, u64, u64)>, uv: Option<(usize, u64, u64)>, idx: (usize, u64, u64), u16: bool, icount: u32, vcount: u32, mat_bg: usize, pipe: Option<usize>, glass: bool, shadow: bool, bmin: Vec3, bmax: Vec3 }
struct Light { pos: Vec3, color: Vec3 }
struct Scene {
    /// (azimuth deg, elevation deg, sun intensity, ambient) from the sidecar `variation.lighting` (--sidecar-lighting)
    lit: Option<(f32, f32, f32, f32)>,
    name: String, bufs: Vec<wgpu::Buffer>, zeros: wgpu::Buffer, draws: Vec<Draw>, mat_bgs: Vec<wgpu::BindGroup>, pipes: Vec<wgpu::RenderPipeline>, /** draw plain colours: views are scored on these (texture would count as detail) */ flat: bool, lights: Vec<Light>,
    bmin: Vec3, bmax: Vec3, side: Value, glb: glb::Glb, sun_vp: Mat4, shadow_view: wgpu::TextureView, shadow_done: bool, rt: Option<rt::SceneData>, grid: Option<(vox::Grid, Vec<Vec3>, Vec<bool>)>,
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
/// A black body's colour in linear Rec.709 at luminance 1, as Blender's node gives (Planck's law over the CIE curves, Wyman's fit). The other one below is an 8-bit display colour: as light, it made lamps far too white
fn blackbody_linear(t: f32) -> Vec3 {
    let g = |x: f32, m: f32, s1: f32, s2: f32| { let d = (x - m) / if x < m { s1 } else { s2 }; (-0.5 * d * d).exp() };
    let (mut x_, mut y_, mut z_) = (0f64, 0f64, 0f64);
    for l in (380..=780).step_by(5) {
        let l = l as f32;
        let m = l as f64 * 1e-9;
        let b = 1.0 / (m.powi(5) * ((1.4388e-2 / (m * t as f64)).exp() - 1.0));
        x_ += b * (1.056 * g(l, 599.8, 37.9, 31.0) + 0.362 * g(l, 442.0, 16.0, 26.7) - 0.065 * g(l, 501.1, 20.4, 26.2)) as f64;
        y_ += b * (0.821 * g(l, 568.8, 46.9, 40.5) + 0.286 * g(l, 530.9, 16.3, 31.1)) as f64;
        z_ += b * (1.217 * g(l, 437.0, 11.8, 36.0) + 0.681 * g(l, 459.0, 26.0, 13.8)) as f64;
    }
    let (x_, y_, z_) = ((x_ / y_) as f32, 1.0f32, (z_ / y_) as f32);
    Vec3::new(3.2406 * x_ - 1.5372 * y_ - 0.4986 * z_, -0.9689 * x_ + 1.8758 * y_ + 0.0415 * z_, 0.0557 * x_ - 0.2040 * y_ + 1.0570 * z_).max(Vec3::ZERO)
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
fn photo_art(seed: u64, node: &str, ex: f32, ey: f32, ez: f32) -> Option<image::RgbaImage> {
    let dir = std::env::var("ART_PHOTOS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("work/assets/art_photos"));
    let mut files: Vec<String> = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|f| f.ends_with(".jpg")).collect();
    files.sort();
    let h = rt::crc32(format!("{seed}:{}", node.split('#').next().unwrap_or(node)).as_bytes());
    if h % 100 < 15 || files.is_empty() { return None }
    let asp = ex.max(ey) / ez.max(1e-3);
    let best = [("0.75", 0.75f32), ("1.0", 1.0), ("1.33", 1.33), ("1.5", 1.5)].into_iter().min_by(|a, b| (a.1 / asp.max(1e-3)).ln().abs().partial_cmp(&(b.1 / asp.max(1e-3)).ln().abs()).unwrap())?.0;
    let cands: Vec<&String> = files.iter().filter(|f| f.contains(&format!("_{best}_"))).collect();
    if cands.is_empty() { return None }
    let f = cands[(h as usize / 100) % cands.len()];
    let mut img = image::open(dir.join(f)).ok()?.to_rgba8();
    image::imageops::flip_vertical_in_place(&mut img);
    Some(img)
}
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

fn load_scene(gpu: &Gpu, path: &Path, mat_bgl: &wgpu::BindGroupLayout, g_bgl: &wgpu::BindGroupLayout, white: &wgpu::TextureView, smp: &wgpu::Sampler, opts: &Opts) -> Scene {
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
    let mut bufs: Vec<wgpu::Buffer> = glb.chunks.iter().map(|c| gpu.dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: c,
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
    let mut mat_bgs: Vec<wgpu::BindGroup> = vec![]; let mut bg_key: HashMap<(usize, String, u32, usize), usize> = HashMap::new();
    // material graphs (ig_shade): one pipeline per graph structure, the numbers in each material's uniforms
    let emitted: Vec<Option<ig_shade::wgsl::Emitted>> = if std::env::var_os("PREVIEW_FLAT").is_some() { vec![] } else { side["graphs"].as_array().map(|a| a.iter().map(ig_shade::wgsl::emit).collect()).unwrap_or_default() };
    let mut pipes: Vec<wgpu::RenderPipeline> = vec![]; let mut pipe_of: HashMap<String, usize> = HashMap::new();
    let side_nodes: HashMap<&str, &serde_json::Value> = side["nodes"].as_array().map(|a| a.iter().filter_map(|n| Some((n["name"].as_str()?, n))).collect()).unwrap_or_default();
    let mut draws = vec![]; let mut bmin = Vec3::splat(1e9); let mut bmax = Vec3::splat(-1e9); let (mut sh_lo, mut sh_hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for (ni, n) in glb.nodes.iter().enumerate() {
        let spec = specs.get(&n.name);
        let cls = spec.and_then(|s| s["spec"]["cls"].as_str()).unwrap_or("");
        let tex = if cls.starts_with("people.") { spec.and_then(|s| s["spec"]["kwargs"]["color"].as_str()).unwrap_or("").to_string() } else { String::new() };
        let lname = n.name.to_lowercase();
        let is_ceil = lname.contains("ceiling");
        // (a shower's panes too: drawn solid, a shower looked like a white wardrobe)
        let glass = (lname.ends_with(".glass") && !lname.contains("wineglass")) || n.prims.iter().any(|p| glb.mats[p.mat.min(glb.mats.len().saturating_sub(1))].name.starts_with("shower_glass"))
            || side_nodes.get(n.name.as_str()).and_then(|x| x["graph"].as_u64()).map_or(false, |i| side["graphs"][i as usize]["out"]["glass"].as_bool() == Some(true) && !lname.contains("wineglass"));
        for p in &n.prims {
            let m = &glb.mats[p.mat.min(glb.mats.len().saturating_sub(1))];
            let mi = p.mat.min(glb.mats.len().saturating_sub(1));
            let tex = if m.tex.is_some() { format!("glb:{mi}") } else { tex.clone() };
            // (art and mirrors are the renderer's shaders: here a painting of its own for each, and a silver sheen, not flat tan and white boards)
            // (path traced: as Blender, a print where its rule gives this picture one, and otherwise the material's own shader)
            // (in the part's own frame, the sidecar's pose: render_rust.py moves the mesh into it, and takes the picture's size and direction from there)
            let art_frame = (opts.rt.is_some() && matches!(cls, "art.Art" | "art.DarkArt")).then(|| {
                let inv = side_nodes.get(n.name.as_str()).and_then(|x| x["pose"].as_array()).map(|pr| {
                    let g_ = |r: usize, c: usize| pr[r][c].as_f64().unwrap_or(0.0) as f32;
                    Mat4::from_cols_array_2d(&[[g_(0, 0), g_(1, 0), g_(2, 0), g_(3, 0)], [g_(0, 1), g_(1, 1), g_(2, 1), g_(3, 1)], [g_(0, 2), g_(1, 2), g_(2, 2), g_(3, 2)], [g_(0, 3), g_(1, 3), g_(2, 3), g_(3, 3)]]).inverse()
                }).unwrap_or(Mat4::IDENTITY);
                let c = &glb.chunks[p.pos.chunk][p.pos.off as usize..(p.pos.off + p.pos.len) as usize];
                let loc: Vec<Vec3> = (0..p.vcount as usize).map(|k| { let f = |d: usize| f32::from_le_bytes(c[(k * 3 + d) * 4..(k * 3 + d) * 4 + 4].try_into().unwrap()); inv.transform_point3(Vec3::new(f(0), f(1), f(2))) }).collect();
                let (lo, hi) = loc.iter().fold((Vec3::splat(1e9), Vec3::splat(-1e9)), |(a, b), v| (a.min(*v), b.max(*v)));
                (loc, lo, hi)
            });
            let rt_print = art_frame.as_ref().and_then(|(_, lo, hi)| photo_art(opts.seed, &n.name, hi.x - lo.x, hi.y - lo.y, hi.z - lo.z));
            let is_art = if opts.rt.is_some() { rt_print.is_some() } else { m.name == "art" || m.name.starts_with("art~") || m.name.starts_with("gallery_art") };
            let shown = is_art;
            let tex = if shown {
                let k = format!("{}:{}", m.name, n.name);
                if !tex_views.contains_key(&k) {
                    // (path traced: Blender's own choice of print for this picture, render_rust.py _photo_art; else a painting of its own)
                    let img = rt_print.clone().unwrap_or_else(|| painting(&format!("{}:{}", path.display(), n.name)));
                    tex_views.insert(k.clone(), load_texture(&gpu.dev, &gpu.q, img));
                }
                k
            } else { tex };
            let real_glass = glass && sn_ior(&side, side_nodes.get(n.name.as_str()).copied()) > 1.2;
            let flags = if cls == "people.hair" || m.mask { 1u32 } else if glass { 2 } else { 0 };
            let sn = side_nodes.get(n.name.as_str());
            let graph = if glass || shown || flags != 0 || m.tex.is_some() { None } else { sn.and_then(|x| x["graph"].as_u64()).and_then(|i| emitted.get(i as usize)).and_then(|e| e.as_ref()) };
            let pipe = graph.map(|e| *pipe_of.entry(e.code.clone()).or_insert_with(|| {
                // (path traced: Blender's bump filter draws a 2 mm joint about a pixel wide, whatever the distance: here it would all but vanish)
                let src = bake::shader(SRC).replace("/*LIB*/", &ig_shade::wgsl::LIB.replace("clamp(mortar / w, 0.0, 1.0)", "clamp(mortar * select(1.0, 3.5, g.params2.w > 0.5) / w, 0.0, 1.0)")).replace("/*SURF*/", &format!("pbr = true;\n  let obj = (m.obj * vec4<f32>(v.wpos, 1.0)).xyz;\n{}", e.code));
                let sm = gpu.dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
                pipes.push(make_main_pipe(&gpu.dev, &sm, g_bgl, mat_bgl, false));
                pipes.len() - 1
            }));
            // (an appliance's door: dark in Cycles, where nothing lights the inside. Here the racks behind it stood fully lit)
            let dark_glass = opts.rt.is_none() && glass && ["glass:", "front:", "blackglass:", "superblackglass:"].iter().any(|q| m.name.starts_with(q));
            let key = (p.mat, tex.clone(), flags, if graph.is_some() { ni } else { usize::MAX });
            let bi = *bg_key.entry(key).or_insert_with(|| {
                let mut u = MatU { color: if dark_glass { [0.02, 0.02, 0.025, 1.0] } else if shown { [1.0, 1.0, 1.0, 1.0] } else { m.color }, emissive: [m.emissive[0], m.emissive[1], m.emissive[2], if lname.contains(".floor") { 2.0 } else { sn.and_then(|x| x["graph"].as_u64()).and_then(|i| side["graphs"][i as usize]["out"]["transmission"].as_f64()).unwrap_or(0.0) as f32 }], flags: [flags as f32, if std::env::var_os("PREVIEW_NOBUMP").is_some() { 1.0 } else { 0.0 }, if dark_glass { 0.85 } else if opts.rt.is_none() && m.name.starts_with("table_top") { 0.45 } else { 0.0 }, if is_cloth(sn.and_then(|x| x["graph"].as_u64()).and_then(|i| side["graphs"][i as usize]["name"].as_str()).unwrap_or(&m.name)) { 0.25 } else if is_mirror(&m.name) && !glass { -1.0 } else if real_glass { -2.0 } else { 0.0 }], obj: Mat4::IDENTITY.to_cols_array_2d(), p: [[0.0; 4]; 48] };
                if graph.is_none() { u.p[47] = if shown { [0.45, 0.0, 0.0, 0.0] } else { [m.rough, m.metal, 0.0, 0.0] }; }
                if let Some(e) = graph {
                    for (i, v) in e.params.iter().enumerate() { u.p[i / 4][i % 4] = *v; }
                    // the object frame the graph's coordinates are in (the sidecar's pose, rows of a 4x4)
                    if let Some(pr) = sn.and_then(|x| x["pose"].as_array()) {
                        let g_ = |r: usize, c: usize| pr[r][c].as_f64().unwrap_or(0.0) as f32;
                        let pm = Mat4::from_cols_array_2d(&[[g_(0, 0), g_(1, 0), g_(2, 0), g_(3, 0)], [g_(0, 1), g_(1, 1), g_(2, 1), g_(3, 1)], [g_(0, 2), g_(1, 2), g_(2, 2), g_(3, 2)], [g_(0, 3), g_(1, 3), g_(2, 3), g_(3, 3)]]);
                        u.obj = pm.inverse().to_cols_array_2d();
                    }
                }
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
            if !lname.starts_with("outdoor") { sh_lo = sh_lo.min(lo); sh_hi = sh_hi.max(hi); }
            let vw = |v: &glb::View| (v.chunk, v.off, v.len);
            // (a print laid on as Blender does it, from the part's own extents along the wall and up: art meshes need have no coordinates of their own)
            let art_uv = (opts.rt.is_some() && is_art).then(|| {
                let (loc, lo, hi) = art_frame.as_ref().unwrap();
                let ax = if hi.x - lo.x >= hi.y - lo.y { 0usize } else { 1 };
                let mut uv = Vec::with_capacity(p.vcount as usize * 8);
                for v in loc {
                    uv.extend_from_slice(&((v[ax] - lo[ax]) / (hi[ax] - lo[ax]).max(1e-4)).to_le_bytes());
                    uv.extend_from_slice(&((v.z - lo.z) / (hi.z - lo.z).max(1e-4)).to_le_bytes());
                }
                bufs.push(gpu.dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &uv, usage: wgpu::BufferUsages::VERTEX }));
                (bufs.len() - 1, 0u64, uv.len() as u64)
            });
            draws.push(Draw { pane: real_glass, mask: flags == 1, mat: mi, node: ni, pos: vw(&p.pos), nrm: p.nrm.as_ref().map(vw), uv: art_uv.or(p.uv.as_ref().map(vw)), idx: vw(&p.idx), u16: p.idx_u16, icount: p.icount, vcount: p.vcount,
                mat_bg: bi, pipe, glass, shadow: !glass && !(opts.no_ceiling && is_ceil), bmin: lo, bmax: hi });
        }
    }
    let lights: Vec<Light> = side["lights"].as_array().map(|ls| ls.iter().map(|l| {
        let loc = &l["loc"]; let w = l["watt"].as_f64().unwrap_or(50.0) as f32; let tk = l["temperature"].as_f64().unwrap_or(5000.0) as f32; let c = if opts.rt.is_some() { blackbody_linear(tk) } else { blackbody(tk) };
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
    // (the house alone: with 5 km of terrain in the bounds a shadow texel was nearly 2 m, and sun on the walls came out in big blocks)
    let (sh_lo, sh_hi) = if sh_hi.x >= sh_lo.x { (sh_lo, sh_hi) } else { (bmin, bmax) };
    let c = (sh_lo + sh_hi) * 0.5; let r = ((sh_hi - sh_lo) * 0.5).length();
    let sv = look_at_rh(c + sdir * r * 2.0, c, Vec3::Z);
    let sp = orthographic_rh(-r, r, -r, r, 0.0, r * 4.0);
    let sh = gpu.dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: SHADOW, height: SHADOW, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
    if opts.verbose { eprintln!("  scene on GPU {:.2}s ({} draws, {} lights, bbox {:?}..{:?})", t.elapsed().as_secs_f32(), draws.len(), lights.len(), bmin, bmax); }
    Scene { lit, name: path.file_stem().unwrap().to_string_lossy().to_string(), bufs, zeros, draws, mat_bgs, pipes, flat: false, lights, bmin, bmax, side, glb,
        sun_vp: sp * sv, shadow_view: sh.create_view(&wgpu::TextureViewDescriptor::default()), shadow_done: false, rt: None, grid: None }
}

// ---------------- cameras ----------------
fn cam_look(eye: Vec3, tgt: Vec3, lens: f32) -> Cam {
    let f = (tgt - eye).normalize(); let r = f.cross(Vec3::Z).normalize(); let u = r.cross(f);
    let m = Mat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), eye.extend(1.0));
    Cam { name: String::new(), view: m, fov_x: 2.0 * (18.0f32 / lens).atan(), ortho: None, near: None, clip: [0.0; 4], clip2: [0.0; 4], only: None, shown: 0.0, key: true, cover: 1.0, slab: 0.0 }
}
fn mat_from_json(v: &Value) -> Mat4 {
    let mut c = [0f32; 16];
    for r in 0..4 { for k in 0..4 { c[k * 4 + r] = v[r][k].as_f64().unwrap() as f32; } }
    Mat4::from_cols_array(&c)
}
fn cam_json(v: &Value, name: &str) -> Cam {
    let lens = v["lens"].as_f64().unwrap_or(18.0) as f32; let sw = v["sensor_width"].as_f64().unwrap_or(36.0) as f32;
    Cam { name: name.to_string(), view: mat_from_json(&v["matrix"]), fov_x: 2.0 * (sw / (2.0 * lens)).atan(), ortho: None, near: None, clip: [0.0; 4], clip2: [0.0; 4], only: None, shown: 0.0, key: true, cover: 1.0, slab: 0.0 }
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
        // (from the doorways too, a step inside: where a person first sees the room, and what stands in it mostly lies ahead)
        {
            let mut seen_doors: Vec<Vec3> = vec![];
            for d in &s.draws {
                let nm = &s.glb.nodes[d.node].name;
                if !nm.starts_with("door") || !(nm.contains("frame") || nm.contains("casing")) { continue }
                let dc = (d.bmin + d.bmax) * 0.5;
                if dc.x < lo.x - 0.3 || dc.x > hi.x + 0.3 || dc.y < lo.y - 0.3 || dc.y > hi.y + 0.3 { continue }
                if seen_doors.iter().any(|q| (*q - dc).truncate().length() < 0.5) { continue }
                seen_doors.push(dc);
                let f = (tgt - dc).truncate().normalize_or_zero().extend(0.0);
                let e = Vec3::new(dc.x, dc.y, z0 + 1.45) + f * 0.3;
                if rm.tris.iter().any(|t| in_tri2(Vec3::new(e.x, e.y, z0), t)) && !inside(e, 0.05) { spare.push((e, f)); }
            }
        }
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
        // the room's floor, sampled: how much of it a view takes in is what says whether it shows the room
        let floor_pts: Vec<Vec3> = { let mut v = vec![]; let mut y = lo.y + 0.15; while y < hi.y { let mut x = lo.x + 0.15; while x < hi.x { if rm.tris.iter().any(|t| in_tri2(Vec3::new(x, y, z0), t)) { v.push(Vec3::new(x, y, z0)); } x += 0.3; } y += 0.3; } v };
        let floor_cover = |e: Vec3, f: Vec3, half: f32| -> f32 {
            let (mut seen_pts, n) = (0usize, floor_pts.len().max(1));
            for q in &floor_pts { let d = (*q - e).truncate(); let l = d.length().max(0.01); let u = d / l;
                if u.dot(f.truncate()).clamp(-1.0, 1.0).acos() < half && !ray_hit(&sh.walls, Vec3::new(e.x, e.y, z0 + 1.0), u.extend(0.0), l - 0.05) { seen_pts += 1; } }
            seen_pts as f32 / n as f32 };
        for &(k, alt, e0, h0) in &tries {
            // (off the walls: an eye on the plane of a wall's face, at the inner corner of an L, saw that wall in blocks across the lens)
            let e0 = { let on = |q: Vec3| rm.tris.iter().any(|t| in_tri2(Vec3::new(q.x, q.y, z0), t)); let mut q = e0; for d in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y] { if !on(e0 + d * 0.25) { q -= d * 0.25; } } if on(q) { q } else { e0 } };
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
            let mut cm = cam_look(*e, look, view_lens);
            // (a large piece or door within 3.4 m, cut off by the frame's edge: a fridge or a door leaf down one side of the picture, too far off for the depth check)
            cm.slab = { let hv = (18.0 / view_lens).atan(); let mut w = 0f32; for d in &s.draws {
                let nm = &s.glb.nodes[d.node].name; let leaf = nm.starts_with("door") && !nm.contains("frame") && !nm.contains("casing");
                if (skip(nm) && !leaf) || d.bmin.z > z0 + 1.2 || nm.contains("urtain") || nm.contains("Ceiling") { continue }
                let (mut a0, mut a1, mut dn) = (f32::MAX, f32::MIN, f32::MAX);
                for q in [(d.bmin.x, d.bmin.y), (d.bmax.x, d.bmin.y), (d.bmin.x, d.bmax.y), (d.bmax.x, d.bmax.y)] { let v = Vec3::new(q.0 - e.x, q.1 - e.y, 0.0); let a = (f.x * v.y - f.y * v.x).atan2(f.dot(v)); a0 = a0.min(a); a1 = a1.max(a); dn = dn.min(v.length()); }
                if dn > 3.4 || a1 - a0 > 3.0 || (a0 > -hv && a1 < hv) { continue }
                w = w.max((a1.min(hv) - a0.max(-hv)).max(0.0) / (2.0 * hv)); }
            if w > 0.12 { w } else { 0.0 } };
            cm.cover = floor_cover(*e, f, (18.0 / view_lens).atan());
            cm.shown = share(*e, f, (18.0 / view_lens).atan()) - 2.0 * out_share; cm.key = key_seen(*e, f, (18.0 / view_lens).atan()); cm.name = if alt > n_normal { format!("{}_A~w{alt}", rm.name) } else if cut_done { format!("{}_A~{}", rm.name, 20 + alt + 8 * k) } else if alt > 0 { format!("{}_A~{alt}", rm.name) } else { format!("{}_{}", rm.name, if k == 0 { "A" } else { "B" }) };
            // (PREVIEW_CAMS=1: one line per camera, for rendering the same view in Blender)
            if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("CAM {} {} {} {} {} {} {} {}", cm.name, e.x, e.y, e.z, look.x, look.y, look.z, lens); }
            out.push(cm);
            if out.iter().filter(|c| !c.name.contains('~')).count() >= n { return out; }
        }
        // the last word: a view from above the walls, ceiling and near walls gone, nothing but this room. Not as if standing in it, but the whole room always shows.
        // (true rectangles only: a notch's corner has no plane to cut it away, and stands through the picture as a slab)
        // (and small rooms: a larger one comes out tiny in the frame, and its standing views are better even when they score poorly)
        // (an L-shaped room too, seen from the side away from its notch: the notch's walls are then far walls like any other)
        let c = (lo + hi) * 0.5;
        let missing: Vec<(f32, f32)> = [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)].into_iter().filter(|(a, b)| { let q = Vec3::new(c.x + a * ((hi.x - lo.x) * 0.5 - 0.22), c.y + b * ((hi.y - lo.y) * 0.5 - 0.22), z0); !rm.tris.iter().any(|t| in_tri2(q, t)) }).collect();
        let fill = rm.area / ((hi.x - lo.x) * (hi.y - lo.y)).max(0.01);
        // the room's main rectangle: door openings add nubs to the floor's outline, and a view clipped at a nub left the wall itself standing through the picture
        let (tl, th) = { let on = |x: f32, y: f32| rm.tris.iter().any(|t| in_tri2(Vec3::new(x, y, z0), t));
            let cov_x = |x: f32| (0..20).filter(|k| on(x, lo.y + (hi.y - lo.y) * (*k as f32 + 0.5) / 20.0)).count() as f32 / 20.0;
            let cov_y = |y: f32| (0..20).filter(|k| on(lo.x + (hi.x - lo.x) * (*k as f32 + 0.5) / 20.0, y)).count() as f32 / 20.0;
            let (mut x0, mut x1, mut y0, mut y1) = (lo.x, hi.x, lo.y, hi.y);
            while x0 < c.x && cov_x(x0 + 0.03) < 0.6 { x0 += 0.03; } while x1 > c.x && cov_x(x1 - 0.03) < 0.6 { x1 -= 0.03; }
            while y0 < c.y && cov_y(y0 + 0.03) < 0.6 { y0 += 0.03; } while y1 > c.y && cov_y(y1 - 0.03) < 0.6 { y1 -= 0.03; }
            (Vec3::new(x0, y0, lo.z), Vec3::new(x1, y1, hi.z)) };
        let fill_t = rm.tris.iter().map(|t| ((t[1] - t[0]).cross(t[2] - t[0]).z.abs()) * 0.5).sum::<f32>() / ((th.x - tl.x) * (th.y - tl.y)).max(0.01);
        let rect = fill > 0.97 || (fill_t > 0.97 && fill_t < 1.25) || (missing.is_empty() && fill > 0.88);
        // the view's direction, toward the notches (none across from each other: one would stand before the lens)
        let (mx, my): (f32, f32) = (missing.iter().map(|q| q.0).sum(), missing.iter().map(|q| q.1).sum());
        let aim = (!missing.is_empty() && missing.len() <= 2 && !(missing.len() == 2 && mx == 0.0 && my == 0.0)).then(|| (if mx != 0.0 { mx.signum() } else if rm.subj.x >= c.x { 1.0 } else { -1.0 }, if my != 0.0 { my.signum() } else if rm.subj.y >= c.y { 1.0 } else { -1.0 }));
        if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("SHAPE {} area {:.2} size {:?} fill {fill:.2} missing {}", rm.name, rm.area, (hi - lo).truncate(), missing.len()); }
        // (not a hallway: its long look below says more, and an overhead view of one was skipped for a bare corner that scored above zero)
        // (a stub of a hallway has no long look: from above it shows, standing in it only walls)
        let hall_stub = rm.area < 7.5 && (hi - lo).truncate().max_element() < 4.0;
        if (!rm.name.starts_with("hallway") || hall_stub) && ((rect && rm.area < 9.0 && (hi - lo).truncate().max_element() < 3.8) || (!rect && rm.area < 12.0 && (hi - lo).truncate().max_element() < 6.0 && fill > 0.55 && aim.is_some())) {
            let (sx, sy) = if let (false, Some(q)) = (rect, aim) { q } else { (if rm.subj.x >= c.x { 1.0f32 } else { -1.0 }, if rm.subj.y >= c.y { 1.0f32 } else { -1.0 }) };
            let f = Vec3::new(sx, sy, 0.0).normalize();
            let r = ((hi - lo).truncate().length() * 0.5).max(1.0);
            let dist = 1.2 * r / 0.5625 + 0.9; // (filling the frame: further back the room floated small on the grey)
            let target = Vec3::new(c.x, c.y, z0 + 1.0);
            let e = target - f * dist * 0.72 + Vec3::new(0.0, 0.0, dist * 0.69);
            // (as near as the room still fits the frame: at a distance from its size alone it sat small on the grey)
            let wall_h = 2.6f32;
            let corners: Vec<Vec3> = [(tl.x, tl.y), (th.x, tl.y), (tl.x, th.y), (th.x, th.y)].iter().flat_map(|q| {
                let far = (q.0 - c.x) * sx > 0.0 || (q.1 - c.y) * sy > 0.0;
                let mut v = vec![Vec3::new(q.0, q.1, z0)]; if far { v.push(Vec3::new(q.0, q.1, z0 + wall_h)); } v }).collect();
            let fits = |d: f32| { let cm = cam_look(target - f * d * 0.72 + Vec3::new(0.0, 0.0, d * 0.69), target, 18.0); let tx = (cm.fov_x * 0.5).tan(); let ty = tx * 9.0 / 16.0;
                corners.iter().all(|q| { let p = cm.view.inverse().transform_point3(*q); p.z < -0.3 && (p.x / -p.z).abs() < tx * 0.94 && (p.y / -p.z).abs() < ty * 0.94 }) };
            let mut dist = dist; { let mut t = dist; while t > 1.5 && fits(t - 0.1) { t -= 0.1; } if fits(t) { dist = t; } else { while !fits(dist) && dist < 30.0 { dist += 0.2; } } }
            let e = target - f * dist * 0.72 + Vec3::new(0.0, 0.0, dist * 0.69);
            let mut cm = cam_look(e, target, 18.0);
            cm.near = Some(0.3);
            // (past the wall's inner face: the trim stops up to 3 cm short of it, and a plane in front of the face kept the wall)
            let m_ = |t: f32, b: f32| if (t - b).abs() > 1e-3 { 0.06 } else { 0.02 };
            // (and short of what stands against that wall: from behind, a plane 3 cm into the room took a wardrobe's back off, and showed it hollow)
            let in_room: Vec<(Vec3, Vec3)> = sh.boxes.iter().filter(|b| { let m = (b.0 + b.1) * 0.5; m.x > lo.x && m.x < hi.x && m.y > lo.y && m.y < hi.y }).map(|b| (b.0, b.1)).collect();
            let short_of = |w: f32, face: f32, m: f32, s: f32, ax: usize| { let front = in_room.iter().map(|b| if s > 0.0 { b.0[ax] } else { -b.1[ax] }).fold(f32::MAX, f32::min); w.min(front - 0.002).max(face + if m > 0.03 { 0.03 } else { 0.002 }) };
            let (fx, fy) = (if sx > 0.0 { tl.x } else { -th.x }, if sy > 0.0 { tl.y } else { -th.y });
            let (mx_, my_) = (if sx > 0.0 { m_(tl.x, lo.x) } else { m_(th.x, hi.x) }, if sy > 0.0 { m_(tl.y, lo.y) } else { m_(th.y, hi.y) });
            cm.clip = [sx, 0.0, 0.0, short_of(fx + mx_, fx, mx_, sx, 0)];
            cm.clip2 = [0.0, sy, 0.0, short_of(fy + my_, fy, my_, sy, 1)];
            cm.only = Some((lo, hi, rm.name.clone(), [cm.clip, cm.clip2]));
            cm.name = format!("{}_A~h", rm.name);
            out.push(cm);
        } else if rm.area < 40.0 && rm.name.starts_with("hallway") {
            // the last word for a passage: from wherever it can be seen furthest, a long look down its length
            let eye_z = z0 + 1.5;
            let (mut best, mut pick) = (0f32, None);
            let mut y = lo.y + 0.25;
            while y < hi.y {
                let mut x = lo.x + 0.25;
                while x < hi.x {
                    let p = Vec3::new(x, y, z0);
                    if rm.tris.iter().any(|t| in_tri2(p, t)) {
                        let e = Vec3::new(x, y, eye_z);
                        let clear = (0..8).map(|k| { let a = k as f32 * std::f32::consts::FRAC_PI_4; ray_dist(&sh.walls, e, Vec3::new(a.cos(), a.sin(), 0.0), 3.0) }).fold(3.0f32, f32::min);
                        let in_piece = sh.boxes.iter().any(|b| x > b.0.x - 0.2 && x < b.1.x + 0.2 && y > b.0.y - 0.2 && y < b.1.y + 0.2);
                        if clear > 0.4 && !in_piece {
                            for k in 0..16 {
                                let a = k as f32 * std::f32::consts::TAU / 16.0;
                                let f = Vec3::new(a.cos(), a.sin(), 0.0);
                                // how much of this room's own floor the view takes in, ray by ray across the lens: a look out through a door sees little of it, and one along a wall half
                                let on = |q: Vec3| rm.tris.iter().any(|t| in_tri2(Vec3::new(q.x, q.y, z0), t));
                                let v: f32 = (-4..=4).map(|h| { let a_ = (h as f32 * 10.0).to_radians(); let dir = Vec3::new(f.x * a_.cos() - f.y * a_.sin(), f.x * a_.sin() + f.y * a_.cos(), 0.0);
                                    let mut w = ray_dist(&sh.walls, e, dir, 8.0);
                                    for b in &sh.boxes { let (mut t0, mut t1) = (0f32, w); let mut hit = true; for ax in 0..2 { let (o_, r_) = (e[ax], dir[ax]); if r_.abs() < 1e-5 { if o_ < b.0[ax] || o_ > b.1[ax] { hit = false; } } else { let (a2, b2) = ((b.0[ax] - o_) / r_, (b.1[ax] - o_) / r_); t0 = t0.max(a2.min(b2)); t1 = t1.min(a2.max(b2)); } } if hit && t0 <= t1 { w = w.min(t0); } }
                                    let mut d_ = 0f32; while d_ < w && on(e + dir * (d_ + 0.1)) { d_ += 0.1; } if d_ < 1.0 { 0.0 } else { d_.min(4.0) } }).sum();
                                if v > best { best = v; pick = Some((e, f)); }
                            }
                        }
                    }
                    x += 0.25;
                }
                y += 0.25;
            }
            if let Some((e, f)) = pick {
                if best > 0.0 {
                    let mut cm = cam_look(e, e + f * 2.0 + Vec3::new(0.0, 0.0, -0.28), 16.0);
                    cm.name = format!("{}_A~s", rm.name);
                    out.push(cm);
                }
            }
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
struct Targets { m: [wgpu::TextureView; 4], m_depth: wgpu::TextureView, m_lit: wgpu::Texture, m_acc: wgpu::Texture, m_lit_v: wgpu::TextureView, m_acc_v: wgpu::TextureView, alb: wgpu::TextureView, mat: wgpu::TextureView, lit: wgpu::TextureView, hdr: wgpu::TextureView, nrm: wgpu::TextureView, depth: wgpu::TextureView, depth_tex: wgpu::Texture, dbuf: wgpu::Buffer, dbpr: u32, out: wgpu::Texture, readback: wgpu::Buffer, w: u32, h: u32, bpr: u32 }
struct Renderer { agx: wgpu::TextureView, agx_smp: wgpu::Sampler, rt: Option<rt::Pipes>, last_gl: Option<Globals>, rt_now: bool,
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
    let alb = mk(wgpu::TextureFormat::Rgba16Float, rt).create_view(&Default::default());
    let mat = mk(wgpu::TextureFormat::Rgba16Float, rt).create_view(&Default::default());
    let lit = mk(wgpu::TextureFormat::Rgba16Float, rt | wgpu::TextureUsages::STORAGE_BINDING).create_view(&Default::default());
    let nrm = mk(wgpu::TextureFormat::Rgba16Float, rt).create_view(&Default::default());
    let depth_tex = mk(wgpu::TextureFormat::Depth32Float, rt | wgpu::TextureUsages::COPY_SRC);
    let depth = depth_tex.create_view(&Default::default());
    let dbpr = (w * 4 + 255) / 256 * 256;
    let dbuf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (dbpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let out = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let bpr = (o.res.0 * 4 + 255) / 256 * 256;
    let readback = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * o.res.1) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    // (what a mirror shows, drawn from the camera mirrored in it: the same formats as the G-buffer, at the traced resolution)
    let mk1 = |fmt| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: rt, view_formats: &[] }).create_view(&Default::default());
    let m = [mk1(wgpu::TextureFormat::Rgba16Float), mk1(wgpu::TextureFormat::Rgba16Float), mk1(wgpu::TextureFormat::Rgba16Float), mk1(wgpu::TextureFormat::Rgba16Float)];
    let m_depth = mk1(wgpu::TextureFormat::Depth32Float);
    let mk2 = || dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: rt | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let (m_lit, m_acc) = (mk2(), mk2());
    let (m_lit_v, m_acc_v) = (m_lit.create_view(&Default::default()), m_acc.create_view(&Default::default()));
    Targets { m, m_depth, m_lit, m_acc, m_lit_v, m_acc_v, alb, mat, lit, hdr, nrm, depth, depth_tex, dbuf, dbpr, out, readback, w, h, bpr }
}

/// a render, exposed again to suit if too bright or too dark for its lights, as a camera would
fn exposed(r: &mut Renderer, s: &mut Scene, c: &Cam, o: &Opts) -> image::RgbaImage {
    if o.rt.is_some() && r.rt_now {
        // Blender's rule (render_rust.py auto_exposure): a mean of 0.36-0.5 over the middle 90 percent, with a guard on near-clipped pixels. Only the last pass runs again
        ensure_rt(r, s, o);
        let t = Instant::now();
        let mut img = render(r, s, c, o, 1.0);
        r.gpu.dev.poll(wgpu::PollType::wait_indefinitely()).ok();
        if o.verbose { eprintln!("  traced {:.3}s", t.elapsed().as_secs_f32()); }
        // (Blender's probes end in a band around 0.45 of the picture's mean: here only the last pass runs again, so it goes straight there, in steps of exposure)
        let stat = |img: &image::RgbaImage| { let mut lum: Vec<f32> = img.pixels().map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0).collect();
            lum.sort_by(|a, b| a.partial_cmp(b).unwrap()); let core = &lum[lum.len() / 20..lum.len() * 19 / 20];
            (core.iter().sum::<f32>() / core.len().max(1) as f32, lum.iter().filter(|v| **v > 0.95).count() as f32 / lum.len().max(1) as f32) };
        let target = std::env::var("RT_MEAN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.45f32);
        let (mut lo, mut hi) = (0.02f32, 64f32);
        let mut xp = 1.0f32;
        for _ in 0..7 {
            let (avg, clip) = stat(&img);
            if (avg - target).abs() < 0.008 && clip < 0.18 { break }
            if avg > target || clip >= 0.18 { hi = xp } else { lo = xp }
            xp = (lo * hi).sqrt();
            img = post_again(r, o, xp);
        }
        if o.verbose { eprintln!("  exposure x{xp:.2}, {:.3}s", t.elapsed().as_secs_f32()); }
        return img;
    }
    let img = render(r, s, c, o, 1.0);
    let (mut sum, mut hi, n) = (0f32, 0u32, img.pixels().len() as f32);
    for p in img.pixels() { let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0; sum += l; if l > 0.97 { hi += 1; } }
    let (m0, h0) = (sum / n, hi as f32 / n);
    if c.ortho.is_none() && (m0 > 0.74 || h0 > 0.10 || m0 < 0.38) {
        // (the tonemap's middle is steep: a little goes a long way)
        let xp = if m0 < 0.38 { (0.5 / m0.max(0.1)).min(1.8) } else { (0.66 / m0).powf(2.2).clamp(0.45, 0.95) };
        return render(r, s, c, o, xp);
    }
    img
}

fn make_main_pipe(dev: &wgpu::Device, sm: &wgpu::ShaderModule, g_bgl: &wgpu::BindGroupLayout, mat_bgl: &wgpu::BindGroupLayout, glass: bool) -> wgpu::RenderPipeline {
    let tgt = [Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: if glass { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None }, write_mask: wgpu::ColorWrites::ALL }),
               Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: if glass { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL } }),
               Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: if glass { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL } }),
               Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: if glass { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL } })];
    let vl = vbuf_layouts();
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(g_bgl), Some(mat_bgl)], immediate_size: 0 });
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&layout),
        vertex: wgpu::VertexState { module: sm, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &vl.iter().cloned().map(Some).collect::<Vec<_>>() },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(!glass && !bake::on()), depth_compare: Some(if bake::on() { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less }), stencil: Default::default(), bias: Default::default() }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: sm, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &tgt }), multiview_mask: None, cache: None })
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
    let g_bgl = bgl(&[uni(0, vf), tex2d(1, true, f), wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
        wgpu::BindGroupLayoutEntry { binding: 6, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }]);
    let sh_bgl = bgl(&[uni(0, vf)]);
    let mat_bgl = bgl(&[uni(0, f), tex2d(1, false, f), wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }]);
    let post_bgl = bgl(&[wgpu::BindGroupLayoutEntry { binding: 7, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false }, count: None },
        wgpu::BindGroupLayoutEntry { binding: 8, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }, uni(0, f), wgpu::BindGroupLayoutEntry { binding: 3, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        wgpu::BindGroupLayoutEntry { binding: 4, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        wgpu::BindGroupLayoutEntry { binding: 5, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }]);
    let pl = |l: &[&wgpu::BindGroupLayout]| dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &l.iter().map(|x| Some(*x)).collect::<Vec<_>>(), immediate_size: 0 });
    let sm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(bake::shader(SRC).into()) });
    let vl = vbuf_layouts();
    let prim = wgpu::PrimitiveState { cull_mode: None, ..Default::default() };
    let ds = |write: bool, bias: bool| wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(write), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(),
        bias: if bias { wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 } } else { Default::default() } };
    let shadow_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl(&[&sh_bgl])),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(vl[0].clone())] },
        primitive: prim, depth_stencil: Some(ds(true, true)), multisample: Default::default(), fragment: None, multiview_mask: None, cache: None });
    let main_pipe = make_main_pipe(&dev, &sm, &g_bgl, &mat_bgl, false); let glass_pipe = make_main_pipe(&dev, &sm, &g_bgl, &mat_bgl, true);
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
    // Blender's AgX, in steps of the log shaper: tools/preview/src/agx_lut.bin, 33 cubed, u16 RGB, blue slowest
    let half = |v: f32| -> u16 { // (0..1)
        if v < 6.2e-5 { return (v * 16777216.0).round() as u16; }
        let b = v.to_bits(); let e = ((b >> 23) & 0xff) as i32 - 127 + 15; let m = (b & 0x7fffff) >> 13;
        ((e as u16) << 10) | m as u16
    };
    let raw = include_bytes!("agx_lut.bin");
    let lut: Vec<u16> = raw.chunks_exact(6).flat_map(|c| { let f = |i: usize| half(u16::from_le_bytes([c[i], c[i + 1]]) as f32 / 65535.0); [f(0), f(2), f(4), half(1.0)] }).collect();
    let agx = dev.create_texture_with_data(&q, &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 33, height: 33, depth_or_array_layers: 33 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D3, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] }, wgpu::wgt::TextureDataOrder::LayerMajor, bytemuck::cast_slice(&lut)).create_view(&Default::default());
    let agx_smp = dev.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    Renderer { agx, agx_smp, rt: None, last_gl: None, rt_now: false, gpu: Gpu { dev, q }, ubuf, g_bgl, mat_bgl, post_bgl, shadow_pipe, main_pipe, glass_pipe, post_pipe, cmp_smp, white, smp, tg, last_depth: None }
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
fn draw_all<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, base: Option<&'a wgpu::RenderPipeline>, glass: bool, shadow: bool, fr: Option<&[Vec4; 6]>, eye: Option<Vec3>, cut: Option<(Vec3, f32)>, only: Option<&(Vec3, Vec3, String, [[f32; 4]; 2])>) {
    let mut bound = None;
    // (the house's rooms, to tell what belongs to another one)
    let rooms: Vec<String> = if only.is_some() { s.glb.nodes.iter().filter(|n| n.name.ends_with(".floor")).map(|n| n.name.replace('/', "_").trim_end_matches(".floor").to_string()).collect() } else { vec![] };
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
        if let Some((lo, hi, room, cuts)) = only {
            let nm = s.glb.nodes[d.node].name.replace('/', "_");
            let low = nm.to_lowercase();
            if low.contains(".ceiling") || low.contains("ceiling") || low.starts_with("outdoor") || low.starts_with("exterior") || low.contains("skirting") || low.contains("pendant") || low.contains("_light") || low.starts_with("beam") { continue; }
            // (door leaves too: one swung into the room stood through the picture like a slab of wall. The frames stay, to show where the doors are)
            if low.starts_with("door") && !low.starts_with("doorclear") && !low.contains("frame") && !low.contains("casing") { continue; }
            if low.contains(".wall") || low.contains(".floor") { if !nm.starts_with(&format!("{room}.")) { continue; } }
            else if (low.starts_with("door") || low.starts_with("window") || low.contains("curtain")) && { let c = (d.bmin + d.bmax) * 0.5; let n = [cut_plane(eye, lo, hi)]; n[0].0 * c.x < n[0].1 + 0.3 * 1.0 && n[0].0 != 0.0 || n[0].2 * c.y < n[0].3 + 0.3 && n[0].2 != 0.0 } { continue; }
            // (another room's things, in a notch of this room's box: a neighbour's books and vase hung in the air outside the walls)
            else if rooms.iter().any(|r| r != room && nm.contains(r.as_str())) { continue; }
            else { let c = (d.bmin + d.bmax) * 0.5; if c.x < lo.x - 0.15 || c.x > hi.x + 0.15 || c.y < lo.y - 0.15 || c.y > hi.y + 0.15 { continue; }
                // (what hangs on a cut wall goes with it: a picture was left hanging in the air)
                // (measured from the planes that cut it: the room's box runs on through a door's threshold, 25 cm past the wall, and a towel rail stayed behind)
                let thin = (d.bmax - d.bmin).truncate().min_element() < 0.12;
                if thin && d.bmin.z > lo.z + 0.3 && cuts.iter().any(|k| Vec3::new(k[0], k[1], k[2]).dot(c) < k[3] + 0.3) { continue; } }
        }
        if let Some(f) = fr { if !visible(f, d.bmin, d.bmax) { continue; } }
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        if let Some(b) = base { let want = if s.flat { None } else { d.pipe }; if bound != Some(want) { rp.set_pipeline(want.map_or(b, |i| &s.pipes[i])); bound = Some(want); } }
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
        sun_dir: sdir.extend(0.0).into(), sun_col: [3.2 * sun_i, 2.9 * sun_i, 2.5 * sun_i, 0.0], sky: [0.72 * ambient, 0.75 * ambient, 0.8 * ambient, 0.0], ground: [0.58 * ambient, 0.55 * ambient, 0.5 * ambient, 0.0],
        cam_pos: eye.extend(1.0).into(), cam_fwd: fwd.extend(0.0).into(), params: [s.lights.len().min(32) as f32, o.exposure * xp, if cam.near.is_some() { o.ao * 0.45 } else { o.ao }, 0.6], clip2: cam.clip2, clip: cam.clip, params2: [1.0 / SHADOW as f32, o.ss as f32, 1.0, 0.0],
        lpos: [[0.0; 4]; 32], lcol: [[0.0; 4]; 32] };
    for (i, l) in s.lights.iter().take(32).enumerate() { gl.lpos[i] = l.pos.extend(1.0).into(); gl.lcol[i] = l.color.extend(1.0).into(); }
    let rt_q = if r.rt_now { o.rt } else { None };
    if rt_q.is_some() {
        // (path traced: no ambient occlusion or fill to fake, and AgX with its look)
        gl.params[2] = 0.0; gl.params2[2] = 0.0; gl.params2[3] = 1.0;
        // (Blender's film exposure before it adjusts: render_rust.py's --exposure)
        gl.params[1] = 3.0 * xp;
        gl.ground = [0.12, 0.11, 0.1, 0.0];
        gl.cam_fwd[3] = std::env::var("RT_LOOK").ok().and_then(|v| v.parse().ok()).unwrap_or(1.2);
        // (the sun of Blender's sky, so glass and the rasterized shadow agree)
        if let Some(d) = &s.rt { gl.sun_dir = d.sun.extend(0.0).into(); }
    }
    r.last_gl = Some(gl);
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
            draw_all(&mut rp, s, None, false, true, None, None, None, None);
        }
        s.shadow_done = true;
    }
    let g_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.g_bgl, entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&s.shadow_view) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.cmp_smp) }, wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&r.tg.m_acc_v) }] });
    let tg = &r.tg; let fr = frustum(vp);
    // (path traced, a whole picture: the door leaves stay, as in Blender. Hiding those by the camera is the plain preview's help)
    let eye_leaf = if rt_q.is_some() && cam.near.is_none() && cam.only.is_none() { None } else { Some(eye) };
    {
        let sky = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &tg.hdr, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } }),
                                 Some(wgpu::RenderPassColorAttachment { view: &tg.nrm, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } }),
                                 Some(wgpu::RenderPassColorAttachment { view: &tg.alb, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } }),
                                 Some(wgpu::RenderPassColorAttachment { view: &tg.mat, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &tg.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &g_bg, &[]);
        rp.set_pipeline(&r.main_pipe); draw_all(&mut rp, s, Some(&r.main_pipe), false, false, Some(&fr), eye_leaf, cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) }), cam.only.as_ref());
        if rt_q.is_none() { rp.set_pipeline(&r.glass_pipe); draw_all(&mut rp, s, None, true, false, Some(&fr), eye_leaf, cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) }), cam.only.as_ref()); }
    }
    if let (Some(_), Some(p), Some(sd)) = (rt_q, r.rt.as_ref(), s.rt.as_ref()) {
        // what each mirror in view shows: the scene traced from the camera mirrored in it, what lies behind the glass cut away, only over the mirror's own part of the picture. Each is finished before the main picture needs it
        let sc = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let black = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let mut me = dev.create_command_encoder(&Default::default());
        me.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None, depth_stencil_attachment: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &tg.m_acc_v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } })] });
        q.submit([me.finish()]);
        let mut panes = mirror_panes(s, eye);
        panes.retain(|m| visible(&fr, m.2, m.3));
        panes.truncate(4);
        for &(n, d, lo, hi) in &panes {
            // the pane's box on screen (a pane reaching behind the camera: all of it)
            let (mut a, mut b) = (Vec2::splat(1.0), Vec2::splat(-1.0)); let mut behind = false;
            for k in 0..8 {
                let c = vp * Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z }).extend(1.0);
                if c.w <= 0.01 { behind = true; continue; }
                let q = Vec2::new(c.x, c.y) / c.w; a = a.min(q); b = b.max(q);
            }
            if behind { a = Vec2::splat(-1.0); b = Vec2::splat(1.0); }
            let (rw, rh) = (o.res.0 as f32, o.res.1 as f32);
            let x0 = (((a.x + 1.0) * 0.5 * rw).floor() - 2.0).clamp(0.0, rw); let x1 = (((b.x + 1.0) * 0.5 * rw).ceil() + 2.0).clamp(0.0, rw);
            let y0 = (((1.0 - b.y) * 0.5 * rh).floor() - 2.0).clamp(0.0, rh); let y1 = (((1.0 - a.y) * 0.5 * rh).ceil() + 2.0).clamp(0.0, rh);
            if x1 <= x0 || y1 <= y0 { continue; }
            let refl = Mat4::from_cols((Vec3::X - 2.0 * n.x * n).extend(0.0), (Vec3::Y - 2.0 * n.y * n).extend(0.0), (Vec3::Z - 2.0 * n.z * n).extend(0.0), (2.0 * d * n).extend(1.0));
            let vpm = vp * refl;
            let eye_m = refl.transform_point3(eye);
            let mut gm = gl; gm.viewproj = vpm.to_cols_array_2d(); gm.inv_viewproj = vpm.inverse().to_cols_array_2d(); gm.cam_pos = eye_m.extend(1.0).into(); gm.clip = [n.x, n.y, n.z, d + 0.002]; gm.clip2 = [0.0; 4];
            let ub = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&gm), usage: wgpu::BufferUsages::UNIFORM });
            let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.g_bgl, entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&s.shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.cmp_smp) }, wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&r.tg.m_acc_v) }] });
            let mut me = dev.create_command_encoder(&Default::default());
            {
                let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } });
                let mut rp = me.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    color_attachments: &[att(&tg.m[0]), att(&tg.m[1]), att(&tg.m[2]), att(&tg.m[3])],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &tg.m_depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
                rp.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
                rp.set_bind_group(0, &bg, &[]);
                let frm = frustum(vpm);
                rp.set_pipeline(&r.main_pipe); draw_all(&mut rp, s, Some(&r.main_pipe), false, false, Some(&frm), None, None, None);
            }
            let qm = rt::Quality { spp: 3, bounces: 2, spec_spp: 1, denoise: 5, ss: 1 };
            rt::encode(dev, q, &mut me, p, sd, &rt::Gb { mirror_lit: &tg.m_acc_v, depth: &tg.m_depth, nrm: &tg.m[1], alb: &tg.m[2], mat: &tg.m[3], lit: &tg.m_lit_v, hdr: &tg.m[0] }, o.res, 1, qm, vpm, eye_m, 1, [sc("RT_LAMP", 1.0), sc("RT_SKY", 1.0), sc("RT_SUN", 1.0)], false);
            let (ox, oy) = (x0 as u32, y0 as u32);
            me.copy_texture_to_texture(wgpu::TexelCopyTextureInfo { texture: &tg.m_lit, mip_level: 0, origin: wgpu::Origin3d { x: ox, y: oy, z: 0 }, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &tg.m_acc, mip_level: 0, origin: wgpu::Origin3d { x: ox, y: oy, z: 0 }, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: (x1 - x0) as u32, height: (y1 - y0) as u32, depth_or_array_layers: 1 });
            q.submit([me.finish()]);
        }
    }
    if let (Some(qu), Some(p), Some(sd)) = (rt_q, r.rt.as_ref(), s.rt.as_ref()) {
        let sc = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        rt::encode(dev, q, &mut enc, p, sd, &rt::Gb { mirror_lit: &tg.m_acc_v, depth: &tg.depth, nrm: &tg.nrm, alb: &tg.alb, mat: &tg.mat, lit: &tg.lit, hdr: &tg.hdr }, o.res, o.ss, qu, vp, eye, 1, [sc("RT_LAMP", 1.0), sc("RT_SKY", 1.0), sc("RT_SUN", 1.0)], cam.near.is_some() || cam.only.is_some());
        // the glass, forward, over the traced picture
        let mk = |v: &'_ wgpu::TextureView| v.clone();
        let (l, n, a, m) = (mk(&tg.lit), mk(&tg.nrm), mk(&tg.alb), mk(&tg.mat));
        let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[att(&l), att(&n), att(&a), att(&m)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &tg.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &g_bg, &[]);
        rp.set_pipeline(&r.glass_pipe); draw_all(&mut rp, s, None, true, false, Some(&fr), eye_leaf, cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) }), cam.only.as_ref());
    }
    let post_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.post_bgl, entries: &[wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&r.agx) }, wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&r.agx_smp) },
        wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(if rt_q.is_some() { &tg.lit } else { &tg.hdr }) },
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

/// The flat mirrors facing this eye: (normal toward the eye, plane offset, box low, box high)
fn mirror_panes(s: &Scene, eye: Vec3) -> Vec<(Vec3, f32, Vec3, Vec3)> {
    let (mut out, mut panes) = (vec![], vec![]);
    for d in &s.draws {
        // (flat panes of real glass too: they mirror the room a little)
        let mut ext = [d.bmax.x - d.bmin.x, d.bmax.y - d.bmin.y, d.bmax.z - d.bmin.z]; ext.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let flat_pane = d.pane && ext[0] < 0.05 && ext[1] > 0.3;
        if !flat_pane && (d.glass || !is_mirror(&s.glb.mats[d.mat].name)) { continue; }
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |k: usize| -> Vec3 { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); Vec3::new(f(0), f(1), f(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        // the largest triangle gives the plane
        let (mut best, mut v0, mut nn) = (0.0f32, Vec3::ZERO, Vec3::Z);
        for t in 0..d.icount as usize / 3 {
            let (a, b, c) = (p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2)));
            let cr = (b - a).cross(c - a); let ar = cr.length();
            if ar > best { best = ar; v0 = a; nn = cr / ar; }
        }
        if best < 1e-5 { continue; }
        if nn.dot(eye - v0) < 0.0 { nn = -nn; }
        // (a mirror with thickness: the face nearest the eye of those lying this way. Behind it, the picture showed the mirror itself from inside, black)
        let mut off = nn.dot(v0);
        for t in 0..d.icount as usize / 3 {
            let (a, b, c) = (p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2)));
            let cr = (b - a).cross(c - a); let ar = cr.length();
            if ar > best * 0.25 && (cr / ar).dot(nn).abs() > 0.99 { off = off.max(nn.dot(a)); }
        }
        if nn.dot(eye) - off < 1e-3 { continue; }
        if flat_pane { panes.push((nn, off, d.bmin, d.bmax)); } else { out.push((nn, off, d.bmin, d.bmax)); }
    }
    // (the largest panes after the mirrors: each one costs a small picture)
    panes.sort_by(|a, b| ((b.3 - b.2).length()).partial_cmp(&(a.3 - a.2).length()).unwrap());
    out.extend(panes);
    out
}

/// the house in the voxel grid, each cell with the part that fills it: id = draw + 1 (glass is seen through; the world outside the house's own surroundings is not in it)
fn build_grid(s: &mut Scene, o: &Opts) {
    let t = Instant::now();
    let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if !nm.starts_with("outdoor") && !nm.starts_with("exterior") { lo = lo.min(d.bmin); hi = hi.max(d.bmax); } }
    let m = Vec3::new(1.5, 1.5, 0.5);
    let mut g = vox::Grid::new(lo - m, hi + m);
    let mut colors = vec![Vec3::ZERO];
    let mut skip = vec![true];
    for (i, d) in s.draws.iter().enumerate() {
        let mt = &s.glb.mats[d.mat];
        colors.push(Vec3::new(mt.color[0], mt.color[1], mt.color[2]));
        skip.push(d.glass);
        if d.glass { continue }
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |k: usize| -> Vec3 { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); Vec3::new(f(0), f(1), f(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t3 in 0..d.icount as usize / 3 { g.tri(&[p(ix(t3 * 3)), p(ix(t3 * 3 + 1)), p(ix(t3 * 3 + 2))], (i + 1) as u16); }
    }
    if o.verbose { eprintln!("  voxel grid {:?} in {:.3}s", g.n, t.elapsed().as_secs_f32()); }
    s.grid = Some((g, colors, skip));
}

/// a candidate view, from the grid: a small plain-colour picture, its depth in `r.last_depth`
fn vox_view(r: &mut Renderer, s: &Scene, cam: &Cam, w: u32, h: u32) -> image::RgbaImage {
    let (g, colors, skip) = s.grid.as_ref().unwrap();
    let eye = cam.view.w_axis.truncate();
    let (right, up, fwd) = (cam.view.x_axis.truncate().normalize(), cam.view.y_axis.truncate().normalize(), -cam.view.z_axis.truncate().normalize());
    let tx = (cam.fov_x * 0.5).tan(); let ty = tx * h as f32 / w as f32;
    let sun = Vec3::new(0.55, 0.35, 0.75).normalize();
    // what this view leaves out: door leaves in the way, the cut wall
    let cut = cam.near.map(|_| { let n = Vec3::new(cam.clip[0], cam.clip[1], cam.clip[2]); (n, (cam.clip[3] - eye.dot(n)).max(0.0)) });
    let hidden: Vec<bool> = std::iter::once(true).chain(s.draws.iter().map(|d| leaf_in_the_way(s, d, Some(eye), cut))).collect();
    let (clip, clip2) = (cam.clip, cam.clip2);
    let pass = |id: u16, p: Vec3| -> bool {
        let i = id as usize;
        if skip[i] || hidden[i] { return true }
        if clip[0] * clip[0] + clip[1] * clip[1] + clip[2] * clip[2] > 0.5 && p.dot(Vec3::new(clip[0], clip[1], clip[2])) < clip[3] { return true }
        if clip2[0] * clip2[0] + clip2[1] * clip2[1] + clip2[2] * clip2[2] > 0.5 && p.dot(Vec3::new(clip2[0], clip2[1], clip2[2])) < clip2[3] { return true }
        false
    };
    let t0 = cam.near.unwrap_or(0.02);
    use rayon::prelude::*;
    let rows: Vec<Vec<([u8; 4], f32)>> = (0..h).into_par_iter().map(|y| (0..w).map(|x| {
        let (u, v) = ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
        let (o, d) = match cam.ortho {
            Some(wd) => (eye + right * (u * wd * 0.5) + up * (v * wd * 0.5 * h as f32 / w as f32), fwd),
            None => (eye, (fwd + right * (u * tx) + up * (v * ty)).normalize()),
        };
        let (px, z) = match g.march(o, d, t0, 60.0, &pass) {
            Some((t, id, axis)) => {
                let mut n = Vec3::ZERO; n[axis] = if d[axis] > 0.0 { -1.0 } else { 1.0 };
                let lit = 0.55 + 0.55 * n.dot(sun).max(0.0) + 0.1 * n.z.max(0.0);
                (colors[id as usize] * lit, t * d.dot(fwd))
            }
            None => (Vec3::new(0.7, 0.75, 0.82), 0.0),
        };
        let e = |c: f32| { let c = c.clamp(0.0, 1.0); (if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 } * 255.0) as u8 };
        ([e(px.x), e(px.y), e(px.z), 255], z)
    }).collect()).collect();
    let mut img = image::RgbaImage::new(w, h);
    let mut depth = vec![0f32; (w * h) as usize];
    for (y, row) in rows.iter().enumerate() { for (x, (c, z)) in row.iter().enumerate() { img.put_pixel(x as u32, y as u32, image::Rgba(*c)); depth[y * w as usize + x] = *z; } }
    r.last_depth = Some(depth);
    img
}

/// the tone-mapping pass alone, at another exposure
fn post_again(r: &mut Renderer, o: &Opts, xp: f32) -> image::RgbaImage {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let mut gl = r.last_gl.unwrap();
    gl.params[1] = 3.0 * xp;
    q.write_buffer(&r.ubuf, 0, bytemuck::bytes_of(&gl));
    let tg = &r.tg;
    let post_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.post_bgl, entries: &[wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&r.agx) }, wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&r.agx_smp) },
        wgpu::BindGroupEntry { binding: 0, resource: r.ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tg.lit) },
        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&tg.nrm) },
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&tg.depth) }] });
    let ov = tg.out.create_view(&Default::default());
    let mut enc = dev.create_command_encoder(&Default::default());
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

/// the traced scene: built once, on the first render
fn ensure_rt(r: &mut Renderer, s: &mut Scene, o: &Opts) {
    if r.rt.is_none() { r.rt = Some(rt::pipes(&r.gpu.dev, o.res.0, o.res.1, o.ss)); }
    if s.rt.is_some() { return }
    let t = Instant::now();
    let mats = probe_albedos(r, s, o);
    if o.verbose { eprintln!("  albedo probe {:.2}s", t.elapsed().as_secs_f32()); }
    // what light passes: glass, and the fittings around the lamps (a lamp shut in its own housing lit nothing)
    let passes = |d: &Draw| { let n = &s.glb.mats[d.mat].name; d.glass || n.starts_with("ceiling_") || n.starts_with("pendant_") || n.starts_with("lamp") };
    let (lo, hi) = (s.bmin - Vec3::splat(30.0), s.bmax + Vec3::splat(30.0));
    let mut tris: Vec<([Vec3; 3], u32)> = vec![];
    for (i, d) in s.draws.iter().enumerate() {
        if passes(d) { continue }
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |k: usize| -> Vec3 { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); Vec3::new(f(0), f(1), f(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t3 in 0..d.icount as usize / 3 {
            let v = [p(ix(t3 * 3)), p(ix(t3 * 3 + 1)), p(ix(t3 * 3 + 2))];
            if v.iter().any(|q| q.x < lo.x || q.y < lo.y || q.z < lo.z || q.x > hi.x || q.y > hi.y || q.z > hi.z) { continue }
            tris.push((v, i as u32));
        }
    }
    let lights: Vec<(Vec3, Vec3)> = s.lights.iter().map(|l| (l.pos, l.color)).collect();
    // the panes that let the sky in, as rectangles
    let mut wins: Vec<rt::Win> = vec![];
    for d in &s.draws {
        let nm = s.glb.nodes[d.node].name.to_lowercase();
        if !d.glass || !(nm.starts_with("window") || nm.starts_with("door")) { continue }
        let e = d.bmax - d.bmin;
        if e.z < 0.3 || e.x.max(e.y) < 0.2 { continue }
        // (its plan line, from the far corners of the bounds: a wall out of square has a pane a little askew)
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let (mut sa, mut sb) = (f32::MAX, f32::MIN); let (mut da, mut db) = (Vec3::ZERO, Vec3::ZERO);
        for k in 0..d.vcount as usize { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); let q = Vec3::new(f(0), f(1), f(2));
            let t = if e.x >= e.y { q.x } else { q.y }; if t < sa { sa = t; da = q; } if t > sb { sb = t; db = q; } }
        let mut u = (db - da) * 0.5; u.z = 0.0;
        let c = (d.bmin + d.bmax) * 0.5; let v = Vec3::new(0.0, 0.0, e.z * 0.5);
        let n = u.cross(v).normalize_or_zero();
        wins.push(rt::Win { c: c.extend(4.0 * u.length() * v.length()).into(), u: u.extend(0.0).into(), v: v.extend(0.0).into(), n: n.extend(0.0).into() });
    }
    if o.verbose { eprintln!("  {} windows", wins.len()); }
    s.rt = Some(rt::build_scene(&r.gpu.dev, &tris, &mats, &lights, &wins, o.seed, o.verbose));
    if o.verbose { eprintln!("  rt scene ready {:.2}s", t.elapsed().as_secs_f32()); }
}

/// each part's mean colour, from its own material drawn small: what a bounce sees of it
fn probe_albedos(r: &mut Renderer, s: &Scene, _o: &Opts) -> Vec<rt::MatD> {
    const T: u32 = 16; let cols = 64u32; let rows = (s.draws.len() as u32 / cols + 1).min(64);
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let (w, h) = (cols * T, rows * T);
    let mk = |fmt, usage| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage, view_formats: &[] });
    let ra = wgpu::TextureUsages::RENDER_ATTACHMENT;
    let (c0, c1, c2, c3) = (mk(wgpu::TextureFormat::Rgba16Float, ra), mk(wgpu::TextureFormat::Rgba16Float, ra), mk(wgpu::TextureFormat::Rgba16Float, ra | wgpu::TextureUsages::COPY_SRC), mk(wgpu::TextureFormat::Rgba16Float, ra | wgpu::TextureUsages::COPY_SRC));
    let dt = mk(wgpu::TextureFormat::Depth32Float, ra);
    let (v0, v1, v2, v3, dv) = (c0.create_view(&Default::default()), c1.create_view(&Default::default()), c2.create_view(&Default::default()), c3.create_view(&Default::default()), dt.create_view(&Default::default()));
    let bpr = (w * 8 + 255) / 256 * 256;
    let rb = |_: u32| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let (rb_a, rb_m) = (rb(0), rb(1));
    let mut enc = dev.create_command_encoder(&Default::default());
    let mut groups = vec![];
    let n = s.draws.len().min((cols * rows) as usize);
    for d in &s.draws[..n] {
        // along the part's thinnest side: its broad face
        let e = d.bmax - d.bmin; let c = (d.bmin + d.bmax) * 0.5;
        let (axis, up) = if e.z <= e.x && e.z <= e.y { (Vec3::Z, Vec3::Y) } else if e.x <= e.y { (Vec3::X, Vec3::Z) } else { (Vec3::Y, Vec3::Z) };
        let rad = e.length() * 0.5 + 0.01;
        let vp = orthographic_rh(-rad, rad, -rad, rad, 0.0, rad * 3.0) * look_at_rh(c + axis * rad * 1.5, c, up);
        let eye = c + axis * rad * 1.5;
        let mut gl: Globals = bytemuck::Zeroable::zeroed();
        gl.viewproj = vp.to_cols_array_2d(); gl.inv_viewproj = vp.inverse().to_cols_array_2d(); gl.sun_vp = s.sun_vp.to_cols_array_2d();
        gl.cam_pos = eye.extend(1.0).into(); gl.cam_fwd = (-axis).extend(1.0).into(); gl.sun_dir = [0.0, 0.0, 1.0, 0.0]; gl.params2 = [1.0 / SHADOW as f32, 1.0, 0.0, 1.0];
        let ub = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&gl), usage: wgpu::BufferUsages::UNIFORM });
        groups.push(dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.g_bgl, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&s.shadow_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.cmp_smp) }, wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&r.tg.m_acc_v) }] }));
    }
    {
        let clr = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clr), store: wgpu::StoreOp::Store } });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[att(&v0), att(&v1), att(&v2), att(&v3)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        for (i, d) in s.draws[..n].iter().enumerate() {
            if d.glass { continue }
            let (cx, cy) = (i as u32 % cols, i as u32 / cols);
            rp.set_viewport((cx * T) as f32, (cy * T) as f32, T as f32, T as f32, 0.0, 1.0);
            rp.set_scissor_rect(cx * T, cy * T, T, T);
            rp.set_bind_group(0, &groups[i], &[]);
            rp.set_pipeline(d.pipe.map_or(&r.main_pipe, |k| &s.pipes[k]));
            rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
            match d.nrm { Some(m) => rp.set_vertex_buffer(1, s.bufs[m.0].slice(m.1..m.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
            match d.uv { Some(m) => rp.set_vertex_buffer(2, s.bufs[m.0].slice(m.1..m.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
            rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
            let isz = if d.u16 { 2 } else { 4 };
            rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
            rp.draw_indexed(0..d.icount, 0, 0..1);
        }
    }
    let copy = |enc: &mut wgpu::CommandEncoder, t: &wgpu::Texture, b: &wgpu::Buffer| enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    copy(&mut enc, &c2, &rb_a); copy(&mut enc, &c3, &rb_m);
    q.submit([enc.finish()]);
    for b in [&rb_a, &rb_m] { b.slice(..).map_async(wgpu::MapMode::Read, |_| {}); }
    dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let (da, dm) = (rb_a.slice(..).get_mapped_range().unwrap(), rb_m.slice(..).get_mapped_range().unwrap());
    let half = |b: &[u8], x: u32, y: u32, c: usize| -> f32 { let o = (y * bpr + x * 8) as usize + c * 2; half_f32(u16::from_le_bytes([b[o], b[o + 1]])) };
    let mut out = vec![];
    for (i, d) in s.draws.iter().enumerate() {
        let m = &s.glb.mats[d.mat];
        let mut acc = Vec3::ZERO; let mut cnt = 0f32;
        if i < n {
            let (cx, cy) = (i as u32 % cols, i as u32 / cols);
            for y in 0..T { for x in 0..T {
                let (px, py) = (cx * T + x, cy * T + y);
                if half(&dm, px, py, 3) < 0.5 { continue }
                let metal = half(&dm, px, py, 0);
                acc += Vec3::new(half(&da, px, py, 0), half(&da, px, py, 1), half(&da, px, py, 2)) * (1.0 - 0.5 * metal); cnt += 1.0;
            } }
        }
        let a = if cnt > 0.0 { acc / cnt } else { Vec3::new(m.color[0], m.color[1], m.color[2]) };
        out.push(rt::MatD { albedo: [a.x.clamp(0.0, 0.95), a.y.clamp(0.0, 0.95), a.z.clamp(0.0, 0.95), 1.0], emissive: [m.emissive[0], m.emissive[1], m.emissive[2], 0.0] });
    }
    drop(da); drop(dm);
    out
}
fn half_f32(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 31) as u32, (h & 1023) as u32);
    let v = if e == 0 { (m as f32) * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m as f32 / 1024.0) * 2f32.powi(e as i32 - 15) };
    if s == 1 { -v } else { v }
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
        let mut s = load_scene(&r.gpu, path, &r.mat_bgl, &r.g_bgl, &r.white, &r.smp, &o);
        let cams = resolve_cams(&s, &o);
        if o.verbose { eprintln!("scene {} ready {:.2}s, {} cams", s.name, t.elapsed().as_secs_f32(), cams.len()); }
        if o.bake.is_some() { bake::run(&mut r, &mut s, &o, &cams); continue; }
        let mut best_detail: HashMap<String, f32> = HashMap::new();
        // views are chosen on plain colours (their score counts edges, and texture is all edges: poor views scored well, and their alternates went untried), then drawn with materials
        let has_graphs = !s.pipes.is_empty();
        s.flat = has_graphs;
        // (an explicit view, path traced: no choosing, so no plain-colour pass first)
        let direct_rt = o.rt.is_some() && !o.cams.is_empty() && o.cams.iter().all(|c| matches!(c, CamSpec::Json(_) | CamSpec::At(..)));
        if direct_rt { s.flat = false; }
        r.rt_now = false;
        let mut chosen: HashMap<String, usize> = HashMap::new();
        for (i, c) in cams.iter().enumerate() {
            // (only a poor view looks further)
            if let Some((base, alt)) = c.name.split_once('~') { let enough = if alt.starts_with('w') { 0.5 } else if alt == "h" { if c.name.starts_with("hallway") { f32::MAX } else { 0.0 } } else if alt == "s" { f32::MAX } else { 1.0 }; if best_detail.get(&format!("{}_{}", s.name, base)).map_or(true, |d| *d >= enough) { continue } }
            let t = Instant::now();
            // (the search, on the voxel grid: PREVIEW_CAM_GPU=1 scores real renders; a view from above draws only its room, so it still does)
            let by_grid = !direct_rt && c.only.is_none() && std::env::var_os("PREVIEW_CAM_GPU").is_none();
            if by_grid && s.grid.is_none() { build_grid(&mut s, &o); }
            let img = if direct_rt { image::RgbaImage::new(o.res.0, o.res.1) } else if by_grid { vox_view(&mut r, &s, c, VOX_W, VOX_H) } else { exposed(&mut r, &mut s, c, &o) };
            if std::env::var_os("PREVIEW_CAM_CMP").is_some() && by_grid {
                // (the two scores side by side, to keep the grid honest)
                let rawv = picture_detail(&img); let fv = flat_columns(&img); let dv = r.last_depth.clone();
                let g = exposed(&mut r, &mut s, c, &o);
                eprintln!("CAMCMP {} grid {rawv:.3} {fv:.3} gpu {:.3} {:.3}", c.name, picture_detail(&g), flat_columns(&g));
                r.last_depth = dv;
            }
            if o.verbose { eprintln!("  cam {} {:.3}s", c.name, t.elapsed().as_secs_f32()); }
            let nm = if c.name.is_empty() { format!("cam{i}") } else { c.name.clone() };
            // a view that is mostly bare: other spots for it are tried, the one with most to see kept; a second view only if it has something to show
            // (what stands right before the lens counts against it: a wall, a door, a bottle on a shelf filling the frame)
            let near = r.last_depth.as_ref().map_or(0.0, |d| { let (mut n, mut c) = (0f32, 0f32); for v in d.iter().step_by(11) { c += 1.0; if *v > 0.0 && *v < 0.9 { n += 1.0; } } n / c.max(1.0) });
            // (and what matters most is that it shows the room: a detailed view of a curtain is still no view of a bathroom)
            // (a bare picture earns none of that: what the room holds may be behind the wall it shows)
            // (the grid's small plain picture has less in it than a render of the same view: over 677 views its scores run at 0.81 of the render's, correlation 0.83)
            let raw = picture_detail(&img) * if by_grid { 1.23 } else { 1.0 };
            // (nor a close-up: book spines filling the frame are detail, and show no room)
            let close = r.last_depth.as_ref().map_or(0.0, |d| { let (mut n, mut c) = (0f32, 0f32); for v in d.iter().step_by(11) { c += 1.0; if *v > 0.0 && *v < 1.5 { n += 1.0; } } n / c.max(1.0) });
            let detail = raw - 1.2 * near + 0.8 * c.shown * (raw / 0.5).min(1.0) - 1.2 * flat_columns(&img) - if c.key { 0.0 } else { 0.8 } - 2.5 * (close - 0.3).max(0.0) - 2.0 * c.slab - 3.0 * (0.4 - c.cover).max(0.0);
            if let Some((base, alt)) = nm.split_once('~') {
                let key = format!("{}_{}", s.name, base);
                if let Some(fr) = frames.iter_mut().find(|(k, _)| *k == key) {
                    if alt == "h" { eprintln!("OVERHEAD {key}"); } if alt == "h" || alt == "s" || detail > best_detail.get(&key).copied().unwrap_or(0.0) + 0.03 { fr.1 = img; chosen.insert(key.clone(), i); best_detail.insert(key, detail); if o.depth { write_npy(&o.out.join(format!("{}_{}.depth.npy", s.name, base)), o.res, r.last_depth.as_ref().unwrap()); } }
                }
                if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("DETAIL {nm} {detail:.2}"); }
                continue;
            }
            if std::env::var_os("PREVIEW_CAMS").is_some() { eprintln!("DETAIL {nm} {detail:.2} raw {raw:.2} near {near:.2} close {close:.2} slab {:.2} cover {:.2}", c.slab, c.cover); }
            if nm.ends_with("_B") && detail < 0.75 { continue }
            best_detail.insert(format!("{}_{}", s.name, nm), detail);
            if o.depth { write_npy(&o.out.join(format!("{}_{}.depth.npy", s.name, nm)), o.res, r.last_depth.as_ref().unwrap()); }
            chosen.insert(format!("{}_{}", s.name, nm), i);
            frames.push((format!("{}_{}", s.name, nm), img));
        }
        {
            s.flat = false;
            if o.rt.is_some() { r.rt_now = true; }
            for fr in frames.iter_mut() { if let Some(&i) = chosen.get(&fr.0) { fr.1 = exposed(&mut r, &mut s, &cams[i], &o); } }
        }
        // (PREVIEW_SAVE_CAMS=dir: the views chosen, for --cam next time: the search is most of a render's time. Not a cutaway's: it needs its clipping too)
        if let Some(d) = std::env::var_os("PREVIEW_SAVE_CAMS") {
            std::fs::create_dir_all(&d).ok();
            for (key, &i) in &chosen {
                let c = &cams[i];
                if c.near.is_some() || c.only.is_some() { continue }
                let m = c.view.to_cols_array();
                let rows: Vec<Vec<f32>> = (0..4).map(|r| (0..4).map(|k| m[k * 4 + r]).collect()).collect();
                let j = serde_json::json!({"matrix": rows, "lens": 18.0 / (c.fov_x * 0.5).tan(), "sensor_width": 36.0});
                std::fs::write(Path::new(&d).join(format!("{}.json", key.strip_prefix(&format!("{}_", s.name)).unwrap_or(key))), j.to_string()).ok();
            }
        }
        r.gpu.dev.poll(wgpu::PollType::wait_indefinitely()).ok();
    }
    if o.bake.is_some() { return; }
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
