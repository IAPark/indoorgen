//! Material rules: a data file (rules/styles.json, built in; MATERIAL_RULES=path.json replaces it, MATERIAL_RULES=none turns it off) that scores materials and sets colours. Every material of a scene passes through
//! `resolve` when the sidecar is written, so a rule reaches any object without touching the code that made it, and `reshade` re-applies a file to a generated
//! house in a fraction of a second. With no file, or no rule that matches, a material stays exactly as its factory drew it.
//!
//! A rule: { "when": {"slot", "class", "object_class", "room", "style"}, "why", then any of:
//!   "set": {class: weight}   the candidates, replacing what the factory drew
//!   "add": {class: weight}   more candidates beside it
//!   "weight": x              multiply the candidates whose class matches when.class (0 rules them out)
//!   "color": {"h": [a, b], "s": [a, b], "v": [a, b]} or {"palette": ["#rrggbb" or [r, g, b] linear, ...]}, with "per": "object" (default) | "room" | "house",
//!            and "key": a name: rules with one key share their draw across slots, so same-length palettes stay paired (a sofa and its curtains)
//!   "paint": a colour like "color" plus "amount": [a, b], mixed over the material with its texture kept; reaches classes that ignore "color" (wood stain, painted trim) }
//! `when` fields are patterns: * wildcards, | alternatives. The slot is the material's name before any ':' or '~' (table_top, sheet, wall).
//! Variety inside a style: "chance" on a rule: 0.3, or {"p": 0.3, "per": "house"|"room"|"object", "key": a name rules share} (default per house), so houses use different
//! parts of a recipe. A colour's "per" can be odds, {"house": 3, "room": 1}, drawn per house. "stray": 0.15 with "stray_palette": that share of objects leave the scheme
//! (no palette: they keep the colour they had). "jitter": {"h", "s", "v"} moves a drawn colour by up to that much (h in turns, s and v as a fraction); the file's own "jitter" is the default for palettes.
//! The file: { "jitter", "groups": {name: when}, "styles": {name: {"weight", "why", "rules": [...]}}, "rules": [...] }. A house draws one style by weight; a style's rules
//! apply to its houses only, after the file's own. `when` can name a group ("group": "wood"), whose fields fill the ones it leaves out.
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct When { pub group: Option<String>, pub slot: Option<String>, pub class: Option<String>, pub object_class: Option<String>, pub room: Option<String>, pub style: Option<String> }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Color {
    #[serde(default)] pub h: Option<[f64; 2]>, #[serde(default)] pub s: Option<[f64; 2]>, #[serde(default)] pub v: Option<[f64; 2]>,
    #[serde(default)] pub palette: Option<Vec<Swatch>>,
    #[serde(default)] pub per: Option<Per>,
    #[serde(default)] pub key: Option<String>,
    #[serde(default)] pub stray: Option<f64>,
    #[serde(default)] pub stray_palette: Option<Vec<Swatch>>,
    #[serde(default)] pub jitter: Option<Jitter>,
    #[serde(default)] pub amount: Option<[f64; 2]>,
}

/// how widely one draw is shared: one scope, or odds over scopes drawn per house
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Per { One(String), Odds(BTreeMap<String, f64>) }
impl Per { fn scopes(&self) -> Vec<&str> { match self { Per::One(s) => vec![s.as_str()], Per::Odds(m) => m.keys().map(|k| k.as_str()).collect() } } }

