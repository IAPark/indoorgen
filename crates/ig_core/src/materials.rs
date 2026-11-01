//! Material selection tables (assets/composition/material_assignments.py). Materials themselves are realised
//! on the Blender side from `{"cls": "module.Class", "kwargs": {...}}` specs.
use crate::mesh::Material;
use crate::rng::Rng;
use serde_json::json;

pub type Weighted = &'static [(&'static str, f64)];

pub const WOOD_TILES: Weighted = &[
    ("wood.CompositeWoodTile", 1.0),
    ("wood.CrossedWoodTile", 1.0),
    ("wood.HardwoodFloor", 1.0),
    ("wood.HexagonWoodTile", 1.0),
    ("wood.SquareWoodTile", 1.0),
    ("wood.StaggeredWoodTile", 1.0),
];
pub const FLOOR: Weighted = &[
    ("wood.CompositeWoodTile", 1.0),
    ("wood.CrossedWoodTile", 1.0),
    ("wood.HardwoodFloor", 1.0),
    ("wood.HexagonWoodTile", 1.0),
    ("wood.SquareWoodTile", 1.0),
    ("wood.StaggeredWoodTile", 1.0),
    ("ceramic.Tile", 4.0),
    ("fabric.Rug", 1.0),
];
pub const WALL: Weighted = &[("ceramic.Plaster", 2.0), ("ceramic.Tile", 1.0)];
pub const KITCHEN_WALL: Weighted = &[("ceramic.Plaster", 5.0), ("ceramic.Tile", 2.0)];
pub const BATHROOM_WALL: Weighted = &[("ceramic.Tile", 5.0)];
pub const GARAGE_WALL: Weighted = &[("ceramic.Concrete", 5.0), ("ceramic.Brick", 1.0), ("ceramic.Plaster", 3.0)];
pub const UTILITY_WALL: Weighted = &[("ceramic.Concrete", 1.0), ("ceramic.Brick", 2.0), ("ceramic.Plaster", 5.0)];
pub const BALCONY_WALL: Weighted = &[("ceramic.Brick", 1.0), ("ceramic.Plaster", 5.0)];
pub const BATHROOM_FLOOR: Weighted = &[("ceramic.Tile", 1.0)];
pub const UTILITY_FLOOR: Weighted = &[("ceramic.Concrete", 1.0), ("ceramic.Plaster", 1.0), ("ceramic.Tile", 1.0)];
pub const CEILING: Weighted = &[("ceramic.Plaster", 1.0)];

pub fn weighted(rng: &mut Rng, table: Weighted) -> &'static str {
    let p: Vec<f64> = table.iter().map(|x| x.1).collect();
    let s: f64 = p.iter().sum();
    let p: Vec<f64> = p.iter().map(|x| x / s).collect();
    table[rng.choice_p(&p)].0
}

pub const OFFICE_FLOOR: Weighted = &[
    ("wood.CompositeWoodTile", 1.0),
    ("wood.CrossedWoodTile", 1.0),
    ("wood.HardwoodFloor", 1.0),
    ("wood.HexagonWoodTile", 1.0),
    ("wood.SquareWoodTile", 1.0),
    ("wood.StaggeredWoodTile", 1.0),
    ("fabric.Rug", 1.0),
];
pub const CONCRETE: Weighted = &[("ceramic.Concrete", 1.0)];

// plausible finishes per room type, as tools/render/roommats.py draws them for Blender (infinigen's tables gave a third of living /
// dining / bedroom walls glossy tile and 40 % of their floors tile); RAW_ROOMS=1 keeps infinigen's tables
const HOME_WALL: Weighted = &[("ceramic.Plaster", 19.0), ("ceramic.Tile", 1.0)];
const LIVING_WALL: Weighted = &[("ceramic.Plaster", 19.0), ("ceramic.Tile", 1.0), ("ceramic.Brick", 1.2)];
const KITCHEN_WALL_REAL: Weighted = &[("ceramic.Plaster", 8.0), ("ceramic.Tile", 2.0)];
const HOME_FLOOR: Weighted = &[("wood.HardwoodFloor", 5.0), ("wood.StaggeredWoodTile", 2.0), ("wood.SquareWoodTile", 1.0), ("wood.CompositeWoodTile", 1.0),
    ("wood.HexagonWoodTile", 0.3), ("wood.CrossedWoodTile", 0.3), ("ceramic.Tile", 0.9), ("fabric.Rug", 1.1)];
