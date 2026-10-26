// usage: gen2 <seed> [room] [out.glb] [--rust-cloth] [--mess LEVEL] [--cams K]
//   --cams K: K photographer-like camera poses per furnished room written to the sidecar json "cameras" (crates/ig_cams); 0 (default) = unchanged
//   --people N: up to N posed photoreal avatars placed after the solve (crates/ig_people); 0 (default) = unchanged output
//   --mess LEVEL: lived-in disorder 0..1 added after the solve (crates/ig_mess); 0 (default) = unchanged output
//   --rust-cloth: also run the native cloth sim for bed cloth that is exported as a sidecar job (default: skip, Blender re-simulates it)
use indoorgen::furn::assets::TableAssets;
use indoorgen::mesh::*;
use indoorgen::pipeline::*;

/// Load KEY=VALUE lines from `.indoorgen.env` (repo root, then the current directory) into the environment, without overriding
/// variables that are already set; `$HOME` / `~` are expanded. tools/env.sh reads the same file for the shell tools.
fn load_local_env() {
    let home = std::env::var("HOME").unwrap_or_default();
    for p in [std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".indoorgen.env"), std::path::PathBuf::from(".indoorgen.env")] {
        let Ok(txt) = std::fs::read_to_string(&p) else { continue };
        for line in txt.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().trim_start_matches("export ").trim(), v.trim().trim_matches('"'));
            if std::env::var_os(k).is_none() {
                let v = v.replace("$HOME", &home).replace("${HOME}", &home);
                let v = if let Some(r) = v.strip_prefix("~/") { format!("{home}/{r}") } else { v };
                std::env::set_var(k, v);
            }
        }
    }
}

