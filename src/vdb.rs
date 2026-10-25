//! OpenVDB voxel remesh (Blender's REMESH modifier, VOXEL mode) through csrc/vdb_shim.cpp; `None` when OpenVDB isn't linked.
#[cfg(has_openvdb)]
extern "C" {
    fn vdb_remesh_voxel(pts: *const f32, npts: i32, tris: *const u32, ntri: i32, voxel: f32, adapt: f32, iso: f32, out_pts: *mut *mut f32, out_npts: *mut i32, out_quads: *mut *mut u32, out_nquads: *mut i32, out_tris: *mut *mut u32, out_ntris: *mut i32) -> i32;
    fn vdb_free(p: *mut std::ffi::c_void);
}

/// BKE_mesh_remesh_voxel through the linked OpenVDB (vertex / face order as Blender's: quads first, then triangles)
#[cfg(has_openvdb)]
pub fn remesh_voxel(pos: &[[f32; 3]], tris: &[[u32; 3]], voxel: f32) -> Option<(Vec<[f32; 3]>, Vec<Vec<u32>>)> {
    let flat: Vec<f32> = pos.iter().flat_map(|p| p.iter().copied()).collect();
    let tf: Vec<u32> = tris.iter().flat_map(|t| t.iter().copied()).collect();
    let (mut op, mut oq, mut ot) = (std::ptr::null_mut::<f32>(), std::ptr::null_mut::<u32>(), std::ptr::null_mut::<u32>());
    let (mut np, mut nq, mut nt) = (0i32, 0i32, 0i32);
    unsafe {
        vdb_remesh_voxel(flat.as_ptr(), pos.len() as i32, tf.as_ptr(), tris.len() as i32, voxel, 0.0, 0.0, &mut op, &mut np, &mut oq, &mut nq, &mut ot, &mut nt);
        let v: Vec<[f32; 3]> = (0..np as usize).map(|i| [*op.add(3 * i), *op.add(3 * i + 1), *op.add(3 * i + 2)]).collect();
        let mut f: Vec<Vec<u32>> = vec![];
        for i in 0..nq as usize {
            let q = [*oq.add(4 * i), *oq.add(4 * i + 1), *oq.add(4 * i + 2), *oq.add(4 * i + 3)];
            f.push(vec![q[0], q[3], q[2], q[1]]);
        }
        for i in 0..nt as usize {
            let t = [*ot.add(3 * i), *ot.add(3 * i + 1), *ot.add(3 * i + 2)];
            f.push(vec![t[2], t[1], t[0]]);
        }
        vdb_free(op as *mut _);
        vdb_free(oq as *mut _);
        vdb_free(ot as *mut _);
        Some((v, f))
    }
}

#[cfg(not(has_openvdb))]
pub fn remesh_voxel(_pos: &[[f32; 3]], _tris: &[[u32; 3]], _voxel: f32) -> Option<(Vec<[f32; 3]>, Vec<Vec<u32>>)> {
    None
}
