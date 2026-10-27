//! indoorgen: Rust indoor scene generator derived from Infinigen Indoors. The code lives in the workspace crates under crates/;
//! this root crate re-exports them under the historical module paths (indoorgen::assets, ::gn, ::furn, ...) and holds the pipeline.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
extern crate self as indoorgen;
pub use ig_core::*;
pub use ig_exbool as exbool;
pub use ig_bm as bm;
pub use ig_gn as gn;
pub use ig_furn as furn;
pub use ig_rooms as rooms;
pub use ig_assets as assets;
pub use ig_mess as mess;
pub use ig_people as people;
pub use ig_cams as cams;
pub mod budget;
pub mod openings;
pub mod pipeline;
pub mod qc;
pub mod rules;
pub mod scenegen;
pub mod snap;
