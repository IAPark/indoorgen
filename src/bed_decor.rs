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

/// half the house's wall thickness (set by the scene build): the walls' inner faces lie this far in from the room outlines
pub static WALL_HALF_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x3FBE_B851_EB85_1EB8); // 0.12
pub fn wall_half() -> f64 { f64::from_bits(WALL_HALF_BITS.load(std::sync::atomic::Ordering::Relaxed)) }

pub fn add(scene: &mut Scene, state: &State, room_polys: &[(String, crate::shp::G)], keepout: &[([f64; 3], [f64; 3], f64)], assets: &crate::assets::RealAssets, seed: u64) -> Vec<String> {
    let mut log = vec![];
    let inner: Vec<crate::shp::G> = room_polys.iter().map(|(_, g)| crate::shp::buffer_mitre(g, -0.18)).collect();
    let mut taken: Vec<Box3> = state.objs.values().filter(|o| o.generator.is_some()).map(|o| world_box(o, o.geom.lo, o.geom.hi)).collect();
    // (and the wardrobes as built: an open door stands outside the solver box, and a bench at the bed's foot went into one)
    for (k, o) in &state.objs {
        if o.generator.as_ref().map_or(true, |g| g.class != "WardrobeFactory") { continue }
        let pre = format!("{k}#");
        taken.extend(scene.nodes.iter().filter(|n| n.name.starts_with(&pre) && !n.mesh.is_empty()).map(|n| n.mesh.bounds()));
    }
    // (and the gallery frames already hung: scene-only, a wall shelf went up into one)
    // (grown 3 cm: the shelf's probe is shrunk 2 cm for touching furniture, and a frame is only 2.5 cm deep)
    taken.extend(scene.nodes.iter().filter(|n| n.name.starts_with("gallery_") && !n.mesh.is_empty()).map(|n| { let (l, h) = n.mesh.bounds(); ([l[0] - 0.03, l[1] - 0.03, l[2] - 0.03], [h[0] + 0.03, h[1] + 0.03, h[2] + 0.03]) }));
    taken.extend(scene.nodes.iter().filter(|n| ["mess_", "dress_", "cushion_", "pendant_", "gallery_"].iter().any(|p| n.name.starts_with(p))).map(|n| n.mesh.bounds()));
    taken.extend(keepout.iter().map(|(c, h, _)| ([c[0] - h[0], c[1] - h[1], 0.0], [c[0] + h[0], c[1] + h[1], 3.0])));
    // windows and doors (cutters, no generator) with their frames: nothing hangs over them
    taken.extend(state.objs.iter().filter(|(k, o)| o.generator.is_none() && ["window", "door", "entrance", "passage"].iter().any(|p| k.starts_with(p)))
        .map(|(_, o)| { let (l, h) = world_box(o, o.geom.lo, o.geom.hi); ([l[0] - 0.08, l[1] - 0.08, l[2] - 0.08], [h[0] + 0.08, h[1] + 0.08, h[2] + 0.08]) }));
    // (what is really there: the dressers' shared occupancy -- every mesh placed so far and the door passages. Each dresser kept its own list,
    // and a bench's blanket went into a wardrobe's door)
    let solid = crate::dress::Occ::from_scene(scene, &[]);
    let clear_of_others = |k: &str, b: &Box3| { let own = format!("{k}#"); !solid.boxes.iter().zip(solid.names.iter()).any(|(t, n)| !n.starts_with(&own) && overlaps(b, t)) };
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
            // (up to the blanket folded on it: the box stopped at the seat, and a blanket went into a wardrobe's door above it)
            let wb = world_box(o, [x0, yc - bw / 2.0, fz], [x1, yc + bw / 2.0, fz + bh + 0.08]);
            if fits(&others, &wb) && clear_of_others(k, &wb) {
                let cls = ["fabric.SofaFabric", "fabric.Velvet", "fabric.Leather", "fabric.CoarseKnitFabric"][r.randint(0, 4) as usize];
                let fm = fab(scene, &format!("bench:{key}"), cls, r.next_u64(), [0.55, 0.5, 0.45]);
                let legm = fab(scene, &format!("bench_leg:{key}"), "wood.Wood", r.next_u64(), [0.35, 0.25, 0.15]);
                let mut top = crate::assets::sofa::pad([x0, yc - bw / 2.0, fz + bh - 0.12], [x1, yc + bw / 2.0, fz + bh], 0.035, 2);
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
                let mut bl = crate::assets::sofa::pad([x0 + 0.03, by - 0.18, fz + bh], [x1 - 0.03, by + 0.18, fz + bh + 0.07], 0.025, 2);
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
            if fits(&others, &wb) && clear_of_others(k, &wb) {
                let pal: [[f32; 3]; 5] = [[0.85, 0.82, 0.78], [0.2, 0.22, 0.3], [0.6, 0.45, 0.45], [0.45, 0.45, 0.45], [0.55, 0.4, 0.3]];
                let sm = fab(scene, &format!("slipper:{key}"), "fabric.FineKnitFabric", r.next_u64(), pal[r.randint(0, 5) as usize]);
                let yaw = r.uniform(-0.4, 0.4);
                let mut m = Mesh::new();
                for dy in [-0.055, 0.055] {
                    let mut s = slipper(dy);
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
    // ---- wall shelf above nightstands and desks (70 %): books, a vase or small plant, a framed photo. The wall is found by marching from
    // the object's centre along its back axis (-x of its pose) to the room's inner outline.
    // (the walls' inner faces: a fixed 0.12 m in from the centre line put shelves 3-6 cm into thick walls)
    let wall_inner: Vec<crate::shp::G> = room_polys.iter().map(|(_, g)| crate::shp::buffer_mitre(g, -(wall_half() + 0.005))).collect();
    for (k, o) in &state.objs {
        let Some(g) = &o.generator else { continue };
        if !matches!(g.class.as_str(), "SideTableFactory" | "SimpleDeskFactory" | "SidetableDeskFactory") { continue }
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "wall_shelf", k]));
        let roll = r.f64();
        if std::env::var_os("WSHELF_DEBUG").is_some() { eprintln!("WSHELF cand {k} roll {roll:.2}"); }
        if roll >= 0.7 { continue }
        let wb = world_box(o, o.geom.lo, o.geom.hi);
        let c = [(wb.0[0] + wb.1[0]) / 2.0, (wb.0[1] + wb.1[1]) / 2.0];
        let Some(ri) = wall_inner.iter().position(|g| crate::shp::contains_pt(g, c)) else { if std::env::var_os("WSHELF_DEBUG").is_some() { eprintln!("WSHELF {k} centre not in room"); } continue };
        // nearest wall along the object's 4 local horizontal axes (nightstands are not always posed with their back to the wall)
        let o0 = o.pose.apply([0.0, 0.0, 0.0]);
        let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
        for ax in [[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]] {
            let oa = o.pose.apply([ax[0], ax[1], 0.0]);
            let dv = { let (dx, dy) = (oa[0] - o0[0], oa[1] - o0[1]); let n = dx.hypot(dy).max(1e-9); [dx / n, dy / n] };
            for st in 0..80 {
                let dd = st as f64 * 0.01;
                let q = [c[0] + dv[0] * dd, c[1] + dv[1] * dd];
                if !crate::shp::contains_pt(&wall_inner[ri], q) {
                    if best.map_or(true, |b| dd < b.0) { best = Some((dd, q, dv)); }
                    break;
                }
            }
        }
        let wall = best.map(|b| b.1);
        let dir = best.map_or([1.0, 0.0], |b| b.2);
        let Some(wq) = wall else { continue };
        let along = [-dir[1], dir[0]];
        let span = (wb.1[0] - wb.0[0]).max(wb.1[1] - wb.0[1]);
        let mut sw = (span * r.uniform(0.9, 1.3)).clamp(0.5, 1.2);
        let sd = 0.22;
        let sz = wb.1[2] + r.uniform(0.75, 0.9); // above a desk lamp
        // shelf board in world space: corners from the wall point
        let pt = |u: f64, t: f64| [wq[0] + along[0] * u - dir[0] * t, wq[1] + along[1] * u - dir[1] * t];
        // (within the room: a shelf by a corner ran on into the side wall) -- narrowed until its front corners are inside, else none
        let inside = |w: f64| [pt(-w / 2.0 - 0.03, sd), pt(w / 2.0 + 0.03, sd), pt(-w / 2.0 - 0.03, 0.02), pt(w / 2.0 + 0.03, 0.02)].iter().all(|q| crate::shp::contains_pt(&wall_inner[ri], *q));
        while sw > 0.4 && !inside(sw) { sw -= 0.05; }
        if !inside(sw) { continue }
        let shelf_box: Box3 = { let ps = [pt(-sw / 2.0, 0.0), pt(sw / 2.0, 0.0), pt(-sw / 2.0, sd), pt(sw / 2.0, sd)]; ([ps.iter().map(|p| p[0]).fold(f64::MAX, f64::min), ps.iter().map(|p| p[1]).fold(f64::MAX, f64::min), sz], [ps.iter().map(|p| p[0]).fold(f64::MIN, f64::max), ps.iter().map(|p| p[1]).fold(f64::MIN, f64::max), sz + 0.4]) };
        // 5 mm tolerance: neighbouring furniture boxes touch the shelf's footprint (at 2 cm a shelf stood 1.6 cm into a wardrobe)
        let probe: Box3 = ([shelf_box.0[0] + 0.005, shelf_box.0[1] + 0.005, shelf_box.0[2] + 0.005], [shelf_box.1[0] - 0.005, shelf_box.1[1] - 0.005, shelf_box.1[2] - 0.005]);
        let blocker = taken.iter().find(|t| overlaps(&probe, t));
        if std::env::var_os("WSHELF_DEBUG").is_some() { eprintln!("WSHELF {k} wall {wq:?} box {shelf_box:?} blocked {:?}", blocker); }
        if blocker.is_some() { continue }
        let wood = fab(scene, &format!("wshelf:{k}"), "wood.Wood", r.next_u64(), [0.5, 0.36, 0.22]);
        let mut board = Mesh::new();
        let (a0, a1, b1, b0) = (pt(-sw / 2.0, 0.0), pt(sw / 2.0, 0.0), pt(sw / 2.0, sd), pt(-sw / 2.0, sd));
        let v = [[a0[0], a0[1], sz], [a1[0], a1[1], sz], [b1[0], b1[1], sz], [b0[0], b0[1], sz], [a0[0], a0[1], sz + 0.025], [a1[0], a1[1], sz + 0.025], [b1[0], b1[1], sz + 0.025], [b0[0], b0[1], sz + 0.025]];
        for f in [[0, 1, 2, 3], [7, 6, 5, 4], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]] { board.add_face(&[v[f[0]], v[f[1]], v[f[2]], v[f[3]]]); }
        let key = k.replace('/', "_");
        scene.add(&format!("wshelf_{key}#0"), "WallShelfFactory", board, wood);
        // items left to right along the shelf
        let yaw = dir[1].atan2(dir[0]) + std::f64::consts::PI; // item front faces the room
        let mut u = -sw / 2.0 + 0.03;
        let mut n = 0;
        while u < sw / 2.0 - 0.08 && n < 5 {
            let pick = r.f64();
            let cls = if pick < 0.45 { "BookColumnFactory" } else if pick < 0.65 { "VaseFactory" } else if pick < 0.8 { "PlantContainerFactory" } else { "Frame" };
            if cls == "Frame" {
                let (fw, fh) = (r.uniform(0.12, 0.2), r.uniform(0.15, 0.25));
                if u + fw > sw / 2.0 - 0.02 { break }
                let fm = fab(scene, &format!("photo_frame:{k}:{n}"), "wood.Wood", r.next_u64(), [0.3, 0.22, 0.15]);
                let art = fab(scene, &format!("photo:{k}:{n}"), "art.Art", r.next_u64(), [0.5, 0.5, 0.5]);
                // (bottom 9 cm out, leaning 5 cm toward the wall: an 8 cm lean from 5 cm put its top into the wall)
                let (p0, p1) = (pt(u, 0.09), pt(u + fw, 0.09));
                let lean = 0.05;
                let fr = [[p0[0], p0[1], sz + 0.025], [p1[0], p1[1], sz + 0.025], [p1[0] + dir[0] * lean, p1[1] + dir[1] * lean, sz + 0.025 + fh], [p0[0] + dir[0] * lean, p0[1] + dir[1] * lean, sz + 0.025 + fh]];
                let mut m = Mesh::new(); m.add_face(&[fr[0], fr[1], fr[2], fr[3]]);
                let mut back = m.clone();
                for q in back.pos.iter_mut() { q[0] += dir[0] * 0.012; q[1] += dir[1] * 0.012; }
                m.append(&back.flipped());
                scene.add(&format!("photo_{key}_{n}#0"), "PhotoFrameFactory", m.clone(), fm);
                let mut inner = Mesh::new();
                let shrink = |p: [f64; 3], c: [f64; 3]| [c[0] + (p[0] - c[0]) * 0.8 - dir[0] * 0.002, c[1] + (p[1] - c[1]) * 0.8 - dir[1] * 0.002, c[2] + (p[2] - c[2]) * 0.8];
                let cc = [(fr[0][0] + fr[2][0]) / 2.0, (fr[0][1] + fr[2][1]) / 2.0, (fr[0][2] + fr[2][2]) / 2.0];
                inner.add_face(&[shrink(fr[0], cc), shrink(fr[1], cc), shrink(fr[2], cc), shrink(fr[3], cc)]);
                scene.add(&format!("photo_{key}_{n}#1"), "PhotoFrameFactory", inner, art);
                u += fw + 0.04; n += 1;
                continue;
            }
            let item = assets.visual(cls, r.next_u64() % 1_000_000, 0);
            let (ilo, ihi) = item.bounds();
            let (iw, idp, ih) = (ihi[0] - ilo[0], ihi[1] - ilo[1], ihi[2] - ilo[2]);
            // (fits the shelf's depth whichever way it turns: scaled by its thin side, a book turned lengthwise reached into the wall)
            let s = (0.32 / ih.max(0.05)).min((sd - 0.04) / iw.max(idp).max(0.05)).min(1.0);
            let wlen = iw.max(idp) * s;
            if u + wlen > sw / 2.0 - 0.02 { break }
            let c2 = pt(u + wlen / 2.0, sd / 2.0);
            let (cx, cy) = ((ilo[0] + ihi[0]) / 2.0, (ilo[1] + ihi[1]) / 2.0);
            for (pi, p) in item.parts.iter().enumerate() {
                let mut m = p.mesh.clone();
                for q in m.pos.iter_mut() {
                    let (dx, dy) = ((q[0] - cx) * s, (q[1] - cy) * s);
                    *q = [c2[0] + dx * yaw.cos() - dy * yaw.sin(), c2[1] + dx * yaw.sin() + dy * yaw.cos(), sz + 0.025 + (q[2] - ilo[2]) * s];
                }
                let mi = scene.material(p.mat.clone());
                scene.add(&format!("wsitem_{key}_{n}#{pi}"), cls, m, mi);
            }
            u += wlen + 0.03; n += 1;
        }
        taken.push(shelf_box);
        log.push(format!("wall shelf ({n} items) over {k}"));
    }
    // ---- laundry basket beside a nightstand (one per bedroom), 50 %
    // (one per room: every nightstand drew its own and a bed had one each side; and against the wall beside the nightstand, the spot
    // nearest a wall: the first free side was often its front, the basket blocking the drawers)
    let mut basket_rooms: Vec<String> = vec![];
    for (i, st) in side_tables.iter().enumerate() {
        let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "basket", &i.to_string()]));
        if r.f64() >= 0.5 { continue }
        let (rad, hgt) = (r.uniform(0.18, 0.23), r.uniform(0.45, 0.6));
        let cx = [(st.0[0] + st.1[0]) / 2.0, (st.0[1] + st.1[1]) / 2.0];
        let Some((rk, rp)) = room_polys.iter().find(|(_, g)| crate::shp::contains_pt(g, cx)) else { continue };
        if basket_rooms.contains(rk) { continue }
        let edge = crate::shp::boundary(rp);
        // its back: the direction (of the four) toward the nearest wall; the basket goes to either side of that, never in front
        let dirs = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]];
        let back = dirs.iter().cloned().min_by(|a, b| { let d = |v: [f64; 2]| crate::shp::distance(&edge, &crate::shp::point(cx[0] + v[0] * 0.3, cx[1] + v[1] * 0.3)); d(*a).partial_cmp(&d(*b)).unwrap() }).unwrap();
        let mut cands: Vec<(f64, [f64; 2])> = [(st.1[0] - cx[0] + rad + 0.08, 0.0), (-(cx[0] - st.0[0]) - rad - 0.08, 0.0), (0.0, st.1[1] - cx[1] + rad + 0.08), (0.0, -(cx[1] - st.0[1]) - rad - 0.08)]
            .iter().filter(|(dx, dy)| (dx * back[0] + dy * back[1]).abs() < 1e-9)
            .map(|(dx, dy)| { let c = [cx[0] + dx, cx[1] + dy]; (crate::shp::distance(&edge, &crate::shp::point(c[0], c[1])), c) }).collect();
        cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut placed = false;
        for (_, c) in cands {
            let fz = st.0[2];
            let wb: Box3 = ([c[0] - rad, c[1] - rad, fz], [c[0] + rad, c[1] + rad, fz + hgt]);
            if !crate::shp::contains_pt(rp, c) || !fits(&taken, &wb) || !clear_of_others("", &wb) { continue }
            let wicker = scene.material({ let mut m = Material::diffuse(&format!("basket:{i}"), [0.62, 0.5, 0.33], 0.85); m.spec = Some(json!({"cls": "fabric.CoarseKnitFabric", "kwargs": {}, "seed": r.next_u64() % 1_000_000})); m });
            let mut m = limb([c[0], c[1], fz], [c[0], c[1], fz + hgt], rad * 0.9, rad, true);
            m.append(&limb([c[0], c[1], fz + hgt - 0.02], [c[0], c[1], fz + hgt + 0.01], rad + 0.01, rad + 0.01, true));
            scene.add(&format!("basket_{i}#0"), "BasketFactory", m, wicker);
            taken.push(wb);
            placed = true;
            break;
        }
        if placed { basket_rooms.push(rk.clone()); log.push(format!("basket by side table {i}")) }
    }
    log
}

