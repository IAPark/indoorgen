//! seating/chairs/chair.py, bedframe.py, bed.py, mattress.py, pillow.py: ChairFactory, BedFactory
use super::geomkit::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "ChairFactory" | "BedFactory" | "SofaFactory" | "ArmChairFactory")
}

const FURN_HARD: Weighted = &[("plastic.PlasticRough", 1.0), ("wood.Wood", 1.0)];
const FURN_LEG: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("wood.Wood", 1.0),
    ("plastic.PlasticRough", 1.0),
    ("metal.BrushedBlackMetal", 3.0),
    ("wood.BlackPlywood", 3.0),
];
const BEDFRAME: Weighted = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0), ("wood.Wood", 7.0), ("ceramic.Plaster", 2.0)];
const FABRICS: Weighted = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];
const BLANKET: Weighted = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0), ("fabric.SofaFabric", 3.0)];

fn spec(name: &str, cls: &str, seed: i64, rgb: [f32; 3]) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

#[derive(Clone)]
struct ChairParams {
    width: f64,
    size: f64,
    thickness: f64,
    seat_back: f64,
    seat_mid: f64,
    seat_mid_x: f64,
    seat_front: f64,
    round_seat: bool,
    leg_thickness: f64,
    leg_height: f64,
    back_height: f64,
    round_leg: bool,
    leg_type: usize,
    leg_x_offset: f64,
    leg_y_offset: [f64; 2],
    back_x_offset: f64,
    back_y_offset: f64,
    has_x_bar: bool,
    has_y_bar: bool,
    bar_offset: [f64; 2],
    has_arm: bool,
    arm_thickness: f64,
    arm_height: f64,
    arm_y: f64,
    arm_z: f64,
    back_thickness: f64,
    back_type: usize, // 0 whole 1 partial 2 horizontal-bar 3 vertical-bar
    back_profile: Vec<(f64, f64)>,
    back_partial_scale: f64,
    limb_profile: f64,
    back_vertical_cuts: usize,
}

impl ChairParams {
    fn sample(r: &mut Rng) -> ChairParams {
        let mut post = r.clone(); // post_init reseeds FixedSeed(factory_seed)
        let width = r.uniform(0.4, 0.5);
        let size = r.uniform(0.38, 0.45);
        let thickness = r.uniform(0.04, 0.08);
        let _bevel = thickness * if r.f64() < 0.4 { 0.1 } else { 0.5 };
        let seat_back = if r.f64() < 0.75 { r.uniform(0.7, 1.0) } else { 1.0 };
        let seat_mid = r.uniform(0.7, 0.8);
        let seat_mid_x = r.uniform(seat_back + seat_mid * (1.0 - seat_back), 1.0);
        let _smz = r.uniform(0.0, 0.5);
        let seat_front = r.uniform(1.0, 1.2);
        let round_seat = r.f64() < 0.6;
        let _sub = r.f64() < 0.5;
        let leg_thickness = r.uniform(0.04, 0.06);
        let limb_profile = r.uniform(1.5, 2.5);
        let leg_height = r.uniform(0.45, 0.5);
        let back_height = r.uniform(0.4, 0.5);
        let round_leg = r.f64() < 0.5;
        let leg_type = r.choice_idx(4);
        let has_x_bar = r.f64() < 0.6;
        let has_y_bar = r.f64() < 0.6;
        let bar_offset = [r.uniform(0.2, 0.4), r.uniform(0.6, 0.8)];
        let has_arm = r.f64() < 0.7;
        let arm_thickness = r.uniform(0.04, 0.06);
        let arm_height = arm_thickness * r.uniform(0.6, 1.0);
        let arm_y = r.uniform(0.8, 1.0) * size;
        let arm_z = r.uniform(0.3, 0.6) * back_height;
        for _ in 0..3 {
            r.f64(); // arm_mid
        }
        r.f64();
        r.f64(); // arm_profile log_uniform(size=2)
        let back_thickness = r.uniform(0.04, 0.05);
        let back_type = r.choice_p(&[0.25, 0.25, 0.25, 0.25]);
        let back_vertical_cuts = r.randint(1, 4) as usize;
        let back_partial_scale = r.uniform(1.0, 1.4);
        let r = &mut post;
        let (mut lx, mut ly, mut bx, mut by) = (0.0, [0.0, 0.0], 0.0, 0.0);
        if leg_type != 0 {
            lx = width * r.uniform(0.05, 0.2);
            ly = [size * r.uniform(0.05, 0.2), size * r.uniform(0.05, 0.2)];
            bx = width * r.uniform(-0.1, 0.15);
            by = size * r.uniform(0.1, 0.25);
        }
        let back_profile = match back_type {
            1 => vec![(r.uniform(0.4, 0.8), 1.0)],
            2 => {
                let n = r.randint(2, 4) as usize;
                let mut locs: Vec<f64> = (0..n).map(|_| r.uniform(1.0, 2.0)).collect();
                let mut acc = 0.0;
                for l in locs.iter_mut() {
                    acc += *l;
                    *l = acc;
                }
                let last = *locs.last().unwrap();
                let ratio = r.uniform(0.5, 0.75);
                let lowest = r.uniform(0.0, 0.4);
                let mut prev = 0.0;
                let mut out = vec![];
                for l in locs {
                    let l = l / last;
                    out.push(((prev + ratio * (l - prev)) * (1.0 - lowest) + lowest, l * (1.0 - lowest) + lowest));
                    prev = l;
                }
                out
            }
            3 => vec![(r.uniform(0.8, 0.9), 1.0)],
            _ => vec![(0.0, 1.0)],
        };
        ChairParams { width, size, thickness, seat_back, seat_mid, seat_mid_x, seat_front, round_seat, leg_thickness, leg_height, back_height, round_leg, leg_type, leg_x_offset: lx, leg_y_offset: ly, back_x_offset: bx, back_y_offset: by, has_x_bar, has_y_bar, bar_offset, has_arm, arm_thickness, arm_height, arm_y, arm_z, back_thickness, back_type, back_profile, back_partial_scale, limb_profile, back_vertical_cuts }
    }

