//! Runtime switches that the native build reads from environment variables. wasm32 has no environment: the host sets them here.
use std::sync::atomic::{AtomicBool, Ordering};

static FORCE_DENSE: AtomicBool = AtomicBool::new(false);

/// force dense furnishing mode on (what `INFINIGEN_DENSE=1` does natively); call before generating anything
pub fn set_dense(on: bool) {
    FORCE_DENSE.store(on, Ordering::Relaxed);
}

/// `INFINIGEN_DENSE` is set (or forced with `set_dense`)
pub fn dense_env() -> bool {
    FORCE_DENSE.load(Ordering::Relaxed) || std::env::var_os("INFINIGEN_DENSE").is_some()
}

static PROGRESS: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();

/// install the progress sink (the wasm module forwards it to a JS callback); first call wins
pub fn set_progress_hook(f: fn(&str)) {
    let _ = PROGRESS.set(f);
}

/// stage / progress message of the pipeline (no-op without a hook)
pub fn progress(msg: &str) {
    if let Some(f) = PROGRESS.get() {
        f(msg);
    }
}

static FAST_STABLE: AtomicBool = AtomicBool::new(false);

/// use the convex half-plane fast path in the stability test (always on for wasm32; `FAST_STABLE=1` natively)
pub fn set_fast_stable(on: bool) {
    FAST_STABLE.store(on, Ordering::Relaxed);
}
pub fn fast_stable() -> bool {
    cfg!(target_arch = "wasm32") || FAST_STABLE.load(Ordering::Relaxed) || std::env::var_os("FAST_STABLE").is_some()
}

static OVERRIDES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// set a tunable that the native build reads from the environment (wasm has none): `var` consults these first
pub fn set_var(k: &str, v: &str) {
    let mut o = OVERRIDES.lock().unwrap();
    o.retain(|e| e.0 != k);
    o.push((k.to_string(), v.to_string()));
}

/// `std::env::var(k).ok()` plus the overrides installed with `set_var`
pub fn var(k: &str) -> Option<String> {
    if let Some(e) = OVERRIDES.lock().unwrap().iter().find(|e| e.0 == k) {
        return Some(e.1.clone());
    }
    std::env::var(k).ok()
}

/// `std::env::var_os(k).is_some()`-style check that also sees the `set_var` overrides (value as a String)
pub fn var_os(k: &str) -> Option<String> {
    var(k)
}

/// drop-in for `std::env::var(k)` (Result) that also sees the `set_var` overrides
pub fn var_r(k: &str) -> Result<String, ()> {
    var(k).ok_or(())
}

/// remove every `set_var` override (the wasm host calls this before applying a new run's settings)
pub fn clear_vars() {
    OVERRIDES.lock().unwrap().clear();
}
