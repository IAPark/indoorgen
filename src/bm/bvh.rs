//! Port of Blender 4.2 `BLI_kdopbvh` (build, overlap, ray cast) with the exact node layout / traversal order, so that
//! the order of overlap pairs (and therefore of everything built from them in bmesh_intersect) matches Blender.
//!
//! Only the pieces used by `bmesh_intersect` / `bmesh_polygon_edgenet` are ported: `new`, `insert`, `balance`,
//! `overlap_ex` (non-self) and `ray_cast_ex` with a callback.

const NONE: usize = usize::MAX;

const KDOP_AXES: [[f32; 3]; 13] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, -1.0, 0.0],
    [1.0, 0.0, -1.0],
    [0.0, 1.0, -1.0],
];
const KDOP_AXES_LENGTH: [f32; 13] = [
    1.0,
    1.0,
    1.0,
    1.732_050_8,
    1.732_050_8,
    1.732_050_8,
    1.732_050_8,
    1.414_213_5,
    1.414_213_5,
    1.414_213_5,
    1.414_213_5,
    1.414_213_5,
    1.414_213_5,
];

pub const RAYCAST_DIST_MAX: f32 = f32::MAX / 2.0;

#[derive(Clone)]
struct Node {
    children: Vec<usize>,
    parent: usize,
    bv: Vec<f32>,
    index: i32,
    node_num: usize,
    main_axis: usize,
}

pub struct BvhTree {
    arr: Vec<Node>,
    /// `tree->nodes` (leaf pointers, permuted by the build)
    nodes: Vec<usize>,
    epsilon: f32,
    pub leaf_num: usize,
    branch_num: usize,
    start_axis: usize,
    stop_axis: usize,
    axis: usize,
    tree_type: usize,
}

fn implicit_needed_branches(tree_type: usize, leafs: usize) -> usize {
    std::cmp::max(1, (leafs + tree_type - 3) / (tree_type - 1))
}

#[derive(Default)]
struct BuildHelper {
    tree_type: usize,
    leafs_num: usize,
    leafs_per_child: [i64; 32],
    branches_on_level: [i64; 32],
    remain_leafs: i64,
}

impl BuildHelper {
    fn new(tree: &BvhTree) -> BuildHelper {
        let mut d = BuildHelper { tree_type: tree.tree_type, leafs_num: tree.leaf_num, ..Default::default() };
        let tt = tree.tree_type as i64;
        d.leafs_per_child[0] = 1;
        while d.leafs_per_child[0] < d.leafs_num as i64 {
            d.leafs_per_child[0] *= tt;
        }
        d.branches_on_level[0] = 1;
        let mut depth = 1;
        while depth < 32 && d.leafs_per_child[depth - 1] != 0 {
            d.branches_on_level[depth] = d.branches_on_level[depth - 1] * tt;
            d.leafs_per_child[depth] = d.leafs_per_child[depth - 1] / tt;
            depth += 1;
        }
        let remain = d.leafs_num as i64 - d.leafs_per_child[1];
        let nnodes = (remain + tt - 2) / (tt - 1);
        d.remain_leafs = remain + nnodes;
        d
    }
    fn implicit_leafs_index(&self, depth: usize, child_index: i64) -> i64 {
        let min_leaf_index = child_index * self.leafs_per_child[depth - 1];
        if min_leaf_index <= self.remain_leafs {
            return min_leaf_index;
        }
        if self.leafs_per_child[depth] != 0 {
            return self.leafs_num as i64 - (self.branches_on_level[depth - 1] - child_index) * self.leafs_per_child[depth];
        }
        self.remain_leafs
    }
}

impl BvhTree {
    pub fn new(maxsize: usize, epsilon: f32, tree_type: usize, axis: usize) -> BvhTree {
        let epsilon = epsilon.max(f32::EPSILON);
        let (start_axis, stop_axis) = match axis {
            26 => (0, 13),
            18 => (7, 13),
            14 => (0, 7),
            8 => (0, 4),
            6 => (0, 3),
            _ => panic!("bad kdop axis"),
        };
        let numnodes = maxsize + implicit_needed_branches(tree_type, maxsize) + tree_type;
        let proto = Node { children: vec![NONE; tree_type], parent: NONE, bv: vec![0.0; axis], index: 0, node_num: 0, main_axis: 0 };
        BvhTree { arr: vec![proto; numnodes], nodes: vec![NONE; numnodes], epsilon, leaf_num: 0, branch_num: 0, start_axis, stop_axis, axis, tree_type }
    }

