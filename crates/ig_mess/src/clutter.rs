//! Clutter objects on floors and surfaces (owner: mess-clutter worker).
//! Runs after the furniture visuals: for every room that holds furniture it picks items per room type and level, places them on the
//! floor (near beds, sofas, chairs, desks, away from doors) and on tabletop-like surfaces found in the final furniture meshes, settles
//! them with rapier against the furniture, and adds the survivors to the scene.
use crate::garment;
use crate::settle::{self, BodyIn, Hull, RoomWorld, Static};
use crate::MessCfg;
use ig_core::mesh::{Mat4, Mesh, Scene, V3};
use ig_core::rng::Rng;
use ig_furn::state::State;
use ig_kit::api::Asset;
use rapier3d_f64::prelude::{Pose, SharedShape};

type Poly = Vec<[f64; 2]>;

/// per room type weights: [bedroom, living-room, kitchen, bathroom, dining-room, other]
type W = [f64; 6];

struct Spec {
    class: &'static str,
    w: W,
    /// largest allowed footprint extent / height of the asset (m)
    max_xy: f64,
    max_z: f64,
}

const SURFACE: &[Spec] = &[
    // decor (Hypersim / 3D-FRONT living spaces: vases, small plants, trinkets on every shelf and sideboard), [bed, living, kitchen, bath, dining, other]
    Spec { class: "VaseFactory", w: [1.0, 2.5, 0.3, 0.3, 2.0, 1.0], max_xy: 0.3, max_z: 0.45 },
    Spec { class: "PlantContainerFactory", w: [1.0, 2.0, 0.8, 0.6, 1.5, 1.0], max_xy: 0.35, max_z: 0.6 },
    Spec { class: "NatureShelfTrinketsFactory", w: [1.0, 1.5, 0.2, 0.3, 1.0, 1.0], max_xy: 0.3, max_z: 0.3 },
    Spec { class: "CupFactory", w: [2.0, 3.0, 4.0, 1.0, 3.0, 2.0], max_xy: 0.13, max_z: 0.14 },
    Spec { class: "BowlFactory", w: [0.0, 1.0, 3.0, 0.0, 2.0, 0.5], max_xy: 0.24, max_z: 0.12 },
    Spec { class: "PlateFactory", w: [0.0, 0.5, 3.0, 0.0, 3.0, 0.0], max_xy: 0.28, max_z: 0.05 },
    Spec { class: "BottleFactory", w: [1.0, 2.0, 2.0, 2.0, 2.0, 1.0], max_xy: 0.15, max_z: 0.4 },
    Spec { class: "CanFactory", w: [0.5, 1.0, 2.0, 0.0, 1.0, 1.0], max_xy: 0.15, max_z: 0.25 },
    Spec { class: "WineglassFactory", w: [0.0, 1.0, 0.5, 0.0, 2.0, 0.0], max_xy: 0.15, max_z: 0.3 },
    Spec { class: "BookFactory", w: [3.0, 3.0, 0.5, 0.3, 0.5, 3.0], max_xy: 0.45, max_z: 0.1 },
    Spec { class: "BookStackFactory", w: [2.0, 2.0, 0.0, 0.0, 0.0, 2.0], max_xy: 0.45, max_z: 0.4 },
    Spec { class: "FoodBoxFactory", w: [0.0, 0.3, 2.0, 0.0, 0.3, 0.0], max_xy: 0.4, max_z: 0.4 },
    Spec { class: "FoodBagFactory", w: [0.0, 0.3, 2.0, 0.0, 0.3, 0.0], max_xy: 0.4, max_z: 0.4 },
    Spec { class: "JarFactory", w: [0.5, 0.5, 1.5, 1.5, 0.5, 0.0], max_xy: 0.25, max_z: 0.35 },
    Spec { class: "TowelFactory", w: [0.5, 0.0, 0.5, 3.0, 0.0, 0.0], max_xy: 0.7, max_z: 0.3 },
];

const FLOOR: &[Spec] = &[
    Spec { class: "BookFactory", w: [2.0, 2.0, 0.3, 0.2, 0.2, 2.0], max_xy: 0.45, max_z: 0.1 },
    Spec { class: "BookStackFactory", w: [1.0, 1.0, 0.0, 0.0, 0.0, 1.0], max_xy: 0.45, max_z: 0.4 },
    Spec { class: "PillowFactory", w: [2.5, 1.5, 0.0, 0.0, 0.0, 0.0], max_xy: 0.8, max_z: 0.35 },
    Spec { class: "TowelFactory", w: [1.0, 0.0, 0.0, 3.0, 0.0, 0.0], max_xy: 0.8, max_z: 0.3 },
    Spec { class: "FoodBagFactory", w: [0.0, 1.0, 1.0, 0.0, 0.3, 0.0], max_xy: 0.4, max_z: 0.4 },
    Spec { class: "CanFactory", w: [0.3, 0.7, 1.0, 0.0, 0.3, 0.3], max_xy: 0.15, max_z: 0.25 },
    Spec { class: "BottleFactory", w: [0.2, 0.3, 1.0, 0.1, 0.05, 0.2], max_xy: 0.15, max_z: 0.4 },
    Spec { class: "CupFactory", w: [0.5, 0.5, 0.3, 0.0, 0.0, 0.3], max_xy: 0.13, max_z: 0.14 },
    // dropped clothes: cloth jobs (re-simulated in Blender at render time), height-field drape as the default mesh
    Spec { class: "ShirtFactory", w: [4.0, 1.5, 0.0, 2.0, 0.0, 1.0], max_xy: 1.4, max_z: 0.3 },
    Spec { class: "PantsFactory", w: [3.0, 1.0, 0.0, 1.5, 0.0, 0.7], max_xy: 1.4, max_z: 0.3 },
];

