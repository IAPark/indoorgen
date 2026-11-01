//! End-to-end generation: rooms -> furniture solve (large/medium/small) -> scene.
use crate::furn::assets::*;
use crate::furn::domain::*;
use crate::furn::geom::*;
use crate::furn::program::*;
use crate::furn::relations::*;
use crate::furn::solver::*;
use crate::furn::state::*;
use crate::furn::tags::*;
use crate::int_hash;
use crate::rng::Rng;
use crate::rooms::consts::RoomConstants;
use crate::rooms::plan::{FloorPlanSolver, PlanConfig};
use crate::rooms::shell::{self, RoomMeshes};
use crate::rooms::solidify::{CutKind, Cutter, Solidifier};
use crate::rooms::state::*;
use indexmap::IndexMap;
use crate::dhash::HashMap;
use std::sync::Arc;
use web_time::Instant;

pub struct Generated {
    pub c: RoomConstants,
    pub shells: Vec<RoomMeshes>,
    pub cutters: Vec<Cutter>,
    pub state: State,
    pub timings: Vec<(String, f64)>,
    pub room_polys: Vec<(String, crate::shp::G)>,
    /// room features built before the solve (also present in `state` as fixed objects)
    pub features: Vec<crate::features::PlacedFeature>,
    /// what the room / furniture layers could not do (furn::layers), per room, for the plan layer above
    pub complaints: Vec<crate::furn::layers::Complaint>,
}

fn sem_room_tag(rt: &str) -> Tag {
    let name: String = rt.split('-').map(|p| { let mut c = p.chars(); c.next().unwrap().to_uppercase().collect::<String>() + c.as_str() }).collect();
    Tag::sem(&name)
}

pub struct Options {
    pub seed: u64,
    /// seed of the furniture solve (constraint-program parameters, annealing streams, asset factory seeds); `seed` keeps the floor plan, shells, doors and windows
    pub furn_seed: u64,
    pub restrict_room: Option<String>, // e.g. "Bedroom": furnish only the first room of this type (old behaviour, `--only-target`)
    /// furnish every room of the house (each room solved independently with its own rng stream, in parallel); `restrict_room` then only
    /// names the target room type whose first room keeps the single-room rng stream
    pub whole_house: bool,
    pub steps_large: usize,
    pub steps_medium: usize,
    pub steps_small: usize,
    /// room features (fireplaces, ...) decided per room before the solve; empty table = none
    pub features: crate::features::FeatureCfg,
    /// open plan: probability that a kitchen / dining / living door becomes a wide passage (0 = off)
    pub passage_prob: f64,
    /// room grammar of the floor plan
    pub rooms: crate::rooms::graph::RoomSet,
    /// shared-wall moves applied to the floor plan before the house is built: (room a, room b, metres a grows by) -- the plan layer's
    /// answers to the complaints of the layers below (scenegen, HIER=1)
    pub nudges: Vec<(String, String, f64)>,
}

/// constraint program for a scene seed (params drawn from the global numpy stream like the python)
pub fn load_program(seed: u64, program_json: &serde_json::Value) -> (Program, Usage) {
    let mut g = Rng::seeded(seed);
    let p: Vec<f64> = vec![g.uniform(0.6, 0.9), g.uniform(0.5, 1.0), g.uniform(0.5, 1.0), g.uniform(0.2, 1.0), g.uniform(40.0, 100.0) / 40.0];
    let flags: Vec<bool> = [0.5, 0.15, 0.15, 0.15, 0.15].iter().map(|&q| g.f64() < q).collect();
    let margins = [g.uniform(0.1, 0.3), g.uniform(0.1, 0.3)];
    let mut params: Params = HashMap::default();
    for (k, v) in ["furniture_fullness_pct", "obj_interior_obj_pct", "obj_on_storage_pct", "obj_on_nonstorage_pct", "painting_area_per_room_area"].iter().zip(&p) {
        params.insert(k.to_string(), *v);
    }
    params.insert("sofa_back_margin".into(), margins[0]);
    params.insert("sofa_side_margin".into(), margins[1]);
    let variant: String = flags.iter().map(|&b| if b { '1' } else { '0' }).collect();
    (Program::load(program_json, &variant, &params), Usage::from_json(&program_json["used_as"]))
}