    pub fn insert(&mut self, index: i32, co: &[[f32; 3]]) {
        let id = self.leaf_num;
        self.nodes[id] = id;
        self.leaf_num += 1;
        let (sa, ea) = (self.start_axis, self.stop_axis);
        let node = &mut self.arr[id];
        for a in sa..ea {
            node.bv[2 * a] = f32::MAX;
            node.bv[2 * a + 1] = -f32::MAX;
        }
        for c in co {
            for a in sa..ea {
                let ax = KDOP_AXES[a];
                let v = c[0] * ax[0] + c[1] * ax[1] + c[2] * ax[2];
                if v < node.bv[2 * a] {
                    node.bv[2 * a] = v;
                }
                if v > node.bv[2 * a + 1] {
                    node.bv[2 * a + 1] = v;
                }
            }
        }
        node.index = index;
        let eps = self.epsilon;
        for a in sa..ea {
            let dist = eps * KDOP_AXES_LENGTH[a];
            node.bv[2 * a] -= dist;
            node.bv[2 * a + 1] += dist;
        }
    }

    fn refit_kdop_hull(&mut self, node: usize, start: usize, end: usize) {
        let (sa, ea) = (self.start_axis, self.stop_axis);
        let mut bv = self.arr[node].bv.clone();
        for a in sa..ea {
            bv[2 * a] = f32::MAX;
            bv[2 * a + 1] = -f32::MAX;
        }
        for j in start..end {
            let nb = &self.arr[self.nodes[j]].bv;
            for a in sa..ea {
                if nb[2 * a] < bv[2 * a] {
                    bv[2 * a] = nb[2 * a];
                }
                if nb[2 * a + 1] > bv[2 * a + 1] {
                    bv[2 * a + 1] = nb[2 * a + 1];
                }
            }
        }
        self.arr[node].bv = bv;
    }

    fn bv_val(&self, leaf_slot: usize, axis: usize) -> f32 {
        self.arr[self.nodes[leaf_slot]].bv[axis]
    }

    fn insertionsort(&mut self, lo: usize, hi: usize, axis: usize) {
        for i in lo..hi {
            let mut j = i;
            let t = self.nodes[i];
            let tv = self.arr[t].bv[axis];
            while j != lo && tv < self.bv_val(j - 1, axis) {
                self.nodes[j] = self.nodes[j - 1];
                j -= 1;
            }
            self.nodes[j] = t;
        }
    }

    fn partition(&mut self, lo: usize, hi: usize, xv: f32, axis: usize) -> usize {
        let mut i = lo;
        let mut j = hi;
        loop {
            while self.bv_val(i, axis) < xv {
                i += 1;
            }
            j -= 1;
            while xv < self.bv_val(j, axis) {
                j -= 1;
            }
            if !(i < j) {
                return i;
            }
            self.nodes.swap(i, j);
            i += 1;
        }
    }

    fn medianof3(&self, lo: usize, mid: usize, hi: usize, axis: usize) -> f32 {
        let (a_lo, a_mid, a_hi) = (self.bv_val(lo, axis), self.bv_val(mid, axis), self.bv_val(hi, axis));
        if a_mid < a_lo {
            if a_hi < a_mid {
                return a_mid;
            }
            if a_hi < a_lo {
                return a_hi;
            }
            return a_lo;
        }
        if a_hi < a_mid {
            if a_hi < a_lo {
                return a_lo;
            }
            return a_hi;
        }
        a_mid
    }

    fn partition_nth_element(&mut self, mut begin: usize, mut end: usize, n: usize, axis: usize) {
        while end - begin > 3 {
            let xv = self.medianof3(begin, (begin + end) / 2, end - 1, axis);
            let cut = self.partition(begin, end, xv, axis);
            if cut <= n {
                begin = cut;
            } else {
                end = cut;
            }
        }
        self.insertionsort(begin, end, axis);
    }

    fn get_largest_axis(bv: &[f32]) -> usize {
        let m0 = bv[1] - bv[0];
        let m1 = bv[3] - bv[2];
        let m2 = bv[5] - bv[4];
        if m0 > m1 {
            if m0 > m2 {
                return 1;
            }
            return 5;
        }
        if m1 > m2 {
            return 3;
        }
        5
    }

