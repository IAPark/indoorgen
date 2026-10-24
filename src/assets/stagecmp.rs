//! Surface comparison of two polygon meshes (symmetric nearest-surface distance of vertex / edge-point / face-centre probes), used to
//! validate hand-ported bpy-op assets stage by stage against python dumps (tools: ~/work/agents/storage/*stages.py).
use crate::dhash::HashMap;

type V = [f64; 3];
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn pt_tri(p: V, t: &[V; 3]) -> f64 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let d3 = dot(ab, bp);
        let d4 = dot(ac, bp);
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                let v = d1 / (d1 - d3);
                [a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]]
            } else {
                let cp = sub(p, c);
                let d5 = dot(ab, cp);
                let d6 = dot(ac, cp);
                if d6 >= 0.0 && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                        let w = d2 / (d2 - d6);
                        [a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]]
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
                            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
                            [b[0] + w * (c[0] - b[0]), b[1] + w * (c[1] - b[1]), b[2] + w * (c[2] - b[2])]
                        } else {
                            let denom = 1.0 / (va + vb + vc);
                            let v = vb * denom;
                            let w = vc * denom;
                            [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
                        }
                    }
                }
            }
        }
    };
    let d = sub(p, q);
    dot(d, d).sqrt()
}

pub fn tri_list(v: &[V], f: &[Vec<u32>]) -> Vec<[V; 3]> {
    let mut t = vec![];
    for fc in f {
        for k in 1..fc.len().saturating_sub(1) {
            t.push([v[fc[0] as usize], v[fc[k] as usize], v[fc[k + 1] as usize]]);
        }
    }
    t
}

pub fn probes(v: &[V], f: &[Vec<u32>], max: usize) -> Vec<V> {
    let mut s: Vec<V> = v.to_vec();
    for fc in f {
        let n = fc.len();
        let mut c = [0.0; 3];
        for k in 0..n {
            let (a, b) = (v[fc[k] as usize], v[fc[(k + 1) % n] as usize]);
            for t in [0.5] {
                s.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
            }
            for q in 0..3 {
                c[q] += a[q] / n as f64;
            }
        }
        s.push(c);
    }
    let stride = (s.len() / max).max(1);
    s.into_iter().step_by(stride).collect()
}

struct Grid {
    lo: V,
    cell: f64,
    n: [i64; 3],
    cells: HashMap<(i64, i64, i64), Vec<u32>>,
    tris: Vec<[V; 3]>,
}

impl Grid {
    fn new(tris: Vec<[V; 3]>) -> Grid {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for t in &tris {
            for p in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        let ext = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f64, f64::max).max(1e-9);
        let cell = ext / 32.0;
        let n = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / cell) as i64 + 1);
        let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::default();
        for (i, t) in tris.iter().enumerate() {
            let mut a = [f64::MAX; 3];
            let mut b = [f64::MIN; 3];
            for p in t {
                for k in 0..3 {
                    a[k] = a[k].min(p[k]);
                    b[k] = b[k].max(p[k]);
                }
            }
            let ca = [0, 1, 2].map(|k| ((a[k] - lo[k]) / cell) as i64);
            let cb = [0, 1, 2].map(|k| ((b[k] - lo[k]) / cell) as i64);
            for x in ca[0]..=cb[0] {
                for y in ca[1]..=cb[1] {
                    for z in ca[2]..=cb[2] {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Grid { lo, cell, n, cells, tris }
    }
    fn dist(&self, p: V) -> f64 {
        let c = [0, 1, 2].map(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64);
        let mut r = 1i64;
        let mut best = f64::MAX;
        let maxr = self.n.iter().copied().max().unwrap() + 64;
        loop {
            // scan shell r (cells with chebyshev distance == r), r==1 scans the 3x3x3 block
            let rng = if r == 1 { 0..=1 } else { r..=r };
            let _ = rng;
            let (r0, r1) = if r == 1 { (0, 1) } else { (r, r) };
            for x in c[0] - r1..=c[0] + r1 {
                for y in c[1] - r1..=c[1] + r1 {
                    for z in c[2] - r1..=c[2] + r1 {
                        let cd = (x - c[0]).abs().max((y - c[1]).abs()).max((z - c[2]).abs());
                        if cd < r0 || cd > r1 {
                            continue;
                        }
                        if let Some(l) = self.cells.get(&(x, y, z)) {
                            for &t in l {
                                best = best.min(pt_tri(p, &self.tris[t as usize]));
                            }
                        }
                    }
                }
            }
            // everything within (r) cells is covered: any triangle not seen is at least r*cell away from p
            if best <= r as f64 * self.cell || r > maxr {
                return best;
            }
            r += 1;
        }
    }
}

/// (mean, max) of the symmetric nearest-surface distance (probes limited to `maxp` per side)
pub fn surface_dist(va: &[V], fa: &[Vec<u32>], vb: &[V], fb: &[Vec<u32>], maxp: usize) -> Option<(f64, f64)> {
    let (ta, tb) = (tri_list(va, fa), tri_list(vb, fb));
    if ta.is_empty() || tb.is_empty() {
        return None;
    }
    let (ga, gb) = (Grid::new(ta), Grid::new(tb));
    let mut sum = 0.0;
    let mut cnt = 0usize;
    let mut mx = 0.0f64;
    for (pv, pf, g) in [(va, fa, &gb), (vb, fb, &ga)] {
        for p in probes(pv, pf, maxp) {
            let d = g.dist(p);
            sum += d;
            cnt += 1;
            mx = mx.max(d);
        }
    }
    Some((sum / cnt as f64, mx))
}
