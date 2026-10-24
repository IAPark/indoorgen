//! Emulation of the draws python factory constructors make on the GLOBAL numpy stream (outside their FixedSeed).
//! `ctor_draws(class, rng)` consumes exactly the same words (and gaussian-cache side effects) as the python __init__
//! chain, and returns the float values that matter to the geometry (see `assets::unseeded()`).
//! Material classes are stubbed but their __init__ draws are reproduced (material_assignments weights are mirrored below).
//! Verified against python with tools/ctorlog.py (real materials, FixedSeed-aware) and against the oracle ctor files.
use crate::rng::Rng;

/// `weighted_sample(reg)`: np.random.choice(classes, p=w/w.sum())
fn wsample(rng: &mut Rng, w: &[f64]) -> usize {
    let s: f64 = w.iter().sum();
    let p: Vec<f64> = w.iter().map(|x| x / s).collect();
    rng.choice_p(&p)
}

/// material_assignments.lampshade = fabrics + LampShade: no material class draws in __init__
const LAMPSHADE_W: [f64; 5] = [1.0, 1.0, 0.3, 1.0, 5.0];
/// material_assignments.furniture_leg: decorative_metal(4) + Wood, GlassVolume, PlasticRough, BrushedBlackMetal(3), BlackPlywood(3)
const FURNITURE_LEG_W: [f64; 9] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0];
/// index of BlackPlywood in furniture_leg
const BLACK_PLYWOOD: usize = 8;

/// materials instantiated with `cls()` and their __init__ draws
fn black_plywood_init(rng: &mut Rng) {
    // shader_shelves_black_wood_sampler
    rng.uniform(1.0, 3.0);
    rng.uniform(0.0, 0.01);
    rng.uniform(0.01, 0.03);
    rng.uniform(0.015, 0.035);
    rng.uniform(0.0, 0.01);
    rng.uniform(0.0, 0.01);
    rng.uniform(0.75, 1.0);
}

fn lamp_base(rng: &mut Rng) {
    // LampFactory.__init__: lampshade_material = weighted_sample(lampshade)(); metal_material = weighted_sample(furniture_leg)()
    wsample(rng, &LAMPSHADE_W);
    if wsample(rng, &FURNITURE_LEG_W) == BLACK_PLYWOOD {
        black_plywood_init(rng);
    }
}


// ---------------------------------------------------------------------------------------------
// materials/text.py (Text / Art / DarkArt): the procedural "art" texture drawn with matplotlib in python. Only its numpy draws matter.
// ---------------------------------------------------------------------------------------------
fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    let step = (b - a) / (n as f64 - 1.0);
    let mut v: Vec<f64> = (0..n).map(|i| i as f64 * step + a).collect();
    if n > 1 {
        v[n - 1] = b;
    }
    v
}

fn hls_to_rgb(h: f64, l: f64, s: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;
    let v = |hue: f64| {
        let hue = hue.rem_euclid(1.0);
        if hue < 1.0 / 6.0 {
            m1 + (m2 - m1) * hue * 6.0
        } else if hue < 0.5 {
            m2
        } else if hue < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0
        } else {
            m1
        }
    };
    [v(h + 1.0 / 3.0), v(h), v(h - 1.0 / 3.0)]
}

fn clip_gaussian(rng: &mut Rng, mean: f64, std: f64, lo: f64, hi: f64) -> f64 {
    let mut i = 0;
    loop {
        let v = rng.normal(mean, std);
        if lo <= v && v <= hi {
            return v;
        }
        if i == 20 {
            return v.clamp(lo, hi);
        }
        i += 1;
    }
}

/// trimesh.points.remove_close count: number of points surviving
fn remove_close_count(pts: &[[f64; 2]], r: f64) -> usize {
    let n = pts.len();
    let mut pairs = vec![];
    let r2 = r * r;
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
    let mut removed = vec![false; n];
    for &(i, j) in &pairs {
        // argmax over the pair's counts, first on ties
        removed[if count[j] > count[i] { j } else { i }] = true;
    }
    n - removed.iter().filter(|&&b| b).count()
}