    pub fn balance(&mut self) {
        let leafs_num = self.leaf_num;
        let tree_type = self.tree_type;
        let tree_offset = 2 - tree_type as i64;
        let branches_num = implicit_needed_branches(tree_type, leafs_num);
        let base = leafs_num - 1; // branches_array = nodearray + (leaf_num - 1)
        let bi = |j: usize| base + j;
        // root
        {
            let root = bi(1);
            self.arr[root].parent = NONE;
            if leafs_num == 1 {
                self.refit_kdop_hull(root, 0, leafs_num);
                let bvc = self.arr[root].bv.clone();
                self.arr[root].main_axis = Self::get_largest_axis(&bvc) / 2;
                self.arr[root].node_num = 1;
                let l0 = self.nodes[0];
                self.arr[root].children[0] = l0;
                self.arr[l0].parent = root;
                self.branch_num = implicit_needed_branches(tree_type, leafs_num);
                for i in 0..self.branch_num {
                    self.nodes[leafs_num + i] = leafs_num + i;
                }
                return;
            }
        }
        let data = BuildHelper::new(self);
        let mut i: i64 = 1;
        let mut depth: usize = 1;
        while i <= branches_num as i64 {
            let first_of_next_level = i * tree_type as i64 + tree_offset;
            let i_stop = std::cmp::min(first_of_next_level, branches_num as i64 + 1);
            for j in i..i_stop {
                let parent = bi(j as usize);
                let parent_level_index = j - i;
                let parent_leafs_begin = data.implicit_leafs_index(depth, parent_level_index);
                let parent_leafs_end = data.implicit_leafs_index(depth, parent_level_index + 1);
                self.refit_kdop_hull(parent, parent_leafs_begin as usize, parent_leafs_end as usize);
                let split_axis = Self::get_largest_axis(&self.arr[parent].bv);
                self.arr[parent].main_axis = split_axis / 2;
                let mut nth = vec![0i64; tree_type + 1];
                nth[0] = parent_leafs_begin;
                nth[tree_type] = parent_leafs_end;
                for k in 1..tree_type {
                    let child_index = j * tree_type as i64 + tree_offset + k as i64;
                    let child_level_index = child_index - first_of_next_level;
                    nth[k] = data.implicit_leafs_index(depth + 1, child_level_index);
                }
                // split_leafs
                for p in 0..tree_type - 1 {
                    if nth[p] >= nth[tree_type] {
                        break;
                    }
                    self.partition_nth_element(nth[p] as usize, nth[tree_type] as usize, nth[p + 1] as usize, split_axis);
                }
                let mut k = 0;
                while k < tree_type {
                    let child_index = j * tree_type as i64 + tree_offset + k as i64;
                    let child_level_index = child_index - first_of_next_level;
                    let cb = data.implicit_leafs_index(depth + 1, child_level_index);
                    let ce = data.implicit_leafs_index(depth + 1, child_level_index + 1);
                    if ce - cb > 1 {
                        let c = bi(child_index as usize);
                        self.arr[parent].children[k] = c;
                        self.arr[c].parent = parent;
                    } else if ce - cb == 1 {
                        let c = self.nodes[cb as usize];
                        self.arr[parent].children[k] = c;
                        self.arr[c].parent = parent;
                    } else {
                        break;
                    }
                    k += 1;
                }
                self.arr[parent].node_num = k;
            }
            i = i * tree_type as i64 + tree_offset;
            depth += 1;
        }
        self.branch_num = implicit_needed_branches(tree_type, leafs_num);
        for i in 0..self.branch_num {
            self.nodes[leafs_num + i] = leafs_num + i;
        }
    }

    fn root(&self) -> usize {
        self.nodes[self.leaf_num]
    }

    fn overlap_test(&self, n1: usize, other: &BvhTree, n2: usize, sa: usize, ea: usize) -> bool {
        let b1 = &self.arr[n1].bv;
        let b2 = &other.arr[n2].bv;
        for a in sa..ea {
            if b1[2 * a] > b2[2 * a + 1] || b2[2 * a] > b1[2 * a + 1] {
                return false;
            }
        }
        true
    }

    fn overlap_traverse(&self, other: &BvhTree, n1: usize, n2: usize, sa: usize, ea: usize, out: &mut Vec<(i32, i32)>) {
        if self.overlap_test(n1, other, n2, sa, ea) {
            if self.arr[n1].node_num == 0 {
                if other.arr[n2].node_num == 0 {
                    if std::ptr::eq(self, other) && n1 == n2 {
                        return;
                    }
                    out.push((self.arr[n1].index, other.arr[n2].index));
                } else {
                    for j in 0..other.tree_type {
                        let c = other.arr[n2].children[j];
                        if c != NONE {
                            self.overlap_traverse(other, n1, c, sa, ea, out);
                        }
                    }
                }
            } else {
                for j in 0..self.tree_type {
                    let c = self.arr[n1].children[j];
                    if c != NONE {
                        self.overlap_traverse(other, c, n2, sa, ea, out);
                    }
                }
            }
        }
    }

