//! Independently randomizable factors for noisy training data: door open status, wall/ceiling/floor colour, mess (config `mess`), time of
//! day / lighting, furniture + furniture positions, people (config `people`). Config section `variation` (+ `mess.seed`, `people.seed`).
//!
//! Independence rule: every factor draws from its OWN random stream, hashed from (its seed, factor name, a stable key such as the room or
//! door index), never from a shared stream, so changing one factor's seed/settings leaves every other factor's draws unchanged. Seeds are
//! optional: `null` (default) derives from `house.seed`, which keeps the default output exactly as before the factors existed.
//!
//! Couplings that cannot be removed physically (documented, see BACKLOG V1):
//!  * furniture -> doors (a door opens only as far as it clears the furniture: the drawn angle is the same, the settled angle can differ),
//!  * furniture -> mess (mess moves/decorates the furniture that exists), furniture -> people (people sit on / walk around the furniture),
//!  * mess -> people (people avoid the clutter keep-out and use chairs where the mess left them), doors -> people/clutter (door swing keep-out),
//!  * doors -> mess clutter (clutter keeps off the swept door zone).
use ig_core::rng::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// All variation factors. Defaults reproduce the output of the generator without this section byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Variation {
    /// Door open status.
    pub doors: Doors,
    /// Wall colour (base colour in the glb material + `variation.walls` in the sidecar for the renderer's procedural wall material).
    pub walls: SurfaceColor,
    /// Ceiling colour (same options as walls).
    pub ceiling: SurfaceColor,
    /// Floor colour (same options as walls; tints the floor material's base colour).
    pub floor: SurfaceColor,
    /// Time of day, sun, sky and lamps (computed values are written to the sidecar `variation.lighting`; the renderer applies them).
    pub lighting: Lighting,
    /// Furniture layout: separate seed for the furniture solve and optional small post-solve position jitter.
    pub furniture: Furniture,
    /// Room features factor (fireplaces, bookshelf walls, window seats): own seed, mode default | none | random | force (see the `features` section).
    pub features: crate::features::FeatureVar,
    /// Always write the `variation` record (every factor's effective seed/settings) to the sidecar, even when everything is default.
    /// The record is also written whenever any factor is not default.
    pub record: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DoorMode {
    /// Infinigen's own draw (about half the doors nearly closed, the rest at random angles).
    Default,
    /// Every door closed.
    Closed,
    /// Every door open to a random angle in angle_deg_range.
    Open,
    /// Each door is open with probability open_prob (random angle in angle_deg_range), else closed.
    Random,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Doors {
    /// Seed of the door stream (null: derived from house.seed). Each door draws from (seed, door index): independent of the other doors and factors.
    pub seed: Option<u64>,
    /// How the desired opening angles are chosen. The furniture/wall clearance sweep (post.door_settle) still limits them.
    pub mode: DoorMode,
    /// mode random: probability that a door is open.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub open_prob: f64,
    /// Opening angle range in degrees for open doors (modes open and random).
    pub angle_deg_range: [f64; 2],
}
impl Default for Doors {
    fn default() -> Self { Doors { seed: None, mode: DoorMode::Default, open_prob: 0.5, angle_deg_range: [15.0, 90.0] } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// Infinigen's own material colour (nothing changes).
    Default,
    /// A colour drawn from `palette` (sRGB, 0..1).
    Palette,
    /// Hue/saturation/value drawn uniformly from the three ranges.
    RandomHsv,
    /// The colours given in `fixed` (rooms not listed keep their default).
    Fixed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SurfaceColor {
    /// Seed of the colour stream (null: derived from house.seed). Each room draws from (seed, room name): independent of the other rooms.
    pub seed: Option<u64>,
    /// How the colour is chosen.
    pub mode: ColorMode,
    /// mode palette: candidate colours as sRGB [r,g,b] in 0..1.
    pub palette: Vec<[f64; 3]>,
    /// mode random_hsv: hue range (0..1, wraps).
    pub hue_range: [f64; 2],
    /// mode random_hsv: saturation range.
    pub sat_range: [f64; 2],
    /// mode random_hsv: value (brightness) range.
    pub value_range: [f64; 2],
    /// One colour per room (true) or one colour for the whole house (false).
    pub per_room: bool,
    /// mode fixed: sRGB [r,g,b] by room name ("bedroom_0/0"), room type ("bedroom") or "*" (every other room).
    pub fixed: BTreeMap<String, [f64; 3]>,
}
impl Default for SurfaceColor {
    fn default() -> Self {
        SurfaceColor { seed: None, mode: ColorMode::Default, palette: vec![], hue_range: [0.0, 1.0], sat_range: [0.05, 0.5], value_range: [0.6, 0.95], per_room: true, fixed: BTreeMap::new() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LightMode {
    /// Infinigen's own lighting (the renderer's default sun; nothing is computed).
    Default,
    /// Sun from time_of_day_h (or the explicit angles).
    Fixed,
    /// Time of day drawn uniformly from time_range_h with the lighting seed.
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LampMode {
    /// Infinigen's own draw (the lights_off stage: 20 percent of scenes have all lamps off).
    Default,
    /// All lamps on.
    On,
    /// All lamps off.
    Off,
    /// Each lamp is on with probability lamp_on_prob.
    Random,
    /// On when the sun is below the horizon (night), off otherwise.
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Lighting {
    /// Seed of the lighting stream (null: derived from house.seed): time of day in mode random, per-lamp draws.
    pub seed: Option<u64>,
    /// default: no sun computed. fixed: use time_of_day_h. random: draw the time from time_range_h.
    pub mode: LightMode,
    /// mode fixed: local solar time in hours (0..24; 12 = solar noon).
    #[schemars(range(min = 0.0, max = 24.0))]
    pub time_of_day_h: f64,
    /// mode random: [from, to] hours, may wrap past midnight (e.g. [18, 6]).
    pub time_range_h: [f64; 2],
    /// Simple solar model: latitude in degrees north.
    #[schemars(range(min = -90.0, max = 90.0))]
    pub latitude_deg: f64,
    /// Simple solar model: day of the year (1..365; 80 = March equinox, 172 = June solstice).
    #[schemars(range(min = 1, max = 365))]
    pub day_of_year: u32,
    /// Compass bearing (degrees clockwise from north) of the scene's +Y axis: rotates the sun around the house.
    pub y_axis_bearing_deg: f64,
    /// Override the computed sun azimuth (compass degrees, clockwise from north).
    pub sun_azimuth_deg: Option<f64>,
    /// Override the computed sun elevation (degrees above the horizon).
    pub sun_elevation_deg: Option<f64>,
    /// Sun lamp strength (Blender sun units; null: computed from the elevation, 0 at night).
    pub sun_strength: Option<f64>,
    /// Sky (world) strength (null: computed from the elevation, small at night).
    pub sky_strength: Option<f64>,
    /// Sun colour temperature in Kelvin (null: computed, reddish near the horizon).
    pub color_temp_k: Option<f64>,
    /// Lamp rule (applies to ceiling lights too unless ceiling_lights is set).
    pub lamps: LampMode,
    /// lamps random: probability that a lamp is on.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub lamp_on_prob: f64,
    /// Multiplier on the power of every lamp that is on.
    #[schemars(range(min = 0.0, max = 20.0))]
    pub lamp_strength_scale: f64,
    /// Force the ceiling lights on (true) or off (false) regardless of the lamp rule; null follows it.
    pub ceiling_lights: Option<bool>,
}
impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            seed: None, mode: LightMode::Default, time_of_day_h: 14.0, time_range_h: [0.0, 24.0], latitude_deg: 40.0, day_of_year: 80, y_axis_bearing_deg: 0.0, sun_azimuth_deg: None,
            sun_elevation_deg: None, sun_strength: None, sky_strength: None, color_temp_k: None, lamps: LampMode::Default, lamp_on_prob: 0.5, lamp_strength_scale: 1.0, ceiling_lights: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Furniture {
    /// Seed of the furniture solve and the asset factory seeds (null: house.seed, i.e. today's furniture). The floor plan, room shells, doors and windows
    /// come only from house.seed.
    pub seed: Option<u64>,
    /// Small post-solve moves of the floor furniture.
    pub jitter: Jitter,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Jitter {
    /// Seed of the jitter stream (null: derived from the furniture seed). Each piece draws from (seed, object key).
    pub seed: Option<u64>,
    /// Largest shift in metres (0 with rotate_deg 0: jitter off). A move is kept only if it creates no new overlap with furniture or walls.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub translate_m: f64,
    /// Largest rotation about the vertical axis in degrees.
    #[schemars(range(min = 0.0, max = 90.0))]
    pub rotate_deg: f64,
}
impl Default for Jitter {
    fn default() -> Self { Jitter { seed: None, translate_m: 0.0, rotate_deg: 0.0 } }
}

// ---------------------------------------------------------------------------------------------------------------- helpers

fn stream(seed: u64, factor: &str, key: &str) -> Rng {
    Rng::seeded(ig_core::int_hash(&[&seed.to_string(), "variation", factor, key]))
}

pub fn srgb_to_linear(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [f64; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h.floor(), h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 { 0 => [v, t, p], 1 => [q, v, p], 2 => [p, v, t], 3 => [p, q, v], 4 => [t, p, v], _ => [v, p, q] }
}
fn range_draw(r: &mut Rng, rg: [f64; 2]) -> f64 {
    r.uniform(rg[0].min(rg[1]), rg[0].max(rg[1]).max(rg[0].min(rg[1]) + 1e-12))
}

impl SurfaceColor {
    /// the sRGB colour for `room` (name like "bedroom_0/0"), or None when the room keeps its default material
    pub fn pick(&self, house_seed: u64, what: &str, room: &str, room_type: &str) -> Option<[f64; 3]> {
        let seed = self.seed.unwrap_or(house_seed);
        let key = if self.per_room { room } else { "*" };
        match self.mode {
            ColorMode::Default => None,
            ColorMode::Palette => {
                if self.palette.is_empty() { return None }
                let mut r = stream(seed, what, key);
                Some(self.palette[r.choice_idx(self.palette.len())])
            }
            ColorMode::RandomHsv => {
                let mut r = stream(seed, what, key);
                let (h, s, v) = (range_draw(&mut r, self.hue_range), range_draw(&mut r, self.sat_range), range_draw(&mut r, self.value_range));
                Some(hsv_to_rgb(h, s, v))
            }
            ColorMode::Fixed => self.fixed.get(room).or_else(|| self.fixed.get(room_type)).or_else(|| self.fixed.get("*")).copied(),
        }
    }
    /// pick for every room; returns {room: (srgb, linear)} for the rooms that change
    pub fn pick_all(&self, house_seed: u64, what: &str, rooms: &[(String, String)]) -> BTreeMap<String, ([f64; 3], [f64; 3])> {
        let mut out = BTreeMap::new();
        for (name, rt) in rooms {
            // room_type() gives "bedroom"; accept the CamelCase tag name too
            if let Some(c) = self.pick(house_seed, what, name, rt) {
                out.insert(name.clone(), (c, c.map(srgb_to_linear)));
            }
        }
        out
    }
    pub fn record(&self, house_seed: u64, picked: &BTreeMap<String, ([f64; 3], [f64; 3])>) -> Value {
        json!({
            "mode": self.mode, "seed": self.seed.unwrap_or(house_seed), "per_room": self.per_room,
            "colors": picked.iter().map(|(k, v)| (k.clone(), json!(v.1))).collect::<serde_json::Map<_, _>>(),
            "colors_srgb": picked.iter().map(|(k, v)| (k.clone(), json!(v.0))).collect::<serde_json::Map<_, _>>(),
        })
    }
}

impl Doors {
    pub fn active(&self) -> bool { self.mode != DoorMode::Default }
    /// the desired opening angle (radians) of the door with cutter index `ci`; `python` is Infinigen's own draw
    pub fn angle(&self, house_seed: u64, ci: usize, python: f64) -> f64 {
        let seed = self.seed.unwrap_or(house_seed);
        let mut r = stream(seed, "doors", &ci.to_string());
        let (u_open, u_ang) = (r.f64(), r.f64());
        let (a0, a1) = (self.angle_deg_range[0].min(self.angle_deg_range[1]), self.angle_deg_range[0].max(self.angle_deg_range[1]));
        let open = (a0 + (a1 - a0) * u_ang).to_radians();
        match self.mode {
            DoorMode::Default => python,
            DoorMode::Closed => 0.0,
            DoorMode::Open => open,
            DoorMode::Random => if u_open < self.open_prob { open } else { 0.0 },
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- lighting

/// Solar position for a local solar time: declination (Cooper), hour angle 15 deg/h, standard spherical astronomy.
/// Returns (elevation, compass azimuth clockwise from north) in degrees. No equation of time, no refraction.
pub fn solar_position(lat_deg: f64, day_of_year: u32, hour: f64) -> (f64, f64) {
    let dec = 23.44f64.to_radians() * (std::f64::consts::TAU * (284.0 + day_of_year as f64) / 365.0).sin();
    let (lat, ha) = (lat_deg.to_radians(), (15.0 * (hour - 12.0)).to_radians());
    let sin_el = lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos();
    let el = sin_el.clamp(-1.0, 1.0).asin();
    // azimuth from north through east: atan2(east, north) of the sun direction
    let east = -dec.cos() * ha.sin();
    let north = dec.sin() * lat.cos() - dec.cos() * lat.sin() * ha.cos();
    (el.to_degrees(), east.atan2(north).to_degrees().rem_euclid(360.0))
}

/// Everything computed for the lighting factor (sidecar `variation.lighting`).
#[derive(Clone, Debug)]
pub struct LightResult {
    pub time_h: f64,
    pub elevation_deg: f64,
    pub azimuth_compass_deg: f64,
    pub sun_dir: [f64; 3],
    pub night: bool,
    pub sun_strength: f64,
    pub sky_strength: f64,
    pub color_temp_k: f64,
}

impl Lighting {
    pub fn active(&self) -> bool { self.mode != LightMode::Default || self.lamps != LampMode::Default || self.lamp_strength_scale != 1.0 || self.ceiling_lights.is_some() }

    /// None in mode default. Sun model: elevation/azimuth from `solar_position`; strength 5 * sqrt(sin el) (0 below 1 degree), sky 0.05 + 0.95 * sqrt(sin el),
    /// colour temperature 2200 K at the horizon rising to 5800 K at 30 degrees elevation.
    pub fn solve(&self, house_seed: u64) -> Option<LightResult> {
        if self.mode == LightMode::Default { return None }
        let seed = self.seed.unwrap_or(house_seed);
        let t = if self.mode == LightMode::Random {
            let (a, b) = (self.time_range_h[0], self.time_range_h[1]);
            let span = if (b - a).abs() >= 24.0 { 24.0 } else { (b - a).rem_euclid(24.0) };
            (a + stream(seed, "lighting", "time").f64() * span).rem_euclid(24.0)
        } else {
            self.time_of_day_h.rem_euclid(24.0)
        };
        let (mut el, mut az) = solar_position(self.latitude_deg, self.day_of_year, t);
        if let Some(e) = self.sun_elevation_deg { el = e }
        if let Some(a) = self.sun_azimuth_deg { az = a.rem_euclid(360.0) }
        let (e, m) = (el.to_radians(), (90.0 - (az - self.y_axis_bearing_deg)).to_radians()); // m: angle from scene +X counter-clockwise
        let sun_dir = [e.cos() * m.cos(), e.cos() * m.sin(), e.sin()];
        let s = e.sin().max(0.0);
        let sun_strength = self.sun_strength.unwrap_or(if el < 1.0 { 0.0 } else { 5.0 * s.sqrt() });
        let sky_strength = self.sky_strength.unwrap_or(0.05 + 0.95 * s.sqrt());
        let color_temp_k = self.color_temp_k.unwrap_or(2200.0 + 3600.0 * (el / 30.0).clamp(0.0, 1.0));
        Some(LightResult { time_h: t, elevation_deg: el, azimuth_compass_deg: az, sun_dir, night: el < 2.0, sun_strength, sky_strength, color_temp_k })
    }

    /// whether the lamp `key` (class `class`) is on; `python_on` is Infinigen's own lights_off outcome, `night` the sun state
    pub fn lamp_on(&self, house_seed: u64, key: &str, class: &str, python_on: bool, night: bool) -> bool {
        let ceiling = class == "CeilingLightFactory" || class == "CeilingClassicLampFactory";
        if ceiling {
            if let Some(c) = self.ceiling_lights { return c }
        }
        let seed = self.seed.unwrap_or(house_seed);
        match self.lamps {
            LampMode::Default => python_on,
            LampMode::On => true,
            LampMode::Off => false,
            LampMode::Random => stream(seed, "lamps", key).f64() < self.lamp_on_prob,
            LampMode::Auto => night,
        }
    }

    pub fn record(&self, house_seed: u64, res: &Option<LightResult>, lamps: &[(String, bool, f64)]) -> Value {
        let mut j = json!({
            "mode": self.mode, "seed": self.seed.unwrap_or(house_seed), "lamps_mode": self.lamps, "lamp_on_prob": self.lamp_on_prob, "lamp_strength_scale": self.lamp_strength_scale,
            "ceiling_lights": self.ceiling_lights,
            "lamps": lamps.iter().map(|(k, on, w)| (k.clone(), json!({"on": on, "watt": w}))).collect::<serde_json::Map<_, _>>(),
            "lamps_on": lamps.iter().filter(|l| l.1).count(), "lamps_total": lamps.len(),
        });
        if let Some(r) = res {
            let m = r.sun_dir[1].atan2(r.sun_dir[0]).to_degrees();
            for (k, v) in [
                ("time_of_day_h", json!(r.time_h)), ("latitude_deg", json!(self.latitude_deg)), ("day_of_year", json!(self.day_of_year)), ("y_axis_bearing_deg", json!(self.y_axis_bearing_deg)),
                ("sun_elevation_deg", json!(r.elevation_deg)), ("sun_azimuth_deg", json!(r.azimuth_compass_deg)),
                ("sun_azimuth_scene_deg", json!(m)), ("sun_dir", json!(r.sun_dir)), ("is_night", json!(r.night)),
                ("sun_strength", json!(r.sun_strength)), ("sky_strength", json!(r.sky_strength)), ("color_temp_K", json!(r.color_temp_k)),
            ] { j[k] = v }
        }
        j
    }
}

// ---------------------------------------------------------------------------------------------------------------- furniture jitter

use ig_furn::geom::Pose;
use ig_furn::state::State;

fn jitter_class_ok(c: &str) -> bool {
    const SKIP: &[&str] = &[
        "Feature", "Kitchen", "Wall", "Mirror", "Ceiling", "Sink", "Toilet", "Bathtub", "Shower", "Oven", "Dishwasher", "Fridge", "Microwave", "Hardware", "Balloon", "Curtain", "Window", "Door", "Rug", "Beverage",
    ];
    !SKIP.iter().any(|s| c.contains(s))
}

fn rot_z(p: [f64; 3], a: f64) -> [f64; 3] {
    let (s, c) = a.sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

fn moved(p: &Pose, pivot: [f64; 3], dyaw: f64, d: [f64; 2]) -> Pose {
    let mut o = p.o;
    let rel = [p.t[0] - pivot[0], p.t[1] - pivot[1], p.t[2] - pivot[2]];
    let r = rot_z(rel, dyaw);
    o.loc = [(pivot[0] + r[0] + d[0]) as f32, (pivot[1] + r[1] + d[1]) as f32, (pivot[2] + r[2]) as f32];
    o.eul[2] = (o.eul[2] as f64 + dyaw) as f32;
    Pose::from_o32(o)
}

fn children(state: &State, parent: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut stack = vec![parent.to_string()];
    while let Some(p) = stack.pop() {
        for (k, o) in &state.objs {
            if o.generator.is_some() && k != parent && !out.contains(k) && o.relations.iter().any(|r| r.target == p) {
                out.push(k.clone());
                stack.push(k.clone());
            }
        }
    }
    out
}

fn inside_room(state: &State, k: &str, margin: f64) -> bool {
    let o = &state.objs[k];
    let (lo, hi) = (o.geom.lo, o.geom.hi);
    [(lo[0], lo[1]), (hi[0], lo[1]), (hi[0], hi[1]), (lo[0], hi[1])].iter().all(|&(x, y)| {
        let w = o.pose.apply([x, y, lo[2]]);
        let sq = [[w[0] - margin, w[1] - margin], [w[0] + margin, w[1] - margin], [w[0] + margin, w[1] + margin], [w[0] - margin, w[1] + margin]];
        let Ok(sq) = ig_core::shp::polygon(&sq, &[]) else { return false };
        state.objs.values().any(|r| r.polygon.as_ref().map_or(false, |p| ig_core::shp::try_contains(p, &sq)))
    })
}

/// Post-solve jitter of floor furniture (and everything resting on it). Returns (moved, candidates). A move is kept when no collision
/// depth grows beyond its original value (+4 mm) against any furniture or room shell and the footprint stays inside a room.
pub fn jitter_furniture(state: &mut State, f: &Furniture, house_seed: u64) -> (usize, usize) {
    let j = &f.jitter;
    if j.translate_m <= 0.0 && j.rotate_deg <= 0.0 { return (0, 0) }
    let seed = j.seed.unwrap_or(f.seed.unwrap_or(house_seed));
    use ig_mess::states::{depths, ok_vs};
    let keys: Vec<String> = state
        .objs
        .iter()
        .filter(|(k, o)| {
            o.generator.as_ref().map_or(false, |g| jitter_class_ok(&g.class))
                && !o.relations.is_empty()
                && o.relations.iter().all(|r| state.objs.get(&r.target).map_or(false, |t| t.polygon.is_some()))
                && state.world(k).lo[2] < 0.15
        })
        .map(|(k, _)| k.clone())
        .collect();
    let (mut n, total) = (0, keys.len());
    for k in keys {
        let mut r = stream(seed, "jitter", &k);
        let (u_r, u_a, u_y) = (r.f64(), r.f64(), r.f64());
        let (dist, ang) = (j.translate_m * u_r.sqrt(), u_a * std::f64::consts::TAU);
        let (d, yaw) = ([ang.cos() * dist, ang.sin() * dist], (u_y * 2.0 - 1.0) * j.rotate_deg.to_radians());
        let o = &state.objs[&k];
        let c = o.pose.apply([(o.geom.lo[0] + o.geom.hi[0]) / 2.0, (o.geom.lo[1] + o.geom.hi[1]) / 2.0, (o.geom.lo[2] + o.geom.hi[2]) / 2.0]);
        let mut group = vec![k.clone()];
        group.extend(children(state, &k));
        let base: Vec<_> = group.iter().map(|g| depths(state, g, &group)).collect();
        let old: Vec<Pose> = group.iter().map(|g| state.objs[g].pose).collect();
        for f in [1.0, 0.6, 0.3] {
            for (g, p) in group.iter().zip(&old) { state.set_pose(g, moved(p, c, yaw * f, [d[0] * f, d[1] * f])); }
            let now: Vec<_> = group.iter().map(|g| depths(state, g, &group)).collect();
            if base.iter().zip(&now).all(|(b, n)| ok_vs(b, n)) && inside_room(state, &k, 0.02) {
                n += 1;
                break;
            }
            for (g, p) in group.iter().zip(&old) { state.set_pose(g, *p); }
        }
    }
    (n, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn solar_sanity() {
        // equinox at 40N: noon elevation 50, due south; sunrise east, sunset west
        let (e, a) = solar_position(40.0, 80, 12.0);
        assert!((e - 50.0).abs() < 1.0 && (a - 180.0).abs() < 1.0, "{e} {a}");
        let (e6, a6) = solar_position(40.0, 80, 6.0);
        assert!(e6.abs() < 2.0 && (a6 - 90.0).abs() < 3.0, "{e6} {a6}");
        let (_, a18) = solar_position(40.0, 80, 18.0);
        assert!((a18 - 270.0).abs() < 3.0);
        assert!(solar_position(40.0, 80, 0.0).0 < -40.0);
    }
    #[test]
    fn defaults_are_inert() {
        let v = Variation::default();
        assert!(!v.doors.active() && v.lighting.solve(1).is_none() && !v.lighting.active());
        assert_eq!(v.walls.pick(1, "walls", "bedroom_0/0", "bedroom"), None);
    }
    #[test]
    fn color_streams_independent() {
        let mut w = SurfaceColor::default();
        w.mode = ColorMode::RandomHsv;
        let a = w.pick(1, "walls", "bedroom_0/0", "bedroom");
        let b = w.pick(1, "walls", "kitchen_0/0", "kitchen");
        assert_ne!(a, b);
        w.seed = Some(9);
        assert_ne!(a, w.pick(1, "walls", "bedroom_0/0", "bedroom"));
        assert_eq!(b.is_some(), true);
    }
}
