//! Graph -> WGSL for the previewer. Every number becomes an entry of the material's uniform array, so all materials of one structure share
//! one pipeline; `code` is the same for them.
use serde_json::Value;

pub const LIB: &str = concat!(include_str!("perlin_noise_3d.wgsl"), "\n", include_str!("lib.wgsl"));
const MAX_PARAMS: usize = 48 * 4;

pub struct Emitted { pub code: String, pub params: Vec<f32> }

#[derive(Clone, Copy, PartialEq)]
enum T { F, C }

struct E { params: Vec<f32>, types: Vec<T>, code: String }

impl E {
    fn p(&mut self, x: f64) -> String {
        let i = self.params.len();
        self.params.push(x as f32);
        format!("m.p[{}].{}", i / 4, ['x', 'y', 'z', 'w'][i % 4])
    }
    fn raw(&mut self, v: &Value) -> Option<(String, T)> {
        Some(match v {
            Value::Number(n) => (self.p(n.as_f64()?), T::F),
            Value::Array(a) if a.len() >= 3 => {
                let (x, y, z) = (self.p(a[0].as_f64()?), self.p(a[1].as_f64()?), self.p(a[2].as_f64()?));
                (format!("vec3<f32>({x}, {y}, {z})"), T::C)
            }
            Value::Object(o) => { let i = o.get("n")?.as_u64()? as usize; (format!("n{i}"), *self.types.get(i)?) }
            _ => return None,
        })
    }
    fn f(&mut self, v: &Value) -> Option<String> {
        let (s, t) = self.raw(v)?;
        Some(if t == T::F { s } else { format!("dot({s}, vec3<f32>(0.2126, 0.7152, 0.0722))") })
    }
    fn c(&mut self, v: &Value) -> Option<String> {
        let (s, t) = self.raw(v)?;
        Some(if t == T::C { s } else { format!("vec3<f32>({s})") })
    }
    fn noise_args(&mut self, i: &Value) -> Option<String> {
        let v = self.c(&i["v"])?;
        let mut s = v;
        for k in ["scale", "detail", "rough", "lac", "dist", "w"] { s += ", "; s += &self.f(&i[k])?; }
        Some(s)
    }
    fn node(&mut self, k: usize, n: &Value) -> Option<()> {
        let i = &n["in"];
        let (e, t) = match n["op"].as_str()? {
            "coord" => ("obj".to_string(), T::C),
            "mapping" => {
                let v = self.c(&i["v"])?;
                let s = self.c(&i["scale"])?;
                let rot = i["rot"].as_array()?;
                if rot.iter().all(|x| x.as_f64() == Some(0.0)) { (format!("{v} * {s}"), T::C) }
                else { let r = self.c(&i["rot"])?; (format!("ig_rotate({v} * {s}, {r})"), T::C) }
            }
            "noise" => (format!("ig_noise({})", self.noise_args(i)?), T::F),
            "noise_color" => (format!("ig_noise_color({})", self.noise_args(i)?), T::C),
            "musgrave" => (format!("ig_musgrave({})", self.noise_args(i)?), T::F),
            "voronoi_color" => { let (v, s) = (self.c(&i["v"])?, self.f(&i["scale"])?); (format!("ig_voronoi_color({v} * {s})"), T::C) }
            "brick_color" | "brick_fac" => {
                let (v, sc, row, w, off, mo) = (self.c(&i["v"])?, self.f(&i["scale"])?, self.f(&i["row"])?, self.f(&i["width"])?, self.f(&i["offset"])?, self.f(&i["mortar"])?);
                if n["op"] == "brick_color" { (format!("vec3<f32>(ig_brick({v}, {sc}, {row}, {w}, {off}, {mo}).x)"), T::C) } else { (format!("ig_brick({v}, {sc}, {row}, {w}, {off}, {mo}).y"), T::F) }
            }
            "checker" => { let (v, sc) = (self.c(&i["v"])?, self.f(&i["scale"])?); (format!("ig_checker({v} * {sc})"), T::F) }
            "wall_uv" => { let v = self.c(&i["v"])?; (format!("vec3<f32>(cross({v}, n).z, {v}.z, 0.0)"), T::C) }
            "add" => { let (a, b) = (self.c(&i["a"])?, self.c(&i["b"])?); (format!("{a} + {b}"), T::C) }
            "ramp" => {
                let f = self.f(&i["fac"])?;
                let stops = i["stops"].as_array()?;
                let (mut e, mut prev) = (None, None);
                for s in stops {
                    let (pos, col) = (self.f(&s[0])?, self.c(&s[1])?);
                    e = Some(match (e, prev) {
                        (Some(acc), Some(p)) => format!("mix({acc}, {col}, smooth_lin({f}, {p}, {pos}))"),
                        _ => col,
                    });
                    prev = Some(pos);
                }
                (e?, T::C)
            }
            "map_range" => {
                let (v, f0, f1, t0, t1) = (self.f(&i["v"])?, self.f(&i["f0"])?, self.f(&i["f1"])?, self.f(&i["t0"])?, self.f(&i["t1"])?);
                (format!("mix({t0}, {t1}, clamp(({v} - {f0}) / ({f1} - {f0}), 0.0, 1.0))"), T::F)
            }
            "mul" => { let (a, b) = (self.f(&i["a"])?, self.f(&i["b"])?); (format!("{a} * {b}"), T::F) }
            "mix_mix" => { let (f, a, b) = (self.f(&i["fac"])?, self.c(&i["a"])?, self.c(&i["b"])?); (format!("mix({a}, {b}, clamp({f}, 0.0, 1.0))"), T::C) }
            "mix_multiply" => { let (f, a, b) = (self.f(&i["fac"])?, self.c(&i["a"])?, self.c(&i["b"])?); (format!("mix({a}, {a} * {b}, clamp({f}, 0.0, 1.0))"), T::C) }
            "mix_difference" => { let (f, a, b) = (self.f(&i["fac"])?, self.c(&i["a"])?, self.c(&i["b"])?); (format!("mix({a}, abs({a} - {b}), clamp({f}, 0.0, 1.0))"), T::C) }
            "hsv_adjust" => { let (c, s, v) = (self.c(&i["color"])?, self.f(&i["sat"])?, self.f(&i["val"])?); (format!("ig_hsv_adjust({c}, {s}, {v})"), T::C) }
            _ => return None,
        };
        self.code += &format!("  let n{k}: {} = {e};\n", if t == T::F { "f32" } else { "vec3<f32>" });
        self.types.push(t);
        Some(())
    }
}

/// statements setting `surf_color`, `surf_rough`, `surf_metal`, `surf_coat`, `surf_height` from `obj`; None for an op this backend lacks
pub fn emit(graph: &Value) -> Option<Emitted> {
    // (glass is the previewer's own pass)
    if graph["out"]["glass"].as_bool() == Some(true) { return None }
    let mut e = E { params: vec![], types: vec![], code: String::new() };
    for (k, n) in graph["nodes"].as_array()?.iter().enumerate() { e.node(k, n)?; }
    let o = &graph["out"];
    let color = e.c(&o["color"])?;
    let mut code = format!("  surf_color = {color};\n");
    for (key, var) in [("roughness", "surf_rough"), ("metallic", "surf_metal"), ("coat", "surf_coat"), ("height", "surf_height")] {
        if !o[key].is_null() { let v = e.f(&o[key])?; code += &format!("  {var} = {v};\n"); }
    }
    if e.params.len() > MAX_PARAMS { return None }
    Some(Emitted { code: e.code + &code, params: e.params })
}
