//! A bathroom vanity: the StandingSink's basin on a cabinet in place of its pedestal, so whatever knows a standing sink (the mirror over it, the dressing on its deck, cameras,
//! window sills) knows it too. In the sink's frame: the back at the lowest x, the basin's front at x = 0, its bottom at z = 0, the floor at z = -stand_height.
use super::bathroom::Sink;
use crate::mesh::Mesh;
use crate::rng::Rng;
use serde_json::json;

#[derive(Clone, Debug)]
pub struct Vanity {
    /// counter past the basin at each side
    pub margin: f64,
    pub overhang: f64,
    /// 0 doors, 1 drawers, 2 a drawer row over doors, 3 open shelf
    pub kind: u8,
    pub floating: bool,
    pub legs: bool,
    /// the basin set into the counter (false: standing on it)
    pub drop_in: bool,
    pub double: bool,
    pub gap: f64,
    pub top_t: f64,
    /// 0 knob, 1 bar, 2 none
    pub handle: u8,
    pub shaker: bool,
    pub seed: u64,
    /// body: class and painted colour
    pub body: (&'static str, Option<[f32; 3]>),
    pub top: (&'static str, [f32; 3]),
}

const INSET: f64 = 0.02;

/// the basin's outline at its rim: the convex hull of its top vertices, drawn in `by` toward its middle
pub fn rim_of(body: &Mesh, depth: f64, by: f64) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = body.pos.iter().filter(|v| v[2] > depth - 0.015).map(|v| [v[0], v[1]]).collect();
    if p.len() < 3 { return vec![] }
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    p.dedup();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = vec![];
    for &q in &p { while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 { h.pop(); } h.push(q); }
    let n = h.len() + 1;
    for &q in p.iter().rev().skip(1) { while h.len() >= n && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 { h.pop(); } h.push(q); }
    h.pop();
    let c = [h.iter().map(|q| q[0]).sum::<f64>() / h.len() as f64, h.iter().map(|q| q[1]).sum::<f64>() / h.len() as f64];
    h.into_iter().map(|q| { let d = (q[0] - c[0]).hypot(q[1] - c[1]).max(1e-6); let k = ((d - by) / d).max(0.2); [c[0] + (q[0] - c[0]) * k, c[1] + (q[1] - c[1]) * k] }).collect()
}

impl Vanity {
    pub fn draw(seed: u64) -> Vanity {
        let mut r = Rng::seeded(seed);
        let kind = [0u8, 0, 1, 2, 2, 3][r.choice_idx(6)];
        let floating = kind != 3 && r.f64() < ig_core::mesh::style_param("vanity_floating_p", 0.25);
        let legs = !floating && r.f64() < if kind == 3 { 1.0 } else { 0.3 };
        let body = [("wood.WhitePlywood", None), ("wood.Wood", None), ("wood.Wood", None), ("wood.WhitePlywood", Some([0.03, 0.03, 0.035])),
            ("wood.WhitePlywood", Some([0.05, 0.1, 0.2])), ("wood.WhitePlywood", Some([0.2, 0.28, 0.2])), ("wood.WhitePlywood", Some([0.45, 0.45, 0.42]))][r.choice_idx(7)];
        let top = [("ceramic.Marble", [0.85, 0.85, 0.83]), ("ceramic.Marble", [0.85, 0.85, 0.83]), ("ceramic.Marble", [0.06, 0.06, 0.07]), ("wood.Wood", [0.5, 0.4, 0.3]), ("ceramic.Concrete", [0.45, 0.45, 0.43])][r.choice_idx(5)];
        Vanity { margin: r.uniform(0.07, 0.26), overhang: r.uniform(0.05, 0.09), kind, floating, legs, drop_in: r.f64() < 0.6, double: r.f64() < ig_core::mesh::style_param("vanity_double_p", 0.15), gap: r.uniform(0.1, 0.25),
            top_t: r.uniform(0.02, 0.045), handle: [0u8, 1, 1, 2][r.choice_idx(4)], shaker: r.f64() < 0.5, seed: r.randint(0, 1 << 40) as u64, body, top }
    }
    /// y of each basin's side
    pub fn basins(&self, s: &Sink) -> Vec<f64> { if self.double { vec![0.0, s.width + self.gap] } else { vec![0.0] } }
    fn span(&self, s: &Sink) -> [f64; 2] { [-self.margin, *self.basins(s).last().unwrap() + s.width + self.margin] }
    fn back(&self, s: &Sink) -> f64 { -(s.size_extrude + 1.0) * s.size }
    /// the counter's top
    fn top_z(&self, s: &Sink) -> f64 { if self.drop_in { s.depth - 0.012 } else { 0.0 } }
    /// the piece's outside, what the solver works with
    pub fn bounds(&self, s: &Sink) -> ([f64; 3], [f64; 3]) {
        let y = self.span(s);
        ([self.back(s), y[0], -s.stand_height], [self.overhang + if self.handle == 2 { 0.0 } else { 0.035 }, y[1], s.depth])
    }
    /// the counter's top, with the hole `rim` (the basin's outline at its rim, convex, around its middle) in the cell [x0, x1] x [y0, y1]: quads between the rim and the
    /// cell's edge along rays from the middle, so the hole follows an oval basin as well as a square one
    fn holed(m: &mut Mesh, rim: &[[f64; 2]], cell: [f64; 4], z: [f64; 2]) {
        let [x0, x1, y0, y1] = cell;
        let c = [rim.iter().map(|p| p[0]).sum::<f64>() / rim.len() as f64, rim.iter().map(|p| p[1]).sum::<f64>() / rim.len() as f64];
        let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
        let mut a: Vec<f64> = rim.iter().map(|p| ang(*p)).chain([[x0, y0], [x1, y0], [x1, y1], [x0, y1]].iter().map(|p| ang(*p))).collect();
        a.sort_by(|p, q| p.partial_cmp(q).unwrap());
        a.dedup_by(|p, q| (*p - *q).abs() < 1e-6);
        // along a ray: where it leaves the rim, and the cell
        let leave_rim = |t: f64| -> [f64; 2] {
            let d = [t.cos(), t.sin()];
            let mut best = f64::MAX;
            for i in 0..rim.len() {
                let (p, q) = (rim[i], rim[(i + 1) % rim.len()]);
                let e = [q[0] - p[0], q[1] - p[1]];
                let den = d[0] * e[1] - d[1] * e[0];
                if den.abs() < 1e-12 { continue }
                let w = [p[0] - c[0], p[1] - c[1]];
                let (r, u) = ((w[0] * e[1] - w[1] * e[0]) / den, (w[0] * d[1] - w[1] * d[0]) / den);
                if r > 0.0 && (-1e-9..=1.0 + 1e-9).contains(&u) { best = best.min(r); }
            }
            [c[0] + d[0] * best, c[1] + d[1] * best]
        };
        let leave_cell = |t: f64| -> [f64; 2] {
            let d = [t.cos(), t.sin()];
            let mut best = f64::MAX;
            if d[0] > 1e-12 { best = best.min((x1 - c[0]) / d[0]); }
            if d[0] < -1e-12 { best = best.min((x0 - c[0]) / d[0]); }
            if d[1] > 1e-12 { best = best.min((y1 - c[1]) / d[1]); }
            if d[1] < -1e-12 { best = best.min((y0 - c[1]) / d[1]); }
            [c[0] + d[0] * best, c[1] + d[1] * best]
        };
        for i in 0..a.len() {
            let (t0, t1) = (a[i], a[(i + 1) % a.len()]);
            let (i0, i1, o0, o1) = (leave_rim(t0), leave_rim(t1), leave_cell(t0), leave_cell(t1));
            m.add_quad([i0[0], i0[1], z[1]], [o0[0], o0[1], z[1]], [o1[0], o1[1], z[1]], [i1[0], i1[1], z[1]]);
            m.add_quad([i0[0], i0[1], z[0]], [i1[0], i1[1], z[0]], [o1[0], o1[1], z[0]], [o0[0], o0[1], z[0]]);
        }
    }

