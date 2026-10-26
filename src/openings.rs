//! Door and window assets placed in room cutters (room/decorate.py populate_doors / populate_windows),
//! built with the exact ports in assets/{doors,windows}.rs.
use crate::assets::doors::{DoorKind, DoorSpec};
use crate::assets::windows::{window_pieces, WindowSpec};
use crate::materials::*;
use crate::mesh::*;
use crate::rng::Rng;
use crate::rooms::consts::RoomConstants;
use crate::rooms::solidify::{CutKind, Cutter};
use serde_json::json;

pub struct Piece {
    pub name: String,
    pub class: String,
    pub mesh: Mesh,
    pub mat: Material,
}

fn place(m: &mut Mesh, c: &Cutter, local: &Mat4) {
    let full = Mat4::translation([c.loc[0], c.loc[1], c.loc[2]]).mul(&Mat4::rot_z(c.rot_z)).mul(local);
    m.transform(&full);
}

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

/// Interior-door geometry constants (metres).
const GAP_SIDE: f64 = 0.003;
const GAP_TOP: f64 = 0.003;
const GAP_BOTTOM: f64 = 0.010;
const LINING: f64 = 0.02; // jamb lining thickness (inside the wall opening)
const STOP_W: f64 = 0.015; // door stop: how far it protrudes from the lining
const STOP_T: f64 = 0.012; // door stop thickness along the wall normal
const CASING_W: f64 = 0.07; // architrave width
const CASING_T: f64 = 0.015; // architrave thickness (proud of the wall face)

/// One door leaf: closed local geometry (cutter frame) plus everything needed to swing it and to keep furniture clear.
pub struct DoorLeaf {
    pub ci: usize,
    pub name: String,
    pub parts: Vec<Piece>, // leaf pieces, closed pose, cutter frame (z relative to the cutter centre)
    pub hs: f64,           // local x sign of the hinge edge
    pub swing: f64,        // local y sign of the swing side
    pub xh: f64,           // local x of the hinge edge
    pub t: f64,
    pub w: f64,
    pub h: f64,
    pub zb: f64, // leaf bottom (local z)
    pub desired: f64,
    pub angle: f64,
    pub limiter: String,
    pub room: String,
}

pub struct Openings {
    pub pieces: Vec<Piece>, // static pieces: windows, door frames / casings
    pub doors: Vec<DoorLeaf>,
}

impl DoorLeaf {
    /// local-frame matrix of the leaf swung open by `a` radians about the hinge axis
    pub fn local_pose(&self, a: f64) -> Mat4 {
        let (px, py) = (self.xh, self.swing * self.t / 2.0);
        Mat4::translation([px, py, 0.0]).mul(&Mat4::rot_z(-self.hs * self.swing * a)).mul(&Mat4::translation([-px, -py, 0.0]))
    }
    pub fn full_pose(&self, ct: &Cutter, a: f64) -> Mat4 {
        Mat4::translation([ct.loc[0], ct.loc[1], ct.loc[2]]).mul(&Mat4::rot_z(ct.rot_z)).mul(&self.local_pose(a))
    }
    /// oriented boxes (centre, half extents, yaw) covering the leaf swept over 0..angle (+ `pad`), for people / clutter keep-out
    pub fn zones(&self, ct: &Cutter, pad: f64) -> Vec<([f64; 3], [f64; 3], f64)> {
        let n = ((self.angle.to_degrees() / 8.0).ceil() as usize).max(1);
        let mut out = vec![];
        for i in 0..=n {
            let a = self.angle * i as f64 / n as f64;
            let m = self.full_pose(ct, a);
            let c = m.apply([self.xh - self.hs * self.w / 2.0, 0.0, self.zb + self.h / 2.0]);
            out.push(([c[0], c[1], c[2]], [self.w / 2.0 + pad, (self.t / 2.0).max(0.06) + pad, self.h / 2.0], ct.rot_z - self.hs * self.swing * a));
        }
        out
    }
    /// world meshes of the leaf pieces at the final angle
    pub fn world_pieces(self, ct: &Cutter) -> Vec<Piece> {
        let m = self.full_pose(ct, self.angle);
        self.parts.into_iter().map(|mut p| { p.mesh.transform(&m); p }).collect()
    }
}

