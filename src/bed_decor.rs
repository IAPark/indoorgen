//! Bedroom dressing after the solve (program.bed_decor): an upholstered bench across the foot of the bed with a folded blanket on it, a pair
//! of slippers beside the bed and a laundry basket next to a nightstand -- the things 3D-FRONT / Hypersim bedrooms have around the bed.
//! Each item goes only where its world box is inside the room (0.18 m from the wall centre line) and clear of every solver object, earlier
//! decor and the door swing zones. Scene geometry only; own stream. Bed frame: x = 0 at the headboard, foot toward +x, y centred.
use crate::furn::state::State;
use crate::mesh::{Material, Mesh, Scene};
use crate::rng::Rng;
use ig_kit::geomkit::limb;
use serde_json::json;

type Box3 = ([f64; 3], [f64; 3]);
fn overlaps(a: &Box3, b: &Box3) -> bool { (0..3).all(|d| a.0[d] < b.1[d] && b.0[d] < a.1[d]) }

fn world_box(o: &crate::furn::state::ObjState, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
    let (mut wl, mut wh) = ([f64::MAX; 3], [f64::MIN; 3]);
    for i in 0..8 {
        let w = o.pose.apply([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]);
        for d in 0..3 { wl[d] = wl[d].min(w[d]); wh[d] = wh[d].max(w[d]); }
    }
    (wl, wh)
}

fn fab(scene: &mut Scene, name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> usize {
    let mut m = Material::diffuse(name, rgb, 0.8);
    m.spec = Some(json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}));
    scene.material(m)
}

