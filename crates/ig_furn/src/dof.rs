//! Relation application (geometry/dof.py, stability.py, validity.py): snap objects to parent planes.
use super::col;
use super::geom::*;
use super::relations::*;
use super::state::*;
use super::tags::*;
use crate::mesh::{cross, dot, sub, V3};
use crate::rng::Rng;
use crate::shp;

fn f32v(p: V3) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

fn up(v: [f32; 3]) -> V3 {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}

/// blender-side (float32) global polygon normal and first-vertex position of triangle `ti` of object `name`
fn plane_normal32(state: &State, name: &str, ti: usize, rev: bool) -> ([f32; 3], [f32; 3]) {
    let o = &state.objs[name];
    let t = &o.geom.tris[ti];
    let m = o.pose.o.matrix_world();
    let ln = crate::bl32::poly_normal(f32v(t.v[0]), f32v(t.v[1]), f32v(t.v[2]));
    let mut n = crate::bl32::global_polygon_normal(&m, ln, rev);
    // dense mode: the exact Blender even-thickness solidify port leaves room floor/ceiling vertices at angled corners up to ~2 cm off
    // the plane, so triangles there slope by 3-4 degrees and everything stable against them came out tilted. Planes within 10 degrees of
    // horizontal are taken as horizontal (python-parity mode keeps the raw normal).
    if crate::state::dense_mode() && n[2].abs() > 0.985 && n[2].abs() < 1.0 {
        n = [0.0, 0.0, n[2].signum()];
    }
    (n, crate::bl32::mat4_mul_point(&m, f32v(t.v[0])))
}

fn plane_normal(state: &State, name: &str, ti: usize, rev: bool) -> (V3, V3) {
    let (n, p) = plane_normal32(state, name, ti, rev);
    (up(n), up(p))
}

/// rotate `name` so plane `a_plane` faces opposite to `b_plane`, then translate to touch with margin (stability.snap_against)
pub fn snap_against(state: &mut State, a: &str, b: &str, a_plane: usize, b_plane: usize, margin: f64, rev: bool) {
    use crate::bl32::{vdot, vneg, vnormalized, vscale, vsub};
    let (na, _pa) = plane_normal32(state, a, a_plane, false);
    let (nb0, pb) = plane_normal32(state, b, b_plane, rev);
    let nb = vneg(nb0);
    let mut axis = cross(up(na), up(nb));
    if norm(axis) > 1e-5 {
        axis = unit(axis);
    } else {
        axis = [0., 0., 1.];
    }
    let angle = vdot(na, nb).clamp(-1.0, 1.0).acos();
    if crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   SNAP {a}->{b} na={na:?} nb={nb:?} axis={axis:?} angle={angle:.9} margin={margin}"); }
    let mut pose = state.objs[a].pose;
    pose.rotate_axis(axis, angle);
    state.set_pose(a, pose);
    let (_na2, pa2) = plane_normal32(state, a, a_plane, false);
    let distance = vdot(vsub(pa2, pb), nb);
    let tr = vscale(vnormalized(nb), -(distance + margin));
    let mut pose = state.objs[a].pose;
    pose.translate(up(tr));
    state.set_pose(a, pose);
}

thread_local! {
    static MASKS: std::cell::RefCell<crate::dhash::FxMap<(String, u64, u32, u32, usize), std::sync::Arc<Vec<usize>>>> = std::cell::RefCell::new(Default::default());
}
/// plane_mask of object `name`, cached per object version (parents of stability tests and surface samples are static)
fn plane_mask_c(state: &State, name: &str, pos: u32, neg: u32, plane: usize) -> std::sync::Arc<Vec<usize>> {
    let o = &state.objs[name];
    let k = (name.to_string(), o.version, pos, neg, plane);
    if let Some(m) = MASKS.with(|c| c.borrow().get(&k).cloned()) {
        return m;
    }
    let m = std::sync::Arc::new(plane_mask(&o.geom, &o.pose, pos, neg, plane));
    MASKS.with(|c| { let mut c = c.borrow_mut(); if c.len() > 20000 { c.clear(); } c.insert(k, m.clone()); });
    m
}

/// random point on the (coplanar tagged) faces of plane `plane` of object b (move_obj_random_pt / random_sample_point)
pub fn random_point_on(state: &State, b: &str, tags: &TagSet, plane: usize, rng: &mut Rng) -> V3 {
    let o = &state.objs[b];
    let (pos, neg) = tag_masks(tags);
    let idxs = plane_mask_c(state, b, pos, neg, plane);
    let areas: Vec<f64> = idxs.iter().map(|&i| {
        let t = &o.geom.tris[i];
        // BM_face_calc_area: Newell cross in float32 on local coordinates
        let f = |p: V3| [p[0] as f32, p[1] as f32, p[2] as f32];
        let vs = [f(t.v[0]), f(t.v[1]), f(t.v[2])];
        let mut n = [0.0f32; 3];
        for i in 0..3 {
            let (p, q) = (vs[i], vs[(i + 1) % 3]);
            n[0] += (p[1] - q[1]) * (p[2] + q[2]);
            n[1] += (p[2] - q[2]) * (p[0] + q[0]);
            n[2] += (p[0] - q[0]) * (p[1] + q[1]);
        }
        ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5f32) as f64
    }).collect();
    let mut cum = vec![];
    let mut s = 0.0;
    for a in &areas {
        s += a;
        cum.push(s);
    }
    let r = rng.f64() * s;
    let fi = cum.partition_point(|&c| c < r).min(idxs.len().saturating_sub(1));
    if crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   RPO n={} fi={} r={:.12} lo={:.12} hi={:.12} tot={:.12}", idxs.len(), fi, r, if fi > 0 { cum[fi-1] } else { 0.0 }, cum[fi], s); }
    if ig_core::env_on!("RPODUMP") && idxs.len() == 64 {
        for (k, &i) in idxs.iter().enumerate() { let t = &o.geom.tris[i]; eprintln!("RPOF {k} {i} area={:.9} {:?} {:?} {:?}", areas[k], t.v[0], t.v[1], t.v[2]); }
    }
    let t = &o.geom.tris[idxs[fi]];
    let (a, b2, c) = (rng.f64(), rng.f64(), rng.f64());
    let sum = a + b2 + c;
    let (a, b2, c) = (a / sum, b2 / sum, c / sum);
    let local = [
        a * t.v[0][0] + b2 * t.v[1][0] + c * t.v[2][0],
        a * t.v[0][1] + b2 * t.v[1][1] + c * t.v[2][1],
        a * t.v[0][2] + b2 * t.v[1][2] + c * t.v[2][2],
    ];
    let g = up(crate::bl32::mat4_mul_point(&o.pose.o.matrix_world(), f32v(local)));
    if ig_core::env_on!("RPODUMP") { eprintln!("RPOP w={:?} local={:?} global={:?}", [a, b2, c], local, g); }
    g
}

