//! objects/elements/doors/{base,panel,lite,louver}.py: parameter sampling (exact numpy draw order, incl. material constructor draws).
use super::matdraw::*;
use crate::rng::Rng;
use crate::rooms::consts::RoomConstants;

pub const HARD: Table = &[
    ("metal.MetalBasic", 1.0),
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("metal.BrushedBlackMetal", 1.0),
    ("wood.Wood", 2.0),
    ("wood.WhitePlywood", 2.0),
    ("wood.BlackPlywood", 2.0),
    ("wood.BlondePlywood", 2.0),
    ("plastic.Plastic", 1.0),
    ("plastic.PlasticRough", 1.0),
    ("plastic.PlasticTranslucent", 1.0),
];
pub const GLASSES: Table = &[("ceramic.Glass", 1.0)];
pub const METAL_NEUTRAL: Table = &[("metal.MetalBasic", 1.0)];
pub const FRAME: Table = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("wood.Wood", 1.0),
    ("wood.WhitePlywood", 0.1),
    ("wood.BlackPlywood", 0.1),
    ("wood.BlondePlywood", 0.1),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DoorKind {
    Panel,
    GlassPanel,
    Louver,
    Lite,
}

#[derive(Clone, Debug)]
pub struct BarInfo {
    pub bar_length: f64,
    pub bar_thickness: f64,
    pub aspect: f64,
    pub height_ratio: f64,
    pub length_ratio: f64,
    pub end_length_ratio: f64,
    pub end_height_ratio: f64,
    pub z_offset: f64,
}

#[derive(Clone, Debug)]
pub struct DoorSpec {
    pub kind: DoorKind,
    pub fseed: u64,
    pub width: f64,
    pub height: f64,
    pub wall_thickness: f64,
    pub depth: f64,
    pub panel_margin: f64,
    pub bevel_width: f64,
    pub out_bevel: bool,
    pub shrink_width: f64,
    pub has_glass: bool,
    pub has_louver: bool,
    pub handle_type: usize, // 0 knob 1 lever 2 pull 3 bar 4 none
    pub frame_style: usize, // 0 single_column 1 full_frame_square 2 full_frame_dome 3 full_frame_double_door
    pub frame_width: f64,
    pub orientation_left: bool,
    pub handle_offset: f64,
    pub handle_height: f64,
    pub arc_glass: bool,
    pub bar: Option<BarInfo>,
    pub knob_radius: f64,
    pub knob_radius_mid: [f64; 7],
    pub knob_depth: f64,
    pub knob_depth_mid: [f64; 7],
    pub lever_radius: f64,
    pub lever_mid_radius: f64,
    pub lever_depth: f64,
    pub lever_mid_depth: f64,
    pub lever_length: f64,
    pub level_type: usize, // 0 wave 1 cylinder 2 bent
    pub pull_size: f64,
    pub pull_depth: f64,
    pub pull_width: f64,
    pub pull_extension: f64,
    pub to_pull_bevel: bool,
    pub pull_bevel_width: f64,
    pub pull_radius: f64,
    pub pull_type: usize, // 0 u 1 tee 2 zed
    pub is_pull_circular: bool,
    pub side_bevel: f64,
    // panel / lite / louver
    pub x_subdivisions: usize,
    pub y_subdivisions: usize,
    pub merge_glass: bool,
    pub lite_dim: [f64; 4],
    pub has_panel: bool,
    pub has_upper_panel: bool,
    pub louver_width: f64,
    pub louver_margin: f64,
    pub louver_size: f64,
    pub louver_angle: f64,
    pub surface_cls: &'static str,
    pub glass_cls: &'static str,
    pub louver_cls: &'static str,
    pub handle_cls: &'static str,
    pub bar_cls: &'static str,
}

/// numpy legacy `RandomState.binomial(n, p)` for n*p <= 30 (inversion)
pub fn binomial(r: &mut Rng, n: i64, p: f64) -> i64 {
    let (p, flip) = if p > 0.5 { (1.0 - p, true) } else { (p, false) };
    let q = 1.0 - p;
    let qn = (n as f64 * q.ln()).exp();
    let np = n as f64 * p;
    let bound = (n as f64).min(np + 10.0 * (np * q + 1.0).sqrt());
    let mut x = 0i64;
    let mut px = qn;
    let mut u = r.f64();
    while u > px {
        x += 1;
        if x as f64 > bound {
            x = 0;
            px = qn;
            u = r.f64();
        } else {
            u -= px;
            px = ((n - x + 1) as f64 * p * px) / (x as f64 * q);
        }
    }
    if flip { n - x } else { x }
}

fn metal_natural_hsv(r: &mut Rng) {
    r.choice_p(&[1.0]);
    r.uniform(-0.1, 0.1);
    r.uniform(-0.1, 0.1);
    r.log_uniform(0.5, 0.2);
}

