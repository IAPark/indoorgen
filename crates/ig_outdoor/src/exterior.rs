//! House exterior (outdoor stage): what the house looks like from outside -- the rooms' walls are plaster on both faces and the
//! house has no roof. Built from the footprint (the rooms' union, out to the wall's outer face) and the exterior openings (cutters
//! on the outer boundary), for the generated house and for the neighbours' shells (src/neighbours.rs):
//!   siding     lap boards (tilted quads with a shadow lip, `exposure` apart) or board-and-batten, cut around the openings
//!   trim       corner boards, window / door casings with sills, a concrete foundation band, steps at the entrances
//!   roof       the footprint decomposed into rectangles (vertical or horizontal slabs, whichever gives fewer), each with a gable
//!              or hip roof over eaves of `overhang` -- overlapping rectangles give the cross gables of real L / T houses --
//!              fascia boards and a procedural shingle texture
//! Classes "OutdoorHouse" (budget-exempt like the terrain); names "exterior_<house>.<part>".
use ig_core::mesh::{encode_png, Mat4, Material, Mesh, Scene};
use ig_core::rng::Rng;
use ig_rooms::solidify::{CutKind, Cutter};
use std::sync::Arc;

type V3 = [f64; 3];

/// One house's exterior style (seeded per house)
#[derive(Clone, Debug)]
pub struct Style {
    pub siding: [f32; 3],
    pub trim: [f32; 3],
    pub roof: [f32; 3],
    /// lap board exposure (m); 0 = board-and-batten
    pub exposure: f64,
    pub pitch_deg: f64,
    pub hip: bool,
    pub overhang: f64,
}

impl Style {
    pub fn draw(r: &mut Rng) -> Style {
        // painted siding colours of North American / European houses (linear RGB)
        let sidings: [[f32; 3]; 10] = [[0.78, 0.77, 0.72], [0.70, 0.64, 0.50], [0.36, 0.42, 0.47], [0.42, 0.47, 0.38], [0.55, 0.50, 0.42],
            [0.12, 0.13, 0.14], [0.45, 0.12, 0.08], [0.62, 0.60, 0.56], [0.25, 0.30, 0.38], [0.80, 0.72, 0.55]];
        let roofs: [[f32; 3]; 6] = [[0.05, 0.05, 0.055], [0.10, 0.08, 0.06], [0.16, 0.16, 0.16], [0.18, 0.07, 0.05], [0.07, 0.10, 0.08], [0.12, 0.11, 0.10]];
        let siding = sidings[r.randint(0, sidings.len() as i64) as usize];
        let dark = siding.iter().sum::<f32>() < 0.9;
        let trim = if dark || r.f64() < 0.75 { [0.85, 0.85, 0.82] } else { [0.15, 0.15, 0.15] };
        Style {
            siding, trim, roof: roofs[r.randint(0, roofs.len() as i64) as usize],
            exposure: if r.f64() < 0.8 { r.uniform(0.13, 0.2) } else { 0.0 },
            pitch_deg: r.uniform(22.0, 40.0), hip: r.f64() < 0.35, overhang: r.uniform(0.3, 0.55),
        }
    }
}

/// Builder state: one mesh per material
struct Parts { siding: Mesh, trim: Mesh, roof: Mesh, concrete: Mesh }

