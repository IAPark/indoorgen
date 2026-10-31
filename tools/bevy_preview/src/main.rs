//! bevy_preview: headless Bevy (wgpu/Metal) previewer for infinigen-rs glb scenes; a drop-in for tools/preview with
//! Bevy's PBR renderer: glTF metallic/roughness materials, alpha-masked textures (clipped shadows), cascaded sun shadows
//! over the whole visible range, sidecar point lights, SSAO, physical atmosphere (sky + aerial perspective + sky-driven
//! ambient/reflections), AgX tonemapping, supersampled anti-aliasing. Many cameras per process (scene loaded once).
//!
//! Units: everything is physical (lux / lumens / nits) using 683 lm/W, so Blender watts and W/m² map 1:1 and the
//! camera exposure is a plain EV100 (Blender's exposure 0 is about EV100 9.15).
mod cams;
mod glb;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::{Exposure, Hdr, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{atmosphere::ScatteringMedium, Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight, NotShadowCaster, SunDisk};
use bevy::log::LogPlugin;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{AtmosphereSettings, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::settings::{PowerPreference, RenderCreation, WgpuSettings};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{BufferDescriptor, BufferUsages, CachedPipelineState, CommandEncoderDescriptor, MapMode, PipelineCache, PollType, TexelCopyBufferInfo, TexelCopyBufferLayout};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderPlugin, RenderSystems};
use bevy::window::ExitCondition;
use cams::{CamSpec, DrawInfo, SceneInfo};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// lumens per watt (and lux per W/m², nits per W/m²/sr): Blender radiometric units -> Bevy photometric units
const LM_PER_W: f32 = 683.0;
/// EV100 at --exposure 0.5 (the tools/preview default), tuned so default renders land near tools/preview brightness
const EV_AT_DEFAULT: f32 = 9.0;

struct Opts {
    inputs: Vec<PathBuf>, out: PathBuf, cams: Vec<CamSpec>, sheet: bool, res: (u32, u32), ss: u32,
    sun: (f32, f32), exposure: f32, ao: f32, no_ceiling: bool, ncams: usize, verbose: bool, ambient: f32, sun_i: f32, light_scale: f32,
    sidecar_lighting: bool, tonemap: String, shadow_dist: f32, fog: f32, point_shadows: usize, sun_strength: f32,
}

fn parse_args() -> Opts {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts { inputs: vec![], out: PathBuf::from("preview.png"), cams: vec![], sheet: false, res: (960, 540), ss: 2,
        sun: (215.0, 38.0), exposure: 0.5, ao: 1.0, no_ceiling: false, ncams: 3, verbose: false, ambient: 1.0, sun_i: 1.0, light_scale: 1.0,
        sidecar_lighting: false, tonemap: "agx".into(), shadow_dist: 400.0, fog: 1.0, point_shadows: 0, sun_strength: 4.0 };
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
            "--ss" => o.ss = val().parse::<u32>().unwrap().max(1),
            "--sun" => { let v = nf(&val()); o.sun = (v[0], v[1]) }
            "--exposure" => o.exposure = val().parse().unwrap(),
            "--ao" => o.ao = val().parse().unwrap(),
            "--ambient" => o.ambient = val().parse().unwrap(),
            "--sun-intensity" => o.sun_i = val().parse().unwrap(),
            "--sun-strength" => o.sun_strength = val().parse().unwrap(),
            "--light-scale" => o.light_scale = val().parse().unwrap(),
            "--no-ceiling" => o.no_ceiling = true,
            "--sidecar-lighting" => o.sidecar_lighting = true,
            "--tonemap" => o.tonemap = val(),
            "--shadow-dist" => o.shadow_dist = val().parse().unwrap(),
            "--fog" => o.fog = val().parse().unwrap(),
            "--point-shadows" => o.point_shadows = val().parse().unwrap(),
            "--cloth" | "--depth" => { if k == "--cloth" { val(); } eprintln!("bevy_preview: {k} is not supported (use tools/preview.sh)"); }
            "-v" => o.verbose = true,
            _ => o.inputs.push(PathBuf::from(k)),
        }
        i += 1;
    }
    assert!(!o.inputs.is_empty(), "usage: bevy_preview [--cam cam.json | --cams N|cam0 | --at ex,ey,ez,tx,ty,tz[,lens] | --person I | --top]... [--sheet] [--res WxH] [--ss N] [--sun az,el] [--exposure E] [--ao A] [--ambient A] [--no-ceiling] [--tonemap agx|aces|tony|reinhard] [--shadow-dist M] [--fog F] [--point-shadows N] -o OUT scene.glb...");
    o
}

