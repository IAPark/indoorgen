//! Simplified visual recipes for classes without a dedicated port: shapes composed inside the sampled bounding box.
use super::*;
use crate::materials::*;
use serde_json::json;

fn spec(name: &str, cls: &str, seed: i64, rgb: [f32; 3]) -> Material {
    spec_material(&format!("{name}:{seed}"), json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

fn pick(rng: &mut Rng, table: Weighted) -> &'static str {
    weighted(rng, table)
}

const FABRICS: Weighted = &[("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0), ("fabric.Leather", 0.3), ("fabric.SofaFabric", 1.0)];
const LARGE_SEAT: Weighted = &[("fabric.Velvet", 0.3), ("fabric.SofaFabric", 0.5), ("fabric.Leather", 0.2)];
const WOODS: Weighted = &[("wood.Wood", 1.0), ("wood.WhitePlywood", 0.1), ("wood.BlackPlywood", 0.1), ("wood.BlondePlywood", 0.1)];
const FURN_HARD: Weighted = &[("plastic.PlasticRough", 1.0), ("wood.Wood", 1.0)];
const FURN_LEG: Weighted = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0), ("wood.Wood", 1.0), ("plastic.PlasticRough", 1.0), ("metal.BrushedBlackMetal", 3.0), ("wood.BlackPlywood", 3.0)];
const TABLE_TOP: Weighted = &[("ceramic.Marble", 1.0), ("wood.Wood", 1.0), ("plastic.PlasticRough", 0.5)];
const CERAMICS: Weighted = &[("ceramic.Ceramic", 1.0)];
const DECOR_HARD: Weighted = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("ceramic.VaseCeramic", 4.0), ("ceramic.Marble", 1.0), ("ceramic.Ceramic", 1.0)];
const METALS: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7)];
const GLASS: Weighted = &[("ceramic.Glass", 1.0)];
const PLASTICS: Weighted = &[("plastic.Plastic", 1.0), ("plastic.PlasticRough", 1.0)];
const BED_FRAME: Weighted = &[("metal.BrushedMetal", 1.0), ("wood.Wood", 7.0), ("ceramic.Plaster", 2.0)];
const RUG: Weighted = &[("fabric.Rug", 7.0), ("fabric.CoarseKnitFabric", 1.0), ("fabric.FineKnitFabric", 1.0)];
const APPL: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("plastic.PlasticRough", 2.0)];

