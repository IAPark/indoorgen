//! tableware/food_box.py FoodBoxFactory: a box (placeholder dimensions) with BEVEL(width=0.001, 1 segment)
use super::textdraw::text_ctor_and_generate;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "FoodBoxFactory"
}

pub struct FoodBox {
    dims: [f64; 3],
    fs: u64,
}

impl FoodBox {
    fn new(fs: u64) -> FoodBox {
        let mut r = Rng::seeded(fs);
        let mut d = [r.log_uniform(0.05, 0.3), r.log_uniform(0.05, 0.3), r.log_uniform(0.05, 0.3)];
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        text_ctor_and_generate(&mut r);
        r.uniform(0.0, 1.0); // texture_shared
        FoodBox { dims: [d[1], d[0], d[2]], fs }
    }
}

/// Blender's BEVEL(width, 1 segment) output for a box, in Blender's exact vertex and face order (independent of the dimensions):
/// vertex k sits at the box corner `corner` (bit 4/2/1 = x/y/z positive) pulled in by w along the two axes other than `full`
const BEVEL_VERTS: [(u8, u8); 24] = [
    (0, 2), (0, 1), (0, 0), (1, 1), (1, 2), (1, 0), (2, 2), (2, 0), (2, 1), (3, 2), (3, 1), (3, 0),
    (4, 2), (4, 0), (4, 1), (5, 2), (5, 1), (5, 0), (6, 2), (6, 1), (6, 0), (7, 2), (7, 0), (7, 1),
];
const BEVEL_FACES: [&[u32]; 26] = [
    &[14, 16, 3, 1], &[20, 22, 17, 13], &[21, 9, 4, 15], &[2, 5, 11, 7], &[8, 10, 23, 19],
    &[0, 1, 2], &[3, 4, 5], &[6, 7, 8], &[9, 10, 11], &[12, 13, 14], &[15, 16, 17], &[18, 19, 20], &[21, 22, 23],
    &[6, 0, 2, 7], &[1, 3, 5, 2], &[4, 9, 11, 5], &[10, 8, 7, 11], &[18, 6, 8, 19], &[9, 21, 23, 10], &[22, 20, 19, 23],
    &[12, 18, 20, 13], &[21, 15, 17, 22], &[16, 14, 13, 17], &[0, 12, 14, 1], &[15, 4, 3, 16], &[6, 18, 12, 0],
];

pub fn beveled_box(h: [f32; 3], w: f32) -> Mesh {
    let mut m = Mesh::new();
    for &(corner, full) in BEVEL_VERTS.iter() {
        let s = [if corner & 4 != 0 { 1.0f32 } else { -1.0 }, if corner & 2 != 0 { 1.0f32 } else { -1.0 }, if corner & 1 != 0 { 1.0f32 } else { -1.0 }];
        let mut p = [0.0f32; 3];
        for a in 0..3 {
            p[a] = if a == full as usize { s[a] * h[a] } else { s[a] * h[a] - s[a] * w };
        }
        m.pos.push([p[0] as f64, p[1] as f64, p[2] as f64]);
    }
    m.faces = BEVEL_FACES.iter().map(|f| f.to_vec()).collect();
    m
}

impl Factory for FoodBox {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let h = self.dims.map(|d| (d / 2.0) as f32 as f64);
        let m = books::new_bbox(-h[0], h[0], -h[1], h[1], -h[2], h[2]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat("ph", [0.5; 3], 0.5), Some(b));
        Some(a)
    }
    /// collision: the box
    fn volumes(&self, _i: u64) -> Option<Vec<crate::piece::Volume>> {
        let h = self.dims.map(|d| d / 2.0 + 0.002);
        Some(vec![crate::piece::Volume::hard([-h[0], -h[1], -h[2]], h)])
    }
    fn asset(&self, _i: u64) -> Asset {
        let h = self.dims.map(|d| (d / 2.0) as f32);
        let m = beveled_box(h, 0.001);
        let mat = spec_material("foodbox", json!({"cls": "text.Text", "kwargs": {}, "seed": self.fs}), [0.8, 0.6, 0.4]);
        let mut a = Asset::default();
        let b = m.bounds();
        a.mesh(m, &mat, Some(b));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(FoodBox::new(fs)))
}
