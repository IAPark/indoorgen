//! Room shell meshes: interior walls (with door/window openings + reveals), floor, ceiling.
use super::consts::RoomConstants;
use super::solidify::Cutter;
use super::state::*;
use crate::mesh::*;
use crate::shp::*;

pub struct RoomMeshes {
    pub name: String,
    pub wall_quads: Vec<[V3; 4]>,
    pub wall: Mesh,
    pub floor: Mesh,
    pub ceiling: Mesh,
    /// python-style closed solidified shell (outer + visible cavity, cutters subtracted), tagged
    pub solid: Vec<crate::furn::geom::Tri>,
}

fn triangulate(poly: &G, z: f64, up: bool) -> Mesh {
    let mut m = Mesh::new();
    for part in parts(poly) {
        if !is_polygon(&part) || area(&part) <= 0.0 {
            continue;
        }
        let ext = exterior(&part);
        let mut flat: Vec<f64> = vec![];
        let mut holes = vec![];
        for p in &ext[..ext.len() - 1] {
            flat.extend([p[0], p[1]]);
        }
        for h in interiors(&part) {
            holes.push(flat.len() / 2);
            for p in &h[..h.len() - 1] {
                flat.extend([p[0], p[1]]);
            }
        }
        let tris = earcutr::earcut(&flat, &holes, 2).unwrap_or_default();
        let base = m.pos.len() as u32;
        for i in 0..flat.len() / 2 {
            m.pos.push([flat[2 * i], flat[2 * i + 1], z]);
        }
        for t in tris.chunks(3) {
            let (a, b, c) = (t[0] as u32 + base, t[1] as u32 + base, t[2] as u32 + base);
            // ensure normal direction
            let (pa, pb, pc) = (m.pos[a as usize], m.pos[b as usize], m.pos[c as usize]);
            let nz = cross(sub(pb, pa), sub(pc, pa))[2];
            if (nz > 0.0) == up {
                m.add_face_idx(&[a, b, c]);
            } else {
                m.add_face_idx(&[a, c, b]);
            }
        }
    }
    m
}

struct Hole {
    s0: f64,
    s1: f64,
    z0: f64,
    z1: f64,
}

fn wall_panel(m: &mut Mesh, p0: P2, p1: P2, z0: f64, z1: f64, holes: &[Hole], quads: &mut Vec<[V3; 4]>) {
    let l = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
    if l < 1e-6 {
        return;
    }
    let mut bs = vec![0.0, l];
    for h in holes {
        bs.push(h.s0.clamp(0.0, l));
        bs.push(h.s1.clamp(0.0, l));
    }
    bs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    bs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let at = |s: f64, z: f64| -> V3 {
        let t = s / l;
        [p0[0] + (p1[0] - p0[0]) * t, p0[1] + (p1[1] - p0[1]) * t, z]
    };
    for w in bs.windows(2) {
        let (sa, sb) = (w[0], w[1]);
        let mid = (sa + sb) / 2.0;
        let mut cuts: Vec<(f64, f64)> =
            holes.iter().filter(|h| h.s0 <= mid && mid <= h.s1).map(|h| (h.z0.max(z0), h.z1.min(z1))).collect();
        cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut zc = z0;
        let mut segs = vec![];
        for (a, b) in cuts {
            if a > zc {
                segs.push((zc, a));
            }
            zc = zc.max(b);
        }
        if zc < z1 {
            segs.push((zc, z1));
        }
        for (za, zb) in segs {
            // inward-facing quad
            let q = [at(sb, za), at(sa, za), at(sa, zb), at(sb, zb)];
            quads.push(q);
            m.add_quad(q[0], q[1], q[2], q[3]);
        }
    }
}

fn reveal(m: &mut Mesh, c: &Cutter, ya: f64, yb: f64) {
    let (hx, z0, z1) = (c.dims[0] / 2.0, c.loc[2] - c.dims[2] / 2.0, c.loc[2] + c.dims[2] / 2.0);
    let t = Mat4::translation([c.loc[0], c.loc[1], 0.0]).mul(&Mat4::rot_z(c.rot_z));
    let mut r = Mesh::new();
    r.add_quad([-hx, ya, z0], [-hx, yb, z0], [-hx, yb, z1], [-hx, ya, z1]);
    r.add_quad([hx, yb, z0], [hx, ya, z0], [hx, ya, z1], [hx, yb, z1]);
    r.add_quad([-hx, ya, z1], [-hx, yb, z1], [hx, yb, z1], [hx, ya, z1]);
    r.add_quad([-hx, ya, z0], [hx, ya, z0], [hx, yb, z0], [-hx, yb, z0]);
    r.transform(&t);
    m.append(&r);
}

