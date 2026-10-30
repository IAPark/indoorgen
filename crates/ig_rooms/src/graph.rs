//! GraphMaker (room/graph.py): samples a room-adjacency graph from the "node_gen" grammar.
//! The constraint DSL is hand-expanded: node_gen is a sum of NLL terms of per-parent child counts.
use super::consts::*;
use super::score::room_term;
use super::state::*;
use crate::rng::Rng;
use crate::shp::*;
use indexmap::IndexMap;
use crate::dhash::HashMap;

#[derive(Clone)]
struct GNode {
    rels: Vec<String>,
    visited: bool,
    new: bool,
}

type GState = IndexMap<String, GNode>;

/// (parent type, child type, low, high, mean); parent "closet@bedroom" = closets related to a bedroom
pub struct Rule {
    parent: &'static str,
    child: &'static str,
    low: usize,
    high: usize,
    mean: f64,
}

const fn rule(parent: &'static str, child: &'static str, low: usize, high: usize, mean: f64) -> Rule {
    Rule { parent, child, low, high, mean }
}

/// which room grammar a house samples
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomSet {
    /// Infinigen `fewer_rooms`: living + dining + kitchen + bathroom + 1-2 bedrooms, all off the living room (no hallways)
    Fewer,
    /// Infinigen's full grammar (hallways, closets, utility rooms, garages, balconies; loose counts)
    Full,
    /// real-home program: a hallway serving the bedrooms and the bathroom in most houses, 1-3 bedrooms, bathrooms <= bedrooms + 1,
    /// optional en-suite / walk-in closet / utility room, dining room optional (kitchen off the living room); no garage, balcony
    Curated,
}

const CURATED_TYPES: [&str; 8] = ["bathroom", "bedroom", "closet", "dining-room", "hallway", "kitchen", "living-room", "utility"];

pub fn rules(set: RoomSet) -> Vec<Rule> {
    if set == RoomSet::Curated {
        return vec![
            rule("root", "living-room", 1, 1, 1.0),
            rule("living-room", "entrance", 0, 1, 0.8),
            rule("living-room", "hallway", 0, 1, 0.7),
            rule("living-room", "bedroom", 0, 2, 0.5),
            rule("living-room", "bathroom", 0, 1, 0.15),
            rule("living-room", "dining-room", 0, 1, 0.6),
            rule("living-room", "kitchen", 0, 1, 0.3),
            rule("dining-room", "kitchen", 0, 1, 0.9),
            rule("hallway", "entrance", 0, 1, 0.3),
            rule("hallway", "bedroom", 1, 3, 1.8),
            rule("hallway", "bathroom", 0, 1, 0.85),
            rule("hallway", "closet", 0, 1, 0.2),
            rule("bedroom", "bathroom", 0, 1, 0.25),
            rule("bedroom", "closet", 0, 1, 0.25),
            rule("kitchen", "utility", 0, 1, 0.2),
        ];
    }
    if set == RoomSet::Fewer {
        vec![
            rule("root", "living-room", 1, 2, 1.1),
            rule("living-room", "bedroom", 1, 2, 1.2),
            rule("living-room", "entrance", 1, 1, 1.0),
            rule("living-room", "bathroom", 1, 1, 1.0),
            rule("living-room", "dining-room", 1, 1, 1.0),
            rule("dining-room", "kitchen", 1, 1, 1.0),
        ]
    } else {
        vec![
            rule("root", "living-room", 1, 2, 1.1),
            rule("living-room", "hallway", 0, 2, 1.2),
            rule("living-room", "bedroom", 0, 2, 1.2),
            rule("living-room", "closet", 0, 1, 0.2),
            rule("living-room", "bathroom", 0, 1, 0.4),
            rule("living-room", "balcony", 0, 1, 0.5),
            rule("living-room", "dining-room", 0, 1, 0.8),
            rule("living-room", "utility", 0, 1, 0.2),
            rule("living-room", "staircase-room", 0, 1, 0.5),
            rule("hallway", "bedroom", 1, 2, 1.5),
            rule("hallway", "closet", 0, 1, 0.2),
            rule("hallway", "bathroom", 0, 1, 0.6),
            rule("hallway", "garage", 0, 1, 0.5),
            rule("hallway", "utility", 0, 1, 0.2),
            rule("hallway", "staircase-room", 0, 1, 0.4),
            rule("utility", "garage", 0, 1, 0.2),
            rule("kitchen", "utility", 0, 1, 0.2),
            rule("bedroom", "bathroom", 0, 1, 0.6),
            rule("bedroom", "closet", 0, 1, 0.6),
            rule("closet@bedroom", "bathroom", 0, 1, 0.5),
            rule("dining-room", "kitchen", 1, 1, 1.0),
            rule("balcony", "exterior", 1, 1, 1.0),
            rule("garage", "exterior", 1, 1, 1.0),
            rule("hallway", "entrance", 0, 1, 0.8),
            rule("living-room", "entrance", 0, 1, 0.8),
        ]
    }
}

