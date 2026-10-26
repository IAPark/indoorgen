//! People plausibility audit of a finished scene (`gen2 ... --qc`): pose kinds per room, bodies overlapping each other / furniture,
//! unsupported sitters, reach targets, facing a blank wall, crouching at toilets, pillows floating / oversized / piled up.
//! Everything is measured again from the final placements and the final scene meshes (not from the placement code's own bookkeeping).
use crate::place::Placement;
use crate::pworld::*;
use crate::PoseKind;
use ig_core::mesh::{Scene, V3};
use serde_json::json;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// distance from p to triangle (Ericson, closest point on triangle)
pub fn pt_tri_dist(p: V3, t: &[V3; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                let v = d1 / (d1 - d3);
                [a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]]
            } else {
                let cp = sub(p, c);
                let (d5, d6) = (dot(ab, cp), dot(ac, cp));
                if d6 >= 0.0 && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                        let w = d2 / (d2 - d6);
                        [a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]]
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
                            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
                            [b[0] + w * (c[0] - b[0]), b[1] + w * (c[1] - b[1]), b[2] + w * (c[2] - b[2])]
                        } else {
                            let den = 1.0 / (va + vb + vc);
                            let (v, w) = (vb * den, vc * den);
                            [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
                        }
                    }
                }
            }
        }
    };
    let d = sub(p, q);
    dot(d, d).sqrt()
}

pub fn facing(p: &Placement) -> [f64; 2] {
    let yaw = p.pose.0[1][0].atan2(p.pose.0[0][0]);
    [-yaw.sin(), yaw.cos()]
}

