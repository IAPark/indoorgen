//! Per-class build cost and collision-box count of the real factories (layout fast-build planning): `buildtime [N]`.
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let n: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let classes = ["SofaFactory", "ArmChairFactory", "ChairFactory", "BarChairFactory", "OfficeChairFactory", "TableDiningFactory", "CoffeeTableFactory", "SideTableFactory", "TableCocktailFactory", "SimpleDeskFactory", "ModernDeskFactory", "BedFactory", "WardrobeFactory", "SimpleBookcaseFactory", "LargeShelfFactory", "CellShelfFactory", "SingleCabinetFactory", "KitchenCabinetFactory", "KitchenSpaceFactory", "TVStandFactory", "FridgeFactory", "OvenFactory", "DishwasherFactory", "ToiletFactory", "BathtubFactory", "ShowerFactory", "StandingSinkFactory", "LargePlantContainerFactory", "FloorLampFactory", "NightstandFactory"];
    println!("{:28} {:>8} {:>8} {:>6} {:>6} {:>9} {:>7}", "class", "asset ms", "geom ms", "parts", "boxes", "layout ms", "lboxes");
    for c in classes {
        let (mut ta, mut tg, mut parts, mut boxes, mut k, mut tl, mut lb) = (0.0, 0.0, 0usize, 0usize, 0u64, 0.0, 0usize);
        for s in 1..=n {
            let Some(f) = indoorgen::assets::make_factory(c, s * 7919) else { break };
            let t = std::time::Instant::now();
            let a = f.asset(s);
            ta += t.elapsed().as_secs_f64();
            let t = std::time::Instant::now();
            let g = a.solver_geom();
            tg += t.elapsed().as_secs_f64();
            parts += a.parts.len(); boxes += g.proxies.len(); k += 1;
            let t = std::time::Instant::now();
            ig_core::envx::set_layout_build(true);
            let la = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.asset(s)));
            ig_core::envx::set_layout_build(false);
            let Ok(la) = la else { println!("{c:28} LAYOUT BUILD PANICS"); break };
            let lg = la.layout_geom();
            tl += t.elapsed().as_secs_f64(); lb += lg.proxies.len();
        }
        if k == 0 { println!("{c:28} (no factory)"); continue }
        let kf = k as f64;
        println!("{c:28} {:8.1} {:8.2} {:6.1} {:6.1} {:9.1} {:7.1}", ta / kf * 1e3, tg / kf * 1e3, parts as f64 / kf, boxes as f64 / kf, tl / kf * 1e3, lb as f64 / kf);
    }
}