fn binom(n: usize, k: usize) -> f64 {
    let mut r = 1.0;
    for i in 0..k {
        r = r * (n - i) as f64 / (i + 1) as f64;
    }
    r
}

fn rule_probs(r: &Rule) -> Vec<f64> {
    let size = r.high - r.low;
    let mut v = vec![0.0; r.low];
    if size > 0 {
        let p = (r.mean - r.low as f64) / size as f64;
        for i in 0..=size {
            v.push(p.powi(i as i32) * (1.0 - p).powi((size - i) as i32) * binom(size, i));
        }
    } else {
        v.push(1.0);
    }
    v
}

fn count_related(st: &GState, parent: &str, child_type: &str) -> usize {
    st.iter().filter(|(k, n)| room_type(k) == child_type && n.rels.iter().any(|t| t == parent)).count()
}

fn nll(count: usize, args: &[f64]) -> f64 {
    if count < args.len() {
        -(args[count] + 1e-20).ln()
    } else {
        -(1e-100f64).ln()
    }
}

pub struct GraphMaker {
    pub level: usize,
    pub c: RoomConstants,
    pub typical_areas: HashMap<String, f64>,
    pub slackness: f64,
    rules: Vec<(Rule, Vec<f64>)>,
    set: RoomSet,
    room_types: Vec<&'static str>,
}

pub const ROOM_TYPES_SORTED: [&str; 11] = [
    "balcony", "bathroom", "bedroom", "closet", "dining-room", "garage", "hallway", "kitchen", "living-room",
    "staircase-room", "utility",
];

impl GraphMaker {
    /// `factory_seed` stream: python draws `slackness` as the first value of FixedSeed(factory_seed).
    pub fn new(factory_seed: u64, c: &RoomConstants, level: usize, set: RoomSet) -> Self {
        let typical_areas = typical_areas(c, level);
        let mut rng = Rng::seeded(factory_seed);
        let slackness = rng.log_uniform(1.1, 1.3);
        let rs = rules(set);
        let rules = rs
            .into_iter()
            .map(|r| {
                let p = rule_probs(&r);
                (r, p)
            })
            .collect();
        GraphMaker {
            level,
            c: c.clone(),
            typical_areas,
            slackness,
            rules,
            set,
            room_types: if set == RoomSet::Curated { CURATED_TYPES.to_vec() } else { ROOM_TYPES_SORTED.to_vec() },
        }
    }

    fn score(&self, st: &GState) -> f64 {
        let mut total = 0.0;
        for (r, probs) in &self.rules {
            // parents: non-New nodes of the parent type
            if let Some(bt) = r.parent.strip_suffix("@bedroom") {
                for (k, n) in st.iter() {
                    if n.new || room_type(k) != bt {
                        continue;
                    }
                    let near_bedroom =
                        n.rels.iter().any(|t| room_type(t) == "bedroom" && st.get(t).map_or(false, |_| true));
                    if !near_bedroom {
                        continue;
                    }
                    total += nll(count_related(st, k, r.child), probs);
                }
            } else {
                for (k, n) in st.iter() {
                    if n.new || room_type(k) != r.parent {
                        continue;
                    }
                    total += nll(count_related(st, k, r.child), probs);
                }
            }
        }
        total
    }

