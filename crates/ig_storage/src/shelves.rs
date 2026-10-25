//! shelves/*.py: LargeShelf, SimpleBookcase, CellShelf, TVStand, SimpleDesk, SidetableDesk, SingleCabinet, KitchenCabinet, KitchenSpace/Island
use super::*;
use crate::materials::*;

pub fn handles(c: &str) -> bool {
    matches!(
        c,
        "LargeShelfFactory" | "SimpleBookcaseFactory" | "CellShelfFactory" | "TVStandFactory" | "SimpleDeskFactory" | "SidetableDeskFactory" | "SingleCabinetFactory" | "KitchenCabinetFactory" | "KitchenSpaceFactory" | "KitchenIslandFactory"
    )
}

fn clip(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

fn shelf_mat(rng: &mut Rng, p: &[f64]) -> Material {
    // ["white", "black_wood", "wood"] style choices
    let i = rng.choice_p(p);
    plywood(["white", "black_wood", "wood"][i.min(2)], false, 0)
}

/// same material with a deterministic per-asset spec seed
fn respec(m: &Material, seed: u64) -> Material {
    let mut m = m.clone();
    if let Some(sp) = m.spec.as_mut() {
        sp["seed"] = serde_json::json!(seed);
    }
    m
}

/// plywood.py shader_shelves_{white,black_wood,wood}[_z] as a render spec (z = z_axis_texture variant, used by kitchen/door frames and panels)
fn plywood(kind: &str, z: bool, seed: u64) -> Material {
    let (name, rgb) = match kind {
        "white" => ("shelf_white", [0.82, 0.8, 0.76]),
        "black_wood" => ("shelf_black_wood", [0.08, 0.06, 0.05]),
        _ => ("shelf_wood", [0.45, 0.3, 0.17]),
    };
    let cls = match (kind, z) {
        ("white", _) => "wood.WhitePlywood".to_string(),
        ("black_wood", false) => "wood.BlackPlywood".to_string(),
        ("wood", false) => "wood.BlondePlywood".to_string(),
        (k, true) => format!("shader.wood.plywood.shader_shelves_{k}_z"),
        _ => unreachable!(),
    };
    let mut m = spec_material(&format!("{name}{}", if z { "_z" } else { "" }), serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb);
    m.roughness = 0.5;
    m
}

fn shader_spec(name: &str, shader: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, serde_json::json!({"cls": format!("shader.{shader}"), "kwargs": {}, "seed": seed}), rgb)
}

fn cls_spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

/// get_shelf_material("metal"): choice over 5 shader functions (one np.random.choice draw)
fn shelf_metal(rng: &mut Rng, seed: u64) -> Material {
    let cls = ["metal.BrushedMetal", "metal.GalvanizedMetal", "metal.GrainedMetal", "metal.HammeredMetal", "metal.MetalBasic"][rng.choice_idx(5)];
    cls_spec("shelf_metal", cls, seed, [0.5, 0.5, 0.52])
}

/// get_shelf_material("white"): choice([shader_shelves_white, shader_rough_plastic], p=[0.6, 0.4])
fn shelf_white_choice(rng: &mut Rng, seed: u64) -> Material {
    if rng.choice_p(&[0.6, 0.4]) == 0 { plywood("white", false, seed) } else { cls_spec("shelf_white_plastic", "plastic.PlasticRough", seed, [0.85, 0.84, 0.8]) }
}

/// kind chosen under FixedSeed(factory_seed): the first np.random.choice of a fresh stream (cabinet factories' get_material_params)
fn fixed_kind(fseed: u64, p: &[f64]) -> &'static str {
    ["white", "black_wood", "wood"][Rng::seeded(fseed).choice_p(p).min(2)]
}

/// Frame of boards: shared by LargeShelf / SingleCabinet / KitchenCabinet
struct Frame {
    depth: f64,
    cell_w: Vec<f64>,
    cell_h: Vec<f64>,
    side_t: f64,
    div_t: f64,
    bottom_h: f64,
    back_t: f64,
}

impl Frame {
    fn width(&self) -> f64 {
        let n = self.cell_w.len();
        (n - 1) as f64 * self.side_t * 2.0 + (n - 1) as f64 * 0.001 + self.cell_w.iter().sum::<f64>()
    }
    fn height(&self) -> f64 {
        (self.cell_h.len() + 1) as f64 * self.div_t + self.bottom_h + self.cell_h.iter().sum::<f64>()
    }
    fn build(&self, a: &mut Asset, m: &Material) {
        let (w, h, d) = (self.width(), self.height(), self.depth);
        // x: depth (back at -d/2 .. front at +d/2), y: width, z: height
        let x0 = -d / 2.0;
        let x1 = d / 2.0;
        a.cuboid([x0, -(w + 2.0 * self.side_t) / 2.0, 0.0], [x0 + self.back_t, (w + 2.0 * self.side_t) / 2.0, h], m);
        // side boards
        let n = self.cell_w.len();
        let mut dist = -(w + self.side_t) / 2.0;
        let mut xs = vec![dist];
        for cw in &self.cell_w {
            dist += self.side_t + cw;
            xs.push(dist);
            dist += self.side_t + 0.001;
            xs.push(dist);
        }
        xs.pop();
        for y in &xs {
            a.cuboid([x0, y - self.side_t / 2.0, 0.0], [x1, y + self.side_t / 2.0, h], m);
        }
        // division boards per cell
        let mut zs = vec![self.bottom_h + self.div_t / 2.0];
        let mut z = zs[0];
        for ch in &self.cell_h {
            z += ch + self.div_t;
            zs.push(z);
        }
        for i in 0..n {
            let yc = (xs[2 * i] + xs[2 * i + 1]) / 2.0;
            let cw = self.cell_w[i];
            for z in &zs {
                a.board([x0 + self.back_t, yc - cw / 2.0, z - self.div_t / 2.0], [x1, yc + cw / 2.0, z + self.div_t / 2.0], m);
            }
        }
    }
}

