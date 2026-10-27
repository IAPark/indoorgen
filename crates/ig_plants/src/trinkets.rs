//! NatureShelfTrinketsFactory: factory choice (FixedSeed(fs) np.random.choice), the MolluskFactory dispatcher (Snail/Shell base factory choice) and
//! create_asset for the mollusk kinds: base.spawn_asset(randint(1e7)) -> TRIANGULATE (BEAUTY) -> trimesh stable pose -> fit into the placeholder cube.
//! Kinds without an exact port (coral, rocks, pinecone, creatures) fall back to the approximate ig_decor::decor trinkets.
use super::mollusk::{snail_final, Maker};
use super::shell::{shell_asset, ShellKind};
use super::*;
use crate::mesh::Mesh;

pub fn handles(c: &str) -> bool {
    c == "NatureShelfTrinketsFactory"
}

/// python-replayed material of a mollusk factory: `<module under infinigen.assets.objects>.<Class>(fs).<attr>`
pub fn mollusk_mat(cls: &str, attr: &str, fs: u64) -> Material {
    ig_core::materials::spec_material("mollusk", serde_json::json!({"cls": format!("factory.{cls}"), "kwargs": {"attr": attr}, "seed": fs}), [0.8, 0.7, 0.6])
}

pub fn mollusk_handles(c: &str) -> bool {
    c == "MolluskFactory"
}

/// MolluskFactory itself: create_asset(face_size 0.003 + decorate_mollusk) of the randomly chosen base factory
pub struct Mollusk {
    fs: u64,
    base: Base,
}

impl Factory for Mollusk {
    fn asset(&self, inst: u64) -> Asset {
        let t = Trinkets { fs: self.fs, base: self.base, fallback: Box::new(NoFallback) };
        let m = t.base_mesh(inst).unwrap();
        let b = m.bounds();
        let mut a = Asset::default();
        a.mesh(m, &mollusk_mat("mollusk.generate.MolluskFactory", "material", self.fs), Some(b));
        a
    }
}

struct NoFallback;
impl Factory for NoFallback {
    fn asset(&self, _: u64) -> Asset {
        Asset::default()
    }
}

pub fn make_mollusk(fs: u64) -> Box<dyn Factory> {
    Box::new(Mollusk { fs, base: mollusk_base(fs) })
}

#[derive(Clone, Copy)]
enum Base {
    Shell(ShellKind),
    Snail(Maker),
    /// MolluskFactory: base factory chosen at construction
    MolluskShell(ShellKind),
    MolluskSnail(Maker),
    /// CarnivoreFactory (false) / HerbivoreFactory (true)
    Creature(bool),
    Pinecone,
    Other,
}

pub struct Trinkets {
    fs: u64,
    base: Base,
    fallback: Box<dyn Factory>,
}

/// MolluskFactory.__init__ choice: np.random.choice([SnailBase, ShellBase], p=[.5, .5]) then the base factory's own FixedSeed(fs) maker choice
fn mollusk_base(fs: u64) -> Base {
    if Rng::seeded(fs).choice_p(&[0.5, 0.5]) == 0 {
        // SnailBaseFactory.makers = [volute, nautilus, snail, conch]
        let m = [Maker::Volute, Maker::Nautilus, Maker::Snail, Maker::Conch][Rng::seeded(fs).choice_idx(4)];
        Base::MolluskSnail(m)
    } else {
        // ShellBaseFactory.makers = [scallop, clam, mussel]
        let k = [ShellKind::Scallop, ShellKind::Clam, ShellKind::Mussel][Rng::seeded(fs).choice_idx(3)];
        Base::MolluskShell(k)
    }
}

impl Trinkets {
    fn base_mesh(&self, i2: u64) -> Option<Mesh> {
        let mut r = inst_rng(self.fs, i2);
        Some(match self.base {
            Base::Shell(k) | Base::MolluskShell(k) => shell_asset(k, &mut r, 0.003),
            Base::Snail(m) | Base::MolluskSnail(m) => snail_final(m, &mut r),
            Base::Pinecone => {
                let g = super::monocot::Growth::new(super::monocot::Kind::Pinecone, self.fs);
                let lm = g.create_asset(&mut r, 0.003);
                let mut o = Mesh::new();
                o.pos = lm.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
                o.faces = lm.f.clone();
                o
            }
            Base::Creature(_) | Base::Other => return None,
        })
    }
}

