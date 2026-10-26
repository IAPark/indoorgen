//! People and cloth: bed covers over / under a lying person, pillows pushed aside, and a throw blanket draped over a person on a sofa.
//! `apply` edits the scene after the people nodes were added (gen2 hook): sidecar cloth jobs (re-simulated by Blender at render time with the
//! person parts as colliders, tools/cloth_jobs.py) and the approximate default meshes (height-field drape of the body, no Blender needed).
use crate::place::{Cover, Placement};
use crate::rig::{add, rapply, sub, Rig, Xf};
use ig_core::mesh::{Mat4, Mesh, Scene, V3};
use serde_json::json;

/// 2.5D max-height grid (world xy), used to drape the default cloth meshes over bodies and furniture
pub struct Hf {
    x0: f64,
    y0: f64,
    cell: f64,
    nx: usize,
    ny: usize,
    z: Vec<f64>,
}

const NONE: f64 = -1e9;

impl Hf {
    pub fn new(lo: [f64; 2], hi: [f64; 2], cell: f64) -> Hf {
        let nx = (((hi[0] - lo[0]) / cell).ceil() as usize + 2).max(2);
        let ny = (((hi[1] - lo[1]) / cell).ceil() as usize + 2).max(2);
        Hf { x0: lo[0], y0: lo[1], cell, nx, ny, z: vec![NONE; nx * ny] }
    }
    fn put(&mut self, ix: i64, iy: i64, z: f64) {
        if ix >= 0 && iy >= 0 && (ix as usize) < self.nx && (iy as usize) < self.ny {
            let i = iy as usize * self.nx + ix as usize;
            if z > self.z[i] {
                self.z[i] = z;
            }
        }
    }
    /// rasterise a triangle (max z per cell)
    pub fn tri(&mut self, t: &[V3; 3]) {
        let c = self.cell;
        let (mut lx, mut hx, mut ly, mut hy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for q in t {
            lx = lx.min(q[0]);
            hx = hx.max(q[0]);
            ly = ly.min(q[1]);
            hy = hy.max(q[1]);
        }
        let (i0, i1) = (((lx - self.x0) / c).floor() as i64, ((hx - self.x0) / c).ceil() as i64);
        let (j0, j1) = (((ly - self.y0) / c).floor() as i64, ((hy - self.y0) / c).ceil() as i64);
        if i1 < 0 || j1 < 0 || i0 >= self.nx as i64 || j0 >= self.ny as i64 {
            return;
        }
        let d = (t[1][1] - t[2][1]) * (t[0][0] - t[2][0]) + (t[2][0] - t[1][0]) * (t[0][1] - t[2][1]);
        for j in j0.max(0)..=j1.min(self.ny as i64 - 1) {
            for i in i0.max(0)..=i1.min(self.nx as i64 - 1) {
                let (x, y) = (self.x0 + i as f64 * c, self.y0 + j as f64 * c);
                if d.abs() < 1e-14 {
                    continue;
                }
                let l1 = ((t[1][1] - t[2][1]) * (x - t[2][0]) + (t[2][0] - t[1][0]) * (y - t[2][1])) / d;
                let l2 = ((t[2][1] - t[0][1]) * (x - t[2][0]) + (t[0][0] - t[2][0]) * (y - t[2][1])) / d;
                let l3 = 1.0 - l1 - l2;
                let e = -0.35 * c / ((hx - lx).max(c));
                if l1 >= e && l2 >= e && l3 >= e {
                    self.put(i, j, l1 * t[0][2] + l2 * t[1][2] + l3 * t[2][2]);
                }
            }
        }
        for q in t {
            self.put(((q[0] - self.x0) / c).round() as i64, ((q[1] - self.y0) / c).round() as i64, q[2]);
        }
    }
    pub fn mesh(&mut self, m: &Mesh) {
        for t in m.triangles() {
            self.tri(&[m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]]);
        }
    }
    /// cloth surface over the obstacles: z(x) = max_y (h(y) - slope * |x - y|) (a cone dilation, two chamfer passes)
    pub fn drape(&mut self, slope: f64) {
        let (nx, ny) = (self.nx as i64, self.ny as i64);
        let s1 = slope * self.cell;
        let s2 = s1 * std::f64::consts::SQRT_2;
        let fw: [(i64, i64, f64); 4] = [(-1, 0, s1), (0, -1, s1), (-1, -1, s2), (1, -1, s2)];
        let bw: [(i64, i64, f64); 4] = [(1, 0, s1), (0, 1, s1), (1, 1, s2), (-1, 1, s2)];
        for pass in 0..2 {
            let (nb, rng): (&[(i64, i64, f64); 4], Vec<(i64, i64)>) = if pass == 0 {
                (&fw, (0..ny).flat_map(|j| (0..nx).map(move |i| (i, j))).collect())
            } else {
                (&bw, (0..ny).rev().flat_map(|j| (0..nx).rev().map(move |i| (i, j))).collect())
            };
            for (i, j) in rng {
                let mut z = self.z[(j * nx + i) as usize];
                for (di, dj, c) in nb.iter() {
                    let (a, b) = (i + di, j + dj);
                    if a >= 0 && b >= 0 && a < nx && b < ny {
                        let q = self.z[(b * nx + a) as usize];
                        if q > NONE / 2.0 {
                            z = z.max(q - c);
                        }
                    }
                }
                self.z[(j * nx + i) as usize] = z;
            }
        }
    }
    pub fn at(&self, x: f64, y: f64) -> Option<f64> {
        let (fx, fy) = ((x - self.x0) / self.cell, (y - self.y0) / self.cell);
        let (i, j) = (fx.floor() as i64, fy.floor() as i64);
        if i < 0 || j < 0 || i + 1 >= self.nx as i64 || j + 1 >= self.ny as i64 {
            return None;
        }
        let (u, v) = (fx - i as f64, fy - j as f64);
        let g = |a: i64, b: i64| self.z[b as usize * self.nx + a as usize];
        let (z00, z10, z01, z11) = (g(i, j), g(i + 1, j), g(i, j + 1), g(i + 1, j + 1));
        let m = z00.min(z10).min(z01).min(z11);
        if m < NONE / 2.0 {
            return None;
        }
        Some(z00 * (1.0 - u) * (1.0 - v) + z10 * u * (1.0 - v) + z01 * (1.0 - u) * v + z11 * u * v)
    }
}

