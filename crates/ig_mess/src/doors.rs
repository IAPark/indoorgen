//! Room doors: open angle varied (state of the door leaf around its hinge). Owner: mess-states worker.
//! The opening pieces are built outside the solver state, so this works on their world meshes: the current angle is estimated from the
//! leaf's far edge, the leaf (+ glass / louver / handle pieces) is rotated about the hinge, and the swept leaf is checked against the
//! furniture's collision proxies.
use crate::MessCfg;
use ig_core::mesh::{Mat4, Mesh};
use ig_furn::state::State;
use ig_rooms::solidify::{CutKind, Cutter};

/// Mess pass for room doors: varies the DESIRED open angle of each door (`(cutter index, angle in radians)`, python's draw on
/// entry). The leaf is then swept from closed towards the angle against furniture / walls by the opening pass, which decides how far
/// it really opens. Returns (doors changed, doors total).
pub fn vary_angles(doors: &mut [(usize, f64)], cfg: &MessCfg) -> (usize, usize) {
    if !cfg.on() {
        return (0, 0);
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    let mut rng = cfg.rng("states.doors");
    let mut changed = 0;
    // one pair of draws per door in cutter order
    let mut order: Vec<usize> = (0..doors.len()).collect();
    order.sort_by_key(|&i| doors[i].0);
    for i in order {
        let (u_hit, u_d) = (rng.f64(), rng.f64());
        let theta = doors[i].1;
        if theta > 1.8 || u_hit >= 0.3 + 0.7 * lvl {
            continue;
        }
        // mostly opened further, sometimes closed up
        let target = (theta + lvl * (u_d * 1.6 - 0.5)).clamp(0.0, 1.45);
        if (target - theta).abs() > 0.02 {
            doors[i].1 = target;
            changed += 1;
        }
    }
    (changed, doors.len())
}

/// Window curtains half drawn: each curtain half (the cloth hangs at the window sides) is stretched toward the window centre so the gap
/// closes by a random fraction. Pieces are named `window{cutter index}.curtain`. Returns (changed, total).
pub fn draw_curtains(items: &mut [(&str, &mut Mesh)], cutters: &[Cutter], cfg: &MessCfg) -> (usize, usize) {
    if !cfg.on() {
        return (0, 0);
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    let mut rng = cfg.rng("states.curtains");
    let (mut changed, mut total) = (0, 0);
    for (ci, ct) in cutters.iter().enumerate() {
        if ct.kind != CutKind::Window {
            continue;
        }
        let name = format!("window{ci}.curtain");
        let Some(i) = items.iter().position(|(n, _)| *n == name) else { continue };
        total += 1;
        let (u_hit, u_l, u_r) = (rng.f64(), rng.f64(), rng.f64());
        if u_hit >= 0.35 + 0.55 * lvl {
            continue;
        }
        let to_c = Mat4::rot_z(-ct.rot_z).mul(&Mat4::translation([-ct.loc[0], -ct.loc[1], -ct.loc[2]]));
        let from_c = Mat4::translation(ct.loc).mul(&Mat4::rot_z(ct.rot_z));
        let m = &mut items[i].1;
        let xs: Vec<f64> = m.pos.iter().map(|p| to_c.apply(*p)[0]).collect();
        let hw = ct.dims[0] / 2.0;
        // inner edges of the right / left halves
        let inner_r = xs.iter().cloned().filter(|x| *x >= 0.0).fold(f64::MAX, f64::min);
        let inner_l = xs.iter().cloned().filter(|x| *x < 0.0).fold(f64::MIN, f64::max);
        if inner_r == f64::MAX || inner_l == f64::MIN {
            continue;
        }
        let (fr, fl) = (lvl * (0.25 + 0.65 * u_r), lvl * (0.25 + 0.65 * u_l));
        let (new_r, new_l) = (inner_r * (1.0 - fr), inner_l * (1.0 - fl));
        let (ext_r, ext_l) = (xs.iter().cloned().fold(f64::MIN, f64::max).max(inner_r + 1e-6), xs.iter().cloned().fold(f64::MAX, f64::min).min(inner_l - 1e-6));
        let _ = hw;
        for (p, &x) in m.pos.iter_mut().zip(&xs) {
            let mut q = to_c.apply(*p);
            q[0] = if x >= 0.0 { ext_r - (ext_r - x) * (ext_r - new_r) / (ext_r - inner_r) } else { ext_l + (x - ext_l) * (new_l - ext_l) / (inner_l - ext_l) };
            *p = from_c.apply(q);
        }
        changed += 1;
    }
    (changed, total)
}
