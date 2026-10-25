//! objects/windows/window.py: WindowFactory (nodegroup_window_geometry + BevelSharp chamfer), exact draw order.
use super::curvekit::*;
use super::doors::binomial as _unused_binomial;
use super::matdraw::*;
use super::sofa::mesh_cube;
use super::solid::Solid;
use crate::mesh::*;
use crate::rng::Rng;

pub const WOODS: Table = &[("wood.Wood", 1.0), ("wood.WhitePlywood", 0.1), ("wood.BlackPlywood", 0.1), ("wood.BlondePlywood", 0.1)];
pub const CURTAIN: Table = &[("fabric.FineKnitFabric", 1.0), ("wood.Wood", 2.0), ("plastic.PlasticRough", 2.0), ("ceramic.GlassVolume", 0.5), ("lamp_shaders.LampShade", 2.0)];

#[derive(Clone, Debug)]
pub struct WinAsset {
    pub frame_mat: &'static str,
    pub rod_mat: &'static str,
    pub cloth_mat: &'static str,
    pub width: f64,
    pub height: f64,
    pub frame_width: f64,
    pub frame_thickness: f64,
    pub panel_h: f64,
    pub panel_v: f64,
    pub sub_frame_width: f64,
    pub sub_frame_thickness: f64,
    pub sub_h: f64,
    pub sub_v: f64,
    pub glass_thickness: f64,
    pub oe_offset: f64,
    pub curtain: bool,
    pub shutter: bool,
    pub shutter_panel_radius: f64,
    pub shutter_width: f64,
    pub shutter_thickness: f64,
    pub shutter_rotation: f64,
    pub shutter_interval: f64,
    pub curtain_frame_depth: f64,
    pub curtain_depth: f64,
    pub curtain_interval_number: i64,
    pub curtain_frame_radius: f64,
    pub curtain_mid_l: f64,
    pub curtain_mid_r: f64,
}

pub struct WindowSpec {
    pub fseed: u64,
    pub bevel_amount: f64,
}

fn clip_gaussian_py(r: &mut Rng, mean: f64, std: f64, lo: f64, hi: f64, max_tries: usize) -> f64 {
    // infinigen.core.util.math.clip_gaussian (see geomkit::clip_gaussian for the 4-arg variant)
    let mut i = 0;
    loop {
        let v = r.normal(mean, std);
        if v >= lo && v <= hi {
            return v;
        }
        i += 1;
        if i >= max_tries {
            return v.clamp(lo, hi);
        }
    }
}

impl WindowSpec {
    /// WindowFactory.__init__ (FixedSeed(fseed)): sample_parameters draws, then BevelSharp()
    pub fn new(fseed: u64) -> WindowSpec {
        let mut r = Rng::seeded(fseed);
        let r = &mut r;
        let frame_width = r.uniform(0.05, 0.1);
        r.uniform(0.01, frame_width);
        r.randint(1, 2);
        r.randint(1, 2);
        r.uniform(0.01, 0.03);
        r.uniform(0.001, 0.003);
        r.uniform(0.03, 0.05);
        r.uniform(0.003, 0.007);
        r.uniform(0.0, 1.0);
        r.uniform(0.001, 0.003);
        let cfd = r.uniform(0.05, 0.1);
        r.uniform(0.03, cfd);
        r.uniform(0.01, 0.02);
        let ms = [sample_call(r, WOODS), sample_call(r, METALS), sample_call(r, CURTAIN)];
        if std::env::var("DBGP").is_ok() { eprintln!("mats {:?}", ms); }
        // shader_window_glass
        clip_gaussian_py(r, 0.0, 0.015, 0.0, 0.03, 20);
        r.uniform(0.05, 0.12);
        let bevel_amount = r.uniform(0.001, 0.006);
        r.uniform(0.0, 1.0);
        WindowSpec { fseed, bevel_amount }
    }

