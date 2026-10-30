//! Bit-level emulation of the float32 Blender / mathutils object-transform pipeline that the python solver relies on
//! (object location / euler / scale, matrix_world, decompose, quaternion conversions, vertex / polygon-normal transforms).
//! Matrices follow Blender's C layout: `m[col][row]`.

pub type V3 = [f32; 3];
pub type M3 = [[f32; 3]; 3];
pub type M4 = [[f32; 4]; 4];

#[inline]
fn dot_v3(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// normalize_v3_v3: returns (normalized, original length)
pub fn normalize_v3(a: V3) -> (V3, f32) {
    let d = dot_v3(a, a);
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        let inv = 1.0f32 / l;
        ([a[0] * inv, a[1] * inv, a[2] * inv], l)
    } else {
        ([0.0; 3], 0.0)
    }
}

/// mathutils `A @ B` (row-major semantics; here m[col][row]): double accumulation of float products
pub fn mat_mul_mathutils(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0f32; 3]; 3];
    for c in 0..3 {
        for row in 0..3 {
            let mut d = 0.0f64;
            for k in 0..3 {
                d += (a[k][row] * b[c][k]) as f64;
            }
            r[c][row] = d as f32;
        }
    }
    r
}

pub fn mul_m3_m3m3(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = b[i][0] * a[0][j] + b[i][1] * a[1][j] + b[i][2] * a[2][j];
        }
    }
    r
}

/// eul_to_mat3 (XYZ order); double math, float result
pub fn eul_to_mat3(e: V3) -> M3 {
    let (ci, cj, ch) = ((e[0] as f64).cos(), (e[1] as f64).cos(), (e[2] as f64).cos());
    let (si, sj, sh) = ((e[0] as f64).sin(), (e[1] as f64).sin(), (e[2] as f64).sin());
    let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
    let mut m = [[0.0f32; 3]; 3];
    m[0][0] = (cj * ch) as f32;
    m[1][0] = (sj * sc - cs) as f32;
    m[2][0] = (sj * cc + ss) as f32;
    m[0][1] = (cj * sh) as f32;
    m[1][1] = (sj * ss + cc) as f32;
    m[2][1] = (sj * cs - sc) as f32;
    m[0][2] = (-sj) as f32;
    m[1][2] = (cj * si) as f32;
    m[2][2] = (cj * ci) as f32;
    m
}

/// quat_to_mat3 (w,x,y,z)
pub fn quat_to_mat3(q: [f32; 4]) -> M3 {
    let sq2 = std::f64::consts::SQRT_2;
    let q0 = sq2 * q[0] as f64;
    let q1 = sq2 * q[1] as f64;
    let q2 = sq2 * q[2] as f64;
    let q3 = sq2 * q[3] as f64;
    let qda = q0 * q1;
    let qdb = q0 * q2;
    let qdc = q0 * q3;
    let qaa = q1 * q1;
    let qab = q1 * q2;
    let qac = q1 * q3;
    let qbb = q2 * q2;
    let qbc = q2 * q3;
    let qcc = q3 * q3;
    let mut m = [[0.0f32; 3]; 3];
    m[0][0] = (1.0 - qbb - qcc) as f32;
    m[0][1] = (qdc + qab) as f32;
    m[0][2] = (-qdb + qac) as f32;
    m[1][0] = (-qdc + qab) as f32;
    m[1][1] = (1.0 - qaa - qcc) as f32;
    m[1][2] = (qda + qbc) as f32;
    m[2][0] = (qdb + qac) as f32;
    m[2][1] = (-qda + qbc) as f32;
    m[2][2] = (1.0 - qaa - qbb) as f32;
    m
}

fn normalize_qt(q: &mut [f32; 4]) {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len != 0.0 {
        let inv = 1.0f32 / len;
        for v in q.iter_mut() {
            *v *= inv;
        }
    } else {
        *q = [1.0, 0.0, 0.0, 0.0];
    }
}