fn rank(ty: &str) -> i32 {
    match ty {
        "Hallway" | "Hall" | "Corridor" | "Staircase" | "StaircaseRoom" | "Balcony" | "Garage" => 0,
        "LivingRoom" | "DiningRoom" => 1,
        "Kitchen" => 2,
        "Bathroom" | "Toilet" | "Closet" | "Utility" | "Laundry" => 4,
        _ => 3,
    }
}

fn plain_white(seed: u64) -> Material {
    // same paint as the skirting boards (gen2): rough plastic, near-white
    let mut pr = Rng::seeded(crate::int_hash(&[&seed.to_string(), "skirting_floor"]));
    pr.uniform(0.02, 0.05);
    pr.uniform(0.08, 0.15);
    let (sat, val) = (pr.uniform(0.0, 0.1), pr.uniform(0.8, 1.0));
    let rough = pr.uniform(0.5, 1.0);
    let col = [val, val * (1.0 - sat), val * (1.0 - sat), 1.0];
    let mut m = Material::diffuse("door_casing", [0.85, 0.85, 0.85], rough as f32);
    m.spec = Some(json!({"cls": "shader.plastic.plastic_rough.shader_rough_plastic", "kwargs": {"base_color": col, "roughness": rough}, "seed": 0}));
    m
}

/// jamb lining + door stop + architrave on both wall faces, cutter frame
fn frame_mesh(c: &RoomConstants, swing: f64, t: f64) -> Mesh {
    let (dw, ds, wt) = (c.door_width, c.door_size, c.wall_thickness);
    let (zf, zt) = (-ds / 2.0, ds / 2.0);
    let mut m = Mesh::new();
    let mut bx = |x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64| m.append(&Mesh::cuboid([x0, y0, z0], [x1, y1, z1]));
    // lining (full wall depth)
    bx(-dw / 2.0, -dw / 2.0 + LINING, -wt / 2.0, wt / 2.0, zf, zt);
    bx(dw / 2.0 - LINING, dw / 2.0, -wt / 2.0, wt / 2.0, zf, zt);
    bx(-dw / 2.0 + LINING, dw / 2.0 - LINING, -wt / 2.0, wt / 2.0, zt - LINING, zt);
    // door stop on the side the leaf closes against (opposite the swing side)
    let (y0, y1) = if swing > 0.0 { (-t / 2.0 - STOP_T, -t / 2.0) } else { (t / 2.0, t / 2.0 + STOP_T) };
    bx(-dw / 2.0 + LINING, -dw / 2.0 + LINING + STOP_W, y0, y1, zf, zt - LINING);
    bx(dw / 2.0 - LINING - STOP_W, dw / 2.0 - LINING, y0, y1, zf, zt - LINING);
    bx(-dw / 2.0 + LINING + STOP_W, dw / 2.0 - LINING - STOP_W, y0, y1, zt - LINING - STOP_W, zt - LINING);
    // architrave, both faces
    for sg in [-1.0, 1.0] {
        let (ya, yb) = if sg > 0.0 { (wt / 2.0, wt / 2.0 + CASING_T) } else { (-wt / 2.0 - CASING_T, -wt / 2.0) };
        bx(-dw / 2.0 - CASING_W, -dw / 2.0, ya, yb, zf, zt);
        bx(dw / 2.0, dw / 2.0 + CASING_W, ya, yb, zf, zt);
        bx(-dw / 2.0 - CASING_W, dw / 2.0 + CASING_W, ya, yb, zt, zt + CASING_W);
    }
    m
}

