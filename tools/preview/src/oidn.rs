//! Open Image Denoise, loaded when the program starts if the library is there: the learned filter Blender's own renders go through. It takes the light as traced, where the noise is still separate from pixel to pixel (upsampled, it took the noise for detail). No library, RT_NOOIDN=1, or the web: the a-trous filter in rt_post.wgsl.
use libloading::{Library, Symbol};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::OnceLock;
type P = *mut c_void;
struct Oidn { lib: Library, dev: usize }
static O: OnceLock<Option<Oidn>> = OnceLock::new();
fn get() -> Option<&'static Oidn> {
    O.get_or_init(|| {
        if std::env::var_os("RT_NOOIDN").is_some() { return None; }
        let home = std::env::var("HOME").unwrap_or_default();
        let bpy = format!("{home}/work/infinigen-ref/.venv52/lib/python3.13/site-packages/bpy/lib/libOpenImageDenoise.dylib");
        let paths = [std::env::var("OIDN_LIB").unwrap_or_default(), "/opt/homebrew/lib/libOpenImageDenoise.dylib".into(), "/usr/local/lib/libOpenImageDenoise.so".into(), bpy];
        let lib = paths.iter().filter(|p| !p.is_empty()).find_map(|p| unsafe { Library::new(p).ok() })?;
        let dev = unsafe {
            let new = lib.get::<unsafe extern "C" fn(c_int) -> P>(b"oidnNewDevice\0").ok()?;
            let commit = lib.get::<unsafe extern "C" fn(P)>(b"oidnCommitDevice\0").ok()?;
            // (1 the CPU; 5 Metal, which wants a real Apple GPU: a virtual machine's is turned down, and the CPU does it)
            let mut d = new(std::env::var("OIDN_DEV").ok().and_then(|v| v.parse().ok()).unwrap_or(1));
            if d.is_null() { d = new(1); }
            commit(d);
            let err = lib.get::<unsafe extern "C" fn(P, *mut *const c_char) -> c_int>(b"oidnGetDeviceError\0").ok()?;
            if err(d, std::ptr::null_mut()) != 0 { d = new(1); commit(d); }
            d as usize
        };
        Some(Oidn { lib, dev })
    }).as_ref()
}
/// the same network on the GPU through MPSGraph (mpsdn/mpsdn.swift), with OIDN's published weights: for a GPU OIDN's own Metal device turns down
struct Mps { _lib: Library, run: unsafe extern "C" fn(*const f32, *const f32, i32, i32, *mut f32) -> i32 }
static M: OnceLock<Option<Mps>> = OnceLock::new();
fn mps(quality: i32) -> Option<&'static Mps> {
    M.get_or_init(|| {
        if std::env::var("RT_DN").as_deref() != Ok("mps") { return None; }
        let home = std::env::var("HOME").unwrap_or_default();
        let lib = unsafe { Library::new(std::env::var("MPSDN_LIB").unwrap_or(format!("{home}/work/ext/oidn/libmpsdn.dylib"))).ok()? };
        let wf = std::env::var("MPSDN_W").unwrap_or(format!("{home}/work/ext/oidn/w/rt_hdr_calb_cnrm{}.tza", if quality <= 4 { "_small" } else { "" }));
        let wc = std::ffi::CString::new(wf).ok()?;
        unsafe {
            if lib.get::<unsafe extern "C" fn(*const c_char) -> i32>(b"mpsdn_init\0").ok()?(wc.as_ptr()) == 0 { eprintln!("mpsdn: no GPU or weights"); return None; }
            let run = *lib.get::<unsafe extern "C" fn(*const f32, *const f32, i32, i32, *mut f32) -> i32>(b"mpsdn_run\0").ok()?;
            Some(Mps { _lib: lib, run })
        }
    }).as_ref()
}
pub fn available() -> bool { mps(4).is_some() || get().is_some() }
/// `color`, `normal`: RGB f32, w x h. Quality: 4 fast, 5 balanced, 6 high. Returns the filtered picture
pub fn denoise(color: &[f32], normal: &[f32], w: usize, h: usize, quality: i32) -> Option<Vec<f32>> {
    if let Some(m) = mps(quality) {
        let mut out = vec![0f32; w * h * 3];
        if unsafe { (m.run)(color.as_ptr(), normal.as_ptr(), w as i32, h as i32, out.as_mut_ptr()) } != 0 { return Some(out); }
    }
    let o = get()?;
    let albedo = vec![0.5f32; w * h * 3]; // (flat: this is light, with no surface colours in it. The network wants an albedo before it takes normals)
    let mut out = vec![0f32; w * h * 3];
    unsafe {
        let dev = o.dev as P;
        let new: Symbol<unsafe extern "C" fn(P, *const c_char) -> P> = o.lib.get(b"oidnNewFilter\0").ok()?;
        let img: Symbol<unsafe extern "C" fn(P, *const c_char, *const c_void, c_int, usize, usize, usize, usize, usize)> = o.lib.get(b"oidnSetSharedFilterImage\0").ok()?;
        let setb: Symbol<unsafe extern "C" fn(P, *const c_char, bool)> = o.lib.get(b"oidnSetFilterBool\0").ok()?;
        let seti: Symbol<unsafe extern "C" fn(P, *const c_char, c_int)> = o.lib.get(b"oidnSetFilterInt\0").ok()?;
        let commit: Symbol<unsafe extern "C" fn(P)> = o.lib.get(b"oidnCommitFilter\0").ok()?;
        let exec: Symbol<unsafe extern "C" fn(P)> = o.lib.get(b"oidnExecuteFilter\0").ok()?;
        let rel: Symbol<unsafe extern "C" fn(P)> = o.lib.get(b"oidnReleaseFilter\0").ok()?;
        let err: Symbol<unsafe extern "C" fn(P, *mut *const c_char) -> c_int> = o.lib.get(b"oidnGetDeviceError\0").ok()?;
        let f = new(dev, c"RT".as_ptr());
        const F3: c_int = 3;
        img(f, c"color".as_ptr(), color.as_ptr() as _, F3, w, h, 0, 0, 0);
        img(f, c"albedo".as_ptr(), albedo.as_ptr() as _, F3, w, h, 0, 0, 0);
        img(f, c"normal".as_ptr(), normal.as_ptr() as _, F3, w, h, 0, 0, 0);
        img(f, c"output".as_ptr(), out.as_mut_ptr() as _, F3, w, h, 0, 0, 0);
        setb(f, c"hdr".as_ptr(), true); setb(f, c"cleanAux".as_ptr(), true); seti(f, c"quality".as_ptr(), quality);
        commit(f); exec(f); rel(f);
        let mut m: *const c_char = std::ptr::null();
        if err(dev, &mut m) != 0 { eprintln!("oidn: {}", CStr::from_ptr(m).to_string_lossy()); return None; }
    }
    Some(out)
}
