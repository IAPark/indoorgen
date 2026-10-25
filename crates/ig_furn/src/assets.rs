//! Empirical asset backend: bounding boxes sampled from the reference factories (used for classes without a dedicated port).
use super::geom::*;
use super::solver::*;
use crate::mesh::V3;
use crate::dhash::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub ph: Option<(V3, V3)>,
    pub asset: Option<(V3, V3)>,
}

#[derive(Default)]
pub struct TableAssets {
    pub table: HashMap<String, Vec<Sample>>,
}

fn v3(x: &serde_json::Value) -> V3 {
    let c = x.as_array().unwrap();
    [c[0].as_f64().unwrap(), c[1].as_f64().unwrap(), c[2].as_f64().unwrap()]
}

fn bb(x: &serde_json::Value) -> Option<(V3, V3)> {
    let a = x.as_array()?;
    Some((v3(&a[0]), v3(&a[1])))
}

impl TableAssets {
    /// Load one or more json files (old format: class -> [[lo,hi]...]; new format: class -> [{ph, asset}...]).
    pub fn load(path: &str) -> TableAssets {
        let mut t = TableAssets::default();
        t.add_file(path);
        t
    }

    pub fn add_file(&mut self, path: &str) {
        let Ok(txt) = std::fs::read_to_string(path) else { return };
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap();
        for (k, list) in v.as_object().unwrap() {
            let e = self.table.entry(k.clone()).or_default();
            for row in list.as_array().unwrap() {
                if row.is_array() {
                    e.push(Sample { ph: None, asset: bb(row) });
                } else {
                    e.push(Sample { ph: row.get("ph").and_then(bb), asset: row.get("asset").and_then(bb) });
                }
            }
        }
    }

    pub fn sample(&self, class: &str, factory_seed: u64, inst_seed: u64) -> Option<Sample> {
        let v = self.table.get(class)?;
        if v.is_empty() {
            return None;
        }
        let h = factory_seed.wrapping_mul(2654435761).wrapping_add(inst_seed.wrapping_mul(40503)) as usize;
        Some(v[h % v.len()])
    }
}

impl Assets for TableAssets {
    fn placeholder(&self, class: &str, factory_seed: u64, inst_seed: u64, kind: PlaceholderKind) -> Option<ObjGeom> {
        let (lo, hi) = match self.sample(class, factory_seed, inst_seed) {
            Some(s) => match kind {
                PlaceholderKind::Real | PlaceholderKind::BboxOfPlaceholder => s.ph.or(s.asset),
                PlaceholderKind::AssetAsPlaceholder | PlaceholderKind::BboxOfAsset => s.asset.or(s.ph),
            }
            .unwrap_or(([-0.15, -0.15, 0.0], [0.15, 0.15, 0.3])),
            None => ([-0.15, -0.15, 0.0], [0.15, 0.15, 0.3]),
        };
        Some(ObjGeom::cuboid(lo, hi, 0))
    }
}
