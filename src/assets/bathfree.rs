//! Freestanding bathtub (bathroom/bathtub.py make_freestanding + SOLIDIFY + SUBSURF), geometric port used for the asset bounding box.
//! Vertex/face ORDER is not Blender's (subsurf layout); positions are the limit surface of the same control cage.
use super::subsurf::subsurf;
use crate::mesh::Mesh;
use crate::dhash::HashMap;

type V3 = [f64; 3];

pub struct BowlParams {
    pub width: f64,
    pub size: f64,
    pub depth: f64,
    pub thickness: f64,
    pub taper: f64,
    pub has_curve: bool,
    pub disp_x: [f64; 2],
    pub disp_y: f64,
    pub alcove_levels: usize,
    pub levels: usize,
    pub side_levels: usize,
}

fn contour(p: &BowlParams, t: f64, i: f64) -> Vec<[f64; 2]> {
    let (w, s) = (p.width, p.size);
    vec![
        [t + p.disp_x[0] * i, t + p.disp_y * i],
        [w - t - p.disp_x[1] * i, t + p.disp_y * i],
        [w - t - p.disp_x[1] * i, s - t - p.disp_y * i],
        [t + p.disp_x[0] * i, s - t - p.disp_y * i],
    ]
}

/// bpy cylinder (n verts) topology, vertex 2i = bottom, 2i+1 = top
fn cylinder_topology(n: usize) -> Vec<Vec<u32>> {
    let mut f = vec![];
    for i in 0..n {
        let a = (2 * i) as u32;
        let m = (2 * n) as u32;
        f.push(vec![a, a + 1, (a + 3) % m, (a + 2) % m]);
    }
    let mut top = vec![3u32, 1];
    let mut k = 2 * n as i64 - 1;
    while k >= 5 {
        top.push(k as u32);
        k -= 2;
    }
    f.push(top);
    f.push((0..n).map(|i| 2 * i as u32).collect());
    f
}

pub fn make_bowl(p: &BowlParams) -> Mesh {
    let (lower, upper) = if p.has_curve { (contour(p, 0.0, 1.0), contour(p, 0.0, -1.0)) } else { (contour(p, 0.0, 0.0), contour(p, 0.0, 0.0)) };
    let n = lower.len();
    let mut m = Mesh::new();
    for k in 0..n {
        let (a, b) = (lower[n - 1 - k], upper[n - 1 - k]);
        m.pos.push([a[0], a[1], 0.0]);
        m.pos.push([b[0], b[1], p.depth * 2.0]);
    }
    m.faces = cylinder_topology(n);
    let m = subsurf(&m, p.alcove_levels, true);
    let lv = p.levels - p.alcove_levels - p.side_levels;
    subsurf(&m, lv, false)
}

pub fn remove_top(m: &Mesh, depth: f64) -> Mesh {
    // faces with centre z > depth are deleted, vertices not used by a remaining face are dropped (keeps order)
    let keep: Vec<&Vec<u32>> = m.faces.iter().filter(|f| f.iter().map(|&v| m.pos[v as usize][2]).sum::<f64>() / f.len() as f64 <= depth).collect();
    let mut remap = vec![u32::MAX; m.pos.len()];
    let mut o = Mesh::new();
    for f in &keep {
        for &v in f.iter() {
            if remap[v as usize] == u32::MAX {
                remap[v as usize] = o.pos.len() as u32;
                o.pos.push(m.pos[v as usize]);
            }
        }
    }
    o.faces = keep.iter().map(|f| f.iter().map(|&v| remap[v as usize]).collect()).collect();
    o
}

/// select all edges, region_to_loop, extrude_edges_move (no translation), resize(value) about the median of the new loop
pub fn extrude_rim_resize(m: &Mesh, scale: V3) -> Mesh {
    let mut cnt: HashMap<(u32, u32), (u32, u32, usize)> = HashMap::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let e = cnt.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
            e.2 += 1;
        }
    }
    let mut bedges: Vec<(u32, u32)> = cnt.values().filter(|e| e.2 == 1).map(|e| (e.0, e.1)).collect();
    bedges.sort();
    let mut dup: HashMap<u32, u32> = HashMap::default();
    let mut o = m.clone();
    for &(a, b) in &bedges {
        for v in [a, b] {
            dup.entry(v).or_insert_with(|| {
                o.pos.push(m.pos[v as usize]);
                (o.pos.len() - 1) as u32
            });
        }
    }
    // existing face winding along (a->b); the neighbour traverses b->a
    for &(a, b) in &bedges {
        o.faces.push(vec![b, a, dup[&a], dup[&b]]);
    }
    let nv: Vec<u32> = dup.values().cloned().collect();
    let mut med = [0.0; 3];
    for &v in &nv {
        for k in 0..3 {
            med[k] += o.pos[v as usize][k] / nv.len() as f64;
        }
    }
    for &v in &nv {
        for k in 0..3 {
            o.pos[v as usize][k] = med[k] + (o.pos[v as usize][k] - med[k]) * scale[k];
        }
    }
    o
}

