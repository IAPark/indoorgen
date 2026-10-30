//! LAYOUT_DUMP=path: the furniture layout after the solve and the solve-level fixes (caps, chairs, coffee tables), before any mesh is
//! built: room polygons, door / window / entrance cutters and, per object, its solver proxies as world-space point sets with height
//! ranges (floor footprints), its front direction and its parent. Drawn by tools/layout_sheet.py.
use crate::furn::tags::{Tag, TagSet};
use crate::pipeline::Generated;
use geos::Geom;
use serde_json::json;

pub fn dump(path: &str, seed: u64, g: &Generated) {
    let rooms: serde_json::Map<String, serde_json::Value> = g.room_polys.iter().filter_map(|(k, p)| Some((k.clone(), json!(p.to_wkt().ok()?)))).collect();
    let cut: Vec<_> = g.cutters.iter().map(|ct| json!({"kind": format!("{:?}", ct.kind), "loc": ct.loc, "rot": ct.rot_z, "dim": ct.dims, "rooms": ct.rooms})).collect();
    let mut objs = vec![];
    for (k, o) in &g.state.objs {
        let class = match &o.generator { Some(gen) => gen.class.clone(), None if o.tags.contains(Tag::sem("FeatureKeepout")) => "Keepout".into(), None => continue };
        let parts: Vec<_> = crate::furn::col::proxies(&g.state, k).iter().map(|p| {
            let z0 = p.corners.iter().map(|c| c[2]).fold(f64::INFINITY, f64::min);
            let z1 = p.corners.iter().map(|c| c[2]).fold(f64::NEG_INFINITY, f64::max);
            json!({"xy": p.corners.iter().map(|c| [c[0], c[1]]).collect::<Vec<_>>(), "z": [z0, z1]})
        }).collect();
        if parts.is_empty() { continue }
        let front = {
            let pl = g.state.planes(k, &TagSet::from([Tag::sub("Front")]));
            pl.first().map(|&i| { let (n, _) = g.state.plane_nd(k, i); [n[0], n[1]] })
        };
        let parent = o.relations.iter().map(|r| r.target.clone()).find(|t| g.state.objs.get(t).map_or(false, |x| x.generator.is_some()));
        objs.push(json!({"key": k, "class": class, "parts": parts, "front": front, "parent": parent}));
    }
    let d = json!({"seed": seed, "wall_thickness": g.c.wall_thickness, "wall_height": g.c.wall_height, "rooms": rooms, "cutters": cut, "objects": objs});
    std::fs::write(path, d.to_string()).unwrap();
}
