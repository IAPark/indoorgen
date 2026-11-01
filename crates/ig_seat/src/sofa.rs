//! seating/sofa.py: SofaFactory / ArmChairFactory (nodegroup_sofa_geometry + SUBSURF level 1 asset)
use super::curvekit::FloatCurve;
use super::geomkit::clip_gaussian;
use super::*;
use crate::materials::*;
use crate::subdiv::{catmull_clark, QMesh};
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "SofaFactory" | "ArmChairFactory")
}

const FAB: Weighted = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];

#[derive(Clone, Debug)]
pub struct SofaParams {
    pub dims: V3,
    pub arm: V3,
    pub back: V3,
    pub seat: V3,
    pub foot: V3,
    pub baseboard_h: f64,
    pub backrest_w: f64,
    pub seat_margin: f64,
    pub backrest_angle: f64,
    pub arm_type: usize,
    pub arm_width: f64,
    pub arm_height: f64,
    pub arms_angle: f64,
    pub footrest: bool,
    pub count: i64,
    pub scaling_footrest: f64,
    pub reflection: f64,
    pub leg_type: bool,
    pub leg_dimensions: f64,
    pub leg_z: f64,
    pub leg_faces: f64,
}

/// sofa_parameter_distribution (exact numpy draw order)
pub fn sofa_params(r: &mut Rng, dimensions: Option<V3>) -> SofaParams {
    let dims = match dimensions {
        Some(d) => d,
        None => {
            let a = r.uniform(0.95, 1.1);
            let b = clip_gaussian(r, 1.75, 0.75, 0.9, 3.0);
            let c = r.uniform(0.69, 0.97);
            [a, b, c]
        }
    };
    let arm = [r.uniform(1.0, 1.0), r.uniform(0.06, 0.15), r.uniform(0.5, 0.75)];
    let back = [r.uniform(0.15, 0.25), 0.0, r.uniform(0.5, 0.75)];
    let seat = [dims[0], r.uniform(0.7, 1.0), r.uniform(0.15, 0.3)];
    let foot = [r.uniform(0.07, 0.25), 0.06, 0.06];
    let baseboard_h = r.uniform(0.05, 0.09);
    let backrest_w = r.uniform(0.1, 0.2);
    let seat_margin = r.uniform(0.97, 1.0);
    let backrest_angle = r.uniform(-0.15, -0.5);
    let arm_type = r.choice_p(&[0.4, 0.2, 0.4]);
    let arm_width = r.uniform(0.6, 0.9);
    let arm_height = r.uniform(0.7, 1.0);
    let arms_angle = r.uniform(0.0, 1.08);
    let u = r.f64();
    let footrest = u > 0.5 && dims[1] > 2.0;
    let count = if r.f64() > 0.2 { 1 } else { 4 };
    let scaling_footrest = r.uniform(1.3, 1.6);
    let reflection = if r.f64() > 0.5 { 1.0 } else { -1.0 };
    let leg_type = r.f64() > 0.5;
    let leg_dimensions = r.uniform(0.4, 0.9);
    let leg_z = r.uniform(1.1, 2.5);
    let leg_faces = r.uniform(4.0, 25.0);
    SofaParams { dims, arm, back, seat, foot, baseboard_h, backrest_w, seat_margin, backrest_angle, arm_type, arm_width, arm_height, arms_angle, footrest, count, scaling_footrest, reflection, leg_type, leg_dimensions, leg_z, leg_faces }
}

// ------------------------------------------------------------------------------------------ mesh primitives
fn f32v(v: V3) -> V3 {
    [v[0] as f32 as f64, v[1] as f32 as f64, v[2] as f32 as f64]
}

