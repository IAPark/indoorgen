//! Port of Blender's Dual Contouring remesher (intern/dualcon, used by the REMESH modifier in SMOOTH / SHARP / BLOCKS modes; Blender 4.2).
//! Same octree traversal order (so vertex / quad order equals Blender's), integer triangle-cube projections, float32 hermite data.
//! Differences: the hole-patching pass (`Octree::trace` / `patch`) is not ported (closed input meshes do not need it; set DUALCON_DEBUG=1
//! to be told when an open path exists), the optional flood fill is not ported, and the 3x3 SVD of the QEF uses a symmetric Jacobi eigen solver
//! (the same pseudo-inverse mathematically; agrees to ~1e-7 relative).
use crate::polyfill;

const GRID_DIMENSION: i32 = 20;
const NUM_AXES: usize = 13;
const NONE: u32 = u32::MAX;

const VERTMAP: [[i64; 3]; 8] = [[0, 0, 0], [0, 0, 1], [0, 1, 0], [0, 1, 1], [1, 0, 0], [1, 0, 1], [1, 1, 0], [1, 1, 1]];
const EDGEMAP: [[usize; 2]; 12] = [[0, 4], [1, 5], [2, 6], [3, 7], [0, 2], [1, 3], [4, 6], [5, 7], [0, 1], [2, 3], [4, 5], [6, 7]];
const CELL_PROC_FACE_MASK: [[usize; 3]; 12] =
    [[0, 4, 0], [1, 5, 0], [2, 6, 0], [3, 7, 0], [0, 2, 1], [4, 6, 1], [1, 3, 1], [5, 7, 1], [0, 1, 2], [2, 3, 2], [4, 5, 2], [6, 7, 2]];
const CELL_PROC_EDGE_MASK: [[usize; 5]; 6] = [[0, 1, 2, 3, 0], [4, 5, 6, 7, 0], [0, 4, 1, 5, 1], [2, 6, 3, 7, 1], [0, 2, 4, 6, 2], [1, 3, 5, 7, 2]];
const FACE_PROC_FACE_MASK: [[[usize; 3]; 4]; 3] = [
    [[4, 0, 0], [5, 1, 0], [6, 2, 0], [7, 3, 0]],
    [[2, 0, 1], [6, 4, 1], [3, 1, 1], [7, 5, 1]],
    [[1, 0, 2], [3, 2, 2], [5, 4, 2], [7, 6, 2]],
];
const FACE_PROC_EDGE_MASK: [[[usize; 6]; 4]; 3] = [
    [[1, 4, 0, 5, 1, 1], [1, 6, 2, 7, 3, 1], [0, 4, 6, 0, 2, 2], [0, 5, 7, 1, 3, 2]],
    [[0, 2, 3, 0, 1, 0], [0, 6, 7, 4, 5, 0], [1, 2, 0, 6, 4, 2], [1, 3, 1, 7, 5, 2]],
    [[1, 1, 0, 3, 2, 0], [1, 5, 4, 7, 6, 0], [0, 1, 5, 0, 4, 1], [0, 3, 7, 2, 6, 1]],
];
const EDGE_PROC_EDGE_MASK: [[[usize; 5]; 2]; 3] = [
    [[3, 2, 1, 0, 0], [7, 6, 5, 4, 0]],
    [[5, 1, 4, 0, 1], [7, 3, 6, 2, 1]],
    [[6, 4, 2, 0, 2], [7, 5, 3, 1, 2]],
];
const PROCESS_EDGE_MASK: [[usize; 4]; 3] = [[3, 2, 1, 0], [7, 5, 6, 4], [11, 10, 9, 8]];
const FACE_MAP: [[usize; 4]; 6] = [[4, 8, 5, 9], [6, 10, 7, 11], [0, 8, 1, 10], [2, 9, 3, 11], [0, 4, 2, 6], [1, 5, 3, 7]];

#[derive(Clone, Copy, Default)]
struct CubeProj {
    origin: i64,
    edges: [i64; 3],
    min: i64,
    max: i64,
}

#[derive(Clone, Copy)]
struct Isect {
    tri_proj: [[i64; 2]; NUM_AXES],
    norm: [f64; 3],
    cube: [CubeProj; NUM_AXES],
}

