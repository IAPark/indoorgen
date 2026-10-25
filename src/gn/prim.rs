//! Mesh / curve primitives (Blender 4.2 vertex and face layouts).
use super::geom::*;
use super::meshops::*;
use super::val::*;
use crate::assets::cabnodes::gcube;
use crate::assets::gnlite as g32;

pub fn mesh_cube(size: V3, n: [i32; 3]) -> Mesh {
    let n = [n[0].max(1) as usize, n[1].max(1) as usize, n[2].max(1) as usize];
    let n = [n[0].max(2), n[1].max(2), n[2].max(2)];
    let mut m = from_gmesh(&gcube(size, n));
    set_sharp_all(&mut m);
    m
}

/// outputs of the Cylinder / Cone nodes
pub struct ConeOut {
    pub mesh: Mesh,
    /// selection fields: (domain, values)
    pub top: (Dom, Vec<bool>),
    pub bottom: (Dom, Vec<bool>),
    pub side: Vec<bool>,
    /// UV map (corner domain)
    pub uv: Vec<[f32; 2]>,
}

/// Exact port of geometry::create_cylinder_or_cone_mesh (mesh_primitive_cylinder_cone.cc).
/// fill: 0 none, 1 ngon, 2 triangle fan.  For cones the node translates the mesh by depth/2 in z.
pub fn mesh_cone(n: usize, rtop: f32, rbot: f32, depth: f32, side_segs: usize, fill_segs: usize, fill: u8, cone: bool) -> ConeOut {
    let cs = n.max(3);
    let ss = side_segs.max(1);
    let fsg = fill_segs.max(1);
    let fill_segments = if fill == 0 { 1 } else { fsg };
    let radius_top = rtop;
    let radius_bottom = rbot;
    let height = 0.5f32 * depth;
    let top_is_point = radius_top == 0.0;
    let bottom_is_point = radius_bottom == 0.0;
    let top_has_center = top_is_point || fill == 2;
    let bottom_has_center = bottom_is_point || fill == 2;
    // totals
    let quad_rings = if top_is_point && bottom_is_point {
        0
    } else {
        let mut q = 0;
        if !top_is_point {
            q += fill_segments - 1;
        }
        q += if !top_is_point && !bottom_is_point { ss } else { ss - 1 };
        if !bottom_is_point {
            q += fill_segments - 1;
        }
        q
    };
    let edge_rings = if top_is_point && bottom_is_point {
        0
    } else {
        let mut e = 0;
        if !top_is_point {
            e += fill_segments;
        }
        e += ss - 1;
        if !bottom_is_point {
            e += fill_segments;
        }
        e
    };
    let tot_verts = if top_is_point && bottom_is_point {
        ss + 1
    } else {
        let mut v = 0;
        if top_has_center {
            v += 1;
        }
        if !top_is_point {
            v += cs * fill_segments;
        }
        v += cs * (ss - 1);
        if !bottom_is_point {
            v += cs * fill_segments;
        }
        if bottom_has_center {
            v += 1;
        }
        v
    };
    let tot_edges = if top_is_point && bottom_is_point {
        ss
    } else {
        let mut e = 0;
        if top_has_center {
            e += cs;
        }
        e += cs * (quad_rings * 2 + 1);
        if bottom_has_center {
            e += cs;
        }
        e
    };
    let first_vert = 0usize;
    let first_ring_verts_start = if top_has_center { 1 } else { first_vert };
    let last_vert = tot_verts - 1;
    let last_ring_verts_start = last_vert.wrapping_sub(cs);
    let first_ring_edges_start = if top_has_center { cs } else { 0 };
    let last_ring_edges_start = first_ring_edges_start + quad_rings * cs * 2;
    let last_fan_edges_start = tot_edges.wrapping_sub(cs);
    let last_edge = tot_edges.wrapping_sub(1);
    let top_faces_len = if !top_is_point {
        (fill_segments - 1) * cs + if top_has_center { cs } else { 0 } + if fill == 1 { 1 } else { 0 }
    } else {
        0
    };
    let side_faces_start = top_faces_len;
    let side_faces_len = if top_is_point && bottom_is_point { 0 } else { ss * cs };
    let bottom_faces_len = if !bottom_is_point {
        (fill_segments - 1) * cs + if bottom_has_center { cs } else { 0 } + if fill == 1 { 1 } else { 0 }
    } else {
        0
    };
    let bottom_faces_start = side_faces_start + side_faces_len;
    let tot_faces = top_faces_len + side_faces_len + bottom_faces_len;

    let empty_sel = |len: usize| vec![false; len];
    // point / vertex special cases
    if top_is_point && bottom_is_point {
        let mut m = Mesh::new();
        if height == 0.0 {
            m.pos.push([0.0, 0.0, 0.0]);
        } else {
            let z_delta = -2.0f32 * height / ss as f32;
            for i in 0..tot_verts {
                m.pos.push([0.0 + 0.0 * i as f32, 0.0 + 0.0 * i as f32, height + z_delta * i as f32]);
            }
            for i in 0..tot_edges {
                m.edges.push([i as u32, i as u32 + 1]);
            }
        }
        if cone {
            for p in m.pos.iter_mut() {
                p[2] += depth * 0.5;
            }
        }
        let nv = m.pos.len();
        return ConeOut { mesh: m, top: (Dom::Point, { let mut v = empty_sel(nv); if nv > 0 { v[0] = true; } v }), bottom: (Dom::Point, { let mut v = empty_sel(nv); if nv > 0 { v[nv - 1] = true; } v }), side: vec![], uv: vec![] };
    }
    // vertices
    let mut circle = vec![[0.0f32; 2]; cs];
    let angle_delta = (2.0f64 * (std::f64::consts::PI / cs as f64)) as f32;
    let mut angle = 0.0f32;
    for c in circle.iter_mut() {
        *c = [angle.cos(), angle.sin()];
        angle += angle_delta;
    }
    let mut pos: Vec<V3> = Vec::with_capacity(tot_verts);
    if top_has_center {
        pos.push([0.0, 0.0, height]);
    }
    if !top_is_point {
        let delta = radius_top / fill_segments as f32;
        for i in 0..fill_segments {
            let r = delta * (i + 1) as f32;
            for j in 0..cs {
                pos.push([circle[j][0] * r, circle[j][1] * r, height]);
            }
        }
    }
    let side_radius_delta = (radius_bottom - radius_top) / ss as f32;
    let height_delta = 2.0f32 * height / ss as f32;
    for i in 0..ss - 1 {
        let ring_radius = radius_top + (side_radius_delta * (i + 1) as f32);
        let ring_height = height - (height_delta * (i + 1) as f32);
        for j in 0..cs {
            pos.push([circle[j][0] * ring_radius, circle[j][1] * ring_radius, ring_height]);
        }
    }
    if !bottom_is_point {
        let delta = radius_bottom / fill_segments as f32;
        for i in 0..fill_segments {
            let r = radius_bottom - (i as f32 * delta);
            for j in 0..cs {
                pos.push([circle[j][0] * r, circle[j][1] * r, -height]);
            }
        }
    }
    if bottom_has_center {
        pos.push([0.0, 0.0, -height]);
    }
    // edges
    let mut edges: Vec<[u32; 2]> = Vec::with_capacity(tot_edges);
    if top_has_center {
        for i in 0..cs {
            edges.push([first_vert as u32, (first_ring_verts_start + i) as u32]);
        }
    }
    for i in 0..edge_rings {
        let this_ring = first_ring_verts_start + i * cs;
        let next_ring = this_ring + cs;
        for j in 0..cs {
            edges.push([(this_ring + j) as u32, (this_ring + (j + 1) % cs) as u32]);
        }
        if i == edge_rings - 1 {
            break;
        }
        for j in 0..cs {
            edges.push([(this_ring + j) as u32, (next_ring + j) as u32]);
        }
    }
    if bottom_has_center {
        for i in 0..cs {
            edges.push([(last_ring_verts_start + i) as u32, last_vert as u32]);
        }
    }
    // faces
    let mut face_sizes: Vec<usize> = vec![0; tot_faces];
    let mut cv: Vec<u32> = vec![];
    let mut ce: Vec<u32> = vec![];
    let tot_corners = {
        let mut c = 0;
        if top_has_center {
            c += cs * 3;
        } else if !top_is_point && fill == 1 {
            c += cs;
        }
        c += quad_rings * (cs * 4);
        if bottom_has_center {
            c += cs * 3;
        } else if !bottom_is_point && fill == 1 {
            c += cs;
        }
        c
    };
    cv.resize(tot_corners, 0);
    ce.resize(tot_corners, 0);
    let mut rings_face_start = 0;
    let mut rings_loop_start = 0;
    if top_has_center {
        rings_face_start = cs;
        rings_loop_start = cs * 3;
        for f in 0..cs {
            face_sizes[f] = 3;
        }
        for i in 0..cs {
            let l = i * 3;
            cv[l] = (first_ring_verts_start + i) as u32;
            ce[l] = (first_ring_edges_start + i) as u32;
            cv[l + 1] = (first_ring_verts_start + (i + 1) % cs) as u32;
            ce[l + 1] = ((i + 1) % cs) as u32;
            cv[l + 2] = 0;
            ce[l + 2] = i as u32;
        }
    } else if fill == 1 {
        rings_face_start = 1;
        rings_loop_start = cs;
        face_sizes[0] = cs;
        for i in 0..cs {
            cv[i] = i as u32;
            ce[i] = i as u32;
        }
    }
    let ring_faces_num = quad_rings * cs;
    for f in 0..ring_faces_num {
        face_sizes[rings_face_start + f] = 4;
    }
    for i in 0..quad_rings {
        let this_ring_loop_start = rings_loop_start + i * cs * 4;
        let this_ring_vert_start = first_ring_verts_start + i * cs;
        let next_ring_vert_start = this_ring_vert_start + cs;
        let this_ring_edges_start = first_ring_edges_start + i * 2 * cs;
        let next_ring_edges_start = this_ring_edges_start + 2 * cs;
        let ring_connections_start = this_ring_edges_start + cs;
        for j in 0..cs {
            let l = this_ring_loop_start + j * 4;
            cv[l] = (this_ring_vert_start + j) as u32;
            ce[l] = (ring_connections_start + j) as u32;
            cv[l + 1] = (next_ring_vert_start + j) as u32;
            ce[l + 1] = (next_ring_edges_start + j) as u32;
            cv[l + 2] = (next_ring_vert_start + (j + 1) % cs) as u32;
            ce[l + 2] = (ring_connections_start + (j + 1) % cs) as u32;
            cv[l + 3] = (this_ring_vert_start + (j + 1) % cs) as u32;
            ce[l + 3] = (this_ring_edges_start + j) as u32;
        }
    }
    let bottom_face_start = rings_face_start + ring_faces_num;
    let bottom_loop_start = rings_loop_start + ring_faces_num * 4;
    if bottom_has_center {
        for f in 0..cs {
            face_sizes[bottom_face_start + f] = 3;
        }
        for i in 0..cs {
            let l = bottom_loop_start + i * 3;
            cv[l] = (last_ring_verts_start + i) as u32;
            ce[l] = (last_fan_edges_start + i) as u32;
            cv[l + 1] = last_vert as u32;
            ce[l + 1] = (last_fan_edges_start + (i + 1) % cs) as u32;
            cv[l + 2] = (last_ring_verts_start + (i + 1) % cs) as u32;
            ce[l + 2] = (last_ring_edges_start + i) as u32;
        }
    } else if fill == 1 {
        face_sizes[bottom_face_start] = cs;
        for i in 0..cs {
            cv[bottom_loop_start + i] = (last_vert - i) as u32;
            ce[bottom_loop_start + i] = (last_edge - ((i + 1) % cs)) as u32;
        }
    }
    let mut m = Mesh::new();
    m.pos = pos;
    m.edges = edges;
    m.foff = vec![0];
    let mut acc = 0u32;
    for s in &face_sizes {
        acc += *s as u32;
        m.foff.push(acc);
    }
    m.cv = cv;
    m.ce = ce;
    // selections
    let top_face = !top_is_point && fill != 0;
    let mut top = vec![false; if top_face { tot_faces } else { tot_verts }];
    if top_is_point {
        top[first_vert] = true;
    } else {
        let len = if top_face { top_faces_len } else { cs };
        for v in top.iter_mut().take(len) {
            *v = true;
        }
    }
    let bottom_face = !bottom_is_point && fill != 0;
    let mut bottom = vec![false; if bottom_face { tot_faces } else { tot_verts }];
    if bottom_is_point {
        bottom[last_vert] = true;
    } else if bottom_face {
        for f in 0..bottom_faces_len {
            bottom[bottom_faces_start + f] = true;
        }
    } else {
        for v in 0..cs {
            bottom[last_ring_verts_start + 1 + v] = true;
        }
    }
    let mut side = vec![false; tot_faces];
    for f in 0..side_faces_len {
        side[side_faces_start + f] = true;
    }
    // UV map
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(tot_corners);
    {
        let mut circ = vec![[0.0f32; 2]; cs];
        let mut ang = 0.0f32;
        let ad = (2.0f64 * std::f64::consts::PI / cs as f64) as f32;
        for c in circ.iter_mut() {
            *c = [ang.cos() * 0.225, ang.sin() * 0.225];
            ang += ad;
        }
        let sc = |f: f32, c: [f32; 2], o: [f32; 2]| [f * c[0] + o[0], f * c[1] + o[1]];
        if top_is_point || fill != 0 {
            let center_left = [0.25f32, 0.25f32];
            let radius_factor_delta = 1.0f32 / if top_is_point { ss as f32 } else { fill_segments as f32 };
            let left_count = if top_is_point { ss } else { fill_segments };
            if top_has_center {
                for i in 0..cs {
                    uvs.push(sc(radius_factor_delta, circ[i], center_left));
                    uvs.push(sc(radius_factor_delta, circ[(i + 1) % cs], center_left));
                    uvs.push(center_left);
                }
            } else if !top_is_point && fill == 1 {
                for i in 0..cs {
                    uvs.push(sc(radius_factor_delta, circ[i], center_left));
                }
            }
            for i in 1..left_count {
                let inner = i as f32 * radius_factor_delta;
                let outer = (i + 1) as f32 * radius_factor_delta;
                for j in 0..cs {
                    uvs.push(sc(inner, circ[j], center_left));
                    uvs.push(sc(outer, circ[j], center_left));
                    uvs.push(sc(outer, circ[(j + 1) % cs], center_left));
                    uvs.push(sc(inner, circ[(j + 1) % cs], center_left));
                }
            }
        }
        if !top_is_point && !bottom_is_point {
            let bottom_v = if fill == 0 { 0.0f32 } else { 0.5f32 };
            let x_delta = 1.0f32 / cs as f32;
            let y_delta = (1.0f32 - bottom_v) / ss as f32;
            for i in 0..ss {
                for j in 0..cs {
                    uvs.push([j as f32 * x_delta, i as f32 * y_delta + bottom_v]);
                    uvs.push([j as f32 * x_delta, (i + 1) as f32 * y_delta + bottom_v]);
                    uvs.push([(j + 1) as f32 * x_delta, (i + 1) as f32 * y_delta + bottom_v]);
                    uvs.push([(j + 1) as f32 * x_delta, i as f32 * y_delta + bottom_v]);
                }
            }
        }
        if bottom_is_point || fill != 0 {
            let center_right = [0.75f32, 0.25f32];
            let radius_factor_delta = 1.0f32 / if bottom_is_point { ss as f32 } else { fill_segments as f32 };
            let right_count = if bottom_is_point { ss } else { fill_segments };
            for i in 0..right_count - 1 {
                let outer = 1.0f32 - i as f32 * radius_factor_delta;
                let inner = 1.0f32 - (i + 1) as f32 * radius_factor_delta;
                for j in 0..cs {
                    uvs.push(sc(outer, circ[j], center_right));
                    uvs.push(sc(inner, circ[j], center_right));
                    uvs.push(sc(inner, circ[(j + 1) % cs], center_right));
                    uvs.push(sc(outer, circ[(j + 1) % cs], center_right));
                }
            }
            if bottom_has_center {
                for i in 0..cs {
                    uvs.push(sc(radius_factor_delta, circ[i], center_right));
                    uvs.push(center_right);
                    uvs.push(sc(radius_factor_delta, circ[(i + 1) % cs], center_right));
                }
            } else if !bottom_is_point && fill == 1 {
                for i in 0..cs {
                    uvs.push(sc(radius_factor_delta, circ[cs - 1 - i], center_right));
                }
            }
        }
    }
    if cone {
        for p in m.pos.iter_mut() {
            p[2] += depth * 0.5;
        }
    }
    set_sharp_all(&mut m);
    let tdom = if top_face { Dom::Face } else { Dom::Point };
    let bdom = if bottom_face { Dom::Face } else { Dom::Point };
    ConeOut { mesh: m, top: (tdom, top), bottom: (bdom, bottom), side, uv: uvs }
}