/// GeometryNodeMeshCube with Blender's vertex / face order
pub fn mesh_cube(size: V3, vx: usize, vy: usize, vz: usize) -> Mesh {
    let (sx, sy, sz) = (size[0] as f32, size[1] as f32, size[2] as f32);
    let xs = |i: usize| -> f64 { (-sx / 2.0 + i as f32 * (sx / (vx - 1) as f32)) as f64 };
    let ys = |i: usize| -> f64 { (-sy / 2.0 + i as f32 * (sy / (vy - 1) as f32)) as f64 };
    let zs = |i: usize| -> f64 { (-sz / 2.0 + i as f32 * (sz / (vz - 1) as f32)) as f64 };
    let mut m = Mesh::new();
    // bottom layer
    for iy in 0..vy {
        for ix in 0..vx {
            m.pos.push([xs(ix), ys(iy), zs(0)]);
        }
    }
    // middle rings
    for iz in 1..vz - 1 {
        for ix in 0..vx {
            m.pos.push([xs(ix), ys(0), zs(iz)]);
        }
        for iy in 1..vy - 1 {
            m.pos.push([xs(0), ys(iy), zs(iz)]);
            m.pos.push([xs(vx - 1), ys(iy), zs(iz)]);
        }
        for ix in 0..vx {
            m.pos.push([xs(ix), ys(vy - 1), zs(iz)]);
        }
    }
    // top layer
    for iy in 0..vy {
        for ix in 0..vx {
            m.pos.push([xs(ix), ys(iy), zs(vz - 1)]);
        }
    }
    let ring = 2 * vx + 2 * (vy - 2);
    let top_off = vx * vy + (vz - 2) * ring;
    let v = |ix: usize, iy: usize, iz: usize| -> u32 {
        (if iz == 0 {
            iy * vx + ix
        } else if iz == vz - 1 {
            top_off + iy * vx + ix
        } else {
            let base = vx * vy + (iz - 1) * ring;
            if iy == 0 {
                base + ix
            } else if iy == vy - 1 {
                base + vx + 2 * (vy - 2) + ix
            } else {
                base + vx + 2 * (iy - 1) + if ix == vx - 1 { 1 } else { 0 }
            }
        }) as u32
    };
    for iy in 0..vy - 1 {
        for ix in 0..vx - 1 {
            m.faces.push(vec![v(ix, iy, 0), v(ix, iy + 1, 0), v(ix + 1, iy + 1, 0), v(ix + 1, iy, 0)]);
        }
    }
    for iz in 0..vz - 1 {
        for ix in 0..vx - 1 {
            m.faces.push(vec![v(ix, 0, iz), v(ix + 1, 0, iz), v(ix + 1, 0, iz + 1), v(ix, 0, iz + 1)]);
        }
    }
    let t = vz - 1;
    for iy in 0..vy - 1 {
        for ix in 0..vx - 1 {
            m.faces.push(vec![v(ix, iy, t), v(ix + 1, iy, t), v(ix + 1, iy + 1, t), v(ix, iy + 1, t)]);
        }
    }
    for iz in 0..vz - 1 {
        for ix in 0..vx - 1 {
            m.faces.push(vec![v(ix, vy - 1, iz), v(ix, vy - 1, iz + 1), v(ix + 1, vy - 1, iz + 1), v(ix + 1, vy - 1, iz)]);
        }
    }
    for iz in 0..vz - 1 {
        for iy in 0..vy - 1 {
            m.faces.push(vec![v(0, iy, iz), v(0, iy, iz + 1), v(0, iy + 1, iz + 1), v(0, iy + 1, iz)]);
        }
    }
    for iz in 0..vz - 1 {
        for iy in 0..vy - 1 {
            m.faces.push(vec![v(vx - 1, iy, iz), v(vx - 1, iy + 1, iz), v(vx - 1, iy + 1, iz + 1), v(vx - 1, iy, iz + 1)]);
        }
    }
    m
}

/// nodegroup_corner_cube
pub fn corner_cube(loc: V3, centering: V3, dims: V3, vx: usize, vy: usize, vz: usize) -> Mesh {
    let mut m = mesh_cube(dims, vx, vy, vz);
    // Map Range (vector, clamped): centering is limited to [0, 1] before mapping to [0.5, -0.5]
    let c = [centering[0].clamp(0.0, 1.0), centering[1].clamp(0.0, 1.0), centering[2].clamp(0.0, 1.0)];
    let t = [(0.5 - c[0]) * dims[0] + loc[0], (0.5 - c[1]) * dims[1] + loc[1], (0.5 - c[2]) * dims[2] + loc[2]];
    for p in m.pos.iter_mut() {
        *p = [p[0] + t[0], p[1] + t[1], p[2] + t[2]];
    }
    m
}

/// GeometryNodeMeshGrid: vertices x-major
pub fn mesh_grid(sx: f64, sy: f64, vx: usize, vy: usize) -> Mesh {
    let mut m = Mesh::new();
    for ix in 0..vx {
        for iy in 0..vy {
            let x = if vx > 1 { -sx / 2.0 + sx * ix as f64 / (vx - 1) as f64 } else { 0.0 };
            let y = if vy > 1 { -sy / 2.0 + sy * iy as f64 / (vy - 1) as f64 } else { 0.0 };
            m.pos.push([x, y, 0.0]);
        }
    }
    for ix in 0..vx.saturating_sub(1) {
        for iy in 0..vy.saturating_sub(1) {
            let a = (ix * vy + iy) as u32;
            m.faces.push(vec![a, a + vy as u32, a + vy as u32 + 1, a + 1]);
        }
    }
    m
}

