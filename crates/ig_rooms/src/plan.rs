//! FloorPlanSolver (room/floor_plan.py) + FloorPlanMoves (room/solver.py) + ContourFactory (room/contour.py)
use super::consts::*;
use super::graph::GraphMaker;
use super::score::{graph_coherent, room_term};
use super::segment::SegmentMaker;
use super::state::*;
use crate::rng::Rng;
use crate::shp::*;

pub struct FloorPlanSolver {
    pub factory_seed: u64,
    pub c: RoomConstants,
    pub graphs: Vec<RoomGraph>,
    pub widths: Vec<f64>,
    pub heights: Vec<f64>,
    pub contours: Vec<G>,
    segment_makers: Vec<SegmentMaker>,
    n_divide_trials: usize,
    best_of: usize,
    iter_per_room: usize,
    score_scale: f64,
}

#[derive(Clone, Copy)]
pub struct PlanConfig {
    pub n_divide_trials: usize,
    pub iters_mult: usize,
    pub rooms: super::graph::RoomSet,
}

impl FloorPlanSolver {
    /// FloorPlanSolver.__init__ (everything under FixedSeed(factory_seed))
    pub fn new(factory_seed: u64, c: &RoomConstants, cfg: PlanConfig) -> Self {
        let mut rng = Rng::seeded(factory_seed);
        let mut graphs = vec![];
        let mut widths = vec![];
        let mut heights = vec![];
        let mut contours = vec![];
        for i in 0..c.n_stories {
            let gm = GraphMaker::new(factory_seed, c, i, cfg.rooms);
            let (graph, width, height);
            {
                let seed = rng.randint(0, 1_000_000) as u64;
                let g = gm.make_graph(seed);
                let (w, h) = gm.suggest_dimensions(&g, &mut rng, None);
                graph = g;
                width = w;
                height = h;
            }
            graphs.push(graph);
            contours.push(bbox(0.0, 0.0, width, height));
            widths.push(width);
            heights.push(height);
        }
        let segment_makers = (0..c.n_stories)
            .map(|i| SegmentMaker::new(factory_seed, c, contours[i].clone(), graphs[i].clone(), i))
            .collect();
        FloorPlanSolver {
            factory_seed,
            c: c.clone(),
            graphs,
            widths,
            heights,
            contours,
            segment_makers,
            n_divide_trials: cfg.n_divide_trials,
            best_of: if cfg.rooms == super::graph::RoomSet::Curated { 12 } else { 1 },
            iter_per_room: if cfg.rooms == super::graph::RoomSet::Curated { cfg.iters_mult * 3 } else { cfg.iters_mult },
            score_scale: 5.0,
        }
    }

    fn add_staircase(&self, rng: &mut Rng, contour: &G) -> G {
        let (x_, y_, x__, y__) = bounds(contour);
        for _ in 0..1000 {
            let area = self.c.segment_margin * self.c.wall_height * 6.0;
            let mut skew = rng.log_uniform(0.5, 0.8);
            if rng.f64() < 0.5 {
                skew = 1.0 / skew;
            }
            let width = self.c.unit_cast((area * skew).sqrt());
            let height = self.c.unit_cast((area / skew).sqrt());
            let x = self.c.unit_cast(rng.uniform(x_, x__ - width));
            let y = self.c.unit_cast(rng.uniform(y_, y__ - height));
            let b = bbox(x, y, x + width, y + height);
            if contains(contour, &b) {
                return b;
            }
        }
        panic!("Invalid staircase");
    }

    /// FloorPlanSolver.solve up to (and including) contour decoration. `rng` = stream A.
    pub fn solve(&self, rng: &mut Rng) -> RoomState {
        let mut state = RoomState::default();
        state.graphs = self.graphs.clone();
        let mut done = false;
        while !done {
            let pholder = self.add_staircase(rng, self.contours.last().unwrap());
            state.objs.clear();
            done = true;
            for j in 0..self.c.n_stories {
                // rooms are assigned to the random boxes by adjacency alone, so the first division that fits put hallways (the most
                // connected room) on big central boxes; keep the best-scoring of up to `best_of` divisions instead (1 = python)
                let mut got: Option<(f64, RoomState)> = None;
                let mut found = 0;
                for _ in 0..self.n_divide_trials * (j + 1) * (j + 1) * self.best_of {
                    if let Some(st) = self.segment_makers[j].build_segments(rng, Some(&pholder)) {
                        let sc = if self.best_of > 1 { room_term(&st, &self.c) } else { 0.0 };
                        if got.as_ref().map_or(true, |(b, _)| sc < *b) {
                            got = Some((sc, st));
                        }
                        found += 1;
                        if found >= self.best_of {
                            break;
                        }
                    }
                }
                let got = got.map(|(_, st)| st);
                match got {
                    Some(st) => {
                        for (k, v) in st.objs {
                            state.objs.insert(k, v);
                        }
                    }
                    None => {
                        done = false;
                        break;
                    }
                }
            }
        }
        let mut state = self.simulated_anneal(rng, state);
        self.decorate(rng, &mut state);
        state
    }

