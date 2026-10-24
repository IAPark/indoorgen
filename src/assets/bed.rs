//! seating/bedframe.py, mattress.py, pillow.py, bed.py: BedFrameFactory / MattressFactory / PillowFactory / BedFactory.
//! Parameters and RNG order are exact; non-cloth geometry is built from ports of the bpy ops (chair.rs, bevelfull.rs, bpyops.rs),
//! cloth parts use the native solver (cloth.rs) and are approximate by design.
use super::chair::*;
use super::subsurf::subsurf;
use super::matdraw::*;
use crate::mesh::*;
use crate::rng::Rng;
use super::bevelfull::bevel_angle_ex;
use super::bpyx::inset_individual;
use super::coil::face_normal;

pub const BEDFRAME: Table = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0), ("wood.Wood", 7.0), ("ceramic.Plaster", 2.0)];

#[derive(Clone, Debug)]
pub struct BedSpec {
    /// ChairSpec with the BedFrameFactory overrides applied
    pub c: ChairSpec,
    pub has_all_legs: bool,
    pub leg_decor: usize, // 0 coiled 1 pad 2 plain 3 legs
    pub leg_decor_wrapped: bool,
    pub seat_sub_x: usize,
    pub seat_sub_y: usize,
    pub dot_distance: f64,
    pub dot_size: f64,
    pub dot_depth: f64,
    pub panel_distance: f64,
    pub panel_margin: f64,
    pub frame_cls: &'static str,
    pub frame_limb_cls: &'static str,
    /// BedFactory: sheet type (0 quilt 1 comforter 2 box_comforter 3 none), folded, cover
    pub sheet_type: usize,
    pub sheet_folded: bool,
    pub has_cover: bool,
}

impl BedSpec {
    pub fn frame(fseed: u64) -> BedSpec {
        let mut c = ChairSpec::new_bed(fseed);
        let mut r = Rng::seeded(fseed);
        let r = &mut r;
        c.width = r.log_uniform(1.4, 2.4);
        c.size = r.uniform(2.0, 2.4);
        c.thickness = r.uniform(0.05, 0.12);
        let has_all_legs = r.uniform(0.0, 1.0) < 0.2;
        c.leg_thickness = r.uniform(0.08, 0.12);
        c.leg_height = r.uniform(0.2, 0.6);
        let leg_decor = r.choice_p(&[2.0 / 7.0, 2.0 / 7.0, 1.0 / 7.0, 2.0 / 7.0]);
        let leg_decor_wrapped = r.uniform(0.0, 1.0) < 0.5;
        c.back_height = r.uniform(0.5, 1.3);
        c.seat_back = 1.0;
        let seat_sub_x = r.randint(1, 4) as usize;
        let seat_sub_y = r.log_uniform(4.0, 10.0) as usize;
        c.has_arm = false;
        c.leg_type = 0;
        c.leg_x_offset = 0.0;
        c.leg_y_offset = [0.0, 0.0];
        c.back_x_offset = 0.0;
        c.back_y_offset = 0.0;
        let frame_cls = sample_call(r, BEDFRAME);
        let frame_limb_cls = sample_call(r, FURN_HARD);
        if std::env::var("DBGB").is_ok() { let mut q = r.clone(); eprintln!("next {} {}", q.f64(), q.f64()); }
        r.uniform(0.0, 1.0); // scratch
        r.uniform(0.0, 1.0); // edge wear
        let dot_distance = r.log_uniform(0.16, 0.2);
        let dot_size = r.uniform(0.005, 0.02);
        let dot_depth = r.uniform(0.04, 0.08);
        let panel_distance = r.uniform(0.3, 0.5);
        let panel_margin = r.uniform(0.01, 0.02);
        c.surface_cls = frame_cls;
        c.limb_cls = frame_limb_cls;
        BedSpec { c, has_all_legs, leg_decor, leg_decor_wrapped, seat_sub_x, seat_sub_y, dot_distance, dot_size, dot_depth, panel_distance, panel_margin, frame_cls, frame_limb_cls, sheet_type: 0, sheet_folded: false, has_cover: false }
    }
}

