//! Linear-blend skinning on the Rocketbox (3ds Max Biped) skeleton. Bone transforms are expressed as `G_i = G_parent * about(pivot_i, Q_i)`
//! where `about(p, Q)` rotates around the REST pivot `p` and `Q` is given in rest-world (body frame) axes; the skin matrix of a bone is then
//! simply `G_i` (the mesh is in bind pose, so `W_i * offset_i = I`). Body frame: x = avatar's right, y = forward, z = up.
use crate::avatar::{AvPart, Avatar};
use std::collections::HashMap;

pub type R3 = [[f64; 3]; 3];
pub type V = [f64; 3];

#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub r: R3,
    pub t: V,
}
pub const I3: R3 = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
impl Xf {
    pub fn ident() -> Xf {
        Xf { r: I3, t: [0.0; 3] }
    }
    pub fn mul(&self, o: &Xf) -> Xf {
        Xf { r: rmul(&self.r, &o.r), t: add(rapply(&self.r, o.t), self.t) }
    }
    pub fn apply(&self, p: V) -> V {
        add(rapply(&self.r, p), self.t)
    }
    /// rotate by q around pivot p
    pub fn about(p: V, q: &R3) -> Xf {
        Xf { r: *q, t: sub(p, rapply(q, p)) }
    }
}
pub fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V) -> V {
    let l = dot(a, a).sqrt().max(1e-12);
    scale(a, 1.0 / l)
}
pub fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
pub fn rmul(a: &R3, b: &R3) -> R3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}
pub fn rapply(r: &R3, v: V) -> V {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}
pub fn rtrans(r: &R3) -> R3 {
    [[r[0][0], r[1][0], r[2][0]], [r[0][1], r[1][1], r[2][1]], [r[0][2], r[1][2], r[2][2]]]
}
pub fn rot_axis(axis: V, deg: f64) -> R3 {
    let a = norm(axis);
    let (s, c) = deg.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [c + a[0] * a[0] * t, a[0] * a[1] * t - a[2] * s, a[0] * a[2] * t + a[1] * s],
        [a[1] * a[0] * t + a[2] * s, c + a[1] * a[1] * t, a[1] * a[2] * t - a[0] * s],
        [a[2] * a[0] * t - a[1] * s, a[2] * a[1] * t + a[0] * s, c + a[2] * a[2] * t],
    ]
}
/// R = Rz * Ry * Rx (degrees): x first
pub fn rot_euler(e: V) -> R3 {
    rmul(&rot_axis([0., 0., 1.], e[2]), &rmul(&rot_axis([0., 1., 0.], e[1]), &rot_axis([1., 0., 0.], e[0])))
}
/// shortest rotation taking unit a to unit b
pub fn arc(a: V, b: V) -> R3 {
    let (a, b) = (norm(a), norm(b));
    let c = dot(a, b);
    if c > 1.0 - 1e-12 {
        return I3;
    }
    if c < -1.0 + 1e-9 {
        let ax = if a[0].abs() < 0.9 { cross(a, [1., 0., 0.]) } else { cross(a, [0., 1., 0.]) };
        return rot_axis(ax, 180.0);
    }
    let v = cross(a, b);
    let ang = c.clamp(-1.0, 1.0).acos().to_degrees();
    rot_axis(v, ang)
}

/// What a pose does to one bone.
#[derive(Clone, Debug)]
pub enum Op {
    /// extra rotation (euler degrees, body axes) relative to what the parents did
    Rel(V),
    /// absolute world orientation of the bone (euler degrees, body axes), independent of the parents
    World(V),
    /// make the bone point at its main child along world direction `dir`, with `twist` degrees about the bone axis first
    Aim { dir: V, twist: f64 },
    /// two-bone IK (UpperArm / Thigh): put the end effector (Hand / Foot pivot) on `target`, elbow/knee towards `pole`
    Ik { target: V, pole: V },
}

pub fn main_child(name: &str) -> Option<String> {
    let m = [("UpperArm", "Forearm"), ("Forearm", "Hand"), ("Thigh", "Calf"), ("Calf", "Foot"), ("Foot", "Toe0")];
    for (a, b) in m {
        if let Some(pre) = name.strip_suffix(a) {
            return Some(format!("{pre}{b}"));
        }
    }
    None
}