pub fn generate(opt: &Options, assets: &dyn Assets, program_json: &serde_json::Value) -> Generated {
    let mut timings = vec![];
    let t0 = Instant::now();
    let seed = opt.seed;
    // --- constraint params (global stream, python's home_furniture_constraints draws) ---
    let mut g = Rng::seeded(opt.furn_seed);
    let p: Vec<f64> = vec![g.uniform(0.6, 0.9), g.uniform(0.5, 1.0), g.uniform(0.5, 1.0), g.uniform(0.2, 1.0), g.uniform(40.0, 100.0) / 40.0];
    let flags: Vec<bool> = [0.5, 0.15, 0.15, 0.15, 0.15].iter().map(|&q| g.f64() < q).collect();
    let margins = [g.uniform(0.1, 0.3), g.uniform(0.1, 0.3)];
    let mut rng0 = Rng::seeded(seed);
    for _ in 0..42 {
        rng0.next_u32();
    }
    let c = RoomConstants::sample(&mut rng0, 1);
    let mut params: Params = HashMap::default();
    for (k, v) in ["furniture_fullness_pct", "obj_interior_obj_pct", "obj_on_storage_pct", "obj_on_nonstorage_pct", "painting_area_per_room_area"].iter().zip(&p) {
        params.insert(k.to_string(), *v);
    }
    params.insert("sofa_back_margin".into(), margins[0]);
    params.insert("sofa_side_margin".into(), margins[1]);
    let variant: String = flags.iter().map(|&b| if b { '1' } else { '0' }).collect();
    let prog = Program::load(program_json, &variant, &params);
    let usage = Usage::from_json(&program_json["used_as"]);
    timings.push(("program".into(), t0.elapsed().as_secs_f64()));

    // --- rooms ---
    let t = Instant::now();
    let cfg = PlanConfig { n_divide_trials: 25, iters_mult: 25, rooms: opt.rooms };
    let solver = FloorPlanSolver::new(seed, &c, cfg);
    let mut a = Rng::seeded(int_hash(&[&seed.to_string(), "solve_rooms"]));
    #[allow(unused_mut)]
    let mut st = solver.solve(&mut a);
    // plan refinement (tools/plan_refine.py): PLAN_NUDGE="room_a,room_b,d;..." moves shared walls before the house is built
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(spec) = std::env::var("PLAN_NUDGE") {
        for item in spec.split(';').filter(|s| !s.trim().is_empty()) {
            let f: Vec<&str> = item.split(',').collect();
            if f.len() != 3 { continue }
            let d: f64 = f[2].trim().parse().unwrap_or(0.0);
            let ok = crate::rooms::nudge::nudge(&mut st, f[0].trim(), f[1].trim(), d);
            if std::env::var_os("GROUPDBG").is_some() { eprintln!("NUDGE {} {} {d}: {}", f[0], f[1], if ok { "moved" } else { "does not fit" }); }
        }
    }
    for (a, b, d) in &opt.nudges {
        let ok = crate::rooms::nudge::nudge(&mut st, a, b, *d);
        if std::env::var_os("GROUPDBG").is_some() || std::env::var_os("LAYERDBG").is_some() { eprintln!("NUDGE {a} {b} {d}: {}", if ok { "moved" } else { "does not fit" }); }
    }
    if std::env::var_os("PLAN_STATS").is_some() {
        for (k, ar, cv, asp) in crate::rooms::score::room_stats(&st) {
            println!("PLAN {seed} {k} area {ar:.1} convexity {cv:.2} aspect {asp:.2}");
        }
        std::process::exit(0);
    }
    // hallways serving fewer than two private rooms go into a neighbour (plan_rules.merge_hallways)
    let mut graph0 = solver.graphs[0].clone();
    if crate::rooms::plan_rules::get().merge_hallways {
        for l in crate::rooms::nudge::merge_useless_hallways(&mut st, &mut graph0) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("PLAN {l}"); } }
    }
    let sol = Solidifier { c: &c, graph: &graph0, level: 0, enable_open: false, passage_prob: opt.passage_prob };
    let mut res = sol.solidify(&mut Rng::seeded(seed), &st);
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("PLAN_DUMP") {
        // floor plan only (room polygons + door / window / entrance cutters), then exit: no furniture
        use geos::Geom;
        let rooms: serde_json::Map<String, serde_json::Value> = st.objs.iter().filter_map(|(k, o)| Some((k.clone(), serde_json::json!(o.polygon.as_ref()?.to_wkt().ok()?)))).collect();
        let cut: Vec<_> = res.cutters.iter().map(|ct| serde_json::json!({"kind": format!("{:?}", ct.kind), "loc": ct.loc, "rot": ct.rot_z, "dim": ct.dims, "rooms": ct.rooms})).collect();
        std::fs::write(&path, serde_json::json!({"seed": seed, "wall_thickness": c.wall_thickness, "wall_height": c.wall_height, "rooms": rooms, "cutters": cut}).to_string()).unwrap();
        std::process::exit(0);
    }
    let mut shells = shell::build_shells(&c, &st, &res.cutters);
    timings.push(("rooms".into(), t.elapsed().as_secs_f64()));

    // --- solver state ---
    let t = Instant::now();
    let mut state = State::new();
    let mut room_polys = vec![];
    for sh in &shells {
        let mut geom = shell::room_geom(&c, sh);
        {
            let (x0, y0, x1, y1) = crate::shp::bounds(st.objs[&sh.name].polygon.as_ref().unwrap());
            geom.lo = [x0, y0, 0.0];
            geom.hi = [x1, y1, c.wall_height];
        }
        if let (Ok(path), false) = (std::env::var("STATE0"), std::env::var("STATE0_NOROOM").is_ok()) {
            // exactness harness: take the room mesh (triangle order, vertex order, face tags) verbatim from a python dump
            let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            if let Some(o) = d["objs"].get(&sh.name) {
                let verts: Vec<[f64; 3]> = o["verts_world"].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
                let names = o["tagmasks"]["face_names"].as_array().unwrap();
                let mut tris = vec![];
                for (pi, poly) in o["polys"].as_array().unwrap().iter().enumerate() {
                    let ix: Vec<usize> = poly.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
                    assert_eq!(ix.len(), 3);
                    let mut bits = 0u32;
                    for part in names[pi].as_str().unwrap().split('.') {
                        bits |= match part { "interior" => crate::furn::geom::INTERIOR, "visible" => crate::furn::geom::VISIBLE, "support" => crate::furn::geom::SUPPORT, "ceiling" => crate::furn::geom::CEILING, "wall" => crate::furn::geom::WALL, _ => 0 };
                    }
                    if std::env::var("S0_NOINT").is_ok() { bits &= !crate::furn::geom::INTERIOR; }
                    if std::env::var("S0_NOVIS").is_ok() { bits &= !crate::furn::geom::VISIBLE; }
                    tris.push(crate::furn::geom::Tri { v: [verts[ix[0]], verts[ix[1]], verts[ix[2]]], tags: bits });
                }
                if std::env::var("S0_REV").is_ok() { tris.reverse(); }
                if let Ok(m) = std::env::var("S0_MIX") {
                    let w = crate::furn::geom::WALL;
                    let keep_py_wall = m == "pywall";
                    let mut o: Vec<crate::furn::geom::Tri> = tris.iter().filter(|t| (t.tags & w != 0) == keep_py_wall).cloned().collect();
                    o.extend(geom.tris.iter().filter(|t| (t.tags & w != 0) != keep_py_wall).cloned());
                    tris = o;
                }
                geom.tris = tris;
            }
        }
        if std::env::var("ROOMSTAT").is_ok() {
            let stat = |tris: &[crate::furn::geom::Tri]| {
                let mut a = [0.0f64; 6];
                let mut n = [0usize; 6];
                for t in tris {
                    let ar = crate::furn::geom::tri_area(&t.v);
                    for (k, bit) in [crate::furn::geom::INTERIOR, crate::furn::geom::VISIBLE, crate::furn::geom::SUPPORT, crate::furn::geom::CEILING, crate::furn::geom::WALL].iter().enumerate() {
                        if t.tags & bit != 0 { a[k] += ar; n[k] += 1; }
                    }
                    a[5] += ar; n[5] += 1;
                }
                format!("int {:.2}/{} vis {:.2}/{} sup {:.2}/{} ceil {:.2}/{} wall {:.2}/{} all {:.2}/{}", a[0], n[0], a[1], n[1], a[2], n[2], a[3], n[3], a[4], n[4], a[5], n[5])
            };
            eprintln!("ROOMSTAT {} {}", sh.name, stat(&geom.tris));
            if std::env::var("ROOMSTAT").unwrap() == "3" {
                for t in &geom.tris { eprintln!("RT3 {} {:?} {}", sh.name, t.v, t.tags); }
            }
            if std::env::var("ROOMSTAT").unwrap() == "2" {
                let mut m: std::collections::BTreeMap<(u32, bool), (usize, f64)> = Default::default();
                for t in &geom.tris {
                    let n = crate::furn::geom::tri_nz(&t.v);
                    let e = m.entry((t.tags, n)).or_default();
                    e.0 += 1;
                    e.1 += crate::furn::geom::tri_area(&t.v);
                }
                for (k, v) in m { eprintln!("  RS tags {:#x} upish {} n {} area {:.3}", k.0, k.1, v.0, v.1); }
            }
        }
        let rt = room_type(&sh.name);
        let tags = TagSet::from([Tag::sem("Room"), Tag::obj(&sh.name), sem_room_tag(rt)]);
        let mut o = ObjState::new(Arc::new(geom), Pose::identity(), tags);
        o.polygon = Some(st.objs[&sh.name].polygon.clone().unwrap());
        room_polys.push((sh.name.clone(), o.polygon.clone().unwrap()));
        state.objs.insert(sh.name.clone(), o);
        state.touch(&sh.name);
    }
    // room neighbour relations
    let names: Vec<String> = shells.iter().map(|s| s.name.clone()).collect();
    for k in &names {
        let mut rels = vec![];
        for r in &st.objs[k].relations {
            let door = res.cutters.iter().any(|ct| matches!(ct.kind, CutKind::Door | CutKind::Passage) && ct.rooms.len() == 2 && ct.rooms.contains(k) && ct.rooms.contains(&r.target));
            rels.push(RelState { relation: Relation::RoomNeighbour(vec![(if door { "door" } else { "wall" }).to_string()]), target: r.target.clone(), child_plane: 0, parent_plane: 0 });
        }
        state.objs.get_mut(k).unwrap().relations = rels;
    }
    let mut counts: HashMap<&str, usize> = HashMap::default();
    for ct in &res.cutters {
        let base = match ct.kind {
            CutKind::Door => "door",
            CutKind::Window => "window",
            CutKind::Entrance => "entrance",
            CutKind::Passage => "passage",
        };
        let n = counts.entry(base).or_insert(0);
        let name = if *n == 0 { base.to_string() } else { format!("{base}.{:03}", n) };
        *n += 1;
        let h = [ct.dims[0] / 2., ct.dims[1] / 2., ct.dims[2] / 2.];
        let g = {
            let r = |v: f64| v as f32 as f64;
            let mut g = crate::furn::geom::cuboid_py([r(-h[0]), r(-h[1]), r(-h[2])], [r(h[0]), r(h[1]), r(h[2])], 0, 0);
            for t in g.tris.iter_mut() { t.tags = 0; }
            g
        };
        let kind_tag = if ct.kind == CutKind::Window { Tag::sem("Window") } else { Tag::sem("Door") };
        let tags = TagSet::from([kind_tag, Tag::sem("Cutter"), Tag::obj(&name)]);
        let mut o = ObjState::new(Arc::new(g), Pose::from_euler(ct.loc, [0., 0., ct.rot_z]), tags);
        o.relations = ct.rooms.iter().map(|r| RelState { relation: Relation::CutFrom, target: r.clone(), child_plane: 0, parent_plane: 0 }).collect();
        if let (Ok(path), false) = (std::env::var("STATE0"), std::env::var("STATE0_NOCUT").is_ok()) {
            // exactness harness: cutter objects (doors / windows) verbatim from the python dump
            let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            if let Some(po) = d["objs"].get(&name) {
                let f3 = |k: &str| -> [f32; 3] { let a = po[k].as_array().unwrap(); [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32] };
                let pose = Pose::from_o32(crate::bl32::Obj32 { loc: f3("loc"), eul: f3("euler"), scale: f3("scale") });
                let loc: Vec<[f64; 3]> = po["verts_local"].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
                let polys = po["polys"].as_array().unwrap();
                let mut tris = vec![];
                for p in polys {
                    let ix: Vec<usize> = p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
                    tris.push(crate::furn::geom::Tri { v: [loc[ix[0]], loc[ix[1]], loc[ix[2]]], tags: 0 });
                }
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for v in &loc { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
                let proxy = Proxy::Box { center: [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.], half: [(hi[0] - lo[0]) / 2., (hi[1] - lo[1]) / 2., (hi[2] - lo[2]) / 2.] };
                o.geom = Arc::new(ObjGeom::from_tris(tris, vec![(proxy, u32::MAX)]).with_mesh_verts(loc.clone()));
                o.pose = pose;
            }
        }
        state.objs.insert(name.clone(), o);
        state.touch(&name);
    }
    // --- room features: decided after the floor plan, before the solve; fixed obstacles + support surfaces for the solver ---
    let features = crate::features::plan(&opt.features, &c, &room_polys, &res.cutters);
    for pf in &features {
        // Object + a non-geometric relation to the room: the obj stages (obj_on_support / obj_ontop_obj) then offer the feature as a
        // parent for small items (mantel, hearth, shelves), while the floor / wall stages, which select by StableAgainst relations,
        // never pick it for moves (it is fixed, built into the shell)
        let mut tags = TagSet::from([Tag::sem("Feature"), Tag::sem("Object"), Tag::gen(pf.class), Tag::obj(&pf.key)]);
        tags.insert(Tag::sem(pf.class));
        let mut o = ObjState::new(Arc::new(pf.asset.solver_geom()), pf.pose(), tags);
        o.relations.push(crate::furn::state::RelState { relation: crate::furn::relations::Relation::Traverse, target: pf.room.clone(), child_plane: 0, parent_plane: 0 });
        o.generator = Some(GenRef { class: pf.class.to_string(), factory_seed: 0, inst_seed: 0, unseeded: None, fit_box: None, size: None, join: None });
        o.active = true;
        state.objs.insert(pf.key.clone(), o);
        state.touch(&pf.key);
        // hearth clearance: an invisible box that furniture must not intrude on
        for (i, (lo, hi)) in pf.keepout.iter().enumerate() {
            let name = format!("{}.keepout{}", pf.key, i);
            let kt = TagSet::from([Tag::sem("FeatureKeepout"), Tag::obj(&name)]);
            let mut ko = ObjState::new(Arc::new(ObjGeom::cuboid(*lo, *hi, 0)), Pose::identity(), kt);
            ko.active = true;
            state.objs.insert(name.clone(), ko);
            state.touch(&name);
        }
    }
    // the wall layer (on; WALL_LAYER=0 turns it off; every room): the windows are held back here and laid out after the furniture,
    // with the pictures and the pieces backed against the walls (wall_layer_windows); patio doors and rooms with a window seat keep
    // theirs. The plan keeps which walls are exterior and the doors.
    let wall_layer = std::env::var("WALL_LAYER").map_or(true, |v| v != "0");
    let mut deferred: Vec<Cutter> = vec![];
    if wall_layer {
        let seat_rooms: std::collections::HashSet<String> = features.iter().filter(|f| f.class == "WindowSeatFeature").map(|f| f.room.clone()).collect();
        // (WALL_LAYER=pilot: living rooms and bedrooms only, as first built)
        let pilot_only = std::env::var("WALL_LAYER").map_or(false, |v| v == "pilot");
        let pilot = |r: &str| !pilot_only || matches!(crate::rooms::state::room_type(r), "living-room" | "bedroom");
        let mut keep = vec![];
        for ct in std::mem::take(&mut res.cutters) {
            if ct.kind == CutKind::Window && ct.rooms.len() == 1 && pilot(&ct.rooms[0]) && !seat_rooms.contains(&ct.rooms[0]) { deferred.push(ct) } else { keep.push(ct) }
        }
        res.cutters = keep;
        // their solver objects stay as window intents: nothing collides with them (NoCollision), but the furniture passes can see a
        // window will come on that wall (a headboard there is avoided); the wall layer removes them when it lays the windows out
        let intents: Vec<String> = state.objs.iter().filter(|(_, o)| o.tags.contains(Tag::sem("Window")) && deferred.iter().any(|ct| (0..3).all(|d| (o.pose.t[d] - ct.loc[d]).abs() < 1e-6))).map(|(k, _)| k.clone()).collect();
        for k in intents {
            let o = state.objs.get_mut(&k).unwrap();
            o.tags = TagSet::from([Tag::sem("WindowIntent"), Tag::sem("NoCollision"), Tag::obj(&k)]);
            state.touch(&k);
        }
    }
    // door clearance: a keep-out as wide as the opening and as deep (0.5 m at open-plan passages) on each room side of every door and
    // entrance; furniture must not stand in it, and its centre is the room's door point for the walkway rule (furn::layout2d)
    for (i, ct) in res.cutters.iter().enumerate() {
        // (as deep as the leaf swings, the opening's width: 0.8 m left wardrobes and chairs inside the drawn swing)
        // (a double door's leaves each sweep half the opening; sliders sweep nothing: walking room only)
        use crate::rooms::solidify::DoorStyle;
        let depth = match (ct.kind, ct.style) {
            (CutKind::Door, DoorStyle::Double) => (ct.dims[0] / 2.0 + 0.05).max(0.8),
            (CutKind::Door, DoorStyle::Sliding | DoorStyle::SlidingGlass) => 0.6,
            (CutKind::Door | CutKind::Entrance, _) => (ct.dims[0] + 0.05).max(0.8),
            // a half wall is not a way through: no clearance, no door point
            (CutKind::Passage, DoorStyle::HalfWall) => continue,
            (CutKind::Passage, _) => 0.5,
            _ => continue,
        };
        // cutter boxes: dims[0] = opening width along the wall (rot_z), dims[1] = depth through the wall
        // (4 cm wider each side than the opening: models reach a few cm past their boxes -- a shower's handle bar and glass stood in
        // a bathroom doorway)
        let (along, w) = (ct.rot_z, ct.dims[0] + 0.08);
        let n = [-along.sin(), along.cos()];
        for room in &ct.rooms {
            let Some((_, poly)) = room_polys.iter().find(|(k, _)| k == room) else { continue };
            let Some(s) = [1.0, -1.0].into_iter().find(|s| crate::shp::contains_pt(poly, [ct.loc[0] + s * n[0] * 0.5, ct.loc[1] + s * n[1] * 0.5])) else { continue };
            let off = c.wall_thickness / 2.0 + depth / 2.0 + 0.01;
            let centre = [ct.loc[0] + s * n[0] * off, ct.loc[1] + s * n[1] * off, 1.0];
            let name = format!("doorclear/{room}/{i}");
            let kt = TagSet::from([Tag::sem("FeatureKeepout"), Tag::obj(&name)]);
            let mut ko = ObjState::new(Arc::new(ObjGeom::cuboid([-w / 2.0, -depth / 2.0, -1.0], [w / 2.0, depth / 2.0, 1.0], 0)), Pose::from_euler(centre, [0.0, 0.0, along]), kt);
            ko.active = true;
            state.objs.insert(name.clone(), ko);
            state.touch(&name);
        }
    }
    timings.push(("state".into(), t.elapsed().as_secs_f64()));

    // --- stages ---
    let mut stages: IndexMap<String, Domain> = program_json["stages"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), parse_domain(v, &HashMap::default()))).collect();
    if let (Some(room), false) = (&opt.restrict_room, opt.whole_house) {
        let rt = Domain::new(TagSet::from([Tag::sem(room)])).with_tags(&TagSet::from([Tag::var("room")]));
        for d in stages.values_mut() {
            *d = domain_tag_substitute(d, Tag::var("room"), &rt).0;
        }
    }
    let mut complaints = vec![];
    if opt.whole_house {
        let ((tl, tm, ts), cs) = solve_whole_house(opt, assets, &prog, &usage, &stages, &mut state);
        complaints = cs;
        timings.push(("solve_large".into(), tl));
        timings.push(("solve_medium".into(), tm));
        timings.push(("solve_small".into(), ts));
    } else {
        let mut limits = HashMap::default();
        limits.insert("room".to_string(), 1usize);
        let (tl, tm, ts) = solve_stages(opt, assets, &prog, &usage, &stages, &mut state, &limits, None, None);
        timings.push(("solve_large".into(), tl));
        timings.push(("solve_medium".into(), tm));
        timings.push(("solve_small".into(), ts));
    }
    // features the room layer moved (a fireplace and its alcove shelves travel with the living-room group): the built asset follows its
    // solver object (span origin / yaw), with its footprint and keep-out boxes moved the same way
    let mut features = features;
    // (and features the room layer removed -- alcove shelves where a window comes -- are not built)
    features.retain(|pf| state.objs.contains_key(&pf.key));
    for pf in features.iter_mut() {
        let Some(o) = state.objs.get(&pf.key) else { continue };
        let (oy, ny) = (pf.yaw, o.pose.r[1][0].atan2(o.pose.r[0][0]));
        let (oo, no) = (pf.origin, o.pose.t);
        if (0..2).all(|d| (oo[d] - no[d]).abs() < 1e-6) && (oy - ny).sin().abs() < 1e-6 && (oy - ny).cos() > 0.0 { continue }
        let dy = ny - oy;
        let (s_, c_) = dy.sin_cos();
        let mv = |q: [f64; 2]| { let d = [q[0] - oo[0], q[1] - oo[1]]; [no[0] + c_ * d[0] - s_ * d[1], no[1] + s_ * d[0] + c_ * d[1]] };
        pf.footprint = pf.footprint.iter().map(|q| mv(*q)).collect();
        pf.keepout = pf.keepout.iter().map(|(lo, hi)| {
            let cs: Vec<[f64; 2]> = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]].iter().map(|q| mv(*q)).collect();
            ([cs.iter().map(|q| q[0]).fold(f64::MAX, f64::min), cs.iter().map(|q| q[1]).fold(f64::MAX, f64::min), lo[2]],
             [cs.iter().map(|q| q[0]).fold(f64::MIN, f64::max), cs.iter().map(|q| q[1]).fold(f64::MIN, f64::max), hi[2]])
        }).collect();
        pf.origin = [no[0], no[1], oo[2]];
        pf.yaw = ny;
        if std::env::var_os("LAYERDBG").is_some() { eprintln!("FEATURE {} moved to ({:.2}, {:.2}), turned {:+.0} deg", pf.key, no[0], no[1], dy.to_degrees()); }
    }
    if wall_layer && !deferred.is_empty() {
        let intents: Vec<String> = state.objs.iter().filter(|(_, o)| o.tags.contains(Tag::sem("WindowIntent"))).map(|(k, _)| k.clone()).collect();
        for k in intents { state.objs.shift_remove(&k); }
        for (i, ct) in wall_layer_windows(&c, &mut state, &room_polys, &res.cutters, &deferred).into_iter().enumerate() {
            let name = format!("window.{:03}", 500 + i);
            let h = [ct.dims[0] / 2., ct.dims[1] / 2., ct.dims[2] / 2.];
            let mut g = crate::furn::geom::cuboid_py([-h[0], -h[1], -h[2]], [h[0], h[1], h[2]], 0, 0);
            for t in g.tris.iter_mut() { t.tags = 0; }
            let tags = TagSet::from([Tag::sem("Window"), Tag::sem("Cutter"), Tag::obj(&name)]);
            let mut o = ObjState::new(Arc::new(g), Pose::from_euler(ct.loc, [0., 0., ct.rot_z]), tags);
            o.relations = ct.rooms.iter().map(|r| RelState { relation: Relation::CutFrom, target: r.clone(), child_plane: 0, parent_plane: 0 }).collect();
            state.objs.insert(name.clone(), o);
            state.touch(&name);
            res.cutters.push(ct);
        }
        // the shells again, with the windows where they now are
        shells = shell::build_shells(&c, &st, &res.cutters);
        // (alcove shelves the wall layer took out for a window are not built)
        features.retain(|pf| state.objs.contains_key(&pf.key));
    }
    Generated { c, shells, cutters: res.cutters, state, timings, room_polys, features, complaints }
}

