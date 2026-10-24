use nalgebra as na;
use parry3d_f64::query;
use parry3d_f64::shape::Cuboid;
fn main() {
    let a = Cuboid::new(na::Vector3::new(0.2205727994441986, 0.7160565853118896, 0.25715383887290955));
    let b = Cuboid::new(na::Vector3::new(0.5491480231285095, 0.884368360042572, 0.46483778953552246));
    let ia = na::Isometry3::from_parts(na::Translation3::new(8.24287707577216, 2.425194949796486, 0.39220425486564636), na::UnitQuaternion::new_unchecked(na::Quaternion::new(0.707106892172712, 0.0, 0.0, 0.7071062908788234)));
    let ib = na::Isometry3::from_parts(na::Translation3::new(11.21003395318985, 3.3797942037076205, 0.5997157096862793), na::UnitQuaternion::new_unchecked(na::Quaternion::new(7.549790126404332e-8, 0.0, 0.0, 1.0)));
    println!("distance {:?}", query::distance(&ia, &a, &ib, &b));
    let ia2 = na::Isometry3::from_parts(na::Translation3::new(8.24287707577216, 2.425194949796486, 0.39220425486564636), na::UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::FRAC_PI_2));
    println!("distance exact rot {:?}", query::distance(&ia2, &a, &ib, &b));
    println!("closest {:?}", query::closest_points(&ia, &a, &ib, &b, 100.0));
}
