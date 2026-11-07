//! Choosing spots and poses for people in the solved scene (owner: people-place worker).
//!
//! `place_ctx` builds a `World` (furniture colliders from the visual meshes, room polygons, doors), enumerates *slots* (a seat, a mattress,
//! the front of a desk / sink / counter / shelf, free floor), picks them with room-type weights and social bonuses, and validates every pick
//! with a collision test of the posed body (oriented boxes derived from `PoseInfo`) against furniture, walls and the people placed before.
use crate::pworld::*;
use crate::{pose_info, PoseInfo, PoseKind};
use ig_core::mesh::{Mat4, Mesh, V3};
use ig_core::rng::Rng;
use ig_furn::state::State;

/// what the checks measured for one person (metres)
#[derive(Clone, Debug, Default)]
pub struct PlaceInfo {
    pub room: String,
    pub support: Option<String>,
    pub pen_furn: f64,
    pub pen_wall: f64,
    pub pen_people: f64,
    /// lowest foot sole above (+) / below (-) the floor, for poses with feet on the floor
    pub feet_err: f64,
    /// hip above (+) / sunk into (-) the seat or mattress top
    pub seat_gap: f64,
    /// hand height minus work surface height
    pub hand_err: f64,
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub avatar: usize,
    pub kind: PoseKind,
    /// local -> world (z rotation + translation)
    pub pose: Mat4,
    pub seed: u64,
    /// body collision boxes in the local frame (lo, hi): the occupied space of the person
    pub boxes: Vec<(V3, V3)>,
    pub info: PlaceInfo,
    /// seat height above the floor the legs were solved for (sitting poses): pass to `build_person_seat`
    pub seat_z: Option<f64>,
    /// how far the support (soft seat / mattress) was lowered for the physics (the body sinks in)
    pub sink: f64,
    /// cloth interaction (bed cover variants, throw blanket)
    pub cover: Cover,
    /// world height of the support surface under the hips the person was placed on (mattress top for `Under`, cover top for `On`)
    pub support_top: f64,
    /// ragdoll result (None = key pose)
    pub settled: Option<crate::ragdoll::Settled>,
    /// pillows (part of the support object) pushed aside / under the head: (support key, part index, world offset)
    pub pillow_moves: Vec<(String, usize, V3)>,
}

/// How a person interacts with cloth (decided at placement; applied by `crate::cover`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    None,
    /// lying on a bed under the comforter: the cover is re-simulated above the body (arms out, head out)
    Under,
    /// lying on top of the (rumpled) cover
    On,
    /// a throw blanket draped over the legs / body (sofa)
    Throw,
}

impl Placement {
    /// the posed, textured person in the local frame (settled ragdoll when present, else the key pose)
    pub fn build(&self) -> ig_kit::api::Asset {
        crate::body::build_person_g(self.avatar, self.kind, self.seed, self.seat_z, self.settled.as_ref().map(|s| &s.g[..]))
    }
    pub fn world_boxes(&self) -> Vec<WBox> {
        world_boxes(&self.boxes, &self.pose)
    }
    /// world-space meshes of the occupied boxes inflated by `pad`: keep-out volumes for the clutter pass
    pub fn keepout(&self, pad: f64) -> Vec<Mesh> {
        self.boxes
            .iter()
            .map(|(lo, hi)| {
                let mut m = Mesh::cuboid([lo[0] - pad, lo[1] - pad, lo[2] - pad], [hi[0] + pad, hi[1] + pad, hi[2] + pad]);
                m.transform(&self.pose);
                m
            })
            .collect()
    }
    /// simple stand-in mesh in the local frame (PEOPLE_PROXY=1): the body boxes plus a head
    pub fn proxy(&self) -> ig_kit::api::Asset {
        let mut a = ig_kit::api::Asset::default();
        let mut body = Mesh::new();
        for (lo, hi) in &self.boxes {
            body.append(&Mesh::cuboid(*lo, *hi));
        }
        let rgb = match self.kind {
            PoseKind::SitUpright | PoseKind::SitRelaxed | PoseKind::SitBedEdge => [0.2, 0.35, 0.7],
            PoseKind::LieBed | PoseKind::LieSofa => [0.6, 0.3, 0.55],
            PoseKind::StandAtDesk | PoseKind::StandAtSink | PoseKind::LeanCounter => [0.75, 0.5, 0.2],
            _ => [0.3, 0.6, 0.35],
        };
        a.parts.push(ig_kit::api::Part { mesh: body, mat: ig_core::mesh::Material::diffuse("person_proxy_body", rgb, 0.8), col: None });
        // head: a small box on top of the highest box, towards the front
        if let Some((lo, hi)) = self.boxes.iter().max_by(|a, b| a.1[2].partial_cmp(&b.1[2]).unwrap()) {
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, hi[2]];
            let head = Mesh::cuboid([c[0] - 0.09, c[1] - 0.1, c[2]], [c[0] + 0.09, c[1] + 0.1, c[2] + 0.22]);
            a.parts.push(ig_kit::api::Part { mesh: head, mat: ig_core::mesh::Material::diffuse("person_proxy_head", [0.85, 0.65, 0.5], 0.7), col: None });
        }
        a
    }
}

pub struct Ctx<'a> {
    pub visuals: Option<&'a std::collections::HashMap<String, ig_kit::api::Asset>>,
    pub floor_z: f64,
    pub wall_t: f64,
    /// extra keep-out boxes (centre, half extents, yaw), e.g. the swept area of room doors; people stay clear of them
    pub keepout: &'a [([f64; 3], [f64; 3], f64)],
    /// only these rooms take people: a room type ("living-room") or a room name prefix ("living-room_0"); empty = every room
    pub rooms: &'a [String],
}

/// Up to `n` people for the scene (0 -> empty). Deterministic per seed. Without visuals the solver placeholders are the colliders.
pub fn place(state: &State, room_polys: &[(String, ig_core::shp::G)], n: usize, seed: u64) -> Vec<Placement> {
    place_ctx(state, room_polys, n, seed, &Ctx { visuals: None, floor_z: 0.135, wall_t: 0.27, keepout: &[], rooms: &[] })
}

