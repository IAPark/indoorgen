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

/// decorate.py room_wall_fns
pub fn wall_table(room_type: &str) -> Weighted {
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
    const PLAIN: &[(f64, &str)] = &[(2.0, "none"), (0.5, "half")];
    let tab = match room_type {
        "living-room" | "bedroom" | "office" | "open-office" | "factory-office" | "break-room" => RICH,
        _ => PLAIN,
    };
    let p: Vec<f64> = tab.iter().map(|x| x.0).collect();
    tab[rng.choice_p(&p)].1
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
        let wall = spec_material(&format!("wall:{name}"), json!({"cls": wall_cls[i], "kwargs": kw, "seed": sr.randint(0, 1_000_000)}), [0.78, 0.76, 0.7]);
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
        let floor = spec_material(&format!("floor:{name}"), json!({"cls": floor_cls[i], "kwargs": {}, "seed": sr.randint(0, 1_000_000)}), [0.45, 0.32, 0.2]);
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
