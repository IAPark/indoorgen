//! The baked renderer in a page, by WebGPU: tools/web_rt/build.sh makes the module. The page hands over a house (glb and sidecar), waits for the bake, then asks for a frame per camera.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

thread_local! { static PROGRESS: RefCell<Option<js_sys::Function>> = RefCell::new(None); }
pub fn progress(f: f32) { PROGRESS.with(|p| { if let Some(cb) = p.borrow().as_ref() { cb.call1(&JsValue::NULL, &JsValue::from_f64(f as f64)).ok(); } }); }

struct Inner { r: Renderer, s: Scene, o: Opts, l: bake::Live, surface: wgpu::Surface<'static> }
#[wasm_bindgen]
pub struct Viewer(Rc<RefCell<Inner>>);

fn opts(res: (u32, u32), seed: u32, secs: f32) -> Opts {
    let mut o = parse_args();
    o.rt = Some(rt::Quality::parse("full")); o.ss = 1; o.res = res; o.seed = seed as u64;
    o.bake = Some(format!("live:{secs}"));
    bake::ON.store(true, std::sync::atomic::Ordering::Relaxed);
    o
}
fn cam(e: &[f32], lens: f32) -> Cam { cam_look(Vec3::new(e[0], e[1], e[2]), Vec3::new(e[3], e[4], e[5]), lens) }

/// whether this browser can run it
#[wasm_bindgen]
pub async fn supported() -> bool { wgpu::util::is_browser_webgpu_supported().await }

/// bake a house: `eye_target` (ex,ey,ez,tx,ty,tz, z up), `lens` in mm on 36 mm film, sets the first exposure
#[wasm_bindgen]
pub async fn bake_scene(canvas: web_sys::HtmlCanvasElement, glb_bytes: Vec<u8>, sidecar: String, seed: u32, bar: f32, eye_target: Vec<f32>, lens: f32, on_progress: js_sys::Function) -> Result<Viewer, JsValue> {
    console_error_panic_hook::set_once();
    PROGRESS.with(|p| *p.borrow_mut() = Some(on_progress));
    let res = (canvas.width(), canvas.height());
    glb::FILES.with(|f| { let mut f = f.borrow_mut(); f.clear(); f.insert("scene.glb".into(), glb_bytes); f.insert("scene.json".into(), sidecar.into_bytes()); });
    let o = opts(res, seed, bar);
    let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = inst.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(|e| JsValue::from_str(&format!("surface: {e}")))?;
    let mut r = init_with(&inst, Some(&surface), &o).await;
    surface.configure(&r.gpu.dev, &wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT, format: wgpu::TextureFormat::Rgba8Unorm, width: res.0, height: res.1, present_mode: wgpu::PresentMode::Fifo, desired_maximum_frame_latency: 2, alpha_mode: wgpu::CompositeAlphaMode::Auto, view_formats: vec![], color_space: wgpu::SurfaceColorSpace::Auto });
    let mut s = load_scene(&r.gpu, Path::new("scene.glb"), &r.mat_bgl, &r.g_bgl, &r.white, &r.smp, &o);
    let c = cam(&eye_target, lens);
    // (0 or less: the default bar)
    let stop = if bar > 0.0 { bake::Stop::Quality(bar) } else { bake::Stop::DEFAULT };
    let l = bake::live_start(&mut r, &mut s, &o, stop, &c).await;
    PROGRESS.with(|p| *p.borrow_mut() = None);
    Ok(Viewer(Rc::new(RefCell::new(Inner { r, s, o, l, surface }))))
}

#[wasm_bindgen]
impl Viewer {
    pub fn stats(&self) -> String { self.0.borrow().l.stats() }
    /// one frame
    pub fn draw(&self, eye_target: Vec<f32>, lens: f32) -> Result<(), JsValue> {
        let mut g = self.0.borrow_mut(); let i = &mut *g;
        let tex = i.surface.get_current_texture();
        let t = match tex { wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t, _ => return Ok(()) };
        bake::live_draw(&mut i.l, &i.r, &i.s, &i.o, &cam(&eye_target, lens), &t.texture);
        i.r.gpu.q.present(t);
        Ok(())
    }
    /// expose again for where the camera now stands
    pub fn expose(&self, eye_target: Vec<f32>, lens: f32) -> js_sys::Promise {
        let me = self.0.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let mut g = me.borrow_mut(); let i = &mut *g;
            let c = cam(&eye_target, lens);
            bake::live_expose(&mut i.l, &i.r, &i.s, &i.o, &c).await;
            Ok(JsValue::NULL)
        })
    }
}
