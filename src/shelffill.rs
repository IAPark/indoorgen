//! Shelf fill: bookcases and built-in bookshelf walls fill some of their own bays with rows of books once the solve and the snap passes are
//! done. Each board's free length (beside whatever the solver already put there, between dividers) and its clearance to the board above
//! are measured on the final meshes, and rows of standing books are built to fit ([`crate::assets::books::fit_column`]) instead of being
//! searched for by the solver, which could rarely fit a row into a bay. Every row is its own object (class BookColumnFactory, parent =
//! the bookcase) in the scene and the sidecar, so it can be labelled or removed like any solver-placed item.
use crate::assets::Asset;
use crate::furn::geom::Pose;
use crate::furn::relations::Relation;
use crate::furn::state::{GenRef, ObjState, RelState, State};
use crate::furn::tags::{Tag, TagSet};
use crate::rng::Rng;
use std::collections::HashMap;
use std::sync::Arc;

type V3 = [f64; 3];

/// shelf classes whose boards are filled, the room types they are filled in (None: any) and the axis that points out of the unit
/// (0: +x, Infinigen furniture; 1: +y, room features in their wall-span frame)
const UNITS: &[(&str, Option<&[&str]>, usize)] = &[
    ("BookshelfWallFeature", None, 1),
    ("SimpleBookcaseFactory", Some(&["living-room", "bedroom", "dining-room", "closet", "utility"]), 0),
    ("LargeShelfFactory", Some(&["living-room", "bedroom", "closet", "utility"]), 0),
    ("CellShelfFactory", Some(&["living-room", "bedroom", "closet", "utility"]), 0),
];

fn mat_of(p: &Pose) -> [[f64; 4]; 4] {
    [[p.r[0][0], p.r[0][1], p.r[0][2], p.t[0]], [p.r[1][0], p.r[1][1], p.r[1][2], p.t[1]], [p.r[2][0], p.r[2][1], p.r[2][2], p.t[2]], [0.0, 0.0, 0.0, 1.0]]
}

/// world point -> unit-local (rigid pose)
fn to_local(p: &Pose, w: V3) -> V3 {
    let d = [w[0] - p.t[0], w[1] - p.t[1], w[2] - p.t[2]];
    [0, 1, 2].map(|i| p.r[0][i] * d[0] + p.r[1][i] * d[1] + p.r[2][i] * d[2])
}

/// subtract interval `b` from every interval of `v`
fn cut(v: &mut Vec<[f64; 2]>, b: [f64; 2]) {
    let mut out = vec![];
    for a in v.drain(..) {
        if b[1] <= a[0] || b[0] >= a[1] { out.push(a); continue }
        if b[0] > a[0] { out.push([a[0], b[0]]) }
        if b[1] < a[1] { out.push([b[1], a[1]]) }
    }
    *v = out;
}


/// shelf decor (the shelf dresser): vases, small plants, trinket sets, bowls, jars between and beside the book rows
const SHELF_DECOR: &[&str] = &["VaseFactory", "VaseFactory", "PlantContainerFactory", "NatureShelfTrinketsFactory", "BowlFactory", "JarFactory"];

/// a decor item scaled to fit `w` x `d` x `h`, re-centred so its footprint centre is the origin and its bottom z = 0; None if it would
/// have to shrink below 45 %
fn decor_fit(assets: &crate::assets::RealAssets, rng: &mut Rng, w: f64, d: f64, h: f64) -> Option<(String, u64, Asset)> {
    for _ in 0..5 {
        let cls = SHELF_DECOR[rng.randint(0, SHELF_DECOR.len() as i64) as usize];
        let fs = rng.next_u64() % 1_000_000;
        let mut a = assets.visual(cls, fs, 0);
        let (lo, hi) = a.bounds();
        let (dx, dy, dz) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        if dx <= 0.0 || dz <= 0.0 { continue }
        let sc = (w * 0.9 / dx.max(dy)).min(d * 0.9 / dx.max(dy)).min(h / dz).min(1.0);
        if sc < 0.45 { continue }
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]];
        for p in a.parts.iter_mut() { for q in p.mesh.pos.iter_mut() { *q = [(q[0] - c[0]) * sc, (q[1] - c[1]) * sc, (q[2] - c[2]) * sc]; } }
        return Some((cls.to_string(), fs, a));
    }
    None
}

