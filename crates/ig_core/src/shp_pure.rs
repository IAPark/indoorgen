//! Pure-Rust 2D geometry backend with the API of the GEOS wrapper (shp_geos.rs): used on wasm32 (no C++ there) and, for testing,
//! natively with `--features pure_geom`. Booleans / offsets run on i_overlay (i64 engine: dyadic coordinates stay exact), the line work
//! (noding, line merge, shared paths, polygonize) is small own code. Results are sane but not bit-identical to GEOS (vertex order,
//! ring start, join arcs differ); shell rings come out clockwise like JTS.
use super::{Cap, Join, P2};
use anyhow::{anyhow, Result};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, OutlineStyle, StrokeStyle};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum G {
    Point(P2),
    MultiPoint(Vec<P2>),
    LineString(Vec<P2>),
    LinearRing(Vec<P2>),
    MultiLineString(Vec<Vec<P2>>),
    /// rings (closed, first = shell); empty vec = POLYGON EMPTY
    Polygon(Vec<Vec<P2>>),
    MultiPolygon(Vec<Vec<Vec<P2>>>),
    Collection(Vec<G>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryTypes {
    Point,
    LineString,
    LinearRing,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
    GeometryCollection,
}

const EPS: f64 = 1e-9;
type Ring = Vec<P2>;

// ---------------------------------------------------------------- small helpers

fn closed(r: &[P2]) -> Ring {
    let mut e = r.to_vec();
    if e.first() != e.last() {
        e.push(e[0]);
    }
    e
}
fn opened(r: &[P2]) -> Ring {
    let mut e = r.to_vec();
    if e.len() > 1 && e.first() == e.last() {
        e.pop();
    }
    e
}
/// shoelace signed area of an open or closed ring (CCW positive)
fn ring_area(r: &[P2]) -> f64 {
    let n = r.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (r[i], r[(i + 1) % n]);
        s += a[0] * b[1] - b[0] * a[1];
    }
    s / 2.0
}
fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn dist(a: P2, b: P2) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
fn near(a: P2, b: P2) -> bool {
    dist(a, b) <= 1e-8
}
fn key(p: P2) -> (i64, i64) {
    ((p[0] * 1e8).round() as i64, (p[1] * 1e8).round() as i64)
}

/// Coordinates snapped onto a registry so that equal points (to 1e-8) share bit-identical coordinates.
#[derive(Default)]
struct Snap(HashMap<(i64, i64), P2>);
impl Snap {
    fn get(&mut self, p: P2) -> P2 {
        let (kx, ky) = key(p);
        for dx in [0i64, -1, 1] {
            for dy in [0i64, -1, 1] {
                if let Some(q) = self.0.get(&(kx + dx, ky + dy)) {
                    if near(*q, p) {
                        return *q;
                    }
                }
            }
        }
        self.0.insert((kx, ky), p);
        p
    }
}

// ---------------------------------------------------------------- constructors / accessors

pub fn polygon(ext: &[P2], holes: &[Vec<P2>]) -> Result<G> {
    let e = closed(ext);
    if e.len() < 4 {
        return Err(anyhow!("geos: Invalid number of points in LinearRing found {} - must be 0 or >= 4", e.len()));
    }
    let mut rings = vec![e];
    for h in holes {
        let hh = closed(h);
        if hh.len() < 4 {
            return Err(anyhow!("geos: Invalid number of points in LinearRing found {} - must be 0 or >= 4", hh.len()));
        }
        rings.push(hh);
    }
    Ok(G::Polygon(rings))
}

pub fn linestring(c: &[P2]) -> Result<G> {
    if c.len() == 1 {
        return Err(anyhow!("geos: Invalid number of points in LineString found 1 - must be 0 or >= 2"));
    }
    Ok(G::LineString(c.to_vec()))
}
pub fn linearring(c: &[P2]) -> Result<G> {
    Ok(G::LinearRing(closed(c)))
}
pub fn multilinestring(lines: Vec<G>) -> Result<G> {
    Ok(G::MultiLineString(lines.iter().flat_map(|l| lines_of(l)).collect()))
}
pub fn multipolygon(ps: Vec<G>) -> Result<G> {
    Ok(G::MultiPolygon(ps.iter().flat_map(|p| polys_of(p)).collect()))
}
pub fn empty_mls() -> G {
    G::MultiLineString(vec![])
}
pub fn point(x: f64, y: f64) -> G {
    G::Point([x, y])
}

pub fn coords_of(g: &G) -> Vec<P2> {
    match g {
        G::Point(p) => vec![*p],
        G::MultiPoint(v) | G::LineString(v) | G::LinearRing(v) => v.clone(),
        G::Polygon(r) => r.first().cloned().unwrap_or_default(),
        G::MultiLineString(ls) => ls.iter().flatten().copied().collect(),
        G::MultiPolygon(ps) => ps.iter().filter_map(|p| p.first()).flatten().copied().collect(),
        G::Collection(gs) => gs.iter().flat_map(coords_of).collect(),
    }
}
fn all_coords(g: &G) -> Vec<P2> {
    match g {
        G::Polygon(r) => r.iter().flatten().copied().collect(),
        G::MultiPolygon(ps) => ps.iter().flatten().flatten().copied().collect(),
        G::Collection(gs) => gs.iter().flat_map(all_coords).collect(),
        _ => coords_of(g),
    }
}

pub fn exterior(g: &G) -> Vec<P2> {
    match g {
        G::Polygon(r) => r.first().cloned().unwrap_or_default(),
        _ => panic!("exterior of a non-polygon"),
    }
}
pub fn interiors(g: &G) -> Vec<Vec<P2>> {
    match g {
        G::Polygon(r) => r.iter().skip(1).cloned().collect(),
        _ => panic!("interiors of a non-polygon"),
    }
}
pub fn gtype(g: &G) -> GeometryTypes {
    match g {
        G::Point(_) => GeometryTypes::Point,
        G::MultiPoint(_) => GeometryTypes::MultiPoint,
        G::LineString(_) => GeometryTypes::LineString,
        G::LinearRing(_) => GeometryTypes::LinearRing,
        G::MultiLineString(_) => GeometryTypes::MultiLineString,
        G::Polygon(_) => GeometryTypes::Polygon,
        G::MultiPolygon(_) => GeometryTypes::MultiPolygon,
        G::Collection(_) => GeometryTypes::GeometryCollection,
    }
}
pub fn is_polygon(g: &G) -> bool {
    matches!(g, G::Polygon(_))
}
pub fn parts(g: &G) -> Vec<G> {
    match g {
        G::MultiPolygon(ps) => ps.iter().map(|p| G::Polygon(p.clone())).collect(),
        G::MultiLineString(ls) => ls.iter().map(|l| G::LineString(l.clone())).collect(),
        G::MultiPoint(v) => v.iter().map(|p| G::Point(*p)).collect(),
        G::Collection(gs) => gs.clone(),
        _ => vec![g.clone()],
    }
}
pub fn is_empty(g: &G) -> bool {
    match g {
        G::Point(_) => false,
        G::MultiPoint(v) | G::LineString(v) | G::LinearRing(v) => v.is_empty(),
        G::MultiLineString(ls) => ls.iter().all(|l| l.is_empty()),
        G::Polygon(r) => r.is_empty() || r[0].is_empty(),
        G::MultiPolygon(ps) => ps.iter().all(|p| p.is_empty() || p[0].is_empty()),
        G::Collection(gs) => gs.iter().all(is_empty),
    }
}
pub fn bounds(g: &G) -> (f64, f64, f64, f64) {
    let c = all_coords(g);
    if c.is_empty() {
        return (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in c {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    (x0, y0, x1, y1)
}

/// all polygons (closed rings, shell first) of the areal parts of `g`
fn polys_of(g: &G) -> Vec<Vec<Ring>> {
    match g {
        G::Polygon(r) if !r.is_empty() && !r[0].is_empty() => vec![r.clone()],
        G::MultiPolygon(ps) => ps.iter().filter(|p| !p.is_empty() && !p[0].is_empty()).cloned().collect(),
        G::Collection(gs) => gs.iter().flat_map(polys_of).collect(),
        _ => vec![],
    }
}
/// all linestrings (open chains) of the linear parts of `g`; polygons contribute nothing
fn lines_of(g: &G) -> Vec<Ring> {
    match g {
        G::LineString(l) | G::LinearRing(l) if l.len() >= 2 => vec![l.clone()],
        G::MultiLineString(ls) => ls.iter().filter(|l| l.len() >= 2).cloned().collect(),
        G::Collection(gs) => gs.iter().flat_map(lines_of).collect(),
        _ => vec![],
    }
}
fn points_of(g: &G) -> Vec<P2> {
    match g {
        G::Point(p) => vec![*p],
        G::MultiPoint(v) => v.clone(),
        G::Collection(gs) => gs.iter().flat_map(points_of).collect(),
        _ => vec![],
    }
}
fn is_areal(g: &G) -> bool {
    !polys_of(g).is_empty()
}
fn is_linear(g: &G) -> bool {
    !lines_of(g).is_empty()
}
/// dimension: 2 areal, 1 linear, 0 points, -1 empty (a mixed collection reports its highest)
fn dim(g: &G) -> i32 {
    if is_areal(g) {
        2
    } else if is_linear(g) {
        1
    } else if !points_of(g).is_empty() {
        0
    } else {
        -1
    }
}

pub fn area(g: &G) -> f64 {
    polys_of(g)
        .iter()
        .map(|p| p.iter().enumerate().map(|(i, r)| if i == 0 { ring_area(r).abs() } else { -ring_area(r).abs() }).sum::<f64>())
        .sum()
}
pub fn length(g: &G) -> f64 {
    let seg = |r: &Ring| -> f64 { r.windows(2).map(|w| dist(w[0], w[1])).sum() };
    let mut s: f64 = lines_of(g).iter().map(seg).sum();
    for p in polys_of(g) {
        s += p.iter().map(seg).sum::<f64>();
    }
    s
}

// ---------------------------------------------------------------- polygon overlay glue

fn contours(polys: &[Vec<Ring>]) -> Vec<Vec<P2>> {
    let mut out = vec![];
    for p in polys {
        for (i, r) in p.iter().enumerate() {
            let mut o = opened(r);
            if o.len() < 3 {
                continue;
            }
            let a = ring_area(&o);
            if a == 0.0 {
                continue;
            }
            if (a > 0.0) != (i == 0) {
                o.reverse();
            }
            out.push(o);
        }
    }
    out
}

fn from_shapes(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> G {
    let mut polys: Vec<Vec<Ring>> = vec![];
    for sh in shapes {
        let mut rings = vec![];
        for (i, c) in sh.into_iter().enumerate() {
            if c.len() < 3 {
                continue;
            }
            let mut o = c;
            let a = ring_area(&o);
            // JTS orientation of results: shells clockwise, holes counter-clockwise
            if (a > 0.0) == (i == 0) {
                o.reverse();
            }
            rings.push(closed(&o));
        }
        if !rings.is_empty() {
            polys.push(rings);
        }
    }
    match polys.len() {
        0 => G::Polygon(vec![]),
        1 => G::Polygon(polys.remove(0)),
        _ => G::MultiPolygon(polys),
    }
}

fn overlay(a: &[Vec<Ring>], b: &[Vec<Ring>], rule: OverlayRule) -> G {
    let (ca, cb) = (contours(a), contours(b));
    if ca.is_empty() && cb.is_empty() {
        return G::Polygon(vec![]);
    }
    let res = if ca.is_empty() {
        match rule {
            OverlayRule::Union | OverlayRule::Xor => cb.overlay_as::<i64>(&Vec::<Vec<P2>>::new(), OverlayRule::Union, FillRule::NonZero),
            _ => vec![],
        }
    } else if cb.is_empty() {
        match rule {
            OverlayRule::Intersect => vec![],
            _ => ca.overlay_as::<i64>(&Vec::<Vec<P2>>::new(), OverlayRule::Union, FillRule::NonZero),
        }
    } else {
        ca.overlay_as::<i64>(&cb, rule, FillRule::NonZero)
    };
    from_shapes(res)
}

// ---------------------------------------------------------------- boolean ops

pub fn union(a: &G, b: &G) -> G {
    match (dim(a), dim(b)) {
        (2, 2) | (2, -1) | (-1, 2) | (-1, -1) => overlay(&polys_of(a), &polys_of(b), OverlayRule::Union),
        (1, 1) | (1, -1) | (-1, 1) => unary_union(&G::Collection(vec![a.clone(), b.clone()])),
        _ => G::Collection(vec![a.clone(), b.clone()]),
    }
}
pub fn difference(a: &G, b: &G) -> G {
    match (dim(a), dim(b)) {
        (2, 2) | (-1, 2) | (-1, -1) => overlay(&polys_of(a), &polys_of(b), OverlayRule::Difference),
        (2, _) => overlay(&polys_of(a), &[], OverlayRule::Union),
        (1, 1) => lines_to_g(line_difference(&lines_of(a), &lines_of(b))),
        (1, 2) => lines_to_g(clip_lines(&lines_of(a), &polys_of(b), false)),
        _ => a.clone(),
    }
}
pub fn intersection(a: &G, b: &G) -> G {
    match (dim(a), dim(b)) {
        (2, 2) => overlay(&polys_of(a), &polys_of(b), OverlayRule::Intersect),
        (1, 2) => lines_to_g(clip_lines(&lines_of(a), &polys_of(b), true)),
        (2, 1) => lines_to_g(clip_lines(&lines_of(b), &polys_of(a), true)),
        (1, 1) => {
            let (f, bk) = shared_paths(a, b);
            let mut l = lines_of(&f);
            l.extend(lines_of(&bk));
            lines_to_g(l)
        }
        (2, _) | (_, 2) | (-1, _) | (_, -1) => G::Polygon(vec![]),
        _ => G::Collection(vec![]),
    }
}
pub fn union_all(gs: &[G]) -> G {
    if gs.is_empty() {
        return G::Collection(vec![]);
    }
    let polys: Vec<Vec<Ring>> = gs.iter().flat_map(polys_of).collect();
    if gs.iter().all(|g| dim(g) <= 2 && (dim(g) == 2 || dim(g) == -1)) {
        if polys.is_empty() {
            return G::Polygon(vec![]);
        }
        return overlay(&polys, &[], OverlayRule::Union);
    }
    if gs.iter().all(|g| dim(g) == 1 || dim(g) == -1) {
        return unary_union(&G::Collection(gs.to_vec()));
    }
    G::Collection(gs.to_vec())
}

fn lines_to_g(mut l: Vec<Ring>) -> G {
    l.retain(|x| x.len() >= 2);
    match l.len() {
        0 => G::LineString(vec![]),
        1 => G::LineString(l.remove(0)),
        _ => G::MultiLineString(l),
    }
}

pub fn unary_union(g: &G) -> G {
    if is_areal(g) {
        return overlay(&polys_of(g), &[], OverlayRule::Union);
    }
    let lines = lines_of(g);
    if lines.is_empty() {
        return g.clone();
    }
    let segs = node_lines(&lines);
    lines_to_g(merge_chains(segs.into_iter().map(|s| s.to_vec()).collect()))
}

pub fn boundary(g: &G) -> G {
    match g {
        G::Polygon(r) => match r.len() {
            0 => G::MultiLineString(vec![]),
            1 => G::LinearRing(r[0].clone()),
            _ => G::MultiLineString(r.clone()),
        },
        G::MultiPolygon(ps) => G::MultiLineString(ps.iter().flatten().cloned().collect()),
        G::LineString(l) if l.len() >= 2 && l.first() != l.last() => G::MultiPoint(vec![l[0], l[l.len() - 1]]),
        _ => G::Collection(vec![]),
    }
}

pub fn centroid(g: &G) -> P2 {
    let polys = polys_of(g);
    let (mut a_sum, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for p in &polys {
        for (i, r) in p.iter().enumerate() {
            let sign = if i == 0 { 1.0 } else { -1.0 };
            let o = opened(r);
            let a = ring_area(&o);
            if a == 0.0 {
                continue;
            }
            // centroid of the ring polygon
            let (mut sx, mut sy) = (0.0, 0.0);
            for k in 0..o.len() {
                let (p0, p1) = (o[k], o[(k + 1) % o.len()]);
                let w = p0[0] * p1[1] - p1[0] * p0[1];
                sx += (p0[0] + p1[0]) * w;
                sy += (p0[1] + p1[1]) * w;
            }
            let (rx, ry) = (sx / (6.0 * a), sy / (6.0 * a));
            let aa = a.abs() * sign;
            a_sum += aa;
            cx += rx * aa;
            cy += ry * aa;
        }
    }
    if a_sum.abs() > 0.0 {
        return [cx / a_sum, cy / a_sum];
    }
    let lines = lines_of(g);
    if !lines.is_empty() {
        let (mut l_sum, mut lx, mut ly) = (0.0, 0.0, 0.0);
        for l in &lines {
            for w in l.windows(2) {
                let d = dist(w[0], w[1]);
                l_sum += d;
                lx += (w[0][0] + w[1][0]) / 2.0 * d;
                ly += (w[0][1] + w[1][1]) / 2.0 * d;
            }
        }
        if l_sum > 0.0 {
            return [lx / l_sum, ly / l_sum];
        }
    }
    let pts = all_coords(g);
    if pts.is_empty() {
        return [f64::NAN, f64::NAN];
    }
    [pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64, pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64]
}

pub fn convex_hull(g: &G) -> G {
    let mut pts = all_coords(g);
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pts.dedup();
    if pts.is_empty() {
        return G::Collection(vec![]);
    }
    if pts.len() == 1 {
        return G::Point(pts[0]);
    }
    let mut lower: Vec<P2> = vec![];
    for &p in &pts {
        while lower.len() >= 2 && cross(sub(lower[lower.len() - 1], lower[lower.len() - 2]), sub(p, lower[lower.len() - 1])) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P2> = vec![];
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(sub(upper[upper.len() - 1], upper[upper.len() - 2]), sub(p, upper[upper.len() - 1])) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper); // CCW
    if lower.len() < 3 {
        return G::LineString(vec![pts[0], pts[pts.len() - 1]]);
    }
    lower.reverse(); // JTS: clockwise
    G::Polygon(vec![closed(&lower)])
}

// ---------------------------------------------------------------- predicates

/// 0 outside, 1 inside, 2 on the boundary
fn pt_in_ring(p: P2, ring: &[P2]) -> u8 {
    let o = opened(ring);
    let n = o.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (o[i], o[(i + 1) % n]);
        // on the segment?
        let ab = sub(b, a);
        let l = dist(a, b);
        if l > 0.0 {
            let t = dot(sub(p, a), ab) / (l * l);
            if (-1e-12..=1.0 + 1e-12).contains(&t) {
                let q = [a[0] + t * ab[0], a[1] + t * ab[1]];
                if dist(p, q) <= 1e-9 {
                    return 2;
                }
            }
        }
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside as u8
}
fn pt_in_poly(p: P2, rings: &[Ring]) -> u8 {
    if rings.is_empty() {
        return 0;
    }
    match pt_in_ring(p, &rings[0]) {
        0 => 0,
        2 => 2,
        _ => {
            for h in &rings[1..] {
                match pt_in_ring(p, h) {
                    1 => return 0,
                    2 => return 2,
                    _ => {}
                }
            }
            1
        }
    }
}
fn pt_in_polys(p: P2, polys: &[Vec<Ring>]) -> u8 {
    let mut best = 0;
    for pl in polys {
        match pt_in_poly(p, pl) {
            1 => return 1,
            2 => best = 2,
            _ => {}
        }
    }
    best
}

pub fn contains(a: &G, b: &G) -> bool {
    let pa = polys_of(a);
    if pa.is_empty() || is_empty(b) {
        return false;
    }
    let mut interior = false;
    for p in points_of(b) {
        match pt_in_polys(p, &pa) {
            0 => return false,
            1 => interior = true,
            _ => {}
        }
    }
    for l in lines_of(b) {
        for w in l.windows(2) {
            for q in [w[0], [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0], w[1]] {
                match pt_in_polys(q, &pa) {
                    0 => return false,
                    1 => interior = true,
                    _ => {}
                }
            }
        }
    }
    let pb = polys_of(b);
    if !pb.is_empty() {
        let ab = area(b);
        let out = overlay(&pb, &pa, OverlayRule::Difference);
        if area(&out) > 1e-9 * (1.0 + ab) {
            return false;
        }
        if ab > 0.0 {
            interior = true;
        }
    }
    interior
}
pub fn try_contains(a: &G, b: &G) -> bool {
    contains(a, b)
}
pub fn within(a: &G, b: &G) -> bool {
    contains(b, a)
}
pub fn contains_pt(poly: &G, p: P2) -> bool {
    pt_in_polys(p, &polys_of(poly)) == 1
}

fn seg_dist(p: P2, a: P2, b: P2) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * ab[0], a[1] + t * ab[1]])
}
fn seg_seg_dist(a: P2, b: P2, c: P2, d: P2) -> f64 {
    if seg_intersect(a, b, c, d) {
        return 0.0;
    }
    seg_dist(a, c, d).min(seg_dist(b, c, d)).min(seg_dist(c, a, b)).min(seg_dist(d, a, b))
}
fn seg_intersect(a: P2, b: P2, c: P2, d: P2) -> bool {
    let o = |p: P2, q: P2, r: P2| cross(sub(q, p), sub(r, p));
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    seg_dist(a, c, d) <= 1e-12 || seg_dist(b, c, d) <= 1e-12 || seg_dist(c, a, b) <= 1e-12 || seg_dist(d, a, b) <= 1e-12
}

pub fn distance(a: &G, b: &G) -> f64 {
    let (pa, pb) = (polys_of(a), polys_of(b));
    // any vertex inside the other polygon -> 0
    for p in all_coords(b) {
        if pt_in_polys(p, &pa) > 0 {
            return 0.0;
        }
    }
    for p in all_coords(a) {
        if pt_in_polys(p, &pb) > 0 {
            return 0.0;
        }
    }
    let segs = |g: &G| -> Vec<(P2, P2)> {
        let mut s = vec![];
        for l in lines_of(g) {
            s.extend(l.windows(2).map(|w| (w[0], w[1])));
        }
        for p in polys_of(g) {
            for r in p {
                s.extend(r.windows(2).map(|w| (w[0], w[1])));
            }
        }
        for p in points_of(g) {
            s.push((p, p));
        }
        s
    };
    let (sa, sb) = (segs(a), segs(b));
    let mut best = f64::INFINITY;
    for &(p, q) in &sa {
        for &(r, s) in &sb {
            best = best.min(seg_seg_dist(p, q, r, s));
            if best == 0.0 {
                return 0.0;
            }
        }
    }
    best
}

fn ring_simple(r: &[P2]) -> bool {
    let o = opened(r);
    let n = o.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, b) = (o[i], o[(i + 1) % n]);
        if near(a, b) {
            continue; // repeated point: tolerated like GEOS
        }
        for j in i + 1..n {
            let (c, d) = (o[j], o[(j + 1) % n]);
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            if adjacent {
                // neighbours may only share their common vertex
                let (shared, p_far, q_far) = if j == i + 1 { (b, a, d) } else { (a, b, c) };
                let _ = shared;
                let u = sub(p_far, shared);
                let v = sub(q_far, shared);
                if cross(u, v).abs() <= 1e-12 * dist(p_far, shared) * dist(q_far, shared) && dot(u, v) > 0.0 {
                    return false; // spike / overlap
                }
                continue;
            }
            if seg_intersect(a, b, c, d) {
                return false;
            }
        }
    }
    true
}
pub fn is_valid(g: &G) -> bool {
    for p in polys_of(g) {
        for r in &p {
            if !ring_simple(r) {
                return false;
            }
        }
        for h in &p[1..] {
            // holes inside the shell, not crossing it
            for &v in &opened(h) {
                if pt_in_ring(v, &p[0]) == 0 {
                    return false;
                }
            }
            let (so, ho) = (opened(&p[0]), opened(h));
            for i in 0..ho.len() {
                for j in 0..so.len() {
                    let (a, b) = (ho[i], ho[(i + 1) % ho.len()]);
                    let (c, d) = (so[j], so[(j + 1) % so.len()]);
                    let o = |p: P2, q: P2, r: P2| cross(sub(q, p), sub(r, p));
                    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
                    if ((d1 > 1e-12 && d2 < -1e-12) || (d1 < -1e-12 && d2 > 1e-12)) && ((d3 > 1e-12 && d4 < -1e-12) || (d3 < -1e-12 && d4 > 1e-12)) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------- normalize / simplify / densify

fn normalize_ring(r: &[P2], clockwise: bool) -> Ring {
    let mut o = opened(r);
    if o.is_empty() {
        return o;
    }
    let mi = (0..o.len()).min_by(|&a, &b| o[a].partial_cmp(&o[b]).unwrap()).unwrap();
    o.rotate_left(mi);
    if (ring_area(&o) < 0.0) != clockwise {
        o[1..].reverse();
    }
    closed(&o)
}
pub fn normalize(g: &G) -> G {
    match g {
        G::Polygon(r) => {
            if r.is_empty() {
                return g.clone();
            }
            let mut rings = vec![normalize_ring(&r[0], true)];
            let mut hs: Vec<Ring> = r[1..].iter().map(|h| normalize_ring(h, false)).collect();
            hs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            rings.extend(hs);
            G::Polygon(rings)
        }
        G::MultiPolygon(ps) => {
            let mut v: Vec<Vec<Ring>> = ps.iter().map(|p| if let G::Polygon(r) = normalize(&G::Polygon(p.clone())) { r } else { vec![] }).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            G::MultiPolygon(v)
        }
        G::LineString(l) => {
            let mut l = l.clone();
            if l.len() >= 2 && l[0] > l[l.len() - 1] {
                l.reverse();
            }
            G::LineString(l)
        }
        G::MultiLineString(ls) => {
            let mut v: Vec<Ring> = ls.iter().map(|l| if let G::LineString(x) = normalize(&G::LineString(l.clone())) { x } else { vec![] }).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            G::MultiLineString(v)
        }
        _ => g.clone(),
    }
}

fn dp(pts: &[P2], tol: f64, out: &mut Vec<bool>, lo: usize, hi: usize) {
    if hi <= lo + 1 {
        return;
    }
    let (mut best, mut bi) = (-1.0, lo);
    for i in lo + 1..hi {
        let d = seg_dist(pts[i], pts[lo], pts[hi]);
        if d > best {
            best = d;
            bi = i;
        }
    }
    if best > tol {
        out[bi] = true;
        dp(pts, tol, out, lo, bi);
        dp(pts, tol, out, bi, hi);
    }
}
fn simplify_chain(pts: &[P2], tol: f64, is_ring: bool) -> Ring {
    if pts.len() <= if is_ring { 4 } else { 2 } {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    let last = pts.len() - 1;
    keep[last] = true;
    if is_ring {
        // split the ring at the vertex farthest from the start so both halves are proper chains
        let far = (1..last).max_by(|&a, &b| dist(pts[a], pts[0]).partial_cmp(&dist(pts[b], pts[0])).unwrap()).unwrap();
        keep[far] = true;
        dp(pts, tol, &mut keep, 0, far);
        dp(pts, tol, &mut keep, far, last);
    } else {
        dp(pts, tol, &mut keep, 0, last);
    }
    let out: Ring = pts.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| *p).collect();
    if is_ring && (out.len() < 4 || !ring_simple(&out)) {
        return pts.to_vec();
    }
    out
}
pub fn simplify(g: &G, tol: f64) -> G {
    match g {
        G::Polygon(r) => G::Polygon(r.iter().map(|x| simplify_chain(x, tol, true)).collect()),
        G::MultiPolygon(ps) => G::MultiPolygon(ps.iter().map(|p| p.iter().map(|x| simplify_chain(x, tol, true)).collect()).collect()),
        G::LineString(l) => G::LineString(simplify_chain(l, tol, l.first() == l.last())),
        G::LinearRing(l) => G::LinearRing(simplify_chain(l, tol, true)),
        G::MultiLineString(ls) => G::MultiLineString(ls.iter().map(|l| simplify_chain(l, tol, l.first() == l.last())).collect()),
        _ => g.clone(),
    }
}

fn dedupe_chain(pts: &[P2], tol: f64, ring: bool) -> Ring {
    let mut out: Ring = vec![];
    for &p in pts {
        if out.last().map_or(true, |q| dist(*q, p) > tol) {
            out.push(p);
        }
    }
    if ring && out.len() >= 2 {
        let f = out[0];
        let l = out.len() - 1;
        if l > 0 && dist(out[l], f) <= tol {
            out[l] = f;
        }
    }
    if ring && out.len() < 4 {
        return pts.to_vec();
    }
    out
}
pub fn remove_repeated_points(g: &G, tol: f64) -> G {
    match g {
        G::Polygon(r) => G::Polygon(r.iter().map(|x| dedupe_chain(x, tol, true)).collect()),
        G::MultiPolygon(ps) => G::MultiPolygon(ps.iter().map(|p| p.iter().map(|x| dedupe_chain(x, tol, true)).collect()).collect()),
        G::LineString(l) => G::LineString(dedupe_chain(l, tol, false)),
        G::LinearRing(l) => G::LinearRing(dedupe_chain(l, tol, true)),
        G::MultiLineString(ls) => G::MultiLineString(ls.iter().map(|l| dedupe_chain(l, tol, false)).collect()),
        _ => g.clone(),
    }
}

fn densify_chain(pts: &[P2], max_len: f64) -> Ring {
    let mut out: Ring = vec![];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        out.push(a);
        let n = (dist(a, b) / max_len).ceil() as usize;
        for i in 1..n {
            let t = i as f64 / n as f64;
            out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}
pub fn segmentize(g: &G, max_len: f64) -> G {
    match g {
        G::Polygon(r) => G::Polygon(r.iter().map(|x| densify_chain(x, max_len)).collect()),
        G::MultiPolygon(ps) => G::MultiPolygon(ps.iter().map(|p| p.iter().map(|x| densify_chain(x, max_len)).collect()).collect()),
        G::LineString(l) => G::LineString(densify_chain(l, max_len)),
        G::LinearRing(l) => G::LinearRing(densify_chain(l, max_len)),
        G::MultiLineString(ls) => G::MultiLineString(ls.iter().map(|l| densify_chain(l, max_len)).collect()),
        _ => g.clone(),
    }
}

// ---------------------------------------------------------------- buffer

fn circle(c: P2, r: f64, quad_segs: i32) -> Ring {
    let n = (4 * quad_segs.max(1)) as usize;
    let mut v: Ring = (0..n)
        .map(|i| {
            let a = -(i as f64) * std::f64::consts::TAU / n as f64;
            // exact quadrant points
            let (s, co) = if i % (n / 4) == 0 { [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)][i / (n / 4)] } else { a.sin_cos() };
            [c[0] + r * co, c[1] + r * s]
        })
        .collect();
    v.push(v[0]);
    v
}

fn join_of(join: Join, quad_segs: i32) -> LineJoin<f64> {
    match join {
        Join::Round => LineJoin::Round(std::f64::consts::FRAC_PI_2 / quad_segs.max(1) as f64),
        Join::Mitre => LineJoin::Miter(2.0 * (1.0f64 / 5.0).asin()),
        Join::Bevel => LineJoin::Bevel,
    }
}

pub fn buffer_full(g: &G, d: f64, join: Join, cap: Cap, quad_segs: i32, single_sided: bool) -> G {
    match g {
        G::Point(p) => buffer_pt(*p, d, cap, quad_segs),
        G::MultiPoint(v) => union_all(&v.iter().map(|p| buffer_pt(*p, d, cap, quad_segs)).collect::<Vec<_>>()),
        G::Collection(gs) => union_all(&gs.iter().map(|x| buffer_full(x, d, join, cap, quad_segs, single_sided)).collect::<Vec<_>>()),
        G::Polygon(_) | G::MultiPolygon(_) => {
            let polys = polys_of(g);
            if polys.is_empty() {
                return G::Polygon(vec![]);
            }
            let cs = contours(&polys);
            if d == 0.0 {
                return from_shapes(cs.overlay_as::<i64>(&Vec::<Vec<P2>>::new(), OverlayRule::Union, FillRule::NonZero));
            }
            // clean first (overlapping / inverted input), then offset
            let clean = cs.overlay_as::<i64>(&Vec::<Vec<P2>>::new(), OverlayRule::Union, FillRule::NonZero);
            let flat: Vec<Vec<P2>> = clean.into_iter().flatten().collect();
            let st = OutlineStyle::new(d).line_join(join_of(join, quad_segs));
            from_shapes(flat.outline_as::<i64>(&st))
        }
        G::LineString(_) | G::LinearRing(_) | G::MultiLineString(_) => {
            if d <= 0.0 && !single_sided {
                return G::Polygon(vec![]);
            }
            let lines = lines_of(g);
            if single_sided {
                let side = if d >= 0.0 { 1.0 } else { -1.0 };
                let w = d.abs();
                let mut quads: Vec<Vec<Ring>> = vec![];
                for l in &lines {
                    for s in l.windows(2) {
                        let (a, b) = (s[0], s[1]);
                        let len = dist(a, b);
                        if len == 0.0 {
                            continue;
                        }
                        let n = [-(b[1] - a[1]) / len * side * w, (b[0] - a[0]) / len * side * w];
                        quads.push(vec![closed(&[a, b, [b[0] + n[0], b[1] + n[1]], [a[0] + n[0], a[1] + n[1]]])]);
                    }
                }
                return overlay(&quads, &[], OverlayRule::Union);
            }
            let lcap = match cap {
                Cap::Round => LineCap::Round(std::f64::consts::FRAC_PI_2 / quad_segs.max(1) as f64),
                Cap::Flat => LineCap::Butt,
                Cap::Square => LineCap::Square,
            };
            let mut all: Vec<Vec<Ring>> = vec![];
            for l in &lines {
                let is_ring = l.len() > 3 && l.first() == l.last();
                let path = if is_ring { opened(l) } else { l.clone() };
                let st = StrokeStyle::new(2.0 * d).start_cap(lcap.clone()).end_cap(lcap.clone()).line_join(join_of(join, quad_segs));
                for sh in path.stroke_as::<i64>(st, is_ring) {
                    all.push(sh.into_iter().map(|c| closed(&c)).collect());
                }
            }
            overlay(&all, &[], OverlayRule::Union)
        }
    }
}

fn buffer_pt(p: P2, d: f64, cap: Cap, quad_segs: i32) -> G {
    if d <= 0.0 {
        return G::Polygon(vec![]);
    }
    match cap {
        Cap::Round => G::Polygon(vec![circle(p, d, quad_segs)]),
        Cap::Square => G::Polygon(vec![closed(&[[p[0] + d, p[1] - d], [p[0] - d, p[1] - d], [p[0] - d, p[1] + d], [p[0] + d, p[1] + d]])]),
        Cap::Flat => G::Polygon(vec![]),
    }
}

pub fn buffer(g: &G, d: f64) -> G {
    buffer_full(g, d, Join::Round, Cap::Round, 16, false)
}
pub fn buffer_mitre(g: &G, d: f64) -> G {
    buffer_full(g, d, Join::Mitre, Cap::Flat, 16, false)
}
pub fn buffer_quad(g: &G, d: f64, quad_segs: i32) -> G {
    buffer_full(g, d, Join::Round, Cap::Round, quad_segs, false)
}

// ---------------------------------------------------------------- line work

/// all segments, noded against each other (split at every touch / crossing / collinear overlap), unique, sorted
fn node_lines(lines: &[Ring]) -> Vec<[P2; 2]> {
    let mut snap = Snap::default();
    let mut segs: Vec<[P2; 2]> = vec![];
    for l in lines {
        for w in l.windows(2) {
            let (a, b) = (snap.get(w[0]), snap.get(w[1]));
            if a != b {
                segs.push([a, b]);
            }
        }
    }
    let n = segs.len();
    let bb: Vec<(f64, f64, f64, f64)> = segs.iter().map(|s| (s[0][0].min(s[1][0]) - 1e-9, s[0][1].min(s[1][1]) - 1e-9, s[0][0].max(s[1][0]) + 1e-9, s[0][1].max(s[1][1]) + 1e-9)).collect();
    let mut out: Vec<[P2; 2]> = vec![];
    for i in 0..n {
        let [p, q] = segs[i];
        let d1 = sub(q, p);
        let l1 = dist(p, q);
        let mut pts: Vec<(f64, P2)> = vec![(0.0, p), (1.0, q)];
        for j in 0..n {
            if i == j || bb[i].0 > bb[j].2 || bb[j].0 > bb[i].2 || bb[i].1 > bb[j].3 || bb[j].1 > bb[i].3 {
                continue;
            }
            let [r, s] = segs[j];
            let d2 = sub(s, r);
            let l2 = dist(r, s);
            let den = cross(d1, d2);
            if den.abs() > 1e-12 * l1 * l2 {
                let t = cross(sub(r, p), d2) / den;
                let u = cross(sub(r, p), d1) / den;
                let tt = 1e-9 / l1;
                let uu = 1e-9 / l2;
                if t >= -tt && t <= 1.0 + tt && u >= -uu && u <= 1.0 + uu {
                    let pt = if u <= uu {
                        r
                    } else if u >= 1.0 - uu {
                        s
                    } else if t <= tt {
                        p
                    } else if t >= 1.0 - tt {
                        q
                    } else {
                        [p[0] + t * d1[0], p[1] + t * d1[1]]
                    };
                    pts.push((t.clamp(0.0, 1.0), snap.get(pt)));
                }
            } else if seg_dist(r, p, q) <= 1e-9 && seg_dist(s, p, q) <= 1e-9 {
                // collinear overlap: the other segment's endpoints split this one
                for e in [r, s] {
                    let t = dot(sub(e, p), d1) / (l1 * l1);
                    if t > 0.0 && t < 1.0 {
                        pts.push((t, e));
                    }
                }
            }
        }
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for w in pts.windows(2) {
            let (a, b) = (snap.get(w[0].1), snap.get(w[1].1));
            if a != b {
                out.push(if a <= b { [a, b] } else { [b, a] });
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out.dedup();
    out
}

/// merge chains of lines at nodes where exactly two line ends meet (JTS LineMerger)
fn merge_chains(lines: Vec<Ring>) -> Vec<Ring> {
    let mut snap = Snap::default();
    let lines: Vec<Ring> = lines.into_iter().filter(|l| l.len() >= 2).map(|l| l.into_iter().map(|p| snap.get(p)).collect()).collect();
    let mut adj: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new(); // node -> (line, end 0=start 1=end)
    for (i, l) in lines.iter().enumerate() {
        adj.entry(key(l[0])).or_default().push((i, 0));
        adj.entry(key(l[l.len() - 1])).or_default().push((i, 1));
    }
    let mut used = vec![false; lines.len()];
    let mut out = vec![];
    // extend `chain` (ending at its last point) as far as possible; returns false if it closed on `first`
    let extend = |chain: &mut Ring, used: &mut Vec<bool>, first: usize| {
        loop {
            let tail = *chain.last().unwrap();
            let Some(at) = adj.get(&key(tail)) else { break };
            if at.len() != 2 {
                break;
            }
            // the other line end at this node
            let cur_end = at.iter().filter(|&&(li, _)| used[li]).count();
            let _ = cur_end;
            let cand: Vec<&(usize, usize)> = at.iter().filter(|&&(li, _)| !used[li]).collect();
            let Some(&&(li, end)) = cand.first() else { break };
            if li == first {
                break;
            }
            used[li] = true;
            let l = &lines[li];
            if end == 0 {
                chain.extend(l.iter().skip(1));
            } else {
                chain.extend(l.iter().rev().skip(1));
            }
        }
    };
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut chain = lines[i].clone();
        extend(&mut chain, &mut used, usize::MAX);
        chain.reverse();
        extend(&mut chain, &mut used, usize::MAX);
        out.push(chain);
    }
    out
}

pub fn line_merge(g: &G) -> G {
    lines_to_g(merge_chains(lines_of(g)))
}

/// pieces of `a`'s segments covered by `b`'s segments: (piece, same direction)
fn overlaps(a: &[Ring], b: &[Ring]) -> Vec<Vec<(Ring, bool)>> {
    let mut res = vec![];
    for l in a {
        let mut per: Vec<(Ring, bool)> = vec![];
        for w in l.windows(2) {
            let (p, q) = (w[0], w[1]);
            let d = sub(q, p);
            let len = dist(p, q);
            if len == 0.0 {
                continue;
            }
            let mut iv: Vec<(f64, f64, bool)> = vec![];
            for m in b {
                for v in m.windows(2) {
                    let (r, s) = (v[0], v[1]);
                    if seg_dist(r, p, q) > 1e-9 && seg_dist(s, p, q) > 1e-9 && seg_dist(p, r, s) > 1e-9 {
                        continue;
                    }
                    // collinear?
                    let dr = cross(d, sub(r, p)).abs() / len;
                    let ds = cross(d, sub(s, p)).abs() / len;
                    if dr > 1e-9 || ds > 1e-9 {
                        continue;
                    }
                    let (t0, t1) = (dot(sub(r, p), d) / (len * len), dot(sub(s, p), d) / (len * len));
                    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
                    if hi - lo > 1e-9 / len {
                        iv.push((lo, hi, t1 > t0));
                    }
                }
            }
            iv.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for (lo, hi, fwd) in iv {
                let pt = |t: f64| if t <= 0.0 { p } else if t >= 1.0 { q } else { [p[0] + t * d[0], p[1] + t * d[1]] };
                per.push((vec![pt(lo), pt(hi)], fwd));
            }
        }
        res.push(per);
    }
    res
}

/// chain consecutive pieces (end == next start)
fn chain_pieces(pieces: Vec<Ring>) -> Vec<Ring> {
    let mut out: Vec<Ring> = vec![];
    for pc in pieces {
        if let Some(l) = out.last_mut() {
            if near(*l.last().unwrap(), pc[0]) {
                l.extend(pc.into_iter().skip(1));
                continue;
            }
        }
        out.push(pc);
    }
    out
}

pub fn shared_paths(a: &G, b: &G) -> (G, G) {
    let (la, lb) = (lines_of(a), lines_of(b));
    let (mut fw, mut bw) = (vec![], vec![]);
    for per in overlaps(&la, &lb) {
        let mut f = vec![];
        let mut k = vec![];
        let mut seen: Vec<Ring> = vec![];
        for (pc, same) in per {
            if seen.iter().any(|s| near(s[0], pc[0]) && near(s[1], pc[1])) {
                continue;
            }
            seen.push(pc.clone());
            if same {
                f.push(pc)
            } else {
                k.push(pc)
            }
        }
        fw.extend(chain_pieces(f));
        bw.extend(chain_pieces(k));
    }
    (G::MultiLineString(fw), G::MultiLineString(bw))
}

fn line_difference(a: &[Ring], b: &[Ring]) -> Vec<Ring> {
    let ov = overlaps(a, b);
    let mut out = vec![];
    for (l, per) in a.iter().zip(ov) {
        // covered intervals per segment index, recomputed by walking the segments
        let mut pieces: Vec<Ring> = vec![];
        for w in l.windows(2) {
            let (p, q) = (w[0], w[1]);
            let d = sub(q, p);
            let len = dist(p, q);
            if len == 0.0 {
                continue;
            }
            let mut cov: Vec<(f64, f64)> = per
                .iter()
                .filter_map(|(pc, _)| {
                    let (s, e) = (pc[0], pc[1]);
                    if seg_dist(s, p, q) > 1e-9 || seg_dist(e, p, q) > 1e-9 || cross(d, sub(s, p)).abs() / len > 1e-9 || cross(d, sub(e, p)).abs() / len > 1e-9 {
                        return None;
                    }
                    let (t0, t1) = (dot(sub(s, p), d) / (len * len), dot(sub(e, p), d) / (len * len));
                    Some((t0.min(t1), t0.max(t1)))
                })
                .collect();
            cov.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let pt = |t: f64| if t <= 0.0 { p } else if t >= 1.0 { q } else { [p[0] + t * d[0], p[1] + t * d[1]] };
            let mut cur = 0.0;
            for (lo, hi) in cov {
                if lo - cur > 1e-9 / len {
                    pieces.push(vec![pt(cur), pt(lo)]);
                }
                cur = cur.max(hi);
            }
            if 1.0 - cur > 1e-9 / len {
                pieces.push(vec![pt(cur), pt(1.0)]);
            }
        }
        let mut chains = chain_pieces(pieces);
        // a ring whose start vertex lies in an uncovered stretch: join the last and first chain
        if chains.len() >= 2 && l.first() == l.last() {
            let first = chains[0].clone();
            let last = chains.last_mut().unwrap();
            if near(*last.last().unwrap(), first[0]) {
                last.extend(first.into_iter().skip(1));
                chains.remove(0);
            }
        }
        out.extend(chains);
    }
    out
}

/// parts of `lines` inside (keep_inside) or outside the polygons (boundary counts as inside)
fn clip_lines(lines: &[Ring], polys: &[Vec<Ring>], keep_inside: bool) -> Vec<Ring> {
    let mut pieces = vec![];
    for l in lines {
        let mut per = vec![];
        for w in l.windows(2) {
            let (p, q) = (w[0], w[1]);
            let d = sub(q, p);
            let len = dist(p, q);
            if len == 0.0 {
                continue;
            }
            let mut ts = vec![0.0, 1.0];
            for pl in polys {
                for r in pl {
                    for v in r.windows(2) {
                        let (r0, s0) = (v[0], v[1]);
                        let e = sub(s0, r0);
                        let den = cross(d, e);
                        if den.abs() > 1e-12 * len * dist(r0, s0) {
                            let t = cross(sub(r0, p), e) / den;
                            let u = cross(sub(r0, p), d) / den;
                            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                                ts.push(t);
                            }
                        }
                    }
                }
            }
            ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for tw in ts.windows(2) {
                if tw[1] - tw[0] <= 1e-9 / len {
                    continue;
                }
                let tm = (tw[0] + tw[1]) / 2.0;
                let m = [p[0] + tm * d[0], p[1] + tm * d[1]];
                let inside = pt_in_polys(m, polys) > 0;
                if inside == keep_inside {
                    per.push(vec![[p[0] + tw[0] * d[0], p[1] + tw[0] * d[1]], [p[0] + tw[1] * d[0], p[1] + tw[1] * d[1]]]);
                }
            }
        }
        pieces.extend(chain_pieces(per));
    }
    pieces
}

/// faces of the planar graph formed by the (noded) linework
pub fn polygonize(g: &G) -> Vec<G> {
    let lines = lines_of(g);
    let segs = node_lines(&lines);
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut pos: Vec<P2> = vec![];
    let mut edges: Vec<(usize, usize)> = vec![];
    for s in &segs {
        let mut id = |p: P2| {
            *ids.entry(key(p)).or_insert_with(|| {
                pos.push(p);
                pos.len() - 1
            })
        };
        let (a, b) = (id(s[0]), id(s[1]));
        edges.push((a, b));
    }
    let n = pos.len();
    // prune dangles
    let mut alive = vec![true; edges.len()];
    loop {
        let mut deg = vec![0usize; n];
        for (i, e) in edges.iter().enumerate() {
            if alive[i] {
                deg[e.0] += 1;
                deg[e.1] += 1;
            }
        }
        let mut changed = false;
        for (i, e) in edges.iter().enumerate() {
            if alive[i] && (deg[e.0] < 2 || deg[e.1] < 2) {
                alive[i] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut nbr: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, e) in edges.iter().enumerate() {
        if alive[i] {
            nbr[e.0].push(e.1);
            nbr[e.1].push(e.0);
        }
    }
    for (v, l) in nbr.iter_mut().enumerate() {
        l.sort_by(|&a, &b| {
            let (da, db) = (sub(pos[a], pos[v]), sub(pos[b], pos[v]));
            da[1].atan2(da[0]).partial_cmp(&db[1].atan2(db[0])).unwrap()
        });
    }
    // connected components
    let mut comp = vec![usize::MAX; n];
    let mut nc = 0;
    for s in 0..n {
        if comp[s] != usize::MAX || nbr[s].is_empty() {
            continue;
        }
        let mut st = vec![s];
        comp[s] = nc;
        while let Some(v) = st.pop() {
            for &w in &nbr[v] {
                if comp[w] == usize::MAX {
                    comp[w] = nc;
                    st.push(w);
                }
            }
        }
        nc += 1;
    }
    let mut visited: HashMap<(usize, usize), bool> = HashMap::new();
    let mut faces: Vec<(Ring, f64, usize)> = vec![]; // positive cycles
    let mut negs: Vec<(Ring, usize)> = vec![];
    for u in 0..n {
        for &v in &nbr[u] {
            if visited.contains_key(&(u, v)) {
                continue;
            }
            let mut cyc = vec![];
            let (mut a, mut b) = (u, v);
            loop {
                visited.insert((a, b), true);
                cyc.push(pos[a]);
                let l = &nbr[b];
                let k = l.iter().position(|&x| x == a).unwrap();
                let c = l[(k + l.len() - 1) % l.len()];
                a = b;
                b = c;
                if (a, b) == (u, v) || visited.contains_key(&(a, b)) {
                    break;
                }
            }
            let ar = ring_area(&cyc);
            if ar > 1e-12 {
                faces.push((cyc, ar, comp[u]));
            } else if ar < -1e-12 {
                negs.push((cyc, comp[u]));
            }
        }
    }
    let mut holes: Vec<Vec<Ring>> = vec![vec![]; faces.len()];
    for (cyc, c) in &negs {
        let p = cyc[0];
        let mut best: Option<usize> = None;
        for (fi, f) in faces.iter().enumerate() {
            if f.2 == *c {
                continue;
            }
            if pt_in_ring(p, &f.0) == 1 && best.map_or(true, |b| f.1 < faces[b].1) {
                best = Some(fi);
            }
        }
        if let Some(b) = best {
            holes[b].push(cyc.clone());
        }
    }
    let mut out: Vec<(P2, G)> = vec![];
    for (fi, (cyc, _, _)) in faces.iter().enumerate() {
        let mut shell = cyc.clone();
        shell.reverse(); // clockwise
        let mut rings = vec![closed(&shell)];
        for h in &holes[fi] {
            let mut hh = h.clone();
            hh.reverse(); // negative cycles are clockwise; holes ccw
            rings.push(closed(&hh));
        }
        let mn = shell.iter().cloned().fold([f64::MAX, f64::MAX], |m, p| if p < m { p } else { m });
        out.push((mn, G::Polygon(rings)));
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out.into_iter().map(|x| x.1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x0: f64, y0: f64, x1: f64, y1: f64) -> G {
        polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], &[]).unwrap()
    }

    #[test]
    fn booleans() {
        let a = sq(0.0, 0.0, 4.0, 3.0);
        let b = sq(2.0, 1.0, 6.0, 5.0);
        assert!((area(&union(&a, &b)) - (12.0 + 16.0 - 4.0)).abs() < 1e-9);
        assert!((area(&intersection(&a, &b)) - 4.0).abs() < 1e-9);
        assert!((area(&difference(&a, &b)) - 8.0).abs() < 1e-9);
        assert!(contains(&a, &sq(1.0, 1.0, 2.0, 2.0)));
        assert!(!contains(&a, &b));
        assert!(within(&sq(1.0, 1.0, 2.0, 2.0), &a));
        assert_eq!(distance(&a, &sq(5.0, 0.0, 6.0, 1.0)), 1.0);
        assert_eq!(distance(&a, &b), 0.0);
        let c = centroid(&a);
        assert!((c[0] - 2.0).abs() < 1e-12 && (c[1] - 1.5).abs() < 1e-12);
        assert!(is_valid(&a));
        let bow = polygon(&[[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]], &[]).unwrap();
        assert!(!is_valid(&bow));
    }

    #[test]
    fn buffers() {
        let a = sq(0.0, 0.0, 4.0, 3.0);
        let g = buffer_mitre(&a, 0.5);
        assert!((area(&g) - 5.0 * 4.0).abs() < 1e-9, "{}", area(&g));
        let g = buffer_mitre(&a, -0.5);
        assert!((area(&g) - 3.0 * 2.0).abs() < 1e-9);
        assert!(is_empty(&buffer_mitre(&a, -2.0)));
        let c = buffer(&point(0.0, 0.0), 1.0);
        assert!((area(&c) - std::f64::consts::PI).abs() < 0.02);
        assert!(is_polygon(&buffer(&buffer(&a, 0.3), -0.3)));
        let l = linestring(&[[0.0, 0.0], [4.0, 0.0]]).unwrap();
        let s = buffer_full(&l, 0.5, Join::Round, Cap::Flat, 16, true);
        assert!((area(&s) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn line_work() {
        let a = sq(0.0, 0.0, 2.0, 2.0);
        let b = sq(2.0, 0.0, 4.0, 2.0);
        let (f, bk) = shared_paths(&boundary(&a), &boundary(&b));
        assert!((length(&f) + length(&bk) - 2.0).abs() < 1e-9);
        let sh = if length(&f) > 0.0 { f } else { bk };
        let l = difference(&boundary(&a), &sh);
        assert!((length(&l) - 6.0).abs() < 1e-9, "{}", length(&l));
        // polygonize: square cut by a line
        let cut = linestring(&[[1.0, -1.0], [1.0, 3.0]]).unwrap();
        let mut ls = parts(&boundary(&a));
        ls.push(cut);
        let m = unary_union(&multilinestring(ls).unwrap());
        let ps = polygonize(&m);
        assert_eq!(ps.len(), 2);
        assert!((ps.iter().map(area).sum::<f64>() - 4.0).abs() < 1e-9);
        let merged = line_merge(&multilinestring(vec![linestring(&[[0.0, 0.0], [1.0, 0.0]]).unwrap(), linestring(&[[1.0, 0.0], [2.0, 0.0]]).unwrap()]).unwrap());
        assert!(matches!(merged, G::LineString(_)));
    }
}
