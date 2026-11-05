//! The traced renderer in a page, by WebGPU: tools/web_rt/build.sh makes the module. The page hands over a house (glb and sidecar), waits for the bake, then asks for a frame per camera.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

thread_local! { static PROGRESS: RefCell<Option<js_sys::Function>> = RefCell::new(None); }
pub fn progress(f: f32) { PROGRESS.with(|p| { if let Some(cb) = p.borrow().as_ref() { cb.call1(&JsValue::NULL, &JsValue::from_f64(f as f64)).ok(); } }); }

struct Inner { r: Renderer, s: Scene, o: Opts }
thread_local! { static CUR: RefCell<Option<Inner>> = RefCell::new(None); }

fn opts(res: (u32, u32), ss: u32, seed: u32, spp: u32) -> Opts {
    let mut o = parse_args();
    o.rt = Some(rt::Quality { spp, ..rt::Quality::parse("full") }); o.ss = ss; o.res = res; o.seed = seed as u64;
    o
}
fn cam(e: &[f32], lens: f32) -> Cam { cam_look(Vec3::new(e[0], e[1], e[2]), Vec3::new(e[3], e[4], e[5]), lens) }

/// whether this browser can run it
#[wasm_bindgen]
pub async fn supported() -> bool { wgpu::util::is_browser_webgpu_supported().await }

/// forget the house held for rendering
#[wasm_bindgen]
pub fn forget() { CUR.with(|c| *c.borrow_mut() = None); }

/// one picture of the house from `eye_target` (ex,ey,ez,tx,ty,tz, z up), `lens` in mm on 36 mm film: RGBA, width x height. `glb_bytes` and `sidecar` are used once per house and size
#[wasm_bindgen]
pub async fn render_view(width: u32, height: u32, ss: u32, glb_bytes: Vec<u8>, sidecar: String, seed: u32, eye_target: Vec<f32>, lens: f32, denoise: Option<js_sys::Function>) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();
    let held = CUR.with(|c| c.borrow_mut().take()).filter(|i| i.o.res == (width, height) && i.o.ss == ss);
    let mut i = match held {
        Some(i) => i,
        None => {
            glb::FILES.with(|f| { let mut f = f.borrow_mut(); f.clear(); f.insert("scene.glb".into(), glb_bytes); f.insert("scene.json".into(), sidecar.into_bytes()); });
            // (without the page's denoiser, many more rays: with few the a-trous filter leaves clouds)
            let o = opts((width, height), ss, seed, if denoise.is_some() { 4 } else { 16 });
            let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let r = init_with(&inst, None, &o).await;
            let s = load_scene(&r.gpu, Path::new("scene.glb"), &r.mat_bgl, &r.g_bgl, &r.white, &r.smp, &o);
            glb::FILES.with(|f| f.borrow_mut().clear()); // (a house's bytes are tens of MB, and the scene has them now)
            Inner { r, s, o }
        }
    };
    let c = cam(&eye_target, lens);
    let img = { let Inner { r, s, o } = &mut i; exposed_web(r, s, &c, o, denoise.as_ref()).await };
    CUR.with(|h| *h.borrow_mut() = Some(i));
    Ok(img.into_raw())
}