/// room_doors stage: python populate_doors (3 door factories, door_chance 1, casing_chance 0), then the leaf is refitted: thin
/// (3.5-4.5 cm), centred in the wall opening inside a jamb lining / stop / architrave, hinged at one jamb and swinging into the room
/// it opens to. `desired` angles keep python's rot_z draw; `settle_doors` limits them against furniture and walls.
fn doors(c: &RoomConstants, cutters: &[Cutter], polys: &[(String, crate::shp::G)], seed: u64, out: &mut Vec<Piece>, leaves: &mut Vec<DoorLeaf>) {
    let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "room_doors"]));
    let dc: Vec<(usize, &Cutter)> = cutters.iter().enumerate().filter(|(_, c)| c.kind != CutKind::Window).collect();
    let kinds = [DoorKind::Panel, DoorKind::GlassPanel, DoorKind::Louver, DoorKind::Lite];
    let mut facs = vec![];
    for _ in 0..3 {
        let k = kinds[r.choice_p(&[4.0 / 12.0, 2.0 / 12.0, 3.0 / 12.0, 3.0 / 12.0])];
        let s = r.randint(0, 10_000_000) as u64;
        facs.push(DoorSpec::new(k, s, c));
    }
    let idx: Vec<usize> = (0..dc.len()).map(|_| r.randint(0, 3) as usize).collect();
    if std::env::var("OPDBG").is_ok() { eprintln!("door facs {:?} idx {:?}", facs.iter().map(|f| (f.kind, f.fseed)).collect::<Vec<_>>(), idx); }
    let white = plain_white(seed);
    for (i, fac) in facs.iter().enumerate() {
        for j in 0..dc.len() {
            if idx[j] != i {
                continue;
            }
            let (ci, ct) = dc[j];
            let dch = r.f64(); // door_chance
            let rot_p = r.f64();
            if std::env::var("OPDBG").is_ok() { eprintln!("dch {dch} rot_p {rot_p}"); }
            let rot_z = if rot_p < 0.5 {
                r.uniform(0.0, 0.1)
            } else if rot_p < 0.7 {
                r.uniform(0.93, 1.03)
            } else {
                r.uniform(0.0, 1.0)
            } * std::f64::consts::FRAC_PI_2;
            if std::env::var("OPDBG").is_ok() { eprintln!("door j={j} ci={ci} fac={i} rot_z={rot_z}"); }
            let pieces = fac.build_door_leaf(j as u64);
            r.f64(); // casing_chance
            // ---- refit (own deterministic stream; python draws above are untouched)
            let mut fr = Rng::seeded(crate::int_hash(&[&seed.to_string(), "door_fit", &ci.to_string()]));
            let t = fr.uniform(0.035, 0.045);
            let hs = if fr.f64() < 0.5 { -1.0 } else { 1.0 };
            // swing room: the side whose room ranks higher (bathroom / bedroom beat hall / living); ties and exterior: local +y
            let dn = (-ct.rot_z.sin(), ct.rot_z.cos());
            let side = |sg: f64| -> Option<(&String, i32)> {
                let p = crate::shp::point(ct.loc[0] + sg * 0.5 * dn.0, ct.loc[1] + sg * 0.5 * dn.1);
                polys.iter().find(|(_, g)| crate::shp::contains(g, &p)).map(|(n, _)| (n, rank(crate::rooms::state::room_type(n))))
            };
            let (sp, sm) = (side(1.0), side(-1.0));
            let swing = match (&sp, &sm) {
                (Some(a), Some(b)) => if b.1 > a.1 { -1.0 } else { 1.0 },
                (None, Some(_)) => -1.0,
                _ => 1.0,
            };
            let room = (if swing > 0.0 { sp } else { sm }).map(|x| x.0.clone()).unwrap_or_default();
            // leaf bounding box (factory frame: x thickness, y width, z up)
            let mut lb = Mesh::new();
            for m in [&pieces.body, &pieces.glass, &pieces.louver] {
                lb.append(m);
            }
            if lb.is_empty() {
                continue;
            }
            let (lo, hi) = lb.bounds();
            let (d, yr, zr) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
            let xc = (lo[0] + hi[0]) / 2.0;
            // hinge end: far from the handle (free edge); without a handle, the high-y end
            let yh = if pieces.handle.is_empty() {
                hi[1]
            } else {
                let (hl, hh) = pieces.handle.bounds();
                if (hl[1] + hh[1]) / 2.0 > (lo[1] + hi[1]) / 2.0 { lo[1] } else { hi[1] }
            };
            let (dw, ds) = (c.door_width, c.door_size);
            let w = dw - 2.0 * (LINING + GAP_SIDE);
            let zb = -ds / 2.0 + GAP_BOTTOM;
            let zt = ds / 2.0 - LINING - GAP_TOP;
            let h = zt - zb;
            let xh = hs * (dw / 2.0 - LINING - GAP_SIDE);
            let sg_y = if yh == lo[1] { 1.0 } else { -1.0 };
            let sx = -hs * sg_y;
            let det_neg = sx * swing < 0.0; // permutation matrix [[0,sx,0],[sy,0,0],[0,0,1]] with sy = -swing: det = -sx*sy
            let map = |m: &Mesh| -> Mesh {
                let mut o = m.clone();
                for p in o.pos.iter_mut() {
                    let q = p[0] - xc;
                    let qd = if q.abs() <= d / 2.0 { q * t / d } else { q.signum() * (t / 2.0 + q.abs() - d / 2.0) };
                    let u = (p[1] - yh).abs() / yr;
                    *p = [xh - hs * u * w, -swing * qd, zb + (p[2] - lo[2]) * h / zr];
                }
                if det_neg { o.flipped() } else { o }
            };
            let sd = fac.fseed;
            let name = format!("door{ci}");
            let mut parts = vec![];
            let mut add = |suffix: &str, mesh: &Mesh, cls: &str, rgb: [f32; 3]| {
                if mesh.is_empty() {
                    return;
                }
                let nm = format!("{name}{suffix}");
                parts.push(Piece { name: nm.clone(), class: "Door".into(), mesh: map(mesh), mat: spec(&nm, cls, sd, rgb) });
            };
            add("", &pieces.body, fac.surface_cls, [0.5, 0.4, 0.3]);
            add(".glass", &pieces.glass, fac.glass_cls, [0.8, 0.9, 1.0]);
            add(".louver", &pieces.louver, fac.louver_cls, [0.5, 0.4, 0.3]);
            add(".handle", &pieces.handle, if fac.handle_type == 3 { fac.bar_cls } else { fac.handle_cls }, [0.7, 0.7, 0.7]);
            // hinge knuckles on the hinge edge, just proud of the swing-side face
            let mut hg = Mesh::new();
            for z in [zb + 0.20, (zb + zt) / 2.0, zt - 0.20] {
                hg.append(&Mesh::cylinder(xh - hs * 0.003, swing * (t / 2.0 + 0.002), z - 0.05, z + 0.05, 0.007, 10));
            }
            let nm = format!("{name}.hinge");
            parts.push(Piece { name: nm.clone(), class: "Door".into(), mesh: hg, mat: spec(&nm, fac.handle_cls, sd, [0.7, 0.7, 0.7]) });
            // static frame
            let mut fm = frame_mesh(c, swing, t);
            place(&mut fm, ct, &Mat4::identity());
            out.push(Piece { name: format!("{name}.frame"), class: "Door".into(), mesh: fm, mat: white.clone() });
            if std::env::var("DOORDBG").is_ok() {
                eprintln!("DOORFIT ci={ci} style={} hs={hs} swing={swing} t={t:.3} w={w:.3} h={h:.3} room={room} d={d:.3}", fac.frame_style);
            }
            leaves.push(DoorLeaf { ci, name, parts, hs, swing, xh, t, w, h, zb, desired: rot_z, angle: 0.0, limiter: "none".into(), room });
        }
        // casing_factory.finalize_assets(casings=[]) -> surface.apply([]) still runs the shader function (global draws)
        crate::assets::doors::apply_draws(&mut r, fac.surface_cls);
    }
}

