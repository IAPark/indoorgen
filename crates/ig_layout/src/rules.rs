//! The rule file (`rules/layout.json`), read at every run: `{ "<room kind>": { "program": [item, ...] } }`. Items go in file order within a tier.
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug)]
pub struct RoomRules {
    #[serde(default)] pub program: Vec<Item>,
    /// other ways to furnish the room: one is drawn by weight from those the room is big enough for; when its must items have no place, the next
    #[serde(default)] pub arrangements: Vec<Arrangement>,
    /// noise on every candidate's score (the terms are worth about 1)
    #[serde(default = "noise_d")] pub noise: f64,
    /// the finished layout is drawn from those within this of the best (a may item is worth 1, so under 1 none is given up for it)
    #[serde(default)] pub pick_within: f64,
    /// how much a room's own priorities differ: each term's weight times e^(+-jitter), drawn per room
    #[serde(default)] pub jitter: f64,
}
fn noise_d() -> f64 { 0.15 }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Arrangement {
    pub name: String,
    #[serde(default = "one")] pub weight: f64,
    #[serde(default)] pub min_area: f64,
    #[serde(default = "big")] pub max_area: f64,
    pub program: Vec<Item>,
}

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
    /// not wanted when one of these is placed (a shower where a tub went)
    #[serde(default)] pub unless: Vec<String>,
    /// its nearest door at least this far, m (a toilet)
    #[serde(default)] pub min_door: f64,
    /// this many of it (id#0, id#1, ...), each its own item of the same piece
    #[serde(default)] pub repeat: u32,
    /// every variant is a candidate, the later ones at a cost, not only the first that fits: what comes after it may need a smaller one
    #[serde(default)] pub all_variants: bool,
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
pub struct Fit { #[serde(default = "big")] pub max_w: f64, #[serde(default = "big")] pub max_h: f64, #[serde(default = "big")] pub max_d: f64, #[serde(default = "eight")] pub draws: u32 }
fn big() -> f64 { 1e9 }
fn eight() -> u32 { 8 }
fn as_is() -> Vec<Variant> { vec![Variant { piece: "as_is".into(), side: false, width: None, height: None, over_window: false }] }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Variant { pub piece: String, #[serde(default)] pub side: bool, /// asked to be this wide (Piece::resize), m
    #[serde(default)] pub width: Option<f64>,
    /// asked to be this high (Piece::resize), m
    #[serde(default)] pub height: Option<f64>, /// a last resort: it may stand before a window after all
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
    /// on the wall behind `parent`, centred over it: centred at `center`, or higher to stand `gap` over its top (a mirror over a basin)
    Over { parent: String, center: f64, gap: f64 },
    /// sized from `parent` and lying under it: as wide as it plus `add_w`, `share` of its length, reaching `past` beyond its foot; both clamped
    /// (`add_d`: as long as it plus this, centred under it, instead)
    Under { parent: String, add_w: f64, #[serde(default)] share: f64, #[serde(default)] past: f64, #[serde(default)] add_d: Option<f64>, w: [f64; 2], d: [f64; 2], #[serde(default = "thick")] t: f64 },
    /// opposite `parent`, facing it: its front `min`..`max` m from the parent's, up to `lateral` off centre
    Facing { parent: String, min: f64, max: f64, #[serde(default)] lateral: f64 },
    /// before `parent`'s front, `gap` m clear, as wide as `width_share` of it
    Front { parent: String, gap: f64, #[serde(default)] width_share: Option<f64> },
    /// at `parent`'s +y / -y side, turned to face across it
    /// (`forward`: that far out before its front, not beside its middle)
    Flank { parent: String, side: i8, gap: f64, #[serde(default)] forward: Option<f64> },
    /// standing on `parent`, centred on it (it must be no wider)
    On { parent: String },
    /// free on the floor, this far from the walls and doors (less, at a cost, when the room is small)
    Free { wall_clear: f64, door_clear: f64 },
    /// at the sides of `parent`, facing it: slots `spacing` m apart, its seat `tuck` under the edge; at its ends too from `end_from` m long
    /// (`tuck` as far as it goes: less, at a cost, when something is in the way)
    Around { parent: String, spacing: f64, tuck: f64, end_from: f64 },
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
