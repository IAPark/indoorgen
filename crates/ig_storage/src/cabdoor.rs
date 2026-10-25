//! shelves/doors.py and drawers.py node graphs (float32). `subdivide` / merge-by-distance of the ramped edges are optional steps
//! (they only refine topology; planar surfaces are identical), see `RampedOpts`.
use super::cabnodes::*;
use super::*;

pub const A90: f32 = 1.5708;

/// CurveToMesh(line (0,0,0)->(0,0,h), profile of 3 points in the xy plane, fill caps)
pub fn curve_prism(h: f32, p: [[f32; 2]; 3]) -> GMesh {
    let mut g = GMesh::new();
    for z in [0.0f32, h] {
        for q in p {
            g.v.push([q[0], q[1], z]);
        }
    }
    let mut push = |g: &mut GMesh, f: Vec<u32>| {
        g.p.push(f);
        g.tag.push(0);
    };
    for i in 0..3u32 {
        let n = (i + 1) % 3;
        push(&mut g, vec![i, n, 3 + n, 3 + i]);
    }
    push(&mut g, vec![2, 1, 0]);
    push(&mut g, vec![3, 4, 5]);
    // fill_mesh_topology: edges along the curve (one per profile vertex), then the ring edges of ring 0 and ring 1
    g.e = vec![[0, 3], [1, 4], [2, 5], [0, 1], [1, 2], [2, 0], [3, 4], [4, 5], [5, 3]];
    g
}

#[derive(Clone, Copy)]
pub struct RampedOpts {
    /// apply Merge by Distance + Subdivide Mesh (level 4) like the node graph (topology exact)
    pub refine: bool,
}

impl Default for RampedOpts {
    fn default() -> Self {
        RampedOpts { refine: true }
    }
}

/// nodegroup_ramped_edge (before Merge by Distance / Subdivide). Returns the mesh and applies the final x shift.
pub fn ramped_edge(height: f32, t2: f32, width: f32, t1: f32, ang: f32, o: RampedOpts) -> GMesh {
    let add = height + 0.0;
    let add_1 = width + 0.0;
    let multiply = add_1 * 0.5;
    let tangent = ang.tan();
    let add_3 = t2 + 0.0;
    let multiply_1 = tangent * add_3;
    let subtract = add_1 - multiply_1;
    let subtract_1 = multiply - subtract;
    let add_4 = t1 + 0.0;
    let add_5 = add_4 + add_3;
    let multiply_2 = multiply * -1.0;
    let ctm = curve_prism(add, [[subtract_1, add_4], [multiply_2, add_4], [subtract_1, add_5]]);
    let cube = gcube([add_1, add_4, add], [2, 2, 2]);
    let multiply_3 = add_4 * 0.5;
    let transform = cube.translated([0.0, multiply_3, 0.0]);
    let cube_1 = gcube([subtract, add_3, add], [2, 2, 2]);
    let multiply_4 = multiply_1 * 0.5;
    let multiply_5 = add_3 * 0.5;
    let add_6 = add_4 + multiply_5;
    let transform_1 = cube_1.translated([multiply_4, add_6, 0.0]);
    let join = GMesh::join(&[transform, transform_1]);
    let multiply_6 = add * 0.5;
    let transform_4 = join.translated([0.0, 0.0, multiply_6]);
    let mut j1 = GMesh::join(&[ctm, transform_4]);
    if o.refine {
        j1 = super::gweld::merge_by_distance(&j1, 0.0001f32);
        j1 = super::gweld::subdivide(&j1, 4);
    }
    let multiply_7 = add_1 * -0.5;
    j1.translated([multiply_7, 0.0, 0.0])
}

