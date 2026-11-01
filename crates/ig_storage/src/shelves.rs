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
        sp["seed"] = serde_json::json!(seed % 2_000_000_000);
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
    let mut m = spec_material(&format!("{name}{}", if z { "_z" } else { "" }), serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}), rgb);
    m.roughness = 0.5;
    m
}

fn shader_spec(name: &str, shader: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, serde_json::json!({"cls": format!("shader.{shader}"), "kwargs": {}, "seed": seed % 2_000_000_000}), rgb)
}

fn cls_spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, serde_json::json!({"cls": cls, "kwargs": {}, "seed": seed % 2_000_000_000}), rgb)
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

/// words a fresh `Rng::seeded(seed)` has consumed when its state equals `st` (the instance stream position of a material draw); the render side
/// replays it as np.random.seed(seed) + burning that many 32-bit words
fn words_consumed(seed: u64, st: &Rng) -> u64 {
    let mut t = st.clone();
    let want = [t.next_u32(), t.next_u32(), t.next_u32()];
    let mut r = Rng::seeded(seed);
    let mut win = [r.next_u32(), r.next_u32(), r.next_u32()];
    for n in 0..200_000u64 {
        if win == want {
            return n;
        }
        win = [win[1], win[2], r.next_u32()];
    }
    0
}

fn inst_seed(fseed: u64, inst: u64) -> u64 {
    int_hash(&[&fseed.to_string(), &inst.to_string()])
}

/// material of a drawer from its get_shelf_material(<Material>) choice: 0 shader_shelves_white, 1 shader_rough_plastic, 2 shader_shelves_black_wood,
/// 3 shader_wood, 4 shader_shelves_wood. The sampled shaders (plastic, wood) draw their parameters from the instance stream: spec seed + `skip`.
fn drawer_material(p: &cabdoor::DrawerPick, seed: u64) -> Material {
    let skipped = |shader: &str, name: &str, rgb: [f32; 3]| {
        let skip = words_consumed(seed, &p.after);
        spec_material(name, serde_json::json!({"cls": format!("shader.{shader}"), "kwargs": {}, "seed": seed, "skip": skip}), rgb)
    };
    match p.idx {
        0 => plywood("white", false, 0),
        1 => skipped("plastic.plastic_rough.shader_rough_plastic", "drawer_plastic", [0.5, 0.5, 0.5]),
        2 => plywood("black_wood", false, 0),
        3 => skipped("wood.wood.shader_wood", "drawer_wood", [0.45, 0.3, 0.17]),
        _ => plywood("wood", false, 0),
    }
}

/// a kitchen front's knob (cabdoor::KNOB_SLOT): brushed steel, matte black when every front is light (steel on white read faintly)
fn knob_metal(fronts_light: bool) -> Material {
    let (name, rgb, rough, metal) = if fronts_light { ("cab_knob_black", 0.04, 0.4, 0.6) } else { ("cab_knob_steel", 0.7, 0.3, 1.0) };
    let mut m = Material::diffuse(name, [rgb, rgb, rgb * 1.01], rough);
    m.metallic = metal;
    m
}

/// slot indices of `g`'s faces into `mats`, the knob slot appended as a metal material
/// (`mats`: 0 frame + door fronts, then boards / countertop, drawer fronts last)
fn with_knob(tags: &[u8], mut mats: Vec<Material>) -> (Vec<u16>, Vec<Material>) {
    let k = mats.len() as u16;
    let last = k - 1;
    let ids = tags.iter().map(|&t| if t >> 1 == cabdoor::KNOB_SLOT { k } else { ((t >> 1) as u16).min(last) }).collect();
    let luma = |m: &Material| 0.3 * m.color[0] + 0.59 * m.color[1] + 0.11 * m.color[2];
    let light = luma(&mats[0]) > 0.4 && luma(&mats[mats.len() - 1]) > 0.4;
    mats.push(knob_metal(light));
    (ids, mats)
}

/// (knob faces, the rest) of a cabinet mesh
fn split_knobs(g: &cabnodes::GMesh) -> (cabnodes::GMesh, cabnodes::GMesh) {
    let (mut kn, mut rest) = (cabnodes::GMesh::new(), cabnodes::GMesh::new());
    for (i, f) in g.p.iter().enumerate() {
        let o = if g.tag[i] >> 1 == cabdoor::KNOB_SLOT { &mut kn } else { &mut rest };
        let base = o.v.len() as u32;
        o.v.extend(f.iter().map(|&j| g.v[j as usize]));
        o.p.push((0..f.len() as u32).map(|j| base + j).collect());
        o.tag.push(g.tag[i]);
    }
    (kn, rest)
}

/// slot materials of a kitchen cabinet row: [frame + doors (K_z), division boards (K), drawers...]
fn kitchen_slot_mats(kind: &str, picks: &[cabdoor::DrawerPick], seed: u64) -> Vec<Material> {
    let mut v = vec![plywood(kind, true, 0), plywood(kind, false, 0)];
    // one front material for the whole cabinet (see KitchenSpace::exact_slots)
    if let Some(p0) = picks.first() { let m0 = drawer_material(p0, seed); v.extend(picks.iter().map(|_| m0.clone())); }
    v
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
    hint: Option<[f64; 3]>,
}

impl LargeShelf {
    fn new(_rng: &mut Rng, factory_seed: u64) -> LargeShelf {
        // the python factory has no init draws: LargeShelfFactory.get_asset_params samples everything per instance
        LargeShelf { fseed: factory_seed, hint: crate::api::size_hint() }
    }
}

