//! opcheck OP --in case.json [--params '{...}'] [--blender res.json] [--tol 1e-5] [--require exact|surface|none] [--out rust.json] [-q]
//!
//! Runs the Rust port of ONE mesh operation on the same case.json that `tools/opcheck.py` feeds to Blender and compares the two results.
//! (case.json / params format: see the header of tools/opcheck.py; the Rust op reads the same keys and uses the same defaults.)
//!   target/fast/opcheck bevel --in tools/opcases/cube.json --params '{"offset":0.1,"segments":3}' --blender /tmp/r.json
//!   target/fast/opcheck --list
//! Without --blender only the Rust result is produced (written to --out). Reports vertex/face counts, exact-order max error, face-list
//! identity (exact and up to cyclic rotation), nearest-neighbour vertex-set match both ways, symmetric surface distance (sampled),
//! and the FIRST differing vertex / face with its neighbourhood (indices + coords) so the bug can be located.
//! Level reached: `exact` (same vertex order, same faces in the same order) > `verts` (same vertex order, same face set, face order
//! differs) > `surface` (same surface, any order) > `none`.  Params may carry "_tol" and "_require" (harness meta, not sent to Blender).
//! Exit code: 0 = required level reached (default requirement = the table's level for the op, see OPS), 1 = mismatch, 2 = op/params
//! not supported by the Rust port or a Rust error.  The last stdout line is machine readable:
//!   RESULT <op> <case-name> level=<exact|surface|none> required=<..> status=<OK|MISMATCH|UNSUPPORTED> ...
//! Add an op: write `fn r_name(c:&Case,p:&Value)->Res` and add a row to OPS.
use indoorgen::dhash::HashMap;
use indoorgen::mesh::Mesh;
use serde_json::{json, Value};

type Res = Result<Mesh, String>;

struct Case {
    j: Value,
    mesh: Mesh,
    /// the Blender mesh's edge list (from the blender result "in_edges"; Blender orders edges of a from_pydata mesh by its own rule)
    edges: Option<Vec<[u32; 2]>>,
}

fn f3(v: &Value) -> [f64; 3] {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
}
fn mesh_of(verts: &Value, polys: &Value, round32: bool) -> Mesh {
    let mut m = Mesh::new();
    m.pos = verts.as_array().unwrap().iter().map(|p| { let q = f3(p); if round32 { [q[0] as f32 as f64, q[1] as f32 as f64, q[2] as f32 as f64] } else { q } }).collect();
    m.faces = polys.as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    m
}
fn pf(p: &Value, k: &str, d: f64) -> f64 {
    p.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}
fn pi(p: &Value, k: &str, d: i64) -> i64 {
    p.get(k).and_then(|x| x.as_i64()).unwrap_or(d)
}
fn pb(p: &Value, k: &str, d: bool) -> bool {
    p.get(k).and_then(|x| x.as_bool()).unwrap_or(d)
}
fn ps<'a>(p: &'a Value, k: &str, d: &'a str) -> &'a str {
    p.get(k).and_then(|x| x.as_str()).unwrap_or(d)
}

/// Blender-style edge list of a face mesh (first use order of the corners)
fn edge_list(m: &Mesh, order: &Option<Vec<[u32; 2]>>) -> (Vec<[u32; 2]>, Vec<Vec<u32>>, Vec<Vec<usize>>) {
    let mut emap: HashMap<(u32, u32), u32> = HashMap::default();
    let mut edges = vec![];
    let mut ce = vec![];
    let mut ef: Vec<Vec<usize>> = vec![];
    for e in order.iter().flatten() {
        emap.insert((e[0].min(e[1]), e[0].max(e[1])), edges.len() as u32);
        edges.push(*e);
        ef.push(vec![]);
    }
    for (fi, f) in m.faces.iter().enumerate() {
        let mut fe = vec![];
        for c in 0..f.len() {
            let (a, b) = (f[c], f[(c + 1) % f.len()]);
            let k = (a.min(b), a.max(b));
            let id = *emap.entry(k).or_insert_with(|| {
                edges.push([k.0, k.1]);
                ef.push(vec![]);
                (edges.len() - 1) as u32
            });
            ef[id as usize].push(fi);
            fe.push(id);
        }
        ce.push(fe);
    }
    (edges, ce, ef)
}

fn fnormal(m: &Mesh, f: &[u32]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-30);
    [n[0] / l, n[1] / l, n[2] / l]
}

/// selected edge indices (into `edge_list`) from sel_edges
fn sel_edges(c: &Case, edges: &[[u32; 2]], ef: &[Vec<usize>]) -> Result<Vec<usize>, String> {
    let s = c.j.get("sel_edges").cloned().unwrap_or(json!("all"));
    if let Some(t) = s.as_str() {
        if t == "all" {
            return Ok((0..edges.len()).collect());
        }
        if let Some(d) = t.strip_prefix("sharp:") {
            let lim = d.parse::<f64>().map_err(|e| e.to_string())?.to_radians();
            let mut out = vec![];
            for (i, fs) in ef.iter().enumerate() {
                if fs.len() == 2 {
                    let (a, b) = (fnormal(&c.mesh, &c.mesh.faces[fs[0]]), fnormal(&c.mesh, &c.mesh.faces[fs[1]]));
                    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
                    if d.acos() > lim {
                        out.push(i);
                    }
                }
            }
            return Ok(out);
        }
        return Err(format!("sel_edges {t}"));
    }
    let mut out = vec![];
    for x in s.as_array().ok_or("sel_edges")? {
        if let Some(i) = x.as_u64() {
            out.push(i as usize);
        } else {
            let (a, b) = (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32);
            let k = [a.min(b), a.max(b)];
            out.push(edges.iter().position(|e| *e == k).ok_or("sel edge not found")?);
        }
    }
    Ok(out)
}

