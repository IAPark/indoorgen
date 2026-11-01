//! Place settings on dining tables (after the solve): the solver scatters forks / knives / spoons anywhere on the top, at any angle
//! (no real table looks like that). Each utensil standing on a TableDiningFactory is laid beside one of that table's plates, long axis
//! at right angles to the nearest table edge -- fork left of the plate, knife and spoon right -- seen from the diner's side. Utensils
//! on a table with no plate, beyond one of each kind per plate, or that would leave the top are removed.
use crate::furn::state::State;

// (chopsticks left of the fork: at 1.2 they lay on the knife and the plate's rim)
const UTENSILS: [(&str, f64); 4] = [("ForkFactory", -1.0), ("KnifeFactory", 1.0), ("SpoonFactory", 1.6), ("ChopsticksFactory", -1.8)];

pub fn arrange(state: &mut State) -> Vec<String> {
    let mut log = vec![];
    // (coffee tables too, for the cookware / oversized pieces only: 0.4 m bowls stood on them like basins)
    let tables: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "TableDiningFactory" || g.class == "CoffeeTableFactory")).map(|(k, _)| k.clone()).collect();
    let mut remove: Vec<String> = vec![];
    // on any furniture top: bowls over 0.3 m (basin-sized, on nightstands too) and pot-shaped fruit containers go
    for (k, o) in &state.objs {
        let c = o.generator.as_ref().map(|g| g.class.as_str()).unwrap_or("");
        if !matches!(c, "BowlFactory" | "FruitContainerFactory" | "VaseFactory") { continue }
        if !o.relations.iter().any(|r| state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())) { continue }
        let w = (o.geom.hi[0] - o.geom.lo[0]).max(o.geom.hi[1] - o.geom.lo[1]);
        let h = o.geom.hi[2] - o.geom.lo[2];
        // (vases over 0.4 m stood on dining tables like floor vases)
        if (c == "BowlFactory" && w > 0.25) || (c == "FruitContainerFactory" && (h > 0.55 * w || w > 0.4)) || (c == "VaseFactory" && h > 0.4) { remove.push(k.clone()); }
    }
    if !remove.is_empty() { log.push(format!("{} oversized bowls / pot-shaped containers removed", remove.len())); }
    for tk in tables {
        let t = state.objs[&tk].clone();
        let dining = t.generator.as_ref().map_or(false, |g| g.class == "TableDiningFactory");
        let on = |o: &crate::furn::state::ObjState| o.relations.iter().any(|r| r.target == tk);
        let class = |o: &crate::furn::state::ObjState| o.generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
        let plates: Vec<String> = state.objs.iter().filter(|(_, o)| on(o) && class(o) == "PlateFactory").map(|(k, _)| k.clone()).collect();
        // cookware (pots, pans) does not stand on a dining table, nor a piece wider than 0.4 m, nor a bowl wider than 0.3 m (the solver set
        // bowls of 0.35 m boxes there; their models came out wider still, like basins)
        // (on the top: chairs relate to their table too)
        let top_z = t.pose.t[2] + t.geom.lo[2] + if dining { 0.4 } else { 0.2 };
        let odd: Vec<String> = state.objs.iter().filter(|(_, o)| on(o) && o.pose.t[2] + o.geom.lo[2] > top_z).filter(|(_, o)| {
            let c = class(o);
            let w = (o.geom.hi[0] - o.geom.lo[0]).max(o.geom.hi[1] - o.geom.lo[1]);
            // (a fruit container deeper than about half its width reads as a stock pot)
            let h = o.geom.hi[2] - o.geom.lo[2];
            matches!(c.as_str(), "PotFactory" | "PanFactory" | "LidFactory" | "KettleFactory") || (c != "PlateFactory" && w > 0.4) || (c == "BowlFactory" && w > 0.25)
                || (c == "FruitContainerFactory" && h > 0.55 * w)
        }).map(|(k, _)| k.clone()).collect();
        if !odd.is_empty() { log.push(format!("{} cookware / oversized pieces removed from {tk}", odd.len())); }
        remove.extend(odd);
        if !dining { continue }
        let utensils: Vec<(String, String)> = state.objs.iter().filter(|(_, o)| on(o)).filter_map(|(k, o)| { let c = class(o); UTENSILS.iter().any(|u| u.0 == c).then(|| (k.clone(), c)) }).collect();
        if utensils.is_empty() { continue }
        if plates.is_empty() { remove.extend(utensils.iter().map(|u| u.0.clone())); log.push(format!("{} utensils removed from {tk} (no plate)", utensils.len())); continue }
        // table local frame (rigid): world -> local
        let r = t.pose.r; let tt = t.pose.t;
        let to_local = |p: [f64; 3]| { let d = [p[0] - tt[0], p[1] - tt[1], p[2] - tt[2]]; [r[0][0] * d[0] + r[1][0] * d[1] + r[2][0] * d[2], r[0][1] * d[0] + r[1][1] * d[1] + r[2][1] * d[2]] };
        let (lo, hi) = (t.geom.lo, t.geom.hi);
        let mut used: std::collections::HashSet<(usize, String)> = Default::default();
        let mut n_set = 0;
        // (cutlery not yet set still stands where the solver left it: only the ones already set count as obstacles)
        let ukeys: std::collections::HashSet<String> = utensils.iter().map(|u| u.0.clone()).collect();
        let mut placed: std::collections::HashSet<String> = Default::default();
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
            // (6 cm between neighbours: at 3.6 cm a knife lay on the spoon's bowl)
            let off = side.signum() * (pr + 0.035) + (side - side.signum()) * 0.1;
            let u = &state.objs[uk];
            let (ul, uh) = (u.geom.lo, u.geom.hi);
            let long_x = uh[0] - ul[0] >= uh[1] - ul[1];
            // (on the top: kept at the solver's height, chopsticks lay 8 mm into the table)
            let pos = [pc[0] + right[0] * off, pc[1] + right[1] * off, t.pose.t[2] + t.geom.hi[2] - ul[2] + 0.001];
            let ll = to_local(pos);
            if ll[0] < lo[0] + 0.03 || ll[0] > hi[0] - 0.03 || ll[1] < lo[1] + 0.03 || ll[1] > hi[1] - 0.03 { remove.push(uk.clone()); continue }
            let yaw = nw[1].atan2(nw[0]) - if long_x { 0.0 } else { std::f64::consts::FRAC_PI_2 };
            // clear of everything else on the table but its plate (a fork lay on a bowl's slope): the footprint after the turn, 1 cm margin
            let (cy, sy) = (yaw.cos(), yaw.sin());
            let half = [(uh[0] - ul[0]) / 2.0, (uh[1] - ul[1]) / 2.0];
            let (ex, ey) = (half[0] * cy.abs() + half[1] * sy.abs() + 0.01, half[0] * sy.abs() + half[1] * cy.abs() + 0.01);
            let clash = state.objs.iter().any(|(ok, oo)| {
                if ok == uk || *ok == plates[pi] || !on(oo) || oo.pose.t[2] + oo.geom.lo[2] < top_z - 0.3 || (ukeys.contains(ok) && !placed.contains(ok)) || remove.contains(ok) { return false }
                let (ol, oh) = (oo.geom.lo, oo.geom.hi);
                let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
                for i in 0..4 { let q = oo.pose.apply([if i & 1 == 0 { ol[0] } else { oh[0] }, if i & 2 == 0 { ol[1] } else { oh[1] }, 0.0]); for d in 0..2 { a[d] = a[d].min(q[d]); b[d] = b[d].max(q[d]); } }
                a[0] < pos[0] + ex && b[0] > pos[0] - ex && a[1] < pos[1] + ey && b[1] > pos[1] - ey
            });
            if clash { remove.push(uk.clone()); continue }
            let u = state.objs.get_mut(uk).unwrap();
            let e = u.pose.o.eul;
            u.pose.set_euler([e[0] as f64, e[1] as f64, yaw]);
            // origin -> footprint centre offset rotates with the object: place the footprint centre, not the origin
            let c = [(ul[0] + uh[0]) / 2.0, (ul[1] + uh[1]) / 2.0, 0.0];
            let cw = u.pose.rot(c);
            u.pose.set_loc([pos[0] - cw[0], pos[1] - cw[1], pos[2]]);
            placed.insert(uk.clone());
            n_set += 1;
        }
        log.push(format!("{n_set} utensils set beside {} plates on {tk}", plates.len()));
    }
    remove.sort(); remove.dedup();
    for k in &remove { state.objs.shift_remove(k); }
    if !remove.is_empty() { log.push(format!("removed {}", remove.len())); }
    log
}