/// room_windows stage: python populate_windows (2 window factories)
fn windows(c: &RoomConstants, cutters: &[Cutter], seed: u64, out: &mut Vec<Piece>) {
    let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "room_windows"]));
    let facs: Vec<WindowSpec> = (0..2).map(|_| WindowSpec::new(r.randint(0, 100_000) as u64)).collect();
    let mut j = 0u64;
    for (ci, ct) in cutters.iter().enumerate() {
        if ct.kind != CutKind::Window {
            continue;
        }
        let parent = ct.rooms.first().cloned().unwrap_or_default();
        let fac = &facs[(crate::int_hash(&[&parent]) % 2) as usize];
        let t = ct.dims[1] * r.uniform(0.1, 0.2);
        let dims = [ct.dims[0], ct.dims[2], t];
        // full-height windows pass open=False; others sample it (curtain/shutter kwargs are swallowed by create_asset)
        let tall = (ct.dims[2] - c.wall_height + c.wall_thickness).abs() < 0.1;
        let mut ir = crate::assets::inst_rng(fac.fseed, j);
        let asset = fac.sample_asset(&mut ir, dims, tall);
        let local = Mat4::translation([0.0, -c.wall_thickness / 2.0, 0.0]).mul(&Mat4::rot_y(std::f64::consts::PI));
        for (tag, mut mesh) in window_pieces(fac, &asset) {
            let (suffix, cls, rgb) = match tag {
                0 => ("", asset.frame_mat, [0.4, 0.3, 0.2]),
                1 => (".glass", "window.glass", [0.8, 0.9, 1.0]),
                2 => (".curtain", asset.cloth_mat, [0.8, 0.7, 0.6]),
                3 => (".rod", asset.rod_mat, [0.6, 0.6, 0.6]),
                _ => (".shutter", asset.frame_mat, [0.5, 0.4, 0.3]),
            };
            place(&mut mesh, ct, &local);
            let nm = format!("window{ci}{suffix}");
            out.push(Piece { name: nm.clone(), class: "Window".into(), mesh, mat: spec(&nm, cls, fac.fseed * 1000 + j, rgb) });
        }
        j += 1;
    }
}