/// rooms + cutters only (no furniture): used for validation against reference dumps
pub fn generate_rooms_only(seed: u64) -> State {
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let assets = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    generate(&Options { seed, furn_seed: seed, restrict_room: None, whole_house: false, steps_large: 0, steps_medium: 0, steps_small: 0, features: Default::default(), passage_prob: 0.0, rooms: crate::rooms::graph::RoomSet::Fewer, nudges: vec![] }, &assets, &prog).state
}

/// the large / medium / small greedy solve stages on `state`; `only_room` restricts the assignments to one room key, `tag` derives
/// a separate rng stream per room (None = the single global stream of the python)
fn solve_stages(opt: &Options, assets: &dyn Assets, prog: &Program, usage: &Usage, stages: &IndexMap<String, Domain>, state: &mut State, limits: &HashMap<String, usize>, only_room: Option<&str>, tag: Option<&str>) -> (f64, f64, f64) {
    let mut ctx = Ctx { prog, usage, assets, bounds: vec![], singletons: HashMap::default(), oracle: Default::default(), populated: Default::default(), ctor_words: Default::default() };
    if let Ok(p) = std::env::var("CTORWORDS") {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        for e in j.as_array().unwrap() {
            ctx.ctor_words.push_back((e["class"].as_str().unwrap_or("").to_string(), e["k"].as_u64().unwrap() as u32));
        }
    }
    if let Ok(p) = std::env::var("PHDUMP") {
        let (a, b) = crate::furn::solver::load_oracle(&p);
        ctx.oracle = a;
        ctx.populated = b;
    }
    let cfg = || AnnealCfg { initial_temp: 3.0, final_temp: 0.001, finetune_pct: 0.15, max_invalid_candidates: 5 };
    let mut run_stage = |name: &str, group: &str, steps: usize, restrict: Option<Vec<MoveKind>>, add: f64, state: &mut State, rng: &mut Rng, ctx: &mut Ctx| {
        let dom = stages[name].clone();
        let assigns = iterate_assignments(&dom, state, &["room", "obj"], limits);
        for asg in assigns {
            if let Some(r) = only_room {
                if asg.get("room").map_or(false, |x| x != r) { continue }
            }
            solve_objects(ctx, state, &dom, &asg, &StageParams { n_steps: steps, restrict_moves: restrict.clone(), addition_scalar: add }, cfg(), rng);
        }
        let _ = group;
    };
    let seed = opt.furn_seed;
    let seed_s = seed.to_string();
    let hs = |what: &str| -> u64 { match tag { None => int_hash(&[&seed_s, what]), Some(t) => int_hash(&[&seed_s, what, t]) } };
    let t = Instant::now();
    let mut rng = Rng::seeded(hs("solve_large"));
    run_stage("on_floor_and_wall", "large", opt.steps_large, None, 3.0, state, &mut rng, &mut ctx);
    run_stage("on_floor_freestanding", "large", opt.steps_large, None, 3.0, state, &mut rng, &mut ctx);
    // kitchen group: fridge and dishwasher at the ends of the counter run (before the run grows into its span)
    if crate::furn::layout2d::knobs::groups() {
        for l in crate::furn::groups::caps(state) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
        let mut rg = Rng::seeded(hs("groups_add"));
        for l in crate::furn::groups::missing_pieces(&mut ctx, state, &mut rg, only_room) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
        if crate::furn::layout_rules::get().bathroom_template { for l in crate::furn::groups::bathroom_template(state, only_room) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } } }
        for l in crate::furn::groups::toilets(state, only_room) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
        let liv = crate::furn::groups::living(&mut ctx, state, &mut rg);
        let eat = crate::furn::groups::eat_in(&mut ctx, state, &mut rg, only_room);
        for l in liv.into_iter().chain(eat) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
    }
    if crate::furn::layout2d::knobs::groups() { for l in crate::furn::groups::kitchen(&mut ctx, state) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } } }
    // built-ins sized exactly for their space (counter runs, kitchen cabinets, wardrobes, bathtubs, showers)
    if crate::furn::layout2d::knobs::builtin_fit() {
        for l in crate::furn::solver::fit_builtins(&mut ctx, state, only_room) { if std::env::var_os("BUILTINDBG").is_some() { eprintln!("BUILTIN {l}"); } }
    }
    let tl = t.elapsed().as_secs_f64();
    // populate_intermediate_pholders: shelves etc. become real assets so children can use their support surfaces
    for (k, o) in state.objs.iter_mut() {
        let Some(g) = o.generator.clone() else { continue };
        // open shelving the dressers restock (shelffill) keeps its box: whatever the solver would put inside is purged (dress::purge)
        let restocked = !std::env::var_os("POPULATE_ALL").is_some() && matches!(g.class.as_str(), "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory");
        if usage.has_usage(&g.class, "AssetPlaceholderForChildren") && !restocked {
            // spawn_asset creates a brand new blender object: same location / euler, unit scale
            let mut ob = o.pose.o;
            ob.scale = [1.0, 1.0, 1.0];
            // add_to_scene(preprocess=True) afterwards re-derives loc / euler / scale from the matrix
            let w = Pose::from_o32(ob).o.matrix_world();
            ob.set_matrix_world(&w);
            o.pose = Pose::from_o32(ob);
            if let Some(geom) = ctx.populated.get(&(g.class.clone(), g.factory_seed, g.inst_seed)) {
                o.geom = Arc::new(geom.clone());
            } else if let Some(geom) = {
                // 'exact' classes (kitchen cabinets) build to the chosen size
                let ex = crate::furn::sizes::model(&g.class).map_or(false, |m| m.exact);
                if ex { ig_kit::api::set_size_hint(crate::furn::solver::cur_size(&g)); }
                let r = assets.solver_asset(&g.class, g.factory_seed, g.inst_seed);
                if ex { ig_kit::api::set_size_hint(None); }
                r
            }.map(|geom| match (g.fit_box, crate::furn::sizes::model(&g.class).filter(|m| !m.exact)) {
                (Some((tlo, thi)), Some(m)) => { let (s, off) = crate::furn::sizes::fit_matrix(m.uniform, geom.lo, geom.hi, tlo, thi); crate::furn::sizes::fit_geom(&geom, s, off) }
                _ => geom,
            }) {
                if std::env::var_os("PHDBG").is_some() { eprintln!("SOLVER_ASSET {} {} {} lo {:?} hi {:?}", g.class, g.factory_seed, g.inst_seed, geom.lo, geom.hi); }
                o.geom = Arc::new(geom);
                let _ = k;
            }
        }
    }
    let keys: Vec<String> = state.objs.keys().cloned().collect();
    for k in keys {
        state.touch(&k);
    }
    let t = Instant::now();
    let mut rng = Rng::seeded(hs("solve_medium"));
    run_stage("on_wall", "medium", opt.steps_medium, None, 3.0, state, &mut rng, &mut ctx);
    run_stage("on_ceiling", "medium", opt.steps_medium, None, 3.0, state, &mut rng, &mut ctx);
    run_stage("side_obj", "medium", opt.steps_medium, None, 3.0, state, &mut rng, &mut ctx);
    // bedroom and desk groups: nightstands beside the headboard, a chair at every desk
    if crate::furn::layout2d::knobs::groups() {
        let mut lg = crate::furn::groups::caps(state);
        lg.extend(crate::furn::groups::beds(state));
        let mut rd = Rng::seeded(hs("groups_dining"));
        lg.extend(crate::furn::groups::dining(&mut ctx, state, &mut rd));
        lg.extend(crate::furn::groups::dining_clear(state, only_room));
        lg.extend(crate::table_setting::gather_chairs(state));
        lg.extend(crate::furn::groups::dining_furnish(&mut ctx, state, &mut rd, only_room));
        lg.extend(crate::furn::groups::room_extras(&mut ctx, state, &mut rd, only_room));
        lg.extend(crate::furn::groups::square_up(state));
        lg.extend(crate::furn::groups::rugs(state));
        lg.extend(crate::furn::groups::wall_hangings(state));
        lg.extend(crate::furn::groups::kitchen_style(state));
        lg.extend(crate::furn::groups::orphans(state));
        if std::env::var_os("GROUPDBG").is_some() { for l in &lg { eprintln!("GROUP {l}"); } }
        let mut rg = Rng::seeded(hs("groups"));
        for l in crate::furn::groups::bedroom_and_desk(&mut ctx, state, &mut rg) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
        for l in crate::furn::groups::wardrobes(state) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
        for l in crate::furn::groups::fittings_clear(state) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } }
    }
    let tm = t.elapsed().as_secs_f64();
    if std::env::var_os("RELDUMP").is_some() {
        for (k, o) in &state.objs {
            let Some(g) = &o.generator else { continue };
            if !["SideTableFactory", "ChairFactory", "OfficeChairFactory", "FridgeFactory", "DishwasherFactory", "SimpleDeskFactory", "BedFactory", "KitchenSpaceFactory"].contains(&g.class.as_str()) { continue }
            eprintln!("RELDUMP {} {k}: tags {:?}", g.class, o.tags);
            for r in &o.relations { eprintln!("RELDUMP    -> {} {:?} child_plane {} parent_plane {}", r.target, r.relation, r.child_plane, r.parent_plane); }
        }
    }
    // LAYOUT_DUMP_STAGE=furniture: stop after the furniture layout (no small items on surfaces); LAYOUT_DUMP then writes it
    if crate::furn::layout2d::knobs::furniture_only() {
        if crate::furn::layout2d::knobs::groups() { let mut rg = Rng::seeded(hs("sinks")); for l in crate::furn::groups::sinks(&mut ctx, state, &mut rg, only_room) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } } }
        return (tl, tm, 0.0);
    }
    let t = Instant::now();
    let mut rng = Rng::seeded(hs("solve_small"));
    run_stage("obj_ontop_obj", "small", opt.steps_small, None, 3.0, state, &mut rng, &mut ctx);
    run_stage("obj_on_support", "small", opt.steps_small, Some(vec![MoveKind::Addition]), 1.0, state, &mut rng, &mut ctx);
    if crate::furn::layout2d::knobs::groups() { let mut rg = Rng::seeded(hs("sinks")); for l in crate::furn::groups::sinks(&mut ctx, state, &mut rg, only_room) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } } }
    if std::env::var_os("KRULEDBG").is_some() {
        for k in state.objs.keys().filter(|k| k.starts_with("kitchen") && !k.contains('.') && crate::furn::layout2d::is_room(&state.objs[k.as_str()])).cloned().collect::<Vec<_>>() {
            let e = crate::furn::kitchen_rules::eval(state, &k, None);
            eprintln!("KRULES {k} {:.2} {:?}", e.score, e.broken);
        }
    }
    if crate::furn::layout2d::knobs::groups() { for l in crate::furn::groups::orphans(state) { if std::env::var_os("GROUPDBG").is_some() { eprintln!("GROUP {l}"); } } }
    let ts = t.elapsed().as_secs_f64();
    (tl, tm, ts)
}

