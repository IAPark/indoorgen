// usage: gen2 <seed> [room] [out.glb] [--only-target] [--rust-cloth]
//   default: every room of the house is furnished; `room` is the target room (cameras favour it). --only-target / ONLY_TARGET=1: furnish only the first room of that type
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
    // default: furnish every room of the house; the room argument names the target room (floor plan seed is `seed`; cameras favour it).
    // --only-target (or ONLY_TARGET=1): furnish just the first room of that type, as before (lockstep / selfcheck use this)
    let only_target = args.iter().position(|a| a == "--only-target").map(|i| { args.remove(i); }).is_some() || std::env::var_os("ONLY_TARGET").is_some();
    let qc_out: Option<String> = args.iter().position(|a| a == "--qc").map(|i| { let v = args[i + 1].clone(); args.drain(i..i + 2); v });
    let no_glb = args.iter().position(|a| a == "--no-glb").map(|i| { args.remove(i); }).is_some();
    if people_n > 0 && indoorgen::people::avatar_count() == 0 {
        eprintln!("WARNING: --people {people_n} requested but no avatars found in {:?}: run tools/people/fetch_rocketbox.sh or set INDOORGEN_ASSETS (e.g. in .indoorgen.env); no people will be placed", indoorgen::people::avatar::root_dir());
    }
    let seed: u64 = args[1].parse().unwrap();
    let room = args.get(2).cloned();
    let out = std::path::PathBuf::from(args.get(3).cloned().unwrap_or("/tmp/gen2.glb".into()));
    // default: the dense-furnishing program + Infinigen's own solver steps 300/200/50 (realistic, lived-in rooms).
    // INFINIGEN_PROGRAM=1: the program exactly as exported from python home.py + the fast_solve.gin steps 100/40/5 (selfcheck/lockstep semantics).
    let orig_prog = std::env::var("INFINIGEN_PROGRAM").map_or(false, |v| v == "1");
    let prog_file = if orig_prog { "home_program.json" } else { "home_program_dense.json" };
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), prog_file)).unwrap()).unwrap();
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let params = indoorgen::scenegen::Params { seed, room: room.clone(), only_target, mess_level, people_n, qc_out, no_glb, orig_prog, prog, fallback };
    let indoorgen::scenegen::Built { scene, room_polys, wall_t, t_scene, done } = indoorgen::scenegen::build_scene(&params);
    if !done { return; }
    let lap = |what: &str| { if std::env::var_os("LAPS").is_some() { eprintln!("LAP {what} {:.1}s since scene start", t_scene.elapsed().as_secs_f64()); } };
    let tw = std::time::Instant::now();
    scene.write_glb(&out).unwrap();
    lap("glb written");
    if cams_k > 0 { add_cameras(&room, &scene, &room_polys, cams_k, seed, wall_t, &out); }
    if indoorgen::assets::prof_on() {
        eprintln!("write_glb {:?}", tw.elapsed());
        indoorgen::assets::prof_report();
    }
}

/// `--cams K`: photographer-like camera poses (crates/ig_cams) appended to the sidecar json as "cameras"
fn add_cameras(target: &Option<String>, scene: &Scene, polys: &[(String, indoorgen::shp::G)], k: usize, seed: u64, wall_t: f64, glb: &std::path::Path) {
    let t = std::time::Instant::now();
    let cams = indoorgen::cams::generate(scene, polys, &indoorgen::cams::Cfg { k, seed, wall_t, target: target.clone() });
    let sp = glb.with_extension("json");
    if let Ok(txt) = std::fs::read_to_string(&sp) {
        let mut j: serde_json::Value = serde_json::from_str(&txt).unwrap();
        j["cameras"] = serde_json::Value::Array(cams.clone());
        std::fs::write(&sp, serde_json::to_vec_pretty(&j).unwrap()).unwrap();
    }
    eprintln!("CAMS {} cameras in {:?}", cams.len(), t.elapsed());
}
