//! tables/dining_table.py, cocktail_table.py: Dining/Coffee/Side/Cocktail tables built from the shared table_utils node-group geometry
use super::geomkit::*;
use super::tableparts::*;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "TableDiningFactory" | "CoffeeTableFactory" | "SideTableFactory" | "TableCocktailFactory")
}

// coffee-table finishes as in real living rooms: mostly wood, some marble and glass, rarely plastic; legs wood / metal (infinigen's
// table drew plastic tops in random hues and ceramic or plastic legs equally often)
const TOP: Weighted = &[("ceramic.Marble", 1.0), ("wood.TiledWood", 1.0), ("wood.Wood", 2.5), ("plastic.PlasticRough", 0.15), ("ceramic.GlassVolume", 0.8)];
const LEG: Weighted = &[("ceramic.Ceramic", 0.2), ("ceramic.Glass", 0.3), ("plastic.Plastic", 0.2), ("metal.MetalBasic", 1.5), ("metal.BrushedBlackMetal", 1.0), ("wood.Wood", 2.0)];

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Straight,
    Stand,
    Square,
}


pub struct Table {
    x: f64,
    y: f64,
    z: f64,
    style: Style,
    legs: usize,
    leg_d: f64,
    top_scale: f64,
    bottom_scale: f64,
    curve: Vec<(f64, f64)>,
    top_t: f64,
    top_n: usize,
    top_w: f64,
    top_aspect: f64,
    fillet: f64,
    vfillet: f64,
    leg_ngon: usize,
    stretch_pos: f64,
    incr: usize,
    seed: u64,
    cocktail: bool,
}

