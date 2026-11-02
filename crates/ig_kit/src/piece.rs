//! Pieces: every asset as one object that owns its state from the solver's first proposal to the final scene. A piece says what it
//! occupies (`Collide`: volumes from its parameters, no mesh built), what it looks like (`Build`: the full model, same frame and size)
//! and whether it can be another size (`Piece::resize`). Bounds, 2D footprints and door-swing hulls are derived from the volumes.
//!
//! The contract (checked by the `colcheck` tool): every vertex of a rigid mesh part lies inside the hard volumes (1 mm), every soft
//! part inside hard + soft, every opening leaf swept fully open inside its opening volume + hard; hard volumes should not stand more
//! than 3 cm past the mesh.
use crate::api::Asset;
use crate::geom::*;
use crate::mesh::*;

/// one collision volume, in the piece's frame
#[derive(Clone, Debug)]
pub enum Shape {
    Box { lo: V3, hi: V3 },
    /// a convex hull of these points (door arcs, angled parts)
    Hull(Vec<V3>),
}

/// a part that opens: its opening volume is generated from it
#[derive(Clone, Debug)]
pub enum Leaf {
    /// a door turning about the axis through `hinge` (unit `axis`, right-handed) from closed (0) to `max_angle` radians: wardrobe and
    /// cabinet doors about z, an oven's or a dishwasher's drop-down door about a horizontal axis
    Hinged { panel: (V3, V3), hinge: V3, axis: V3, max_angle: f64 },
    /// a drawer box sliding `travel` metres along unit `dir`
    Drawer { panel: (V3, V3), dir: V3, travel: f64 },
}

#[derive(Clone, Debug)]
pub enum Role {
    /// rigid: nothing may overlap it
    Hard,
    /// rigid, and items may stand on its top face
    Support,
    /// drapes, foliage, throws: may overlap neighbouring furniture and door clearances, not walls or walkways
    Soft,
    /// a door's swing or a drawer's pull-out (the volume's shape is the leaf's sweep)
    Opening(Leaf),
    /// rigid, sunk into what it stands on (a sink's basin under the counter's top): hard against everything but that object
    Inset,
}

#[derive(Clone, Debug)]
pub struct Volume {
    pub shape: Shape,
    pub role: Role,
}

impl Volume {
    pub fn hard(lo: V3, hi: V3) -> Volume { Volume { shape: Shape::Box { lo, hi }, role: Role::Hard } }
    pub fn support(lo: V3, hi: V3) -> Volume { Volume { shape: Shape::Box { lo, hi }, role: Role::Support } }
    pub fn soft(lo: V3, hi: V3) -> Volume { Volume { shape: Shape::Box { lo, hi }, role: Role::Soft } }
    pub fn inset(lo: V3, hi: V3) -> Volume { Volume { shape: Shape::Box { lo, hi }, role: Role::Inset } }
    pub fn hard_hull(pts: Vec<V3>) -> Volume { Volume { shape: Shape::Hull(pts), role: Role::Hard } }
    /// the opening volume of a leaf: the panel swept from closed to fully open
    pub fn opening(leaf: Leaf) -> Volume { Volume { shape: Shape::Hull(sweep(&leaf, 9)), role: Role::Opening(leaf) } }

    /// (an inset volume is rigid too, but below what the piece stands on: not in its bounds, its relation faces or its size)
    pub fn is_rigid(&self) -> bool { matches!(self.role, Role::Hard | Role::Support) }
    pub fn points(&self) -> Vec<V3> {
        match &self.shape {
            Shape::Box { lo, hi } => corners(*lo, *hi).to_vec(),
            Shape::Hull(p) => p.clone(),
        }
    }
    pub fn bounds(&self) -> (V3, V3) { bounds_of(&self.points()) }
}

/// What a piece occupies, from its parameters alone. `&mut`: the piece keeps the decisions it makes here (sizes, layout of its parts,
/// the variant it picked) so its build reuses them.
pub trait Collide {
    fn collision(&mut self) -> Vec<Volume>;
}

/// What a piece looks like: the full model, in the frame and at the size of its collision (built from the decisions its collision made).
pub trait Build {
    fn mesh(&mut self) -> Asset;
}