#[derive(Deserialize, Default, Clone, Copy, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Jitter { pub h: f64, pub s: f64, pub v: f64 }

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Chance { P(f64), Full { p: f64, #[serde(default)] per: Option<String>, #[serde(default)] key: Option<String> } }

/// a palette entry: "#rrggbb" (sRGB, as a paint chart gives it) or [r, g, b] linear
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Swatch { Rgb([f64; 3]), Hex(String) }
impl Swatch {
    fn linear(&self) -> Option<[f64; 3]> {
        match self {
            Swatch::Rgb(c) => Some(*c),
            Swatch::Hex(h) => { let v = u32::from_str_radix(h.strip_prefix('#')?, 16).ok().filter(|_| h.len() == 7)?; let d = |x: u32| { let c = (x & 255) as f64 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } }; Some([d(v >> 16), d(v >> 8), d(v)]) }
        }
    }
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Rule {
    pub when: When, pub why: String,
    pub set: Option<BTreeMap<String, f64>>, pub add: Option<BTreeMap<String, f64>>, pub weight: Option<f64>, pub color: Option<Color>, pub paint: Option<Color>, pub chance: Option<Chance>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Style { pub weight: f64, pub why: String, pub rules: Vec<Rule>, pub params: BTreeMap<String, f64> }

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Config { pub params: BTreeMap<String, f64>, pub jitter: Option<Jitter>, pub groups: BTreeMap<String, When>, pub styles: BTreeMap<String, Style>, pub rules: Vec<Rule> }

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // a style's rules join the list, marked with their style; groups fill in what a `when` leaves out
        for (name, st) in &mut c.styles { for mut r in std::mem::take(&mut st.rules) { if r.when.style.is_some() { return Err(format!("rule ({}) in style {name} names a style", r.why)); } r.when.style = Some(name.clone()); c.rules.push(r); } }
        for r in &mut c.rules {
            let Some(g) = r.when.group.take() else { continue };
            let f = c.groups.get(&g).ok_or_else(|| format!("rule ({}) names an unknown group {g}", r.why))?;
            r.when.slot = r.when.slot.take().or_else(|| f.slot.clone()); r.when.class = r.when.class.take().or_else(|| f.class.clone());
            r.when.object_class = r.when.object_class.take().or_else(|| f.object_class.clone()); r.when.room = r.when.room.take().or_else(|| f.room.clone());
        }
        for (i, r) in c.rules.iter().enumerate() {
            if r.set.is_none() && r.add.is_none() && r.weight.is_none() && r.color.is_none() && r.paint.is_none() { return Err(format!("rule {i} ({}) does nothing", r.why)); }
            if r.why.is_empty() { return Err(format!("rule {i} has no \"why\"")); }
            for c in [&r.color, &r.paint].into_iter().flatten() {
                if let Some(p) = &c.palette { for x in p { if x.linear().is_none() { return Err(format!("rule {i} ({}): bad colour {x:?}", r.why)); } } }
                for x in c.stray_palette.iter().flatten() { if x.linear().is_none() { return Err(format!("rule {i} ({}): bad colour {x:?}", r.why)); } }
                if let Some(per) = &c.per { for x in per.scopes() { if !matches!(x, "object" | "room" | "house") { return Err(format!("rule {i} ({}): per {x}", r.why)); } } }
            }
            match &r.chance {
                Some(Chance::P(p)) | Some(Chance::Full { p, .. }) if !(0.0..=1.0).contains(p) => return Err(format!("rule {i} ({}): chance {p}", r.why)),
                Some(Chance::Full { per: Some(x), .. }) if !matches!(x.as_str(), "object" | "room" | "house") => return Err(format!("rule {i} ({}): chance per {x}", r.why)),
                _ => {}
            }
            if r.paint.as_ref().map_or(false, |c| c.amount.is_none()) { return Err(format!("rule {i} ({}): paint needs an amount", r.why)); }
            if let Some(s) = &r.when.style { if !s.contains('*') && !s.split('|').all(|x| c.styles.contains_key(x)) { return Err(format!("rule {i} names an unknown style {s}")); } }
        }
        Ok(c)
    }
    /// a number decided per style: its "params", else the file's, else the default
    pub fn param(&self, house: u64, key: &str, default: f64) -> f64 {
        self.style_of(house).and_then(|s| self.styles.get(s)).and_then(|s| s.params.get(key)).or_else(|| self.params.get(key)).copied().unwrap_or(default)
    }
    /// the house's style: a draw over the styles' weights
    pub fn style_of(&self, house: u64) -> Option<&str> {
        let total: f64 = self.styles.values().map(|s| s.weight.max(0.0)).sum();
        if total <= 0.0 { return None }
        let mut u = unit(&[&house.to_string(), "style"]) * total;
        for (k, s) in &self.styles { let w = s.weight.max(0.0); if u < w { return Some(k) } u -= w; }
        self.styles.keys().next_back().map(|k| k.as_str())
    }
}

/// rules/styles.json, or the file named by MATERIAL_RULES ("none": no rules); a bad file stops the run rather than quietly doing nothing
pub fn loaded() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| { let mut c = match std::env::var("MATERIAL_RULES").as_deref() {
        Ok("none") => Config::default(),
        Ok(p) => { let t = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("MATERIAL_RULES {p}: {e}")); Config::parse(&t).unwrap_or_else(|e| panic!("MATERIAL_RULES {p}: {e}")) }
        Err(_) => Config::parse(include_str!("../../../rules/styles.json")).expect("rules/styles.json"),
    };
    // (FORCE_STYLE=name: every house in that style, to review one style's geometry)
    if let Ok(st) = std::env::var("FORCE_STYLE") { assert!(c.styles.contains_key(&st), "FORCE_STYLE {st}"); for (k, v) in c.styles.iter_mut() { v.weight = if *k == st { 1.0 } else { 0.0 }; } }
    c })
}

