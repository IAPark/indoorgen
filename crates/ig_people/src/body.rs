//! Avatar library, skinning, poses, materials (owner: people-body worker). Stubs until implemented.
use crate::{PoseInfo, PoseKind};

pub fn avatar_count() -> usize {
    0
}

/// rough adult proportions so placement can be developed before the real library lands
pub fn pose_info(_avatar: usize, kind: PoseKind) -> PoseInfo {
    use PoseKind::*;
    let stand = |hands| PoseInfo { kind, seat: None, feet: Some([[-0.1, 0.0, 0.0], [0.1, 0.0, 0.0]]), hands, lo: [-0.3, -0.2, 0.0], hi: [0.3, 0.2, 1.72] };
    match kind {
        SitUpright | SitRelaxed | SitBedEdge => PoseInfo { kind, seat: Some([0.0, -0.05, 0.46]), feet: Some([[-0.12, 0.35, 0.0], [0.12, 0.35, 0.0]]), hands: None, lo: [-0.3, -0.25, 0.0], hi: [0.3, 0.55, 1.3] },
        LieBed | LieSofa => PoseInfo { kind, seat: Some([0.0, 0.0, 0.1]), feet: None, hands: None, lo: [-0.3, -0.95, 0.0], hi: [0.3, 0.85, 0.3] },
        LeanCounter | StandAtSink | StandAtDesk | Reach => stand(Some([[-0.2, 0.35, 0.92], [0.2, 0.35, 0.92]])),
        Crouch => PoseInfo { kind, seat: None, feet: Some([[-0.12, 0.0, 0.0], [0.12, 0.0, 0.0]]), hands: None, lo: [-0.3, -0.3, 0.0], hi: [0.3, 0.4, 1.0] },
        Stand | Walk => stand(None),
    }
}

pub fn build_person(_avatar: usize, _kind: PoseKind, _seed: u64) -> ig_kit::api::Asset {
    ig_kit::api::Asset::default()
}
