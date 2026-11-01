//! ModernTable (not in infinigen): dining / coffee / side tables in the shapes real ones come in, beyond infinigen's rectangular
//! tops on thin legs: a top that is rectangular, rounded-rectangular, oval or round (a superellipse outline, with an edge bevel),
//! on four tapered or square legs, splayed mid-century legs, a trestle (two end frames and a stretcher), a central pedestal, a
//! waterfall slab (two solid side panels) or, for coffee / side tables, a solid drum / block; coffee and side tables can carry a lower
//! shelf. Frame as tables.rs: centred on x / y, z = 0 on the floor, y the long side.
use super::*;
use crate::materials::*;
use super::modern_sofa::box_uv;
use ig_kit::geomkit::limb;
use serde_json::json;

const TOP: Weighted = &[("ceramic.Marble", 1.0), ("wood.TiledWood", 0.6), ("wood.Wood", 3.0), ("ceramic.GlassVolume", 0.5), ("plastic.PlasticRough", 0.2)];
const LEG: Weighted = &[("wood.Wood", 3.0), ("metal.BrushedBlackMetal", 1.2), ("metal.MetalBasic", 0.8), ("metal.BrushedMetal", 0.5)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Base { Legs, Splayed, Trestle, Pedestal, Slab, Drum }

pub struct ModernTable {
    /// half extents (x short, y long), height
    hx: f64,
    hy: f64,
    h: f64,
    /// superellipse exponent of the top outline: 2 = ellipse / circle, 4-6 rounded rectangle, 40 = sharp rectangle
    sq: f64,
    top_t: f64,
    base: Base,
    leg_r: f64,
    square_legs: bool,
    shelf: bool,
    /// the slab panels' distance in from the ends (a dining table's: chairs at the ends tucked into flush panels)
    slab_inset: f64,
    seed: u64,
}

/// superellipse outline (n points, CCW)
fn outline(hx: f64, hy: f64, e: f64, n: usize) -> Vec<[f64; 2]> {
    (0..n).map(|i| {
        let t = std::f64::consts::TAU * i as f64 / n as f64;
        let (c, s) = (t.cos(), t.sin());
        [hx * c.signum() * c.abs().powf(2.0 / e), hy * s.signum() * s.abs().powf(2.0 / e)]
    }).collect()
}

/// a closed prism over `ring` from z0 to z1 (a small bevel: the top ring is inset by `bev`)
fn prism(ring: &[[f64; 2]], z0: f64, z1: f64, bev: f64) -> Mesh {
    let mut m = Mesh::new();
    let n = ring.len();
    let c = ring.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n as f64, a[1] + p[1] / n as f64]);
    let inset = |p: [f64; 2], d: f64| { let (dx, dy) = (p[0] - c[0], p[1] - c[1]); let l = dx.hypot(dy).max(1e-9); [p[0] - dx / l * d, p[1] - dy / l * d] };
    let zb = z1 - bev.min(0.4 * (z1 - z0));
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        m.add_quad([a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], zb], [a[0], a[1], zb]);
        let (ai, bi) = (inset(a, bev), inset(b, bev));
        m.add_quad([a[0], a[1], zb], [b[0], b[1], zb], [bi[0], bi[1], z1], [ai[0], ai[1], z1]);
    }
    let base = m.pos.len() as u32;
    for p in ring.iter().rev() { m.pos.push([p[0], p[1], z0]); }
    m.faces.push((0..n as u32).map(|k| base + k).collect());
    let base = m.pos.len() as u32;
    for p in ring { let q = inset(*p, bev); m.pos.push([q[0], q[1], z1]); }
    m.faces.push((0..n as u32).map(|k| base + k).collect());
    m
}

fn bx(lo: V3, hi: V3) -> Mesh { let mut m = Mesh::cuboid(lo, hi); box_uv(&mut m); m }

