//! tableware/can.py CanFactory: exact port (shapely/GEOS outline, solidify, geo_cap extrude / scale / extrude, wrap strip)
use super::gnlite::{self, F3};
use super::*;
use crate::materials::*;
use serde_json::json;

pub fn handles(c: &str) -> bool {
    c == "CanFactory"
}

/// overall extents minus the nominal ones (x half-extent, y half-extent, height; measured)
const CAN_EXTRA: [f64; 3] = [0.0, 0.0, 0.0];

pub struct Can {
    x_length: f64,
    z_length: f64,
    rect: bool,
    skewness: f64,
    fs: u64,
}

impl Can {
    fn new(fs: u64) -> Can {
        let mut r = Rng::seeded(fs);
        let x_length = r.log_uniform(0.05, 0.1);
        let z_length = x_length * r.log_uniform(0.5, 2.5);
        let rect = r.randint(0, 2) == 1;
        let skewness = if r.uniform(0.0, 1.0) < 0.5 { r.uniform(1.0, 2.5) } else { 1.0 };
        let mut c = Can { x_length, z_length, rect, skewness, fs };
        // built to the layout box [width (y), depth (x), height]: the outline is x_length wide in x and x_length / skewness in y
        if let Some(h) = crate::api::size_hint() {
            c.x_length = h[1] / 2.0 - CAN_EXTRA[0];
            c.skewness = (h[1] - 2.0 * CAN_EXTRA[0]) / (h[0] - 2.0 * CAN_EXTRA[1]);
            c.z_length = h[2] - CAN_EXTRA[2];
        }
        c
    }

    /// make_coords(): shapely buffer -> affinity.scale -> boundary.segmentize(0.01) -> coords[:-1]
    fn coords(&self, r: &mut Rng) -> Vec<(f64, f64)> {
        let poly = if !self.rect {
            ig_core::shp::buffer_quad(&ig_core::shp::point(0.0, 0.0), self.x_length, 64)
        } else {
            let side = self.x_length * r.uniform(0.2, 0.8);
            // shapely.box(-side, -side, side, side) (ccw): ring starts at (xmax, ymin)
            let bx = ig_core::shp::polygon(&[[side, -side], [side, side], [-side, side], [-side, -side], [side, -side]], &[]).unwrap();
            ig_core::shp::buffer_quad(&bx, self.x_length - side, 16)
        };
        // affinity.scale(p, yfact=1/skewness) about the bounding box centre
        let mut pts: Vec<(f64, f64)> = ig_core::shp::exterior(&poly).into_iter().map(|p| (p[0], p[1])).collect();
        let (mut minx, mut maxx, mut miny, mut maxy) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in &pts {
            minx = minx.min(p.0);
            maxx = maxx.max(p.0);
            miny = miny.min(p.1);
            maxy = maxy.max(p.1);
        }
        let (x0, y0) = ((minx + maxx) / 2.0, (miny + maxy) / 2.0);
        let (xf, yf) = (1.0, 1.0 / self.skewness);
        let xoff = x0 - x0 * xf;
        let yoff = y0 - y0 * yf;
        for p in pts.iter_mut() {
            *p = (xf * p.0 + 0.0 * p.1 + xoff, 0.0 * p.0 + yf * p.1 + yoff);
        }
        // segmentize(0.01) via GEOS densify on the closed ring
        let ring: Vec<[f64; 2]> = pts.iter().map(|p| [p.0, p.1]).collect();
        let lr = ig_core::shp::linearring(&ring).unwrap();
        let dens = ig_core::shp::coords_of(&ig_core::shp::segmentize(&lr, 0.01));
        let n = dens.len();
        (0..n - 1).map(|i| (dens[i][0], dens[i][1])).collect()
    }
}

fn newell(p: &[F3]) -> F3 {
    let mut n = [0.0f32; 3];
    let mut prev = p[p.len() - 1];
    for &cur in p {
        n[0] += (prev[1] - cur[1]) * (prev[2] + cur[2]);
        n[1] += (prev[2] - cur[2]) * (prev[0] + cur[0]);
        n[2] += (prev[0] - cur[0]) * (prev[1] + cur[1]);
        prev = cur;
    }
    gnlite::normalize(n)
}

fn to_f3(v: [f64; 3]) -> F3 {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}
fn to_v3(v: F3) -> [f64; 3] {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}

/// Extrude Mesh (Faces, Individual off) for meshes whose selected faces form disjoint regions of one face each.
/// Returns the new vertex block start; `offsets[k]` = displacement of the new vertices of selected face k.
fn extrude_single_faces(m: &mut Mesh, sel: &[usize], offsets: &[F3]) {
    let mut sides: Vec<Vec<u32>> = vec![];
    for (si, &fi) in sel.iter().enumerate() {
        let f = m.faces[fi].clone();
        let base = m.pos.len() as u32;
        for &v in &f {
            let p = to_f3(m.pos[v as usize]);
            let o = offsets[si];
            m.pos.push(to_v3([p[0] + o[0], p[1] + o[1], p[2] + o[2]]));
        }
        let new: Vec<u32> = (0..f.len() as u32).map(|k| base + k).collect();
        let k = f.len();
        for i in 0..k {
            let (a, b) = (i, (i + 1) % k);
            sides.push(vec![new[b], new[a], f[a], f[b]]);
        }
        m.faces[fi] = new;
    }
    m.faces.extend(sides);
}

