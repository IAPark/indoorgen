//! Solved-layout snapshot for debug reruns: `SOLVE_SNAPSHOT=save:PATH` writes the solver state (every object: geometry, pose, tags,
//! generator, relations) and the layers' complaints right after the solve; `SOLVE_SNAPSHOT=load:PATH` builds the plan, shells and
//! features as usual and then takes the objects from the file instead of solving (~7 s saved per house), so the post-solve passes
//! (wall-layer windows, scenegen) run on the same layout. Same seed / config / binary layout code only: nothing checks that the file
//! matches. Tags are written by kind + name (interned ids differ between runs), polygons as rings.
use crate::furn::geom::{ObjGeom, Pose, Proxy, Tri};
use crate::furn::layers::{Complaint, Layer};
use crate::furn::relations::{Geo, GeoKind, Relation};
use crate::furn::state::{GenRef, ObjState, RelState, State};
use crate::furn::tags::{Kind, Tag, TagSet};
use crate::shp::{self, G};
use serde_json::{json, Value};
use std::sync::Arc;

pub enum Mode { Save(String), Load(String) }

pub fn mode() -> Option<Mode> {
    let v = std::env::var("SOLVE_SNAPSHOT").ok()?;
    if let Some(p) = v.strip_prefix("save:") { return Some(Mode::Save(p.into())) }
    if let Some(p) = v.strip_prefix("load:") { return Some(Mode::Load(p.into())) }
    eprintln!("SOLVE_SNAPSHOT wants save:PATH or load:PATH, got {v:?}");
    None
}

fn f(v: &Value) -> f64 { v.as_f64().unwrap() }
fn v3(v: &Value) -> [f64; 3] { [f(&v[0]), f(&v[1]), f(&v[2])] }
fn v3s(v: &Value) -> Vec<[f64; 3]> { v.as_array().unwrap().iter().map(v3).collect() }
fn p2s(v: &Value) -> Vec<[f64; 2]> { v.as_array().unwrap().iter().map(|p| [f(&p[0]), f(&p[1])]).collect() }

fn kind_s(k: Kind) -> &'static str { match k { Kind::Sem => "s", Kind::Sub => "u", Kind::Gen => "g", Kind::Var => "v", Kind::Obj => "o" } }
fn tag_w(t: &Tag) -> Value { json!([kind_s(t.kind()), t.name(), t.neg]) }
fn tag_r(v: &Value) -> Tag {
    let n = v[1].as_str().unwrap();
    let t = match v[0].as_str().unwrap() { "s" => Tag::sem(n), "u" => Tag::sub(n), "g" => Tag::gen(n), "v" => Tag::var(n), _ => Tag::obj(n) };
    if v[2].as_bool().unwrap() { t.negated() } else { t }
}
fn tags_w(t: &TagSet) -> Value { Value::Array(t.0.iter().map(tag_w).collect()) }
fn tags_r(v: &Value) -> TagSet { TagSet::from(v.as_array().unwrap().iter().map(tag_r)) }

fn geo_kind_s(k: GeoKind) -> &'static str { match k { GeoKind::Touching => "touching", GeoKind::SupportedBy => "supported_by", GeoKind::CoPlanar => "coplanar", GeoKind::StableAgainst => "stable_against" } }
fn rel_w(r: &Relation) -> Value {
    match r {
        Relation::Any => json!("any"),
        Relation::Negated(r) => json!({"neg": rel_w(r)}),
        Relation::RoomNeighbour(v) => json!({"neighbour": v}),
        Relation::Geometry(g) => json!({"geo": geo_kind_s(g.kind), "child": tags_w(&g.child), "parent": tags_w(&g.parent), "margin": g.margin, "check_z": g.check_z, "rev_normal": g.rev_normal}),
        Relation::CutFrom => json!("cut_from"),
        Relation::SharedEdge => json!("shared_edge"),
        Relation::Traverse => json!("traverse"),
    }
}
fn rel_r(v: &Value) -> Relation {
    match v.as_str() {
        Some("any") => return Relation::Any,
        Some("cut_from") => return Relation::CutFrom,
        Some("shared_edge") => return Relation::SharedEdge,
        Some("traverse") => return Relation::Traverse,
        _ => {}
    }
    if let Some(r) = v.get("neg") { return Relation::Negated(Box::new(rel_r(r))) }
    if let Some(n) = v.get("neighbour") { return Relation::RoomNeighbour(n.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()) }
    let kind = match v["geo"].as_str().unwrap() { "touching" => GeoKind::Touching, "supported_by" => GeoKind::SupportedBy, "coplanar" => GeoKind::CoPlanar, _ => GeoKind::StableAgainst };
    Relation::Geometry(Geo { kind, child: tags_r(&v["child"]), parent: tags_r(&v["parent"]), margin: f(&v["margin"]), check_z: v["check_z"].as_bool().unwrap(), rev_normal: v["rev_normal"].as_bool().unwrap() })
}

