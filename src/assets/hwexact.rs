//! bpy-primitive replication (cylinder 32 / cube / torus) and `transform_apply` in float32, used by python factories that build
//! their meshes with `bpy.ops.mesh.primitive_*_add` + `butil.apply_transform` + `join_objects`.
use super::cabnodes::{sin_cos_from_fraction, GMesh};
use crate::bl32;

pub fn bpy_cylinder() -> GMesh {
    let n = 32usize;
    let mut g = GMesh::new();
    for i in 0..n {
        let (s, c) = sin_cos_from_fraction(i, n);
        g.v.push([s, c, -1.0]);
        g.v.push([s, c, 1.0]);
    }
    let q = |i: usize| -> Vec<u32> {
        let a = (2 * i) as u32;
        vec![a, a + 1, (a + 3) % 64, (a + 2) % 64]
    };
    for i in 0..30 {
        g.p.push(q(i));
    }
    let mut top = vec![3u32, 1];
    let mut k = 63i32;
    while k >= 5 {
        top.push(k as u32);
        k -= 2;
    }
    g.p.push(top);
    g.p.push(q(30));
    g.p.push(q(31));
    g.p.push((0..32).map(|i| 2 * i as u32).collect());
    g.tag = vec![0; g.p.len()];
    g
}

pub fn bpy_cube() -> GMesh {
    let mut g = GMesh::new();
    for x in [-1.0f32, 1.0] {
        for y in [-1.0f32, 1.0] {
            for z in [-1.0f32, 1.0] {
                g.v.push([x, y, z]);
            }
        }
    }
    g.p = vec![vec![0, 1, 3, 2], vec![2, 3, 7, 6], vec![6, 7, 5, 4], vec![4, 5, 1, 0], vec![2, 6, 4, 0], vec![7, 3, 1, 5]];
    g.tag = vec![0; 6];
    g
}

/// bpy.ops.object.transform_apply(location=apply_loc, rotation=True, scale=True) of an object whose scale/rotation_euler/location
/// were set from python doubles.
pub fn apply_obj(g: &GMesh, scale: [f64; 3], rot: [f64; 3], loc: [f64; 3], apply_loc: bool) -> GMesh {
    let s = [scale[0] as f32, scale[1] as f32, scale[2] as f32];
    let r = [rot[0] as f32, rot[1] as f32, rot[2] as f32];
    let rm = bl32::eul_to_mat3(r);
    let sm = [[s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]]];
    let m = bl32::mul_m3_m3m3(&rm, &sm);
    let t = if apply_loc { [loc[0] as f32, loc[1] as f32, loc[2] as f32] } else { [0.0; 3] };
    let mut o = g.clone();
    for p in o.v.iter_mut() {
        let (x, y, z) = (p[0], p[1], p[2]);
        let row = |k: usize| -> f32 { x * m[0][k] + y * m[1][k] + z * m[2][k] + t[k] };
        *p = [row(0), row(1), row(2)];
    }
    o
}

/// bpy.ops.object.join: active (first) first, then the others in order
pub fn join_fwd(list: &[GMesh]) -> GMesh {
    let mut o = GMesh::new();
    for g in list {
        let base = o.v.len() as u32;
        o.v.extend_from_slice(&g.v);
        for (i, f) in g.p.iter().enumerate() {
            o.p.push(f.iter().map(|x| x + base).collect());
            o.tag.push(g.tag.get(i).copied().unwrap_or(0));
        }
    }
    o
}

