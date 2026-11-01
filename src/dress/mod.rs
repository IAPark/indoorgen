//! Surface dressing (program.top_dress): an explicit registry of the furniture whose surfaces are dressed, and one placement solver per
//! surface type -- shelves ([`crate::shelffill`] + shelf decor), fireplace mantels and hearths ([`fireplace`]), kitchen counter runs
//! ([`counter`]) and bathroom fixtures ([`bath`]). Each solver lays a small composed set into planned slots (pairs, clusters, zones by
//! the sink / stove) instead of sampling random spots; every other surface stays bare apart from what the solver put there.
//!
//! The registry also drives two exclusions so the composed sets are not undone: [`purge`] removes the solver's own small objects from
//! registered surfaces (appliances, lamps, TVs stay), and the clutter pass skips registered surfaces ([`no_clutter_classes`]).
use crate::assets::Asset;
use crate::furn::state::State;
use crate::mesh::{Mesh, Scene};

pub mod bath;
pub mod counter;
pub mod fireplace;
pub mod nightstand;
pub mod props;

/// furniture / feature classes whose surfaces belong to a dresser
pub const DRESSED: &[&str] = &[
    // shelves (book rows: shelffill; decor: shelf decor)
    "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory", "BookshelfWallFeature",
    // mantel + hearth
    "FireplaceFeature",
    // kitchen counter runs
    "KitchenSpaceFactory",
    // bathroom fixtures
    "BathtubFactory", "StandingSinkFactory", "BathroomSinkFactory", "ToiletFactory",
];

/// small objects the solver puts on surfaces; removed from registered surfaces before dressing
const SMALL: &[&str] = &[
    "BottleFactory", "CupFactory", "JarFactory", "BowlFactory", "PlateFactory", "PotFactory", "PanFactory", "WineglassFactory", "VaseFactory",
    "CanFactory", "FoodBoxFactory", "FoodBagFactory", "BookColumnFactory", "BookStackFactory", "BookFactory", "NatureShelfTrinketsFactory",
    "PlantContainerFactory", "FruitContainerFactory", "KnifeFactory", "ForkFactory", "SpoonFactory", "ChopsticksFactory", "TowelFactory",
    "BalloonFactory", "ToiletPaperFactory",
];

/// set while the dressers own their surfaces (other decor passes skip them)
pub static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_dressed(class: &str) -> bool { DRESSED.contains(&class) }

/// classes the clutter pass must not use as surfaces
pub fn no_clutter_classes() -> Vec<String> { DRESSED.iter().map(|s| s.to_string()).collect() }

/// Remove the solver's small objects (and whatever rests on them) from registered surfaces. Returns the removed keys.
pub fn purge(state: &mut State) -> Vec<String> {
    let dressed: Vec<String> = state.objs.iter().filter(|(_, o)| o.generator.as_ref().map_or(false, |g| is_dressed(&g.class))).map(|(k, _)| k.clone()).collect();
    let mut gone: Vec<String> = state.objs.iter().filter(|(_, o)| {
        o.generator.as_ref().map_or(false, |g| SMALL.contains(&g.class.as_str())) && o.relations.iter().any(|r| dressed.contains(&r.target))
    }).map(|(k, _)| k.clone()).collect();
    loop {
        let more: Vec<String> = state.objs.iter().filter(|(k, o)| !gone.contains(k) && o.relations.iter().any(|r| gone.contains(&r.target))).map(|(k, _)| k.clone()).collect();
        if more.is_empty() { break }
        gone.extend(more);
    }
    gone.sort();
    for k in &gone { state.objs.shift_remove(k); }
    gone
}

pub type Box3 = ([f64; 3], [f64; 3]);

pub fn overlaps(a: &Box3, b: &Box3) -> bool { (0..3).all(|d| a.0[d] < b.1[d] && b.0[d] < a.1[d]) }