pub fn add(scene: &mut Scene, state: &State, room_polys: &[(String, crate::shp::G)], keepout: &[([f64; 3], [f64; 3], f64)], seed: u64) -> Vec<String> {
    let mut log = vec![];
    let inner: Vec<crate::shp::G> = room_polys.iter().map(|(_, g)| crate::shp::buffer_mitre(g, -0.18)).collect();
    let mut taken: Vec<Box3> = state.objs.values().filter(|o| o.generator.is_some()).map(|o| world_box(o, o.geom.lo, o.geom.hi)).collect();
    taken.extend(scene.nodes.iter().filter(|n| ["mess_", "dress_", "cushion_", "pendant_", "gallery_"].iter().any(|p| n.name.starts_with(p))).map(|n| n.mesh.bounds()));
    taken.extend(keepout.iter().map(|(c, h, _)| ([c[0] - h[0], c[1] - h[1], 0.0], [c[0] + h[0], c[1] + h[1], 3.0])));
    let fits = |taken: &[Box3], b: &Box3| -> bool {
        let corners = [[b.0[0], b.0[1]], [b.1[0], b.0[1]], [b.1[0], b.1[1]], [b.0[0], b.1[1]]];
        inner.iter().any(|g| corners.iter().all(|c| crate::shp::contains_pt(g, *c))) && !taken.iter().any(|t| overlaps(b, t))
    };
    let side_tables: Vec<Box3> = state.objs.values().filter(|o| o.generator.as_ref().map_or(false, |g| g.class == "SideTableFactory")).map(|o| world_box(o, o.geom.lo, o.geom.hi)).collect();
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        if g.class != "BedFactory" { continue }
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "bed_decor", k]));
        let (lo, hi) = (o.geom.lo, o.geom.hi);
        let me = world_box(o, lo, hi);
        let others: Vec<Box3> = taken.iter().filter(|t| !(t.0 == me.0 && t.1 == me.1)).cloned().collect();
        let fz = lo[2]; // floor in the bed frame
        let key = k.replace('/', "_");
        // ---- bench at the foot, 45 %
        if r.f64() < 0.45 {
            let (bd, bh) = (r.uniform(0.38, 0.45), r.uniform(0.42, 0.48));
            let bw = ((hi[1] - lo[1]) * r.uniform(0.6, 0.8)).max(0.9);
            let (x0, x1) = (hi[0] + 0.12, hi[0] + 0.12 + bd);
            let yc = (lo[1] + hi[1]) / 2.0;
            let wb = world_box(o, [x0, yc - bw / 2.0, fz], [x1, yc + bw / 2.0, fz + bh]);
            if fits(&others, &wb) {
                let cls = ["fabric.SofaFabric", "fabric.Velvet", "fabric.Leather", "fabric.CoarseKnitFabric"][r.randint(0, 4) as usize];
                let fm = fab(scene, &format!("bench:{key}"), cls, r.next_u64(), [0.55, 0.5, 0.45]);
                let legm = fab(scene, &format!("bench_leg:{key}"), "wood.Wood", r.next_u64(), [0.35, 0.25, 0.15]);
                let mut top = crate::assets::modern_sofa::pad([x0, yc - bw / 2.0, fz + bh - 0.12], [x1, yc + bw / 2.0, fz + bh], 0.035, 2);
                for q in top.pos.iter_mut() { *q = o.pose.apply(*q) }
                scene.add(&format!("bench_{key}#0"), "BenchFactory", top, fm);
                let mut legs = Mesh::new();
                for (lx, ly) in [(x0 + 0.04, yc - bw / 2.0 + 0.04), (x1 - 0.04, yc - bw / 2.0 + 0.04), (x0 + 0.04, yc + bw / 2.0 - 0.04), (x1 - 0.04, yc + bw / 2.0 - 0.04)] {
                    legs.append(&limb([lx, ly, fz], [lx, ly, fz + bh - 0.12], 0.018, 0.022, true));
                }
                for q in legs.pos.iter_mut() { *q = o.pose.apply(*q) }
                scene.add(&format!("bench_{key}#1"), "BenchFactory", legs, legm);
                // folded blanket on one end
                let pal: [[f32; 3]; 6] = [[0.85, 0.82, 0.76], [0.4, 0.45, 0.55], [0.6, 0.55, 0.5], [0.3, 0.32, 0.3], [0.7, 0.6, 0.55], [0.5, 0.6, 0.55]];
                let bm = fab(scene, &format!("blanket:{key}"), "fabric.CoarseKnitFabric", r.next_u64(), pal[r.randint(0, 6) as usize]);
                let by = yc + bw * 0.22;
                let mut bl = crate::assets::modern_sofa::pad([x0 + 0.03, by - 0.18, fz + bh], [x1 - 0.03, by + 0.18, fz + bh + 0.07], 0.025, 2);
                for q in bl.pos.iter_mut() { *q = o.pose.apply(*q) }
                scene.add(&format!("blanket_{key}#0"), "BlanketFactory", bl, bm);
                taken.push(wb);
                log.push(format!("bench at {k}"));
            }
        }
        // ---- slippers beside the bed (either side, near the head), 60 %
        if r.f64() < 0.6 {
            let side = if r.f64() < 0.5 { lo[1] - 0.2 } else { hi[1] + 0.2 };
            let x = lo[0] + (hi[0] - lo[0]) * r.uniform(0.3, 0.55);
            let wb = world_box(o, [x - 0.15, side - 0.12, fz], [x + 0.15, side + 0.12, fz + 0.08]);
            if fits(&others, &wb) {
                let pal: [[f32; 3]; 5] = [[0.85, 0.82, 0.78], [0.2, 0.22, 0.3], [0.6, 0.45, 0.45], [0.45, 0.45, 0.45], [0.55, 0.4, 0.3]];
                let sm = fab(scene, &format!("slipper:{key}"), "fabric.FineKnitFabric", r.next_u64(), pal[r.randint(0, 5) as usize]);
                let yaw = r.uniform(-0.4, 0.4);
                let mut m = Mesh::new();
                for dy in [-0.055, 0.055] {
                    let mut s = crate::assets::modern_sofa::pad([-0.13, dy - 0.045, 0.0], [0.13, dy + 0.045, 0.055], 0.025, 2);
                    for q in s.pos.iter_mut() {
                        let (xx, yy) = (q[0] * yaw.cos() - q[1] * yaw.sin(), q[0] * yaw.sin() + q[1] * yaw.cos());
                        *q = o.pose.apply([x + xx, side + yy, fz + q[2]]);
                    }
                    m.append(&s);
                }
                scene.add(&format!("slippers_{key}#0"), "SlipperFactory", m, sm);
                taken.push(wb);
                log.push(format!("slippers at {k}"));
            }
        }
    }
    // ---- laundry basket beside a nightstand (one per bedroom), 50 %
    for (i, st) in side_tables.iter().enumerate() {
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "basket", &i.to_string()]));
        if r.f64() >= 0.5 { continue }
        let (rad, hgt) = (r.uniform(0.18, 0.23), r.uniform(0.45, 0.6));
        let cx = [(st.0[0] + st.1[0]) / 2.0, (st.0[1] + st.1[1]) / 2.0];
        let mut placed = false;
        for (dx, dy) in [(st.1[0] - cx[0] + rad + 0.08, 0.0), (-(cx[0] - st.0[0]) - rad - 0.08, 0.0), (0.0, st.1[1] - cx[1] + rad + 0.08), (0.0, -(cx[1] - st.0[1]) - rad - 0.08)] {
            let c = [cx[0] + dx, cx[1] + dy];
            let fz = st.0[2];
            let wb: Box3 = ([c[0] - rad, c[1] - rad, fz], [c[0] + rad, c[1] + rad, fz + hgt]);
            if !fits(&taken, &wb) { continue }
            let wicker = scene.material({ let mut m = Material::diffuse(&format!("basket:{i}"), [0.62, 0.5, 0.33], 0.85); m.spec = Some(json!({"cls": "fabric.CoarseKnitFabric", "kwargs": {}, "seed": r.next_u64() % 1_000_000})); m });
            let mut m = limb([c[0], c[1], fz], [c[0], c[1], fz + hgt], rad * 0.9, rad, true);
            m.append(&limb([c[0], c[1], fz + hgt - 0.02], [c[0], c[1], fz + hgt + 0.01], rad + 0.01, rad + 0.01, true));
            scene.add(&format!("basket_{i}#0"), "BasketFactory", m, wicker);
            taken.push(wb);
            placed = true;
            break;
        }
        if placed { log.push(format!("basket by side table {i}")) }
    }
    log
}
