//! Gallery walls after the solve (program.gallery_wall): a cluster of 3-7 small framed pictures hung on the wall behind 60 % of sofas,
//! 45 % of beds and 35 % of sideboards / dining storage (not TV stands: the TV hangs there) -- the picture groups Hypersim / 3D-FRONT living spaces have over
//! their furniture. The wall is found by marching from the furniture's back along its -x (back) axis until leaving the room's inner
//! outline; frames are thin boxes (wood / black / white border + art.Art canvas) laid out on a jittered grid centred over the furniture,
//! between 0.25 m above its top and 0.35 m below the ceiling. Scene geometry only; own stream.
use crate::furn::state::State;
use crate::mesh::{Material, Mesh, Scene};
use crate::rng::Rng;
use serde_json::json;

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, rgb, 0.6);
    m.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}));
    m
}

pub fn add(scene: &mut Scene, state: &State, room_polys: &[(String, crate::shp::G)], seed: u64, ceiling_z: f64, wall_t: f64) -> Vec<String> {
    let mut log = vec![];
    let inner: Vec<crate::shp::G> = room_polys.iter().map(|(_, g)| crate::shp::buffer_mitre(g, -wall_t / 2.0)).collect();
    let mut used_walls: Vec<([f64; 2], [f64; 2])> = vec![];
    // occupied wall space: every solver object's world box (wall art, mirrors, shelves, windows, doors, lamps ...); frames that would
    // overlap one are skipped (paintings hung over the solver's own wall art)
    let world_box = |o: &crate::furn::state::ObjState| -> ([f64; 3], [f64; 3]) {
        let (l, h) = (o.geom.lo, o.geom.hi);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..8 {
            let w = o.pose.apply([if i & 1 == 0 { l[0] } else { h[0] }, if i & 2 == 0 { l[1] } else { h[1] }, if i & 4 == 0 { l[2] } else { h[2] }]);
            for d in 0..3 { lo[d] = lo[d].min(w[d]); hi[d] = hi[d].max(w[d]); }
        }
        (lo, hi)
    };
    let mut occupied: Vec<([f64; 3], [f64; 3])> = state.objs.values().filter(|o| o.generator.is_some() || o.tags.0.iter().any(|t| format!("{t:?}").contains("Window") || format!("{t:?}").contains("Door"))).map(|o| world_box(o)).collect();
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        let p = match g.class.as_str() {
            "SofaFactory" => 0.6,
            "BedFactory" => 0.45,
            "SingleCabinetFactory" | "KitchenCabinetFactory" => 0.35,
            _ => continue,
        };
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "gallery_wall", k]));
        if r.f64() >= p { continue }
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        let top = (0..8).map(|i| o.pose.apply([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }])[2]).fold(f64::MIN, f64::max);
        // the room containing the object, and the wall behind its back (local -x)
        let c = o.pose.apply([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, 0.0]);
        let Some(ri) = inner.iter().position(|g| crate::shp::contains_pt(g, [c[0], c[1]])) else { continue };
        let back = o.pose.apply([lo[0], (lo[1] + hi[1]) / 2.0, 0.0]);
        let dir = { let a = o.pose.apply([0.0, 0.0, 0.0]); let b = o.pose.apply([-1.0, 0.0, 0.0]); let (dx, dy) = (b[0] - a[0], b[1] - a[1]); let n = (dx * dx + dy * dy).sqrt().max(1e-9); [dx / n, dy / n] };
        let mut wall = None;
        for s in 0..60 {
            let d = s as f64 * 0.01;
            let q = [back[0] + dir[0] * d, back[1] + dir[1] * d];
            if !crate::shp::contains_pt(&inner[ri], q) { wall = Some((q, d)); break }
        }
        let Some((wq, gap)) = wall else { continue };
        if gap > 0.4 { continue }
        // one gallery per wall stretch
        if used_walls.iter().any(|(q, _)| (q[0] - wq[0]).hypot(q[1] - wq[1]) < 1.5) { continue }
        used_walls.push((wq, dir));
        let along = [-dir[1], dir[0]]; // wall tangent
        let width = (hi[1] - lo[1]).min(2.2);
        let z0 = top + 0.25;
        let z1 = (ceiling_z - 0.35).min(z0 + 1.1);
        if z1 - z0 < 0.35 { continue }
        let n = r.randint(3, 8) as usize;
        let cols = if n <= 3 { n } else { (n + 1) / 2 };
        let rows = (n + cols - 1) / cols;
        let border_cls = [("wood.Wood", [0.45f32, 0.3, 0.18]), ("metal.BrushedBlackMetal", [0.04, 0.04, 0.04]), ("plastic.PlasticRough", [0.9, 0.9, 0.88])][r.randint(0, 3) as usize];
        let frame_m = scene.material(spec(&format!("gallery_frame:{k}"), border_cls.0, r.next_u64(), border_cls.1));
        let mut placed = 0;
        for i in 0..n {
            let (ci, ri_) = (i % cols, i / cols);
            let cell_w = width / cols as f64;
            let cell_h = (z1 - z0) / rows as f64;
            let fw = (cell_w * r.uniform(0.55, 0.8)).clamp(0.18, 0.6);
            let fh = (cell_h * r.uniform(0.55, 0.85)).clamp(0.18, 0.6);
            let u = -width / 2.0 + cell_w * (ci as f64 + 0.5) + r.uniform(-0.04, 0.04);
            let zc = z0 + cell_h * (ri_ as f64 + 0.5) + r.uniform(-0.04, 0.04);
            // frame box: 2.5 cm deep, its back on the wall face
            let centre = [wq[0] - dir[0] * 0.0125 + along[0] * u, wq[1] - dir[1] * 0.0125 + along[1] * u];
            let fbox = {
                let c0 = [centre[0] - along[0] * fw / 2.0, centre[1] - along[1] * fw / 2.0];
                let c1 = [centre[0] + along[0] * fw / 2.0, centre[1] + along[1] * fw / 2.0];
                let pad = 0.03; // keep a small gap to neighbours
                ([c0[0].min(c1[0]) - 0.05 - pad, c0[1].min(c1[1]) - 0.05 - pad, zc - fh / 2.0 - pad], [c0[0].max(c1[0]) + 0.05 + pad, c0[1].max(c1[1]) + 0.05 + pad, zc + fh / 2.0 + pad])
            };
            let me = world_box(o);
            if occupied.iter().any(|b| !(b.0 == me.0 && b.1 == me.1) && (0..3).all(|d| fbox.0[d] < b.1[d] && b.0[d] < fbox.1[d])) { continue }
            occupied.push(fbox);
            let corners = |hw: f64, hd: f64, h: f64, dz: f64| -> Mesh {
                let mut m = Mesh::new();
                let p = |s: f64, t: f64, z: f64| [centre[0] + along[0] * s - dir[0] * t, centre[1] + along[1] * s - dir[1] * t, z];
                let (za, zb) = (zc - h / 2.0 + dz, zc + h / 2.0 + dz);
                let v = [p(-hw, -hd, za), p(hw, -hd, za), p(hw, hd, za), p(-hw, hd, za), p(-hw, -hd, zb), p(hw, -hd, zb), p(hw, hd, zb), p(-hw, hd, zb)];
                for f in [[0, 1, 2, 3], [7, 6, 5, 4], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]] { m.add_face(&[v[f[0]], v[f[1]], v[f[2]], v[f[3]]]) }
                m
            };
            let name = format!("gallery_{}_{i}", k.replace('/', "_"));
            scene.add(&format!("{name}#0"), "GalleryFrameFactory", corners(fw / 2.0, 0.0125, fh, 0.0), frame_m);
            // canvas: slightly proud of the frame front, inset by the border
            let b = r.uniform(0.02, 0.05).min(fw * 0.2);
            let art = scene.material(spec(&format!("gallery_art:{k}:{i}"), "art.Art", r.next_u64(), [0.6, 0.5, 0.4]));
            let mut cm = Mesh::new();
            let p = |s: f64, z: f64| [centre[0] + along[0] * s - dir[0] * 0.0135, centre[1] + along[1] * s - dir[1] * 0.0135, z];
            let (hw, hh) = (fw / 2.0 - b, fh / 2.0 - b);
            cm.add_face(&[p(-hw, zc - hh), p(hw, zc - hh), p(hw, zc + hh), p(-hw, zc + hh)]);
            scene.add(&format!("{name}#1"), "GalleryFrameFactory", cm, art);
            placed += 1;
        }
        log.push(format!("{placed} frames over {k}"));
    }
    log
}
