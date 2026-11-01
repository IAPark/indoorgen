//! ig_assets: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub use ig_core::*;
pub use ig_exbool as exbool;
pub use ig_bm as bm;
pub use ig_geom::*;
pub use ig_kit::*;
pub use ig_gn as gn;
pub use ig_furn as furn;
pub use ig_rooms as rooms;
pub use ig_shared::*;
pub use ig_seat::*;
pub use ig_storage::*;
pub use ig_plants::*;
pub use ig_decor::*;
pub use ig_openings::*;
pub mod pieces;
pub use pieces::install_pieces;
// Procedural asset factories (ports of infinigen/assets/objects/*): parameter sampling + simplified geometry.
use crate::geom::*;
use crate::furn::solver::{Assets, PlaceholderKind};
use crate::int_hash;
use crate::mesh::*;
use crate::rng::Rng;
pub fn make_factory(class: &str, factory_seed: u64) -> Option<Box<dyn Factory>> {
    let mut rng = Rng::seeded(factory_seed);
    match class {
        "TapFactory" => Some(Box::new(taps::TapFactory { fseed: factory_seed })),
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
        c if fridge::handles(c) => fridge::make(c, &mut rng, factory_seed),
        c if wardrobe::handles(c) => wardrobe::make(c, &mut rng, factory_seed),
        c if shower::handles(c) => shower::make(c, &mut rng, factory_seed),
        c if vase::handles(c) => vase::make(c, &mut rng, factory_seed),
        "MolluskFactory" => Some(trinkets::make_mollusk(factory_seed)),
        c if trinkets::handles(c) => trinkets::make(c, &mut rng, factory_seed),
        c if decor::handles(c) => decor::make(c, &mut rng, factory_seed),
        c if sofa::handles(c) => sofa::make(c, &mut rng),
        c if officechair::handles(c) => officechair::make(c, &mut rng, factory_seed),
        "MattressFactory" => mattress::make(factory_seed),
        "PillowFactory" => pillow::make(factory_seed),
        "BedFrameFactory" => bed::make_frame(factory_seed),
        "BedFactory" if unseeded().is_some() && bed::make(factory_seed).is_some() => bed::make(factory_seed),
        c if chair::handles(c) => chair::make(c, &mut rng, factory_seed),
        c if towel::handles(c) && unseeded().is_some() => towel::make(c, &mut rng, factory_seed),
        c if stools::handles(c) => stools::make(c, &mut rng),
        c if foodbag::handles(c) => foodbag::make(c, &mut rng, factory_seed),
        c if foodbox::handles(c) => foodbox::make(c, &mut rng, factory_seed),
        c if cutlery::handles(c) => cutlery::make(c, &mut rng, factory_seed),
        c if bottle::handles(c) => bottle::make(c, &mut rng, factory_seed),
        c if lid::handles(c) => lid::make(c, &mut rng, factory_seed),
        c if cup::handles(c) => cup::make(c, &mut rng, factory_seed),
        c if mollusk::handles(c) => mollusk::make(c, &mut rng, factory_seed),
        c if jar::handles(c) => jar::make(c, &mut rng, factory_seed),
        c if bowl::fruit_handles(c) => bowl::fruit_make(factory_seed),
        c if bowl::handles(c) => bowl::make(c, &mut rng, factory_seed),
        c if plate::handles(c) => plate::make(c, &mut rng, factory_seed),
        c if wineglass::handles(c) => wineglass::make(c, &mut rng, factory_seed),
        c if can::handles(c) => can::make(c, &mut rng, factory_seed),
        c if tableware::handles(c) => tableware::make(c, &mut rng, factory_seed),
        c if ceiling_fan::handles(c) => ceiling_fan::make(c, factory_seed),
        c if lamps::handles(c) => lamps::make(c, &mut rng, factory_seed),
        c if tables::handles(c) => tables::make(c, &mut rng),
        c if seating::handles(c) => seating::make(c, &mut rng, factory_seed),
        c if monocot::handles(c) => monocot::make(c, &mut rng, factory_seed),
        c if shell::handles(c) && c != "MolluskFactory" => shell::make(c, &mut rng, factory_seed),
        c if mushroom::handles(c) => mushroom::make(c, &mut rng, factory_seed),
        c if fern::handles(c) => fern::make(c, &mut rng, factory_seed),
        c if cactus::handles(c) => cactus::make(c, &mut rng, factory_seed),
        c if plants::handles(c) => plants::make(c, &mut rng, factory_seed),
        _ => None,
    }
}
/// Assets backend: real factories where ported, empirical bbox table otherwise.
pub struct RealAssets {
    pub fallback: crate::furn::assets::TableAssets,
}

