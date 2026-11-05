//! The depth prepass and drawing in material order, for `--rt video`: the surface shader then runs only for what shows. The one light path
//! is rt::encode, as for a still; this is only how its G-buffer is drawn.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// set for `--rt video`: the surface shader then leaves light to rt.rs, and draws with a depth that is already there
pub static ON: AtomicBool = AtomicBool::new(false);
pub fn on() -> bool { ON.load(Ordering::Relaxed) }
/// the surface shader as video draws it: light is done by rt.rs, so fs_main only fills the G-buffer. And a fragment shader for the depth prepass of what its texture cuts out
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

/// the opaque draws in a frustum, without draw_all's per-draw lookups (and in material order, if the list is)
pub fn draw_list<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, base: &'a wgpu::RenderPipeline, list: &[u32]) -> u64 {
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
        rp.draw_indexed(0..d.icount, 0, 0..1);
        tris += d.icount as u64 / 3;
    }
    tris
}

/// the depth prepass: everything once, the surface shader then only for what shows
pub fn z_pipes(r: &Renderer) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let dev = &r.gpu.dev;
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
    (z, zmask)
}
pub fn z_draw<'a>(rp: &mut wgpu::RenderPass<'a>, s: &'a Scene, z: &'a wgpu::RenderPipeline, zmask: &'a wgpu::RenderPipeline, list: &[u32]) {
    rp.set_pipeline(z);
    for &i in list.iter().filter(|&&i| !s.draws[i as usize].mask) { let d = &s.draws[i as usize];
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, 0..1);
    }
    let mut set = false;
    for &i in list.iter().filter(|&&i| s.draws[i as usize].mask) { let d = &s.draws[i as usize];
        if !set { rp.set_pipeline(zmask); set = true; }
        rp.set_vertex_buffer(0, s.bufs[d.pos.0].slice(d.pos.1..d.pos.1 + d.vcount as u64 * 12));
        match d.nrm { Some(n) => rp.set_vertex_buffer(1, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 12)), None => rp.set_vertex_buffer(1, s.zeros.slice(0..d.vcount as u64 * 12)) }
        match d.uv { Some(n) => rp.set_vertex_buffer(2, s.bufs[n.0].slice(n.1..n.1 + d.vcount as u64 * 8)), None => rp.set_vertex_buffer(2, s.zeros.slice(0..d.vcount as u64 * 8)) }
        rp.set_bind_group(1, &s.mat_bgs[d.mat_bg], &[]);
        let isz = if d.u16 { 2 } else { 4 };
        rp.set_index_buffer(s.bufs[d.idx.0].slice(d.idx.1..d.idx.1 + d.icount as u64 * isz), if d.u16 { wgpu::IndexFormat::Uint16 } else { wgpu::IndexFormat::Uint32 });
        rp.draw_indexed(0..d.icount, 0, 0..1);
    }
}
