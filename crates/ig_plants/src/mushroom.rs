//! mushroom/*.py (MushroomFactory: cap + stem + gills, clusters of 1-5). Constructor and instance draws follow the python order; the cap / stem are spun
//! from the same Bezier profiles (blops::spin_anchors) and decorated with the same modifiers, but the voxel remeshes, the stem band / web geometry nodes and the
//! morel pattern are not reproduced (approximation, not surface-equal).
use super::bpymod::{simple_deform, Deform};
use super::legtex::{displace, Tex, TexKind};
use super::*;
use crate::mesh::{Mat4, Mesh};
use crate::rng::Rng;
use super::leafkit::LMesh;
use std::f64::consts::PI;

pub fn handles(c: &str) -> bool {
    c == "MushroomFactory"
}

type V = [f64; 3];

#[derive(Clone, Debug)]
struct CapCfg {
    x: Vec<f64>,
    z: Vec<f64>,
    vec: Vec<usize>,
    has_gill: bool,
}

#[derive(Clone, Debug)]
struct GillCfg {
    x: Vec<f64>,
    z: Vec<f64>,
    vec: Vec<usize>,
}

pub struct MushroomFactory {
    fs: u64,
    directional: bool,
    lowered: bool,
    tolerant: f64,
    cap: CapCfg,
    radius: f64,
    inner_radius: f64,
    gill: Option<GillCfg>,
    web: u8, // 0 hollow, 1 solid, 2 none
    has_band: bool,
    is_morel: bool,
}

fn cap_configs(r: &mut Rng) -> Vec<CapCfg> {
    let mut v = vec![];
    // campanulate
    let x = r.uniform(0.12, 0.15);
    let z = vec![0.0, 0.0, r.uniform(0.03, 0.05), r.uniform(0.1, 0.12), r.uniform(0.16, 0.2), 0.2];
    v.push(CapCfg { x: vec![0.0, x, x, 0.08, 0.04, 0.0], z, vec: vec![], has_gill: true });
    // conical
    let z = r.uniform(0.2, 0.3);
    let x1 = r.uniform(0.12, 0.15);
    v.push(CapCfg { x: vec![0.0, x1, 0.01, 0.0], z: vec![0.0, 0.0, z, z], vec: vec![1], has_gill: true });
    // convex
    let z = r.uniform(0.14, 0.16);
    let z2 = r.uniform(0.04, 0.06);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.01, 0.0], z: vec![0.0, 0.0, z2, z, z], vec: vec![1], has_gill: true });
    // depressed
    let z = r.uniform(0.03, 0.05);
    let z2 = r.uniform(0.06, 0.08);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.0], z: vec![0.0, 0.0, z2, z], vec: vec![1], has_gill: true });
    // flat
    let z = r.uniform(0.05, 0.07);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.0], z: vec![0.0, 0.0, z, z], vec: vec![1], has_gill: true });
    // infundiuliform
    let z = r.uniform(0.08, 0.12);
    let x = r.uniform(0.12, 0.15);
    let zz = z + r.uniform(0.005, 0.01);
    v.push(CapCfg { x: vec![0.0, 0.03, x, x - 0.01, 0.0], z: vec![0.0, 0.0, z, zz, 0.02], vec: vec![], has_gill: false });
    // ovate
    let z = r.uniform(0.2, 0.3);
    let x1 = r.uniform(0.12, 0.15);
    v.push(CapCfg { x: vec![0.0, x1, 0.08, 0.01, 0.0], z: vec![0.0, 0.0, 0.8 * z, z, z], vec: vec![1], has_gill: true });
    // umbillicate
    let z = r.uniform(0.03, 0.05);
    let z2 = r.uniform(0.06, 0.08);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.02, 0.0], z: vec![0.0, 0.04, z2, z + 0.02, z], vec: vec![], has_gill: false });
    // umbonate
    let z = r.uniform(0.05, 0.07);
    let z_ = z + r.uniform(0.02, 0.04);
    v.push(CapCfg { x: vec![0.0, 0.15, 0.12, 0.06, 0.02, 0.0], z: vec![0.0, 0.0, z - 0.01, z, z_, z_], vec: vec![1], has_gill: true });
    v
}

