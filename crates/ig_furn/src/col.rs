//! Collision / distance queries over convex proxies (replaces FCL meshes).
use super::geom::*;
use super::state::*;
use super::tags::TagSet;
use crate::mesh::V3;
use nalgebra as na;
use parry3d_f64::bounding_volume::{Aabb, BoundingVolume};
use parry3d_f64::query;
use parry3d_f64::shape::SharedShape;
use crate::dhash::HashMap;
use std::sync::Arc;

pub struct ProxyShape {
    pub shape: SharedShape,
    pub iso: na::Isometry3<f64>,
    pub aabb: Aabb,
    pub class: u32,
    pub corners: Vec<V3>, // world-space points (for containment tests)
    /// ROLE_*
    pub role: u8,
}

/// do proxy `ra` of object `a` and proxy `rb` of object `b` collide by their roles? Hard against hard always; a soft volume (drape,
/// foliage) only against the room's structure (walls, doors: objects without a generator), not door clearances; openings (door swings,
/// drawer pull-outs, door clearances) never against each other, against hard parts unless one object stands on or in the other
pub fn roles_collide(state: &State, a: &str, ra: u8, b: &str, rb: u8) -> bool {
    // (a door clearance is an opening: a cabinet's door may swing over it)
    let ra = if a.starts_with("doorclear/") { ROLE_OPEN } else { ra };
    let rb = if b.starts_with("doorclear/") { ROLE_OPEN } else { rb };
    match (ra, rb) {
        (ROLE_HARD, ROLE_HARD) => true,
        (ROLE_SOFT, _) => state.objs[b].generator.is_none() && !b.starts_with("doorclear/"),
        (_, ROLE_SOFT) => state.objs[a].generator.is_none() && !a.starts_with("doorclear/"),
        (ROLE_OPEN, ROLE_OPEN) => false,
        _ => !(state.objs[a].relations.iter().any(|r| r.target == b) || state.objs[b].relations.iter().any(|r| r.target == a)),
    }
}

/// the role of an object's proxy `i`
pub fn proxy_role(_name: &str, o: &ObjState, i: usize) -> u8 { o.geom.role(i) }

thread_local! {
    static CACHE: std::cell::RefCell<HashMap<String, (u64, Arc<Vec<ProxyShape>>)>> = std::cell::RefCell::new(HashMap::default());
    static SHAPES: std::cell::RefCell<HashMap<usize, SharedShape>> = std::cell::RefCell::new(HashMap::default());
}

pub fn proxies(state: &State, name: &str) -> Arc<Vec<ProxyShape>> {
    let o = &state.objs[name];
    let hit = CACHE.with(|c| c.borrow().get(name).filter(|(v, _)| *v == o.version).map(|(_, p)| p.clone()));
    if let Some(p) = hit {
        return p;
    }
    let mut out = vec![];
    for (pi, (p, class)) in o.geom.proxies.iter().enumerate() {
        let role = proxy_role(name, o, pi);
        let shape = parry_shape(p);
        let iso = proxy_pose(p, &o.pose);
        let aabb = shape.compute_aabb(&iso);
        let corners: Vec<V3> = match p {
            Proxy::Box { center, half } => {
                let mut v = vec![];
                for sx in [-1.0, 1.0] {
                    for sy in [-1.0, 1.0] {
                        for sz in [-1.0, 1.0] {
                            v.push(o.pose.apply([center[0] + sx * half[0], center[1] + sy * half[1], center[2] + sz * half[2]]));
                        }
                    }
                }
                v
            }
            Proxy::Hull(pts) => pts.iter().map(|p| o.pose.apply(*p)).collect(),
        };
        out.push(ProxyShape { shape, iso, aabb, class: *class, corners, role });
    }
    let arc = Arc::new(out);
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), (o.version, arc.clone())));
    arc
}

pub fn clear_cache() {
    CACHE.with(|c| c.borrow_mut().clear());
}

fn inside(a: &ProxyShape, b: &ProxyShape) -> bool {
    // all corners of a strictly inside b (surface-based collision ignores full containment)
    use parry3d_f64::query::PointQuery;
    let pb = b.shape.as_ref();
    a.corners.iter().all(|c| {
        let p = na::Point3::new(c[0], c[1], c[2]);
        pb.contains_point(&b.iso, &p) && (pb.project_point(&b.iso, &p, false).point - p).norm() > 1e-3
    })
}