impl DoorSpec {
    pub fn new(kind: DoorKind, fseed: u64, c: &RoomConstants) -> DoorSpec {
        // BaseDoorFactory.__init__
        let mut r = Rng::seeded(fseed);
        let r = &mut r;
        let width = c.door_width;
        let height = c.door_size;
        let depth = c.wall_thickness * r.log_uniform(0.25, 0.5);
        let panel_margin = r.log_uniform(0.08, 0.12);
        let bevel_width = r.uniform(0.005, 0.01);
        let out_bevel = r.uniform(0.0, 1.0) < 0.7;
        let shrink_width = r.log_uniform(0.005, 0.06);
        let surface_cls = sample_init(r, HARD);
        let has_glass = r.uniform(0.0, 1.0) < 0.5;
        let glass_cls = sample_init(r, GLASSES);
        let louver_cls = sample_init(r, HARD);
        let handle_cls = sample_init(r, METAL_NEUTRAL);
        let handle_type = r.choice_idx(5);
        let frame_style = r.choice_idx(4);
        let frame_width = r.uniform(0.02, 0.06);
        let orientation_left = r.choice_idx(2) == 0;
        let handle_offset = panel_margin * 0.5;
        let handle_height = height * r.uniform(0.45, 0.5);
        let arc_glass = r.choice_idx(2) == 1;
        let mut bar_cls = "metal.MetalBasic";
        let bar = if handle_type == 3 {
            let bar_length = r.uniform(0.7, 0.9) * width;
            let bar_thickness = r.uniform(0.025, 0.045) * height;
            let aspect = r.uniform(0.4, 0.6);
            let height_ratio = r.uniform(0.7, 0.9);
            let length_ratio = r.uniform(0.5, 0.8);
            let end_length_ratio = r.uniform(0.1, 0.15);
            let end_height_ratio = r.uniform(1.8, 3.0);
            let z_offset = -r.uniform(0.0, 0.1) * height;
            bar_cls = sample_init(r, METALS);
            metal_natural_hsv(r);
            Some(BarInfo { bar_length, bar_thickness, aspect, height_ratio, length_ratio, end_length_ratio, end_height_ratio, z_offset })
        } else {
            None
        };
        let knob_radius = r.uniform(0.03, 0.04);
        let base_radius = r.uniform(1.1, 1.2);
        let mid_radius = r.uniform(0.4, 0.5);
        let k5 = r.uniform(0.6, 0.8);
        let knob_radius_mid = [base_radius, base_radius, mid_radius, mid_radius, 1.0, k5, 0.0];
        let knob_depth = r.uniform(0.08, 0.1);
        let knob_depth_mid = [0.0, r.uniform(0.1, 0.15), r.uniform(0.25, 0.3), r.uniform(0.35, 0.45), r.uniform(0.6, 0.8), 1.0, 1.0 + 1e-3];
        let lever_radius = r.uniform(0.03, 0.04);
        let lever_mid_radius = r.uniform(0.01, 0.02);
        let lever_depth = r.uniform(0.05, 0.08);
        let lever_mid_depth = r.uniform(0.15, 0.25);
        let lever_length = r.log_uniform(0.15, 0.2);
        let level_type = r.choice_idx(3);
        let pull_size = r.log_uniform(0.1, 0.4);
        let pull_depth = r.uniform(0.05, 0.08);
        let pull_width = r.log_uniform(0.08, 0.15);
        let pull_extension = r.uniform(0.05, 0.15);
        let to_pull_bevel = r.uniform(0.0, 1.0) < 0.5;
        let pull_bevel_width = r.uniform(0.02, 0.04);
        let pull_radius = r.uniform(0.01, 0.02);
        let pull_type = r.choice_idx(3);
        let is_pull_circular = r.uniform(0.0, 1.0) < 0.5 || pull_type == 2;
        sample_init(r, FRAME);
        // BevelSharp(): amount, segments
        r.uniform(0.001, 0.006);
        r.uniform(0.0, 1.0);
        let side_bevel = r.log_uniform(0.005, 0.015);
        metal_hsv(r);
        let mut s = DoorSpec {
            kind, fseed, width, height, wall_thickness: c.wall_thickness, depth, panel_margin, bevel_width, out_bevel, shrink_width, has_glass, has_louver: true, handle_type, frame_style, frame_width, orientation_left, handle_offset, handle_height, arc_glass, bar,
            knob_radius, knob_radius_mid, knob_depth, knob_depth_mid, lever_radius, lever_mid_radius, lever_depth, lever_mid_depth, lever_length, level_type, pull_size, pull_depth, pull_width, pull_extension, to_pull_bevel, pull_bevel_width, pull_radius, pull_type, is_pull_circular, side_bevel,
            x_subdivisions: 1, y_subdivisions: 1, merge_glass: false, lite_dim: [0.0, 1.0, 0.0, 1.0], has_panel: true, has_upper_panel: false, louver_width: 0.0, louver_margin: 0.0, louver_size: 0.0, louver_angle: 0.0,
            surface_cls, glass_cls, louver_cls, handle_cls, bar_cls,
        };
        // subclasses (each re-seeds the stream from the start)
        if kind == DoorKind::Panel || kind == DoorKind::GlassPanel || kind == DoorKind::Louver || kind == DoorKind::Lite {
            let mut r = Rng::seeded(fseed);
            s.x_subdivisions = if r.uniform(0.0, 1.0) < 0.5 { 1 } else { 2 };
            s.y_subdivisions = binomial(&mut r, 5, 0.45).max(1) as usize;
        }
        match kind {
            DoorKind::GlassPanel => {
                let mut r = Rng::seeded(fseed);
                s.x_subdivisions = 2;
                s.y_subdivisions = binomial(&mut r, 5, 0.5).max(2) as usize;
                s.merge_glass = s.y_subdivisions < 4;
                s.has_glass = true;
            }
            DoorKind::Louver => {
                let mut r = Rng::seeded(fseed);
                s.x_subdivisions = 1;
                s.y_subdivisions = binomial(&mut r, 5, 0.4).max(1) as usize;
                s.has_panel = r.uniform(0.0, 1.0) < 0.7;
                s.has_upper_panel = r.uniform(0.0, 1.0) < 0.5;
                s.louver_width = r.uniform(0.002, 0.004);
                s.louver_margin = r.uniform(0.02, 0.03);
                s.louver_size = r.log_uniform(0.05, 0.1);
                s.louver_angle = r.uniform(std::f64::consts::PI / 4.5, std::f64::consts::PI / 3.5);
                s.has_louver = true;
            }
            DoorKind::Lite => {
                let mut r = Rng::seeded(fseed);
                let u = r.uniform(0.0, 1.0);
                let mut subdivide_glass = false;
                let dim;
                if u <= 1.0 / 6.0 {
                    dim = [0.0, 1.0, r.uniform(0.4, 0.6), 1.0];
                    subdivide_glass = true;
                } else if u <= 1.0 / 3.0 {
                    dim = [0.0, 1.0, 0.0, 1.0];
                    subdivide_glass = true;
                } else if u <= 0.5 {
                    let a = r.uniform(0.3, 0.4);
                    dim = [0.0, a, r.uniform(0.4, 0.6), 1.0];
                } else if u <= 2.0 / 3.0 {
                    let a = r.uniform(0.3, 0.4);
                    dim = [0.0, a, r.uniform(0.4, 0.6), 1.0];
                } else if u <= 5.0 / 6.0 {
                    dim = [0.0, 1.0, 0.0, 1.0];
                } else {
                    let x = r.uniform(0.3, 0.35);
                    dim = [x, 1.0 - x, r.uniform(0.7, 0.8), 1.0];
                }
                s.lite_dim = dim;
                if subdivide_glass {
                    s.x_subdivisions = [1usize, 3][r.choice_idx(2)];
                    let base = (s.height / s.width * s.x_subdivisions as f64) as i64;
                    s.y_subdivisions = (base + r.randint(-1, 2)).max(0) as usize;
                } else {
                    s.x_subdivisions = 1;
                    s.y_subdivisions = 1;
                }
                s.has_glass = true;
            }
            _ => {}
        }
        s
    }
}