/// key landmarks of a placed person, world frame
pub struct Landmarks {
    pub pelvis: V3,
    pub chest: V3,
    pub neck: V3,
    pub head: V3,
    pub top_z: f64,
}

pub fn landmarks(pl: &Placement) -> Option<Landmarks> {
    let av = crate::body::avatar_of(pl.avatar)?;
    let g: Vec<Xf> = match &pl.settled {
        Some(s) => s.g.clone(),
        None => crate::body::pose_avatar_seat_g(&av, pl.kind, pl.seed, pl.seat_z).1,
    };
    let rig = Rig::new(&av);
    let p = |n: &str| {
        let i = rig.id(n);
        pl.pose.apply(g[i].apply(rig.rest(i)))
    };
    let skinned = crate::body::skin_parts(&av, &g);
    let mut top = f64::MIN;
    for (pi, part) in skinned.iter().enumerate() {
        if av.parts[pi].kind == crate::avatar::PartKind::Hair {
            continue;
        }
        for q in &part.0 {
            top = top.max(pl.pose.apply([q[0] as f64, q[1] as f64, q[2] as f64])[2]);
        }
    }
    Some(Landmarks { pelvis: p("Bip01 Pelvis"), chest: p("Bip01 Spine2"), neck: p("Bip01 Neck"), head: p("Bip01 Head"), top_z: top })
}

/// names of the person nodes that act as cloth colliders (body + head, no hair cards)
fn body_nodes(pi: usize, pl: &Placement) -> Vec<String> {
    let Some(av) = crate::body::avatar_of(pl.avatar) else { return vec![] };
    av.parts.iter().enumerate().filter(|(_, p)| p.kind != crate::avatar::PartKind::Hair).map(|(k, _)| format!("person{pi}_{:?}#{k}", pl.kind)).collect()
}

fn all_nodes(pi: usize, pl: &Placement) -> Vec<String> {
    let Some(av) = crate::body::avatar_of(pl.avatar) else { return vec![] };
    (0..av.parts.len()).map(|k| format!("person{pi}_{:?}#{k}", pl.kind)).collect()
}

