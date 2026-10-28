//! Ceiling decor after the furniture solve (program.ceiling_decor): pendant lamps hung over dining tables and kitchen islands, and
//! exposed wooden ceiling beams across some rectangular living / dining rooms -- the architectural detail Hypersim interiors have and the
//! solver's rooms lacked. Pure scene geometry (nodes + point lights): nothing here is a solver object, nothing collides with furniture
//! (pendants hang ~0.75 m above the table top, beams sit in the top 0.2 m of the room, clear of the ceiling lights).
//! Stream: int_hash(seed, "ceiling_decor", room) -- independent of every other factor.
use crate::furn::state::State;
use crate::mesh::{Light, Material, Mesh, Scene};
use crate::rng::Rng;
use ig_kit::geomkit::limb;
use serde_json::json;

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, rgb, 0.6);
    m.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}));
    m
}

/// world xy footprint (lo, hi) and top z of a solver object
fn footprint(state: &State, k: &str) -> Option<([f64; 2], [f64; 2], f64)> {
    let o = state.objs.get(k)?;
    let (l, h) = (o.geom.lo, o.geom.hi);
    let (mut lo, mut hi, mut top) = ([f64::MAX; 2], [f64::MIN; 2], f64::MIN);
    for x in [l[0], h[0]] {
        for y in [l[1], h[1]] {
            for z in [l[2], h[2]] {
                let w = o.pose.apply([x, y, z]);
                lo[0] = lo[0].min(w[0]); lo[1] = lo[1].min(w[1]);
                hi[0] = hi[0].max(w[0]); hi[1] = hi[1].max(w[1]);
                top = top.max(w[2]);
            }
        }
    }
    Some((lo, hi, top))
}

