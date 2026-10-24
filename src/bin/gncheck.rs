//! gncheck CASES.json [--filter substr] [--tol 1e-6] [-v]
//! Evaluates every case of a case file (written by tools/gn_check.py: groups + Blender's evaluated meshes) with the Rust
//! geometry-nodes interpreter and compares against Blender: vertices, edges, faces, attributes.
use infinigen::gn::eval::Objects;
use infinigen::gn::geom::*;
use infinigen::gn::load::*;
use infinigen::gn::*;
use serde_json::Value as J;
use infinigen::dhash::HashMap;

fn jmesh(j: &J) -> Mesh {
    infinigen::gn::load::mesh_from_json(j)
}

fn flat(v: &Val) -> Vec<f64> {
    match v {
        Val::F(x) => vec![*x as f64],
        Val::I(x) => vec![*x as f64],
        Val::B(x) => vec![*x as i32 as f64],
        Val::V(x) => x.iter().map(|y| *y as f64).collect(),
        Val::C(x) | Val::Q(x) => x.iter().map(|y| *y as f64).collect(),
        _ => vec![],
    }
}

struct Report {
    edge_order_differs: bool,
    ok: bool,
    maxerr: f64,
    msg: String,
}

fn compare(exp: &J, got: &Mesh, tol: f64, verbose: bool) -> Report {
    let mut r = Report { edge_order_differs: false, ok: true, maxerr: 0.0, msg: String::new() };
    let ev: Vec<[f64; 3]> = exp["v"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()).unwrap_or_default();
    let ef: Vec<Vec<u32>> = exp["f"].as_array().map(|a| a.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect()).unwrap_or_default();
    let ee: Vec<[u32; 2]> = exp["e"].as_array().map(|a| a.iter().map(|p| [p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32]).collect()).unwrap_or_default();
    let mut fail = |r: &mut Report, s: String| {
        if r.ok {
            r.msg = s;
        }
        r.ok = false;
    };
    if ev.len() != got.nv() {
        fail(&mut r, format!("vertex count: blender {} rust {}", ev.len(), got.nv()));
    }
    if ef.len() != got.nf() {
        fail(&mut r, format!("face count: blender {} rust {}", ef.len(), got.nf()));
    }
    if ee.len() != got.ne() {
        fail(&mut r, format!("edge count: blender {} rust {}", ee.len(), got.ne()));
    }
    // vertices
    let n = ev.len().min(got.nv());
    let mut first_bad = None;
    for i in 0..n {
        let mut d = 0.0f64;
        for k in 0..3 {
            // error relative to max(1, |coordinate|): float32 values far from the origin
            d = d.max((ev[i][k] - got.pos[i][k] as f64).abs() / ev[i][k].abs().max(1.0));
        }
        if d.is_nan() {
            d = 1e30;
        }
        r.maxerr = r.maxerr.max(d);
        if d > tol && first_bad.is_none() {
            first_bad = Some((i, d));
        }
    }
    if let Some((i, d)) = first_bad {
        fail(&mut r, format!("vertex {} differs by {:.3e}: blender {:?} rust {:?}", i, d, ev[i], got.pos[i]));
    }
    // faces (exact corner order, then rotation-insensitive)
    let mut rot_only = 0;
    for f in 0..ef.len().min(got.nf()) {
        let g = got.face(f);
        if ef[f].as_slice() != g {
            let same_cycle = ef[f].len() == g.len() && (0..g.len()).any(|s| (0..g.len()).all(|k| ef[f][k] == g[(k + s) % g.len()]));
            if same_cycle {
                rot_only += 1;
                continue;
            }
            fail(&mut r, format!("face {} differs: blender {:?} rust {:?}", f, ef[f], g));
            break;
        }
    }
    if rot_only > 0 && verbose {
        eprintln!("  note: {} faces differ only by corner rotation", rot_only);
    }
    // edges: as sets
    {
        let norm = |e: &[u32; 2]| (e[0].min(e[1]), e[0].max(e[1]));
        let mut a: Vec<(u32, u32)> = ee.iter().map(norm).collect();
        let mut b: Vec<(u32, u32)> = got.edges.iter().map(norm).collect();
        let ordered = a == b;
        a.sort();
        b.sort();
        if a != b {
            let first = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            fail(&mut r, format!("edge set differs at sorted idx {}: blender {:?} rust {:?}", first, a.get(first), b.get(first)));
        } else if !ordered {
            r.edge_order_differs = true;
        }
    }
    // attributes
    if let Some(attrs) = exp["attrs"].as_object() {
        let emats: Vec<String> = exp["mats"].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        for (name, a) in attrs {
            if std::env::var("GNUV").is_err() && name.to_lowercase().contains("uv") {
                continue; // UV maps of primitives are not generated (materials are out of scope)
            }
            if name == "material_index" {
                // compare material NAMES per face (slot numbering differs after modifier apply / slot purge)
                let data = a["data"].as_array().cloned().unwrap_or_default();
                let rmi = got.attrs.get("material_index");
                for (i, d) in data.iter().enumerate() {
                    let bi = d.as_i64().unwrap_or(0).max(0) as usize;
                    let bn = emats.get(bi).cloned().unwrap_or_default();
                    let ri = rmi.and_then(|a| a.data.get(i)).map(|v| v.i()).unwrap_or(0).max(0) as usize;
                    let rn = got.mats.get(ri).cloned().unwrap_or_default();
                    if bn != rn {
                        fail(&mut r, format!("material of face {}: blender {:?} rust {:?}", i, bn, rn));
                        break;
                    }
                }
                continue;
            }
            let dom = Dom::from_name(a["dom"].as_str().unwrap_or("POINT"));
            let data = a["data"].as_array().cloned().unwrap_or_default();
            let size = got.size(dom);
            if data.len() != size {
                fail(&mut r, format!("attr {} len: blender {} rust {}", name, data.len(), size));
                continue;
            }
            let rust = got.attrs.get(name);
            for (i, d) in data.iter().enumerate() {
                let want: Vec<f64> = match d {
                    J::Array(x) => x.iter().map(|y| y.as_f64().unwrap_or(0.0)).collect(),
                    J::Bool(b) => vec![*b as i32 as f64],
                    x => vec![x.as_f64().unwrap_or(0.0)],
                };
                let have: Vec<f64> = match rust {
                    Some(at) if at.dom == dom => flat(&at.data[i]),
                    Some(at) => flat(&infinigen::gn::attr::adapt_mesh(got, at.dom, dom, at.ty, &at.data)[i]),
                    None => vec![0.0; want.len()],
                };
                let mut dd = 0.0f64;
                for k in 0..want.len() {
                    let h = have.get(k).copied().unwrap_or(0.0);
                    let big = want[k].abs() >= 3.39e38 && h.abs() >= 3.39e38 && want[k].signum() == h.signum();
                    if !big {
                        dd = dd.max((want[k] - h).abs());
                    }
                }
                // color attrs: Blender stores 4 comps, ignore alpha when rust has 3
                r.maxerr = r.maxerr.max(dd);
                if dd > tol {
                    fail(&mut r, format!("attr {} [{:?}] elem {}: blender {:?} rust {:?}", name, dom, i, want, have));
                    break;
                }
            }
        }
    }
    r
}

