//! Room-phase state: polygons, shared-edge relations and the room graph.
use crate::shp::*;
use indexmap::IndexMap;
use std::collections::BTreeSet;

pub fn room_type(name: &str) -> &str {
    name.split('_').next().unwrap()
}
pub fn room_level(name: &str) -> usize {
    name.split('/').next().unwrap().split('_').nth(1).unwrap().parse().unwrap()
}
pub fn room_name(t: &str, level: usize, n: usize) -> String {
    format!("{t}_{level}/{n}")
}

pub const EXTERIOR: &str = "exterior";
pub const ENTRANCE: &str = "entrance";
pub const STAIRCASE: &str = "staircase";
pub const STAIRCASE_ROOM: &str = "staircase-room";

#[derive(Clone)]
pub struct Rel {
    pub target: String,
    pub value: G,
}

#[derive(Clone)]
pub struct RoomObj {
    pub polygon: Option<G>,
    pub relations: Vec<Rel>,
}

#[derive(Clone, Debug)]
pub struct RoomGraph {
    pub names: Vec<String>,
    pub ns: Vec<Vec<usize>>,
    pub entrance: Option<usize>,
    pub invalid: BTreeSet<usize>,
}

impl RoomGraph {
    pub fn new(children: Vec<Vec<usize>>, names: Vec<String>, entrance: Option<usize>) -> Self {
        let mut ns: Vec<Vec<usize>> = vec![vec![]; children.len()];
        for (i, cs) in children.iter().enumerate() {
            for &c in cs {
                if !ns[i].contains(&c) {
                    ns[i].push(c);
                }
                if !ns[c].contains(&i) {
                    ns[c].push(i);
                }
            }
        }
        let invalid = names
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(room_type(n), EXTERIOR | ENTRANCE))
            .map(|(i, _)| i)
            .collect();
        RoomGraph { names, ns, entrance, invalid }
    }
    /// python `len(graph)`
    pub fn len(&self) -> usize {
        self.names.len() - 1
    }
    pub fn indices_of_type(&self, t: &str) -> Vec<usize> {
        self.names.iter().enumerate().filter(|(_, r)| room_type(r) == t).map(|(i, _)| i).collect()
    }
    pub fn root(&self) -> &str {
        match self.entrance {
            None => &self.names[self.indices_of_type(STAIRCASE_ROOM)[0]],
            Some(e) => &self.names[e],
        }
    }
    pub fn neighbours(&self, name: &str) -> BTreeSet<&str> {
        let i = self.names.iter().position(|n| n == name).unwrap();
        self.ns[i].iter().map(|&j| self.names[j].as_str()).collect()
    }
    pub fn valid_ns(&self) -> Vec<(usize, BTreeSet<usize>)> {
        (0..self.names.len())
            .filter(|i| !self.invalid.contains(i))
            .map(|i| (i, self.ns[i].iter().copied().filter(|j| !self.invalid.contains(j)).collect()))
            .collect()
    }
    pub fn valid_neighbours(&self, name: &str) -> BTreeSet<&str> {
        let i = self.names.iter().position(|n| n == name).unwrap();
        self.ns[i]
            .iter()
            .filter(|j| !self.invalid.contains(j))
            .map(|&j| self.names[j].as_str())
            .collect()
    }
}

#[derive(Clone, Default)]
pub struct RoomState {
    pub objs: IndexMap<String, RoomObj>,
    pub graphs: Vec<RoomGraph>,
}

impl RoomState {
    pub fn poly(&self, k: &str) -> &G {
        self.objs[k].polygon.as_ref().unwrap()
    }
    pub fn valid_rooms(&self) -> impl Iterator<Item = (&String, &RoomObj)> {
        self.objs.iter().filter(|(k, _)| !matches!(room_type(k), EXTERIOR | STAIRCASE))
    }
}

// ---- room/utils.py ----

pub fn shared(s: &G, t: &G) -> G {
    let (forward, backward) = shared_paths(&boundary(s), &boundary(t));
    if length(&forward) > 0.0 {
        forward
    } else if length(&backward) > 0.0 {
        backward
    } else {
        empty_mls()
    }
}

pub fn update_shared(state: &mut RoomState, i: &str) {
    let pi = state.poly(i).clone();
    let targets: Vec<String> = state.objs[i].relations.iter().map(|r| r.target.clone()).collect();
    let vals: Vec<G> = targets.iter().map(|t| shared(&pi, state.poly(t))).collect();
    for (r, v) in state.objs[i].relations.iter_mut().zip(vals) {
        r.value = v;
    }
    let lvl = room_level(i);
    let others: Vec<String> = state
        .valid_rooms()
        .filter(|(k, _)| room_level(k) == lvl && k.as_str() != i)
        .map(|(k, _)| k.clone())
        .collect();
    for k in others {
        let v = shared(state.poly(&k), &pi);
        let o = state.objs.get_mut(&k).unwrap();
        let r = o.relations.iter_mut().find(|r| r.target == i).expect("relation");
        r.value = v;
    }
}

pub fn update_contour(state_: &mut RoomState, state: &RoomState, indices: &[String]) {
    let i = &indices[0];
    let exterior_n = room_name(EXTERIOR, room_level(i), 0);
    let minus = union_all(&indices.iter().map(|k| state.poly(k).clone()).collect::<Vec<_>>());
    let plus = union_all(&indices.iter().map(|k| state_.poly(k).clone()).collect::<Vec<_>>());
    let np = union(&difference(state.poly(&exterior_n), &minus), &plus);
    state_.objs.get_mut(&exterior_n).unwrap().polygon = Some(np);
}

pub fn update_exterior(state: &mut RoomState, i: &str) {
    let exterior_n = room_name(EXTERIOR, room_level(i), 0);
    let mut v = linestring(&exterior(state.poly(i))).unwrap();
    for q in &state.objs[i].relations {
        if length(&q.value) > 1e-6 {
            v = difference(&v, &q.value);
        }
    }
    let val = if matches!(gtype(&v), geos::GeometryTypes::MultiLineString) {
        v
    } else if length(&v) > 0.0 {
        multilinestring(vec![v]).unwrap()
    } else {
        empty_mls()
    };
    let r = state
        .objs
        .get_mut(&exterior_n)
        .unwrap()
        .relations
        .iter_mut()
        .find(|r| r.target == i)
        .expect("exterior relation");
    r.value = val;
}