    /// `rim`: the basin's outline at its rim (its own frame), convex, drawn in a little under its edge
    pub fn parts(&self, s: &Sink, rim: &[[f64; 2]]) -> (Mesh, Mesh, Mesh) {
        let (mut body, mut top, mut metal) = (Mesh::new(), Mesh::new(), Mesh::new());
        let [y0, y1] = self.span(s);
        let (x0, ztop, floor) = (self.back(s), self.top_z(s), -s.stand_height);
        let zb = ztop - self.top_t;
        let fx = self.overhang - INSET;
        // the counter, around the basins when they are set in it
        if self.drop_in && rim.len() >= 3 {
            let (fr, ba) = (self.overhang, x0);
            let mut y = y0;
            let bs = self.basins(s);
            for (i, b) in bs.iter().enumerate() {
                let end = if i + 1 < bs.len() { b + s.width + self.gap / 2.0 } else { y1 };
                let r: Vec<[f64; 2]> = rim.iter().map(|p| [p[0], p[1] + b]).collect();
                Self::holed(&mut top, &r, [ba, fr, y, end], [zb, ztop]);
                y = end;
            }
            // the edge all round
            let e = 0.0005;
            top.append(&Mesh::cuboid([ba, y0, zb], [ba + e, y1, ztop]));
            top.append(&Mesh::cuboid([fr - e, y0, zb], [fr, y1, ztop]));
            top.append(&Mesh::cuboid([ba, y0, zb], [fr, y0 + e, ztop]));
            top.append(&Mesh::cuboid([ba, y1 - e, zb], [fr, y1, ztop]));
        } else {
            top.append(&Mesh::cuboid([x0, y0, zb], [self.overhang, y1, ztop]));
        }
        let base = if self.floating { floor + 0.22 } else if self.legs { floor + 0.14 } else { floor + 0.08 };
        let (ya, yb) = (y0 + INSET, y1 - INSET);
        if self.legs || self.kind == 3 {
            let w = 0.035;
            for y in [ya, yb - w] { for x in [x0 + 0.02, fx - w] { body.append(&Mesh::cuboid([x, y, floor], [x + w, y + w, base])); } }
        } else if !self.floating {
            body.append(&Mesh::cuboid([x0, ya, floor], [fx - 0.05, yb, base]));
        }
        if self.kind == 3 {
            // two ends, a back, a shelf and the bottom under the counter
            let t = 0.022;
            for y in [ya, yb - t] { body.append(&Mesh::cuboid([x0, y, base], [fx, y + t, zb])); }
            body.append(&Mesh::cuboid([x0, ya, base], [x0 + 0.015, yb, zb]));
            body.append(&Mesh::cuboid([x0, ya, base], [fx, yb, base + t]));
            body.append(&Mesh::cuboid([x0, ya, zb.min(0.0) - t - 0.02], [fx, yb, zb.min(0.0) - 0.02]));
            return (body, top, metal);
        }
        // (solid only below the basins: above, a bowl set in the counter hangs into a hollow between four boards)
        let solid = zb.min(0.0);
        body.append(&Mesh::cuboid([x0, ya, base], [fx, yb, solid]));
        if zb > solid {
            let t = 0.012;
            body.append(&Mesh::cuboid([x0, ya, solid], [x0 + t, yb, zb]));
            body.append(&Mesh::cuboid([fx - t, ya, solid], [fx, yb, zb]));
            body.append(&Mesh::cuboid([x0, ya, solid], [fx, ya + t, zb]));
            body.append(&Mesh::cuboid([x0, yb - t, solid], [fx, yb, zb]));
        }
        let cols = (((yb - ya) / 0.42).round() as usize).max(1);
        let cw = (yb - ya) / cols as f64;
        let gap = 0.004;
        let mut front = |body: &mut Mesh, metal: &mut Mesh, a: f64, b: f64, c: f64, d: f64, knob_side: f64, drawer: bool| {
            let (x, xf) = (fx, fx + 0.018);
            if self.shaker && d - c > 0.2 && b - a > 0.2 {
                let f = 0.05;
                body.append(&Mesh::cuboid([x, a, c], [xf, a + f, d]));
                body.append(&Mesh::cuboid([x, b - f, c], [xf, b, d]));
                body.append(&Mesh::cuboid([x, a + f, c], [xf, b - f, c + f]));
                body.append(&Mesh::cuboid([x, a + f, d - f], [xf, b - f, d]));
                body.append(&Mesh::cuboid([x, a + f, c + f], [x + 0.008, b - f, d - f]));
            } else {
                body.append(&Mesh::cuboid([x, a, c], [xf, b, d]));
            }
            let (hy, hz) = if drawer { ((a + b) / 2.0, (c + d) / 2.0) } else { (if knob_side > 0.0 { b - 0.04 } else { a + 0.04 }, d - 0.09) };
            match self.handle {
                0 => metal.append(&Mesh::cuboid([xf, hy - 0.012, hz - 0.012], [xf + 0.03, hy + 0.012, hz + 0.012])),
                1 => {
                    let half = if drawer { 0.07 } else { 0.0 };
                    let (dy, dz) = if drawer { (half, 0.006) } else { (0.006, 0.07) };
                    metal.append(&Mesh::cuboid([xf + 0.022, hy - dy, hz - dz], [xf + 0.032, hy + dy, hz + dz]));
                    for (py, pz) in if drawer { [(hy - dy + 0.01, hz), (hy + dy - 0.01, hz)] } else { [(hy, hz - dz + 0.01), (hy, hz + dz - 0.01)] } {
                        metal.append(&Mesh::cuboid([xf, py - 0.004, pz - 0.004], [xf + 0.022, py + 0.004, pz + 0.004]));
                    }
                }
                _ => {}
            }
        };
        let (zl, zh) = (base + 0.004, zb - 0.004);
        for c in 0..cols {
            let (a, b) = (ya + c as f64 * cw + gap, ya + (c + 1) as f64 * cw - gap);
            let side = if c % 2 == 0 { 1.0 } else { -1.0 };
            match self.kind {
                1 => { let n = if zh - zl > 0.55 { 3 } else { 2 }; for i in 0..n { let h = (zh - zl) / n as f64; front(&mut body, &mut metal, a, b, zl + i as f64 * h + gap, zl + (i + 1) as f64 * h - gap, side, true); } }
                2 => { let m = zh - 0.17; front(&mut body, &mut metal, a, b, m + gap, zh, side, true); front(&mut body, &mut metal, a, b, zl, m - gap, side, false); }
                _ => front(&mut body, &mut metal, a, b, zl, zh, side, false),
            }
        }
        (body, top, metal)
    }
    pub fn materials(&self) -> [crate::mesh::Material; 3] {
        let mut spec = json!({"cls": self.body.0, "kwargs": {}, "seed": self.seed});
        if let Some(p) = self.body.1 { spec["paint"] = json!({"rgb": p, "amount": 0.9}); }
        [ig_core::materials::spec_material("vanity_body", spec, [0.6, 0.5, 0.4]),
         ig_core::materials::spec_material("vanity_top", json!({"cls": self.top.0, "kwargs": {}, "seed": self.seed + 1}), self.top.1),
         ig_core::materials::spec_material("vanity_handle", json!({"cls": "metal.BrushedMetal", "kwargs": {}, "seed": self.seed + 2}), [0.6, 0.6, 0.6])]
    }
}
