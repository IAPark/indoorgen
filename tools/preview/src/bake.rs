//! bake: baked rendering of the house (--bake MODE[:SECONDS], SECONDS of baking, 25 if not given). Then frames at 30+ fps with the look of --rt full. See bake.wgsl.
//! still: a picture per camera (BAKE_FRAMES, 16: jittered frames averaged). video: -o X.mp4 by ffmpeg, a walk through the rooms (BAKE_LEN frames, 300) or BAKE_PATH=file.json ([[ex,ey,ez,tx,ty,tz], ...], one a frame); BAKE_SAVE_PATH=file keeps the walk.
//! measure: what the parts of a frame cost. Others: BAKE_SCALE (1.5), BAKE_CELL (0.05 m), BAKE_TAA (0.12), BAKE_CODEC (h264_videotoolbox or libx264), BAKE_DUMP=dir (every frame as a png), RT_LAMP, RT_SKY, RT_SUN, RT_SHOW as --rt.
//! Assumes a static scene and a perspective camera, one sky seed per bake; cutaways are not handled.
use super::*;
use glam::{Mat4, Vec2, Vec3, Vec4};
use std::sync::atomic::{AtomicBool, Ordering};

pub static ON: AtomicBool = AtomicBool::new(false);
pub fn on() -> bool { ON.load(Ordering::Relaxed) }
/// the surface shader as this mode draws it: light is done elsewhere, so fs_main only fills the G-buffer. And a fragment shader for the depth prepass of what its texture cuts out
pub fn shader(src: &str) -> String {
    if !on() { return src.to_string(); }
    let (a, b) = (src.find("/*LIT<*/").expect("LIT marker"), src.find("/*>LIT*/").expect("LIT marker"));
    format!("{}var col = vec3<f32>(0.0);\n{}\n{}", &src[..a], &src[b..], FS_Z)
}
const FS_Z: &str = r#"
@vertex fn vs_z(v: VIn) -> VOut { var o: VOut; o.clip = g.viewproj * vec4<f32>(v.pos, 1.0); o.wpos = v.pos; o.nrm = v.nrm; o.uv = v.uv; return o; }
@fragment fn fs_z(v: VOut) {
  if (dot(g.clip.xyz, g.clip.xyz) > 0.5 && dot(g.clip.xyz, v.wpos) < g.clip.w) { discard; }
  if (m.flags.x > 0.5 && m.flags.x < 1.5 && m.color.a * textureSampleLevel(base_tex, base_smp, v.uv, 0.0).a < 0.5) { discard; }
}
"#;

pub struct Gb { pub hdr: wgpu::TextureView, pub nrm: wgpu::TextureView, pub alb: wgpu::TextureView, pub mat: wgpu::TextureView, pub depth: wgpu::TextureView, pub w: u32, pub h: u32 }
pub fn make_gb(dev: &wgpu::Device, w: u32, h: u32) -> Gb {
    let mk = |fmt| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }).create_view(&Default::default());
    let f = wgpu::TextureFormat::Rgba16Float;
    Gb { hdr: mk(f), nrm: mk(f), alb: mk(f), mat: mk(f), depth: mk(wgpu::TextureFormat::Depth32Float), w, h }
}
/// the camera's matrices and the uniforms of a G-buffer pass, path traced look
pub fn globals(s: &Scene, cam: &Cam, o: &Opts, aspect: f32, jitter: (f32, f32)) -> (Globals, Mat4, Vec3) {
    let (zn, zf) = clip_range(s);
    let zn = cam.near.unwrap_or(zn);
    let mut proj = perspective_rh(2.0 * ((cam.fov_x / 2.0).tan() / aspect).atan(), aspect, zn, zf);
    proj.z_axis.x += jitter.0; proj.z_axis.y += jitter.1;
    let view = cam.view.inverse(); let vp = proj * view; let eye = cam.view.w_axis.truncate(); let fwd = -cam.view.z_axis.truncate().normalize();
    let sd = s.rt.as_ref().unwrap();
    let mut gl: Globals = bytemuck::Zeroable::zeroed();
    gl.viewproj = vp.to_cols_array_2d(); gl.inv_viewproj = vp.inverse().to_cols_array_2d(); gl.sun_vp = s.sun_vp.to_cols_array_2d();
    gl.sun_dir = sd.sun.extend(0.0).into(); gl.ground = [0.12, 0.11, 0.1, 0.0]; gl.cam_pos = eye.extend(1.0).into();
    gl.cam_fwd = fwd.extend(0.0).into(); gl.cam_fwd[3] = 1.2;
    gl.params = [0.0, 3.0, 0.0, 0.6]; gl.params2 = [1.0 / SHADOW as f32, o.ss as f32, 0.0, 1.0]; gl.clip = cam.clip; gl.clip2 = cam.clip2;
    (gl, vp, eye)
}
/// the opaque draws in a frustum, without draw_all's per-draw lookups
pub fn draw_fast<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, base: &'a wgpu::RenderPipeline, list: &[u32]) -> u64 {
    let mut bound = None; let mut tris = 0u64;
    for &i in list {
        let d = &s.draws[i as usize];
        if bound != Some(d.pipe) { rp.set_pipeline(d.pipe.map_or(base, |k| &s.pipes[k])); bound = Some(d.pipe); }
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
        match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
        rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, i..i + 1);
        tris += d.icount as u64 / 3;
    }
    tris
}
pub fn g_bg(r: &Renderer, s: &Scene, ub: &wgpu::Buffer) -> wgpu::BindGroup {
    r.gpu.dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &r.g_bgl, entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&s.shadow_view) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.cmp_smp) }, wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&r.tg.m_acc_v) }] })
}
fn wiggled(c: &Cam, f: usize) -> Cam {
    let a = f as f32 * 0.21; let mut m = c.clone();
    let rot = Mat4::from_rotation_z(0.25 * a.sin());
    let t = c.view.w_axis.truncate();
    m.view = Mat4::from_translation(t + Vec3::new(0.15 * (a * 0.7).sin(), 0.15 * (a * 0.9).cos(), 0.0)) * rot * Mat4::from_translation(-t) * c.view;
    m
}
struct Stat { name: String, enc: Vec<f32>, wait: Vec<f32> }
impl Stat {
    fn new(n: &str) -> Stat { Stat { name: n.into(), enc: vec![], wait: vec![] } }
    fn print(&mut self) {
        let med = |v: &mut Vec<f32>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { 0.0 } else { v[v.len() / 2] } };
        let (e, w) = (med(&mut self.enc), med(&mut self.wait)); let hi = self.wait.last().copied().unwrap_or(0.0);
        println!("{:42} encode {:7.2} ms  gpu wait {:7.2} ms  (max {:7.2})  total {:7.2}", self.name, e, w, hi, e + w);
    }
}
/// phase 1: what each part of a frame costs here
pub fn measure(r: &mut Renderer, s: &mut Scene, o: &Opts, cams: &[Cam]) {
    ensure_rt(r, s, o);
    let cam0 = &cams[0];
    let n = 24;
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    // a depth-only pipeline on the same layout
    let sm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(shader(SRC).replace("/*LIB*/", ig_shade::wgsl::LIB).replace("/*SURF*/", "").into()) });
    let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&r.g_bgl)], immediate_size: 0 });
    let vl = vbuf_layouts();
    let zpipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(vl[0].clone())] },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() }, depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
        multisample: Default::default(), fragment: None, multiview_mask: None, cache: None });
    println!("draws {} pipes {} mat_bgs {} buffers {}", s.draws.len(), s.pipes.len(), s.mat_bgs.len(), s.bufs.len());
    let total_tris: u64 = s.draws.iter().map(|d| d.icount as u64 / 3).sum();
    println!("scene tris {total_tris}");
    let black = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    let ub = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let bg = g_bg(r, s, &ub);
    for (w, h) in [(1440u32, 810u32), (960, 540)] {
        let gb = make_gb(dev, w, h);
        let aspect = w as f32 / h as f32;
        let mut st_z = Stat::new(&format!("depth only {w}x{h}")); let mut st_c = Stat::new(&format!("gbuffer after prepass {w}x{h}"));
        let (mut nd, mut nt) = (0usize, 0u64);
        for f in 0..n + 3 {
            let cam = wiggled(cam0, f);
            let (gl, vp, eye) = globals(s, &cam, o, aspect, (0.0, 0.0));
            q.write_buffer(&ub, 0, bytemuck::bytes_of(&gl));
            let fr = frustum(vp);
            let mut list: Vec<u32> = s.draws.iter().enumerate().filter(|(_, d)| !d.glass && visible(&fr, d.bmin, d.bmax)).map(|(i, _)| i as u32).collect();
            // front to back, near boxes first
            list.sort_by_key(|&i| { let d = &s.draws[i as usize]; (eye.clamp(d.bmin, d.bmax) - eye).length_squared() as u32 * 16 });
            let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } });
            // prepass and then the surfaces
            let mut gz = gl; gz.sun_vp = gl.viewproj;
            q.write_buffer(&ub, 0, bytemuck::bytes_of(&gz));
            let t = Instant::now();
            let mut enc = dev.create_command_encoder(&Default::default());
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None, color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
                rp.set_bind_group(0, &bg, &[]);
                rp.set_pipeline(&zpipe);
                for &i in list.iter().filter(|&&i| !s.draws[i as usize].mask) { let d = &s.draws[i as usize];
                    rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
                    let isz = if d.u16 { 2 } else { 4 };
                    rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
                    rp.draw_indexed(0..d.icount, 0, 0..1);
                }
            }
            q.submit([enc.finish()]);
            let e = t.elapsed().as_secs_f32() * 1000.0;
            dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if f >= 3 { st_z.enc.push(e); st_z.wait.push(t.elapsed().as_secs_f32() * 1000.0 - e); }
            let t = Instant::now();
            let mut enc = dev.create_command_encoder(&Default::default());
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    color_attachments: &[att(&gb.hdr), att(&gb.nrm), att(&gb.alb), att(&gb.mat)],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }) });
                rp.set_bind_group(0, &bg, &[]);
                rp.set_pipeline(&r.main_pipe);
                nt = draw_fast(&mut rp, s, &r.main_pipe, &list); nd = list.len();
            }
            q.submit([enc.finish()]);
            let e = t.elapsed().as_secs_f32() * 1000.0;
            dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if f >= 3 { st_c.enc.push(e); st_c.wait.push(t.elapsed().as_secs_f32() * 1000.0 - e); }
            let _ = &mut st_c;
        }
        println!("  in view: {nd} draws, {nt} tris");
        st_z.print(); st_c.print();
        // the rays: from the G-buffer, closest-hit into the hemisphere and any-hit at the sun, one ray a pixel
        q.write_buffer(&ub, 0, bytemuck::bytes_of(&globals(s, cam0, o, aspect, (0.0, 0.0)).0));
        let p = r.rt.as_ref().unwrap(); let sd = s.rt.as_ref().unwrap();
        let src = format!("{}\n{}", include_str!("rt.wgsl"), RAY_BENCH);
        let module = unsafe { dev.create_shader_module_trusted(wgpu::ShaderModuleDescriptor { label: Some("bench"), source: wgpu::ShaderSource::Wgsl(src.into()) }, wgpu::ShaderRuntimeChecks::unchecked()) };
        let l = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&p.trace_bgl)], immediate_size: 0 });
        let mk_p = |ep: &str| dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: Some(&l), module: &module, entry_point: Some(ep), compilation_options: Default::default(), cache: None });
        let (b_c, b_o, b_e) = (mk_p("bench_closest"), mk_p("bench_occluded"), mk_p("bench_empty"));
        let mkt = || dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING, view_formats: &[] }).create_view(&Default::default());
        let (o1, o2, i1, i2) = (mkt(), mkt(), mkt(), mkt());
        use wgpu::{BindGroupEntry as B, BindingResource as R};
        let tb = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.trace_bgl, entries: &[
            B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: sd.nodes.as_entire_binding() }, B { binding: 2, resource: sd.tris.as_entire_binding() },
            B { binding: 3, resource: sd.mats.as_entire_binding() }, B { binding: 4, resource: sd.lights.as_entire_binding() }, B { binding: 5, resource: sd.sky.as_entire_binding() },
            B { binding: 6, resource: R::TextureView(&gb.depth) }, B { binding: 7, resource: R::TextureView(&gb.nrm) }, B { binding: 8, resource: R::TextureView(&gb.alb) },
            B { binding: 9, resource: R::TextureView(&o1) }, B { binding: 10, resource: R::TextureView(&o2) }, B { binding: 11, resource: sd.wins.as_entire_binding() }, B { binding: 12, resource: R::TextureView(&i1) }, B { binding: 13, resource: R::TextureView(&i2) }] });
        let (_, vp, eye) = globals(s, cam0, o, aspect, (0.0, 0.0));
        let pr = rt::Params { inv_vp: vp.inverse().to_cols_array_2d(), cam: eye.extend(1.0).into(), sun_dir: sd.sun.extend(sd.skyp.sun_size * 0.5).into(), sun_col: (sd.sun_e * sd.skyp.strength).extend(0.0).into(),
            dims: [w, h, 1, 1], misc: [3, 1, sd.nlights, sd.sky_wh.0], misc2: [sd.sky_wh.1, 3, sd.nwins, 0], scale: [1.0, sd.skyp.strength, 1.0, sd.skyp.sun_size * 0.5] };
        q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&pr));
        // the G-buffer of this camera
        let (gl, _, _) = globals(s, cam0, o, aspect, (0.0, 0.0));
        q.write_buffer(&ub, 0, bytemuck::bytes_of(&gl));
        let list: Vec<u32> = s.draws.iter().enumerate().filter(|(_, d)| !d.glass).map(|(i, _)| i as u32).collect();
        let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } });
        let mut enc = dev.create_command_encoder(&Default::default());
        { let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                color_attachments: &[att(&gb.hdr), att(&gb.nrm), att(&gb.alb), att(&gb.mat)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
            rp.set_bind_group(0, &bg, &[]); rp.set_pipeline(&r.main_pipe); draw_fast(&mut rp, s, &r.main_pipe, &list); }
        q.submit([enc.finish()]); dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        for (name, pipe) in [("closest (hemisphere) 1 ray/px", &b_c), ("any-hit (sun) 1 ray/px", &b_o), ("empty compute reading gbuffer", &b_e)] {
            let mut st = Stat::new(&format!("{name} {w}x{h}"));
            for f in 0..n / 2 {
                let t = Instant::now();
                let mut enc = dev.create_command_encoder(&Default::default());
                { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(pipe); c.set_bind_group(0, &tb, &[]); c.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1); }
                q.submit([enc.finish()]);
                let e = t.elapsed().as_secs_f32() * 1000.0;
                dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                if f >= 2 { st.enc.push(e); st.wait.push(t.elapsed().as_secs_f32() * 1000.0 - e); }
            }
            st.print();
            let mut v = st.wait.clone(); v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("    = {:.1} M rays/s (if one ray each, {} px)", (w * h) as f32 / v[v.len() / 2] / 1000.0, w * h);
        }
    }
}
const RAY_BENCH: &str = r#"
fn bench_setup(id: vec3<u32>) -> vec3<f32> { seed = id.x * 73856093u ^ id.y * 19349663u; seed = seed * 747796405u + 2891336453u; return vec3<f32>(0.0); }
@compute @workgroup_size(8, 8) fn bench_closest(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let d = textureLoad(g_depth, px, 0); if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(0.0)); return; }
  let pos = world_of(px, d, vec2<f32>(f32(w), f32(h)), p.inv_vp);
  let n = normalize(textureLoad(g_nrm, px, 0).xyz + vec3<f32>(1e-5));
  bench_setup(id);
  let hit = closest(pos + n * 0.002, cosine(n), 1000.0);
  textureStore(out_irr, px, vec4<f32>(hit.t));
}
@compute @workgroup_size(8, 8) fn bench_occluded(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let d = textureLoad(g_depth, px, 0); if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(0.0)); return; }
  let pos = world_of(px, d, vec2<f32>(f32(w), f32(h)), p.inv_vp);
  let n = normalize(textureLoad(g_nrm, px, 0).xyz + vec3<f32>(1e-5));
  bench_setup(id);
  let v = occluded(pos + n * 0.002, p.sun_dir.xyz, 1000.0);
  textureStore(out_irr, px, vec4<f32>(select(0.0, 1.0, v)));
}
@compute @workgroup_size(8, 8) fn bench_empty(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let d = textureLoad(g_depth, px, 0); if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(0.0)); return; }
  let pos = world_of(px, d, vec2<f32>(f32(w), f32(h)), p.inv_vp);
  let n = textureLoad(g_nrm, px, 0).xyz;
  textureStore(out_irr, px, vec4<f32>(pos * dot(n, n), 1.0));
}
"#;

