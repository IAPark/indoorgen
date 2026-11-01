//! seating/chairs/office_chair.py + seats/curvy_seats.py (+ table_decorations lofting / warp_around_curve) + tables/legs/wheeled.py
use super::curvekit::*;
use crate::mesh::*;
use std::f64::consts::PI;

pub struct SeatParams {
    pub width: f64,
    pub thickness: f64,
    pub front_rel: f64,
    pub front_bent: f64,
    pub seat_bent: f64,
    pub mid_rel: f64,
    pub mid_bent: f64,
    pub back_rel: f64,
    pub back_bent: f64,
    pub top_rel: f64,
    pub top_bent: f64,
    pub seat_height: f64,
    pub mid_pos: f64,
}

impl SeatParams {
    pub fn office(width: f64, seat_height: f64, front_rel: f64, front_bent: f64, seat_bent: f64, mid_bent: f64, mid_rel: f64, back_bent: f64, back_rel: f64, mid_pos: f64) -> SeatParams {
        SeatParams { width, thickness: 0.03, front_rel, front_bent, seat_bent, mid_rel, mid_bent, back_rel, back_bent, top_rel: 0.5, top_bent: -0.2, seat_height, mid_pos }
    }
}

/// nodegroup_bent in float32: rotate each position about z by |p| * p.x * amount
fn bent(pts: &mut [V3], amount: f64) {
    let am = amount as f32;
    for p in pts.iter_mut() {
        let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let len = (x * x + y * y + z * z).sqrt();
        let ang = (len * x) * am;
        let (s, c) = (ang.sin(), ang.cos());
        let nx = c * x + (-s) * y;
        let ny = s * x + c * y;
        *p = [nx as f64, ny as f64, z as f64];
    }
}

/// CurveCircle (radius 0.5, float32) then Transform (scale, translation)
fn circle_profile(n: usize, scale: [f64; 3], z: f64) -> Vec<V3> {
    let step = (2.0f32 * std::f32::consts::PI) / n as f32;
    let (sx, sy, sz, tz) = (scale[0] as f32, scale[1] as f32, scale[2] as f32, z as f32);
    (0..n)
        .map(|i| {
            let th = step * i as f32;
            let x = 0.5f32 * th.cos();
            let y = 0.5f32 * th.sin();
            [(sx * x) as f64, (sy * y) as f64, (sz * 0.0f32 + tz) as f64]
        })
        .collect()
}

/// generate_curvy_seats
pub fn curvy_seat(sp: &SeatParams, u_res: usize, v_res: usize) -> Mesh {
    let w = sp.width;
    let m32 = |a: f64, b: f64| -> f64 { ((a as f32) * (b as f32)) as f64 };
    let t = sp.thickness;
    // profile curves in output (join) order: ascending z
    let c0 = circle_profile(u_res, [0.0, 0.005, 1.0], 0.0);
    let mut c1 = circle_profile(u_res, [m32(w, sp.front_rel), 0.005, 1.0], 0.06);
    bent(&mut c1, sp.front_bent);
    let mut c2 = circle_profile(u_res, [w, t, 1.0], 0.5);
    bent(&mut c2, sp.seat_bent);
    let mut c3 = circle_profile(u_res, [m32(w, sp.mid_rel), t, 1.0], 1.0);
    bent(&mut c3, sp.mid_bent);
    let mut c4 = circle_profile(u_res, [m32(w, sp.back_rel), t, 1.0], 1.5);
    bent(&mut c4, sp.back_bent);
    let mut c5 = circle_profile(u_res, [m32(w, sp.top_rel), 0.005, 1.0], 2.02);
    bent(&mut c5, sp.top_bent);
    let c6 = circle_profile(u_res, [0.0, 0.005, 1.0], 2.1);
    let profiles = [c0, c1, c2, c3, c4, c5, c6];
    // lofting: per u a Catmull-Rom spline through the 7 profile points, resampled to v_res points
    let mut grid: Vec<V3> = vec![[0.0; 3]; u_res * v_res];
    for u in 0..u_res {
        let ctrl: Vec<V3> = profiles.iter().map(|p| p[u]).collect();
        let sp = catmull_rom_eval(&ctrl, 12).resample(v_res);
        for r in 0..v_res {
            grid[r * u_res + u] = sp.pts[r];
        }
    }
    // warp around the seat spine curve
    let start = [0.0, -w * 0.5, 0.03];
    let sh = [0.0, sp.mid_pos, -0.05];
    let eh = [0.0, 0.1, 0.1];
    let end = [0.0, w * 0.5, sp.seat_height];
    let curve = bezier_segment_eval(start, sh, eh, end, 128).resample(1025);
    let tan = curve.tangents();
    let nor = curve.normals(&tan);
    let (mut zmin, mut zmax) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &grid {
        zmin = zmin.min(p[2] as f32 as f64);
        zmax = zmax.max(p[2] as f32 as f64);
    }
    let mut m = Mesh::new();
    for p in &grid {
        // float32 map range (clamped) * 1024, rounded half up
        let (z32, lo32, hi32) = (p[2] as f32, zmin as f32, zmax as f32);
        let f = (((z32 - lo32) / (hi32 - lo32)) * 1.0f32 + 0.0f32).clamp(0.0, 1.0);
        let idx = (1024.0f32 * f + 0.5f32).floor() as usize;
        let (n, tg) = (nor[idx], tan[idx]);
        let b = cross(tg, n);
        let q = curve.pts[idx];
        m.pos.push([q[0] + n[0] * p[0] + b[0] * p[1], q[1] + n[1] * p[0] + b[1] * p[1], q[2] + n[2] * p[0] + b[2] * p[1]]);
    }
    // MeshCylinder topology: top cap, side quads, bottom cap
    let u = u_res as u32;
    m.faces.push((0..u).collect());
    for r in 0..(v_res as u32 - 1) {
        for i in 0..u {
            let i1 = (i + 1) % u;
            m.faces.push(vec![r * u + i, (r + 1) * u + i, (r + 1) * u + i1, r * u + i1]);
        }
    }
    let last = (v_res as u32 - 1) * u;
    m.faces.push((0..u).rev().map(|i| last + i).collect());
    let _ = PI;
    m
}