/// floor clutter per m^2 of room floor at level 1, per room type [bedroom, living, kitchen, bath, dining, other]
const FLOOR_DENSITY: W = [0.45, 0.5, 0.2, 0.5, 0.12, 0.4]; // bedrooms halved: clothes piles dominated bedroom views
/// surface items per m^2 of surface at level 1
const SURFACE_DENSITY: W = [7.0, 6.0, 6.0, 7.0, 5.0, 6.0];

fn room_slot(rt: &str) -> usize {
    match rt {
        "bedroom" => 0,
        "living-room" => 1,
        "kitchen" => 2,
        "bathroom" => 3,
        "dining-room" => 4,
        _ => 5,
    }
}

fn pick<'a>(rng: &mut Rng, specs: &'a [Spec], slot: usize) -> Option<&'a Spec> {
    let w: Vec<f64> = specs.iter().map(|s| s.w[slot]).collect();
    let sum: f64 = w.iter().sum();
    if sum <= 0.0 {
        return None;
    }
    Some(&specs[rng.choice_p(&w.iter().map(|x| x / sum).collect::<Vec<_>>())])
}

fn in_poly(p: [f64; 2], poly: &Poly) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn poly_edge_dist(p: [f64; 2], poly: &Poly) -> f64 {
    let n = poly.len();
    (0..n).map(|i| seg_dist(p, poly[i], poly[(i + 1) % n])).fold(f64::MAX, f64::min)
}

fn rz(yaw: f64) -> [[f64; 2]; 2] {
    [[yaw.cos(), -yaw.sin()], [yaw.sin(), yaw.cos()]]
}

/// class substrings of furniture that carries tabletop-like surfaces for surface clutter
fn carries_surface(class: &str) -> bool {
    const YES: [&str; 12] = ["Fireplace", "Desk", "Table", "Stand", "Shelf", "Bookcase", "Cabinet", "Dresser", "Counter", "Sink", "Wardrobe", "Nightstand"];
    const NO: [&str; 6] = ["Lamp", "Plant", "Monitor", "Trinket", "Book", "Light"];
    YES.iter().any(|y| class.contains(y)) && !NO.iter().any(|n| class.contains(n))
}

/// furniture classes that attract floor clutter
fn attracts(class: &str) -> f64 {
    for (s, w) in [("Bed", 1.0), ("Sofa", 1.0), ("ArmChair", 0.8), ("Chair", 0.8), ("Desk", 0.8), ("Wardrobe", 0.9), ("Dresser", 0.8), ("Cabinet", 0.5), ("Shelf", 0.4), ("Bookcase", 0.4), ("Table", 0.5)] {
        if class.contains(s) {
            return w;
        }
    }
    0.0
}

struct Placed {
    shapes: Vec<(Pose, SharedShape)>,
    lo: V3,
    hi: V3,
}

struct Item {
    class: &'static str,
    asset: Asset,
    hulls: Vec<Hull>,
    /// local bbox
    lo: V3,
    hi: V3,
    body: BodyIn,
    floor: bool,
    garment: bool,
    drop_t: V3,
    drop_yaw: f64,
    /// free height needed above the drop spot
    head: f64,
}

#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub floor: usize,
    pub surface: usize,
    pub tried: usize,
    pub removed: usize,
}

/// first draws of ig_decor Cup::new: is_short, [straight, x_lowest, depth, has_guard]
fn cup_has_handle(fseed: u64) -> bool {
    let mut r = Rng::seeded(fseed);
    if r.uniform(0.0, 1.0) < 0.5 {
        r.uniform(0.0, 1.0);
        r.log_uniform(0.6, 0.9);
        r.log_uniform(0.25, 0.5);
        r.uniform(0.0, 1.0) < 0.8
    } else {
        false
    }
}

/// A well-conditioned point set for the convex hull of a (round, high-poly) part: extreme points along ~96 directions plus a snapped,
/// subsampled bottom ring. Hulls of raw dense vertices have thousands of near-coplanar facets, and the contact manifold then rocks on a
/// single tiny facet forever; with few large facets the item comes to rest.
fn simplify_hull(pos: &[V3]) -> Hull {
    if pos.len() <= 16 {
        return pos.to_vec();
    }
    let mut out: Hull = vec![];
    let n = 96;
    let ga = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    let mut used = std::collections::BTreeSet::new();
    for i in 0..n {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
        let r = (1.0 - z * z).sqrt();
        let d = [r * (ga * i as f64).cos(), r * (ga * i as f64).sin(), z];
        let mut best = (f64::MIN, 0usize);
        for (k, p) in pos.iter().enumerate() {
            let v = p[0] * d[0] + p[1] * d[1] + p[2] * d[2];
            if v > best.0 {
                best = (v, k);
            }
        }
        if used.insert(best.1) {
            out.push(pos[best.1]);
        }
    }
    let minz = pos.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    let mut ring: Vec<V3> = pos.iter().filter(|p| p[2] < minz + 0.0007).map(|p| [p[0], p[1], minz]).collect();
    if ring.len() > 3 {
        let c = [ring.iter().map(|p| p[0]).sum::<f64>() / ring.len() as f64, ring.iter().map(|p| p[1]).sum::<f64>() / ring.len() as f64];
        ring.sort_by(|a, b| (a[1] - c[1]).atan2(a[0] - c[0]).partial_cmp(&(b[1] - c[1]).atan2(b[0] - c[0])).unwrap());
        let step = (ring.len() / 20).max(1);
        out.extend(ring.iter().step_by(step).copied());
    } else {
        out.extend(ring);
    }
    out
}

