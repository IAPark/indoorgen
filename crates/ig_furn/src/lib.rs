//! ig_furn: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
pub use crate as furn;
pub use ig_core::*;
pub use ig_geom::*;
pub use ig_kit::*;
pub mod domain;
pub mod relations;
pub mod program;
pub mod state;
pub mod col;
pub mod layout2d;
pub mod layout_rules;
pub mod sizes;
pub mod groups;
pub mod fcl;
pub mod mpr;
pub mod eval;
pub mod dof;
pub mod solver;
pub mod assets;