fn ex(lo: V3, hi: V3) -> V3 {
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

pub fn proxy_asset(class: &str, lo: V3, hi: V3, rng: &mut Rng) -> Asset {
    let mut a = Asset::default();
    let d = ex(lo, hi);
    let seed = rng.randint(0, 1_000_000);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    match class {
        "ChairFactory" | "BarChairFactory" | "OfficeChairFactory" => {
            let seat = spec("seat", pick(rng, if class == "OfficeChairFactory" { FABRICS } else { FURN_HARD }), seed, [0.5, 0.4, 0.3]);
            let leg = spec("chairleg", pick(rng, FURN_LEG), seed + 1, [0.3, 0.3, 0.3]);
            let z_seat = lo[2] + d[2] * if class == "BarChairFactory" { 0.75 } else { 0.5 };
            let t = 0.05f64.min(d[2] * 0.08);
            a.board([lo[0], lo[1], z_seat - t], [hi[0], hi[1], z_seat], &seat);
            a.cuboid([lo[0], lo[1], z_seat], [lo[0] + 0.04, hi[1], hi[2]], &seat);
            let lr = 0.02;
            for (x, y) in [(lo[0] + lr, lo[1] + lr), (lo[0] + lr, hi[1] - lr), (hi[0] - lr, lo[1] + lr), (hi[0] - lr, hi[1] - lr)] {
                a.cylinder(x, y, lo[2], z_seat - t, lr, &leg);
            }
        }
        "SofaFactory" | "ArmChairFactory" => {
            let f = spec("sofa_fabric", pick(rng, LARGE_SEAT), seed, [0.4, 0.4, 0.45]);
            let arm = d[1].min(0.2).max(0.12);
            a.cuboid([lo[0], lo[1], lo[2] + 0.06], [hi[0], hi[1], lo[2] + d[2] * 0.3], &f);
            a.cuboid([lo[0], lo[1] + arm, lo[2] + 0.06], [lo[0] + d[0] * 0.25, hi[1] - arm, hi[2]], &f);
            a.cuboid([lo[0], lo[1], lo[2] + 0.06], [hi[0], lo[1] + arm, lo[2] + d[2] * 0.7], &f);
            a.cuboid([lo[0], hi[1] - arm, lo[2] + 0.06], [hi[0], hi[1], lo[2] + d[2] * 0.7], &f);
            a.cuboid([lo[0] + 0.05, lo[1] + 0.05, lo[2]], [hi[0] - 0.05, hi[1] - 0.05, lo[2] + 0.06], &spec("sofa_leg", "wood.Wood", seed + 1, [0.2, 0.15, 0.1]));
        }
        "BedFactory" => {
            let f = spec("bedframe", pick(rng, BED_FRAME), seed, [0.5, 0.4, 0.3]);
            let m = spec("mattress", pick(rng, FABRICS), seed + 1, [0.9, 0.9, 0.9]);
            let zf = lo[2] + (d[2] * 0.25).min(0.4);
            a.cuboid([lo[0], lo[1], lo[2]], [hi[0], hi[1], zf], &f);
            a.cuboid([lo[0], lo[1], zf], [lo[0] + 0.08, hi[1], hi[2]], &f);
            a.cuboid([lo[0] + 0.08, lo[1] + 0.02, zf], [hi[0] - 0.02, hi[1] - 0.02, zf + 0.28], &m);
            let pw = (d[1] / 2.0 - 0.1).max(0.3);
            for s in [-1.0, 1.0] {
                let yc = c[1] + s * d[1] * 0.25;
                a.mesh(Mesh::ellipsoid([lo[0] + 0.3, yc, zf + 0.36], [0.2, pw / 2.0, 0.09], 16, 8), &spec("pillow", "fabric.FineKnitFabric", seed + 2 + s as i64, [0.9, 0.9, 0.9]), None);
            }
            a.cuboid([lo[0] + 0.7, lo[1] + 0.02, zf + 0.26], [hi[0] - 0.02, hi[1] - 0.02, zf + 0.34], &spec("blanket", "fabric.CoarseKnitFabric", seed + 3, [0.6, 0.5, 0.5]));
        }
        "TableDiningFactory" | "CoffeeTableFactory" | "SideTableFactory" | "TableCocktailFactory" | "SidetableDeskFactory" | "SimpleDeskFactory" => {
            let top = spec("tabletop", pick(rng, TABLE_TOP), seed, [0.6, 0.45, 0.3]);
            let leg = spec("tableleg", pick(rng, FURN_LEG), seed + 1, [0.3, 0.3, 0.3]);
            let t = (d[2] * 0.06).clamp(0.02, 0.06);
            a.board([lo[0], lo[1], hi[2] - t], [hi[0], hi[1], hi[2]], &top);
            let lr = (d[0].min(d[1]) * 0.04).clamp(0.02, 0.05);
            for (x, y) in [(lo[0] + 0.1, lo[1] + 0.1), (lo[0] + 0.1, hi[1] - 0.1), (hi[0] - 0.1, lo[1] + 0.1), (hi[0] - 0.1, hi[1] - 0.1)] {
                a.cylinder(x, y, lo[2], hi[2] - t, lr, &leg);
            }
        }
        "FloorLampFactory" | "DeskLampFactory" | "LampFactory" | "PointLampFactory" => {
            let m = spec("lamp_base", pick(rng, METALS), seed, [0.7, 0.7, 0.7]);
            let shade = spec("lampshade", "fabric.FineKnitFabric", seed + 1, [0.9, 0.85, 0.7]);
            let r = (d[0].max(d[1]) / 2.0).max(0.05);
            a.cylinder(c[0], c[1], lo[2], lo[2] + 0.03, r * 0.7, &m);
            a.cylinder(c[0], c[1], lo[2], hi[2] - d[2] * 0.3, 0.012, &m);
            let mut emit = Material::diffuse("lamp_emit", [1.0, 0.9, 0.7], 0.5);
            emit.emissive = [1.0, 0.85, 0.6];
            emit.emissive_strength = 3.0;
            let _ = emit;
            a.frustum(c[0], c[1], hi[2] - d[2] * 0.3, hi[2], r * 0.6, r, &shade);
        }
        "CeilingLightFactory" | "CeilingClassicLampFactory" => {
            let body = spec("ceiling_fixture", "metal.MetalBasic", seed, [0.8, 0.8, 0.8]);
            a.frustum(c[0], c[1], lo[2], hi[2], d[0] / 2.0, d[0] / 2.0 * 0.9, &body);
        }
        "PlantContainerFactory" | "LargePlantContainerFactory" | "VaseFactory" => {
            let pot = spec("pot", pick(rng, DECOR_HARD), seed, [0.7, 0.4, 0.3]);
            let leaf = spec("plant", "ceramic.Ceramic", seed, [0.2, 0.45, 0.15]);
            let r = d[0].min(d[1]) / 2.0;
            let ph = d[2] * 0.4;
            a.frustum(c[0], c[1], lo[2], lo[2] + ph, r * 0.75, r, &pot);
            let mut m = Material::diffuse(&format!("foliage:{seed}"), [0.15 + 0.1 * rng.f64() as f32, 0.4 + 0.2 * rng.f64() as f32, 0.12], 0.6);
            m.spec = None;
            a.mesh(Mesh::ellipsoid([c[0], c[1], lo[2] + ph + (d[2] - ph) * 0.5], [d[0] / 2.0, d[1] / 2.0, (d[2] - ph) / 2.0], 14, 8), &m, Some(([lo[0], lo[1], lo[2] + ph], hi)));
            let _ = leaf;
        }
        "BowlFactory" | "PotFactory" | "CupFactory" | "WineglassFactory" | "JarFactory" | "BottleFactory" | "CanFactory" | "PlateFactory" | "FruitContainerFactory" | "PanFactory" => {
            let table = match class {
                "WineglassFactory" | "BottleFactory" | "JarFactory" => GLASS,
                "CanFactory" | "PotFactory" | "PanFactory" => METALS,
                "PlateFactory" | "BowlFactory" | "CupFactory" => CERAMICS,
                _ => DECOR_HARD,
            };
            let m = spec("tableware", pick(rng, table), seed, [0.85, 0.85, 0.85]);
            let r = d[0].min(d[1]) / 2.0;
            let top_r = if class == "BowlFactory" || class == "PlateFactory" { r } else { r * 0.95 };
            a.frustum(c[0], c[1], lo[2], hi[2], r * if class == "BowlFactory" { 0.5 } else { 0.9 }, top_r, &m);
        }
        "BookStackFactory" | "BookColumnFactory" | "BookFactory" => {
            let m = spec("book", "ceramic.Plaster", seed, [0.5, 0.2, 0.2]);
            let n = if class == "BookStackFactory" { 3 } else { ((d[1] / 0.03) as usize).clamp(3, 14) };
            if class == "BookStackFactory" {
                let mut z = lo[2];
                for i in 0..n {
                    let th = d[2] / n as f64;
                    let j = 0.01 * (i as f64);
                    a.cuboid([lo[0] + j, lo[1] + j, z], [hi[0] - j, hi[1] - j, z + th], &spec("book", "ceramic.Plaster", seed + i as i64, [0.4, 0.3, 0.2]));
                    z += th;
                }
            } else {
                let w = d[1] / n as f64;
                for i in 0..n {
                    a.cuboid([lo[0], lo[1] + w * i as f64, lo[2]], [hi[0], lo[1] + w * (i as f64 + 0.85), hi[2] - 0.01 * (i % 3) as f64], &spec("book", "ceramic.Plaster", seed + i as i64, [0.4, 0.3, 0.2]));
                }
            }
            let _ = m;
        }
        "OvenFactory" | "DishwasherFactory" | "BeverageFridgeFactory" | "MicrowaveFactory" => {
            let m = spec("appliance", pick(rng, APPL), seed, [0.7, 0.7, 0.72]);
            a.cuboid(lo, hi, &m);
            a.cuboid([hi[0], lo[1] + d[1] * 0.08, lo[2] + d[2] * 0.25], [hi[0] + 0.02, hi[1] - d[1] * 0.08, hi[2] - d[2] * 0.1], &spec("appl_glass", "ceramic.Glass", seed + 1, [0.05, 0.05, 0.05]));
        }
        "TVFactory" | "MonitorFactory" => {
            let m = spec("tv_body", "plastic.PlasticRough", seed, [0.05, 0.05, 0.05]);
            a.cuboid([lo[0], lo[1], lo[2] + d[2] * 0.15], [lo[0] + (d[0] * 0.15).min(0.05), hi[1], hi[2]], &m);
            a.cuboid([lo[0], c[1] - d[1] * 0.1, lo[2]], [hi[0], c[1] + d[1] * 0.1, lo[2] + d[2] * 0.15], &m);
        }
        "RugFactory" => {
            a.cuboid(lo, hi, &spec("rug", pick(rng, RUG), seed, [0.5, 0.3, 0.3]));
        }
        "WallArtFactory" | "MirrorFactory" => {
            let frame = spec("frame", pick(rng, WOODS), seed, [0.3, 0.2, 0.1]);
            let inner = if class == "MirrorFactory" { spec("mirror", "metal.Mirror", seed + 1, [0.8, 0.85, 0.9]) } else { spec("art", "art.Art", seed + 1, [0.6, 0.5, 0.4]) };
            a.cuboid(lo, hi, &frame);
            let m = d[1].min(d[2]) * 0.06;
            a.cuboid([hi[0], lo[1] + m, lo[2] + m], [hi[0] + 0.003, hi[1] - m, hi[2] - m], &inner);
        }
        "NatureShelfTrinketsFactory" | "BalloonFactory" | "TowelFactory" | "BlanketFactory" => {
            let m = spec("trinket", "ceramic.Ceramic", seed, [0.7, 0.6, 0.5]);
            a.mesh(Mesh::ellipsoid([c[0], c[1], (lo[2] + hi[2]) / 2.0], [d[0] / 2.0, d[1] / 2.0, d[2] / 2.0], 10, 6), &m, Some((lo, hi)));
        }
        _ => {
            a.cuboid(lo, hi, &spec("generic", "wood.Wood", seed, [0.5, 0.4, 0.3]));
        }
    }
    a
}