fn yaw_of(f: [f64; 2]) -> f64 {
    (-f[0]).atan2(f[1])
}

fn rot2(v: [f64; 2], yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

fn pose_mat(origin: [f64; 2], z: f64, yaw: f64) -> Mat4 {
    let (s, c) = yaw.sin_cos();
    Mat4([[c, -s, 0.0, origin[0]], [s, c, 0.0, origin[1]], [0.0, 0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]])
}

fn world_boxes(boxes: &[(V3, V3)], pose: &Mat4) -> Vec<WBox> {
    let yaw = pose.0[1][0].atan2(pose.0[0][0]);
    boxes
        .iter()
        .map(|(lo, hi)| {
            let c = pose.apply([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]);
            WBox { c, h: [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0, (hi[2] - lo[2]) / 2.0], yaw }
        })
        .collect()
}

/// the body as a few boxes (local frame), leaving out the regions that touch the support (seat, work surface)
pub fn body_boxes(pi: &PoseInfo) -> Vec<(V3, V3)> {
    use PoseKind::*;
    let (lo, hi) = (pi.lo, pi.hi);
    match pi.kind {
        SitUpright | SitRelaxed | SitBedEdge => {
            let s = pi.seat.unwrap_or([0.0, 0.0, 0.46]);
            let mut v = vec![
                ([lo[0], lo[1], s[2] + 0.10], [hi[0], s[1] + 0.09, hi[2]]),
                ([lo[0] * 0.7, s[1] - 0.1, s[2] + 0.03], [hi[0] * 0.7, pi.feet.map(|f| (f[0][1] + f[1][1]) / 2.0 + 0.05).unwrap_or(hi[1]).min(hi[1]), s[2] + 0.15]),
            ];
            if let Some(f) = pi.feet {
                let fy = (f[0][1] + f[1][1]) / 2.0;
                v.push(([-0.26_f64.max(lo[0]), fy - 0.09, 0.04], [0.26_f64.min(hi[0]), fy + 0.09, s[2] - 0.02]));
            }
            v
        }
        LieBed | LieSofa => {
            let s = pi.seat.map(|s| s[2]).unwrap_or(0.1);
            vec![([lo[0], lo[1], (s - 0.06).max(lo[2]) + 0.03], hi)]
        }
        LeanCounter | StandAtSink | StandAtDesk => {
            let hy = pi.hands.map(|h| (h[0][1] + h[1][1]) / 2.0).unwrap_or(hi[1]);
            vec![([lo[0], lo[1], lo[2] + 0.04], [hi[0], hi[1].min(hy - 0.12), hi[2]])]
        }
        _ => vec![([lo[0], lo[1], lo[2] + 0.04], hi)],
    }
}

#[derive(Clone)]
struct Slot {
    kind: PoseKind,
    room: usize,
    /// sits: point on the backrest front face (backless: seat centre); lies: hip target; hand poses: surface edge; others: origin
    anchor: [f64; 2],
    /// sits: seat front direction; lies: direction of the head; hand poses: towards the surface; others: facing
    face: [f64; 2],
    support: Option<usize>,
    /// seat / mattress top, or work surface height
    top: f64,
    w: f64,
    /// sits: seat depth (backrest face to front edge)
    depth: f64,
    soft: bool,
    has_back: bool,
    group: usize,
}

struct Planner<'a> {
    w: &'a World,
    placed: Vec<Placement>,
    placed_boxes: Vec<Vec<WBox>>,
    placed_slot: Vec<Slot>,
    why: std::cell::Cell<u32>,
}