/// GeometryNodeMeshCone, n-gon fill: z from `depth` (top ring, radius_top) down to 0
pub fn mesh_cone(n: usize, side: usize, rtop: f64, rbot: f64, depth: f64) -> Mesh {
    let mut m = Mesh::new();
    for r in 0..=side {
        let t = r as f32 / side as f32;
        let rad = rtop as f32 + (rbot as f32 - rtop as f32) * t;
        let z = depth as f32 * (1.0 - t);
        for i in 0..n {
            let a = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
            m.pos.push([(rad * a.cos()) as f64, (rad * a.sin()) as f64, z as f64]);
        }
    }
    let nn = n as u32;
    m.faces.push((0..nn).collect());
    for r in 0..side as u32 {
        for i in 0..nn {
            let i1 = (i + 1) % nn;
            m.faces.push(vec![r * nn + i, (r + 1) * nn + i, (r + 1) * nn + i1, r * nn + i1]);
        }
    }
    m.faces.push((0..nn).rev().map(|i| side as u32 * nn + i).collect());
    m
}

/// GeometryNodeMeshCylinder, triangle-fan caps
pub fn mesh_cylinder_fan(n: usize, side: usize, radius: f64, depth: f64) -> Mesh {
    let mut m = Mesh::new();
    m.pos.push([0.0, 0.0, depth / 2.0]);
    for r in 0..=side {
        let z = (depth as f32 / 2.0 - depth as f32 * r as f32 / side as f32) as f64;
        for i in 0..n {
            let a = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
            m.pos.push([(radius as f32 * a.cos()) as f64, (radius as f32 * a.sin()) as f64, z]);
        }
    }
    m.pos.push([0.0, 0.0, -depth / 2.0]);
    let nn = n as u32;
    for i in 0..nn {
        m.faces.push(vec![1 + i, 1 + (i + 1) % nn, 0]);
    }
    for r in 0..side as u32 {
        for i in 0..nn {
            let i1 = (i + 1) % nn;
            m.faces.push(vec![1 + r * nn + i, 1 + (r + 1) * nn + i, 1 + (r + 1) * nn + i1, 1 + r * nn + i1]);
        }
    }
    let last = 1 + side as u32 * nn;
    let c = last + nn;
    for i in 0..nn {
        m.faces.push(vec![last + i, c, last + (i + 1) % nn]);
    }
    m
}

// ------------------------------------------------------------------------------------------ mesh operations
/// Transform Geometry (components): scale, then euler XYZ rotation, then translation. Face winding is left alone.
pub fn tf(m: &Mesh, t: V3, rot: V3, s: V3) -> Mesh {
    let r = Mat4::euler_xyz(rot);
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        let q = r.apply([p[0] * s[0], p[1] * s[1], p[2] * s[2]]);
        *p = [q[0] + t[0], q[1] + t[1], q[2] + t[2]];
    }
    o
}

pub fn join(parts: &[&Mesh]) -> Mesh {
    let mut o = Mesh::new();
    for m in parts {
        o.append(m);
    }
    o
}

pub fn flip_faces(m: &Mesh) -> Mesh {
    m.clone().flipped()
}

fn face_normal(m: &Mesh, f: &[u32]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l == 0.0 { n } else { [n[0] / l, n[1] / l, n[2] / l] }
}

/// ExtrudeMesh (faces, individual): returns the mesh and the indices of the new top faces
pub fn extrude_individual(m: &Mesh, offset: f64) -> (Mesh, Vec<usize>) {
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    let mut tops = vec![];
    for f in &m.faces {
        let n = face_normal(m, f);
        let base = o.pos.len() as u32;
        for &i in f {
            let p = m.pos[i as usize];
            o.pos.push([p[0] + n[0] * offset, p[1] + n[1] * offset, p[2] + n[2] * offset]);
        }
        let k = f.len();
        for i in 0..k {
            let j = (i + 1) % k;
            o.faces.push(vec![f[i], f[j], base + j as u32, base + i as u32]);
        }
        tops.push(o.faces.len());
        o.faces.push((0..k as u32).map(|i| base + i).collect());
    }
    (o, tops)
}

/// ScaleElements (face domain, uniform) about each face's own center
pub fn scale_faces(m: &Mesh, faces: &[usize], s: f64) -> Mesh {
    let mut o = m.clone();
    for &fi in faces {
        let f = &m.faces[fi];
        let mut c = [0.0; 3];
        for &i in f {
            for k in 0..3 {
                c[k] += m.pos[i as usize][k] / f.len() as f64;
            }
        }
        for &i in f {
            let p = m.pos[i as usize];
            o.pos[i as usize] = [c[0] + (p[0] - c[0]) * s, c[1] + (p[1] - c[1]) * s, c[2] + (p[2] - c[2]) * s];
        }
    }
    o
}