/// An asset that owns its state.
pub trait Piece: Collide + Build + Send + Sync {
    /// the same piece at another size ([width (y), depth (x), height (z)]); None: it has one size
    fn resize(&self, _size: [f64; 3]) -> Option<Box<dyn Piece>> { None }
    /// a counter run with its ends set (one leg of an L: [blind end, blind width, joined end, wall-cabinet reach]); None: it has no ends
    fn with_ends(&self, _ends: [f64; 4]) -> Option<Box<dyn Piece>> { None }
    /// the piece with its soft parts drawn in to its rigid ones (a bed's bedding tucked in, for a tight spot); asked again, smaller still
    /// (a compact single bed); None: it cannot
    fn tucked(&self) -> Option<Box<dyn Piece>> { None }
    /// collision written from the piece's parameters (false: the compatibility wrapper's layout-build boxes)
    fn native(&self) -> bool { true }
    /// its current size [width (y), depth (x), height (z)]: the rigid volumes' extents
    fn size(&mut self) -> [f64; 3] {
        let (lo, hi) = rigid_bounds(&self.collision());
        [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]]
    }
}

pub fn corners(lo: V3, hi: V3) -> [V3; 8] {
    std::array::from_fn(|i| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }])
}

pub fn bounds_of(pts: &[V3]) -> (V3, V3) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in pts { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    (lo, hi)
}

/// the bounds of the rigid (hard and support) volumes: what the solver's relations and broad phase use
pub fn rigid_bounds(vols: &[Volume]) -> (V3, V3) {
    bounds_of(&vols.iter().filter(|v| v.is_rigid()).flat_map(|v| v.points()).collect::<Vec<_>>())
}

/// `p` turned by `t` radians about the axis through `h` along unit `a` (Rodrigues)
pub fn rotate_about(p: V3, h: V3, a: V3, t: f64) -> V3 {
    let v = sub(p, h);
    let (s, c) = t.sin_cos();
    let k = cross(a, v);
    let d = dot(a, v);
    [0, 1, 2].map(|i| h[i] + v[i] * c + k[i] * s + a[i] * d * (1.0 - c))
}

/// the leaf's panel corners from closed to fully open (`steps` + 1 positions): their hull holds the whole sweep (a panel turning by up to
/// 180 degrees about one of its edges sweeps a convex sector; the chords between steps lie inside the arc's hull of the panel corners at
/// a bulge below 1 - cos(step / 2), under 4 mm for a 0.6 m door in 9 steps of 10 degrees -- the steps' end corners are pushed out by it)
pub fn sweep(leaf: &Leaf, steps: usize) -> Vec<V3> {
    let mut out = vec![];
    match leaf {
        Leaf::Hinged { panel, hinge, axis, max_angle } => {
            let cs = corners(panel.0, panel.1);
            let dt = max_angle / steps as f64;
            // corners pushed out radially by the chord bulge so the hull holds the arcs between steps
            let grow = 1.0 / (dt / 2.0).cos();
            for i in 0..=steps {
                let t = dt * i as f64;
                for c in &cs {
                    let p = rotate_about(*c, *hinge, *axis, t);
                    // radial push (from the axis), only for intermediate steps; the closed and open positions are exact
                    let v = sub(p, *hinge);
                    let along = dot(v, *axis);
                    let foot = [hinge[0] + axis[0] * along, hinge[1] + axis[1] * along, hinge[2] + axis[2] * along];
                    let g = if i == 0 || i == steps { 1.0 } else { grow };
                    out.push([0, 1, 2].map(|k| foot[k] + (p[k] - foot[k]) * g));
                }
            }
        }
        Leaf::Drawer { panel, dir, travel } => {
            for c in corners(panel.0, panel.1) {
                out.push(c);
                out.push([c[0] + dir[0] * travel, c[1] + dir[1] * travel, c[2] + dir[2] * travel]);
            }
        }
    }
    out
}