impl Planner<'_> {
    fn no(&self, code: u32) -> Option<Placement> {
        self.why.set(code);
        None
    }

    /// cloth variant of a slot and the support top height it implies (bed: mattress top when the person goes under the cover)
    fn cover_variant(&self, s: &Slot, seed: u64) -> (Cover, f64) {
        use PoseKind::*;
        let force = std::env::var("PEOPLE_COVER").ok();
        let h = (seed.wrapping_mul(0x9E3779B97F4A7C15) >> 40) % 100;
        match s.kind {
            LieBed => {
                if let Some(oi) = s.support {
                    let o = &self.w.objs[oi];
                    if !o.cover.is_empty() && force.as_deref() != Some("none") {
                        let under = match force.as_deref() {
                            Some("under") => true,
                            Some("on") => false,
                            _ => h < 50,
                        };
                        if under {
                            if let Some(t) = o.top_xy_nocover(s.anchor[0], s.anchor[1], self.w.floor_z + 1.0) {
                                return (Cover::Under, t);
                            }
                        } else {
                            let t = o.column(s.anchor[0], s.anchor[1]).into_iter().filter(|z| *z <= self.w.floor_z + 1.0).last().unwrap_or(s.top);
                            return (Cover::On, t);
                        }
                    }
                }
                (Cover::None, s.top)
            }
            LieSofa | SitRelaxed => {
                let want = match force.as_deref() {
                    Some("throw") => true,
                    Some(_) => false,
                    None => h < if s.kind == LieSofa { 70 } else { 35 },
                };
                (if want && s.soft { Cover::Throw } else { Cover::None }, s.top)
            }
            _ => (Cover::None, s.top),
        }
    }

    /// pose + collision check of one slot with one avatar; None when it does not fit
    fn fit(&self, s: &Slot, avatar: usize, seed: u64, tol: f64) -> Option<Placement> {
        use PoseKind::*;
        let fl = self.w.floor_z;
        let sitting = matches!(s.kind, SitUpright | SitRelaxed | SitBedEdge);
        let sink = if s.soft { 0.04 } else { 0.0 };
        let seat_rel = s.top - fl - sink;
        if sitting && !(0.30..=0.70).contains(&seat_rel) {
            return self.no(11);
        }
        let seat_z = if sitting { Some(seat_rel) } else { None };
        // the sitter's own seat is skipped by the collision test, so a pulled-out footrest / chaise in front of the seat would go
        // through the shins: no upright sitting where the support has a surface below seat height where the lower legs hang
        if matches!(s.kind, SitUpright | SitRelaxed) {
            if let Some(oi) = s.support {
                let o = &self.w.objs[oi];
                for d in [0.55, 0.65] {
                    let (x, y) = (s.anchor[0] + s.face[0] * d, s.anchor[1] + s.face[1] * d);
                    if o.column(x, y).iter().any(|z| *z > fl + 0.15 && *z < s.top - 0.12) {
                        return self.no(14);
                    }
                }
            }
        }
        let (cover, top) = self.cover_variant(s, seed);
        // arms-out variant of the lying pose (flag bit read by the pose library)
        let mut seed = if cover == Cover::Under { seed | (1 << 20) } else { seed };
        if s.kind == LieSofa {
            // sofa shorter than a stretched-out person: knees up (seed bit 21 read by the pose library)
            if let Some(oi) = s.support {
                let (llo, lhi) = local_extents(&self.w.objs[oi]);
                if (lhi[1] - llo[1] < 2.05 || std::env::var_os("PEOPLE_KNEES").is_some()) && seed % 4 != 3 {
                    seed |= 1 << 21;
                }
            }
        }
        let pi = match seat_z {
            Some(z) => crate::pose_info_seat(avatar, s.kind, z),
            None => pose_info(avatar, s.kind),
        };
        // lying: slot.face is the head direction, the avatar's head is towards -y
        let yaw = if matches!(s.kind, LieBed | LieSofa) { yaw_of([-s.face[0], -s.face[1]]) } else { yaw_of(s.face) };
        let mut hand_err = 0.0;
        let (origin, z, seat_gap);
        match s.kind {
            SitUpright | SitRelaxed | SitBedEdge => {
                let Some(seat) = pi.seat else { return self.no(90) };
                let back = (seat[1] - pi.lo[1]) - if s.soft { 0.03 } else { 0.04 };
                if s.has_back && s.depth < back + 0.12 {
                    return self.no(1);
                }
                let off = if s.has_back { back } else { 0.0 };
                let hip = [s.anchor[0] + s.face[0] * off, s.anchor[1] + s.face[1] * off];
                let o = rot2([seat[0], seat[1]], yaw);
                origin = [hip[0] - o[0], hip[1] - o[1]];
                z = fl;
                seat_gap = seat[2] + fl - s.top;
            }
            LieBed | LieSofa => {
                let Some(seat) = pi.seat else { return self.no(90) };
                let o = rot2([seat[0], seat[1]], yaw);
                origin = [s.anchor[0] - o[0], s.anchor[1] - o[1]];
                z = top - seat[2] - 0.02;
                seat_gap = -0.02;
            }
            LeanCounter | StandAtSink | StandAtDesk => {
                let Some(h) = pi.hands else { return self.no(91) };
                let hy = (h[0][1] + h[1][1]) / 2.0;
                let hz = (h[0][2] + h[1][2]) / 2.0;
                hand_err = hz - (s.top - fl);
                if hand_err.abs() > hand_tol() {
                    return self.no(4);
                }
                origin = [s.anchor[0] - s.face[0] * (hy - 0.08), s.anchor[1] - s.face[1] * (hy - 0.08)];
                z = fl;
                seat_gap = 0.0;
            }
            Reach => {
                let d = pi.hands.map(|h| (h[0][1] + h[1][1]) / 2.0 - 0.1).unwrap_or(0.5);
                origin = [s.anchor[0] - s.face[0] * d, s.anchor[1] - s.face[1] * d];
                z = fl;
                seat_gap = 0.0;
            }
            Crouch => {
                let d = pi.hi[1] + 0.12;
                origin = [s.anchor[0] - s.face[0] * d, s.anchor[1] - s.face[1] * d];
                z = fl;
                seat_gap = 0.0;
            }
            _ => {
                origin = s.anchor;
                z = fl;
                seat_gap = 0.0;
            }
        }
        let pose = pose_mat(origin, z, yaw);
        if s.kind == Crouch && (self.w.rooms[s.room].ty == "bathroom" || self.w.objs.iter().any(|o| o.class.contains("Toilet") && (o.lo[0] - 1.2..o.hi[0] + 1.2).contains(&origin[0]) && (o.lo[1] - 1.2..o.hi[1] + 1.2).contains(&origin[1]))) {
            return self.no(13); // no crouching next to toilets
        }
        if s.kind == Reach {
            // a real target at hand height: the support's surface within 15 cm of the raised hand, and the support tall enough to hold it
            let (Some(h), Some(si)) = (pi.hands, s.support) else { return self.no(14) };
            let ob = &self.w.objs[si];
            let hw = h.map(|q| pose.apply(q));
            let hz = hw[0][2].max(hw[1][2]);
            if ob.hi[2] < hz - 0.25 || ob.hi[2] - fl < 1.4 {
                return self.no(14);
            }
            let near = |q: V3| ob.tris.iter().filter(|t| (0..3).all(|k| q[k] > t[0][k].min(t[1][k]).min(t[2][k]) - 0.2 && q[k] < t[0][k].max(t[1][k]).max(t[2][k]) + 0.2)).any(|t| crate::audit::pt_tri_dist(q, t) < 0.15);
            if !(near(hw[0]) || near(hw[1])) {
                return self.no(14);
            }
        }
        let asset = crate::build_person_seat(avatar, s.kind, seed, seat_z);
        let boxes = body_slabs(&asset, &pi);
        let wb = world_boxes(&boxes, &pose);
        let room = &self.w.rooms[s.room];
        if !in_poly(origin, &room.poly) {
            return self.no(5);
        }
        // doors stay clear
        if room.doors.iter().any(|d| (d[0] - origin[0]).hypot(d[1] - origin[1]) < 1.0) {
            return self.no(6);
        }
        let pen_wall = self.w.pen_wall(&wb, s.room);
        if pen_wall > tol {
            return self.no(7);
        }
        if !self.w.extra.is_empty() && pen_boxes(&wb, &self.w.extra) > 0.005 {
            return self.no(12); // inside a door's swing area
        }
        let skip = match s.kind {
            SitUpright | SitRelaxed | SitBedEdge | LieBed | LieSofa => s.support,
            _ => None,
        };
        let pen_furn = self.w.pen_furniture(&wb, skip);
        // crude proxy boxes of the legs overlap table rails / legs a little: sitters get a looser bound until the real meshes are checked
        let tol_f = if matches!(s.kind, SitUpright | SitRelaxed) { tol.max(sit_tol()) } else { tol };
        if pen_furn > tol_f {
            return self.no(8);
        }
        let mut pen_people = 0.0f64;
        for (i, other) in self.placed_boxes.iter().enumerate() {
            pen_people = pen_people.max(pen_boxes(&wb, other));
            // personal space: body origins keep 0.4 m apart (people on one sofa excepted)
            let o = &self.placed[i].pose.0;
            let lie = |k: PoseKind| matches!(k, LieBed | LieSofa);
            let close = (o[0][3] - origin[0]).hypot(o[1][3] - origin[1]) < 0.4;
            if !lie(s.kind) && !lie(self.placed[i].kind) && close && !(s.group == self.placed_slot[i].group && matches!(s.kind, SitUpright | SitRelaxed)) {
                return self.no(9);
            }
        }
        if pen_people > tol.min(0.01) {
            return self.no(10);
        }
        let feet_err = match pi.feet {
            Some(f) if !matches!(s.kind, LieBed | LieSofa) => f[0][2].min(f[1][2]) + z - fl,
            _ => 0.0,
        };
        let info = PlaceInfo { room: room.name.clone(), support: s.support.map(|i| self.w.objs[i].key.clone()), pen_furn, pen_wall, pen_people, feet_err, seat_gap, hand_err };
        let sink = if s.soft { 0.04 } else { 0.0 };
        Some(Placement { avatar, kind: s.kind, pose, seed, boxes, info, seat_z, sink: if matches!(s.kind, LieBed | LieSofa) { 0.02 } else { sink }, cover, support_top: top, settled: None, pillow_moves: vec![] })
    }
}