fn add(a: V3, b: V3) -> V3 { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn mul(a: V3, s: f64) -> V3 { [a[0] * s, a[1] * s, a[2] * s] }

/// wall frame: point at (along a, out d, height z) on the segment from `o` along unit `u` with outward unit `n`
fn wp(o: [f64; 2], u: [f64; 2], n: [f64; 2], a: f64, d: f64, z: f64) -> V3 { [o[0] + u[0] * a + n[0] * d, o[1] + u[1] * a + n[1] * d, z] }

/// an oriented box in a wall frame: along [a0, a1], out [d0, d1], height [z0, z1]
fn wbox(m: &mut Mesh, o: [f64; 2], u: [f64; 2], n: [f64; 2], a: (f64, f64), d: (f64, f64), z: (f64, f64)) {
    let p = |i: usize, j: usize, k: usize| wp(o, u, n, if i == 0 { a.0 } else { a.1 }, if j == 0 { d.0 } else { d.1 }, if k == 0 { z.0 } else { z.1 });
    m.add_quad(p(0, 1, 0), p(1, 1, 0), p(1, 1, 1), p(0, 1, 1)); // front
    m.add_quad(p(1, 0, 0), p(0, 0, 0), p(0, 0, 1), p(1, 0, 1)); // back
    m.add_quad(p(0, 0, 1), p(0, 1, 1), p(1, 1, 1), p(1, 0, 1)); // top
    m.add_quad(p(0, 0, 0), p(1, 0, 0), p(1, 1, 0), p(0, 1, 0)); // bottom
    m.add_quad(p(0, 0, 0), p(0, 1, 0), p(0, 1, 1), p(0, 0, 1)); // a0 end
    m.add_quad(p(1, 1, 0), p(1, 0, 0), p(1, 0, 1), p(1, 1, 1)); // a1 end
}

/// an opening on a wall segment: along [a0, a1], height [z0, z1]
#[derive(Clone, Copy)]
struct Hole { a0: f64, a1: f64, z0: f64, z1: f64, door: bool }

const TRIM_W: f64 = 0.09;
const SIDING_OUT: f64 = 0.025;

/// siding over a wall rectangle (or a gable triangle: `span(z)` gives the along-wall extent at height z), minus the holes
fn siding(m: &mut Mesh, o: [f64; 2], u: [f64; 2], n: [f64; 2], z0: f64, z1: f64, exposure: f64, span: &dyn Fn(f64) -> (f64, f64), holes: &[Hole]) {
    let cut = |lo: f64, hi: f64, za: f64, zb: f64| -> Vec<(f64, f64)> {
        let mut segs = vec![(lo, hi)];
        for h in holes {
            if zb <= h.z0 - TRIM_W || za >= h.z1 + TRIM_W { continue }
            let (c0, c1) = (h.a0 - TRIM_W, h.a1 + TRIM_W);
            segs = segs.into_iter().flat_map(|(s0, s1)| {
                let mut v = vec![];
                if c0 > s0 { v.push((s0, c0.min(s1))); }
                if c1 < s1 { v.push((c1.max(s0), s1)); }
                v
            }).filter(|(s0, s1)| s1 - s0 > 0.02).collect();
        }
        segs
    };
    if exposure > 0.0 {
        // lap siding: each board leans out at its bottom edge, a horizontal lip underneath casts the shadow line
        let mut z = z0;
        while z < z1 - 1e-3 {
            let zt = (z + exposure).min(z1);
            let (lo, hi) = span(zt.min(z1));
            let (lo2, hi2) = span(z);
            let (lo, hi) = (lo.max(lo2), hi.min(hi2));
            if hi - lo > 0.02 {
                for (s0, s1) in cut(lo, hi, z, zt) {
                    m.add_quad(wp(o, u, n, s0, SIDING_OUT, z), wp(o, u, n, s1, SIDING_OUT, z), wp(o, u, n, s1, 0.008, zt), wp(o, u, n, s0, 0.008, zt));
                    m.add_quad(wp(o, u, n, s0, 0.004, z), wp(o, u, n, s1, 0.004, z), wp(o, u, n, s1, SIDING_OUT, z), wp(o, u, n, s0, SIDING_OUT, z));
                }
            }
            z = zt;
        }
    } else {
        // board-and-batten: flat boards with a batten every 0.4 m
        let rows = ((z1 - z0) / 0.3).ceil().max(1.0) as usize;
        for k in 0..rows {
            let (za, zb) = (z0 + (z1 - z0) * k as f64 / rows as f64, z0 + (z1 - z0) * (k + 1) as f64 / rows as f64);
            let (lo, hi) = { let (a, b) = span(za); let (c, d) = span(zb); (a.max(c), b.min(d)) };
            if hi - lo < 0.02 { continue }
            for (s0, s1) in cut(lo, hi, za, zb) {
                m.add_quad(wp(o, u, n, s0, 0.012, za), wp(o, u, n, s1, 0.012, za), wp(o, u, n, s1, 0.012, zb), wp(o, u, n, s0, 0.012, zb));
                let mut a = (s0 / 0.4).ceil() * 0.4;
                while a < s1 - 0.03 { wbox(m, o, u, n, (a - 0.025, a + 0.025), (0.012, 0.03), (za, zb)); a += 0.4; }
            }
        }
    }
}

/// orthogonal footprint -> rectangles (x0, y0, x1, y1): slabs between consecutive vertex coordinates along one axis, merged
/// when neighbouring slabs have the same intervals; the axis giving fewer rectangles wins
fn rectangles(ring: &[[f64; 2]]) -> Vec<[f64; 4]> {
    let slab = |ax: usize| -> Vec<[f64; 4]> {
        let oy = 1 - ax;
        let mut xs: Vec<f64> = ring.iter().map(|p| p[ax]).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let mut out: Vec<[f64; 4]> = vec![];
        let mut open: Vec<(f64, f64, f64)> = vec![]; // (y0, y1, x start)
        for w in xs.windows(2) {
            let xm = 0.5 * (w[0] + w[1]);
            let mut ys: Vec<f64> = vec![];
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                if (a[ax] - xm) * (b[ax] - xm) < 0.0 { ys.push(a[oy] + (b[oy] - a[oy]) * (xm - a[ax]) / (b[ax] - a[ax])); }
            }
            ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let ivs: Vec<(f64, f64)> = ys.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
            // close intervals that do not continue
            let mut next = vec![];
            for (y0, y1, xs0) in open.drain(..) {
                if ivs.iter().any(|iv| (iv.0 - y0).abs() < 1e-6 && (iv.1 - y1).abs() < 1e-6) { next.push((y0, y1, xs0)); } else { out.push([xs0, y0, w[0], y1]); }
            }
            for iv in &ivs { if !next.iter().any(|o: &(f64, f64, f64)| (o.0 - iv.0).abs() < 1e-6 && (o.1 - iv.1).abs() < 1e-6) { next.push((iv.0, iv.1, w[0])); } }
            open = next;
        }
        let xl = *xs.last().unwrap();
        for (y0, y1, xs0) in open { out.push([xs0, y0, xl, y1]); }
        if ax == 1 { out.iter().map(|r| [r[1], r[0], r[3], r[2]]).collect() } else { out }
    };
    // merge rectangles that share a whole edge (the slabs only merge along one axis), until none do
    let merge = |mut rs: Vec<[f64; 4]>| -> Vec<[f64; 4]> {
        let eq = |a: f64, b: f64| (a - b).abs() < 1e-6;
        loop {
            let mut done = true;
            'outer: for i in 0..rs.len() {
                for j in 0..rs.len() {
                    if i == j { continue }
                    let (a, b) = (rs[i], rs[j]);
                    let m = if eq(a[0], b[0]) && eq(a[2], b[2]) && eq(a[3], b[1]) { Some([a[0], a[1], a[2], b[3]]) }
                        else if eq(a[1], b[1]) && eq(a[3], b[3]) && eq(a[2], b[0]) { Some([a[0], a[1], b[2], a[3]]) } else { None };
                    if let Some(m) = m { rs[i] = m; rs.remove(j); done = false; break 'outer; }
                }
            }
            if done { return rs }
        }
    };
    let (a, b) = (merge(slab(0)), merge(slab(1)));
    let mut rs = if b.len() < a.len() { b } else { a };
    // a nearly rectangular footprint (notches, bays) gets one roof over its bounding rectangle
    let area = |r: &[f64; 4]| (r[2] - r[0]) * (r[3] - r[1]);
    let bb = rs.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
    if rs.iter().map(area).sum::<f64>() > 0.85 * area(&bb) { rs = vec![bb]; }
    rs
}

