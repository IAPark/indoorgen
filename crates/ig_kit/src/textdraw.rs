//! RNG-exact replay of infinigen/assets/materials/text.py `Text()` construction + `Text()()` material generation
//! (matplotlib rendering is skipped; only the numpy.random consumption and the colour decisions that feed back into
//! that consumption are reproduced) and of the Plaster material generator.
use crate::geomkit::clip_gaussian;
use crate::rng::Rng;

const N_FONTS: i64 = 10;
const N_BOXSTYLES: i64 = 10;
const N_HATCHES: i64 = 10;

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    let div = (n - 1) as f64;
    let delta = b - a;
    let step = delta / div;
    let mut y: Vec<f64> = (0..n)
        .map(|i| if step == 0.0 { (i as f64 / div) * delta + a } else { i as f64 * step + a })
        .collect();
    y[n - 1] = b;
    y
}

fn hls_to_rgb(h: f64, l: f64, s: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - (l * s) };
    let m1 = 2.0 * l - m2;
    let one_third = 1.0 / 3.0;
    let two_third = 2.0 / 3.0;
    let v = |m1: f64, m2: f64, hue: f64| -> f64 {
        let hue = py_mod(hue, 1.0);
        if hue < 1.0 / 6.0 {
            return m1 + (m2 - m1) * hue * 6.0;
        }
        if hue < 0.5 {
            return m2;
        }
        if hue < two_third {
            return m1 + (m2 - m1) * (two_third - hue) * 6.0;
        }
        m1
    };
    [v(m1, m2, h + one_third), v(m1, m2, h), v(m1, m2, h - one_third)]
}

fn py_mod(a: f64, b: f64) -> f64 {
    let m = a % b;
    if m != 0.0 && ((m < 0.0) != (b < 0.0)) {
        m + b
    } else {
        m
    }
}

fn colormap(r: &mut Rng) -> Vec<[f64; 3]> {
    let count = 20usize;
    let mut sat: Vec<f64> = linspace(1.0, 0.5, 10);
    sat.extend(linspace(0.5, 1.0, 10));
    let (hue, mut light);
    if r.uniform(0.0, 1.0) < 0.5 {
        // sequential
        let h0 = r.uniform(0.0, 1.0);
        let lin = linspace(0.0, 0.5, count);
        hue = lin.iter().map(|x| py_mod(h0 + x, 1.0)).collect::<Vec<_>>();
        let a = r.uniform(0.0, 1.0);
        let b = r.uniform(0.6, 0.8);
        light = linspace(a, b, count);
    } else {
        let h0 = r.uniform(0.0, 1.0);
        let lin = linspace(0.0, 0.5, count);
        hue = lin.iter().map(|x| py_mod(h0 + x, 1.0)).collect::<Vec<_>>();
        let mid = r.uniform(0.6, 0.8);
        let a = r.uniform(0.1, 0.3);
        let mut l = linspace(a, mid, 10);
        let b = r.uniform(0.1, 0.3);
        l.extend(linspace(mid, b, 10));
        light = l;
    }
    let ks = r.uniform(0.0, 1.0);
    for s in sat.iter_mut() {
        *s *= ks;
    }
    let kl = r.uniform(0.5, 1.0);
    for l in light.iter_mut() {
        *l *= kl;
    }
    (0..count).map(|i| hls_to_rgb(hue[i], light[i], sat[i])).collect()
}

struct Tx {
    cmap: Vec<[f64; 3]>,
}

impl Tx {
    fn random_color(&self, r: &mut Rng) -> [f64; 3] {
        let u = r.uniform(0.0, 1.0);
        if u < 0.03 {
            [1.0, 1.0, 1.0]
        } else if u < 0.03 + 0.05 {
            [0.0, 0.0, 0.0]
        } else {
            self.cmap[r.randint(0, self.cmap.len() as i64) as usize]
        }
    }
    fn random_colors(&self, r: &mut Rng) {
        loop {
            let c = self.random_color(r);
            let d = self.random_color(r);
            let s = (c[0] - d[0]).abs() + (c[1] - d[1]).abs() + (c[2] - d[2]).abs();
            if s > 0.2 {
                return;
            }
        }
    }
}

