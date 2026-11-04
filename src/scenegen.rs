//! gen2's scene build as a library function (shared by the `gen2` binary and the wasm module): rooms -> furniture solve -> assets -> mess/people -> budget.
//! The binary parses the CLI, loads the data files, calls `build_scene`, writes the glb + sidecar; wasm passes embedded data and returns the bytes.
use crate::furn::assets::TableAssets;
use crate::mesh::*;
use crate::pipeline::*;

pub struct Params {
    /// every generation setting (seed, room, mess, people, cloth, ... see `crate::config`)
    pub cfg: crate::config::GenConfig,
    /// the furnishing program json matching `cfg.program.dense` (rules are applied to it by `build_scene`)
    pub prog: serde_json::Value,
    pub fallback: TableAssets,
}

pub struct Built {
    pub scene: Scene,
    pub room_polys: Vec<(String, indoorgen::shp::G)>,
    pub wall_t: f64,
    pub t_scene: web_time::Instant,
    /// false after `no_glb`
    pub done: bool,
    /// the sidecar `variation` record (every factor's effective seed/settings and computed values); None when all factors are default
    pub variation: Option<serde_json::Value>,
    /// sidecar `objects` list (key, class, parent, room per generated object)
    pub objects: Vec<serde_json::Value>,
    /// sidecar `outdoor` record (terrain settings, vegetation spots for the renderer); None without the outdoor stage
    pub outdoor: Option<serde_json::Value>,
    /// the outdoor stage's inputs (rooms as WKT, final cutters, wall dimensions): gen2 writes them as X.house.json for
    /// `--outdoor-only` (src/gen2cli.rs), which re-runs only the outdoor stage
    pub house: serde_json::Value,
}

/// the outdoor stage's inputs as JSON (Built::house)
#[cfg(not(target_arch = "wasm32"))]
pub fn house_record(seed: u64, polys: &[(String, indoorgen::shp::G)], cutters: &[indoorgen::rooms::solidify::Cutter], c: &indoorgen::rooms::consts::RoomConstants) -> serde_json::Value {
    use geos::Geom;
    serde_json::json!({
        "seed": seed, "wall_thickness": c.wall_thickness, "wall_height": c.wall_height,
        "rooms": polys.iter().map(|(k, g)| serde_json::json!([k, g.to_wkt().unwrap_or_default()])).collect::<Vec<_>>(),
        "cutters": cutters.iter().map(|ct| serde_json::json!({"kind": format!("{:?}", ct.kind), "style": ct.style.name(), "loc": ct.loc, "rot_z": ct.rot_z, "dims": ct.dims, "rooms": ct.rooms})).collect::<Vec<_>>(),
    })
}

/// (browser build: no WKT, the record is empty -- --outdoor-only is a native tool)
#[cfg(target_arch = "wasm32")]
pub fn house_record(_seed: u64, _polys: &[(String, indoorgen::shp::G)], _cutters: &[indoorgen::rooms::solidify::Cutter], _c: &indoorgen::rooms::consts::RoomConstants) -> serde_json::Value { serde_json::Value::Null }

