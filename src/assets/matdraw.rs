//! numpy.random consumption of infinigen material generators (`weighted_sample(table)()()`), reproduced draw-for-draw
//! without building any shader. Only the draws matter: they advance the global stream the factory parameters are read from.
use crate::rng::Rng;

/// `weighted_sample(reg)`: np.random.choice(classes, p=w/sum(w)) -> index
pub fn weighted_index(r: &mut Rng, weights: &[f64]) -> usize {
    let s: f64 = weights.iter().sum();
    let p: Vec<f64> = weights.iter().map(|w| w / s).collect();
    r.choice_p(&p)
}

pub type Table = &'static [(&'static str, f64)];

pub const DECORATIVE_METAL: Table = &[("metal.BrushedMetal", 1.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 1.0)];
pub const DECORATIVE_HARD: Table = &[
    ("metal.BrushedMetal", 1.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 1.0),
    ("ceramic.VaseCeramic", 4.0),
    ("ceramic.ColoredGlass", 2.0),
    ("ceramic.Marble", 1.0),
    ("ceramic.GlassVolume", 2.0),
    ("ceramic.Ceramic", 1.0),
];
pub const PLASTICS: Table = &[("plastic.Plastic", 1.0), ("plastic.PlasticRough", 1.0), ("plastic.PlasticTranslucent", 1.0)];
pub const METALS: Table = &[
    ("metal.MetalBasic", 3.0),
    ("metal.BrushedMetal", 2.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 0.7),
    ("metal.BrushedBlackMetal", 0.3),
];
pub const GRAPHICDESIGN: Table = &[("text.Text", 1.0), ("dev.BasicBSDF", 0.1)];
pub const FRONT_MAYBEGLASS: Table = &[
    ("metal.MetalBasic", 3.0),
    ("metal.BrushedMetal", 2.0),
    ("metal.GalvanizedMetal", 1.0),
    ("metal.GrainedMetal", 1.0),
    ("metal.HammeredMetal", 0.7),
    ("metal.BrushedBlackMetal", 0.3),
    ("plastic.PlasticRough", 2.0),
    ("metal.BlackGlass", 4.0),
    ("ceramic.Glass", 10.0),
];
pub const POTTING_SOIL: Table = &[("terrain.Mud", 1.0), ("terrain.Sand", 1.0), ("terrain.Soil", 3.0), ("terrain.Dirt", 6.0)];

/// weighted_sample(table)() : class chosen + instantiated (constructor draws only)
pub fn sample_init(r: &mut Rng, t: Table) -> &'static str {
    let w: Vec<f64> = t.iter().map(|x| x.1).collect();
    let name = t[weighted_index(r, &w)].0;
    init(r, name);
    name
}

/// weighted_sample(table)()() : class chosen, instantiated and called
pub fn sample_call(r: &mut Rng, t: Table) -> &'static str {
    let name = sample_init(r, t);
    call(r, name);
    name
}

fn log_uniform(r: &mut Rng, a: f64, b: f64) -> f64 {
    r.log_uniform(a, b)
}

// colors.py
fn metal_natural_hsv(r: &mut Rng) {
    r.choice_p(&[1.0]); // palette weighted_sample: one double
    r.uniform(-0.1, 0.1);
    r.uniform(-0.1, 0.1);
    log_uniform(r, 0.5, 0.2);
}
pub fn metal_hsv(r: &mut Rng) {
    if r.uniform(0.0, 1.0) < 0.2 {
        metal_natural_hsv(r);
        return;
    }
    r.uniform(0.0, 1.0);
    r.uniform(0.3, 0.6);
    log_uniform(r, 0.02, 0.5);
}

/// constructor-time draws of a material class
pub fn init(r: &mut Rng, name: &str) {
    match name {
        "wood.WhitePlywood" => {
            r.uniform(0.7, 1.0);
            r.normal(0.0, 0.005);
            r.normal(0.0, 0.005);
            r.normal(0.0, 0.005);
            r.uniform(0.7, 1.0);
        }
        "wood.BlackPlywood" => {
            r.uniform(1.0, 3.0);
            r.uniform(0.0, 0.01);
            r.uniform(0.01, 0.03);
            r.uniform(0.015, 0.035);
            r.uniform(0.0, 0.01);
            r.uniform(0.0, 0.01);
            r.uniform(0.75, 1.0);
        }
        "wood.BlondePlywood" => {
            r.uniform(0.03, 0.09);
            r.uniform(0.5, 0.7);
            r.uniform(0.7, 1.0);
            r.uniform(0.02, 0.06);
            r.uniform(0.6, 1.0);
            r.uniform(0.3, 0.6);
            r.uniform(0.03, 0.05);
            r.uniform(0.6, 1.0);
            r.uniform(0.1, 0.3);
            r.uniform(1.0, 3.0);
            r.uniform(0.75, 1.0);
        }
        _ => {}
    }
}

