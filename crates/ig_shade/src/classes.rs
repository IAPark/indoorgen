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
        "wood.HardwoodFloor" => hardwood(&mut r, base),
        "ceramic.Tile" => tile(&mut r, &kw, None, base),
        "ceramic.Brick" => brick(&mut r, base),
        "ceramic.Concrete" => concrete(&mut r, base),
        // (a sink's basin is glazed, whatever its counter is)
        "ceramic.Marble" | "ceramic.MarbleRegular" | "ceramic.MarbleVoronoi" if family == "sink" => ceramic(&mut r, Some([0.88, 0.88, 0.86]), 0.3),
        "ceramic.Marble" | "ceramic.MarbleRegular" | "ceramic.MarbleVoronoi" => marble(&mut r, base),
        "fabric.FineKnitFabric" => fabric(&mut r, base, 250.0, 0.0006, 0.9),
        "fabric.CoarseKnitFabric" | "fabric.LinedFabric" => fabric(&mut r, base, 90.0, 0.0015, 0.95),
        "fabric.SofaFabric" | "fabric.PlaidFabric" => fabric(&mut r, base, 160.0, 0.001, 0.92),
        "fabric.Velvet" => fabric(&mut r, base, 400.0, 0.0004, 0.75),
        "fabric.Rug" => fabric(&mut r, base, 70.0, 0.003, 0.97),
        "fabric.Leather" => leather(&mut r, base),
        "wood.StaggeredWoodTile" => tile(&mut r, &json!({"shape": "staggered"}), Some(()), base),
        "wood.SquareWoodTile" | "wood.CrossedWoodTile" => tile(&mut r, &json!({"shape": "square"}), Some(()), base),
        "wood.CompositeWoodTile" => tile(&mut r, &json!({"shape": "rectangle"}), Some(()), base),
        "wood.HexagonWoodTile" => tile(&mut r, &json!({"shape": "diamond"}), Some(()), base),
        "metal.MetalBasic" | "metal.GalvanizedMetal" | "metal.GrainedMetal" | "metal.HammeredMetal" => metal(&mut r, base, false),
        "metal.BrushedMetal" => metal(&mut r, base, true),
        "metal.BrushedBlackMetal" => metal(&mut r, [0.02, 0.02, 0.022], true),
        "wood.Wood" => {
            // large flat panels of any orientation: straight-grain veneer (the noise-warped grain read as burl or contour lines there), the grain
            // up the face on upright ones
            let mut e = ext;
            e.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if e[1] > 0.35 && e[0] < 0.12 * e[1] {
                {
                    // (black veneer on tops only: on doors and wardrobes it made near-black slabs)
                    let upright = ext[2] > e[0] + 1e-6;
                    shelves_wood(&mut r, !upright && crc32fast::hash(node.split('#').next().unwrap_or(node).as_bytes()) % 10 >= 7, upright)
                }
            } else {
                // the grain runs along x: parts thin in x showed end grain, so they get it vertical
                wood(&mut r, ext[0] < 0.5 * ext[1].max(ext[2]))
            }
        }
        // glass, with no parts of its own to see: Blender's glass shader, the previewer's glass pass
        "ceramic.Glass" | "ceramic.ColoredGlass" | "ceramic.GlassVolume" | "shader.ceramic.glass.shader_glass" | "window.glass" => glass(&mut r, cls, base, mat),
        "metal.Mirror" => G::default().done("shader_mirror", json!({"color": [0.95, 0.95, 0.95], "metallic": 1.0, "roughness": 0.0})),
        "metal.WhiteMetal" => metal(&mut r, [0.85, 0.85, 0.85], false),
        "lamp_shaders.shader_black" => G::default().done("shader_black", json!({"color": base, "roughness": 0.5})),
        "lamp_shaders.shader_lamp_bulb_nonemissive" => G::default().done("shader_bulb", json!({"color": base, "roughness": 0.3})),
        "lamp_shaders.LampShade" => G::default().done("shader_lampshade", json!({"color": base, "roughness": r.uniform(0.5, 0.8), "transmission": r.uniform(0.05, 0.2)})),
        "plastic.Plastic" => rough_plastic(&mut r, &json!({"base_color": base, "scale": 1.0})),
        "plastic.PlasticTranslucent" => G::default().done("shader_translucent_plastic", json!({"color": base, "roughness": 0.2, "transmission": 0.6})),
        "ceramic.VaseCeramic" => ceramic(&mut r, Some(base), 0.5),
        "art.ArtRug" => fabric(&mut r, base, 70.0, 0.003, 0.97),
        // people have their own textures (tools/render/people_mats.py)
        c if c.starts_with("people.") => return None,
        // everything else (labels, covers, plants, soil, trinkets, terrain): the part's colour with a natural unevenness
        _ => mottled(&mut r, base),
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
        // (a faint mottle: at the shader's contrast painted walls read as camouflage)
        Some(c) => { let k = r.uniform(0.94, 0.98); (c, [c[0] * k, c[1] * k, c[2] * k]) }
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
    let h = g.mul(&n, r.log_uniform(1e-4, 4e-4)); // (glaze is smooth: at python's up to 5 mm toilets and basins looked hand-moulded)
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
        // (muted: at the shader's saturation furniture came out traffic yellow and orange)
        _ => hsv(r.f(), r.uniform(0.08, 0.35), r.log_uniform(0.03, 0.5)),
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
        // (the bright end less saturated than the shader's: in linear colour it came out peach)
        let (b_, m_, d_) = (hsv(r.uniform(0.07, 0.11), r.uniform(0.32, 0.45), r.uniform(0.7, 1.0)), hsv(r.uniform(0.02, 0.06), r.uniform(0.6, 1.0), r.uniform(0.3, 0.6)), hsv(r.uniform(0.03, 0.05), r.uniform(0.6, 1.0), r.uniform(0.1, 0.3)));
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