/// SubdivideMesh: simple (linear) subdivision, `level` times
pub fn simple_subdivide(m: &Mesh, level: usize) -> Mesh {
    let mut cur = m.clone();
    for _ in 0..level {
        let mut o = Mesh::new();
        o.pos = cur.pos.clone();
        let mut emap: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
        let mut mid = |o: &mut Mesh, a: u32, b: u32| -> u32 {
            let key = if a < b { (a, b) } else { (b, a) };
            if let Some(&i) = emap.get(&key) {
                return i;
            }
            let (p, q) = (cur.pos[a as usize], cur.pos[b as usize]);
            o.pos.push([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]);
            let i = (o.pos.len() - 1) as u32;
            emap.insert(key, i);
            i
        };
        for f in &cur.faces {
            let k = f.len();
            let mids: Vec<u32> = (0..k).map(|i| mid(&mut o, f[i], f[(i + 1) % k])).collect();
            let mut c = [0.0; 3];
            for &i in f {
                for d in 0..3 {
                    c[d] += cur.pos[i as usize][d] / k as f64;
                }
            }
            o.pos.push(c);
            let ci = (o.pos.len() - 1) as u32;
            for i in 0..k {
                o.faces.push(vec![f[i], mids[i], ci, mids[(i + k - 1) % k]]);
            }
        }
        cur = o;
    }
    cur
}

/// SubdivisionSurface node / SUBSURF modifier (Catmull-Clark)
pub fn subsurf(m: &Mesh, level: usize) -> Mesh {
    let q = QMesh { pos: m.pos.clone(), faces: m.faces.clone() };
    let r = catmull_clark(&q, level, None);
    Mesh { pos: r.pos, faces: r.faces, uv: vec![] }
}

/// MeshLine (end points) + InstanceOnPoints + Realize
fn array_fill_line(start: V3, end: V3, dims: V3, count: usize, inst: &Mesh) -> Mesh {
    let mult = [dims[0] * 0.0, dims[1] * -0.5, dims[2] * 0.0];
    let a = [end[0] + mult[0], end[1] + mult[1], end[2] + mult[2]];
    let b = [start[0] - mult[0], start[1] - mult[1], start[2] - mult[2]];
    let mut pts = vec![];
    for i in 0..count {
        let t = if count > 1 { i as f64 / (count - 1) as f64 } else { 0.0 };
        pts.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
    }
    instance_on_points(&pts, inst, [1.0; 3])
}

fn instance_on_points(pts: &[V3], inst: &Mesh, scale: V3) -> Mesh {
    let mut o = Mesh::new();
    for p in pts {
        o.append(&tf(inst, *p, [0.0; 3], scale));
    }
    o
}

fn mix(v: f64, c: f64, fac: f64) -> f64 {
    (1.0 - fac) * v + fac * c
}

fn map_range_clamped(v: f64, fmin: f64, fmax: f64, tmin: f64, tmax: f64) -> f64 {
    let f = if fmax != fmin { (v - fmin) / (fmax - fmin) } else { 0.0 };
    let r = tmin + f * (tmax - tmin);
    if tmin > tmax { r.clamp(tmax, tmin) } else { r.clamp(tmin, tmax) }
}