fn mat_from_json(v: &serde_json::Value) -> Option<[[f64; 4]; 4]> {
    let a = v.as_array()?;
    let mut m = [[0.0; 4]; 4];
    for i in 0..4 {
        let r = a.get(i)?.as_array()?;
        for j in 0..4 {
            m[i][j] = r.get(j)?.as_f64()?;
        }
    }
    Some(m)
}

fn person_tris(scene: &Scene, names: &[String]) -> Vec<[V3; 3]> {
    let mut out = vec![];
    for n in &scene.nodes {
        if names.contains(&n.name) {
            for t in n.mesh.triangles() {
                out.push([n.mesh.pos[t[0] as usize], n.mesh.pos[t[1] as usize], n.mesh.pos[t[2] as usize]]);
            }
        }
    }
    out
}

fn env_f(k: &str, d: f64) -> f64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

pub fn enabled() -> bool {
    std::env::var("PEOPLE_CLOTH").map(|v| v != "0").unwrap_or(true)
}

/// Gen2 hook: call after the person nodes were added (names `person{i}_{kind:?}#{part}`) and before the clutter pass.
pub fn apply(people: &[Placement], scene: &mut Scene, floor_z: f64) {
    if !enabled() {
        return;
    }
    for (pi, pl) in people.iter().enumerate() {
        // pillows pushed aside / under the head (both bed variants)
        for (bed, part, d) in &pl.pillow_moves {
            let name = format!("{bed}#{part}");
            if let Some(n) = scene.nodes.iter_mut().find(|n| n.name == name) {
                n.mesh.transform(&Mat4::translation(*d));
            }
            for j in scene.cloth.iter_mut() {
                if j["kind"] == "follow" && j["part"] == name.as_str() {
                    let xy = j["xy"].as_array().cloned().unwrap_or_default();
                    if xy.len() == 2 {
                        j["xy"] = json!([xy[0].as_f64().unwrap_or(0.0) + d[0], xy[1].as_f64().unwrap_or(0.0) + d[1]]);
                    }
                }
            }
        }
        match pl.cover {
            Cover::Under => under_cover(pi, pl, scene),
            Cover::On => on_cover(pi, pl, scene),
            Cover::Throw => throw_blanket(pi, pl, scene, floor_z),
            Cover::None => {}
        }
    }
}

/// bed cloth jobs of the support object of a person: (index into scene.cloth) of the cover jobs (cloth with colliders)
fn bed_jobs(scene: &Scene, bed: &str) -> Vec<usize> {
    let pre = format!("{bed}#");
    scene.cloth.iter().enumerate().filter(|(_, j)| j["kind"] == "cloth" && j["target"].as_str().map(|t| t.starts_with(&pre)).unwrap_or(false) && j["collide"].as_array().map(|c| !c.is_empty()).unwrap_or(false)).map(|(i, _)| i).collect()
}