impl MushroomFactory {
    pub fn new(fs: u64) -> MushroomFactory {
        // MushroomFactory.__init__
        let mut r = Rng::seeded(fs);
        let directional = r.randint(0, 2) == 0;
        let lowered = r.uniform(0.0, 1.0) < 0.5;
        // MushroomGrowthFactory / cap / stem each restart at the factory seed
        let mut rc = Rng::seeded(fs);
        // growth: base hue (the cap factory receives it but only uses it for the shader)
        let _ = (rc.uniform(0.0, 1.0),);
        let mut rc = Rng::seeded(fs);
        let xs = rc.uniform(0.7, 1.4);
        let zs = rc.uniform(0.7, 1.4);
        let cfgs = cap_configs(&mut rc);
        let idx = rc.choice_p(&[2.0, 2.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 1.0]);
        let mut cap = cfgs[idx].clone();
        cap.x = cap.x.iter().map(|v| v * xs).collect();
        cap.z = cap.z.iter().map(|v| v * zs).collect();
        let radius = cap.x.iter().cloned().fold(f64::MIN, f64::max);
        let inner_radius = rc.log_uniform(0.2, 0.35) * radius;
        let adn = GillCfg {
            x: vec![radius, (radius + inner_radius) / 2.0, inner_radius, inner_radius, radius],
            z: vec![0.0, -rc.uniform(0.05, 0.08), -rc.uniform(0.0, 0.02), 0.0, 0.0],
            vec: vec![3],
        };
        let dec = GillCfg {
            x: vec![radius, (radius + inner_radius) / 2.0, inner_radius, 0.0, radius],
            z: vec![0.0, -rc.uniform(0.05, 0.08), -rc.uniform(0.08, 0.1), 0.0, 0.0],
            vec: vec![2],
        };
        let gi = rc.choice_p(&[1.0, 1.0, 1.0]);
        let gill = if !cap.has_gill { None } else { [Some(adn), Some(dec), None][gi].clone() };
        let shader = rc.choice_p(&[2.0, 1.0, 1.0, 1.0]);
        let is_morel = rc.uniform(0.0, 1.0) < 0.5 && shader == 0;
        let mut rs = Rng::seeded(fs);
        let web = rs.choice_p(&[1.0, 1.0, 2.0]) as u8;
        let has_band = rs.uniform(0.0, 1.0) < 0.75;
        let tolerant = r.uniform(0.0, 0.2);
        if std::env::var("MUCTOR").is_ok() { eprintln!("CTOR {} band {} web {} morel {} gill {} directional {} r {:.3}", fs, has_band, web, is_morel, gill.is_some(), directional, radius); }
        MushroomFactory { fs, directional, lowered, tolerant, cap, radius, inner_radius, gill, web, has_band, is_morel }
    }

    fn spin(&self, x: &[f64], z: &[f64], vec: &[usize]) -> Mesh {
        let anchors: Vec<[f64; 3]> = (0..x.len()).map(|i| [x[i], 0.0, z[i]]).collect();
        // draw.spin: bezier mesh (already welded by curve2mesh) -> mean radius -> WELD again -> spin -> remove_doubles
        let prof = super::blops::bezier_curve_points(&anchors, vec);
        let mean_r: f64 = prof.iter().map(|p| ((p[0] * p[0] + p[1] * p[1]) as f64).sqrt()).sum::<f64>() / prof.len() as f64;
        let steps = ((2.0 * PI * mean_r / 5e-3) as usize).min(128);
        let w = super::blops::weld_polyline(&prof, 1e-3);
        let m = super::blops::spin_profile(&w, steps);
        super::blops::remove_doubles(&m, 1e-3)
    }

