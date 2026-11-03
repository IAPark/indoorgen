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
    /// how often the room wants it at all, drawn per room (rooms of one house differ)
    #[serde(default = "one")] pub chance: f64,
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
    /// its soft parts keep to the walk and the doors like hard ones (foliage; bedding may spread over them)
    #[serde(default)] pub soft_blocks: bool,
    /// it lies on the floor under what stands there: only the walls limit it (a rug)
    #[serde(default)] pub cover: bool,
    /// the piece is drawn again until it fits these
    #[serde(default)] pub fit: Option<Fit>,
    #[serde(default)] pub terms: Vec<Term>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Fit { #[serde(default = "big")] pub max_w: f64, #[serde(default = "big")] pub max_h: f64, #[serde(default = "eight")] pub draws: u32 }
fn big() -> f64 { 1e9 }
fn eight() -> u32 { 8 }
fn as_is() -> Vec<Variant> { vec![Variant { piece: "as_is".into(), side: false, over_window: false }] }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Variant { pub piece: String, #[serde(default)] pub side: bool, /// a last resort: it may stand before a window after all
    #[serde(default)] pub over_window: bool }

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// back against a wall, sliding along it
    Wall,
    /// its back in line with `parent`'s, next to its +y / -y side
    Beside { parent: String, side: i8, #[serde(default)] gap: f64 },
    /// on a wall at these centre heights, tried in order; one `tall_from` m or taller hangs with its bottom at `tall_bottom` instead
    Hang { #[serde(default)] centers: Vec<f64>, #[serde(default = "big")] tall_from: f64, #[serde(default)] tall_bottom: f64 },
    /// sized from `parent` and lying under it: as wide as it plus `add_w`, `share` of its length, reaching `past` beyond its foot; both clamped
    Under { parent: String, add_w: f64, share: f64, past: f64, w: [f64; 2], d: [f64; 2], #[serde(default = "thick")] t: f64 },
}
fn thick() -> f64 { 0.015 }

/// a score term: `w` times a value of about 0..1
#[derive(Deserialize, Clone, Debug)]
pub struct Term { pub name: String, #[serde(default = "one")] pub w: f64, #[serde(default)] pub of: Option<String> }
fn one() -> f64 { 1.0 }

/// the rules built in (LAYOUT_RULES names a file to read instead)
pub fn default_rules() -> HashMap<String, RoomRules> {
    if let Some(p) = std::env::var_os("LAYOUT_RULES") { return load(&p.to_string_lossy()) }
    serde_json::from_str(include_str!("../../../rules/layout.json")).expect("rules/layout.json")
}

pub fn load(path: &str) -> HashMap<String, RoomRules> {
    let txt = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&txt).unwrap_or_else(|e| panic!("{path}: {e}"))
}