/// audit of the people (and the pillows of the beds) of a finished scene; the JSON is merged into the qc report by gen2
pub fn audit(w: &World, people: &[Placement], scene: &Scene) -> serde_json::Value {
    use PoseKind::*;
    let boxes: Vec<Vec<WBox>> = people.iter().map(|p| p.world_boxes()).collect();
    let windows: Vec<[f64; 2]> = scene
        .nodes
        .iter()
        .filter(|n| n.class == "Window" && !n.mesh.is_empty())
        .map(|n| {
            let (a, b) = n.mesh.bounds();
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
        })
        .collect();
    let mut per: Vec<serde_json::Value> = vec![];
    let mut issues: Vec<String> = vec![];
    let mut by_room: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (i, p) in people.iter().enumerate() {
        let o = [p.pose.0[0][3], p.pose.0[1][3]];
        let f = facing(p);
        let ri = w.rooms.iter().position(|r| r.name == p.info.room).unwrap_or(0);
        let room = &w.rooms[ri];
        let tag = format!("person{i} {:?} [{}]", p.kind, room.name);
        by_room.entry(room.name.clone()).or_default().push(format!("{:?}", p.kind));
        let sup = p.info.support.as_ref().and_then(|k| w.objs.iter().position(|ob| &ob.key == k));
        // body vs furniture (skipping the support), vs walls
        let skip = match p.kind {
            SitUpright | SitRelaxed | SitBedEdge | LieBed | LieSofa => sup,
            _ => None,
        };
        let pen_furn = w.pen_furniture(&boxes[i], skip);
        let pen_wall = w.pen_wall(&boxes[i], ri).max(0.0);
        if pen_furn > 0.03 {
            issues.push(format!("{tag}: body penetrates furniture {:.0} mm", pen_furn * 1000.0));
        }
        if pen_wall > 0.03 {
            issues.push(format!("{tag}: body through wall {:.0} mm", pen_wall * 1000.0));
        }
        let mut m = serde_json::Map::new();
        m.insert("i".into(), json!(i));
        m.insert("kind".into(), json!(format!("{:?}", p.kind)));
        m.insert("room".into(), json!(room.name));
        m.insert("support".into(), json!(p.info.support));
        m.insert("at".into(), json!([o[0], o[1]]));
        m.insert("pen_furn".into(), json!(pen_furn));
        // facing: distance to the wall ahead, furniture / person / window ahead
        if !matches!(p.kind, LieBed | LieSofa) {
            let wall_ahead = ray_poly(o, f, &room.poly);
            let mut obj_ahead = f64::MAX;
            for (oi, ob) in w.objs.iter().enumerate() {
                if ob.room != ri || !w.blocks(oi) || ob.hi[2] - w.floor_z < 0.4 || Some(ob.key.as_str()) == p.info.support.as_deref() {
                    continue;
                }
                if let Some(t) = ray_aabb(o, f, [ob.lo[0] - 0.1, ob.lo[1] - 0.1], [ob.hi[0] + 0.1, ob.hi[1] + 0.1]) {
                    obj_ahead = obj_ahead.min(t);
                }
            }
            let mut person_ahead = f64::MAX;
            for (j, q) in people.iter().enumerate() {
                if j != i {
                    let d = [q.pose.0[0][3] - o[0], q.pose.0[1][3] - o[1]];
                    let l = d[0].hypot(d[1]);
                    if l > 0.1 && (d[0] * f[0] + d[1] * f[1]) / l > 0.85 {
                        person_ahead = person_ahead.min(l);
                    }
                }
            }
            let window_ahead = windows
                .iter()
                .filter_map(|wc| {
                    let d = [wc[0] - o[0], wc[1] - o[1]];
                    let l = d[0].hypot(d[1]);
                    if l > 0.1 && (d[0] * f[0] + d[1] * f[1]) / l > 0.9 && (l - wall_ahead).abs() < 0.8 { Some(l) } else { None }
                })
                .fold(f64::MAX, f64::min);
            let blank = matches!(p.kind, Stand | Walk) && wall_ahead < 1.3 && obj_ahead > wall_ahead - 0.05 && person_ahead > wall_ahead && window_ahead == f64::MAX;
            m.insert("wall_ahead".into(), json!(wall_ahead));
            m.insert("obj_ahead".into(), json!(if obj_ahead == f64::MAX { serde_json::Value::Null } else { json!(obj_ahead) }));
            m.insert("blank_wall".into(), json!(blank));
            if blank {
                issues.push(format!("{tag}: faces a blank wall at {:.2} m", wall_ahead));
            }
        }
        // lying people: the heels (lowest points of the foot-end quarter of the body) rest on the surface
        if p.kind == LieBed {
            let pre = format!("person{i}_");
            let mut pts: Vec<V3> = vec![];
            for n in scene.nodes.iter().filter(|n| n.name.starts_with(&pre)) {
                pts.extend(n.mesh.pos.iter().copied());
            }
            if !pts.is_empty() {
                let along = |q: &V3| q[0] * f[0] + q[1] * f[1];
                let (mut a0, mut a1) = (f64::MAX, f64::MIN);
                for q in &pts {
                    a0 = a0.min(along(q));
                    a1 = a1.max(along(q));
                }
                // f = the avatar's +y = towards the feet for a lying avatar (head towards -y)
                let thr = a1 - 0.25 * (a1 - a0);
                let mut low: Vec<&V3> = pts.iter().filter(|q| along(q) > thr).collect();
                low.sort_by(|a, b| a[2].partial_cmp(&b[2]).unwrap());
                let mut gap = f64::MAX;
                for q in low.iter().take(40) {
                    let (z, _) = top_below(scene, q[0], q[1], q[2] + 0.02, &[], w.floor_z);
                    gap = gap.min(q[2] - z);
                }
                m.insert("heel_gap".into(), json!(gap));
                if gap > 0.06 {
                    issues.push(format!("{tag}: heels {:.0} cm above the surface", gap * 100.0));
                }
            }
        }
        // sitters need a seat under the hips
        if matches!(p.kind, SitUpright | SitRelaxed | SitBedEdge) {
            let pi = match p.seat_z {
                Some(z) => crate::pose_info_seat(p.avatar, p.kind, z),
                None => crate::pose_info(p.avatar, p.kind),
            };
            if let (Some(seat), Some(si)) = (pi.seat, sup) {
                let hip = p.pose.apply(seat);
                let ob = &w.objs[si];
                let col = ob.column(hip[0], hip[1]);
                let gap = col.iter().copied().filter(|z| *z <= hip[2] + 0.12).last().map(|z| hip[2] - z);
                m.insert("hip_gap".into(), json!(gap));
                if gap.map_or(true, |g| g > 0.12) {
                    issues.push(format!("{tag}: hips not over the seat of {} (gap {:?})", ob.key, gap.map(|g| (g * 100.0).round() / 100.0)));
                }
            } else if sup.is_none() {
                issues.push(format!("{tag}: sitter without support"));
            }
        }
        // reach: a furniture surface within 25 cm of the hand
        if p.kind == Reach {
            let pi = crate::pose_info(p.avatar, p.kind);
            if let Some(h) = pi.hands {
                let hw: Vec<V3> = h.iter().map(|q| p.pose.apply(*q)).collect();
                let mut best = f64::MAX;
                let mut what = String::new();
                for (oi, ob) in w.objs.iter().enumerate() {
                    if ob.room != ri || !w.blocks(oi) {
                        continue;
                    }
                    for hp in &hw {
                        if (0..3).any(|k| hp[k] < ob.lo[k] - 0.4 || hp[k] > ob.hi[k] + 0.4) {
                            continue;
                        }
                        for t in &ob.tris {
                            let d = pt_tri_dist(*hp, t);
                            if d < best {
                                best = d;
                                what = ob.key.clone();
                            }
                        }
                    }
                }
                m.insert("reach_dist".into(), json!(if best == f64::MAX { serde_json::Value::Null } else { json!(best) }));
                m.insert("reach_target".into(), json!(what));
                let hz = hw.iter().map(|q| q[2]).fold(f64::MIN, f64::max) - w.floor_z;
                m.insert("reach_height".into(), json!(hz));
                if best > 0.25 {
                    issues.push(format!("{tag}: reaches for nothing (nearest furniture {:.2} m)", best));
                }
            }
        }
        // crouching next to a toilet
        if p.kind == Crouch {
            let near_toilet = w.objs.iter().any(|ob| ob.class.contains("Toilet") && ob.lo[0] - 1.0 < o[0] && o[0] < ob.hi[0] + 1.0 && ob.lo[1] - 1.0 < o[1] && o[1] < ob.hi[1] + 1.0);
            if near_toilet {
                issues.push(format!("{tag}: crouches next to a toilet"));
            }
        }
        per.push(serde_json::Value::Object(m));
    }
    // pairs
    let mut pairs = vec![];
    for i in 0..people.len() {
        for j in i + 1..people.len() {
            let pen = pen_boxes(&boxes[i], &boxes[j]);
            let d = (people[i].pose.0[0][3] - people[j].pose.0[0][3]).hypot(people[i].pose.0[1][3] - people[j].pose.0[1][3]);
            if pen > 0.02 {
                issues.push(format!("person{i} {:?} and person{j} {:?} overlap {:.0} mm", people[i].kind, people[j].kind, pen * 1000.0));
            }
            let same_room = people[i].info.room == people[j].info.room;
            if same_room && people[i].kind == people[j].kind && !matches!(people[i].kind, SitUpright | SitRelaxed) {
                issues.push(format!("person{i} and person{j} in {} have the same pose {:?}", people[i].info.room, people[i].kind));
            }
            pairs.push(json!({"a": i, "b": j, "pen": pen, "dist": d}));
        }
    }
    for (room, kinds) in &by_room {
        let ty = room.split('_').next().unwrap_or("");
        let cap = match ty {
            "bathroom" => 1,
            _ => usize::MAX,
        };
        if kinds.len() > cap {
            issues.push(format!("{room}: {} people in a {ty}", kinds.len()));
        }
    }
    // pillows: measured on the final scene meshes
    let mut pillows = vec![];
    for o in &w.objs {
        if o.pillows.is_empty() {
            continue;
        }
        let excl: Vec<String> = o.pillows.iter().map(|&(_, _, pi)| format!("{}#{pi}", o.key)).collect();
        let mut pl_lo_hi: Vec<(usize, V3, V3)> = vec![];
        for &(_, _, pi) in &o.pillows {
            let name = format!("{}#{pi}", o.key);
            let Some(n) = scene.nodes.iter().find(|n| n.name == name) else { continue };
            let (lo, hi) = n.mesh.bounds();
            pl_lo_hi.push((pi, lo, hi));
        }
        for (k, &(pi, lo, hi)) in pl_lo_hi.iter().enumerate() {
            let name = format!("{}#{pi}", o.key);
            let h = hi[2] - lo[2];
            // support under 9 sample points: highest surface of any non-pillow, non-person node below the pillow centre
            let mut best_gap = f64::MAX;
            let mut sup_name = String::new();
            for sx in 0..3 {
                for sy in 0..3 {
                    let x = lo[0] + (hi[0] - lo[0]) * (0.2 + 0.3 * sx as f64);
                    let y = lo[1] + (hi[1] - lo[1]) * (0.2 + 0.3 * sy as f64);
                    let (z, who) = top_below(scene, x, y, 0.5 * (lo[2] + hi[2]) + 0.02, &excl, w.floor_z);
                    let gap = lo[2] - z;
                    if gap < best_gap {
                        best_gap = gap;
                        sup_name = who;
                    }
                }
            }
            let mut stacked = 0;
            for (k2, &(_, lo2, hi2)) in pl_lo_hi.iter().enumerate() {
                if k2 == k {
                    continue;
                }
                let ov = |d: usize| (hi[d].min(hi2[d]) - lo[d].max(lo2[d])).max(0.0);
                let a = ov(0) * ov(1);
                let small = ((hi[0] - lo[0]) * (hi[1] - lo[1])).min((hi2[0] - lo2[0]) * (hi2[1] - lo2[1]));
                if ov(2) > 0.02 && a > 0.4 * small {
                    stacked += 1;
                }
            }
            let floating = best_gap > 0.06;
            if floating {
                issues.push(format!("pillow {name}: floats {:.0} cm above {sup_name}", best_gap * 100.0));
            }
            if h > 0.30 {
                issues.push(format!("pillow {name}: {:.0} cm tall (oversized)", h * 100.0));
            }
            if stacked > 0 {
                issues.push(format!("pillow {name}: overlaps {stacked} other pillow(s)"));
            }
            if best_gap < -0.10 {
                issues.push(format!("pillow {name}: sunk {:.0} cm into {sup_name}", -best_gap * 100.0));
            }
            pillows.push(json!({"part": name, "height": h, "gap": best_gap, "support": sup_name, "stacked": stacked}));
        }
    }
    let kinds: std::collections::BTreeMap<String, usize> = people.iter().fold(Default::default(), |mut m, p| {
        *m.entry(format!("{:?}", p.kind)).or_default() += 1;
        m
    });
    json!({"n": people.len(), "kinds": kinds, "rooms": by_room, "per_person": per, "pairs": pairs, "pillows": pillows, "issues": issues})
}