fn project_frame(n: V3, p: V3) -> [f64; 2] {
    let (u, v) = plane_frame(n);
    [dot(p, u), dot(p, v)]
}

static LAST_FAIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn stab_fail(n: u64) {
    LAST_FAIL.store(n, std::sync::atomic::Ordering::Relaxed);
    if FAILSTAT.load(std::sync::atomic::Ordering::Relaxed) {
        let mut g = PLACE_STATS.lock().unwrap();
        g.get_or_insert_with(Default::default).entry(format!("#fail{n}")).or_default().0 += 1;
    }
}

/// stability.stable_against
pub fn stable_against(state: &State, name: &str, rs: &RelState, g: &Geo) -> bool {
    let pa_list = state.planes(name, &g.child);
    let pb_list = state.planes(&rs.target, &g.parent);
    if rs.child_plane >= pa_list.len() || rs.parent_plane >= pb_list.len() {
        stab_fail(1); if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #1"); } return false;
    }
    let (pa, pb) = (pa_list[rs.child_plane], pb_list[rs.parent_plane]);
    let (na, _) = plane_normal(state, name, pa, false);
    let (nb, origin_b) = plane_normal(state, &rs.target, pb, g.rev_normal);
    let d = dot(na, nb);
    if !((d.abs() - 1.0).abs() <= 1e-2 + 1e-5 || (d + 1.0).abs() <= 1e-2) {
        stab_fail(2); if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #2"); } return false;
    }
    let sa = state.world(name);
    let so = &state.objs[&rs.target];
    let (pos, neg) = tag_masks(&g.parent);
    let mask = plane_mask_c(state, &rs.target, pos, neg, pb);
    if mask.is_empty() {
        stab_fail(3); if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #3"); } return false;
    }
    // the child plane's vertices must sit at the margin from the parent plane (cheap, so tested before the polygon projections;
    // a pure conjunction, the result is unchanged)
    {
        let wa = &sa.tris[pa];
        for v in wa {
            let dist = dist_to_plane(*v, origin_b, nb);
            if (dist - g.margin).abs() > 1e-2 + 1e-5 * g.margin.abs() {
                stab_fail(5); if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #5"); } return false;
            }
        }
    }
    if !ig_core::env_on!("NO_FAST2D") {
        return stable_2d(state, name, rs, g, &sa, &mask, pb, nb, origin_b);
    }
    let sbw = state.world(&rs.target);
    let b_tris: Vec<[V3; 3]> = mask.iter().map(|&i| sbw.tris[i]).collect();
    let proj_a = projected(&sa.tris, nb, origin_b);
    let proj_b = projected(&b_tris, nb, origin_b);
    let (Some(proj_a), Some(proj_b)) = (proj_a, proj_b) else { return false };
    // wasm / FAST_STABLE: convex parent support -> half-plane test instead of buffer + within
    if g.check_z && ig_core::envx::fast_stable() {
        if let Some(r) = shp::within_convex_offset(&proj_a, &proj_b, 1e-2) {
            if !r {
                stab_fail(4); return false;
            }
            return true;
        }
    }
    let proj_b_buf = if g.check_z { crate::geom::projected_buffer(&b_tris, nb, origin_b, 1e-2) } else { None };
    let res = if g.check_z {
        if ig_core::env_on!("STAB") && (name.contains("WallArt") || name.contains("KitchenSpace")) { eprintln!("STABZ {name} nb={:?} a={:?} b={:?} nA={} nB={} areaA={:.3} areaB={:.3} outside={:.4}", nb, shp::bounds(&proj_a), shp::bounds(&proj_b), sa.tris.len(), mask.len(), shp::area(&proj_a), shp::area(&proj_b), shp::area(&shp::difference(&proj_a, &shp::buffer(&proj_b, 1e-2)))); }
        match &proj_b_buf {
            Some(b) => shp::within(&proj_a, b),
            None => shp::within(&proj_a, &shp::buffer(&proj_b, 1e-2)),
        }
    } else {
        // rotate so the projection of +z becomes the x axis; compare extents along y
        let z2 = {
            let (u, v) = plane_frame(nb);
            [dot([0., 0., 1.], u), dot([0., 0., 1.], v)]
        };
        let ang = z2[1].atan2(z2[0]).to_degrees();
        let ra = shp::rotate_origin(&proj_a, ang);
        let rb = shp::rotate_origin(&proj_b, ang);
        let (_, ay0, _, ay1) = shp::bounds(&ra);
        let (_, by0, _, by1) = shp::bounds(&rb);
        if ig_core::env_on!("STAB") && name.contains("WallArt") {
            eprintln!("STABX {name} nb={:?} ang={ang:.1} a_y=({ay0:.2},{ay1:.2}) b_y=({by0:.2},{by1:.2}) a_x={:?} b_x={:?}", nb.map(|v| (v * 100.0).round() / 100.0), (shp::bounds(&ra).0, shp::bounds(&ra).2), (shp::bounds(&rb).0, shp::bounds(&rb).2));
        }
        by0 <= ay0 && ay1 <= by1
    };
    if !res {
        stab_fail(4); if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #4"); } return false;
    }
    true
}

thread_local! {
    static UVERTS: std::cell::RefCell<crate::dhash::FxMap<usize, (std::sync::Arc<ObjGeom>, std::sync::Arc<Vec<V3>>)>> = std::cell::RefCell::new(Default::default());
}
/// distinct local vertices of a geometry (cached by Arc identity; the cache holds the Arc so the address cannot be reused)
fn unique_verts(g: &std::sync::Arc<ObjGeom>) -> std::sync::Arc<Vec<V3>> {
    let k = std::sync::Arc::as_ptr(g) as usize;
    if let Some(v) = UVERTS.with(|c| c.borrow().get(&k).filter(|(a, _)| std::sync::Arc::ptr_eq(a, g)).map(|(_, v)| v.clone())) {
        return v;
    }
    let mut pts: Vec<V3> = g.tris.iter().flat_map(|t| t.v.iter().copied()).collect();
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup();
    let v = std::sync::Arc::new(pts);
    UVERTS.with(|c| { let mut c = c.borrow_mut(); if c.len() > 4000 { c.clear(); } c.insert(k, (g.clone(), v.clone())); });
    v
}