fn triangulate_beauty(m: &Mesh) -> Vec<[u32; 3]> {
    let mut first: Vec<[u32; 3]> = vec![];
    let mut extra: Vec<[u32; 3]> = vec![];
    for f in &m.faces {
        match f.len() {
            3 => first.push([f[0], f[1], f[2]]),
            4 => {
                let p = |k: usize| {
                    let q = m.pos[f[k] as usize];
                    [q[0] as f32, q[1] as f32, q[2] as f32]
                };
                if crate::polyfill::quad_split_24_beauty([p(0), p(1), p(2), p(3)]) {
                    first.push([f[0], f[2], f[3]]);
                    extra.push([f[0], f[1], f[2]]);
                } else {
                    first.push([f[1], f[3], f[0]]);
                    extra.push([f[1], f[2], f[3]]);
                }
            }
            n => {
                for k in 1..n - 1 {
                    first.push([f[0], f[k], f[k + 1]]);
                }
            }
        }
    }
    first.extend(extra);
    first
}

impl Factory for Trinkets {
    fn placeholder(&self, i: u64) -> Option<Asset> {
        self.fallback.placeholder(i)
    }

    fn asset(&self, i: u64) -> Asset {
        if matches!(self.base, Base::Other) {
            return self.fallback.asset(i);
        }
        let mut r = inst_rng(self.fs, i);
        let size = inst_rng(self.fs, i).uniform(0.1, 0.15);
        let i2 = r.randint(0, 10_000_000) as u64;
        if let Base::Creature(herb) = self.base {
            // create_asset of Carnivore/Herbivore (hair off): the asset keeps its pose, only the fit into the placeholder cube applies
            let mut rc = inst_rng(self.fs, i2);
            let cm = crate::creature_asm::build_creature(herb, &mut rc);
            let pos = super::nature::finish_trinket_ex(cm.pos.clone(), None, (size as f32) as f64);
            let mut o = Mesh::new();
            o.pos = pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
            o.faces = cm.faces.clone();
            let b = o.bounds();
            let mut a = Asset::default();
            // creatures keep their material in `body_material` (no `.material` attribute)
            a.mesh(o, &mollusk_mat("elements.nature_shelf_trinkets.generate.NatureShelfTrinketsFactory", "base_factory.body_material", self.fs), Some(b));
            return a;
        }
        let t0 = web_time::Instant::now();
        let m = self.base_mesh(i2).unwrap();
        if std::env::var("TRDBG").is_ok() { eprintln!("i2 {} base mesh {} verts {:?}", i2, m.pos.len(), t0.elapsed()); }
        let tris = triangulate_beauty(&m);
        let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let tri_idx: Vec<[usize; 3]> = tris.iter().map(|t| [t[0] as usize, t[1] as usize, t[2] as usize]).collect();
        if std::env::var("TRDBG").is_ok() { eprintln!("tri {:?}", t0.elapsed()); }
        let pos = super::nature::finish_trinket(pos, &tri_idx, (size as f32) as f64);
        let mut o = Mesh::new();
        o.pos = pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        o.faces = tris.iter().map(|t| t.to_vec()).collect();
        let b = o.bounds();
        let mut a = Asset::default();
        a.mesh(o, &mollusk_mat("elements.nature_shelf_trinkets.generate.NatureShelfTrinketsFactory", "base_factory.material", self.fs), Some(b));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    if !handles(c) {
        return None;
    }
    let probs = [1.0, 1.0, 1.0, 1.0, 3.0, 2.0, 3.0, 2.0, 2.0, 2.0, 2.0, 5.0, 5.0];
    let tot: f64 = probs.iter().sum();
    let kind = Rng::seeded(fs).choice_p(&probs.map(|x| x / tot));
    let base = match kind {
        4 => mollusk_base(fs),
        5 => Base::Snail(Maker::Auger),
        6 => Base::Shell(ShellKind::Clam),
        7 => Base::Snail(Maker::Conch),
        8 => Base::Shell(ShellKind::Mussel),
        9 => Base::Shell(ShellKind::Scallop),
        10 => Base::Snail(Maker::Volute),
        3 => Base::Pinecone,
        11 => Base::Creature(false),
        12 => Base::Creature(true),
        _ => Base::Other,
    };
    let fallback = ig_decor::decor::make(c, rng, fs)?;
    Some(Box::new(Trinkets { fs, base, fallback }))
}
