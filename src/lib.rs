//! infinigen-rs: Rust port of Infinigen Indoors scene generation. The code lives in the workspace crates under crates/;
//! this root crate re-exports them under the historical module paths (infinigen::assets, ::gn, ::furn, ...) and holds the pipeline.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
pub use ig_core::*;
pub use ig_exbool as exbool;
pub use ig_bm as bm;
pub use ig_gn as gn;
pub use ig_furn as furn;
pub use ig_rooms as rooms;
pub use ig_assets as assets;
pub mod openings;
pub mod pipeline;