pub fn make_openings(c: &RoomConstants, cutters: &[Cutter], polys: &[(String, crate::shp::G)], seed: u64) -> Openings {
    let mut pieces = vec![];
    let mut doors_v = vec![];
    doors(c, cutters, polys, seed, &mut pieces, &mut doors_v);
    windows(c, cutters, seed, &mut pieces);
    Openings { pieces, doors: doors_v }
}

/// Limit every door's opening angle: the leaf (a thin box, inflated by 2 cm) is swept from closed towards the desired angle in
/// 1 degree steps and stops at the last angle where it touches no furniture (visual meshes, else solver proxies; rugs excepted) and
/// stays inside the walls of the room it swings into. People and clutter are placed afterwards and keep out of the swept zone
/// (`DoorLeaf::zones`). Prints one `DOOR` report line per door.
pub fn settle_doors(
    ops: &mut Openings,
    cutters: &[Cutter],
    c: &RoomConstants,
    state: &crate::furn::state::State,
    visuals: &std::collections::HashMap<String, crate::assets::Asset>,
    polys: &[(String, crate::shp::G)],
) {
    use parry3d_f64::math::{Isometry, Point, Translation, Vector};
    use parry3d_f64::na::UnitQuaternion;
    use parry3d_f64::query;
    use parry3d_f64::shape::{Cuboid, SharedShape, TriMesh};
    const MARGIN: f64 = 0.02;
    let wt = c.wall_thickness;
    for d in ops.doors.iter_mut() {
        if let Some(v) = std::env::var("DOORDESIRED").ok().and_then(|v| v.parse::<f64>().ok()) {
            d.desired = v.to_radians(); // testing: force the desired angle
        }
        let ct = &cutters[d.ci];
        let m0 = d.full_pose(ct, 0.0);
        let pv = m0.apply([d.xh, d.swing * d.t / 2.0, 0.0]);
        let reach = d.w + d.t + 0.1;
        let (zlo, zhi) = (ct.loc[2] + d.zb - MARGIN, ct.loc[2] + d.zb + d.h + MARGIN);
        let (rlo, rhi) = ([pv[0] - reach, pv[1] - reach], [pv[0] + reach, pv[1] + reach]);
        let near = |lo: [f64; 3], hi: [f64; 3]| lo[0] < rhi[0] && hi[0] > rlo[0] && lo[1] < rhi[1] && hi[1] > rlo[1] && lo[2] < zhi && hi[2] > zlo;
        // obstacles near the swing disc
        let mut obs: Vec<(String, SharedShape, Isometry<f64>)> = vec![];
        for (k, o) in &state.objs {
            let Some(gen) = &o.generator else { continue };
            if gen.class.contains("Rug") || gen.class.contains("Mat") && gen.class.contains("Floor") {
                continue;
            }
            let mut done = false;
            if let Some(a) = visuals.get(k) {
                let pm = Mat4([
                    [o.pose.r[0][0], o.pose.r[0][1], o.pose.r[0][2], o.pose.t[0]],
                    [o.pose.r[1][0], o.pose.r[1][1], o.pose.r[1][2], o.pose.t[1]],
                    [o.pose.r[2][0], o.pose.r[2][1], o.pose.r[2][2], o.pose.t[2]],
                    [0.0, 0.0, 0.0, 1.0],
                ]);
                done = !a.parts.is_empty();
                for part in &a.parts {
                    if part.mesh.is_empty() {
                        continue;
                    }
                    let (l, h) = part.mesh.bounds();
                    let (mut wl, mut wh) = ([f64::MAX; 3], [f64::MIN; 3]);
                    for i in 0..8 {
                        let q = pm.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
                        for e in 0..3 {
                            wl[e] = wl[e].min(q[e]);
                            wh[e] = wh[e].max(q[e]);
                        }
                    }
                    if !near(wl, wh) {
                        continue;
                    }
                    let verts: Vec<Point<f64>> = part.mesh.pos.iter().map(|p| { let q = pm.apply(*p); Point::new(q[0], q[1], q[2]) }).collect();
                    let tris: Vec<[u32; 3]> = part.mesh.triangles();
                    if tris.is_empty() {
                        continue;
                    }
                    obs.push((k.clone(), match TriMesh::new(verts, tris) { Ok(t) => SharedShape::new(t), Err(_) => continue }, Isometry::identity()));
                }
            }
            if !done {
                for px in ig_furn::col::proxies(state, k).iter() {
                    let (l, h) = (px.aabb.mins, px.aabb.maxs);
                    if near([l.x, l.y, l.z], [h.x, h.y, h.z]) {
                        obs.push((k.clone(), px.shape.clone(), px.iso));
                    }
                }
            }
        }
        if std::env::var("DOORFAKE").is_ok() {
            // testing: a fake 30 cm obstacle in the leaf's path at 40 degrees
            let cc = d.full_pose(ct, 40f64.to_radians()).apply([d.xh - d.hs * d.w * 0.8, 0.0, d.zb + 0.5]);
            obs.push(("fake_obstacle".into(), SharedShape::cuboid(0.15, 0.15, 0.5), Isometry::translation(cc[0], cc[1], cc[2])));
        }
        if std::env::var("DOORDBG").is_ok() { eprintln!("DOORDBG ci={} obstacles near swing: {:?}", d.ci, obs.iter().map(|o| o.0.clone()).collect::<std::collections::BTreeSet<_>>()); }
        // the room the door swings into (inset by the wall half thickness) for the wall test
        let interior = polys.iter().find(|(n, _)| *n == d.room).map(|(_, g)| crate::shp::buffer_mitre(g, -wt / 2.0));
        let wall_hit = |a: f64| -> bool {
            let Some(g) = &interior else { return false };
            let lp = d.local_pose(a);
            let to_w = Mat4::translation(ct.loc).mul(&Mat4::rot_z(ct.rot_z));
            for i in 0..=6 {
                let s = d.w * i as f64 / 6.0;
                for fy in [-1.0, 1.0] {
                    let p = lp.apply([d.xh - d.hs * s, fy * (d.t / 2.0 + MARGIN), 0.0]);
                    if d.swing * p[1] >= wt / 2.0 + 0.003 {
                        let q = to_w.apply(p);
                        if !crate::shp::contains(g, &crate::shp::point(q[0], q[1])) {
                            return true;
                        }
                    }
                }
            }
            false
        };
        let cub = Cuboid::new(Vector::new(d.w / 2.0 + MARGIN, d.t / 2.0 + MARGIN, d.h / 2.0 + MARGIN));
        let hit = |a: f64| -> Option<String> {
            let m = d.full_pose(ct, a);
            let cc = m.apply([d.xh - d.hs * d.w / 2.0, 0.0, d.zb + d.h / 2.0]);
            let yaw = ct.rot_z - d.hs * d.swing * a;
            let iso = Isometry::from_parts(Translation::new(cc[0], cc[1], cc[2]), UnitQuaternion::from_axis_angle(&Vector::z_axis(), yaw));
            if wall_hit(a) {
                return Some("wall".into());
            }
            for (k, sh, oi) in &obs {
                if query::intersection_test(&iso, &cub, oi, sh.as_ref()).unwrap_or(false) {
                    return Some(k.clone());
                }
            }
            None
        };
        let n = d.desired.to_degrees().floor() as usize;
        let mut ok = 0usize;
        let mut lim = String::from("none");
        for k in 1..=n {
            if let Some(w) = hit((k as f64).to_radians()) {
                lim = w;
                break;
            }
            ok = k;
        }
        if lim == "none" {
            // all whole degrees passed: take the exact desired angle if it is also free
            d.angle = match hit(d.desired) {
                None => d.desired,
                Some(w) => { lim = w; (ok as f64).to_radians() }
            };
        } else {
            d.angle = (ok as f64).to_radians();
        }
        d.limiter = lim;
        eprintln!(
            "DOOR ci={} at=({:.2},{:.2}) room={} desired={:.0}deg final={:.0}deg limiter={} thickness={:.1}cm",
            d.ci, ct.loc[0], ct.loc[1], d.room, d.desired.to_degrees(), d.angle.to_degrees(), d.limiter, d.t * 100.0
        );
    }
}