// ---------------------------------------------------------------------------------------------- factory
use super::tableparts::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "OfficeChairFactory" | "BarChairFactory")
}

const SEAT: Weighted = &[("fabric.Velvet", 0.3), ("fabric.SofaFabric", 0.5), ("fabric.Leather", 0.2)];
const LEG: Weighted = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0), ("wood.Wood", 1.0), ("ceramic.GlassVolume", 1.0)];

pub struct OfficeChair {
    x: f64,
    z: f64,
    top_t: f64,
    wheeled: bool,
    sp: SeatParams,
    bottom_scale: f64,
    leg_d: f64,
    curve: Vec<(f64, f64)>,
    poles: usize,
    joint_h: f64,
    wheel_arc: f64,
    wheel_w: f64,
    wheel_rot: f64,
    pole_len: f64,
    seed: u64,
    /// collision measured when the chair is made: the base's box and the seat's (the seat surface sampled coarsely), a quarter-turned
    col: Vec<(V3, V3)>,
}

/// the bounds of `meshes` turned a quarter about z (x, y) -> (-y, x), grown by `g`
fn quarter_box(meshes: &[Mesh], g: f64) -> (V3, V3) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for m in meshes { let (a, b) = m.bounds(); for k in 0..3 { lo[k] = lo[k].min(a[k]); hi[k] = hi[k].max(b[k]); } }
    ([-hi[1] - g, lo[0] - g, lo[2] - g], [-lo[1] + g, hi[0] + g, hi[2] + g])
}

impl OfficeChair {
    fn new(rng: &mut Rng) -> OfficeChair {
        let mut r = rng.clone();
        let r = &mut r;
        let x = r.uniform(0.5, 0.6);
        let z = r.uniform(1.0, 1.4);
        let top_t = r.uniform(0.5, 0.7);
        let wheeled = r.choice_idx(2) == 1;
        let front_rel = r.uniform(0.5, 0.8);
        let front_bent = r.uniform(-1.5, -0.4);
        let seat_bent = r.uniform(-1.5, -0.4);
        let mid_bent = r.uniform(-2.4, -0.5);
        let mid_rel = r.uniform(0.5, 0.9);
        let back_bent = r.uniform(-1.0, -0.1);
        let back_rel = r.uniform(0.6, 0.9);
        let mid_pos = r.uniform(0.4, 0.6);
        let _leg_ngon = r.choice_idx(2);
        let bottom_scale = r.uniform(1.1, 1.3);
        let sp = SeatParams::office(x, top_t, front_rel, front_bent, seat_bent, mid_bent, mid_rel, back_bent, back_rel, mid_pos);
        let (mut leg_d, mut curve, mut poles, mut joint_h) = (0.0, vec![], 4, 0.0);
        let (mut wheel_arc, mut wheel_w, mut wheel_rot, mut pole_len) = (0.0, 0.0, 0.0, 0.0);
        if !wheeled {
            leg_d = r.uniform(0.7 * x, 0.9 * x);
            curve = vec![(0.0, r.uniform(0.1, 0.2)), (0.5, r.uniform(0.1, 0.2)), (0.9, r.uniform(0.2, 0.3)), (1.0, 1.0)];
        } else {
            leg_d = r.uniform(0.03, 0.05);
            poles = [4, 5][r.choice_idx(2)];
            joint_h = r.uniform(0.5, 0.8) * (z - top_t);
            wheel_arc = r.uniform(120.0, 240.0);
            wheel_w = r.uniform(0.11, 0.15);
            wheel_rot = r.uniform(0.0, 360.0);
            pole_len = r.uniform(1.6, 2.0);
        }
        let mut c = OfficeChair { x, z, top_t, wheeled, sp, bottom_scale, leg_d, curve, poles, joint_h, wheel_arc, wheel_w, wheel_rot, pole_len, seed: r.randint(0, 1_000_000) as u64, col: vec![] };
        // built to the layout box [width (y), depth (x), height]: the seat width x (and with it the stand / the wheel base) gives the
        // footprint, the height scales the seat height and the stand with it
        if let Some(h) = crate::api::size_hint() {
            for _ in 0..3 {
                let e = c.extents();
                let f = (h[1] / e[0]).min(h[0] / e[1]);
                c.x *= f;
                c.sp.width = c.x;
                if c.wheeled { c.pole_len *= f; c.wheel_w *= f } else { c.leg_d *= f }
                let zf = h[2] / e[2];
                c.z *= zf;
                c.top_t *= zf;
                c.sp.seat_height = c.top_t;
                c.joint_h *= zf;
            }
        }
        // its collision, measured once: the base, and the seat (sampled at 48 x 32; the 256 x 128 build bulges up to ~1 cm between samples)
        let parts = c.parts((48, 32));
        let n = parts.len();
        c.col = vec![quarter_box(&parts[..n - 1], 0.003), quarter_box(&parts[n - 1..], 0.012)];
        c
    }

