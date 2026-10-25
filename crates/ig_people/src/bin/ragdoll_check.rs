//! ragdoll_check KIND [AVATAR_IDX] [SEED] [out.json]: settle one person on a synthetic bed / sofa / floor and dump key + settled vertices.
use ig_people::ragdoll::*;
use ig_people::*;
use ig_core::mesh::Mat4;

fn boxtris(lo: [f64; 3], hi: [f64; 3]) -> Vec<[[f64; 3]; 3]> {
    let m = ig_core::mesh::Mesh::cuboid(lo, hi);
    m.triangles().iter().map(|t| [m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]]).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let kind = match a.get(1).map(|s| s.as_str()).unwrap_or("LieBed") {
        "LieBed" => PoseKind::LieBed,
        "LieSofa" => PoseKind::LieSofa,
        "SitRelaxed" => PoseKind::SitRelaxed,
        "SitBedEdge" => PoseKind::SitBedEdge,
        "Stand" => PoseKind::Stand,
        "StandAtDesk" => PoseKind::StandAtDesk,
        o => panic!("kind {o}"),
    };
    let ai: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let seed: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(7);
    let out = a.get(4).cloned().unwrap_or("/tmp/rag.json".into());
    let av = body::avatar_of(ai).expect("avatar");
    let model = ragdoll::model(&av);
    let (seat_z, tops): (Option<f64>, f64) = match kind {
        PoseKind::SitRelaxed => (Some(0.42 - 0.04), 0.42),
        PoseKind::SitBedEdge => (Some(0.52 - 0.04), 0.52),
        _ => (None, 0.55),
    };
    let (posed, g) = body::pose_avatar_seat_g(&av, kind, seed, seat_z);
    let info = &posed.info;
    let seat = info.seat.unwrap_or([0.0; 3]);
    let mut statics = vec![];
    let z0;
    let mut boxes: Vec<([f64; 3], [f64; 3])> = vec![];
    match kind {
        PoseKind::LieBed | PoseKind::LieSofa => {
            // bed top at tops (sunk by 0.04 for the collider); body: lowest point on z=0 -> origin z so that seat is at the top - 0.02
            z0 = tops - seat[2] - 0.02;
            boxes.push(([-1.0, -1.2, 0.0], [1.0, 1.2, tops]));
        }
        PoseKind::Stand | PoseKind::StandAtDesk => {
            z0 = 0.0;
        }
        _ => {
            z0 = 0.0;
            let by = info.lo[1] - 0.03;
            boxes.push(([-0.6, by - 0.05, 0.0], [0.6, 0.38, tops]));
            if kind == PoseKind::SitRelaxed {
                boxes.push(([-0.6, by - 0.25, 0.0], [0.6, by, 0.95]));
            }
        }
    }
    for (lo, hi) in &boxes {
        statics.push(StaticTris { tris: boxtris(*lo, *hi), dz: 0.04 });
    }
    let pose = Mat4([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., z0], [0., 0., 0., 1.]]);
    let t = std::time::Instant::now();
    let s = settle(&av, &model, &g, &pose, &statics, 0.0, 0.03).expect("settle");
    println!("{kind:?} av{ai} seed{seed}: bodies {} steps {} moved {:.3} pen {:.4} total {:.0} ms (settle {:.0} ms)", s.bodies, s.steps, s.moved, s.pen, t.elapsed().as_secs_f64() * 1000.0, s.ms);
    let sk = body::skin_parts(&av, &g);
    let ss = body::skin_parts(&av, &s.g);
    let pts = |p: &Vec<(Vec<[f32; 3]>, Vec<[f32; 3]>)>| -> Vec<[f32; 3]> {
        let mut o = vec![];
        for (pi, part) in p.iter().enumerate() {
            if av.parts[pi].kind == ig_people::avatar::PartKind::Hair { continue; }
            o.extend(part.0.iter().step_by(3).map(|q| [q[0], q[1], q[2] + z0 as f32]));
        }
        o
    };
    let j = serde_json::json!({"key": pts(&sk), "settled": pts(&ss), "boxes": boxes.iter().map(|(l, h)| serde_json::json!([l, [h[0], h[1], h[2] - 0.04]])).collect::<Vec<_>>()});
    std::fs::write(out, j.to_string()).unwrap();
}