fn large_shelf_frame(rng: &mut Rng, dims: [f64; 3], bottom_h: f64, side_t: f64, div_t: f64, cell_h: Vec<f64>, cell_w: Vec<f64>, depth: f64) -> Frame {
    let _ = (rng, dims);
    Frame { depth, cell_w, cell_h, side_t, div_t, bottom_h, back_t: 0.01 }
}

pub struct LargeShelf {
    fseed: u64,
}

impl LargeShelf {
    fn new(_rng: &mut Rng, factory_seed: u64) -> LargeShelf {
        // the python factory has no init draws: LargeShelfFactory.get_asset_params samples everything per instance
        LargeShelf { fseed: factory_seed }
    }
}

impl LargeShelf {
    fn params(&self, inst: u64) -> (cabnodes::ShelfParams, Material) {
        let mut r = inst_rng(self.fseed, inst);
        // sample_params()
        let dims = [r.uniform(0.25, 0.35), r.uniform(0.3, 2.0), r.uniform(0.9, 2.0)];
        let bottom_h = 0.083;
        let depth = dims[0] - 0.01;
        let num_h = (((dims[2] - 0.083) / 0.3) as usize).max(1);
        let cell_h = vec![(dims[2] - 0.083) / num_h as f64; num_h];
        let num_v = ((dims[1] / 0.5) as usize).max(1);
        let cell_w = vec![dims[1] / num_v as f64; num_v];
        // get_asset_params(): remaining draws in python order
        let side_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let y_gap = r.uniform(0.01, 0.05);
        let div_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let screw_depth = r.uniform(0.001, 0.004);
        let screw_r = r.uniform(0.001, 0.004);
        let screw_wgap = r.uniform(0.0, 0.02);
        for (lo, hi) in [(0.025, 0.06), (0.05, 0.1), (0.01, 0.025), (0.002, 0.005), (0.0, 0.05)] {
            r.uniform(lo, hi);
        }
        let mat = shelf_mat(&mut r, &[0.4, 0.3, 0.3]);
        let mat = respec(&mat, self.fseed * 1000 + inst);
        let mut p = cabnodes::ShelfParams { shelf_depth: depth, side_t, back_t: 0.01, y_gap, bottom_h, div_t, screw_depth, screw_r, screw_wgap, cell_w, cell_h, ..Default::default() };
        p.derive();
        (p, mat)
    }
}

impl Factory for LargeShelf {
    fn asset(&self, inst: u64) -> Asset {
        let (p, mat) = self.params(inst);
        cabnodes::shelf_frame(&p, &mat, &mat)
    }
    fn exact(&self, inst: u64) -> Option<cabnodes::ExactMesh> {
        let (p, _) = self.params(inst);
        Some(cabnodes::shelf_gmesh(&p, true).to_exact())
    }
}

pub struct SimpleBookcase {
    fseed: u64,
}

impl SimpleBookcase {
    fn new(_rng: &mut Rng, factory_seed: u64) -> SimpleBookcase {
        // python factory has no init draws; everything is sampled per instance in get_asset_params
        SimpleBookcase { fseed: factory_seed }
    }
}

pub use super::cabnodes::BookParams;

impl SimpleBookcase {
    fn params(&self, i: u64) -> BookParams {
        let mut r = inst_rng(self.fseed, i);
        let dims = [r.uniform(0.25, 0.4), r.uniform(0.5, 0.7), r.uniform(0.7, 0.9)];
        let (d, w, h) = (dims[0] - 0.015, dims[1], dims[2]);
        let st = r.uniform(0.005, 0.03);
        let dt = clip(r.normal(0.015, 0.005), 0.005, 0.025);
        let gap = clip(r.normal(0.14, 0.05), 0.0, 0.2);
        let bt = r.uniform(0.01, 0.02);
        let screw_depth = r.uniform(0.002, 0.008);
        let screw_r = r.uniform(0.003, 0.008);
        let screw_gap = r.uniform(0.03, 0.1);
        let at = r.uniform(0.002, 0.005);
        let aw = r.uniform(0.01, 0.04);
        let atl = r.uniform(0.03, 0.1);
        let abl = r.uniform(0.02, 0.05);
        BookParams { d, w, h, st, dt, gap, bt, screw_depth, screw_r, screw_gap, at, aw, atl, abl }
    }
}

