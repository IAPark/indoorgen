//! Shared node-graph ports for shelves/cabinets: large_shelf.py frame (LargeShelfBaseFactory.create_asset), used by
//! LargeShelf / SingleCabinet / KitchenCabinet. Node space: x = width, y = depth (front at +y), z up; the node graph ends with
//! a Transform of Rotation (0, 0, -1.5708) (applied by `shelf_frame`).
use super::*;

pub const R90: f64 = 1.5708;

/// Blender mesh cube node (centred, `size`) translated by `t`.
pub fn cube_at(size: V3, t: V3) -> Mesh {
    Mesh::cuboid([t[0] - size[0] / 2.0, t[1] - size[1] / 2.0, t[2] - size[2] / 2.0], [t[0] + size[0] / 2.0, t[1] + size[1] / 2.0, t[2] + size[2] / 2.0])
}

/// GeometryNodeMeshCylinder (axis z, `n` vertices, start angle 0, caps).
pub fn cylinder_node(n: usize, radius: f64, depth: f64) -> Mesh {
    Mesh::cylinder(0.0, 0.0, -depth / 2.0, depth / 2.0, radius, n)
}

#[derive(Clone, Debug, Default)]
pub struct ShelfParams {
    pub shelf_depth: f64,
    pub side_t: f64,
    pub back_t: f64,
    pub y_gap: f64,
    pub bottom_h: f64,
    pub div_t: f64,
    pub screw_depth: f64,
    pub screw_r: f64,
    pub screw_wgap: f64,
    pub cell_w: Vec<f64>,
    pub cell_h: Vec<f64>,
    // derived (update_translation_params)
    pub shelf_width: f64,
    pub shelf_height: f64,
    pub side_x: Vec<f64>,
    pub div_x: Vec<f64>,
    pub div_z: Vec<f64>,
}

impl ShelfParams {
    /// LargeShelfBaseFactory.update_translation_params
    pub fn derive(&mut self) {
        let (st, dt) = (self.side_t, self.div_t);
        let mut width = (self.cell_w.len() as f64 - 1.0) * st * 2.0 + (self.cell_w.len() as f64 - 1.0) * 0.001;
        let mut height = (self.cell_h.len() as f64 + 1.0) * dt + self.bottom_h;
        for w in &self.cell_w {
            width += w;
        }
        for h in &self.cell_h {
            height += h;
        }
        self.shelf_width = width;
        self.shelf_height = height;
        let mut dist = -(width + st) / 2.0;
        let mut sx = vec![dist];
        for w in &self.cell_w {
            dist += st + w;
            sx.push(dist);
            dist += st + 0.001;
            sx.push(dist);
        }
        sx.pop();
        let mut dist = self.bottom_h + dt / 2.0;
        let mut dz = vec![dist];
        for h in &self.cell_h {
            dist += h + dt;
            dz.push(dist);
        }
        self.div_x = (0..self.cell_w.len()).map(|i| (sx[2 * i] + sx[2 * i + 1]) / 2.0).collect();
        self.side_x = sx;
        self.div_z = dz;
    }
}

/// geometry_nodes() of large_shelf.py with the final rotation. Returns an Asset (frame parts + division boards with support tags).
pub fn shelf_frame(p: &ShelfParams, frame: &Material, board: &Material) -> Asset {
    let mut a = Asset::default();
    let d = p.shelf_depth;
    let h = p.shelf_height;
    // side boards
    for &x in &p.side_x {
        let m = cube_at([p.side_t, d + 0.004, h + 0.002], [x, 0.0, (h + 0.002) / 2.0]);
        let b = m.bounds();
        a.mesh(m, frame, Some(b));
    }
    // back board
    {
        let (w, t, hh) = (p.shelf_width + p.side_t * 2.0, p.back_t, h - 0.001);
        let m = cube_at([w, t, hh], [0.0, d * -0.5 + t * -0.5, hh * 0.5]);
        let b = m.bounds();
        a.mesh(m, frame, Some(b));
    }
    // bottom boards
    for i in 0..p.cell_w.len() {
        let m = cube_at([p.cell_w[i], p.side_t, p.bottom_h], [p.div_x[i], d * 0.5 - p.y_gap, p.bottom_h * 0.5]);
        let b = m.bounds();
        a.mesh(m, frame, Some(b));
    }
    // division boards (+ screw heads)
    for i in 0..p.cell_w.len() {
        for &z in &p.div_z {
            let w = p.cell_w[i];
            let mut m = cube_at([w, d, p.div_t], [0.0, 0.0, 0.0]);
            let board_box = m.bounds();
            let sx = w * 0.5 - p.screw_wgap;
            let sy = d * 0.5 - p.screw_wgap;
            let sz = p.div_t * -0.5;
            let mut sc = Mesh::new();
            for (x, y) in [(sx, -sy), (sx, sy)] {
                let mut c = cylinder_node(32, p.screw_r, p.screw_depth);
                c.transform(&Mat4::translation([x, y, sz]));
                sc.append(&c);
            }
            let mut sc2 = sc.clone();
            sc2.transform(&Mat4::scale([-1.0, 1.0, 1.0]));
            m.append(&sc2);
            m.append(&sc);
            m.transform(&Mat4::translation([p.div_x[i], 0.0, z]));
            let col = ([board_box.0[0] + p.div_x[i], board_box.0[1], board_box.0[2] + z], [board_box.1[0] + p.div_x[i], board_box.1[1], board_box.1[2] + z]);
            a.mesh(m, board, Some(col));
            a.supports.push(col);
        }
    }
    a.transform(&Mat4::rot_z(-R90));
    a
}