// ---------------- the cache ----------------
const K_PTS: usize = 4;
const EMPTY: u32 = 0xffff_ffff;
#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Pt { pos: [f32; 3], nrm: u32 }
#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Slot { key: u32, e: u32, vis: f32, id: u32, any: u32, all: u32, e_hit: u32, pad: u32 }
#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Bp { lo: [f32; 4], hi: [f32; 4], grid: [u32; 4], run: [u32; 4], fl: [f32; 4], scales: [f32; 4], prev_vp: [[f32; 4]; 4], prev_eye: [f32; 4], frame: [u32; 4] }

#[derive(Default, Clone, Copy)] struct IdHash(u64);
impl std::hash::Hasher for IdHash { fn finish(&self) -> u64 { self.0 } fn write(&mut self, _: &[u8]) { unreachable!() } fn write_u64(&mut self, v: u64) { self.0 = v.wrapping_mul(0x9E37_79B9_7F4A_7C15); } }
type IdMap = HashMap<u64, u32, std::hash::BuildHasherDefault<IdHash>>;
/// a cell is a place, a side, and the part of the scene it is on: nothing lights a rug through the floor under it
const ID_MOD: u32 = 2047;

fn enc_n(n: Vec3) -> u32 {
    let d = n.x.abs() + n.y.abs() + n.z.abs(); let (mut x, mut y) = (n.x / d, n.y / d);
    if n.z < 0.0 { let (ax, ay) = (x.abs(), y.abs()); x = (1.0 - ay) * x.signum(); y = (1.0 - ax) * y.signum(); }
    (((x * 0.5 + 0.5) * 65535.0 + 0.5) as u32) | ((((y * 0.5 + 0.5) * 65535.0 + 0.5) as u32) << 16)
}
struct Grid { lo: Vec3, hi: Vec3, cell: f32, nx: u32, ny: u32 }
impl Grid {
    fn level(&self, q: Vec3) -> u32 { let d = (self.lo - q).max(q - self.hi).max(Vec3::ZERO); let m = d.max_element(); if m <= 0.0 { 0 } else if m < 16.0 { 1 } else if m < 128.0 { 2 } else { 3 } }
    fn cell_of(&self, l: u32) -> f32 { self.cell * (1u32 << (3 * l)) as f32 }
    fn key(&self, l: u32, q: Vec3, side: u32) -> u32 {
        let c = self.cell_of(l);
        if l == 0 { let v = (q - self.lo) / c; (v.x as u32 + self.nx * (v.y as u32 + self.ny * v.z as u32)) * 6 + side }
        else { let v = q - Vec3::new(-c * 512.0, -c * 512.0, -c * 32.0); let (x, y, z) = ((v.x / c).floor() as i32 as u32, (v.y / c).floor() as i32 as u32, (v.z / c).floor() as i32 as u32);
            0x8000_0000 | ((l - 1) << 29) | (x & 1023) | ((y & 1023) << 10) | ((z & 63) << 20) | (side << 26) }
    }
}
fn side_of(n: Vec3) -> u32 { let a = n.abs(); if a.x >= a.y && a.x >= a.z { if n.x >= 0.0 { 0 } else { 1 } } else if a.y >= a.z { if n.y >= 0.0 { 2 } else { 3 } } else if n.z >= 0.0 { 4 } else { 5 } }
/// every surface as points closer together than a cell, on both its sides: each cell keeps a few of those that fell in it
fn build_cells(s: &Scene, g: &Grid) -> (Vec<(u32, u32)>, Vec<Pt>) {
    let mut map: IdMap = IdMap::default(); let mut keys: Vec<(u32, u32)> = vec![]; let mut pts: Vec<[Pt; K_PTS]> = vec![]; let mut cnt: Vec<u32> = vec![];
    let mut rng = 0x1234_5678u32;
    let mut stack: Vec<[Vec3; 3]> = vec![];
    let mut add = |key: u32, id: u32, q: Vec3, n: Vec3, rng: &mut u32| {
        let i = *map.entry(key as u64 | ((id as u64) << 32)).or_insert_with(|| { keys.push((key, id)); pts.push([Pt { pos: [0.0; 3], nrm: 0 }; K_PTS]); cnt.push(0); keys.len() as u32 - 1 }) as usize;
        let c = cnt[i]; cnt[i] += 1;
        let p = Pt { pos: q.into(), nrm: enc_n(n) };
        if (c as usize) < K_PTS { for k in c as usize..K_PTS { pts[i][k] = p; } }
        else { *rng ^= *rng << 13; *rng ^= *rng >> 17; *rng ^= *rng << 5; let k = *rng % (c + 1); if (k as usize) < K_PTS { pts[i][k as usize] = p; } }
    };
    for (di, d) in s.draws.iter().enumerate() {
        if d.glass { continue }
        let id = di as u32 % ID_MOD;
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |k: usize| -> Vec3 { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); Vec3::new(f(0), f(1), f(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t in 0..d.icount as usize / 3 {
            let (a, b, c) = (p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2)));
            let cr = (b - a).cross(c - a); let ar = cr.length(); if ar < 1e-10 { continue }
            let n = cr / ar;
            stack.push([a, b, c]);
            while let Some([a, b, c]) = stack.pop() {
                let m = (a + b + c) / 3.0; let l = g.level(m); let lim = g.cell_of(l) * 0.7;
                let (ab, bc, ca) = ((b - a).length_squared(), (c - b).length_squared(), (a - c).length_squared());
                let mx = ab.max(bc).max(ca);
                if mx > lim * lim {
                    if ab == mx { let h = (a + b) * 0.5; stack.push([a, h, c]); stack.push([h, b, c]); }
                    else if bc == mx { let h = (b + c) * 0.5; stack.push([a, b, h]); stack.push([a, h, c]); }
                    else { let h = (c + a) * 0.5; stack.push([a, b, h]); stack.push([h, b, c]); }
                    continue
                }
                add(g.key(l, m, side_of(n)), id, m, n, &mut rng);
                add(g.key(l, m, side_of(-n)), id, m, -n, &mut rng);
            }
        }
    }
    (keys, pts.into_iter().flatten().collect())
}
fn hash_table(keys: &[(u32, u32)], aliases: &[(u32, u32, u32)]) -> (Vec<Slot>, u32) {
    let bits = ((keys.len() + aliases.len()) * 2).next_power_of_two().trailing_zeros().max(8);
    let n = 1usize << bits; let mask = n - 1;
    let mut t = vec![Slot { key: EMPTY, e: 0, vis: 0.0, id: 0, any: 0, all: 0, e_hit: 0, pad: 0 }; n];
    let mut worst = 0;
    for (k, id, i) in keys.iter().enumerate().map(|(i, &(k, id))| (k, id, i as u32)).chain(aliases.iter().copied()) {
        let mut h = k ^ id.wrapping_mul(0x9E37_79B1); h ^= h >> 15; let mut s = (h.wrapping_mul(2654435761) >> (32 - bits)) as usize; let mut c = 0;
        while t[s].key != EMPTY { s = (s + 1) & mask; c += 1; }
        worst = worst.max(c);
        t[s] = Slot { key: k, e: i, vis: 0.0, id, any: 0, all: 0, e_hit: 0, pad: 0 };
    }
    if worst >= 32 { eprintln!("  bake: long probe chain {worst}"); }
    (t, bits)
}