/// one slip-on slipper, toe to +x: a soft sole and a padded arch over the front, open at the heel side
pub fn slipper(dy: f64) -> Mesh {
    let (top, x0, xe, w0, h0) = (0.018, -0.03, 0.122, 0.043, 0.05);
    let mut m = crate::assets::sofa::pad([-0.13, dy - 0.045, 0.0], [0.13, dy + 0.045, top + 0.006], 0.008, 2);
    let n = 18;
    let arch = |x: f64, scale: f64| -> Vec<[f64; 3]> {
        let u = ((x - x0) / (xe - x0)).clamp(0.0, 1.0);
        let f = (1.0 - u * u).max(0.0);
        (0..=n).map(|i| { let a = std::f64::consts::PI * i as f64 / n as f64; [x, dy + scale * w0 * f.powf(0.35) * a.cos(), top + scale * h0 * f.powf(0.6) * a.sin()] }).collect()
    };
    // the outside, from the opening to the toe, then the opening's rolled rim and the inside back into it
    let mut rings: Vec<Vec<[f64; 3]>> = vec![arch(x0 + 0.05, 0.78), arch(x0 + 0.015, 0.8), arch(x0 - 0.002, 0.82), arch(x0 - 0.002, 0.92)];
    rings.push(arch(x0, 1.0));
    for k in 1..=12 { rings.push(arch(x0 + (xe - x0) * (k as f64 / 12.0).min(0.995), 1.0)); }
    for k in 0..rings.len() - 1 {
        for i in 0..n { m.add_quad(rings[k][i], rings[k][i + 1], rings[k + 1][i + 1], rings[k + 1][i]); }
    }
    m
}