/// Dining chairs the solver left against the walls (their "near the table" relation is soft; cramped plans park them in pairs along
/// a wall): a chair whose seat is more than 0.8 m from its table's edge moves to a free slot around the table, facing it (slots every
/// ~0.6 m along each side, layout_rules.chair_tuck under the top); a slot must lie in the chair's room and clear every other object. No free slot: removed.
pub fn gather_chairs(state: &mut State) -> Vec<String> {
    type B = ([f64; 3], [f64; 3]);
    let wbox = |o: &crate::furn::state::ObjState| -> B {
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let ov = |a: &B, b: &B| (0..2).all(|d| a.0[d] < b.1[d] - 0.01 && b.0[d] < a.1[d] - 0.01) && a.0[2] < b.1[2] && b.0[2] < a.1[2];
    let mut log = vec![];
    let tables: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "TableDiningFactory")).map(|(k, _)| k.clone()).collect();
    let mut remove = vec![];
    for tk in tables {
        let t = state.objs[&tk].clone();
        let (r, tt) = (t.pose.r, t.pose.t);
        let to_local = |p: [f64; 3]| { let d = [p[0] - tt[0], p[1] - tt[1]]; [r[0][0] * d[0] + r[1][0] * d[1], r[0][1] * d[0] + r[1][1] * d[1]] };
        let to_world = |l: [f64; 2]| [tt[0] + r[0][0] * l[0] + r[0][1] * l[1], tt[1] + r[1][0] * l[0] + r[1][1] * l[1]];
        let (lo, hi) = (t.geom.lo, t.geom.hi);
        let chairs: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| g.class == "ChairFactory") && o.relations.iter().any(|x| x.target == tk)).map(|(k, _)| k.clone()).collect();
        // seat centre of a chair (local frame: back at x = lo, front toward +x)
        let seat = |o: &crate::furn::state::ObjState| o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]);
        let _far: Vec<String> = chairs.iter().filter(|k| {
            let l = to_local(seat(&state.objs[k.as_str()]));
            let dx = (lo[0] - l[0]).max(l[0] - hi[0]).max(0.0); let dy = (lo[1] - l[1]).max(l[1] - hi[1]).max(0.0);
            if std::env::var_os("CHAIR_DEBUG").is_some() { eprintln!("CHAIRDBG {k} local {:.2?} table lo {:.2?} hi {:.2?} dist {:.2}", l, [lo[0], lo[1]], [hi[0], hi[1]], dx.hypot(dy)); }
            dx.hypot(dy) > 0.8
        }).cloned().collect();
        // every chair takes its own slot: five chairs at a 1 m square table shared sides and overlapped
        let far = chairs.clone();
        if far.is_empty() { continue }
        let room = state.objs[&far[0]].relations.iter().map(|x| x.target.clone()).find(|x| state.objs.get(x).map_or(false, |o| o.polygon.is_some()));
        let poly = room.and_then(|k| state.objs[&k].polygon.clone());
        // a round table seats its chairs at the four diagonals when the four sides leave some out (one chair against a wall
        // went missing and the table read as half set)
        let round = t.tags.contains(crate::furn::tags::Tag::sem("RoundTable"));
        let saved: Vec<(String, crate::furn::geom::Pose)> = far.iter().map(|k| (k.clone(), state.objs[k].pose)).collect();
        let mut best: Option<(usize, Vec<(String, crate::furn::geom::Pose)>, Vec<String>, usize)> = None;
        for diag in if round { vec![false, true] } else { vec![false] } {
        for (k, p) in &saved { state.objs.get_mut(k).unwrap().pose = *p; }
        let mut unseated: Vec<String> = vec![];
        // candidate slots: (local position on the edge, outward normal)
        let mut slots: Vec<([f64; 2], [f64; 2])> = vec![];
        // fill order: the middle slots of the long sides first, alternating sides, then outward, then the short sides -- a set that
        // reads balanced (chairs took the slot nearest where the solver left them: two on one side, none opposite)
        let mut prio: Vec<(usize, usize)> = vec![];
        let long_axis = if hi[1] - lo[1] >= hi[0] - lo[0] { 0usize } else { 1 };
        for (si, (axis, sign)) in [(0usize, 1.0f64), (0, -1.0), (1, 1.0), (1, -1.0)].into_iter().enumerate() {
            let o = 1 - axis; let len = hi[o] - lo[o];
            let n = ((len / 0.6).floor() as usize).max(1);
            for i in 0..n {
                let mut p = [0.0; 2];
                p[axis] = if sign > 0.0 { hi[axis] } else { lo[axis] };
                p[o] = lo[o] + len * (i as f64 + 0.5) / n as f64;
                let mut nl = [0.0; 2]; nl[axis] = sign;
                slots.push((p, nl));
                let from_mid = ((i as f64 + 0.5) - n as f64 / 2.0).abs().floor() as usize;
                prio.push((if axis == long_axis { from_mid } else { 100 + from_mid }, si % 2));
            }
        }
        if diag {
            let (c, rad) = ([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], (hi[0] - lo[0]).min(hi[1] - lo[1]) / 2.0);
            slots = (0..4).map(|k| { let a = std::f64::consts::FRAC_PI_4 + k as f64 * std::f64::consts::FRAC_PI_2; ([c[0] + rad * a.cos(), c[1] + rad * a.sin()], [a.cos(), a.sin()]) }).collect();
            prio = (0..4).map(|k| (0, k % 2)).collect();
        }
        let mut moved = 0;
        let mut taken: Vec<usize> = vec![];
        // the chairs seated so far keep 6 cm apart (two at a corner met arm to arm)
        let mut seated: Vec<B> = vec![];
        for ck in &far {
            let c = state.objs[ck].clone();
            let (cl, ch) = (c.geom.lo, c.geom.hi);
            let depth = ch[0] - cl[0];
            // (door clearances too: a chair stood 5 cm into a doorway)
            // (and 0.6 m around a passage, which has no clearance of its own)
            let doors: Vec<B> = state.objs.iter().filter(|(k, o)| k.starts_with("doorclear/") || (k.starts_with("passage") && o.generator.is_none())).map(|(k, o)| {
                let mut b = wbox(o); let m = if k.starts_with("passage") { 0.6 } else { 0.0 };
                for d in 0..2 { b.0[d] -= m; b.1[d] += m; } b.0[2] = -10.0; b.1[2] = 10.0; b }).collect();
            let others: Vec<B> = doors.into_iter().chain(state.objs.iter().filter(|(k, o)| *k != ck && *k != &tk && o.generator.is_some() && !o.generator.as_ref().unwrap().class.contains("Rug") && !o.relations.iter().any(|x| x.target == tk && o.generator.as_ref().unwrap().class != "ChairFactory")).map(|(_, o)| wbox(o))).collect();
            let mut done = false;
            let here = to_local(seat(&c));
            let mut order: Vec<usize> = (0..slots.len()).filter(|i| !taken.contains(i)).collect();
            order.sort_by(|&a, &b| { let d = |i: usize| (slots[i].0[0] - here[0]).hypot(slots[i].0[1] - here[1]); prio[a].0.cmp(&prio[b].0).then(d(a).partial_cmp(&d(b)).unwrap()) });
            for si in order {
                let (p, nl) = (&slots[si].0, &slots[si].1);
                // seat centre 0.5 depth out from the edge minus the tuck (layout_rules.chair_tuck: the seat's front under the top, as
                // chairs are left at a table); chair faces -n (its +x toward the table)
                let off = 0.5 * depth - crate::furn::layout_rules::get().chair_tuck;
                let sc = to_world([p[0] + nl[0] * off, p[1] + nl[1] * off]);
                let nw = [r[0][0] * nl[0] + r[0][1] * nl[1], r[1][0] * nl[0] + r[1][1] * nl[1]];
                let yaw = (-nw[1]).atan2(-nw[0]);
                let mut np = c.pose;
                let e = np.o.eul; np.set_euler([e[0] as f64, e[1] as f64, yaw]);
                let cc = np.rot([(cl[0] + ch[0]) / 2.0, (cl[1] + ch[1]) / 2.0, 0.0]);
                np.set_loc([sc[0] - cc[0], sc[1] - cc[1], c.pose.t[2]]);
                let mut cand = c.clone(); cand.pose = np;
                let b = wbox(&cand);
                if seated.iter().any(|o| (0..2).all(|d| b.0[d] < o.1[d] + 0.06 && o.0[d] < b.1[d] + 0.06)) { continue }
                if others.iter().any(|o| ov(&b, o)) {
                    if std::env::var_os("CHAIR_DEBUG").is_some() {
                        let hit: Vec<&String> = state.objs.iter().filter(|(k, o)| *k != ck && *k != &tk && o.generator.is_some() && ov(&b, &wbox(o))).map(|(k, _)| k).collect();
                        eprintln!("CHAIRDBG {ck} slot {si} diag {diag}: hits {hit:?}");
                    }
                    continue
                }
                if let Some(pg) = &poly {
                    if ![[b.0[0], b.0[1]], [b.1[0], b.0[1]], [b.0[0], b.1[1]], [b.1[0], b.1[1]]].iter().all(|q| crate::shp::contains_pt(pg, *q)) { continue }
                }
                state.objs.get_mut(ck).unwrap().pose = np;
                seated.push(b);
                taken.push(si);
                done = true; moved += 1; break;
            }
            if !done { unseated.push(ck.clone()); }
        }
        if best.as_ref().map_or(true, |b| moved > b.0) {
            best = Some((moved, far.iter().map(|k| (k.clone(), state.objs[k].pose)).collect(), unseated.clone(), slots.len()));
        }
        if moved == far.len() { break }
        }
        let (moved, poses, unseated, nslots) = best.unwrap();
        for (k, p) in poses { state.objs.get_mut(&k).unwrap().pose = p; }
        remove.extend(unseated.iter().cloned());
        let slots_len = nslots;
        log.push(format!("{} of {} chairs seated at {tk} ({} slots, {} removed)", moved, far.len(), slots_len, far.len() - moved));
        // a narrow room: chairs that cut the path between its doors go, one at a time (keeping two at the table)
        if let Some(room) = crate::furn::layout2d::room_of(state, &tk) {
            // first the whole set slid up to 0.4 m (a table with chairs on one side only read as half set)
            if !crate::furn::layout2d::walk_status(state, &room).0 {
                use crate::furn::layout2d as l2;
                let group: Vec<String> = std::iter::once(tk.clone()).chain(far.iter().filter(|k| !unseated.contains(k) && state.objs.contains_key(k.as_str())).cloned()).collect();
                let saved: Vec<(String, crate::furn::geom::Pose)> = group.iter().map(|k| (k.clone(), state.objs[k.as_str()].pose)).collect();
                let clear = |st: &crate::furn::state::State| group.iter().all(|k| l2::outside_room_depth(st, k, &room) < 1e-3
                    && st.objs.iter().all(|(o, b)| group.contains(o) || b.generator.is_none() && !o.starts_with("doorclear/") && !o.starts_with("door") || l2::is_room(b) || b.tags.contains(crate::furn::tags::Tag::sem("NoCollision")) || b.relations.iter().any(|r| group.contains(&r.target)) || l2::depth(st, k, o) <= 1e-4));
                let mut found = false;
                'o: for step in 1..=8 {
                    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        let d = step as f64 * 0.05;
                        for (k, p) in &saved { let mut q = *p; q.translate([dx * d, dy * d, 0.0]); state.set_pose(k, q); state.touch(k); }
                        if crate::furn::layout2d::walk_status(state, &room).0 && clear(state) { found = true; log.push(format!("table set {tk} slid {d:.2} m to open the path")); break 'o }
                    }
                }
                if !found { for (k, p) in &saved { state.set_pose(k, *p); state.touch(k); } }
            }
            loop {
                let (ok, _) = crate::furn::layout2d::walk_status(state, &room);
                let seated: Vec<String> = far.iter().filter(|k| !unseated.contains(k) && !remove.contains(k) && state.objs.contains_key(k.as_str())).cloned().collect();
                if ok || seated.len() <= 2 { break }
                let mut fix = None;
                for c in &seated {
                    let o = state.objs.shift_remove(c).unwrap();
                    let good = crate::furn::layout2d::walk_status(state, &room).0;
                    state.objs.insert(c.clone(), o);
                    if good { fix = Some(c.clone()); break }
                }
                // no single chair: the one standing nearest a wall goes, and the next pass tries again
                let c = fix.unwrap_or_else(|| seated[0].clone());
                state.objs.shift_remove(&c);
                remove.push(c.clone());
                log.push(format!("chair {c} removed (it cut the path between the doors)"));
            }
        }
    }
    // and anything resting on a removed chair
    let mut all = remove.clone();
    loop {
        let more: Vec<String> = state.objs.iter().filter(|(k, o)| !all.contains(k) && o.relations.iter().any(|x| all.contains(&x.target))).map(|(k, _)| k.clone()).collect();
        if more.is_empty() { break }
        all.extend(more);
    }
    for k in &all { state.objs.shift_remove(k); }
    log
}