fn sel_idx(c: &Case, key: &str, n: usize) -> Vec<usize> {
    match c.j.get(key) {
        None => (0..n).collect(),
        Some(v) if v.as_str() == Some("all") => (0..n).collect(),
        Some(v) => v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect(),
    }
}

fn bm_out(bm: &indoorgen::bm::BMesh) -> Mesh {
    let (p, _e, f) = bm.to_mesh();
    let mut out = Mesh::new();
    out.pos = p.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    out.faces = f;
    out
}

fn bevel_otype(s: &str) -> Result<indoorgen::bm::bevel::OffsetType, String> {
    use indoorgen::bm::bevel::OffsetType::*;
    Ok(match s {
        "OFFSET" => Offset,
        "WIDTH" => Width,
        "DEPTH" => Depth,
        "PERCENT" => Percent,
        "ABSOLUTE" => Absolute,
        _ => return Err(format!("offset_type {s}")),
    })
}

fn r_bevel(c: &Case, p: &Value) -> Res {
    use indoorgen::bm::bevel::*;
    use indoorgen::bm::*;
    let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let (edges, ce, ef) = edge_list(&c.mesh, &c.edges);
    let mut bm = BMesh::from_mesh(&pos, &edges, &c.mesh.faces, &ce);
    indoorgen::assets::bevmod::set_mesh_vert_normals(&mut bm, &pos, &c.mesh.faces);
    let o = BevelOpts {
        offset: if ps(p, "offset_type", "OFFSET") == "PERCENT" { pf(p, "offset_pct", 0.0) } else { pf(p, "offset", 0.1) } as f32,
        offset_type: bevel_otype(ps(p, "offset_type", "OFFSET"))?,
        segments: pi(p, "segments", 1) as i32,
        profile: pf(p, "profile", 0.5) as f32,
        limit_offset: pb(p, "clamp_overlap", false),
        loop_slide: pb(p, "loop_slide", true),
        mat_nr: -1,
    };
    if ps(p, "affect", "EDGES") == "VERTICES" {
        for v in bm.vert_ids() {
            bm.verts[v].flag &= !ELEM_TAG;
        }
        for e in bm.edge_ids() {
            bm.edges[e].flag &= !ELEM_TAG;
        }
        for v in sel_idx(c, "sel_verts", pos.len()) {
            bm.verts[v].flag |= ELEM_TAG;
        }
        bm_mesh_bevel_ex(&mut bm, &o, true)?;
    } else {
        let sel = sel_edges(c, &edges, &ef)?;
        bevel_edges(&mut bm, &sel, &o)?;
    }
    Ok(bm_out(&bm))
}

fn r_bevel_mod(c: &Case, p: &Value) -> Res {
    let seg = pi(p, "segments", 1) as i32;
    let angle = match ps(p, "limit_method", "ANGLE") {
        "ANGLE" => Some(pf(p, "angle_limit", 30f64.to_radians()) as f32),
        "NONE" => None,
        m => return Err(format!("limit_method {m}")),
    };
    if ps(p, "offset_type", "OFFSET") != "OFFSET" {
        return Err("offset_type != OFFSET".into());
    }
    indoorgen::assets::bevmod::bevel_modifier_ex(&c.mesh, pf(p, "width", 0.1), seg, angle, pb(p, "use_clamp_overlap", true), pf(p, "profile", 0.5) as f32, c.edges.as_deref())
}

fn r_solidify(c: &Case, p: &Value) -> Res {
    let th = pf(p, "thickness", 0.1);
    let off = pf(p, "offset", -1.0);
    match ps(p, "solidify_mode", "EXTRUDE") {
        "EXTRUDE" => {
            if pb(p, "use_even_offset", false) || pf(p, "thickness_clamp", 0.0) != 0.0 || pb(p, "use_flip_normals", false) {
                return Ok(indoorgen::assets::bpyops::solidify(&c.mesh, th, off));
            }
            let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
            let (edges, ce, _ef) = edge_list(&c.mesh, &c.edges);
            let s = indoorgen::assets::solidx::solidify_extrude(&pos, &edges, &c.mesh.faces, &ce, th as f32, off as f32, pb(p, "use_rim", true));
            let mut m = Mesh::new();
            m.pos = s.pos.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
            m.faces = s.faces;
            Ok(m)
        }
        "NON_MANIFOLD" => {
            use indoorgen::assets::solidnm::*;
            let b = match ps(p, "nonmanifold_boundary_mode", "NONE") {
                "NONE" => Boundary::None,
                "ROUND" => Boundary::Round,
                _ => Boundary::Flat,
            };
            let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
            let r = solidify_nonmanifold_ex(&pos, &c.mesh.faces, if std::env::var("SOLNOEDGES").is_ok() { None } else { c.edges.as_deref() }, th as f32, off as f32, b)?;
            let mut m = Mesh::new();
            m.pos = r.pos.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
            m.faces = r.faces;
            Ok(m)
        }
        m => Err(format!("solidify_mode {m}")),
    }
}

