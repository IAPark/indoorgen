//! Mess pass (beyond Infinigen): lived-in disorder added AFTER the furniture solve, so `--mess 0` (default) keeps the
//! solver output and every existing check (lockstep, phcheck, refcmp) untouched.
//! - `states`: furniture states on the solved layout (chairs pulled out/turned, doors/drawers ajar, unmade beds, curtains,
//!   crooked rugs). Runs before the visual assets are built.
//! - `clutter`: extra objects dropped on floors and surfaces (clothes, books, dishes, ...), then settled.
//! - `settle`: rigid-body settling of clutter (rapier) so nothing floats or intersects.
//! All randomness comes from `MessCfg::rng(stream)` so results are deterministic per (scene seed, level, stream).
pub mod check;
pub mod clutter;
pub mod garment;
pub mod settle;
pub mod doors;
pub mod states;
pub mod visuals;

pub use ig_core::*;

#[derive(Clone, Debug)]
pub struct MessCfg {
    /// 0 = off (exact current output), 1 = very messy
    pub level: f64,
    pub seed: u64,
    /// multipliers of the floor / surface clutter counts (1 = as tuned)
    pub floor_scale: f64,
    pub surface_scale: f64,
}

impl MessCfg {
    pub fn on(&self) -> bool {
        self.level > 0.0
    }
    /// independent deterministic stream per purpose (e.g. "states", "clutter.floor")
    pub fn rng(&self, stream: &str) -> ig_core::rng::Rng {
        ig_core::rng::Rng::seeded(ig_core::int_hash(&[&self.seed.to_string(), "mess", stream]))
    }
}