/// asphalt shingle texture (512 px = 1 m x 1 m, 7 courses): tabs with tone variation and dark keyways, a granule speckle
fn shingle_texture(col: [f32; 3], seed: u64) -> Arc<Vec<u8>> {
    let n = 512usize;
    let mut img = vec![0u8; n * n * 4];
    let mut r = Rng::seeded(seed ^ 0x5419_61e5);
    let srgb = |x: f32| -> u8 { (if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }.clamp(0.0, 1.0) * 255.0) as u8 };
    let rows = 7usize;
    let rh = n / rows;
    let tones: Vec<f32> = (0..rows * 8).map(|_| r.uniform(0.75, 1.25) as f32).collect();
    for y in 0..n {
        let row = (y / rh).min(rows - 1);
        let yin = y % rh;
        let off = if row % 2 == 0 { 0 } else { n / 6 };
        for x in 0..n {
            let tab = ((x + off) % n) / (n / 3);
            let xin = ((x + off) % n) % (n / 3);
            let mut v = tones[row * 8 + tab % 8] * r.uniform(0.85, 1.15) as f32;
            if yin < 6 { v *= 0.35; } // course shadow line
            if xin < 3 { v *= 0.45; } // keyway
            let i = (y * n + x) * 4;
            img[i] = srgb(col[0] * v); img[i + 1] = srgb(col[1] * v); img[i + 2] = srgb(col[2] * v); img[i + 3] = 255;
        }
    }
    Arc::new(encode_png(n as u32, n as u32, &img))
}