/// nodegroup_double_rampled_edge
pub fn double_rampled_edge(height: f32, t2: f32, width: f32, t1: f32, ang: f32, o: RampedOpts) -> GMesh {
    let add = height + 0.0;
    let add_1 = width + 0.0;
    let add_2 = ang + 0.0;
    let tangent = add_2.tan();
    let add_3 = t2 + 0.0;
    let multiply = tangent * add_3;
    let multiply_1 = 2.0f32 * multiply;
    let subtract = add_1 - multiply_1;
    let multiply_2 = subtract * 0.5;
    let multiply_3 = multiply_2 * -1.0;
    let add_4 = t1 + 0.0;
    let add_5 = add_4 + add_3;
    let multiply_4 = add_1 * 0.5;
    let multiply_5 = multiply_4 * -1.0;
    let prof = [[multiply_3, add_4], [multiply_5, add_4], [multiply_3, add_5]];
    let ctm = curve_prism(add, prof);
    let cube = gcube([add_1, add_4, add], [2, 2, 2]);
    let multiply_6 = add_4 * 0.5;
    let transform = cube.translated([0.0, multiply_6, 0.0]);
    let cube_1 = gcube([subtract, add_3, add], [2, 2, 2]);
    let multiply_7 = add_3 * 0.5;
    let add_6 = add_4 + multiply_7;
    let transform_1 = cube_1.translated([0.0, add_6, 0.0]);
    let join = GMesh::join(&[transform, transform_1]);
    let multiply_8 = add * 0.5;
    let transform_4 = join.translated([0.0, 0.0, multiply_8]);
    let mprof = [[-prof[0][0], prof[0][1]], [-prof[1][0], prof[1][1]], [-prof[2][0], prof[2][1]]];
    let ctm1 = curve_prism(add, mprof);
    let mut j = GMesh::join(&[ctm, transform_4, ctm1]);
    if o.refine {
        j = super::gweld::merge_by_distance(&j, 0.0001f32);
        j = super::gweld::subdivide(&j, 4);
    }
    j
}

pub struct DoorParams {
    pub height: f64,
    pub width: f64,
    pub e1: f64,
    pub ew: f64,
    pub e2: f64,
    pub ang: f64,
    pub knob_r: f64,
    pub knob_len: f64,
    pub has_mid: bool,
    pub left: bool,
}

/// geometry_door_nodes (+ mid board, knob), result before the Object Info placement of the cabinet.
pub fn door_gmesh(p: &DoorParams, o: RampedOpts) -> GMesh {
    door_gmesh_t(p, o, None)
}

