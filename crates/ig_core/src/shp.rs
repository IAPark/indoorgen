//! Shapely-flavoured 2D geometry helpers, so ports of the Python geometry code stay line-for-line.
//! Backend: GEOS (C++) natively (`shp_geos.rs`); on wasm32 (or with the `pure_geom` feature) the pure-Rust backend `shp_pure.rs`
//! (i_overlay + own line work). Both export the same functions; everything below the re-export is backend independent.
pub type P2 = [f64; 2];

#[derive(Clone, Copy, PartialEq)]
pub enum Join {
    Round,
    Mitre,
    Bevel,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Cap {
    Round,
    Flat,
    Square,
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "shp_geos.rs"]
pub mod geos_backend;
#[path = "shp_pure.rs"]
pub mod pure;

#[cfg(not(any(target_arch = "wasm32", feature = "pure_geom")))]
pub use geos_backend::*;
#[cfg(any(target_arch = "wasm32", feature = "pure_geom"))]
pub use pure::*;

pub fn bbox(x0: f64, y0: f64, x1: f64, y1: f64) -> G {
    polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], &[]).unwrap()
}

pub fn signed_area(c: &[P2]) -> f64 {
    let mut s = 0.0;
    for i in 0..c.len() - 1 {
        s += c[i][0] * c[i + 1][1] - c[i + 1][0] * c[i][1];
    }
    s / 2.0
}

/// shapely.ops.orient(sign=1.0): exterior CCW, interiors CW
pub fn orient(g: &G) -> G {
    let ccw = |c: &Vec<P2>| signed_area(c) > 0.0;
    let mut e = exterior(g);
    if !ccw(&e) {
        e.reverse();
    }
    let hs: Vec<Vec<P2>> = interiors(g)
        .into_iter()
        .map(|mut h| {
            if ccw(&h) {
                h.reverse();
            }
            h
        })
        .collect();
    polygon(&e, &hs).unwrap()
}

pub fn translate(g: &G, dx: f64, dy: f64) -> G {
    map_coords(g, |p| [p[0] + dx, p[1] + dy])
}

/// shapely.affinity.rotate(g, angle_deg) about bbox centre (origin='center')
pub fn rotate_center(g: &G, deg: f64) -> G {
    let (x0, y0, x1, y1) = bounds(g);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    map_coords(g, |p| {
        let (x, y) = (p[0] - cx, p[1] - cy);
        [cx + x * c - y * s, cy + x * s + y * c]
    })
}

pub fn map_coords(g: &G, f: impl Fn(P2) -> P2 + Copy) -> G {
    match gtype(g) {
        GeometryTypes::Polygon => {
            let e: Vec<P2> = exterior(g).into_iter().map(f).collect();
            let h: Vec<Vec<P2>> = interiors(g).into_iter().map(|r| r.into_iter().map(f).collect()).collect();
            polygon(&e, &h).unwrap()
        }
        GeometryTypes::LineString | GeometryTypes::LinearRing => {
            let c: Vec<P2> = coords_of(g).into_iter().map(f).collect();
            linestring(&c).unwrap()
        }
        GeometryTypes::MultiLineString => {
            multilinestring(parts(g).iter().map(|p| map_coords(p, f)).collect()).unwrap()
        }
        GeometryTypes::MultiPolygon => {
            let ps: Vec<G> = parts(g).iter().map(|p| map_coords(p, f)).collect();
            multipolygon(ps).unwrap()
        }
        GeometryTypes::Point => {
            let c = coords_of(g)[0];
            let q = f(c);
            point(q[0], q[1])
        }
        _ => g.clone(),
    }
}

/// shapely.affinity.rotate(g, deg, origin=(0,0))
pub fn rotate_origin(g: &G, deg: f64) -> G {
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    map_coords(g, |p| [p[0] * c - p[1] * s, p[0] * s + p[1] * c])
}

/// Fast path of `within(a, buffer(b, d))`: when `b` is a single convex polygon without holes, `a` lies within the d-offset of `b` iff all
/// vertices of `a` satisfy the offset half-planes of `b` (the rounded buffer corners are ignored: <= 0.41 d more permissive at corners).
/// `None` when `b` is not a convex polygon (caller falls back to the exact test).
pub fn within_convex_offset(a: &G, b: &G, d: f64) -> Option<bool> {
    if !matches!(gtype(b), GeometryTypes::Polygon) || !interiors(b).is_empty() {
        return None;
    }
    let mut r = exterior(b);
    if r.len() > 1 && r.first() == r.last() {
        r.pop();
    }
    let n = r.len();
    if n < 3 {
        return None;
    }
    let mut area2 = 0.0;
    for i in 0..n {
        let (p, q) = (r[i], r[(i + 1) % n]);
        area2 += p[0] * q[1] - q[0] * p[1];
    }
    if area2 == 0.0 {
        return None;
    }
    let sgn = if area2 > 0.0 { 1.0 } else { -1.0 };
    // convexity: all turns have the sign of the area
    for i in 0..n {
        let (p, q, w) = (r[i], r[(i + 1) % n], r[(i + 2) % n]);
        let c = (q[0] - p[0]) * (w[1] - q[1]) - (q[1] - p[1]) * (w[0] - q[0]);
        if c * sgn < -1e-12 {
            return None;
        }
    }
    let pts: Vec<P2> = match gtype(a) {
        GeometryTypes::Polygon => exterior(a),
        GeometryTypes::MultiPolygon => parts(a).iter().flat_map(|p| exterior(p)).collect(),
        _ => return None,
    };
    for &v in &pts {
        for i in 0..n {
            let (p, q) = (r[i], r[(i + 1) % n]);
            let (ex, ey) = (q[0] - p[0], q[1] - p[1]);
            let len = (ex * ex + ey * ey).sqrt();
            if len == 0.0 {
                continue;
            }
            // signed distance to the left of the edge (inside for CCW), flipped for CW
            let dist = sgn * (ex * (v[1] - p[1]) - ey * (v[0] - p[0])) / len;
            if dist < -d - 1e-9 {
                return Some(false);
            }
        }
    }
    Some(true)
}

/// `contains_pt(buffer(g, d), p)` without building the buffer: p inside the polygon(s) or within `d` of a ring (exact distance instead of the
/// polygonal approximation of the round join)
pub fn within_distance_pt(g: &G, p: P2, d: f64) -> bool {
    if contains_pt(g, p) {
        return true;
    }
    let seg_dist = |a: P2, b: P2| -> f64 {
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let l2 = ex * ex + ey * ey;
        let t = if l2 == 0.0 { 0.0 } else { (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / l2).clamp(0.0, 1.0) };
        ((p[0] - a[0] - t * ex).powi(2) + (p[1] - a[1] - t * ey).powi(2)).sqrt()
    };
    for part in parts(g) {
        if !matches!(gtype(&part), GeometryTypes::Polygon) {
            continue;
        }
        let mut rings = vec![exterior(&part)];
        rings.extend(interiors(&part));
        for r in rings {
            for w in r.windows(2) {
                if seg_dist(w[0], w[1]) <= d {
                    return true;
                }
            }
        }
    }
    false
}
