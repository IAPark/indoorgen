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
//! Vegetation: tree and bush spots (Poisson-disk, clear of the walls) go to the sidecar `outdoor.props`; the renderer instances
//! Infinigen's TreeFactory / BushFactory variants there (tools/render/tree_cache.py builds them once in Blender 4.2).
//! Stream: int_hash(seed, "outdoor").
use crate::mesh::{Material, Mesh, Scene};
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
    /// Tree / bush variants the renderer builds and reuses (a cache shared by all houses).
    #[schemars(range(min = 1, max = 32))]
    pub tree_variants: u32,
    #[schemars(range(min = 1, max = 32))]
    pub bush_variants: u32,
}

impl Default for OutdoorCfg {
    fn default() -> Self {
        OutdoorCfg { on: true, near_extent: 70.0, near_spacing: 1.0, far_radius: 2500.0, ground_scale: 4.0, mountain_height: None, mountain_coverage: None, pad_depth: 0.45, pad_blend: 18.0,
            tree_density: 4.0, bush_density: 3.0, tree_far: 400.0, tree_wall_clearance: 5.0, tree_variants: 5, bush_variants: 4 }
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
    let gm = scene.material(spec(&format!("outdoor_ground:{gcls}"), gcls, r.next_u64(), [0.32, 0.28, 0.2]));
    let mm = scene.material(spec(&format!("outdoor_mountain:{mcls}"), mcls, r.next_u64(), [0.4, 0.38, 0.35]));
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
    // ---- vegetation: groves (Infinigen's per-species noise masks, generate_nature.py): trees fill the patches where a low-frequency
    // Perlin mask is high, Poisson-spaced inside them, with clearings between and a few solitary field trees; woodland continues past
    // the fine grid to `tree_far` m (the mid-distance was an empty plain). Bushes gather at grove edges and near the house.
    let mut props: Vec<Value> = vec![];
    let mut placed: Vec<([f64; 2], f64)> = vec![];
    let grove_seed = r.next_u64() as u32;
    let mask = |x: f64, y: f64| perlin(x, y, grove_seed, 3, 0.012);
    let thresh = 0.12 - 0.25 * (cfg.tree_density / 10.0).min(1.0);
    let far = cfg.tree_far.max(e);
    let mut add = |kind: &str, x: f64, y: f64, d: f64, sep: f64, variants: u32, sc: (f64, f64), r: &mut Rng, placed: &mut Vec<([f64; 2], f64)>, props: &mut Vec<Value>| -> bool {
        if placed.iter().any(|(p, rr)| (p[0] - x).hypot(p[1] - y) < rr.max(sep)) { return false }
        let z = t.height(x, y, d) - 0.05;
        let variant = 1000 + r.randint(0, variants as i64) as u64;
        props.push(json!({"kind": kind, "variant": variant, "loc": [x, y, z], "rot": r.uniform(0.0, std::f64::consts::TAU), "scale": r.uniform(sc.0, sc.1)}));
        placed.push(([x, y], sep));
        true
    };
    let (mut n_grove, mut n_solo, mut n_far) = (0, 0, 0);
    // near: within the fine grid
    for _ in 0..(((2.0 * e).powi(2) / 1000.0) * cfg.tree_density * 12.0) as usize {
        let (x, y) = (centre[0] + r.uniform(-e, e), centre[1] + r.uniform(-e, e));
        let d = t.dist(x, y);
        if d < cfg.tree_wall_clearance || d > e * 0.97 { continue }
        let m = mask(x, y);
        let solo = r.f64() < 0.02;
        if m < thresh && !solo { continue }
        if add("tree", x, y, d, if solo { 12.0 } else { 4.5 }, cfg.tree_variants, (0.8, 1.35), &mut r, &mut placed, &mut props) { if solo { n_solo += 1 } else { n_grove += 1 } }
    }
    // far: woodland on the ring out to `far` (sparser sampling, same mask; only on gentle ground)
    let ring_area = std::f64::consts::PI * far * far - (2.0 * e).powi(2);
    for _ in 0..((ring_area / 1000.0) * cfg.tree_density * 3.0) as usize {
        let rr = (e * e + r.f64() * (far * far - e * e)).sqrt();
        let a = r.uniform(0.0, std::f64::consts::TAU);
        let (x, y) = (centre[0] + rr * a.cos(), centre[1] + rr * a.sin());
        if (x - centre[0]).abs() < e && (y - centre[1]).abs() < e { continue }
        if mask(x, y) < thresh { continue }
        if add("tree", x, y, rr, 6.0, cfg.tree_variants, (0.9, 1.4), &mut r, &mut placed, &mut props) { n_far += 1 }
    }
    // bushes: grove edges (mask just below the threshold) and the band around the house
    let mut n_bush = 0;
    for _ in 0..((crate::shp::area(&crate::shp::buffer_mitre(&house, 25.0)) / 100.0) * cfg.bush_density * 6.0) as usize {
        let (x, y) = (centre[0] + r.uniform(-e, e), centre[1] + r.uniform(-e, e));
        let d = t.dist(x, y);
        if d < 1.5 || d > e * 0.97 { continue }
        let m = mask(x, y);
        let near_house = d < 12.0 && r.f64() < 0.35;
        if !(near_house || (m > thresh - 0.12 && m < thresh + 0.05)) { continue }
        if add("bush", x, y, d, 1.8, cfg.bush_variants, (0.7, 1.4), &mut r, &mut placed, &mut props) { n_bush += 1 }
    }
    log.push(format!("{n_grove} grove trees, {n_solo} field trees, {n_far} far trees (to {far:.0} m), {n_bush} bushes"));
    log.push(format!("terrain {gcls} / {mcls}, mountains {m_height:.0} m coverage {coverage:.2}, pad {:.2} m below the floor", cfg.pad_depth));
    let rec = json!({"terrain": {"ground_material": gcls, "mountain_material": mcls, "mountain_height": m_height, "coverage": coverage, "pad_z": t.pad_z},
        "props": props, "tree_variants": cfg.tree_variants, "bush_variants": cfg.bush_variants});
    (rec, log)
}
