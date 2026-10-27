//! Shapely-flavoured helpers over GEOS, so ports of the Python geometry code stay line-for-line (native backend).
use super::{Cap, Join, P2};
use anyhow::{anyhow, Result};
use geos::{BufferParams, CapStyle, CoordSeq, Geom, Geometry, JoinStyle};
pub use geos::GeometryTypes;

pub type G = Geometry;

pub fn err<T>(r: geos::GResult<T>) -> Result<T> {
    r.map_err(|e| anyhow!("geos: {e}"))
}

fn seq(coords: &[P2]) -> Result<CoordSeq> {
    let v: Vec<Vec<f64>> = coords.iter().map(|c| c.to_vec()).collect();
    err(CoordSeq::new_from_vec(&v))
}

pub fn polygon(ext: &[P2], holes: &[Vec<P2>]) -> Result<G> {
    let mut e = ext.to_vec();
    if e.first() != e.last() {
        e.push(e[0]);
    }
    let shell = err(Geometry::create_linear_ring(seq(&e)?))?;
    let mut hs = vec![];
    for h in holes {
        let mut hh = h.clone();
        if hh.first() != hh.last() {
            hh.push(hh[0]);
        }
        hs.push(err(Geometry::create_linear_ring(seq(&hh)?))?);
    }
    err(Geometry::create_polygon(shell, hs))
}

pub fn linestring(c: &[P2]) -> Result<G> {
    err(Geometry::create_line_string(seq(c)?))
}

pub fn linearring(c: &[P2]) -> Result<G> {
    err(Geometry::create_linear_ring(seq(c)?))
}

pub fn multipolygon(ps: Vec<G>) -> Result<G> {
    err(Geometry::create_multipolygon(ps))
}

pub fn multilinestring(lines: Vec<G>) -> Result<G> {
    err(Geometry::create_multiline_string(lines))
}

pub fn empty_mls() -> G {
    Geometry::create_multiline_string(vec![]).unwrap()
}

pub fn point(x: f64, y: f64) -> G {
    Geometry::create_point(CoordSeq::new_from_vec(&[vec![x, y]]).unwrap()).unwrap()
}

pub fn coords_of(g: &G) -> Vec<P2> {
    let cs = g.get_coord_seq().unwrap();
    let n = cs.size().unwrap();
    (0..n).map(|i| [cs.get_x(i).unwrap(), cs.get_y(i).unwrap()]).collect()
}

pub fn exterior(g: &G) -> Vec<P2> {
    coords_of(&Geom::clone(&g.get_exterior_ring().unwrap()).unwrap())
}

pub fn interiors(g: &G) -> Vec<Vec<P2>> {
    let n = g.get_num_interior_rings().unwrap();
    (0..n).map(|i| coords_of(&Geom::clone(&g.get_interior_ring_n(i).unwrap()).unwrap())).collect()
}

pub fn gtype(g: &G) -> GeometryTypes {
    g.geometry_type().unwrap()
}

pub fn is_polygon(g: &G) -> bool {
    matches!(gtype(g), GeometryTypes::Polygon)
}

/// Sub-geometries of a multi/collection (or the geometry itself).
pub fn parts(g: &G) -> Vec<G> {
    match gtype(g) {
        GeometryTypes::MultiPolygon
        | GeometryTypes::MultiLineString
        | GeometryTypes::GeometryCollection
        | GeometryTypes::MultiPoint => {
            let n = g.get_num_geometries().unwrap();
            (0..n).map(|i| Geom::clone(&g.get_geometry_n(i).unwrap()).unwrap()).collect()
        }
        _ => vec![Clone::clone(g)],
    }
}

pub fn bounds(g: &G) -> (f64, f64, f64, f64) {
    (
        g.get_x_min().unwrap(),
        g.get_y_min().unwrap(),
        g.get_x_max().unwrap(),
        g.get_y_max().unwrap(),
    )
}

