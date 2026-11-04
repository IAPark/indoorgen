//! Tables: dining, coffee, side and cocktail (bar-height) tables as one piece. A top whose outline is a superellipse -- rectangular,
//! rounded-rectangular, oval or round -- with an edge bevel, on one of nine bases: plain legs (round or square), splayed legs, tapered
//! legs joined by stretchers (infinigen's table legs), a sled frame at each end, a trestle, a pedestal, a waterfall slab, a solid drum,
//! or a cocktail table's stand; coffee and side tables can carry a lower shelf. Built straight to its size ([width (y), depth (x),
//! height]); every shape decision is made when the table is made, so its collision (`volumes`, from these parameters, no mesh built)
//! and its build agree. Frame: centred on x / y, y the long side, z = 0 on the floor.
use super::tableparts::*;
use super::*;
use crate::materials::*;
use super::sofa::box_uv;
use crate::piece::Volume;
use ig_kit::geomkit::limb;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "TableDiningFactory" | "CoffeeTableFactory" | "SideTableFactory" | "TableCocktailFactory")
}

const TOP: Weighted = &[("ceramic.Marble", 1.0), ("wood.TiledWood", 0.6), ("wood.Wood", 3.0), ("ceramic.GlassVolume", 0.5), ("plastic.PlasticRough", 0.2)];
const LEG: Weighted = &[("wood.Wood", 3.0), ("metal.BrushedBlackMetal", 1.2), ("metal.MetalBasic", 0.8), ("metal.BrushedMetal", 0.5)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind { Dining, Coffee, Side, Cocktail }

#[derive(Clone, Copy, PartialEq, Debug)]
enum Base { Legs, Splayed, Tapered, Sled, Trestle, Pedestal, Slab, Drum, Stand }

pub struct Table {
    kind: Kind,
    /// half extents (x short, y long), height
    hx: f64,
    hy: f64,
    h: f64,
    /// superellipse exponent of the top outline: 2 = ellipse / circle, 4-7 rounded rectangle, 40 = sharp rectangle
    sq: f64,
    top_t: f64,
    base: Base,
    /// leg radius at the top (legs, splayed, tapered), a sled frame's profile half-width
    leg_r: f64,
    square_legs: bool,
    /// tapered legs: their radius profile (fraction down the leg, fraction of the top radius), how far the feet spread (x the anchor),
    /// stretcher height (fraction of the way down) and pattern, the leg's section (4: square, 32: round)
    taper: Vec<(f64, f64)>,
    spread: f64,
    stretch_at: f64,
    stretch_incr: usize,
    leg_ngon: usize,
    /// a lower shelf's height (coffee / side tables on legs)
    shelf_z: Option<f64>,
    /// the slab panels' distance in from the ends (a dining table's: chairs at the ends tuck into flush panels)
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

/// the axis-aligned bounds of a capped cylinder of radius `r` from `a` to `b` (it reaches r * sqrt(1 - d_k^2) past its ends on axis k)
fn cyl_box(a: V3, b: V3, r: f64) -> (V3, V3) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
    let e = [0, 1, 2].map(|k| r * (1.0 - (d[k] / l).powi(2)).max(0.0).sqrt());
    ([0, 1, 2].map(|k| a[k].min(b[k]) - e[k]), [0, 1, 2].map(|k| a[k].max(b[k]) + e[k]))
}

impl Table {
    pub fn new(cls: &str, r: &mut Rng) -> Table {
        let kind = match cls { "TableDiningFactory" => Kind::Dining, "CoffeeTableFactory" => Kind::Coffee, "TableCocktailFactory" => Kind::Cocktail, _ => Kind::Side };
        let (mut hx, mut hy, mut h) = match kind {
            Kind::Dining => { let w = r.uniform(0.85, 1.05); let l = if r.f64() < 0.7 { r.uniform(1.4, 2.4) } else { w }; (w / 2.0, l / 2.0, r.uniform(0.72, 0.77)) }
            Kind::Coffee => { let w = r.uniform(0.55, 0.8); let l = if r.f64() < 0.75 { r.uniform(1.0, 1.4) } else { w }; (w / 2.0, l / 2.0, r.uniform(0.38, 0.47)) }
            Kind::Side => { let w = r.uniform(0.4, 0.6); (w / 2.0, w / 2.0 * r.uniform(1.0, 1.25), r.uniform(0.5, 0.65)) }
            Kind::Cocktail => { let w = r.uniform(0.55, 0.8); (w / 2.0, w / 2.0, r.uniform(1.0, 1.12)) }
        };
        // (the size asked: the layout box as given -- a nightstand's box can be deeper than wide)
        if let Some(sz) = crate::api::size_hint() { hy = sz[0] / 2.0; hx = sz[1] / 2.0; h = sz[2]; } else if hx > hy { std::mem::swap(&mut hx, &mut hy); }
        // a dining table up to 1.6 m long is round some of the time (the rule round_table_p): squared to its depth, 0.9-1.2 m (the layout
        // reads it from the round top's collision and seats one chair per side round it); the old path's round hint, likewise
        let round = kind == Kind::Dining && (ig_kit::api::round_hint() || (2.0 * hy.max(hx) <= 1.6 + 1e-9 && r.f64() < crate::piece::rules().round_table_p));
        if round { let d = (2.0 * hx.min(hy)).clamp(0.9, 1.2) / 2.0; hx = d; hy = d; }
        let squareish = hy.max(hx) / hy.min(hx) < 1.15;
        // outline: round / oval tops only where the box allows (round in a square box, oval in a long one), else (rounded) rectangles
        let mut sq = match r.choice_p(&[0.45, 0.3, 0.25]) { 0 => 40.0, 1 => r.uniform(4.0, 7.0), _ => if squareish { 2.0 } else { 2.4 } };
        if round { sq = 2.0; }
        let w: &[f64] = match kind {
            //               Legs  Splay Taper Sled  Trest Pedes Slab  Drum  Stand
            Kind::Dining => &[0.25, 0.1, 0.15, 0.1, 0.15, 0.15, 0.1, 0.0, 0.0],
            Kind::Coffee => &[0.25, 0.1, 0.1, 0.1, 0.0, 0.1, 0.15, 0.2, 0.0],
            Kind::Side => &[0.25, 0.1, 0.1, 0.05, 0.0, 0.2, 0.1, 0.2, 0.0],
            Kind::Cocktail => &[0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5],
        };
        let mut base = [Base::Legs, Base::Splayed, Base::Tapered, Base::Sled, Base::Trestle, Base::Pedestal, Base::Slab, Base::Drum, Base::Stand][r.choice_p(w)];
        // (a square dining table seats its chairs at the corners, diagonally: corner legs went through their seats -- a pedestal there)
        if kind == Kind::Dining && squareish && matches!(base, Base::Legs | Base::Splayed | Base::Tapered | Base::Slab | Base::Sled) { base = Base::Pedestal; }
        // a pedestal carries a round / square-ish top or a short oval; a trestle a long one (its frames stand across y)
        if base == Base::Pedestal && hy.max(hx) / hy.min(hx) > 1.6 { base = Base::Trestle; }
        if base == Base::Trestle && hy / hx < 1.3 { base = Base::Legs; }
        let leg_r = r.uniform(0.018, 0.032) * if kind == Kind::Dining { 1.2 } else { 1.0 };
        let square_legs = r.f64() < 0.35;
        let top_t = if base == Base::Slab { r.uniform(0.035, 0.05) } else { r.uniform(0.022, 0.045) };
        let taper = vec![(0.0, 1.0), (0.4, r.uniform(0.85, 0.95)), (1.0, r.uniform(0.4, 0.6))];
        let spread = r.uniform(1.0, 1.2);
        let stretch_at = r.uniform(0.2, 0.6);
        let stretch_incr = r.choice_idx(3);
        let leg_ngon = [4usize, 32][r.choice_idx(2)];
        let shelf_z = if matches!(kind, Kind::Coffee | Kind::Side) && matches!(base, Base::Legs | Base::Splayed) && r.f64() < 0.4 { Some(h * r.uniform(0.18, 0.3)) } else { None };
        let slab_inset = if kind == Kind::Dining { (hy * 0.3).min(0.28) } else { 0.0 };
        Table { kind, hx, hy, h, sq, top_t, base, leg_r, square_legs, taper, spread, stretch_at, stretch_incr, leg_ngon, shelf_z, slab_inset, seed: r.randint(0, 1_000_000) as u64 }
    }

    fn bevel(&self) -> f64 { self.top_t * 0.3 }
    fn ztop(&self) -> f64 { self.h - self.top_t }
    fn k(&self) -> f64 { if self.sq >= 30.0 { 0.88 } else if self.sq >= 4.0 { 0.8 } else { 0.68 } }

    /// leg radius at the top, kept inside the top's edge
    fn r0(&self) -> f64 { self.leg_r.min(0.12 * self.hx.min(self.hy)) }

    /// where the legs stand under the top (a superellipse shrinks the corners: the legs move in with it)
    fn leg_spots(&self) -> [[f64; 2]; 4] {
        let (k, r0) = (self.k(), self.r0());
        let (cx, cy) = ((self.hx * k).min(self.hx - r0 * 1.15), (self.hy * k).min(self.hy - r0 * 1.15));
        [[-cx, -cy], [cx, -cy], [cx, cy], [-cx, cy]]
    }

    /// the splayed legs' foot offset (the feet stay inside the top's outline)
    fn splay(&self) -> f64 { (0.06 * (self.h / 0.75)).min(self.hx.min(self.hy) * (1.0 - self.k()) - self.r0() * 0.6).max(0.0) }

    /// tapered legs: their radius at the top, the feet's spread kept inside the top's outline
    fn taper_r(&self) -> f64 { self.r0() * 1.4 }
    fn spread(&self) -> f64 {
        let s = self.leg_spots()[2];
        let r = self.taper_r();
        self.spread.min((self.hx - r) / s[0].abs().max(1e-6)).min((self.hy - r) / s[1].abs().max(1e-6)).max(1.0)
    }

    /// pedestal / stand: column radius, foot half extents
    fn pedestal(&self) -> (f64, f64, f64) {
        let (hx, hy) = (self.hx, self.hy);
        let rc = (hx.min(hy) * 0.13).clamp(0.05, 0.1);
        let fr = hx.min(hy) * 0.62;
        (rc, fr * (hx / hy).clamp(0.7, 1.0), fr * (hy / hx).clamp(0.7, 1.4))
    }

    /// a sled frame's posts: their x, the frames' y, the profile half-width
    fn sled(&self) -> (f64, f64, f64) {
        let p = self.r0();
        (self.hx - 0.04 - p, self.hy - 0.06 - p, p)
    }

    /// slab sides: thickness, inset from the top's ends, half width. Under a round or oval top they stand in from its ends and within its outline (at the ends they stuck out past it)
    fn slab(&self) -> (f64, f64, f64) {
        let (t, mut i) = (self.top_t.max(0.035), self.slab_inset);
        let mut px = if i > 0.0 { self.hx - 0.06 } else { self.hx };
        if self.sq < 10.0 {
            i = i.max(0.2 * self.hy);
            let f = (self.hy - i) / self.hy;
            px = px.min(self.hx * (1.0 - f.powf(self.sq)).max(0.0).powf(1.0 / self.sq) * 0.95);
        }
        (t, i, px)
    }

    /// the flat of the top, inside its bevel (on a round / oval top: the inscribed rectangle)
    fn support(&self) -> (V3, V3) {
        let c = std::f64::consts::FRAC_1_SQRT_2.powf(2.0 / self.sq);
        let (ix, iy) = (self.hx * c - self.bevel(), self.hy * c - self.bevel());
        ([-ix, -iy, self.h - 0.001], [ix, iy, self.h])
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let top_cls = weighted(&mut r, TOP);
        let topm = spec_material("table_top", json!({"cls": top_cls, "kwargs": {}, "seed": self.seed}), [0.55, 0.40, 0.26]);
        // a slab / drum base is the top's own material (a waterfall table, a solid block)
        // (not glass: a solid block of it was a ghost of a table)
        let legm = if matches!(self.base, Base::Slab | Base::Drum) && !top_cls.contains("Glass") { topm.clone() } else {
            spec_material("table_leg", json!({"cls": weighted(&mut r, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.3, 0.25, 0.2])
        };
        let (hx, hy, h) = (self.hx, self.hy, self.h);
        let ztop = self.ztop();
        let mut a = Asset::default();
        let mut top = prism(&outline(hx, hy, self.sq, 96), ztop, h, self.bevel());
        box_uv(&mut top);
        let tb = top.bounds();
        a.mesh(top, &topm, Some(tb));
        let mut legs: Vec<Mesh> = vec![];
        let r0 = self.r0();
        let spots = self.leg_spots();
        match self.base {
            Base::Legs | Base::Splayed => {
                let splay = if self.base == Base::Splayed { self.splay() } else { 0.0 };
                for c in spots {
                    let (sx, sy) = (c[0].signum() * splay, c[1].signum() * splay);
                    legs.push(if self.square_legs && splay == 0.0 { bx([c[0] - r0, c[1] - r0, 0.0], [c[0] + r0, c[1] + r0, ztop]) }
                        else { limb([c[0], c[1], ztop], [c[0] + sx, c[1] + sy, 0.0], r0, r0 * 0.6, true) });
                }
                // (no apron: dining chairs tuck under the top, an apron at chair-arm height cut into them)
                if let Some(zs) = self.shelf_z {
                    let (cx, cy) = (spots[2][0], spots[2][1]);
                    let mut s = prism(&outline(cx + r0, cy + r0, self.sq.max(4.0), 64), zs, zs + 0.018, 0.004);
                    box_uv(&mut s);
                    let sb = s.bounds();
                    a.mesh(s, &topm, Some(sb));
                }
            }
            Base::Tapered => {
                // infinigen's table legs: a radius profile along each leg, the feet spread out, stretchers between them
                let anch: Vec<V3> = spots.iter().map(|c| [c[0], c[1], 0.0]).collect();
                let d = 2.0 * self.taper_r();
                let leg = leg_straight(&self.taper, 1.0, d, 32, self.leg_ngon, 0.1);
                let st = strecher(d * 0.5);
                legs.extend(legs_and_stretchers(&anch, ztop, self.spread(), &leg, Some((&st, self.stretch_incr, self.stretch_at))));
            }
            Base::Sled => {
                // a frame at each end: two posts on a foot rail, under a top rail
                let (fx, fy, p) = self.sled();
                for sy in [-1.0f64, 1.0] {
                    let y = sy * fy;
                    for sx in [-1.0f64, 1.0] { legs.push(bx([sx * fx - p, y - p, 0.0], [sx * fx + p, y + p, ztop])); }
                    // (between the posts: run through them, their faces lay in one plane and fought, black at the corners)
                    legs.push(bx([-fx + p, y - p, 0.0], [fx - p, y + p, 2.0 * p]));
                    legs.push(bx([-fx + p, y - p, ztop - 2.0 * p], [fx - p, y + p, ztop]));
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
            Base::Pedestal | Base::Stand => {
                let (rc, fx, fy) = self.pedestal();
                legs.push(limb([0.0, 0.0, ztop], [0.0, 0.0, 0.05], rc, rc * 1.15, true));
                let mut foot = prism(&outline(fx, fy, 2.0, 48), 0.0, 0.05, 0.02);
                box_uv(&mut foot);
                legs.push(foot);
            }
            Base::Slab => {
                // two solid side panels flush with the top's ends (a waterfall table); a dining table's set in from the ends
                let (t, i, px) = self.slab();
                legs.push(bx([-px, -hy + i, 0.0], [px, -hy + i + t, ztop]));
                legs.push(bx([-px, hy - i - t, 0.0], [px, hy - i, ztop]));
            }
            Base::Drum => {
                let mut d = prism(&outline(hx * 0.9, hy * 0.9, self.sq, 64), 0.0, ztop, 0.0);
                box_uv(&mut d);
                legs.push(d);
            }
        }
        for m in legs { let b = m.bounds(); a.mesh(m, &legm, Some(b)); }
        a.supports.push(self.support());
        a
    }

    /// collision from the parameters: the top, the base's parts, the support
    fn vols(&self) -> Vec<Volume> {
        let (hx, hy, h) = (self.hx, self.hy, self.h);
        let ztop = self.ztop();
        let r0 = self.r0();
        let spots = self.leg_spots();
        let mut v = vec![if self.sq < 3.0 {
            // a round / oval top: its outline (24 points pushed out by 1 / cos(7.5 deg) to hold the arcs between them), extruded
            let g = 1.0 / (std::f64::consts::PI / 24.0).cos();
            let ring = outline(hx * g, hy * g, self.sq, 24);
            Volume::hard_hull(ring.iter().flat_map(|q| [[q[0], q[1], ztop], [q[0], q[1], h]]).collect())
        } else { Volume::hard([-hx, -hy, ztop], [hx, hy, h]) }];
        let (s0, s1) = self.support();
        v.push(Volume::support(s0, s1));
        match self.base {
            Base::Legs | Base::Splayed => {
                let splay = if self.base == Base::Splayed { self.splay() } else { 0.0 };
                for c in spots {
                    let (lo, hi) = if self.square_legs && splay == 0.0 { ([c[0] - r0, c[1] - r0, 0.0], [c[0] + r0, c[1] + r0, ztop]) }
                        else { cyl_box([c[0], c[1], ztop], [c[0] + c[0].signum() * splay, c[1] + c[1].signum() * splay, 0.0], r0) };
                    v.push(Volume::hard(lo, hi));
                }
                if let Some(zs) = self.shelf_z {
                    let (cx, cy) = (spots[2][0] + r0, spots[2][1] + r0);
                    v.push(Volume::hard([-cx, -cy, zs], [cx, cy, zs + 0.018]));
                }
            }
            Base::Tapered => {
                let rr = self.taper_r();
                let s = self.spread();
                for c in spots {
                    let (lo, hi) = cyl_box([c[0], c[1], ztop], [c[0] * s, c[1] * s, 0.0], rr);
                    v.push(Volume::hard(lo, hi));
                }
                // stretchers: between the legs' points at their height
                let f = self.stretch_at;
                let g = 1.0 + (s - 1.0) * f;
                let (sx, sy) = (spots[2][0] * g, spots[2][1] * g);
                let zs = ztop * (1.0 - f);
                let rs = rr * 0.5;
                v.push(Volume::hard([-sx - rs, -sy - rs, zs - rs], [sx + rs, sy + rs, zs + rs]));
            }
            Base::Sled => {
                let (fx, fy, p) = self.sled();
                for sy in [-1.0f64, 1.0] { v.push(Volume::hard([-fx - p, sy * fy - p, 0.0], [fx + p, sy * fy + p, ztop])); }
            }
            Base::Trestle => {
                for sy in [-1.0f64, 1.0] {
                    let y = sy * hy * 0.72;
                    v.push(Volume::hard([-hx * 0.75, y - 0.045, 0.0], [hx * 0.75, y + 0.045, ztop]));
                }
                v.push(Volume::hard([-0.025, -hy * 0.72, 0.28 * h], [0.025, hy * 0.72, 0.28 * h + 0.07]));
            }
            Base::Pedestal | Base::Stand => {
                let (rc, fx, fy) = self.pedestal();
                let rr = rc * 1.15;
                v.push(Volume::hard([-rr, -rr, 0.05], [rr, rr, ztop]));
                v.push(Volume::hard([-fx, -fy, 0.0], [fx, fy, 0.05]));
            }
            Base::Slab => {
                let (t, i, px) = self.slab();
                v.push(Volume::hard([-px, -hy + i, 0.0], [px, -hy + i + t, ztop]));
                v.push(Volume::hard([-px, hy - i - t, 0.0], [px, hy - i, ztop]));
            }
            Base::Drum => v.push(Volume::hard([-hx * 0.9, -hy * 0.9, 0.0], [hx * 0.9, hy * 0.9, ztop])),
        }
        v
    }
}

impl Factory for Table {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> { Some([2.0 * self.hy, 2.0 * self.hx, self.h]) }
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> { Some(([-self.hx, -self.hy, 0.0], [self.hx, self.hy, self.h])) }
    fn asset(&self, _i: u64) -> Asset { self.build() }
    fn volumes(&self, _i: u64) -> Option<Vec<Volume>> { Some(self.vols()) }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    // 35 % of side tables are bedside cabinets (drawers / shelf / door), their own stream (NO_NIGHTSTAND=1: off)
    if c == "SideTableFactory" && std::env::var_os("NO_NIGHTSTAND").is_none() {
        let mut pick = Rng::seeded(crate::int_hash(&["nightstand", &rng.clone().next_u64().to_string()]));
        if pick.f64() < 0.35 { return Some(Box::new(super::nightstand::Nightstand::new(&mut rng.clone()))); }
    }
    Some(Box::new(Table::new(c, rng)))
}
