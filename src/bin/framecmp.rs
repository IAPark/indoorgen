//! asset() vs scene_asset() bounds per class (the layout build uses asset(); the final, scene_asset): `framecmp [N]`
fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    for c in ["SofaFactory", "ArmChairFactory", "ChairFactory", "BarChairFactory", "OfficeChairFactory", "TableDiningFactory", "CoffeeTableFactory", "SideTableFactory", "TableCocktailFactory", "SimpleDeskFactory", "BedFactory", "WardrobeFactory", "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory", "TVStandFactory", "FridgeFactory", "ShowerFactory", "FloorLampFactory", "BookStackFactory"] {
        let mut worst = 0.0f64;
        for s in 1..=n {
            let Some(f) = indoorgen::assets::make_factory(c, s * 7919) else { break };
            let (a, b) = (f.asset(s).bounds(), ig_kit::api::scene_asset(&*f, s).bounds());
            let d = (0..3).map(|k| (a.0[k] - b.0[k]).abs().max((a.1[k] - b.1[k]).abs())).fold(0.0, f64::max);
            worst = worst.max(d);
        }
        println!("{c:24} max bounds diff {:.3} m{}", worst, if worst > 0.01 { "  <--" } else { "" });
    }
}
