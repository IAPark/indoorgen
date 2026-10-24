//! small_plants/spider_plant.py SpiderPlantFactory: the exported graph (7 leaf versions, 11 bases) is pruned to the sampled counts and its
//! random constants are replaced draw-for-draw (data/gn/spider_plant.json); scaled by 0.1.
use super::gnedit::*;
use super::*;
use crate::materials::spec_material;
use serde_json::json;
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    c == "SpiderPlantFactory"
}

pub struct SpiderPlant {
    fs: u64,
}

static TPL: OnceLock<serde_json::Value> = OnceLock::new();

impl Factory for SpiderPlant {
    fn asset(&self, inst: u64) -> Asset {
        let mut r = inst_rng(self.fs, inst);
        let n_leaf = r.randint(4, 8) as usize;
        let n_base = r.randint(5, 12) as usize;
        let init_base_radius = r.uniform(0.10, 0.20);
        let diff_base_radius = init_base_radius - 0.04;
        let init_x_r = r.uniform(1.2, 1.5);
        let diff_x_r = r.uniform(0.7, 1.1);
        let init_x_s = r.uniform(1.4, 2.0);
        let diff_x_s = r.uniform(0.2, 0.6);
        let nb = n_base as f64;
        let mut j = TPL.get_or_init(|| serde_json::from_str(include_str!("../../data/gn/spider_plant.json")).unwrap()).clone();
        let top = "geometry_spider_plant_nodes";
        let g2i = nodes_of_type(&j, top, "GeometryNodeGeometryToInstance")[0].clone();
        let join = nodes_of_type(&j, top, "GeometryNodeJoinGeometry")[0].clone();
        prune_multi(&mut j, top, &g2i, "Geometry", n_leaf);
        prune_multi(&mut j, top, &join, "Geometry", n_base);
        // leaf versions
        let leaf_nodes = sources_into(&j, top, &g2i, "Geometry");
        for ln in &leaf_nodes {
            let lg = group_of(&j, top, ln);
            let xg = group_of(&j, &lg, "Group");
            let zg = group_of(&j, &lg, "Group.001");
            let cg = group_of(&j, &lg, "Group.002");
            let xr = r.normal(0.0, 1.2).abs();
            let zr = r.normal(0.0, 0.6).abs();
            let k = r.uniform(0.0, 0.05);
            let m = r.uniform(0.8, 1.3);
            set_in(&mut j, &xg, "Map Range", "To Max", json!(xr));
            set_in(&mut j, &zg, "Map Range", "To Max", json!(zr));
            set_curve_y(&mut j, &cg, "Float Curve", &[0.1, 0.1 + k / 1.5, 0.1 + k / 1.5, 0.1, 0.1 - k, 0.0]);
            set_in(&mut j, &cg, "Math", "Value_001", json!(m));
        }
        // plant bases
        let shared = upstream(&j, top, &g2i);
        let reals = sources_into(&j, top, &join, "Geometry");
        for (i, rn) in reals.iter().enumerate() {
            let rad = init_base_radius - (i as f64 * diff_base_radius) / nb;
            let x_r = init_x_r - (i as f64 * diff_x_r) / nb;
            let x_s = init_x_s - (i as f64 * diff_x_s) / nb;
            let count = r.randint(20, 40);
            let chain: Vec<String> = upstream(&j, top, rn).into_iter().filter(|n| !shared.contains(n)).collect();
            for n in chain {
                match node_type(&j, top, &n).as_str() {
                    "GeometryNodeCurvePrimitiveCircle" => set_in(&mut j, top, &n, "Radius", json!(rad)),
                    "GeometryNodeResampleCurve" => set_in(&mut j, top, &n, "Count", json!(count)),
                    "FunctionNodeRandomValue" => {
                        set_in(&mut j, top, &n, "Min_001", json!(-0.3 * rad));
                        set_in(&mut j, top, &n, "Max_001", json!(0.3 * rad));
                    }
                    "ShaderNodeValue" => set_prop(&mut j, top, &n, "value", json!(x_s)),
                    "GeometryNodeGroup" => {
                        let g = group_of(&j, top, &n);
                        if g.starts_with("nodegroup_leaf_rotate_on_base") {
                            set_in(&mut j, &g, "Math", "Value", json!(x_r));
                        }
                    }
                    _ => {}
                }
            }
        }
        let g = eval_geo(j, top);
        let g = crate::gn::meshops::realize_instances(&g, 1000);
        let mut m = Mesh::new();
        if let Some(gm) = &g.mesh {
            for p in &gm.pos {
                m.pos.push([(p[0] * 0.1f32) as f64, (p[1] * 0.1f32) as f64, (p[2] * 0.1f32) as f64]);
            }
            m.faces = gm.faces_vec();
        }
        let mat = spec_material("spider_plant", json!({"cls": "plant.SpiderPlant", "kwargs": {}, "seed": self.fs}), [0.2, 0.4, 0.2]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(SpiderPlant { fs }))
}
