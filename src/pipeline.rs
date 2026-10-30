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
    let st = solver.solve(&mut a);
    if std::env::var_os("PLAN_STATS").is_some() {
        for (k, ar, cv, asp) in crate::rooms::score::room_stats(&st) {
            println!("PLAN {seed} {k} area {ar:.1} convexity {cv:.2} aspect {asp:.2}");
        }
        std::process::exit(0);
    }
    let sol = Solidifier { c: &c, graph: &solver.graphs[0], level: 0, enable_open: false, passage_prob: opt.passage_prob };
    let res = sol.solidify(&mut Rng::seeded(seed), &st);
    if let Ok(path) = std::env::var("PLAN_DUMP") {
        // floor plan only (room polygons + door / window / entrance cutters), then exit: no furniture
        use geos::Geom;
        let rooms: serde_json::Map<String, serde_json::Value> = st.objs.iter().filter_map(|(k, o)| Some((k.clone(), serde_json::json!(o.polygon.as_ref()?.to_wkt().ok()?)))).collect();
        let cut: Vec<_> = res.cutters.iter().map(|ct| serde_json::json!({"kind": format!("{:?}", ct.kind), "loc": ct.loc, "rot": ct.rot_z, "dim": ct.dims, "rooms": ct.rooms})).collect();
        std::fs::write(&path, serde_json::json!({"seed": seed, "wall_thickness": c.wall_thickness, "wall_height": c.wall_height, "rooms": rooms, "cutters": cut}).to_string()).unwrap();
        std::process::exit(0);
    }
    let shells = shell::build_shells(&c, &st, &res.cutters);
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
        o.generator = Some(GenRef { class: pf.class.to_string(), factory_seed: 0, inst_seed: 0, unseeded: None });
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
    timings.push(("state".into(), t.elapsed().as_secs_f64()));

    // --- stages ---
    let mut stages: IndexMap<String, Domain> = program_json["stages"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), parse_domain(v, &HashMap::default()))).collect();
    if let (Some(room), false) = (&opt.restrict_room, opt.whole_house) {
        let rt = Domain::new(TagSet::from([Tag::sem(room)])).with_tags(&TagSet::from([Tag::var("room")]));
        for d in stages.values_mut() {
            *d = domain_tag_substitute(d, Tag::var("room"), &rt).0;
        }
    }
    if opt.whole_house {
        let (tl, tm, ts) = solve_whole_house(opt, assets, &prog, &usage, &stages, &mut state);
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
    Generated { c, shells, cutters: res.cutters, state, timings, room_polys, features }
}

/// rooms + cutters only (no furniture): used for validation against reference dumps
pub fn generate_rooms_only(seed: u64) -> State {
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let assets = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    generate(&Options { seed, furn_seed: seed, restrict_room: None, whole_house: false, steps_large: 0, steps_medium: 0, steps_small: 0, features: Default::default(), passage_prob: 0.0, rooms: crate::rooms::graph::RoomSet::Fewer }, &assets, &prog).state
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
    let tl = t.elapsed().as_secs_f64();
    // populate_intermediate_pholders: shelves etc. become real assets so children can use their support surfaces
    for (k, o) in state.objs.iter_mut() {
        let Some(g) = o.generator.clone() else { continue };
        if usage.has_usage(&g.class, "AssetPlaceholderForChildren") {
            // spawn_asset creates a brand new blender object: same location / euler, unit scale
            let mut ob = o.pose.o;
            ob.scale = [1.0, 1.0, 1.0];
            // add_to_scene(preprocess=True) afterwards re-derives loc / euler / scale from the matrix
            let w = Pose::from_o32(ob).o.matrix_world();
            ob.set_matrix_world(&w);
            o.pose = Pose::from_o32(ob);
            if let Some(geom) = ctx.populated.get(&(g.class.clone(), g.factory_seed, g.inst_seed)) {
                o.geom = Arc::new(geom.clone());
            } else if let Some(geom) = assets.solver_asset(&g.class, g.factory_seed, g.inst_seed) {
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
    let tm = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut rng = Rng::seeded(hs("solve_small"));
    run_stage("obj_ontop_obj", "small", opt.steps_small, None, 3.0, state, &mut rng, &mut ctx);
    run_stage("obj_on_support", "small", opt.steps_small, Some(vec![MoveKind::Addition]), 1.0, state, &mut rng, &mut ctx);
    let ts = t.elapsed().as_secs_f64();
    (tl, tm, ts)
}

fn clone_state(s: &State) -> State {
    State { objs: s.objs.clone(), caches: Default::default(), next_version: s.next_version }
}

/// Furnish every room: rooms are independent in the constraint program (the solver only looks at the active room's objects), so each
/// room is solved on its own copy of the room state with its own rng stream and the results are merged in room-key order.
/// The first room of the target room type keeps the single-room rng stream (same furniture as `--only-target`).
fn solve_whole_house(opt: &Options, assets: &dyn Assets, prog: &Program, usage: &Usage, stages: &IndexMap<String, Domain>, state: &mut State) -> (f64, f64, f64) {
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
    let work = |(r, mut st): (&String, State)| -> (State, (f64, f64, f64)) {
        let tag = if target.as_ref() == Some(r) { None } else { Some(r.as_str()) };
        let t = solve_stages(opt, sa.get(), prog, usage, stages, &mut st, &no_limits, Some(r), tag);
        if std::env::var_os("ROOMTIMES").is_some() { eprintln!("ROOMTIME {r} large {:.2} medium {:.2} small {:.2} objs {}", t.0, t.1, t.2, st.objs.len()); }
        (st, t)
    };
    let jobs: Vec<(&String, State)> = rooms.iter().map(|r| (r, clone_state(state))).collect();
    let results: Vec<(State, (f64, f64, f64))> = if serial { jobs.into_iter().map(work).collect() } else { jobs.into_par_iter().map(work).collect() };
    let mut tot = (0.0, 0.0, 0.0);
    for (r, (st, t)) in rooms.iter().zip(results) {
        tot = (tot.0 + t.0, tot.1 + t.1, tot.2 + t.2);
        for (k, o) in st.objs {
            if base_keys.contains(&k) { continue }
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
    tot
}