    /// sample_asset_params(dimensions=(w, h, t), open=False) on the instance stream
    pub fn sample_asset(&self, r: &mut Rng, dims: [f64; 3], open_given: bool) -> WinAsset {
        let u = |r: &mut Rng, a: f64, b: f64| r.uniform(a, b);
        let [width, height, frame_thickness] = dims;
        let frame_width = u(r, 0.02, 0.05) * width.min(height).min(2.0);
        let panel_width = u(r, 0.8, 1.5).min(width);
        let panel_height = u(r, panel_width, 3.0).min(height);
        let panel_v = (width / panel_width).floor().max(1.0);
        let panel_h = (height / panel_height).floor().max(1.0);
        let glass_thickness = u(r, 0.01, 0.03);
        let sub_frame_width = u(r, glass_thickness, frame_width);
        let sub_frame_thickness = u(r, glass_thickness, frame_thickness);
        let sub_panel_width = u(r, 0.4, panel_width.min(1.0));
        let mut sub_panel_height = u(r, 0.4, panel_height.min(1.0));
        sub_panel_height = sub_panel_height.min(2.0 * sub_panel_width).max(0.5 * sub_panel_width);
        let sub_v = (panel_width / sub_panel_width).floor().max(1.0);
        let sub_h = (panel_height / sub_panel_height).floor().max(1.0);
        // open is given (False): not drawn; shutter / curtain come from the factory (None) and are drawn
        if !open_given {
            u(r, 0.0, 1.0); // open = U(0,1) < 0.5 (then forced to False)
        }
        let shutter = u(r, 0.0, 1.0) < 0.2;
        let curtain = u(r, 0.0, 1.0) < 0.3;
        let mut open_type = r.randint(0, 3);
        if panel_v as i64 % 2 == 1 {
            open_type = r.randint(1, 3);
        }
        let mut oe_offset = 0.0;
        if open_type == 0 {
            if frame_thickness < sub_frame_thickness * 2.0 {
                open_type = r.randint(1, 2);
            } else {
                oe_offset = u(r, sub_frame_thickness / 2.0, (frame_thickness - 2.0 * sub_frame_thickness) / 2.0);
            }
        }
        let _ = open_type;
        let shutter_panel_radius = u(r, 0.001, 0.003);
        let shutter_width = u(r, 0.03, 0.05);
        let shutter_thickness = u(r, 0.003, 0.007);
        let shutter_rotation = u(r, 0.0, 1.0);
        let shutter_interval = shutter_width + u(r, 0.001, 0.003);
        let curtain_frame_depth = u(r, 0.05, 0.1);
        let curtain_depth = u(r, 0.03, curtain_frame_depth);
        let curtain_interval_number = (width / u(r, 0.08, 0.2)) as i64;
        let curtain_frame_radius = u(r, 0.01, 0.02);
        let curtain_mid_l = -u(r, 0.0, width / 2.0);
        let curtain_mid_r = u(r, 0.0, width / 2.0);
        let frame_mat = sample_call(r, WOODS);
        let rod_mat = sample_call(r, METALS);
        let cloth_mat = sample_call(r, CURTAIN);
        WinAsset { frame_mat, rod_mat, cloth_mat, width, height, frame_width, frame_thickness, panel_h, panel_v, sub_frame_width, sub_frame_thickness, sub_h, sub_v, glass_thickness, oe_offset, curtain, shutter, shutter_panel_radius, shutter_width, shutter_thickness, shutter_rotation, shutter_interval, curtain_frame_depth, curtain_depth, curtain_interval_number, curtain_frame_radius, curtain_mid_l, curtain_mid_r }
    }
}

fn quad(w: f64, h: f64) -> Curve {
    Curve::new(vec![[w / 2.0, h / 2.0, 0.0], [-w / 2.0, h / 2.0, 0.0], [-w / 2.0, -h / 2.0, 0.0], [w / 2.0, -h / 2.0, 0.0]], true)
}

