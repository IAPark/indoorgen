//! ig_rooms: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
pub use crate as rooms;
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub use ig_core::*;
pub use ig_geom::*;
pub use ig_bm as bm;
pub use ig_exbool as exbool;
pub mod consts;
pub mod graph;
pub mod plan;
pub mod plan_rules;
pub mod nudge;
pub mod score;
pub mod segment;
pub mod state;
pub mod solidify;
pub mod shell;
pub mod skirting;
pub mod walldeco;
pub mod pyroom;
pub mod pysolid;