    fn placeholder_bounds(&self) -> (V3, V3) {
        let (x0, x1) = (-self.width / 2.0 - self.leg_x_offset.max(self.back_x_offset), self.width / 2.0 + self.leg_x_offset.max(self.back_x_offset));
        let (y0, y1) = (-self.size - self.leg_y_offset[1] - self.leg_thickness * 0.5, self.leg_y_offset[0].max(self.back_y_offset));
        // rotate +90 degrees about z: (x, y) -> (-y, x)
        ([-y1, x0, -self.leg_height], [-y0, x1, self.back_height * 1.2])
    }

    /// geometry in the pre-rotation frame (seat centred on x, front toward -y); returns parts as (mesh, kind) with kind 0 seat/panel 1 limb
    fn build(&self, a: &mut Asset, surf: &Material, panel: &Material, limbm: &Material, ir: &mut Rng) {
        let p = self;
        let (w, s) = (p.width, p.size);
        // seat outline
        let xa = [0.0, 0.1, 1.0, p.seat_mid_x, p.seat_back, 0.0].map(|v| v * w / 2.0);
        let ya = [-p.seat_front, -p.seat_front, -1.0, -p.seat_mid, 0.0, 0.0].map(|v| v * s);
        let mut half: Vec<[f64; 2]> = (0..6).map(|i| [xa[i], ya[i]]).collect();
        let mut poly: Vec<[f64; 2]> = vec![];
        poly.extend(half.iter().cloned());
        half.reverse();
        for q in &half {
            if q[0].abs() > 1e-9 {
                poly.push([-q[0], q[1]]);
            }
        }
        // dedupe consecutive
        poly.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        let poly = if p.round_seat { chaikin(&poly, 2) } else { poly };
        let seat = extrude(&poly, -p.thickness / 2.0, p.thickness / 2.0);
        let sb = seat.bounds();
        a.parts.push(Part { mesh: seat, mat: surf.clone(), col: Some(sb) });
        a.supports.push(sb);
        // legs: starts at seat corners
        let starts = [[-p.seat_back * w / 2.0, 0.0], [-w / 2.0, -s], [w / 2.0, -s], [p.seat_back * w / 2.0, 0.0]];
        let mut ends = starts;
        ends[0][0] -= p.leg_x_offset;
        ends[1][0] -= p.leg_x_offset;
        ends[2][0] += p.leg_x_offset;
        ends[3][0] += p.leg_x_offset;
        ends[0][1] += p.leg_y_offset[0];
        ends[3][1] += p.leg_y_offset[0];
        ends[1][1] -= p.leg_y_offset[1];
        ends[2][1] -= p.leg_y_offset[1];
        let mut leg_pts = vec![];
        for i in 0..4 {
            let inx = if starts[i][0] < 0.0 { 1.0 } else { -1.0 };
            let iny = if starts[i][1] < -s / 2.0 { 1.0 } else { -1.0 };
            let off = [inx * p.leg_thickness / 2.0, iny * p.leg_thickness / 2.0];
            let a0 = [starts[i][0] + off[0], starts[i][1] + off[1], 0.0];
            let a1 = [ends[i][0] + off[0], ends[i][1] + off[1], -p.leg_height];
            let l = curved_limb(a0, a1, p.leg_type, p.limb_profile, p.leg_thickness / 2.0, p.round_leg);
            let lb = l.bounds();
            a.parts.push(Part { mesh: l, mat: limbm.clone(), col: Some(lb) });
            leg_pts.push((a0, a1));
        }
        let at = |i: usize, f: f64| -> V3 { let (a0, a1) = leg_pts[i]; [a0[0] + (a1[0] - a0[0]) * f, a0[1] + (a1[1] - a0[1]) * f, a0[2] + (a1[2] - a0[2]) * f] };
        if p.has_x_bar {
            let f = ir.uniform(p.bar_offset[0], p.bar_offset[1]);
            for (i, j) in [(0, 3), (1, 2)] {
                let m = limb(at(i, f), at(j, f), p.leg_thickness * 0.25, p.leg_thickness * 0.25, p.round_leg);
                a.parts.push(Part { mesh: m, mat: limbm.clone(), col: None });
            }
        }
        if p.has_y_bar {
            let f = ir.uniform(p.bar_offset[0], p.bar_offset[1]);
            for (i, j) in [(0, 1), (2, 3)] {
                let m = limb(at(i, f), at(j, f), p.leg_thickness * 0.25, p.leg_thickness * 0.25, p.round_leg);
                a.parts.push(Part { mesh: m, mat: limbm.clone(), col: None });
            }
        }
        // back posts
        let bs = [[-p.seat_back * w / 2.0, 0.0], [p.seat_back * w / 2.0, 0.0]];
        let be = [[bs[0][0] + p.back_x_offset, p.back_y_offset], [bs[1][0] - p.back_x_offset, p.back_y_offset]];
        let mut posts = vec![];
        for i in 0..2 {
            let inx = if bs[i][0] < 0.0 { 1.0 } else { -1.0 };
            let off = [inx * p.leg_thickness / 2.0, -p.leg_thickness / 2.0];
            let a0 = [bs[i][0] + off[0], bs[i][1] + off[1], 0.0];
            let a1 = [be[i][0] + off[0], be[i][1] + off[1], p.back_height];
            a.parts.push(Part { mesh: curved_limb(a1, a0, p.leg_type, p.limb_profile, p.leg_thickness / 2.0, p.round_leg), mat: limbm.clone(), col: None });
            posts.push((a0, a1));
        }
        // back panel(s): strips bridged between the two (possibly curved) post curves; posts start at the top like python's make_backs
        let curves: Vec<Vec<V3>> = posts.iter().map(|&(a0, a1)| limb_curve_pts(a1, a0, p.leg_type, p.limb_profile, 12)).collect();
        let tpan = p.thickness.min(p.back_thickness);
        let shift = |q: V3| -> V3 { [q[0] + if q[0] > 0.0 { p.back_thickness / 2.0 } else { -p.back_thickness / 2.0 }, q[1], q[2]] };
        for &(z0, z1) in &p.back_profile {
            let (zlo, zhi) = (z0 * p.back_height, z1 * p.back_height);
            let mut m = Mesh::new();
            for k in 0..12 {
                let (l0, l1) = (shift(curves[0][k]), shift(curves[0][k + 1]));
                let (r0, r1) = (shift(curves[1][k]), shift(curves[1][k + 1]));
                let zc = (l0[2] + l1[2]) / 2.0;
                if zc < zlo || zc > zhi {
                    continue;
                }
                let sc = |q: V3| -> V3 { if p.back_type == 1 { [q[0], q[1] * p.back_partial_scale, q[2]] } else { q } };
                let (l0, l1, r0, r1) = (sc(l0), sc(l1), sc(r0), sc(r1));
                let up = |q: V3, d: f64| -> V3 { [q[0], q[1] + d, q[2]] };
                for d in [tpan / 2.0, -tpan / 2.0] {
                    m.add_quad(up(l0, d), up(r0, d), up(r1, d), up(l1, d));
                }
                // top/bottom edge faces of the slab
                m.add_quad(up(l0, tpan / 2.0), up(l0, -tpan / 2.0), up(r0, -tpan / 2.0), up(r0, tpan / 2.0));
                m.add_quad(up(l1, -tpan / 2.0), up(l1, tpan / 2.0), up(r1, tpan / 2.0), up(r1, -tpan / 2.0));
            }
            if !m.is_empty() {
                let mb = m.bounds();
                a.parts.push(Part { mesh: m, mat: panel.clone(), col: Some(mb) });
            }
        }
        if p.back_type == 3 {
            // vertical-bar: slats from the seat up to the middle of the panel band
            let (z0, z1) = p.back_profile[0];
            let zmax = (z0 + z1) * p.back_height / 2.0;
            let cuts = p.back_vertical_cuts;
            for c in 1..=cuts {
                let f = c as f64 / (cuts + 1) as f64;
                let pts: Vec<V3> = (0..=12).map(|k| { let (l, r) = (shift(curves[0][k]), shift(curves[1][k])); [l[0] + (r[0] - l[0]) * f, l[1] + (r[1] - l[1]) * f, l[2]] }).filter(|q| q[2] <= zmax && q[2] >= -p.thickness / 2.0).collect();
                for w in pts.windows(2) {
                    let l = limb(w[0], w[1], p.back_thickness / 2.0, p.back_thickness / 2.0, false);
                    a.parts.push(Part { mesh: l, mat: panel.clone(), col: None });
                }
            }
        }
        if p.has_arm {
            for sx in [-1.0, 1.0] {
                let x = sx * (w / 2.0 + p.arm_thickness / 4.0);
                let start = [sx * p.seat_back * w / 2.0, p.back_y_offset, p.arm_z];
                let end = [x, -p.arm_y, p.arm_z];
                let m = limb(start, end, p.arm_thickness / 2.0, p.arm_thickness / 2.0, p.round_leg);
                a.parts.push(Part { mesh: m, mat: limbm.clone(), col: None });
                let post = limb([x, -p.arm_y, 0.0], [x, -p.arm_y, p.arm_z], p.arm_thickness / 2.0, p.arm_thickness / 2.0, p.round_leg);
                a.parts.push(Part { mesh: post, mat: limbm.clone(), col: None });
            }
        }
    }
}

