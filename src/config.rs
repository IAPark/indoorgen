//! One typed generation config (`GenConfig`) for the whole pipeline: native CLI (`gen2 --config FILE.json`, `--dump-config`, `--schema`),
//! the wasm module (`generate(seed, room, config_json)`, `config_schema()`, `default_config()`) and the web settings panel.
//!
//! Every field is optional in JSON (missing = the default = today's behaviour); unknown keys are ignored. The old environment
//! variables keep working: `GenConfig::apply_vars` maps them onto the fields (gen2 does that at startup, after `--config` and before the
//! CLI flags). Debug-only variables (`*DBG`, `*LOG`, `PHDUMP`, `TILTCHECK`, ...) are not part of the config.
//!
//! Deep crates that cannot see this type (cloth `Calib`, solver internals) read their tunables through `ig_core::envx`; `install()`
//! pushes the config values there once per run.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// All generation settings. Sections: house, program, solver, mess, cloth, people, geometry, post, cameras, output, rules, advanced.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct GenConfig {
    /// Which house and which room.
    pub house: House,
    /// Which furnishing program is solved.
    pub program: Program,
    /// Furniture solver (simulated annealing) settings.
    pub solver: Solver,
    /// Lived-in disorder added after the solve.
    pub mess: Mess,
    /// Cloth simulation for bed linen.
    pub cloth: Cloth,
    /// Posed people (native only).
    pub people: People,
    /// Mesh detail budget.
    pub geometry: Geometry,
    /// Post-solve fix-ups.
    pub post: Post,
    /// Camera poses written to the sidecar json.
    pub cameras: Cameras,
    /// Output options (native).
    pub output: Output,
    /// Independently randomizable factors (doors, wall/ceiling/floor colours, lighting, furniture seed + jitter); see src/variation.rs. Mess and people seeds are in their sections.
    pub variation: crate::variation::Variation,
    /// Room features (fireplaces with chimney breasts, bookshelf walls, window seats) built into the shells before the furniture solve. Off by default.
    pub features: crate::features::Features,
    /// Editable furniture rules (applied to the furnishing program before the solve).
    pub rules: Option<crate::rules::RuleSpec>,
    /// Escape hatch for rarely used tunables.
    pub advanced: Advanced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct House {
    /// Random seed: the floor plan and everything else derive from it.
    pub seed: u64,
    /// Target room type, e.g. Bedroom, Kitchen, LivingRoom, DiningRoom, Bathroom (cameras favour it). Empty: no target.
    pub room: Option<String>,
    /// Furnish only the first room of the target type (fast). Off: furnish every room of the house.
    pub only_target: bool,
}
impl Default for House {
    fn default() -> Self { House { seed: 0, room: None, only_target: false } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Program {
    /// Dense furnishing program (lived-in rooms, Infinigen solver steps 300/200/50). Off: the program exactly as exported from python (fewer objects, steps 100/40/5).
    pub dense: bool,
    /// Dense program: at most this many shelf trinket sets per shelf piece.
    #[schemars(range(min = 0, max = 20))]
    pub trinkets_per_shelf: usize,
    /// Dense program: at most this many shelf trinket sets per house (each is ~250k triangles before the budget).
    #[schemars(range(min = 0, max = 400))]
    pub trinkets_max: usize,
    /// Dense program: kitchen counters may be placed over windows.
    pub counter_over_windows: bool,
}
impl Default for Program {
    fn default() -> Self { Program { dense: true, trinkets_per_shelf: 3, trinkets_max: 40, counter_over_windows: true } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Solver {
    /// Annealing steps for large furniture (default: 300 dense / 100 original program).
    #[schemars(range(min = 1, max = 5000))]
    pub steps_large: Option<usize>,
    /// Annealing steps for medium furniture (default: 200 dense / 40 original).
    #[schemars(range(min = 1, max = 5000))]
    pub steps_medium: Option<usize>,
    /// Annealing steps for small objects (default: 50 dense / 5 original).
    #[schemars(range(min = 1, max = 5000))]
    pub steps_small: Option<usize>,
    /// Multiplier for the default step counts (not for explicit steps_*). 0.6 is the fast web preset, 1 the gen2 quality.
    #[schemars(range(min = 0.05, max = 4.0))]
    pub steps_scale: f64,
    /// Pose samples tried per placement of large objects (dense program only).
    #[schemars(range(min = 1, max = 400))]
    pub place_tries: usize,
    /// Constructive placement: screen candidate poses against the parent footprint before the exact checks (dense only, experimental, no wall-time gain).
    pub constructive: bool,
    /// Candidate poses per constructive placement.
    #[schemars(range(min = 1, max = 1024))]
    pub place_cand: usize,
    /// Reuse placeholder geometry between solver steps (dense only).
    pub placeholder_reuse: bool,
    /// Analytic bounding boxes for cups, jars, sofas, ... instead of building the full mesh in the solver (dense only; native; always on in wasm).
    pub cheap_placeholders: bool,
    /// Convex half-plane stability test (faster, slightly different placement; always on in wasm).
    pub fast_stable: bool,
}
impl Default for Solver {
    fn default() -> Self {
        Solver { steps_large: None, steps_medium: None, steps_small: None, steps_scale: 1.0, place_tries: 40, constructive: false, place_cand: 64, placeholder_reuse: true, cheap_placeholders: true, fast_stable: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Mess {
    /// Lived-in disorder 0..1 (clutter, open doors, moved furniture). 0 leaves the scene untouched.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub level: f64,
    /// Seed of the mess stream (null: house.seed). Furniture, floor plan and the other factors do not depend on it.
    pub seed: Option<u64>,
}
impl Default for Mess {
    fn default() -> Self { Mess { level: 0.0, seed: None } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Cloth {
    /// Run the Rust cloth simulation for bed linen. Off (native default): the cloth is exported as a sidecar job that Blender re-simulates at render time.
    pub on: bool,
    /// Substeps per frame (more: tighter drape, slower). 2 = gen2 quality, 1 = web preset.
    #[schemars(range(min = 1, max = 8))]
    pub steps: u32,
    /// Conjugate-gradient iterations per solve (40 = gen2 quality, 20 = web preset).
    #[schemars(range(min = 5, max = 200))]
    pub cg_iters: usize,
    /// Conjugate-gradient tolerance.
    #[schemars(range(min = 0.0000001, max = 0.01))]
    pub cg_tol: f64,
    /// Multiplier on the substeps per frame.
    #[schemars(range(min = 1, max = 8))]
    pub substeps_mult: u32,
    /// Collision thickness (m).
    #[schemars(range(min = 0.001, max = 0.1))]
    pub collision_thickness: f64,
    /// Friction against colliders.
    #[schemars(range(min = 0.0, max = 50.0))]
    pub friction: f64,
    /// Multiplier on the spring stiffness.
    #[schemars(range(min = 0.1, max = 10.0))]
    pub stiffness_scale: f64,
    /// Pressure from the pillow/body only acts on vertices with a collider this far below (m); 0 disables.
    #[schemars(range(min = 0.0, max = 0.5))]
    pub support_reach: f64,
    /// Final projection that removes remaining penetrations.
    pub final_project: bool,
}
impl Default for Cloth {
    fn default() -> Self {
        Cloth { on: false, steps: 2, cg_iters: 40, cg_tol: 1e-4, substeps_mult: 1, collision_thickness: 0.035, friction: 10.0, stiffness_scale: 1.0, support_reach: 0.12, final_project: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct People {
    /// Seed of the people placement (null: house.seed).
    pub seed: Option<u64>,
    /// Up to this many posed photoreal avatars placed after the solve (needs the Rocketbox assets; native only, ignored in the browser).
    #[schemars(range(min = 0, max = 12))]
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Geometry {
    /// Simplify meshes to a per-object triangle budget (smaller files and memory). Off: full source detail (huge scenes).
    pub mesh_budget: bool,
    /// Multiplier on the triangle budget of every object (2 = twice the detail).
    #[schemars(range(min = 0.1, max = 8.0))]
    pub budget_scale: f64,
}
impl Default for Geometry {
    fn default() -> Self { Geometry { mesh_budget: true, budget_scale: 1.0 } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Post {
    /// Snap supported objects onto the visible support surface and remove overlapping rugs / cabinets.
    pub snap: bool,
    /// Push objects out of the walls (needs snap).
    pub wall_push: bool,
    /// Sweep door leaves against furniture and walls and limit their opening angle.
    pub door_settle: bool,
    /// Bookcases (living rooms, bedrooms) and built-in bookshelf walls fill this mean share of their boards with rows of books
    /// (each unit draws its own share within +-0.25). 0: off, shelves only hold what the solver placed.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub shelf_books: f64,
}
impl Default for Post {
    fn default() -> Self { Post { snap: true, wall_push: true, door_settle: true, shelf_books: 0.7 } }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Cameras {
    /// Photographer-like camera poses per furnished room, written to the sidecar json. 0: none.
    #[schemars(range(min = 0, max = 16))]
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Output {
    /// Write the quality-control report (placement defects) to this path prefix (native).
    pub qc_path: Option<String>,
    /// Stop before the mesh budget and glb export (native; for QC runs).
    pub no_glb: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Advanced {
    /// Extra tunables by their native environment-variable name (e.g. {"CL_G": 0.01}); true sets a flag. Wins over the fields above where both exist.
    pub env: std::collections::BTreeMap<String, Value>,
}

impl GenConfig {
    /// the defaults of the browser build: fast web preset (one room, mess 0.5, Rust cloth, 4 cameras, steps x0.6, cloth 1/20)
    pub fn web_default() -> GenConfig {
        let mut c = GenConfig::default();
        c.house.only_target = true;
        c.mess.level = 0.5;
        c.cloth.on = true;
        c.cloth.steps = 1;
        c.cloth.cg_iters = 20;
        c.solver.steps_scale = 0.6;
        c.cameras.count = 4;
        c
    }

    /// the JSON Schema of the config (field docs become descriptions)
    pub fn schema() -> Value {
        serde_json::to_value(schemars::schema_for!(GenConfig)).unwrap()
    }

    /// `base` with the (partial) JSON object `over` deep-merged on top, parsed as a config
    pub fn from_partial(base: &GenConfig, over: &Value) -> Result<GenConfig, String> {
        let mut v = serde_json::to_value(base).map_err(|e| e.to_string())?;
        merge(&mut v, over);
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    /// Map native environment variables (or any `get(name)` source) onto the fields. Only variables that are present change a field.
    /// Same semantics as the old `std::env::var*` reads: flags count when set (any value), `NO_*` flags switch a feature off.
    pub fn apply_vars(&mut self, get: &dyn Fn(&str) -> Option<String>) {
        let has = |k: &str| get(k).is_some();
        let num = |k: &str| get(k).and_then(|v| v.trim().parse::<f64>().ok());
        if get("INFINIGEN_PROGRAM").map_or(false, |v| v == "1") { self.program.dense = false; }
        if has("ONLY_TARGET") { self.house.only_target = true; }
        if has("NO_PH_REUSE") { self.solver.placeholder_reuse = false; }
        if has("NO_CHEAP_PH") { self.solver.cheap_placeholders = false; }
        if has("CONSTRUCTIVE") { self.solver.constructive = true; }
        if has("FAST_STABLE") { self.solver.fast_stable = true; }
        if has("NO_COUNTER_WIN") { self.program.counter_over_windows = false; }
        if let Some(v) = num("PLACE_TRIES") { self.solver.place_tries = v as usize; }
        if let Some(v) = num("PLACE_CAND") { self.solver.place_cand = v as usize; }
        if let Some(v) = num("STEPS_L") { self.solver.steps_large = Some(v as usize); }
        if let Some(v) = num("STEPS_M") { self.solver.steps_medium = Some(v as usize); }
        if let Some(v) = num("STEPS_S") { self.solver.steps_small = Some(v as usize); }
        if let Some(v) = num("TRINKETS_PER_SHELF") { self.program.trinkets_per_shelf = v as usize; }
        if let Some(v) = num("TRINKETS_MAX") { self.program.trinkets_max = v as usize; }
        if get("MESH_BUDGET").map_or(false, |v| v == "0") { self.geometry.mesh_budget = false; }
        if let Some(v) = num("MESH_BUDGET_SCALE") { self.geometry.budget_scale = v; }
        if has("NO_SNAP") { self.post.snap = false; }
        if has("NO_WALLPUSH") { self.post.wall_push = false; }
        if let Some(v) = num("CL_STEPS") { self.cloth.steps = v as u32; }
        if let Some(v) = num("CL_CG") { self.cloth.cg_iters = v as usize; }
        if let Some(v) = num("CL_TOL") { self.cloth.cg_tol = v; }
        if let Some(v) = num("CL_SUBM") { self.cloth.substeps_mult = v as u32; }
        if let Some(v) = num("CL_COL") { self.cloth.collision_thickness = v; }
        if let Some(v) = num("CL_FRIC") { self.cloth.friction = v; }
        if let Some(v) = num("CL_K") { self.cloth.stiffness_scale = v; }
        if let Some(v) = num("CL_SUPPORT") { self.cloth.support_reach = v; }
        if let Some(v) = num("CL_FINAL") { self.cloth.final_project = v > 0.5; }
    }

    /// `apply_vars` over the process environment
    pub fn apply_env(&mut self) {
        self.apply_vars(&|k| std::env::var(k).ok());
    }

    /// Push the values that deep crates read through `ig_core::envx` (cloth `Calib`, fast_stable, advanced.env). Call once per run before
    /// `build_scene` (build_scene does). Values equal to the default are left alone so that a plain environment variable keeps working.
    pub fn install(&self) {
        use ig_core::envx::{set_fast_stable, set_var};
        let d = Cloth::default();
        let c = &self.cloth;
        let mut f = |k: &str, v: f64, dv: f64| if v != dv { set_var(k, &format!("{v}")) };
        f("CL_STEPS", c.steps as f64, d.steps as f64);
        f("CL_CG", c.cg_iters as f64, d.cg_iters as f64);
        f("CL_TOL", c.cg_tol, d.cg_tol);
        f("CL_SUBM", c.substeps_mult as f64, d.substeps_mult as f64);
        f("CL_COL", c.collision_thickness, d.collision_thickness);
        f("CL_FRIC", c.friction, d.friction);
        f("CL_K", c.stiffness_scale, d.stiffness_scale);
        f("CL_SUPPORT", c.support_reach, d.support_reach);
        f("CL_FINAL", if c.final_project { 1.0 } else { 0.0 }, 1.0);
        set_fast_stable(self.solver.fast_stable);
        #[cfg(not(target_arch = "wasm32"))]
        if !self.solver.cheap_placeholders { std::env::set_var("NO_CHEAP_PH", "1"); }
        for (k, v) in &self.advanced.env {
            match v {
                Value::Bool(false) | Value::Null => {}
                Value::Bool(true) => set_var(k, "1"),
                Value::String(t) => set_var(k, t),
                other => set_var(k, &other.to_string()),
            }
        }
    }

    /// annealing steps (large, medium, small): explicit values, else the program defaults times `steps_scale`
    pub fn steps(&self) -> (usize, usize, usize) {
        let (dl, dm, ds) = if self.program.dense { (300.0, 200.0, 50.0) } else { (100.0, 40.0, 5.0) };
        let sc = |d: f64| ((d * self.solver.steps_scale).round() as usize).max(1);
        (self.solver.steps_large.unwrap_or(sc(dl)), self.solver.steps_medium.unwrap_or(sc(dm)), self.solver.steps_small.unwrap_or(sc(ds)))
    }
}

/// deep merge: objects merge key by key, everything else (also null and arrays) replaces
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && v.is_object() => merge(bv, v),
                    _ => { b.insert(k.clone(), v.clone()); }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}