/// the solver's geometry for a piece's volumes: the rigid volumes' boxes merged to at most `max_rigid` (the pair whose union adds the
/// least empty space first: a shelf's boards fold into its carcass, a chair keeps seat, back and arms apart -- the solver's cost grows
/// with the proxies), each a hard proxy with its triangles (canonical tags over the rigid bounds, for the relations); support surfaces
/// as support-tagged top faces; soft and opening volumes as proxies with their role
pub fn solver_geom(vols: &[Volume], max_rigid: usize) -> ObjGeom {
    let (lo, hi) = rigid_bounds(vols);
    // rigid boxes merge; rigid hulls (a round table top) stay as they are
    let mut boxes: Vec<(V3, V3)> = vols.iter().filter(|v| v.is_rigid() && matches!(v.shape, Shape::Box { .. })).map(|v| v.bounds()).collect();
    let hulls: Vec<&Vec<V3>> = vols.iter().filter(|v| v.is_rigid()).filter_map(|v| match &v.shape { Shape::Hull(p) => Some(p), _ => None }).collect();
    let inside = |a: &(V3, V3), b: &(V3, V3)| (0..3).all(|k| a.0[k] >= b.0[k] - 1e-6 && a.1[k] <= b.1[k] + 1e-6);
    let vol = |b: &(V3, V3)| (0..3).map(|k| (b.1[k] - b.0[k]).max(0.0)).product::<f64>();
    let uni = |a: &(V3, V3), b: &(V3, V3)| -> (V3, V3) { ([0, 1, 2].map(|k| a.0[k].min(b.0[k])), [0, 1, 2].map(|k| a.1[k].max(b.1[k]))) };
    // boxes inside another go (of identical ones, the first stays)
    let dedup = |bs: &mut Vec<(V3, V3)>| {
        let k = bs.clone();
        let keep: Vec<bool> = (0..k.len()).map(|i| !(0..k.len()).any(|j| j != i && inside(&k[i], &k[j]) && (!inside(&k[j], &k[i]) || j < i))).collect();
        let mut it = keep.iter();
        bs.retain(|_| *it.next().unwrap());
    };
    dedup(&mut boxes);
    while boxes.len() + hulls.len() > max_rigid.max(1 + hulls.len()) {
        let mut best = (f64::INFINITY, 0, 1);
        for i in 0..boxes.len() { for j in i + 1..boxes.len() {
            let c = vol(&uni(&boxes[i], &boxes[j])) - vol(&boxes[i]) - vol(&boxes[j]);
            if c < best.0 { best = (c, i, j); }
        }}
        let u = uni(&boxes[best.1], &boxes[best.2]);
        boxes.remove(best.2);
        boxes[best.1] = u;
        dedup(&mut boxes);
    }
    let mut tris: Vec<Tri> = vec![];
    let mut proxies = vec![];
    let mut roles = vec![];
    for (a, b) in &boxes {
        tris.extend(box_tris(*a, *b, 0));
        proxies.push((Proxy::Box { center: [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0), half: [0, 1, 2].map(|k| ((b[k] - a[k]) / 2.0).max(1e-6)) }, u32::MAX));
        roles.push(ROLE_HARD);
    }
    for p in &hulls {
        let (a, b) = bounds_of(p);
        tris.extend(box_tris(a, b, 0));
        proxies.push((Proxy::Hull((*p).clone()), u32::MAX));
        roles.push(ROLE_HARD);
    }
    canonical_tag(&mut tris, lo, hi);
    for v in vols {
        match &v.role {
            Role::Support => {
                let (a, b) = v.bounds();
                for mut t in box_tris(a, b, 0) {
                    if cross(sub(t.v[1], t.v[0]), sub(t.v[2], t.v[0]))[2] > 1e-12 { t.tags = SUPPORT; tris.push(t); }
                }
            }
            Role::Soft | Role::Opening(_) | Role::Inset => {
                let p = match &v.shape { Shape::Box { lo: a, hi: b } => Proxy::Box { center: [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0), half: [0, 1, 2].map(|k| ((b[k] - a[k]) / 2.0).max(1e-6)) }, Shape::Hull(pts) => Proxy::Hull(pts.clone()) };
                proxies.push((p, u32::MAX));
                roles.push(match v.role { Role::Soft => ROLE_SOFT, Role::Inset => ROLE_INSET, _ => ROLE_OPEN });
            }
            Role::Hard => {}
        }
    }
    let mut g = ObjGeom::from_tris(tris, proxies);
    g.roles = roles;
    g
}