    fn deform(m: &mut Mesh, method: Deform, angle: f64, axis: usize) {
        let mut v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        simple_deform(&mut v, method, angle, axis);
        for (p, q) in m.pos.iter_mut().zip(&v) {
            *p = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
    }

    fn lm(m: &Mesh) -> LMesh {
        LMesh { v: m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), f: m.faces.clone(), e: vec![] }
    }
    fn from_lm(l: &LMesh) -> Mesh {
        let mut o = Mesh::new();
        o.pos = l.v.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
        o.faces = l.f.clone();
        o
    }
    fn dump(name: &str, m: &Mesh) {
        if let Ok(d) = std::env::var("MUDUMP") {
            let v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
            let _ = std::fs::write(format!("{d}/{name}.json"), serde_json::json!({"verts": v, "polys": m.faces}).to_string());
        }
    }
    fn disp(m: &mut Mesh, tex: Option<&Tex>, strength: f64, mid: f64, vg: Option<&[f32]>) {
        let mut v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        super::bpymod::displace(&mut v, &m.faces, tex, strength, mid, super::bpymod::DispDir::Normal, vg);
        for (p, q) in m.pos.iter_mut().zip(&v) {
            *p = [q[0] as f64, q[1] as f64, q[2] as f64];
        }
    }

    /// draw.remesh_fill with ig_kit's dual contouring port; its missing hole patching can leave quads that reference non-existent vertices
    /// (thin tips of the gill sheet), those quads are dropped
    fn remesh_fill_safe(m: &LMesh, resolution: f64) -> LMesh {
        let n = m.v.len();
        let s = super::leafkit::solidify(m, 0.1);
        let top: Vec<f32> = (0..s.v.len()).map(|i| if i >= n { 1.0 } else { 0.0 }).collect();
        let dims = s.dimensions();
        let maxd = dims[0].max(dims[1]).max(dims[2]) as f64;
        let depth = (((maxd + 0.01) / resolution).log2()).ceil() as i32;
        let (rv, rq) = super::dualcon::dualcon(&s.v, &s.f, 2, depth, 0.9, 1.0);
        let nv = rv.len() as i32;
        let rf: Vec<Vec<u32>> = rq.iter().filter(|q| q.iter().all(|&i| i >= 0 && i < nv)).map(|q| q.iter().map(|&i| i as u32).collect()).collect();
        let mut r = LMesh { v: rv, f: rf, e: vec![] };
        r.ensure_face_edges();
        let samp = super::leafkit::sample_nearest_surface_ex(&s, &top, &r.v, true);
        let tv: Vec<f32> = samp.iter().map(|x| x.0).collect();
        for (i, x) in samp.iter().enumerate() {
            r.v[i] = x.1;
        }
        let del: Vec<bool> = tv.iter().map(|&t| t > 1e-3).collect();
        r.delete_verts(&del)
    }

