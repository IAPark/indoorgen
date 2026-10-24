//! tables/table_utils.py node groups ported as plain geometry: n-gon profile + n-gon cylinder lofts, anchors, legs & stretchers, table top.
use super::geomkit::*;
use crate::mesh::*;
use std::f64::consts::PI;

pub type P2 = [f64; 2];

/// nodegroup_n_gon_profile: circle of radius 0.5 with n points (rotated pi/n then -pi/2), scaled (width, width*aspect), corners filleted
pub fn ngon_profile(n: usize, width: f64, aspect: f64, fillet_ratio: f64) -> Vec<P2> {
    let n = n.max(3);
    let pts: Vec<P2> = (0..n)
        .map(|k| {
            let a = PI / n as f64 + 2.0 * PI * k as f64 / n as f64 - PI / 2.0;
            [0.5 * a.cos() * width, 0.5 * a.sin() * width * aspect]
        })
        .collect();
    fillet_polygon(&pts, width * fillet_ratio, 8)
}

/// GeometryNodeFilletCurve (poly mode, limit radius): replace each corner by `count` points on an arc
pub fn fillet_polygon(p: &[P2], r: f64, count: usize) -> Vec<P2> {
    if r <= 1e-9 {
        return p.to_vec();
    }
    let n = p.len();
    let mut out = vec![];
    for i in 0..n {
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let (u, v) = ([a[0] - b[0], a[1] - b[1]], [c[0] - b[0], c[1] - b[1]]);
        let (lu, lv) = ((u[0] * u[0] + u[1] * u[1]).sqrt(), (v[0] * v[0] + v[1] * v[1]).sqrt());
        let (u, v) = ([u[0] / lu, u[1] / lu], [v[0] / lv, v[1] / lv]);
        let cosang = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0);
        let ang = cosang.acos();
        if ang < 1e-3 || (PI - ang) < 1e-3 {
            out.push(b);
            continue;
        }
        // distance from corner to tangent points
        let mut d = r / (ang / 2.0).tan();
        d = d.min(lu / 2.0).min(lv / 2.0);
        let rr = d * (ang / 2.0).tan();
        let t0 = [b[0] + u[0] * d, b[1] + u[1] * d];
        let t1 = [b[0] + v[0] * d, b[1] + v[1] * d];
        let bis = [u[0] + v[0], u[1] + v[1]];
        let lb = (bis[0] * bis[0] + bis[1] * bis[1]).sqrt();
        let cen = [b[0] + bis[0] / lb * rr / (ang / 2.0).sin(), b[1] + bis[1] / lb * rr / (ang / 2.0).sin()];
        let (a0, a1) = ((t0[1] - cen[1]).atan2(t0[0] - cen[0]), (t1[1] - cen[1]).atan2(t1[0] - cen[0]));
        let mut da = a1 - a0;
        while da > PI {
            da -= 2.0 * PI;
        }
        while da < -PI {
            da += 2.0 * PI;
        }
        for k in 0..count {
            let t = if count > 1 { k as f64 / (count - 1) as f64 } else { 0.0 };
            let a = a0 + da * t;
            out.push([cen[0] + rr * a.cos(), cen[1] + rr * a.sin()]);
        }
    }
    out
}

/// FloatCurve through control points (x in 0..1): smooth cubic Hermite with Catmull-Rom tangents
pub fn float_curve(pts: &[(f64, f64)], t: f64) -> f64 {
    if pts.is_empty() {
        return 1.0;
    }
    let n = pts.len();
    if t <= pts[0].0 {
        return pts[0].1;
    }
    if t >= pts[n - 1].0 {
        return pts[n - 1].1;
    }
    let mut i = 0;
    while i + 1 < n && t > pts[i + 1].0 {
        i += 1;
    }
    let (x0, y0, x1, y1) = (pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1);
    let slope = |k: usize| -> f64 {
        if k == 0 {
            (pts[1].1 - pts[0].1) / (pts[1].0 - pts[0].0)
        } else if k == n - 1 {
            (pts[n - 1].1 - pts[n - 2].1) / (pts[n - 1].0 - pts[n - 2].0)
        } else {
            (pts[k + 1].1 - pts[k - 1].1) / (pts[k + 1].0 - pts[k - 1].0)
        }
    };
    let (m0, m1) = (slope(i), slope(i + 1));
    let h = x1 - x0;
    let s = (t - x0) / h;
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * y0 + (s3 - 2.0 * s2 + s) * h * m0 + (-2.0 * s3 + 3.0 * s2) * y1 + (s3 - s2) * h * m1
}

