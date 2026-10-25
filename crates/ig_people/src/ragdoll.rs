//! Ragdoll settling of a posed avatar (rapier3d-f64): one rigid body per major bone (pelvis, spine segments, neck, head, upper/lower arms,
//! hands, thighs, shins, feet) with a convex hull fitted to the skinned mesh vertices that bone owns (fingers, toes, clavicles, face and
//! helper bones ride on their segment), joined by spherical joints (cone limits, PD motors toward the key pose) and revolute hinges (knee,
//! elbow: 0..150 degrees of flexion). The body starts in the key pose, is pulled toward it by moderate joint motors and falls onto the
//! static furniture / floor colliders for ~1.5 s of simulated time at a fixed 1/60 s step; the settled bone transforms replace the key pose
//! for skinning. Deterministic: fixed step, no threads, order-stable inputs.
//!
//! Frames: the key pose `g_key` maps the rest (bind) pose into the person's local contract frame (see body.rs); `pose` maps local to world.
//! Env: `RAGDOLL=0` disables (A/B), `RAGDOLL_K` / `RAGDOLL_C` motor stiffness / damping, `RAGDOLL_STEPS` max steps, `RAGDOLL_DBG` prints.
use crate::avatar::Avatar;
use crate::rig::*;
use ig_core::mesh::{Mat4, V3};
use rapier3d_f64::prelude::*;

pub type Tri = [V3; 3];

/// a static collider soup (world frame); `dz` lowers it (soft supports: the body sinks in)
pub struct StaticTris {
    pub tris: Vec<Tri>,
    pub dz: f64,
}

pub fn enabled() -> bool {
    std::env::var("RAGDOLL").map(|v| v != "0").unwrap_or(true)
}

/// poses that are settled by physics (the others stay kinematic)
pub fn wants(kind: crate::PoseKind) -> bool {
    use crate::PoseKind::*;
    matches!(kind, LieBed | LieSofa | SitRelaxed | SitBedEdge)
}

#[derive(Clone, Debug, Default)]
pub struct Settled {
    /// global bone transforms (rest point -> local contract frame) after settling
    pub g: Vec<Xf>,
    pub steps: usize,
    pub ms: f64,
    /// largest movement of a segment origin from the key pose (m)
    pub moved: f64,
    /// deepest hull-vs-static penetration at the end (m)
    pub pen: f64,
    pub bodies: usize,
}

struct Seg {
    bone: usize,
    parent: Option<usize>,
    name: &'static str,
    lim_deg: f64,
    hinge: bool,
}

fn v(p: V) -> Vector {
    Vector::new(p[0], p[1], p[2])
}

fn quat_of(r: &R3) -> Rotation {
    // robust matrix -> quaternion (Shepperd)
    let t = r[0][0] + r[1][1] + r[2][2];
    let (x, y, z, w);
    if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        w = 0.25 * s;
        x = (r[2][1] - r[1][2]) / s;
        y = (r[0][2] - r[2][0]) / s;
        z = (r[1][0] - r[0][1]) / s;
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        w = (r[2][1] - r[1][2]) / s;
        x = 0.25 * s;
        y = (r[0][1] + r[1][0]) / s;
        z = (r[0][2] + r[2][0]) / s;
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        w = (r[0][2] - r[2][0]) / s;
        x = (r[0][1] + r[1][0]) / s;
        y = 0.25 * s;
        z = (r[1][2] + r[2][1]) / s;
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        w = (r[1][0] - r[0][1]) / s;
        x = (r[0][2] + r[2][0]) / s;
        y = (r[1][2] + r[2][1]) / s;
        z = 0.25 * s;
    }
    let n = (x * x + y * y + z * z + w * w).sqrt();
    Rotation::from_xyzw(x / n, y / n, z / n, w / n)
}

fn mat_of(q: &Rotation) -> R3 {
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

fn xf_inv(x: &Xf) -> Xf {
    let rt = rtrans(&x.r);
    Xf { r: rt, t: scale(rapply(&rt, x.t), -1.0) }
}

fn world_xf(pose: &Mat4) -> Xf {
    let m = &pose.0;
    Xf { r: [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]], t: [m[0][3], m[1][3], m[2][3]] }
}

