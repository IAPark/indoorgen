//! States that edit the built visual assets (called through `states::post_visuals`).
use crate::states::V3;
use crate::MessCfg;
use ig_furn::state::State;
use ig_kit::api::Asset;
use std::collections::HashMap;

/// connected components (by shared vertex index) of a mesh: returns component id per vertex
pub fn components(m: &ig_core::mesh::Mesh) -> (Vec<usize>, usize) {
    let n = m.pos.len();
    let mut p: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for f in &m.faces {
        for w in f.windows(2) {
            let (a, b) = (find(&mut p, w[0] as usize), find(&mut p, w[1] as usize));
            if a != b {
                p[a] = b;
            }
        }
    }
    let mut ids: HashMap<usize, usize> = HashMap::new();
    let mut out = vec![0; n];
    for v in 0..n {
        let r = find(&mut p, v);
        let l = ids.len();
        out[v] = *ids.entry(r).or_insert(l);
    }
    (out, ids.len())
}

pub fn dump(name: &str, class: &str, a: &Asset) {
    eprintln!("ASSET {name} {class} parts={}", a.parts.len());
    for (pi, p) in a.parts.iter().enumerate() {
        let (c, n) = components(&p.mesh);
        let mut bb = vec![([f64::MAX; 3], [f64::MIN; 3], 0usize); n];
        for (v, q) in p.mesh.pos.iter().enumerate() {
            let b = &mut bb[c[v]];
            b.2 += 1;
            for k in 0..3 {
                b.0[k] = b.0[k].min(q[k]);
                b.1[k] = b.1[k].max(q[k]);
            }
        }
        eprintln!("  part {pi} verts={} comps={}", p.mesh.pos.len(), n);
        for (i, b) in bb.iter().enumerate().take(40) {
            eprintln!("    c{i} n={} lo={:.3?} hi={:.3?}", b.2, b.0, b.1);
        }
    }
}

pub fn post_visuals(visuals: &mut HashMap<String, Asset>, state: &State, cfg: &MessCfg) {
    let _: Option<V3> = None;
    if std::env::var("MESS_DUMP_ASSETS").is_ok() {
        for (k, a) in visuals.iter() {
            if !state.objs.contains_key(k) { continue; }
            let cls = state.objs[k].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
            if cls.contains("Cabinet") || cls.contains("KitchenSpace") {
                dump(k, &cls, a);
            }
        }
    }
    let lvl = cfg.level.clamp(0.0, 1.0);
    let mut rep = crate::states::last_report().unwrap_or_default();
    let mut keys: Vec<String> = visuals.keys().cloned().collect();
    keys.sort();
    let mut rng = cfg.rng("states.beds");
    let mut drng = cfg.rng("states.cabinets");
    let mut xf_hint: Option<f64>;
    for k in &keys {
        xf_hint = None;
        if !state.objs.contains_key(k) { continue; } // removed after solving (snap/QC prune)
        let cls = state.objs[k].generator.as_ref().map(|g| g.class.clone()).unwrap_or_default();
        if matches!(cls.as_str(), "KitchenCabinetFactory" | "KitchenSpaceFactory") {
            let clear = { let (lo, hi) = asset_bounds(&visuals[k]); front_clearance(state, k, lo, hi) };
            xf_hint = front_plane(&visuals[k]);
            let g = state.objs[k].generator.clone().unwrap();
            if clear > 0.04 {
                if let Some(f) = ig_assets::make_factory(&g.class, g.factory_seed) {
                    ig_storage::cabdoor::set_drawer_pull(Some((lvl, cfg.seed ^ g.factory_seed ^ g.inst_seed.wrapping_mul(7919), clear - 0.02)));
                    ig_kit::set_unseeded(g.unseeded.clone());
                    let na = ig_kit::api::scene_asset(&*f, g.inst_seed);
                    ig_kit::set_unseeded(None);
                    ig_storage::cabdoor::set_drawer_pull(None);
                    let moved_parts = na.parts.iter().zip(&visuals[k].parts).filter(|(a, b)| a.mesh.pos != b.mesh.pos).count();
                    *rep.candidates.entry("drawer_cabinet".into()).or_default() += 1;
                    if moved_parts > 0 && na.parts.len() == visuals[k].parts.len() {
                        *rep.changed.entry("drawer_cabinet".into()).or_default() += 1;
                        let mut na = na;
                        keep_offset(&visuals[k], &mut na);
                        visuals.insert(k.clone(), na);
                    }
                }
            }
        }
        if !matches!(cls.as_str(), "KitchenCabinetFactory" | "KitchenSpaceFactory") {
            xf_hint = None;
        }
        if matches!(cls.as_str(), "SingleCabinetFactory" | "KitchenCabinetFactory" | "KitchenSpaceFactory") {
            let (n, tot) = ajar_doors(visuals.get_mut(k).unwrap(), state, k, lvl, &mut drng, xf_hint);
            *rep.candidates.entry("cabinet_door".into()).or_default() += tot;
            *rep.changed.entry("cabinet_door".into()).or_default() += n;
        }
        if cls == "BedFactory" {
            let changed = unmake_bed(visuals.get_mut(k).unwrap(), lvl, &mut rng);
            *rep.candidates.entry("bed".into()).or_default() += 1;
            if changed {
                *rep.changed.entry("bed".into()).or_default() += 1;
            }
        }
    }
    rep.print();
}

