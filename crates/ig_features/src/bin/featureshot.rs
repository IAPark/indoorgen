//! featureshot OUT.glb [seed0]: builds each built-in feature on synthetic wall spans and writes a glb (one mini room per case, laid out in a grid)
use ig_core::mesh::{Material, Mat4, Mesh, Scene};
use ig_features::*;

fn bxm(m: &mut Mesh, lo: [f64; 3], hi: [f64; 3]) {
    m.append(&Mesh::cuboid(lo, hi));
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&a[1]);
    let s0: u64 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(1);
    let mut scene = Scene::default();
    let wall = scene.material(Material::diffuse("wall", [0.86, 0.85, 0.82], 0.9));
    let floor = scene.material(Material::diffuse("floor", [0.55, 0.42, 0.3], 0.7));
    let glass = scene.material(Material::diffuse("window", [0.8, 0.9, 1.0], 0.1));
    // FIREPLACE=N: N fireplaces on plain 3.6 m spans (one per seed) instead of the built-in cases
    let fp_n: Option<usize> = std::env::var("FIREPLACE").ok().and_then(|v| v.parse().ok());
    let feats: Vec<Box<dyn Feature>> = if fp_n.is_some() { vec![Box::new(fireplace::FireplaceFeature)] } else { builtin_features() };
    // (feature index, length, height, openings)
    let op = |w: bool, x0: f64, x1: f64, z0: f64, z1: f64| Opening { is_window: w, x: [x0, x1], z: [z0, z1] };
    let fp_cases: Vec<(usize, f64, f64, Vec<Opening>)> = (0..fp_n.unwrap_or(0)).map(|i| (0, 3.6, if i % 4 == 3 { 3.0 } else { 2.7 }, vec![])).collect();
    let cases: Vec<(usize, f64, f64, Vec<Opening>)> = if fp_n.is_some() { fp_cases } else { vec![
        (0, 2.0, 2.7, vec![]),
        (0, 3.2, 2.7, vec![op(false, 2.5, 3.2, 0.0, 2.05)]),
        (0, 4.5, 2.8, vec![op(true, 1.6, 2.8, 0.9, 1.9)]),
        (0, 1.4, 2.4, vec![]),
        (0, 3.6, 3.0, vec![op(true, 1.4, 2.2, 1.0, 2.0)]),
        (1, 2.6, 2.7, vec![op(true, 0.9, 1.9, 0.8, 1.9)]),
        (1, 3.8, 2.7, vec![op(true, 1.4, 2.6, 0.7, 1.8)]),
        (1, 2.2, 2.6, vec![op(true, 0.6, 1.6, 0.95, 2.0)]),
    ] };
    let t = 0.2;
    let mut n = 0;
    for (ci, (fi, len, h, ops)) in cases.iter().enumerate() {
        let span = WallSpan { length: *len, height: *h, wall_thickness: t, openings: ops.clone(), room_type: "LivingRoom".into() };
        let f = &feats[*fi];
        let ivs = f.fits(&span);
        eprintln!("case {ci} {} len {len} h {h}: fits {:?}", f.class(), ivs);
        let Some(iv) = ivs.first().copied() else { continue };
        let ox = (ci % 3) as f64 * 6.0;
        let oy = (ci / 3) as f64 * 9.0;
        // room shell: wall with holes, floor, ceiling-less
        let mut wm = Mesh::new();
        let mut gm = Mesh::new();
        let mut xs = vec![0.0, *len];
        for o in ops {
            xs.push(o.x[0]);
            xs.push(o.x[1]);
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in xs.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a < 1e-6 {
                continue;
            }
            let o = ops.iter().find(|o| o.x[0] <= a + 1e-9 && o.x[1] >= b - 1e-9);
            match o {
                None => bxm(&mut wm, [a, -t, 0.0], [b, 0.0, *h]),
                Some(o) => {
                    bxm(&mut wm, [a, -t, 0.0], [b, 0.0, o.z[0]]);
                    bxm(&mut wm, [a, -t, o.z[1]], [b, 0.0, *h]);
                    if o.is_window {
                        bxm(&mut gm, [a, -t / 2.0 - 0.005, o.z[0]], [b, -t / 2.0 + 0.005, o.z[1]]);
                    }
                }
            }
        }
        let mut fl = Mesh::new();
        bxm(&mut fl, [-1.0, -t, -0.05], [*len + 1.0, 3.0, 0.0]);
        let sh = Mat4::translation([ox, oy, 0.0]);
        for (m, mi, nm) in [(wm, wall, "wall"), (gm, glass, "glass"), (fl, floor, "floor")] {
            let mut m = m;
            m.transform(&sh);
            scene.add(&format!("case{ci}_{nm}"), "shell", m, mi);
        }
        let fa = f.build(&span, iv, s0 + ci as u64);
        for (pi_, p) in fa.asset.parts.iter().enumerate() {
            let mut m = p.mesh.clone();
            m.transform(&sh);
            let mi = scene.material(p.mat.clone());
            scene.add(&format!("case{ci}_{}#{pi_}", fa.class), fa.class, m, mi);
        }
        // support boxes as thin translucent-free markers are omitted; report counts
        eprintln!("  built {} parts, {} supports, bounds {:?}", fa.asset.parts.len(), fa.asset.supports.len(), fa.asset.bounds());
        n += 1;
    }
    scene.write_glb(&out).unwrap();
    eprintln!("{n} cases -> {}", out.display());
}