fn blackbody(t: f32) -> Vec3 {
    let t = t / 100.0;
    let r = if t <= 66.0 { 255.0 } else { (329.698727446 * (t - 60.0).powf(-0.1332047592)).clamp(0.0, 255.0) };
    let g = if t <= 66.0 { (99.4708025861 * t.ln() - 161.1195681661).clamp(0.0, 255.0) } else { (288.1221695283 * (t - 60.0).powf(-0.0755148492)).clamp(0.0, 255.0) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { (138.5177312231 * (t - 10.0).ln() - 305.0447927307).clamp(0.0, 255.0) };
    Vec3::new(r, g, b) / 255.0
}

/// Blender Z-up -> Bevy Y-up (the glb root node's -90° rotation about X): (x, y, z) -> (x, z, -y)
#[inline] fn zup(v: [f32; 3]) -> [f32; 3] { [v[0], v[2], -v[1]] }
fn zup_mat() -> Mat4 { Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2) }

// ---------------- textures ----------------
/// RGBA8 sRGB image with a full mip chain. For alpha-tested textures each mip's alpha is rescaled so the fraction of
/// texels above the cutoff matches level 0 (otherwise mip averaging erodes leaf cards to nothing at a distance).
fn make_image(img: image::RgbaImage, cutoff: Option<f32>) -> Image {
    let mut img = img;
    if img.width().max(img.height()) > 1024 { let s = 1024.0 / img.width().max(img.height()) as f32; img = image::imageops::resize(&img, ((img.width() as f32 * s) as u32).max(1), ((img.height() as f32 * s) as u32).max(1), image::imageops::FilterType::Triangle); }
    let (w, h) = img.dimensions();
    let levels = 32 - w.max(h).leading_zeros();
    let coverage = |im: &image::RgbaImage, cut: f32, scale: f32| { let n = im.pixels().filter(|p| (p[3] as f32 / 255.0 * scale) >= cut).count(); n as f32 / (im.width() * im.height()) as f32 };
    let c0 = cutoff.map(|c| coverage(&img, c, 1.0));
    let mut data = Vec::with_capacity((w * h * 4 * 4 / 3) as usize + 64);
    let mut cur = img;
    for l in 0..levels {
        if l > 0 {
            let (cw, ch) = cur.dimensions();
            cur = image::imageops::resize(&cur, (cw / 2).max(1), (ch / 2).max(1), image::imageops::FilterType::Triangle);
            if let (Some(cut), Some(c0)) = (cutoff, c0) {
                // binary search the alpha scale that restores the level-0 coverage
                let (mut lo, mut hi) = (0.25f32, 4.0f32);
                for _ in 0..12 { let mid = (lo + hi) * 0.5; if coverage(&cur, cut, mid) < c0 { lo = mid } else { hi = mid } }
                let s = (lo + hi) * 0.5;
                for p in cur.pixels_mut() { p[3] = (p[3] as f32 * s).round().clamp(0.0, 255.0) as u8; }
            }
        }
        data.extend_from_slice(cur.as_raw());
    }
    let mut im = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    im.texture_descriptor.mip_level_count = levels;
    im.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear, min_filter: ImageFilterMode::Linear, mipmap_filter: ImageFilterMode::Linear, anisotropy_clamp: 8, ..default()
    });
    im
}

// ---------------- scene ----------------
struct Loaded { info: SceneInfo, name: String, ents: Vec<Entity>, lit: Option<(f32, f32, f32, f32)> }