pub fn build_shells(c: &RoomConstants, state: &RoomState, cutters: &[Cutter]) -> Vec<RoomMeshes> {
    let wt = c.wall_thickness;
    let (z0, z1) = (wt / 2.0, c.wall_height - wt / 2.0);
    let mut out = vec![];
    for (name, obj) in state.valid_rooms() {
        let poly = obj.polygon.as_ref().unwrap();
        let cavity = buffer_mitre(poly, -wt / 2.0);
        let mut wall = Mesh::new();
        let mut wall_quads: Vec<[V3; 4]> = vec![];
        for part in parts(&cavity) {
            if !is_polygon(&part) || area(&part) <= 0.0 {
                continue;
            }
            let part = orient(&part);
            let mut rings = vec![exterior(&part)];
            rings.extend(interiors(&part));
            for ring in rings {
                for w in ring.windows(2) {
                    let (p0, p1) = (w[0], w[1]);
                    let l = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
                    if l < 1e-6 {
                        continue;
                    }
                    let d = [(p1[0] - p0[0]) / l, (p1[1] - p0[1]) / l];
                    let n = [-d[1], d[0]];
                    let mut holes = vec![];
                    for ct in cutters.iter().filter(|ct| ct.rooms.contains(name)) {
                        let ax = [ct.rot_z.cos(), ct.rot_z.sin()];
                        if (d[0] * ax[1] - d[1] * ax[0]).abs() > 0.02 {
                            continue;
                        }
                        let rel = [ct.loc[0] - p0[0], ct.loc[1] - p0[1]];
                        let off = rel[0] * n[0] + rel[1] * n[1];
                        // cutter centre sits on the room boundary: wt/2 outside the cavity edge
                        if (off + wt / 2.0).abs() > 0.03 {
                            continue;
                        }
                        let sc = rel[0] * d[0] + rel[1] * d[1];
                        let h = ct.dims[0] / 2.0;
                        holes.push(Hole { s0: sc - h, s1: sc + h, z0: ct.loc[2] - ct.dims[2] / 2.0, z1: ct.loc[2] + ct.dims[2] / 2.0 });
                    }
                    wall_panel(&mut wall, p0, p1, z0, z1, &holes, &mut wall_quads);
                }
            }
        }
        for ct in cutters.iter().filter(|ct| ct.rooms.first() == Some(name)) {
            if ct.rooms.len() == 2 {
                reveal(&mut wall, ct, -wt / 2.0, wt / 2.0);
            } else {
                let ay = [-ct.rot_z.sin(), ct.rot_z.cos()];
                let probe = point(ct.loc[0] + ay[0] * 0.2, ct.loc[1] + ay[1] * 0.2);
                let s = if contains(poly, &probe) { 1.0 } else { -1.0 };
                let (a, b) = if s > 0.0 { (0.0, wt / 2.0) } else { (-wt / 2.0, 0.0) };
                reveal(&mut wall, ct, a, b);
            }
        }
        out.push(RoomMeshes {
            name: name.clone(),
            wall_quads,
            wall,
            floor: triangulate(&cavity, z0, true),
            ceiling: triangulate(&cavity, z1, false),
            solid: solid_room(c, name, poly, cutters, &ext_centers(state, name)),
        });
    }
    out
}

use crate::furn::geom::{ObjGeom, Proxy, Tri, CEILING, SUPPORT, VISIBLE, WALL};