/// wood/wood.py: returns (colour, roughness, coat, relief) for the grain coordinates `v`, in a graph whose node 0 is the object coordinate
fn wood_on(g: &mut G, r: &mut R, v: &Value, tint: Option<[f64; 3]>) -> (Value, Value, f64, Value) {
    // assets/colors.py wood_colors
    const WOOD: [u32; 49] = [0x4C2F27,0x69432D,0x371803,0x7F4040,0xCC9576,0x9E8170,0x3D2B1F,0x8D6A58,0x8B3325,0x79443C,0x88540B,0x9B5F43,0x4E3828,0x4E3828,0xC09A6B,0x944536,0x3F0110,0x773C12,0x6E4E37,0x5C4033,0x5C4033,0x3C3034,0x96704C,0x371B1A,0x483B32,0x43141A,0x471713,0xC3B090,0x6B4423,0x674D46,0x5D2E1A,0x331C1F,0x7A5640,0xB99984,0x71543D,0x8F4B28,0x491A00,0x836446,0x7F461B,0x6A3208,0x724115,0xA0522B,0x832A0C,0x371B1A,0xC7A373,0x483B32,0x635147,0x664228,0x5C5248];
    let c0 = crate::hex_to_hsv(WOOD[(r.u64() % WOOD.len() as u64) as usize]);
    // (brightness up to 3x and capped, saturation not raised: python's up to 20x turned honey browns traffic yellow and orange)
    let drawn = hsv(c0[0] + r.uniform(0.0, 0.03), (c0[1] + r.uniform(-0.3, 0.0)).clamp(0.2, 0.65), (c0[2] * r.log_uniform(0.7, 3.0)).min(0.55));
    // (floors keep the room's plain wood colour: the drawn ones reached crimson)
    let color = tint.unwrap_or(drawn);
    let w = r.f();
    let m2 = g.mapping(v, [5.0, 100.0, 100.0], [0.0; 3]);
    let mu2 = g.musgrave(&m2, 10.0, 15.0, 7.0, 2.0, w);
    let mr2 = g.map_range(&mu2, 0.0, 1.0, 1.0, -1.0);
    let n1 = g.noise(v, 0.5, 1.0, 1.1, w);
    let mu1 = g.musgrave(v, n1, 15.0, 0.2, 2.4, w);
    let mr = g.map_range(&mu1, 0.0, 1.0, -1.4, 1.5);
    let mr1 = g.map_range(&mr, 0.0, 1.0, 1.0, 0.5);
    let m0 = g.mapping(v, [0.15, 1.0, 0.15], [0.0; 3]);
    let n0 = g.noise(&m0, 5.0, 5.0, 1.0, w);
    let mu0 = g.musgrave(&n0, 4.0, 10.0, 0.0, 2.0, w);
    let mx = g.mix(0.5, n0, mu0, "mix");
    let mx1 = g.mix(0.9, mr1, mx, "multiply");
    let mx1_soft = mx1.clone();
    let mx2 = g.mix(0.95, mr2, mx1, "multiply");
    // (grain darkening to 40 %, and only the darkest grain all the way: at the shader's 20 % over the whole range floors went blotchy)
    let dark = g.hsv_adjust(json!(color), 0.85, 0.4);
    let fac = g.map_range(&mx2, 0.0, 0.5, 0.35, 1.0);
    let col = g.mix(fac, dark, json!(color), "mix");
    let rn = g.noise(&c_of(&g), r.log_uniform(40.0, 50.0), 2.0, 0.0, 0.0);
    let (r0, dr) = (r.uniform(0.0, 0.4), r.uniform(0.0, 0.8));
    let rough = g.map_range(&rn, 0.0, 1.0, r0, r0 + dr);
    // relief from the smoother grain layer, 1.5 mm filled by the varnish (the shader's up to 1 cm of its fibre noise crumpled the wood like foil)
    let coat = r.uniform(0.0, 1.4).min(1.0);
    let h = g.mul(&mx1_soft, 0.0015 * (1.0 - 0.9 * coat));
    (col, rough, coat, h)
}