pub struct Chair {
    p: ChairParams,
    seed: u64,
    fseed: u64,
}

impl Chair {
    fn new(rng: &mut Rng, fseed: u64) -> Chair {
        let mut r = rng.clone();
        let p = ChairParams::sample(&mut r);
        Chair { p, seed: r.randint(0, 1_000_000) as u64, fseed }
    }
}

impl Factory for Chair {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (lo, hi) = self.p.placeholder_bounds();
        let mut a = Asset::default();
        a.cuboid(lo, hi, &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let surf = spec("chair_seat", weighted(&mut r, FURN_HARD), self.seed as i64, [0.5, 0.35, 0.25]);
        let panel = if r.f64() < 0.3 { surf.clone() } else { spec("chair_panel", weighted(&mut r, FURN_HARD), self.seed as i64 + 1, [0.5, 0.35, 0.25]) };
        let limbm = spec("chair_limb", weighted(&mut r, FURN_LEG), self.seed as i64 + 2, [0.3, 0.3, 0.3]);
        let mut a = Asset::default();
        let mut ir = inst_rng(self.fseed, _i);
        self.p.build(&mut a, &surf, &panel, &limbm, &mut ir);
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

pub struct Bed {
    p: ChairParams,
    width: f64,
    size: f64,
    thickness: f64,
    leg_t: f64,
    leg_h: f64,
    back_h: f64,
    has_all_legs: bool,
    back_type: usize, // 0 coiled 1 pad 2 whole 3 hbar 4 vbar
    seed: u64,
    mattress_w: f64,
    mattress_s: f64,
    mattress_t: f64,
    sheet_kind: usize,
    folded: bool,
    has_cover: bool,
}

impl Bed {
    fn new(rng: &mut Rng) -> Bed {
        let mut r = rng.clone();
        let p = ChairParams::sample(&mut r); // ChairFactory.__init__ draws first
        let mut r = rng.clone(); // BedFrameFactory reseeds FixedSeed(factory_seed)
        let width = r.log_uniform(1.4, 2.4);
        let size = r.uniform(2.0, 2.4);
        let thickness = r.uniform(0.05, 0.12);
        let has_all_legs = r.f64() < 0.2;
        let leg_t = r.uniform(0.08, 0.12);
        let leg_h = r.uniform(0.2, 0.6);
        let _decor = r.f64();
        let _wrapped = r.f64();
        let back_h = r.uniform(0.5, 1.3);
        let back_type = r.choice_p(&[3.0 / 11.0, 3.0 / 11.0, 2.0 / 11.0, 1.0 / 11.0, 1.0 / 11.0]);
        let mattress_t = r.uniform(0.2, 0.35);
        let mattress_w = width * r.uniform(0.88, 0.96);
        let mattress_s = size * r.uniform(0.88, 0.96);
        let sheet_kind = r.choice_p(&[4.0 / 13.0, 4.0 / 13.0, 4.0 / 13.0, 1.0 / 13.0]);
        let folded = r.f64() < 0.5;
        let has_cover = r.f64() < 0.5;
        Bed { p, width, size, thickness, leg_t, leg_h, back_h, has_all_legs, back_type, seed: r.randint(0, 1_000_000) as u64, mattress_w, mattress_s, mattress_t, sheet_kind, folded, has_cover }
    }
}

impl Factory for Bed {
    fn placeholder_exact(&self, _i: u64) -> Option<super::cabnodes::ExactMesh> {
        Some(super::chair::rotated_bbox_exact(-self.width / 2.0, self.width / 2.0, -self.size - self.leg_t * 0.5, 0.0, -self.leg_h, self.back_h * 1.2))
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        // ChairFactory.create_placeholder with the BedFrame attributes (vertical legs => zero offsets), rotated +90deg
        let (y0, y1) = (-self.size - self.leg_t * 0.5, 0.0);
        let mut a = Asset::default();
        a.cuboid([-y1, -self.width / 2.0, -self.leg_h], [-y0, self.width / 2.0, self.back_h * 1.2], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        let _ = &self.p;
        let mut r = Rng::seeded(self.seed + i);
        let frame_cls = weighted(&mut r, BEDFRAME);
        let frame = spec("bed_frame", frame_cls, self.seed as i64, [0.45, 0.32, 0.22]);
        let fab_cls = weighted(&mut r, FABRICS);
        let (w, s, t, lt) = (self.width, self.size, self.thickness, self.leg_t);
        let mut a = Asset::default();
        // pre-rotation frame: x across, y from -size (foot) to 0 (head), seat slab centred at z=0
        a.board([-w / 2.0, -s, -t / 2.0], [w / 2.0, 0.0, t / 2.0], &frame);
        let lh = self.leg_h;
        let mut legs = vec![(-w / 2.0 + lt / 2.0, -lt / 2.0), (w / 2.0 - lt / 2.0, -lt / 2.0), (-w / 2.0 + lt / 2.0, -s + lt / 2.0), (w / 2.0 - lt / 2.0, -s + lt / 2.0)];
        if self.has_all_legs {
            legs.push((0.0, -s / 2.0));
        }
        for (x, y) in legs {
            a.cuboid([x - lt / 2.0, y - lt / 2.0, -lh], [x + lt / 2.0, y + lt / 2.0, -t / 2.0], &frame);
        }
        // headboard
        let bt = (self.thickness).min(lt);
        a.cuboid([-w / 2.0, -bt, -t / 2.0], [w / 2.0, 0.0, self.back_h], &frame);
        let _ = self.back_type;
        // mattress sits on the slab: size along y
        let (mw, ms, mt) = (self.mattress_w, self.mattress_s, self.mattress_t);
        let my1 = -0.02 - (s - ms) * 0.2;
        let mat_m = spec("mattress", "fabric.FineKnitFabric", self.seed as i64 + 3, [0.92, 0.92, 0.9]);
        a.cuboid([-mw / 2.0, my1 - ms, t / 2.0 - 0.02], [mw / 2.0, my1, t / 2.0 - 0.02 + mt], &mat_m);
        let ztop = t / 2.0 - 0.02 + mt;
        // sheet draped over the mattress: top + side flaps
        if self.sheet_kind != 3 {
            let sheet_mat = spec("sheet", fab_cls, self.seed as i64 + 4, [0.7, 0.7, 0.75]);
            let sw = mw * r.uniform(1.4, 1.6).min(1.12);
            let sl = ms * r.uniform(0.7, 1.0);
            let y1 = my1 - r.uniform(0.0, 0.15) - 0.25 * ms;
            let sag = r.uniform(0.15, 0.3);
            let sheet_t = 0.03;
            a.cuboid([-mw / 2.0 - 0.005, y1 - sl, ztop], [mw / 2.0 + 0.005, y1, ztop + sheet_t], &sheet_mat);
            a.cuboid([-sw / 2.0, y1 - sl, ztop - sag], [-mw / 2.0 - 0.005, y1, ztop + 0.01], &sheet_mat);
            a.cuboid([mw / 2.0 + 0.005, y1 - sl, ztop - sag], [sw / 2.0, y1, ztop + 0.01], &sheet_mat);
            if self.has_cover {
                let cov = spec("cover", weighted(&mut r, BLANKET), self.seed as i64 + 5, [0.6, 0.5, 0.5]);
                let cy1 = y1 - sl * 0.5;
                a.cuboid([-mw / 2.0 - 0.02, cy1 - ms * 0.35, ztop + sheet_t], [mw / 2.0 + 0.02, cy1, ztop + sheet_t + 0.04], &cov);
            }
        }
        // pillows
        let n = r.randint(2, 4);
        let pw = r.uniform(0.4, 0.7);
        for k in 0..n {
            let xc = (k as f64 - (n as f64 - 1.0) / 2.0) * (mw / n as f64) * 0.9;
            let yc = my1 - r.uniform(0.15, 0.4);
            let pillow = spec("pillow", "fabric.FineKnitFabric", self.seed as i64 + 10 + k, [0.9, 0.9, 0.92]);
            a.mesh(Mesh::ellipsoid([xc, yc, ztop + 0.08], [pw / 2.0, pw * 0.35, 0.09], 18, 8), &pillow, Some(([xc - pw / 2.0, yc - pw * 0.35, ztop], [xc + pw / 2.0, yc + pw * 0.35, ztop + 0.17])));
        }
        for part in a.parts.iter_mut() {
            let n = part.mat.name.as_str();
            if n.starts_with("mattress") || n.starts_with("sheet") || n.starts_with("cover") {
                part.mesh = part.mesh.smoothed(0.02, 2, 1);
            }
        }
        a.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    Some(match c {
        "ChairFactory" => Box::new(Chair::new(rng, fseed)),
        "BedFactory" => Box::new(Bed::new(rng)),
        "SofaFactory" => Box::new(Sofa::new(rng, false)),
        "ArmChairFactory" => Box::new(Sofa::new(rng, true)),
        _ => return None,
    })
}

const SOFA_FAB: Weighted = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];

pub struct Sofa {
    dims: [f64; 3],
    arm: [f64; 3],
    back_d: f64,
    back_z: f64,
    seat_frac: f64,
    seat_h: f64,
    base_h: f64,
    back_w: f64,
    back_angle: f64,
    arm_type: usize,
    arm_w: f64,
    foot: f64,
    count: usize,
    leg_round: bool,
    seed: u64,
}

impl Sofa {
    fn new(rng: &mut Rng, armchair: bool) -> Sofa {
        let mut r = rng.clone();
        let cls = weighted(&mut r, SOFA_FAB);
        let _ = cls;
        let clip = |r: &mut Rng, m: f64, s: f64, lo: f64, hi: f64| (m + s * r.randn()).clamp(lo, hi);
        let dims = if armchair {
            [r.uniform(0.8, 1.0), r.uniform(0.9, 1.1), r.uniform(0.69, 0.97)]
        } else {
            [r.uniform(0.95, 1.1), clip(&mut r, 1.75, 0.75, 0.9, 3.0), r.uniform(0.69, 0.97)]
        };
        let arm = [1.0, r.uniform(0.06, 0.15), r.uniform(0.5, 0.75)];
        let back_d = r.uniform(0.15, 0.25);
        let back_z = r.uniform(0.5, 0.75);
        let seat_frac = r.uniform(0.7, 1.0);
        let seat_h = r.uniform(0.15, 0.3);
        let foot = r.uniform(0.07, 0.25);
        let base_h = r.uniform(0.05, 0.09);
        let back_w = r.uniform(0.1, 0.2);
        let _margin = r.uniform(0.97, 1.0);
        let back_angle = -r.uniform(0.15, 0.5);
        let arm_type = r.choice_p(&[0.4, 0.2, 0.4]);
        let arm_w = r.uniform(0.6, 0.9);
        let _ah = r.uniform(0.7, 1.0);
        let _aa = r.uniform(0.0, 1.08);
        let _fr = r.f64();
        let count = if r.f64() > 0.2 { 1 } else { 4 };
        let _sf = r.uniform(1.3, 1.6);
        let _refl = r.f64();
        let leg_round = r.f64() > 0.5;
        Sofa { dims, arm, back_d, back_z, seat_frac, seat_h, base_h, back_w, back_angle, arm_type, arm_w, foot, count, leg_round, seed: r.randint(0, 1_000_000) as u64 }
    }
    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let fab = spec("sofa_fabric", weighted(&mut r, SOFA_FAB), self.seed as i64, [0.4, 0.4, 0.45]);
        let legm = spec("sofa_leg", "wood.Wood", self.seed as i64 + 1, [0.2, 0.15, 0.1]);
        let [d, l, h] = self.dims;
        let (x0, x1) = (-d / 2.0, d / 2.0);
        let (y0, y1) = (-l / 2.0, l / 2.0);
        let foot_h = self.foot.min(0.2) * 0.6 + 0.04;
        let mut a = Asset::default();
        let arm_w = self.arm[1] + 0.06;
        let arm_top = (self.arm[2] + foot_h).min(h);
        // base
        a.cuboid([x0, y0, foot_h], [x1, y1, foot_h + self.base_h], &fab);
        // seat cushions between the arms
        let iy0 = y0 + arm_w;
        let iy1 = y1 - arm_w;
        let bx = x0 + self.back_w;
        let seat_top = foot_h + self.base_h + self.seat_h;
        let sd = (x1 - bx) * self.seat_frac.max(0.85);
        let n = self.count.max(1);
        let cw = (iy1 - iy0) / n as f64;
        for k in 0..n {
            let ya = iy0 + k as f64 * cw + 0.004;
            let yb = iy0 + (k + 1) as f64 * cw - 0.004;
            a.mesh(rounded_box([x1 - sd, ya, foot_h + self.base_h], [x1, yb, seat_top], 0.035), &fab, Some(([x1 - sd, ya, foot_h + self.base_h], [x1, yb, seat_top])));
        }
        a.supports.push(([x1 - sd, iy0, foot_h + self.base_h], [x1, iy1, seat_top]));
        // backrest: slab reclined by back_angle, spanning the interior width
        let br_h = h - foot_h - self.base_h;
        let tilt = self.back_angle.abs() * 0.4;
        for k in 0..n {
            let ya = iy0 + k as f64 * cw + 0.004;
            let yb = iy0 + (k + 1) as f64 * cw - 0.004;
            let mut m = rounded_box([0.0, ya, 0.0], [self.back_w + 0.04, yb, br_h], 0.04);
            // shear in x with height to recline
            for v in m.pos.iter_mut() {
                v[0] += x0 + v[2] * tilt;
                v[2] += foot_h + self.base_h;
            }
            let b = m.bounds();
            a.mesh(m, &fab, Some(b));
        }
        // arms
        for (ya, yb) in [(y0, y0 + arm_w), (y1 - arm_w, y1)] {
            match self.arm_type {
                1 => {
                    let r0 = arm_w / 2.0;
                    let yc = (ya + yb) / 2.0;
                    a.cuboid([x0, ya, foot_h], [x1, yb, arm_top - r0], &fab);
                    a.mesh(limb([x0, yc, arm_top - r0], [x1, yc, arm_top - r0], r0, r0, true), &fab, Some(([x0, ya, arm_top - r0 - r0], [x1, yb, arm_top])));
                }
                2 => {
                    let m = taper_box([x0, ya, foot_h], [x1, yb, arm_top], 0.7);
                    let b = m.bounds();
                    a.mesh(m, &fab, Some(b));
                }
                _ => {
                    a.mesh(rounded_box([x0, ya, foot_h], [x1, yb, arm_top], 0.03), &fab, Some(([x0, ya, foot_h], [x1, yb, arm_top])));
                }
            }
        }
        // feet
        let fr = 0.03;
        for (x, y) in [(x0 + 0.08, y0 + 0.08), (x0 + 0.08, y1 - 0.08), (x1 - 0.08, y0 + 0.08), (x1 - 0.08, y1 - 0.08)] {
            if self.leg_round {
                a.cylinder(x, y, 0.0, foot_h, fr, &legm);
            } else {
                a.cuboid([x - fr, y - fr, 0.0], [x + fr, y + fr, foot_h], &legm);
            }
        }
        // Python applies a SUBSURF (level 1) to the whole sofa
        for part in a.parts.iter_mut() {
            if part.mat.name.starts_with("sofa_leg") {
                continue;
            }
            part.mesh = part.mesh.smoothed(0.012, 2, 1);
            part.col = part.col.or(None);
        }
        a
    }
}

/// box with softly bevelled corners: chaikin-rounded xy outline extruded, edges inset at the top
fn rounded_box(lo: V3, hi: V3, r: f64) -> Mesh {
    let r = r.min((hi[0] - lo[0]) / 2.5).min((hi[1] - lo[1]) / 2.5).min((hi[2] - lo[2]) / 2.5).max(1e-3);
    let mut m = Mesh::new();
    // three stacked layers: bottom inset, body, top inset (quad strips)
    let layers = [(lo[2], r), (lo[2] + r, 0.0), (hi[2] - r, 0.0), (hi[2], r)];
    let ring = |z: f64, i: f64| -> [V3; 4] {
        [[lo[0] + i, lo[1] + i, z], [hi[0] - i, lo[1] + i, z], [hi[0] - i, hi[1] - i, z], [lo[0] + i, hi[1] - i, z]]
    };
    let rs: Vec<[V3; 4]> = layers.iter().map(|&(z, i)| ring(z, i)).collect();
    for k in 0..3 {
        for e in 0..4 {
            let f = (e + 1) % 4;
            m.add_quad(rs[k][e], rs[k][f], rs[k + 1][f], rs[k + 1][e]);
        }
    }
    let b = rs[0];
    m.add_quad(b[0], b[3], b[2], b[1]);
    let t = rs[3];
    m.add_quad(t[0], t[1], t[2], t[3]);
    m
}

/// box whose top face is shrunk in y toward its centre by `k`
fn taper_box(lo: V3, hi: V3, k: f64) -> Mesh {
    let yc = (lo[1] + hi[1]) / 2.0;
    let hw = (hi[1] - lo[1]) / 2.0 * k;
    let b = [[lo[0], lo[1], lo[2]], [hi[0], lo[1], lo[2]], [hi[0], hi[1], lo[2]], [lo[0], hi[1], lo[2]]];
    let t = [[lo[0], yc - hw, hi[2]], [hi[0], yc - hw, hi[2]], [hi[0], yc + hw, hi[2]], [lo[0], yc + hw, hi[2]]];
    let mut m = Mesh::new();
    for e in 0..4 {
        let f = (e + 1) % 4;
        m.add_quad(b[e], b[f], t[f], t[e]);
    }
    m.add_quad(b[0], b[3], b[2], b[1]);
    m.add_quad(t[0], t[1], t[2], t[3]);
    m
}

impl Factory for Sofa {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = self.build();
        let (lo, hi) = a.bounds();
        a = Asset::default();
        a.cuboid(lo, hi, &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}

/// align_bezier limb: leg_type 2 = up-curved (vertical handle at the start), 3 = down-curved (at the end), else straight
fn limb_curve_pts(a: V3, b: V3, kind: usize, prof: f64, n: usize) -> Vec<V3> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let hl = norm3(d) * 0.3904;
    let sgn = if d[2] >= 0.0 { 1.0 } else { -1.0 };
    let (mut h1, mut h2) = ([d[0] * 0.3904, d[1] * 0.3904, d[2] * 0.3904], [-d[0] * 0.3904, -d[1] * 0.3904, -d[2] * 0.3904]);
    if kind == 2 {
        h1 = [0.0, 0.0, sgn * hl * prof];
    } else if kind == 3 {
        h2 = [0.0, 0.0, -sgn * hl * prof];
    }
    let p1 = [a[0] + h1[0], a[1] + h1[1], a[2] + h1[2]];
    let p2 = [b[0] + h2[0], b[1] + h2[1], b[2] + h2[2]];
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            [0, 1, 2].map(|k| w[0] * a[k] + w[1] * p1[k] + w[2] * p2[k] + w[3] * b[k])
        })
        .collect()
}