/// a roof plane quad / triangle with shingle uvs: u along `ax` (m), v up the slope (m)
fn roof_poly(m: &mut Mesh, pts: &[V3], ax: [f64; 2], up: V3) {
    if m.uv.len() != m.pos.len() { m.uv.resize(m.pos.len(), [0.0, 0.0]); }
    let base = m.pos.len() as u32;
    for p in pts {
        m.pos.push(*p);
        m.uv.push([p[0] * ax[0] + p[1] * ax[1], p[0] * up[0] + p[1] * up[1] + p[2] * up[2]]);
    }
    m.faces.push((0..pts.len() as u32).map(|k| base + k).collect());
}

/// one rectangle's roof over eaves at `ze`
fn rect_roof(p: &mut Parts, rc: [f64; 4], ze: f64, st: &Style, gable_walls: &mut Vec<([f64; 2], [f64; 2], [f64; 2], f64, f64, f64)>) {
    let (x0, y0, x1, y1) = (rc[0], rc[1], rc[2], rc[3]);
    let along_x = x1 - x0 >= y1 - y0;
    let t = st.pitch_deg.to_radians().tan();
    let o = st.overhang;
    // local frame: s along the ridge, w across it
    let (s0, s1, w0, w1) = if along_x { (x0, x1, y0, y1) } else { (y0, y1, x0, x1) };
    let to = |s: f64, w: f64, z: f64| -> V3 { if along_x { [s, w, z] } else { [w, s, z] } };
    let half = 0.5 * (w1 - w0);
    let wm = 0.5 * (w0 + w1);
    let ridge = ze + half * t;
    let eave = ze - o * t;
    let (we0, we1) = (w0 - o, w1 + o);
    let ax = if along_x { [1.0, 0.0] } else { [0.0, 1.0] };
    let up_a = { let c = 1.0 / (1.0 + t * t).sqrt(); if along_x { [0.0, c, t * c] } else { [c, 0.0, t * c] } };
    let up_b = { let c = 1.0 / (1.0 + t * t).sqrt(); if along_x { [0.0, -c, t * c] } else { [-c, 0.0, t * c] } };
    if st.hip {
        let (se0, se1) = (s0 - o, s1 + o);
        let inset = (half + o).min(0.5 * (se1 - se0));
        let (r0, r1) = (se0 + inset, se1 - inset);
        roof_poly(&mut p.roof, &[to(se0, we0, eave), to(se1, we0, eave), to(r1, wm, ridge), to(r0, wm, ridge)], ax, up_a);
        roof_poly(&mut p.roof, &[to(se1, we1, eave), to(se0, we1, eave), to(r0, wm, ridge), to(r1, wm, ridge)], ax, up_b);
        let ax2 = if along_x { [0.0, 1.0] } else { [1.0, 0.0] };
        roof_poly(&mut p.roof, &[to(se0, we1, eave), to(se0, we0, eave), to(r0, wm, ridge)], ax2, if along_x { [1.0, 0.0, t] } else { [0.0, 1.0, t] });
        roof_poly(&mut p.roof, &[to(se1, we0, eave), to(se1, we1, eave), to(r1, wm, ridge)], ax2, if along_x { [-1.0, 0.0, t] } else { [0.0, -1.0, t] });
        for (a, b) in [((se0, we0), (se1, we0)), ((se1, we1), (se0, we1)), ((se0, we1), (se0, we0)), ((se1, we0), (se1, we1))] {
            fascia(&mut p.trim, to(a.0, a.1, eave), to(b.0, b.1, eave));
        }
    } else {
        let rake = 0.3;
        let (se0, se1) = (s0 - rake, s1 + rake);
        roof_poly(&mut p.roof, &[to(se0, we0, eave), to(se1, we0, eave), to(se1, wm, ridge), to(se0, wm, ridge)], ax, up_a);
        roof_poly(&mut p.roof, &[to(se1, we1, eave), to(se0, we1, eave), to(se0, wm, ridge), to(se1, wm, ridge)], ax, up_b);
        for (a, b) in [((se0, we0), (se1, we0)), ((se1, we1), (se0, we1))] { fascia(&mut p.trim, to(a.0, a.1, eave), to(b.0, b.1, eave)); }
        // rake boards and the gable walls (siding up to the ridge) at both ends
        for (s, sign) in [(se0, -1.0), (se1, 1.0)] {
            fascia(&mut p.trim, to(s, we0, eave), to(s, wm, ridge));
            fascia(&mut p.trim, to(s, wm, ridge), to(s, we1, eave));
            let sw = if sign < 0.0 { s0 } else { s1 };
            // gable triangle in a wall frame: origin at (sw, w0), along +w, outward along the ridge axis
            let (o2, u2, n2) = if along_x { ([sw, w0], [0.0, 1.0], [sign, 0.0]) } else { ([w0, sw], [1.0, 0.0], [0.0, sign]) };
            gable_walls.push((o2, u2, n2, w1 - w0, ze, ridge));
        }
    }
}

