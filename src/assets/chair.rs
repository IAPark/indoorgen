//! seating/chairs/chair.py ChairFactory (and the BedFrame / mattress building blocks that depend on it): exact parameter sampling and
//! surface-exact geometry from ports of the bpy operators (bevelfull.rs, bpyops.rs, bridge.rs, subsurf.rs).
use super::bevelfull::bevel_angle;
use super::blops::bezier_curve_points;
use super::bpyops::*;
use super::subsurf::subsurf;
use super::matdraw::*;
use super::*;
use crate::materials::spec_material;
use crate::mesh::*;
use crate::rng::Rng;
use serde_json::json;

pub const FURN_LEG: Table = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0), ("wood.Wood", 1.0), ("ceramic.GlassVolume", 1.0), ("plastic.PlasticRough", 1.0), ("metal.BrushedBlackMetal", 3.0), ("wood.BlackPlywood", 3.0)];
pub const FURN_HARD: Table = &[("plastic.PlasticRough", 1.0), ("wood.Wood", 1.0)];

pub fn handles(c: &str) -> bool {
    c == "ChairFactory"
}

pub fn make(_c: &str, _rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Chair { spec: ChairSpec::new(fseed) }))
}

#[derive(Clone, Debug)]
pub struct ChairSpec {
    pub fseed: u64,
    pub width: f64,
    pub size: f64,
    pub thickness: f64,
    pub bevel_width: f64,
    pub seat_back: f64,
    pub seat_mid: f64,
    pub seat_mid_x: f64,
    pub seat_mid_z: f64,
    pub seat_front: f64,
    pub is_seat_round: bool,
    pub is_seat_subsurf: bool,
    pub leg_thickness: f64,
    pub limb_profile: f64,
    pub leg_height: f64,
    pub back_height: f64,
    pub is_leg_round: bool,
    pub leg_type: usize, // 0 vertical 1 straight 2 up-curved 3 down-curved
    pub leg_x_offset: f64,
    pub leg_y_offset: [f64; 2],
    pub back_x_offset: f64,
    pub back_y_offset: f64,
    pub has_leg_x_bar: bool,
    pub has_leg_y_bar: bool,
    pub leg_offset_bar: [f64; 2],
    pub has_arm: bool,
    pub arm_thickness: f64,
    pub arm_height: f64,
    pub arm_y: f64,
    pub arm_z: f64,
    pub arm_mid: [f64; 3],
    pub arm_profile: [f64; 2],
    pub back_thickness: f64,
    pub back_type: usize, // 0 whole 1 partial 2 horizontal-bar 3 vertical-bar
    pub bed_back: usize, // bed only: 0 coiled 1 pad 2 whole 3 horizontal-bar 4 vertical-bar
    pub back_profile: Vec<(f64, f64)>,
    pub back_vertical_cuts: i64,
    pub back_partial_scale: f64,
    pub surface_cls: &'static str,
    pub limb_cls: &'static str,
    pub panel_cls: &'static str,
}

impl ChairSpec {
    pub fn new(fseed: u64) -> ChairSpec {
        Self::new_impl(fseed, false)
    }

    /// ChairFactory.__init__ as run by BedFrameFactory (back_types is the bed table: coiled, pad, whole, horizontal-bar, vertical-bar);
    /// `bed_back` holds the bed back type, `back_type` the equivalent chair back type (coiled / pad / whole -> 0)
    pub fn new_bed(fseed: u64) -> ChairSpec {
        Self::new_impl(fseed, true)
    }