// ---------------------------------------------------------------------------------------------------------------
// Blender-faithful (float32, vertex-order exact) geometry-node primitives used by the exact-topology ports.
// ---------------------------------------------------------------------------------------------------------------

/// Exact mesh as python's `obj.data` would be: vertex array (float32 values), polygon vertex lists, per-face tag names.
#[derive(Clone, Default, Debug)]
pub struct ExactMesh {
    pub verts: Vec<[f64; 3]>,
    pub polys: Vec<Vec<u32>>,
    pub names: Vec<String>,
}

/// float32 polygon mesh with per-face tag id (0 none, 1 support).
#[derive(Clone, Default, Debug)]
pub struct GMesh {
    pub v: Vec<[f32; 3]>,
    pub p: Vec<Vec<u32>>,
    pub tag: Vec<u8>,
    /// explicit edges (only tracked by primitives used before Subdivide Mesh; empty otherwise)
    pub e: Vec<[u32; 2]>,
}

pub const TAG_SUPPORT: u8 = 1;

impl GMesh {
    pub fn new() -> GMesh {
        GMesh::default()
    }
    /// JoinGeometry node: inputs are joined in REVERSE order of the socket links (observed in Blender 4.2).
    pub fn join(list: &[GMesh]) -> GMesh {
        let mut o = GMesh::new();
        for g in list.iter().rev() {
            let base = o.v.len() as u32;
            o.v.extend_from_slice(&g.v);
            for (i, f) in g.p.iter().enumerate() {
                o.p.push(f.iter().map(|x| x + base).collect());
                o.tag.push(g.tag[i]);
            }
            o.e.extend(g.e.iter().map(|e| [e[0] + base, e[1] + base]));
        }
        o
    }
    /// Transform node (loc, euler XYZ, scale), float32 like Blender (mul_m4_v3).
    pub fn transformed(&self, loc: [f32; 3], rot: [f32; 3], scale: [f32; 3]) -> GMesh {
        // Transform node: euler -> quaternion (double math, float result) -> quat_to_mat3
        let (ti, tj, th) = (rot[0] as f64 * 0.5, rot[1] as f64 * 0.5, rot[2] as f64 * 0.5);
        let (ci, cj, ch) = (ti.cos(), tj.cos(), th.cos());
        let (si, sj, sh) = (ti.sin(), tj.sin(), th.sin());
        let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
        let q = [(cj * cc + sj * ss) as f32, (cj * sc - sj * cs) as f32, (cj * ss + sj * cc) as f32, (cj * cs - sj * sc) as f32];
        let m = crate::bl32::quat_to_mat3(q);
        let mut m = m;
        for c in 0..3 {
            for r in 0..3 {
                m[c][r] *= scale[c];
            }
        }
        let identity = rot == [0.0; 3] && scale == [1.0; 3];
        let mut o = self.clone();
        static FMAV: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let var = FMAV.get_or_init(|| std::env::var("FMAV").unwrap_or_default());
        for p in o.v.iter_mut() {
            if identity {
                *p = [p[0] + loc[0], p[1] + loc[1], p[2] + loc[2]];
            } else {
                let (x, y, z) = (p[0], p[1], p[2]);
                let row = |k: usize| -> f32 {
                    match var.as_str() {
                        "a" => m[2][k].mul_add(z, y.mul_add(m[1][k], x * m[0][k])) + loc[k],
                        "b" => m[2][k].mul_add(z, y.mul_add(m[1][k], x * m[0][k])) + loc[k],
                        "c" => (x * m[0][k] + y * m[1][k]) + m[2][k].mul_add(z, loc[k]),
                        "d" => y.mul_add(m[1][k], x.mul_add(m[0][k], 0.0)) + m[2][k] * z + loc[k],
                        "e" => m[2][k].mul_add(z, m[1][k].mul_add(y, m[0][k].mul_add(x, loc[k]))),
                        _ => x * m[0][k] + y * m[1][k] + m[2][k] * z + loc[k],
                    }
                };
                *p = [row(0), row(1), row(2)];
            }
        }
        o
    }
    pub fn translated(&self, t: [f32; 3]) -> GMesh {
        self.transformed(t, [0.0; 3], [1.0; 3])
    }
    /// GeometryNodeTriangulate (quad: shortest diagonal; ngon: handled only for triangles/quads here)
    pub fn triangulated(&self) -> GMesh {
        self.triangulated_m(false)
    }
    /// `beauty` selects MOD_TRIANGULATE_QUAD_BEAUTY (edit-mode quads_convert_to_tris) instead of the node's shortest diagonal
    pub fn triangulated_m(&self, beauty: bool) -> GMesh {
        let mut o = GMesh { v: self.v.clone(), p: vec![], tag: vec![], e: vec![] };
        let mut extra: Vec<(Vec<u32>, u8)> = vec![];
        for (fi, f) in self.p.iter().enumerate() {
            let t = self.tag[fi];
            let t_ = t;
            if f.len() == 3 {
                o.p.push(f.clone());
                o.tag.push(t);
            } else if f.len() == 4 {
                let (a, b, c, d) = (f[0], f[1], f[2], f[3]);
                let ls = |i: u32, j: u32| {
                    let (p, q) = (self.v[i as usize], self.v[j as usize]);
                    let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
                    dx * dx + dy * dy + dz * dz
                };
                let d1 = ls(a, c);
                let d2 = ls(b, d);
                let split_ac = if beauty { super::polyfill::quad_split_24_beauty([self.v[a as usize], self.v[b as usize], self.v[c as usize], self.v[d as usize]]) } else { d1 < d2 };
                if fi == 228 && { static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *T.get_or_init(|| std::env::var("TRIDBG").is_ok()) } { eprintln!("quad {:?} d1 {:e} d2 {:e} verts {:?}", f, d1, d2, f.iter().map(|&i| self.v[i as usize]).collect::<Vec<_>>()); }
                if split_ac {
                    // diagonal a-c
                    o.p.push(vec![a, c, d]);
                    o.tag.push(t);
                    extra.push((vec![a, b, c], t));
                } else {
                    // diagonal b-d
                    o.p.push(vec![b, d, a]);
                    o.tag.push(t);
                    extra.push((vec![b, c, d], t));
                }
            } else {
                let co: Vec<[f32; 3]> = f.iter().map(|&i| self.v[i as usize]).collect();
                let tris = super::polyfill::triangulate_ngon(&co);
                let last = tris.len() - 1;
                for (k, t) in tris.iter().enumerate() {
                    let face = vec![f[t[0] as usize], f[t[1] as usize], f[t[2] as usize]];
                    if k == last {
                        o.p.push(face);
                        o.tag.push(t_);
                    } else {
                        extra.push((face, t_));
                    }
                }
            }
        }
        for (p, t) in extra {
            o.p.push(p);
            o.tag.push(t);
        }
        o
    }
    pub fn to_exact(&self) -> ExactMesh {
        ExactMesh {
            verts: self.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect(),
            polys: self.p.clone(),
            names: self.tag.iter().map(|&t| if t == TAG_SUPPORT { "support".to_string() } else { String::new() }).collect(),
        }
    }
    pub fn to_mesh(&self) -> Mesh {
        let mut m = Mesh::new();
        for f in &self.p {
            let pts: Vec<V3> = f.iter().map(|&i| [self.v[i as usize][0] as f64, self.v[i as usize][1] as f64, self.v[i as usize][2] as f64]).collect();
            m.add_face(&pts);
        }
        m
    }
}