// ====================================================================================== geometry
use super::solid::Solid;
use crate::mesh::*;
use crate::dhash::HashMap;

#[derive(Clone, Debug)]
pub struct Panel {
    pub dim: [f64; 4], // x_min, x_max, y_min, y_max (door-plane coordinates)
    pub louver: bool,
    pub glass: bool,
}

impl DoorSpec {
    /// make_panels() including its np.random draws (instance stream `r`)
    pub fn make_panels(&self, r: &mut Rng) -> Vec<Panel> {
        let mut panels = vec![];
        let (w, h, m) = (self.width, self.height, self.panel_margin);
        if self.kind == DoorKind::Lite {
            let lin = |a: f64, b: f64, n: usize| -> Vec<f64> { (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect() };
            let xr: Vec<f64> = lin(self.lite_dim[0], self.lite_dim[1], self.x_subdivisions).iter().map(|v| v * (w - m * 2.0) + m).collect();
            let yr: Vec<f64> = lin(self.lite_dim[2], self.lite_dim[3], self.y_subdivisions).iter().map(|v| v * (h - m * 2.0) + m).collect();
            for i in 0..xr.len() - 1 {
                for j in 0..yr.len() - 1 {
                    panels.push(Panel { dim: [xr[i], xr[i + 1], yr[j], yr[j + 1]], louver: false, glass: true });
                }
            }
            return panels;
        }
        let xc: Vec<i64> = (0..self.x_subdivisions).map(|_| r.randint(1, 4)).collect();
        let xs: f64 = xc.iter().sum::<i64>() as f64;
        let mut acc = 0.0;
        let x_cuts: Vec<f64> = xc.iter().map(|&v| { acc += v as f64 / xs; acc }).collect();
        let mut yc: Vec<i64> = (0..self.y_subdivisions).map(|_| r.randint(2, 5)).collect();
        yc.sort();
        yc.reverse();
        let ys: f64 = yc.iter().sum::<i64>() as f64;
        let mut acc = 0.0;
        let y_cuts: Vec<f64> = yc.iter().map(|&v| { acc += v as f64 / ys; acc }).collect();
        for j in 0..y_cuts.len() {
            for i in 0..x_cuts.len() {
                let x_min = m + (w - m) * if i > 0 { x_cuts[i - 1] } else { 0.0 };
                let x_max = (w - m) * x_cuts[i];
                let y_min = m + (h - m) * if j > 0 { y_cuts[j - 1] } else { 0.0 };
                let y_max = (h - m) * y_cuts[j];
                panels.push(Panel { dim: [x_min, x_max, y_min, y_max], louver: false, glass: false });
            }
        }
        match self.kind {
            DoorKind::GlassPanel => {
                if self.merge_glass {
                    let first = panels[panels.len() - self.x_subdivisions].dim;
                    let last = panels.last().unwrap().dim;
                    let merged = Panel { dim: [first[0], last[1], first[2], last[3]], louver: false, glass: true };
                    let mut v = vec![merged];
                    v.extend(panels[..self.x_subdivisions].iter().cloned());
                    return v;
                }
                let n = panels.len();
                for p in panels[n - self.x_subdivisions..].iter_mut() {
                    p.glass = true;
                }
            }
            DoorKind::Louver => {
                let n = panels.len();
                if n == 1 {
                    panels[0].louver = true;
                } else if n == 2 {
                    if !self.has_panel {
                        panels[0].louver = true;
                    }
                    panels[1].louver = true;
                } else {
                    let mut v = if self.has_upper_panel { vec![panels[0].clone(), panels[n - 1].clone()] } else { vec![panels[0].clone()] };
                    for p in v.iter_mut() {
                        p.louver = true;
                    }
                    return v;
                }
            }
            _ => {}
        }
        panels
    }
}

fn q(s: &mut Solid, vm: &mut HashMap<[i64; 3], u32>, pts: &[V3]) -> Vec<u32> {
    pts.iter().map(|p| s.add_vertex(*p, vm)).collect()
}

fn orient(s: &Solid, mut f: Vec<Vec<u32>>, expect: V3) -> Vec<Vec<u32>> {
    let n = s.loop_normal(&f[0]);
    if n[0] * expect[0] + n[1] * expect[1] + n[2] * expect[2] < 0.0 {
        for l in f.iter_mut() {
            l.reverse();
        }
    }
    // holes must run opposite to the outer loop
    f
}

/// pre-bevel door body in slab coordinates: x in [0,w], y in [0,d] (y=0 front), z in [0,h]; raised panels at +-bw/2
pub fn door_slab(spec: &DoorSpec, panels: &[Panel]) -> Solid {
    let (w, d, h) = (spec.width, spec.depth, spec.height);
    let bw = spec.bevel_width;
    let t = spec.shrink_width;
    let mut s = Solid::default();
    let mut vm: HashMap<[i64; 3], u32> = HashMap::default();
    // per side: outer face with pocket holes, then pocket walls etc.
    for side in 0..2 {
        let (ys, out) = if side == 0 { (0.0, -1.0) } else { (d, 1.0) };
        let yfloor = ys - out * bw / 2.0;
        let yraise = ys + out * bw / 2.0;
        let rect = |y: f64, x0: f64, x1: f64, z0: f64, z1: f64| -> Vec<V3> { vec![[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]] };
        let outer = q(&mut s, &mut vm, &rect(ys, 0.0, w, 0.0, h));
        let mut loops = vec![outer];
        let mut pocket_faces: Vec<(Vec<Vec<u32>>, V3)> = vec![];
        let mut pocket_tag: Vec<u8> = vec![];
        for p in panels.iter().filter(|p| p.louver) {
            // louver cutter: through hole [x_min-m, x_max+m] x [y_min-m, y_max+m] (the ring that fills it is a separate shell)
            let m = spec.louver_margin;
            let [x0, x1, z0, z1] = p.dim;
            let (hx0, hx1, hz0, hz1) = (x0 - m, x1 + m, z0 - m, z1 + m);
            loops.push(q(&mut s, &mut vm, &rect(ys, hx0, hx1, hz0, hz1)));
            let tv: Vec<u32> = q(&mut s, &mut vm, &rect(ys, hx0, hx1, hz0, hz1));
            let bv: Vec<u32> = q(&mut s, &mut vm, &rect(if side == 0 { d } else { 0.0 }, hx0, hx1, hz0, hz1));
            let exp = [[0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]];
            if side == 0 {
                for k in 0..4 {
                    let k2 = (k + 1) % 4;
                    pocket_faces.push((vec![vec![tv[k], tv[k2], bv[k2], bv[k]]], exp[k]));
                    pocket_tag.push(0);
                }
            }
        }
        for p in panels.iter().filter(|p| !p.louver) {
            let tag_here: u8 = if p.glass { 1 } else { 0 };
            let [x0, x1, z0, z1] = p.dim;
            let (rx0, rx1, rz0, rz1) = (x0 + 2e-3, x1 - 2e-3, z0 + 2e-3, z1 - 2e-3);
            loops.push(q(&mut s, &mut vm, &rect(ys, rx0, rx1, rz0, rz1)));
            // walls
            let top = rect(ys, rx0, rx1, rz0, rz1);
            let bot = rect(yfloor, rx0, rx1, rz0, rz1);
            let tv: Vec<u32> = q(&mut s, &mut vm, &top);
            let bv: Vec<u32> = q(&mut s, &mut vm, &bot);
            let inward = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]];
            // rect order: (x0,z0),(x1,z0),(x1,z1),(x0,z1): edges 0:(z0 side) 1:(x1 side) 2:(z1 side) 3:(x0 side)
            let exp = [[0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]];
            let _ = inward;
            for k in 0..4 {
                let k2 = (k + 1) % 4;
                pocket_faces.push((vec![vec![tv[k], tv[k2], bv[k2], bv[k]]], exp[k]));
            }
            // floor ring (flat) with hole R1, slope ring, raised face
            let area = (rx1 - rx0) * (rz1 - rz0);
            if area > 0.01 {
                let (a0, a1, b0, b1) = (rx0 + t, rx1 - t, rz0 + t, rz1 - t);
                let r1 = q(&mut s, &mut vm, &rect(yfloor, a0, a1, b0, b1));
                pocket_faces.push((vec![bv.clone(), r1.clone()], [0.0, out, 0.0]));
                let (c0, c1, e0, e1) = (a0 + bw, a1 - bw, b0 + bw, b1 - bw);
                let r2 = q(&mut s, &mut vm, &rect(yraise, c0, c1, e0, e1));
                let uin = [[0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]];
                for k in 0..4 {
                    let k2 = (k + 1) % 4;
                    let u = uin[k];
                    let n = [-u[0], out, -u[2]];
                    pocket_faces.push((vec![vec![r1[k], r1[k2], r2[k2], r2[k]]], n));
                }
                pocket_faces.push((vec![r2.clone()], [0.0, out, 0.0]));
            } else {
                pocket_faces.push((vec![bv.clone()], [0.0, out, 0.0]));
            }
            pocket_tag.resize(pocket_faces.len(), tag_here);
        }
        let f = orient(&s, loops, [0.0, out, 0.0]);
        // orient: fix holes direction (opposite to outer)
        let mut f = f;
        for i in 1..f.len() {
            let nh = s.loop_normal(&f[i]);
            let no = s.loop_normal(&f[0]);
            if nh[0] * no[0] + nh[1] * no[1] + nh[2] * no[2] > 0.0 {
                f[i].reverse();
            }
        }
        s.faces.push(f);
        s.tags.push(0);
        for ((pf, n), tg) in pocket_faces.into_iter().zip(pocket_tag.into_iter()) {
            let mut f = orient(&s, pf, n);
            for i in 1..f.len() {
                let nh = s.loop_normal(&f[i]);
                let no = s.loop_normal(&f[0]);
                if nh[0] * no[0] + nh[1] * no[1] + nh[2] * no[2] > 0.0 {
                    f[i].reverse();
                }
            }
            s.faces.push(f);
            s.tags.push(tg);
        }
    }
    // slab sides
    let sides: [([V3; 4], V3); 4] = [
        ([[0.0, 0.0, 0.0], [0.0, d, 0.0], [0.0, d, h], [0.0, 0.0, h]], [-1.0, 0.0, 0.0]),
        ([[w, 0.0, 0.0], [w, d, 0.0], [w, d, h], [w, 0.0, h]], [1.0, 0.0, 0.0]),
        ([[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, d, 0.0], [0.0, d, 0.0]], [0.0, 0.0, -1.0]),
        ([[0.0, 0.0, h], [w, 0.0, h], [w, d, h], [0.0, d, h]], [0.0, 0.0, 1.0]),
    ];
    for (pts, n) in sides {
        let l = q(&mut s, &mut vm, &pts);
        let f = orient(&s, vec![l], n);
        s.faces.push(f);
        s.tags.push(0);
    }
    s
}

