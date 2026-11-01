//! cornerA / cornerB camera picking, ported from tools/render/viewcam.py (same spots, tests and ray-scored heading search) so the
//! picks can be iterated in ~1 s instead of a Blender start per view; the chosen cameras go to Blender with --dump-cams / --cam.
//! All in the scene's Z-up frame, on the glb's world-space triangles (instanced outdoor nodes are not part of a room).
use crate::cams::{cam_look, Cam, SceneInfo};
use bevy::math::Vec3;
use std::collections::HashMap;

type P2 = (f64, f64);

fn hull(pts: &[P2]) -> Vec<P2> {
    let mut p: Vec<P2> = pts.to_vec();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    p.dedup();
    if p.len() < 3 { return p }
    let cr = |o: P2, a: P2, b: P2| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let (mut lo, mut up): (Vec<P2>, Vec<P2>) = (vec![], vec![]);
    for &q in &p { while lo.len() >= 2 && cr(lo[lo.len() - 2], lo[lo.len() - 1], q) <= 1e-9 { lo.pop(); } lo.push(q); }
    for &q in p.iter().rev() { while up.len() >= 2 && cr(up[up.len() - 2], up[up.len() - 1], q) <= 1e-9 { up.pop(); } up.push(q); }
    lo.pop(); up.pop(); lo.extend(up); lo
}

fn simplify(mut h: Vec<P2>, min_dev: f64) -> Vec<P2> {
    while h.len() > 3 {
        let n = h.len();
        let dev = |i: usize| { let (a, b, c) = (h[(i + n - 1) % n], h[i], h[(i + 1) % n]); let l = (c.0 - a.0).hypot(c.1 - a.1).max(1e-9); ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / l };
        let i = (0..n).min_by(|&a, &b| dev(a).partial_cmp(&dev(b)).unwrap()).unwrap();
        if dev(i) >= min_dev { break }
        h.remove(i);
    }
    h
}

fn ray_box(o: P2, d: P2, b: [f64; 4], tmax: f64) -> bool {
    let (mut t0, mut t1) = (0.0f64, tmax);
    for (k, (lo, hi)) in [(b[0], b[1]), (b[2], b[3])].into_iter().enumerate() {
        let (ok, dk) = if k == 0 { (o.0, d.0) } else { (o.1, d.1) };
        if dk.abs() < 1e-12 { if !(lo <= ok && ok <= hi) { return false } }
        else { let (mut a, mut c) = ((lo - ok) / dk, (hi - ok) / dk); if a > c { std::mem::swap(&mut a, &mut c); } t0 = t0.max(a); t1 = t1.min(c); if t0 > t1 { return false } }
    }
    true
}
fn ray_seg(o: P2, d: P2, p: P2, q: P2, tmax: f64) -> bool {
    let (ex, ey) = (q.0 - p.0, q.1 - p.1);
    let den = d.0 * ey - d.1 * ex;
    if den.abs() < 1e-12 { return false }
    let t = ((p.0 - o.0) * ey - (p.1 - o.1) * ex) / den;
    let s = ((p.0 - o.0) * d.1 - (p.1 - o.1) * d.0) / den;
    1e-6 < t && t < tmax && (0.0..=1.0).contains(&s)
}