/// GeometryNodeMeshCube (centred). `n` = vertices per axis.
pub fn gcube(size: [f32; 3], n: [usize; 3]) -> GMesh {
    let mut g = GMesh::new();
    let (nx, ny, nz) = (n[0], n[1], n[2]);
    let pos = |i: usize, k: usize| -> f32 {
        if n[k] == 2 {
            if i == 0 { -size[k] * 0.5 } else { size[k] * 0.5 }
        } else {
            -size[k] * 0.5 + (size[k] / (n[k] as f32 - 1.0)) * i as f32
        }
    };
    // vertex index lookup
    let ring = nx * ny - (nx - 2) * (ny - 2);
    let vidx = |i: usize, j: usize, k: usize| -> u32 {
        let layer = |kk: usize| -> usize { if kk == 0 { 0 } else if kk == nz - 1 { nx * ny + (nz - 2) * ring } else { nx * ny + (kk - 1) * ring } };
        let base = layer(k);
        if k == 0 || k == nz - 1 {
            (base + j * nx + i) as u32
        } else {
            // boundary-only ring, y-major then x
            let mut idx = 0;
            for jj in 0..ny {
                for ii in 0..nx {
                    if jj == 0 || jj == ny - 1 || ii == 0 || ii == nx - 1 {
                        if jj == j && ii == i {
                            return (base + idx) as u32;
                        }
                        idx += 1;
                    }
                }
            }
            unreachable!()
        }
    };
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                if k == 0 || k == nz - 1 || j == 0 || j == ny - 1 || i == 0 || i == nx - 1 {
                    g.v.push([pos(i, 0), pos(j, 1), pos(k, 2)]);
                }
            }
        }
    }
    let quad = |g: &mut GMesh, a: u32, b: u32, c: u32, d: u32| {
        g.p.push(vec![a, b, c, d]);
        g.tag.push(0);
    };
    // bottom (-z)
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            quad(&mut g, vidx(i, j, 0), vidx(i, j + 1, 0), vidx(i + 1, j + 1, 0), vidx(i + 1, j, 0));
        }
    }
    // front (-y)
    for k in 0..nz - 1 {
        for i in 0..nx - 1 {
            quad(&mut g, vidx(i, 0, k), vidx(i + 1, 0, k), vidx(i + 1, 0, k + 1), vidx(i, 0, k + 1));
        }
    }
    // top (+z)
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            quad(&mut g, vidx(i, j, nz - 1), vidx(i + 1, j, nz - 1), vidx(i + 1, j + 1, nz - 1), vidx(i, j + 1, nz - 1));
        }
    }
    // back (+y)
    for k in 0..nz - 1 {
        for i in 0..nx - 1 {
            quad(&mut g, vidx(i, ny - 1, k), vidx(i, ny - 1, k + 1), vidx(i + 1, ny - 1, k + 1), vidx(i + 1, ny - 1, k));
        }
    }
    // left (-x)
    for k in 0..nz - 1 {
        for j in 0..ny - 1 {
            quad(&mut g, vidx(0, j, k), vidx(0, j, k + 1), vidx(0, j + 1, k + 1), vidx(0, j + 1, k));
        }
    }
    // right (+x)
    for k in 0..nz - 1 {
        for j in 0..ny - 1 {
            quad(&mut g, vidx(nx - 1, j, k), vidx(nx - 1, j + 1, k), vidx(nx - 1, j + 1, k + 1), vidx(nx - 1, j, k + 1));
        }
    }
    g.e = calc_edges(&g.p);
    g
}