/// door_gmesh with per-face material tags: `tags` = [frame (edges + knob), lower panel, upper panel] (written into GMesh::tag; None keeps all 0)
pub fn door_gmesh_t(p: &DoorParams, o: RampedOpts, tags: Option<[u8; 3]>) -> GMesh {
    let f = |x: f64| x as f32;
    let (h, w, e1, ew, e2, ang) = (f(p.height), f(p.width), f(p.e1), f(p.ew), f(p.e2), f(p.ang));
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    let bt = f(p.e1 - 0.005);
    let a = ramped_edge(h, e2, ew, e1, ang, o);
    let b = ramped_edge(w, e2, ew, e1, ang, o);
    // panel_edge_frame
    let ma = w.mul_add(0.5, 0.001);
    let value = ma * -1.0;
    let t7 = b.transformed([0.0, -0.0001, 0.0], z3, [0.9999, 1.0, 1.0]);
    let mul1 = ma * 1.0;
    let add_ = mul1 + -0.0001;
    let add1 = h + 0.0001;
    let t3 = t7.transformed([add_, 0.0, add1], [0.0, -A90, 0.0], one);
    let add2 = value + 0.0001;
    let t2_ = t7.transformed([add2, 0.0, 0.0], [0.0, A90, 0.0], one);
    let t0 = a.translated([ma, 0.0, 0.0]);
    let t1_ = t0.transformed(z3, z3, [-1.0, 1.0, 1.0]);
    let frame_geo = GMesh::join(&[t3, t2_, t1_, t0]);
    let add_v = value + 0.0001;
    // mid board
    let (mid, mid_h) = if p.has_mid {
        let add = w + -0.0001;
        let add_1 = bt + 0.0;
        let multiply = h * 0.5;
        let multiply_k = add_1 * 0.5;
        let add_k = multiply_k + 0.004;
        let add_2 = multiply + -0.0001;
        let cube = gcube([add, add_1, add_2], [5, 5, 5]);
        let multiply_1 = multiply * 0.5;
        let t4 = cube.translated([0.0, add_k, multiply_1]);
        let multiply_2 = multiply * 1.5;
        let t7 = cube.translated([0.0, add_k, multiply_2]);
        let (mut t4, mut t7) = (t4, t7);
        if let Some(t) = tags {
            set_tag(&mut t4, t[1]);
            set_tag(&mut t7, t[2]);
        }
        (GMesh::join(&[t4, t7]), multiply)
    } else {
        let add = w + -0.0001;
        let add_1 = bt + 0.0;
        let multiply_k = add_1 * 0.5;
        let add_k = multiply_k + 0.004;
        let multiply = h * 1.0;
        let add_2 = multiply + -0.0001;
        let cube = gcube([add, add_1, add_2], [5, 5, 5]);
        let multiply_1 = multiply * 0.5;
        let mut c = cube.translated([0.0, add_k, multiply_1]);
        if let Some(t) = tags {
            set_tag(&mut c, t[1]);
        }
        (c, multiply)
    };
    let mut frame = vec![frame_geo];
    if p.has_mid {
        let d = double_rampled_edge(w, e2, ew, e1, ang, o);
        frame.push(d.transformed([add_v, -0.0001, mid_h], [0.0, A90, 0.0], one));
    }
    // knob handle
    let kr = f(p.knob_r);
    let kl = f(p.knob_len);
    let knob_mid = h * 0.5;
    let knob = {
        let add = e2 + e1;
        let add_1 = add + kl;
        let cyl = gcylinder(64, kr, add_1, 1);
        let subtract = w - ew;
        let multiply = subtract * -0.5;
        let add_2 = multiply + -0.005;
        let multiply_1 = add_1 * 0.5;
        cyl.transformed([add_2, multiply_1, knob_mid], [A90, 0.0, 0.0], one)
    };
    frame.push(knob);
    let mut join1 = GMesh::join(&frame);
    if let Some(t) = tags {
        set_tag(&mut join1, t[0]);
    }
    let geos = GMesh::join(&[join1, mid]);
    let multiply = w * -0.5;
    let tr = geos.translated([multiply, 0.0, 0.0]);
    let tri = tr.triangulated();
    let mut t1 = tri.transformed(z3, z3, [if p.left { -1.0 } else { 1.0 }, 1.0, 1.0]);
    if p.left {
        // FlipFaces: keeps the first corner, reverses the others
        for f in t1.p.iter_mut() {
            f[1..].reverse();
        }
    }
    t1.transformed(z3, [0.0, 0.0, -A90], one)
}

/// shelf_gmesh(p, true) with per-face material tags: frame parts (sides, back, bottom boards) get `frame_t`, division boards + screws `board_t` (| support bit)
pub fn shelf_gmesh_slots(p: &ShelfParams, frame_t: u8, board_t: u8) -> GMesh {
    let tag_support = true;
    let f = |x: f64| x as f32;
    let (st, d, h) = (f(p.side_t), f(p.shelf_depth), f(p.shelf_height));
    let sw = f(p.shelf_width);
    let bt = f(p.back_t);
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    // side boards
    let hh = h + 0.002f32;
    let dd = d + 0.004f32;
    let mut sides = vec![];
    for &x in &p.side_x {
        sides.push(gcube([st, dd, hh], [2, 2, 2]).translated([f(x), 0.0, hh * 0.5]));
    }
    // back board
    let width = sw + f(p.side_t * 2.0);
    let bh = h + -0.001f32;
    let back = {
        let ma = d.mul_add(-0.5, bt * -0.5);
        gcube([width, bt, bh], [2, 2, 2]).translated([0.0, ma, bh * 0.5])
    };
    let mut bottoms = vec![];
    let bhgt = f(p.bottom_h);
    for i in 0..p.cell_w.len() {
        let sub = d * 0.5 - f(p.y_gap);
        bottoms.push(gcube([f(p.cell_w[i]), st, bhgt], [2, 2, 2]).translated([f(p.div_x[i]), sub, bhgt * 0.5]));
    }
    let mut frame_in = vec![back];
    frame_in.extend(sides);
    frame_in.extend(bottoms);
    let mut frame = GMesh::join(&frame_in);
    set_tag(&mut frame, frame_t);
    // division boards
    let dt = f(p.div_t);
    let mut boards = vec![];
    for i in 0..p.cell_w.len() {
        for j in 0..p.div_z.len() {
            let w = f(p.cell_w[i]);
            let cube = if tag_support { gcube_tagged([w, d, dt]) } else { gcube([w, d, dt], [2, 2, 2]) };
            // screw head group
            let sx = w * 0.5 - f(p.screw_wgap);
            let m1 = d * 0.5;
            let gap = f(p.screw_wgap) + 0.0;
            let s1 = m1 - gap;
            let m2 = s1 * -1.0;
            let m3 = dt * -0.5;
            let cyl = gcylinder(32, f(p.screw_r), f(p.screw_depth), 2);
            let t1 = cyl.translated([sx, m2, m3]);
            let t6 = cyl.translated([sx, s1, m3]);
            let j2 = GMesh::join(&[t1, t6]);
            let t4 = j2.transformed(z3, z3, [-1.0, 1.0, 1.0]);
            let screws = GMesh::join(&[t4, j2]);
            let joined = GMesh::join(&[cube, screws]);
            boards.push(joined.translated([f(p.div_x[i]), 0.0, f(p.div_z[j])]));
        }
    }
    let mut boards = GMesh::join(&boards);
    for t in boards.tag.iter_mut() {
        *t |= board_t;
    }
    let all = GMesh::join(&[frame, boards]);
    let tri = all.triangulated();
    let _ = one;
    tri.transformed(z3, [0.0, 0.0, -1.5708f32], one)
}