fn build_mesh(g: &glb::Glb, p: &glb::Prim) -> Mesh {
    let rd3 = |v: &glb::View, n: usize| -> Vec<[f32; 3]> {
        let b = g.bytes(v);
        (0..n).map(|i| { let r = |k: usize| f32::from_le_bytes(b[(i * 3 + k) * 4..(i * 3 + k) * 4 + 4].try_into().unwrap()); zup([r(0), r(1), r(2)]) }).collect()
    };
    let n = p.vcount as usize;
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, rd3(&p.pos, n));
    if let Some(nv) = &p.nrm { m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, rd3(nv, n)); }
    if let Some(uv) = &p.uv {
        let b = g.bytes(uv);
        let v: Vec<[f32; 2]> = (0..n).map(|i| { let r = |k: usize| f32::from_le_bytes(b[(i * 2 + k) * 4..(i * 2 + k) * 4 + 4].try_into().unwrap()); [r(0), r(1)] }).collect();
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, v);
    }
    let ib = g.bytes(&p.idx); let ic = p.icount as usize;
    if p.idx_u16 { m.insert_indices(Indices::U16((0..ic).map(|k| u16::from_le_bytes([ib[k * 2], ib[k * 2 + 1]])).collect())); }
    else { m.insert_indices(Indices::U32((0..ic).map(|k| u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap())).collect())); }
    if p.nrm.is_none() { m.duplicate_vertices(); m.compute_flat_normals(); }
    m
}