    /// Hard "node" constraint violation count.
    fn violations(&self, st: &GState) -> f64 {
        let cnt = |t: &str| st.keys().filter(|k| room_type(k) == t).count() as f64;
        let ge = |v: f64, lo: f64| (lo - v).max(0.0);
        if self.set == RoomSet::Fewer {
            return ge(cnt("entrance"), 1.0)
                + (cnt("staircase-room") - 0.0).abs()
                + ge(cnt("living-room"), 1.0)
                + ge(cnt("kitchen"), 1.0)
                + ge(cnt("bedroom"), 1.0)
                + ge(cnt("bathroom"), 1.0);
        }
        let in_range = |v: f64, lo: f64, hi: f64| if v < lo { lo - v } else if v > hi { v - hi } else { 0.0 };
        let mut viol = 0.0;
        if self.set == RoomSet::Curated {
            for (t, lo, hi) in [("living-room", 1.0, 1.0), ("kitchen", 1.0, 1.0), ("entrance", 1.0, 1.0), ("dining-room", 0.0, 1.0), ("hallway", 0.0, 1.0), ("utility", 0.0, 1.0), ("closet", 0.0, 2.0), ("bedroom", 1.0, 3.0), ("staircase-room", 0.0, 0.0)] {
                viol += in_range(cnt(t), lo, hi);
            }
            viol += in_range(cnt("bathroom"), 1.0, cnt("bedroom") + 1.0);
        } else {
            let n_total = st.keys().filter(|k| !matches!(room_type(k), EXTERIOR | ENTRANCE)).count() as f64;
            let rg_count = st.len() as f64;
            let or0 = |a: f64| a.min(rg_count.abs());
            viol += in_range(n_total, 4.0, 15.0);
            viol += or0(ge(cnt("living-room"), 1.0));
            viol += or0(ge(cnt("entrance"), 1.0));
            viol += ge(cnt("bedroom"), 1.0);
            viol += or0(ge(cnt("dining-room"), 1.0));
            viol += or0(ge(cnt("kitchen"), 1.0));
            viol += in_range(cnt("garage"), 0.0, 1.0);
            viol += in_range(cnt("hallway"), 0.0, 3.0);
            viol += cnt("staircase-room").abs();
        }
        let related = |r: &str, t: &str| -> usize { count_related(st, r, t) };
        let related_via = |r: &str, mid: &str, t: &str| -> usize {
            st.iter()
                .filter(|(k, n)| room_type(k) == mid && n.rels.iter().any(|x| x == r))
                .map(|(k, _)| related(k, t))
                .sum::<usize>()
                .min(usize::MAX)
        };
        let _ = related_via;
        for (r, _) in st.iter().filter(|(k, _)| room_type(k) == "bedroom") {
            let pb = related(r, "bathroom") >= 1;
            // bathrooms related to (closets related to r)
            let pbc = st
                .iter()
                .filter(|(k, n)| room_type(k) == "closet" && n.rels.iter().any(|x| x == r))
                .any(|(k, _)| related(k, "bathroom") >= 1);
            let via_hall = st
                .iter()
                .filter(|(k, n)| room_type(k) == "hallway" && n.rels.iter().any(|x| x == r))
                .any(|(k, _)| related(k, "bathroom") >= 1);
            let via_living = st
                .iter()
                .filter(|(k, n)| room_type(k) == "living-room" && n.rels.iter().any(|x| x == r))
                .any(|(k, _)| related(k, "bathroom") >= 1);
            if pb && pbc {
                viol += 1.0;
            }
            if !(pb || pbc || via_hall || via_living) {
                viol += 1.0;
            }
        }
        viol
    }

    fn add_new(st: &mut GState, t: &str, level: usize, count: usize, n: &str, i: usize) {
        for j in 0..i {
            let name = room_name(t, level, count + j);
            st.insert(name.clone(), GNode { rels: vec![n.to_string()], visited: false, new: true });
            st.get_mut(n).unwrap().rels.push(name);
        }
    }

