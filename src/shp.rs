//! Shapely-flavoured helpers over GEOS, so ports of the Python geometry code stay line-for-line.
use anyhow::{anyhow, Result};
use geos::{BufferParams, CapStyle, CoordSeq, Geom, Geometry, GeometryTypes, JoinStyle};

pub type G = Geometry;
pub type P2 = [f64; 2];

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

pub fn bbox(x0: f64, y0: f64, x1: f64, y1: f64) -> G {
    polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], &[]).unwrap()
}

pub fn linestring(c: &[P2]) -> Result<G> {
    err(Geometry::create_line_string(seq(c)?))
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
            Geometry::create_multipolygon(ps).unwrap()
        }
        GeometryTypes::Point => {
            let c = coords_of(g)[0];
            let q = f(c);
            point(q[0], q[1])
        }
        _ => Clone::clone(g),
    }
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

/// shapely.affinity.rotate(g, deg, origin=(0,0))
pub fn rotate_origin(g: &G, deg: f64) -> G {
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    map_coords(g, |p| [p[0] * c - p[1] * s, p[0] * s + p[1] * c])
}
