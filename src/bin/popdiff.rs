use infinigen::assets::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let (fs, inst) = (j["fseed"].as_u64().unwrap(), j["inst"].as_u64().unwrap());
    let f = make_factory(a[2].as_str(), fs).unwrap();
    let ex = f.exact(inst).unwrap();
    let pre = ex.polys.len();
    let tr = ex.triangulated();
    let py: Vec<Vec<u32>> = j["polys"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()).collect();
    println!("pre-tri polys {} post {} py {}", pre, tr.polys.len(), py.len());
    let mut n = 0;
    for (i, (x, y)) in py.iter().zip(&tr.polys).enumerate() {
        if x != y { println!("diff at {i}: py {:?} rs {:?}", x, y); n += 1; if n > 6 { break; } }
    }
    // which pre-triangulation faces are quads
    let quads: Vec<usize> = ex.polys.iter().enumerate().filter(|(_, p)| p.len() != 3).map(|(i, _)| i).collect();
    println!("quads: {} first {:?} last {:?}", quads.len(), quads.first(), quads.last());
    for &q in quads.iter().take(30) { let p = &ex.polys[q]; let vs: Vec<_> = p.iter().map(|&i| ex.verts[i as usize]).collect(); println!("quad {q} {:?} {:?}", p, vs); }
}
