//! assetsheet CLASSES N out.glb [seed0]: build N of each asset straight from its factory (no house, no layout) on a floor, and write out.cams.json
//! with each one's box for the previewer: the iteration loop for asset geometry, see tools/assets.sh. CLASSES: comma list, `Sink` = SinkFactory;
//! also the dressed props (Kettle, LogStack, LogBasket, DishRack, UtensilCrock, SoapDispenser, ToothbrushCup, ToolSet, Candlestick, Slipper).
use indoorgen::assets::*;
use indoorgen::dress::props;
use indoorgen::furn::assets::TableAssets;
use indoorgen::mesh::*;

fn prop(name: &str, seed: u64) -> Option<Asset> {
    Some(match name {
        "Kettle" => props::kettle(seed),
        "LogStack" => props::log_stack(seed),
        "LogBasket" => props::log_basket(seed),
        "DishRack" => props::dish_rack(seed),
        "UtensilCrock" => props::utensil_crock(seed),
        "SoapDispenser" => props::soap_dispenser(seed),
        "ToothbrushCup" => props::toothbrush_cup(seed),
        "ToolSet" => props::tool_set(seed),
        "Candlestick" => props::candlestick(seed),
        "Slipper" => {
            let mut a = Asset::default();
            let mut m = indoorgen::bed_decor::slipper(-0.055);
            m.append(&indoorgen::bed_decor::slipper(0.055));
            a.mesh(m, &Material::diffuse("slipper", [0.3, 0.35, 0.5], 0.9), None);
            a
        }
        _ => return None,
    })
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let a: Vec<String> = std::env::args().collect();
    let classes: Vec<String> = a[1].split(',').map(|s| s.to_string()).collect();
    let n: u64 = a[2].parse().unwrap();
    let out = std::path::PathBuf::from(&a[3]);
    let seed0: u64 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    let mut fallback = TableAssets::load(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_bbox_table.json"));
    fallback.add_file(concat!(env!("CARGO_MANIFEST_DIR"), "/data/asset_samples.json"));
    let assets = RealAssets { fallback };
    let mut scene = Scene::default();
    let mut cams = vec![];
    let mut y = 0.0f64;
    let mut xmax = 0.0f64;
    for class in &classes {
        let full = if class.ends_with("Factory") { class.clone() } else { format!("{class}Factory") };
        let (mut x, mut row) = (0.0f64, 0.3f64);
        for i in 0..n {
            // (scattered: the first draws of nearby seeds are correlated, and neighbours came out near identical)
            let seed = (seed0 + i).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 24;
            let t = std::time::Instant::now();
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(p) = prop(class, seed) { return p }
                indoorgen::assets::set_unseeded(indoorgen::assets::ctor_draws::ctor_draws(&full, &mut indoorgen::rng::Rng::seeded(seed)));
                assets.visual(&full, seed, 0)
            }));
            let Ok(asset) = built else { println!("{class} seed {seed}: PANICS"); continue };
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let (lo, hi) = asset.bounds();
            if !(0..3).all(|d| hi[d] > lo[d] && hi[d].is_finite() && lo[d].is_finite()) { println!("{class} seed {seed}: EMPTY"); continue }
            let r = (0..3).map(|d| (hi[d] - lo[d]).powi(2)).sum::<f64>().sqrt() / 2.0;
            // (room round each one: a close-up's wide lens takes in about 2 radii either side)
            let gap = 2.5 * r + 0.3;
            let (cx, cy) = (x + gap, y + gap);
            let shift = Mat4::translation([cx - (lo[0] + hi[0]) / 2.0, cy - (lo[1] + hi[1]) / 2.0, -lo[2]]);
            let name = format!("{class}_{seed}");
            for (pi, p) in asset.parts.iter().enumerate() {
                let mut m = p.mesh.clone();
                m.transform(&shift);
                let mi = scene.material(p.mat.clone());
                scene.add(&format!("{name}#{pi}"), &full, m, mi);
            }
            println!("{class} seed {seed}: {ms:.0} ms, {:.2} x {:.2} x {:.2} m, {} parts", hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2], asset.parts.len());
            cams.push(serde_json::json!({"name": name, "class": class, "seed": seed, "ms": ms, "c": [cx, cy, (hi[2] - lo[2]) / 2.0], "r": r}));
            x += 2.0 * gap;
            row = row.max(gap);
        }
        xmax = xmax.max(x);
        y += 2.0 * row;
    }
    let fm = scene.material(Material::diffuse("floor", [0.62, 0.6, 0.58], 0.9));
    let mut floor = Mesh::new();
    floor.add_quad([-1.0, -1.0, 0.0], [xmax + 1.0, -1.0, 0.0], [xmax + 1.0, y + 1.0, 0.0], [-1.0, y + 1.0, 0.0]);
    scene.add("sheet.floor", "", floor, fm);
    scene.write_glb(&out).unwrap();
    std::fs::write(out.with_extension("cams.json"), serde_json::to_string(&cams).unwrap()).unwrap();
}