/// all faces of `g` get tag `t`
pub fn set_tag(g: &mut GMesh, t: u8) {
    for x in g.tag.iter_mut() {
        *x = t;
    }
}

pub struct DrawerParams {
    pub board_t: f64,
    pub board_w: f64,
    pub board_h: f64,
    pub depth: f64,
    pub side_h: f64,
    pub width: f64,
    pub knob_r: f64,
    pub knob_len: f64,
}

/// cabinet drawer geometry_nodes (drawers.py)
pub fn drawer_gmesh(p: &DrawerParams) -> GMesh {
    let f = |x: f64| x as f32;
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    let (t, bw, bh) = (f(p.board_t), f(p.board_w), f(p.board_h));
    // drawer_door_board
    let board = {
        let (add, add_1, add_2) = (bw + 0.0, t + 0.0, bh + 0.0);
        let cube = gcube([add, add_1, add_2], [5, 5, 5]);
        let multiply = add_1 * -0.5;
        let multiply_1 = add_2 * 0.5;
        cube.translated([0.0, multiply, multiply_1])
    };
    // door_knob
    let knob = {
        let add = f(p.knob_len) + 0.0;
        let cyl = gcylinder(64, f(p.knob_r), add, 1);
        let multiply = add * 0.5;
        let add_1 = multiply + 0.0001;
        let add_2 = bh + 0.0;
        let multiply_1 = add_2 * 0.5;
        cyl.transformed([0.0, add_1, multiply_1], [A90, 0.0, 0.0], one)
    };
    // kallax_drawer_frame
    let frame = {
        let depth = f(p.depth - p.board_t);
        let (add, add_1, add_2) = (t + 0.0, depth + 0.0, f(p.side_h) + 0.0);
        let cube = gcube([add, add_1, add_2], [4, 4, 4]);
        let add_3 = f(p.width) + 0.0;
        let multiply = add_3 * 0.5;
        let multiply_1 = add_1 * -0.5;
        let add_4 = multiply_1 + -0.0001;
        let multiply_add = add_2.mul_add(0.5, 0.01);
        let transform = cube.translated([multiply, add_4, multiply_add]);
        let transform_1 = transform.transformed(z3, z3, [-1.0, 1.0, 1.0]);
        let add_5 = add + -0.0001;
        let add_6 = add_3 + add_5;
        let cube_1 = gcube([add_6, add_1, add], [4, 4, 4]);
        let multiply_add_1 = add_1.mul_add(-0.5, -0.0001);
        let transform_2 = cube_1.translated([0.0, multiply_add_1, 0.01]);
        let cube_2 = gcube([add_3, add, add_2], [4, 4, 4]);
        let multiply_2 = add * 0.5;
        let multiply_add_2 = add_1.mul_add(-1.0, multiply_2);
        let multiply_add_3 = add_2.mul_add(0.5, 0.01);
        let transform_3 = cube_2.translated([0.0, multiply_add_2, multiply_add_3]);
        GMesh::join(&[transform_1, transform, transform_2, transform_3])
    };
    let all = GMesh::join(&[knob, board, frame]);
    all.triangulated().transformed(z3, [0.0, 0.0, -A90], one)
}

