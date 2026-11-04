//! Wainscot: real boards and rails on a room's walls, below a chair rail, in one of four kinds per house, and a picture rail. Built once the room is furnished: it stops at doors and
//! low windows, and at thin things hung on the wall that its depth would come through (a tall mirror); what stands against the wall hides it. How often and which kinds: rules/styles.json
//! "params". Everything stays within about skirting depth, so nothing placed against a wall has to move.
use crate::features::wall_spans;
use crate::mesh::Mesh;
use crate::rng::Rng;
use serde_json::json;

const KINDS: [&str; 4] = ["panel", "batten", "bead", "rail"];
const ROOMS: [&str; 5] = ["living-room", "dining-room", "bedroom", "hallway", "office"];

/// free stretches of [0, len] once `cut` is taken out
fn free(len: f64, mut cut: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    cut.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let (mut out, mut x) = (vec![], 0.0);
    for c in cut { if c[0] > x { out.push([x, c[0].min(len)]); } x = f64::max(x, c[1]); }
    if x < len { out.push([x, len]); }
    out.into_iter().filter(|s: &[f64; 2]| s[1] - s[0] > 0.06).collect()
}

struct Frame { o: [f64; 2], c: f64, s: f64, z: f64 }
impl Frame {
    fn boxed(&self, m: &mut Mesh, lo: [f64; 3], hi: [f64; 3]) {
        let mut b = Mesh::cuboid(lo, hi);
        for p in b.pos.iter_mut() { *p = [self.o[0] + self.c * p[0] - self.s * p[1], self.o[1] + self.s * p[0] + self.c * p[1], p[2] + self.z]; }
        m.append(&b);
    }
}

