//! Relations (constraint_language/relations.py)
use super::tags::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum GeoKind {
    Touching,
    SupportedBy,
    CoPlanar,
    StableAgainst,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Geo {
    pub kind: GeoKind,
    pub child: TagSet,
    pub parent: TagSet,
    pub margin: f64,
    pub check_z: bool,
    pub rev_normal: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Relation {
    Any,
    Negated(Box<Relation>),
    RoomNeighbour(Vec<String>),
    Geometry(Geo),
    CutFrom,
    SharedEdge,
    Traverse,
}

impl Relation {
    pub fn neg(self) -> Relation {
        match self {
            Relation::Negated(r) => *r,
            r => Relation::Negated(Box::new(r)),
        }
    }
    pub fn is_neg(&self) -> bool {
        matches!(self, Relation::Negated(_))
    }
    pub fn geo(&self) -> Option<&Geo> {
        match self {
            Relation::Geometry(g) => Some(g),
            _ => None,
        }
    }

    fn ident_class(&self) -> Option<u8> {
        match self {
            Relation::CutFrom => Some(0),
            Relation::SharedEdge => Some(1),
            Relation::Traverse => Some(2),
            _ => None,
        }
    }

    pub fn implies(&self, other: &Relation) -> bool {
        match self {
            Relation::Any => matches!(other, Relation::Any),
            Relation::Negated(s) => match other {
                Relation::Any => false,
                Relation::Negated(o) => s.implies(o),
                _ => !s.implies(other) && !self.intersects(other, true),
            },
            Relation::RoomNeighbour(a) => match other {
                Relation::Any => true,
                Relation::RoomNeighbour(b) => b.iter().all(|x| a.contains(x)),
                _ => false,
            },
            Relation::Geometry(g) => g.implies(other),
            _ => matches!(other, Relation::Any) || self.ident_class() == other.ident_class(),
        }
    }

    pub fn satisfies(&self, other: &Relation) -> bool {
        match self {
            Relation::Any => matches!(other, Relation::Any),
            Relation::Negated(s) => match other {
                Relation::Any => false,
                Relation::Negated(o) => s.satisfies(o),
                _ => !s.satisfies(other) && !self.intersects(other, true),
            },
            Relation::RoomNeighbour(_) => self.implies(other),
            Relation::Geometry(g) => g.satisfies(other),
            _ => self.implies(other),
        }
    }

    pub fn intersects(&self, other: &Relation, strict: bool) -> bool {
        match self {
            Relation::Any => true,
            Relation::Negated(s) => match other {
                Relation::Negated(o) => {
                    if matches!(**s, Relation::Any) || matches!(**o, Relation::Any) {
                        false
                    } else {
                        true
                    }
                }
                _ => other.intersects(self, strict),
            },
            Relation::RoomNeighbour(a) => match other {
                Relation::Any => true,
                Relation::RoomNeighbour(b) => a.iter().any(|x| b.contains(x)),
                _ => false,
            },
            Relation::Geometry(g) => g.intersects(other, strict),
            _ => matches!(other, Relation::Any) || self.ident_class() == other.ident_class(),
        }
    }

    pub fn intersection(&self, other: &Relation) -> Relation {
        match self {
            Relation::Any => other.clone(),
            Relation::Negated(s) => s.difference(other),
            Relation::RoomNeighbour(a) => match other {
                Relation::Any => self.clone(),
                Relation::RoomNeighbour(b) => Relation::RoomNeighbour(a.iter().filter(|x| b.contains(x)).cloned().collect()),
                _ => Relation::RoomNeighbour(vec![]),
            },
            Relation::Geometry(g) => g.intersection(other),
            _ => self.clone(),
        }
    }

    pub fn difference(&self, other: &Relation) -> Relation {
        match self {
            Relation::Any => other.clone().neg(),
            Relation::Negated(s) => s.intersection(other),
            Relation::RoomNeighbour(a) => match other {
                Relation::Any => Relation::Any.neg(),
                Relation::RoomNeighbour(b) => Relation::RoomNeighbour(a.iter().filter(|x| !b.contains(x)).cloned().collect()),
                _ => self.clone(),
            },
            Relation::Geometry(g) => g.difference(other),
            _ => Relation::Any.neg(),
        }
    }
}

impl Geo {
    fn compat(&self, other: &Geo) -> bool {
        // issubclass(other.__class__, self.__class__)
        match (self.kind, other.kind) {
            (a, b) if a == b => true,
            (GeoKind::Touching, GeoKind::SupportedBy) => true,
            _ => false,
        }
    }

    fn implies(&self, other: &Relation) -> bool {
        match other {
            Relation::Any => true,
            Relation::Negated(o) => match &**o {
                Relation::Any => false,
                Relation::Geometry(og) => {
                    if !self.compat(og) {
                        return false;
                    }
                    let nc = TagSet::from(og.child.0.iter().map(|t| flip(*t)));
                    let np = TagSet::from(og.parent.0.iter().map(|t| flip(*t)));
                    implies(&self.child, &nc) && implies(&self.parent, &np)
                }
                _ => panic!("Geo.implies unhandled {other:?}"),
            },
            Relation::Geometry(og) => self.compat(og) && implies(&self.child, &og.child) && implies(&self.parent, &og.parent),
            _ => panic!("Geo.implies unhandled {other:?}"),
        }
    }

    fn satisfies(&self, other: &Relation) -> bool {
        match other {
            Relation::Any => true,
            Relation::Negated(o) => match &**o {
                Relation::Any => false,
                Relation::Geometry(og) => {
                    if !self.compat(og) {
                        return false;
                    }
                    let nc = TagSet::from(og.child.0.iter().map(|t| flip(*t)));
                    let np = TagSet::from(og.parent.0.iter().map(|t| flip(*t)));
                    satisfies(&self.child, &nc) && satisfies(&self.parent, &np)
                }
                _ => panic!("Geo.satisfies unhandled"),
            },
            Relation::Geometry(og) => self.compat(og) && satisfies(&self.child, &og.child) && satisfies(&self.parent, &og.parent),
            _ => panic!("Geo.satisfies unhandled {other:?}"),
        }
    }

    fn intersects(&self, other: &Relation, strict: bool) -> bool {
        let compat_tags = |a: &TagSet, b: &TagSet| {
            if strict {
                implies(a, b) || implies(b, a)
            } else {
                !contradiction(&a.union(b))
            }
        };
        match other {
            Relation::Any => true,
            Relation::Negated(o) => match &**o {
                Relation::Any => false,
                Relation::Geometry(og) => !Relation::Geometry(og.clone()).implies(&Relation::Geometry(self.clone())),
                _ => false,
            },
            Relation::Geometry(og) => self.compat(og) && compat_tags(&self.child, &og.child) && compat_tags(&self.parent, &og.parent),
            _ => false,
        }
    }

    fn with_tags(&self, child: TagSet, parent: TagSet) -> Relation {
        Relation::Geometry(Geo { child, parent, ..self.clone() })
    }

    fn intersection(&self, other: &Relation) -> Relation {
        match other {
            Relation::Any => Relation::Geometry(self.clone()),
            Relation::Negated(r) => Relation::Geometry(self.clone()).difference(r),
            Relation::Geometry(og) => {
                if !self.compat(og) {
                    return Relation::Any.neg();
                }
                self.with_tags(self.child.union(&og.child), self.parent.union(&og.parent))
            }
            _ => Relation::Any.neg(),
        }
    }

    fn difference(&self, other: &Relation) -> Relation {
        match other {
            Relation::Any => Relation::Any.neg(),
            Relation::Negated(r) => Relation::Geometry(self.clone()).intersection(r),
            Relation::Geometry(og) => {
                if !self.intersects(other, false) {
                    return Relation::Geometry(self.clone());
                }
                if implies(&self.child, &og.child) && implies(&self.parent, &og.parent) {
                    return Relation::Any.neg();
                }
                self.with_tags(difference(&self.child, &og.child), difference(&self.parent, &og.parent))
            }
            _ => Relation::Any.neg(),
        }
    }
}

fn flip(t: Tag) -> Tag {
    Tag { id: t.id, neg: !t.neg }
}