thread_local! {
    /// apply_relations_surfacesample (dof 1): are parent 2's plane points inside parent 1's padded plane polygon
    static ALLIN: std::cell::RefCell<crate::dhash::FxMap<(String, u64, usize, u32, u32, String, u64, usize, u32, u32), bool>> = std::cell::RefCell::new(Default::default());
    /// projected parent support surfaces (rings in the plane frame), keyed by target / version / plane / frame
    static SUPPORT2D: std::cell::RefCell<crate::dhash::FxMap<(String, u64, usize, [u64; 6]), std::sync::Arc<Vec<Vec<[f64; 2]>>>>> = std::cell::RefCell::new(Default::default());
}

/// the 2D tail of `stable_against`: the child's projected vertex hull inside the (cached) projected parent surface, 1 cm tolerance
/// (check_z), or the child's extent along the projected +z inside the parent's (walls)
#[allow(clippy::too_many_arguments)]
fn stable_2d(state: &State, name: &str, rs: &RelState, g: &Geo, sa: &World, mask: &[usize], pb: usize, nb: V3, origin_b: V3) -> bool {
    use super::layout2d::{hull, in_rings, point_in_convex, ring_dist, rings};
    let n = unit(nb);
    let (u, v) = plane_frame(n);
    let pr = |p: &V3| { let r = sub(*p, origin_b); [dot(r, u), dot(r, v)] };
    let so = &state.objs[&rs.target];
    let key = (rs.target.clone(), so.version, pb, [n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), origin_b[0].to_bits(), origin_b[1].to_bits(), origin_b[2].to_bits()]);
    let b = match SUPPORT2D.with(|c| c.borrow().get(&key).cloned()) {
        Some(b) => b,
        None => {
            let sbw = state.world(&rs.target);
            let b_tris: Vec<[V3; 3]> = mask.iter().map(|&i| sbw.tris[i]).collect();
            let r = std::sync::Arc::new(projected(&b_tris, nb, origin_b).map(|p| rings(&p)).unwrap_or_default());
            SUPPORT2D.with(|c| { let mut c = c.borrow_mut(); if c.len() > 4000 { c.clear(); } c.insert(key, r.clone()); });
            r
        }
    };
    if b.is_empty() {
        return false;
    }
    // the child's distinct local vertices (cached per geometry), posed: a box placeholder has 8, its triangle soup 36 points
    let co = &state.objs[name];
    let local = unique_verts(&co.geom);
    let a = hull(local.iter().map(|p| pr(&co.pose.apply(*p))).collect());
    let _ = sa;
    if a.len() < 3 && g.check_z {
        return false;
    }
    let ok = if g.check_z {
        a.iter().all(|p| in_rings(&b, *p) || ring_dist(&b, *p) <= 1e-2)
            && (a.len() < 3 || b.iter().flatten().all(|q| point_in_convex(&a, *q) <= 1e-2))
    } else {
        let z2 = [dot([0., 0., 1.], u), dot([0., 0., 1.], v)];
        let ang = z2[1].atan2(z2[0]);
        let (sn, cs) = ang.sin_cos();
        let y = |p: &[f64; 2]| p[0] * sn + p[1] * cs;
        let (ay0, ay1) = a.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(y(p)), hi.max(y(p))));
        let (by0, by1) = b.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(y(p)), hi.max(y(p))));
        by0 <= ay0 && ay1 <= by1
    };
    if !ok {
        stab_fail(4);
        if ig_core::env_on!("STAB") { eprintln!("STAB {name} fail #4 (2d)"); }
    }
    ok
}

pub fn coplanar(state: &State, name: &str, rs: &RelState, g: &Geo) -> bool {
    let pa_list = state.planes(name, &g.child);
    let pb_list = state.planes(&rs.target, &g.parent);
    if rs.child_plane >= pa_list.len() || rs.parent_plane >= pb_list.len() {
        return false;
    }
    let (pa, pb) = (pa_list[rs.child_plane], pb_list[rs.parent_plane]);
    let (na, _) = plane_normal(state, name, pa, false);
    let (nb, origin_b) = plane_normal(state, &rs.target, pb, g.rev_normal);
    let d = dot(na, nb);
    if !((d.abs() - 1.0).abs() <= 1e-2 + 1e-5 || (d + 1.0).abs() <= 1e-2) {
        return false;
    }
    let sa = state.world(name);
    for v in &sa.tris[pa] {
        let dist = dist_to_plane(*v, origin_b, nb);
        if (dist - g.margin).abs() > 1e-2 + 1e-5 * g.margin.abs() {
            return false;
        }
    }
    true
}

pub fn all_relations_valid(state: &State, name: &str) -> bool {
    for rs in &state.objs[name].relations {
        match &rs.relation {
            Relation::Geometry(g) if g.kind == GeoKind::StableAgainst => {
                if !stable_against(state, name, rs, g) {
                    return false;
                }
            }
            Relation::Geometry(g) if g.kind == GeoKind::CoPlanar => {
                if !coplanar(state, name, rs, g) {
                    return false;
                }
            }
            // non-geometric relations (room features hold a Traverse relation to their room): nothing to check
            Relation::Traverse | Relation::Any => {}
            r => panic!("Unhandled relation {r:?}"),
        }
    }
    true
}

pub fn no_collision_tag() -> Tag {
    Tag::sem("NoCollision")
}

/// validity.check_post_move_validity
pub fn check_post_move_validity(state: &State, name: &str) -> bool {
    let r = check_post_move_validity_inner(state, name);
    if crate::rng::TRACE_ON.with(|t| t.get()) {
        let o = &state.objs[name];
        let rz = o.pose.r[1][0].atan2(o.pose.r[0][0]);
        let why = if r { String::new() } else if !all_relations_valid(state, name) { "relations".to_string() } else {
            let nc = no_collision_tag();
            let others: Vec<&String> = state.objs.iter().filter(|(k, o)| k.as_str() != name && !o.tags.contains(nc)).map(|(k, _)| k).collect();
            format!("collision depth {:.5} {:?}", col::max_depth_against(state, name, &others), others.iter().map(|o| (o.as_str(), col::depth(state, name, o))).filter(|x| x.1 > 1e-9).collect::<Vec<_>>())
        };
        eprintln!("RT   POSTVALID {name} {r} [{:.5}, {:.5}, {:.5}] {:.4} dims {:?} bb0 {:?} {why} FULL z={:.9} lo={:?} hi={:?} R={:?}", o.pose.t[0], o.pose.t[1], o.pose.t[2], rz, o.geom.dims().map(|v| (v * 1e6).round() / 1e6), o.geom.lo.map(|v| (v * 1e6).round() / 1e6), o.pose.t[2], o.geom.lo, o.geom.hi, o.pose.r);
    }
    r
}

