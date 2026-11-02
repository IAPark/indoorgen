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
        // a quarter of the beds are single beds (1.0-1.5 x 1.9-2.2 m) so small bedrooms get one (dense mode only before: without it a
        // 3 x 3 m bedroom took no 1.4-2.4 m bed and stayed empty); NO_SINGLE_BEDS=1: python's sizes outside dense mode
        if fseed % 4 == 0 && (ig_core::envx::dense_env() || std::env::var_os("NO_SINGLE_BEDS").is_none()) {
            c.width = 1.0 + (c.width - 1.4) / 1.0 * 0.5;
            c.size = 1.9 + (c.size - 2.0) * 0.75;
        }
        // (a tight spot's last resort: Piece::tucked asked twice)
        if crate::api::tuck_hint() >= 2 {
            // (the mattress: the frame adds ~12 cm across and 10 cm along, and 1.07 x 2.0 missed an angled room by 10 cm)
            c.width = 0.85;
            c.size = 1.85;
        }
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
        let table: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/bed_seat_edges.json"))).unwrap();
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
use super::mattress::{sim_bedding, MattressSpec};
use super::pillow::PillowSpec;
use super::cloth::{simulate_job, Calib, ClothParams, Collider};
use super::clothes::{Blanket, BlanketKind};
use super::ctor_draws::{blanket_ctor, pillow_ctor, towel_material_draw};
use super::towel::{subdivide_edge_ring, TowelSpec};
use super::{inst_rng, mat, Asset, ClothJob, Factory, Follower, Part};
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
    // the flat "blanket" cover (type 0) settles as a stiff panel in the cloth sim; 70 % of those beds get a comforter instead (own hash,
    // the ctor stream is unchanged; NO_COMFORTER_BIAS=1 off)
    if s.sheet_type == 0 && std::env::var_os("NO_COMFORTER_BIAS").is_none() && crate::int_hash(&["bed_comforter", &fseed.to_string()]) % 10 < 7 {
        s.sheet_type = 1;
    }
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

/// bpy.ops.mesh.bisect(plane_co=(0,0,z), plane_no=(0,0,1), clear_inner=True) (threshold 1e-4): vertices within the threshold are "on the plane"
/// and kept; only edges with endpoints strictly on opposite sides are cut (factor d_a / (d_a - d_b) in f32); faces entirely on the inner side go.
pub fn clip_above(m: &Mesh, z: f64) -> Mesh {
    let zf = z as f32;
    let eps = 1e-4f32;
    let side = |p: V3| -> i32 {
        let d = p[2] as f32 - zf;
        if d.abs() < eps { 0 } else if d > 0.0 { 1 } else { -1 }
    };
    let mut out = Mesh::new();
    let mut vmap: crate::dhash::HashMap<u32, u32> = crate::dhash::HashMap::default();
    let mut emap: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    for f in &m.faces {
        let k = f.len();
        let sd: Vec<i32> = f.iter().map(|&i| side(m.pos[i as usize])).collect();
        if sd.iter().all(|&x| x == 0) {
            // flat on the plane: kept untouched
        } else if sd.iter().all(|&x| x <= 0) {
            continue;
        }
        let mut poly: Vec<u32> = vec![];
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let (sa, sb) = (sd[i], sd[(i + 1) % k]);
            if sa >= 0 {
                let id = *vmap.entry(a).or_insert_with(|| {
                    out.pos.push(m.pos[a as usize]);
                    (out.pos.len() - 1) as u32
                });
                poly.push(id);
            }
            if sa * sb < 0 {
                let key = (a.min(b), a.max(b));
                let id = *emap.entry(key).or_insert_with(|| {
                    // interpolate from the lower index endpoint (edge v1 -> v2 order of a Blender mesh is not known here)
                    let (v1, v2) = (m.pos[key.0 as usize], m.pos[key.1 as usize]);
                    let (da, db) = (v1[2] as f32 - zf, v2[2] as f32 - zf);
                    let fac = da / (da - db);
                    let c = |q: usize| (v1[q] as f32 + (v2[q] as f32 - v1[q] as f32) * fac) as f64;
                    out.pos.push([c(0), c(1), c(2)]);
                    (out.pos.len() - 1) as u32
                });
                poly.push(id);
            }
        }
        poly.dedup();
        if poly.len() >= 2 && poly.first() == poly.last() {
            poly.pop();
        }
        if poly.len() >= 3 {
            out.faces.push(poly);
        }
    }
    out
}

/// make_coiled with its two stream draws (dot radius factor, smooth factor)
fn coil(g: &Mesh, s: &BedSpec, r: &mut Rng) -> Mesh {
    if std::env::var("BEDDBG").is_ok() {
        eprintln!("coil input {}v/{}f", g.pos.len(), g.faces.len());
    }
    let ru = r.uniform(0.06, 0.08);
    let sf = r.uniform(0.5, 1.0);
    if std::env::var("COILPARAMS").is_ok() { eprintln!("COILPARAMS dd {} dep {} ds {} ru {} sf {}", s.dot_distance, s.dot_depth, s.dot_size, ru, sf); }
    if std::env::var("COIL_F64").is_err() {
        if let Some(m) = super::coilx::make_coiled_f32(g, s.dot_distance, s.dot_depth, s.dot_size, ru, sf, None) {
            return m;
        }
    }
    super::coil::make_coiled(g, s.dot_distance, s.dot_depth, s.dot_size, ru, sf)
}

/// butil.modify_mesh(obj, "BEVEL", segments=4): width 0.1, angle limit 30 degrees
fn pad_bevel(m: &Mesh) -> Mesh {
    if std::env::var("PAD_OLD").is_err() {
        match super::bevmod::bevel_modifier(m, 0.1, 4, Some(30f64.to_radians() as f32), true, 0.5) {
            Ok(r) => return r,
            Err(e) => eprintln!("pad bevmod failed: {e}"),
        }
    }
    bevel_angle_ex(m, 0.1, 4, 30f64.to_radians(), false, None).unwrap_or_else(|e| {
        eprintln!("pad bevel failed: {e}");
        m.clone()
    })
}

/// divide for the back slab: garbled hulls are left undivided (the z draw is still consumed)
fn back_divide(m: &Mesh, dist: f64, garbled: bool, r: &mut Rng) -> Mesh {
    if garbled {
        r.uniform(0.5, 1.0);
        m.clone()
    } else {
        divide(m, dist, r)
    }
}

static CLEAN_GEOMETRY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Scene export mode: repair python's own geometry defects (garbled flat hulls) instead of reproducing them (refcmp keeps the bug-compatible default).
pub fn set_clean_geometry(on: bool) {
    CLEAN_GEOMETRY.store(on, std::sync::atomic::Ordering::Relaxed);
}
fn clean_geometry() -> bool {
    CLEAN_GEOMETRY.load(std::sync::atomic::Ordering::Relaxed)
}

