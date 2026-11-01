//! CeilingFanFactory (not in infinigen): a ceiling fan with an optional light kit and pull cords. Local frame: the ceiling mount at the
//! origin, everything hangs below it (z <= 0).
//!   canopy (cup at the ceiling) -> downrod (none for a flush / hugger mount) -> motor housing (a free profile curve with optional
//!   trim rings, vents and a finial) -> blade arms (straight / curved / none) -> blades (a free outline: base shape x random width
//!   curve x tip treatment x sweep; optional two-tone top) -> switch housing -> light kit (none / bowl / globe / schoolhouse / tulips /
//!   disc / cage) -> bead-chain pull cords with fobs.
//! Randomness: a style family (industrial, tropical, farmhouse, mid-century, modern, traditional) sets coherent odds for every choice;
//! `variety` (0 conservative .. 1 wild) mutates choices away from the family and widens every range and curve; `imperfection`
//! (0 factory-new .. 1 well used) adds per-blade pitch / spacing / droop error, swaying and twisted cords, mismatched fobs, a dusty or
//! faded finish and a dead bulb. Every parameter can be fixed through `FanOverrides` (config `program.ceiling_fan.params`; null =
//! drawn). The draws are taken in a fixed order whatever is overridden, so fixing one parameter never changes the others.
//! `CeilingFan::build` also returns the bulb positions (for point lights) and the dimensions the placement needs. Placement in houses:
//! src/ceiling_decor.rs.
use super::*;
use crate::materials::*;
use ig_kit::geomkit::{extrude, limb};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Black / dark metal, three flat metal or wood blades, cage or exposed bulbs.
    Industrial,
    /// Leaf or oval blades in rattan / wood, bronze, five blades, amber or frosted glass.
    Tropical,
    /// Gray-wash or oak paddles, black or bronze body, schoolhouse or cage light.
    Farmhouse,
    /// Three walnut blades, brass or white body, globe light.
    MidCentury,
    /// Swept or tapered blades, white / nickel / black, flush mounts, LED disc.
    Modern,
    /// Five paddles on curved irons, brass or bronze, bowl or tulip lights.
    Traditional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BladeShape {
    /// Straight rectangular paddle.
    Paddle,
    /// Width changes linearly from root to tip.
    Tapered,
    /// Rounded oval, widest in the middle.
    Oval,
    /// Palm-leaf: widest past the middle, pointed tip.
    Leaf,
    /// Swept, curved blade (modern).
    Scimitar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tip {
    Round,
    Square,
    /// Cut at an angle (the leading edge longer).
    Angled,
    /// A V notch in the tip.
    Notched,
    Pointed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArmStyle {
    /// Flat straight blade irons.
    Straight,
    /// Irons that dip and rise to the blade.
    Curved,
    /// Blades screwed straight to the motor (no irons).
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightKit {
    /// No light: a cap or finial under the switch housing.
    None,
    /// Shallow glass dome.
    Bowl,
    /// Glass sphere.
    Globe,
    /// Schoolhouse (bulged cylinder) shade.
    Schoolhouse,
    /// Several tulip shades on curved arms.
    Tulips,
    /// Flat integrated LED disc.
    Disc,
    /// Exposed bulb in a wire cage.
    Cage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Glass {
    Frosted,
    Opal,
    Amber,
    Smoked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    BrushedNickel,
    MatteBlack,
    OilRubbedBronze,
    White,
    AntiqueBrass,
    Chrome,
    Walnut,
    Oak,
    Maple,
    GrayWash,
    Rattan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Fob {
    Ball,
    Cylinder,
    Acorn,
    Teardrop,
}

/// Fixed values for any ceiling-fan parameter; null (the default) draws it from the seed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FanOverrides {
    /// Style family: sets the odds of every other choice.
    pub style: Option<Style>,
    /// 0: every choice follows the style family, ranges narrow; 1: choices often leave the family, ranges and curves wide.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub variety: Option<f64>,
    /// 0: factory new; 1: well used (blade error, swaying / twisted cords, mismatched fobs, dust, a dead bulb).
    #[schemars(range(min = 0.0, max = 1.0))]
    pub imperfection: Option<f64>,
    /// Number of blades.
    #[schemars(range(min = 2, max = 8))]
    pub blades: Option<u32>,
    /// Blade tip radius from the fan axis (m); a 52" fan is 0.66.
    #[schemars(range(min = 0.3, max = 1.0))]
    pub span: Option<f64>,
    pub blade_shape: Option<BladeShape>,
    pub tip: Option<Tip>,
    /// Width multipliers of the blade at 6 points from root to tip (1 = the base shape).
    pub blade_profile: Option<Vec<f64>>,
    /// Sideways sweep of the blade's centre line at the tip, as a fraction of its length (negative: the other way).
    #[schemars(range(min = -0.4, max = 0.4))]
    pub blade_sweep: Option<f64>,
    /// Blade width at the root (m).
    #[schemars(range(min = 0.05, max = 0.3))]
    pub blade_root_width: Option<f64>,
    /// Blade width near the tip (m).
    #[schemars(range(min = 0.03, max = 0.3))]
    pub blade_tip_width: Option<f64>,
    /// Blade thickness (m).
    #[schemars(range(min = 0.003, max = 0.03))]
    pub blade_thickness: Option<f64>,
    /// Blade pitch about its long axis (degrees).
    #[schemars(range(min = 0.0, max = 30.0))]
    pub blade_pitch_deg: Option<f64>,
    /// Blade droop toward the tip (degrees; negative tilts up).
    #[schemars(range(min = -10.0, max = 15.0))]
    pub blade_droop_deg: Option<f64>,
    /// Static rotation of the blade set (degrees).
    #[schemars(range(min = 0.0, max = 360.0))]
    pub blade_angle_deg: Option<f64>,
    pub blade_finish: Option<Finish>,
    /// Finish of the blades' top face; null with two_tone off = same as blade_finish.
    pub blade_top_finish: Option<Finish>,
    /// Reversible two-tone blades (the top face in blade_top_finish).
    pub two_tone: Option<bool>,
    /// Finish of the canopy, downrod, motor, irons, switch housing, chains and fobs.
    pub body_finish: Option<Finish>,
    /// Motor housing radius (m).
    #[schemars(range(min = 0.06, max = 0.25))]
    pub housing_radius: Option<f64>,
    /// Motor housing height (m).
    #[schemars(range(min = 0.05, max = 0.3))]
    pub housing_height: Option<f64>,
    /// Housing outline: radius multipliers at 5 heights from top to bottom (1 = housing_radius).
    pub housing_profile: Option<Vec<f64>>,
    /// Decorative trim rings around the housing.
    #[schemars(range(min = 0, max = 5))]
    pub housing_rings: Option<u32>,
    /// Cooling vents (slots) around the housing.
    pub housing_vents: Option<bool>,
    /// Downrod length (m); 0 = flush (hugger) mount. The placement may shorten it to keep the floor clearance.
    #[schemars(range(min = 0.0, max = 1.5))]
    pub downrod_length: Option<f64>,
    /// Canopy radius (m).
    #[schemars(range(min = 0.04, max = 0.15))]
    pub canopy_radius: Option<f64>,
    pub arms: Option<ArmStyle>,
    pub light_kit: Option<LightKit>,
    /// Number of tulip shades (light kit "tulips").
    #[schemars(range(min = 2, max = 6))]
    pub shades: Option<u32>,
    pub glass: Option<Glass>,
    /// Power of each bulb (W).
    #[schemars(range(min = 5.0, max = 200.0))]
    pub bulb_watt: Option<f64>,
    /// Bulb colour temperature (K).
    #[schemars(range(min = 2000.0, max = 6500.0))]
    pub bulb_temperature: Option<f64>,
    /// Number of pull cords (fan speed, light).
    #[schemars(range(min = 0, max = 3))]
    pub pull_cords: Option<u32>,
    /// Pull-cord length below the switch housing (m).
    #[schemars(range(min = 0.03, max = 0.8))]
    pub cord_length: Option<f64>,
    /// Fob of every cord (with imperfection, cords may get mismatched fobs).
    pub fob: Option<Fob>,
}

/// Ceiling fans in houses (src/ceiling_decor.rs): which rooms, how often, clearances, and fixed parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FanConfig {
    /// Chance that an eligible room's central ceiling light becomes a fan.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub probability: f64,
    /// Room types that can get a fan (living-room, bedroom, dining-room, kitchen, hallway, ...).
    pub rooms: Vec<String>,
    /// Lowest blade height above the floor (m); 2.13 m = 7 ft. The downrod shortens, then the mount goes flush, else no fan.
    #[schemars(range(min = 1.8, max = 3.0))]
    pub min_blade_height: f64,
    /// Gap between the blade tips and the walls (m); the span shrinks to keep it.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub wall_clearance: f64,
    /// Fixed parameters (null: drawn per fan; style / variety / imperfection included).
    pub params: FanOverrides,
}

impl Default for FanConfig {
    fn default() -> Self {
        FanConfig { probability: 0.4, rooms: vec!["living-room".into(), "bedroom".into()], min_blade_height: 2.13, wall_clearance: 0.3, params: FanOverrides::default() }
    }
}

/// Every parameter resolved.
#[derive(Clone, Debug, Serialize)]
pub struct FanSpec {
    pub style: Style,
    pub variety: f64,
    pub imperfection: f64,
    pub blades: u32,
    pub span: f64,
    pub blade_shape: BladeShape,
    pub tip: Tip,
    pub blade_profile: Vec<f64>,
    pub blade_sweep: f64,
    pub blade_root_width: f64,
    pub blade_tip_width: f64,
    pub blade_thickness: f64,
    pub blade_pitch_deg: f64,
    pub blade_droop_deg: f64,
    pub blade_angle_deg: f64,
    pub blade_finish: Finish,
    pub blade_top_finish: Finish,
    pub two_tone: bool,
    pub body_finish: Finish,
    pub housing_radius: f64,
    pub housing_height: f64,
    pub housing_profile: Vec<f64>,
    pub housing_rings: u32,
    pub housing_vents: bool,
    pub downrod_length: f64,
    pub canopy_radius: f64,
    pub canopy_height: f64,
    pub arms: ArmStyle,
    pub light_kit: LightKit,
    pub shades: u32,
    pub glass: Glass,
    pub bulb_watt: f64,
    pub bulb_temperature: f64,
    pub pull_cords: u32,
    pub cord_length: f64,
    pub fob: Fob,
    /// per blade: (pitch error deg, spacing error rad, droop error deg)
    pub blade_errors: Vec<(f64, f64, f64)>,
    /// per cord: (sway angle rad, sway direction rad, twist, fob)
    pub cord_states: Vec<(f64, f64, f64, Fob)>,
    /// 0..1: dust / fading on the finishes
    pub wear: f64,
    /// index of a dead bulb (multi-bulb kits), if any
    pub dead_bulb: Option<usize>,
    pub seed: u64,
}

/// Bulb of the light kit (local frame); `on: false` = burnt out.
#[derive(Clone, Debug)]
pub struct Bulb {
    pub loc: V3,
    pub watt: f64,
    pub temperature: f64,
    pub on: bool,
}

/// What the placement needs to know.
#[derive(Clone, Debug)]
pub struct FanDims {
    /// blade tip radius (m)
    pub radius: f64,
    /// z of the blade plane (local, below the ceiling: negative)
    pub blade_z: f64,
    /// lowest z of the fan body (light kit), cords excluded
    pub bottom_z: f64,
    /// lowest z including the pull cords
    pub cord_bottom_z: f64,
}

#[derive(Clone)]
pub struct CeilingFan {
    pub spec: FanSpec,
}

fn pick<T: Copy>(r: &mut Rng, items: &[(T, f64)]) -> T {
    let w: Vec<f64> = items.iter().map(|x| x.1).collect();
    items[r.choice_p(&w).min(items.len() - 1)].0
}

/// The family's pick, or (with chance `v`) a pick from `all`: two draws always.
fn choose<T: Copy>(r: &mut Rng, fam: &[(T, f64)], all: &[(T, f64)], v: f64) -> T {
    let a = pick(r, fam);
    let b = pick(r, all);
    if r.f64() < v { b } else { a }
}

/// `base` jittered by up to +-rel (x (1 + 2 variety)): one draw.
fn jit(r: &mut Rng, base: f64, rel: f64, v: f64) -> f64 {
    base * (1.0 + rel * (1.0 + 2.0 * v) * (r.f64() * 2.0 - 1.0))
}

const ALL_SHAPES: [(BladeShape, f64); 5] = [(BladeShape::Paddle, 1.0), (BladeShape::Tapered, 1.0), (BladeShape::Oval, 1.0), (BladeShape::Leaf, 1.0), (BladeShape::Scimitar, 1.0)];
const ALL_TIPS: [(Tip, f64); 5] = [(Tip::Round, 1.0), (Tip::Square, 1.0), (Tip::Angled, 1.0), (Tip::Notched, 0.6), (Tip::Pointed, 0.8)];
const ALL_METALS: [(Finish, f64); 6] = [(Finish::BrushedNickel, 1.0), (Finish::MatteBlack, 1.0), (Finish::OilRubbedBronze, 1.0), (Finish::White, 1.0), (Finish::AntiqueBrass, 1.0), (Finish::Chrome, 0.6)];
const ALL_BLADE_FINISHES: [(Finish, f64); 9] = [(Finish::Walnut, 1.0), (Finish::Oak, 1.0), (Finish::Maple, 0.8), (Finish::GrayWash, 0.8), (Finish::Rattan, 0.6), (Finish::White, 1.0), (Finish::MatteBlack, 0.8), (Finish::BrushedNickel, 0.4), (Finish::OilRubbedBronze, 0.4)];
const ALL_KITS: [(LightKit, f64); 7] = [(LightKit::None, 1.0), (LightKit::Bowl, 1.0), (LightKit::Globe, 1.0), (LightKit::Schoolhouse, 1.0), (LightKit::Tulips, 1.0), (LightKit::Disc, 1.0), (LightKit::Cage, 1.0)];
const ALL_ARMS: [(ArmStyle, f64); 3] = [(ArmStyle::Straight, 1.0), (ArmStyle::Curved, 1.0), (ArmStyle::None, 0.5)];
const ALL_GLASS: [(Glass, f64); 4] = [(Glass::Frosted, 1.0), (Glass::Opal, 1.0), (Glass::Amber, 1.0), (Glass::Smoked, 1.0)];
const ALL_FOBS: [(Fob, f64); 4] = [(Fob::Ball, 1.0), (Fob::Cylinder, 1.0), (Fob::Acorn, 1.0), (Fob::Teardrop, 1.0)];

/// A style family's odds and typical values.
struct Family {
    blades: &'static [(u32, f64)],
    shapes: &'static [(BladeShape, f64)],
    tips: &'static [(Tip, f64)],
    body: &'static [(Finish, f64)],
    blade_fin: &'static [(Finish, f64)],
    kits: &'static [(LightKit, f64)],
    arms: &'static [(ArmStyle, f64)],
    glass: &'static [(Glass, f64)],
    fobs: &'static [(Fob, f64)],
    /// housing outline at 5 heights (top -> bottom), radius multipliers
    housing: [f64; 5],
    span: f64,
    root_w: f64,
    flush_p: f64,
    two_tone_p: f64,
}

fn family(s: Style) -> Family {
    use BladeShape::*;
    use Finish::*;
    use LightKit as K;
    use Tip::*;
    match s {
        Style::Industrial => Family {
            blades: &[(3, 0.7), (4, 0.2), (5, 0.1)], shapes: &[(Paddle, 0.5), (Tapered, 0.5)], tips: &[(Square, 0.6), (Angled, 0.4)],
            body: &[(MatteBlack, 0.6), (OilRubbedBronze, 0.25), (BrushedNickel, 0.15)], blade_fin: &[(MatteBlack, 0.35), (Walnut, 0.35), (OilRubbedBronze, 0.3)],
            kits: &[(K::Cage, 0.55), (K::None, 0.3), (K::Schoolhouse, 0.15)], arms: &[(ArmStyle::Straight, 0.8), (ArmStyle::None, 0.2)],
            glass: &[(Glass::Smoked, 0.5), (Glass::Amber, 0.5)], fobs: &[(Fob::Cylinder, 0.7), (Fob::Ball, 0.3)],
            housing: [0.85, 1.0, 1.0, 1.0, 0.85], span: 0.66, root_w: 0.12, flush_p: 0.1, two_tone_p: 0.0,
        },
        Style::Tropical => Family {
            blades: &[(5, 0.7), (4, 0.2), (6, 0.1)], shapes: &[(Leaf, 0.7), (Oval, 0.3)], tips: &[(Pointed, 0.6), (Round, 0.4)],
            body: &[(OilRubbedBronze, 0.5), (AntiqueBrass, 0.25), (White, 0.25)], blade_fin: &[(Rattan, 0.4), (Walnut, 0.3), (Oak, 0.3)],
            kits: &[(K::Bowl, 0.4), (K::Tulips, 0.3), (K::None, 0.3)], arms: &[(ArmStyle::Curved, 0.8), (ArmStyle::Straight, 0.2)],
            glass: &[(Glass::Amber, 0.5), (Glass::Frosted, 0.5)], fobs: &[(Fob::Acorn, 0.5), (Fob::Teardrop, 0.5)],
            housing: [0.7, 1.0, 1.05, 0.85, 0.5], span: 0.68, root_w: 0.12, flush_p: 0.1, two_tone_p: 0.1,
        },
        Style::Farmhouse => Family {
            blades: &[(5, 0.6), (4, 0.3), (3, 0.1)], shapes: &[(Paddle, 0.7), (Tapered, 0.3)], tips: &[(Square, 0.5), (Round, 0.5)],
            body: &[(MatteBlack, 0.5), (OilRubbedBronze, 0.3), (White, 0.2)], blade_fin: &[(GrayWash, 0.4), (Oak, 0.3), (Walnut, 0.3)],
            kits: &[(K::Schoolhouse, 0.45), (K::Cage, 0.3), (K::Bowl, 0.25)], arms: &[(ArmStyle::Straight, 0.7), (ArmStyle::Curved, 0.3)],
            glass: &[(Glass::Opal, 0.6), (Glass::Frosted, 0.4)], fobs: &[(Fob::Cylinder, 0.5), (Fob::Ball, 0.5)],
            housing: [0.9, 1.0, 1.0, 0.95, 0.8], span: 0.66, root_w: 0.13, flush_p: 0.15, two_tone_p: 0.35,
        },
        Style::MidCentury => Family {
            blades: &[(3, 0.7), (4, 0.3)], shapes: &[(Tapered, 0.5), (Oval, 0.5)], tips: &[(Round, 0.7), (Angled, 0.3)],
            body: &[(AntiqueBrass, 0.4), (White, 0.4), (MatteBlack, 0.2)], blade_fin: &[(Walnut, 0.8), (Oak, 0.2)],
            kits: &[(K::Globe, 0.6), (K::None, 0.4)], arms: &[(ArmStyle::Straight, 0.6), (ArmStyle::None, 0.4)],
            glass: &[(Glass::Opal, 0.8), (Glass::Frosted, 0.2)], fobs: &[(Fob::Ball, 0.7), (Fob::Teardrop, 0.3)],
            housing: [0.6, 0.9, 1.0, 0.9, 0.55], span: 0.62, root_w: 0.12, flush_p: 0.2, two_tone_p: 0.0,
        },
        Style::Modern => Family {
            blades: &[(3, 0.6), (4, 0.2), (5, 0.1), (8, 0.1)], shapes: &[(Scimitar, 0.5), (Tapered, 0.3), (Paddle, 0.2)], tips: &[(Angled, 0.4), (Pointed, 0.3), (Round, 0.3)],
            body: &[(White, 0.4), (BrushedNickel, 0.3), (MatteBlack, 0.3)], blade_fin: &[(White, 0.4), (MatteBlack, 0.3), (Maple, 0.3)],
            kits: &[(K::Disc, 0.6), (K::None, 0.4)], arms: &[(ArmStyle::None, 0.7), (ArmStyle::Straight, 0.3)],
            glass: &[(Glass::Opal, 0.8), (Glass::Frosted, 0.2)], fobs: &[(Fob::Cylinder, 0.8), (Fob::Ball, 0.2)],
            housing: [0.95, 1.0, 1.0, 1.0, 0.95], span: 0.66, root_w: 0.11, flush_p: 0.45, two_tone_p: 0.0,
        },
        Style::Traditional => Family {
            blades: &[(5, 0.8), (4, 0.2)], shapes: &[(Paddle, 0.6), (Oval, 0.4)], tips: &[(Round, 0.6), (Notched, 0.2), (Square, 0.2)],
            body: &[(AntiqueBrass, 0.4), (OilRubbedBronze, 0.4), (BrushedNickel, 0.2)], blade_fin: &[(Walnut, 0.4), (Oak, 0.3), (Maple, 0.3)],
            kits: &[(K::Bowl, 0.5), (K::Tulips, 0.4), (K::None, 0.1)], arms: &[(ArmStyle::Curved, 0.7), (ArmStyle::Straight, 0.3)],
            glass: &[(Glass::Frosted, 0.6), (Glass::Amber, 0.4)], fobs: &[(Fob::Acorn, 0.5), (Fob::Teardrop, 0.3), (Fob::Ball, 0.2)],
            housing: [0.65, 0.95, 1.05, 0.95, 0.6], span: 0.66, root_w: 0.13, flush_p: 0.1, two_tone_p: 0.4,
        },
    }
}

impl CeilingFan {
    pub fn sample(seed: u64, ov: &FanOverrides) -> CeilingFan {
        let mut r = Rng::seeded(int_hash(&[&seed.to_string(), "ceiling_fan"]));
        // family, variety, imperfection first: they shape everything after
        let style_d = pick(&mut r, &[(Style::Industrial, 0.15), (Style::Tropical, 0.12), (Style::Farmhouse, 0.18), (Style::MidCentury, 0.15), (Style::Modern, 0.2), (Style::Traditional, 0.2)]);
        let variety_d = r.uniform(0.1, 0.6);
        let imperf_d = r.f64().powi(2) * 0.8;
        let style = ov.style.unwrap_or(style_d);
        let v = ov.variety.unwrap_or(variety_d).clamp(0.0, 1.0);
        let imp = ov.imperfection.unwrap_or(imperf_d).clamp(0.0, 1.0);
        let f = family(style);
        let blades_d = choose(&mut r, f.blades, &[(3u32, 1.0), (4, 1.0), (5, 1.0), (6, 0.4), (8, 0.15)], v);
        let span_d = jit(&mut r, f.span, 0.1, v);
        let shape_d = choose(&mut r, f.shapes, &ALL_SHAPES, v);
        let tip_d = choose(&mut r, f.tips, &ALL_TIPS, v);
        // the width curve: 6 multipliers, smooth noise around 1 (wider with variety)
        // (correlated: neighbours move together; independent wide steps crumpled the outline into waves)
        let amp = 0.05 + 0.15 * v;
        let mut prof_d = vec![];
        let (mut acc, mut slope) = (0.0, 0.0);
        for _ in 0..6 {
            slope = slope * 0.6 + (r.f64() * 2.0 - 1.0) * amp;
            acc = (acc + slope).clamp(-0.35, 0.45);
            prof_d.push(1.0 + acc);
        }
        let sweep_d = if shape_d == BladeShape::Scimitar { -r.uniform(0.08, 0.2) } else { (r.f64() * 2.0 - 1.0) * 0.12 * v };
        let root_w_d = jit(&mut r, f.root_w, 0.15, v);
        let tip_w_d = root_w_d * jit(&mut r, 1.0, 0.2, v);
        let thick_d = r.uniform(0.006, 0.012);
        let pitch_d = jit(&mut r, 12.0, 0.2, v);
        let droop_d = r.uniform(0.0, 2.5);
        let angle_d = r.uniform(0.0, 360.0);
        let body_d = choose(&mut r, f.body, &ALL_METALS, v);
        let blade_fin_d = choose(&mut r, f.blade_fin, &ALL_BLADE_FINISHES, v);
        let two_tone_d = r.f64() < f.two_tone_p + 0.3 * v;
        let top_fin_d = pick(&mut r, &ALL_BLADE_FINISHES);
        let hr_d = jit(&mut r, 0.12, 0.2, v);
        let flush_d = r.f64() < f.flush_p;
        let hh_d = if flush_d { r.uniform(0.07, 0.1) } else { r.uniform(0.11, 0.2) };
        let mut hprof_d = vec![];
        for k in 0..5 {
            hprof_d.push((f.housing[k] * jit(&mut r, 1.0, 0.08, v)).clamp(0.3, 1.4));
        }
        let rings_d = pick(&mut r, &[(0u32, 0.5), (1, 0.2), (2, 0.2), (3, 0.1)]);
        let vents_d = r.f64() < 0.15 + 0.3 * v;
        let downrod_d = if flush_d { 0.0 } else { r.uniform(0.08, 0.3) };
        let canopy_d = r.uniform(0.06, 0.09);
        let arms_d = if shape_d == BladeShape::Scimitar { r.f64(); r.f64(); r.f64(); ArmStyle::None } else { choose(&mut r, f.arms, &ALL_ARMS, v) };
        let kit_d = choose(&mut r, f.kits, &ALL_KITS, v);
        let shades_d = r.randint(3, 5) as u32;
        let glass_d = choose(&mut r, f.glass, &ALL_GLASS, v);
        let watt_d = r.uniform(40.0, 75.0);
        let temp_d = r.uniform(2700.0, 3500.0);
        let cords_d = pick(&mut r, &[(0u32, 0.2), (1, 0.35), (2, 0.45)]);
        let cord_len_d = r.uniform(0.15, 0.32);
        let fob_d = choose(&mut r, f.fobs, &ALL_FOBS, v);
        // imperfections (always drawn; scaled by imp)
        let mut errs = vec![];
        for _ in 0..8 {
            let (a, b, c) = (r.f64() * 2.0 - 1.0, r.f64() * 2.0 - 1.0, r.f64() * 2.0 - 1.0);
            errs.push((a * 3.0 * imp, b * 0.06 * imp, c * 2.5 * imp));
        }
        let mut cstates = vec![];
        for _ in 0..3 {
            let sway = r.f64() * 0.35 * imp;
            let dir = r.uniform(0.0, TAU);
            let twist = r.f64() * imp;
            let alt = pick(&mut r, &ALL_FOBS);
            let mismatch = r.f64() < 0.5 * imp;
            cstates.push((sway, dir, twist, if mismatch { Some(alt) } else { None }));
        }
        let wear = imp * r.uniform(0.4, 1.0);
        let dead_draw = r.f64();
        let o = ov;
        let fob = o.fob.unwrap_or(fob_d);
        let shape = o.blade_shape.unwrap_or(shape_d);
        let mut blade_profile = o.blade_profile.clone().unwrap_or(prof_d);
        blade_profile.resize(6, 1.0);
        let mut housing_profile = o.housing_profile.clone().unwrap_or(hprof_d);
        housing_profile.resize(5, 1.0);
        let light_kit = o.light_kit.unwrap_or(kit_d);
        let shades = o.shades.unwrap_or(shades_d).clamp(2, 6);
        let n_bulbs = if light_kit == LightKit::Tulips { shades as usize } else { 1 };
        let dead_bulb = if n_bulbs > 1 && dead_draw < 0.4 * imp { Some((dead_draw * 1000.0) as usize % n_bulbs) } else { None };
        let spec = FanSpec {
            style, variety: v, imperfection: imp,
            blades: o.blades.unwrap_or(blades_d).clamp(2, 8),
            span: o.span.unwrap_or(span_d),
            blade_shape: shape,
            tip: o.tip.unwrap_or(tip_d),
            blade_profile,
            blade_sweep: o.blade_sweep.unwrap_or(sweep_d),
            blade_root_width: o.blade_root_width.unwrap_or(root_w_d),
            blade_tip_width: o.blade_tip_width.unwrap_or(tip_w_d),
            blade_thickness: o.blade_thickness.unwrap_or(thick_d),
            blade_pitch_deg: o.blade_pitch_deg.unwrap_or(pitch_d),
            blade_droop_deg: o.blade_droop_deg.unwrap_or(droop_d),
            blade_angle_deg: o.blade_angle_deg.unwrap_or(angle_d),
            blade_finish: o.blade_finish.unwrap_or(blade_fin_d),
            blade_top_finish: o.blade_top_finish.unwrap_or(top_fin_d),
            two_tone: o.two_tone.unwrap_or(two_tone_d),
            body_finish: o.body_finish.unwrap_or(body_d),
            housing_radius: o.housing_radius.unwrap_or(hr_d),
            housing_height: o.housing_height.unwrap_or(hh_d),
            housing_profile,
            housing_rings: o.housing_rings.unwrap_or(rings_d).min(5),
            housing_vents: o.housing_vents.unwrap_or(vents_d),
            downrod_length: o.downrod_length.unwrap_or(downrod_d).max(0.0),
            canopy_radius: o.canopy_radius.unwrap_or(canopy_d),
            canopy_height: 0.07,
            arms: o.arms.unwrap_or(arms_d),
            light_kit, shades,
            glass: o.glass.unwrap_or(glass_d),
            bulb_watt: o.bulb_watt.unwrap_or(watt_d),
            bulb_temperature: o.bulb_temperature.unwrap_or(temp_d),
            pull_cords: o.pull_cords.unwrap_or(cords_d).min(3),
            cord_length: o.cord_length.unwrap_or(cord_len_d),
            fob,
            blade_errors: errs,
            cord_states: cstates.into_iter().map(|(a, b, c, m)| (a, b, c, if o.fob.is_some() { fob } else { m.unwrap_or(fob) })).collect(),
            wear, dead_bulb, seed,
        };
        CeilingFan { spec }
    }

    pub fn clone_with_downrod(&self, downrod: f64) -> CeilingFan {
        let mut c = self.clone();
        c.spec.downrod_length = downrod.max(0.0);
        c
    }

    pub fn clone_with_span(&self, span: f64) -> CeilingFan {
        let mut c = self.clone();
        c.spec.span = span;
        c
    }

    /// Build the fan: (asset, bulbs, dims), local frame (ceiling mount at the origin).
    pub fn build(&self) -> (Asset, Vec<Bulb>, FanDims) {
        let s = &self.spec;
        let mut a = Asset::default();
        let body = finish_material("fan_body", s.body_finish, s.seed, s.wear);
        let blade_m = finish_material("fan_blade", s.blade_finish, s.seed + 1, s.wear);
        let top_m = finish_material("fan_blade_top", s.blade_top_finish, s.seed + 3, s.wear);
        let glass = glass_material(s.glass, s.seed + 2);
        let dark = Material::diffuse("fan_vent", [0.02, 0.02, 0.02], 0.8);
        let push = |a: &mut Asset, m: Mesh, mat: &Material| {
            if m.is_empty() { return }
            let b = m.bounds();
            a.mesh(m, mat, Some(b));
        };
        // ---- canopy, downrod
        let (cr, ch) = (s.canopy_radius, s.canopy_height);
        let flush = s.downrod_length <= 1e-6;
        let mut metal = Mesh::new();
        let ch_eff = if flush { ch * 0.6 } else { ch };
        metal.append(&Mesh::frustum(0.0, 0.0, -ch_eff, 0.0, cr * 0.55, cr, 32));
        let mut z = -ch_eff;
        if !flush {
            metal.append(&Mesh::cylinder(0.0, 0.0, z - s.downrod_length, z + 0.005, 0.011, 16));
            z -= s.downrod_length;
            metal.append(&Mesh::cylinder(0.0, 0.0, z - 0.02, z + 0.01, 0.022, 16));
            z -= 0.01;
        }
        // ---- motor housing: a smooth outline through the 5 profile radii (top -> bottom), closed on the axis
        let (hr, hh) = (s.housing_radius * if flush { 1.15 } else { 1.0 }, s.housing_height);
        let (zt, zb) = (z, z - hh);
        let hp = &s.housing_profile;
        let mut prof = vec![(0.0, zt)];
        let steps = 16;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = t * 4.0;
            let k = (x.floor() as usize).min(3);
            let u = x - k as f64;
            // Catmull-Rom through the profile points
            let p = |j: isize| hp[(j.clamp(0, 4)) as usize];
            let (p0, p1, p2, p3) = (p(k as isize - 1), p(k as isize), p(k as isize + 1), p(k as isize + 2));
            let rr = 0.5 * (2.0 * p1 + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u * u * u);
            prof.push(((hr * rr).max(0.02), zt - hh * t));
        }
        prof.push((0.0, zb));
        metal.append(&Mesh::revolve(0.0, 0.0, &prof, 48));
        let r_at = |t: f64| -> f64 { let i = ((t * steps as f64).round() as usize).min(steps); prof[i + 1].0 };
        for j in 0..s.housing_rings {
            let t = (j as f64 + 1.0) / (s.housing_rings as f64 + 1.0);
            let zz = zt - hh * t;
            metal.append(&Mesh::cylinder(0.0, 0.0, zz - 0.004, zz + 0.004, r_at(t) + 0.005, 48));
        }
        let mut vents = Mesh::new();
        if s.housing_vents {
            let (t0, t1) = (0.15, 0.35);
            let rv = r_at(0.25) + 0.001;
            for i in 0..16 {
                let a = TAU * i as f64 / 16.0;
                let mut m = Mesh::cuboid([-0.002, -0.006, zt - hh * t1], [0.003, 0.006, zt - hh * t0]);
                m.transform(&Mat4::translation([rv, 0.0, 0.0]));
                m.transform(&Mat4::rot_z(a));
                vents.append(&m);
            }
        }
        // ---- blades and irons: attached in the lower third of the housing
        let blade_z = zb + hh * 0.3;
        let n = s.blades.max(2) as usize;
        let hr_blade = r_at(0.7);
        let root = hr_blade + match s.arms { ArmStyle::None => 0.01, _ => 0.07 };
        let mut irons = Mesh::new();
        let mut blades = Mesh::new();
        let mut tops = Mesh::new();
        for i in 0..n {
            let (pe, se, de) = s.blade_errors[i % s.blade_errors.len()];
            let ang = s.blade_angle_deg.to_radians() + TAU * i as f64 / n as f64 + se;
            let rot = Mat4::rot_z(ang);
            match s.arms {
                ArmStyle::Straight => {
                    let mut m = limb([hr_blade * 0.85, 0.0, blade_z - 0.008], [root + 0.06, 0.0, blade_z - 0.008], 0.009, 0.007, false);
                    m.transform(&rot);
                    irons.append(&m);
                }
                ArmStyle::Curved => {
                    let p0 = [hr_blade * 0.85, 0.0, blade_z - 0.005];
                    let p1 = [(hr_blade + root) / 2.0 + 0.02, 0.0, blade_z - 0.03];
                    let p2 = [root + 0.06, 0.0, blade_z - 0.008];
                    let mut m = limb(p0, p1, 0.009, 0.008, false);
                    m.append(&limb(p1, p2, 0.008, 0.007, false));
                    m.transform(&rot);
                    irons.append(&m);
                }
                ArmStyle::None => {}
            }
            let place = |m: &mut Mesh| {
                m.transform(&Mat4::rot_x((s.blade_pitch_deg + pe).to_radians()));
                m.transform(&Mat4::translation([-root, 0.0, 0.0]));
                m.transform(&Mat4::rot_y((s.blade_droop_deg + de).to_radians()));
                m.transform(&Mat4::translation([root, 0.0, blade_z]));
                m.transform(&rot);
            };
            let (mut b, top) = blade_mesh(s, root, s.span);
            place(&mut b);
            blades.append(&b);
            if let Some(mut t) = top { place(&mut t); tops.append(&t); }
        }
        // ---- switch housing and light kit
        let (sr, sh) = (r_at(1.0).max(hr * 0.4) * 0.9, 0.05);
        let zs = zb + 0.004;
        metal.append(&Mesh::revolve(0.0, 0.0, &[(0.0, zs), (sr, zs), (sr, zs - sh * 0.8), (sr * 0.85, zs - sh), (0.0, zs - sh)], 32));
        let zk = zs - sh;
        let mut glass_m = Mesh::new();
        let mut bulb_glow = Mesh::new();
        let mut bulbs = vec![];
        let (w, t) = (s.bulb_watt, s.bulb_temperature);
        let mut bottom = zk;
        match s.light_kit {
            LightKit::None => {
                // a cap, or a finial on traditional / tropical fans
                if matches!(s.style, Style::Traditional | Style::Tropical) {
                    metal.append(&Mesh::revolve(0.0, 0.0, &[(0.0, zk + 0.001), (sr * 0.5, zk + 0.001), (sr * 0.25, zk - 0.02), (sr * 0.35, zk - 0.035), (sr * 0.1, zk - 0.06), (0.0, zk - 0.07)], 24));
                    bottom = zk - 0.07;
                } else {
                    metal.append(&Mesh::revolve(0.0, 0.0, &[(0.0, zk + 0.001), (sr * 0.6, zk + 0.001), (sr * 0.4, zk - 0.02), (0.0, zk - 0.028)], 24));
                    bottom = zk - 0.028;
                }
            }
            LightKit::Bowl => {
                let (gr, gd) = (sr * 1.7, sr * 0.9);
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.008, zk + 0.001, gr * 0.98, 32));
                glass_m.append(&Mesh::revolve(0.0, 0.0, &[(gr * 0.96, zk - 0.008), (gr, zk - gd * 0.3), (gr * 0.78, zk - gd * 0.75), (gr * 0.4, zk - gd * 0.97), (0.0, zk - gd)], 32));
                bulbs.push(Bulb { loc: [0.0, 0.0, zk - gd * 0.4], watt: w, temperature: t, on: true });
                bottom = zk - gd;
            }
            LightKit::Globe => {
                let gr = sr * 1.25;
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.02, zk + 0.001, sr * 0.55, 24));
                let c = [0.0, 0.0, zk - 0.015 - gr * 0.95];
                glass_m.append(&Mesh::ellipsoid(c, [gr, gr, gr], 32, 16));
                bulbs.push(Bulb { loc: c, watt: w, temperature: t, on: true });
                bottom = c[2] - gr;
            }
            LightKit::Schoolhouse => {
                let (gr, gh) = (sr * 1.15, sr * 2.0);
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.015, zk + 0.001, gr * 0.6, 24));
                let z0 = zk - 0.012;
                glass_m.append(&Mesh::revolve(0.0, 0.0, &[(gr * 0.58, z0), (gr * 0.8, z0 - gh * 0.1), (gr, z0 - gh * 0.45), (gr * 0.95, z0 - gh * 0.8), (gr * 0.7, z0 - gh * 0.97), (0.0, z0 - gh)], 32));
                bulbs.push(Bulb { loc: [0.0, 0.0, z0 - gh * 0.45], watt: w, temperature: t, on: true });
                bottom = z0 - gh;
            }
            LightKit::Disc => {
                let gr = (hr * 1.05).max(0.11);
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.012, zk + 0.001, gr, 48));
                glass_m.append(&Mesh::cylinder(0.0, 0.0, zk - 0.024, zk - 0.011, gr * 0.93, 48));
                bulbs.push(Bulb { loc: [0.0, 0.0, zk - 0.04], watt: w * 0.8, temperature: t, on: true });
                bottom = zk - 0.024;
            }
            LightKit::Cage => {
                // socket, an exposed bulb, and a wire cage of vertical ribs and rings around it
                let (gr, gh) = (sr * 1.1, sr * 2.2);
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.035, zk + 0.001, 0.018, 16));
                let c = [0.0, 0.0, zk - 0.035 - gh * 0.38];
                bulb_glow.append(&Mesh::ellipsoid(c, [gr * 0.42, gr * 0.42, gr * 0.6], 20, 12));
                let ribs = 6;
                for i in 0..ribs {
                    let a = TAU * i as f64 / ribs as f64;
                    let pt = |rr: f64, zz: f64| [rr * a.cos(), rr * a.sin(), zz];
                    let (p0, p1, p2, p3) = (pt(0.02, zk - 0.03), pt(gr, zk - 0.03 - gh * 0.3), pt(gr, zk - 0.03 - gh * 0.7), pt(0.015, zk - 0.03 - gh));
                    metal.append(&limb(p0, p1, 0.0015, 0.0015, true));
                    metal.append(&limb(p1, p2, 0.0015, 0.0015, true));
                    metal.append(&limb(p2, p3, 0.0015, 0.0015, true));
                }
                // wire hoops (thin tubes around the circle; a cylinder here rendered as a floating plate)
                for f in [0.3, 0.7] {
                    let zz = zk - 0.03 - gh * f;
                    let m = 24;
                    for j in 0..m {
                        let (a0, a1) = (TAU * j as f64 / m as f64, TAU * (j + 1) as f64 / m as f64);
                        metal.append(&limb([gr * a0.cos(), gr * a0.sin(), zz], [gr * a1.cos(), gr * a1.sin(), zz], 0.0015, 0.0015, true));
                    }
                }
                bulbs.push(Bulb { loc: c, watt: w * 0.7, temperature: (t - 400.0).max(2200.0), on: true });
                bottom = zk - 0.03 - gh;
            }
            LightKit::Tulips => {
                let k = s.shades as usize;
                let (ra, drop) = (sr + 0.1, 0.07);
                metal.append(&Mesh::cylinder(0.0, 0.0, zk - 0.03, zk + 0.001, sr * 0.6, 24));
                for i in 0..k {
                    let ang = TAU * i as f64 / k as f64 + PI / k as f64;
                    let (c, si) = (ang.cos(), ang.sin());
                    let p0 = [sr * 0.5 * c, sr * 0.5 * si, zk - 0.02];
                    let p1 = [ra * 0.7 * c, ra * 0.7 * si, zk - 0.035];
                    let p2 = [ra * c, ra * si, zk - drop];
                    metal.append(&limb(p0, p1, 0.006, 0.006, true));
                    metal.append(&limb(p1, p2, 0.006, 0.006, true));
                    let (tr, th) = (0.055, 0.11);
                    let mut cup = Mesh::revolve(0.0, 0.0, &[(0.012, 0.0), (tr * 0.6, -th * 0.2), (tr, -th * 0.7), (tr * 0.92, -th)], 24);
                    cup.transform(&Mat4::translation(p2));
                    glass_m.append(&cup);
                    bulbs.push(Bulb { loc: [p2[0], p2[1], p2[2] - th * 0.5], watt: w * 0.55, temperature: t, on: true });
                    bottom = bottom.min(p2[2] - th);
                }
            }
        }
        if let Some(d) = s.dead_bulb { if let Some(b) = bulbs.get_mut(d) { b.on = false; } }
        // ---- pull cords: bead chains from the switch housing (swaying / twisted with imperfection), a fob at the end
        let mut cords = Mesh::new();
        let mut cord_bottom = bottom;
        for i in 0..s.pull_cords as usize {
            let (sway, dir, twist, fob) = s.cord_states[i % s.cord_states.len()];
            let ang = 0.6 + i as f64 * 1.4;
            let (x, y) = (sr * 0.85 * ang.cos(), sr * 0.85 * ang.sin());
            let z0 = zs - sh * 0.5;
            let len = s.cord_length * (1.0 - 0.18 * i as f64);
            let step = 0.0048;
            let beads = (len / step) as usize;
            // the chain hangs at `sway` from vertical toward `dir`, with a slight bow and a twist wobble
            let pos = |b: usize| -> V3 {
                let t = b as f64 * step;
                let off = t * sway.sin() + 0.004 * twist * (t * 40.0).sin();
                let bow = 0.08 * sway * (PI * t / len.max(1e-3)).sin() * len;
                [x + (off + bow) * dir.cos(), y + (off + bow) * dir.sin(), z0 - t * sway.cos()]
            };
            for b in 0..beads {
                cords.append(&Mesh::ellipsoid(pos(b), [0.0019, 0.0019, 0.0019], 6, 4));
            }
            let e = pos(beads);
            let (fx, fy, zf) = (e[0], e[1], e[2] - 0.002);
            match fob {
                Fob::Ball => cords.append(&Mesh::ellipsoid([fx, fy, zf - 0.009], [0.009, 0.009, 0.009], 16, 8)),
                Fob::Cylinder => cords.append(&Mesh::cylinder(fx, fy, zf - 0.03, zf, 0.006, 16)),
                Fob::Acorn => {
                    cords.append(&Mesh::ellipsoid([fx, fy, zf - 0.016], [0.008, 0.008, 0.013], 16, 8));
                    cords.append(&Mesh::cylinder(fx, fy, zf - 0.007, zf, 0.0085, 16));
                }
                Fob::Teardrop => cords.append(&Mesh::revolve(fx, fy, &[(0.0, zf), (0.004, zf - 0.006), (0.009, zf - 0.02), (0.006, zf - 0.028), (0.0, zf - 0.03)], 16)),
            }
            cord_bottom = cord_bottom.min(zf - 0.03);
        }
        let bulb_m = {
            let mut m = Material::diffuse("fan_bulb", [1.0, 0.9, 0.75], 0.3);
            m.emissive = [1.0, 0.8, 0.55];
            m.emissive_strength = 4.0;
            m
        };
        push(&mut a, metal, &body);
        push(&mut a, irons, &body);
        push(&mut a, vents, &dark);
        push(&mut a, blades, &blade_m);
        push(&mut a, tops, &top_m);
        push(&mut a, glass_m, &glass);
        push(&mut a, bulb_glow, &bulb_m);
        push(&mut a, cords, &body);
        let dims = FanDims { radius: s.span, blade_z, bottom_z: bottom, cord_bottom_z: cord_bottom };
        (a, bulbs, dims)
    }
}

