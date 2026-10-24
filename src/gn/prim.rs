//! Mesh / curve primitives (Blender 4.2 vertex and face layouts).
use super::geom::*;
use super::meshops::*;
use super::val::*;
use crate::assets::cabnodes::gcube;
use crate::assets::gn as g32;

pub fn mesh_cube(size: V3, n: [i32; 3]) -> Mesh {
    let n = [n[0].max(1) as usize, n[1].max(1) as usize, n[2].max(1) as usize];
    let n = [n[0].max(2), n[1].max(2), n[2].max(2)];
    let mut m = from_gmesh(&gcube(size, n));
    set_sharp_all(&mut m);
    m
}

/// Cylinder (cone=false: centred, z in +-depth/2) or cone (cone=true: z in [0, depth] like Blender 4.2).
/// fill: 0 none, 1 ngon, 2 triangle fan. Layout verified against Blender: [top centre][top fill rings inner->outer]
/// [side rings][bottom fill rings outer->inner][bottom centre].
pub fn mesh_cone(n: usize, rtop: f32, rbot: f32, depth: f32, side_segs: usize, fill_segs: usize, fill: u8, cone: bool) -> Mesh {
    let n = n.max(3);
    let rings = side_segs.max(1);
    let fsegs = fill_segs.max(1);
    let top_point = rtop == 0.0;
    let bot_point = rbot == 0.0;
    let top_center = top_point || fill == 2;
    let bot_center = bot_point || fill == 2;
    let ad = (2.0f64 * (std::f64::consts::PI / n as f64)) as f32;
    let (ztop, zbot) = if cone { (depth, 0.0) } else { (depth * 0.5, -depth * 0.5) };
    let mut circ: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut ang = 0.0f32;
    for _ in 0..n {
        circ.push([ang.cos(), ang.sin()]);
        ang += ad;
    }
    let nn = n as u32;
    let mut pos: Vec<V3> = vec![];
    let mut push_ring = |pos: &mut Vec<V3>, rad: f32, z: f32| -> u32 {
        let s = pos.len() as u32;
        for c in &circ {
            pos.push([c[0] * rad, c[1] * rad, z]);
        }
        s
    };
    if top_center {
        pos.push([0.0, 0.0, ztop]);
    }
    let apex_top = 0u32;
    // top fill rings (inner -> outer), only when a cap exists
    let mut top_fill: Vec<u32> = vec![];
    if !top_point && fill != 0 {
        for k in 1..fsegs {
            top_fill.push(push_ring(&mut pos, rtop * k as f32 / fsegs as f32, ztop));
        }
    }
    let mut ring_start: Vec<Option<u32>> = vec![];
    for r in 0..=rings {
        let is_pt = (r == 0 && top_point) || (r == rings && bot_point);
        if is_pt {
            ring_start.push(None);
            continue;
        }
        let (rad, z) = if r == 0 {
            (rtop, ztop)
        } else if r == rings {
            (rbot, zbot)
        } else {
            let t = r as f32 / rings as f32;
            (rtop * (1.0 - t) + rbot * t, ztop - depth * t)
        };
        ring_start.push(Some(push_ring(&mut pos, rad, z)));
    }
    let mut bot_fill: Vec<u32> = vec![];
    if !bot_point && fill != 0 {
        for k in (1..fsegs).rev() {
            bot_fill.push(push_ring(&mut pos, rbot * k as f32 / fsegs as f32, zbot));
        }
    }
    let apex_bot = pos.len() as u32;
    if bot_center {
        pos.push([0.0, 0.0, zbot]);
    }
    let mut faces: Vec<Vec<u32>> = vec![];
    // top cap
    if !top_point {
        let outer = ring_start[0].unwrap();
        let inner = top_fill.first().copied().unwrap_or(outer);
        if fill == 2 {
            for i in 0..nn {
                faces.push(vec![inner + i, inner + (i + 1) % nn, apex_top]);
            }
        } else if fill == 1 {
            faces.push((0..nn).map(|i| inner + i).collect());
        }
        // fill rings inner -> outer
        let mut chain = top_fill.clone();
        chain.push(outer);
        for w in chain.windows(2) {
            for i in 0..nn {
                let j = (i + 1) % nn;
                faces.push(vec![w[0] + i, w[1] + i, w[1] + j, w[0] + j]);
            }
        }
    }
    // sides
    for r in 0..rings {
        for i in 0..nn {
            let j = (i + 1) % nn;
            match (ring_start[r], ring_start[r + 1]) {
                (Some(a), Some(b)) => faces.push(vec![a + i, b + i, b + j, a + j]),
                (None, Some(b)) => faces.push(vec![b + i, b + j, apex_top]),
                (Some(a), None) => faces.push(vec![a + i, apex_bot, a + j]),
                _ => {}
            }
        }
    }
    // bottom cap
    if !bot_point {
        let outer = ring_start[rings].unwrap();
        let mut chain = vec![outer];
        chain.extend(bot_fill.iter().copied());
        for w in chain.windows(2) {
            for i in 0..nn {
                let j = (i + 1) % nn;
                faces.push(vec![w[0] + i, w[1] + i, w[1] + j, w[0] + j]);
            }
        }
        let inner = *chain.last().unwrap();
        if fill == 2 {
            for i in 0..nn {
                faces.push(vec![inner + i, apex_bot, inner + (i + 1) % nn]);
            }
        } else if fill == 1 {
            faces.push((0..nn).rev().map(|i| inner + i).collect());
        }
    }
    let mut m = Mesh::from_faces(pos, &faces, &[]);
    set_sharp_all(&mut m);
    m
}

