//! SegmentMaker (room/segment.py): split the contour into boxes, merge down to #rooms, assign rooms.
use super::consts::*;
use super::state::*;
use crate::pyset::PySet;
use crate::rng::Rng;
use crate::shp::*;
use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};

pub struct SegmentMaker {
    pub c: RoomConstants,
    pub level: usize,
    pub contour: G,
    pub graph: RoomGraph,
    pub n_boxes: usize,
    box_ratio: f64,
    n_box_trials: usize,
}

type Segments = IndexMap<usize, G>;
type Shared = IndexMap<usize, IndexMap<usize, G>>;

impl SegmentMaker {
    pub fn new(factory_seed: u64, c: &RoomConstants, contour: G, graph: RoomGraph, level: usize) -> Self {
        let mut rng = Rng::seeded(factory_seed);
        let n_boxes = (graph.len() as f64 * rng.uniform(1.8, 2.0)) as usize;
        SegmentMaker { c: c.clone(), level, contour, graph, n_boxes, box_ratio: 0.15, n_box_trials: 100 }
    }

    fn divide_segments(&self, rng: &mut Rng) -> Ni<Segments> {
        let mut segments: Segments = IndexMap::new();
        segments.insert(0, self.contour.clone());
        for _ in 0..self.n_boxes {
            let keys: Vec<usize> = segments.keys().copied().collect();
            let prob: Vec<f64> = segments.values().map(|v| area(v).sqrt()).collect();
            for _ in 0..self.n_box_trials {
                let k = keys[rng.choice_p(&prob)];
                let (x, y, xx, yy) = bounds(&segments[&k]);
                let (w, h) = (xx - x, yy - y);
                let r = rng.uniform(0.25, 0.75);
                let mut line: Option<G> = None;
                if w >= h {
                    let w_ = self.c.unit_cast(r * w);
                    let bound = (self.box_ratio * h).max(self.c.segment_margin);
                    if w_ >= bound && w - w_ >= bound {
                        line = Some(linestring(&[[x + w_, -100.0], [x + w_, 100.0]]).unwrap());
                    }
                } else {
                    let h_ = self.c.unit_cast(r * h);
                    let bound = (self.box_ratio * w).max(self.c.segment_margin);
                    if h_ >= bound && h - h_ >= bound {
                        line = Some(linestring(&[[-100.0, y + h_], [100.0, y + h_]]).unwrap());
                    }
                }
                if let Some(line) = line {
                    let i = *segments.keys().max().unwrap();
                    let cut = cut_polygon_by_line(&segments[&k], &line);
                    if cut.len() != 2 {
                        return Err(NotImpl("cut did not give two polygons"));
                    }
                    let (s, t) = (&cut[0], &cut[1]);
                    let s_ = self.c.canonicalize(s)?;
                    let t_ = self.c.canonicalize(t)?;
                    if (area(s) - area(&s_)).abs() < 1e-3 && (area(t) - area(&t_)).abs() < 1e-3 {
                        segments.insert(k, s_);
                        segments.insert(i + 1, t_);
                        break;
                    }
                }
            }
        }
        Ok(segments)
    }

    fn merge_segment(
        &self,
        segments: &mut Segments,
        shared_edges: &mut Shared,
        attached: &mut BTreeMap<usize, BTreeSet<usize>>,
        i: usize,
        j: usize,
    ) -> Ni<()> {
        assert!(i != j);
        let s = self.c.canonicalize(&union(&segments[&i], &segments[&j]))?;
        if !is_valid_polygon(&s) {
            return Ok(());
        }
        segments.insert(j, s);
        segments.shift_remove(&i);
        shared_edges.shift_remove(&i);
        attached.remove(&i);
        for (_, ses) in shared_edges.iter_mut() {
            ses.shift_remove(&i);
        }
        for (_, ats) in attached.iter_mut() {
            ats.remove(&i);
        }
        let keys: Vec<usize> = segments.keys().copied().collect();
        for &k in &keys {
            for &l in &keys {
                if k != l && (k == j || l == j) {
                    let se = shared(&segments[&k], &segments[&l]);
                    let long = length(&se) >= self.c.segment_margin;
                    shared_edges.get_mut(&k).unwrap().insert(l, se);
                    if long {
                        attached.entry(k).or_default().insert(l);
                        attached.entry(l).or_default().insert(k);
                    }
                }
            }
        }
        Ok(())
    }