/// bpy.ops.mesh.primitive_torus_add(major_segments, minor_segments=12, major_radius, minor_radius) (add_mesh_torus.py)
pub fn bpy_torus(major_seg: usize, minor_seg: usize, major_rad: f64, minor_rad: f64) -> GMesh {
    let pi2 = std::f64::consts::PI * 2.0;
    let (major_rad, minor_rad) = (major_rad as f32 as f64, minor_rad as f32 as f64);
    let mut g = GMesh::new();
    let tot = (major_seg * minor_seg) as u32;
    let mut i1 = 0u32;
    for mi in 0..major_seg {
        let a = (mi as f64 / major_seg as f64) * pi2;
        let (c, s) = if major_seg == 128 { (f32::from_bits(ROT128[mi].0), f32::from_bits(ROT128[mi].1)) } else { (a.cos() as f32, a.sin() as f32) };
        // Matrix.Rotation(angle,3,'Z') rows: [c -s 0; s c 0; 0 0 1]; matrix @ vec accumulates in double
        for ni in 0..minor_seg {
            let angle = pi2 * ni as f64 / minor_seg as f64;
            let vx = (major_rad + angle.cos() * minor_rad) as f32;
            let vz = (angle.sin() * minor_rad) as f32;
            let x = (c as f64 * vx as f64) as f32;
            let y = (s as f64 * vx as f64) as f32;
            g.v.push([x, y, vz]);
            let (i2, i3, i4);
            if ni + 1 == minor_seg {
                i2 = (mi * minor_seg) as u32;
                i3 = i1 + minor_seg as u32;
                i4 = i2 + minor_seg as u32;
            } else {
                i2 = i1 + 1;
                i3 = i1 + minor_seg as u32;
                i4 = i3 + 1;
            }
            let w = |i: u32| if i >= tot { i - tot } else { i };
            g.p.push(vec![i1, w(i3), w(i4), w(i2)]);
            i1 += 1;
        }
    }
    g.tag = vec![0; g.p.len()];
    g
}
pub const ROT128: [(u32, u32); 128] = [
    (0x3f800000, 0x00000000),
    (0x3f7fb10f, 0x3d48fb15),
    (0x3f7ec46d, 0x3dc8bd3b),
    (0x3f7d3aac, 0x3e164080),
    (0x3f7b14be, 0x3e47c5c7),
    (0x3f7853f8, 0x3e78cfcb),
    (0x3f74fa0a, 0x3e94a035),
    (0x3f710908, 0x3eac7cd4),
    (0x3f6c835f, 0x3ec3ef13),
    (0x3f676bd7, 0x3edae882),
    (0x3f61c598, 0x3ef15ae9),
    (0x3f5b941b, 0x3f039c3b),
    (0x3f54db31, 0x3f0e39da),
    (0x3f4d9f01, 0x3f187fc2),
    (0x3f45e403, 0x3f22679a),
    (0x3f3daefa, 0x3f2beb49),
    (0x3f3504f2, 0x3f3504f4),
    (0x3f2beb4a, 0x3f3daef9),
    (0x3f22679b, 0x3f45e402),
    (0x3f187fc0, 0x3f4d9f02),
    (0x3f0e39d8, 0x3f54db33),
    (0x3f039c3f, 0x3f5b9419),
    (0x3ef15aeb, 0x3f61c597),
    (0x3edae87d, 0x3f676bd8),
    (0x3ec3ef1d, 0x3f6c835d),
    (0x3eac7cd7, 0x3f710908),
    (0x3e94a030, 0x3f74fa0b),
    (0x3e78cfc0, 0x3f7853f9),
    (0x3e47c5cc, 0x3f7b14be),
    (0x3e164085, 0x3f7d3aac),
    (0x3dc8bd25, 0x3f7ec46d),
    (0x3d48fb69, 0x3f7fb10f),
    (0x33a22169, 0x3f800000),
    (0xbd48fb41, 0x3f7fb10f),
    (0xbdc8bd51, 0x3f7ec46d),
    (0xbe16407b, 0x3f7d3aac),
    (0xbe47c5c2, 0x3f7b14be),
    (0xbe78cfd5, 0x3f7853f7),
    (0xbe94a02b, 0x3f74fa0c),
    (0xbeac7cd2, 0x3f710908),
    (0xbec3ef18, 0x3f6c835e),
    (0xbedae887, 0x3f676bd6),
    (0xbef15ae6, 0x3f61c598),
    (0xbf039c3d, 0x3f5b941a),
    (0xbf0e39d5, 0x3f54db34),
    (0xbf187fbe, 0x3f4d9f04),
    (0xbf226799, 0x3f45e404),
    (0xbf2beb4b, 0x3f3daef8),
    (0xbf3504f6, 0x3f3504f0),
    (0xbf3daef8, 0x3f2beb4b),
    (0xbf45e404, 0x3f226799),
    (0xbf4d9eff, 0x3f187fc4),
    (0xbf54db30, 0x3f0e39dc),
    (0xbf5b941a, 0x3f039c3d),
    (0xbf61c599, 0x3ef15ae6),
    (0xbf676bda, 0x3edae878),
    (0xbf6c835e, 0x3ec3ef17),
    (0xbf710909, 0x3eac7cd2),
    (0xbf74fa09, 0x3e94a03a),
    (0xbf7853f7, 0x3e78cfd4),
    (0xbf7b14bf, 0x3e47c5c1),
    (0xbf7d3aac, 0x3e16407a),
    (0xbf7ec46e, 0x3dc8bd10),
    (0xbf7fb10f, 0x3d48fb3d),
    (0xbf800000, 0x33bbbd2e),
    (0xbf7fb10f, 0xbd48fabe),
    (0xbf7ec46e, 0xbdc8bd10),
    (0xbf7d3aac, 0xbe16407a),
    (0xbf7b14bf, 0xbe47c5c1),
    (0xbf7853f7, 0xbe78cfd4),
    (0xbf74fa0c, 0xbe94a02b),
    (0xbf710909, 0xbeac7cd2),
    (0xbf6c8361, 0xbec3ef09),
    (0xbf676bda, 0xbedae878),
    (0xbf61c599, 0xbef15ae6),
    (0xbf5b941a, 0xbf039c3d),
    (0xbf54db30, 0xbf0e39dc),
    (0xbf4d9f04, 0xbf187fbd),
    (0xbf45e409, 0xbf226792),
    (0xbf3daefd, 0xbf2beb45),
    (0xbf3504f6, 0xbf3504f0),
    (0xbf2beb4b, 0xbf3daef8),
    (0xbf226799, 0xbf45e404),
    (0xbf187fbe, 0xbf4d9f04),
    (0xbf0e39e3, 0xbf54db2b),
    (0xbf039c44, 0xbf5b9416),
    (0xbef15af4, 0xbf61c595),
    (0xbedae887, 0xbf676bd6),
    (0xbec3ef18, 0xbf6c835e),
    (0xbeac7cd2, 0xbf710908),
    (0xbe94a02b, 0xbf74fa0c),
    (0xbe78cff4, 0xbf7853f5),
    (0xbe47c5e2, 0xbf7b14bd),
    (0xbe16409a, 0xbf7d3aab),
    (0xbdc8bd51, 0xbf7ec46d),
    (0xbd48fb41, 0xbf7fb10f),
    (0x33a22169, 0xbf800000),
    (0x3d48fb69, 0xbf7fb10f),
    (0x3dc8bce6, 0xbf7ec46e),
    (0x3e164065, 0xbf7d3aad),
    (0x3e47c5ad, 0xbf7b14c0),
    (0x3e78cfc0, 0xbf7853f9),
    (0x3e94a030, 0xbf74fa0b),
    (0x3eac7cd7, 0xbf710908),
    (0x3ec3ef1d, 0xbf6c835d),
    (0x3edae86f, 0xbf676bdc),
    (0x3ef15add, 0xbf61c59b),
    (0x3f039c38, 0xbf5b941d),
    (0x3f0e39d8, 0xbf54db33),
    (0x3f187fc0, 0xbf4d9f02),
    (0x3f22679b, 0xbf45e402),
    (0x3f2beb4d, 0xbf3daef6),
    (0x3f3504ed, 0xbf3504fa),
    (0x3f3daef4, 0xbf2beb4f),
    (0x3f45e400, 0xbf22679d),
    (0x3f4d9f01, 0xbf187fc2),
    (0x3f54db31, 0xbf0e39da),
    (0x3f5b941b, 0xbf039c3b),
    (0x3f61c59a, 0xbef15ae2),
    (0x3f676bd4, 0xbedae891),
    (0x3f6c835c, 0xbec3ef22),
    (0x3f710907, 0xbeac7cdc),
    (0x3f74fa0a, 0xbe94a035),
    (0x3f7853f8, 0xbe78cfcb),
    (0x3f7b14bf, 0xbe47c5b7),
    (0x3f7d3aad, 0xbe164070),
    (0x3f7ec46c, 0xbdc8bd7b),
    (0x3f7fb10f, 0xbd48fb95),
];