/// Coffee tables beside the sofa (the solver's "near" relation lets them sit at the sofa's end): each coffee table within 3 m of a sofa
/// in its room moves in front of that sofa -- centred on it, 0.45 m from the seat front, long side parallel to the sofa -- when that spot
/// is in the room and clear of other furniture (rugs excepted); objects resting on it move along.
pub fn coffee_tables(state: &mut State) -> Vec<String> {
    type B = ([f64; 3], [f64; 3]);
    let wbox = |o: &crate::furn::state::ObjState| -> B {
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let cls = |o: &crate::furn::state::ObjState| o.generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
    let room_of = |o: &crate::furn::state::ObjState| o.relations.iter().map(|x| x.target.clone()).find(|t| t.contains('/'));
    let mut log = vec![];
    let tables: Vec<String> = state.objs.iter().filter(|(_, o)| cls(o) == "CoffeeTableFactory").map(|(k, _)| k.clone()).collect();
    let mut used_sofas: Vec<String> = vec![];
    for tk in tables {
        let t = state.objs[&tk].clone();
        let Some(room) = room_of(&t) else { continue };
        let tc = t.pose.apply([(t.geom.lo[0] + t.geom.hi[0]) / 2.0, (t.geom.lo[1] + t.geom.hi[1]) / 2.0, 0.0]);
        // nearest sofa in the same room
        let sofa = state.objs.iter().filter(|(k, o)| cls(o) == "SofaFactory" && room_of(o).as_deref() == Some(room.as_str()) && !used_sofas.contains(k)).map(|(k, o)| {
            let c = o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]);
            (k.clone(), (c[0] - tc[0]).hypot(c[1] - tc[1]))
        }).filter(|x| x.1 < 3.0).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let Some((sk, _)) = sofa else { continue };
        let s = state.objs[&sk].clone();
        // sofa frame: x = depth (back at lo, seat front at hi), y = width
        let front = s.pose.rot([1.0, 0.0, 0.0]);
        let ylen = (s.geom.hi[1] - s.geom.lo[1]).max(0.1);
        let (tl, th) = (t.geom.lo, t.geom.hi);
        let (tx, ty) = (th[0] - tl[0], th[1] - tl[1]);
        // table's long axis along the sofa's width; its half-depth toward the sofa
        let long_x = tx >= ty;
        let half_d = 0.5 * if long_x { ty } else { tx };
        let dist = 0.45 + half_d;
        let seat_front = s.pose.apply([s.geom.hi[0], (s.geom.lo[1] + s.geom.hi[1]) / 2.0, 0.0]);
        let c = [seat_front[0] + front[0] * dist, seat_front[1] + front[1] * dist];
        let sofa_y = s.pose.rot([0.0, 1.0, 0.0]);
        let yaw = if long_x { sofa_y[1].atan2(sofa_y[0]) } else { front[1].atan2(front[0]) };
        let _ = ylen;
        let mut np = t.pose;
        let e = np.o.eul; np.set_euler([e[0] as f64, e[1] as f64, yaw]);
        let cc = np.rot([(tl[0] + th[0]) / 2.0, (tl[1] + th[1]) / 2.0, 0.0]);
        np.set_loc([c[0] - cc[0], c[1] - cc[1], t.pose.t[2]]);
        let mut cand = t.clone(); cand.pose = np;
        let b = wbox(&cand);
        let kids: Vec<String> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|x| x.target == tk)).map(|(k, _)| k.clone()).collect();
        let blocked = state.objs.iter().any(|(k, o)| *k != tk && !kids.contains(k) && o.generator.is_some() && !cls(o).contains("Rug") && {
            let ob = wbox(o); (0..2).all(|d| b.0[d] < ob.1[d] - 0.01 && ob.0[d] < b.1[d] - 0.01) && b.0[2] < ob.1[2] && ob.0[2] < b.1[2]
        });
        let inside = state.objs.get(&room).and_then(|r| r.polygon.clone()).map_or(true, |pg| [[b.0[0], b.0[1]], [b.1[0], b.0[1]], [b.0[0], b.1[1]], [b.1[0], b.1[1]]].iter().all(|q| crate::shp::contains_pt(&pg, *q)));
        if blocked || !inside { log.push(format!("{tk}: front of {sk} blocked")); continue }
        // move the table and its children rigidly (rotation about the old table centre by the yaw change, then translation)
        let dyaw = yaw - t.pose.o.eul[2] as f64;
        let old_c = tc;
        let new_c = [c[0], c[1]];
        for kk in &kids {
            let o = state.objs.get_mut(kk).unwrap();
            let (dx, dy) = (o.pose.t[0] - old_c[0], o.pose.t[1] - old_c[1]);
            let (cs, sn) = (dyaw.cos(), dyaw.sin());
            let e = o.pose.o.eul;
            o.pose.set_euler([e[0] as f64, e[1] as f64, e[2] as f64 + dyaw]);
            o.pose.set_loc([new_c[0] + cs * dx - sn * dy, new_c[1] + sn * dx + cs * dy, o.pose.t[2]]);
        }
        state.objs.get_mut(&tk).unwrap().pose = np;
        used_sofas.push(sk.clone());
        log.push(format!("{tk} set in front of {sk} ({} items along)", kids.len()));
    }
    log
}