const KITCHEN_FLOOR: Weighted = &[("ceramic.Tile", 5.0), ("wood.HardwoodFloor", 3.0), ("wood.StaggeredWoodTile", 1.0)];
fn raw_rooms() -> bool { crate::envx::var_os("RAW_ROOMS").is_some() }

/// decorate.py room_wall_fns
pub fn wall_table(room_type: &str) -> Weighted {
    if !raw_rooms() {
        match room_type {
            "living-room" | "dining-room" => return LIVING_WALL,
            "bedroom" => return HOME_WALL,
            "kitchen" => return KITCHEN_WALL_REAL,
            _ => {}
        }
    }
    match room_type {
        "kitchen" => KITCHEN_WALL,
        "garage" | "warehouse" => GARAGE_WALL,
        "utility" => UTILITY_WALL,
        "balcony" => BALCONY_WALL,
        "bathroom" | "restroom" => BATHROOM_WALL,
        _ => WALL,
    }
}

/// decorate.py room_floor_fns
pub fn floor_table(room_type: &str) -> Weighted {
    if !raw_rooms() {
        match room_type {
            "living-room" | "dining-room" | "bedroom" => return HOME_FLOOR,
            "kitchen" => return KITCHEN_FLOOR,
            _ => {}
        }
    }
    match room_type {
        "bathroom" | "restroom" | "balcony" => BATHROOM_FLOOR,
        "utility" => UTILITY_FLOOR,
        "garage" | "warehouse" => CONCRETE,
        "office" | "factory-office" | "open-office" => OFFICE_FLOOR,
        _ => FLOOR,
    }
}

/// decorate.py room_ceiling_fns
pub fn ceiling_table(room_type: &str) -> Weighted {
    match room_type {
        "garage" | "warehouse" => CONCRETE,
        _ => CEILING,
    }
}

pub fn spec_material(name: &str, spec: serde_json::Value, fallback_rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, fallback_rgb, 0.7);
    m.spec = Some(spec);
    m
}

/// painted furniture colours (Blender linear base colour): sage, navy, charcoal, cream, terracotta, dusty blue, forest green, warm
/// grey, mustard, blush
pub const FURNITURE_PAINT: [[f32; 3]; 10] = [[0.30, 0.38, 0.28], [0.03, 0.06, 0.13], [0.06, 0.065, 0.07], [0.74, 0.69, 0.57], [0.45, 0.17, 0.09],
    [0.22, 0.33, 0.42], [0.05, 0.12, 0.07], [0.35, 0.33, 0.30], [0.55, 0.38, 0.07], [0.62, 0.40, 0.36]];

/// a painted finish (plywood under 90 % paint, a trace of its grain left) in a colour of `FURNITURE_PAINT` picked by `seed`; the
/// preview keeps `fallback_rgb`
pub fn painted_material(name: &str, seed: u64, fallback_rgb: [f32; 3]) -> Material {
    let c = FURNITURE_PAINT[(crate::int_hash(&["paint", &seed.to_string()]) % FURNITURE_PAINT.len() as u64) as usize];
    spec_material(name, serde_json::json!({"cls": "wood.WhitePlywood", "kwargs": {}, "seed": seed, "paint": {"rgb": c, "amount": 0.9}}), fallback_rgb)
}

/// How a wall gets its second material (decorate.room_walls, room_wall_alternative_fns)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WallAlt {
    None,
    /// bisect at min z + wall_height * frac; the part above gets the alternative material
    Half { frac: f64 },
    /// faces touching the longest horizontal edge line get the alternative material
    Aligned,
}

pub struct RoomMaterials {
    pub wall: Material,
    pub alt: WallAlt,
    pub alt_wall: Option<Material>,
    pub floor: Material,
    pub ceiling: Material,
}

