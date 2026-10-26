//! gndbg FILE.json GROUP NODE.. SOCKET : evaluate a node output and print a summary of the geometry
use indoorgen::gn::*;
use indoorgen::dhash::HashMap;
fn summ(g: &Geometry, ind: usize) {
    let p = " ".repeat(ind);
    if let Some(m) = &g.mesh {
        println!("{}mesh v={} f={} v0={:?} v202={:?}", p, m.nv(), m.nf(), m.pos.get(0), m.pos.get(202));
    }
    if let Some(c) = &g.curves {
        println!("{}curves n={} pts={}", p, c.size(Dom::Curve), c.size(Dom::Point));
    }
    if let Some(i) = &g.inst {
        println!("{}instances n={} refs={} idx(first)={:?}", p, i.n(), i.refs.len(), i.idx.iter().take(8).collect::<Vec<_>>());
        for k in 0..i.n().min(3) {
            println!("{} xf[{}] = {:?}", p, k, i.xf[k]);
        }
        for (k, r) in i.refs.iter().enumerate().take(7) {
            println!("{} ref {}:", p, k);
            summ(r, ind + 4);
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let groups = load_file(&a[1]).unwrap();
    let path: Vec<String> = a[3..a.len() - 1].to_vec();
    let v = eval_probe(&groups, &a[2], &HashMap::default(), &Objects::default(), &path, &a[a.len() - 1]).unwrap();
    match v {
        Value::G(g) => {
            summ(&g, 0);
            if let Ok(path) = std::env::var("GNDUMP") {
                let r = indoorgen::gn::meshops::realize_instances(&g, 1000);
                if let Some(m) = &r.mesh {
                    let v: Vec<[f32; 3]> = m.pos.clone();
                    std::fs::write(path, serde_json::to_string(&v).unwrap()).unwrap();
                }
            }
        }
        _ => println!("not geometry"),
    }
}