impl Factory for Can {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        Some([2.0 * self.x_length / self.skewness + 2.0 * CAN_EXTRA[1], 2.0 * self.x_length + 2.0 * CAN_EXTRA[0], self.z_length + CAN_EXTRA[2]])
    }
    fn asset(&self, i: u64) -> Asset {
        let mut r = inst_rng(self.fs, i);
        let coords = self.coords(&mut r);
        let n = coords.len();
        let t = self.z_length as f32;
        // --- new_circle + write_co + edge_face_add: one ngon whose loop starts at vertex 1
        let mut m = Mesh::new();
        for &(x, y) in &coords {
            m.pos.push([x as f32 as f64, y as f32 as f64, 0.0]);
        }
        let l: Vec<u32> = (1..n as u32).chain(std::iter::once(0)).collect();
        // --- SOLIDIFY (offset -1): shell moved against the vertex normal
        let lp: Vec<F3> = l.iter().map(|&v| to_f3(m.pos[v as usize])).collect();
        let nrm = newell(&lp);
        let mut s = Mesh::new();
        s.pos = m.pos.clone();
        for p in &m.pos {
            let q = to_f3(*p);
            s.pos.push(to_v3([q[0] - nrm[0] * t, q[1] - nrm[1] * t, q[2] - nrm[2] * t]));
        }
        let nn = n as u32;
        s.faces.push(l.clone());
        let mut top = vec![l[0] + nn];
        top.extend(l[1..].iter().rev().map(|&v| v + nn));
        s.faces.push(top);
        for j in 0..nn {
            let (a, b) = (j, (j + 1) % nn);
            s.faces.push(vec![b, a, a + nn, b + nn]);
        }
        // --- geo_cap: extrude (offset 0) both caps, scale, extrude inwards
        let scale = r.uniform(0.96, 0.98) as f32;
        let depth = r.uniform(0.005, 0.01) as f32;
        let zero = [[0.0f32; 3], [0.0f32; 3]];
        extrude_single_faces(&mut s, &[0, 1], &zero);
        for fi in 0..2 {
            let f = &s.faces[fi];
            let mut c = [0.0f32; 3];
            for &v in f {
                let p = to_f3(s.pos[v as usize]);
                c = [c[0] + p[0], c[1] + p[1], c[2] + p[2]];
            }
            let k = f.len() as f32;
            let c = [c[0] / k, c[1] / k, c[2] / k];
            for &v in f.clone().iter() {
                let p = to_f3(s.pos[v as usize]);
                s.pos[v as usize] = to_v3([c[0] + (p[0] - c[0]) * scale, c[1] + (p[1] - c[1]) * scale, c[2] + (p[2] - c[2]) * scale]);
            }
        }
        let mut offs = [[0.0f32; 3]; 2];
        for fi in 0..2 {
            let pts: Vec<F3> = s.faces[fi].iter().map(|&v| to_f3(s.pos[v as usize])).collect();
            let nf = newell(&pts);
            offs[fi] = [nf[0] * -depth, nf[1] * -depth, nf[2] * -depth];
        }
        extrude_single_faces(&mut s, &[0, 1], &offs);
        // --- wrap strip
        let lowest = self.z_length * r.uniform(0.0, 0.1);
        let highest = self.z_length * r.uniform(0.9, 1.0);
        let mut w = Mesh::new();
        let sc = 1.001f32;
        for &(x, y) in &coords {
            for z in [lowest, highest] {
                let (xf, yf) = (x as f32, y as f32);
                w.pos.push(to_v3([xf * sc, yf * sc, z as f32]));
            }
        }
        for k in 0..nn {
            let k1 = (k + 1) % nn;
            w.faces.push(vec![2 * k, 2 * k + 1, 2 * k1 + 1, 2 * k1]);
        }
        let metal = spec_material("can_metal", json!({"cls": "metal.MetalBasic", "kwargs": {}, "seed": self.fs}), [0.7, 0.7, 0.7]);
        let wrap = spec_material("can_wrap", json!({"cls": "text.Text", "kwargs": {}, "seed": self.fs}), [0.6, 0.5, 0.4]);
        let mut a = Asset::default();
        let sb = s.bounds();
        a.mesh(s, &metal, Some(sb));
        let wb = w.bounds();
        a.mesh(w, &wrap, Some(wb));
        a
    }
}

pub fn make(_c: &str, _rng: &mut Rng, fs: u64) -> Option<Box<dyn Factory>> {
    Some(Box::new(Can::new(fs)))
}