/// The blade (u: from `root` out to `span` along +x, v: across, thickness in z) and, for two-tone blades, its top veneer.
/// Outline: the base shape's half width x the smooth width curve (`blade_profile`) x the tip treatment, along a centre line swept by
/// `blade_sweep`.
fn blade_mesh(s: &FanSpec, root: f64, span: f64) -> (Mesh, Option<Mesh>) {
    let len = (span - root).max(0.1);
    let (w0, w1) = (s.blade_root_width, s.blade_tip_width);
    let k = 32;
    let base = |t: f64| -> f64 {
        match s.blade_shape {
            BladeShape::Paddle => (w0 + (w1 - w0) * t) / 2.0,
            BladeShape::Tapered => (w0 + (w1 * 0.75 - w0) * t) / 2.0,
            BladeShape::Oval => ((w0 * 0.35).max(w0.max(w1) * 1.15 / 2.0 * (PI * (0.08 + 0.92 * t)).sin().max(0.0).powf(0.6))),
            BladeShape::Leaf => (w0 * 0.3).max(w0.max(w1) * 1.2 / 2.0 * (PI * t.powf(0.75)).sin().max(0.0)),
            BladeShape::Scimitar => w0 * (1.0 - 0.55 * t) / 2.0,
        }
    };
    // the width curve through 6 points (linear between them; smooth enough at 32 steps after the base shape)
    let prof = |t: f64| -> f64 {
        let x = t * 5.0;
        let i = (x.floor() as usize).min(4);
        let u = x - i as f64;
        let u = u * u * (3.0 - 2.0 * u);
        s.blade_profile[i] * (1.0 - u) + s.blade_profile[i + 1] * u
    };
    // tip treatment over the last fraction of the length
    let tip_len = (w1.max(w0) * 0.55 / len).min(0.35);
    let tip = |t: f64, side: f64| -> f64 {
        if t <= 1.0 - tip_len { return 1.0 }
        let q = (t - (1.0 - tip_len)) / tip_len; // 0 .. 1 over the tip
        match s.tip {
            Tip::Round => (1.0 - q * q).max(0.0).sqrt(),
            Tip::Square => if q > 0.97 { 0.0 } else { 1.0 },
            Tip::Pointed => (1.0 - q).max(0.0).powf(0.7),
            // the leading edge (side +1) runs out to the tip, the trailing one is cut back
            Tip::Angled => if side > 0.0 { if q > 0.97 { 0.0 } else { 1.0 } } else { (1.0 - q).max(0.0) },
            Tip::Notched => if q > 0.97 { 0.0 } else { 1.0 },
        }
    };
    let sweep = |t: f64| s.blade_sweep * len * t * t;
    let mut lead = vec![];
    let mut trail = vec![];
    for i in 0..=k {
        let t = i as f64 / k as f64;
        let hw = base(t) * prof(t);
        let u = root + len * t;
        let c = sweep(t);
        lead.push([u, c + (hw * tip(t, 1.0)).max(0.0015)]);
        trail.push([u, c - (hw * tip(t, -1.0)).max(0.0015)]);
    }
    let mut poly: Vec<[f64; 2]> = lead;
    if s.tip == Tip::Notched {
        // a V cut into the square tip
        let hw = base(1.0) * prof(1.0);
        poly.push([root + len - hw * 0.8, sweep(1.0)]);
    }
    poly.extend(trail.into_iter().rev());
    let th = s.blade_thickness;
    let blade = extrude(&poly, -th / 2.0, th / 2.0);
    let top = if s.two_tone && s.blade_top_finish != s.blade_finish { Some(extrude(&poly, th / 2.0 - 0.0002, th / 2.0 + 0.0008)) } else { None };
    (blade, top)
}

