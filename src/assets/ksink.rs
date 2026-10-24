//! table_decorations/sink.py: SinkFactory (kitchen sink). Placeholder bounds are exact (analytic bounds of nodegroup_sink_geometry);
//! the asset is an approximation (counter plate + basin).
use super::*;

pub fn handles(c: &str) -> bool {
    c == "SinkFactory"
}

pub struct KSink {
    width: f64,
    depth: f64,
    upper: f64,
    lower: f64,
    hole_radius: f64,
    margin: f64,
    tap_margin: f64,
    protrude: f64,
    curvature: f64,
    fseed: u64,
}

impl KSink {
    pub fn new(rng: &mut Rng) -> KSink {
        let mut r = rng.clone();
        let width = r.uniform(0.4, 1.0);
        let depth = r.uniform(0.4, 0.5);
        let curvature = r.uniform(1.0, 1.0);
        let upper = r.uniform(0.2, 0.4);
        let lower = r.uniform(0.0, 0.01);
        let hole_radius = r.uniform(0.02, 0.05);
        let margin = r.uniform(0.02, 0.05);
        let tap_margin = r.uniform(0.1, 0.12);
        let protrude = r.uniform(0.01, 0.025);
        KSink { width, depth, upper, lower, hole_radius, margin, tap_margin, protrude, curvature, fseed: 0 }
    }
    /// bounds of the "Geometry" output of nodegroup_sink_geometry (float32 like Blender)
    fn geo_bounds(&self) -> (V3, V3) {
        let f = |x: f64| x as f32;
        let (w, d, m, t) = (f(self.width), f(self.depth), f(self.margin), f(self.tap_margin));
        let add_2 = w + m;
        let add_4 = (d + m) + t;
        let shift = (t + m) / 2.56f32;
        let tr = t * -0.5f32;
        let x0 = -(add_4 / 2.0) + tr + shift;
        let x1 = add_4 / 2.0 + tr + shift;
        let (y0, y1) = (-(add_2 / 2.0), add_2 / 2.0);
        let z1 = f(self.upper);
        let _ = (self.lower, self.hole_radius);
        ([x0 as f64, y0 as f64, 0.0], [x1 as f64, y1 as f64, z1 as f64])
    }
}

impl Factory for KSink {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (mut lo, hi) = self.geo_bounds();
        lo[2] = (hi[2] as f32 - self.protrude as f32) as f64;
        let mut a = Asset::default();
        a.cuboid(lo, hi, &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, i: u64) -> Asset {
        use super::gnasset::GnTemplate;
        use serde_json::json;
        static TPL: std::sync::OnceLock<GnTemplate> = std::sync::OnceLock::new();
        let tpl = TPL.get_or_init(|| GnTemplate::parse(include_str!("../../data/gn/ksink.json")));
        let mut t = tpl.clone_template();
        let vals = [self.width, self.depth, self.curvature, self.upper, self.lower, self.hole_radius, self.margin, self.tap_margin];
        for (k, v) in vals.iter().enumerate() {
            t.set_group_default("nodegroup_sink_geometry", &format!("Socket_{k}"), json!(*v));
        }
        let outs = t.run_group("nodegroup_sink_geometry").expect("sink gn");
        let m = mat("sink", [0.75, 0.75, 0.77], 0.3);
        let mut a = Asset::default();
        for (_, g) in &outs {
            a.mesh(super::gnhelpers::to_mesh(g), &m, None);
        }
        // tap child at (-Depth / 2, 0, Upper Height)
        let mut tm = super::taps::tap_mesh(self.fseed, i);
        let loc = [(-self.depth / 2.0) as f32, 0.0f32, self.upper as f32];
        for p in tm.pos.iter_mut() {
            *p = [(p[0] as f32 + loc[0]) as f64, (p[1] as f32 + loc[1]) as f64, (p[2] as f32 + loc[2]) as f64];
        }
        a.mesh(tm, &m, None);
        a
    }
}

pub fn make(c: &str, rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    match c {
        "SinkFactory" => {
            let mut k = KSink::new(rng);
            k.fseed = fseed;
            Some(Box::new(k))
        }
        _ => None,
    }
}
