use indoorgen::assets::bed::BedSpec;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s: u64 = a[1].parse().unwrap();
    let b = BedSpec::frame(s);
    let c = &b.c;
    println!("{}", serde_json::json!({"width": c.width, "size": c.size, "thickness": c.thickness, "bevel_width": c.bevel_width, "leg_thickness": c.leg_thickness, "leg_height": c.leg_height, "back_height": c.back_height, "is_leg_round": c.is_leg_round, "has_leg_x_bar": c.has_leg_x_bar, "has_leg_y_bar": c.has_leg_y_bar, "leg_offset_bar": c.leg_offset_bar, "back_thickness": c.back_thickness, "bed_back": c.bed_back, "back_profile": c.back_profile, "has_all_legs": b.has_all_legs, "leg_decor": b.leg_decor, "wrapped": b.leg_decor_wrapped, "sx": b.seat_sub_x, "sy": b.seat_sub_y, "dot_distance": b.dot_distance, "dot_size": b.dot_size, "dot_depth": b.dot_depth, "panel_distance": b.panel_distance, "panel_margin": b.panel_margin, "surf": b.frame_cls, "limb": b.frame_limb_cls, "panel": c.panel_cls, "seat_mid": c.seat_mid, "limb_profile": c.limb_profile}));
}