/// the wainscot of a house: (node name, material slot's spec class, mesh) per room
pub fn build(g: &crate::pipeline::Generated, seed: u64) -> Vec<(String, &'static str, Mesh)> {
    let house = |k: &str, d: f64| ig_core::mesh::style_param(k, d);
    let mut r = Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "wainscot"]));
    let (on, picture) = (r.f64() < house("wainscot_p", 0.35), r.f64() < house("picture_rail_p", 0.15));
    let weights = [house("wainscot_panel", 2.0), house("wainscot_batten", 1.5), house("wainscot_bead", 1.5), house("wainscot_rail", 1.0)];
    let mut u = r.f64() * weights.iter().sum::<f64>();
    let kind = KINDS[weights.iter().position(|w| { if u < *w { true } else { u -= w; false } }).unwrap_or(0)];
    // a chair rail at about a third of the wall, a taller board and batten now and then
    let height = if kind == "batten" && r.f64() < 0.3 { r.uniform(1.3, 1.6) } else { r.uniform(0.82, 1.05) };
    let (bay, batten_gap, room_p) = (r.uniform(0.55, 0.85), r.uniform(0.38, 0.6), house("wainscot_room_p", 0.75));
    let (rail_h, rail_d) = (r.uniform(0.035, 0.07), r.uniform(0.018, 0.03));
    let wt = g.c.wall_thickness;
    let mut out = vec![];
    if !on && !picture { return out }
    for (name, poly) in &g.room_polys {
        let rt = crate::rooms::state::room_type(name);
        if !ROOMS.contains(&rt) { continue }
        let mut rr = Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "wainscot", name]));
        let here = on && rr.f64() < room_p;
        if !here && !picture { continue }
        let (mut boards, mut rails, mut upper) = (Mesh::new(), Mesh::new(), Mesh::new());
        for sp in wall_spans(name, poly, &g.c, &g.cutters) {
            const CORNER: f64 = 0.05;
            let (c, s) = (sp.yaw.cos(), sp.yaw.sin());
            let f = Frame { o: [sp.origin[0] - c * CORNER, sp.origin[1] - s * CORNER], c, s, z: wt / 2.0 };
            let len = sp.span.length + 2.0 * CORNER;
            // (openings in this frame, with room for their casings)
            let open: Vec<([f64; 2], [f64; 2])> = sp.span.openings.iter().map(|o| ([o.x[0] + CORNER - 0.09, o.x[1] + CORNER + 0.09], [o.z[0], o.z[1] + 0.09])).collect();
            // what hangs thin on this wall
            let mut hung: Vec<([f64; 2], [f64; 2])> = vec![];
            for (k, o) in &g.state.objs {
                if o.generator.is_none() || crate::furn::layout2d::room_of(&g.state, k).as_deref() != Some(name.as_str()) { continue }
                let (l, h) = (o.geom.lo, o.geom.hi);
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for i in 0..8 {
                    let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
                    let d = [w[0] - f.o[0], w[1] - f.o[1]];
                    let v = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c, w[2] - f.z];
                    for a in 0..3 { lo[a] = lo[a].min(v[a]); hi[a] = hi[a].max(v[a]); }
                }
                if lo[1] < 0.06 && hi[1] > -0.02 && hi[1] - lo[1] < 0.15 { hung.push(([lo[0] - 0.02, hi[0] + 0.02], [lo[2], hi[2]])); }
            }
            let along = |z0: f64, z1: f64| -> Vec<[f64; 2]> {
                free(len, open.iter().chain(hung.iter()).filter(|(_, z)| z[0] < z1 && z[1] > z0).map(|(x, _)| *x).collect())
            };
            if picture {
                let z = (g.c.wall_height - wt - 0.34).min(2.3);
                for a in along(z, z + 0.03) { f.boxed(&mut upper, [a[0], 0.0, z], [a[1], 0.018, z + 0.03]); }
            }
            if !here { continue }
            for a in along(0.0, height + rail_h) {
                // the rail, a flat ground under it
                f.boxed(&mut rails, [a[0], 0.0, height], [a[1], rail_d, height + rail_h]);
                f.boxed(&mut rails, [a[0], 0.0, height + rail_h], [a[1], rail_d + 0.006, height + rail_h + 0.012]);
                f.boxed(&mut boards, [a[0], 0.0, 0.0], [a[1], 0.006, height]);
                let w = a[1] - a[0];
                match kind {
                    "batten" => {
                        let n = (w / batten_gap).round().max(1.0) as usize;
                        let bw = 0.065;
                        for i in 0..=n {
                            let x = a[0] + (w - bw) * i as f64 / n as f64;
                            f.boxed(&mut rails, [x, 0.0, 0.0], [x + bw, 0.016, height]);
                        }
                    }
                    "panel" => {
                        let n = (w / bay).round().max(1.0) as usize;
                        let (bw, inset, st) = (w / n as f64, 0.055, 0.018);
                        let (z0, z1) = (0.2, height - 0.07);
                        if z1 - z0 < 0.2 || bw < 0.25 { continue }
                        for i in 0..n {
                            let (x0, x1) = (a[0] + bw * i as f64 + inset, a[0] + bw * (i + 1) as f64 - inset);
                            let d = 0.014;
                            f.boxed(&mut rails, [x0, 0.0, z0], [x1, d, z0 + st]);
                            f.boxed(&mut rails, [x0, 0.0, z1 - st], [x1, d, z1]);
                            f.boxed(&mut rails, [x0, 0.0, z0 + st], [x0 + st, d, z1 - st]);
                            f.boxed(&mut rails, [x1 - st, 0.0, z0 + st], [x1, d, z1 - st]);
                        }
                    }
                    _ => {}
                }
            }
        }
        if !boards.pos.is_empty() { out.push((format!("{name}.skirting_wainscot_board"), if kind == "bead" { "wall.Beadboard" } else { "plastic.PlasticRough" }, boards)); }
        if !rails.pos.is_empty() { out.push((format!("{name}.skirting_wainscot"), "plastic.PlasticRough", rails)); }
        if !upper.pos.is_empty() { out.push((format!("{name}.skirting_picture_rail"), "plastic.PlasticRough", upper)); }
    }
    out
}

/// the material of one
pub fn material(cls: &str, node: &str, seed: u64) -> crate::mesh::Material {
    let slot = if node.ends_with("_board") { "wainscot_board" } else if node.ends_with("picture_rail") { "picture_rail" } else { "wainscot" };
    ig_core::materials::spec_material(slot, json!({"cls": cls, "kwargs": {"base_color": [0.86, 0.85, 0.82], "scale": 1.0}, "seed": seed}), [0.86, 0.85, 0.82])
}