fn cross_i(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot_i(a: [i64; 3], b: [i64; 3]) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Isect {
    fn new(cube: [[i64; 3]; 2], tri: [[i64; 3]; 3]) -> Isect {
        let mut axes = [[0i64; 3]; NUM_AXES];
        axes[0] = [1, 0, 0];
        axes[1] = [0, 1, 0];
        axes[2] = [0, 0, 1];
        let mut te = [[0i64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                te[i][j] = tri[(i + 1) % 3][j] - tri[i][j];
            }
        }
        axes[3] = cross_i(te[0], te[1]);
        let mut ct = 4;
        for i in 0..3 {
            for j in 0..3 {
                axes[ct] = cross_i(axes[j], te[i]);
                ct += 1;
            }
        }
        let d1 = [(tri[1][0] - tri[0][0]) as f64, (tri[1][1] - tri[0][1]) as f64, (tri[1][2] - tri[0][2]) as f64];
        let d2 = [(tri[2][0] - tri[1][0]) as f64, (tri[2][1] - tri[1][1]) as f64, (tri[2][2] - tri[1][2]) as f64];
        let mut norm = [d1[1] * d2[2] - d1[2] * d2[1], d1[2] * d2[0] - d1[0] * d2[2], d1[0] * d2[1] - d1[1] * d2[0]];
        let mag = norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2];
        if mag > 0.0 {
            let m = mag.sqrt();
            norm[0] /= m;
            norm[1] /= m;
            norm[2] /= m;
        }
        let mut cubeedge = [[0i64; 3]; 3];
        for i in 0..3 {
            cubeedge[i][i] = cube[1][i] - cube[0][i];
        }
        let mut cp = [CubeProj::default(); NUM_AXES];
        let mut tri_proj = [[0i64; 2]; NUM_AXES];
        for axis in 0..NUM_AXES {
            let c = &mut cp[axis];
            c.origin = dot_i(axes[axis], cube[0]);
            for i in 0..3 {
                c.edges[i] = dot_i(axes[axis], cubeedge[i]);
            }
            let (mut max, mut min) = (0i64, 0i64);
            for i in 1..8 {
                let proj = VERTMAP[i][0] * c.edges[0] + VERTMAP[i][1] * c.edges[1] + VERTMAP[i][2] * c.edges[2];
                if proj > max {
                    max = proj;
                }
                if proj < min {
                    min = proj;
                }
            }
            c.min = min;
            c.max = max;
            let vts = [dot_i(axes[axis], tri[0]), dot_i(axes[axis], tri[1]), dot_i(axes[axis], tri[2])];
            tri_proj[axis] = [vts[0], vts[0]];
            for i in 1..3 {
                if vts[i] < tri_proj[axis][0] {
                    tri_proj[axis][0] = vts[i];
                }
                if vts[i] > tri_proj[axis][1] {
                    tri_proj[axis][1] = vts[i];
                }
            }
        }
        Isect { tri_proj, norm, cube: cp }
    }

    fn child(&self) -> Isect {
        let mut s = *self;
        for i in 0..NUM_AXES {
            for j in 0..3 {
                s.cube[i].edges[j] = self.cube[i].edges[j] >> 1;
            }
            s.cube[i].min = self.cube[i].min >> 1;
            s.cube[i].max = self.cube[i].max >> 1;
        }
        s
    }

    fn box_mask(&self) -> u8 {
        let mut bmask = [[0u8; 2]; 3];
        let child_len = self.cube[0].edges[0] >> 1;
        for i in 0..3 {
            let mid = self.cube[i].origin + child_len;
            if mid >= self.tri_proj[i][0] {
                bmask[i][0] = 1;
            }
            if mid < self.tri_proj[i][1] {
                bmask[i][1] = 1;
            }
        }
        let mut boxmask = 0u8;
        let mut ct = 0;
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..2 {
                    boxmask |= (bmask[0][i] & bmask[1][j] & bmask[2][k]) << ct;
                    ct += 1;
                }
            }
        }
        boxmask
    }

    fn shift(&mut self, off: [i64; 3]) {
        for i in 0..NUM_AXES {
            let e = self.cube[i].edges;
            self.cube[i].origin += off[0] * e[0] + off[1] * e[1] + off[2] * e[2];
        }
    }

    fn is_intersecting(&self) -> bool {
        for i in 0..NUM_AXES {
            let p0 = self.cube[i].origin + self.cube[i].min;
            let p1 = self.cube[i].origin + self.cube[i].max;
            if p0 > self.tri_proj[i][1] || p1 < self.tri_proj[i][0] {
                return false;
            }
        }
        true
    }

    fn is_intersecting_primary(&self, e: usize) -> bool {
        for i in 0..NUM_AXES {
            let p0 = self.cube[i].origin;
            let p1 = self.cube[i].origin + self.cube[i].edges[e];
            if p0 < p1 {
                if p0 > self.tri_proj[i][1] || p1 < self.tri_proj[i][0] {
                    return false;
                }
            } else if p1 > self.tri_proj[i][1] || p0 < self.tri_proj[i][0] {
                return false;
            }
        }
        true
    }

    fn intersection_primary(&self, e: usize) -> f32 {
        let i = 3;
        let p0 = self.cube[i].origin;
        let p1 = self.cube[i].origin + self.cube[i].edges[e];
        let p2 = self.tri_proj[i][1];
        let d = p1 - p0;
        let alpha = if d == 0 {
            0.5
        } else {
            let a = (p2 - p0) as f64 / d as f64;
            if a < 0.0 || a > 1.0 {
                0.5
            } else {
                a
            }
        };
        alpha as f32
    }
}

#[derive(Clone)]
struct Internal {
    child: [u32; 8],
    leaf_bits: u8,
}
impl Internal {
    fn new() -> Internal {
        Internal { child: [NONE; 8], leaf_bits: 0 }
    }
    fn has(&self, i: usize) -> bool {
        self.child[i] != NONE
    }
    fn is_leaf(&self, i: usize) -> bool {
        (self.leaf_bits >> i) & 1 == 1
    }
}

