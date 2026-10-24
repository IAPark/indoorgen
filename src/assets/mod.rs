//! Procedural asset factories (ports of infinigen/assets/objects/*): parameter sampling + simplified geometry.
use crate::furn::geom::*;
use crate::furn::solver::{Assets, PlaceholderKind};
use crate::int_hash;
use crate::mesh::*;
use crate::rng::Rng;

pub mod bathroom;
pub mod geomkit;
pub mod gn;
pub mod can;
pub mod bevel;
pub mod bowl;
pub mod bridge;
pub mod jar;
pub mod lid;
pub mod cup;
pub mod vase;
pub mod blops;
pub mod wineglass;
pub mod plate;
pub mod bottle;
pub mod cutlery;
pub mod foodbox;
pub mod subsurf;
pub mod tables;
pub mod tableparts;
pub mod lamps;
pub mod tableware;
pub mod stools;
pub mod decor;
pub mod appliances;
pub mod cookware;
pub mod cabnodes;
pub mod polyfill;
pub mod cabdoor;
pub mod aquarium;
pub mod fontdata;
pub mod gweld;
pub mod hwexact;
pub mod bathfree;
pub mod pangeom;
pub mod ksink;
pub mod books;
pub mod textdraw;
pub mod matdraw;
pub mod walldecor;
pub mod plants;
pub mod proxy;
pub mod seating;
pub mod ctor_draws;
pub mod cloth;
pub mod pydump;
pub mod clothes;
pub mod curvekit;
pub mod officechair;
pub mod sofa;
pub mod doors;
pub mod doorhandles;
pub mod bevel3;
pub mod towel;
pub mod chair;
pub mod gnhelpers;
pub mod bpyops;
pub mod bevelfull;
pub mod spiderplant;
pub mod snakeplant;
pub mod succulent;
pub mod gnedit;
pub mod balloon;
pub mod chopsticks;
pub mod gnasset;
pub mod gnports;
pub mod bevelgn;
pub mod solid;
pub mod windows;
pub mod shelves;

#[derive(Clone)]
pub struct Part {
    pub mesh: Mesh,
    pub mat: Material,
    /// axis aligned collision box (local space)
    pub col: Option<(V3, V3)>,
}

#[derive(Clone, Default)]
pub struct Asset {
    pub parts: Vec<Part>,
    /// top faces of these boxes carry the SupportSurface tag
    pub supports: Vec<(V3, V3)>,
}

impl Asset {
    pub fn cuboid(&mut self, lo: V3, hi: V3, mat: &Material) {
        self.parts.push(Part { mesh: Mesh::cuboid(lo, hi), mat: mat.clone(), col: Some((lo, hi)) });
    }
    /// board whose top face is a support surface
    pub fn board(&mut self, lo: V3, hi: V3, mat: &Material) {
        self.cuboid(lo, hi, mat);
        self.supports.push((lo, hi));
    }
    pub fn cylinder(&mut self, cx: f64, cy: f64, z0: f64, z1: f64, r: f64, mat: &Material) {
        self.parts.push(Part { mesh: Mesh::cylinder(cx, cy, z0, z1, r, 20), mat: mat.clone(), col: Some(([cx - r, cy - r, z0], [cx + r, cy + r, z1])) });
    }
    pub fn frustum(&mut self, cx: f64, cy: f64, z0: f64, z1: f64, r0: f64, r1: f64, mat: &Material) {
        let r = r0.max(r1);
        self.parts.push(Part { mesh: Mesh::frustum(cx, cy, z0, z1, r0, r1, 20), mat: mat.clone(), col: Some(([cx - r, cy - r, z0], [cx + r, cy + r, z1])) });
    }
    pub fn mesh(&mut self, mesh: Mesh, mat: &Material, col: Option<(V3, V3)>) {
        self.parts.push(Part { mesh, mat: mat.clone(), col });
    }
    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &self.parts {
            let (a, b) = p.mesh.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        (lo, hi)
    }
    pub fn transform(&mut self, m: &Mat4) {
        for p in self.parts.iter_mut() {
            p.mesh.transform(m);
            if let Some((lo, hi)) = p.col {
                let a = m.apply(lo);
                let b = m.apply(hi);
                p.col = Some(([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]));
            }
        }
        for s in self.supports.iter_mut() {
            let a = m.apply(s.0);
            let b = m.apply(s.1);
            *s = ([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]);
        }
    }

