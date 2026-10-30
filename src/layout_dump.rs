//! LAYOUT_DUMP=path: the furniture layout after the solve and the solve-level fixes (caps, chairs, coffee tables), before any mesh is
//! built: room polygons, door / window / entrance cutters and, per object, its solver proxies as world-space point sets with height
//! ranges (floor footprints), its front direction and its parent. Drawn by tools/layout_sheet.py.
//! LAYOUT_DUMP_STAGE=furniture: the solver stops after the furniture stages (floor / wall pieces, then wall-mounted, ceiling and side
//! pieces), skipping the small items on surfaces; the post-solve layout fixes still run before the dump.
use crate::furn::tags::{Tag, TagSet};
use crate::pipeline::Generated;
use serde_json::json;

pub fn dump(path: &str, seed: u64, g: &Generated) {
    std::fs::write(path, to_json(seed, g).to_string()).unwrap();
}

/// the layout as JSON (also returned by the browser build's `layout()`): rooms (WKT, outline coordinates, inner floor, walkway status),
/// cutters, and per object its footprint prisms, front, parent, room, size and seeds
pub fn to_json(seed: u64, g: &Generated) -> serde_json::Value {
    use crate::furn::layout2d;
    let ring = |p: &crate::shp::G| -> Vec<Vec<[f64; 2]>> { crate::shp::parts(p).iter().filter(|q| crate::shp::is_polygon(q)).map(|q| crate::shp::exterior(q)).collect() };
    let room_info: serde_json::Map<String, serde_json::Value> = g.room_polys.iter().map(|(k, p)| {
        let (connected, bad) = if g.state.objs.contains_key(k) { layout2d::walk_status(&g.state, k) } else { (true, vec![]) };
        let floor = if g.state.objs.contains_key(k) { ring(&layout2d::room_floor(&g.state, k)) } else { vec![] };
        (k.clone(), json!({"type": crate::rooms::state::room_type(k), "outline": ring(p), "floor": floor, "area": crate::shp::area(p), "doors_connected": connected, "unreachable": bad}))
    }).collect();
    // WKT (native only: the browser build has no GEOS); room_info carries the outlines as coordinates everywhere
    #[cfg(not(target_arch = "wasm32"))]
    let rooms: serde_json::Map<String, serde_json::Value> = { use geos::Geom; g.room_polys.iter().filter_map(|(k, p)| Some((k.clone(), json!(p.to_wkt().ok()?)))).collect() };
    #[cfg(target_arch = "wasm32")]
    let rooms: serde_json::Map<String, serde_json::Value> = Default::default();
    let cut: Vec<_> = g.cutters.iter().map(|ct| json!({"kind": format!("{:?}", ct.kind), "style": ct.style.name(), "loc": ct.loc, "rot": ct.rot_z, "dim": ct.dims, "rooms": ct.rooms})).collect();
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
        let gen = o.generator.as_ref();
        let size = gen.and_then(crate::furn::solver::cur_size);
        let room = layout2d::room_of(&g.state, k);
        objs.push(json!({"key": k, "class": class, "parts": parts, "front": front, "parent": parent, "room": room, "size": size, "round": o.tags.contains(Tag::sem("RoundTable")),
            "factory_seed": gen.map(|x| x.factory_seed), "inst_seed": gen.map(|x| x.inst_seed), "yaw_deg": o.pose.r[1][0].atan2(o.pose.r[0][0]).to_degrees(), "pos": o.pose.t}));
    }
    let stage = if crate::furn::layout2d::knobs::furniture_only() { "furniture" } else { "full" };
    json!({"seed": seed, "stage": stage, "knobs": crate::furn::layout2d::knobs::get(), "wall_thickness": g.c.wall_thickness, "wall_height": g.c.wall_height, "rooms": rooms, "room_info": room_info, "cutters": cut, "objects": objs})
}