static COUNTER_OVER_WINDOWS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// dense mode: kitchen counter runs (KitchenSpaceFactory, 2.4 m placeholder incl. upper cabinets) may stand in front of windows
/// (python's cutter boxes make most window walls unusable, leaving kitchens without any counter)
pub fn set_counter_over_windows(on: bool) { COUNTER_OVER_WINDOWS.store(on, std::sync::atomic::Ordering::Relaxed); }

fn check_post_move_validity_inner(state: &State, name: &str) -> bool {
    let nc = no_collision_tag();
    let class = state.objs[name].generator.as_ref().map(|g| g.class.as_str()).unwrap_or("");
    let skip_win = COUNTER_OVER_WINDOWS.load(std::sync::atomic::Ordering::Relaxed) && (
        (class == "KitchenSpaceFactory" && !state.objs.iter().any(|(k, o)| k.as_str() != name && o.generator.as_ref().map_or(false, |g| g.class == "KitchenSpaceFactory")))
        // beds under a window are common; small bedrooms with windows on the free walls got no bed at all (headboard vs the cutter box)
        || class == "BedFactory");
    let win = Tag::sem("Window");
    let others: Vec<&String> = state.objs.iter().filter(|(k, o)| k.as_str() != name && !o.tags.contains(nc) && !(skip_win && o.tags.contains(win.clone()))).map(|(k, _)| k).collect();
    if others.is_empty() {
        return true;
    }
    if !all_relations_valid(state, name) {
        return false;
    }
    if state.objs[name].tags.contains(nc) {
        return true;
    }
    // 2D: prisms against every other object, the house floor instead of the room meshes, then the layout rules (walkways, hallways)
    for b in &others {
        if super::layout2d::is_room(&state.objs[b.as_str()]) {
            continue;
        }
        if super::layout2d::depth(state, name, b) > 1e-4 {
            return false;
        }
    }
    if super::layout2d::outside_depth(state, name) > 1e-4 {
        return false;
    }
    super::layout2d::layout_ok(state, name)
}

fn lstsq_3(a: &[V3], c: &[f64]) -> (V3, usize, f64) {
    // min-norm least squares for m x 3 system via normal equations on up to 3 rows (m<=3), returns (t, rank, residual^2)
    let m = a.len();
    // solve using Gram–Schmidt on rows: A has rank<=m; min-norm solution t = A^T (A A^T)^+ c
    let mut g = vec![vec![0.0; m]; m];
    for i in 0..m {
        for j in 0..m {
            g[i][j] = dot(a[i], a[j]);
        }
    }
    // pseudo-inverse of small symmetric g via eigen-free Gaussian elimination with rank detection
    let (y, rank) = solve_sym_psd(&g, c);
    let mut t = [0.0; 3];
    for i in 0..m {
        for k in 0..3 {
            t[k] += a[i][k] * y[i];
        }
    }
    let mut res = 0.0;
    for i in 0..m {
        let r = dot(a[i], t) - c[i];
        res += r * r;
    }
    (t, rank, res)
}

fn solve_sym_psd(g: &[Vec<f64>], c: &[f64]) -> (Vec<f64>, usize) {
    let n = g.len();
    // Gram-Schmidt style: select independent rows by pivoting
    let mut m: Vec<Vec<f64>> = g.iter().enumerate().map(|(i, r)| { let mut r = r.clone(); r.push(c[i]); r }).collect();
    let mut piv_cols = vec![];
    let mut row = 0;
    for col in 0..n {
        let mut best = row;
        for r in row..n {
            if m[r][col].abs() > m[best][col].abs() {
                best = r;
            }
        }
        if row >= n || m[best][col].abs() < 1e-9 {
            continue;
        }
        m.swap(row, best);
        let p = m[row][col];
        for k in 0..=n {
            m[row][k] /= p;
        }
        for r in 0..n {
            if r != row {
                let f = m[r][col];
                for k in 0..=n {
                    m[r][k] -= f * m[row][k];
                }
            }
        }
        piv_cols.push(col);
        row += 1;
    }
    let mut y = vec![0.0; n];
    for (r, &col) in piv_cols.iter().enumerate() {
        y[col] = m[r][n];
    }
    (y, piv_cols.len())
}

struct PlanePick {
    child_name_plane: usize,
    parent: String,
    parent_plane: usize,
    margin: f64,
    rev: bool,
    parent_tags: TagSet,
    relation: Relation,
}

/// dof.check_init_valid: rotate to satisfy plane normals, then solve the translation system.
/// Returns (valid, dof, translation).
fn check_init_valid(state: &mut State, name: &str, picks: &[PlanePick]) -> (bool, usize, V3) {
    let get_rot = |state: &State, p: &PlanePick| -> (V3, f64, V3) {
        use crate::bl32::{vdot, vneg};
        let (na, _) = plane_normal32(state, name, p.child_name_plane, false);
        let (nb, _) = plane_normal32(state, &p.parent, p.parent_plane, p.rev);
        let nb = vneg(nb);
        let mut axis = cross(up(na), up(nb));
        if norm(axis) > 1e-3 {
            axis = unit(axis);
        } else {
            axis = [0., 0., 1.];
        }
        (axis, vdot(na, nb).clamp(-1.0, 1.0).acos(), up(nb))
    };
    let (axis, angle, first_nb) = get_rot(state, &picks[0]);
    let mut pose = state.objs[name].pose;
    pose.rotate_axis(axis, angle);
    state.set_pose(name, pose);
    let mut dof_remaining = true;
    for p in &picks[1..] {
        let (axis, angle, _) = get_rot(state, p);
        if angle.abs() <= 1e-1 {
            continue;
        }
        let allowed = (0..3).all(|k| (axis[k] - first_nb[k]).abs() <= 1e-2 + 1e-5 * first_nb[k].abs()) || (0..3).all(|k| (axis[k] + first_nb[k]).abs() <= 1e-2 + 1e-5 * first_nb[k].abs());
        if dof_remaining && allowed {
            let mut pose = state.objs[name].pose;
            pose.rotate_axis(axis, angle);
            state.set_pose(name, pose);
            dof_remaining = false;
        } else {
            return (false, 0, [0.; 3]);
        }
    }
    solve_contacts(state, name, picks)
}