/// draws made while building the handle (before the door body): lever shape parameters, then the handle material's shader draws
#[derive(Clone, Debug, Default)]
pub struct HandleDraws {
    pub wave: [f64; 2],
    pub bent: f64,
}

impl DoorSpec {
    pub fn handle_draws(&self, r: &mut Rng) -> HandleDraws {
        let mut h = HandleDraws::default();
        if self.handle_type == 1 {
            match self.level_type {
                0 => {
                    h.wave = [r.uniform(0.2, 0.3), r.uniform(0.1, 0.15)];
                }
                2 => {
                    h.bent = r.uniform(0.2, 0.3);
                }
                _ => {}
            }
        }
        if self.handle_type <= 2 {
            // MetalBasic.apply(): shader_metal draws roughness first, then the colour (metal_hsv), then the specular level
            r.uniform(10.0, 25.0);
            r.uniform(0.0, 0.2);
            r.uniform(0.4, 0.7);
            metal_hsv(r);
            r.uniform(0.5, 1.0);
        }
        h
    }
}

// ---------------------------------------------------------------------------------- frame / arc / assembly
use super::curvekit::*;

fn weld_solid(m: &Mesh) -> Solid {
    Solid::from_mesh(&m.welded(1e-9))
}

fn chamfer_mesh(m: &Mesh) -> Mesh {
    weld_solid(m).chamfer_sharp(0.001, 70f64.to_radians()).to_mesh()
}

