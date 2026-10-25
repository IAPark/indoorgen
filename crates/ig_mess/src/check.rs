//! Audit of the finished scene's clutter (owner: mess-clutter worker): works on the final world meshes only (not on the hulls the
//! settle step used), so it also catches export mistakes. Reports interpenetration against furniture / other clutter, items hovering
//! above their support, and items outside every room.
use crate::settle::{Hull, RoomWorld, Static};
use ig_core::mesh::Scene;

#[derive(Default, Debug, Clone)]
pub struct Report {
    pub items: usize,
    pub floor_items: usize,
    pub max_pen_furniture: f64,
    pub max_pen_item: f64,
    pub floating: usize,
    pub max_gap: f64,
    pub outside: usize,
}

fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

/// Clutter nodes are named `mess_<room>_<idx>_<Class>#<part>`.
pub fn audit(scene: &Scene, rooms: &[(String, ig_core::shp::G)]) -> Report {
    let mut rep = Report::default();
    let mut groups: Vec<(String, Vec<&ig_core::mesh::Node>)> = vec![];
    for n in &scene.nodes {
        if let Some(rest) = n.name.strip_prefix("mess_") {
            let key = rest.split('#').next().unwrap_or(rest).to_string();
            match groups.last_mut() {
                Some((k, v)) if *k == key => v.push(n),
                _ => groups.push((key, vec![n])),
            }
        }
    }
    if groups.is_empty() {
        return rep;
    }
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (_, g) in &groups {
        for n in g {
            let (a, b) = n.mesh.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
    }
    // statics: everything that is not clutter and not above the clutter area
    let mut statics = vec![];
    for n in &scene.nodes {
        if n.name.starts_with("mess_") || n.mesh.is_empty() || n.name.contains("ceiling") || n.name.contains("CeilingLight") {
            continue;
        }
        let (a, b) = n.mesh.bounds();
        if (0..2).any(|k| b[k] < lo[k] - 0.5 || a[k] > hi[k] + 0.5) {
            continue;
        }
        if let Some(s) = Static::from_mesh(&n.mesh, 20000) {
            statics.push(s);
        }
    }
    let polys: Vec<Vec<[f64; 2]>> = rooms.iter().map(|(_, g)| ig_core::shp::exterior(g)).collect();
    let mut hulls: Vec<Vec<Hull>> = vec![];
    for (_, g) in &groups {
        let mut hs = vec![];
        for n in g {
            if n.mesh.pos.len() < 4 {
                continue;
            }
            let stride = (n.mesh.pos.len() / 800).max(1);
            let h: Hull = n.mesh.pos.iter().step_by(stride).copied().collect();
            // the lowest vertices decide the support: keep them in the sample
            hs.push(h);
        }
        hulls.push(hs);
    }
    let mut world = RoomWorld::new(&statics);
    let res = world.audit(&hulls);
    for ((_, g), r) in groups.iter().zip(&res) {
        rep.items += 1;
        rep.max_pen_furniture = rep.max_pen_furniture.max(r.pen_static);
        rep.max_pen_item = rep.max_pen_item.max(r.pen_item);
        if r.gap > 0.01 {
            rep.floating += 1;
        }
        if r.gap < 1.0 {
            rep.max_gap = rep.max_gap.max(r.gap);
        }
        let (a, b) = g.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(mut a, mut b), n| {
            let (x, y) = n.mesh.bounds();
            for k in 0..3 {
                a[k] = a[k].min(x[k]);
                b[k] = b[k].max(y[k]);
            }
            (a, b)
        });
        let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        if !polys.iter().any(|p| p.len() >= 3 && in_poly(c, p)) {
            rep.outside += 1;
        }
        if g[0].name.contains("_floor") {
            rep.floor_items += 1;
        }
    }
    rep
}
