//! The material classes (infinigen/assets/materials/*), and the choices that used to live in the render script: which variant a part gets from
//! its material name and shape.
use crate::{hsv, G, R};
use serde_json::{json, Value};

/// the graph of a material, or None where its class is not ported yet (Blender then runs the Python class, the previewer the flat colour).
/// `mat`: material name; `node`: node name; `ext`: the part's size in its own frame; `rgb`: the material's plain colour
pub fn for_material(spec: &Value, mat: &str, node: &str, ext: [f64; 3], rgb: [f32; 3]) -> Option<Value> {
    let cls = spec.get("cls")?.as_str()?;
    let seed = spec.get("seed").and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|x| x as u64))).unwrap_or(0);
    let kw = spec.get("kwargs").cloned().unwrap_or(json!({}));
    let mut r = R::new(seed);
    let family = mat.split(':').next().unwrap_or("");
    let base = [rgb[0] as f64, rgb[1] as f64, rgb[2] as f64];
    let mut g = match cls {
        "ceramic.Plaster" => plaster(&mut r, matches!(family, "wall" | "wall_alt").then_some(base)),
        // sanitary ware is white (the shader's random hues made mauve and mint toilets)
        "ceramic.Ceramic" if matches!(family, "toilet_ceramic" | "tub_ceramic" | "sink_ceramic") => ceramic(&mut r, Some([0.9, 0.9, 0.88]), 0.3),
        "ceramic.Ceramic" => ceramic(&mut r, None, 0.8),
        "plastic.PlasticRough" | "shader.plastic.plastic_rough.shader_rough_plastic" => rough_plastic(&mut r, &kw),
        "wood.WhitePlywood" => white_plywood(&mut r),
        "wood.BlondePlywood" => shelves_wood(&mut r, false, false),
        "wood.BlackPlywood" => shelves_wood(&mut r, true, false),
        "shader.wood.plywood.shader_shelves_wood_z" => shelves_wood(&mut r, false, true),
        "shader.wood.plywood.shader_shelves_black_wood_z" => shelves_wood(&mut r, true, true),
        "shader.wood.wood.shader_wood" => wood(&mut r, kw.get("vertical").and_then(|v| v.as_bool()).unwrap_or(false)),
        "wood.Wood" => {
            // large flat tops: straight-grain veneer (the noise-warped grain read as burl or contour lines there)
            let mut e = ext;
            e.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if e[1] > 0.35 && ext[2] <= e[0] + 1e-6 && ext[2] < 0.12 * e[1] {
                shelves_wood(&mut r, crc32fast::hash(node.split('#').next().unwrap_or(node).as_bytes()) % 10 >= 7, false)
            } else {
                // the grain runs along x: parts thin in x showed end grain, so they get it vertical
                wood(&mut r, ext[0] < 0.5 * ext[1].max(ext[2]))
            }
        }
        _ => return None,
    };
    // a painted finish over the material
    if let Some(p) = spec.get("paint") {
        let (rgb, amt) = (p.get("rgb")?.clone(), p.get("amount")?.as_f64()?);
        let c = g["out"]["color"].clone();
        let mut gg = G { nodes: g["nodes"].as_array()?.clone() };
        let m = gg.mix(amt, c, rgb, "mix");
        g["nodes"] = json!(gg.nodes);
        g["out"]["color"] = m;
    }
    // ceilings: plaster of any hue, pulled toward off-white with its texture kept
    if node.ends_with(".ceiling") {
        let c = g["out"]["color"].clone();
        let mut gg = G { nodes: g["nodes"].as_array()?.clone() };
        let m = gg.mix(0.85, c, json!([0.88, 0.87, 0.84]), "mix");
        g["nodes"] = json!(gg.nodes);
        g["out"]["color"] = m;
    }
    Some(g)
}

/// ceramic/plaster.py. `tint`: the room's wall colour (its muted paint), where the shader would draw any hue
fn plaster(r: &mut R, tint: Option<[f64; 3]>) -> Value {
    let (front, back) = match tint {
        Some(c) => { let k = r.uniform(0.82, 0.95); (c, [c[0] * k, c[1] * k, c[2] * k]) }
        None => {
            let hue = r.f();
            let fv = r.log_uniform(0.5, 1.0);
            let bv = fv * r.uniform(0.6, 1.0);
            if r.f() < 0.4 { (hsv(hue, r.uniform(0.3, 0.5), fv), hsv(hue + r.uniform(-0.1, 0.1), r.uniform(0.3, 0.5), bv)) } else { (hsv(hue, 0.0, fv), hsv(hue, 0.0, bv)) }
        }
    };
    let mut g = G::default();
    let v = g.coord();
    let (d0, d1, dist) = (r.log_uniform(15.0, 30.0), r.log_uniform(15.0, 30.0), r.log_uniform(4.0, 8.0));
    let mus = g.musgrave(&v, 5.0, d0, 0.0, 2.0, 0.0);
    let n = g.noise(&v, 5.0, d1, dist, 0.0);
    let (a, b) = (r.uniform(0.3, 0.5), r.uniform(0.2, 0.3));
    let n = g.ramp(&n, &[(0.0, [0.0; 3]), (a, [1.0; 3])]);
    let d = g.mix(1.0, mus, n, "difference");
    let color = g.ramp(&d, &[(b, back), (1.0, front)]);
    let bump = g.musgrave(&v, r.uniform(1e3, 2e3), 2.0, 2.0, 2.0, 0.0);
    let h = g.mul(&bump, r.log_uniform(1e-4, 3e-4));
    let rough = r.uniform(0.7, 0.8);
    g.done("shader_plaster", json!({"color": color, "roughness": rough, "height": h}))
}

