//! Deterministic HashMap/HashSet (fixed SipHash keys): iteration order is reproducible run to run,
//! which the exact-placement and mesh-order checks rely on. Use these instead of std's RandomState maps.
use std::collections::hash_map::DefaultHasher;
use std::hash::BuildHasherDefault;
pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<DefaultHasher>>;
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<DefaultHasher>>;

/// FxHash (rustc's hasher): deterministic and several times faster than SipHash for integer keys. Its iteration order differs from
/// [`HashMap`]'s, so switch a map only where it is used for lookups (never iterated), or the output changes.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);
const FX_K: u64 = 0xf135_7aea_2e62_a9c5;
impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(FX_K);
    }
}
impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut c = bytes.chunks_exact(8);
        for w in &mut c { self.add(u64::from_le_bytes(w.try_into().unwrap())) }
        let r = c.remainder();
        if !r.is_empty() {
            let mut b = [0u8; 8];
            b[..r.len()].copy_from_slice(r);
            self.add(u64::from_le_bytes(b) ^ ((r.len() as u64) << 56));
        }
    }
    #[inline] fn write_u8(&mut self, i: u8) { self.add(i as u64) }
    #[inline] fn write_u16(&mut self, i: u16) { self.add(i as u64) }
    #[inline] fn write_u32(&mut self, i: u32) { self.add(i as u64) }
    #[inline] fn write_u64(&mut self, i: u64) { self.add(i) }
    #[inline] fn write_usize(&mut self, i: usize) { self.add(i as u64) }
    #[inline] fn write_i32(&mut self, i: i32) { self.add(i as u32 as u64) }
    #[inline] fn write_i64(&mut self, i: i64) { self.add(i as u64) }
    #[inline]
    fn finish(&self) -> u64 {
        // final avalanche so low bits (hashbrown's bucket index) depend on every input bit
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^ (h >> 33)
    }
}
/// lookup-only maps on hot paths (see [`FxHasher`])
pub type FxMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type FxSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;