/// Unmade bed: the sheet / comforter is pulled toward the foot (and slightly askew), the cover thrown crooked. Edits the cloth jobs (Blender
/// re-simulates them with the new placement) and the approximate part meshes the same way.
fn unmake_bed(a: &mut Asset, lvl: f64, rng: &mut ig_core::rng::Rng) -> bool {
    use ig_core::mesh::Mat4;
    let jobs: Vec<usize> = (0..a.cloth.len()).filter(|&i| a.cloth[i].build.is_some()).collect();
    if jobs.is_empty() {
        return false;
    }
    let mut any = false;
    for (n, ji) in jobs.iter().enumerate() {
        // sheet (n = 0): pulled back 10-45 cm toward the foot; cover (n = 1): moved along and across by a smaller amount
        let (u1, u2, u3, u4) = (rng.f64(), rng.f64(), rng.f64(), rng.f64());
        let (dx, dy, yaw) = if n == 0 {
            (lvl * (0.1 + 0.35 * u1), lvl * (u2 * 2.0 - 1.0) * 0.08, lvl * (u3 * 2.0 - 1.0) * 6f64.to_radians())
        } else {
            (lvl * (u1 * 2.0 - 0.6) * 0.3, lvl * (u2 * 2.0 - 1.0) * 0.15, lvl * (u3 * 2.0 - 1.0) * 14f64.to_radians())
        };
        let _ = u4;
        let target = a.cloth[*ji].target;
        let t = Mat4::translation([dx, dy, 0.0]);
        let job = &mut a.cloth[*ji];
        job.pre.transform(&t);
        if let Some(b) = job.build.as_mut() {
            let x = b["x"].as_f64().unwrap_or(0.0);
            b["x"] = serde_json::json!(x + dx);
            b["dy"] = serde_json::json!(dy);
            b["yaw"] = serde_json::json!(yaw);
        }
        if let Some(p) = a.parts.get_mut(target) {
            p.mesh.transform(&t);
        }
        any = true;
    }
    any
}

struct Comp {
    part: usize,
    id: usize,
    lo: V3,
    hi: V3,
}