// ---------------- pipelines and targets ----------------
const F16: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
struct Pipes {
    g0_bgl: wgpu::BindGroupLayout, validate: (wgpu::ComputePipeline, wgpu::BindGroupLayout), round: (wgpu::ComputePipeline, wgpu::BindGroupLayout), filter: (wgpu::ComputePipeline, wgpu::BindGroupLayout), lamps: (wgpu::ComputePipeline, wgpu::BindGroupLayout), mix: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    spec: (wgpu::ComputePipeline, wgpu::BindGroupLayout), specf: (wgpu::ComputePipeline, wgpu::BindGroupLayout), compose: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    tone: (wgpu::ComputePipeline, wgpu::BindGroupLayout), taa: (wgpu::ComputePipeline, wgpu::BindGroupLayout), fin: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    z: wgpu::RenderPipeline, zmask: wgpu::RenderPipeline,
}
#[derive(Clone, Copy)] enum K { Lut3, Uni, Sto, StoRw, Tex, Filt, Smp, Out16, Out8 }
fn bgl(dev: &wgpu::Device, e: &[(u32, K)]) -> wgpu::BindGroupLayout {
    use wgpu::{BindGroupLayoutEntry as E, BindingType as T};
    let v: Vec<E> = e.iter().map(|&(b, k)| E { binding: b, visibility: wgpu::ShaderStages::COMPUTE, count: None, ty: match k {
        K::Uni => T::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        K::Sto => T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
        K::StoRw => T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
        K::Tex => T::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        K::Filt => T::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        K::Smp => T::Sampler(wgpu::SamplerBindingType::Filtering),
        K::Lut3 => T::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false },
        K::Out16 => T::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: F16, view_dimension: wgpu::TextureViewDimension::D2 },
        K::Out8 => T::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 },
    } }).collect();
    dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &v })
}
fn make_pipes(r: &Renderer) -> Pipes {
    let dev = &r.gpu.dev;
    let src = format!("{}\n{}", include_str!("rt.wgsl"), include_str!("bake.wgsl"));
    let module = unsafe { dev.create_shader_module_trusted(wgpu::ShaderModuleDescriptor { label: Some("bake"), source: wgpu::ShaderSource::Wgsl(src.into()) }, wgpu::ShaderRuntimeChecks::unchecked()) };
    use wgpu::{BindGroupLayoutEntry as E, BindingType as T};
    let vis = wgpu::ShaderStages::COMPUTE;
    let buf = |b, ty| E { binding: b, visibility: vis, count: None, ty: T::Buffer { ty, has_dynamic_offset: false, min_binding_size: None } };
    let ro = wgpu::BufferBindingType::Storage { read_only: true };
    let tx = |b| E { binding: b, visibility: vis, count: None, ty: T::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } };
    let g0_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[buf(0, wgpu::BufferBindingType::Uniform), buf(1, ro), buf(2, ro), buf(3, ro), buf(4, ro), buf(5, ro),
        E { binding: 6, visibility: vis, count: None, ty: T::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } }, tx(7), tx(8), buf(11, ro)] });
    let mk = |ep: &str, e: &[(u32, K)]| {
        let l1 = bgl(dev, e);
        let l = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&g0_bgl), Some(&l1)], immediate_size: 0 });
        (dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(ep), layout: Some(&l), module: &module, entry_point: Some(ep), compilation_options: Default::default(), cache: None }), l1)
    };
    use K::*;
    let validate = mk("bk_validate", &[(0, Uni), (2, Sto), (5, StoRw)]);
    let round = mk("bk_round", &[(0, Uni), (1, Sto), (2, Sto), (3, Sto), (4, StoRw)]);
    let filter = mk("bk_filter", &[(0, Uni), (1, Sto), (3, Sto), (4, StoRw)]);
    let lamps = mk("bk_lamps", &[(0, Uni), (2, Sto), (7, StoRw)]);
    let mix = mk("bk_mix", &[(0, Uni), (1, Sto), (3, Sto), (6, StoRw), (8, Sto)]);
    let spec = mk("bk_spec", &[(0, Uni), (1, Sto), (16, Out16)]);
    let specf = mk("bk_specf", &[(0, Uni), (15, Tex), (16, Out16)]);
    let compose = mk("bk_compose", &[(0, Uni), (1, Sto), (10, Tex), (11, Tex), (12, Tex), (13, Filt), (14, Out16), (18, Smp)]);
    let tone = mk("bk_tone", &[(0, Uni), (15, Tex), (16, Out16), (17, Lut3), (18, Smp)]);
    let taa = mk("bk_taa", &[(0, Uni), (15, Tex), (16, Out16), (17, Filt), (18, Smp)]);
    let fin = mk("bk_final", &[(0, Uni), (15, Tex), (19, Out8)]);
    // the prepass
    let sm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(shader(SRC).replace("/*LIB*/", ig_shade::wgsl::LIB).replace("/*SURF*/", "").into()) });
    let vl = vbuf_layouts();
    let ds = wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() };
    let pz = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&r.g_bgl)], immediate_size: 0 });
    let z = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pz),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(vl[0].clone())] },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() }, depth_stencil: Some(ds.clone()), multisample: Default::default(), fragment: None, multiview_mask: None, cache: None });
    let pm = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&r.g_bgl), Some(&r.mat_bgl)], immediate_size: 0 });
    let zmask = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pm),
        vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_z"), compilation_options: Default::default(), buffers: &vl.iter().cloned().map(Some).collect::<Vec<_>>() },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() }, depth_stencil: Some(ds), multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: &sm, entry_point: Some("fs_z"), compilation_options: Default::default(), targets: &[] }), multiview_mask: None, cache: None });
    Pipes { g0_bgl, validate, round, filter, lamps, mix, spec, specf, compose, tone, taa, fin, z, zmask }
}
fn tex(dev: &wgpu::Device, w: u32, h: u32, f: wgpu::TextureFormat, u: wgpu::TextureUsages) -> (wgpu::Texture, wgpu::TextureView) {
    let t = dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: f, usage: u, view_formats: &[] });
    let v = t.create_view(&Default::default()); (t, v)
}
fn sto(dev: &wgpu::Device, b: &[u8], extra: wgpu::BufferUsages) -> wgpu::Buffer { dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: b, usage: wgpu::BufferUsages::STORAGE | extra }) }
macro_rules! bg { ($dev:expr, $l:expr, $($b:expr => $r:expr),* $(,)?) => { $dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: $l, entries: &[$(wgpu::BindGroupEntry { binding: $b, resource: $r }),*] }) } }
fn tv(v: &wgpu::TextureView) -> wgpu::BindingResource<'_> { wgpu::BindingResource::TextureView(v) }
fn dispatch(enc: &mut wgpu::CommandEncoder, pipe: &wgpu::ComputePipeline, g0: &wgpu::BindGroup, g1: &wgpu::BindGroup, w: u32, h: u32) {
    let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(pipe); c.set_bind_group(0, g0, &[]); c.set_bind_group(1, g1, &[]); c.dispatch_workgroups(w, h, 1);
}

