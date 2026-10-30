//! Tunable room-by-room layout rules (the furn::groups passes). Defaults are the reviewed values; native runs read
//! LAYOUT_RULES_JSON (a file with {"layout": {...}}), the browser build takes them per call (ig_wasm `layout()` "rules").

use std::sync::RwLock;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LayoutRules {
    // living room
    /// sofa and TV on facing walls: viewing distance range, m
    pub tv_view_min: f64,
    pub tv_view_max: f64,
    /// sofa backed to a side wall looking toward the TV: farthest, m
    pub side_wall_max: f64,
    /// TV and sofa placed together: sofa front this far from the TV, m
    pub float_min: f64,
    pub float_max: f64,
    /// living rooms of this size or more get one / two armchairs, m2
    pub armchair_one_m2: f64,
    pub armchair_two_m2: f64,
    /// coffee table width as a share of the sofa's
    pub coffee_ratio: f64,
    // dining
    /// chair spacing along a long side, m
    pub chair_spacing: f64,
    /// a chair at each end of tables of `small_table` m and longer
    pub end_chairs: bool,
    /// tables shorter than this get one chair per side, m
    pub small_table: f64,
    /// table clearance from walls / door points when centring, m
    pub table_wall_clear: f64,
    pub table_door_clear: f64,
    /// dining rooms smaller than this get a 1 m table, m2
    pub small_dining_m2: f64,
    /// a sideboard on a free wall; a tall cabinet too from `dining_cabinet_m2`; a rug under the table
    pub dining_sideboard: bool,
    pub dining_cabinet_m2: f64,
    pub dining_rug: bool,
    pub dining_plant: bool,
    // other rooms
    /// potted plants in corners: living room (from 20 m2; one below), bedroom (from 14 m2), hallway (from 6 m2)
    pub living_plants: usize,
    pub bedroom_plants: usize,
    pub hallway_plants: usize,
    /// a bookcase on a free living-room wall (from 20 m2, no built-in shelves)
    pub living_bookcase: bool,
    // kitchen
    /// the L template (fridge and range at the leg ends)
    pub kitchen_template: bool,
    /// counter runs added on the other free walls
    pub extra_runs: usize,
    /// aisle between facing runs, m
    pub kitchen_aisle: f64,
    /// counter between the sink and the range, m
    pub sink_range_gap: f64,
    // bathroom
    /// the wet-wall template (tub from a corner, sink and toilet beside it)
    pub bathroom_template: bool,
    /// gap between fixtures, m
    pub fixture_gap: f64,
    /// a tub and a shower together only from this size, m2
    pub tub_and_shower_m2: f64,
    /// a toilet nearer a door than this moves, m
    pub toilet_door_min: f64,
    // bedroom
    /// desks in bedrooms (from `desk_min_m2`)
    pub desks: bool,
    pub desk_min_m2: f64,
    /// a wardrobe nearer the bed than this moves, m
    pub wardrobe_bed_min: f64,
}

impl Default for LayoutRules {
    fn default() -> Self {
        LayoutRules {
            tv_view_min: 1.3, tv_view_max: 4.0, side_wall_max: 3.6, float_min: 2.2, float_max: 3.8,
            armchair_one_m2: 20.0, armchair_two_m2: 30.0, coffee_ratio: 0.75,
            chair_spacing: 0.6, end_chairs: true, small_table: 1.2, table_wall_clear: 0.75, table_door_clear: 1.0, small_dining_m2: 15.0,
            dining_sideboard: true, dining_cabinet_m2: 18.0, dining_rug: true, dining_plant: true,
            living_plants: 2, bedroom_plants: 1, hallway_plants: 0, living_bookcase: true,
            kitchen_template: true, extra_runs: 3, kitchen_aisle: 1.0, sink_range_gap: 0.4,
            bathroom_template: true, fixture_gap: 0.35, tub_and_shower_m2: 14.0, toilet_door_min: 1.0,
            desks: false, desk_min_m2: 18.0, wardrobe_bed_min: 0.7,
        }
    }
}

static RULES: RwLock<Option<LayoutRules>> = RwLock::new(None);

pub fn set(r: &LayoutRules) { *RULES.write().unwrap() = Some(r.clone()); }
pub fn get() -> LayoutRules { RULES.read().unwrap().clone().unwrap_or_default() }
