//! Deterministic HashMap/HashSet (fixed SipHash keys): iteration order is reproducible run to run,
//! which the exact-placement and mesh-order checks rely on. Use these instead of std's RandomState maps.
use std::collections::hash_map::DefaultHasher;
use std::hash::BuildHasherDefault;
pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<DefaultHasher>>;
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<DefaultHasher>>;