/// nodegroup_sofa_geometry (Subdivide = True: ends with SubdivisionSurface level 1)
pub fn sofa_geometry(p: &SofaParams) -> Mesh {
    let d = p.dims;
    let a = p.arm;
    let b = p.back;
    let s = p.seat;
    let f = p.foot;
    let count = p.count;
    // base board dims
    let cb = [d[0] - b[0], d[1] - 2.0 * a[1], p.baseboard_h];
    let n = ((cb[1] as f32 / s[1] as f32).ceil()) as f64;
    let ni = n as usize;
    let cdy = cb[1] / n;
    let cd = [s[0], cdy, s[2]];
    let vm017 = [0.0, -cb[1] * 0.5, cb[2]];
    let vm018 = [0.0, cb[1] * 0.5, cb[2]];
    let c018 = [p.backrest_w, 0.0, s[2]];
    let vm022 = [vm017[0] + c018[0], vm017[1] + c018[1], vm017[2] + c018[2]];
    let vm023 = [vm018[0] + c018[0], vm018[1] + c018[1], vm018[2] + c018[2]];
    // back cushions
    let cush_dims = [d[2] - s[2] - p.baseboard_h, cdy, p.backrest_w];
    let cc = corner_cube([0.0; 3], [0.1, 0.5, 1.0], cush_dims, 2, 2, 2);
    let (ex, tops) = extrude_individual(&cc, 0.03);
    let sc = scale_faces(&ex, &tops, 0.6);
    let cush = subsurf(&sc, 1);
    let bt = [-p.backrest_w + (b[0] + 0.1), 0.0, 0.0];
    let cush_t = tf(&cush, bt, [0.0, p.backrest_angle - 1.5708, 0.0], [p.seat_margin, p.seat_margin, 1.0]);
    let group003 = array_fill_line(vm022, vm023, cd, ni, &cush_t);
    // seat cushions
    let seat_c = corner_cube([0.0; 3], [0.0, 0.5, 0.0], [cd[0], cd[1] * 1.03, cd[2]], 2, 2, 2);
    let seat_t = tf(&seat_c, [0.0; 3], [0.0; 3], [p.seat_margin, p.seat_margin, 1.0]);
    let group002 = array_fill_line(vm017, vm018, cd, ni, &seat_t);
    // footrest pieces
    let sw007 = if count == 4 { 1.0 } else { p.reflection };
    let c012 = [1.0, sw007, 1.1];
    let vm019 = [vm018[0] * c012[0], vm018[1] * c012[1], vm018[2] * c012[2]];
    let vm020 = [cd[0] * c012[0], cd[1] * c012[1], cd[2] * c012[2]];
    let t011 = tf(&seat_t, [0.0; 3], [0.0; 3], [p.scaling_footrest, 1.0, 1.1]);
    let group_a = array_fill_line(vm017, vm019, vm020, count as usize, &t011);
    let t012 = tf(&t011, [0.0; 3], [0.0; 3], [1.0, n, 1.0]);
    let group001 = array_fill_line([0.0; 3], [0.0, 0.0, vm019[2]], [0.0; 3], 1, &t012);
    let sw008 = if count == 4 { group001 } else { group_a };
    let sw009 = if p.footrest { sw008 } else { Mesh::new() };
    let part = std::env::var("SOFA_PART").unwrap_or_default();
    if part == "back" { return group003; }
    if part == "seatc" { return group002; }
    // Join.007 = [Group.002, Switch.009] -> reversed
    let join007 = join(&[&group002, &sw009]);
    let sub_a = simple_subdivide(&join007, 2);
    // Join.008 = [Group.003, Subdivide] -> reversed
    let join008 = join(&[&group003, &sub_a]);
    if part == "join008" { return join008; }
    // legs
    let grid1 = mesh_grid(1.0, 1.0, 2, 2);
    let gscale = [d[0] * 1.0 - f[0] * 2.5, d[1] * 1.0 - f[1] * 2.5, 0.0];
    let pts_a: Vec<V3> = grid1.pos.iter().map(|q| [q[0] * gscale[0] + d[0] * 0.5, q[1] * gscale[1], q[2] * gscale[2]]).collect();
    let lf = p.leg_faces as usize;
    let cone = mesh_cone(lf, 4, 0.01, 0.025, 0.07);
    let leg_cone = tf(&cone, [0.0, 0.0, 0.01], [0.0, 3.1416, 0.0], [p.leg_dimensions, p.leg_dimensions, p.leg_z]);
    let foot_cube = corner_cube([0.0; 3], [0.5, 0.5, 0.9], f, 4, 4, 4);
    let leg_cube = tf(&foot_cube, [0.0; 3], [0.0; 3], [0.5, 0.8, 0.8]);
    let leg = if p.leg_type { leg_cube } else { leg_cone };
    let realize001 = instance_on_points(&pts_a, &leg, [1.0; 3]);
    if part == "legs" { return realize001; }
    // back legs
    let c005 = [1.0, n, 1.0];
    let vm005 = [cb[0] / c005[0], cb[1] / c005[1], cb[2] / c005[2]];
    let sw002 = if count == 4 { cb } else { vm005 };
    let grid = mesh_grid(1.0, sw002[1] * 0.7, 1, 2);
    let vm008 = [sw002[0] - 0.1 + b[0], 0.0, 0.0];
    let pts_b: Vec<V3> = grid.pos.iter().map(|q| [q[0] + vm008[0], q[1] + vm008[1], q[2] * 0.9 + vm008[2]]).collect();
    let realize = instance_on_points(&pts_b, &leg, [1.0, 1.0, 1.2]);
    let c003 = [b[0], d[1] - 2.0 * a[1], b[2]];
    let vm002 = [c003[0], 0.0, 0.0];
    if part == "backlegs" { return realize; }
    let bb001 = corner_cube(vm002, [0.0, 0.5, -1.0], vm005, 2, 2, 2);
    let join003 = join(&[&realize, &bb001]);
    let vm011 = [0.0, vm005[1] * (n - 1.0) * 0.5, 0.0];
    let vm012 = [vm011[0], vm011[1] * p.reflection, vm011[2]];
    let t006 = tf(&join003, vm012, [0.0; 3], [p.scaling_footrest, 1.0, 1.0]);
    let sw004 = if p.footrest { Mesh::new() } else { t006 };
    let bb002 = corner_cube(vm002, [0.0, 0.5, -1.0], cb, 3, 3, 3);
    let t008 = tf(&bb002, [0.0; 3], [0.0; 3], [p.scaling_footrest, 1.0, 1.0]);
    let t007 = tf(&realize, [0.0; 3], [0.0; 3], [p.scaling_footrest, 1.3, 1.0]);
    let join004 = join(&[&t008, &t007]);
    let sw005 = if p.footrest { join004 } else { Mesh::new() };
    let sw006 = if count == 4 { sw005 } else { sw004 };
    let base_board = corner_cube(vm002, [0.0, 0.5, -1.0], cb, 2, 2, 2);
    let back_board = corner_cube([0.0; 3], [0.0, 0.5, -1.0], c003, 2, 2, 2);
    // arms
    let a_sq = [a[0], a[1], a[2] - a[1]];
    let arm_loc = [0.0, d[1] * 0.5, 0.0];
    let arm_cube001 = corner_cube(arm_loc, [0.0, 1.0, 0.0], a_sq, 4, 4, 4);
    let cyl_depth = a_sq[0] * 1.0001;
    let cyl = mesh_cylinder_fan(32, 4, a_sq[1], cyl_depth);
    let cyl_t = tf(&cyl, [cyl_depth / 2.0, arm_loc[1], a_sq[2]], [0.0, 1.5708, 0.0], [1.0; 3]);
    let round_top = join(&[&arm_cube001, &cyl_t]);
    let sw_arm = if p.arm_type == 0 { arm_cube001.clone() } else { round_top };
    let sw001 = if p.arm_type == 2 {
        let mut ac = corner_cube(arm_loc, [0.0, 1.0, 0.0], a, 4, 4, 10);
        let c1 = FloatCurve::new(&[(0.0092, 0.7688), (0.1011, 0.5937), (0.1494, 0.4062), (0.3954, 0.0781), (1.0, 0.2187)]);
        let c2 = FloatCurve::new(&[(0.1341, 0.2094), (0.7386, 1.0), (0.9682, 0.0781), (1.0, 0.0)]);
        let (sa, ca) = p.arms_angle.sin_cos();
        for q in ac.pos.iter_mut() {
            let u = map_range_clamped(q[2], -0.1, a[2], -0.1, 0.2);
            let w1 = mix(u, c1.eval_node(u), p.arm_width);
            let m1 = w1 * (q[1] - arm_loc[1]);
            let v = map_range_clamped(q[0], -1.0, 0.6, 2.1, -1.1);
            let h1 = mix(v, c2.eval_node(v), p.arm_height);
            let m3 = h1 * (q[2] - 0.0);
            let off = [0.0, m1 * ca - m3 * sa, m1 * sa + m3 * ca];
            *q = [q[0] + off[0], q[1] + off[1], q[2] + off[2]];
        }
        ac
    } else {
        sw_arm
    };
    let mirrored = tf(&sw001, [0.0; 3], [0.0; 3], [1.0, -1.0, 1.0]);
    let flipped = flip_faces(&mirrored);
    if part == "arm" { return sw001; }
    let join001 = join(&[&sw001, &flipped]);
    let join002 = join(&[&back_board, &join001]);
    let join005 = join(&[&sw006, &base_board, &join002]);
    let join006 = join(&[&realize001, &join005]);
    if part == "join006" { return join006; }
    if part == "join005" { return join005; }
    if part == "join002" { return join002; }
    if part == "join001" { return join001; }
    if part == "bb" { return base_board; }
    if part == "backb" { return back_board; }
    if part == "sw006" { return sw006; }
    let sub_b = simple_subdivide(&join006, 2);
    let join010 = join(&[&join008, &realize001, &sub_b]);
    if part == "join010" { return join010; }
    subsurf(&join010, 1)
}

