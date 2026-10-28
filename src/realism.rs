//! Default "realism" rules on top of the dense program (program.realism): counts that make rooms read like real homes, from
//! looking at generated rooms next to 3D-FRONT / Hypersim interiors. Each rule only applies when the user's rules (GenConfig.rules) do
//! not already set that room + class + relation.
use crate::rules::{Prefer, Rule, RoomRules, RuleSpec};

/// (room type, class, relation, min, max, prefer more?)
const RULES: &[(&str, &str, &str, u32, u32, bool)] = &[
    // living room: a seating group around one TV, a few storage pieces (the dense default allowed 3..8, rooms looked like libraries)
    ("LivingRoom", "SofaFactory", "on_floor", 1, 2, false),
    ("LivingRoom", "TVFactory", "ontop:TVStandFactory", 1, 1, false),
    ("LivingRoom", "Storage", "against_wall", 1, 4, false),
    ("LivingRoom", "ArmChairFactory", "on_floor", 0, 2, false),
    ("LivingRoom", "WallArtFactory", "against_wall", 1, 4, false),
    ("LivingRoom", "RugFactory", "on_floor", 1, 1, false),
    // shelves held a dozen trinkets and four potted plants; no kitchen cabinets, at most one desk lamp
    ("LivingRoom", "NatureShelfTrinketsFactory", "on:Storage", 0, 4, false),
    ("LivingRoom", "PlantContainerFactory", "ontop:Storage", 0, 2, false),
    ("LivingRoom", "DeskLampFactory", "ontop:Furniture", 0, 1, false),
    ("LivingRoom", "KitchenCabinetFactory", "against_wall", 0, 0, false),
    // kitchen: cabinets and appliances, not bookcases or desk lamps (the generic storage rule put up to 5 bookcases in kitchens)
    ("Kitchen", "SimpleBookcaseFactory", "against_wall", 0, 0, false),
    ("Kitchen", "CellShelfFactory", "against_wall", 0, 0, false),
    ("Kitchen", "DeskLampFactory", "ontop:Furniture", 0, 0, false),
    ("Kitchen", "Storage", "against_wall", 0, 2, false),
    // tall open shelves took the only free wall of small kitchens before the counters (no KitchenSpace at all)
    ("Kitchen", "LargeShelfFactory", "against_wall", 0, 0, false),
    // book columns stood on kitchen counters
    ("Kitchen", "BookColumnFactory", "on:Storage", 0, 0, false),
    ("Kitchen", "BookStackFactory", "on:Storage", 0, 0, false),
    // one full-size fridge (FridgeFactory, ours) per kitchen and none elsewhere
    ("Kitchen", "FridgeFactory", "against_wall", 1, 1, false),
    ("LivingRoom", "FridgeFactory", "against_wall", 0, 0, false),
    ("Bedroom", "FridgeFactory", "against_wall", 0, 0, false),
    ("DiningRoom", "FridgeFactory", "against_wall", 0, 0, false),
    ("Bathroom", "FridgeFactory", "against_wall", 0, 0, false),
    // bathroom: a toilet always, a bathtub where it fits, no bookcases, a few toiletries rather than dozens of bottles
    ("Bathroom", "ToiletFactory", "against_wall", 1, 1, false),
    ("Bathroom", "BathtubFactory", "against_wall", 1, 1, false),
    ("Bathroom", "SimpleBookcaseFactory", "against_wall", 0, 0, false),
    ("Bathroom", "SimpleBookcaseFactory", "on_floor", 0, 0, false),
    ("Bathroom", "LargeShelfFactory", "on_floor", 0, 0, false),
    ("Bathroom", "CellShelfFactory", "against_wall", 0, 0, false),
    ("Bathroom", "CellShelfFactory", "on_floor", 0, 0, false),
    ("Bathroom", "BottleFactory", "on:Storage", 0, 3, false),
    // shelves in bathrooms held bowls, books and trinkets by the dozen; walls up to four paintings
    ("Bathroom", "BowlFactory", "on:Storage", 0, 0, false),
    ("Bathroom", "BookColumnFactory", "on:Storage", 0, 0, false),
    ("Bathroom", "BookStackFactory", "on:Storage", 0, 0, false),
    ("Bathroom", "NatureShelfTrinketsFactory", "on:Storage", 0, 2, false),
    ("Bathroom", "WallArtFactory", "against_wall", 0, 1, false),
    ("Bathroom", "LargePlantContainerFactory", "against_wall", 0, 1, false),
    // bedroom: a bed, nightstands, a couple of storage pieces, not a library
    ("Bedroom", "Storage", "against_wall", 1, 2, false),
    // one bed (the dense default allowed two and always got two), one floor lamp, no kitchen cabinets, shelves not piled with trinkets
    ("Bedroom", "Bed", "against_wall", 1, 1, false),
    ("Bedroom", "FloorLampFactory", "against_wall", 0, 1, false),
    ("Bedroom", "KitchenCabinetFactory", "against_wall", 0, 0, false),
    ("Bedroom", "DeskLampFactory", "ontop:Furniture", 0, 2, false),
    ("Bedroom", "NatureShelfTrinketsFactory", "on:Storage", 0, 4, false),
    ("Bedroom", "BookColumnFactory", "on:Storage", 0, 3, false),
    // a wardrobe (WardrobeFactory, ours) in nearly every bedroom, as in 3D-FRONT; never elsewhere
    ("Bedroom", "WardrobeFactory", "against_wall", 1, 1, false),
    ("LivingRoom", "WardrobeFactory", "against_wall", 0, 0, false),
    ("Kitchen", "WardrobeFactory", "against_wall", 0, 0, false),
    ("DiningRoom", "WardrobeFactory", "against_wall", 0, 0, false),
    ("Bathroom", "WardrobeFactory", "against_wall", 0, 0, false),
    ("Bedroom", "SimpleBookcaseFactory", "against_wall", 0, 1, false),
    ("Bedroom", "CellShelfFactory", "against_wall", 0, 1, false),
    // a tall open shelf plus the wardrobe left small bedrooms no wall for the bed
    ("Bedroom", "LargeShelfFactory", "against_wall", 0, 0, false),
    ("Bedroom", "LargePlantContainerFactory", "against_wall", 0, 1, false),
    ("Bedroom", "PlantContainerFactory", "ontop:Storage", 0, 2, false),
    // dining room
    ("DiningRoom", "Storage", "against_wall", 1, 2, false),
    // narrow dining rooms got three shelves and one chair
    ("DiningRoom", "LargeShelfFactory", "against_wall", 0, 1, false),
    ("DiningRoom", "CellShelfFactory", "against_wall", 0, 1, false),
    ("DiningRoom", "SimpleBookcaseFactory", "against_wall", 0, 1, false),
    // dining rooms had 10+ wine glasses and desk lamps
    ("DiningRoom", "WineglassFactory", "on:Storage", 0, 3, false),
    ("DiningRoom", "WineglassFactory", "ontop:TableDiningFactory", 0, 4, false),
    ("DiningRoom", "CupFactory", "on:Storage", 0, 3, false),
    ("DiningRoom", "NatureShelfTrinketsFactory", "on:Storage", 0, 3, false),
    ("DiningRoom", "DeskLampFactory", "ontop:Furniture", 0, 0, false),
    // ... which left dining shelves near empty: books and vases instead, preferred
    ("DiningRoom", "BookColumnFactory", "on:Storage", 0, 8, true),
    ("DiningRoom", "VaseFactory", "on:Storage", 0, 3, true),
];