    fn new_impl(fseed: u64, bed: bool) -> ChairSpec {
        let mut r = Rng::seeded(fseed);
        let r = &mut r;
        let width = r.uniform(0.4, 0.5);
        let size = r.uniform(0.38, 0.45);
        let thickness = r.uniform(0.04, 0.08);
        let bevel_width = thickness * if r.uniform(0.0, 1.0) < 0.4 { 0.1 } else { 0.5 };
        let seat_back = if r.uniform(0.0, 1.0) < 0.75 { r.uniform(0.7, 1.0) } else { 1.0 };
        let seat_mid = r.uniform(0.7, 0.8);
        let seat_mid_x = r.uniform(seat_back + seat_mid * (1.0 - seat_back), 1.0);
        let seat_mid_z = r.uniform(0.0, 0.5);
        let seat_front = r.uniform(1.0, 1.2);
        let is_seat_round = r.uniform(0.0, 1.0) < 0.6;
        let is_seat_subsurf = r.uniform(0.0, 1.0) < 0.5;
        let leg_thickness = r.uniform(0.04, 0.06);
        let limb_profile = r.uniform(1.5, 2.5);
        let leg_height = r.uniform(0.45, 0.5);
        let back_height = r.uniform(0.4, 0.5);
        let is_leg_round = r.uniform(0.0, 1.0) < 0.5;
        let leg_type = r.choice_idx(4);
        let has_leg_x_bar = r.uniform(0.0, 1.0) < 0.6;
        let has_leg_y_bar = r.uniform(0.0, 1.0) < 0.6;
        let leg_offset_bar = [r.uniform(0.2, 0.4), r.uniform(0.6, 0.8)];
        let has_arm = r.uniform(0.0, 1.0) < 0.7;
        let arm_thickness = r.uniform(0.04, 0.06);
        let arm_height = arm_thickness * r.uniform(0.6, 1.0);
        let arm_y = r.uniform(0.8, 1.0) * size;
        let arm_z = r.uniform(0.3, 0.6) * (back_height);
        let arm_mid = [r.uniform(-0.03, 0.03), r.uniform(-0.03, 0.09), r.uniform(-0.09, 0.03)];
        let arm_profile = [r.log_uniform(0.1, 3.0), r.log_uniform(0.1, 3.0)];
        let back_thickness = r.uniform(0.04, 0.05);
        let (back_type, bed_back) = if bed {
            let b = r.choice_p(&[0.3, 0.3, 0.2, 0.1, 0.1]);
            (match b { 3 => 2, 4 => 3, _ => 0 }, b)
        } else {
            (r.choice_p(&[0.25, 0.25, 0.25, 0.25]), 0)
        };
        let back_vertical_cuts = r.randint(1, 4);
        let back_partial_scale = r.uniform(1.0, 1.4);
        // materials (same stream): limb class (+init +call draws), surface class, panel class
        let limb_cls = sample_call(r, FURN_LEG);
        let surface_cls = sample_call(r, FURN_HARD);
        let panel_cls = if r.uniform(0.0, 1.0) < 0.3 { surface_cls } else { sample_call(r, FURN_HARD) };
        // post_init reseeds
        let mut p = Rng::seeded(fseed);
        let p = &mut p;
        let (mut lx, mut ly, mut bx, mut by) = (0.0, [0.0, 0.0], 0.0, 0.0);
        if leg_type != 0 && !bed {
            lx = width * p.uniform(0.05, 0.2);
            ly = [size * p.uniform(0.05, 0.2), size * p.uniform(0.05, 0.2)];
            bx = width * p.uniform(-0.1, 0.15);
            by = size * p.uniform(0.1, 0.25);
        }
        let back_profile = match back_type {
            1 => vec![(p.uniform(0.4, 0.8), 1.0)],
            2 => {
                let n = p.randint(2, 4) as usize;
                let mut locs: Vec<f64> = (0..n).map(|_| p.uniform(1.0, 2.0)).collect();
                let mut acc = 0.0;
                for l in locs.iter_mut() {
                    acc += *l;
                    *l = acc;
                }
                let last = *locs.last().unwrap();
                let ratio = p.uniform(0.5, 0.75);
                let lowest = p.uniform(0.0, 0.4);
                let mut prev = 0.0;
                let mut out = vec![];
                for l in locs {
                    let l = l / last;
                    out.push(((prev + ratio * (l - prev)) * (1.0 - lowest) + lowest, l * (1.0 - lowest) + lowest));
                    prev = l;
                }
                out
            }
            3 => vec![(p.uniform(0.8, 0.9), 1.0)],
            _ => vec![(0.0, 1.0)],
        };
        ChairSpec { fseed, width, size, thickness, bevel_width, seat_back, seat_mid, seat_mid_x, seat_mid_z, seat_front, is_seat_round, is_seat_subsurf, leg_thickness, limb_profile, leg_height, back_height, is_leg_round, leg_type, leg_x_offset: lx, leg_y_offset: ly, back_x_offset: bx, back_y_offset: by, has_leg_x_bar, has_leg_y_bar, leg_offset_bar, has_arm, arm_thickness, arm_height, arm_y, arm_z, arm_mid, arm_profile, back_thickness, back_type, bed_back, back_profile, back_vertical_cuts, back_partial_scale, surface_cls, limb_cls, panel_cls }
    }

