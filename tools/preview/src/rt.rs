//! rt: hybrid path tracing for the previewer (--rt fast|full). The rasterizer draws primary visibility and materials into a G-buffer; a compute shader traces the BVH
//! of the scene for bounce light, soft sun, lamps and reflections; an edge-aware filter stands in for Blender's denoiser. The light is Blender's: the same seeded sky
//! (render_rust.py), the same units, AgX with Medium High Contrast and the same auto-exposure rule.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;


#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quality { pub spp: u32, pub bounces: u32, pub spec_spp: u32, pub denoise: u32, pub ss: u32, pub video: bool, pub hist: bool, /** a mirror's picture: none of the camera cells (see rt.wgsl lcv_v) */ pub no_cells: bool }
impl Quality {
    pub fn parse(s: &str) -> Quality {
        match s {
            "fast" => Quality { spp: 1, bounces: 2, spec_spp: 2, denoise: 5, ss: 2, video: false, hist: false, no_cells: false },
            // (frames of a moving camera, at 62 ms each: no learned filter, no finer pixels, and the cache reaches further)
            "video" => Quality { spp: 1, bounces: 4, spec_spp: 1, denoise: 5, ss: 2, video: true, hist: true, no_cells: false },
            "full" => Quality { spp: 2, bounces: 4, spec_spp: 3, denoise: 5, ss: 3, video: false, hist: false, no_cells: false },
            _ => panic!("--rt fast|full"),
        }
    }
}

/// what a picture traces at and how many surface pixels each takes: a video's later frames trace half as many lines as its first
pub fn trace_size(o: &crate::Opts, _q: Quality) -> ((u32, u32), u32) {
    (o.res, o.ss) // (a video's frames too: their edges and textures from 2x2 surfaces, as a still's. One surface a pixel left them soft, and TAA could not make it up)
}
/// a video's first frame is a fast picture, whose light the later frames build on. They would otherwise start from a few rays, in blotches
pub fn frame_quality(q: Quality, first: bool) -> Quality {
    if q.video && first { Quality { hist: true, ..Quality::parse("fast") } } else { q }
}
fn halton(mut i: u32, b: u32) -> f32 { let (mut f, mut r) = (1.0, 0.0); while i > 0 { f /= b as f32; r += f * (i % b) as f32; i /= b; } r }
/// this frame's sub-pixel shift, in clip space
pub fn jitter(res: (u32, u32)) -> (f32, f32) { let k = this_frame() % 16 + 1; (0.25 * (halton(k, 2) - 0.5) * 2.0 / res.0 as f32, 0.25 * (halton(k, 3) - 0.5) * 2.0 / res.1 as f32) /* (a quarter: edge flicker goes with the shift, and a full pixel's doubled it) */ }
/// a video's picture built up over frames, at the end: edges, by the shifted camera
pub fn taa(dev: &wgpu::Device, enc: &mut wgpu::CommandEncoder, p: &Pipes, sd: &SceneData, lit: &wgpu::TextureView, depth: &wgpu::TextureView, alb: &wgpu::TextureView, vp0: Mat4, eye: Vec3, res: (u32, u32)) {
    use wgpu::BindGroupEntry as B; use wgpu::BindingResource as R;
    let mut st = sd.taa_st.lock().unwrap();
    let i = st.map_or(0, |x| 1 - x.2);
    let (pv, pe, first) = st.map_or((vp0, eye, 1.0f32), |x| (x.0, x.1, 0.0));
    let mut tu = [0f32; 24]; tu[..16].copy_from_slice(&pv.to_cols_array()); tu[16..19].copy_from_slice(&pe.to_array()); tu[20] = first; tu[21] = res.0 as f32; tu[22] = res.1 as f32; tu[23] = 0.25; tu[19] = std::env::var("VID_SHARP").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    let ub = wgpu::util::DeviceExt::create_buffer_init(dev, &wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&tu), usage: wgpu::BufferUsages::UNIFORM });
    let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.taa_bgl, entries: &[B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 8, resource: ub.as_entire_binding() }, B { binding: 2, resource: R::TextureView(depth) }, B { binding: 14, resource: R::TextureView(alb) }, B { binding: 4, resource: R::TextureView(lit) }, B { binding: 5, resource: R::TextureView(&p.taa_h[1 - i]) }, B { binding: 6, resource: R::TextureView(&p.taa_h[i]) }] });
    { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.taa); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups(res.0.div_ceil(8), res.1.div_ceil(8), 1); }
    // (the first frame shows itself: finer surfaces, so its history is them averaged)
    if first < 1.0 { enc.copy_texture_to_texture(p.taa_h[i].texture().as_image_copy(), lit.texture().as_image_copy(), wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 }); }
    *st = Some((vp0, eye, i));
}
static FRAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
/// a video's frames each take fresh rays, or their noise never averages out; a still's are the same each time
pub fn this_frame() -> u32 { FRAME.load(std::sync::atomic::Ordering::Relaxed) }
pub fn next_frame(video: bool) -> u32 { if video { FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed) } else { 1 } }

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
pub struct Params {
    pub inv_vp: [[f32; 4]; 4], pub cam: [f32; 4], pub sun_dir: [f32; 4], pub sun_col: [f32; 4],
    /// w, h (traced), ss, spp
    pub dims: [u32; 4],
    /// bounces, frame, lights, sky width
    pub misc: [u32; 4],
    /// sky height, specular rays, -, -
    pub misc2: [u32; 4],
    /// lamp scale, sky scale, sun scale, sun disc (radians)
    pub scale: [f32; 4],
}
#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct Node { lo: [f32; 3], a: u32, hi: [f32; 3], b: u32 }
#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct Tri { v0: [f32; 3], mat: u32, e1: [f32; 4], e2: [f32; 4] }
#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
pub struct MatD { pub albedo: [f32; 4], pub emissive: [f32; 4] }
#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct LightD { pos: [f32; 4], col: [f32; 4] }