    fn cap_mesh(&self, i: u64, face_size: f64) -> Mesh {
        let mut r = inst_rng(self.fs, i);
        let mut obj = self.spin(&self.cap.x, &self.cap.z, &self.cap.vec);
        Self::dump("c004", &obj);
        let src = Self::lm(&obj);
        let tris = super::dualcon::corner_tris(&src.v, &src.f);
        let nets = super::voxelremesh::voxel_remesh(&src.v, &tris, face_size as f32);
        obj = Self::from_lm(&LMesh { v: nets.v, f: nets.f, e: vec![] });
        Self::dump("c005", &obj);
        {
            // transfer_all: every vertex moves onto the nearest point of the pre-remesh surface
            let mut l = Self::lm(&obj);
            let snapped = super::leafkit::sample_nearest_surface(&src, &vec![0f32; src.v.len()], &l.v);
            for (i, sp) in snapped.iter().enumerate() {
                l.v[i] = sp.1;
            }
            obj = Self::from_lm(&l);
        }
        Self::dump("c009", &obj);
        // geo_morel draws (always evaluated)
        let vscale = r.uniform(15.0, 20.0);
        let vrand = r.uniform(0.5, 1.0);
        if self.is_morel {
            let w: Vec<f32> = obj
                .pos
                .iter()
                .enumerate()
                .map(|(vi, p)| {
                    let prm = crate::gn::noise::VorP { feature: 3, metric: 0, dim: 3, scale: vscale as f32, detail: 0.0, roughness: 0.5, lacunarity: 2.0, smoothness: 0.0, exponent: 0.0, randomness: (vrand as f32).clamp(0.0, 1.0), normalize: false, max_distance: 0.5 + 0.5 * (vrand as f32).clamp(0.0, 1.0) };
                    let sc = vscale as f32;
                    let d = crate::gn::noise::vor_fractal_dist_to_edge(&prm, [p[0] as f32 * sc, p[1] as f32 * sc, p[2] as f32 * sc, 0.0]);
                    if std::env::var("MUDBG").is_ok() && (d - 0.05).abs() < 2e-5 { eprintln!("BORDER {:?} d {:e} scale {} rand {}", p, d - 0.05, vscale, vrand); }
                    if std::env::var("MUDBG2").is_ok() && [4179, 29865, 29866, 59744, 64495].contains(&vi) { eprintln!("IDX {} {:?} d {:e}", vi, p, d - 0.05); }
                    if d < 0.05 { 1.0 } else { 0.0 }
                })
                .collect();
            Self::disp(&mut obj, None, 0.04, 0.7, Some(&w));
            Self::dump("c010", &obj);
        }
        if let Some(g) = &self.gill {
            let steps = r.randint(8, 20) as usize;
            let pts: Vec<[f64; 3]> = (0..g.x.len()).map(|k| [g.x[k], 0.0, g.z[k]]).collect();
            let prof = super::blops::bezier_curve_points(&pts, &g.vec);
            let w = super::blops::weld_polyline(&prof, 1e-3);
            let n = w.len();
            let mut outline = LMesh { v: w, f: vec![], e: (0..n.saturating_sub(1) as u32).map(|i| [i, i + 1]).collect() };
            outline.e.push([n as u32 - 1, 0]);
            let filled = super::leafkit::fill_outline(&outline);
            Self::dump("g012", &Self::from_lm(&filled));
            Self::dump("g013", &Self::from_lm(&super::leafkit::solidify(&filled, 0.1)));
            let sheet = Self::remesh_fill_safe(&filled, 0.005);
            let sheet = Self::from_lm(&sheet);
            Self::dump("g017", &sheet);
            // bpy.ops.mesh.spin(dupli=True): copy k+1 = R * copy k (float32), then remove_doubles
            let a = -((2.0 * PI) as f32 / steps as f32);
            let (c, sn) = ((a as f64).cos() as f32, (a as f64).sin() as f32);
            let mut cur: Vec<[f32; 3]> = sheet.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
            let mut gm = Mesh::new();
            for k in 0..steps {
                let off = gm.pos.len() as u32;
                for p in &cur {
                    gm.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
                }
                for f in &sheet.faces {
                    gm.faces.push(f.iter().map(|&v| v + off).collect());
                }
                cur = cur.iter().map(|p| [p[0] * c + p[1] * -sn, p[0] * sn + p[1] * c, p[2]]).collect();
            }
            Self::dump("g018", &gm);
            let mut gill = super::blops::remove_doubles(&gm, 1e-3);
            Self::dump("g019", &gill);
            gill = Self::subsurf2face(&gill, face_size);
            Self::dump("g019b", &gill);
            obj.append(&gill);
            Self::dump("c020", &obj);
        }
        let marble = r.choice_idx(2) == 1;
        let scale = r.log_uniform(0.01, 0.05);
        let kind = if marble { TexKind::Marble(0, 0) } else { TexKind::Stucci(0) };
        Self::disp(&mut obj, Some(&Tex::new(kind, scale)), 0.008, 0.0, None);
        Self::dump("c011", &obj);
        // geo_extension(0.1)
        let ns = r.uniform(0.05, 0.1);
        let sc = r.uniform(1.4, 2.8);
        let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
        obj = super::gnhelpers::geo_extension(&obj, off, sc, ns);
        Self::dump("c012", &obj);
        let a = r.uniform(-PI / 4.0, PI / 4.0);
        Self::deform(&mut obj, Deform::Twist, a, 0);
        Self::dump("c013", &obj);
        let rr = [r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25), r.uniform(-0.25, 0.25)];
        for p in obj.pos.iter_mut() {
            let (x, y) = (p[0] as f32, p[1] as f32);
            p[0] = (x + (if x > 0.0 { rr[0] } else { rr[1] } * x as f64) as f32) as f64;
            p[1] = (y + (if y > 0.0 { rr[2] } else { rr[3] } * y as f64) as f32) as f64;
        }
        Self::dump("c014", &obj);
        obj
    }

    fn graphs() -> &'static serde_json::Value {
        static G: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        G.get_or_init(|| serde_json::from_str(include_str!("../data/mushroom_gn.json")).unwrap())
    }

    /// evaluate one exported node group on a mesh; `patches` = (node name, input id, value). Returns the mesh and its edge array
    fn run_group(name: &str, m: &Mesh, edges: &[[u32; 2]], patches: &[(&str, &str, serde_json::Value)]) -> (Mesh, Vec<[u32; 2]>) {
        use crate::gn;
        use crate::gn::geom::{Geometry as GGeom, Mesh as GMeshT};
        use crate::gn::val::Ty;
        let mut j = Self::graphs().clone();
        if let Some(nodes) = j["groups"][name]["nodes"].as_array_mut() {
            for n in nodes.iter_mut() {
                for (nn, sock, v) in patches {
                    if n["name"].as_str() == Some(*nn) {
                        if let Some(ins) = n["in"].as_array_mut() {
                            for s in ins.iter_mut() {
                                if s["id"].as_str() == Some(*sock) {
                                    s["v"] = v.clone();
                                }
                            }
                        }
                    }
                }
            }
        }
        let groups = gn::load::load_groups(j);
        let g = groups.map.get(name).unwrap();
        let v: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let gm = GMeshT::from_faces(v, &m.faces, edges);
        let mut inputs: crate::dhash::HashMap<String, gn::Value> = crate::dhash::HashMap::default();
        let gi = g.inputs.iter().find(|i| i.ty == Ty::Geo).unwrap();
        inputs.insert(gi.id.clone(), gn::Value::geo(GGeom::from_mesh(gm)));
        let outs = gn::eval(&groups, name, &inputs, &gn::Objects::default()).expect("mushroom group eval");
        let oi = g.outputs.iter().position(|o| o.ty == Ty::Geo).unwrap();
        match &outs[oi].1 {
            gn::Value::G(gg) => {
                let r = gn::meshops::realize_instances(gg, 1000);
                let mut o = Mesh::new();
                let mut e = vec![];
                if let Some(gm) = &r.mesh {
                    o.pos = gm.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
                    o.faces = gm.faces_vec();
                    e = gm.edges.clone();
                }
                (o, e)
            }
            _ => (Mesh::new(), vec![]),
        }
    }

    /// SMOOTH modifier (all axes) over the edge array
    fn smooth(m: &mut Mesh, edges: &[[u32; 2]], factor: f32, iterations: usize) {
        let n = m.pos.len();
        let mut co: Vec<[f32; 3]> = m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let fo = 1.0f32 - factor;
        for _ in 0..iterations {
            let mut acc = vec![[0.0f32; 3]; n];
            let mut cnt = vec![0u32; n];
            for e in edges {
                let (a, b) = (e[0] as usize, e[1] as usize);
                let mid = [(co[a][0] + co[b][0]) * 0.5, (co[a][1] + co[b][1]) * 0.5, (co[a][2] + co[b][2]) * 0.5];
                cnt[a] += 1;
                cnt[b] += 1;
                for k in 0..3 {
                    acc[a][k] += mid[k];
                    acc[b][k] += mid[k];
                }
            }
            for i in 0..n {
                if cnt[i] > 0 {
                    let f = 1.0f32 / cnt[i] as f32;
                    for k in 0..3 {
                        acc[i][k] *= f;
                    }
                }
                for k in 0..3 {
                    co[i][k] = fo * co[i][k] + factor * acc[i][k];
                }
            }
        }
        m.pos = co.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    }

    fn snap_remesh(obj: &Mesh, face_size: f64, tag: &str) -> Mesh {
        let src = Self::lm(obj);
        let tris = super::dualcon::corner_tris(&src.v, &src.f);
        let nets = super::voxelremesh::voxel_remesh(&src.v, &tris, face_size as f32);
        let raw = Self::from_lm(&LMesh { v: nets.v.clone(), f: nets.f.clone(), e: vec![] });
        Self::dump(&format!("{tag}raw"), &raw);
        let mut l = LMesh { v: nets.v, f: nets.f, e: vec![] };
        let snapped = super::leafkit::sample_nearest_surface(&src, &vec![0f32; src.v.len()], &l.v);
        for (i, sp) in snapped.iter().enumerate() {
            l.v[i] = sp.1;
        }
        Self::from_lm(&l)
    }

    /// decorate.subsurface2face_size
    fn subsurf2face(m: &Mesh, face_size: f64) -> Mesh {
        let mut tot = 0.0f64;
        for f in &m.faces {
            tot += face_area(m, f);
        }
        let area = tot / m.faces.len().max(1) as f64;
        if area < 1e-6 {
            return m.clone();
        }
        let levels = (area / face_size).log2().ceil() as i64;
        if levels > 0 {
            super::subsurf::subsurf_osd(m, levels as usize, false, None).unwrap_or_else(|| super::subsurf::subsurf(m, levels as usize, false))
        } else {
            m.clone()
        }
    }

    fn stem_mesh(&self, i: u64, face_size: f64) -> Mesh {
        use serde_json::json;
        let mut r = inst_rng(self.fs, i);
        let ir = self.inner_radius;
        let length = r.log_uniform(0.4, 0.8);
        let x = [0.0, ir, r.log_uniform(1.0, 2.0) * ir, ir * r.uniform(1.0, 1.2), 0.0];
        let z = [0.0, 0.0, -length * r.uniform(0.3, 0.7), -length, -length];
        let mut obj = self.spin(&x, &z, &[1, 4]);
        Self::dump("s018", &obj);
        obj = Self::snap_remesh(&obj, face_size, "s019");
        Self::dump("s021", &obj);
        if self.has_band {
            let scale = r.uniform(0.008, 0.01);
            let wscale = r.log_uniform(5.0, 10.0);
            let dist = r.uniform(5.0, 10.0);
            let thr = -r.uniform(0.3, 0.7) * length;
            let (m, _) = Self::run_group("geo_band", &obj, &[], &[("Wave Texture", "Scale", json!(wscale)), ("Wave Texture", "Distortion", json!(dist)), ("Compare", "B", json!(thr)), ("Vector Math.002", "Scale", json!(scale))]);
            obj = m;
            Self::dump("s022band", &obj);
        }
        if self.web != 2 {
            let mut web;
            if self.web == 0 {
                let outer = ir * r.uniform(2.0, 3.5);
                let zt = r.uniform(0.0, 0.05);
                let len = r.log_uniform(0.2, 0.4);
                let xa = [ir, (outer + ir) / 2.0, outer];
                let za = [-zt, -zt - r.uniform(0.3, 0.4) * len, -zt - len];
                web = self.spin(&xa, &za, &[]);
                Self::dump("w025", &web);
                web = super::subsurf::subsurf_osd(&web, 3, false, None).unwrap_or_else(|| super::subsurf::subsurf(&web, 3, false));
                Self::dump("w026", &web);
                let vs = r.uniform(15.0, 20.0);
                let (m, e) = Self::run_group("geo_voronoi", &web, &[], &[("Voronoi Texture", "Scale", json!(vs))]);
                web = m;
                Self::dump("w027", &web);
                Self::smooth(&mut web, &e, 0.5, 2);
                Self::dump("w028", &web);
            } else {
                let outer = ir * r.uniform(1.5, 3.5);
                let zt = r.uniform(0.0, 0.05);
                let len = r.uniform(0.15, 0.2);
                let xa = [ir, (outer + ir) / 2.0, outer];
                let za = [-zt, -zt - r.uniform(0.3, 0.4) * len, -zt - len];
                web = self.spin(&xa, &za, &[]);
                let sc = -r.uniform(0.008, 0.01);
                let wscale = r.log_uniform(5.0, 10.0);
                let dist = r.uniform(5.0, 10.0);
                let (m, _) = Self::run_group("geo_inverse_band", &web, &[], &[("Wave Texture", "Scale", json!(wscale)), ("Wave Texture", "Distortion", json!(dist)), ("Vector Math.002", "Scale", json!(sc))]);
                web = m;
                Self::dump("w027", &web);
            }
            let strength = r.uniform(0.1, 0.2);
            let nscale = r.uniform(1.4, 2.8);
            let off = [r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0)];
            web = super::gnhelpers::geo_extension(&web, off, nscale, strength);
            Self::dump("w029", &web);
            web = Self::subsurf2face(&web, face_size / 2.0);
            let mut j = web;
            j.append(&obj);
            obj = j;
            Self::dump("s030", &obj);
        }
        let ns = r.uniform(0.005, 0.01);
        Self::disp(&mut obj, Some(&Tex::new(TexKind::Stucci(0), ns)), 0.008, 0.0, None);
        Self::dump("s031", &obj);
        let a = -r.uniform(0.0, PI / 2.0);
        Self::deform(&mut obj, Deform::Bend, a, 1);
        Self::dump("s032", &obj);
        obj
    }

    /// MushroomGrowthFactory.create_asset: cap + stem joined, lowered to z = 0 at the lowest vertex
    fn growth(&self, i: u64) -> Mesh {
        let mut cap = self.cap_mesh(i, 0.0015);
        let stem = self.stem_mesh(i, 0.0015);
        cap.append(&stem);
        let mut lo = 0;
        for (k, p) in cap.pos.iter().enumerate() {
            if p[2] < cap.pos[lo][2] {
                lo = k;
            }
        }
        let l = [-cap.pos[lo][0], -cap.pos[lo][1], -cap.pos[lo][2]];
        for p in cap.pos.iter_mut() {
            for k in 0..3 {
                p[k] += l[k];
            }
        }
        cap
    }

    pub fn mesh(&self, inst: u64) -> Mesh {
        let mut r = inst_rng(self.fs, inst);
        let n = r.randint(1, 6) as usize;
        let mut mushrooms: Vec<Mesh> = vec![];
        let mut keypoints: Vec<Vec<V>> = vec![];
        for j in 0..n {
            let m = self.growth(j as u64 + inst * 10);
            let mut k: Vec<V> = m.pos.iter().step_by(40).copied().filter(|p| p[2] > self.tolerant).collect();
            if k.is_empty() {
                k = m.pos.iter().step_by(40).copied().collect();
            }
            if k.is_empty() {
                k = vec![[0.0; 3]];
            }
            keypoints.push(k);
            mushrooms.push(m);
        }
        let (rotations, start_locs, directions): (Vec<V>, Vec<V>, Vec<V>) = if self.directional {
            let rot_y: Vec<f64> = if self.lowered { (0..n).map(|_| r.uniform(0.0, PI / 6.0)).collect() } else { vec![0.0; n] };
            let rot_z: Vec<f64> = (0..n).map(|_| -PI / 2.0 + r.uniform(-PI / 8.0, PI / 8.0)).collect();
            (
                (0..n).map(|i| [0.0, rot_y[i], rot_z[i]]).collect(),
                (0..n).map(|i| [if n > 1 { self.radius * n as f64 * 0.4 * i as f64 / (n - 1) as f64 } else { 0.0 }, 0.0, 0.0]).collect(),
                vec![[0.0, 1.0, 0.0]; n],
            )
        } else {
            let angles = super::monocot::polygon_angles(&mut r, n, PI / 10.0, PI * 2.0);
            let rot_y: Vec<f64> = if self.lowered { (0..n).map(|_| r.uniform(0.0, PI / 6.0)).collect() } else { vec![0.0; n] };
            let rot_z: Vec<f64> = (0..n).map(|i| angles[i] + r.uniform(-PI / 8.0, PI / 8.0)).collect();
            (
                (0..n).map(|i| [0.0, rot_y[i], rot_z[i]]).collect(),
                vec![[0.0; 3]; n],
                (0..n).map(|i| [angles[i].cos(), angles[i].sin(), 0.0]).collect(),
            )
        };
        // find_closest
        let sc: Vec<f64> = (0..n).map(|_| r.uniform(0.3, 1.2)).collect();
        let mats: Vec<Mat4> = (0..n).map(|i| Mat4::euler_xyz(rotations[i]).mul(&Mat4::scale([sc[i]; 3]))).collect();
        let mut verts: Vec<Vec<V>> = (0..n).map(|i| keypoints[i].iter().map(|p| mat_apply(&mats[i], *p)).collect()).collect();
        let mut locations: Vec<V> = vec![[0.0; 3]];
        for i in 1..n {
            let basis: Vec<V> = verts[..i].iter().flatten().copied().collect();
            let mut chosen = None;
            for k in 0..20 {
                let d = 4.0 * k as f64 / 19.0 * self.radius;
                let off = [start_locs[i][0] + directions[i][0] * d, start_locs[i][1] + directions[i][1] * d, start_locs[i][2] + directions[i][2] * d];
                let mind = verts[i]
                    .iter()
                    .map(|v| {
                        let q = [v[0] + off[0], v[1] + off[1], v[2] + off[2]];
                        basis.iter().map(|b| ((b[0] - q[0]).powi(2) + (b[1] - q[1]).powi(2) + (b[2] - q[2]).powi(2)).sqrt()).fold(f64::MAX, f64::min)
                    })
                    .fold(f64::MAX, f64::min);
                if mind > 0.008 {
                    chosen = Some(off);
                    break;
                }
            }
            let off = chosen.unwrap_or([start_locs[i][0] + directions[i][0] * 4.0 * self.radius, start_locs[i][1] + directions[i][1] * 4.0 * self.radius, start_locs[i][2] + directions[i][2] * 4.0 * self.radius]);
            for v in verts[i].iter_mut() {
                for k in 0..3 {
                    v[k] += off[k];
                }
            }
            locations.push(off);
        }
        let mut out = Mesh::new();
        for i in 0..n {
            let mut m = mushrooms[i].clone();
            m.transform(&Mat4::translation(locations[i]).mul(&mats[i]));
            out.append(&m);
        }
        let a = r.uniform(-PI / 8.0, PI / 8.0);
        let axis = r.choice_idx(2);
        Self::deform(&mut out, Deform::Bend, a, axis);
        out
    }
}