    /// make_seat() stages: 0 curve outline, 1 grid, 2 solidified, 3 subsurf, 4 bevel
    pub fn make_seat_stage(&self, stage: usize) -> Mesh {
        let w2 = self.width / 2.0;
        let xs = [0.0, 0.1, 1.0, self.seat_mid_x, self.seat_back, 0.0].map(|v| v * w2);
        let ys = [-self.seat_front, -self.seat_front, -1.0, -self.seat_mid, 0.0, 0.0].map(|v| v * self.size);
        let zs = [0.0, 0.0, 0.0, self.seat_mid_z, 0.0, 0.0].map(|v| v * self.thickness);
        let anchors: Vec<[f64; 3]> = (0..6).map(|i| [xs[i], ys[i], zs[i]]).collect();
        let vec_idx: Vec<usize> = if self.is_seat_round { vec![4] } else { vec![2, 4] };
        let half: Vec<V3> = bezier_curve_points(&anchors, &vec_idx).iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        // MIRROR (x) with merge: the end points lie on the plane
        let n = half.len();
        // edge loop order of BM_mesh_edgeloops_find: [P0, M1, M2, .., M(n-2), P(n-1), .., P1]
        let mut lp: Vec<V3> = vec![half[0]];
        for i in 1..n - 1 {
            lp.push([-half[i][0], half[i][1], half[i][2]]);
        }
        for i in (1..n).rev() {
            lp.push(half[i]);
        }
        if stage == 0 {
            let mut m = Mesh::new();
            m.pos = lp;
            return m;
        }
        let grid = fill_grid_loop(&lp);
        if stage == 1 {
            return grid;
        }
        let solid = solidify(&grid, self.thickness, 0.0);
        if stage == 2 {
            return solid;
        }
        let sub = subsurf(&solid, 1, !self.is_seat_subsurf);
        if stage == 3 {
            return sub;
        }
        bevel_angle(&sub, self.bevel_width, 8, 30f64.to_radians(), false).unwrap_or(sub)
    }
}