/// ceramic/ceramic.py
fn ceramic(r: &mut R, white: Option<[f64; 3]>, rmax: f64) -> Value {
    let v0 = r.log_uniform(0.3, 0.6);
    let color = white.unwrap_or_else(|| if r.f() < 0.8 { hsv(r.f(), r.uniform(0.2, 0.4), v0) } else { hsv(0.0, 0.0, v0) });
    let mut g = G::default();
    let v = g.coord();
    let m = g.musgrave(&v, r.log_uniform(20.0, 40.0), 2.0, 2.0, 2.0, 0.0);
    let rough = g.map_range(&m, 0.0, 1.0, 0.0, rmax);
    let n = g.noise(&v, r.log_uniform(20.0, 40.0), 2.0, 0.0, 0.0);
    let h = g.mul(&n, r.log_uniform(1e-3, 5e-3));
    g.done("shader_ceramic", json!({"color": color, "roughness": rough, "coat": 1.0, "coat_roughness": rough, "specular": 1.0, "height": h}))
}

/// plastic/plastic_rough.py
fn rough_plastic(r: &mut R, kw: &Value) -> Value {
    let rough0 = kw.get("roughness").and_then(|v| v.as_f64()).unwrap_or_else(|| r.f());
    let seed = kw.get("seed").and_then(|v| v.as_f64()).unwrap_or_else(|| r.uniform(-1000.0, 1000.0));
    let scale = kw.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let color = match kw.get("base_color").and_then(|v| v.as_array()) {
        Some(c) if c.len() >= 3 => [c[0].as_f64().unwrap_or(0.5), c[1].as_f64().unwrap_or(0.5), c[2].as_f64().unwrap_or(0.5)],
        _ if kw.get("clear").and_then(|v| v.as_bool()).unwrap_or(false) => hsv(0.0, 0.0, r.log_uniform(0.02, 0.8)),
        _ => hsv(r.f(), r.uniform(0.5, 0.8), r.log_uniform(0.01, 0.5)),
    };
    let rough = 0.05 + 0.2 * rough0;
    let mut g = G::default();
    let v = g.coord();
    let n = g.noise(&v, scale * 2000.0, 15.0, 2.0, seed);
    let h = g.mul(&n, rough * 0.4 * 0.003);
    g.done("shader_rough_plastic", json!({"color": color, "roughness": rough, "height": h}))
}

/// wood/plywood.py shader_shelves_white
fn white_plywood(r: &mut R) -> Value {
    let v = r.uniform(0.7, 1.0);
    let k = |r: &mut R| v * (1.0 + 0.005 * (r.f() + r.f() + r.f() - 1.5));
    let c = [k(r), k(r), k(r)];
    G::default().done("shader_shelves_white", json!({"color": c, "roughness": r.uniform(0.7, 1.0)}))
}

/// wood/plywood.py shader_shelves_wood (blonde) and shader_shelves_black_wood
fn shelves_wood(r: &mut R, black: bool, z_axis: bool) -> Value {
    let stops = if black {
        let (d, m) = (r.uniform(0.0, 0.01), r.uniform(0.01, 0.03));
        [(0.15, [d; 3]), (0.5, [m; 3]), (1.0, [r.uniform(0.015, 0.035), r.uniform(0.0, 0.01), r.uniform(0.0, 0.01)])]
    } else {
        let (b_, m_, d_) = (hsv(r.uniform(0.03, 0.09), r.uniform(0.5, 0.7), r.uniform(0.7, 1.0)), hsv(r.uniform(0.02, 0.06), r.uniform(0.6, 1.0), r.uniform(0.3, 0.6)), hsv(r.uniform(0.03, 0.05), r.uniform(0.6, 1.0), r.uniform(0.1, 0.3)));
        [(0.02, d_), (0.11, m_), (0.8, b_)]
    };
    let (wave, rough) = (r.uniform(1.0, 3.0), r.uniform(0.75, 1.0));
    let mut g = G::default();
    let v = g.coord();
    let m1 = g.mapping(&v, if z_axis { [0.1, 0.1, 2.0] } else { [0.1, 2.0, 2.0] }, [0.0; 3]);
    let n1 = g.noise(&m1, 100.0, 10.0, 2.0, 0.0);
    let vo = g.voronoi_color(&n1, 40.0);
    let c1 = g.ramp(&vo, &[(0.0864, [0.0; 3]), (0.1091, [1.0; 3])]);
    let m2 = g.mapping(&v, if z_axis { [wave, wave, 0.1] } else { [wave, 0.1, 0.1] }, [0.0; 3]);
    let n2 = g.noise(&m2, 3.0, 15.0, 2.0, 0.0);
    let nc = g.noise_color(&m2, 3.0, 15.0, 2.0);
    let mu = g.musgrave(&n2, 20.0, 3.0, 2.0, 2.0, 0.0);
    let mx = g.mix(0.5, mu, nc, "mix");
    let c2 = g.ramp(&mx, &[(0.0818, [0.0; 3]), (0.85, [1.0; 3])]);
    let f = g.mix(0.6, c1.clone(), c2.clone(), "mix");
    let color = g.ramp(&f, &stops);
    let rel = g.mix(0.004, c1, c2, "mix");
    let h = g.mul(&rel, 0.01);
    g.done(if black { "shader_shelves_black_wood" } else { "shader_shelves_wood" }, json!({"color": color, "roughness": rough, "height": h}))
}