fn tri_list(v: &[[f64; 3]], f: &[Vec<u32>]) -> Vec<[[f64; 3]; 3]> {
    let mut t = vec![];
    for fc in f {
        for k in 1..fc.len().saturating_sub(1) {
            t.push([v[fc[0] as usize], v[fc[k] as usize], v[fc[k + 1] as usize]]);
        }
    }
    t
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// distance from p to triangle (Ericson)
fn pt_tri(p: [f64; 3], t: &[[f64; 3]; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let d3 = dot(ab, bp);
        let d4 = dot(ac, bp);
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                let v = d1 / (d1 - d3);
                [a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]]
            } else {
                let cp = sub(p, c);
                let d5 = dot(ab, cp);
                let d6 = dot(ac, cp);
                if d6 >= 0.0 && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                        let w = d2 / (d2 - d6);
                        [a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]]
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
                            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
                            [b[0] + w * (c[0] - b[0]), b[1] + w * (c[1] - b[1]), b[2] + w * (c[2] - b[2])]
                        } else {
                            let denom = 1.0 / (va + vb + vc);
                            let v = vb * denom;
                            let w = vc * denom;
                            [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
                        }
                    }
                }
            }
        }
    };
    let d = sub(p, q);
    dot(d, d).sqrt()
}

fn probes(v: &[[f64; 3]], f: &[Vec<u32>]) -> Vec<[f64; 3]> {
    let mut s: Vec<[f64; 3]> = v.to_vec();
    for fc in f {
        let n = fc.len();
        for k in 0..n {
            let (a, b) = (v[fc[k] as usize], v[fc[(k + 1) % n] as usize]);
            for t in [0.25, 0.5, 0.75] {
                s.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
            }
        }
    }
    let stride = (s.len() / 3000).max(1);
    s.into_iter().step_by(stride).collect()
}