#[derive(Clone, Default)]
struct Leaf {
    edge_parity: u16,
    prim: u8,
    signs: u8,
    minidx: i32,
    offs: Vec<[f32; 4]>,
}

type Nd = Option<u32>;

struct Octree {
    ints: Vec<Internal>,
    leaves: Vec<Leaf>,
    root: u32,
    dimen: i64,
    mindimen: i64,
    max_depth: i32,
    origin: [f32; 3],
    range: f32,
    hermite: f32,
    mode: i32,
    verts: Vec<[f32; 3]>,
    quads: Vec<[i32; 4]>,
}

fn popcount_below(mask: u8, idx: usize) -> usize {
    (0..idx).filter(|&j| (mask >> j) & 1 == 1).count()
}

impl Octree {
    // ---------- scan conversion ----------
    fn add_triangle_top(&mut self, vt: [[f32; 3]; 3]) {
        let mut v = vt;
        for i in 0..3 {
            for j in 0..3 {
                v[i][j] = (self.dimen as f32 * (v[i][j] - self.origin[j])) / self.range;
            }
        }
        let cube = [[0i64; 3], [self.dimen; 3]];
        let mut trig = [[0i64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                trig[i][j] = v[i][j] as i64;
            }
        }
        let p = Isect::new(cube, trig);
        let root = self.root;
        self.add_triangle(root, &p, self.max_depth);
    }

    fn add_triangle(&mut self, node: u32, p: &Isect, height: i32) {
        const VERTDIFF: [[i64; 3]; 8] = [[0, 0, 0], [0, 0, 1], [0, 1, -1], [0, 0, 1], [1, -1, -1], [0, 0, 1], [0, 1, -1], [0, 0, 1]];
        let boxmask = p.box_mask();
        let mut subp = p.child();
        let mut tempdiff = [0i64; 3];
        for i in 0..8 {
            tempdiff[0] += VERTDIFF[i][0];
            tempdiff[1] += VERTDIFF[i][1];
            tempdiff[2] += VERTDIFF[i][2];
            if boxmask & (1 << i) != 0 {
                subp.shift(tempdiff);
                tempdiff = [0; 3];
                if subp.is_intersecting() {
                    if !self.ints[node as usize].has(i) {
                        if height == 1 {
                            let li = self.leaves.len() as u32;
                            self.leaves.push(Leaf::default());
                            let n = &mut self.ints[node as usize];
                            n.child[i] = li;
                            n.leaf_bits |= 1 << i;
                        } else {
                            let ii = self.ints.len() as u32;
                            self.ints.push(Internal::new());
                            self.ints[node as usize].child[i] = ii;
                        }
                    }
                    let chd = self.ints[node as usize].child[i];
                    if self.ints[node as usize].is_leaf(i) {
                        self.update_cell(chd, &subp);
                    } else {
                        self.add_triangle(chd, &subp, height - 1);
                    }
                }
            }
        }
    }

    fn update_cell(&mut self, leaf: u32, p: &Isect) {
        let mask = [0usize, 4, 8];
        let lf = &mut self.leaves[leaf as usize];
        let mut oldc = 0;
        let mut newo = [[0f32; 4]; 3];
        let mut nn = 0usize;
        for i in 0..3 {
            if (lf.edge_parity >> mask[i]) & 1 == 0 {
                if p.is_intersecting_primary(i) {
                    lf.edge_parity |= 1 << mask[i];
                    newo[nn] = [p.intersection_primary(i), p.norm[0] as f32, p.norm[1] as f32, p.norm[2] as f32];
                    nn += 1;
                }
            } else {
                newo[nn] = lf.offs[oldc];
                nn += 1;
                oldc += 1;
            }
        }
        if nn > oldc {
            lf.offs = newo[..nn].to_vec();
        }
    }

    fn prepare_primal(&mut self, node: u32) {
        let n = self.ints[node as usize].clone();
        for i in 0..8 {
            if n.has(i) {
                if n.is_leaf(i) {
                    let lf = &mut self.leaves[n.child[i] as usize];
                    let e = lf.edge_parity;
                    lf.prim = ((e & 0x1) | ((e & 0x10) >> 3) | ((e & 0x100) >> 6)) as u8;
                } else {
                    self.prepare_primal(n.child[i]);
                }
            }
        }
    }

    // ---------- traversal helpers ----------
    fn kids(&self, node: Nd, leaf: bool) -> [Nd; 8] {
        let mut c = [None; 8];
        if let (Some(n), false) = (node, leaf) {
            let nd = &self.ints[n as usize];
            for i in 0..8 {
                if nd.has(i) {
                    c[i] = Some(nd.child[i]);
                }
            }
        }
        c
    }
    fn kid_leaf(&self, node: Nd, i: usize) -> bool {
        match node {
            Some(n) => self.ints[n as usize].is_leaf(i),
            None => false,
        }
    }