impl Factory for SimpleBookcase {
    fn exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        Some(cabnodes::bookcase_gmesh(&self.params(i)).to_exact())
    }
    fn asset(&self, i: u64) -> Asset {
        let _bp = self.params(i);
        let mut r = inst_rng(self.fseed, i);
        // sample_params(): Dimensions (depth+0.015, width, height)
        let dims = [r.uniform(0.25, 0.4), r.uniform(0.5, 0.7), r.uniform(0.7, 0.9)];
        let (d, w, h) = (dims[0] - 0.015, dims[1], dims[2]);
        // get_asset_params() draws in python order
        let st = r.uniform(0.005, 0.03);
        let dt = clip(r.normal(0.015, 0.005), 0.005, 0.025);
        let gap = clip(r.normal(0.14, 0.05), 0.0, 0.2);
        let bt = r.uniform(0.01, 0.02);
        let screw_depth = r.uniform(0.002, 0.008);
        let screw_r = r.uniform(0.003, 0.008);
        let screw_gap = r.uniform(0.03, 0.1);
        let at = r.uniform(0.002, 0.005);
        let aw = r.uniform(0.01, 0.04);
        let atl = r.uniform(0.03, 0.1);
        let abl = r.uniform(0.02, 0.05);
        // get_asset_params tail: frame_material = get_shelf_material("white"), metal_material = get_shelf_material("metal") (stream continues after the 11 draws above)
        let m = shelf_white_choice(&mut r, self.fseed * 1000 + i);
        let metal = shelf_metal(&mut r, self.fseed * 1000 + i + 1);
        let mut a = Asset::default();
        use cabnodes::*;
        let push = |a: &mut Asset, mesh: Mesh, mt: &Material, sup: bool| {
            let b = mesh.bounds();
            a.mesh(mesh, mt, Some(b));
            if sup {
                a.supports.push(b);
            }
        };
        // side boards
        let sx = (w - st) * -0.5;
        push(&mut a, cube_at([st, d, h], [sx, 0.0, h * 0.5]), &m, false);
        push(&mut a, cube_at([st, d, h], [-sx, 0.0, h * 0.5]), &m, false);
        // division boards
        let mult = dt * 0.5;
        let add = gap + mult;
        let sub = h - mult;
        let zs = [add, (sub + add) * 0.5, sub];
        for z in zs {
            push(&mut a, cube_at([w - st * 2.0, d, dt], [0.0, 0.0, z]), &m, true);
        }
        // back board
        push(&mut a, cube_at([w, bt, h], [0.0, d * -0.5 + bt * -0.5, h * 0.5]), &m, false);
        // screw heads (axis along x), mirrored in x
        let sy = d * 0.5 - screw_gap;
        let z_top = h - dt * 0.5;
        let z_bot = dt * 0.5 + gap;
        let pts = [(sy, z_top), (sy, z_bot), (-sy, z_top), (0.0, (z_top + z_bot) * 0.5), (-sy, z_bot)];
        let mut sc = Mesh::new();
        for (y, z) in pts {
            let mut c = cylinder_node(32, screw_r, screw_depth);
            c.transform(&Mat4::rot_y(R90));
            c.transform(&Mat4::translation([w * 0.5, y, z]));
            sc.append(&c);
        }
        let mut sc2 = sc.clone();
        sc2.transform(&Mat4::scale([-1.0, 1.0, 1.0]));
        sc.append(&sc2);
        push(&mut a, sc, &metal, false);
        // attach gadgets
        let zt = h - dt;
        push(&mut a, cube_at([aw, atl, at], [0.0, (d - atl) * -0.5, zt]), &metal, false);
        push(&mut a, cube_at([aw, at, abl], [0.0, d * -0.5, zt - abl * 0.5]), &metal, false);
        a.transform(&Mat4::rot_z(-R90));
        a
    }
}

pub struct CellShelf {
    dims: [f64; 3],
    cell: f64,
    hn: usize,
    vn: usize,
    div_t: f64,
    ext_t: f64,
    attach: f64,
    mat: Material,
    screw: Material,
}

const METALS_TABLE: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3)];

impl CellShelf {
    fn new(rng: &mut Rng, tv: bool, fseed: u64) -> CellShelf {
        let mut r = rng.clone();
        let mut dims = if tv {
            [r.uniform(0.3, 0.45), r.uniform(0.7, 2.1), r.uniform(0.3, 0.5)]
        } else {
            [r.uniform(0.3, 0.45), r.uniform(0.7, 2.1), r.uniform(0.35, 2.1)]
        };
        let hn = ((dims[1] / 0.35) as usize).max(1);
        let cell = dims[1] / hn as f64;
        let vn = ((dims[2] / cell) as usize).max(1);
        dims[2] = vn as f64 * cell;
        let div_t = clip(r.normal(0.015, 0.005), 0.008, 0.022);
        let ext_t = clip(r.normal(0.04, 0.005), 0.028, 0.052);
        let attach = clip(r.normal(0.05, 0.02), 0.02, 0.1);
        let mat = plywood(["black_wood", "white", "wood"][r.choice_p(&[0.3, 0.2, 0.5]).min(2)], false, fseed);
        // screw heads: weighted_sample(material_assignments.metals)()()
        let screw = weighted(&mut r, METALS_TABLE);
        let screw = cls_spec("shelf_screw", screw, fseed + 1, [0.5, 0.5, 0.52]);
        CellShelf { dims, cell, hn, vn, div_t, ext_t, attach, mat, screw }
    }
}