    pub fn make_graph(&self, seed: u64) -> RoomGraph {
        let mut rng = Rng::seeded(seed);
        loop {
            let root = room_name("root", self.level, 0);
            let mut state: GState = IndexMap::new();
            state.insert(root, GNode { rels: vec![], visited: false, new: false });
            for _ in 0..40 {
                let mut unvisited: Vec<String> =
                    state.iter().filter(|(_, n)| !n.visited).map(|(k, _)| k.clone()).collect();
                unvisited.sort();
                if unvisited.is_empty() {
                    break;
                }
                let n = unvisited[rng.randint(0, unvisited.len() as i64) as usize].clone();
                let score = self.score(&state);
                let mut scores = vec![score];
                let mut states = vec![state.clone()];
                let types: Vec<&str> = self.room_types.iter().copied().chain([ENTRANCE, EXTERIOR]).collect();
                for t in types {
                    let count = state.keys().filter(|k| room_type(k) == t).count();
                    for i in 1..3 {
                        let mut st = state.clone();
                        Self::add_new(&mut st, t, self.level, count, &n, i);
                        scores.push(self.score(&st));
                        states.push(st);
                    }
                    let mn = scores.iter().cloned().fold(f64::INFINITY, f64::min);
                    let scores_: Vec<f64> = scores.iter().map(|s| s - mn).collect();
                    let i = if scores_.iter().all(|&s| s < 0.01) {
                        0
                    } else {
                        let p: Vec<f64> = scores_.iter().map(|s| (-s).exp()).collect();
                        let sum: f64 = p.iter().sum();
                        let p: Vec<f64> = p.iter().map(|x| x / sum).collect();
                        rng.choice_p(&p)
                    };
                    state = states.swap_remove(i);
                    scores = vec![scores[i]];
                    states = vec![state.clone()];
                }
                for node in state.values_mut() {
                    node.new = false;
                }
                if room_type(&n) == "root" {
                    state.shift_remove(&n);
                    let first = state.keys().next().unwrap().clone();
                    let others: Vec<String> = state.keys().filter(|k| **k != first).cloned().collect();
                    for (k, node) in state.iter_mut() {
                        if *k == first {
                            node.rels = others.clone();
                        } else {
                            node.rels = vec![first.clone()];
                        }
                    }
                } else {
                    state.get_mut(&n).unwrap().visited = true;
                }
            }
            if self.violations(&state) == 0.0 {
                return self.state2graph(state, &mut rng);
            }
        }
    }

    fn state2graph(&self, state: GState, rng: &mut Rng) -> RoomGraph {
        let ext_name = room_name(EXTERIOR, self.level, 0);
        // merge exterior
        let mut ext_connected: Vec<String> = vec![];
        for (k, n) in &state {
            if room_type(k) == EXTERIOR {
                for t in &n.rels {
                    if !ext_connected.contains(t) {
                        ext_connected.push(t.clone());
                    }
                }
            }
        }
        let mut st: GState = state.into_iter().filter(|(k, _)| room_type(k) != EXTERIOR).collect();
        for k in &ext_connected {
            let n = st.get_mut(k).unwrap();
            n.rels.retain(|t| room_type(t) != EXTERIOR);
            n.rels.push(ext_name.clone());
        }
        st.insert(ext_name.clone(), GNode { rels: ext_connected.clone(), visited: false, new: false });
        // merge entrance
        let mut ent_connected: Vec<String> = vec![];
        for (k, n) in &st {
            if room_type(k) == ENTRANCE {
                for t in &n.rels {
                    if !ent_connected.contains(t) {
                        ent_connected.push(t.clone());
                    }
                }
            }
        }
        let mut st: GState = st.into_iter().filter(|(k, _)| room_type(k) != ENTRANCE).collect();
        for k in &ent_connected {
            st.get_mut(k).unwrap().rels.retain(|t| room_type(t) != ENTRANCE);
        }
        let entrance = if ent_connected.is_empty() {
            None
        } else {
            ent_connected.sort();
            let e = ent_connected[rng.choice_idx(ent_connected.len())].clone();
            st.get_mut(&e).unwrap().rels.push(ext_name.clone());
            st.get_mut(&ext_name).unwrap().rels.push(e.clone());
            Some(e)
        };
        let names: Vec<String> =
            st.keys().filter(|k| room_type(k) != EXTERIOR).cloned().chain([ext_name.clone()]).collect();
        let children: Vec<Vec<usize>> = names
            .iter()
            .map(|n| st[n].rels.iter().map(|r| names.iter().position(|x| x == r).unwrap()).collect())
            .collect();
        let ent_idx = entrance.map(|e| names.iter().position(|x| *x == e).unwrap());
        RoomGraph::new(children, names, ent_idx)
    }

