//! bathroom/toilet.py ToiletFactory.create_asset ported stage by stage (surface-equal to Blender; vertex order is not Blender's for
//! bridge / solidify stages). Stage dumps: ~/work/agents/storage/toistages.py, compared by src/bin/toicmp.rs.
use super::bathroom::Toilet;
use super::blops::align_bezier_points;
use super::bridge::{bridge_bm_ex, bridge_two_loops_surface, Interp};
use crate::mesh::Mesh;
use crate::rng::Rng;

type V3 = [f64; 3];

fn f32v(p: [f32; 3]) -> V3 {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

/// `apply_transform` of a uniform-scale + translation object (float32 matrix product)
fn xform(pts: &[V3], s: f64, loc: V3) -> Vec<V3> {
    let (s, l) = (s as f32, [loc[0] as f32, loc[1] as f32, loc[2] as f32]);
    pts.iter().map(|p| [((p[0] as f32) * s + l[0]) as f64, ((p[1] as f32) * s + l[1]) as f64, ((p[2] as f32) * s + l[2]) as f64]).collect()
}

pub struct Ring {
    pub pts: Vec<V3>,
    /// loop order (indices into pts)
    pub order: Vec<u32>,
}

/// build_curve(): align_bezier + MIRROR(x). Returns the 224-vertex ring in python's vertex order
pub fn build_curve(t: &Toilet) -> Ring {
    let mid = t.mid_offset();
    let anchors = [[0.0, -t.size_mid * t.size, 0.0], [t.width / 2.0, 0.0, 0.0], [0.0, mid, 0.0]];
    let axes = [Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0]), Some([1.0, 0.0, 0.0])];
    let half = align_bezier_points(&anchors, &axes, &t.curve_scales);
    let n = half.len();
    let mut pts: Vec<V3> = half.iter().map(|p| f32v(*p)).collect();
    for i in 1..n - 1 {
        pts.push([-pts[i][0], pts[i][1], pts[i][2]]);
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.extend((n as u32..pts.len() as u32).rev());
    Ring { pts, order }
}

pub struct Stages {
    pub upper: Vec<V3>,
    pub lower: Vec<V3>,
    pub bottom: Vec<V3>,
    pub bridged: Mesh,
    /// Blender edge list of `bridged` (BMesh order after bridge_edge_loops), the SOLIDIFY NON_MANIFOLD plane order depends on it
    pub bridged_edges: Vec<[u32; 2]>,
}

/// upper / lower / bottom rings and the bridged tube (before SOLIDIFY)
pub fn tube_bridge(t: &Toilet, r: &mut Rng) -> Stages {
    let ring = build_curve(t);
    let upper = ring.pts.clone();
    let lower = xform(&upper, t.tube_scale, [0.0, t.tube_scale * t.mid_offset() / 2.0, -t.depth]);
    let bottom = xform(&upper, t.stand_scale, [0.0, t.tube_scale * (1.0 - t.size_mid) * t.size / 2.0 * t.bottom_offset, -t.height]);
    let n = upper.len() as u32;
    let mut m = Mesh::new();
    m.pos = upper.iter().chain(lower.iter()).cloned().collect();
    let shape = r.uniform(0.1, 0.2);
    let la: Vec<u32> = ring.order.clone();
    let lb: Vec<u32> = ring.order.iter().map(|&i| i + n).collect();
    // the wire input of bpy.ops.mesh.bridge_edge_loops: two copies of the welded / mirrored ring, edges in the order the modifiers leave them
    // (half curve 0..n-1 sequential, then the mirrored points 0 - n - ... - (n_half - 1) closing at the last half point)
    let total = n as usize;
    let half = (total + 2) / 2;
    let mut edges: Vec<[u32; 2]> = vec![];
    for k in 0..2u32 {
        let o = k * n;
        for i in 0..half as u32 - 1 {
            edges.push([o + i, o + i + 1]);
        }
        edges.push([o, o + half as u32]);
        for i in half as u32..n - 1 {
            edges.push([o + i, o + i + 1]);
        }
        edges.push([o + n - 1, o + half as u32 - 1]);
    }
    let (bridged, bridged_edges) = match bridge_bm_ex(&m, &la, &lb, 64, shape, Interp::Surface, 1.0, "SMOOTH", Some(&edges)) {
        Ok(r) => r,
        Err(_) => {
            bridge_two_loops_surface(&mut m, la, lb, 64, shape);
            let e = super::bathkit::edge_list(&m.faces).into_iter().map(|(a, b)| [a, b]).collect();
            (m, e)
        }
    };
    Stages { upper, lower, bottom, bridged, bridged_edges }
}