/// BKE_mesh_calc_edges (4.2): edges grouped by `v_low & (maps-1)` hash map, first-appearance order inside each map.
/// `maps` = power_of_2_min(min(8, system threads)) when the mesh has >= 1000 faces, else 1.
pub fn calc_edges_bl(faces: &[Vec<u32>], maps: usize) -> Vec<[u32; 2]> {
    let maps = if faces.len() < 1000 { 1 } else { maps };
    let mask = (maps - 1) as u32;
    let mut out = vec![];
    for t in 0..maps as u32 {
        let mut seen = crate::dhash::HashSet::default();
        for f in faces {
            for c in 0..f.len() {
                let v = f[c];
                let vp = f[(c + f.len() - 1) % f.len()];
                if v == vp {
                    continue;
                }
                let e = [v.min(vp), v.max(vp)];
                if (e[0] & mask) == t && seen.insert(e) {
                    out.push(e);
                }
            }
        }
    }
    out
}

/// Catmull-Clark limit evaluation for meshes whose faces are all quads around valence-4 vertices (regular bicubic B-spline patches).
pub fn regular_bspline_eval(m: &GMesh) -> impl Fn(usize, usize, f32, f32) -> [f32; 3] + '_ {
    use crate::dhash::HashMap;
    let mut dir: HashMap<(u32, u32), (usize, usize)> = HashMap::default();
    for (fi, f) in m.p.iter().enumerate() {
        for k in 0..f.len() {
            dir.insert((f[k], f[(k + 1) % f.len()]), (fi, k));
        }
    }
    move |fi: usize, _ptex: usize, u: f32, v: f32| -> [f32; 3] {
        let f = &m.p[fi];
        let (p0, p1, p2, p3) = (f[0], f[1], f[2], f[3]);
        let pos = |i: u32| m.v[i as usize];
        // face containing directed edge a->b: returns its vertices rotated so that [0]=a,[1]=b
        let rot = |a: u32, b: u32| -> [u32; 4] {
            let (gi, k) = dir[&(a, b)];
            let g = &m.p[gi];
            [g[k], g[(k + 1) % 4], g[(k + 2) % 4], g[(k + 3) % 4]]
        };
        let mut p = [[0u32; 4]; 4];
        p[1][1] = p0;
        p[2][1] = p1;
        p[2][2] = p2;
        p[1][2] = p3;
        // below (v-): edge p1->p0 in face B = [p1,p0,y0,y1]
        let b = rot(p1, p0);
        p[1][0] = b[2];
        p[2][0] = b[3];
        // right (u+): edge p2->p1 in R = [p2,p1,x0,x1] ; x0 neighbour of p1, x1 neighbour of p2
        let r = rot(p2, p1);
        p[3][1] = r[2];
        p[3][2] = r[3];
        // above (v+): edge p3->p2 in A = [p3,p2,z0,z1]
        let a = rot(p3, p2);
        p[2][3] = a[2];
        p[1][3] = a[3];
        // left (u-): edge p0->p3 in L = [p0,p3,w0,w1]
        let l = rot(p0, p3);
        p[0][2] = l[2];
        p[0][1] = l[3];
        // diagonal corners: face across the edge (p0, p[1][0]) from B: directed edge p[1][0]->p0 ; = [y0,p0,z,w]
        let c00 = rot(p[1][0], p0);
        p[0][0] = c00[3];
        let d = rot(p1, p[2][0]);
        p[3][0] = d[2];
        let e = rot(p2, p[3][2]);
        p[3][3] = e[2];
        let g = rot(p[0][2], p3);
        p[0][3] = g[3];
        let bs = |t: f32| -> [f32; 4] {
            let t2 = t * t;
            let t3 = t2 * t;
            [(1.0 - 3.0 * t + 3.0 * t2 - t3) / 6.0, (4.0 - 6.0 * t2 + 3.0 * t3) / 6.0, (1.0 + 3.0 * t + 3.0 * t2 - 3.0 * t3) / 6.0, t3 / 6.0]
        };
        let (wu, wv) = (bs(u), bs(v));
        let mut r = [0f32; 3];
        for i in 0..4 {
            for j in 0..4 {
                let w = wu[i] * wv[j];
                let q = pos(p[i][j]);
                for k in 0..3 {
                    r[k] += q[k] * w;
                }
            }
        }
        r
    }
}