fn wood(r: &mut R, vertical: bool) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let v = if vertical {
        let quarter = if r.f() < 0.5 { 0.0 } else { std::f64::consts::FRAC_PI_2 };
        g.mapping(&c, [1.0; 3], [std::f64::consts::FRAC_PI_2, 0.0, quarter])
    } else { c };
    let (col, rough, coat, h) = wood_on(&mut g, r, &v, None);
    g.done("shader_wood", json!({"color": col, "roughness": rough, "coat": coat, "height": h}))
}

/// wood/hardwood_floor.py: planks with the grain along them (python's grain ran across the planks of most floors), each its own cut of the wood
fn hardwood(r: &mut R, rgb: [f64; 3]) -> Value {
    let rot = if r.f() < 0.1 { 0.0 } else { std::f64::consts::FRAC_PI_2 };
    let mut g = G::default();
    let c = g.coord();
    let m = g.mapping(&c, [1.0; 3], [0.0, 0.0, rot]);
    let (row, width) = (r.log_uniform(0.06, 0.15), r.log_uniform(0.6, 1.0));
    let (shade, mortar) = g.brick(&m, 1.0, row, width, 0.5, 0.002);
    let v = g.add_v(&m, shade);
    let (col, rough, coat, h) = wood_on(&mut g, r, &v, Some(rgb));
    let color = g.mix(mortar, col, json!([0.0, 0.0, 0.0]), "mix");
    g.done("shader_hardwood_floor", json!({"color": color, "roughness": rough, "coat": coat, "height": h}))
}