/// nodegroup_tagged_cube: cube 2x2x2 with face index 2 (top, +z) tagged `support`.
pub fn gcube_tagged(size: [f32; 3]) -> GMesh {
    let mut g = gcube(size, [2, 2, 2]);
    g.tag[2] = TAG_SUPPORT;
    g
}

/// GeometryNodeMeshCylinder; fill 0 = none, 1 = ngon, 2 = triangle fan.
pub fn gcylinder(n: usize, radius: f32, depth: f32, fill: u8) -> GMesh {
    let mut g = GMesh::new();
    let ad = (2.0f64 * (std::f64::consts::PI / n as f64)) as f32;
    let top = depth * 0.5;
    // circle points: angle accumulates in float (mesh_primitive_cylinder_cone)
    let mut circ: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut ang = 0.0f32;
    for _ in 0..n {
        circ.push([ang.cos(), ang.sin()]);
        ang += ad;
    }
    let ring = |z: f32, g: &mut GMesh| {
        for c in &circ {
            g.v.push([c[0] * radius, c[1] * radius, z]);
        }
    };
    let fan = fill == 2;
    if fan {
        g.v.push([0.0, 0.0, top]);
    }
    ring(top, &mut g);
    ring(-top, &mut g);
    if fan {
        g.v.push([0.0, 0.0, -top]);
    }
    let t0 = if fan { 1u32 } else { 0 };
    let b0 = t0 + n as u32;
    let mut push = |g: &mut GMesh, f: Vec<u32>| {
        g.p.push(f);
        g.tag.push(0);
    };
    let nn = n as u32;
    if fan {
        for i in 0..nn {
            push(&mut g, vec![t0 + i, t0 + (i + 1) % nn, 0]);
        }
    } else if fill == 1 {
        push(&mut g, (0..nn).collect());
    }
    for i in 0..nn {
        let j = (i + 1) % nn;
        push(&mut g, vec![t0 + i, b0 + i, b0 + j, t0 + j]);
    }
    if fan {
        let c = b0 + nn;
        for i in 0..nn {
            push(&mut g, vec![b0 + i, c, b0 + (i + 1) % nn]);
        }
    } else if fill == 1 {
        push(&mut g, (0..nn).rev().map(|i| b0 + i).collect());
    }
    g
}