/// trimesh.points.remove_close: indices kept
fn remove_close(pts: &[[f64; 2]], radius: f64) -> Vec<usize> {
    let n = pts.len();
    let mut pairs: Vec<(usize, usize)> = vec![];
    let r2 = radius * radius;
    for i in 0..n {
        for j in i + 1..n {
            let (dx, dy) = (pts[i][0] - pts[j][0], pts[i][1] - pts[j][1]);
            if dx * dx + dy * dy <= r2 {
                pairs.push((i, j));
            }
        }
    }
    let mut count = vec![0usize; n];
    for &(i, j) in &pairs {
        count[i] += 1;
        count[j] += 1;
    }
    let mut mask = vec![true; n];
    for &(i, j) in &pairs {
        let hi = if count[j] > count[i] { j } else { i };
        mask[hi] = false;
    }
    (0..n).filter(|&i| mask[i]).collect()
}

fn decimate(pts: &[[f64; 2]], n: usize, r: &mut Rng) -> Vec<[f64; 2]> {
    let mut dist = 0.1;
    let ratio = 1.2;
    loop {
        let culled = remove_close(pts, dist);
        if culled.len() <= n || dist > 10.0 {
            dist /= ratio;
            break;
        }
        dist *= ratio;
    }
    let keep = remove_close(pts, dist);
    let mut culled: Vec<[f64; 2]> = keep.iter().map(|&i| pts[i]).collect();
    r.shuffle(&mut culled);
    culled.truncate(n);
    culled
}

fn loc_uniform_rng(r: &mut Rng) -> f64 {
    // loc_uniform(0, 1): uniform(0 + 0.1 * 1, 0 + (1 - 0.1) * 1)
    r.uniform(0.0 + 0.1 * (1.0 - 0.0), 0.0 + (1.0 - 0.1) * (1.0 - 0.0))
}