fn main() {
    load_local_env();
    let mut args: Vec<String> = std::env::args().collect();
    // cloth that is exported as a sidecar job is re-simulated in Blender at render time: skip the Rust sim unless --rust-cloth
    if let Some(i) = args.iter().position(|a| a == "--rust-cloth") {
        args.remove(i);
    } else {
        indoorgen::assets::cloth::SKIP_JOB_SIM.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mess_level: f64 = match args.iter().position(|a| a == "--mess") {
        Some(i) => {
            let v = args[i + 1].parse().expect("--mess LEVEL");
            args.drain(i..i + 2);
            v
        }
        None => 0.0,
    };
    let people_n: usize = match args.iter().position(|a| a == "--people") {
        Some(i) => {
            let v = args[i + 1].parse().expect("--people N");
            args.drain(i..i + 2);
            v
        }
        None => 0,
    };
    let cams_k: usize = match args.iter().position(|a| a == "--cams") {
        Some(i) => {
            let v = args[i + 1].parse().expect("--cams K");
            args.drain(i..i + 2);
            v
        }
        None => 0,
    };
    let qc_out: Option<String> = args.iter().position(|a| a == "--qc").map(|i| { let v = args[i + 1].clone(); args.drain(i..i + 2); v });
    let no_glb = args.iter().position(|a| a == "--no-glb").map(|i| { args.remove(i); }).is_some();
    if people_n > 0 && indoorgen::people::avatar_count() == 0 {
        eprintln!("WARNING: --people {people_n} requested but no avatars found in {:?}: run tools/people/fetch_rocketbox.sh or set INDOORGEN_ASSETS (e.g. in .indoorgen.env); no people will be placed", indoorgen::people::avatar::root_dir());
    }
    let seed: u64 = args[1].parse().unwrap();
    let mess = indoorgen::mess::MessCfg { level: mess_level, seed };
    let room = args.get(2).cloned();
    let out = std::path::PathBuf::from(args.get(3).cloned().unwrap_or("/tmp/gen2.glb".into()));
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = indoorgen::assets::RealAssets { fallback };
    let t = std::time::Instant::now();
    let mut g = generate(&Options { seed, restrict_room: room, steps_large: std::env::var("STEPS_L").ok().and_then(|v| v.parse().ok()).unwrap_or(100), steps_medium: std::env::var("STEPS_M").ok().and_then(|v| v.parse().ok()).unwrap_or(40), steps_small: 5 }, &assets, &prog);
    eprintln!("total {:?}  stages {:?}  cpu {:.2}", t.elapsed(), g.timings, indoorgen::assets::thread_cpu());
    if std::env::var("EVAL").is_ok() {
        let (prog, _) = indoorgen::pipeline::load_program(seed, &prog);
        let mut memo = indoorgen::furn::eval::Memo::new(&prog, None);
        let mut ev = indoorgen::furn::eval::Evaluator::new(&prog, &g.state, &mut memo);
        let r = ev.evaluate_problem(true, true);
        eprintln!("FINAL loss {:.3} viol {:.1}", r.loss(), r.viol_count());
        eprintln!("terms {:?}", r.loss_vals.iter().filter(|x| x.1.abs() > 0.01).map(|x| (x.0.clone(), (x.1 * 100.0).round() / 100.0)).collect::<Vec<_>>());
        eprintln!("viol {:?}", r.violations.iter().filter(|x| x.1 > 0.0).collect::<Vec<_>>());
    }
    let t_scene = std::time::Instant::now();
    let mut scene = Scene::default();
    let obj = scene.material(Material::diffuse("object", [0.6, 0.35, 0.3], 0.7));
    let decos = indoorgen::materials::decorate_rooms(seed, &g.shells.iter().map(|s| (s.name.clone(), indoorgen::rooms::state::room_type(&s.name).to_string())).collect::<Vec<_>>());
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
        if !matches!(ct.kind, indoorgen::rooms::solidify::CutKind::Door | indoorgen::rooms::solidify::CutKind::Entrance) {
            continue;
        }
        let (hw, hd) = (g.c.door_width / 2.0, wt / 2.0 + 1e-4);
        let (c, s_) = (ct.rot_z.cos(), ct.rot_z.sin());
        let pt = |x: f64, y: f64| [ct.loc[0] + c * x - s_ * y, ct.loc[1] + s_ * x + c * y];
        floor_polys.push(indoorgen::shp::polygon(&[pt(-hw, -hd), pt(hw, -hd), pt(hw, hd), pt(-hw, hd), pt(-hw, -hd)], &[]).unwrap());
    }
    for (nm, ceil, z) in [("skirting_floor", false, wt / 2.0), ("skirting_ceiling", true, g.c.wall_height - wt / 2.0)] {
        let mut r = indoorgen::rng::Rng::seeded(indoorgen::int_hash(&[&seed.to_string(), nm]));
        let sk = if ceil { indoorgen::rooms::skirting::make(&polys, z, ceil, &mut r) } else { indoorgen::rooms::skirting::make_split(&floor_polys, z, ceil, &mut r, &[wt, g.c.door_width]) };
        if let (Some(sk), true) = (&sk, std::env::var("SKIRTDBG").is_ok()) {
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
    indoorgen::mess::states::apply(&mut g.state, &mess);
    let mut ops = indoorgen::openings::make_openings(&g.c, &g.cutters, &g.room_polys, seed);
    if mess.on() {
        // mess pass: room door desired open angles varied (the opening pass limits them against furniture / walls below)
        let mut ang: Vec<(usize, f64)> = ops.doors.iter().map(|d| (d.ci, d.desired)).collect();
        let (n, tot) = indoorgen::mess::doors::vary_angles(&mut ang, &mess);
        for (d, (_, a)) in ops.doors.iter_mut().zip(&ang) {
            d.desired = *a;
        }
        let mut it: Vec<(&str, &mut indoorgen::mesh::Mesh)> = ops.pieces.iter_mut().map(|p| (p.name.as_str(), &mut p.mesh)).collect();
        let (cn, ctot) = indoorgen::mess::doors::draw_curtains(&mut it, &g.cutters, &mess);
        eprintln!("MESS doors changed {n} of {tot}; curtains changed {cn} of {ctot}");
    }
    for p in std::mem::take(&mut ops.pieces) {
        let m = scene.material(p.mat);
        scene.add(&p.name, &p.class, p.mesh, m);
    }
    // lights_off stage: chance 0.2 (python: stage runs when uniform() <= chance)
    let lights_off = indoorgen::rng::Rng::seeded(indoorgen::int_hash(&[&seed.to_string(), "lights_off", "0"])).f64() <= 0.2;
    if std::env::var("TILTCHECK").is_ok() {
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
    if std::env::var("POSES").is_ok() {
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
    let mut visuals: std::collections::HashMap<String, indoorgen::assets::Asset> = {
        use rayon::prelude::*;
        let items: Vec<(&String, &indoorgen::furn::state::GenRef)> = g.state.objs.iter().filter_map(|(k, o)| o.generator.as_ref().map(|gen| (k, gen))).collect();
        items
            .par_iter()
            .map(|(k, gen)| {
                indoorgen::assets::set_unseeded(gen.unseeded.clone());
                let a = assets.visual(&gen.class, gen.factory_seed, gen.inst_seed);
                indoorgen::assets::set_unseeded(None);
                ((*k).clone(), a)
            })
            .collect()
    };
    // python's KitchenCabinet visual starts at local y = 0 while its solver box is centred: align them (see snap.rs)
    if std::env::var_os("NO_SNAP").is_none() {
        let mv = indoorgen::snap::recenter_visuals(&g.state, &mut visuals, &["KitchenCabinetFactory"]);
        eprintln!("RECENTER {} KitchenCabinet visuals aligned with their solver box", mv.len());
        let cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
        let gone = indoorgen::snap::remove_overlapping(&mut g.state, &visuals, &cut_keys, 0.08);
        for (k, o) in &gone { eprintln!("OVERLAP removed {k} (inside {o})"); }
    }
    // mess pass, part 1b: states that edit the built assets (bed cloth, cabinet doors / drawers; no-op at --mess 0)
    indoorgen::mess::states::post_visuals(&mut visuals, &g.state, &mess);
    // supported objects rest on the VISIBLE surface of their parent (the solver used the python placeholder, which differs by cm)
    {
        let cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
        if std::env::var_os("NO_SNAP").is_none() {
            // repeated: an item buried in a double board first rests on the inner board, the next pass finds the real top
            let mut sn = vec![];
            for _ in 0..3 {
                let one = indoorgen::snap::snap_children(&mut g.state, &visuals, &cut_keys, g.c.wall_thickness / 2.0, 0.30, 0.03);
                let moved = one.iter().any(|s| s.removed || s.dz.abs() > 1e-4);
                sn.extend(one);
                if !moved { break }
            }
            if std::env::var_os("SNAPDBG").is_some() { for s in &sn { eprintln!("SNAP {} dz {:+.1} mm{}", s.key, s.dz * 1000.0, if s.removed { " REMOVED" } else { "" }); } }
            eprintln!("SNAP {} supported objects moved to the visible support surface", sn.len());
        }
    }
    // door leaves: swept from closed to the desired angle against the solved furniture (visual meshes) and walls
    indoorgen::openings::settle_doors(&mut ops, &g.cutters, &g.c, &g.state, &visuals, &g.room_polys);
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
            if std::env::var_os("CUTDBG").is_some() { eprintln!("SINKSHIFT {k} back {:.3} m (wanted {:.3}, room {:.3})", shift, want, room); }
            let o = g.state.objs.get_mut(&k).unwrap();
            let mut p = o.pose.clone();
            for d in 0..3 { p.t[d] -= fdir[d] * shift; }
            o.pose = p;
        }
    }
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
                if std::env::var("CUTDBG").is_ok() { let (l, h) = w.bounds(); eprintln!("CUTTER {k} -> {} lo {:?} hi {:?}", rs.target, l, h); }
                cutters.entry(rs.target.clone()).or_default().push(w);
            }
        }
    }
    let mut counts = std::collections::BTreeMap::new();
    for (k, o) in &g.state.objs {
        let Some(gen) = &o.generator else { continue };
        *counts.entry(gen.class.clone()).or_insert(0) += 1;
        if !lights_off {
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
                scene.lights.push(indoorgen::mesh::Light { loc: o.pose.apply(l), watt, radius, temperature: temp, name: k.clone() });
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
                    let (mlo, mhi) = m.bounds();
                    let (clo, chi) = c.bounds();
                    // only cut parts that have horizontal surfaces inside the cutter (countertop, cabinet top boards); door and
                    // drawer fronts below the counter are vertical panels: cutting them exposed the basin from the front
                    let horiz_area: f64 = m.faces.iter().map(|f| {
                        let ps: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
                        let c: V3 = [0, 1, 2].map(|d| ps.iter().map(|p| p[d]).sum::<f64>() / ps.len() as f64);
                        if !(0..3).all(|d| c[d] > clo[d] && c[d] < chi[d]) { return 0.0 }
                        let mut nrm = [0.0; 3];
                        for i in 0..ps.len() { let (a, b) = (ps[i], ps[(i + 1) % ps.len()]); nrm[0] += (a[1] - b[1]) * (a[2] + b[2]); nrm[1] += (a[2] - b[2]) * (a[0] + b[0]); nrm[2] += (a[0] - b[0]) * (a[1] + b[1]); }
                        let area = 0.5 * dot(nrm, nrm).sqrt();
                        if area > 1e-12 && (nrm[2] * 0.5 / area).abs() > 0.7 { area } else { 0.0 }
                    }).sum();
                    if (0..3).all(|d| mlo[d] <= chi[d] && clo[d] <= mhi[d]) && horiz_area > 1e-3 {
                        let tb = std::time::Instant::now();
                        let nv0 = m.pos.len();
                        m = mesh_diff(&m, c);
                        if std::env::var_os("CUTLOG").is_some() { eprintln!("CUT {k}#{pi} nv {nv0} -> {} cutter nv {} {:?}", m.pos.len(), c.pos.len(), tb.elapsed()); }
                    }
                }
            }
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&format!("{k}#{pi}"), &gen.class, m, mi, pose_m.0);
        }
    }
    eprintln!("objects: {counts:?}  scene build {:?}", t_scene.elapsed());
    let tw = std::time::Instant::now();
    // people (no-op at --people 0): placements from the solved scene, posed avatar meshes added as nodes
    let people = indoorgen::people::place::place_ctx(&g.state, &g.room_polys, people_n, seed, &indoorgen::people::place::Ctx { visuals: Some(&visuals), floor_z: g.c.wall_thickness / 2.0, wall_t: g.c.wall_thickness, keepout: &door_zones });
    for (pi, pl) in people.iter().enumerate() {
        let mut a = pl.build();
        if a.parts.is_empty() && std::env::var_os("PEOPLE_PROXY").is_some() {
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
    // the clutter pass keeps off the people (their occupied boxes, padded 4 cm)
    let mut keepout: Vec<indoorgen::mesh::Mesh> = people.iter().flat_map(|p| p.keepout(0.04)).collect();
    // ... and off the swept area of the room doors
    for (c, h, yaw) in &door_zones {
        let mut m = Mesh::cuboid([-h[0], -h[1], -h[2]], [h[0], h[1], h[2]]);
        m.transform(&Mat4::translation(*c).mul(&Mat4::rot_z(*yaw)));
        keepout.push(m);
    }
    // mess pass, part 2: clutter (no-op at --mess 0)
    indoorgen::mess::clutter::apply_keepout(&mut scene, &g.state, &visuals, &g.room_polys, &mess, &keepout);
    if let Some(qp) = &qc_out {
        let cut_keys: Vec<String> = visuals.iter().filter(|(_, a)| !a.cutters.is_empty()).map(|(k, _)| k.clone()).collect();
        let label = format!("{}_{}_p{}_m{}", seed, args.get(2).map(|s| s.as_str()).unwrap_or("all"), people_n, mess_level);
        indoorgen::qc::run(&indoorgen::qc::QcIn { scene: &scene, state: &g.state, room_polys: &g.room_polys, cutters: &g.cutters, doors: &door_infos, wall_t: g.c.wall_thickness, cut_keys: &cut_keys }, std::path::Path::new(qp), &label);
    }
    if no_glb { return; }
    scene.write_glb(&out).unwrap();
    if cams_k > 0 { add_cameras(&scene, &g.room_polys, cams_k, seed, g.c.wall_thickness, &out); }
    if indoorgen::assets::prof_on() {
        eprintln!("write_glb {:?}", tw.elapsed());
        indoorgen::assets::prof_report();
    }
}

/// `--cams K`: photographer-like camera poses (crates/ig_cams) appended to the sidecar json as "cameras"
fn add_cameras(scene: &Scene, polys: &[(String, indoorgen::shp::G)], k: usize, seed: u64, wall_t: f64, glb: &std::path::Path) {
    let t = std::time::Instant::now();
    let cams = indoorgen::cams::generate(scene, polys, &indoorgen::cams::Cfg { k, seed, wall_t });
    let sp = glb.with_extension("json");
    if let Ok(txt) = std::fs::read_to_string(&sp) {
        let mut j: serde_json::Value = serde_json::from_str(&txt).unwrap();
        j["cameras"] = serde_json::Value::Array(cams.clone());
        std::fs::write(&sp, serde_json::to_vec_pretty(&j).unwrap()).unwrap();
    }
    eprintln!("CAMS {} cameras in {:?}", cams.len(), t.elapsed());
}

/// BSP boolean DIFFERENCE `a - b` on ig_core meshes (uv dropped: the exporter regenerates them). Only the faces of `a` near `b` go through
/// the boolean; the others are copied unchanged (keeps big counter tops cheap).
fn mesh_diff(a: &Mesh, b: &Mesh) -> Mesh {
    use indoorgen::gn::geom::Mesh as GM;
    let (blo, bhi) = b.bounds();
    let e = 1e-3;
    // the boolean needs the closed part: an open near-faces piece leaves an inverted lid (BSP) or patchy hole walls (exact)
    let whole = std::env::var_os("CUT_PIECE").is_none();
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
    let r = if whole { indoorgen::gn::csg::mesh_boolean("DIFFERENCE", &conv(&sub), &[&conv(b)]) } else { indoorgen::gn::csg::mesh_boolean_exact("DIFFERENCE", &conv(&sub), &[&conv(b)], false, true) };
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
    let ok = |f: &Vec<u32>| {
        let n = f.len() as f64;
        let c: V3 = [0, 1, 2].map(|d| f.iter().map(|&i| r.pos[i as usize][d] as f64).sum::<f64>() / n);
        let in_bounds = (0..3).all(|d| c[d] >= slo[d] - 1e-3 && c[d] <= shi[d] + 1e-3);
        let keep = in_bounds && !(inside_b(c) && dist_b(c) > 1e-3);
        if keep && std::env::var_os("CUTDBG").is_some() {
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
