//! Procedural trees in Rust (outdoor stage): the stem structure of Weber & Penn, "Creation and Rendering of Realistic Trees"
//! (SIGGRAPH 1995) -- recursive stems with per-level length / curvature / down-angle / rotation / branch counts, a crown shape ratio,
//! taper and upward attraction -- and, for broadleaf crowns, space colonization (Runions, Lane & Prusinkiewicz, "Modeling Trees with a
//! Space Colonization Algorithm", 2007): branches grow toward attraction points that fill a crown envelope, radii by the pipe model.
//! Written from the papers (no code taken from Sapling / Arbaro). Leaves are alpha-masked cards, each carrying a cluster of leaves drawn
//! procedurally into one RGBA texture per species (glTF alphaMode MASK). Level of detail: `Lod::Near` full stems and cards, `Lod::Far`
//! trunk + main limbs and a few large cards.
use ig_core::mesh::{encode_png, Material, Mesh};
use ig_core::rng::Rng;
use std::sync::Arc;

type V3 = [f64; 3];
fn add(a: V3, b: V3) -> V3 { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn sub(a: V3, b: V3) -> V3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn mul(a: V3, s: f64) -> V3 { [a[0] * s, a[1] * s, a[2] * s] }
fn dot(a: V3, b: V3) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn len(a: V3) -> f64 { dot(a, a).sqrt() }
fn norm(a: V3) -> V3 { let l = len(a).max(1e-12); mul(a, 1.0 / l) }
/// rotate v about unit axis k by angle a (Rodrigues)
fn rot(v: V3, k: V3, a: f64) -> V3 {
    let (s, c) = a.sin_cos();
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
}
fn perp(d: V3) -> V3 { norm(cross(d, if d[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] })) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Species { Oak, Maple, Birch, Pine, Willow, Shrub }
pub const ALL: [Species; 6] = [Species::Oak, Species::Maple, Species::Birch, Species::Pine, Species::Willow, Species::Shrub];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod { Near, Far }

/// Weber-Penn crown shape ratio (shape 0 conical .. 7 flame; 8 = the crown envelope of a space-colonization species)
fn shape_ratio(shape: u32, r: f64) -> f64 {
    let r = r.clamp(0.0, 1.0);
    match shape {
        0 => 0.2 + 0.8 * r,
        1 => 0.2 + 0.8 * (std::f64::consts::PI * r).sin(),
        2 => 0.2 + 0.8 * (0.5 * std::f64::consts::PI * r).sin(),
        3 => 1.0,
        4 => 0.5 + 0.5 * r,
        5 => if r <= 0.7 { r / 0.7 } else { (1.0 - r) / 0.3 },
        6 => 1.0 - 0.8 * r,
        7 => if r <= 0.7 { 0.5 + 0.5 * r / 0.7 } else { 0.5 + 0.5 * (1.0 - r) / 0.3 },
        _ => 1.0,
    }
}

/// per-level stem parameters (Weber-Penn names)
#[derive(Clone, Copy)]
struct Level { length: f64, length_v: f64, curve: f64, curve_v: f64, curve_back: f64, curve_res: usize, branches: usize, down: f64, down_v: f64, rotate: f64, rotate_v: f64, taper: f64 }

struct Params {
    shape: u32, base_size: f64, scale: f64, scale_v: f64, ratio: f64, ratio_power: f64, flare: f64, attraction_up: f64,
    levels: Vec<Level>,
    /// leaf cards per terminal stem, card size (m), leaf aspect
    leaves: usize, card: f64,
    /// space-colonization crown (broadleaf): envelope radius / height fractions of the tree height; None = Weber-Penn throughout
    sc: Option<(f64, f64)>,
    bark: [f32; 3], leaf: [f32; 3], leaf_kind: LeafKind,
    /// number of trunks from the ground (shrubs)
    trunks: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum LeafKind { Broad, Lobed, Needle, Lance, Small }

fn lv(length: f64, length_v: f64, curve: f64, curve_v: f64, curve_res: usize, branches: usize, down: f64, down_v: f64, rotate: f64, rotate_v: f64) -> Level {
    Level { length, length_v, curve, curve_v, curve_back: 0.0, curve_res, branches, down, down_v, rotate, rotate_v, taper: 1.0 }
}

fn params(sp: Species) -> Params {
    use Species::*;
    match sp {
        Oak => Params { shape: 8, base_size: 0.25, scale: 13.0, scale_v: 3.0, ratio: 0.022, ratio_power: 1.3, flare: 0.45, attraction_up: 0.4,
            levels: vec![lv(0.36, 0.05, 0.0, 25.0, 6, 0, 0.0, 0.0, 0.0, 0.0)], leaves: 6, card: 0.85, sc: Some((0.62, 0.74)),
            bark: [0.16, 0.13, 0.1], leaf: [0.12, 0.22, 0.05], leaf_kind: LeafKind::Lobed, trunks: 1 },
        Maple => Params { shape: 8, base_size: 0.3, scale: 14.0, scale_v: 3.0, ratio: 0.018, ratio_power: 1.3, flare: 0.3, attraction_up: 0.6,
            levels: vec![lv(0.46, 0.05, 0.0, 15.0, 6, 0, 0.0, 0.0, 0.0, 0.0)], leaves: 6, card: 0.8, sc: Some((0.46, 0.7)),
            bark: [0.2, 0.17, 0.14], leaf: [0.13, 0.25, 0.05], leaf_kind: LeafKind::Broad, trunks: 1 },
        Birch => Params { shape: 7, base_size: 0.35, scale: 15.0, scale_v: 3.0, ratio: 0.013, ratio_power: 1.4, flare: 0.2, attraction_up: 0.3,
            levels: vec![lv(1.0, 0.0, 0.0, 10.0, 8, 0, 0.0, 0.0, 0.0, 0.0), lv(0.32, 0.06, -10.0, 30.0, 6, 42, 55.0, -15.0, 140.0, 30.0), lv(0.45, 0.1, 25.0, 40.0, 4, 18, 45.0, 15.0, 140.0, 40.0), lv(0.45, 0.1, 0.0, 30.0, 2, 6, 45.0, 10.0, 140.0, 0.0)],
            leaves: 7, card: 0.62, sc: None, bark: [0.62, 0.6, 0.56], leaf: [0.18, 0.3, 0.07], leaf_kind: LeafKind::Small, trunks: 1 },
        Pine => Params { shape: 0, base_size: 0.2, scale: 17.0, scale_v: 4.0, ratio: 0.017, ratio_power: 1.2, flare: 0.3, attraction_up: 0.1,
            levels: vec![lv(1.0, 0.0, 0.0, 6.0, 10, 0, 0.0, 0.0, 0.0, 0.0), lv(0.28, 0.05, 5.0, 15.0, 5, 70, 88.0, -25.0, 137.5, 15.0), lv(0.4, 0.08, 0.0, 20.0, 3, 14, 50.0, 10.0, 140.0, 20.0)],
            leaves: 6, card: 0.6, sc: None, bark: [0.22, 0.15, 0.1], leaf: [0.05, 0.12, 0.05], leaf_kind: LeafKind::Needle, trunks: 1 },
        Willow => Params { shape: 3, base_size: 0.3, scale: 12.0, scale_v: 2.5, ratio: 0.02, ratio_power: 1.3, flare: 0.4, attraction_up: -2.0,
            levels: vec![lv(1.0, 0.0, 0.0, 20.0, 6, 0, 0.0, 0.0, 0.0, 0.0), lv(0.4, 0.08, -20.0, 30.0, 6, 26, 50.0, 10.0, 140.0, 40.0), lv(0.9, 0.15, 30.0, 20.0, 8, 22, 30.0, 10.0, 120.0, 30.0)],
            leaves: 5, card: 0.42, sc: None, bark: [0.2, 0.17, 0.12], leaf: [0.2, 0.3, 0.1], leaf_kind: LeafKind::Lance, trunks: 1 },
        Shrub => Params { shape: 8, base_size: 0.0, scale: 1.6, scale_v: 0.5, ratio: 0.03, ratio_power: 1.3, flare: 0.0, attraction_up: 0.5,
            levels: vec![lv(0.35, 0.1, 0.0, 30.0, 3, 0, 0.0, 0.0, 0.0, 0.0)], leaves: 5, card: 0.32, sc: Some((0.6, 0.85)),
            bark: [0.18, 0.14, 0.1], leaf: [0.15, 0.27, 0.07], leaf_kind: LeafKind::Small, trunks: 4 },
    }
}

/// a stem polyline with radii; `tips`: points + directions where leaf cards go
struct Stem { pts: Vec<V3>, rad: Vec<f64> }

pub struct Tree {
    pub bark: Mesh,
    pub leaves: Mesh,
    /// one crown centre per leaf face (Scene::add_bent: leaf-card normals bent away from it)
    pub leaf_centres: Vec<V3>,
    pub height: f64,
}

struct Builder<'a> {
    p: &'a Params,
    r: Rng,
    stems: Vec<Stem>,
    cards: Vec<(V3, V3)>,
    lod: Lod,
    /// crowding (Palubicki et al. 2009, Pirk et al. 2012: crowns share the space between neighbours): the tree's own crown radius
    /// and its neighbours' (trunk position, crown radius), local frame; space past the power-diagram boundary belongs to a neighbour
    own_r: f64,
    nb: Vec<([f64; 2], f64)>,
}

impl<'a> Builder<'a> {
    /// is the point in this tree's share of the space (its Laguerre / power cell among the neighbours' crowns)?
    fn mine(&self, p: V3) -> bool {
        let own = p[0] * p[0] + p[1] * p[1] - self.own_r * self.own_r;
        self.nb.iter().all(|(c, r)| own <= (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) - r * r)
    }

    /// Weber-Penn stem at `level`, from `start` along `dir`, length `l`, base radius `rad0`; recurses into children
    fn stem(&mut self, level: usize, start: V3, dir: V3, l: f64, rad0: f64, tree_h: f64) {
        let lp = self.p.levels[level];
        // (the trunk gets 3x the segments: the root flare needs rings near the ground)
        let res = lp.curve_res.max(1) * if level == 0 { 3 } else { 1 };
        let seg = l / res as f64;
        let mut d = norm(dir);
        let bend_axis = perp(d);
        let mut pts = vec![start];
        let mut rad = vec![rad0 * if level == 0 { 1.0 + self.p.flare * 0.99 } else { 1.0 }];
        let mut p = start;
        for i in 0..res {
            let t = (i + 1) as f64 / res as f64;
            // curvature: Curve split over the segments (CurveBack bends the second half the other way), CurveV random per segment
            let c = if lp.curve_back != 0.0 && i >= res / 2 { lp.curve_back } else { lp.curve };
            let ang = (c / res as f64 + self.r.uniform(-1.0, 1.0) * lp.curve_v / res as f64).to_radians();
            d = norm(rot(d, bend_axis, ang));
            // attraction up (levels >= 1): segments bend toward (or, negative, away from -- weeping) the vertical
            if level >= 1 && self.p.attraction_up != 0.0 {
                let decl = d[2].clamp(-1.0, 1.0).acos();
                let k = self.p.attraction_up * decl * (1.0 - d[2].abs()).max(0.05) / res as f64 * 0.6;
                let axis = cross(d, [0.0, 0.0, 1.0]);
                if len(axis) > 1e-6 { d = norm(rot(d, norm(axis), k)); }
            }
            p = add(p, mul(d, seg));
            pts.push(p);
            // Weber-Penn root flare: Flare * (100^y - 1) / 100, y = 1 - 8t (a curved flare, not a cone)
            let flare = if level == 0 { let y = (1.0 - 8.0 * t).max(0.0); self.p.flare * (100f64.powf(y) - 1.0) / 100.0 } else { 0.0 };
            // (a space-colonization trunk ends at the crown base, where its limbs take over: it keeps ~60 % of its radius there)
            let taper = if level == 0 && self.p.sc.is_some() { 0.4 } else { lp.taper * 0.95 };
            rad.push((rad0 * (1.0 - taper * t)).max(0.002) * (1.0 + flare));
        }
        let stem_pts = pts.clone();
        self.stems.push(Stem { pts, rad: rad.clone() });
        // children (Weber-Penn levels), or the space-colonization crown off the trunk
        if level == 0 {
            if let Some((rr, hh)) = self.p.sc { self.sc_crown(&stem_pts, &rad, tree_h, rr, hh); return }
        }
        let terminal = level + 1 >= self.p.levels.len() || (self.lod == Lod::Far && level >= 1);
        if terminal {
            // leaf cards along the outer part of a terminal stem
            let nl = if self.lod == Lod::Far { 1 } else { self.p.leaves * 2 };
            for k in 0..nl {
                let t = 0.35 + 0.65 * (k as f64 + self.r.f64()) / nl as f64;
                let pos = point_at(&stem_pts, t);
                self.cards.push((pos, d));
            }
            return;
        }
        let cl = self.p.levels[level + 1];
        let n = if level == 0 { cl.branches } else { (cl.branches as f64 * (0.2 + 0.8 * (l / tree_h).min(1.0) * 2.0)).round() as usize };
        let base = if level == 0 { self.p.base_size } else { 0.0 };
        let mut rot_acc = self.r.uniform(0.0, 360.0);
        for i in 0..n {
            let off = base + (1.0 - base) * (i as f64 + self.r.uniform(0.0, 0.8)) / n as f64;
            let at = point_at(&stem_pts, off);
            let pdir = dir_at(&stem_pts, off);
            let lmax = cl.length + self.r.uniform(-1.0, 1.0) * cl.length_v;
            let cl_len = if level == 0 {
                l * lmax * shape_ratio(self.p.shape, (1.0 - off) / (1.0 - base).max(1e-3))
            } else { lmax * (l - 0.6 * off * l) };
            if cl_len < 0.05 { continue }
            rot_acc += cl.rotate + self.r.uniform(-1.0, 1.0) * cl.rotate_v;
            let down = if cl.down_v < 0.0 { cl.down + cl.down_v * (1.0 - 2.0 * shape_ratio(0, (l - off * l) / l.max(1e-6))) } else { cl.down + self.r.uniform(-1.0, 1.0) * cl.down_v };
            let side = perp(pdir);
            let mut cd = rot(pdir, side, down.to_radians());
            cd = rot(cd, pdir, rot_acc.to_radians());
            // (a crowded first-order branch stops at the boundary with its neighbour's crown)
            let mut cl_len = cl_len;
            if level == 0 && !self.nb.is_empty() {
                let mut f = 1.0;
                while f > 0.3 && !self.mine(add(at, mul(cd, cl_len * f))) { f -= 0.1; }
                cl_len *= f;
            }
            let prad = rad_at(&rad, off);
            let crad = (prad * (cl_len / l).powf(self.p.ratio_power)).min(prad * 0.9);
            self.stem(level + 1, at, cd, cl_len, crad, tree_h);
        }
    }

    /// space colonization (Runions 2007) from the trunk's upper part: attraction points fill an ellipsoid crown with a few random
    /// lobes; nodes grow by step D toward the mean direction of the points within the influence radius, points within the kill
    /// distance are removed; radii by the pipe model (r^2.5 summed toward the trunk)
    fn sc_crown(&mut self, trunk: &[V3], trunk_rad: &[f64], h: f64, rr: f64, hh: f64) {
        let base = trunk[0];
        let (cr, ch) = (h * rr, h * hh);
        let cz = base[2] + h - ch * 0.5;
        let n_attr = if self.lod == Lod::Far { 250 } else if h < 3.0 { 350 } else { 2600 };
        // envelope: a smaller ellipsoid core plus 5-8 lobes that bulge past it (smooth domes read as topiary)
        let nlobe = self.r.randint(9, 14) as usize;
        let lobes: Vec<(V3, f64)> = (0..nlobe).map(|_| {
            let a = self.r.uniform(0.0, std::f64::consts::TAU);
            let rr_ = cr * self.r.uniform(0.35, 0.95);
            ([a.cos() * rr_, a.sin() * rr_, self.r.uniform(-0.35, 0.45) * ch], cr * self.r.uniform(0.28, 0.45))
        }).collect();
        let mut attr: Vec<V3> = vec![];
        let mut guard = 0;
        while attr.len() < n_attr && guard < n_attr * 200 {
            guard += 1;
            let q = [self.r.uniform(-1.3, 1.3), self.r.uniform(-1.3, 1.3), self.r.uniform(-1.3, 1.3)];
            let pnt = [base[0] + q[0] * cr, base[1] + q[1] * cr, cz + q[2] * ch * 0.5];
            let in_core = dot(q, q) < 0.36 && q[2] > -0.6;
            let in_lobe = lobes.iter().any(|(c, lr)| len(sub(pnt, [base[0] + c[0], base[1] + c[1], cz + c[2]])) < *lr);
            if (in_core || in_lobe) && self.mine(pnt) { attr.push(pnt); }
        }
        let step = h * 0.035;
        let (di, dk) = (step * 9.0, step * 1.6);
        // nodes: (pos, parent); seed nodes along the trunk's upper 45 %
        let mut nodes: Vec<(V3, Option<usize>)> = vec![];
        // (branches start from the top few trunk nodes: the trunk forks into a few main limbs, as in photos of oaks and maples;
        // seeding the upper 60 % stacked parallel horizontal limbs up a pole)
        let k0 = trunk.len().saturating_sub(3);
        for (i, &tp) in trunk.iter().enumerate() { nodes.push((tp, if i == 0 { None } else { Some(i - 1) })); let _ = k0; }
        let first_free = 0;
        for _iter in 0..90 {
            if attr.is_empty() { break }
            let mut pull: std::collections::HashMap<usize, (V3, usize)> = Default::default();
            for a in &attr {
                let mut best = (f64::MAX, usize::MAX);
                for (ni, (np, _)) in nodes.iter().enumerate().skip(first_free) {
                    if ni < k0 { continue }
                    let dd = len(sub(*a, *np));
                    if dd < best.0 { best = (dd, ni); }
                }
                if best.0 < di {
                    let e = pull.entry(best.1).or_insert(([0.0; 3], 0));
                    e.0 = add(e.0, norm(sub(*a, nodes[best.1].0))); e.1 += 1;
                }
            }
            if pull.is_empty() { break }
            let mut keys: Vec<usize> = pull.keys().copied().collect(); keys.sort();
            for ni in keys {
                let (dsum, _) = pull[&ni];
                let jitter = [self.r.uniform(-0.15, 0.15), self.r.uniform(-0.15, 0.15), self.r.uniform(-0.05, 0.2)];
                // (upward tropism, Runions et al. 2007: limbs rise toward the light instead of leaving the trunk flat)
                let d = norm(add(add(norm(dsum), jitter), [0.0, 0.0, 0.3 * self.p.attraction_up.max(0.0) + 0.15]));
                let np = add(nodes[ni].0, mul(d, step));
                nodes.push((np, Some(ni)));
            }
            attr.retain(|a| !nodes.iter().skip(k0).any(|(np, _)| len(sub(*a, *np)) < dk));
        }
        // pipe model radii: leaves r0, parent r = (sum child r^2.5)^(1/2.5)
        let nn = nodes.len();
        let mut children: Vec<Vec<usize>> = vec![vec![]; nn];
        for (i, (_, pa)) in nodes.iter().enumerate() { if let Some(p) = pa { children[*p].push(i); } }
        let r0 = 0.003 * (h / 10.0).max(0.4);
        let mut rad = vec![0.0f64; nn];
        for i in (0..nn).rev() {
            if children[i].is_empty() { rad[i] = r0 } else { rad[i] = children[i].iter().map(|&c| rad[c].powf(2.1)).sum::<f64>().powf(1.0 / 2.1); }
        }
        // limbs carry the trunk: the pipe sum at the crown's lowest seed node is scaled up to the trunk's radius there (~600 tips at
        // r0 gave a 6 cm limb system on a 30 cm trunk -- real trees have far more twigs); tips stay at r0, thick parts scale
        if k0 < nn && rad[k0] > r0 {
            let target = trunk_rad[k0.min(trunk_rad.len() - 1)] * 0.8;
            let s = (target / rad[k0]).clamp(1.0, 6.0);
            for i in trunk.len()..nn { rad[i] = r0 + (rad[i] - r0) * s; }
            rad[k0] *= s;
        }
        // the trunk keeps its Weber-Penn radius below the crown
        for i in 0..trunk.len().min(nn) { rad[i] = rad[i].max(trunk_rad[i.min(trunk_rad.len() - 1)] * 0.5).min(trunk_rad[0] * 1.5); }
        // emit chains (each branch from a fork to a tip) as stems, skipping the trunk itself
        for i in trunk.len()..nn {
            let Some(pa) = nodes[i].1 else { continue };
            // start a chain where the parent is a fork or the trunk
            if children[pa].len() == 1 && pa >= trunk.len() { continue }
            let mut pts = vec![nodes[pa].0];
            let mut rs = vec![rad[pa].min(rad[i] * 1.6).min(rad[pa] * 0.7)];
            let mut cur = i;
            loop {
                pts.push(nodes[cur].0); rs.push(rad[cur].min(rs[0]));
                if children[cur].len() != 1 { break }
                cur = children[cur][0];
            }
            if self.lod == Lod::Far && rs[0] < r0 * 3.0 { continue }
            self.stems.push(Stem { pts, rad: rs });
        }
        // leaf cards at tips and on thin twigs
        for i in 0..nn {
            if children[i].len() > 1 || i < trunk.len() { continue }
            let thin = rad[i] < r0 * 3.5;
            if !thin { continue }
            let d = match nodes[i].1 { Some(p) => norm(sub(nodes[i].0, nodes[p].0)), None => [0.0, 0.0, 1.0] };
            let n = if self.lod == Lod::Far { if self.r.f64() < 0.3 { 1 } else { 0 } } else if children[i].is_empty() { 6 } else { 4 };
            for _ in 0..n { self.cards.push((nodes[i].0, d)); }
        }
    }
}

fn point_at(pts: &[V3], t: f64) -> V3 {
    let x = t.clamp(0.0, 1.0) * (pts.len() - 1) as f64;
    let i = (x.floor() as usize).min(pts.len() - 2);
    let u = x - i as f64;
    add(mul(pts[i], 1.0 - u), mul(pts[i + 1], u))
}
fn dir_at(pts: &[V3], t: f64) -> V3 {
    let x = t.clamp(0.0, 1.0) * (pts.len() - 1) as f64;
    let i = (x.floor() as usize).min(pts.len() - 2);
    norm(sub(pts[i + 1], pts[i]))
}
fn rad_at(r: &[f64], t: f64) -> f64 {
    let x = t.clamp(0.0, 1.0) * (r.len() - 1) as f64;
    let i = (x.floor() as usize).min(r.len() - 2);
    let u = x - i as f64;
    r[i] * (1.0 - u) + r[i + 1] * u
}

/// tapered tube along a polyline (parallel-transported rings); `sides` by the base radius
fn tube(m: &mut Mesh, s: &Stem, lod: Lod) {
    if s.pts.len() < 2 { return }
    let r0 = s.rad[0];
    let sides = match lod { Lod::Near => if r0 > 0.15 { 12 } else if r0 > 0.05 { 8 } else if r0 > 0.015 { 5 } else { 3 }, Lod::Far => if r0 > 0.1 { 6 } else { 3 } };
    let mut frame = perp(norm(sub(s.pts[1], s.pts[0])));
    let mut rings: Vec<Vec<V3>> = vec![];
    for i in 0..s.pts.len() {
        let d = if i + 1 < s.pts.len() { norm(sub(s.pts[i + 1], s.pts[i])) } else { norm(sub(s.pts[i], s.pts[i - 1])) };
        frame = norm(sub(frame, mul(d, dot(frame, d))));
        if len(frame) < 1e-6 { frame = perp(d); }
        let b = cross(d, frame);
        rings.push((0..sides).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / sides as f64;
            add(s.pts[i], add(mul(frame, a.cos() * s.rad[i]), mul(b, a.sin() * s.rad[i])))
        }).collect());
    }
    for i in 0..rings.len() - 1 {
        for k in 0..sides {
            let k2 = (k + 1) % sides;
            m.add_quad(rings[i][k], rings[i][k2], rings[i + 1][k2], rings[i + 1][k]);
        }
    }
}