/// ceramic/tile.py and the wood tiles. Square, rectangle and staggered as in python; its hexagon, crossed and composite patterns are not ported:
/// they come out as diamonds and squares. A glaze keeps the room's plain colour `rgb`, where the shader would draw any hue
fn tile(r: &mut R, kw: &Value, wood_base: Option<()>, rgb: [f64; 3]) -> Value {
    let shape = kw.get("shape").and_then(|v| v.as_str()).unwrap_or(["square", "rectangle", "staggered", "diamond"][(r.u64() % 4) as usize]);
    let vertical = kw.get("vertical").and_then(|v| v.as_bool()).unwrap_or(false);
    let scale = kw.get("scale").and_then(|v| v.as_f64()).unwrap_or_else(|| r.log_uniform(1.0, 2.0));
    let mut g = G::default();
    let c = g.coord();
    let u = if vertical { g.wall_uv(&c) } else { c.clone() };
    let quarter = std::f64::consts::FRAC_PI_2;
    let turn = (r.u64() % 4) as f64 * quarter;
    let (m, sc, row, width, offset, check) = match shape {
        "staggered" => {
            let hs = scale * r.log_uniform(6.0, 10.0); // (boards 8-16 cm wide: python's reached 50)
            let vs = hs * r.log_uniform(0.05, 0.2);
            (g.mapping(&u, [1.0, 1.0, 0.0], [0.0; 3]), 1.0, 1.0 / hs, 1.0 / vs, 0.5, false)
        }
        "rectangle" => {
            let size = r.log_uniform(0.14, 0.28); // (home tile: at python's up to 80 cm a wall read as paving)
            (g.mapping(&u, [scale, scale * r.log_uniform(1.3, 2.0), 0.0], [0.0, 0.0, turn]), 1.0 / size, 1.0, 1.0, 0.5, false)
        }
        _ => {
            let size = r.log_uniform(0.14, 0.28); // (home tile: at python's up to 80 cm a wall read as paving)
            let diamond = if shape == "diamond" || r.f() < 0.3 { quarter / 2.0 } else { 0.0 };
            (g.mapping(&u, [scale, scale, 0.0], [0.0, 0.0, turn + diamond]), 1.0 / size, 1.0, 1.0, 0.0, true)
        }
    };
    // (hairline joints between boards: at a tile's width they read as decking)
    let joint = if shape == "staggered" { r.uniform(0.0012, 0.002) } else { r.uniform(0.005, 0.01) };
    let (shade, mortar) = g.brick(&m, sc, row, width, offset, joint);
    let alternating = check && r.f() < 0.75;
    let chk = alternating.then(|| g.checker(&m, sc));
    let (col, rough, coat, h0): (Value, Value, f64, Option<Value>) = match wood_base {
        Some(()) => {
            // each tile its own cut; alternate ones turned a quarter
            let base = match &chk {
                Some(k) => { let t = g.mapping(&m, [1.0; 3], [0.0, 0.0, quarter]); g.mix(k.clone(), m.clone(), t, "mix") }
                None => m.clone(),
            };
            let v = g.add_v(&base, shade);
            let (a, b, c_, h) = wood_on(&mut g, r, &v, Some(rgb));
            (a, b, c_, Some(h))
        }
        None => {
            let k = r.uniform(0.82, 0.98);
            let dark = [rgb[0] * k, rgb[1] * k, rgb[2] * k];
            let cc = match &chk { Some(q) => g.mix(q.clone(), json!(rgb), json!(dark), "mix"), None => json!(rgb) };
            let (lo, hi) = (r.uniform(0.1, 0.3), r.uniform(0.6, 0.8));
            let mu = g.musgrave(&c, r.log_uniform(20.0, 40.0), 2.0, 2.0, 2.0, 0.0);
            (cc, g.map_range(&mu, 0.0, 1.0, lo, hi), 1.0, None)
        }
    };
    let dk = g.mul(&mortar, r.uniform(0.4, 1.0));
    let color = g.mix(dk, col, json!([0.0, 0.0, 0.0]), "mix");
    let h = g.mul(&mortar, -r.uniform(0.01, 0.02));
    let _ = h0;
    g.done("tile", json!({"color": color, "roughness": rough, "coat": coat, "height": h}))
}

/// a running-bond brick wall in the room's brick colour (a simple graph, not a port of ceramic/brick.py)
fn brick(r: &mut R, rgb: [f64; 3]) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let u = g.wall_uv(&c);
    let (shade, mortar) = g.brick(&u, 1.0, r.uniform(0.07, 0.085), r.uniform(0.21, 0.25), 0.5, 0.005);
    let k = r.uniform(0.55, 0.7);
    let dark = json!([rgb[0] * k, rgb[1] * k, rgb[2] * k]);
    let col = g.mix(shade, json!(rgb), dark, "mix");
    let n = g.noise(&c, 30.0, 4.0, 0.0, 0.0);
    let col = g.mix(0.25, col, n, "multiply");
    let color = g.mix(mortar.clone(), col, json!([0.5, 0.48, 0.45]), "mix");
    let h = g.mul(&mortar, -0.004);
    g.done("shader_brick", json!({"color": color, "roughness": r.uniform(0.8, 0.95), "height": h}))
}

/// cloth in the part's own colour (white bedding stays white, where the shaders draw any colour): a fine weave as relief, and the cm-scale
/// unevenness that keeps it from reading as plastic. Compact graphs, not ports of fabric/*.py
fn fabric(r: &mut R, rgb: [f64; 3], threads: f64, depth: f64, rough: f64) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let mottle = g.noise(&c, r.uniform(20.0, 35.0), 4.0, 0.0, r.uniform(0.0, 10.0));
    let tone = g.map_range(&mottle, 0.3, 0.7, 0.93, 1.0);
    let color = g.mix(1.0, json!(rgb), tone, "multiply");
    let wx = g.mapping(&c, [1.0, 0.25, 0.25], [0.0; 3]);
    let wy = g.mapping(&c, [0.25, 1.0, 0.25], [0.0; 3]);
    let a = g.noise(&wx, threads, 2.0, 0.0, 0.0);
    let b = g.noise(&wy, threads, 2.0, 0.0, 0.0);
    let weave = g.mix(0.5, a, b, "mix");
    let h = g.mul(&weave, depth);
    g.done("shader_fabric", json!({"color": color, "roughness": rough, "height": h}))
}