/// triangle BVH (median split) for the probe rays
struct Bvh { tris: Vec<[Vec3; 3]>, owner: Vec<usize>, nodes: Vec<(Vec3, Vec3, u32, u32, u32)> } // (lo, hi, left/first, right/count, leaf flag)
impl Bvh {
    fn new(tris: Vec<[Vec3; 3]>, owner: Vec<usize>) -> Bvh {
        let mut idx: Vec<u32> = (0..tris.len() as u32).collect();
        let mut b = Bvh { tris: vec![], owner: vec![], nodes: vec![] };
        let cent: Vec<Vec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        fn build(b: &mut Bvh, tris: &[[Vec3; 3]], cent: &[Vec3], idx: &mut [u32], start: usize) -> u32 {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for &i in idx.iter() { for v in tris[i as usize] { lo = lo.min(v); hi = hi.max(v); } }
            let me = b.nodes.len() as u32;
            b.nodes.push((lo, hi, 0, 0, 0));
            if idx.len() <= 8 {
                b.nodes[me as usize] = (lo, hi, start as u32, idx.len() as u32, 1);
                return me;
            }
            let ext = hi - lo;
            let ax = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
            idx.sort_by(|&a, &c| cent[a as usize][ax].partial_cmp(&cent[c as usize][ax]).unwrap());
            let mid = idx.len() / 2;
            let (l, r) = idx.split_at_mut(mid);
            let li = build(b, tris, cent, l, start);
            let ri = build(b, tris, cent, r, start + mid);
            b.nodes[me as usize] = (lo, hi, li, ri, 0);
            me
        }
        if !tris.is_empty() { build(&mut b, &tris, &cent, &mut idx, 0); }
        b.tris = idx.iter().map(|&i| tris[i as usize]).collect();
        b.owner = idx.iter().map(|&i| owner[i as usize]).collect();
        b
    }
    /// nearest hit within `dist`: (t, normal, owner)
    fn cast(&self, o: Vec3, d: Vec3, dist: f32) -> Option<(f32, Vec3, usize)> {
        if self.nodes.is_empty() { return None }
        let inv = Vec3::ONE / d;
        let mut best: Option<(f32, Vec3, usize)> = None;
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let (lo, hi, a, b, leaf) = self.nodes[ni as usize];
            let (t1, t2) = ((lo - o) * inv, (hi - o) * inv);
            let tmin = t1.min(t2).max_element().max(0.0);
            let tmax = t1.max(t2).min_element().min(best.map_or(dist, |x| x.0));
            if tmin > tmax { continue }
            if leaf == 1 {
                for k in a..a + b {
                    let [p0, p1, p2] = self.tris[k as usize];
                    let (e1, e2) = (p1 - p0, p2 - p0);
                    let h = d.cross(e2); let det = e1.dot(h);
                    if det.abs() < 1e-12 { continue }
                    let f = 1.0 / det; let s = o - p0; let u = f * s.dot(h);
                    if !(0.0..=1.0).contains(&u) { continue }
                    let q = s.cross(e1); let v = f * d.dot(q);
                    if v < 0.0 || u + v > 1.0 { continue }
                    let t = f * e2.dot(q);
                    if t > 1e-5 && t < best.map_or(dist, |x| x.0) { best = Some((t, e1.cross(e2).normalize_or_zero(), self.owner[k as usize])); }
                }
            } else { stack.push(a); stack.push(b); }
        }
        best
    }
}