/// positions of an all-quad mesh moved to the Catmull-Clark limit surface (SUBSURF modifier with use_limit_surface at its last level)
pub fn limit_surface(m: &Mesh) -> Mesh {
    let nv = m.pos.len();
    let mut inc: Vec<Vec<usize>> = vec![vec![]; nv];
    for (fi, f) in m.faces.iter().enumerate() {
        for &v in f {
            inc[v as usize].push(fi);
        }
    }
    let mut o = m.clone();
    for v in 0..nv {
        let faces = &inc[v];
        if faces.is_empty() {
            continue;
        }
        // all incident faces must be quads and form a closed fan
        let mut edge_nb: Vec<u32> = vec![];
        let mut diag: Vec<u32> = vec![];
        let mut ok = true;
        for &fi in faces {
            let f = &m.faces[fi];
            if f.len() != 4 {
                ok = false;
                break;
            }
            let k = f.iter().position(|&x| x as usize == v).unwrap();
            let (prev, next, opp) = (f[(k + 3) % 4], f[(k + 1) % 4], f[(k + 2) % 4]);
            for e in [prev, next] {
                if !edge_nb.contains(&e) {
                    edge_nb.push(e);
                }
            }
            diag.push(opp);
        }
        let n = faces.len();
        if !ok || edge_nb.len() != n || n < 3 {
            continue;
        }
        let nf = n as f64;
        let mut acc = [0.0; 3];
        let pv = m.pos[v];
        for k in 0..3 {
            acc[k] = nf * nf * pv[k];
        }
        for &e in &edge_nb {
            for k in 0..3 {
                acc[k] += 4.0 * m.pos[e as usize][k];
            }
        }
        for &d in &diag {
            for k in 0..3 {
                acc[k] += m.pos[d as usize][k];
            }
        }
        let den = nf * nf + 5.0 * nf;
        o.pos[v] = [acc[0] / den, acc[1] / den, acc[2] / den];
    }
    o
}

