//! Materials as data: each class draws its parameters and builds a shader graph (JSON). Two backends read the same graph: `wgsl` for the
//! previewer, tools/render/shade_graph.py for Blender. Ported from infinigen/assets/materials (close look-alikes, no bit parity).
pub mod classes;
pub mod wgsl;
use serde_json::{json, Value};

pub use classes::for_material;

/// the material's own random stream, from its seed
pub struct R(fastrand::Rng);
impl R {
    pub fn new(seed: u64) -> R { R(fastrand::Rng::with_seed(seed)) }
    pub fn u64(&mut self) -> u64 { self.0.u64(..) }
    pub fn f(&mut self) -> f64 { self.0.f64() }
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.f() }
    pub fn log_uniform(&mut self, a: f64, b: f64) -> f64 { self.uniform(a.ln(), b.ln()).exp() }
}

type LinHsv = palette::Hsv<palette::encoding::Linear<palette::encoding::Srgb>, f64>;

/// hsv (hue 0..1) to a linear colour, as infinigen's hsv2rgba takes them
pub fn hsv(h: f64, s: f64, v: f64) -> [f64; 3] {
    use palette::FromColor;
    let c = palette::LinSrgb::from_color(LinHsv::new(h.rem_euclid(1.0) * 360.0, s, v));
    [c.red, c.green, c.blue]
}

/// 0xRRGGBB as sRGB -> linear -> hsv (hue 0..1), as infinigen's hex2rgba + rgb2hsv
pub fn hex_to_hsv(c: u32) -> [f64; 3] {
    use palette::FromColor;
    let lin = palette::Srgb::new(((c >> 16) & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, (c & 255) as f64 / 255.0).into_linear();
    let h = LinHsv::from_color(lin);
    [h.hue.into_positive_degrees() / 360.0, h.saturation, h.value]
}

/// a graph under construction. Inputs are numbers, [r, g, b], or a node's id as {"n": id}; every op has one output
#[derive(Default)]
pub struct G { pub nodes: Vec<Value> }

pub fn link(id: usize) -> Value { json!({"n": id}) }

impl G {
    fn add(&mut self, op: &str, inp: Value) -> Value {
        self.nodes.push(json!({"op": op, "in": inp}));
        link(self.nodes.len() - 1)
    }
    /// object-space position
    pub fn coord(&mut self) -> Value { self.add("coord", json!({})) }
    /// (v * scale) turned by the XYZ euler `rot`
    pub fn mapping(&mut self, v: &Value, scale: [f64; 3], rot: [f64; 3]) -> Value { self.add("mapping", json!({"v": v, "scale": scale, "rot": rot})) }
    /// Blender's noise texture: fBM normalised to about 0..1
    pub fn noise(&mut self, v: &Value, scale: impl Into<Value>, detail: f64, dist: f64, w: f64) -> Value {
        self.add("noise", json!({"v": v, "scale": scale.into(), "detail": detail, "rough": 0.5, "lac": 2.0, "dist": dist, "w": w}))
    }
    pub fn noise_color(&mut self, v: &Value, scale: f64, detail: f64, dist: f64) -> Value {
        self.add("noise_color", json!({"v": v, "scale": scale, "detail": detail, "rough": 0.5, "lac": 2.0, "dist": dist, "w": 0.0}))
    }
    /// Blender's old musgrave texture: unnormalised fBM, amplitude falling by lacunarity^-dimension
    pub fn musgrave(&mut self, v: &Value, scale: impl Into<Value>, detail: f64, dimension: f64, lac: f64, w: f64) -> Value {
        self.add("musgrave", json!({"v": v, "scale": scale.into(), "detail": detail, "rough": lac.powf(-dimension), "lac": lac, "dist": 0.0, "w": w}))
    }
    pub fn voronoi_color(&mut self, v: &Value, scale: f64) -> Value { self.add("voronoi_color", json!({"v": v, "scale": scale})) }
    /// colour ramp, linear between (position, colour) stops in rising order
    pub fn ramp(&mut self, fac: &Value, stops: &[(f64, [f64; 3])]) -> Value {
        self.add("ramp", json!({"fac": fac, "stops": stops.iter().map(|s| json!([s.0, s.1])).collect::<Vec<_>>()}))
    }
    pub fn map_range(&mut self, v: &Value, f0: f64, f1: f64, t0: f64, t1: f64) -> Value { self.add("map_range", json!({"v": v, "f0": f0, "f1": f1, "t0": t0, "t1": t1})) }
    pub fn mul(&mut self, a: &Value, b: impl Into<Value>) -> Value { self.add("mul", json!({"a": a, "b": b.into()})) }
    /// blend: "mix" | "multiply" | "difference"
    pub fn mix(&mut self, fac: impl Into<Value>, a: impl Into<Value>, b: impl Into<Value>, blend: &str) -> Value {
        self.add(&format!("mix_{blend}"), json!({"fac": fac.into(), "a": a.into(), "b": b.into()}))
    }
    /// Blender's Hue / Saturation / Value node at hue 0.5
    pub fn hsv_adjust(&mut self, color: impl Into<Value>, sat: f64, val: f64) -> Value { self.add("hsv_adjust", json!({"color": color.into(), "sat": sat, "val": val})) }
    /// Blender's brick texture: each brick's random grey ("brick_color") and the mortar mask ("brick_fac"); `offset` shifts every other row
    pub fn brick(&mut self, v: &Value, scale: f64, row: f64, width: f64, offset: f64, mortar: f64) -> (Value, Value) {
        let i = json!({"v": v, "scale": scale, "row": row, "width": width, "offset": offset, "mortar": mortar});
        (self.add("brick_color", i.clone()), self.add("brick_fac", i))
    }
    pub fn checker(&mut self, v: &Value, scale: f64) -> Value { self.add("checker", json!({"v": v, "scale": scale})) }
    /// (distance along a wall, height, 0): infinigen's vertical tile coordinates
    pub fn wall_uv(&mut self, v: &Value) -> Value { self.add("wall_uv", json!({"v": v})) }
    pub fn add_v(&mut self, a: &Value, b: impl Into<Value>) -> Value { self.add("add", json!({"a": a, "b": b.into()})) }
    /// a finished graph; `out` keys: color, roughness, metallic, coat, coat_roughness, specular, height (metres of relief)
    pub fn done(self, name: &str, out: Value) -> Value { json!({"name": name, "nodes": self.nodes, "out": out}) }
}