fn under_cover(pi: usize, pl: &Placement, scene: &mut Scene) {
    let Some(bed) = pl.info.support.clone() else { return };
    let Some(lm) = landmarks(pl) else { return };
    let jobs = bed_jobs(scene, &bed);
    if jobs.is_empty() {
        return;
    }
    let body = body_nodes(pi, pl);
    let btris = person_tris(scene, &body);
    // edge of the cover on the body: halfway between the mid chest and the neck
    // (the sim drags the edge ~10 cm towards the feet while it crumples: start it nearer the neck)
    let ef = env_f("COVER_EDGE", 0.8);
    let edge = [lm.chest[0] + (lm.neck[0] - lm.chest[0]) * ef, lm.chest[1] + (lm.neck[1] - lm.chest[1]) * ef];
    let mut first = true;
    for &ji in &jobs {
        let pose = scene.cloth[ji]["pose"].clone();
        let Some(bm) = mat_from_json(&pose) else { continue };
        // bed-local x of the edge: bed x axis in world = first column of the pose rotation
        let ax = [bm[0][0], bm[1][0]];
        let xe = (edge[0] - bm[0][3]) * ax[0] + (edge[1] - bm[1][3]) * ax[1];
        let target = scene.cloth[ji]["target"].as_str().unwrap_or("").to_string();
        // person parts collide with the cloth
        let mut col: Vec<serde_json::Value> = scene.cloth[ji]["collide"].as_array().cloned().unwrap_or_default();
        for b in &body {
            col.push(json!(b));
        }
        scene.cloth[ji]["collide"] = json!(col);
        let build = scene.cloth[ji]["build"].clone();
        if build.is_null() || !first {
            continue;
        }
        first = false;
        // the first cover job is the sheet / comforter: its head edge goes to the chest, it starts above the body
        let size = build["size"].as_f64().unwrap_or(1.9);
        let x_old = build["x"].as_f64().unwrap_or(1.0);
        let x_new = xe + size * 0.5;
        let dx = x_new - x_old;
        let mut zs: Vec<f64> = btris.iter().filter(|t| (t[0][0] - bm[0][3]) * ax[0] + (t[0][1] - bm[1][3]) * ax[1] > xe + 0.1).flat_map(|t| t.iter().map(|q| q[2])).collect();
        zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let body_top = zs.get(zs.len() * 85 / 100).copied().unwrap_or(0.0);
        let mattress_top = pl.support_top;
        let dz = (body_top - mattress_top).max(0.0) * env_f("COVER_DZK", 0.7) + env_f("COVER_DZ0", 0.02);
        if std::env::var_os("COVER_DBG").is_some() {
            eprintln!("COVER under: edge local x {xe:.2} (x_old {x_old:.2} -> {x_new:.2}) body_top {body_top:.3} support_top {mattress_top:.3} lm.top {:.3} dz {dz:.3}", lm.top_z);
        }
        let b = &mut scene.cloth[ji]["build"];
        b["x"] = json!(x_new);
        b["dz"] = json!(dz);
        // default mesh: shift the sheet like the build, then lift it over the body
        let t = Mat4::translation([ax[0] * dx, ax[1] * dx, 0.0]);
        let mut hf = Hf::new([lm.pelvis[0] - 1.6, lm.pelvis[1] - 1.6], [lm.pelvis[0] + 1.6, lm.pelvis[1] + 1.6], 0.03);
        for tr in &btris {
            if (tr[0][0] - bm[0][3]) * ax[0] + (tr[0][1] - bm[1][3]) * ax[1] > xe {
                hf.tri(tr);
            }
        }
        hf.drape(1.2);
        if let Some(n) = scene.nodes.iter_mut().find(|n| n.name == target) {
            n.mesh.transform(&t);
            for q in n.mesh.pos.iter_mut() {
                if let Some(z) = hf.at(q[0], q[1]) {
                    q[2] = q[2].max(z + 0.02);
                }
            }
        }
    }
}

fn on_cover(pi: usize, pl: &Placement, scene: &mut Scene) {
    let Some(bed) = pl.info.support.clone() else { return };
    let jobs = bed_jobs(scene, &bed);
    if jobs.is_empty() {
        return;
    }
    // the covers are re-simulated without the person; the person is re-dropped onto the new cover surface at the hips
    let bases: Vec<serde_json::Value> = scene.cloth.iter().filter(|j| j["kind"] == "cloth" && j["target"].as_str().map(|t| t.starts_with(&format!("{bed}#"))).unwrap_or(false)).map(|j| j["target"].clone()).collect();
    for name in all_nodes(pi, pl) {
        scene.cloth.push(json!({"kind": "follow", "part": name, "xy": [pl.pose.0[0][3], pl.pose.0[1][3]], "z_used": pl.support_top, "bases": bases}));
    }
}