/// translation (min-norm) that puts every picked child plane in contact (margin) with its parent plane: (valid, dof, t)
fn solve_contacts(state: &State, name: &str, picks: &[PlanePick]) -> (bool, usize, V3) {
    let mut a = vec![];
    let mut c = vec![];
    for p in picks {
        use crate::bl32::{vadd, vdot, vscale, vsub};
        let (_, pa) = plane_normal32(state, name, p.child_name_plane, false);
        let (nb, pb) = plane_normal32(state, &p.parent, p.parent_plane, p.rev);
        let pb = vadd(pb, vscale(nb, p.margin));
        a.push(up(nb));
        c.push(vdot(nb, vsub(pb, pa)));
    }
    let (t, rank, res) = lstsq_3(&a, &c);
    if res < 1e-3 {
        (true, 3 - rank, t)
    } else {
        (false, 0, [0.; 3])
    }
}


// ---------------------------------------------------------------------------------------------------------------------------
// dense mode: constructive pose sampling. The python solver draws a random point on the parent plane, snaps the object onto it and
// only then (GEOS projection + buffer + within, collision) learns that the footprint sticks out of the support polygon or through
// a wall. Here candidate positions are screened first with the cheap necessary part of that test (every projected vertex of the
// child must lie inside the buffered parent footprint, or inside the parent's extent for check_z=false relations); only a
// position that passes goes through the exact snap + validity pipeline, and a plane with no feasible candidate fails at once.
static CONSTRUCTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CAND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(64);
pub fn set_constructive(on: bool, candidates: usize) {
    CONSTRUCTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
    CAND.store(candidates.max(1), std::sync::atomic::Ordering::Relaxed);
}

struct PlaneSampler {
    tris: Vec<[V3; 3]>,
    cum: Vec<f64>,
}

impl PlaneSampler {
    fn new(state: &State, b: &str, tags: &TagSet, plane: usize) -> Option<PlaneSampler> {
        let o = &state.objs[b];
        let (pos, neg) = tag_masks(tags);
        let idxs = plane_mask_c(state, b, pos, neg, plane);
        if idxs.is_empty() {
            return None;
        }
        let w = state.world(b);
        let tris: Vec<[V3; 3]> = idxs.iter().map(|&i| w.tris[i]).collect();
        let mut cum = vec![];
        let mut s = 0.0;
        for t in &tris {
            s += tri_area(t);
            cum.push(s);
        }
        if s <= 0.0 {
            return None;
        }
        Some(PlaneSampler { tris, cum })
    }
    fn draw(&self, rng: &mut Rng) -> V3 {
        let r = rng.f64() * self.cum[self.cum.len() - 1];
        let fi = self.cum.partition_point(|&c| c < r).min(self.tris.len() - 1);
        let t = &self.tris[fi];
        let (a, b, c) = (rng.f64(), rng.f64(), rng.f64());
        let sum = (a + b + c).max(1e-12);
        let (a, b, c) = (a / sum, b / sum, c / sum);
        [a * t[0][0] + b * t[1][0] + c * t[2][0], a * t[0][1] + b * t[1][1] + c * t[2][1], a * t[0][2] + b * t[1][2] + c * t[2][2]]
    }
}

enum PreKind {
    /// check_z: child vertices inside the buffered parent footprint
    Inside(std::sync::Arc<Rings>),
    /// !check_z: extent along the rotated y axis inside the parent's extent: (sin, cos, child y range, parent y range)
    YRange(f64, f64, (f64, f64), (f64, f64)),
}

struct PreRel {
    kind: PreKind,
    pts: Vec<[f64; 2]>,
    u: V3,
    v: V3,
}

fn build_pre(state: &State, name: &str, p: &PlanePick) -> Option<(PreRel, V3)> {
    let Relation::Geometry(g) = &p.relation else { return None };
    if g.kind != GeoKind::StableAgainst {
        return None;
    }
    let (nb, origin_b) = plane_normal(state, &p.parent, p.parent_plane, g.rev_normal);
    let so = &state.objs[&p.parent];
    let (pos, neg) = tag_masks(&g.parent);
    let mask = plane_mask_c(state, &p.parent, pos, neg, p.parent_plane);
    if mask.is_empty() {
        return None;
    }
    let sbw = state.world(&p.parent);
    let b_tris: Vec<[V3; 3]> = mask.iter().map(|&i| sbw.tris[i]).collect();
    let sa = state.world(name);
    let pts = projected_points(&sa.tris, nb, origin_b);
    if pts.is_empty() {
        return None;
    }
    let (u, v) = plane_frame(nb);
    let kind = if g.check_z {
        PreKind::Inside(projected_buffer_rings(&b_tris, nb, origin_b, 1e-2)?)
    } else {
        let z2 = [dot([0., 0., 1.], u), dot([0., 0., 1.], v)];
        let ang = z2[1].atan2(z2[0]).to_degrees();
        let (sn, cs) = (ang.to_radians().sin(), ang.to_radians().cos());
        let yr = |pts: &[[f64; 2]]| pts.iter().fold((f64::MAX, f64::MIN), |a, q| {
            let y = q[0] * sn + q[1] * cs;
            (a.0.min(y), a.1.max(y))
        });
        let bp = projected_points(&b_tris, nb, origin_b);
        if bp.is_empty() {
            return None;
        }
        PreKind::YRange(sn, cs, yr(&pts), yr(&bp))
    };
    Some((PreRel { kind, pts, u, v }, nb))
}

fn pre_ok(rels: &[PreRel], delta: V3) -> bool {
    for r in rels {
        let d2 = [dot(delta, r.u), dot(delta, r.v)];
        match &r.kind {
            PreKind::Inside(rings) => {
                for q in &r.pts {
                    if !rings.contains([q[0] + d2[0], q[1] + d2[1]]) {
                        return false;
                    }
                }
            }
            PreKind::YRange(sn, cs, (c0, c1), (b0, b1)) => {
                let dy = d2[0] * sn + d2[1] * cs;
                if c0 + dy < b0 - 1e-7 || c1 + dy > b1 + 1e-7 {
                    return false;
                }
            }
        }
    }
    true
}

fn outcome_dbg(state: &State, name: &str, tag: &str) {
    if FAILSTAT.load(std::sync::atomic::Ordering::Relaxed) {
        let class = state.objs[name].generator.as_ref().map_or("?".to_string(), |g| g.class.clone());
        PLACE_STATS.lock().unwrap().get_or_insert_with(Default::default).entry(format!("!{tag} {class}")).or_default().0 += 1;
    }
}

