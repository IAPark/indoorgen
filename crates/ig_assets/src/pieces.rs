//! The piece registry (ig_kit::piece): each class as a Piece. Classes not yet ported natively run through `FactoryPiece`, a wrapper
//! around their old Factory that derives everything from the piece's seed (instance seed, constructor draws, a size from the class's
//! size model) and whose collision is the boxes of a layout build (one per part: soft for drapes and foliage, hard otherwise). A sized
//! class that cannot build to a size gets one transform, from its collision's rigid bounds, for both its collision and its mesh.
use crate::furn::sizes;
use ig_kit::piece::{rigid_bounds, Piece, Volume, Collide, Build, Shape};
use ig_kit::api::Asset;
use ig_core::mesh::{Mat4, V3};
use ig_core::rng::Rng;


/// classes kept on the solver's old path for now (counter runs and kitchen cabinets: real per-cabinet placeholders, kitchen plans; a
/// large plant: 1.5 s a build until its collision is ported)
const NOT_YET: &[&str] = &["KitchenSpaceFactory", "KitchenCabinetFactory", "KitchenIslandFactory", "LargePlantContainerFactory"];

/// install the registry (once per process: gen2, the wasm entry, tools)
pub fn install_pieces() {
    ig_kit::piece::install(make_piece);
}

pub fn make_piece(class: &str, seed: u64) -> Option<Box<dyn Piece>> {
    if NOT_YET.contains(&class) { return None }
    crate::make_factory(class, seed)?;
    Some(Box::new(FactoryPiece::new(class, seed, None)))
}

pub struct FactoryPiece {
    class: String,
    seed: u64,
    inst: u64,
    draws: Option<Vec<f64>>,
    /// the size asked of the piece ([width, depth, height]); None: its own (sampled from the class size model when it has one)
    size: Option<[f64; 3]>,
    exact: bool,
    /// what the collision decided: the volumes (factory frame, sized) and the sizing transform the mesh gets too
    col: Option<(Vec<Volume>, Option<(V3, V3)>)>,
}

fn hash(tag: &str, seed: u64) -> u64 { crate::int_hash(&[tag, &seed.to_string()]) }

impl FactoryPiece {
    fn new(class: &str, seed: u64, size: Option<[f64; 3]>) -> FactoryPiece {
        let draws = ig_kit::ctor_draws::ctor_draws(class, &mut Rng::seeded(hash("piece_draws", seed)));
        let m = sizes::model(class);
        let size = size.or_else(|| m.map(|m| sizes::sample(m, &mut Rng::seeded(hash("piece_size", seed)))));
        FactoryPiece { class: class.into(), seed, inst: hash("piece_inst", seed) % 10_000_000, draws, size, exact: m.map_or(false, |m| m.exact), col: None }
    }

    /// run `f` with the factory's hints set as this piece's state says (the old factories read them), restored after
    fn with_hints<T>(&self, f: impl FnOnce() -> T) -> T {
        let (s0, u0) = (ig_kit::api::size_hint(), ig_kit::ctor_draws::unseeded());
        ig_kit::api::set_size_hint(if self.exact { self.size } else { None });
        ig_kit::ctor_draws::set_unseeded(self.draws.clone());
        let r = f();
        ig_kit::api::set_size_hint(s0);
        ig_kit::ctor_draws::set_unseeded(u0);
        r
    }

    /// the volumes in the factory's frame and the scale-and-shift that sizes them (a sized class that does not build to size)
    fn volumes(&mut self) -> &(Vec<Volume>, Option<(V3, V3)>) {
        if self.col.is_none() {
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
            // the sizing transform: the rigid bounds fitted to the asked size, as scenegen fitted the final asset
            let fit = match (self.exact, self.size, sizes::model(&self.class)) {
                (false, Some(x), Some(m)) => {
                    let (lo, hi) = rigid_bounds(&vols);
                    let (tlo, thi) = sizes::target_box(lo, hi, x);
                    Some(sizes::fit_matrix(m.uniform, lo, hi, tlo, thi))
                }
                _ => None,
            };
            if let Some((s, o)) = fit { for v in vols.iter_mut() { scale_volume(v, s, o); } }
            self.col = Some((vols, fit));
        }
        self.col.as_ref().unwrap()
    }
}

fn scale_volume(v: &mut Volume, s: V3, o: V3) {
    let f = |p: V3| -> V3 { [0, 1, 2].map(|k| p[k] * s[k] + o[k]) };
    v.shape = match &v.shape {
        Shape::Box { lo, hi } => { let (a, b) = (f(*lo), f(*hi)); Shape::Box { lo: [0, 1, 2].map(|k| a[k].min(b[k])), hi: [0, 1, 2].map(|k| a[k].max(b[k])) } }
        Shape::Hull(p) => Shape::Hull(p.iter().map(|q| f(*q)).collect()),
    };
}

impl Collide for FactoryPiece {
    fn collision(&mut self) -> Vec<Volume> { self.volumes().0.clone() }
}

impl Build for FactoryPiece {
    fn mesh(&mut self) -> Asset {
        let fit = self.volumes().1;
        let mut a = self.with_hints(|| {
            let f = crate::make_factory(&self.class, self.seed).expect("factory");
            ig_kit::api::scene_asset(&*f, self.inst)
        });
        if let Some((s, o)) = fit {
            let mut mt = Mat4::scale(s);
            for k in 0..3 { mt.0[k][3] = o[k]; }
            a.transform(&mt);
            for c in a.cols.iter_mut() { let (p, q) = (mt.apply(c.0), mt.apply(c.1)); *c = ([0, 1, 2].map(|k| p[k].min(q[k])), [0, 1, 2].map(|k| p[k].max(q[k]))); }
        }
        a
    }
}

impl Piece for FactoryPiece {
    fn native(&self) -> bool { false }
    fn resize(&self, size: [f64; 3]) -> Option<Box<dyn Piece>> {
        sizes::model(&self.class)?;
        Some(Box::new(FactoryPiece::new(&self.class, self.seed, Some(size))))
    }
}