/// Build one clutter asset from a fresh factory seed. `want_handle`: Some(true) only handled cups, Some(false) only plain ones.
fn build_item(rng: &mut Rng, spec: &'static Spec, cup_handle: Option<bool>) -> Option<Item> {
    let mut fseed = rng.randint(0, 1_000_000) as u64;
    let iseed = rng.randint(0, 1_000_000) as u64;
    if spec.class == "ShirtFactory" || spec.class == "PantsFactory" {
        let mut gr = Rng::seeded(fseed);
        let kind = if spec.class == "ShirtFactory" { garment::Kind::Shirt } else { garment::Kind::Pants };
        let mut g = garment::make(&mut gr, kind);
        garment::crumple(&mut g.mesh, &mut gr, 1.0);
        let (mut lo, mut hi) = g.mesh.bounds();
        for q in &mut g.mesh.pos {
            q[2] -= lo[2];
        }
        hi[2] -= lo[2];
        lo[2] = 0.0;
        let hull: Hull = (0..8).map(|i| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]).collect();
        let mut asset = Asset::default();
        asset.parts.push(ig_kit::api::Part { mesh: g.mesh, mat: g.mat, col: None });
        let body = BodyIn { hulls: vec![hull.clone()], t: [0.0; 3], yaw: 0.0, tilt: [0.0; 2], density: 100.0 };
        return Some(Item { class: spec.class, asset, hulls: vec![hull], lo, hi, body, floor: true, garment: true, drop_t: [0.0; 3], drop_yaw: 0.0, head: 0.45 });
    }
    if spec.class == "CupFactory" {
        // cups with a handle run an exact boolean (~1.7 s): redraw until the requested kind
        let want = cup_handle.unwrap_or(false);
        let mut n = 0;
        while cup_has_handle(fseed) != want && n < 50 {
            fseed = rng.randint(0, 1_000_000) as u64;
            n += 1;
        }
    }
    // constructor draws some factories need (Pillow, Towel, ...)
    ig_kit::ctor_draws::set_unseeded(ig_kit::ctor_draws::ctor_draws(spec.class, &mut Rng::seeded(fseed)));
    let f = ig_assets::make_factory(spec.class, fseed);
    let Some(f) = f else {
        ig_kit::ctor_draws::set_unseeded(None);
        return None;
    };
    let mut asset = ig_kit::api::scene_asset(&*f, iseed);
    ig_kit::ctor_draws::set_unseeded(None);
    if asset.parts.is_empty() {
        return None;
    }
    // bbox over all parts; put the lowest point at z = 0 in the local frame
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in &asset.parts {
        if p.mesh.is_empty() {
            continue;
        }
        let (a, b) = p.mesh.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let (mut ex, mut ey, mut ez) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
    // oversized samples (factory ranges run large: 20 cm mugs) are scaled down to the limit; more than 2x too big: rejected
    let sc = (spec.max_xy / ex.max(ey)).min(spec.max_z / ez).min(1.0);
    if sc < 0.5 || ex.max(ey) < 0.015 || ez < 0.004 {
        return None;
    }
    if sc < 1.0 {
        for p in &mut asset.parts {
            for q in &mut p.mesh.pos {
                for k in 0..3 { q[k] *= sc; }
            }
        }
        for k in 0..3 { lo[k] *= sc; hi[k] *= sc; }
        ex *= sc; ey *= sc; ez *= sc;
    }
    let _ = (ex, ey, ez);
    let dz = -lo[2];
    for p in &mut asset.parts {
        for q in &mut p.mesh.pos {
            q[2] += dz;
        }
    }
    lo[2] += dz;
    hi[2] += dz;
    let mut hulls = vec![];
    if spec.class == "TowelFactory" {
        // folded/rolled towel: a box is a robust and close hull
        let mut pts = vec![];
        for i in 0..8 {
            pts.push([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]);
        }
        hulls.push(pts);
    }
    for p in &asset.parts {
        if spec.class == "TowelFactory" {
            break;
        }
        if p.mesh.pos.len() < 4 {
            continue;
        }
        let pts = simplify_hull(&p.mesh.pos);
        if settle::hull_shape(&pts).is_some() {
            hulls.push(pts);
        }
    }
    if hulls.is_empty() {
        return None;
    }
    let body = BodyIn { hulls: hulls.clone(), t: [0.0; 3], yaw: 0.0, tilt: [0.0; 2], density: 400.0 };
    let head = hi[2] - lo[2] + 0.06;
    Some(Item { class: spec.class, asset, hulls, lo, hi, body, floor: false, garment: false, drop_t: [0.0; 3], drop_yaw: 0.0, head })
}