/// Python's random.Random, as far as render_rust.py draws from it (MT19937, init_by_array, gauss pairs)
pub struct PyRandom { mt: [u32; 624], i: usize, gauss_next: Option<f64> }
impl PyRandom {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = 19650218;
        for i in 1..624 { mt[i] = 1812433253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32); }
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..624 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1664525)).wrapping_add(seed).wrapping_add(j as u32);
            i += 1; j = 0;
            if i >= 624 { mt[0] = mt[623]; i = 1; }
        }
        for _ in 0..623 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 { mt[0] = mt[623]; i = 1; }
        }
        mt[0] = 0x8000_0000;
        PyRandom { mt, i: 624, gauss_next: None }
    }
    fn u32(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % 624] & 0x7fff_ffff);
                self.mt[k] = self.mt[(k + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.i = 0;
        }
        let mut y = self.mt[self.i]; self.i += 1;
        y ^= y >> 11; y ^= (y << 7) & 0x9d2c_5680; y ^= (y << 15) & 0xefc6_0000; y ^ (y >> 18)
    }
    pub fn random(&mut self) -> f64 { let (a, b) = ((self.u32() >> 5) as f64, (self.u32() >> 6) as f64); (a * 67108864.0 + b) / 9007199254740992.0 }
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.random() }
    pub fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = match self.gauss_next.take() {
            Some(z) => z,
            None => {
                let x2pi = self.random() * std::f64::consts::TAU;
                let g2rad = (-2.0 * (1.0 - self.random()).ln()).sqrt();
                self.gauss_next = Some(x2pi.sin() * g2rad);
                x2pi.cos() * g2rad
            }
        };
        mu + z * sigma
    }
}

/// render_rust.py's sky for a seed
#[derive(Clone, Copy, Debug)]
pub struct SkyParams { pub sun_size: f32, pub sun_intensity: f32, pub elevation: f32, pub rotation: f32, pub altitude: f32, pub air: f32, pub aerosol: f32, pub ozone: f32, pub strength: f32 }
pub fn sky_params(seed: u64) -> SkyParams {
    let mut h = crc32(format!("{seed}:sky").as_bytes());
    if h == 0 { h = 1; }
    let mut r = PyRandom::new(h);
    let mut clip = |r: &mut PyRandom, mu: f64, sd: f64, lo: f64, hi: f64| r.gauss(mu, sd).max(lo).min(hi);
    let sun_size = clip(&mut r, 0.5, 0.3, 0.25, 5.0).to_radians();
    let sun_intensity = r.uniform(0.8, 1.0);
    let elevation = r.uniform(10f64.to_radians().sin(), 1.0).asin();
    let rotation = r.uniform(0.0, std::f64::consts::TAU);
    let altitude = clip(&mut r, 100.0, 400.0, 0.0, 2000.0);
    let air = clip(&mut r, 1.0, 0.2, 0.7, 1.3);
    let aerosol = clip(&mut r, 1.0, 1.0, 0.1, 2.0);
    let ozone = clip(&mut r, 1.0, 1.0, 0.1, 10.0);
    let strength = r.uniform(0.18, 0.22);
    SkyParams { sun_size: sun_size as f32, sun_intensity: sun_intensity as f32, elevation: elevation as f32, rotation: rotation as f32, altitude: altitude as f32, air: air as f32, aerosol: aerosol as f32, ozone: ozone as f32, strength: strength as f32 }
}
pub fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b { c ^= x as u32; for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 }; } }
    !c
}

/// single scattering: the sky above the horizon as a table, and the sun's light at the ground. The shape is Nishita's; the coefficients are fitted to what Blender 5's SINGLE_SCATTERING
/// sky gives for six seeds (radiance over the hemisphere within about 2 percent, the sun within about 10): ~/work/bench/rt/sky/fit.py
pub fn sky_table(p: &SkyParams, w: usize, h: usize) -> (Vec<[f32; 4]>, Vec3, Vec3) {
    const RE: f32 = 6_360_000.0; const RA: f32 = 6_420_000.0;
    const HR: f32 = 7723.388; const HM: f32 = 1651.504; const G: f32 = 0.763;
    let br = Vec3::new(5.998e-6, 11.991e-6, 27.217e-6) * p.air;
    let bms = 15.071e-6 * p.aerosol;
    let bme = bms * 1.239;
    let oz = Vec3::new(3.198e-6, 1.883e-6, 0.556e-6) * p.ozone;
    let e0 = Vec3::new(239.985, 207.214, 191.486);
    let e_sun = Vec3::new(189.01, 154.708, 130.151);
    let sun = sun_dir(p);
    let o = Vec3::new(0.0, 0.0, RE + p.altitude.max(1.0));
    let sphere = |o: Vec3, d: Vec3, r: f32| -> f32 { let b = o.dot(d); let c = o.dot(o) - r * r; let k = b * b - c; if k > 0.0 { -b + k.sqrt() } else { 0.0 } };
    let optical = |a: Vec3, d: Vec3, len: f32, n: usize| -> Vec3 {
        let (mut r, mut m, mut z) = (0f32, 0f32, 0f32);
        let ds = len / n as f32;
        for i in 0..n {
            let h = (a + d * ((i as f32 + 0.5) * ds)).length() - RE;
            r += (-h / HR).exp() * ds; m += (-h / HM).exp() * ds;
            z += (1.0 - ((h - 25000.0).abs() / 15000.0)).max(0.0) * ds;
        }
        br * r + Vec3::splat(bme * m) + oz * z
    };
    let mut out = vec![[0f32; 4]; w * h];
    for row in 0..h {
        let v = (row as f32 + 0.5) / h as f32;
        // (rows packed toward the horizon, where the sky changes fastest)
        let el = v * v * std::f32::consts::FRAC_PI_2;
        for col in 0..w {
            let az = (col as f32 + 0.5) / w as f32 * std::f32::consts::TAU;
            let d = Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
            let len = sphere(o, d, RA).min(300_000.0);
            let n = 10; let ds = len / n as f32;
            let mu = d.dot(sun);
            let pr = 3.0 / (16.0 * std::f32::consts::PI) * (1.0 + mu * mu);
            let pm = 3.0 / (8.0 * std::f32::consts::PI) * ((1.0 - G * G) * (1.0 + mu * mu)) / ((2.0 + G * G) * (1.0 + G * G - 2.0 * G * mu).max(1e-4).powf(1.5));
            let (mut l, mut acc) = (Vec3::ZERO, Vec3::ZERO);
            for i in 0..n {
                let q = o + d * ((i as f32 + 0.5) * ds);
                let hh = q.length() - RE;
                acc += optical(o + d * (i as f32 * ds), d, ds, 1);
                let t = (-(acc + optical(q, sun, sphere(q, sun, RA), 5))).exp();
                l += t * (br * (-hh / HR).exp() * pr + Vec3::splat(bms * (-hh / HM).exp() * pm)) * ds;
            }
            let c = l * e0;
            out[row * w + col] = [c.x, c.y, c.z, 1.0];
        }
    }
    let t = (-optical(o, sun, sphere(o, sun, RA), 24)).exp();
    (out, sun, e_sun * t * p.sun_intensity)
}
pub fn sun_dir(p: &SkyParams) -> Vec3 {
    let (e, r) = (p.elevation, p.rotation);
    Vec3::new(e.cos() * r.sin(), e.cos() * r.cos(), e.sin())
}

