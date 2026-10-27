//! Browser build of the scene generator: one furnished room as GLB bytes.
//!
//! ```js
//! import init, { generate, last_scene_json, set_progress } from "./pkg/ig_wasm.js";
//! await init();
//! set_progress((msg) => console.log(msg));
//! const glb = generate(5, "Bedroom", '{"mess":0.5,"cloth":true,"cams":4,"dense":true}');   // Uint8Array
//! const side = JSON.parse(last_scene_json());   // nodes, lights, cloth jobs, cameras
//! ```
//! Pipeline = `indoorgen::scenegen::build_scene` (the same code as `gen2 SEED ROOM --only-target`), data files embedded.
use indoorgen::furn::assets::TableAssets;
use indoorgen::scenegen::{build_scene, Params};
use serde::Deserialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

// raw-deflate packed copies of data/*.json (tools/wasm/pack_data.sh): 14 MB of json -> 0.8 MB in the module
static PROGRAM_DENSE: &[u8] = include_bytes!("../../../data/wasm/home_program_dense.json.deflate");
static PROGRAM_ORIG: &[u8] = include_bytes!("../../../data/wasm/home_program.json.deflate");
static BBOX_TABLE: &[u8] = include_bytes!("../../../data/wasm/asset_bbox_table.json.deflate");
static SAMPLES: &[u8] = include_bytes!("../../../data/wasm/asset_samples.json.deflate");

fn unpack(z: &[u8]) -> String {
    String::from_utf8(miniz_oxide::inflate::decompress_to_vec(z).expect("inflate")).expect("utf8")
}

// single-threaded wasm: talc is much faster than the default dlmalloc for the many small allocations of the solver
#[cfg(feature = "talc")]
#[global_allocator]
static ALLOC: talc::wasm::WasmDynamicTalc = talc::wasm::new_wasm_dynamic_allocator();

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

thread_local! {
    static LAST_JSON: RefCell<String> = RefCell::new(String::new());
    static PROGRESS: RefCell<Option<js_sys::Function>> = RefCell::new(None);
}

fn progress_hook(msg: &str) {
    PROGRESS.with(|p| {
        if let Some(f) = p.borrow().as_ref() {
            let _ = f.call1(&JsValue::NULL, &JsValue::from_str(msg));
        }
    });
}

#[derive(Deserialize)]
#[serde(default)]
struct Opts {
    /// lived-in disorder 0..1 (clutter, open doors, moved furniture)
    mess: f64,
    /// run the Rust cloth simulation for bed linen
    cloth: bool,
    /// photographer-like camera poses written to the sidecar json
    cams: usize,
    /// dense furnishing program (false: the program exactly as exported from python, fewer objects)
    dense: bool,
    /// furnish only the first room of the requested type (the only mode the browser build ships)
    only_target: bool,
    /// web preset defaults: cloth substeps per frame 1 (native 2) and CG iterations 20 (native 40): faster, looser drape
    cloth_steps: u32,
    cloth_cg: u32,
    /// web preset default 0.6: multiplier for the solver steps (dense program: 300 large / 200 medium / 50 small); 1.0 = gen2 quality (about 1.4x slower, ~25% more objects)
    steps_scale: f64,
    /// extra tunables by their native environment-variable name (STEPS_L, PLACE_TRIES, TRINKETS_MAX, MESH_BUDGET_SCALE, NO_SNAP, ...),
    /// applied after the options above so they win; cleared before every run
    env: std::collections::BTreeMap<String, serde_json::Value>,
    /// furniture rules (indoorgen::rules::RuleSpec: per room type class / relation / min / max / prefer); absent = built-in rules
    rules: Option<indoorgen::rules::RuleSpec>,
}
impl Default for Opts {
    fn default() -> Self {
        Opts { mess: 0.5, cloth: true, cams: 4, dense: true, only_target: true, cloth_steps: 1, cloth_cg: 20, steps_scale: 0.6, env: Default::default(), rules: None }
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| error(&format!("ig_wasm panic: {info}"))));
    ig_core::envx::set_progress_hook(progress_hook);
}

/// callback `(message: string) => void` called at every pipeline stage
#[wasm_bindgen]
pub fn set_progress(f: Option<js_sys::Function>) {
    PROGRESS.with(|p| *p.borrow_mut() = f);
}