impl LargeShelf {
    /// the draws that fix the overall size, in python order: dims [depth, width, height], side board and division thicknesses
    fn size_draws(&self, r: &mut Rng) -> ([f64; 3], f64, f64, f64) {
        let dims = [r.uniform(0.25, 0.35), r.uniform(0.3, 2.0), r.uniform(0.9, 2.0)];
        let side_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let y_gap = r.uniform(0.01, 0.05);
        let div_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        (dims, side_t, y_gap, div_t)
    }
    /// overall [width, depth, height] of the frame for drawn dims: boards add to the cells
    fn totals(dims: [f64; 3], side_t: f64, div_t: f64) -> [f64; 3] {
        let nv = ((dims[1] / 0.5) as usize).max(1) as f64;
        let nh = (((dims[2] - 0.083) / 0.3) as usize).max(1) as f64;
        [dims[1] + 2.0 * side_t + (nv - 1.0) * (2.0 * side_t + 0.001), dims[0] + LS_DEPTH_EXTRA, dims[2] + (nh + 1.0) * div_t]
    }
    /// dims and cell counts giving overall size `h` [width (y), depth (x), height] exactly: the counts follow the size the way the
    /// generator derives them (cells 0.5 m wide, 0.3 m high at least) and the cells take up the rest
    fn dims_for(h: [f64; 3], side_t: f64, div_t: f64) -> ([f64; 3], usize, usize) {
        let nv = (((h[0] - 2.0 * side_t) / 0.5) as usize).max(1);
        let nh = (((h[2] - 0.083 - 2.0 * div_t) / 0.3) as usize).max(1);
        let dims = [h[1] - LS_DEPTH_EXTRA, h[0] - 2.0 * side_t - (nv as f64 - 1.0) * (2.0 * side_t + 0.001), h[2] - (nh as f64 + 1.0) * div_t];
        (dims, nv, nh)
    }
    fn params(&self, inst: u64) -> (cabnodes::ShelfParams, &'static str) {
        let mut r = inst_rng(self.fseed, inst);
        // sample_params() and the first draws of get_asset_params()
        let (mut dims, side_t, y_gap, div_t) = self.size_draws(&mut r);
        let mut counts = None;
        if let Some(h) = self.hint { let (d, nv, nh) = Self::dims_for(h, side_t, div_t); dims = d; counts = Some((nv, nh)); }
        let bottom_h = 0.083;
        let depth = dims[0] - 0.01;
        let num_h = counts.map_or((((dims[2] - 0.083) / 0.3) as usize).max(1), |c| c.1);
        let cell_h = vec![(dims[2] - 0.083) / num_h as f64; num_h];
        let num_v = counts.map_or(((dims[1] / 0.5) as usize).max(1), |c| c.0);
        let cell_w = vec![dims[1] / num_v as f64; num_v];
        // get_asset_params(): remaining draws in python order (side_t, y_gap and div_t were drawn above)
        let screw_depth = r.uniform(0.001, 0.004);
        let screw_r = r.uniform(0.001, 0.004);
        let screw_wgap = r.uniform(0.0, 0.02);
        for (lo, hi) in [(0.025, 0.06), (0.05, 0.1), (0.01, 0.025), (0.002, 0.005), (0.0, 0.05)] {
            r.uniform(lo, hi);
        }
        let kind = ["white", "black_wood", "wood"][r.choice_p(&[0.4, 0.3, 0.3]).min(2)];
        let mut p = cabnodes::ShelfParams { shelf_depth: depth, side_t, back_t: 0.01, y_gap, bottom_h, div_t, screw_depth, screw_r, screw_wgap, cell_w, cell_h, ..Default::default() };
        p.derive();
        (p, kind)
    }
}

/// the box depth minus the drawn depth (screw heads / attach gadgets stand proud; measured)
const LS_DEPTH_EXTRA: f64 = 0.002;

impl Factory for LargeShelf {
    fn natural_size(&self, inst: u64) -> Option<[f64; 3]> {
        let (dims, side_t, _, div_t) = self.size_draws(&mut inst_rng(self.fseed, inst));
        Some(Self::totals(dims, side_t, div_t)).map(|t| [t[0], t[1], t[2]])
    }
    fn asset(&self, inst: u64) -> Asset {
        // large_shelf.py get_material_func: frame_material = shader_shelves_<kind>[_z] (z texture), board_material = the plain variant
        let (p, kind) = self.params(inst);
        cabnodes::shelf_frame(&p, &plywood(kind, true, 0), &plywood(kind, false, 0))
    }
    fn exact(&self, inst: u64) -> Option<cabnodes::ExactMesh> {
        let (p, _) = self.params(inst);
        Some(cabnodes::shelf_gmesh(&p, true).to_exact())
    }
}

pub struct SimpleBookcase {
    fseed: u64,
    hint: Option<[f64; 3]>,
}

impl SimpleBookcase {
    fn new(_rng: &mut Rng, factory_seed: u64) -> SimpleBookcase {
        // python factory has no init draws; everything is sampled per instance in get_asset_params
        SimpleBookcase { fseed: factory_seed, hint: crate::api::size_hint() }
    }
}

pub use super::cabnodes::BookParams;

impl SimpleBookcase {
    /// built to the layout box [width (y), depth (x), height]: the back board adds its thickness to the depth and the screw heads
    /// their length to the width
    fn sized(&self, d: f64, w: f64, h: f64, bt: f64, screw_depth: f64) -> (f64, f64, f64) {
        match self.hint { Some(b) => (b[1] - bt, b[0] - screw_depth, b[2]), None => (d, w, h) }
    }
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
        let (d, w, h) = self.sized(d, w, h, bt, screw_depth);
        BookParams { d, w, h, st, dt, gap, bt, screw_depth, screw_r, screw_gap, at, aw, atl, abl }
    }
}