/// a fine pebbled grain in the part's colour
fn leather(r: &mut R, rgb: [f64; 3]) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let n = g.noise(&c, r.uniform(150.0, 300.0), 3.0, 0.5, 0.0);
    let m = g.noise(&c, r.uniform(8.0, 14.0), 3.0, 0.0, 0.0);
    let tone = g.map_range(&m, 0.3, 0.7, 0.85, 1.0);
    let color = g.mix(1.0, json!(rgb), tone, "multiply");
    let h = g.mul(&n, 0.0012);
    g.done("shader_leather", json!({"color": color, "roughness": r.uniform(0.35, 0.55), "height": h}))
}

/// cloudy grey-brown cast concrete in the part's colour
fn concrete(r: &mut R, rgb: [f64; 3]) -> Value {
    let mut g = G::default();
    let c = g.coord();
    // (a faint unevenness: stronger, and warped, it drew contour lines across the wall)
    let n = g.noise(&c, r.uniform(1.5, 3.0), 6.0, 0.0, 0.0);
    let tone = g.map_range(&n, 0.25, 0.75, 0.94, 1.02);
    let color = g.mix(1.0, json!(rgb), tone, "multiply");
    let p = g.noise(&c, 200.0, 3.0, 0.0, 0.0);
    let h = g.mul(&p, 0.001);
    g.done("shader_concrete", json!({"color": color, "roughness": r.uniform(0.75, 0.95), "height": h}))
}

/// polished veined stone: thin dark lines where warped noise crosses its middle
fn marble(r: &mut R, rgb: [f64; 3]) -> Value {
    let mut g = G::default();
    let c = g.coord();
    // (broad, faint veins: dark and dense they read as scribbles at room scale)
    let n = g.noise(&c, r.uniform(0.8, 1.5), 6.0, r.uniform(1.0, 2.5), 0.0);
    let vein = g.ramp(&n, &[(0.485, [0.0; 3]), (0.5, [1.0; 3]), (0.515, [0.0; 3])]);
    let k = r.uniform(0.62, 0.8);
    let color = g.mix(vein, json!(rgb), json!([rgb[0] * k, rgb[1] * k, rgb[2] * k]), "mix");
    g.done("shader_marble", json!({"color": color, "roughness": r.uniform(0.08, 0.2), "coat": 0.5}))
}

/// ceramic/glass.py: `glass` selects Blender's glass shader; window panes refract nothing (a plane, not a slab)
fn glass(r: &mut R, cls: &str, rgb: [f64; 3], mat: &str) -> Value {
    let tint = if cls == "ceramic.ColoredGlass" { rgb } else if mat.starts_with("shower_glass") { [0.94, 0.97, 0.96] } else { [0.98, 0.98, 0.98] };
    let ior = if cls == "window.glass" || mat.ends_with(".glass") { 1.01 } else { r.uniform(1.45, 1.52) };
    G::default().done("shader_glass", json!({"color": tint, "roughness": if mat.starts_with("shower_glass") { 0.04 } else { 0.02 }, "glass": true, "ior": ior}))
}

/// a part's colour with a soft unevenness and a little relief
fn mottled(r: &mut R, rgb: [f64; 3]) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let n = g.noise(&c, r.uniform(8.0, 20.0), 5.0, 0.0, 0.0);
    let tone = g.map_range(&n, 0.3, 0.7, 0.88, 1.04);
    let color = g.mix(1.0, json!(rgb), tone, "multiply");
    let h = g.mul(&n, 0.0015);
    g.done("shader_plain", json!({"color": color, "roughness": r.uniform(0.5, 0.85), "height": h}))
}

/// metal/metal_basic.py and brushed_metal.py, in the part's own colour; the galvanized, hammered and grained metals are plain for now
fn metal(r: &mut R, rgb: [f64; 3], brushed: bool) -> Value {
    let mut g = G::default();
    let c = g.coord();
    let v = if brushed { g.mapping(&c, [0.2, 0.2, 5.0], [0.0; 3]) } else { c };
    let n = g.noise(&v, if brushed { 100.0 } else { r.uniform(10.0, 25.0) }, 6.0, 0.1, 0.0);
    let (lo, hi) = if brushed { (r.uniform(0.15, 0.25), r.uniform(0.3, 0.5)) } else { (r.uniform(0.0, 0.2), r.uniform(0.4, 0.7)) };
    let rough = g.map_range(&n, 0.0, 1.0, lo, hi);
    g.done("shader_metal", json!({"color": rgb, "metallic": 1.0, "roughness": rough}))
}

/// the graph's object coordinate (node 0)
fn c_of(_g: &G) -> Value { crate::link(0) }
