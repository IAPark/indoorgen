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
pub use ig_features as feature_kit;
pub mod budget;
pub mod config;
pub mod features;
pub mod rules;
pub mod openings;
pub mod pipeline;
#[cfg(not(target_arch = "wasm32"))]
pub mod snapshot;
pub mod layout_dump;
pub mod qc;
pub mod invariants;
pub mod scenegen;
#[cfg(not(target_arch = "wasm32"))]
pub mod gen2cli;
pub mod realism;
pub mod ceiling_decor;
pub mod soft_decor;
pub mod gallery_wall;
pub use ig_outdoor as outdoor;
pub use ig_outdoor::exterior;
pub mod bath_decor;
pub mod top_dress;
pub mod kitchen_window;
pub mod table_setting;
pub mod dress;
pub mod bed_decor;
pub mod shelffill;
pub mod snap;
pub mod variation;
