//! Lay avatars out in several poses on a floor (visual check).  people_showcase OUT.glb [--poses Stand,SitUpright,..] [--avatars 0,1,..] [--seed N] [--rowlen N]
use ig_core::mesh::{Material, Mesh, Scene};
use ig_people::*;
use ig_people::body::{avatar_name};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&a[1]);
    let arg = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
    let n = avatar_count();
    eprintln!("avatars {n}");
    let avs: Vec<usize> = match arg("--avatars") { Some(s) => s.split(',').map(|x| x.parse().unwrap()).collect(), None => (0..n).collect() };
    let poses: Vec<PoseKind> = match arg("--poses") {
        Some(s) => s.split(',').map(|x| *ALL_POSES.iter().find(|p| format!("{p:?}") == x).unwrap_or_else(|| panic!("pose {x}"))).collect(),
        None => ALL_POSES.to_vec(),
    };
    let seed: u64 = arg("--seed").map(|s| s.parse().unwrap()).unwrap_or(1);
    let yaw: f64 = arg("--yaw").map(|s| s.parse().unwrap()).unwrap_or(0.0);
    let mut sc = Scene::default();
    let floor = sc.material(Material::diffuse("floor", [0.55, 0.5, 0.45], 0.7));
    let prop = sc.material(Material::diffuse("prop", [0.35, 0.3, 0.3], 0.6));
    let (dx, dy) = (1.6, 2.4);
    sc.add("floor", "Floor", Mesh::cuboid([-1.0, -1.5 - dy * (if a.iter().any(|x| x == "--byavatar") { avs.len() } else { poses.len() }) as f64, -0.05], [dx * (if a.iter().any(|x| x == "--byavatar") { poses.len() } else { avs.len() }) as f64 + 0.5, 1.5, 0.0]), floor);
    let mut tb = 0.0;
    let mut cnt = 0;
    let byav = a.iter().any(|x| x == "--byavatar");
    for (r0, &k) in poses.iter().enumerate() {
        for (c0, &av) in avs.iter().enumerate() {
            let (r, c) = if byav { (c0, r0) } else { (r0, c0) };
            let t = Instant::now();
            let info = pose_info(av, k);
            let asset = build_person(av, k, seed + c0 as u64 + r0 as u64 * 7);
            tb += t.elapsed().as_secs_f64();
            cnt += 1;
            let (x, y) = (c as f64 * dx, -(r as f64) * dy);
            use PoseKind::*;
            let pose = ig_core::mesh::Mat4::translation([x, y, 0.0]).mul(&ig_core::mesh::Mat4::rot_z(yaw.to_radians()));
            let mut props: Vec<([f64; 3], [f64; 3], usize)> = vec![];
            if let Some(s) = info.seat {
                match k {
                    LieBed | LieSofa => props.push(([-0.7, -1.2, -0.3], [0.7, 1.2, 0.0], prop)),
                    _ => props.push(([-0.3, -0.32, 0.0], [0.3, 0.12, s[2]], prop)),
                }
            }
            if let Some(h) = info.hands {
                let top = (h[0][2] + h[1][2]) * 0.5 - 0.015;
                let yf = h[0][1].max(h[1][1]) + 0.12;
                props.push(([-0.6, yf, 0.0], [0.6, yf + 0.6, top], prop));
            }
            for (lo, hi, m) in props {
                let mut cm = Mesh::cuboid(lo, hi);
                cm.transform(&pose);
                sc.add("prop", "Prop", cm, m);
            }
            for (i, p) in asset.parts.iter().enumerate() {
                let mut m = p.mesh.clone();
                m.transform(&pose);
                let mi = sc.material(p.mat.clone());
                sc.add_posed(&format!("person{r}_{c}_{k:?}#{i}"), "Person", m, mi, pose.0);
            }
            if r == 0 && c < 3 { eprintln!("{:?} {:?}: seat {:?} feet {:?} hands {:?} lo {:.2?} hi {:.2?}", avatar_name(av), k, info.seat, info.feet, info.hands, info.lo, info.hi); }
        }
    }
    eprintln!("build+info per person: {:.3}s ({} people)", tb / cnt as f64, cnt);
    sc.write_glb(&out).unwrap();
}