fn loops(c: &Case) -> Result<Vec<Vec<u32>>, String> {
    let l = c.j.get("loops").ok_or("case needs \"loops\"")?;
    Ok(l.as_array().unwrap().iter().map(|a| a.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect())
}

fn r_bridge(c: &Case, p: &Value) -> Res {
    let l = loops(c)?;
    if l.len() != 2 {
        return Err("bridge: exactly two loops supported".into());
    }
    let cuts = pi(p, "number_cuts", 0) as usize;
    let shape = pf(p, "profile_shape_factor", 0.0);
    let mode = match ps(p, "interpolation", "PATH") {
        "LINEAR" => indoorgen::assets::bridge::Interp::Linear,
        "SURFACE" => indoorgen::assets::bridge::Interp::Surface,
        _ => indoorgen::assets::bridge::Interp::Path,
    };
    indoorgen::assets::bridge::bridge_bm(&c.mesh, &l[0], &l[1], cuts, shape, mode, pf(p, "smoothness", 1.0), ps(p, "profile_shape", "SMOOTH"), c.edges.as_deref())
}

fn r_fill_grid(c: &Case, p: &Value) -> Res {
    if !pb(p, "use_interp_simple", true) {
        return Err("fill_grid: only use_interp_simple=True".into());
    }
    let l = loops(c)?;
    if l.len() != 1 {
        return Err("fill_grid: exactly one loop".into());
    }
    let lp: Vec<[f64; 3]> = l[0].iter().map(|&i| c.mesh.pos[i as usize]).collect();
    Ok(indoorgen::assets::bpyops::fill_grid_loop(&lp))
}

fn r_subsurf(c: &Case, p: &Value) -> Res {
    let lv = pi(p, "levels", 1) as usize;
    let simple = match ps(p, "subdivision_type", "CATMULL_CLARK") {
        "CATMULL_CLARK" => false,
        "SIMPLE" => true,
        t => return Err(format!("subdivision_type {t}")),
    };
    if std::env::var("SUBSURF_OLD").is_ok() {
        return Ok(indoorgen::assets::subsurf::subsurf(&c.mesh, lv, simple));
    }
    // optional per-loop UV (case key "uv", from tools/stagedump_uv): Blender refines UV-carrying meshes with a face-varying channel
    let uvids: Option<Vec<i32>> = c.j.get("uv").and_then(|u| u.as_array()).map(|u| {
        let mut ids: std::collections::HashMap<(u32, u64, u64), i32> = std::collections::HashMap::new();
        let mut out = vec![];
        let mut li = 0;
        for f in &c.mesh.faces {
            for &v in f {
                let (a, b) = (u[li][0].as_f64().unwrap().to_bits(), u[li][1].as_f64().unwrap().to_bits());
                let n = ids.len() as i32;
                out.push(*ids.entry((v, a, b)).or_insert(n));
                li += 1;
            }
        }
        out
    });
    if uvids.is_some() {
        return indoorgen::assets::subsurf::subsurf_osd_uv(&c.mesh, lv, simple, true, pi(p, "quality", 3) as usize, c.edges.as_deref(), uvids.as_deref()).ok_or_else(|| "OpenSubdiv refiner failed".to_string());
    }
    indoorgen::assets::subsurf::subsurf_osd(&c.mesh, lv, simple, c.edges.as_deref()).ok_or_else(|| "OpenSubdiv refiner failed".to_string())
}

fn r_subdivide(c: &Case, p: &Value) -> Res {
    if pi(p, "number_cuts", 1) != 1 || pf(p, "smoothness", 0.0) != 0.0 || c.j.get("sel_edges").map_or(false, |s| s.as_str() != Some("all")) {
        return Err("subdivide: only all edges, 1 cut, smoothness 0 (== simple subdivision of quads)".into());
    }
    if c.mesh.faces.iter().any(|f| f.len() != 4) {
        return Err("subdivide: quads only".into());
    }
    Ok(indoorgen::assets::bathkit::simple_subsurf(&c.mesh, 1))
}

fn gn_of(m: &Mesh) -> indoorgen::gn::geom::Mesh {
    indoorgen::gn::geom::Mesh::from_faces(m.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), &m.faces, &[])
}
fn from_gn(r: &indoorgen::gn::geom::Mesh) -> Mesh {
    let mut o = Mesh::new();
    o.pos = r.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    o.faces = r.faces_vec();
    o
}

