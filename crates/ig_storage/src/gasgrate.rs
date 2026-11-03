//! appliances/oven.py `gas_grates` + the hollow cut of the gas oven (OvenFactory.create_asset, use_gas = 1).
//!
//! python builds every grate piece with `bpy.ops.mesh.primitive_*_add(location, scale)`: the new object gets the location (and the rotation set
//! afterwards) while the `scale` is baked into the MESH DATA (ED_object_new_primitive_matrix), object scale stays 1. The pieces are unioned with
//! BOOLEAN (EXACT) modifiers (the operand is transformed into the target object's local space), the union of each grid is voxel-remeshed in that local
//! space (voxel_size 0.004), smoothed, and finally the grids and cylinders are unioned. The result is joined into the (identity-transform) oven body.
use crate::mesh::Mesh;

type V3 = [f64; 3];

/// object transform: world = loc + R(rot_z) * (scale * local)
#[derive(Clone, Copy)]
struct Xf {
    loc: V3,
    rot_z: f64,
    scale: V3,
}

impl Xf {
    fn new(loc: V3, rot_z: f64, scale: V3) -> Xf {
        // location / scale / rotation_euler are float32 properties
        let r = |v: V3| [v[0] as f32 as f64, v[1] as f32 as f64, v[2] as f32 as f64];
        Xf { loc: r(loc), rot_z: rot_z as f32 as f64, scale: r(scale) }
    }
    fn to_world(&self, p: V3) -> V3 {
        let (s, c) = self.rot_z.sin_cos();
        let q = [p[0] * self.scale[0], p[1] * self.scale[1], p[2] * self.scale[2]];
        [self.loc[0] + c * q[0] - s * q[1], self.loc[1] + s * q[0] + c * q[1], self.loc[2] + q[2]]
    }
    fn to_local(&self, w: V3) -> V3 {
        let (s, c) = self.rot_z.sin_cos();
        let d = [w[0] - self.loc[0], w[1] - self.loc[1], w[2] - self.loc[2]];
        let q = [c * d[0] + s * d[1], -s * d[0] + c * d[1], d[2]];
        [q[0] / self.scale[0], q[1] / self.scale[1], q[2] / self.scale[2]]
    }
}

struct Obj {
    mesh: Mesh,
    xf: Xf,
}

fn f32m(m: &mut Mesh) {
    for p in m.pos.iter_mut() {
        *p = [p[0] as f32 as f64, p[1] as f32 as f64, p[2] as f32 as f64];
    }
}

/// bpy.ops.mesh.primitive_cube_add(size=1) mesh data
fn unit_cube() -> Mesh {
    let g = crate::hwexact::bpy_cube();
    let mut m = Mesh::new();
    m.pos = g.v.iter().map(|p| [p[0] as f64 * 0.5, p[1] as f64 * 0.5, p[2] as f64 * 0.5]).collect();
    m.faces = g.p.clone();
    m
}

/// primitive_cube_add(size=1, scale): vertices 0.5 * scale (float32)
fn scaled_cube(scale: V3) -> Mesh {
    let mut m = unit_cube();
    let s = [scale[0] as f32, scale[1] as f32, scale[2] as f32];
    for p in m.pos.iter_mut() {
        *p = [(p[0] as f32 * s[0]) as f64, (p[1] as f32 * s[1]) as f64, (p[2] as f32 * s[2]) as f64];
    }
    m
}

/// bpy.ops.mesh.primitive_cylinder_add(radius, depth) mesh data (32 vertices, n-gon caps)
fn cylinder(radius: f64, depth: f64) -> Mesh {
    let g = crate::hwexact::bpy_cylinder();
    let (r, h) = (radius as f32, (depth / 2.0) as f32);
    let mut m = Mesh::new();
    m.pos = g.v.iter().map(|p| [(p[0] * r) as f64, (p[1] * r) as f64, (p[2] * h) as f64]).collect();
    m.faces = g.p.clone();
    m
}

/// run `f` with the patch-vote robustness of the exact boolean enabled
fn with_vote<T>(f: impl FnOnce() -> T) -> T {
    crate::exbool::exact_boolean::set_patch_vote(true);
    let r = f();
    crate::exbool::exact_boolean::set_patch_vote(false);
    r
}

fn bool_in(m: &Mesh) -> crate::exbool::exact_boolean::BoolInput {
    crate::exbool::exact_boolean::BoolInput { verts: m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), faces: m.faces.clone() }
}

fn bool_out(o: &crate::exbool::exact_boolean::BoolOutput) -> Mesh {
    let mut m = Mesh::new();
    m.pos = o.verts.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    m.faces = o.faces.clone();
    m
}