    /// the parts as asset() builds them (seat at a coarse resolution: its bounds are what is needed), before the quarter turn
    fn parts(&self, seat_res: (usize, usize)) -> Vec<Mesh> {
        let top_h = self.z - self.top_t;
        let mut seat = curvy_seat(&self.sp, seat_res.0, seat_res.1);
        seat.transform(&Mat4::translation([0.0, 0.0, top_h]));
        let mut parts: Vec<Mesh> = vec![];
        if !self.wheeled {
            let leg = leg_single_stand(&self.curve, 1.0, self.leg_d, 64);
            parts.extend(legs_and_stretchers(&[[0.0, 0.0, 0.0]], top_h, self.bottom_scale, &leg, None));
        } else {
            parts.extend(wheeled_leg(self.joint_h, self.leg_d, top_h, self.wheel_arc, self.wheel_w, self.wheel_rot, self.pole_len, self.poles));
        }
        parts.push(seat);
        parts
    }

    /// overall extents in the final frame (the parts are turned a quarter about z: x extent = y extent before)
    fn extents(&self) -> [f64; 3] {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for m in self.parts((48, 32)) {
            let (a, b) = m.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]]
    }
}

impl Factory for OfficeChair {
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        Some(self.col.iter().map(|(lo, hi)| crate::piece::Volume::hard(*lo, *hi)).collect())
    }
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        let e = self.extents();
        Some([e[1], e[0], e[2]])
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let seat_m = spec_material("seat", json!({"cls": weighted(&mut r, SEAT), "kwargs": {}, "seed": self.seed}), [0.35, 0.3, 0.3]);
        let leg_m = spec_material("leg", json!({"cls": weighted(&mut r, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.35, 0.3, 0.3]);
        let top_h = self.z - self.top_t;
        let mut seat = curvy_seat(&self.sp, 256, 128);
        seat.transform(&Mat4::translation([0.0, 0.0, top_h]));
        let mut parts: Vec<Mesh> = vec![];
        if !self.wheeled {
            let leg = leg_single_stand(&self.curve, 1.0, self.leg_d, 64);
            parts.extend(legs_and_stretchers(&[[0.0, 0.0, 0.0]], top_h, self.bottom_scale, &leg, None));
        } else {
            parts.extend(wheeled_leg(self.joint_h, self.leg_d, top_h, self.wheel_arc, self.wheel_w, self.wheel_rot, self.pole_len, self.poles));
        }
        parts.push(seat);
        let rot = Mat4::euler_xyz([0.0, 0.0, std::f64::consts::FRAC_PI_2]);
        let mut a = Asset::default();
        for (i, mut m) in parts.into_iter().enumerate() {
            m.transform(&rot);
            let b = m.bounds();
            let mat = if i + 1 == self.n_parts_hint(&m) { &seat_m } else { &leg_m };
            let _ = mat;
            a.mesh(m, if b.1[2] > top_h && i + 1 > 0 && b.0[2] >= top_h - 0.2 { &seat_m } else { &leg_m }, Some(b));
        }
        let _ = (self.x, self.z);
        a
    }
}

impl OfficeChair {
    fn n_parts_hint(&self, _m: &Mesh) -> usize {
        0
    }
}

