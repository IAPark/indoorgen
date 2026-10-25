//! Avatar library (Microsoft Rocketbox, loaded once from FBX via assimp into `avatar.bin`), pose library and posed-mesh builder.
//!
//! Frame of every PoseInfo / built mesh: z up, facing +y, +x = the person's right, floor at z = 0.
//! - standing poses: origin on the floor midway between the soles
//! - sitting poses: origin on the floor below the seat contact patch (`seat` = lowest point of the buttocks/thighs, i.e. the seat-top height;
//!   the feet are on z = 0, so a chair of that height fits the pose)
//! - lying poses: body on its back (or side for LieSofa), head towards -y, face up; lowest point of the body on z = 0 (`seat.z` = 0), origin below the hip
//! - `feet`: sole centers [left(-x), right(+x)]; `hands`: palm-area centers [left, right] (None unless the pose uses them)
use crate::avatar::{self, Avatar, PartKind};
use crate::poses::*;
use crate::rig::*;
use crate::{PoseInfo, PoseKind};
use ig_core::mesh::{Material, Mesh};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

fn names() -> &'static Vec<String> {
    static N: OnceLock<Vec<String>> = OnceLock::new();
    N.get_or_init(avatar::list_avatars)
}

pub fn avatar_count() -> usize {
    names().len()
}

pub fn avatar_name(i: usize) -> Option<&'static str> {
    names().get(i).map(|s| s.as_str())
}

pub fn avatar_of(i: usize) -> Option<Arc<Avatar>> {
    static C: OnceLock<Mutex<HashMap<usize, Arc<Avatar>>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(a) = c.lock().unwrap().get(&i) {
        return Some(a.clone());
    }
    let a = Arc::new(avatar::load(names().get(i)?)?);
    c.lock().unwrap().insert(i, a.clone());
    Some(a)
}

/// a skinned pose, shifted into the contract frame
pub struct Posed {
    pub info: PoseInfo,
    pub parts: Vec<(Vec<[f32; 3]>, Vec<[f32; 3]>)>,
}

pub fn pose_avatar(av: &Avatar, kind: PoseKind, seed: u64) -> Posed {
    pose_avatar_seat(av, kind, seed, None)
}

/// like `pose_avatar`; with `seat_z` (sitting poses only) the legs are solved by IK so that the seat contact lies at that height with the
/// feet flat on the floor (chairs / beds / sofas of different heights)
pub fn pose_avatar_seat(av: &Avatar, kind: PoseKind, seed: u64, seat_z: Option<f64>) -> Posed {
    pose_avatar_seat_g(av, kind, seed, seat_z).0
}

/// like `pose_avatar_seat`, plus the global bone transforms of the key pose in the contract frame (rest point -> contract frame)
pub fn pose_avatar_seat_g(av: &Avatar, kind: PoseKind, seed: u64, seat_z: Option<f64>) -> (Posed, Vec<Xf>) {
    use PoseKind::*;
    // default seat heights: dining/office chair, sofa, bed edge
    let seat_z = match kind {
        SitUpright => Some(seat_z.unwrap_or(0.45)),
        SitRelaxed => Some(seat_z.unwrap_or(0.42)),
        SitBedEdge => Some(seat_z.unwrap_or(0.50)),
        _ => None,
    };
    let Some(target) = seat_z else { return pose_once(av, kind, seed, None) };
    let mut r = 0.0;
    let mut p = pose_once(av, kind, seed, Some(r));
    for _ in 0..4 {
        let err = target - p.0.info.seat.map(|s| s[2]).unwrap_or(target);
        if err.abs() < 2e-3 {
            break;
        }
        r += err;
        p = pose_once(av, kind, seed, Some(r));
    }
    p
}

fn pose_once(av: &Avatar, kind: PoseKind, seed: u64, root_z: Option<f64>) -> (Posed, Vec<Xf>) {
    let rig = Rig::new(av);
    let mut org = [0.0; 3];
    let mut out = None;
    for _pass in 0..3 {
        let def = make_def(&rig, kind, seed, org, root_z);
        let g = rig.eval(&def.ops, def.root_t);
        let skin: Vec<_> = av.parts.iter().map(|p| skin_part(p, &g)).collect();
        let hip = g[rig.id("Bip01 Pelvis")].apply(rig.rest(rig.id("Bip01 Pelvis")));
        let m = measure(av, &rig, &skin, hip);
        let (ox, oy, oz) = match def.anchor {
            Anchor::Feet => ((m.sole[0][0] + m.sole[1][0]) * 0.5, (m.sole[0][1] + m.sole[1][1]) * 0.5, m.zfoot),
            Anchor::Seat => (m.seat[0], m.seat[1], m.zfoot),
            Anchor::Lie => (m.seat[0], m.seat[1], m.zmin),
        };
        let sh = [ox, oy, oz];
        let again = (0..3).any(|k| (sh[k] - org[k]).abs() > 1e-4) && def.hands;
        org = sh;
        if again && _pass < 2 {
            continue;
        }
        let rel = |p: [f64; 3]| [p[0] - ox, p[1] - oy, p[2] - oz];
        let info = PoseInfo {
            kind,
            seat: match def.anchor {
                Anchor::Feet => None,
                _ => Some(rel(m.seat)),
            },
            feet: match def.anchor {
                Anchor::Lie => None,
                _ => Some([rel(m.sole[0]), rel(m.sole[1])]),
            },
            hands: if def.hands { Some([rel(m.hands[0]), rel(m.hands[1])]) } else { None },
            lo: rel(m.lo),
            hi: rel(m.hi),
        };
        let parts = skin
            .into_iter()
            .map(|(p, n)| (p.into_iter().map(|q| [(q[0] as f64 - ox) as f32, (q[1] as f64 - oy) as f32, (q[2] as f64 - oz) as f32]).collect(), n))
            .collect();
        let gl: Vec<Xf> = g.iter().map(|x| Xf { r: x.r, t: [x.t[0] - ox, x.t[1] - oy, x.t[2] - oz] }).collect();
        out = Some((Posed { info, parts }, gl));
        break;
    }
    out.expect("pose converged")
}

