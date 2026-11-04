//! Material rules: a data file (MATERIAL_RULES=path.json) that scores materials and sets colours, read at run time. Every material of a scene passes through
//! `resolve` when the sidecar is written, so a rule reaches any object without touching the code that made it, and `reshade` re-applies a file to a generated
//! house in a fraction of a second. With no file, or no rule that matches, a material stays exactly as its factory drew it.
//!
//! A rule: { "when": {"slot", "class", "object_class", "room", "style"}, "why", then any of:
//!   "set": {class: weight}   the candidates, replacing what the factory drew
//!   "add": {class: weight}   more candidates beside it
//!   "weight": x              multiply the candidates whose class matches when.class (0 rules them out)
//!   "color": {"h": [a, b], "s": [a, b], "v": [a, b]} or {"palette": [[r, g, b], ...]}, with "per": "object" (default) | "room" | "house" }
//! `when` fields are patterns: * wildcards, | alternatives. The slot is the material's name before any ':' or '~' (table_top, sheet, wall).
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct When { pub slot: Option<String>, pub class: Option<String>, pub object_class: Option<String>, pub room: Option<String>, pub style: Option<String> }

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Color {
    #[serde(default)] pub h: Option<[f64; 2]>, #[serde(default)] pub s: Option<[f64; 2]>, #[serde(default)] pub v: Option<[f64; 2]>,
    #[serde(default)] pub palette: Option<Vec<[f64; 3]>>,
    #[serde(default)] pub per: Option<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Rule {
    pub when: When, pub why: String,
    pub set: Option<BTreeMap<String, f64>>, pub add: Option<BTreeMap<String, f64>>, pub weight: Option<f64>, pub color: Option<Color>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Style { pub weight: f64, pub why: String }

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Config { pub styles: BTreeMap<String, Style>, pub rules: Vec<Rule> }

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let c: Config = serde_json::from_str(text).map_err(|e| e.to_string())?;
        for (i, r) in c.rules.iter().enumerate() {
            if r.set.is_none() && r.add.is_none() && r.weight.is_none() && r.color.is_none() { return Err(format!("rule {i} ({}) does nothing", r.why)); }
            if r.why.is_empty() { return Err(format!("rule {i} has no \"why\"")); }
            if let Some(s) = &r.when.style { if !s.contains('*') && !s.split('|').all(|x| c.styles.contains_key(x)) { return Err(format!("rule {i} names an unknown style {s}")); } }
        }
        Ok(c)
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

/// the file named by MATERIAL_RULES, once; a bad file stops the run rather than quietly doing nothing
pub fn loaded() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| match std::env::var("MATERIAL_RULES") {
        Ok(p) => { let t = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("MATERIAL_RULES {p}: {e}")); Config::parse(&t).unwrap_or_else(|e| panic!("MATERIAL_RULES {p}: {e}")) }
        Err(_) => Config::default(),
    })
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

/// what `resolve` changed, for `reshade --explain`
#[derive(Debug, Clone)]
pub struct Trace { pub rules: Vec<String>, pub odds: Vec<(String, f64)> }

/// the rules applied to one part: its spec and plain colour, unchanged if nothing matches. `house` chooses the style and what "per": "house" shares.
pub fn resolve(cfg: &Config, house: u64, spec: &Value, mat: &str, node: &str, object_class: &str, rgb: [f32; 3]) -> (Value, [f32; 3], Trace) {
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
    let mut cands: Vec<(String, f64)> = vec![(cls0.to_string(), 1.0)];
    let mut touched = false;
    for r in &cfg.rules {
        if !applies(&r.when, None) { continue }
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
        let mut u = unit(&[&house.to_string(), "class", slot, object]) * total;
        for (k, w) in &cands { let w = w.max(0.0); if u < w { cls = k.clone(); break } u -= w; }
    }
    let mut out = spec.clone();
    out["cls"] = Value::String(cls.clone());
    let mut col = rgb;
    for r in &cfg.rules {
        let Some(c) = &r.color else { continue };
        if !applies(&r.when, Some(&cls)) { continue }
        let per = match c.per.as_deref() { Some("house") => String::new(), Some("room") => room.to_string(), _ => object.to_string() };
        let key = [house.to_string(), slot.to_string(), per];
        let d = |what: &str, a: [f64; 2]| a[0] + (a[1] - a[0]) * unit(&[&key[0], &key[1], &key[2], what]);
        let v = if let Some(p) = c.palette.as_ref().filter(|p| !p.is_empty()) { p[((unit(&[&key[0], &key[1], &key[2], "palette"]) * p.len() as f64) as usize).min(p.len() - 1)] }
            else { let hsv0 = crate::rgb_to_hsv([rgb[0] as f64, rgb[1] as f64, rgb[2] as f64]); crate::hsv(c.h.map_or(hsv0[0], |a| d("h", a)), c.s.map_or(hsv0[1], |a| d("s", a)), c.v.map_or(hsv0[2], |a| d("v", a))) };
        col = [v[0] as f32, v[1] as f32, v[2] as f32];
        trace.rules.push(r.why.clone());
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
    }
    #[test] fn patterns() {
        assert!(matches("*Glass*", "ceramic.GlassVolume")); assert!(matches("wood.*|metal.*", "metal.Basic")); assert!(!matches("wood.*", "ceramic.Wood")); assert!(matches("a*c", "abc"));
    }
}
