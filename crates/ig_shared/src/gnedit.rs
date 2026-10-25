//! JSON-level surgery on exported node groups (data/gn/*.json): patch constants, prune multi-input links, find upstream blocks, evaluate.
use crate::gn;
use crate::gn::geom::Geometry as GGeom;
use crate::gn::val::Ty;
use serde_json::{json, Value as J};
use crate::dhash::{HashMap, HashSet};

pub fn node_mut<'a>(j: &'a mut J, group: &str, node: &str) -> &'a mut J {
    j["groups"][group]["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["name"].as_str() == Some(node)).unwrap_or_else(|| panic!("node {} not in {}", node, group))
}

pub fn set_in(j: &mut J, group: &str, node: &str, id: &str, v: J) {
    let n = node_mut(j, group, node);
    for s in n["in"].as_array_mut().unwrap() {
        if s["id"].as_str() == Some(id) {
            s["v"] = v;
            return;
        }
    }
    panic!("socket {} not found on {}/{}", id, group, node);
}

pub fn set_curve_y(j: &mut J, group: &str, node: &str, ys: &[f64]) {
    let n = node_mut(j, group, node);
    let pts = n["p"]["curve"]["pts"].as_array_mut().unwrap();
    for (p, y) in pts.iter_mut().zip(ys) {
        p[1] = json!(*y);
    }
}

pub fn set_prop(j: &mut J, group: &str, node: &str, key: &str, v: J) {
    node_mut(j, group, node)["p"][key] = v;
}

/// referenced group of a Group node
pub fn group_of(j: &J, group: &str, node: &str) -> String {
    j["groups"][group]["nodes"].as_array().unwrap().iter().find(|n| n["name"].as_str() == Some(node)).and_then(|n| n["p"]["group"].as_str()).unwrap_or("").to_string()
}

pub fn nodes_of_type(j: &J, group: &str, ty: &str) -> Vec<String> {
    j["groups"][group]["nodes"].as_array().unwrap().iter().filter(|n| n["type"].as_str() == Some(ty)).map(|n| n["name"].as_str().unwrap().to_string()).collect()
}

/// source node names of the links into (node, socket), ordered by sort id
pub fn sources_into(j: &J, group: &str, node: &str, sock: &str) -> Vec<String> {
    let mut v: Vec<(i64, String)> = j["groups"][group]["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["tn"].as_str() == Some(node) && l["ts"].as_str() == Some(sock))
        .map(|l| (l["sort"].as_i64().unwrap_or(0), l["fn"].as_str().unwrap().to_string()))
        .collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

/// keep only the first `keep` links (by sort id) into (node, socket)
pub fn prune_multi(j: &mut J, group: &str, node: &str, sock: &str, keep: usize) {
    let srcs = sources_into(j, group, node, sock);
    let drop: HashSet<String> = srcs.into_iter().skip(keep).collect();
    let links = j["groups"][group]["links"].as_array_mut().unwrap();
    links.retain(|l| !(l["tn"].as_str() == Some(node) && l["ts"].as_str() == Some(sock) && drop.contains(l["fn"].as_str().unwrap())));
}

pub fn upstream(j: &J, group: &str, node: &str) -> HashSet<String> {
    let links = j["groups"][group]["links"].as_array().unwrap();
    let mut seen: HashSet<String> = HashSet::default();
    let mut stack = vec![node.to_string()];
    while let Some(n) = stack.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        for l in links {
            if l["tn"].as_str() == Some(&n) {
                stack.push(l["fn"].as_str().unwrap().to_string());
            }
        }
    }
    seen
}

pub fn node_type(j: &J, group: &str, node: &str) -> String {
    j["groups"][group]["nodes"].as_array().unwrap().iter().find(|n| n["name"].as_str() == Some(node)).map(|n| n["type"].as_str().unwrap().to_string()).unwrap_or_default()
}

/// evaluate `group` (no inputs) and return its first geometry output
pub fn eval_geo(j: J, group: &str) -> GGeom {
    let groups = gn::load::load_groups(j);
    let objects = gn::Objects::default();
    let outs = gn::eval(&groups, group, &HashMap::default(), &objects).expect("group eval");
    let g = &groups.map[group];
    let gi = g.outputs.iter().position(|o| o.ty == Ty::Geo).expect("no geometry output");
    match &outs[gi].1 {
        gn::Value::G(gg) => (**gg).clone(),
        _ => panic!("output is not geometry"),
    }
}