/// get_shelf_material(<Material object>) -> default branch: choice(p) + the shader function's own draws
pub fn shelf_material_draws(r: &mut Rng) {
    shelf_material_pick(r);
}

/// Same, returning the choice (0 shader_shelves_white, 1 shader_rough_plastic, 2 shader_shelves_black_wood, 3 shader_wood, 4 shader_shelves_wood)
/// and the generator state right after the choice (= where the chosen shader function's own draws start).
pub fn shelf_material_pick(r: &mut Rng) -> (usize, Rng) {
    let i = r.choice_p(&[0.3, 0.2, 0.3, 0.1, 0.1]);
    let after = r.clone();
    match i {
        1 => {
            for _ in 0..5 {
                r.f64();
            }
        }
        3 => {
            r.choice_idx(49);
            for _ in 0..9 {
                r.f64();
            }
        }
        _ => {}
    }
    (i, after)
}

/// a drawer's material choice (drawer_material; the knob_material draws are replayed but unused: the drawer node graph sets one material)
#[derive(Clone)]
pub struct DrawerPick {
    pub idx: usize,
    pub after: Rng,
}

pub struct KcSpec {
    /// Dimensions (x depth, y width, z height) of the cabinet row
    pub dims: [f64; 3],
    pub widths: Vec<f64>,
    pub drawer_only: bool,
}

/// KitchenCabinetBaseFactory.create_asset(i): `r` is the instance stream (FixedSeed(int_hash((fseed, i)))).
pub fn kitchen_cabinet_gmesh(r: &mut Rng, spec: &KcSpec, o: RampedOpts) -> GMesh {
    kitchen_cabinet_gmesh_ex(r, spec, o, None).0
}

/// Same, for a factory object that already built an instance: the frame's random parameters (`frame_rnd`, filled into the python
/// factory's `frame_params` by its first cabinet) persist and are not drawn again. Returns the parameters in use.
pub fn kitchen_cabinet_gmesh_ex(r: &mut Rng, spec: &KcSpec, o: RampedOpts, frame_rnd: Option<(f64, f64, f64, f64)>) -> (GMesh, (f64, f64, f64, f64)) {
    kitchen_cabinet_core(r, spec, o, frame_rnd, None)
}

