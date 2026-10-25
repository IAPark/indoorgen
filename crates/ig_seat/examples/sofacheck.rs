//! sofacheck CLASS SEEDS: winding sanity of the sofa asset (directed-edge consistency, signed volume); dumps obj
use ig_seat::mesh::Mesh;
use ig_seat::rng::Rng;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let class = a[1].clone();
    for sp in a[2].split(',') {
        let s: u64 = sp.parse().unwrap();
        let mut rng = Rng::seeded(s);
        let f = ig_seat::sofa::make(&class, &mut rng).unwrap();
        let asset = f.asset(0);
        for p in &asset.parts {
            let m: &Mesh = &p.mesh;
            let mut dir: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
            for fc in &m.faces {
                for k in 0..fc.len() {
                    *dir.entry((fc[k], fc[(k + 1) % fc.len()])).or_insert(0) += 1;
                }
            }
            let mut bad = 0;
            for (&(a, b), &c) in dir.iter() {
                if c > 1 || dir.get(&(b, a)).copied().unwrap_or(0) == 0 {
                    bad += 1;
                }
            }
            let mut vol = 0.0;
            for fc in &m.faces {
                for k in 1..fc.len() - 1 {
                    let (p0, p1, p2) = (m.pos[fc[0] as usize], m.pos[fc[k] as usize], m.pos[fc[k + 1] as usize]);
                    vol += p0[0] * (p1[1] * p2[2] - p1[2] * p2[1]) - p0[1] * (p1[0] * p2[2] - p1[2] * p2[0]) + p0[2] * (p1[0] * p2[1] - p1[1] * p2[0]);
                }
            }
            println!("{class} seed {s}: verts {} faces {} bad directed edges {bad} signed volume {:.4}", m.pos.len(), m.faces.len(), vol / 6.0);
            if let Ok(d) = std::env::var("OBJ") {
                let mut o = String::new();
                for v in &m.pos {
                    o += &format!("v {} {} {}\n", v[0], v[1], v[2]);
                }
                for fc in &m.faces {
                    o += &format!("f {}\n", fc.iter().map(|x| (x + 1).to_string()).collect::<Vec<_>>().join(" "));
                }
                std::fs::write(format!("{d}/sofa_{s}.obj"), o).unwrap();
            }
        }
    }
}
