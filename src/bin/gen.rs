use indoorgen::int_hash;
use indoorgen::mesh::*;
use indoorgen::rng::Rng;
use indoorgen::rooms::consts::RoomConstants;
use indoorgen::rooms::plan::{FloorPlanSolver, PlanConfig};
use indoorgen::rooms::{shell, solidify};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args[1].parse().unwrap();
    let out = std::path::PathBuf::from(&args[2]);
    let mut rng0 = Rng::seeded(seed);
    for _ in 0..42 { rng0.next_u32(); }
    let c = RoomConstants::sample(&mut rng0, 1);
    let cfg = PlanConfig { n_divide_trials: 25, iters_mult: 25, rooms: indoorgen::rooms::graph::RoomSet::Fewer };
    let solver = FloorPlanSolver::new(seed, &c, cfg);
    let mut a = Rng::seeded(int_hash(&[&seed.to_string(), "solve_rooms"]));
    let st = solver.solve(&mut a);
    let sol = solidify::Solidifier { c: &c, graph: &solver.graphs[0], level: 0, enable_open: false, passage_prob: 0.0 };
    let res = sol.solidify(&mut Rng::seeded(seed), &st);
    let shells = shell::build_shells(&c, &st, &res.cutters);
    let mut scene = Scene::default();
    let wall = scene.material(Material::diffuse("wall", [0.8, 0.78, 0.72], 0.9));
    let floor = scene.material(Material::diffuse("floor", [0.45, 0.32, 0.2], 0.6));
    let ceil = scene.material(Material::diffuse("ceiling", [0.9, 0.9, 0.9], 0.9));
    for s in shells {
        scene.add(&format!("{}.wall", s.name), "room", s.wall, wall);
        scene.add(&format!("{}.floor", s.name), "room", s.floor, floor);
        scene.add(&format!("{}.ceiling", s.name), "room", s.ceiling, ceil);
    }
    scene.write_glb(&out).unwrap();
}
