//! dumppart KIND args... out.json : dump node-group ports for exactness debugging
use infinigen::assets::tableparts::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = |i: usize| a[i].parse::<f64>().unwrap();
    let (mesh, out) = match a[1].as_str() {
        "top" => (table_top_exact(a[2].parse().unwrap(), f(3), f(4), f(5), f(6), f(7)), &a[8]),
        "seat" => {
            use infinigen::assets::officechair::*;
            // seat width thickness front_rel front_bent seat_bent mid_rel mid_bent back_rel back_bent top_rel top_bent seat_height mid_pos out
            let sp = SeatParams { width: f(2), thickness: f(3), front_rel: f(4), front_bent: f(5), seat_bent: f(6), mid_rel: f(7), mid_bent: f(8), back_rel: f(9), back_bent: f(10), top_rel: f(11), top_bent: f(12), seat_height: f(13), mid_pos: f(14) };
            (curvy_seat(&sp, 256, 128), &a[15])
        }
        "capcyl" => {
            use infinigen::assets::officechair::*;
            (capped_cylinder(f(2), f(3), f(4)), &a[5])
        }
        "ngon" => {
            let c = infinigen::assets::tableparts::ngon_profile_curve(a[2].parse().unwrap(), f(3), f(4), f(5));
            let mut m = infinigen::mesh::Mesh::new();
            m.pos = if std::env::var("RES").is_ok() { c.resample(512).pts } else { c.pts.clone() };
            (m, &a[6])
        }
        "chcube" => {
            let m = infinigen::mesh::Mesh::cuboid([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).welded(1e-9);
            let s = infinigen::assets::solid::Solid::from_mesh(&m).chamfer_sharp(f(2), 70f64.to_radians());
            (s.to_mesh(), &a[3])
        }
        "doorbody" => {
            use infinigen::assets::doors::*;
            let kind = match a[2].as_str() { "PanelDoorFactory" => DoorKind::Panel, "GlassPanelDoorFactory" => DoorKind::GlassPanel, "LouverDoorFactory" => DoorKind::Louver, _ => DoorKind::Lite };
            let mut rr = infinigen::rng::Rng::seeded(1);
            let mut c = infinigen::rooms::consts::RoomConstants::sample(&mut rr, 1);
            c.door_width = f(5); c.door_size = f(6); c.wall_thickness = f(7);
            let spec = DoorSpec::new(kind, a[3].parse().unwrap(), &c);
            let mut r = infinigen::assets::inst_rng(spec.fseed, 0);
            let _hd = spec.handle_draws(&mut r);
            let panels = spec.make_panels(&mut r);
            if std::env::var("DBGP").is_ok() { eprintln!("{:?} xs {} ys {} m {}", panels, spec.x_subdivisions, spec.y_subdivisions, spec.panel_margin); }
            let slab = door_slab(&spec, &panels);
            let ch = if std::env::var("NOCH").is_ok() { slab } else { slab.chamfer_sharp(0.001, 70f64.to_radians()) };
            let mut m = ch.to_mesh();
            for p in m.pos.iter_mut() { *p = [p[0] - spec.width, p[1] - spec.depth / 2.0, p[2] - spec.height / 2.0]; }
            (m, &a[4])
        }
        "doorfull" => {
            use infinigen::assets::doors::*;
            let kind = match a[2].as_str() { "PanelDoorFactory" => DoorKind::Panel, "GlassPanelDoorFactory" => DoorKind::GlassPanel, "LouverDoorFactory" => DoorKind::Louver, _ => DoorKind::Lite };
            let mut rr = infinigen::rng::Rng::seeded(1);
            let mut c = infinigen::rooms::consts::RoomConstants::sample(&mut rr, 1);
            c.door_width = f(5); c.door_size = f(6); c.wall_thickness = f(7);
            let spec = DoorSpec::new(kind, a[3].parse().unwrap(), &c);
            let mut r = infinigen::assets::inst_rng(spec.fseed, 0);
            let parts = spec.build_parts(&mut r);
            let which = std::env::var("PART").unwrap_or_default();
            let m = match which.as_str() { "frame" => parts.frame.clone(), "door" => { let mut m = parts.door.clone(); m.append(&parts.glass); m.append(&parts.louver); m }, _ => spec.assemble(&parts) };
            (m, &a[4])
        }
        "window" => {
            use infinigen::assets::windows::*;
            let fseed: u64 = a[2].parse().unwrap();
            let spec = WindowSpec::new(fseed);
            let mut r = infinigen::assets::inst_rng(fseed, 0);
            let asset = spec.sample_asset(&mut r, [f(3), f(4), f(5)], true);
            if std::env::var("DBGP").is_ok() { eprintln!("{:?} bevel {}", asset, spec.bevel_amount); }
            (window_mesh(&spec, &asset), &a[6])
        }
        "wheel" => {
            use infinigen::assets::officechair::*;
            (chair_wheel(f(2), f(3), f(4), f(5), 0.6, f(6)).0, &a[7])
        }
        _ => panic!(),
    };
    let verts: Vec<_> = mesh.pos.iter().map(|v| serde_json::json!([v[0], v[1], v[2]])).collect();
    let polys: Vec<_> = mesh.faces.iter().map(|f| serde_json::json!(f)).collect();
    std::fs::write(out, serde_json::json!({"verts": verts, "polys": polys}).to_string()).unwrap();
    println!("RSDUMP {} verts {} faces", mesh.pos.len(), mesh.faces.len());
}