    // ---------- parity pass ----------
    fn process_edge_parity(&mut self, node: [Nd; 4], dir: usize) {
        let mut con = false;
        for i in 0..4 {
            let l = &self.leaves[node[i].unwrap() as usize];
            if (l.edge_parity >> PROCESS_EDGE_MASK[dir][i]) & 1 == 1 {
                con = true;
                break;
            }
        }
        if con {
            for i in 0..4 {
                self.leaves[node[i].unwrap() as usize].edge_parity |= 1 << PROCESS_EDGE_MASK[dir][i];
            }
        }
    }

    fn edge_proc(&mut self, node: [Nd; 4], leaf: [bool; 4], dir: usize, write: bool) {
        if node.iter().any(|n| n.is_none()) {
            return;
        }
        if leaf.iter().all(|&l| l) {
            if write {
                self.process_edge_write(node, dir);
            } else {
                self.process_edge_parity(node, dir);
            }
            return;
        }
        let mut chd = [[None; 8]; 4];
        for j in 0..4 {
            chd[j] = self.kids(node[j], leaf[j]);
        }
        for i in 0..2 {
            let m = EDGE_PROC_EDGE_MASK[dir][i];
            let mut ne = [None; 4];
            let mut le = [false; 4];
            for j in 0..4 {
                if leaf[j] {
                    le[j] = leaf[j];
                    ne[j] = node[j];
                } else {
                    le[j] = self.kid_leaf(node[j], m[j]);
                    ne[j] = chd[j][m[j]];
                }
            }
            self.edge_proc(ne, le, m[4], write);
        }
    }

    fn face_proc(&mut self, node: [Nd; 2], leaf: [bool; 2], dir: usize, write: bool) {
        if node[0].is_none() || node[1].is_none() {
            return;
        }
        if leaf[0] && leaf[1] {
            return;
        }
        let mut chd = [[None; 8]; 2];
        for j in 0..2 {
            chd[j] = self.kids(node[j], leaf[j]);
        }
        for i in 0..4 {
            let m = FACE_PROC_FACE_MASK[dir][i];
            let mut nf = [None; 2];
            let mut lf = [false; 2];
            for j in 0..2 {
                if leaf[j] {
                    lf[j] = leaf[j];
                    nf[j] = node[j];
                } else {
                    lf[j] = self.kid_leaf(node[j], m[j]);
                    nf[j] = chd[j][m[j]];
                }
            }
            self.face_proc(nf, lf, m[2], write);
        }
        let orders = [[0usize, 0, 1, 1], [0, 1, 0, 1]];
        for i in 0..4 {
            let m = FACE_PROC_EDGE_MASK[dir][i];
            let c = [m[1], m[2], m[3], m[4]];
            let order = orders[m[0]];
            let mut ne = [None; 4];
            let mut le = [false; 4];
            for j in 0..4 {
                let o = order[j];
                if leaf[o] {
                    le[j] = leaf[o];
                    ne[j] = node[o];
                } else {
                    le[j] = self.kid_leaf(node[o], c[j]);
                    ne[j] = chd[o][c[j]];
                }
            }
            self.edge_proc(ne, le, m[5], write);
        }
    }

    fn cell_proc(&mut self, node: Nd, leaf: bool, write: bool) {
        if node.is_none() || leaf {
            return;
        }
        let chd = self.kids(node, leaf);
        for i in 0..8 {
            let l = self.kid_leaf(node, i);
            self.cell_proc(chd[i], l, write);
        }
        for i in 0..12 {
            let c = [CELL_PROC_FACE_MASK[i][0], CELL_PROC_FACE_MASK[i][1]];
            let lf = [self.kid_leaf(node, c[0]), self.kid_leaf(node, c[1])];
            self.face_proc([chd[c[0]], chd[c[1]]], lf, CELL_PROC_FACE_MASK[i][2], write);
        }
        for i in 0..6 {
            let m = CELL_PROC_EDGE_MASK[i];
            let mut ne = [None; 4];
            let mut le = [false; 4];
            for j in 0..4 {
                le[j] = self.kid_leaf(node, m[j]);
                ne[j] = chd[m[j]];
            }
            self.edge_proc(ne, le, m[4], write);
        }
    }

    // ---------- signs ----------
    fn build_signs(&mut self) {
        let mut table = vec![0u8; 1 << 12];
        for i in 0..256usize {
            let mut ind = 0usize;
            for j in (0..12).rev() {
                ind <<= 1;
                if ((i >> EDGEMAP[j][0]) & 1) ^ ((i >> EDGEMAP[j][1]) & 1) == 1 {
                    ind |= 1;
                }
            }
            table[ind] = i as u8;
        }
        let mut cube = [0i32; 8];
        let root = Some(self.root);
        self.build_signs_rec(&table, root, false, 1, &mut cube);
    }