pub fn apply(scene: &mut Scene, state: &State, visuals: &std::collections::HashMap<String, Asset>, rooms: &[(String, ig_core::shp::G)], cfg: &MessCfg) {
    apply_keepout(scene, state, visuals, rooms, cfg, &[]);
}

/// `keepout`: world-space meshes of the volumes occupied by people (static colliders for the placement and the settling)
pub fn apply_keepout(scene: &mut Scene, state: &State, _visuals: &std::collections::HashMap<String, Asset>, rooms: &[(String, ig_core::shp::G)], cfg: &MessCfg, keepout: &[Mesh]) {
    if !cfg.on() {
        return;
    }
    let t0 = web_time::Instant::now();
    let mut tot = Stats::default();
    for (rname, g) in rooms {
        let poly: Poly = ig_core::shp::exterior(g);
        if poly.len() < 3 {
            continue;
        }
        let rt = rname.split('_').next().unwrap_or("").to_string();
        let st = clutter_room(scene, state, rname, &rt, &poly, cfg, keepout);
        tot.floor += st.floor;
        tot.surface += st.surface;
        tot.tried += st.tried;
        tot.removed += st.removed;
    }
    if std::env::var_os("MESSCHECK").is_some() {
        let r = crate::check::audit(scene, rooms);
        eprintln!("MESSCHECK items={} max_pen_furniture={:.4} max_pen_item={:.4} floating_gt1cm={} max_gap={:.4} outside={}", r.items, r.max_pen_furniture, r.max_pen_item, r.floating, r.max_gap, r.outside);
    }
    eprintln!("MESS level={} floor={} surface={} tried={} removed={} time={:?}", cfg.level, tot.floor, tot.surface, tot.tried, tot.removed, t0.elapsed());
}

fn node_obj(name: &str) -> &str {
    name.split('#').next().unwrap_or(name)
}

/// everything the placement needs about one room
struct Room<'a> {
    poly: &'a Poly,
    bbox: [f64; 4],
    floor_z: f64,
    doors: Vec<[f64; 2]>,
    obstacles: Vec<(f64, V3, V3)>,
    surf_tris: Vec<([V3; 3], f64)>,
    cum: Vec<f64>,
    world: RoomWorld,
    placed: Vec<Placed>,
}