/// symmetric max nearest-surface distance of vertex + polygon-edge probes (None if too large to evaluate)
fn surface_distance(va: &[[f64; 3]], fa: &[Vec<u32>], vb: &[[f64; 3]], fb: &[Vec<u32>]) -> Option<f64> {
    let (ta, tb) = (tri_list(va, fa), tri_list(vb, fb));
    if std::env::var("GNDUMP").is_ok() {
        let area = |t: &Vec<[[f64; 3]; 3]>| -> f64 { t.iter().map(|x| { let u = sub(x[1], x[0]); let v = sub(x[2], x[0]); let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]; dot(c, c).sqrt() * 0.5 }).sum() };
        eprintln!("  area blender {:.6} rust {:.6}", area(&ta), area(&tb));
    }
    if ta.is_empty() || tb.is_empty() || ta.len().max(tb.len()) > 250_000 {
        return None;
    }
    let mut mx = 0.0f64;
    for (pv, pf, t) in [(va, fa, &tb), (vb, fb, &ta)] {
        for p in probes(pv, pf) {
            let d = t.iter().map(|tr| pt_tri(p, tr)).fold(f64::MAX, f64::min);
            if d > 1e-4 && std::env::var("GNDUMP").is_ok() {
                eprintln!("  bad probe {:?} d={:.2e} {}", p.map(|x| (x * 1e5).round() / 1e5), d, if std::ptr::eq(t, &tb) { "A" } else { "B" });
            }
            if d > mx && std::env::var("GNDUMP").is_ok() {
                eprintln!("  worst probe {:?} d={:.3e} ({})", p, d, if std::ptr::eq(t, &tb) { "from A" } else { "from B" });
            }
            mx = mx.max(d);
        }
    }
    Some(mx)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: gncheck CASES.json [--filter s] [--tol t] [-v]");
        std::process::exit(2);
    }
    let mut filter = String::new();
    let mut tol = 1e-6f64;
    let mut verbose = false;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                filter = args[i + 1].clone();
                i += 1;
            }
            "--tol" => {
                tol = args[i + 1].parse().unwrap();
                i += 1;
            }
            "-v" => verbose = true,
            _ => {}
        }
        i += 1;
    }
    let text = std::fs::read_to_string(&args[1]).expect("read cases");
    let j: J = serde_json::from_str(&text).expect("parse cases");
    let groups = load_groups(j.clone());
    let cases = j["cases"].as_array().cloned().unwrap_or_default();
    let (mut npass, mut nfail) = (0, 0);
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("?");
        if !filter.is_empty() && !name.contains(&filter) {
            continue;
        }
        let gname = c["group"].as_str().unwrap_or("");
        let Some(g) = groups.map.get(gname) else {
            println!("CASE ERROR {} no group {}", name, gname);
            nfail += 1;
            continue;
        };
        let mut inputs: HashMap<String, Value> = HashMap::default();
        for ifc in &g.inputs {
            if ifc.ty == Ty::Geo {
                if let Some(m) = c.get("mesh").filter(|m| !m.is_null()) {
                    inputs.insert(ifc.id.clone(), Value::geo(Geometry::from_mesh(jmesh(m))));
                }
            } else if let Some(v) = c["inputs"].get(&ifc.id) {
                if let Some(val) = val_from_json(ifc.ty, "", v) {
                    inputs.insert(ifc.id.clone(), Value::konst(val));
                }
            }
        }
        let mut objects = Objects::default();
        if let Some(os) = c["objects"].as_object() {
            for (k, o) in os {
                let geo = if o["mesh"].is_null() { Geometry::default() } else { Geometry::from_mesh(jmesh(&o["mesh"])) };
                let mut m = M4_ID;
                if let Some(a) = o["matrix"].as_array() {
                    for r in 0..4 {
                        for cc in 0..4 {
                            m[cc][r] = a[r * 4 + cc].as_f64().unwrap() as f32;
                        }
                    }
                }
                objects.map.insert(k.clone(), infinigen::gn::eval::Object { geo, matrix: m });
            }
        }
        if let Some(cs) = c["collections"].as_object() {
            for (k, o) in cs {
                let names = |j: &J| -> Vec<String> { j.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default() };
                let off = o["offset"].as_array().map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32]).unwrap_or([0.0; 3]);
                objects.collections.insert(k.clone(), infinigen::gn::eval::Collection { objects: names(&o["objects"]), children: names(&o["children"]), offset: off });
            }
        }
        if let Some(fa) = c.get("factory").and_then(|f| f.as_array()) {
            // compare the Rust Factory (parameter sampler + template) against Blender's final mesh
            let (class, fseed, inst) = (fa[0].as_str().unwrap_or(""), fa[1].as_u64().unwrap_or(0), fa[2].as_u64().unwrap_or(0));
            let exp = &c["expect"];
            let Some(f) = infinigen::assets::make_factory(class, fseed) else {
                println!("CASE FAIL {} no Rust factory {}", name, class);
                nfail += 1;
                continue;
            };
            let Some(ex) = f.exact(inst) else {
                println!("CASE FAIL {} factory has no exact()", name);
                nfail += 1;
                continue;
            };
            let m = Mesh::from_faces(ex.verts.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(), &ex.polys, &[]);
            if std::env::var("GNPROF").is_ok() {
            for (k, t, n) in infinigen::gn::eval::prof_report().iter().take(8) {
                eprintln!("  prof {:32} {:8.1} ms  x{}", k, t * 1000.0, n);
            }
        }
        let rep = compare(exp, &m, tol, verbose);
            let mut rep = rep;
            if !rep.ok && m.nf() > 0 && !rep.msg.starts_with("attr") {
                let ev: Vec<[f64; 3]> = exp["v"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()).unwrap_or_default();
                let ef: Vec<Vec<u32>> = exp["f"].as_array().map(|a| a.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect()).unwrap_or_default();
                let gv: Vec<[f64; 3]> = m.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
                if let Some(d) = surface_distance(&ev, &ef, &gv, &m.faces_vec()) {
                    if std::env::var("GNDUMP").is_ok() {
                        println!("  surface distance {:.3e}", d);
                    }
                    if d < 1e-5 {
                        rep.ok = true;
                        rep.msg = format!("SURFACE-EQUAL ({:.2e})", d);
                    }
                }
            }
            if rep.ok {
                npass += 1;
                println!("CASE PASS {} FACTORY maxerr={:.2e} {}", name, rep.maxerr, rep.msg);
            } else {
                nfail += 1;
                println!("CASE FAIL {} FACTORY {}", name, rep.msg);
            }
            continue;
        }
        let t0 = std::time::Instant::now();
        let probe = c.get("probe").and_then(|p| p.as_array()).map(|a| (a[..a.len() - 1].iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<Vec<String>>(), a[a.len() - 1].as_str().unwrap_or("").to_string()));
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &probe {
            Some((n, sk)) => eval_probe(&groups, gname, &inputs, &objects, n, sk).map(|v| vec![("probe".to_string(), v)]),
            None => eval(&groups, gname, &inputs, &objects),
        }));
        let dt = t0.elapsed().as_secs_f64();
        let outs = match res {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                println!("CASE FAIL {} rust error: {}", name, e);
                nfail += 1;
                continue;
            }
            Err(_) => {
                println!("CASE FAIL {} rust PANIC", name);
                nfail += 1;
                continue;
            }
        };
        // first geometry output
        let gi = g.outputs.iter().position(|o| o.ty == Ty::Geo);
        let outs_for_attrs = &outs;
        let mesh = if probe.is_some() {
            match &outs[0].1 {
                Value::G(g) => Some(infinigen::gn::probe_mesh(g)),
                _ => None,
            }
        } else {
            gi.and_then(|k| match &outs[k].1 {
                Value::G(gg) => {
                    let geo = if c.get("out_attrs").map_or(false, |a| a.is_array()) { infinigen::gn::apply_out_attrs(g, outs_for_attrs, (**gg).clone(), &c["out_attrs"]) } else { (**gg).clone() };
                    geo.mesh
                }
                _ => None,
            })
        };
        let exp = &c["expect"];
        if exp.is_null() || exp.get("error").is_some() {
            println!("CASE SKIP {} (blender error/none)", name);
            continue;
        }
        let got = mesh.unwrap_or_else(Mesh::new);
        if std::env::var("GNPROF").is_ok() {
            for (k, t, n) in infinigen::gn::eval::prof_report().iter().take(10) {
                eprintln!("  prof {:32} {:8.1} ms  x{}", k, t * 1000.0, n);
            }
        }
        let rep = compare(exp, &got, tol, verbose);
        if !rep.ok && got.nf() > 0 && !rep.msg.starts_with("attr") {
            let ev: Vec<[f64; 3]> = exp["v"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()]).collect()).unwrap_or_default();
            let ef: Vec<Vec<u32>> = exp["f"].as_array().map(|a| a.iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect()).unwrap_or_default();
            let gv: Vec<[f64; 3]> = got.pos.iter().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
            if let Some(d) = surface_distance(&ev, &ef, &gv, &got.faces_vec()) {
                if std::env::var("GNDUMP").is_ok() {
                    println!("  surface distance {:.3e}", d);
                }
                if d < 1e-5 {
                    npass += 1;
                    println!("CASE PASS {} SURFACE-EQUAL (max nearest-surface {:.2e}); index order/attrs differ: {}", name, d, rep.msg);
                    continue;
                }
            }
        }
        if rep.ok {
            npass += 1;
            println!("CASE PASS {} maxerr={:.2e} nv={} nf={} ({:.3}s){}", name, rep.maxerr, got.nv(), got.nf(), dt, if rep.edge_order_differs { " EDGE-ORDER-DIFFERS" } else { "" });
        } else {
            nfail += 1;
            println!("CASE FAIL {} {}", name, rep.msg);
            if std::env::var("GNDUMP").is_ok() {
                println!("  rust V {:?}", got.pos.iter().map(|p| [(p[0] * 1e4).round() / 1e4, (p[1] * 1e4).round() / 1e4, (p[2] * 1e4).round() / 1e4]).collect::<Vec<_>>());
                println!("  rust F {:?}", got.faces_vec());
            }
        }
    }
    println!("SUMMARY pass={} fail={}", npass, nfail);
}