// ---------------- the baker ----------------
pub struct Baked { table: wgpu::Buffer, base: Bp, ncells: u32, nslots: u32, seconds: f32, rounds: u32 }
fn params(s: &Scene, inv_vp: Mat4, eye: Vec3, w: u32, h: u32, show: u32) -> rt::Params {
    let sd = s.rt.as_ref().unwrap();
    rt::Params { inv_vp: inv_vp.to_cols_array_2d(), cam: eye.extend(1.0).into(), sun_dir: sd.sun.extend(sd.skyp.sun_size * 0.5).into(), sun_col: (sd.sun_e * sd.skyp.strength).extend(0.0).into(),
        dims: [w, h, 1, 1], misc: [0, 1, sd.nlights, sd.sky_wh.0], misc2: [sd.sky_wh.1, 0, sd.nwins, show], scale: [1.0, sd.skyp.strength, 1.0, sd.skyp.sun_size * 0.5] }
}
fn wait(dev: &wgpu::Device) { dev.poll(wgpu::PollType::wait_indefinitely()).unwrap(); }
/// how far the bake has got, for a page to show
#[cfg(target_arch = "wasm32")]
pub fn progress(f: f32) { crate::web::progress(f); }
#[cfg(not(target_arch = "wasm32"))]
pub fn progress(_f: f32) {}
/// When a bake stops: after so many seconds, or when the light has settled to this estimated error (mean relative error of a cell; `Stop::CEILING` seconds at most)
#[derive(Clone, Copy, Debug)]
pub enum Stop { Time(f32), Quality(f32) }
impl Stop {
    pub const DEFAULT: Stop = Stop::Quality(0.08);
    pub const CEILING: f32 = 180.0;
    /// "25": seconds. "q0.04": a quality bar. Nothing: the default bar
    pub fn parse(s: &str) -> Stop { if let Some(q) = s.strip_prefix('q') { Stop::Quality(q.parse().expect("bar")) } else if s.is_empty() { Stop::DEFAULT } else { Stop::Time(s.parse().expect("seconds")) } }
}
pub async fn bake(r: &Renderer, s: &Scene, pp: &Pipes, stop: Stop, verbose: bool) -> Baked {
    let t0 = Instant::now();
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let sd = s.rt.as_ref().unwrap();
    // the house's own box; what lies outside has coarser cells
    let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if !nm.starts_with("outdoor") && !nm.starts_with("exterior") { lo = lo.min(d.bmin); hi = hi.max(d.bmax); } }
    lo -= Vec3::splat(0.1); hi += Vec3::splat(0.1);
    let mut cell = std::env::var("BAKE_CELL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05f32);
    while { let e = (hi - lo) / cell; (e.x as f64 + 1.0) * (e.y as f64 + 1.0) * (e.z as f64 + 1.0) * 6.0 > 2.0e9 } { cell *= 1.25; }
    let e = (hi - lo) / cell; let (nx, ny) = (e.x as u32 + 1, e.y as u32 + 1);
    let g = Grid { lo, hi, cell, nx, ny };
    let (mut keys, mut pts) = build_cells(s, &g);
    if verbose { eprintln!("  bake: {} cells (both sides), {:.2}s", keys.len(), t0.elapsed().as_secs_f32()); }
    let mut base = Bp { lo: lo.extend(cell).into(), hi: hi.extend(1.2).into(), grid: [nx, ny, 0, 0], run: [0; 4], fl: [1.0, 3.0, 0.0, 1.0], scales: [1.0; 4], prev_vp: Mat4::IDENTITY.to_cols_array_2d(), prev_eye: [0.0; 4], frame: [0; 4] };
    let ub = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Bp>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let pub_ = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&params(s, Mat4::IDENTITY, Vec3::ZERO, 8, 8, 0)), usage: wgpu::BufferUsages::UNIFORM });
    let (_, dummy_d) = tex(dev, 8, 8, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
    let (_, dummy) = tex(dev, 8, 8, F16, wgpu::TextureUsages::TEXTURE_BINDING);
    let g0 = bg!(dev, &pp.g0_bgl, 0 => pub_.as_entire_binding(), 1 => sd.nodes.as_entire_binding(), 2 => sd.tris.as_entire_binding(), 3 => sd.mats.as_entire_binding(), 4 => sd.lights.as_entire_binding(), 5 => sd.sky.as_entire_binding(), 6 => tv(&dummy_d), 7 => tv(&dummy), 8 => tv(&dummy), 11 => sd.wins.as_entire_binding());
    // what lies inside a solid is dropped: a third to a half of the points
    let ptb = sto(dev, bytemuck::cast_slice(&pts), wgpu::BufferUsages::empty());
    let flb = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: pts.len() as u64 * 4, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let rbf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: pts.len() as u64 * 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let bgv = bg!(dev, &pp.validate.1, 0 => ub.as_entire_binding(), 2 => ptb.as_entire_binding(), 5 => flb.as_entire_binding());
    let chunk = 1u32 << 18;
    let np = pts.len() as u32;
    for off in (0..np).step_by(chunk as usize) {
        base.run = [0, off, np, 0]; q.write_buffer(&ub, 0, bytemuck::bytes_of(&base));
        let mut enc = dev.create_command_encoder(&Default::default());
        dispatch(&mut enc, &pp.validate.0, &g0, &bgv, chunk / 64, 1);
        q.submit([enc.finish()]); gpu_done(dev, q).await;
    }
    let mut aliases: Vec<(u32, u32, u32)> = vec![];
    let mut enc = dev.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(&flb, 0, &rbf, 0, pts.len() as u64 * 4);
    q.submit([enc.finish()]);
    map_read(dev, &[rbf.slice(..)]).await;
    {
        let data = rbf.slice(..).get_mapped_range().unwrap(); let fl: &[u32] = bytemuck::cast_slice(&data);
        let (mut k2, mut p2) = (Vec::with_capacity(keys.len()), Vec::with_capacity(pts.len()));
        let mut dropped: Vec<(u32, u32)> = vec![];
        for (i, &k) in keys.iter().enumerate() {
            let c = &pts[i * K_PTS..(i + 1) * K_PTS]; let f = &fl[i * K_PTS..(i + 1) * K_PTS];
            let want = if f.iter().any(|v| *v == 1) { 1 } else { 2 };
            let Some(first) = (0..K_PTS).find(|&j| f[j] == want) else { dropped.push(k); continue };
            k2.push(k);
            for j in 0..K_PTS { p2.push(if f[j] == want { c[j] } else { c[first] }); }
        }
        if verbose { eprintln!("  bake: {} of {} cells outside solids, {:.2}s", k2.len(), keys.len(), t0.elapsed().as_secs_f32()); }
        keys = k2; pts = p2;
        // (a cell dropped may still be seen: a sliver of moulding in a crack. It takes the light of the nearest cell kept, of its part or else of any on that side)
        let mut by_id: IdMap = IdMap::default(); let mut by_pos: IdMap = IdMap::default();
        for (i, &(k, id)) in keys.iter().enumerate() { by_id.insert(k as u64 | ((id as u64) << 32), i as u32); by_pos.insert(k as u64, i as u32); }
        let offs: Vec<[i32; 3]> = { let mut v = vec![]; for x in -2i32..=2 { for y in -2i32..=2 { for z in -2i32..=2 { if (x, y, z) != (0, 0, 0) { v.push([x, y, z]); } } } } v.sort_by_key(|o| o[0] * o[0] + o[1] * o[1] + o[2] * o[2]); v };
        for &(k, id) in &dropped {
            if k & 0x8000_0000 != 0 { continue }
            let (sd, q) = (k % 6, k / 6); let (x, y, z) = ((q % nx) as i32, ((q / nx) % ny) as i32, (q / (nx * ny)) as i32);
            let key_at = |o: &[i32; 3]| -> Option<u32> { let (a, b, c) = (x + o[0], y + o[1], z + o[2]); if a < 0 || b < 0 || c < 0 || a as u32 >= nx || b as u32 >= ny { None } else { Some((a as u32 + nx * (b as u32 + ny * c as u32)) * 6 + sd) } };
            let t = offs.iter().filter_map(|o| key_at(o).and_then(|kk| by_id.get(&(kk as u64 | ((id as u64) << 32))))).next()
                .or_else(|| by_pos.get(&(k as u64))).or_else(|| offs.iter().filter_map(|o| key_at(o).and_then(|kk| by_pos.get(&(kk as u64)))).next());
            if let Some(&t) = t { aliases.push((k, id, t)); }
        }
        if verbose { eprintln!("  bake: {} of {} dropped cells point at a neighbour, {:.2}s", aliases.len(), dropped.len(), t0.elapsed().as_secs_f32()); }
    }
    drop(flb); drop(rbf); drop(ptb);
    let (slots, bits) = hash_table(&keys, &aliases);
    base.grid[2] = (1u32 << bits) - 1; base.grid[3] = 32 - bits;
    let nc = keys.len() as u32; let nslots = slots.len() as u32;
    let tb = sto(dev, bytemuck::cast_slice(&slots), wgpu::BufferUsages::empty());
    let ptb = sto(dev, bytemuck::cast_slice(&pts), wgpu::BufferUsages::empty());
    let cv = |_: u32| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: nc as u64 * 96, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let cvs = [cv(0), cv(1)];
    let bgr = [0, 1].map(|i: usize| bg!(dev, &pp.round.1, 0 => ub.as_entire_binding(), 1 => tb.as_entire_binding(), 2 => ptb.as_entire_binding(), 3 => cvs[i].as_entire_binding(), 4 => cvs[1 - i].as_entire_binding()));
    let lmb = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: nc as u64 * 8, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    let bgl_ = bg!(dev, &pp.lamps.1, 0 => ub.as_entire_binding(), 2 => ptb.as_entire_binding(), 7 => lmb.as_entire_binding());
    for off in (0..nc).step_by(1 << 17) {
        base.run = [0, off, nc, 0]; q.write_buffer(&ub, 0, bytemuck::bytes_of(&base));
        let mut enc = dev.create_command_encoder(&Default::default());
        dispatch(&mut enc, &pp.lamps.0, &g0, &bgl_, (1 << 17) / 64, 1);
        q.submit([enc.finish()]); gpu_done(dev, q).await;
    }
    if verbose { eprintln!("  bake: lamps seen, {:.2}s", t0.elapsed().as_secs_f32()); }
    let samples = 2u32; let (mut round, mut cur) = (0u32, 0usize);
    let chunk = 1u32 << 17;
    let t_r = Instant::now();
    // How settled the light is: a random sample of cells compared between two rounds. Their difference over the rounds between says what error remains (of a running mean: |X(n+k) - X(n)| * sqrt(n / k))
    // (16 runs of cells spread through the buffer: cells lie part by part, so one run would see a single object)
    let run_cells = (nc as u64 / 16).min(4096);
    let sample = run_cells * 96 * 16;
    let rb_err = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: sample, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    // (only cells inside the house's own box: what is looked at. A garden's leaves are the noisiest cells and hardly seen)
    let inside: Vec<bool> = (0..16u64).flat_map(|r| (0..run_cells).map(move |i| (r * (nc as u64 / 16) + i) as usize)).map(|i| { let q = Vec3::from(pts[i * K_PTS].pos); q.cmpge(lo).all() && q.cmple(hi).all() }).collect();
    let mut prev_light: Option<(Vec<f32>, u32)> = None; let mut err = f32::MAX; let mut next_check = 4u32;
    let done = |round: u32, err: f32, t: f32| match stop { Stop::Time(b) => t >= b && round >= 4, Stop::Quality(q) => err <= q || t >= Stop::CEILING };
    while !done(round, err, t0.elapsed().as_secs_f32()) && round < 4000 {
        progress(match stop { Stop::Time(b) => t0.elapsed().as_secs_f32() / b.max(0.1), Stop::Quality(q) => if err == f32::MAX { 0.0 } else { (q / err) * (q / err) } });
        base.fl[0] = 2.0 / (round as f32 + 2.0);
        for off in (0..nc).step_by(chunk as usize) {
            base.run = [round, off, nc, samples]; q.write_buffer(&ub, 0, bytemuck::bytes_of(&base));
            let mut enc = dev.create_command_encoder(&Default::default());
            dispatch(&mut enc, &pp.round.0, &g0, &bgr[cur], chunk / 64, 1);
            q.submit([enc.finish()]); gpu_done(dev, q).await;
        }
        cur = 1 - cur; round += 1;
        if round == next_check {
            let mut enc = dev.create_command_encoder(&Default::default());
            for k in 0..16u64 { enc.copy_buffer_to_buffer(&cvs[cur], k * (nc as u64 / 16) * 96, &rb_err, k * run_cells * 96, run_cells * 96); }
            q.submit([enc.finish()]);
            map_read(dev, &[rb_err.slice(..)]).await;
            let light: Vec<f32> = { let m = rb_err.slice(..).get_mapped_range().unwrap(); let v: &[f32] = bytemuck::cast_slice(&m);
                v.chunks_exact(24).map(|c| (0..6).map(|i| c[i * 4] + c[i * 4 + 1] + c[i * 4 + 2]).sum::<f32>()).collect() };
            rb_err.unmap();
            if let Some((p, n)) = &prev_light {
                // cell by cell, so a dim room counts as much as the sunlit outside of the house, which settles at once and would swamp a sum by brightness. (Cells far below the typical one are as good as dark: their noise doesn't show)
                let mut sorted: Vec<f32> = light.iter().copied().filter(|v| *v > 0.0).collect();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let floor = if sorted.is_empty() { 1e-6 } else { sorted[sorted.len() / 2] * 0.1 };
                let (mut d, mut c) = (0f32, 0f32);
                for (k, (a, b)) in light.iter().zip(p).enumerate() { if *a + *b > 0.0 && inside[k] { d += (a - b).abs() / (0.5 * (a + b) + floor); c += 1.0; } }
                err = d / c.max(1.0) * (*n as f32 / (round - n) as f32).sqrt();
                if std::env::var_os("BAKE_ERRLOG").is_some() { eprintln!("  round {round} err {err:.4} {:.1}s", t0.elapsed().as_secs_f32()); }
            }
            prev_light = Some((light, round));
            next_check = round + (round / 4).max(4);
        }
    }
    if verbose { eprintln!("  bake: {round} rounds, error {err:.4}, {:.2}s each, {:.2}s total", t_r.elapsed().as_secs_f32() / round.max(1) as f32, t0.elapsed().as_secs_f32()); }
    let tf = Instant::now();
    let passes: Vec<u32> = std::env::var("BAKE_FILTER").unwrap_or("4,2,1".into()).split(',').filter_map(|v| v.parse().ok()).collect();
    let bgf = [0, 1].map(|i: usize| bg!(dev, &pp.filter.1, 0 => ub.as_entire_binding(), 1 => tb.as_entire_binding(), 3 => cvs[i].as_entire_binding(), 4 => cvs[1 - i].as_entire_binding()));
    for st in passes {
        for off in (0..nslots).step_by(1 << 18) {
            base.run = [st, off, nslots, 0]; q.write_buffer(&ub, 0, bytemuck::bytes_of(&base));
            let mut enc = dev.create_command_encoder(&Default::default());
            dispatch(&mut enc, &pp.filter.0, &g0, &bgf[cur], (1 << 18) / 64, 1);
            q.submit([enc.finish()]); gpu_done(dev, q).await;
        }
        cur = 1 - cur;
    }
    if verbose { eprintln!("  bake: filtered {:.2}s", tf.elapsed().as_secs_f32()); }
    // packed for a frame
    let table = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: nslots as u64 * 32, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    let sc = |k: &str| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(1.0f32);
    base.scales = [sc("RT_LAMP"), sc("RT_SKY"), sc("RT_SUN"), 1.0];
    let bgm = bg!(dev, &pp.mix.1, 0 => ub.as_entire_binding(), 1 => tb.as_entire_binding(), 3 => cvs[cur].as_entire_binding(), 6 => table.as_entire_binding(), 8 => lmb.as_entire_binding());
    for off in (0..nslots).step_by(1 << 20) {
        base.run = [0, off, nslots, 0]; q.write_buffer(&ub, 0, bytemuck::bytes_of(&base));
        let mut enc = dev.create_command_encoder(&Default::default());
        dispatch(&mut enc, &pp.mix.0, &g0, &bgm, (1 << 20) / 64, 1);
        q.submit([enc.finish()]); gpu_done(dev, q).await;
    }
    base.run = [0; 4];
    Baked { table, base, ncells: nc, nslots, seconds: t0.elapsed().as_secs_f32(), rounds: round }
}