    fn filter_segments(&self, rng: &mut Rng) -> Ni<(Segments, Shared)> {
        let mut segments = self.divide_segments(rng)?;
        let mut shared_edges: Shared = IndexMap::new();
        let mut attached: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let keys: Vec<usize> = segments.keys().copied().collect();
        for &k in &keys {
            for &l in &keys {
                if k < l {
                    let se = shared(&segments[&k], &segments[&l]);
                    let long = length(&se) >= self.c.segment_margin;
                    shared_edges.entry(k).or_default().insert(l, se.clone());
                    shared_edges.entry(l).or_default().insert(k, se);
                    if long {
                        attached.entry(k).or_default().insert(l);
                        attached.entry(l).or_default().insert(k);
                    }
                }
            }
        }
        while segments.len() > self.graph.len() {
            let ks: Vec<usize> = shared_edges.keys().copied().collect();
            let prob: Vec<f64> = ks
                .iter()
                .map(|c| 1.0 / (attached.get(c).map_or(0, |s| s.len()) as f64 + 1.0))
                .collect();
            let k = ks[rng.choice_p(&prob)];
            let ses: Vec<(usize, G)> = shared_edges[&k].iter().map(|(a, b)| (*a, b.clone())).collect();
            let candidates = self.c.filter(&ses, Some(1e-6));
            if candidates.is_empty() {
                return Err(NotImpl("no merge candidates"));
            }
            let empty = BTreeSet::new();
            let ak = attached.get(&k).unwrap_or(&empty).clone();
            let prob: Vec<f64> = candidates
                .iter()
                .map(|c| {
                    let ac = attached.get(c).unwrap_or(&empty);
                    (ac.difference(&ak).count() as f64).powi(2) + 0.5
                })
                .collect();
            let n = candidates[rng.choice_p(&prob)];
            self.merge_segment(&mut segments, &mut shared_edges, &mut attached, k, n)?;
        }
        Ok((segments, shared_edges))
    }

