//! Domain algebra (reasoning/domain.py)
use super::relations::*;
use super::tags::*;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Domain {
    pub tags: TagSet,
    pub relations: Vec<(Relation, Domain)>,
}

pub type RelDom = (Relation, Domain);

pub fn reldom_implies(a: &RelDom, b: &RelDom) -> bool {
    a.0.implies(&b.0) && a.1.implies(&b.1)
}

pub fn reldom_compatible(a: &RelDom, b: &RelDom) -> bool {
    let (an, bn) = (a.0.is_neg(), b.0.is_neg());
    match (an, bn) {
        (true, false) => {
            if let Relation::Negated(ar) = &a.0 {
                if b.0.implies(ar) && b.1.intersects(&a.1, false, false) {
                    return false;
                }
            }
        }
        (false, true) => {
            if let Relation::Negated(br) = &b.0 {
                if a.0.implies(br) && a.1.intersects(&b.1, false, false) {
                    return false;
                }
            }
        }
        _ => {}
    }
    true
}

pub fn reldom_satisfies(a: &RelDom, b: &RelDom) -> bool {
    a.0.intersects(&b.0, true) && a.1.satisfies(&b.1)
}

pub fn reldom_intersects(a: &RelDom, b: &RelDom, l: bool, r: bool) -> bool {
    a.0.intersects(&b.0, false) && a.1.intersects(&b.1, l, r)
}

pub fn reldom_intersection(a: &RelDom, b: &RelDom) -> RelDom {
    (a.0.intersection(&b.0), a.1.intersection(&b.1))
}

impl Domain {
    pub fn new(tags: TagSet) -> Domain {
        Domain { tags, relations: vec![] }
    }

    pub fn finalized(&self, check_anyrel: bool, check_variable: bool) -> bool {
        if check_variable && self.tags.pos().any(|t| t.kind() == Kind::Var) {
            return false;
        }
        for (rel, d) in &self.relations {
            if check_anyrel && matches!(rel, Relation::Any) {
                return false;
            }
            if !d.finalized(false, true) {
                return false;
            }
        }
        true
    }

    pub fn implies(&self, other: &Domain) -> bool {
        implies(&self.tags, &other.tags)
            && other.relations.iter().all(|o| self.relations.iter().any(|r| reldom_implies(r, o)))
    }

    pub fn add_relation(&mut self, new_rel: Relation, new_dom: Domain) {
        let mut covered = false;
        let new = (new_rel, new_dom);
        for i in 0..self.relations.len() {
            let er = self.relations[i].clone();
            if new.0.is_neg() {
                continue;
            } else if er.0.is_neg() {
                continue;
            } else if reldom_implies(&er, &new) {
                covered = true;
            } else if reldom_satisfies(&er, &new) || reldom_satisfies(&new, &er) {
                self.relations[i] = reldom_intersection(&new, &er);
                covered = true;
            } else if new.1.intersects(&er.1, false, true) {
                self.relations[i] = (er.0.clone(), er.1.intersection(&new.1));
            }
        }
        if !covered {
            self.relations.push(new);
        }
    }

    pub fn with_relation(&self, rel: Relation, dom: Domain) -> Domain {
        let mut n = self.clone();
        n.add_relation(rel, dom);
        n
    }

    pub fn with_tags(&self, tags: &TagSet) -> Domain {
        let mut n = self.clone();
        n.tags.update(tags);
        n
    }

    pub fn satisfies(&self, other: &Domain) -> bool {
        if !satisfies(&self.tags, &other.tags) {
            return false;
        }
        let bothsat = |a: &RelDom, b: &RelDom| a.0.satisfies(&b.0) && a.1.satisfies(&b.1);
        for orel in &other.relations {
            if let Relation::Negated(n) = &orel.0 {
                let probe: RelDom = ((**n).clone(), orel.1.clone());
                if self.relations.iter().any(|s| bothsat(s, &probe)) {
                    return false;
                }
            } else if !self.relations.iter().any(|s| bothsat(s, orel)) {
                return false;
            }
        }
        true
    }

    pub fn intersects(&self, other: &Domain, require_left: bool, require_right: bool) -> bool {
        let mut all = self.tags.clone();
        all.update(&other.tags);
        if contradiction(&all) {
            return false;
        }
        for a in &self.relations {
            for b in &other.relations {
                if std::ptr::eq(a, b) {
                    continue;
                }
                if !reldom_compatible(a, b) {
                    return false;
                }
            }
        }
        let a_pos: Vec<&RelDom> = self.relations.iter().filter(|r| !r.0.is_neg()).collect();
        let b_pos: Vec<&RelDom> = other.relations.iter().filter(|r| !r.0.is_neg()).collect();
        if require_left {
            if !satisfies(&other.tags, &self.tags) {
                return false;
            }
            for a in &a_pos {
                if !b_pos.iter().any(|b| reldom_intersects(a, b, false, false)) {
                    return false;
                }
            }
        }
        if require_right {
            if !satisfies(&self.tags, &other.tags) {
                return false;
            }
            for b in &b_pos {
                if !a_pos.iter().any(|a| reldom_intersects(a, b, false, false)) {
                    return false;
                }
            }
        }
        true
    }

    pub fn intersection(&self, other: &Domain) -> Domain {
        let newtags = self.tags.union(&other.tags);
        if contradiction(&newtags) {
            panic!("Contradictory tags in Domain::intersection {:?} {:?}", self.tags, other.tags);
        }
        let mut nd = Domain::new(newtags);
        for (r, d) in self.relations.iter().chain(other.relations.iter()) {
            nd.add_relation(r.clone(), d.clone());
        }
        nd
    }

    pub fn traverse<'a>(&'a self, out: &mut Vec<&'a Domain>) {
        out.push(self);
        for (_, d) in &self.relations {
            d.traverse(out);
        }
    }

    pub fn get_objs_named(&self) -> Vec<String> {
        let mut v: Vec<String> = self.tags.pos().filter(|t| t.kind() == Kind::Obj).map(|t| t.name()).collect();
        for (r, d) in &self.relations {
            if r.is_neg() {
                continue;
            }
            v.extend(d.get_objs_named());
        }
        v
    }
}

/// domain_tag_substitute (reasoning/domain_substitute.py)
pub fn domain_tag_substitute(domain: &Domain, vartag: Tag, subst: &Domain) -> (Domain, bool) {
    let mut domain = domain.clone();
    let o_match = domain.tags.contains(vartag);
    let mut rd_sub = vec![];
    let mut any = false;
    for (r, d) in &domain.relations {
        let (d2, m) = domain_tag_substitute(d, vartag, subst);
        any |= m;
        rd_sub.push((r.clone(), d2));
    }
    if !(o_match || any) {
        return (domain, false);
    }
    domain.relations = vec![];
    for (r, d) in rd_sub {
        domain.add_relation(r, d);
    }
    if o_match {
        domain.tags.remove(vartag);
        domain = domain.intersection(subst);
    }
    (domain, true)
}

pub fn substitute_all(dom: &Domain, assignments: &[(Tag, Domain)]) -> Domain {
    let mut d = dom.clone();
    for (v, a) in assignments {
        d = domain_tag_substitute(&d, *v, a).0;
    }
    d
}