impl Factory for CellShelf {
    fn exact(&self, _i: u64) -> Option<cabnodes::ExactMesh> {
        Some(cabnodes::cellshelf_gmesh(self.dims[0], self.cell, self.hn, self.vn, self.div_t, self.ext_t, self.attach).to_exact())
    }
    fn asset(&self, _i: u64) -> Asset {
        use cabnodes::*;
        let mut a = Asset::default();
        let (d, c, h, v, dv, e) = (self.dims[0], self.cell, self.hn as f64, self.vn as f64, self.div_t, self.ext_t);
        let leg = 0.0;
        let m = &self.mat;
        let push = |a: &mut Asset, mesh: Mesh, sup: bool| {
            let b = mesh.bounds();
            a.mesh(mesh, m, Some(b));
            if sup {
                a.supports.push(b);
            }
        };
        let z5 = (v - 1.0) * dv + v * c; // inner height (side_z)
        // side boards
        let add7 = c * h + (e + dv * (h - 1.0));
        let sx = 0.5 * add7;
        for sg in [1.0, -1.0] {
            push(&mut a, cube_at([e, d, z5], [sg * sx, d * 0.5, z5 * 0.5 + (e + leg)]), false);
        }
        // top / bottom boards
        let bx = (e * 2.0 + dv * (h - 1.0)) + c * h + 0.002;
        let zb = e * 0.5 + leg;
        let zt = zb + e + v * c + dv * (v - 1.0);
        push(&mut a, cube_at([bx, d, e], [0.0, d * 0.5, zb]), true);
        push(&mut a, cube_at([bx, d, e], [0.0, d * 0.5, zt]), true);
        // vertical divisions
        for i in 1..self.hn {
            let i = i as f64;
            let x = (0.5 * (h - 1.0) - i + 0.5) * dv + c * (h * 0.5 - i);
            push(&mut a, cube_at([dv, d - 0.001, z5], [x, d * 0.5, (leg + e) + z5 * 0.5]), false);
        }
        // horizontal divisions
        for j in 1..self.vn {
            let j = j as f64;
            let z = j * c + (j - 1.0) * dv + (e + leg + dv * 0.5);
            push(&mut a, cube_at([h * c + (h - 1.0) * dv, d, dv], [0.0, d * 0.5, z]), true);
        }
        // attach gadget (no backboard)
        let sz = self.attach;
        let ax = (bx * 0.5 - e) - sz * 0.5;
        let az = (leg + z5 + e - 0.02) - sz * 0.5;
        push(&mut a, cube_at([sz, 0.001, sz], [-ax, 0.0, az]), false);
        push(&mut a, cube_at([sz, 0.001, sz], [ax, 0.0, az]), false);
        // screw heads
        let sxx = (bx - e) * 0.5;
        let sy = e * 0.5;
        let szz = z5 + leg + e * 2.0;
        let mut sc = Mesh::new();
        for (x, y) in [(sxx, sy), (sxx, d - sy), (-sxx, d - sy), (-sxx, sy)] {
            let mut cyl = cylinder_node(32, 0.005, 0.001);
            cyl.transform(&Mat4::translation([x, y, szz]));
            sc.append(&cyl);
        }
        { let b = sc.bounds(); a.mesh(sc, &self.screw, Some(b)); }
        a.transform(&Mat4::rot_z(-R90));
        a
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (x, y, z) = (self.dims[0], self.dims[1], self.dims[2]);
        let mut a = Asset::default();
        let m = mat("ph", [0.5; 3], 0.5);
        a.cuboid([0.0, -y / 2.0 * 1.1, 0.0], [x, y / 2.0 * 1.1, z + (self.vn as f64 - 1.0) * self.div_t + 2.0 * self.ext_t], &m);
        Some(a)
    }
}

pub struct SimpleDesk {
    side: bool,
    fseed: u64,
}

impl SimpleDesk {
    fn new(_rng: &mut Rng, side: bool, fseed: u64) -> SimpleDesk {
        SimpleDesk { side, fseed }
    }
}

pub struct DeskParams {
    d: f64,
    w: f64,
    h: f64,
    top_white: bool,
    leg_white: bool,
    leg_r: f64,
    leg_dist: f64,
    t: f64,
}

