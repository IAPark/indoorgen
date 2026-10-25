//! String to Curves (Bfont glyph outlines extracted from Blender, data/gn/glyphs.json; advances from assets::fontdata)
use super::eval::*;
use super::geom::*;
use super::load::*;
use super::nodes::{outs, Outs};
use super::val::*;
use std::rc::Rc;
use std::sync::OnceLock;

struct Glyph {
    adv: f32,
    splines: Vec<(bool, Vec<[V3; 3]>, Vec<(u8, u8)>)>,
}

fn glyphs() -> &'static crate::dhash::HashMap<char, Glyph> {
    static G: OnceLock<crate::dhash::HashMap<char, Glyph>> = OnceLock::new();
    G.get_or_init(|| {
        let j: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/glyphs.json"))).unwrap();
        let v3 = |a: &serde_json::Value| -> V3 { [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32] };
        let mut m = crate::dhash::HashMap::default();
        for (k, v) in j.as_object().unwrap() {
            let mut g = Glyph { adv: ((v["adv"].as_f64().unwrap() * 1000.0).round() / 1000.0) as f32, splines: vec![] };
            let offs = v["off"].as_array().unwrap();
            let lens = v["len"].as_array().unwrap();
            for ci in 0..offs.len() {
                let (o, l) = (offs[ci].as_u64().unwrap() as usize, lens[ci].as_u64().unwrap() as usize);
                let mut pts = vec![];
                let mut types = vec![];
                for i in o..o + l {
                    pts.push([v3(&v["pos"][i]), v3(&v["hl"][i]), v3(&v["hr"][i])]);
                    types.push((v["tl"][i].as_u64().unwrap() as u8, v["tr"][i].as_u64().unwrap() as u8));
                }
                g.splines.push((v["cyc"][ci].as_bool().unwrap(), pts, types));
            }
            m.insert(k.chars().next().unwrap(), g);
        }
        m
    })
}

fn advance(c: char) -> f32 {
    match glyphs().get(&c) {
        Some(g) => g.adv,
        None => 0.0,
    }
}

pub fn text_nodes(ev: &Ev, ni: usize, node: &Node) -> Res<Option<Outs>> {
    if node.ty != "GeometryNodeStringToCurves" {
        return Ok(None);
    }
    let s = match ev.cval(ni, "String")? {
        Val::S(s) => s.to_string(),
        _ => String::new(),
    };
    let size = ev.cf(ni, "Size")?;
    if std::env::var("S2CDBG").is_ok() {
        eprintln!("S2C {:?} size {}", s, size);
    }
    let mut inst = Instances::default();
    let total: f32 = s.chars().map(advance).sum::<f32>() * size;
    let x0 = match node.ps("align_x") {
        "CENTER" => -total / 2.0,
        "RIGHT" => -total,
        _ => 0.0,
    };
    let mut xo = x0;
    for c in s.chars() {
        if let Some(g) = glyphs().get(&c) {
            let mut cv = Curves::new();
            cv.has_radius = true;
            for (cyc, pts, types) in &g.splines {
                let co: Vec<V3> = pts.iter().map(|p| [p[0][0] * size, p[0][1] * size, p[0][2] * size]).collect();
                cv.push_kind(CKind::Bezier, &co, *cyc);
                let base = cv.pos.len() - co.len();
                for (i, p) in pts.iter().enumerate() {
                    cv.hl[base + i] = [p[1][0] * size, p[1][1] * size, p[1][2] * size];
                    cv.hr[base + i] = [p[2][0] * size, p[2][1] * size, p[2][2] * size];
                    cv.htl[base + i] = types[i].0;
                    cv.htr[base + i] = types[i].1;
                }
            }
            let mut xf: M4 = [[0.0; 4]; 4];
            for i in 0..4 {
                xf[i][i] = 1.0;
            }
            xf[3][0] = xo;
            inst.refs.push(Rc::new(Geometry::from_curves(cv)));
            inst.idx.push((inst.refs.len() - 1) as u32);
            inst.xf.push(xf);
        }
        xo += advance(c) * size;
    }
    let mut o = Geometry::default();
    o.inst = Some(inst);
    Ok(Some(outs(node, vec![("Curve Instances", Value::geo(o))])))
}
