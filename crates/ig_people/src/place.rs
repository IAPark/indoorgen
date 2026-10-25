//! Choosing spots and poses for people in the solved scene (owner: people-place worker).
use crate::PoseKind;
use ig_furn::state::State;

#[derive(Clone, Debug)]
pub struct Placement {
    pub avatar: usize,
    pub kind: PoseKind,
    /// local -> world (z rotation + translation)
    pub pose: ig_core::mesh::Mat4,
    pub seed: u64,
}

/// Up to `n` people for the scene (0 -> empty). Deterministic per seed.
pub fn place(_state: &State, _room_polys: &[(String, ig_core::shp::G)], n: usize, _seed: u64) -> Vec<Placement> {
    if n == 0 {
        return vec![];
    }
    vec![]
}
