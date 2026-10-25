//! Genome of CarnivoreFactory (tiger_genome) / HerbivoreFactory (herbivore_genome): part parameter sampling and the attachment tree, in numpy draw order.
use super::creature_nurbs::*;
use super::creature_rng::*;
use ig_core::rng::Rng;
use serde_json::{json, Value as J};

#[derive(Clone, Debug)]
pub enum Kind {
    NurbsBody(NurbsParams),
    NurbsHead(NurbsParams),
    Tail,
    BackLeg,
    FrontLeg,
    Foot,
    CarnivoreHead,
    Jaw,
    Eye,
    CatNose,
    CatEar,
    HoofAnkle,
    HoofClaw,
    Horn,
}

/// a PartFactory with its (shared) sampled parameters
#[derive(Clone, Debug)]
pub struct Fac {
    pub kind: Kind,
    /// group input name -> value for the node-group parts (python `params` dict)
    pub params: J,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Basis {
    Global,
    Normal,
}

#[derive(Clone, Debug)]
pub struct Att {
    pub coord: [f64; 3],
    pub rest: [f64; 3],
    pub side: i32,
    pub basis: Basis,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub fac: usize,
    pub att: Option<Att>,
    pub children: Vec<usize>,
}

#[derive(Default, Debug)]
pub struct Genome {
    pub facs: Vec<Fac>,
    pub nodes: Vec<Node>,
}

impl Genome {
    fn fac(&mut self, f: Fac) -> usize {
        self.facs.push(f);
        self.facs.len() - 1
    }
    /// genome.part(fac)
    fn part(&mut self, fac: usize) -> usize {
        self.nodes.push(Node { fac, att: None, children: vec![] });
        self.nodes.len() - 1
    }
    /// genome.attach(child, parent, ...) returns parent
    fn attach(&mut self, child: usize, parent: usize, coord: [f64; 3], rest: [f64; 3], side: i32, basis: Basis) -> usize {
        self.nodes[child].att = Some(Att { coord, rest, side, basis });
        self.nodes[parent].children.push(child);
        parent
    }
    /// preorder traversal (tree.iter_nodes)
    pub fn preorder(&self, root: usize) -> Vec<usize> {
        let mut out = vec![];
        fn rec(g: &Genome, n: usize, out: &mut Vec<usize>) {
            out.push(n);
            for &c in &g.nodes[n].children {
                rec(g, c, out);
            }
        }
        rec(self, root, &mut out);
        out
    }
    /// (parent, child) pairs in postorder (tree.iter_parent_child(postorder=True)), root last with parent None
    pub fn postorder_pairs(&self, root: usize) -> Vec<(Option<usize>, usize)> {
        let mut out = vec![];
        fn rec(g: &Genome, n: usize, parent: Option<usize>, out: &mut Vec<(Option<usize>, usize)>) {
            for &c in &g.nodes[n].children {
                rec(g, c, Some(n), out);
            }
            out.push((parent, n));
        }
        rec(self, root, None, &mut out);
        out
    }
}

fn v3(a: [f64; 3]) -> J {
    json!([a[0], a[1], a[2]])
}
fn mul3(a: [f64; 3], b: &[f64]) -> [f64; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn n3(r: &mut Rng, std: f64) -> Vec<f64> {
    normal_n(r, 1.0, std, 3)
}
fn n3s(r: &mut Rng, stds: [f64; 3]) -> Vec<f64> {
    normal_each(r, 1.0, &stds)
}

// ------------------------------------------------------------------------------------------------ part factories (sample_params)

fn tail(r: &mut Rng) -> Fac {
    let l = r.normal(0.5, 0.1);
    let a = r.normal(1.0, 0.1);
    let asp = r.normal(1.0, 0.05);
    Fac { kind: Kind::Tail, params: json!({"length_rad1_rad2": [l, 0.05, 0.02], "angles_deg": [31.39 * a, 65.81 * a, -106.93 * a], "aspect": asp}) }
}

fn carnivore_head(r: &mut Rng, lrr_override: Option<[f64; 3]>) -> Fac {
    let l = mul3([0.36, 0.20, 0.18], &n3(r, 0.2));
    let s = mul3([0.22, 0.15, 0.15], &n3(r, 0.2));
    let asp = r.normal(1.0, 0.2);
    // muscle_params: the noise is drawn into a copy (`v *= N(...)` acts on np.array(v)) and params.update(muscle_params) restores the originals
    let muscle: [(&str, [f64; 3]); 10] = [
        ("Nose Bridge Scale", [1.0, 0.35, 0.9]),
        ("Jaw Muscle Middle Coord", [0.24, 0.41, 1.3]),
        ("Jaw StartRad, EndRad, Fullness", [0.06, 0.11, 1.5]),
        ("Jaw ProfileHeight, StartTilt, EndTilt", [0.8, 33.1, 0.0]),
        ("Lip Muscle Middle Coord", [0.95, 0.0, 1.5]),
        ("Lip StartRad, EndRad, Fullness", [0.05, 0.09, 1.48]),
        ("Lip ProfileHeight, StartTilt, EndTilt", [0.8, 0.0, -17.2]),
        ("Forehead Muscle Middle Coord", [0.7, -1.32, 1.31]),
        ("Forehead StartRad, EndRad, Fullness", [0.06, 0.05, 2.5]),
        ("Forehead ProfileHeight, StartTilt, EndTilt", [0.3, 60.6, 66.0]),
    ];
    for _ in 0..muscle.len() {
        n3(r, 0.05);
    }
    let eye_rad = 0.023 * r.normal(1.0, 0.3);
    let eo = normal_each(r, 0.0, &[0.0, 0.02, 0.03]);
    let mut p = json!({"length_rad1_rad2": v3(l), "snout_length_rad1_rad2": v3(s), "aspect": asp, "EyeRad": eye_rad, "EyeOffset": [-0.25 + eo[0], 0.45 + eo[1], 0.3 + eo[2]]});
    for (k, v) in muscle {
        p[k] = v3(v);
    }
    if let Some(o) = lrr_override {
        p["length_rad1_rad2"] = v3(o);
    }
    Fac { kind: Kind::CarnivoreHead, params: p }
}

fn jaw(r: &mut Rng, lrr_override: Option<[f64; 3]>, canine: Option<f64>) -> Fac {
    let l = mul3([0.4, 0.12, 0.08], &n3(r, 0.1));
    let ws = 1.0 * clip_gaussian(r, 1.0, 0.1, 0.5, 1.0);
    let cl = 0.05 * r.normal(1.0, 0.2);
    let inc = 0.01 * r.normal(1.0, 0.2);
    let crook = 1.2 * r.normal(1.0, 0.3);
    let ts = 1.0 * clip_gaussian(r, 1.0, 0.1, 0.5, 1.0);
    let tx = 0.9 * clip_gaussian(r, 1.0, 0.1, 0.5, 1.0);
    let mut p = json!({"length_rad1_rad2": v3(l), "Width Shaping": ws, "Canine Length": cl, "Incisor Size": inc, "Tooth Crookedness": crook, "Tongue Shaping": ts, "Tongue X Scale": tx});
    if let Some(o) = lrr_override {
        p["length_rad1_rad2"] = v3(o);
    }
    if let Some(c) = canine {
        p["Canine Length"] = json!(c);
    }
    Fac { kind: Kind::Jaw, params: p }
}

fn mammal_eye(r: &mut Rng, radius_override: f64) -> Fac {
    let rad = 0.03 * r.normal(1.0, 0.1);
    let th = 0.35 * r.normal(1.0, 0.05);
    let fu = 2.0 * r.normal(1.0, 0.1);
    let _ = rad;
    Fac { kind: Kind::Eye, params: json!({"Radius": radius_override, "Eyelid Thickness Ratio": th, "Eyelid Fullness": fu}) }
}

fn cat_nose(r: &mut Rng) -> Fac {
    let sm = r.normal(0.7, 0.05);
    let ns = 0.03 * sm * r.normal(1.0, 0.1);
    let cr = 0.237 * r.normal(1.0, 0.1);
    Fac { kind: Kind::CatNose, params: json!({"Nose Radius": 0.11 * sm, "Nostril Size": ns, "Crease": cr}) }
}

fn cat_ear(r: &mut Rng) -> Fac {
    let _size = clip_gaussian(r, 1.0, 0.1, 0.2, 5.0);
    let l = mul3([0.25, 0.1, 0.0], &normal_each(r, 1.0, &[0.1, 0.05, 0.05]));
    let depth = 0.06 * r.normal(1.0, 0.1);
    let curl = 49.0 * r.normal(1.0, 0.2);
    Fac { kind: Kind::CatEar, params: json!({"length_rad1_rad2": v3(l), "Depth": depth, "Thickness": 0.01, "Curl Deg": curl}) }
}

fn foot(r: &mut Rng) -> Fac {
    let l = mul3([0.27, 0.04, 0.09], &n3s(r, [0.2, 0.05, 0.05]));
    let toes = (r.normal(4.0, 1.0).trunc() as i64).max(2);
    let tl = mul3([0.3, 0.045, 0.025], &n3(r, 0.1));
    let trot = -r.normal(0.7, 0.15);
    let splay = 20.0 * r.normal(1.0, 0.2);
    let bean = 0.03 * r.normal(1.0, 0.2);
    let curl = 30.0 * r.normal(1.0, 0.4);
    let cp = mul3([0.3, 0.5, 0.0], &n3(r, 0.1));
    Fac {
        kind: Kind::Foot,
        params: json!({"length_rad1_rad2": v3(l), "Num Toes": toes, "Toe Length Rad1 Rad2": v3(tl), "Toe Rotate": [0.0, trot, 0.0], "Toe Splay": splay, "Toebean Radius": bean, "Claw Curl Deg": curl, "Claw Pct Length Rad1 Rad2": v3(cp)}),
    }
}

fn add3(a: [f64; 3], b: &[f64]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn back_leg(r: &mut Rng, lrr_override: Option<[f64; 3]>) -> Fac {
    let l = mul3([1.8, 0.1, 0.05], &n3s(r, [0.2, 0.0, 0.0]));
    let th = mul3([0.33, 0.15, 2.5], &n3(r, 0.1));
    let ca = mul3([0.17, 0.07, 2.5], &n3(r, 0.1));
    let thh = add3([0.6, 0.0, 0.0], &normal_each(r, 0.0, &[0.05, 2.0, 10.0]));
    let cah = add3([0.8, 0.0, 0.0], &normal_each(r, 0.0, &[0.05, 10.0, 10.0]));
    let l = lrr_override.unwrap_or(l);
    Fac {
        kind: Kind::BackLeg,
        params: json!({"length_rad1_rad2": v3(l), "angles_deg": [40.0, -120.0, 100.0], "fullness": 50.0, "aspect": 1.0, "Thigh Rad1 Rad2 Fullness": v3(th), "Calf Rad1 Rad2 Fullness": v3(ca),
            "Thigh Height Tilt1 Tilt2": v3(thh), "Calf Height Tilt1 Tilt2": v3(cah)}),
    }
}

fn front_leg(r: &mut Rng, lrr_override: Option<[f64; 3]>) -> Fac {
    let l = mul3([1.43, 0.1, 0.1], &n3s(r, [0.2, 0.0, 0.0]));
    let sh = mul3([0.22, 0.22, 2.5], &n3(r, 0.1));
    let ca = mul3([0.08, 0.08, 2.5], &n3(r, 0.1));
    let el = mul3([0.12, 0.1, 2.5], &n3(r, 0.1));
    let shh = add3([0.74, 0.0, 0.0], &normal_each(r, 0.0, &[0.05, 10.0, 10.0]));
    let elh = add3([0.9, 0.0, 0.0], &normal_each(r, 0.0, &[0.05, 10.0, 10.0]));
    let cah = add3([0.74, 0.0, 0.0], &normal_each(r, 0.0, &[0.05, 10.0, 10.0]));
    let l = lrr_override.unwrap_or(l);
    Fac {
        kind: Kind::FrontLeg,
        params: json!({"length_rad1_rad2": v3(l), "angles_deg": [-40.0, 120.0, -100.0], "aspect": 1.0, "Shoulder Rad1 Rad2 Fullness": v3(sh), "Calf Rad1 Rad2 Fullness": v3(ca),
            "Elbow Rad1 Rad2 Fullness": v3(el), "Shoulder Height, Tilt1, Tilt2": v3(shh), "Elbow Height, Tilt1, Tilt2": v3(elh), "Calf Height, Tilt1, Tilt2": v3(cah)}),
    }
}

// ------------------------------------------------------------------------------------------------ CarnivoreFactory

pub fn tiger_genome(r: &mut Rng) -> (Genome, usize) {
    let mut g = Genome::default();
    let mut bp = sample_nurbs(r, "body_feline", &|_| 0.2, 0.7);
    let k = bp.thetas.len();
    bp.thetas[k - 3] *= r.normal(1.0, 0.1);
    let f = g.fac(Fac { kind: Kind::NurbsBody(bp), params: J::Null });
    let body = g.part(f);

    let f = tail(r);
    let f = g.fac(f);
    let tailn = g.part(f);
    let rest0 = r.normal(0.0, 10.0);
    g.attach(tailn, body, [0.07, 1.0, 1.0], [rest0, 180.0, 0.0], 1, Basis::Global);

    let head;
    if r.f64() < 0.5 {
        let hl = mul3([0.36, 0.20, 0.18], &n3(r, 0.1));
        let hf = carnivore_head(r, Some(hl));
        let hf = g.fac(hf);
        head = g.part(hf);
        let jp = [1.05, 0.55, 0.5];
        let jf = jaw(r, Some([hl[0] * jp[0], hl[1] * jp[1], hl[2] * jp[2]]), None);
        let jf = g.fac(jf);
        let jawn = g.part(jf);
        let c0 = 0.2 * r.normal(1.0, 0.1);
        let c2 = 0.35 * r.normal(1.0, 0.1);
        let rr = r.uniform(10.0, 35.0);
        g.attach(jawn, head, [c0, 0.0, c2], [0.0, rr, 0.0], 1, Basis::Global);
    } else {
        let hp = sample_nurbs(r, "head_carnivore", &|_| 0.3, 0.5);
        let headl = hp.length;
        let hf = g.fac(Fac { kind: Kind::NurbsHead(hp), params: J::Null });
        head = g.part(hf);
        let hl = mul3([headl, 0.20, 0.18], &n3(r, 0.1));
        let jp = [0.7, 0.55, 0.5];
        let jf = jaw(r, Some([hl[0] * jp[0], hl[1] * jp[1], hl[2] * jp[2]]), None);
        let jf = g.fac(jf);
        let jawn = g.part(jf);
        let c2 = 0.3 * r.normal(1.0, 0.1);
        let rr = r.uniform(10.0, 35.0);
        g.attach(jawn, head, [0.12, 0.0, c2], [0.0, rr, 0.0], 1, Basis::Global);

        let rad = r.normal(0.027, 0.009);
        let ef = mammal_eye(r, rad);
        let ef = g.fac(ef);
        let eye_t = r.uniform(0.61, 0.64);
        let splay = r.uniform(90.0, 140.0) / 180.0;
        let rr = r.uniform(0.8, 0.9);
        for side in [-1, 1] {
            let e = g.part(ef);
            g.attach(e, head, [eye_t, splay, rr], [0.0, 0.0, 0.0], side, Basis::Normal);
        }
    }

    let nf = cat_nose(r);
    let nf = g.fac(nf);
    let nose = g.part(nf);
    let a = r.uniform(0.9, 0.96);
    let c2 = r.uniform(0.5, 0.7);
    g.attach(nose, head, [a, 1.0, c2], [0.0, 20.0, 0.0], 1, Basis::Global);

    let ef = cat_ear(r);
    let ef = g.fac(ef);
    let t = r.normal(0.33, 0.07);
    let splay = r.uniform(100.0, 150.0) / 180.0;
    let rn = normal_n(r, 0.0, 4.0, 3);
    let rot = [-20.0 + rn[0], -10.0 + rn[1], -23.0 + rn[2]];
    for side in [-1, 1] {
        let e = g.part(ef);
        g.attach(e, head, [t, splay, 1.0], rot, side, Basis::Normal);
    }

    let neck_t = 0.7;
    let splay = clip_gaussian(r, 130.0, 7.0, 90.0, 130.0) / 180.0;
    let shoulder_t = clip_gaussian(r, 0.12, 0.05, 0.08, 0.12);
    let lrr = mul3([1.6, 0.1, 0.05], &n3s(r, [0.15, 0.05, 0.05]));

    let ff = foot(r);
    let ff = g.fac(ff);
    let bf = back_leg(r, Some(lrr));
    let bf = g.fac(bf);
    for side in [-1, 1] {
        let fo = g.part(ff);
        let le = g.part(bf);
        let leg = g.attach(fo, le, [0.9, 0.0, 0.0], [0.0, 0.0, 0.0], 1, Basis::Global);
        g.attach(leg, body, [shoulder_t, splay, 1.2], [0.0, 90.0, 0.0], side, Basis::Global);
    }
    let frf = front_leg(r, Some(lrr));
    let frf = g.fac(frf);
    for side in [-1, 1] {
        let fo = g.part(ff);
        let le = g.part(frf);
        let leg = g.attach(fo, le, [0.9, 0.0, 0.0], [0.0, 0.0, 0.0], 1, Basis::Global);
        g.attach(leg, body, [neck_t - shoulder_t, splay, 0.8], [0.0, 90.0, 0.0], side, Basis::Global);
    }
    let c0 = r.normal(0.97, 0.01);
    let r1 = r.normal(20.0, 5.0);
    g.attach(head, body, [c0, 0.0, 0.0], [0.0, r1, 0.0], 1, Basis::Global);
    (g, body)
}

// ------------------------------------------------------------------------------------------------ HerbivoreFactory

fn hoof_ankle(r: &mut Rng) -> Fac {
    let l = [0.45 * r.normal(1.0, 0.05), 0.07 * r.normal(1.0, 0.05), 0.1 * r.normal(1.0, 0.05)];
    let a = [-90.0 + r.normal(0.0, 5.0), 40.0 + r.normal(0.0, 5.0), r.normal(0.0, 5.0)];
    Fac {
        kind: Kind::HoofAnkle,
        params: json!({"length_rad1_rad2": v3(l), "angles_deg": v3(a), "aspect": 1.0, "Upper Rad1 Rad2 Fullness": [0.2, 0.0, 4.0], "Lower Rad1 Rad2 Fullness": [0.15, 0.0, 4.0], "Height, Tilt1, Tilt2": [1.0, 0.0, 0.0]}),
    }
}

fn hoof_claw(r: &mut Rng) -> Fac {
    let sx = 0.1 * r.normal(1.0, 0.05);
    let sy = 0.1 * r.normal(1.0, 0.05);
    let sz = 0.08 * r.normal(1.0, 0.05);
    let rr = 0.5 + r.normal(0.0, 1.0);
    Fac { kind: Kind::HoofClaw, params: json!({"n": 20, "m": 20, "sx": sx, "sy": sy, "sz": sz, "r": rr}) }
}

const HORN_KEYS: [&str; 8] = ["length", "rad1", "rad2", "thickness", "density_of_ridge", "depth_of_ridge", "height", "rotation_x"];
const HORN_TEMPLATES: [[f64; 8]; 3] = [
    [0.1, 0.5, 0.1, 0.1, 150.0, 0.01, -0.1, -1.0],  // bull
    [0.4, 0.7, 0.5, 0.1, 150.0, 0.1, 0.1, 0.0],     // gazelle
    [0.5, 0.18, 0.3, 0.15, 250.0, 0.02, 0.1, 0.0],   // goat
];
const HORN_RANGE: [[f64; 2]; 8] = [[0.1, 0.6], [0.1, 1.0], [0.1, 1.0], [0.05, 0.3], [100.0, 300.0], [0.01, 0.1], [-0.3, 0.3], [-1.0, 1.0]];

fn horn(r: &mut Rng) -> Fac {
    let mut w = dirichlet(r, &[1.0, 1.0, 1.0]);
    let norm = w[0] + w[1] + w[2];
    for x in w.iter_mut() {
        *x /= norm;
    }
    let mut p = serde_json::Map::new();
    for (i, k) in HORN_KEYS.iter().enumerate() {
        let mut v = 0.0 + HORN_TEMPLATES[0][i] * w[0];
        v += HORN_TEMPLATES[1][i] * w[1];
        v += HORN_TEMPLATES[2][i] * w[2];
        let (l, h) = (HORN_RANGE[i][0], HORN_RANGE[i][1]);
        v += r.normal(0.0, 0.02 * (h - l));
        p.insert(k.to_string(), json!(v));
    }
    Fac { kind: Kind::Horn, params: J::Object(p) }
}

pub fn herbivore_genome(r: &mut Rng) -> (Genome, usize) {
    let mut g = Genome::default();
    let bp = sample_nurbs(r, "body_herbivore", &|k| match k {
        "body_herbivore_giraffe" => 0.02,
        "body_herbivore_llama" => 0.1,
        _ => 0.2,
    }, 1.0);
    let f = g.fac(Fac { kind: Kind::NurbsBody(bp), params: J::Null });
    let body = g.part(f);

    let neck_t = 0.67;
    let splay = clip_gaussian(r, 130.0, 7.0, 90.0, 130.0) / 180.0;
    let shoulder_t = clip_gaussian(r, 0.1, 0.05, 0.05, 0.2);
    let mut lrr = mul3([1.8, 0.1, 0.05], &n3s(r, [0.1, 0.05, 0.05]));

    let leg_rest = [0.0, 90.0, 0.0];
    let foot_rest = [0.0, -90.0, 0.0];
    let ff = hoof_ankle(r);
    let ff = g.fac(ff);
    let cf = hoof_claw(r);
    let cf = g.fac(cf);
    let bf = back_leg(r, Some(lrr));
    let bfi = g.fac(bf);
    let frf = front_leg(r, Some(lrr));
    let frfi = g.fac(frf);
    if r.f64() < 0.15 {
        let lenscale = r.uniform(1.0, 1.3);
        // both leg factories share the `params` array: the in-place `[0] *= lenscale` is applied twice
        let l0 = lrr[0] * lenscale * lenscale;
        for fi in [bfi, frfi] {
            g.facs[fi].params["length_rad1_rad2"][0] = json!(l0);
        }
        lrr[0] = l0;
    }

    for side in [-1, 1] {
        let claw = g.part(cf);
        let ank = g.part(ff);
        let foot = g.attach(claw, ank, [0.7, -1.0, 0.0], [0.0, 90.0, 0.0], 1, Basis::Global);
        let leg = g.part(bfi);
        let back = g.attach(foot, leg, [0.95, 1.0, 0.2], foot_rest, 1, Basis::Global);
        g.attach(back, body, [shoulder_t, splay, 1.0], leg_rest, side, Basis::Global);
    }
    for side in [-1, 1] {
        let claw = g.part(cf);
        let ank = g.part(ff);
        let foot = g.attach(claw, ank, [0.7, 1.0, 0.0], [0.0, 90.0, 0.0], 1, Basis::Normal);
        let leg = g.part(frfi);
        let front = g.attach(foot, leg, [0.95, 0.0, 0.5], [0.0, -70.0, 0.0], 1, Basis::Global);
        g.attach(front, body, [neck_t - shoulder_t, splay + 0.0 / 180.0, 0.9], leg_rest, side, Basis::Global);
    }

    let hp = sample_nurbs(r, "head_herbivore", &|_| 0.2, 0.5);
    let head_len = hp.length;
    let hf = g.fac(Fac { kind: Kind::NurbsHead(hp), params: J::Null });
    let head = g.part(hf);

    let rad = r.normal(0.035, 0.01);
    let ef = mammal_eye(r, rad);
    let ef = g.fac(ef);
    let eye_t = r.uniform(0.34, 0.45);
    let esplay = r.uniform(80.0, 140.0) / 180.0;
    let rr = r.uniform(0.7, 0.9);
    for side in [-1, 1] {
        let e = g.part(ef);
        g.attach(e, head, [eye_t, esplay, rr], [0.0, 0.0, 0.0], side, Basis::Normal);
    }

    let jf = jaw(r, Some([0.6 * head_len, 0.12, 0.08]), Some(0.0));
    let jf = g.fac(jf);
    let jawn = g.part(jf);
    let c0 = 0.25 * r.normal(1.0, 0.1);
    let c2 = 0.35 * r.normal(1.0, 0.1);
    let rr = 10.0 * r.normal(1.0, 0.1);
    g.attach(jawn, head, [c0, 0.0, c2], [0.0, rr, 0.0], 1, Basis::Global);

    if r.f64() < 0.7 {
        let nf = cat_nose(r);
        let nf = g.fac(nf);
        let nose = g.part(nf);
        g.attach(nose, head, [0.95, 1.0, 0.45], [0.0, 20.0, 0.0], 1, Basis::Global);
    }

    let t = r.uniform(0.15, eye_t - 0.07);
    let splay = r.normal(125.0, 15.0) / 180.0;
    let mut ef = cat_ear(r);
    let sc = normal_n(r, 1.2, 0.1, 3);
    {
        let l = ef.params["length_rad1_rad2"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
        ef.params["length_rad1_rad2"] = v3([l[0] * sc[0], l[1] * sc[1], l[2] * sc[2]]);
    }
    let ef = g.fac(ef);
    let rn = n3(r, 0.1);
    let rot = [0.0 * rn[0], -10.0 * rn[1], -23.0 * rn[2]];
    for side in [-1, 1] {
        let e = g.part(ef);
        g.attach(e, head, [t, splay, 1.0], rot, side, Basis::Normal);
    }

    if r.f64() < 0.7 {
        let mut hf = horn(r);
        let m1 = r.uniform(0.1, 2.0);
        let m2 = r.uniform(0.07, 1.5);
        let m3 = r.uniform(0.07, 1.5);
        for (k, m) in [("length", m1), ("rad1", m2), ("rad2", m3)] {
            let v = hf.params[k].as_f64().unwrap() * m;
            hf.params[k] = json!(v);
        }
        let hfi = g.fac(hf);
        let t2 = r.uniform(0.25, t);
        let splay2 = r.uniform(splay + 20.0 / 180.0, 130.0 / 180.0);
        let rx = r.uniform(-40.0, 0.0);
        let rz = r.normal(120.0, 10.0);
        for side in [-1, 1] {
            let h = g.part(hfi);
            g.attach(h, head, [t2, splay2, 0.5], [rx, 0.0, rz], side, Basis::Global);
        }
    } else {
        let _ = r.f64(); // `elif U() < 0:` never true but draws
    }

    g.attach(head, body, [0.97, 0.0, 0.2], [0.0, 20.0, 0.0], 1, Basis::Global);
    (g, body)
}