fn segments(rig: &Rig) -> Vec<Seg> {
    let mut v: Vec<Seg> = vec![];
    let add = |name: &'static str, lim: f64, hinge: bool, v: &mut Vec<Seg>| {
        if let Some(b) = rig.try_id(name) {
            v.push(Seg { bone: b, parent: None, name, lim_deg: lim, hinge });
        }
    };
    add("Bip01 Pelvis", 0.0, false, &mut v);
    add("Bip01 Spine", 18.0, false, &mut v);
    add("Bip01 Spine1", 18.0, false, &mut v);
    add("Bip01 Spine2", 18.0, false, &mut v);
    add("Bip01 Neck", 30.0, false, &mut v);
    add("Bip01 Head", 35.0, false, &mut v);
    for (l, side) in [("Bip01 L ", 0), ("Bip01 R ", 1)] {
        let _ = side;
        let n = |s: &str| -> &'static str { Box::leak(format!("{l}{s}").into_boxed_str()) };
        add(n("UpperArm"), 70.0, false, &mut v);
        add(n("Forearm"), 0.0, true, &mut v);
        add(n("Hand"), 45.0, false, &mut v);
        add(n("Thigh"), 45.0, false, &mut v);
        add(n("Calf"), 0.0, true, &mut v);
        add(n("Foot"), 28.0, false, &mut v);
    }
    // parent segment: nearest ancestor (rig parents) that is a segment
    let bones: Vec<usize> = v.iter().map(|s| s.bone).collect();
    for i in 0..v.len() {
        let mut b = rig.parents[v[i].bone];
        while b >= 0 {
            if let Some(p) = bones.iter().position(|&x| x as i32 == b) {
                v[i].parent = Some(p);
                break;
            }
            b = rig.parents[b as usize];
        }
    }
    v
}

/// segment index owning each bone (nearest segment ancestor-or-self; the pelvis for the root chain)
fn owner(rig: &Rig, segs: &[Seg]) -> Vec<usize> {
    let n = rig.av.bones.len();
    let mut out = vec![0usize; n];
    for b in 0..n {
        let mut c = b as i32;
        let mut found = 0;
        while c >= 0 {
            if let Some(s) = segs.iter().position(|s| s.bone as i32 == c) {
                found = s;
                break;
            }
            c = rig.parents[c as usize];
        }
        out[b] = found;
    }
    out
}

/// per-segment hull points in the segment frame (rest coordinates minus the rest pivot)
fn hull_points(av: &Avatar, rig: &Rig, segs: &[Seg], own: &[usize]) -> Vec<Vec<V>> {
    let mut pts: Vec<Vec<V>> = vec![vec![]; segs.len()];
    for p in &av.parts {
        if p.kind == crate::avatar::PartKind::Hair {
            continue;
        }
        for i in 0..p.pos.len() {
            let mut w = vec![0.0f32; segs.len()];
            for k in 0..4 {
                if p.weights[i][k] > 0.0 {
                    w[own[p.joints[i][k] as usize]] += p.weights[i][k];
                }
            }
            let (si, wm) = w.iter().enumerate().fold((0, 0.0f32), |a, (i, &x)| if x > a.1 { (i, x) } else { a });
            if wm < 0.4 {
                continue;
            }
            let piv = rig.rest(segs[si].bone);
            pts[si].push(sub([p.pos[i][0] as f64, p.pos[i][1] as f64, p.pos[i][2] as f64], piv));
        }
    }
    pts
}

/// keep the extreme point of `n` Fibonacci-sphere directions (a tight polytope of at most `n` vertices: cheap contact generation against
/// dense furniture meshes)
fn reduce_hull(pts: &[V], n: usize) -> Vec<V> {
    if pts.len() <= n {
        return pts.to_vec();
    }
    let mut keep: Vec<usize> = vec![];
    let ga = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    for i in 0..n {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
        let r = (1.0 - z * z).sqrt();
        let d = [r * (ga * i as f64).cos(), r * (ga * i as f64).sin(), z];
        let mut best = (f64::MIN, 0usize);
        for (j, p) in pts.iter().enumerate() {
            let t = dot(*p, d);
            if t > best.0 {
                best = (t, j);
            }
        }
        if !keep.contains(&best.1) {
            keep.push(best.1);
        }
    }
    keep.into_iter().map(|j| pts[j]).collect()
}