/// heights (z) of every triangle covering (x, y)
pub fn heights(tris: &[[[f64; 3]; 3]], x: f64, y: f64) -> Vec<f64> {
    let mut out = vec![];
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        if x < a[0].min(b[0]).min(c[0]) || x > a[0].max(b[0]).max(c[0]) || y < a[1].min(b[1]).min(c[1]) || y > a[1].max(b[1]).max(c[1]) { continue }
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-12 { continue }
        let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
        let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
        let l3 = 1.0 - l1 - l2;
        if l1 < -1e-9 || l2 < -1e-9 || l3 < -1e-9 { continue }
        out.push(l1 * a[2] + l2 * b[2] + l3 * c[2]);
    }
    out
}

/// Triangles bucketed on a 2D xy grid (5 cm cells): a height probe only tests the triangles whose xy box covers its cell (testing every
/// triangle of a cabinet per probe made top_dress the slowest pass of the scene build, ~18 s per house)
pub struct TriGrid { tris: Vec<[[f64; 3]; 3]>, lo: [f64; 2], cell: f64, nx: usize, ny: usize, cells: Vec<Vec<u32>> }

impl TriGrid {
    pub fn new(tris: Vec<[[f64; 3]; 3]>) -> TriGrid {
        let cell = 0.05;
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for t in &tris { for p in t { for d in 0..2 { lo[d] = lo[d].min(p[d]); hi[d] = hi[d].max(p[d]); } } }
        if tris.is_empty() { return TriGrid { tris, lo: [0.0; 2], cell, nx: 1, ny: 1, cells: vec![vec![]] } }
        let nx = (((hi[0] - lo[0]) / cell).ceil() as usize).max(1);
        let ny = (((hi[1] - lo[1]) / cell).ceil() as usize).max(1);
        let mut cells = vec![vec![]; nx * ny];
        for (i, t) in tris.iter().enumerate() {
            let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
            for p in t { for d in 0..2 { a[d] = a[d].min(p[d]); b[d] = b[d].max(p[d]); } }
            let (x0, x1) = (((a[0] - lo[0]) / cell) as usize, (((b[0] - lo[0]) / cell) as usize).min(nx - 1));
            let (y0, y1) = (((a[1] - lo[1]) / cell) as usize, (((b[1] - lo[1]) / cell) as usize).min(ny - 1));
            for x in x0.min(nx - 1)..=x1 { for y in y0.min(ny - 1)..=y1 { cells[x * ny + y].push(i as u32); } }
        }
        TriGrid { tris, lo, cell, nx, ny, cells }
    }
    /// same result as [`heights`] on all the triangles
    pub fn heights(&self, x: f64, y: f64) -> Vec<f64> {
        let (fx, fy) = ((x - self.lo[0]) / self.cell, (y - self.lo[1]) / self.cell);
        if fx < 0.0 || fy < 0.0 { return vec![] }
        let (cx, cy) = (fx as usize, fy as usize);
        if cx >= self.nx || cy >= self.ny { return vec![] }
        let sub: Vec<[[f64; 3]; 3]> = self.cells[cx * self.ny + cy].iter().map(|&i| self.tris[i as usize]).collect();
        heights(&sub, x, y)
    }
}

/// all triangles of an asset (its local frame)
pub fn tris_of(a: &Asset) -> Vec<[[f64; 3]; 3]> {
    a.parts.iter().flat_map(|p| p.mesh.faces.iter().flat_map(move |f| (1..f.len().saturating_sub(1)).map(move |i| [p.mesh.pos[f[0] as usize], p.mesh.pos[f[i] as usize], p.mesh.pos[f[i + 1] as usize]]))).collect()
}