/// connected components (by shared vertices) of a mesh: face lists
fn components(m: &Mesh) -> Vec<Vec<usize>> {
    let n = m.pos.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(p: &mut Vec<u32>, mut x: u32) -> u32 {
        while p[x as usize] != x {
            let g = p[p[x as usize] as usize];
            p[x as usize] = g;
            x = g;
        }
        x
    }
    for f in &m.faces {
        for &v in &f[1..] {
            let (a, b) = (find(&mut parent, f[0]), find(&mut parent, v));
            if a != b {
                parent[a as usize] = b;
            }
        }
    }
    let mut idx: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut out: Vec<Vec<usize>> = vec![];
    for (fi, f) in m.faces.iter().enumerate() {
        let r = find(&mut parent, f[0]);
        let k = *idx.entry(r).or_insert_with(|| {
            out.push(vec![]);
            out.len() - 1
        });
        out[k].push(fi);
    }
    out
}

fn bbox(m: &Mesh, faces: &[usize]) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for &fi in faces {
        for &v in &m.faces[fi] {
            for k in 0..3 {
                lo[k] = lo[k].min(m.pos[v as usize][k]);
                hi[k] = hi[k].max(m.pos[v as usize][k]);
            }
        }
    }
    (lo, hi)
}

/// sub-mesh of the given faces (vertices renumbered in first-use order)
fn submesh(m: &Mesh, faces: &[usize]) -> Mesh {
    let mut map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut o = Mesh::new();
    for &fi in faces {
        let f: Vec<u32> = m.faces[fi]
            .iter()
            .map(|&v| {
                *map.entry(v).or_insert_with(|| {
                    o.pos.push(m.pos[v as usize]);
                    (o.pos.len() - 1) as u32
                })
            })
            .collect();
        o.faces.push(f);
    }
    o
}

/// Split `a` into (components that cannot touch the box of `b`, the rest). A component whose box is disjoint from the box of the other operand is
/// untouched by a union with it (only nested components inside another component of the same operand would differ, which voxel-remeshed grids
/// do not have).
fn split_far(a: &Mesh, other_lo: [f64; 3], other_hi: [f64; 3]) -> (Mesh, Mesh) {
    let pad = 1e-4;
    let (mut far, mut near) = (vec![], vec![]);
    for c in components(a) {
        let (lo, hi) = bbox(a, &c);
        let apart = (0..3).any(|k| hi[k] + pad < other_lo[k] || lo[k] - pad > other_hi[k]);
        if apart {
            far.extend(c);
        } else {
            near.extend(c);
        }
    }
    far.sort_unstable();
    near.sort_unstable();
    (submesh(a, &far), submesh(a, &near))
}

fn mesh_box(m: &Mesh) -> ([f64; 3], [f64; 3]) {
    let all: Vec<usize> = (0..m.faces.len()).collect();
    bbox(m, &all)
}

fn append(a: &mut Mesh, b: &Mesh) {
    let base = a.pos.len() as u32;
    a.pos.extend_from_slice(&b.pos);
    for f in &b.faces {
        a.faces.push(f.iter().map(|&v| v + base).collect());
    }
}

/// butil.boolean(objs): the first object absorbs the others one by one (BOOLEAN modifier UNION, EXACT); result in the first object's local space
fn union_all(objs: &[Obj]) -> Mesh {
    use crate::exbool::exact_boolean::*;
    let t0 = web_time::Instant::now();
    let mut keep = objs[0].mesh.clone();
    f32m(&mut keep);
    for o in &objs[1..] {
        let mut other = Mesh::new();
        other.faces = o.mesh.faces.clone();
        other.pos = o.mesh.pos.iter().map(|&p| objs[0].xf.to_local(o.xf.to_world(p))).collect();
        f32m(&mut other);
        // big operands: components away from the other operand's box skip the exact boolean (same surface, much less to triangulate / classify)
        let (mut keep_far, mut other_far) = (Mesh::new(), Mesh::new());
        if keep.faces.len() + other.faces.len() > 4000 {
            let (kb, ob) = (mesh_box(&keep), mesh_box(&other));
            let (kf, kn) = split_far(&keep, ob.0, ob.1);
            let (of, on) = split_far(&other, kb.0, kb.1);
            keep_far = kf;
            other_far = of;
            keep = kn;
            other = on;
        }
        let mut out = if keep.faces.is_empty() || other.faces.is_empty() {
            let mut m = keep.clone();
            append(&mut m, &other);
            m
        } else {
            let r = with_vote(|| exact_boolean(&[bool_in(&keep), bool_in(&other)], BoolOp::Union, false, false));
            bool_out(&r)
        };
        append(&mut out, &keep_far);
        append(&mut out, &other_far);
        keep = out;
        if let Ok(d) = std::env::var("GASSTEPS") {
            static M: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let k = M.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let v: Vec<Vec<f64>> = keep.pos.iter().map(|p| p.to_vec()).collect();
            let _ = std::fs::write(format!("{d}/step{k:03}.json"), serde_json::json!({"verts": v, "polys": keep.faces}).to_string());
        }
    }
    if let Ok(d) = std::env::var("GASDUMP") {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let v: Vec<Vec<f64>> = keep.pos.iter().map(|p| p.to_vec()).collect();
        let _ = std::fs::write(format!("{d}/union{k}.json"), serde_json::json!({"verts": v, "polys": keep.faces}).to_string());
    }
    if std::env::var("GASDBG").is_ok() {
        eprintln!("union_all {} objs -> {} verts {} faces in {:?}", objs.len(), keep.pos.len(), keep.faces.len(), t0.elapsed());
    }
    keep
}

