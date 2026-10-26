//! Placement quality audit of a finished scene (`gen2 --qc OUT.json`; driver: tools/qc_place.sh). Works on the final WORLD meshes of
//! every object (the same triangles that are exported to the glb), not on solver placeholders, and reports per object:
//!   * gap to the visible surface below its contact points (floating > 5 mm / sunk > 5 mm),
//!   * penetration into other objects' meshes (> 1 cm),
//!   * outside its room / through a wall,
//!   * blocking a door (passage box in front of the opening) or a window (box in front of the glass),
//!   * small objects resting on implausible supports (TV, chair seat, ...).
use crate::mesh::{Mesh, Node, Scene, V3};
use rayon::prelude::*;
use serde_json::json;

pub struct DoorInfo {
    pub ci: usize,
    pub desired: f64,
    pub angle: f64,
    pub limiter: String,
}

pub struct QcIn<'a> {
    pub scene: &'a Scene,
    pub state: &'a crate::furn::state::State,
    pub room_polys: &'a [(String, crate::shp::G)],
    pub cutters: &'a [crate::rooms::solidify::Cutter],
    pub doors: &'a [DoorInfo],
    pub wall_t: f64,
    pub cut_keys: &'a [String],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Furn,
    Mess,
    Person,
}

struct Obj {
    key: String,
    class: String,
    kind: Kind,
    nodes: Vec<usize>,
    lo: V3,
    hi: V3,
}

fn tris(m: &Mesh, mut f: impl FnMut(V3, V3, V3)) {
    for fc in &m.faces {
        for k in 1..fc.len().saturating_sub(1) {
            f(m.pos[fc[0] as usize], m.pos[fc[k] as usize], m.pos[fc[k + 1] as usize]);
        }
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// first surface below p (vertical ray, both triangle sides): (z, node index, up-facing unit normal)
pub(crate) fn down_hit(nodes: &[Node], bb: &[(V3, V3)], cand: &[usize], p: V3) -> Option<(f64, usize, V3)> {
    let mut best: Option<(f64, usize, V3)> = None;
    for &ni in cand {
        let (lo, hi) = bb[ni];
        if p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1] || lo[2] > p[2] {
            continue;
        }
        tris(&nodes[ni].mesh, |a, b, c| {
            let n = cross(sub(b, a), sub(c, a));
            if n[2].abs() < 1e-12 {
                return;
            }
            // barycentric in xy
            let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if d.abs() < 1e-15 {
                return;
            }
            let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
            let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
            let l3 = 1.0 - l1 - l2;
            let e = -1e-9;
            if l1 < e || l2 < e || l3 < e {
                return;
            }
            let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
            if z > p[2] + 1e-9 {
                return;
            }
            if best.map_or(true, |b| z > b.0) {
                let l = dot(n, n).sqrt();
                let s = if n[2] < 0.0 { -1.0 / l } else { 1.0 / l };
                best = Some((z, ni, [n[0] * s, n[1] * s, n[2] * s]));
            }
        });
    }
    best
}

fn ray_count(m: &Mesh, o: V3, d: V3) -> usize {
    let mut n = 0;
    tris(m, |a, b, c| {
        let (e1, e2) = (sub(b, a), sub(c, a));
        let h = cross(d, e2);
        let det = dot(e1, h);
        if det.abs() < 1e-14 {
            return;
        }
        let f = 1.0 / det;
        let s = sub(o, a);
        let u = f * dot(s, h);
        if !(0.0..=1.0).contains(&u) {
            return;
        }
        let q = cross(s, e1);
        let v = f * dot(d, q);
        if v < 0.0 || u + v > 1.0 {
            return;
        }
        if f * dot(e2, q) > 0.0 {
            n += 1;
        }
    });
    n
}

/// point inside the (closed) mesh: every axis ray, both ways, crosses an odd number of faces (open cavities stay outside)
fn inside(m: &Mesh, p: V3) -> bool {
    let o = [p[0] + 1.37e-6, p[1] + 2.11e-6, p[2] + 0.83e-6];
    for d in [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
        if ray_count(m, o, d) % 2 == 0 {
            return false;
        }
    }
    true
}

fn pt_tri_dist(p: V3, t: &[V3; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let len = |v: V3| dot(v, v).sqrt();
    if d1 <= 0.0 && d2 <= 0.0 {
        return len(ap);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return len(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return len(sub(p, [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v]));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return len(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return len(sub(p, [a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w]));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return len(sub(p, [b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w]));
    }
    let den = 1.0 / (va + vb + vc);
    let (v, w) = (vb * den, vc * den);
    len(sub(p, [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]))
}

fn mesh_dist(m: &Mesh, p: V3) -> f64 {
    let mut d = f64::MAX;
    tris(m, |a, b, c| d = d.min(pt_tri_dist(p, &[a, b, c])));
    d
}

fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn edge_dist(p: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    let mut best = f64::MAX;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if l2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1]));
    }
    best
}