impl Room<'_> {
    /// find a spot for `it` (floor or surface) that is free of furniture and of earlier items; sets its drop pose
    fn place(&mut self, rng: &mut Rng, it: &mut Item, floor: bool) -> bool {
        let yaw = rng.uniform(0.0, std::f64::consts::TAU);
        let (cx, cy) = ((it.lo[0] + it.hi[0]) / 2.0, (it.lo[1] + it.hi[1]) / 2.0);
        let (hx, hy) = ((it.hi[0] - it.lo[0]) / 2.0, (it.hi[1] - it.lo[1]) / 2.0);
        let rad = hx.hypot(hy);
        let hgt = it.hi[2] - it.lo[2];
        let [bx0, by0, bx1, by1] = self.bbox;
        let total = self.cum.last().copied().unwrap_or(0.0);
        for _ in 0..40 {
            let (px, py, pz);
            if floor {
                let (x, y) = (rng.uniform(bx0, bx1), rng.uniform(by0, by1));
                if !in_poly([x, y], self.poly) || poly_edge_dist([x, y], self.poly) < 0.22 + rad {
                    continue;
                }
                if self.doors.iter().any(|d| (d[0] - x).hypot(d[1] - y) < 1.15) {
                    continue;
                }
                let mut walk = false;
                for i in 0..self.doors.len() {
                    for j in i + 1..self.doors.len() {
                        if seg_dist([x, y], self.doors[i], self.doors[j]) < 0.45 {
                            walk = true;
                        }
                    }
                }
                if walk && rng.f64() < 0.85 {
                    continue;
                }
                let mut best = 0.0f64;
                for (w, lo, hi) in &self.obstacles {
                    let dx = (lo[0] - x).max(0.0).max(x - hi[0]);
                    let dy = (lo[1] - y).max(0.0).max(y - hi[1]);
                    best = best.max(w * (-dx.hypot(dy) / 0.55).exp());
                }
                if rng.f64() > 0.08 + 0.92 * best {
                    continue;
                }
                let Some(t) = self.world.ray([x, y, self.floor_z + 0.35], [0.0, 0.0, -1.0], 0.9) else { continue };
                let z = self.floor_z + 0.35 - t;
                if (z - self.floor_z).abs() > 0.04 {
                    continue;
                }
                (px, py, pz) = (x, y, z);
            } else {
                if self.cum.is_empty() {
                    return false;
                }
                let u = rng.f64() * total;
                let ti = self.cum.partition_point(|&c| c < u).min(self.surf_tris.len() - 1);
                let tri = &self.surf_tris[ti].0;
                let (mut a, mut b) = (rng.f64(), rng.f64());
                if a + b > 1.0 {
                    a = 1.0 - a;
                    b = 1.0 - b;
                }
                let p: V3 = std::array::from_fn(|k| tri[0][k] + a * (tri[1][k] - tri[0][k]) + b * (tri[2][k] - tri[0][k]));
                (px, py, pz) = (p[0], p[1], p[2]);
                if !in_poly([px, py], self.poly) {
                    continue;
                }
                let rm = rz(yaw);
                let mut supported = true;
                for (sx, sy) in [(0.0, 0.0), (0.8, 0.8), (-0.8, 0.8), (0.8, -0.8), (-0.8, -0.8)] {
                    let (lx, ly) = (sx * hx, sy * hy);
                    let (wx, wy) = (px + rm[0][0] * lx + rm[0][1] * ly, py + rm[1][0] * lx + rm[1][1] * ly);
                    match self.world.ray([wx, wy, pz + 0.05], [0.0, 0.0, -1.0], 0.1) {
                        Some(t) if (0.05 - t).abs() < 0.012 => {}
                        _ => {
                            supported = false;
                            break;
                        }
                    }
                }
                if !supported {
                    continue;
                }
            }
            if self.world.ray([px, py, pz + 0.004], [0.0, 0.0, 1.0], it.head.max(hgt + 0.06)).is_some() {
                continue;
            }
            if it.garment {
                // the whole outline must lie on open floor: nothing within 0.4 m above any sample, floor level below it
                let rm = rz(yaw);
                let pts = &it.asset.parts[0].mesh.pos;
                let mut open = true;
                for q in pts.iter().step_by((pts.len() / 60).max(1)) {
                    let (lx, ly) = (q[0] - cx, q[1] - cy);
                    let (wx, wy) = (px + rm[0][0] * lx + rm[0][1] * ly, py + rm[1][0] * lx + rm[1][1] * ly);
                    if self.world.ray([wx, wy, pz + 0.004], [0.0, 0.0, 1.0], 0.4).is_some() || !in_poly([wx, wy], self.poly) || poly_edge_dist([wx, wy], self.poly) < 0.15 {
                        open = false;
                        break;
                    }
                    match self.world.ray([wx, wy, pz + 0.1], [0.0, 0.0, -1.0], 0.3) {
                        Some(t) if (t - 0.1).abs() < 0.04 => {}
                        _ => {
                            open = false;
                            break;
                        }
                    }
                }
                if !open {
                    continue;
                }
            }
            // asset origin so that the local bbox centre sits at p, bottom 12 mm above the surface
            let rm = rz(yaw);
            let t = [px - (rm[0][0] * cx + rm[0][1] * cy), py - (rm[1][0] * cx + rm[1][1] * cy), pz + 0.012];
            it.body.t = t;
            it.body.yaw = yaw;
            it.drop_t = t;
            it.drop_yaw = yaw;
            it.body.tilt = if floor && !it.garment && rng.f64() < 0.25 { [rng.uniform(-0.25, 0.25), rng.uniform(-0.25, 0.25)] } else { [0.0; 2] };
            let pose = settle::pose_of(t, yaw, it.body.tilt);
            let shapes: Vec<(Pose, SharedShape)> = it.hulls.iter().filter_map(|h| settle::hull_shape(h).map(|s| (pose, s))).collect();
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            for (p, s) in &shapes {
                let b = s.compute_aabb(p);
                for (k, (l, h)) in [(b.mins.x, b.maxs.x), (b.mins.y, b.maxs.y), (b.mins.z, b.maxs.z)].into_iter().enumerate() {
                    lo[k] = lo[k].min(l);
                    hi[k] = hi[k].max(h);
                }
            }
            let mut clash = self.world.hits_static(&shapes, 0.002);
            if !clash {
                'o: for o in &self.placed {
                    if (0..3).any(|k| o.hi[k] + 0.01 < lo[k] || o.lo[k] - 0.01 > hi[k]) {
                        continue;
                    }
                    for (p1, s1) in &shapes {
                        for (p2, s2) in &o.shapes {
                            if let Ok(Some(c)) = rapier3d_f64::parry::query::contact(p1, &**s1, p2, &**s2, 0.0) {
                                if c.dist < 0.01 {
                                    clash = true;
                                    break 'o;
                                }
                            }
                        }
                    }
                }
            }
            if clash {
                continue;
            }
            self.placed.push(Placed { shapes, lo, hi });
            it.floor = floor;
            return true;
        }
        false
    }
}