fn rough_plastic(r: &mut Rng) {
    r.uniform(0.0, 1.0);
    r.uniform(-1000.0, 1000.0);
    r.uniform(0.0, 1.0);
    r.uniform(0.5, 0.8);
    log_uniform(r, 0.01, 0.5);
}
fn translucent_plastic(r: &mut Rng) {
    r.uniform(0.0, 1.0);
    r.uniform(0.5, 0.8);
    log_uniform(r, 0.4, 0.8);
}

/// draws of `Cls()()` after construction (the `()` call that builds the shader)
pub fn call(r: &mut Rng, name: &str) {
    match name {
        "metal.BrushedMetal" | "metal.GalvanizedMetal" => {
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
        }
        "metal.GrainedMetal" => {
            r.uniform(0.0, 1.0);
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
        }
        "metal.HammeredMetal" => {
            r.uniform(-1000.0, 1000.0);
            metal_hsv(r);
            log_uniform(r, 0.8, 1.2);
            log_uniform(r, 0.001, 0.003);
        }
        "metal.MetalBasic" => {
            metal_hsv(r);
            r.uniform(10.0, 25.0);
            r.uniform(0.0, 0.2);
            r.uniform(0.4, 0.7);
            r.uniform(0.5, 1.0);
        }
        "lamp_shaders.LampShade" => {
            r.uniform(0.03, 0.08);
            r.uniform(0.5, 0.8);
            r.uniform(0.05, 0.2);
        }
        "fabric.FineKnitFabric" => {
            r.uniform(0.2, 1.0);
            r.uniform(0.2, 1.0);
            r.uniform(0.2, 1.0);
            r.uniform(0.0, 1.0);
            r.uniform(100.0, 300.0);
            r.uniform(0.75, 1.33);
            r.uniform(0.001, 0.005);
        }
        "ceramic.VaseCeramic" => {
            r.uniform(0.0, 1.0);
            r.uniform(0.0, 0.75);
            r.uniform(0.0, 0.3);
        }
        "ceramic.ColoredGlass" => {
            r.uniform(0.0, 1.0);
            r.uniform(0.0, 0.2);
            r.uniform(0.05, 0.2);
        }
        "ceramic.GlassVolume" => {
            if r.uniform(0.0, 1.0) >= 0.3 {
                r.uniform(0.0, 1.0);
                r.uniform(0.5, 0.9);
                r.uniform(0.6, 0.9);
            }
        }
        "ceramic.Ceramic" => {
            if r.uniform(0.0, 1.0) < 0.8 {
                r.uniform(0.0, 1.0);
                r.uniform(0.2, 0.4);
                log_uniform(r, 0.3, 0.6);
            } else {
                log_uniform(r, 0.3, 0.6);
            }
            log_uniform(r, 20.0, 40.0);
            log_uniform(r, 20.0, 40.0);
            r.uniform(0.02, 0.05);
            log_uniform(r, 0.001, 0.005);
            log_uniform(r, 20.0, 40.0);
        }
        "plastic.Plastic" => {
            let rough = r.uniform(0.0, 1.0);
            let transl = r.uniform(0.0, 1.0);
            r.uniform(0.0, 1.0); // clear
            if rough > transl {
                rough_plastic(r);
            } else {
                translucent_plastic(r);
            }
        }
        "text.Text" => super::textdraw::text_ctor_and_generate(r),
        "dev.BasicBSDF" => {
            r.uniform(0.05, 0.95);
            r.uniform(0.05, 0.95);
            r.uniform(0.05, 0.95);
            r.normal(0.6, 0.3);
            if r.uniform(0.0, 1.0) < 0.3 {
                r.uniform(0.0, 1.0);
            }
            if r.uniform(0.0, 1.0) >= 0.8 {
                r.uniform(0.0, 0.2);
            }
        }
        "plastic.PlasticRough" => rough_plastic(r),
        "plastic.PlasticTranslucent" => translucent_plastic(r),
        "wood.Wood" => {
            r.randint(0, 49); // bark_hsv: np.random.choice(wood_colors)
            r.uniform(0.0, 0.05);
            r.uniform(-0.3, 0.2);
            log_uniform(r, 0.2, 20.0);
            r.uniform(0.0, 1.0);
            log_uniform(r, 0.0002, 0.01);
            r.uniform(0.0, 0.4);
            log_uniform(r, 40.0, 50.0);
            r.uniform(0.0, 0.8);
            r.uniform(0.0, 1.4);
        }
        // no draws: Marble, Glass, BrushedBlackMetal, WhiteMetal, plywoods (ctor only), terrain
        _ => {}
    }
}
