//! doorparams CLASS fseed const_seed : print door parameters as json
use indoorgen::assets::doors::*;
use indoorgen::rng::Rng;
use indoorgen::rooms::consts::RoomConstants;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let kind = match a[1].as_str() { "PanelDoorFactory" => DoorKind::Panel, "GlassPanelDoorFactory" => DoorKind::GlassPanel, "LouverDoorFactory" => DoorKind::Louver, _ => DoorKind::Lite };
    let mut r = Rng::seeded(1);
    let mut c = RoomConstants::sample(&mut r, 1);
    c.door_width = a[3].parse().unwrap(); c.door_size = a[4].parse().unwrap(); c.wall_thickness = a[5].parse().unwrap();
    println!("consts {} {} {}", c.door_width, c.door_size, c.wall_thickness);
    println!("{:?}", DoorSpec::new(kind, a[2].parse().unwrap(), &c));
}