fn verify_dbg(state: &State, name: &str, tag: &str) {
    outcome_dbg(state, name, tag);
    if !ig_core::env_on!("PREVERIFY") {
        return;
    }
    LAST_FAIL.store(0, std::sync::atomic::Ordering::Relaxed);
    let ok = all_relations_valid(state, name);
    let cls = state.objs[name].generator.as_ref().map_or("?".to_string(), |g| g.class.clone());
    eprintln!("PREVERIFY {cls} {tag} ok={ok} fail={}", LAST_FAIL.load(std::sync::atomic::Ordering::Relaxed));
}

/// Some(None): no feasible position (fail the placement); Some(Some(..)): placed; None: not applicable, use the python-style sampling
fn constructive_pose(state: &mut State, name: &str, picks: &[PlanePick], dof: usize, t0: V3, rng: &mut Rng) -> Option<Option<Vec<(String, usize)>>> {
    let ncand = CAND.load(std::sync::atomic::Ordering::Relaxed);
    let result = |picks: &[PlanePick]| Some(Some(picks.iter().map(|p| (p.parent.clone(), p.parent_plane)).collect::<Vec<_>>()));
    if dof == 2 && picks.len() == 1 {
        let p = &picks[0];
        let Relation::Geometry(g) = &p.relation else { return None };
        if g.kind != GeoKind::StableAgainst {
            return None;
        }
        let rz = 2.0 * std::f64::consts::PI * rng.randint(0, 4) as f64 / 4.0;
        let mut pose = state.objs[name].pose;
        pose.set_euler([0., 0., rz]);
        state.set_pose(name, pose);
        let sampler = PlaneSampler::new(state, &p.parent, &p.parent_tags, p.parent_plane)?;
        let rf0 = sampler.draw(rng);
        let mut pose = state.objs[name].pose;
        pose.set_loc(rf0);
        state.set_pose(name, pose);
        snap_against(state, name, &p.parent, p.child_name_plane, p.parent_plane, p.margin, p.rev);
        let Some((pre, nb)) = build_pre(state, name, p) else { return Some(Some(vec![(p.parent.clone(), p.parent_plane)])) };
        let pre = [pre];
        let inplane = |d: V3| -> V3 { let k = dot(d, nb); [d[0] - k * nb[0], d[1] - k * nb[1], d[2] - k * nb[2]] };
        if pre_ok(&pre, [0.0; 3]) {
            verify_dbg(state, name, "dof2-rf0");
            return result(picks);
        }
        for _ in 0..ncand {
            let rf = sampler.draw(rng);
            if pre_ok(&pre, inplane(sub(rf, rf0))) {
                let mut pose = state.objs[name].pose;
                pose.set_loc(rf);
                state.set_pose(name, pose);
                snap_against(state, name, &p.parent, p.child_name_plane, p.parent_plane, p.margin, p.rev);
                verify_dbg(state, name, "dof2-cand");
                return result(picks);
            }
        }
        outcome_dbg(state, name, "dof2-none");
        return Some(None);
    }
    if dof == 1 && picks.len() == 2 {
        let (p1, p2) = (&picks[0], &picks[1]);
        let (n1, _) = plane_normal(state, &p1.parent, p1.parent_plane, false);
        let po1 = &state.objs[&p1.parent];
        let (pos1, neg1) = tag_masks(&p1.parent_tags);
        let m1 = plane_mask(&po1.geom, &po1.pose, pos1, neg1, p1.parent_plane);
        let w1 = state.world(&p1.parent);
        let t1: Vec<[V3; 3]> = m1.iter().map(|&i| w1.tris[i]).collect();
        let Some(proj1) = projected(&t1, n1, [0., 0., 0.]) else { return None };
        let po2 = &state.objs[&p2.parent];
        let (pos2, neg2) = tag_masks(&p2.parent_tags);
        let m2 = plane_mask(&po2.geom, &po2.pose, pos2, neg2, p2.parent_plane);
        let w2 = state.world(&p2.parent);
        let all_in = if ig_core::envx::fast_stable() {
            m2.iter().all(|&i| w2.tris[i].iter().all(|p| shp::within_distance_pt(&proj1, project_frame_origin(n1, *p), 1e-1)))
        } else {
            let padded = shp::buffer(&proj1, 1e-1);
            m2.iter().all(|&i| w2.tris[i].iter().all(|p| shp::contains_pt(&padded, project_frame_origin(n1, *p))))
        };
        let (first, _second) = if all_in && !is_coplanar_rel(&p2.relation) { (p2, p1) } else { (p1, p2) };
        // exact construction instead of two sequential snaps: check_init_valid already rotated the object and solved the contact of
        // both planes (translation `t0`); the remaining freedom is a slide `s` along the intersection line of the two planes
        let (na, _) = plane_normal(state, &p1.parent, p1.parent_plane, p1.rev);
        let (nb2, _) = plane_normal(state, &p2.parent, p2.parent_plane, p2.rev);
        let d = cross(na, nb2);
        if norm(d) < 1e-4 {
            return None;
        }
        let d = unit(d);
        let Some(sampler) = PlaneSampler::new(state, &first.parent, &first.parent_tags, first.parent_plane) else { return None };
        // reference pose: the python-style snaps at a random point (they also finish the small rotations check_init_valid skips),
        // then the exact contact solve removes the position error the sequential snaps leave for non-perpendicular planes
        let _ = t0;
        let rf0 = sampler.draw(rng);
        let mut pose = state.objs[name].pose;
        pose.set_loc(rf0);
        state.set_pose(name, pose);
        snap_against(state, name, &first.parent, first.child_name_plane, first.parent_plane, first.margin, first.rev);
        snap_against(state, name, &_second.parent, _second.child_name_plane, _second.parent_plane, _second.margin, _second.rev);
        let (ok, dof_now, t1) = solve_contacts(state, name, picks);
        if ok && dof_now == 1 {
            let mut pose = state.objs[name].pose;
            pose.translate(t1);
            state.set_pose(name, pose);
        }
        let loc0 = state.objs[name].pose.t;
        let mut pres: Vec<PreRel> = vec![];
        for p in [p1, p2] {
            if let Some((pr, _)) = build_pre(state, name, p) {
                pres.push(pr);
            }
        }
        if pres.is_empty() {
            outcome_dbg(state, name, "dof1-nopre");
            return result(picks);
        }
        let slide = |state: &mut State, s: f64| {
            let mut pose = state.objs[name].pose;
            pose.translate([d[0] * s, d[1] * s, d[2] * s]);
            state.set_pose(name, pose);
        };
        // a first slide drawn like the python sampler would (uniform on the plane), kept when it is feasible, so the distribution is not
        // biased towards s = 0; then more draws
        if pre_ok(&pres, [0.0; 3]) {
            verify_dbg(state, name, "dof1-rf0");
            return result(picks);
        }
        for _ in 0..ncand {
            let s = dot(sub(sampler.draw(rng), loc0), d);
            let dl = [d[0] * s, d[1] * s, d[2] * s];
            if pre_ok(&pres, dl) {
                slide(state, s);
                verify_dbg(state, name, "dof1-cand");
                return result(picks);
            }
        }
        outcome_dbg(state, name, "dof1-none");
        return Some(None);
    }
    None
}