/// add a decor object standing at unit-local `at`
fn add_decor(state: &mut State, visuals: &mut HashMap<String, Asset>, up: &Pose, key: &str, room: &str, name: String, cls: String, fs: u64, a: Asset, at: V3, yaw: f64) {
    let local = Pose::from_euler(at, [0.0, 0.0, yaw]);
    let um = mat_of(up);
    let lm = mat_of(&local);
    let mut wm = [[0.0; 4]; 4];
    for i in 0..4 { for j in 0..4 { wm[i][j] = (0..4).map(|k| um[i][k] * lm[k][j]).sum() } }
    let tags = TagSet::from([Tag::sem("Object"), Tag::sem("OfficeShelfItem"), Tag::gen(&cls), Tag::obj(&name)]);
    let mut o = ObjState::new(Arc::new(a.solver_geom()), Pose::from_matrix(wm), tags);
    o.generator = Some(GenRef { class: cls, factory_seed: fs, inst_seed: 0, unseeded: None, fit_box: None, size: None, join: None, piece: None });
    o.relations.push(RelState { relation: Relation::Any, target: key.to_string(), child_plane: 0, parent_plane: 0 });
    o.relations.push(RelState { relation: Relation::Any, target: room.to_string(), child_plane: 0, parent_plane: 0 });
    state.objs.insert(name.clone(), o);
    state.touch(&name);
    visuals.insert(name, a);
}