fn alt_choice(rng: &mut Rng, room_type: &str) -> &'static str {
    const RICH: &[(f64, &str)] = &[(2.0, "none"), (1.0, "half"), (1.0, "ceramic.Plaster"), (1.0, "art.Art"), (0.5, "art.DarkArt")];
    // (roommats.py: plain paint mostly, wainscoting sometimes; printed art walls read as synthetic)
    const RICH_REAL: &[(f64, &str)] = &[(6.0, "none"), (1.5, "half")];
    let rich = if raw_rooms() { RICH } else { RICH_REAL };
    const PLAIN: &[(f64, &str)] = &[(2.0, "none"), (0.5, "half")];
    let tab = match room_type {
        "living-room" | "bedroom" | "dining-room" | "office" | "open-office" | "factory-office" | "break-room" => rich,
        _ => PLAIN,
    };
    let p: Vec<f64> = tab.iter().map(|x| x.0).collect();
    tab[rng.choice_p(&p)].1
}

/// the plain colour a material class shows where its procedural shader is not realised (previewers, the browser, glb viewers),
/// varied by the material's seed: off-white or light paint for plaster, light oak .. walnut for wood, white / grey / beige tile,
/// muted rugs, red-brown brick (every floor was one tan and every wall one beige)
pub fn fallback_rgb(cls: &str, seed: u64) -> [f32; 3] {
    let mut r = Rng::seeded(seed ^ 0x6a09_e667);
    let pick = |r: &mut Rng, pal: &[[f32; 3]]| { let c = pal[r.randint(0, pal.len() as i64) as usize]; let v = r.uniform(0.94, 1.06) as f32; [c[0] * v, c[1] * v, c[2] * v] };
    match cls {
        "ceramic.Plaster" => pick(&mut r, &[[0.82, 0.80, 0.75], [0.85, 0.84, 0.80], [0.78, 0.76, 0.70], [0.70, 0.74, 0.66], [0.66, 0.72, 0.78], [0.76, 0.70, 0.64], [0.86, 0.82, 0.70], [0.62, 0.62, 0.64]]),
        "ceramic.Tile" => pick(&mut r, &[[0.86, 0.86, 0.84], [0.62, 0.63, 0.64], [0.74, 0.70, 0.62], [0.40, 0.42, 0.44], [0.55, 0.66, 0.70], [0.80, 0.78, 0.72]]),
        "ceramic.Brick" => pick(&mut r, &[[0.45, 0.20, 0.13], [0.52, 0.28, 0.18], [0.38, 0.22, 0.17]]),
        "ceramic.Concrete" => pick(&mut r, &[[0.50, 0.50, 0.48], [0.58, 0.57, 0.54]]),
        "fabric.Rug" => pick(&mut r, &[[0.55, 0.50, 0.44], [0.36, 0.40, 0.46], [0.48, 0.30, 0.26], [0.62, 0.60, 0.56], [0.30, 0.34, 0.30]]),
        c if c.starts_with("wood.") => pick(&mut r, &[[0.62, 0.46, 0.30], [0.50, 0.34, 0.20], [0.36, 0.23, 0.14], [0.70, 0.56, 0.40], [0.55, 0.50, 0.44], [0.44, 0.30, 0.20]]),
        _ => [0.8, 0.8, 0.8],
    }
}

fn wall_kwargs(cls: &str, shape: &str) -> serde_json::Value {
    if cls == "ceramic.Brick" { json!({}) } else { json!({"vertical": true, "alternating": false, "shape": shape}) }
}

