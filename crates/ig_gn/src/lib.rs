//! ig_gn: part of the infinigen-rs workspace (see crates/README.md).
#![allow(unused_imports, dead_code, unused_variables, unused_mut, non_snake_case, ambiguous_glob_reexports)]
pub use crate as gn;
/// old `crate::assets::X` paths resolve to the crate root (every crate re-exports the asset toolkit)
pub mod assets {
    pub use super::*;
}
pub use ig_core::*;
pub use ig_exbool as exbool;
pub use ig_bm as bm;
pub use ig_geom::*;
pub use ig_kit::*;
// Geometry Nodes interpreter: evaluates Blender 4.2 node groups exported as JSON (tools/gn_export.py).
pub mod attr;
pub mod cdt;
pub use crate::bm::bullet_hull;
pub mod csg;
pub mod xbool;
pub mod fbool;
pub mod dual;
pub mod curveops;
pub mod eval;
pub mod field;
pub mod geom;
pub mod load;
pub mod math;
pub mod meshops;
pub mod noise;
pub mod nodes;
pub mod nodes_geo;
pub mod nodes_geo2;
pub mod nodes_mesh;
pub mod nodes_fields;
pub mod nodes_curves;
pub mod nodes_text;
pub mod prim;
pub mod rot;
pub mod val;

pub use eval::{apply_out_attrs, eval, eval_probe, probe_mesh, Ev, Objects, Value};
pub use geom::*;
pub use load::{load_file, Groups};
pub use val::{Ty, Val};