fn load_scene(world: &mut World, path: &Path, o: &Opts) -> Loaded {
    let t = Instant::now();
    let g = glb::load(path);
    let side: Value = std::fs::read(path.with_extension("json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if o.verbose { eprintln!("  glb read {:.2}s ({} nodes, {:.0} MB)", t.elapsed().as_secs_f32(), g.nodes.len(), g.data.len() as f32 / 1e6); }
    let mut specs: HashMap<String, Value> = HashMap::new();
    if let Some(ns) = side["nodes"].as_array() { for n in ns { if let Some(nm) = n["name"].as_str() { specs.insert(nm.to_string(), n.clone()); } } }
    // flat list of primitives (+ per-node flags)
    struct P { node: usize, prim: usize, cls: String, tex: String, ceil: bool, glass: bool }
    let mut plist: Vec<P> = vec![];
    for (ni, n) in g.nodes.iter().enumerate() {
        let spec = specs.get(&n.name);
        let cls = spec.and_then(|s| s["spec"]["cls"].as_str()).unwrap_or("").to_string();
        let tex = if cls.starts_with("people.") { spec.and_then(|s| s["spec"]["kwargs"]["color"].as_str()).unwrap_or("").to_string() } else { String::new() };
        let lname = n.name.to_lowercase();
        for pi in 0..n.prims.len() { plist.push(P { node: ni, prim: pi, cls: cls.clone(), tex: tex.clone(), ceil: lname.contains("ceiling"), glass: lname.ends_with(".glass") && !lname.contains("wineglass") }); }
    }
    let show = |p: &P| !(o.no_ceiling && p.ceil);
    // decode textures (people texture files + glb-embedded base colour images) and build meshes, all in parallel
    let mut tex_keys: Vec<(String, Option<f32>)> = vec![];
    for p in &plist { if !p.tex.is_empty() && !tex_keys.iter().any(|k| k.0 == p.tex) { tex_keys.push((p.tex.clone(), if p.cls == "people.hair" { Some(0.5) } else { None })); } }
    for (mi, m) in g.mats.iter().enumerate() { if m.tex.is_some() { tex_keys.push((format!("glb:{mi}"), m.mask)); } }
    let (images, meshes): (Vec<Option<Image>>, Vec<Option<Mesh>>) = rayon::join(
        || tex_keys.par_iter().map(|(k, cut)| {
            let img = if let Some(mi) = k.strip_prefix("glb:") { image::load_from_memory(g.mats[mi.parse::<usize>().unwrap()].tex.as_ref().unwrap()).ok() } else { image::open(k).ok() };
            img.map(|i| make_image(i.to_rgba8(), *cut))
        }).collect(),
        || plist.par_iter().map(|p| if show(p) { Some(build_mesh(&g, &g.nodes[p.node].prims[p.prim])) } else { None }).collect());
    if o.verbose { eprintln!("  meshes+textures built {:.2}s ({} prims, {} textures)", t.elapsed().as_secs_f32(), plist.len(), tex_keys.len()); }
    let mut tex_h: HashMap<String, Handle<Image>> = HashMap::new();
    {
        let mut ia = world.resource_mut::<Assets<Image>>();
        for ((k, _), im) in tex_keys.iter().zip(images) { if let Some(im) = im { tex_h.insert(k.clone(), ia.add(im)); } }
    }
    // materials keyed by (glb material, texture, flags)
    let mut mat_h: HashMap<(usize, String, u32), Handle<StandardMaterial>> = HashMap::new();
    let mut draws = vec![]; let mut bmin = Vec3::splat(1e9); let mut bmax = Vec3::splat(-1e9);
    let (mut obmin, mut obmax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    let mut ents = vec![];
    let mut spawn: Vec<(Mesh, Handle<StandardMaterial>, Aabb, bool)> = vec![];
    for (p, mesh) in plist.iter().zip(meshes) {
        let n = &g.nodes[p.node]; let pr = &n.prims[p.prim];
        let mi = pr.mat.min(g.mats.len().saturating_sub(1));
        let (lo, hi) = (Vec3::from(pr.bmin), Vec3::from(pr.bmax));
        let lname = n.name.to_lowercase();
        if !p.ceil && !lname.starts_with("skirting_ceiling") {
            if lname.starts_with("outdoor") { obmin = obmin.min(lo); obmax = obmax.max(hi); } else { bmin = bmin.min(lo); bmax = bmax.max(hi); }
        }
        draws.push(DrawInfo { node: p.node, pos: pr.pos, idx: pr.idx, u16: pr.idx_u16, icount: pr.icount, bmin: lo, bmax: hi });
        let Some(mesh) = mesh else { continue };
        let m = g.mats.get(mi);
        let tex = if m.map_or(false, |m| m.tex.is_some()) { format!("glb:{mi}") } else { p.tex.clone() };
        let flags = if p.cls == "people.hair" || m.map_or(false, |m| m.mask.is_some()) { 1u32 } else if p.glass { 2 } else { 0 };
        let mh = mat_h.entry((mi, tex.clone(), flags)).or_insert_with(|| {
            let (c, e, rough, metal, cut) = m.map_or(([0.8, 0.8, 0.8, 1.0], [0.0; 3], 0.6, 0.0, 0.5), |m| (m.color, m.emissive, m.roughness, m.metallic, m.mask.unwrap_or(0.5)));
            let mut sm = StandardMaterial {
                base_color: Color::linear_rgba(c[0], c[1], c[2], if flags == 2 { 0.18 } else { c[3] }),
                base_color_texture: tex_h.get(&tex).cloned(),
                perceptual_roughness: rough.clamp(0.089, 1.0), metallic: metal,
                emissive: LinearRgba::rgb(e[0], e[1], e[2]) * LM_PER_W,
                double_sided: true, cull_mode: None,
                alpha_mode: match flags { 1 => AlphaMode::Mask(cut), 2 => AlphaMode::Blend, _ => if m.map_or(false, |m| m.blend) { AlphaMode::Blend } else { AlphaMode::Opaque } },
                ..default()
            };
            if flags == 2 { sm.perceptual_roughness = 0.08; sm.metallic = 0.0; sm.reflectance = 0.5; }
            world.resource_mut::<Assets<StandardMaterial>>().add(sm)
        }).clone();
        let (a, b) = (zup(pr.bmin), zup(pr.bmax));
        let aabb = Aabb::from_min_max(Vec3::new(a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])), Vec3::new(a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])));
        spawn.push((mesh, mh, aabb, flags == 2));
    }
    for (mesh, mh, aabb, glass) in spawn {
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let mut e = world.spawn((Mesh3d(h), MeshMaterial3d(mh), Transform::IDENTITY, aabb));
        if glass { e.insert(NotShadowCaster); }
        ents.push(e.id());
    }
    // the scene bbox (used by --top) covers the house only, unless the scene is all outdoor
    if bmin.x > bmax.x { bmin = obmin; bmax = obmax; }
    // sidecar point lights (Blender watts -> lumens)
    if let Some(ls) = side["lights"].as_array() {
        for (li, l) in ls.iter().enumerate() {
            let loc = &l["loc"]; let w = l["watt"].as_f64().unwrap_or(50.0) as f32; let c = blackbody(l["temperature"].as_f64().unwrap_or(5000.0) as f32);
            let p = [loc[0].as_f64().unwrap() as f32, loc[1].as_f64().unwrap() as f32, loc[2].as_f64().unwrap() as f32];
            let lm = w * LM_PER_W * o.light_scale;
            ents.push(world.spawn((PointLight { color: Color::srgb(c.x, c.y, c.z), intensity: lm, range: (lm / 4.0 / std::f32::consts::PI / 0.5).sqrt().clamp(8.0, 60.0),
                radius: l["radius"].as_f64().unwrap_or(0.05) as f32, shadow_maps_enabled: li < o.point_shadows, ..default() }, Transform::from_translation(Vec3::from(zup(p))))).id());
        }
    }
    let lit = if o.sidecar_lighting { side["variation"]["lighting"]["sun_dir"].as_array().map(|d| {
        let f = |i: usize| d[i].as_f64().unwrap() as f32;
        let l = &side["variation"]["lighting"];
        (f(1).atan2(f(0)).to_degrees(), f(2).asin().to_degrees(), l["sun_strength"].as_f64().unwrap_or(4.0) as f32 / 4.0, l["sky_strength"].as_f64().unwrap_or(1.0) as f32)
    }) } else { None };
    if o.verbose { eprintln!("  scene spawned {:.2}s ({} meshes, {} materials, bbox {:?}..{:?})", t.elapsed().as_secs_f32(), ents.len(), mat_h.len(), bmin, bmax); }
    Loaded { info: SceneInfo { glb: g, side, draws, bmin, bmax }, name: path.file_stem().unwrap().to_string_lossy().to_string(), ents, lit }
}

