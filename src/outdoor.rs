//! Outdoor terrain around the house (program.outdoor): what the windows look out on. Infinigen builds coarse SDF terrain from its
//! elements (terrain/source/common/elements) and drops the house on it; for window views the 2.5D part is enough, so the `ground` and
//! `mountains` elements are ported as heightfields with their noise (FastNoiseLite Perlin fractal + Blender's Jenkins hash, as in
//! terrain/source/common/utils and infinigen_gpl/bnodes/utils/blender_noise.h):
//!   ground(x, y)    = Perlin(seed_g, octaves 9, freq 0.01) * scale (rolling ground, a few metres)
//!   mountains(x, y) = max over 3 groups of Perlin(log_uniform freq) * height * ramp(mask Perlin) + a slope Perlin (distant ridges)
//! The mountains fade in with the distance from the house (ridges on the horizon, not in the garden), and the ground eases into a
//! level pad `pad_depth` below the floor around the house (the house sits on a plinth, the windows look out level).
//! Geometry: a regular grid out to `near_extent` around the house (quads under the house dropped) and a radial ring beyond it to
//! `far_radius`; materials are Infinigen's terrain shaders (terrain.Mud / Dirt / Soil / Sand / Stone / CrackedGround / CobbleStone
//! by its ground weights; terrain.Mountain / Sandstone on the ridges), resolved by the renderer like every material spec.
//! Vegetation: a plant-community simulation places trees (species by site, sizes by competition) and understory shrubs, with a
//! canopy shell for the distant forest; Rust trees go in as geometry, or (trees = "infinigen") the spots go to the sidecar
//! `outdoor.props` and the renderer instances Infinigen's TreeFactory / BushFactory variants (tools/render/tree_cache.py).
//! Stream: int_hash(seed, "outdoor").
use crate::mesh::{Mat4, Material, Mesh, Scene};
use crate::rng::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct OutdoorCfg {
    /// Terrain and vegetation around the house.
    pub on: bool,
    /// Half size of the fine terrain grid around the house (m).
    #[schemars(range(min = 10.0, max = 300.0))]
    pub near_extent: f64,
    /// Fine grid spacing (m).
    #[schemars(range(min = 0.25, max = 5.0))]
    pub near_spacing: f64,
    /// Outer radius of the terrain (m).
    #[schemars(range(min = 100.0, max = 10000.0))]
    pub far_radius: f64,
    /// Ground relief amplitude (m; Infinigen's ground scale).
    #[schemars(range(min = 0.0, max = 30.0))]
    pub ground_scale: f64,
    /// Peak mountain height on the horizon (m; null: drawn 40-150 m).
    pub mountain_height: Option<f64>,
    /// How much of the horizon carries mountains (0-1; null: drawn).
    pub mountain_coverage: Option<f64>,
    /// Ground below the floor at the walls (m): the house's plinth.
    #[schemars(range(min = 0.0, max = 3.0))]
    pub pad_depth: f64,
    /// Distance over which the ground eases from the pad into the terrain (m).
    #[schemars(range(min = 2.0, max = 100.0))]
    pub pad_blend: f64,
    /// Tree density inside groves (trees per 1000 m2 of grove; groves cover part of the land, see the grove mask).
    #[schemars(range(min = 0.0, max = 20.0))]
    pub tree_density: f64,
    /// Bushes per 100 m2 of the 1.5-12 m band around the house.
    #[schemars(range(min = 0.0, max = 20.0))]
    pub bush_density: f64,
    /// Woodland extends this far from the house (m; trees past the fine grid are cheap instances).
    #[schemars(range(min = 50.0, max = 2000.0))]
    pub tree_far: f64,
    /// Closest a tree trunk stands to a wall (m).
    #[schemars(range(min = 1.0, max = 30.0))]
    pub tree_wall_clearance: f64,
    /// Tree source: "rust" (crates/ig_plants/src/rtree.rs, geometry in the scene) or "infinigen" (the renderer instances Infinigen's
    /// TreeFactory variants from its cache; Blender renders only).
    pub trees: String,
    /// Tree / bush variants the renderer builds and reuses (a cache shared by all houses).
    #[schemars(range(min = 1, max = 32))]
    pub tree_variants: u32,
    #[schemars(range(min = 1, max = 32))]
    pub bush_variants: u32,
}

impl Default for OutdoorCfg {
    fn default() -> Self {
        OutdoorCfg { on: true, near_extent: 70.0, near_spacing: 1.0, far_radius: 2500.0, ground_scale: 4.0, mountain_height: None, mountain_coverage: None, pad_depth: 0.45, pad_blend: 18.0,
            tree_density: 4.0, bush_density: 3.0, tree_far: 400.0, tree_wall_clearance: 5.0, tree_variants: 5, bush_variants: 4, trees: "rust".into() }
    }
}

// ---------------------------------------------------------------- noise (FastNoiseLite Perlin, Blender Jenkins hash)

