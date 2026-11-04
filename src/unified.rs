//! Furnish the rooms the rules cover with the unified solver (crates/ig_layout); the old passes leave those rooms empty (`Options::solver_rooms`).
//! Built-ins, doors and windows are already there. A room whose must items have no place gets what the solver did place, and says so.
use crate::furn::{groups, layout2d};
use crate::pipeline::Generated;
use ig_layout::{house, rules, solve, world};

/// the room kinds the rules cover
pub fn kinds() -> Vec<String> { rules::default_rules().into_keys().collect() }


/// the solver's answer for every covered room of a house, before anything is placed: the plan layer can still ask for another plan
pub struct Plan { rooms: Vec<(String, solve::Solution)> }

impl Plan {
    /// must items with no place, over the house
    pub fn missing(&self) -> usize { self.rooms.iter().map(|(_, s)| s.report.iter().filter(|r| r.tier == 0 && !r.placed).count()).sum() }
}

/// what makes a plan not worth keeping: must items with no place, and kitchen appliances that cannot be opened
pub fn unfit(g: &Generated, plan: &Plan) -> usize {
    // (a room without its bed outweighs any kitchen fault)
    100 * plan.missing() + g.room_polys.iter().filter(|(n, _)| n.starts_with("kitchen")).map(|(n, _)| { let e = crate::furn::kitchen_rules::eval(&g.state, n, None); if e.unusable > 0 { let cl: Vec<&str> = g.state.objs.iter().filter(|(k, _)| layout2d::room_of(&g.state, k).as_deref() == Some(n.as_str())).filter_map(|(_, o)| o.generator.as_ref().map(|gg| gg.class.as_str())).collect(); eprintln!("PLAN fault {n}: {:?} holds {cl:?}", e.broken); } e.unusable + 99 * e.absent }).sum::<usize>()
}

/// alcove shelves standing in a window's way go, with their keep-outs and what stands on them (the old passes moved them with the room's group; the wall layer only
/// clears them when a window has nowhere else to go)
pub fn clear_windows(g: &mut Generated) -> Vec<String> {
    let mut log = vec![];
    let alcoves: Vec<(String, String)> = g.features.iter().filter(|f| f.key.contains("alcove")).map(|f| (f.key.clone(), f.room.clone())).collect();
    for (k, room) in alcoves {
        let Some(o) = g.state.objs.get(&k) else { continue };
        let (l, h) = (o.geom.lo, o.geom.hi);
        let corners: Vec<[f64; 3]> = [[l[0], l[1], 0.0], [h[0], l[1], 0.0], [h[0], h[1], 0.0], [l[0], h[1], 0.0]].iter().map(|c| o.pose.apply(*c)).collect();
        let (x0, x1) = (corners.iter().map(|c| c[0]).fold(f64::MAX, f64::min), corners.iter().map(|c| c[0]).fold(f64::MIN, f64::max));
        let (y0, y1) = (corners.iter().map(|c| c[1]).fold(f64::MAX, f64::min), corners.iter().map(|c| c[1]).fold(f64::MIN, f64::max));
        let blocks = g.cutters.iter().filter(|c| matches!(c.kind, crate::rooms::solidify::CutKind::Window) && c.rooms.contains(&room)).any(|c| {
            let (sn, cs) = c.rot_z.sin_cos();
            let (ax, ay) = (c.dims[0] / 2.0 * cs.abs() + c.dims[1] / 2.0 * sn.abs(), c.dims[0] / 2.0 * sn.abs() + c.dims[1] / 2.0 * cs.abs());
            x0 < c.loc[0] + ax && x1 > c.loc[0] - ax && y0 < c.loc[1] + ay && y1 > c.loc[1] - ay
        });
        if !blocks { continue }
        let kids: Vec<String> = g.state.objs.iter().filter(|(kk, oo)| kk.starts_with(&format!("{k}.keepout")) || oo.relations.iter().any(|r| r.target == k)).map(|(kk, _)| kk.clone()).collect();
        for kk in kids { if g.state.objs.contains_key(&kk) { layout2d::remove_tree(&mut g.state, &kk); } }
        layout2d::remove_tree(&mut g.state, &k);
        g.features.retain(|f| f.key != k);
        log.push(format!("{room}: alcove shelves {k} removed, a window stands there"));
    }
    log
}