fn is_coplanar_rel(r: &Relation) -> bool {
    matches!(r, Relation::Geometry(g) if g.kind == GeoKind::CoPlanar)
}

/// dof.apply_relations_surfacesample; returns the (parent, plane) list on success
pub fn apply_relations_surfacesample(state: &mut State, name: &str, rng: &mut Rng) -> Option<Vec<(String, usize)>> {
    // stable sort: CoPlanar last
    {
        let o = state.objs.get_mut(name).unwrap();
        o.relations.sort_by_key(|r| is_coplanar_rel(&r.relation));
    }
    let rels = state.objs[name].relations.clone();
    assert!(!rels.is_empty() && rels.len() <= 3, "{name} has {} relations", rels.len());
    let mut picks = vec![];
    for rs in &rels {
        let Relation::Geometry(g) = &rs.relation else { panic!("bad relation {:?}", rs.relation) };
        let parent_planes = state.planes(&rs.target, &g.parent);
        let obj_planes = state.planes(name, &g.child);
        if rs.parent_plane >= parent_planes.len() || rs.child_plane >= obj_planes.len() {
            if ig_core::env_on!("DBG2") {
                eprintln!("  [{name}] plane idx oob: parent {}/{} (target {}) child {}/{}", rs.parent_plane, parent_planes.len(), rs.target, rs.child_plane, obj_planes.len());
            }
            continue;
        }
        let (margin, rev) = match g.kind {
            GeoKind::StableAgainst | GeoKind::CoPlanar => (g.margin, g.rev_normal),
            GeoKind::SupportedBy => (0.0, false),
            GeoKind::Touching => (g.margin, false),
        };
        picks.push(PlanePick {
            child_name_plane: obj_planes[rs.child_plane],
            parent: rs.target.clone(),
            parent_plane: parent_planes[rs.parent_plane],
            margin,
            rev,
            parent_tags: g.parent.clone(),
            relation: rs.relation.clone(),
        });
    }
    if picks.is_empty() {
        return None;
    }
    let (valid, dof, t) = check_init_valid(state, name, &picks);
    if !valid {
        if ig_core::env_on!("DBG2") {
            eprintln!("  [{name}] check_init_valid false (picks {})", picks.len());
        }
        return None;
    }
    if FAILSTAT.load(std::sync::atomic::Ordering::Relaxed) {
        let class = state.objs[name].generator.as_ref().map_or("?".to_string(), |g| g.class.clone());
        PLACE_STATS.lock().unwrap().get_or_insert_with(Default::default).entry(format!("@dof{dof}x{} {class}", picks.len())).or_default().0 += 1;
    }
    if CONSTRUCTIVE.load(std::sync::atomic::Ordering::Relaxed) && (dof == 1 || dof == 2) {
        if let Some(r) = constructive_pose(state, name, &picks, dof, t, rng) {
            return r;
        }
    }
    if dof == 0 {
        let mut pose = state.objs[name].pose;
        pose.translate(t);
        state.set_pose(name, pose);
    } else if dof == 1 {
        assert!(picks.len() == 2);
        let (p1, p2) = (&picks[0], &picks[1]);
        // parent1 plane polygon vs parent2 plane points, projected along parent1's normal (origin (0,0,0))
        // depends only on the two (static) parent surfaces: cached per parent pair / version / plane / tag masks
        let (pos1, neg1) = tag_masks(&p1.parent_tags);
        let (pos2, neg2) = tag_masks(&p2.parent_tags);
        let key = (p1.parent.clone(), state.objs[&p1.parent].version, p1.parent_plane, pos1, neg1, p2.parent.clone(), state.objs[&p2.parent].version, p2.parent_plane, pos2, neg2);
        let cached = ALLIN.with(|c| c.borrow().get(&key).cloned());
        let all_in = match cached { Some(v) => v, None => {
        let (n1, _) = plane_normal(state, &p1.parent, p1.parent_plane, false);
        let po1 = &state.objs[&p1.parent];
        let m1 = plane_mask(&po1.geom, &po1.pose, pos1, neg1, p1.parent_plane);
        let w1 = state.world(&p1.parent);
        let t1: Vec<[V3; 3]> = m1.iter().map(|&i| w1.tris[i]).collect();
        let proj1 = projected(&t1, n1, [0., 0., 0.]).expect("parent1 projection");
        let po2 = &state.objs[&p2.parent];
        let m2 = plane_mask(&po2.geom, &po2.pose, pos2, neg2, p2.parent_plane);
        let w2 = state.world(&p2.parent);
        let v = if ig_core::envx::fast_stable() {
            m2.iter().all(|&i| w2.tris[i].iter().all(|p| shp::within_distance_pt(&proj1, project_frame_origin(n1, *p), 1e-1)))
        } else {
            let padded = shp::buffer(&proj1, 1e-1);
            m2.iter().all(|&i| w2.tris[i].iter().all(|p| shp::contains_pt(&padded, project_frame_origin(n1, *p))))
        };
        ALLIN.with(|c| { let mut c = c.borrow_mut(); if c.len() > 20000 { c.clear(); } c.insert(key, v); });
        v } };
        if crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   DOF1 all_in={all_in} p1plane={} p2plane={}", p1.parent_plane, p2.parent_plane); }
        let (first, second) = if all_in && !is_coplanar_rel(&p2.relation) { (p2, p1) } else { (p1, p2) };
        let rf = random_point_on(state, &first.parent, &first.parent_tags, first.parent_plane, rng);
        let mut pose = state.objs[name].pose;
        pose.set_loc(rf);
        state.set_pose(name, pose);
        snap_against(state, name, &first.parent, first.child_name_plane, first.parent_plane, first.margin, first.rev);
        snap_against(state, name, &second.parent, second.child_name_plane, second.parent_plane, second.margin, second.rev);
    } else if dof == 2 {
        for p in &picks {
            let rz = 2.0 * std::f64::consts::PI * rng.randint(0, 4) as f64 / 4.0;
            let mut pose = state.objs[name].pose;
            pose.set_euler([0., 0., rz]);
            state.set_pose(name, pose);
            match &p.relation {
                Relation::Geometry(g) if g.kind == GeoKind::StableAgainst => {
                    let rf = random_point_on(state, &p.parent, &p.parent_tags, p.parent_plane, rng);
                    let mut pose = state.objs[name].pose;
                    pose.set_loc(rf);
                    state.set_pose(name, pose);
                    snap_against(state, name, &p.parent, p.child_name_plane, p.parent_plane, p.margin, p.rev);
                }
                Relation::Geometry(g) if g.kind == GeoKind::SupportedBy => {
                    let rf = random_point_on(state, &p.parent, &p.parent_tags, p.parent_plane, rng);
                    let mut pose = state.objs[name].pose;
                    pose.set_loc(rf);
                    state.set_pose(name, pose);
                    snap_against(state, name, &p.parent, p.child_name_plane, p.parent_plane, 0.0, false);
                }
                r => panic!("dof 2 relation {r:?}"),
            }
        }
    } else {
        // dof == 3 would mean no constraint at all
    }
    Some(picks.iter().map(|p| (p.parent.clone(), p.parent_plane)).collect())
}

