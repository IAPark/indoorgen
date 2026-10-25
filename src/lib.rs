#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
pub mod dhash;
pub mod pyhash;
pub mod bl32;
pub mod assets;
pub mod furn;
pub mod materials;
pub mod mesh;
pub mod openings;
pub mod pipeline;
pub mod pyset;
pub mod rng;
pub mod rooms;
pub mod shp;
pub mod subdiv;
pub mod vdb;
pub mod osd;

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
pub mod gn;
pub mod bm;

pub mod exbool;
