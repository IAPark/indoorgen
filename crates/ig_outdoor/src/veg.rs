//! The outdoor stage's plant-community simulation (see src/outdoor.rs; its own module: an edit here recompiles only this file).
use super::*;

/// Trees and shrubs for the outdoor stage: appends their spots to `props`, the log line to `log`.
#[allow(clippy::too_many_arguments)]
pub(super) fn plant_community(t: &Terrain, cfg: &OutdoorCfg, r: &mut Rng, centre: [f64; 2], e: f64, mask: &dyn Fn(f64, f64) -> f64, thresh: f64, far: f64, props: &mut Vec<Value>, log: &mut Vec<String>) {
    let mut r = r;
    // species (rtree order Oak, Maple, Birch, Pine, Willow): max crown radius (m), growth (m per step), shade tolerance, life (steps),
    // seed dispersal (m), seed rain (chance per mature tree per recruitment)
    struct Sp { rmax: f64, grow: f64, tol: f64, life: f64, disperse: f64, fec: f64 }
    let sps = [
        Sp { rmax: 7.0, grow: 0.30, tol: 0.5, life: 60.0, disperse: 12.0, fec: 0.3 },
        Sp { rmax: 5.5, grow: 0.35, tol: 0.8, life: 45.0, disperse: 18.0, fec: 0.35 },
        Sp { rmax: 3.4, grow: 0.6, tol: 0.15, life: 22.0, disperse: 30.0, fec: 0.4 },
        Sp { rmax: 3.8, grow: 0.35, tol: 0.25, life: 50.0, disperse: 25.0, fec: 0.4 },
        Sp { rmax: 5.5, grow: 0.5, tol: 0.2, life: 30.0, disperse: 15.0, fec: 0.5 },
    ];
    // per-house dominance: one or two species lead
    let mut dom = [0.0f64; 5];
    for w in dom.iter_mut() { *w = r.uniform(0.05, 0.35); }
    let lead = r.randint(0, 5) as usize; dom[lead] = 1.0;
    if r.f64() < 0.6 { dom[r.randint(0, 5) as usize] = 0.8; }
    let nat = |x: f64, y: f64| t.height(x, y, 1e4);
    let site = |x: f64, y: f64| -> [f64; 5] {
        let h = nat(x, y);
        let (dx, dy) = ((nat(x + 3.0, y) - nat(x - 3.0, y)) / 6.0, (nat(x, y + 3.0) - nat(x, y - 3.0)) / 6.0);
        let slope = dx.hypot(dy);
        let wet = (nat(x + 30.0, y) + nat(x - 30.0, y) + nat(x, y + 30.0) + nat(x, y - 30.0)) / 4.0 - h;
        let elev = h - (t.pad_z + cfg.pad_depth);
        let (w, s, hi) = (ramp(wet, 0.12, 0.7), ramp(slope, 0.12, 0.45), ramp(elev, 8.0, 50.0));
        [(1.0 - 0.8 * s) * (1.0 - 0.6 * w) * (1.0 - 0.4 * hi), (1.0 - 0.7 * s) * (1.0 - 0.5 * w) * (1.0 - 0.7 * hi), 0.45 * (1.0 - 0.5 * w),
         0.15 + 0.9 * s.max(hi) - 0.5 * w, 0.03 + 1.2 * w * (1.0 - s)]
    };
    let pick = |r: &mut Rng, suit: &[f64; 5]| -> (usize, f64) {
        let ws: Vec<f64> = (0..5).map(|i| (suit[i] * dom[i]).max(0.0)).collect();
        let tot: f64 = ws.iter().sum();
        let mut u = r.f64() * tot;
        for i in 0..5 { if u < ws[i] { return (i, suit[i].clamp(0.2, 1.0)) } u -= ws[i]; }
        (4, suit[4].clamp(0.2, 1.0))
    };
    let seed_ok = |x: f64, y: f64, r: &mut Rng| -> Option<f64> {
        let rr = (x - centre[0]).hypot(y - centre[1]);
        if rr > far { return None }
        let d = if (x - centre[0]).abs() < e * 0.5 && (y - centre[1]).abs() < e * 0.5 { t.dist(x, y) } else { rr };
        if d < cfg.tree_wall_clearance { return None }
        if t.site.as_ref().map_or(false, |s| s.blocked(x, y, 9.0)) { return None }
        let p = ramp(mask(x, y), thresh - 0.08, thresh + 0.05).max(0.006);
        if r.f64() < p { Some(d) } else { None }
    };
    // plants: pos, species, crown radius, age, vigour, alive
    struct Pl { p: [f64; 2], s: usize, r: f64, age: f64, life: f64, vig: f64, alive: bool }
    let mut pls: Vec<Pl> = vec![];
    let area = std::f64::consts::PI * far * far;
    for _ in 0..(area * cfg.tree_density / 1000.0 * 3.0) as usize {
        let rr = far * r.f64().sqrt();
        let a = r.uniform(0.0, std::f64::consts::TAU);
        let (x, y) = (centre[0] + rr * a.cos(), centre[1] + rr * a.sin());
        if seed_ok(x, y, &mut r).is_none() { continue }
        let (s, vig) = pick(&mut r, &site(x, y));
        let life = sps[s].life * r.uniform(0.75, 1.3);
        let age = r.uniform(0.0, 0.5 * life);
        pls.push(Pl { p: [x, y], s, r: r.uniform(0.3, 1.0) + sps[s].grow * age * 0.5, age, life, vig, alive: true });
    }
    let n_seeded = pls.len();
    let cell = 16.0;
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
    let (mut n_thinned, mut n_old) = (0, 0);
    let cap = pls.len() * 2;
    for step in 0..28 {
        let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, q) in pls.iter().enumerate() { if q.alive { grid.entry(key(q.p)).or_default().push(i); } }
        // shading: per plant, the strongest overlap by a larger crown
        let mut shade = vec![0.0f64; pls.len()];
        for i in 0..pls.len() {
            if !pls[i].alive { continue }
            let (cx, cy) = key(pls[i].p);
            for gx in cx - 1..=cx + 1 { for gy in cy - 1..=cy + 1 {
                let Some(v) = grid.get(&(gx, gy)) else { continue };
                for &j in v {
                    if j == i || pls[j].r <= pls[i].r { continue }
                    let d = (pls[i].p[0] - pls[j].p[0]).hypot(pls[i].p[1] - pls[j].p[1]);
                    let reach = 0.65 * (pls[i].r + pls[j].r);
                    if d < reach { shade[i] = shade[i].max(1.0 - d / reach); }
                }
            }}
        }
        for i in 0..pls.len() {
            let q = &mut pls[i];
            if !q.alive { continue }
            let sp = &sps[q.s];
            q.age += 1.0;
            if shade[i] > 0.0 && r.f64() < shade[i] * (1.0 - sp.tol) * 0.3 { q.alive = false; n_thinned += 1; continue }
            if q.age > q.life { q.alive = false; n_old += 1; continue }
            let light = 1.0 - shade[i] * (1.0 - sp.tol);
            q.r += sp.grow * q.vig * light * (1.0 - q.r / sp.rmax).max(0.0);
        }
        // recruitment: mature trees seed their own species nearby
        if step % 3 == 2 && pls.len() < cap {
            let parents: Vec<([f64; 2], usize)> = pls.iter().filter(|q| q.alive && q.r > 0.5 * sps[q.s].rmax).map(|q| (q.p, q.s)).collect();
            for (pp, s) in parents {
                if r.f64() > sps[s].fec { continue }
                let a = r.uniform(0.0, std::f64::consts::TAU);
                let dd = sps[s].disperse * r.f64().sqrt();
                let (x, y) = (pp[0] + dd * a.cos(), pp[1] + dd * a.sin());
                if seed_ok(x, y, &mut r).is_none() { continue }
                let vig = site(x, y)[s].clamp(0.2, 1.0);
                if r.f64() > vig { continue }
                pls.push(Pl { p: [x, y], s, r: 0.3, age: 0.0, life: sps[s].life * r.uniform(0.75, 1.3), vig, alive: true });
            }
        }
    }
    // (saplings only in the fine grid: past it they are invisible and only cost geometry)
    let trees: Vec<&Pl> = pls.iter().filter(|q| {
        let near = (q.p[0] - centre[0]).abs() < e && (q.p[1] - centre[1]).abs() < e;
        // (and its crown clear of the house, not only its trunk: the clearance is checked as a seed, and a grown oak's branches and leaves came through a bathroom wall)
        q.alive && q.r >= if near { 0.9 } else { 2.2 } && (!near || t.dist(q.p[0], q.p[1]) >= 1.25 * q.r + 0.5)
    }).collect();
    let n_near = trees.iter().filter(|q| (q.p[0] - centre[0]).abs() < e && (q.p[1] - centre[1]).abs() < e).count();
    let mut by_sp = [0usize; 5];
    for q in &trees { by_sp[q.s] += 1; }
    // understory shrubs (near grid only): seeds at grove edges and in the band around the house; deep under a crown they die, and
    // shrubs thin each other (Poisson spacing)
    let mut shrubs: Vec<[f64; 2]> = vec![];
    let mut tgrid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (i, q) in trees.iter().enumerate() { tgrid.entry(key(q.p)).or_default().push(i); }
    let cover = |x: f64, y: f64| -> f64 {
        let (cx, cy) = key([x, y]);
        let mut c = 0.0f64;
        for gx in cx - 1..=cx + 1 { for gy in cy - 1..=cy + 1 {
            for &j in tgrid.get(&(gx, gy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                let d = (x - trees[j].p[0]).hypot(y - trees[j].p[1]);
                if d < 0.8 { return 9.0 }
                c = c.max(1.0 - d / trees[j].r);
            }
        }}
        c
    };
    for _ in 0..((2.0 * e).powi(2) / 100.0 * cfg.bush_density * 0.6) as usize {
        let (x, y) = (centre[0] + r.uniform(-e, e), centre[1] + r.uniform(-e, e));
        let d = t.dist(x, y);
        if d < 1.5 || d > e * 0.97 { continue }
        let m = mask(x, y);
        let near_house = d < 12.0 && r.f64() < 0.35;
        if !(near_house || (m > thresh - 0.14 && r.f64() < 0.5)) { continue }
        let c = cover(x, y);
        if c > 0.55 || r.f64() < c { continue }
        if shrubs.iter().any(|p| (p[0] - x).hypot(p[1] - y) < 2.5) { continue }
        if t.site.as_ref().map_or(false, |s| s.blocked(x, y, 2.5)) { continue }
        shrubs.push([x, y]);
    }
    log.push(format!("plant community: {n_seeded} seeds, {} trees survive ({n_near} in the fine grid; {n_thinned} thinned, {n_old} died of age; oak/maple/birch/pine/willow {:?}), {} shrubs (to {far:.0} m)",
        trees.len(), by_sp, shrubs.len()));
    // props: trees (crown radius -> scale against the species' nominal crown) and shrubs
    let crown = |s: usize| ig_plants::rtree::crown_radius(ig_plants::rtree::ALL[s]);
    let ground_z = |x: f64, y: f64| {
        let d = if (x - centre[0]).abs() < e && (y - centre[1]).abs() < e { t.dist(x, y) } else { (x - centre[0]).hypot(y - centre[1]) };
        t.height(x, y, d) - 0.05
    };
    for q in &trees {
        let variant = 1000 + r.randint(0, cfg.tree_variants as i64) as u64;
        let scale = (q.r / crown(q.s)).clamp(0.3, 1.4);
        props.push(json!({"kind": "tree", "species": q.s, "crown": q.r, "variant": variant, "loc": [q.p[0], q.p[1], ground_z(q.p[0], q.p[1])],
            "rot": r.uniform(0.0, std::f64::consts::TAU), "scale": scale}));
    }
    for p in &shrubs {
        let variant = 1000 + r.randint(0, cfg.bush_variants as i64) as u64;
        props.push(json!({"kind": "bush", "species": 5, "variant": variant, "loc": [p[0], p[1], ground_z(p[0], p[1])], "rot": r.uniform(0.0, std::f64::consts::TAU), "scale": r.uniform(0.7, 1.4)}));
    }
}