struct Text {
    colormap: Vec<[f64; 3]>,
    n_patches: i64,
    n_texts: i64,
    n_barcodes: i64,
    barcode_length: i64,
}

const CMAP_N: usize = 20;
impl Text {
    /// Text.__init__(has_barcode)
    fn init(rng: &mut Rng, has_barcode: bool) -> Text {
        let seq = rng.f64() < 0.5;
        let hue: Vec<f64> = { let u = rng.f64(); linspace(0.0, 0.5, CMAP_N).iter().map(|x| (u + x).rem_euclid(1.0)).collect() };
        let mut sat: Vec<f64> = linspace(1.0, 0.5, CMAP_N / 2).into_iter().chain(linspace(0.5, 1.0, CMAP_N / 2)).collect();
        let mut light: Vec<f64>;
        if seq {
            let a = rng.uniform(0.0, 1.0);
            let b = rng.uniform(0.6, 0.8);
            light = linspace(a, b, CMAP_N);
        } else {
            let mid = rng.uniform(0.6, 0.8);
            let a = rng.uniform(0.1, 0.3);
            let first = linspace(a, mid, CMAP_N / 2);
            let b = rng.uniform(0.1, 0.3);
            light = first.into_iter().chain(linspace(mid, b, CMAP_N / 2)).collect();
        }
        let ks = rng.uniform(0.0, 1.0);
        sat.iter_mut().for_each(|x| *x *= ks);
        let kl = rng.uniform(0.5, 1.0);
        light.iter_mut().for_each(|x| *x *= kl);
        let colormap = (0..CMAP_N).map(|i| hls_to_rgb(hue[i], light[i], sat[i])).collect();
        let n_patches = rng.randint(5, 8);
        rng.f64(); // force_horizontal
        let n_texts = rng.randint(2, 4);
        let n_barcodes = if has_barcode && rng.f64() < 0.5 { 1 } else { 0 };
        rng.uniform(0.3, 0.6); // barcode_scale
        let barcode_length = rng.randint(25, 40);
        rng.log_uniform(1.5, 3.0); // barcode_aspect
        Text { colormap, n_patches, n_texts, n_barcodes, barcode_length }
    }

    /// Art.__init__ + DarkArt.__init__
    fn init_dark_art(rng: &mut Rng) -> Text {
        let mut t = Text::init(rng, true);
        t.n_barcodes = 0;
        t.n_texts = 0;
        t.n_patches = rng.randint(10, 15);
        rng.uniform(5.0, 10.0); // darken_scale
        rng.uniform(0.5, 1.0); // darken_ratio
        t
    }

    fn random_color(&self, rng: &mut Rng) -> [f64; 3] {
        let r = rng.f64();
        if r < 0.03 {
            [1.0; 3]
        } else if r < 0.03 + 0.05 {
            [0.0; 3]
        } else {
            self.colormap[rng.randint(0, self.colormap.len() as i64) as usize]
        }
    }

    fn random_colors(&self, rng: &mut Rng) {
        loop {
            let c = self.random_color(rng);
            let d = self.random_color(rng);
            if (0..3).map(|k| (c[k] - d[k]).abs()).sum::<f64>() > 0.2 {
                return;
            }
        }
    }

