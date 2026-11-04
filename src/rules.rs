//! User-editable furniture rules: what goes into each room type, without Python.
//!
//! Infinigen's furniture rules are constraint expressions (python `home.py`), exported as a JSON node DAG
//! (`data/home_program*.json`, loader `crates/ig_furn/src/program.rs`). This module
//!  * has a small expression builder that appends nodes in that exact format ([`B`]),
//!  * defines the user-facing [`RuleSpec`] (per room type: class / relation / min / max / prefer),
//!  * compiles a spec into the program ([`apply`]: new constraints `user_<Room>_<n>`, overrides of the built-in
//!    count ranges for the same class + room, `replace`, `disable`),
//!  * exports the current built-in rules as a spec ([`default_rules`]) so a UI can show and edit them.
//!
//! An empty spec never touches the program (output is byte-identical to a run without rules).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

// ------------------------------------------------------------------ user-facing format

/// Furniture rules. Per room type a list of rules, plus a global list of constraint names to drop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RuleSpec {
    /// Rules per room type. Keys: Bedroom, LivingRoom, Kitchen, DiningRoom, Bathroom, Closet, Hallway, Garage, Balcony,
    /// StaircaseRoom, Utility, or "Any" (applies to every room: use it for global caps).
    pub rooms: BTreeMap<String, RoomRules>,
    /// Names of built-in constraints / score terms to remove (see `constraint_names` in `rules_info`), e.g. "sofa_positioning".
    pub disable: Vec<String>,
}

/// Rules of one room type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RoomRules {
    /// true: drop ALL built-in constraints and score terms of this room type first (you then define everything that
    /// goes in the room, the room may lack e.g. a sink or beds). false: your rules override / add to the built-in ones.
    pub replace: bool,
    /// The rules.
    pub rules: Vec<Rule>,
}

/// How many objects of a class a room should hold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Rule {
    /// Factory class ("SofaFactory", "ArmChairFactory", "RugFactory", ...) or semantic tag ("Storage", "Bed", "Seating",
    /// "Lighting", ...). "Plant" is short for LargePlantContainerFactory.
    pub class: String,
    /// Where the objects go: "on_floor", "against_wall", "on:<class>" (on a support surface of that class, e.g. on:Storage),
    /// "ontop:<class>" (on the top face), "beside:<class>" (left/right next to that class, e.g. beside:Bed).
    pub relation: String,
    /// Minimum number per room (omit for no minimum).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    /// Maximum number per room (omit for no maximum).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// Optional soft preference: "more" or "fewer" objects of this class (a score term, not a hard constraint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefer: Option<Prefer>,
    /// Strength of the preference (default 1; the built-in terms use 3-5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

impl Default for Rule {
    fn default() -> Self {
        Rule { class: String::new(), relation: "on_floor".into(), min: None, max: None, prefer: None, weight: None }
    }
}

/// Soft preference direction.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Prefer {
    /// maximise the count
    More,
    /// minimise the count
    Fewer,
}

pub const ROOM_TYPES: [&str; 11] =
    ["Balcony", "Bathroom", "Bedroom", "Closet", "DiningRoom", "Garage", "Hallway", "Kitchen", "LivingRoom", "StaircaseRoom", "Utility"];
const BIG: u32 = 1000;

// ------------------------------------------------------------------ canonical form / builder

fn canon(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut ks: Vec<&String> = m.keys().collect();
            ks.sort();
            let mut s = String::from("{");
            for k in ks {
                s.push_str(k);
                s.push(':');
                s.push_str(&canon(&m[k]));
                s.push(',');
            }
            s + "}"
        }
        Value::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        Value::Number(n) => format!("{}", n.as_f64().unwrap()),
        o => o.to_string(),
    }
}

fn rf(i: usize) -> Value {
    json!({ "ref": i })
}

fn tag(kind: &str, name: &str) -> Value {
    let mut m = Map::new();
    m.insert(kind.into(), json!(name));
    Value::Object(m)
}
pub fn sem(n: &str) -> Value {
    tag("sem", n)
}
pub fn gen(n: &str) -> Value {
    tag("gen", n)
}
pub fn sub(n: &str) -> Value {
    tag("sub", n)
}
pub fn neg(t: Value) -> Value {
    json!({ "neg": t })
}

/// Relation presets (copied from the python export: infinigen_examples/constraints/util.py).
pub mod rel {
    use super::*;
    fn stable(child: Vec<Value>, parent: Vec<Value>, margin: f64) -> Value {
        json!({"rel": {"_": "StableAgainst", "child_tags": child, "parent_tags": parent, "margin": margin, "check_z": true, "rev_normal": false}})
    }
    pub fn any() -> Value {
        json!({"rel": {"_": "AnyRelation"}})
    }
    /// resting on the floor
    pub fn on_floor() -> Value {
        stable(vec![neg(sub("Back")), neg(sub("Front")), neg(sub("Top")), sub("Bottom")], vec![neg(sub("Ceiling")), neg(sub("Wall")), sub("SupportSurface"), sub("Visible")], 0.01)
    }
    /// back against a wall
    pub fn against_wall() -> Value {
        stable(vec![neg(sub("Front")), neg(sub("Top")), sub("Back")], vec![neg(sub("Ceiling")), neg(sub("SupportSurface")), sub("Visible"), sub("Wall")], 0.07)
    }
    /// standing on the top face of another object
    pub fn ontop() -> Value {
        stable(vec![neg(sub("Back")), neg(sub("Front")), neg(sub("Top")), sub("Bottom")], vec![neg(sub("Back")), neg(sub("Bottom")), neg(sub("Front")), sub("Top")], 0.0)
    }
    /// on a support surface (shelf board, table top) of another object
    pub fn on() -> Value {
        stable(vec![neg(sub("Back")), neg(sub("Front")), neg(sub("Top")), sub("Bottom")], vec![sub("SupportSurface")], 0.0)
    }
    /// side by side (left/right faces)
    pub fn leftright_leftright() -> Value {
        let t = vec![neg(sub("Back")), neg(sub("Bottom")), neg(sub("Front")), neg(sub("SupportSurface")), neg(sub("Top"))];
        stable(t.clone(), t, 0.05)
    }
}

