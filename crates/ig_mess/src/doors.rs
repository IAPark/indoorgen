//! Room doors: open angle varied (state of the door leaf around its hinge). Owner: mess-states worker.
//! The opening pieces are built outside the solver state, so this works on their world meshes: the current angle is estimated from the
//! leaf's far edge, the leaf (+ glass / louver / handle pieces) is rotated about the hinge, and the swept leaf is checked against the
//! furniture's collision proxies.
use crate::MessCfg;
use ig_core::mesh::{Mat4, Mesh};
use ig_furn::state::State;
use ig_rooms::solidify::{CutKind, Cutter};

/// `items`: (piece name, world mesh) of every opening piece; door pieces are named `door{cutter index}[.suffix]`.
/// Returns (doors changed, doors total).
pub fn open_doors(items: &mut [(&str, &mut Mesh)], cutters: &[Cutter], door_width: f64, door_size: f64, wall_t: f64, state: &State, cfg: &MessCfg) -> (usize, usize) {
    use parry3d_f64::math::{Isometry, Point};
    use parry3d_f64::query;
    use parry3d_f64::shape::SharedShape;
    if !cfg.on() {
        return (0, 0);
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    let mut rng = cfg.rng("states.doors");
    let furn: Vec<&String> = state.objs.iter().filter(|(_, o)| o.generator.is_some()).map(|(k, _)| k).collect();
    let (mut changed, mut total) = (0, 0);
    for (ci, ct) in cutters.iter().enumerate() {
        if ct.kind == CutKind::Window {
            continue;
        }
        let base = format!("door{ci}");
        let idx: Vec<usize> = (0..items.len()).filter(|&i| items[i].0 == base || items[i].0.strip_prefix(&base).map_or(false, |r| r.starts_with('.'))).collect();
        let Some(&body) = idx.first() else { continue };
        total += 1;
        let (u_hit, u_d) = (rng.f64(), rng.f64());
        let to_c = Mat4::rot_z(-ct.rot_z).mul(&Mat4::translation([-ct.loc[0], -ct.loc[1], -ct.loc[2]]));
        let from_c = Mat4::translation(ct.loc).mul(&Mat4::rot_z(ct.rot_z));
        let pivot = [door_width / 2.0, wall_t / 2.0];
        // current angle from the body's farthest point from the hinge, in the cutter frame
        let mut far = (0.0, [0.0; 2]);
        for p in &items[body].1.pos {
            let q = to_c.apply(*p);
            let d = ((q[0] - pivot[0]).powi(2) + (q[1] - pivot[1]).powi(2)).sqrt();
            if d > far.0 {
                far = (d, [q[0] - pivot[0], q[1] - pivot[1]]);
            }
        }
        if far.0 < 0.3 {
            continue;
        }
        // signed leaf angle: the leaf swings to +y or -y of the doorway depending on the cutter
        let th_s = far.1[1].atan2(-far.1[0]);
        let sg = if th_s < -0.05 { -1.0 } else { 1.0 };
        let theta = th_s.abs();
        if std::env::var("MESS_DUMP_DOORS").is_ok() {
            eprintln!("ROOMDOOR {base} theta={theta:.2} leaf={:.2} far={:?}", far.0, far.1);
        }
        if theta > 1.8 || u_hit >= 0.3 + 0.7 * lvl {
            continue;
        }
        // new angle: mostly opened further, sometimes closed up
        let target = (theta + lvl * (u_d * 1.6 - 0.5)).clamp(0.0, 1.45);
        let leaf_len = far.0;
        let mut applied = false;
        for f in [1.0, 0.6, 0.3] {
            let th2 = theta + (target - theta) * f;
            // swept leaf: hull of the leaf slab at theta, mid and th2 (cutter frame -> world)
            let mut pts = vec![];
            for &t in &[theta, (theta + th2) / 2.0, th2] {
                let dir = [-t.cos(), sg * t.sin()];
                for &s in &[0.0, leaf_len] {
                    for &w in &[-0.03, 0.03] {
                        for &z in &[-door_size / 2.0 + 0.05, door_size / 2.0 - 0.05] {
                            let q = from_c.apply([pivot[0] + dir[0] * s - dir[1] * w, pivot[1] + dir[1] * s + dir[0] * w, z]);
                            pts.push(Point::new(q[0], q[1], q[2]));
                        }
                    }
                }
            }
            let Some(hull) = SharedShape::convex_hull(&pts) else { break };
            let hit = furn.iter().any(|k| ig_furn::col::proxies(state, k).iter().any(|px| query::intersection_test(&Isometry::identity(), hull.as_ref(), &px.iso, px.shape.as_ref()).unwrap_or(false)));
            if hit {
                continue;
            }
            let delta = th2 - theta;
            if delta.abs() < 0.02 {
                break;
            }
            let rot = from_c.mul(&Mat4::translation([pivot[0], pivot[1], 0.0])).mul(&Mat4::rot_z(-sg * delta)).mul(&Mat4::translation([-pivot[0], -pivot[1], 0.0])).mul(&to_c);
            for &i in &idx {
                items[i].1.transform(&rot);
            }
            applied = true;
            break;
        }
        if applied {
            changed += 1;
        }
    }
    (changed, total)
}