/// wood/wood.py
fn wood(r: &mut R, vertical: bool) -> Value {
    // assets/colors.py wood_colors
    const WOOD: [u32; 49] = [0x4C2F27,0x69432D,0x371803,0x7F4040,0xCC9576,0x9E8170,0x3D2B1F,0x8D6A58,0x8B3325,0x79443C,0x88540B,0x9B5F43,0x4E3828,0x4E3828,0xC09A6B,0x944536,0x3F0110,0x773C12,0x6E4E37,0x5C4033,0x5C4033,0x3C3034,0x96704C,0x371B1A,0x483B32,0x43141A,0x471713,0xC3B090,0x6B4423,0x674D46,0x5D2E1A,0x331C1F,0x7A5640,0xB99984,0x71543D,0x8F4B28,0x491A00,0x836446,0x7F461B,0x6A3208,0x724115,0xA0522B,0x832A0C,0x371B1A,0xC7A373,0x483B32,0x635147,0x664228,0x5C5248];
    let c0 = crate::hex_to_hsv(WOOD[(r.u64() % WOOD.len() as u64) as usize]);
    let color = hsv(c0[0] + r.uniform(0.0, 0.05), (c0[1] + r.uniform(-0.3, 0.2)).clamp(0.0, 1.0), (c0[2] * r.log_uniform(0.2, 20.0)).min(1.0));
    let w = r.f();
    let mut g = G::default();
    let c = g.coord();
    let v = if vertical {
        let quarter = if r.f() < 0.5 { 0.0 } else { std::f64::consts::FRAC_PI_2 };
        g.mapping(&c, [1.0; 3], [std::f64::consts::FRAC_PI_2, 0.0, quarter])
    } else { c };
    let m2 = g.mapping(&v, [5.0, 100.0, 100.0], [0.0; 3]);
    let mu2 = g.musgrave(&m2, 10.0, 15.0, 7.0, 2.0, w);
    let mr2 = g.map_range(&mu2, 0.0, 1.0, 1.0, -1.0);
    let n1 = g.noise(&v, 0.5, 1.0, 1.1, w);
    let mu1 = g.musgrave(&v, n1, 15.0, 0.2, 2.4, w);
    let mr = g.map_range(&mu1, 0.0, 1.0, -1.4, 1.5);
    let mr1 = g.map_range(&mr, 0.0, 1.0, 1.0, 0.5);
    let m0 = g.mapping(&v, [0.15, 1.0, 0.15], [0.0; 3]);
    let n0 = g.noise(&m0, 5.0, 5.0, 1.0, w);
    let mu0 = g.musgrave(&n0, 4.0, 10.0, 0.0, 2.0, w);
    let mx = g.mix(0.5, n0, mu0, "mix");
    let mx1 = g.mix(0.9, mr1, mx, "multiply");
    let mx2 = g.mix(0.95, mr2, mx1, "multiply");
    let dark = g.hsv_adjust(json!(color), 0.8, 0.2);
    let col = g.mix(mx2.clone(), dark, json!(color), "mix");
    let h = g.mul(&mx2, r.log_uniform(2e-4, 1e-2));
    let rn = g.noise(&c_of(&g), r.log_uniform(40.0, 50.0), 2.0, 0.0, 0.0);
    let (r0, dr) = (r.uniform(0.0, 0.4), r.uniform(0.0, 0.8));
    let rough = g.map_range(&rn, 0.0, 1.0, r0, r0 + dr);
    let coat = r.uniform(0.0, 1.4).min(1.0);
    g.done("shader_wood", json!({"color": col, "roughness": rough, "coat": coat, "height": h}))
}

/// the graph's object coordinate (node 0)
fn c_of(_g: &G) -> Value { crate::link(0) }