    /// `BLI_bvhtree_overlap_ex(tree1 = self, tree2 = other, ..., flag = USE_THREADING | RETURN_PAIRS)` as built in a release
    /// build (threading only above 1024 leafs in tree1; per thread results are returned in reversed discovery order).
    pub fn overlap_ex(&self, other: &BvhTree) -> Vec<(i32, i32)> {
        let use_threading = self.leaf_num > 1024;
        let root1 = self.root();
        let root2 = other.root();
        let sa = self.start_axis.min(other.start_axis);
        let ea = self.stop_axis.min(other.stop_axis);
        if !self.overlap_test(root1, other, root2, sa, ea) {
            return vec![];
        }
        let root_node_len = std::cmp::min(self.tree_type, self.arr[root1].node_num);
        let thread_num = if use_threading { root_node_len } else { 1 };
        let mut result = vec![];
        for j in 0..thread_num {
            let mut out = vec![];
            if use_threading {
                let c = self.arr[root1].children[j];
                self.overlap_traverse(other, c, root2, sa, ea, &mut out);
            } else {
                self.overlap_traverse(other, root1, root2, sa, ea, &mut out);
            }
            out.reverse();
            result.extend(out);
        }
        result
    }

    /// `BLI_bvhtree_ray_cast_ex` (radius 0) with callback `cb(index, hit_dist) -> new hit (index, dist)`.
    /// `cb` receives (leaf index, current hit dist) and may return Some((index, dist)) to update the hit.
    pub fn ray_cast<F: FnMut(i32, f32) -> Option<(i32, f32)>>(&self, co: [f32; 3], dir: [f32; 3], hit_index: &mut i32, hit_dist: &mut f32, cb: &mut F) {
        let mut ray_dot_axis = [0f32; 3];
        let mut idot_axis = [0f32; 3];
        let mut index = [0usize; 6];
        for i in 0..3 {
            let ax = KDOP_AXES[i];
            ray_dot_axis[i] = dir[0] * ax[0] + dir[1] * ax[1] + dir[2] * ax[2];
            if ray_dot_axis[i].abs() < f32::EPSILON {
                ray_dot_axis[i] = 0.0;
                idot_axis[i] = f32::MAX;
            } else {
                idot_axis[i] = 1.0 / ray_dot_axis[i];
            }
            index[2 * i] = if idot_axis[i] < 0.0 { 1 } else { 0 };
            index[2 * i + 1] = 1 - index[2 * i];
            index[2 * i] += 2 * i;
            index[2 * i + 1] += 2 * i;
        }
        if self.leaf_num == 0 {
            return;
        }
        let root = self.root();
        self.dfs_raycast(root, &co, &ray_dot_axis, &idot_axis, &index, hit_index, hit_dist, cb);
    }

    fn fast_ray_nearest_hit(&self, node: usize, co: &[f32; 3], idot: &[f32; 3], index: &[usize; 6], hit_dist: f32) -> f32 {
        let bv = &self.arr[node].bv;
        let t1x = (bv[index[0]] - co[0]) * idot[0];
        let t2x = (bv[index[1]] - co[0]) * idot[0];
        let t1y = (bv[index[2]] - co[1]) * idot[1];
        let t2y = (bv[index[3]] - co[1]) * idot[1];
        let t1z = (bv[index[4]] - co[2]) * idot[2];
        let t2z = (bv[index[5]] - co[2]) * idot[2];
        if (t1x > t2y || t2x < t1y || t1x > t2z || t2x < t1z || t1y > t2z || t2y < t1z) || (t2x < 0.0 || t2y < 0.0 || t2z < 0.0) || (t1x > hit_dist || t1y > hit_dist || t1z > hit_dist) {
            return f32::MAX;
        }
        // max_fff
        let mut m = t1x;
        if t1y > m {
            m = t1y;
        }
        if t1z > m {
            m = t1z;
        }
        m
    }

    #[allow(clippy::too_many_arguments)]
    fn dfs_raycast<F: FnMut(i32, f32) -> Option<(i32, f32)>>(
        &self,
        node: usize,
        co: &[f32; 3],
        rda: &[f32; 3],
        idot: &[f32; 3],
        index: &[usize; 6],
        hit_index: &mut i32,
        hit_dist: &mut f32,
        cb: &mut F,
    ) {
        let dist = self.fast_ray_nearest_hit(node, co, idot, index, *hit_dist);
        if dist >= *hit_dist {
            return;
        }
        let n = &self.arr[node];
        if n.node_num == 0 {
            if let Some((i, d)) = cb(n.index, *hit_dist) {
                *hit_index = i;
                *hit_dist = d;
            }
        } else if rda[n.main_axis] > 0.0 {
            for i in 0..n.node_num {
                self.dfs_raycast(n.children[i], co, rda, idot, index, hit_index, hit_dist, cb);
            }
        } else {
            for i in (0..n.node_num).rev() {
                self.dfs_raycast(n.children[i], co, rda, idot, index, hit_index, hit_dist, cb);
            }
        }
    }
}