    fn build_signs_rec(&mut self, table: &[u8], node: Nd, is_leaf: bool, sg: i32, rvalue: &mut [i32; 8]) {
        let node = match node {
            None => {
                *rvalue = [sg; 8];
                return;
            }
            Some(n) => n,
        };
        if !is_leaf {
            let chd = self.kids(Some(node), false);
            let leaf: Vec<bool> = (0..8).map(|i| self.kid_leaf(Some(node), i)).collect();
            rvalue[0] = sg;
            let mut oris = [0i32; 8];
            self.build_signs_rec(table, chd[0], leaf[0], sg, &mut oris);
            let mut cube = [0i32; 8];
            for i in 1..8 {
                self.build_signs_rec(table, chd[i], leaf[i], oris[i], &mut cube);
                rvalue[i] = cube[i];
            }
        } else {
            let lf = &mut self.leaves[node as usize];
            lf.signs = table[lf.edge_parity as usize];
            if ((sg as u8) ^ lf.signs) & 1 == 1 {
                lf.signs = !lf.signs;
            }
            for i in 0..8 {
                rvalue[i] = ((lf.signs >> i) & 1) as i32;
            }
        }
    }

    // ---------- leaf location ----------
    fn locate_leaf_check(&self, st: [i64; 3]) -> Option<u32> {
        let mut node = self.root;
        let mut i = GRID_DIMENSION - 1;
        while i > GRID_DIMENSION - self.max_depth - 1 {
            let index = ((((st[0] >> i) & 1) << 2) | (((st[1] >> i) & 1) << 1) | ((st[2] >> i) & 1)) as usize;
            let n = &self.ints[node as usize];
            if !n.has(index) {
                return None;
            }
            node = n.child[index];
            i -= 1;
        }
        Some(node)
    }

    fn edge_pt(&self, leaf: &Leaf, index: usize, st: [i64; 3], len: i64) -> ([f32; 3], [f32; 3]) {
        let count = popcount_below(leaf.prim, index);
        let o = leaf.offs[count];
        let mut pt = [st[0] as f32, st[1] as f32, st[2] as f32];
        pt[index] += o[0] * len as f32;
        (pt, [o[1], o[2], o[3]])
    }

    fn fill_edge_intersections(&self, lf: &Leaf, st: [i64; 3], len: i64, pts: &mut [[f32; 3]; 12], norms: &mut [[f32; 3]; 12], parity: &mut [bool; 12]) {
        *parity = [false; 12];
        let pmask = [0usize, 4, 8];
        for i in 0..3 {
            if (lf.prim >> i) & 1 == 1 {
                let (p, n) = self.edge_pt(lf, i, st, len);
                pts[pmask[i]] = p;
                norms[pmask[i]] = n;
                parity[pmask[i]] = true;
            }
        }
        let fmask = [[6usize, 10], [2, 9], [1, 5]];
        let femask = [[1usize, 2], [0, 2], [0, 1]];
        for i in 0..3 {
            let mut nst = st;
            nst[i] += len;
            if let Some(nl) = self.locate_leaf_check(nst) {
                let node = &self.leaves[nl as usize];
                for k in 0..2 {
                    if (node.prim >> femask[i][k]) & 1 == 1 {
                        let (p, n) = self.edge_pt(node, femask[i][k], nst, len);
                        pts[fmask[i][k]] = p;
                        norms[fmask[i][k]] = n;
                        parity[fmask[i][k]] = true;
                    }
                }
            }
        }
        let emask = [3usize, 7, 11];
        for i in 0..3 {
            let mut nst = [st[0] + len, st[1] + len, st[2] + len];
            nst[i] -= len;
            if let Some(nl) = self.locate_leaf_check(nst) {
                let node = &self.leaves[nl as usize];
                if (node.prim >> i) & 1 == 1 {
                    let (p, n) = self.edge_pt(node, i, nst, len);
                    pts[emask[i]] = p;
                    norms[emask[i]] = n;
                    parity[emask[i]] = true;
                }
            }
        }
    }

