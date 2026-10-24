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
use std::time::Instant;

pub struct Generated {
    pub c: RoomConstants,
    pub shells: Vec<RoomMeshes>,
    pub cutters: Vec<Cutter>,
    pub state: State,
    pub timings: Vec<(String, f64)>,
    pub room_polys: Vec<(String, crate::shp::G)>,
}

fn sem_room_tag(rt: &str) -> Tag {
    let name: String = rt.split('-').map(|p| { let mut c = p.chars(); c.next().unwrap().to_uppercase().collect::<String>() + c.as_str() }).collect();
    Tag::sem(&name)
}

pub struct Options {
    pub seed: u64,
    pub restrict_room: Option<String>, // e.g. "Bedroom"
    pub steps_large: usize,
    pub steps_medium: usize,
    pub steps_small: usize,
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
    let mut g = Rng::seeded(seed);
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
    let cfg = PlanConfig { n_divide_trials: 25, iters_mult: 25, fewer_rooms: true };
    let solver = FloorPlanSolver::new(seed, &c, cfg);
    let mut a = Rng::seeded(int_hash(&[&seed.to_string(), "solve_rooms"]));
    let st = solver.solve(&mut a);
    let sol = Solidifier { c: &c, graph: &solver.graphs[0], level: 0, enable_open: false };
    let res = sol.solidify(&mut Rng::seeded(seed), &st);
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
        if let Ok(path) = std::env::var("STATE0") {
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
                    tris.push(crate::furn::geom::Tri { v: [verts[ix[0]], verts[ix[1]], verts[ix[2]]], tags: bits });
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
            let door = res.cutters.iter().any(|ct| ct.kind == CutKind::Door && ct.rooms.len() == 2 && ct.rooms.contains(k) && ct.rooms.contains(&r.target));
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
        };
        let n = counts.entry(base).or_insert(0);
        let name = if *n == 0 { base.to_string() } else { format!("{base}.{:03}", n) };
        *n += 1;
        let h = [ct.dims[0] / 2., ct.dims[1] / 2., ct.dims[2] / 2.];
        let g = ObjGeom::from_tris(box_tris([-h[0], -h[1], -h[2]], h, 0), vec![(Proxy::Box { center: [0.; 3], half: h }, u32::MAX)]);
        let kind_tag = if ct.kind == CutKind::Window { Tag::sem("Window") } else { Tag::sem("Door") };
        let tags = TagSet::from([kind_tag, Tag::sem("Cutter"), Tag::obj(&name)]);
        let mut o = ObjState::new(Arc::new(g), Pose::from_euler(ct.loc, [0., 0., ct.rot_z]), tags);
        o.relations = ct.rooms.iter().map(|r| RelState { relation: Relation::CutFrom, target: r.clone(), child_plane: 0, parent_plane: 0 }).collect();
        if let Ok(path) = std::env::var("STATE0") {
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
    timings.push(("state".into(), t.elapsed().as_secs_f64()));

    // --- stages ---
    let mut stages: IndexMap<String, Domain> = program_json["stages"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), parse_domain(v, &HashMap::default()))).collect();
    if let Some(room) = &opt.restrict_room {
        let rt = Domain::new(TagSet::from([Tag::sem(room)])).with_tags(&TagSet::from([Tag::var("room")]));
        for d in stages.values_mut() {
            *d = domain_tag_substitute(d, Tag::var("room"), &rt).0;
        }
    }
    let mut limits = HashMap::default();
    limits.insert("room".to_string(), 1usize);
    let mut ctx = Ctx { prog: &prog, usage: &usage, assets, bounds: vec![], singletons: HashMap::default(), oracle: Default::default(), populated: Default::default(), ctor_words: Default::default() };
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
        let assigns = iterate_assignments(&dom, state, &["room", "obj"], &limits);
        for asg in assigns {
            solve_objects(ctx, state, &dom, &asg, &StageParams { n_steps: steps, restrict_moves: restrict.clone(), addition_scalar: add }, cfg(), rng);
        }
        let _ = group;
    };
    let seed_s = seed.to_string();
    let t = Instant::now();
    let mut rng = Rng::seeded(int_hash(&[&seed_s, "solve_large"]));
    run_stage("on_floor_and_wall", "large", opt.steps_large, None, 3.0, &mut state, &mut rng, &mut ctx);
    run_stage("on_floor_freestanding", "large", opt.steps_large, None, 3.0, &mut state, &mut rng, &mut ctx);
    timings.push(("solve_large".into(), t.elapsed().as_secs_f64()));
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
    let mut rng = Rng::seeded(int_hash(&[&seed_s, "solve_medium"]));
    run_stage("on_wall", "medium", opt.steps_medium, None, 3.0, &mut state, &mut rng, &mut ctx);
    run_stage("on_ceiling", "medium", opt.steps_medium, None, 3.0, &mut state, &mut rng, &mut ctx);
    run_stage("side_obj", "medium", opt.steps_medium, None, 3.0, &mut state, &mut rng, &mut ctx);
    timings.push(("solve_medium".into(), t.elapsed().as_secs_f64()));
    let t = Instant::now();
    let mut rng = Rng::seeded(int_hash(&[&seed_s, "solve_small"]));
    run_stage("obj_ontop_obj", "small", opt.steps_small, None, 3.0, &mut state, &mut rng, &mut ctx);
    run_stage("obj_on_support", "small", opt.steps_small, Some(vec![MoveKind::Addition]), 1.0, &mut state, &mut rng, &mut ctx);
    timings.push(("solve_small".into(), t.elapsed().as_secs_f64()));
    Generated { c, shells, cutters: res.cutters, state, timings, room_polys }
}

/// rooms + cutters only (no furniture): used for validation against reference dumps
pub fn generate_rooms_only(seed: u64) -> State {
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let assets = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    generate(&Options { seed, restrict_room: None, steps_large: 0, steps_medium: 0, steps_small: 0 }, &assets, &prog).state
}
