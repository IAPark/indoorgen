//! OpenSubdiv 3.6.0 limit evaluation as Blender 4.2's CPU subsurf modifier does it (csrc/osd_shim.cpp).
use std::os::raw::{c_float, c_int, c_void};

extern "C" {
    fn osd_create(nv: c_int, pos: *const c_float, nf: c_int, fsz: *const c_int, fv: *const c_int, level: c_int, adaptive: c_int, simple: c_int, corner: c_int) -> *mut c_void;
    fn osd_eval(h: *mut c_void, n: c_int, ptex: *const c_int, u: *const c_float, v: *const c_float, out: *mut c_float);
    fn osd_free(h: *mut c_void);
}

pub struct Osd {
    h: *mut c_void,
    /// ptex face index of the first ptex face of every coarse face
    pub face_ptex_offset: Vec<i32>,
}

impl Osd {
    /// `pos`: float32 coarse positions, `faces`: polygons. `simple`: SIMPLE subdivision (Catmull-Clark with all edges / vertices infinitely sharp,
    /// Blender's BUGGY_SIMPLE_SCHEME_WORKAROUND). Loose vertices / edges must have been removed by the caller.
    pub fn new(pos: &[[f32; 3]], faces: &[Vec<u32>], level: usize, adaptive: bool, simple: bool) -> Option<Osd> {
        let flat: Vec<f32> = pos.iter().flat_map(|p| p.iter().cloned()).collect();
        let sizes: Vec<c_int> = faces.iter().map(|f| f.len() as c_int).collect();
        let verts: Vec<c_int> = faces.iter().flat_map(|f| f.iter().map(|&v| v as c_int)).collect();
        let h = unsafe { osd_create(pos.len() as c_int, flat.as_ptr(), faces.len() as c_int, sizes.as_ptr(), verts.as_ptr(), level as c_int, adaptive as c_int, simple as c_int, 0) };
        if h.is_null() {
            return None;
        }
        let mut off = Vec::with_capacity(faces.len());
        let mut acc = 0i32;
        for f in faces {
            off.push(acc);
            acc += if f.len() == 4 { 1 } else { f.len() as i32 };
        }
        Some(Osd { h, face_ptex_offset: off })
    }

    pub fn eval(&self, q: &[(i32, f32, f32)]) -> Vec<[f32; 3]> {
        let ptex: Vec<c_int> = q.iter().map(|x| x.0).collect();
        let u: Vec<f32> = q.iter().map(|x| x.1).collect();
        let v: Vec<f32> = q.iter().map(|x| x.2).collect();
        let mut out = vec![0f32; 3 * q.len()];
        unsafe { osd_eval(self.h, q.len() as c_int, ptex.as_ptr(), u.as_ptr(), v.as_ptr(), out.as_mut_ptr()) };
        out.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
    }
}

impl Drop for Osd {
    fn drop(&mut self) {
        unsafe { osd_free(self.h) }
    }
}