/// most people placed in one room of this type
fn room_cap(ty: &str) -> usize {
    match ty {
        "bathroom" => 1,
        "bedroom" | "kitchen" => 2,
        _ => 4,
    }
}

fn sit_tol() -> f64 {
    std::env::var("PEOPLE_SIT_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.07)
}

/// the posed body as slab boxes (local frame): 8 slices of the real mesh along z (along y for lying poses). Hands that reach for the
/// support (work surface, shelf) are cut off the slabs so that only the body itself is tested.
pub fn body_slabs(a: &ig_kit::api::Asset, pi: &PoseInfo) -> Vec<(V3, V3)> {
    use PoseKind::*;
    let lying = matches!(pi.kind, LieBed | LieSofa);
    let ax = if lying { 1 } else { 2 };
    let mut pts: Vec<V3> = vec![];
    for p in &a.parts {
        pts.extend(p.mesh.pos.iter().copied());
    }
    if pts.is_empty() {
        return body_boxes(pi);
    }
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in &pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let nb = 8;
    let step = (hi[ax] - lo[ax]) / nb as f64;
    let reach = pi.hands.map(|h| ((h[0][1] + h[1][1]) / 2.0, (h[0][2] + h[1][2]) / 2.0));
    let mut out = vec![];
    for b in 0..nb {
        let (z0, z1) = (lo[ax] + b as f64 * step, lo[ax] + (b + 1) as f64 * step);
        let (mut l, mut h) = ([f64::MAX; 3], [f64::MIN; 3]);
        let mut any = false;
        for p in &pts {
            if p[ax] < z0 || p[ax] > z1 {
                continue;
            }
            if let Some((hy, hz)) = reach {
                if p[1] > hy - 0.12 && p[2] > hz - 0.3 && !lying {
                    continue;
                }
            }
            any = true;
            for k in 0..3 {
                l[k] = l[k].min(p[k]);
                h[k] = h[k].max(p[k]);
            }
        }
        if any {
            out.push((l, h));
        }
    }
    out
}

fn hand_tol() -> f64 {
    std::env::var("PEOPLE_HAND_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.15)
}

fn norm(v: [f64; 2]) -> [f64; 2] {
    let l = v[0].hypot(v[1]).max(1e-9);
    [v[0] / l, v[1] / l]
}

/// object extents in its own frame, from the world triangles (pose = z rotation + translation)
fn local_extents(o: &Obj) -> (V3, V3) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for t in &o.tris {
        for p in t {
            let d = [p[0] - o.pose.t[0], p[1] - o.pose.t[1], p[2] - o.pose.t[2]];
            let r = &o.pose.r;
            let q = [r[0][0] * d[0] + r[1][0] * d[1] + r[2][0] * d[2], r[0][1] * d[0] + r[1][1] * d[1] + r[2][1] * d[2], r[0][2] * d[0] + r[1][2] * d[1] + r[2][2] * d[2]];
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    (lo, hi)
}

/// all slots of the scene (before the social weighting)
fn make_slots(w: &World, rng: &mut Rng) -> Vec<Slot> {
    use PoseKind::*;
    let fl = w.floor_z;
    let mut out: Vec<Slot> = vec![];
    for (oi, o) in w.objs.iter().enumerate() {
        let room = o.room;
        let rty = w.rooms[room].ty.as_str();
        let c = o.class.as_str();
        if !w.blocks(oi) {
            continue;
        }
        let (llo, lhi) = local_extents(o);
        let height = o.hi[2] - fl;
        let front = o.dir([1.0, 0.0]);
        let side = o.dir([0.0, 1.0]);
        let is_arm = c.contains("ArmChair");
        let is_sofa = c.contains("Sofa");
        let is_chair = !is_arm && (c.contains("Chair") || c.contains("Stool"));
        if is_chair || is_arm || is_sofa {
            let Some(seat) = analyze_seat(o, llo, lhi, fl) else { continue };
            if seat.top - fl < 0.28 || seat.top - fl > 0.85 || seat.x1 - seat.x0 < 0.3 {
                continue;
            }
            let (ya, yb) = (seat.ya, seat.yb);
            let span = yb - ya;
            if span < 0.4 {
                continue;
            }
            let nseat = if is_sofa { ((span / 0.62).round() as usize).clamp(1, 4) } else { 1 };
            for i in 0..nseat {
                let ly = ya + (i as f64 + 0.5) * span / nseat as f64;
                let a = o.w([seat.x0, ly, 0.0]);
                let kind = if is_chair || rng.f64() >= 0.7 { SitUpright } else { SitRelaxed };
                let base = match rty {
                    "dining-room" => 3.0,
                    "living-room" => 1.0,
                    "kitchen" => 0.6,
                    "bedroom" => 1.6, // desk / armchair seats: the bedroom pose mix must not be all LieBed
                    _ => 0.8,
                };
                out.push(Slot { kind, room, anchor: [a[0], a[1]], face: front, support: Some(oi), top: seat.top, w: base, depth: seat.x1 - seat.x0, soft: !is_chair, has_back: seat.has_back, group: oi });
            }
            // lying on a sofa: wide enough for a person, along the sofa
            if is_sofa && lhi[1] - llo[1] > 1.75 {
                let ly = 0.5 * (llo[1] + lhi[1]);
                let a = o.w([0.5 * (seat.x0 + seat.x1), ly, 0.0]);
                let head = if rng.f64() < 0.5 { side } else { [-side[0], -side[1]] };
                out.push(Slot { kind: LieSofa, room, anchor: [a[0], a[1]], face: head, support: Some(oi), top: seat.top, w: 0.35, depth: 0.0, soft: true, has_back: false, group: oi });
            }
            continue;
        }
        if c == "BedFactory" {
            let len = lhi[0] - llo[0];
            let wid = lhi[1] - llo[1];
            let Some(top) = o.top_at(0.5 * len, 0.0, fl + 1.0) else { continue };
            let head_dir = o.dir([-1.0, 0.0]);
            // lying along the bed, head near the pillows
            let nl = if wid > 1.5 { 2 } else { 1 };
            for i in 0..nl {
                let ly = if nl == 1 { 0.0 } else if i == 0 { -0.36 } else { 0.36 };
                let a = o.w([llo[0] + 1.15, ly, 0.0]);
                out.push(Slot { kind: LieBed, room, anchor: [a[0], a[1]], face: head_dir, support: Some(oi), top, w: if nl == 1 { 0.45 } else { 0.3 }, depth: 0.0, soft: true, has_back: false, group: oi });
            }
            // sitting on the bed edge, both sides
            for sgn in [-1.0, 1.0] {
                let lx = llo[0] + 0.62 * len;
                let mut y: f64 = 0.0;
                while y.abs() < wid {
                    let ny = y + sgn * 0.02;
                    match o.top_at(lx, ny, fl + 1.0) {
                        Some(t) if (t - top).abs() < 0.12 => y = ny,
                        _ => break,
                    }
                }
                let a = o.w([lx, y - sgn * 0.12, 0.0]);
                let face = norm([side[0] * sgn, side[1] * sgn]);
                if top - fl <= 0.66 {
                    out.push(Slot { kind: SitBedEdge, room, anchor: [a[0], a[1]], face, support: Some(oi), top, w: 1.4, depth: 0.0, soft: true, has_back: false, group: oi });
                }
            }
            continue;
        }
        // work surfaces: front sides, a few positions along the long axis
        let top = o.top_at(0.5 * (llo[0] + lhi[0]), 0.5 * (llo[1] + lhi[1]), fl + 1.4).unwrap_or(o.hi[2]);
        let kind_surface = if c.ends_with("DeskFactory") {
            Some((StandAtDesk, 1.0))
        } else if c.contains("Sink") {
            Some((StandAtSink, 2.5))
        } else if c == "KitchenSpaceFactory" {
            Some((StandAtSink, 2.0))
        } else if c.contains("Cabinet") || c.contains("Oven") || c.contains("Dishwasher") || c.contains("Counter") {
            Some((LeanCounter, if rty == "kitchen" { 1.5 } else { 0.4 }))
        } else {
            None
        };
        if let Some((kind, wt)) = kind_surface {
            let t = top - fl;
            if !(0.6..=1.2).contains(&t) {
                continue;
            }
            let len = lhi[1] - llo[1];
            let np = if len > 1.6 { 3 } else if len > 0.9 { 2 } else { 1 };
            for i in 0..np {
                let ly = if np == 1 { 0.5 * (llo[1] + lhi[1]) } else { llo[1] + 0.3 + (len - 0.6) * (i as f64 + rng.uniform(0.2, 0.8)) / np as f64 };
                for (sgn, wmul) in [(1.0, 1.0), (-1.0, 0.35)] {
                    let lx = if sgn > 0.0 { lhi[0] } else { llo[0] };
                    let a = o.w([lx, ly, 0.0]);
                    let face = norm([-front[0] * sgn, -front[1] * sgn]);
                    out.push(Slot { kind, room, anchor: [a[0], a[1]], face, support: Some(oi), top, w: wt * wmul / np as f64, depth: 0.0, soft: false, has_back: false, group: oi });
                }
            }
            continue;
        }
        // shelves: reach up or crouch down
        if (c.contains("Bookcase") || c.contains("Shelf") || c.contains("TVStand") || c.contains("Dresser") || c.contains("Wardrobe")) && !c.contains("Trinket") {
            let len = lhi[1] - llo[1];
            if len < 0.3 {
                continue;
            }
            let kind = if height > 0.9 {
                Reach
            } else if height > 0.3 && height < 0.85 {
                Crouch
            } else {
                continue;
            };
            let ly = 0.5 * (llo[1] + lhi[1]) + rng.uniform(-0.2, 0.2) * len;
            for (sgn, wmul) in [(1.0, 1.0), (-1.0, 0.2)] {
                let lx = if sgn > 0.0 { lhi[0] } else { llo[0] };
                let a = o.w([lx, ly, 0.0]);
                let face = norm([-front[0] * sgn, -front[1] * sgn]);
                out.push(Slot { kind, room, anchor: [a[0], a[1]], face, support: Some(oi), top, w: if kind == Reach { 0.9 } else { 0.35 } * wmul, depth: 0.0, soft: false, has_back: false, group: oi });
            }
        }
    }
    // free floor: scattered candidates per furnished room
    for (ri, r) in w.rooms.iter().enumerate() {
        if r.n_furn < 2 {
            continue;
        }
        let rw = match r.ty.as_str() {
            "living-room" => 1.4,
            "kitchen" => 0.9,
            "dining-room" => 0.8,
            "bedroom" => 0.8,
            "bathroom" => 0.3,
            _ => 0.6,
        };
        let pts: Vec<usize> = w.objs.iter().enumerate().filter(|(_, o)| o.room == ri && !o.class.contains("Light") && o.hi[2] - fl > 0.4).map(|(i, _)| i).collect();
        let nsamp = 24;
        for _ in 0..nsamp {
            let x = rng.uniform(r.bbox[0], r.bbox[2]);
            let y = rng.uniform(r.bbox[1], r.bbox[3]);
            if !in_poly([x, y], &r.poly) || inside_dist([x, y], &r.poly) < 0.55 {
                continue;
            }
            let look = if !pts.is_empty() && rng.f64() < 0.7 {
                let o = &w.objs[pts[rng.choice_idx(pts.len())]];
                [0.5 * (o.lo[0] + o.hi[0]) - x, 0.5 * (o.lo[1] + o.hi[1]) - y]
            } else {
                let a = rng.uniform(0.0, std::f64::consts::TAU);
                [a.cos(), a.sin()]
            };
            // never stand facing a blank wall: the view must be open (>= 1.6 m to the wall) or end at a piece of furniture; else turn to the room centre
            let look = {
                let f = norm(look);
                let wall = ray_poly([x, y], f, &r.poly);
                let furn = w.objs.iter().filter(|o| o.room == ri && o.hi[2] - fl > 0.4 && !o.class.contains("Light")).filter_map(|o| ray_aabb([x, y], f, [o.lo[0] - 0.1, o.lo[1] - 0.1], [o.hi[0] + 0.1, o.hi[1] + 0.1])).fold(f64::MAX, f64::min);
                if wall >= 1.6 || furn < wall - 0.05 {
                    f
                } else {
                    let cf = norm([0.5 * (r.bbox[0] + r.bbox[2]) - x, 0.5 * (r.bbox[1] + r.bbox[3]) - y]);
                    if ray_poly([x, y], cf, &r.poly) >= 1.6 { cf } else { continue }
                }
            };
            let kind = if rng.f64() < 0.65 { Stand } else { Walk };
            out.push(Slot { kind, room: ri, anchor: [x, y], face: norm(look), support: None, top: fl, w: rw / nsamp as f64 * 2.0, depth: 0.0, soft: false, has_back: false, group: usize::MAX });
        }
    }
    out
}

/// Up to `n` people for the scene using the visual meshes / floor height / wall thickness in `ctx`.
pub fn place_ctx(state: &State, room_polys: &[(String, ig_core::shp::G)], n: usize, seed: u64, ctx: &Ctx) -> Vec<Placement> {
    if n == 0 {
        return vec![];
    }
    let t0 = web_time::Instant::now();
    let mut world = World::new(state, room_polys, ctx.visuals, ctx.floor_z, ctx.wall_t);
    world.extra = ctx.keepout.iter().map(|(c, h, yaw)| crate::pworld::WBox { c: *c, h: *h, yaw: *yaw }).collect();
    if world.rooms.is_empty() {
        return vec![];
    }
    let mut rng = Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "people_place", &n.to_string()]));
    let mut slots = make_slots(&world, &mut rng);
    if !ctx.rooms.is_empty() {
        slots.retain(|s| { let r = &world.rooms[s.room]; ctx.rooms.iter().any(|w| r.ty == *w || r.name.starts_with(w.as_str())) });
    }
    // testing: PEOPLE_FORCE=LieBed,SitRelaxed keeps only slots of those pose kinds
    if let Ok(f) = std::env::var("PEOPLE_FORCE") {
        let want: Vec<&str> = f.split(',').collect();
        slots.retain(|s| want.contains(&format!("{:?}", s.kind).as_str()));
    }
    let dbg = std::env::var_os("PEOPLE_DEBUG").is_some();
    if dbg {
        let mut cnt: std::collections::BTreeMap<String, usize> = Default::default();
        for s in &slots {
            *cnt.entry(format!("{:?}", s.kind)).or_default() += 1;
        }
        eprintln!("PEOPLE slots {cnt:?}");
    }
    let navatar = crate::avatar_count().max(1);
    let mut pl = Planner { w: &world, placed: vec![], placed_boxes: vec![], placed_slot: vec![], why: Default::default() };
    let mut tries = 0;
    while pl.placed.len() < n && !slots.is_empty() && tries < 400 {
        tries += 1;
        // weights with the social layer
        let mut ws: Vec<f64> = Vec::with_capacity(slots.len());
        for s in &slots {
            let mut wgt = s.w;
            // people per room by type: bathrooms hold at most one, bedrooms two
            let in_room = pl.placed_slot.iter().filter(|q| q.room == s.room).count();
            if in_room >= room_cap(&world.rooms[s.room].ty) {
                ws.push(0.0);
                continue;
            }
            for ps in pl.placed_slot.iter() {
                let same_room = ps.room == s.room;
                wgt *= if same_room { 2.2 } else { 0.5 };
                if ps.kind == s.kind {
                    // never two identical poses in one room (people may share a table / sofa)
                    wgt *= if same_room && !matches!(s.kind, PoseKind::SitUpright | PoseKind::SitRelaxed) { 0.0 } else { 0.7 };
                }
                let sitting = |k: PoseKind| matches!(k, PoseKind::SitUpright | PoseKind::SitRelaxed);
                if same_room && sitting(s.kind) && sitting(ps.kind) {
                    let d = [ps.anchor[0] - s.anchor[0], ps.anchor[1] - s.anchor[1]];
                    let dist = d[0].hypot(d[1]);
                    if dist < 3.0 {
                        let u = norm(d);
                        let a = s.face[0] * u[0] + s.face[1] * u[1];
                        let b = -(ps.face[0] * u[0] + ps.face[1] * u[1]);
                        if (a > 0.3 && b > 0.3) || dist < 1.8 {
                            wgt *= 3.5; // facing each other (sofa + armchair, across a table) or side by side
                        }
                    }
                }
                if s.group == ps.group && s.group != usize::MAX {
                    wgt *= if matches!(s.kind, PoseKind::StandAtDesk | PoseKind::StandAtSink | PoseKind::LeanCounter | PoseKind::Reach | PoseKind::Crouch) { 0.15 } else { 1.6 };
                }
            }
            ws.push(wgt * rng.uniform(0.6, 1.0));
        }
        let tot: f64 = ws.iter().sum();
        if tot <= 0.0 {
            break;
        }
        let mut u = rng.f64() * tot;
        let mut idx = ws.len() - 1;
        for (i, w) in ws.iter().enumerate() {
            if u < *w {
                idx = i;
                break;
            }
            u -= w;
        }
        let s = slots.remove(idx);
        let mut avatar = rng.randint(0, navatar as i64) as usize;
        // no identical twins: step to the next avatar not already in the scene
        for _ in 0..navatar {
            if !pl.placed.iter().any(|p| p.avatar == avatar) { break; }
            avatar = (avatar + 1) % navatar;
        }
        let pseed = rng.next_u64() % 1_000_000;
        if let Some(p) = pl.fit(&s, avatar, pseed, 0.012) {
            if dbg {
                eprintln!("PEOPLE pick {:?} room {} support {:?}", s.kind, p.info.room, p.info.support);
            }
            pl.placed_boxes.push(p.world_boxes());
            pl.placed.push(p);
            pl.placed_slot.push(s);
        } else if dbg {
            eprintln!("PEOPLE fail {:?} support {:?} code {}", s.kind, s.support.map(|i| &world.objs[i].key), pl.why.get());
        }
    }
    if crate::ragdoll::enabled() {
        let tr = web_time::Instant::now();
        let mut pl_placed = std::mem::take(&mut pl.placed);
        for p in pl_placed.iter_mut() {
            if crate::ragdoll::wants(p.kind) {
                ragdoll_person(&world, p);
            }
        }
        // settled bodies must not overlap each other (limbs moved): the later person is dropped
        let nb: Vec<Vec<WBox>> = pl_placed.iter().map(|p| p.world_boxes()).collect();
        let mut keep = vec![true; pl_placed.len()];
        for j in 1..pl_placed.len() {
            for i in 0..j {
                if keep[i] && pen_boxes(&nb[i], &nb[j]) > 0.02 {
                    keep[j] = false;
                }
            }
        }
        let mut ki = keep.iter();
        pl_placed.retain(|_| *ki.next().unwrap());
        pl.placed = pl_placed;
        if dbg {
            eprintln!("PEOPLE ragdoll pass {:?}", tr.elapsed());
        }
    }
    if std::env::var_os("PEOPLE_REPORT").is_some() {
        report(&pl.placed, &world, n, t0.elapsed());
    }
    pl.placed
}