type V3 = [f64; 3];

impl BedSpec {
    /// BedFrameFactory.make_seat: grid wireframe extruded to walls, SOLIDIFY NON_MANIFOLD (thickness leg_thickness - 1e-3, offset 0), bevel.
    /// Built directly as the solid Blender produces for a wall lattice: per lattice point one vertex per gap between consecutive walls
    /// (intersection of the facing offset planes), side quads per wall, rim quads on the top / bottom edges and a cap polygon at
    /// every point with >= 3 walls (triangles at T junctions, quads at crossings).
    pub fn seat_solid(&self) -> Mesh {
        let c = &self.c;
        seat_solid_params(self.seat_sub_x, self.seat_sub_y, c.width, c.size, c.thickness, c.leg_thickness)
    }
}

/// lattice solid of the bed seat; vertex indices depend only on (nx, ny)
pub fn seat_solid_params(nx: usize, ny: usize, width: f64, size: f64, thickness: f64, leg_thickness: f64) -> Mesh {
    {
        let (ax, ay) = ((width - leg_thickness) / 2.0, (size - leg_thickness) / 2.0);
        let h = (leg_thickness - 1e-3) / 2.0;
        let (z0, z1) = (0.0, thickness);
        let xs: Vec<f64> = (0..=nx).map(|i| -ax + 2.0 * ax * i as f64 / nx as f64).collect();
        let ys: Vec<f64> = (0..=ny).map(|j| -ay + 2.0 * ay * j as f64 / ny as f64).collect();
        let mut m = Mesh::new();
        // gap vertices: key (i, j, wall index a) = gap whose sector starts (ccw) at wall a; wall dirs 0:E 1:N 2:W 3:S
        let dirs: [(f64, f64); 4] = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
        let walls_at = |i: usize, j: usize| -> Vec<usize> {
            let mut w = vec![];
            if i < nx { w.push(0); }
            if j < ny { w.push(1); }
            if i > 0 { w.push(2); }
            if j > 0 { w.push(3); }
            w
        };
        let mut gap: crate::dhash::HashMap<(usize, usize, usize), (u32, u32)> = crate::dhash::HashMap::default();
        let mut caps: Vec<Vec<u32>> = vec![];
        for i in 0..=nx {
            for j in 0..=ny {
                let w = walls_at(i, j);
                let n = w.len();
                let mut ring = vec![];
                for k in 0..n {
                    let (a, b) = (w[k], w[(k + 1) % n]);
                    let (ua, ub) = (dirs[a], dirs[b]);
                    // left normal of a, right normal of b
                    let (na, nb) = ((-ua.1, ua.0), (ub.1, -ub.0));
                    let det = na.0 * nb.1 - na.1 * nb.0;
                    let q = if det.abs() < 1e-9 { (na.0 * h, na.1 * h) } else { ((h * nb.1 - h * na.1) / det, (na.0 * h - nb.0 * h) / det) };
                    let p = [xs[i] + q.0, ys[j] + q.1];
                    let lo = m.pos.len() as u32;
                    m.pos.push([p[0], p[1], z0]);
                    m.pos.push([p[0], p[1], z1]);
                    gap.insert((i, j, a), (lo, lo + 1));
                    ring.push((lo, lo + 1));
                }
                if n >= 3 {
                    caps.push(ring.iter().map(|r| r.1).collect());
                    caps.push(ring.iter().rev().map(|r| r.0).collect());
                }
            }
        }
        let prev_wall = |i: usize, j: usize, wl: usize| -> usize {
            let w = walls_at(i, j);
            let k = w.iter().position(|&x| x == wl).unwrap();
            w[(k + w.len() - 1) % w.len()]
        };
        // every wall once: from P=(i,j) in direction E or N
        for i in 0..=nx {
            for j in 0..=ny {
                for &wl in &[0usize, 1] {
                    let (qi, qj) = if wl == 0 { (i + 1, j) } else { (i, j + 1) };
                    if qi > nx || qj > ny {
                        continue;
                    }
                    // left side at P: gap starting at this wall; at Q: gap starting at the wall before (Q -> P)
                    let back = wl + 2;
                    let pl = gap[&(i, j, wl)];
                    let pr = gap[&(i, j, prev_wall(i, j, wl))];
                    let ql = gap[&(qi, qj, prev_wall(qi, qj, back))];
                    let qr = gap[&(qi, qj, back)];
                    // left side (outward = left normal), right side, bottom, top
                    m.faces.push(vec![pl.0, pl.1, ql.1, ql.0]);
                    m.faces.push(vec![pr.0, qr.0, qr.1, pr.1]);
                    m.faces.push(vec![pl.0, ql.0, qr.0, pr.0]);
                    m.faces.push(vec![pl.1, pr.1, qr.1, ql.1]);
                }
            }
        }
        m.faces.extend(caps);
        m
    }
}