impl Factory for SimpleBookcase {
    fn natural_size(&self, inst: u64) -> Option<[f64; 3]> {
        let mut r = inst_rng(self.fseed, inst);
        let dims = [r.uniform(0.25, 0.4), r.uniform(0.5, 0.7), r.uniform(0.7, 0.9)];
        let _ = (r.uniform(0.005, 0.03), r.normal(0.015, 0.005), r.normal(0.14, 0.05));
        let bt = r.uniform(0.01, 0.02);
        let screw_depth = r.uniform(0.002, 0.008);
        Some([dims[1] + screw_depth, dims[0] - 0.015 + bt, dims[2]])
    }
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
        let (d, w, h) = self.sized(d, w, h, bt, screw_depth);
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
    /// cell height (= `cell` unless built to a size)
    cell_z: f64,
    hn: usize,
    vn: usize,
    div_t: f64,
    ext_t: f64,
    attach: f64,
    mat: Material,
    screw: Material,
    natural: [f64; 3],
}

const METALS_TABLE: Weighted = &[("metal.MetalBasic", 3.0), ("metal.BrushedMetal", 2.0), ("metal.GalvanizedMetal", 1.0), ("metal.GrainedMetal", 1.0), ("metal.HammeredMetal", 0.7), ("metal.BrushedBlackMetal", 0.3)];

impl CellShelf {
    fn new(rng: &mut Rng, tv: bool, fseed: u64) -> CellShelf {
        let mut r = rng.clone();
        let mut dims = if tv {
            // TV stands 1.25-2.1 m wide (infinigen 0.7-2.1): narrower stands could not hold the TV (no TV in 4/10 living rooms)
            let d = [r.uniform(0.3, 0.45), r.uniform(0.7, 2.1), r.uniform(0.3, 0.5)];
            [d[0], 1.25 + (d[1] - 0.7) / 1.4 * 0.85, d[2]]
        } else {
            [r.uniform(0.3, 0.45), r.uniform(0.7, 2.1), r.uniform(0.35, 2.1)]
        };
        let mut hn = ((dims[1] / 0.35) as usize).max(1);
        let mut cell = dims[1] / hn as f64;
        let mut vn = ((dims[2] / cell) as usize).max(1);
        dims[2] = vn as f64 * cell;
        let div_t = clip(r.normal(0.015, 0.005), 0.008, 0.022);
        let ext_t = clip(r.normal(0.04, 0.005), 0.028, 0.052);
        let natural = [2.0 * ext_t + div_t * (hn as f64 - 1.0) + cell * hn as f64 + 0.002, dims[0], 2.0 * ext_t + div_t * (vn as f64 - 1.0) + cell * vn as f64];
        let mut cell_z = cell;
        // built to the layout box [width (y), depth (x), height]: the cell counts follow the width as the generator derives them and
        // the cells (wider than high for a low stand) take up the rest; boards keep their thickness
        if let Some(h) = crate::api::size_hint() {
            dims[0] = h[1];
            hn = (((h[0] - 2.0 * ext_t) / 0.35) as usize).max(1);
            cell = (h[0] - 0.002 - 2.0 * ext_t - div_t * (hn as f64 - 1.0)) / hn as f64;
            vn = (((h[2] - 2.0 * ext_t) / (cell + div_t)).round() as usize).max(1);
            cell_z = (h[2] - 2.0 * ext_t - div_t * (vn as f64 - 1.0)) / vn as f64;
            dims[1] = h[0];
            dims[2] = h[2];
        }
        let attach = clip(r.normal(0.05, 0.02), 0.02, 0.1);
        let mat = plywood(["black_wood", "white", "wood"][r.choice_p(&[0.3, 0.2, 0.5]).min(2)], false, fseed);
        // screw heads: weighted_sample(material_assignments.metals)()()
        let screw = weighted(&mut r, METALS_TABLE);
        let screw = cls_spec("shelf_screw", screw, fseed + 1, [0.5, 0.5, 0.52]);
        CellShelf { dims, cell, cell_z, hn, vn, div_t, ext_t, attach, mat, screw, natural }
    }
}