/// Pillows of a bed that overlap a lying person: the one nearest to the head slides under the head, the others move to the side.
/// Returns (part index, world offset). `bb` = world AABB of the person, `head` / `hd` = head position and head direction (xy).
fn pillow_plan(o: &Obj, bb: [V3; 2], head: V3, hd: [f64; 2]) -> Vec<(usize, V3)> {
    let n = [-hd[1], hd[0]];
    let mut cand: Vec<(f64, usize, V3, V3)> = vec![];
    for &(a, b, pi) in &o.pillows {
        let (plo, phi) = o.part_bounds(a, b);
        let ov = |k: usize| plo[k] < bb[1][k] - 0.03 && phi[k] > bb[0][k] + 0.03;
        if !(ov(0) && ov(1)) {
            continue;
        }
        let c = [(plo[0] + phi[0]) / 2.0, (plo[1] + phi[1]) / 2.0];
        cand.push((((c[0] - head[0]).hypot(c[1] - head[1])), pi, plo, phi));
    }
    cand.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap().then(x.1.cmp(&y.1)));
    let mut out = vec![];
    for (i, (d, pi, plo, phi)) in cand.iter().enumerate() {
        let c = [(plo[0] + phi[0]) / 2.0, (plo[1] + phi[1]) / 2.0];
        if i == 0 && *d < 0.6 {
            // under the head (a little towards the headboard)
            out.push((*pi, [head[0] + hd[0] * 0.30 - c[0], head[1] + hd[1] * 0.30 - c[1], -0.05]));
            continue;
        }
        let half = [(phi[0] - plo[0]) / 2.0, (phi[1] - plo[1]) / 2.0];
        let hl = n[0].abs() * half[0] + n[1].abs() * half[1];
        let lat = (c[0] - head[0]) * n[0] + (c[1] - head[1]) * n[1];
        let sgn = if lat < 0.0 { -1.0 } else { 1.0 };
        let target = sgn * (0.34 + hl);
        out.push((*pi, [n[0] * (target - lat), n[1] * (target - lat), 0.0]));
    }
    out
}

