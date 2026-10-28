//! Cloth jobs on library soft bodies: Jolt Physics (MIT) or Bullet (zlib), through csrc/cloth_shim.cpp. Units as
//! `ig_kit::cloth` (Blender CLOTH settings, simulation time in frames); colliders are static triangle meshes.
use std::os::raw::c_int;

#[repr(C)]
struct ClothJobC {
    nv: c_int,
    pos: *const f64,
    ntri: c_int,
    tri: *const c_int,
    pin: *const f64,
    mass: f64,
    tension: f64,
    compression: f64,
    shear: f64,
    bending: f64,
    tension_damp: f64,
    shear_damp: f64,
    air_damping: f64,
    gravity: f64,
    pressure: f64,
    distance_min: f64,
    end_frame: c_int,
    self_collision: c_int,
    avg_len: f64,
    ncol: c_int,
    col_nv: *const c_int,
    col_pos: *const *const f64,
    col_ntri: *const c_int,
    col_tri: *const *const c_int,
}

extern "C" {
    fn cloth_jolt(j: *const ClothJobC, out: *mut f64) -> c_int;
    fn cloth_bullet(j: *const ClothJobC, out: *mut f64) -> c_int;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backend {
    Jolt,
    Bullet,
}

/// Material and run settings (Blender CLOTH names; pressure already includes the caller's calibration).
#[derive(Clone, Debug)]
pub struct Params {
    pub mass: f64,
    pub tension: f64,
    pub compression: f64,
    pub shear: f64,
    pub bending: f64,
    pub tension_damp: f64,
    pub shear_damp: f64,
    pub air_damping: f64,
    pub gravity: f64,
    pub pressure: f64,
    pub distance_min: f64,
    pub end_frame: u32,
    pub self_collision: bool,
    pub avg_len: f64,
}

/// A static collider as a triangle mesh.
pub struct Mesh<'a> {
    pub pos: &'a [[f64; 3]],
    pub tris: &'a [[u32; 3]],
}

pub fn simulate(backend: Backend, pos: &[[f64; 3]], tris: &[[u32; 3]], pin: &[f64], p: &Params, colliders: &[Mesh]) -> Vec<[f64; 3]> {
    let flat: Vec<f64> = pos.iter().flat_map(|v| v.iter().copied()).collect();
    let tri: Vec<c_int> = tris.iter().flat_map(|t| t.iter().map(|&i| i as c_int)).collect();
    let cpos: Vec<Vec<f64>> = colliders.iter().map(|c| c.pos.iter().flat_map(|v| v.iter().copied()).collect()).collect();
    let ctri: Vec<Vec<c_int>> = colliders.iter().map(|c| c.tris.iter().flat_map(|t| t.iter().map(|&i| i as c_int)).collect()).collect();
    let col_nv: Vec<c_int> = colliders.iter().map(|c| c.pos.len() as c_int).collect();
    let col_ntri: Vec<c_int> = colliders.iter().map(|c| c.tris.len() as c_int).collect();
    let col_pos: Vec<*const f64> = cpos.iter().map(|v| v.as_ptr()).collect();
    let col_tri: Vec<*const c_int> = ctri.iter().map(|v| v.as_ptr()).collect();
    let job = ClothJobC {
        nv: pos.len() as c_int,
        pos: flat.as_ptr(),
        ntri: tris.len() as c_int,
        tri: tri.as_ptr(),
        pin: if pin.len() == pos.len() { pin.as_ptr() } else { std::ptr::null() },
        mass: p.mass,
        tension: p.tension,
        compression: p.compression,
        shear: p.shear,
        bending: p.bending,
        tension_damp: p.tension_damp,
        shear_damp: p.shear_damp,
        air_damping: p.air_damping,
        gravity: p.gravity,
        pressure: p.pressure,
        distance_min: p.distance_min,
        end_frame: p.end_frame as c_int,
        self_collision: p.self_collision as c_int,
        avg_len: p.avg_len,
        ncol: colliders.len() as c_int,
        col_nv: col_nv.as_ptr(),
        col_pos: col_pos.as_ptr(),
        col_ntri: col_ntri.as_ptr(),
        col_tri: col_tri.as_ptr(),
    };
    let mut out = vec![0.0f64; 3 * pos.len()];
    unsafe {
        match backend {
            Backend::Jolt => cloth_jolt(&job, out.as_mut_ptr()),
            Backend::Bullet => cloth_bullet(&job, out.as_mut_ptr()),
        };
    }
    out.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}