/// n-ary variant: all operands in ONE exact boolean (a union is associative), saves the repeated arena construction of the chain
fn union_nary(objs: &[Obj]) -> Mesh {
    use crate::exbool::exact_boolean::*;
    let t0 = web_time::Instant::now();
    let mut inputs = vec![];
    for (i, o) in objs.iter().enumerate() {
        let mut m = Mesh::new();
        m.faces = o.mesh.faces.clone();
        m.pos = if i == 0 { o.mesh.pos.clone() } else { o.mesh.pos.iter().map(|&p| objs[0].xf.to_local(o.xf.to_world(p))).collect() };
        f32m(&mut m);
        inputs.push(bool_in(&m));
    }
    let r = exact_boolean(&inputs, BoolOp::Union, false, false);
    let m = bool_out(&r);
    if let Ok(d) = std::env::var("GASDUMP") {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let v: Vec<Vec<f64>> = m.pos.iter().map(|p| p.to_vec()).collect();
        let _ = std::fs::write(format!("{d}/nary{k}.json"), serde_json::json!({"verts": v, "polys": m.faces}).to_string());
    }
    if std::env::var("GASDBG").is_ok() {
        eprintln!("union_nary {} objs -> {} verts {} faces in {:?}", objs.len(), m.pos.len(), m.faces.len(), t0.elapsed());
    }
    m
}

fn tri_fan(faces: &[Vec<u32>]) -> Vec<[u32; 3]> {
    let mut t = vec![];
    for f in faces {
        for k in 1..f.len() - 1 {
            t.push([f[0], f[k], f[k + 1]]);
        }
    }
    t
}

/// REMESH modifier VOXEL (OpenVDB, adaptivity 0) + SMOOTH (8 iterations, factor 1)
fn remesh_smooth(m: &Mesh, voxel: f32) -> Option<Mesh> {
    let pos: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let tris = tri_fan(&m.faces);
    // OpenVDB when linked (native), else the pure-Rust surface-nets remesh (wasm, or builds without OpenVDB)
    let (v, f) = match crate::vdb::remesh_voxel(&pos, &tris, voxel) {
        Some(r) => r,
        None => {
            let n = ig_shared::voxelremesh::voxel_remesh_nets(&pos, &tris, voxel);
            (n.v, n.f)
        }
    };
    let mut r = Mesh::new();
    r.pos = v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    r.faces = f;
    let mut s = crate::coil::smooth(&r, 1.0, 8);
    f32m(&mut s);
    Some(s)
}

pub struct GrateParams {
    pub width: f64,
    pub depth: f64,
    pub grate_width: f64,
    pub grate_depth: f64,
    pub height: f64,
    pub thickness: f64,
    pub grids: Vec<usize>,
    pub branches: usize,
    pub center_ratio: f64,
    pub middle_ratio: f64,
}