    /// geometry for the solver when the real asset acts as placeholder: board faces + canonical + support tags, boxes as proxies
    pub fn solver_geom(&self) -> ObjGeom {
        let (lo, hi) = self.bounds();
        let mut tris: Vec<Tri> = vec![];
        let mut proxies = vec![];
        for p in &self.parts {
            if let Some((a, b)) = p.col {
                tris.extend(box_tris(a, b, 0));
                proxies.push((Proxy::Box { center: [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.], half: [(b[0] - a[0]) / 2., (b[1] - a[1]) / 2., (b[2] - a[2]) / 2.] }, u32::MAX));
            }
        }
        canonical_tag(&mut tris, lo, hi);
        // support surfaces: upward triangles of support boards
        for (a, b) in &self.supports {
            for mut t in box_tris(*a, *b, 0) {
                let u = sub(t.v[1], t.v[0]);
                let v = sub(t.v[2], t.v[0]);
                if cross(u, v)[2] > 1e-12 {
                    t.tags = SUPPORT;
                    tris.push(t);
                }
            }
        }
        ObjGeom::from_tris(tris, proxies)
    }

    /// python's box placeholder layout (8 verts, 12 tris) for the bounds of this asset: see ObjGeom::cuboid
    pub fn bbox_exact(&self) -> cabnodes::ExactMesh {
        let (lo, hi) = self.bounds();
        cabnodes::box_exact(lo, hi)
    }

    pub fn bbox_geom(&self) -> ObjGeom {
        let (lo, hi) = self.bounds();
        ObjGeom::cuboid(lo, hi, 0)
    }
}

pub trait Factory {
    /// create_asset (final, detailed asset)
    fn asset(&self, inst_seed: u64) -> Asset;
    /// create_placeholder override, if the class defines one
    fn placeholder(&self, _inst_seed: u64) -> Option<Asset> {
        None
    }
    /// bounds of the final asset when the full mesh is not ported (used for bbox-of-asset placeholders)
    fn bounds_hint(&self, _inst_seed: u64) -> Option<(V3, V3)> {
        None
    }
    /// vertex-order-exact python placeholder mesh when it is not a plain box (e.g. KitchenSpace: two joined tagged boxes)
    fn placeholder_exact(&self, _inst_seed: u64) -> Option<cabnodes::ExactMesh> {
        None
    }
    /// vertex-order-exact python mesh of spawn_asset(inst) (float32 values), when the class is ported at topology level
    fn exact(&self, _inst_seed: u64) -> Option<cabnodes::ExactMesh> {
        None
    }
}

thread_local! {
    static UNSEEDED: std::cell::RefCell<Option<Vec<f64>>> = std::cell::RefCell::new(None);
}
/// values the python factory constructor drew from the global stream (outside its FixedSeed) for the factory about to be built
pub fn set_unseeded(v: Option<Vec<f64>>) {
    UNSEEDED.with(|u| *u.borrow_mut() = v);
}
pub fn unseeded() -> Option<Vec<f64>> {
    UNSEEDED.with(|u| u.borrow().clone())
}

pub fn make_factory(class: &str, factory_seed: u64) -> Option<Box<dyn Factory>> {
    let mut rng = Rng::seeded(factory_seed);
    match class {
        c if shelves::handles(c) => shelves::make(c, &mut rng, factory_seed),
        c if succulent::handles(c) => succulent::make(c, &mut rng, factory_seed),
        c if balloon::handles(c) => balloon::make(c, &mut rng, factory_seed),
        c if spiderplant::handles(c) => spiderplant::make(c, &mut rng, factory_seed),
        c if snakeplant::handles(c) => snakeplant::make(c, &mut rng, factory_seed),
        c if chopsticks::handles(c) => chopsticks::make(c, &mut rng, factory_seed),
        c if gnports::handles(c) => gnports::make(c, &mut rng, factory_seed),
        c if bevelgn::handles(c) => bevelgn::make(c, &mut rng, factory_seed),
        c if aquarium::handles(c) => aquarium::make(c, &mut rng, factory_seed),
        c if ksink::handles(c) => ksink::make(c, &mut rng, factory_seed),
        c if bathroom::handles(c) => bathroom::make(c, &mut rng, factory_seed),
        c if walldecor::handles(c) => walldecor::make(c, &mut rng),
        c if books::handles(c) => books::make(c, &mut rng, factory_seed),
        c if cookware::handles(c) => cookware::make(c, &mut rng, factory_seed),
        c if appliances::handles(c) => appliances::make(c, &mut rng, factory_seed),
        c if vase::handles(c) => vase::make(c, &mut rng, factory_seed),
        c if decor::handles(c) => decor::make(c, &mut rng, factory_seed),
        c if sofa::handles(c) => sofa::make(c, &mut rng),
        c if officechair::handles(c) => officechair::make(c, &mut rng, factory_seed),
        c if stools::handles(c) => stools::make(c, &mut rng),
        c if foodbox::handles(c) => foodbox::make(c, &mut rng, factory_seed),
        c if cutlery::handles(c) => cutlery::make(c, &mut rng, factory_seed),
        c if bottle::handles(c) => bottle::make(c, &mut rng, factory_seed),
        c if lid::handles(c) => lid::make(c, &mut rng, factory_seed),
        c if cup::handles(c) => cup::make(c, &mut rng, factory_seed),
        c if jar::handles(c) => jar::make(c, &mut rng, factory_seed),
        c if bowl::fruit_handles(c) => bowl::fruit_make(factory_seed),
        c if bowl::handles(c) => bowl::make(c, &mut rng, factory_seed),
        c if plate::handles(c) => plate::make(c, &mut rng, factory_seed),
        c if wineglass::handles(c) => wineglass::make(c, &mut rng, factory_seed),
        c if can::handles(c) => can::make(c, &mut rng, factory_seed),
        c if tableware::handles(c) => tableware::make(c, &mut rng, factory_seed),
        c if lamps::handles(c) => lamps::make(c, &mut rng, factory_seed),
        c if tables::handles(c) => tables::make(c, &mut rng),
        c if seating::handles(c) => seating::make(c, &mut rng, factory_seed),
        c if plants::handles(c) => plants::make(c, &mut rng, factory_seed),
        _ => None,
    }
}

