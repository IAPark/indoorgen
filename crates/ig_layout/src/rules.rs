//! The rule file (`rules/layout.json`), read at every run: `{ "<room kind>": { "program": [item, ...] } }`. Items go in file order within a tier.
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug)]
pub struct RoomRules { pub program: Vec<Item> }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub class: String,
    /// 0 must, 1 should, 2 may
    #[serde(default)] pub tier: u8,
    /// the room's size for it to be wanted, m2
    #[serde(default)] pub min_area: f64,
    /// how it stands: wall / beside
    pub place: Place,
    /// tried in order until one has a place: as_is, tucked, compact (Piece::tucked once, twice); `side` turns the long side to the wall
    #[serde(default = "as_is")] pub variants: Vec<Variant>,
    /// floor to keep free before its front, m (blocks furniture, not other openings)
    #[serde(default)] pub front: f64,
    /// taller than a sill it does not stand before a window
    #[serde(default)] pub avoid_window: bool,
    /// the walk must reach its front
    #[serde(default)] pub reach: bool,
    #[serde(default)] pub terms: Vec<Term>,
}
fn as_is() -> Vec<Variant> { vec![Variant { piece: "as_is".into(), side: false }] }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Variant { pub piece: String, #[serde(default)] pub side: bool }

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// back against a wall, sliding along it
    Wall,
    /// its back in line with `parent`'s, next to its +y / -y side
    Beside { parent: String, side: i8, #[serde(default)] gap: f64 },
}

/// a score term: `w` times a value of about 0..1
#[derive(Deserialize, Clone, Debug)]
pub struct Term { pub name: String, #[serde(default = "one")] pub w: f64 }
fn one() -> f64 { 1.0 }

pub fn load(path: &str) -> HashMap<String, RoomRules> {
    let txt = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&txt).unwrap_or_else(|e| panic!("{path}: {e}"))
}
