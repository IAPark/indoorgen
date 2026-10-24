//! bathroom/toilet.py ToiletFactory.create_asset ported stage by stage (surface-equal to Blender; vertex order is not Blender's for
//! bridge / solidify stages). Stage dumps: ~/work/agents/storage/toistages.py, compared by src/bin/toicmp.rs.
use super::bathroom::Toilet;
use super::blops::align_bezier_points;
use super::bridge_ex::bridge_two_loops_ex;
use crate::mesh::Mesh;
use crate::rng::Rng;

type V3 = [f64; 3];

fn f32v(p: [f32; 3]) -> V3 {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

/// `apply_transform` of a uniform-scale + translation object (float32 matrix product)
fn xform(pts: &[V3], s: f64, loc: V3) -> Vec<V3> {
    let (s, l) = (s as f32, [loc[0] as f32, loc[1] as f32, loc[2] as f32]);
    pts.iter().map(|p| [((p[0] as f32) * s + l[0]) as f64, ((p[1] as f32) * s + l[1]) as f64, ((p[2] as f32) * s + l[2]) as f64]).collect()
}

pub struct Ring {
    pub pts: Vec<V3>,
    /// loop order (indices into pts)
    pub order: Vec<u32>,
}

/// build_curve(): align_bezier + MIRROR(x). Returns the 224-vertex ring in python's vertex order
pub fn build_curve(t: &Toilet) -> Ring {
    let mid = t.mid_offset();
    let anchors = [[0.0, -t.size_mid * t.size, 0.0], [t.width / 2.0, 0.0, 0.0], [0.0, mid, 0.0]];
    let axes = [Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0]), Some([1.0, 0.0, 0.0])];
    let half = align_bezier_points(&anchors, &axes, &t.curve_scales);
    let n = half.len();
    let mut pts: Vec<V3> = half.iter().map(|p| f32v(*p)).collect();
    for i in 1..n - 1 {
        pts.push([-pts[i][0], pts[i][1], pts[i][2]]);
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.extend((n as u32..pts.len() as u32).rev());
    Ring { pts, order }
}

pub struct Stages {
    pub upper: Vec<V3>,
    pub lower: Vec<V3>,
    pub bottom: Vec<V3>,
    pub bridged: Mesh,
}

/// upper / lower / bottom rings and the bridged tube (before SOLIDIFY)
pub fn tube_bridge(t: &Toilet, r: &mut Rng) -> Stages {
    let ring = build_curve(t);
    let upper = ring.pts.clone();
    let lower = xform(&upper, t.tube_scale, [0.0, t.tube_scale * t.mid_offset() / 2.0, -t.depth]);
    let bottom = xform(&upper, t.stand_scale, [0.0, t.tube_scale * (1.0 - t.size_mid) * t.size / 2.0 * t.bottom_offset, -t.height]);
    let n = upper.len() as u32;
    let mut m = Mesh::new();
    m.pos = upper.iter().chain(lower.iter()).cloned().collect();
    let shape = r.uniform(0.1, 0.2);
    let la: Vec<u32> = ring.order.clone();
    let lb: Vec<u32> = ring.order.iter().map(|&i| i + n).collect();
    bridge_two_loops_ex(&mut m, la, lb, 64, shape, true);
    Stages { upper, lower, bottom, bridged: m }
}

/// SOLIDIFY(NON_MANIFOLD, thickness, offset=1, boundary FLAT) of the bridged surface
pub fn solidify_tube(t: &Toilet, bridged: &Mesh) -> Mesh {
    use super::solidnm::*;
    let pos: Vec<[f32; 3]> = bridged.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let out = solidify_nonmanifold(&pos, &bridged.faces, t.thickness as f32, 1.0, Boundary::Flat).expect("solidify");
    let mut m = Mesh::new();
    m.pos = out.pos.iter().map(|p| f32v(*p)).collect();
    m.faces = out.faces;
    m
}

/// make_tube tail: extrude the upward-facing faces by thickness+extrude_height, then clip z to extrude_height
pub fn extrude_top(t: &Toilet, solid: &Mesh) -> Mesh {
    use super::bpyx::*;
    let mut m = solid.clone();
    let nor = face_normals(&m);
    let sel: Vec<bool> = nor.iter().map(|n| n[2] > 0.9).collect();
    extrude_region(&mut m, &sel, [0.0, 0.0, (t.thickness as f32 + t.extrude_height as f32) as f64]);
    let e = t.extrude_height as f32;
    for p in m.pos.iter_mut() {
        p[2] = (p[2] as f32).min(e) as f64;
    }
    m
}
