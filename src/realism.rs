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
    ("Kitchen", "Storage", "against_wall", 0, 3, false),
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
    ("Bedroom", "LargePlantContainerFactory", "against_wall", 0, 1, false),
    ("Bedroom", "PlantContainerFactory", "ontop:Storage", 0, 2, false),
    // dining room
    ("DiningRoom", "Storage", "against_wall", 1, 3, false),
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
