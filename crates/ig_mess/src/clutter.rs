//! Clutter objects on floors and surfaces (owner: mess-clutter worker).
use crate::MessCfg;
use ig_core::mesh::Scene;
use ig_furn::state::State;

/// Adds clutter nodes (and cloth jobs) to the scene after the furniture visuals. No-op when `!cfg.on()`.
pub fn apply(_scene: &mut Scene, _state: &State, _visuals: &std::collections::HashMap<String, ig_kit::api::Asset>, cfg: &MessCfg) {
    if !cfg.on() {
        return;
    }
}
