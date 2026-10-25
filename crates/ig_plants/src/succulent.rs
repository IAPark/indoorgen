//! small_plants/succulent.py SucculentFactory: exported graph (7 bases) pruned to the sampled base count; every random constant replaced draw-for-draw
//! (data/gn/succulent.json); scaled by 0.2, lifted by 0.01.
use super::gnedit::*;
use super::*;
use crate::materials::spec_material;
use serde_json::{json, Value as J};
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    c == "SucculentFactory"
}

pub struct Succulent {
    thin: bool,
    fs: u64,
}

static TPL: OnceLock<J> = OnceLock::new();

fn suffix(i: usize) -> String {
    if i == 0 { String::new() } else { format!(".{:03}", i) }
}

impl Succulent {
    /// the constructor's draw from the global stream: np.random.choice(["thin_petal", "thick_petal"], p=[.65, .35])
    pub fn new(fs: u64) -> Succulent {
        let u = match super::unseeded() {
            Some(v) if !v.is_empty() => v[0],
            _ => Rng::seeded(fs).uniform(0.0, 1.0),
        };
        Succulent { thin: u < 0.65, fs }
    }
    /// ctor draw taken from a given stream (inside a FixedSeed block, e.g. PlantContainer)
    pub fn with_mode(fs: u64, thin: bool) -> Succulent {
        Succulent { thin, fs }
    }
}

