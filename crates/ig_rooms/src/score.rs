//! Room-level score terms (home_room_constraints "room" score + graph_coherent).
use super::consts::*;
use super::state::*;
use crate::shp::*;
use std::collections::BTreeSet;

fn norm2(v: P2) -> P2 {
    let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if n > 0.0 {
        [v[0] / n, v[1] / n]
    } else {
        v
    }
}

fn access_angle(state: &RoomState, x: &str) -> f64 {
    if state.graphs.is_empty() {
        return 0.0;
    }
    let graph = &state.graphs[room_level(x)];
    if graph.root() == x {
        return 0.0;
    }
    let root = centroid(state.poly(graph.root()));
    let co = centroid(state.poly(x));
    let mut best = std::f64::consts::PI;
    for n in graph.valid_neighbours(x) {
        let co_ = centroid(state.poly(n));
        let a = norm2([co_[0] - co[0], co_[1] - co[1]]);
        let b = norm2([root[0] - co[0], root[1] - co[1]]);
        let dot = (1.0 - 1e-6) * (a[0] * b[0] + a[1] * b[1]);
        best = best.min(dot.acos());
    }
    best
}

fn aspect_ratio(state: &RoomState, c: &str) -> f64 {
    let (x, y, xx, yy) = bounds(state.poly(c));
    let a = (xx - x) / (yy - y);
    a.max(1.0 / a)
}

fn convexity(state: &RoomState, x: &str) -> f64 {
    area(&convex_hull(state.poly(x))) / area(state.poly(x))
}

fn n_verts(state: &RoomState, x: &str) -> f64 {
    exterior(state.poly(x)).len() as f64
}

fn grid_line_count(state: &RoomState, ks: &[&str], c: &RoomConstants, dir: char) -> f64 {
    let mut s = BTreeSet::new();
    for k in ks {
        for p in exterior(state.poly(k)) {
            let v = if dir == 'x' { p[0] } else { p[1] };
            s.insert((v / c.unit).trunc() as i64);
        }
    }
    s.len() as f64
}

fn narrowness(state: &RoomState, x: &str, c: &RoomConstants, thresh: f64) -> f64 {
    let p = buffer(state.poly(x), 0.0);
    let mut count = 0.0;
    let n = (thresh / c.unit) as i32;
    for i in 0..n {
        let q = buffer(&buffer_full(&p, -c.unit / 2.0 * i as f64, Join::Mitre, Cap::Flat, 16, false), 0.0);
        if !(is_valid(&q) && is_polygon(&q) && area(&q) > 0.0) {
            count += 100.0;
        } else {
            count += length(&p) - length(&buffer_full(&q, c.unit / 2.0 * i as f64, Join::Mitre, Cap::Flat, 16, false));
        }
    }
    count
}

fn rel_value<'a>(state: &'a RoomState, from: &str, to: &str) -> &'a G {
    &state.objs[from].relations.iter().find(|r| r.target == to).unwrap().value
}

fn hinge_sq(a: f64, norm: f64) -> f64 {
    let x = (a / norm).ln();
    let h = if x < 0.0 {
        -x
    } else if x > 0.4 {
        x - 0.4
    } else {
        0.0
    };
    h * h
}

/// The "room" score term, in terms of weights/definitions from home.py (single story).
/// (room name, area m2, convex hull area / area, bbox aspect) of every valid room (PLAN_STATS=1 in gen2)
pub fn room_stats(state: &RoomState) -> Vec<(String, f64, f64, f64)> {
    state.valid_rooms().map(|(k, _)| (k.clone(), area(state.poly(k)), convexity(state, k), aspect_ratio(state, k))).collect()
}

