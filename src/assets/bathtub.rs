//! bathroom/bathtub.py BathtubFactory.create_asset stage by stage (shared with bathroom_sink.py): freestanding and alcove variants.
use super::bathfree;
use super::bathkit::*;
use crate::dhash::HashMap;
use super::bathroom::Bathtub;
use crate::mesh::Mesh;
use crate::rng::Rng;

pub struct Built {
    pub body: Mesh,
    pub hole: Mesh,
    pub extra: Vec<Mesh>,
    /// (label, python stage key, mesh)
    pub stages: Vec<(String, String, Mesh)>,
}

fn stage(st: &mut Vec<(String, String, Mesh)>, name: &str, key: &str, m: &Mesh) {
    st.push((name.to_string(), key.to_string(), m.clone()));
}

pub fn bowl_params(t: &Bathtub) -> bathfree::BowlParams {
    bathfree::BowlParams {
        width: t.width,
        size: t.size,
        depth: t.depth,
        thickness: t.thickness,
        taper: t.taper,
        has_curve: t.has_curve,
        disp_x: t.disp_x,
        disp_y: t.disp_y,
        alcove_levels: t.alcove_levels,
        levels: 5,
        side_levels: 2,
    }
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// polygon centres / normals as read_center / read_normal
fn centers_normals(m: &Mesh) -> (Vec<V3>, Vec<V3>) {
    (super::bpyx::face_centers(m), super::bpyx::face_normals(m))
}

/// SUBSURF (Catmull-Clark, limit surface) on a wire polyline: cubic B-spline refinement, then limit positions; ends fixed
fn subdivide_wire(pts: &[V3], levels: i64) -> Vec<V3> {
    let mut cur = pts.to_vec();
    for _ in 0..levels {
        let n = cur.len();
        let mut nx = vec![];
        for i in 0..n {
            let v = if i == 0 || i == n - 1 {
                cur[i]
            } else {
                let (a, b) = (cur[i - 1], cur[i + 1]);
                [(a[0] + 6.0 * cur[i][0] + b[0]) / 8.0, (a[1] + 6.0 * cur[i][1] + b[1]) / 8.0, (a[2] + 6.0 * cur[i][2] + b[2]) / 8.0]
            };
            nx.push(v);
            if i + 1 < n {
                let b = cur[i + 1];
                nx.push([(cur[i][0] + b[0]) / 2.0, (cur[i][1] + b[1]) / 2.0, (cur[i][2] + b[2]) / 2.0]);
            }
        }
        cur = nx;
    }
    if levels > 0 {
        let n = cur.len();
        let src = cur.clone();
        for i in 1..n - 1 {
            for k in 0..3 {
                cur[i][k] = (src[i - 1][k] + 4.0 * src[i][k] + src[i + 1][k]) / 6.0;
            }
        }
    }
    cur
}

pub fn make_legs(t: &Bathtub, obj: &Mesh, r: &mut Rng) -> Vec<Mesh> {
    let (co, nor) = centers_normals(obj);
    let zmin = co.iter().map(|c| c[2] as f32).fold(f32::INFINITY, f32::min);
    let leg_height = (zmin + t.leg_height as f32) as f64;
    let mut legs = vec![];
    for u in [1.0f64, -1.0] {
        for v in [1.0f64, -1.0] {
            let mut best = (f64::NEG_INFINITY, usize::MAX);
            for (i, c) in co.iter().enumerate() {
                if c[2] < leg_height {
                    let m = u * c[0] + v * c[1];
                    if m > best.0 {
                        best = (m, i);
                    }
                }
            }
            let i = best.1;
            let (pp, n) = (co[i], nor[i]);
            let q = [pp[0] + t.leg_side * n[0], pp[1] + t.leg_side * n[1] * t.leg_y_scale, pp[2] + t.leg_side * n[2]];
            let rr = [q[0], q[1], 0.0];
            let pts = subdivide_wire(&[pp, q, rr], t.leg_sub);
            let tube = super::gnhelpers::geo_radius_ex(&[pts], t.leg_radius, 32, 0.004, false);
            let bw = t.leg_radius * r.uniform(0.3, 0.7);
            if let Ok(d) = std::env::var("LEGDUMP") {
                std::fs::write(format!("{d}/legpre{}.json", legs.len()), serde_json::json!({"v": tube.pos, "p": tube.faces, "w": bw}).to_string()).unwrap();
            }
            let mut leg = bevel_def_seg(&tube, bw, 1);
            for p in leg.pos.iter_mut() {
                p[2] = (p[2] as f32 + t.leg_radius as f32) as f64;
            }
            legs.push(leg);
        }
    }
    legs
}

/// add_base: skirt below the plane z = min(z) + leg_height (outline = intersection of the bowl with that plane), extruded down, clipped
/// at z = 0, subdivided twice and solidified
pub fn add_base(t: &Bathtub, obj: &Mesh) -> Mesh {
    let zmin = obj.pos.iter().map(|p| p[2] as f32).fold(f32::INFINITY, f32::min);
    let h = (zmin + t.leg_height as f32) as f64;
    // outline of the surface cut by the plane z = h
    let mut cross: HashMap<(u32, u32), u32> = HashMap::default();
    let mut pts: Vec<V3> = vec![];
    let mut is_diag: Vec<bool> = vec![];
    let real_edges: crate::dhash::HashSet<(u32, u32)> = edge_list(&obj.faces).into_iter().collect();
    let mut edge_pt = |a: u32, b: u32, pts: &mut Vec<V3>| -> Option<u32> {
        let (pa, pb) = (obj.pos[a as usize], obj.pos[b as usize]);
        if (pa[2] - h) * (pb[2] - h) >= 0.0 {
            return None;
        }
        let k = ekey(a, b);
        Some(*cross.entry(k).or_insert_with(|| {
            let (p0, p1) = (obj.pos[k.0 as usize], obj.pos[k.1 as usize]);
            let f = ((h - p0[2]) / (p1[2] - p0[2])) as f32 as f64;
            pts.push([p0[0] + (p1[0] - p0[0]) * f, p0[1] + (p1[1] - p0[1]) * f, h]);
            is_diag.push(!real_edges.contains(&k));
            (pts.len() - 1) as u32
        }))
    };
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut order: Vec<u32> = vec![];
    for f in &obj.faces {
        // the exact boolean triangulates faces first (mesh_intersect.cc polyfill_triangulate_poly: quads split along 0-2 unless flipped)
        let tris: Vec<[u32; 3]> = if f.len() == 3 {
            vec![[f[0], f[1], f[2]]]
        } else if f.len() == 4 {
            let p = |i: usize| obj.pos[f[i] as usize];
            let (d12, d13, d14) = (sub3(p(1), p(0)), sub3(p(2), p(0)), sub3(p(3), p(0)));
            let flip = dot3(cross3(d12, d13), cross3(d14, d13)) > 0.0;
            if flip {
                vec![[f[0], f[1], f[3]], [f[1], f[2], f[3]]]
            } else {
                vec![[f[0], f[1], f[2]], [f[0], f[2], f[3]]]
            }
        } else {
            panic!("bathtub base: ngon in bowl")
        };
        for t in tris {
            let mut cr: Vec<u32> = vec![];
            for c in 0..3 {
                if let Some(i) = edge_pt(t[c], t[(c + 1) % 3], &mut pts) {
                    cr.push(i);
                }
            }
            if cr.len() == 2 {
                for (a, b) in [(cr[0], cr[1]), (cr[1], cr[0])] {
                    let e = adj.entry(a).or_insert_with(|| {
                        order.push(a);
                        vec![]
                    });
                    e.push(b);
                }
            }
        }
    }
    let loops = loops_from_adj(&adj, &order);
    assert_eq!(loops.len(), 1, "bathtub base outline loops");
    // find_dissolve_verts: triangulation-diagonal points that lie on a straight line (|cos - 1| < 1e-4) are dissolved
    let lp0 = &loops[0];
    let mut lp: Vec<u32> = vec![];
    for (k, &i) in lp0.iter().enumerate() {
        if is_diag[i as usize] {
            let (a, c) = (pts[lp0[(k + lp0.len() - 1) % lp0.len()] as usize], pts[lp0[(k + 1) % lp0.len()] as usize]);
            let b = pts[i as usize];
            let (v1, v2) = (sub3(b, a), sub3(c, b));
            let (l1, l2) = (dot3(v1, v1).sqrt(), dot3(v2, v2).sqrt());
            let cos = dot3(v1, v2) / (l1 * l2);
            if (cos - 1.0).abs() < 1e-4 {
                continue;
            }
        }
        lp.push(i);
    }
    let lp = &lp;
    let n = lp.len();
    let mut m = Mesh::new();
    for &i in lp {
        m.pos.push(pts[i as usize]);
    }
    for &i in lp {
        let p = pts[i as usize];
        m.pos.push([p[0], p[1], ((p[2] as f32) - t.depth as f32).max(0.0) as f64]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.faces.push(vec![i as u32, j as u32, (j + n) as u32, (i + n) as u32]);
    }
    // normals_make_consistent(inside=False): seed face = the one with the vertex farthest from the origin, pointing away from it
    orient_outward(&mut m);
    let m = super::subsurf::subsurf(&m, 2, false);
    bathfree::solidify(&m, t.thickness, -1.0)
}

/// bmesh recalc_face_normals(inside=False) for a connected quad mesh: flood-fill consistent winding, seed = face owning the vertex
/// farthest from the origin, oriented so its normal points away from the origin
pub fn orient_outward(m: &mut Mesh) {
    let mut best = (-1.0f64, 0usize);
    for (fi, f) in m.faces.iter().enumerate() {
        for &v in f {
            let p = m.pos[v as usize];
            let d = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
            if d > best.0 {
                best = (d, fi);
            }
        }
    }
    let nf = m.faces.len();
    let ef = edge_faces(&m.faces);
    let mut done = vec![false; nf];
    let seed = best.1;
    let sn = super::bpyops::face_normal(m, &m.faces[seed]);
    let far = m.faces[seed].iter().map(|&v| m.pos[v as usize]).fold((-1.0, [0.0; 3]), |a, p| {
        let d = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
        if d > a.0 {
            (d, p)
        } else {
            a
        }
    });
    if sn[0] * far.1[0] + sn[1] * far.1[1] + sn[2] * far.1[2] < 0.0 {
        m.faces[seed].reverse();
    }
    let mut stack = vec![seed];
    done[seed] = true;
    while let Some(f) = stack.pop() {
        let fv = m.faces[f].clone();
        for c in 0..fv.len() {
            let (a, b) = (fv[c], fv[(c + 1) % fv.len()]);
            for &g in &ef[&ekey(a, b)] {
                let g = g as usize;
                if done[g] {
                    continue;
                }
                // neighbour must traverse b -> a
                let gv = &m.faces[g];
                let mut same = false;
                for k in 0..gv.len() {
                    if gv[k] == a && gv[(k + 1) % gv.len()] == b {
                        same = true;
                    }
                }
                if same {
                    m.faces[g].reverse();
                }
                done[g] = true;
                stack.push(g);
            }
        }
    }
}

pub fn find_hole(obj: &Mesh, x: f64, y: f64) -> V3 {
    let (co, nor) = centers_normals(obj);
    let score: Vec<f64> = (0..co.len())
        .map(|i| {
            let up = if nor[i][2] > 0.0 { 1.0 } else { 0.0 };
            ((co[i][0] - x).abs() + (co[i][1] - y).abs()) - up
        })
        .collect();
    let m = score.iter().cloned().fold(f64::INFINITY, f64::min);
    // mirror-symmetric meshes tie in exact arithmetic and float noise decides in Blender; observed: the lower-y face wins
    let mut best = usize::MAX;
    for i in 0..co.len() {
        if score[i] <= m + 1e-6 && (best == usize::MAX || co[i][1] < co[best][1]) {
            best = i;
        }
    }
    co[best]
}

pub fn add_hole(t: &Bathtub, obj: &Mesh, r: &mut Rng, centered: bool) -> Mesh {
    let mut loc = if t.alcove {
        find_hole(obj, t.width / 2.0, t.size / 2.0)
    } else {
        find_hole(obj, r.uniform(0.35, 0.4) * t.width, t.size / 2.0)
    };
    if centered {
        loc = find_hole(obj, t.width / 2.0, t.size / 2.0);
    }
    let mut g = super::hwexact::bpy_cylinder();
    for p in g.v.iter_mut() {
        p[2] = (p[2] + 1.0) * 0.5;
    }
    from_g(&super::hwexact::apply_obj(&g, [t.hole_radius, t.hole_radius, 0.005], [0.0; 3], loc, true))
}

pub fn build(t: &Bathtub, r: &mut Rng) -> Built {
    let mut stages = vec![];
    let p = bowl_params(t);
    let bowl = bathfree::make_bowl(&p);
    stage(&mut stages, "bowl", "bowl", &bowl);
    let rt = bathfree::remove_top(&bowl, p.depth);
    stage(&mut stages, "removetop", "removetop", &rt);
    let u = r.uniform(0.5, 0.7);
    let st = bathfree::pipeline(&p, u);
    stage(&mut stages, "freestanding", "freestanding", &st.freestanding);
    let mut parts: Vec<Mesh> = vec![];
    if t.has_legs {
        let legs = make_legs(t, &st.freestanding, r);
        for (i, l) in legs.iter().enumerate() {
            stage(&mut stages, &format!("leg{i}"), &format!("leg{i}"), l);
        }
        parts.extend(legs);
    } else {
        let base = add_base(t, &st.freestanding);
        stage(&mut stages, "base", "base", &base);
        parts.push(base);
    }
    let mut all = vec![&st.side];
    all.extend(parts.iter());
    let joined = join(&all);
    let n_main = if t.has_legs { st.side.pos.len() } else { joined.pos.len() };
    stage(&mut stages, "prehole", "prehole", &joined);
    let hole = add_hole(t, &joined, r, false);
    stage(&mut stages, "hole", "hole", &hole);
    let body = super::subsurf::subsurf_bl(&joined, 1, false);
    let hole_s = super::subsurf::subsurf_bl(&hole, 1, false);
    let _ = n_main;
    Built { body, hole: hole_s, extra: vec![], stages }
}