    fn compute_minimizer(&self, lf: &Leaf, st: [i64; 3], len: i64) -> [f32; 3] {
        let mut pts = [[0f32; 3]; 12];
        let mut norms = [[0f32; 3]; 12];
        let mut parity = [false; 12];
        self.fill_edge_intersections(lf, st, len, &mut pts, &mut norms, &mut parity);
        let center = [(st[0] + len / 2) as f32, (st[1] + len / 2) as f32, (st[2] + len / 2) as f32];
        match self.mode {
            0 => center,
            1 => {
                let mut mp = [0f32; 3];
                let mut ec = 0;
                for i in 0..12 {
                    if parity[i] {
                        for k in 0..3 {
                            mp[k] += pts[i][k];
                        }
                        ec += 1;
                    }
                }
                if ec == 0 {
                    return mp;
                }
                for k in 0..3 {
                    mp[k] /= ec as f32;
                }
                mp
            }
            _ => {
                let mut rvalue = center;
                let mut mp = [0f32; 3];
                let mut ata = [0f32; 6];
                let mut atb = [0f32; 3];
                let mut ec = 0;
                for i in 0..12 {
                    if parity[i] {
                        let n = norms[i];
                        let p = pts[i];
                        ata[0] += n[0] * n[0];
                        ata[1] += n[0] * n[1];
                        ata[2] += n[0] * n[2];
                        ata[3] += n[1] * n[1];
                        ata[4] += n[1] * n[2];
                        ata[5] += n[2] * n[2];
                        let fq: u32 = { static Q: std::sync::OnceLock<u32> = std::sync::OnceLock::new(); *Q.get_or_init(|| std::env::var("FMAQ").ok().and_then(|s| s.parse().ok()).unwrap_or(0)) };
                        let pn = if fq & 1 != 0 { p[2].mul_add(n[2], p[1].mul_add(n[1], p[0] * n[0])) } else { p[0] * n[0] + p[1] * n[1] + p[2] * n[2] };
                        if fq & 2 != 0 {
                            atb[0] = n[0].mul_add(pn, atb[0]);
                            atb[1] = n[1].mul_add(pn, atb[1]);
                            atb[2] = n[2].mul_add(pn, atb[2]);
                        } else {
                            atb[0] += n[0] * pn;
                            atb[1] += n[1] * pn;
                            atb[2] += n[2] * pn;
                        }
                        mp[0] += p[0];
                        mp[1] += p[1];
                        mp[2] += p[2];
                        ec += 1;
                    }
                }
                if ec > 0 {
                    for k in 0..3 {
                        mp[k] /= ec as f32;
                    }
                    rvalue = solve_least_squares(&ata, &atb, &mp);
                }
                let nh1 = self.hermite * len as f32;
                let nh2 = (1.0 + self.hermite) * len as f32;
                let s = [st[0] as f32, st[1] as f32, st[2] as f32];
                if rvalue[0] < s[0] - nh1 || rvalue[1] < s[1] - nh1 || rvalue[2] < s[2] - nh1 || rvalue[0] > s[0] + nh2 || rvalue[1] > s[1] + nh2 || rvalue[2] > s[2] + nh2 {
                    rvalue = mp;
                }
                rvalue
            }
        }
    }

    fn generate_minimizer(&mut self, node: u32, st: [i64; 3], len: i64, height: i32, offset: &mut i32) {
        if height == 0 {
            let lf = &self.leaves[node as usize];
            let mut r = self.compute_minimizer(lf, st, len);
            let smask = lf.signs;
            let dimen = self.dimen as f32;
            for j in 0..3 {
                r[j] = r[j] * self.range / dimen + self.origin[j];
            }
            let mult = if smask > 0 && smask < 255 { 1 } else { 0 };
            for _ in 0..mult {
                self.verts.push(r);
            }
            self.leaves[node as usize].minidx = *offset;
            *offset += mult;
        } else {
            let len = len >> 1;
            let n = self.ints[node as usize].clone();
            for i in 0..8 {
                if n.has(i) {
                    let nst = [st[0] + VERTMAP[i][0] * len, st[1] + VERTMAP[i][1] * len, st[2] + VERTMAP[i][2] * len];
                    self.generate_minimizer(n.child[i], nst, len, height - 1, offset);
                }
            }
        }
    }

    fn process_edge_write(&mut self, node: [Nd; 4], dir: usize) {
        let l3 = &self.leaves[node[3].unwrap() as usize];
        let edgeind = PROCESS_EDGE_MASK[dir][3];
        if (l3.edge_parity >> edgeind) & 1 == 1 {
            let flip = (l3.signs >> EDGEMAP[edgeind][1]) & 1 > 0;
            let mi = |k: usize| self.leaves[node[k].unwrap() as usize].minidx;
            let ind = if flip { [mi(2), mi(3), mi(1), mi(0)] } else { [mi(0), mi(1), mi(3), mi(2)] };
            self.quads.push(ind);
        }
    }

    fn debug_open_paths(&self) -> usize {
        let mut n = 0;
        for l in &self.leaves {
            for f in 0..6 {
                let a: u32 = FACE_MAP[f].iter().map(|&e| ((l.edge_parity >> e) & 1) as u32).sum();
                if a & 1 == 1 {
                    n += 1;
                }
            }
        }
        n
    }
}

type M3 = [[f32; 3]; 3];

