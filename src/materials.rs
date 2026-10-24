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

pub fn wall_table(room_type: &str) -> Weighted {
    match room_type {
        "kitchen" => KITCHEN_WALL,
        "garage" => GARAGE_WALL,
        "utility" => UTILITY_WALL,
        "balcony" => BALCONY_WALL,
        "bathroom" | "restroom" => BATHROOM_WALL,
        _ => WALL,
    }
}

pub fn floor_table(room_type: &str) -> Weighted {
    match room_type {
        "bathroom" | "balcony" => BATHROOM_FLOOR,
        "utility" => UTILITY_FLOOR,
        "garage" => &[("ceramic.Concrete", 1.0)],
        _ => FLOOR,
    }
}

pub fn spec_material(name: &str, spec: serde_json::Value, fallback_rgb: [f32; 3]) -> Material {
    let mut m = Material::diffuse(name, fallback_rgb, 0.7);
    m.spec = Some(spec);
    m
}

pub struct RoomMaterials {
    pub wall: Material,
    pub floor: Material,
    pub ceiling: Material,
}

/// room_walls / room_floors / room_ceilings material choices for one room
pub fn room_materials(rng: &mut Rng, room: &str, room_type: &str) -> RoomMaterials {
    let wall_cls = weighted(rng, wall_table(room_type));
    let shape = ["square", "rectangle", "hexagon"][rng.choice_idx(3)];
    let kw = if wall_cls == "ceramic.Brick" { json!({}) } else { json!({"vertical": true, "alternating": false, "shape": shape}) };
    let kw = if wall_cls == "ceramic.Plaster" || wall_cls == "ceramic.Concrete" { json!({"vertical": true, "alternating": false, "shape": shape}) } else { kw };
    let floor_cls = weighted(rng, floor_table(room_type));
    RoomMaterials {
        wall: spec_material(&format!("wall:{room}"), json!({"cls": wall_cls, "kwargs": kw, "seed": rng.randint(0, 1_000_000)}), [0.78, 0.76, 0.7]),
        floor: spec_material(&format!("floor:{room}"), json!({"cls": floor_cls, "kwargs": {}, "seed": rng.randint(0, 1_000_000)}), [0.45, 0.32, 0.2]),
        ceiling: spec_material(&format!("ceiling:{room}"), json!({"cls": "ceramic.Plaster", "kwargs": {}, "seed": rng.randint(0, 1_000_000)}), [0.9, 0.9, 0.9]),
    }
}