fn clone_state(s: &State) -> State {
    State { objs: s.objs.clone(), caches: Default::default(), next_version: s.next_version }
}

/// Furnish every room: rooms are independent in the constraint program (the solver only looks at the active room's objects), so each
/// room is solved on its own copy of the room state with its own rng stream and the results are merged in room-key order.
/// The first room of the target room type keeps the single-room rng stream (same furniture as `--only-target`).
fn solve_whole_house(opt: &Options, assets: &dyn Assets, prog: &Program, usage: &Usage, stages: &IndexMap<String, Domain>, state: &mut State) -> ((f64, f64, f64), Vec<crate::furn::layers::Complaint>) {
    use rayon::prelude::*;
    let no_limits = HashMap::default();
    let mut rooms: Vec<String> = iterate_assignments(&stages["on_floor_and_wall"], state, &["room"], &no_limits).into_iter().filter_map(|a| a.get("room").cloned()).collect();
    rooms.sort();
    let target: Option<String> = opt.restrict_room.as_ref().and_then(|rt| {
        let want = Tag::sem(rt);
        rooms.iter().find(|r| state.objs[r.as_str()].tags.contains(want)).cloned()
    });
    let base_keys: std::collections::HashSet<String> = state.objs.keys().cloned().collect();
    let serial = std::env::var_os("SERIAL_ROOMS").is_some();
    // SAFETY: the only Assets implementations used here (RealAssets, TableAssets) are read-only tables plus Mutex-guarded memo caches
    // (gen2 already shares RealAssets between rayon workers for the visual builds)
    struct SyncAssets<'a>(&'a dyn Assets);
    unsafe impl Sync for SyncAssets<'_> {}
    impl SyncAssets<'_> { fn get(&self) -> &dyn Assets { self.0 } }
    let sa = SyncAssets(assets);
    let work = |(r, mut st): (&String, State)| -> (State, (f64, f64, f64), Vec<crate::furn::layers::Complaint>) {
        let tag = if target.as_ref() == Some(r) { None } else { Some(r.as_str()) };
        let _ = crate::furn::layers::drain();
        let t = solve_stages(opt, sa.get(), prog, usage, stages, &mut st, &no_limits, Some(r), tag);
        // the furniture layer's own check: every door of the room joined by walkable floor (the walkway repair after the solve deletes
        // pieces to get there)
        let (connected, bad) = crate::furn::layout2d::walk_status(&st, r);
        let door_at = st.objs.keys().filter(|k| k.starts_with(&format!("doorclear/{r}/"))).find_map(|k| crate::furn::layout2d::prisms(&st, k).first().map(|p| { let n = p.poly.len() as f64; [p.poly.iter().map(|v| v[0]).sum::<f64>() / n, p.poly.iter().map(|v| v[1]).sum::<f64>() / n] }));
        if !connected { crate::furn::layers::complain(crate::furn::layers::Layer::Furniture, r, "door_paths", door_at, "walkable floor joining every door", true); }
        for b in bad { crate::furn::layers::complain(crate::furn::layers::Layer::Furniture, r, "reach", None, format!("{b} reachable from the doors"), false); }
        if std::env::var_os("ROOMTIMES").is_some() { eprintln!("ROOMTIME {r} large {:.2} medium {:.2} small {:.2} objs {}", t.0, t.1, t.2, st.objs.len()); }
        (st, t, crate::furn::layers::drain())
    };
    let jobs: Vec<(&String, State)> = rooms.iter().map(|r| (r, clone_state(state))).collect();
    let results: Vec<(State, (f64, f64, f64), Vec<crate::furn::layers::Complaint>)> = if serial { jobs.into_iter().map(work).collect() } else { jobs.into_par_iter().map(work).collect() };
    let mut tot = (0.0, 0.0, 0.0);
    let mut complaints = vec![];
    for (r, (st, t, cs)) in rooms.iter().zip(results) {
        tot = (tot.0 + t.0, tot.1 + t.1, tot.2 + t.2);
        complaints.extend(cs);
        // the room's built-ins the room layer removed (alcove shelves where a window comes) go from the house too
        let rk = r.replace('/', "_");
        let gone: Vec<String> = state.objs.keys().filter(|k| k.contains("Feature_") && k.contains(&rk) && !st.objs.contains_key(k.as_str())).cloned().collect();
        for k in gone { state.objs.shift_remove(&k); }
        for (k, o) in st.objs {
            if base_keys.contains(&k) {
                // the room's built-ins the room layer moved (a fireplace with the living-room group) keep their new place
                if k.contains("Feature_") && k.contains(&r.replace('/', "_")) {
                    if let Some(b) = state.objs.get_mut(&k) { if b.pose.t != o.pose.t || b.pose.r != o.pose.r { b.pose = o.pose; } }
                }
                continue
            }
            if state.objs.contains_key(&k) {
                eprintln!("NAME COLLISION across rooms: {k} ({r}) dropped");
                continue;
            }
            state.objs.insert(k, o);
        }
    }
    for o in state.objs.values_mut() { o.active = true; }
    let keys: Vec<String> = state.objs.keys().cloned().collect();
    for k in keys { state.touch(&k); }
    (tot, complaints)
}