impl Factory for CellShelf {
    fn natural_size(&self, _i: u64) -> Option<[f64; 3]> {
        Some(self.natural)
    }
    fn exact(&self, _i: u64) -> Option<cabnodes::ExactMesh> {
        Some(cabnodes::cellshelf_gmesh(self.dims[0], self.cell, self.cell_z, self.hn, self.vn, self.div_t, self.ext_t, self.attach).to_exact())
    }
    fn asset(&self, _i: u64) -> Asset {
        use cabnodes::*;
        let mut a = Asset::default();
        let (d, c, h, v, dv, e) = (self.dims[0], self.cell, self.hn as f64, self.vn as f64, self.div_t, self.ext_t);
        let cz = self.cell_z;
        let leg = 0.0;
        let m = &self.mat;
        let push = |a: &mut Asset, mesh: Mesh, sup: bool| {
            let b = mesh.bounds();
            a.mesh(mesh, m, Some(b));
            if sup {
                a.supports.push(b);
            }
        };
        let z5 = (v - 1.0) * dv + v * cz; // inner height (side_z)
        // side boards
        let add7 = c * h + (e + dv * (h - 1.0));
        let sx = 0.5 * add7;
        for sg in [1.0, -1.0] {
            push(&mut a, cube_at([e, d, z5], [sg * sx, d * 0.5, z5 * 0.5 + (e + leg)]), false);
        }
        // top / bottom boards
        let bx = (e * 2.0 + dv * (h - 1.0)) + c * h + 0.002;
        let zb = e * 0.5 + leg;
        let zt = zb + e + v * cz + dv * (v - 1.0);
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
            let z = j * cz + (j - 1.0) * dv + (e + leg + dv * 0.5);
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
    hint: Option<[f64; 3]>,
}

impl SimpleDesk {
    fn clone_unhinted(&self) -> SimpleDesk {
        SimpleDesk { side: self.side, fseed: self.fseed, hint: None }
    }
    fn new(_rng: &mut Rng, side: bool, fseed: u64) -> SimpleDesk {
        SimpleDesk { side, fseed, hint: crate::api::size_hint() }
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
        // built to the layout box [width (y), depth (x), height] (the legs sit at leg_dist from the corners)
        let (d, w, h) = match self.hint { Some(b) => (b[1] - DESK_EXTRA[0], b[0] - DESK_EXTRA[1], b[2] - DESK_EXTRA[2]), None => (d, w, h) };
        DeskParams { d, w, h, top_white, leg_white, leg_r, leg_dist, t }
    }
}

/// overall extents minus the drawn [depth, width, height] (measured)
const DESK_EXTRA: [f64; 3] = [0.0, 0.0, 0.0];

impl Factory for SimpleDesk {
    fn natural_size(&self, i: u64) -> Option<[f64; 3]> {
        let mut h = self.clone_unhinted();
        let p = h.params(i);
        Some([p.w + DESK_EXTRA[1], p.d + DESK_EXTRA[0], p.h + DESK_EXTRA[2]])
    }
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
    /// built to the layout box [width (y), depth (x), height] (size hint at construction)
    hint: Option<[f64; 3]>,
    sized: std::cell::RefCell<Option<(u64, [f64; 3], usize)>>,
}

impl SingleCabinet {
    fn new(rng: &mut Rng, fseed: u64) -> SingleCabinet {
        // init draws only the dimensions (SingleCabinetFactory.sample_params under FixedSeed)
        let mut r = rng.clone();
        let dims = [r.uniform(0.25, 0.35), r.uniform(0.3, 0.7), r.uniform(0.9, 1.8)];
        SingleCabinet { dims, fseed, hint: crate::api::size_hint(), sized: Default::default() }
    }
    /// overall extents [width, depth, height] of the exact mesh for drawn dims and instance stream
    fn extents(&self, dims: [f64; 3], inst: u64, num_h: Option<usize>) -> [f64; 3] {
        let g = cabdoor::single_cabinet_gmesh(&mut inst_rng(self.fseed, inst), dims, Default::default(), num_h);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for v in &g.v {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k] as f64);
                hi[k] = hi[k].max(v[k] as f64);
            }
        }
        [hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]]
    }
    /// the dims the frame is drawn with: with a size hint, dims whose overall extents (boards, doors and knobs included) are the box's.
    /// The board thicknesses are the first instance draws, the door depth is measured on the mesh.
    fn dims_for(&self, inst: u64) -> ([f64; 3], Option<usize>) {
        let Some(h) = self.hint else { return (self.dims, None) };
        if let Some((i, d, n)) = *self.sized.borrow() { if i == inst { return (d, Some(n)); } }
        let mut r = inst_rng(self.fseed, inst);
        let side_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let _ = r.uniform(0.01, 0.05);
        let div_t = clip(r.normal(0.02, 0.002), 0.015, 0.025);
        let nh = (((h[2] - 0.083 - 2.0 * div_t) / 0.3) as usize).max(1);
        let mut dims = [h[1] - 0.045, h[0] - 2.0 * side_t - 0.002, h[2] - (nh as f64 + 1.0) * div_t];
        for _ in 0..2 {
            let e = self.extents(dims, inst, Some(nh));
            dims[0] += h[1] - e[1];
            dims[1] += h[0] - e[0];
            dims[2] += h[2] - e[2];
        }
        *self.sized.borrow_mut() = Some((inst, dims, nh));
        (dims, Some(nh))
    }
}