impl BedSpec {
    pub fn make_seat(&self) -> Mesh {
        let c = &self.c;
        let mut m = self.seat_solid();
        for p in m.pos.iter_mut() {
            p[1] -= c.size / 2.0;
            p[2] -= c.thickness / 2.0;
        }
        let table: serde_json::Value = serde_json::from_str(include_str!("../../data/bed_seat_edges.json")).unwrap();
        let eo: Vec<(u32, u32)> = table[format!("{},{}", self.seat_sub_x, self.seat_sub_y)].as_array().map(|a| a.iter().map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32)).collect()).unwrap_or_default();
        bevel_angle_ex(&m, c.bevel_width, 8, 30f64.to_radians(), false, if eo.is_empty() { None } else { Some(&eo) }).unwrap_or(m)
    }
}

impl BedSpec {
    /// debugging access to the frame parts by name
    pub fn part(&self, name: &str) -> Mesh {
        match name {
            "seat0" => self.seat_solid(),
            "seat" => self.make_seat(),
            "frame" => {
                let (a, b, c) = self.frame_parts(0);
                let mut m = join(&[a]);
                m.append(&join(&b));
                m.append(&join(&c));
                m.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
                m
            }
            _ => panic!("unknown part {name}"),
        }
    }
}

// ------------------------------------------------------------------------------------------------ assembly
use super::cloth::{simulate, Calib, ClothParams, Collider};
use super::clothes::{Blanket, BlanketKind};
use super::ctor_draws::{blanket_ctor, pillow_ctor, towel_material_draw};
use super::towel::{subdivide_edge_ring, TowelSpec};
use super::{inst_rng, mat, Asset, Factory, Part};
use crate::materials::spec_material;
use serde_json::json;

const FABRICS_W: Table = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];

pub struct Bed {
    pub s: BedSpec,
    pub fseed: u64,
    pub frame_only: bool,
}

pub fn make_frame(fseed: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Bed { s: BedSpec::frame(fseed), fseed, frame_only: true }))
}

pub fn make(fseed: u64) -> Option<Box<dyn Factory>> {
    let v = super::unseeded()?;
    if v.len() < 3 {
        return None;
    }
    let mut s = BedSpec::frame(fseed);
    s.sheet_type = v[0] as usize;
    s.sheet_folded = v[1] > 0.5;
    s.has_cover = v[2] > 0.5;
    Some(Box::new(Bed { s, fseed, frame_only: false }))
}

fn box_walls(lo: V3, hi: V3) -> Mesh {
    // four side walls (open top and bottom), outward normals
    let mut m = Mesh::cuboid(lo, hi).welded(1e-9);
    m.faces.retain(|f| {
        let z0 = m.pos[f[0] as usize][2];
        !f.iter().all(|&i| (m.pos[i as usize][2] - z0).abs() < 1e-12)
    });
    compact(&m)
}