fn metal(name: &str, rgb: [f32; 3], rough: f32, metallic: f32) -> Material {
    let mut m = Material::diffuse(name, rgb, rough);
    m.metallic = metallic;
    m
}

/// Material of a finish (plain PBR values: they render the same in the glb preview and in Blender); `wear` (0..1) greys and roughens
/// it (dust, fading).
pub fn finish_material(prefix: &str, f: Finish, seed: u64, wear: f64) -> Material {
    let name = |s: &str| format!("{prefix}_{s}{}", if wear > 0.05 { format!("_w{}", (wear * 10.0).round() as u32) } else { String::new() });
    let mut m = match f {
        Finish::BrushedNickel => metal(&name("brushed_nickel"), [0.62, 0.62, 0.6], 0.32, 1.0),
        Finish::MatteBlack => metal(&name("matte_black"), [0.03, 0.03, 0.03], 0.55, 0.3),
        Finish::OilRubbedBronze => metal(&name("oil_rubbed_bronze"), [0.14, 0.09, 0.06], 0.4, 0.8),
        Finish::White => metal(&name("white"), [0.88, 0.88, 0.86], 0.45, 0.0),
        Finish::AntiqueBrass => metal(&name("antique_brass"), [0.55, 0.42, 0.2], 0.3, 1.0),
        Finish::Chrome => metal(&name("chrome"), [0.85, 0.85, 0.86], 0.08, 1.0),
        Finish::Walnut => metal(&name("walnut"), [0.25, 0.15, 0.08], 0.5, 0.0),
        Finish::Oak => metal(&name("oak"), [0.55, 0.4, 0.24], 0.55, 0.0),
        Finish::Maple => metal(&name("maple"), [0.72, 0.58, 0.4], 0.5, 0.0),
        Finish::GrayWash => metal(&name("gray_wash"), [0.5, 0.48, 0.45], 0.6, 0.0),
        Finish::Rattan => spec_material(&name("rattan"), json!({"cls": "fabric.CoarseKnitFabric", "kwargs": {}, "seed": seed % 2_000_000_000}), [0.62, 0.48, 0.3]),
    };
    if wear > 0.05 {
        // dust: toward a light grey, rougher, less metallic
        let w = (wear * 0.35) as f32;
        for k in 0..3 { m.color[k] = m.color[k] * (1.0 - w) + 0.55 * w; }
        m.roughness = (m.roughness + 0.4 * wear as f32).min(1.0);
        m.metallic *= 1.0 - 0.5 * wear as f32;
    }
    m
}