/// a fascia board under a roof edge from a to b
fn fascia(m: &mut Mesh, a: V3, b: V3) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-6 { return }
    let n = [d[1] / l * 0.03, -d[0] / l * 0.03];
    let lo = |p: V3| [p[0], p[1], p[2] - 0.18];
    m.add_quad(lo(a), lo(b), b, a);
    m.add_quad(add(lo(b), [n[0], n[1], 0.0]), add(lo(a), [n[0], n[1], 0.0]), add(a, [n[0], n[1], 0.0]), add(b, [n[0], n[1], 0.0]));
    m.add_quad(lo(a), add(lo(a), [n[0], n[1], 0.0]), add(lo(b), [n[0], n[1], 0.0]), lo(b));
}

/// Add one house's exterior to the scene. `outer`: the footprint's outer face (a closed ring, metres), `cutters`: the house's
/// openings (only those on the outer boundary are used), `wall_top` the walls' top, `ground_z` the ground at the walls.
/// `fill`: put dark glass and door panels in the openings (a neighbour's shell has no windows or doors of its own)
pub fn add_house(scene: &mut Scene, tag: &str, outer: &[[f64; 2]], cutters: &[Cutter], wall_t: f64, floor_z: f64, wall_top: f64, ground_z: f64, st: &Style, seed: u64, fill: bool) -> usize {
    let mut ring: Vec<[f64; 2]> = outer.to_vec();
    if ring.len() > 1 && (ring[0][0] - ring[ring.len() - 1][0]).abs() < 1e-9 && (ring[0][1] - ring[ring.len() - 1][1]).abs() < 1e-9 { ring.pop(); }
    if ring.len() < 3 { return 0 }
    let area2: f64 = (0..ring.len()).map(|k| { let (a, b) = (ring[k], ring[(k + 1) % ring.len()]); a[0] * b[1] - b[0] * a[1] }).sum();
    let ccw = area2 > 0.0;
    let mut p = Parts { siding: Mesh::new(), trim: Mesh::new(), roof: Mesh::new(), concrete: Mesh::new() };
    let (mut glass, mut doors) = (Mesh::new(), Mesh::new());
    let ze = wall_top + 0.05;
    let zs = floor_z + 0.1; // siding starts above the foundation band
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = d[0].hypot(d[1]);
        if l < 0.05 { continue }
        let u = [d[0] / l, d[1] / l];
        let n = if ccw { [u[1], -u[0]] } else { [-u[1], u[0]] };
        // openings on this segment: cutters whose centre lies within a wall thickness of the outer face
        let holes: Vec<Hole> = cutters.iter().filter(|c| c.kind != CutKind::Passage).filter_map(|c| {
            let rel = [c.loc[0] - a[0], c.loc[1] - a[1]];
            let (al, off) = (rel[0] * u[0] + rel[1] * u[1], rel[0] * n[0] + rel[1] * n[1]);
            if off > 0.05 || off < -wall_t * 1.2 || al < 0.0 || al > l { return None }
            let hw = c.dims[0] / 2.0;
            let door = matches!(c.kind, CutKind::Door | CutKind::Entrance);
            Some(Hole { a0: al - hw, a1: al + hw, z0: (c.loc[2] - c.dims[2] / 2.0).max(floor_z), z1: c.loc[2] + c.dims[2] / 2.0, door })
        }).collect();
        siding(&mut p.siding, a, u, n, zs, ze, st.exposure, &|_| (0.0, l), &holes);
        // foundation band
        wbox(&mut p.concrete, a, u, n, (0.0, l), (0.0, 0.05), (ground_z - 0.15, zs));
        // corner board at the start vertex when the corner is convex
        let pa = ring[(k + ring.len() - 1) % ring.len()];
        let dp = [a[0] - pa[0], a[1] - pa[1]];
        let turn = dp[0] * d[1] - dp[1] * d[0];
        if (turn > 0.0) == ccw { wbox(&mut p.trim, a, u, n, (-0.05, 0.11), (0.0, SIDING_OUT + 0.015), (zs - 0.02, ze)); }
        for h in &holes {
            if fill {
                // the opening's back: dark glass (a window, with a muntin cross) or a door leaf, a little behind the wall face
                if h.door { wbox(&mut doors, a, u, n, (h.a0, h.a1), (-0.06, -0.03), (h.z0, h.z1)); }
                else {
                    glass.add_quad(wp(a, u, n, h.a0, -0.05, h.z0), wp(a, u, n, h.a1, -0.05, h.z0), wp(a, u, n, h.a1, -0.05, h.z1), wp(a, u, n, h.a0, -0.05, h.z1));
                    let (am, zm) = (0.5 * (h.a0 + h.a1), 0.5 * (h.z0 + h.z1));
                    wbox(&mut p.trim, a, u, n, (am - 0.02, am + 0.02), (-0.05, -0.02), (h.z0, h.z1));
                    wbox(&mut p.trim, a, u, n, (h.a0, h.a1), (-0.05, -0.02), (zm - 0.02, zm + 0.02));
                }
                // reveals: the opening's sides back to the glass
                wbox(&mut p.trim, a, u, n, (h.a0 - 0.01, h.a0), (-0.06, SIDING_OUT), (h.z0, h.z1));
                wbox(&mut p.trim, a, u, n, (h.a1, h.a1 + 0.01), (-0.06, SIDING_OUT), (h.z0, h.z1));
                wbox(&mut p.trim, a, u, n, (h.a0, h.a1), (-0.06, SIDING_OUT), (h.z1, h.z1 + 0.01));
                if !h.door { wbox(&mut p.trim, a, u, n, (h.a0, h.a1), (-0.06, SIDING_OUT), (h.z0 - 0.01, h.z0)); }
            }
            let dd = (SIDING_OUT, SIDING_OUT + 0.02);
            wbox(&mut p.trim, a, u, n, (h.a0 - TRIM_W, h.a0), dd, (h.z0 - if h.door { 0.0 } else { TRIM_W }, h.z1 + TRIM_W));
            wbox(&mut p.trim, a, u, n, (h.a1, h.a1 + TRIM_W), dd, (h.z0 - if h.door { 0.0 } else { TRIM_W }, h.z1 + TRIM_W));
            wbox(&mut p.trim, a, u, n, (h.a0 - TRIM_W - 0.02, h.a1 + TRIM_W + 0.02), (SIDING_OUT, SIDING_OUT + 0.035), (h.z1 + TRIM_W, h.z1 + TRIM_W + 0.04));
            if h.door {
                // steps down to the ground in front of an exterior door
                let rise = (floor_z - ground_z).max(0.0);
                let nst = (rise / 0.18).round().max(1.0) as usize;
                for s in 0..nst {
                    let ztop = floor_z - rise * s as f64 / nst as f64;
                    wbox(&mut p.concrete, a, u, n, (h.a0 - 0.25, h.a1 + 0.25), (0.0, 0.3 * (s + 1) as f64 + 0.05), (ground_z - 0.1, ztop));
                }
            } else {
                wbox(&mut p.trim, a, u, n, (h.a0 - TRIM_W - 0.03, h.a1 + TRIM_W + 0.03), (0.0, SIDING_OUT + 0.05), (h.z0 - TRIM_W - 0.035, h.z0 - TRIM_W));
            }
        }
    }
    // roof, built in the house's own frame (its longest wall along x): the rectangle decomposition needs axis-aligned walls, and
    // a neighbour turned toward a curving road or a turning circle is not (it came out as a heap of sliver roofs)
    let (ang, piv) = {
        let mut best = (0.0, 0.0);
        for k in 0..ring.len() { let (a, b) = (ring[k], ring[(k + 1) % ring.len()]); let l = (b[0] - a[0]).hypot(b[1] - a[1]); if l > best.0 { best = (l, (b[1] - a[1]).atan2(b[0] - a[0])); } }
        (best.1, ring[0])
    };
    let (ca, sa) = (ang.cos(), ang.sin());
    let local: Vec<[f64; 2]> = ring.iter().map(|q| { let (dx, dy) = (q[0] - piv[0], q[1] - piv[1]); [ca * dx + sa * dy, -sa * dx + ca * dy] }).collect();
    let mut rp = Parts { siding: Mesh::new(), trim: Mesh::new(), roof: Mesh::new(), concrete: Mesh::new() };
    let mut gables = vec![];
    for rc in rectangles(&local) {
        // a narrow strip (< 2 m across) between wings gets a lower hip roof (a steep gable over it read as a sawtooth of ridges;
        // dropping it left the ceiling showing)
        if (rc[2] - rc[0]).min(rc[3] - rc[1]) < 2.0 {
            // (a hip at 70 % of the main pitch, at most 25 deg: at 8 deg the bump-out read as a flat roof from the street)
            let lo = Style { pitch_deg: (st.pitch_deg * 0.7).min(25.0), hip: true, overhang: 0.25, ..st.clone() };
            rect_roof(&mut rp, rc, ze, &lo, &mut gables);
        } else {
            rect_roof(&mut rp, rc, ze, st, &mut gables);
        }
    }
    for (o, u, n, w, z0, z1) in gables {
        let rise = z1 - z0;
        siding(&mut rp.siding, o, u, n, z0, z1, st.exposure, &|z| { let f = ((z - z0) / rise).clamp(0.0, 1.0); (0.5 * w * f, w - 0.5 * w * f) }, &[]);
    }
    let back = Mat4::translation([piv[0], piv[1], 0.0]).mul(&Mat4::rot_z(ang));
    for (dst, mut m) in [(&mut p.roof, rp.roof), (&mut p.trim, rp.trim), (&mut p.siding, rp.siding)] {
        m.transform(&back);
        if dst.is_empty() { *dst = m; } else { dst.append(&m); }
    }
    let mut r = Rng::seeded(seed);
    let sm = scene.material(Material::diffuse(&format!("ext_siding:{tag}"), st.siding, 0.6));
    let tm = scene.material(Material::diffuse(&format!("ext_trim:{tag}"), st.trim, 0.5));
    let cm = scene.material(Material::diffuse("ext_concrete", [0.42, 0.41, 0.39], 0.9));
    let mut rm = Material::diffuse(&format!("ext_roof:{tag}"), [1.0, 1.0, 1.0], 0.85);
    rm.texture = Some(shingle_texture(st.roof, r.next_u64()));
    rm.alpha_cutoff = 0.0;
    let rmi = scene.material(rm);
    let gm = scene.material(Material::diffuse("ext_window_glass", [0.015, 0.02, 0.025], 0.06));
    let dcols: [[f32; 3]; 5] = [[0.25, 0.04, 0.03], [0.05, 0.08, 0.14], [0.06, 0.06, 0.06], [0.30, 0.18, 0.08], [0.75, 0.74, 0.70]];
    let dm = scene.material(Material::diffuse(&format!("ext_door:{tag}"), dcols[r.randint(0, 5) as usize], 0.4));
    let mut n = 0;
    for (part, m, mi) in [("siding", p.siding, sm), ("trim", p.trim, tm), ("foundation", p.concrete, cm), ("roof", p.roof, rmi), ("windows", glass, gm), ("door", doors, dm)] {
        if m.is_empty() { continue }
        n += m.faces.len();
        scene.add(&format!("exterior_{tag}.{part}"), "OutdoorHouse", m, mi);
    }
    let _ = mul;
    n
}