/// If all points lie in one axis-aligned plane (extent < 1e-4 along one axis), the strict 2D convex hull as a single n-gon.
fn flat_hull_polygon(hp: &[[f32; 3]]) -> Option<(Vec<[f32; 3]>, Vec<[u32; 2]>, Vec<Vec<u32>>)> {
    if hp.len() < 3 {
        return None;
    }
    let ext = |k: usize| {
        let lo = hp.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min);
        let hi = hp.iter().map(|p| p[k]).fold(f32::NEG_INFINITY, f32::max);
        hi - lo
    };
    let ax = (0..3).min_by(|&a, &b| ext(a).partial_cmp(&ext(b)).unwrap()).unwrap();
    if ext(ax) > 1e-4 {
        return None;
    }
    let (u, v) = match ax { 0 => (1, 2), 1 => (0, 2), _ => (0, 1) };
    let mut idx: Vec<usize> = (0..hp.len()).collect();
    idx.sort_by(|&a, &b| (hp[a][u], hp[a][v]).partial_cmp(&(hp[b][u], hp[b][v])).unwrap());
    let cr = |o: usize, a: usize, b: usize| (hp[a][u] - hp[o][u]) as f64 * (hp[b][v] - hp[o][v]) as f64 - (hp[a][v] - hp[o][v]) as f64 * (hp[b][u] - hp[o][u]) as f64;
    let mut h: Vec<usize> = vec![];
    for pass in 0..2 {
        let start = h.len();
        let it: Vec<usize> = if pass == 0 { idx.clone() } else { idx.iter().rev().cloned().collect() };
        for i in it {
            while h.len() >= start + 2 && cr(h[h.len() - 2], h[h.len() - 1], i) <= 1e-9 {
                h.pop();
            }
            h.push(i);
        }
        h.pop();
    }
    if h.len() < 3 {
        return None;
    }
    let n = h.len() as u32;
    let pts: Vec<[f32; 3]> = h.iter().map(|&i| hp[i]).collect();
    let edges: Vec<[u32; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();
    Some((pts, edges, vec![(0..n).collect()]))
}