/// a leaf-cluster card: a quad centred near `p`, roughly facing outward/up, with uv into one of 4 atlas cells
fn card(m: &mut Mesh, r: &mut Rng, p: V3, d: V3, size: f64, centre: V3) {
    let out = norm(add(norm(sub(p, centre)), [0.0, 0.0, 0.4]));
    let mut n = norm(add(mul(out, 0.7), [r.uniform(-0.5, 0.5), r.uniform(-0.5, 0.5), r.uniform(-0.2, 0.6)]));
    if len(n) < 1e-6 { n = [0.0, 0.0, 1.0]; }
    let u = norm(cross(n, if n[2].abs() < 0.95 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] }));
    let v = cross(n, u);
    let a = r.uniform(0.0, std::f64::consts::TAU);
    let (u, v) = (add(mul(u, a.cos()), mul(v, a.sin())), add(mul(v, a.cos()), mul(u, -a.sin())));
    let s = size * r.uniform(0.75, 1.25) * 0.5;
    let c = add(p, mul(d, size * 0.15));
    let q = [add(c, add(mul(u, -s), mul(v, -s))), add(c, add(mul(u, s), mul(v, -s))), add(c, add(mul(u, s), mul(v, s))), add(c, add(mul(u, -s), mul(v, s)))];
    let cell = r.randint(0, 4) as usize;
    let (cu, cv) = ((cell % 2) as f64 * 0.5, (cell / 2) as f64 * 0.5);
    let base = m.pos.len() as u32;
    let uvs = [[cu, cv + 0.5], [cu + 0.5, cv + 0.5], [cu + 0.5, cv], [cu, cv]];
    // (the mesh keeps one uv per vertex: give the card its own four vertices)
    if m.uv.len() != m.pos.len() { m.uv.resize(m.pos.len(), [0.0, 0.0]); }
    for k in 0..4 { m.pos.push(q[k]); m.uv.push(uvs[k]); }
    m.faces.push(vec![base, base + 1, base + 2, base + 3]);
}