fn find_spec(specs: &'static [Spec], class: &str) -> &'static Spec {
    specs.iter().find(|s| s.class == class).unwrap()
}

fn clutter_room(scene: &mut Scene, state: &State, rname: &str, rt: &str, poly: &Poly, cfg: &MessCfg, keepout: &[Mesh]) -> Stats {
    let t_room = web_time::Instant::now();
    let dbg = std::env::var_os("MESSDBG").is_some();
    let mut stats = Stats::default();
    let slot = room_slot(rt);
    let (mut bx0, mut by0, mut bx1, mut by1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in poly {
        bx0 = bx0.min(p[0]);
        by0 = by0.min(p[1]);
        bx1 = bx1.max(p[0]);
        by1 = by1.max(p[1]);
    }
    // furniture of this room
    let mut furn: Vec<&str> = vec![];
    let mut doors: Vec<[f64; 2]> = vec![];
    for (k, o) in &state.objs {
        let xy = [o.pose.t[0], o.pose.t[1]];
        if o.generator.is_some() {
            if in_poly(xy, poly) {
                furn.push(k.as_str());
            }
        } else if (k.starts_with("door") || k.starts_with("entrance") || k.starts_with("open")) && poly_edge_dist(xy, poly) < 0.6 {
            doors.push(xy);
        }
    }
    if furn.is_empty() {
        return stats;
    }
    let furn_keys: std::collections::HashSet<&str> = furn.iter().copied().collect();
    let floor_name = format!("{rname}.floor");
    let Some(floor_node) = scene.nodes.iter().find(|n| n.name == floor_name) else { return stats };
    let floor_z = floor_node.mesh.bounds().1[2];
    let mut statics: Vec<Static> = vec![];
    let mut surf_tris: Vec<([V3; 3], f64)> = vec![];
    let mut obstacles: Vec<(f64, V3, V3)> = vec![];
    let pad = 0.15;
    for n in &scene.nodes {
        if n.class == "room" {
            if n.name.starts_with(rname) && (n.name.ends_with(".floor") || n.name.ends_with(".wall") || n.name.ends_with(".wall.alt")) {
                if let Some(s) = Static::from_mesh(&n.mesh, 20000) {
                    statics.push(s);
                }
            }
            continue;
        }
        if n.class.contains("CeilingLight") || n.name.starts_with("skirting_ceiling") || n.name.starts_with("mess") || n.mesh.is_empty() {
            continue;
        }
        let (lo, hi) = n.mesh.bounds();
        if hi[0] < bx0 - pad || lo[0] > bx1 + pad || hi[1] < by0 - pad || lo[1] > by1 + pad {
            continue;
        }
        let in_room = furn_keys.contains(node_obj(&n.name));
        if !in_room && !(n.class == "Door" || n.class == "Window" || n.class == "skirting") {
            continue;
        }
        if let Some(s) = Static::from_mesh(&n.mesh, 20000) {
            statics.push(s);
        }
        if in_room {
            let w = attracts(&n.class);
            if w > 0.0 && hi[2] - lo[2] > 0.05 && lo[2] < floor_z + 0.6 {
                obstacles.push((w, lo, hi));
            }
            if carries_surface(&n.class) {
                for t in n.mesh.triangles() {
                    let (a, b, c) = (n.mesh.pos[t[0] as usize], n.mesh.pos[t[1] as usize], n.mesh.pos[t[2] as usize]);
                    let nn = ig_core::mesh::cross(ig_core::mesh::sub(b, a), ig_core::mesh::sub(c, a));
                    let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
                    let area = l / 2.0;
                    if area < 1e-4 || nn[2] / l < 0.98 {
                        continue;
                    }
                    let z = (a[2] + b[2] + c[2]) / 3.0;
                    if z < floor_z + 0.25 || z > floor_z + 1.8 {
                        continue;
                    }
                    surf_tris.push(([a, b, c], area));
                }
            }
        }
    }
    for k in keepout {
        let (lo, hi) = k.bounds();
        if hi[0] < bx0 - pad || lo[0] > bx1 + pad || hi[1] < by0 - pad || lo[1] > by1 + pad {
            continue;
        }
        if let Some(s) = Static::from_mesh(k, 20000) {
            statics.push(s);
        }
    }
    let world = RoomWorld::new(&statics);
    let area_total = ig_core::shp::signed_area(poly).abs();
    let surf_area: f64 = surf_tris.iter().map(|t| t.1).sum();
    let n_floor_target = ((cfg.level * cfg.floor_scale * FLOOR_DENSITY[slot] * area_total).round() as usize).min(45);
    let n_surf_target = ((cfg.level * cfg.surface_scale * SURFACE_DENSITY[slot] * surf_area).round() as usize).min((40.0 * cfg.surface_scale.max(1.0)) as usize);
    if dbg {
        eprintln!("MESSDBG {rname} area {area_total:.1} surf_area {surf_area:.2} tris {} statics {} targets floor {n_floor_target} surf {n_surf_target} floor_z {floor_z:.3} doors {}", surf_tris.len(), world.n_static(), doors.len());
    }
    let mut cum = vec![];
    let mut s = 0.0;
    for t in &surf_tris {
        s += t.1;
        cum.push(s);
    }
    let mut room = Room { poly, bbox: [bx0, by0, bx1, by1], floor_z, doors, obstacles, surf_tris, cum, world, placed: vec![] };
    let mut rng = cfg.rng(&format!("clutter.{rname}"));

    // candidates are drawn first (deterministic), built in parallel, then placed in order until the targets are met.
    // The two slow assets (pillow ~2.5 s, cup with a handle ~1.7 s) are at most one each per room and come first.
    struct Cand {
        spec: &'static Spec,
        floor: bool,
        seed: u64,
        handle: Option<bool>,
    }
    let mut srng = cfg.rng(&format!("clutter.slow.{rname}"));
    let want_pillow = FLOOR.iter().find(|s| s.class == "PillowFactory").unwrap().w[slot] > 0.0 && n_floor_target >= 2 && srng.f64() < (0.4 + cfg.level * 0.6).min(1.0);
    let want_cup = SURFACE[0].w[slot] > 0.0 && n_surf_target >= 2 && srng.f64() < 0.8;
    let mut cands: Vec<Cand> = vec![];
    if want_cup {
        cands.push(Cand { spec: find_spec(SURFACE, "CupFactory"), floor: false, seed: srng.next_u64(), handle: Some(true) });
    }
    if want_pillow {
        cands.push(Cand { spec: find_spec(FLOOR, "PillowFactory"), floor: true, seed: srng.next_u64(), handle: None });
    }
    for (floor, n_want) in [(false, n_surf_target), (true, n_floor_target)] {
        for _ in 0..(n_want * 3 / 2 + 3) {
            let Some(spec) = pick(&mut rng, if floor { FLOOR } else { SURFACE }, slot) else { break };
            if spec.class == "PillowFactory" {
                continue;
            }
            cands.push(Cand { spec, floor, seed: rng.next_u64(), handle: Some(false) });
        }
    }
    let tb = web_time::Instant::now();
    let mut built: Vec<Option<Item>> = {
        use rayon::prelude::*;
        cands.par_iter().map(|c| build_item(&mut Rng::seeded(c.seed), c.spec, c.handle)).collect()
    };
    if dbg {
        eprintln!("MESSDBG {rname} built {} candidates in {:.2}s", cands.len(), tb.elapsed().as_secs_f64());
    }
    let mut items: Vec<Item> = vec![];
    for floor in [false, true] {
        let n_want = if floor { n_floor_target } else { n_surf_target };
        let mut got = 0;
        for (c, it) in cands.iter().zip(built.iter_mut()) {
            if c.floor != floor || got >= n_want {
                continue;
            }
            let Some(mut it) = it.take() else { continue };
            stats.tried += 1;
            if room.place(&mut rng, &mut it, floor) {
                items.push(it);
                got += 1;
            }
        }
    }
    let t_build = tb.elapsed().as_secs_f64();
    if items.is_empty() {
        return stats;
    }
    let mut world = room.world;
    let rigid: Vec<usize> = (0..items.len()).filter(|&i| !items[i].garment).collect();
    let bodies: Vec<BodyIn> = rigid.iter().map(|&i| std::mem::replace(&mut items[i].body, BodyIn { hulls: vec![], t: [0.0; 3], yaw: 0.0, tilt: [0.0; 2], density: 1.0 })).collect();
    if dbg {
        for (k, &i) in rigid.iter().enumerate() {
            eprintln!("MESSDBG body {k}: {} floor={} t={:.2?}", items[i].class, items[i].floor, bodies[k].t);
        }
    }
    let ts = web_time::Instant::now();
    let rest_rigid = world.settle(&bodies, 480);
    if dbg {
        eprintln!("MESSDBG {rname} build+place {t_build:.2}s settle {:.2}s total {:.2}s", ts.elapsed().as_secs_f64(), t_room.elapsed().as_secs_f64());
    }
    let mut rest: Vec<Option<crate::settle::Rest>> = vec![None; items.len()];
    for (k, &i) in rigid.iter().enumerate() {
        rest[i] = Some(rest_rigid[k].clone());
    }
    for (idx, it) in items.iter().enumerate() {
        let (m, ok) = match &rest[idx] {
            Some(r) => {
                let m = Mat4(r.m);
                let c = m.apply([(it.lo[0] + it.hi[0]) / 2.0, (it.lo[1] + it.hi[1]) / 2.0, (it.lo[2] + it.hi[2]) / 2.0]);
                let bad = r.pen_static > 0.008 || r.pen_item > 0.008 || r.speed > 0.5 || r.gap > 0.01 || !in_poly([c[0], c[1]], poly) || poly_edge_dist([c[0], c[1]], poly) < 0.1;
                if bad && dbg {
                    eprintln!("  removed {} pen_static {:.4} pen_item {:.4} speed {:.3} gap {:.3} floor {}", it.class, r.pen_static, r.pen_item, r.speed, r.gap.min(9.0), it.floor);
                }
                (m, !bad)
            }
            None => {
                // garment: stays where it was placed (cloth job drops it from 0.15 m)
                let t = it.drop_t;
                (Mat4(settle::mat_of(&settle::pose_of(t, it.drop_yaw, [0.0; 2]))), true)
            }
        };
        if !ok {
            stats.removed += 1;
            continue;
        }
        if it.floor {
            stats.floor += 1;
        } else {
            stats.surface += 1;
        }
        let nm_base = format!("mess_{rname}_{idx}_{}", it.class);
        if it.garment {
            let part = &it.asset.parts[0];
            let mut pre = part.mesh.clone();
            pre.transform(&m);
            // pre-sim state: 0.15 m above the support
            let lift = 0.22 - 0.012;
            for q in &mut pre.pos {
                q[2] += lift;
            }
            // default mesh: height-field drape (each vertex lands on whatever lies below it)
            let mut draped = part.mesh.clone();
            draped.transform(&m);
            let mut rr = cfg.rng(&format!("drape.{rname}.{idx}"));
            let wob = rr.uniform(0.0, 6.28);
            let mut support: Vec<f64> = Vec::with_capacity(draped.pos.len());
            for q in &draped.pos {
                let top = q[2] + 1.0;
                let mut sz = floor_z;
                for (ox, oy) in [(0.0, 0.0), (0.025, 0.0), (-0.025, 0.0), (0.0, 0.025), (0.0, -0.025)] {
                    if let Some(t) = world.ray([q[0] + ox, q[1] + oy, top], [0.0, 0.0, -1.0], 1.6) {
                        sz = sz.max(top - t);
                    }
                }
                support.push(sz);
            }
            // cloth cannot drop like a cliff beside a narrow support (chair back): the surface falls at most `slope` m per m away from
            // the highest support (cone dilation over the garment's own vertices), otherwise it becomes a tall spike
            let slope = 1.3;
            let xy: Vec<[f64; 2]> = draped.pos.iter().map(|q| [q[0], q[1]]).collect();
            let mut dil = support.clone();
            if support.iter().cloned().fold(f64::MIN, f64::max) - support.iter().cloned().fold(f64::MAX, f64::min) > 0.05 {
                for i in 0..xy.len() {
                    for j in 0..xy.len() {
                        if support[j] > dil[i] {
                            let d = ((xy[i][0] - xy[j][0]).powi(2) + (xy[i][1] - xy[j][1]).powi(2)).sqrt();
                            dil[i] = dil[i].max(support[j] - slope * d);
                        }
                    }
                }
            }
            // and a garment never stands more than 0.3 m above the typical support under it (no tall cones over chair backs)
            let mut sorted = support.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let cap = sorted[sorted.len() / 2] + 0.3;
            for (k, q) in draped.pos.iter_mut().enumerate() {
                let local_z = q[2] - m.0[2][3];
                q[2] = dil[k].min(cap) + local_z + 0.003 + 0.004 * (q[0] * 23.0 + wob).sin() * (q[1] * 19.0).sin();
            }
            let (glo, ghi) = pre.bounds();
            let mut collide: Vec<String> = vec![format!("{rname}.floor")];
            for n in &scene.nodes {
                if n.class == "room" || n.name.starts_with("mess_") || n.mesh.is_empty() || n.class == "Door" || n.class == "Window" || n.class == "skirting" || n.class.contains("Light") {
                    continue;
                }
                let (a, b) = n.mesh.bounds();
                if a[0] > ghi[0] + 0.2 || b[0] < glo[0] - 0.2 || a[1] > ghi[1] + 0.2 || b[1] < glo[1] - 0.2 || a[2] > floor_z + 0.9 {
                    continue;
                }
                if n.mesh.pos.len() > 120000 {
                    continue;
                }
                collide.push(n.name.clone());
            }
            collide.truncate(8);
            if std::env::var_os("GARMENT_HF").is_none() {
                // default mesh: the garment falls onto the floor / furniture in the Rust cloth solver (soft folds, thickness kept)
                let mut cpos: Vec<V3> = vec![];
                let mut cfaces: Vec<Vec<u32>> = vec![];
                for nm in &collide {
                    if let Some(n) = scene.nodes.iter().find(|n| &n.name == nm) {
                        let o = cpos.len() as u32;
                        cpos.extend_from_slice(&n.mesh.pos);
                        cfaces.extend(n.mesh.faces.iter().map(|f| f.iter().map(|i| i + o).collect::<Vec<u32>>()));
                    }
                }
                let col = ig_kit::cloth::Collider::new(&cpos, &cfaces);
                let mut cp = ig_kit::cloth::ClothParams::default();
                cp.mass = 0.05;
                cp.tension = 2.0;
                cp.distance_min = 0.002;
                cp.end_frame = 40;
                let mut cal = ig_kit::cloth::Calib::default();
                cal.col_thickness = 0.006;
                let out = ig_kit::cloth::simulate(&pre.pos, &pre.faces, &cp, &[&col], &cal);
                let mut sim = pre.clone();
                sim.pos = out;
                draped = ig_shared::subsurf::subsurf(&sim, 1, false);
                // the solver keeps a collision skin (~8 mm) above what the garment lies on (floor, rug, bed): close the gap
                let mut gap = f64::MAX;
                for q in draped.pos.iter().step_by(3) {
                    if let Some(t) = world.ray([q[0], q[1], q[2] + 0.05], [0.0, 0.0, -1.0], 0.3) {
                        gap = gap.min(q[2] - (q[2] + 0.05 - t));
                    }
                }
                if gap > 0.001 && gap < 0.02 {
                    for q in &mut draped.pos { q[2] -= gap - 0.001; }
                }
            }
            let name = format!("{nm_base}#0");
            let faces = pre.faces.clone();
            scene.cloth.push(serde_json::json!({
                "kind": "cloth", "target": name, "verts": pre.pos.iter().map(|p| serde_json::json!([p[0], p[1], p[2]])).collect::<Vec<_>>(), "faces": faces,
                "collide": collide, "kwargs": {"mass": 0.05, "tension_stiffness": 2, "distance_min": 0.005}, "end_frame": 50,
                "subsurf": 1, "pin": [], "build": null, "pose": Mat4::identity().0, "post": null, "z_from": null,
            }));
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&name, it.class, draped, mi, m.0);
            continue;
        }
        for (pi, part) in it.asset.parts.iter().enumerate() {
            let mut mesh: Mesh = part.mesh.clone();
            mesh.transform(&m);
            let mi = scene.material(part.mat.clone());
            scene.add_posed(&format!("{nm_base}#{pi}"), it.class, mesh, mi, m.0);
        }
    }
    stats
}