/// polygonal geometry as [[exterior, hole, ...], ...]; anything else is dropped with a note
fn poly_w(key: &str, g: &G) -> Value {
    if shp::is_empty(g) { return json!([]) }
    let ps: Vec<G> = shp::parts(g);
    if !ps.iter().all(shp::is_polygon) { eprintln!("SNAPSHOT {key}: non-polygonal footprint {:?} not saved", shp::gtype(g)); return Value::Null }
    Value::Array(ps.iter().map(|p| { let mut rings = vec![shp::exterior(p)]; rings.extend(shp::interiors(p)); json!(rings) }).collect())
}
fn poly_r(v: &Value, single: bool) -> Option<G> {
    let ps: Vec<G> = v.as_array()?.iter().map(|p| {
        let rings: Vec<Vec<[f64; 2]>> = p.as_array().unwrap().iter().map(p2s).collect();
        shp::polygon(&rings[0], &rings[1..]).unwrap()
    }).collect();
    if single && ps.len() == 1 { return ps.into_iter().next() }
    Some(shp::multipolygon(ps).unwrap())
}

fn geom_w(g: &ObjGeom) -> Value {
    json!({
        "tris": g.tris.iter().map(|t| json!([t.v, t.tags])).collect::<Vec<_>>(),
        "proxies": g.proxies.iter().map(|(p, m)| match p { Proxy::Box { center, half } => json!({"box": [center, half], "m": m}), Proxy::Hull(v) => json!({"hull": v, "m": m}) }).collect::<Vec<_>>(),
        "lo": g.lo, "hi": g.hi,
        "convex": g.convex.as_ref().map(|c| c.as_ref().clone()),
        "roles": g.roles,
    })
}
fn geom_r(v: &Value) -> ObjGeom {
    ObjGeom {
        tris: v["tris"].as_array().unwrap().iter().map(|t| { let vs = v3s(&t[0]); Tri { v: [vs[0], vs[1], vs[2]], tags: t[1].as_u64().unwrap() as u32 } }).collect(),
        proxies: v["proxies"].as_array().unwrap().iter().map(|p| {
            let m = p["m"].as_u64().unwrap() as u32;
            if let Some(b) = p.get("box") { (Proxy::Box { center: v3(&b[0]), half: v3(&b[1]) }, m) } else { (Proxy::Hull(v3s(&p["hull"])), m) }
        }).collect(),
        lo: v3(&v["lo"]), hi: v3(&v["hi"]),
        convex: if v["convex"].is_null() { None } else { Some(Arc::new(v3s(&v["convex"]))) },
        roles: v["roles"].as_array().map_or(vec![], |a| a.iter().map(|x| x.as_u64().unwrap() as u8).collect()),
    }
}

