//! bathroom/bathroom_sink.py BathroomSinkFactory / StandingSinkFactory.create_asset (shares the bowl / base / hole helpers of bathtub.rs).
use super::bathfree;
use super::bathkit::*;
use super::bathroom::Sink;
use super::bathtub;
use crate::mesh::Mesh;
use crate::rng::Rng;

pub struct SinkBuilt {
    pub body: Mesh,
    pub hole: Mesh,
    pub tap: Option<Mesh>,
    pub stages: Vec<(String, String, Mesh)>,
}

fn stand_mesh(s: &Sink, cuts: i64) -> Mesh {
    let (r, h) = (s.stand_radius, s.stand_height);
    let (cx, cy) = (s.width / 2.0, s.size / 2.0);
    let ring: Vec<[f64; 2]> = if s.stand_circular {
        let g = super::hwexact::bpy_cylinder();
        let _ = g;
        (0..16).map(|i| {
            let (sn, cs) = super::cabnodes::sin_cos_from_fraction(i, 16);
            [sn as f64, cs as f64]
        }).collect()
    } else {
        vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
    };
    let n = ring.len();
    let levels = cuts as usize + 2;
    let sc = s.stand_bottom / s.stand_radius;
    let mut m = Mesh::new();
    for k in 0..levels {
        let z = -h * k as f64 / (levels - 1) as f64;
        let f = if k == levels - 1 { sc } else { 1.0 };
        for p in &ring {
            m.pos.push([cx + p[0] * r * f, cy + p[1] * r * f, z]);
        }
    }
    // outward faces: ring is counter-clockwise from above for the square; the cylinder ring runs clockwise (sin, cos)
    let ccw = {
        let a: f64 = (0..n).map(|i| { let (p, q) = (ring[i], ring[(i + 1) % n]); p[0] * q[1] - q[0] * p[1] }).sum();
        a > 0.0
    };
    for k in 0..levels - 1 {
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b, c, d) = ((k * n + i) as u32, (k * n + j) as u32, ((k + 1) * n + j) as u32, ((k + 1) * n + i) as u32);
            if ccw {
                m.faces.push(vec![a, b, c, d]);
            } else {
                m.faces.push(vec![a, d, c, b]);
            }
        }
    }
    let top: Vec<u32> = (0..n as u32).collect();
    let bot: Vec<u32> = (0..n as u32).map(|i| ((levels - 1) * n) as u32 + i).collect();
    if ccw {
        m.faces.push(top);
        m.faces.push(bot.into_iter().rev().collect());
    } else {
        m.faces.push(top.into_iter().rev().collect());
        m.faces.push(bot);
    }
    let m = simple_subsurf_bl(&m, 2);
    super::subsurf::subsurf_bl(&m, 1, false)
}

fn simple_subsurf_bl(m: &Mesh, l: usize) -> Mesh {
    super::subsurf::subsurf_bl(m, l, true)
}

pub fn build(s: &Sink, r: &mut Rng) -> SinkBuilt {
    let t = &s.tub;
    let mut stages = vec![];
    let mut obj = if s.alcove {
        let mut st = vec![];
        let o = bathtub::alcove_body(t, &mut st);
        stages.extend(st);
        o
    } else {
        let p = bathtub::bowl_params(t);
        // every step carries the UV topology of the bpy cylinder (uvlab.rs): Blender refines UV-carrying meshes with a face-varying channel
        let dump = |name: &str, m: &Mesh| {
            if let Ok(d) = std::env::var("SINKDUMP") {
                let v: Vec<Vec<f32>> = m.pos.iter().map(|p| vec![p[0] as f32, p[1] as f32, p[2] as f32]).collect();
                let _ = std::fs::write(format!("{d}/{name}.json"), serde_json::json!({"verts": v}).to_string());
            }
        };
        let (bowl, lab) = bathfree::make_bowl_uv(&p);
        dump("bowl", &bowl);
        let (rt, lab) = bathfree::remove_top_uv(&bowl, &lab, p.depth);
        let (sol, lab) = bathfree::solidify_exact(&rt, &lab, t.thickness, -1.0);
        dump("solid", &sol);
        let fin = super::uvlab::subsurf_uv(&sol, &lab, 2, false, None).map_or_else(|| sol.clone(), |r| r.0); // (no OpenSubdiv in wasm: the cage)
        dump("side", &fin);
        fin
    };
    // obj.location -= min(co); apply; scale = [width, size, depth] / dimensions; apply
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &obj.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f32);
            hi[k] = hi[k].max(p[k] as f32);
        }
    }
    for p in obj.pos.iter_mut() {
        for k in 0..3 {
            p[k] = (p[k] as f32 - lo[k]) as f64;
        }
    }
    let dims = [t.width, t.size, t.depth];
    let ext = [(hi[0] - lo[0]) as f64, (hi[1] - lo[1]) as f64, (hi[2] - lo[2]) as f64];
    for p in obj.pos.iter_mut() {
        for k in 0..3 {
            p[k] = (p[k] as f32 * (dims[k] / ext[k]) as f32) as f64;
        }
    }
    stage(&mut stages, "scaled", "scaled", &obj);
    if s.has_extrude {
        let c = super::bpyx::face_centers(&obj);
        let nr = super::bpyx::face_normals(&obj);
        let sel: Vec<bool> = (0..obj.faces.len()).map(|i| (c[i][1] as f32) > (t.size / 2.0) as f32 && (nr[i][1] as f32) > 0.1).collect();
        super::bpyx::extrude_region(&mut obj, &sel, [0.0, s.size_extrude * t.size, 0.0]);
        obj = compact(&obj);
    }
    if s.has_stand {
        let cuts = r.randint(3, 6);
        let st = stand_mesh(s, cuts);
        obj = join(&[&obj, &st]);
    }
    stage(&mut stages, "prehole", "prehole", &obj);
    let hole = bathtub::add_hole(t, &obj, r, true, true);
    if std::env::var("HOLEDBG").is_ok() {
        eprintln!("BODYN {} subsurf-nv", obj.pos.len());
    }
    let rot = |m: &Mesh| -> Mesh {
        let mut o = m.clone();
        for p in o.pos.iter_mut() {
            *p = [-p[1], p[0], p[2]];
        }
        o
    };
    let body = rot(&obj);
    let hole = rot(&hole);
    let mut tap = None;
    if s.has_extrude {
        let seed = r.randint(0, 10_000_000) as u64;
        let mut tm = super::taps::tap_mesh(s.fseed, seed);
        let min_x = tm.pos.iter().map(|p| p[0] as f32).fold(f32::INFINITY, f32::min);
        let loc = [((-1.0 - s.size_extrude + s.tap_offset) * t.size) as f32 - min_x, (t.width / 2.0) as f32, t.depth as f32];
        for p in tm.pos.iter_mut() {
            *p = [(p[0] as f32 + loc[0]) as f64, (p[1] as f32 + loc[1]) as f64, (p[2] as f32 + loc[2]) as f64];
        }
        tap = Some(tm);
    }
    stage(&mut stages, "final", "final", &join(&[&body, &hole]));
    SinkBuilt { body, hole, tap, stages }
}

fn stage(st: &mut Vec<(String, String, Mesh)>, name: &str, key: &str, m: &Mesh) {
    st.push((name.to_string(), key.to_string(), m.clone()));
}
