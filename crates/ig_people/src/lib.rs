//! People (beyond Infinigen): photoreal rigged avatars (Microsoft Rocketbox, MIT) posed in Rust and placed after the
//! furniture solve. `gen2 ... --people N` (0 = default = unchanged output).
//! - `body`  (owner: people-body worker): avatar library loading (FBX via assimp), skinning, pose library, materials.
//! - `place` (owner: people-place worker): choosing spots + poses from the solved State, collision checks.
//! The contract between them is `PoseKind`, `PoseInfo`, `pose_info` and `build_person` below.
pub mod avatar;
pub mod body;
pub mod poses;
pub mod rig;
pub mod place;
pub mod pworld;
pub mod ragdoll;
pub mod cover;
pub mod audit;
pub mod pillows;

pub use ig_core::*;
pub use ig_core::mesh::V3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PoseKind {
    SitUpright,
    SitRelaxed,
    SitBedEdge,
    Stand,
    LeanCounter,
    StandAtSink,
    StandAtDesk,
    LieBed,
    LieSofa,
    Walk,
    Reach,
    Crouch,
}

pub const ALL_POSES: [PoseKind; 12] = [
    PoseKind::SitUpright, PoseKind::SitRelaxed, PoseKind::SitBedEdge, PoseKind::Stand, PoseKind::LeanCounter, PoseKind::StandAtSink,
    PoseKind::StandAtDesk, PoseKind::LieBed, PoseKind::LieSofa, PoseKind::Walk, PoseKind::Reach, PoseKind::Crouch,
];

/// Geometry of a posed avatar in its local frame: z up, facing +y, origin on the floor between the feet
/// (for sitting / lying poses: origin below the seat / hip contact, on the floor).
#[derive(Clone, Debug)]
pub struct PoseInfo {
    pub kind: PoseKind,
    /// hip/seat contact point (sitting, lying), to be put on a seat or mattress top
    pub seat: Option<V3>,
    /// feet soles (center points); None when the feet are off the floor (lying)
    pub feet: Option<[V3; 2]>,
    /// hand contact points (leaning on a counter, reaching, at a desk / sink)
    pub hands: Option<[V3; 2]>,
    /// local bounding box of the posed body
    pub lo: V3,
    pub hi: V3,
}

/// Number of avatars available in the library (0 until the body worker lands it).
pub fn avatar_count() -> usize {
    body::avatar_count()
}

/// Posed geometry facts for placement (cheap; no mesh build).
pub fn pose_info(avatar: usize, kind: PoseKind) -> PoseInfo {
    body::pose_info(avatar, kind)
}

/// Full posed, textured avatar mesh in its local frame (parts carry material specs for render_rust.py).
pub fn build_person(avatar: usize, kind: PoseKind, seed: u64) -> ig_kit::api::Asset {
    body::build_person(avatar, kind, seed)
}

/// Sitting poses adapted to a seat of the given height (additive to the contract): legs are solved so the seat contact is at `seat_z`
/// and the feet stay flat on the floor. Other poses ignore `seat_z`.
pub fn pose_info_seat(avatar: usize, kind: PoseKind, seat_z: f64) -> PoseInfo {
    body::pose_info_seat(avatar, kind, seat_z)
}
pub fn build_person_seat(avatar: usize, kind: PoseKind, seed: u64, seat_z: Option<f64>) -> ig_kit::api::Asset {
    body::build_person_seat(avatar, kind, seed, seat_z)
}
