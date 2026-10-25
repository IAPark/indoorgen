//! The 12 pose definitions (joint rotations on the Rocketbox biped) and the measurements taken from a posed mesh.
use crate::avatar::{Avatar, PartKind};
use crate::rig::*;
use crate::PoseKind;
use std::collections::HashMap;

/// tiny deterministic generator for per-instance variation
pub struct Var {
    s: u64,
    on: bool,
}
impl Var {
    pub fn new(seed: u64) -> Var {
        Var { s: seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03, on: seed != 0 }
    }
    fn next(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
    /// uniform in [-a, a] (0 when variation is off)
    pub fn v(&mut self, a: f64) -> f64 {
        let u = self.next();
        if self.on {
            (u * 2.0 - 1.0) * a
        } else {
            0.0
        }
    }
    pub fn flag(&mut self) -> bool {
        self.next() < 0.5
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    /// origin between the soles, soles on z = 0
    Feet,
    /// origin below the seat contact, soles on z = 0
    Seat,
    /// lowest body point on z = 0, origin below the hip
    Lie,
}

pub struct Def {
    pub ops: HashMap<usize, Op>,
    pub root_t: V,
    pub anchor: Anchor,
    pub hands: bool,
}

fn d(x: f64, y: f64, z: f64) -> V {
    norm([x, y, z])
}

struct B<'a> {
    rig: &'a Rig<'a>,
    ops: HashMap<usize, Op>,
}
impl<'a> B<'a> {
    fn set(&mut self, name: &str, op: Op) {
        if let Some(i) = self.rig.try_id(name) {
            self.ops.insert(i, op);
        }
    }
    fn side(&mut self, s: f64, part: &str, op: Op) {
        let n = format!("Bip01 {} {}", if s < 0.0 { "L" } else { "R" }, part);
        self.set(&n, op);
    }
    fn rel(&mut self, name: &str, e: V) {
        self.set(&format!("Bip01 {name}"), Op::Rel(e));
    }
    fn arm_hang(&mut self, s: f64, out: f64, fwd: f64, elbow: f64) {
        self.side(s, "UpperArm", Op::Aim { dir: d(s * out, fwd, -1.0), twist: 0.0 });
        self.side(s, "Forearm", Op::Aim { dir: d(s * (out * 0.6), elbow, -1.0), twist: 0.0 });
    }
}

/// legs straight below the hips (the biped's rest pose has the ankles ~14 cm behind the hips), feet flat, toes slightly out
fn stand_legs(b: &mut B, v: &mut Var) {
    for s in [-1.0, 1.0] {
        b.side(s, "Thigh", Op::Aim { dir: d(s * (0.03 + v.v(0.01)), 0.02, -1.0), twist: 0.0 });
        b.side(s, "Calf", Op::Aim { dir: d(s * 0.0, -0.03, -1.0), twist: 0.0 });
        b.side(s, "Foot", Op::World([0.0, 0.0, -s * (6.0 + v.v(3.0))]));
    }
}

/// build the joint operations of a pose. `org` = (x, y, z) of the final frame's origin in the avatar's rest body frame (for hand targets
/// of standing poses, whose feet stay at the rest position).
pub fn make_def(rig: &Rig, kind: PoseKind, seed: u64, org: V, root_z: Option<f64>) -> Def {
    use PoseKind::*;
    let mut v = Var::new(seed);
    let mut b = B { rig, ops: HashMap::new() };
    let pelvis = add(rig.rest(rig.id("Bip01 Pelvis")), [0.0, 0.0, root_z.unwrap_or(0.0)]);
    let at = |x: f64, y: f64, z: f64| add(org, [x, y, z]);
    // common head / neck variation
    let head = |b: &mut B, v: &mut Var, pitch: f64, yaw: f64| {
        let p = pitch + v.v(4.0);
        let y = yaw + v.v(7.0);
        b.rel("Neck", [p * 0.4, 0.0, y * 0.4]);
        b.rel("Head", [p * 0.6, 0.0, y * 0.6]);
    };
    let mut anchor = Anchor::Feet;
    let mut hands = false;
    match kind {
        Stand => {
            stand_legs(&mut b, &mut v);
            b.rel("Spine1", [v.v(2.0) - 1.0, 0.0, v.v(3.0)]);
            b.rel("Spine2", [v.v(2.0), 0.0, v.v(3.0)]);
            head(&mut b, &mut v, 0.0, 0.0);
            let out = 0.10 + v.v(0.03);
            for s in [-1.0, 1.0] {
                b.arm_hang(s, out, -0.03, 0.22 + v.v(0.08));
            }
        }
        Walk => {
            let sg = if v.flag() { 1.0 } else { -1.0 };
            // leg `s` forward when s == sg
            for s in [-1.0, 1.0] {
                let fwd = s == sg;
                if fwd {
                    b.side(s, "Thigh", Op::Aim { dir: d(s * 0.03, 0.34, -1.0), twist: 0.0 });
                    b.side(s, "Calf", Op::Aim { dir: d(s * 0.0, 0.12, -1.0), twist: 0.0 });
                    b.side(s, "Foot", Op::World([12.0, 0.0, -s * 4.0]));
                } else {
                    b.side(s, "Thigh", Op::Aim { dir: d(s * 0.03, -0.30, -1.0), twist: 0.0 });
                    b.side(s, "Calf", Op::Aim { dir: d(s * 0.0, -0.62, -1.0), twist: 0.0 });
                    b.side(s, "Foot", Op::World([-32.0, 0.0, -s * 4.0]));
                }
                // arms swing opposite to the legs
                if fwd {
                    b.side(s, "UpperArm", Op::Aim { dir: d(s * 0.12, -0.42, -1.0), twist: 0.0 });
                    b.side(s, "Forearm", Op::Aim { dir: d(s * 0.08, -0.15, -1.0), twist: 0.0 });
                } else {
                    b.side(s, "UpperArm", Op::Aim { dir: d(s * 0.10, 0.45, -1.0), twist: 0.0 });
                    b.side(s, "Forearm", Op::Aim { dir: d(s * 0.06, 0.95, -0.85), twist: 0.0 });
                }
            }
            b.rel("Pelvis", [-3.0, 0.0, sg * 6.0]);
            b.rel("Spine1", [-3.0, 0.0, -sg * 6.0]);
            head(&mut b, &mut v, 0.0, 0.0);
        }
        Crouch => {
            for s in [-1.0, 1.0] {
                b.side(s, "Thigh", Op::Aim { dir: d(s * 0.28, 1.0, 0.12), twist: 0.0 });
                b.side(s, "Calf", Op::Aim { dir: d(s * 0.05, -0.55, -1.0), twist: 0.0 });
                b.side(s, "Foot", Op::World([-12.0, 0.0, -s * 15.0]));
                b.side(s, "UpperArm", Op::Aim { dir: d(s * 0.25, 0.55, -0.8), twist: 0.0 });
                b.side(s, "Forearm", Op::Aim { dir: d(s * 0.15, 1.0, -0.35), twist: 0.0 });
            }
            b.rel("Pelvis", [-28.0, 0.0, 0.0]);
            b.rel("Spine1", [-14.0, 0.0, 0.0]);
            b.rel("Spine2", [-8.0, 0.0, 0.0]);
            head(&mut b, &mut v, 12.0, 0.0);
        }
        SitUpright | SitRelaxed | SitBedEdge => {
            anchor = Anchor::Seat;
            let spread = match kind {
                SitUpright => 0.16,
                SitRelaxed => 0.30,
                _ => 0.14,
            };
            for s in [-1.0, 1.0] {
                let sp = spread + v.v(0.03);
                if root_z.is_some() {
                    // seat height given: legs by IK, ankles flat on the floor below / in front of the hips
                    let fz = rig.rest(rig.id(&format!("Bip01 {} Foot", if s < 0.0 { "L" } else { "R" })))[2];
                    let reach = if kind == SitRelaxed { 0.55 + v.v(0.1) } else { 0.40 + v.v(0.02) };
                    let tgt = [pelvis[0] + s * (0.088 + sp * 0.55 * reach), pelvis[1] + reach, fz];
                    b.side(s, "Thigh", Op::Ik { target: tgt, pole: d(s * 0.15, 0.4, 1.0) });
                    b.side(s, "Foot", Op::World([if kind == SitRelaxed { 6.0 } else { 0.0 }, 0.0, -s * (8.0 + v.v(4.0))]));
                    continue;
                }
                b.side(s, "Thigh", Op::Aim { dir: d(s * sp, 1.0, -0.04), twist: 0.0 });
                let (cf, cy) = match kind {
                    SitRelaxed => (0.55 + v.v(0.2), 0.0),
                    SitBedEdge => (0.02, 0.0),
                    _ => (0.10, 0.0),
                };
                b.side(s, "Calf", Op::Aim { dir: d(s * (0.03 + cy), cf, -1.0), twist: 0.0 });
                b.side(s, "Foot", Op::World([if kind == SitRelaxed { 6.0 } else { 0.0 }, 0.0, -s * (8.0 + v.v(4.0))]));
            }
            hands = false;
            let thigh_hand = |s: f64, fwd: f64, h: f64| [pelvis[0] + s * 0.17, pelvis[1] + fwd, pelvis[2] + h];
            match kind {
                SitUpright => {
                    b.rel("Pelvis", [-3.0, 0.0, 0.0]);
                    b.rel("Spine1", [1.0, 0.0, 0.0]);
                    head(&mut b, &mut v, 0.0, 0.0);
                    for s in [-1.0, 1.0] {
                        b.side(s, "UpperArm", Op::Ik { target: thigh_hand(s, 0.24 + v.v(0.04), 0.115), pole: d(s * 0.6, -0.4, -0.2) });
                    }
                }
                SitRelaxed => {
                    b.rel("Pelvis", [14.0, 0.0, 0.0]);
                    b.rel("Spine1", [-6.0, 0.0, 0.0]);
                    b.rel("Spine2", [-6.0, 0.0, 0.0]);
                    head(&mut b, &mut v, 6.0, 0.0);
                    for s in [-1.0, 1.0] {
                        b.side(s, "UpperArm", Op::Ik { target: [pelvis[0] + s * (0.13 + v.v(0.02)), pelvis[1] + 0.13 + v.v(0.05), pelvis[2] + 0.17], pole: d(s * 0.8, -0.5, -0.2) });
                    }
                }
                _ => {
                    // sitting on a bed edge, leaning forward, forearms on the thighs
                    b.rel("Pelvis", [-6.0, 0.0, 0.0]);
                    b.rel("Spine1", [-10.0, 0.0, 0.0]);
                    b.rel("Spine2", [-10.0, 0.0, 0.0]);
                    head(&mut b, &mut v, 14.0, 0.0);
                    for s in [-1.0, 1.0] {
                        b.side(s, "UpperArm", Op::Ik { target: thigh_hand(s, 0.34 + v.v(0.04), 0.115), pole: d(s * 0.5, -0.2, 0.3) });
                    }
                }
            }
        }
        LieBed | LieSofa => {
            anchor = Anchor::Lie;
            let side = kind == LieSofa && seed % 4 == 3;
            let roll = if side { if seed % 8 == 3 { 90.0 } else { -90.0 } } else { 0.0 };
            // lie down: rotate the whole body about x (head towards -y, face up), then optionally roll onto the side
            let r0 = rmul(&rot_axis([0., 1., 0.], roll), &rot_axis([1., 0., 0.], 90.0));
            let (ex, ey, ez) = euler_of(&r0);
            b.set("Bip01", Op::Rel([ex, ey, ez]));
            for s in [-1.0, 1.0] {
                b.side(s, "Calf", Op::Rel([if side { 0.0 } else { 8.0 }, 0.0, 0.0]));
            }
            if side {
                // legs bent up a little, top arm in front
                for s in [-1.0, 1.0] {
                    b.side(s, "Thigh", Op::Rel([s * 0.0 + 25.0, 0.0, 0.0]));
                    b.side(s, "Calf", Op::Rel([-45.0, 0.0, 0.0]));
                }
                // face direction (world x) and which arm is on top
                let fx = if roll < 0.0 { -1.0 } else { 1.0 };
                let top = if roll < 0.0 { 1.0 } else { -1.0 };
                b.side(top, "UpperArm", Op::Aim { dir: d(fx * 0.30, 1.0, -0.35), twist: 0.0 });
                b.side(top, "Forearm", Op::Aim { dir: d(fx * 0.55, 0.9, -0.25), twist: 0.0 });
                b.side(-top, "UpperArm", Op::Aim { dir: d(fx * 0.9, -0.25, 0.05), twist: 0.0 });
                b.side(-top, "Forearm", Op::Aim { dir: d(fx * 0.85, -0.35, 0.25), twist: 0.0 });
            } else {
                // seed bit 20: arms out above the cover (beside the head) for a person under a bed cover
                let arms_up = kind == LieBed && seed & (1 << 20) != 0;
                for s in [-1.0, 1.0] {
                    b.side(s, "Thigh", Op::Rel([0.0, 0.0, s * (5.0 + v.v(2.0))]));
                    if kind == LieSofa && seed & (1 << 21) != 0 {
                        // short sofa: knees up, feet on the cushion
                        b.side(s, "Thigh", Op::Rel([38.0 + v.v(6.0), 0.0, s * (6.0 + v.v(3.0))]));
                        b.side(s, "Calf", Op::Rel([-68.0 + v.v(6.0), 0.0, 0.0]));
                    }
                    if arms_up {
                        b.side(s, "UpperArm", Op::Aim { dir: d(s * 0.85, -0.40 + v.v(0.1), 0.05), twist: 0.0 });
                        b.side(s, "Forearm", Op::Aim { dir: d(s * 0.30, -1.0, 0.10), twist: 0.0 });
                    } else {
                        b.side(s, "UpperArm", Op::Aim { dir: d(s * 0.14, 1.0, -0.12), twist: 0.0 });
                        b.side(s, "Forearm", Op::Aim { dir: d(s * 0.05, 1.0, 0.05), twist: 0.0 });
                    }
                }
            }
            b.rel("Neck", [-8.0 + v.v(3.0), 0.0, v.v(6.0)]);
            b.rel("Head", [-6.0, 0.0, v.v(6.0)]);
        }
        LeanCounter | StandAtSink | StandAtDesk | Reach => {
            hands = true;
            let (lean, hz, hy, hx, hp) = match kind {
                LeanCounter => (20.0, 0.93, 0.38, 0.24, 0.0),
                StandAtSink => (14.0, 0.95, 0.31, 0.13, 20.0),
                StandAtDesk => (8.0, 0.78, 0.30, 0.17, 12.0),
                _ => (-3.0, 1.65, 0.50, 0.12, -10.0),
            };
            // hip hinge: legs upright, the trunk leans
            stand_legs(&mut b, &mut v);
            b.rel("Pelvis", [-lean * 0.55, 0.0, 0.0]);
            b.rel("Spine1", [-lean * 0.25, 0.0, 0.0]);
            b.rel("Spine2", [-lean * 0.20, 0.0, 0.0]);
            head(&mut b, &mut v, hp, 0.0);
            for s in [-1.0, 1.0] {
                if kind == Reach && s < 0.0 {
                    b.arm_hang(s, 0.10, -0.03, 0.2);
                    continue;
                }
                let (tx, ty, tz) = if kind == Reach { (0.16, hy, hz) } else { (s * hx, hy, hz) };
                b.side(s, "UpperArm", Op::Ik { target: at(tx, ty, tz), pole: d(s * 0.7, -0.3, -0.7) });
            }
        }
    }
    let curl = match kind {
        LeanCounter | StandAtSink => 8.0,
        StandAtDesk => 14.0,
        Reach => 12.0,
        _ => 20.0,
    };
    for side in [-1.0, 1.0] {
        let c = curl * (1.0 + v.v(0.25));
        curl_hand(&mut b, side, c);
    }
    Def { ops: b.ops, root_t: [0.0, 0.0, root_z.unwrap_or(0.0)], anchor, hands }
}

/// relaxed finger curl (`c` degrees at the middle joints) on the rest-pose hand: axes come from the rest geometry (palm faces the body in the A pose)
fn curl_hand(b: &mut B, s: f64, c: f64) {
    let rig = b.rig;
    let sd = if s < 0.0 { "L" } else { "R" };
    let palm = [-s, 0.0, 0.0];
    let fac = [0.5, 0.75, 0.9, 1.0, 1.15];
    for k in 0..5 {
        let names = [format!("Bip01 {sd} Finger{k}"), format!("Bip01 {sd} Finger{k}1"), format!("Bip01 {sd} Finger{k}2")];
        let ids: Vec<Option<usize>> = names.iter().map(|n| rig.try_id(n)).collect();
        let amt = [0.55 * c, c, 0.7 * c];
        for j in 0..3 {
            let Some(i) = ids[j] else { continue };
            let f = if let Some(Some(ch)) = ids.get(j + 1) { sub(rig.rest(*ch), rig.rest(i)) } else if j > 0 { sub(rig.rest(i), rig.rest(ids[j - 1].unwrap())) } else { continue };
            let f = norm(f);
            let n = norm(sub(palm, scale(f, dot(palm, f))));
            let ax = cross(f, n);
            let ang = amt[j] * fac[k] * if k == 0 { 0.8 } else { 1.0 };
            let q = rot_axis(ax, ang);
            let (ex, ey, ez) = euler_of(&q);
            b.ops.insert(i, Op::Rel([ex, ey, ez]));
        }
    }
}

/// recover euler (degrees, R = Rz Ry Rx) from a rotation matrix
pub fn euler_of(r: &R3) -> (f64, f64, f64) {
    let sy = -r[2][0];
    let y = sy.clamp(-1.0, 1.0).asin();
    if y.cos().abs() > 1e-6 {
        (r[2][1].atan2(r[2][2]).to_degrees(), y.to_degrees(), r[1][0].atan2(r[0][0]).to_degrees())
    } else {
        ((-r[1][2]).atan2(r[1][1]).to_degrees(), y.to_degrees(), 0.0)
    }
}

/// contact measurements of a skinned avatar (before the anchor shift)
pub struct Meas {
    pub sole: [[f64; 3]; 2],
    pub zfoot: f64,
    pub seat: [f64; 3],
    pub zmin: f64,
    pub hands: [[f64; 3]; 2],
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

/// `skin`: skinned positions per part. Bones are looked up by name.
pub fn measure(av: &Avatar, rig: &Rig, skin: &[(Vec<[f32; 3]>, Vec<[f32; 3]>)], hip: [f64; 3]) -> Meas {
    let bl = |n: &str| rig.id(n);
    let foot_l = [bl("Bip01 L Foot"), bl("Bip01 L Toe0")];
    let foot_r = [bl("Bip01 R Foot"), bl("Bip01 R Toe0")];
    let pel = [bl("Bip01 Pelvis"), bl("Bip01 L Thigh"), bl("Bip01 R Thigh")];
    let hand_ids = |side: &str| -> Vec<usize> { av.bones.iter().enumerate().filter(|(_, b)| b.name.starts_with(&format!("Bip01 {side} ")) && (b.name.contains("Hand") || b.name.contains("Finger"))).map(|(i, _)| i).collect() };
    let (hl, hr) = (hand_ids("L"), hand_ids("R"));
    let wsum = |p: &crate::avatar::AvPart, i: usize, set: &[usize]| -> f32 { (0..4).filter(|&k| set.contains(&(p.joints[i][k] as usize))).map(|k| p.weights[i][k]).sum() };
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    let mut fl: Vec<[f64; 3]> = vec![];
    let mut fr: Vec<[f64; 3]> = vec![];
    let mut pv: Vec<[f64; 3]> = vec![];
    let mut hlv: Vec<[f64; 3]> = vec![];
    let mut hrv: Vec<[f64; 3]> = vec![];
    let mut zmin_body = f64::MAX;
    for (pi, p) in av.parts.iter().enumerate() {
        for i in 0..p.pos.len() {
            let q = skin[pi].0[i];
            let q = [q[0] as f64, q[1] as f64, q[2] as f64];
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
            if p.kind == PartKind::Hair {
                continue;
            }
            zmin_body = zmin_body.min(q[2]);
            if p.kind != PartKind::Body {
                continue;
            }
            if wsum(p, i, &foot_l) > 0.5 {
                fl.push(q);
            }
            if wsum(p, i, &foot_r) > 0.5 {
                fr.push(q);
            }
            if wsum(p, i, &pel) > 0.5 && q[1] - hip[1] > -0.13 && q[1] - hip[1] < 0.03 {
                pv.push(q);
            }
            if wsum(p, i, &hl) > 0.5 {
                hlv.push(q);
            }
            if wsum(p, i, &hr) > 0.5 {
                hrv.push(q);
            }
        }
    }
    let low_centroid = |v: &[[f64; 3]], band: f64| -> ([f64; 3], f64) {
        let z0 = v.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
        let sel: Vec<_> = v.iter().filter(|p| p[2] < z0 + band).collect();
        let n = sel.len().max(1) as f64;
        ([sel.iter().map(|p| p[0]).sum::<f64>() / n, sel.iter().map(|p| p[1]).sum::<f64>() / n, z0], z0)
    };
    let (sl, zl) = low_centroid(&fl, 0.02);
    let (sr, zr) = low_centroid(&fr, 0.02);
    let (seat, _) = low_centroid(&pv, 0.035);
    let cen = |v: &[[f64; 3]]| {
        let n = v.len().max(1) as f64;
        [v.iter().map(|p| p[0]).sum::<f64>() / n, v.iter().map(|p| p[1]).sum::<f64>() / n, v.iter().map(|p| p[2]).sum::<f64>() / n]
    };
    Meas { sole: [sl, sr], zfoot: zl.min(zr), seat, zmin: zmin_body, hands: [cen(&hlv), cen(&hrv)], lo, hi }
}