/// Generate the scene of `seed` and return the GLB bytes of room type `room` ("Bedroom", "Kitchen", "LivingRoom", ...).
/// `opts_json`: `{"mess":0.5,"cloth":true,"cams":4,"dense":true}` (all optional). The sidecar json is available from `last_scene_json()`.
#[wasm_bindgen]
pub fn generate(seed: u32, room: &str, opts_json: &str) -> Result<Vec<u8>, JsValue> {
    let opts: Opts = if opts_json.trim().is_empty() { Opts::default() } else { serde_json::from_str(opts_json).map_err(|e| JsValue::from_str(&format!("bad options: {e}")))? };
    indoorgen::assets::cloth::SKIP_JOB_SIM.store(!opts.cloth, std::sync::atomic::Ordering::Relaxed);
    ig_core::envx::clear_vars();
    ig_core::envx::set_var("CL_STEPS", &opts.cloth_steps.to_string());
    ig_core::envx::set_var("CL_CG", &opts.cloth_cg.to_string());
    let (dl, dm, ds) = if opts.dense { (300.0, 200.0, 50.0) } else { (100.0, 40.0, 5.0) };
    for (k, d) in [("STEPS_L", dl), ("STEPS_M", dm), ("STEPS_S", ds)] {
        ig_core::envx::set_var(k, &(((d * opts.steps_scale).round() as usize).max(1)).to_string());
    }
    for (k, v) in &opts.env {
        // flags: true / "1" set them, false / null leave them unset
        match v {
            serde_json::Value::Bool(false) | serde_json::Value::Null => {}
            serde_json::Value::Bool(true) => ig_core::envx::set_var(k, "1"),
            serde_json::Value::String(t) => ig_core::envx::set_var(k, t),
            other => ig_core::envx::set_var(k, &other.to_string()),
        }
    }
    let orig_prog = !opts.dense;
    let mut prog: serde_json::Value = serde_json::from_str(&unpack(if orig_prog { PROGRAM_ORIG } else { PROGRAM_DENSE })).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    if let Some(r) = &opts.rules {
        indoorgen::rules::try_apply(&mut prog, r).map_err(|e| JsValue::from_str(&format!("rules: {}", e.join("; "))))?;
    }
    let mut fallback = TableAssets::default();
    fallback.add_str(&unpack(BBOX_TABLE));
    fallback.add_str(&unpack(SAMPLES));
    let room_opt = if room.is_empty() { None } else { Some(room.to_string()) };
    let p = Params { seed: seed as u64, room: room_opt.clone(), only_target: opts.only_target, mess_level: opts.mess, people_n: 0, qc_out: None, no_glb: false, orig_prog, prog, fallback };
    let built = build_scene(&p);
    ig_core::envx::progress("writing glb");
    let mut glb: Vec<u8> = Vec::new();
    let mut side = built.scene.write_glb_to(|| Ok(&mut glb)).map_err(|e| JsValue::from_str(&format!("glb: {e}")))?;
    if opts.cams > 0 {
        ig_core::envx::progress("cameras");
        let cams = indoorgen::cams::generate(&built.scene, &built.room_polys, &indoorgen::cams::Cfg { k: opts.cams, seed: seed as u64, wall_t: built.wall_t, target: room_opt });
        side["cameras"] = serde_json::Value::Array(cams);
    }
    LAST_JSON.with(|j| *j.borrow_mut() = side.to_string());
    ig_core::envx::progress("done");
    Ok(glb)
}

/// JSON for the rules editor: `{rooms, classes:{factories,tags}, relations, defaults: RuleSpec, constraint_names}` (dense program)
#[wasm_bindgen]
pub fn rules_info() -> Result<String, JsValue> {
    let prog: serde_json::Value = serde_json::from_str(&unpack(PROGRAM_DENSE)).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    Ok(indoorgen::rules::rules_info(&prog).to_string())
}

/// the built-in rules as a RuleSpec JSON string
#[wasm_bindgen]
pub fn default_rules() -> Result<String, JsValue> {
    let prog: serde_json::Value = serde_json::from_str(&unpack(PROGRAM_DENSE)).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    Ok(serde_json::to_string(&indoorgen::rules::default_rules(&prog)).unwrap())
}

/// validate a RuleSpec JSON string: returns "" when ok, else the error messages (one per line)
#[wasm_bindgen]
pub fn validate_rules(spec_json: &str) -> Result<String, JsValue> {
    let spec = match indoorgen::rules::parse(spec_json) { Ok(s) => s, Err(e) => return Ok(e) };
    let prog: serde_json::Value = serde_json::from_str(&unpack(PROGRAM_DENSE)).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    Ok(indoorgen::rules::validate(&spec, &prog).join("\n"))
}

/// the sidecar json (nodes, lights, cloth jobs, cameras) of the last `generate` call
#[wasm_bindgen]
pub fn last_scene_json() -> String {
    LAST_JSON.with(|j| j.borrow().clone())
}

// meshoptimizer (C++) needs operator new / delete; there is no C++ runtime on wasm32-unknown-unknown, so back them with the Rust allocator
// (16-byte header keeps the size for the delete).
#[no_mangle]
pub unsafe extern "C" fn _Znwm(size: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align_unchecked(size + 16, 16);
    let p = std::alloc::alloc(layout);
    if p.is_null() {
        std::process::abort();
    }
    *(p as *mut usize) = size;
    p.add(16)
}
#[no_mangle]
pub unsafe extern "C" fn _ZdlPv(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let p = ptr.sub(16);
    let size = *(p as *mut usize);
    std::alloc::dealloc(p, std::alloc::Layout::from_size_align_unchecked(size + 16, 16));
}