fn r_boolean(c: &Case, p: &Value) -> Res {
    use indoorgen::exbool::exact_boolean::*;
    let o = c.j.get("other").ok_or("case needs \"other\"")?;
    let mut om = mesh_of(&o["verts"], &o["polys"], true);
    if let Some(l) = o.get("loc") {
        let l = f3(l);
        for q in om.pos.iter_mut() {
            for k in 0..3 {
                q[k] = (q[k] as f32 + l[k] as f32) as f64;
            }
        }
    }
    let op = ps(p, "operation", "DIFFERENCE");
    match ps(p, "solver", "EXACT") {
        "EXACT" => {
            let bop = match op {
                "UNION" => BoolOp::Union,
                "INTERSECT" => BoolOp::Intersect,
                _ => BoolOp::Difference,
            };
            let inp = |m: &Mesh| BoolInput { verts: m.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect(), faces: m.faces.clone() };
            let r = exact_boolean(&[inp(&c.mesh), inp(&om)], bop, pb(p, "use_self", false), pb(p, "use_hole_tolerant", false));
            let mut m = Mesh::new();
            m.pos = r.verts.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
            m.faces = r.faces;
            Ok(m)
        }
        "FAST" => {
            let id = indoorgen::gn::geom::M4_ID;
            let (a, b) = (gn_of(&c.mesh), gn_of(&om));
            Ok(from_gn(&indoorgen::gn::fbool::mesh_boolean_float(op, &[(&a, id), (&b, id)])))
        }
        s => Err(format!("solver {s}")),
    }
}

fn r_triangulate(c: &Case, p: &Value) -> Res {
    if ps(p, "ngon_method", "BEAUTY") != "BEAUTY" {
        return Err("triangulate: ngon_method BEAUTY only".into());
    }
    let qm = ps(p, "quad_method", "BEAUTY");
    let sel = vec![true; c.mesh.faces.len()];
    let r = indoorgen::gn::nodes_mesh::triangulate(&gn_of(&c.mesh), &sel, pi(p, "min_vertices", 4) as usize, qm);
    Ok(from_gn(&r))
}

fn r_triangulate_op(c: &Case, p: &Value) -> Res {
    let qm = ps(p, "quad_method", "BEAUTY");
    if ps(p, "ngon_method", "BEAUTY") != "BEAUTY" {
        return Err("triangulate_op: ngon_method BEAUTY only".into());
    }
    let n = c.mesh.faces.len();
    let mut sel = vec![false; n];
    for i in sel_idx(c, "sel_faces", n) {
        sel[i] = true;
    }
    let r = indoorgen::gn::nodes_mesh::triangulate(&gn_of(&c.mesh), &sel, 4, qm);
    Ok(from_gn(&r))
}

fn r_beautify_fill(c: &Case, p: &Value) -> Res {
    if pf(p, "angle_limit", std::f64::consts::PI) < std::f64::consts::PI - 1e-6 {
        return Err("beautify_fill: angle_limit < 180 not ported".into());
    }
    let n = c.mesh.faces.len();
    let sel = sel_idx(c, "sel_faces", n);
    let mut is_sel = vec![false; n];
    for i in sel {
        is_sel[i] = c.mesh.faces[i].len() == 3;
    }
    if c.mesh.faces.iter().any(|f| f.len() != 3) {
        return Err("beautify_fill: triangle meshes only".into());
    }
    let tris: Vec<[u32; 3]> = c.mesh.faces.iter().map(|f| [f[0], f[1], f[2]]).collect();
    let r = indoorgen::assets::bridge::beautify_loop(&c.mesh.pos, tris, &HashMap::default(), c.edges.as_deref(), true, Some(&is_sel));
    let mut m = c.mesh.clone();
    m.faces = r.iter().map(|t| t.to_vec()).collect();
    Ok(m)
}

fn r_remove_doubles(c: &Case, p: &Value) -> Res {
    if c.j.get("sel_verts").map_or(false, |s| s.as_str() != Some("all")) {
        return Err("remove_doubles: all verts only".into());
    }
    Ok(indoorgen::assets::blops::remove_doubles(&c.mesh, pf(p, "threshold", 1e-4) as f32))
}

fn r_displace(c: &Case, p: &Value) -> Res {
    use indoorgen::assets::bpymod::*;
    use indoorgen::assets::legtex::{Tex, TexKind};
    let t = p.get("texture").ok_or("displace: texture spec")?;
    let basis = match t["noise_basis"].as_str().unwrap_or("BLENDER_ORIGINAL") {
        "BLENDER_ORIGINAL" => 0,
        "ORIGINAL_PERLIN" => 1,
        "IMPROVED_PERLIN" => 2,
        b => return Err(format!("noise_basis {b}")),
    };
    let kind = match t["type"].as_str().unwrap_or("CLOUDS") {
        "CLOUDS" => TexKind::Clouds,
        "STUCCI" => TexKind::Stucci(match t["stucci_type"].as_str().unwrap_or("PLASTIC") { "PLASTIC" => 0, "WALL_IN" => 1, _ => 2 }),
        "MARBLE" => TexKind::Marble(
            match t["marble_type"].as_str().unwrap_or("SOFT") { "SOFT" => 0, "SHARP" => 1, _ => 2 },
            match t["noise_basis_2"].as_str().unwrap_or("SIN") { "SIN" => 0, "SAW" => 1, _ => 2 },
        ),
        k => return Err(format!("texture type {k}")),
    };
    let mut tex = Tex::new(kind, t["noise_scale"].as_f64().unwrap_or(0.25));
    tex.basis = basis;
    tex.hard = t["noise_type"].as_str() == Some("HARD_NOISE");
    tex.noisedepth = t["noise_depth"].as_i64().unwrap_or(2) as i32;
    if let Some(x) = t["turbulence"].as_f64() {
        tex.turbul = x as f32;
    }
    let dir = match ps(p, "direction", "NORMAL") { "X" => DispDir::X, "Y" => DispDir::Y, "Z" => DispDir::Z, "NORMAL" => DispDir::Normal, d => return Err(format!("direction {d}")) };
    let mut v: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    displace(&mut v, &c.mesh.faces, Some(&tex), pf(p, "strength", 1.0), pf(p, "mid_level", 0.5), dir, None);
    let mut m = c.mesh.clone();
    m.pos = v.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    Ok(m)
}