/// mat3_normalized_to_quat (Mike Day's method, Blender >= 2.9)
pub fn mat3_normalized_to_quat(m: &M3) -> [f32; 4] {
    let mut q = [0.0f32; 4];
    if m[2][2] < 0.0 {
        if m[0][0] > m[1][1] {
            let trace = 1.0f32 + m[0][0] - m[1][1] - m[2][2];
            let mut s = 2.0f32 * trace.sqrt();
            if m[1][2] < m[2][1] {
                s = -s;
            }
            q[1] = 0.25f32 * s;
            s = 1.0f32 / s;
            q[0] = (m[1][2] - m[2][1]) * s;
            q[2] = (m[0][1] + m[1][0]) * s;
            q[3] = (m[2][0] + m[0][2]) * s;
            if trace == 1.0 && q[0] == 0.0 && q[2] == 0.0 && q[3] == 0.0 {
                q[1] = 1.0;
            }
        } else {
            let trace = 1.0f32 - m[0][0] + m[1][1] - m[2][2];
            let mut s = 2.0f32 * trace.sqrt();
            if m[2][0] < m[0][2] {
                s = -s;
            }
            q[2] = 0.25f32 * s;
            s = 1.0f32 / s;
            q[0] = (m[2][0] - m[0][2]) * s;
            q[1] = (m[0][1] + m[1][0]) * s;
            q[3] = (m[1][2] + m[2][1]) * s;
            if trace == 1.0 && q[0] == 0.0 && q[1] == 0.0 && q[3] == 0.0 {
                q[2] = 1.0;
            }
        }
    } else if m[0][0] < -m[1][1] {
        let trace = 1.0f32 - m[0][0] - m[1][1] + m[2][2];
        let mut s = 2.0f32 * trace.sqrt();
        if m[0][1] < m[1][0] {
            s = -s;
        }
        q[3] = 0.25f32 * s;
        s = 1.0f32 / s;
        q[0] = (m[0][1] - m[1][0]) * s;
        q[1] = (m[2][0] + m[0][2]) * s;
        q[2] = (m[1][2] + m[2][1]) * s;
        if trace == 1.0 && q[0] == 0.0 && q[1] == 0.0 && q[2] == 0.0 {
            q[3] = 1.0;
        }
    } else {
        let trace = 1.0f32 + m[0][0] + m[1][1] + m[2][2];
        let mut s = 2.0f32 * trace.sqrt();
        q[0] = 0.25f32 * s;
        s = 1.0f32 / s;
        q[1] = (m[1][2] - m[2][1]) * s;
        q[2] = (m[2][0] - m[0][2]) * s;
        q[3] = (m[0][1] - m[1][0]) * s;
        if trace == 1.0 && q[1] == 0.0 && q[2] == 0.0 && q[3] == 0.0 {
            q[0] = 1.0;
        }
    }
    // (the final normalize_qt of the C source is not observable in the reference outputs)
    let _ = normalize_qt;
    q
}

fn det_m3(m: &M3) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// mat4_to_loc_rot_size: (loc, normalized rot, size)
pub fn mat4_to_loc_rot_size(w: &M4) -> (V3, M3, V3) {
    let mat3: M3 = [[w[0][0], w[0][1], w[0][2]], [w[1][0], w[1][1], w[1][2]], [w[2][0], w[2][1], w[2][2]]];
    let loc = [w[3][0], w[3][1], w[3][2]];
    let mut rot = [[0.0f32; 3]; 3];
    let mut size = [0.0f32; 3];
    for i in 0..3 {
        let (n, l) = normalize_v3(mat3[i]);
        rot[i] = n;
        size[i] = l;
    }
    if det_m3(&rot) < 0.0 {
        for i in 0..3 {
            for j in 0..3 {
                rot[i][j] = -rot[i][j];
            }
            size[i] = -size[i];
        }
    }
    (loc, rot, size)
}

/// Matrix.decompose(): (loc, quat, scale)
pub fn decompose(w: &M4) -> (V3, [f32; 4], V3) {
    let (loc, rot, size) = mat4_to_loc_rot_size(w);
    (loc, mat3_normalized_to_quat(&rot), size)
}

/// mat3_normalized_to_eulO (XYZ), picking the solution with the smaller component sum
pub fn mat3_normalized_to_eul(m: &M3) -> V3 {
    let var = crate::env_str!("EULVAR").unwrap_or_default();
    let cy = if var.contains('d') { (m[0][0] as f64).hypot(m[0][1] as f64) as f32 } else { m[0][0].hypot(m[0][1]) };
    let (mut e1, mut e2);
    if cy > 3.756e-5f32 {
        e1 = [m[1][2].atan2(m[2][2]), (-m[0][2]).atan2(cy), m[0][1].atan2(m[0][0])];
        e2 = [(-m[1][2]).atan2(-m[2][2]), (-m[0][2]).atan2(-cy), (-m[0][1]).atan2(-m[0][0])];
    } else {
        e1 = [(-m[2][1]).atan2(m[1][1]), (-m[0][2]).atan2(cy), 0.0];
        e2 = e1;
    }
    let _ = (&mut e1, &mut e2);
    let d1 = e1[0].abs() + e1[1].abs() + e1[2].abs();
    let d2 = e2[0].abs() + e2[1].abs() + e2[2].abs();
    if d1 > d2 {
        e2
    } else {
        e1
    }
}

