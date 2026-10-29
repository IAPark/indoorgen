//! Place settings on dining tables (after the solve): the solver scatters forks / knives / spoons anywhere on the top, at any angle
//! (no real table looks like that). Each utensil standing on a TableDiningFactory is laid beside one of that table's plates, long axis
//! at right angles to the nearest table edge -- fork left of the plate, knife and spoon right -- seen from the diner's side. Utensils
//! on a table with no plate, beyond one of each kind per plate, or that would leave the top are removed.
use crate::furn::state::State;

const UTENSILS: [(&str, f64); 4] = [("ForkFactory", -1.0), ("KnifeFactory", 1.0), ("SpoonFactory", 1.6), ("ChopsticksFactory", 1.2)];

pub fn arrange(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    let tables: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "TableDiningFactory")).map(|(k, _)| k.clone()).collect();
    let mut remove: Vec<String> = vec![];
    for tk in tables {
        let t = state.objs[&tk].clone();
        let on = |o: &crate::furn::state::ObjState| o.relations.iter().any(|r| r.target == tk);
        let class = |o: &crate::furn::state::ObjState| o.generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
        let plates: Vec<String> = state.objs.iter().filter(|(_, o)| on(o) && class(o) == "PlateFactory").map(|(k, _)| k.clone()).collect();
        let utensils: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| on(o)).filter_map(|(k, o)| { let c = class(o); UTENSILS.iter().any(|u| u.0 == c).then(|| (k.clone(), c)) }).collect();
        if utensils.is_empty() { continue }
        if plates.is_empty() { remove.extend(utensils.iter().map(|u| u.0.clone())); log.push(format!("{} utensils removed from {tk} (no plate)", utensils.len())); continue }
        // table local frame (rigid): world -> local
        let r = t.pose.r; let tt = t.pose.t;
        let to_local = |p: [f64; 3]| { let d = [p[0] - tt[0], p[1] - tt[1], p[2] - tt[2]]; [r[0][0] * d[0] + r[1][0] * d[1] + r[2][0] * d[2], r[0][1] * d[0] + r[1][1] * d[1] + r[2][1] * d[2]] };
        let (lo, hi) = (t.geom.lo, t.geom.hi);
        let mut used: std::collections::HashSet<(usize, String)> = Default::default();
        let mut n_set = 0;
        for (i, (uk, uc)) in utensils.iter().enumerate() {
            // the plate with this kind still free, starting round-robin
            let Some(pi) = (0..plates.len()).map(|j| (i + j) % plates.len()).find(|j| !used.contains(&(*j, uc.clone()))) else { remove.push(uk.clone()); continue };
            used.insert((pi, uc.clone()));
            let p = &state.objs[&plates[pi]];
            let pc = p.pose.t;
            let pr = 0.5 * (p.geom.hi[0] - p.geom.lo[0]).max(p.geom.hi[1] - p.geom.lo[1]);
            // nearest edge of the top to the plate (local), its outward normal in world
            let l = to_local(pc);
            let d = [(l[0] - lo[0], [-1.0, 0.0]), (hi[0] - l[0], [1.0, 0.0]), (l[1] - lo[1], [0.0, -1.0]), (hi[1] - l[1], [0.0, 1.0])];
            let nl = d.iter().min_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).unwrap().1;
            let nw = [r[0][0] * nl[0] + r[0][1] * nl[1], r[1][0] * nl[0] + r[1][1] * nl[1]];
            // diner faces -n: their left is along (n.y, -n.x) ... rotate n by +90 deg gives the diner's right
            let right = [-nw[1], nw[0]];
            let side = UTENSILS.iter().find(|u| u.0 == uc.as_str()).unwrap().1;
            let off = side.signum() * (pr + 0.035) + (side - side.signum()) * 0.06;
            let u = &state.objs[uk];
            let (ul, uh) = (u.geom.lo, u.geom.hi);
            let long_x = uh[0] - ul[0] >= uh[1] - ul[1];
            let pos = [pc[0] + right[0] * off, pc[1] + right[1] * off, u.pose.t[2]];
            let ll = to_local(pos);
            if ll[0] < lo[0] + 0.03 || ll[0] > hi[0] - 0.03 || ll[1] < lo[1] + 0.03 || ll[1] > hi[1] - 0.03 { remove.push(uk.clone()); continue }
            let yaw = nw[1].atan2(nw[0]) - if long_x { 0.0 } else { std::f64::consts::FRAC_PI_2 };
            let u = state.objs.get_mut(uk).unwrap();
            let e = u.pose.o.eul;
            u.pose.set_euler([e[0] as f64, e[1] as f64, yaw]);
            // origin -> footprint centre offset rotates with the object: place the footprint centre, not the origin
            let c = [(ul[0] + uh[0]) / 2.0, (ul[1] + uh[1]) / 2.0, 0.0];
            let cw = u.pose.rot(c);
            u.pose.set_loc([pos[0] - cw[0], pos[1] - cw[1], pos[2]]);
            n_set += 1;
        }
        log.push(format!("{n_set} utensils set beside {} plates on {tk}", plates.len()));
    }
    remove.sort(); remove.dedup();
    for k in &remove { state.objs.shift_remove(k); }
    if !remove.is_empty() { log.push(format!("removed {}", remove.len())); }
    log
}
