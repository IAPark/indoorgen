//! The unified layout solver. One search places every piece of a room from rules that are data (`rules/*.json`): the solver names no class.
//!
//!   house.rs   the room as the floor plan gives it: outline, walls, doors, windows
//!   world.rs   the voxel world: a 64-layer column per 5 cm floor cell, one channel per kind of occupancy; the exact prism tests; the walk check
//!   rules.rs   the rule file
//!   solve.rs   beam search over the program; every rejection has a named reason
//!
//! `lay` (src/bin/lay.rs) is the loop: one room in milliseconds, rule edits without a rebuild, JSON for tools/layview.py.
pub mod house;
pub mod rules;
pub mod solve;
pub mod world;
