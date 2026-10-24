//! make_skirting_board: baseboards along the union of room floor polygons (floor) and ceiling crown boards.
use crate::mesh::*;
use crate::rng::Rng;
use crate::shp::*;

pub struct Skirting {
    pub mesh: Mesh,
    pub roughness: f32,
}

fn orient_ring(mut r: Vec<P2>, ccw: bool) -> Vec<P2> {
    if r.len() > 1 && (signed_area(&r) > 0.0) != ccw {
        r.reverse();
    }
    r
}

/// profile (thickness d at relative height t) from the python control points
fn profile(cp: &[(f64, f64)], t: f64) -> f64 {
    for w in cp.windows(2) {
        if t <= w[1].0 {
            let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
            return w[0].1 + (w[1].1 - w[0].1) * f;
        }
    }
    cp.last().unwrap().1
}

pub fn make(polys: &[G], z_base: f64, ceiling: bool, rng: &mut Rng) -> Option<Skirting> {
    let thickness = rng.uniform(0.02, 0.05);
    let height = rng.uniform(0.08, 0.15);
    let _color = [rng.f64(), rng.f64(), rng.f64()];
    let roughness = rng.uniform(0.5, 1.0);
    let n_peaks = rng.randint(1, 4) as usize;
    let start_y = rng.uniform(0.0, 0.5);
    let mid_x = rng.uniform(0.2, 0.8);
    let mut px: Vec<f64> = (0..n_peaks).map(|_| rng.uniform(0.0, mid_x)).collect();
    let mut py: Vec<f64> = (0..n_peaks).map(|_| rng.uniform(start_y, 1.0)).collect();
    px.sort_by(|a, b| a.partial_cmp(b).unwrap());
    py.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut cp = vec![(0.0, start_y)];
    cp.extend(px.iter().cloned().zip(py.iter().cloned()));
    cp.push((mid_x, 1.0));
    cp.push((1.0, 1.0));
    cp.dedup_by(|b, a| (b.0 - a.0).abs() < 1e-6);
    if polys.is_empty() {
        return None;
    }
    let un = union_all(polys);
    let b = buffer_mitre(&buffer_mitre(&un, 0.05), -0.05);
    let mut m = Mesh::new();
    let k = 6usize;
    for part in parts(&b) {
        if !is_polygon(&part) {
            continue;
        }
        let mut rings = vec![orient_ring(exterior(&part), true)];
        for h in interiors(&part) {
            rings.push(orient_ring(h, false));
        }
        for r in rings {
            let r: Vec<P2> = r[..r.len() - 1].to_vec();
            let n = r.len();
            if n < 3 {
                continue;
            }
            // miter offset per vertex (left of travel = into the room)
            let off: Vec<P2> = (0..n)
                .map(|i| {
                    let (p0, p1, p2) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
                    let n1 = unit2([-(p1[1] - p0[1]), p1[0] - p0[0]]);
                    let n2 = unit2([-(p2[1] - p1[1]), p2[0] - p1[0]]);
                    let dot = (n1[0] * n2[0] + n1[1] * n2[1]).max(-0.5);
                    [(n1[0] + n2[0]) / (1.0 + dot), (n1[1] + n2[1]) / (1.0 + dot)]
                })
                .collect();
            // closed profile loop (d, h): wall-top, then down the visible face
            let mut prof: Vec<(f64, f64)> = vec![(0.0, height)];
            for j in 0..=k {
                let z = height * (1.0 - j as f64 / k as f64);
                let t = 1.0 - z / height;
                prof.push((thickness * profile(&cp, 1.0 - t), z));
            }
            prof.push((0.0, 0.0));
            let z_of = |z: f64| if ceiling { z_base - z } else { z_base + z };
            for i in 0..n {
                let j = (i + 1) % n;
                for q in 0..prof.len() {
                    let q2 = (q + 1) % prof.len();
                    let pt = |v: usize, pr: (f64, f64)| -> V3 { [r[v][0] + off[v][0] * pr.0, r[v][1] + off[v][1] * pr.0, z_of(pr.1)] };
                    let (a, bb, c, d) = (pt(i, prof[q]), pt(j, prof[q]), pt(j, prof[q2]), pt(i, prof[q2]));
                    if ceiling {
                        m.add_quad(a, d, c, bb);
                    } else {
                        m.add_quad(a, bb, c, d);
                    }
                }
            }
        }
    }
    Some(Skirting { mesh: m, roughness: roughness as f32 })
}

fn unit2(v: P2) -> P2 {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-12);
    [v[0] / l, v[1] / l]
}