/// Fill the shelves. `share`: mean share of the boards that get books (each unit draws its own share around it); 0 = off.
/// Returns the keys of the rows added (already in `state` and `visuals`).
pub fn fill(state: &mut State, visuals: &mut HashMap<String, Asset>, seed: u64, share: f64, assets: Option<&crate::assets::RealAssets>) -> Vec<String> {
    if share <= 0.0 { return vec![] }
    let mut units: Vec<(String, usize, String)> = vec![];
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let Some((_, rooms, front)) = UNITS.iter().find(|u| u.0 == g.class) else { continue };
        let Some(room) = o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.get(t).map_or(false, |x| x.polygon.is_some())) else { continue };
        if let Some(rs) = rooms { if !rs.contains(&crate::rooms::state::room_type(&room)) { continue } }
        if visuals.contains_key(k) { units.push((k.clone(), *front, room)) }
    }
    units.sort();
    let mut added = vec![];
    for (key, front, room) in units {
        let mut rng = Rng::seeded(crate::int_hash(&[&seed.to_string(), "shelffill", &key]));
        let p_fill = (share + rng.uniform(-0.25, 0.25)).clamp(0.0, 1.0);
        let up = state.objs[&key].pose.clone();
        let asset = &visuals[&key];
        let sups = asset.supports.clone();
        // every face of the unit (local frame) as a box: dividers / back panels / the board above are found per bay below
        let faces: Vec<(V3, V3)> = asset.parts.iter().flat_map(|p| p.mesh.faces.iter().map(move |f| {
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for &i in f { let q = p.mesh.pos[i as usize]; for d in 0..3 { lo[d] = lo[d].min(q[d]); hi[d] = hi[d].max(q[d]); } }
            (lo, hi)
        })).collect();
        // what the solver already put on this unit, as local boxes
        let mut kids: Vec<(V3, V3)> = state.objs.iter().filter(|(_, o)| o.relations.iter().any(|r| r.target == key)).filter_map(|(k, o)| {
            let a = visuals.get(k)?;
            let (lo, hi) = a.bounds();
            let (mut l, mut h) = ([f64::MAX; 3], [f64::MIN; 3]);
            for i in 0..8 {
                let c = [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
                let q = to_local(&up, o.pose.apply(c));
                for d in 0..3 { l[d] = l[d].min(q[d]); h[d] = h[d].max(q[d]); }
            }
            Some((l, h))
        }).collect();
        let (la, da) = (1 - front, front); // long axis along the boards, depth axis out of the unit
        let mut n = 0usize;
        for s in &sups {
            let top = s.1[2];
            let (a0, a1) = (s.0[la] + 0.01, s.1[la] - 0.01);
            let (d0, d1) = (s.0[da], s.1[da]);
            if a1 - a0 < 0.12 || d1 - d0 < 0.12 { continue }
            // clearance: the lowest face above this board that overlaps it (the next board, the top)
            let mid = [d0 + (d1 - d0) * 0.25, d1 - (d1 - d0) * 0.25];
            let above = faces.iter().filter(|(lo, hi)| lo[2] > top + 0.02 && lo[la] < a1 && hi[la] > a0 && lo[da] < mid[1] && hi[da] > mid[0] && hi[2] - lo[2] < 0.03)
                .map(|(lo, _)| lo[2]).fold(f64::MAX, f64::min);
            if above == f64::MAX {
                // the unit's top (below 1.9 m): one decor piece, toward one end
                if let (Some(ast), true) = (assets, rng.f64() < 0.55) {
                    let wt = up.apply([0.0, 0.0, top])[2];
                    if wt < 1.9 {
                        if let Some((cls, fs, a)) = decor_fit(ast, &mut rng, 0.3, d1 - d0, 0.45) {
                            let x = if rng.f64() < 0.5 { a0 + 0.18 } else { a1 - 0.18 };
                            // (clear of what already stands on the top: a bowl went into a desk lamp's base)
                            let busy = |x: f64| kids.iter().any(|(lo, hi)| hi[2] > top - 0.01 && lo[la] < x + 0.2 && hi[la] > x - 0.2);
                            let x = if !busy(x) { x } else if !busy(a0 + a1 - x) { a0 + a1 - x } else { f64::NAN };
                            if x.is_nan() { continue }
                            // (not up into a window over the unit: a vase stood 9 cm into the glass's zone)
                            let (blo, bhi) = a.bounds();
                            let c0 = up.apply({ let mut q = [0.0; 3]; q[la] = x - 0.15; q[da] = d0; q[2] = top; q });
                            let c1 = up.apply({ let mut q = [0.0; 3]; q[la] = x + 0.15; q[da] = d1 + 0.2; q[2] = top + bhi[2] - blo[2]; q });
                            let (wl, wh) = ([c0[0].min(c1[0]), c0[1].min(c1[1]), c0[2]], [c0[0].max(c1[0]), c0[1].max(c1[1]), c1[2]]);
                            let at_window = state.objs.iter().any(|(wk, w)| wk.starts_with("window") && w.generator.is_none() && {
                                let (ol, oh) = crate::furn::layout2d::lohi_world(state, wk);
                                (0..3).all(|d| wl[d] < oh[d] + 0.2 && ol[d] - 0.2 < wh[d]) });
                            if at_window { continue }
                            let mut at = [0.0; 3]; at[la] = x; at[da] = (d0 + d1) / 2.0; at[2] = top;
                            let name = format!("{key}.decor{n}"); n += 1;
                            add_decor(state, visuals, &up, &key, &room, name.clone(), cls, fs, a, at, rng.uniform(-0.4, 0.4));
                            added.push(name);
                        }
                    }
                }
                continue
            }
            let clear = above - top - 0.015;
            if clear < 0.15 { continue }
            let books = rng.f64() < p_fill;
            // boards without books: half get a small styled group (1-2 decor pieces), the rest stay empty
            if !books && (assets.is_none() || rng.f64() < 0.5) { continue }
            // free length: minus dividers (faces crossing the bay's slab) and the solver's items on this board
            let slab = (top + 0.01, top + clear);
            let mut free = vec![[a0, a1]];
            for (lo, hi) in &faces {
                if lo[2] < slab.1 && hi[2] > slab.0 && lo[da] < mid[1] && hi[da] > mid[0] && hi[la] - lo[la] < 0.06 { cut(&mut free, [lo[la] - 0.005, hi[la] + 0.005]) }
            }
            for (lo, hi) in &kids {
                if lo[2] < slab.1 && hi[2] > top - 0.01 && lo[da] < d1 && hi[da] > d0 { cut(&mut free, [lo[la] - 0.02, hi[la] + 0.02]) }
            }
            let depth = (d1 - d0 - 0.03).min(0.32);
            if let Some(ast) = assets {
                // decor: on a book board one piece carved off an end of the widest bay; a board without books gets 1-2 pieces
                let widest = free.iter().cloned().max_by(|a, b| (a[1] - a[0]).partial_cmp(&(b[1] - b[0])).unwrap());
                if let Some(iv) = widest {
                    let n_dec = if books { if rng.f64() < 0.5 { 1 } else { 0 } } else { rng.randint(1, 3) as usize };
                    let span = iv[1] - iv[0];
                    for j in 0..n_dec {
                        let w = if books { rng.uniform(0.16, 0.26).min(span * 0.4) } else { (span / n_dec as f64).min(0.35) };
                        if w < 0.1 { break }
                        let Some((cls, fs, a)) = decor_fit(ast, &mut rng, w, d1 - d0, clear - 0.02) else { continue };
                        let x = if books { if rng.f64() < 0.5 { iv[0] + w / 2.0 } else { iv[1] - w / 2.0 } } else { iv[0] + span * (j as f64 + 0.5) / n_dec as f64 };
                        let mut at = [0.0; 3]; at[la] = x; at[da] = (d0 + d1) / 2.0; at[2] = top;
                        let name = format!("{key}.decor{n}"); n += 1;
                        add_decor(state, visuals, &up, &key, &room, name.clone(), cls, fs, a, at, rng.uniform(-0.5, 0.5));
                        added.push(name);
                        cut(&mut free, [x - w / 2.0 - 0.02, x + w / 2.0 + 0.02]);
                    }
                }
            }
            if !books { continue }
            for iv in free {
                let mut x = iv[0];
                while iv[1] - x >= 0.12 {
                    // a gap now and then (leaning space, a bookend)
                    if x > iv[0] && rng.f64() < 0.2 { x += rng.uniform(0.06, 0.2); continue }
                    let w = (iv[1] - x).min(rng.uniform(0.3, 1.0));
                    if w < 0.12 { break }
                    let fs = rng.next_u64();
                    let Some(col) = crate::assets::books::fit_column(fs, w, clear, depth) else { break };
                    let (clo, chi) = col.bounds();
                    let cw = chi[0] - clo[0];
                    // column frame: books along +x from 0, standing on z = 0, y in [clo1, chi1]; spines on the y = chi1 side face out of the unit
                    let fpos = d1 - 0.015; // front of the row, 1.5 cm behind the board edge
                    let (r, t): ([[f64; 3]; 3], V3) = if front == 0 {
                        // long = y, out = +x: rz(-90): column x -> unit -y, column y -> unit +x (spine side out); the row spans y in [x, x + cw]
                        ([[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [fpos - chi[1], x + cw, top])
                    } else {
                        // long = x, out = +y: identity, spines at +y
                        ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [x, fpos - chi[1], top])
                    };
                    let local = Pose::from_matrix([[r[0][0], r[0][1], r[0][2], t[0]], [r[1][0], r[1][1], r[1][2], t[1]], [r[2][0], r[2][1], r[2][2], t[2]], [0.0, 0.0, 0.0, 1.0]]);
                    let um = mat_of(&up);
                    let lm = mat_of(&local);
                    let mut wm = [[0.0; 4]; 4];
                    for i in 0..4 { for j in 0..4 { wm[i][j] = (0..4).map(|k| um[i][k] * lm[k][j]).sum() } }
                    let name = format!("{key}.books{n}");
                    n += 1;
                    let tags = TagSet::from([Tag::sem("Object"), Tag::sem("OfficeShelfItem"), Tag::gen("BookColumnFactory"), Tag::obj(&name)]);
                    // this row is occupied space for the other boards / duplicate support boxes of the unit
                    {
                        let (mut l, mut h) = ([f64::MAX; 3], [f64::MIN; 3]);
                        for i in 0..8 {
                            let c = [if i & 1 == 0 { clo[0] } else { chi[0] }, if i & 2 == 0 { clo[1] } else { chi[1] }, if i & 4 == 0 { clo[2] } else { chi[2] }];
                            let q = local.apply(c);
                            for d in 0..3 { l[d] = l[d].min(q[d]); h[d] = h[d].max(q[d]); }
                        }
                        if std::env::var_os("SHELFDBG").is_some() { eprintln!("SHELFDBG {key} row {n} board z {top:.3} iv {:.3}..{:.3} x {x:.3} cw {cw:.3} local {:?} {:?}", iv[0], iv[1], l.map(|v| (v * 1000.0).round() / 1000.0), h.map(|v| (v * 1000.0).round() / 1000.0)); }
                        kids.push((l, h));
                    }
                    let mut o = ObjState::new(Arc::new(col.solver_geom()), Pose::from_matrix(wm), tags);
                    o.generator = Some(GenRef { class: "BookColumnFactory".into(), factory_seed: fs, inst_seed: 0, unseeded: None, fit_box: None, size: None, join: None, piece: None });
                    o.relations.push(RelState { relation: Relation::Any, target: key.clone(), child_plane: 0, parent_plane: 0 });
                    o.relations.push(RelState { relation: Relation::Any, target: room.clone(), child_plane: 0, parent_plane: 0 });
                    state.objs.insert(name.clone(), o);
                    state.touch(&name);
                    visuals.insert(name.clone(), col);
                    added.push(name);
                    x += cw + rng.uniform(0.0, 0.02);
                }
            }
        }
    }
    added
}