/// all faces flat shaded (sharp_face = true), as the 4.2 primitives do
pub fn set_sharp_all(m: &mut Mesh) {
    if m.nf() > 0 {
        m.attrs.insert("sharp_face".into(), Attr { dom: Dom::Face, ty: Ty::B, data: vec![Val::B(true); m.nf()] });
    }
}

pub fn mesh_uv_sphere(segs: usize, rings: usize, radius: f32) -> Mesh {
    mesh_uv_sphere_uv(segs, rings, radius).0
}

/// exact port of create_uv_sphere_mesh; also returns the UV map (corner domain)
pub fn mesh_uv_sphere_uv(segs: usize, rings: usize, radius: f32) -> (Mesh, Vec<[f32; 2]>) {
    let segments = segs.max(3);
    let rings = rings.max(2);
    let delta_theta = (std::f64::consts::PI / rings as f64) as f32;
    let delta_phi = ((2.0f32 as f64 * std::f64::consts::PI) / segments as f64) as f32;
    let mut cosines = vec![0.0f32; segments + 1];
    let mut sines = vec![0.0f32; segments + 1];
    for sg in 1..=segments {
        let phi = sg as f32 * delta_phi;
        cosines[sg] = phi.cos();
        sines[sg] = phi.sin();
    }
    let mut pos: Vec<V3> = vec![[0.0, 0.0, radius]];
    for ring in 1..rings {
        let theta = ring as f32 * delta_theta;
        let sin_theta = theta.sin();
        let z = theta.cos();
        for sg in 1..=segments {
            let x = sin_theta * cosines[sg];
            let y = sin_theta * sines[sg];
            pos.push([x * radius, y * radius, z * radius]);
        }
    }
    pos.push([0.0, 0.0, -radius]);
    let nv = pos.len();
    let mut m = Mesh::new();
    // edges
    for sg in 0..segments {
        m.edges.push([0, (1 + sg) as u32]);
    }
    let mut ring_start = 1usize;
    for ring in 0..rings - 1 {
        let next = ring_start + segments;
        for sg in 0..segments {
            m.edges.push([(ring_start + sg) as u32, (ring_start + (sg + 1) % segments) as u32]);
        }
        if ring + 2 < rings {
            for sg in 0..segments {
                m.edges.push([(ring_start + sg) as u32, (next + sg) as u32]);
            }
        }
        ring_start += segments;
    }
    let last = nv - 1;
    let last_ring = last - segments;
    for sg in 0..segments {
        m.edges.push([last as u32, (last_ring + sg) as u32]);
    }
    // faces
    m.foff = vec![0];
    let mut acc = 0u32;
    let mut push_face = |m: &mut Mesh, n: u32| {
        acc += n;
        m.foff.push(acc);
    };
    for _ in 0..segments {
        push_face(&mut m, 3);
    }
    for _ in 0..segments * (rings - 2) {
        push_face(&mut m, 4);
    }
    for _ in 0..segments {
        push_face(&mut m, 3);
    }
    let nxt = |s: usize| if s == segments - 1 { 0 } else { s + 1 };
    for sg in 0..segments {
        let sn = nxt(sg);
        m.cv.extend([0u32, (1 + sg) as u32, (1 + sn) as u32]);
        m.ce.extend([sg as u32, (segments + sg) as u32, sn as u32]);
    }
    let (rings_vert_start, rings_edge_start) = (1usize, segments);
    for ring in 1..=rings - 2 {
        let rvs = rings_vert_start + (ring - 1) * segments;
        let res = rings_edge_start + (ring - 1) * segments * 2;
        let nvs = rvs + segments;
        let nes = res + segments * 2;
        let rves = res + segments;
        for sg in 0..segments {
            let sn = nxt(sg);
            m.cv.extend([(rvs + sg) as u32, (nvs + sg) as u32, (nvs + sn) as u32, (rvs + sn) as u32]);
            m.ce.extend([(rves + sg) as u32, (nes + sg) as u32, (rves + sn) as u32, (res + sg) as u32]);
        }
    }
    let last_edge_ring_start = segments * (rings - 2) * 2 + segments;
    let bottom_fan = last_edge_ring_start + segments;
    for sg in 0..segments {
        let sn = nxt(sg);
        m.cv.extend([last as u32, (last_ring + sn) as u32, (last_ring + sg) as u32]);
        m.ce.extend([(bottom_fan + sn) as u32, (last_edge_ring_start + sg) as u32, (bottom_fan + sg) as u32]);
    }
    // UVs
    let rf = rings as f32;
    let sf = segments as f32;
    let dy = 1.0f32 / rf;
    let si = 1.0f32 / sf;
    let mut uv: Vec<[f32; 2]> = vec![];
    for i in 0..segments {
        let sgf = i as f32;
        uv.push([(sgf + 0.5) * si, 0.0]);
        uv.push([sgf * si, dy]);
        uv.push([(sgf + 1.0) * si, dy]);
    }
    for ir in 1..=rings - 2 {
        let ring = ir as f32;
        for i in 0..segments {
            let sgf = i as f32;
            uv.push([sgf * si, ring / rf]);
            uv.push([sgf * si, (ring + 1.0) / rf]);
            uv.push([(sgf + 1.0) * si, (ring + 1.0) / rf]);
            uv.push([(sgf + 1.0) * si, ring / rf]);
        }
    }
    for i in 0..segments {
        let sgf = i as f32;
        uv.push([(sgf + 0.5) * si, 1.0]);
        uv.push([(sgf + 1.0) * si, 1.0 - dy]);
        uv.push([sgf * si, 1.0 - dy]);
    }
    m.pos = pos;
    set_sharp_all(&mut m);
    // vertex normals: unit directions (x, y, z) before the radius multiplication
    let mut vn: Vec<V3> = vec![[0.0, 0.0, 1.0]];
    for ring in 1..rings {
        let theta = ring as f32 * delta_theta;
        let sin_theta = theta.sin();
        let z = theta.cos();
        for sg in 1..=segments {
            vn.push([sin_theta * cosines[sg], sin_theta * sines[sg], z]);
        }
    }
    vn.push([0.0, 0.0, -1.0]);
    m.vnor = Some(vn);
    (m, uv)
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

/// UV map of the Cube node (corner domain), port of calculate_uvs in mesh_primitive_cuboid.cc
pub fn cube_uv(n: [i32; 3]) -> Vec<[f32; 2]> {
    let ex = (n[0].max(2) - 1) as usize;
    let ey = (n[1].max(2) - 1) as usize;
    let ez = (n[2].max(2) - 1) as usize;
    let xd = 0.25f32 / ex as f32;
    let yd = 0.25f32 / ey as f32;
    let zd = 0.25f32 / ez as f32;
    let mut uv = vec![];
    let f = |v: usize| v as f32;
    for y in 0..ey {
        for x in 0..ex {
            uv.push([0.25 + f(x) * xd, 0.375 - f(y) * yd]);
            uv.push([0.25 + f(x) * xd, 0.375 - f(y + 1) * yd]);
            uv.push([0.25 + f(x + 1) * xd, 0.375 - f(y + 1) * yd]);
            uv.push([0.25 + f(x + 1) * xd, 0.375 - f(y) * yd]);
        }
    }
    for z in 0..ez {
        for x in 0..ex {
            uv.push([0.25 + f(x) * xd, 0.375 + f(z) * zd]);
            uv.push([0.25 + f(x + 1) * xd, 0.375 + f(z) * zd]);
            uv.push([0.25 + f(x + 1) * xd, 0.375 + f(z + 1) * zd]);
            uv.push([0.25 + f(x) * xd, 0.375 + f(z + 1) * zd]);
        }
    }
    for y in 0..ey {
        for x in 0..ex {
            uv.push([0.25 + f(x) * xd, 0.625 + f(y) * yd]);
            uv.push([0.25 + f(x + 1) * xd, 0.625 + f(y) * yd]);
            uv.push([0.25 + f(x + 1) * xd, 0.625 + f(y + 1) * yd]);
            uv.push([0.25 + f(x) * xd, 0.625 + f(y + 1) * yd]);
        }
    }
    for z in 0..ez {
        for x in 0..ex {
            uv.push([1.0 - f(x) * xd, 0.375 + f(z) * zd]);
            uv.push([1.0 - f(x) * xd, 0.375 + f(z + 1) * zd]);
            uv.push([1.0 - f(x + 1) * xd, 0.375 + f(z + 1) * zd]);
            uv.push([1.0 - f(x + 1) * xd, 0.375 + f(z) * zd]);
        }
    }
    for z in 0..ez {
        for y in 0..ey {
            uv.push([0.25 - f(y) * yd, 0.375 + f(z) * zd]);
            uv.push([0.25 - f(y) * yd, 0.375 + f(z + 1) * zd]);
            uv.push([0.25 - f(y + 1) * yd, 0.375 + f(z + 1) * zd]);
            uv.push([0.25 - f(y + 1) * yd, 0.375 + f(z) * zd]);
        }
    }
    for z in 0..ez {
        for y in 0..ey {
            uv.push([0.50 + f(y) * yd, 0.375 + f(z) * zd]);
            uv.push([0.50 + f(y + 1) * yd, 0.375 + f(z) * zd]);
            uv.push([0.50 + f(y + 1) * yd, 0.375 + f(z + 1) * zd]);
            uv.push([0.50 + f(y) * yd, 0.375 + f(z + 1) * zd]);
        }
    }
    uv
}

/// UV map of the Grid node
pub fn grid_uv(m: &Mesh, sx: f32, sy: f32) -> Vec<[f32; 2]> {
    let dx = if sx == 0.0 { 0.0 } else { 1.0 / sx };
    let dy = if sy == 0.0 { 0.0 } else { 1.0 / sy };
    m.cv.iter().map(|&v| { let co = m.pos[v as usize]; [(co[0] + sx * 0.5) * dx, (co[1] + sy * 0.5) * dy] }).collect()
}