/// room_walls / room_floors / room_ceilings material decisions for all rooms of a scene, in room-mesh order
/// (rooms = (name, room_type)).  Exactness vs python:
///  * the wall / floor / ceiling CLASS of every room replays python's np.random stream: run_stage seeds each stage with
///    FixedSeed(int_hash((scene_seed, "room_walls" | "room_floors" | "room_ceilings"))) and the class draws
///    (`weighted_sample` per room, in room order) come before any other draw of the stage -> exact.
///  * everything after them (tile shape, alternative none/half/aligned, bisect height, per-material params) follows
///    python only in distribution: in python those draws come after the material generators of all rooms consumed the
///    stream (hundreds of data dependent shader draws) and in `set()` iteration order of fresh instances (memory address
///    order), so they are not reproducible; they come from a deterministic side stream with the same distributions.
pub fn decorate_rooms(seed: u64, rooms: &[(String, String)]) -> Vec<RoomMaterials> {
    let sd = seed.to_string();
    let mut wr = Rng::seeded(crate::int_hash(&[&sd, "room_walls"]));
    let wall_cls: Vec<&str> = rooms.iter().map(|r| weighted(&mut wr, wall_table(&r.1))).collect();
    let mut fr = Rng::seeded(crate::int_hash(&[&sd, "room_floors"]));
    let floor_cls: Vec<&str> = rooms.iter().map(|r| weighted(&mut fr, floor_table(&r.1))).collect();
    let mut cr = Rng::seeded(crate::int_hash(&[&sd, "room_ceilings"]));
    let ceil_cls: Vec<&str> = rooms.iter().map(|r| weighted(&mut cr, ceiling_table(&r.1))).collect();
    // side stream for the non-replayable draws
    let mut sr = Rng::seeded(crate::int_hash(&[&sd, "room_walls", "side"]));
    let mut out = vec![];
    let mut last_kw = json!({});
    for (i, (name, rt)) in rooms.iter().enumerate() {
        let shape = ["square", "rectangle", "hexagon"][sr.choice_idx(3)];
        let kw = wall_kwargs(wall_cls[i], shape);
        last_kw = kw.clone();
        let ws = sr.randint(0, 1_000_000);
        let wall = spec_material(&format!("wall:{name}"), json!({"cls": wall_cls[i], "kwargs": kw, "seed": ws}), fallback_rgb(wall_cls[i], ws as u64));
        out.push((wall, rt.as_str()));
    }
    let mut res = vec![];
    for (i, (wall, rt)) in out.into_iter().enumerate() {
        let name = &rooms[i].0;
        let a = alt_choice(&mut sr, rt);
        let (alt, alt_wall) = match a {
            "none" => (WallAlt::None, None),
            "half" => {
                let frac = sr.uniform(0.3, 0.6);
                // python: plaster.Plaster()(**kwargs) with the kwargs left over from the last room of the wall loop
                let m = spec_material(&format!("wall_alt:{name}"), json!({"cls": "ceramic.Plaster", "kwargs": last_kw, "seed": sr.randint(0, 1_000_000)}), [0.85, 0.83, 0.8]);
                (WallAlt::Half { frac }, Some(m))
            }
            cls => {
                let scale = sr.log_uniform(0.5, 2.0);
                let mut kw = last_kw.clone();
                kw["scale"] = json!(scale);
                let m = spec_material(&format!("wall_alt:{name}"), json!({"cls": cls, "kwargs": kw, "seed": sr.randint(0, 1_000_000)}), [0.85, 0.83, 0.8]);
                (WallAlt::Aligned, Some(m))
            }
        };
        let fs = sr.randint(0, 1_000_000);
        let floor = spec_material(&format!("floor:{name}"), json!({"cls": floor_cls[i], "kwargs": {}, "seed": fs}), fallback_rgb(floor_cls[i], fs as u64));
        let ceiling = spec_material(&format!("ceiling:{name}"), json!({"cls": ceil_cls[i], "kwargs": {}, "seed": sr.randint(0, 1_000_000)}), [0.9, 0.9, 0.9]);
        res.push(RoomMaterials { wall, alt, alt_wall, floor, ceiling });
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alt_hist() {
        let rooms = vec![("living-room_0/0".to_string(), "living-room".to_string()), ("kitchen_0/0".to_string(), "kitchen".to_string())];
        let mut h = std::collections::BTreeMap::new();
        for seed in 0..400u64 {
            let r = decorate_rooms(seed, &rooms);
            for (i, m) in r.iter().enumerate() {
                let k = (i, m.alt_wall.as_ref().map(|x| x.spec.as_ref().unwrap()["cls"].as_str().unwrap().to_string()), format!("{:?}", std::mem::discriminant(&m.alt)));
                *h.entry(k).or_insert(0) += 1;
            }
        }
        for (k, v) in h { println!("{k:?} {v}"); }
    }
}