fn r_convex_hull(c: &Case, p: &Value) -> Res {
    use indoorgen::bm::*;
    let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let (edges, ce, _ef) = edge_list(&c.mesh, &c.edges);
    let mut bm = BMesh::from_mesh(&pos, &edges, &c.mesh.faces, &ce);
    let mut hp = indoorgen::bm::hull::HullParams::default();
    hp.use_existing_faces = pb(p, "use_existing_faces", true);
    hp.delete_unused = pb(p, "delete_unused", true);
    hp.make_holes = pb(p, "make_holes", false);
    hp.join_triangles = pb(p, "join_triangles", true);
    if !indoorgen::bm::hull::convex_hull(&mut bm, &hp) {
        return Err("convex_hull: cancelled".into());
    }
    Ok(bm_out(&bm))
}

fn r_bm_op(c: &Case, which: u8) -> Res {
    use indoorgen::bm::*;
    let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let (edges, ce, _ef) = edge_list(&c.mesh, &c.edges);
    let mut bm = BMesh::from_mesh(&pos, &edges, &c.mesh.faces, &ce);
    if which == 2 {
        // make_coiled's first four ops in ONE edit session (the mempool free lists carry over)
        use indoorgen::bm::hull::*;
        poke_all(&mut bm);
        tris_to_quads_all(&mut bm);
        if std::env::var("COIL2").is_err() {
            poke_all(&mut bm);
            poke_all(&mut bm);
        }
    } else if which == 0 {
        indoorgen::bm::hull::poke_all(&mut bm);
    } else {
        indoorgen::bm::hull::tris_to_quads_all(&mut bm);
    }
    Ok(bm_out(&bm))
}
fn r_coil4(c: &Case, _p: &Value) -> Res {
    r_bm_op(c, 2)
}
fn r_poke(c: &Case, _p: &Value) -> Res {
    r_bm_op(c, 0)
}
fn r_tris_to_quads(c: &Case, _p: &Value) -> Res {
    r_bm_op(c, 1)
}

fn r_remesh(c: &Case, p: &Value) -> Res {
    let mode = ps(p, "mode", "VOXEL");
    if mode == "VOXEL" {
        // decor worker's surface-nets approximation of OpenVDB (vertex/face order is not OpenVDB's)
        let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
        let mut tris: Vec<[u32; 3]> = vec![];
        for f in &c.mesh.faces {
            for k in 1..f.len() - 1 {
                tris.push([f[0], f[k], f[k + 1]]);
            }
        }
        let r = indoorgen::assets::voxelremesh::voxel_remesh(&pos, &tris, pf(p, "voxel_size", 0.1) as f32);
        let mut m = Mesh::new();
        m.pos = r.v.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
        m.faces = r.f;
        return Ok(m);
    }
    if mode != "SHARP" {
        return Err(format!("remesh: mode {mode} not ported"));
    }
    let pos: Vec<[f32; 3]> = c.mesh.pos.iter().map(|q| [q[0] as f32, q[1] as f32, q[2] as f32]).collect();
    let (v, q) = indoorgen::assets::dualcon::dualcon(&pos, &c.mesh.faces, 2, pi(p, "octree_depth", 4) as i32, pf(p, "scale", 0.9) as f32, pf(p, "sharpness", 1.0) as f32);
    let mut m = Mesh::new();
    m.pos = v.iter().map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]).collect();
    m.faces = q.iter().map(|x| x.iter().map(|&i| i as u32).collect()).collect();
    Ok(m)
}

/// (op, required level, function). Ops missing here (SUBSURF variants, WELD, ...) report UNSUPPORTED.
const OPS: &[(&str, &str, fn(&Case, &Value) -> Res)] = &[
    ("bevel", "exact", r_bevel),
    ("bevel_mod", "exact", r_bevel_mod),
    ("solidify", "exact", r_solidify),
    ("bridge", "exact", r_bridge),
    ("fill_grid", "surface", r_fill_grid),
    ("subsurf", "surface", r_subsurf),
    ("subdivide", "surface", r_subdivide),
    ("boolean", "surface", r_boolean),
    ("triangulate", "exact", r_triangulate),
    ("triangulate_op", "exact", r_triangulate_op),
    ("remove_doubles", "exact", r_remove_doubles),
    ("beautify_fill", "exact", r_beautify_fill),
    ("displace", "exact", r_displace),
    ("convex_hull", "exact", r_convex_hull),
    ("poke", "exact", r_poke),
    ("coil4", "exact", r_coil4),
    ("tris_to_quads", "exact", r_tris_to_quads),
    ("remesh", "surface", r_remesh),
];