/// `text.Text()` then its call (material generation): consumes exactly python's numpy.random words.
pub fn text_ctor_and_generate(r: &mut Rng) {
    let cmap = colormap(r);
    let tx = Tx { cmap };
    let n_patches = r.randint(5, 8) as usize;
    let force_horizontal = r.uniform(0.0, 1.0) < 0.75;
    let n_texts = r.randint(2, 4) as usize;
    let _ = force_horizontal;
    let n_barcodes = if r.uniform(0.0, 1.0) < 0.5 { 1usize } else { 0 };
    let _barcode_scale = r.uniform(0.3, 0.6);
    let barcode_length = r.randint(25, 40);
    let _aspect = r.log_uniform(1.5, 3.0);

    // build_image
    tx.random_color(r); // facecolor
    let n = n_patches + n_texts + n_barcodes;
    let m = 8 * n;
    let xs: Vec<f64> = (0..m).map(|_| loc_uniform_rng(r)).collect();
    let ys: Vec<f64> = (0..m).map(|_| loc_uniform_rng(r)).collect();
    let pts: Vec<[f64; 2]> = xs.iter().zip(ys.iter()).map(|(&x, &y)| [x, y]).collect();
    let locs = decimate(&pts, n, r);
    // divider
    if r.uniform(0.0, 1.0) >= 0.6 {
        if r.uniform(0.0, 1.0) >= 0.7 {
            r.uniform(5.0, 10.0);
        }
        loc_uniform_rng(r);
        loc_uniform_rng(r);
        r.randint(0, 4);
        tx.random_color(r);
    }
    let patch_p = [2.0 / 13.0, 4.0 / 13.0, 1.0 / 13.0, 1.0 / 13.0, 1.0 / 13.0, 2.0 / 13.0, 2.0 / 13.0];
    let np_ = n_patches.min(locs.len());
    for _ in 0..np_ {
        r.log_uniform(0.2, 0.8);
        r.log_uniform(0.2, 0.8);
        let fn_ = r.choice_p(&patch_p);
        // kwargs
        if r.uniform(0.0, 1.0) < 0.2 {
            r.uniform(0.5, 0.8);
        }
        r.uniform(0.0, 1.0); // fill
        if r.uniform(0.0, 1.0) >= 0.8 {
            r.uniform(-30.0, 30.0);
        }
        r.uniform(0.0, std::f64::consts::PI * 2.0);
        tx.random_colors(r);
        if r.uniform(0.0, 1.0) < 0.3 {
            r.randint(0, N_HATCHES);
        }
        r.uniform(2.0, 5.0);
        match fn_ {
            2 => {
                r.uniform(0.0, 360.0);
                r.uniform(0.0, 360.0);
                r.uniform(0.2, 0.8);
            }
            3 => {
                r.randint(3, 9);
            }
            5 => {
                r.uniform(0.0, 1.0);
                r.uniform(0.0, 1.0);
                r.log_uniform(0.6, 1.5);
            }
            6 => {
                r.uniform(0.2, 0.4);
                r.randint(0, N_BOXSTYLES);
                r.log_uniform(0.6, 1.5);
                r.log_uniform(0.6, 1.5);
            }
            _ => {}
        }
    }
    // texts
    let end_t = (n_patches + n_texts).min(locs.len());
    let nt = end_t.saturating_sub(n_patches.min(locs.len()));
    let character_p = {
        let mut w = vec![1.5f64; 26];
        w.extend(vec![0.5; 26]);
        w.extend(vec![0.5; 10]);
        let s: f64 = w.iter().sum();
        w.iter().map(|x| x / s).collect::<Vec<f64>>()
    };
    for _ in 0..nt {
        let nchar = clip_gaussian(r, 3.0, 7.0, 2.0, 15.0) as i64;
        // np.random.choice(set, size=n, p=p): n doubles
        for _ in 0..nchar.max(0) {
            r.f64();
        }
        let _ = &character_p;
        r.randint(0, N_FONTS);
        tx.random_colors(r);
        r.log_uniform(0.75, 1.0);
        clip_gaussian(r, 0.3, 0.2, 0.2, 0.65);
        r.uniform(-10.0, 10.0);
        r.randint(0, 3);
        r.randint(0, 3);
    }
    // barcodes
    let nb = locs.len().saturating_sub(n_patches + n_texts);
    for _ in 0..nb {
        for _ in 0..barcode_length {
            r.next_u32();
        }
    }
    // shader
    r.uniform(0.2, 1.0);
    if r.f64() < 0.5 {
        r.uniform(0.0, 0.4);
    }
    r.uniform(0.0, 0.5);
    r.uniform(0.0, 0.2);
}

/// materials/ceramic/plaster.py Plaster()() : draws only
pub fn plaster_draws(r: &mut Rng) {
    let colored = r.uniform(0.0, 1.0) < 0.4;
    r.uniform(0.0, 1.0); // hue
    r.log_uniform(0.5, 1.0);
    r.uniform(0.6, 1.0);
    if colored {
        r.uniform(0.3, 0.5);
        r.uniform(-0.1, 0.1);
        r.uniform(0.3, 0.5);
    } else {
        r.uniform(-0.1, 0.1);
    }
    r.log_uniform(15.0, 30.0);
    r.log_uniform(15.0, 30.0);
    r.log_uniform(4.0, 8.0);
    r.uniform(0.3, 0.5);
    r.uniform(0.2, 0.3);
    r.log_uniform(0.0001, 0.0003);
    r.uniform(1e3, 2e3);
    r.uniform(0.7, 0.8);
}