/// highest scene surface under (x, y) that is <= zmax, ignoring pillows / towels of `bed`, people, ceilings, walls
pub fn top_below(scene: &Scene, x: f64, y: f64, zmax: f64, excl: &[String], floor_z: f64) -> (f64, String) {
    let mut best = (floor_z, "floor".to_string());
    for n in &scene.nodes {
        if n.class == "Person" || n.class == "Window" || n.class == "Door" || n.name.contains("ceiling") || n.name.contains("wall") {
            continue;
        }
        if excl.contains(&n.name) {
            continue;
        }
        if let Some((k, _)) = n.name.split_once('#') {
            if k.starts_with("mess_") {
                continue;
            }
        } else if !n.name.ends_with(".floor") {
            continue;
        }
        let (lo, hi) = n.mesh.bounds();
        if x < lo[0] || x > hi[0] || y < lo[1] || y > hi[1] || lo[2] > zmax {
            continue;
        }
        for t in n.mesh.triangles() {
            let (a, b, c) = (n.mesh.pos[t[0] as usize], n.mesh.pos[t[1] as usize], n.mesh.pos[t[2] as usize]);
            if x < a[0].min(b[0]).min(c[0]) || x > a[0].max(b[0]).max(c[0]) || y < a[1].min(b[1]).min(c[1]) || y > a[1].max(b[1]).max(c[1]) {
                continue;
            }
            let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if d.abs() < 1e-14 {
                continue;
            }
            let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
            let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                if z <= zmax && z > best.0 {
                    best = (z, k_of(&n.name));
                }
            }
        }
    }
    best
}

fn k_of(name: &str) -> String {
    name.to_string()
}