/// tables/legs/wheeled.py nodegroup_wheeled_leg: meshes in the order python's join leaves them
fn wheeled_leg(joint_h: f64, leg_d: f64, top_h: f64, arc: f64, wheel_w: f64, wheel_rot: f64, pole_len: f64, poles: usize) -> Vec<Mesh> {
    let _ = arc; // the factory's 'Leg Wheel Arc Sweep Angle' is never passed to the node group: its default 240 applies
    let (wm, winst) = chair_wheel(240.0, wheel_w, wheel_rot, 0.5, 0.6, pole_len);
    // transform_geometry: rotation (0, 1.5708, 0) on mesh and nested instances
    let ry = Mat4::euler_xyz([0.0, 1.5708, 0.0]);
    let mut wm = wm;
    wm.transform(&ry);
    let winst: Vec<Mesh> = winst.into_iter().map(|mut m| { m.transform(&ry); m }).collect();
    // anchors: n-gon of width 0.001
    let anchors = super::tableparts::anchor_points(poles, 0.001, 1.0);
    let table_h = 0.025f64;
    let bscale = (2.0f32 / 0.001f32) as f64;
    let mut legs: Vec<Mesh> = vec![];
    for a in &anchors {
        let p = [a[0], a[1], a[2] + table_h];
        // subtract_2 = position - ((position - (0,0,table_h - offset)) * (s, s, 1))
        let rel = [p[0], p[1], p[2] - (table_h - 0.025)];
        let v = [p[0] - rel[0] * bscale, p[1] - rel[1] * bscale, p[2] - rel[2]];
        let r1 = align_euler(&I3, v, 2, None);
        let r2 = align_euler(&r1, p, 0, Some(2));
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        legs.push(apply_inst(&wm, p, &r2, [1.0, 1.0, l]));
        for im in &winst {
            legs.push(apply_inst(im, p, &r2, [1.0, 1.0, l]));
        }
    }
    // AlignBottomToFloor
    let mut minz = f64::INFINITY;
    for m in &legs {
        for q in &m.pos {
            minz = minz.min(q[2] as f32 as f64);
        }
    }
    let offset = -minz;
    for m in legs.iter_mut() {
        for q in m.pos.iter_mut() {
            q[2] = (q[2] as f32 + offset as f32) as f64; // float32 add: the lowest vertex lands exactly on z = 0
        }
    }
    let depth1 = joint_h - offset;
    let mut cyl1 = mesh_cylinder(64, 1, leg_d * 0.5, depth1);
    cyl1.transform(&Mat4::translation([0.0, 0.0, depth1 * 0.5 + offset]));
    let depth2 = top_h - joint_h;
    let mut cyl2 = mesh_cylinder(64, 1, leg_d * 0.5 - 0.0025, depth2);
    cyl2.transform(&Mat4::translation([0.0, 0.0, top_h - 0.5 * depth2]));
    let mut out = vec![cyl2, cyl1];
    out.extend(legs);
    out
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    if c == "BarChairFactory" {
        return Some(Box::new(BarChair::new(rng, fseed)));
    }
    Some(Box::new(OfficeChair::new(rng)))
}

// ---------------------------------------------------------------------------------------------- BarChair
pub struct BarChair {
    fseed: u64,
    x: f64,
    z: f64,
    top_t: f64,
    style: usize, // 0 straight 1 single_stand 2 wheeled
    ngon: usize,
    bottom_scale: f64,
    leg_d: f64,
    curve: Vec<(f64, f64)>,
    legs: usize,
    stretch_pos: f64,
    incr: usize,
    poles: usize,
    joint_h: f64,
    wheel_arc: f64,
    wheel_w: f64,
    wheel_rot: f64,
    pole_len: f64,
    seed: u64,
}

