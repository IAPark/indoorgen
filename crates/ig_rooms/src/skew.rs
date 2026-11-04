//! Slightly out-of-square rooms. A plan's walls lie along long straight lines shared by several rooms, so each line is tilted as a whole, by a small angle of its own, and
//! every vertex goes to where its two lines now cross: walls stay straight through T junctions and neighbours stay joined, which moving corners one by one would not give.
use crate::shp::*;
use crate::state::RoomState;
use std::collections::BTreeMap;

fn key(v: f64) -> i64 { (v * 1e5).round() as i64 }

/// lines at one coordinate: (coordinate, slope, pivot along the line); `slope_at` reads between them
fn slope_at(lines: &[(f64, f64, f64)], v: f64) -> (f64, f64) {
    if lines.is_empty() { return (0.0, 0.0) }
    let i = lines.partition_point(|l| l.0 < v);
    if i == 0 { return (lines[0].1, lines[0].2) }
    if i == lines.len() { let l = lines[i - 1]; return (l.1, l.2) }
    let (a, b) = (lines[i - 1], lines[i]);
    let t = (v - a.0) / (b.0 - a.0).max(1e-9);
    (a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

/// `draw(line index key)` gives each line's angle in degrees
pub fn skew_plan(st: &mut RoomState, mut draw: impl FnMut() -> f64) {
    let rings: Vec<Vec<P2>> = st.objs.values().filter_map(|o| o.polygon.as_ref()).map(exterior).collect();
    // the extent of each line, from the walls along it
    let (mut xl, mut yl): (BTreeMap<i64, (f64, f64, f64)>, BTreeMap<i64, (f64, f64, f64)>) = Default::default();
    for r in &rings {
        for w in r.windows(2) {
            let (p, q) = (w[0], w[1]);
            if (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() > 1e-6 {
                let e = xl.entry(key(p[0])).or_insert((p[0], f64::MAX, f64::MIN));
                e.1 = e.1.min(p[1].min(q[1])); e.2 = e.2.max(p[1].max(q[1]));
            }
            if (p[1] - q[1]).abs() < 1e-6 && (p[0] - q[0]).abs() > 1e-6 {
                let e = yl.entry(key(p[1])).or_insert((p[1], f64::MAX, f64::MIN));
                e.1 = e.1.min(p[0].min(q[0])); e.2 = e.2.max(p[0].max(q[0]));
            }
        }
    }
    let mut lines = |m: &BTreeMap<i64, (f64, f64, f64)>| -> Vec<(f64, f64, f64)> { m.values().map(|&(c, lo, hi)| (c, draw().to_radians().tan(), (lo + hi) / 2.0)).collect() };
    let (xs, ys) = (lines(&xl), lines(&yl));
    // on the tilted lines x = x0 + a (y - py) and y = y0 + b (x - px)
    let map = |p: P2| -> P2 {
        let ((a, py), (b, px)) = (slope_at(&xs, p[0]), slope_at(&ys, p[1]));
        let y = (p[1] + b * (p[0] - a * py - px)) / (1.0 - a * b);
        [p[0] + a * (y - py), y]
    };
    // (a T junction is a vertex of one room only: its neighbour's wall gets it too, as float rounding would no longer leave it on that wall, and their shared stretch would be lost)
    let all: Vec<P2> = rings.iter().flatten().copied().collect();
    let node = |r: &[P2]| -> Vec<P2> {
        let mut out: Vec<P2> = vec![];
        for w in r.windows(2) {
            let (p, q) = (w[0], w[1]);
            out.push(p);
            let d = [q[0] - p[0], q[1] - p[1]];
            let l2 = d[0] * d[0] + d[1] * d[1];
            if l2 < 1e-12 || (d[0].abs() > 1e-6 && d[1].abs() > 1e-6) { continue }
            let mut on: Vec<(f64, P2)> = all.iter().filter_map(|v| {
                let t = ((v[0] - p[0]) * d[0] + (v[1] - p[1]) * d[1]) / l2;
                let off = (v[0] - p[0]) * d[1] - (v[1] - p[1]) * d[0];
                (off.abs() < 1e-6 && t > 1e-6 && t < 1.0 - 1e-6).then_some((t, *v))
            }).collect();
            on.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            on.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
            out.extend(on.into_iter().map(|x| x.1));
        }
        if let Some(f) = r.first() { out.push(*f); }
        out
    };
    for o in st.objs.values_mut() {
        if let Some(g) = &o.polygon {
            let ext: Vec<P2> = node(&exterior(g)).into_iter().map(map).collect();
            let holes: Vec<Vec<P2>> = interiors(g).iter().map(|h| node(h).into_iter().map(map).collect()).collect();
            if let Ok(n) = polygon(&ext, &holes) { o.polygon = Some(n); }
        }
        for r in &mut o.relations {
            let ls: Vec<G> = parts(&r.value).iter().filter(|p| !is_empty(p)).filter_map(|p| linestring(&coords_of(p).into_iter().map(map).collect::<Vec<_>>()).ok()).collect();
            if let Ok(m) = multilinestring(ls) { r.value = m; }
        }
    }
}

/// the T junctions again out of the room outlines, once the walls have been worked out: whatever reads a room's walls afterwards wants one wall, not its pieces
pub fn drop_collinear(st: &mut RoomState) {
    for o in st.objs.values_mut() {
        let Some(g) = &o.polygon else { continue };
        let clean = |r: Vec<P2>| -> Vec<P2> {
            let n = r.len().saturating_sub(1);
            if n < 4 { return r }
            let keep: Vec<P2> = (0..n).filter(|&i| {
                let (a, b, c) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
                let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
                lu < 1e-9 || lv < 1e-9 || (u[0] * v[1] - u[1] * v[0]).abs() / (lu * lv) > 1e-7 || u[0] * v[0] + u[1] * v[1] < 0.0
            }).map(|i| r[i]).collect();
            let mut k = keep;
            if let Some(f) = k.first().copied() { k.push(f); }
            k
        };
        let (ext, holes) = (clean(exterior(g)), interiors(g).into_iter().map(clean).collect::<Vec<_>>());
        if ext.len() >= 4 { if let Ok(n) = polygon(&ext, &holes) { o.polygon = Some(n); } }
    }
}