impl SimpleDesk {
    fn params(&self, i: u64) -> DeskParams {
        // all draws happen per instance in python (get_asset_params): dims, two material choices, then leg params
        let mut r = inst_rng(self.fseed, i);
        let (d, w, h) = if self.side {
            let w = 0.55 * r.normal(1.0, 0.1);
            (w, w, w * r.normal(1.0, 0.05))
        } else {
            (r.uniform(0.5, 0.75), r.uniform(0.8, 2.0), r.uniform(0.6, 0.8))
        };
        let top_white = r.choice_idx(2) == 0;
        let leg_white = r.choice_idx(2) == 0;
        let leg_r = r.uniform(0.01, 0.025);
        let leg_dist = r.uniform(0.035, 0.07);
        let t = r.uniform(0.01, 0.03);
        DeskParams { d, w, h, top_white, leg_white, leg_r, leg_dist, t }
    }
}

impl Factory for SimpleDesk {
    fn exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        let p = self.params(i);
        Some(cabnodes::desk_gmesh(p.d, p.w, p.h, p.leg_r, p.leg_dist, p.t).to_exact())
    }
    fn asset(&self, i: u64) -> Asset {
        let p = self.params(i);
        let (d, w, h, t, leg_r, leg_dist) = (p.d, p.w, p.h, p.t, p.leg_r, p.leg_dist);
        let sd = self.fseed * 1000 + i;
        let top = if p.top_white { plywood("white", false, sd) } else { plywood("black_wood", false, sd) };
        let leg = if p.leg_white { shader_spec("leg_white", "wood.plywood.shader_shelves_white_metallic", sd, [0.9, 0.9, 0.9]) } else { shader_spec("leg_black", "wood.plywood.shader_shelves_black_metallic", sd, [0.05, 0.05, 0.05]) };
        let mut a = Asset::default();
        a.board([-d / 2.0, -w / 2.0, h - t], [d / 2.0, w / 2.0, h], &top);
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                a.cylinder(sx * (d / 2.0 - leg_dist), sy * (w / 2.0 - leg_dist), 0.0, h - t, leg_r, &leg);
            }
        }
        a
    }
}

pub struct SingleCabinet {
    dims: [f64; 3],
    fseed: u64,
}

impl SingleCabinet {
    fn new(rng: &mut Rng, fseed: u64) -> SingleCabinet {
        // init draws only the dimensions (SingleCabinetFactory.sample_params under FixedSeed)
        let mut r = rng.clone();
        let dims = [r.uniform(0.25, 0.35), r.uniform(0.3, 0.7), r.uniform(0.9, 1.8)];
        SingleCabinet { dims, fseed }
    }
}

impl Factory for SingleCabinet {
    fn exact(&self, inst: u64) -> Option<cabnodes::ExactMesh> {
        let mut r = inst_rng(self.fseed, inst);
        Some(cabdoor::single_cabinet_gmesh(&mut r, self.dims, Default::default()).to_exact())
    }
    fn asset(&self, inst: u64) -> Asset {
        let dims = self.dims;
        let num_h = (((dims[2] - 0.083) / 0.3) as usize).max(1);
        let cell_h = vec![(dims[2] - 0.083) / num_h as f64; num_h];
        // per-instance draws (python get_asset_params): frame material choice, then the large-shelf parameters
        let mut r = inst_rng(self.fseed, inst);
        let _ = shelf_mat(&mut r, &[0.5, 0.2, 0.3]); // keeps the stream position of the geometry draws below
        // python: frame_material = first np.random.choice of FixedSeed(factory_seed) (get_material_params)
        let kind = fixed_kind(self.fseed, &[0.5, 0.2, 0.3]);
        let mat = plywood(kind, false, self.fseed);
        let panel = plywood(kind, true, self.fseed + 1);
        let side_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let _bottom_gap = r.uniform(0.01, 0.05);
        let div_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let frame = Frame { depth: dims[0] - 0.01, cell_w: vec![dims[1]], cell_h, side_t, div_t, bottom_h: 0.083, back_t: 0.01 };
        let mut a = Asset::default();
        frame.build(&mut a, &mat);
        // doors on the front: one if the cabinet is narrower than 0.55 else two
        let (w, h, d) = (frame.width() + 2.0 * frame.side_t, frame.height(), frame.depth);
        let nd = if w < 0.55 { 1 } else { 2 };
        let dw = if nd == 1 { w } else { w / 2.0 - 0.0005 };
        for k in 0..nd {
            let y0 = if nd == 1 { -w / 2.0 } else { -w / 2.0 + k as f64 * (w / 2.0 + 0.0005) };
            a.cuboid([d / 2.0, y0, frame.bottom_h], [d / 2.0 + 0.015, y0 + dw, h], &panel);
        }
        a
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (x, y, z) = (self.dims[0], self.dims[1], self.dims[2]);
        let mut a = Asset::default();
        let m = mat("ph", [0.5; 3], 0.5);
        a.cuboid([-x / 2.0 * 1.2, -y / 2.0 * 1.2, 0.0], [x / 2.0 * 1.2, y / 2.0 * 1.2, (z + 0.083) * 1.02], &m);
        Some(a)
    }
}

pub struct KitchenCabinet {
    dims: [f64; 3],
    widths: Vec<f64>,
    mat: Material,
    fseed: Option<u64>, // Some => standalone factory: dims/widths are sampled per instance (python get_asset_params)
}

