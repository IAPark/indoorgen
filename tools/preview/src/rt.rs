//! rt: hybrid path tracing for the previewer (--rt fast|full). The rasterizer draws primary visibility and materials into a G-buffer; a compute shader traces the BVH
//! of the scene for bounce light, soft sun, lamps and reflections; an edge-aware filter stands in for Blender's denoiser. The light is Blender's: the same seeded sky
//! (render_rust.py), the same units, AgX with Medium High Contrast and the same auto-exposure rule.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;


#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quality { pub spp: u32, pub bounces: u32, pub spec_spp: u32, pub denoise: u32, pub ss: u32 }
impl Quality {
    pub fn parse(s: &str) -> Quality {
        match s {
            "fast" => Quality { spp: 3, bounces: 2, spec_spp: 1, denoise: 5, ss: 3 },
            "full" => Quality { spp: 6, bounces: 3, spec_spp: 3, denoise: 5, ss: 3 },
            _ => panic!("--rt fast|full"),
        }
    }
}

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

pub struct Pipes {
    pub fine: wgpu::TextureView, pub trace_fine: wgpu::ComputePipeline,
    pub trace: wgpu::ComputePipeline, pub trace_spec: wgpu::ComputePipeline, pub denoise: wgpu::ComputePipeline, pub compose: wgpu::ComputePipeline,
    pub trace_bgl: wgpu::BindGroupLayout, pub denoise_bgl: wgpu::BindGroupLayout, pub compose_bgl: wgpu::BindGroupLayout,
    pub ubuf: wgpu::Buffer, pub step_ubufs: Vec<wgpu::Buffer>,
    pub irr: [wgpu::TextureView; 2], pub spec: [wgpu::TextureView; 2],
}

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
pub struct Win { pub c: [f32; 4], pub u: [f32; 4], pub v: [f32; 4], pub n: [f32; 4] }
pub struct SceneData { pub wins: wgpu::Buffer, pub nwins: u32, pub nodes: wgpu::Buffer, pub tris: wgpu::Buffer, pub mats: wgpu::Buffer, pub lights: wgpu::Buffer, pub sky: wgpu::Buffer, pub nlights: u32, pub sky_wh: (u32, u32), pub sun: Vec3, pub sun_e: Vec3, pub skyp: SkyParams }

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
    let trace_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), sto(1), sto(2), sto(3), sto(4), sto(5), dep(6), tex(7), tex(8), out(9), out(10), sto(11), tex(12), tex(13)] });
    let denoise_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), uni(1), dep(2), tex(3), tex(4), tex(5), out(6), out(7)] });
    let compose_bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uni(0), sto(1), dep(2), tex(3), tex(4), tex(5), tex(6), tex(7), tex(8), out(9), tex(10)] });
    let pipe = |bgl: &wgpu::BindGroupLayout, src: &str, ep: &str| {
        let sm = module(src);
        let l = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(bgl)], immediate_size: 0 });
        dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: Some(&l), module: &sm, entry_point: Some(ep), compilation_options: Default::default(), cache: None })
    };
    let trace_spec = pipe(&trace_bgl, include_str!("rt.wgsl"), "trace_spec");
    let trace_fine = pipe(&trace_bgl, include_str!("rt.wgsl"), "trace_fine");
    let (trace, denoise, compose) = (pipe(&trace_bgl, include_str!("rt.wgsl"), "trace"), pipe(&denoise_bgl, include_str!("rt_post.wgsl"), "denoise"), pipe(&compose_bgl, include_str!("rt_comp.wgsl"), "compose"));
    let ubuf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let step_ubufs = (0..12).map(|i: u32| dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&[1u32 << (i % 6), i % 6, (i / 6), 0]), usage: wgpu::BufferUsages::UNIFORM })).collect();
    let mk_at = |w: u32, h: u32| dev.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: FMT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING, view_formats: &[] }).create_view(&Default::default());
    let mk = || mk_at(w, h);
    Pipes { fine: mk_at(w * ss, h * ss), trace_fine, trace, trace_spec, denoise, compose, trace_bgl, denoise_bgl, compose_bgl, ubuf, step_ubufs, irr: [mk(), mk()], spec: [mk(), mk()] }
}