fn obj_verts<'a>(nodes: &'a [Node], o: &'a Obj) -> impl Iterator<Item = V3> + 'a {
    o.nodes.iter().flat_map(move |&n| nodes[n].mesh.pos.iter().copied())
}

fn is_tableware(c: &str) -> bool {
    ["Bottle", "Can", "Cup", "Wineglass", "Plate", "Bowl", "Jar", "FoodBox", "FoodBag", "Vase", "Pot", "Pan", "Cutlery", "Fork", "Knife", "Spoon", "Mug", "Glass"]
        .iter()
        .any(|k| c.contains(k))
}

fn bad_support(c: &str) -> bool {
    (c.contains("TV") || c.contains("Monitor") || c.contains("Chair") || c.contains("Stool") || c.contains("Sofa") || c.contains("Bed") || c.contains("Toilet") || c.contains("Lamp") || c.contains("Mirror") || c.contains("Sink"))
        && !c.contains("Cabinet")
}

pub fn run(inp: &QcIn, out: &std::path::Path, label: &str) -> serde_json::Value {
    let t0 = std::time::Instant::now();
    let nodes = &inp.scene.nodes;
    let bb: Vec<(V3, V3)> = nodes.iter().map(|n| n.mesh.bounds()).collect();
    // group nodes into objects
    let mut objs: Vec<Obj> = vec![];
    let mut idx: std::collections::HashMap<String, usize> = Default::default();
    let mut structure: Vec<usize> = vec![];
    for (i, n) in nodes.iter().enumerate() {
        let Some((key, _)) = n.name.split_once('#') else {
            structure.push(i);
            continue;
        };
        let kind = if key.starts_with("mess_") { Kind::Mess } else if key.starts_with("person") { Kind::Person } else { Kind::Furn };
        let oi = *idx.entry(key.to_string()).or_insert_with(|| {
            objs.push(Obj { key: key.to_string(), class: n.class.clone(), kind, nodes: vec![], lo: [f64::MAX; 3], hi: [f64::MIN; 3] });
            objs.len() - 1
        });
        let o = &mut objs[oi];
        o.nodes.push(i);
        for d in 0..3 {
            o.lo[d] = o.lo[d].min(bb[i].0[d]);
            o.hi[d] = o.hi[d].max(bb[i].1[d]);
        }
    }
    let node_obj: std::collections::HashMap<usize, usize> = objs.iter().enumerate().flat_map(|(oi, o)| o.nodes.iter().map(move |&n| (n, oi))).collect();
    // rooms: inner polygons
    let inner: Vec<(String, Vec<[f64; 2]>)> = inp.room_polys.iter().map(|(n, g)| (n.clone(), crate::shp::exterior(&crate::shp::buffer_mitre(g, -inp.wall_t / 2.0)))).collect();
    let room_of = |p: [f64; 2]| -> Option<usize> { inner.iter().position(|(_, poly)| poly.len() > 2 && in_poly(p, poly)) };

    // solver relations as plain data (State is not Sync): key -> (parent object keys, keys of objects that are related TO it)
    let parent_of: std::collections::HashMap<String, String> = inp.state.objs.iter().filter_map(|(k, s)| s.relations.iter().find(|r| inp.state.objs.get(&r.target).map_or(false, |t| t.generator.is_some())).map(|r| (k.clone(), r.target.clone()))).collect();
    let children_of: std::collections::HashSet<(String, String)> = inp.state.objs.iter().flat_map(|(k, s)| s.relations.iter().map(move |r| (r.target.clone(), k.clone()))).collect();
    let (cut_keys, cutters, wall_t) = (inp.cut_keys, inp.cutters, inp.wall_t);
    let results: Vec<serde_json::Value> = (0..objs.len())
        .into_par_iter()
        .map(|oi| {
            let o = &objs[oi];
            let mut issues: Vec<String> = vec![];
            let mut m = serde_json::Map::new();
            let ctr = [(o.lo[0] + o.hi[0]) / 2.0, (o.lo[1] + o.hi[1]) / 2.0];
            let room = room_of(ctr);
            m.insert("key".into(), json!(o.key));
            m.insert("class".into(), json!(o.class));
            m.insert("room".into(), json!(room.map(|r| inner[r].0.clone())));
            m.insert("lo".into(), json!(o.lo));
            m.insert("hi".into(), json!(o.hi));
            if o.kind == Kind::Person {
                return serde_json::Value::Object(m);
            }
            let size = [o.hi[0] - o.lo[0], o.hi[1] - o.lo[1], o.hi[2] - o.lo[2]];
            let small = size.iter().cloned().fold(0.0, f64::max) < 0.6;
            // candidates for ray casts: every node of another non-person object plus the structure (floor)
            let cand: Vec<usize> = (0..nodes.len())
                .filter(|&n| {
                    if let Some(&po) = node_obj.get(&n) {
                        po != oi && objs[po].kind != Kind::Person
                    } else {
                        // structure: only floors can support
                        nodes[n].name.ends_with(".floor")
                    }
                })
                .filter(|&n| bb[n].0[0] <= o.hi[0] && bb[n].1[0] >= o.lo[0] && bb[n].0[1] <= o.hi[1] && bb[n].1[1] >= o.lo[1] && bb[n].0[2] <= o.hi[2])
                .collect();
            // ---- gap: lowest vertices (within 1 cm of the object's lowest point), clustered on a 3 cm grid
            let mut seen: std::collections::HashSet<(i64, i64)> = Default::default();
            let mut contacts: Vec<V3> = vec![];
            let mut vs: Vec<V3> = obj_verts(nodes, o).filter(|v| v[2] < o.lo[2] + 0.01).collect();
            vs.sort_by(|a, b| a[2].partial_cmp(&b[2]).unwrap());
            for v in vs {
                if seen.insert(((v[0] / 0.03).floor() as i64, (v[1] / 0.03).floor() as i64)) {
                    contacts.push(v);
                    if contacts.len() >= 24 {
                        break;
                    }
                }
            }
            let mut gap = f64::MAX;
            let mut support: Option<(usize, V3)> = None;
            let mut sup_votes: std::collections::BTreeMap<usize, usize> = Default::default();
            for c in &contacts {
                if let Some((z, ni, nrm)) = down_hit(nodes, &bb, &cand, [c[0], c[1], c[2] + 0.02]) {
                    let g = c[2] - z;
                    if g < gap {
                        gap = g;
                        support = Some((ni, nrm));
                    }
                    if g.abs() < 0.02 {
                        *sup_votes.entry(node_obj.get(&ni).copied().unwrap_or(usize::MAX)).or_default() += 1;
                    }
                }
            }
            let parent = parent_of.get(&o.key).cloned();
            if gap < f64::MAX {
                m.insert("gap".into(), json!((gap * 1e4).round() / 1e4));
                if let Some((ni, nrm)) = support {
                    let sname = node_obj.get(&ni).map(|&s| objs[s].key.clone()).unwrap_or_else(|| nodes[ni].name.clone());
                    m.insert("support".into(), json!(sname));
                    m.insert("support_tilt_deg".into(), json!((nrm[2].clamp(-1.0, 1.0).acos().to_degrees() * 10.0).round() / 10.0));
                }
                let cut = cut_keys.contains(&o.key);
                if gap > 0.005 && (gap < 0.15 || (parent.is_some() && gap < 1.0)) {
                    issues.push(format!("floating {:.1}mm", gap * 1000.0));
                } else if gap < -0.005 && !cut {
                    issues.push(format!("sunk {:.1}mm", -gap * 1000.0));
                }
            }
            if let Some(p) = &parent {
                m.insert("parent".into(), json!(p));
            }
            // ---- implausible support (small items)
            if small && gap < 0.02 {
                if let Some((ni, nrm)) = support {
                    if let Some(&so) = node_obj.get(&ni) {
                        let sc = &objs[so].class;
                        let tilt = nrm[2].clamp(-1.0, 1.0).acos().to_degrees();
                        if is_tableware(&o.class) && bad_support(sc) {
                            issues.push(format!("implausible support {} on {}", o.class, sc));
                        } else if tilt > 25.0 && gap.abs() < 0.01 {
                            issues.push(format!("resting on a {:.0} deg slope of {}", tilt, sc));
                        }
                    }
                }
            }
            // ---- penetration into other objects
            let mut samples: Vec<V3> = vec![];
            let nv: usize = o.nodes.iter().map(|&n| nodes[n].mesh.pos.len()).sum();
            let stride = (nv / 150).max(1);
            for (k, v) in obj_verts(nodes, o).enumerate() {
                if k % stride == 0 {
                    samples.push(v);
                }
            }
            let nf: usize = o.nodes.iter().map(|&n| nodes[n].mesh.faces.len()).sum();
            let fstride = (nf / 150).max(1);
            let mut fk = 0;
            for &n in &o.nodes {
                for f in &nodes[n].mesh.faces {
                    if fk % fstride == 0 {
                        let c: V3 = [0, 1, 2].map(|d| f.iter().map(|&i| nodes[n].mesh.pos[i as usize][d]).sum::<f64>() / f.len() as f64);
                        samples.push(c);
                    }
                    fk += 1;
                }
            }
            let mut worst: Option<(f64, usize)> = None;
            for other in 0..objs.len() {
                if other == oi || objs[other].kind == Kind::Person {
                    continue;
                }
                let q = &objs[other];
                if (0..3).any(|d| q.lo[d] > o.hi[d] - 0.01 || q.hi[d] < o.lo[d] + 0.01) {
                    continue;
                }
                // sinks are cut into their counters by design
                if (cut_keys.contains(&o.key) && parent.as_deref() == Some(q.key.as_str())) || (cut_keys.contains(&q.key) && children_of.contains(&(o.key.clone(), q.key.clone()))) {
                    continue;
                }
                for &n in &q.nodes {
                    let (nl, nh) = bb[n];
                    let e = 0.01;
                    for s in &samples {
                        if (0..3).any(|d| s[d] < nl[d] + e || s[d] > nh[d] - e) {
                            continue;
                        }
                        if inside(&nodes[n].mesh, *s) {
                            let d = mesh_dist(&nodes[n].mesh, *s);
                            if d > 0.01 && worst.map_or(true, |w| d > w.0) {
                                worst = Some((d, other));
                            }
                        }
                    }
                }
            }
            if let Some((d, other)) = worst {
                m.insert("pen".into(), json!((d * 1e4).round() / 1e4));
                m.insert("pen_with".into(), json!(objs[other].key));
                issues.push(format!("penetrates {} by {:.1}cm", objs[other].key, d * 100.0));
            }
            // ---- outside own room / through the wall
            if o.kind != Kind::Person {
                match room {
                    None => {
                        issues.push("outside every room".to_string());
                    }
                    Some(r) => {
                        let poly = &inner[r].1;
                        let mut out = 0.0f64;
                        for v in obj_verts(nodes, o) {
                            if v[2] > 2.6 {
                                continue;
                            }
                            if !in_poly([v[0], v[1]], poly) {
                                out = out.max(edge_dist([v[0], v[1]], poly));
                            }
                        }
                        if out > 0.02 {
                            m.insert("outside_by".into(), json!((out * 1e4).round() / 1e4));
                            issues.push(format!("through wall by {:.1}cm", out * 100.0));
                        }
                    }
                }
            }
            // ---- doors / windows
            for (ci, ct) in cutters.iter().enumerate() {
                use crate::rooms::solidify::CutKind;
                let (c, s) = (ct.rot_z.cos(), ct.rot_z.sin());
                let to_local = |p: V3| -> V3 {
                    let (dx, dy) = (p[0] - ct.loc[0], p[1] - ct.loc[1]);
                    [c * dx + s * dy, -s * dx + c * dy, p[2] - ct.loc[2]]
                };
                if (ct.loc[0] - ctr[0]).hypot(ct.loc[1] - ctr[1]) > 4.0 {
                    continue;
                }
                let (hw, depth) = (ct.dims[0] / 2.0, if matches!(ct.kind, CutKind::Window) { 0.12 + wall_t / 2.0 } else { 0.6 });
                let (zlo, zhi) = match ct.kind {
                    CutKind::Window => (-ct.dims[2] / 2.0, ct.dims[2] / 2.0),
                    _ => (-ct.dims[2] / 2.0 + 0.05, ct.dims[2] / 2.0 - 0.1),
                };
                let mut hit: f64 = 0.0;
                let mut check = |p: V3| {
                    let l = to_local(p);
                    // wall slab itself (|y| < t/2 + eps) is part of the opening
                    if l[0].abs() < hw - 0.02 && l[1].abs() < depth && l[2] > zlo && l[2] < zhi {
                        hit = hit.max(depth - l[1].abs());
                    }
                };
                for &n in &o.nodes {
                    let mesh = &nodes[n].mesh;
                    for v in &mesh.pos {
                        check(*v);
                    }
                    for f in &mesh.faces {
                        let cc: V3 = [0, 1, 2].map(|d| f.iter().map(|&i| mesh.pos[i as usize][d]).sum::<f64>() / f.len() as f64);
                        check(cc);
                    }
                }
                if hit > 0.0 {
                    let which = match ct.kind {
                        CutKind::Window => "blocks window",
                        _ => "blocks door passage",
                    };
                    issues.push(format!("{which} (cutter {ci}, {:.0}cm into the zone)", hit * 100.0));
                    m.insert(format!("blocks_{ci}"), json!(true));
                }
            }
            m.insert("issues".into(), json!(issues));
            serde_json::Value::Object(m)
        })
        .collect();

    // door swing limits
    let swing: Vec<serde_json::Value> = inp
        .doors
        .iter()
        .filter(|d| d.desired - d.angle > 0.35 && d.limiter != "none")
        .map(|d| json!({"cutter": d.ci, "desired_deg": d.desired.to_degrees().round(), "final_deg": d.angle.to_degrees().round(), "limiter": d.limiter}))
        .collect();

    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &results {
        for i in r["issues"].as_array().into_iter().flatten() {
            let s = i.as_str().unwrap();
            let cls = ["floating", "sunk", "penetrates", "through wall", "outside every room", "blocks door", "blocks window", "implausible", "resting on"].iter().find(|k| s.starts_with(*k)).copied().unwrap_or("other");
            *counts.entry(cls.to_string()).or_default() += 1;
        }
    }
    let rep = json!({"scene": label, "objects": results.len(), "counts": counts, "door_swing_limited": swing, "results": results, "qc_secs": t0.elapsed().as_secs_f64()});
    std::fs::write(out, serde_json::to_vec_pretty(&rep).unwrap()).unwrap();
    for r in &results {
        for i in r["issues"].as_array().into_iter().flatten() {
            eprintln!("QC {label} {} {} [{}]: {}", r["key"].as_str().unwrap_or(""), r["class"].as_str().unwrap_or(""), r["room"].as_str().unwrap_or("?"), i.as_str().unwrap());
        }
    }
    eprintln!("QC {label} objects={} counts={:?} swing_limited={} in {:?}", results.len(), counts, swing.len(), t0.elapsed());
    rep
}