/// nodegroup_door_frame_warper (before the bevel); returns the mesh in the door-frame object's coordinates
pub fn door_frame_mesh(full_frame: bool, top_dome: bool, door_width: f64, door_height: f64, door_depth: f64, fw: f64) -> Mesh {
    let hh = if full_frame { fw + door_height } else { door_height };
    let mut m;
    if !full_frame {
        // single post: profile (width fw, height depth) along z
        let prof = Curve::new(vec![[fw / 2.0, door_depth / 2.0, 0.0], [-fw / 2.0, door_depth / 2.0, 0.0], [-fw / 2.0, -door_depth / 2.0, 0.0], [fw / 2.0, -door_depth / 2.0, 0.0]], true);
        let line = Curve::line([0.0, 0.0, 0.0], [0.0, 0.0, hh]);
        m = curve_to_mesh(&line, &prof, true);
    } else {
        let prof = Curve::new(vec![[door_depth / 2.0, fw * 1.4142 / 2.0, 0.0], [-door_depth / 2.0, fw * 1.4142 / 2.0, 0.0], [-door_depth / 2.0, -fw * 1.4142 / 2.0, 0.0], [door_depth / 2.0, -fw * 1.4142 / 2.0, 0.0]], true);
        let wtot = fw + door_width;
        let mut pts: Vec<V3> = vec![[0.0, 0.0, hh]];
        if top_dome {
            // arc (16 points, radius wtot/2) from the top right corner over the top to the top left corner
            let r = wtot * 0.5;
            let arc = Curve::arc(16, r, 1.5708, 3.1416);
            let mut a = arc;
            a.transform(&Mat4::translation([-r, 0.0, hh]).mul(&Mat4::euler_xyz([1.5708, 1.5708, 0.0])));
            pts.clear();
            pts.extend(a.pts.iter().cloned());
        } else {
            pts.push([-wtot, 0.0, hh]);
        }
        pts.push([-wtot, 0.0, 0.0]);
        pts.push([0.0, 0.0, 0.0]);
        let mut path = Curve::new(pts, true);
        path.tilt = vec![0.0; path.pts.len()];
        m = curve_to_mesh(&path, &prof, false);
    }
    for p in m.pos.iter_mut() {
        *p = [p[0] + fw, p[1], p[2] - fw];
    }
    m
}