/// Adds pendants and beams to `scene`; returns a short log. `ceiling_z`: ceiling underside, `wall_t`: wall thickness.
pub fn add(scene: &mut Scene, state: &State, room_polys: &[(String, crate::shp::G)], seed: u64, ceiling_z: f64, wall_t: f64, night: bool) -> Vec<String> {
    let mut log = vec![];
    let classes = |c: &str| state.objs.iter().filter(move |(_, o)| o.generator.as_ref().map_or(false, |g| g.class == c)).map(|(k, _)| k.clone()).collect::<Vec<_>>();
    let ceiling_lights: Vec<[f64; 2]> = state.objs.values().filter(|o| o.generator.as_ref().map_or(false, |g| g.class.starts_with("CeilingLight") || g.class.starts_with("CeilingClassic"))).map(|o| [o.pose.t[0], o.pose.t[1]]).collect();
    let mut pend_xy: Vec<[f64; 2]> = vec![];
    // ---- pendants over dining tables (65 %) and kitchen islands (50 %)
    let mut n = 0;
    for (cls, p) in [("TableDiningFactory", 0.65), ("KitchenIslandFactory", 0.5)] {
        for k in classes(cls) {
            let Some((lo, hi, top)) = footprint(state, &k) else { continue };
            let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "ceiling_decor", &k]));
            if r.f64() >= p { continue }
            let (lx, ly) = (hi[0] - lo[0], hi[1] - lo[1]);
            let along = if lx >= ly { 0 } else { 1 };
            let len = lx.max(ly);
            let count = if len > 2.2 { 3 } else if len > 1.5 { 2 } else { 1 };
            let shade_r = r.uniform(0.12, 0.22) * if count > 1 { 0.8 } else { 1.0 };
            let shade_h = r.uniform(0.16, 0.3);
            let zb = (top + r.uniform(0.65, 0.85)).min(ceiling_z - 0.5);
            let style = r.randint(0, 3);
            let (shade_cls, rgb) = [("metal.BrushedBlackMetal", [0.05f32, 0.05, 0.05]), ("metal.MetalBasic", [0.7, 0.7, 0.7]), ("ceramic.Ceramic", [0.9, 0.9, 0.88])][style as usize];
            let shade_m = scene.material(spec(&format!("pendant_shade:{k}"), shade_cls, r.next_u64(), rgb));
            let cord_m = scene.material(Material::diffuse("pendant_cord", [0.02, 0.02, 0.02], 0.5));
            let mut bulb = Material::diffuse("pendant_bulb", [1.0, 0.92, 0.8], 0.3);
            bulb.emissive = [1.0, 0.85, 0.65];
            bulb.emissive_strength = 6.0;
            let bulb_m = scene.material(bulb);
            let on = night || r.f64() < 0.4;
            for i in 0..count {
                let f = (i as f64 + 0.5) / count as f64;
                let mut c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                c[along] = lo[along] + len * (0.2 + 0.6 * f);
                let name = format!("pendant_{n}");
                n += 1;
                scene.add(&format!("{name}#0"), "PendantLampFactory", limb([c[0], c[1], zb + shade_h], [c[0], c[1], ceiling_z], 0.006, 0.006, false), cord_m);
                // canopy at the ceiling
                scene.add(&format!("{name}#1"), "PendantLampFactory", limb([c[0], c[1], ceiling_z - 0.03], [c[0], c[1], ceiling_z], 0.05, 0.05, false), shade_m);
                // open-bottom shade: a tapered tube (r small at the top), bulb just inside
                let shade = limb([c[0], c[1], zb + shade_h], [c[0], c[1], zb], shade_r * 0.35, shade_r, false);
                scene.add(&format!("{name}#2"), "PendantLampFactory", shade, shade_m);
                scene.add(&format!("{name}#3"), "PendantLampFactory", limb([c[0], c[1], zb + 0.04], [c[0], c[1], zb + 0.1], 0.03, 0.03, true), bulb_m);
                let light = Light { loc: [c[0], c[1], zb - 0.02], watt: 35.0, radius: 0.03, temperature: 2900.0, name: format!("{name}_light") };
                if on { scene.lights.push(light) } else { scene.lights_off.push(light) }
                pend_xy.push(c);
            }
            log.push(format!("{count} pendant(s) over {k}"));
        }
    }
    // ---- exposed beams in 25 % of the living / dining rooms that hold furniture
    for (room, poly) in room_polys {
        let rt = crate::rooms::state::room_type(room);
        if !matches!(rt, "living-room" | "dining-room") { continue }
        let furnished = state.objs.values().any(|o| o.generator.is_some() && crate::shp::contains_pt(poly, [o.pose.t[0], o.pose.t[1]]));
        if !furnished { continue }
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "ceiling_decor", room]));
        if r.f64() >= 0.25 { continue }
        // beams span the room's short side, spaced along the long side, clipped to the room outline (L-shaped rooms: one piece per arm)
        let inset = wall_t / 2.0;
        let inner = crate::shp::buffer_mitre(poly, -inset);
        let (x0, y0, x1, y1) = crate::shp::bounds(&inner);
        let span_x = (x1 - x0) < (y1 - y0);
        let (a0, a1) = if span_x { (y0, y1) } else { (x0, x1) };
        let spacing = r.uniform(0.9, 1.4);
        let (bw, bd) = (r.uniform(0.12, 0.18), r.uniform(0.14, 0.22));
        let m = scene.material(spec(&format!("beam:{room}"), "wood.Wood", r.next_u64(), [0.45, 0.3, 0.18]));
        let k = ((a1 - a0) / spacing).floor() as usize;
        let mut placed = 0;
        for i in 1..k.max(1) {
            let a = a0 + (a1 - a0) * i as f64 / k as f64;
            let near = ceiling_lights.iter().chain(pend_xy.iter()).any(|p| ((if span_x { p[1] } else { p[0] }) - a).abs() < bw / 2.0 + 0.15);
            if near { continue }
            let strip = if span_x { crate::shp::bbox(x0 - 1.0, a - bw / 2.0, x1 + 1.0, a + bw / 2.0) } else { crate::shp::bbox(a - bw / 2.0, y0 - 1.0, a + bw / 2.0, y1 + 1.0) };
            let cut = crate::shp::intersection(&strip, &inner);
            for (j, part) in crate::shp::parts(&cut).iter().enumerate() {
                if crate::shp::is_empty(part) || crate::shp::area(part) < 0.05 { continue }
                let (px0, py0, px1, py1) = crate::shp::bounds(part);
                scene.add(&format!("beam_{room}_{i}_{j}").replace('/', "_"), "CeilingBeam", Mesh::cuboid([px0, py0, ceiling_z - bd], [px1, py1, ceiling_z + 0.01]), m);
                placed += 1;
            }
        }
        if placed > 0 { log.push(format!("{placed} beams in {room}")) }
    }
    log
}
