use indoorgen::furn::eval::*;
use indoorgen::furn::geom::*;
use indoorgen::furn::relations::*;
use indoorgen::furn::state::*;
use indoorgen::furn::tags::*;
use indoorgen::pipeline::*;
use serde_json::Value;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args[1].parse().unwrap();
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
    let prog_json: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/home_program.json")).unwrap()).unwrap();
    let (prog, _usage) = load_program(seed, &prog_json);
    // rooms/cutters from our own deterministic generation (identical to the reference for this seed)
    let g = generate_rooms_only(seed);
    let mut state = g;
    for (k, o) in dump["objs"].as_object().unwrap() {
        if state.objs.contains_key(k) {
            continue;
        }
        let Some(gen) = o["gen"].as_str() else { continue };
        let bb = o["bb"].as_array().unwrap();
        let lo: Vec<f64> = bb[0].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let hi: Vec<f64> = bb[6].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let geom = ObjGeom::cuboid([lo[0], lo[1], lo[2]], [hi[0], hi[1], hi[2]], 0);
        let m: Vec<Vec<f64>> = o["matrix"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()).collect();
        let pose = Pose::from_matrix([[m[0][0], m[0][1], m[0][2], m[0][3]], [m[1][0], m[1][1], m[1][2], m[1][3]], [m[2][0], m[2][1], m[2][2], m[2][3]], [0.0, 0.0, 0.0, 1.0]]);
        let mut tags = TagSet::new();
        for t in o["tags"].as_array().unwrap() {
            let s = t.as_str().unwrap();
            let (neg, s) = if let Some(r) = s.strip_prefix('-') { (true, r) } else { (false, s) };
            let tag = if let Some(c) = s.strip_prefix("gen:") { Tag::gen(c) } else if let Some(c) = s.strip_prefix("obj:") { Tag::obj(c) } else { Tag::sem(s) };
            tags.insert(if neg { tag.negated() } else { tag });
        }
        let mut os = ObjState::new(Arc::new(geom), pose, tags);
        os.generator = Some(GenRef { class: gen.to_string(), factory_seed: 0, inst_seed: 0, unseeded: None });
        for r in o["relations"].as_array().unwrap() {
            let kind = r["rel"].as_str().unwrap();
            let gk = match kind { "StableAgainst" => GeoKind::StableAgainst, "CoPlanar" => GeoKind::CoPlanar, "SupportedBy" => GeoKind::SupportedBy, "Touching" => GeoKind::Touching, _ => continue };
            let ts = |k: &str| TagSet::from(r[k].as_array().unwrap().iter().map(|t| { let s = t.as_str().unwrap(); let (n, s) = if let Some(x) = s.strip_prefix('-') { (true, x) } else { (false, s) }; let tg = Tag::sub(s); if n { tg.negated() } else { tg } }));
            os.relations.push(RelState { relation: Relation::Geometry(Geo { kind: gk, child: ts("child_tags"), parent: ts("parent_tags"), margin: r["margin"].as_f64().unwrap_or(0.0), check_z: r["check_z"].as_bool().unwrap_or(true), rev_normal: r["rev_normal"].as_bool().unwrap_or(false) }), target: r["target"].as_str().unwrap().to_string(), child_plane: r["cp"].as_u64().unwrap_or(0) as usize, parent_plane: r["pp"].as_u64().unwrap_or(0) as usize });
        }
        os.active = true;
        state.objs.insert(k.clone(), os);
        state.touch(k);
    }
    let mut memo = Memo::new(&prog, None);
    let mut ev = Evaluator::new(&prog, &state, &mut memo);
    let r = ev.evaluate_problem(true, true);
    if std::env::var("VALID").is_ok() {
        for (k, o) in state.objs.iter() {
            if o.generator.is_some() && !o.relations.is_empty() {
                let rel = indoorgen::furn::dof::all_relations_valid(&state, k);
                let post = indoorgen::furn::dof::check_post_move_validity(&state, k);
                if !rel || !post {
                    println!("INVALID {k} relations_ok={rel} post_ok={post}");
                }
            }
        }
    }
    println!("{:30} {:>14} {:>14}", "term", "python", "rust");
    for (k, v) in &r.loss_vals {
        let py = dump["loss"].get(k).and_then(|x| x.as_f64());
        println!("{:30} {:>14.5} {:>14.5}", k, py.unwrap_or(f64::NAN), v);
    }
    println!("-- violations");
    for (k, v) in &r.violations {
        let py = dump["viol"].get(k).and_then(|x| x.as_f64());
        println!("{:30} {:>14.5} {:>14.5}", k, py.unwrap_or(f64::NAN), v);
    }
    println!("total loss rust {:.4} py {:.4}", r.loss(), dump["loss"].as_object().unwrap().values().map(|x| x.as_f64().unwrap()).sum::<f64>());
}