pub fn frame_center_offsets(style: usize, width: f64, fw: f64) -> (f64, f64) {
    // (center_x_offset, center_z_offset) as in BaseDoorFactory._create_asset
    match style {
        0 => (-0.5 * fw, 0.0),
        1 => (0.5 * width, 0.0),
        2 => (0.5 * width, -0.25 * width - fw / 4.0),
        _ => (width, 0.0),
    }
}

fn bbox_center(m: &Mesh) -> V3 {
    let (lo, hi) = m.bounds();
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]
}

fn translate(m: &mut Mesh, t: V3) {
    for p in m.pos.iter_mut() {
        *p = [p[0] + t[0], p[1] + t[1], p[2] + t[2]];
    }
}

fn mirror_x(m: &Mesh) -> Mesh {
    let mut o = m.clone();
    for p in o.pos.iter_mut() {
        p[0] = -p[0];
    }
    o.flipped()
}

fn push_face(s: &mut Solid, mut loops: Vec<Vec<u32>>, expect: V3) {
    let n = s.loop_normal(&loops[0]);
    if n[0] * expect[0] + n[1] * expect[1] + n[2] * expect[2] < 0.0 {
        for l in loops.iter_mut() {
            l.reverse();
        }
    }
    for i in 1..loops.len() {
        let nh = s.loop_normal(&loops[i]);
        let no = s.loop_normal(&loops[0]);
        if nh[0] * no[0] + nh[1] * no[1] + nh[2] * no[2] > 0.0 {
            loops[i].reverse();
        }
    }
    s.faces.push(loops);
    s.tags.push(0);
}

/// LouverDoorFactory.louver extras (in slab coordinates, chamfered): the ring that refills the cut hole + the slanted blades
pub fn louver_meshes(spec: &DoorSpec, panels: &[Panel]) -> Mesh {
    let d = spec.depth;
    let lw = spec.louver_width;
    let m = spec.louver_margin;
    let mut out = Mesh::new();
    let ch = |s: &Solid| s.chamfer_sharp(0.001, 70f64.to_radians()).to_mesh();
    for p in panels.iter().filter(|p| p.louver) {
        let [x0, x1, z0, z1] = p.dim;
        // ring
        {
            let mut s = Solid::default();
            let mut vm: HashMap<[i64; 3], u32> = HashMap::default();
            let (ya, yb) = (-lw, d + lw);
            let (bx0, bx1, bz0, bz1) = (x0 - m, x1 + m, z0 - m, z1 + m);
            let rect = |x0: f64, x1: f64, z0: f64, z1: f64, y: f64| -> Vec<V3> { vec![[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]] };
            let of = q(&mut s, &mut vm, &rect(bx0, bx1, bz0, bz1, ya));
            let ob = q(&mut s, &mut vm, &rect(bx0, bx1, bz0, bz1, yb));
            let hf = q(&mut s, &mut vm, &rect(x0, x1, z0, z1, ya));
            let hb = q(&mut s, &mut vm, &rect(x0, x1, z0, z1, yb));
            let out_n = [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]];
            for k in 0..4 {
                let k2 = (k + 1) % 4;
                push_face(&mut s, vec![vec![of[k], of[k2], ob[k2], ob[k]]], out_n[k]);
                let n = out_n[k];
                push_face(&mut s, vec![vec![hf[k], hf[k2], hb[k2], hb[k]]], [-n[0], -n[1], -n[2]]);
            }
            push_face(&mut s, vec![of.clone(), hf.clone()], [0.0, -1.0, 0.0]);
            push_face(&mut s, vec![ob.clone(), hb.clone()], [0.0, 1.0, 0.0]);
            out.append(&ch(&s));
        }
        // blades
        let t = spec.louver_angle.tan();
        let count = (((z1 - z0) / spec.louver_size).ceil() + 0.5) as i64;
        let nrm = (1.0 + t * t).sqrt();
        let (ny, nz) = (-t / nrm, 1.0 / nrm);
        let h = lw / 2.0;
        for k in 0..count {
            let zc0 = z0 - d * t / 2.0 + k as f64 * spec.louver_size;
            let zc1 = zc0 + d * t;
            let mut poly: Vec<(f64, f64)> = vec![(0.0 + ny * h, zc0 + nz * h), (d + ny * h, zc1 + nz * h), (d - ny * h, zc1 - nz * h), (0.0 - ny * h, zc0 - nz * h)];
            // clip z >= z0 then z <= z1
            for (lo, bound) in [(true, z0), (false, z1)] {
                let inside = |p: &(f64, f64)| if lo { p.1 >= bound } else { p.1 <= bound };
                let mut np = vec![];
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    let (ia, ib) = (inside(&a), inside(&b));
                    if ia {
                        np.push(a);
                    }
                    if ia != ib {
                        let tt = (bound - a.1) / (b.1 - a.1);
                        np.push((a.0 + (b.0 - a.0) * tt, bound));
                    }
                }
                poly = np;
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() < 3 {
                continue;
            }
            let area: f64 = (0..poly.len()).map(|i| { let (a, b) = (poly[i], poly[(i + 1) % poly.len()]); a.0 * b.1 - b.0 * a.1 }).sum::<f64>() / 2.0;
            if area.abs() < 1e-12 {
                continue;
            }
            let sgn = if area > 0.0 { 1.0 } else { -1.0 };
            let mut s = Solid::default();
            let mut vm: HashMap<[i64; 3], u32> = HashMap::default();
            let l0: Vec<u32> = poly.iter().map(|&(y, z)| s.add_vertex([x0, y, z], &mut vm)).collect();
            let l1: Vec<u32> = poly.iter().map(|&(y, z)| s.add_vertex([x1, y, z], &mut vm)).collect();
            push_face(&mut s, vec![l0.clone()], [-1.0, 0.0, 0.0]);
            push_face(&mut s, vec![l1.clone()], [1.0, 0.0, 0.0]);
            for i in 0..poly.len() {
                let j = (i + 1) % poly.len();
                let (dy, dz) = (poly[j].0 - poly[i].0, poly[j].1 - poly[i].1);
                // outward normal of a CCW (y right, z up) polygon edge: (dz, -dy)
                push_face(&mut s, vec![vec![l0[i], l0[j], l1[j], l1[i]]], [0.0, sgn * dz, -sgn * dy]);
            }
            out.append(&ch(&s));
        }
    }
    out
}

