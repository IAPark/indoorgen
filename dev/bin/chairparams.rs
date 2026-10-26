use indoorgen::assets::chair::ChairSpec;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (s0, n): (u64, u64) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    for s in s0..s0 + n {
        let c = ChairSpec::new(s);
        println!("{} leg_type={} round_leg={} back={} arm={} xbar={} ybar={} seat_round={} seat_sub={}", s, c.leg_type, c.is_leg_round, c.back_type, c.has_arm, c.has_leg_x_bar, c.has_leg_y_bar, c.is_seat_round, c.is_seat_subsurf);
    }
}