fn project_frame_origin(n: V3, p: V3) -> [f64; 2] {
    project_frame(n, p)
}

fn local_normal64(state: &State, name: &str, ti: usize) -> V3 {
    let t = &state.objs[name].geom.tris[ti];
    up(crate::bl32::poly_normal(f32v(t.v[0]), f32v(t.v[1]), f32v(t.v[2])))
}

/// combined_stability_matrix: product of (I - n n^T) over parent planes
pub fn combined_stability_matrix(state: &State, parents: &[(String, usize)]) -> [[f64; 3]; 3] {
    let mut m = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    for (name, ti) in parents {
        // python uses the polygon's *local* blender normal here (not rotated into the world)
        let n = unit(local_normal64(state, name, *ti));
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (if i == j { 1.0 } else { 0.0 }) - n[i] * n[j];
            }
        }
        let mut out = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    out[i][j] += m[i][k] * r[k][j];
                }
            }
        }
        m = out;
    }
    m
}

/// combine_rotation_constraints
pub fn combine_rotation_constraints(state: &State, parents: &[(String, usize)]) -> Option<V3> {
    let first = unit(local_normal64(state, &parents[0].0, parents[0].1));
    for (name, ti) in &parents[1..] {
        let n = unit(local_normal64(state, name, *ti));
        if (dot(first, n) - 1.0).abs() > 0.01 + 1e-5 {
            return None;
        }
    }
    Some(first)
}

/// dof.try_apply_relation_constraints
static PLACE_TRIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(10);
/// dense mode: more pose samples per placement attempt (python: 10) so big wall objects (kitchen counter runs) find a free stretch of wall
pub fn set_place_tries(n: usize) { PLACE_TRIES.store(n, std::sync::atomic::Ordering::Relaxed); }

/// PLACESTAT=1: per class (placements, pose tries, successful placements) of try_apply_relation_constraints
pub static FAILSTAT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static PLACE_STATS: std::sync::Mutex<Option<std::collections::BTreeMap<String, (u64, u64, u64)>>> = std::sync::Mutex::new(None);
fn place_stat(class: &str, tries: u64, ok: bool) {
    let mut g = PLACE_STATS.lock().unwrap();
    let e = g.get_or_insert_with(Default::default).entry(class.to_string()).or_default();
    e.0 += 1;
    e.1 += tries;
    e.2 += ok as u64;
}
pub fn try_apply_relation_constraints(state: &mut State, name: &str, rng: &mut Rng) -> bool {
    static STAT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let stat = *STAT.get_or_init(|| { let on = ig_core::env_on!("PLACESTAT"); if ig_core::env_on!("FAILSTAT") { FAILSTAT.store(true, std::sync::atomic::Ordering::Relaxed); } on });
    if !stat {
        return try_apply_relation_constraints_inner(state, name, rng, &mut 0);
    }
    let mut tries = 0u64;
    let ok = try_apply_relation_constraints_inner(state, name, rng, &mut tries);
    let class = state.objs[name].generator.as_ref().map_or("?".to_string(), |g| g.class.clone());
    place_stat(&class, tries, ok);
    ok
}

fn try_apply_relation_constraints_inner(state: &mut State, name: &str, rng: &mut Rng, ntries: &mut u64) -> bool {
    let big = state.objs[name].generator.as_ref().map_or(false, |g| matches!(g.class.as_str(), "KitchenSpaceFactory" | "KitchenIslandFactory" | "BedFactory"));
    let tries = if big { PLACE_TRIES.load(std::sync::atomic::Ordering::Relaxed) } else { 10 };
    for _ in 0..tries {
        *ntries += 1;
        let Some(parents) = apply_relations_surfacesample(state, name, rng) else {
            if ig_core::env_on!("DBG2") { eprintln!("  [{name}] apply failed (init invalid)"); }
            return false;
        };
        let valid = if *ntries > 0 && FAILSTAT.load(std::sync::atomic::Ordering::Relaxed) {
            let rel_ok = all_relations_valid(state, name);
            let v = rel_ok && check_post_move_validity(state, name);
            let class = state.objs[name].generator.as_ref().map_or("?".to_string(), |g| g.class.clone());
            let mut g = PLACE_STATS.lock().unwrap();
            let e = g.get_or_insert_with(Default::default).entry(format!("~{class}")).or_default();
            // (tries, relations failed, collision failed)
            e.0 += 1;
            if !rel_ok { e.1 += 1 } else if !v { e.2 += 1 }
            v
        } else {
            check_post_move_validity(state, name)
        };
        if ig_core::env_on!("DBG2") && !valid {
            let rel_ok = all_relations_valid(state, name);
            eprintln!("  [{name}] post-move invalid: relations_ok={rel_ok} pos={:?}", state.objs[name].pose.t);
            if rel_ok {
                let nc = no_collision_tag();
                let others: Vec<&String> = state.objs.iter().filter(|(k, o)| k.as_str() != name && !o.tags.contains(nc)).map(|(k, _)| k).collect();
                eprintln!("    collides with {:?}", others.iter().map(|o| (o.as_str(), col::depth(state, name, o))).filter(|x| x.1 > 1e-4).collect::<Vec<_>>());
            }
        }
        if valid {
            let dt = combined_stability_matrix(state, &parents);
            let dr = combine_rotation_constraints(state, &parents);
            let o = state.objs.get_mut(name).unwrap();
            o.dof_t = Some(dt);
            o.dof_r = dr;
            return true;
        }
    }
    false
}