impl Table {
    /// bounding box of the table as built: the top (its rim sweeps a little beyond the nominal outline) and the legs (the tips of
    /// straight legs end a little under z = 0, a wide stance reaches past the top)
    fn overall(&self) -> (V3, V3) {
        let top_h = self.z - self.top_t;
        let mut top = table_top_exact(self.top_n, self.top_w, self.top_aspect, self.fillet, self.top_t, self.vfillet);
        top.transform(&Mat4::translation([0.0, 0.0, top_h]));
        let (mut lo, mut hi) = top.bounds();
        let (_, meshes) = self.leg_meshes(top_h);
        for m in &meshes {
            let (a, b) = m.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        (lo, hi)
    }
    fn set_outline(&mut self, nx: f64, ny: f64) {
        if self.style == Style::Stand {
            self.leg_d *= nx / self.x;
        }
        let round = self.top_n == 32;
        self.x = nx;
        self.y = ny;
        self.top_w = if round { ny } else { 1.414 * nx };
        self.top_aspect = if round { 1.0 } else { ny / nx };
    }
    /// built to the layout box [width (y), depth (x), height]: the table's overall extents are the box's, the legs follow the top
    fn fit(&mut self, h: [f64; 3]) {
        if self.cocktail {
            return;
        }
        let round = self.top_n == 32;
        let (nx, ny) = if round { let d = h[0].min(h[1]); (d, d) } else { (h[1], h[0]) };
        self.z = h[2];
        self.set_outline(nx, ny);
        for _ in 0..4 {
            let (lo, hi) = self.overall();
            let (ex, ey, ez) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
            if (ex - nx).abs() < 5e-4 && (ey - ny).abs() < 5e-4 && (ez - h[2]).abs() < 5e-4 { break }
            let (cx, cy) = (self.x, self.y);
            self.set_outline(cx - (ex - nx), cy - (ey - ny));
            self.z -= ez - h[2];
        }
    }
    fn new(cls: &str, rng: &mut Rng) -> Table {
        let mut t = Table::new_raw(cls, rng);
        if let Some(h) = crate::api::size_hint() {
            t.fit(h);
        }
        t
    }
    fn new_raw(cls: &str, rng: &mut Rng) -> Table {
        // exact python draw order (dining_table.py / cocktail_table.py sample_parameters), fresh FixedSeed stream
        let mut r = rng.clone();
        let r = &mut r;
        if cls == "TableCocktailFactory" {
            let x = r.uniform(0.5, 0.8);
            let z = r.uniform(1.0, 1.5);
            let ngon = [4usize, 32][r.choice_idx(2)];
            let round = ngon >= 32;
            let stand = r.choice_idx(2) == 1;
            let (style, legs, leg_d, curve, bottom_scale_default);
            if stand {
                style = Style::Stand;
                legs = 1;
                leg_d = r.uniform(0.7 * x, 0.9 * x);
                curve = vec![(0.0, r.uniform(0.1, 0.2)), (0.5, r.uniform(0.1, 0.2)), (0.9, r.uniform(0.2, 0.3)), (1.0, 1.0)];
                bottom_scale_default = 1.0;
            } else {
                style = Style::Straight;
                leg_d = r.uniform(0.05, 0.07);
                legs = if round { 3 + r.choice_idx(2) } else { ngon };
                curve = vec![(0.0, 1.0), (0.4, r.uniform(0.85, 0.95)), (1.0, r.uniform(0.4, 0.6))];
                bottom_scale_default = 1.0;
            }
            let _ = bottom_scale_default;
            let top_t = r.uniform(0.02, 0.05);
            let fillet = if round { 0.499 } else { r.uniform(0.0, 0.05) };
            let vfillet = r.uniform(0.1, 0.3);
            let leg_ngon = [4usize, 32][r.choice_idx(2)];
            let bottom_scale = r.uniform(1.1, 1.3);
            let stretch_pos = r.uniform(0.2, 0.6);
            let incr = r.choice_idx(3);
            return Table { x, y: x, z, style, legs, leg_d, top_scale: 0.7, bottom_scale, curve, top_t, top_n: if round { 32 } else { 4 }, top_w: if round { x } else { 1.414 * x }, top_aspect: 1.0, fillet, vfillet, leg_ngon, stretch_pos, incr, seed: r.randint(0, 1_000_000) as u64, cocktail: true };
        }
        // Coffee/Side tables compute their dimensions BEFORE the seeded block (from global state, unseeded in python): emulate with
        // an independent stream, and do not consume them from the parameter stream. Dining draws them inside the block.
        let dims = match cls {
            "SideTableFactory" => {
                let u = super::unseeded().unwrap_or_else(|| { let mut g = rng.clone(); vec![g.normal(1.0, 0.05), g.normal(1.0, 0.05)] });
                let w = 0.55 * u[0];
                [w, w, 0.95 * w * u[1]]
            }
            "CoffeeTableFactory" => {
                let u = super::unseeded().unwrap_or_else(|| { let mut g = rng.clone(); vec![g.uniform(1.0, 1.5), g.uniform(0.6, 0.9), g.uniform(0.4, 0.5)] });
                [u[0], u[1], u[2]]
            }
            _ => {
                let w = r.uniform(0.91, 1.16);
                let l = if r.f64() < 0.7 { r.uniform(1.4, 2.8) } else { w * r.normal(1.0, 0.1) };
                [l, w, r.uniform(0.65, 0.85)]
            }
        };
        let [x, y, z] = dims;
        let style = [Style::Straight, Style::Stand, Style::Square][r.choice_p(&[0.5, 0.1, 0.4])];
        let (legs, leg_d, top_scale, bottom_scale, curve);
        match style {
            Style::Stand => {
                legs = 2;
                leg_d = r.uniform(0.22 * x, 0.28 * x);
                curve = vec![(0.0, r.uniform(0.1, 0.2)), (0.5, r.uniform(0.1, 0.2)), (0.9, r.uniform(0.2, 0.3)), (1.0, 1.0)];
                top_scale = r.uniform(0.6, 0.7);
                bottom_scale = 1.0;
            }
            Style::Square => {
                legs = 2;
                leg_d = r.uniform(0.07, 0.10);
                curve = vec![];
                top_scale = 0.8;
                bottom_scale = 1.0;
            }
            Style::Straight => {
                leg_d = r.uniform(0.05, 0.07);
                legs = 4;
                curve = vec![(0.0, 1.0), (0.4, r.uniform(0.85, 0.95)), (1.0, r.uniform(0.4, 0.6))];
                top_scale = 0.8;
                bottom_scale = r.uniform(1.0, 1.2);
            }
        }
        let top_t = r.uniform(0.03, 0.06);
        let fillet = r.uniform(0.0, 0.02);
        let vfillet = r.uniform(0.1, 0.3);
        let stretch_pos = r.uniform(0.2, 0.6);
        let incr = r.choice_idx(3);
        let seed = r.randint(0, 1_000_000) as u64;
        // a dining table the layout made round (ig_kit round hint): a circular top as wide as the table is deep, the same draws
        if cls == "TableDiningFactory" && ig_kit::api::round_hint() {
            return Table { x: y, y, z, style, legs, leg_d, top_scale, bottom_scale, curve, top_t, top_n: 32, top_w: y, top_aspect: 1.0, fillet: 0.499, vfillet, leg_ngon: 4, stretch_pos, incr, seed, cocktail: false };
        }
        Table { x, y, z, style, legs, leg_d, top_scale, bottom_scale, curve, top_t, top_n: 4, top_w: 1.414 * x, top_aspect: y / x, fillet, vfillet, leg_ngon: 4, stretch_pos, incr, seed, cocktail: false }
    }

    /// leg anchors and the leg / stretcher meshes for a top at height `top_h`
    fn leg_meshes(&self, top_h: f64) -> (Vec<V3>, Vec<Mesh>) {
        let anch = anchor_points(self.legs, self.top_scale * self.top_w, self.top_aspect);
        let meshes = match self.style {
            Style::Straight => {
                let leg = leg_straight(&self.curve, 1.0, self.leg_d, 32, self.leg_ngon, 0.1);
                let st = strecher(self.leg_d * 0.5);
                legs_and_stretchers(&anch, top_h, self.bottom_scale, &leg, Some((&st, self.incr, self.stretch_pos)))
            }
            Style::Stand => {
                let leg = leg_single_stand(&self.curve, 1.0, self.leg_d, 64);
                legs_and_stretchers(&anch, top_h, self.bottom_scale, &leg, None)
            }
            Style::Square => {
                let w = 0.707 * self.top_scale * self.top_w * self.top_aspect;
                let leg = leg_square(w, 1.0, self.incr > 0, self.leg_d);
                legs_and_stretchers(&anch, top_h, self.bottom_scale, &leg, None)
            }
        };
        (anch, meshes)
    }

    fn build(&self) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let topm = spec_material("table_top", json!({"cls": weighted(&mut r, TOP), "kwargs": {}, "seed": self.seed}), [0.6, 0.45, 0.3]);
        let legm = spec_material("table_leg", json!({"cls": weighted(&mut r, LEG), "kwargs": {}, "seed": self.seed + 1}), [0.3, 0.3, 0.3]);
        let mut a = Asset::default();
        let top_h = self.z - self.top_t;
        let mut top = table_top_exact(self.top_n, self.top_w, self.top_aspect, self.fillet, self.top_t, self.vfillet);
        top.transform(&Mat4::translation([0.0, 0.0, top_h]));
        let (anch, meshes) = self.leg_meshes(top_h);
        // Join Geometry output order: the last-linked input first -> [stretchers, legs] then the top
        let nl = anch.len();
        let (legs, st) = meshes.split_at(nl);
        for m in st.iter().chain(legs.iter()) {
            let b = m.bounds();
            a.mesh(m.clone(), &legm, Some(b));
        }
        let tb = top.bounds();
        a.mesh(top, &topm, Some(tb));
        a.supports.push(([tb.0[0], tb.0[1], tb.1[2] - 0.001], tb.1));
        a
    }
}

