//! ig_kit: part of the infinigen-rs workspace (see crates/README.md).
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
pub mod api;
pub mod piece;
pub mod bevel;
pub mod bevel3;
pub mod bevelfull;
pub mod bevmod;
pub mod blops;
pub mod bpymod;
pub mod bpyops;
pub mod bpyx;
pub mod bridge;
pub mod cabnodes;
pub mod cloth;
pub mod vbd;
pub mod clothes;
pub mod coil;
pub mod ctor_draws;
pub mod curvekit;
pub mod dualcon;
pub mod fontdata;
pub mod geomkit;
pub mod gnlite;
pub mod gweld;
pub mod legacynoise;
pub mod legacynoise_tables;
pub mod legtex;
pub mod matdraw;
pub mod prismcut;
pub mod pydump;
pub mod solid;
pub mod solidnm;
pub mod solidx;
pub mod stagecmp;
pub mod stagefile;
pub mod tableparts;
pub mod textdraw;
pub use api::*;
pub use ctor_draws::{set_unseeded, unseeded};
