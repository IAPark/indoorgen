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
    let ang = 30.0f32 * (std::f64::consts::PI / 180.0) as f32;
    let (mut obj, obj_edges) = super::bevmod::bevel_modifier_edges(&base, t.bevel_w, 8, Some(ang), true, 0.5, None).expect("bevel");
    if t.has_extrude {
        let nr = super::bpyx::face_normals(&obj);
        let keep: Vec<bool> = nr.iter().map(|n| !((n[1] as f32) > 0.5)).collect();
        let open = separate_faces(&obj, &keep);
        // bpy.ops.mesh.bridge_edge_loops: BM_mesh_edgeloops_find starts the loop with the first selected edge (v1, v2) in mesh edge order and
        // continues through v2 (list = [v1, v2, ...]); compact() keeps the ascending order of the used vertices
        let mut used = vec![false; obj.pos.len()];
        let mut cnt: crate::dhash::HashMap<(u32, u32), u32> = Default::default();
        for (fi, f) in obj.faces.iter().enumerate() {
            if !keep[fi] {
                continue;
            }
            for k in 0..f.len() {
                used[f[k] as usize] = true;
                let (a, b) = (f[k], f[(k + 1) % f.len()]);
                *cnt.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let mut newidx = vec![u32::MAX; obj.pos.len()];
        let mut nn = 0u32;
        for v in 0..obj.pos.len() {
            if used[v] {
                newidx[v] = nn;
                nn += 1;
            }
        }
        let loops = {
            let bnd: Vec<[u32; 2]> = obj_edges.iter().cloned().filter(|e| cnt.get(&(e[0].min(e[1]), e[0].max(e[1]))) == Some(&1)).collect();
            let mut adj: crate::dhash::HashMap<u32, Vec<u32>> = Default::default();
            for e in &bnd {
                adj.entry(e[0]).or_default().push(e[1]);
                adj.entry(e[1]).or_default().push(e[0]);
            }
            let (v1, v2) = (bnd[0][0], bnd[0][1]);
            let mut lp = vec![v1, v2];
            let (mut prev, mut cur) = (v1, v2);
            loop {
                let nx = adj[&cur].iter().cloned().find(|&w| w != prev).expect("tv loop");
                if nx == v1 {
                    break;
                }
                lp.push(nx);
                prev = cur;
                cur = nx;
            }
            vec![lp.into_iter().map(|v| newidx[v as usize]).collect::<Vec<u32>>()]
        };
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
            let js = serde_json::json!({"v": m.pos, "p": m.faces, "loops": [loops[0], [n0 + 2, n0, n0 + 1, n0 + 3]], "cuts": 32, "shape": shape});
            std::fs::write(format!("{d}/tv_bridge_in.json"), js.to_string()).unwrap();
        }
        let la = loops[0].clone();
        let lb = vec![n0 + 2, n0, n0 + 1, n0 + 3];
        super::bridge::bridge_two_loops(&mut m, la, lb, 32, shape);
        let shift = (hmax + hmin - th) / 2.0;
        for p in m.pos.iter_mut() {
            let (y, z) = (p[1] as f32, p[2] as f32);
            p[2] = (z + (shift as f32) * ((y - d as f32).max(0.0)) / t.extrude as f32) as f64;
        }
        obj = m;
    }
    if let Ok(d) = std::env::var("PANDBG") {
        let _ = std::fs::write(format!("{d}/rs_tvbase.json"), serde_json::json!({"v": obj.pos, "p": obj.faces}).to_string());
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