/// all faces flat shaded (sharp_face = true), as the 4.2 primitives do
pub fn set_sharp_all(m: &mut Mesh) {
    if m.nf() > 0 {
        m.attrs.insert("sharp_face".into(), Attr { dom: Dom::Face, ty: Ty::B, data: vec![Val::B(true); m.nf()] });
    }
}

pub fn mesh_uv_sphere(segs: usize, rings: usize, radius: f32) -> Mesh {
    let segs = segs.max(3);
    let rings = rings.max(2);
    let mut pos: Vec<V3> = vec![[0.0, 0.0, radius]];
    let delta_theta = (std::f64::consts::PI * 2.0 / segs as f64) as f32;
    let delta_phi = (std::f64::consts::PI / rings as f64) as f32;
    let _ = delta_theta;
    for r in 1..rings {
        let phi = delta_phi * r as f32;
        let z = phi.cos() * radius;
        let rr = phi.sin() * radius;
        for s in 0..segs {
            let theta = delta_theta * (s + 1) as f32;
            pos.push([theta.cos() * rr, theta.sin() * rr, z]);
        }
    }
    pos.push([0.0, 0.0, -radius]);
    let nv = pos.len() as u32;
    let sg = segs as u32;
    let mut faces: Vec<Vec<u32>> = vec![];
    // top fan
    for s in 0..sg {
        faces.push(vec![0, 1 + s, 1 + (s + 1) % sg]);
    }
    for r in 0..(rings as u32 - 2) {
        let a = 1 + r * sg;
        let b = a + sg;
        for s in 0..sg {
            let sn = (s + 1) % sg;
            faces.push(vec![a + s, b + s, b + sn, a + sn]);
        }
    }
    let last = 1 + (rings as u32 - 2) * sg;
    for s in 0..sg {
        faces.push(vec![nv - 1, last + (s + 1) % sg, last + s]);
    }
    let mut m = Mesh::from_faces(pos, &faces, &[]);
    set_sharp_all(&mut m);
    m.vnor = Some(m.pos.iter().map(|p| super::math::norm3_c(*p)).collect());
    m
}

pub fn mesh_line(count: usize, start: V3, delta: V3) -> Mesh {
    let mut m = Mesh::new();
    for i in 0..count {
        m.pos.push([start[0] + delta[0] * i as f32, start[1] + delta[1] * i as f32, start[2] + delta[2] * i as f32]);
    }
    for i in 0..count.saturating_sub(1) {
        m.edges.push([i as u32, i as u32 + 1]);
    }
    m
}

pub fn mesh_grid(sx: f32, sy: f32, vx: usize, vy: usize) -> Mesh {
    let (vx, vy) = (vx.max(1), vy.max(1));
    let mut pos = vec![];
    // Blender evaluates the grid positions in double precision
    let g = |s: f32, n: usize, i: usize| -> f32 { if n > 1 { ((-(s as f64)) / 2.0 + i as f64 * (s as f64 / (n - 1) as f64)) as f32 } else { 0.0 } };
    for x in 0..vx {
        for y in 0..vy {
            let px = g(sx, vx, x);
            let py = g(sy, vy, y);
            pos.push([px, py, 0.0]);
        }
    }
    let mut faces = vec![];
    for x in 0..vx.saturating_sub(1) {
        for y in 0..vy.saturating_sub(1) {
            let a = (x * vy + y) as u32;
            let b = ((x + 1) * vy + y) as u32;
            faces.push(vec![a, b, b + 1, a + 1]);
        }
    }
    // Blender edge order: columns (y direction) first, then rows (x direction)
    let mut edges: Vec<[u32; 2]> = vec![];
    for x in 0..vx {
        for y in 0..vy.saturating_sub(1) {
            edges.push([(x * vy + y) as u32, (x * vy + y + 1) as u32]);
        }
    }
    for y in 0..vy {
        for x in 0..vx.saturating_sub(1) {
            edges.push([(x * vy + y) as u32, ((x + 1) * vy + y) as u32]);
        }
    }
    let mut m = Mesh::from_faces(pos, &faces, &edges);
    set_sharp_all(&mut m);
    m
}