pub struct DoorParts {
    pub frame: Mesh,
    pub door: Mesh,
    pub glass: Mesh,
    pub louver: Mesh,
    pub handles: Vec<Mesh>,
    pub arc: Mesh,
}

pub struct DoorPieces {
    pub body: Mesh,
    pub glass: Mesh,
    pub louver: Mesh,
    pub handle: Mesh,
}

impl DoorSpec {
    pub fn build_parts(&self, r: &mut Rng) -> DoorParts {
        let hd = self.handle_draws(r);
        let panels = self.make_panels(r);
        // surface.apply([...]) (+ glass / louver applies) run the shader functions once each, consuming draws
        apply_draws(r, self.surface_cls);
        if self.frame_style == 2 && self.arc_glass {
            apply_draws(r, "ceramic.Glass");
        }
        if self.has_glass {
            apply_draws(r, "ceramic.Glass");
        }
        if self.has_louver {
            apply_draws(r, self.louver_cls);
        }
        let ncalls = if self.frame_style == 3 { 2 } else { 1 };
        let us: Vec<f64> = if self.handle_type <= 1 { (0..ncalls).map(|_| r.uniform(0.42, 0.45)).collect() } else { vec![] };
        let slab = door_slab(self, &panels);
        let sh = [-self.width, -self.depth / 2.0, -self.height / 2.0];
        let mut door = Mesh::new();
        let mut glass = Mesh::new();
        for (t, mut m) in slab.chamfer_sharp(0.001, 70f64.to_radians()).split_by_tag() {
            translate(&mut m, sh);
            if t == 1 { glass.append(&m) } else { door.append(&m) }
        }
        let mut louver = louver_meshes(self, &panels);
        translate(&mut louver, sh);
        let (full, dome, fwid) = match self.frame_style {
            0 => (false, false, self.width),
            1 => (true, false, self.width),
            2 => (true, true, self.width),
            _ => (true, false, 2.0 * self.width),
        };
        let frame = chamfer_mesh(&door_frame_mesh(full, dome, fwid, self.height, self.depth, self.frame_width));
        let arc = if self.frame_style == 2 {
            // nodegroup_arc_on_door: half-disc prism (32-pt arc + centre) of depth 0.8 d, standing on z = h/2
            let r0 = self.width / 2.0;
            let arc = Curve::arc(32, r0, 0.0, 3.1416);
            let mut pts = arc.pts.clone();
            pts.push([0.0, 0.0, 0.0]);
            let prof = Curve::new(pts, true);
            let dd = self.depth * 0.8;
            let mut m = curve_to_mesh(&Curve::line([0.0, 0.0, -dd / 2.0], [0.0, 0.0, dd / 2.0]), &prof, true);
            let rot = Mat4::euler_xyz([1.5708, 0.0, 0.0]);
            for p in m.pos.iter_mut() {
                *p = rot.apply(*p);
            }
            translate(&mut m, [-self.width / 2.0, 0.0, self.height / 2.0]);
            chamfer_mesh(&m)
        } else {
            let mut c = Mesh::cuboid([-0.0005, -0.0005, -0.0005], [0.0005, 0.0005, 0.0005]).welded(1e-9);
            translate(&mut c, [-self.width / 2.0, 0.0, 0.0]);
            c
        };
        // handles (door-object coordinates)
        let mut all = door.clone();
        all.append(&glass);
        all.append(&louver);
        let c = if all.is_empty() { [0.0; 3] } else { bbox_center(&all) };
        let mut handles = vec![];
        match self.handle_type {
            0 | 1 => {
                if let Some(h) = super::doorhandles::handle_geometry(self, &hd) {
                    for u in &us {
                        let mut m = h.clone();
                        translate(&mut m, [c[0] - self.width * u, c[1], c[2]]);
                        handles.push(m);
                    }
                }
            }
            2 => {
                if let Some(h) = super::doorhandles::handle_geometry(self, &hd) {
                    handles.push(h);
                }
            }
            3 => handles.push(super::doorhandles::bar_mesh(self)),
            _ => {}
        }
        DoorParts { frame, door, glass, louver, handles, arc }
    }