impl ModernTable {
    pub fn new(cls: &str, r: &mut Rng) -> ModernTable {
        let (mut hx, mut hy, mut h) = match cls {
            "TableDiningFactory" => { let w = r.uniform(0.85, 1.05); let l = if r.f64() < 0.7 { r.uniform(1.4, 2.4) } else { w }; (w / 2.0, l / 2.0, r.uniform(0.72, 0.77)) }
            "CoffeeTableFactory" => { let w = r.uniform(0.55, 0.8); let l = if r.f64() < 0.75 { r.uniform(1.0, 1.4) } else { w }; (w / 2.0, l / 2.0, r.uniform(0.38, 0.47)) }
            _ => { let w = r.uniform(0.4, 0.6); (w / 2.0, w / 2.0 * r.uniform(1.0, 1.25), r.uniform(0.5, 0.65)) }
        };
        // (the layout box's width / depth as given: a nightstand's box can be deeper than wide)
        if let Some(sz) = crate::api::size_hint() { hy = sz[0] / 2.0; hx = sz[1] / 2.0; h = sz[2]; } else if hx > hy { std::mem::swap(&mut hx, &mut hy); }
        let squareish = hy.max(hx) / hy.min(hx) < 1.15;
        // outline: round / oval tops only where the box allows (round in a square box, oval in a long one), else (rounded) rectangles
        let sq = match r.choice_p(&[0.45, 0.3, 0.25]) { 0 => 40.0, 1 => r.uniform(4.0, 7.0), _ => if squareish { 2.0 } else { 2.4 } };
        // (the layout made it round -- RoundTable: chairs set round it -- a circle / oval)
        let sq = if ig_kit::api::round_hint() { 2.0 } else { sq };
        let dining = cls == "TableDiningFactory";
        let lowish = cls != "TableDiningFactory";
        let w: &[f64] = if dining { &[0.4, 0.15, 0.2, 0.15, 0.1, 0.0] } else if lowish && cls == "CoffeeTableFactory" { &[0.35, 0.15, 0.0, 0.1, 0.2, 0.2] } else { &[0.35, 0.15, 0.0, 0.2, 0.1, 0.2] };
        let mut base = [Base::Legs, Base::Splayed, Base::Trestle, Base::Pedestal, Base::Slab, Base::Drum][r.choice_p(w)];
        // a pedestal carries a round / square-ish top or a short oval; a trestle a long one
        // (a square dining table seats its chairs at the corners, diagonally: corner legs went through their seats -- a pedestal there)
        if dining && squareish && matches!(base, Base::Legs | Base::Splayed | Base::Slab) { base = Base::Pedestal; }
        if base == Base::Pedestal && hy.max(hx) / hy.min(hx) > 1.6 { base = Base::Trestle; }
        // (the trestle frames stand across y: a long top along y)
        if base == Base::Trestle && hy / hx < 1.3 { base = Base::Legs; }
        let leg_r = r.uniform(0.018, 0.032) * if dining { 1.2 } else { 1.0 };
        let square_legs = r.f64() < 0.35;
        let shelf = !dining && matches!(base, Base::Legs | Base::Splayed) && r.f64() < 0.4;
        let top_t = if base == Base::Slab { r.uniform(0.035, 0.05) } else { r.uniform(0.022, 0.045) };
        let slab_inset = if dining { (hy * 0.3).min(0.28) } else { 0.0 };
        ModernTable { hx, hy, h, sq, top_t, base, leg_r, square_legs, shelf, slab_inset, seed: r.randint(0, 1_000_000) as u64 }
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let topm = spec_material("table_top", json!({"cls": weighted(&mut r, TOP), "kwargs": {}, "seed": self.seed}), [0.55, 0.40, 0.26]);
        // a slab / drum base is the top's own material (a waterfall table, a solid block)
        let legm = if matches!(self.base, Base::Slab | Base::Drum) { topm.clone() } else {
            spec_material("table_leg", json!({"cls": weighted(&mut r, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.3, 0.25, 0.2])
        };
        let (hx, hy, h, tt) = (self.hx, self.hy, self.h, self.top_t);
        let ztop = h - tt;
        let mut a = Asset::default();
        let ring = outline(hx, hy, self.sq, 96);
        let mut top = prism(&ring, ztop, h, tt * 0.3);
        box_uv(&mut top);
        let tb = top.bounds();
        a.mesh(top, &topm, Some(tb));
        let mut legs: Vec<Mesh> = vec![];
        // leg spots: inside the outline (a superellipse shrinks the corners: legs move in with it)
        let k = if self.sq >= 30.0 { 0.88 } else if self.sq >= 4.0 { 0.8 } else { 0.68 };
        let r0 = self.leg_r.min(0.12 * hx.min(hy));
        // (the leg's outer face inside the top's edge: a narrow nightstand's thick legs stood past it)
        let (cx, cy) = ((hx * k).min(hx - r0 * 1.15), (hy * k).min(hy - r0 * 1.15));
        let corners = [[-cx, -cy], [cx, -cy], [cx, cy], [-cx, cy]];
        match self.base {
            Base::Legs | Base::Splayed => {
                // (the splayed feet stay inside the top's outline: the table keeps to its layout box)
                let splay = if self.base == Base::Splayed { (0.06 * (h / 0.75)).min(hx.min(hy) * (1.0 - k) - r0 * 0.6).max(0.0) } else { 0.0 };
                for c in corners {
                    let (sx, sy) = (c[0].signum() * splay, c[1].signum() * splay);
                    let m = if self.square_legs && splay == 0.0 { bx([c[0] - r0, c[1] - r0, 0.0], [c[0] + r0, c[1] + r0, ztop]) }
                        else { limb([c[0], c[1], ztop], [c[0] + sx, c[1] + sy, 0.0], r0, r0 * 0.6, true) };
                    legs.push(m);
                }
                // (no apron: dining chairs tuck under the top, an apron at chair-arm height cut into them)
                if self.shelf {
                    let zs = h * r.uniform(0.18, 0.3);
                    let mut s = prism(&outline(cx + r0, cy + r0, self.sq.max(4.0), 64), zs, zs + 0.018, 0.004);
                    box_uv(&mut s);
                    let sb = s.bounds();
                    a.mesh(s, &topm, Some(sb));
                }
            }
            Base::Trestle => {
                // two end frames (a post on a foot, under a cleat) and a stretcher between them
                for sy in [-1.0f64, 1.0] {
                    let y = sy * hy * 0.72;
                    legs.push(bx([-hx * 0.7, y - 0.04, ztop - 0.05], [hx * 0.7, y + 0.04, ztop]));
                    legs.push(bx([-0.04, y - 0.035, 0.06], [0.04, y + 0.035, ztop - 0.05]));
                    legs.push(bx([-hx * 0.75, y - 0.045, 0.0], [hx * 0.75, y + 0.045, 0.06]));
                }
                legs.push(bx([-0.025, -hy * 0.72, 0.28 * h], [0.025, hy * 0.72, 0.28 * h + 0.07]));
            }
            Base::Pedestal => {
                let rc = (hx.min(hy) * 0.13).clamp(0.05, 0.1);
                legs.push(limb([0.0, 0.0, ztop], [0.0, 0.0, 0.05], rc, rc * 1.15, true));
                let fr = hx.min(hy) * 0.62;
                let mut foot = prism(&outline(fr * (hx / hy).clamp(0.7, 1.0), fr * (hy / hx).clamp(0.7, 1.4), 2.0, 48), 0.0, 0.05, 0.02);
                box_uv(&mut foot);
                legs.push(foot);
            }
            Base::Slab => {
                // two solid side panels flush with the top's ends (a waterfall table); a dining table's set in from the ends, where its
                // end chairs tuck under the top
                let (t, i) = (tt.max(0.035), self.slab_inset);
                let px = if i > 0.0 { hx - 0.06 } else { hx };
                legs.push(bx([-px, -hy + i, 0.0], [px, -hy + i + t, ztop]));
                legs.push(bx([-px, hy - i - t, 0.0], [px, hy - i, ztop]));
            }
            Base::Drum => {
                // a solid block / drum under the top, a little inset
                let mut d = prism(&outline(hx * 0.9, hy * 0.9, self.sq, 64), 0.0, ztop, 0.0);
                box_uv(&mut d);
                legs.push(d);
            }
        }
        for m in legs { let b = m.bounds(); a.mesh(m, &legm, Some(b)); }
        a.supports.push(([tb.0[0], tb.0[1], h - 0.001], tb.1));
        a
    }
}

impl Factory for ModernTable {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> { Some([2.0 * self.hy, 2.0 * self.hx, self.h]) }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> { Some(([-self.hx, -self.hy, 0.0], [self.hx, self.hy, self.h])) }
    fn asset(&self, _i: u64) -> Asset { self.build() }
}
