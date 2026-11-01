//! The piece registry (ig_kit::piece): each class as a Piece. Classes not yet ported natively run through `FactoryPiece`, a wrapper
//! around their old Factory that derives everything from the piece's seed (instance seed, constructor draws) and whose collision is the
//! boxes of a layout build (one per part: soft for drapes and foliage, hard otherwise). Nothing is scaled: a class that builds to a size
//! (its size model is 'exact') takes one from its size model and resizes on request; any other keeps the size its factory gives it.
use crate::furn::sizes;
use ig_kit::piece::{Piece, Volume, Collide, Build};
use ig_kit::api::Asset;
use ig_core::rng::Rng;

/// classes kept on the solver's old path for now (counter runs and kitchen cabinets: real per-cabinet placeholders, kitchen plans; a
/// large plant: 1.5 s a build until its collision is ported)
const NOT_YET: &[&str] = &["KitchenSpaceFactory", "KitchenCabinetFactory", "KitchenIslandFactory", "LargePlantContainerFactory"];

/// install the registry (once per process: gen2, the wasm entry, tools)
pub fn install_pieces() {
    ig_kit::piece::install(make_piece);
}

/// classes the solver takes as wrapped pieces while the port runs: the ones already laid out by their real parts; the rest stay on the
/// old path (a box and a rebuilt factory) until their collision is ported -- the wrapper's layout build per proposal is slow for small
/// items. colcheck covers every class either way (`any_piece`).
const WRAPPED: &[&str] = &["SofaFactory", "ArmChairFactory", "ChairFactory", "BarChairFactory", "OfficeChairFactory", "TableDiningFactory", "CoffeeTableFactory",
    "SideTableFactory", "TableCocktailFactory", "SimpleDeskFactory", "BedFactory", "WardrobeFactory", "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory",
    "TVStandFactory", "FridgeFactory", "ShowerFactory", "FloorLampFactory", "BookStackFactory"];