/// Same geometry with per-face material slots in `tag >> 1` (when `picks` is given): 0 frame + doors (K_z), 1 division boards (K), 2 + n = the
/// n-th drawer (its choice is pushed to `picks`). The polygon order is identical to the untagged call.
pub fn kitchen_cabinet_core(r: &mut Rng, spec: &KcSpec, o: RampedOpts, frame_rnd: Option<(f64, f64, f64, f64)>, mut picks: Option<&mut Vec<DrawerPick>>) -> (GMesh, (f64, f64, f64, f64)) {
    use super::cabnodes::*;
    let tagged = picks.is_some();
    let f = |x: f64| x as f32;
    let (dx, _dy, dz) = (spec.dims[0], spec.dims[1], spec.dims[2]);
    let depth = dx - 0.01;
    let num_h = ((dz - 0.06) / 0.3) as usize;
    let cell_h: Vec<f64> = vec![(dz - 0.06) / num_h as f64; num_h];
    let (side_t, div_t, bottom_h) = (0.02, 0.02, 0.06);
    // frame random params (drawn once, by the first cabinet's frame)
    let mut rnd: Option<(f64, f64, f64, f64)> = frame_rnd;
    let mut frames: Vec<GMesh> = vec![];
    let mut attach_comps: Vec<Option<GMesh>> = vec![];
    let mut y_trans = vec![];
    // get_cabinet_params
    {
        let mut accum = 0.0;
        for w in &spec.widths {
            accum += side_t + w / 2.0;
            y_trans.push(accum);
            accum += side_t + w / 2.0 + 0.0005;
        }
    }
    for (k, &w) in spec.widths.iter().enumerate() {
        if rnd.is_none() {
            let y_gap = r.uniform(0.01, 0.05);
            let sd = r.uniform(0.001, 0.004);
            let sr = r.uniform(0.001, 0.004);
            let sw = r.uniform(0.0, 0.02);
            r.uniform(0.025, 0.06);
            r.uniform(0.05, 0.1);
            r.uniform(0.01, 0.025);
            r.uniform(0.002, 0.005);
            r.uniform(0.0, 0.05);
            rnd = Some((y_gap, sd, sr, sw));
        }
        let (y_gap, sd, sr, sw) = rnd.unwrap();
        let mut sp = ShelfParams { shelf_depth: depth, side_t, back_t: 0.01, y_gap, bottom_h, div_t, screw_depth: sd, screw_r: sr, screw_wgap: sw, cell_w: vec![w], cell_h: cell_h.clone(), ..Default::default() };
        sp.derive();
        frames.push(if tagged { shelf_gmesh_slots(&sp, 0, 2) } else { shelf_gmesh(&sp, true) });
        // attach type
        let kind = if spec.drawer_only { r.choice_p(&[0.5, 0.5]) } else { r.choice_p(&[0.4, 0.4, 0.2]) }; // 0 drawer, 1 door, 2 none
        let shelf_w = sp.shelf_width + side_t * 2.0;
        match kind {
            1 => {
                let (dw, hinges): (f64, Vec<[f64; 3]>) = if shelf_w <= 0.6 {
                    (shelf_w, vec![[depth / 2.0 + 0.0025, -shelf_w / 2.0, bottom_h]])
                } else {
                    (shelf_w / 2.0 - 0.0005, vec![[depth / 2.0 + 0.008, -shelf_w / 2.0, bottom_h], [depth / 2.0 + 0.008, shelf_w / 2.0, bottom_h]])
                };
                let dh = sp.div_z[sp.div_z.len() - 1] - sp.div_z[0] + div_t;
                let ew = r.uniform(0.03, 0.05);
                let e2 = r.uniform(0.005, 0.008);
                let ang = r.uniform(0.6, 0.8);
                let kr = r.uniform(0.003, 0.006);
                let kl = r.uniform(0.018, 0.035);
                let _gap = r.uniform(0.05, 0.15);
                if std::env::var("KCDBG").is_ok() { eprintln!("door k={k} dh={dh} dw={dw} ew={ew} e2={e2} ang={ang} kr={kr} kl={kl} hinges={:?}", hinges); }
                let mut dp = DoorParams { height: dh, width: dw, e1: 0.01, ew, e2, ang, knob_r: kr, knob_len: kl, has_mid: false, left: false };
                let right = door_gmesh(&dp, o);
                dp.left = true;
                let left = door_gmesh(&dp, o);
                let z3 = [0f32; 3];
                let mut att = vec![right.translated([f(hinges[0][0]), f(hinges[0][1]), f(hinges[0][2])])];
                if hinges.len() > 1 {
                    att.push(left.translated([f(hinges[1][0]), f(hinges[1][1]), f(hinges[1][2])]));
                }
                let _ = z3;
                let join = GMesh::join(&att);
                attach_comps.push(Some(join.translated([0.0, f(y_trans[k]), 0.0])));
            }
            0 => {
                let mut att = vec![];
                for i in 0..cell_h.len() {
                    let drawer_h = sp.div_z[i + 1] - sp.div_z[i] - div_t;
                    let board_t = r.uniform(0.005, 0.01);
                    let side_h = r.uniform(0.05, 0.2);
                    let dwid = sp.shelf_width - r.uniform(0.015, 0.025);
                    let _tilt = r.uniform(0.02, 0.03);
                    let kr = r.uniform(0.003, 0.006);
                    let kl = r.uniform(0.018, 0.035);
                    let (idx, after) = shelf_material_pick(r);
                    shelf_material_draws(r);
                    let dpar = DrawerParams { board_t, board_w: sp.shelf_width, board_h: drawer_h, depth, side_h, width: dwid, knob_r: kr, knob_len: kl };
                    let mut d = drawer_gmesh(&dpar);
                    if let Some(pk) = picks.as_deref_mut() {
                        set_tag(&mut d, ((2 + pk.len()) << 1) as u8);
                        pk.push(DrawerPick { idx, after });
                    }
                    let hinge = [depth / 2.0, 0.0, div_t / 2.0 + sp.div_z[i]];
                    att.push(d.translated([f(hinge[0]), f(hinge[1]), f(hinge[2])]));
                }
                let join = GMesh::join(&att);
                attach_comps.push(Some(join.translated([0.0, f(y_trans[k]), 0.0])));
            }
            _ => attach_comps.push(None),
        }
    }
    let mut parts: Vec<GMesh> = vec![];
    let comps: Vec<GMesh> = attach_comps.into_iter().flatten().collect();
    if !comps.is_empty() {
        parts.push(GMesh::join(&comps));
    }
    for (i, fr) in frames.iter().enumerate() {
        parts.push(fr.translated([0.0, f(y_trans[i]), 0.0]));
    }
    // butil.join_objects: parts in order (first object first)
    let mut out = GMesh::new();
    for g in &parts {
        let base = out.v.len() as u32;
        out.v.extend_from_slice(&g.v);
        for (i, fc) in g.p.iter().enumerate() {
            out.p.push(fc.iter().map(|x| x + base).collect());
            out.tag.push(g.tag[i]);
        }
    }
    (out, rnd.unwrap())
}