fn unit(parts: &[&str]) -> f64 {
    let mut h = crc32fast::Hasher::new();
    for p in parts { h.update(p.as_bytes()); h.update(&[0]); }
    // (a second round: crc alone is linear, and near keys gave near draws)
    fastrand::Rng::with_seed(h.finalize() as u64).f64()
}

/// * wildcards, | alternatives
pub fn matches(pat: &str, s: &str) -> bool {
    pat.split('|').any(|p| {
        let parts: Vec<&str> = p.split('*').collect();
        if parts.len() == 1 { return p == s }
        let mut rest = s;
        for (i, q) in parts.iter().enumerate() {
            if i == 0 { match rest.strip_prefix(q) { Some(r) => rest = r, None => return false } }
            else if i + 1 == parts.len() { return rest.ends_with(q) }
            else { match rest.find(q) { Some(k) => rest = &rest[k + q.len()..], None => return false } }
        }
        true
    })
}

pub fn slot_of(mat: &str) -> &str { mat.split(':').next().unwrap_or("").split('~').next().unwrap_or("") }

const ROOMS: [&str; 12] = ["living-room", "dining-room", "bedroom", "bathroom", "kitchen", "hallway", "closet", "utility", "garage", "balcony", "office", "restroom"];
pub fn room_of(node: &str) -> &str { ROOMS.iter().find(|r| node.contains(**r)).copied().unwrap_or("") }
/// which room of its kind: bedroom_1, from bedroom_1/0.wall or bedroom_1_0_stand#2
fn room_id(node: &str) -> String {
    let room = room_of(node);
    let Some(i) = (!room.is_empty()).then(|| node.find(room)).flatten() else { return String::new() };
    let rest = &node[i + room.len()..];
    format!("{room}{}", rest.strip_prefix('_').map_or(String::new(), |r| format!("_{}", r.split(|c: char| !c.is_ascii_digit()).next().unwrap_or(""))))
}

/// what `resolve` changed, for `reshade --explain`
#[derive(Debug, Clone)]
pub struct Trace { pub rules: Vec<String>, pub odds: Vec<(String, f64)> }

/// the rules applied to one part: its spec and plain colour, unchanged if nothing matches. `house` chooses the style and what "per": "house" shares.
pub fn resolve(cfg: &Config, house: u64, spec: &Value, mat: &str, node: &str, object_class: &str, rgb: [f32; 3]) -> (Value, [f32; 3], Trace) {
    resolve_drawn(cfg, house, house, spec, mat, node, object_class, rgb)
}