/// The wall layer's windows: each held-back window slides along its own exterior wall (same width, height and sill) to
/// the spot that keeps it clear of what stands against that wall -- tall pieces (wardrobes, bookcases, the TV, built-ins, kitchen
/// units) weigh most, a headboard a little, low pieces (a sofa's back, a bench) not at all -- never over another window, door or the
/// entrance, and near where the plan put it when that spot is free. Pictures and mirrors a window now covers come off the wall.
fn wall_layer_windows(c: &RoomConstants, state: &mut State, room_polys: &[(String, crate::shp::G)], fixed: &[Cutter], deferred: &[Cutter]) -> Vec<Cutter> {
    let mut out: Vec<Cutter> = vec![];
    let mut scores: Vec<f64> = vec![];
    let wb = |st: &State, k: &str| -> ([f64; 3], [f64; 3]) {
        let o = &st.objs[k];
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    // the street side: the entrance's facade (the outdoor stage runs the road in front of it, crates/ig_outdoor/src/site.rs); a
    // window on a wall facing that way is preferred (a house showed the street a long blank wall: its rooms' windows had all gone
    // to the side and back walls)
    let inside_any0 = |q: [f64; 2]| room_polys.iter().any(|(_, g)| crate::shp::contains_pt(g, q));
    let front_n: Option<[f64; 2]> = fixed.iter().find(|ct| ct.kind == CutKind::Entrance).map(|ct| {
        let n = [-ct.rot_z.sin(), ct.rot_z.cos()];
        if inside_any0([ct.loc[0] + n[0] * 0.6, ct.loc[1] + n[1] * 0.6]) { [-n[0], -n[1]] } else { n }
    });
    for ct in deferred {
        let room = &ct.rooms[0];
        let Some((_, poly)) = room_polys.iter().find(|(n, _)| n == room) else { out.push(ct.clone()); continue };
        // the room's exterior walls: outline edges whose outside (0.3 m out) lies in no room; the plan's own wall first
        let ring = crate::shp::exterior(poly);
        let p = [ct.loc[0], ct.loc[1]];
        let inside_any = |q: [f64; 2]| room_polys.iter().any(|(_, g)| crate::shp::contains_pt(g, q));
        let mut walls: Vec<(f64, [f64; 2], [f64; 2])> = vec![];
        for w in ring.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt();
            if l < 1.0 { continue }
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let nrm = [-dy / l, dx / l];
            let outside = [m[0] + nrm[0] * 0.3, m[1] + nrm[1] * 0.3];
            let outside2 = [m[0] - nrm[0] * 0.3, m[1] - nrm[1] * 0.3];
            if inside_any(outside) && inside_any(outside2) { continue }
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (l * l)).clamp(0.0, 1.0);
            let d = ((a[0] + dx * t - p[0]).powi(2) + (a[1] + dy * t - p[1]).powi(2)).sqrt();
            walls.push((d, a, b));
        }
        // the window's own wall first: the nearest one running along it (the nearest wall alone was sometimes the one round the
        // corner: the window kept its rotation there, across the wall -- no hole, a curtain on a blank wall)
        let runs_along = |a: &[f64; 2], b: &[f64; 2]| ((b[1] - a[1]).atan2(b[0] - a[0]) - ct.rot_z).cos().abs() > 0.99;
        walls.sort_by(|x, y| (!runs_along(&x.1, &x.2), x.0).partial_cmp(&(!runs_along(&y.1, &y.2), y.0)).unwrap());
        let w0 = ct.dims[0];
        let sill = ct.loc[2] - ct.dims[2] / 2.0;
        // 35 cm of wall beside the frame, down to 15 cm at a cost (a bedroom's one outside wall, 3 m with a 2 m wardrobe on it, had
        // 0.51 m clear: its window went behind the wardrobe)
        let margin_pref = c.wall_thickness / 2.0 + 0.35;
        let margin = c.wall_thickness / 2.0 + 0.15;
        // best over walls x widths x positions: penalty for what stands against the wall, a little for narrowing the window and for
        // leaving its own wall / spot
        let mut best: Option<(f64, Cutter)> = None;
        // what tall pieces the best spot still overlaps, m
        let mut best_tall = 0.0f64;
        // the best spot were the alcove shelves over it gone: (score, window, the shelves)
        let mut best_free: Option<(f64, Cutter, Vec<String>)> = None;
        let flip = walls.first().map_or(false, |(_, a, b)| ((b[1] - a[1]).atan2(b[0] - a[0]) - ct.rot_z).cos() < 0.0);
        for (wi, (_, a, b)) in walls.iter().enumerate() {
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            let along = |q: [f64; 2]| (q[0] - a[0]) * u[0] + (q[1] - a[1]) * u[1];
            // this wall faces the street (outward normal within ~25 deg of the entrance's): +1.1, a little more than leaving the
            // window's own wall costs
            let street = front_n.map_or(0.0, |f| {
                let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let mut n = [-u[1], u[0]];
                if inside_any0([m[0] + n[0] * 0.3, m[1] + n[1] * 0.3]) { n = [-n[0], -n[1]]; }
                if n[0] * f[0] + n[1] * f[1] > 0.9 { 1.1 } else { 0.0 }
            });
            let off = |q: [f64; 2]| ((q[0] - a[0]) * -u[1] + (q[1] - a[1]) * u[0]).abs();
            let mut spans: Vec<(f64, f64, f64)> = vec![];
            // alcove shelves beside a fireplace: removable when a window has nowhere else to go
            let mut alcs: Vec<(f64, f64, f64, String)> = vec![];
            let mut sinks: Vec<(f64, f64)> = vec![];
            let mut lows: Vec<(f64, f64, f64)> = vec![];
            let mut beds: Vec<(f64, f64)> = vec![];
            for (k, o) in &state.objs {
                let Some(g) = &o.generator else { continue };
                if crate::furn::layout2d::room_of(state, k).as_deref() != Some(room.as_str()) { continue }
                let (lo, hi) = wb(state, k);
                let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
                if corners.iter().map(|q| off(*q)).fold(f64::INFINITY, f64::min) > 0.6 { continue }
                let s0 = corners.iter().map(|q| along(*q)).fold(f64::INFINITY, f64::min);
                let s1 = corners.iter().map(|q| along(*q)).fold(f64::NEG_INFINITY, f64::max);
                let tall = hi[2] > sill + 0.02;
                // a counter run takes a window over its worktop (its wall cabinets come off there) when the sill clears the top
                // (a run along this wall; one turned across it meets the wall with its wall cabinets: a corner, not a counter)
                let counter_ok = g.class == "KitchenSpaceFactory" && (o.pose.r[0][0] * u[0] + o.pose.r[1][0] * u[1]).abs() < 0.3;
                let weight = match g.class.as_str() {
                    "WallArtFactory" | "MirrorFactory" | "RugFactory" | "CeilingLightFactory" | "SinkFactory" => 0.0,
                    "KitchenSpaceFactory" if counter_ok => 0.3,
                    // (a range under a window: the curtain, the draught)
                    "OvenFactory" => 30.0,
                    // (a headboard rising past the sill stands in front of the glass)
                    "BedFactory" => if tall { 6.0 } else { 2.0 },
                    "SimpleBookcaseFactory" | "LargeShelfFactory" | "CellShelfFactory" | "SingleCabinetFactory" if hi[2] - lo[2] < 1.3 => 0.5,
                    // (a wardrobe in front of the glass is never right, a bed under a sill is: at 10 a window went 11 cm behind a
                    // wardrobe's side rather than narrow beside it)
                    "WardrobeFactory" | "ShowerFactory" | "FridgeFactory" => 40.0,
                    "TVStandFactory" | "FireplaceFeature" | "BookshelfWallFeature" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "KitchenSpaceFactory" | "KitchenCabinetFactory" => 10.0,
                    _ if tall => 5.0,
                    _ => 0.0,
                };
                // (10 cm each side for the tall ones: their models reach past the footprint -- a shower's glass, a wardrobe's doors)
                let m = if weight >= 5.0 { 0.1 } else { 0.0 };
                if weight > 0.0 { spans.push((s0 - m, s1 + m, weight)); }
                // (and what stands on them: books, ornaments)
                let alc_parent = if g.class == "BookshelfWallFeature" { Some(k.clone()) } else { o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.get(t).and_then(|p| p.generator.as_ref()).map_or(false, |pg| pg.class == "BookshelfWallFeature")) };
                if let Some(a) = alc_parent { alcs.push((s0 - m, s1 + m, weight, a)); }
                // a kitchen sink draws the window over it
                if g.class == "SinkFactory" { sinks.push((s0, s1)); }
                // what a window over it needs its sill above: the worktop (a run's top is its wall cabinets'), the range's back guard,
                // a tap, the basin's dressing
                let need = match g.class.as_str() {
                    // (the worktop: the top of the run's base box, 0.8-1.05 m by its style)
                    "KitchenSpaceFactory" => Some(crate::furn::layout2d::prisms(state, k).iter().map(|p| p.z[1]).filter(|z| *z < lo[2] + 1.3).fold(lo[2] + 0.95, f64::max) + 0.03),
                    "OvenFactory" => Some(hi[2].max(lo[2] + 1.02) + 0.03),
                    // (a sink's tap and a basin's taps stand taller than their layout boxes; the cistern takes the dresser's items)
                    "SinkFactory" => Some(hi[2] + 0.3),
                    "ToiletFactory" => Some(hi[2] + 0.2),
                    "StandingSinkFactory" => Some(hi[2] + 0.35),
                    // (a window over a bed: its sill clear of the headboard)
                    "BedFactory" => Some(hi[2] + 0.05),
                    // (over a nightstand / side table: clear of the lamp on it -- a window over a headboard ran on over the nightstands)
                    "SideTableFactory" => Some(hi[2] + 0.55),
                    // (over a sofa or an armchair: clear of its back -- an armchair's back stood 11 cm into the glass)
                    "SofaFactory" | "ArmChairFactory" => Some(hi[2] + 0.05),
                    // (over a TV stand: clear of the TV on it -- a room's one outside wall held the TV)
                    "TVStandFactory" => Some(state.objs.iter().filter(|(_, c)| c.generator.as_ref().map_or(false, |cg| cg.class == "TVFactory") && c.relations.iter().any(|r| &r.target == k)).map(|(ck, _)| wb(state, ck).1[2]).fold(hi[2] + 0.8, f64::max) + 0.05),
                    // (low storage -- a sideboard, a low bookcase -- takes a window over it, its sill above its top)
                    "SimpleBookcaseFactory" | "CellShelfFactory" | "SingleCabinetFactory" | "LargeShelfFactory" if hi[2] - lo[2] < 1.3 => Some(hi[2] + 0.4),
                    _ => None,
                };
                if let Some(z) = need { lows.push((s0, s1, z)); }
                if g.class == "BedFactory" { beds.push((s0, s1)); }
            }
            let mut taken: Vec<(f64, f64)> = vec![];
            for o in fixed.iter().chain(out.iter()) {
                if off([o.loc[0], o.loc[1]]) > 0.3 { continue }
                let s = along([o.loc[0], o.loc[1]]);
                if s < -0.3 || s > len + 0.3 { continue }
                taken.push((s - o.dims[0] / 2.0 - 0.2, s + o.dims[0] / 2.0 + 0.2));
            }
            if std::env::var_os("WLDBG").is_some() { eprintln!("WLDBG {room} wall {wi} {:?}->{:?} len {len:.2} spans {:?} taken {:?}", a, b, spans.iter().map(|(a, b, c)| (format!("{a:.2}"), format!("{b:.2}"), *c)).collect::<Vec<_>>(), taken); }
            let t0 = along(p);
            let mut w = w0;
            // (down to 0.6 m: a short wall half taken by a wardrobe had no 0.9 m stretch beside it)
            while w >= (0.6f64).max(w0 * 0.3) - 1e-9 {
                if len >= w + 2.0 * margin {
                    let steps = ((len - 2.0 * margin - w) / 0.1).floor() as usize;
                    for i in 0..=steps {
                        let t = margin + w / 2.0 + i as f64 * 0.1;
                        let (x0, x1) = (t - w / 2.0, t + w / 2.0);
                        if taken.iter().any(|(a0, a1)| x0 < *a1 && *a0 < x1) { continue }
                        // (outside along its whole width: a wall exterior at its middle bordered a bathroom further along -- a living-room
                        // window opened into it)
                        let nrm = [-u[1], u[0]];
                        let exterior = (0..=4).all(|i| { let tt = x0 + (x1 - x0) * i as f64 / 4.0; let q = [a[0] + u[0] * tt, a[1] + u[1] * tt];
                            let (o1, o2) = ([q[0] + nrm[0] * 0.3, q[1] + nrm[1] * 0.3], [q[0] - nrm[0] * 0.3, q[1] - nrm[1] * 0.3]); !(inside_any(o1) && inside_any(o2)) });
                        if !exterior { continue }
                        let pen: f64 = spans.iter().map(|(s0, s1, wt)| (x1.min(*s1) - x0.max(*s0)).max(0.0) * wt).sum();
                        let over_sink: f64 = sinks.iter().map(|(s0, s1)| 2.0 - ((s0 + s1) / 2.0 - t).abs().min(2.0)).fold(0.0, f64::max);
                        // over a counter / basin the sill rises to clear it (the head stays): at least 0.6 m of glass, a little penalty
                        let head = sill + ct.dims[2];
                        let need = lows.iter().filter(|(s0, s1, _)| x0 < *s1 && *s0 < x1).map(|(_, _, z)| *z).fold(sill, f64::max);
                        // (0.45 m of glass over a headboard: a tall one left a small bedroom's window only the stretch behind its wardrobe)
                        let over_bed = beds.iter().any(|(s0, s1)| x0 < *s1 && *s0 < x1);
                        if head - need < if over_bed { 0.45 } else { 0.6 } { continue }
                        let near_end = 4.0 * (margin_pref - x0.min(len - x1)).max(0.0);
                        let score = -near_end + over_sink - 0.5 * (need - sill) - pen - 1.0 * (w0 - w) - if wi == 0 { 0.2 * (t - t0).abs() } else { 1.0 } + street;
                        let over_alc: Vec<&(f64, f64, f64, String)> = alcs.iter().filter(|(s0, s1, _, _)| x0 < *s1 && *s0 < x1).collect();
                        let pen_alc: f64 = over_alc.iter().map(|(s0, s1, wt, _)| (x1.min(*s1) - x0.max(*s0)).max(0.0) * wt).sum();
                        let mut alc_keys: Vec<String> = over_alc.iter().map(|x| x.3.clone()).collect();
                        alc_keys.sort(); alc_keys.dedup();
                        let free_score = score + pen_alc - 1.0 * alc_keys.len() as f64;
                        if !over_alc.is_empty() && best_free.as_ref().map_or(true, |bb| free_score > bb.0) {
                            let mut nc = ct.clone();
                            let q = [a[0] + u[0] * t, a[1] + u[1] * t];
                            nc.loc = [q[0], q[1], (need + head) / 2.0];
                            nc.dims[0] = w;
                            nc.dims[2] = head - need;
                            nc.rot_z = if wi == 0 && runs_along(a, b) { ct.rot_z } else { u[1].atan2(u[0]) + if flip { std::f64::consts::PI } else { 0.0 } };
                            best_free = Some((free_score, nc, alc_keys.clone()));
                        }
                        if best.as_ref().map_or(true, |bb| score > bb.0) {
                            best_tall = spans.iter().filter(|(_, _, wt)| *wt >= 5.0).map(|(s0, s1, _)| (x1.min(*s1) - x0.max(*s0)).max(0.0)).sum();
                            let mut nc = ct.clone();
                            let q = [a[0] + u[0] * t, a[1] + u[1] * t];
                            nc.loc = [q[0], q[1], (need + head) / 2.0];
                            nc.dims[0] = w;
                            nc.dims[2] = head - need;
                            // its rotation follows the wall's direction, turned like the plan's window is against its own wall (the
                            // outline runs one way round, so the same turn faces every exterior wall's window outward)
                            nc.rot_z = if wi == 0 && runs_along(a, b) { ct.rot_z } else { u[1].atan2(u[0]) + if flip { std::f64::consts::PI } else { 0.0 } };
                            best = Some((score, nc));
                        }
                    }
                }
                w -= 0.1;
            }
        }
        let Some((mut score, mut nc)) = best else { continue };
        // no clear stretch, but one behind alcove shelves: the shelves go (with their keep-outs and what stands on them)
        // a bathroom window with only a spot behind something tall (the shower): a small high window over it (a bathroom's frosted
        // strip; leaving it out gave the street side a long blank wall), or none when the wall is too low for one
        if best_tall > 0.02 && crate::rooms::state::room_type(room) == "bathroom" && best_free.as_ref().map_or(true, |b| b.0 < -5.0) {
            // the sill clears the tops of the tall pieces in front of it (a cabinet nearly to the ceiling hid the window inside)
            let half = nc.dims[0].min(0.9) / 2.0 + 0.1;
            let top = state.objs.iter().filter(|(k, o)| o.generator.is_some() && crate::furn::layout2d::room_of(state, k).as_deref() == Some(room.as_str()))
                .map(|(k, _)| wb(state, k)).filter(|(lo, hi)| {
                    nc.loc[0] + half > lo[0] - 0.6 && nc.loc[0] - half < hi[0] + 0.6 && nc.loc[1] + half > lo[1] - 0.6 && nc.loc[1] - half < hi[1] + 0.6
                }).map(|(_, hi)| hi[2]).fold(0.0f64, f64::max);
            let (sill, head) = ((c.wall_height - 0.7).max(2.05).max(top + 0.08), c.wall_height - c.wall_thickness / 2.0 - 0.2);
            if head - sill < 0.35 {
                if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {room}: window left out (only behind a tall piece)"); }
                continue;
            }
            nc.dims[0] = nc.dims[0].min(0.9);
            nc.dims[2] = head - sill;
            nc.loc[2] = (sill + head) / 2.0;
            score = -1.0;
            if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {room}: high window over a tall piece, sill {sill:.2} m"); }
        }
        if let Some((fs, fnc, keys)) = best_free {
            if score < -5.0 && fs >= -5.0 {
                for a in &keys {
                    let kids: Vec<String> = state.objs.keys().filter(|k| k.starts_with(&format!("{a}.keepout")) || state.objs[k.as_str()].relations.iter().any(|r| &r.target == a)).cloned().collect();
                    for k in kids { if state.objs.contains_key(&k) { crate::furn::layout2d::remove_tree(state, &k); } }
                    if state.objs.contains_key(a) { crate::furn::layout2d::remove_tree(state, a); }
                    if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {room}: alcove shelves {a} removed for a window"); }
                }
                score = fs; nc = fnc;
            }
        }
        // a window with nowhere to go but behind a wardrobe (a bedroom's one outside wall, 1.5 m, all wardrobe): the wardrobe goes, the
        // room keeps its daylight
        if score < -20.0 {
            let (c, sn) = (nc.rot_z.cos(), nc.rot_z.sin());
            let gone: Vec<String> = state.objs.iter().filter(|(k, o)| o.generator.as_ref().map_or(false, |g| g.class == "WardrobeFactory") && crate::furn::layout2d::room_of(state, k).as_deref() == Some(room.as_str()) && {
                let (lo, hi) = wb(state, k);
                // (its box in the window's frame overlapping the opening's width, within 0.8 m of the wall)
                let q: Vec<[f64; 2]> = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]].iter().map(|p| { let (dx, dy) = (p[0] - nc.loc[0], p[1] - nc.loc[1]); [dx * c + dy * sn, -dx * sn + dy * c] }).collect();
                let (a0, a1) = (q.iter().map(|v| v[0]).fold(f64::MAX, f64::min), q.iter().map(|v| v[0]).fold(f64::MIN, f64::max));
                let (b0, b1) = (q.iter().map(|v| v[1]).fold(f64::MAX, f64::min), q.iter().map(|v| v[1]).fold(f64::MIN, f64::max));
                a0 < nc.dims[0] / 2.0 && a1 > -nc.dims[0] / 2.0 && b0 < 0.8 && b1 > -0.8
            }).map(|(k, _)| k.clone()).collect();
            for k in &gone {
                crate::furn::layout2d::remove_tree(state, k);
                if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {room}: wardrobe {k} removed for the room's window"); }
            }
            if !gone.is_empty() { score = -1.0; }
        }
        scores.push(score);
        if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {room}: window {w0:.2} m -> {:.2} m at ({:.2}, {:.2}) (score {score:.2})", nc.dims[0], nc.loc[0], nc.loc[1]); }
        out.push(nc);
    }
    // a window with no clear stretch (behind a TV stand or a wardrobe) is left out when its room has another window
    let keep: Vec<bool> = (0..out.len()).map(|i| {
        let room = out[i].rooms.first();
        let others = (0..out.len()).any(|j| j != i && out[j].rooms.first() == room && (scores[j] >= -5.0 || scores[j] > scores[i])) || fixed.iter().any(|o| o.kind == CutKind::Window && o.rooms.first() == room);
        // (or when it is a utility room, a closet or a garage: none is better than one behind a shelf unit)
        let service = room.map_or(false, |r| matches!(crate::rooms::state::room_type(r), "utility" | "closet" | "garage"));
        // (a service room's window goes sooner: -2.5 is about the narrowing alone plus a tall piece's edge)
        let k = !(scores[i] < -5.0 && others) && !(service && scores[i] < -2.5);
        if !k && std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER {:?}: window left out (score {:.2}, the room has another)", room, scores[i]); }
        k
    }).collect();
    let mut it = keep.iter();
    out.retain(|_| *it.next().unwrap());
    // pictures and mirrors a window now covers come off the wall
    // (the plan's own windows too: a mirror hung beside one)
    let wins: Vec<([f64; 2], [f64; 2], f64, f64)> = out.iter().chain(fixed.iter().filter(|c| c.kind == CutKind::Window)).map(|ct| {
        let (cz, sz) = (ct.rot_z.cos(), ct.rot_z.sin());
        let hw = ct.dims[0] / 2.0 + 0.05;
        // along the wall its width (+15 cm), across it 35 cm each side of the centre line (past the wall face: a mirror on the face stayed)
        let ext = [(cz * hw).abs() + 0.15 * cz.abs() + 0.35 * sz.abs(), (sz * hw).abs() + 0.15 * sz.abs() + 0.35 * cz.abs()];
        ([ct.loc[0] - ext[0], ct.loc[1] - ext[1]], [ct.loc[0] + ext[0], ct.loc[1] + ext[1]], ct.loc[2] - ct.dims[2] / 2.0, ct.loc[2] + ct.dims[2] / 2.0)
    }).collect();
    // (towel rails and holders too)
    // (and potted plants: one stood in front of the glass)
    let hung: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| matches!(g.class.as_str(), "WallArtFactory" | "MirrorFactory" | "HardwareFactory" | "LargePlantContainerFactory"))).map(|(k, _)| k.clone()).collect();
    for k in hung {
        let (lo, hi) = wb(state, &k);
        if wins.iter().any(|(wl, wh, z0, z1)| lo[0] < wh[0] && wl[0] < hi[0] && lo[1] < wh[1] && wl[1] < hi[1] && lo[2] < *z1 && *z0 < hi[2]) {
            crate::furn::layout2d::remove_tree(state, &k);
            if std::env::var_os("LAYERDBG").is_some() { eprintln!("WALLLAYER removed {k} (a window there now)"); }
        }
    }
    out
}