impl KitchenCabinet {
    fn with_dims(rng: &mut Rng, dims: Option<[f64; 3]>) -> KitchenCabinet {
        let mut r = rng.clone();
        let dims = dims.unwrap_or_else(|| [r.uniform(0.25, 0.35), r.uniform(1.0, 4.0), r.uniform(0.5, 1.3)]);
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s: f64 = ws.iter().sum();
        let mat = shelf_mat(&mut r, &[0.4, 0.3, 0.3]);
        KitchenCabinet { dims, widths: ws.iter().map(|w| w / s * dims[1]).collect(), mat, fseed: None }
    }
    /// instance-level sampling in python order: dims (3 uniforms), then one U(0.55,1) per cell
    fn from_instance(fseed: u64, inst: u64) -> KitchenCabinet {
        let mut r = inst_rng(fseed, inst);
        let dims = [r.uniform(0.25, 0.35), r.uniform(1.0, 4.0), r.uniform(0.5, 1.3)];
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s: f64 = ws.iter().sum();
        let mat = shelf_mat(&mut r, &[0.4, 0.3, 0.3]);
        KitchenCabinet { dims, widths: ws.iter().map(|w| w / s * dims[1]).collect(), mat, fseed: Some(fseed) }
    }
    fn build(&self, a: &mut Asset, origin: V3) {
        let (d, w, h) = (self.dims[0], self.dims[1], self.dims[2]);
        let t = 0.02;
        let kind = fixed_kind(self.fseed.unwrap_or(0), &[0.4, 0.3, 0.3]);
        let (frame, board) = (plywood(kind, true, self.fseed.unwrap_or(0)), plywood(kind, false, self.fseed.unwrap_or(0) + 1));
        let (x0, x1) = (origin[0] - d / 2.0 - 0.005, origin[0] + d / 2.0 + 0.005);
        let mut y = origin[1] - w / 2.0;
        let z0 = origin[2];
        a.cuboid([x0, y, z0], [x0 + 0.01, origin[1] + w / 2.0, z0 + h], &frame);
        let mut ys = vec![y];
        for cw in &self.widths {
            y += cw;
            ys.push(y);
        }
        for yy in &ys {
            a.cuboid([x0, yy - t / 2.0, z0], [x1, yy + t / 2.0, z0 + h], &frame);
        }
        a.board([x0, origin[1] - w / 2.0, z0], [x1, origin[1] + w / 2.0, z0 + t], &board);
        a.board([x0, origin[1] - w / 2.0, z0 + h - t], [x1, origin[1] + w / 2.0, z0 + h], &board);
        for k in 0..self.widths.len() {
            a.cuboid([x1, ys[k] + 0.001, z0 + 0.06], [x1 + 0.015, ys[k + 1] - 0.001, z0 + h - 0.0], &frame);
        }
    }
}

impl Factory for KitchenCabinet {
    fn exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        let fs = self.fseed?;
        let mut r = inst_rng(fs, i);
        let spec = cabdoor::KcSpec { dims: self.dims, widths: self.widths.clone(), drawer_only: false };
        Some(cabdoor::kitchen_cabinet_gmesh(&mut r, &spec, Default::default()).to_exact())
    }
    fn asset(&self, i: u64) -> Asset {
        let mut a = Asset::default();
        if let Some(fs) = self.fseed {
            KitchenCabinet::from_instance(fs, i).build(&mut a, [0.0, 0.0, 0.0]);
        } else {
            self.build(&mut a, [0.0, 0.0, 0.0]);
        }
        a
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let (x, y, z) = (self.dims[0] + 0.01, self.dims[1], self.dims[2]);
        let num_w = (y / 0.45).max(1.0);
        let width = (num_w - 1.0) * 0.02 * 2.0 + (num_w - 1.0) * 0.001 + y;
        let height = (((z - 0.06) / 0.3) as i64 + 1) as f64 * 0.02 + 0.06 + z;
        let mut a = Asset::default();
        a.cuboid([-x / 2.0, -width / 2.0, 0.0], [x / 2.0, width / 2.0, height], &mat("ph", [0.5; 3], 0.5));
        Some(a)
    }
}

pub struct KitchenSpace {
    dims: [f64; 3],
    island: bool,
    bottom_h: f64,
    top_h: f64,
    factory_seed: u64,
}

impl KitchenSpace {
    fn new(rng: &mut Rng, island: bool, factory_seed: u64) -> KitchenSpace {
        let mut r = rng.clone();
        // python: dimensions = mathutils.Vector((...)) => float32 components (also after `dimensions.x *= uniform(1.5, 2)`)
        let f = |x: f64| x as f32 as f64;
        let mut dims = [f(r.uniform(0.7, 1.0)), f(r.uniform(1.7, 5.0)), f(r.uniform(2.3, 2.5))];
        if island {
            dims[0] = f(dims[0] * r.uniform(1.5, 2.0));
        }
        let bottom_h = r.uniform(0.8, 1.0);
        let top_h = r.uniform(0.8, 1.0);
        KitchenSpace { dims, island, bottom_h, top_h, factory_seed }
    }
}