fn rot(x: u32, k: u32) -> u32 { (x << k) | (x >> (32 - k)) }
fn hash_final(a: &mut u32, b: &mut u32, c: &mut u32) {
    *c ^= *b; *c = c.wrapping_sub(rot(*b, 14));
    *a ^= *c; *a = a.wrapping_sub(rot(*c, 11));
    *b ^= *a; *b = b.wrapping_sub(rot(*a, 25));
    *c ^= *b; *c = c.wrapping_sub(rot(*b, 16));
    *a ^= *c; *a = a.wrapping_sub(rot(*c, 4));
    *b ^= *a; *b = b.wrapping_sub(rot(*a, 14));
    *c ^= *b; *c = c.wrapping_sub(rot(*b, 24));
}
/// Blender's hash of 2 / 3 uints (myhash)
fn myhash2(kx: u32, ky: u32) -> u32 {
    let mut a = 0xdeadbeefu32.wrapping_add((2 << 2) + 13);
    let (mut b, mut c) = (a, a);
    b = b.wrapping_add(ky);
    a = a.wrapping_add(kx);
    hash_final(&mut a, &mut b, &mut c);
    c
}
fn myhash3(kx: u32, ky: u32, kz: u32) -> u32 {
    let mut a = 0xdeadbeefu32.wrapping_add((3 << 2) + 13);
    let (mut b, mut c) = (a, a);
    c = c.wrapping_add(kz);
    b = b.wrapping_add(ky);
    a = a.wrapping_add(kx);
    hash_final(&mut a, &mut b, &mut c);
    c
}
fn myhash1(kx: u32) -> u32 {
    let mut a = 0xdeadbeefu32.wrapping_add((1 << 2) + 13);
    let (mut b, mut c) = (a, a);
    a = a.wrapping_add(kx);
    hash_final(&mut a, &mut b, &mut c);
    c
}
fn hash_to_float(k: u32) -> f64 { myhash1(k) as f64 / 4294967295.0 }
fn log_uniform(lo: f64, hi: f64, seed: u32) -> f64 { (lo.ln() + (hi.ln() - lo.ln()) * hash_to_float(seed)).exp() }
fn ramp(x: f64, lo: f64, hi: f64) -> f64 { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) }

/// FastNoiseLite's Gradients3D (64 vectors, padded to 4)
fn grad(i: usize) -> [f32; 3] {
    const A: [[f32; 3]; 4] = [[0., 1., 1.], [0., -1., 1.], [0., 1., -1.], [0., -1., -1.]];
    const B: [[f32; 3]; 4] = [[1., 0., 1.], [-1., 0., 1.], [1., 0., -1.], [-1., 0., -1.]];
    const C: [[f32; 3]; 4] = [[1., 1., 0.], [-1., 1., 0.], [1., -1., 0.], [-1., -1., 0.]];
    const LAST: [[f32; 3]; 4] = [[1., 1., 0.], [0., -1., 1.], [-1., 1., 0.], [0., -1., -1.]];
    let (row, col) = (i / 4, i % 4);
    if row == 15 { LAST[col] } else { [A, B, C][row % 3][col] }
}
const PX: i32 = 501125321;
const PY: i32 = 1136930381;
const PZ: i32 = 1720413743;
fn grad_coord(seed: i32, xp: i32, yp: i32, zp: i32, xd: f32, yd: f32, zd: f32) -> f32 {
    let mut h = (seed ^ xp ^ yp ^ zp).wrapping_mul(0x27d4eb2d);
    h ^= h >> 15;
    h &= 63 << 2;
    let g = grad((h >> 2) as usize);
    xd * g[0] + yd * g[1] + zd * g[2]
}
fn floor_i(f: f32) -> i32 { if f >= 0.0 { f as i32 } else { f as i32 - 1 } }
fn quintic(t: f32) -> f32 { t * t * t * (t * (t * 6.0 - 15.0) + 10.0) }
fn lerp(a: f32, b: f32, t: f32) -> f32 { a + t * (b - a) }
fn single_perlin(seed: i32, x: f32, y: f32, z: f32) -> f32 {
    let (x0, y0, z0) = (floor_i(x), floor_i(y), floor_i(z));
    let (xd0, yd0, zd0) = (x - x0 as f32, y - y0 as f32, z - z0 as f32);
    let (xd1, yd1, zd1) = (xd0 - 1.0, yd0 - 1.0, zd0 - 1.0);
    let (xs, ys, zs) = (quintic(xd0), quintic(yd0), quintic(zd0));
    let (x0, y0, z0) = (x0.wrapping_mul(PX), y0.wrapping_mul(PY), z0.wrapping_mul(PZ));
    let (x1, y1, z1) = (x0.wrapping_add(PX), y0.wrapping_add(PY), z0.wrapping_add(PZ));
    let xf00 = lerp(grad_coord(seed, x0, y0, z0, xd0, yd0, zd0), grad_coord(seed, x1, y0, z0, xd1, yd0, zd0), xs);
    let xf10 = lerp(grad_coord(seed, x0, y1, z0, xd0, yd1, zd0), grad_coord(seed, x1, y1, z0, xd1, yd1, zd0), xs);
    let xf01 = lerp(grad_coord(seed, x0, y0, z1, xd0, yd0, zd1), grad_coord(seed, x1, y0, z1, xd1, yd0, zd1), xs);
    let xf11 = lerp(grad_coord(seed, x0, y1, z1, xd0, yd1, zd1), grad_coord(seed, x1, y1, z1, xd1, yd1, zd1), xs);
    lerp(lerp(xf00, xf10, ys), lerp(xf01, xf11, ys), zs) * 0.964_921_4
}
/// FastNoiseLite fractal Perlin as Infinigen calls it (bounding 1/1.75, lacunarity 2, gain 0.5)
fn perlin(x: f64, y: f64, seed: u32, octaves: u32, freq: f64) -> f64 {
    let (mut x, mut y, mut z) = ((x * freq) as f32, (y * freq) as f32, 0.0f32);
    let mut seed = seed as i32;
    let (mut sum, mut amp) = (0.0f32, 1.0f32 / 1.75);
    for _ in 0..octaves {
        sum += single_perlin(seed, x, y, z) * amp;
        seed = seed.wrapping_add(1);
        x *= 2.0; y *= 2.0; z *= 2.0;
        amp *= 0.5;
    }
    sum as f64
}