fn v3(p: [f32; 3]) -> V3 {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

impl ChairSpec {
    /// ChairFactory.make_limb(leg_ends, leg_starts): bezier from start to end (axes / scale per leg type), shifted by the leg thickness
    pub fn make_limb(&self, starts: &[V3], ends: &[V3]) -> Vec<Vec<V3>> {
        let mut out = vec![];
        for (s, e) in starts.iter().zip(ends.iter()) {
            let anchors = [[s[0], s[1], s[2]], [e[0], e[1], e[2]]];
            let pts: Vec<V3> = match self.leg_type {
                2 => super::blops::align_bezier_points(&anchors, &[Some([0.0, 0.0, 1.0]), None], &[self.limb_profile, 1.0]).iter().map(|p| v3(*p)).collect(),
                3 => super::blops::align_bezier_points(&anchors, &[None, Some([0.0, 0.0, 1.0])], &[1.0, self.limb_profile]).iter().map(|p| v3(*p)).collect(),
                _ => super::blops::align_bezier_points(&anchors, &[None, None], &[1.0, 1.0]).iter().map(|p| v3(*p)).collect(),
            };
            let sx = if s[0] < 0.0 { 1.0 } else { -1.0 };
            let sy = if s[1] < -self.size / 2.0 { 1.0 } else { -1.0 };
            let loc = [sx * self.leg_thickness / 2.0, sy * self.leg_thickness / 2.0, 0.0];
            // apply_transform in float32: the location is stored as float32 and added to the float32 vertices
            let l32 = [loc[0] as f32, loc[1] as f32, loc[2] as f32];
            out.push(pts.iter().map(|p| [(p[0] as f32 + l32[0]) as f64, (p[1] as f32 + l32[1]) as f64, (p[2] as f32 + l32[2]) as f64]).collect());
        }
        out
    }

    pub fn make_legs(&self) -> Vec<Vec<V3>> {
        let (w2, sz) = (self.width / 2.0, self.size);
        let sb = self.seat_back;
        let starts: Vec<V3> = [[-sb, 0.0], [-1.0, -1.0], [1.0, -1.0], [sb, 0.0]].iter().map(|p| [p[0] * w2, p[1] * sz, 0.0]).collect();
        let mut ends = starts.clone();
        ends[0][0] -= self.leg_x_offset;
        ends[1][0] -= self.leg_x_offset;
        ends[2][0] += self.leg_x_offset;
        ends[3][0] += self.leg_x_offset;
        ends[0][1] += self.leg_y_offset[0];
        ends[3][1] += self.leg_y_offset[0];
        ends[1][1] -= self.leg_y_offset[1];
        ends[2][1] -= self.leg_y_offset[1];
        for e in ends.iter_mut() {
            e[2] = -self.leg_height;
        }
        // make_limb(leg_ends, leg_starts) is called with (ends, starts) and iterates zip(starts, ends)
        self.make_limb(&starts, &ends)
    }

    pub fn make_backs(&self) -> Vec<Vec<V3>> {
        let sb = self.seat_back * self.width / 2.0;
        let back_starts = vec![[-sb, 0.0, 0.0], [sb, 0.0, 0.0]];
        let mut back_ends = back_starts.clone();
        back_ends[0][0] += self.back_x_offset;
        back_ends[1][0] += -self.back_x_offset;
        for e in back_ends.iter_mut() {
            e[1] = self.back_y_offset;
            e[2] = self.back_height;
        }
        // make_limb(back_starts, back_ends): inside, `leg_starts` = back_ends and `leg_ends` = back_starts
        self.make_limb(&back_ends, &back_starts)
    }
}

/// decorate.solidify(obj, axis=2, thickness) on a polyline: closed square tube with horizontal cross sections, centred on the path
pub fn square_tube(path: &[V3], t: f64) -> Mesh {
    square_tube_axis(path, t, 2)
}

/// decorate.solidify(obj, axis, thickness): closed square tube whose cross sections are perpendicular to `axis`
pub fn square_tube_axis(path: &[V3], t: f64, axis: usize) -> Mesh {
    let mut m = Mesh::new();
    let n = path.len();
    let h = t / 2.0;
    let axes: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
    // (a0, a1, axis) is a right handed frame for axis 0 and 2, left handed for axis 1
    let hand = if axis == 1 { -1.0 } else { 1.0 };
    let down = (path[n - 1][axis] - path[0][axis]) * hand < 0.0;
    let cs = [[-h, -h], [h, -h], [h, h], [-h, h]];
    for p in path {
        for c in &cs {
            let mut q = *p;
            q[axes[0]] += c[0];
            q[axes[1]] += c[1];
            m.pos.push(q);
        }
    }
    let ring = |i: usize, k: usize| (i * 4 + k) as u32;
    for i in 0..n - 1 {
        for k in 0..4 {
            let k2 = (k + 1) % 4;
            let f = vec![ring(i, k), ring(i, k2), ring(i + 1, k2), ring(i + 1, k)];
            m.faces.push(if down { f.into_iter().rev().collect() } else { f });
        }
    }
    let c0: Vec<u32> = (0..4).map(|k| ring(0, k)).collect();
    let c1: Vec<u32> = (0..4).map(|k| ring(n - 1, k)).collect();
    if down {
        m.faces.push(c0);
        m.faces.push(c1.into_iter().rev().collect());
    } else {
        m.faces.push(c0.into_iter().rev().collect());
        m.faces.push(c1);
    }
    m
}


impl ChairSpec {
    /// solidify(obj, axis, thickness) of ChairFactory: square tube + 8 segment bevel (round legs) or a geo_radius tube
    pub fn solidify_limb(&self, path: &[V3], axis: usize, thickness: f64) -> Mesh {
        if self.is_leg_round {
            let m = square_tube_axis(path, thickness, axis);
            bevel_angle(&m, self.bevel_width, 8, 30f64.to_radians(), false).unwrap_or(m)
        } else {
            super::gnhelpers::geo_radius(&[path.to_vec()], thickness / 2.0, 32, 0.004)
        }
    }

    /// make_leg_decors (x / y stretcher bars); `r` is the instance stream (create_asset)
    pub fn make_leg_decors(&self, legs: &[Vec<V3>], r: &mut Rng) -> Vec<Mesh> {
        let mut out = vec![];
        let pick = |z_height: f64| -> Vec<V3> {
            legs.iter()
                .map(|l| {
                    let mut best = l[0];
                    let mut bd = f64::INFINITY;
                    for p in l {
                        let d = (p[2] - z_height).abs();
                        if d < bd {
                            bd = d;
                            best = *p;
                        }
                    }
                    best
                })
                .collect()
        };
        let line = |a: V3, b: V3| -> Vec<V3> { super::blops::bezier_curve_points(&[[a[0], a[1], a[2]], [b[0], b[1], b[2]]], &[]).iter().map(|p| v3(*p)).collect() };
        if self.has_leg_x_bar {
            let z = -self.leg_height * r.uniform(self.leg_offset_bar[0], self.leg_offset_bar[1]);
            let l = pick(z);
            out.push(self.solidify_limb(&line(l[0], l[3]), 0, self.leg_thickness));
            out.push(self.solidify_limb(&line(l[1], l[2]), 0, self.leg_thickness));
        }
        if self.has_leg_y_bar {
            let z = -self.leg_height * r.uniform(self.leg_offset_bar[0], self.leg_offset_bar[1]);
            let l = pick(z);
            out.push(self.solidify_limb(&line(l[0], l[1]), 1, self.leg_thickness));
            out.push(self.solidify_limb(&line(l[2], l[3]), 1, self.leg_thickness));
        }
        out
    }
}


impl ChairSpec {
    /// make_arms(base, backs): `seat` is the final seat mesh, `backs` the raw back polylines
    pub fn make_arms(&self, seat: &Mesh, backs: &[Vec<V3>]) -> Vec<Mesh> {
        // end = co[argmin(co.x - (|co.y + arm_y| < 0.02))]
        let mut bi = 0;
        let mut bv = f64::INFINITY;
        for (i, p) in seat.pos.iter().enumerate() {
            let v = p[0] - if (p[1] + self.arm_y).abs() < 0.02 { 1.0 } else { 0.0 };
            if v < bv {
                bv = v;
                bi = i;
            }
        }
        let mut end = seat.pos[bi];
        end[0] += self.arm_thickness / 4.0;
        let end_ = [-end[0], end[1], end[2]];
        let mut si = 0;
        let mut sv = f64::INFINITY;
        for (i, p) in backs[0].iter().enumerate() {
            let v = p[0] - if (p[2] - self.arm_z).abs() < 0.02 { 1.0 } else { 0.0 };
            if v < sv {
                sv = v;
                si = i;
            }
        }
        let mut start = backs[0][si];
        start[0] -= self.arm_thickness / 4.0;
        let start_ = [-start[0], start[1], start[2]];
        let mut out = vec![];
        for (start, end) in [(start, end), (start_, end_)] {
            let mid = [end[0] + self.arm_mid[0] * if end[0] > 0.0 { -1.0 } else { 1.0 }, end[1] + self.arm_mid[1], start[2] + self.arm_mid[2]];
            let anchors = [[start[0], start[1], start[2]], [mid[0], mid[1], mid[2]], [end[0], end[1], end[2]]];
            let ax0 = [end[0] - start[0], end[1] - start[1], 0.0];
            let r2 = 1.0 / 2f64.sqrt();
            let pts: Vec<V3> = super::blops::align_bezier_points(&anchors, &[Some(ax0), Some([0.0, r2, r2]), Some([0.0, 0.0, 1.0])], &[1.0, self.arm_profile[0], self.arm_profile[1], 1.0]).iter().map(|p| v3(*p)).collect();
            if self.is_leg_round {
                out.push(super::gnhelpers::geo_radius_ex(&[pts], self.arm_thickness / 2.0, 32, 0.004, false));
            } else {
                // extrude_edges_move along x, then SOLIDIFY(thickness=arm_height, offset=0)
                let dx = if end[0] < 0.0 { self.arm_thickness } else { -self.arm_thickness };
                let n = pts.len();
                let mut rib = Mesh::new();
                for p in &pts {
                    rib.pos.push(*p);
                }
                for p in &pts {
                    rib.pos.push([p[0] + dx, p[1], p[2]]);
                }
                for i in 0..n - 1 {
                    rib.faces.push(vec![i as u32, (i + 1) as u32, (n + i + 1) as u32, (n + i) as u32]);
                }
                out.push(solidify(&rib, self.arm_height, 0.0));
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------ back decors (bridge_edge_loops)
type F3 = [f32; 3];
fn f3(a: V3) -> F3 {
    [a[0] as f32, a[1] as f32, a[2] as f32]
}
fn lerp32(a: F3, b: F3, t: f32) -> F3 {
    let s = 1.0f32 - t;
    [s * a[0] + t * b[0], s * a[1] + t * b[1], s * a[2] + t * b[2]]
}
fn len32(a: F3, b: F3) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
/// BM_edgeloop_calc_center (cyclic weights, float32)
fn loop_center(l: &[V3]) -> F3 {
    let n = l.len();
    let p = |k: usize| f3(l[k % n]);
    let mut co = [0.0f32; 3];
    let mut totw = 0.0f32;
    let mut w_prev = len32(p(n + n - 2), p(n - 1));
    for step in 0..n {
        let k = (n - 1 + step) % n;
        let (cur, next) = (p(k), p(k + 1));
        let w_curr = len32(cur, next);
        let w = w_curr + w_prev;
        for i in 0..3 {
            co[i] += cur[i] * w;
        }
        totw += w;
        w_prev = w_curr;
    }
    if totw != 0.0 {
        let f = 1.0f32 / totw;
        for i in 0..3 {
            co[i] *= f;
        }
    }
    co
}

/// bridge_edge_loops(number_cuts=cuts, interpolation=LINEAR, profile_shape_factor=psf) between two open chains with matching vertices
pub fn bridge_open_linear(a: &[V3], b: &[V3], cuts: usize, psf: f64) -> Mesh {
    let n = a.len();
    let resolu = cuts + 2;
    let ca = loop_center(a);
    let cb = loop_center(b);
    let falloff: Vec<f32> = (0..resolu)
        .map(|i| {
            let mut fac = i as f32 / (resolu - 1) as f32;
            fac = (1.0f32 - 2.0f32 * (0.5f32 - fac).abs()).abs();
            fac = 3.0 * fac * fac - 2.0 * fac * fac * fac;
            1.0f32 + fac * psf as f32
        })
        .collect();
    let mut m = Mesh::new();
    for k in 0..n {
        let (va, vb) = (f3(a[k]), f3(b[k]));
        for i in 0..resolu {
            let t = i as f32 / (resolu - 1) as f32;
            let mut co = lerp32(va, vb, t);
            if psf != 0.0 && i > 0 && i < resolu - 1 {
                let c = lerp32(ca, cb, t);
                co = lerp32(c, co, falloff[i]);
            }
            m.pos.push([co[0] as f64, co[1] as f64, co[2] as f64]);
        }
    }
    for k in 0..n - 1 {
        for i in 0..resolu - 1 {
            let id = |kk: usize, ii: usize| (kk * resolu + ii) as u32;
            m.faces.push(vec![id(k, i), id(k + 1, i), id(k + 1, i + 1), id(k, i + 1)]);
        }
    }
    m
}

impl ChairSpec {
    /// make_back_decors for the plain ChairFactory back types (whole / partial / horizontal-bar / vertical-bar)
    pub fn make_back_decors(&self, backs: &[Vec<V3>], r: &mut Rng) -> Vec<Mesh> {
        let smoothness = r.uniform(0.0, 1.0);
        let psf = r.uniform(0.0, 0.4);
        let _ = smoothness;
        let bt2 = self.back_thickness / 2.0;
        let shifted: Vec<Vec<V3>> = backs.iter().map(|p| p.iter().map(|q| [q[0] + if q[0] > 0.0 { bt2 } else { -bt2 }, q[1], q[2]]).collect()).collect();
        let mut grids = Mesh::new();
        for &(zmin, zmax) in &self.back_profile {
            let (lo, hi) = (zmin * self.back_height, zmax * self.back_height);
            let mut chains: Vec<(Vec<V3>, bool, bool)> = vec![];
            for p in &shifted {
                let mut idx: Vec<usize> = vec![];
                for i in 0..p.len() - 1 {
                    let cz = f64::from(((p[i][2] as f32) + (p[i + 1][2] as f32)) * 0.5f32);
                    if lo <= cz && cz <= hi {
                        idx.push(i);
                    }
                }
                if idx.is_empty() {
                    continue;
                }
                let (i0, i1) = (idx[0], idx[idx.len() - 1] + 1);
                // select_loose + delete removes the chain end vertices that continue into unselected (loose) edges
                chains.push(((i0..=i1).map(|i| p[i]).collect(), i0 > 0, i1 < p.len() - 1));
            }
            if chains.len() == 2 && chains[0].0.len() == chains[1].0.len() {
                let (c0, c1) = (&chains[0], &chains[1]);
                let g = bridge_open_linear(&c0.0, &c1.0, 64, psf);
                let resolu = 66usize;
                let n = c0.0.len();
                let mut dead = vec![false; g.pos.len()];
                let first_cut = c0.1 || c1.1;
                let last_cut = c0.2 || c1.2;
                for (flag, k) in [(first_cut, 0usize), (last_cut, n - 1)] {
                    if flag {
                        dead[k * resolu] = true;
                        dead[k * resolu + resolu - 1] = true;
                    }
                }
                let base = grids.pos.len() as u32;
                let mut map = vec![u32::MAX; g.pos.len()];
                for (i, p) in g.pos.iter().enumerate() {
                    if !dead[i] {
                        map[i] = grids.pos.len() as u32;
                        grids.pos.push(*p);
                    }
                }
                let _ = base;
                for f in g.faces {
                    if f.iter().all(|&v| !dead[v as usize]) {
                        grids.faces.push(f.iter().map(|&v| map[v as usize]).collect());
                    }
                }
            }
        }
        if std::env::var("DBGP").is_ok() { eprintln!("bridge verts {}", grids.pos.len()); }
        let mut out = vec![];
        if !grids.pos.is_empty() {
            let t = self.thickness.min(self.back_thickness);
            let s = solidify(&grids, t, 0.0);
            let mut m = bevel_angle(&s, self.bevel_width, 8, 30f64.to_radians(), false).unwrap_or(s);
            if self.back_type == 1 {
                for p in m.pos.iter_mut() {
                    p[1] *= self.back_partial_scale;
                }
            }
            out.push(m);
        }
        if self.back_type == 3 {
            // wire grid of the unshifted backs, vertical edges only, cut at the panel mid height
            let g = bridge_open_linear(&backs[0], &backs[1], self.back_vertical_cuts as usize, psf);
            let resolu = self.back_vertical_cuts as usize + 2;
            let zcut = (self.back_profile[0].0 + self.back_profile[0].1) * self.back_height / 2.0;
            let n = backs[0].len();
            let mut lines: Vec<Vec<V3>> = vec![];
            // lines index i == cut index (0 and resolu-1 are the rails)
            for i in 0..resolu {
                let line: Vec<V3> = (0..n).map(|k| g.pos[k * resolu + i]).filter(|p| p[2] >= -self.thickness / 2.0 && p[2] <= zcut).collect();
                if line.len() >= 2 {
                    lines.push(line);
                }
            }
            if self.is_leg_round {
                let mut m = Mesh::new();
                for (li, l) in lines.iter().enumerate() {
                    let mut t = square_tube_axis(l, self.back_thickness, 2);
                    // the cut lines created by bridge_edge_loops run bottom -> top: decorate.solidify then builds an inward wound box
                    if li > 0 && li + 1 < lines.len() {
                        t = t.flipped();
                    }
                    m.append(&t);
                }
                let m = if std::env::var("NOBEV").is_ok() { m } else { bevel_angle(&m, self.bevel_width, 8, 30f64.to_radians(), false).unwrap_or(m) };
                out.push(m);
            } else {
                out.push(super::gnhelpers::geo_radius(&lines, self.back_thickness / 2.0, 32, 0.004));
            }
        }
        out
    }
}


pub struct Chair {
    pub spec: ChairSpec,
}

impl ChairSpec {
    /// create_asset() parts before the final rotation: (seat, limb parts, panel parts)
    pub fn build(&self, inst: u64) -> (Vec<Mesh>, Vec<Mesh>, Vec<Mesh>) {
        let mut r = super::inst_rng(self.fseed, inst);
        let seat = self.make_seat_stage(4);
        let legs = self.make_legs();
        let backs = self.make_backs();
        let leg_decors = self.make_leg_decors(&legs, &mut r);
        let arms = if self.has_arm { self.make_arms(&seat, &backs) } else { vec![] };
        let back_decors = self.make_back_decors(&backs, &mut r);
        let mut limbs: Vec<Mesh> = vec![];
        for l in &legs {
            limbs.push(self.solidify_limb(l, 2, self.leg_thickness));
        }
        for b in &backs {
            limbs.push(self.solidify_limb(b, 2, self.back_thickness));
        }
        limbs.extend(leg_decors);
        limbs.extend(arms);
        (vec![seat], limbs, back_decors)
    }

    pub fn placeholder_bounds(&self) -> (V3, V3) {
        let ox = self.leg_x_offset.max(self.back_x_offset);
        let (x0, x1) = (-self.width / 2.0 - ox, self.width / 2.0 + ox);
        let (y0, y1) = (-self.size - self.leg_y_offset[1] - self.leg_thickness * 0.5, self.leg_y_offset[0].max(self.back_y_offset));
        // rotate +90 degrees about z: (x, y) -> (-y, x)
        ([-y1, x0, -self.leg_height], [-y0, x1, self.back_height * 1.2])
    }
}

impl Factory for Chair {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (lo, hi) = self.spec.placeholder_bounds();
        let mut a = Asset::default();
        a.cuboid(lo, hi, &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let sp = &self.spec;
        let m = |name: &str, cls: &str, k: i64, rgb: [f32; 3]| spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": sp.fseed as i64 + k}), rgb);
        let surf = m("chair_seat", sp.surface_cls, 0, [0.5, 0.35, 0.25]);
        let panel = m("chair_panel", sp.panel_cls, 1, [0.5, 0.35, 0.25]);
        let limb = m("chair_limb", sp.limb_cls, 2, [0.3, 0.3, 0.3]);
        let (seat, limbs, panels) = sp.build(i);
        let rot = Mat4::rot_z(std::f64::consts::FRAC_PI_2);
        let mut a = Asset::default();
        for (group, mt) in [(seat, &surf), (limbs, &limb), (panels, &panel)] {
            let mut joined = Mesh::new();
            for g in &group {
                joined.append(g);
            }
            if joined.is_empty() {
                continue;
            }
            joined.transform(&rot);
            let b = joined.bounds();
            a.parts.push(Part { mesh: joined, mat: mt.clone(), col: Some((b.0, b.1)) });
        }
        a
    }
}