impl BarChair {
    fn new(rng: &mut Rng, fseed: u64) -> BarChair {
        let mut r = rng.clone();
        let r = &mut r;
        let x = r.uniform(0.35, 0.45);
        let z = r.uniform(0.7, 1.0);
        let top_t = r.uniform(0.06, 0.10);
        let style = r.choice_idx(3);
        let ngon = [4usize, 32][r.choice_idx(2)];
        let bottom_scale = r.uniform(1.1, 1.3);
        let mut b = BarChair { fseed, x, z, top_t, style, ngon, bottom_scale, leg_d: 0.0, curve: vec![], legs: 1, stretch_pos: 0.5, incr: 0, poles: 4, joint_h: 0.0, wheel_arc: 0.0, wheel_w: 0.0, wheel_rot: 0.0, pole_len: 0.0, seed: 0 };
        match style {
            1 => {
                b.leg_d = r.uniform(0.7 * x, 0.9 * x);
                b.curve = vec![(0.0, r.uniform(0.1, 0.2)), (0.5, r.uniform(0.1, 0.2)), (0.9, r.uniform(0.2, 0.3)), (1.0, 1.0)];
            }
            0 => {
                b.leg_d = r.uniform(0.04, 0.06);
                b.legs = [3usize, 4][r.choice_idx(2)];
                b.curve = vec![(0.0, 1.0), (0.4, r.uniform(0.85, 0.95)), (1.0, r.uniform(0.4, 0.6))];
                b.stretch_pos = r.uniform(0.6, 0.9);
                b.incr = r.choice_idx(3);
            }
            _ => {
                b.leg_d = r.uniform(0.03, 0.05);
                b.poles = [4usize, 5][r.choice_idx(2)];
                b.joint_h = r.uniform(0.5, 0.8) * (z - top_t);
                b.wheel_arc = r.uniform(120.0, 240.0);
                b.wheel_w = r.uniform(0.11, 0.15);
                b.wheel_rot = r.uniform(0.0, 360.0);
                b.pole_len = r.uniform(1.6, 2.0);
            }
        }
        b.seed = r.randint(0, 1_000_000) as u64;
        b
    }
}

