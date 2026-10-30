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
use indoorgen::config::GenConfig;
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

/// Legacy flat options (kept for backwards compatibility): mess (number), cloth (bool), cams (number), dense, only_target,
/// cloth_steps, cloth_cg, steps_scale, env. They are mapped onto the config; the rest of the JSON is a (partial) GenConfig.
fn config_from_opts(opts_json: &str) -> Result<GenConfig, String> {
    let mut cfg = GenConfig::web_default();
    if opts_json.trim().is_empty() { return Ok(cfg); }
    let mut j: serde_json::Value = serde_json::from_str(opts_json).map_err(|e| e.to_string())?;
    let Some(o) = j.as_object_mut() else { return Err("options must be a JSON object".into()) };
    if let Some(v) = o.get("mess").and_then(|v| v.as_f64()) { cfg.mess.level = v; o.remove("mess"); }
    if let Some(v) = o.get("cloth").and_then(|v| v.as_bool()) { cfg.cloth.on = v; o.remove("cloth"); }
    if let Some(v) = o.get("cams").and_then(|v| v.as_u64()) { cfg.cameras.count = v as usize; o.remove("cams"); }
    if let Some(v) = o.remove("dense").and_then(|v| v.as_bool()) { cfg.program.dense = v; }
    if let Some(v) = o.remove("only_target").and_then(|v| v.as_bool()) { cfg.house.only_target = v; }
    if let Some(v) = o.remove("cloth_steps").and_then(|v| v.as_u64()) { cfg.cloth.steps = v as u32; }
    if let Some(v) = o.remove("cloth_cg").and_then(|v| v.as_u64()) { cfg.cloth.cg_iters = v as usize; }
    if let Some(v) = o.remove("steps_scale").and_then(|v| v.as_f64()) { cfg.solver.steps_scale = v; }
    if let Some(serde_json::Value::Object(env)) = o.remove("env") {
        // flags: true / "1" set them, false / null leave them unset; known names map onto fields, the rest stay tunables
        let get = |k: &str| -> Option<String> {
            match env.get(k)? {
                serde_json::Value::Bool(false) | serde_json::Value::Null => None,
                serde_json::Value::Bool(true) => Some("1".into()),
                serde_json::Value::String(t) => Some(t.clone()),
                other => Some(other.to_string()),
            }
        };
        cfg.apply_vars(&get);
        for (k, v) in env { cfg.advanced.env.insert(k, v); }
    }
    GenConfig::from_partial(&cfg, &j)
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
/// `opts_json`: a (partial) GenConfig, e.g. `{"mess":{"level":0.5},"cloth":{"on":true}}`; the legacy flat keys `{"mess":0.5,"cloth":true,"cams":4,"dense":true,...}` still work (all optional, defaults = `default_config()`). The sidecar json is available from `last_scene_json()`.
#[wasm_bindgen]
pub fn generate(seed: u32, room: &str, opts_json: &str) -> Result<Vec<u8>, JsValue> {
    let mut cfg = config_from_opts(opts_json).map_err(|e| JsValue::from_str(&format!("bad options: {e}")))?;
    cfg.house.seed = seed as u64;
    cfg.house.room = if room.is_empty() { None } else { Some(room.to_string()) };
    cfg.people.count = 0;
    cfg.output = Default::default();
    ig_core::envx::clear_vars();
    let orig_prog = !cfg.program.dense;
    let prog: serde_json::Value = serde_json::from_str(&unpack(if orig_prog { PROGRAM_ORIG } else { PROGRAM_DENSE })).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    let mut fallback = TableAssets::default();
    fallback.add_str(&unpack(BBOX_TABLE));
    fallback.add_str(&unpack(SAMPLES));
    let room_opt = if room.is_empty() { None } else { Some(room.to_string()) };
    let (cams_k, room_opt) = (cfg.cameras.count, cfg.house.room.clone());
    let built = build_scene(&Params { cfg, prog, fallback });
    ig_core::envx::progress("writing glb");
    let mut glb: Vec<u8> = Vec::new();
    let mut side = built.scene.write_glb_to(|| Ok(&mut glb)).map_err(|e| JsValue::from_str(&format!("glb: {e}")))?;
    if cams_k > 0 {
        ig_core::envx::progress("cameras");
        let cams = indoorgen::cams::generate(&built.scene, &built.room_polys, &indoorgen::cams::Cfg { k: cams_k, seed: seed as u64, wall_t: built.wall_t, target: room_opt });
        side["cameras"] = serde_json::Value::Array(cams);
    }
    built.merge_sidecar(&mut side);
    LAST_JSON.with(|j| *j.borrow_mut() = side.to_string());
    ig_core::envx::progress("done");
    Ok(glb)
}

/// Furniture layout of house `seed` as JSON (furn layout dump: rooms with outlines, inner floors and walkway status, door / window
/// cutters, objects with footprints, front, parent, room, size, seeds). `opts_json` (all optional):
/// `{"knobs": {"rules":true,"sizes":true,"builtin_fit":true,"groups":true,"furniture_only":true,"group_w":3,"size_w":0.5},
///   "rooms": "curated" | "fewer" | "full", ...any partial GenConfig}`
#[wasm_bindgen]
pub fn layout(seed: u32, opts_json: &str) -> Result<String, JsValue> {
    use indoorgen::furn::layout2d::knobs;
    let mut j: serde_json::Value = if opts_json.trim().is_empty() { serde_json::json!({}) } else { serde_json::from_str(opts_json).map_err(|e| JsValue::from_str(&format!("bad options: {e}")))? };
    let o = j.as_object_mut().ok_or_else(|| JsValue::from_str("options must be a JSON object"))?;
    let kj = o.remove("knobs").unwrap_or(serde_json::json!({}));
    let fo_given = kj.get("furniture_only").is_some();
    let mut k: knobs::Knobs = serde_json::from_value(kj).map_err(|e| JsValue::from_str(&format!("bad knobs: {e}")))?;
    // the layout page stops after the furniture stages unless asked for the small items too
    if !fo_given { k.furniture_only = true; }
    let rooms = o.remove("rooms").and_then(|v| v.as_str().map(|s| s.to_string()));
    knobs::set(&k);
    let mut cfg = config_from_opts(&j.to_string()).map_err(|e| JsValue::from_str(&format!("bad options: {e}")))?;
    cfg.house.seed = seed as u64;
    cfg.house.room = Some("LivingRoom".into());
    cfg.house.only_target = false;
    // the same annealing steps as native runs (web_default uses 0.6) unless the caller asks for fewer
    cfg.solver.steps_scale = j.get("steps_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    if let Some(r) = rooms { cfg.house.rooms = r; }
    cfg.people.count = 0;
    cfg.mess.level = 0.0;
    cfg.cloth.on = false;
    cfg.cameras.count = 0;
    cfg.output = Default::default();
    ig_core::envx::clear_vars();
    let prog: serde_json::Value = serde_json::from_str(&unpack(PROGRAM_DENSE)).map_err(|e| JsValue::from_str(&format!("program: {e}")))?;
    let mut fallback = TableAssets::default();
    fallback.add_str(&unpack(BBOX_TABLE));
    fallback.add_str(&unpack(SAMPLES));
    let t0 = web_time::Instant::now();
    let mut v = indoorgen::scenegen::layout_only(&Params { cfg, prog, fallback });
    v["ms"] = serde_json::json!(t0.elapsed().as_secs_f64() * 1000.0);
    Ok(v.to_string())
}

/// JSON Schema (draft-07) of the generation config: sections, field types, ranges, descriptions
#[wasm_bindgen]
pub fn config_schema() -> String {
    GenConfig::schema().to_string()
}

/// the default config of the browser build as JSON (fast web preset); `generate` merges its options over it
#[wasm_bindgen]
pub fn default_config() -> String {
    serde_json::to_string(&GenConfig::web_default()).unwrap()
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