pub struct Pipes { pub sunv: wgpu::TextureView, pub var: [wgpu::TextureView; 2], pub mom: [wgpu::TextureView; 2], pub vis: wgpu::Buffer, pub lc_room: wgpu::ComputePipeline, pub lc_fill: wgpu::ComputePipeline, pub lc_seed: wgpu::ComputePipeline, pub resv_t: [wgpu::Buffer; 2], pub taa: wgpu::ComputePipeline, pub taa_bgl: wgpu::BindGroupLayout, pub taa_h: [wgpu::TextureView; 2], pub temporal: wgpu::ComputePipeline, pub tmp_bgl: wgpu::BindGroupLayout, pub hist: [[wgpu::TextureView; 2]; 4], pub hn: [[wgpu::TextureView; 2]; 4], pub ubuf_pre: wgpu::Buffer, pub lc_resolve: wgpu::ComputePipeline, pub restir: wgpu::ComputePipeline, pub restir_spec: wgpu::ComputePipeline, pub resv: wgpu::Buffer,
    pub fine: wgpu::TextureView, pub glint: wgpu::TextureView, pub lamp: [wgpu::TextureView; 2], pub trace_fine: wgpu::ComputePipeline,
    pub trace: wgpu::ComputePipeline, pub gather_q: wgpu::ComputePipeline, pub trace_spec: wgpu::ComputePipeline, pub denoise: wgpu::ComputePipeline, pub compose: wgpu::ComputePipeline,
    pub trace_bgl: wgpu::BindGroupLayout, pub denoise_bgl: wgpu::BindGroupLayout, pub compose_bgl: wgpu::BindGroupLayout,
    pub ubuf: wgpu::Buffer, pub step_ubufs: Vec<wgpu::Buffer>,
    pub irr: [wgpu::TextureView; 2], pub spec: [wgpu::TextureView; 2],
}

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
pub struct Win { pub c: [f32; 4], pub u: [f32; 4], pub v: [f32; 4], pub n: [f32; 4] }
pub struct SceneData { pub m_idx: wgpu::Buffer, pub m_acc: wgpu::Buffer, pub m_out: wgpu::Buffer, pub lc_guide: wgpu::Buffer, pub pane_st: std::sync::Mutex<Option<(Vec3, Vec3)>>, pub taa_st: std::sync::Mutex<Option<(Mat4, Vec3, usize)>>, pub hist_st: std::sync::Mutex<Option<(Mat4, Vec3, usize, (u32, u32))>>, pub lc_eye: std::sync::Mutex<Vec3>, pub lc_cells: wgpu::Buffer, pub lc_acc: wgpu::Buffer, pub lc_out: wgpu::Buffer, pub lc_n: u32, pub wins: wgpu::Buffer, pub nwins: u32, pub nodes: wgpu::Buffer, pub tris: wgpu::Buffer, pub mats: wgpu::Buffer, pub lights: wgpu::Buffer, pub sky: wgpu::Buffer, pub nlights: u32, pub sky_wh: (u32, u32), pub sun: Vec3, pub sun_e: Vec3, pub skyp: SkyParams }