/// triangles with their draw's index
pub fn build_scene(dev: &wgpu::Device, tris_in: &[([Vec3; 3], u32)], mats: &[MatD], lights: &[(Vec3, Vec3)], wins: &[Win], seed: u64, verbose: bool) -> SceneData {
    use obvhs::triangle::Triangle;
    let t0 = std::time::Instant::now();
    let tris_o: Vec<Triangle> = tris_in.iter().map(|(t, _)| Triangle { v0: t[0].into(), v1: t[1].into(), v2: t[2].into() }).collect();
    let mut core = std::time::Duration::ZERO;
    let mut params = if std::env::var_os("RT_BVH_FAST").is_some() { obvhs::BvhBuildParams::fast_build() } else { obvhs::BvhBuildParams::medium_build() };
    params.max_prims_per_leaf = 4;
    let bvh = obvhs::bvh2::builder::build_bvh2_from_tris(&tris_o, params, &mut core);
    let nodes: Vec<Node> = bvh.nodes.iter().map(|n| Node { lo: n.aabb.min.into(), a: n.first_index, hi: n.aabb.max.into(), b: n.prim_count }).collect();
    let tris: Vec<Tri> = bvh.primitive_indices.iter().map(|&i| {
        let (t, m) = &tris_in[i as usize];
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        Tri { v0: t[0].into(), mat: *m, e1: e1.extend(0.0).into(), e2: e2.extend(0.0).into() }
    }).collect();
    if verbose { eprintln!("  rt bvh: {} tris, {} nodes, {:.2}s", tris.len(), nodes.len(), t0.elapsed().as_secs_f32()); }
    let sp = sky_params(seed);
    let (w, h) = (128u32, 64u32);
    let (table, sun, sun_e) = sky_table(&sp, w as usize, h as usize);
    if verbose { eprintln!("  sky {sp:?} sun {sun:?} e {sun_e:?}, {:.2}s total", t0.elapsed().as_secs_f32()); }
    let buf = |b: &[u8]| dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: b, usage: wgpu::BufferUsages::STORAGE });
    let ld: Vec<LightD> = lights.iter().take(64).map(|(p, c)| LightD { pos: p.extend(1.0).into(), col: c.extend(1.0).into() }).collect();
    let ld2 = if ld.is_empty() { vec![LightD { pos: [0.0; 4], col: [0.0; 4] }] } else { ld.clone() };
    let w0 = [Win { c: [0.0; 4], u: [0.0; 4], v: [0.0; 4], n: [0.0; 4] }];
    SceneData { wins: buf(bytemuck::cast_slice(if wins.is_empty() { &w0 } else { wins })), nwins: wins.len() as u32, nodes: buf(bytemuck::cast_slice(&nodes)), tris: buf(bytemuck::cast_slice(&tris)), mats: buf(bytemuck::cast_slice(mats)), lights: buf(bytemuck::cast_slice(&ld2)), sky: buf(bytemuck::cast_slice(&table)), nlights: ld.len() as u32, sky_wh: (w, h), sun, sun_e, skyp: sp }
}

pub struct Gb<'a> { pub mirror_lit: &'a wgpu::TextureView, pub depth: &'a wgpu::TextureView, pub nrm: &'a wgpu::TextureView, pub alb: &'a wgpu::TextureView, pub mat: &'a wgpu::TextureView, pub lit: &'a wgpu::TextureView, pub hdr: &'a wgpu::TextureView }

