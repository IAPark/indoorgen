//! Moves, proposals, simulated annealing and the greedy stage driver (example_solver/*).
use super::dof::*;
use super::domain::*;
use super::eval::*;
use super::geom::*;
use super::program::*;
use super::relations::*;
use super::state::*;
use super::tags::*;
use crate::mesh::{dot, V3};
use crate::rng::Rng;
use indexmap::IndexMap;
use std::collections::BTreeSet;
use crate::dhash::{HashMap};
use std::sync::Arc;

/// usage_lookup
pub struct Usage {
    pub by_class: HashMap<String, TagSet>,
    pub by_tag: HashMap<Tag, BTreeSet<String>>,
}

impl Usage {
    pub fn from_json(v: &serde_json::Value) -> Usage {
        let mut by_class: HashMap<String, TagSet> = HashMap::default();
        let mut by_tag: HashMap<Tag, BTreeSet<String>> = HashMap::default();
        for (tag, classes) in v.as_object().unwrap() {
            let t = Tag::sem(tag);
            by_tag.entry(t).or_default();
            for c in classes.as_array().unwrap() {
                let c = c.as_str().unwrap().to_string();
                by_class.entry(c.clone()).or_default().insert(t);
                by_tag.get_mut(&t).unwrap().insert(c);
            }
        }
        Usage { by_class, by_tag }
    }
    pub fn usages_of(&self, class: &str) -> TagSet {
        let mut s = self.by_class.get(class).cloned().unwrap_or_default();
        s.insert(Tag::gen(class));
        s
    }
    pub fn has_usage(&self, class: &str, tag: &str) -> bool {
        self.by_class.get(class).map_or(false, |s| s.contains(Tag::sem(tag)))
    }
    pub fn all_factories(&self) -> BTreeSet<String> {
        self.by_class.keys().cloned().collect()
    }
    pub fn factories_for_usage(&self, t: Tag) -> BTreeSet<String> {
        self.by_tag.get(&t).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlaceholderKind {
    Real,
    AssetAsPlaceholder,
    BboxOfPlaceholder,
    BboxOfAsset,
}

/// Source of placeholder geometry (the asset factories).
pub trait Assets {
    fn placeholder(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom>;
    /// real asset geometry for solver purposes (shelves populated before small objects are placed)
    fn solver_asset(&self, _class: &str, _factory_seed: u64, _inst_seed: u64) -> Option<ObjGeom> {
        None
    }
}

pub struct Ctx<'a> {
    pub prog: &'a Program,
    pub usage: &'a Usage,
    pub assets: &'a dyn Assets,
    pub bounds: Vec<Bound>,
    pub singletons: HashMap<String, u64>,
    /// exactness harness: placeholder geometry recorded from the python run, keyed (class, factory seed, instance seed)
    pub oracle: HashMap<(String, u64, u64), ObjGeom>,
    /// real (populated) asset meshes recorded from python, keyed like `oracle`
    pub populated: HashMap<(String, u64, u64), ObjGeom>,
    /// exactness harness: unseeded constructor word counts recorded from python (one entry per constructed factory, in order)
    pub ctor_words: std::collections::VecDeque<(String, u32)>,
}

pub fn load_oracle(path: &str) -> (HashMap<(String, u64, u64), ObjGeom>, HashMap<(String, u64, u64), ObjGeom>) {
    use super::geom::{subpart_bit, Proxy, Tri};
    let mut out = HashMap::default();
    let mut pop = HashMap::default();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let j: serde_json::Value = serde_json::from_str(line).unwrap();
        let is_pop = j.get("kind").and_then(|k| k.as_str()) == Some("populated");
        let verts: Vec<V3> = j["verts"].as_array().unwrap().iter().map(|v| { let a = v.as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] }).collect();
        let names = j["face_names"].as_array().unwrap();
        let mut tris = vec![];
        for (pi, poly) in j["polys"].as_array().unwrap().iter().enumerate() {
            let ix: Vec<usize> = poly.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
            let mut bits = 0u32;
            if !names.is_empty() {
                for part in names[pi].as_str().unwrap().split('.') {
                    let mut c = part.to_string();
                    if let Some(f) = c.get_mut(0..1) { f.make_ascii_uppercase(); }
                    bits |= match part { "support" => super::geom::SUPPORT, _ => subpart_bit(&c) };
                }
            }
            tris.push(Tri { v: [verts[ix[0]], verts[ix[1]], verts[ix[2]]], tags: bits });
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &verts { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
        let proxy = if verts.len() == 8 {
            Proxy::Box { center: [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.], half: [(hi[0] - lo[0]) / 2., (hi[1] - lo[1]) / 2., (hi[2] - lo[2]) / 2.] }
        } else {
            Proxy::Hull(verts.clone())
        };
        let g = ObjGeom::from_tris(tris, vec![(proxy, u32::MAX)]).with_mesh_verts(verts.clone());
        let key = (j["class"].as_str().unwrap().to_string(), j["fseed"].as_u64().unwrap(), j["inst"].as_i64().unwrap() as u64);
        if is_pop {
            pop.insert(key, g);
        } else {
            out.insert(key, g);
        }
    }
    (out, pop)
}

#[derive(Clone)]
pub struct PoseBackup {
    pose: Pose,
    dof_t: Option<[[f64; 3]; 3]>,
    dof_r: Option<V3>,
}

fn backup(o: &ObjState) -> PoseBackup {
    PoseBackup { pose: o.pose, dof_t: o.dof_t, dof_r: o.dof_r }
}

fn restore(state: &mut State, name: &str, b: &PoseBackup, with_dof: bool) {
    let o = state.objs.get_mut(name).unwrap();
    // python restores location and rotation_euler only; the (float32 drifting) scale is left as is
    let mut ob = b.pose.o;
    ob.scale = o.pose.o.scale;
    o.pose = Pose::from_o32(ob);
    if with_dof {
        o.dof_t = b.dof_t;
        o.dof_r = b.dof_r;
    }
    state.touch(name);
}

pub enum Move {
    Addition { name: String, class: String, assignments: Vec<RelState>, force_tags: TagSet },
    Deletion { name: String, backup: Option<ObjState> },
    Translate { name: String, v: V3, bak: Option<PoseBackup> },
    Rotate { name: String, axis: V3, angle: f64, bak: Option<PoseBackup> },
    ReinitPose { name: String, bak: Option<PoseBackup> },
    PlaneChange { name: String, rel_idx: usize, plane_idx: usize, old_idx: usize, bak: Option<PoseBackup> },
    Resample { name: String, old: Option<(ObjState, PoseBackup)> },
}

impl Move {
    pub fn name(&self) -> &str {
        match self {
            Move::Addition { name, .. } | Move::Deletion { name, .. } | Move::Translate { name, .. } | Move::Rotate { name, .. } | Move::ReinitPose { name, .. } | Move::PlaneChange { name, .. } | Move::Resample { name, .. } => name,
        }
    }
}

/// draws a factory's __init__ makes on the global numpy stream (outside its FixedSeed); see assets/ctor_draws.rs
fn unseeded_init_draws(class: &str, rng: &mut Rng) {
    crate::assets::set_unseeded(crate::assets::ctor_draws::ctor_draws(class, rng));
}

pub fn sample_rand_placeholder(ctx: &mut Ctx, rng: &mut Rng, class: &str) -> Option<(ObjGeom, super::state::GenRef)> {
    let singleton = ctx.usage.has_usage(class, "SingleGenerator");
    let fac_seed = if singleton && ctx.singletons.contains_key(class) {
        ctx.singletons[class]
    } else {
        let s = rng.randint(0, 10_000_000) as u64;
        if singleton {
            ctx.singletons.insert(class.to_string(), s);
        }
        // factory constructors draw from the global stream before they enter FixedSeed(factory_seed)
        if std::env::var("BRUTE").is_ok() && class == "CoffeeTableFactory" {
            for k in 0..12 {
                let mut r2 = rng.clone();
                for _ in 0..k { r2.next_u32(); }
                eprintln!("BRUTE words={k} -> inst {}", r2.randint(0, 10_000_000));
            }
        }
        if std::env::var("CTOREMU").is_ok() {
            // emulate the python constructors without the oracle; if CTORWORDS is loaded too, compare word counts
            let before = rng.clone();
            unseeded_init_draws(class, rng);
            if let Some((c, k)) = ctx.ctor_words.pop_front() {
                let got = crate::assets::ctor_draws::words_between(&before, rng);
                if c != class || got.map(|g| g % 624) != Some(k) {
                    eprintln!("CTOR MISMATCH python {c} k={k} rust {class} k={got:?}");
                } else {
                    eprintln!("CTOR OK {class} k={k}");
                }
            }
        } else if let Some((c, k)) = ctx.ctor_words.pop_front() {
            if c != class {
                eprintln!("CTOR MISMATCH python {c} rust {class}");
            }
            if matches!(class, "SideTableFactory" | "CoffeeTableFactory") {
                // real draws: the gaussian cache state matters
                unseeded_init_draws(class, rng);
            } else {
                crate::assets::set_unseeded(None);
                for _ in 0..k {
                    rng.next_u32();
                }
            }
        } else {
            unseeded_init_draws(class, rng);
        }
        s
    };
    let inst_seed = rng.randint(0, 10_000_000) as u64;
    let kind = if ctx.usage.has_usage(class, "RealPlaceholder") {
        PlaceholderKind::Real
    } else if ctx.usage.has_usage(class, "AssetAsPlaceholder") {
        PlaceholderKind::AssetAsPlaceholder
    } else if ctx.usage.has_usage(class, "PlaceholderBBox") {
        PlaceholderKind::BboxOfPlaceholder
    } else {
        PlaceholderKind::BboxOfAsset
    };
    let geom = match ctx.oracle.get(&(class.to_string(), fac_seed, inst_seed)) {
        Some(g) => g.clone(),
        None => {
            if !ctx.oracle.is_empty() {
                eprintln!("ORACLE MISS {class} {fac_seed} {inst_seed}");
            }
            ctx.assets.placeholder(class, fac_seed, inst_seed, kind)?
        }
    };
    Some((geom, super::state::GenRef { class: class.to_string(), factory_seed: fac_seed, inst_seed }))
}

pub fn apply_move(mv: &mut Move, state: &mut State, ctx: &mut Ctx, rng: &mut Rng) -> bool {
    match mv {
        Move::Addition { name, class, assignments, force_tags } => {
            if state.objs.contains_key(name.as_str()) {
                eprintln!("NAME COLLISION {name}");
                return false;
            }
            let Some((geom, gen)) = sample_rand_placeholder(ctx, rng, class) else {
                if std::env::var("ADDDBG").is_ok() {
                    eprintln!("ADD {class}: no placeholder");
                }
                return false;
            };
            let mut tags = force_tags.clone();
            tags.update(&ctx.usage.usages_of(class));
            let mut o = ObjState::new(Arc::new(geom), Pose::identity(), tags);
            o.generator = Some(gen);
            o.relations = assignments.clone();
            state.objs.insert(name.clone(), o);
            state.touch(name);
            let okk = try_apply_relation_constraints(state, name, rng);
            if std::env::var("ADDDBG").is_ok() {
                eprintln!("ADD {class}: relation placement ok={okk}");
            }
            okk
        }
        Move::Deletion { name, backup } => {
            *backup = state.objs.shift_remove(name.as_str());
            true
        }
        Move::Translate { name, v, bak } => {
            *bak = Some(backup(&state.objs[name.as_str()]));
            let mut p = state.objs[name.as_str()].pose;
            p.translate(*v);
            state.set_pose(name, p);
            check_post_move_validity(state, name)
        }
        Move::Rotate { name, axis, angle, bak } => {
            *bak = Some(backup(&state.objs[name.as_str()]));
            let mut p = state.objs[name.as_str()].pose;
            p.rotate_axis(*axis, *angle);
            state.set_pose(name, p);
            check_post_move_validity(state, name)
        }
        Move::ReinitPose { name, bak } => {
            *bak = Some(backup(&state.objs[name.as_str()]));
            try_apply_relation_constraints(state, name, rng)
        }
        Move::PlaneChange { name, rel_idx, plane_idx, old_idx, bak } => {
            *bak = Some(backup(&state.objs[name.as_str()]));
            *old_idx = state.objs[name.as_str()].relations[*rel_idx].parent_plane;
            state.objs.get_mut(name.as_str()).unwrap().relations[*rel_idx].parent_plane = *plane_idx;
            try_apply_relation_constraints(state, name, rng)
        }
        Move::Resample { name, old } => {
            let o = state.objs[name.as_str()].clone();
            let class = o.generator.as_ref().unwrap().class.clone();
            let b = backup(&o);
            let Some((geom, gen)) = sample_rand_placeholder(ctx, rng, &class) else { return false };
            *old = Some((o, b));
            let oo = state.objs.get_mut(name.as_str()).unwrap();
            oo.geom = Arc::new(geom);
            oo.generator = Some(gen);
            // the resampled placeholder is a brand new blender object: origin, no rotation, unit scale
            oo.pose = Pose::identity();
            state.touch(name);
            let _ = apply_relations_surfacesample(state, name, rng);
            check_post_move_validity(state, name)
        }
    }
}

pub fn revert_move(mv: &mut Move, state: &mut State) {
    match mv {
        Move::Addition { name, .. } => {
            state.objs.shift_remove(name.as_str());
        }
        Move::Deletion { name, backup } => {
            if let Some(b) = backup.take() {
                state.objs.insert(name.clone(), b);
                state.touch(name);
                // parse_scene.add_to_scene(preprocess=True) applies the object scale to the mesh
                state.bake_scale(name);
                state.matrix_roundtrip(name);
            }
        }
        Move::Translate { name, bak, .. } | Move::Rotate { name, bak, .. } => {
            if let Some(b) = bak {
                restore(state, name, b, false);
            }
        }
        Move::ReinitPose { name, bak } => {
            if let Some(b) = bak {
                restore(state, name, b, true);
            }
        }
        Move::PlaneChange { name, rel_idx, old_idx, bak, .. } => {
            state.objs.get_mut(name.as_str()).unwrap().relations[*rel_idx].parent_plane = *old_idx;
            if let Some(b) = bak {
                restore(state, name, b, true);
            }
        }
        Move::Resample { name, old } => {
            if let Some((o, b)) = old.take() {
                let oo = state.objs.get_mut(name.as_str()).unwrap();
                oo.geom = o.geom;
                oo.generator = o.generator;
                oo.pose = b.pose;
                oo.dof_t = b.dof_t;
                oo.dof_r = b.dof_r;
                state.touch(name);
                // add_to_scene(preprocess=False) of the old object, then restore_pose_backup (loc + euler)
                state.matrix_roundtrip(name);
                let oo = state.objs.get_mut(name.as_str()).unwrap();
                let mut ob = oo.pose.o;
                ob.loc = b.pose.o.loc;
                ob.eul = b.pose.o.eul;
                oo.pose = Pose::from_o32(ob);
                state.touch(name);
            }
        }
    }
}

// ---------------- proposals ----------------

fn active_for_stage(prop: &Domain, filter: &Domain) -> bool {
    prop.intersects(filter, false, true)
}

pub fn preproc_bounds(bounds: &[Bound], state: &State, filter: &Domain, reverse: bool, shuffle: bool, rng: &mut Rng) -> Vec<Bound> {
    let mut b: Vec<Bound> = bounds.iter().filter(|b| active_for_stage(&b.domain, filter)).cloned().collect();
    if shuffle {
        rng.shuffle(&mut b);
    }
    let counts: Vec<usize> = b.iter().map(|x| state.objkeys_in_dom(&x.domain).len()).collect();
    let key = |i: usize| -> i32 {
        let bb = &b[i];
        let bc = counts[i] as f64;
        let res = if bb.high.map_or(false, |h| h < bc) {
            1
        } else if bb.low.map_or(false, |l| l > bc) {
            -1
        } else {
            0
        };
        if reverse { -res } else { res }
    };
    let mut order: Vec<usize> = (0..b.len()).collect();
    order.sort_by_key(|&i| key(i)); // stable like python sorted
    order.into_iter().filter(|&i| key(i) != 1).map(|i| b[i].clone()).collect()
}

fn lookup_generator(ctx: &Ctx, preds: &TagSet, rng: &mut Rng) -> Vec<String> {
    assert!(!contradiction(preds));
    let gen_tags: Vec<Tag> = preds.pos().filter(|t| t.kind() == Kind::Gen).collect();
    if gen_tags.len() == 1 {
        return vec![gen_tags[0].name()];
    }
    let mut options = ctx.usage.all_factories();
    for t in preds.pos() {
        let f = ctx.usage.factories_for_usage(t);
        options = options.intersection(&f).cloned().collect();
    }
    for t in preds.neg() {
        let f = ctx.usage.factories_for_usage(t);
        options = options.difference(&f).cloned().collect();
    }
    let mut v: Vec<String> = options.into_iter().collect();
    v.sort();
    if std::env::var("LOOKUP").is_ok() && preds.pos().any(|t| t.name().contains("OfficeShelf")) {
        eprintln!("LOOKUP pos={:?} neg={:?} -> {:?}", preds.pos().map(|t| t.name()).collect::<Vec<_>>(), preds.neg().map(|t| t.name()).collect::<Vec<_>>(), v);
    }
    rng.shuffle(&mut v);
    v
}

fn minimize_redundant_relations(mut relations: Vec<RelDom>) -> (RelDom, Vec<RelDom>, bool) {
    // AnyRelations to the back; python sorts with reverse=True on a bool key => True first, i.e. AnyRelation FIRST
    relations.sort_by_key(|r| std::cmp::Reverse(matches!(r.0, Relation::Any)));
    let first = relations.remove(0);
    let (rel, dom) = (first.0.clone(), first.1.clone());
    let mut remaining = vec![];
    for (r_later, mut d_later) in relations {
        if d_later.intersects(&dom, false, false) {
            d_later = d_later.intersection(&dom);
        }
        if reldom_implies(&(rel.clone(), dom.clone()), &(r_later.clone(), d_later.clone())) {
            continue;
        }
        remaining.push((r_later, d_later));
    }
    let implied = remaining.iter().any(|later| reldom_implies(later, &(rel.clone(), dom.clone())));
    ((rel, dom), remaining, implied)
}

struct Frame {
    rel: Relation,
    remaining: Vec<RelDom>,
    cands: Vec<String>,
    ci: usize,
    cur: String,
    order: Vec<usize>,
    oi: usize,
    have_order: bool,
    base_len: usize,
    parent_tags: TagSet,
}

/// Lazy port of propose_relations.find_assignments (a python generator: shuffles happen on demand).
pub struct AssignGen {
    init: Option<Vec<RelDom>>,
    stack: Vec<Frame>,
    assignments: Vec<RelState>,
}

impl AssignGen {
    fn new(relations: Vec<RelDom>) -> Self {
        AssignGen { init: Some(relations), stack: vec![], assignments: vec![] }
    }

    fn descend(&mut self, state: &State, mut relations: Vec<RelDom>) -> Option<Vec<RelState>> {
        loop {
            if relations.is_empty() {
                return Some(self.assignments.clone());
            }
            let ((rel, dom), remaining, implied) = minimize_redundant_relations(relations);
            if implied {
                relations = remaining;
                continue;
            }
            if matches!(rel, Relation::Any) {
                panic!("Got AnyRelation as first relation");
            }
            let g = rel.geo().expect("geometry relation");
            let cands = state.objkeys_in_dom(&dom);
            self.stack.push(Frame { parent_tags: g.parent.clone(), rel, remaining, cands, ci: 0, cur: String::new(), order: vec![], oi: 0, have_order: false, base_len: self.assignments.len() });
            return None;
        }
    }

    fn next(&mut self, state: &State, rng: &mut Rng) -> Option<Vec<RelState>> {
        if let Some(r) = self.init.take() {
            if let Some(a) = self.descend(state, r) {
                return Some(a);
            }
        }
        loop {
            let f = self.stack.last_mut()?;
            if !f.have_order {
                if f.ci >= f.cands.len() {
                    self.stack.pop();
                    continue;
                }
                f.cur = f.cands[f.ci].clone();
                f.ci += 1;
                let n = state.planes(&f.cur, &f.parent_tags).len();
                f.order = (0..n).collect();
                rng.shuffle(&mut f.order);
                f.oi = 0;
                f.have_order = true;
            }
            if f.oi >= f.order.len() {
                f.have_order = false;
                continue;
            }
            let pp = f.order[f.oi];
            f.oi += 1;
            let rs = RelState { relation: f.rel.clone(), target: f.cur.clone(), child_plane: 0, parent_plane: pp };
            let rem = f.remaining.clone();
            let bl = f.base_len;
            self.assignments.truncate(bl);
            self.assignments.push(rs);
            if let Some(a) = self.descend(state, rem) {
                return Some(a);
            }
        }
    }
}

/// Lazily evaluated move proposals (python generators): RNG draws interleave with apply/evaluate.
pub enum Gen {
    Addition { started: bool, bounds: Vec<Bound>, bi: usize, options: Option<Vec<String>>, ci: usize, assign: Option<AssignGen>, tags: TagSet },
    Deletion { started: bool, bounds: Vec<Bound>, bi: usize, cands: Vec<String>, ci: usize, have: bool },
    PlaneChange { started: bool, cands: Vec<String>, ci: usize, ri: usize, order: Vec<usize>, oi: usize, have_order: bool },
    Resample { started: bool, cands: Vec<String>, ci: usize },
    Reinit { cands: Vec<String> },
    Translate { cands: Vec<String>, temp: f64 },
    Rotate { cands: Vec<String>, temp: f64 },
}

fn pose_candidates(state: &State, filter: &Domain) -> Vec<String> {
    state.objs.iter().filter(|(_, o)| o.active && state.domain_contains(filter, o)).map(|(k, _)| k.clone()).collect()
}

pub fn make_gen(kind: MoveKind, state: &State, filter: &Domain, temp: f64) -> Gen {
    match kind {
        MoveKind::Addition => Gen::Addition { started: false, bounds: vec![], bi: 0, options: None, ci: 0, assign: None, tags: TagSet::new() },
        MoveKind::Deletion => Gen::Deletion { started: false, bounds: vec![], bi: 0, cands: vec![], ci: 0, have: false },
        MoveKind::PlaneChange => Gen::PlaneChange { started: false, cands: vec![], ci: 0, ri: 0, order: vec![], oi: 0, have_order: false },
        MoveKind::Resample => Gen::Resample { started: false, cands: vec![], ci: 0 },
        MoveKind::ReinitPose => Gen::Reinit { cands: pose_candidates(state, filter).into_iter().filter(|k| state.objs[k].dof_t.is_some()).collect() },
        MoveKind::Translate => Gen::Translate { cands: pose_candidates(state, filter).into_iter().filter(|k| state.objs[k].dof_t.is_some()).collect(), temp },
        MoveKind::Rotate => {
            let no_rot = Tag::sem("NoRotation");
            let cands = pose_candidates(state, filter)
                .into_iter()
                .filter(|k| {
                    let o = &state.objs[k];
                    !o.tags.contains(no_rot) && o.dof_r.map_or(false, |a| dot(a, [0., 0., 1.]) > 0.95)
                })
                .collect();
            Gen::Rotate { cands, temp }
        }
    }
}

impl Gen {
    pub fn next(&mut self, ctx: &mut Ctx, state: &State, filter: &Domain, rng: &mut Rng) -> Option<Move> {
        match self {
            Gen::Addition { started, bounds, bi, options, ci, assign, tags } => {
                if !*started {
                    *started = true;
                    *bounds = preproc_bounds(&ctx.bounds.clone(), state, filter, false, true, rng);
                }
                loop {
                    if let Some(ag) = assign {
                        if let Some(a) = ag.next(state, rng) {
                            let class = options.as_ref().unwrap()[*ci].clone();
                            let mut name;
                            let mut tries = 0;
                            loop {
                                let first = format!("{:04}_{}", rng.randint(0, 1_000_000), class);
                                if !state.objs.contains_key(&first) {
                                    name = format!("{:04}_{}", rng.randint(0, 1_000_000), class);
                                    break;
                                }
                                tries += 1;
                                if tries >= 100 {
                                    name = first;
                                    break;
                                }
                            }
                            return Some(Move::Addition { name, class, assignments: a, force_tags: tags.clone() });
                        }
                        *assign = None;
                        *ci += 1;
                        continue;
                    }
                    if *bi >= bounds.len() {
                        return None;
                    }
                    let bound = bounds[*bi].clone();
                    if bound.low.is_none() {
                        *bi += 1;
                        continue;
                    }
                    if options.is_none() {
                        let o = lookup_generator(ctx, &bound.domain.tags, rng);
                        if o.is_empty() {
                            if bound.low.is_none() || bound.low == Some(0.0) {
                                *bi += 1;
                                continue;
                            }
                            panic!("no generators for {:?}", bound.domain.tags);
                        }
                        *options = Some(o);
                        *ci = 0;
                    }
                    if *ci >= options.as_ref().unwrap().len() {
                        *options = None;
                        *bi += 1;
                        continue;
                    }
                    let class = options.as_ref().unwrap()[*ci].clone();
                    let found = ctx.usage.usages_of(&class);
                    let mut prop_dom = bound.domain.intersection(filter);
                    prop_dom.tags.update(&found);
                    let search: Vec<RelDom> = prop_dom.relations.iter().filter(|r| !r.0.is_neg()).cloned().collect();
                    assert!(!bound.domain.relations.is_empty(), "unconstrained {class}");
                    *tags = prop_dom.tags.clone();
                    *assign = Some(AssignGen::new(search));
                }
            }
            Gen::Deletion { started, bounds, bi, cands, ci, have } => {
                if !*started {
                    *started = true;
                    let mut b = preproc_bounds(&ctx.bounds, state, filter, true, true, rng);
                    if b.is_empty() {
                        return None;
                    }
                    rng.shuffle(&mut b);
                    *bounds = b;
                }
                loop {
                    if *bi >= bounds.len() {
                        return None;
                    }
                    if !*have {
                        *cands = state.objkeys_in_dom(&bounds[*bi].domain);
                        rng.shuffle(cands);
                        *ci = 0;
                        *have = true;
                    }
                    if *ci >= cands.len() {
                        *have = false;
                        *bi += 1;
                        continue;
                    }
                    let c = cands[*ci].clone();
                    *ci += 1;
                    return Some(Move::Deletion { name: c, backup: None });
                }
            }
            Gen::PlaneChange { started, cands, ci, ri, order, oi, have_order } => {
                if !*started {
                    *started = true;
                    *cands = state.objkeys_in_dom(filter);
                    rng.shuffle(cands);
                }
                loop {
                    if *ci >= cands.len() {
                        return None;
                    }
                    let cand = cands[*ci].clone();
                    let rels = &state.objs[&cand].relations;
                    if *ri >= rels.len() {
                        *ci += 1;
                        *ri = 0;
                        *have_order = false;
                        continue;
                    }
                    let rs = &rels[*ri];
                    if !*have_order {
                        let Some(g) = rs.relation.geo() else {
                            *ri += 1;
                            continue;
                        };
                        let n = state.planes(&rs.target, &g.parent).len();
                        if n <= 1 {
                            *ri += 1;
                            continue;
                        }
                        *order = (0..n).collect();
                        rng.shuffle(order);
                        *oi = 0;
                        *have_order = true;
                    }
                    if *oi >= order.len() {
                        *have_order = false;
                        *ri += 1;
                        continue;
                    }
                    let pi = order[*oi];
                    *oi += 1;
                    if pi == rs.parent_plane {
                        continue;
                    }
                    return Some(Move::PlaneChange { name: cand.clone(), rel_idx: *ri, plane_idx: pi, old_idx: 0, bak: None });
                }
            }
            Gen::Resample { started, cands, ci } => {
                if !*started {
                    *started = true;
                    *cands = state.objkeys_in_dom(filter);
                    rng.shuffle(cands);
                }
                while *ci < cands.len() {
                    let c = cands[*ci].clone();
                    *ci += 1;
                    let class = &state.objs[&c].generator.as_ref().unwrap().class;
                    if ctx.usage.has_usage(class, "SingleGenerator") {
                        continue;
                    }
                    return Some(Move::Resample { name: c, old: None });
                }
                None
            }
            Gen::Reinit { cands } => {
                if cands.is_empty() {
                    return None;
                }
                Some(Move::ReinitPose { name: rng.choice(cands).clone(), bak: None })
            }
            Gen::Translate { cands, temp } => {
                if cands.is_empty() {
                    return None;
                }
                let name = rng.choice(cands).clone();
                let var = (8.0 * *temp).max(0.01);
                let r = [rng.normal(0.0, var), rng.normal(0.0, var), rng.normal(0.0, var)];
                let m = state.objs[&name].dof_t.unwrap();
                let v = [dot(m[0], r), dot(m[1], r), dot(m[2], r)];
                if crate::rng::TRACE_ON.with(|t| t.get()) { eprintln!("RT   TRANSLATE {name} r={r:?} var={var} dof={m:?} v={v:?}"); }
                Some(Move::Translate { name, v, bak: None })
            }
            Gen::Rotate { cands, temp } => {
                if cands.is_empty() {
                    return None;
                }
                let step = 2.0 * std::f64::consts::PI / 8.0;
                let name = rng.choice(cands).clone();
                let var = (std::f64::consts::PI * *temp).max(0.0);
                let ang = rng.normal(0.0, var) / step;
                let ang = if ang > 0.0 { ang.ceil() } else { ang.floor() };
                let axis = state.objs[&name].dof_r.unwrap();
                Some(Move::Rotate { name, axis, angle: ang * step, bak: None })
            }
        }
    }
}

// ---------------- annealing ----------------

#[derive(Clone, Copy)]
pub struct Schedule {
    pub start: f64,
    pub end: f64,
    pub pct: f64,
}

impl Schedule {
    fn at(&self, t: f64) -> f64 {
        if t < 0.0 {
            return self.start;
        }
        if t > self.pct {
            return self.end;
        }
        self.start + (self.end - self.start) * (t / self.pct)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MoveKind {
    Addition,
    Deletion,
    PlaneChange,
    Resample,
    ReinitPose,
    Translate,
    Rotate,
}

pub struct AnnealCfg {
    pub initial_temp: f64,
    pub final_temp: f64,
    pub finetune_pct: f64,
    pub max_invalid_candidates: usize,
}

pub fn move_weights(restrict: Option<&[MoveKind]>, add_scalar: f64) -> Vec<(MoveKind, Schedule)> {
    let all = [
        (MoveKind::Addition, Schedule { start: 6.0 * add_scalar, end: 0.1 * add_scalar, pct: 0.9 }),
        (MoveKind::Deletion, Schedule { start: 2.0, end: 0.0, pct: 0.5 }),
        (MoveKind::PlaneChange, Schedule { start: 2.0, end: 0.1, pct: 1.0 }),
        (MoveKind::Resample, Schedule { start: 1.0, end: 0.1, pct: 0.7 }),
        (MoveKind::ReinitPose, Schedule { start: 1.0, end: 0.5, pct: 1.0 }),
        (MoveKind::Translate, Schedule { start: 1.0, end: 1.0, pct: 1.0 }),
        (MoveKind::Rotate, Schedule { start: 0.5, end: 0.5, pct: 1.0 }),
    ];
    all.into_iter().filter(|(k, _)| restrict.map_or(true, |r| r.contains(k))).collect()
}

pub struct Annealer {
    pub stats: HashMap<String, [usize; 4]>, // kind -> [steps, proposed, valid, accepted]
    pub cfg: AnnealCfg,
    pub memo: Memo,
    pub curr: Option<(f64, f64)>, // (loss, viol)
    pub iter: usize,
    pub max_iters: usize,
    cooling: f64,
}

impl Annealer {
    pub fn new(cfg: AnnealCfg, memo: Memo, max_iters: usize) -> Annealer {
        let cooling = if max_iters == 0 {
            0.0
        } else {
            let steps = max_iters as f64 * (1.0 - cfg.finetune_pct);
            (cfg.final_temp / cfg.initial_temp).powf(1.0 / steps)
        };
        Annealer { stats: HashMap::default(), cfg, memo, curr: None, iter: 0, max_iters, cooling }
    }

    fn temp(&self) -> f64 {
        (self.cfg.initial_temp * self.cooling.powi(self.iter as i32)).clamp(self.cfg.final_temp, self.cfg.initial_temp)
    }

    fn evaluate(&mut self, ctx: &Ctx, state: &State) -> (f64, f64) {
        if std::env::var("NOMEMO").is_ok() {
            self.memo.clear();
        }
        let mut ev = Evaluator::new(ctx.prog, state, &mut self.memo);
        let r = ev.evaluate_problem(true, true);
        if crate::rng::TRACE_ON.with(|t| t.get()) {
            eprintln!("RT   EVAL {}", r.loss_vals.iter().map(|(k, v)| format!("{k}:{v:.5}")).collect::<Vec<_>>().join(" "));
        }
        (r.loss(), r.viol_count())
    }

    pub fn step(&mut self, ctx: &mut Ctx, state: &mut State, kind: MoveKind, filter: &Domain, rng: &mut Rng) {
        if self.curr.is_none() {
            self.curr = Some(self.evaluate(ctx, state));
        }
        let temp = self.temp();
        let mut gen = make_gen(kind, state, filter, temp);
        let st = self.stats.entry(format!("{kind:?}")).or_insert([0; 4]);
        st[0] += 1;
        let mut chosen: Option<(Move, (f64, f64))> = None;
        let mut retry = 0usize;
        while let Some(mut mv) = gen.next(ctx, state, filter, rng) {
            self.stats.get_mut(&format!("{kind:?}")).unwrap()[1] += 1;
            if retry == self.cfg.max_invalid_candidates {
                break;
            }
            if apply_move(&mut mv, state, ctx, rng) {
                self.stats.get_mut(&format!("{kind:?}")).unwrap()[2] += 1;
                self.evict(ctx, state, &mv);
                let r = self.evaluate(ctx, state);
                chosen = Some((mv, r));
                break;
            }
            self.evict(ctx, state, &mv);
            revert_move(&mut mv, state);
            retry += 1;
        }
        if let Some((mut mv, prop)) = chosen {
            let diff = prop.0 - self.curr.unwrap().0;
            let log_prob = -diff / temp;
            let viol_diff = prop.1 - self.curr.unwrap().1;
            let accept = if viol_diff < 0.0 {
                true
            } else if viol_diff > 0.0 {
                false
            } else {
                rng.f64().ln() < log_prob
            };
            if let Ok(t) = std::env::var("TRACE") {
                if mv_name(&mv).contains(&t) {
                    eprintln!("TRACE it={} {:?} accept={accept} loss={:.3} viol={:.1} diff={:.2} vdiff={:.1} temp={:.3} {}", self.iter, kind, prop.0, prop.1, diff, viol_diff, temp, mv_name(&mv));
                    if std::env::var("TRACEV").is_ok() {
                        let mut ev = Evaluator::new(ctx.prog, state, &mut self.memo);
                        let r = ev.evaluate_problem(true, true);
                        eprintln!("   viol terms {:?} (curr loss {:.3} viol {:.1})", r.violations.iter().filter(|x| x.1 > 0.0).collect::<Vec<_>>(), self.curr.unwrap().0, self.curr.unwrap().1);
                    }
                }
            }
            if accept {
                self.stats.get_mut(&format!("{kind:?}")).unwrap()[3] += 1;
                self.curr = Some(prop);
                if let Move::Deletion { backup, .. } = &mut mv {
                    *backup = None;
                }
            } else {
                self.evict(ctx, state, &mv);
                revert_move(&mut mv, state);
            }
        }
        self.iter += 1;
    }

    fn evict(&mut self, ctx: &Ctx, state: &State, mv: &Move) {
        match mv {
            Move::Deletion { .. } => self.memo.clear(),
            m => {
                if let Some(o) = state.objs.get(m.name()) {
                    self.memo.evict_for_obj(ctx.prog, &o.tags);
                } else {
                    self.memo.clear();
                }
            }
        }
        super::col::clear_stale(state);
    }
}

pub fn choose_move_type(moves: &[(MoveKind, Schedule)], it: usize, max_it: usize, rng: &mut Rng) -> MoveKind {
    let t = it as f64 / max_it as f64;
    let w: Vec<f64> = moves.iter().map(|(_, s)| s.at(t)).collect();
    moves[rng.choice_p(&w)].0
}

// ---------------- greedy driver ----------------

pub fn is_active_room_object(state: &State, key: &str, assign: &IndexMap<String, String>) -> bool {
    for (var, a) in assign {
        match var.as_str() {
            "room" => {
                let anc = find_ancestors(state, key, &Tag::sem("Room"), &mut BTreeSet::new());
                if !anc.contains(a) {
                    return false;
                }
            }
            "obj" => {
                let anc = find_ancestors(state, key, &Tag::sem("Object"), &mut BTreeSet::new());
                if !anc.is_empty() && !anc.contains(key) {
                    return false;
                }
            }
            v => panic!("unknown var {v}"),
        }
    }
    true
}

fn find_ancestors(state: &State, key: &str, filter: &Tag, seen: &mut BTreeSet<String>) -> BTreeSet<String> {
    seen.insert(key.to_string());
    let o = &state.objs[key];
    // Domain({filter}) containment = tag satisfaction
    if satisfies(&o.tags, &TagSet::from([*filter])) {
        return BTreeSet::from([key.to_string()]);
    }
    let mut res = BTreeSet::new();
    for rs in &o.relations {
        if seen.contains(&rs.target) {
            continue;
        }
        res.extend(find_ancestors(state, &rs.target, filter, seen));
    }
    res
}

pub fn update_active_flags(state: &mut State, assign: &IndexMap<String, String>) -> usize {
    let keys: Vec<String> = state.objs.keys().cloned().collect();
    let mut count = 0;
    for k in keys {
        let a = is_active_room_object(state, &k, assign);
        state.objs.get_mut(&k).unwrap().active = a;
        count += a as usize;
    }
    count
}

/// greedy.iterate_assignments
pub fn iterate_assignments(dom: &Domain, state: &State, vars: &[&str], limits: &HashMap<String, usize>) -> Vec<IndexMap<String, String>> {
    if vars.is_empty() {
        return vec![IndexMap::new()];
    }
    let var = vars[0];
    let vtag = Tag::var(var);
    let mut all = vec![];
    dom.traverse(&mut all);
    let doms_for_var: Vec<&Domain> = all.into_iter().filter(|d| d.tags.contains(vtag)).collect();
    if doms_for_var.is_empty() {
        return vec![IndexMap::new()];
    }
    let mut combined = doms_for_var[0].clone();
    for d in &doms_for_var[1..] {
        combined = combined.intersection(d);
    }
    combined.tags.remove(vtag);
    assert!(combined.intersects(&combined.clone(), false, false));
    let mut cands = state.objkeys_in_dom(&combined);
    cands.sort();
    let nochild = Tag::sem("NoChildren");
    cands.retain(|c| !state.objs[c].tags.contains(nochild));
    let mut out = vec![];
    for (i, key) in cands.iter().enumerate() {
        if let Some(l) = limits.get(var) {
            if i >= *l {
                break;
            }
        }
        let sub = combined.with_tags(&TagSet::from([Tag::obj(key)]));
        let dom_objkey = domain_tag_substitute(dom, vtag, &sub).0;
        for rest in iterate_assignments(&dom_objkey, state, &vars[1..], limits) {
            let mut m = IndexMap::new();
            m.insert(var.to_string(), key.clone());
            m.extend(rest);
            out.push(m);
        }
    }
    out
}

pub struct StageParams {
    pub n_steps: usize,
    pub restrict_moves: Option<Vec<MoveKind>>,
    pub addition_scalar: f64,
}

/// Solver.solve_objects
pub fn solve_objects(ctx: &mut Ctx, state: &mut State, stage: &Domain, assign: &IndexMap<String, String>, sp: &StageParams, cfg: AnnealCfg, rng: &mut Rng) -> usize {
    let moves = move_weights(sp.restrict_moves.as_deref(), sp.addition_scalar);
    let subs: Vec<(Tag, Domain)> = assign.iter().map(|(k, objkey)| (Tag::var(k), Domain::new(state.objs[objkey].tags.clone()))).collect();
    let filter = substitute_all(stage, &subs);
    assert!(filter.finalized(false, true), "non-finalized stage domain");
    let orig = ctx.prog.constraint_bounds();
    ctx.bounds = orig.clone();
    let bounds = preproc_bounds(&orig, state, &filter, false, true, rng);
    if bounds.is_empty() {
        return 0;
    }
    update_active_flags(state, assign);
    let n_start = state.objs.len();
    let stage_no = crate::rng::CUR.with(|c| c.get().0) + 1;
    crate::rng::set_cur(stage_no, -1);
    let rt: Option<usize> = std::env::var("RNGTR").ok().and_then(|v| v.parse().ok());
    if rt.is_some() { eprintln!("RT STAGE peek {}", rng.peek()); }
    let memo = Memo::new(ctx.prog, Some(&filter));
    let mut ann = Annealer::new(cfg, memo, sp.n_steps);
    for j in 0..sp.n_steps {
        let tr = rt.map_or(false, |l| j < l);
        crate::rng::set_cur(stage_no, j as i64);
        crate::rng::set_trace(tr);
        if tr {
            eprintln!("RT ITER {j} choose peek {}", rng.peek());
            for (k, o) in state.objs.iter() {
                if o.generator.is_some() {
                    eprintln!("RT   OBJ {k} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.8} {:.8} {:.8}", o.pose.o.loc[0], o.pose.o.loc[1], o.pose.o.loc[2], o.pose.o.eul[0], o.pose.o.eul[1], o.pose.o.eul[2], o.pose.o.scale[0], o.pose.o.scale[1], o.pose.o.scale[2]);
                }
            }
        }
        let kind = choose_move_type(&moves, j, sp.n_steps, rng);
        if tr { eprintln!("RT   chose {kind:?}\nRT   step peek {}", rng.peek()); }
        ann.step(ctx, state, kind, &filter, rng);
        if tr { eprintln!("RT   end peek {}", rng.peek()); }
        crate::rng::set_trace(false);
    }
    if std::env::var("DBG").is_ok() {
        eprintln!("stage assign={:?} bounds={} objs {}->{} loss/viol={:?} stats={:?}", assign, bounds.len(), n_start, state.objs.len(), ann.curr, ann.stats);
    }
    for o in state.objs.values_mut() {
        o.active = true;
    }
    state.objs.len().saturating_sub(n_start)
}

fn mv_name(m: &Move) -> String {
    m.name().to_string()
}