fn obj_w(key: &str, o: &ObjState) -> Value {
    let o32 = &o.pose.o;
    json!({
        "key": key,
        "geom": geom_w(&o.geom),
        "pose": {"r": o.pose.r, "t": o.pose.t, "loc": o32.loc, "eul": o32.eul, "scale": o32.scale},
        "tags": tags_w(&o.tags),
        "gen": o.generator.as_ref().map(|g| json!({"class": g.class, "fs": g.factory_seed, "is": g.inst_seed, "unseeded": g.unseeded, "fit": g.fit_box, "size": g.size, "join": g.join})),
        "rels": o.relations.iter().map(|r| json!({"rel": rel_w(&r.relation), "target": r.target, "cp": r.child_plane, "pp": r.parent_plane})).collect::<Vec<_>>(),
        "dof_t": o.dof_t, "dof_r": o.dof_r, "active": o.active,
        "poly": o.polygon.as_ref().map(|g| (poly_w(key, g), shp::is_polygon(g))),
    })
}
fn obj_r(v: &Value) -> ObjState {
    let p = &v["pose"];
    let f32s = |v: &Value| v3(v).map(|x| x as f32); // exact: written from f32
    let mut o32 = crate::bl32::Obj32::new(f32s(&p["loc"]), f32s(&p["eul"]));
    o32.scale = f32s(&p["scale"]);
    let r = &p["r"];
    let pose = Pose { r: [v3(&r[0]), v3(&r[1]), v3(&r[2])], t: v3(&p["t"]), o: o32 };
    let mut o = ObjState::new(Arc::new(geom_r(&v["geom"])), pose, tags_r(&v["tags"]));
    o.generator = v["gen"].as_object().map(|g| GenRef {
        class: g["class"].as_str().unwrap().into(),
        factory_seed: g["fs"].as_u64().unwrap(),
        inst_seed: g["is"].as_u64().unwrap(),
        unseeded: g["unseeded"].as_array().map(|a| a.iter().map(f).collect()),
        fit_box: g["fit"].as_array().map(|a| (v3(&a[0]), v3(&a[1]))),
        size: g["size"].as_array().map(|_| v3(&g["size"])),
        join: g["join"].as_array().map(|a| [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])]),
        piece: None,
    });
    // a piece again from its class and seed, at the size it was saved at
    if let Some(g) = o.generator.as_mut() {
        if let Some(p) = ig_kit::piece::make_piece(&g.class, g.factory_seed) {
            let p = crate::furn::state::PieceRef::new(p);
            let p = match g.size { Some(x) if (0..3).any(|k| (p.size()[k] - x[k]).abs() > 1e-6) => p.resize(x).unwrap_or(p), _ => p };
            g.piece = Some(p);
        }
    }
    o.relations = v["rels"].as_array().unwrap().iter().map(|r| RelState { relation: rel_r(&r["rel"]), target: r["target"].as_str().unwrap().into(), child_plane: r["cp"].as_u64().unwrap() as usize, parent_plane: r["pp"].as_u64().unwrap() as usize }).collect();
    o.dof_t = v["dof_t"].as_array().map(|a| [v3(&a[0]), v3(&a[1]), v3(&a[2])]);
    o.dof_r = v["dof_r"].as_array().map(|_| v3(&v["dof_r"]));
    o.active = v["active"].as_bool().unwrap();
    o.polygon = v["poly"].as_array().and_then(|a| poly_r(&a[0], a[1].as_bool().unwrap()));
    o
}

fn layer_s(l: Layer) -> &'static str { match l { Layer::Plan => "plan", Layer::Room => "room", Layer::Furniture => "furniture", Layer::OnTop => "ontop" } }

pub fn save(path: &str, state: &State, complaints: &[Complaint]) {
    let t = std::time::Instant::now();
    let doc = json!({
        "objs": state.objs.iter().map(|(k, o)| obj_w(k, o)).collect::<Vec<_>>(),
        "complaints": complaints.iter().map(|c| json!({"layer": layer_s(c.layer), "room": c.room, "kind": c.kind, "at": c.at, "need": c.need, "hard": c.hard})).collect::<Vec<_>>(),
    });
    std::fs::write(path, serde_json::to_vec(&doc).unwrap()).unwrap();
    eprintln!("SNAPSHOT saved {} objects to {path} in {:.2}s", state.objs.len(), t.elapsed().as_secs_f64());
}

/// replaces `state`'s objects with the snapshot's (each touched, so derived-geometry caches rebuild); returns the complaints
pub fn load(path: &str, state: &mut State) -> Vec<Complaint> {
    let t = std::time::Instant::now();
    let doc: Value = serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("SOLVE_SNAPSHOT load {path}: {e}"))).unwrap();
    state.objs.clear();
    for v in doc["objs"].as_array().unwrap() {
        let k = v["key"].as_str().unwrap().to_string();
        state.objs.insert(k.clone(), obj_r(v));
        state.touch(&k);
    }
    let complaints = doc["complaints"].as_array().unwrap().iter().map(|c| Complaint {
        layer: match c["layer"].as_str().unwrap() { "plan" => Layer::Plan, "room" => Layer::Room, "furniture" => Layer::Furniture, _ => Layer::OnTop },
        room: c["room"].as_str().unwrap().into(),
        kind: Box::leak(c["kind"].as_str().unwrap().to_string().into_boxed_str()),
        at: c["at"].as_array().map(|a| [f(&a[0]), f(&a[1])]),
        need: c["need"].as_str().unwrap().into(),
        hard: c["hard"].as_bool().unwrap(),
    }).collect();
    eprintln!("SNAPSHOT loaded {} objects from {path} in {:.2}s (solve skipped)", state.objs.len(), t.elapsed().as_secs_f64());
    complaints
}