impl Factory for Table {
    fn natural_size(&self, _inst_seed: u64) -> Option<[f64; 3]> {
        if self.cocktail {
            return None;
        }
        let (lo, hi) = self.overall();
        let (ex, ey) = (hi[0] - lo[0], hi[1] - lo[1]);
        Some([ex.max(ey), ex.min(ey), hi[2] - lo[2]])
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        if !self.cocktail {
            return None;
        }
        // TableCocktailFactory.create_placeholder: the 512-sample top profile resampled to 10 points, filled and extruded over the full height.
        let prof = ngon_profile_curve(self.top_n, self.top_w, self.top_aspect, self.fillet).resample(512).resample(10);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &prof.pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let mut a = Asset::default();
        let m = spec_material("table_top", json!({"cls": "ceramic.Marble", "kwargs": {}, "seed": 0}), [0.6, 0.45, 0.3]);
        a.cuboid([lo[0], lo[1], 0.0], [hi[0], hi[1], self.z], &m);
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        self.build()
    }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    // 55 % of dining / coffee / side tables are ModernTable (round / oval / rounded tops, trestle / pedestal / slab / drum bases,
    // lower shelves), the rest infinigen's; the choice is its own stream so infinigen's draws are unchanged. NO_MODERN_TABLE=1: off
    // (a round table in a box that is not square: infinigen's round table builds a circle to the short side and missed the box by up
    // to 10 cm; ModernTable builds the oval that fills it)
    let oval = c == "TableDiningFactory" && ig_kit::api::round_hint() && crate::api::size_hint().map_or(false, |h| (h[0] - h[1]).abs() > 0.01);
    // 35 % of side tables are bedside cabinets (drawers / shelf / door), their own stream (NO_NIGHTSTAND=1: off)
    if c == "SideTableFactory" && std::env::var_os("NO_NIGHTSTAND").is_none() {
        let mut pick = Rng::seeded(crate::int_hash(&["nightstand", &rng.clone().next_u64().to_string()]));
        if pick.f64() < 0.35 { return Some(Box::new(super::nightstand::Nightstand::new(&mut rng.clone()))); }
    }
    if c != "TableCocktailFactory" && (oval || std::env::var_os("NO_MODERN_TABLE").is_none()) {
        let mut pick = Rng::seeded(crate::int_hash(&["modern_table", &rng.clone().next_u64().to_string()]));
        if oval || pick.f64() < 0.55 { return Some(Box::new(super::modern_table::ModernTable::new(c, &mut rng.clone()))); }
    }
    Some(Box::new(Table::new(c, rng)))
}