/// Level spot of radius `rad` at local (x, y) on a surface whose top lies in z band `band`: all 5 probes find a top in the band within
/// 1.5 cm of each other, nothing of the furniture between that top and `clear` above it, and (`solid`) no surface between `floor_z` and
/// the band under the probes (rims and tank lids, not the basin under them). Returns the top z.
pub fn level_spot(tris: &[[[f64; 3]; 3]], x: f64, y: f64, rad: f64, band: (f64, f64), clear: f64, solid: Option<f64>) -> Option<f64> {
    // (the whole footprint: probing to 80 % of the radius missed a basin rim sloping up under the rest of a soap dispenser's base)
    let probes = [[x, y], [x - rad * 0.8, y - rad * 0.8], [x + rad * 0.8, y - rad * 0.8], [x - rad * 0.8, y + rad * 0.8], [x + rad * 0.8, y + rad * 0.8],
                  [x - rad, y], [x + rad, y], [x, y - rad], [x, y + rad]];
    let mut tops = vec![];
    for pr in probes {
        let hs = heights(tris, pr[0], pr[1]);
        let top = hs.iter().cloned().filter(|z| *z >= band.0 && *z <= band.1).fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m: f64| m.max(z))))?;
        if hs.iter().any(|z| *z > top + 0.01 && *z < top + clear) { return None }
        if let Some(fz) = solid { if hs.iter().any(|z| *z > fz + 0.04 && *z < band.0 - 0.02) { return None } }
        tops.push(top);
    }
    let zt = tops.iter().cloned().fold(f64::MIN, f64::max);
    if tops.iter().any(|z| zt - z > 0.015) { return None }
    Some(zt)
}

/// highest 1 cm height level of an asset's top surface between `zlo` and `zhi` (local z) shared by >= `min_count` cells of a 16 x 8
/// grid over its footprint -- the working surface (counter top, ledge) rather than the bounds' top (taps, raised lids, upper cabinets)
pub fn plateau(a: &Asset, zlo: f64, zhi: f64, min_count: usize) -> Option<f64> {
    let tris = TriGrid::new(tris_of(a));
    let (lo, hi) = a.bounds();
    let mut levels: std::collections::BTreeMap<i64, usize> = Default::default();
    for gx in 0..16 { for gy in 0..8 {
        let (x, y) = (lo[0] + (hi[0] - lo[0]) * (gx as f64 + 0.5) / 16.0, lo[1] + (hi[1] - lo[1]) * (gy as f64 + 0.5) / 8.0);
        let t = tris.heights(x, y).into_iter().filter(|z| *z > zlo && *z < zhi).fold(f64::NAN, f64::max);
        if t.is_finite() { *levels.entry((t * 100.0).round() as i64).or_default() += 1 }
    } }
    levels.iter().rev().find(|(_, c)| **c >= min_count).map(|(z, _)| *z as f64 / 100.0)
}

/// Occupied space for one dressing run: world boxes of every node already in the scene that is not room shell / skirting, plus what
/// the dressers place.
pub struct Occ { pub boxes: Vec<Box3>, pub names: Vec<String> }

impl Occ {
    pub fn from_scene(scene: &Scene, skip_prefix: &[String]) -> Occ {
        // windows: their boxes span the wall reveal and reach down to counters set under them; the glass is in the wall, items cannot touch it
        let ns: Vec<&crate::mesh::Node> = scene.nodes.iter().filter(|n| n.class != "room" && n.class != "Window" && !n.name.starts_with("skirting") && !n.mesh.is_empty() && !skip_prefix.iter().any(|p| n.name.starts_with(p.as_str()))).collect();
        let mut o = Occ { boxes: ns.iter().map(|n| n.mesh.bounds()).collect(), names: ns.iter().map(|n| n.name.clone()).collect() };
        o.add_door_zones(scene);
        o
    }
    /// keep-outs in every door's passage (the QC's door zone: 0.6 m each side of the wall, the opening's width): no dresser puts an
    /// item there (fireplace tools and firewood stood in a doorway; a door leaf swinging over a rug's basket)
    pub fn add_door_zones(&mut self, scene: &Scene) {
        for n in &scene.nodes {
            if n.class != "Door" || !n.name.ends_with(".frame") || n.mesh.is_empty() { continue }
            let (lo, hi) = n.mesh.bounds();
            let thin_x = hi[0] - lo[0] < hi[1] - lo[1];
            let (d, m) = (0.6, 0.06);
            let (l, h) = if thin_x { ([lo[0] - d, lo[1] + m, lo[2] + 0.05], [hi[0] + d, hi[1] - m, hi[2] - 0.1]) } else { ([lo[0] + m, lo[1] - d, lo[2] + 0.05], [hi[0] - m, hi[1] + d, hi[2] - 0.1]) };
            self.boxes.push((l, h)); self.names.push(format!("{} passage", n.name));
        }
    }
    /// keep-outs in front of every window's glass (the QC's window zone: ~0.12 m into the room plus the wall's half thickness, over the
    /// glass height) -- a bedside lamp on a nightstand by a window stood in it
    pub fn add_window_zones(&mut self, scene: &Scene) {
        for n in &scene.nodes {
            if n.class != "Window" || !n.name.ends_with(".glass") || n.mesh.is_empty() { continue }
            let (lo, hi) = n.mesh.bounds();
            // the glass is thin across the wall: grow that axis by 0.2 m each way
            let thin_x = hi[0] - lo[0] < hi[1] - lo[1];
            let g = 0.2;
            // (and out to the frame: the opening is a few cm past the glass all round -- a roll under the glass stood in the sill's zone)
            let (f, fz) = (0.05, 0.08);
            let (l, h) = if thin_x { ([lo[0] - g, lo[1] - f, lo[2] - fz], [hi[0] + g, hi[1] + f, hi[2] + fz]) } else { ([lo[0] - f, lo[1] - g, lo[2] - fz], [hi[0] + f, hi[1] + g, hi[2] + fz]) };
            self.boxes.push((l, h)); self.names.push(format!("{} zone", n.name));
        }
    }
    pub fn free(&self, b: &Box3) -> bool {
        match self.boxes.iter().position(|o| overlaps(b, o)) {
            None => true,
            Some(i) => { if std::env::var_os("DRESSDBG").is_some() { eprintln!("DRESSDBG blocked by {}", self.names.get(i).map(|s| s.as_str()).unwrap_or("(placed item)")) } false }
        }
    }
}