/// large_shelf.py geometry_nodes as a faithful float32 mesh (before the final rotation when `rotate` is false).
pub fn shelf_gmesh(p: &ShelfParams, tag_support: bool) -> GMesh {
    let f = |x: f64| x as f32;
    let (st, d, h) = (f(p.side_t), f(p.shelf_depth), f(p.shelf_height));
    let sw = f(p.shelf_width);
    let bt = f(p.back_t);
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    // side boards
    let hh = h + 0.002f32;
    let dd = d + 0.004f32;
    let mut sides = vec![];
    for &x in &p.side_x {
        sides.push(gcube([st, dd, hh], [2, 2, 2]).translated([f(x), 0.0, hh * 0.5]));
    }
    // back board
    let width = sw + f(p.side_t * 2.0);
    let bh = h + -0.001f32;
    let back = {
        let ma = d.mul_add(-0.5, bt * -0.5);
        gcube([width, bt, bh], [2, 2, 2]).translated([0.0, ma, bh * 0.5])
    };
    let mut bottoms = vec![];
    let bhgt = f(p.bottom_h);
    for i in 0..p.cell_w.len() {
        let sub = d * 0.5 - f(p.y_gap);
        bottoms.push(gcube([f(p.cell_w[i]), st, bhgt], [2, 2, 2]).translated([f(p.div_x[i]), sub, bhgt * 0.5]));
    }
    let mut frame_in = vec![back];
    frame_in.extend(sides);
    frame_in.extend(bottoms);
    let frame = GMesh::join(&frame_in);
    // division boards
    let dt = f(p.div_t);
    let mut boards = vec![];
    for i in 0..p.cell_w.len() {
        for j in 0..p.div_z.len() {
            let w = f(p.cell_w[i]);
            let cube = if tag_support { gcube_tagged([w, d, dt]) } else { gcube([w, d, dt], [2, 2, 2]) };
            // screw head group
            let sx = w * 0.5 - f(p.screw_wgap);
            let m1 = d * 0.5;
            let gap = f(p.screw_wgap) + 0.0;
            let s1 = m1 - gap;
            let m2 = s1 * -1.0;
            let m3 = dt * -0.5;
            let cyl = gcylinder(32, f(p.screw_r), f(p.screw_depth), 2);
            let t1 = cyl.translated([sx, m2, m3]);
            let t6 = cyl.translated([sx, s1, m3]);
            let j2 = GMesh::join(&[t1, t6]);
            let t4 = j2.transformed(z3, z3, [-1.0, 1.0, 1.0]);
            let screws = GMesh::join(&[t4, j2]);
            let joined = GMesh::join(&[cube, screws]);
            boards.push(joined.translated([f(p.div_x[i]), 0.0, f(p.div_z[j])]));
        }
    }
    let boards = GMesh::join(&boards);
    let all = GMesh::join(&[frame, boards]);
    let tri = all.triangulated();
    let _ = one;
    tri.transformed(z3, [0.0, 0.0, -1.5708f32], one)
}

/// bbox_mesh_from_hipoly layout (cube triangulated; x fastest vertex order).
pub fn box_exact(lo: V3, hi: V3) -> ExactMesh {
    let mut verts = vec![];
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                verts.push([if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }]);
            }
        }
    }
    let polys: Vec<Vec<u32>> = [[2, 1, 0], [1, 4, 0], [5, 6, 4], [6, 3, 2], [4, 2, 0], [3, 5, 1], [2, 3, 1], [1, 5, 4], [5, 7, 6], [6, 7, 3], [4, 6, 2], [3, 7, 5]].iter().map(|p| p.to_vec()).collect();
    let n = ["bottom", "", "top", "", "back", "front"];
    ExactMesh { verts, polys, names: (0..12).map(|i| n[i % 6].to_string()).collect() }
}

/// new_bbox() layout used by create_placeholder overrides (z fastest vertex order).
pub fn new_bbox_exact(lo: V3, hi: V3) -> ExactMesh {
    let mut verts = vec![];
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                verts.push([if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }]);
            }
        }
    }
    let polys: Vec<Vec<u32>> = [[1, 2, 0], [3, 6, 2], [7, 4, 6], [5, 0, 4], [6, 0, 2], [3, 5, 7], [1, 3, 2], [3, 7, 6], [7, 5, 4], [5, 1, 0], [6, 4, 0], [3, 1, 5]].iter().map(|p| p.to_vec()).collect();
    let n = ["back", "", "front", "", "bottom", "top"];
    ExactMesh { verts, polys, names: (0..12).map(|i| n[i % 6].to_string()).collect() }
}

