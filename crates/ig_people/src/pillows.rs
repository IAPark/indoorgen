//! Pillow settling after people were added (`gen2 --people N`, N > 0): pillows / towels of a bed (cloth followers) end up on the
//! mattress, a nightstand or the floor, resting on a surface, not inside the person and not stacked in a heap.
//! (1) a pillow taller than 28 cm is the diverged pressure sim of a torus pillow: flattened to 18 cm; (2) pillows overlapping the lying
//! person or each other slide to the nearest free spot on the bed; (3) every follower is dropped onto the surface beneath its footprint.
use crate::place::{Cover, Placement};
use ig_core::mesh::{Mat4, Scene, V3};
use serde_json::json;

fn node_idx(scene: &Scene, name: &str) -> Option<usize> {
    scene.nodes.iter().position(|n| n.name == name)
}

pub fn settle(people: &[Placement], scene: &mut Scene, floor_z: f64) {
    // followers per bed
    let mut beds: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for j in &scene.cloth {
        if j["kind"] == "follow" {
            if let Some(p) = j["part"].as_str() {
                if let Some((k, _)) = p.split_once('#') {
                    beds.entry(k.to_string()).or_default().push(p.to_string());
                }
            }
        }
    }
    for (bed, parts) in beds {
        let fol: Vec<String> = parts;
        // bed AABB without the followers
        let (mut blo, mut bhi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for n in &scene.nodes {
            if n.name.starts_with(&format!("{bed}#")) && !fol.contains(&n.name) && !n.mesh.is_empty() {
                let (a, b) = n.mesh.bounds();
                for k in 0..3 {
                    blo[k] = blo[k].min(a[k]);
                    bhi[k] = bhi[k].max(b[k]);
                }
            }
        }
        if blo[0] > bhi[0] {
            continue;
        }
        // people lying on this bed: world AABB of their meshes; the pillow nearest to a head stays under it
        let mut occ: Vec<(V3, V3)> = vec![];
        let mut heads: Vec<[f64; 2]> = vec![];
        for (pi, pl) in people.iter().enumerate() {
            if pl.kind == crate::PoseKind::LieBed && pl.info.support.as_deref() == Some(bed.as_str()) {
                let pre = format!("person{pi}_");
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for n in scene.nodes.iter().filter(|n| n.name.starts_with(&pre)) {
                    let (a, b) = n.mesh.bounds();
                    for k in 0..3 {
                        lo[k] = lo[k].min(a[k]);
                        hi[k] = hi[k].max(b[k]);
                    }
                }
                if lo[0] < hi[0] {
                    occ.push((lo, hi));
                }
                if let Some(lm) = crate::cover::landmarks(pl) {
                    heads.push([lm.head[0], lm.head[1]]);
                }
                let _ = Cover::None;
            }
        }
        let n = fol.len();
        // 1: flatten oversized, remember footprints
        let mut fp: Vec<(V3, V3)> = vec![];
        for name in &fol {
            let Some(ni) = node_idx(scene, name) else { fp.push(([0.0; 3], [0.0; 3])); continue };
            let (lo, hi) = scene.nodes[ni].mesh.bounds();
            if hi[2] - lo[2] > 0.28 {
                let f = 0.18 / (hi[2] - lo[2]);
                let zc = lo[2];
                for q in scene.nodes[ni].mesh.pos.iter_mut() {
                    q[2] = zc + (q[2] - zc) * f;
                }
            }
            fp.push(scene.nodes[ni].mesh.bounds());
        }
        // 2: lateral resolution. Pillows under a head are anchored; the others get the nearest free spot inside the bed outline.
        let anchored: Vec<bool> = fp.iter().map(|(lo, hi)| heads.iter().any(|h| h[0] > lo[0] - 0.05 && h[0] < hi[0] + 0.05 && h[1] > lo[1] - 0.05 && h[1] < hi[1] + 0.05)).collect();
        let mut pos: Vec<[f64; 2]> = fp.iter().map(|(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]).collect();
        let half: Vec<[f64; 2]> = fp.iter().map(|(lo, hi)| [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0]).collect();
        let mut done = vec![false; n];
        for i in 0..n {
            if anchored[i] {
                done[i] = true;
            }
        }
        let overlap = |c: [f64; 2], h: [f64; 2], c2: [f64; 2], h2: [f64; 2], pad: f64| (c[0] - c2[0]).abs() < h[0] + h2[0] - pad && (c[1] - c2[1]).abs() < h[1] + h2[1] - pad;
        for i in 0..n {
            if done[i] {
                continue;
            }
            let ok = |c: [f64; 2], pos: &Vec<[f64; 2]>, done: &Vec<bool>| -> bool {
                if c[0] - half[i][0] < blo[0] + 0.02 || c[0] + half[i][0] > bhi[0] - 0.02 || c[1] - half[i][1] < blo[1] + 0.02 || c[1] + half[i][1] > bhi[1] - 0.02 {
                    return false;
                }
                for (lo, hi) in &occ {
                    if overlap(c, half[i], [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0], 0.06) {
                        return false;
                    }
                }
                (0..n).all(|j| j == i || !done[j] || !overlap(c, half[i], pos[j], half[j], 0.08))
            };
            let mut best: Option<([f64; 2], f64)> = None;
            if ok(pos[i], &pos, &done) {
                best = Some((pos[i], 0.0));
            } else {
                let step = 0.04;
                let (nx, ny) = (((bhi[0] - blo[0]) / step) as i32, ((bhi[1] - blo[1]) / step) as i32);
                for a in 0..=nx {
                    for b in 0..=ny {
                        let c = [blo[0] + a as f64 * step, blo[1] + b as f64 * step];
                        let d = (c[0] - pos[i][0]).hypot(c[1] - pos[i][1]);
                        if best.map_or(true, |(_, bd)| d < bd) && ok(c, &pos, &done) {
                            best = Some((c, d));
                        }
                    }
                }
            }
            if let Some((c, _)) = best {
                pos[i] = c;
            }
            done[i] = true;
        }
        // 3: settle onto the surface under the footprint
        let excl: Vec<String> = fol.clone();
        // soft bodies on the bed drape over the bed's sheets (rest on the highest point, bottom layer follows the surface)
        let bed_col = {
            let mut pos: Vec<V3> = vec![];
            let mut faces: Vec<Vec<u32>> = vec![];
            for n in &scene.nodes {
                if n.name.starts_with(&format!("{bed}#")) && !fol.contains(&n.name) && !n.mesh.is_empty() {
                    let o = pos.len() as u32;
                    pos.extend_from_slice(&n.mesh.pos);
                    faces.extend(n.mesh.faces.iter().map(|f| f.iter().map(|i| i + o).collect::<Vec<u32>>()));
                }
            }
            ig_kit::cloth::Collider::new(&pos, &faces)
        };
        for (i, name) in fol.iter().enumerate() {
            let Some(ni) = node_idx(scene, name) else { continue };
            let (lo, hi) = scene.nodes[ni].mesh.bounds();
            let (c0, h0) = ([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], half[i]);
            let (dx, dy) = (pos[i][0] - c0[0], pos[i][1] - c0[1]);
            let mut zs: Vec<f64> = vec![];
            for sx in 0..3 {
                for sy in 0..3 {
                    let (x, y) = (pos[i][0] + h0[0] * (-0.5 + 0.5 * sx as f64), pos[i][1] + h0[1] * (-0.5 + 0.5 * sy as f64));
                    zs.push(crate::audit::top_below(scene, x, y, hi[2] + 0.3, &excl, floor_z).0);
                }
            }
            if std::env::var_os("PILLOW_DBG").is_some() {
                let c = crate::audit::top_below(scene, pos[i][0], pos[i][1], hi[2] + 0.3, &excl, floor_z);
                eprintln!("PILLOW {name} lo.z {:.3} hi.z {:.3} zs {:?} centre top {:.3} on {}", lo[2], hi[2], zs.iter().map(|z| (z * 1000.0).round() / 1000.0).collect::<Vec<_>>(), c.0, c.1);
            }
            zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let under_head = anchored[i];
            let sup = if under_head { zs[4] - 0.03 } else { zs[4] };
            let dz = sup + 0.01 - lo[2];
            if dx.abs() < 1e-6 && dy.abs() < 1e-6 && dz.abs() < 1e-6 {
                continue;
            }
            scene.nodes[ni].mesh.transform(&Mat4::translation([dx, dy, dz]));
            let on_bed = crate::audit::top_below(scene, pos[i][0], pos[i][1], hi[2] + 0.3, &excl, floor_z).1.starts_with(&format!("{bed}#"));
            if on_bed && !under_head && std::env::var_os("PILLOW_NOWRAP").is_none() {
                let thin = (hi[2] - lo[2]) < 0.06;
                if thin {
                    ig_kit::cloth::rest_and_wrap(&mut scene.nodes[ni].mesh.pos, &bed_col, 0.02, 0.005, 0.1);
                } else {
                    ig_kit::cloth::rest_and_wrap(&mut scene.nodes[ni].mesh.pos, &bed_col, 0.07, 0.01, 0.05);
                }
            }
            for j in scene.cloth.iter_mut() {
                if j["kind"] == "follow" && j["part"] == name.as_str() {
                    j["xy"] = json!([pos[i][0], pos[i][1]]);
                    j["z_used"] = json!(sup);
                }
            }
        }
    }
}
