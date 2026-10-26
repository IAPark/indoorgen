//! ig_exbool: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
pub use crate as exbool;
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub use ig_core::*;
// Faithful port of Blender 4.2's EXACT mesh boolean (BLI mesh_intersect / mesh_boolean), vertex/face/edge order exact.
// Developed and verified in a local boolean-port test bench (51 Blender fixtures, all exact).
pub mod boolean;
pub mod cdt_q;
pub mod exact_boolean;
pub mod hashtab;
pub mod imesh;
pub mod intersect;
pub mod libcxx_sort;
pub mod mpq;
pub mod polyfill;