thread_local! {
    /// classes found not native (a class is ported whole: every factory struct behind it writes its volumes)
    static NOT_NATIVE: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

pub fn make_piece(class: &str, seed: u64) -> Option<Box<dyn Piece>> {
    if NOT_YET.contains(&class) { return None }
    let wrapped = std::env::var_os("PIECES_WRAPPED").is_some() && WRAPPED.contains(&class);
    // (every proposal of every class comes through here: a class known not to be native goes straight back)
    if !wrapped && NOT_NATIVE.with(|n| n.borrow().contains(class)) { return None }
    crate::make_factory(class, seed)?;
    // a native piece -- its factory writes its volumes -- for any class; PIECES_WRAPPED=1: also a wrapped one for the classes above
    // (asked first, without the wrapper's layout build: every proposal of every class comes through here)
    let mut p = FactoryPiece::new(class, seed, None);
    if p.try_native() || wrapped {
        p.volumes();
        Some(Box::new(p))
    } else {
        NOT_NATIVE.with(|n| n.borrow_mut().insert(class.to_string()));
        None
    }
}

/// any class with a factory as a piece (what colcheck checks): native when its factory writes its volumes, else wrapped
pub fn any_piece(class: &str, seed: u64) -> Option<Box<dyn Piece>> {
    if NOT_YET.contains(&class) { return None }
    crate::make_factory(class, seed)?;
    let mut p = FactoryPiece::new(class, seed, None);
    p.volumes();
    Some(Box::new(p))
}

pub struct FactoryPiece {
    class: String,
    seed: u64,
    inst: u64,
    draws: Option<Vec<f64>>,
    /// the size it is built to ([width, depth, height]: a class that builds to a size); None: the size its factory gives it
    size: Option<[f64; 3]>,
    /// its factory writes its volumes (else: the boxes of a layout build)
    native: bool,
    /// what the collision decided
    col: Option<Vec<Volume>>,
}

fn hash(tag: &str, seed: u64) -> u64 { crate::int_hash(&[tag, &seed.to_string()]) }

/// the class builds to a size it is given (its size model is 'exact')
fn builds_to_size(class: &str) -> bool { sizes::model(class).map_or(false, |m| m.exact) }

impl FactoryPiece {
    fn new(class: &str, seed: u64, size: Option<[f64; 3]>) -> FactoryPiece {
        let draws = ig_kit::ctor_draws::ctor_draws(class, &mut Rng::seeded(hash("piece_draws", seed)));
        let size = if builds_to_size(class) { size.or_else(|| sizes::model(class).map(|m| sizes::sample(m, &mut Rng::seeded(hash("piece_size", seed))))) } else { None };
        FactoryPiece { class: class.into(), seed, inst: hash("piece_inst", seed) % 10_000_000, draws, size, native: false, col: None }
    }

    /// run `f` with the factory's hints set as this piece's state says (the old factories read them), restored after
    fn with_hints<T>(&self, f: impl FnOnce() -> T) -> T {
        let (s0, u0) = (ig_kit::api::size_hint(), ig_kit::ctor_draws::unseeded());
        ig_kit::api::set_size_hint(self.size);
        ig_kit::ctor_draws::set_unseeded(self.draws.clone());
        let r = f();
        ig_kit::api::set_size_hint(s0);
        ig_kit::ctor_draws::set_unseeded(u0);
        r
    }

    /// the boxes of a layout build: one per part (soft for drapes and foliage), the extra boxes, the supports
    fn layout_build_volumes(&self) -> Vec<Volume> {
        let a = self.with_hints(|| {
            let f = crate::make_factory(&self.class, self.seed).expect("factory");
            // (the coarse build only where it is known to hold and to pay: subdivided cushions and bodies, bevelled chair frames,
            // a bed's bedding drape; a tub's and a basin's builds pair coarse and subdivided meshes, the rest build in a millisecond)
            ig_core::envx::set_layout_build(matches!(self.class.as_str(), "SofaFactory" | "ArmChairFactory" | "ChairFactory" | "OvenFactory" | "DishwasherFactory" | "BedFactory"));
            let a = f.asset(self.inst);
            ig_core::envx::set_layout_build(false);
            a
        });
        let mut vols: Vec<Volume> = vec![];
        for (i, p) in a.parts.iter().enumerate() {
            if p.mesh.pos.is_empty() { continue }
            let (lo, hi) = p.col.unwrap_or_else(|| p.mesh.bounds());
            vols.push(if a.is_soft(i) { Volume::soft(lo, hi) } else { Volume::hard(lo, hi) });
        }
        vols.extend(a.cols.iter().map(|(lo, hi)| Volume::hard(*lo, *hi)));
        vols.extend(a.supports.iter().map(|(lo, hi)| Volume::support(*lo, *hi)));
        vols
    }

    /// the factory's own volumes, if it writes them (kept as the collision)
    fn try_native(&mut self) -> bool {
        if self.col.is_none() {
            if let Some(v) = self.with_hints(|| crate::make_factory(&self.class, self.seed).and_then(|f| f.volumes(self.inst))) {
                self.native = true;
                self.col = Some(v);
            }
        }
        self.native
    }

    /// the collision: the factory's own volumes from its parameters, else the boxes of a layout build
    fn volumes(&mut self) -> &Vec<Volume> {
        if self.col.is_none() {
            let native = self.with_hints(|| crate::make_factory(&self.class, self.seed).and_then(|f| f.volumes(self.inst)));
            self.native = native.is_some();
            self.col = Some(match native { Some(v) => v, None => self.layout_build_volumes() });
        }
        self.col.as_ref().unwrap()
    }
}

impl Collide for FactoryPiece {
    fn collision(&mut self) -> Vec<Volume> { self.volumes().clone() }
}

impl Build for FactoryPiece {
    fn mesh(&mut self) -> Asset {
        self.with_hints(|| {
            let f = crate::make_factory(&self.class, self.seed).expect("factory");
            ig_kit::api::scene_asset(&*f, self.inst)
        })
    }
}

impl Piece for FactoryPiece {
    fn native(&self) -> bool { self.native }
    fn resize(&self, size: [f64; 3]) -> Option<Box<dyn Piece>> {
        if !builds_to_size(&self.class) { return None }
        let mut p = FactoryPiece::new(&self.class, self.seed, Some(size));
        p.volumes();
        Some(Box::new(p))
    }
}