/// drop unreferenced vertices
fn compact(m: &Mesh) -> Mesh {
    let mut map = vec![u32::MAX; m.pos.len()];
    let mut o = Mesh::new();
    for f in &m.faces {
        let g: Vec<u32> = f.iter().map(|&v| {
            if map[v as usize] == u32::MAX {
                map[v as usize] = o.pos.len() as u32;
                o.pos.push(m.pos[v as usize]);
            }
            map[v as usize]
        }).collect();
        o.faces.push(g);
    }
    o
}

fn clip_above(m: &Mesh, z: f64) -> Mesh {
    let mut out = Mesh::new();
    for f in &m.faces {
        let pts: Vec<V3> = f.iter().map(|&i| m.pos[i as usize]).collect();
        let mut poly: Vec<V3> = vec![];
        for k in 0..pts.len() {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            let (ina, inb) = (a[2] >= z, b[2] >= z);
            if ina {
                poly.push(a);
            }
            if ina != inb {
                let t = (z - a[2]) / (b[2] - a[2]);
                poly.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z]);
            }
        }
        if poly.len() >= 3 {
            out.add_face(&poly);
        }
    }
    out.weld(1e-9);
    out
}

/// make_coiled with its two stream draws (dot radius factor, smooth factor)
fn coil(g: &Mesh, s: &BedSpec, r: &mut Rng) -> Mesh {
    if std::env::var("BEDDBG").is_ok() {
        eprintln!("coil input {}v/{}f", g.pos.len(), g.faces.len());
    }
    let ru = r.uniform(0.06, 0.08);
    let sf = r.uniform(0.5, 1.0);
    super::coil::make_coiled(g, s.dot_distance, s.dot_depth, s.dot_size, ru, sf)
}

/// butil.modify_mesh(obj, "BEVEL", segments=4): width 0.1, angle limit 30 degrees
fn pad_bevel(m: &Mesh) -> Mesh {
    bevel_angle_ex(m, 0.1, 4, 30f64.to_radians(), false, None).unwrap_or_else(|e| {
        eprintln!("pad bevel failed: {e}");
        m.clone()
    })
}

fn ceil_cuts(size: f64, dist: f64) -> usize {
    (size / dist).ceil() as usize
}

fn divide(m: &Mesh, dist: f64, r: &mut Rng) -> Mesh {
    let (lo, hi) = m.bounds();
    divide_dims(m, [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], dist, r)
}

/// divide with explicit `obj.dimensions` (python reads stale bounds of the joined legs, before clamp / wrap)
fn divide_dims(m: &Mesh, dims: [f64; 3], dist: f64, r: &mut Rng) -> Mesh {
    let (lo, hi) = ([0.0; 3], dims);
    let mut out = m.clone();
    for i in 0..3 {
        let d = if i != 2 { dist } else { dist * r.uniform(0.5, 1.0) };
        if std::env::var("BEDDBG").is_ok() { eprintln!("divide axis {i} size {} dist {} cuts {}", hi[i] - lo[i], d, ceil_cuts(hi[i] - lo[i], d)); }
        out = subdivide_edge_ring(&out, ceil_cuts(hi[i] - lo[i], d), i);
    }
    out
}

impl BedSpec {
    fn legs(&self) -> Vec<Vec<V3>> {
        let mut legs = self.c.make_legs();
        if self.has_all_legs {
            let (w2, sz, lh) = (self.c.width / 2.0, self.c.size, self.c.leg_height);
            let starts: Vec<V3> = [[-1.0, -0.5], [0.0, -1.0], [0.0, 0.0], [1.0, -0.5]].iter().map(|p| [p[0] * w2, p[1] * sz, 0.0]).collect();
            let ends: Vec<V3> = starts.iter().map(|p| [p[0], p[1], -lh]).collect();
            legs.extend(self.c.make_limb(&starts, &ends));
        }
        legs
    }