fn weld_shell(m: &Mesh) -> Mesh {
    m.welded(1e-9)
}

/// line_seq: `amount` parallel lines (along x, length `width`) between y = -height/2 and y = height/2
fn line_seq(width: f64, height: f64, amount: usize) -> Vec<Curve> {
    (0..amount)
        .map(|i| {
            let y = -height / 2.0 + (i as f64 + 1.0) * (height / (amount as f64 + 1.0));
            Curve::line([width / 2.0, y, 0.0], [-width / 2.0, y, 0.0])
        })
        .collect()
}

/// nodegroup_window_panel: returns the shells (frame, dividers, glass)
pub fn window_panel(w: f64, h: f64, fw: f64, ft: f64, pw: f64, pt: f64, hamount: usize, vamount: usize, glass: Option<f64>) -> Vec<(u8, Mesh)> {
    let mut out = vec![];
    // vertical-ish dividers (rotated line_seq with swapped size), then horizontal ones, then frame
    let mut vert = Mesh::new();
    for mut c in line_seq(h, w, vamount - 1) {
        c.transform(&Mat4::euler_xyz([0.0, 0.0, 1.5708]));
        let m = curve_to_mesh(&c, &quad(pw, pt - 0.001 - 0.001), false);
        vert.append(&m);
    }
    let mut hor = Mesh::new();
    for c in line_seq(w, h, hamount - 1) {
        hor.append(&curve_to_mesh(&c, &quad(pw, pt - 0.001), false));
    }
    let frame = curve_to_mesh(&quad(w, h), &quad(fw * (2.0f32).sqrt() as f64, ft), false);
    out.push((0, vert));
    out.push((0, hor));
    out.push((0, frame));
    if let Some(gt) = glass {
        out.push((1, mesh_cube([w, h, gt], 2, 2, 2)));
    }
    out
}

impl WinAsset {
    /// window geometry (nodegroup_window_geometry, open = false), shells before the final rotation / bevel
    pub fn shells(&self) -> Vec<(u8, Mesh)> {
        let (w, h) = (self.width, self.height);
        let (v, hn) = (self.panel_v, self.panel_h);
        let cell_w = (w - self.frame_width * v) / v;
        let cell_h = (h - self.frame_width * hn) / hn;
        let pw = cell_w - self.sub_frame_width;
        let ph = cell_h - self.sub_frame_width;
        let mut cell: Vec<(u8, Mesh)> = if self.shutter {
            vec![] // shutter built in shutter_shells
        } else {
            window_panel(pw, ph, self.sub_frame_width, self.sub_frame_thickness, self.sub_frame_width, self.sub_frame_thickness, self.sub_h as usize, self.sub_v as usize, Some(self.glass_thickness))
        };
        if self.shutter {
            cell = self.shutter_shells(pw, ph);
        }
        let mut out = vec![];
        // instances: index i -> column c = floor(i / H), row r = i % H
        let n = (hn * v) as usize;
        let x0 = -w * 0.5 + (w / v) * 0.5;
        let y0 = -h * 0.5 + (h / hn) * 0.5;
        for i in 0..n {
            let c = (i as f64 / hn).floor();
            let rr = i as f64 % hn;
            let ox = x0 + c * (cell_w + self.frame_width);
            let oy = y0 + rr * (cell_h + self.frame_width);
            let oz = (-1.0f64).powf(c) * self.oe_offset;
            for (tag, m) in &cell {
                let mut t = m.clone();
                for p in t.pos.iter_mut() {
                    *p = [p[0] + ox, p[1] + oy, p[2] + oz];
                }
                out.push((*tag, t));
            }
        }
        // outer panel
        out.extend(window_panel(w, h, self.frame_width, self.frame_thickness, self.frame_width, self.frame_thickness, hn as usize, v as usize, None));
        out
    }