fn curved_limb(a: V3, b: V3, kind: usize, prof: f64, r: f64, round: bool) -> Mesh {
    if kind < 2 {
        return limb(a, b, r, r, round);
    }
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let hl = norm3(d) * 0.3904; // blender AUTO handle length for a 2-point spline
    let sgn = if d[2] >= 0.0 { 1.0 } else { -1.0 };
    let (mut h1, mut h2) = ([d[0] * 0.3904, d[1] * 0.3904, d[2] * 0.3904], [-d[0] * 0.3904, -d[1] * 0.3904, -d[2] * 0.3904]);
    if kind == 2 {
        h1 = [0.0, 0.0, sgn * hl * prof];
    } else {
        h2 = [0.0, 0.0, -sgn * hl * prof];
    }
    let p1 = [a[0] + h1[0], a[1] + h1[1], a[2] + h1[2]];
    let p2 = [b[0] + h2[0], b[1] + h2[1], b[2] + h2[2]];
    let n = 14;
    let pt = |t: f64| -> V3 {
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        [0, 1, 2].map(|k| w[0] * a[k] + w[1] * p1[k] + w[2] * p2[k] + w[3] * b[k])
    };
    let mut m = Mesh::new();
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        m.append(&limb(pt(t0), pt(t1), r, r, round));
    }
    m
}
