// usage: gen2 <seed> [room] [out.glb] [--only-target] [--rust-cloth]
//   default: every room of the house is furnished; `room` is the target room (cameras favour it). --only-target / ONLY_TARGET=1: furnish only the first room of that type
// usage: gen2 <seed> [room] [out.glb] [--rust-cloth] [--mess LEVEL] [--cams K]
//   --rules FILE.json (furniture rules per room type), --print-default-rules, --rules-info
//   --config FILE.json (partial GenConfig; env vars and flags win), --dump-config, --schema, --set section.key=JSON (see src/config.rs)
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

/// take `--flag VALUE` out of the argument list
fn take_val(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    let v = args.get(i + 1).unwrap_or_else(|| panic!("{flag} needs a value")).clone();
    args.drain(i..i + 2);
    Some(v)
}
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    args.iter().position(|a| a == flag).map(|i| { args.remove(i); }).is_some()
}

fn main() {
    load_local_env();
    use indoorgen::config::GenConfig;
    let mut args: Vec<String> = std::env::args().collect();
    if take_flag(&mut args, "--schema") {
        println!("{}", serde_json::to_string_pretty(&GenConfig::schema()).unwrap());
        return;
    }
    // precedence: defaults < --config FILE < environment variables < command-line flags
    let mut cfg = GenConfig::default();
    if let Some(f) = take_val(&mut args, "--config") {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("--config {f}: {e}"))).unwrap_or_else(|e| panic!("--config {f}: {e}"));
        cfg = GenConfig::from_partial(&cfg, &j).unwrap_or_else(|e| panic!("--config {f}: {e}"));
    }
    cfg.apply_env();
    if take_flag(&mut args, "--print-default-rules") || args.iter().any(|a| a == "--rules-info") {
        // the built-in rules of the dense program as a RuleSpec (edit it and pass it back with --rules / config `rules`); --rules-info: + vocabulary
        let p: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/data/home_program_dense.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let v = if take_flag(&mut args, "--rules-info") { indoorgen::rules::rules_info(&p) } else { serde_json::to_value(indoorgen::rules::default_rules(&p)).unwrap() };
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
        return;
    }
    // --rules FILE.json: shortcut for the config's `rules` field (furniture rules per room type, see src/rules.rs)
    if let Some(f) = take_val(&mut args, "--rules") {
        let text = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("--rules {f}: {e}"));
        cfg.rules = Some(indoorgen::rules::parse(&text).unwrap_or_else(|e| panic!("--rules {f}: {e}")));
    }
    if take_flag(&mut args, "--rust-cloth") { cfg.cloth.on = true; }
    if let Some(v) = take_val(&mut args, "--mess") { cfg.mess.level = v.parse().expect("--mess LEVEL"); }
    if let Some(v) = take_val(&mut args, "--people") { cfg.people.count = v.parse().expect("--people N"); }
    if let Some(v) = take_val(&mut args, "--cams") { cfg.cameras.count = v.parse().expect("--cams K"); }
    if take_flag(&mut args, "--only-target") { cfg.house.only_target = true; }
    if let Some(v) = take_val(&mut args, "--qc") { cfg.output.qc_path = Some(v); }
    if take_flag(&mut args, "--no-glb") { cfg.output.no_glb = true; }
    let dump = take_flag(&mut args, "--dump-config");
    // --set section.key=JSON (repeatable): any config field, e.g. --set solver.steps_scale=0.5 --set post.snap=false
    while let Some(kv) = take_val(&mut args, "--set") {
        let (k, v) = kv.split_once('=').expect("--set section.key=VALUE");
        let val: serde_json::Value = serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.to_string()));
        let mut over = val;
        for part in k.rsplit('.') { over = serde_json::json!({ part: over }); }
        cfg = GenConfig::from_partial(&cfg, &over).unwrap_or_else(|e| panic!("--set {kv}: {e}"));
    }
    // positional: <seed> [room] [out.glb]
    if let Some(s) = args.get(1) { cfg.house.seed = s.parse().expect("seed"); }
    if let Some(r) = args.get(2) { cfg.house.room = Some(r.clone()); }
    if dump {
        println!("{}", serde_json::to_string_pretty(&cfg).unwrap());
        return;
    }
    if cfg.people.count > 0 && indoorgen::people::avatar_count() == 0 {
        eprintln!("WARNING: people.count {} requested but no avatars found in {:?}: run tools/people/fetch_rocketbox.sh or set INDOORGEN_ASSETS (e.g. in .indoorgen.env); no people will be placed", cfg.people.count, indoorgen::people::avatar::root_dir());
    }
    let (seed, room, cams_k) = (cfg.house.seed, cfg.house.room.clone(), cfg.cameras.count);
    let out = std::path::PathBuf::from(args.get(3).cloned().unwrap_or("/tmp/gen2.glb".into()));
    let prog_file = if cfg.program.dense { "home_program_dense.json" } else { "home_program.json" };
    let prog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), prog_file)).unwrap()).unwrap();
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    if let Some(spec) = &cfg.rules {
        if let Err(errs) = indoorgen::rules::try_apply(&mut prog.clone(), spec) {
            for e in errs { eprintln!("rules: {e}"); }
            std::process::exit(2);
        }
    }
    let params = indoorgen::scenegen::Params { cfg, prog, fallback };
    let built = indoorgen::scenegen::build_scene(&params);
    let indoorgen::scenegen::Built { scene, room_polys, wall_t, t_scene, done, .. } = &built;
    let (wall_t, t_scene, done) = (*wall_t, *t_scene, *done);
    if !done { return; }
    let lap = |what: &str| { if std::env::var_os("LAPS").is_some() { eprintln!("LAP {what} {:.1}s since scene start", t_scene.elapsed().as_secs_f64()); } };
    let tw = std::time::Instant::now();
    scene.write_glb(&out).unwrap();
    lap("glb written");
    if cams_k > 0 { add_cameras(&room, scene, room_polys, cams_k, seed, wall_t, &out); }
    {
        // sidecar `objects` (always) and `variation` record (only when some factor is not default)
        let sp = out.with_extension("json");
        if let Ok(txt) = std::fs::read_to_string(&sp) {
            let mut j: serde_json::Value = serde_json::from_str(&txt).unwrap();
            built.merge_sidecar(&mut j);
            std::fs::write(&sp, serde_json::to_vec_pretty(&j).unwrap()).unwrap();
        }
    }
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