    fn simulated_anneal(&self, rng: &mut Rng, mut state: RoomState) -> RoomState {
        let mut score = room_term(&state, &self.c);
        let it = self.iter_per_room * self.graphs.iter().map(|g| g.len()).sum::<usize>();
        for n in 0..it {
            let state_ = self.perturb_state(rng, &state);
            let score_ = room_term(&state_, &self.c);
            let violated = graph_coherent(&state_, &self.c);
            let scale = self.score_scale * n as f64 / it as f64;
            if rng.f64().ln() < (score - score_) * scale && violated == 0.0 {
                state = state_;
                score = score_;
            }
        }
        state
    }

    // ---- FloorPlanMoves ----

    fn perturb_state(&self, rng: &mut Rng, state: &RoomState) -> RoomState {
        loop {
            let ks: Vec<&String> = state.objs.keys().filter(|k| room_type(k) != EXTERIOR).collect();
            let k = (*rng.choice(&ks)).clone();
            let mut state_ = state.clone();
            let rn = rng.f64();
            let res: Ni<Vec<String>> = if room_type(&k) == STAIRCASE {
                self.move_staircase(rng, &mut state_)
            } else if rn < 0.4 {
                self.extrude_room_out(rng, &mut state_, &k)
            } else if rn < 0.8 {
                self.extrude_room_in(rng, &mut state_, &k)
            } else {
                self.swap_room(rng, &mut state_, &k)
            };
            let indices = res.unwrap_or_default();
            if !indices.is_empty() {
                if indices.iter().any(|i| room_type(i) != STAIRCASE) {
                    if !self.c.fixed_contour {
                        update_contour(&mut state_, state, &indices);
                    }
                    for k in &indices {
                        update_shared(&mut state_, k);
                        update_exterior(&mut state_, k);
                    }
                }
                return state_;
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn extrude_room(&self, rng: &mut Rng, state: &RoomState, k: &str, out: bool) -> Ni<(G, G, f64, f64)> {
        let coords = exterior(state.poly(k));
        let n = coords.len() - 1;
        let lengths: Vec<f64> = (0..n)
            .map(|i| ((coords[i][0] - coords[i + 1][0]).powi(2) + (coords[i][1] - coords[i + 1][1]).powi(2)).sqrt())
            .collect();
        let weights: Vec<f64> = lengths.iter().map(|l| l.sqrt()).collect();
        let m = rng.choice_p(&weights);
        let mut cc = coords.clone();
        cc.extend_from_slice(&coords[1..coords.len() - 1]);
        let (mut u, mut v) = (cc[m], cc[m + 1]);
        let r = rng.f64();
        let t = rng.uniform(0.35, 0.65);
        let mid = [
            self.c.unit_cast(u[0] + t * (v[0] - u[0])),
            self.c.unit_cast(u[1] + t * (v[1] - u[1])),
        ];
        let dist = |a: P2, b: P2| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        if dist(u, mid).max(dist(v, mid)) < self.c.segment_margin {
            if r < 0.2 {
                u = mid;
            } else if r < 0.4 {
                v = mid;
            }
        }
        let stride = self.c.unit * (rng.randint(0, 5) + 1) as f64;
        let theta = (v[1] - u[1]).atan2(v[0] - u[0]);
        let theta_ = if out { theta - std::f64::consts::FRAC_PI_2 } else { theta + std::f64::consts::FRAC_PI_2 };
        let u_ = [u[0] + stride * theta_.cos(), u[1] + stride * theta_.sin()];
        let v_ = [v[0] + stride * theta_.cos(), v[1] + stride * theta_.sin()];
        let mk = |pts: &[P2]| polygon(pts, &[]).map_err(|_| NotImpl("bad cutter"));
        if out {
            let c = mk(&[v, u, u_, v_])?;
            let s = self.c.canonicalize(&union(state.poly(k), &c))?;
            Ok((c, s, stride, theta))
        } else {
            let c = mk(&[u, v, v_, u_])?;
            let s = self.c.canonicalize(&difference(state.poly(k), &c))?;
            Ok((c, s, stride, theta))
        }
    }

    fn rel_targets(state: &RoomState, k: &str) -> Vec<String> {
        state.objs[k].relations.iter().map(|r| r.target.clone()).collect()
    }

    fn extrude_room_out(&self, rng: &mut Rng, state: &mut RoomState, k: &str) -> Ni<Vec<String>> {
        let (c, s, stride, _theta) = self.extrude_room(rng, state, k, true)?;
        let mut indices = vec![k.to_string()];
        for l in Self::rel_targets(state, k) {
            let p = state.poly(&l).clone();
            if distance(&p, &c) <= stride {
                state.objs.get_mut(&l).unwrap().polygon = Some(self.c.canonicalize(&difference(&p, &c))?);
                if !indices.contains(&l) {
                    indices.push(l);
                }
            }
        }
        state.objs.get_mut(k).unwrap().polygon = Some(s);
        Ok(indices)
    }

    fn extrude_room_in(&self, rng: &mut Rng, state: &mut RoomState, k: &str) -> Ni<Vec<String>> {
        let (c, s, stride, theta) = self.extrude_room(rng, state, k, false)?;
        let mut indices = vec![k.to_string()];
        for l in Self::rel_targets(state, k) {
            let p = state.poly(&l).clone();
            if distance(&p, &c) <= stride {
                let q = if (theta.abs() - std::f64::consts::FRAC_PI_2).abs() < 0.1 {
                    union(&translate(&p, -stride, 0.0), &translate(&p, stride, 0.0))
                } else {
                    union(&translate(&p, 0.0, -stride), &translate(&p, 0.0, stride))
                };
                let inter = intersection(&q, &c);
                if area(&inter) > 0.1 {
                    state.objs.get_mut(&l).unwrap().polygon = Some(self.c.canonicalize(&union(&p, &inter))?);
                    if !indices.contains(&l) {
                        indices.push(l);
                    }
                }
            }
        }
        state.objs.get_mut(k).unwrap().polygon = Some(s);
        Ok(indices)
    }

    fn swap_room(&self, rng: &mut Rng, state: &mut RoomState, k: &str) -> Ni<Vec<String>> {
        let cands: Vec<String> = state.objs[k]
            .relations
            .iter()
            .filter(|r| length(&r.value) > 0.0)
            .map(|r| r.target.clone())
            .collect();
        if cands.is_empty() {
            return Err(NotImpl("no swap candidates"));
        }
        let j = rng.choice(&cands).clone();
        let pk = state.poly(k).clone();
        let pj = state.poly(&j).clone();
        state.objs.get_mut(k).unwrap().polygon = Some(pj);
        state.objs.get_mut(&j).unwrap().polygon = Some(pk);
        Ok(vec![k.to_string(), j])
    }

    fn move_staircase(&self, rng: &mut Rng, state: &mut RoomState) -> Ni<Vec<String>> {
        let name = room_name(STAIRCASE, 0, 0);
        let mut p = state.poly(&name).clone();
        if rng.f64() < 0.2 {
            // python: `p.coords` on a Polygon raises NotImplementedError
            return Err(NotImpl("polygon has no coords"));
        }
        let dx = self.c.unit * rng.randint(-4, 5) as f64;
        let dy = self.c.unit * rng.randint(-4, 5) as f64;
        p = translate(&p, dx, dy);
        let ext = room_name(EXTERIOR, state.graphs.len() - 1, 0);
        if contains(state.poly(&ext), &buffer(&p, -0.1)) {
            let mut names = vec![];
            for i in 0..state.graphs.len() {
                let n = room_name(STAIRCASE, i, 0);
                state.objs.get_mut(&n).unwrap().polygon = Some(p.clone());
                names.push(n);
            }
            return Ok(names);
        }
        Err(NotImpl("staircase outside"))
    }

    // ---- ContourFactory.decorate (non-fixed contour) ----

    fn get_length(polygon: &G, p: P2) -> Option<f64> {
        let coords = exterior(polygon);
        let coords = &coords[..coords.len() - 1];
        let n = coords.len();
        let d = |a: P2, b: P2| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let i = coords.iter().position(|c| (c[0] - p[0]).abs() + (c[1] - p[1]).abs() < 0.1)?;
        let q = d(coords[i], coords[(i + 1) % n]);
        let r = d(coords[i], coords[(i + n - 1) % n]);
        Some(q.min(r))
    }

    fn decorate(&self, rng: &mut Rng, state: &mut RoomState) {
        let maximal_radius = 4.0;
        for i in (0..state.graphs.len()).rev() {
            let exterior_name = room_name(EXTERIOR, i, 0);
            let ext = match self.c.canonicalize(state.poly(&exterior_name)) {
                Ok(e) => e,
                Err(_) => panic!("decorate: exterior canonicalize failed"),
            };
            let c = centroid(&ext);
            let coords = exterior(&ext);
            for p in coords {
                let sq = buffer_full(&point(p[0], p[1]), 0.1, Join::Round, Cap::Square, 16, false);
                if area(&intersection(&sq, &ext)) > 2.0 * 0.1 * 0.1 {
                    continue;
                }
                let inside = point(p[0] + (c[0] - p[0]) * 1e-2, p[1] + (c[1] - p[1]) * 1e-2);
                if !contains(&ext, &inside) {
                    continue;
                }
                let mut length = match Self::get_length(&ext, p) {
                    Some(l) => l,
                    None => continue,
                };
                let mut kk: Option<String> = None;
                let mut ll: Option<f64> = None;
                for (k, o) in state.objs.iter() {
                    if !matches!(room_type(k), STAIRCASE | EXTERIOR) && room_level(k) == i {
                        kk = Some(k.clone());
                        ll = Self::get_length(o.polygon.as_ref().unwrap(), p);
                        if ll.is_some() {
                            break;
                        }
                    }
                }
                let (k, l) = match (kk, ll) {
                    (Some(k), Some(l)) => (k, l),
                    _ => continue,
                };
                length = length.min(l).min(maximal_radius);
                let mut direction = None;
                'dirs: for xs in [-1.0, 1.0] {
                    for ys in [-1.0, 1.0] {
                        if contains(&ext, &point(p[0] + xs, p[1] + ys)) {
                            direction = Some([xs, ys]);
                            break 'dirs;
                        }
                    }
                }
                let direction = match direction {
                    Some(d) => d,
                    None => continue,
                };
                // rg(("weighted_choice", (1,"round"), (2,"sharp"), (7,"none")))
                // round / cut corners: Infinigen's 10% / 20% made angled walls common (diagonal counters, wedge-shaped rooms); real
                // homes have few
                let pa = crate::plan_rules::get().angled_corner_p.clamp(0.0, 1.0);
                let corner = ["round", "sharp", "none"][rng.choice_p(&[pa * 0.375, pa * 0.625, 1.0 - pa])];
                if corner == "none" {
                    continue;
                }
                length -= self.c.unit;
                let mut result: Option<(G, G)> = None;
                while length > 0.0 {
                    let q = [p[0] + direction[0] * length, p[1] + direction[1] * length];
                    let cutter = bbox(p[0].min(q[0]), p[1].min(q[1]), p[0].max(q[0]), p[1].max(q[1]));
                    let quad_segs = if corner == "sharp" { 1 } else { rng.randint(4, 7) as i32 };
                    let circle = buffer_full(&point(q[0], q[1]), length, Join::Round, Cap::Round, quad_segs, false);
                    let ext_ = match self.c.canonicalize(&buffer(&union(&difference(&ext, &cutter), &circle), 0.0)) {
                        Ok(e) => e,
                        Err(_) => panic!("decorate: canonicalize exterior_"),
                    };
                    // a small room (closet) the cut splits or empties: try a smaller corner
                    let Ok(new) = self.c.canonicalize(&buffer(&intersection(state.poly(&k), &ext_), 0.0)) else {
                        length -= self.c.unit;
                        continue;
                    };
                    let ok = [0.0, 0.25, 0.5, 0.75].iter().all(|m| {
                        let b = buffer(&new, -m + 1e-2);
                        matches!(gtype(&b), GeometryTypes::Polygon)
                    });
                    if ok {
                        let (x, y, x_, y_) = max_segment(&exterior(&new));
                        if ((x - x_).powi(2) + (y - y_).powi(2)).sqrt() >= 1.0 {
                            result = Some((new, ext_));
                            break;
                        }
                    }
                    length -= self.c.unit;
                }
                let (new, ext_) = match result {
                    Some(r) if length > 0.0 => r,
                    _ => continue,
                };
                let nc = exterior(&new);
                let mut diff: Vec<P2> = vec![];
                for w in nc.windows(2) {
                    let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    diff.push(if n > 0.0 { [d[0] / n, d[1] / n] } else { d });
                }
                let nd = diff.len();
                if (0..nd).any(|a| {
                    let b = (a + 1) % nd;
                    diff[a][0] * diff[b][0] + diff[a][1] * diff[b][1] < -0.01
                }) {
                    continue;
                }
                state.objs.get_mut(&k).unwrap().polygon = Some(new);
                state.objs.get_mut(&exterior_name).unwrap().polygon = Some(ext_);
                update_exterior(state, &k);
            }
        }
    }
}

/// max_mls for a single ring: the first longest segment
pub fn max_segment(c: &[P2]) -> (f64, f64, f64, f64) {
    let mut best = (0.0, c[0][0], c[0][1], c[1][0], c[1][1]);
    for (i, w) in c.windows(2).enumerate() {
        let l = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        if i == 0 || l > best.0 {
            best = (l, w[0][0], w[0][1], w[1][0], w[1][1]);
        }
    }
    (best.1, best.2, best.3, best.4)
}