pub fn solve_house(g: &Generated, seed: u64) -> Plan {
    let rs = rules::default_rules();
    let mut rooms = vec![];
    let cutters: Vec<house::Cutter> = g.cutters.iter().map(|c| house::Cutter { kind: format!("{:?}", c.kind), loc: c.loc, rot_z: c.rot_z, dims: c.dims, rooms: c.rooms.clone() }).collect();
    let polys: Vec<(String, Vec<[f64; 2]>)> = g.room_polys.iter().map(|(n, p)| (n.clone(), { let mut r = crate::shp::exterior(p); if r.len() > 1 && r[0] == r[r.len() - 1] { r.pop(); } r })).collect();
    for (name, ring) in polys {
        let room = house::room_from(&name, ring, &cutters, g.c.wall_thickness, g.c.wall_height);
        let Some(rr) = rs.get(&room.kind) else { continue };
        // the built-ins stand as they are
        let feature_keys: Vec<String> = g.features.iter().filter(|f| f.room == name).map(|f| f.key.clone()).collect();
        let fixed: Vec<world::Prism> = feature_keys.iter().filter_map(|k| g.state.objs.get(k)).map(|o| {
            // (a little past its layout box: a built-in's mesh runs over it)
            let (l, h) = ([o.geom.lo[0] - 0.03, o.geom.lo[1] - 0.03, o.geom.lo[2]], [o.geom.hi[0] + 0.03, o.geom.hi[1] + 0.03, o.geom.hi[2]]);
            let hull = world::hull2([[l[0], l[1]], [h[0], l[1]], [h[0], h[1]], [l[0], h[1]]].iter().map(|c| { let q = o.pose.apply([c[0], c[1], 0.0]); [q[0], q[1]] }).collect());
            world::Prism { hull, z: [0.0, h[2] - l[2]], role: world::HARD }
        }).collect();
        // and the rules can name them: a built-in's frame runs along its wall with y into the room; as an item, +x is out into the room
        let named: Vec<solve::Placed> = g.features.iter().filter(|f| f.room == name).map(|f| {
            let (l, h) = f.asset.bounds();
            solve::fixed_item(f.class, f.class, world::Pose { yaw: f.yaw + std::f64::consts::FRAC_PI_2, t: [f.origin[0], f.origin[1]], z: 0.0 }, [l[1], -h[0], l[2]], [h[1], -l[0], h[2]])
        }).collect();
        rooms.push((name, solve::solve_with(&room, rr, seed ^ 0x1a70, None, &fixed, &named)));
    }
    Plan { rooms }
}

/// the solved rooms into the house
pub fn apply(g: &mut Generated, plan: Plan, seed: u64) -> Vec<String> {
    let mut log = vec![];
    let mut dump: Vec<serde_json::Value> = vec![];
    let floor_z = g.c.wall_thickness / 2.0;
    for (name, s) in plan.rooms {
        let missing: Vec<&str> = s.report.iter().filter(|r| r.tier == 0 && !r.placed).map(|r| r.id.as_str()).collect();
        if !missing.is_empty() { log.push(format!("{name}: a must has no place: {missing:?}")); }
        let rel = groups::floor_relation(&name);
        let key = |id: &str| format!("{}_{}", name.replace('/', "_"), id.replace('#', "_"));
        for p in &s.placed {
            let Some(mut piece) = solve::build_piece(p) else { log.push(format!("{name}: {} has no piece", p.id)); continue };
            // (a shower closes itself where its sides have no wall: known only now that it stands)
            if p.class == "ShowerFactory" {
                let (lo, hi) = ig_kit::piece::rigid_bounds(&piece.collision());
                let wall_at = |y: f64| [0.2, 0.5, 0.8].iter().all(|f: &f64| { let q = p.pose.apply([lo[0] + (hi[0] - lo[0]) * f, y]); house::edge_dist(&s.room.outline, q) < s.room.wall_t / 2.0 + 0.08 });
                let ends = [if wall_at(lo[1]) { 1.0 } else { 0.0 }, if wall_at(hi[1]) { 1.0 } else { 0.0 }, 0.0, 0.0];
                if let Some(x) = piece.with_ends(ends) { piece = x; }
            }
            groups::insert_piece(&mut g.state, &key(&p.id), &p.class, piece, [p.pose.t[0], p.pose.t[1], floor_z + p.pose.z], p.pose.yaw, Some(&rel), p.parent.as_ref().map(|x| key(x)).as_deref(), p.cover);
        }
        // (LAY_DUMP=PATH: what the solver saw, for tools/layview.py; and the checker's own view of the walk beside it)
        let (joined, bad) = layout2d::walk_status(&g.state, &name);
        if !joined || !s.doors_joined || !bad.is_empty() { log.push(format!("{name}: WALK solver joined {} checker joined {joined} cut off {bad:?}", s.doors_joined)); }
        dump.push(serde_json::json!({ "house": format!("gen{seed}"), "sol": s }));
        log.push(format!("{name}: {} placed in {:.0} ms, left out {:?}", s.placed.len(), s.ms, s.report.iter().filter(|r| !r.placed).map(|r| r.id.as_str()).collect::<Vec<_>>()));
    }
    if let Some(p) = std::env::var_os("LAY_DUMP") { let _ = std::fs::write(p, serde_json::to_vec(&dump).unwrap()); }
    log
}
