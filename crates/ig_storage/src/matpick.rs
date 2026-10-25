//! material_assignments tables used by the GN appliances and the material-slot -> spec mapping for the exported node-graph templates.
use super::*;
use crate::materials::*;
use serde_json::json;

pub const METALS: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3)];
pub const MAYBE_GLASS: Weighted = &[
    ("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3),
    ("metal.BlackGlass", 4.0), ("ceramic.Glass", 10.0),
];
pub const GLASS: Weighted = &[("metal.BlackGlass", 2.0), ("ceramic.Glass", 10.0)];
pub const HANDLE: Weighted = &[
    ("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3),
    ("plastic.PlasticRough", 1.0), ("wood.Wood", 3.0), ("metal.WhiteMetal", 2.0),
];

pub fn spec_of(name: &str, cls: &str, seed: u64) -> Material {
    let rgb = if cls.starts_with("ceramic.Glass") || cls.contains("Glass") { [0.1, 0.1, 0.12] } else { [0.6, 0.6, 0.62] };
    spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

/// GN appliance material sockets, in the order of get_material_params (the exported templates name the stubs "stub", "stub.001", ... in that order).
/// `r` is the FixedSeed(factory_seed) stream right after sample_parameters. Returns (slot name, material) pairs; the unnamed slot "gn" (faces without
/// SetMaterial: material index 0 in python = the first slot) maps to the Surface material.
pub fn gn_appliance_mats(c: &str, mut r: Rng, fseed: u64) -> Vec<(String, Material)> {
    let roles: &[(&str, Weighted)] = match c {
        "DishwasherFactory" => &[("Surface", METALS), ("Front", MAYBE_GLASS), ("WhiteMetal", METALS), ("Top", METALS), ("NameMaterial", METALS)],
        "BeverageFridgeFactory" => &[("Surface", METALS), ("Front", MAYBE_GLASS), ("Handle", HANDLE), ("Back", METALS)],
        "OvenFactory" => &[("Surface", METALS), ("Back", METALS), ("WhiteMetal", METALS), ("SuperBlackGlass", MAYBE_GLASS), ("Glass", GLASS)],
        "MicrowaveFactory" => &[("Surface", METALS), ("Back", METALS), ("BlackGlass", MAYBE_GLASS), ("Glass", GLASS)],
        _ => &[],
    };
    let mut out = vec![];
    for (k, (role, table)) in roles.iter().enumerate() {
        let cls = weighted(&mut r, table);
        let nm = if k == 0 { "stub".to_string() } else { format!("stub.{:03}", k) };
        let m = spec_of(&format!("{}:{}", role.to_lowercase(), fseed), cls, fseed + k as u64);
        if k == 0 {
            out.push(("gn".to_string(), m.clone()));
        }
        out.push((nm, m));
    }
    out
}
