// usage: gen2 <seed> [room] [out.glb] [--rust-cloth] [--mess LEVEL]
//   --mess LEVEL: lived-in disorder 0..1 added after the solve (crates/ig_mess); 0 (default) = unchanged output
//   --rust-cloth: also run the native cloth sim for bed cloth that is exported as a sidecar job (default: skip, Blender re-simulates it)
use infinigen::furn::assets::TableAssets;
use infinigen::mesh::*;
use infinigen::pipeline::*;

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // cloth that is exported as a sidecar job is re-simulated in Blender at render time: skip the Rust sim unless --rust-cloth
    if let Some(i) = args.iter().position(|a| a == "--rust-cloth") {
        args.remove(i);
    } else {
        infinigen::assets::cloth::SKIP_JOB_SIM.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mess_level: f64 = match args.iter().position(|a| a == "--mess") {
        Some(i) => {
            let v = args[i + 1].parse().expect("--mess LEVEL");
            args.drain(i..i + 2);
            v
        }
        None => 0.0,
    };
    let seed: u64 = args[1].parse().unwrap();
    let mess = infinigen::mess::MessCfg { level: mess_level, seed };
    let room = args.get(2).cloned();
    let out = std::path::PathBuf::from(args.get(3).cloned().unwrap_or("/tmp/gen2.glb".into()));
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = infinigen::assets::RealAssets { fallback };
    let t = std::time::Instant::now();
    let mut g = generate(&Options { seed, restrict_room: room, steps_large: std::env::var("STEPS_L").ok().and_then(|v| v.parse().ok()).unwrap_or(100), steps_medium: std::env::var("STEPS_M").ok().and_then(|v| v.parse().ok()).unwrap_or(40), steps_small: 5 }, &assets, &prog);
    eprintln!("total {:?}  stages {:?}  cpu {:.2}", t.elapsed(), g.timings, infinigen::assets::thread_cpu());
    if std::env::var("EVAL").is_ok() {
        let (prog, _) = infinigen::pipeline::load_program(seed, &prog);
        let mut memo = infinigen::furn::eval::Memo::new(&prog, None);
        let mut ev = infinigen::furn::eval::Evaluator::new(&prog, &g.state, &mut memo);
        let r = ev.evaluate_problem(true, true);
        eprintln!("FINAL loss {:.3} viol {:.1}", r.loss(), r.viol_count());
        eprintln!("terms {:?}", r.loss_vals.iter().filter(|x| x.1.abs() > 0.01).map(|x| (x.0.clone(), (x.1 * 100.0).round() / 100.0)).collect::<Vec<_>>());
        eprintln!("viol {:?}", r.violations.iter().filter(|x| x.1 > 0.0).collect::<Vec<_>>());
    }
    let t_scene = std::time::Instant::now();
    let mut scene = Scene::default();
    let obj = scene.material(Material::diffuse("object", [0.6, 0.35, 0.3], 0.7));
    let decos = infinigen::materials::decorate_rooms(seed, &g.shells.iter().map(|s| (s.name.clone(), infinigen::rooms::state::room_type(&s.name).to_string())).collect::<Vec<_>>());
    for (s, rm) in g.shells.iter().zip(decos) {
        use infinigen::materials::WallAlt;
        let (w, f, c) = (scene.material(rm.wall), scene.material(rm.floor), scene.material(rm.ceiling));
        let split = match rm.alt {
            WallAlt::None => None,
            WallAlt::Half { frac } => Some(infinigen::rooms::walldeco::apply_half(&s.wall, g.c.wall_height, frac)),
            WallAlt::Aligned => Some(infinigen::rooms::walldeco::apply_aligned(&s.wall)),
        };
        match (split, rm.alt_wall) {
            (Some((base, alt)), Some(am)) => {
                let ai = scene.material(am);
                scene.add(&format!("{}.wall", s.name), "room", base, w);
                scene.add(&format!("{}.wall.alt", s.name), "room", alt, ai);
            }
            _ => scene.add(&format!("{}.wall", s.name), "room", s.wall.clone(), w),
        }
        scene.add(&format!("{}.floor", s.name), "room", s.floor.clone(), f);
        scene.add(&format!("{}.ceiling", s.name), "room", s.ceiling.clone(), c);
    }
    let wt = g.c.wall_thickness;
    let polys: Vec<_> = g.room_polys.iter().map(|x| infinigen::shp::buffer_mitre(&x.1, -wt / 2.0)).collect();
    for (nm, ceil, z) in [("skirting_floor", false, wt / 2.0), ("skirting_ceiling", true, g.c.wall_height - wt / 2.0)] {
        let mut r = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), nm]));
        if let Some(sk) = infinigen::rooms::skirting::make(&polys, z, ceil, &mut r) {
            // skirting_board.py: shader_rough_plastic(base_color=hsv2rgba(white_hsv()), roughness=uniform(0.5, 1)) from the same FixedSeed stream
            // (thickness, height, white_hsv = (0, U(0, .1), U(.8, 1)), roughness)
            let mut pr = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), nm]));
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
    for p in infinigen::openings::make_openings(&g.c, &g.cutters, seed) {
        let m = scene.material(p.mat);
        scene.add(&p.name, &p.class, p.mesh, m);
    }
    // lights_off stage: chance 0.2 (python: stage runs when uniform() <= chance)
    let lights_off = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), "lights_off", "0"])).f64() <= 0.2;
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
    // mess pass, part 1: furniture states on the solved layout (no-op at --mess 0)
    infinigen::mess::states::apply(&mut g.state, &mess);
    // build the final assets of all generated objects in parallel (each build is independent and deterministic; results are consumed in
    // the original order below)
    let mut visuals: std::collections::HashMap<String, infinigen::assets::Asset> = {
        use rayon::prelude::*;
        let items: Vec<(&String, &infinigen::furn::state::GenRef)> = g.state.objs.iter().filter_map(|(k, o)| o.generator.as_ref().map(|gen| (k, gen))).collect();
        items
            .par_iter()
            .map(|(k, gen)| {
                infinigen::assets::set_unseeded(gen.unseeded.clone());
                let a = assets.visual(&gen.class, gen.factory_seed, gen.inst_seed);
                infinigen::assets::set_unseeded(None);
                ((*k).clone(), a)
            })
            .collect()
    };
    // mess pass, part 1b: states that edit the built assets (bed cloth, cabinet doors / drawers; no-op at --mess 0)
    infinigen::mess::states::post_visuals(&mut visuals, &g.state, &mess);
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
                    if infinigen::assets::inst_rng(gen.factory_seed, gen.inst_seed).f64() < 0.6 {
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
                let mut r = infinigen::rng::Rng::seeded(gen.factory_seed);
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
                scene.lights.push(infinigen::mesh::Light { loc: o.pose.apply(l), watt, radius, temperature: temp, name: k.clone() });
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
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&format!("{k}#{pi}"), &gen.class, m, mi, pose_m.0);
        }
    }
    eprintln!("objects: {counts:?}  scene build {:?}", t_scene.elapsed());
    let tw = std::time::Instant::now();
    // mess pass, part 2: clutter (no-op at --mess 0)
    infinigen::mess::clutter::apply(&mut scene, &g.state, &visuals, &mess);
    scene.write_glb(&out).unwrap();
    if infinigen::assets::prof_on() {
        eprintln!("write_glb {:?}", tw.elapsed());
        infinigen::assets::prof_report();
    }
}
