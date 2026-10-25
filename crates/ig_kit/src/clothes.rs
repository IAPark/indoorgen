//! clothes/blanket.py (Blanket, Comforter, BoxComforter) and shared Blender-op emulation used by the cloth assets
//! (grid primitive, SOLIDIFY modifier, object transform apply in float32).
use crate::mesh::{Mesh, V3};
use crate::rng::Rng;

// ------------------------------------------------------------------ Blender primitive / modifier emulation

/// bpy.ops.mesh.primitive_grid_add(x_subdivisions=nx, y_subdivisions=ny): (nx+1)*(ny+1) verts, x fastest, spanning [-1,1]^2 (size 2).
/// Also returns the BMesh edge creation order (per face, row-major: left, bottom, right, top; new edges only).
pub fn grid(nx: usize, ny: usize) -> (Mesh, Vec<(u32, u32)>) {
    let (xt, yt) = (nx + 1, ny + 1);
    let mut m = Mesh::new();
    let (xi, yi) = (2.0f32 / (xt as f32 - 1.0), 2.0f32 / (yt as f32 - 1.0));
    for y in 0..yt {
        for x in 0..xt {
            m.pos.push([((x as f32 * xi) - 1.0) as f64, ((y as f32 * yi) - 1.0) as f64, 0.0]);
        }
    }
    let mut edges: Vec<(u32, u32)> = vec![];
    let mut seen = crate::dhash::HashSet::default();
    let mut add = |a: u32, b: u32, edges: &mut Vec<(u32, u32)>| {
        if seen.insert((a.min(b), a.max(b))) {
            edges.push((a, b));
        }
    };
    for y in 0..ny {
        for x in 0..nx {
            let i = (y * xt + x) as u32;
            let (i1, j, j1) = (i + 1, i + xt as u32, i + xt as u32 + 1);
            m.faces.push(vec![i, i1, j1, j]);
            add(j, i, &mut edges);
            add(i, i1, &mut edges);
            add(i1, j1, &mut edges);
            add(j1, j, &mut edges);
        }
    }
    (m, edges)
}

/// Blender object transform (scale, euler z, location) applied to mesh data in float32 (`mul_m4_v3`), like `apply_transform(obj, True)`.
pub fn apply_srt_f32(m: &mut Mesh, scale: [f32; 3], rz: f32, loc: [f32; 3]) {
    let (s, c) = (rz.sin(), rz.cos());
    // rotation about z: x' = c*x - s*y ; y' = s*x + c*y (matrix columns as in eul_to_mat3 for a pure z rotation)
    for p in m.pos.iter_mut() {
        let (x, y, z) = (p[0] as f32 * scale[0], p[1] as f32 * scale[1], p[2] as f32 * scale[2]);
        let nx = c * x + (-s) * y + 0.0 * z + loc[0];
        let ny = s * x + c * y + 0.0 * z + loc[1];
        let nz = 0.0 * x + 0.0 * y + 1.0 * z + loc[2];
        *p = [nx as f64, ny as f64, nz as f64];
    }
}

/// SOLIDIFY modifier (simple mode, no rim fill customisations): vertices = [original (displaced by ofs_orig), copies (ofs_new)],
/// faces = [original, reversed copies, side quads for boundary edges in `edges` order].
/// `offset` in [-1,1] (-1: shell inside the surface); vertex normals are angle-weighted averages of face normals.
pub fn solidify_simple(m: &Mesh, edges: &[(u32, u32)], thickness: f64, offset: f64) -> Mesh {
    let n = m.pos.len();
    let nrm = vertex_normals(m);
    let ofs_orig = (offset + 1.0) * 0.5 * thickness;
    let ofs_new = ofs_orig - thickness;
    let mut out = Mesh::new();
    for i in 0..n {
        out.pos.push([m.pos[i][0] + nrm[i][0] * ofs_orig, m.pos[i][1] + nrm[i][1] * ofs_orig, m.pos[i][2] + nrm[i][2] * ofs_orig]);
    }
    for i in 0..n {
        out.pos.push([m.pos[i][0] + nrm[i][0] * ofs_new, m.pos[i][1] + nrm[i][1] * ofs_new, m.pos[i][2] + nrm[i][2] * ofs_new]);
    }
    let nn = n as u32;
    for f in &m.faces {
        out.faces.push(f.clone());
    }
    for f in &m.faces {
        let mut r: Vec<u32> = vec![f[0] + nn];
        for k in (1..f.len()).rev() {
            r.push(f[k] + nn);
        }
        out.faces.push(r);
    }
    // boundary edges: used by exactly one face, with the loop direction a->b of that face
    let mut dir: crate::dhash::HashMap<(u32, u32), (u32, u32, u32)> = Default::default();
    for f in &m.faces {
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            let e = dir.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
            e.2 += 1;
        }
    }
    for &(p, q) in edges {
        if let Some(&(a, b, cnt)) = dir.get(&(p.min(q), p.max(q))) {
            if cnt == 1 {
                out.faces.push(vec![b, a, a + nn, b + nn]);
            }
        }
    }
    out
}