/// SOLIDIFY(NON_MANIFOLD, thickness, offset=1, boundary FLAT) of the bridged surface
pub fn solidify_tube(t: &Toilet, bridged: &Mesh, edges: &[[u32; 2]]) -> Mesh {
    use super::solidnm::*;
    let pos: Vec<[f32; 3]> = bridged.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let out = solidify_nonmanifold_ex(&pos, &bridged.faces, Some(edges), t.thickness as f32, 1.0, Boundary::Flat).expect("solidify");
    let mut m = Mesh::new();
    m.pos = out.pos.iter().map(|p| f32v(*p)).collect();
    m.faces = out.faces;
    m
}

/// make_tube tail: extrude the upward-facing faces by thickness+extrude_height, then clip z to extrude_height
pub fn extrude_top(t: &Toilet, solid: &Mesh) -> Mesh {
    use super::bpyx::*;
    let mut m = solid.clone();
    let nor = face_normals(&m);
    let sel: Vec<bool> = nor.iter().map(|n| n[2] > 0.9).collect();
    extrude_region(&mut m, &sel, [0.0, 0.0, (t.thickness as f32 + t.extrude_height as f32) as f64]);
    let e = t.extrude_height as f32;
    for p in m.pos.iter_mut() {
        p[2] = (p[2] as f32).min(e) as f64;
    }
    m
}

// ---------------------------------------------------------------------------------------------------------------------
// remaining stages of ToiletFactory.create_asset
use super::bathkit::*;
use super::cabnodes::GMesh;
use super::hwexact;
use crate::dhash::HashMap;

pub struct Parts {
    pub tube: Mesh,
    pub seat: Mesh,
    pub cover: Mesh,
    pub stand: Mesh,
    pub back: Mesh,
    pub tank: Mesh,
    pub hw: Mesh,
}

fn f32c(p: V3) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// make_plane: duplicate the faces above 2/3 of the rim height, extrude the vertices behind the seat and flatten them
pub fn make_plane(t: &Toilet, tube: &Mesh) -> Mesh {
    let e = t.extrude_height as f32;
    let c = super::bpyx::face_centers(tube);
    let sel: Vec<bool> = c.iter().map(|p| (p[2] as f32) > e * 2.0 / 3.0).collect();
    let mut seat = separate_faces(tube, &sel);
    let lim = (t.mid_offset() + t.seat_thickness) as f32;
    let vsel: Vec<bool> = seat.pos.iter().map(|p| (p[1] as f32) > lim).collect();
    extrude_vsel(&mut seat, &vsel, [0.0, t.seat_size + t.thickness * 2.0, 0.0]);
    let ymax = (t.mid_offset() + t.seat_size) as f32;
    for p in seat.pos.iter_mut() {
        p[1] = (p[1] as f32).min(ymax) as f64;
    }
    compact(&seat)
}

pub fn make_seat(t: &Toilet, plane: &Mesh) -> (Mesh, Mesh) {
    let mut seat = super::bpyops::solidify(plane, t.extrude_height, 1.0);
    if t.has_seat_cut {
        // cutter cube (scale thickness) at (0, -thickness / 2 - size_mid * size, 0): BOOLEAN DIFFERENCE (BSP csg here: surface-equal)
        let cutter = cube_scaled([t.thickness; 3], [0.0, -t.thickness / 2.0 - t.size_mid * t.size, 0.0]);
        seat = boolean("DIFFERENCE", &seat, &cutter);
    }
    seat = bevel_def(&seat, 0.1, 2);
    // cover: fill the hole inside the seat
    let mut best = (f64::MAX, (0u32, 0u32));
    for (a, b) in edge_list(&plane.faces) {
        let (pa, pb) = (plane.pos[a as usize], plane.pos[b as usize]);
        let d = (((pa[0] + pb[0]) / 2.0) as f32).abs() as f64 + (((pa[1] + pb[1]) / 2.0) as f32).abs() as f64;
        if d < best.0 {
            best = (d, (a, b));
        }
    }
    let loops = boundary_loops(plane);
    let lp = loops.iter().find(|l| l.contains(&best.1 .0) && l.contains(&best.1 .1)).expect("seat hole loop");
    let pts: Vec<V3> = lp.iter().map(|&v| plane.pos[v as usize]).collect();
    let mut grid = super::bpyops::fill_grid_loop(&pts);
    if grid.faces.iter().map(|f| super::bpyops::face_normal(&grid, f)[2]).sum::<f64>() < 0.0 {
        for f in grid.faces.iter_mut() {
            f.reverse();
        }
    }
    let mut cover = plane.clone();
    let mut bykey: HashMap<[u64; 3], u32> = HashMap::default();
    for &v in lp {
        let p = plane.pos[v as usize];
        bykey.insert([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()], v);
    }
    let mut gmap = vec![0u32; grid.pos.len()];
    for (i, p) in grid.pos.iter().enumerate() {
        gmap[i] = match bykey.get(&[p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]) {
            Some(&v) => v,
            None => {
                cover.pos.push(*p);
                (cover.pos.len() - 1) as u32
            }
        };
    }
    for f in &grid.faces {
        cover.faces.push(f.iter().map(|&v| gmap[v as usize]).collect());
    }
    let mut cover = super::bpyops::solidify(&cover, t.extrude_height, 1.0);
    let (e, mo) = (t.extrude_height, t.mid_offset());
    for p in cover.pos.iter_mut() {
        *p = [p[0], (p[1] as f32 + (-mo - t.seat_size + e / 2.0) as f32) as f64, (p[2] as f32 + (-e / 2.0) as f32) as f64];
    }
    apply_scale_loc(&mut cover, [1.0; 3], t.cover_rotation, [0.0, mo + t.seat_size - e / 2.0, e * 1.5]);
    let cover = bevel_def(&cover, 0.1, 2);
    (seat, cover)
}

