// builds csrc/ (Jolt Physics + Bullet + the cloth shim) with CMake; sources: JOLT_DIR / BULLET_DIR (default ~/work/ref/...)
fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let jolt = std::env::var("JOLT_DIR").unwrap_or(format!("{home}/work/ref/JoltPhysics"));
    let bullet = std::env::var("BULLET_DIR").unwrap_or(format!("{home}/work/ref/bullet3"));
    println!("cargo:rerun-if-changed=csrc/cloth_shim.cpp");
    println!("cargo:rerun-if-changed=csrc/CMakeLists.txt");
    println!("cargo:rerun-if-env-changed=JOLT_DIR");
    println!("cargo:rerun-if-env-changed=BULLET_DIR");
    let dst = cmake::Config::new("csrc").define("JOLT_DIR", &jolt).define("BULLET_DIR", &bullet).profile("Release").build_target("clothshim").build();
    let b = dst.join("build");
    for (dir, lib) in [("", "clothshim"), ("jolt", "Jolt"), ("bt_softbody", "BulletSoftBody"), ("bt_dynamics", "BulletDynamics"), ("bt_collision", "BulletCollision"), ("bt_linearmath", "LinearMath")] {
        println!("cargo:rustc-link-search=native={}", b.join(dir).display());
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=c++");
}
