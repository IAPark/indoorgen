//! ig_core: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub mod bl32;
pub mod envx;
pub mod dhash;
pub mod materials;
pub mod mesh;
pub mod osd;
pub mod polyfill;
pub mod pyhash;
pub mod pyset;
pub mod rng;
pub mod shp;
pub mod subdiv;
pub mod vdb;

/// infinigen int_hash((a, b)) = abs(int(md5(str(a)+str(b)), 16)) % (2^32 - 1)
pub fn int_hash(parts: &[&str]) -> u64 {
    let mut ctx = md5::Context::new();
    for p in parts {
        ctx.consume(p.as_bytes());
    }
    let d = ctx.compute();
    let v = u128::from_be_bytes(d.0);
    (v % ((1u128 << 32) - 1)) as u64
}