    /// leg decor (convex hull walls of the legs): plain / legs = walls, coiled / pad = walls with the divide grid
    fn leg_decor(&self, legs: &[Vec<V3>], r: &mut Rng) -> Mesh {
        let c = &self.c;
        let zf = r.uniform(0.7, 0.9);
        let (mut x0, mut x1) = (-(c.width / 2.0 - c.leg_thickness / 2.0), c.width / 2.0 - c.leg_thickness / 2.0);
        let (mut y0, mut y1) = (-c.size + c.leg_thickness / 2.0, -c.leg_thickness / 2.0);
        if std::env::var("BEDDBG").is_ok() {
            let pts: Vec<V3> = legs.iter().flatten().cloned().collect();
            let mn = |k: usize| pts.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
            let mx = |k: usize| pts.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max);
            eprintln!("legs bounds x {} {} y {} {} z {} {}", mn(0), mx(0), mn(1), mx(1), mn(2), mx(2));
        }
        if self.leg_decor_wrapped {
            let d = c.leg_thickness / 2.0 + 1e-3;
            x0 -= d;
            x1 += d;
            y0 -= d;
            y1 += d;
        }
        let ldims = {
            let pts: Vec<V3> = legs.iter().flatten().cloned().collect();
            let ext = |k: usize| pts.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max) - pts.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
            [ext(0), ext(1), ext(2)]
        };
        let m = box_walls([x0, y0, -c.leg_height * zf], [x1, y1, 0.0]);
        if std::env::var("BEDDBG").is_ok() {
            eprintln!("leg decor box {:?} {:?} (leg_height {} zf {}) walls verts {}", [x0, y0, -c.leg_height * zf], [x1, y1, 0.0], c.leg_height, zf, m.pos.len());
        }
        match self.leg_decor {
            0 => {
                let g = divide_dims(&m, ldims, self.dot_distance, r);
                coil(&g, self, r)
            }
            1 => {
                let g = divide_dims(&m, ldims, self.panel_distance, r);
                let sel = vec![true; g.faces.len()];
                pad_bevel(&inset_individual(&g, &sel, self.panel_margin, self.panel_margin))
            }
            _ => m,
        }
    }

    /// (limb meshes, panel meshes, seat) before the final rotation
    pub fn frame_parts(&self, inst: u64) -> (Mesh, Vec<Mesh>, Vec<Mesh>) {
        let mut r = inst_rng(self.c.fseed, inst);
        self.frame_parts_with(&mut r)
    }

    pub fn frame_parts_with(&self, r: &mut Rng) -> (Mesh, Vec<Mesh>, Vec<Mesh>) {
        let c = &self.c;
        let seat = self.make_seat();
        let legs = self.legs();
        let backs = c.make_backs();
        let mut panels = vec![];
        let mut limbs = vec![];
        panels.push(self.leg_decor(&legs, r));
        let mut decors = c.make_back_decors(&backs, r);
        if self.c.back_type == 3 && decors.len() > 1 {
            limbs.push(decors.remove(1));
        }
        if self.c.bed_back <= 1 {
            // coiled / pad: the hull slab of the back rails on top of the (bisected) bridge panel
            let t = c.thickness.min(c.leg_thickness);
            let (xh, yc) = (c.width / 2.0 - c.leg_thickness / 2.0, -c.leg_thickness / 2.0);
            let slab = Mesh::cuboid([-xh, yc - t / 2.0, 0.0], [xh, yc + t / 2.0, c.back_height]).welded(1e-9);
            let mut g = if self.c.bed_back == 0 {
                let g = divide(&slab, self.dot_distance, r);
                coil(&g, self, r)
            } else {
                let g = divide(&slab, self.panel_distance, r);
                let sel: Vec<bool> = g.faces.iter().map(|f| face_normal(&g, f)[1].abs() > 0.5).collect();
                let ins = inset_individual(&g, &sel, self.panel_margin, self.panel_margin);
                if std::env::var("BEDDBG").is_ok() {
                    eprintln!("backpad: slab {} verts divided {}v/{}f inset {}v/{}f", slab.pos.len(), g.pos.len(), g.faces.len(), ins.pos.len(), ins.faces.len());
                }
                pad_bevel(&ins)
            };
            for p in g.pos.iter_mut() {
                *p = [p[0] * (1.0 - 1e-3), p[1] * (1.0 - 1e-3), p[2] * (1.0 - 1e-3)];
            }
            panels.push(g);
            decors = decors.iter().map(|m| clip_above(m, c.back_height)).collect();
        }
        if std::env::var("BEDDBG").is_ok() {
            eprintln!("rust seat {} panels {:?} decors {:?}", seat.pos.len(), panels.iter().map(|m| m.pos.len()).collect::<Vec<_>>(), decors.iter().map(|m| m.pos.len()).collect::<Vec<_>>());
        }
        panels.extend(decors);
        for l in &legs {
            limbs.push(c.solidify_limb(l, 2, c.leg_thickness));
        }
        for b in &backs {
            limbs.push(c.solidify_limb(b, 2, c.back_thickness));
        }
        (seat, limbs, panels)
    }
}