    fn shutter_shells(&self, w: f64, h: f64) -> Vec<(u8, Mesh)> {
        let fw = self.frame_width;
        let hp = h - fw; // Math.003 (shutter group): Height - FrameWidth
        let n = (hp / self.shutter_interval).floor();
        let ti = hp / n;
        let rot = Mat4::euler_xyz([self.shutter_rotation, 0.0, 0.0]);
        let mut out = vec![];
        // central bar cube, offset by the rotated line end
        let mut bar = mesh_cube([self.shutter_panel_radius, hp - 2.0 * ti, self.shutter_panel_radius], 2, 2, 2);
        let off = rot.apply([0.0, self.shutter_width * 0.5, 0.0]);
        for p in bar.pos.iter_mut() {
            *p = [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
        }
        out.push((4, bar));
        for i in 0..(n as i64 - 1) {
            let mut sl = mesh_cube([w - fw, self.shutter_width, self.shutter_thickness], 2, 2, 2);
            let y = i as f64 * ti + (hp * -0.5 + ti);
            for p in sl.pos.iter_mut() {
                let q = rot.apply(*p);
                *p = [q[0], q[1] + y, q[2]];
            }
            out.push((4, sl));
        }
        out.push((4, curve_to_mesh(&quad(w, h), &quad(fw * (2.0f32).sqrt() as f64, self.frame_thickness), false)));
        out
    }
}

/// Blender icosphere (BMesh create_icosphere): `sub` = node Subdivisions (1 = icosahedron)
pub fn icosphere(sub: usize, r: f64) -> Mesh {
    let s5 = 5f64.sqrt();
    let mut v: Vec<V3> = vec![[0.0, 0.0, -1.0]];
    for k in 0..5 {
        let a = (-36.0f64 + 72.0 * k as f64).to_radians();
        v.push([2.0 / s5 * a.cos(), 2.0 / s5 * a.sin(), -1.0 / s5]);
    }
    for k in 0..5 {
        let a = (-72.0f64 + 72.0 * k as f64).to_radians();
        v.push([2.0 / s5 * a.cos(), 2.0 / s5 * a.sin(), 1.0 / s5]);
    }
    v.push([0.0, 0.0, 1.0]);
    let d = |a: V3, b: V3| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let el = d(v[0], v[1]);
    let adj = |i: usize, j: usize| (d(v[i], v[j]) - el).abs() < 1e-6;
    let mut tris: Vec<[V3; 3]> = vec![];
    for i in 0..12 {
        for j in i + 1..12 {
            for k in j + 1..12 {
                if adj(i, j) && adj(j, k) && adj(i, k) {
                    let n = crate::mesh::cross(crate::mesh::sub(v[j], v[i]), crate::mesh::sub(v[k], v[i]));
                    let c = [v[i][0] + v[j][0] + v[k][0], v[i][1] + v[j][1] + v[k][1], v[i][2] + v[j][2] + v[k][2]];
                    if crate::mesh::dot(n, c) > 0.0 {
                        tris.push([v[i], v[j], v[k]]);
                    } else {
                        tris.push([v[i], v[k], v[j]]);
                    }
                }
            }
        }
    }
    let norm = |a: V3| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let mid = |a: V3, b: V3| norm([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]);
    for _ in 1..sub {
        let mut nt = vec![];
        for t in &tris {
            let (a, b, c) = (t[0], t[1], t[2]);
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            nt.push([a, ab, ca]);
            nt.push([ab, b, bc]);
            nt.push([ca, bc, c]);
            nt.push([ab, bc, ca]);
        }
        tris = nt;
    }
    let mut m = Mesh::new();
    for t in tris {
        m.add_tri([t[0][0] * r, t[0][1] * r, t[0][2] * r], [t[1][0] * r, t[1][1] * r, t[1][2] * r], [t[2][0] * r, t[2][1] * r, t[2][2] * r]);
    }
    m.welded(1e-9)
}

fn shifted(mut m: Mesh, o: V3) -> Mesh {
    for p in m.pos.iter_mut() {
        *p = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
    }
    m
}

impl WinAsset {
    /// nodegroup_curtain (+ the window's final translation), mesh boolean omitted
    pub fn curtain_shells(&self) -> Vec<(u8, Mesh)> {
        let (w, h) = (self.width, self.height);
        let rad = self.curtain_frame_radius;
        let mut out = vec![];
        let k = (self.curtain_interval_number as f32 * 6.28f32 / w as f32) as f64;
        let mut cloth = Mesh::new();
        for (a, b) in [(self.curtain_mid_r, w / 2.0), (-w / 2.0, self.curtain_mid_l)] {
            let line = Curve::line([a, 0.0, 0.0], [b, 0.0, 0.0]).resample(200);
            let mut pts = line.pts.clone();
            let mut len = 0.0;
            for i in 0..pts.len() {
                if i > 0 {
                    let q = line.pts[i - 1];
                    let p = line.pts[i];
                    len += ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                }
                pts[i][2] += ((len * k + 1.68) as f32).sin() as f64 * self.curtain_depth;
            }
            cloth.append(&curve_to_mesh(&Curve::new(pts, false), &quad(h, 0.002), false));
        }
        out.push((2, cloth));
        let y0 = -0.47 * h;
        let circ = Curve::circle(32, rad);
        let mut rods = Mesh::new();
        for (a, b) in [([-w / 2.0, 0.0, 0.0], [-w / 2.0, 0.0, -self.curtain_frame_depth]), ([w / 2.0, 0.0, 0.0], [w / 2.0, 0.0, -self.curtain_frame_depth]), ([-w / 2.0, 0.0, 0.0], [w / 2.0, 0.0, 0.0])] {
            rods.append(&curve_to_mesh(&Curve::line(a, b), &circ, true));
        }
        out.push((3, shifted(rods, [0.0, y0, 0.0])));
        out.push((3, shifted(icosphere(4, 2.0 * rad), [-w / 2.0, y0, 0.0])));
        out.push((3, shifted(icosphere(4, 2.0 * rad), [w / 2.0, y0, 0.0])));
        let dz = self.curtain_frame_depth + self.frame_thickness * 0.5;
        out.into_iter().map(|(t, m)| (t, shifted(m, [0.0, 0.0, dz]))).collect()
    }
}

/// chamfered window pieces after the final x rotation: (tag, mesh) with tag 0 frame, 1 glass, 2 curtain cloth, 3 curtain rod
pub fn window_pieces(spec: &WindowSpec, a: &WinAsset) -> Vec<(u8, Mesh)> {
    let mut shells = a.shells();
    if a.curtain {
        shells.extend(a.curtain_shells());
    }
    let rot = Mat4::euler_xyz([std::f64::consts::FRAC_PI_2, 0.0, 0.0]);
    let mut out = vec![];
    for tag in 0..5u8 {
        let mut raw = Mesh::new();
        for (t, m) in &shells {
            if *t == tag {
                raw.append(&weld_shell(m));
            }
        }
        if raw.is_empty() {
            continue;
        }
        let mut s = Solid::from_mesh(&raw);
        for p in s.pos.iter_mut() {
            *p = rot.apply(*p);
        }
        out.push((tag, s.chamfer_sharp(spec.bevel_amount, 70f64.to_radians()).to_mesh()));
    }
    out
}

/// full asset: all pieces joined
pub fn window_mesh(spec: &WindowSpec, a: &WinAsset) -> Mesh {
    let mut m = Mesh::new();
    for (_, p) in window_pieces(spec, a) {
        m.append(&p);
    }
    m
}

pub fn unseeded_inst_rng(fseed: u64, inst: u64) -> Rng {
    super::inst_rng(fseed, inst)
}