/// make_back: convex hull (exact bmesh port) of the joined back posts, SOLIDIFY (thickness t, offset 0), normals made outward
fn back_hull_slab(backs: &[Vec<V3>], t: f64) -> Mesh {
    use crate::bm::BMesh;
    let mut pos: Vec<[f32; 3]> = vec![];
    let mut edges: Vec<[u32; 2]> = vec![];
    for b in backs {
        let base = pos.len() as u32;
        for (i, p) in b.iter().enumerate() {
            pos.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            if i > 0 {
                edges.push([base + i as u32 - 1, base + i as u32]);
            }
        }
    }
    let mut bm = BMesh::from_mesh(&pos, &edges, &[], &[]);
    crate::bm::hull::convex_hull(&mut bm, &crate::bm::hull::HullParams::default());
    let (mut hp, mut he, mut hf) = bm.to_mesh();
    // The posts' points are coplanar (float noise 1e-7 only): bmesh's hull of a flat point set is a pile of overlapping, flipped
    // triangles (python has the same defect: torn zig-zag headboards after divide()). Replace it by the flat convex polygon.
    if clean_geometry() {
        if let Some((p2, e2, f2)) = flat_hull_polygon(&hp) {
            hp = p2;
            he = e2;
            hf = f2;
        }
    }
    if let Ok(p) = std::env::var("BEDDUMP4") {
        std::fs::write(p, serde_json::json!({"verts": hp.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect::<Vec<_>>(), "polys": hf}).to_string()).ok();
    }
    let mut emap: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    for (i, e) in he.iter().enumerate() {
        emap.insert((e[0].min(e[1]), e[0].max(e[1])), i as u32);
    }
    let ce: Vec<Vec<u32>> = hf.iter().map(|f| (0..f.len()).map(|k| emap[&(f[k].min(f[(k + 1) % f.len()]), f[k].max(f[(k + 1) % f.len()]))]).collect()).collect();
    let s = super::solidx::solidify_extrude(&hp, &he, &hf, &ce, t as f32, 0.0, true);
    // normals_make_consistent (Blender's outer-most-loop criterion) on a BMesh of the solid
    let mut sbm = crate::bm::BMesh::from_mesh(&s.pos, &s.edges, &s.faces, &s.corner_edges);
    super::coilx::recalc_face_normals_bm(&mut sbm);
    let (sp, se, sf) = sbm.to_mesh();
    let mut m = Mesh::new();
    m.pos = sp.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    m.faces = sf;
    let _ = se;
    m
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
    pub fn legs_pub(&self) -> Vec<Vec<V3>> {
        self.legs()
    }
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
        let m = box_walls([x0, y0, -c.leg_height * zf], [x1, y1, 0.0]);
        if std::env::var("BEDDBG").is_ok() {
            eprintln!("leg decor box {:?} {:?} (leg_height {} zf {}) walls verts {}", [x0, y0, -c.leg_height * zf], [x1, y1, 0.0], c.leg_height, zf, m.pos.len());
        }
        match self.leg_decor {
            0 => {
                let g = divide(&m, self.dot_distance, r);
                if let Ok(p) = std::env::var("BEDDUMP") {
                    let j = serde_json::json!({"verts": g.pos, "polys": g.faces});
                    std::fs::write(p, j.to_string()).ok();
                }
                coil(&g, self, r)
            }
            1 => {
                let g = divide(&m, self.panel_distance, r);
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
        // posts with many (float-noisy, collinear) points make python's convex hull degenerate: its edge rings are not axis parallel,
        // so divide() subdivides (almost) nothing there; model that as an undivided slab
        if std::env::var("BEDDBG").is_ok() { eprintln!("backs n {:?} xspread {:?}", backs.iter().map(|b| b.len()).collect::<Vec<_>>(), backs.iter().map(|b| b.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) - b.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min)).collect::<Vec<_>>()); }
        let _garbled0 = 0;
        if self.c.bed_back <= 1 {
            // coiled / pad: the hull slab of the back rails on top of the (bisected) bridge panel
            let t = c.thickness.min(c.leg_thickness);
            let (xh, yc) = (c.width / 2.0 - c.leg_thickness / 2.0, -c.leg_thickness / 2.0);
            let slab = if std::env::var("BED_OLD_SLAB").is_ok() {
                Mesh::cuboid([-xh, yc - t / 2.0, 0.0], [xh, yc + t / 2.0, c.back_height]).welded(1e-9)
            } else {
                back_hull_slab(&backs, t)
            };
            let garbled = false;
            if let Ok(p) = std::env::var("BEDDUMP3") {
                std::fs::write(p, serde_json::json!({"verts": slab.pos, "polys": slab.faces}).to_string()).ok();
            }
            let mut g = if self.c.bed_back == 0 {
                let g = back_divide(&slab, self.dot_distance, garbled, r);
                if let Ok(p) = std::env::var("BEDDUMP2") {
                    std::fs::write(p, serde_json::json!({"verts": g.pos, "polys": g.faces}).to_string()).ok();
                }
                coil(&g, self, r)
            } else {
                let g = back_divide(&slab, self.panel_distance, garbled, r);
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
    /// collision: the frame and mattress measured once on the layout build (it stops at the mattress: no bedding simulated), and the
    /// bedding as a soft volume over the mattress -- sheet and cover hang down its sides and foot, pillows stand on it
    /// collision from the spec: the frame (bed.py's box: the seat's depth and width, the legs below, the headboard up to 1.2 of the
    /// back's height), the mattress on it (its block and puff), the bedding as soft
    /// (round the mattress by the sheet's overhang but the head end and down to the floor, pillows up to 0.42 m up)
    fn volumes(&self, inst: u64) -> Option<Vec<crate::piece::Volume>> {
        use crate::piece::Volume;
        let c = &self.s.c;
        let g = 0.004;
        // (behind the frame: an upholstered headboard's padding up to 2.5 cm, the bedding pressed against it up to 5 cm -- one back plane
        // for both, against the wall; the legs' decor panels 2 cm out at the sides)
        let sd = 0.02;
        let (lo, hi) = ([-0.05, -c.width / 2.0 - sd, -c.leg_height - g], [c.size + c.leg_thickness * 0.5 + g, c.width / 2.0 + sd, c.back_height * 1.2 + g]);
        let mut v = vec![Volume::hard(lo, hi)];
        // (the headboard's panels can stand wider than the frame)
        v.push(Volume::hard([lo[0], -c.width / 2.0 - 0.06, 0.0], [0.15, c.width / 2.0 + 0.06, hi[2]]));
        if !self.frame_only {
            let mut r = bedding_rng(c.fseed, inst);
            let ms = bed_mattress(c, &mut r);
            let _ = blanket_ctor(&mut r, self.s.sheet_type >= 2);
            let (wm, sm) = sheet_scale(self.s.sheet_type, &mut r);
            let (a, b) = ms.bounds();
            // (turned a quarter, its centre at (size / 2, 0, thickness / 2))
            let (ml, mh) = ([c.size / 2.0 - b[1], a[0], a[2] + ms.thickness / 2.0], [c.size / 2.0 - a[1], b[0], b[2] + ms.thickness / 2.0]);
            v.push(Volume::hard([ml[0] - g, ml[1] - g, ml[2] - g], [mh[0] + g, mh[1] + g, mh[2] + g]));
            // the bedding hangs over the mattress (drape): it reaches as far as the sheet's overhang and its slide toward the foot carry it
            // (a simulated one spreads up to 0.45 m), not past the headboard, which holds it at the head end
            // (tucked in, it keeps within the frame: the soft volume comes in to the hard box)
            let tuck = crate::api::tuck_hint() > 0;
            let (side, foot) = if tuck { (hi[1] - mh[1], hi[0] - mh[0]) } else if sim_bedding() { (0.45, 0.45) } else {
                let gm = DrapeGeom::of(c, &ms);
                let lift = drape_loft(self.s.sheet_type) + 0.012;
                let (ov_s, ov_f) = ((wm - 1.0) / 2.0 * ms.width, (0.15 + sm * ms.size - gm.x1).max(0.0));
                let ov = ov_s.hypot(ov_f);
                let [sl, yaw] = drape_slide(c.fseed, inst, 0);
                // (slid and turned on the bed: at most its half length times the turn)
                let off = sl.abs() + yaw.abs() * (ms.size * sm + ms.width * wm) / 2.0;
                (gm.reach(ov, gm.gap_side, lift) + off, gm.reach(ov, gm.gap_foot, lift) + off)
            };
            v.push(Volume::soft([lo[0], ml[1] - side, lo[2]], [mh[0] + foot, mh[1] + side, mh[2] + 0.42]));
            v.push(Volume::support([ml[0] + 0.05, ml[1] + 0.05, ms.thickness - 0.001], [mh[0] - 0.05, mh[1] - 0.05, ms.thickness]));
        }
        Some(v)
    }
    fn placeholder_exact(&self, _i: u64) -> Option<super::cabnodes::ExactMesh> {
        let c = &self.s.c;
        Some(super::chair::rotated_bbox_exact(-c.width / 2.0, c.width / 2.0, -c.size - c.leg_thickness * 0.5, 0.0, -c.leg_height, c.back_height * 1.2))
    }

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
        let (seat, mut limbs, mut panels) = s.frame_parts_with(&mut r);
        // upholstered headboards (half the beds, own stream): each panel becomes 3-6 vertical padded channels over its footprint, in fabric
        let ukey = format!("{}:{}", self.fseed, inst);
        let upholstered = std::env::var_os("NO_UPHOLSTERED_BED").is_none() && crate::int_hash(&["bed_upholstered_p", &ukey]) % 2 == 0;
        let panel = if upholstered {
            {
                // fabric by hash (independent of the shared stream: the first draws of nearby seeds were correlated)
                let h = crate::int_hash(&["bed_headboard_fab", &ukey]) as usize;
                let cls = ["fabric.SofaFabric", "fabric.SofaFabric", "fabric.Velvet", "fabric.CoarseKnitFabric", "fabric.Leather", "fabric.SofaFabric", "fabric.Velvet"][h % 7];
                spec_material(&format!("bed_headboard:{}", h % 1_000_000), json!({"cls": cls, "kwargs": {}, "seed": (h % 1_000_000) as i64}), [0.55, 0.52, 0.5])
            }
        } else { panel };
        if upholstered {
            // replace the headboard (the tallest panel: the back slab / decor rising above the frame) by vertical padded channels over its
            // footprint; other panels (leg decor) stay
            let seat_top = seat.bounds().1[2];
            let tall: Vec<usize> = (0..panels.len()).filter(|&i| panels[i].bounds().1[2] > seat_top + 0.2).collect();
            if !tall.is_empty() {
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for &i in &tall { let (l, h) = panels[i].bounds(); for d in 0..3 { lo[d] = lo[d].min(l[d]); hi[d] = hi[d].max(h[d]); } }
                lo[2] = lo[2].max(seat_top - 0.05);
                let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                let thin = if ext[0] < ext[1] { 0 } else { 1 };
                let long = 1 - thin;
                let n = ((ext[long] / 0.3).round() as usize).clamp(3, 7);
                let t = ext[thin].max(0.08);
                let c0 = (lo[thin] + hi[thin]) / 2.0;
                let mut keep: Vec<Mesh> = (0..panels.len()).filter(|i| !tall.contains(i)).map(|i| panels[i].clone()).collect();
                for k in 0..n {
                    let (mut plo, mut phi) = (lo, hi);
                    plo[thin] = c0 - t / 2.0; phi[thin] = c0 + t / 2.0;
                    plo[long] = lo[long] + ext[long] * k as f64 / n as f64 + 0.004;
                    phi[long] = lo[long] + ext[long] * (k + 1) as f64 / n as f64 - 0.004;
                    keep.push(super::sofa::pad(plo, phi, 0.04, 2));
                }
                panels = keep;
            }
        }
        let _ = &mut limbs;
        let mut frame_all = Mesh::new();
        let mut frame_idx: Vec<usize> = vec![];
        for (g, mt) in [(vec![seat], &surf), (limbs, &limb), (panels, &panel)] {
            let mut j = join(&g);
            if j.is_empty() {
                continue;
            }
            j.transform(&rot);
            frame_all.append(&j);
            let b = j.bounds();
            frame_idx.push(a.parts.len());
            a.parts.push(Part { mesh: j, mat: mt.clone(), col: Some((b.0, b.1)) });
        }
        if self.frame_only {
            return a;
        }
        // ---- outer stream continues after the frame (BedFactory.create_asset)
        // ---- mattress (MattressFactory: its own FixedSeed(fseed) ctor, type / width / size overridden from the bed stream)
        // (from here its own stream: the collision replays these draws without building the frame)
        let mut r = bedding_rng(c.fseed, inst);
        let ms = bed_mattress(c, &mut r);
        let (mut mattress, mjob) = ms.build_job(&mut inst_rng(c.fseed, inst));
        mattress.transform(&rot);
        mattress.transform(&Mat4::translation([c.size / 2.0, 0.0, ms.thickness / 2.0]));
        let mattress_idx = a.parts.len();
        if let Some((mut pre, pressure, pin)) = mjob {
            pre.transform(&rot);
            pre.transform(&Mat4::translation([c.size / 2.0, 0.0, ms.thickness / 2.0]));
            a.cloth.push(ClothJob { target: mattress_idx, pre, collide: vec![], kwargs: json!({"gravity": 0, "use_pressure": true, "uniform_pressure_force": pressure, "vertex_group_mass": "pin"}), end_frame: 50, subsurf: 0, pin, z_from: None, build: None, post: None });
        }
        let mat_m = m("mattress", ms.mat_cls, 3, [0.92, 0.92, 0.9]);
        let mb = mattress.bounds();
        a.parts.push(Part { mesh: mattress.clone(), mat: mat_m, col: Some((mb.0, mb.1)) });
        // a layout build (envx::set_layout_build) ends here: the bedding's two cloth simulations are most of a bed's build; in their
        // place a box over the mattress -- 3 cm each side where sheet and cover hang, 15 cm up for them and the pillows
        if ig_core::envx::layout_build() {
            let (lo, hi) = ([mb.0[0] - 0.03, mb.0[1] - 0.03, mb.1[2] - 0.02], [mb.1[0] + 0.03, mb.1[1] + 0.03, mb.1[2] + 0.15]);
            a.parts.push(Part { mesh: Mesh::cuboid(lo, hi), mat: m("bedding_layout", "fabric.FineKnitFabric", 4, [0.7, 0.7, 0.75]), col: Some((lo, hi)) });
            return a;
        }
        let (mw, msz) = (ms.width, ms.size);
        let cm = Collider::new(&mattress.pos, &mattress.faces);
        let cf = Collider::new(&frame_all.pos, &frame_all.faces);
        // the floor under the bed: hanging cloth must not sink into it
        let floor_c = {
            let zf = frame_all.bounds().0[2];
            Collider::new(&[[-20.0, -20.0, zf], [20.0, -20.0, zf], [20.0, 20.0, zf], [-20.0, 20.0, zf]], &[vec![0, 1, 2, 3]])
        };
        let fabric = m("sheet", "fabric.FineKnitFabric", 4, [0.7, 0.7, 0.75]);

        // ---- sheet: "none" falls into python's `case _` = box comforter
        let kind = [BlanketKind::Blanket, BlanketKind::Comforter, BlanketKind::BoxComforter, BlanketKind::BoxComforter][s.sheet_type];
        let bc = blanket_ctor(&mut r, s.sheet_type >= 2);
        let (wm, sm) = sheet_scale(s.sheet_type, &mut r);
        // `py_pressure`: python's draw, what Blender's cloth sim gets in the job; `pressure`: the Rust solver's (its calibration
        // differs: a filled comforter at 1.0-1.5 lofted no more than a sheet; PUFF_SCALE overrides). Blender given the boosted value
        // blew a folded comforter up into a half-metre lump over the pillows.
        let py_pressure = match s.sheet_type {
            0 => 0.0,
            1 => r.uniform(1.0, 1.5),
            _ => r.log_uniform(8.0, 15.0),
        };
        let pressure = if s.sheet_type == 1 { py_pressure * std::env::var("PUFF_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(2.6) } else { py_pressure };
        let mut b = Blanket::from_unseeded(kind, &[mw * wm, msz * sm, bc[2], bc[3]]);
        b.margin = bc[3];
        let mut pre = b.create_asset();
        let mut fold_p = serde_json::Value::Null;
        if s.sheet_folded {
            let theta = r.uniform(-std::f64::consts::PI / 6.0, std::f64::consts::PI / 6.0);
            let ym = b.size * (0.5 - r.uniform(0.1, 0.3));
            fold_p = json!({"theta": theta, "y_margin": ym});
            pre = fold(&pre, theta, ym);
        }
        let z_sheet = mattress.bounds().1[2];
        let off = r.uniform(0.0, 0.15);
        pre.transform(&rot);
        pre.transform(&Mat4::translation([b.size / 2.0 + off, 0.0, z_sheet]));
        let sim = sim_bedding();
        let geom = DrapeGeom::of(c, &ms);
        let pleats = Pleats::new(&mut r);
        let sheet = if !sim { drape(&pre, &geom, 0.0, drape_loft(s.sheet_type), if s.sheet_type >= 2 { b.margin } else { 0.0 }, &pleats, drape_slide(c.fseed, inst, 0), 0.0, 1.0) } else {
        let mut p = ClothParams::default();
        p.mass = 0.05;
        p.tension = 2.0;
        p.distance_min = 5e-3;
        p.use_pressure = true;
        p.pressure = pressure;
        p.use_self_collision = s.sheet_folded;
        // python sets tension 2 but keeps Blender's compression 15 / bending 0.5: the overhang hung as stiff vertical panels; a quilted
        // comforter is soft in compression and bending (SOFT_DRAPE=0 keeps the old values)
        if std::env::var("SOFT_DRAPE").map_or(true, |v| v != "0") {
            p.compression = 2.0;
            p.bending = std::env::var("DRAPE_BEND").ok().and_then(|v| v.parse().ok()).unwrap_or(0.12);
        }
        let mut cal = Calib::default();
        cal.press_scale *= std::env::var("SHEETP").ok().and_then(|v| v.parse().ok()).unwrap_or(0.85);
        let out = simulate_job(&pre.pos, &pre.faces, &p, &[&cm, &cf, &floor_c], &cal);
        let mut sheet = pre.clone();
        sheet.pos = out;
        subsurf(&sheet, 2, false)
        };
        let sb = sheet.bounds();
        let sheet_idx = a.parts.len();
        let btype = ["blanket", "comforter", "box", "box"][s.sheet_type];
        if sim {
        let mut collide = vec![mattress_idx];
        collide.extend(frame_idx.iter().cloned());
        a.cloth.push(ClothJob { target: sheet_idx, pre: pre.clone(), collide, kwargs: json!({"mass": 0.05, "tension_stiffness": 2, "distance_min": 0.005, "use_pressure": true, "uniform_pressure_force": py_pressure, "use_self_collision": s.sheet_folded}), end_frame: 50, subsurf: 2, pin: vec![], z_from: Some((mattress_idx, z_sheet)), build: Some(json!({"type": btype, "width": b.width, "size": b.size, "thickness": b.thickness, "margin": b.margin, "fold": fold_p, "x": b.size / 2.0 + off})), post: None });
        }
        a.soft.push(a.parts.len());
        a.parts.push(Part { mesh: sheet.clone(), mat: fabric.clone(), col: Some((sb.0, sb.1)) });

        // ---- cover (always made: python never reads has_cover); 80 frames, no pressure
        let cc = blanket_ctor(&mut r, false);
        let (cw0, cs0) = (r.uniform(1.6, 1.8), r.uniform(0.3, 0.4));
        // a bed runner / throw overhangs the sides by ~10-20 cm (infinigen's 1.6-1.8 x width hung stiffly to the floor on both sides)
        let (cw, cs) = if std::env::var_os("NO_THROW_FIT").is_none() { (mw * (1.0 + (cw0 - 1.6) * 1.2 + 0.12), msz * (cs0 * 0.8)) } else { (mw * cw0, msz * cs0) };
        let cb = Blanket::from_unseeded(BlanketKind::Blanket, &[cw, cs, cc[2], 0.0]);
        let mut cover = cb.create_asset();
        let z_cover = sheet.bounds().1[2];
        let off = r.uniform(0.0, 0.3);
        cover.transform(&rot);
        cover.transform(&Mat4::translation([c.size / 2.0 + off, 0.0, z_cover]));
        let cvm = if !sim { drape(&cover, &geom, drape_loft(s.sheet_type) + 0.008, 0.0, 0.0, &pleats, drape_slide(c.fseed, inst, 1), 0.0, 1.0) } else {
        // the comforter is a thin two-layer shell: the cover only sees its outer (top) layer, else vertices that sink between the layers pop out below it
        let top_faces = {
            let k = pre.faces.iter().take_while(|f| {
                let (a, b, c) = (pre.pos[f[0] as usize], pre.pos[f[1] as usize], pre.pos[f[2] as usize]);
                (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0
            }).count();
            if k == pre.faces.len() || k == 0 { sheet.faces.len() } else { (k * 16).min(sheet.faces.len()) }
        };
        let cs_ = Collider::new(&sheet.pos, &sheet.faces[..top_faces]);
        if std::env::var_os("BEDDBG").is_some() {
            let nz = |fs: &[Vec<u32>]| -> f64 { fs.iter().map(|f| { let (a, b, c) = (sheet.pos[f[0] as usize], sheet.pos[f[1] as usize], sheet.pos[f[2] as usize]); let (u, v) = ([b[0]-a[0], b[1]-a[1], b[2]-a[2]], [c[0]-a[0], c[1]-a[1], c[2]-a[2]]); let n = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]; n[2] / (n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt().max(1e-12) }).sum::<f64>() / fs.len().max(1) as f64 };
            eprintln!("TOPFACES {} of {} (pre {}): mean nz top {:.3} rest {:.3}", top_faces, sheet.faces.len(), pre.faces.len(), nz(&sheet.faces[..top_faces]), nz(&sheet.faces[top_faces..]));
        }
        let mut p = ClothParams::default();
        p.mass = 0.05;
        p.tension = 2.0;
        p.distance_min = 5e-3;
        p.end_frame = 80;
        p.distance_min = std::env::var("COVER_DMIN").ok().and_then(|v| v.parse().ok()).unwrap_or(5e-3);
        let out = simulate_job(&cover.pos, &cover.faces, &p, &[&cs_, &cm, &floor_c], &Calib::default());
        let mut cvm = cover.clone();
        cvm.pos = out;
        subsurf(&cvm, 2, false)
        };
        let cbb = cvm.bounds();
        if sim {
        a.cloth.push(ClothJob { target: a.parts.len(), pre: cover.clone(), collide: vec![sheet_idx, mattress_idx], kwargs: json!({"mass": 0.05, "tension_stiffness": 2, "distance_min": 0.005}), end_frame: 80, subsurf: 2, pin: vec![], z_from: Some((sheet_idx, z_cover)), build: Some(json!({"type": "blanket", "width": cb.width, "size": cb.size, "thickness": cb.thickness, "margin": 0.0, "fold": null, "x": c.size / 2.0 + off})), post: None });
        }
        let cover_final = cvm.clone();
        a.soft.push(a.parts.len());
        a.parts.push(Part { mesh: cvm, mat: fabric.clone(), col: Some((cbb.0, cbb.1)) });

        // ---- pillows
        let bases = [&sheet, &mattress];
        // soft bodies rest on everything that is on the bed (sheet, mattress, cover)
        let rest_col = {
            let mut pos = vec![];
            let mut faces: Vec<Vec<u32>> = vec![];
            for mm in [&sheet, &mattress, &cover_final] {
                let o = pos.len() as u32;
                pos.extend_from_slice(&mm.pos);
                faces.extend(mm.faces.iter().map(|f| f.iter().map(|i| i + o).collect()));
            }
            Collider::new(&pos, &faces)
        };
        let mut rests: Vec<(usize, f64, f64, f64)> = vec![];
        let (sheet_part, cover_part) = (sheet_idx, a.parts.len() - 1);
        let np = r.randint(2, 4) as usize;
        let pc = pillow_ctor(&mut r);
        let pspec = PillowSpec::from_ctor(&pc);
        let pillow = pspec.build(&mut inst_rng(c.fseed, inst));
        let pill_mat = m("pillow", "fabric.FineKnitFabric", 10, [0.9, 0.9, 0.92]);
        let xs: Vec<f64> = (0..10).map(|_| r.uniform(0.1, 0.4) * c.size).collect();
        let ys: Vec<f64> = (0..10).map(|_| r.uniform(-0.3, 0.3) * c.width).collect();
        // pillows in a row against the headboard (x small), long side across the bed, slightly overlapping and tilted; the scatter's
        // draws are kept so the rest of the bed stream is unchanged (NO_PILLOW_ROW=1: infinigen's random scatter)
        let row = std::env::var_os("NO_PILLOW_ROW").is_none();
        let scattered = scatter(&bases, &xs, &ys, np, &mut r);
        let (plo, phi) = pillow.bounds();
        let (pl, pw) = ((phi[0] - plo[0]).max(phi[1] - plo[1]), (phi[0] - plo[0]).min(phi[1] - plo[1]));
        let long_x = phi[0] - plo[0] >= phi[1] - plo[1];
        let usable = c.width * 0.92;
        let per = (usable / pl).floor().max(1.0) as usize;
        let n_row = np.min(per.max(1)).min(3);
        let locs: Vec<[f64; 3]> = if row {
            (0..n_row).map(|i| {
                let y = -usable / 2.0 + usable * (i as f64 + 0.5) / n_row as f64;
                let x = c.size * 0.02 + pw * 0.5 + 0.03;
                let z = bases.iter().filter_map(|t| face_top_z(t, x, y)).fold(f64::NEG_INFINITY, f64::max);
                [x, y, if z.is_finite() { z } else { scattered.first().map_or(0.5, |l| l[2]) }]
            }).collect()
        } else { scattered.clone() };
        for (pi, loc) in locs.iter().cloned().enumerate() {
            let mut pm = pillow.clone();
            let rz_s = r.uniform(0.0, std::f64::consts::PI);
            // long side along y: rotate 90 deg if the pillow's long side is x; small per-pillow jitter
            let rz = if row { (if long_x { std::f64::consts::FRAC_PI_2 } else { 0.0 }) + (rz_s - std::f64::consts::FRAC_PI_2) * 0.04 } else { rz_s };
            let _ = pi;
            let lowz = pm.bounds().0[2];
            pm.transform(&Mat4::rot_z(rz));
            pm.transform(&Mat4::translation([loc[0], loc[1], loc[2] + 0.02 - lowz]));
            super::cloth::rest_and_wrap(&mut pm.pos, &rest_col, 0.07, 0.01, 0.15);
            let b = pm.bounds();
            a.followers.push(Follower { part: a.parts.len(), xy: [loc[0], loc[1]], z_used: loc[2], bases: vec![sheet_idx, mattress_idx] });
            // (pillows and the towel are bedding: soft, within the bed's soft volume)
            rests.push((a.parts.len(), 0.07, 0.01, 0.15));
            a.soft.push(a.parts.len());
            a.parts.push(Part { mesh: pm, mat: pill_mat.clone(), col: Some((b.0, b.1)) });
        }
        // ---- towel
        let mut ts = TowelSpec::from_stream(c.fseed, &mut r);
        let _ = towel_material_draw(&mut r);
        ts.mat_index = 0;
        let tm0 = ts.build(inst);
        let txs: Vec<f64> = (0..10).map(|_| r.uniform(0.5, 0.8) * c.size).collect();
        let tys: Vec<f64> = (0..10).map(|_| r.uniform(-0.3, 0.3) * c.width).collect();
        for loc in scatter(&bases, &txs, &tys, 1, &mut r) {
            let mut tm = tm0.clone();
            let rz = r.uniform(0.0, std::f64::consts::PI);
            let lowz = tm.bounds().0[2];
            tm.transform(&Mat4::rot_z(rz));
            tm.transform(&Mat4::translation([loc[0], loc[1], loc[2] + 0.02 - lowz]));
            super::cloth::rest_and_wrap(&mut tm.pos, &rest_col, 0.02, 0.005, 0.1);
            let tmat = m("towel", "fabric.FineKnitFabric", 20, [0.9, 0.9, 0.9]);
            let b = tm.bounds();
            a.followers.push(Follower { part: a.parts.len(), xy: [loc[0], loc[1]], z_used: loc[2], bases: vec![sheet_idx, mattress_idx] });
            rests.push((a.parts.len(), 0.02, 0.005, 0.1));
            a.soft.push(a.parts.len());
            a.parts.push(Part { mesh: tm, mat: tmat, col: Some((b.0, b.1)) });
        }
        // (a tucked bed keeps what lies on it within its frame too)
        if geom.tuck {
            for &(part, ..) in &rests {
                let (l, h) = a.parts[part].mesh.bounds();
                let dy = (-geom.ymax - l[1]).max(0.0) + (geom.ymax - h[1]).min(0.0);
                let dx = (geom.xmax - h[0]).min(0.0);
                a.parts[part].mesh.transform(&Mat4::translation([dx, dy, 0.0]));
            }
        }
        if !sim && !geom.tuck {
            // unmade (mess pass): the sheet pulled back toward the foot and askew, the cover thrown crooked, both more rumpled; what lay on them
            // comes to rest on them again
            let (loft, margin) = (drape_loft(s.sheet_type), if s.sheet_type >= 2 { b.margin } else { 0.0 });
            let mat_mesh = mattress.clone();
            // (kept to the soft volume the bed was placed with: pulled through a wall, the whole bed was removed)
            let mut geom = geom.clone();
            let lift = loft + 0.012;
            let (ov_s, ov_f) = ((wm - 1.0) / 2.0 * ms.width, (0.15 + sm * ms.size - geom.x1).max(0.0));
            let ov = ov_s.hypot(ov_f);
            geom.xmax = geom.x1 + geom.reach(ov, geom.gap_foot, lift);
            geom.ymax = geom.yh + geom.reach(ov, geom.gap_side, lift);
            a.unmake = Some(std::sync::Arc::new(move |a: &mut Asset, lvl: f64, r: &mut Rng| {
                let tilt = |r: &mut Rng, y: f64, t: f64| [lvl * (r.f64() * 2.0 - 1.0) * y, lvl * (r.f64() * 2.0 - 1.0) * t.to_radians()];
                let (p0, s0) = (lvl * (0.1 + 0.35 * r.f64()), tilt(r, 0.08, 6.0));
                let (p1, s1) = (lvl * (r.f64() * 2.0 - 0.6) * 0.3, tilt(r, 0.15, 14.0));
                let k = 1.0 + 2.5 * lvl;
                let sh = drape(&pre, &geom, 0.0, loft, margin, &pleats, s0, p0, k);
                let cv = drape(&cover, &geom, loft + 0.008, 0.0, 0.0, &pleats, s1, p0.min(p1 + 0.1).max(p1), k);
                let col = {
                    let (mut pos, mut faces): (Vec<V3>, Vec<Vec<u32>>) = (vec![], vec![]);
                    for mm in [&sh, &mat_mesh, &cv] {
                        let o = pos.len() as u32;
                        pos.extend_from_slice(&mm.pos);
                        faces.extend(mm.faces.iter().map(|f| f.iter().map(|i| i + o).collect()));
                    }
                    Collider::new(&pos, &faces)
                };
                a.parts[sheet_part].mesh = sh;
                a.parts[cover_part].mesh = cv;
                for &(part, depth, skin, gap) in &rests { super::cloth::rest_and_wrap(&mut a.parts[part].mesh.pos, &col, depth, skin, gap); }
                for &i in [sheet_part, cover_part].iter().chain(rests.iter().map(|x| &x.0)) { let b = a.parts[i].mesh.bounds(); a.parts[i].col = Some(b); }
                true
            }));
        }
        a
    }
}

/// BedFactory.scatter: drop points (x, y) at height 1 onto the highest surface of `bases`, trimesh remove_close culling to `n`, random order
fn scatter(bases: &[&Mesh], xs: &[f64], ys: &[f64], n: usize, r: &mut Rng) -> Vec<V3> {
    let mut pts: Vec<V3> = vec![];
    for k in 0..xs.len() {
        let z = bases.iter().filter_map(|t| face_top_z(t, xs[k], ys[k])).fold(f64::NEG_INFINITY, f64::max);
        pts.push([xs[k], ys[k], if z.is_finite() { z } else { 0.0 }]);
    }
    decimate(&pts, n, r)
}

/// trimesh.points.remove_close: indices kept
fn remove_close(pts: &[V3], radius: f64) -> Vec<usize> {
    let n = pts.len();
    let mut pairs = vec![];
    for i in 0..n {
        for j in i + 1..n {
            let d: f64 = (0..3).map(|k| (pts[i][k] - pts[j][k]).powi(2)).sum();
            if d <= radius * radius {
                pairs.push((i, j));
            }
        }
    }
    let mut count = vec![0usize; n];
    for &(i, j) in &pairs {
        count[i] += 1;
        count[j] += 1;
    }
    let mut removed = vec![false; n];
    for &(i, j) in &pairs {
        removed[if count[j] > count[i] { j } else { i }] = true;
    }
    (0..n).filter(|&i| !removed[i]).collect()
}

/// decorate.decimate(points, n)
fn decimate(pts: &[V3], n: usize, r: &mut Rng) -> Vec<V3> {
    let (mut dist, ratio) = (0.1f64, 1.2f64);
    loop {
        let culled = remove_close(pts, dist);
        if culled.len() <= n || dist > 10.0 {
            dist /= ratio;
            break;
        }
        dist *= ratio;
    }
    let mut kept: Vec<V3> = remove_close(pts, dist).into_iter().map(|i| pts[i]).collect();
    r.shuffle(&mut kept);
    kept.truncate(n);
    kept
}

/// BlanketFactory.fold: rotate by theta, shift by -y_margin, bisect at y = 0, mirror the y > 0 half down (y -> -y, z -> 0.05 - z), undo
fn fold(m: &Mesh, theta: f64, ym: f64) -> Mesh {
    let mut o = m.clone();
    o.transform(&Mat4::rot_z(theta));
    o.transform(&Mat4::translation([0.0, -ym, 0.0]));
    let mut o = bisect_y(&o);
    for p in o.pos.iter_mut() {
        if p[1] > 0.0 {
            p[1] = -p[1];
            p[2] = 0.05 - p[2];
        }
    }
    o.transform(&Mat4::translation([0.0, ym, 0.0]));
    o.transform(&Mat4::rot_z(-theta));
    o
}

/// bpy.ops.mesh.bisect(plane_co=0, plane_no=(0,1,0)): faces crossing y = 0 are split along the cut (no fill)
fn bisect_y(m: &Mesh) -> Mesh {
    let th = 1e-4;
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    let side = |y: f64| if y > th { 1 } else if y < -th { -1 } else { 0 };
    let mut cut: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
    for f in &m.faces {
        let n = f.len();
        // ring with inserted cut vertices; side 0 on the plane for the cut points
        let mut ring: Vec<(u32, i32)> = vec![];
        for k in 0..n {
            let (a, b) = (f[k], f[(k + 1) % n]);
            let (sa, sb) = (side(m.pos[a as usize][1]), side(m.pos[b as usize][1]));
            ring.push((a, sa));
            if sa * sb < 0 {
                let key = (a.min(b), a.max(b));
                let id = *cut.entry(key).or_insert_with(|| {
                    let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
                    let t = pa[1] / (pa[1] - pb[1]);
                    o.pos.push([pa[0] + (pb[0] - pa[0]) * t, 0.0, pa[2] + (pb[2] - pa[2]) * t]);
                    (o.pos.len() - 1) as u32
                });
                ring.push((id, 0));
            }
        }
        let pos_n = ring.iter().filter(|r| r.1 > 0).count();
        let neg_n = ring.iter().filter(|r| r.1 < 0).count();
        let planar: Vec<usize> = (0..ring.len()).filter(|&i| ring[i].1 == 0).collect();
        if pos_n > 0 && neg_n > 0 && planar.len() >= 2 {
            // split between the first two on-plane ring entries that separate the sides
            let (i, j) = (planar[0], *planar.last().unwrap());
            let (i, j) = if planar.len() > 2 { find_split(&ring) .unwrap_or((i, j)) } else { (i, j) };
            let l = ring.len();
            let mut f1 = vec![];
            let mut k = i;
            loop {
                f1.push(ring[k].0);
                if k == j { break; }
                k = (k + 1) % l;
            }
            let mut f2 = vec![];
            let mut k = j;
            loop {
                f2.push(ring[k].0);
                if k == i { break; }
                k = (k + 1) % l;
            }
            o.faces.push(f1);
            o.faces.push(f2);
        } else {
            o.faces.push(ring.iter().map(|r| r.0).collect());
        }
    }
    o
}

fn find_split(ring: &[(u32, i32)]) -> Option<(usize, usize)> {
    let l = ring.len();
    let pl: Vec<usize> = (0..l).filter(|&i| ring[i].1 == 0).collect();
    for a in 0..pl.len() {
        for b in a + 1..pl.len() {
            let (i, j) = (pl[a], pl[b]);
            let between: Vec<i32> = (i + 1..j).map(|k| ring[k].1).filter(|&s| s != 0).collect();
            let outside: Vec<i32> = (0..l).filter(|&k| k < i || k > j).map(|k| ring[k].1).filter(|&s| s != 0).collect();
            if !between.is_empty() && !outside.is_empty() && between.iter().all(|&s| s == between[0]) && outside.iter().all(|&s| s == -between[0]) {
                return Some((i, j));
            }
        }
    }
    None
}

/// the stream of a bed's mattress and bedding decisions (apart from the frame's)
fn bedding_rng(fseed: u64, inst: u64) -> Rng {
    inst_rng(fseed ^ 0x6265_6464, inst)
}

/// the mattress of a bed: type and size over the frame (MattressFactory's own ctor for the rest)
fn bed_mattress(c: &ChairSpec, r: &mut Rng) -> MattressSpec {
    let mut ms = MattressSpec::new(c.fseed);
    ms.coiled = r.choice_p(&[0.25, 0.75]) == 0;
    ms.width = c.width * r.uniform(0.88, 0.96);
    ms.size = c.size * r.uniform(0.88, 0.96);
    ms
}

/// the sheet's size over the mattress's (width, length)
fn sheet_scale(sheet_type: usize, r: &mut Rng) -> (f64, f64) {
    if sheet_type == 0 { (r.uniform(1.4, 1.6), r.uniform(0.9, 1.1)) } else { (r.uniform(1.4, 1.8), r.uniform(0.9, 1.2)) }
}

/// what the bedding hangs over, in the bed's frame: the mattress's top rectangle (z = its top's edge, the dome over it), the frame's
/// rail below it (how far it stands out past the mattress at the side and the foot) and the floor
#[derive(Clone)]
struct DrapeGeom {
    tuck: bool,
    /// how far the cloth may reach: a tucked-in one the frame's outline, an unmade one the soft volume
    xmax: f64,
    ymax: f64,
    x0: f64,
    x1: f64,
    yh: f64,
    top: f64,
    rail: f64,
    gap_side: f64,
    gap_foot: f64,
    floor: f64,
    dome_h: f64,
    size_mid: f64,
    ms_w: f64,
    ms_s: f64,
}

/// the edge's roll radius
const DRAPE_R: f64 = 0.035;
/// the pleats' largest depth, and their extra at a corner, where the cloth bunches
const PLEAT: f64 = 0.03;
const PLEAT_CORNER: f64 = 0.03;
/// the skirt's flare toward its hem, and the rumples' largest height
const FLARE: f64 = 0.05;
const RUMPLE: f64 = 0.016;
/// how far cloth lying on the floor can run out
const DRAPE_POOL: f64 = 0.10;

impl DrapeGeom {
    fn of(c: &ChairSpec, ms: &MattressSpec) -> DrapeGeom {
        let (x0, x1) = ((c.size - ms.size) / 2.0, (c.size + ms.size) / 2.0);
        DrapeGeom { tuck: crate::api::tuck_hint() > 0, xmax: if crate::api::tuck_hint() > 0 { c.size + c.leg_thickness * 0.5 } else { f64::INFINITY }, ymax: if crate::api::tuck_hint() > 0 { c.width / 2.0 + 0.018 } else { f64::INFINITY }, x0, x1, yh: ms.width / 2.0, top: ms.thickness, rail: 0.0, gap_side: ((c.width - ms.width) / 2.0 + 0.012 - DRAPE_R).max(0.0), gap_foot: (c.size + c.leg_thickness * 0.5 - x1).max(0.0) + 0.015, floor: -c.leg_height, dome_h: ms.puff().1, size_mid: c.size / 2.0, ms_w: ms.width, ms_s: ms.size }
    }

    /// the mattress's top under (x, y)
    fn top_at(&self, x: f64, y: f64) -> f64 {
        let (u, v) = ((2.0 * y / self.ms_w).clamp(-1.0, 1.0), (2.0 * (x - self.size_mid) / self.ms_s).clamp(-1.0, 1.0));
        self.top + self.dome_h * (1.0 - u.powi(4)) * (1.0 - v.powi(4))
    }

    /// the coordinate round the mattress's outline, continuous from one side round the foot to the other (a jump tore the folds at the corners);
    /// (qx, qy) the point on the outline, (dx, dy) out from it, `ang` that direction's angle
    fn perimeter(&self, qx: f64, qy: f64, dx: f64, dy: f64, ang: f64) -> f64 {
        use std::f64::consts::{FRAC_PI_2, PI};
        const R: f64 = 0.2;
        let (x1, yh) = (self.x1, self.yh);
        if dx > 0.0 && dy > 0.0 { x1 + R * (FRAC_PI_2 - ang) }
        else if dx > 0.0 && dy == 0.0 { x1 + R * FRAC_PI_2 + yh - qy }
        else if dx > 0.0 { x1 + R * FRAC_PI_2 + 2.0 * yh - R * ang }
        else if dy > 0.0 { qx + if dx < 0.0 { R * (FRAC_PI_2 - ang) } else { 0.0 } }
        else if dy < 0.0 { x1 + R * PI + 2.0 * yh + x1 - qx + if dx < 0.0 { R * (ang + FRAC_PI_2) } else { 0.0 } }
        else { self.x0 + qy }
    }

    /// `s` of cloth past the mattress's edge, `gap` to the rail: out by h at height z, and the surface's normal (nh, nz) there -- over the
    /// edge's roll, down and out to the rail, straight down, then running out on the floor
    fn profile(&self, s: f64, gap: f64, lift: f64) -> (f64, f64, f64, f64) {
        let r = DRAPE_R;
        let top = self.top + lift;
        let arc = std::f64::consts::FRAC_PI_2 * r;
        if s < arc { let a = s / r; return (r * a.sin(), top - r * (1.0 - a.cos()), a.sin(), a.cos()) }
        let drop = top - r - self.rail;
        let l = gap.hypot(drop);
        if s < arc + l { let t = (s - arc) / l; return (r + gap * t, top - r - drop * t, drop / l, gap / l) }
        // (tucked in: what is left goes under the mattress at the rail)
        if self.tuck { return (r + gap, self.rail, 1.0, 0.0) }
        let v = self.rail - self.floor - 0.004;
        if s < arc + l + v { return (r + gap, self.rail - (s - arc - l), 1.0, 0.0) }
        let rem = s - arc - l - v;
        (r + gap + (rem * 0.45).min(DRAPE_POOL), self.floor + 0.004 + (rem * 0.05).min(0.02), 0.0, 1.0)
    }

    /// how far past the mattress's edge a cloth with `over` of overhang reaches, its layers and pleats included
    fn reach(&self, over: f64, gap: f64, lift: f64) -> f64 {
        self.profile(over * 1.06, gap, lift).0.max(DRAPE_R + gap.min(over)) + lift + PLEAT + PLEAT_CORNER + FLARE + RUMPLE + 0.008
    }
}

/// the hanging folds and the rumples (shared by the layers, so they nest)
#[derive(Clone)]
struct Pleats {
    wave: [(f64, f64, f64); 5],
    lump: [(f64, f64, f64); 4],
    crease: [(f64, f64, f64); 3],
}

impl Pleats {
    fn new(r: &mut Rng) -> Pleats {
        let tau = std::f64::consts::TAU;
        let mut dir = |l: f64| (l * r.uniform(0.75, 1.25), r.uniform(0.0, tau), r.uniform(0.0, tau));
        Pleats { wave: [0.09, 0.14, 0.23, 0.37, 0.61].map(&mut dir), lump: [0.34, 0.52, 0.81, 1.27].map(&mut dir), crease: [0.55, 0.8, 1.1].map(&mut dir) }
    }
    /// -1..1 along the perimeter coordinate u
    fn at(&self, u: f64) -> f64 {
        let tau = std::f64::consts::TAU;
        let (mut v, mut n) = (0.0, 0.0);
        for (l, ph, w) in self.wave { let w = 0.5 + 0.5 * w.sin().abs(); v += w * (tau * u / l + ph).sin(); n += w; }
        v / n
    }
    /// 0..1: soft lumps, a few long creases running through them (ridged sines, bent by the lumps)
    fn rumple(&self, x: f64, y: f64) -> f64 {
        let tau = std::f64::consts::TAU;
        let mut lumps = 0.0;
        for (l, a, ph) in self.lump { lumps += (tau * (x * a.cos() + y * a.sin()) / l + ph).sin(); }
        lumps /= 4.0;
        let mut crease = 0.0f64;
        for (l, a, ph) in self.crease {
            let t = tau * (x * a.cos() + y * a.sin()) / l + ph + 2.2 * lumps;
            // (on one side of each lump only: a crease that fades out along its length)
            crease = crease.max((1.0 - t.sin().abs()).powi(8) * (0.5 + 0.5 * (t * 0.37 + ph).sin()));
        }
        (0.87 * (lumps + 1.0) / 2.0 + 0.13 * (1.0 - crease)).clamp(0.0, 1.0)
    }
}

/// how a cloth lies on the bed, off true: [sideways slide, turn]; 0 the sheet, 1 the cover
fn drape_slide(fseed: u64, inst: u64, which: u64) -> [f64; 2] {
    let mut r = inst_rng(fseed ^ (0x736c_6964 + which), inst);
    [r.uniform(-0.05, 0.05), r.uniform(-0.03, 0.03)]
}

/// a comforter's fill (a sheet's none)
fn drape_loft(sheet_type: usize) -> f64 {
    if sheet_type == 0 { 0.0 } else { 0.07 }
}

/// a flat cloth (lying at its lowest z, its layers and any fold above that) laid over the bed without simulating it. It is subdivided first,
/// each point keeping which layer it is on; then a comforter's top layer stands `loft` over its bottom, easing to nothing at its edges and,
/// a box comforter (`margin`, its seams' spacing), domed over each cell; then each point is laid: over the mattress it follows the top; past
/// its edge it keeps its distance along the surface (DrapeGeom::profile), radially at the corners, where the extra cloth bunches into a pleat,
/// its skirt flaring and its hem waving. `lift`: what it lies on; `slide`: off true on the bed; `pull`: moved toward the foot; `rumple`: the
/// rumples' scale (an unmade bed's are larger)
fn drape(pre: &Mesh, g: &DrapeGeom, lift: f64, loft: f64, margin: f64, pl: &Pleats, slide: [f64; 2], pull: f64, rumple: f64) -> Mesh {
    let (lo, hi) = pre.bounds();
    let zmin = lo[2];
    let two_layers = pre.pos.iter().any(|p| { let d = p[2] - zmin; d > 0.002 && d < 0.02 });
    // (z = which layer: 0 bottom, 1 top, + 2 a fold, lying 5 cm over the layer it came from, flipped: +0.06 and +0.07 over the two, +0.05 over the one)
    let mut m = pre.clone();
    for p in m.pos.iter_mut() {
        let d = p[2] - zmin;
        let folded = d >= 0.03;
        p[2] = if two_layers { ((if folded { d - 0.06 } else { d }) / 0.01).clamp(0.0, 1.0) } else { 0.0 } + if folded { 2.0 } else { 0.0 };
    }
    let mut m = subsurf(&m, 1, false);
    let c0 = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    // the seams run from the cloth's middle (Blanket::box_pinch)
    let dome = |d: f64| { let t = (d / margin).rem_euclid(1.0); let t = if t > 0.5 { 1.0 - t } else { t } * 2.0; (1.0 - (1.0 - t) * (1.0 - t)).sqrt() };
    for p in m.pos.iter_mut() {
        let (layer, stack) = if p[2] <= 1.0 { (p[2].max(0.0), 0.0) } else { ((p[2] - 2.0).clamp(0.0, 1.0), (p[2] - 1.0).min(1.0)) };
        let edge = (p[0] - lo[0]).min(hi[0] - p[0]).min(p[1] - lo[1]).min(hi[1] - p[1]).max(0.0);
        let mut fill = (edge / 0.09).min(1.0).sqrt();
        if margin > 0.0 { fill *= 0.15 + 0.85 * dome(p[0] - c0[0]) * dome(p[1] - c0[1]); }
        // (a throw's edge comes down onto what it lies on: floating, it showed a dark line)
        let settle = if lift < 0.02 { 0.3 + 0.7 * (edge / 0.06).min(1.0) } else { 1.0 };
        p[2] = lift * settle + 0.003 + loft * layer * fill + stack * (loft + 0.006);
    }
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (sy, cy) = slide[1].sin_cos();
    for p in m.pos.iter_mut() {
        let off = p[2];
        let (dx0, dy0) = (p[0] - c[0], p[1] - c[1]);
        let (x, y) = (c[0] + dx0 * cy - dy0 * sy + pull, c[1] + dx0 * sy + dy0 * cy + slide[0]);
        // (a comforter's fill takes up most of the rumples)
        let rum = rumple * RUMPLE * pl.rumple(x, y) * if loft > 0.0 { 0.5 } else { 1.0 };
        let (qx, qy) = (x.clamp(g.x0, g.x1), y.clamp(-g.yh, g.yh));
        let (dx, dy) = (x - qx, y - qy);
        let s0 = dx.hypot(dy);
        if s0 < 1e-9 {
            *p = [x, y, g.top_at(x, y) + off + rum];
            continue;
        }
        let (nx, ny) = (dx / s0, dy / s0);
        let corner = dx != 0.0 && dy != 0.0;
        let ang = dy.atan2(dx);
        let u = g.perimeter(qx, qy, dx, dy, ang);
        let wave = pl.at(u);
        // the hem's length waves with the folds
        let s = s0 * (1.0 + 0.06 * pl.at(u * 0.37 + 11.0));
        let gap = nx.max(0.0) * g.gap_foot + ny.abs() * g.gap_side;
        let (h, z, nh, nz) = g.profile(s, gap, 0.0);
        let ramp = (s / 0.2).min(1.0);
        // toward the head (where side tables stand against the frame) it hangs close: no flare, folds or fill standing out
        let free = if g.tuck { 0.0 } else if ny == 0.0 { 1.0 } else { (((x - g.x0) - 0.55) / 0.35).clamp(0.0, 1.0) };
        // the folds deepen down the skirt, which flares toward its hem where a fold opens out
        let drop = ((g.top - z) / 0.5).clamp(0.0, 1.0);
        let mut fold = PLEAT * ramp * (0.4 + 0.6 * drop) * wave * nh;
        fold += FLARE * drop * drop * (0.5 + 0.5 * pl.at(u * 0.53 + 5.0)) * nh;
        if corner { fold += PLEAT_CORNER * ramp * (4.0 * ang).sin() * nh; }
        let h = (h - (1.0 - free) * (h - DRAPE_R - gap).max(0.0)) + (0.15 + 0.85 * free) * off * nh + free * (fold + rum * nh * 0.5);
        // (at the head end it stays clear of the headboard)
        let px = (qx + nx * h).max(if nx < 0.0 { 0.005 } else { f64::NEG_INFINITY });
        let py = qy + ny * h;
        let (px, py) = (px.min(g.xmax), py.clamp(-g.ymax, g.ymax));
        *p = [px, py, z + (off + rum) * nz];
    }
    m
}
