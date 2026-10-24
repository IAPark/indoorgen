//! appliances/tv.py TVFactory / MonitorFactory.create_asset: bevelled cube body (optional depth extrusion bridged to a plane), screen
//! recess (boolean), two legs (geo_radius) or a single bevelled leg + base.
use super::bathkit::*;
use super::cabnodes::GMesh;
use super::cookware::Tv;
use super::bathtub::stage;
use super::hwexact;
use crate::mesh::Mesh;
use crate::rng::Rng;

fn cube(scale: [f64; 3], loc: [f64; 3], pre_loc: Option<[f64; 3]>) -> Mesh {
    let mut g: GMesh = hwexact::bpy_cube();
    if let Some(l) = pre_loc {
        g = hwexact::apply_obj(&g, [1.0; 3], [0.0; 3], l, true);
    }
    from_g(&hwexact::apply_obj(&g, scale, [0.0; 3], loc, true))
}

pub struct TvBuilt {
    pub body: Mesh,
    pub legs: Vec<Mesh>,
    pub stages: Vec<(String, String, Mesh)>,
}

pub fn build(t: &Tv, r: &mut Rng) -> TvBuilt {
    let (tw, th, d) = (t.width + 2.0 * t.side, t.height + t.side + t.bottom, t.depth);
    let mut stages = vec![];
    // make_base
    let base = cube([tw / 2.0, d / 2.0, th / 2.0], [0.0; 3], Some([0.0, 1.0, 1.0]));
    let mut obj = bevel_def(&base, t.bevel_w, 8);
    if t.has_extrude {
        let nr = super::bpyx::face_normals(&obj);
        let keep: Vec<bool> = nr.iter().map(|n| !((n[1] as f32) > 0.5)).collect();
        let open = separate_faces(&obj, &keep);
        let loops = boundary_loops(&open);
        assert_eq!(loops.len(), 1, "tv back loop");
        let hmin = th * r.uniform(0.1, 0.3);
        let hmax = th * r.uniform(0.5, 0.7);
        let width = tw * r.uniform(0.3, 0.6);
        // extra plane: bpy plane (-1,-1,0),(1,-1,0),(-1,1,0),(1,1,0); scale, rotation x = -pi/2, location (no apply: join folds it in)
        let mut plane = Mesh::new();
        plane.pos = vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        plane.faces = vec![vec![0, 1, 3, 2]];
        apply_scale_loc(&mut plane, [width / 2.0, (hmax - hmin) / 2.0, 1.0], -std::f64::consts::FRAC_PI_2, [0.0, t.extrude + d, th / 2.0]);
        let n0 = open.pos.len() as u32;
        let mut m = join(&[&open, &plane]);
        let shape = -r.uniform(0.0, 0.4);
        if let Ok(d) = std::env::var("BRIDGEDUMP") {
            let js = serde_json::json!({"v": m.pos, "p": m.faces, "loops": [loops[0], [n0, n0 + 1, n0 + 3, n0 + 2]], "cuts": 32, "shape": shape});
            std::fs::write(format!("{d}/tv_bridge_in.json"), js.to_string()).unwrap();
        }
        let la = loops[0].clone();
        let lb = vec![n0, n0 + 1, n0 + 3, n0 + 2];
        super::bridge::bridge_two_loops(&mut m, la, lb, 32, shape);
        let shift = (hmax + hmin - th) / 2.0;
        for p in m.pos.iter_mut() {
            let (y, z) = (p[1] as f32, p[2] as f32);
            p[2] = (z + (shift as f32) * ((y - d as f32).max(0.0)) / t.extrude as f32) as f64;
        }
        obj = m;
    }
    stage(&mut stages, "base", "base", &obj);
    // make_screen: cutter cube
    let cutter = cube([t.width / 2.0, 1.0, t.height / 2.0], [0.0, 1e-3, t.bottom], Some([0.0, -1.0, 1.0]));
    obj = boolean_exact("DIFFERENCE", &obj, &cutter);
    stage(&mut stages, "screen", "screen", &obj);
    // legs
    let mut legs = vec![];
    if t.two_legs {
        let p0 = [-tw / 2.0 * t.leg_w * r.uniform(0.0, 0.6), 0.0, th * r.uniform(0.3, 0.5)];
        let verts = [p0, [0.0, 0.0, -t.leg_len], [0.0, t.leg_len_y / 2.0, -t.leg_len], [0.0, -t.leg_len_y / 2.0, -t.leg_len]];
        let f32v: Vec<[f64; 3]> = verts.iter().map(|p| [p[0] as f32 as f64, p[1] as f32 as f64, p[2] as f32 as f64]).collect();
        let mut leg = super::gnhelpers::geo_radius_graph(&f32v, &[[0, 1], [1, 2], [1, 3]], t.leg_radius(), 16, 0.004, true);
        let zmin = (-t.leg_len - t.leg_r * r.uniform(0.0, 0.6)) as f32;
        for p in leg.pos.iter_mut() {
            p[2] = (p[2] as f32).max(zmin) as f64;
        }
        let tr = |m: &Mesh, sx: f64, x: f64| -> Mesh {
            let mut o = m.clone();
            for p in o.pos.iter_mut() {
                *p = [((p[0] as f32) * sx as f32 + x as f32) as f64, (p[1] as f32 + ((1.0 - t.leg_w) * t.extrude) as f32) as f64, p[2]];
            }
            if sx < 0.0 {
                for f in o.faces.iter_mut() {
                    f.reverse();
                }
            }
            o
        };
        legs.push(tr(&leg, 1.0, tw / 2.0 * t.leg_w));
        legs.push(tr(&leg, -1.0, -tw / 2.0 * t.leg_w));
    } else {
        let lw = tw * r.uniform(0.05, 0.1);
        let lh = (t.leg_len + th * r.uniform(0.3, 0.5)) / 2.0;
        let leg = cube([lw, t.leg_r, lh], [0.0, t.extrude / 2.0, -t.leg_len], Some([0.0, 1.0, 1.0]));
        let leg = bevel_def(&leg, t.leg_bevel, 8);
        let bw = tw * r.uniform(0.15, 0.3);
        let b = from_g(&hwexact::apply_obj(&hwexact::bpy_cube(), [bw, t.leg_len_y / 2.0, t.leg_r], [0.0; 3], [0.0, t.extrude / 2.0, -t.leg_len], true));
        let base = bevel_def(&b, t.leg_bevel, 8);
        legs.push(leg);
        legs.push(base);
    }
    TvBuilt { body: obj, legs, stages }
}

impl Tv {
    pub fn leg_radius(&self) -> f64 {
        self.leg_r
    }
}