/// max penetration depth between objects a and b (0 if none / merely touching)
pub fn depth(state: &State, a: &str, b: &str) -> f64 {
    let pa = proxies(state, a);
    let pb = proxies(state, b);
    let mut best = 0.0f64;
    for x in pa.iter() {
        for y in pb.iter() {
            if !x.aabb.intersects(&y.aabb) || ((x.role | y.role) != ROLE_HARD && !roles_collide(state, a, x.role, b, y.role)) {
                continue;
            }
            if let Ok(Some(c)) = query::contact(&x.iso, x.shape.as_ref(), &y.iso, y.shape.as_ref(), 0.0) {
                if c.dist < 0.0 {
                    let d = -c.dist;
                    if d > 1e-9 && !(inside(x, y) || inside(y, x)) {
                        if ig_core::env_on!("DEPTHDBG") { eprintln!("DEPTH {a} vs {b}: {d:.7} xaabb {:?} yaabb {:?} class {} {}", x.aabb, y.aabb, x.class, y.class); }
                        best = best.max(d);
                    }
                }
            }
        }
    }
    best
}

/// any_touching(a vs set) with constrain_contact(max_depth): returns max depth over `others`
pub fn max_depth_against(state: &State, a: &str, others: &[&String]) -> f64 {
    let mut best = 0.0f64;
    for b in others {
        best = best.max(depth(state, a, b));
    }
    best
}