// ---------------------------------------------------------------- terrain

struct Terrain {
    seed_g: u32,
    ground_scale: f64,
    m_seed: u32,
    m_min_freq: f64,
    m_max_freq: f64,
    m_scale: f64,
    m_mask_freq: f64,
    m_ramp: (f64, f64),
    m_slope_scale: f64,
    centre: [f64; 2],
    house: crate::shp::G,
    pad_z: f64,
    pad_blend: f64,
    level: f64,
}

impl Terrain {
    fn ground(&self, x: f64, y: f64) -> f64 { perlin(x, y, myhash2(self.seed_g, 0), 9, 0.01) * self.ground_scale }
    fn mountains(&self, x: f64, y: f64) -> f64 {
        let mut h = 0.0f64;
        for i in 0..3u32 {
            let f = log_uniform(self.m_min_freq, self.m_max_freq, myhash3(self.m_seed, i, 0));
            let t = perlin(x, y, myhash3(self.m_seed, i, 1), 6, f) * self.m_scale;
            let m = perlin(x, y, myhash3(self.m_seed, i, 2), 2, self.m_mask_freq);
            h = h.max(t * ramp(m, self.m_ramp.0, self.m_ramp.1));
        }
        h + perlin(x, y, myhash2(self.m_seed, 4), 6, 0.0015) * self.m_slope_scale
    }
    /// terrain height (world z) at (x, y); `d` = distance to the house footprint
    fn height(&self, x: f64, y: f64, d: f64) -> f64 {
        let r = (x - self.centre[0]).hypot(y - self.centre[1]);
        let far = smooth(300.0, 1200.0, r);
        let free = self.ground(x, y) - self.level + self.mountains(x, y) * far;
        let b = smooth(1.5, 1.5 + self.pad_blend, d);
        self.pad_z * (1.0 - b) + (self.pad_z + free) * b
    }
    fn dist(&self, x: f64, y: f64) -> f64 {
        if crate::shp::contains_pt(&self.house, [x, y]) { 0.0 } else { crate::shp::distance(&self.house, &crate::shp::point(x, y)) }
    }
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, rgb, 0.9);
    m.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}));
    m
}