/// simple_bookcase.py geometry_nodes (float32, vertex order exact).
pub fn bookcase_gmesh(q: &BookParams) -> GMesh {
    let f = |x: f64| x as f32;
    let (d, w, h) = (f(q.d), f(q.w), f(q.h));
    let (st, dt, gap, bt) = (f(q.st), f(q.dt), f(q.gap), f(q.bt));
    let z3 = [0f32; 3];
    // side board
    let sub = w - st;
    let mh = h * 0.5;
    let left = gcube([st, d, h], [10, 10, 10]).translated([sub * -0.5, 0.0, mh]);
    let right = gcube([st, d, h], [10, 10, 10]).translated([sub * 0.5, 0.0, mh]);
    let side = GMesh::join(&[left, right]);
    // division boards
    let dbw = w - st * 2.0;
    let board = gcube_tagged([dbw, d, dt]);
    let m = dt * 0.5;
    let add = gap + m;
    let subt = h - m;
    let add1 = subt + add;
    let b1 = board.translated([0.0, 0.0, add]);
    let b2 = board.translated([0.0, 0.0, add1 * 0.5]);
    let b3 = board.translated([0.0, 0.0, subt]);
    // back board
    let back = gcube([w, bt, h], [10, 10, 10]).translated([0.0, d.mul_add(-0.5, bt * -0.5), h * 0.5]);
    let frame = GMesh::join(&[side, b1, b2, back, b3]);
    // screw heads
    let cyl = gcylinder(32, f(q.screw_r), f(q.screw_depth), 2).transformed(z3, [0.0, 1.5708f32, 0.0], [1.0; 3]);
    let sx = w * 0.5;
    let sg = f(q.screw_gap) + 0.0;
    let sy = d * 0.5 - sg;
    let ztop = h - dt * 0.5;
    let zbot = dt * 0.5 + gap;
    let ysy = sy * -1.0;
    let zmid = (ztop + zbot) * 0.5;
    let t2 = cyl.translated([sx, sy, ztop]);
    let t1 = cyl.translated([sx, sy, zbot]);
    let t3 = cyl.translated([sx, ysy, ztop]);
    let t5 = cyl.translated([sx, 0.0, zmid]);
    let t6 = cyl.translated([sx, ysy, zbot]);
    let j2 = GMesh::join(&[t2, t1, t3, t5, t6]);
    let t4 = j2.transformed(z3, z3, [-1.0, 1.0, 1.0]);
    let screws = GMesh::join(&[t4, j2]);
    // attach gadgets
    let (at, aw, atl, abl) = (f(q.at), f(q.aw), f(q.atl), f(q.abl));
    let ztop2 = h - dt;
    let a1 = gcube([aw, atl, at], [5, 5, 5]).translated([0.0, (d - atl) * -0.5, ztop2]);
    let a2 = gcube([aw, at, abl], [5, 5, 5]).translated([0.0, d * -0.5, ztop2 - abl * 0.5]);
    let attach = GMesh::join(&[a1, a2]);
    let all = GMesh::join(&[frame, screws, attach]);
    all.triangulated().transformed(z3, [0.0, 0.0, -1.5708f32], [1.0; 3])
}

/// BLI sin_cos_from_fraction(numerator, denominator): octant-symmetric float sin/cos (used by BMesh circle creation)
pub fn sin_cos_from_fraction(k: usize, n: usize) -> (f32, f32) {
    // blenlib/intern/math_rotation.c: map the numerator into the primary octant (all in integer arithmetic), then one sinf / cosf
    let mut numerator = (k * 8) as i64;
    let octant = numerator / n as i64;
    let denominator = (n * 8) as i64;
    let mut cos_sign = 1.0f32;
    let mut swap = false;
    match octant {
        0 => {}
        1 | 2 => {
            numerator = denominator / 4 - numerator;
            swap = true;
        }
        3 | 4 => {
            numerator = denominator / 2 - numerator;
            cos_sign = -1.0;
        }
        5 | 6 => {
            numerator = numerator - denominator * 3 / 4;
            swap = true;
            cos_sign = -1.0;
        }
        _ => {
            numerator = numerator - denominator;
        }
    }
    let angle = (2.0f64 * std::f64::consts::PI) as f32 * (numerator as f32 / denominator as f32);
    let (s, c) = (angle.sin(), angle.cos() * cos_sign);
    if swap {
        (c, s)
    } else {
        (s, c)
    }
}