/// cornerA (`which` 0) / cornerB (1) of `room`, as viewcam.py picks them; None when the room has no floor
pub fn corner_cam(s: &SceneInfo, room: &str, which: usize, verbose: bool) -> Option<Cam> {
    let g = &s.glb;
    let name = |di: usize| g.nodes[s.draws[di].node].name.as_str();
    let tris_of = |di: usize| -> Vec<[Vec3; 3]> {
        let d = &s.draws[di];
        let pb = g.bytes(&d.pos); let ib = g.bytes(&d.idx);
        let v = |i: usize| { let r = |k: usize| f32::from_le_bytes(pb[(i * 3 + k) * 4..(i * 3 + k) * 4 + 4].try_into().unwrap()); Vec3::new(r(0), r(1), r(2)) };
        let ix = |k: usize| if d.u16 { u16::from_le_bytes([ib[k * 2], ib[k * 2 + 1]]) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize };
        (0..d.icount as usize / 3).map(|t| [v(ix(3 * t)), v(ix(3 * t + 1)), v(ix(3 * t + 2))]).collect()
    };
    // floor: area centroid, outline = simplified hull of its vertices
    let (mut area, mut ax, mut ay) = (0.0f64, 0.0f64, 0.0f64);
    let mut ftris: Vec<[P2; 3]> = vec![];
    let mut pts: Vec<P2> = vec![];
    for di in 0..s.draws.len() {
        let n = name(di);
        if !(n.starts_with(room) && n.contains(".floor")) { continue }
        for t in tris_of(di) {
            let p: Vec<P2> = t.iter().map(|v| (v.x as f64, v.y as f64)).collect();
            let a = ((p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1)).abs() / 2.0;
            area += a; ax += (p[0].0 + p[1].0 + p[2].0) / 3.0 * a; ay += (p[0].1 + p[1].1 + p[2].1) / 3.0 * a;
            ftris.push([p[0], p[1], p[2]]);
            for q in p { pts.push(((q.0 * 1e4).round() / 1e4, (q.1 * 1e4).round() / 1e4)); }
        }
    }
    if area <= 0.0 { eprintln!("viewpick: no floor for {room}"); return None }
    let cen = (ax / area, ay / area);
    let outline = simplify(hull(&pts), 0.25);
    // furniture boxes (grouped per asset: "<id>#<k>"), door leaves
    let skip = [".floor", ".wall", ".ceiling", "skirting", "exterior", "entrance", "placeholder", "vc", "portal", "window", "door", "outdoor"];
    let mut groups: HashMap<String, [f64; 4]> = HashMap::new();
    let mut leaves: Vec<[f64; 4]> = vec![];
    let leaf_re = |n: &str| { let n = n.to_lowercase(); n.strip_prefix("door").map_or(false, |r| { let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect(); !digits.is_empty() && (r.len() == digits.len() || r[digits.len()..].starts_with('#')) }) };
    let key = |n: &str| n.split('#').next().unwrap_or(n).to_string();
    for (di, d) in s.draws.iter().enumerate() {
        let n = name(di); let nl = n.to_lowercase();
        let bb = [d.bmin.x as f64, d.bmax.x as f64, d.bmin.y as f64, d.bmax.y as f64];
        if leaf_re(n) { leaves.push(bb); }
        if skip.iter().any(|k| nl.contains(k)) || n.ends_with(".meshed") { continue }
        if d.bmin.z >= 1.9 || d.bmax.z <= 0.3 { continue }
        let gb = groups.entry(key(n)).or_insert([9e9, -9e9, 9e9, -9e9]);
        gb[0] = gb[0].min(bb[0]); gb[1] = gb[1].max(bb[1]); gb[2] = gb[2].min(bb[2]); gb[3] = gb[3].max(bb[3]);
    }
    let boxes: Vec<[f64; 4]> = groups.values().copied().collect();
    let inside_b = |b: &[f64; 4], q: P2| b[0] <= q.0 && q.0 <= b[1] && b[2] <= q.1 && q.1 <= b[3];
    let blocked = |q: P2| boxes.iter().any(|b| b[0] - 0.8 < q.0 && q.0 < b[1] + 0.8 && b[2] - 0.8 < q.1 && q.1 < b[3] + 0.8);
    let clearance = |q: P2| boxes.iter().chain(leaves.iter()).map(|b| (b[0] - q.0).max(0.0).max(q.0 - b[1]).hypot((b[2] - q.1).max(0.0).max(q.1 - b[3]))).fold(9.0, f64::min);
    let rot = |q: P2, deg: f64| { let (fx, fy) = (cen.0 - q.0, cen.1 - q.1); let l = fx.hypot(fy).max(1e-9); let a = deg.to_radians(); ((fx * a.cos() - fy * a.sin()) / l, (fx * a.sin() + fy * a.cos()) / l) };
    let openness = |q: P2| (-4..=4).filter(|k| {
        let d = rot(q, 10.0 * *k as f64);
        let hit = boxes.iter().chain(leaves.iter()).any(|b| !inside_b(b, q) && ray_box(q, d, *b, 2.0))
            || (0..outline.len()).any(|i| ray_seg(q, d, outline[(i + outline.len() - 1) % outline.len()], outline[i], 2.0));
        !hit
    }).count();
    let content = |q: P2| (-8..=8).filter(|k| {
        let d = rot(q, 5.0 * *k as f64);
        let far = boxes.iter().any(|b| !inside_b(b, q) && ray_box(q, d, *b, 5.0));
        let near = boxes.iter().any(|b| !inside_b(b, q) && ray_box(q, d, *b, 1.2));
        far && !near
    }).count();
    let leaf_near = |q: P2| (-10..=10).any(|k| { let d = rot(q, 5.0 * k as f64); leaves.iter().any(|b| ray_box(q, d, *b, 1.5)) });
    let in_tri = |t: &[P2; 3], p: P2| {
        let ((ax_, ay_), (bx_, by_), (cx_, cy_)) = (t[0], t[1], t[2]);
        let d1 = (p.0 - bx_) * (ay_ - by_) - (ax_ - bx_) * (p.1 - by_); let d2 = (p.0 - cx_) * (by_ - cy_) - (bx_ - cx_) * (p.1 - cy_); let d3 = (p.0 - ax_) * (cy_ - ay_) - (cx_ - ax_) * (p.1 - ay_);
        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
    };
    let on_floor = |q: P2| [(0.0, 0.0), (0.3, 0.0), (-0.3, 0.0), (0.0, 0.3), (0.0, -0.3)].iter().all(|(dx, dy)| ftris.iter().any(|t| in_tri(t, (q.0 + dx, q.1 + dy))));
    let cz = s.draws.iter().enumerate().filter(|(di, _)| { let n = name(*di); n.starts_with(room) && n.contains(".ceiling") }).map(|(_, d)| d.bmax.z).fold(f32::MIN, f32::max);
    // the room's own geometry within 1 m of its outline (not outdoor) for the probe rays
    let (rx0, rx1) = (outline.iter().map(|p| p.0).fold(f64::MAX, f64::min) - 1.0, outline.iter().map(|p| p.0).fold(f64::MIN, f64::max) + 1.0);
    let (ry0, ry1) = (outline.iter().map(|p| p.1).fold(f64::MAX, f64::min) - 1.0, outline.iter().map(|p| p.1).fold(f64::MIN, f64::max) + 1.0);
    let (mut tris, mut owner) = (vec![], vec![]);
    for (di, d) in s.draws.iter().enumerate() {
        if name(di).to_lowercase().contains("outdoor") { continue }
        if (d.bmax.x as f64) < rx0 || (d.bmin.x as f64) > rx1 || (d.bmax.y as f64) < ry0 || (d.bmin.y as f64) > ry1 { continue }
        for t in tris_of(di) { tris.push(t); owner.push(di); }
    }
    let bvh = Bvh::new(tris, owner);
    let headroom = |q: P2| cz == f32::MIN || bvh.cast(Vec3::new(q.0 as f32, q.1 as f32, 1.5), Vec3::Z, 10.0).map_or(true, |(t, _, _)| 1.5 + t > cz - 0.1);
    // candidate spots: the 6 outline corners farthest from the centre, then the long walls' midpoints; 0.45 / 1.0 / 1.4 m in
    let mut cands: Vec<P2> = outline.clone();
    cands.sort_by(|a, b| (b.0 - cen.0).hypot(b.1 - cen.1).partial_cmp(&(a.0 - cen.0).hypot(a.1 - cen.1)).unwrap());
    cands.truncate(6);
    let n = outline.len();
    for i in 0..n { let (a, b) = (outline[(i + n - 1) % n], outline[i]); if (b.0 - a.0).hypot(b.1 - a.1) > 1.5 { cands.push(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)); } }
    let mut spots: Vec<P2> = vec![];
    for p in &cands {
        let (dx, dy) = (cen.0 - p.0, cen.1 - p.1); let l = dx.hypot(dy).max(1.0e-9);
        for inset in [0.45, 1.0, 1.4] {
            let q = (p.0 + dx / l * inset, p.1 + dy / l * inset);
            if inset >= l - 0.3 || !on_floor(q) || !headroom(q) { continue }
            let _ = (blocked(q), openness(q), clearance(q), leaf_near(q), content(q)); // (viewcam.py computes these per spot too; the ray score decides)
            spots.push(q);
        }
    }
    let fixed = ["wall", "floor", "ceiling", "skirting", "exterior", "entrance", "window", "portal", "door", "frame", "light", "outdoor"];
    let view_score = |q: P2, yaw: f64| -> f64 {
        let (fx, fy) = (cen.0 - q.0, cen.1 - q.1); let l = fx.hypot(fy).max(1e-9);
        let (c_, s_) = (yaw.cos(), yaw.sin());
        let (ux, uy) = ((fx * c_ - fy * s_) / l, (fx * s_ + fy * c_) / l);
        let o = Vec3::new(q.0 as f32, q.1 as f32, 1.5);
        let fwd = (Vec3::new((q.0 + ux * 3.0) as f32, (q.1 + uy * 3.0) as f32, 0.75) - o).normalize();
        let right = fwd.cross(Vec3::Z).normalize(); let up = right.cross(fwd);
        let (mut furn, mut near, mut leaf, mut wall, mut nr) = (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut cols: HashMap<String, (usize, usize)> = HashMap::new();
        let mut close: HashMap<String, (usize, usize)> = HashMap::new();
        let mut slab: HashMap<String, usize> = HashMap::new();
        for i in 0..24 {
            for j in 0..14 {
                let d = (fwd + right * (((i as f32 + 0.5) / 24.0) * 2.0 - 1.0) + up * ((((j as f32 + 0.5) / 14.0) * 2.0 - 1.0) * 0.5625)).normalize();
                nr += 1;
                let Some((t, nrm, di)) = bvh.cast(o, d, 8.0) else { continue };
                let nm = name(di); let nl = nm.to_lowercase();
                if leaf_re(nm) { leaf += 1; continue }
                if t < 1.8 { near += 1; }
                if fixed.iter().any(|k| nl.contains(k)) { if t < 1.8 && nrm.z.abs() < 0.5 { wall += 1; } continue }
                if t < 1.3 { let e = close.entry(key(nm)).or_insert((i, i)); e.0 = e.0.min(i); e.1 = e.1.max(i); }
                // (a furniture side or an open wardrobe door close by fills the frame like a bare wall)
                if t < 1.8 && nrm.z.abs() < 0.5 { wall += 1; }
                if t < 3.0 && nrm.z.abs() < 0.3 { *slab.entry(key(nm)).or_insert(0) += 1; }
                if t <= 5.0 {
                    furn += 1;
                    let e = cols.entry(key(nm)).or_insert((i, i)); e.0 = e.0.min(i); e.1 = e.1.max(i);
                }
            }
        }
        let n = nr as f64;
        let cut = cols.values().filter(|(a, b)| (*a == 0 || *b == 23) && b - a < 6).count() as f64;
        // foreground clutter: a small piece (8 columns or fewer) within 1.3 m of the lens -- a utensil crock filling a kitchen
        // view's corner
        let clutter = close.values().filter(|(a, b)| b - a < 8).count() as f64;
        // one piece's flat vertical side over 18 % of the frame (an open wardrobe door, a fridge flank): it walls the view off
        let slabs: f64 = slab.values().map(|&c| (c as f64 / nr as f64 - 0.18).max(0.0)).sum();
        2.0 * (cols.len() as f64 - cut) + 30.0 * furn as f64 / n - 50.0 * near as f64 / n - 60.0 * leaf as f64 / n - 3.0 * cut - 4.0 * clutter - 40.0 * slabs - 25.0 * wall as f64 / n
    };
    let mut views: Vec<(f64, f64, P2)> = spots.iter().map(|&q| {
        let (sv, yaw) = (-16..=16).map(|k| { let y = (2.5 * k as f64).to_radians(); (view_score(q, y), y) })
            .fold((f64::MIN, 0.0f64), |b, x| if x.0 > b.0 || (x.0 == b.0 && x.1.abs() < b.1.abs()) { x } else { b });
        (sv, yaw, q)
    }).collect();
    if views.is_empty() { eprintln!("viewpick: no usable spot in {room}"); return None }
    if verbose { eprintln!("VIEWCAND {room} {:?}", views.iter().map(|v| (format!("{:.1}", v.0), format!("{:.0}", v.1.to_degrees()), format!("{:.2},{:.2}", v.2 .0, v.2 .1))).collect::<Vec<_>>()); }
    let order: Vec<usize> = { let mut o: Vec<usize> = (0..views.len()).collect(); o.sort_by(|&a, &b| views[b].0.partial_cmp(&views[a].0).unwrap()); o };
    let mut pick = order[0];
    if which == 1 {
        // another viewpoint: 1.2 m or more from cornerA's spot, or 0.6 m and a heading 35 deg or more apart (a small room's other
        // good spots stood just over a metre from A, looking elsewhere; the 1.2 m rule took a wardrobe door instead)
        let head = |v: &(f64, f64, P2)| { let (fx, fy) = (cen.0 - v.2 .0, cen.1 - v.2 .1); fy.atan2(fx) + v.1 };
        let a = &views[order[0]];
        let ok = |i: usize| {
            let b = &views[i];
            let d = (b.2 .0 - a.2 .0).hypot(b.2 .1 - a.2 .1);
            let mut dh = (head(b) - head(a)).abs() % std::f64::consts::TAU; if dh > std::f64::consts::PI { dh = std::f64::consts::TAU - dh; }
            d >= 1.2 || (d >= 0.6 && dh >= 35f64.to_radians())
        };
        pick = order.iter().skip(1).copied().find(|&i| ok(i)).unwrap_or(order[1.min(order.len() - 1)]);
    }
    let (sv, yaw, q) = views.swap_remove(pick);
    let (fx, fy) = (cen.0 - q.0, cen.1 - q.1); let l = fx.hypot(fy).max(1e-9);
    let (ux, uy) = ((fx * yaw.cos() - fy * yaw.sin()) / l, (fx * yaw.sin() + fy * yaw.cos()) / l);
    if verbose { eprintln!("VIEWCAM {room} corner{} ({:.2}, {:.2}) yaw {:.1} score {sv:.1}", if which == 0 { 'A' } else { 'B' }, q.0, q.1, yaw.to_degrees()); }
    let mut c = cam_look(Vec3::new(q.0 as f32, q.1 as f32, 1.5), Vec3::new((q.0 + ux * 3.0) as f32, (q.1 + uy * 3.0) as f32, 0.75), 18.0);
    c.name = format!("{}_corner{}", room.replace('/', "-"), if which == 0 { 'A' } else { 'B' });
    Some(c)
}