/// Rest-pose geometry that does not depend on the pose: segments, owners and hull points (cached per avatar by the caller if needed).
pub struct Model {
    segs: Vec<Seg>,
    own: Vec<usize>,
    hulls: Vec<Vec<V>>,
}

pub fn model(av: &Avatar) -> Model {
    let rig = Rig::new(av);
    let segs = segments(&rig);
    let own = owner(&rig, &segs);
    let hulls = hull_points(av, &rig, &segs, &own).into_iter().map(|p| reduce_hull(&p, env_f("RAGDOLL_HULLDIRS", 90.0) as usize)).collect();
    Model { segs, own, hulls }
}

/// cached `model` per avatar index
pub fn model_of(avatar: usize, av: &Avatar) -> std::sync::Arc<Model> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static C: OnceLock<Mutex<HashMap<usize, Arc<Model>>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some(m) = c.lock().unwrap().get(&avatar) {
        return m.clone();
    }
    let m = Arc::new(model(av));
    c.lock().unwrap().insert(avatar, m.clone());
    m
}

fn env_f(k: &str, d: f64) -> f64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// Settle the avatar in the key pose `g_key` (local contract frame) placed in the world by `pose` against the static colliders.
/// `lift`: start this far above the key pose (the body then falls onto the supports). Returns None when the model is incomplete.
pub fn settle(av: &Avatar, m: &Model, g_key: &[Xf], pose: &Mat4, statics: &[StaticTris], floor_z: f64, lift: f64) -> Option<Settled> {
    let t0 = std::time::Instant::now();
    let rig = Rig::new(av);
    let segs = &m.segs;
    if segs.len() < 16 {
        return None;
    }
    let pw = world_xf(pose);
    let gw: Vec<Xf> = g_key.iter().map(|x| pw.mul(x)).collect();
    let up = [0.0, 0.0, lift];
    let mut w = PhysicsWorld::default();
    w.gravity = Vector::new(0.0, 0.0, -9.81 * env_f("RAGDOLL_G", 1.0));
    w.integration_parameters.dt = 1.0 / 60.0;
    w.integration_parameters.normalized_max_corrective_velocity = 0.8;
    w.integration_parameters.normalized_max_linear_velocity = 4.0;
    w.integration_parameters.num_solver_iterations = env_f("RAGDOLL_ITERS", 12.0) as usize;
    let g_static = InteractionGroups::new(Group::GROUP_2, Group::GROUP_1, InteractionTestMode::And);
    let g_body = InteractionGroups::new(Group::GROUP_1, Group::GROUP_2, InteractionTestMode::And);
    // static world
    let (mut blo, mut bhi) = ([f64::MAX; 3], [f64::MIN; 3]);
    // body bbox from segment origins +- 1.2 m (limbs are < 1 m): used only to cull static triangles
    for s in segs {
        let p = gw[s.bone].apply(rig.rest(s.bone));
        for k in 0..3 {
            blo[k] = blo[k].min(p[k]);
            bhi[k] = bhi[k].max(p[k]);
        }
    }
    let pad = 0.45;
    let mut n_tri = 0usize;
    let mut n_static = 0;
    let cell = env_f("RAGDOLL_CELL", 0.02);
    for st in statics {
        // cull to the person's neighbourhood, then vertex-cluster the soup (dense furniture meshes make contact generation slow)
        use std::collections::BTreeMap;
        let mut cells: BTreeMap<(i64, i64, i64), usize> = BTreeMap::new();
        let mut acc: Vec<([f64; 3], f64)> = vec![];
        let mut idx3: Vec<[usize; 3]> = vec![];
        for t in &st.tris {
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            for q in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            if (0..3).any(|k| lo[k] > bhi[k] + pad || hi[k] < blo[k] - pad) {
                continue;
            }
            let mut ids = [0usize; 3];
            for (j, q) in t.iter().enumerate() {
                let key = ((q[0] / cell).floor() as i64, (q[1] / cell).floor() as i64, (q[2] / cell).floor() as i64);
                let n = acc.len();
                let id = *cells.entry(key).or_insert(n);
                if id == n {
                    acc.push(([0.0; 3], 0.0));
                }
                acc[id].0 = add(acc[id].0, *q);
                acc[id].1 += 1.0;
                ids[j] = id;
            }
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                idx3.push(ids);
            }
        }
        if idx3.is_empty() {
            continue;
        }
        let verts: Vec<Vector> = acc.iter().map(|(s, n)| Vector::new(s[0] / n, s[1] / n, s[2] / n - st.dz)).collect();
        let idx: Vec<[u32; 3]> = idx3.iter().map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect();
        n_tri += idx.len();
        if let Ok(c) = ColliderBuilder::trimesh(verts, idx) {
            w.insert_collider(c.friction(0.8).restitution(0.0).collision_groups(g_static), None);
            n_static += 1;
        }
    }
    let t_static = t0.elapsed().as_secs_f64() * 1000.0;
    let floor = ColliderBuilder::cuboid(20.0, 20.0, 0.5).translation(Vector::new((blo[0] + bhi[0]) * 0.5, (blo[1] + bhi[1]) * 0.5, floor_z - 0.5)).friction(0.8).restitution(0.0).collision_groups(g_static);
    w.insert_collider(floor, None);
    // bodies
    let mut handles: Vec<RigidBodyHandle> = vec![];
    let mut key_pos: Vec<V> = vec![];
    for (si, s) in segs.iter().enumerate() {
        let g = &gw[s.bone];
        let piv = rig.rest(s.bone);
        let pos = add(g.apply(piv), up);
        key_pos.push(pos);
        let rb = RigidBodyBuilder::dynamic().pose(Pose::from_parts(v(pos), quat_of(&g.r))).linear_damping(0.8).angular_damping(2.0).can_sleep(false);
        let bh = w.insert_body(rb);
        let pts: Vec<Vector> = m.hulls[si].iter().map(|&p| v(p)).collect();
        let col = if pts.len() >= 8 { ColliderBuilder::convex_hull(&pts) } else { None };
        let col = match col {
            Some(c) => c,
            None => {
                // degenerate: a small ball at the pivot
                ColliderBuilder::ball(0.05)
            }
        };
        // anatomical masses (70 kg person): keeps the mass ratios of the chain moderate for the solver
        let mass = match s.name.trim_start_matches("Bip01 ").trim_start_matches("L ").trim_start_matches("R ") {
            "Pelvis" => 10.0,
            "Spine" => 5.5,
            "Spine1" => 7.0,
            "Spine2" => 11.0,
            "Neck" => 1.4,
            "Head" => 5.6,
            "UpperArm" => 1.9,
            "Forearm" => 1.1,
            "Hand" => 0.45,
            "Thigh" => 7.0,
            "Calf" => 3.2,
            _ => 1.0,
        } * env_f("RAGDOLL_MASS", 1.0);
        w.insert_collider(col.mass(mass).friction(0.8).restitution(0.0).collision_groups(g_body), Some(bh));
        handles.push(bh);
    }
    // motor gains from the supported weight: torque to hold a joint within `dev` radians against gravity (force-based motors), so a light
    // neck still holds a heavy head and a hip holds a whole leg, while contact forces larger than that win
    let nseg = segs.len();
    let mass: Vec<f64> = handles.iter().map(|&h| w.bodies[h].mass()).collect();
    let com: Vec<V> = handles.iter().map(|&h| { let c = w.bodies[h].center_of_mass(); [c.x, c.y, c.z] }).collect();
    let mut sub_m = mass.clone();
    let mut sub_c: Vec<V> = (0..nseg).map(|i| scale(com[i], mass[i])).collect();
    for i in (0..nseg).rev() {
        if let Some(p) = segs[i].parent {
            sub_m[p] += sub_m[i];
            sub_c[p] = add(sub_c[p], sub_c[i]);
        }
    }
    // (sub_c holds the mass-weighted sum of the subtree COMs; children were folded into parents above)
    let kscale = env_f("RAGDOLL_K", 25.0);
    let kl = env_f("RAGDOLL_KL", 4.0);
    let gains = |si: usize, dev: f64, piv: V| -> (f64, f64) {
        let trunk = matches!(segs[si].name, "Bip01 Spine" | "Bip01 Spine1" | "Bip01 Spine2" | "Bip01 Neck" | "Bip01 Head");
        let kscale = if trunk { kscale } else { kl };
        let cm = scale(sub_c[si], 1.0 / sub_m[si].max(1e-6));
        let d = len(sub(cm, piv)).max(0.06);
        let kk = sub_m[si] * 9.81 * d / dev * kscale;
        let inertia = sub_m[si] * d * d + 0.02 * sub_m[si];
        (kk, 2.0 * 0.8 * (kk * inertia).sqrt())
    };
    for (si, s) in segs.iter().enumerate() {
        let Some(pi) = s.parent else { continue };
        let ps = &segs[pi];
        let anchor1 = sub(rig.rest(s.bone), rig.rest(ps.bone));
        // key-pose relative rotation child-in-parent (rest axes): R_rel = Rp^T Rc
        let rel = rmul(&rtrans(&gw[ps.bone].r), &gw[s.bone].r);
        let f2 = quat_of(&rtrans(&rel));
        let gw_piv = |i: usize| -> V { add(gw[segs[i].bone].apply(rig.rest(segs[i].bone)), up) };
        let (k, c) = gains(si, env_f("RAGDOLL_DEV_LIMB", 0.3), gw_piv(si));
        if s.hinge {
            // flexion axis: knees bend backwards (-x), elbows forwards (+x); a bent key pose uses its own rotation axis (sign towards that)
            let knee = s.name.ends_with("Calf");
            let fl = if knee { [-1.0, 0.0, 0.0] } else { [1.0, 0.0, 0.0] };
            let tr = rel[0][0] + rel[1][1] + rel[2][2];
            let ang = ((tr - 1.0) * 0.5).clamp(-1.0, 1.0).acos();
            let (axis, a_key) = if ang > 0.05 {
                let ar = norm([rel[2][1] - rel[1][2], rel[0][2] - rel[2][0], rel[1][0] - rel[0][1]]);
                if dot(ar, fl) < 0.0 { (scale(ar, -1.0), -ang) } else { (ar, ang) }
            } else {
                (fl, 0.0)
            };
            let f1 = Rotation::from_rotation_arc(Vector::X, v(axis));
            let f2h = quat_of(&rtrans(&rel)) * f1;
            let lo = (-6.0f64).to_radians() - a_key;
            let hi = 150.0f64.to_radians() - a_key;
            let j = GenericJointBuilder::new(JointAxesMask::LIN_X | JointAxesMask::LIN_Y | JointAxesMask::LIN_Z | JointAxesMask::ANG_Y | JointAxesMask::ANG_Z)
                .local_frame1(Pose::from_parts(v(anchor1), f1))
                .local_frame2(Pose::from_parts(Vector::ZERO, f2h))
                .limits(JointAxis::AngX, [lo.min(-0.02), hi.max(0.02)])
                .motor_model(JointAxis::AngX, MotorModel::ForceBased)
                .motor_position(JointAxis::AngX, 0.0, k, c)
                .build();
            w.insert_impulse_joint(handles[pi], handles[si], j);
        } else {
            let lim = s.lim_deg.to_radians();
            let trunk = matches!(s.name, "Bip01 Spine" | "Bip01 Spine1" | "Bip01 Spine2" | "Bip01 Neck" | "Bip01 Head");
            let (k, c) = gains(si, if trunk { env_f("RAGDOLL_DEV_TRUNK", 0.12) } else { env_f("RAGDOLL_DEV_LIMB", 0.3) }, gw_piv(si));
            let mut jb = SphericalJointBuilder::new().local_frame1(Pose::from_parts(v(anchor1), Rotation::IDENTITY)).local_frame2(Pose::from_parts(Vector::ZERO, f2));
            for ax in [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ] {
                jb = jb.limits(ax, [-lim, lim]).motor_model(ax, MotorModel::ForceBased).motor_position(ax, 0.0, k, c);
            }
            w.insert_impulse_joint(handles[pi], handles[si], jb.build());
        }
    }
    let pen_of = |w: &PhysicsWorld, h: RigidBodyHandle| -> f64 {
        let mut pen = 0.0f64;
        let body = &w.bodies[h];
        for &ch in body.colliders() {
            let col = &w.colliders[ch];
            let shape = col.shared_shape().clone();
            let cp = *col.position();
            for (oh, oc) in w.intersect_shape(cp, &*shape, QueryFilter::default()) {
                if oh == ch || oc.parent().is_some() {
                    continue;
                }
                if let Ok(Some(ct)) = rapier3d_f64::parry::query::contact(&cp, &*shape, oc.position(), oc.shape(), 0.0) {
                    pen = pen.max(-ct.dist);
                }
            }
        }
        pen
    };
    // start clear of the supports: raise the whole body until no hull penetrates deeper than 4 mm (the broad phase needs one step)
    w.integration_parameters.dt = 1e-4;
    w.step();
    w.integration_parameters.dt = 1.0 / 60.0;
    for _ in 0..4 {
        let worst = handles.iter().map(|&h| pen_of(&w, h)).fold(0.0f64, f64::max);
        if worst < 0.004 {
            break;
        }
        for &h in &handles {
            let b = &mut w.bodies[h];
            let mut t = b.translation();
            t.z += worst + 0.004;
            b.set_translation(t, true);
        }
        w.integration_parameters.dt = 1e-4;
        w.step();
        w.integration_parameters.dt = 1.0 / 60.0;
    }
    if std::env::var_os("RAGDOLL_DBG").is_some() {
        w.step();
        for (si, s) in segs.iter().enumerate() {
            eprintln!("  init pen {:<16} {:.3}", s.name, pen_of(&w, handles[si]));
        }
    }
    // run
    let max_steps = env_f("RAGDOLL_STEPS", 120.0) as usize;
    let mut steps = 0;
    let mut calm = 0;
    for step in 0..max_steps {
        w.step();
        steps = step + 1;
        if step >= 30 && step % 5 == 4 {
            let mut vmax = 0.0f64;
            for &h in &handles {
                let b = &w.bodies[h];
                vmax = vmax.max(b.linvel().length() + 0.15 * b.angvel().length());
            }
            if vmax < 0.04 {
                calm += 1;
                if calm >= 3 {
                    break;
                }
            } else {
                calm = 0;
            }
        }
    }
    // read back
    let inv_pw = xf_inv(&pw);
    let mut gs: Vec<Xf> = vec![Xf::ident(); av.bones.len()];
    let mut seg_g: Vec<Xf> = vec![];
    let mut moved = 0.0f64;
    for (si, s) in segs.iter().enumerate() {
        let b = &w.bodies[handles[si]];
        let q = *b.position();
        let r = mat_of(&q.rotation);
        let t = [q.translation.x, q.translation.y, q.translation.z];
        let piv = rig.rest(s.bone);
        // x' = R (x - piv) + t
        let gwn = Xf { r, t: sub(t, rapply(&r, piv)) };
        moved = moved.max(len(sub(t, key_pos[si])));
        if std::env::var_os("RAGDOLL_DBG").is_some() {
            eprintln!("  seg {:<16} hull {:4} key ({:.2},{:.2},{:.2}) end ({:.2},{:.2},{:.2})", s.name, m.hulls[si].len(), key_pos[si][0], key_pos[si][1], key_pos[si][2], t[0], t[1], t[2]);
        }
        seg_g.push(inv_pw.mul(&gwn));
    }
    for b in 0..av.bones.len() {
        let si = m.own[b];
        let sb = segs[si].bone;
        // rigidly attached to its segment as in the key pose
        let rel = xf_inv(&g_key[sb]).mul(&g_key[b]);
        gs[b] = seg_g[si].mul(&rel);
    }
    // penetration of the final hulls against the statics
    let mut pen = 0.0f64;
    for &h in &handles {
        let body = &w.bodies[h];
        for &ch in body.colliders() {
            let col = &w.colliders[ch];
            let shape = col.shared_shape().clone();
            let cp = *col.position();
            for (oh, oc) in w.intersect_shape(cp, &*shape, QueryFilter::default()) {
                if oh == ch || oc.parent().is_some() {
                    continue;
                }
                if let Ok(Some(ct)) = rapier3d_f64::parry::query::contact(&cp, &*shape, oc.position(), oc.shape(), 0.0) {
                    pen = pen.max(-ct.dist);
                }
            }
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if std::env::var_os("RAGDOLL_DBG").is_some() {
        eprintln!("RAGDOLL bodies {} statics {} (tris {}) steps {} moved {:.3} pen {:.4} {:.0} ms (statics {:.0} ms)", segs.len(), n_static, n_tri, steps, moved, pen, ms, t_static);
    }
    Some(Settled { g: gs, steps, ms, moved, pen, bodies: segs.len() })
}