/// cell_shelf.py geometry_nodes (float32, vertex order exact). has_base_frame = False, has_backboard = False, tag_support = True.
/// `cell_z`: the height of a cell (python: the same as `cell`; build-to-size boxes make cells taller or lower than wide)
pub fn cellshelf_gmesh(depth: f64, cell: f64, cell_z: f64, hn: usize, vn: usize, div_t: f64, ext_t: f64, attach: f64) -> GMesh {
    let f = |x: f64| x as f32;
    let (d, c, cz, dv, e, sz) = (f(depth), f(cell), f(cell_z), f(div_t), f(ext_t), f(attach));
    let (h, v) = (hn as f32, vn as f32);
    let leg = 0.0f32;
    let z3 = [0f32; 3];
    // side boards
    let z5 = {
        let sub = v - 1.0;
        let m = sub * dv;
        let m1 = v * cz;
        m + m1
    };
    let side = {
        let add_6 = e + dv * (h - 1.0);
        let add_7 = c * h + add_6;
        let m4 = 0.5f32 * add_7;
        let m5 = d * 0.5;
        let m6 = z5 * 0.5;
        let add_9 = e + (m6 + leg);
        let cube = gcube([e, d, z5], [5, 5, 5]);
        let t0 = cube.translated([m4, m5, add_9]);
        let t1 = cube.translated([m4 * -1.0, m5, add_9]);
        GMesh::join(&[t0, t1])
    };
    // top / bottom boards
    let (tb, x) = {
        let m = e * 2.0;
        let m1 = dv * (h - 1.0);
        let add_4 = m + m1;
        let add_6 = add_4 + c * h;
        let add_7 = add_6 + 0.002f32;
        let cube = gcube_tagged([add_7, d, e]);
        let m3 = d * 0.5;
        let m4 = e * 0.5;
        let add_10 = m4 + leg;
        let bottom = cube.translated([0.0, m3, add_10]);
        let add_11 = add_10 + e;
        let m5 = v * cz;
        let add_13 = add_11 + m5;
        let m6 = dv * (v - 1.0);
        let add_15 = add_13 + m6;
        let top = cube.translated([0.0, m3, add_15]);
        (GMesh::join(&[bottom, top]), add_7)
    };
    let mut comps = vec![side, tb];
    // vertical divisions
    if hn > 1 {
        let mesh = gcube([dv, d + -0.001f32, z5], [5, 5, 5]);
        let mut list = vec![];
        for i in 1..hn {
            let i = i as f32;
            let m = 0.5f32 * (h + -1.0);
            let sub = m - i;
            let add3 = sub + 0.5;
            let m1 = add3 * dv;
            let m2 = h * 0.5;
            let s1 = m2 - i;
            let m3 = c * s1;
            let add4 = m1 + m3;
            let m4 = d * 0.5;
            let add5 = leg + e;
            let m5 = z5 * 0.5;
            let add6 = add5 + m5;
            list.push(mesh.translated([add4, m4, add6]));
        }
        comps.push(GMesh::join(&list));
    }
    // horizontal divisions
    if vn > 1 {
        let w = c * h + (h + -1.0) * dv;
        let mesh = gcube_tagged([w, d, dv]);
        let mut list = vec![];
        for j in 1..vn {
            let j = j as f32;
            let m = d * 0.5;
            let m1 = j * cz;
            let add1 = j + -1.0;
            let m2 = add1 * dv;
            let add3 = m1 + m2;
            let add4 = e + leg;
            let m3 = dv * 0.5;
            let add5 = add4 + m3;
            let add6 = add3 + add5;
            list.push(mesh.translated([0.0, m, add6]));
        }
        comps.push(GMesh::join(&list));
    }
    // attach gadget (no backboard)
    {
        let cube = gcube([sz, 0.001f32, sz], [2, 2, 2]);
        let m = x * 0.5;
        let sub = m - e;
        let m1 = sz * 0.5;
        let s1 = sub - m1;
        let m2 = s1 * -1.0;
        let add1 = leg + z5;
        let add2 = add1 + e;
        let add3 = add2 + -0.02f32;
        let s2 = add3 - m1;
        let t6 = cube.translated([m2, 0.0, s2]);
        let t7 = cube.translated([s1, 0.0, s2]);
        comps.push(GMesh::join(&[t6, t7]));
    }
    let frame = GMesh::join(&comps);
    // screw heads
    let screws = {
        let sub = x - e;
        let m = sub * 0.5;
        let m1 = e * 0.5;
        let add1 = z5 + leg;
        let m2 = e * 2.0;
        let add2 = add1 + m2;
        let s1 = d - m1;
        let m3 = m * -1.0;
        let cyl = gcylinder(32, 0.005f32, 0.001f32, 1);
        let t2 = cyl.translated([m, m1, add2]);
        let t3 = cyl.translated([m, s1, add2]);
        let t4 = cyl.translated([m3, s1, add2]);
        let t5 = cyl.translated([m3, m1, add2]);
        GMesh::join(&[t2, t3, t4, t5])
    };
    let all = GMesh::join(&[frame, screws]);
    all.triangulated().transformed(z3, [0.0, 0.0, -1.5708f32], [1.0; 3])
}

