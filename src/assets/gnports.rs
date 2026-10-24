//! Assets ported through the geometry-node interpreter (data/gn/*.json templates + RNG-exact parameter samplers).
use super::gnasset::*;
use super::*;
use std::sync::OnceLock;

pub fn handles(c: &str) -> bool {
    matches!(c, "TriangleShelfFactory")
}

pub fn make(c: &str, _rng: &mut Rng, fseed: u64) -> Option<Box<dyn Factory>> {
    match c {
        "TriangleShelfFactory" => {
            static TPL: OnceLock<GnTemplate> = OnceLock::new();
            let tpl = TPL.get_or_init(|| GnTemplate::parse(include_str!("../../data/gn/triangle_shelf.json")));
            Some(Box::new(GnFactory { tpl, sample: triangle_shelf_params, fseed }))
        }
        _ => None,
    }
}

fn p1(v: f64) -> Vec<f64> {
    vec![v]
}

/// shelves/triangle_shelf.py TriangleShelfFactory.get_asset_params (geometry parameters only; the material draws come last)
pub fn triangle_shelf_params(fseed: u64, inst: u64) -> Params {
    let mut r = inst_rng(fseed, inst);
    let mut p = Params::default();
    // sample_params(): Dimensions, leg_length, board_width
    let d0 = r.uniform(0.25, 0.35);
    let _d1 = r.uniform(0.25, 0.35);
    let d2 = r.uniform(0.5, 0.7);
    let leg_length = d2;
    let board_width = d0;
    let leg_board_gap = r.uniform(0.002, 0.005);
    let leg_width = r.uniform(0.01, 0.03);
    let leg_depth = r.uniform(0.01, 0.02);
    let leg_curvature_ratio = r.uniform(0.0, 0.02);
    let board_thickness = r.uniform(0.01, 0.025);
    let board_extrude_length = r.uniform(0.03, 0.07);
    let side_board_height = r.uniform(0.02, 0.04);
    let bottom_layer_height = r.uniform(0.05, 0.1);
    let top_layer_height = leg_length - r.uniform(0.02, 0.07);
    let mid_layer_height = (top_layer_height + bottom_layer_height) / 2.0;
    p.insert("leg_board_gap".into(), p1(leg_board_gap));
    p.insert("leg_width".into(), p1(leg_width));
    p.insert("leg_depth".into(), p1(leg_depth));
    p.insert("leg_length".into(), p1(leg_length));
    p.insert("leg_curvature_ratio".into(), p1(leg_curvature_ratio));
    p.insert("board_thickness".into(), p1(board_thickness));
    p.insert("board_width".into(), p1(board_width));
    p.insert("board_extrude_length".into(), p1(board_extrude_length));
    p.insert("side_board_height".into(), p1(side_board_height));
    p.insert("bottom_layer_height".into(), p1(bottom_layer_height));
    p.insert("top_layer_height".into(), p1(top_layer_height));
    p.insert("mid_layer_height".into(), p1(mid_layer_height));
    p
}