/// Convert room meshes into a tagged solver geometry with convex collision proxies (floor/ceiling slabs, wall slabs).
pub fn room_geom(c: &RoomConstants, m: &RoomMeshes) -> ObjGeom {
    let wt = c.wall_thickness;
    let (z0, z1) = (wt / 2.0, c.wall_height - wt / 2.0);
    let mut tris: Vec<Tri> = vec![];
    let _ = &m.solid;
    let mut push = |mesh: &Mesh, default: u32| {
        for t in &mesh.triangles() {
            let v = [mesh.pos[t[0] as usize], mesh.pos[t[1] as usize], mesh.pos[t[2] as usize]];
            let n = cross(sub(v[1], v[0]), sub(v[2], v[0]));
            let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let nz = n[2] / nl;
            let zs = [v[0][2], v[1][2], v[2][2]];
            let tag = if nz > 0.99 && zs.iter().all(|z| (z - z0).abs() < 1e-6) {
                SUPPORT | VISIBLE
            } else if nz < -0.99 && zs.iter().all(|z| (z - z1).abs() < 1e-6) {
                CEILING | VISIBLE
            } else {
                default | VISIBLE
            };
            tris.push(Tri { v, tags: tag });
        }
    };
    push(&m.floor, SUPPORT);
    push(&m.ceiling, CEILING);
    push(&m.wall, WALL);
    let mut proxies: Vec<(Proxy, u32)> = vec![];
    // floor / ceiling slabs: one hull per triangle of the cavity triangulation
    for (mesh, zs, class) in [(&m.floor, (0.0, z0), SUPPORT | VISIBLE), (&m.ceiling, (z1, c.wall_height), CEILING | VISIBLE)] {
        for t in &mesh.triangles() {
            let mut pts = vec![];
            for &i in t {
                let p = mesh.pos[i as usize];
                pts.push([p[0], p[1], zs.0]);
                pts.push([p[0], p[1], zs.1]);
            }
            if crate::furn::geom::hull_ok(&pts) {
                proxies.push((Proxy::Hull(pts), class));
            }
        }
    }
    for q in &m.wall_quads {
        let n = {
            let v = cross(sub(q[1], q[0]), sub(q[2], q[0]));
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let mut pts = vec![];
        for p in q {
            pts.push(*p);
            pts.push([p[0] - n[0] * wt / 2.0, p[1] - n[1] * wt / 2.0, p[2] - n[2] * wt / 2.0]);
        }
        if crate::furn::geom::hull_ok(&pts) {
            proxies.push((Proxy::Hull(pts), WALL | VISIBLE));
        }
    }
    let tris = if m.solid.is_empty() || std::env::var_os("OLDROOM").is_some() { tris } else { m.solid.clone() };
    let mut g = ObjGeom::from_tris(tris, proxies);
    // the reference room mesh is the full hollow shell: its bounding box spans the polygon bbox and the wall height
    let _ = &mut g;
    g
}

pub fn ext_centers(state: &RoomState, name: &str) -> Vec<P2> {
    let mut out = vec![];
    for (k, o) in state.objs.iter() {
        if room_type(k) != EXTERIOR {
            continue;
        }
        if let Some(r) = o.relations.iter().find(|r| r.target == name) {
            for ls in parts(&r.value) {
                let cs = coords_of(&ls);
                for w in cs.windows(2) {
                    out.push([(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0]);
                }
            }
        }
    }
    out
}

/// holes of the cutters that cross the wall edge p0->p1 (CCW cavity side on the left); `target` is the signed offset of the
/// cutter centre from the edge line.
fn edge_holes(cutters: &[Cutter], name: &str, p0: P2, p1: P2, target: f64) -> Vec<Hole> {
    let l = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
    let d = [(p1[0] - p0[0]) / l, (p1[1] - p0[1]) / l];
    let n = [-d[1], d[0]];
    let mut holes = vec![];
    for ct in cutters.iter().filter(|ct| ct.rooms.iter().any(|r| r == name)) {
        let ax = [ct.rot_z.cos(), ct.rot_z.sin()];
        if (d[0] * ax[1] - d[1] * ax[0]).abs() > 0.02 {
            continue;
        }
        let rel = [ct.loc[0] - p0[0], ct.loc[1] - p0[1]];
        let off = rel[0] * n[0] + rel[1] * n[1];
        if (off - target).abs() > 0.03 {
            continue;
        }
        let sc = rel[0] * d[0] + rel[1] * d[1];
        let h = ct.dims[0] / 2.0;
        holes.push(Hole { s0: sc - h, s1: sc + h, z0: ct.loc[2] - ct.dims[2] / 2.0, z1: ct.loc[2] + ct.dims[2] / 2.0 });
    }
    holes
}

/// Port of RoomSolidifier.make_room: the solver's room object is the *uncut* closed shell = outer prism (polygon, z 0..H,
/// not visible) + inner cavity (visible; the polygon inset by wall_thickness/2 with even/mitre offset).  Face tags come
/// from the prism face centres (floor / ceiling / wall) and from the proximity of the face centre to the exterior edges
/// (interior flag); the inner faces copy them.  (Cutters are subtracted only from a mesh copy, not from the solver object.)
pub fn solid_room(c: &RoomConstants, name: &str, poly: &G, cutters: &[Cutter], ext: &[P2]) -> Vec<crate::furn::geom::Tri> {
    if std::env::var_os("OLDROOM_SOLID").is_none() {
        if let Some(t) = solid_room_exact(c, poly, ext) {
            return t;
        }
    }
    solid_room_approx(c, name, poly, cutters, ext)
}

/// rings (exterior first, closing point dropped) of `orient(segmentize(canonicalize(poly), door_width))`
pub fn room_rings(c: &RoomConstants, poly: &G) -> Option<Vec<Vec<P2>>> {
    let pc = c.canonicalize(poly).ok()?;
    let outer_poly = segmentize(&pc, c.door_width);
    let mut rings: Vec<Vec<P2>> = vec![];
    for part in parts(&outer_poly) {
        if !is_polygon(&part) || area(&part) <= 0.0 {
            continue;
        }
        if !rings.is_empty() {
            return None; // python expects a single polygon
        }
        let part = orient(&part);
        let mut rs = vec![exterior(&part)];
        rs.extend(interiors(&part));
        rings = rs.into_iter().map(|mut r| { r.pop(); r }).collect();
    }
    if rings.is_empty() {
        None
    } else {
        Some(rings)
    }
}

/// python-exact room mesh (vertex / triangle order, float32 positions, per-face tags), see `pyroom::make_room`
pub fn solid_room_exact(c: &RoomConstants, poly: &G, ext: &[P2]) -> Option<Vec<crate::furn::geom::Tri>> {
    use crate::furn::geom::{Tri, CEILING, INTERIOR, SUPPORT, VISIBLE, WALL};
    use super::pyroom::*;
    let rings = room_rings(c, poly)?;
    let out = std::panic::catch_unwind(|| make_room(&rings, c.wall_thickness, c.wall_height, ext)).ok()?;
    let p3 = |i: u32| -> V3 { let p = out.pos[i as usize]; [p[0] as f64, p[1] as f64, p[2] as f64] };
    Some(
        out.tris
            .iter()
            .zip(&out.tags)
            .map(|(t, &g)| {
                let mut tags = 0u32;
                for (bit, flag) in [(T_INTERIOR, INTERIOR), (T_CEILING, CEILING), (T_SUPPORT, SUPPORT), (T_WALL, WALL), (T_VISIBLE, VISIBLE)] {
                    if g & bit != 0 {
                        tags |= flag;
                    }
                }
                Tri { v: [p3(t[0]), p3(t[1]), p3(t[2])], tags }
            })
            .collect(),
    )
}

pub fn solid_room_approx(c: &RoomConstants, _name: &str, poly: &G, _cutters: &[Cutter], ext: &[P2]) -> Vec<crate::furn::geom::Tri> {
    use crate::furn::geom::{Tri, CEILING, INTERIOR, SUPPORT, VISIBLE, WALL};
    let wt = c.wall_thickness;
    let h = c.wall_height;
    let (low, high) = (wt / 2.0, h - wt / 2.0);
    let is_int = |x: f64, y: f64| -> bool { !ext.iter().any(|e| (x - e[0]).abs() + (y - e[1]).abs() < wt * 4.0) };
    let pc = c.canonicalize(poly).unwrap_or_else(|_| poly.clone());
    let outer_poly = segmentize(&pc, c.door_width);
    let mut tris: Vec<Tri> = vec![];
    for part in parts(&outer_poly) {
        if !is_polygon(&part) || area(&part) <= 0.0 {
            continue;
        }
        let part = orient(&part);
        let mut rings = vec![exterior(&part)];
        rings.extend(interiors(&part));
        let rings: Vec<Vec<P2>> = rings.into_iter().map(|mut r| { r.pop(); r }).collect();
        // inset (mitre) of every ring vertex
        let mut inset: Vec<P2> = vec![];
        for r in &rings {
            let n = r.len();
            let nrm = |a: P2, b: P2| -> P2 {
                let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt().max(1e-12);
                [-(b[1] - a[1]) / l, (b[0] - a[0]) / l]
            };
            for i in 0..n {
                let n0 = nrm(r[(i + n - 1) % n], r[i]);
                let n1 = nrm(r[i], r[(i + 1) % n]);
                let k = (wt / 2.0) / (1.0 + n0[0] * n1[0] + n0[1] * n1[1]).max(0.05);
                inset.push([r[i][0] + (n0[0] + n1[0]) * k, r[i][1] + (n0[1] + n1[1]) * k]);
            }
        }
        let flat: Vec<P2> = rings.iter().flatten().copied().collect();
        // floor / ceiling: the same triangulation for outer and inner layer
        let mut fl = Mesh::new();
        {
            let mut f2: Vec<f64> = vec![];
            let mut holes = vec![];
            for (ri, r) in rings.iter().enumerate() {
                if ri > 0 {
                    holes.push(f2.len() / 2);
                }
                for p in r {
                    f2.extend([p[0], p[1]]);
                }
            }
            for t in earcutr::earcut(&f2, &holes, 2).unwrap_or_default().chunks(3) {
                fl.pos.clear();
                let _ = t;
            }
            let ts = earcutr::earcut(&f2, &holes, 2).unwrap_or_default();
            for t in ts.chunks(3) {
                let (a, b, cc) = (flat[t[0]], flat[t[1]], flat[t[2]]);
                let ccw = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]) > 0.0;
                let tri = if ccw { [t[0], t[1], t[2]] } else { [t[0], t[2], t[1]] };
                let ctr = [(a[0] + b[0] + cc[0]) / 3.0, (a[1] + b[1] + cc[1]) / 3.0];
                let int = if is_int(ctr[0], ctr[1]) { INTERIOR } else { 0 };
                let p3 = |pts: &[P2], i: usize, z: f64| -> V3 { [pts[i][0], pts[i][1], z] };
                // outer floor (down), outer ceiling (up), inner floor (up), inner ceiling (down)
                tris.push(Tri { v: [p3(&flat, tri[0], 0.0), p3(&flat, tri[2], 0.0), p3(&flat, tri[1], 0.0)], tags: SUPPORT | int });
                tris.push(Tri { v: [p3(&flat, tri[0], h), p3(&flat, tri[1], h), p3(&flat, tri[2], h)], tags: CEILING | int });
                tris.push(Tri { v: [p3(&inset, tri[0], low), p3(&inset, tri[1], low), p3(&inset, tri[2], low)], tags: SUPPORT | int | VISIBLE });
                tris.push(Tri { v: [p3(&inset, tri[0], high), p3(&inset, tri[2], high), p3(&inset, tri[1], high)], tags: CEILING | int | VISIBLE });
            }
        }
        let mut base = 0;
        for r in &rings {
            let n = r.len();
            for i in 0..n {
                let (i0, i1) = (base + i, base + (i + 1) % n);
                let (a, b) = (flat[i0], flat[i1]);
                let int = if is_int((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0) { INTERIOR } else { 0 };
                let q3 = |pts: &[P2], j: usize, z: f64| -> V3 { [pts[j][0], pts[j][1], z] };
                // outer wall faces away from the cavity (region on the left of a->b)
                let oq = [q3(&flat, i0, 0.0), q3(&flat, i1, 0.0), q3(&flat, i1, h), q3(&flat, i0, h)];
                tris.push(Tri { v: [oq[0], oq[1], oq[2]], tags: WALL | int });
                tris.push(Tri { v: [oq[0], oq[2], oq[3]], tags: WALL | int });
                let iq = [q3(&inset, i0, low), q3(&inset, i0, high), q3(&inset, i1, high), q3(&inset, i1, low)];
                tris.push(Tri { v: [iq[0], iq[1], iq[2]], tags: WALL | int | VISIBLE });
                tris.push(Tri { v: [iq[0], iq[2], iq[3]], tags: WALL | int | VISIBLE });
            }
            base += n;
        }
    }
    tris
}

fn exterior_line(g: &G) -> G {
    let mut tot = vec![];
    for p in parts(g) {
        tot.extend(exterior(&p));
    }
    linestring(&tot).unwrap()
}