    /// Text.build_image(bbox=(0,1,0,1))
    fn build_image(&self, rng: &mut Rng) {
        self.random_color(rng); // facecolor
        let n = (self.n_patches + self.n_texts + self.n_barcodes) as usize;
        // get_locs: loc_uniform(0,1,8n) twice, decimate
        let m = 8 * n;
        let xs: Vec<f64> = (0..m).map(|_| rng.uniform(0.1, 0.9)).collect();
        let ys: Vec<f64> = (0..m).map(|_| rng.uniform(0.1, 0.9)).collect();
        let pts: Vec<[f64; 2]> = xs.iter().zip(&ys).map(|(x, y)| [*x, *y]).collect();
        let mut dist = 0.1f64;
        loop {
            let c = remove_close_count(&pts, dist);
            if c <= n || dist > 10.0 {
                dist /= 1.2;
                break;
            }
            dist *= 1.2;
        }
        let cnt = remove_close_count(&pts, dist);
        let mut idx: Vec<usize> = (0..cnt).collect();
        rng.shuffle(&mut idx); // np.random.permutation(culled)
        let nlocs = cnt.min(n);
        let (np_, nt) = (self.n_patches as usize, self.n_texts as usize);
        let npatch = np_.min(nlocs);
        let ntext = nt.min(nlocs.saturating_sub(np_));
        let nbar = nlocs.saturating_sub(np_ + nt);
        // add_divider
        if rng.f64() >= 0.6 {
            if rng.f64() >= 0.7 {
                rng.uniform(5.0, 10.0);
            }
            rng.uniform(0.1, 0.9);
            rng.uniform(0.1, 0.9);
            rng.randint(0, 4);
            self.random_color(rng);
        }
        for _ in 0..npatch {
            rng.log_uniform(0.2, 0.8); // scale_uniform (Art: 0.1,0.5; same word count)
            rng.log_uniform(0.2, 0.8);
            let f = wsample(rng, &[2.0, 4.0, 1.0, 1.0, 1.0, 2.0, 2.0]);
            if rng.f64() < 0.2 {
                rng.uniform(0.5, 0.8);
            }
            rng.f64();
            if rng.f64() >= 0.8 {
                rng.uniform(-30.0, 30.0);
            }
            rng.uniform(0.0, std::f64::consts::PI * 2.0);
            self.random_colors(rng);
            if rng.f64() < 0.3 {
                rng.choice_idx(10);
            }
            rng.uniform(2.0, 5.0);
            match f {
                2 => {
                    rng.uniform(0.0, 360.0);
                    rng.uniform(0.0, 360.0);
                    rng.uniform(0.2, 0.8);
                }
                3 => {
                    rng.randint(3, 9);
                }
                5 => {
                    rng.f64();
                    rng.f64();
                    rng.log_uniform(0.6, 1.5);
                }
                6 => {
                    rng.uniform(0.2, 0.4);
                    rng.choice_idx(10);
                    rng.log_uniform(0.6, 1.5);
                    rng.log_uniform(0.6, 1.5);
                }
                _ => {}
            }
        }
        for _ in 0..ntext {
            // generate_text
            let k = clip_gaussian(rng, 3.0, 7.0, 2.0, 15.0) as i64;
            for _ in 0..k {
                rng.f64();
            }
            rng.choice_idx(10); // font family
            self.random_colors(rng);
            rng.log_uniform(0.75, 1.0);
            clip_gaussian(rng, 0.3, 0.2, 0.2, 0.65);
            rng.uniform(-10.0, 10.0);
            rng.choice_idx(3);
            rng.choice_idx(3);
        }
        for _ in 0..nbar {
            for _ in 0..self.barcode_length {
                rng.randint(0, 2);
            }
        }
    }

    /// shader_text(nw) draws
    fn shader_draws(rng: &mut Rng) {
        rng.uniform(0.2, 1.0);
        if rng.f64() < 0.5 {
            rng.uniform(0.0, 0.4);
        }
        rng.uniform(0.0, 0.5);
        rng.uniform(0.0, 0.2);
    }
}