    /// `rng` is the caller's stream (stream A); returns None if no valid assignment.
    pub fn build_segments(&self, rng: &mut Rng, placeholder: Option<&G>) -> Option<RoomState> {
        let mut seed = rng.randint(0, 100_000_000) as u64;
        let (segments, shared_edges) = loop {
            let mut sub = Rng::seeded(seed);
            if let Ok(r) = self.filter_segments(&mut sub) {
                break r;
            }
            seed += 1;
        };
        let c = &self.c;
        let neighbours_all: IndexMap<usize, PySetLike> = shared_edges
            .iter()
            .map(|(k, se)| {
                let ses: Vec<(usize, G)> = se.iter().map(|(a, b)| (*a, b.clone())).collect();
                (*k, PySetLike(c.filter(&ses, None).into_iter().collect()))
            })
            .collect();
        let mut exterior_edges: IndexMap<usize, G> = IndexMap::new();
        let mut exterior_neighbours: Vec<u64> = vec![];
        for (k, s) in &segments {
            let mut l = boundary(s);
            for ls in shared_edges[k].values() {
                l = difference(&l, ls);
            }
            if length(&l) > 0.0 {
                let e = if matches!(gtype(&l), geos::GeometryTypes::LineString | geos::GeometryTypes::LinearRing) {
                    multilinestring(vec![l.clone()]).unwrap()
                } else {
                    l.clone()
                };
                exterior_edges.insert(*k, e);
            } else {
                exterior_edges.insert(*k, empty_mls());
            }
            if segment_filter(&l, c.segment_margin) {
                exterior_neighbours.push(*k as u64);
            }
        }
        let mut staircase_candidates: Vec<u64> = vec![];
        if let Some(ph) = placeholder {
            for (k, s) in &segments {
                if area(&intersection(s, ph)) / area(ph) > c.staircase_thresh {
                    staircase_candidates.push(*k as u64);
                }
            }
            if staircase_candidates.is_empty() {
                return None;
            }
        }
        let ext_idx = self.graph.names.iter().position(|n| *n == room_name(EXTERIOR, self.level, 0)).unwrap();
        let exterior_rooms: Vec<usize> = self.graph.ns[ext_idx].clone();
        let mut unassigned = PySet::from_iter(neighbours_all.keys().map(|&k| k as u64));
        let n_graph = self.graph.len();
        let mut assignment = vec![0usize; n_graph];
        let valid_ns: BTreeMap<usize, BTreeSet<usize>> = self.graph.valid_ns().into_iter().collect();
        let stair_rooms = self.graph.indices_of_type(STAIRCASE_ROOM);

        fn assign(
            i: usize,
            n_graph: usize,
            assignment: &mut Vec<usize>,
            unassigned: &mut PySet,
            stair_rooms: &[usize],
            staircase_candidates: &[u64],
            exterior_rooms: &[usize],
            exterior_neighbours: &[u64],
            valid_ns: &BTreeMap<usize, BTreeSet<usize>>,
            neighbours_all: &IndexMap<usize, PySetLike>,
        ) -> bool {
            if i == n_graph {
                return true;
            }
            let candidates = if stair_rooms.contains(&i) {
                unassigned.intersection_list(staircase_candidates)
            } else if exterior_rooms.contains(&i) {
                unassigned.intersection_list(exterior_neighbours)
            } else {
                unassigned.copy()
            };
            let vn = &valid_ns[&i];
            let n_unassigned = vn.iter().filter(|&&j| j > i).count();
            let assigned_neighbours: BTreeSet<usize> = vn.iter().filter(|&&j| j < i).map(|&j| assignment[j]).collect();
            for n in candidates.iter() {
                let n = n as usize;
                let na = &neighbours_all[&n].0;
                if assigned_neighbours.is_subset(na) {
                    let cnt = na.iter().filter(|&&x| unassigned.contains(x as u64)).count();
                    if cnt >= n_unassigned {
                        assignment[i] = n;
                        unassigned.remove(n as u64);
                        if assign(
                            i + 1,
                            n_graph,
                            assignment,
                            unassigned,
                            stair_rooms,
                            staircase_candidates,
                            exterior_rooms,
                            exterior_neighbours,
                            valid_ns,
                            neighbours_all,
                        ) {
                            return true;
                        }
                        unassigned.add(n as u64);
                    }
                }
            }
            false
        }

        if !assign(
            0,
            n_graph,
            &mut assignment,
            &mut unassigned,
            &stair_rooms,
            &staircase_candidates,
            &exterior_rooms,
            &exterior_neighbours,
            &valid_ns,
            &neighbours_all,
        ) {
            return None;
        }
        let mut names: IndexMap<usize, String> = IndexMap::new();
        for j in shared_edges.keys() {
            let idx = assignment.iter().position(|a| a == j)?;
            names.insert(*j, self.graph.names[idx].clone());
        }
        let mut st = RoomState::default();
        for (i, r) in self.graph.names.iter().enumerate() {
            if self.graph.invalid.contains(&i) {
                continue;
            }
            let poly = c.canonicalize(&segments[&assignment[i]]).ok()?;
            let relations = shared_edges[&assignment[i]]
                .iter()
                .map(|(j, se)| Rel { target: names[j].clone(), value: se.clone() })
                .collect();
            st.objs.insert(r.clone(), RoomObj { polygon: Some(poly), relations });
        }
        let relations = exterior_edges
            .iter()
            .map(|(j, se)| Rel { target: names[j].clone(), value: se.clone() })
            .collect();
        st.objs.insert(
            room_name(EXTERIOR, self.level, 0),
            RoomObj { polygon: Some(self.contour.clone()), relations },
        );
        if let Some(ph) = placeholder {
            st.objs.insert(room_name(STAIRCASE, self.level, 0), RoomObj { polygon: Some(ph.clone()), relations: vec![] });
        }
        Some(st)
    }
}

/// plain set of ints (order irrelevant: used for subset / size queries)
pub struct PySetLike(pub BTreeSet<usize>);