    /// full single door: frame + hinged door (+ arc), split into material pieces
    pub fn assemble_pieces(&self, parts: &DoorParts) -> DoorPieces {
        let w = self.width;
        let d = self.depth;
        let (_, cz) = frame_center_offsets(self.frame_style, w, self.frame_width);
        let double = self.frame_style == 3;
        let flip = !double && self.orientation_left;
        let mut frame = parts.frame.clone();
        if flip && self.frame_style == 0 {
            translate(&mut frame, [-w, 0.0, 0.0]);
        }
        let fc = bbox_center(&frame);
        let vx = if flip {
            if double { -w } else if self.frame_style == 0 { 0.0 } else { -w / 2.0 }
        } else if double { w } else if self.frame_style == 0 { 0.0 } else { w / 2.0 };
        let make = |m: &Mesh, flip_lr: bool, extra: V3| -> Mesh {
            let mut m = if flip_lr { mirror_x(m) } else { m.clone() };
            translate(&mut m, [0.0, d / 2.0, cz]);
            translate(&mut m, extra);
            m
        };
        let e1 = [fc[0] + vx, fc[1] - d / 2.0, fc[2]];
        let e2 = [fc[0] - w, fc[1] - d / 2.0, fc[2]];
        let mut body = Mesh::new();
        body.append(&frame);
        let mut glass = Mesh::new();
        let mut louver = Mesh::new();
        let mut handle = Mesh::new();
        if self.frame_style == 2 && self.arc_glass {
            glass.append(&make(&parts.arc, flip, e1));
        } else {
            body.append(&make(&parts.arc, flip, e1));
        }
        body.append(&make(&parts.door, flip, e1));
        glass.append(&make(&parts.glass, flip, e1));
        louver.append(&make(&parts.louver, flip, e1));
        if let Some(h) = parts.handles.first() {
            handle.append(&make(h, flip, e1));
        }
        if double {
            body.append(&make(&parts.arc, true, e2));
            body.append(&make(&parts.door, true, e2));
            glass.append(&make(&parts.glass, true, e2));
            louver.append(&make(&parts.louver, true, e2));
            if let Some(h) = parts.handles.get(1).or(parts.handles.first()) {
                handle.append(&make(h, true, e2));
            }
        }
        // final: rotate z by 1.5708 and put the lowest point on z = 0
        let mut all = Mesh::new();
        for m in [&body, &glass, &louver, &handle] {
            all.append(m);
        }
        let (lo, _) = all.bounds();
        let rot = Mat4::euler_xyz([0.0, 0.0, 1.5708]);
        let mut fin = |m: &mut Mesh| {
            for p in m.pos.iter_mut() {
                *p = rot.apply(*p);
            }
            translate(m, [0.0, 0.0, -lo[2]]);
        };
        fin(&mut body);
        fin(&mut glass);
        fin(&mut louver);
        fin(&mut handle);
        DoorPieces { body, glass, louver, handle }
    }

    /// factory(j): the instance-seeded door asset split into material pieces
    pub fn build_door(&self, inst: u64) -> DoorPieces {
        let mut r = super::inst_rng(self.fseed, inst);
        let parts = self.build_parts(&mut r);
        self.assemble_pieces(&parts)
    }

    pub fn assemble(&self, parts: &DoorParts) -> Mesh {
        let p = self.assemble_pieces(parts);
        let mut m = p.body;
        m.append(&p.glass);
        m.append(&p.louver);
        m.append(&p.handle);
        m
    }
}


/// draws of `Cls().apply(obj, ...)` (the shader function runs once per apply call, consuming global numpy draws in shader-body order)
pub fn apply_draws(r: &mut Rng, cls: &str) {
    match cls {
        "metal.MetalBasic" => {
            r.uniform(10.0, 25.0);
            r.uniform(0.0, 0.2);
            r.uniform(0.4, 0.7);
            metal_hsv(r);
            r.uniform(0.5, 1.0);
        }
        "metal.BrushedMetal" | "metal.GalvanizedMetal" => {
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
        }
        "metal.GrainedMetal" => {
            r.uniform(0.0, 1.0);
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
        }
        "metal.HammeredMetal" => {
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
            r.log_uniform(0.8, 1.2);
            r.log_uniform(0.001, 0.003);
        }
        "wood.Wood" => {
            r.randint(0, 49);
            r.uniform(0.0, 0.05);
            r.uniform(-0.3, 0.2);
            r.log_uniform(0.2, 20.0);
            r.uniform(0.0, 1.0);
            r.log_uniform(0.0002, 0.01);
            r.uniform(0.0, 0.4);
            r.log_uniform(40.0, 50.0);
            r.uniform(0.0, 0.8);
            r.uniform(0.0, 1.4);
        }
        "plastic.Plastic" => {
            let a = r.uniform(0.0, 1.0);
            let b = r.uniform(0.0, 1.0);
            r.uniform(0.0, 1.0);
            if a > b {
                apply_draws(r, "plastic.PlasticRough");
            } else {
                apply_draws(r, "plastic.PlasticTranslucent");
            }
        }
        "plastic.PlasticRough" => {
            r.uniform(0.0, 1.0);
            r.uniform(-1000.0, 1000.0);
            r.uniform(0.0, 1.0);
            r.uniform(0.5, 0.8);
            r.log_uniform(0.01, 0.5);
        }
        "plastic.PlasticTranslucent" => {
            r.uniform(0.0, 1.0);
            r.uniform(0.5, 0.8);
            r.log_uniform(0.4, 0.8);
        }
        "ceramic.Glass" => {
            if r.uniform(0.0, 1.0) >= 0.5 {
                r.uniform(0.0, 1.0);
            }
        }
        _ => {}
    }
}