fn deform_axis_x(m: &mut Mesh, taper: bool, factor: f64) {
    // SIMPLE_DEFORM, deform_axis X (axis_map {1,2,0}): z_r = x, x_r = y, y_r = z ; limits [0,1] never clip
    let f = factor as f32;
    let (mut lo, mut hi) = (f32::MAX, -f32::MAX);
    for p in &m.pos {
        lo = lo.min(p[0] as f32);
        hi = hi.max(p[0] as f32);
    }
    let sf = f / (hi - lo).max(f32::EPSILON);
    for p in m.pos.iter_mut() {
        let (x_r, y_r, z_r) = (p[1] as f32, p[2] as f32, p[0] as f32);
        let (nx, ny, nz) = if taper {
            let sc = z_r * sf;
            (x_r + x_r * sc, y_r + y_r * sc, z_r)
        } else {
            let sc = z_r * z_r * sf - sf + 1.0;
            (x_r * sc, y_r * sc, z_r * (1.0 + sf))
        };
        *p = [nz as f64, nx as f64, ny as f64];
    }
}

pub fn vertex_normals(m: &Mesh) -> Vec<V3> {
    let mut n = vec![[0.0f64; 3]; m.pos.len()];
    for f in &m.faces {
        let k = f.len();
        // face normal (Newell)
        let mut fnrm = [0.0; 3];
        for i in 0..k {
            let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % k] as usize]);
            fnrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
            fnrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
            fnrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let l = (fnrm[0] * fnrm[0] + fnrm[1] * fnrm[1] + fnrm[2] * fnrm[2]).sqrt();
        if l == 0.0 {
            continue;
        }
        for c in &mut fnrm {
            *c /= l;
        }
        for i in 0..k {
            let (p, q, r) = (m.pos[f[(i + k - 1) % k] as usize], m.pos[f[i] as usize], m.pos[f[(i + 1) % k] as usize]);
            let norm = |a: V3| {
                let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
                [a[0] / l, a[1] / l, a[2] / l]
            };
            let d1 = norm([p[0] - q[0], p[1] - q[1], p[2] - q[2]]);
            let d2 = norm([r[0] - q[0], r[1] - q[1], r[2] - q[2]]);
            let ang = (d1[0] * d2[0] + d1[1] * d2[1] + d1[2] * d2[2]).clamp(-1.0, 1.0).acos();
            for c in 0..3 {
                n[f[i] as usize][c] += fnrm[c] * ang;
            }
        }
    }
    for v in n.iter_mut() {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.0 {
            for c in v.iter_mut() {
                *c /= l;
            }
        }
    }
    n
}

/// SOLIDIFY modifier (EXTRUDE mode, default flags: rim on, no even offset, no clamp) with `offset` in [-1, 1]
pub fn solidify(m: &Mesh, thickness: f64, offset: f64) -> Mesh {
    let nrm = vertex_normals(m);
    let ofs_orig = -(((-offset + 1.0) * 0.5) * thickness);
    let ofs_new = thickness + ofs_orig;
    let nv = m.pos.len();
    let mut o = Mesh::new();
    o.pos = m.pos.clone();
    for (i, p) in m.pos.iter().enumerate() {
        o.pos.push(*p);
        let _ = i;
    }
    // INIT_VERT_ARRAY_OFFSETS (do_flip = false): which vertex copy receives which displacement
    let idx_new = if ofs_new >= ofs_orig { 0 } else { nv };
    let idx_orig = if ofs_new < ofs_orig { 0 } else { nv };
    for i in 0..nv {
        for c in 0..3 {
            if ofs_new != 0.0 {
                o.pos[idx_new + i][c] += nrm[i][c] * ofs_new;
            }
            if ofs_orig != 0.0 {
                o.pos[idx_orig + i][c] += nrm[i][c] * ofs_orig;
            }
        }
    }
    // faces: originals, then reversed copies
    for f in &m.faces {
        o.faces.push(f.clone());
    }
    for f in &m.faces {
        o.faces.push(f.iter().rev().map(|&v| v + nv as u32).collect());
    }
    // rim: boundary edges
    let mut cnt: HashMap<(u32, u32), (u32, u32, usize)> = HashMap::default();
    for f in &m.faces {
        let k = f.len();
        for i in 0..k {
            let (a, b) = (f[i], f[(i + 1) % k]);
            let e = cnt.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
            e.2 += 1;
        }
    }
    let mut be: Vec<(u32, u32)> = cnt.values().filter(|e| e.2 == 1).map(|e| (e.0, e.1)).collect();
    be.sort();
    for (a, b) in be {
        o.faces.push(vec![b, a, a + nv as u32, b + nv as u32]);
    }
    o
}

pub struct Stages {
    pub bowl: Mesh,
    pub removetop: Mesh,
    pub freestanding: Mesh,
    pub solid: Mesh,
    pub side: Mesh,
    pub fin: Mesh,
}

pub fn pipeline(p: &BowlParams, zshift_u: f64) -> Stages {
    let bowl = make_bowl(p);
    let rt = remove_top(&bowl, p.depth);
    let mut m = extrude_rim_resize(&rt, [1.0 + p.thickness * 2.0 / p.width, 1.0 + p.thickness / p.size, 1.0]);
    for q in m.pos.iter_mut() {
        q[1] -= p.size / 2.0;
    }
    deform_axis_x(&mut m, true, p.taper);
    deform_axis_x(&mut m, false, p.taper);
    let minz = m.pos.iter().map(|q| q[2]).fold(f64::INFINITY, f64::min);
    for q in m.pos.iter_mut() {
        q[1] += p.size / 2.0;
        q[2] += -minz * zshift_u;
    }
    let free = m.clone();
    let solid = solidify(&m, p.thickness, -1.0);
    let side = subsurf(&solid, p.side_levels, false);
    let fin = subsurf(&side, 1, false);
    Stages { bowl, removetop: rt, freestanding: free, solid, side, fin }
}