/// Posed geometry facts for placement (cached per avatar and pose; computed from the neutral, variation-free pose).
pub fn pose_info(avatar: usize, kind: PoseKind) -> PoseInfo {
    static C: OnceLock<Mutex<HashMap<(usize, PoseKind), PoseInfo>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(i) = c.lock().unwrap().get(&(avatar, kind)) {
        return i.clone();
    }
    let info = match avatar_of(avatar) {
        Some(av) => {
            let mut i = pose_avatar(&av, kind, 0).info;
            // seeded variation moves head / torso / free arms a little: keep the box a bit generous
            for k in 0..3 {
                i.lo[k] -= 0.04;
                i.hi[k] += 0.04;
            }
            i
        }
        None => fallback_info(kind),
    };
    c.lock().unwrap().insert((avatar, kind), info.clone());
    info
}

/// pose info for a sitting pose adapted to a seat of height `seat_z` (see `pose_avatar_seat`)
pub fn pose_info_seat(avatar: usize, kind: PoseKind, seat_z: f64) -> PoseInfo {
    match avatar_of(avatar) {
        Some(av) => {
            let mut i = pose_avatar_seat(&av, kind, 0, Some(seat_z)).info;
            for k in 0..3 {
                i.lo[k] -= 0.04;
                i.hi[k] += 0.04;
            }
            i
        }
        None => pose_info(avatar, kind),
    }
}

/// rough adult proportions, used when no avatar library is installed
fn fallback_info(kind: PoseKind) -> PoseInfo {
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

fn spec(cls: &str, av: &Avatar, p: &avatar::AvPart) -> serde_json::Value {
    let tex = |f: &str| if f.is_empty() { serde_json::Value::Null } else { serde_json::json!(av.dir.join("Textures").join(f).to_string_lossy()) };
    serde_json::json!({"cls": cls, "kwargs": {"color": tex(&p.color), "normal": tex(&p.normal), "spec": tex(&p.spec), "avatar": av.name}})
}

/// skinned parts of an avatar for explicit global bone transforms (contract frame)
pub fn skin_parts(av: &Avatar, g: &[Xf]) -> Vec<(Vec<[f32; 3]>, Vec<[f32; 3]>)> {
    av.parts.iter().map(|p| skin_part(p, g)).collect()
}

/// Full posed, textured avatar mesh in the contract frame. Parts: body (skin + clothes), head, hair/opacity cards.
pub fn build_person(avatar: usize, kind: PoseKind, seed: u64) -> ig_kit::api::Asset {
    build_person_seat(avatar, kind, seed, None)
}

/// `build_person` with the seat height adapted (sitting poses; None = natural pose)
pub fn build_person_seat(avatar: usize, kind: PoseKind, seed: u64, seat_z: Option<f64>) -> ig_kit::api::Asset {
    build_person_g(avatar, kind, seed, seat_z, None)
}

/// `build_person_seat` with explicit (settled) global bone transforms instead of the key pose
pub fn build_person_g(avatar: usize, kind: PoseKind, seed: u64, seat_z: Option<f64>, g: Option<&[Xf]>) -> ig_kit::api::Asset {
    let mut asset = ig_kit::api::Asset::default();
    let Some(av) = avatar_of(avatar) else { return asset };
    let mut posed = pose_avatar_seat(&av, kind, seed, seat_z);
    if let Some(g) = g {
        posed.parts = skin_parts(&av, g);
    }
    for (pi, p) in av.parts.iter().enumerate() {
        let cls = match p.kind {
            PartKind::Body => "people.cloth",
            PartKind::Head => "people.skin",
            PartKind::Hair => "people.hair",
        };
        let mut m = Mesh::new();
        m.pos = posed.parts[pi].0.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
        m.uv = p.uv.iter().map(|u| [u[0] as f64, 1.0 - u[1] as f64]).collect();
        m.faces = p.tris.iter().map(|t| vec![t[0], t[1], t[2]]).collect();
        let mat = Material {
            spec: Some(spec(cls, &av, p)),
            name: format!("{}_{}", av.name, p.name),
            color: [0.6, 0.5, 0.45, 1.0],
            roughness: 0.6,
            metallic: 0.0,
            emissive: [0.0; 3],
            emissive_strength: 0.0,
        };
        asset.parts.push(ig_kit::api::Part { mesh: m, mat, col: None });
    }
    asset
}
