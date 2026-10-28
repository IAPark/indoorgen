use indoorgen::int_hash;
use indoorgen::rng::Rng;
use indoorgen::rooms::consts::RoomConstants;
use indoorgen::rooms::plan::{FloorPlanSolver, PlanConfig};
use indoorgen::shp::*;
use geos::Geom;
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args[1].parse().unwrap();
    let skip: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(42);
    let mut rng0 = Rng::seeded(seed);
    for _ in 0..skip {
        rng0.next_u32();
    }
    let c = RoomConstants::sample(&mut rng0, 1);
    println!("{:?}", c);
    let t0 = std::time::Instant::now();
    let cfg = PlanConfig { n_divide_trials: 25, iters_mult: 25, fewer_rooms: true };
    let solver = FloorPlanSolver::new(seed, &c, cfg);
    println!("graphs {:?}\nwh {:?} {:?}", solver.graphs, solver.widths, solver.heights);
    let mut a = Rng::seeded(int_hash(&[&seed.to_string(), "solve_rooms"]));
    let st = solver.solve(&mut a);
    println!("solve {:?}", t0.elapsed());
    let mut objs = serde_json::Map::new();
    for (k, o) in &st.objs {
        objs.insert(k.clone(), json!({"polygon": o.polygon.as_ref().map(|p| p.to_wkt().unwrap())}));
    }
    if std::env::var("DBG").is_ok() {
        let ext = st.objs.keys().find(|k| k.starts_with("exterior")).unwrap().clone();
        for r in &st.objs[&ext].relations {
            eprintln!("EXTREL {} {}", r.target, r.value.to_wkt().unwrap());
        }
    }
    let sol = indoorgen::rooms::solidify::Solidifier { c: &c, graph: &solver.graphs[0], level: 0, enable_open: false, passage_prob: 0.0 };
    let mut rc = Rng::seeded(seed);
    let res = sol.solidify(&mut rc, &st);
    let cut: Vec<_> = res.cutters.iter().map(|c| json!({"kind": format!("{:?}", c.kind), "loc": c.loc, "rot": c.rot_z, "dim": c.dims, "rooms": c.rooms})).collect();
    println!("{}", serde_json::to_string_pretty(&json!({"objs": objs, "cutters": cut})).unwrap());
    let _ = bbox;
}
