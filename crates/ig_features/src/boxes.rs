//! Small mesh helpers shared by the features: axis-aligned boxes with per-face metre UVs (1 uv unit = 1 m, so Infinigen's brick / tile
//! shaders, which read the UV map, get real-world course heights) and cylinders along an arbitrary axis.
use ig_core::mesh::{Mat4, Mesh, V3};

/// Face selector bits for [`add_box`]: -x, +x, -y, +y, -z, +z.
pub const NX: u8 = 1;
pub const PX: u8 = 2;
pub const NY: u8 = 4;
pub const PY: u8 = 8;
pub const NZ: u8 = 16;
pub const PZ: u8 = 32;
pub const ALL: u8 = 63;

/// Add the faces of the box [lo, hi] selected by `faces` (bits above). Outward CCW winding, one quad per face with its own 4 vertices
/// (flat normals), UV = the two in-plane coordinates in metres (vertical faces: u horizontal, v = z).
pub fn add_box(m: &mut Mesh, lo: V3, hi: V3, faces: u8) {
    let c = |x: usize, y: usize, z: usize| [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }];
    // (bit, quad, uv axes)
    let quads: [(u8, [V3; 4], (usize, usize)); 6] = [
        (NX, [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)], (1, 2)),
        (PX, [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)], (1, 2)),
        (NY, [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)], (0, 2)),
        (PY, [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)], (0, 2)),
        (NZ, [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)], (0, 1)),
        (PZ, [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], (0, 1)),
    ];
    pad_uv(m);
    for (bit, q, (ua, va)) in quads {
        if faces & bit == 0 { continue }
        m.add_quad(q[0], q[1], q[2], q[3]);
        for p in q { m.uv.push([p[ua], p[va]]); }
    }
}

/// bring `uv` to one entry per vertex before appending vertices that carry their own uv
pub fn pad_uv(m: &mut Mesh) {
    while m.uv.len() < m.pos.len() { m.uv.push([0.0, 0.0]); }
}

/// Add a single quad (CCW from the viewer side) with explicit metre UVs from the two axes `ua`, `va`.
pub fn add_quad_uv(m: &mut Mesh, q: [V3; 4], ua: usize, va: usize) {
    pad_uv(m);
    m.add_quad(q[0], q[1], q[2], q[3]);
    for p in q { m.uv.push([p[ua], p[va]]); }
}

/// closed cylinder from `a` to `b` (radius r, n sides) with UV (angle * r, length) on the side, flat caps
pub fn add_cylinder(m: &mut Mesh, a: V3, b: V3, r: f64, n: usize) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let mut cyl = Mesh::cylinder(0.0, 0.0, 0.0, len, r, n);
    // z axis -> d
    let z = [d[0] / len, d[1] / len, d[2] / len];
    let up = if z[2].abs() > 0.99 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let x = norm(cross(up, z));
    let y = cross(z, x);
    let mt = Mat4([[x[0], y[0], z[0], a[0]], [x[1], y[1], z[1], a[1]], [x[2], y[2], z[2], a[2]], [0.0, 0.0, 0.0, 1.0]]);
    let local = cyl.pos.clone();
    cyl.transform(&mt);
    let uv: Vec<[f64; 2]> = local.iter().map(|p| [p[1].atan2(p[0]) * r, p[2]]).collect();
    pad_uv(m);
    let base = m.pos.len() as u32;
    m.pos.extend(cyl.pos.iter().cloned());
    m.faces.extend(cyl.faces.iter().map(|f| f.iter().map(|i| i + base).collect::<Vec<u32>>()));
    m.uv.extend(uv);
}

fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn norm(a: V3) -> V3 { let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt(); [a[0] / l, a[1] / l, a[2] / l] }