/// Expression builder over the `nodes` array of one program variant. Nodes are hash-consed: building an expression that already
/// exists returns the existing node. Every method returns the node id.
pub struct B<'a> {
    pub nodes: &'a mut Vec<Value>,
    memo: HashMap<String, usize>,
    nvar: usize,
}

impl<'a> B<'a> {
    pub fn new(nodes: &'a mut Vec<Value>) -> Self {
        let memo = Self::memo_of(nodes);
        B { nodes, memo, nvar: 0 }
    }
    /// the hash-consing memo of a nodes array (canonical form -> first id)
    pub fn memo_of(nodes: &[Value]) -> HashMap<String, usize> {
        let mut memo = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            memo.entry(canon(n)).or_insert(i);
        }
        memo
    }
    /// a builder over `nodes` with its memo kept from an earlier builder (the nodes only changed through builders since): building
    /// the memo is the cost of B::new, paid once per variant instead of once per rule
    pub fn with_memo(nodes: &'a mut Vec<Value>, memo: HashMap<String, usize>) -> Self {
        B { nodes, memo, nvar: 0 }
    }
    pub fn into_memo(self) -> HashMap<String, usize> {
        self.memo
    }
    pub fn add(&mut self, v: Value) -> usize {
        let k = canon(&v);
        if let Some(&i) = self.memo.get(&k) {
            return i;
        }
        self.nodes.push(v);
        self.memo.insert(k, self.nodes.len() - 1);
        self.nodes.len() - 1
    }
    pub fn scene(&mut self) -> usize {
        self.add(json!({"_": "scene"}))
    }
    /// `objs[tags...]` (tags sorted like the python export: negations first)
    pub fn tagged(&mut self, objs: usize, mut tags: Vec<Value>) -> usize {
        tags.sort_by_key(|t| (t.get("neg").is_none(), canon(t)));
        self.add(json!({"_": "tagged", "objs": rf(objs), "tags": {"tags": tags}}))
    }
    pub fn related_to(&mut self, child: usize, parent: usize, relation: Value) -> usize {
        self.add(json!({"_": "related_to", "child": rf(child), "parent": rf(parent), "relation": relation}))
    }
    pub fn count(&mut self, objs: usize) -> usize {
        self.add(json!({"_": "count", "objs": rf(objs)}))
    }
    pub fn in_range(&mut self, val: usize, low: f64, high: f64) -> usize {
        self.add(json!({"_": "in_range", "val": rf(val), "low": low, "high": high, "mean": 0}))
    }
    pub fn constant(&mut self, v: f64) -> usize {
        self.add(json!({"_": "constant", "value": v}))
    }
    pub fn scalar(&mut self, op: &str, operands: &[usize]) -> usize {
        self.add(json!({"_": "ScalarOperatorExpression", "func": {"op": op}, "operands": {"list": operands.iter().map(|&i| rf(i)).collect::<Vec<_>>()}}))
    }
    pub fn boolean(&mut self, op: &str, operands: &[usize]) -> usize {
        self.add(json!({"_": "BoolOperatorExpression", "func": {"op": op}, "operands": {"list": operands.iter().map(|&i| rf(i)).collect::<Vec<_>>()}}))
    }
    pub fn hinge(&mut self, val: usize, low: f64, high: f64) -> usize {
        self.add(json!({"_": "hinge", "val": rf(val), "low": low, "high": high}))
    }
    pub fn distance(&mut self, objs: usize, others: usize, others_tags: Vec<Value>) -> usize {
        self.add(json!({"_": "distance", "objs": rf(objs), "others": rf(others), "others_tags": {"tags": others_tags}}))
    }
    /// score term "maximise x": python `x.maximize(weight=w)` = x * -w (score terms are minimised)
    pub fn maximize(&mut self, x: usize, w: f64) -> usize {
        let c = self.constant(-w);
        self.scalar("mul", &[x, c])
    }
    pub fn minimize(&mut self, x: usize, w: f64) -> usize {
        let c = self.constant(w);
        self.scalar("mul", &[x, c])
    }
    fn fresh_var(&mut self) -> String {
        loop {
            self.nvar += 1;
            let n = format!("var_user_{}", self.nvar);
            if !self.memo.contains_key(&canon(&json!({"_": "item", "name": n}))) && !self.nodes.iter().any(|x| x["name"] == n.as_str() || x["var"] == n.as_str()) {
                return n;
            }
        }
    }
    fn quant(&mut self, kind: &str, objs: usize, f: impl FnOnce(&mut Self, usize) -> usize) -> usize {
        let var = self.fresh_var();
        let item = self.add(json!({"_": "item", "name": var, "member_of": rf(objs)}));
        let pred = f(self, item);
        self.add(json!({"_": kind, "objs": rf(objs), "var": var, "pred": rf(pred)}))
    }
    /// `objs.all(lambda r: pred(r))`
    pub fn for_all(&mut self, objs: usize, f: impl FnOnce(&mut Self, usize) -> usize) -> usize {
        self.quant("ForAll", objs, f)
    }
    /// `objs.mean(lambda r: expr(r))`
    pub fn mean_over(&mut self, objs: usize, f: impl FnOnce(&mut Self, usize) -> usize) -> usize {
        self.quant("MeanOver", objs, f)
    }
    pub fn sum_over(&mut self, objs: usize, f: impl FnOnce(&mut Self, usize) -> usize) -> usize {
        self.quant("SumOver", objs, f)
    }

    // ---- sets
    /// all rooms
    pub fn rooms(&mut self) -> usize {
        let s = self.scene();
        self.tagged(s, vec![neg(sem("Object")), sem("Room")])
    }
    /// all non-room objects
    pub fn objects(&mut self) -> usize {
        let s = self.scene();
        self.tagged(s, vec![neg(sem("Room")), sem("Object")])
    }
    /// `rooms[T].excludes(room_types)`; "Any" = all rooms
    pub fn rooms_of(&mut self, t: &str) -> usize {
        let r = self.rooms();
        if t == "Any" {
            return r;
        }
        let a = self.tagged(r, vec![sem(t)]);
        let negs: Vec<Value> = ROOM_TYPES.iter().filter(|&&x| x != t).map(|x| neg(sem(x))).collect();
        self.tagged(a, negs)
    }
    /// `obj[Furniture].related_to(rooms, on_floor)`
    pub fn furniture(&mut self) -> usize {
        let o = self.objects();
        let f = self.tagged(o, vec![sem("Furniture")]);
        let r = self.rooms();
        self.related_to(f, r, rel::on_floor())
    }
    /// `furniture.related_to(rooms, against_wall)`
    pub fn wallfurn(&mut self) -> usize {
        let f = self.furniture();
        let r = self.rooms();
        self.related_to(f, r, rel::against_wall())
    }
}