/// nodegroup_n_gon_cylinder: profile polygon swept down from z=0 to z=-height, scaled by radius_fn(t), flat caps
pub fn ngon_cylinder(profile: &[P2], height: f64, z_res: usize, radius_fn: &dyn Fn(f64) -> f64, caps: bool) -> Mesh {
    let rings: Vec<Vec<V3>> = (0..z_res)
        .map(|i| {
            let t = i as f64 / (z_res - 1) as f64;
            let r = radius_fn(t);
            profile.iter().map(|p| [p[0] * r, p[1] * r, -t * height]).collect()
        })
        .collect();
    let mut m = Mesh::new();
    let n = profile.len();
    for k in 0..z_res - 1 {
        for i in 0..n {
            let j = (i + 1) % n;
            m.add_quad(rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]);
        }
    }
    if caps {
        for (ring, up) in [(&rings[0], true), (&rings[z_res - 1], false)] {
            let c = [ring.iter().map(|p| p[0]).sum::<f64>() / n as f64, ring.iter().map(|p| p[1]).sum::<f64>() / n as f64, ring[0][2]];
            for i in 0..n {
                let j = (i + 1) % n;
                if up {
                    m.add_tri(ring[i], c, ring[j]);
                } else {
                    m.add_tri(ring[i], ring[j], c);
                }
            }
        }
    }
    m
}

/// nodegroup_create_anchors: leg anchor points (x,y) for `n` legs
pub fn anchors(n: usize, width: f64, aspect: f64) -> Vec<P2> {
    match n {
        1 => vec![[0.0, 0.0]],
        2 => vec![[0.3535 * width, 0.0], [-0.3535 * width, 0.0]],
        _ => ngon_profile(n, width, aspect, 0.0),
    }
}

/// place a unit leg (hangs down local -z from z=0, scaled in z by the leg length) from top anchor `top` to the floor point
pub fn place_leg(leg: &Mesh, top: V3, bottom_scale: f64, align_x: bool) -> Mesh {
    let bot = [top[0] * bottom_scale, top[1] * bottom_scale, 0.0];
    let v = [top[0] - bot[0], top[1] - bot[1], top[2] - bot[2]];
    let l = norm3(v).max(1e-9);
    let z = [v[0] / l, v[1] / l, v[2] / l];
    // local X: radial direction toward the anchor, projected perpendicular to z
    let mut x = [top[0], top[1], top[2]];
    let d = x[0] * z[0] + x[1] * z[1] + x[2] * z[2];
    x = [x[0] - d * z[0], x[1] - d * z[1], x[2] - d * z[2]];
    if !align_x || norm3(x) < 1e-6 {
        x = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let d = x[0] * z[0] + x[1] * z[1] + x[2] * z[2];
        x = [x[0] - d * z[0], x[1] - d * z[1], x[2] - d * z[2]];
    }
    let x = unit3(x);
    let y = cross(z, x);
    let mut m = leg.clone();
    for p in m.pos.iter_mut() {
        let (lx, ly, lz) = (p[0], p[1], p[2] * l);
        *p = [top[0] + x[0] * lx + y[0] * ly + z[0] * lz, top[1] + x[1] * lx + y[1] * ly + z[1] * lz, top[2] + x[2] * lx + y[2] * ly + z[2] * lz];
    }
    m
}