/// Settle one placed person with the ragdoll against the furniture around it; updates the body boxes.
fn ragdoll_person(w: &World, p: &mut Placement) {
    use crate::ragdoll::*;
    let Some(av) = crate::body::avatar_of(p.avatar) else { return };
    let (posed, g) = crate::body::pose_avatar_seat_g(&av, p.kind, p.seed, p.seat_z);
    // world AABB of the key pose
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for part in &posed.parts {
        for q in &part.0 {
            let wq = p.pose.apply([q[0] as f64, q[1] as f64, q[2] as f64]);
            for k in 0..3 {
                lo[k] = lo[k].min(wq[k]);
                hi[k] = hi[k].max(wq[k]);
            }
        }
    }
    let pad = 0.5;
    let support = p.info.support.clone();
    // key-pose head position and lying direction (world)
    let rig = crate::rig::Rig::new(&av);
    let hb = rig.id("Bip01 Head");
    let hpos = p.pose.apply(g[hb].apply(rig.rest(hb)));
    let hd = { let v = [-p.pose.0[0][1], -p.pose.0[1][1], 0.0]; let l = v[0].hypot(v[1]).max(1e-9); [v[0] / l, v[1] / l] };
    let mut statics: Vec<StaticTris> = vec![];
    for o in &w.objs {
        if !w.blocks_phys(o) || (0..3).any(|k| o.lo[k] > hi[k] + pad || o.hi[k] < lo[k] - pad) {
            continue;
        }
        let is_support = support.as_deref() == Some(o.key.as_str());
        let mut moves: Vec<(usize, V3)> = vec![];
        if is_support && p.kind == PoseKind::LieBed {
            moves = pillow_plan(o, [lo, hi], hpos, hd);
            p.pillow_moves = moves.iter().map(|(pi, d)| (o.key.clone(), *pi, *d)).collect();
        }
        let tris = if is_support { o.tris_with(p.cover == Cover::Under, &moves) } else { o.tris.clone() };
        statics.push(StaticTris { tris, dz: if is_support { p.sink } else { 0.0 } });
    }
    let model = model_of(p.avatar, &av);
    // a body that is flung away (> 0.45 m) or deeply stuck is retried with looser motors; then the key pose stays
    let mut res = None;
    for soft in [1.0, 2.0] {
        let base = if std::env::var_os("RAGDOLL_BASE").is_some() { Opts::default() } else { Opts::for_kind(p.kind) };
        let opt = Opts { soft, ..base };
        if let Some(st) = settle_k(&av, &model, &g, &p.pose, &statics, w.floor_z, 0.03, &opt) {
            // reject: flung away, deeply stuck, torso rotated more than ~30 degrees, hips left the seat / mattress footprint
            if std::env::var_os("RAGDOLL_DBG").is_some() {
                eprintln!("RAGDOLL {:?} soft {} tilt {:.2} twist {:.2} drift {:.2} moved {:.2} pen {:.3}", p.kind, soft, st.tilt, st.twist, st.drift, st.moved, st.pen);
            }
            let tilt_max = match p.kind {
                PoseKind::SitUpright | PoseKind::SitBedEdge => 0.3,
                PoseKind::LieBed | PoseKind::LieSofa => 0.4,
                _ => 0.53,
            };
            let ok = st.moved < 0.6 && st.pen < 0.05 && st.tilt < tilt_max && st.drift < 0.25 && st.twist < 0.5;
            res = Some(st);
            if ok {
                break;
            }
            res = None;
        }
    }
    let Some(st) = res else { return };
    let asset = crate::body::build_person_g(p.avatar, p.kind, p.seed, p.seat_z, Some(&st.g));
    let pi = match p.seat_z {
        Some(z) => crate::pose_info_seat(p.avatar, p.kind, z),
        None => pose_info(p.avatar, p.kind),
    };
    p.boxes = body_slabs(&asset, &pi);
    p.info.pen_furn = p.info.pen_furn.max(st.pen);
    p.settled = Some(st);
}