/// Add `asset` (any local frame) to the scene standing on world point `at` (its footprint centre there, its bottom at at.z), turned by
/// `yaw`, uniformly scaled by `scale`. Returns the world box.
pub fn place(scene: &mut Scene, name: &str, class: &str, asset: &Asset, at: [f64; 3], yaw: f64, scale: f64) -> Box3 {
    let (lo, hi) = asset.bounds();
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let (c, s) = (yaw.cos(), yaw.sin());
    let (mut wl, mut wh) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (pi, p) in asset.parts.iter().enumerate() {
        let mut m: Mesh = p.mesh.clone();
        for q in m.pos.iter_mut() {
            let (dx, dy) = ((q[0] - cx) * scale, (q[1] - cy) * scale);
            *q = [at[0] + dx * c - dy * s, at[1] + dx * s + dy * c, at[2] + 0.002 + (q[2] - lo[2]) * scale];
            for d in 0..3 { wl[d] = wl[d].min(q[d]); wh[d] = wh[d].max(q[d]); }
        }
        let mi = scene.material(p.mat.clone());
        scene.add(&format!("{name}#{pi}"), class, m, mi);
    }
    (wl, wh)
}

/// world box `asset` would take at `at` / `yaw` / `scale` (for the free test before placing)
pub fn box_at(asset: &Asset, at: [f64; 3], yaw: f64, scale: f64) -> Box3 {
    let (lo, hi) = asset.bounds();
    let (hx, hy) = ((hi[0] - lo[0]) * scale / 2.0, (hi[1] - lo[1]) * scale / 2.0);
    let (c, s) = (yaw.cos().abs(), yaw.sin().abs());
    let (ex, ey) = (hx * c + hy * s, hx * s + hy * c);
    ([at[0] - ex, at[1] - ey, at[2] + 0.002], [at[0] + ex, at[1] + ey, at[2] + 0.002 + (hi[2] - lo[2]) * scale])
}

/// footprint radius / height of an asset at scale 1
pub fn dims(a: &Asset) -> (f64, f64, f64) {
    let (lo, hi) = a.bounds();
    (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])
}

/// Run every dresser. `features`: the placed room features (fireplaces). Returns log lines.
pub fn run(scene: &mut Scene, state: &State, visuals: &std::collections::HashMap<String, Asset>, features: &[crate::features::PlacedFeature], assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    log.extend(fireplace::dress(scene, state, features, assets, seed));
    log.extend(counter::dress(scene, state, visuals, assets, seed));
    log.extend(bath::dress(scene, state, visuals, assets, seed));
    log.extend(nightstand::dress(scene, state, visuals, assets, seed));
    log
}