pub fn mesh_circle(n: usize, radius: f32, fill: u8) -> Mesh {
    let n = n.max(3);
    let step = (std::f64::consts::PI * 2.0 / n as f64) as f32;
    let mut pos: Vec<V3> = (0..n).map(|i| {
        let a = step * i as f32;
        [a.cos() * radius, a.sin() * radius, 0.0]
    }).collect();
    let mut faces: Vec<Vec<u32>> = vec![];
    let nn = n as u32;
    let mut edges: Vec<[u32; 2]> = (0..nn).map(|i| [i, (i + 1) % nn]).collect();
    match fill {
        1 => faces.push((0..nn).collect()),
        2 => {
            pos.push([0.0, 0.0, 0.0]);
            for i in 0..nn {
                faces.push(vec![i, (i + 1) % nn, nn]);
            }
            edges.extend((0..nn).map(|i| [i, nn]));
        }
        _ => {}
    }
    let mut m = Mesh::from_faces(pos, &faces, &edges);
    set_sharp_all(&mut m);
    m
}

// ------------------------------------------------------------------ curves
pub fn curve_line(a: V3, b: V3) -> Curves {
    let mut c = Curves::new();
    c.push_poly(&[a, b], false);
    c
}

pub fn curve_circle(res: usize, radius: f32) -> Curves {
    let s = g32::curve_circle(radius, res.max(3));
    let mut c = Curves::new();
    c.push_poly(&s.pts, true);
    c
}

pub fn curve_quadrilateral_rect(w: f32, h: f32) -> Curves {
    let (x, y) = (w * 0.5, h * 0.5);
    let mut c = Curves::new();
    c.push_poly(&[[x, y, 0.0], [-x, y, 0.0], [-x, -y, 0.0], [x, -y, 0.0]], true);
    c
}

pub fn curve_quadratic_bezier(res: usize, p1: V3, p2: V3, p3: V3) -> Curves {
    let n = res.max(3) + 1;
    let mut pts = vec![];
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let q1 = lerp3(p1, p2, t);
        let q2 = lerp3(p2, p3, t);
        pts.push(lerp3(q1, q2, t));
    }
    let mut c = Curves::new();
    c.push_poly(&pts, false);
    c
}

pub fn lerp3(a: V3, b: V3, t: f32) -> V3 {
    [a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t, a[2] * (1.0 - t) + b[2] * t]
}

/// Blender's icosphere: icosahedron with the (rounded) bmesh constants, `sub` = Subdivisions (1 = 12 vertices)
pub fn mesh_ico_sphere(radius: f32, sub: usize) -> Mesh {
    const ICOVERT: [[f32; 3]; 12] = [
        [0.0, 0.0, -200.0],
        [144.72, -105.144, -89.443],
        [-55.277, -170.128, -89.443],
        [-178.885, 0.0, -89.443],
        [-55.277, 170.128, -89.443],
        [144.72, 105.144, -89.443],
        [55.277, -170.128, 89.443],
        [-144.72, -105.144, 89.443],
        [-144.72, 105.144, 89.443],
        [55.277, 170.128, 89.443],
        [178.885, 0.0, 89.443],
        [0.0, 0.0, 200.0],
    ];
    const ICOFACE: [[u32; 3]; 20] = [
        [0, 1, 2], [1, 0, 5], [0, 2, 3], [0, 3, 4], [0, 4, 5], [1, 5, 10], [2, 1, 6], [3, 2, 7], [4, 3, 8], [5, 4, 9],
        [1, 10, 6], [2, 6, 7], [3, 7, 8], [4, 8, 9], [5, 9, 10], [6, 10, 11], [7, 6, 11], [8, 7, 11], [9, 8, 11], [10, 9, 11],
    ];
    let mut pos: Vec<V3> = ICOVERT.iter().map(|v| [v[0] / 200.0 * radius, v[1] / 200.0 * radius, v[2] / 200.0 * radius]).collect();
    let mut faces: Vec<[u32; 3]> = ICOFACE.to_vec();
    for _ in 1..sub {
        let mut cache: crate::dhash::HashMap<(u32, u32), u32> = crate::dhash::HashMap::default();
        let mut nf = vec![];
        let mut mid = |a: u32, b: u32, pos: &mut Vec<V3>| -> u32 {
            let key = (a.min(b), a.max(b));
            if let Some(&i) = cache.get(&key) {
                return i;
            }
            let (pa, pb) = (pos[a as usize], pos[b as usize]);
            let m = [(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5, (pa[2] + pb[2]) * 0.5];
            pos.push(m);
            let i = pos.len() as u32 - 1;
            cache.insert(key, i);
            i
        };
        for f in &faces {
            let (a, b, c) = (f[0], f[1], f[2]);
            let ab = mid(a, b, &mut pos);
            let bc = mid(b, c, &mut pos);
            let ca = mid(c, a, &mut pos);
            nf.push([a, ab, ca]);
            nf.push([ab, b, bc]);
            nf.push([ca, bc, c]);
            nf.push([ab, bc, ca]);
        }
        faces = nf;
    }
    if sub > 1 {
        for p in pos.iter_mut() {
            let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            if l > 0.0 {
                *p = [p[0] / l * radius, p[1] / l * radius, p[2] / l * radius];
            }
        }
    }
    let fl: Vec<Vec<u32>> = faces.iter().map(|f| f.to_vec()).collect();
    let mut m = Mesh::from_faces(pos, &fl, &[]);
    set_sharp_all(&mut m);
    m
}