pub fn encode(dev: &wgpu::Device, q: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, p: &Pipes, sd: &SceneData, gb: &Gb, res: (u32, u32), ss: u32, qu: Quality, vp: Mat4, eye: Vec3, frame: u32, scales: [f32; 3], cutaway: bool) {
    let pr = Params { inv_vp: vp.inverse().to_cols_array_2d(), cam: eye.extend(1.0).into(), sun_dir: sd.sun.extend(sd.skyp.sun_size * 0.5).into(), sun_col: (sd.sun_e * sd.skyp.strength * scales[2]).extend(0.0).into(),
        dims: [res.0, res.1, ss, qu.spp], misc: [qu.bounces, frame, sd.nlights, sd.sky_wh.0], misc2: [sd.sky_wh.1, qu.spec_spp, sd.nwins, match std::env::var("RT_SHOW").as_deref() { Ok("albedo") => 2, Ok("sun") => 5, Ok("normal") => 6, Ok("flags") => 7, Ok("light") => 3, Ok("spec") => 4, _ => cutaway as u32 }], scale: [scales[0], sd.skyp.strength * scales[1], scales[2], sd.skyp.sun_size * 0.5] };
    q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&pr));
    use wgpu::{BindGroupEntry as B, BindingResource as R};
    let (wx, wy) = (res.0.div_ceil(8), res.1.div_ceil(8));
    // the G-buffer's light first, then, with its clean result to shade by, the reflections. The trace writes one of the pairs and reads the other
    let trace_bg = |irr_out: &wgpu::TextureView, spec_out: &wgpu::TextureView, irr_in: &wgpu::TextureView| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.trace_bgl, entries: &[
        B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: sd.nodes.as_entire_binding() }, B { binding: 2, resource: sd.tris.as_entire_binding() },
        B { binding: 3, resource: sd.mats.as_entire_binding() }, B { binding: 4, resource: sd.lights.as_entire_binding() }, B { binding: 5, resource: sd.sky.as_entire_binding() },
        B { binding: 6, resource: R::TextureView(gb.depth) }, B { binding: 7, resource: R::TextureView(gb.nrm) }, B { binding: 8, resource: R::TextureView(gb.alb) },
        B { binding: 9, resource: R::TextureView(irr_out) }, B { binding: 10, resource: R::TextureView(spec_out) }, B { binding: 11, resource: sd.wins.as_entire_binding() }, B { binding: 12, resource: R::TextureView(irr_in) }, B { binding: 13, resource: R::TextureView(gb.mirror_lit) }] });
    let filter = |enc: &mut wgpu::CommandEncoder, a: usize, b: usize, step: usize| {
        let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.denoise_bgl, entries: &[
            B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: p.step_ubufs[step].as_entire_binding() }, B { binding: 2, resource: R::TextureView(gb.depth) }, B { binding: 3, resource: R::TextureView(gb.nrm) },
            B { binding: 4, resource: R::TextureView(&p.irr[a]) }, B { binding: 5, resource: R::TextureView(&p.spec[a]) }, B { binding: 6, resource: R::TextureView(&p.irr[b]) }, B { binding: 7, resource: R::TextureView(&p.spec[b]) }] });
        let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.denoise); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups(wx, wy, 1);
    };
    { let tg = trace_bg(&p.irr[0], &p.spec[0], &p.spec[1]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
    let mut cur = 0usize;
    for i in 0..qu.denoise as usize { filter(enc, cur, 1 - cur, i); cur = 1 - cur; }
    { let tg = trace_bg(&p.irr[1 - cur], &p.spec[cur], &p.irr[cur]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace_spec); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups(wx, wy, 1); }
    for i in 0..3usize { filter(enc, cur, 1 - cur, 6 + i); cur = 1 - cur; }
    { let tg = trace_bg(&p.fine, &p.spec[1 - cur], &p.irr[cur]); let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.trace_fine); c.set_bind_group(0, &tg, &[]); c.dispatch_workgroups((res.0 * ss).div_ceil(8), (res.1 * ss).div_ceil(8), 1); }
    let bg = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.compose_bgl, entries: &[
        B { binding: 0, resource: p.ubuf.as_entire_binding() }, B { binding: 1, resource: sd.sky.as_entire_binding() }, B { binding: 2, resource: R::TextureView(gb.depth) }, B { binding: 3, resource: R::TextureView(gb.nrm) },
        B { binding: 4, resource: R::TextureView(gb.alb) }, B { binding: 5, resource: R::TextureView(gb.mat) }, B { binding: 6, resource: R::TextureView(&p.irr[cur]) }, B { binding: 7, resource: R::TextureView(&p.spec[cur]) },
        B { binding: 8, resource: R::TextureView(gb.hdr) }, B { binding: 9, resource: R::TextureView(gb.lit) }, B { binding: 10, resource: R::TextureView(&p.fine) }] });
    let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p.compose); c.set_bind_group(0, &bg, &[]); c.dispatch_workgroups((res.0 * ss).div_ceil(8), (res.1 * ss).div_ceil(8), 1);
}