impl KitchenSpace {
    /// KitchenCabinetFactory(fseed, dimensions=(x, y - 0.15, bottom_h), drawer_only=True)(i=0)
    fn bottom_cabinet(&self) -> cabnodes::GMesh {
        let [x, y, _z] = self.dims;
        let dims = [x, y - 0.15, self.bottom_h];
        let mut r = Rng::seeded(self.factory_seed);
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s = np_sum(&ws);
        let widths: Vec<f64> = ws.iter().map(|w| w / s * dims[1]).collect();
        let mut ri = inst_rng(self.factory_seed, 0);
        let spec = cabdoor::KcSpec { dims, widths, drawer_only: true };
        cabdoor::kitchen_cabinet_gmesh(&mut ri, &spec, Default::default())
    }
}

impl KitchenSpace {
    fn cabinet(&self, dims: [f64; 3], drawer_only: bool, inst: u64, prev: Option<(f64, f64, f64, f64)>) -> (cabnodes::GMesh, (f64, f64, f64, f64)) {
        let mut r = Rng::seeded(self.factory_seed);
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s = np_sum(&ws);
        let widths: Vec<f64> = ws.iter().map(|w| w / s * dims[1]).collect();
        let mut ri = inst_rng(self.factory_seed, inst);
        let spec = cabdoor::KcSpec { dims, widths, drawer_only };
        cabdoor::kitchen_cabinet_gmesh_ex(&mut ri, &spec, Default::default(), prev)
    }

    fn translated(mut g: cabnodes::GMesh, t: [f64; 3]) -> cabnodes::GMesh {
        for p in g.v.iter_mut() {
            for k in 0..3 {
                p[k] = p[k] + t[k] as f32;
            }
        }
        g
    }

    /// KitchenSpaceFactory.create_asset(i): bottom cabinet + countertop (geometry_nodes_add_cabinet_top), three top cabinets, joined and
    /// (non-island) scaled to `dimensions`
    fn assemble(&self, inst: u64) -> cabnodes::GMesh {
        let parts = self.raw_parts(inst);
        let mut g = hwexact::join_fwd(&parts);
        if !self.island {
            self.scale_to_dims(std::slice::from_mut(&mut g));
        }
        g
    }

    /// non-island: scale the joined set so its bbox equals `dimensions` (applied to a list of parts it uses their common bbox)
    fn scale_to_dims(&self, parts: &mut [cabnodes::GMesh]) {
        let [x, y, z] = self.dims;
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for g in parts.iter() {
            for v in &g.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let dims = [x as f32, y as f32, z as f32];
        for g in parts.iter_mut() {
            for v in g.v.iter_mut() {
                for k in 0..3 {
                    v[k] = v[k] * (dims[k] / (hi[k] - lo[k]));
                }
            }
        }
    }

    /// [bottom cabinet, countertop, (top left, top right, top mid)] before the final scaling
    fn raw_parts(&self, inst: u64) -> Vec<cabnodes::GMesh> {
        let [x, y, z] = self.dims;
        let bottom = self.bottom_cabinet();
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for v in &bottom.v {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        let (xr, yr) = ((hi[0] - lo[0]) as f64, (hi[1] - lo[1]) as f64);
        let top = super::tableparts::table_top_exact(4, xr * 1.414, yr / xr, 0.01, 0.05, 0.01);
        let mut slab = cabnodes::GMesh::new();
        slab.v = top.pos.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        slab.p = top.faces.clone();
        slab.tag = vec![0; slab.p.len()];
        let slab = Self::translated(slab, [0.0, ((lo[1] + hi[1]) / 2.0) as f64, hi[2] as f64]);
        let mut parts = vec![bottom, slab];
        if !self.island {
            let mut r = inst_rng(self.factory_seed, inst);
            let tmw = r.uniform(1.0, 1.3);
            let ctw = (y - tmw) / 2.0 - 0.05;
            let th = self.top_h;
            // left and right share one KitchenCabinetFactory object: the second instance reuses the first one's frame parameters
            let (left, rnd) = self.cabinet([x / 2.0, ctw, th], false, 0, None);
            let (right, _) = self.cabinet([x / 2.0, ctw, th], false, 1, Some(rnd));
            let (mid, _) = self.cabinet([x * 0.66, tmw, th * 0.8], false, 0, None);
            parts.push(Self::translated(left, [-x / 4.0, 0.0, z - th]));
            parts.push(Self::translated(right, [-x / 4.0, y - ctw, z - th]));
            parts.push(Self::translated(mid, [-x / 6.0, y / 2.0 - tmw / 2.0, z - th * 0.8]));
        }
        parts
    }
}

/// numpy float64 pairwise sum (n < 128: 8-way unrolled blocks)
pub fn np_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut s = 0.0;
        for x in a {
            s += x;
        }
        s
    } else {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i + 8 <= n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    }
}