fn clip_(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// SingleCabinetBaseFactory.create_asset(i): shelf frame + doors + hinge attachments (instance stream `r`)
pub fn single_cabinet_gmesh(r: &mut Rng, dims: [f64; 3], o: RampedOpts) -> GMesh {
    single_cabinet_core(r, dims, o, None)
}

/// door panels of a SingleCabinet (door nodegroup mid boards): lower / upper glass flags (`has_mid` false: one panel = lower)
#[derive(Clone, Copy, Default, Debug)]
pub struct SinglePanels {
    pub has_mid: bool,
    pub lower_glass: bool,
    pub upper_glass: bool,
}

/// Same with per-face material slots in `tag >> 1` when `panels` is given: 0 shelf frame, 1 division boards, 2 door frame + knob, 3 lower panel,
/// 4 upper panel (hinge plates: 0, python gives them no material = material index 0 = the first slot).
pub fn single_cabinet_core(r: &mut Rng, dims: [f64; 3], o: RampedOpts, mut panels: Option<&mut SinglePanels>) -> GMesh {
    let tagged = panels.is_some();
    let f = |x: f64| x as f32;
    let depth = dims[0] - 0.01;
    let bottom_h = 0.083;
    let num_h = ((dims[2] - 0.083) / 0.3) as usize;
    let cell_h = vec![(dims[2] - 0.083) / num_h as f64; num_h];
    // LargeShelfBaseFactory.get_asset_params
    let side_t = clip_(r.normal(0.02, 0.002), 0.015, 0.025);
    let y_gap = r.uniform(0.01, 0.05);
    let div_t = clip_(r.normal(0.02, 0.002), 0.015, 0.025);
    let sd = r.uniform(0.001, 0.004);
    let sr = r.uniform(0.001, 0.004);
    let sw = r.uniform(0.0, 0.02);
    for (lo, hi) in [(0.025, 0.06), (0.05, 0.1), (0.01, 0.025), (0.002, 0.005), (0.0, 0.05)] {
        r.uniform(lo, hi);
    }
    let mut sp = ShelfParams { shelf_depth: depth, side_t, back_t: 0.01, y_gap, bottom_h, div_t, screw_depth: sd, screw_r: sr, screw_wgap: sw, cell_w: vec![dims[1]], cell_h, ..Default::default() };
    sp.derive();
    let shelf = if tagged { shelf_gmesh_slots(&sp, 0, 2) } else { shelf_gmesh(&sp, true) };
    // door params
    let shelf_w = sp.shelf_width + side_t * 2.0;
    let (door_w, num_door) = if shelf_w < 0.55 { (shelf_w, 1) } else { (shelf_w / 2.0 - 0.0005, 2) };
    let mut door_h = sp.div_z[sp.div_z.len() - 1] - sp.div_z[0] + div_t;
    if sp.div_z.len() > 5 && r.choice_p(&[0.5, 0.5]) == 0 {
        door_h = sp.div_z[3] - sp.div_z[0] + div_t;
    }
    let e1 = r.uniform(0.01, 0.018);
    let ew = r.uniform(0.03, 0.05);
    let e2 = r.uniform(0.005, 0.008);
    let ang = r.uniform(0.6, 0.8);
    let kr = r.uniform(0.003, 0.006);
    let kl = r.uniform(0.018, 0.035);
    let gap = r.uniform(0.05, 0.15);
    let has_mid = r.choice_p(&[0.6, 0.4]) == 0;
    let (mut lower_glass, mut upper_glass) = (false, false);
    if has_mid {
        lower_glass = r.choice_p(&[0.7, 0.3]) == 1;
        // upper = choice([lower, "glass"], p=[0.6, 0.4])
        upper_glass = r.choice_p(&[0.6, 0.4]) == 1 || lower_glass;
    }
    if let Some(p) = panels.as_deref_mut() {
        *p = SinglePanels { has_mid, lower_glass, upper_glass };
    }
    let mut dp = DoorParams { height: door_h, width: door_w, e1, ew, e2, ang, knob_r: kr, knob_len: kl, has_mid, left: false };
    let dtags = if tagged { Some([4u8, 6, 8]) } else { None };
    let right = door_gmesh_t(&dp, o, dtags);
    dp.left = true;
    let left = door_gmesh_t(&dp, o, dtags);
    let attach_height = [gap, door_h - gap];
    let (hinges, attach_pos): (Vec<[f64; 3]>, Vec<[f64; 3]>) = if num_door == 1 {
        (vec![[depth / 2.0 + 0.0025, -shelf_w / 2.0, bottom_h]], attach_height.iter().map(|z| [depth / 2.0, -sp.shelf_width / 2.0, bottom_h + z]).collect())
    } else {
        let mut ap: Vec<[f64; 3]> = attach_height.iter().map(|z| [depth / 2.0, -sp.shelf_width / 2.0, bottom_h + z]).collect();
        ap.extend(attach_height.iter().map(|z| [depth / 2.0, sp.shelf_width / 2.0, bottom_h + z]));
        (vec![[depth / 2.0 + 0.008, -shelf_w / 2.0, bottom_h], [depth / 2.0 + 0.008, shelf_w / 2.0, bottom_h]], ap)
    };
    let z3 = [0f32; 3];
    let one = [1f32; 3];
    let mut doors = vec![right.translated([f(hinges[0][0]), f(hinges[0][1]), f(hinges[0][2])])];
    if hinges.len() > 1 {
        doors.push(left.translated([f(hinges[1][0]), f(hinges[1][1]), f(hinges[1][2])]));
    }
    let mut attaches = vec![];
    for pos in &attach_pos {
        let cube = gcube([0.0006, 0.02, 0.045], [2, 2, 2]);
        let t = cube.translated([0.0, -0.01, 0.0]);
        let cube_1 = gcube([0.0005, 0.034, 0.02], [2, 2, 2]);
        let j = GMesh::join(&[t, cube_1]);
        let t1 = j.translated([0.0, -0.017, 0.0]);
        let t2 = t1.transformed(z3, [0.0, 0.0, -A90], one);
        attaches.push(t2.translated([f(pos[0]), f(pos[1]), f(pos[2])]));
    }
    let ja = GMesh::join(&attaches);
    let mut all = doors;
    all.push(ja);
    let joined = GMesh::join(&all);
    // butil.join_objects([shelf, obj])
    let mut out = shelf;
    let base = out.v.len() as u32;
    out.v.extend_from_slice(&joined.v);
    for (i, fc) in joined.p.iter().enumerate() {
        out.p.push(fc.iter().map(|x| x + base).collect());
        out.tag.push(joined.tag[i]);
    }
    out
}