/// Cabinet doors ajar: front-layer pieces (the door panels sit in a thin layer at the cabinet front, local +x) are clustered into doors and
/// rotated about the vertical hinge edge. Returns (doors opened, doors found).
fn ajar_doors(a: &mut Asset, state: &State, name: &str, lvl: f64, rng: &mut ig_core::rng::Rng, xf_hint: Option<f64>) -> (usize, usize) {
    use parry3d_f64::math::{Isometry, Point};
    use parry3d_f64::query;
    use parry3d_f64::shape::SharedShape;
    let mut comps: Vec<Comp> = vec![];
    let mut cid: Vec<Vec<usize>> = vec![];
    for (pi, p) in a.parts.iter().enumerate() {
        let (c, n) = components(&p.mesh);
        let mut bb = vec![([f64::MAX; 3], [f64::MIN; 3]); n];
        for (v, q) in p.mesh.pos.iter().enumerate() {
            let b = &mut bb[c[v]];
            for k in 0..3 {
                b.0[k] = b.0[k].min(q[k]);
                b.1[k] = b.1[k].max(q[k]);
            }
        }
        for (i, b) in bb.into_iter().enumerate() {
            comps.push(Comp { part: pi, id: i, lo: b.0, hi: b.1 });
        }
        cid.push(c);
    }
    // front plane: panel-like pieces (thin in x, large in y and z)
    let Some(xf) = xf_hint.or_else(|| front_plane(a)) else { return (0, 0) };
    let front: Vec<usize> = (0..comps.len()).filter(|&i| comps[i].lo[0] >= xf - 0.006 && comps[i].lo[0] <= xf + 0.006).collect();
    // cluster by (y, z) rectangle overlap (shrunk by 3 mm so neighbouring doors stay apart)
    let mut par: Vec<usize> = (0..front.len()).collect();
    fn find(p: &mut Vec<usize>, mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for i in 0..front.len() {
        for j in i + 1..front.len() {
            let (a_, b_) = (&comps[front[i]], &comps[front[j]]);
            let ov = |k: usize| a_.lo[k] + 0.003 < b_.hi[k] - 0.003 && b_.lo[k] + 0.003 < a_.hi[k] - 0.003;
            if ov(1) && ov(2) {
                let (ri, rj) = (find(&mut par, i), find(&mut par, j));
                par[ri] = rj;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..front.len() {
        let r = find(&mut par, i);
        groups.entry(r).or_default().push(front[i]);
    }
    struct Door {
        members: Vec<usize>,
        y0: f64,
        y1: f64,
        z0: f64,
        z1: f64,
        x0: f64,
        x1: f64,
    }
    let mut doors: Vec<Door> = vec![];
    for (_, m) in groups {
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &i in &m {
            for k in 0..3 {
                lo[k] = lo[k].min(comps[i].lo[k]);
                hi[k] = hi[k].max(comps[i].hi[k]);
            }
        }
        let (w, h) = (hi[1] - lo[1], hi[2] - lo[2]);
        // doors: tall enough and not drawer-fronts (wide and low)
        if (0.12..=1.0).contains(&w) && h >= 0.3 && h > 0.6 * w && m.len() >= 3 {
            doors.push(Door { members: m, y0: lo[1], y1: hi[1], z0: lo[2], z1: hi[2], x0: lo[0], x1: hi[0] });
        }
    }
    if std::env::var("MESS_DUMP_DOORS").is_ok() {
        for d in &doors {
            eprintln!("DOOR {name} xf={xf:.3} y={:.3}..{:.3} z={:.3}..{:.3} x={:.3}..{:.3} members={}", d.y0, d.y1, d.z0, d.z1, d.x0, d.x1, d.members.len());
        }
    }
    doors.sort_by(|p, q| p.y0.partial_cmp(&q.y0).unwrap());
    let total = doors.len();
    let obj = &state.objs[name];
    let others: Vec<String> = state.objs.iter().filter(|(k, o)| k.as_str() != name && (o.generator.is_some() || o.polygon.is_some())).map(|(k, _)| k.clone()).collect();
    let mut opened = 0;
    for di in 0..total {
        let (u_hit, u_ang, u_side) = (rng.f64(), rng.f64(), rng.f64());
        let d = &doors[di];
        if u_hit >= 0.25 + 0.65 * lvl {
            continue;
        }
        let nb_lo = di > 0 && (d.y0 - doors[di - 1].y1).abs() < 0.03;
        let nb_hi = di + 1 < total && (doors[di + 1].y0 - d.y1).abs() < 0.03;
        let hinge_lo = match (nb_lo, nb_hi) {
            (false, true) => true,
            (true, false) => false,
            _ => u_side < 0.5,
        };
        let hy = if hinge_lo { d.y0 } else { d.y1 };
        let hx = d.x0;
        let theta = lvl * (4.0 + 36.0 * u_ang) * std::f64::consts::PI / 180.0;
        let phi = if hinge_lo { -theta } else { theta };
        // collision of the swept door volume (hull of the door box at 0 and at theta) with every other object's proxies
        let rotate = |p: V3, ph: f64| -> V3 {
            let (s, c) = ph.sin_cos();
            let (dx, dy) = (p[0] - hx, p[1] - hy);
            [hx + dx * c - dy * s, hy + dx * s + dy * c, p[2]]
        };
        let mut pts: Vec<Point<f64>> = vec![];
        for &ph in &[0.0, phi * 0.5, phi] {
            for &x in &[d.x0, d.x1 + 0.05] {
                for &y in &[d.y0, d.y1] {
                    for &z in &[d.z0 + 0.01, d.z1 - 0.01] {
                        let q = obj.pose.apply(rotate([x, y, z], ph));
                        pts.push(Point::new(q[0], q[1], q[2]));
                    }
                }
            }
        }
        let Some(hull) = SharedShape::convex_hull(&pts) else { continue };
        let mut hit = false;
        for o in &others {
            for px in ig_furn::col::proxies(state, o).iter() {
                if query::intersection_test(&Isometry::identity(), hull.as_ref(), &px.iso, px.shape.as_ref()).unwrap_or(false) {
                    hit = true;
                    break;
                }
            }
            if hit {
                break;
            }
        }
        if hit {
            continue;
        }
        for &mi in &d.members {
            let c = &comps[mi];
            for (v, q) in a.parts[c.part].mesh.pos.iter_mut().enumerate() {
                if cid[c.part][v] == c.id {
                    *q = rotate(*q, phi);
                }
            }
        }
        opened += 1;
    }
    (opened, total)
}

/// free depth in front of an object (local +x), up to 0.3 m, tested with a slab against every other object's collision proxies
fn front_clearance(state: &State, name: &str, lo: V3, hi: V3) -> f64 {
    use parry3d_f64::math::{Isometry, Point};
    use parry3d_f64::query;
    use parry3d_f64::shape::SharedShape;
    let o = &state.objs[name];
    for &c in &[0.30, 0.22, 0.15, 0.08, 0.04] {
        let mut pts = vec![];
        for &x in &[hi[0] + 0.005, hi[0] + c] {
            for &y in &[lo[1], hi[1]] {
                for &z in &[0.1, (hi[2] - 0.05).max(0.2)] {
                    let q = o.pose.apply([x, y, z]);
                    pts.push(Point::new(q[0], q[1], q[2]));
                }
            }
        }
        let Some(hull) = SharedShape::convex_hull(&pts) else { continue };
        let hit = state.objs.iter().filter(|(k, p)| k.as_str() != name && (p.generator.is_some() || p.polygon.is_some())).any(|(k, _)| {
            ig_furn::col::proxies(state, k).iter().any(|px| query::intersection_test(&Isometry::identity(), hull.as_ref(), &px.iso, px.shape.as_ref()).unwrap_or(false))
        });
        if !hit {
            return c;
        }
    }
    0.0
}

/// local x of the plane of the frontmost door-like panels (thin in x, large in y and z)
fn front_plane(a: &Asset) -> Option<f64> {
    let mut best: Option<f64> = None;
    for p in &a.parts {
        let (c, n) = components(&p.mesh);
        let mut bb = vec![([f64::MAX; 3], [f64::MIN; 3]); n];
        for (v, q) in p.mesh.pos.iter().enumerate() {
            let b = &mut bb[c[v]];
            for k in 0..3 {
                b.0[k] = b.0[k].min(q[k]);
                b.1[k] = b.1[k].max(q[k]);
            }
        }
        for b in bb {
            if b.1[0] - b.0[0] < 0.03 && b.1[1] - b.0[1] > 0.1 && b.1[2] - b.0[2] > 0.1 {
                best = Some(best.map_or(b.0[0], |m: f64| m.max(b.0[0])));
            }
        }
    }
    best
}

fn asset_bounds(a: &Asset) -> (V3, V3) {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in &a.parts {
        let (l, h) = p.mesh.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(l[k]);
            hi[k] = hi[k].max(h[k]);
        }
    }
    (lo, hi)
}

/// The old visual may have been shifted after it was built (snap::recenter_visuals moves KitchenCabinet visuals onto their solver box).
/// A rebuilt asset (drawers pulled out) comes back unshifted: find the constant offset between the two on a part that did not move
/// (same vertex count, every vertex displaced by the same vector) and apply it to the rebuilt asset.
fn keep_offset(old: &ig_kit::api::Asset, na: &mut ig_kit::api::Asset) {
    // rigid offsets of all parts; pulled drawers move too, so take the most common one (the carcass, shelves, top, ...)
    let mut offs: Vec<([f64; 3], usize)> = vec![];
    for (a, b) in old.parts.iter().zip(&na.parts) {
        if a.mesh.pos.len() != b.mesh.pos.len() || a.mesh.pos.is_empty() {
            continue;
        }
        let c = [a.mesh.pos[0][0] - b.mesh.pos[0][0], a.mesh.pos[0][1] - b.mesh.pos[0][1], a.mesh.pos[0][2] - b.mesh.pos[0][2]];
        if !a.mesh.pos.iter().zip(&b.mesh.pos).all(|(p, q)| (0..3).all(|k| (p[k] - q[k] - c[k]).abs() < 1e-6)) {
            continue;
        }
        match offs.iter_mut().find(|(o, _)| (0..3).all(|k| (o[k] - c[k]).abs() < 1e-5)) {
            Some(e) => e.1 += 1,
            None => offs.push((c, 1)),
        }
    }
    let d = offs.iter().max_by_key(|(_, n)| *n).map(|(o, _)| *o);
    let Some(d) = d else { return };
    if d.iter().all(|x| x.abs() < 1e-9) {
        return;
    }
    let sh = |v: &mut [f64; 3]| for k in 0..3 { v[k] += d[k]; };
    for p in na.parts.iter_mut() { p.mesh.pos.iter_mut().for_each(sh); }
    for c in na.cutters.iter_mut() { c.pos.iter_mut().for_each(sh); }
    for s in na.supports.iter_mut() { sh(&mut s.0); sh(&mut s.1); }
}