/// a volume ready for many point tests (hull face planes computed once)
pub struct Prepared {
    pub role: Role,
    shape: Prep,
}
enum Prep { Box(V3, V3), Planes(Vec<(V3, f64)>) }

impl Prepared {
    pub fn new(v: &Volume) -> Prepared {
        let shape = match &v.shape { Shape::Box { lo, hi } => Prep::Box(*lo, *hi), Shape::Hull(p) => Prep::Planes(hull_planes(p)) };
        Prepared { role: v.role.clone(), shape }
    }
    /// how far `p` lies outside (0 inside): distance to a box, the largest face-plane overshoot for a hull
    pub fn outside(&self, p: V3) -> f64 {
        match &self.shape {
            Prep::Box(lo, hi) => (0..3).map(|k| (lo[k] - p[k]).max(p[k] - hi[k]).max(0.0)).map(|d| d * d).sum::<f64>().sqrt(),
            Prep::Planes(pl) => pl.iter().map(|(n, d)| dot(*n, p) - d).fold(0.0, f64::max),
        }
    }
    /// is `p` inside (grown by `tol`)?
    pub fn contains(&self, p: V3, tol: f64) -> bool {
        match &self.shape {
            Prep::Box(lo, hi) => (0..3).all(|k| p[k] >= lo[k] - tol && p[k] <= hi[k] + tol),
            Prep::Planes(pl) => !pl.is_empty() && pl.iter().all(|(n, d)| dot(*n, p) - d <= tol),
        }
    }
}

/// the outward face planes (unit normal, offset) of the convex hull of `pts`: a point triple whose plane has every point on one side
/// is a face plane (brute force: hulls here are small, a door sweep has 80 points)
pub fn hull_planes(pts: &[V3]) -> Vec<(V3, f64)> {
    let n = pts.len();
    let mut out: Vec<(V3, f64)> = vec![];
    if n < 4 { return out }
    for i in 0..n { for j in i + 1..n { for l in j + 1..n {
        let nrm = cross(sub(pts[j], pts[i]), sub(pts[l], pts[i]));
        let len = dot(nrm, nrm).sqrt();
        if len < 1e-9 { continue }
        let mut nrm = [nrm[0] / len, nrm[1] / len, nrm[2] / len];
        let mut d = dot(nrm, pts[i]);
        let (mut pos, mut neg) = (false, false);
        for q in pts { let s = dot(nrm, *q) - d; if s > 1e-7 { pos = true } if s < -1e-7 { neg = true } if pos && neg { break } }
        if pos && neg { continue }
        if pos { nrm = [-nrm[0], -nrm[1], -nrm[2]]; d = -d; }
        if !out.iter().any(|(m, e)| dot(*m, nrm) > 1.0 - 1e-9 && (e - d).abs() < 1e-9) { out.push((nrm, d)); }
    }}}
    out
}

/// the piece registry (installed by the asset crate, which holds the classes: ig_assets::install_pieces)
pub type Registry = fn(&str, u64) -> Option<Box<dyn Piece>>;
static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
pub fn install(r: Registry) { let _ = REGISTRY.set(r); }

/// a new piece of `class` from its seed; None: no registry installed, or the class has no piece yet (the caller keeps its old path)
pub fn make_piece(class: &str, seed: u64) -> Option<Box<dyn Piece>> {
    REGISTRY.get().and_then(|r| r(class, seed))
}

/// the rules pieces decide by (set from the layout rules, GenConfig.rules: user-editable)
#[derive(Clone, Copy, Debug)]
pub struct PieceRules {
    /// how often a dining table up to 1.6 m long is round
    pub round_table_p: f64,
}
impl Default for PieceRules { fn default() -> Self { PieceRules { round_table_p: 0.5 } } }
static RULES: std::sync::RwLock<Option<PieceRules>> = std::sync::RwLock::new(None);
pub fn set_rules(r: PieceRules) { *RULES.write().unwrap() = Some(r); }
pub fn rules() -> PieceRules { RULES.read().unwrap().unwrap_or_default() }
