//! RoomConstants (infinigen/core/constraints/constraint_language/constants.py), n_stories fixed to 1.
use crate::rng::Rng;
use crate::shp::*;

#[derive(Clone, Debug)]
pub struct RoomConstants {
    pub n_stories: usize,
    pub unit: f64,
    pub segment_margin: f64,
    pub wall_thickness: f64,
    pub wall_height: f64,
    pub door_width: f64,
    pub door_margin: f64,
    pub door_size: f64,
    pub max_window_length: f64,
    pub window_height: f64,
    pub window_margin: f64,
    pub window_size: f64,
    pub window_top: f64,
    pub staircase_snap: f64,
    pub staircase_thresh: f64,
    pub fixed_contour: bool,
}

/// Raised where the python raises NotImplementedError (caught by the solver as "try again").
#[derive(Debug)]
pub struct NotImpl(pub &'static str);
pub type Ni<T> = Result<T, NotImpl>;

impl RoomConstants {
    /// Draw order follows the python constructor: global, door, window params.
    pub fn sample(rng: &mut Rng, n_stories: usize) -> Self {
        let unit = 0.5;
        let segment_margin = 1.4;
        let wall_thickness = rng.uniform(0.2, 0.3);
        let wall_height = rng.uniform(2.8, 3.2);
        let door_width_ratio = rng.uniform(0.7, 0.8);
        let door_size = rng.uniform(2.0, 2.4);
        let door_width = (segment_margin - wall_thickness) * door_width_ratio;
        let door_margin = (segment_margin - door_width) / 2.0;
        let max_window_length = rng.uniform(6.0, 8.0);
        let window_height = rng.uniform(0.8, 1.2);
        let window_size = rng.uniform(1.0, 1.5);
        let window_margin = rng.uniform(0.2, 0.25);
        let mut window_top = wall_height - wall_thickness - window_height - window_size;
        window_top = window_top.max(wall_thickness / 2.0);
        let window_size = wall_height - wall_thickness - window_top - window_height;
        RoomConstants {
            n_stories,
            unit,
            segment_margin,
            wall_thickness,
            wall_height,
            door_width,
            door_margin,
            door_size,
            max_window_length,
            window_height,
            window_margin,
            window_size,
            window_top,
            staircase_snap: 1.2,
            staircase_thresh: 0.6,
            fixed_contour: false,
        }
    }

    pub fn unit_cast(&self, x: f64) -> f64 {
        (x / self.unit).round_ties_even() * self.unit
    }

    /// RoomConstants.canonicalize
    pub fn canonicalize(&self, p: &G) -> Ni<G> {
        let mut p = buffer(p, 0.0);
        loop {
            let ps = simplify_polygon(&p);
            if area(&p) == 0.0 {
                return Err(NotImpl("Polygon empty."));
            }
            if !is_polygon(&ps) {
                return Err(NotImpl("Invalid multi polygon"));
            }
            let po = orient(&ps);
            let mut coords = exterior(&po);
            let l = coords.len();
            for c in coords.iter_mut() {
                let r = [self.unit_cast(c[0]), self.unit_cast(c[1])];
                if (c[0] - r[0]).abs() < 1e-3 && (c[1] - r[1]).abs() < 1e-3 {
                    *c = r;
                }
            }
            let n = coords.len() - 1;
            let mut diff: Vec<P2> = Vec::with_capacity(n);
            for i in 0..n {
                let d = [coords[i + 1][0] - coords[i][0], coords[i + 1][1] - coords[i][1]];
                let nrm = (d[0] * d[0] + d[1] * d[1]).sqrt() + 1e-6;
                diff.push([d[0] / nrm, d[1] / nrm]);
            }
            let mut invalid = vec![];
            for i in 0..n {
                let prev = diff[(i + n - 1) % n];
                let prod = prev[0] * diff[i][0] + prev[1] * diff[i][1];
                if prod < -0.8 || prod > 1.0 - 1e-6 {
                    invalid.push(i);
                }
            }
            let mut valid: Vec<usize> = (0..n).collect();
            if !invalid.is_empty() {
                let i = invalid[invalid.len() / 2];
                valid.retain(|&v| v != i);
            }
            if valid.len() < 3 {
                return Err(NotImpl("degenerate"));
            }
            let mut ring: Vec<P2> = valid.iter().map(|&i| coords[i]).collect();
            ring.push(ring[0]);
            let q = match polygon(&ring, &[]) {
                Ok(q) => q,
                Err(_) => return Err(NotImpl("bad ring")),
            };
            p = q;
            if exterior(&p).len() == l {
                break;
            }
        }
        if !is_valid_polygon(&p) {
            return Err(NotImpl("Invalid polygon"));
        }
        Ok(orient(&p))
    }

    pub fn filter(&self, ses: &[(usize, G)], margin: Option<f64>) -> Vec<usize> {
        let margin = margin.unwrap_or(self.segment_margin);
        ses.iter().filter(|(_, se)| segment_filter(se, margin)).map(|(k, _)| *k).collect()
    }
}

// ---- infinigen/assets/utils/shapes.py ----

pub fn is_valid_polygon(p: &G) -> bool {
    is_polygon(p) && area(p) > 0.0 && is_valid(p) && interiors(p).is_empty()
}

pub fn simplify_polygon(p: &G) -> G {
    remove_repeated_points(&normalize(&simplify(p, 1e-6)), 0.01)
}

/// segment_filter: does any segment of any linestring exceed `margin`?
pub fn segment_filter(mls: &G, margin: f64) -> bool {
    for ls in parts(mls) {
        if is_empty(&ls) {
            continue;
        }
        let c = coords_of(&ls);
        if c.len() < 2 {
            continue;
        }
        for w in c.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            if d > margin {
                return true;
            }
        }
    }
    false
}

/// cut_polygon_by_line(polygon, line) -> polygons (order as GEOS polygonize yields)
pub fn cut_polygon_by_line(polygon: &G, line: &G) -> Vec<G> {
    let mut lines = parts(&boundary(polygon));
    lines.push(Clone::clone(line));
    let merged = line_merge(&multilinestring(lines).unwrap());
    let borders = unary_union(&merged);
    polygonize(&borders)
}