/// simple_desk.py geometry_nodes (float32, vertex order exact): tagged top slab + 4 legs (128-gon cylinders), rotated +1.5708 about z.
pub fn desk_gmesh(depth: f64, width: f64, height: f64, leg_r: f64, leg_dist: f64, thick: f64) -> GMesh {
    let f = |x: f64| x as f32;
    let (d, w, h, r, dist, t) = (f(depth), f(width), f(height), f(leg_r), f(leg_dist), f(thick));
    let z3 = [0f32; 3];
    let add = t + 0.0;
    let top = gcube_tagged([w, d, add]).translated([0.0, 0.0, h - add * 0.5]);
    let sub = h - t;
    let cyl = gcylinder(128, r, sub, 1);
    let m = w * 0.5;
    let a = dist + 0.0;
    let x = m - a;
    let y = 0.5f32 * d - a;
    let z = sub * 0.5;
    let t0 = cyl.translated([x, y, z]);
    let t2 = cyl.translated([x * -1.0, y, z]);
    let t3 = cyl.translated([x, y * -1.0, z]);
    let t4 = cyl.translated([x * -1.0, y * -1.0, z]);
    let legs = GMesh::join(&[t0, t2, t3, t4]);
    let all = GMesh::join(&[top, legs]);
    all.triangulated().transformed(z3, [0.0, 0.0, 1.5708f32], [1.0; 3])
}

impl ExactMesh {
    /// tagging.tag_canonical_surfaces (rtol 0.01) followed by relabel: Back(min x), Front(max x), Top(max z), Bottom(min z)
    /// applied in this order on triangle meshes; face names are merged as sorted '.'-joined sets.
    pub fn with_canonical_tags(&self) -> ExactMesh {
        let mut o = self.clone();
        if o.names.len() != o.polys.len() {
            o.names = vec![String::new(); o.polys.len()];
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &o.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        for (tag, axis, is_max) in [("back", 0usize, false), ("front", 0, true), ("top", 2, true), ("bottom", 2, false)] {
            let target = if is_max { hi[axis] } else { lo[axis] };
            let atol = 0.01 * (hi[axis] - lo[axis]);
            // np.isclose(a, b, atol=atol) with default rtol 1e-5: |a - b| <= atol + rtol * |b|
            let ok: Vec<bool> = o.verts.iter().map(|v| (v[axis] - target).abs() <= atol + 1e-5 * target.abs()).collect();
            for (fi, p) in o.polys.iter().enumerate() {
                if p.len() == 3 && p.iter().all(|&i| ok[i as usize]) {
                    let ex = &o.names[fi];
                    let mut parts: Vec<String> = if ex.is_empty() { vec![] } else { ex.split('.').map(|s| s.to_string()).collect() };
                    if !parts.iter().any(|s| s == tag) {
                        parts.push(tag.to_string());
                    }
                    parts.sort();
                    o.names[fi] = parts.join(".");
                }
            }
        }
        o
    }
}

/// bke::mesh_calc_edges(keep_existing = false) for meshes with < 1000 faces: edges (min, max) in order of first appearance,
/// iterating faces and corners with (previous corner, corner)
pub fn calc_edges(faces: &[Vec<u32>]) -> Vec<[u32; 2]> {
    let mut seen = crate::dhash::HashSet::default();
    let mut out = vec![];
    for f in faces {
        for c in 0..f.len() {
            let v = f[c];
            let vp = f[(c + f.len() - 1) % f.len()];
            if vp == v {
                continue;
            }
            let e = [v.min(vp), v.max(vp)];
            if seen.insert(e) {
                out.push(e);
            }
        }
    }
    out
}

impl ExactMesh {
    /// triangulate remaining quads/ngons like the populate stage does (shortest diagonal / beauty ngons; first triangles in place, others appended)
    pub fn triangulated(&self) -> ExactMesh {
        let g = GMesh { v: self.verts.iter().map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]).collect(), p: self.polys.clone(), tag: self.names.iter().map(|n| if n == "support" { TAG_SUPPORT } else { 0 }).collect(), e: vec![] };
        if self.polys.iter().all(|p| p.len() == 3) {
            return self.clone();
        }
        let t = g.triangulated_m(true);
        ExactMesh { verts: self.verts.clone(), polys: t.p, names: t.tag.iter().map(|&x| if x == TAG_SUPPORT { "support".to_string() } else { String::new() }).collect() }
    }
}

#[derive(Clone, Copy)]
pub struct BookParams {
    pub d: f64,
    pub w: f64,
    pub h: f64,
    pub st: f64,
    pub dt: f64,
    pub gap: f64,
    pub bt: f64,
    pub screw_depth: f64,
    pub screw_r: f64,
    pub screw_gap: f64,
    pub at: f64,
    pub aw: f64,
    pub atl: f64,
    pub abl: f64,
}
