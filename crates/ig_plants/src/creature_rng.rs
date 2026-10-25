//! numpy legacy-RandomState helpers missing from ig_core::rng (dirichlet via legacy gamma).
use ig_core::rng::Rng;

fn legacy_exponential(r: &mut Rng) -> f64 {
    -(1.0 - r.f64()).ln()
}

/// numpy legacy_standard_gamma
pub fn standard_gamma(r: &mut Rng, shape: f64) -> f64 {
    if shape == 1.0 {
        return legacy_exponential(r);
    }
    if shape == 0.0 {
        return 0.0;
    }
    if shape < 1.0 {
        loop {
            let u = r.f64();
            let v = legacy_exponential(r);
            if u <= 1.0 - shape {
                let x = u.powf(1.0 / shape);
                if x <= v {
                    return x;
                }
            } else {
                let y = -((1.0 - u) / shape).ln();
                let x = (1.0 - shape + shape * y).powf(1.0 / shape);
                if x <= v + y {
                    return x;
                }
            }
        }
    } else {
        let b = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * b).sqrt();
        loop {
            let mut x;
            let mut v;
            loop {
                x = r.randn();
                v = 1.0 + c * x;
                if v > 0.0 {
                    break;
                }
            }
            v = v * v * v;
            let u = r.f64();
            if u < 1.0 - 0.0331 * (x * x) * (x * x) {
                return b * v;
            }
            if u.ln() < 0.5 * x * x + b * (1.0 - v + v.ln()) {
                return b * v;
            }
        }
    }
}

/// np.random.dirichlet(alpha)
pub fn dirichlet(r: &mut Rng, alpha: &[f64]) -> Vec<f64> {
    let g: Vec<f64> = alpha.iter().map(|&a| standard_gamma(r, a)).collect();
    let mut acc = 0.0;
    for x in &g {
        acc += x;
    }
    let inv = 1.0 / acc;
    g.iter().map(|x| x * inv).collect()
}

/// np.random.normal(mean, std, n)
pub fn normal_n(r: &mut Rng, mean: f64, std: f64, n: usize) -> Vec<f64> {
    (0..n).map(|_| r.normal(mean, std)).collect()
}

/// np.random.normal(mean, std_array)
pub fn normal_each(r: &mut Rng, mean: f64, stds: &[f64]) -> Vec<f64> {
    stds.iter().map(|&s| r.normal(mean, s)).collect()
}

/// math.clip_gaussian
pub fn clip_gaussian(r: &mut Rng, mean: f64, std: f64, min: f64, max: f64) -> f64 {
    let mut i = 0;
    loop {
        let v = r.normal(mean, std);
        if min <= v && v <= max {
            return v;
        }
        if i == 20 {
            return v.clamp(min, max);
        }
        i += 1;
    }
}