/// Blender object transform (location / XYZ euler / scale)
#[derive(Clone, Copy, Debug)]
pub struct Obj32 {
    pub loc: V3,
    pub eul: V3,
    pub scale: V3,
}

impl Obj32 {
    pub fn new(loc: V3, eul: V3) -> Obj32 {
        Obj32 { loc, eul, scale: [1.0; 3] }
    }

    /// BKE_object_to_mat4
    pub fn matrix_world(&self) -> M4 {
        let r = eul_to_mat3(self.eul);
        let s: M3 = [[self.scale[0], 0.0, 0.0], [0.0, self.scale[1], 0.0], [0.0, 0.0, self.scale[2]]];
        let m = mul_m3_m3m3(&r, &s);
        [[m[0][0], m[0][1], m[0][2], 0.0], [m[1][0], m[1][1], m[1][2], 0.0], [m[2][0], m[2][1], m[2][2], 0.0], [self.loc[0], self.loc[1], self.loc[2], 1.0]]
    }

    /// `obj.matrix_world = M` (BKE_object_apply_mat4 without compat euler)
    pub fn set_matrix_world(&mut self, w: &M4) {
        let (loc, rot, size) = mat4_to_loc_rot_size(w);
        self.loc = loc;
        self.scale = size;
        let var = crate::env_str!("EULVAR").unwrap_or_default();
        if var.contains('m') {
            self.eul = mat3_normalized_to_eul(&rot);
        } else {
            let mut q = mat3_normalized_to_quat(&rot);
            if var.contains('n') {
                normalize_qt(&mut q);
            }
            self.eul = mat3_normalized_to_eul(&quat_to_mat3(q));
        }
    }

    pub fn translate(&mut self, v: [f64; 3]) {
        for k in 0..3 {
            self.loc[k] += v[k] as f32;
        }
    }

    /// iu.rotate(scene, a, axis, angle): decompose, left-multiply by the rotation, rebuild with LocRotScale, assign matrix_world
    pub fn rotate(&mut self, axis: [f64; 3], angle: f64) {
        self.rotate_dbg(axis, angle);
    }

    pub fn rotate_dbg(&mut self, axis: [f64; 3], angle: f64) -> RotDbg {
        let w = self.matrix_world();
        let (loc, quat, scale) = decompose(&w);
        let rot = quat_to_mat3(quat);
        // trimesh rotation_matrix in float64 -> mathutils float32
        let rm = rotation_matrix(angle, axis);
        let t3: M3 = [[rm[0][0] as f32, rm[1][0] as f32, rm[2][0] as f32], [rm[0][1] as f32, rm[1][1] as f32, rm[2][1] as f32], [rm[0][2] as f32, rm[1][2] as f32, rm[2][2] as f32]];
        let prod = mat_mul_mathutils(&t3, &rot);
        let q = mat3_normalized_to_quat_unnormalized(&prod);
        let r2 = quat_to_mat3(q);
        let s: M3 = [[scale[0], 0.0, 0.0], [0.0, scale[1], 0.0], [0.0, 0.0, scale[2]]];
        let m = mul_m3_m3m3(&r2, &s);
        let nm: M4 = [[m[0][0], m[0][1], m[0][2], 0.0], [m[1][0], m[1][1], m[1][2], 0.0], [m[2][0], m[2][1], m[2][2], 0.0], [loc[0], loc[1], loc[2], 1.0]];
        self.set_matrix_world(&nm);
        RotDbg { tm: t3, quat, scale, rm: rot, prod, q2: q, lrs: nm }
    }
}

#[derive(Debug)]
pub struct RotDbg {
    pub tm: M3,
    pub quat: [f32; 4],
    pub scale: V3,
    pub rm: M3,
    pub prod: M3,
    pub q2: [f32; 4],
    pub lrs: M4,
}

/// Matrix.to_quaternion() for a 4x4: copy 3x3, mat3_to_quat (normalizes columns first)
fn mat3_normalized_to_quat_unnormalized(m: &M3) -> [f32; 4] {
    let mut u = [[0.0f32; 3]; 3];
    for i in 0..3 {
        u[i] = normalize_v3(m[i]).0;
    }
    mat3_normalized_to_quat(&u)
}