    /// returns (width, height); python `suggest_dimensions(graph, None, None)` for the first story
    pub fn suggest_dimensions(&self, graph: &RoomGraph, rng: &mut Rng, wh: Option<(f64, f64)>) -> (f64, f64) {
        let area: f64 = graph
            .names
            .iter()
            .filter(|r| room_type(r) != EXTERIOR)
            .map(|r| self.typical_areas[room_type(r)])
            .sum::<f64>()
            * self.slackness;
        let aspect = match wh {
            None => {
                let (lo, hi) = (0.7, 1.0);
                rng.uniform(lo, hi)
            }
            Some((w, h)) => w / h,
        };
        (self.c.unit_cast((area * aspect).sqrt()), self.c.unit_cast((area / aspect).sqrt()))
    }
}

/// GraphMaker.get_typical_areas
fn typical_areas(c: &RoomConstants, level: usize) -> HashMap<String, f64> {
    let mut typical: HashMap<String, f64> = HashMap::default();
    let mut undecided = vec![];
    let lengths: Vec<f64> = (0..20)
        .map(|i| (1.5f64.ln() + (25f64.ln() - 1.5f64.ln()) * i as f64 / 19.0).exp())
        .collect();
    for t in ROOM_TYPES_SORTED {
        let name = room_name(t, level, 0);
        let holder = room_name(STAIRCASE, level, 0);
        let ext = room_name(EXTERIOR, level, 0);
        let mut scores = vec![];
        for &l in &lengths {
            let mut state = RoomState::default();
            state.graphs = vec![RoomGraph::new(vec![vec![]], vec![name.clone()], Some(0)); level + 1];
            state.objs.insert(name.clone(), RoomObj { polygon: Some(bbox(0.0, 0.0, l, l)), relations: vec![] });
            state.objs.insert(holder.clone(), RoomObj { polygon: Some(bbox(-l, -l, 0.0, 0.0)), relations: vec![] });
            state.objs.insert(
                ext.clone(),
                RoomObj {
                    polygon: Some(bbox(-l, -l, 0.0, 0.0)),
                    relations: vec![Rel { target: name.clone(), value: empty_mls() }],
                },
            );
            update_exterior(&mut state, &name);
            scores.push(room_term(&state, c));
        }
        let mn = scores.iter().cloned().fold(f64::INFINITY, f64::min);
        let sel: Vec<bool> = scores.iter().map(|s| s - mn < 1.0).collect();
        let nsel = sel.iter().filter(|&&b| b).count();
        if nsel as f64 > sel.len() as f64 / 2.0 {
            undecided.push(t);
        } else {
            let ml: f64 = lengths.iter().zip(&sel).filter(|(_, &b)| b).map(|(l, _)| l.ln()).sum::<f64>() / nsel as f64;
            typical.insert(t.to_string(), ml.exp().powi(2));
        }
    }
    let m = if typical.is_empty() { 10.0 } else { typical.values().sum::<f64>() / typical.len() as f64 };
    for t in undecided {
        typical.insert(t.to_string(), m);
    }
    // types outside room_types (entrance etc.) never appear in graph names after merging
    typical
}