impl Factory for KitchenSpace {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        if !self.island {
            return None;
        }
        let e = self.bottom_cabinet();
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &e.v {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k] as f64);
                hi[k] = hi[k].max(v[k] as f64);
            }
        }
        // countertop slab (geometry_nodes_add_cabinet_top): generate_table_top(thickness 0.05, 4-gon, profile width 1.414 * x range,
        // aspect (y range / x range), fillet ratio 0.01, vertical fillet 0.01) translated to (0, y centre, bbox max z)
        let (xr, yr) = (hi[0] - lo[0], hi[1] - lo[1]);
        let top = super::tableparts::table_top_exact(4, xr * 1.414, yr / xr, 0.01, 0.05, 0.01);
        let (tlo, thi) = top.bounds();
        let ycen = (lo[1] + hi[1]) / 2.0;
        for k in 0..3 {
            let off = [0.0, ycen, hi[2]][k];
            lo[k] = lo[k].min(tlo[k] + off);
            hi[k] = hi[k].max(thi[k] + off);
        }
        Some((lo, hi))
    }
    fn placeholder_exact(&self, _i: u64) -> Option<cabnodes::ExactMesh> {
        if self.island {
            return None;
        }
        let [x, y, z] = self.dims;
        let (l1, h1) = super::geomkit::new_bbox_f32(-x / 2.0 * 1.08, x / 2.0 * 1.08, 0.0, y, 0.0, self.bottom_h + 0.13);
        let (l2, h2) = super::geomkit::new_bbox_f32(-x / 2.0, x * 0.16, 0.0, y, z - self.top_h - 0.1, z);
        let b1 = cabnodes::new_bbox_exact(l1, h1);
        let b2 = cabnodes::new_bbox_exact(l2, h2);
        let mut e = b1.clone();
        e.names[5] = "support".into();
        e.names[11] = "support".into();
        let mut n2 = b2.clone();
        n2.names = (0..12).map(|i| if i % 6 == 5 { "top".to_string() } else { String::new() }).collect();
        let base = e.verts.len() as u32;
        e.verts.extend(n2.verts.iter().cloned());
        // objects are joined first (quads: box1 then box2), then triangulated: first triangles in place, second triangles appended
        let p2: Vec<Vec<u32>> = n2.polys.iter().map(|p| p.iter().map(|v| v + base).collect()).collect();
        let mut polys = vec![];
        let mut names = vec![];
        for half in 0..2 {
            for (pp, nn) in [(&e.polys, &e.names), (&p2, &n2.names)] {
                for k in 0..6 {
                    polys.push(pp[half * 6 + k].clone());
                    names.push(nn[half * 6 + k].clone());
                }
            }
        }
        e.polys = polys;
        e.names = names;
        Some(e)
    }
    fn exact(&self, i: u64) -> Option<cabnodes::ExactMesh> {
        Some(self.assemble(i).to_exact())
    }
    fn asset(&self, i: u64) -> Asset {
        let mut a = Asset::default();
        let mut parts = self.raw_parts(i);
        if !self.island {
            self.scale_to_dims(&mut parts);
        }
        // part 0/2.. = cabinets (KitchenCabinetFactory(factory_seed): z-textured frame material), part 1 = countertop (shader_marble)
        let kind = fixed_kind(self.factory_seed, &[0.4, 0.3, 0.3]);
        let cab = plywood(kind, true, self.factory_seed);
        let top = cls_spec("counter_marble", "ceramic.Marble", self.factory_seed + 7, [0.85, 0.85, 0.85]);
        for (k, g) in parts.iter().enumerate() {
            a.mesh(g.to_mesh(), if k == 1 { &top } else { &cab }, None);
        }
        a
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let [x, y, z] = self.dims;
        let mut a = Asset::default();
        let m = mat("ph", [0.5; 3], 0.5);
        a.board([-x / 2.0 * 1.08, 0.0, 0.0], [x / 2.0 * 1.08, y, self.bottom_h + 0.13], &m);
        if !self.island {
            a.cuboid([-x / 2.0, 0.0, z - self.top_h - 0.1], [x * 0.16, y, z], &m);
        }
        Some(a)
    }
}

pub fn make(c: &str, rng: &mut Rng, factory_seed: u64) -> Option<Box<dyn Factory>> {
    let seed = factory_seed;
    Some(match c {
        "LargeShelfFactory" => Box::new(LargeShelf::new(rng, seed)),
        "SimpleBookcaseFactory" => Box::new(SimpleBookcase::new(rng, factory_seed)),
        "CellShelfFactory" => Box::new(CellShelf::new(rng, false, seed)),
        "TVStandFactory" => Box::new(CellShelf::new(rng, true, seed)),
        "SimpleDeskFactory" => Box::new(SimpleDesk::new(rng, false, factory_seed)),
        "SidetableDeskFactory" => Box::new(SimpleDesk::new(rng, true, factory_seed)),
        "SingleCabinetFactory" => Box::new(SingleCabinet::new(rng, factory_seed)),
        "KitchenCabinetFactory" => Box::new(KitchenCabinet { fseed: Some(factory_seed), ..KitchenCabinet::with_dims(rng, None) }),
        "KitchenSpaceFactory" => Box::new(KitchenSpace::new(rng, false, seed)),
        "KitchenIslandFactory" => Box::new(KitchenSpace::new(rng, true, seed)),
        _ => return None,
    })
}