/// stretcher cylinder between two points
pub fn stretcher(a: V3, b: V3, width: f64, n: usize) -> Mesh {
    let prof = ngon_profile(n, width, 1.0, 0.0);
    let cyl = ngon_cylinder(&prof, 1.0, 2, &|_| 1.0, true);
    place_leg(&cyl, a, 1.0, false).tap(|m| {
        // reorient: map local z-length onto (b - a)
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = norm3(v).max(1e-9);
        let z = unit3(v);
        let x0 = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let d = x0[0] * z[0] + x0[1] * z[1] + x0[2] * z[2];
        let x = unit3([x0[0] - d * z[0], x0[1] - d * z[1], x0[2] - d * z[2]]);
        let y = cross(z, x);
        m.pos = cyl.pos.iter().map(|p| { let (lx, ly, lz) = (p[0], p[1], -p[2] * l); [a[0] + x[0] * lx + y[0] * ly + z[0] * lz, a[1] + x[1] * lx + y[1] * ly + z[1] * lz, a[2] + x[2] * lx + y[2] * ly + z[2] * lz] }).collect();
    })
}

trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&mut Self)) -> Self;
}
impl Tap for Mesh {
    fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

/// table top slab: filleted polygon extruded `thickness` upward from z0 (bevel of the top edge ~ vertical fillet)
pub fn table_top(n: usize, width: f64, aspect: f64, fillet: f64, thickness: f64, vfillet: f64, z0: f64) -> Mesh {
    let poly = ngon_profile(n, width, aspect, fillet);
    let m = extrude(&poly, z0, z0 + thickness);
    let r = (vfillet * thickness * 0.5).max(0.0);
    if r > 1e-4 {
        m.smoothed(r, 3, 0)
    } else {
        m
    }
}

// ======================================================================= exact node-graph ports (curvekit based)
use super::curvekit::*;

fn rz(a: f64) -> Mat4 {
    Mat4::euler_xyz([0.0, 0.0, a])
}

/// nodegroup_n_gon_profile: cyclic curve (circle r=0.5, rotated by 3.1416/n then -1.5708, scaled (w, w*aspect), poly-filleted)
pub fn ngon_profile_curve(n: usize, width: f64, aspect: f64, fillet_ratio: f64) -> Curve {
    let mut c = Curve::circle(n, 0.5);
    c.transform_trs([0.0; 3], [0.0, 0.0, 3.1416f32 / n as f32], [1.0; 3]);
    c.transform_trs([0.0; 3], [0.0, 0.0, -1.5708f32], [1.0; 3]);
    c.transform_trs([0.0; 3], [0.0; 3], [width as f32, aspect as f32 * width as f32, 1.0]);
    c.fillet_poly((width as f32 * fillet_ratio as f32) as f64, 8)
}

pub struct NgonCyl {
    pub mesh: Mesh,
    pub profile: Curve,
    pub caps: Mesh,
}

/// nodegroup_n_gon_cylinder with a radius curve (poly curve in the xz plane); `res` main-curve samples, `prof_res` profile samples
pub fn ngon_cylinder_nodes(radius_curve: &Curve, height: f64, n: usize, width: f64, aspect: f64, fillet: f64, prof_res: usize, res: usize) -> NgonCyl {
    let mut main = Curve::line([0.0, 0.0, 0.0], [0.0, 0.0, -height]);
    main.set_tilt(3.1416);
    let main = main.resample(res);
    // spline parameter factor
    let mut acc = vec![0.0f32];
    for i in 1..main.pts.len() {
        acc.push(acc[i - 1] + vlen3(sub(main.pts[i], main.pts[i - 1])) as f32);
    }
    let tot = *acc.last().unwrap();
    let factors: Vec<f64> = acc.iter().map(|a| (a / tot) as f64).collect();
    let prof = ngon_profile_curve(n, width, aspect, fillet);
    let prof = prof.resample(prof_res);
    let mut mesh = curve_to_mesh(&main, &prof, true);
    // z range of the radius curve
    let zmin = radius_curve.pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let zmax = radius_curve.pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    let np = prof.pts.len();
    for (i, &f) in factors.iter().enumerate() {
        let s = radius_curve.sample_factor(f);
        let l = (s[0] * s[0] + s[1] * s[1]).sqrt();
        let z = -height + (s[2] - zmin) / (zmax - zmin) * (0.0 + height);
        for j in 0..np {
            let p = &mut mesh.pos[i * np + j];
            *p = [p[0] * l, p[1] * l, z];
        }
    }
    // caps: ring 0 and the last ring (vertex order as left by DeleteGeometry, face loops as left by FlipFaces)
    let mut caps = Mesh::new();
    let last = (factors.len() - 1) * np;
    for j in 0..np {
        caps.pos.push(mesh.pos[j]);
    }
    for j in 0..np {
        caps.pos.push(mesh.pos[last + j]);
    }
    let mut f0: Vec<u32> = vec![(np - 1) as u32];
    f0.extend(0..(np - 1) as u32);
    caps.faces.push(f0);
    let mut f1: Vec<u32> = vec![np as u32];
    f1.extend((np as u32 + 1..2 * np as u32).rev());
    caps.faces.push(f1);
    NgonCyl { mesh, profile: prof, caps }
}

fn vlen3(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// nodegroup_generate_radius_curve: line (1,0,1)->(1,0,-1) resampled, x multiplied by the float curve of the spline factor
pub fn radius_curve(ctrl: &[(f64, f64)], res: usize) -> Curve {
    let line = Curve::line([1.0, 0.0, 1.0], [1.0, 0.0, -1.0]).resample(res);
    let fc = FloatCurve::new(ctrl);
    let mut acc = vec![0.0f32];
    for i in 1..line.pts.len() {
        acc.push(acc[i - 1] + vlen3(sub(line.pts[i], line.pts[i - 1])) as f32);
    }
    let tot = *acc.last().unwrap();
    let mut c = line.clone();
    for (i, p) in c.pts.iter_mut().enumerate() {
        p[0] *= fc.eval((acc[i] / tot) as f64).clamp(0.0, 1.0); // the node's mapping has clip enabled: y limited to [0,1]
    }
    c
}

pub fn plain_radius_curve() -> Curve {
    Curve::line([1.0, 0.0, 1.0], [1.0, 0.0, -1.0])
}

/// nodegroup_generate_table_top (+ the Translation by the caller): returns the joined mesh at z in [0, thickness]
pub fn table_top_exact(n: usize, width: f64, aspect: f64, fillet: f64, thickness: f64, vfillet: f64) -> Mesh {
    let cyl = ngon_cylinder_nodes(&plain_radius_curve(), thickness, n, width, aspect, fillet, 512, 10);
    let mut arc = Curve::arc(4, 0.7071, 0.0, 4.7124);
    arc.transform(&Mat4::euler_xyz([0.0, 0.0, -0.7854]));
    arc.transform(&Mat4::euler_xyz([0.0, 1.5708, 0.0]));
    arc.transform(&Mat4::translation([0.0, 0.5, 0.0]));
    arc.transform(&Mat4::scale([1.0, vfillet, 1.0]));
    let mut prof = arc.fillet_poly(vfillet, 8);
    let m6 = Mat4::scale([thickness, thickness, thickness]).mul(&Mat4::identity());
    let r6 = Mat4::euler_xyz([1.5708, 1.5708, 0.0]);
    prof.transform(&r6);
    prof.transform(&m6);
    let mut mesh = curve_to_mesh(&cyl.profile, &prof, false);
    let mut out = cyl.caps.clone();
    // transform_5 (z - th/2) applies to the swept mesh only
    for p in mesh.pos.iter_mut() {
        p[2] -= thickness * 0.5;
    }
    out.append(&mesh);
    for p in out.pos.iter_mut() {
        p[2] += thickness;
    }
    out
}

/// nodegroup_create_anchors points (z = 0)
pub fn anchor_points(n: usize, width: f64, aspect: f64) -> Vec<V3> {
    match n {
        1 => vec![[0.0, 0.0, 0.0]],
        2 => vec![[width * 0.3535, 0.0, 0.0], [width * -0.3535, 0.0, 0.0]],
        _ => ngon_profile_curve(n, width, aspect, 0.0).pts,
    }
}

fn norm(a: V3) -> V3 {
    let l = vlen3(a);
    if l == 0.0 { a } else { [a[0] / l, a[1] / l, a[2] / l] }
}

/// instance transform for a leg/stretcher instance: local z -> axis `v`, then (optionally) pivot about it so local x points at `target`'s projection
pub fn align_frame(v: V3, target: Option<V3>) -> [V3; 3] {
    let vh = norm(v);
    // minimal rotation of z onto vh
    let z = [0.0, 0.0, 1.0];
    let ax = cross(z, vh);
    let (mut x, mut y);
    if vlen3(ax) < 1e-12 {
        if vh[2] >= 0.0 {
            x = [1.0, 0.0, 0.0];
            y = [0.0, 1.0, 0.0];
        } else {
            x = [1.0, 0.0, 0.0];
            y = [0.0, -1.0, 0.0];
        }
    } else {
        let axn = norm(ax);
        let ang = dot(z, vh).clamp(-1.0, 1.0).acos();
        let rot = |d: V3| -> V3 {
            let a = [axn[0] * dot(d, axn), axn[1] * dot(d, axn), axn[2] * dot(d, axn)];
            let df = sub(d, a);
            let cr = cross(axn, df);
            [a[0] + df[0] * ang.cos() + cr[0] * ang.sin(), a[1] + df[1] * ang.cos() + cr[1] * ang.sin(), a[2] + df[2] * ang.cos() + cr[2] * ang.sin()]
        };
        x = rot([1.0, 0.0, 0.0]);
        y = rot([0.0, 1.0, 0.0]);
    }
    if let Some(t) = target {
        let d = dot(t, vh);
        let pt = [t[0] - vh[0] * d, t[1] - vh[1] * d, t[2] - vh[2] * d];
        let degenerate = vlen3(pt) <= 1e-12;
        {
            let d2 = dot(x, vh);
            let pa = norm([x[0] - vh[0] * d2, x[1] - vh[1] * d2, x[2] - vh[2] * d2]);
            let pt = norm(pt);
            let ang = dot(pa, pt).clamp(-1.0, 1.0).acos();
            let sgn = if dot(cross(pa, pt), vh) < 0.0 { -1.0 } else { 1.0 };
            // Blender quirk (measured): a vector parallel to the pivot axis yields a 60 degree turn about the pivot
            let ang = if degenerate { std::f64::consts::FRAC_PI_3 } else { ang * sgn };
            let rot = |d: V3| -> V3 {
                let a = [vh[0] * dot(d, vh), vh[1] * dot(d, vh), vh[2] * dot(d, vh)];
                let df = sub(d, a);
                let cr = cross(vh, df);
                [a[0] + df[0] * ang.cos() + cr[0] * ang.sin(), a[1] + df[1] * ang.cos() + cr[1] * ang.sin(), a[2] + df[2] * ang.cos() + cr[2] * ang.sin()]
            };
            x = rot(x);
            y = rot(y);
        }
    }
    [x, y, vh]
}

/// place a mesh (local, hanging down -z) at `pos` with frame and z scale
pub fn place_instance(m: &Mesh, pos: V3, frame: &[V3; 3], zscale: f64) -> Mesh {
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        let (lx, ly, lz) = (p[0], p[1], p[2] * zscale);
        *p = [pos[0] + frame[0][0] * lx + frame[1][0] * ly + frame[2][0] * lz, pos[1] + frame[0][1] * lx + frame[1][1] * ly + frame[2][1] * lz, pos[2] + frame[0][2] * lx + frame[1][2] * ly + frame[2][2] * lz];
    }
    o
}

/// nodegroup_generate_leg_straight (Resolution `res`, profile resolution 64)
pub fn leg_straight(ctrl: &[(f64, f64)], height: f64, diameter: f64, res: usize, ngon: usize, fillet: f64) -> Mesh {
    let rc = radius_curve(ctrl, res);
    ngon_cylinder_nodes(&rc, height, ngon, diameter, 1.0, fillet, 64, res).mesh
}

/// nodegroup_generate_single_stand
pub fn leg_single_stand(ctrl: &[(f64, f64)], height: f64, diameter: f64, res: usize) -> Mesh {
    let rc = radius_curve(ctrl, res);
    ngon_cylinder_nodes(&rc, height, res, diameter, 1.0, 0.0, 64, res).mesh
}

/// nodegroup_strecher (unit height, 32-gon of the given width, default fillet ratio 0.2, resolution 64)
pub fn strecher(width: f64) -> Mesh {
    ngon_cylinder_nodes(&plain_radius_curve(), 1.0, 32, width, 1.0, 0.2, 64, 64).mesh
}

/// nodegroup_generate_leg_square
pub fn leg_square(width: f64, height: f64, connector: bool, profile_width: f64) -> Mesh {
    let res = if connector { 5 } else { 4 };
    let sweep = if connector { 6.2832 } else { 4.7124 };
    let mut c = Curve::arc(res, 0.7071, 0.0, sweep);
    if connector {
        let l = c.pts.pop().unwrap();
        for k in 0..3 {
            c.pts[0][k] = (c.pts[0][k] + l[k]) / 2.0; // MergeByDistance (All) puts the merged vertex at the group centroid
        }
        c.cyclic = true;
    }
    let n = c.pts.len();
    c.tilt = vec![if connector { 3.1416 } else { 1.5708 }; n];
    c.transform(&rz(-0.7854));
    c.transform(&Mat4::translation([0.0, 0.0, -0.5]).mul(&Mat4::euler_xyz([1.5708, 0.0, 0.0])));
    c.transform(&Mat4::scale([width, 1.0, height]));
    let c = c.fillet_poly(0.03, 8);
    let prof = ngon_profile_curve(4, profile_width, 0.5, 0.1);
    let mut m = curve_to_mesh(&c, &prof, true);
    m.transform(&rz(1.5708));
    m
}

pub enum LegKind<'a> {
    Straight { ctrl: &'a [(f64, f64)], ngon: usize, diameter: f64 },
    Stand { ctrl: &'a [(f64, f64)], diameter: f64 },
    Square { width: f64, connector: bool, profile_width: f64 },
}

/// nodegroup_create_legs_and_strechers on anchor points: returns (legs, stretchers) meshes in join order
pub fn legs_and_stretchers(anchors: &[V3], table_height: f64, bottom_scale: f64, leg: &Mesh, stretcher: Option<(&Mesh, usize, f64)>) -> Vec<Mesh> {
    let mut out = vec![];
    let tops: Vec<V3> = anchors.iter().map(|p| [p[0], p[1], p[2] + table_height]).collect();
    let vecs: Vec<V3> = tops
        .iter()
        .map(|p| {
            let rel = [p[0], p[1], p[2] - table_height];
            let bot = [rel[0] * bottom_scale, rel[1] * bottom_scale, rel[2]];
            sub(*p, bot)
        })
        .collect();
    for (p, v) in tops.iter().zip(&vecs) {
        let fr = align_frame(*v, Some(*p));
        out.push(place_instance(leg, *p, &fr, vlen3(*v)));
    }
    if let Some((st, incr, rel)) = stretcher {
        let n = tops.len();
        let pts: Vec<V3> = tops.iter().zip(&vecs).map(|(p, v)| [p[0] + v[0] * -rel, p[1] + v[1] * -rel, p[2] + v[2] * -rel]).collect();
        let eq = incr != 0 && (n as f64 / incr as f64) == 2.0;
        for i in 0..n {
            if eq && !((i as f64) < n as f64 / 2.0) {
                continue;
            }
            let j = (i + incr) % n;
            let d = sub(pts[i], pts[j]);
            let fr = if vlen3(d) == 0.0 {
                let (s, c) = std::f64::consts::FRAC_PI_3.sin_cos();
                [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
            } else {
                align_frame(d, Some([0.0, 0.0, 1.0]))
            };
            out.push(place_instance(st, pts[i], &fr, vlen3(d)));
        }
    }
    out
}