impl Factory for SingleCabinet {
    fn natural_size(&self, inst: u64) -> Option<[f64; 3]> {
        Some(self.extents(self.dims, inst, None))
    }
    fn exact(&self, inst: u64) -> Option<cabnodes::ExactMesh> {
        let mut r = inst_rng(self.fseed, inst);
        let (dims, nh) = self.dims_for(inst);
        Some(cabdoor::single_cabinet_gmesh(&mut r, dims, Default::default(), nh).to_exact())
    }
    fn exact_slots(&self, inst: u64) -> Option<(Vec<u16>, Vec<Material>)> {
        let mut r = inst_rng(self.fseed, inst);
        let mut pan = cabdoor::SinglePanels::default();
        let (dims, nh) = self.dims_for(inst);
        let g = cabdoor::single_cabinet_core(&mut r, dims, Default::default(), Some(&mut pan), nh);
        let kind = fixed_kind(self.fseed, &[0.5, 0.2, 0.3]);
        // doors get the kind as a string: frame white / black_wood_z / wood (NOT z, doors.py get_material_func); panels white / black_wood_z / wood_z or glass
        let door_frame = plywood(kind, kind == "black_wood", 0);
        let glass = shader_spec("door_glass", "ceramic.glass.shader_glass", 0, [0.1, 0.1, 0.12]);
        let panel = |glass_on: bool| if glass_on { glass.clone() } else { plywood(kind, true, 0) };
        let mats = vec![plywood(kind, true, 0), plywood(kind, false, 0), door_frame, panel(pan.lower_glass), panel(pan.upper_glass)];
        Some((g.tag.iter().map(|&t| (t >> 1) as u16).collect(), mats))
    }
    fn asset(&self, inst: u64) -> Asset {
        let (dims, nh) = self.dims_for(inst);
        let num_h = nh.unwrap_or((((dims[2] - 0.083) / 0.3) as usize).max(1));
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
        let mut dims = [r.uniform(0.25, 0.35), r.uniform(1.0, 4.0), r.uniform(0.5, 1.3)];
        // the solver's chosen size (furn::sizes 'exact'): [width, depth, height] -> dims [depth, length, height]
        if let Some(h) = crate::api::size_hint() {
            dims = [h[1], h[0], h[2]];
        }
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
    fn exact_slots(&self, i: u64) -> Option<(Vec<u16>, Vec<Material>)> {
        let fs = self.fseed?;
        let mut r = inst_rng(fs, i);
        let spec = cabdoor::KcSpec { dims: self.dims, widths: self.widths.clone(), drawer_only: false };
        let mut picks = vec![];
        let (g, _) = cabdoor::kitchen_cabinet_core(&mut r, &spec, Default::default(), None, Some(&mut picks));
        let kind = fixed_kind(fs, &[0.4, 0.3, 0.3]);
        Some(with_knob(&g.tag, kitchen_slot_mats(kind, &picks, inst_seed(fs, i))))
    }
    fn asset(&self, i: u64) -> Asset {
        let mut a = Asset::default();
        // python samples the dimensions once per factory (KitchenCabinetFactory.sample_params under FixedSeed(factory_seed)), and the exact
        // geometry (`exact`) uses them: the approximate asset (its collision boxes are the solver geometry of the populated cabinet) must too
        let _ = i;
        self.build(&mut a, [0.0, 0.0, 0.0]);
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

#[derive(Clone)]
pub struct KitchenSpace {
    dims: [f64; 3],
    island: bool,
    bottom_h: f64,
    top_h: f64,
    factory_seed: u64,
    /// one leg of an L (ig_kit::api::kitchen_join): [blind end, blind width, joined end, wall-cabinet reach]
    join: Option<[f64; 4]>,
}

impl KitchenSpace {
    fn new(rng: &mut Rng, island: bool, factory_seed: u64) -> KitchenSpace {
        let mut r = rng.clone();
        // python: dimensions = mathutils.Vector((...)) => float32 components (also after `dimensions.x *= uniform(1.5, 2)`)
        let f = |x: f64| x as f32 as f64;
        let mut dims = [f(r.uniform(0.7, 1.0)), f(r.uniform(1.7, 5.0)), f(r.uniform(2.3, 2.5))];
        if island {
            dims[0] = f(dims[0] * r.uniform(1.5, 2.0));
        } else if let Some(h) = crate::api::size_hint() {
            // the solver's chosen size (furn::sizes): run length = width, depth, height incl. the top cabinets
            dims = [f(h[1]), f(h[0]), f(h[2])];
        }
        // (a worktop near the standard 0.9 m: 0.8-1.0 m base cabinets plus plinth and top made 1.0-1.2 m counters, ranges stretched to
        // match and window sills clipped)
        let bottom_h = r.uniform(0.74, 0.8);
        let top_h = r.uniform(0.8, 1.0);
        let join = if island { None } else { crate::api::kitchen_join() };
        KitchenSpace { dims, island, bottom_h, top_h, factory_seed, join }
    }
}

impl KitchenSpace {
    /// KitchenCabinetFactory(fseed, dimensions=(x, y - 0.15, bottom_h), drawer_only=True)(i=0)
    fn bottom_cabinet(&self) -> cabnodes::GMesh {
        self.bottom_cabinet_t(None)
    }
    fn bottom_cabinet_t(&self, picks: Option<&mut Vec<cabdoor::DrawerPick>>) -> cabnodes::GMesh {
        let [x, y, _z] = self.dims;
        let dims = [x, y - 0.15, self.bottom_h];
        let mut r = Rng::seeded(self.factory_seed);
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s = np_sum(&ws);
        let widths: Vec<f64> = ws.iter().map(|w| w / s * dims[1]).collect();
        let mut ri = inst_rng(self.factory_seed, 0);
        let spec = cabdoor::KcSpec { dims, widths, drawer_only: true };
        cabdoor::kitchen_cabinet_core(&mut ri, &spec, Default::default(), None, picks).0
    }
}

impl KitchenSpace {
    fn cabinet(&self, dims: [f64; 3], drawer_only: bool, inst: u64, prev: Option<(f64, f64, f64, f64)>) -> (cabnodes::GMesh, (f64, f64, f64, f64)) {
        self.cabinet_t(dims, drawer_only, inst, prev, None)
    }
    fn cabinet_t(&self, dims: [f64; 3], drawer_only: bool, inst: u64, prev: Option<(f64, f64, f64, f64)>, picks: Option<&mut Vec<cabdoor::DrawerPick>>) -> (cabnodes::GMesh, (f64, f64, f64, f64)) {
        let mut r = Rng::seeded(self.factory_seed);
        let n = ((dims[1] / 0.45) as usize).max(1);
        let ws: Vec<f64> = (0..n).map(|_| r.uniform(0.55, 1.0)).collect();
        let s = np_sum(&ws);
        let widths: Vec<f64> = ws.iter().map(|w| w / s * dims[1]).collect();
        let mut ri = inst_rng(self.factory_seed, inst);
        let spec = cabdoor::KcSpec { dims, widths, drawer_only };
        cabdoor::kitchen_cabinet_core(&mut ri, &spec, Default::default(), prev, picks)
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
        let mut parts = parts;
        if !self.island {
            self.scale_to_dims(&mut parts);
            self.fit_base_width(&mut parts);
            self.apply_join(&mut parts);
        }
        hwexact::join_fwd(&parts)
    }

    /// the base cabinets and the worktop across the run's full width (the wall cabinets set it, 3 cm wider: the base stopped 1.5 cm
    /// short of the appliances beside it, and showed 3 cm short where a window took the wall cabinets)
    fn fit_base_width(&self, parts: &mut [cabnodes::GMesh]) {
        let [_, y, _] = self.dims;
        let yr = |g: &cabnodes::GMesh| g.v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v[1]), b.max(v[1])));
        let (a0, a1) = parts.iter().map(yr).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), (c, d)| (a.min(c), b.max(d)));
        if (a1 - a0 - y as f32).abs() > 0.01 { return }
        for g in parts.iter_mut().take(2) {
            let (b0, b1) = yr(g);
            if b1 - b0 < 0.05 { continue }
            let k = (a1 - a0) / (b1 - b0);
            for v in g.v.iter_mut() { v[1] = a0 + (v[1] - b0) * k; }
        }
    }

    /// one leg of an L, after the scaling to size: the corner leg's base cabinets leave the corner square (`blind` m at that end,
    /// under the worktop, behind the other leg), and the other leg's wall cabinet at the joint reaches `reach` m on into the corner
    /// to meet the corner leg's wall cabinets (parts: bottom, countertop, wall cabinets)
    fn apply_join(&self, parts: &mut [cabnodes::GMesh]) {
        let Some([bs, bw, js, reach]) = self.join else { return };
        let yr = |g: &cabnodes::GMesh| g.v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v[1]), b.max(v[1])));
        // (an end of +-2: both ends -- the middle run of a U)
        if bs != 0.0 && bw > 0.0 && !parts.is_empty() {
            let (b0, b1) = yr(&parts[0]);
            let (w, bw) = (b1 - b0, bw as f32);
            let (cut_lo, cut_hi) = (if bs < 0.0 || bs.abs() > 1.5 { bw } else { 0.0 }, if bs > 0.0 { bw } else { 0.0 });
            if w > cut_lo + cut_hi + 0.1 {
                let k = (w - cut_lo - cut_hi) / w;
                let (xz_lo, xz_hi) = parts[0].v.iter().fold(([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]), |(l, h), v| ([l[0].min(v[0]), l[1].min(v[2])], [h[0].max(v[0]), h[1].max(v[2])]));
                for v in parts[0].v.iter_mut() { v[1] = b0 + cut_lo + (v[1] - b0) * k; }
                // the corner square closed by a blind box in the cabinets' finish (left empty, the joint showed a slot into the void)
                let tag = parts[0].tag.iter().copied().find(|t| t >> 1 != cabdoor::KNOB_SLOT).unwrap_or(0);
                for (y0, y1) in [(b0, b0 + cut_lo), (b1 - cut_hi, b1)] {
                    if y1 - y0 < 0.01 { continue }
                    let base = parts[0].v.len() as u32;
                    let (x0, x1, z0, z1) = (xz_lo[0], xz_hi[0], xz_lo[1], xz_hi[1]);
                    parts[0].v.extend([[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]);
                    for f in [[0u32, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
                        parts[0].p.push(f.iter().map(|i| base + i).collect());
                        parts[0].tag.push(tag);
                    }
                }
            }
        }
        if js.abs() > 1.5 && reach > 0.0 && parts.len() > 2 {
            for e in [-1.0f64, 1.0] { let mut j = self.clone(); j.join = Some([0.0, 0.0, e, reach]); j.apply_join(parts); }
            return;
        }
        if js != 0.0 && reach > 0.0 && parts.len() > 2 {
            // the wall cabinet nearest the joined end
            let ends: Vec<(usize, (f32, f32))> = (2..parts.len()).map(|i| (i, yr(&parts[i]))).collect();
            let Some(&(i, (u0, u1))) = ends.iter().min_by(|a, b| { let e = |r: &(f32, f32)| if js < 0.0 { r.0 } else { -r.1 }; e(&a.1).partial_cmp(&e(&b.1)).unwrap() }) else { return };
            // (a filler box in the cabinet's finish, not the cabinet stretched: stretching widened its doors and slanted the handle)
            let r = reach as f32;
            let (xz_lo, xz_hi) = parts[i].v.iter().fold(([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]), |(l, h), v| ([l[0].min(v[0]), l[1].min(v[2])], [h[0].max(v[0]), h[1].max(v[2])]));
            // (to the base's end plus the reach: from the cabinet's end it fell short by the cabinets' inset)
            let (b0, b1) = yr(&parts[0]);
            let (y0, y1) = if js < 0.0 { (b0 - r, u0) } else { (u1, b1 + r) };
            let tag = parts[i].tag.first().copied().unwrap_or(0);
            let base = parts[i].v.len() as u32;
            let (x0, x1, z0, z1) = (xz_lo[0], xz_hi[0], xz_lo[1], xz_hi[1]);
            parts[i].v.extend([[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]);
            for f in [[0u32, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
                parts[i].p.push(f.iter().map(|q| base + q).collect());
                parts[i].tag.push(tag);
            }
        }
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
        self.raw_parts_t(inst, None)
    }

    /// raw_parts; with `slots` the faces carry material slots in `tag >> 1`: 0 frame/doors (K_z), 1 boards (K), 2 countertop, 3.. drawers
    /// (their picks and instance seeds are appended to `slots`)
    fn raw_parts_t(&self, inst: u64, mut slots: Option<&mut Vec<(cabdoor::DrawerPick, u64)>>) -> Vec<cabnodes::GMesh> {
        let [x, y, z] = self.dims;
        // remap a cabinet's local slots (0 frame, 1 board, 2+n drawer n) into the global numbering
        let mut remap = |g: &mut cabnodes::GMesh, pk: Vec<cabdoor::DrawerPick>, seed: u64, slots: &mut Option<&mut Vec<(cabdoor::DrawerPick, u64)>>| {
            let Some(sl) = slots.as_deref_mut() else { return };
            let base = sl.len();
            for t in g.tag.iter_mut() {
                let s = (*t >> 1) as usize;
                let ns = if s >= 2 { (3 + base + s - 2).min(127) } else { s };
                *t = ((ns << 1) as u8) | (*t & 1);
            }
            sl.extend(pk.into_iter().map(|p| (p, seed)));
        };
        let mut pk0 = vec![];
        let mut bottom = self.bottom_cabinet_t(slots.is_some().then_some(&mut pk0));
        remap(&mut bottom, pk0, inst_seed(self.factory_seed, 0), &mut slots);
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
        slab.tag = vec![if slots.is_some() { 4 } else { 0 }; slab.p.len()];
        let slab = Self::translated(slab, [0.0, ((lo[1] + hi[1]) / 2.0) as f64, hi[2] as f64]);
        let mut parts = vec![bottom, slab];
        if !self.island {
            let mut r = inst_rng(self.factory_seed, inst);
            let tmw = r.uniform(1.0, 1.3);
            let ctw = (y - tmw) / 2.0 - 0.05;
            let th = self.top_h;
            // left and right share one KitchenCabinetFactory object: the second instance reuses the first one's frame parameters
            let (mut pl, mut pr, mut pm) = (vec![], vec![], vec![]);
            let on = slots.is_some();
            // a run narrower than the middle wall cabinet and two side ones: one wall cabinet as wide as the base (a 1.0-1.3 m
            // cabinet over a 0.6 m run widened the set's box, and scaling it to the run's size shrank the base cabinets into its middle)
            if y < tmw + 0.45 {
                let (mut mid, _) = self.cabinet_t([x / 2.0, y - 0.15, th], false, 0, None, on.then_some(&mut pm));
                remap(&mut mid, pm, inst_seed(self.factory_seed, 0), &mut slots);
                parts.push(Self::translated(mid, [-x / 4.0, 0.0, z - th]));
                return parts;
            }
            let (mut left, rnd) = self.cabinet_t([x / 2.0, ctw, th], false, 0, None, on.then_some(&mut pl));
            remap(&mut left, pl, inst_seed(self.factory_seed, 0), &mut slots);
            let (mut right, _) = self.cabinet_t([x / 2.0, ctw, th], false, 1, Some(rnd), on.then_some(&mut pr));
            remap(&mut right, pr, inst_seed(self.factory_seed, 1), &mut slots);
            // (the middle cabinet as deep and tall as the side ones: python's deeper, shorter middle unit stood out of the row)
            let (mut mid, _) = self.cabinet_t([x / 2.0, tmw, th], false, 0, None, on.then_some(&mut pm));
            remap(&mut mid, pm, inst_seed(self.factory_seed, 0), &mut slots);
            parts.push(Self::translated(left, [-x / 4.0, 0.0, z - th]));
            parts.push(Self::translated(right, [-x / 4.0, y - ctw, z - th]));
            parts.push(Self::translated(mid, [-x / 4.0, y / 2.0 - tmw / 2.0, z - th]));
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

impl KitchenSpace {
    fn island_hint(&self, dense: bool) -> (V3, V3) {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        if dense {
            // dense mode: the island's cabinet row without building its door / drawer meshes: n = floor((y - 0.15) / 0.45) cells of
            // width w + 0.0405 each (side boards + gap); x from the carcass (-0.005) to the front (+0.055 is the larger of the door and
            // drawer protrusions, up to 2.7 cm generous), z up to the cabinet top (+ up to 8.2 cm of plinth / handles)
            let [x, y, _z] = self.dims;
            let d1 = y - 0.15;
            let n = ((d1 / 0.45) as usize).max(1) as f64;
            lo = [-x / 2.0 - 0.005, -0.001, 0.0];
            hi = [x / 2.0 + 0.055, d1 + n * 0.0405 + 0.0005, self.bottom_h + 0.082];
        } else {
            let e = self.bottom_cabinet();
            for v in &e.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k] as f64);
                    hi[k] = hi[k].max(v[k] as f64);
                }
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
        (lo, hi)
    }
}

impl Factory for KitchenSpace {
    fn bounds_hint(&self, _i: u64) -> Option<(V3, V3)> {
        if !self.island {
            return None;
        }
        Some(self.island_hint(crate::dense()))
    }
    fn placeholder_exact(&self, _i: u64) -> Option<cabnodes::ExactMesh> {
        if self.island {
            return None;
        }
        let [x, y, z] = self.dims;
        // (the base box as deep as the cabinets: python's 8 % margin left the run 3 cm off its wall and an L's legs 7 cm apart)
        let (l1, h1) = super::geomkit::new_bbox_f32(-x / 2.0, x / 2.0, 0.0, y, 0.0, self.bottom_h + 0.13);
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
    fn exact_slots(&self, i: u64) -> Option<(Vec<u16>, Vec<Material>)> {
        let mut dr = vec![];
        let mut parts = self.raw_parts_t(i, Some(&mut dr));
        // the same steps as `assemble` (the L legs' blind boxes add faces: slots one short of the mesh were dropped, and every
        // part came out in one material)
        if !self.island {
            self.scale_to_dims(&mut parts);
            self.fit_base_width(&mut parts);
            self.apply_join(&mut parts);
        }
        let g = hwexact::join_fwd(&parts);
        let kind = fixed_kind(self.factory_seed, &[0.4, 0.3, 0.3]);
        let mut mats = vec![plywood(kind, true, 0), plywood(kind, false, 0), cls_spec("counter_marble", "ceramic.Marble", self.factory_seed + 7, [0.85, 0.85, 0.85])];
        // one door / drawer front material for the whole run (each front drew its own of 5 shaders: patchwork kitchens)
        if let Some((p0, s0)) = dr.first() { let m0 = drawer_material(p0, *s0); mats.extend(dr.iter().map(|_| m0.clone())); }
        Some(with_knob(&g.tag, mats))
    }
    fn asset(&self, i: u64) -> Asset {
        let mut a = Asset::default();
        let mut parts = self.raw_parts(i);
        if !self.island {
            self.scale_to_dims(&mut parts);
            self.fit_base_width(&mut parts);
            self.apply_join(&mut parts);
        }
        // part 0/2.. = cabinets (KitchenCabinetFactory(factory_seed): z-textured frame material), part 1 = countertop (shader_marble)
        let kind = fixed_kind(self.factory_seed, &[0.4, 0.3, 0.3]);
        let cab = plywood(kind, true, self.factory_seed);
        let top = cls_spec("counter_marble", "ceramic.Marble", self.factory_seed + 7, [0.85, 0.85, 0.85]);
        let knob = knob_metal(kind == "white");
        for (k, g) in parts.iter().enumerate() {
            if k == 1 { a.mesh(g.to_mesh(), &top, None); continue }
            // the knobs (cabdoor::KNOB_SLOT faces) in metal
            let (kn, rest) = split_knobs(g);
            a.mesh(rest.to_mesh(), &cab, None);
            if !kn.p.is_empty() { a.mesh(kn.to_mesh(), &knob, None); }
        }
        a
    }
    fn placeholder(&self, _i: u64) -> Option<Asset> {
        let [x, y, z] = self.dims;
        let mut a = Asset::default();
        let m = mat("ph", [0.5; 3], 0.5);
        a.board([-x / 2.0, 0.0, 0.0], [x / 2.0, y, self.bottom_h + 0.13], &m);
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
        // 60 % of TV stands are media consoles (drawers / doors / slats / cubbies on legs, a plinth or a frame), the rest infinigen's
        // cubby bench; the choice is its own stream (NO_CONSOLE=1: off)
        "TVStandFactory" => {
            let mut pick = Rng::seeded(crate::int_hash(&["media_console", &seed.to_string()]));
            if std::env::var_os("NO_CONSOLE").is_none() && pick.f64() < 0.6 { Box::new(super::console::MediaConsole::new(&mut rng.clone())) } else { Box::new(CellShelf::new(rng, true, seed)) }
        }
        "SimpleDeskFactory" => Box::new(SimpleDesk::new(rng, false, factory_seed)),
        "SidetableDeskFactory" => Box::new(SimpleDesk::new(rng, true, factory_seed)),
        "SingleCabinetFactory" => Box::new(SingleCabinet::new(rng, factory_seed)),
        "KitchenCabinetFactory" => Box::new(KitchenCabinet { fseed: Some(factory_seed), ..KitchenCabinet::with_dims(rng, None) }),
        "KitchenSpaceFactory" => Box::new(KitchenSpace::new(rng, false, seed)),
        "KitchenIslandFactory" => Box::new(KitchenSpace::new(rng, true, seed)),
        _ => return None,
    })
}

#[cfg(test)]
mod island_tests {
    use super::*;
    #[test]
    fn island_cabinet_bounds_vs_dims() {
        let t = web_time::Instant::now();
        for seed in 0..40u64 {
            let ks = KitchenSpace::new(&mut Rng::seeded(seed), true, seed);
            let e = ks.bottom_cabinet();
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for v in &e.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k] as f64);
                    hi[k] = hi[k].max(v[k] as f64);
                }
            }
        }
        eprintln!("40 island cabinets {:?}", t.elapsed());
        let mut st = [[f64::INFINITY, f64::NEG_INFINITY]; 5];
        for seed in 0..60u64 {
            let ks = KitchenSpace::new(&mut Rng::seeded(seed), true, seed);
            let e = ks.bottom_cabinet();
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for v in &e.v {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k] as f64);
                    hi[k] = hi[k].max(v[k] as f64);
                }
            }
            let d = [lo[0] + ks.dims[0] / 2.0, hi[0] - ks.dims[0] / 2.0, lo[1], hi[1] - (ks.dims[1] - 0.15), hi[2] - ks.bottom_h];
            for k in 0..5 {
                st[k][0] = st[k][0].min(d[k]);
                st[k][1] = st[k][1].max(d[k]);
            }
        }
        {
            let (mut worst, mut under) = ([0.0f64; 6], 0);
            for seed in 0..60u64 {
                let ks = KitchenSpace::new(&mut Rng::seeded(seed), true, seed);
                let (dl, dh) = (ks.island_hint(true), ks.island_hint(false));
                for k in 0..3 {
                    let (a, b) = (dl.0[k] - dh.0[k], dl.1[k] - dh.1[k]);
                    worst[k] = worst[k].max(a.abs());
                    worst[3 + k] = worst[3 + k].max(b.abs());
                    if a > 1e-6 || b < -1e-6 { under += 1; }
                }
            }
            eprintln!("ISLAND dense vs exact hint: worst |dlo| {:?} |dhi| {:?} cases where dense is smaller {under}", &worst[0..3], &worst[3..6]);
        }
        eprintln!("ISLAND delta ranges (lo_x+x/2, hi_x-x/2, lo_y, hi_y-(y-0.15), hi_z-bh): {:?}", st);
    }
}