const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub fn pipes(dev: &wgpu::Device, w: u32, h: u32, ss: u32) -> Pipes {
    // (no bounds checks in the traversal: every index comes from the BVH built here, and they were much of the cost of a ray)
    let module = |src: &str| unsafe { dev.create_shader_module_trusted(wgpu::ShaderModuleDescriptor { label: Some("rt"), source: wgpu::ShaderSource::Wgsl(src.into()) }, wgpu::ShaderRuntimeChecks::unchecked()) };
    use wgpu::{BindGroupLayoutEntry as E, BindingType as T, ShaderStages as S};
    let c = S::COMPUTE;
    let uni = |b| E { binding: b, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
    let sto = |b| E { binding: b, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None };
    let tex = |b| E { binding: b, visibility: c, ty: T::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
    let dep = |b| E { binding: b, visibility: c, ty: T::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
    let out = |b| E { binding: b, visibility: c, ty: T::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: FMT, view_dimension: wgpu::TextureViewDimension::D2 }, count: None };
    let trace_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[out(32), E { binding: 28, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 29, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 30, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 27, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 26, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, tex(22), uni(23), sto(24), E { binding: 25, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, uni(0), sto(1), sto(2), sto(3), sto(4), sto(5), dep(6), tex(7), tex(8), out(9), out(10), sto(11), tex(12), tex(13), out(14), out(15), tex(16), tex(17), E { binding: 18, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 19, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 20, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }, E { binding: 21, visibility: c, ty: T::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
    let denoise_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), uni(1), dep(2), tex(3), tex(4), tex(5), out(6), out(7), tex(13), tex(17), out(18), tex(19)] });
    let compose_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), sto(1), dep(2), tex(3), tex(4), tex(5), tex(6), tex(7), tex(8), out(9), tex(10), tex(11), tex(12), sto(13), tex(14), tex(15), tex(16)] });
    let pipe = |bgl: &wgpu::BindGroupLayout, src: &str, ep: &str| {
        let sm = module(src);
        let l = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(bgl)], immediate_size: 0 });
        dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: Some(&l), module: &sm, entry_point: Some(ep), compilation_options: Default::default(), cache: None })
    };
    let trace_spec = pipe(&trace_bgl, include_str!("rt.wgsl"), "trace_spec");
    let trace_fine = pipe(&trace_bgl, include_str!("rt.wgsl"), "trace_fine");
    let restir = pipe(&trace_bgl, include_str!("rt.wgsl"), "restir");
    let lc_resolve = pipe(&trace_bgl, include_str!("rt.wgsl"), "lc_resolve");
    let restir_spec = pipe(&trace_bgl, include_str!("rt.wgsl"), "restir_spec");
    let resv = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (w as u64 * h as u64 * 96).max(96), usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    let tmp_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), uni(8), dep(2), tex(3), tex(4), tex(5), out(6), tex(9), tex(10), out(11), tex(12), tex(15), out(16)] });
    let temporal = pipe(&tmp_bgl, include_str!("rt_post.wgsl"), "temporal");
    let taa_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), uni(8), dep(2), tex(4), tex(5), out(6), tex(14)] });
    let taa = pipe(&taa_bgl, include_str!("rt_post.wgsl"), "taa");
    let (trace, denoise, compose) = (pipe(&trace_bgl, include_str!("rt.wgsl"), "trace"), pipe(&denoise_bgl, include_str!("rt_post.wgsl"), "denoise"), pipe(&compose_bgl, include_str!("rt_comp.wgsl"), "compose"));
    let ubuf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let ubuf_pre = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let step_ubufs = (0..30).map(|i: u32| dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&[1u32 << (i % 6), i % 6, (i / 6).min(1), if i >= 24 { 1u32 } else if i >= 18 { 0 } else if i >= 12 { 5 } else { 3 }]), usage: wgpu::BufferUsages::UNIFORM })).collect();
    let mk_at = |w: u32, h: u32| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: FMT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST, view_formats: &[] }).create_view(&Default::default());
    let mk = || mk_at(w, h);
    // (what the trace found of the sun and the nearest lamps at each traced pixel: the sun's visibility, then six lamps' as index and share. Compose and the highlights read them)
    let vis = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: (w as u64 * h as u64 * 16).max(16), usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    Pipes { sunv: mk(), var: [mk(), mk()], mom: [mk(), mk()], vis, lc_room: pipe(&trace_bgl, include_str!("rt.wgsl"), "lc_fill_room"), lc_fill: pipe(&trace_bgl, include_str!("rt.wgsl"), "lc_fill_view"), lc_seed: pipe(&trace_bgl, include_str!("rt.wgsl"), "lc_seed"), resv_t: [0, 1].map(|_| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: 96, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false })), taa, taa_bgl, taa_h: [mk_at(w * ss, h * ss), mk_at(w * ss, h * ss)], temporal, tmp_bgl, hist: [[mk(), mk()], [mk(), mk()], [mk(), mk()], [mk(), mk()]], hn: [[mk(), mk()], [mk(), mk()], [mk(), mk()], [mk(), mk()]], ubuf_pre, lc_resolve, restir, restir_spec, resv, lamp: [mk(), mk()], glint: mk(), fine: mk_at(w * ss, h * ss), trace_fine, trace, gather_q: pipe(&trace_bgl, include_str!("rt.wgsl"), "gather_q"), trace_spec, denoise, compose, trace_bgl, denoise_bgl, compose_bgl, ubuf, step_ubufs, irr: [mk(), mk()], spec: [mk(), mk()] }
}

/// triangles with their draw's index
const LC_SLOTS: usize = 1 << 18; // must match LC_MASK in rt.wgsl
const M_MAPS: u64 = 16384; // must match M_MAPS in rt.wgsl
pub fn build_scene(dev: &wgpu::Device, tris_in: Vec<([Vec3; 3], u32)>, mats: &[MatD], lights: &[(Vec3, Vec3)], wins: &[Win], seed: u64, verbose: bool) -> SceneData {
    use obvhs::triangle::Triangle;
    let t0 = web_time::Instant::now();
    let tris_o: Vec<Triangle> = tris_in.iter().map(|(t, _)| Triangle { v0: t[0].into(), v1: t[1].into(), v2: t[2].into() }).collect();
    let mut core = std::time::Duration::ZERO;
    // (the fast build: rays cost the same on these houses, measured on 16 views, and it takes 40% less time)
    let mut params = obvhs::BvhBuildParams::fast_build();
    params.max_prims_per_leaf = 4;
    crate::memlog("bvh start");
    let bvh = obvhs::bvh2::builder::build_bvh2_from_tris(&tris_o, params, &mut core);
    crate::memlog("bvh built");
    let nodes: Vec<Node> = bvh.nodes.iter().map(|n| Node { lo: n.aabb.min.into(), a: n.first_index, hi: n.aabb.max.into(), b: n.prim_count }).collect();
    drop(tris_o); // (the build's copies go as soon as they can: the peak is here, and a page's heap never shrinks)
    let tris: Vec<Tri> = bvh.primitive_indices.iter().map(|&i| {
        let (t, m) = &tris_in[i as usize];
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        Tri { v0: t[0].into(), mat: *m, e1: e1.extend(mats[*m as usize].albedo[3]).into(), e2: e2.extend(0.0).into() } // (e1's w: 0 for a fitting, which shadow rays pass)
    }).collect();
    let (nprims, nnodes) = (tris.len(), nodes.len());
    drop(bvh); drop(tris_in);
    if verbose { eprintln!("  rt bvh: {nprims} tris, {nnodes} nodes, {:.2}s", t0.elapsed().as_secs_f32()); }
    let sp = sky_params(seed);
    let (w, h) = (128u32, 64u32);
    crate::memlog("tris made");
    let (table, sun, sun_e) = sky_table(&sp, w as usize, h as usize);
    if verbose { eprintln!("  sky {sp:?} sun {sun:?} e {sun_e:?}, {:.2}s total", t0.elapsed().as_secs_f32()); }
    crate::memlog("sky made");
    #[cfg(not(target_arch = "wasm32"))]
    { extern "C" { fn mi_collect(force: bool); } unsafe { mi_collect(true); } }
    let buf = |b: &[u8]| dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: b, usage: wgpu::BufferUsages::STORAGE });
    let ld: Vec<LightD> = lights.iter().take(64).map(|(p, c)| LightD { pos: p.extend(1.0).into(), col: c.extend(1.0).into() }).collect();
    let ld2 = if ld.is_empty() { vec![LightD { pos: [0.0; 4], col: [0.0; 4] }] } else { ld.clone() };
    let w0 = [Win { c: [0.0; 4], u: [0.0; 4], v: [0.0; 4], n: [0.0; 4] }];
    // (RT_NO_LC=1: none. The light cache's table starts empty: paths make the cells)
    let lc_n = if std::env::var_os("RT_NO_LC").is_none() { LC_SLOTS as u32 } else { 0 };
    let lc_size = if lc_n > 0 { LC_SLOTS as u64 * 16 } else { 16 };
    let lc_mk = |u| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: lc_size, usage: u, mapped_at_creation: false });
    let lc_rw = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let lc_cells = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: if lc_n > 0 { LC_SLOTS as u64 * 4 } else { 16 }, usage: lc_rw, mapped_at_creation: false });
    // (the reflection cache's direction maps: a map index for each slot, and the maps, given out as reflections ask for them)
    let m_mk = |sz: u64| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: if lc_n > 0 { sz } else { 16 }, usage: lc_rw, mapped_at_creation: false });
    SceneData { m_idx: m_mk((LC_SLOTS as u64 + 1) * 4), m_acc: m_mk(M_MAPS * 1024), m_out: m_mk(M_MAPS * 85 * 16), lc_guide: dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: if lc_n > 0 { LC_SLOTS as u64 * 64 } else { 64 }, usage: lc_rw, mapped_at_creation: false }), pane_st: std::sync::Mutex::new(None), taa_st: std::sync::Mutex::new(None), hist_st: std::sync::Mutex::new(None), lc_eye: std::sync::Mutex::new(Vec3::splat(f32::MAX)), lc_cells, lc_acc: lc_mk(lc_rw), lc_out: lc_mk(lc_rw), lc_n, wins: buf(bytemuck::cast_slice(if wins.is_empty() { &w0 } else { wins })), nwins: wins.len() as u32, nodes: buf(bytemuck::cast_slice(&nodes)), tris: buf(bytemuck::cast_slice(&tris)), mats: buf(bytemuck::cast_slice(mats)), lights: buf(bytemuck::cast_slice(&ld2)), sky: buf(bytemuck::cast_slice(&table)), nlights: ld.len() as u32, sky_wh: (w, h), sun, sun_e, skyp: sp }
}