/// Build one tree (local frame: trunk base at the origin, z up).
pub fn build(sp: Species, seed: u64, lod: Lod) -> Tree { build_crowded(sp, seed, lod, 0.0, &[]) }

/// The nominal crown radius of a species at scale 1 (m): what the outdoor stage's plant simulation sizes trees against.
pub fn crown_radius(sp: Species) -> f64 {
    let p = params(sp);
    match p.sc { Some((rr, _)) => p.scale * rr * 0.85, None => p.scale * match sp { Species::Willow => 0.42, Species::Birch => 0.22, _ => 0.24 } }
}

/// Build one tree among neighbours: `own_r` its crown radius, `nb` the neighbours' (trunk xy, crown radius), all in the tree's local
/// frame; branches and attraction points stay on this tree's side of the power-diagram boundaries, so crowns grow away from each
/// other and lean into gaps. (own_r 0 / no neighbours = a free-standing tree.)
pub fn build_crowded(sp: Species, seed: u64, lod: Lod, own_r: f64, nb: &[([f64; 2], f64)]) -> Tree {
    let p = params(sp);
    let mut b = Builder { p: &p, r: Rng::seeded(seed), stems: vec![], cards: vec![], lod, own_r, nb: nb.to_vec() };
    let h = (p.scale + b.r.uniform(-1.0, 1.0) * p.scale_v).max(0.5);
    for t in 0..p.trunks.max(1) {
        let (dir, start) = if p.trunks > 1 {
            let a = std::f64::consts::TAU * t as f64 / p.trunks as f64 + b.r.uniform(-0.3, 0.3);
            (norm([a.cos() * 0.35, a.sin() * 0.35, 1.0]), [a.cos() * 0.05, a.sin() * 0.05, 0.0])
        } else { ([b.r.uniform(-0.03, 0.03), b.r.uniform(-0.03, 0.03), 1.0], [0.0, 0.0, 0.0]) };
        let l0 = h * (p.levels[0].length + b.r.uniform(-1.0, 1.0) * p.levels[0].length_v);
        // (from the tree height, not the trunk length: a space-colonization trunk ends mid-crown and came out a thin stick)
        let rad0 = (h * p.ratio * if p.sc.is_some() { 1.25 } else { 1.0 } / (p.trunks as f64).sqrt()).max(0.01);
        b.stem(0, start, dir, l0, rad0, h);
    }
    let mut bark = Mesh::new();
    for s in &b.stems { tube(&mut bark, s, lod); }
    // crown centre: the mean of the card positions (the volume the cards shade as)
    let centre = if b.cards.is_empty() { [0.0, 0.0, h * 0.65] } else {
        let n = b.cards.len() as f64;
        let c = b.cards.iter().fold([0.0; 3], |a, (p, _)| add(a, *p));
        [c[0] / n, c[1] / n, c[2] / n]
    };
    let mut leaves = Mesh::new();
    let size = p.card * if lod == Lod::Far { 2.6 } else { 1.0 } * (h / p.scale).sqrt();
    let cards = std::mem::take(&mut b.cards);
    let n_cards = cards.len();
    for (pos, d) in cards { card(&mut leaves, &mut b.r, pos, d, size, centre); }
    Tree { bark, leaves, leaf_centres: vec![centre; n_cards], height: h }
}