pub fn vertex_normals(m: &Mesh) -> Vec<V3> {
    let mut acc = vec![[0.0f64; 3]; m.pos.len()];
    for f in &m.faces {
        let n = f.len();
        let mut fnrm = [0.0; 3];
        for k in 0..n {
            let (a, b) = (m.pos[f[k] as usize], m.pos[f[(k + 1) % n] as usize]);
            fnrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
            fnrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
            fnrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let l = (fnrm[0] * fnrm[0] + fnrm[1] * fnrm[1] + fnrm[2] * fnrm[2]).sqrt();
        if l < 1e-300 {
            continue;
        }
        for v in fnrm.iter_mut() {
            *v /= l;
        }
        for k in 0..n {
            let p = m.pos[f[k] as usize];
            let a = sub(m.pos[f[(k + 1) % n] as usize], p);
            let b = sub(m.pos[f[(k + n - 1) % n] as usize], p);
            let (la, lb) = (norm(a), norm(b));
            if la < 1e-300 || lb < 1e-300 {
                continue;
            }
            let cosang = ((a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (la * lb)).clamp(-1.0, 1.0);
            let w = cosang.acos();
            for c in 0..3 {
                acc[f[k] as usize][c] += fnrm[c] * w;
            }
        }
    }
    for a in acc.iter_mut() {
        let l = norm(*a);
        if l > 1e-300 {
            *a = [a[0] / l, a[1] / l, a[2] / l];
        }
    }
    acc
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn norm(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

// ------------------------------------------------------------------ BlanketFactory family

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BlanketKind {
    Blanket,
    Comforter,
    BoxComforter,
}

#[derive(Clone, Debug)]
pub struct Blanket {
    pub kind: BlanketKind,
    pub width: f64,
    pub size: f64,
    pub thickness: f64,
    pub margin: f64,
}

impl Blanket {
    /// constructor draws on the stream the python __init__ runs on (width, size ratio, thickness); material draws are made by the caller
    /// via `ctor_draws::blanket_material_draws` before `draw_margin`.
    pub fn from_draws(kind: BlanketKind, r: &mut Rng) -> Blanket {
        let width = r.log_uniform(0.9, 1.2);
        let size = width * r.log_uniform(0.4, 0.7);
        let thickness = r.log_uniform(0.004, 0.008);
        Blanket { kind, width, size, thickness, margin: 0.0 }
    }
    pub fn draw_margin(&mut self, r: &mut Rng) {
        self.margin = r.uniform(0.3, 0.4);
    }
    pub fn from_unseeded(kind: BlanketKind, vals: &[f64]) -> Blanket {
        Blanket { kind, width: vals[0], size: vals[1], thickness: vals[2], margin: vals.get(3).copied().unwrap_or(0.0) }
    }

    /// BlanketFactory.create_asset: flat grid, centered at the origin
    fn grid_mesh(&self) -> (Mesh, Vec<(u32, u32)>) {
        let ny = (self.size / self.width * 64.0) as usize;
        let (mut m, e) = grid(64, ny);
        apply_srt_f32(&mut m, [(self.width / 2.0) as f32, (self.size / 2.0) as f32, 1.0], 0.0, [0.0; 3]);
        (m, e)
    }

    pub fn create_asset(&self) -> Mesh {
        let (m, e) = self.grid_mesh();
        match self.kind {
            BlanketKind::Blanket => m,
            _ => {
                let s = solidify_simple(&m, &e, 0.01, -1.0);
                if self.kind == BlanketKind::Comforter {
                    s
                } else {
                    self.box_pinch(s)
                }
            }
        }
    }

    /// BoxComforter: weld (remove_doubles 0.02) the selected vertex lines of the solidified grid (pinches top and bottom layer together)
    fn box_pinch(&self, mut s: Mesh) -> Mesh {
        let margin = self.margin;
        let cell = self.width / 64.0 / 2.0;
        let n = s.pos.len() / 2;
        let sel: Vec<bool> = s.pos.iter().map(|p| {
            let (x, y) = (p[0], p[1]);
            ((x / margin - (x / margin).round()).abs() * margin < cell) || ((y / margin - (y / margin).round()).abs() * margin < cell)
        }).collect();
        let _ = n;
        remove_doubles(&mut s, &sel, 0.02);
        s
    }
}

/// bpy.ops.mesh.remove_doubles(threshold) restricted to selected vertices: merges vertices closer than `thr` into the one that
/// sorts first (BMesh sorts candidates by x+y+z), drops faces that collapse, keeps the remaining vertex/face order.
pub fn remove_doubles(m: &mut Mesh, sel: &[bool], thr: f64) {
    let nv = m.pos.len();
    let mut order: Vec<usize> = (0..nv).filter(|&i| sel[i]).collect();
    let key = |i: usize| m.pos[i][0] as f32 + m.pos[i][1] as f32 + m.pos[i][2] as f32;
    order.sort_by(|&a, &b| key(a).partial_cmp(&key(b)).unwrap());
    let mut target: Vec<u32> = (0..nv as u32).collect();
    let mut merged = vec![false; nv];
    for ii in 0..order.len() {
        let i = order[ii];
        if merged[i] {
            continue;
        }
        let ki = key(i);
        for jj in ii + 1..order.len() {
            let j = order[jj];
            if (key(j) - ki) as f64 > thr * 3.0 {
                break;
            }
            if merged[j] {
                continue;
            }
            let d = sub(m.pos[i], m.pos[j]);
            if norm(d) <= thr {
                target[j] = i as u32;
                merged[j] = true;
            }
        }
    }
    // compact
    let mut newidx = vec![u32::MAX; nv];
    let mut pos = vec![];
    for i in 0..nv {
        if !merged[i] {
            newidx[i] = pos.len() as u32;
            pos.push(m.pos[i]);
        }
    }
    let mut faces = vec![];
    for f in &m.faces {
        let mut g: Vec<u32> = vec![];
        for &v in f {
            let t = newidx[target[v as usize] as usize];
            if g.last() != Some(&t) {
                g.push(t);
            }
        }
        while g.len() > 1 && g.first() == g.last() {
            g.pop();
        }
        let mut uniq = g.clone();
        uniq.sort();
        uniq.dedup();
        if g.len() >= 3 && uniq.len() == g.len() {
            faces.push(g);
        }
    }
    m.pos = pos;
    m.faces = faces;
}