/// python's trimesh/FCL signed distance between a and (the tagged faces of) b, clamped at 0 by the caller (None: not representable here)
fn fcl_dist_objs(state: &State, a: &str, b: &str, b_tags: &TagSet) -> Option<f64> {
    use super::mpr::*;
    let (pos, neg) = tag_masks(b_tags);
    let restrict = pos != 0 || neg != 0;
    let (oa, ob) = (&state.objs[a], &state.objs[b]);
    let mean = |v: &[V3]| -> V3 {
        let n = v.len() as f64;
        let mut s = [0.0; 3];
        for p in v {
            for k in 0..3 {
                s[k] += p[k];
            }
        }
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let tri_gap = |lo: &[f64; 3], hi: &[f64; 3], wt: &[V3; 3]| -> f64 {
        let mut g2 = 0.0;
        for k in 0..3 {
            let tl = wt[0][k].min(wt[1][k]).min(wt[2][k]);
            let th = wt[0][k].max(wt[1][k]).max(wt[2][k]);
            let gap = (tl - hi[k]).max(lo[k] - th).max(0.0);
            g2 += gap * gap;
        }
        g2.sqrt()
    };
    let conv_vs_tris = |vc: &[V3], oc: &ObjState, om: &ObjState, mask: Option<(u32, u32)>| -> f64 {
        let shape = ConvexShape::new(vc, mean(vc), oc.pose.r, oc.pose.t);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in vc {
            let w = oc.pose.apply(*p);
            for k in 0..3 {
                lo[k] = lo[k].min(w[k]);
                hi[k] = hi[k].max(w[k]);
            }
        }
        let mut best = f64::INFINITY;
        for t in &om.geom.tris {
            if let Some((p, n)) = mask {
                if t.tags & p != p || t.tags & n != 0 {
                    continue;
                }
            }
            let wt = [om.pose.apply(t.v[0]), om.pose.apply(t.v[1]), om.pose.apply(t.v[2])];
            if tri_gap(&lo, &hi, &wt) >= best {
                continue;
            }
            let tri = TriShape::new(t.v, om.pose.r, om.pose.t);
            let d = distance(&shape, &tri);
            if d < 0.0 {
                return 0.0;
            }
            best = best.min(d);
        }
        best
    };
    match (&oa.geom.convex, &ob.geom.convex, restrict) {
        (Some(va), Some(vb), false) => {
            let sa = ConvexShape::new(va, mean(va), oa.pose.r, oa.pose.t);
            let sb = ConvexShape::new(vb, mean(vb), ob.pose.r, ob.pose.t);
            let d = distance(&sa, &sb);
            Some(if d < 0.0 { 0.0 } else { d })
        }
        (Some(va), _, _) => Some(conv_vs_tris(va, oa, ob, if restrict { Some((pos, neg)) } else { None })),
        (None, Some(vb), false) => Some(conv_vs_tris(vb, ob, oa, None)),
        _ => None,
    }
}

pub fn dist_objs(state: &State, a: &str, b: &str, b_tags: &TagSet) -> f64 {
    let (pos, neg) = tag_masks(b_tags);
    if pos == 0 && neg == 0 && !super::layout2d::is_room(&state.objs[a]) && !super::layout2d::is_room(&state.objs[b]) {
        return super::layout2d::dist(state, a, b);
    }
    if let Some(d) = fcl_dist_objs(state, a, b, b_tags) {
        return d;
    }
    let pa = proxies(state, a);
    let pb = proxies(state, b);
    let (pos, neg) = tag_masks(b_tags);
    let restrict = pos != 0 || neg != 0;
    let mut best = f64::INFINITY;
    for x in pa.iter() {
        for y in pb.iter() {
            if restrict && !(y.class & pos == pos && y.class & neg == 0) {
                continue;
            }
            // parry's `distance` is not exact for some cuboid pairs; closest points are
            let d = match query::closest_points(&x.iso, x.shape.as_ref(), &y.iso, y.shape.as_ref(), f64::MAX) {
                Ok(query::ClosestPoints::WithinMargin(p, q)) => (p - q).norm(),
                Ok(query::ClosestPoints::Intersecting) => 0.0,
                _ => f64::INFINITY,
            };
            best = best.min(d);
        }
    }
    best
}

/// min_dist(objs, others) with the reference's argument preprocessing (self-pairs excluded when either side is a single).
/// `b_tags` restricts the faces of `others` that count.
pub fn min_dist(state: &State, a: &[String], b: Option<&[String]>, b_tags: &TagSet) -> f64 {
    match b {
        None => {
            let mut best = f64::INFINITY;
            for i in 0..a.len() {
                for j in i + 1..a.len() {
                    best = best.min(dist_objs(state, &a[i], &a[j], &TagSet::new()));
                }
            }
            best
        }
        Some(b) => {
            if a.len() == 1 && b.len() == 1 && a[0] == b[0] {
                return 0.0;
            }
            let mut best = f64::INFINITY;
            for x in a {
                if b.len() == 1 && a.len() > 1 && *x == b[0] {
                    continue;
                }
                for y in b {
                    if a.len() == 1 && b.len() > 1 && x == y {
                        continue;
                    }
                    let dd = dist_objs(state, x, y, b_tags);
                    if ig_core::env_on!("DISTDBG") && crate::rng::TRACE_ON.with(|t| t.get()) { { let (px, py) = (proxies(state, x), proxies(state, y)); eprintln!("RT   PAIR {x} {y} {dd:.5} shapes {:?} {:?} iso {:?} {:?}", px[0].shape.as_cuboid().map(|c| c.half_extents), py[0].shape.as_cuboid().map(|c| c.half_extents), px[0].iso, py[0].iso); } }
                    best = best.min(dd);
                }
            }
            best
        }
    }
}

/// depth between a free-standing proxy and all proxies of object `b`
pub fn depth_shape(state: &State, x: &ProxyShape, b: &str) -> f64 {
    let pb = proxies(state, b);
    let mut best = 0.0f64;
    for y in pb.iter() {
        if !x.aabb.intersects(&y.aabb) {
            continue;
        }
        if let Ok(Some(c)) = query::contact(&x.iso, x.shape.as_ref(), &y.iso, y.shape.as_ref(), 0.0) {
            if c.dist < 0.0 {
                let skip = {
                    // b entirely inside the free-space box counts as no surface contact
                    use parry3d_f64::query::PointQuery;
                    y.corners.iter().all(|c| {
                        let p = na::Point3::new(c[0], c[1], c[2]);
                        x.shape.as_ref().contains_point(&x.iso, &p) && (x.shape.as_ref().project_point(&x.iso, &p, false).point - p).norm() > 1e-3
                    })
                };
                if !skip {
                    best = best.max(-c.dist);
                }
            }
        }
    }
    best
}

pub fn clear_stale(_state: &State) {}

/// python's trimesh/FCL collision between object a and object b: None when no contact is produced, else the max contact depth.
/// Convex meshes are fcl.Convex objects (libccd MPR); non-convex meshes are BVHs (per-triangle MPR against a convex, SAT against another BVH).
pub fn pair_depth(state: &State, a: &str, b: &str) -> Option<f64> {
    use super::mpr::*;
    let (oa, ob) = (&state.objs[a], &state.objs[b]);
    let mean = |v: &[V3]| -> V3 {
        let n = v.len() as f64;
        let mut s = [0.0; 3];
        for p in v {
            for k in 0..3 {
                s[k] += p[k];
            }
        }
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let cvx = |o: &ObjState, v: &'_ [V3]| -> (V3, ) { (mean(v),) };
    let _ = cvx;
    match (&oa.geom.convex, &ob.geom.convex) {
        (Some(va), Some(vb)) => {
            let sa = ConvexShape::new(va, mean(va), oa.pose.r, oa.pose.t);
            let sb = ConvexShape::new(vb, mean(vb), ob.pose.r, ob.pose.t);
            penetration(&sa, &sb)
        }
        (Some(va), None) => convex_vs_mesh(va, oa, ob),
        (None, Some(vb)) => convex_vs_mesh(vb, ob, oa),
        (None, None) => {
            let (wa, wb) = (state.world(a), state.world(b));
            super::fcl::mesh_depth(&wa.tris, &wb.tris)
        }
    }
}

fn convex_vs_mesh(vc: &[V3], oc: &ObjState, om: &ObjState) -> Option<f64> {
    use super::mpr::*;
    let n = vc.len() as f64;
    let mut s = [0.0; 3];
    for p in vc {
        for k in 0..3 {
            s[k] += p[k];
        }
    }
    let shape = ConvexShape::new(vc, [s[0] / n, s[1] / n, s[2] / n], oc.pose.r, oc.pose.t);
    // world aabb of the convex for culling
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in vc {
        let w = oc.pose.apply(*p);
        for k in 0..3 {
            lo[k] = lo[k].min(w[k]);
            hi[k] = hi[k].max(w[k]);
        }
    }
    let mut best: Option<f64> = None;
    for t in &om.geom.tris {
        let wt = [om.pose.apply(t.v[0]), om.pose.apply(t.v[1]), om.pose.apply(t.v[2])];
        let skip = (0..3).any(|k| {
            let tl = wt[0][k].min(wt[1][k]).min(wt[2][k]);
            let th = wt[0][k].max(wt[1][k]).max(wt[2][k]);
            tl > hi[k] + 1e-6 || th < lo[k] - 1e-6
        });
        if skip {
            continue;
        }
        let tri = TriShape::new(t.v, om.pose.r, om.pose.t);
        if let Some(d) = penetration(&shape, &tri) {
            best = Some(best.map_or(d, |x: f64| x.max(d)));
        }
    }
    best
}

/// FCL-semantics max contact depth between object `a` and the set `others` (None: no contact produced)
pub fn fcl_max_depth(state: &State, a: &str, others: &[&String]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for b in others {
        if let Some(d) = pair_depth(state, a, b) {
            best = Some(best.map_or(d, |x: f64| x.max(d)));
        }
    }
    best
}

/// convex free-space box (trimesh box, `bverts` local, pose = its world transform) against object `ob`
pub fn free_box_depth(bverts: &[V3], fpose: &Pose, ob: &ObjState) -> Option<f64> {
    use super::mpr::*;
    if !super::layout2d::is_room(ob) {
        // 2D: the free box and the object as prisms
        let w: Vec<V3> = bverts.iter().map(|v| fpose.apply(*v)).collect();
        let fb = super::layout2d::prism_of(&w);
        let d = super::layout2d::prisms_of_obj(ob).iter().map(|p| super::layout2d::prism_depth(&fb, p)).fold(0.0f64, f64::max);
        return if d > 0.0 { Some(d) } else { None };
    }
    let shape = ConvexShape::new(bverts, [0.0; 3], fpose.r, fpose.t);
    match &ob.geom.convex {
        Some(vb) => {
            let n = vb.len() as f64;
            let mut s = [0.0; 3];
            for p in vb.iter() {
                for k in 0..3 {
                    s[k] += p[k];
                }
            }
            let sb = ConvexShape::new(vb, [s[0] / n, s[1] / n, s[2] / n], ob.pose.r, ob.pose.t);
            // python: b_col.in_collision_other(a_free_col) -> the furniture is object 1, the free box object 2
            penetration(&sb, &shape)
        }
        None => {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in bverts {
                let w = fpose.apply(*p);
                for k in 0..3 {
                    lo[k] = lo[k].min(w[k]);
                    hi[k] = hi[k].max(w[k]);
                }
            }
            let mut best: Option<f64> = None;
            for t in &ob.geom.tris {
                let wt = [ob.pose.apply(t.v[0]), ob.pose.apply(t.v[1]), ob.pose.apply(t.v[2])];
                if (0..3).any(|k| wt[0][k].min(wt[1][k]).min(wt[2][k]) > hi[k] + 1e-6 || wt[0][k].max(wt[1][k]).max(wt[2][k]) < lo[k] - 1e-6) {
                    continue;
                }
                let tri = TriShape::new(t.v, ob.pose.r, ob.pose.t);
                if let Some(d) = penetration(&shape, &tri) {
                    best = Some(best.map_or(d, |x: f64| x.max(d)));
                }
            }
            best
        }
    }
}

/// per-house state: proxies are keyed by object name and version, which repeat between houses
pub fn reset_caches() {
    CACHE.with(|c| c.borrow_mut().clear());
    SHAPES.with(|c| c.borrow_mut().clear());
}