pub struct Gb<'a> { pub mirror_lit: &'a wgpu::TextureView, pub depth: &'a wgpu::TextureView, pub nrm: &'a wgpu::TextureView, pub alb: &'a wgpu::TextureView, pub mat: &'a wgpu::TextureView, pub lit: &'a wgpu::TextureView, pub hdr: &'a wgpu::TextureView }

/// the light as traced, through Open Image Denoise and back: everything so far has to finish first
#[cfg(not(target_arch = "wasm32"))]
fn learned_filter(dev: &wgpu::Device, q: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, irr: &wgpu::Texture, nrm: &wgpu::Texture, res: (u32, u32), ss: u32, quality: i32) {
    let (w, h) = (res.0 as usize, res.1 as usize); let (nw, nh) = (res.0 * ss, res.1 * ss);
    let bpr = (res.0 * 8 + 255) / 256 * 256; let nbpr = (nw * 8 + 255) / 256 * 256;
    let mk = |sz: u64| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: sz, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let (bi, bn) = (mk((bpr * res.1) as u64), mk((nbpr * nh) as u64));
    let copy = |e: &mut wgpu::CommandEncoder, t: &wgpu::Texture, b: &wgpu::Buffer, r: u32, ww: u32, hh: u32| e.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r), rows_per_image: Some(hh) } }, wgpu::Extent3d { width: ww, height: hh, depth_or_array_layers: 1 });
    copy(enc, irr, &bi, bpr, res.0, res.1); copy(enc, nrm, &bn, nbpr, nw, nh);
    let done = std::mem::replace(enc, dev.create_command_encoder(&Default::default())); q.submit([done.finish()]);
    bi.slice(..).map_async(wgpu::MapMode::Read, |_| {}); bn.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let half = |d: &[u8], o: usize| half::f16::from_le_bytes([d[o], d[o + 1]]).to_f32();
    let (di, dn) = (bi.slice(..).get_mapped_range().unwrap(), bn.slice(..).get_mapped_range().unwrap());
    let (mut col, mut nor) = (vec![0f32; w * h * 3], vec![0f32; w * h * 3]);
    let mut alpha = vec![0u8; w * h * 2];
    for y in 0..h { for x in 0..w {
        let o = y * bpr as usize + x * 8; let on = (y * ss as usize + ss as usize / 2) * nbpr as usize + (x * ss as usize + ss as usize / 2) * 8;
        for c in 0..3 { let v = half(&di, o + c * 2); col[(y * w + x) * 3 + c] = if v.is_finite() { v } else { 0.0 }; let n = half(&dn, on + c * 2); nor[(y * w + x) * 3 + c] = if n.is_finite() { n } else { 0.0 }; }
        alpha[(y * w + x) * 2..(y * w + x) * 2 + 2].copy_from_slice(&di[o + 6..o + 8]);
    } }
    drop(di); drop(dn);
    let Some(out) = crate::oidn::denoise(&col, &nor, w, h, std::env::var("RT_OIDN_Q").ok().and_then(|v| v.parse().ok()).unwrap_or(quality)) else { return };
    let mut bytes = Vec::with_capacity(w * h * 8);
    for i in 0..w * h { for c in 0..3 { bytes.extend_from_slice(&half::f16::from_f32(out[i * 3 + c]).to_le_bytes()); } bytes.extend_from_slice(&alpha[i * 2..i * 2 + 2]); }
    q.write_texture(wgpu::TexelCopyTextureInfo { texture: irr, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(res.0 * 8), rows_per_image: Some(res.1) }, wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 });
}

/// on a page the denoiser runs in the page, which takes waiting: one trace to hand the light over (`Capture`, stopping there), one to put the result in its place (`Inject`)
#[cfg(target_arch = "wasm32")]
pub enum WebDn { Off, Capture(Option<(wgpu::Buffer, wgpu::Buffer, u32, u32)>), Inject(Vec<u8>) }
#[cfg(target_arch = "wasm32")]
thread_local! { pub static WEB_DN: std::cell::RefCell<WebDn> = std::cell::RefCell::new(WebDn::Off); }

