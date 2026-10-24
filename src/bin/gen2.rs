use infinigen::furn::assets::TableAssets;
use infinigen::mesh::*;
use infinigen::pipeline::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args[1].parse().unwrap();
    let room = args.get(2).cloned();
    let out = std::path::PathBuf::from(args.get(3).cloned().unwrap_or("/tmp/gen2.glb".into()));
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program.json")).unwrap()).unwrap();
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = infinigen::assets::RealAssets { fallback };
    let t = std::time::Instant::now();
    let g = generate(&Options { seed, restrict_room: room, steps_large: std::env::var("STEPS_L").ok().and_then(|v| v.parse().ok()).unwrap_or(100), steps_medium: std::env::var("STEPS_M").ok().and_then(|v| v.parse().ok()).unwrap_or(40), steps_small: 5 }, &assets, &prog);
    eprintln!("total {:?}  stages {:?}", t.elapsed(), g.timings);
    if std::env::var("EVAL").is_ok() {
        let (prog, _) = infinigen::pipeline::load_program(seed, &prog);
        let mut memo = infinigen::furn::eval::Memo::new(&prog, None);
        let mut ev = infinigen::furn::eval::Evaluator::new(&prog, &g.state, &mut memo);
        let r = ev.evaluate_problem(true, true);
        eprintln!("FINAL loss {:.3} viol {:.1}", r.loss(), r.viol_count());
        eprintln!("terms {:?}", r.loss_vals.iter().filter(|x| x.1.abs() > 0.01).map(|x| (x.0.clone(), (x.1 * 100.0).round() / 100.0)).collect::<Vec<_>>());
        eprintln!("viol {:?}", r.violations.iter().filter(|x| x.1 > 0.0).collect::<Vec<_>>());
    }
    let mut scene = Scene::default();
    let obj = scene.material(Material::diffuse("object", [0.6, 0.35, 0.3], 0.7));
    let mut mrng = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), "room_materials"]));
    for s in &g.shells {
        let rm = infinigen::materials::room_materials(&mut mrng, &s.name, infinigen::rooms::state::room_type(&s.name));
        let (w, f, c) = (scene.material(rm.wall), scene.material(rm.floor), scene.material(rm.ceiling));
        scene.add(&format!("{}.wall", s.name), "room", s.wall.clone(), w);
        scene.add(&format!("{}.floor", s.name), "room", s.floor.clone(), f);
        scene.add(&format!("{}.ceiling", s.name), "room", s.ceiling.clone(), c);
    }
    let wt = g.c.wall_thickness;
    let polys: Vec<_> = g.room_polys.iter().map(|x| infinigen::shp::buffer_mitre(&x.1, -wt / 2.0)).collect();
    for (nm, ceil, z) in [("skirting_floor", false, wt / 2.0), ("skirting_ceiling", true, g.c.wall_height - wt / 2.0)] {
        let mut r = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), nm]));
        if let Some(sk) = infinigen::rooms::skirting::make(&polys, z, ceil, &mut r) {
            let mi = scene.material(Material::diffuse(nm, [0.85, 0.85, 0.85], sk.roughness));
            scene.add(nm, "skirting", sk.mesh, mi);
        }
    }
    for p in infinigen::openings::make_openings(&g.c, &g.cutters, seed) {
        let m = scene.material(p.mat);
        scene.add(&p.name, &p.class, p.mesh, m);
    }
    // lights_off stage: chance 0.2 (python: stage runs when uniform() <= chance)
    let lights_off = infinigen::rng::Rng::seeded(infinigen::int_hash(&[&seed.to_string(), "lights_off", "0"])).f64() <= 0.2;
    if std::env::var("POSES").is_ok() {
        for (k, o) in &g.state.objs {
            if let Some(gen) = &o.generator {
                if matches!(gen.class.as_str(), "SofaFactory" | "TVStandFactory" | "CoffeeTableFactory" | "ArmChairFactory") {
                    eprintln!("POSE {k} t=({:.2},{:.2}) front=({:.2},{:.2}) lo={:?} hi={:?}", o.pose.t[0], o.pose.t[1], o.pose.r[0][0], o.pose.r[1][0], o.geom.lo.map(|v| (v * 100.0).round() / 100.0), o.geom.hi.map(|v| (v * 100.0).round() / 100.0));
                }
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
                    let (lo, hi) = (o.geom.lo, o.geom.hi);
                    Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, hi[2] + 0.04])
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
                scene.lights.push(infinigen::mesh::Light { loc: o.pose.apply(l), watt, radius, temperature: temp, name: k.clone() });
            }
        }
        let asset = assets.visual(&gen.class, gen.factory_seed, gen.inst_seed);
        let _ = obj;
        for (pi, part) in asset.parts.iter().enumerate() {
            let mut m = part.mesh.clone();
            m.transform(&Mat4([
                [o.pose.r[0][0], o.pose.r[0][1], o.pose.r[0][2], o.pose.t[0]],
                [o.pose.r[1][0], o.pose.r[1][1], o.pose.r[1][2], o.pose.t[1]],
                [o.pose.r[2][0], o.pose.r[2][1], o.pose.r[2][2], o.pose.t[2]],
                [0.0, 0.0, 0.0, 1.0],
            ]));
            let mi = scene.material(part.mat.clone());
            scene.add(&format!("{k}#{pi}"), &gen.class, m, mi);
        }
    }
    eprintln!("objects: {counts:?}");
    scene.write_glb(&out).unwrap();
}