impl Factory for Succulent {
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fs, inst);
        // get_params
        let (cross_y_bottom, cross_y_top, cross_x, num_bases);
        let (init_base_radius, diff_base_radius, init_x_r, diff_x_r, diff_petal_scale);
        let init_petal_num: i32;
        if self.thin {
            cross_y_bottom = r.uniform(0.08, 0.25);
            cross_y_top = r.uniform(-0.04, 0.02);
            cross_x = r.uniform(0.3, 0.6);
            num_bases = r.randint(5, 8) as usize;
            init_base_radius = r.uniform(0.09, 0.11);
            diff_base_radius = 0.1;
            init_x_r = r.uniform(-1.2, -1.35);
            diff_x_r = r.uniform(-0.7, -1.1);
            init_petal_num = r.randint(num_bases as i64, 15) as i32;
            diff_petal_scale = r.uniform(0.5, 0.9);
        } else {
            cross_y_bottom = r.uniform(0.22, 0.30);
            cross_y_top = r.uniform(0.08, 0.15);
            cross_x = r.uniform(0.14, 0.16);
            num_bases = r.randint(3, 6) as usize;
            init_base_radius = r.uniform(0.12, 0.14);
            diff_base_radius = 0.11;
            init_x_r = r.uniform(-1.3, -1.4);
            diff_x_r = r.uniform(-0.1, -1.2);
            init_petal_num = r.randint(num_bases as i64, 12) as i32;
            diff_petal_scale = r.uniform(0.6, 0.9);
        }
        let nb = num_bases as f64;
        let (mut base_radius, mut petal_x_r, mut petal_num, mut petal_scale, mut base_z) = (vec![], vec![], vec![], vec![], vec![]);
        for i in 0..num_bases {
            let fi = i as f64;
            base_radius.push(init_base_radius - (fi * diff_base_radius) / nb);
            petal_x_r.push(init_x_r - (fi * diff_x_r) / nb);
            petal_num.push(init_petal_num - i as i32 + r.randint(0, 2) as i32);
            petal_scale.push(1.0 - (fi * diff_petal_scale) / nb);
            base_z.push(0.0 + fi * if self.thin { r.uniform(0.005, 0.008) } else { r.uniform(0.005, 0.006) });
        }
        let contour_bit = if self.thin { r.randint(0, 3) } else { r.randint(0, 2) } as usize;
        let _material_bit = if self.thin { r.randint(0, 3) } else { r.randint(0, 2) };
        let cp: [f64; 6] = if self.thin {
            [[0.08, 0.4, 0.46, 0.36, 0.17, 0.05], [0.22, 0.37, 0.50, 0.49, 0.30, 0.08], [0.21, 0.26, 0.31, 0.36, 0.29, 0.16]][contour_bit]
        } else {
            [[0.10, 0.36, 0.44, 0.45, 0.30, 0.24], [0.16, 0.35, 0.48, 0.42, 0.30, 0.18]][contour_bit]
        };
        // graph
        let mut j = TPL.get_or_init(|| serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gn/succulent.json"))).unwrap()).clone();
        let top = "geometry_succulent_nodes";
        let join = nodes_of_type(&j, top, "GeometryNodeJoinGeometry")[0].clone();
        prune_multi(&mut j, top, &join, "Geometry", num_bases);
        set_prop(&mut j, top, "petal_cross_coutour_y_bottom", "value", json!(cross_y_bottom));
        set_prop(&mut j, top, "petal_cross_coutour_x", "value", json!(cross_x));
        set_prop(&mut j, top, "petal_cross_coutour_y_top", "value", json!(cross_y_top));
        set_prop(&mut j, top, "petal_stem_curvature_scale", "value", json!(r.normal(0.0, 1.0).abs()));
        set_prop(&mut j, top, "petal_z_coutour_scale", "value", json!(r.uniform(0.4, 0.9)));
        for i in 0..num_bases {
            let sfx = suffix(i);
            let pg_node = if i == 0 { "Group".to_string() } else { format!("Group.{:03}", 2 * i) };
            let pb_node = format!("Group.{:03}", 2 * i + 1);
            let pg = group_of(&j, top, &pg_node);
            // draws of nodegroup_petal_geometry(...)
            let k = r.uniform(0.0, 0.3);
            let n1 = r.normal(0.0, 0.04);
            let n2 = r.normal(0.0, 0.1);
            let n3 = r.normal(0.0, 0.03);
            let n4 = r.normal(0.0, 0.06);
            let n5 = r.normal(0.0, 0.04);
            let noise_scale = r.uniform(0.0, 0.02);
            let z_rot = -1.57 + r.normal(0.0, 0.3);
            let subs: Vec<String> = j["groups"][&pg]["nodes"].as_array().unwrap().iter().filter(|n| n["type"].as_str() == Some("GeometryNodeGroup")).map(|n| n["name"].as_str().unwrap().to_string()).collect();
            let (stem_g, zc_g, cc_g) = (group_of(&j, &pg, &subs[0]), group_of(&j, &pg, &subs[1]), group_of(&j, &pg, &subs[2]));
            set_curve_y(&mut j, &stem_g, "Float Curve", &[0.0, 0.2 - k / 2.5, 0.4 - k / 1.1, 0.6 - k, 0.8 - k / 1.5, 1.0 - k / 3.0]);
            set_curve_y(&mut j, &zc_g, "Float Curve", &[cp[0], cp[1] * (1.0 + n1), cp[2] * (1.0 + n2), cp[3] * (1.0 + n3), cp[4] * (1.0 + n4), cp[5] * (1.0 + n5), 0.0]);
            set_in(&mut j, &cc_g, "Vector Math", "Scale", json!(noise_scale));
            set_prop(&mut j, top, &format!("base_circle_radius{}", sfx), "value", json!(base_radius[i]));
            set_prop(&mut j, top, &format!("petal_x_rotation{}", sfx), "value", json!(petal_x_r[i]));
            set_prop(&mut j, top, &format!("base_z_rotation{}", sfx), "value", json!(z_rot));
            set_prop(&mut j, top, &format!("base_petal_num{}", sfx), "integer", json!(petal_num[i]));
            set_prop(&mut j, top, &format!("petal_scale{}", sfx), "value", json!(petal_scale[i]));
            set_prop(&mut j, top, &format!("base_z{}", sfx), "value", json!(base_z[i]));
            let pbg = group_of(&j, top, &pb_node);
            let bp_node = j["groups"][&pbg]["nodes"].as_array().unwrap().iter().find(|n| n["p"]["group"].as_str().map_or(false, |g| g.starts_with("nodegroup_base_perturbation"))).map(|n| n["name"].as_str().unwrap().to_string()).unwrap();
            let bpg = group_of(&j, &pbg, &bp_node);
            let rr = base_radius[i];
            for (nm, a) in [("Random Value", 0.8), ("Random Value.001", 0.8), ("Random Value.002", 0.2)] {
                set_in(&mut j, &bpg, nm, "Min_001", json!(-a * rr));
                set_in(&mut j, &bpg, nm, "Max_001", json!(a * rr));
            }
        }
        let g = eval_geo(j, top);
        let g = crate::gn::meshops::realize_instances(&g, 1000);
        let mut m = Mesh::new();
        if let Some(gm) = &g.mesh {
            for p in &gm.pos {
                let s = 0.2f32;
                m.pos.push([(p[0] * s) as f64, (p[1] * s) as f64, (p[2] * s + 0.01f32) as f64]);
            }
            m.faces = gm.faces_vec();
        }
        let mat = spec_material("succulent", json!({"cls": "plant.Succulent", "kwargs": {}, "seed": self.fs}), [0.3, 0.5, 0.3]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Succulent::new(fs)))
}