// ------------------------------------------------------------------------------------------ geometry-nodes route (exact order)
/// nodegroup_sofa_geometry evaluated by the node interpreter (data/gn/sofa_group.json, exported with tools/gn_export.py group):
/// Blender's vertex / face / edge order. Returns the mesh and its edge list.
pub fn sofa_geometry_gn(p: &SofaParams) -> Option<(Mesh, Vec<[u32; 2]>)> {
    sofa_geometry_gn_lvl(p, None)
}

/// `cap`: limit every subdivision level (outline-only evaluation for solver placeholders)
pub fn sofa_geometry_gn_lvl(p: &SofaParams, cap: Option<i64>) -> Option<(Mesh, Vec<[u32; 2]>)> {
    use crate::gnasset::GnTemplate;
    use serde_json::Value as J;
    static TPL: std::sync::OnceLock<GnTemplate> = std::sync::OnceLock::new();
    let tpl = TPL.get_or_init(|| GnTemplate::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/sofa_group.json"))));
    let mut t = tpl.clone_template();
    if let Some(c) = cap {
        t.cap_subdivision(c);
    }
    let g = "nodegroup_sofa_geometry";
    let v3 = |v: V3| json!([v[0], v[1], v[2]]);
    t.set_group_default(g, "Socket_1", v3(p.dims));
    t.set_group_default(g, "Socket_2", v3(p.arm));
    t.set_group_default(g, "Socket_3", v3(p.back));
    t.set_group_default(g, "Socket_4", v3(p.seat));
    t.set_group_default(g, "Socket_5", v3(p.foot));
    t.set_group_default(g, "Socket_6", json!(p.baseboard_h));
    t.set_group_default(g, "Socket_7", json!(p.backrest_w));
    t.set_group_default(g, "Socket_8", json!(p.seat_margin));
    t.set_group_default(g, "Socket_9", json!(p.backrest_angle));
    t.set_group_default(g, "Socket_10", json!(p.arm_width));
    t.set_group_default(g, "Socket_11", J::from(p.arm_type as i64));
    t.set_group_default(g, "Socket_12", json!(p.arm_height));
    t.set_group_default(g, "Socket_13", json!(p.arms_angle));
    t.set_group_default(g, "Socket_14", json!(p.footrest));
    t.set_group_default(g, "Socket_15", J::from(p.count));
    t.set_group_default(g, "Socket_16", json!(p.scaling_footrest));
    t.set_group_default(g, "Socket_17", J::from(p.reflection as i64));
    t.set_group_default(g, "Socket_18", json!(p.leg_type));
    t.set_group_default(g, "Socket_19", json!(p.leg_dimensions));
    t.set_group_default(g, "Socket_20", json!(p.leg_z));
    t.set_group_default(g, "Socket_21", J::from(p.leg_faces as i64));
    let outs = t.run_group(g).ok()?;
    let geo = &outs.iter().find(|(n, _)| n == "Geometry")?.1;
    let m = geo.mesh.as_ref()?;
    let mut o = Mesh::new();
    o.pos = m.pos.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    o.faces = m.faces_vec();
    Some((o, m.edges.clone()))
}

// ------------------------------------------------------------------------------------------ factory
pub struct Sofa {
    params: SofaParams,
    /// the node group's outline (its un-subdivided cage: the subdivided sofa lies within it), measured when the sofa is made
    outline: Option<(V3, V3)>,
    seed: u64,
}

impl Sofa {
    pub fn new(rng: &mut Rng, armchair: bool) -> Sofa {
        let mut r = rng.clone();
        let params = if armchair {
            let dims = [r.uniform(0.8, 1.0), r.uniform(0.9, 1.1), r.uniform(0.69, 0.97)];
            sofa_params(&mut r, Some(dims))
        } else {
            sofa_params(&mut r, None)
        };
        let mut params = params;
        // built to the layout box [width (y), depth (x), height]: the node group's dims are [depth, width, height]; the overall extents
        // (footrest pieces, legs below the frame, subdivision) are measured on the node group's outline and the dims corrected
        if let Some(h) = crate::api::size_hint() {
            params.dims = [h[1], h[0], h[2]];
            params.footrest = params.footrest && h[0] > 2.0 && h[1] >= 0.8 * params.scaling_footrest;
            for _ in 0..4 {
                params.seat[0] = params.dims[0];
                params.arm[2] = params.arm[2].min(0.8 * params.dims[2]);
                params.back[2] = params.back[2].min(0.8 * params.dims[2]);
                params.arm[0] = params.dims[0]; // the arms run the full depth (python draws exactly 1.0 for dims 0.95-1.1)
                let Some((lo, hi)) = sofa_geometry_gn_lvl(&params, Some(0)).map(|(m, _)| m.bounds()) else { break };
                let e = [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]];
                if (0..3).all(|k| (e[k] - h[k]).abs() < 0.003) { break }
                params.dims[0] *= h[1] / e[1];
                params.dims[1] *= h[0] / e[0];
                params.dims[2] += h[2] - e[2];
            }
            params.seat[0] = params.dims[0];
            params.arm[0] = params.dims[0];
        }
        let outline = sofa_geometry_gn_lvl(&params, Some(0)).map(|(m, _)| m.bounds());
        Sofa { params, outline, seed: r.randint(0, 1_000_000) as u64 }
    }
}

