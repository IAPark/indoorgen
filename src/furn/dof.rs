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
    (crate::bl32::global_polygon_normal(&m, ln, rev), crate::bl32::mat4_mul_point(&m, f32v(t.v[0])))
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

/// random point on the (coplanar tagged) faces of plane `plane` of object b (move_obj_random_pt / random_sample_point)
pub fn random_point_on(state: &State, b: &str, tags: &TagSet, plane: usize, rng: &mut Rng) -> V3 {
    let o = &state.objs[b];
    let (pos, neg) = tag_masks(tags);
    let idxs = plane_mask(&o.geom, &o.pose, pos, neg, plane);
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
    if std::env::var("RPODUMP").is_ok() && idxs.len() == 64 {
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
    if std::env::var("RPODUMP").is_ok() { eprintln!("RPOP w={:?} local={:?} global={:?}", [a, b2, c], local, g); }
    g
}

fn project_frame(n: V3, p: V3) -> [f64; 2] {
    let (u, v) = plane_frame(n);
    [dot(p, u), dot(p, v)]
}

/// stability.stable_against
pub fn stable_against(state: &State, name: &str, rs: &RelState, g: &Geo) -> bool {
    let pa_list = state.planes(name, &g.child);
    let pb_list = state.planes(&rs.target, &g.parent);
    if rs.child_plane >= pa_list.len() || rs.parent_plane >= pb_list.len() {
        if std::env::var("STAB").is_ok() { eprintln!("STAB {name} fail #1"); } return false;
    }
    let (pa, pb) = (pa_list[rs.child_plane], pb_list[rs.parent_plane]);
    let (na, _) = plane_normal(state, name, pa, false);
    let (nb, origin_b) = plane_normal(state, &rs.target, pb, g.rev_normal);
    let d = dot(na, nb);
    if !((d.abs() - 1.0).abs() <= 1e-2 + 1e-5 || (d + 1.0).abs() <= 1e-2) {
        if std::env::var("STAB").is_ok() { eprintln!("STAB {name} fail #2"); } return false;
    }
    let sa = state.world(name);
    let so = &state.objs[&rs.target];
    let (pos, neg) = tag_masks(&g.parent);
    let mask = plane_mask(&so.geom, &so.pose, pos, neg, pb);
    if mask.is_empty() {
        if std::env::var("STAB").is_ok() { eprintln!("STAB {name} fail #3"); } return false;
    }
    let sbw = state.world(&rs.target);
    let b_tris: Vec<[V3; 3]> = mask.iter().map(|&i| sbw.tris[i]).collect();
    let proj_a = projected(&sa.tris, nb, origin_b);
    let proj_b = projected(&b_tris, nb, origin_b);
    let (Some(proj_a), Some(proj_b)) = (proj_a, proj_b) else { return false };
    let res = if g.check_z {
        if std::env::var("STAB").is_ok() && name.contains("WallArt") { eprintln!("STABZ {name} nb={:?} a={:?} b={:?} nA={} nB={} areaA={:.3} areaB={:.3} outside={:.4}", nb, shp::bounds(&proj_a), shp::bounds(&proj_b), sa.tris.len(), mask.len(), shp::area(&proj_a), shp::area(&proj_b), shp::area(&shp::difference(&proj_a, &shp::buffer(&proj_b, 1e-2)))); }
        shp::within(&proj_a, &shp::buffer(&proj_b, 1e-2))
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
        if std::env::var("STAB").is_ok() && name.contains("WallArt") {
            eprintln!("STABX {name} nb={:?} ang={ang:.1} a_y=({ay0:.2},{ay1:.2}) b_y=({by0:.2},{by1:.2}) a_x={:?} b_x={:?}", nb.map(|v| (v * 100.0).round() / 100.0), (shp::bounds(&ra).0, shp::bounds(&ra).2), (shp::bounds(&rb).0, shp::bounds(&rb).2));
        }
        by0 <= ay0 && ay1 <= by1
    };
    if !res {
        if std::env::var("STAB").is_ok() { eprintln!("STAB {name} fail #4"); } return false;
    }
    let wa = &sa.tris[pa];
    for v in wa {
        let dist = dist_to_plane(*v, origin_b, nb);
        if (dist - g.margin).abs() > 1e-2 + 1e-5 * g.margin.abs() {
            if std::env::var("STAB").is_ok() { eprintln!("STAB {name} fail #5"); } return false;
        }
    }
    true
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

fn check_post_move_validity_inner(state: &State, name: &str) -> bool {
    let nc = no_collision_tag();
    let others: Vec<&String> = state.objs.iter().filter(|(k, o)| k.as_str() != name && !o.tags.contains(nc)).map(|(k, _)| k).collect();
    if others.is_empty() {
        return true;
    }
    if !all_relations_valid(state, name) {
        return false;
    }
    if state.objs[name].tags.contains(nc) {
        return true;
    }
    col::fcl_max_depth(state, name, &others).map_or(true, |d| d <= 1e-4)
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
            if std::env::var("DBG2").is_ok() {
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
        if std::env::var("DBG2").is_ok() {
            eprintln!("  [{name}] check_init_valid false (picks {})", picks.len());
        }
        return None;
    }
    if dof == 0 {
        let mut pose = state.objs[name].pose;
        pose.translate(t);
        state.set_pose(name, pose);
    } else if dof == 1 {
        assert!(picks.len() == 2);
        let (p1, p2) = (&picks[0], &picks[1]);
        // parent1 plane polygon vs parent2 plane points, projected along parent1's normal (origin (0,0,0))
        let (n1, _) = plane_normal(state, &p1.parent, p1.parent_plane, false);
        let po1 = &state.objs[&p1.parent];
        let (pos1, neg1) = tag_masks(&p1.parent_tags);
        let m1 = plane_mask(&po1.geom, &po1.pose, pos1, neg1, p1.parent_plane);
        let w1 = state.world(&p1.parent);
        let t1: Vec<[V3; 3]> = m1.iter().map(|&i| w1.tris[i]).collect();
        let proj1 = projected(&t1, n1, [0., 0., 0.]).expect("parent1 projection");
        let po2 = &state.objs[&p2.parent];
        let (pos2, neg2) = tag_masks(&p2.parent_tags);
        let m2 = plane_mask(&po2.geom, &po2.pose, pos2, neg2, p2.parent_plane);
        let w2 = state.world(&p2.parent);
        let padded = shp::buffer(&proj1, 1e-1);
        let all_in = m2.iter().all(|&i| w2.tris[i].iter().all(|p| shp::contains_pt(&padded, project_frame_origin(n1, *p))));
        if crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   DOF1 all_in={all_in} p1plane={} p2plane={} n_m2={}", p1.parent_plane, p2.parent_plane, m2.len()); }
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
pub fn try_apply_relation_constraints(state: &mut State, name: &str, rng: &mut Rng) -> bool {
    for _ in 0..10 {
        let Some(parents) = apply_relations_surfacesample(state, name, rng) else {
            if std::env::var("DBG2").is_ok() { eprintln!("  [{name}] apply failed (init invalid)"); }
            return false;
        };
        let valid = check_post_move_validity(state, name);
        if std::env::var("DBG2").is_ok() && !valid {
            let rel_ok = all_relations_valid(state, name);
            eprintln!("  [{name}] post-move invalid: relations_ok={rel_ok} pos={:?}", state.objs[name].pose.t);
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
