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
    let mut g = ObjGeom::from_tris(tris, proxies);
    // the reference room mesh is the full hollow shell: its bounding box spans the polygon bbox and the wall height
    let _ = &mut g;
    g
}