// ---------------- app / rendering ----------------
fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.build()
        .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, close_when_requested: false, ..default() })
        .set(LogPlugin { level: bevy::log::Level::WARN, filter: "wgpu=error,naga=warn,bevy_render=warn".into(), ..default() })
        .set(RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(WgpuSettings { power_preference: PowerPreference::HighPerformance, ..default() })), ..default() })
        .disable::<PipelinedRenderingPlugin>());
    app.insert_resource(DirectionalLightShadowMap { size: 4096 });
    app.insert_resource(GlobalAmbientLight::NONE);
    app.insert_resource(ClearColor(Color::BLACK));
    while app.plugins_state() == bevy::app::PluginsState::Adding { bevy::tasks::tick_global_task_pools_on_main_thread(); }
    app.finish();
    app.cleanup();
    app
}

/// Synchronous readback of the render target, done in the render world right after the frame is submitted (Bevy's
/// Screenshot is asynchronous and arrives several frames later).
#[derive(Default)]
struct GrabState { want: bool, target: Option<AssetId<Image>>, out: Option<(Vec<u8>, u32, u32, TextureFormat)> }
#[derive(Resource, Clone, Default)]
struct Grab(Arc<Mutex<GrabState>>);

fn grab_system(grab: Res<Grab>, imgs: Res<RenderAssets<GpuImage>>, dev: Res<RenderDevice>, q: Res<RenderQueue>) {
    let mut g = grab.0.lock().unwrap();
    if !g.want { return; }
    let Some(gi) = g.target.and_then(|t| imgs.get(t)) else { return };
    let size = gi.texture_descriptor.size; let fmt = gi.texture_descriptor.format;
    let row = (size.width * 4).div_ceil(256) * 256;
    let buf = dev.create_buffer(&BufferDescriptor { label: Some("grab"), size: (row * size.height) as u64, usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = dev.create_command_encoder(&CommandEncoderDescriptor { label: Some("grab") });
    enc.copy_texture_to_buffer(gi.texture.as_image_copy(), TexelCopyBufferInfo { buffer: &buf, layout: TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, size);
    q.submit([enc.finish()]);
    buf.slice(..).map_async(MapMode::Read, |r| { r.expect("map grab buffer"); });
    dev.poll(PollType::wait_indefinitely()).expect("poll");
    let data = buf.slice(..).get_mapped_range().expect("mapped range");
    let mut out = Vec::with_capacity((size.width * size.height * 4) as usize);
    for y in 0..size.height as usize { out.extend_from_slice(&data[y * row as usize..y * row as usize + size.width as usize * 4]); }
    drop(data); buf.unmap();
    g.out = Some((out, size.width, size.height, fmt)); g.want = false;
}

/// true when no render pipeline is still queued/compiling (Bevy skips draws whose pipeline is not ready)
fn pipelines_settled(app: &App) -> bool {
    let pc = app.get_sub_app(RenderApp).unwrap().world().resource::<PipelineCache>();
    pc.waiting_pipelines().next().is_none() && pc.pipelines().all(|p| matches!(p.state, CachedPipelineState::Ok(_) | CachedPipelineState::Err(_)))
}

/// render one frame (after `min_warm` warm-up frames and until pipelines have settled) and read the target back
fn capture(app: &mut App, grab: &Grab, min_warm: usize, verbose: bool) -> (Vec<u8>, u32, u32, TextureFormat) {
    let mut k = 0;
    let mut settled = pipelines_settled(app);
    while (k < min_warm || !settled) && k < 60 {
        let t = Instant::now(); app.update(); k += 1;
        settled = pipelines_settled(app);
        if verbose { eprintln!("    frame {k} {:.3}s{}", t.elapsed().as_secs_f32(), if settled { "" } else { " (pipelines pending)" }); }
    }
    grab.0.lock().unwrap().want = true;
    let t = Instant::now(); app.update();
    if verbose { eprintln!("    frame+grab {:.3}s ({} pipelines)", t.elapsed().as_secs_f32(), app.get_sub_app(RenderApp).unwrap().world().resource::<PipelineCache>().pipelines().count()); }
    grab.0.lock().unwrap().out.take().expect("render target readback failed")
}

/// box-filter `ss`x`ss` blocks in linear light -> sRGB RGBA8
fn downsample(im: &(Vec<u8>, u32, u32, TextureFormat), res: (u32, u32), ss: u32) -> image::RgbaImage {
    let data = &im.0;
    let sw = im.1 as usize;
    let bgra = matches!(im.3, TextureFormat::Bgra8UnormSrgb | TextureFormat::Bgra8Unorm);
    let lut: Vec<f32> = (0..256).map(|i| { let c = i as f32 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } }).collect();
    let enc = |l: f32| -> u8 { let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }; (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8 };
    let (w, h) = (res.0 as usize, res.1 as usize); let s = ss as usize;
    let mut out = vec![0u8; w * h * 4];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut acc = [0f32; 3];
            for j in 0..s { for i in 0..s { let o = ((y * s + j) * sw + x * s + i) * 4; for c in 0..3 { acc[c] += lut[data[o + c] as usize]; } } }
            let n = (s * s) as f32;
            let px = [enc(acc[0] / n), enc(acc[1] / n), enc(acc[2] / n)];
            row[x * 4..x * 4 + 4].copy_from_slice(&if bgra { [px[2], px[1], px[0], 255] } else { [px[0], px[1], px[2], 255] });
        }
    });
    image::RgbaImage::from_raw(res.0, res.1, out).unwrap()
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
    let mut app = make_app();
    let grab = Grab::default();
    app.get_sub_app_mut(RenderApp).unwrap().insert_resource(grab.clone()).add_systems(Render, grab_system.in_set(RenderSystems::Cleanup));
    let frames_v = std::env::var("BP_FRAMES").is_ok();
    if o.verbose { eprintln!("bevy init {:.2}s", t0.elapsed().as_secs_f32()); }
    let (rw, rh) = (o.res.0 * o.ss, o.res.1 * o.ss);
    let target = {
        let mut im = Image::new_target_texture(rw, rh, TextureFormat::Rgba8UnormSrgb, None);
        im.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        app.world_mut().resource_mut::<Assets<Image>>().add(im)
    };
    grab.0.lock().unwrap().target = Some(target.id());
    let tonemap = match o.tonemap.as_str() { "aces" => Tonemapping::AcesFitted, "tony" => Tonemapping::TonyMcMapface, "reinhard" => Tonemapping::ReinhardLuminance, "none" => Tonemapping::None, _ => Tonemapping::AgX };
    let medium = app.world_mut().resource_mut::<Assets<ScatteringMedium>>().add(ScatteringMedium::earth(256, 256));
    app.world_mut().spawn(Atmosphere::earth(medium));
    let ev = EV_AT_DEFAULT - (o.exposure / 0.5).log2();
    let cam_e = {
        let w = app.world_mut();
        let mut e = w.spawn((Camera3d::default(), RenderTarget::Image(target.clone().into()), Hdr, Msaa::Off, tonemap, Exposure { ev100: ev },
            Projection::Perspective(PerspectiveProjection { near: 0.05, far: 4000.0, ..default() }), Transform::IDENTITY,
            AtmosphereSettings::default(), AtmosphereEnvironmentMapLight { intensity: o.ambient, ..default() }));
        if o.ao > 0.0 { e.insert(ScreenSpaceAmbientOcclusion { quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High, ..default() }); }
        if o.fog > 0.0 {
            // light haze on top of the physical aerial perspective (which is very weak at a few hundred metres)
            e.insert(DistanceFog { color: Color::srgba(0.62, 0.70, 0.80, 1.0), falloff: FogFalloff::from_visibility_colors(3500.0 / o.fog, Color::srgb(0.30, 0.34, 0.40), Color::srgb(0.62, 0.70, 0.80)), ..default() });
        }
        e.id()
    };
    let sun_e = app.world_mut().spawn((DirectionalLight { shadow_maps_enabled: true, ..default() }, Transform::IDENTITY, SunDisk::EARTH,
        CascadeShadowConfigBuilder { num_cascades: 4, minimum_distance: 0.05, first_cascade_far_bound: 6.0, maximum_distance: o.shadow_dist, overlap_proportion: 0.25 }.build())).id();
    let mut frames: Vec<(String, image::RgbaImage)> = vec![];
    for path in &o.inputs {
        let t = Instant::now();
        let s = load_scene(app.world_mut(), path, &o);
        let cams = cams::resolve_cams(&s.info, &o.cams, o.ncams, o.res);
        // sun
        let (sun_az, sun_el, sun_i, ambient) = s.lit.map_or((o.sun.0, o.sun.1, o.sun_i, o.ambient), |l| (l.0, l.1, l.2, l.3));
        let (az, el) = (sun_az.to_radians(), sun_el.to_radians());
        let sdir = Vec3::from(zup([el.cos() * az.cos(), el.cos() * az.sin(), el.sin()]));
        {
            let w = app.world_mut();
            let up = if sdir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
            *w.get_mut::<Transform>(sun_e).unwrap() = Transform::IDENTITY.looking_to(-sdir, up);
            w.get_mut::<DirectionalLight>(sun_e).unwrap().illuminance = o.sun_strength * LM_PER_W * sun_i;
            w.get_mut::<AtmosphereEnvironmentMapLight>(cam_e).unwrap().intensity = ambient;
        }
        if o.verbose { eprintln!("scene {} loaded {:.2}s, {} cams", s.name, t.elapsed().as_secs_f32(), cams.len()); }
        for (i, c) in cams.iter().enumerate() {
            let tc = Instant::now();
            {
                let w = app.world_mut();
                *w.get_mut::<Transform>(cam_e).unwrap() = Transform::from_matrix(zup_mat() * c.view);
                let aspect = o.res.0 as f32 / o.res.1 as f32;
                *w.get_mut::<Projection>(cam_e).unwrap() = match c.ortho {
                    Some(wd) => Projection::Orthographic(OrthographicProjection { near: 0.05, far: 4000.0, scaling_mode: ScalingMode::Fixed { width: wd, height: wd / aspect }, ..OrthographicProjection::default_3d() }),
                    None => Projection::Perspective(PerspectiveProjection { fov: 2.0 * ((c.fov_x / 2.0).tan() / aspect).atan(), aspect_ratio: aspect, near: 0.05, far: 4000.0, ..default() }),
                };
            }
            // the first camera of a scene also pays for GPU upload + pipeline compilation
            let img = capture(&mut app, &grab, if i == 0 { 2 } else { 0 }, frames_v);
            let img = downsample(&img, o.res, o.ss);
            if o.verbose { eprintln!("  cam {} {:.3}s{}", c.name, tc.elapsed().as_secs_f32(), if i == 0 { " (incl. GPU upload/pipelines)" } else { "" }); }
            let nm = if c.name.is_empty() { format!("cam{i}") } else { c.name.clone() };
            frames.push((format!("{}_{}", s.name, nm), img));
        }
        let w = app.world_mut();
        for e in s.ents { w.despawn(e); }
        if o.verbose { eprintln!("scene {} done {:.2}s", s.name, t.elapsed().as_secs_f32()); }
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
        if let Some(p) = o.out.parent() { std::fs::create_dir_all(p).ok(); }
        save_png(&frames[0].1, &o.out); println!("{}", o.out.display());
    } else {
        std::fs::create_dir_all(&o.out).ok();
        for (nm, im) in &frames { let p = o.out.join(format!("{nm}.png")); save_png(im, &p); println!("{}", p.display()); }
    }
    if o.verbose { eprintln!("total {:.2}s", t0.elapsed().as_secs_f32()); }
    // skip Bevy/wgpu teardown (can take a while with a big scene resident)
    std::process::exit(0);
}
