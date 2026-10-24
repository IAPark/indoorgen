//! Faithful port of Blender 4.2's EXACT mesh boolean (BLI mesh_intersect / mesh_boolean), vertex/face/edge order exact.
//! Developed and verified in ~/work/ref/boolport (51 Blender fixtures, all exact).
pub mod boolean;
pub mod cdt_q;
pub mod exact_boolean;
pub mod hashtab;
pub mod imesh;
pub mod intersect;
pub mod libcxx_sort;
pub mod mpq;
pub mod polyfill;