pub fn make_stand(t: &Toilet, tube: &Mesh, bottom: &[V3], r: &mut Rng, order: &[u32]) -> Mesh {
    let shape = r.uniform(0.0, 0.15);
    make_stand_shape(t, tube, bottom, shape, order)
}

pub fn make_stand_shape(t: &Toilet, tube: &Mesh, bottom: &[V3], shape: f64, order: &[u32]) -> Mesh {
    let mut best = (f64::MAX, (0u32, 0u32));
    for (a, b) in edge_list(&tube.faces) {
        let (pa, pb) = (f32c(tube.pos[a as usize]), f32c(tube.pos[b as usize]));
        let d = [pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let horizontal = if l > 0.0 { (d[2] / l).abs() < 0.1 } else { false };
        let cy = ((pa[1] + pb[1]) / 2.0) as f64;
        let cz = ((pa[2] + pb[2]) / 2.0) as f64;
        let under = cz < -t.stand_depth;
        let v = cy - (horizontal as i32 as f64) - (under as i32 as f64);
        if v < best.0 {
            best = (v, (a, b));
        }
    }
    let ring = edge_loop_quads(tube, best.1);
    let n = ring.len() as u32;
    assert_eq!(n as usize, bottom.len(), "stand ring size");
    let mut m = Mesh::new();
    m.pos = ring.iter().map(|&v| tube.pos[v as usize]).collect();
    m.pos.extend_from_slice(bottom);
    let la: Vec<u32> = (0..n).collect();
    let lb: Vec<u32> = order.iter().map(|&i| i + n).collect();
    super::bridge::bridge_two_loops(&mut m, la, lb, 64, shape);
    m
}

/// CORRECTIVE_SMOOTH defaults on a wire loop (no faces: pure 5x simple smoothing, lambda 0.5, float32)
fn smooth_loop(pts: &mut Vec<[f32; 3]>) {
    let n = pts.len();
    let div = 0.5f32 * (1.0f32 / 2.0f32);
    for _ in 0..5 {
        let mut d = vec![[0.0f32; 3]; n];
        for i in 0..n {
            let j = (i + 1) % n;
            let dir = [pts[j][0] - pts[i][0], pts[j][1] - pts[i][1], pts[j][2] - pts[i][2]];
            for k in 0..3 {
                d[i][k] += dir[k];
                d[j][k] -= dir[k];
            }
        }
        for i in 0..n {
            for k in 0..3 {
                pts[i][k] += d[i][k] * div;
            }
        }
    }
}

pub fn make_back(t: &Toilet, tube: &Mesh) -> Mesh {
    let mo = t.mid_offset();
    let c = super::bpyx::face_centers(tube);
    let nrm = super::bpyx::face_normals(tube);
    let sel: Vec<bool> = (0..tube.faces.len()).map(|i| (c[i][1] as f32) > (mo - t.back_thickness) as f32 && (nrm[i][1] as f32) > 0.1).collect();
    let loops = region_loops(tube, &sel);
    assert_eq!(loops.len(), 1, "back region loops");
    let lp = &loops[0];
    let mut pts: Vec<[f32; 3]> = lp.iter().map(|&v| f32c(tube.pos[v as usize])).collect();
    smooth_loop(&mut pts);
    let n = pts.len();
    let dy = (t.back_size + t.thickness * 2.0) as f32;
    let med = pts.iter().map(|p| p[0]).sum::<f32>() / n as f32;
    let ymax = (mo + t.back_size) as f32;
    let mut m = Mesh::new();
    for p in &pts {
        m.pos.push([p[0] as f64, (p[1] - 0.01) as f64, p[2] as f64]);
    }
    for p in &pts {
        let x = med + (p[0] - med) * t.back_scale as f32;
        m.pos.push([x as f64, ((p[1] + dy) - 0.01).min(ymax) as f64, p[2] as f64]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.faces.push(vec![i as u32, j as u32, (j + n) as u32, (i + n) as u32]);
    }
    m.faces.push((0..n as u32).map(|i| i + n as u32).collect());
    for p in m.pos.iter_mut() {
        p[1] = (p[1] as f32).min(ymax) as f64;
    }
    m
}

fn cube_scaled(scale: V3, loc: V3) -> Mesh {
    let g = hwexact::apply_obj(&hwexact::bpy_cube(), scale, [0.0; 3], loc, true);
    from_g(&g)
}

pub fn make_tank(t: &Toilet, r: &mut Rng) -> Mesh {
    let mo = t.mid_offset();
    let yc = mo + t.back_size - t.tank_size / 2.0;
    let tank = cube_scaled([t.tank_width / 2.0, t.tank_size / 2.0, t.tank_height / 2.0], [0.0, yc, t.tank_height / 2.0]);
    let tank = simple_subsurf(&tank, 2);
    let tank = bevel_def(&tank, 0.1, 2);
    let ce = t.tank_cap_extrude;
    let cap = cube_scaled([t.tank_width / 2.0 + ce, t.tank_size / 2.0 + ce, t.tank_cap_height / 2.0], [0.0, yc, t.tank_height]);
    let cap = bevel_def(&cap, r.uniform(0.0, t.extrude_height), 4);
    join(&[&tank, &cap])
}

fn new_cylinder() -> GMesh {
    let mut g = hwexact::bpy_cylinder();
    for p in g.v.iter_mut() {
        p[2] = (p[2] + 1.0) * 0.5;
    }
    g
}

pub fn make_hardware(t: &Toilet, r: &mut Rng) -> Mesh {
    let mo = t.mid_offset();
    if !t.handle {
        let g = hwexact::apply_obj(&new_cylinder(), [t.hw_radius, t.hw_radius, t.tank_cap_height / 2.0 + 1e-3], [0.0; 3], [0.0, mo + t.back_size - t.tank_size / 2.0, t.tank_height], true);
        return from_g(&g);
    }
    let hp = std::f64::consts::FRAC_PI_2;
    let a = hwexact::apply_obj(&new_cylinder(), [t.hw_radius, t.hw_radius, t.hw_cap], [hp, 0.0, 0.0], [0.0; 3], true);
    let lx = -t.hw_radius * r.uniform(0.0, 0.5);
    let lz = -t.hw_radius * r.uniform(0.0, 0.5);
    let b = hwexact::apply_obj(&new_cylinder(), [t.hw_radius / 2.0, t.hw_radius / 2.0, t.hw_length], [0.0, hp, 0.0], [lx, -t.hw_cap, lz], true);
    let j = hwexact::join_fwd(&[a, b]);
    let (loc, rot) = if t.hw_side {
        let x = -t.tank_width / 2.0 + t.hw_radius + r.uniform(0.01, 0.02);
        let z = t.tank_height - t.hw_radius - r.uniform(0.02, 0.03);
        ([x, mo + t.back_size - t.tank_size, z], [0.0, 0.0, 0.0])
    } else {
        let y = mo + t.back_size - t.tank_size + t.hw_radius + r.uniform(0.01, 0.02);
        let z = t.tank_height - t.hw_radius - r.uniform(0.02, 0.03);
        ([-t.tank_width / 2.0, y, z], [0.0, 0.0, -hp])
    };
    let g = hwexact::apply_obj(&j, [1.0; 3], rot, loc, true);
    bevel_def(&from_g(&g), r.uniform(0.005, 0.01), 2)
}

pub fn build(t: &Toilet, r: &mut Rng) -> Parts {
    let st = tube_bridge(t, r);
    let order: Vec<u32> = build_curve(t).order;
    let sol = solidify_tube(t, &st.bridged, &st.bridged_edges);
    let tube = extrude_top(t, &sol);
    let plane = make_plane(t, &tube);
    let (seat, cover) = make_seat(t, &plane);
    let stand = make_stand(t, &tube, &st.bottom, r, &order);
    let back = make_back(t, &tube);
    let tank = make_tank(t, r);
    let tube_b = bevel_def(&tube, 0.1, 2);
    let hw = make_hardware(t, r);
    Parts { tube: tube_b, seat, cover, stand, back, tank, hw }
}