/// bounding box placeholder of the final asset (uses the float32 exact mesh when the class is ported at topology level)
pub fn bbox_of_asset(f: &dyn Factory, inst: u64) -> ObjGeom {
    if let Some((lo, hi)) = f.bounds_hint(inst) {
        return ObjGeom::cuboid(lo, hi, 0);
    }
    if let Some(e) = f.exact(inst) {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &e.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        return ObjGeom::cuboid(lo, hi, 0);
    }
    f.asset(inst).bbox_geom()
}

pub fn inst_rng(factory_seed: u64, inst_seed: u64) -> Rng {
    Rng::seeded(int_hash(&[&factory_seed.to_string(), &inst_seed.to_string()]))
}

pub fn mat(name: &str, rgb: [f32; 3], rough: f32) -> Material {
    Material::diffuse(name, rgb, rough)
}

/// Assets backend: real factories where ported, empirical bbox table otherwise.
pub struct RealAssets {
    pub fallback: crate::furn::assets::TableAssets,
}

impl Assets for RealAssets {
    fn placeholder(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom> {
        if let Some(f) = make_factory(class, factory_seed) {
            return Some(match kind {
                PlaceholderKind::Real => match f.placeholder(inst_seed) {
                    Some(p) => p.bbox_geom(),
                    // python overrides create_placeholder for these classes (a custom bbox); until the formula is ported use the sampled box
                    None => match self.fallback.placeholder(class, factory_seed, inst_seed, kind) {
                        Some(g) => g,
                        None => f.asset(inst_seed).solver_geom(),
                    },
                },
                PlaceholderKind::AssetAsPlaceholder => f.asset(inst_seed).solver_geom(),
                PlaceholderKind::BboxOfPlaceholder => match f.placeholder(inst_seed) {
                    Some(p) => p.bbox_geom(),
                    None => match self.fallback.placeholder(class, factory_seed, inst_seed, kind) {
                        Some(g) => g,
                        None => bbox_of_asset(&*f, inst_seed),
                    },
                },
                PlaceholderKind::BboxOfAsset => bbox_of_asset(&*f, inst_seed),
            });
        }
        self.fallback.placeholder(class, factory_seed, inst_seed, kind)
    }
    fn solver_asset(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Option<ObjGeom> {
        make_factory(class, factory_seed).map(|f| f.asset(inst_seed).solver_geom())
    }
}

impl RealAssets {
    /// final visual asset for a generated object (real port if available, otherwise a recipe in the sampled bbox)
    pub fn visual(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Asset {
        if let Some(f) = make_factory(class, factory_seed) {
            return f.asset(inst_seed);
        }
        let (lo, hi) = match self.fallback.sample(class, factory_seed, inst_seed).and_then(|s| s.asset.or(s.ph)) {
            Some(b) => b,
            None => ([-0.15, -0.15, 0.0], [0.15, 0.15, 0.3]),
        };
        let mut rng = inst_rng(factory_seed, inst_seed);
        proxy::proxy_asset(class, lo, hi, &mut rng)
    }
}
