//! ig_storage: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
// names the old assets/mod.rs imported privately (asset modules use them through `use super::*`)
use ig_core::mesh::*;
use ig_core::rng::Rng;
use ig_core::int_hash;
use ig_geom::geom::*;
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub use ig_core::*;
pub use ig_exbool as exbool;
pub use ig_bm as bm;
pub use ig_geom::*;
pub use ig_kit::*;
pub use ig_gn as gn;
pub use ig_rooms as rooms;
pub use ig_shared::*;
pub mod appliances;
pub mod aquarium;
pub mod bathfree;
pub mod bathroom;
pub mod bathsink;
pub mod bathtub;
pub mod bevelgn;
pub mod cabdoor;
pub mod cookware;
pub mod gnports;
pub mod matpick;
pub mod ksink;
pub mod pangeom;
pub mod potbars;
pub mod shelves;
pub mod taps;
pub mod toiletx;
pub mod uvlab;
pub mod tvx;