// ---- fabric shaders (their draws; geometry never depends on them but the global stream does) ----
fn coarse_knit_draws(rng: &mut Rng) {
    for _ in 0..2 { rng.uniform(0.0, 1.0); }
    rng.choice_idx(2);
    for _ in 0..2 { rng.uniform(0.0, 1.0); }
    rng.choice_idx(2);
    for _ in 0..3 { rng.uniform(0.0, 1.0); }
    for _ in 0..3 { rng.uniform(0.0, 1.0); }
    rng.choice_idx(3);
    for _ in 0..3 { rng.uniform(0.0, 1.0); } // knit_res, brick_res, crease_res
    for _ in 0..2 { rng.uniform(0.0, 1.0); }
    rng.choice_idx(2);
    rng.uniform(0.1, 0.9);
}
fn fine_knit_draws(rng: &mut Rng) {
    for _ in 0..3 { rng.uniform(0.2, 1.0); }
    for _ in 0..4 { rng.f64(); }
}
fn leather_draws(rng: &mut Rng) {
    rng.uniform(-1000.0, 1000.0);
    wsample(rng, &[0.7, 0.3]);
    for _ in 0..3 { rng.normal(0.0, 1.0); } // mixture_of_gaussian: normal(mu[3], std[3])
    rng.f64();
    rng.f64();
}
fn sofa_fabric_draws(rng: &mut Rng) {
    for _ in 0..4 { rng.f64(); } // fabric_hsv x3, brightness
}
fn rug_shader_draws(rng: &mut Rng) {
    rng.f64(); // mix factor
    rng.log_uniform(250.0, 500.0);
    rng.f64();
    rng.f64();
    let low_sat = rng.f64() < 0.2;
    let _ = low_sat;
    if low_sat { rng.log_uniform(0.02, 0.05); } else { rng.log_uniform(0.2, 0.4); }
    for _ in 0..3 { rng.f64(); }
    for _ in 0..3 { rng.f64(); }
}

/// ArtFabric / ArtRug generate(): build_image, base shader, text shader
fn art_generate(rng: &mut Rng, t: &Text, rug: bool) {
    t.build_image(rng);
    if rug {
        rug_shader_draws(rng);
    } else {
        match wsample(rng, &[1.0, 1.0, 2.0, 1.0]) {
            0 => coarse_knit_draws(rng),
            1 => fine_knit_draws(rng),
            2 => leather_draws(rng),
            _ => sofa_fabric_draws(rng),
        }
    }
    Text::shader_draws(rng);
}

#[derive(Clone, Copy)]
enum Mat { Coarse, Fine, Leather, Sofa, ArtFabric, ArtRug, Rug }

/// `weighted_sample(list)()` followed by `surface_material_gen()` (instantiate + generate)
fn pick_and_generate(rng: &mut Rng, mats: &[(Mat, f64)]) {
    pick_and_generate_idx(rng, mats);
}

/// returns the index (in `mats`) of the sampled material
fn pick_and_generate_idx(rng: &mut Rng, mats: &[(Mat, f64)]) -> usize {
    let w: Vec<f64> = mats.iter().map(|m| m.1).collect();
    let mi = wsample(rng, &w);
    let m = mats[mi].0;
    match m {
        Mat::Coarse => coarse_knit_draws(rng),
        Mat::Fine => fine_knit_draws(rng),
        Mat::Leather => leather_draws(rng),
        Mat::Sofa => { rng.log_uniform(0.005, 0.01); sofa_fabric_draws(rng) }
        Mat::Rug => { rng.log_uniform(0.01, 0.02); rug_shader_draws(rng) }
        Mat::ArtFabric => { let t = Text::init_dark_art(rng); art_generate(rng, &t, false) }
        Mat::ArtRug => { let t = Text::init_dark_art(rng); art_generate(rng, &t, true) }
    }
    mi
}

fn fabrics_plus(extra: &[(Mat, f64)]) -> Vec<(Mat, f64)> {
    let mut v = vec![(Mat::Coarse, 1.0), (Mat::Fine, 1.0), (Mat::Leather, 0.3), (Mat::Sofa, 1.0)];
    v.extend_from_slice(extra);
    v
}