fn throw_blanket(pi: usize, pl: &Placement, scene: &mut Scene, floor_z: f64) {
    use ig_kit::clothes::{Blanket, BlanketKind};
    let Some(lm) = landmarks(pl) else { return };
    let mut r = ig_core::rng::Rng::seeded(ig_core::int_hash(&[&pl.seed.to_string(), "throw", &pi.to_string()]));
    let width = r.uniform(1.15, 1.45);
    let size = r.uniform(0.85, 1.15);
    let thick = r.uniform(0.005, 0.008);
    // over the lap / legs: centre ahead of the pelvis along the body (towards the feet for lying, forward for sitting)
    let yaw = pl.pose.0[1][0].atan2(pl.pose.0[0][0]);
    let fwd = [-yaw.sin(), yaw.cos()];
    let lying = matches!(pl.kind, crate::PoseKind::LieBed | crate::PoseKind::LieSofa);
    let ahead = if lying { 0.20 } else { 0.30 };
    let c = [lm.pelvis[0] + fwd[0] * ahead, lm.pelvis[1] + fwd[1] * ahead];
    let z0 = if lying { lm.top_z + 0.12 } else { lm.pelvis[2] + 0.32 };
    // pose: width along the person's x axis, size along the body axis (see cloth_jobs._build_blanket: obj yaw pi/2 then pose)
    let pose = Mat4::translation([c[0], c[1], 0.0]).mul(&Mat4::rot_z(yaw - std::f64::consts::FRAC_PI_2));
    let bl = Blanket::from_unseeded(BlanketKind::Blanket, &[width, size, thick, 0.0]);
    let mut pre = bl.create_asset();
    pre.transform(&Mat4::rot_z(std::f64::consts::FRAC_PI_2));
    pre.transform(&pose);
    for q in pre.pos.iter_mut() {
        q[2] += z0;
    }
    // default mesh: drape over the person and the furniture around
    let (plo, phi) = pre.bounds();
    let mut hf = Hf::new([plo[0] - 0.3, plo[1] - 0.3], [phi[0] + 0.3, phi[1] + 0.3], 0.025);
    let body = all_nodes(pi, pl);
    let mut collide: Vec<String> = vec![];
    for n in &scene.nodes {
        if n.mesh.is_empty() || n.class == "room" && !n.name.ends_with(".floor") || n.class == "Door" || n.class == "Window" || n.class == "skirting" || n.class.contains("Light") || n.name.starts_with("mess_") {
            continue;
        }
        let (a, b) = n.mesh.bounds();
        if a[0] > phi[0] + 0.3 || b[0] < plo[0] - 0.3 || a[1] > phi[1] + 0.3 || b[1] < plo[1] - 0.3 || a[2] > z0 + 0.1 {
            continue;
        }
        if n.name.ends_with(".floor") {
            collide.push(n.name.clone());
            continue;
        }
        if n.class != "Person" || body.contains(&n.name) {
            hf.mesh(&n.mesh);
        }
        if n.mesh.pos.len() <= 120000 && !(n.class == "Person" && !body_nodes(pi, pl).contains(&n.name)) {
            collide.push(n.name.clone());
        }
    }
    hf.drape(1.6);
    let mut draped = pre.clone();
    for q in draped.pos.iter_mut() {
        let base = hf.at(q[0], q[1]).unwrap_or(floor_z);
        q[2] = base.max(floor_z) + 0.012;
    }
    // person parts first so that they are never cut by the limit below, then furniture
    let mut cl: Vec<String> = body_nodes(pi, pl);
    for n in collide {
        if !cl.contains(&n) {
            cl.push(n);
        }
    }
    cl.truncate(10);
    let name = format!("throw{pi}#0");
    let mut rgb = [0.0f32; 3];
    for c in rgb.iter_mut() {
        *c = r.uniform(0.25, 0.8) as f32;
    }
    let seed = (pl.seed % 100000) as i64;
    let mi = scene.material(ig_core::materials::spec_material(&format!("throw{pi}"), json!({"cls": "fabric.CoarseKnitFabric", "kwargs": {}, "seed": seed}), rgb));
    scene.add(&name, "Throw", draped, mi);
    let faces = pre.faces.clone();
    scene.cloth.push(json!({
        "kind": "cloth", "target": name, "verts": pre.pos.iter().map(|p| json!([p[0], p[1], p[2]])).collect::<Vec<_>>(), "faces": faces,
        "collide": cl, "kwargs": {"mass": 0.05, "tension_stiffness": 2, "distance_min": 0.005}, "end_frame": 80, "subsurf": 2, "pin": [],
        "build": {"type": "blanket", "width": width, "size": size, "thickness": thick, "margin": 0.0, "fold": null, "x": 0.0, "z": z0},
        "pose": pose.0, "post": null, "z_from": null,
    }));
    let _ = (add, rapply, sub);
}