/// Eigen 3.4 JacobiSVD<Matrix3f> (no preconditioner for square matrices, full U and V), float32 throughout
fn jacobi_svd3(a: &M3) -> (M3, [f32; 3], M3) {
    let eps = f32::EPSILON;
    let precision = 2.0 * eps;
    let min_pos = f32::MIN_POSITIVE;
    let mut scale = 0.0f32;
    for r in a {
        for &x in r {
            scale = scale.max(x.abs());
        }
    }
    if scale == 0.0 {
        scale = 1.0;
    }
    let mut w: M3 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            w[i][j] = a[i][j] / scale;
        }
    }
    let mut u: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut v = u;
    let mut max_diag = w[0][0].abs().max(w[1][1].abs()).max(w[2][2].abs());
    // rotation (c, s)
    let apply_left = |m: &mut M3, p: usize, q: usize, c: f32, s: f32| {
        for i in 0..3 {
            let (xi, yi) = (m[p][i], m[q][i]);
            m[p][i] = c * xi + s * yi;
            m[q][i] = -s * xi + c * yi;
        }
    };
    // applyOnTheRight(p, q, j) == columns rotated with j.transpose(): x = c xi - s yi, y = s xi + c yi
    let apply_right = |m: &mut M3, p: usize, q: usize, c: f32, s: f32| {
        for i in 0..3 {
            let (xi, yi) = (m[i][p], m[i][q]);
            m[i][p] = c * xi - s * yi;
            m[i][q] = s * xi + c * yi;
        }
    };
    let mut finished = false;
    let mut guard = 0;
    while !finished && guard < 100 {
        guard += 1;
        finished = true;
        for p in 1..3 {
            for q in 0..p {
                let threshold = min_pos.max(precision * max_diag);
                if w[p][q].abs() > threshold || w[q][p].abs() > threshold {
                    finished = false;
                    // real_2x2_jacobi_svd
                    let mut m2 = [[w[p][p], w[p][q]], [w[q][p], w[q][q]]];
                    let t = m2[0][0] + m2[1][1];
                    let d = m2[1][0] - m2[0][1];
                    let (rs, rc) = if d.abs() < min_pos {
                        (0.0f32, 1.0f32)
                    } else {
                        let uu = t / d;
                        let tmp = (1.0 + uu * uu).sqrt();
                        (1.0 / tmp, uu / tmp)
                    };
                    // m.applyOnTheLeft(0, 1, rot1)
                    for i in 0..2 {
                        let (xi, yi) = (m2[0][i], m2[1][i]);
                        m2[0][i] = rc * xi + rs * yi;
                        m2[1][i] = -rs * xi + rc * yi;
                    }
                    // j_right.makeJacobi(m, 0, 1)
                    let (x, y, z) = (m2[0][0], m2[0][1], m2[1][1]);
                    let deno = 2.0 * y.abs();
                    let (jrc, jrs) = if deno < min_pos {
                        (1.0f32, 0.0f32)
                    } else {
                        let tau = (x - z) / deno;
                        let ww = (tau * tau + 1.0).sqrt();
                        let t = if tau > 0.0 { 1.0 / (tau + ww) } else { 1.0 / (tau - ww) };
                        let sign_t = if t > 0.0 { 1.0f32 } else { -1.0 };
                        let n = 1.0 / (t * t + 1.0).sqrt();
                        (n, -sign_t * (y / y.abs()) * t.abs() * n)
                    };
                    // j_left = rot1 * j_right.transpose()   (transpose: s -> -s)
                    let (ots_c, ots_s) = (jrc, -jrs);
                    let jlc = rc * ots_c - rs * ots_s;
                    let jls = rc * ots_s + rs * ots_c;
                    apply_left(&mut w, p, q, jlc, jls);
                    // U.applyOnTheRight(p, q, j_left.transpose())  == apply_right with the transposed rotation of the transpose = j_left itself
                    apply_right(&mut u, p, q, jlc, -jls);
                    apply_right(&mut w, p, q, jrc, jrs);
                    apply_right(&mut v, p, q, jrc, jrs);
                    max_diag = max_diag.max(w[p][p].abs().max(w[q][q].abs()));
                }
            }
        }
    }
    let mut sv = [0f32; 3];
    for i in 0..3 {
        let a = w[i][i].abs();
        sv[i] = a;
        if a != 0.0 {
            let f = w[i][i] / a;
            for r in 0..3 {
                u[r][i] *= f;
            }
        }
    }
    for i in 0..3 {
        let mut pos = i;
        let mut mx = sv[i];
        for k in i + 1..3 {
            if sv[k] > mx {
                mx = sv[k];
                pos = k;
            }
        }
        if mx == 0.0 {
            break;
        }
        if pos != i {
            sv.swap(i, pos);
            for r in 0..3 {
                let t = u[r][i];
                u[r][i] = u[r][pos];
                u[r][pos] = t;
                let t = v[r][i];
                v[r][i] = v[r][pos];
                v[r][pos] = t;
            }
        }
    }
    for x in sv.iter_mut() {
        *x *= scale;
    }
    (u, sv, v)
}

/// result = pinv(A, tol 0.1) * (b - A mp) + mp
fn solve_least_squares(half_a: &[f32; 6], b: &[f32; 3], mp: &[f32; 3]) -> [f32; 3] {
    let a: M3 = [[half_a[0], half_a[1], half_a[2]], [half_a[1], half_a[3], half_a[4]], [half_a[2], half_a[4], half_a[5]]];
    let fs: u32 = { static S: std::sync::OnceLock<u32> = std::sync::OnceLock::new(); *S.get_or_init(|| std::env::var("FMAS").ok().and_then(|s| s.parse().ok()).unwrap_or(7)) };
    let (u, sv, v) = jacobi_svd3(&a);
    // V * diag(select(|s| > 0.1, 1/s, 0)) * U^T
    let d: [f32; 3] = [0, 1, 2].map(|i| if sv[i].abs() > 0.1 { 1.0 / sv[i] } else { 0.0 });
    let mut vd = v;
    for r in 0..3 {
        for c in 0..3 {
            vd[r][c] = v[r][c] * d[c];
        }
    }
    let mut pinv: M3 = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            // (VD) * U^T : sum_k VD[r][k] * U[c][k]
            pinv[r][c] = if fs & 1 != 0 { vd[r][2].mul_add(u[c][2], vd[r][1].mul_add(u[c][1], vd[r][0] * u[c][0])) } else { vd[r][0] * u[c][0] + vd[r][1] * u[c][1] + vd[r][2] * u[c][2] };
        }
    }
    // b2 = b + A * (-mp)
    let mut b2 = [0f32; 3];
    for i in 0..3 {
        let amp = if fs & 2 != 0 { a[i][2].mul_add(-mp[2], a[i][1].mul_add(-mp[1], a[i][0] * -mp[0])) } else { a[i][0] * -mp[0] + a[i][1] * -mp[1] + a[i][2] * -mp[2] };
        b2[i] = b[i] + amp;
    }
    let mut r = [0f32; 3];
    for i in 0..3 {
        r[i] = if fs & 4 != 0 { pinv[i][2].mul_add(b2[2], pinv[i][1].mul_add(b2[1], pinv[i][0] * b2[0])) + mp[i] } else { pinv[i][0] * b2[0] + pinv[i][1] * b2[1] + pinv[i][2] * b2[2] + mp[i] };
    }
    r
}