pub struct Rig<'a> {
    pub av: &'a Avatar,
    idx: HashMap<&'a str, usize>,
    /// parents with thighs re-parented to the pelvis and clavicles to Spine2 (Rocketbox hangs them under Spine / Neck, which makes spine and
    /// neck bends drag hips and shoulders)
    pub parents: Vec<i32>,
}
impl<'a> Rig<'a> {
    pub fn new(av: &'a Avatar) -> Rig<'a> {
        let idx: HashMap<&'a str, usize> = av.bones.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
        let mut parents: Vec<i32> = av.bones.iter().map(|b| b.parent).collect();
        for (i, b) in av.bones.iter().enumerate() {
            let np = if b.name.ends_with("Thigh") { idx.get("Bip01 Pelvis") } else if b.name.ends_with("Clavicle") { idx.get("Bip01 Spine2") } else { None };
            if let Some(&np) = np {
                parents[i] = np as i32;
            }
        }
        Rig { av, idx, parents }
    }
    pub fn id(&self, name: &str) -> usize {
        *self.idx.get(name).unwrap_or_else(|| panic!("no bone {name}"))
    }
    pub fn try_id(&self, name: &str) -> Option<usize> {
        self.idx.get(name).copied()
    }
    pub fn rest(&self, i: usize) -> V {
        let p = self.av.bones[i].pos;
        [p[0] as f64, p[1] as f64, p[2] as f64]
    }
    /// evaluate global bone transforms; `root_t` translates the whole body
    pub fn eval(&self, ops: &HashMap<usize, Op>, root_t: V) -> Vec<Xf> {
        let n = self.av.bones.len();
        let mut g: Vec<Xf> = Vec::with_capacity(n);
        let mut stash: HashMap<usize, V> = HashMap::new();
        for i in 0..n {
            let b = &self.av.bones[i];
            let pg = if self.parents[i] < 0 { Xf { r: I3, t: root_t } } else { g[self.parents[i] as usize] };
            let piv = self.rest(i);
            let aim = |dir: V, twist: f64, ch: usize| -> R3 {
                let rdir = norm(sub(self.rest(ch), piv));
                let d = rapply(&rtrans(&pg.r), norm(dir));
                rmul(&arc(rdir, d), &rot_axis(rdir, twist))
            };
            let q: R3 = if let Some(op) = ops.get(&i) {
                match op {
                    Op::Rel(e) => rot_euler(*e),
                    Op::World(e) => rmul(&rtrans(&pg.r), &rot_euler(*e)),
                    Op::Aim { dir, twist } => match main_child(&b.name).and_then(|c| self.try_id(&c)) {
                        Some(ch) => aim(*dir, *twist, ch),
                        None => I3,
                    },
                    Op::Ik { target, pole } => {
                        let ch = main_child(&b.name).and_then(|c| self.try_id(&c));
                        match ch.and_then(|c| main_child(&self.av.bones[c].name).and_then(|n| self.try_id(&n)).map(|e| (c, e))) {
                            Some((mid, end)) => {
                                let l1 = len(sub(self.rest(mid), piv));
                                let l2 = len(sub(self.rest(end), self.rest(mid)));
                                let s = pg.apply(piv);
                                let to = sub(*target, s);
                                let d = len(to).clamp(0.05, l1 + l2 - 1e-3);
                                let u = norm(to);
                                let a = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
                                let h = (l1 * l1 - a * a).max(0.0).sqrt();
                                let pp = sub(*pole, scale(u, dot(*pole, u)));
                                let pp = norm(pp);
                                let e = add(s, add(scale(u, a), scale(pp, h)));
                                let end_pos = add(s, scale(u, d));
                                stash.insert(mid, norm(sub(end_pos, e)));
                                aim(norm(sub(e, s)), 0.0, mid)
                            }
                            None => I3,
                        }
                    }
                }
            } else if let Some(d) = stash.get(&i) {
                match main_child(&b.name).and_then(|c| self.try_id(&c)) {
                    Some(ch) => aim(*d, 0.0, ch),
                    None => I3,
                }
            } else {
                I3
            };
            g.push(pg.mul(&Xf::about(piv, &q)));
        }
        g
    }
}

/// skinned positions and normals per part (body frame)
pub fn skin_part(p: &AvPart, g: &[Xf]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let n = p.pos.len();
    let mut pos = Vec::with_capacity(n);
    let mut nrm = Vec::with_capacity(n);
    for i in 0..n {
        let v = [p.pos[i][0] as f64, p.pos[i][1] as f64, p.pos[i][2] as f64];
        let nv = [p.nrm[i][0] as f64, p.nrm[i][1] as f64, p.nrm[i][2] as f64];
        let mut acc = [0.0; 3];
        let mut an = [0.0; 3];
        for k in 0..4 {
            let w = p.weights[i][k] as f64;
            if w <= 0.0 {
                continue;
            }
            let x = &g[p.joints[i][k] as usize];
            let q = x.apply(v);
            let m = rapply(&x.r, nv);
            for c in 0..3 {
                acc[c] += w * q[c];
                an[c] += w * m[c];
            }
        }
        pos.push([acc[0] as f32, acc[1] as f32, acc[2] as f32]);
        let l = dot(an, an).sqrt().max(1e-12);
        nrm.push([(an[0] / l) as f32, (an[1] / l) as f32, (an[2] / l) as f32]);
    }
    (pos, nrm)
}