/// `gas_grates(...)`: the joined grates in WORLD (= oven body) coordinates
pub fn gas_grates(p: &GrateParams) -> Option<Mesh> {
    // EXBOOL_SLOW=1: reference run with the brute-force exact boolean paths (identical output, minutes slower; checked by the test below)
    crate::exbool::exact_boolean::set_slow_paths(std::env::var_os("EXBOOL_SLOW").is_some());
    let (width, depth, gw, gd, t) = (p.width, p.depth, p.grate_width, p.grate_depth, p.thickness);
    let height = p.height;
    let high = height + t * 0.9;
    let ng = p.grids.len() as f64;
    // every grid is independent (cube unions + voxel remesh + smooth): one thread each
    let grid_fn = |i: usize, n: usize| -> Option<Vec<Obj>> {
        let mut grates: Vec<Obj> = vec![];
        let fi = i as f64;
        let cube = |loc: V3, scale: V3, rz: f64| Obj { mesh: scaled_cube(scale), xf: Xf::new(loc, rz, [1.0; 3]) };
        let mut cubes = vec![
            cube([depth / 2.0, gw / ng * fi + (width - gw) / 2.0 + t / 2.0, height], [gd + t, t, t], 0.0),
            cube([depth / 2.0, gw / ng * (fi + 1.0) + (width - gw) / 2.0 - t / 2.0, height], [gd + t, t, t], 0.0),
        ];
        let nn = n as f64;
        for j in 0..=n {
            cubes.push(cube([gd / nn * j as f64 + (depth - gd) / 2.0, gw / ng * (fi + 0.5) + (width - gw) / 2.0, high], [t, gw / ng, t], 0.0));
        }
        for j in 0..n {
            let min_dist = (gw / ng / 2.0).min(gd / nn / 2.0);
            let line_len = (gw / ng / 2.0).max(gd / nn / 2.0) - min_dist;
            let center_dist = min_dist * p.center_ratio;
            let middle_dist = min_dist * p.middle_ratio;
            let (x_center, y_center, x_middle, y_middle, x_full, y_full);
            if gw / ng / 2.0 > gd / nn / 2.0 {
                x_center = center_dist;
                y_center = line_len + center_dist;
                x_middle = middle_dist;
                y_middle = line_len + middle_dist;
                x_full = min_dist;
                y_full = line_len + min_dist;
            } else {
                x_center = center_dist + line_len;
                y_center = center_dist;
                x_middle = middle_dist + line_len;
                y_middle = middle_dist;
                x_full = min_dist + line_len;
                y_full = min_dist;
            }
            let center = [gd / nn * (j as f64 + 0.5) + (depth - gd) / 2.0, gw / ng * (fi + 0.5) + (width - gw) / 2.0];
            // (four arms, a plain cross round the burner: other counts bent into a jumble of short angled bars)
            let arms = 4;
            for k in 0..arms {
                let angle = 2.0 * std::f64::consts::PI / arms as f64 * k as f64;
                let (mut x0, mut y0) = (x_center * angle.cos(), y_center * angle.sin());
                let (mut x1, mut y1) = (x_middle * angle.cos(), y_middle * angle.sin());
                let mut seg = |x0: f64, y0: f64, x1: f64, y1: f64, cubes: &mut Vec<Obj>| {
                    let loc = [center[0] + (x0 + x1) / 2.0, center[1] + (y0 + y1) / 2.0, high];
                    let scale = [((x0 - x1).powi(2) + (y0 - y1).powi(2)).powf(0.5), t, t];
                    let ang = (y1 - y0).atan2(x1 - x0);
                    cubes.push(Obj { mesh: scaled_cube(scale), xf: Xf::new(loc, ang, [1.0; 3]) });
                };
                seg(x0, y0, x1, y1, &mut cubes);
                x0 = x1;
                y0 = y1;
                if x_full - x0.abs() < y_full - y0.abs() {
                    x1 = x_full * sign(x0);
                    y1 = y0;
                } else {
                    x1 = x0;
                    y1 = y_full * sign(y0);
                }
                seg(x0, y0, x1, y1, &mut cubes);
            }
            grates.push(Obj { mesh: cylinder(center_dist + t, t / 2.0), xf: Xf::new([center[0], center[1], height], 0.0, [1.0; 3]) });
        }
        // (crisp bars: remeshing their union at 4 mm and smoothing it melted the grate into wobbling ridges)
        grates.extend(cubes);
        Some(grates)
    };
    let mut out = Mesh::new();
    for (i, &n) in p.grids.iter().enumerate() {
        for o in grid_fn(i, n)? {
            let base = out.pos.len() as u32;
            out.pos.extend(o.mesh.pos.iter().map(|&q| o.xf.to_world(q)));
            out.faces.extend(o.mesh.faces.iter().map(|f| f.iter().map(|v| v + base).collect::<Vec<u32>>()));
        }
    }
    f32m(&mut out);
    Some(out)
}

fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// the hollow cutter: unit cube with BEVEL (segments 8, width = grate thickness, local units) placed at the grate level; world coordinates
pub fn hollow(p: &GrateParams, z: f64) -> Option<Mesh> {
    let t = p.thickness;
    let xf = Xf::new([p.depth / 2.0, p.width / 2.0, z], 0.0, [1.0; 3]);
    let cube = scaled_cube([p.grate_depth + t, p.grate_width + t, t * 2.0]);
    let b = crate::bevmod::bevel_modifier_ex(&cube, t, 8, Some(30f32.to_radians()), true, 0.5, None).ok()?;
    let mut m = Mesh::new();
    m.faces = b.faces;
    m.pos = b.pos.iter().map(|&q| xf.to_world(q)).collect();
    f32m(&mut m);
    Some(m)
}