/// one line per person + a summary, recomputed from scratch on the final placements
fn report(placed: &[Placement], w: &World, n: usize, dt: std::time::Duration) {
    use PoseKind::*;
    let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
    let (mut mf, mut mw, mut mp, mut mfe, mut mh) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let boxes: Vec<Vec<WBox>> = placed.iter().map(|p| p.world_boxes()).collect();
    for (i, p) in placed.iter().enumerate() {
        let skip = match p.kind {
            SitUpright | SitRelaxed | SitBedEdge | LieBed | LieSofa => p.info.support.as_ref().and_then(|k| w.objs.iter().position(|o| &o.key == k)),
            _ => None,
        };
        let room = w.rooms.iter().position(|r| r.name == p.info.room).unwrap_or(0);
        let pf = w.pen_furniture(&boxes[i], skip);
        let pw = w.pen_wall(&boxes[i], room).max(0.0);
        let mut pp = 0.0f64;
        for (j, b) in boxes.iter().enumerate() {
            if j != i {
                pp = pp.max(pen_boxes(&boxes[i], b));
            }
        }
        *kinds.entry(format!("{:?}", p.kind)).or_default() += 1;
        mf = mf.max(pf);
        mw = mw.max(pw);
        mp = mp.max(pp);
        mfe = mfe.max(p.info.feet_err.abs());
        mh = mh.max(p.info.hand_err.abs());
        eprintln!(
            "PEOPLE  {:?} at=({:.2},{:.2}) room={} support={} pen_furn={:.3} pen_wall={:.3} pen_people={:.3} feet_err={:+.3} seat_gap={:+.3} hand_err={:+.3}",
            p.kind,
            p.pose.0[0][3],
            p.pose.0[1][3],
            p.info.room,
            p.info.support.as_deref().unwrap_or("-"),
            pf,
            pw,
            pp,
            p.info.feet_err,
            p.info.seat_gap,
            p.info.hand_err
        );
    }
    eprintln!("PEOPLE_SUMMARY requested={n} placed={} kinds={kinds:?} max_pen_furn={mf:.3} max_pen_wall={mw:.3} max_pen_people={mp:.3} max_feet_err={mfe:.3} max_hand_err={mh:.3} time={dt:?}", placed.len());
}
