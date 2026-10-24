//! wall_decorations/wall_art.py: WallArtFactory, MirrorFactory
use super::bridge::bridge_two_loops;
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    matches!(c, "WallArtFactory" | "MirrorFactory")
}

const FRAME: Weighted = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("wood.Wood", 1.0),
    ("wood.WhitePlywood", 0.1),
    ("wood.BlackPlywood", 0.1),
    ("wood.BlondePlywood", 0.1),
];
const ART: Weighted = &[("art.Art", 1.0), ("art.DarkArt", 0.5)];

pub struct WallArt {
    width: f64,
    height: f64,
    thickness: f64,
    depth: f64,
    bevel_segments: usize,
    bevel_width: f64,
    mirror: bool,
    seed: u64,
}

impl WallArt {
    fn new(rng: &mut Rng, mirror: bool) -> WallArt {
        let mut r = rng.clone();
        let width = r.log_uniform(0.4, 2.0);
        let height = r.log_uniform(0.4, 2.0);
        let thickness = r.uniform(0.02, 0.05);
        let depth = r.uniform(0.01, 0.02);
        let bevel_segments = [0usize, 1, 4][r.choice_idx(3)];
        let bevel_width = r.uniform(depth / 4.0, depth / 2.0);
        WallArt { width, height, thickness, depth, bevel_segments, bevel_width, mirror, seed: r.randint(0, 1_000_000) as u64 }
    }
}

impl WallArt {
    /// new_plane() scaled (w/2, h/2, 1) and rotated (pi/2, 0, pi/2), applied in float32
    fn plane_verts(&self) -> Vec<[f64; 3]> {
        let e = [std::f32::consts::FRAC_PI_2, 0.0f32, std::f32::consts::FRAC_PI_2];
        let rm = crate::bl32::eul_to_mat3(e);
        let sc = [(self.width / 2.0) as f32, (self.height / 2.0) as f32, 1.0f32];
        let mut m = [[0.0f32; 3]; 3];
        for c in 0..3 {
            for r in 0..3 {
                m[c][r] = rm[c][r] * sc[c];
            }
        }
        [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .iter()
            .map(|&(x, y)| {
                let z = 0.0f32;
                let mut o = [0.0f64; 3];
                for i in 0..3 {
                    o[i] = (x * m[0][i] + y * m[1][i] + m[2][i] * z) as f64;
                }
                o
            })
            .collect()
    }

    /// frame: wire loop -> SOLIDIFY(thickness, offset 1) -> bridge_edge_loops -> SOLIDIFY(depth, offset 1) -> BEVEL
    fn frame(&self) -> Mesh {
        let p = self.plane_verts();
        let t = self.thickness as f32;
        let mut m = Mesh::new();
        for q in &p {
            let c = [q[0] as f32, q[1] as f32, q[2] as f32];
            let d = c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
            let n = if d > 1.0e-35f32 {
                let inv = 1.0f32 / d.sqrt();
                [c[0] * inv, c[1] * inv, c[2] * inv]
            } else {
                [0.0; 3]
            };
            m.pos.push([(c[0] + n[0] * t) as f64, (c[1] + n[1] * t) as f64, (c[2] + n[2] * t) as f64]);
        }
        m.pos.extend(p.iter().cloned());
        bridge_two_loops(&mut m, vec![2, 3, 1, 0], vec![6, 7, 5, 4], 0, 0.0);
        let s = books::solidify_offset(&m, self.depth, 1.0);
        if self.bevel_segments > 0 && std::env::var("WALLSTAGE").is_err() {
            return super::bevel::bevel(&s, super::bevel::Offset::Width(self.bevel_width), self.bevel_segments, 30f64.to_radians()).unwrap_or(s);
        }
        s
    }
}

impl Factory for WallArt {
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let mut a = Asset::default();
        let (w, h, t) = (self.width, self.height, self.thickness);
        a.cuboid([-0.01, -w / 2.0 - t, -h / 2.0 - t], [0.15, w / 2.0 + t, h / 2.0 + t], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
    fn asset(&self, _i: u64) -> Asset {
        let mut r = Rng::seeded(self.seed);
        let inner_m = if self.mirror {
            spec_material("mirror", json!({"cls": "metal.Mirror", "kwargs": {}, "seed": self.seed}), [0.8, 0.85, 0.9])
        } else {
            spec_material("art", json!({"cls": weighted(&mut r, ART), "kwargs": {}, "seed": self.seed}), [0.6, 0.5, 0.4])
        };
        let frame_m = spec_material("frame", json!({"cls": weighted(&mut r, FRAME), "kwargs": {}, "seed": self.seed + 1}), [0.3, 0.2, 0.1]);
        let (hw, hh, t, d) = (self.width / 2.0, self.height / 2.0, self.thickness, self.depth);
        let mut a = Asset::default();
        // the art/mirror plane faces +x at x=0; the frame ring stands `depth` proud of it
        let mut plane = Mesh::new();
        plane.pos = self.plane_verts();
        plane.faces.push(vec![0, 1, 3, 2]);
        plane.uv = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let pb = plane.bounds();
        a.mesh(plane, &inner_m, Some(pb));
        let ring = self.frame();
        let rb = ring.bounds();
        a.mesh(ring, &frame_m, Some(rb));
        a
    }
}

pub fn make(c: &str, rng: &mut Rng) -> Option<Box<dyn Factory>> {
    Some(Box::new(WallArt::new(rng, c == "MirrorFactory")))
}
