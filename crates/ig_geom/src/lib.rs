//! ig_geom: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
/// old `crate::furn::geom` paths
pub mod furn {
    pub use crate::{geom, tags};
}
pub use ig_core::*;
pub mod geom;
pub mod tags;