impl Factory for Sofa {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        let p = &self.params;
        let (lo, hi) = sofa_geometry_gn_lvl(p, Some(0))?.0.bounds();
        Some([hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]])
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        // create_placeholder(): the node group output (PlaceholderBBox uses its bounding box)
        let mut r = Rng::seeded(self.seed);
        let fab = spec_material("sofa_fabric", json!({"cls": weighted(&mut r, FAB), "kwargs": {}, "seed": self.seed}), [0.4, 0.4, 0.45]);
        // dense mode: only the outline matters (PlaceholderBBox); skip the subdivision of the cushion cubes (cage bbox, a few mm larger)
        let cap = if crate::dense() { Some(0) } else { None };
        let place = match sofa_geometry_gn_lvl(&self.params, cap) {
            Some((m, _)) if std::env::var("SOFA_OLD").is_err() => m,
            _ => sofa_geometry(&self.params),
        };
        let mut a = Asset::default();
        let b = place.bounds();
        a.mesh(place, &fab, Some(b));
        Some(a)
    }
    /// dense mode: outline of the un-subdivided node group (within ~1 cm of the final asset)
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        if !crate::dense() {
            return None;
        }
        sofa_geometry_gn_lvl(&self.params, Some(0)).map(|(m, _)| m.bounds())
    }
    /// collision: the outline measured when it was made, one box
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        let (lo, hi) = self.outline?;
        Some(vec![crate::piece::Volume::hard([lo[0] - 0.002, lo[1] - 0.002, lo[2] - 0.002], [hi[0] + 0.002, hi[1] + 0.002, hi[2] + 0.002])])
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let fab = spec_material("sofa_fabric", json!({"cls": weighted(&mut r, FAB), "kwargs": {}, "seed": self.seed}), [0.4, 0.4, 0.45]);
        let gn = if std::env::var("SOFA_OLD").is_err() { sofa_geometry_gn(&self.params) } else { None };
        let hipoly = match gn {
            Some((place, edges)) => crate::subsurf::subsurf_osd(&place, 1, false, Some(&edges)).unwrap_or_else(|| limit_surface(&subsurf(&place, 1))),
            None => limit_surface(&subsurf(&sofa_geometry(&self.params), 1)),
        };
        let mut a = Asset::default();
        let b = hipoly.bounds();
        a.mesh(hipoly, &fab, Some(b));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    // 60 % of sofas / armchairs are the part-built ModernSofa (cushions, arms, legs as separate upholstered parts), the rest infinigen's;
    // the choice is its own stream so infinigen's draws are unchanged
    let mut pick = Rng::seeded(crate::int_hash(&["modern_sofa", &rng.clone().next_u64().to_string()]));
    if std::env::var_os("NO_MODERN_SOFA").is_none() && pick.f64() < 0.6 {
        return Some(Box::new(super::modern_sofa::ModernSofa::new(&mut rng.clone(), c == "ArmChairFactory")));
    }
    Some(Box::new(Sofa::new(rng, c == "ArmChairFactory")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sofa_cheap_outline() {
        let (mut worst, mut tf, mut tc) = (0.0f64, 0.0, 0.0);
        for seed in 0..24u64 {
            for arm in [false, true] {
                let s = Sofa::new(&mut Rng::seeded(seed), arm);
                let t = web_time::Instant::now();
                let (full, _) = sofa_geometry_gn_lvl(&s.params, None).unwrap();
                tf += t.elapsed().as_secs_f64();
                let t = web_time::Instant::now();
                let (cheap, _) = sofa_geometry_gn_lvl(&s.params, Some(0)).unwrap();
                tc += t.elapsed().as_secs_f64();
                let (a, b) = (full.bounds(), cheap.bounds());
                let e = (0..3).map(|k| (a.0[k] - b.0[k]).abs().max((a.1[k] - b.1[k]).abs())).fold(0.0, f64::max);
                if e > 0.02 { eprintln!("seed {seed} arm {arm} err {e:.4} {a:?} {b:?}"); }
                worst = worst.max(e);
            }
        }
        eprintln!("sofa outline: worst bbox err {worst:.4} m, full {tf:.2}s cheap {tc:.2}s");
    }
}