/// The species' materials: bark (plain) and the leaf-cluster card material (RGBA texture, alpha mask). `tint` shifts the leaf colour
/// (seasons: 0 summer .. 1 autumn).
pub fn materials(sp: Species, tint: f64) -> (Material, Material) {
    let p = params(sp);
    let bark = Material::diffuse(&format!("rtree_bark:{sp:?}"), p.bark, 0.9);
    let mut leaf = Material::diffuse(&format!("rtree_leaf:{sp:?}:{:.1}", tint), [1.0, 1.0, 1.0], 0.55);
    let c = p.leaf;
    let autumn = [0.45f32, 0.25, 0.05];
    let t = tint as f32;
    let col = [c[0] * (1.0 - t) + autumn[0] * t, c[1] * (1.0 - t) + autumn[1] * t, c[2] * (1.0 - t) + autumn[2] * t];
    leaf.texture = Some(leaf_texture(p.leaf_kind, col, (tint * 1000.0) as u64 + sp as u64));
    leaf.alpha_cutoff = 0.5;
    (bark, leaf)
}

/// 512x512 atlas of 4 leaf-cluster cells (2x2): each cell holds a twiglet with 5-14 leaves of the species' kind, colour-varied
fn leaf_texture(kind: LeafKind, col: [f32; 3], seed: u64) -> Arc<Vec<u8>> {
    let n = 512usize;
    let mut img = vec![0u8; n * n * 4];
    let mut r = Rng::seeded(seed ^ 0x51ea_f00d);
    let lin2srgb = |x: f32| -> u8 { (if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }.clamp(0.0, 1.0) * 255.0) as u8 };
    let mut put = |x: i64, y: i64, c: [f32; 3]| {
        if x < 0 || y < 0 || x >= n as i64 || y >= n as i64 { return }
        let i = (y as usize * n + x as usize) * 4;
        img[i] = lin2srgb(c[0]); img[i + 1] = lin2srgb(c[1]); img[i + 2] = lin2srgb(c[2]); img[i + 3] = 255;
    };
    for cell in 0..4 {
        let (ox, oy) = ((cell % 2) as f64 * 256.0, (cell / 2) as f64 * 256.0);
        let cx = ox + 128.0; let cy = oy + 128.0;
        // twiglet stalk
        let stalk_a = r.uniform(0.0, std::f64::consts::TAU);
        let (sdx, sdy) = (stalk_a.cos(), stalk_a.sin());
        for k in 0..110 { let t = k as f64 - 55.0; for w in -1i64..=1 { put((cx + sdx * t) as i64 + w, (cy + sdy * t) as i64, [0.08, 0.06, 0.04]); } }
        let nleaf = match kind { LeafKind::Needle => 160, LeafKind::Small => 55, LeafKind::Lance => 30, _ => 26 };
        for _ in 0..nleaf {
            let t = r.uniform(-60.0, 60.0);
            let sp = r.uniform(-35.0, 35.0);
            let (bx, by) = (cx + sdx * t - sdy * sp * 0.4, cy + sdy * t + sdx * sp * 0.4);
            let a = stalk_a + r.uniform(-1.6, 1.6) + if r.f64() < 0.5 { std::f64::consts::PI } else { 0.0 } * 0.0;
            let (lx, ly) = (a.cos(), a.sin());
            let (l, w) = match kind { LeafKind::Broad => (r.uniform(50.0, 75.0), 0.55), LeafKind::Lobed => (r.uniform(45.0, 65.0), 0.6), LeafKind::Needle => (r.uniform(35.0, 60.0), 0.07), LeafKind::Lance => (r.uniform(60.0, 95.0), 0.16), LeafKind::Small => (r.uniform(26.0, 40.0), 0.6) };
            let v = r.uniform(0.75, 1.25) as f32;
            let lc = [col[0] * v * r.uniform(0.9, 1.1) as f32, col[1] * v, col[2] * v * r.uniform(0.85, 1.15) as f32];
            let steps = (l as usize).max(4);
            for si in 0..steps {
                let u = si as f64 / steps as f64;
                // width profile: pointed ellipse; lobed leaves wave along the edge
                let mut half = (u * std::f64::consts::PI).sin().powf(0.8) * l * w * 0.5;
                if kind == LeafKind::Lobed { half *= 0.75 + 0.35 * (u * 18.0).sin().abs(); }
                let px = bx + lx * u * l; let py = by + ly * u * l;
                let hw = half.ceil() as i64;
                for o in -hw..=hw {
                    let shade = 1.0 - 0.18 * (o as f32 / (hw.max(1) as f32)).abs();
                    let vein = if o == 0 { 1.15 } else { 1.0 };
                    put((px - ly * o as f64) as i64, (py + lx * o as f64) as i64, [lc[0] * shade * vein, lc[1] * shade * vein, lc[2] * shade]);
                }
            }
        }
    }
    Arc::new(encode_png(n as u32, n as u32, &img))
}
