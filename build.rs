// OpenSubdiv 3.6.0 (the version bundled with Blender 4.2) + a small C ABI shim, see csrc/osd_shim.cpp.
// Default clang flags (FP contraction on) reproduce Blender's results best; OSD_FLAGS adds experiment flags (e.g. -ffp-contract=off).
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
    let mut files = vec![];
    collect(Path::new("csrc/opensubdiv"), &mut files);
    files.push("csrc/osd_shim.cpp".into());
    let mut b = cc::Build::new();
    b.cpp(true).std("c++14").include("csrc").warnings(false).files(&files);
    for x in std::env::var("OSD_FLAGS").unwrap_or_default().split_whitespace() {
        b.flag(x);
    }
    b.compile("osdshim");
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-env-changed=OSD_FLAGS");
}