pub fn encode(dev: &wgpu::Device, q: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, p: &Pipes, sd: &SceneData, gb: &Gb, res: (u32, u32), ss: u32, qu: Quality, vp: Mat4, eye: Vec3, frame: u32, scales: [f32; 3], cutaway: bool) {
    let mut qu = qu; if ss > 1 { if let Some(n) = std::env::var("RT_SPP").ok().and_then(|v| v.parse().ok()) { qu.spp = n; } }
    // (fast mode's one path a pixel shares with its neighbours; full mode's two showed no gain from it)
    let use_restir = (ss > 1 || qu.video) && qu.spp == 1;
    let use_lc = (ss > 1 || qu.video) && sd.lc_n > 0;
    let use_rs_spec = ss > 1 || qu.video;
    // (a video after its first frame gathers for what comes into view at a quarter of its pixels, one in each 2x2, and the rest take theirs by ReSTIR: games' practice, and the gather is most of a frame)
    let qgather = qu.video && qu.hist && use_restir && sd.hist_st.lock().unwrap().is_some();
    let pr = Params { inv_vp: vp.inverse().to_cols_array_2d(), cam: eye.extend(1.0 + if use_restir { 2.0 } else { 0.0 } + if use_rs_spec { 4.0 } else { 0.0 } + if use_lc { 8.0 } else { 0.0 } + if qu.video { 32.0 } else { 0.0 } + if qu.video && qu.hist { 64.0 } else { 0.0 } + if qgather { 1024.0 } else { 0.0 } + if qu.hist { 2048.0 } else { 0.0 } + if qu.no_cells { 4096.0 } else { 0.0 }).into(), sun_dir: sd.sun.extend(sd.skyp.sun_size * 0.5).into(), sun_col: (sd.sun_e * sd.skyp.strength * scales[2]).extend(0.0).into(),
        dims: [res.0, res.1, ss, qu.spp], misc: [qu.bounces, frame, sd.nlights, sd.sky_wh.0], misc2: [sd.sky_wh.1, qu.spec_spp, sd.nwins, match std::env::var("RT_SHOW").as_deref() { Ok("albedo") => 2, Ok("sun") => 5, Ok("normal") => 6, Ok("flags") => 7, Ok("noglint") => 8, Ok("sheer") => 9, Ok("light") => 3, Ok("soft") => 10, Ok("unc") => 12, Ok("lamps") => 11, Ok("spec") => 4, _ => cutaway as u32 }], scale: [scales[0], sd.skyp.strength * scales[1], scales[2], sd.skyp.sun_size * 0.5] };
    q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&pr));
    use wgpu::{BindGroupEntry as B, BindingResource as R};
    let (wx, wy) = (res.0.div_ceil(8), res.1.div_ceil(8));
    // (RT_PROF=1: each pass alone, timed)
    let prof = std::env::var_os("RT_PROF").is_some();
    let t0 = std::cell::Cell::new(web_time::Instant::now());
    let lap = |enc: &mut wgpu::CommandEncoder, name: &str| {
        if !prof { return; }
        let done = std::mem::replace(enc, dev.create_command_encoder(&Default::default()));
        q.submit([done.finish()]); dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        eprintln!("  prof {name:14} {:7.1} ms", t0.get().elapsed().as_secs_f32() * 1000.0); t0.set(web_time::Instant::now());
    };
    lap(enc, "before trace");
    // the G-buffer's light first, then, with its clean result to shade by, the reflections. The trace writes one of the pairs and reads the other
    // (a video's history: last frame's images are read and this frame's written, turn about)
    let hist_prev = if qu.hist { *sd.hist_st.lock().unwrap() } else { None };
    let pv_u = { let mut u = [0f32; 20]; let (v0, e0) = hist_prev.map_or((vp, eye), |x| (x.0, x.1)); u[..16].copy_from_slice(&v0.to_cols_array()); u[16..19].copy_from_slice(&e0.to_array()); wgpu::util::DeviceExt::create_buffer_init(dev, &wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&u), usage: wgpu::BufferUsages::UNIFORM }) };
    let bg_u = std::cell::Cell::new(&p.ubuf);
    let trace_bg = |irr_out: &wgpu::TextureView, spec_out: &wgpu::TextureView, irr_in: &wgpu::TextureView, lamp_out: &wgpu::TextureView, lamp_in: &wgpu::TextureView| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.trace_bgl, entries: &[
        B { binding: 27, resource: p.vis.as_entire_binding() }, B { binding: 28, resource: sd.m_idx.as_entire_binding() }, B { binding: 29, resource: sd.m_acc.as_entire_binding() }, B { binding: 30, resource: sd.m_out.as_entire_binding() }, B { binding: 26, resource: sd.lc_guide.as_entire_binding() }, B { binding: 22, resource: R::TextureView(&p.hn[0][hist_prev.map_or(0, |x| x.2)]) }, B { binding: 23, resource: pv_u.as_entire_binding() }, B { binding: 24, resource: p.resv_t[1 - hist_prev.map_or(1, |x| 1 - x.2)].as_entire_binding() }, B { binding: 25, resource: p.resv_t[hist_prev.map_or(1, |x| 1 - x.2)].as_entire_binding() }, B { binding: 18, resource: p.resv.as_entire_binding() }, B { binding: 19, resource: sd.lc_cells.as_entire_binding() }, B { binding: 20, resource: sd.lc_acc.as_entire_binding() }, B { binding: 21, resource: sd.lc_out.as_entire_binding() }, B { binding: 0, resource: bg_u.get().as_entire_binding() }, B { binding: 1, resource: sd.nodes.as_entire_binding() }, B { binding: 2, resource: sd.tris.as_entire_binding() },
        B { binding: 3, resource: sd.mats.as_entire_binding() }, B { binding: 4, resource: sd.lights.as_entire_binding() }, B { binding: 5, resource: sd.sky.as_entire_binding() },
        B { binding: 6, resource: R::TextureView(gb.depth) }, B { binding: 7, resource: R::TextureView(gb.nrm) }, B { binding: 8, resource: R::TextureView(gb.alb) },
        B { binding: 9, resource: R::TextureView(irr_out) }, B { binding: 10, resource: R::TextureView(spec_out) }, B { binding: 11, resource: sd.wins.as_entire_binding() }, B { binding: 12, resource: R::TextureView(irr_in) }, B { binding: 13, resource: R::TextureView(gb.mirror_lit) }, B { binding: 14, resource: R::TextureView(&p.glint) }, B { binding: 15, resource: R::TextureView(lamp_out) }, B { binding: 16, resource: R::TextureView(lamp_in) }, B { binding: 17, resource: R::TextureView(gb.mat) }, B { binding: 32, resource: R::TextureView(&p.sunv) }] });
    let hist_i = hist_prev.map_or(0, |x| 1 - x.2);
    let accum = |enc: &mut wgpu::CommandEncoder, kind: usize, cur_v: &wgpu::TextureView, show: bool| {
        let mut tu = [0f32; 24]; let (pv, pe, first, pr) = hist_prev.map_or((vp, eye, 1.0, res), |x| (x.0, x.1, 0.0, x.3));
        tu[..16].copy_from_slice(&pv.to_cols_array()); tu[16..19].copy_from_slice(&pe.to_array()); tu[20] = first; tu[21] = pr.0 as f32; tu[22] = pr.1 as f32; tu[23] = kind as f32;
        let ub = wgpu::util::DeviceExt::create_buffer_init(dev, &wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&tu), usage: wgpu::BufferUsages::UNIFORM });
        let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.tmp_bgl, entries: &[B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 8, resource: ub.as_entire_binding() }, B { binding: 2, resource: R::TextureView(gb.depth) }, B { binding: 3, resource: R::TextureView(gb.nrm) }, B { binding: 4, resource: R::TextureView(cur_v) }, B { binding: 5, resource: R::TextureView(&p.hist[kind][1 - hist_i]) }, B { binding: 6, resource: R::TextureView(&p.hist[kind][hist_i]) }, B { binding: 9, resource: R::TextureView(&p.lamp[0]) }, B { binding: 10, resource: R::TextureView(&p.hn[kind][1 - hist_i]) }, B { binding: 11, resource: R::TextureView(&p.hn[kind][hist_i]) }, B { binding: 12, resource: R::TextureView(&p.glint) }, B { binding: 15, resource: R::TextureView(&p.mom[1 - hist_i]) }, B { binding: 16, resource: R::TextureView(&p.mom[hist_i]) }] });
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.temporal); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups(res.0.div_ceil(8), res.1.div_ceil(8), 1); }
        if show { enc.copy_texture_to_texture(p.hist[kind][hist_i].texture().as_image_copy(), cur_v.texture().as_image_copy(), wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 }); }
    };
    let filter = |enc: &mut wgpu::CommandEncoder, a: usize, b: usize, step: usize| {
        let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.denoise_bgl, entries: &[
            B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: p.step_ubufs[step].as_entire_binding() }, B { binding: 2, resource: R::TextureView(gb.depth) }, B { binding: 3, resource: R::TextureView(gb.nrm) },
            B { binding: 4, resource: R::TextureView(&p.irr[a]) }, B { binding: 5, resource: R::TextureView(&p.spec[a]) }, B { binding: 6, resource: R::TextureView(&p.irr[b]) }, B { binding: 7, resource: R::TextureView(&p.spec[b]) }, B { binding: 17, resource: R::TextureView(&p.var[a]) }, B { binding: 18, resource: R::TextureView(&p.var[b]) }, B { binding: 19, resource: R::TextureView(&p.mom[hist_i]) }, B { binding: 13, resource: R::TextureView(&p.hn[if step < 6 { 0 } else { 1 }][hist_i]) }] });
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.denoise); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups(wx, wy, 1);
    };
    // (the rays of a pass all run at once and can't see each other's light: a sparse pass first, so the full trace of a new view's first frame reads the cache too)
    let new_view = use_lc && { let mut le = sd.lc_eye.lock().unwrap(); let n = *le != eye && !(qu.video && le.x != f32::MAX); *le = eye; n };
    if new_view {
        let mut pre = pr; pre.cam[3] += 16.0; q.write_buffer(&p.ubuf_pre, 0, bytemuck::bytes_of(&pre));
        bg_u.set(&p.ubuf_pre);
        let tg = trace_bg(&p.irr[0], &p.spec[0], &p.spec[1], &p.lamp[0], &p.lamp[1]);
        bg_u.set(&p.ubuf);
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_resolve); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(LC_SLOTS as u32 / 64, 1, 1); }
        lap(enc, "lc fill");
    }
    { let tg = trace_bg(&p.irr[0], &p.spec[0], &p.spec[1], &p.lamp[0], &p.lamp[1]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
    lap(enc, "trace");
    if qgather {
        let tg = trace_bg(&p.irr[1], &p.spec[1], &p.irr[0], &p.lamp[1], &p.lamp[0]);
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.gather_q); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(res.0.div_ceil(16), res.1.div_ceil(16), 1); drop(c);
        lap(enc, "gather_q");
    }
    // what this trace's paths left in the cache, for the reflections next and later frames
    if use_lc {
        let tg = trace_bg(&p.irr[1], &p.spec[1], &p.irr[0], &p.lamp[1], &p.lamp[0]);
        if qu.video && qu.hist && hist_prev.is_some() {
            let n = 16384u32;
            let g = ((n as f32).sqrt() as u32).div_ceil(8);
            { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_fill); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(g, g, 1); }
            lap(enc, "cache fill");
        }
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_resolve); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(LC_SLOTS as u32 / 64, 1, 1); drop(c);
        lap(enc, "lc resolve");
    }
    let mut cur = 0usize;
    if use_restir {
        let tg = trace_bg(&p.irr[1], &p.spec[1], &p.irr[0], &p.lamp[1], &p.lamp[0]);
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.restir); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); drop(c);
        cur = 1; lap(enc, "restir");
    }
    #[cfg(target_arch = "wasm32")]
    let mut capture_stop = false;
    // the light's filter: the learned one where there is one (not for a mirror's small picture), or the a-trous rounds
    #[cfg(not(target_arch = "wasm32"))]
    let learned = ss > 1 && !qu.video && qu.denoise > 0 && crate::oidn::available() && std::env::var_os("RT_NOFILTER").is_none();
    #[cfg(target_arch = "wasm32")]
    let learned = if ss == 1 { false } else {
        WEB_DN.with(|d| {
            let mut d = d.borrow_mut();
            match &mut *d {
                WebDn::Off => None,
                WebDn::Capture(slot) => {
                    let (nw, nh) = (res.0 * ss, res.1 * ss); let bpr = (res.0 * 8 + 255) / 256 * 256; let nbpr = (nw * 8 + 255) / 256 * 256;
                    let mk = |sz: u64| dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: sz, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
                    let (bi, bn) = (mk((bpr * res.1) as u64), mk((nbpr * nh) as u64));
                    let copy = |e: &mut wgpu::CommandEncoder, t: &wgpu::Texture, b: &wgpu::Buffer, r_: u32, ww: u32, hh: u32| e.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        wgpu::TexelCopyBufferInfo { buffer: b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r_), rows_per_image: Some(hh) } }, wgpu::Extent3d { width: ww, height: hh, depth_or_array_layers: 1 });
                    copy(enc, p.irr[cur].texture(), &bi, bpr, res.0, res.1); copy(enc, gb.nrm.texture(), &bn, nbpr, nw, nh);
                    *slot = Some((bi, bn, bpr, nbpr));
                    Some(true)
                }
                WebDn::Inject(bytes) => {
                    let done = std::mem::replace(enc, dev.create_command_encoder(&Default::default())); q.submit([done.finish()]);
                    q.write_texture(wgpu::TexelCopyTextureInfo { texture: p.irr[cur].texture(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, bytes,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(res.0 * 8), rows_per_image: Some(res.1) }, wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 });
                    Some(false)
                }
            }
        }).map_or(false, |capture| { capture_stop = capture; true })
    };
    #[cfg(target_arch = "wasm32")]
    if capture_stop { return; }
    if qu.hist && qu.video { accum(enc, 2, &p.lamp[0], true); accum(enc, 3, &p.sunv, true); accum(enc, 0, &p.irr[cur], true); lap(enc, "temporal"); }
    if learned {
        #[cfg(not(target_arch = "wasm32"))]
        learned_filter(dev, q, enc, p.irr[cur].texture(), gb.nrm.texture(), res, ss, 4);
    } else {
        for i in 0..if std::env::var_os("RT_NOFILTER").is_some() && ss > 1 { 0 } else { qu.denoise as usize } {
            filter(enc, cur, 1 - cur, i); cur = 1 - cur;
            // (a video keeps the first round as its history, as SVGF does: what few rays began is cleaned as it is kept, and its noise does not stay on a wall. Light from the first frame the filter leaves as it is)
            if i == 0 && qu.video && qu.hist && hist_prev.is_some() { enc.copy_texture_to_texture(p.irr[cur].texture().as_image_copy(), p.hist[0][hist_i].texture().as_image_copy(), wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 }); }
        }
    }
    lap(enc, "denoise");
    if qu.hist && !qu.video { accum(enc, 2, &p.lamp[0], false); accum(enc, 0, &p.irr[cur], false); lap(enc, "keep light"); }
    if qu.hist && !qu.video && use_lc {
        let tg = trace_bg(&p.irr[1 - cur], &p.spec[1 - cur], &p.irr[cur], &p.lamp[1], &p.lamp[0]);
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_seed); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
        // (the cache's coarse cells, all round the room, while this first frame has its time: what a turn will reveal is then lit already)
        { let g = (262144f32.sqrt() as u32).div_ceil(8); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_room); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(g, g, 1); }
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_resolve); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(LC_SLOTS as u32 / 64, 1, 1); }
        lap(enc, "seed cache");
    }
    { let tg = trace_bg(&p.irr[1 - cur], &p.spec[cur], &p.irr[cur], &p.lamp[1], &p.lamp[0]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace_spec); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
    lap(enc, "trace_spec");
    if use_rs_spec {
        let tg = trace_bg(&p.irr[1 - cur], &p.spec[1 - cur], &p.spec[cur], &p.lamp[0], &p.lamp[1]);
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.restir_spec); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); drop(c);
        enc.copy_texture_to_texture(p.spec[1 - cur].texture().as_image_copy(), p.spec[cur].texture().as_image_copy(), wgpu::Extent3d { width: res.0, height: res.1, depth_or_array_layers: 1 });
        lap(enc, "restir_spec");
    }
    if qu.hist && qu.video { accum(enc, 1, &p.spec[cur], true); lap(enc, "temporal spec"); }
    // (rounds on rough reflections, in the step's w: three keep a metal's smudges; with one ray a pixel that leaves blotches, so five)
    let mut spec_base = if qu.spec_spp > 1 || (qu.video && hist_prev.is_some()) { 6 } else { 12 }; // (a video has its frames in place of more rays)
    #[cfg(not(target_arch = "wasm32"))]
    for i in 0..5usize { filter(enc, cur, 1 - cur, spec_base + i); cur = 1 - cur; }
    lap(enc, "spec filter");
    if qu.hist && !qu.video { accum(enc, 1, &p.spec[cur], false); }
    if qu.hist && !qu.video && use_lc {
        let tg = trace_bg(&p.irr[1 - cur], &p.spec[1 - cur], &p.spec[cur], &p.lamp[1], &p.lamp[0]);
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.lc_resolve); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(LC_SLOTS as u32 / 64, 1, 1); }
    }
    if qu.hist { *sd.hist_st.lock().unwrap() = Some((vp, eye, hist_i, res)); }
    { let tg = trace_bg(&p.fine, &p.spec[1 - cur], &p.irr[cur], &p.lamp[0], &p.lamp[1]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace_fine); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups((res.0 * ss).div_ceil(8), (res.1 * ss).div_ceil(8), 1); }
    lap(enc, "trace_fine");
    let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.compose_bgl, entries: &[
        B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: sd.sky.as_entire_binding() }, B { binding: 2, resource: R::TextureView(gb.depth) }, B { binding: 3, resource: R::TextureView(gb.nrm) },
        B { binding: 4, resource: R::TextureView(gb.alb) }, B { binding: 5, resource: R::TextureView(gb.mat) }, B { binding: 6, resource: R::TextureView(&p.irr[cur]) }, B { binding: 7, resource: R::TextureView(&p.spec[cur]) },
        B { binding: 8, resource: R::TextureView(gb.hdr) }, B { binding: 9, resource: R::TextureView(gb.lit) }, B { binding: 10, resource: R::TextureView(&p.fine) }, B { binding: 11, resource: R::TextureView(&p.glint) }, B { binding: 12, resource: R::TextureView(&p.lamp[1]) }, B { binding: 13, resource: p.vis.as_entire_binding() }, B { binding: 16, resource: R::TextureView(&p.sunv) }, B { binding: 14, resource: R::TextureView(&p.mom[hist_i]) }, B { binding: 15, resource: R::TextureView(&p.var[cur]) }] });
    let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.compose); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups((res.0 * ss).div_ceil(8), (res.1 * ss).div_ceil(8), 1);
    drop(c);
    lap(enc, "compose");
}