/// Blender mesh corner tessellation (mesh_tessellate.cc): tri / quad (shortest-diagonal rule via is_quad_flip_v3_first_third_fast) / ngon (polyfill).
pub fn corner_tris(pos: &[[f32; 3]], faces: &[Vec<u32>]) -> Vec<[u32; 3]> {
    let mut out = vec![];
    for f in faces {
        match f.len() {
            3 => out.push([f[0], f[1], f[2]]),
            4 => {
                let mut a = [f[0], f[1], f[2]];
                let mut b = [f[0], f[2], f[3]];
                let sub = |p: [f32; 3], q: [f32; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                let cr = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
                let (v1, v2, v3, v4) = (pos[a[0] as usize], pos[a[1] as usize], pos[a[2] as usize], pos[b[2] as usize]);
                let d12 = sub(v2, v1);
                let d13 = sub(v3, v1);
                let d14 = sub(v4, v1);
                let ca = cr(d12, d13);
                let cb = cr(d14, d13);
                if ca[0] * cb[0] + ca[1] * cb[1] + ca[2] * cb[2] > 0.0 {
                    a[2] = b[2];
                    b[0] = a[1];
                }
                out.push(a);
                out.push(b);
            }
            n if n >= 5 => {
                let tris = polyfill::triangulate_ngon_plain(&f.iter().map(|&i| pos[i as usize]).collect::<Vec<_>>());
                for t in tris {
                    out.push([f[t[0] as usize], f[t[1] as usize], f[t[2] as usize]]);
                }
            }
            _ => {}
        }
    }
    out
}

/// REMESH modifier, dualcon modes. mode: 0 BLOCKS (centroid), 1 SMOOTH (mass point), 2 SHARP. Returns (vertex positions, quads).
pub fn dualcon(pos: &[[f32; 3]], faces: &[Vec<u32>], mode: i32, depth: i32, scale: f32, hermite: f32) -> (Vec<[f32; 3]>, Vec<[i32; 4]>) {
    let tris = corner_tris(pos, faces);
    // bounds
    let (mut mn, mut mx) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in pos {
        for k in 0..3 {
            mn[k] = mn[k].min(p[k]);
            mx[k] = mx[k].max(p[k]);
        }
    }
    // DualConInputReader::reset
    let mut maxsize = 0f32;
    for i in 0..3 {
        let d = mx[i] - mn[i];
        if d > maxsize {
            maxsize = d;
        }
    }
    for i in 0..3 {
        mn[i] = (mx[i] + mn[i]) / 2.0 - maxsize / 2.0;
        mx[i] = (mx[i] + mn[i]) / 2.0 + maxsize / 2.0;
    }
    for i in 0..3 {
        mn[i] -= maxsize * (1.0 / scale - 1.0) / 2.0;
    }
    maxsize *= 1.0 / scale;
    let dimen = 1i64 << GRID_DIMENSION;
    let mut o = Octree {
        ints: vec![Internal::new()],
        leaves: vec![],
        root: 0,
        dimen,
        mindimen: dimen >> depth,
        max_depth: depth,
        origin: mn,
        range: maxsize,
        hermite,
        mode,
        verts: vec![],
        quads: vec![],
    };
    for t in &tris {
        let vt = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        if vt.iter().any(|p| p[0].is_nan() || p[1].is_nan() || p[2].is_nan()) {
            continue;
        }
        o.add_triangle_top(vt);
    }
    // resetMinimalEdges
    o.cell_proc(Some(o.root), false, false);
    o.prepare_primal(o.root);
    if std::env::var("DUALCON_DEBUG").is_ok() {
        eprintln!("dualcon: {} tris, {} leaves, {} internal, odd faces {}", tris.len(), o.leaves.len(), o.ints.len(), o.debug_open_paths());
    }
    o.build_signs();
    let mut offset = 0;
    let root = o.root;
    o.generate_minimizer(root, [0, 0, 0], dimen, depth, &mut offset);
    o.cell_proc(Some(root), false, true);
    let _ = o.mindimen;
    (o.verts, o.quads)
}
