//! Camera resolution, ported unchanged from tools/preview/src/main.rs (all math in the scene's Blender Z-up frame):
//! --cam cam.json, --at, --view/--cams NAME, --person I, --top and the auto-camera placement (2 per room).
use crate::glb::{Glb, View};
use bevy::math::{Mat4, Vec2, Vec3};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Cam { pub name: String, pub view: Mat4, pub fov_x: f32, pub ortho: Option<f32> }

#[derive(Clone)]
pub enum CamSpec { Json(PathBuf), At([f32; 6], f32), Person(usize), Top, Named(String) }

pub struct DrawInfo { pub node: usize, pub pos: View, pub idx: View, pub u16: bool, pub icount: u32, pub bmin: Vec3, pub bmax: Vec3 }
pub struct SceneInfo { pub glb: Glb, pub side: Value, pub draws: Vec<DrawInfo>, pub bmin: Vec3, pub bmax: Vec3 }

pub fn cam_look(eye: Vec3, tgt: Vec3, lens: f32) -> Cam {
    let f = (tgt - eye).normalize(); let r = f.cross(Vec3::Z).normalize(); let u = r.cross(f);
    let m = Mat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), eye.extend(1.0));
    Cam { name: String::new(), view: m, fov_x: 2.0 * (18.0f32 / lens).atan(), ortho: None }
}
fn mat_from_json(v: &Value) -> Mat4 {
    let mut c = [0f32; 16];
    for r in 0..4 { for k in 0..4 { c[k * 4 + r] = v[r][k].as_f64().unwrap() as f32; } }
    Mat4::from_cols_array(&c)
}
pub fn cam_json(v: &Value, name: &str) -> Cam {
    let lens = v["lens"].as_f64().unwrap_or(18.0) as f32; let sw = v["sensor_width"].as_f64().unwrap_or(36.0) as f32;
    Cam { name: name.to_string(), view: mat_from_json(&v["matrix"]), fov_x: 2.0 * (sw / (2.0 * lens)).atan(), ortho: None }
}
fn person_pos(s: &SceneInfo, idx: usize) -> Option<Vec3> {
    let mut seen: Vec<String> = vec![];
    for n in s.side["nodes"].as_array()? {
        if n["class"] == "Person" && n["pose"].is_array() {
            let nm = n["name"].as_str()?.split('#').next()?.to_string();
            if !seen.contains(&nm) { seen.push(nm); if seen.len() == idx + 1 { let p = &n["pose"]; return Some(Vec3::new(p[0][3].as_f64()? as f32, p[1][3].as_f64()? as f32, p[2][3].as_f64()? as f32)); } }
        }
    }
    None
}
type Tri = [Vec3; 3];
fn node_tris(s: &SceneInfo, pred: impl Fn(&str) -> bool) -> Vec<Tri> {
    let mut out = vec![];
    for d in &s.draws {
        if !pred(&s.glb.nodes[d.node].name) { continue; }
        let pb = s.glb.bytes(&d.pos);
        let ib = s.glb.bytes(&d.idx);
        let p = |i: usize| -> Vec3 { let r = |k: usize| f32::from_le_bytes(pb[(i * 3 + k) * 4..(i * 3 + k) * 4 + 4].try_into().unwrap()); Vec3::new(r(0), r(1), r(2)) };
        let ix = |k: usize| -> usize { if d.u16 { u16::from_le_bytes(ib[k * 2..k * 2 + 2].try_into().unwrap()) as usize } else { u32::from_le_bytes(ib[k * 4..k * 4 + 4].try_into().unwrap()) as usize } };
        for t in 0..d.icount as usize / 3 { out.push([p(ix(t * 3)), p(ix(t * 3 + 1)), p(ix(t * 3 + 2))]); }
    }
    out
}
fn ray_hit(tris: &[Tri], o: Vec3, dir: Vec3, maxd: f32) -> bool {
    for t in tris {
        let e1 = t[1] - t[0]; let e2 = t[2] - t[0]; let pv = dir.cross(e2); let det = e1.dot(pv);
        if det.abs() < 1e-9 { continue; }
        let inv = 1.0 / det; let tv = o - t[0]; let u = tv.dot(pv) * inv; if !(0.0..=1.0).contains(&u) { continue; }
        let qv = tv.cross(e1); let v = dir.dot(qv) * inv; if v < 0.0 || u + v > 1.0 { continue; }
        let tt = e2.dot(qv) * inv; if tt > 0.01 && tt < maxd { return true; }
    }
    false
}
fn in_tri2(p: Vec3, t: &Tri) -> bool {
    let s = |a: Vec3, b: Vec3, c: Vec3| (a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y);
    let d1 = s(p, t[0], t[1]); let d2 = s(p, t[1], t[2]); let d3 = s(p, t[2], t[0]);
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}
struct Shell { walls: Vec<Tri>, boxes: Vec<(Vec3, Vec3)> }
fn shell_of(s: &SceneInfo) -> Shell {
    let walls = node_tris(s, |n| n.contains(".wall") || n.contains("exterior"));
    let skip = |nm: &str| nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("window") || nm.starts_with("door") || nm.contains("exterior") || nm.contains("rug");
    let boxes = s.draws.iter().filter(|d| !skip(&s.glb.nodes[d.node].name) && d.bmax.z > 0.3 && d.bmin.z < 1.9 && (d.bmax - d.bmin).length() > 0.5).map(|d| (d.bmin, d.bmax)).collect();
    Shell { walls, boxes }
}
impl Shell {
    fn blocked(&self, eye: Vec3, tgt: Vec3) -> bool { let d = tgt - eye; ray_hit(&self.walls, eye, d.normalize(), d.length() - 0.15) }
    fn clearance(&self, e: Vec3) -> f32 {
        let mut m = 3.0f32;
        for k in 0..8 { let a = k as f32 * std::f32::consts::FRAC_PI_4; let d = Vec3::new(a.cos(), a.sin(), 0.0);
            let mut lo = 0.0f32; let mut hi = 3.0f32; if !ray_hit(&self.walls, e, d, hi) { continue; }
            for _ in 0..7 { let mid = (lo + hi) * 0.5; if ray_hit(&self.walls, e, d, mid) { hi = mid } else { lo = mid } } m = m.min(hi); }
        for (lo, hi) in &self.boxes { let dx = (lo.x - e.x).max(0.0).max(e.x - hi.x); let dy = (lo.y - e.y).max(0.0).max(e.y - hi.y); m = m.min((dx * dx + dy * dy).sqrt()); }
        m
    }
    fn openness(&self, e: Vec3, tgt: Vec3, len: f32) -> usize {
        let f = (tgt - e).truncate().normalize_or_zero(); let mut n = 0;
        for k in -4..=4 { let a = (k as f32 * 10.0).to_radians(); let d = Vec3::new(f.x * a.cos() - f.y * a.sin(), f.x * a.sin() + f.y * a.cos(), 0.0); if !ray_hit(&self.walls, e, d, len) { n += 1; } }
        n
    }
}
pub fn auto_cams(s: &SceneInfo, n: usize) -> Vec<Cam> {
    let sh = shell_of(s);
    let skip = |nm: &str| nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.starts_with("window") || nm.starts_with("door") || nm.contains("exterior") || nm.contains("rug");
    let mut floors: HashMap<String, Vec<Tri>> = HashMap::new();
    for d in &s.draws { let nm = &s.glb.nodes[d.node].name; if nm.ends_with(".floor") { floors.entry(nm.clone()).or_default(); } }
    let names: Vec<String> = floors.keys().cloned().collect();
    for nm in names { let t = node_tris(s, |x| x == nm); floors.insert(nm, t); }
    struct Room { name: String, tris: Vec<Tri>, subj: Vec3, nobj: usize, area: f32 }
    let mut rooms: Vec<Room> = vec![];
    for (nm, tris) in floors.iter() {
        if tris.is_empty() { continue; }
        let area: f32 = tris.iter().map(|t| ((t[1] - t[0]).cross(t[2] - t[0]).z.abs()) * 0.5).sum();
        let (mut acc, mut w, mut cnt) = (Vec3::ZERO, 0.0, 0);
        for d in &s.draws { if skip(&s.glb.nodes[d.node].name) { continue; } let c = (d.bmin + d.bmax) * 0.5;
            if tris.iter().any(|t| in_tri2(Vec3::new(c.x, c.y, 0.0), t)) { let wt = (d.bmax - d.bmin).length(); acc += c * wt; w += wt; cnt += 1; } }
        let fc: Vec3 = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0 * ((t[1] - t[0]).cross(t[2] - t[0]).z.abs())).sum::<Vec3>() / (2.0 * area);
        let subj = if w > 0.0 { acc / w } else { fc };
        rooms.push(Room { name: nm.trim_end_matches(".floor").replace('/', "_"), tris: tris.clone(), subj, nobj: cnt, area });
    }
    rooms.sort_by(|a, b| (b.nobj, b.area as i32).cmp(&(a.nobj, a.area as i32)).then(a.name.cmp(&b.name)));
    let mut out = vec![];
    for rm in rooms.iter().filter(|r| r.nobj > 0 || r.area > 6.0) {
        let z0 = rm.tris[0][0].z; let tgt = Vec3::new(rm.subj.x, rm.subj.y, z0 + 0.9);
        let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for t in &rm.tris { for v in t { lo = lo.min(*v); hi = hi.max(*v); } }
        let mut cands: Vec<(f32, Vec3)> = vec![];
        let step = 0.6; let mut y = lo.y + step / 2.0;
        while y < hi.y { let mut x = lo.x + step / 2.0; while x < hi.x {
            let p = Vec3::new(x, y, z0);
            if rm.tris.iter().any(|t| in_tri2(p, t)) {
                let e = Vec3::new(x, y, z0 + 1.5); let dist = (e - tgt).truncate().length();
                if dist > 1.8 && !sh.blocked(e, tgt) { cands.push((dist, e)); }
            }
            x += step; } y += step; }
        cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let boxes: Vec<(Vec3, Vec3)> = s.draws.iter().filter(|d| { let nm = &s.glb.nodes[d.node].name; !(nm.contains(".floor") || nm.contains(".wall") || nm.contains(".ceiling") || nm.contains("skirting") || nm.contains("exterior") || nm.contains("rug") || nm.contains("Ceiling")) && (d.bmax - d.bmin).max_element() > 0.25 }).map(|d| (d.bmin, d.bmax)).collect();
        let inside = |p: Vec3, m: f32| boxes.iter().any(|(l, h)| p.x > l.x - m && p.x < h.x + m && p.y > l.y - m && p.y < h.y + m && p.z > l.z - m && p.z < h.z + m);
        cands.retain(|(_, e)| !inside(*e, 0.15) && ![0.3f32, 0.6, 0.9].iter().any(|t| inside(*e + (tgt - *e).normalize_or_zero() * *t, 0.0)));
        let mut chosen: Vec<Vec3> = vec![]; let mut tried = 0;
        for (dist, e) in cands {
            if chosen.len() >= 2 || tried > 60 { break; }
            tried += 1;
            if dist > 7.5 { continue; }
            let cl = sh.clearance(e); if cl < 0.45 { continue; }
            if sh.openness(e, tgt, 1.2) < 7 { continue; }
            if chosen.iter().any(|c| (*c - e).truncate().length() < 2.5) { continue; }
            chosen.push(e);
        }
        for (k, e) in chosen.iter().enumerate() {
            let mut cm = cam_look(*e, tgt, 18.0); cm.name = format!("{}_{}", rm.name, if k == 0 { "A" } else { "B" }); out.push(cm);
            if out.len() >= n { return out; }
        }
    }
    out
}
pub fn resolve_cams(s: &SceneInfo, specs: &[CamSpec], ncams: usize, res: (u32, u32)) -> Vec<Cam> {
    let mut v = vec![];
    let sidecams = s.side["cameras"].as_array().cloned().unwrap_or_default();
    if specs.is_empty() { if sidecams.is_empty() { v.extend(auto_cams(s, ncams)); } else { for (i, c) in sidecams.iter().enumerate().take(ncams) { v.push(cam_json(c, c["name"].as_str().unwrap_or(&format!("cam{i}")))); } } }
    for sp in specs.iter().cloned() {
        match sp {
            CamSpec::Json(p) => { let j: Value = serde_json::from_slice(&std::fs::read(&p).expect("cam json")).unwrap(); let nm = p.file_stem().unwrap().to_string_lossy().to_string(); v.push(cam_json(&j, &nm)); }
            CamSpec::At(a, lens) => { let mut c = cam_look(Vec3::new(a[0], a[1], a[2]), Vec3::new(a[3], a[4], a[5]), lens); c.name = format!("at{}", v.len()); v.push(c); }
            CamSpec::Named(nm) => {
                if let Some(c) = sidecams.iter().find(|c| c["name"] == nm.as_str()) { v.push(cam_json(c, &nm)); }
                else if nm == "auto" { v.extend(auto_cams(s, ncams)); }
                else if let Some(n) = nm.strip_prefix("cam").and_then(|x| x.parse::<usize>().ok()) { let ac = auto_cams(s, n + 1); if let Some(c) = ac.into_iter().nth(n) { v.push(c); } }
                else { eprintln!("camera {nm} not found"); }
            }
            CamSpec::Person(i) => {
                if let Some(p) = person_pos(s, i) {
                    let sh = shell_of(s);
                    let c0 = (s.bmin + s.bmax) * 0.5; let mut d0 = (c0 - p).truncate().normalize_or_zero(); if d0 == Vec2::ZERO { d0 = Vec2::X; }
                    let base = d0.y.atan2(d0.x); let tgt = p + Vec3::Z * 0.9; let mut best: Option<Vec3> = None;
                    'o: for rad in [3.2f32, 2.6, 2.0, 1.6] {
                        let mut ks: Vec<i32> = (-8..8).collect(); ks.sort_by_key(|k| k.abs());
                        for k in ks { let a = base + k as f32 * std::f32::consts::PI / 8.0; let e = Vec3::new(p.x + a.cos() * rad, p.y + a.sin() * rad, 1.55);
                            if sh.blocked(e, tgt) || sh.blocked(e, p + Vec3::Z * 1.5) || sh.clearance(e) < 0.35 { continue; } best = Some(e); break 'o; }
                    }
                    let eye = best.unwrap_or(p + d0.extend(0.0) * 2.0 + Vec3::Z * 1.6);
                    let mut c = cam_look(eye, tgt, 24.0); c.name = format!("person{i}"); v.push(c);
                } else { eprintln!("person {i} not found"); }
            }
            CamSpec::Top => {
                let c = (s.bmin + s.bmax) * 0.5; let ext = s.bmax - s.bmin;
                let mut cm = cam_look(Vec3::new(c.x, c.y, s.bmax.z + 20.0), Vec3::new(c.x, c.y + 1e-3, c.z), 18.0);
                cm.ortho = Some(ext.x.max(ext.y * res.0 as f32 / res.1 as f32) * 1.05); cm.name = "top".into(); v.push(cm);
            }
        }
    }
    v
}
