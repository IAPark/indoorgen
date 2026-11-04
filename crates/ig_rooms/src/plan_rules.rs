//! Tunable floor-plan rules: room size targets, windows per room type, second doors, angled corners. Defaults are the reviewed
//! values; native runs read LAYOUT_RULES_JSON ({"plan": {...}}), the browser build takes them per call (ig_wasm `layout()` "rules").

use std::sync::RwLock;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlanRules {
    /// area targets (the room solver leaves rooms up to ~1.5x alone), m2
    pub bedroom_m2: f64,
    pub living_m2: f64,
    pub kitchen_m2: f64,
    pub dining_m2: f64,
    pub bathroom_m2: f64,
    /// most windows per room
    pub closet_windows: usize,
    pub bathroom_windows: usize,
    pub bedroom_windows: usize,
    pub living_windows: usize,
    pub dining_windows: usize,
    pub kitchen_windows: usize,
    pub hallway_windows: usize,
    /// any other room (utility, staircase room, garage)
    pub other_windows: usize,
    /// chance of a second door between public rooms that share a wall
    pub extra_door_p: f64,
    /// walls out of square: each straight wall line of a house tilts by up to this many degrees (the house draws how much of it it uses); 0 keeps every wall on the grid
    pub wall_skew_deg: f64,
    /// chance a corner of the outline is cut or rounded
    pub angled_corner_p: f64,
    /// weight of the usable-rectangle term in the room solver (share of a main room in its largest rectangle, narrow rooms)
    pub shape_w: f64,
    /// chance a living / dining room gets a sliding glass patio door (its widest window, 1.4 m and up)
    pub patio_door_p: f64,
    /// chance a door between living / dining / hallway on a long shared wall is a double door
    pub double_door_p: f64,
    /// chance a closet door slides
    pub sliding_closet_p: f64,
    /// chance the living-dining door is a wide slider
    pub sliding_interior_p: f64,
    /// chance a kitchen / dining / living door on a long shared wall is a wide doorless opening (open plan)
    pub open_plan_p: f64,
    /// chance the kitchen-dining opening is a half wall (open above counter height), when both rooms keep another door
    pub half_wall_p: f64,
    /// merge hallways that serve fewer than two bedrooms / bathrooms into a neighbouring room
    pub merge_hallways: bool,
}

impl Default for PlanRules {
    fn default() -> Self {
        PlanRules { bedroom_m2: 14.0, living_m2: 22.0, kitchen_m2: 15.0, dining_m2: 13.0, bathroom_m2: 7.5, closet_windows: 0, bathroom_windows: 1, bedroom_windows: 1,
            living_windows: 2, dining_windows: 1, kitchen_windows: 1, hallway_windows: 0, other_windows: 1, extra_door_p: 0.7, wall_skew_deg: 2.5, angled_corner_p: 0.08, shape_w: 120.0,
            patio_door_p: 0.5, double_door_p: 0.4, sliding_closet_p: 0.5, sliding_interior_p: 0.3, open_plan_p: 0.3, half_wall_p: 0.85, merge_hallways: true }
    }
}

static RULES: RwLock<Option<PlanRules>> = RwLock::new(None);

pub fn set(r: &PlanRules) { *RULES.write().unwrap() = Some(r.clone()); }
pub fn get() -> PlanRules { RULES.read().unwrap().clone().unwrap_or_default() }