/// Adds the terrain to `scene`; returns the sidecar `outdoor` record (terrain settings + vegetation spots) and a log.
pub fn add(scene: &mut Scene, room_polys: &[(String, crate::shp::G)], seed: u64, floor_z: f64, wall_t: f64, cfg: &OutdoorCfg) -> (Value, Vec<String>) {
    let mut log = vec![];
    let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "outdoor"]));
    let polys: Vec<crate::shp::G> = room_polys.iter().map(|(_, g)| g.clone()).collect();
    let house = crate::shp::buffer_mitre(&crate::shp::union_all(&polys), wall_t / 2.0);
    let (x0, y0, x1, y1) = crate::shp::bounds(&house);
    let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
    // Infinigen's draws, in its order: ground seed, then the mountains' seed and parameters
    let seed_g = r.next_u64() as u32;
    let m_seed = r.next_u64() as u32;
    let m_height = cfg.mountain_height.unwrap_or(r.uniform(40.0, 150.0));
    let coverage = cfg.mountain_coverage.unwrap_or(r.uniform(0.2, 0.7));
    let mut t = Terrain {
        seed_g, ground_scale: cfg.ground_scale, m_seed, m_min_freq: 0.0012, m_max_freq: 0.004, m_scale: m_height * 2.0, m_mask_freq: 0.0012,
        m_ramp: (-1.1 - coverage * 2.0 + 1.6, -0.9 - coverage * 2.0 + 1.6), m_slope_scale: m_height * 0.15,
        centre, house: house.clone(), pad_z: floor_z - cfg.pad_depth, pad_blend: cfg.pad_blend, level: 0.0,
    };
    // the free terrain's level at the house: the pad joins it without a step
    t.level = t.ground(centre[0], centre[1]);
    // materials (Infinigen's ground weights; ridges in mountain stone)
    // (Infinigen's ground weights without mud and sand: mud's puddles mirrored the sky -- a flooded field -- without grass on it)
    let ground_cls = ["terrain.Dirt", "terrain.Dirt", "terrain.Soil", "terrain.Soil", "terrain.CobbleStone", "terrain.CrackedGround", "terrain.Stone"];
    let gcls = ground_cls[r.randint(0, ground_cls.len() as i64) as usize];
    // (Stone mostly: the Mountain shader samples each of its 16 strata colours independently -- teal and pink bands at a distance)
    let mcls = if r.f64() < 0.7 { "terrain.Stone" } else { "terrain.Mountain" };
    // (fallback colour = lawn green: what plain renderers -- the previewer, the browser -- show; Cycles uses the shader + lawn tint)
    let gm = scene.material(spec(&format!("outdoor_ground:{gcls}"), gcls, r.next_u64(), [0.07, 0.13, 0.035]));
    let mm = scene.material(spec(&format!("outdoor_mountain:{mcls}"), mcls, r.next_u64(), [0.22, 0.24, 0.17]));
    // ---- fine grid (quads entirely under the house dropped)
    let (e, s) = (cfg.near_extent, cfg.near_spacing);
    let n = (2.0 * e / s).round() as usize;
    let gx = |i: usize| centre[0] - e + i as f64 * s;
    let gy = |j: usize| centre[1] - e + j as f64 * s;
    let mut hz = vec![0.0; (n + 1) * (n + 1)];
    let mut inside = vec![false; (n + 1) * (n + 1)];
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (gx(i), gy(j));
            let d = t.dist(x, y);
            inside[j * (n + 1) + i] = d <= 0.0;
            hz[j * (n + 1) + i] = t.height(x, y, d);
        }
    }
    let mut near = Mesh::new();
    for j in 0..n {
        for i in 0..n {
            let id = |a: usize, b: usize| b * (n + 1) + a;
            if inside[id(i, j)] && inside[id(i + 1, j)] && inside[id(i, j + 1)] && inside[id(i + 1, j + 1)] { continue }
            let p = |a: usize, b: usize| [gx(a), gy(b), hz[id(a, b)]];
            near.add_quad(p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1));
        }
    }
    // ---- far ring: from the grid's square edge out to far_radius (log-spaced rings)
    let (na, nr) = (720usize, 160usize);
    let mut ring: Vec<Vec<[f64; 3]>> = vec![];
    for k in 0..=nr {
        let mut row = vec![];
        for a in 0..na {
            let ang = std::f64::consts::TAU * a as f64 / na as f64;
            let (c, sn) = (ang.cos(), ang.sin());
            let r_in = e / c.abs().max(sn.abs());
            let rr = r_in * (cfg.far_radius / r_in).powf(k as f64 / nr as f64);
            let (x, y) = (centre[0] + rr * c, centre[1] + rr * sn);
            let d = rr - e.min(rr); // far from the house: the pad has long blended out
            row.push([x, y, t.height(x, y, d + e * 0.5)]);
        }
        ring.push(row);
    }
    let mut far_g = Mesh::new();
    let mut far_m = Mesh::new();
    for k in 0..nr {
        for a in 0..na {
            let b = (a + 1) % na;
            let (p00, p10, p11, p01) = (ring[k][a], ring[k][b], ring[k + 1][b], ring[k + 1][a]);
            let zmax = p00[2].max(p10[2]).max(p11[2]).max(p01[2]);
            let target = if zmax - t.pad_z > 25.0 { &mut far_m } else { &mut far_g };
            target.add_quad(p00, p10, p11, p01);
        }
    }
    // (shared vertices: smooth shading; per-quad vertices shaded the ridges as facets)
    for m in [&mut near, &mut far_g, &mut far_m] { m.weld(1e-6); }
    scene.add("outdoor_terrain_near#0", "OutdoorTerrain", near, gm);
    scene.add("outdoor_terrain_far#0", "OutdoorTerrain", far_g, gm);
    if !far_m.is_empty() { scene.add("outdoor_terrain_far#1", "OutdoorTerrain", far_m, mm); }
    // ---- vegetation: a plant-community simulation (Deussen et al., "Realistic modeling and rendering of plant ecosystems", 1998;
    // Lane & Prusinkiewicz, "Generating spatial distributions for multilevel models of plant communities", 2002). Seeds fall where
    // Infinigen's low-frequency grove mask (generate_nature.py) is high, plus a few in the open; each plant is a crown circle that grows
    // toward its species' size, and where two crowns overlap the smaller one is shaded: it grows slower and dies with a chance set by
    // its shade tolerance (self-thinning). Old trees die (gaps), mature trees drop seeds of their own species nearby (stands, not a
    // catalogue), so the survivors have natural spacing and a mix of ages and sizes. Species follow the site (a light version of
    // Pałubicki et al., "Ecoclimates", 2022): willows in wet hollows, pines on slopes and high ground, oak and maple on the mesic
    // ground, birch as the fast short-lived pioneer of gaps and edges, all weighted by a per-house dominance draw. Understory shrubs
    // survive at edges and in gaps, not deep under the canopy. Woodland continues past the fine grid to `tree_far` m.
    let mut props: Vec<Value> = vec![];
    let grove_seed = r.next_u64() as u32;
    let mask = |x: f64, y: f64| perlin(x, y, grove_seed, 3, 0.012);
    let thresh = 0.12 - 0.25 * (cfg.tree_density / 10.0).min(1.0);
    let far = cfg.tree_far.max(e);
    // species (rtree order Oak, Maple, Birch, Pine, Willow): max crown radius (m), growth (m per step), shade tolerance, life (steps),
    // seed dispersal (m), seed rain (chance per mature tree per recruitment)
    struct Sp { rmax: f64, grow: f64, tol: f64, life: f64, disperse: f64, fec: f64 }
    let sps = [
        Sp { rmax: 7.0, grow: 0.30, tol: 0.5, life: 60.0, disperse: 12.0, fec: 0.3 },
        Sp { rmax: 5.5, grow: 0.35, tol: 0.8, life: 45.0, disperse: 18.0, fec: 0.35 },
        Sp { rmax: 3.4, grow: 0.6, tol: 0.15, life: 22.0, disperse: 30.0, fec: 0.4 },
        Sp { rmax: 3.8, grow: 0.35, tol: 0.25, life: 50.0, disperse: 25.0, fec: 0.4 },
        Sp { rmax: 5.5, grow: 0.5, tol: 0.2, life: 30.0, disperse: 15.0, fec: 0.5 },
    ];
    // per-house dominance: one or two species lead
    let mut dom = [0.0f64; 5];
    for w in dom.iter_mut() { *w = r.uniform(0.05, 0.35); }
    let lead = r.randint(0, 5) as usize; dom[lead] = 1.0;
    if r.f64() < 0.6 { dom[r.randint(0, 5) as usize] = 0.8; }
    let nat = |x: f64, y: f64| t.height(x, y, 1e4);
    let site = |x: f64, y: f64| -> [f64; 5] {
        let h = nat(x, y);
        let (dx, dy) = ((nat(x + 3.0, y) - nat(x - 3.0, y)) / 6.0, (nat(x, y + 3.0) - nat(x, y - 3.0)) / 6.0);
        let slope = dx.hypot(dy);
        let wet = (nat(x + 30.0, y) + nat(x - 30.0, y) + nat(x, y + 30.0) + nat(x, y - 30.0)) / 4.0 - h;
        let elev = h - (t.pad_z + cfg.pad_depth);
        let (w, s, hi) = (ramp(wet, 0.12, 0.7), ramp(slope, 0.12, 0.45), ramp(elev, 8.0, 50.0));
        [(1.0 - 0.8 * s) * (1.0 - 0.6 * w) * (1.0 - 0.4 * hi), (1.0 - 0.7 * s) * (1.0 - 0.5 * w) * (1.0 - 0.7 * hi), 0.45 * (1.0 - 0.5 * w),
         0.15 + 0.9 * s.max(hi) - 0.5 * w, 0.03 + 1.2 * w * (1.0 - s)]
    };
    let pick = |r: &mut Rng, suit: &[f64; 5]| -> (usize, f64) {
        let ws: Vec<f64> = (0..5).map(|i| (suit[i] * dom[i]).max(0.0)).collect();
        let tot: f64 = ws.iter().sum();
        let mut u = r.f64() * tot;
        for i in 0..5 { if u < ws[i] { return (i, suit[i].clamp(0.2, 1.0)) } u -= ws[i]; }
        (4, suit[4].clamp(0.2, 1.0))
    };
    let seed_ok = |x: f64, y: f64, r: &mut Rng| -> Option<f64> {
        let rr = (x - centre[0]).hypot(y - centre[1]);
        if rr > far { return None }
        let d = if (x - centre[0]).abs() < e * 0.5 && (y - centre[1]).abs() < e * 0.5 { t.dist(x, y) } else { rr };
        if d < cfg.tree_wall_clearance { return None }
        let p = ramp(mask(x, y), thresh - 0.08, thresh + 0.05).max(0.006);
        if r.f64() < p { Some(d) } else { None }
    };
    // plants: pos, species, crown radius, age, vigour, alive
    struct Pl { p: [f64; 2], s: usize, r: f64, age: f64, life: f64, vig: f64, alive: bool }
    let mut pls: Vec<Pl> = vec![];
    let area = std::f64::consts::PI * far * far;
    for _ in 0..(area * cfg.tree_density / 1000.0 * 3.0) as usize {
        let rr = far * r.f64().sqrt();
        let a = r.uniform(0.0, std::f64::consts::TAU);
        let (x, y) = (centre[0] + rr * a.cos(), centre[1] + rr * a.sin());
        if seed_ok(x, y, &mut r).is_none() { continue }
        let (s, vig) = pick(&mut r, &site(x, y));
        let life = sps[s].life * r.uniform(0.75, 1.3);
        let age = r.uniform(0.0, 0.5 * life);
        pls.push(Pl { p: [x, y], s, r: r.uniform(0.3, 1.0) + sps[s].grow * age * 0.5, age, life, vig, alive: true });
    }
    let n_seeded = pls.len();
    let cell = 16.0;
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
    let (mut n_thinned, mut n_old) = (0, 0);
    let cap = pls.len() * 2;
    for step in 0..28 {
        let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, q) in pls.iter().enumerate() { if q.alive { grid.entry(key(q.p)).or_default().push(i); } }
        // shading: per plant, the strongest overlap by a larger crown
        let mut shade = vec![0.0f64; pls.len()];
        for i in 0..pls.len() {
            if !pls[i].alive { continue }
            let (cx, cy) = key(pls[i].p);
            for gx in cx - 1..=cx + 1 { for gy in cy - 1..=cy + 1 {
                let Some(v) = grid.get(&(gx, gy)) else { continue };
                for &j in v {
                    if j == i || pls[j].r <= pls[i].r { continue }
                    let d = (pls[i].p[0] - pls[j].p[0]).hypot(pls[i].p[1] - pls[j].p[1]);
                    let reach = 0.65 * (pls[i].r + pls[j].r);
                    if d < reach { shade[i] = shade[i].max(1.0 - d / reach); }
                }
            }}
        }
        for i in 0..pls.len() {
            let q = &mut pls[i];
            if !q.alive { continue }
            let sp = &sps[q.s];
            q.age += 1.0;
            if shade[i] > 0.0 && r.f64() < shade[i] * (1.0 - sp.tol) * 0.3 { q.alive = false; n_thinned += 1; continue }
            if q.age > q.life { q.alive = false; n_old += 1; continue }
            let light = 1.0 - shade[i] * (1.0 - sp.tol);
            q.r += sp.grow * q.vig * light * (1.0 - q.r / sp.rmax).max(0.0);
        }
        // recruitment: mature trees seed their own species nearby
        if step % 3 == 2 && pls.len() < cap {
            let parents: Vec<([f64; 2], usize)> = pls.iter().filter(|q| q.alive && q.r > 0.5 * sps[q.s].rmax).map(|q| (q.p, q.s)).collect();
            for (pp, s) in parents {
                if r.f64() > sps[s].fec { continue }
                let a = r.uniform(0.0, std::f64::consts::TAU);
                let dd = sps[s].disperse * r.f64().sqrt();
                let (x, y) = (pp[0] + dd * a.cos(), pp[1] + dd * a.sin());
                if seed_ok(x, y, &mut r).is_none() { continue }
                let vig = site(x, y)[s].clamp(0.2, 1.0);
                if r.f64() > vig { continue }
                pls.push(Pl { p: [x, y], s, r: 0.3, age: 0.0, life: sps[s].life * r.uniform(0.75, 1.3), vig, alive: true });
            }
        }
    }
    // (saplings only in the fine grid: past it they are invisible and only cost geometry)
    let trees: Vec<&Pl> = pls.iter().filter(|q| {
        let near = (q.p[0] - centre[0]).abs() < e && (q.p[1] - centre[1]).abs() < e;
        q.alive && q.r >= if near { 0.9 } else { 2.2 }
    }).collect();
    let n_near = trees.iter().filter(|q| (q.p[0] - centre[0]).abs() < e && (q.p[1] - centre[1]).abs() < e).count();
    let mut by_sp = [0usize; 5];
    for q in &trees { by_sp[q.s] += 1; }
    // understory shrubs (near grid only): seeds at grove edges and in the band around the house; deep under a crown they die, and
    // shrubs thin each other (Poisson spacing)
    let mut shrubs: Vec<[f64; 2]> = vec![];
    let mut tgrid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (i, q) in trees.iter().enumerate() { tgrid.entry(key(q.p)).or_default().push(i); }
    let cover = |x: f64, y: f64| -> f64 {
        let (cx, cy) = key([x, y]);
        let mut c = 0.0f64;
        for gx in cx - 1..=cx + 1 { for gy in cy - 1..=cy + 1 {
            for &j in tgrid.get(&(gx, gy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                let d = (x - trees[j].p[0]).hypot(y - trees[j].p[1]);
                if d < 0.8 { return 9.0 }
                c = c.max(1.0 - d / trees[j].r);
            }
        }}
        c
    };
    for _ in 0..((2.0 * e).powi(2) / 100.0 * cfg.bush_density * 0.6) as usize {
        let (x, y) = (centre[0] + r.uniform(-e, e), centre[1] + r.uniform(-e, e));
        let d = t.dist(x, y);
        if d < 1.5 || d > e * 0.97 { continue }
        let m = mask(x, y);
        let near_house = d < 12.0 && r.f64() < 0.35;
        if !(near_house || (m > thresh - 0.14 && r.f64() < 0.5)) { continue }
        let c = cover(x, y);
        if c > 0.55 || r.f64() < c { continue }
        if shrubs.iter().any(|p| (p[0] - x).hypot(p[1] - y) < 2.5) { continue }
        shrubs.push([x, y]);
    }
    log.push(format!("plant community: {n_seeded} seeds, {} trees survive ({n_near} in the fine grid; {n_thinned} thinned, {n_old} died of age; oak/maple/birch/pine/willow {:?}), {} shrubs (to {far:.0} m)",
        trees.len(), by_sp, shrubs.len()));
    // props: trees (crown radius -> scale against the species' nominal crown) and shrubs
    let crown = |s: usize| crate::assets::rtree::crown_radius(crate::assets::rtree::ALL[s]);
    let ground_z = |x: f64, y: f64| {
        let d = if (x - centre[0]).abs() < e && (y - centre[1]).abs() < e { t.dist(x, y) } else { (x - centre[0]).hypot(y - centre[1]) };
        t.height(x, y, d) - 0.05
    };
    for q in &trees {
        let variant = 1000 + r.randint(0, cfg.tree_variants as i64) as u64;
        let scale = (q.r / crown(q.s)).clamp(0.3, 1.4);
        props.push(json!({"kind": "tree", "species": q.s, "crown": q.r, "variant": variant, "loc": [q.p[0], q.p[1], ground_z(q.p[0], q.p[1])],
            "rot": r.uniform(0.0, std::f64::consts::TAU), "scale": scale}));
    }
    for p in &shrubs {
        let variant = 1000 + r.randint(0, cfg.bush_variants as i64) as u64;
        props.push(json!({"kind": "bush", "species": 5, "variant": variant, "loc": [p[0], p[1], ground_z(p[0], p[1])], "rot": r.uniform(0.0, std::f64::consts::TAU), "scale": r.uniform(0.7, 1.4)}));
    }
    // ---- far canopy (after Bruneton & Neyret, "Real-time realistic rendering and lighting of forests", 2012: distant forest as a
    // canopy surface, not trees): past ~0.9 tree_far the grove mask raises a crown-height shell over the far ring, sinking into the
    // ground at the grove edges; off the rock (the ridge material) only
    {
        let bump_seed = r.next_u64() as u32;
        let ch = |x: f64, y: f64, z: f64| -> f64 {
            if z - t.pad_z > 25.0 { return 0.0 }
            smooth(thresh - 0.06, thresh + 0.3, mask(x, y)).powf(1.5) * (11.0 + 5.0 * perlin(x, y, bump_seed, 3, 0.02))
        };
        let mut canopy = Mesh::new();
        let r_of = |p: [f64; 3]| (p[0] - centre[0]).hypot(p[1] - centre[1]);
        for k in 0..nr {
            for a in 0..na {
                let b = (a + 1) % na;
                let q = [ring[k][a], ring[k][b], ring[k + 1][b], ring[k + 1][a]];
                if r_of(q[0]) < 0.9 * far { continue }
                let hs: Vec<f64> = q.iter().map(|p| ch(p[0], p[1], p[2])).collect();
                if hs.iter().all(|h| *h < 0.3) { continue }
                let v: Vec<[f64; 3]> = q.iter().zip(&hs).map(|(p, h)| [p[0], p[1], p[2] + if *h < 0.3 { -1.5 } else { *h }]).collect();
                canopy.add_quad(v[0], v[1], v[2], v[3]);
            }
        }
        if !canopy.is_empty() {
            canopy.weld(1e-6);
            let cm = scene.material(Material::diffuse("outdoor_canopy", [0.03, 0.06, 0.022], 0.95));
            log.push(format!("far canopy: {} quads", canopy.faces.len()));
            scene.add("outdoor_canopy#0", "OutdoorTree", canopy, cm);
        }
    }
    // Rust trees (crates/ig_plants/src/rtree.rs) as scene geometry. Trees in the fine grid are built one by one among their
    // neighbours (rtree::build_crowded: crowns share the space, lean into gaps); far trees and shrubs are one build per (species,
    // variant) copied to every spot. The sidecar props are then left empty (the renderer's Infinigen tree cache is for trees = "infinigen").
    if cfg.trees == "rust" {
        use crate::assets::rtree::{self, Lod};
        let species = rtree::ALL;
        let mut built: std::collections::HashMap<(u8, u64), (Mesh, Mesh, Vec<[f64; 3]>)> = Default::default();
        let mut mats: std::collections::HashMap<u8, (usize, usize)> = Default::default();
        let (mut bark_all, mut leaf_all): (std::collections::HashMap<u8, Mesh>, std::collections::HashMap<u8, Mesh>) = (Default::default(), Default::default());
        let mut cent_all: std::collections::HashMap<u8, Vec<[f64; 3]>> = Default::default();
        let mut inst: std::collections::HashMap<(u8, u64), Vec<[[f64; 4]; 4]>> = Default::default();
        let tree_at: Vec<([f64; 2], f64)> = props.iter().filter(|p| p["kind"] == "tree")
            .map(|p| ([p["loc"][0].as_f64().unwrap(), p["loc"][1].as_f64().unwrap()], p["crown"].as_f64().unwrap())).collect();
        // the near trees' crowded builds, then the props in order (shared builds for the rest)
        let near_of = |pr: &Value| {
            let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap()];
            pr["kind"] == "tree" && (loc[0] - centre[0]).abs().max((loc[1] - centre[1]).abs()) <= e
        };
        let crowded: Vec<Option<(Mesh, Mesh, Vec<[f64; 3]>, bool)>> = {
            props.iter().enumerate().map(|(pi, pr)| {
                if !near_of(pr) { return None }
                let variant = pr["variant"].as_u64().unwrap_or(1000);
                let si = pr["species"].as_u64().unwrap_or(5) as usize;
                let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap()];
                let (rot, sc) = (pr["rot"].as_f64().unwrap_or(0.0), pr["scale"].as_f64().unwrap_or(1.0));
                let bseed = seed ^ (variant * 7919) ^ (si as u64) << 20;
                // neighbours in the tree's local frame (unrotated, unscaled)
                let cr = pr["crown"].as_f64().unwrap_or(1.0);
                let (cs, sn) = ((-rot).cos(), (-rot).sin());
                let nb: Vec<([f64; 2], f64)> = tree_at.iter().filter(|(p, rn)| {
                    let dd = (p[0] - loc[0]).hypot(p[1] - loc[1]);
                    dd > 0.05 && dd < cr + rn
                }).map(|(p, rn)| {
                    let (dx, dy) = ((p[0] - loc[0]) / sc, (p[1] - loc[1]) / sc);
                    ([dx * cs - dy * sn, dx * sn + dy * cs], rn / sc)
                }).collect();
                let t = rtree::build_crowded(species[si], bseed ^ (pi as u64) << 32, Lod::Near, cr / sc, &nb);
                Some((t.bark, t.leaves, t.leaf_centres, !nb.is_empty()))
            }).collect()
        };
        let n_crowded = crowded.iter().filter(|c| c.as_ref().map_or(false, |c| c.3)).count();
        for (pi, pr) in props.iter().enumerate() {
            let variant = pr["variant"].as_u64().unwrap_or(1000);
            let si = pr["species"].as_u64().unwrap_or(5) as usize;
            let loc = [pr["loc"][0].as_f64().unwrap(), pr["loc"][1].as_f64().unwrap(), pr["loc"][2].as_f64().unwrap()];
            let (rot, sc) = (pr["rot"].as_f64().unwrap_or(0.0), pr["scale"].as_f64().unwrap_or(1.0));
            let d = (loc[0] - centre[0]).abs().max((loc[1] - centre[1]).abs());
            let lod = if d > e { Lod::Far } else { Lod::Near };
            let bseed = seed ^ (variant * 7919) ^ (si as u64) << 20;
            let m = Mat4::translation(loc).mul(&Mat4::rot_z(rot)).mul(&Mat4::scale([sc; 3]));
            mats.entry(si as u8).or_insert_with(|| { let (bm, lm) = rtree::materials(species[si], 0.0); (scene.material(bm), scene.material(lm)) });
            if let Some(c) = &crowded[pi] {
                // a one-off build: merged into its species' mesh in world space
                let (mut bb, mut ll) = (c.0.clone(), c.1.clone());
                bb.transform(&m); ll.transform(&m);
                bark_all.entry(si as u8).or_default().append(&bb);
                let le = leaf_all.entry(si as u8).or_default();
                if le.uv.len() != le.pos.len() { le.uv.resize(le.pos.len(), [0.0, 0.0]); }
                le.append(&ll);
                cent_all.entry(si as u8).or_default().extend(c.2.iter().map(|p| m.apply(*p)));
            } else {
                // a shared build: one more instance of it
                let key = (si as u8, variant * 2 + (lod == Lod::Far) as u64);
                built.entry(key).or_insert_with(|| { let t = rtree::build(species[si], bseed, lod); (t.bark, t.leaves, t.leaf_centres) });
                inst.entry(key).or_default().push(m.0);
            }
        }
        let mut keys: Vec<u8> = bark_all.keys().copied().collect(); keys.sort();
        for k in keys {
            let (bi, li) = mats[&k];
            scene.add(&format!("outdoor_rtree_{:?}#0", species[k as usize]), "OutdoorTree", bark_all.remove(&k).unwrap(), bi);
            scene.add_bent(&format!("outdoor_rtree_{:?}#1", species[k as usize]), "OutdoorTree", leaf_all.remove(&k).unwrap(), li, cent_all.remove(&k).unwrap_or_default());
        }
        // shared builds as instanced nodes (one mesh, a transform per tree), in key order
        let mut bkeys: Vec<(u8, u64)> = built.keys().copied().collect(); bkeys.sort();
        let n_shared = bkeys.len();
        let n_inst: usize = inst.values().map(|v| v.len()).sum();
        for k in bkeys {
            let (b, l, lc) = built.remove(&k).unwrap();
            let (bi, li) = mats[&k.0];
            let xs = inst.remove(&k).unwrap_or_default();
            let nm = format!("outdoor_rtree_{:?}_v{}", species[k.0 as usize], k.1);
            scene.add_instanced(&format!("{nm}#0"), "OutdoorTree", b, bi, None, xs.clone());
            scene.add_instanced(&format!("{nm}#1"), "OutdoorTree", l, li, Some(lc), xs);
        }
        log.push(format!("rust trees: {n_crowded} near trees built among neighbours, {n_shared} shared builds placed {n_inst} times"));
        props.clear();
    }
    log.push(format!("terrain {gcls} / {mcls}, mountains {m_height:.0} m coverage {coverage:.2}, pad {:.2} m below the floor", cfg.pad_depth));
    let rec = json!({"terrain": {"ground_material": gcls, "mountain_material": mcls, "mountain_height": m_height, "coverage": coverage, "pad_z": t.pad_z},
        "props": props, "tree_variants": cfg.tree_variants, "bush_variants": cfg.bush_variants});
    (rec, log)
}