pub fn with_realism_rules(mut spec: RuleSpec) -> RuleSpec {
    for &(room, class, rel, min, max, more) in RULES {
        let rr = spec.rooms.entry(room.into()).or_insert_with(RoomRules::default);
        if rr.rules.iter().any(|x| x.class == class && x.relation == rel) {
            continue;
        }
        rr.rules.push(Rule { class: class.into(), relation: rel.into(), min: Some(min), max: Some(max), prefer: more.then_some(Prefer::More), weight: None });
    }
    spec
}

/// Room type (CamelCase, as in RuleSpec.rooms) of a room object key like "living-room_0/0".
fn room_type_of(key: &str) -> String {
    key.split('_').next().unwrap_or("").split('-').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default() }).collect()
}

/// Post-solve cap enforcement: the solver accepts moves at equal violation count, so a 0..1 cap can be broken while satisfying a
/// built-in minimum (5 wall art pieces against a cap of 4, two TVs on one stand). For every factory class with a max in `spec`
/// (summed over its rules in that room type), extra objects in a room are removed -- the ones with the fewest dependents, added last
/// first -- together with everything resting on them. Returns the removed keys.
pub fn enforce_caps(state: &mut crate::furn::state::State, spec: &RuleSpec) -> Vec<String> {
    use std::collections::{BTreeMap, HashSet};
    let mut caps: BTreeMap<(String, String), u32> = BTreeMap::new();
    for (room, rr) in &spec.rooms {
        for r in &rr.rules {
            if !r.class.ends_with("Factory") { continue }
            if let Some(m) = r.max { *caps.entry((room.clone(), r.class.clone())).or_insert(0) += m }
        }
    }
    let keys: Vec<String> = state.objs.keys().cloned().collect();
    let is_room = |k: &str| state.objs.get(k).map_or(false, |o| o.generator.is_none()) && k.contains('/');
    let room_of = |k: &str| -> Option<String> {
        let mut cur = k.to_string();
        for _ in 0..8 {
            let o = state.objs.get(&cur)?;
            if let Some(t) = o.relations.iter().map(|r| r.target.clone()).find(|t| is_room(t)) { return Some(t) }
            cur = o.relations.iter().map(|r| r.target.clone()).find(|t| state.objs.contains_key(t))?;
        }
        None
    };
    let dependents = |k: &str| keys.iter().filter(|c| state.objs[c.as_str()].relations.iter().any(|r| r.target == k)).count();
    let mut by: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for k in &keys {
        let Some(g) = &state.objs[k.as_str()].generator else { continue };
        let Some(room) = room_of(k) else { continue };
        by.entry((room, g.class.clone())).or_default().push(k.clone());
    }
    let mut remove: HashSet<String> = HashSet::new();
    for ((room, class), objs) in &by {
        let Some(&cap) = caps.get(&(room_type_of(room), class.clone())).or_else(|| caps.get(&("Any".to_string(), class.clone()))) else { continue };
        if objs.len() as u32 <= cap { continue }
        let mut order: Vec<(usize, usize, &String)> = objs.iter().enumerate().map(|(i, k)| (dependents(k), usize::MAX - i, k)).collect();
        order.sort();
        if std::env::var_os("CAPS_DEBUG").is_some() { eprintln!("CAPS {room} {class} {} > {cap}", objs.len()); }
        for (_, _, k) in order.iter().take(objs.len() - cap as usize) { remove.insert((*k).clone()); }
    }
    // everything resting on a removed object goes too
    loop {
        let more: Vec<String> = keys.iter().filter(|k| !remove.contains(*k) && state.objs[k.as_str()].relations.iter().any(|r| remove.contains(&r.target))).cloned().collect();
        if more.is_empty() { break }
        remove.extend(more);
    }
    let mut out: Vec<String> = remove.into_iter().collect();
    out.sort();
    for k in &out { state.objs.shift_remove(k); }
    out
}
