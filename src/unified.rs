//! LAYOUT=unified: furnish the rooms the rules cover with the unified solver (crates/ig_layout) after the old passes, in place of what they left.
//! What stays: built-ins, ceiling lights, doors, windows. A room whose must items have no place keeps its old layout for now (the plan layer will take this).
use crate::furn::{groups, layout2d};
use crate::pipeline::Generated;
use ig_layout::{house, rules, solve, world};

pub fn apply(g: &mut Generated, seed: u64) -> Vec<String> {
    let rs = rules::default_rules();
    let mut log = vec![];
    let mut dump: Vec<serde_json::Value> = vec![];
    let cutters: Vec<house::Cutter> = g.cutters.iter().map(|c| house::Cutter { kind: format!("{:?}", c.kind), loc: c.loc, rot_z: c.rot_z, dims: c.dims, rooms: c.rooms.clone() }).collect();
    let floor_z = g.c.wall_thickness / 2.0;
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
        let s = solve::solve_with(&room, rr, seed ^ 0x1a70, None, &fixed, &named);
        if s.report.iter().any(|r| r.tier == 0 && !r.placed) {
            log.push(format!("{name}: a must has no place, the old layout stays ({:?})", s.report.iter().filter(|r| r.tier == 0 && !r.placed).map(|r| &r.why).collect::<Vec<_>>()));
            continue;
        }
        // the room's floor relation, from what stands there now
        let rel = g.state.objs.values().flat_map(|o| o.relations.iter()).find(|r| r.target == name && r.relation.geo().is_some()).cloned();
        let gone: Vec<String> = g.state.objs.iter().filter(|(k, o)| o.generator.as_ref().map_or(false, |gn| !gn.class.ends_with("Feature") && !matches!(gn.class.as_str(), "CeilingLightFactory" | "Door" | "Window") && (gn.class != "HardwareFactory" || rr.program.iter().chain(rr.arrangements.iter().flat_map(|a| a.program.iter())).any(|i| i.class == gn.class))) && (layout2d::room_of(&g.state, k).as_deref() == Some(name.as_str()) || (layout2d::room_of(&g.state, k).is_none() && house::inside(&room.outline, { let q = o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, 0.0]); [q[0], q[1]] }))) && !feature_keys.contains(k)).map(|(k, _)| k.clone()).collect();
        for k in gone { if g.state.objs.contains_key(&k) { layout2d::remove_tree(&mut g.state, &k); } }
        let key = |id: &str| format!("{}_{id}", name.replace('/', "_"));
        for p in &s.placed {
            let Some(piece) = solve::build_piece(p) else { log.push(format!("{name}: {} has no piece", p.id)); continue };
            groups::insert_piece(&mut g.state, &key(&p.id), &p.class, piece, [p.pose.t[0], p.pose.t[1], floor_z + p.pose.z], p.pose.yaw, rel.as_ref(), p.parent.as_ref().map(|x| key(x)).as_deref(), p.cover);
        }
        // (LAY_DUMP=PATH: what the solver saw, for tools/layview.py; and the checker's own view of the walk beside it)
        let (joined, bad) = layout2d::walk_status(&g.state, &name);
        if !joined || !s.doors_joined { log.push(format!("{name}: WALK solver joined {} checker joined {joined} cut off {bad:?}", s.doors_joined)); }
        dump.push(serde_json::json!({ "house": format!("gen{seed}"), "sol": s }));
        log.push(format!("{name}: {} placed in {:.0} ms, left out {:?}", s.placed.len(), s.ms, s.report.iter().filter(|r| !r.placed).map(|r| r.id.as_str()).collect::<Vec<_>>()));
    }
    if let Some(p) = std::env::var_os("LAY_DUMP") { let _ = std::fs::write(p, serde_json::to_vec(&dump).unwrap()); }
    log
}
