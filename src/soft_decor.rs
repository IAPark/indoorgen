//! Throw cushions on sofas and armchairs after the solve (program.soft_decor): 3D-FRONT / Hypersim living rooms always have them, the
//! solver's sofas were bare. A cushion is a rounded-square superquadric (0.36-0.48 m, 0.11-0.15 m thick) standing on the seat and leaning
//! ~15 degrees back against the backrest; the seat height comes from the sofa's own visual mesh. Scene geometry only; own stream.
use crate::furn::state::State;
use crate::mesh::{Material, Mesh, Scene};
use crate::rng::Rng;
use serde_json::json;
use std::collections::HashMap;

/// superquadric cushion centred at the origin: thickness along x, width along y, height along z
fn cushion(t: f64, w: f64, h: f64) -> Mesh {
    let (nu, nv) = (24usize, 12usize);
    let sp = |c: f64, e: f64| c.signum() * c.abs().powf(e);
    let p = |u: f64, v: f64| -> [f64; 3] {
        let (e1, e2) = (0.35, 0.3);
        [t / 2.0 * sp(v.cos(), e1) * sp(u.cos(), e2), w / 2.0 * sp(v.cos(), e1) * sp(u.sin(), e2), h / 2.0 * sp(v.sin(), e1)]
    };
    let mut m = Mesh::new();
    for i in 0..nu {
        for j in 0..nv {
            let (u0, u1) = (2.0 * std::f64::consts::PI * i as f64 / nu as f64, 2.0 * std::f64::consts::PI * (i + 1) as f64 / nu as f64);
            let (v0, v1) = (-std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * j as f64 / nv as f64, -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (j + 1) as f64 / nv as f64);
            m.add_face(&[p(u0, v0), p(u1, v0), p(u1, v1), p(u0, v1)]);
        }
    }
    m
}

pub fn add(scene: &mut Scene, state: &State, visuals: &HashMap<String, crate::assets::Asset>, seed: u64) -> Vec<String> {
    let mut log = vec![];
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let sofa = g.class == "SofaFactory";
        if !(sofa || g.class == "ArmChairFactory") { continue }
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "soft_decor", k]));
        if r.f64() >= if sofa { 0.85 } else { 0.5 } { continue }
        let Some(a) = visuals.get(k) else { continue };
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        let (d, w) = (hi[0] - lo[0], hi[1] - lo[1]);
        // seat top: highest visual vertex in the front half of the footprint, away from the arms
        let seat = a.parts.iter().flat_map(|p| p.mesh.pos.iter()).filter(|q| q[0] > lo[0] + 0.45 * d && q[0] < lo[0] + 0.8 * d && q[1] > lo[1] + 0.25 * w && q[1] < hi[1] - 0.25 * w).map(|q| q[2]).fold(f64::MIN, f64::max);
        if seat == f64::MIN || seat > lo[2] + 0.75 * (hi[2] - lo[2]) { continue }
        // backrest front face: lowest x of the visual above the seat in the middle of the width
        let back = a.parts.iter().flat_map(|p| p.mesh.pos.iter()).filter(|q| q[2] > seat + 0.12 && q[1] > lo[1] + 0.3 * w && q[1] < hi[1] - 0.3 * w && q[0] < lo[0] + 0.6 * d).map(|q| q[0]).fold(f64::MIN, f64::max);
        let back = if back == f64::MIN { lo[0] + 0.25 * d } else { back };
        let n = if sofa { ((w / 0.65).floor() as usize).clamp(2, 4) } else { 1 };
        let (cw, ct) = (r.uniform(0.36, 0.48), r.uniform(0.11, 0.15));
        // (narrower than their spacing, twist included: neighbours overlapped, one resting on the next)
        let cw = if n > 1 { cw.min(0.6 * w / n as f64 - 0.06) } else { cw };
        let palette = [("fabric.SofaFabric", 2.0), ("fabric.Velvet", 1.0), ("fabric.CoarseKnitFabric", 1.5), ("fabric.FineKnitFabric", 1.0)];
        let tot: f64 = palette.iter().map(|x| x.1).sum();
        let mut u = r.f64() * tot;
        let cls = palette.iter().find(|x| { u -= x.1; u < 0.0 }).map_or("fabric.SofaFabric", |x| x.0);
        let mut mat = Material::diffuse(&format!("cushion:{k}"), [0.7, 0.68, 0.64], 0.8);
        mat.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": r.next_u64() % 2_000_000_000}));
        let mi = scene.material(mat);
        let tilt = 15f64.to_radians();
        for i in 0..n {
            let f = (i as f64 + 0.5) / n as f64;
            let y = if n == 1 { (lo[1] + hi[1]) / 2.0 } else { lo[1] + w * (0.2 + 0.6 * f) };
            let yaw = r.uniform(-0.12, 0.12);
            // the seat under this cushion (it rises toward the backrest: measured mid-seat, cushions sank 1-2 cm into it)
            let (fx0, fx1) = (back, back + ct + 0.06 + cw * tilt.sin());
            let seat = a.parts.iter().flat_map(|p| p.mesh.pos.iter()).filter(|q| q[0] > fx0 && q[0] < fx1 && (q[1] - y).abs() < cw / 2.0 && q[2] < seat + 0.12).map(|q| q[2]).fold(seat, f64::max);
            let mut c = cushion(ct, cw, cw);
            for q in c.pos.iter_mut() {
                // lean back (top toward -x), small twist, then into the sofa frame, then world
                let (x, yy, z) = (q[0], q[1], q[2]);
                let (x1, z1) = (x * tilt.cos() - z * tilt.sin(), x * tilt.sin() + z * tilt.cos());
                let (x2, y2) = (x1 * yaw.cos() - yy * yaw.sin(), x1 * yaw.sin() + yy * yaw.cos());
                let (cx, cz) = (back + ct / 2.0 + 0.02 + cw / 2.0 * tilt.sin(), seat + cw / 2.0 * tilt.cos() + 0.005);
                *q = o.pose.apply([cx + x2, y + y2, cz + z1]);
            }
            scene.add(&format!("cushion_{}_{i}#0", k.replace('/', "_")), "CushionFactory", c, mi);
        }
        log.push(format!("{n} cushion(s) on {k}"));
    }
    log
}