/// `resolve` with the rules' draws from `draw` while the style stays the house's: another finish of the same house (an era repainted)
pub fn resolve_drawn(cfg: &Config, house: u64, draw_seed: u64, spec: &Value, mat: &str, node: &str, object_class: &str, rgb: [f32; 3]) -> (Value, [f32; 3], Trace) {
    let mut trace = Trace { rules: vec![], odds: vec![] };
    let Some(cls0) = spec.get("cls").and_then(|v| v.as_str()) else { return (spec.clone(), rgb, trace) };
    if cfg.rules.is_empty() { return (spec.clone(), rgb, trace) }
    let (slot, room, object) = (slot_of(mat), room_of(node), node.split('#').next().unwrap_or(node));
    let style = cfg.style_of(house);
    let applies = |w: &When, cls: Option<&str>| -> bool {
        w.slot.as_ref().map_or(true, |p| matches(p, slot)) && w.object_class.as_ref().map_or(true, |p| matches(p, object_class))
            && w.room.as_ref().map_or(true, |p| matches(p, room)) && w.style.as_ref().map_or(true, |p| style.map_or(false, |s| matches(p, s)))
            && cls.map_or(true, |c| w.class.as_ref().map_or(true, |p| matches(p, c)))
    };
    let scope = |per: &str| match per { "house" => String::new(), "room" => room_id(node), _ => object.to_string() };
    // (a rule's chance: one draw per house, room or object, so the class and colour halves of a rule agree)
    let active = |r: &Rule| match &r.chance {
        None => true,
        Some(Chance::P(p)) => unit(&[&draw_seed.to_string(), "chance", &r.why]) < *p,
        Some(Chance::Full { p, per, key }) => unit(&[&draw_seed.to_string(), "chance", key.as_deref().unwrap_or(&r.why), &scope(per.as_deref().unwrap_or("house"))]) < *p,
    };
    let mut cands: Vec<(String, f64)> = vec![(cls0.to_string(), 1.0)];
    let mut touched = false;
    for r in &cfg.rules {
        if !applies(&r.when, None) || !active(r) { continue }
        if let Some(set) = &r.set { if r.when.class.as_ref().map_or(true, |p| matches(p, cls0)) { cands = set.iter().map(|(k, v)| (k.clone(), *v)).collect(); touched = true; trace.rules.push(r.why.clone()); } }
        if let Some(add) = &r.add { if r.when.class.as_ref().map_or(true, |p| matches(p, cls0)) { for (k, v) in add { match cands.iter_mut().find(|c| c.0 == *k) { Some(c) => c.1 += v, None => cands.push((k.clone(), *v)) } } touched = true; trace.rules.push(r.why.clone()); } }
        if let Some(w) = r.weight { for c in cands.iter_mut() { if r.when.class.as_ref().map_or(true, |p| matches(p, &c.0)) { c.1 *= w; touched = true; trace.rules.push(r.why.clone()); } } }
    }
    trace.rules.dedup();
    let total: f64 = cands.iter().map(|c| c.1.max(0.0)).sum();
    trace.odds = cands.iter().map(|c| (c.0.clone(), if total > 0.0 { c.1.max(0.0) / total } else { 0.0 })).collect();
    // (one draw per slot and object: the parts of a piece stay one material)
    let mut cls = cls0.to_string();
    if touched && total > 0.0 {
        let mut u = unit(&[&draw_seed.to_string(), "class", slot, object]) * total;
        for (k, w) in &cands { let w = w.max(0.0); if u < w { cls = k.clone(); break } u -= w; }
    }
    let mut out = spec.clone();
    out["cls"] = Value::String(cls.clone());
    let mut col = rgb;
    let name = |c: &Color, what: &str| c.key.clone().unwrap_or_else(|| format!("{slot}/{what}"));
    let per_of = |c: &Color, what: &str| -> String {
        match &c.per {
            None => scope("object"),
            Some(Per::One(x)) => scope(x),
            Some(Per::Odds(m)) => {
                let mut u = unit(&[&draw_seed.to_string(), &name(c, what), "per"]) * m.values().map(|w| w.max(0.0)).sum::<f64>();
                for (k, w) in m { if u < w.max(0.0) { return scope(k) } u -= w.max(0.0); }
                scope(m.keys().next_back().map_or("object", |k| k.as_str()))
            }
        }
    };
    let pick = |p: &[Swatch], parts: &[&str]| p[((unit(parts) * p.len() as f64) as usize).min(p.len() - 1)].linear().unwrap_or([0.5; 3]);
    let draw = |c: &Color, what: &str, base: [f32; 3]| -> [f32; 3] {
        let h = draw_seed.to_string();
        // (a stray piece leaves the scheme: it takes the stray palette, or keeps the colour it had)
        if c.stray.map_or(false, |p| unit(&[&h, "stray", slot, object, what]) < p) {
            return match c.stray_palette.as_ref().filter(|p| !p.is_empty()) {
                Some(p) => { let v = pick(p, &[&h, "strayc", slot, object, what]); [v[0] as f32, v[1] as f32, v[2] as f32] }
                None => base,
            };
        }
        let (k, per) = (name(c, what), per_of(c, what));
        let d = |w: &str, a: [f64; 2]| a[0] + (a[1] - a[0]) * unit(&[&h, &k, &per, w]);
        let palette = c.palette.as_ref().filter(|p| !p.is_empty());
        let mut v = if let Some(p) = palette { pick(p, &[&h, &k, &per, "palette"]) }
            else { let hsv0 = crate::rgb_to_hsv([base[0] as f64, base[1] as f64, base[2] as f64]); crate::hsv(c.h.map_or(hsv0[0], |a| d("h", a)), c.s.map_or(hsv0[1], |a| d("s", a)), c.v.map_or(hsv0[2], |a| d("v", a))) };
        if let Some(j) = c.jitter.or(if palette.is_some() { cfg.jitter } else { None }) {
            let q = crate::rgb_to_hsv(v);
            let t = |w: &str, a: f64| d(w, [-a, a]);
            v = crate::hsv(q[0] + t("jh", j.h), (q[1] * (1.0 + t("js", j.s))).clamp(0.0, 1.0), (q[2] * (1.0 + t("jv", j.v))).clamp(0.0, 1.0));
        }
        [v[0] as f32, v[1] as f32, v[2] as f32]
    };
    for r in &cfg.rules {
        if !applies(&r.when, Some(&cls)) || !active(r) { continue }
        if let Some(c) = &r.color { col = draw(c, "color", col); trace.rules.push(r.why.clone()); }
        if let Some(c) = &r.paint {
            let rgb = draw(c, "paint", col);
            let a = c.amount.unwrap_or([1.0, 1.0]);
            let amount = a[0] + (a[1] - a[0]) * unit(&[&draw_seed.to_string(), slot, &per_of(c, "paint"), "amount"]);
            out["paint"] = serde_json::json!({"rgb": rgb, "amount": amount});
            trace.rules.push(r.why.clone());
        }
    }
    (out, col, trace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn cfg(t: &str) -> Config { Config::parse(t).unwrap() }
    #[test] fn nothing_matches_nothing_changes() {
        let c = cfg(r#"{"rules":[{"when":{"slot":"sofa_fabric"},"why":"x","set":{"fabric.Leather":1}}]}"#);
        let s = json!({"cls":"wood.Wood","seed":3});
        let (o, rgb, _) = resolve(&c, 1, &s, "table_top~2", "bedroom_0_0_stand#0", "SideTableFactory", [0.5, 0.4, 0.3]);
        assert_eq!(o, s); assert_eq!(rgb, [0.5, 0.4, 0.3]);
    }
    #[test] fn weight_zero_rules_a_class_out() {
        let c = cfg(r#"{"rules":[{"when":{"slot":"pot"},"why":"x","add":{"ceramic.Ceramic":1}},{"when":{"slot":"pot","class":"*Glass*"},"why":"y","weight":0}]}"#);
        let (o, _, _) = resolve(&c, 1, &json!({"cls":"ceramic.GlassVolume","seed":3}), "pot:ceramic.GlassVolume", "kitchen_0_0_plant#0", "PlantContainer", [1.0; 3]);
        assert_eq!(o["cls"], "ceramic.Ceramic");
    }
    #[test] fn parts_of_a_piece_agree_and_objects_differ() {
        let c = cfg(r#"{"rules":[{"when":{"slot":"leg"},"why":"x","set":{"a":1,"b":1,"c":1,"d":1,"e":1,"f":1}}]}"#);
        let f = |n: &str| resolve(&c, 7, &json!({"cls":"z"}), "leg", n, "", [1.0; 3]).0["cls"].clone();
        assert_eq!(f("t_a#0"), f("t_a#3"));
        assert!((0..30).map(|i| f(&format!("t_{i}#0"))).collect::<Vec<_>>().windows(2).any(|w| w[0] != w[1]));
    }
    #[test] fn bad_files_are_refused() {
        assert!(Config::parse(r#"{"rules":[{"when":{},"why":"x"}]}"#).is_err());
        assert!(Config::parse(r#"{"rules":[{"when":{"slott":"a"},"why":"x","weight":1}]}"#).is_err());
        assert!(Config::parse(r#"{"rules":[{"when":{"style":"nope"},"why":"x","weight":1}]}"#).is_err());
        assert!(Config::parse(r#"{"rules":[{"when":{"group":"nope"},"why":"x","weight":1}]}"#).is_err());
    }
    #[test] fn keys_pair_palettes_and_rooms_differ() {
        let c = cfg(r##"{"rules":[{"when":{"slot":"a"},"why":"x","color":{"palette":["#000000","#ffffff"],"per":"house","key":"k"}},{"when":{"slot":"b"},"why":"x","color":{"palette":["#000000","#ffffff"],"per":"house","key":"k"}}]}"##);
        for h in 0..20 { let f = |m: &str| resolve(&c, h, &json!({"cls":"z"}), m, "n", "", [1.0; 3]).1; assert_eq!(f("a"), f("b")); }
        assert_eq!(room_id("bedroom_1/0.wall"), "bedroom_1"); assert_eq!(room_id("bedroom_1_0_stand#2"), "bedroom_1"); assert_eq!(room_id("skirting"), "");
    }
    #[test] fn paint_keeps_the_class() {
        let c = cfg(r##"{"rules":[{"when":{"slot":"t"},"why":"x","paint":{"palette":["#ffffff"],"amount":[0.5,0.5]}}]}"##);
        let (o, _, _) = resolve(&c, 1, &json!({"cls":"wood.Wood"}), "t", "n", "", [1.0; 3]);
        assert_eq!(o["cls"], "wood.Wood"); assert_eq!(o["paint"]["amount"], 0.5); assert_eq!(o["paint"]["rgb"][0], 1.0);
    }
    #[test] fn style_rules_and_groups() {
        let c = cfg(r##"{"groups":{"g":{"slot":"a"}},"styles":{"s":{"weight":1,"why":"w","rules":[{"when":{"group":"g"},"why":"x","color":{"palette":["#000000"]}}]}}}"##);
        assert_eq!(resolve(&c, 1, &json!({"cls":"z"}), "a", "n", "", [1.0; 3]).1, [0.0; 3]);
        assert_eq!(resolve(&c, 1, &json!({"cls":"z"}), "b", "n", "", [1.0; 3]).1, [1.0; 3]);
    }
    #[test] fn chance_and_stray() {
        let c = cfg(r##"{"rules":[{"when":{"slot":"a"},"why":"x","chance":0.5,"color":{"palette":["#000000"]}}]}"##);
        let on = (0..200).filter(|h| resolve(&c, *h, &json!({"cls":"z"}), "a", "n", "", [1.0; 3]).1 == [0.0; 3]).count();
        assert!((60..140).contains(&on), "{on}");
        let c = cfg(r##"{"rules":[{"when":{"slot":"a"},"why":"x","color":{"palette":["#000000"],"per":"house","stray":0.5}}]}"##);
        let kept = (0..200).filter(|i| resolve(&c, 1, &json!({"cls":"z"}), "a", &format!("o{i}#0"), "", [1.0; 3]).1 == [1.0; 3]).count();
        assert!((60..140).contains(&kept), "{kept}");
        // parts of one piece stray together
        assert_eq!(resolve(&c, 1, &json!({"cls":"z"}), "a", "o7#0", "", [1.0; 3]).1, resolve(&c, 1, &json!({"cls":"z"}), "a", "o7#3", "", [1.0; 3]).1);
    }
    #[test] fn per_odds_and_jitter_with_shared_keys() {
        let c = cfg(r##"{"jitter":{"h":0.02,"s":0.1,"v":0.1},"rules":[{"when":{"slot":"a|b"},"why":"x","color":{"palette":["#804020"],"per":{"house":1,"room":1},"key":"k"}}]}"##);
        for h in 0..20 { let f = |m: &str| resolve(&c, h, &json!({"cls":"z"}), m, "bedroom_0_0_x#0", "", [1.0; 3]).1; assert_eq!(f("a"), f("b")); }
        let v: Vec<_> = (0..20).map(|h| resolve(&c, h, &json!({"cls":"z"}), "a", "n", "", [1.0; 3]).1[0]).collect();
        assert!(v.windows(2).any(|w| w[0] != w[1]));
        assert!(Config::parse(r##"{"rules":[{"when":{},"why":"x","chance":2,"weight":1}]}"##).is_err());
    }
    #[test] fn the_shipped_file_parses() { assert!(Config::parse(include_str!("../../../rules/styles.json")).is_ok()); }
    #[test] fn patterns() {
        assert!(matches("*Glass*", "ceramic.GlassVolume")); assert!(matches("wood.*|metal.*", "metal.Basic")); assert!(!matches("wood.*", "ceramic.Wood")); assert!(matches("a*c", "abc"));
    }
}