/// inverse of house_record: (seed, rooms, cutters, wall thickness, wall height)
#[cfg(not(target_arch = "wasm32"))]
pub fn house_inputs(j: &serde_json::Value) -> (u64, Vec<(String, indoorgen::shp::G)>, Vec<indoorgen::rooms::solidify::Cutter>, f64, f64) {
    use indoorgen::rooms::solidify::{CutKind, Cutter, DoorStyle};
    let rooms = j["rooms"].as_array().map(|a| a.iter().filter_map(|r| Some((r[0].as_str()?.to_string(), geos::Geometry::new_from_wkt(r[1].as_str()?).ok()?))).collect()).unwrap_or_default();
    let f3 = |v: &serde_json::Value| [0, 1, 2].map(|k| v[k].as_f64().unwrap_or(0.0));
    let cutters = j["cutters"].as_array().map(|a| a.iter().map(|c| Cutter {
        kind: match c["kind"].as_str().unwrap_or("") { "Door" => CutKind::Door, "Entrance" => CutKind::Entrance, "Passage" => CutKind::Passage, _ => CutKind::Window },
        style: match c["style"].as_str().unwrap_or("") { "double" => DoorStyle::Double, "sliding" => DoorStyle::Sliding, "sliding_glass" => DoorStyle::SlidingGlass, "half_wall" => DoorStyle::HalfWall, _ => DoorStyle::Swing },
        loc: f3(&c["loc"]), rot_z: c["rot_z"].as_f64().unwrap_or(0.0), dims: f3(&c["dims"]),
        rooms: c["rooms"].as_array().map(|r| r.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
    }).collect()).unwrap_or_default();
    (j["seed"].as_u64().unwrap_or(0), rooms, cutters, j["wall_thickness"].as_f64().unwrap_or(0.1), j["wall_height"].as_f64().unwrap_or(2.7))
}

impl Built {
    /// add the variation record to the sidecar json (no-op when there is none)
    pub fn merge_sidecar(&self, side: &mut serde_json::Value) {
        side["objects"] = serde_json::Value::Array(self.objects.clone());
        if let Some(v) = &self.variation { side["variation"] = v.clone(); }
        if let Some(v) = &self.outdoor { side["outdoor"] = v.clone(); }
    }
}

thread_local! {
    /// set by `layout_only`: build_scene stops after the layout and leaves its JSON here
    static LAYOUT_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LAYOUT_OUT: std::cell::RefCell<Option<serde_json::Value>> = const { std::cell::RefCell::new(None) };
}

/// run the generator up to the furniture layout (the solve and its layout fixes) and return the layout JSON (layout_dump::to_json);
/// the 2D layout switches (furn::layout2d::knobs) must be set by the caller
pub fn layout_only(p: &Params) -> serde_json::Value {
    LAYOUT_ONLY.with(|c| c.set(true));
    let _ = build_scene(p);
    LAYOUT_ONLY.with(|c| c.set(false));
    LAYOUT_OUT.with(|o| o.borrow_mut().take()).unwrap_or(serde_json::Value::Null)
}

pub fn build_scene(p: &Params) -> Built {
    // caches keyed by names / versions / addresses that repeat from house to house (a second generation in one process -- the
    // wasm page -- read the first house's entries)
    crate::furn::dof::reset_caches();
    crate::furn::col::reset_caches();
    crate::furn::layout2d::reset_caches();
    crate::furn::eval::reset_caches();
    crate::furn::solver::reset_caches();
    #[cfg(not(target_arch = "wasm32"))]
    if !LAYOUT_ONLY.with(|c| c.get()) {
        indoorgen::furn::layout2d::knobs::set(&indoorgen::furn::layout2d::knobs::from_env());
        // tunable rules (LAYOUT_RULES_JSON: a file with {"layout": {...}, "plan": {...}}, any subset of fields)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(j) = std::env::var("LAYOUT_RULES_JSON").ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            if let Ok(r) = serde_json::from_value(j.get("layout").cloned().unwrap_or(serde_json::json!({}))) { indoorgen::furn::layout_rules::set(&r); }
            if let Ok(r) = serde_json::from_value(j.get("plan").cloned().unwrap_or(serde_json::json!({}))) { indoorgen::rooms::plan_rules::set(&r); }
        }
    }
    indoorgen::assets::bed::set_clean_geometry(true);
    let cfg = &p.cfg;
    cfg.install();
    // cloth that is exported as a sidecar job is re-simulated in Blender at render time: skip the Rust sim unless cloth.on
    indoorgen::assets::cloth::SKIP_JOB_SIM.store(!cfg.cloth.on, std::sync::atomic::Ordering::Relaxed);
    let seed = cfg.house.seed;
    let only_target = cfg.house.only_target;
    let var = &cfg.variation;
    let mess = indoorgen::mess::MessCfg { level: cfg.mess.level, seed: cfg.mess.seed.unwrap_or(seed), floor_scale: cfg.mess.floor_scale, surface_scale: cfg.mess.surface_scale };
    let furn_seed = var.furniture.seed.unwrap_or(seed);
    let people_seed = cfg.people.seed.unwrap_or(seed);
    let room = cfg.house.room.clone();
    let (people_n, mess_level, no_glb, qc_out, orig_prog) = (cfg.people.count, cfg.mess.level, cfg.output.no_glb, cfg.output.qc_path.clone(), !cfg.program.dense);
    // default: the dense-furnishing program + Infinigen's own solver steps 300/200/50 (realistic, lived-in rooms).
    // program.dense=false (INFINIGEN_PROGRAM=1): the program exactly as exported from python home.py + the fast_solve.gin steps 100/40/5 (selfcheck/lockstep semantics).
    let mut prog_owned;
    let mut prog = &p.prog;
    let feat_cfg = crate::features::FeatureCfg::resolve(&cfg.features, &var.features, seed);
    // with room features on, built-in rules fill them: decor on mantels / hearths, books and decor on bookshelf walls (user rules for the
    // same class + relation win)
    // dense program: realism rules (program.realism) and, with room features, the rules that furnish the features; user rules win
    let rules = if orig_prog { cfg.rules.clone() } else {
        let mut spec = cfg.rules.clone();
        if cfg.program.realism { spec = Some(crate::realism::with_realism_rules(spec.unwrap_or_default())) }
        if feat_cfg.on() { spec = Some(crate::features::with_feature_rules(spec.unwrap_or_default())) }
        spec
    };
    if let Some(spec) = &rules {
        // the rules pass over the 6.6 MB program is ~10 s single-threaded and identical for every house of a config: cached on disk,
        // keyed by the program, the spec, the feature flag and this binary's build (a rebuild invalidates it). RULES_NOCACHE=1 skips it.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&p.prog).unwrap_or_default().hash(&mut h);
            serde_json::to_string(spec).unwrap_or_default().hash(&mut h);
            (feat_cfg.on() && !orig_prog).hash(&mut h);
            #[cfg(not(target_arch = "wasm32"))]
            std::env::current_exe().ok().and_then(|e| std::fs::metadata(e).ok()).and_then(|m| m.modified().ok()).map(|t| format!("{t:?}")).hash(&mut h);
            h.finish()
        };
        // browser build: no filesystem; the merged program is kept in memory for the next generate call of the page
        #[cfg(target_arch = "wasm32")]
        {
            thread_local! { static MEM: std::cell::RefCell<Option<(u64, serde_json::Value)>> = const { std::cell::RefCell::new(None) }; }
            prog_owned = match MEM.with(|m| m.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
                Some(v) => v,
                None => {
                    let mut v = p.prog.clone();
                    crate::rules::apply(&mut v, spec);
                    if feat_cfg.on() && !orig_prog { crate::rules::add_feature_scores(&mut v); }
                    MEM.with(|m| *m.borrow_mut() = Some((key, v.clone())));
                    v
                }
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
        let path = std::env::temp_dir().join("indoorgen_rules_cache").join(format!("{key:016x}.json"));
        let cached = if std::env::var_os("RULES_NOCACHE").is_some() { None } else { std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) };
        if let Some(v) = cached { prog_owned = v; } else {
            prog_owned = p.prog.clone();
            crate::rules::apply(&mut prog_owned, spec);
            if feat_cfg.on() && !orig_prog { crate::rules::add_feature_scores(&mut prog_owned); }
            if std::env::var_os("RULES_NOCACHE").is_none() {
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let tmp = path.with_extension(format!("tmp{}", std::process::id()));
                if std::fs::write(&tmp, serde_json::to_vec(&prog_owned).unwrap_or_default()).is_ok() { let _ = std::fs::rename(&tmp, &path); }
            }
        }
        }
        prog = &prog_owned;
    }
    indoorgen::furn::solver::set_placeholder_reuse(!orig_prog && cfg.solver.placeholder_reuse);
    if !orig_prog { indoorgen::furn::dof::set_place_tries(cfg.solver.place_tries); }
    indoorgen::furn::dof::set_constructive(!orig_prog && cfg.solver.constructive, cfg.solver.place_cand);
    indoorgen::furn::dof::set_counter_over_windows(!orig_prog && cfg.program.counter_over_windows);
    if !orig_prog {
        #[cfg(not(target_arch = "wasm32"))]
        std::env::set_var("INFINIGEN_DENSE", "1");
        ig_core::envx::set_dense(true);
    }
    let (steps_l, steps_m, steps_s) = cfg.steps();
    let assets = indoorgen::assets::RealAssets { fallback: p.fallback.clone() };
    // classes ported to pieces (native collision) are laid out and built as one (PIECES=0: the old boxes and factories)
    if std::env::var("PIECES").map_or(true, |v| v != "0") { indoorgen::assets::install_pieces(); }
    let t = web_time::Instant::now();
    ig_core::envx::progress("floor plan + furniture solve");
    // the rooms the rules cover go to the unified solver (crates/ig_layout) alone; a single-room or parity run keeps the old passes
    let unified = cfg.program.realism && !orig_prog && !only_target;
    let mk_at = |nudges: Vec<(String, String, f64)>, plan_attempt: u32| Options { seed, furn_seed, restrict_room: room.clone(), whole_house: !only_target, steps_large: steps_l, steps_medium: steps_m, steps_small: steps_s, features: feat_cfg.clone(), passage_prob: if orig_prog { 0.0 } else { cfg.program.open_plan }, rooms: if orig_prog { crate::rooms::graph::RoomSet::Fewer } else { cfg.house.room_set() }, nudges, solver_rooms: if unified { indoorgen::unified::kinds() } else { vec![] }, plan_attempt };
    let mk = |nudges: Vec<(String, String, f64)>| mk_at(nudges, 0);
    let mut g = generate(&mk(vec![]), &assets, prog);
    // the plan layer answers the layers below (HIER=1): while hard complaints remain, the shared wall of the complaining room nearest
    // the complaint moves (0.25 / 0.5 m either way) and the house is solved again; a move stays when it leaves fewer hard complaints
    // (then fewer soft ones). At most 3 tries, within twice the first solve's time.
    if ig_core::envx::var_os("HIER").is_some() && !only_target {
        let base = t.elapsed().as_secs_f64();
        let key = |g: &Generated| (g.complaints.iter().filter(|c| c.hard).count(), g.complaints.iter().filter(|c| !c.hard).count());
        let mut best = key(&g);
        let mut kept: Vec<(String, String, f64)> = vec![];
        let mut tried: std::collections::HashSet<(String, String, i64)> = Default::default();
        eprintln!("HIER start: {} hard / {} soft complaints", best.0, best.1);
        for c in g.complaints.iter().filter(|c| c.hard) { eprintln!("HIER   {} {} {:?}: {}", c.room, c.kind, c.at, c.need); }
        let mut tries = 0;
        while best.0 > 0 && tries < 3 && t.elapsed().as_secs_f64() < 2.0 * base {
            let Some(c) = g.complaints.iter().find(|c| c.hard).cloned() else { break };
            // the room's shared walls (1 m or more), nearest the complaint first (the longest when it has no location)
            let Some(rp) = g.room_polys.iter().find(|(n, _)| *n == c.room).map(|(_, p)| crate::shp::buffer(p, 0.05)) else { break };
            let mut walls: Vec<(f64, String)> = g.room_polys.iter().filter(|(n, _)| *n != c.room).filter_map(|(n, p)| {
                let shared = crate::shp::intersection(&rp, &crate::shp::buffer(p, 0.05));
                let len = crate::shp::area(&shared) / 0.1;
                if len < 1.0 { return None }
                let d = c.at.map_or(-len, |a| crate::shp::distance(&crate::shp::point(a[0], a[1]), &shared));
                Some((d, n.clone()))
            }).collect();
            walls.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let Some((nb, d)) = walls.iter().flat_map(|(_, n)| [0.25f64, -0.25, 0.5, -0.5].map(|d| (n.clone(), d))).find(|(n, d)| !tried.contains(&(c.room.clone(), n.clone(), (d * 100.0) as i64))) else { break };
            tried.insert((c.room.clone(), nb.clone(), (d * 100.0) as i64));
            tries += 1;
            let mut ns = kept.clone();
            ns.push((c.room.clone(), nb.clone(), d));
            let ng = generate(&mk(ns.clone()), &assets, prog);
            let k = key(&ng);
            eprintln!("HIER try {tries}: {} grows {d:+.2} m into {nb} for {} -> {} hard / {} soft ({})", c.room, c.kind, k.0, k.1, if k < best { "kept" } else { "dropped" });
            if k < best { g = ng; best = k; kept = ns; }
        }
        eprintln!("HIER done: {} hard / {} soft complaints, {} wall moves, {:.1} s (first solve {base:.1} s)", best.0, best.1, kept.len(), t.elapsed().as_secs_f64());
    } else if ig_core::envx::var_os("LAYERDBG").is_some() {
        eprintln!("LAYERS {} hard / {} soft complaints", g.complaints.iter().filter(|c| c.hard).count(), g.complaints.iter().filter(|c| !c.hard).count());
    }
    // a plan with a room that cannot work (no place for a must item, a kitchen whose appliances cannot open) is planned again: at most twice, the plan with fewest such faults wins
    if unified { for l in indoorgen::unified::clear_windows(&mut g) { eprintln!("UNIFIED {l}"); } }
    let mut plan = unified.then(|| indoorgen::unified::solve_house(&g, seed));
    let mut faults = plan.as_ref().map_or(0, |p| indoorgen::unified::unfit(&g, p));
    for attempt in 1..=2 {
        if faults == 0 || plan.is_none() { break }
        // (time: a third plan only for a room with a must missing)
        if faults < 100 && t.elapsed().as_secs_f64() > 11.0 { break }
        // (the caches are keyed by name and version, which a new house starts over)
        crate::furn::dof::reset_caches(); crate::furn::col::reset_caches(); crate::furn::layout2d::reset_caches(); crate::furn::eval::reset_caches(); crate::furn::solver::reset_caches();
        let mut g2 = generate(&mk_at(vec![], attempt), &assets, prog);
        for l in indoorgen::unified::clear_windows(&mut g2) { eprintln!("UNIFIED {l}"); }
        let p2 = indoorgen::unified::solve_house(&g2, seed);
        let f2 = indoorgen::unified::unfit(&g2, &p2);
        eprintln!("PLAN again {attempt}: {faults} faults, {f2} now");
        if f2 < faults { g = g2; plan = Some(p2); faults = f2; }
    }
    if cfg.program.realism && !orig_prog {
        if let Some(spec) = &rules {
            let rm = crate::realism::enforce_caps(&mut g.state, spec);
            if !rm.is_empty() { eprintln!("CAPS removed {} objects over the rule maxima: {:?}", rm.len(), &rm[..rm.len().min(12)]); }
        }
        if cfg.program.top_dress {
            let gone = crate::dress::purge(&mut g.state);
            if !gone.is_empty() { eprintln!("DRESS purged {} solver items from dressed surfaces", gone.len()); }
        }
        for l in crate::table_setting::gather_chairs(&mut g.state) { eprintln!("CHAIRS {l}"); }
        for l in crate::table_setting::coffee_tables(&mut g.state) { eprintln!("COFFEE {l}"); }
        if let Some(p) = plan.take() { for l in indoorgen::unified::apply(&mut g, p, seed) { eprintln!("UNIFIED {l}"); } }
        for l in crate::table_setting::arrange(&mut g.state) { eprintln!("TABLE {l}"); }
        // (a living room's only seat, taken out to join the doors up, goes back on a wall that leaves them joined)
        let seats: Vec<(String, indoorgen::furn::state::ObjState)> = g.state.objs.iter().filter(|(k, o)| o.generator.as_ref().map_or(false, |gn| gn.class == "SofaFactory" || gn.class == "ArmChairFactory") && indoorgen::furn::layout2d::room_of(&g.state, k).map_or(false, |r| r.starts_with("living-room"))).map(|(k, o)| (k.clone(), o.clone())).collect();
        for l in indoorgen::furn::layout2d::repair(&mut g.state) { eprintln!("WALKWAY {l}"); }
        for l in indoorgen::furn::groups::reseat_living_rooms(&mut g.state, &seats) { eprintln!("WALKWAY {l}"); }
        // (a built-in the repair took out is not built)
        g.features.retain(|pf| g.state.objs.contains_key(&pf.key));
    }
    ig_core::envx::progress(&format!("solved ({:.1}s): {}", t.elapsed().as_secs_f64(), g.timings.iter().map(|(k, v)| format!("{k} {v:.1}s")).collect::<Vec<_>>().join(", ")));
    eprintln!("total {:?}  stages {:?}  cpu {:.2}", t.elapsed(), g.timings, indoorgen::assets::thread_cpu());
    if let Some(m) = indoorgen::furn::dof::PLACE_STATS.lock().unwrap().as_ref() {
        let (mut a, mut b, mut c) = (0, 0, 0);
        for (k, v) in m { eprintln!("PLACESTAT {k} placements {} tries {} ok {}", v.0, v.1, v.2); a += v.0; b += v.1; c += v.2; }
        eprintln!("PLACESTAT TOTAL placements {a} tries {b} ok {c}");
    }
    // dense program: every extra shelf gets filled with NatureShelfTrinkets (~250k triangles each; 120-200 per house took
    // 1-2 min of build time and most of the glb): at most 3 per shelf piece and 40 per house
    if !orig_prog {
        let (per_parent, total) = (cfg.program.trinkets_per_shelf, cfg.program.trinkets_max);
        let mut keys: Vec<String> = g.state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |gg| gg.class == "NatureShelfTrinketsFactory")).map(|(k, _)| k.clone()).collect();
        keys.sort();
        let mut per: std::collections::BTreeMap<String, usize> = Default::default();
        let (mut kept, mut gone) = (0usize, 0usize);
        for k in keys {
            let parent = g.state.objs[&k].relations.iter().map(|r| r.target.clone()).find(|t| g.state.objs.get(t).map_or(false, |o| o.generator.is_some())).unwrap_or_default();
            let n = per.entry(parent).or_default();
            if *n >= per_parent || kept >= total {
                g.state.objs.shift_remove(&k);
                gone += 1;
            } else {
                *n += 1;
                kept += 1;
            }
        }
        if gone > 0 { eprintln!("TRINKETS kept {kept}, removed {gone}"); }
    }
    if LAYOUT_ONLY.with(|c| c.get()) {
        LAYOUT_OUT.with(|o| *o.borrow_mut() = Some(crate::layout_dump::to_json(seed, &g)));
        return Built { scene: Scene::default(), room_polys: g.room_polys, wall_t: g.c.wall_thickness, t_scene: web_time::Instant::now(), done: false, variation: None, objects: vec![], outdoor: None, house: serde_json::Value::Null };
    }
    if let Ok(path) = std::env::var("LAYOUT_DUMP") {
        crate::layout_dump::dump(&path, seed, &g);
        std::process::exit(0);
    }
    if ig_core::envx::var_r("EVAL").is_ok() {
        let (prog, _) = indoorgen::pipeline::load_program(seed, prog);
        let mut memo = indoorgen::furn::eval::Memo::new(&prog, None);
        let mut ev = indoorgen::furn::eval::Evaluator::new(&prog, &g.state, &mut memo);
        let r = ev.evaluate_problem(true, true);
        eprintln!("FINAL loss {:.3} viol {:.1}", r.loss(), r.viol_count());
        eprintln!("terms {:?}", r.loss_vals.iter().filter(|x| x.1.abs() > 0.01).map(|x| (x.0.clone(), (x.1 * 100.0).round() / 100.0)).collect::<Vec<_>>());
        eprintln!("viol {:?}", r.violations.iter().filter(|x| x.1 > 0.0).collect::<Vec<_>>());
    }
    let t_scene = web_time::Instant::now();
    let lap = |what: &str| { ig_core::envx::progress(what); if ig_core::envx::var_os("LAPS").is_some() { eprintln!("LAP {what} {:.1}s since scene start", t_scene.elapsed().as_secs_f64()); } };
    let mut scene = Scene::default();
    let obj = scene.material(Material::diffuse("object", [0.6, 0.35, 0.3], 0.7));
    let room_list: Vec<(String, String)> = g.shells.iter().map(|s| (s.name.clone(), indoorgen::rooms::state::room_type(&s.name).to_string())).collect();
    ig_core::mesh::set_house_seed(seed);
    let mut decos = indoorgen::materials::decorate_rooms(seed, &room_list);
    // variation: wall / ceiling / floor base colours (own seeds; the procedural material specs are untouched, the renderer reads `variation.walls` etc.)
    let (wall_cols, ceil_cols, floor_cols) = (var.walls.pick_all(seed, "walls", &room_list), var.ceiling.pick_all(seed, "ceiling", &room_list), var.floor.pick_all(seed, "floor", &room_list));
    for ((name, _), rm) in room_list.iter().zip(decos.iter_mut()) {
        let set = |m: &mut Material, c: [f64; 3]| m.color = [c[0] as f32, c[1] as f32, c[2] as f32, 1.0];
        if let Some(c) = wall_cols.get(name) {
            set(&mut rm.wall, c.1);
            // a plaster half-wall / second material follows the wall colour; art / dark-art alternatives keep theirs
            if let Some(am) = rm.alt_wall.as_mut() { if am.spec.as_ref().map_or(false, |sp| sp["cls"] == "ceramic.Plaster") { set(am, c.1) } }
        }
        if let Some(c) = ceil_cols.get(name) { set(&mut rm.ceiling, c.1); }
        if let Some(c) = floor_cols.get(name) { set(&mut rm.floor, c.1); }
    }
    // the doorway reveal (shell.rs `reveal`) has a bottom quad at floor height in the WALL material: it showed as a white strip across
    // every doorway. Move those quads to the room's floor mesh so the floor runs continuously through the opening.
    let floor_z0 = g.c.wall_thickness / 2.0;
    let mut shells_fix: Vec<(indoorgen::mesh::Mesh, indoorgen::mesh::Mesh)> = vec![];
    for s in g.shells.iter() {
        let (mut wall, mut floor) = (s.wall.clone(), s.floor.clone());
        let is_sill = |f: &Vec<u32>| f.iter().all(|&i| (s.wall.pos[i as usize][2] - floor_z0).abs() < 1e-9);
        for f in s.wall.faces.iter().filter(|f| is_sill(f)) {
            let pts: Vec<_> = f.iter().map(|&i| s.wall.pos[i as usize]).collect();
            floor.add_face(&pts);
        }
        wall.faces.retain(|f| !is_sill(f));
        shells_fix.push((wall, floor));
    }
    for ((s, rm), (swall, sfloor)) in g.shells.iter().zip(decos).zip(shells_fix) {
        use indoorgen::materials::WallAlt;
        let (w, f, c) = (scene.material(rm.wall), scene.material(rm.floor), scene.material(rm.ceiling));
        let split = match rm.alt {
            WallAlt::None => None,
            WallAlt::Half { frac } => Some(indoorgen::rooms::walldeco::apply_half(&swall, g.c.wall_height, frac)),
            WallAlt::Aligned => Some(indoorgen::rooms::walldeco::apply_aligned(&swall)),
        };
        match (split, rm.alt_wall) {
            (Some((base, alt)), Some(am)) => {
                let ai = scene.material(am);
                scene.add(&format!("{}.wall", s.name), "room", base, w);
                scene.add(&format!("{}.wall.alt", s.name), "room", alt, ai);
            }
            _ => scene.add(&format!("{}.wall", s.name), "room", swall.clone(), w),
        }
        scene.add(&format!("{}.floor", s.name), "room", sfloor, f);
        scene.add(&format!("{}.ceiling", s.name), "room", s.ceiling.clone(), c);
    }
    let wt = g.c.wall_thickness;
    let polys: Vec<_> = g.room_polys.iter().map(|x| indoorgen::shp::buffer_mitre(&x.1, -wt / 2.0)).collect();
    // floor boards: python's contour is the union of the floor faces, which include the floor strip through every doorway;
    // the ring is then split at door-jamb (wall thickness) and opening (door width) segments, so no board runs across doors
    let mut floor_polys = polys.clone();
    for ct in &g.cutters {
        if !matches!(ct.kind, indoorgen::rooms::solidify::CutKind::Door | indoorgen::rooms::solidify::CutKind::Entrance | indoorgen::rooms::solidify::CutKind::Passage) {
            continue;
        }
        // (a half wall is wall to the floor: taken for a doorway, its boards stopped either side and left the bare wall under the opening)
        if ct.style == indoorgen::rooms::solidify::DoorStyle::HalfWall { continue }
        // (wide openings, double doors and sliders: their own width)
        let hw = if ct.kind == indoorgen::rooms::solidify::CutKind::Passage || ct.style != indoorgen::rooms::solidify::DoorStyle::Swing { ct.dims[0] / 2.0 } else { g.c.door_width / 2.0 };
        let hd = wt / 2.0 + 1e-4;
        let (c, s_) = (ct.rot_z.cos(), ct.rot_z.sin());
        let pt = |x: f64, y: f64| [ct.loc[0] + c * x - s_ * y, ct.loc[1] + s_ * x + c * y];
        floor_polys.push(indoorgen::shp::polygon(&[pt(-hw, -hd), pt(hw, -hd), pt(hw, hd), pt(-hw, hd), pt(-hw, -hd)], &[]).unwrap());
    }
    let mut passage_widths = vec![wt, g.c.door_width];
    passage_widths.extend(g.cutters.iter().filter(|ct| ct.style != indoorgen::rooms::solidify::DoorStyle::HalfWall && (ct.kind == indoorgen::rooms::solidify::CutKind::Passage || ct.style != indoorgen::rooms::solidify::DoorStyle::Swing)).map(|ct| ct.dims[0]));
    for (nm, ceil, z) in [("skirting_floor", false, wt / 2.0), ("skirting_ceiling", true, g.c.wall_height - wt / 2.0)] {
        let mut r = indoorgen::rng::Rng::seeded(indoorgen::int_hash(&[&seed.to_string(), nm]));
        let sk = if ceil { indoorgen::rooms::skirting::make(&polys, z, ceil, &mut r) } else { indoorgen::rooms::skirting::make_split(&floor_polys, z, ceil, &mut r, &passage_widths) };
        if let (Some(sk), true) = (&sk, ig_core::envx::var_r("SKIRTDBG").is_ok()) {
            for ct in &g.cutters {
                if !matches!(ct.kind, indoorgen::rooms::solidify::CutKind::Door | indoorgen::rooms::solidify::CutKind::Entrance) { continue }
                let (c, s_) = (ct.rot_z.cos(), ct.rot_z.sin());
                let inside = sk.mesh.pos.iter().filter(|p| {
                    let (dx, dy) = (p[0] - ct.loc[0], p[1] - ct.loc[1]);
                    let (lx, ly) = (c * dx + s_ * dy, -s_ * dx + c * dy);
                    lx.abs() < g.c.door_width / 2.0 - 0.02 && ly.abs() < wt / 2.0 + 0.06
                }).count();
                eprintln!("SKIRT {nm} door {:?} at ({:.2},{:.2}) verts inside opening: {inside}", ct.kind, ct.loc[0], ct.loc[1]);
            }
        }
        if let Some(sk) = sk {
            // skirting_board.py: shader_rough_plastic(base_color=hsv2rgba(white_hsv()), roughness=uniform(0.5, 1)) from the same FixedSeed stream
            // (thickness, height, white_hsv = (0, U(0, .1), U(.8, 1)), roughness)
            let mut pr = indoorgen::rng::Rng::seeded(indoorgen::int_hash(&[&seed.to_string(), nm]));
            pr.uniform(0.02, 0.05);
            pr.uniform(0.08, 0.15);
            let (sat, val) = (pr.uniform(0.0, 0.1), pr.uniform(0.8, 1.0));
            let rough = pr.uniform(0.5, 1.0);
            let col = [val, val * (1.0 - sat), val * (1.0 - sat), 1.0];
            let mut skm = Material::diffuse(nm, [0.85, 0.85, 0.85], sk.roughness);
            skm.spec = Some(serde_json::json!({"cls": "shader.plastic.plastic_rough.shader_rough_plastic", "kwargs": {"base_color": col, "roughness": rough}, "seed": 0}));
            let mi = scene.material(skm);
            scene.add(nm, "skirting", sk.mesh, mi);
        }
    }
    // mess pass, part 1 (before openings, whose doors avoid the moved furniture): furniture states on the solved layout (no-op at --mess 0)
    let jit = indoorgen::variation::jitter_furniture(&mut g.state, &var.furniture, seed);
    if jit.1 > 0 { eprintln!("JITTER {} of {} floor furniture groups moved", jit.0, jit.1); }
    indoorgen::mess::states::apply(&mut g.state, &mess);
    let mut ops = indoorgen::openings::make_openings(&g.c, &g.cutters, &g.room_polys, seed);
    // variation: door open status from its own stream (replaces Infinigen's draw; the mess pass then leaves the doors alone)
    if var.doors.active() {
        for d in ops.doors.iter_mut() { d.desired = var.doors.angle(seed, d.ci, d.desired); }
    }
    if mess.on() && !var.doors.active() {
        // mess pass: room door desired open angles varied (the opening pass limits them against furniture / walls below)
        let mut ang: Vec<(usize, f64)> = ops.doors.iter().map(|d| (d.ci, d.desired)).collect();
        let (n, tot) = indoorgen::mess::doors::vary_angles(&mut ang, &mess);
        for (d, (_, a)) in ops.doors.iter_mut().zip(&ang) {
            d.desired = *a;
        }
        let mut it: Vec<(&str, &mut indoorgen::mesh::Mesh)> = ops.pieces.iter_mut().map(|p| (p.name.as_str(), &mut p.mesh)).collect();
        let (cn, ctot) = indoorgen::mess::doors::draw_curtains(&mut it, &g.cutters, &mess);
        eprintln!("MESS doors changed {n} of {tot}; curtains changed {cn} of {ctot}");
    } else if mess.on() {
        let mut it: Vec<(&str, &mut indoorgen::mesh::Mesh)> = ops.pieces.iter_mut().map(|p| (p.name.as_str(), &mut p.mesh)).collect();
        let (cn, ctot) = indoorgen::mess::doors::draw_curtains(&mut it, &g.cutters, &mess);
        eprintln!("MESS curtains changed {cn} of {ctot} (door angles come from variation.doors)");
    }
    for p in std::mem::take(&mut ops.pieces) {
        let m = scene.material(p.mat);
        scene.add(&p.name, &p.class, p.mesh, m);
    }
    // lights_off stage: chance 0.2 (python: stage runs when uniform() <= chance)
    let lights_off = indoorgen::rng::Rng::seeded(indoorgen::int_hash(&[&seed.to_string(), "lights_off", "0"])).f64() <= 0.2;
    let sun = var.lighting.solve(seed);
    let night = sun.as_ref().map_or(false, |s| s.night);
    let mut lamp_rec: Vec<(String, bool, f64)> = vec![];
    if ig_core::envx::var_r("TILTCHECK").is_ok() {
        // every placed object must only be rotated about the vertical axis (python's solver never tilts furniture)
        let mut tilted = 0;
        for (k, o) in &g.state.objs {
            if let Some(gen) = &o.generator {
                let r = o.pose.r;
                // python itself leaves up to ~0.02 rad of tilt on objects stable against slightly sloped room planes (e.g. DiningRoom_0 Chair 26163);
                // the tilted-oven bug was far larger, so flag > 0.05 rad (~3 degrees)
                if r[2][0].abs() > 0.05 || r[2][1].abs() > 0.05 || r[0][2].abs() > 0.05 || r[1][2].abs() > 0.05 {
                    tilted += 1;
                    eprintln!("TILTED {k} {} r={:?}", gen.class, r);
                }
            }
        }
        eprintln!("TILTCHECK {} objects tilted", tilted);
    }
    if ig_core::envx::var_r("POSES").is_ok() {
        for (k, o) in &g.state.objs {
            if let Some(gen) = &o.generator {
                if matches!(gen.class.as_str(), "SofaFactory" | "TVStandFactory" | "CoffeeTableFactory" | "ArmChairFactory") {
                    eprintln!("POSE {k} t=({:.2},{:.2}) front=({:.2},{:.2}) lo={:?} hi={:?}", o.pose.t[0], o.pose.t[1], o.pose.r[0][0], o.pose.r[1][0], o.geom.lo.map(|v| (v * 100.0).round() / 100.0), o.geom.hi.map(|v| (v * 100.0).round() / 100.0));
                }
            }
        }
    }
    // build the final assets of all generated objects in parallel (each build is independent and deterministic; results are consumed in
    // the original order below)
    // one object's visual: built to its size / fitted to its layout box, the geometry budget applied (the parallel build below and
    // the closed-door rebuild of wardrobes)
    let build_visual = |k: &String, gen: &indoorgen::furn::state::GenRef, round: bool| -> indoorgen::assets::Asset {
            ig_kit::api::set_kitchen_style((gen.class == "KitchenSpaceFactory" || gen.class == "KitchenIslandFactory").then_some(gen.factory_seed));
            // a piece builds itself, at its own size and in its collision's frame
            if let Some(p) = &gen.piece {
                let mut a = p.mesh();
                indoorgen::budget::apply_asset(&mut a, &cfg.geometry);
                return a;
            }
            ig_kit::api::set_round_hint(round);
            indoorgen::assets::set_unseeded(gen.unseeded.clone());
            // the size the solver chose (furn::sizes): 'exact' classes build to it, the others are fitted to the box
            let exact = indoorgen::furn::sizes::model(&gen.class).map_or(false, |m| m.exact) || (gen.class == "RugFactory" && gen.size.is_some());
            if exact { ig_kit::api::set_size_hint(indoorgen::furn::solver::cur_size(gen)); }
            ig_kit::api::set_kitchen_join(gen.join);
            let mut a = assets.visual(&gen.class, gen.factory_seed, gen.inst_seed);
            ig_kit::api::set_kitchen_join(None);
            ig_kit::api::set_round_hint(false);
            // SIZEDBG: a class built to a size must come out at it (extents [width (y), depth (x), height (z)] against the hint)
            if exact && std::env::var_os("SIZEDBG").is_some() {
                if let Some(h) = indoorgen::furn::solver::cur_size(gen) {
                    let (lo, hi) = a.bounds();
                    let e = [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]];
                    let err = [0, 1, 2].map(|k| (e[k] - h[k]).abs());
                    let bad = err.iter().any(|v| *v > 0.02);
                    eprintln!("SIZEDBG {k} {} fs {} inst {} round {} hint [{:.3}, {:.3}, {:.3}] built [{:.3}, {:.3}, {:.3}] err [{:.3}, {:.3}, {:.3}]{}", gen.class, gen.factory_seed, gen.inst_seed, round, h[0], h[1], h[2], e[0], e[1], e[2], err[0], err[1], err[2], if bad { " OFF" } else { "" });
                }
            }
            // a class laid out by its parts goes onto its box by its layout build's bounds, as its parts did: by its own the fit
            // differed (a bed's real pillows against its layout's bedding box) and its headboard rose 11 cm past the raised sill
            let layout_bounds = if indoorgen::furn::solver::layout_real_class(&gen.class) { assets.layout_raw(&gen.class, gen.factory_seed, gen.inst_seed).map(|r| r.2) } else { None };
            if exact { ig_kit::api::set_size_hint(None); }
            // exact classes are built at the size but at their own origin (wardrobes: back-left corner): move them onto the box
            if let (true, Some((tlo, thi))) = (exact, gen.fit_box) {
                let (lo, hi) = layout_bounds.unwrap_or_else(|| a.bounds());
                // (an L leg's wall cabinet reaching on into the corner widens the model toward that end: kitchen_plan.rs)
                let reach = gen.join.map_or(0.0, |j| if j[2].abs() > 1.5 { 0.0 } else { j[2] * j[3] / 2.0 });
                let off = [tlo[0] - lo[0], (tlo[1] + thi[1]) / 2.0 + reach - (lo[1] + hi[1]) / 2.0, tlo[2] - lo[2]];
                if off.iter().any(|v| v.abs() > 1e-6) { a.transform(&indoorgen::mesh::Mat4::translation(off)); }
            }
            if let (false, Some((tlo, thi)), Some(m)) = (exact, gen.fit_box, indoorgen::furn::sizes::model(&gen.class)) {
                let (lo, hi) = layout_bounds.unwrap_or_else(|| a.bounds());
                if std::env::var_os("FITDBG").is_some() { eprintln!("FITDBG {k} {} asset {:.2?}..{:.2?} box {:.2?}..{:.2?}", gen.class, lo, hi, tlo, thi); }
                let (s, off) = indoorgen::furn::sizes::fit_matrix(m.uniform, lo, hi, tlo, thi);
                let mut mt = indoorgen::mesh::Mat4::scale(s);
                for k in 0..3 { mt.0[k][3] = off[k]; }
                a.transform(&mt);
                for c in a.cols.iter_mut() { let (p, q) = (mt.apply(c.0), mt.apply(c.1)); *c = (p, q); }
            }
            if ig_core::envx::var_os("BUDGETLOG").is_some() {
                let tc = |a: &indoorgen::assets::Asset| a.parts.iter().map(|p| p.mesh.faces.iter().map(|f| f.len().saturating_sub(2)).sum::<usize>()).sum::<usize>();
                let t0 = web_time::Instant::now();
                let before = tc(&a);
                indoorgen::budget::apply_asset(&mut a, &cfg.geometry);
                eprintln!("BUDGETLOG {} tris {} -> {} {:.3}s", gen.class, before, tc(&a), t0.elapsed().as_secs_f64());
            } else {
                indoorgen::budget::apply_asset(&mut a, &cfg.geometry);
            }
            indoorgen::assets::set_unseeded(None);
        a
    };
    for l in indoorgen::furn::groups::tuck_crowded_beds(&mut g.state) { eprintln!("GROUP {l}"); }
    let mut visuals: std::collections::HashMap<String, indoorgen::assets::Asset> = {
        use rayon::prelude::*;
        let items: Vec<(&String, &indoorgen::furn::state::GenRef, bool)> = g.state.objs.iter().filter(|(k, _)| !g.features.iter().any(|f| &f.key == *k)).filter_map(|(k, o)| o.generator.as_ref().map(|gen| (k, gen, o.tags.contains(indoorgen::furn::tags::Tag::sem("RoundTable"))))).collect();
        items
            .par_iter()
            .map(|(k, gen, round)| ((*k).clone(), build_visual(k, gen, *round)))
            .collect()
    };
    lap("visuals built");
    // through-wall safety net: a floor piece whose built footprint reaches more than 4 cm past its room's wall face (into the wall or the
    // next room) is removed with what stands on it -- rare solver failures at the inside corner of an L room (seeds 322, 323: a
    // cabinet 15-20 cm through the wall, its items outside every room). Wall-mounted, ceiling and rug pieces are not checked
    {
        let wt = g.c.wall_thickness;
        let inner: std::collections::HashMap<String, indoorgen::shp::G> = g.room_polys.iter().map(|(n, p)| (n.clone(), indoorgen::shp::buffer_mitre(p, -wt / 2.0 + 0.04))).collect();
        let mut bad: Vec<String> = vec![];
        for (k, o) in &g.state.objs {
            let Some(gen) = &o.generator else { continue };
            if g.features.iter().any(|f| &f.key == k) { continue }
            if ["WallArt", "Mirror", "CeilingLight", "Rug", "Window", "Door", "Hardware", "Curtain", "Shelf", "Kitchen"].iter().any(|c| gen.class.contains(c)) && gen.class != "LargeShelfFactory" { continue }
            let Some(a) = visuals.get(k) else { continue };
            let Some(room) = indoorgen::furn::layout2d::room_of(&g.state, k) else { continue };
            let Some(poly) = inner.get(&room) else { continue };
            // (its rigid parts: soft ones -- bedding -- may spread within the soft volume, and a corner of their box means nothing in an angled room)
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for (i, part) in a.parts.iter().enumerate() {
                if a.is_soft(i) || part.mesh.pos.is_empty() { continue }
                let (l, h) = part.mesh.bounds();
                for d in 0..3 { lo[d] = lo[d].min(l[d]); hi[d] = hi[d].max(h[d]); }
            }
            if lo[0] > hi[0] { continue }
            // only pieces standing on the floor (the bottom within 15 cm of it)
            if o.pose.apply([0.0, 0.0, lo[2]])[2] > wt / 2.0 + 0.15 { continue }
            let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
            let out = corners.iter().any(|c| { let p = o.pose.apply([c[0], c[1], lo[2]]); !indoorgen::shp::contains_pt(poly, [p[0], p[1]]) });
            // (a box corner past the wall is not the mesh past it: a round table's box in an angled room lost the room its table)
            let out = out && a.parts.iter().enumerate().any(|(i, part)| !a.is_soft(i) && part.mesh.pos.iter().any(|q| { let p = o.pose.apply(*q); !indoorgen::shp::contains_pt(poly, [p[0], p[1]]) }));
            if out { bad.push(k.clone()); }
        }
        for k in bad {
            let gone = indoorgen::furn::layout2d::remove_tree(&mut g.state, &k);
            for x in &gone { visuals.remove(x); }
            eprintln!("THROUGHWALL removed {k} (+{} on it)", gone.len().saturating_sub(1));
        }
    }
    // kitchen sinks (and storage) out of the wall: the tap at the back of a sink set into a run against the wall reached 2.2 cm into it; the sink shifts
    // by its overshoot along whichever horizontal axis brings every corner of its built box inside the wall face (up to 6 cm)
    {
        let wt = g.c.wall_thickness;
        let inner: std::collections::HashMap<String, indoorgen::shp::G> = g.room_polys.iter().map(|(n, p)| (n.clone(), indoorgen::shp::buffer_mitre(p, -wt / 2.0 - 0.005))).collect();
        let mut moves: Vec<(String, [f64; 2])> = vec![];
        for (k, o) in &g.state.objs {
            // (and free-standing storage a few cm into the wall: a cell shelf by 2.1 cm; the net above takes what is past 4 cm)
            if o.generator.as_ref().map_or(true, |gg| !matches!(gg.class.as_str(), "SinkFactory" | "CellShelfFactory" | "SimpleBookcaseFactory" | "LargeShelfFactory" | "SingleCabinetFactory" | "TVStandFactory" | "SideTableFactory" | "WardrobeFactory")) { continue }
            let (Some(a), Some(room)) = (visuals.get(k), indoorgen::furn::layout2d::room_of(&g.state, k)) else { continue };
            let Some(poly) = inner.get(&room) else { continue };
            let (lo, hi) = a.bounds();
            let pts: Vec<[f64; 2]> = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]].iter().map(|c| { let p = o.pose.apply([c[0], c[1], hi[2]]); [p[0], p[1]] }).collect();
            let edge = indoorgen::shp::boundary(poly);
            let over = pts.iter().filter(|p| !indoorgen::shp::contains_pt(poly, **p)).map(|p| indoorgen::shp::distance(&edge, &indoorgen::shp::point(p[0], p[1]))).fold(0.0f64, f64::max);
            // (flush pieces read a few mm past the face: only what is more than 1.5 cm in moves)
            if over <= 0.015 || over > 0.06 { continue }
            let d = over + 0.005;
            if let Some(sh) = [[d, 0.0], [-d, 0.0], [0.0, d], [0.0, -d]].into_iter().find(|sh| pts.iter().all(|p| indoorgen::shp::contains_pt(poly, [p[0] + sh[0], p[1] + sh[1]]))) { moves.push((k.clone(), sh)); }
        }
        for (k, sh) in moves {
            let o = g.state.objs.get_mut(&k).unwrap();
            let t = o.pose.t;
            o.pose.set_loc([t[0] + sh[0], t[1] + sh[1], t[2]]);
            eprintln!("OUTOFWALL {k}: moved {:.3} m", sh[0].abs() + sh[1].abs());
        }
    }
    // fridges flush with their counter run: a fridge shallower than the run, its back to the wall, stood 11-15 cm behind the counter
    // fronts (seeds 301, 318, ...); it moves forward to the run's front when nothing stands in the way (the invariants' appliance_flush)
    {
        let wb = |o: &indoorgen::furn::state::ObjState, lo: [f64; 3], hi: [f64; 3]| -> ([f64; 3], [f64; 3]) {
            let (mut a, mut b) = ([f64::MAX; 3], [f64::MIN; 3]);
            for i in 0..8 { let p = o.pose.apply([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]); for d in 0..3 { a[d] = a[d].min(p[d]); b[d] = b[d].max(p[d]); } }
            (a, b)
        };
        let boxes: std::collections::HashMap<String, ([f64; 3], [f64; 3])> = g.state.objs.iter().filter_map(|(k, o)| { let a = visuals.get(k)?; let (l, h) = a.bounds(); Some((k.clone(), wb(o, l, h))) }).collect();
        let mut moves: Vec<(String, [f64; 3])> = vec![];
        let mut shallow: Vec<(String, f64)> = vec![];
        for (k, o) in &g.state.objs {
            if o.generator.as_ref().map_or(true, |gg| !matches!(gg.class.as_str(), "FridgeFactory" | "OvenFactory")) { continue }
            let oven = o.generator.as_ref().map_or(false, |gg| gg.class == "OvenFactory");
            let Some(&(al, ah)) = boxes.get(k) else { continue };
            let fa = [o.pose.r[0][0], o.pose.r[1][0]];
            let ax = if fa[0].abs() > fa[1].abs() { 0 } else { 1 };
            let sgn = if fa[ax] > 0.0 { 1.0 } else { -1.0 };
            let side = 1 - ax;
            // the run beside it: a counter run facing the same way, touching it sideways (gap < 5 cm)
            let mut best: Option<f64> = None;
            for (rk, ro) in &g.state.objs {
                if ro.generator.as_ref().map_or(true, |gg| gg.class != "KitchenSpaceFactory") { continue }
                let fr = [ro.pose.r[0][0], ro.pose.r[1][0]];
                if fr[0] * fa[0] + fr[1] * fa[1] < 0.9 { continue }
                let Some(&(rl, rh)) = boxes.get(rk) else { continue };
                let gap = (rl[side] - ah[side]).max(al[side] - rh[side]);
                if gap > 0.05 { continue }
                let df = if sgn > 0.0 { ah[ax] - rh[ax] } else { rl[ax] - al[ax] };
                if best.map_or(true, |b| df.abs() < b.abs()) { best = Some(df); }
            }
            let Some(df) = best else { continue };
            // (a range more than 6 cm proud of the run moves back when the room has the space behind it: one stood 10 cm out)
            if oven && df > 0.06 {
                let mut d = [0.0; 3]; d[ax] = -sgn * df;
                let room = indoorgen::furn::layout2d::room_of(&g.state, k);
                let inner = room.and_then(|r| g.room_polys.iter().find(|(n, _)| *n == r).map(|(_, p)| indoorgen::shp::buffer_mitre(p, -g.c.wall_thickness / 2.0 + 0.005)));
                let ok = inner.map_or(false, |pg| [[al[0] + d[0], al[1] + d[1]], [ah[0] + d[0], al[1] + d[1]], [ah[0] + d[0], ah[1] + d[1]], [al[0] + d[0], ah[1] + d[1]]].iter().all(|q| indoorgen::shp::contains_pt(&pg, *q)));
                if ok { moves.push((k.clone(), d)); } else { shallow.push((k.clone(), df)); }
                continue;
            }
            // (and a recessed range or dishwasher comes forward too: one stood 8 cm behind its run's front, 1019)
            if df > -0.03 { continue }
            let shift = -df;
            let mut d = [0.0; 3]; d[ax] = sgn * shift;
            let (nl, nh) = ([al[0] + d[0], al[1] + d[1], al[2]], [ah[0] + d[0], ah[1] + d[1], ah[2]]);
            // the space the fridge moves into is clear (runs beside it excepted: they touch it by design)
            let clear = boxes.iter().all(|(ok, &(ol, oh))| {
                if ok == k { return true }
                if g.state.objs.get(ok).and_then(|x| x.generator.as_ref()).map_or(false, |gg| matches!(gg.class.as_str(), "KitchenSpaceFactory" | "RugFactory" | "CeilingLightFactory")) { return true }
                // what touched it before the move (on it, beside it) stays a non-issue
                let before = (0..3).all(|i| ol[i] < ah[i] && oh[i] > al[i]);
                before || !(0..3).all(|i| ol[i] < nh[i] - 0.005 && oh[i] > nl[i] + 0.005)
            });
            if clear { moves.push((k.clone(), d)); }
        }
        // (against the wall, no space behind: built that much shallower, about its back -- ovens are fitted to their box, not sized)
        for (k, df) in shallow {
            let Some(a) = visuals.get_mut(&k) else { continue };
            let (lo, hi) = a.bounds();
            let depth = hi[0] - lo[0];
            if depth < 0.3 || df > 0.25 * depth { continue }
            let f = (depth - df) / depth;
            let mut m = indoorgen::mesh::Mat4::scale([f, 1.0, 1.0]);
            m.0[0][3] = lo[0] * (1.0 - f);
            a.transform(&m);
            eprintln!("FLUSH {k}: built {df:.2} m shallower to the run's front");
        }
        for (k, d) in moves {
            let shift = d[0].abs() + d[1].abs();
            // (moved, not built deeper: the fridge is built to its layout size)
            let o = g.state.objs.get_mut(&k).unwrap();
            let t = o.pose.t;
            o.pose.set_loc([t[0] + d[0], t[1] + d[1], t[2]]);
            eprintln!("FLUSH {k}: moved {shift:.2} m to the run's front");
        }
    }
    // an open wardrobe door (wardrobe.rs: a quarter of hinged wardrobes stand with one bay open) whose swing would cut into a
    // neighbour is closed: the solver places against the closed box (seed 312: a door swung through the side table in front)
    {
        let wb = |o: &indoorgen::furn::state::ObjState, lo: [f64; 3], hi: [f64; 3]| -> ([f64; 3], [f64; 3]) {
            let (mut a, mut b) = ([f64::MAX; 3], [f64::MIN; 3]);
            for i in 0..8 { let p = o.pose.apply([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]); for d in 0..3 { a[d] = a[d].min(p[d]); b[d] = b[d].max(p[d]); } }
            (a, b)
        };
        let mut reclose: Vec<String> = vec![];
        for (k, o) in &g.state.objs {
            let Some(gen) = &o.generator else { continue };
            if gen.class != "WardrobeFactory" { continue }
            let Some(a) = visuals.get(k) else { continue };
            // the carcass depth: the first part (the cabinet body); anything past it in +x is an open door
            let Some(body) = a.parts.first() else { continue };
            let (bl, bh) = body.mesh.bounds();
            let (al, ah) = a.bounds();
            if ah[0] < bh[0] + 0.1 { continue }
            let (sl, sh) = wb(o, [bh[0] + 0.03, al[1], bl[2] + 0.05], [ah[0], ah[1], ah[2]]);
            // (and the openings: a window's zone 0.15 m into the room over the glass height, a door's 0.6 m passage)
            let hit_open = g.cutters.iter().any(|ct| {
                let (hw, depth, z0, z1) = match ct.kind {
                    indoorgen::rooms::solidify::CutKind::Window => (ct.dims[0] / 2.0, 0.15 + g.c.wall_thickness / 2.0, ct.loc[2] - ct.dims[2] / 2.0, ct.loc[2] + ct.dims[2] / 2.0),
                    indoorgen::rooms::solidify::CutKind::Passage => return false,
                    _ => (ct.dims[0] / 2.0, 0.6, ct.loc[2] - ct.dims[2] / 2.0, ct.loc[2] + ct.dims[2] / 2.0),
                };
                // the swing box's corners in the opening's frame (x along the wall, y through it)
                let (c, sn) = (ct.rot_z.cos(), ct.rot_z.sin());
                let pts = [[sl[0], sl[1]], [sh[0], sl[1]], [sh[0], sh[1]], [sl[0], sh[1]]].map(|p| { let (dx, dy) = (p[0] - ct.loc[0], p[1] - ct.loc[1]); [dx * c + dy * sn, -dx * sn + dy * c] });
                let (lx0, lx1) = (pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min), pts.iter().map(|p| p[0]).fold(f64::MIN, f64::max));
                let (ly0, ly1) = (pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min), pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max));
                lx0 < hw && lx1 > -hw && ly0 < depth && ly1 > -depth && sl[2] < z1 && sh[2] > z0
            });
            let hit = hit_open || g.state.objs.iter().any(|(k2, o2)| {
                if k2 == k || o2.generator.as_ref().map_or(true, |g2| g2.class.contains("Rug") || g2.class.contains("CeilingLight")) { return false }
                let Some(a2) = visuals.get(k2) else { return false };
                let (l2, h2) = a2.bounds();
                let (ol, oh) = wb(o2, l2, h2);
                (0..3).all(|d| ol[d] < sh[d] && oh[d] > sl[d])
            });
            if hit { reclose.push(k.clone()); }
        }
        for k in reclose {
            let gen = g.state.objs[&k].generator.clone().unwrap();
            ig_kit::api::set_closed_hint(true);
            let a = build_visual(&k, &gen, false);
            ig_kit::api::set_closed_hint(false);
            eprintln!("WARDROBE {k}: open door would hit a neighbour or an opening's zone -- closed");
            visuals.insert(k, a);
        }
    }
    // painted kitchens: 18 % of kitchens have every cabinet (runs, wall cabinets, the island) in one paint colour over the frame and fronts
    // (materials "shelf_*"; worktops, knobs and appliances keep theirs). Per room, its own stream (NO_PAINT=1: off; Blender only)
    if std::env::var_os("NO_PAINT").is_none() {
        for (k, o) in &g.state.objs {
            let Some(gen) = &o.generator else { continue };
            if !matches!(gen.class.as_str(), "KitchenSpaceFactory" | "KitchenCabinetFactory" | "KitchenIslandFactory") { continue }
            let Some(room) = indoorgen::furn::layout2d::room_of(&g.state, k) else { continue };
            let hs = indoorgen::int_hash(&["kitchen_paint", &seed.to_string(), &room]);
            if hs % 100 >= 18 { continue }
            let m = indoorgen::materials::painted_from(&format!("kitchen_paint:{room}"), hs, [0.85, 0.85, 0.85], &[0, 1, 2, 3, 5, 6, 7]);
            if let Some(a) = visuals.get_mut(k) { for p in a.parts.iter_mut() { if p.mat.name.starts_with("shelf_") { p.mat = m.clone(); } } }
        }
    }
    // room features: their built meshes (span frame, posed by the state object) are visuals from here on, so the snap passes see mantels,
    // hearths and shelves as the surfaces their children rest on; the features themselves are on every skip list (never moved / removed)
    for pf in &g.features { visuals.insert(pf.key.clone(), pf.asset.clone()); }
    let feature_keys: Vec<String> = g.features.iter().map(|f| f.key.clone()).collect();
    // python's KitchenCabinet visual starts at local y = 0 while its solver box is centred: align them (see snap.rs)
    if cfg.post.snap {
        let mv = indoorgen::snap::recenter_visuals(&g.state, &mut visuals, &["KitchenCabinetFactory"]);
        eprintln!("RECENTER {} KitchenCabinet visuals aligned with their solver box", mv.len());
        let mut cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
        cut_keys.extend(feature_keys.iter().cloned());
        let gone = indoorgen::snap::remove_overlapping(&mut g.state, &visuals, &cut_keys, 0.08, &g.room_polys);
        for (k, o) in &gone { eprintln!("OVERLAP removed {k} (inside {o})"); }
        for k in indoorgen::snap::remove_overlapping_rugs(&mut g.state, &visuals, 0.10) { eprintln!("OVERLAP removed rug {k} (overlaps a larger rug)"); }
        let inner_rooms: Vec<indoorgen::shp::G> = g.room_polys.iter().map(|(_, p)| indoorgen::shp::buffer_mitre(p, -g.c.wall_thickness / 2.0 + 0.02)).collect();
        for (k, what) in indoorgen::snap::rugs_clear_of(&mut g.state, &visuals, &feature_keys, 0.8, &|_, c| inner_rooms.iter().any(|r| indoorgen::shp::contains_pt(r, c))) { eprintln!("RUG {what} {k} (under a room feature)"); }
    }
    // mess pass, part 1b: states that edit the built assets (bed cloth, cabinet doors / drawers; no-op at --mess 0)
    indoorgen::mess::states::post_visuals(&mut visuals, &g.state, &mess);
    // after the door / drawer states (they address parts by index)
    if cfg.program.dense && cfg.program.counter_over_windows {
        for l in crate::kitchen_window::clear_uppers(&g.state, &mut visuals) { eprintln!("KITCHEN {l}"); }
        for l in crate::kitchen_window::fit_dishwashers(&g.state, &mut visuals) { eprintln!("KITCHEN {l}"); }
    }
    // supported objects rest on the VISIBLE surface of their parent (the solver used the python placeholder, which differs by cm)
    {
        let mut cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
        cut_keys.extend(feature_keys.iter().cloned());
        if cfg.post.snap {
            // repeated: an item buried in a double board first rests on the inner board, the next pass finds the real top
            let mut sn = vec![];
            for _ in 0..3 {
                let one = indoorgen::snap::snap_children(&mut g.state, &visuals, &cut_keys, g.c.wall_thickness / 2.0, 0.30, 0.03);
                let moved = one.iter().any(|s| s.removed || s.dz.abs() > 1e-4);
                sn.extend(one);
                if !moved { break }
            }
            if ig_core::envx::var_os("SNAPDBG").is_some() { for s in &sn { eprintln!("SNAP {} dz {:+.1} mm{}", s.key, s.dz * 1000.0, if s.removed { " REMOVED" } else { "" }); } }
            eprintln!("SNAP {} supported objects moved to the visible support surface", sn.len());
        }
        for k in crate::features::prune_feature_children(&mut g.state, &visuals, &g.features) { eprintln!("FEATURE removed {k} (taller than its shelf)"); }
    }
    if cfg.post.snap {
        let drawn = indoorgen::snap::untuck_chairs(&mut g.state, &visuals);
        for (k, s) in &drawn { eprintln!("UNTUCK {k} drawn back {:.1} cm (arms under the table top)", s * 100.0); }
        // (a chair resting on a table's foot, drawn back off it, was left in the air: settled again)
        if !drawn.is_empty() {
            let mut skip: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
            skip.extend(feature_keys.iter().cloned());
            indoorgen::snap::snap_children(&mut g.state, &visuals, &skip, g.c.wall_thickness / 2.0, 0.30, 0.03);
        }
        let fboxes: Vec<([f64; 3], [f64; 3])> = g.features.iter().map(|pf| {
            let p = pf.pose();
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for part in &pf.asset.parts { for q in &part.mesh.pos { let w = [0, 1, 2].map(|i| p.r[i][0] * q[0] + p.r[i][1] * q[1] + p.r[i][2] * q[2] + p.t[i]); for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); } } }
            (lo, hi)
        }).collect();
        let wt = g.c.wall_thickness;
        let inner: Vec<Vec<[f64; 2]>> = g.room_polys.iter().map(|(_, gg)| indoorgen::shp::exterior(&indoorgen::shp::buffer_mitre(gg, -wt / 2.0))).collect();
        for (k, m) in indoorgen::snap::clear_ceiling_lights(&mut g.state, &visuals, &fboxes, &inner) {
            match m { Some(d) => eprintln!("CEILLIGHT {k} moved ({:+.2}, {:+.2}) clear of a tall piece", d[0], d[1]), None => eprintln!("CEILLIGHT {k} removed (over a tall piece, no room to move)") }
        }
    }
    if cfg.post.snap && cfg.post.wall_push {
        let wt = g.c.wall_thickness;
        let inner: Vec<Vec<[f64; 2]>> = g.room_polys.iter().map(|(_, gg)| indoorgen::shp::exterior(&indoorgen::shp::buffer_mitre(gg, -wt / 2.0))).collect();
        for (k, dx, dy) in indoorgen::snap::push_out_of_walls(&mut g.state, &visuals, &inner, 0.35) { eprintln!("WALLPUSH {k} by ({:+.3},{:+.3})", dx, dy); }
    }
    // bookcases / bookshelf walls fill some of their bays with rows of books (dense program; sized to each bay on the final meshes)
    if !orig_prog {
        let rows = crate::shelffill::fill(&mut g.state, &mut visuals, furn_seed, cfg.post.shelf_books, if cfg.program.top_dress { Some(&assets) } else { None });
        if !rows.is_empty() { eprintln!("SHELFFILL {} rows of books", rows.len()); }
    }
    lap("snap done");
    // door leaves: swept from closed to the desired angle against the solved furniture (visual meshes) and walls
    if cfg.post.door_settle { indoorgen::openings::settle_doors(&mut ops, &g.cutters, &g.c, &g.state, &visuals, &g.room_polys); }
    lap("doors settled");
    let door_zones: Vec<([f64; 3], [f64; 3], f64)> = ops.doors.iter().flat_map(|d| d.zones(&g.cutters[d.ci], 0.08)).collect();
    let door_infos: Vec<indoorgen::qc::DoorInfo> = ops.doors.iter().map(|d| indoorgen::qc::DoorInfo { ci: d.ci, desired: d.desired, angle: d.angle, limiter: d.limiter.clone() }).collect();
    for d in std::mem::take(&mut ops.doors) {
        let ct = g.cutters[d.ci].clone();
        for p in d.world_pieces(&ct) {
            let m = scene.material(p.mat);
            scene.add(&p.name, &p.class, p.mesh, m);
        }
    }
    // cut-in sinks: python places the basin flush with the countertop front, but cabinet doors sit recessed below the counter
    // edge, so the basin stuck out past the cabinet fronts; move each sink back (towards the wall, sink local -x = tap side) so its
    // front is >= 6 cm behind the countertop's front edge, limited by the room behind it
    let sink_keys: Vec<String> = g.state.objs.iter().filter(|(k, _)| visuals.get(*k).map_or(false, |a| !a.cutters.is_empty())).map(|(k, _)| k.clone()).collect();
    for k in sink_keys {
        let o = &g.state.objs[&k];
        let Some(rs) = o.relations.first() else { continue };
        let (Some(t), Some(tv)) = (g.state.objs.get(&rs.target), visuals.get(&rs.target)) else { continue };
        let mat_of = |p: &indoorgen::furn::geom::Pose| Mat4([
            [p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]],
            [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]],
            [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let (sm, tm) = (mat_of(&o.pose), mat_of(&t.pose));
        let fdir = [o.pose.r[0][0], o.pose.r[1][0], o.pose.r[2][0]]; // sink local +x in world
        let along = |p: V3| dot(p, fdir);
        let sink_pts: Vec<f64> = visuals[&k].parts.iter().flat_map(|p| p.mesh.pos.iter().map(|q| along(sm.apply(*q)))).collect();
        let counter_pts: Vec<f64> = tv.parts.iter().filter(|p| p.mat.name.starts_with("counter")).flat_map(|p| p.mesh.pos.iter().map(|q| along(tm.apply(*q)))).collect();
        if sink_pts.is_empty() || counter_pts.is_empty() { continue }
        let (s_back, s_front) = (sink_pts.iter().cloned().fold(f64::MAX, f64::min), sink_pts.iter().cloned().fold(f64::MIN, f64::max));
        let (c_back, c_front) = (counter_pts.iter().cloned().fold(f64::MAX, f64::min), counter_pts.iter().cloned().fold(f64::MIN, f64::max));
        let want = s_front - (c_front - 0.06);
        let room = (s_back - c_back - 0.005).max(0.0);
        let shift = want.min(room);
        if shift > 1e-4 {
            if ig_core::envx::var_os("CUTDBG").is_some() { eprintln!("SINKSHIFT {k} back {:.3} m (wanted {:.3}, room {:.3})", shift, want, room); }
            let o = g.state.objs.get_mut(&k).unwrap();
            let mut p = o.pose.clone();
            for d in 0..3 { p.t[d] -= fdir[d] * shift; }
            o.pose = p;
        }
    }
    lap("sink shift");
    // python apply_cutter: the world-space cutter of each object is boolean-differenced from every part of its relation targets
    let mut cutters: std::collections::HashMap<String, Vec<indoorgen::mesh::Mesh>> = Default::default();
    for (k, o) in &g.state.objs {
        let Some(a) = visuals.get(k) else { continue };
        if a.cutters.is_empty() { continue }
        let pm = Mat4([
            [o.pose.r[0][0], o.pose.r[0][1], o.pose.r[0][2], o.pose.t[0]],
            [o.pose.r[1][0], o.pose.r[1][1], o.pose.r[1][2], o.pose.t[1]],
            [o.pose.r[2][0], o.pose.r[2][1], o.pose.r[2][2], o.pose.t[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let mut seen: Vec<&String> = vec![];
        for rs in &o.relations {
            if seen.contains(&&rs.target) { continue }
            seen.push(&rs.target);
            for c in &a.cutters {
                let mut w = c.clone();
                w.transform(&pm);
                if ig_core::envx::var_r("CUTDBG").is_ok() { let (l, h) = w.bounds(); eprintln!("CUTTER {k} -> {} lo {:?} hi {:?}", rs.target, l, h); }
                cutters.entry(rs.target.clone()).or_default().push(w);
            }
        }
    }
    let mut counts = std::collections::BTreeMap::new();
    // rooms without daylight (no window, no glazed door) and the small service rooms (bathrooms, closets, hallways, utility -- one small
    // window lets in next to nothing on an overcast day): one of their ceiling lights stays on whatever the lights-off draw says
    // (a bathroom with its lamps drawn off rendered murky at the auto exposure's limit; interiors are photographed with those lit)
    let lit_rooms: std::collections::HashSet<String> = g.cutters.iter()
        .filter(|ct| ct.kind == indoorgen::rooms::solidify::CutKind::Window || (ct.kind == indoorgen::rooms::solidify::CutKind::Entrance && ct.style == indoorgen::rooms::solidify::DoorStyle::SlidingGlass))
        .flat_map(|ct| ct.rooms.iter().cloned()).collect();
    let room_at = |p: [f64; 3]| g.room_polys.iter().find(|(_, poly)| indoorgen::shp::contains_pt(poly, [p[0], p[1]])).map(|(n, _)| n.clone());
    let mut forced_on: std::collections::HashSet<String> = Default::default();
    for (k, o) in &g.state.objs {
        let Some(gen) = &o.generator else { continue };
        *counts.entry(gen.class.clone()).or_insert(0) += 1;
        {
            // python's lights_off outcome is the default; variation.lighting decides otherwise. The bulb draws below are pure functions of the
            // factory seed, so computing them for lamps that stay off changes nothing.
            let mut on = var.lighting.lamp_on(seed, k, &gen.class, !lights_off, night);
            if matches!(gen.class.as_str(), "CeilingLightFactory" | "CeilingClassicLampFactory") {
                if let Some(rm) = room_at(o.pose.t) {
                    let service = ["bathroom", "closet", "hallway", "utility"].iter().any(|t| rm.starts_with(t));
                    if service || !lit_rooms.contains(&rm) {
                        if on { forced_on.insert(rm); } else if forced_on.insert(rm) { on = true; }
                    }
                }
            }
            let local = match gen.class.as_str() {
                "CeilingLightFactory" => Some([0.0, 0.0, -0.03]),
                "CeilingClassicLampFactory" => Some([0.0, 0.0, o.geom.lo[2] - 0.03]),
                "DeskLampFactory" | "FloorLampFactory" | "LampFactory" => {
                    // python lamp.py create_asset: `if np.random.uniform() < 0.6` (first draw of the spawn_asset instance stream) adds the
                    // bulb at local (0, 0, bound_box max z - ShadeHeight / 2), i.e. inside the shade
                    if indoorgen::assets::inst_rng(gen.factory_seed, gen.inst_seed).f64() < 0.6 {
                        let a = &visuals[k];
                        let zmax = a.parts.iter().flat_map(|p| p.mesh.pos.iter()).map(|q| q[2]).fold(f64::MIN, f64::max);
                        let (slo, shi) = a.parts[0].mesh.bounds(); // part 0 = shade
                        Some([0.0, 0.0, zmax - (shi[2] - slo[2]) * 0.5])
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(l) = local {
                let mut r = indoorgen::rng::Rng::seeded(gen.factory_seed);
                let watt = r.uniform(40.0, 100.0) * if gen.class.contains("Lamp") && !gen.class.contains("Ceiling") { 0.5 } else { 1.0 };
                let radius = r.uniform(0.02, 0.03);
                let mut temp;
                let mut tries = 0;
                loop {
                    temp = r.normal(4700.0, 700.0);
                    if (3500.0..=6500.0).contains(&temp) || tries >= 20 { break; }
                    tries += 1;
                }
                let temp = temp.clamp(3500.0, 6500.0);
                // LampFactory: bulb_fac Temperature = max(T * 0.6, 2500)
                let temp = if gen.class.contains("Lamp") && !gen.class.contains("Ceiling") { (temp * 0.6).max(2500.0) } else { temp };
                let watt = watt * var.lighting.lamp_strength_scale;
                lamp_rec.push((k.clone(), on, watt));
                let light = indoorgen::mesh::Light { loc: o.pose.apply(l), watt, radius, temperature: temp, name: k.clone() };
                if on { scene.lights.push(light) } else if var.record || var.lighting != Default::default() { scene.lights_off.push(light) }
            }
        }
        let asset = &visuals[k];
        let _ = obj;
        let pose_m = Mat4([
            [o.pose.r[0][0], o.pose.r[0][1], o.pose.r[0][2], o.pose.t[0]],
            [o.pose.r[1][0], o.pose.r[1][1], o.pose.r[1][2], o.pose.t[1]],
            [o.pose.r[2][0], o.pose.r[2][1], o.pose.r[2][2], o.pose.t[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        scene.cloth.extend(asset.cloth_json(k, &pose_m));
        for (pi, part) in asset.parts.iter().enumerate() {
            let mut m = part.mesh.clone();
            m.transform(&pose_m);
            if let Some(cs) = cutters.get(k) {
                for c in cs {
                    // piece by piece: a part can hold a whole cabinet run, and cutting it all tore the door fronts beside the sink
                    // (chosen in the object's own frame: on a unit standing at an angle the cutter's world box was wider than the sink, took in the door, and the boolean tore it)
                    let tb = web_time::Instant::now();
                    let nv0 = m.pos.len();
                    let to_local = |q: V3| { let r = &o.pose.r; let d = [q[0] - o.pose.t[0], q[1] - o.pose.t[1], q[2] - o.pose.t[2]]; [r[0][0] * d[0] + r[1][0] * d[1] + r[2][0] * d[2], r[0][1] * d[0] + r[1][1] * d[1] + r[2][1] * d[2], r[0][2] * d[0] + r[1][2] * d[1] + r[2][2] * d[2]] };
                    let mut local_c = c.clone();
                    for q in local_c.pos.iter_mut() { *q = to_local(*q); }
                    let mut local_m = m.clone();
                    for q in local_m.pos.iter_mut() { *q = to_local(*q); }
                    let (clo, chi) = local_c.bounds();
                    let (mut keep, mut cut) = split_for_cut(&local_m, clo, chi);
                    keep.pos = m.pos.clone();
                    cut.pos = m.pos.clone();
                    if !cut.faces.is_empty() {
                        let mut r = mesh_diff(&cut, c);
                        r.append(&keep);
                        m = r;
                        if ig_core::envx::var_os("CUTLOG").is_some() { eprintln!("CUT {k}#{pi} nv {nv0} -> {} cutter nv {} {:?}", m.pos.len(), c.pos.len(), tb.elapsed()); }
                    }
                }
            }
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&format!("{k}#{pi}"), &gen.class, m, mi, pose_m.0);
        }
    }
    lap("objects added");
    eprintln!("objects: {counts:?}  scene build {:?}", t_scene.elapsed());
    let tw = web_time::Instant::now();
    // people (no-op at --people 0): placements from the solved scene, posed avatar meshes added as nodes
    let people = indoorgen::people::place::place_ctx(&g.state, &g.room_polys, people_n, people_seed, &indoorgen::people::place::Ctx { visuals: Some(&visuals), floor_z: g.c.wall_thickness / 2.0, wall_t: g.c.wall_thickness, keepout: &door_zones });
    for (pi, pl) in people.iter().enumerate() {
        let mut a = pl.build();
        if a.parts.is_empty() && ig_core::envx::var_os("PEOPLE_PROXY").is_some() {
            a = pl.proxy();
        }
        for (k, part) in a.parts.iter().enumerate() {
            let mut m = part.mesh.clone();
            m.transform(&pl.pose);
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&format!("person{pi}_{:?}#{k}", pl.kind), "Person", m, mi, pl.pose.0);
        }
    }
    indoorgen::people::cover::apply(&people, &mut scene, g.c.wall_thickness / 2.0);
    if !people.is_empty() { indoorgen::people::pillows::settle(&people, &mut scene, g.c.wall_thickness / 2.0); }
    // the clutter pass keeps off the people (their occupied boxes, padded 4 cm)
    let mut keepout: Vec<indoorgen::mesh::Mesh> = people.iter().flat_map(|p| p.keepout(0.04)).collect();
    // ... and off the swept area of the room doors
    for (c, h, yaw) in &door_zones {
        let mut m = Mesh::cuboid([-h[0], -h[1], -h[2]], [h[0], h[1], h[2]]);
        m.transform(&Mat4::translation(*c).mul(&Mat4::rot_z(*yaw)));
        keepout.push(m);
    }
    // mess pass, part 2: clutter (no-op at --mess 0)
    let dress_on = cfg.program.top_dress && !orig_prog;
    crate::dress::ACTIVE.store(dress_on, std::sync::atomic::Ordering::Relaxed);
    *indoorgen::mess::clutter::NO_SURFACE.write().unwrap() = if dress_on { crate::dress::no_clutter_classes() } else { vec![] };
    indoorgen::mess::clutter::apply_keepout(&mut scene, &g.state, &visuals, &g.room_polys, &mess, &keepout);
    let cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
    fn qcin<'a>(scene: &'a Scene, g: &'a indoorgen::pipeline::Generated, door_infos: &'a [indoorgen::qc::DoorInfo], cut_keys: &'a [String]) -> indoorgen::qc::QcIn<'a> { indoorgen::qc::QcIn { scene, state: &g.state, room_polys: &g.room_polys, cutters: &g.cutters, doors: door_infos, wall_t: g.c.wall_thickness, cut_keys } }
    // clutter that ended up inside furniture / other clutter is removed (the placement works on convex hulls)
    if mess.on() && cfg.post.snap {
        let rep = indoorgen::qc::run(&qcin(&scene, &g, &door_infos, &cut_keys), std::path::Path::new(""), "");
        let gone = indoorgen::qc::prune_clutter(&mut scene, &rep);
        eprintln!("PRUNE {} clutter items removed for interpenetration", gone.len());
    }
    lap("people+mess+prune");
    if let Some(qp) = &qc_out {
        let label = format!("{}_{}_p{}_m{}", seed, room.as_deref().unwrap_or("all"), people_n, mess_level);
        let mut rep = indoorgen::qc::run(&qcin(&scene, &g, &door_infos, &cut_keys), std::path::Path::new(qp), &label);
        // people / pillow plausibility audit, merged into the report
        let pw = indoorgen::people::pworld::World::new(&g.state, &g.room_polys, Some(&visuals), g.c.wall_thickness / 2.0, g.c.wall_thickness);
        let pa = indoorgen::people::audit::audit(&pw, &people, &scene);
        for i in pa["issues"].as_array().into_iter().flatten() { eprintln!("QCP {label} {}", i.as_str().unwrap_or("")); }
        eprintln!("QCP {label} people={} kinds={} issues={}", pa["n"], pa["kinds"], pa["issues"].as_array().map_or(0, |a| a.len()));
        rep["people"] = pa;
        std::fs::write(qp, serde_json::to_vec_pretty(&rep).unwrap()).unwrap();
    }
    if ig_core::envx::var_os("SCENESTAT").is_some() {
        let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for n in &scene.nodes { let e = by.entry(n.class.clone()).or_default(); e.0 += 1; e.1 += n.mesh.faces.iter().map(|f| f.len().saturating_sub(2)).sum::<usize>(); }
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse((x.1).1));
        for (c, (n, t)) in v.iter().take(25) { eprintln!("SCENESTAT {c} nodes {n} tris {t}"); }
    }
    // sidecar `objects`: one entry per generated object (node-name prefix before '#', or door/window id before '.'), so a consumer can remove an object with
    // its children or select by class exactly
    let objects = {
        let mut order: Vec<String> = vec![];
        let mut info: std::collections::HashMap<String, (String, [f64; 3], [f64; 3])> = Default::default();
        for n in &scene.nodes {
            let key = if let Some((k, _)) = n.name.split_once('#') { k.to_string() } else if n.class == "Door" || n.class == "Window" { n.name.split('.').next().unwrap().to_string() } else { continue };
            if n.mesh.pos.is_empty() { continue }
            let (lo, hi) = n.mesh.bounds();
            let e = info.entry(key.clone()).or_insert_with(|| { order.push(key.clone()); (n.class.clone(), lo, hi) });
            for d in 0..3 { e.1[d] = e.1[d].min(lo[d]); e.2[d] = e.2[d].max(hi[d]); }
        }
        let room_of = |lo: [f64; 3], hi: [f64; 3]| -> serde_json::Value {
            let p = indoorgen::shp::point((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
            g.room_polys.iter().find(|(_, gg)| indoorgen::shp::contains(gg, &p)).map(|(n, _)| serde_json::json!(n)).unwrap_or(serde_json::Value::Null)
        };
        order.iter().map(|k| {
            let (cls, lo, hi) = &info[k];
            let (class, parent, room) = match g.state.objs.get(k) {
                Some(o) if o.generator.is_some() => {
                    let parent = o.relations.iter().map(|r| r.target.clone()).find(|t| g.state.objs.get(t).map_or(false, |x| x.generator.is_some()));
                    let room = o.relations.iter().map(|r| r.target.clone()).find(|t| g.state.objs.get(t).map_or(false, |x| x.polygon.is_some()));
                    (o.generator.as_ref().unwrap().class.clone(), parent.map_or(serde_json::Value::Null, |p| serde_json::json!(p)), room.map_or_else(|| room_of(*lo, *hi), |r| serde_json::json!(r)))
                }
                _ => (cls.clone(), serde_json::Value::Null, room_of(*lo, *hi)),
            };
            serde_json::json!({"key": k, "class": class, "parent": parent, "room": room})
        }).collect::<Vec<_>>()
    };
    // room features: pose, wall span and per-class layout facts (breast extents / alcoves for a fireplace) next to the object entry
    let mut objects = objects;
    for o in objects.iter_mut() {
        let key = o["key"].as_str().unwrap_or("").to_string();
        if let Some(pf) = g.features.iter().find(|f| f.key == key) {
            o["feature"] = serde_json::json!({"origin": pf.origin, "yaw": pf.yaw, "span_len": pf.span_len, "x": pf.x, "footprint": pf.footprint, "keepout": pf.keepout, "info": pf.info});
        }
    }
    if cfg.program.ceiling_decor && !orig_prog {
        let ceiling_z = g.c.wall_height - g.c.wall_thickness / 2.0;
        for l in crate::ceiling_decor::add(&mut scene, &g.state, &g.room_polys, seed, ceiling_z, g.c.wall_thickness, night) { eprintln!("CEILING {l}"); } lap("ceiling decor");
        for l in crate::ceiling_decor::fans(&mut scene, &g.state, &g.room_polys, seed, ceiling_z, g.c.wall_thickness / 2.0, g.c.wall_thickness, &cfg.program.ceiling_fan) { eprintln!("FAN {l}"); } lap("ceiling fans");
    }
    if cfg.program.soft_decor && !orig_prog {
        for l in crate::soft_decor::add(&mut scene, &g.state, &visuals, seed) { eprintln!("SOFT {l}"); } lap("soft decor");
    }
    if cfg.program.gallery_wall && !orig_prog {
        let ceiling_z = g.c.wall_height - g.c.wall_thickness / 2.0;
        for l in crate::gallery_wall::add(&mut scene, &g.state, &g.room_polys, seed, ceiling_z, g.c.wall_thickness) { eprintln!("GALLERY {l}"); } lap("gallery wall");
    }
    if cfg.program.bath_decor && !orig_prog {
        crate::bed_decor::WALL_HALF_BITS.store((g.c.wall_thickness / 2.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
        for l in crate::bath_decor::add(&mut scene, &g.state, &visuals, &assets, seed) { eprintln!("BATH {l}"); } lap("bath decor");
    }
    if cfg.program.top_dress && !orig_prog {
        for l in crate::dress::run(&mut scene, &g.state, &visuals, &g.features, &assets, seed) { eprintln!("DRESS {l}"); } lap("dressers");
        for l in crate::top_dress::add(&mut scene, &g.state, &visuals, &assets, seed) { eprintln!("DRESS {l}"); } lap("top dress");
    }
    if cfg.program.bed_decor && !orig_prog {
        for l in crate::bed_decor::add(&mut scene, &g.state, &g.room_polys, &door_zones, &assets, seed) { eprintln!("BEDDECOR {l}"); } lap("bed decor");
    }
    // invariant checks on the finished scene (every dresser has run): INVARIANTS=1
    if ig_core::envx::var_os("INVARIANTS").is_some() {
        let qi = qcin(&scene, &g, &door_infos, &cut_keys);
        let _ = crate::invariants::check(&crate::invariants::Inv { scene: &scene, state: &g.state, visuals: &visuals, qc: &qi, floor_z: g.c.wall_thickness / 2.0, ceiling_z: g.c.wall_height - g.c.wall_thickness / 2.0 });
        lap("invariants");
    }
    let house = house_record(seed, &g.room_polys, &g.cutters, &g.c);
    // terrain and vegetation outside (after the invariants: the terrain is not furniture)
    let outdoor = if cfg.program.outdoor.on && !orig_prog {
        let (rec, log) = crate::outdoor::add(&mut scene, &g.room_polys, &g.cutters, g.c.wall_height, seed, g.c.wall_thickness / 2.0, g.c.wall_thickness, &cfg.program.outdoor);
        for l in log { eprintln!("OUTDOOR {l}"); }
        lap("outdoor");
        Some(rec)
    } else { None };
    // sidecar `variation` record: every factor's effective seed / settings and the values the renderer needs (written only when some factor is not default)
    let variation = if var.record || *var != Default::default() || cfg.mess.seed.is_some() || cfg.people.seed.is_some() {
        Some(serde_json::json!({
            "house": {"seed": seed, "room": room},
            "doors": {"mode": var.doors.mode, "seed": var.doors.seed.unwrap_or(seed), "open_prob": var.doors.open_prob, "angle_deg_range": var.doors.angle_deg_range,
                "state": door_infos.iter().map(|d| (format!("door{}", d.ci), serde_json::json!({"desired_deg": d.desired.to_degrees(), "angle_deg": d.angle.to_degrees(), "open": d.angle.to_degrees() > 2.0, "limiter": d.limiter}))).collect::<serde_json::Map<_, _>>()},
            "walls": var.walls.record(seed, &wall_cols),
            "ceiling": var.ceiling.record(seed, &ceil_cols),
            "floor": var.floor.record(seed, &floor_cols),
            "mess": {"level": cfg.mess.level, "seed": mess.seed},
            "lighting": var.lighting.record(seed, &sun, &lamp_rec),
            "furniture": {"seed": furn_seed, "jitter": var.furniture.jitter, "jitter_moved": jit.0, "jitter_candidates": jit.1},
            "people": {"count": people_n, "seed": people_seed, "placed": people.len()},
        }))
    } else { None };
    if no_glb { return Built { scene, room_polys: g.room_polys, wall_t: g.c.wall_thickness, t_scene, done: false, variation, objects, outdoor, house }; }
    lap("qc done");
    { let tb = web_time::Instant::now(); let (a, b, k) = indoorgen::budget::apply(&mut scene, &cfg.geometry); if a > 0 { eprintln!("BUDGET tris {a} -> {b} ({k} nodes simplified) in {:.1}s", tb.elapsed().as_secs_f64()); } }
    Built { scene, room_polys: g.room_polys, wall_t: g.c.wall_thickness, t_scene, done: true, variation, objects, outdoor, house }
}

/// BSP boolean DIFFERENCE `a - b` on ig_core meshes (uv dropped: the exporter regenerates them). Only the faces of `a` near `b` go through
/// the boolean; the others are copied unchanged (keeps big counter tops cheap).
/// (what a cutter leaves alone, what it cuts), by connected piece. Cut: a piece with flat faces over a good part of the footprint (a countertop, a top board), or with
/// a board standing well inside it (a divider, which would stand up through the basin). A door front lies at the footprint's edge, and stays whole
fn split_for_cut(m: &Mesh, clo: V3, chi: V3) -> (Mesh, Mesh) {
    let key = |p: V3| [(p[0] * 2e4).round() as i64, (p[1] * 2e4).round() as i64, (p[2] * 2e4).round() as i64];
    let mut ids: std::collections::HashMap<[i64; 3], usize> = Default::default();
    let mut parent: Vec<usize> = vec![];
    fn find(p: &mut Vec<usize>, mut x: usize) -> usize { while p[x] != x { p[x] = p[p[x]]; x = p[x]; } x }
    let face_roots: Vec<usize> = m.faces.iter().map(|f| {
        let mut first = None;
        for &i in f {
            let n = parent.len();
            let id = *ids.entry(key(m.pos[i as usize])).or_insert_with(|| n);
            if id == n { parent.push(n); }
            match first { None => first = Some(id), Some(a) => { let (x, y) = (find(&mut parent, a), find(&mut parent, id)); parent[x] = y; } }
        }
        first.unwrap_or(0)
    }).collect();
    let roots: Vec<usize> = face_roots.iter().map(|&r| find(&mut parent, r)).collect();
    let mut wants: std::collections::HashMap<usize, f64> = Default::default();
    let mut board: std::collections::HashSet<usize> = Default::default();
    let mut reach: std::collections::HashSet<usize> = Default::default();
    let mut heights: std::collections::HashMap<usize, (f64, f64)> = Default::default();
    for (fi, f) in m.faces.iter().enumerate() {
        let ps: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
        let (fl, fh) = ps.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1]), l[2].min(q[2])], [h[0].max(q[0]), h[1].max(q[1]), h[2].max(q[2])]));
        { let e = heights.entry(roots[fi]).or_insert((f64::MAX, f64::MIN)); e.0 = e.0.min(fl[2]); e.1 = e.1.max(fh[2]); }
        if !(0..3).all(|d| fl[d] < chi[d] && fh[d] > clo[d]) { continue }
        let ins = 0.04;
        if (0..2).all(|d| fl[d] < chi[d] - ins && fh[d] > clo[d] + ins) && fh[2] > clo[2] + 0.02 { reach.insert(roots[fi]); }
        let mut nrm = [0.0; 3];
        for i in 0..ps.len() { let (a, b) = (ps[i], ps[(i + 1) % ps.len()]); nrm[0] += (a[1] - b[1]) * (a[2] + b[2]); nrm[1] += (a[2] - b[2]) * (a[0] + b[0]); nrm[2] += (a[0] - b[0]) * (a[1] + b[1]); }
        let area = 0.5 * dot(nrm, nrm).sqrt();
        if area > 1e-12 && (nrm[2] * 0.5 / area).abs() > 0.7 {
            let overlap = (fh[0].min(chi[0]) - fl[0].max(clo[0])) * (fh[1].min(chi[1]) - fl[1].max(clo[1]));
            *wants.entry(roots[fi]).or_insert(0.0) += area.min(overlap);
        }
    }
    // (the board's height, not one face's: a divider cut into short faces was left standing through the basin's floor)
    for r in reach { if heights.get(&r).map_or(false, |h| h.1 - h.0 > 0.1) { board.insert(r); } }
    let foot = (chi[0] - clo[0]) * (chi[1] - clo[1]);
    let (mut keep, mut cut) = (Mesh::new(), Mesh::new());
    keep.pos = m.pos.clone();
    cut.pos = m.pos.clone();
    for (fi, f) in m.faces.iter().enumerate() {
        let r = roots[fi];
        if board.contains(&r) || wants.get(&r).copied().unwrap_or(0.0) > 0.25 * foot { cut.faces.push(f.clone()); } else { keep.faces.push(f.clone()); }
    }
    (keep, cut)
}

fn mesh_diff(a: &Mesh, b: &Mesh) -> Mesh {
    use indoorgen::gn::geom::Mesh as GM;
    let (blo, bhi) = b.bounds();
    let split = if ig_core::envx::var_os("CUT_NOSPLIT").is_none() { Some(grid_split(a, blo, bhi, 0.1)) } else { None };
    let a = split.as_ref().unwrap_or(a);
    let e = 1e-3;
    // the boolean needs the closed part: an open near-faces piece leaves an inverted lid (BSP) or patchy hole walls (exact)
    let whole = ig_core::envx::var_os("CUT_PIECE").is_none();
    let near: Vec<bool> = a.faces.iter().map(|f| {
        if whole { return true }
        (0..3).all(|d| {
            let (mn, mx) = f.iter().fold((f64::MAX, f64::MIN), |(mn, mx), &i| (mn.min(a.pos[i as usize][d]), mx.max(a.pos[i as usize][d])));
            mn <= bhi[d] + e && mx >= blo[d] - e
        })
    }).collect();
    let mut sub = Mesh::new();
    let mut map = vec![u32::MAX; a.pos.len()];
    let mut o = Mesh::new();
    o.pos = a.pos.clone();
    for (fi, f) in a.faces.iter().enumerate() {
        if near[fi] {
            let mut nf = vec![];
            for &i in f {
                if map[i as usize] == u32::MAX { map[i as usize] = sub.pos.len() as u32; sub.pos.push(a.pos[i as usize]); }
                nf.push(map[i as usize]);
            }
            sub.faces.push(nf);
        } else {
            o.faces.push(f.clone());
        }
    }
    let conv = |m: &Mesh| GM::from_faces(m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), &m.faces, &[]);
    // (exact: the BSP left stray triangles standing on the countertop beside a sink's corner)
    let r = indoorgen::gn::csg::mesh_boolean_exact("DIFFERENCE", &conv(&sub), &[&conv(b)], false, true);
    let base = o.pos.len() as u32;
    o.pos.extend(r.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]));
    // `sub` is an open piece of the part, so the BSP inside test can keep cutter faces that lie outside it (e.g. the cutter's
    // top cap above the countertop): a difference never adds geometry outside the original piece, so drop those faces
    let (slo, shi) = sub.bounds();
    // ...and a difference never keeps material inside the cutter: drop faces whose centre is inside b by more than 1 mm
    // (hole walls lie on b's surface and stay); b is the closed cutter, tested by ray parity
    let btris: Vec<[V3; 3]> = b.faces.iter().flat_map(|f| (1..f.len() - 1).map(move |k| [f[0], f[k], f[k + 1]])).map(|t| t.map(|i| b.pos[i as usize])).collect();
    let inside_b = |p: V3| -> bool {
        let mut n = 0;
        for t in &btris {
            // ray +x from p (tiny y/z jitter avoids edge hits)
            let (o, d) = ([p[0], p[1] + 1.3e-7, p[2] + 2.9e-7], [1.0, 0.0, 0.0]);
            let (e1, e2) = (sub3(t[1], t[0]), sub3(t[2], t[0]));
            let h = cross(d, e2);
            let a = dot(e1, h);
            if a.abs() < 1e-12 { continue }
            let f = 1.0 / a;
            let sv = sub3(o, t[0]);
            let u = f * dot(sv, h);
            if !(0.0..=1.0).contains(&u) { continue }
            let q = cross(sv, e1);
            let v = f * dot(d, q);
            if v < 0.0 || u + v > 1.0 { continue }
            if f * dot(e2, q) > 0.0 { n += 1 }
        }
        n % 2 == 1
    };
    let dist_b = |p: V3| -> f64 { btris.iter().map(|t| pt_tri_dist(p, t)).fold(f64::MAX, f64::min) };
    let nofilter = ig_core::envx::var_os("CUT_NOFILTER").is_some();
    let ok = |f: &Vec<u32>| {
        if nofilter { return true }
        let n = f.len() as f64;
        let c: V3 = [0, 1, 2].map(|d| f.iter().map(|&i| r.pos[i as usize][d] as f64).sum::<f64>() / n);
        let in_bounds = (0..3).all(|d| c[d] >= slo[d] - 1e-3 && c[d] <= shi[d] + 1e-3) && f.iter().all(|&i| (0..3).all(|d| (r.pos[i as usize][d] as f64) >= slo[d] - 2e-3 && (r.pos[i as usize][d] as f64) <= shi[d] + 2e-3));
        let keep = in_bounds && !(inside_b(c) && dist_b(c) > 1e-3);
        if keep && ig_core::envx::var_os("CUTDBG").is_some() {
            let (blo, bhi) = b.bounds();
            if (0..3).all(|d| c[d] > blo[d] + 0.01 && c[d] < bhi[d] - 0.01) {
                eprintln!("CUTKEEP c=({:.3},{:.3},{:.3}) inside={} dist={:.4}", c[0], c[1], c[2], inside_b(c), dist_b(c));
            }
        }
        keep
    };
    o.faces.extend(r.faces_vec().into_iter().filter(|f| ok(f)).map(|f| f.into_iter().map(|x| x + base).collect::<Vec<u32>>()));
    o
}


/// Cut every face of `a` that crosses one of the grid planes x = k h / y = k h (k covering the cutter footprint +- 0.2 m) into convex pieces with
/// consistently computed points on shared edges. Long thin fan triangles of big counter tops made the BSP lose slivers (hairline cracks beside the sink).
fn grid_split(a: &Mesh, blo: V3, bhi: V3, h: f64) -> Mesh {
    let mut planes: Vec<(usize, f64)> = vec![];
    for d in 0..2 {
        let (k0, k1) = (((blo[d] - 0.2) / h).floor() as i64, ((bhi[d] + 0.2) / h).ceil() as i64);
        for k in k0..=k1 { planes.push((d, k as f64 * h)); }
    }
    let mut o = Mesh::new();
    let cut = |p: V3, q: V3, d: usize, c: f64| -> V3 {
        // canonical orientation so both neighbours compute bit-identical points
        let (p, q) = if (p[0], p[1], p[2]) <= (q[0], q[1], q[2]) { (p, q) } else { (q, p) };
        let t = (c - p[d]) / (q[d] - p[d]);
        let mut r = [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]), p[2] + t * (q[2] - p[2])];
        r[d] = c;
        r
    };
    for f in &a.faces {
        let pts: Vec<V3> = f.iter().map(|&i| a.pos[i as usize]).collect();
        let crosses = planes.iter().any(|&(d, c)| pts.iter().any(|p| p[d] < c - 1e-9) && pts.iter().any(|p| p[d] > c + 1e-9));
        if !crosses { o.add_face(&pts); continue }
        let mut polys: Vec<Vec<V3>> = triangulate_face(&a.pos, f).into_iter().map(|t| t.iter().map(|&i| a.pos[i as usize]).collect()).collect();
        for &(d, c) in &planes {
            let mut next = vec![];
            for poly in polys {
                if !(poly.iter().any(|p| p[d] < c - 1e-9) && poly.iter().any(|p| p[d] > c + 1e-9)) { next.push(poly); continue }
                let (mut lo, mut hi) = (vec![], vec![]);
                for i in 0..poly.len() {
                    let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                    let (sp, sq) = (p[d] - c, q[d] - c);
                    if sp <= 1e-9 { lo.push(p) }
                    if sp >= -1e-9 { hi.push(p) }
                    if (sp < -1e-9 && sq > 1e-9) || (sp > 1e-9 && sq < -1e-9) { let x = cut(p, q, d, c); lo.push(x); hi.push(x); }
                }
                if lo.len() >= 3 { next.push(lo) }
                if hi.len() >= 3 { next.push(hi) }
            }
            polys = next;
        }
        for p in polys { o.add_face(&p); }
    }
    o
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// distance from point p to triangle t (Ericson, Real-Time Collision Detection 5.1.5)
fn pt_tri_dist(p: V3, t: &[V3; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub3(b, a), sub3(c, a), sub3(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let len = |v: V3| dot(v, v).sqrt();
    if d1 <= 0.0 && d2 <= 0.0 { return len(ap) }
    let bp = sub3(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 { return len(bp) }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 { let v = d1 / (d1 - d3); return len(sub3(p, [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v])) }
    let cp = sub3(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 { return len(cp) }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 { let w = d2 / (d2 - d6); return len(sub3(p, [a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w])) }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return len(sub3(p, [b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w]));
    }
    let den = 1.0 / (va + vb + vc);
    let (v, w) = (vb * den, vc * den);
    len(sub3(p, [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]))
}