pub fn area(g: &G) -> f64 {
    g.area().unwrap()
}
pub fn length(g: &G) -> f64 {
    g.length().unwrap()
}
pub fn union(a: &G, b: &G) -> G {
    a.union(b).unwrap()
}
pub fn difference(a: &G, b: &G) -> G {
    a.difference(b).unwrap()
}
pub fn intersection(a: &G, b: &G) -> G {
    a.intersection(b).unwrap()
}
pub fn union_all(gs: &[G]) -> G {
    if gs.is_empty() {
        return Geometry::create_geometry_collection(vec![]).unwrap();
    }
    let coll = Geometry::create_geometry_collection(gs.to_vec()).unwrap();
    coll.unary_union().unwrap()
}

pub fn boundary(g: &G) -> G {
    g.boundary().unwrap()
}
pub fn centroid(g: &G) -> P2 {
    let c = g.get_centroid().unwrap();
    [c.get_x().unwrap(), c.get_y().unwrap()]
}
pub fn convex_hull(g: &G) -> G {
    g.convex_hull().unwrap()
}
pub fn is_valid(g: &G) -> bool {
    g.is_valid().unwrap_or(false)
}
pub fn contains(a: &G, b: &G) -> bool {
    a.contains(b).unwrap()
}
pub fn distance(a: &G, b: &G) -> f64 {
    a.distance(b).unwrap()
}
pub fn normalize(g: &G) -> G {
    let mut c = Clone::clone(g);
    c.normalize().unwrap();
    c
}

/// shapely.simplify(g, tol) (preserve_topology=True)
pub fn simplify(g: &G, tol: f64) -> G {
    g.topology_preserve_simplify(tol).unwrap()
}

pub fn remove_repeated_points(g: &G, tol: f64) -> G {
    g.remove_repeated_points(tol).unwrap()
}

pub fn segmentize(g: &G, max_len: f64) -> G {
    g.densify(max_len).unwrap()
}

pub fn buffer_full(g: &G, d: f64, join: Join, cap: Cap, quad_segs: i32, single_sided: bool) -> G {
    let p = BufferParams::builder()
        .quadrant_segments(quad_segs)
        .end_cap_style(match cap {
            Cap::Round => CapStyle::Round,
            Cap::Flat => CapStyle::Flat,
            Cap::Square => CapStyle::Square,
        })
        .join_style(match join {
            Join::Round => JoinStyle::Round,
            Join::Mitre => JoinStyle::Mitre,
            Join::Bevel => JoinStyle::Bevel,
        })
        .mitre_limit(5.0)
        .single_sided(single_sided)
        .build()
        .unwrap();
    g.buffer_with_params(d, &p).unwrap()
}

/// shapely default buffer(d)
pub fn buffer(g: &G, d: f64) -> G {
    buffer_full(g, d, Join::Round, Cap::Round, 16, false)
}
pub fn buffer_mitre(g: &G, d: f64) -> G {
    buffer_full(g, d, Join::Mitre, Cap::Flat, 16, false)
}

pub fn line_merge(g: &G) -> G {
    g.line_merge().unwrap()
}

/// shapely.ops.shared_paths(a, b) -> (forward, backward)
pub fn shared_paths(a: &G, b: &G) -> (G, G) {
    let r = a.shared_paths(b).unwrap();
    let f = Geom::clone(&r.get_geometry_n(0).unwrap()).unwrap();
    let bk = Geom::clone(&r.get_geometry_n(1).unwrap()).unwrap();
    (f, bk)
}

pub fn is_empty(g: &G) -> bool {
    g.is_empty().unwrap()
}

pub fn within(a: &G, b: &G) -> bool {
    a.within(b).unwrap_or(false)
}

pub fn contains_pt(poly: &G, p: P2) -> bool {
    poly.contains(&point(p[0], p[1])).unwrap_or(false)
}


/// `g.buffer(d, quad_segs)` with GEOS's default style (round cap and join)
pub fn buffer_quad(g: &G, d: f64, quad_segs: i32) -> G {
    g.buffer(d, quad_segs).unwrap()
}

pub fn unary_union(g: &G) -> G {
    g.unary_union().unwrap()
}

/// faces of the planar graph formed by the (noded) linework, as GEOS polygonize yields them
pub fn polygonize(g: &G) -> Vec<G> {
    let polys = Geometry::polygonize(&[Clone::clone(g)]).unwrap();
    parts(&polys)
}

/// `a.contains(b)`, false when GEOS raises (invalid input)
pub fn try_contains(a: &G, b: &G) -> bool {
    a.contains(b).unwrap_or(false)
}