// ------------------------------------------------------------------------------------------------ comparison
fn linf(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs()).max((a[2] - b[2]).abs())
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn cs(p: [f64; 3]) -> String {
    format!("({:.6},{:.6},{:.6})", p[0], p[1], p[2])
}
fn canon(f: &[u32]) -> Vec<u32> {
    let k = f.iter().enumerate().min_by_key(|x| *x.1).unwrap().0;
    (0..f.len()).map(|i| f[(k + i) % f.len()]).collect()
}

fn nn_dists(a: &[[f64; 3]], b: &[[f64; 3]]) -> Vec<(f64, usize)> {
    a.iter().map(|p| b.iter().enumerate().map(|(j, q)| (dist(*p, *q), j)).fold((f64::MAX, 0), |m, x| if x.0 < m.0 { x } else { m })).collect()
}

/// triangles covering the surface: tris as is; quads with BOTH diagonals (non-planar quads / different split = same surface);
/// n-gons ear-clipped (concave faces from booleans)
fn tris_of(m: &Mesh) -> Vec<[[f64; 3]; 3]> {
    let mut t = vec![];
    for f in &m.faces {
        let p = |i: usize| m.pos[f[i] as usize];
        match f.len() {
            0..=2 => {}
            3 => t.push([p(0), p(1), p(2)]),
            4 => {
                t.push([p(0), p(1), p(2)]);
                t.push([p(0), p(2), p(3)]);
                t.push([p(1), p(2), p(3)]);
                t.push([p(1), p(3), p(0)]);
            }
            _ => {
                let co: Vec<[f32; 3]> = f.iter().map(|&i| [m.pos[i as usize][0] as f32, m.pos[i as usize][1] as f32, m.pos[i as usize][2] as f32]).collect();
                for tr in indoorgen::assets::polyfill::triangulate_ngon_plain(&co) {
                    t.push([p(tr[0] as usize), p(tr[1] as usize), p(tr[2] as usize)]);
                }
            }
        }
    }
    t
}
fn samples(t: &[[[f64; 3]; 3]]) -> Vec<[f64; 3]> {
    let mut s = vec![];
    for tr in t {
        let mid = |a: [f64; 3], b: [f64; 3]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
        s.extend_from_slice(tr);
        s.push(mid(tr[0], tr[1]));
        s.push(mid(tr[1], tr[2]));
        s.push(mid(tr[2], tr[0]));
        s.push([(tr[0][0] + tr[1][0] + tr[2][0]) / 3.0, (tr[0][1] + tr[1][1] + tr[2][1]) / 3.0, (tr[0][2] + tr[1][2] + tr[2][2]) / 3.0]);
    }
    s
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// closest point on triangle (Ericson)
fn pt_tri(p: [f64; 3], t: &[[f64; 3]; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let r = |q: [f64; 3]| dist(p, q);
    if d1 <= 0.0 && d2 <= 0.0 {
        return r(a);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return r(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return r([a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]]);
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return r(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return r([a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return r([b[0] + w * (c[0] - b[0]), b[1] + w * (c[1] - b[1]), b[2] + w * (c[2] - b[2])]);
    }
    let den = 1.0 / (va + vb + vc);
    let (v, w) = (vb * den, vc * den);
    r([a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w])
}
/// (max, mean) distance of the samples of `a` to the triangles of `b`; None if too expensive
fn surf_dist(a: &Mesh, b: &Mesh) -> Option<(f64, f64)> {
    let (ta, tb) = (tris_of(a), tris_of(b));
    if ta.is_empty() || tb.is_empty() {
        return None;
    }
    let s = samples(&ta);
    if s.len() as f64 * tb.len() as f64 > 6e8 {
        return None;
    }
    let (mut mx, mut sum) = (0f64, 0f64);
    for p in &s {
        let d = tb.iter().map(|t| pt_tri(*p, t)).fold(f64::MAX, f64::min);
        mx = mx.max(d);
        sum += d;
    }
    Some((mx, sum / s.len() as f64))
}

fn neigh_report(tag: &str, m: &Mesh, other: &Mesh, vi: usize, max_faces: usize) {
    println!("  {tag} neighbourhood of vertex {vi}:");
    let mut n = 0;
    for (fi, f) in m.faces.iter().enumerate() {
        if f.contains(&(vi as u32)) {
            n += 1;
            if n > max_faces {
                println!("    ...");
                break;
            }
            let mut s = format!("    face {fi}:");
            for &v in f {
                let o = other.pos.get(v as usize).map(|p| cs(*p)).unwrap_or("-".into());
                s += &format!(" {v}{}={} (other {})", if v as usize == vi { "*" } else { "" }, cs(m.pos[v as usize]), o);
            }
            println!("{s}");
        }
    }
}

struct Cmp {
    level: &'static str,
}

fn compare(rs: &Mesh, bl: &Mesh, tol: f64) -> Cmp {
    println!("verts: rust {} blender {}   faces: rust {} blender {}", rs.pos.len(), bl.pos.len(), rs.faces.len(), bl.faces.len());
    let same_nv = rs.pos.len() == bl.pos.len();
    let same_nf = rs.faces.len() == bl.faces.len();
    let mut order_err = f64::NAN;
    let mut first_v = None;
    if same_nv {
        order_err = 0.0;
        for i in 0..rs.pos.len() {
            let e = linf(rs.pos[i], bl.pos[i]);
            if e > tol && first_v.is_none() {
                first_v = Some(i);
            }
            order_err = order_err.max(e);
        }
    }
    println!("exact-order max error: {}", if same_nv { format!("{order_err:.3e}") } else { "n/a (vertex counts differ)".into() });
    let faces_exact = rs.faces == bl.faces;
    let faces_cyc = same_nf && rs.faces.iter().zip(&bl.faces).all(|(a, b)| a.len() == b.len() && canon(a) == canon(b));
    println!("face lists identical: {}   identical up to cyclic rotation: {}", faces_exact, faces_cyc);
    // as sets (face order free)
    let mut sa: Vec<Vec<u32>> = rs.faces.iter().map(|f| canon(f)).collect();
    let mut sb: Vec<Vec<u32>> = bl.faces.iter().map(|f| canon(f)).collect();
    sa.sort();
    sb.sort();
    let faces_set = sa == sb;
    println!("face sets equal (face order free, same vertex ids): {}", faces_set);
    // nearest neighbour vertex sets
    let nr = nn_dists(&rs.pos, &bl.pos);
    let nb = nn_dists(&bl.pos, &rs.pos);
    let mr = nr.iter().map(|x| x.0).fold(0.0, f64::max);
    let mb = nb.iter().map(|x| x.0).fold(0.0, f64::max);
    println!("nearest-neighbour vertex distance: rust->blender max {mr:.3e} ({} > tol), blender->rust max {mb:.3e} ({} > tol)", nr.iter().filter(|x| x.0 > tol).count(), nb.iter().filter(|x| x.0 > tol).count());
    let sd = (surf_dist(rs, bl), surf_dist(bl, rs));
    let surf_ok = match sd {
        (Some(a), Some(b)) => {
            println!("surface distance (sampled): rust->blender max {:.3e} mean {:.3e}; blender->rust max {:.3e} mean {:.3e}", a.0, a.1, b.0, b.1);
            a.0 <= tol && b.0 <= tol
        }
        _ => {
            println!("surface distance: skipped (empty or too large)");
            mr <= tol && mb <= tol
        }
    };
    let nn_ok = mr <= tol && mb <= tol;
    // first differences
    if let Some(i) = first_v {
        println!("FIRST differing vertex (exact order): {i}  rust {}  blender {}  (linf {:.3e})", cs(rs.pos[i]), cs(bl.pos[i]), linf(rs.pos[i], bl.pos[i]));
        neigh_report("blender", bl, rs, i, 6);
        neigh_report("rust", rs, bl, i, 6);
        if let Some(&(d, j)) = nr.get(i) {
            println!("  rust vertex {i} nearest blender vertex: {j} at {d:.3e}");
        }
    }
    if !nn_ok {
        let mut w: Vec<(f64, usize)> = nb.iter().enumerate().map(|(i, x)| (x.0, i)).filter(|x| x.0 > tol).collect();
        w.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for (d, i) in w.iter().take(5) {
            println!("  blender vertex {i} {} has no rust vertex within tol (nearest {} at {d:.3e})", cs(bl.pos[*i]), nb[*i].1);
        }
        let mut w: Vec<(f64, usize)> = nr.iter().enumerate().map(|(i, x)| (x.0, i)).filter(|x| x.0 > tol).collect();
        w.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for (d, i) in w.iter().take(5) {
            println!("  rust vertex {i} {} has no blender vertex within tol (nearest {} at {d:.3e})", cs(rs.pos[*i]), nr[*i].1);
        }
    }
    if !faces_cyc {
        let n = rs.faces.len().min(bl.faces.len());
        let k = (0..n).find(|&k| rs.faces[k].len() != bl.faces[k].len() || canon(&rs.faces[k]) != canon(&bl.faces[k]));
        match k {
            Some(k) => {
                println!("FIRST differing face: {k}");
                let desc = |m: &Mesh, f: &[u32]| f.iter().map(|&v| format!("{v}={}", m.pos.get(v as usize).map(|p| cs(*p)).unwrap_or("-".into()))).collect::<Vec<_>>().join(" ");
                println!("  rust    : {}", desc(rs, &rs.faces[k]));
                println!("  blender : {}", desc(bl, &bl.faces[k]));
                if k > 0 {
                    println!("  (previous face {} equal: blender {:?})", k - 1, bl.faces[k - 1]);
                }
            }
            None => println!("FIRST differing face: none in common prefix (face counts differ)"),
        }
        if !faces_set {
            let sbs: std::collections::HashSet<&Vec<u32>> = sb.iter().collect();
            let sas: std::collections::HashSet<&Vec<u32>> = sa.iter().collect();
            if let Some(f) = sa.iter().find(|f| !sbs.contains(f)) {
                println!("  first rust face not in blender: {:?}", f);
            }
            if let Some(f) = sb.iter().find(|f| !sas.contains(f)) {
                println!("  first blender face not in rust: {:?}", f);
            }
        }
    }
    let exact = same_nv && same_nf && order_err <= tol && faces_cyc;
    let verts_exact = same_nv && same_nf && order_err <= tol && faces_set;
    let surface = surf_ok && nn_ok;
    Cmp { level: if exact { "exact" } else if verts_exact { "verts" } else if surface { "surface" } else { "none" } }
}

fn rank(l: &str) -> i32 {
    match l {
        "exact" => 3,
        "verts" => 2,
        "surface" => 1,
        _ => 0,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 || a[1] == "--list" {
        println!("ops: {}", OPS.iter().map(|o| format!("{}({})", o.0, o.1)).collect::<Vec<_>>().join(" "));
        return;
    }
    let op = a[1].clone();
    let arg = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let inp = arg("--in").expect("--in case.json");
    let mut j: Value = serde_json::from_str(&std::fs::read_to_string(&inp).unwrap()).unwrap();
    let mut params = j.get("params").cloned().unwrap_or(json!({}));
    if let Some(s) = arg("--params") {
        let o: Value = serde_json::from_str(&s).expect("--params json");
        for (k, v) in o.as_object().unwrap() {
            params[k] = v.clone();
        }
    }
    // the blender result's params (defaults filled in python) are authoritative when comparing
    for k in params.as_object().unwrap().keys().filter(|k| k.starts_with("sel_") || *k == "loops" || *k == "other").cloned().collect::<Vec<_>>() {
        j[&k] = params[&k].clone();
        params.as_object_mut().unwrap().remove(&k);
    }
    // harness meta keys inside params: "_tol" (comparison tolerance), "_require" (level floor for this case); ignored by Blender
    let meta_tol = params.get("_tol").and_then(|x| x.as_f64());
    let meta_req = params.get("_require").and_then(|x| x.as_str()).map(|s| s.to_string());
    params.as_object_mut().unwrap().remove("_tol");
    params.as_object_mut().unwrap().remove("_require");
    let tol: f64 = arg("--tol").map(|s| s.parse().unwrap()).or(meta_tol).unwrap_or(1e-5);
    let name = j.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| std::path::Path::new(&inp).file_stem().unwrap().to_string_lossy().to_string());
    let (_, req_default, f) = match OPS.iter().find(|o| o.0 == op) {
        Some(o) => *o,
        None => {
            println!("RESULT {op} {name} level=none required=- status=UNSUPPORTED (no rust op)");
            std::process::exit(2);
        }
    };
    let required = arg("--require").or(meta_req).unwrap_or(req_default.to_string());
    let round32 = true;
    let mesh = mesh_of(&j["verts"], &j["polys"], round32);
    j["verts"] = json!(mesh.pos);
    let blender_json: Option<Value> = arg("--blender").map(|bp| serde_json::from_str(&std::fs::read_to_string(&bp).unwrap()).unwrap());
    let edges = blender_json.as_ref().and_then(|b| b.get("in_edges")).map(|e| e.as_array().unwrap().iter().map(|x| [x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32]).collect());
    // parameters Blender actually used (python fills its defaults, e.g. the DISPLACE texture) are the base, explicit params override
    if let Some(bpm) = blender_json.as_ref().and_then(|b| b.get("params")).and_then(|x| x.as_object()) {
        for (k, v) in bpm {
            if params.get(k).is_none() && !k.starts_with("sel_") {
                params[k] = v.clone();
            }
        }
    }
    let c = Case { j, mesh, edges };
    let t0 = std::time::Instant::now();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&c, &params)));
    let r = match r {
        Ok(r) => r,
        Err(_) => Err("rust panic".into()),
    };
    let rs = match r {
        Ok(m) => m,
        Err(e) => {
            println!("RESULT {op} {name} level=none required={required} status=UNSUPPORTED ({e})");
            std::process::exit(2);
        }
    };
    println!("rust {op} on '{name}' took {:?}  params {}", t0.elapsed(), params);
    if let Some(o) = arg("--out") {
        std::fs::write(&o, json!({"op": op, "verts": rs.pos, "polys": rs.faces}).to_string()).unwrap();
    }
    let Some(b) = blender_json else {
        println!("RESULT {op} {name} level=none required={required} status=NOBLENDER verts={} faces={}", rs.pos.len(), rs.faces.len());
        return;
    };
    if let Some(call) = b.get("call") {
        println!("blender call: {}", call);
    }
    // re-run the rust op with the parameters Blender actually used (python fills defaults) where the keys are known to both
    let bl = mesh_of(&b["verts"], &b["polys"], false);
    let cmp = compare(&rs, &bl, tol);
    let ok = rank(cmp.level) >= rank(&required);
    println!("RESULT {op} {name} level={} required={required} status={} verts={}/{} faces={}/{}", cmp.level, if ok { "OK" } else { "MISMATCH" }, rs.pos.len(), bl.pos.len(), rs.faces.len(), bl.faces.len());
    std::process::exit(if ok { 0 } else { 1 });
}