impl Assets for RealAssets {
    fn natural_size(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Option<[f64; 3]> {
        // the factory built without a size hint: its constructor draws give the size
        let saved = ig_kit::api::size_hint();
        ig_kit::api::set_size_hint(None);
        let r = make_factory(class, factory_seed).and_then(|f| f.natural_size(inst_seed));
        ig_kit::api::set_size_hint(saved);
        r
    }
    fn placeholder(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom> {
        let t = thread_cpu();
        let key = cache_key(class, factory_seed, inst_seed, &format!("{kind:?}"));
        if !memo_off() {
            if let Some(r) = memo_get(&PH_MEMO, &key) {
                return r;
            }
        }
        let r = self.placeholder_raw(class, factory_seed, inst_seed, kind).map(round_geom_f32);
        memo_put(&PH_MEMO, key, &r);
        if prof_on() {
            prof_add(format!("placeholder {kind:?} {class}"), thread_cpu() - t);
        }
        r
    }
    fn layout_geom(&self, class: &str, factory_seed: u64, inst_seed: u64, unseeded: Option<&[f64]>, size: Option<[f64; 3]>, fit: Option<((V3, V3), ig_geom::geom::LayoutFit)>, round: bool) -> Option<ObjGeom> {
        use ig_geom::geom::LayoutFit;
        let t = thread_cpu();
        // the hints of the final build (build_visual in scenegen), the caller's restored after
        let (s0, r0, u0) = (ig_kit::api::size_hint(), ig_kit::api::round_hint(), crate::unseeded());
        ig_kit::api::set_size_hint(size);
        ig_kit::api::set_round_hint(round);
        set_unseeded(unseeded.map(|u| u.to_vec()));
        let raw = self.layout_raw(class, factory_seed, inst_seed);
        ig_kit::api::set_size_hint(s0);
        ig_kit::api::set_round_hint(r0);
        set_unseeded(u0);
        let r = raw.map(|(boxes, sup, (lo, hi))| {
            // onto the solver's box as the final asset goes (scenegen build_visual): moved (built to size) or scaled and moved
            let (sc, off) = match fit {
                Some(((tlo, thi), LayoutFit::Exact)) => ([1.0; 3], [tlo[0] - lo[0], (tlo[1] + thi[1]) / 2.0 - (lo[1] + hi[1]) / 2.0, tlo[2] - lo[2]]),
                Some(((tlo, thi), LayoutFit::Scale { uniform })) => crate::furn::sizes::fit_matrix(uniform, lo, hi, tlo, thi),
                None => ([1.0; 3], [0.0; 3]),
            };
            let tf = |b: &(V3, V3)| -> (V3, V3) {
                let (p, q) = ([0, 1, 2].map(|k| b.0[k] * sc[k] + off[k]), [0, 1, 2].map(|k| b.1[k] * sc[k] + off[k]));
                ([0, 1, 2].map(|k| p[k].min(q[k])), [0, 1, 2].map(|k| p[k].max(q[k])))
            };
            let boxes: Vec<(V3, V3)> = boxes.iter().map(tf).collect();
            let sup: Vec<(V3, V3)> = sup.iter().map(tf).collect();
            // (open shapes keep their parts -- a seat under a table top, legs round a bed's frame; box-shaped storage is one box: the
            // solver's collision cost grows with the boxes, and a wardrobe's or a shelf unit's parts fill their box anyway)
            let open = matches!(class, "SofaFactory" | "ArmChairFactory" | "ChairFactory" | "BarChairFactory" | "OfficeChairFactory" | "TableDiningFactory" | "CoffeeTableFactory" | "SideTableFactory" | "TableCocktailFactory" | "SimpleDeskFactory" | "BedFactory" | "FloorLampFactory");
            round_geom_f32(ig_kit::api::layout_geom_of(&boxes, &sup, if open { 6 } else { 1 }))
        });
        if prof_on() { prof_add(format!("layout_geom {class}"), thread_cpu() - t); }
        r
    }
    fn solver_asset(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Option<ObjGeom> {
        let t = thread_cpu();
        let key = cache_key(class, factory_seed, inst_seed, "solver");
        if !memo_off() {
            if let Some(r) = memo_get(&SOLVER_MEMO, &key) {
                return r;
            }
        }
        let r = make_factory(class, factory_seed).map(|f| scene_asset(&*f, inst_seed).solver_geom());
        memo_put(&SOLVER_MEMO, key, &r);
        if prof_on() {
            prof_add(format!("solver_asset {class}"), thread_cpu() - t);
        }
        r
    }
}
// ---- memo caches: the solver, the final export and bbox placeholders build the same (class, fseed, inst) assets over and over.
// Factories are pure functions of (class, factory seed, instance seed, unseeded ctor draws), so results are cached by exactly that key.
type CacheKey = (String, u64, u64, String, Option<Vec<u64>>);
fn cache_key(class: &str, fs: u64, inst: u64, what: &str) -> CacheKey {
    let what = match ig_kit::api::size_hint() { Some(h) => format!("{what}|size {:?}", h.map(|x| x.to_bits())), None => what.to_string() };
    let what = if ig_kit::api::round_hint() { format!("{what}|round") } else { what };
    let what = if ig_kit::api::closed_hint() { format!("{what}|closed") } else { what };
    let what = match ig_kit::api::kitchen_join() { Some(j) => format!("{what}|join {:?}", j.map(|x| x.to_bits())), None => what };
    (class.to_string(), fs, inst, what, unseeded().map(|v| v.iter().map(|x| x.to_bits()).collect()))
}
type Memo<T> = std::sync::Mutex<Option<std::collections::HashMap<CacheKey, T>>>;
// (nothing a layout build makes -- coarse meshes -- goes into or comes out of the caches of real builds)
fn memo_get<T: Clone>(m: &Memo<T>, k: &CacheKey) -> Option<T> {
    if ig_core::envx::layout_build() { return None }
    m.lock().unwrap().as_ref().and_then(|h| h.get(k).cloned())
}
fn memo_put<T: Clone>(m: &Memo<T>, k: CacheKey, v: &T) {
    if ig_core::envx::layout_build() { return }
    m.lock().unwrap().get_or_insert_with(Default::default).insert(k, v.clone());
}
static PH_MEMO: Memo<Option<ObjGeom>> = std::sync::Mutex::new(None);
static SOLVER_MEMO: Memo<Option<ObjGeom>> = std::sync::Mutex::new(None);
static VISUAL_MEMO: Memo<Asset> = std::sync::Mutex::new(None);
static LAYOUT_MEMO: Memo<Option<(Vec<(V3, V3)>, Vec<(V3, V3)>, (V3, V3))>> = std::sync::Mutex::new(None);
fn memo_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| ig_core::env_on!("NOMEMO"))
}
impl RealAssets {
    /// the raw layout build of a piece (its part boxes, support boxes, bounds), under the hints already set (size, round, draws);
    /// cached per build. The final asset is fitted onto its box with these bounds too (scenegen), so it lands where its parts were laid out.
    pub fn layout_raw(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Option<(Vec<(V3, V3)>, Vec<(V3, V3)>, (V3, V3))> {
        let t = thread_cpu();
        // the raw build's boxes, cached per build (class, seeds, draws, the size asked of a build-to-size class): the fit onto the
        // solver's box is a scale and a shift, applied to the boxes below -- a toilet tried at 30 sizes is built once
        let key = cache_key(class, factory_seed, inst_seed, "layout");
        match if memo_off() { None } else { memo_get(&LAYOUT_MEMO, &key) } {
            Some(r) => r,
            None => {
                let r = make_factory(class, factory_seed).map(|f| {
                    // (the coarse build only where it is known to hold and to pay: subdivided cushions and bodies, bevelled chair
                    // frames, a bed's bedding drape; a tub's and a basin's builds pair their coarse and subdivided meshes, the rest build in a millisecond)
                    ig_core::envx::set_layout_build(matches!(class, "SofaFactory" | "ArmChairFactory" | "ChairFactory" | "OvenFactory" | "DishwasherFactory" | "BedFactory"));
                    // (the approximate parts: scene_asset's exact python mesh only re-splits them, at 0.6 s for an oven)
                    let a = f.asset(inst_seed);
                    ig_core::envx::set_layout_build(false);
                    let (boxes, sup) = a.layout_boxes();
                    (boxes, sup, a.bounds())
                });
                if ig_core::env_on!("LAYOUTDBG") { eprintln!("LAYOUTDBG build {class} {factory_seed} {inst_seed} {:.1} ms boxes {} bounds {:?}", (thread_cpu() - t) * 1e3, r.as_ref().map_or(0, |x| x.0.len()), r.as_ref().map(|x| (x.2.0.map(|v| (v * 1000.0).round() / 1000.0), x.2.1.map(|v| (v * 1000.0).round() / 1000.0)))); }
                memo_put(&LAYOUT_MEMO, key, &r);
                r
            }
        }
    }
    pub fn placeholder_raw(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom> {
        if let Some(f) = make_factory(class, factory_seed) {
            return Some(match kind {
                PlaceholderKind::Real if f.placeholder_exact(inst_seed).is_some() => exact_geom(&f.placeholder_exact(inst_seed).unwrap()),
                PlaceholderKind::Real => match f.placeholder(inst_seed) {
                    // Mirror / WallArt / PlantContainer placeholders are not new_bbox boxes (plain float32 bounds)
                    Some(p) if matches!(class, "MirrorFactory" | "WallArtFactory" | "PlantContainerFactory" | "LargePlantContainerFactory") => p.bbox_geom_plain(),
                    Some(p) => p.bbox_geom(),
                    // python overrides create_placeholder for these classes (a custom bbox); until the formula is ported use the sampled box
                    None => match self.fallback.placeholder(class, factory_seed, inst_seed, kind) {
                        Some(g) => g,
                        None => f.asset(inst_seed).solver_geom(),
                    },
                },
                PlaceholderKind::AssetAsPlaceholder => f.asset(inst_seed).solver_geom(),
                PlaceholderKind::BboxOfPlaceholder => match f.placeholder(inst_seed) {
                    Some(p) => p.bbox_geom_corners(),
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
    /// final visual asset for a generated object (real port if available, otherwise a recipe in the sampled bbox)
    pub fn visual(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Asset {
        let t = thread_cpu();
        let key = cache_key(class, factory_seed, inst_seed, "visual");
        if !memo_off() {
            if let Some(r) = memo_get(&VISUAL_MEMO, &key) {
                return r;
            }
        }
        let r = self.visual_inner(class, factory_seed, inst_seed);
        memo_put(&VISUAL_MEMO, key, &r);
        if prof_on() {
            prof_add(format!("visual {class}"), thread_cpu() - t);
            if ig_core::env_on!("VISDBG") { eprintln!("VISDBG {class} {factory_seed} {inst_seed} {:.3}s faces {}", thread_cpu() - t, r.parts.iter().map(|p| p.mesh.faces.len()).sum::<usize>()); }
        }
        r
    }
    fn visual_inner(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Asset {
        if let Some(f) = make_factory(class, factory_seed) {
            return scene_asset(&*f, inst_seed);
        }
        let (lo, hi) = match self.fallback.sample(class, factory_seed, inst_seed).and_then(|s| s.asset.or(s.ph)) {
            Some(b) => b,
            None => ([-0.15, -0.15, 0.0], [0.15, 0.15, 0.3]),
        };
        let mut rng = inst_rng(factory_seed, inst_seed);
        proxy::proxy_asset(class, lo, hi, &mut rng)
    }
}

