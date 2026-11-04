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
    {
        // a lathed column flaring into its foot (subdividing a 16-sided cage with the flare in one step left the foot's rim toothed like a cog,
        // its original and its new corners moving by different amounts)
        let foot = s.stand_bottom.max(r);
        let flare = (h * 0.22).min(0.16);
        let mut prof = vec![(0.0, 0.01), (r, 0.01), (r, -h + flare)];
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            prof.push((r + (foot - r) * t * t * (3.0 - 2.0 * t), -h + flare * (1.0 - t) + 0.012 * t));
        }
        prof.push((foot, -h));
        prof.push((0.0, -h));
        prof.reverse(); // (bottom up: the faces look outward)
        let mut m = Mesh::revolve(0.0, 0.0, &prof, 48);
        // (a square pedestal: the same lathe drawn out to a rounded square; a flared cage of 4 corners subdivided into lumps)
        if !s.stand_circular {
            for p in m.pos.iter_mut() {
                let a = p[1].atan2(p[0]);
                let k = 1.0 / (a.cos().abs().powi(6) + a.sin().abs().powi(6)).powf(1.0 / 6.0);
                p[0] *= k; p[1] *= k;
            }
        }
        for p in m.pos.iter_mut() { p[0] += cx; p[1] += cy; }
        let _ = cuts;
        m
    }
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
        // (drawn for a vanity too: the tap's seed comes after it, and the collision reads the same stream)
        let cuts = r.randint(3, 6);
        if s.vanity.is_none() {
            let st = stand_mesh(s, cuts);
            obj = join(&[&obj, &st]);
        }
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