pub fn room_term(state: &RoomState, c: &RoomConstants) -> f64 {
    let rooms: Vec<&String> = state.valid_rooms().map(|(k, _)| k).collect();
    let of = |t: &str| rooms.iter().copied().filter(move |k| room_type(k) == t).collect::<Vec<_>>();
    let mut total = 0.0;

    // access angle
    let s: f64 = rooms
        .iter()
        .filter(|k| !matches!(room_type(k), "utility" | "bathroom" | "closet"))
        .map(|k| (access_angle(state, k) - std::f64::consts::FRAC_PI_2).max(0.0))
        .sum();
    total += s * 5.0;

    // area
    // room size targets (no penalty between norm and 1.49 x norm). Infinigen's home.py norms (kitchen 20, bedroom 40, living 40,
    // dining 20, closet 5, bathroom 8, utility 5, garage 25, balcony 8) gave 46 m2 bedrooms and 65 m2 living rooms; these follow
    // real homes / 3D-FRONT (bedroom ~12-20 m2, living room ~25-35 m2). The typical areas (graph.rs) and so the house size follow.
    // (bedrooms past ~21 m2 and living rooms past ~33 m2 read as half empty in review: 14 / 22; plan_rules)
    let pr = crate::plan_rules::get();
    let area_terms: [(&str, f64); 10] = [
        ("kitchen", pr.kitchen_m2),
        ("bedroom", pr.bedroom_m2),
        ("living-room", pr.living_m2),
        ("dining-room", pr.dining_m2),
        ("closet", 3.0),
        ("bathroom", pr.bathroom_m2),
        ("utility", 4.0),
        ("garage", 20.0),
        ("balcony", 6.0),
        ("staircase-room", 25.0),
    ];
    let mut s = 0.0;
    for (t, norm) in area_terms {
        for k in of(t) {
            s += hinge_sq(area(state.poly(k)), norm);
        }
    }
    total += s * 500.0;

    // aspect ratio
    let mut s = 0.0;
    for t in ["kitchen", "bedroom", "living-room", "dining-room"] {
        for k in of(t) {
            s += aspect_ratio(state, k).ln();
        }
    }
    total += s * 50.0;
    let mut s = 0.0;
    for t in ["closet", "bathroom"] {
        for k in of(t) {
            s += aspect_ratio(state, k).ln();
        }
    }
    total += s * 40.0;

    // convexity, vertex count (non-hallway)
    let nonhall: Vec<&&String> = rooms.iter().filter(|k| room_type(k) != "hallway").collect();
    total += nonhall.iter().map(|k| convexity(state, k).ln()).sum::<f64>() * 5.0;
    // main rooms: L-shaped living rooms with a corridor arm (the hallway area term pushes circulation into them) read as hallways
    total += rooms.iter().filter(|k| matches!(room_type(k), "living-room" | "dining-room" | "bedroom" | "kitchen")).map(|k| convexity(state, k).ln()).sum::<f64>() * 120.0;
    total += nonhall.iter().map(|k| (n_verts(state, k) - 6.0).max(0.0).powf(1.5)).sum::<f64>() * 1.0;

    // exterior sharing
    let ext_tags = ["bedroom", "balcony", "dining-room", "kitchen", "living-room"];
    let ext_name = room_name(EXTERIOR, 0, 0);
    let ext_len = length(state.poly(&ext_name));
    let mut s = 0.0;
    for t in ext_tags {
        for k in of(t) {
            s += length(rel_value(state, &ext_name, k)) / ext_len;
        }
    }
    total += s * -10.0;
    let mut s = 0.0;
    for t in ext_tags {
        for k in of(t) {
            let mls = rel_value(state, &ext_name, k);
            let nv: usize = parts(mls).iter().map(|ls| if is_empty(ls) { 0 } else { coords_of(ls).len() }).sum();
            s += (nv as f64 - 2.0).max(0.0);
        }
    }
    total += s * -1.0;

    // grid lines
    let all: Vec<&str> = rooms.iter().map(|k| k.as_str()).collect();
    total += (grid_line_count(state, &all, c, 'x') + grid_line_count(state, &all, c, 'y')) * 2.0;
    let mut s = 0.0;
    for k in &all {
        s += grid_line_count(state, &[k], c, 'x') + grid_line_count(state, &[k], c, 'y');
    }
    total += s * 2.0;

    // hallway / staircase area
    let mut s = 0.0;
    for t in ["hallway", "staircase-room"] {
        for k in of(t) {
            s += area(state.poly(k));
        }
    }
    total += s * 20.0;
    // hallways are corridors: a size target (they otherwise absorb the floor left over: 20-60 m2 halls) and no part wider than
    // 2 m on wall centrelines (an opening at 2 m width leaves only the too-wide parts)
    let mut s = 0.0;
    for k in of("hallway") {
        let p = state.poly(k);
        s += hinge_sq(area(p), 5.0) * 500.0;
        let wide = buffer_full(&buffer_full(p, -1.0, Join::Mitre, Cap::Flat, 16, false), 1.0, Join::Mitre, Cap::Flat, 16, false);
        if !is_empty(&wide) {
            s += area(&wide) * 100.0;
        }
    }
    total += s;

    // narrowness
    // minimum widths on wall centrelines (clear width ~0.3 m less): 2.5 m left strip bedrooms and dining rooms ~2.2 m clear
    let mut s = 0.0;
    for k in &rooms {
        let thresh = match room_type(k) {
            "living-room" | "bedroom" | "dining-room" => 3.0,
            "kitchen" | "garage" => 2.5, // galley kitchens are fine
            "hallway" | "closet" => 1.5,
            _ => 2.0, // bathroom, utility, staircase, balcony
        };
        s += narrowness(state, k, c, thresh);
    }
    total += s * 2000.0;

    // staircase room terms (only with a staircase room)
    let ph = room_name(STAIRCASE, 0, 0);
    for k in of("staircase-room") {
        let pa = area(state.poly(&ph));
        let inter = area(&intersection(state.poly(k), state.poly(&ph)));
        total += inter / pa * -50.0;
        let h = inter / pa;
        let hinge = if h < c.staircase_thresh {
            c.staircase_thresh - h
        } else if h > 1.0 {
            h - 1.0
        } else {
            0.0
        };
        total += hinge * 1e5;
        total += (area(state.poly(k)) / pa - inter) * 5.0;
    }
    total
}

/// graph_coherent violation count.
pub fn graph_coherent(state: &RoomState, c: &RoomConstants) -> f64 {
    let mut count = 0.0;
    for (i, graph) in state.graphs.iter().enumerate() {
        for k in &graph.names {
            let ns = graph.neighbours(k);
            for r in &state.objs[k].relations {
                if ns.contains(r.target.as_str()) && !segment_filter(&r.value, c.segment_margin) {
                    count += if room_type(k) == EXTERIOR { 2.0 } else { 1.0 };
                }
            }
        }
        let total_area: f64 = graph
            .names
            .iter()
            .filter(|n| !graph.invalid.contains(&graph.names.iter().position(|m| m == *n).unwrap()))
            .map(|n| area(state.poly(n)))
            .sum();
        if (total_area - area(state.poly(&room_name(EXTERIOR, i, 0)))).abs() > 0.01 {
            count += 1.0;
        }
        if i > 0 && !c.fixed_contour {
            if !contains(state.poly(&room_name(EXTERIOR, i - 1, 0)), state.poly(&room_name(EXTERIOR, i, 0))) {
                count += 1.0;
            }
        }
    }
    count
}
