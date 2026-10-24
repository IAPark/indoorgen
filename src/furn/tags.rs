//! Tags (core/tags.py): interned atoms, optionally negated, in small sorted sets.
use crate::dhash::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Kind {
    Sem,
    Sub,
    Gen,
    Var,
    Obj,
}

struct Interner {
    names: Vec<(Kind, String)>,
    map: HashMap<(Kind, String), u32>,
}

fn interner() -> &'static Mutex<Interner> {
    static I: OnceLock<Mutex<Interner>> = OnceLock::new();
    I.get_or_init(|| Mutex::new(Interner { names: vec![], map: HashMap::default() }))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag {
    pub id: u32,
    pub neg: bool,
}

impl std::fmt::Debug for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{:?}:{}", if self.neg { "-" } else { "" }, self.kind(), self.name())
    }
}

impl Tag {
    fn make(kind: Kind, name: &str) -> Tag {
        let mut i = interner().lock().unwrap();
        let key = (kind, name.to_string());
        if let Some(&id) = i.map.get(&key) {
            return Tag { id, neg: false };
        }
        let id = i.names.len() as u32;
        i.names.push(key.clone());
        i.map.insert(key, id);
        Tag { id, neg: false }
    }
    pub fn sem(n: &str) -> Tag {
        Self::make(Kind::Sem, n)
    }
    pub fn sub(n: &str) -> Tag {
        Self::make(Kind::Sub, n)
    }
    pub fn gen(n: &str) -> Tag {
        Self::make(Kind::Gen, n)
    }
    pub fn var(n: &str) -> Tag {
        Self::make(Kind::Var, n)
    }
    pub fn obj(n: &str) -> Tag {
        Self::make(Kind::Obj, n)
    }
    pub fn kind(&self) -> Kind {
        interner().lock().unwrap().names[self.id as usize].0
    }
    pub fn name(&self) -> String {
        interner().lock().unwrap().names[self.id as usize].1.clone()
    }
    pub fn negated(self) -> Tag {
        assert!(!self.neg);
        Tag { id: self.id, neg: true }
    }
    pub fn positive(self) -> Tag {
        Tag { id: self.id, neg: false }
    }
}

/// sorted, deduplicated
#[derive(Clone, PartialEq, Eq, Hash, Default, Debug, PartialOrd, Ord)]
pub struct TagSet(pub Vec<Tag>);

impl TagSet {
    pub fn new() -> Self {
        TagSet(vec![])
    }
    pub fn from(it: impl IntoIterator<Item = Tag>) -> Self {
        let mut v: Vec<Tag> = it.into_iter().collect();
        v.sort();
        v.dedup();
        TagSet(v)
    }
    pub fn insert(&mut self, t: Tag) {
        if let Err(p) = self.0.binary_search(&t) {
            self.0.insert(p, t);
        }
    }
    pub fn remove(&mut self, t: Tag) {
        if let Ok(p) = self.0.binary_search(&t) {
            self.0.remove(p);
        }
    }
    pub fn contains(&self, t: Tag) -> bool {
        self.0.binary_search(&t).is_ok()
    }
    pub fn update(&mut self, o: &TagSet) {
        for &t in &o.0 {
            self.insert(t);
        }
    }
    pub fn union(&self, o: &TagSet) -> TagSet {
        let mut r = self.clone();
        r.update(o);
        r
    }
    pub fn pos(&self) -> impl Iterator<Item = Tag> + '_ {
        self.0.iter().copied().filter(|t| !t.neg)
    }
    pub fn neg(&self) -> impl Iterator<Item = Tag> + '_ {
        self.0.iter().copied().filter(|t| t.neg).map(|t| t.positive())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

fn has(s: &TagSet, t: Tag) -> bool {
    s.contains(t)
}

pub fn contradiction(tags: &TagSet) -> bool {
    if tags.neg().any(|n| has(tags, n)) {
        return true;
    }
    if tags.pos().filter(|t| t.kind() == Kind::Gen).count() > 1 {
        return true;
    }
    if tags.pos().filter(|t| matches!(t.kind(), Kind::Obj | Kind::Var)).count() > 1 {
        return true;
    }
    false
}

/// p1 ⊇ p2 and n1 ⊇ n2, and t1 not contradictory
pub fn implies(t1: &TagSet, t2: &TagSet) -> bool {
    !contradiction(t1) && t2.pos().all(|p| has(t1, p)) && t2.neg().all(|n| has(t1, n.negated()))
}

pub fn satisfies(t1: &TagSet, t2: &TagSet) -> bool {
    // p1 ⊇ p2, n1 ∩ p2 = ∅, n2 ∩ p1 = ∅
    t2.pos().all(|p| has(t1, p))
        && !t2.pos().any(|p| has(t1, p.negated()))
        && !t2.neg().any(|n| has(t1, n))
}

pub fn difference(t1: &TagSet, t2: &TagSet) -> TagSet {
    let (p1, n1): (Vec<Tag>, Vec<Tag>) = (t1.pos().collect(), t1.neg().collect());
    let (p2, n2): (Vec<Tag>, Vec<Tag>) = (t2.pos().collect(), t2.neg().collect());
    let mut out = TagSet::new();
    for &p in &p1 {
        out.insert(p);
    }
    for &n in &n2 {
        if !n1.contains(&n) {
            out.insert(n);
        }
    }
    for &n in &n1 {
        out.insert(n.negated());
    }
    for &p in &p2 {
        if !p1.contains(&p) {
            out.insert(p.negated());
        }
    }
    out
}