// ------------------------------------------------------------------ vocabulary

#[derive(Clone, Debug)]
pub struct Vocab {
    /// factory classes ("SofaFactory")
    pub gens: BTreeSet<String>,
    /// semantic tags usable as class ("Storage")
    pub sems: BTreeSet<String>,
    furniture_gens: BTreeSet<String>,
    lighting_gens: BTreeSet<String>,
    furniture_sems: BTreeSet<String>,
}

fn used_as(program: &Value, key: &str) -> BTreeSet<String> {
    program["used_as"][key].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

pub fn vocab(program: &Value) -> Vocab {
    let mut gens = BTreeSet::new();
    let mut sems = BTreeSet::new();
    let furniture_gens = used_as(program, "Furniture");
    let lighting_gens = used_as(program, "Lighting");
    let mut furniture_sems = BTreeSet::new();
    if let Some(m) = program["used_as"].as_object() {
        for (k, v) in m {
            let list: BTreeSet<String> = v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
            gens.extend(list.iter().cloned());
            let skip = ["Door", "Window", "RealPlaceholder", "AssetAsPlaceholder", "AssetPlaceholderForChildren", "PlaceholderBBox", "SingleGenerator", "NoRotation", "NoCollision", "NoChildren", "Object", "ClothDrapeItem"];
            if !list.is_empty() && !skip.contains(&k.as_str()) {
                sems.insert(k.clone());
                if k == "Furniture" || list.iter().all(|g| furniture_gens.contains(g)) {
                    furniture_sems.insert(k.clone());
                }
            }
        }
    }
    // classes that appear in the program's tags
    if let Some(vars) = program["variants"].as_object() {
        if let Some(v) = vars.values().next() {
            for n in v["nodes"].as_array().into_iter().flatten() {
                if n["_"] == "tagged" {
                    for t in n["tags"]["tags"].as_array().into_iter().flatten() {
                        if let Some(g) = t["gen"].as_str() {
                            gens.insert(g.into());
                        }
                        if let Some(s) = t["sem"].as_str() {
                            if !ROOM_TYPES.contains(&s) && !["Room", "Object", "Cutter"].contains(&s) {
                                sems.insert(s.into());
                            }
                        }
                    }
                }
            }
        }
    }
    // room features (src/features.rs): fixed objects with support surfaces, usable as relation parents (on:FireplaceFeature)
    gens.extend(["FireplaceFeature", "BookshelfWallFeature", "WindowSeatFeature", "WetBarFeature", "MudroomFeature"].map(String::from));
    Vocab { gens, sems, furniture_gens, lighting_gens, furniture_sems }
}

#[derive(Clone, Debug, PartialEq)]
enum Rel {
    OnFloor,
    AgainstWall,
    On(String),
    OnTop(String),
    Beside(String),
}

fn alias(c: &str) -> &str {
    match c {
        "Plant" => "LargePlantContainerFactory",
        o => o,
    }
}

fn parse_rel(s: &str) -> Result<Rel, String> {
    let s = s.trim();
    Ok(match s {
        "on_floor" | "" => Rel::OnFloor,
        "against_wall" => Rel::AgainstWall,
        _ => {
            let (k, p) = s.split_once(':').ok_or_else(|| format!("relation '{s}': use on_floor, against_wall, on:<class>, ontop:<class> or beside:<class>"))?;
            let p = alias(p.trim()).to_string();
            match k.trim() {
                "on" => Rel::On(p),
                "ontop" => Rel::OnTop(p),
                "beside" => Rel::Beside(p),
                _ => return Err(format!("relation '{s}': unknown kind '{k}'")),
            }
        }
    })
}

impl Vocab {
    fn tag_of(&self, c: &str) -> Value {
        if self.gens.contains(c) { gen(c) } else { sem(c) }
    }
    fn known(&self, c: &str) -> bool {
        self.gens.contains(c) || self.sems.contains(c)
    }
    fn is_furniture(&self, c: &str) -> bool {
        self.furniture_gens.contains(c) || self.furniture_sems.contains(c)
    }
    /// objects of class `c` standing on the floor (and optionally against a wall), as python does it
    fn floor_set(&self, b: &mut B, c: &str, wall: bool) -> usize {
        let c = alias(c);
        let t = self.tag_of(c);
        let rooms = b.rooms();
        if c.ends_with("Feature") {
            // room features are fixed objects built into the shell: they hold a non-geometric relation to their room, not on_floor
            let o = b.objects();
            let x = b.tagged(o, vec![t]);
            return b.related_to(x, rooms, rel::any());
        }
        if self.is_furniture(c) {
            let base = if wall { b.wallfurn() } else { b.furniture() };
            b.tagged(base, vec![t])
        } else {
            let o = b.objects();
            let mut x = b.tagged(o, vec![t]);
            if self.lighting_gens.contains(c) {
                // python: obj[Lighting][FloorLampFactory]
                let o2 = b.objects();
                let l = b.tagged(o2, vec![sem("Lighting")]);
                x = b.tagged(l, vec![self.tag_of(c)]);
            }
            let x = b.related_to(x, rooms, rel::on_floor());
            if wall {
                b.related_to(x, rooms, rel::against_wall())
            } else {
                x
            }
        }
    }
    /// the counted object set for a rule inside room variable `item`
    fn counted(&self, b: &mut B, class: &str, r: &Rel, item: usize) -> usize {
        let class = alias(class);
        match r {
            Rel::OnFloor => {
                let s = self.floor_set(b, class, false);
                b.related_to(s, item, rel::any())
            }
            Rel::AgainstWall => {
                let s = self.floor_set(b, class, true);
                b.related_to(s, item, rel::any())
            }
            Rel::On(p) | Rel::OnTop(p) => {
                let parent_all = self.floor_set(b, p, false);
                let parent_in = b.related_to(parent_all, item, rel::any());
                let o = b.objects();
                let base = if self.lighting_gens.contains(class) {
                    let l = b.tagged(o, vec![sem("Lighting")]);
                    b.tagged(l, vec![self.tag_of(class)])
                } else {
                    b.tagged(o, vec![self.tag_of(class)])
                };
                let relation = if matches!(r, Rel::On(_)) { rel::on() } else { rel::ontop() };
                let child = b.related_to(base, parent_all, relation);
                b.related_to(child, parent_in, rel::any())
            }
            Rel::Beside(p) => {
                let child_base = self.floor_set(b, class, false);
                let wf = b.wallfurn();
                let child = b.related_to(child_base, wf, rel::leftright_leftright());
                let parent_all = self.floor_set(b, p, true);
                let parent_in = b.related_to(parent_all, item, rel::any());
                b.related_to(child, parent_in, rel::any())
            }
        }
    }
}

// ------------------------------------------------------------------ analysis of existing constraints

fn refid(v: &Value) -> Option<usize> {
    v.get("ref").and_then(|x| x.as_u64()).map(|x| x as usize)
}

fn pos_tags(n: &Value) -> Vec<(&str, &str)> {
    let mut out = vec![];
    for t in n["tags"]["tags"].as_array().into_iter().flatten() {
        if let Some(o) = t.as_object() {
            if o.len() == 1 && !o.contains_key("neg") {
                let (k, v) = o.iter().next().unwrap();
                out.push((k.as_str(), v.as_str().unwrap_or("")));
            }
        }
    }
    out
}

/// the most recently applied positive class tag of an object-set expression (`storage_freestanding` -> Storage)
fn head_of(nodes: &[Value], mut i: usize) -> Option<String> {
    loop {
        let n = &nodes[i];
        match n["_"].as_str()? {
            "tagged" => {
                if let Some((_, name)) = pos_tags(n).into_iter().find(|(k, _)| *k == "sem" || *k == "gen") {
                    return Some(name.to_string());
                }
                i = refid(&n["objs"])?;
            }
            "related_to" => i = refid(&n["child"])?,
            _ => return None,
        }
    }
}

fn has_room_tag(nodes: &[Value], mut i: usize) -> bool {
    loop {
        let n = &nodes[i];
        match n["_"].as_str().unwrap_or("") {
            "tagged" => {
                if pos_tags(n).iter().any(|&(k, v)| k == "sem" && v == "Room") {
                    return true;
                }
                i = refid(&n["objs"]).unwrap_or(0);
            }
            "related_to" => i = refid(&n["child"]).unwrap_or(0),
            _ => return false,
        }
    }
}

/// room type a ForAll/MeanOver ranges over: Some(Some(T)) typed, Some(None) all rooms, None = something else (skip)
fn rooms_scope(nodes: &[Value], mut i: usize) -> Option<Option<String>> {
    let mut pos = vec![];
    let mut negs = 0;
    loop {
        let n = &nodes[i];
        match n["_"].as_str()? {
            "tagged" => {
                for t in n["tags"]["tags"].as_array()? {
                    if t.get("neg").is_some() {
                        if t["neg"].get("sem").map_or(false, |s| s != "Object") {
                            negs += 1;
                        }
                    }
                }
                for (k, v) in pos_tags(n) {
                    if k == "sem" && v != "Room" {
                        pos.push(v.to_string());
                    }
                }
                i = refid(&n["objs"])?;
            }
            "scene" => break,
            _ => return None,
        }
    }
    match (pos.len(), negs) {
        (1, _) if ROOM_TYPES.contains(&pos[0].as_str()) => Some(Some(pos[0].clone())),
        (0, 0) => Some(None),
        _ => None,
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Floor { wall: bool },
    Parent { rel: &'static str, parent: String },
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    /// None = all rooms
    room: Option<String>,
    class: String,
    kind: Kind,
    in_range: usize,
    low: f64,
    high: f64,
    rooms_node: usize,
}

fn chain_relations<'a>(nodes: &'a [Value], mut i: usize, out: &mut Vec<(&'a Value, usize)>) {
    loop {
        let n = &nodes[i];
        match n["_"].as_str().unwrap_or("") {
            "tagged" => i = refid(&n["objs"]).unwrap_or(0),
            "related_to" => {
                out.push((&n["relation"]["rel"], refid(&n["parent"]).unwrap_or(0)));
                i = refid(&n["child"]).unwrap_or(0)
            }
            _ => return,
        }
    }
}

fn parent_rel_kind(r: &Value) -> &'static str {
    let pt: Vec<(String, bool)> = r["parent_tags"].as_array().into_iter().flatten().map(|t| (canon(t), t.get("neg").is_some())).collect();
    if pt.iter().any(|(s, n)| !n && s.contains("Top")) {
        "ontop"
    } else if pt.iter().any(|(s, n)| !n && s.contains("SupportSurface")) {
        "on"
    } else {
        "beside"
    }
}

fn analyze_count(nodes: &[Value], count: usize, var: &str) -> Option<(String, Kind)> {
    let c = &nodes[count];
    if c["_"] != "count" {
        return None;
    }
    let top = &nodes[refid(&c["objs"])?];
    if top["_"] != "related_to" || top["relation"]["rel"]["_"] != "AnyRelation" {
        return None;
    }
    let x = refid(&top["child"])?;
    let p = refid(&top["parent"])?;
    let is_item = |i: usize| nodes[i]["_"] == "item" && nodes[i]["name"] == var;
    let parent_in: Option<usize> = if is_item(p) {
        None
    } else if nodes[p]["_"] == "related_to" && is_item(refid(&nodes[p]["parent"])?) {
        Some(refid(&nodes[p]["child"])?)
    } else {
        return None;
    };
    let class = head_of(nodes, x)?;
    let mut rels = vec![];
    chain_relations(nodes, x, &mut rels);
    let mut wall = false;
    let mut parent_kind: Option<(&'static str, usize)> = None;
    for (r, par) in rels {
        if r["_"] == "StableAgainst" || r["_"] == "CoPlanar" {
            if has_room_tag(nodes, par) {
                if r["parent_tags"].as_array().into_iter().flatten().any(|t| t == &sub("Wall")) {
                    wall = true;
                }
            } else if parent_kind.is_none() {
                parent_kind = Some((parent_rel_kind(r), par));
            }
        }
    }
    match parent_kind {
        None if parent_in.is_none() => Some((class, Kind::Floor { wall })),
        None => None,
        Some((k, par)) => {
            let pc = parent_in.and_then(|q| head_of(nodes, q)).or_else(|| head_of(nodes, par)).unwrap_or_else(|| "Furniture".into());
            Some((class, Kind::Parent { rel: k, parent: pc }))
        }
    }
}

fn flatten_and(nodes: &[Value], i: usize, out: &mut Vec<usize>) {
    let n = &nodes[i];
    if n["_"] == "BoolOperatorExpression" && n["func"]["op"] == "and" {
        for o in n["operands"]["list"].as_array().into_iter().flatten() {
            if let Some(j) = refid(o) {
                flatten_and(nodes, j, out);
            }
        }
    } else {
        out.push(i);
    }
}

/// count ranges found in the program's constraints
fn entries(variant: &Value) -> Vec<Entry> {
    let nodes: &[Value] = match variant["nodes"].as_array() {
        Some(n) => n,
        None => return vec![],
    };
    let mut out = vec![];
    for (name, r) in variant["constraints"].as_object().into_iter().flatten() {
        let Some(root) = refid(r) else { continue };
        let f = &nodes[root];
        if f["_"] != "ForAll" {
            continue;
        }
        let (Some(objs), Some(pred), Some(var)) = (refid(&f["objs"]), refid(&f["pred"]), f["var"].as_str()) else { continue };
        let Some(scope) = rooms_scope(nodes, objs) else { continue };
        let mut conj = vec![];
        flatten_and(nodes, pred, &mut conj);
        for j in conj {
            let n = &nodes[j];
            if n["_"] != "in_range" {
                continue;
            }
            let (Some(lo), Some(hi), Some(val)) = (n["low"].as_f64(), n["high"].as_f64(), refid(&n["val"])) else { continue };
            if let Some((class, kind)) = analyze_count(nodes, val, var) {
                out.push(Entry { name: name.clone(), room: scope.clone(), class, kind, in_range: j, low: lo, high: hi, rooms_node: objs });
            }
        }
    }
    out
}

fn entry_matches(e: &Entry, class: &str, r: &Rel) -> bool {
    if e.class != class {
        return false;
    }
    match (&e.kind, r) {
        (Kind::Floor { .. }, Rel::OnFloor | Rel::AgainstWall) => true,
        (Kind::Parent { rel, .. }, Rel::On(_)) => *rel == "on",
        (Kind::Parent { rel, .. }, Rel::OnTop(_)) => *rel == "ontop",
        (Kind::Parent { rel, .. }, Rel::Beside(_)) => *rel == "beside",
        _ => false,
    }
}

fn entry_rule(e: &Entry) -> Rule {
    let relation = match &e.kind {
        Kind::Floor { wall: true } => "against_wall".to_string(),
        Kind::Floor { wall: false } => "on_floor".to_string(),
        Kind::Parent { rel, parent } => format!("{rel}:{parent}"),
    };
    Rule { class: e.class.clone(), relation, min: Some(e.low as u32), max: Some(e.high as u32), prefer: None, weight: None }
}

/// The built-in rules of a program variant as a [`RuleSpec`] (every count range the importer can identify: class / relation / min / max).
/// Feeding it back to [`apply`] changes nothing.
pub fn default_rules(program: &Value) -> RuleSpec {
    let mut spec = RuleSpec::default();
    let Some(v) = program["variants"].as_object().and_then(|m| m.get("00000").or_else(|| m.values().next())) else { return spec };
    let mut seen = BTreeSet::new();
    for e in entries(v) {
        let room = e.room.clone().unwrap_or_else(|| "Any".into());
        let r = entry_rule(&e);
        if seen.insert((room.clone(), r.class.clone(), r.relation.clone())) {
            spec.rooms.entry(room).or_default().rules.push(r);
        }
    }
    spec
}

/// names of all built-in constraints and score terms (valid entries of `disable`)
pub fn constraint_names(program: &Value) -> Vec<String> {
    let mut s = BTreeSet::new();
    if let Some(v) = program["variants"].as_object() {
        for var in v.values() {
            for k in ["constraints", "score_terms"] {
                for n in var[k].as_object().into_iter().flatten() {
                    s.insert(n.0.clone());
                }
            }
        }
    }
    s.into_iter().collect()
}

/// JSON for UIs: `{rooms: [...], classes: {factories: [...], tags: [...]}, relations: [...], defaults: RuleSpec, constraint_names: [...]}`
pub fn rules_info(program: &Value) -> Value {
    let v = vocab(program);
    json!({
        "rooms": ROOM_TYPES.iter().chain(["Any"].iter()).collect::<Vec<_>>(),
        "classes": {"factories": v.gens, "tags": v.sems, "furniture_factories": v.furniture_gens},
        "relations": ["on_floor", "against_wall", "on:<class>", "ontop:<class>", "beside:<class>"],
        "defaults": default_rules(program),
        "constraint_names": constraint_names(program),
    })
}

// ------------------------------------------------------------------ validation + apply

/// Errors in the spec (empty = ok).
pub fn validate(spec: &RuleSpec, program: &Value) -> Vec<String> {
    let voc = vocab(program);
    let names: BTreeSet<String> = constraint_names(program).into_iter().collect();
    let mut errs = vec![];
    for d in &spec.disable {
        if !names.contains(d) {
            errs.push(format!("disable: unknown constraint '{d}'"));
        }
    }
    for (room, rr) in &spec.rooms {
        if room != "Any" && !ROOM_TYPES.contains(&room.as_str()) {
            errs.push(format!("unknown room type '{room}' (use one of {} or Any)", ROOM_TYPES.join(", ")));
        }
        for (i, r) in rr.rules.iter().enumerate() {
            let at = format!("{room} rule {}", i + 1);
            if !voc.known(alias(&r.class)) {
                errs.push(format!("{at}: unknown class '{}'", r.class));
            }
            match parse_rel(&r.relation) {
                Err(e) => errs.push(format!("{at}: {e}")),
                Ok(Rel::On(p) | Rel::OnTop(p) | Rel::Beside(p)) => {
                    if !voc.known(&p) {
                        errs.push(format!("{at}: unknown parent class '{p}'"));
                    }
                }
                _ => {}
            }
            if let (Some(a), Some(b)) = (r.min, r.max) {
                if a > b {
                    errs.push(format!("{at}: min {a} > max {b}"));
                }
            }
            if r.prefer.is_some() && r.weight.map_or(false, |w| !(w >= 0.0)) {
                errs.push(format!("{at}: weight must be >= 0"));
            }
        }
    }
    errs
}

/// clone `root` with the nodes in `map` replaced; ancestors of replaced nodes are copied (the shared DAG stays untouched)
fn subst(b: &mut B, id: usize, map: &mut HashMap<usize, usize>) -> usize {
    if let Some(&r) = map.get(&id) {
        return r;
    }
    let mut node = b.nodes[id].clone();
    let mut changed = false;
    fn walk(v: &mut Value, f: &mut dyn FnMut(usize) -> usize) {
        match v {
            Value::Object(m) => {
                if m.len() == 1 && m.contains_key("ref") {
                    let r = m["ref"].as_u64().unwrap() as usize;
                    m.insert("ref".into(), json!(f(r)));
                } else {
                    for x in m.values_mut() {
                        walk(x, f);
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f)),
            _ => {}
        }
    }
    let mut kids = vec![];
    walk(&mut node.clone(), &mut |r| {
        kids.push(r);
        r
    });
    let mut newkids = HashMap::new();
    for k in kids {
        let n = subst(b, k, map);
        if n != k {
            changed = true;
        }
        newkids.insert(k, n);
    }
    let res = if changed {
        walk(&mut node, &mut |r| newkids[&r]);
        b.add(node)
    } else {
        id
    };
    map.insert(id, res);
    res
}

fn set_constraint(variant: &mut Value, section: &str, name: &str, id: usize) {
    variant[section].as_object_mut().unwrap().insert(name.into(), rf(id));
}

fn scope_of_root(nodes: &[Value], r: &Value) -> Option<Option<String>> {
    let n = &nodes[refid(r)?];
    if matches!(n["_"].as_str()?, "ForAll" | "MeanOver" | "SumOver") {
        rooms_scope(nodes, refid(&n["objs"])?)
    } else {
        None
    }
}

fn apply_variant(variant: &mut Value, spec: &RuleSpec, voc: &Vocab, warn: &mut Vec<String>) {
    for d in &spec.disable {
        for sec in ["constraints", "score_terms"] {
            if let Some(m) = variant[sec].as_object_mut() {
                m.shift_remove(d);
            }
        }
    }
    for (room, rr) in &spec.rooms {
        if rr.replace && room != "Any" {
            let nodes = variant["nodes"].as_array().unwrap().clone();
            for sec in ["constraints", "score_terms"] {
                let drop: Vec<String> = variant[sec].as_object().unwrap().iter().filter(|(_, r)| scope_of_root(&nodes, r) == Some(Some(room.clone()))).map(|(k, _)| k.clone()).collect();
                for k in drop {
                    variant[sec].as_object_mut().unwrap().shift_remove(&k);
                }
            }
        }
    }
    let mut memo = B::memo_of(variant["nodes"].as_array().unwrap());
    for (room, rr) in &spec.rooms {
        for (ri, rule) in rr.rules.iter().enumerate() {
            let class = alias(&rule.class).to_string();
            let Ok(r) = parse_rel(&rule.relation) else { continue };
            let n = ri + 1;
            let is_any = room == "Any";
            if rule.min.is_some() || rule.max.is_some() {
                let lo = rule.min.unwrap_or(0) as f64;
                let hi = rule.max.unwrap_or(BIG) as f64;
                let es: Vec<Entry> = entries(variant).into_iter().filter(|e| entry_matches(e, &class, &r)).collect();
                let typed: Vec<&Entry> = es.iter().filter(|e| if is_any { e.room.is_none() } else { e.room.as_deref() == Some(room) }).collect();
                let mut nodes = std::mem::take(variant["nodes"].as_array_mut().unwrap());
                {
                    let mut b = B::with_memo(&mut nodes, std::mem::take(&mut memo));
                    let mut changes: Vec<(String, usize)> = vec![];
                    // 1. override identified built-in ranges
                    for e in &typed {
                        if (e.low - lo).abs() < 1e-9 && (e.high - hi).abs() < 1e-9 {
                            continue;
                        }
                        let old = b.nodes[e.in_range].clone();
                        let mut nn = old.clone();
                        nn["low"] = json!(lo);
                        nn["high"] = json!(hi);
                        let new_in = b.add(nn);
                        let root = refid(&variant["constraints"][&e.name]).unwrap();
                        let mut map = HashMap::from([(e.in_range, new_in)]);
                        let nr = subst(&mut b, root, &mut map);
                        changes.push((e.name.clone(), nr));
                    }
                    // 2. an all-rooms built-in range that cannot hold together with the user range: exclude this room type from it
                    if !is_any {
                        for e in es.iter().filter(|e| e.room.is_none()) {
                            if lo > e.high || hi < e.low {
                                let cur = changes.iter().rev().find(|c| c.0 == e.name).map(|c| c.1).or_else(|| refid(&variant["constraints"][&e.name])).unwrap();
                                let nr_rooms = {
                                    let t = sem(room);
                                    b.tagged(e.rooms_node, vec![neg(t)])
                                };
                                let mut map = HashMap::from([(e.rooms_node, nr_rooms)]);
                                let nr = subst(&mut b, cur, &mut map);
                                changes.push((e.name.clone(), nr));
                            }
                        }
                    }
                    // 3. nothing to override: a new constraint
                    let mut added = None;
                    if typed.is_empty() {
                        let scope = b.rooms_of(room);
                        let id = b.for_all(scope, |b, item| {
                            let c = voc.counted(b, &class, &r, item);
                            let cnt = b.count(c);
                            b.in_range(cnt, lo, hi)
                        });
                        added = Some(id);
                    } else if typed.iter().all(|e| (e.low - lo).abs() < 1e-9 && (e.high - hi).abs() < 1e-9) && changes.is_empty() {
                        // unchanged built-in
                    }
                    memo = b.into_memo();
                    variant["nodes"] = Value::Array(nodes);
                    for (name, id) in changes {
                        set_constraint(variant, "constraints", &name, id);
                    }
                    if let Some(id) = added {
                        set_constraint(variant, "constraints", &format!("user_{room}_{n}"), id);
                    }
                }
                if typed.is_empty() && es.iter().any(|e| e.room.is_some() && Some(e.room.clone().unwrap()) != Some(room.clone())) && !is_any {
                    // other room types have their own ranges for this class: unaffected
                }
            }
            if let Some(p) = rule.prefer {
                let w = rule.weight.unwrap_or(1.0);
                let mut nodes = std::mem::take(variant["nodes"].as_array_mut().unwrap());
                let mut b = B::with_memo(&mut nodes, std::mem::take(&mut memo));
                let scope = b.rooms_of(room);
                let id = b.mean_over(scope, |b, item| {
                    let c = voc.counted(b, &class, &r, item);
                    let cnt = b.count(c);
                    if p == Prefer::More { b.maximize(cnt, w) } else { b.minimize(cnt, w) }
                });
                memo = b.into_memo();
                variant["nodes"] = Value::Array(nodes);
                set_constraint(variant, "score_terms", &format!("user_{room}_{n}"), id);
            }
        }
    }
    let _ = warn;
}

/// Apply `spec` to every variant of the loaded program JSON. Returns the validation errors (nothing is changed when there are any).
/// An empty spec is a no-op.
pub fn try_apply(program: &mut Value, spec: &RuleSpec) -> Result<(), Vec<String>> {
    if *spec == RuleSpec::default() {
        return Ok(());
    }
    let errs = validate(spec, program);
    if !errs.is_empty() {
        return Err(errs);
    }
    let voc = vocab(program);
    let mut warn = vec![];
    if let Some(vars) = program["variants"].as_object_mut() {
        for (_, v) in vars.iter_mut() {
            apply_variant(v, spec, &voc, &mut warn);
        }
    }
    Ok(())
}

/// Apply `spec` to the loaded program JSON (all 32 variants). Invalid specs are reported on stderr and ignored.
pub fn apply(program: &mut Value, spec: &RuleSpec) {
    if let Err(errs) = try_apply(program, spec) {
        for e in errs {
            eprintln!("RULES error: {e}");
        }
        eprintln!("RULES: spec ignored");
    }
}

/// Parse a spec from JSON text (an empty string = empty spec).
pub fn parse(text: &str) -> Result<RuleSpec, String> {
    if text.trim().is_empty() {
        return Ok(RuleSpec::default());
    }
    serde_json::from_str(text).map_err(|e| format!("rules JSON: {e}"))
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> Value {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/data/home_program_dense.json");
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// structural form of a subtree (item names and ids ignored)
    fn tree(nodes: &[Value], i: usize, memo: &mut HashMap<usize, String>) -> String {
        if let Some(s) = memo.get(&i) {
            return s.clone();
        }
        let mut n = nodes[i].clone();
        if let Some(m) = n.as_object_mut() {
            m.remove("name");
            m.remove("var");
        }
        fn go(v: &Value, nodes: &[Value], memo: &mut HashMap<usize, String>) -> String {
            match v {
                Value::Object(m) if m.len() == 1 && m.contains_key("ref") => tree(nodes, m["ref"].as_u64().unwrap() as usize, memo),
                Value::Object(m) => {
                    let mut ks: Vec<_> = m.keys().collect();
                    ks.sort();
                    format!("{{{}}}", ks.iter().map(|k| format!("{k}:{}", go(&m[*k], nodes, memo))).collect::<Vec<_>>().join(","))
                }
                Value::Array(a) => format!("[{}]", a.iter().map(|x| go(x, nodes, memo)).collect::<Vec<_>>().join(",")),
                Value::Number(n) => format!("{}", n.as_f64().unwrap()),
                o => o.to_string(),
            }
        }
        let s = go(&n, nodes, memo);
        memo.insert(i, s.clone());
        s
    }

    #[test]
    fn presets_exist_in_program() {
        let p = load();
        let nodes = p["variants"]["00000"]["nodes"].as_array().unwrap();
        let set: BTreeSet<String> = nodes.iter().filter(|n| n["_"] == "related_to").map(|n| canon(&n["relation"])).collect();
        for (n, r) in [("on_floor", rel::on_floor()), ("against_wall", rel::against_wall()), ("ontop", rel::ontop()), ("on", rel::on()), ("lr", rel::leftright_leftright()), ("any", rel::any())] {
            assert!(set.contains(&canon(&r)), "preset {n} not found in the python export");
        }
    }

    /// rebuild python's extras with the builder and compare the subtree structurally with the python export
    #[test]
    fn builder_matches_python() {
        let mut p = load();
        let voc = vocab(&p);
        let v = &mut p["variants"]["00000"];
        let mut nodes = v["nodes"].as_array().unwrap().clone();
        let orig = nodes.clone();
        let find = |name: &str, idx: usize| -> usize {
            let root = refid(&v["constraints"][name]).unwrap();
            let pred = refid(&orig[root]["pred"]).unwrap();
            let mut c = vec![];
            flatten_and(&orig, pred, &mut c);
            c[idx]
        };
        let mut memo = HashMap::new();
        let mut b = B::new(&mut nodes);
        let bed = b.rooms_of("Bedroom");
        let cases: Vec<(&str, usize, &str, &str, f64, f64)> = vec![
            ("bedroom", 0, "Bed", "against_wall", 1.0, 2.0),
            ("bedroom", 1, "SideTableFactory", "beside:Bed", 1.0, 2.0),
            ("bedroom", 2, "RugFactory", "on_floor", 0.0, 1.0),
            ("bedroom", 3, "SimpleDeskFactory", "against_wall", 0.0, 1.0),
            ("bedroom", 5, "FloorLampFactory", "against_wall", 0.0, 1.0),
        ];
        for (cname, idx, class, relation, lo, hi) in cases {
            let want = find(cname, idx);
            let r = parse_rel(relation).unwrap();
            let got = b.in_range(0, 0.0, 0.0); // placeholder to keep the builder borrowed
            let _ = got;
            let ex = b.for_all(bed, |b, item| {
                let c = voc.counted(b, class, &r, item);
                let cnt = b.count(c);
                b.in_range(cnt, lo, hi)
            });
            let pred = refid(&b.nodes[ex]["pred"]).unwrap();
            let a = tree(b.nodes, pred, &mut memo);
            let w = tree(&orig, want, &mut HashMap::new());
            assert_eq!(a, w, "{cname}[{idx}] {class} {relation}");
        }
        // score term: bedroom_dense = bedrooms.mean(sidetables.count().maximize(3))
        let want = refid(&v["score_terms"]["bedroom_dense"]).unwrap();
        let r = parse_rel("beside:Bed").unwrap();
        let ex = b.mean_over(bed, |b, item| {
            let c = voc.counted(b, "SideTableFactory", &r, item);
            let cnt = b.count(c);
            b.maximize(cnt, 3.0)
        });
        assert_eq!(tree(b.nodes, ex, &mut memo), tree(&orig, want, &mut HashMap::new()));
        // whole constraint: kitchen_dense = kitchens.all(wallfurn[KitchenCabinetFactory].related_to(r).count().in_range(0, 2))
        let want = refid(&v["constraints"]["kitchen_dense"]).unwrap();
        let k = b.rooms_of("Kitchen");
        let r = parse_rel("against_wall").unwrap();
        let ex = b.for_all(k, |b, item| {
            let c = voc.counted(b, "KitchenCabinetFactory", &r, item);
            let cnt = b.count(c);
            b.in_range(cnt, 0.0, 2.0)
        });
        assert_eq!(tree(b.nodes, ex, &mut memo), tree(&orig, want, &mut HashMap::new()));
    }

    #[test]
    fn defaults_roundtrip_is_noop() {
        let mut p = load();
        let d = default_rules(&p);
        assert!(d.rooms.contains_key("LivingRoom"));
        eprintln!("{}", serde_json::to_string_pretty(&d).unwrap());
        let before = p.clone();
        try_apply(&mut p, &d).unwrap();
        assert_eq!(before, p, "applying the exported defaults must not change the program");
    }

    #[test]
    fn override_changes_range() {
        let mut p = load();
        let spec = parse(r#"{"rooms":{"LivingRoom":{"rules":[{"class":"SofaFactory","min":2,"max":3},{"class":"ArmChairFactory","max":0}]},
            "Bedroom":{"rules":[{"class":"BedFactory","min":2,"max":2}]}, "Kitchen":{"rules":[{"class":"KitchenIslandFactory","max":0}]}}}"#).unwrap();
        try_apply(&mut p, &spec).unwrap();
        let d = default_rules(&p);
        eprintln!("{}", serde_json::to_string_pretty(&d).unwrap());
        let lr = &d.rooms["LivingRoom"].rules;
        let sofa = lr.iter().find(|r| r.class == "SofaFactory").unwrap();
        assert_eq!((sofa.min, sofa.max), (Some(2), Some(3)));
        let arm = lr.iter().find(|r| r.class == "ArmChairFactory").unwrap();
        assert_eq!((arm.min, arm.max), (Some(0), Some(0)));
        // the program still loads
        let params: ig_furn::program::Params = ["furniture_fullness_pct", "obj_interior_obj_pct", "obj_on_storage_pct", "obj_on_nonstorage_pct", "painting_area_per_room_area"].iter().map(|k| (k.to_string(), 0.7)).chain([("sofa_back_margin".to_string(), 0.2), ("sofa_side_margin".to_string(), 0.2)]).collect();
        let prog = ig_furn::program::Program::load(&p, "00000", &params);
        assert!(prog.constraint_bounds().len() > 10);
    }
}

/// Score term "feature_focus" (room features on): in living rooms, sofas (weight 4) and armchairs (weight 3) are rewarded for how directly
/// they face the room's fireplace (Infinigen's focus_score, as its armchairs face the coffee table); rooms without a fireplace score 0.
pub fn add_feature_scores(program: &mut Value) {
    let Some(vars) = program.get_mut("variants").and_then(|v| v.as_object_mut()) else { return };
    for (_, var) in vars.iter_mut() {
        let mut nodes = std::mem::take(var["nodes"].as_array_mut().unwrap());
        let term = {
            let mut b = B::new(&mut nodes);
            let living = b.rooms_of("LivingRoom");
            b.mean_over(living, |b, r| {
                let o = b.objects();
                let fp = b.tagged(o, vec![json!({"gen": "FireplaceFeature"})]);
                let fp_in = b.related_to(fp, r, rel::any());
                let mut parts = vec![];
                for (g, w) in [("SofaFactory", 4.0), ("ArmChairFactory", 3.0)] {
                    let f = b.furniture();
                    let c = b.tagged(f, vec![json!({"gen": g})]);
                    let c_in = b.related_to(c, r, rel::any());
                    // a reward per seat (1 - focus_score: 1 facing, 0 facing away): a seat facing something else just earns
                    // nothing, so neither a sum nor a mean of penalties tempts the solver to delete seats
                    let m = b.sum_over(c_in, |b, o| {
                        let fs = b.add(json!({"_": "focus_score", "objs": rf(o), "others": rf(fp_in)}));
                        let one = b.constant(1.0);
                        b.scalar("sub", &[one, fs])
                    });
                    parts.push(b.maximize(m, w));
                }
                b.scalar("add", &parts)
            })
        };
        var["nodes"] = Value::Array(nodes);
        if let Some(st) = var.get_mut("score_terms").and_then(|v| v.as_object_mut()) {
            st.insert("feature_focus".into(), json!({"ref": term}));
        }
    }
}