/// Returns Some(values) for classes whose constructor draws values the geometry needs (Coffee/Side table dims etc.).
/// Classes with no unseeded draws consume nothing. Unknown classes are treated as "no draws".
pub fn ctor_draws(class: &str, rng: &mut Rng) -> Option<Vec<f64>> {
    match class {
        "CoffeeTableFactory" => Some(vec![rng.uniform(1.0, 1.5), rng.uniform(0.6, 0.9), rng.uniform(0.4, 0.5)]),
        "SideTableFactory" => {
            let a = rng.normal(1.0, 0.05);
            let b = rng.normal(1.0, 0.05);
            Some(vec![a, b])
        }
        "CeilingLightFactory" => {
            let mult = rng.uniform(1.0, 3.0); // BevelSharp(mult=U(1,3))
            let amount = rng.uniform(0.001, 0.006);
            let seg = rng.f64(); // `4 if uniform() < 0 else 1`
            Some(vec![mult, amount, seg])
        }
        "LampFactory" | "DeskLampFactory" => {
            lamp_base(rng);
            None
        }
        "FloorLampFactory" => {
            rng.choice_idx(2); // lamp_type = np.random.choice(["FloorLamp1","FloorLamp2"])
            lamp_base(rng);
            None
        }
        "VaseFactory" => {
            let z = rng.uniform(0.17, 0.5);
            let r = rng.uniform(0.3, 0.6);
            Some(vec![z, r])
        }
        "BedFactory" => {
            // rg(sheet_types) (4 weights), sheet_folded, has_cover
            wsample(rng, &[4.0, 4.0, 4.0, 1.0]);
            rng.f64();
            rng.f64();
            None
        }
        "TowelFactory" => {
            // width, length (= width * lu), thickness, fold_type (0 fold / 1 roll), folds, extra_thickness (= thickness * u), material index
            let w = rng.log_uniform(0.3, 0.6);
            let lu = rng.log_uniform(1.0, 1.5);
            let t = rng.log_uniform(0.003, 0.01);
            let ft = wsample(rng, &[2.0, 1.0]); // fold / roll
            let folds = rng.randint(2, 4);
            let eu = rng.uniform(0.2, 0.3);
            let mi = pick_and_generate_idx(rng, &fabrics_plus(&[(Mat::ArtRug, 2.0), (Mat::Rug, 5.0)]));
            Some(vec![w, w * lu, t, ft as f64, folds as f64, t * eu, mi as f64])
        }
        "BlanketFactory" | "ComforterFactory" | "BoxComforterFactory" => {
            rng.log_uniform(0.9, 1.2);
            rng.log_uniform(0.4, 0.7);
            rng.log_uniform(0.004, 0.008);
            pick_and_generate(rng, &fabrics_plus(&[(Mat::ArtFabric, 3.0)]));
            if class == "BoxComforterFactory" {
                rng.uniform(0.3, 0.4);
            }
            None
        }
        "PantsFactory" => {
            rng.log_uniform(0.45, 0.55);
            rng.uniform(0.0, 0.05);
            rng.choice_idx(3);
            rng.f64(); // length (one uniform in every branch)
            rng.uniform(0.1, 0.15);
            rng.log_uniform(0.02, 0.03);
            pick_and_generate(rng, &fabrics_plus(&[(Mat::ArtFabric, 5.0)]));
            None
        }
        "ShirtFactory" => {
            rng.log_uniform(0.45, 0.55);
            rng.uniform(0.25, 0.3);
            rng.uniform(0.1, 0.15);
            rng.choice_idx(2);
            rng.f64(); // sleeve_length
            rng.uniform(0.14, 0.18);
            rng.uniform(0.5, 0.8);
            rng.log_uniform(0.02, 0.03);
            pick_and_generate(rng, &fabrics_plus(&[(Mat::ArtFabric, 5.0)]));
            None
        }
        "BookFactory" => {
            rng.log_uniform(1.0, 1.5);
            rng.log_uniform(1.3, 1.8);
            rng.f64(); // is_paperback
            rng.uniform(0.005, 0.01);
            if rng.f64() >= 0.5 {
                rng.log_uniform(0.002, 0.008);
            }
            rng.uniform(0.002, 0.003);
            // cover: text.Text() then generate()
            let t = Text::init(rng, true);
            t.build_image(rng);
            Text::shader_draws(rng);
            rng.f64(); // scratch
            rng.f64(); // edge_wear
            rng.f64(); // texture_shared
            None
        }
        _ => None,
    }
}

/// number of 32-bit words `after` is ahead of `before` (small distances only)
pub fn words_between(before: &Rng, after: &Rng) -> Option<u32> {
    let sig = |r: &Rng| { let mut c = r.clone(); [c.next_u32(), c.next_u32(), c.next_u32(), c.next_u32()] };
    let target = sig(after);
    let mut c = before.clone();
    for k in 0..4000u32 {
        if sig(&c) == target {
            return Some(k);
        }
        c.next_u32();
    }
    None
}
