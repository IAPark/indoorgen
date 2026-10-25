// C/C++ shims under csrc/:
//  - OpenSubdiv 3.6.0 (the version bundled with Blender 4.2) + C ABI shim csrc/osd_shim.cpp. Default clang flags (FP contraction on)
//    reproduce Blender best; OSD_FLAGS adds experiment flags (e.g. -ffp-contract=off).
//  - OpenVDB voxel remesh (Blender's REMESH modifier, VOXEL mode) csrc/vdb_shim.cpp, if `brew install openvdb` is present (cfg has_openvdb).
use std::path::Path;

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map(|x| x == "cpp").unwrap_or(false) {
            out.push(p);
        }
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_openvdb)");
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OSD_FLAGS");
    // OpenSubdiv
    let mut files = vec![];
    let ext = std::env::var("OSD_EXTERNAL").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=OSD_EXTERNAL");
    collect(Path::new("csrc/opensubdiv"), &mut files);
    if !ext.is_empty() {
        let keep = std::env::var("OSD_EXT_KEEP").unwrap_or_default(); // comma list of path substrings still compiled from our sources
        println!("cargo:rerun-if-env-changed=OSD_EXT_KEEP");
        files.retain(|f| keep.split(',').any(|k| !k.is_empty() && f.to_string_lossy().contains(k)));
        println!("cargo:rustc-link-search=native={ext}");
        println!("cargo:rustc-link-lib=dylib=osdCPU");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{ext}");
    }
    files.push("csrc/osd_shim.cpp".into());
    let nofma: Vec<String> = std::env::var("OSD_NOFMA").unwrap_or_default().split(',').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect();
    println!("cargo:rerun-if-env-changed=OSD_NOFMA");
    let (off, on): (Vec<_>, Vec<_>) = files.iter().cloned().partition(|f| nofma.iter().any(|n| f.to_string_lossy().contains(n.as_str())));
    for (name, set, contract) in [("osdshim", on, false), ("osdshim_off", off, true)] {
        if set.is_empty() {
            continue;
        }
        let mut b = cc::Build::new();
        b.cpp(true).std("c++14").include("csrc").warnings(false).files(&set);
        for x in std::env::var("OSD_FLAGS").unwrap_or_default().split_whitespace() {
            b.flag(x);
        }
        if contract {
            b.flag("-ffp-contract=off");
        }
        b.compile(name);
    }
    // OpenVDB
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
    for l in ["openvdb", "tbb", "blosc", "Imath-3_2", "z", "c++"] {
        println!("cargo:rustc-link-lib={l}");
    }
    println!("cargo:rustc-cfg=has_openvdb");
}
