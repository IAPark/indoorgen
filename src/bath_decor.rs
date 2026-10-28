//! Bathroom dressing after the solve (program.bath_decor): towels folded over the towel bars (HardwareFactory rails >= 0.3 m), a bath mat
//! in front of the bathtub or toilet, and 2-4 toiletry bottles on the sink's back rim. Bathroom views showed ~6 objects (Hypersim
//! bathrooms: towels, mats, toiletries everywhere); the clutter pass put its items on shelves the cramped views rarely see.
//! Scene geometry only (bottles are BottleFactory assets); own stream.
use crate::furn::state::State;
use crate::mesh::{Material, Mesh, Scene};
use crate::rng::Rng;
use serde_json::json;
use std::collections::HashMap;

fn fabric(scene: &mut Scene, name: &str, r: &mut Rng) -> usize {
    // towels: whites, greys, beiges, some muted colours
    let pal: [[f32; 3]; 7] = [[0.92, 0.92, 0.9], [0.85, 0.83, 0.78], [0.6, 0.6, 0.6], [0.75, 0.68, 0.58], [0.35, 0.45, 0.55], [0.55, 0.62, 0.55], [0.7, 0.55, 0.5]];
    let rgb = pal[r.randint(0, 7) as usize];
    let mut m = Material::diffuse(name, rgb, 0.95);
    m.spec = None;
    scene.material(m)
}

fn bx(lo: [f64; 3], hi: [f64; 3], pose: &crate::furn::geom::Pose) -> Mesh {
    let mut m = Mesh::cuboid(lo, hi);
    for q in m.pos.iter_mut() { *q = pose.apply(*q) }
    m
}

pub fn add(scene: &mut Scene, state: &State, visuals: &HashMap<String, crate::assets::Asset>, assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    let in_bath = |o: &crate::furn::state::ObjState| o.relations.iter().any(|r| r.target.starts_with("bathroom"));
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "bath_decor", k]));
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        match g.class.as_str() {
            // towel bar: a towel folded over it, hanging 0.35-0.55 m in front of the wall on both sides of the rail
            "HardwareFactory" if hi[1] - lo[1] >= 0.3 => {
                if r.f64() >= 0.85 { continue }
                let m = fabric(scene, &format!("towel:{k}"), &mut r);
                let w = (hi[1] - lo[1]) * r.uniform(0.6, 0.85);
                let yc = (lo[1] + hi[1]) / 2.0 + r.uniform(-0.05, 0.05);
                let drop = r.uniform(0.35, 0.55);
                let (x0, x1) = (lo[0] - 0.005, hi[0] + 0.01);
                let zt = hi[2] + 0.01;
                let t = 0.012;
                let mut mesh = bx([x0, yc - w / 2.0, zt - t], [x1, yc + w / 2.0, zt], &o.pose);
                mesh.append(&bx([x0, yc - w / 2.0, zt - drop], [x0 + t, yc + w / 2.0, zt], &o.pose));
                mesh.append(&bx([x1 - t, yc - w / 2.0, zt - drop * r.uniform(0.6, 0.95)], [x1, yc + w / 2.0, zt], &o.pose));
                scene.add(&format!("towel_{}#0", k.replace('/', "_")), "TowelFactory", mesh, m);
                log.push(format!("towel on {k}"));
            }
            // bath mat on the floor in front of the bathtub / toilet (front = local +x)
            "BathtubFactory" | "ToiletFactory" if in_bath(o) => {
                if r.f64() >= if g.class == "BathtubFactory" { 0.8 } else { 0.4 } { continue }
                let m = fabric(scene, &format!("bathmat:{k}"), &mut r);
                let (mw, md) = (r.uniform(0.5, 0.8).min(hi[1] - lo[1]), r.uniform(0.4, 0.55));
                let yc = (lo[1] + hi[1]) / 2.0;
                let mesh = bx([hi[0] + 0.03, yc - mw / 2.0, lo[2]], [hi[0] + 0.03 + md, yc + mw / 2.0, lo[2] + 0.012], &o.pose);
                scene.add(&format!("bathmat_{}#0", k.replace('/', "_")), "BathMatFactory", mesh, m);
                log.push(format!("mat at {k}"));
            }
            // toiletries on the back rim of the sink (highest visual surface in the back 20 % of its depth)
            "StandingSinkFactory" | "BathroomSinkFactory" => {
                let Some(a) = visuals.get(k) else { continue };
                let d = hi[0] - lo[0];
                let rim: Vec<[f64; 3]> = a.parts.iter().flat_map(|p| p.mesh.pos.iter().copied()).filter(|q| q[0] < lo[0] + 0.2 * d && q[2] > lo[2] + 0.5 * (hi[2] - lo[2])).collect();
                if std::env::var_os("BATH_DEBUG").is_some() { eprintln!("BATHDBG {k} lo {lo:?} hi {hi:?} rim {} vis z {:?}", rim.len(), a.parts.iter().flat_map(|p| p.mesh.pos.iter()).map(|q| q[2]).fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)))); }
                if rim.is_empty() { continue }
                let n = r.randint(2, 5) as usize;
                let mut placed = 0;
                let mut taken: Vec<f64> = vec![];
                for i in 0..n * 4 {
                    if placed >= n { break }
                    // a flat spot: the visual's top surface within 4 cm of (x, y) is level (no tap, no basin edge)
                    let x = lo[0] + d * r.uniform(0.04, 0.18);
                    let y = lo[1] + (hi[1] - lo[1]) * r.uniform(0.06, 0.94);
                    if taken.iter().any(|t| (t - y).abs() < 0.09) { continue }
                    let near: Vec<f64> = rim.iter().filter(|q| (q[0] - x).hypot(q[1] - y) < 0.04).map(|q| q[2]).collect();
                    if near.len() < 3 { continue }
                    let zt = near.iter().cloned().fold(f64::MIN, f64::max);
                    let flat = near.iter().filter(|&&z| z > zt - 0.03).count() as f64 / near.len() as f64;
                    let around = rim.iter().filter(|q| (q[0] - x).hypot(q[1] - y) < 0.09 && q[2] > zt + 0.02).count();
                    if flat < 0.3 || around > 0 { continue }
                    taken.push(y);
                    let b = assets.visual("BottleFactory", r.next_u64() % 1_000_000, 0);
                    let (blo, bhi) = b.bounds();
                    let s = (0.16 / (bhi[2] - blo[2]).max(0.05)).min(1.0);
                    for (pi, p) in b.parts.iter().enumerate() {
                        let mut m = p.mesh.clone();
                        for q in m.pos.iter_mut() {
                            let l = [x + (q[0] - (blo[0] + bhi[0]) / 2.0) * s, y + (q[1] - (blo[1] + bhi[1]) / 2.0) * s, zt + (q[2] - blo[2]) * s];
                            *q = o.pose.apply(l);
                        }
                        let mi = scene.material(p.mat.clone());
                        scene.add(&format!("toiletry_{}_{i}#{pi}", k.replace('/', "_")), "BottleFactory", m, mi);
                    }
                    placed += 1;
                }
                if placed > 0 { log.push(format!("{placed} toiletries on {k}")) }
            }
            _ => {}
        }
    }
    log
}
