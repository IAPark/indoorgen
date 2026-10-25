//! Furniture states on the solved layout (owner: mess-states worker).
use crate::MessCfg;
use ig_furn::state::State;

/// Mutates poses / per-object state flags of the solved scene before visuals are built. No-op when `!cfg.on()`.
pub fn apply(_state: &mut State, cfg: &MessCfg) {
    if !cfg.on() {
        return;
    }
}