fn glass_material(g: Glass, seed: u64) -> Material {
    match g {
        // the ceiling lights' translucent cover: lets the bulb's light through in Cycles
        Glass::Frosted => spec_material("fan_glass_frosted", json!({"cls": "lamp_shaders.shader_lamp_bulb_nonemissive", "kwargs": {}, "seed": seed % 2_000_000_000}), [0.85, 0.83, 0.8]),
        Glass::Opal => spec_material("fan_glass_opal", json!({"cls": "lamp_shaders.shader_lamp_bulb_nonemissive", "kwargs": {}, "seed": seed % 2_000_000_000}), [0.95, 0.94, 0.92]),
        Glass::Amber => metal("fan_glass_amber", [0.75, 0.45, 0.15], 0.15, 0.0),
        Glass::Smoked => metal("fan_glass_smoked", [0.2, 0.2, 0.22], 0.1, 0.0),
    }
}

impl CeilingFan {
    /// collision from the spec, as `build` lays the fan out down from the ceiling: the canopy, downrod, motor housing (its outline's
    /// spline bounded by 1.25x its largest profile radius, trim rings 5 mm proud) and switch housing as one column; the blade disc (tip
    /// radius with the sweep and half the widest blade sideways; pitched and drooped about the root); the light kit; each pull cord's
    /// sway cone and fob
    pub fn volumes_from_spec(&self) -> Vec<crate::piece::Volume> {
        use crate::piece::Volume;
        let s = &self.spec;
        let flush = s.downrod_length <= 1e-6;
        let ch_eff = if flush { s.canopy_height * 0.6 } else { s.canopy_height };
        let zt = -ch_eff - if flush { 0.0 } else { s.downrod_length + 0.01 };
        let (hr, hh) = (s.housing_radius * if flush { 1.15 } else { 1.0 }, s.housing_height);
        let zb = zt - hh;
        // the housing outline's radius at t (build's Catmull-Rom through the profile, sampled at 16 steps)
        let hp = &s.housing_profile;
        let r_at = |t: f64| -> f64 {
            let x = (t * 16.0).round() / 16.0 * 4.0;
            let k = (x.floor() as usize).min(3);
            let u = x - k as f64;
            let p = |j: isize| hp[(j.clamp(0, 4)) as usize];
            let (p0, p1, p2, p3) = (p(k as isize - 1), p(k as isize), p(k as isize + 1), p(k as isize + 2));
            let rr = 0.5 * (2.0 * p1 + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u * u * u);
            (hr * rr).max(0.02)
        };
        let hmax = (hr * hp.iter().cloned().fold(0.0, f64::max) * 1.25).max(0.02);
        let (sr, sh) = (r_at(1.0).max(hr * 0.4) * 0.9, 0.05);
        let zs = zb + 0.004;
        let zk = zs - sh;
        let rb = s.canopy_radius.max(hmax + 0.006).max(sr).max(0.022) + 0.003;
        let m = 0.003;
        let mut v = vec![Volume::hard([-rb, -rb, zk - m], [rb, rb, m])];
        // blades and irons
        let blade_z = zb + hh * 0.3;
        let root = r_at(0.7) + if matches!(s.arms, ArmStyle::None) { 0.01 } else { 0.07 };
        let len = (s.span - root).max(0.1);
        let w = s.blade_root_width.max(s.blade_tip_width);
        let hw = 0.6 * w * s.blade_profile.iter().cloned().fold(0.0, f64::max);
        let side = s.blade_sweep.abs() * len + hw;
        let rbl = (root + len).hypot(side) + 0.01;
        let (mut down, mut up) = (0.0f64, 0.0f64);
        for &(pe, _, de) in &s.blade_errors {
            let (p, d) = ((s.blade_pitch_deg + pe).to_radians(), (s.blade_droop_deg + de).to_radians());
            let pitch = hw * p.sin().abs() + s.blade_thickness;
            down = down.max(len * d.sin().max(0.0) + pitch);
            up = up.max(len * (-d).sin().max(0.0) + pitch);
        }
        v.push(Volume::hard([-rbl, -rbl, blade_z - down.max(0.04) - m], [rbl, rbl, blade_z + up + 0.001 + m]));
        // the light kit: its radius and its bottom
        let (rk, bottom) = match s.light_kit {
            LightKit::None => if matches!(s.style, Style::Traditional | Style::Tropical) { (sr * 0.5, zk - 0.07) } else { (sr * 0.6, zk - 0.028) },
            LightKit::Bowl => (sr * 1.7, zk - sr * 0.9),
            LightKit::Globe => (sr * 1.25, zk - 0.015 - sr * 1.25 * 1.95),
            LightKit::Schoolhouse => (sr * 1.15, zk - 0.012 - sr * 2.0),
            LightKit::Disc => ((hr * 1.05).max(0.11), zk - 0.024),
            LightKit::Cage => (sr * 1.1 + 0.0015, zk - 0.03 - sr * 2.2 - 0.0015),
            LightKit::Tulips => (sr + 0.1 + 0.055 + 0.006, zk - 0.07 - 0.11),
        };
        v.push(Volume::hard([-rk - m, -rk - m, bottom - m], [rk + m, rk + m, zk + 0.001]));
        // the pull cords: a chain from the switch housing swaying from vertical, its bow and twist, a fob below
        for i in 0..s.pull_cords as usize {
            let (sway, _, twist, _) = s.cord_states[i % s.cord_states.len()];
            let ang = 0.6 + i as f64 * 1.4;
            let (x, y) = (sr * 0.85 * ang.cos(), sr * 0.85 * ang.sin());
            let len = s.cord_length * (1.0 - 0.18 * i as f64);
            let reach = len * sway.sin().abs() + 0.004 * twist.abs() + 0.08 * sway.abs() * len + 0.012;
            let z0 = zs - sh * 0.5;
            v.push(Volume::hard([x - reach, y - reach, z0 - len * sway.cos() - 0.032 - m], [x + reach, y + reach, z0 + 0.002]));
        }
        v
    }
}

impl Factory for CeilingFan {
    fn asset(&self, _i: u64) -> Asset {
        self.build().0
    }
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        Some(self.volumes_from_spec())
    }
}

pub fn handles(c: &str) -> bool {
    c == "CeilingFanFactory"
}

pub fn make(_c: &str, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(CeilingFan::sample(fs, &FanOverrides::default())))
}