impl Factory for BarChair {
    /// collision: the round seat (its domed cap a little over the top), the base -- legs and stretchers, the stand, or the wheeled
    /// base measured once
    fn volumes(&self, _inst: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        let (x, th) = (self.x, self.top_t);
        let top_h = self.z - th;
        // the seat: a disc with a swept rim and a domed cap, from the instance's draws -- measured on a 24-segment build (the 128-segment
        // one lies within a few mm of it)
        let mut ir = inst_rng(self.fseed, _inst);
        let (cap_radius, bevel) = (ir.uniform(2.0, 3.2), ir.uniform(0.01, 0.04));
        let (slo, shi) = capped_cylinder_full(th, x, cap_radius, bevel / th, 0.0140, -0.0020, 24).bounds();
        let g = 0.006;
        let mut v = vec![Volume::hard([slo[0] - g, slo[1] - g, slo[2] + top_h - g], [shi[0] + g, shi[1] + g, shi[2] + top_h + g])];
        v.push(Volume::support([-x * 0.3, -x * 0.3, shi[2] + top_h - 0.001], [x * 0.3, x * 0.3, shi[2] + top_h]));
        match self.style {
            0 => {
                let anch = anchor_points(self.legs, 0.7 * self.x, 1.0);
                let rl = self.leg_d / 2.0 + 0.003;
                for a in &anch {
                    let (b, t) = ([a[0] * self.bottom_scale, a[1] * self.bottom_scale, 0.0], [a[0], a[1], top_h]);
                    let d = [t[0] - b[0], t[1] - b[1], t[2] - b[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let e = [0, 1, 2].map(|k| rl * (1.0 - (d[k] / l).powi(2)).max(0.0).sqrt());
                    v.push(Volume::hard([0, 1, 2].map(|k| b[k].min(t[k]) - e[k]), [0, 1, 2].map(|k| b[k].max(t[k]) + e[k])));
                }
                let g = 1.0 + (self.bottom_scale - 1.0) * self.stretch_pos;
                let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                for a in &anch { for k in 0..2 { lo[k] = lo[k].min(a[k] * g); hi[k] = hi[k].max(a[k] * g); } }
                let (zs, rs) = (top_h * (1.0 - self.stretch_pos), self.leg_d * 0.25 + 0.003);
                v.push(Volume::hard([lo[0] - rs, lo[1] - rs, zs - rs], [hi[0] + rs, hi[1] + rs, zs + rs]));
            }
            1 => { let rl = self.leg_d / 2.0 + 0.003; v.push(Volume::hard([-rl, -rl, -0.002], [rl, rl, top_h + 0.002])); }
            _ => {
                let parts = wheeled_leg(self.joint_h, self.leg_d, top_h, self.wheel_arc, self.wheel_w, self.wheel_rot, self.pole_len, self.poles);
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for m in &parts { let (a, b) = m.bounds(); for k in 0..3 { lo[k] = lo[k].min(a[k]); hi[k] = hi[k].max(b[k]); } }
                v.push(Volume::hard([lo[0] - 0.003, lo[1] - 0.003, lo[2] - 0.003], [hi[0] + 0.003, hi[1] + 0.003, hi[2] + 0.003]));
            }
        }
        Some(v)
    }
    fn asset(&self, inst: u64) -> Asset {
        let mut mr = Rng::seeded(self.seed);
        let seat_m = spec_material("seat", json!({"cls": weighted(&mut mr, SEAT), "kwargs": {}, "seed": self.seed}), [0.35, 0.3, 0.3]);
        let leg_m = spec_material("leg", json!({"cls": weighted(&mut mr, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.35, 0.3, 0.3]);
        // generate_round_seats draws its remaining arguments inside the instance's FixedSeed
        let mut r = inst_rng(self.fseed, inst);
        let cap_radius = r.uniform(2.0, 3.2);
        let bevel = r.uniform(0.01, 0.04);
        let th = self.top_t;
        let top_h = self.z - th;
        let mut seat = capped_cylinder_full(th * 1.0, self.x, cap_radius, bevel / th, 0.0140, -0.0020, 128);
        seat.transform(&Mat4::translation([0.0, 0.0, top_h]));
        let mut parts: Vec<Mesh> = vec![];
        match self.style {
            0 => {
                let leg = leg_straight(&self.curve, 1.0, self.leg_d, 32, self.ngon, 0.1);
                let st = strecher(self.leg_d * 0.5);
                let anch = anchor_points(self.legs, 0.7 * self.x, 1.0);
                let m = legs_and_stretchers(&anch, top_h, self.bottom_scale, &leg, Some((&st, self.incr, self.stretch_pos)));
                let (lg, sts) = m.split_at(anch.len());
                parts.extend(sts.iter().cloned());
                parts.extend(lg.iter().cloned());
            }
            1 => {
                let leg = leg_single_stand(&self.curve, 1.0, self.leg_d, 64);
                parts.extend(legs_and_stretchers(&[[0.0, 0.0, 0.0]], top_h, self.bottom_scale, &leg, None));
            }
            _ => parts.extend(wheeled_leg(self.joint_h, self.leg_d, top_h, self.wheel_arc, self.wheel_w, self.wheel_rot, self.pole_len, self.poles)),
        }
        parts.push(seat);
        let mut a = Asset::default();
        let n = parts.len();
        for (i, m) in parts.into_iter().enumerate() {
            let b = m.bounds();
            a.mesh(m, if i + 1 == n { &seat_m } else { &leg_m }, Some(b));
        }
        a
    }
}

// ---------------------------------------------------------------------------------------------- wheeled leg primitives
/// GeometryNodeMeshUVSphere (float32 like Blender)
pub fn uv_sphere(segments: usize, rings: usize, radius: f64) -> Mesh {
    let r = radius as f32;
    let mut m = Mesh::new();
    m.pos.push([0.0, 0.0, r as f64]);
    for ri in 1..rings {
        let theta = std::f32::consts::PI * ri as f32 / rings as f32;
        let (st, ct) = theta.sin_cos();
        for k in 0..segments {
            let phi = 2.0 * std::f32::consts::PI * (k + 1) as f32 / segments as f32;
            let (sp, cp) = phi.sin_cos();
            m.pos.push([(r * st * cp) as f64, (r * st * sp) as f64, (r * ct) as f64]);
        }
    }
    m.pos.push([0.0, 0.0, -r as f64]);
    let s = segments as u32;
    for k in 0..s {
        m.faces.push(vec![0, 1 + k, 1 + (k + 1) % s]);
    }
    for ri in 0..(rings as u32 - 2) {
        let a = 1 + ri * s;
        let b = 1 + (ri + 1) * s;
        for k in 0..s {
            let k1 = (k + 1) % s;
            m.faces.push(vec![a + k, b + k, b + k1, a + k1]);
        }
    }
    let last = m.pos.len() as u32 - 1;
    let a = 1 + (rings as u32 - 2) * s;
    for k in 0..s {
        m.faces.push(vec![last, a + (k + 1) % s, a + k]);
    }
    m
}

/// keep vertices passing `keep`, drop faces touching removed vertices, preserve order
pub fn delete_verts(m: &Mesh, keep: &dyn Fn(V3) -> bool) -> Mesh {
    let mut map = vec![u32::MAX; m.pos.len()];
    let mut out = Mesh::new();
    for (i, p) in m.pos.iter().enumerate() {
        if keep(*p) {
            map[i] = out.pos.len() as u32;
            out.pos.push(*p);
        }
    }
    for f in &m.faces {
        if f.iter().all(|&i| map[i as usize] != u32::MAX) {
            out.faces.push(f.iter().map(|&i| map[i as usize]).collect());
        }
    }
    out
}

/// MeshCylinder (n-gon caps): `fill_segments` concentric inner rings on each cap
pub fn mesh_cylinder_f(vertices: usize, side_segments: usize, fill_segments: usize, radius: f64, depth: f64) -> Mesh {
    let mut m = Mesh::new();
    let n = vertices as u32;
    let f = fill_segments.max(1);
    // (radius fraction, z) per ring, top to bottom
    let mut rings: Vec<(f32, f32)> = vec![];
    let half = depth as f32 / 2.0;
    for k in 1..f {
        rings.push((k as f32 / f as f32, half));
    }
    for r in 0..=side_segments {
        rings.push((1.0, half - depth as f32 * r as f32 / side_segments as f32));
    }
    for k in (1..f).rev() {
        rings.push((k as f32 / f as f32, -half));
    }
    for (frac, z) in &rings {
        for i in 0..vertices {
            let a = 2.0 * std::f32::consts::PI * i as f32 / vertices as f32;
            let rr = radius as f32 * frac;
            m.pos.push([(rr * a.cos()) as f64, (rr * a.sin()) as f64, *z as f64]);
        }
    }
    m.faces.push((0..n).collect());
    for r in 0..(rings.len() as u32 - 1) {
        for i in 0..n {
            let i1 = (i + 1) % n;
            m.faces.push(vec![r * n + i, (r + 1) * n + i, (r + 1) * n + i1, r * n + i1]);
        }
    }
    let last = (rings.len() as u32 - 1) * n;
    m.faces.push((0..n).rev().map(|i| last + i).collect());
    m
}

pub fn mesh_cylinder(vertices: usize, side_segments: usize, radius: f64, depth: f64) -> Mesh {
    mesh_cylinder_f(vertices, side_segments, 1, radius, depth)
}

/// nodegroup_create_cap + nodegroup_capped_cylinder (defaults: flatness 4, resolution 64, vertical fillet 0.4, z offset 0)
pub fn capped_cylinder(thickness: f64, radius: f64, cap_rel_scale: f64) -> Mesh {
    capped_cylinder_full(thickness, radius, 4.0, 0.4, cap_rel_scale, 0.0, 64)
}

/// nodegroup_capped_cylinder with every input
pub fn capped_cylinder_full(thickness_in: f64, radius: f64, flat: f64, vfillet: f64, cap_rel_scale: f64, cap_z_offset: f64, res: usize) -> Mesh {
    let thickness = (thickness_in as f32 * 2.0f32) as f64; // the node group's "multiply" (Thickness * 2)
    let rings = (flat as f32 * 257.0f32) as usize;
    let sph = uv_sphere(res, rings, flat);
    let dz = -((flat as f32 * flat as f32 - 1.0f32).sqrt()) as f64;
    let mut cap = sph;
    for p in cap.pos.iter_mut() {
        p[2] = (p[2] as f32 + dz as f32) as f64;
    }
    let mut cap = delete_verts(&cap, &|p| !((p[2] as f32) < 0.0));
    let s = (radius as f32 * 0.5f32 + cap_rel_scale as f32) as f64;
    let tz = (thickness as f32 + cap_z_offset as f32) as f64;
    for p in cap.pos.iter_mut() {
        *p = [p[0] * s, p[1] * s, p[2] * s + tz];
    }
    let mut top = super::tableparts::table_top_exact(res, radius, 1.0, 0.0, thickness, vfillet);
    top.append(&cap);
    top
}

// ---------------------------------------------------------------------------------------------- generic align (AlignEulerToVector as a rotation matrix)
pub type M3 = [[f64; 3]; 3];

fn m3_mul(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                r[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    r
}
fn m3_vec(a: &M3, v: V3) -> V3 {
    [a[0][0] * v[0] + a[0][1] * v[1] + a[0][2] * v[2], a[1][0] * v[0] + a[1][1] * v[1] + a[1][2] * v[2], a[2][0] * v[0] + a[2][1] * v[1] + a[2][2] * v[2]]
}
pub fn m3_axis_angle(ax: V3, ang: f64) -> M3 {
    let (s, c) = ang.sin_cos();
    let (x, y, z) = (ax[0], ax[1], ax[2]);
    let t = 1.0 - c;
    [[t * x * x + c, t * x * y - s * z, t * x * z + s * y], [t * x * y + s * z, t * y * y + c, t * y * z - s * x], [t * x * z - s * y, t * y * z + s * x, t * z * z + c]]
}
fn unit(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0.0 { v } else { [v[0] / l, v[1] / l, v[2] / l] }
}
fn axis_vec(a: usize) -> V3 {
    let mut v = [0.0; 3];
    v[a] = 1.0;
    v
}
/// AlignEulerToVector: main `axis` (0=X,1=Y,2=Z), optional `pivot` axis, applied to `rot` (matrix). Zero vector leaves the rotation unchanged.
pub fn align_euler(rot: &M3, vector: V3, axis: usize, pivot: Option<usize>) -> M3 {
    if vector == [0.0, 0.0, 0.0] {
        return *rot;
    }
    let old_axis = m3_vec(rot, axis_vec(axis));
    match pivot {
        None => {
            let new_axis = unit(vector);
            let cr = cross(old_axis, new_axis);
            let cl = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
            let d = dot(old_axis, new_axis);
            let r = if cl < 1e-12 {
                if d > 0.0 {
                    return *rot;
                }
                // anti-parallel: half turn about z (about y when the main axis is z)
                m3_axis_angle(if axis == 2 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] }, std::f64::consts::PI)
            } else {
                m3_axis_angle([cr[0] / cl, cr[1] / cl, cr[2] / cl], cl.atan2(d))
            };
            m3_mul(&r, rot)
        }
        Some(p) => {
            let pv = m3_vec(rot, axis_vec(p));
            let d = dot(vector, pv);
            let proj = [vector[0] - pv[0] * d, vector[1] - pv[1] * d, vector[2] - pv[2] * d];
            let degenerate = (proj[0] * proj[0] + proj[1] * proj[1] + proj[2] * proj[2]).sqrt() <= 1e-12;
            let ang = if degenerate {
                std::f64::consts::FRAC_PI_3
            } else {
                let pa = unit([old_axis[0] - pv[0] * dot(old_axis, pv), old_axis[1] - pv[1] * dot(old_axis, pv), old_axis[2] - pv[2] * dot(old_axis, pv)]);
                let pt = unit(proj);
                let a = dot(pa, pt).clamp(-1.0, 1.0).acos();
                if dot(cross(pa, pt), pv) < 0.0 { -a } else { a }
            };
            m3_mul(&m3_axis_angle(pv, ang), rot)
        }
    }
}
pub const I3: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn apply_inst(m: &Mesh, pos: V3, r: &M3, scale: V3) -> Mesh {
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        let q = m3_vec(r, [p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]]);
        *p = [pos[0] + q[0], pos[1] + q[1], pos[2] + q[2]];
    }
    o
}

fn join_meshes(ms: &[&Mesh]) -> Mesh {
    let mut o = Mesh::new();
    for m in ms {
        o.append(m);
    }
    o
}

/// nodegroup_chair_wheel: (mesh component, realized instance meshes); python leaves the capped cylinders as instances until a later RealizeInstances
pub fn chair_wheel(arc_sweep_deg: f64, wheel_width: f64, wheel_rot_deg: f64, pole_width: f64, pole_aspect: f64, pole_length: f64) -> (Mesh, Vec<Mesh>) {
    use super::tableparts::*;
    let w = wheel_width;
    let mut cc = capped_cylinder(0.02, 0.5, 0.01);
    cc.transform(&Mat4::translation([0.0, -0.02, 0.0]).mul(&Mat4::euler_xyz([-1.5708, 0.0, 0.0])));
    let p0 = [0.0, w, 0.0];
    let p1 = [0.0, -w, 0.0];
    let r0 = align_euler(&I3, p0, 1, None);
    let r1 = align_euler(&I3, p1, 1, None);
    let mut insts = vec![apply_inst(&cc, p0, &r0, [1.0; 3]), apply_inst(&cc, p1, &r1, [1.0; 3])];
    let diameter = 0.5 + 0.08;
    let sweep = arc_sweep_deg;
    let start = ((sweep * 0.5 + -90.0) * -1.0).to_radians();
    let mut arc = Curve::arc(32, diameter / 2.0, start, sweep.to_radians());
    arc.transform(&Mat4::euler_xyz([1.5708, 0.0, 0.0]));
    let quad = Curve::new(vec![[w, 0.01, 0.0], [-w, 0.01, 0.0], [-w, -0.01, 0.0], [w, -0.01, 0.0]], true);
    let prof = quad.fillet_poly(0.03, 4);
    let rim = curve_to_mesh(&arc, &prof, true);
    let mut hub = mesh_cylinder_f(32, 8, 4, 0.05, 0.2);
    hub.transform(&Mat4::translation([0.22, 0.0, 0.225]));
    let mut wheel = join_meshes(&[&hub, &rim]);
    let t6 = Mat4::translation([-0.22, 0.0, 0.0]);
    wheel.transform(&t6);
    for i in insts.iter_mut() {
        i.transform(&t6);
    }
    let rc = Curve::line([1.0, 0.0, -1.0], [1.0, 0.0, 1.0]);
    let mut pole = ngon_cylinder_nodes(&rc, pole_length, 4, pole_width, pole_aspect, 0.15, 64, 32).mesh;
    pole.transform(&Mat4::euler_xyz([0.0, -1.5708, 0.0]));
    let t4 = Mat4::translation([pole_length - 0.15, 0.0, pole_width * -0.3535 - 0.3]).mul(&Mat4::euler_xyz([0.0, 0.0, wheel_rot_deg.to_radians()]));
    wheel.transform(&t4);
    for i in insts.iter_mut() {
        i.transform(&t4);
    }
    let mut all = join_meshes(&[&pole, &wheel]);
    let sc = Mat4::scale([0.15, 0.15, 0.15]);
    all.transform(&sc);
    for i in insts.iter_mut() {
        i.transform(&sc);
    }
    (all, insts)
}