fn join(ms: &[Mesh]) -> Mesh {
    let mut o = Mesh::new();
    for m in ms {
        o.append(m);
    }
    o
}

fn face_top_z(m: &Mesh, x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for f in &m.faces {
        for t in 1..f.len() - 1 {
            let (a, b, c) = (m.pos[f[0] as usize], m.pos[f[t] as usize], m.pos[f[t + 1] as usize]);
            let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if d.abs() < 1e-14 {
                continue;
            }
            let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
            let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                best = Some(best.map_or(z, |b| b.max(z)));
            }
        }
    }
    best
}

fn quad_grid_pillow(w: f64, s: f64, pressure_h: f64, n: usize) -> Mesh {
    // inflated cushion: two sheets joined at the rim with a smooth height profile
    let mut m = Mesh::new();
    let id = |i: usize, j: usize, top: bool| ((if top { 1 } else { 0 }) * (n + 1) * (n + 1) + j * (n + 1) + i) as u32;
    for top in [false, true] {
        for j in 0..=n {
            for i in 0..=n {
                let (u, v) = (i as f64 / n as f64 * 2.0 - 1.0, j as f64 / n as f64 * 2.0 - 1.0);
                let h = pressure_h * ((1.0 - u * u) * (1.0 - v * v)).max(0.0).sqrt();
                m.pos.push([u * w / 2.0, v * s / 2.0, if top { h } else { -h }]);
            }
        }
    }
    for j in 0..n {
        for i in 0..n {
            m.faces.push(vec![id(i, j, true), id(i + 1, j, true), id(i + 1, j + 1, true), id(i, j + 1, true)]);
            m.faces.push(vec![id(i, j, false), id(i, j + 1, false), id(i + 1, j + 1, false), id(i + 1, j, false)]);
        }
    }
    m.weld(1e-9);
    m
}

impl Factory for Bed {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let c = &self.s.c;
        let (y0, y1) = (-c.size - c.leg_thickness * 0.5, 0.0);
        let mut a = Asset::default();
        a.cuboid([-y1, -c.width / 2.0, -c.leg_height], [-y0, c.width / 2.0, c.back_height * 1.2], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }

    fn asset(&self, inst: u64) -> Asset {
        let s = &self.s;
        let c = &s.c;
        let fs = c.fseed as i64;
        let m = |name: &str, cls: &str, k: i64, rgb: [f32; 3]| spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": fs + k}), rgb);
        let surf = m("bed_frame", s.frame_cls, 0, [0.45, 0.32, 0.22]);
        let panel = m("bed_panel", c.panel_cls, 1, [0.5, 0.35, 0.25]);
        let limb = m("bed_limb", s.frame_limb_cls, 2, [0.3, 0.3, 0.3]);
        let rot = Mat4::rot_z(std::f64::consts::FRAC_PI_2);
        let mut a = Asset::default();
        if std::env::var("BEDDBG").is_ok() {
            eprintln!("leg_decor {} wrapped {} bed_back {} back_type {} all_legs {} sub {}x{} frame {} limb {}", s.leg_decor, s.leg_decor_wrapped, c.bed_back, c.back_type, s.has_all_legs, s.seat_sub_x, s.seat_sub_y, s.frame_cls, s.frame_limb_cls);
        }
        let mut r = inst_rng(c.fseed, inst);
        let (seat, limbs, panels) = s.frame_parts_with(&mut r);
        let mut frame_all = Mesh::new();
        for (g, mt) in [(vec![seat], &surf), (limbs, &limb), (panels, &panel)] {
            let mut j = join(&g);
            if j.is_empty() {
                continue;
            }
            j.transform(&rot);
            frame_all.append(&j);
            let b = j.bounds();
            a.parts.push(Part { mesh: j, mat: mt.clone(), col: Some((b.0, b.1)) });
        }
        if self.frame_only {
            return a;
        }
        // ---- outer stream continues after the frame (BedFactory.create_asset)
        // mattress: MattressFactory(fseed) ctor (FixedSeed) then overrides
        let mut mr = Rng::seeded(c.fseed);
        let _w = mr.log_uniform(0.9, 2.0);
        let _sz = mr.uniform(2.0, 2.4);
        let mt = mr.uniform(0.2, 0.35);
        let dot_d = mr.log_uniform(0.16, 0.2);
        let _dot_s = mr.uniform(0.005, 0.02);
        let _dot_h = mr.uniform(0.04, 0.08);
        let _mtype = mr.choice_p(&[0.5, 0.5]);
        let mat_cls = sample_call(&mut mr, FABRICS_W);
        let coiled = r.choice_p(&[0.25, 0.75]) == 0;
        let mw = c.width * r.uniform(0.88, 0.96);
        let msz = c.size * r.uniform(0.88, 0.96);
        let mat_m = m("mattress", mat_cls, 3, [0.92, 0.92, 0.9]);
        let mut mattress = Mesh::cuboid([-mw / 2.0, -msz / 2.0, -mt / 2.0], [mw / 2.0, msz / 2.0, mt / 2.0]);
        let _ = (coiled, dot_d);
        mattress.transform(&rot);
        mattress.transform(&Mat4::translation([msz / 2.0, 0.0, mt / 2.0]));
        let mb = mattress.bounds();
        a.parts.push(Part { mesh: mattress.clone(), mat: mat_m, col: Some((mb.0, mb.1)) });
        let ztop = mt;
        // sheet
        let mut tops: Vec<Mesh> = vec![mattress.clone()];
        if s.sheet_type != 3 {
            let kind = [BlanketKind::Blanket, BlanketKind::Comforter, BlanketKind::BoxComforter][s.sheet_type];
            let bc = blanket_ctor(&mut r, s.sheet_type == 2);
            let (wm, sm) = if s.sheet_type == 0 { (r.uniform(1.4, 1.6), r.uniform(0.9, 1.1)) } else { (r.uniform(1.4, 1.8), r.uniform(0.9, 1.2)) };
            let pressure = match s.sheet_type {
                0 => 0.0,
                1 => r.uniform(1.0, 1.5),
                _ => r.log_uniform(8.0, 15.0),
            };
            let mut b = Blanket::from_unseeded(kind, &[mw * wm, msz * sm, bc[2], bc[3]]);
            b.margin = bc[3];
            let mut pre = b.create_asset();
            if s.sheet_folded {
                r.uniform(-std::f64::consts::PI / 6.0, std::f64::consts::PI / 6.0);
                r.uniform(0.1, 0.3);
            }
            let off = r.uniform(0.0, 0.15);
            pre.transform(&rot);
            pre.transform(&Mat4::translation([b.size / 2.0 + off, 0.0, ztop]));
            let mut p = ClothParams::default();
            p.mass = 0.05;
            p.tension = 2.0;
            p.distance_min = 5e-3;
            p.use_pressure = true;
            p.pressure = pressure;
            p.use_self_collision = s.sheet_folded;
            let cm = Collider::new(&mattress.pos, &mattress.faces);
            let cf = Collider::new(&frame_all.pos, &frame_all.faces);
            let out = simulate(&pre.pos, &pre.faces, &p, &[&cm, &cf], &Calib::default());
            let mut sheet = pre.clone();
            sheet.pos = out;
            let sheet = subsurf(&sheet, 2, false);
            let sm_ = m("sheet", "fabric.FineKnitFabric", 4, [0.7, 0.7, 0.75]);
            let sb = sheet.bounds();
            tops.push(sheet.clone());
            a.parts.push(Part { mesh: sheet, mat: sm_, col: Some((sb.0, sb.1)) });
        }
        let _ = (&mut r, FABRICS_W);
        // pillows
        let np = r.randint(2, 4) as usize;
        let pc = pillow_ctor(&mut r);
        let pill_mat = m("pillow", "fabric.FineKnitFabric", 10, [0.9, 0.9, 0.92]);
        let mut pts = vec![];
        let xs: Vec<f64> = (0..10).map(|_| r.uniform(0.1, 0.4) * c.size).collect();
        let ys: Vec<f64> = (0..10).map(|_| r.uniform(-0.3, 0.3) * c.width).collect();
        for k in 0..10 {
            pts.push([xs[k], ys[k]]);
        }
        let mut idx: Vec<usize> = (0..10).collect();
        r.shuffle(&mut idx);
        for &k in idx.iter().take(np) {
            let z = tops.iter().filter_map(|t| face_top_z(t, pts[k][0], pts[k][1])).fold(f64::NEG_INFINITY, f64::max);
            let z = if z.is_finite() { z } else { ztop };
            let mut pm = quad_grid_pillow(pc[1], pc[2], pc[1] * 0.12, 12);
            let rz = r.uniform(0.0, std::f64::consts::PI);
            let lowz = pm.bounds().0[2];
            pm.transform(&Mat4::rot_z(rz));
            pm.transform(&Mat4::translation([pts[k][0], pts[k][1], z + 0.02 - lowz]));
            let b = pm.bounds();
            a.parts.push(Part { mesh: pm, mat: pill_mat.clone(), col: Some((b.0, b.1)) });
        }
        // towel
        let _n_towel = 1;
        let mut ts = TowelSpec::from_stream(c.fseed, &mut r);
        let _ = towel_material_draw(&mut r);
        ts.mat_index = 0;
        let txs: Vec<f64> = (0..10).map(|_| r.uniform(0.5, 0.8) * c.size).collect();
        let tys: Vec<f64> = (0..10).map(|_| r.uniform(-0.3, 0.3) * c.width).collect();
        let mut idx: Vec<usize> = (0..10).collect();
        r.shuffle(&mut idx);
        let k = idx[0];
        let z = tops.iter().filter_map(|t| face_top_z(t, txs[k], tys[k])).fold(f64::NEG_INFINITY, f64::max);
        let z = if z.is_finite() { z } else { ztop };
        let mut tm = ts.build(inst);
        let rz = r.uniform(0.0, std::f64::consts::PI);
        let lowz = tm.bounds().0[2];
        tm.transform(&Mat4::rot_z(rz));
        tm.transform(&Mat4::translation([txs[k], tys[k], z + 0.02 - lowz]));
        let tmat = m("towel", "fabric.FineKnitFabric", 20, [0.9, 0.9, 0.9]);
        let b = tm.bounds();
        a.parts.push(Part { mesh: tm, mat: tmat, col: Some((b.0, b.1)) });
        a
    }
}
