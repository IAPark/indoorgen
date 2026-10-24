// C/C++ shims (csrc/*.cpp): OpenVDB voxel remesh (Blender's REMESH modifier in VOXEL mode), more can be added by dropping files into csrc/.
use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_openvdb)");
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-changed=build.rs");
    let brew = ["/opt/homebrew", "/usr/local"].iter().find(|p| Path::new(&format!("{p}/opt/openvdb")).exists()).copied();
    let Some(brew) = brew else {
        println!("cargo:warning=OpenVDB (brew install openvdb) not found: voxel remesh falls back to the pure-Rust approximation");
        return;
    };
    let mut b = cc::Build::new();
    b.cpp(true).file("csrc/vdb_shim.cpp").flag("-std=c++17").flag("-O2").flag("-ffp-contract=on").warnings(false);
    for lib in ["openvdb", "onetbb", "boost", "imath", "c-blosc"] {
        b.include(format!("{brew}/opt/{lib}/include"));
    }
    b.include(format!("{brew}/include"));
    b.compile("vdbshim");
    for lib in ["openvdb", "onetbb", "c-blosc", "imath"] {
        println!("cargo:rustc-link-search=native={brew}/opt/{lib}/lib");
    }
    println!("cargo:rustc-link-lib=openvdb");
    println!("cargo:rustc-link-lib=tbb");
    println!("cargo:rustc-link-lib=blosc");
    println!("cargo:rustc-link-lib=Imath-3_2");
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-cfg=has_openvdb");
}