fn mat_apply(m: &Mat4, p: V) -> V {
    let a = &m.0;
    [a[0][0] * p[0] + a[0][1] * p[1] + a[0][2] * p[2] + a[0][3], a[1][0] * p[0] + a[1][1] * p[1] + a[1][2] * p[2] + a[1][3], a[2][0] * p[0] + a[2][1] * p[1] + a[2][2] * p[2] + a[2][3]]
}

impl Factory for MushroomFactory {
    fn asset(&self, inst: u64) -> Asset {
        let mesh = self.mesh(inst);
        let b = mesh.bounds();
        let mut a = Asset::default();
        a.mesh(mesh, &mat("mushroom", [0.5, 0.35, 0.25], 0.6), Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(MushroomFactory::new(fs)))
}

fn face_area(m: &Mesh, f: &[u32]) -> f64 {
    // polygon area as Blender's area_poly_v3 (Newell magnitude / 2)
    let n = f.len();
    let mut a = [0.0f32; 3];
    let p = |k: usize| {
        let q = m.pos[f[k] as usize];
        [q[0] as f32, q[1] as f32, q[2] as f32]
    };
    let mut vp = p(n - 1);
    for k in 0..n {
        let vc = p(k);
        a[0] += (vp[1] - vc[1]) * (vp[2] + vc[2]);
        a[1] += (vp[2] - vc[2]) * (vp[0] + vc[0]);
        a[2] += (vp[0] - vc[0]) * (vp[1] + vc[1]);
        vp = vc;
    }
    ((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() * 0.5f32) as f64
}