/// trimesh.transformations.rotation_matrix(angle, direction) 3x3 part, row-major [row][col], float64
pub fn rotation_matrix(angle: f64, axis: [f64; 3]) -> [[f64; 3]; 3] {
    let (sa, ca) = (angle.sin(), angle.cos());
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let d = [axis[0] / n, axis[1] / n, axis[2] / n];
    // R = diag(cosa) + outer(d, d) * (1 - cosa) + [[0,-z,y],[z,0,-x],[-y,x,0]] * sina
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = d[i] * d[j] * (1.0 - ca);
        }
        r[i][i] += ca;
    }
    let (x, y, z) = (d[0] * sa, d[1] * sa, d[2] * sa);
    r[0][1] -= z;
    r[0][2] += y;
    r[1][0] += z;
    r[1][2] -= x;
    r[2][0] -= y;
    r[2][1] += x;
    r
}

/// mathutils `Matrix(4x4) @ Vector(3)`: double accumulation of float products, float result
pub fn mat4_mul_point(w: &M4, v: V3) -> V3 {
    let mut out = [0.0f32; 3];
    for row in 0..3 {
        let mut d = 0.0f64;
        d += (w[0][row] * v[0]) as f64;
        d += (w[1][row] * v[1]) as f64;
        d += (w[2][row] * v[2]) as f64;
        d += (w[3][row] * 1.0f32) as f64;
        out[row] = d as f32;
    }
    out
}

/// butil.global_polygon_normal(obj, polygon): decompose -> quaternion -> matrix, rot @ local normal, divided by its float64 norm
pub fn global_polygon_normal(w: &M4, local_normal: V3, rev: bool) -> V3 {
    let (_, quat, _) = decompose(w);
    let r = quat_to_mat3(quat);
    let mut n = [0.0f32; 3];
    for row in 0..3 {
        let mut d = 0.0f64;
        d += (r[0][row] * local_normal[0]) as f64;
        d += (r[1][row] * local_normal[1]) as f64;
        d += (r[2][row] * local_normal[2]) as f64;
        n[row] = d as f32;
    }
    if rev {
        n = [-n[0], -n[1], -n[2]];
    }
    let nn = (n[0] as f64 * n[0] as f64 + n[1] as f64 * n[1] as f64 + n[2] as f64 * n[2] as f64).sqrt();
    // Vector / float: multiply by the reciprocal in float32
    let inv = 1.0f32 / (nn as f32);
    [n[0] * inv, n[1] * inv, n[2] * inv]
}

/// Blender polygon normal of a triangle (normal_tri_v3 + normalize)
pub fn poly_normal(a: V3, b: V3, c: V3) -> V3 {
    let n1 = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let n2 = [b[0] - c[0], b[1] - c[1], b[2] - c[2]];
    let n = [n1[1] * n2[2] - n1[2] * n2[1], n1[2] * n2[0] - n1[0] * n2[2], n1[0] * n2[1] - n1[1] * n2[0]];
    // normalize_v3 on the face normal divides (the matrix column normalisation multiplies by the reciprocal)
    let d = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    if d > 1.0e-35f32 {
        let l = d.sqrt();
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0; 3]
    }
}

// ---- mathutils.Vector (size 3) primitives ----

pub fn vsub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn vadd(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn vneg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
/// `float * Vector` / `Vector * float`: scalar converted to float32
pub fn vscale(a: V3, s: f64) -> V3 {
    let s = s as f32;
    [a[0] * s, a[1] * s, a[2] * s]
}
/// `Vector / float`: multiplies by the float32 reciprocal
pub fn vdiv(a: V3, d: f64) -> V3 {
    let inv = 1.0f32 / (d as f32);
    [a[0] * inv, a[1] * inv, a[2] * inv]
}
/// Vector.dot: float products accumulated in double; the double is returned as is
pub fn vdot(a: V3, b: V3) -> f64 {
    let mut d = 0.0f64;
    for i in 0..3 {
        d += (a[i] * b[i]) as f64;
    }
    d
}
/// Vector.normalized()
pub fn vnormalized(a: V3) -> V3 {
    let d = (a[0] as f64) * (a[0] as f64) + (a[1] as f64) * (a[1] as f64) + (a[2] as f64) * (a[2] as f64);
    if d > 1.0e-35 {
        let inv = 1.0f32 / (d.sqrt() as f32);
        [a[0] * inv, a[1] * inv, a[2] * inv]
    } else {
        [0.0; 3]
    }
}