// ---------------- a frame ----------------
struct Frame {
    gb: Gb, mirror: Gb, m_lit: (wgpu::Texture, wgpu::TextureView), m_tmp: (wgpu::Texture, wgpu::TextureView), ub_gm: wgpu::Buffer, ub_pm: wgpu::Buffer, ub_bm: wgpu::Buffer, g_bgm: wgpu::BindGroup, g0m: wgpu::BindGroup, b_cm: wgpu::BindGroup, b_sm: wgpu::BindGroup, b_fm: wgpu::BindGroup, lit: wgpu::TextureView, disp: wgpu::TextureView, hist: [wgpu::TextureView; 2], specs: [wgpu::TextureView; 3], out: (wgpu::Texture, wgpu::TextureView), black: wgpu::TextureView,
    rb: wgpu::Buffer, bpr: u32, res: (u32, u32),
    ub_g: wgpu::Buffer, ub_p: wgpu::Buffer, ub_b: wgpu::Buffer, ub_b2: wgpu::Buffer, g_bg: wgpu::BindGroup, g0: wgpu::BindGroup,
    b_spec: wgpu::BindGroup, b_f: [wgpu::BindGroup; 2], b_compose: wgpu::BindGroup, b_tone: wgpu::BindGroup, b_taa: [wgpu::BindGroup; 2], b_fin: [wgpu::BindGroup; 2],
    /// draws in material order, and door parts by door
    order: Vec<u32>, doors: Vec<(Vec<u32>, Vec<(Vec3, Vec3)>)>, hidden: Vec<bool>,
    cur: usize, n: u32, prev_vp: Mat4, prev_eye: Vec3, show: u32,
}
fn make_frame(r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, res: (u32, u32), scale: f32) -> Frame {
    let dev = &r.gpu.dev; let sd = s.rt.as_ref().unwrap();
    let (w, h) = ((res.0 as f32 * scale) as u32 & !1, (res.1 as f32 * scale) as u32 & !1);
    use wgpu::TextureUsages as U;
    let gb = make_gb(dev, w, h); let mirror = make_gb(dev, res.0, res.1);
    let lit = tex(dev, w, h, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING | U::RENDER_ATTACHMENT).1;
    let disp = tex(dev, w, h, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING).1;
    let hist = [tex(dev, w, h, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING).1, tex(dev, w, h, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING).1];
    let specs = [0, 1, 2].map(|_| tex(dev, w / 2, h / 2, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING).1);
    let out = tex(dev, res.0, res.1, wgpu::TextureFormat::Rgba8Unorm, U::STORAGE_BINDING | U::COPY_SRC);
    let black = tex(dev, 4, 4, F16, U::TEXTURE_BINDING).1;
    let m_lit = tex(dev, res.0, res.1, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING | U::COPY_DST | U::RENDER_ATTACHMENT);
    let m_tmp = tex(dev, res.0, res.1, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING | U::COPY_SRC);
    let bpr = (res.0 * 4 + 255) / 256 * 256;
    let rb = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * res.1) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let ubuf = |sz: usize| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: sz as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let (ub_g, ub_p, ub_b, ub_b2) = (ubuf(std::mem::size_of::<Globals>()), ubuf(std::mem::size_of::<rt::Params>()), ubuf(std::mem::size_of::<Bp>()), ubuf(std::mem::size_of::<Bp>()));
    let g_bg_ = g_bg(r, s, &ub_g);
    let g0 = bg!(dev, &pp.g0_bgl, 0 => ub_p.as_entire_binding(), 1 => sd.nodes.as_entire_binding(), 2 => sd.tris.as_entire_binding(), 3 => sd.mats.as_entire_binding(), 4 => sd.lights.as_entire_binding(), 5 => sd.sky.as_entire_binding(), 6 => tv(&gb.depth), 7 => tv(&gb.nrm), 8 => tv(&gb.alb), 11 => sd.wins.as_entire_binding());
    let smp = dev.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    let b_spec = bg!(dev, &pp.spec.1, 0 => ub_b.as_entire_binding(), 1 => bk.table.as_entire_binding(), 16 => tv(&specs[0]));
    let b_f = [bg!(dev, &pp.specf.1, 0 => ub_b.as_entire_binding(), 15 => tv(&specs[0]), 16 => tv(&specs[1])), bg!(dev, &pp.specf.1, 0 => ub_b2.as_entire_binding(), 15 => tv(&specs[1]), 16 => tv(&specs[2]))];
    let b_compose = bg!(dev, &pp.compose.1, 0 => ub_b.as_entire_binding(), 1 => bk.table.as_entire_binding(), 10 => tv(&gb.mat), 11 => tv(&gb.hdr), 12 => tv(&specs[2]), 13 => tv(&m_lit.1), 14 => tv(&lit), 18 => wgpu::BindingResource::Sampler(&smp));
    let b_tone = bg!(dev, &pp.tone.1, 0 => ub_b.as_entire_binding(), 15 => tv(&lit), 16 => tv(&disp), 17 => wgpu::BindingResource::TextureView(&r.agx), 18 => wgpu::BindingResource::Sampler(&r.agx_smp));
    let b_taa = [0, 1].map(|i: usize| bg!(dev, &pp.taa.1, 0 => ub_b.as_entire_binding(), 15 => tv(&disp), 16 => tv(&hist[i]), 17 => tv(&hist[1 - i]), 18 => wgpu::BindingResource::Sampler(&smp)));
    let b_fin = [0, 1].map(|i: usize| bg!(dev, &pp.fin.1, 0 => ub_b.as_entire_binding(), 15 => tv(&hist[i]), 19 => tv(&out.1)));
    let (ub_gm, ub_pm, ub_bm) = (ubuf(std::mem::size_of::<Globals>()), ubuf(std::mem::size_of::<rt::Params>()), ubuf(std::mem::size_of::<Bp>()));
    let g_bgm = g_bg(r, s, &ub_gm);
    let g0m = bg!(dev, &pp.g0_bgl, 0 => ub_pm.as_entire_binding(), 1 => sd.nodes.as_entire_binding(), 2 => sd.tris.as_entire_binding(), 3 => sd.mats.as_entire_binding(), 4 => sd.lights.as_entire_binding(), 5 => sd.sky.as_entire_binding(), 6 => tv(&mirror.depth), 7 => tv(&mirror.nrm), 8 => tv(&mirror.alb), 11 => sd.wins.as_entire_binding());
    let sm = [0, 1].map(|_| tex(dev, res.0 / 2, res.1 / 2, F16, U::STORAGE_BINDING | U::TEXTURE_BINDING).1);
    let b_sm = bg!(dev, &pp.spec.1, 0 => ub_bm.as_entire_binding(), 1 => bk.table.as_entire_binding(), 16 => tv(&sm[0]));
    let b_fm = bg!(dev, &pp.specf.1, 0 => ub_bm.as_entire_binding(), 15 => tv(&sm[0]), 16 => tv(&sm[1]));
    let b_cm = bg!(dev, &pp.compose.1, 0 => ub_bm.as_entire_binding(), 1 => bk.table.as_entire_binding(), 10 => tv(&mirror.mat), 11 => tv(&mirror.hdr), 12 => tv(&sm[1]), 13 => tv(&black), 14 => tv(&m_tmp.1), 18 => wgpu::BindingResource::Sampler(&smp));
    let mut order: Vec<u32> = (0..s.draws.len() as u32).filter(|&i| !s.draws[i as usize].glass).collect();
    order.sort_by_key(|&i| s.draws[i as usize].pipe.map_or(0, |k| k + 1));
    let mut by: HashMap<String, (Vec<u32>, Vec<(Vec3, Vec3)>)> = HashMap::new();
    for (i, d) in s.draws.iter().enumerate() {
        let nm = &s.glb.nodes[d.node].name;
        if !nm.starts_with("door") || nm.contains("frame") || nm.contains("casing") { continue }
        let e = by.entry(nm.split('.').next().unwrap_or(nm).to_string()).or_default();
        e.0.push(i as u32); e.1.push((d.bmin, d.bmax));
    }
    let show = match std::env::var("RT_SHOW").as_deref() { Ok("albedo") => 2, Ok("sun") => 5, Ok("normal") => 6, Ok("flags") => 7, Ok("light") => 3, Ok("spec") => 4, _ => 0 };
    Frame { gb, mirror, m_lit, m_tmp, ub_gm, ub_pm, ub_bm, g_bgm, g0m, b_cm, b_sm, b_fm, lit, disp, hist, specs, out, black, rb, bpr, res, ub_g, ub_p, ub_b, ub_b2, g_bg: g_bg_, g0, b_spec, b_f, b_compose, b_tone, b_taa, b_fin, order, doors: by.into_values().collect(), hidden: vec![false; s.draws.len()], cur: 0, n: 0, prev_vp: Mat4::IDENTITY, prev_eye: Vec3::ZERO, show }
}
fn halton(mut i: u32, b: u32) -> f32 { let (mut f, mut r) = (1.0, 0.0); while i > 0 { f /= b as f32; r += f * (i % b) as f32; i /= b; } r }
fn z_pass<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, pp: &'a Pipes, list: &[u32]) {
    rp.set_pipeline(&pp.z);
    for &i in list.iter().filter(|&&i| !s.draws[i as usize].mask) { let d = &s.draws[i as usize];
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, 0..1);
    }
    let mut set = false;
    for &i in list.iter().filter(|&&i| s.draws[i as usize].mask) { let d = &s.draws[i as usize];
        if !set { rp.set_pipeline(&pp.zmask); set = true; }
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
        match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
        rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, 0..1);
    }
}
/// what each flat mirror in view shows: the same surfaces, from the camera mirrored in it, what lies behind the glass cut away, lit from the cache
fn mirrors(f: &Frame, r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, gl: &Globals, vp: Mat4, eye: Vec3, fr: &[Vec4; 6]) {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let (rw, rh) = (f.res.0 as f32, f.res.1 as f32);
    for (n, d, lo, hi) in mirror_panes(s, eye) {
        if !visible(fr, lo, hi) { continue }
        let (mut a, mut b) = (Vec2::splat(1.0), Vec2::splat(-1.0)); let mut behind = false;
        for k in 0..8 {
            let c = vp * Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z }).extend(1.0);
            if c.w <= 0.01 { behind = true; continue; }
            let v = Vec2::new(c.x, c.y) / c.w; a = a.min(v); b = b.max(v);
        }
        if behind { a = Vec2::splat(-1.0); b = Vec2::splat(1.0); }
        let x0 = (((a.x + 1.0) * 0.5 * rw).floor() - 2.0).clamp(0.0, rw); let x1 = (((b.x + 1.0) * 0.5 * rw).ceil() + 2.0).clamp(0.0, rw);
        let y0 = (((1.0 - b.y) * 0.5 * rh).floor() - 2.0).clamp(0.0, rh); let y1 = (((1.0 - a.y) * 0.5 * rh).ceil() + 2.0).clamp(0.0, rh);
        if x1 <= x0 || y1 <= y0 { continue; }
        let refl = Mat4::from_cols((Vec3::X - 2.0 * n.x * n).extend(0.0), (Vec3::Y - 2.0 * n.y * n).extend(0.0), (Vec3::Z - 2.0 * n.z * n).extend(0.0), (2.0 * d * n).extend(1.0));
        let vpm = vp * refl; let eye_m = refl.transform_point3(eye);
        let mut gm = *gl; gm.viewproj = vpm.to_cols_array_2d(); gm.inv_viewproj = vpm.inverse().to_cols_array_2d(); gm.sun_vp = gm.viewproj; gm.cam_pos = eye_m.extend(1.0).into(); gm.clip = [n.x, n.y, n.z, d + 0.002]; gm.clip2 = [0.0; 4];
        q.write_buffer(&f.ub_gm, 0, bytemuck::bytes_of(&gm));
        let frm = frustum(vpm);
        let list: Vec<u32> = f.order.iter().copied().filter(|&i| { let d = &s.draws[i as usize]; visible(&frm, d.bmin, d.bmax) }).collect();
        let (sx, sy, sw, sh) = (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        let black = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } });
        let mut enc = dev.create_command_encoder(&Default::default());
        {
            // (what lies behind the mirror is cut in the fragment shader: the prepass runs one for all)
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None, color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.mirror.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
            rp.set_scissor_rect(sx, sy, sw, sh);
            rp.set_bind_group(0, &f.g_bgm, &[]);
            rp.set_pipeline(&pp.zmask);
            for &i in &list { let d = &s.draws[i as usize];
                rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
                match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
                match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
                rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
                let isz = if d.u16 { 2 } else { 4 };
                rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
                rp.draw_indexed(0..d.icount, 0, 0..1);
            }
        }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                color_attachments: &[att(&f.mirror.hdr), att(&f.mirror.nrm), att(&f.mirror.alb), att(&f.mirror.mat)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.mirror.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }) });
            rp.set_scissor_rect(sx, sy, sw, sh);
            rp.set_bind_group(0, &f.g_bgm, &[]);
            rp.set_pipeline(&r.main_pipe);
            draw_fast(&mut rp, s, &r.main_pipe, &list);
        }
        q.write_buffer(&f.ub_pm, 0, bytemuck::bytes_of(&params(s, vpm.inverse(), eye_m, f.res.0, f.res.1, std::env::var("BAKE_MSHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(0))));
        let mut bp = bk.base; bp.frame = [f.n, 0, 0, 2]; bp.run = [sx, sy, 0, 0]; bp.fl[3] = 1.0;
        q.write_buffer(&f.ub_bm, 0, bytemuck::bytes_of(&bp));
        // (rays for what is glossy in it too, at half its size: a metal door otherwise comes out in the room's whole light, and it may face nothing bright)
        let (mw, mh) = ((f.res.0 / 2).div_ceil(8), (f.res.1 / 2).div_ceil(8));
        dispatch(&mut enc, &pp.spec.0, &f.g0m, &f.b_sm, mw, mh);
        dispatch(&mut enc, &pp.specf.0, &f.g0m, &f.b_fm, mw, mh);
        dispatch(&mut enc, &pp.compose.0, &f.g0m, &f.b_cm, sw.div_ceil(8), sh.div_ceil(8));
        enc.copy_texture_to_texture(wgpu::TexelCopyTextureInfo { texture: &f.m_tmp.0, mip_level: 0, origin: wgpu::Origin3d { x: sx, y: sy, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyTextureInfo { texture: &f.m_lit.0, mip_level: 0, origin: wgpu::Origin3d { x: sx, y: sy, z: 0 }, aspect: wgpu::TextureAspect::All }, wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 });
        q.submit([enc.finish()]);
    }
}
/// everything up to the picture of this frame in linear light; alpha 1 starts the history over
fn surfaces_and_light(f: &mut Frame, r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, o: &Opts, cam: &Cam, xp: f32, jitter: bool) -> (Mat4, Vec3, wgpu::CommandEncoder) {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let (w, h) = (f.gb.w, f.gb.h);
    // (a video at a quarter: flicker on edges goes with the jitter's size, 17 levels a frame at full and about 1 at none. A still averages its frames, so it takes it all)
    let jsc = std::env::var("BAKE_JIT").ok().and_then(|v| v.parse().ok()).unwrap_or(if o.bake.as_deref().map_or(false, |m| m.starts_with("video")) { 0.25f32 } else { 1.0 });
    let jit = if jitter { let k = f.n % 16 + 1; (jsc * (halton(k, 2) - 0.5) * 2.0 / w as f32, jsc * (halton(k, 3) - 0.5) * 2.0 / h as f32) } else { (0.0, 0.0) };
    let (mut gl, vp, eye) = globals(s, cam, o, res_aspect(f), jit);
    gl.sun_vp = gl.viewproj;
    q.write_buffer(&f.ub_g, 0, bytemuck::bytes_of(&gl));
    let fr = frustum(vp);
    for (parts, boxes) in &f.doors {
        // (leaves never go, in stills or video: one vanishing as the camera neared it was plain to see. BAKE_DOORS=hide brings the old rule back for a path that must pass through one)
        let hide = std::env::var("BAKE_DOORS").as_deref() == Ok("hide") && boxes.iter().any(|(lo, hi)| { let d = (eye.clamp(*lo, *hi) - eye).truncate().length(); let w = (*hi - *lo).truncate().max_element(); d < 1.5 || (d < 2.6 && w / d > 0.33) });
        for &i in parts { f.hidden[i as usize] = hide; }
    }
    mirrors(f, r, s, pp, bk, &gl, vp, eye, &fr);
    let list: Vec<u32> = f.order.iter().copied().filter(|&i| { let d = &s.draws[i as usize]; !f.hidden[i as usize] && visible(&fr, d.bmin, d.bmax) }).collect();
    let black = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(black), store: wgpu::StoreOp::Store } });
    let mut enc = dev.create_command_encoder(&Default::default());
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None, color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &f.g_bg, &[]);
        z_pass(&mut rp, s, pp, &list);
    }
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[att(&f.gb.hdr), att(&f.gb.nrm), att(&f.gb.alb), att(&f.gb.mat)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &f.g_bg, &[]);
        rp.set_pipeline(&r.main_pipe);
        draw_fast(&mut rp, s, &r.main_pipe, &list);
    }
    let pr = params(s, vp.inverse(), eye, w, h, f.show);
    q.write_buffer(&f.ub_p, 0, bytemuck::bytes_of(&pr));
    let mut bp = bk.base; bp.fl[1] = 3.0 * xp; bp.prev_vp = f.prev_vp.to_cols_array_2d(); bp.prev_eye = f.prev_eye.extend(1.0).into();
    bp.frame = [f.n, f.n & 1, (f.n >> 1) & 1, 1];
    bp.fl[3] = 1.0; q.write_buffer(&f.ub_b, 0, bytemuck::bytes_of(&bp));
    bp.fl[3] = 2.0; q.write_buffer(&f.ub_b2, 0, bytemuck::bytes_of(&bp));
    let (wx, wy) = (w.div_ceil(8), h.div_ceil(8)); let (wx2, wy2) = ((w / 2).div_ceil(8), (h / 2).div_ceil(8));
    dispatch(&mut enc, &pp.spec.0, &f.g0, &f.b_spec, wx2, wy2);
    dispatch(&mut enc, &pp.specf.0, &f.g0, &f.b_f[0], wx2, wy2);
    dispatch(&mut enc, &pp.specf.0, &f.g0, &f.b_f[1], wx2, wy2);
    dispatch(&mut enc, &pp.compose.0, &f.g0, &f.b_compose, wx, wy);
    // the glass, forward, over the picture
    {
        let att = |v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            color_attachments: &[att(&f.lit), att(&f.gb.nrm), att(&f.gb.alb), att(&f.gb.mat)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.gb.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }) });
        rp.set_bind_group(0, &f.g_bg, &[]);
        rp.set_pipeline(&r.glass_pipe);
        for (i, d) in s.draws.iter().enumerate() {
            if !d.glass || f.hidden[i] || !visible(&fr, d.bmin, d.bmax) { continue }
            rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
            match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
            match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
            rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
            let isz = if d.u16 { 2 } else { 4 };
            rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
            rp.draw_indexed(0..d.icount, 0, 0..1);
        }
    }
    // (what comes next reprojects into the picture as it would be without this frame's jitter: with it, the history wobbled by half a pixel)
    let vp0 = globals(s, cam, o, res_aspect(f), (0.0, 0.0)).1;
    (vp0, eye, enc)
}
fn res_aspect(f: &Frame) -> f32 { f.gb.w as f32 / f.gb.h as f32 }
/// the shown picture from the linear one. alpha: the share of this frame in the history; clamp: motion
fn post(f: &mut Frame, r: &Renderer, pp: &Pipes, bk: &Baked, enc: &mut wgpu::CommandEncoder, xp: f32, alpha: f32, clamp: bool, vp_eye: (Mat4, Vec3)) {
    let q = &r.gpu.q;
    let (w, h) = (f.gb.w, f.gb.h); let (wx, wy) = (w.div_ceil(8), h.div_ceil(8));
    let mut bp = bk.base; bp.fl[0] = alpha; bp.fl[1] = 3.0 * xp; bp.prev_vp = f.prev_vp.to_cols_array_2d(); bp.prev_eye = f.prev_eye.extend(1.0).into(); bp.frame = [f.n, f.n & 1, (f.n >> 1) & 1, clamp as u32]; bp.fl[3] = 1.0;
    q.write_buffer(&f.ub_b, 0, bytemuck::bytes_of(&bp));
    dispatch(enc, &pp.tone.0, &f.g0, &f.b_tone, wx, wy);
    dispatch(enc, &pp.taa.0, &f.g0, &f.b_taa[f.cur], wx, wy);
    dispatch(enc, &pp.fin.0, &f.g0, &f.b_fin[f.cur], f.res.0.div_ceil(8), f.res.1.div_ceil(8));
    f.prev_vp = vp_eye.0; f.prev_eye = vp_eye.1;
}
async fn read_img(f: &Frame, r: &Renderer, mut enc: wgpu::CommandEncoder) -> image::RgbaImage {
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &f.out.0, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &f.rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(f.bpr), rows_per_image: Some(f.res.1) } }, wgpu::Extent3d { width: f.res.0, height: f.res.1, depth_or_array_layers: 1 });
    q.submit([enc.finish()]);
    let sl = f.rb.slice(..); map_read(dev, &[f.rb.slice(..)]).await;
    let data = sl.get_mapped_range().unwrap();
    let mut img = image::RgbaImage::new(f.res.0, f.res.1);
    for y in 0..f.res.1 as usize { let row = &data[y * f.bpr as usize..y * f.bpr as usize + f.res.0 as usize * 4]; img.as_mut()[y * f.res.0 as usize * 4..(y + 1) * f.res.0 as usize * 4].copy_from_slice(row); }
    drop(data); f.rb.unmap();
    img
}
/// Blender's rule, as `exposed`: a mean of 0.45 over the middle 90 percent, a guard on near-clipped pixels
pub async fn expose(f: &mut Frame, r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, o: &Opts, cam: &Cam) -> f32 {
    let (vp, eye, mut enc) = surfaces_and_light(f, r, s, pp, bk, o, cam, 1.0, false);
    post(f, r, pp, bk, &mut enc, 1.0, 1.0, false, (vp, eye));
    let mut img = read_img(f, r, enc).await;
    let stat = |img: &image::RgbaImage| { let mut lum: Vec<f32> = img.pixels().map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0).collect();
        lum.sort_by(|a, b| a.partial_cmp(b).unwrap()); let core = &lum[lum.len() / 20..lum.len() * 19 / 20];
        (core.iter().sum::<f32>() / core.len().max(1) as f32, lum.iter().filter(|v| **v > 0.95).count() as f32 / lum.len().max(1) as f32) };
    let target = std::env::var("RT_MEAN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.45f32);
    let (mut lo, mut hi, mut xp) = (0.02f32, 64f32, 1.0f32);
    for _ in 0..if std::env::var_os("RT_FIXEDXP").is_some() { 0 } else { 7 } {
        let (avg, clip) = stat(&img);
        if (avg - target).abs() < 0.008 && clip < 0.18 { break }
        if avg > target || clip >= 0.18 { hi = xp } else { lo = xp }
        xp = (lo * hi).sqrt();
        let mut enc = r.gpu.dev.create_command_encoder(&Default::default());
        post(f, r, pp, bk, &mut enc, xp, 1.0, false, (vp, eye));
        img = read_img(f, r, enc).await;
    }
    xp
}
/// a still: several frames of the same camera, jittered, averaged
fn still(r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, o: &Opts, cam: &Cam, frames: u32) -> image::RgbaImage {
    let scale = std::env::var("BAKE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.5f32);
    let mut f = make_frame(r, s, pp, bk, o.res, scale);
    let xp = pollster::block_on(expose(&mut f, r, s, pp, bk, o, cam));
    if o.verbose { eprintln!("  exposure x{xp:.2}"); }
    let mut img = None;
    for k in 0..frames {
        f.n = k;
        let t = Instant::now();
        let (vp, eye, mut enc) = surfaces_and_light(&mut f, r, s, pp, bk, o, cam, xp, true);
        f.cur = 1 - f.cur;
        post(&mut f, r, pp, bk, &mut enc, xp, 1.0 / (k as f32 + 1.0), false, (vp, eye));
        if k + 1 == frames { img = Some(pollster::block_on(read_img(&f, r, enc))); } else { r.gpu.q.submit([enc.finish()]); wait(&r.gpu.dev); }
        if o.verbose && k < 4 { eprintln!("  frame {k}: {:.1} ms", t.elapsed().as_secs_f32() * 1000.0); }
    }
    img.unwrap()
}
pub fn run(r: &mut Renderer, s: &mut Scene, o: &Opts, cams: &[Cam]) {
    let spec = o.bake.clone().unwrap(); let (mode, secs) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
    if mode == "measure" { return measure(r, s, o, cams); }
    let t0 = Instant::now();
    ensure_rt(r, s, o);
    if o.verbose { eprintln!("  scene for rays {:.2}s", t0.elapsed().as_secs_f32()); }
    let pp = make_pipes(r);
    if o.verbose { eprintln!("  pipelines {:.2}s", t0.elapsed().as_secs_f32()); }
    let stop = match Stop::parse(secs) { Stop::Time(b) => Stop::Time(b - t0.elapsed().as_secs_f32()), q => q };
    let bk = pollster::block_on(bake(r, s, &pp, stop, o.verbose));
    eprintln!("baked in {:.2}s total: {} cells, {} slots, {} rounds", t0.elapsed().as_secs_f32(), bk.ncells, bk.nslots, bk.rounds);
    let _ = bk.seconds;
    let frames = std::env::var("BAKE_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(16u32);
    match mode {
        "still" => {
            for (i, c) in cams.iter().enumerate() {
                let img = still(r, s, &pp, &bk, o, c, frames);
                let is_png = o.out.extension().map_or(false, |e| e == "png");
                let p = if is_png { o.out.clone() } else { std::fs::create_dir_all(&o.out).ok(); o.out.join(format!("{}_{}.png", s.name, if c.name.is_empty() { format!("cam{i}") } else { c.name.clone() })) };
                save_png(&img, &p); println!("{}", p.display());
            }
        }
        "video" => {
            let frames = std::env::var("BAKE_LEN").ok().and_then(|v| v.parse().ok()).unwrap_or(300usize);
            let path: Vec<(Vec3, Vec3)> = match std::env::var("BAKE_PATH").as_deref() {
                // (a file: [[ex,ey,ez,tx,ty,tz], ...], one a frame)
                Ok(p) if p != "auto" => { let v: Value = serde_json::from_slice(&std::fs::read(p).expect("path file")).expect("path json");
                    v.as_array().unwrap().iter().map(|q| { let f = |i: usize| q[i].as_f64().unwrap() as f32; (Vec3::new(f(0), f(1), f(2)), Vec3::new(f(3), f(4), f(5))) }).collect() }
                _ => auto_path(s, frames),
            };
            assert!(!path.is_empty(), "no path through this house");
            if let Ok(d) = std::env::var("BAKE_SAVE_PATH") { let j: Vec<Vec<f32>> = path.iter().map(|(a, b)| vec![a.x, a.y, a.z, b.x, b.y, b.z]).collect(); std::fs::write(d, serde_json::to_string(&j).unwrap()).ok(); }
            video(r, s, &pp, &bk, o, &path, t0);
        }
        _ => panic!("--bake measure|still|video[:seconds]"),
    }
}

// ---------------- a path through the house, and a video ----------------
/// a walk through the rooms, on the floor plan: from room to room by the shortest way that keeps clear of things, smoothed. (eye, target) a frame
pub fn auto_path(s: &Scene, frames: usize) -> Vec<(Vec3, Vec3)> {
    let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    let mut floors: Vec<(String, Vec3)> = vec![];
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if !nm.starts_with("outdoor") && !nm.starts_with("exterior") { lo = lo.min(d.bmin); hi = hi.max(d.bmax); } if nm.ends_with(".floor") { floors.push((nm.clone(), (d.bmin + d.bmax) * 0.5)); } }
    const C: f32 = 0.1;
    let (nx, ny) = (((hi.x - lo.x) / C) as usize + 2, ((hi.y - lo.y) / C) as usize + 2);
    let floor_z = floors.iter().map(|f| f.1.z).fold(f32::MAX, f32::min);
    let mut solid = vec![false; nx * ny];
    for d in &s.draws {
        let nm = s.glb.nodes[d.node].name.to_lowercase();
        if nm.ends_with(".floor") || nm.contains("ceiling") || nm.starts_with("outdoor") || nm.starts_with("exterior") { continue }
        // (door leaves are solid: the path goes only through doors that stand open. It used to walk through shut ones, and the frame filled with door)
        if d.bmin.z > floor_z + 2.1 || d.bmax.z < floor_z + 0.15 { continue }
        let pb = &s.glb.chunks[d.pos.0][d.pos.1 as usize..(d.pos.1 + d.pos.2) as usize];
        let ib = &s.glb.chunks[d.idx.0][d.idx.1 as usize..(d.idx.1 + d.idx.2) as usize];
        let p = |k: usize| -> Vec3 { let f = |c: usize| f32::from_le_bytes(pb[(k * 3 + c) * 4..(k * 3 + c) * 4 + 4].try_into().unwrap()); Vec3::new(f(0), f(1), f(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t in 0..d.icount as usize / 3 {
            let (a, b, c) = (p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2)));
            if a.z.max(b.z).max(c.z) < floor_z + 0.15 || a.z.min(b.z).min(c.z) > floor_z + 2.1 { continue }
            let l = (b - a).length().max((c - a).length()).max((c - b).length());
            let n = ((l / (C * 0.7)).ceil() as usize).clamp(1, 400);
            for i in 0..=n { for j in 0..=(n - i) {
                let q = a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32);
                if q.z < floor_z + 0.15 || q.z > floor_z + 2.1 { continue }
                let (x, y) = (((q.x - lo.x) / C) as i64, ((q.y - lo.y) / C) as i64);
                if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny { solid[y as usize * nx + x as usize] = true; }
            } }
        }
    }
    // distance to the nearest thing, by sweeps (in cells)
    let mut dist = vec![1e6f32; nx * ny];
    for i in 0..nx * ny { if solid[i] { dist[i] = 0.0; } }
    for _ in 0..2 {
        for y in 0..ny { for x in 0..nx { let i = y * nx + x; if x > 0 { dist[i] = dist[i].min(dist[i - 1] + 1.0); } if y > 0 { dist[i] = dist[i].min(dist[i - nx] + 1.0); } if x > 0 && y > 0 { dist[i] = dist[i].min(dist[i - nx - 1] + 1.41); } if y > 0 && x + 1 < nx { dist[i] = dist[i].min(dist[i - nx + 1] + 1.41); } } }
        for y in (0..ny).rev() { for x in (0..nx).rev() { let i = y * nx + x; if x + 1 < nx { dist[i] = dist[i].min(dist[i + 1] + 1.0); } if y + 1 < ny { dist[i] = dist[i].min(dist[i + nx] + 1.0); } if x + 1 < nx && y + 1 < ny { dist[i] = dist[i].min(dist[i + nx + 1] + 1.41); } if y + 1 < ny && x > 0 { dist[i] = dist[i].min(dist[i + nx - 1] + 1.41); } } }
    }
    let clear = 3.0f32; // cells
    let free = |i: usize| dist[i] >= clear;
    let snap = |p: Vec3| -> Option<usize> { let (cx, cy) = (((p.x - lo.x) / C) as i64, ((p.y - lo.y) / C) as i64); let mut best = None; let mut bd = i64::MAX;
        for y in 0..ny as i64 { for x in 0..nx as i64 { let i = y as usize * nx + x as usize; if free(i) { let d = (x - cx).pow(2) + (y - cy).pow(2); if d < bd { bd = d; best = Some(i); } } } } best };
    let dijkstra = |from: usize| -> (Vec<f32>, Vec<u32>) {
        let mut cost = vec![f32::MAX; nx * ny]; let mut prev = vec![u32::MAX; nx * ny];
        let mut h = std::collections::BinaryHeap::new();
        cost[from] = 0.0; h.push((std::cmp::Reverse(0u32), from as u32));
        while let Some((std::cmp::Reverse(c), i)) = h.pop() {
            let i = i as usize; if c as f32 / 16.0 > cost[i] { continue }
            let (x, y) = ((i % nx) as i64, (i / nx) as i64);
            for (dx, dy, w) in [(1i64, 0i64, 1.0f32), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (1, 1, 1.41), (1, -1, 1.41), (-1, 1, 1.41), (-1, -1, 1.41)] {
                let (a, b) = (x + dx, y + dy); if a < 0 || b < 0 || a >= nx as i64 || b >= ny as i64 { continue }
                let j = b as usize * nx + a as usize; if !free(j) { continue }
                // (the middle of the way: close to things costs more)
                let nc = cost[i] + w * (1.0 + 40.0 / (dist[j] * dist[j]).max(1.0));
                if nc < cost[j] { cost[j] = nc; prev[j] = i as u32; h.push((std::cmp::Reverse((nc * 16.0) as u32), j as u32)); }
            }
        }
        (cost, prev)
    };
    floors.sort_by(|a, b| a.0.cmp(&b.0));
    let mut stops: Vec<usize> = floors.iter().filter_map(|f| snap(f.1)).collect();
    if stops.is_empty() { return vec![] }
    // the rooms, nearest next, from the most connected
    // (the room that reaches the most: with doors solid, a shut bathroom is a place of its own, and a walk begun there saw nothing but tile)
    let first = (0..stops.len()).max_by_key(|&k| { let (c, _) = dijkstra(stops[k]); c.iter().filter(|v| **v < f32::MAX).count() }).unwrap_or(0);
    let mut route: Vec<usize> = vec![]; let mut at = stops.remove(first);
    let want = frames as f32 / 30.0 * 1.0 / C; // cells of walk wanted, at 1 m/s
    let mut guard = 0;
    loop {
        guard += 1;
        let (cost, prev) = dijkstra(at);
        let next = stops.iter().enumerate().filter(|(_, &t)| cost[t] < f32::MAX).min_by(|a, b| cost[*a.1].partial_cmp(&cost[*b.1]).unwrap()).map(|(k, &t)| (k, t));
        let Some((k, t)) = next else { break };
        let mut seg = vec![t]; let mut c = t; while prev[c] != u32::MAX { c = prev[c] as usize; seg.push(c); } seg.reverse();
        route.extend(seg); stops.remove(k); at = t;
        if route.len() as f32 >= want || guard > 64 { break }
    }
    // (one room, or the others out of reach: to its far end from where it stands)
    if route.len() < 30 {
        let (cost, prev) = dijkstra(at);
        let far = (0..nx * ny).filter(|&i| cost[i] < f32::MAX).max_by(|&a, &b| cost[a].partial_cmp(&cost[b]).unwrap());
        if let Some(t) = far { let mut seg = vec![t]; let mut c = t; while prev[c] != u32::MAX { c = prev[c] as usize; seg.push(c); } seg.reverse(); route.extend(seg); }
    }
    if route.len() < 8 { return vec![] }
    let mut pts: Vec<Vec3> = route.iter().map(|&i| Vec3::new(lo.x + ((i % nx) as f32 + 0.5) * C, lo.y + ((i / nx) as f32 + 0.5) * C, floor_z + 1.5)).collect();
    // smooth: and it stays clear, as the steps were small next to the clearance
    for _ in 0..40 { let o = pts.clone(); for i in 1..pts.len() - 1 { pts[i] = (o[i - 1] + o[i] * 2.0 + o[i + 1]) * 0.25; } }
    let mut acc = vec![0f32; pts.len()]; for i in 1..pts.len() { acc[i] = acc[i - 1] + (pts[i] - pts[i - 1]).length(); }
    let total = *acc.last().unwrap();
    let at_s = |t: f32| -> Vec3 { let t = t.clamp(0.0, total); let k = acc.partition_point(|v| *v < t).clamp(1, pts.len() - 1); let f = (t - acc[k - 1]) / (acc[k] - acc[k - 1]).max(1e-5); pts[k - 1].lerp(pts[k], f) };
    let speed = 1.0f32; let mut fwd = (at_s(1.2) - at_s(0.0)).normalize_or_zero();
    (0..frames).map(|f| {
        // (back along the way when it runs out, rather than standing at its end staring at a wall)
        let t0 = (f as f32 / 30.0 * speed) % (2.0 * total.max(0.1)); let t = if t0 > total { 2.0 * total - t0 } else { t0 };
        let e = at_s(t);
        let aim = if t0 > total { at_s(t - 1.2) - e } else { at_s(t + 1.2) - e }; if aim.length() > 0.05 { fwd = fwd.lerp(aim.normalize(), 0.06).normalize(); }
        // (it looks about, gently, and a little down)
        let yaw = 0.2 * (f as f32 / 30.0 * 0.5).sin();
        let lookdir = Mat4::from_rotation_z(yaw).transform_vector3(fwd) + Vec3::new(0.0, 0.0, -0.1);
        (e, e + lookdir)
    }).collect()
}
fn pct(v: &[f32], p: f32) -> f32 { let mut s = v.to_vec(); s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[((s.len() - 1) as f32 * p) as usize] }
fn video(r: &Renderer, s: &Scene, pp: &Pipes, bk: &Baked, o: &Opts, path: &[(Vec3, Vec3)], t_start: Instant) {
    use std::sync::{Arc, atomic::AtomicU32};
    let (dev, q) = (&r.gpu.dev, &r.gpu.q);
    let scale = std::env::var("BAKE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.5f32);
    let mut f = make_frame(r, s, pp, bk, o.res, scale);
    let lens = std::env::var("BAKE_LENS").ok().and_then(|v| v.parse().ok()).unwrap_or(18.0f32);
    let cam_at = |k: usize| cam_look(path[k].0, path[k].1, lens);
    // exposure: set here, for places along the path, and eased between them
    let te = Instant::now();
    let step = 15usize;
    let keys: Vec<f32> = (0..path.len()).step_by(step).map(|k| pollster::block_on(expose(&mut f, r, s, pp, bk, o, &cam_at(k))).max(1e-3).ln()).collect();
    let keyd: Vec<f32> = (0..keys.len()).map(|i| { let (mut a, mut w) = (0.0, 0.0); for j in 0..keys.len() { let d = (i as f32 - j as f32) / 3.0; let g = (-d * d).exp(); a += keys[j] * g; w += g; } a / w }).collect();
    let xp_at = |k: usize| { let u = k as f32 / step as f32; let i = (u as usize).min(keyd.len() - 1); let j = (i + 1).min(keyd.len() - 1); (keyd[i] + (keyd[j] - keyd[i]) * (u - i as f32)).exp() };
    eprintln!("exposure keys {}, {:.2}s; since the start of the bake {:.2}s", keys.len(), te.elapsed().as_secs_f32(), t_start.elapsed().as_secs_f32());
    // the encoder takes what the GPU has done, on threads of its own
    let (w, h) = o.res; let is_vid = o.out.extension().map_or(false, |e| e != "png") && o.out.as_os_str() != "/dev/null";
    let codec = std::env::var("BAKE_CODEC").unwrap_or("h264_videotoolbox".into());
    let mut child = if is_vid {
        let mut a: Vec<String> = ["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"].iter().map(|s| s.to_string()).collect();
        a.push(format!("{w}x{h}")); for x in ["-r", "30", "-i", "-", "-c:v"] { a.push(x.into()); } a.push(codec.clone());
        if codec == "h264_videotoolbox" { for x in ["-b:v", "16M", "-realtime", "1"] { a.push(x.into()); } } else { for x in ["-preset", "veryfast", "-crf", "18"] { a.push(x.into()); } }
        for x in ["-pix_fmt", "yuv420p"] { a.push(x.into()); } a.push(o.out.display().to_string());
        Some(std::process::Command::new("ffmpeg").args(&a).stdin(std::process::Stdio::piped()).spawn().expect("ffmpeg"))
    } else { None };
    let stdin = child.as_mut().and_then(|c| c.stdin.take());
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
    let (back_tx, back_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let writer = std::thread::spawn(move || { use std::io::Write; let mut out = stdin; for b in rx { if let Some(p) = out.as_mut() { p.write_all(&b).ok(); } back_tx.send(b).ok(); } drop(out); });
    let dump = std::env::var("BAKE_DUMP").ok().map(PathBuf::from);
    if let Some(d) = &dump { std::fs::create_dir_all(d).ok(); }
    let alpha = std::env::var("BAKE_TAA").ok().and_then(|v| v.parse().ok()).unwrap_or(0.12f32);
    let ring: Vec<wgpu::Buffer> = (0..5).map(|_| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (f.bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false })).collect();
    let flags: Vec<Arc<AtomicU32>> = ring.iter().map(|_| Arc::new(AtomicU32::new(0))).collect();
    let mut queue: std::collections::VecDeque<(usize, usize)> = Default::default();
    let (mut times, mut enc_ms): (Vec<f32>, Vec<f32>) = (vec![], vec![]);
    let bpr = f.bpr as usize; let row = w as usize * 4;
    let mut pool: Vec<Vec<u8>> = vec![];
    let mut take = |slot: usize, k: usize, ring: &Vec<wgpu::Buffer>, pool: &mut Vec<Vec<u8>>| {
        let data = ring[slot].slice(..).get_mapped_range().unwrap();
        let mut b = pool.pop().unwrap_or_else(|| vec![0u8; row * h as usize]);
        if bpr == row { b.copy_from_slice(&data[..row * h as usize]); } else { for y in 0..h as usize { b[y * row..(y + 1) * row].copy_from_slice(&data[y * bpr..y * bpr + row]); } }
        drop(data); ring[slot].unmap();
        if let Some(d) = &dump { let im = image::RgbaImage::from_raw(w, h, b.clone()).unwrap(); save_png(&im, &d.join(format!("{k:05}.png"))); }
        tx.send(b).ok();
    };
    let t0 = Instant::now(); let mut last = t0;
    for k in 0..path.len() {
        let cam = cam_at(k); let xp = xp_at(k);
        f.n = k as u32;
        let t_enc = Instant::now();
        let (vp, eye, mut enc) = surfaces_and_light(&mut f, r, s, pp, bk, o, &cam, xp, true);
        f.cur = 1 - f.cur;
        post(&mut f, r, pp, bk, &mut enc, xp, if k == 0 { 1.0 } else { alpha }, true, (vp, eye));
        // the next free buffer of the ring
        let slot = k % ring.len();
        while queue.iter().any(|&(sl, _)| sl == slot) {
            while let Some(&(sl, kk)) = queue.front() {
                if flags[sl].load(Ordering::Acquire) == 1 { queue.pop_front(); take(sl, kk, &ring, &mut pool); while let Ok(b) = back_rx.try_recv() { pool.push(b); } } else { break }
            }
            if queue.iter().any(|&(sl, _)| sl == slot) { dev.poll(wgpu::PollType::Poll).ok(); std::thread::yield_now(); }
        }
        enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &f.out.0, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &ring[slot], layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(f.bpr), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        q.submit([enc.finish()]);
        flags[slot].store(0, Ordering::Release);
        let fl = flags[slot].clone();
        ring[slot].slice(..).map_async(wgpu::MapMode::Read, move |_| fl.store(1, Ordering::Release));
        queue.push_back((slot, k));
        enc_ms.push(t_enc.elapsed().as_secs_f32() * 1000.0);
        // (a frame's end is when the GPU has begun it: never run far ahead of it)
        while queue.len() >= 3 {
            dev.poll(wgpu::PollType::Poll).ok();
            let Some(&(sl, kk)) = queue.front() else { break };
            if flags[sl].load(Ordering::Acquire) == 1 { queue.pop_front(); take(sl, kk, &ring, &mut pool); while let Ok(b) = back_rx.try_recv() { pool.push(b); } } else { std::thread::yield_now(); }
        }
        let now = Instant::now(); times.push((now - last).as_secs_f32() * 1000.0); last = now;
    }
    while let Some((sl, kk)) = queue.pop_front() {
        while flags[sl].load(Ordering::Acquire) != 1 { dev.poll(wgpu::PollType::Poll).ok(); std::thread::yield_now(); }
        take(sl, kk, &ring, &mut pool);
    }
    drop(take); drop(tx);
    writer.join().ok();
    if let Some(mut c) = child { c.wait().ok(); }
    let total = t0.elapsed().as_secs_f32(); let n = path.len();
    // the slowest second
    let (mut worst, mut sum, mut a) = (0f32, 0f32, 0usize);
    for b in 0..times.len() { sum += times[b]; while b - a >= 30 { sum -= times[a]; a += 1; } if b >= 29 { worst = worst.max(sum); } }
    println!("FRAMES {n} in {total:.2}s (written and encoded): {:.1} fps; frame ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1}; slowest 30 frames {:.1} fps; encode-side CPU p50 {:.1} ms; codec {}", n as f32 / total, pct(&times, 0.5), pct(&times, 0.95), pct(&times, 0.99), pct(&times, 1.0), 30000.0 / worst.max(1.0), pct(&enc_ms, 0.5), if is_vid { codec } else { "none".into() });
}

/// A bake kept for drawing live in a page, frame by frame, as the camera goes
pub struct Live { pp: Pipes, bk: Baked, f: Frame, pub xp: f32, n: u32 }
impl Live { pub fn stats(&self) -> String { format!("{} cells, {} slots, {} rounds, {:.1} s bake, exposure x{:.2}", self.bk.ncells, self.bk.nslots, self.bk.rounds, self.bk.seconds, self.xp) } }
pub async fn live_start(r: &mut Renderer, s: &mut Scene, o: &Opts, stop: Stop, cam: &Cam) -> Live {
    ensure_rt_async(r, s, o).await;
    let pp = make_pipes(r);
    let bk = bake(r, s, &pp, stop, false).await;
    let mut f = make_frame(r, s, &pp, &bk, o.res, 1.5);
    let xp = expose(&mut f, r, s, &pp, &bk, o, cam).await;
    Live { pp, bk, f, xp, n: 0 }
}
pub fn live_draw(l: &mut Live, r: &Renderer, s: &Scene, o: &Opts, cam: &Cam, target: &wgpu::Texture) {
    l.f.n = l.n;
    let (vp, eye, mut enc) = surfaces_and_light(&mut l.f, r, s, &l.pp, &l.bk, o, cam, l.xp, true);
    l.f.cur = 1 - l.f.cur;
    post(&mut l.f, r, &l.pp, &l.bk, &mut enc, l.xp, if l.n == 0 { 1.0 } else { 0.12 }, true, (vp, eye));
    enc.copy_texture_to_texture(l.f.out.0.as_image_copy(), target.as_image_copy(), wgpu::Extent3d { width: o.res.0, height: o.res.1, depth_or_array_layers: 1 });
    r.gpu.q.submit([enc.finish()]);
    l.n += 1;
}
pub async fn live_expose(l: &mut Live, r: &Renderer, s: &Scene, o: &Opts, cam: &Cam) {
    l.xp = expose(&mut l.f, r, s, &l.pp, &l.bk, o, cam).await;
    l.n = 0;
}
