//! Hole patching of Blender's dualcon (Octree::trace / patch / patchSplit / connectFace / locateCell / patchAdjacent, intern/dualcon/intern/octree.cpp):
//! rings of cell faces with odd edge parity are found bottom-up (findPaths / combinePaths), split along the octree planes and closed by walking a
//! Bresenham line over the face and flipping the edge parities of the cells on both sides (new leaves get an intersection at `alpha` with a zero normal).
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Pe {
    pos: [i64; 3],
    next: usize,
}
const NIL: usize = usize::MAX;

/// PathList node: a chain of elements head..tail (linked through Pe::next)
#[derive(Clone, Copy)]
pub(super) struct Pl {
    head: usize,
    tail: usize,
    length: i64,
}

impl Octree {
    fn pe_new(&mut self, pos: [i64; 3], next: usize) -> usize {
        self.pes.push(Pe { pos, next });
        self.pes.len() - 1
    }
    fn pe_eq(&self, a: usize, b: usize) -> bool {
        self.pes[a].pos == self.pes[b].pos
    }

    // ---------------------------------------------------------------- trace
    pub(super) fn trace_all(&mut self) {
        self.pes.clear();
        let root = self.root;
        let mut paths = vec![];
        self.trace_node(root, [0, 0, 0], self.dimen, self.max_depth, &mut paths);
    }

    fn fill_children(&self, node: u32) -> ([Nd; 8], [bool; 8]) {
        let kids = self.kids(Some(node), false);
        let mut leaf = [false; 8];
        for i in 0..8 {
            leaf[i] = self.ints[node as usize].has(i) && self.ints[node as usize].is_leaf(i);
        }
        (kids, leaf)
    }

    fn trace_node(&mut self, node: u32, st: [i64; 3], len: i64, depth: i32, paths: &mut Vec<Pl>) {
        let len = len >> 1;
        let mut chdpaths: Vec<Vec<Pl>> = vec![vec![]; 8];
        let (chd, _chdleaf) = self.fill_children(node);
        let mut nst = [[0i64; 3]; 8];
        for i in 0..8 {
            for j in 0..3 {
                nst[i][j] = st[j] + len * VERTMAP[i][j];
            }
            if chd[i].is_none() || self.ints[node as usize].is_leaf(i) {
                chdpaths[i] = vec![];
            } else {
                let mut p = vec![];
                self.trace_node(chd[i].unwrap(), nst[i], len, depth - 1, &mut p);
                chdpaths[i] = p;
            }
        }
        // connectors on the faces
        let (chd, chdleaf) = self.fill_children(node);
        let mut conn: Vec<Vec<Pl>> = vec![vec![]; 12];
        for i in 0..12 {
            let c = [CELL_PROC_FACE_MASK[i][0], CELL_PROC_FACE_MASK[i][1]];
            let nf = [chd[c[0]], chd[c[1]]];
            let lf = [chdleaf[c[0]], chdleaf[c[1]]];
            let df = [depth - 1, depth - 1];
            let nstf = [nst[c[0]], nst[c[1]]];
            let mut cp = vec![];
            self.find_paths(nf, lf, df, nstf, depth - 1, CELL_PROC_FACE_MASK[i][2], &mut cp);
            conn[i] = cp;
        }
        // connect paths
        let mut rings: Vec<Pl> = vec![];
        let mut take = |v: &mut Vec<Vec<Pl>>, i: usize| std::mem::take(&mut v[i]);
        let (c1, c2) = (take(&mut chdpaths, 0), take(&mut chdpaths, 1));
        let mut l0 = c1;
        self.combine_paths(&mut l0, c2, std::mem::take(&mut conn[8]), &mut rings);
        let mut l2 = take(&mut chdpaths, 2);
        let c3 = take(&mut chdpaths, 3);
        self.combine_paths(&mut l2, c3, std::mem::take(&mut conn[9]), &mut rings);
        let mut l4 = take(&mut chdpaths, 4);
        let c5 = take(&mut chdpaths, 5);
        self.combine_paths(&mut l4, c5, std::mem::take(&mut conn[10]), &mut rings);
        let mut l6 = take(&mut chdpaths, 6);
        let c7 = take(&mut chdpaths, 7);
        self.combine_paths(&mut l6, c7, std::mem::take(&mut conn[11]), &mut rings);

        self.combine_paths(&mut l0, l2, std::mem::take(&mut conn[4]), &mut rings);
        self.combine_paths(&mut l4, l6, std::mem::take(&mut conn[5]), &mut rings);
        self.combine_paths(&mut l0, vec![], std::mem::take(&mut conn[6]), &mut rings);
        self.combine_paths(&mut l4, vec![], std::mem::take(&mut conn[7]), &mut rings);

        self.combine_paths(&mut l0, l4, std::mem::take(&mut conn[0]), &mut rings);
        self.combine_paths(&mut l0, vec![], std::mem::take(&mut conn[1]), &mut rings);
        self.combine_paths(&mut l0, vec![], std::mem::take(&mut conn[2]), &mut rings);
        self.combine_paths(&mut l0, vec![], std::mem::take(&mut conn[3]), &mut rings);

        if !rings.is_empty() {
            self.patch(node, st, len << 1, rings);
        }
        *paths = l0;
    }

    fn face_parity(&self, leaf: u32, index: usize) -> bool {
        let e = self.leaves[leaf as usize].edge_parity;
        let a: u32 = FACE_MAP[index].iter().map(|&k| ((e >> k) & 1) as u32).sum();
        a & 1 == 1
    }

    fn find_paths(&mut self, node: [Nd; 2], leaf: [bool; 2], depth: [i32; 2], st: [[i64; 3]; 2], maxdep: i32, dir: usize, paths: &mut Vec<Pl>) {
        if node[0].is_none() || node[1].is_none() {
            return;
        }
        if !(leaf[0] && leaf[1]) {
            let mut chd = [[None; 8]; 2];
            let mut chdleaf = [[false; 8]; 2];
            let mut nst = [[[0i64; 3]; 8]; 2];
            for j in 0..2 {
                if !leaf[j] {
                    let (c, l) = self.fill_children(node[j].unwrap());
                    chd[j] = c;
                    chdleaf[j] = l;
                    let len = self.dimen >> (self.max_depth - depth[j] + 1);
                    for i in 0..8 {
                        for k in 0..3 {
                            nst[j][i][k] = st[j][k] + len * VERTMAP[i][k];
                        }
                    }
                }
            }
            for i in 0..4 {
                let c = [FACE_PROC_FACE_MASK[dir][i][0], FACE_PROC_FACE_MASK[dir][i][1]];
                let mut nf = [None; 2];
                let mut df = [0i32; 2];
                let mut lf = [false; 2];
                let mut nstf = [[0i64; 3]; 2];
                for j in 0..2 {
                    if leaf[j] {
                        lf[j] = leaf[j];
                        nf[j] = node[j];
                        df[j] = depth[j];
                        nstf[j] = st[j];
                    } else {
                        lf[j] = chdleaf[j][c[j]];
                        nf[j] = chd[j][c[j]];
                        df[j] = depth[j] - 1;
                        nstf[j] = nst[j][c[j]];
                    }
                }
                self.find_paths(nf, lf, df, nstf, maxdep - 1, FACE_PROC_FACE_MASK[dir][i][2], paths);
            }
        } else {
            let ind = if depth[0] == maxdep { 0 } else { 1 };
            let fcind = 2 * dir + (1 - ind);
            if self.face_parity(node[ind].unwrap(), fcind) {
                let e2 = self.pe_new(st[1], NIL);
                let e1 = self.pe_new(st[0], e2);
                paths.insert(0, Pl { head: e1, tail: e2, length: 2 });
            }
        }
    }

    fn reverse_chain(&mut self, l: &mut Pl) {
        let mut prev = l.head;
        let mut next = self.pes[prev].next;
        self.pes[prev].next = NIL;
        while next != NIL {
            let tnext = self.pes[next].next;
            self.pes[next].next = prev;
            prev = next;
            next = tnext;
        }
        l.tail = l.head;
        l.head = prev;
    }

    /// combineSinglePath(list, idx, singlist): Some(new list) when hooked up (list[idx] removed, `sing` consumed)
    fn combine_single(&mut self, list: &mut Vec<Pl>, idx: usize, sing: &mut Pl) -> Option<Pl> {
        let mut l1 = list[idx];
        let mut l2 = *sing;
        if self.pe_eq(l1.head, l2.head) || self.pe_eq(l1.tail, l2.tail) {
            if l1.length < l2.length {
                self.reverse_chain(&mut l1);
                list[idx] = l1;
            } else {
                self.reverse_chain(&mut l2);
                *sing = l2;
            }
        }
        if self.pe_eq(l1.head, l2.tail) {
            let temp = self.pes[l1.head].next;
            self.pes[l2.tail].next = temp;
            let n = Pl { head: l2.head, tail: l1.tail, length: l1.length + l2.length - 1 };
            list.remove(idx);
            Some(n)
        } else if self.pe_eq(l1.tail, l2.head) {
            let temp = self.pes[l2.head].next;
            self.pes[l1.tail].next = temp;
            let n = Pl { head: l1.head, tail: l2.tail, length: l1.length + l2.length - 1 };
            list.remove(idx);
            Some(n)
        } else {
            None
        }
    }

    fn combine_paths(&mut self, list1: &mut Vec<Pl>, list2: Vec<Pl>, paths: Vec<Pl>, rings: &mut Vec<Pl>) {
        let mut list2 = list2;
        let mut nlist: Vec<Pl> = vec![];
        for p in paths {
            let mut sing = p;
            for which in 0..3 {
                let mut idx = 0;
                loop {
                    let len = match which {
                        0 => list1.len(),
                        1 => list2.len(),
                        _ => nlist.len(),
                    };
                    if idx >= len {
                        break;
                    }
                    let r = match which {
                        0 => self.combine_single(list1, idx, &mut sing),
                        1 => self.combine_single(&mut list2, idx, &mut sing),
                        _ => self.combine_single(&mut nlist, idx, &mut sing),
                    };
                    match r {
                        Some(t) => sing = t,
                        None => idx += 1,
                    }
                }
            }
            if self.pe_eq(sing.head, sing.tail) {
                let temp = sing.head;
                sing.head = self.pes[temp].next;
                sing.length -= 1;
                self.pes[sing.tail].next = sing.head;
                rings.insert(0, sing);
            } else {
                nlist.insert(0, sing);
            }
        }
        list1.extend(list2);
        list1.extend(nlist);
    }

    // ---------------------------------------------------------------- patch
    fn patch(&mut self, node: u32, st: [i64; 3], len: i64, rings: Vec<Pl>) {
        if len == self.mindimen {
            return;
        }
        let (x0, x1) = self.patch_split(node, st, len, rings, 0);
        let (y0, y1) = self.patch_split(node, st, len, x0, 1);
        let (y2, y3) = self.patch_split(node, st, len, x1, 1);
        let (z0, z1) = self.patch_split(node, st, len, y0, 2);
        let (z2, z3) = self.patch_split(node, st, len, y1, 2);
        let (z4, z5) = self.patch_split(node, st, len, y2, 2);
        let (z6, z7) = self.patch_split(node, st, len, y3, 2);
        let zl = [z0, z1, z2, z3, z4, z5, z6, z7];
        let len = len >> 1;
        let mut count = 0usize;
        for i in 0..8 {
            if !zl[i].is_empty() {
                let nori = [st[0] + len * VERTMAP[i][0], st[1] + len * VERTMAP[i][1], st[2] + len * VERTMAP[i][2]];
                let child = self.ints[node as usize].child[i];
                self.patch(child, nori, len, zl[i].clone());
            }
            if self.ints[node as usize].has(i) {
                count += 1;
            }
        }
        let _ = count;
    }

    fn patch_split(&mut self, node: u32, st: [i64; 3], len: i64, rings: Vec<Pl>, dir: usize) -> (Vec<Pl>, Vec<Pl>) {
        let mut n1: Vec<Pl> = vec![];
        let mut n2: Vec<Pl> = vec![];
        for r in rings {
            self.patch_split_single(node, st, len, r.head, dir, &mut n1, &mut n2);
        }
        (n1, n2)
    }

    fn get_side(&self, e: usize, pos: i64, dir: usize) -> i32 {
        if self.pes[e].pos[dir] < pos {
            -1
        } else {
            1
        }
    }

    fn find_pair(&self, head: usize, pos: i64, dir: usize) -> (i32, usize, usize) {
        let mut side = self.get_side(head, pos, dir);
        let anchor = head;
        let mut ppre1 = head;
        let mut cur = self.pes[head].next;
        while cur != anchor && self.get_side(cur, pos, dir) == side {
            ppre1 = cur;
            cur = self.pes[cur].next;
        }
        if cur == anchor {
            return (side, NIL, NIL);
        }
        side = self.get_side(cur, pos, dir);
        let mut ppre2 = cur;
        cur = self.pes[cur].next;
        while self.get_side(cur, pos, dir) == side {
            ppre2 = cur;
            cur = self.pes[cur].next;
        }
        if side == -1 {
            std::mem::swap(&mut ppre1, &mut ppre2);
        }
        (0, ppre1, ppre2)
    }

    fn compress_ring(&mut self, ring: usize) -> usize {
        if ring == NIL {
            return NIL;
        }
        let mut cur = self.pes[self.pes[ring].next].next;
        let mut pre = self.pes[ring].next;
        let mut prepre = ring;
        let mut anchor = prepre;
        loop {
            while self.pe_eq(cur, prepre) {
                if cur == prepre {
                    anchor = NIL;
                    break;
                } else {
                    self.pes[prepre].next = self.pes[cur].next;
                    pre = self.pes[prepre].next;
                    cur = self.pes[pre].next;
                    anchor = prepre;
                }
            }
            if anchor == NIL {
                break;
            }
            prepre = pre;
            pre = cur;
            cur = self.pes[cur].next;
            if prepre == anchor {
                break;
            }
        }
        anchor
    }

    fn patch_split_single(&mut self, node: u32, st: [i64; 3], len: i64, head: usize, dir: usize, n1: &mut Vec<Pl>, n2: &mut Vec<Pl>) {
        if head == NIL {
            return;
        }
        let (side, mut pre1, mut pre2) = self.find_pair(head, st[dir] + len / 2, dir);
        if side != 0 {
            let nring = Pl { head, tail: NIL, length: 0 };
            if side == -1 {
                n1.insert(0, nring);
            } else {
                n2.insert(0, nring);
            }
        } else {
            let nxt1 = self.pes[pre1].next;
            let nxt2 = self.pes[pre2].next;
            self.pes[pre1].next = nxt2;
            self.pes[pre2].next = nxt1;
            self.connect_face(node, st, len, dir, pre1, pre2);
            if self.pe_eq(pre1, self.pes[pre1].next) {
                if pre1 == self.pes[pre1].next {
                    pre1 = NIL;
                } else {
                    let temp = self.pes[pre1].next;
                    self.pes[pre1].next = self.pes[temp].next;
                }
            }
            if pre2 != NIL && self.pe_eq(pre2, self.pes[pre2].next) {
                if pre2 == self.pes[pre2].next {
                    pre2 = NIL;
                } else {
                    let temp = self.pes[pre2].next;
                    self.pes[pre2].next = self.pes[temp].next;
                }
            }
            pre1 = self.compress_ring(pre1);
            pre2 = self.compress_ring(pre2);
            self.patch_split_single(node, st, len, pre1, dir, n1, n2);
            self.patch_split_single(node, st, len, pre2, dir, n1, n2);
        }
    }

    // ---------------------------------------------------------------- geometry of the patch
    fn leaf_at(&self, pos: [i64; 3]) -> Option<u32> {
        self.locate_leaf_check(pos)
    }

    fn stored_edge(&self, leaf: u32, index: usize) -> bool {
        (self.leaves[leaf as usize].prim >> index) & 1 == 1
    }
    fn edge_count(&self, leaf: u32, index: usize) -> usize {
        let m = self.leaves[leaf as usize].prim;
        (0..index).filter(|&j| (m >> j) & 1 == 1).count()
    }

    /// getEdgeIntersectionByIndex(st, index, pt, check=1)
    fn edge_isect_by_index(&self, st: [i64; 3], index: usize) -> Option<[f32; 3]> {
        let leaf = self.locate_leaf_check(st)?;
        if self.stored_edge(leaf, index) {
            let off = self.leaves[leaf as usize].offs[self.edge_count(leaf, index)][0];
            let mut pt = [st[0] as f32, st[1] as f32, st[2] as f32];
            pt[index] += off * self.mindimen as f32;
            Some(pt)
        } else {
            None
        }
    }

    fn get_face_point(&self, pos: [i64; 3], dir: usize) -> (i64, i64, f32, f32) {
        let mut avg = [0f32; 3];
        let mut num = 0i32;
        for i in 0..4 {
            let edgeind = FACE_MAP[dir * 2][i];
            let mut nst = [0i64; 3];
            for j in 0..3 {
                nst[j] = pos[j] + self.mindimen * VERTMAP[EDGEMAP[edgeind][0]][j];
            }
            if let Some(off) = self.edge_isect_by_index(nst, edgeind / 4) {
                avg[0] += off[0];
                avg[1] += off[1];
                avg[2] += off[2];
                num += 1;
            }
        }
        if num == 0 {
            avg = [pos[0] as f32, pos[1] as f32, pos[2] as f32];
        } else {
            avg[0] /= num as f32;
            avg[1] /= num as f32;
            avg[2] /= num as f32;
        }
        let xdir = (dir + 1) % 3;
        let ydir = (dir + 2) % 3;
        let xf = avg[xdir];
        let yf = avg[ydir];
        let minshift = GRID_DIMENSION - self.max_depth;
        let x = pos[xdir] >> minshift;
        let y = pos[ydir] >> minshift;
        let p = (xf - pos[xdir] as f32) / self.mindimen as f32;
        let q = (yf - pos[ydir] as f32) / self.mindimen as f32;
        (x, y, p, q)
    }

    /// flipEdge(leaf, index, alpha)
    fn flip_edge_alpha(&mut self, leaf: u32, index: usize, alpha: f32) {
        self.leaves[leaf as usize].edge_parity ^= 1 << index;
        if index & 3 == 0 {
            let ind = index / 4;
            let parity = (self.leaves[leaf as usize].edge_parity >> index) & 1 == 1;
            if parity && !self.stored_edge(leaf, ind) {
                self.leaves[leaf as usize].prim |= 1 << ind;
                let count = self.edge_count(leaf, ind);
                self.leaves[leaf as usize].offs.insert(count, [alpha, 0.0, 0.0, 0.0]);
            }
        }
    }

    fn patch_adjacent(&mut self, leaf1: u32, leaf2: u32, walkdir: i64, inc: i64, dir: usize, side: i64, alpha: f32) -> u32 {
        let wd = walkdir as usize;
        let edgedir = (dir + (3 - wd)) % 3;
        let incdir = (dir + wd) % 3;
        let ind1 = if edgedir == 1 { ((dir + 3 - edgedir) % 3) as i64 - 1 } else { 2 - ((dir + 3 - edgedir) % 3) as i64 };
        let ind2 = if edgedir == 1 { ((incdir + 3 - edgedir) % 3) as i64 - 1 } else { 2 - ((incdir + 3 - edgedir) % 3) as i64 };
        let eind1 = (edgedir << 2) | ((side << ind1) as usize) | ((if inc > 0 { 1usize } else { 0 }) << ind2);
        let eind2 = (edgedir << 2) | ((side << ind1) as usize) | ((if inc > 0 { 0usize } else { 1 }) << ind2);
        self.flip_edge_alpha(leaf1, eind1, alpha);
        self.flip_edge_alpha(leaf2, eind2, alpha);
        leaf2
    }

    /// locateCell: descend from `node` (cell at `st`, edge `len`) towards the cell containing the marcher; creates missing nodes
    fn locate_cell(&mut self, node: u32, st: [i64; 3], len: i64, ori: [i64; 3], dir: usize, side: i64) -> (u32, [i64; 3], i64) {
        let len = len >> 1;
        let mut ind = 0usize;
        for i in 0..3 {
            ind <<= 1;
            if i == dir && side == 1 {
                ind |= if ori[i] <= st[i] + len { 0 } else { 1 };
            } else {
                ind |= if ori[i] < st[i] + len { 0 } else { 1 };
            }
        }
        let rst = [st[0] + VERTMAP[ind][0] * len, st[1] + VERTMAP[ind][1] * len, st[2] + VERTMAP[ind][2] * len];
        if self.ints[node as usize].has(ind) {
            let chd = self.ints[node as usize].child[ind];
            if self.ints[node as usize].is_leaf(ind) {
                (chd, rst, len)
            } else {
                self.locate_cell(chd, rst, len, ori, dir, side)
            }
        } else if len == self.mindimen {
            let li = self.leaves.len() as u32;
            self.leaves.push(Leaf::default());
            let n = &mut self.ints[node as usize];
            n.child[ind] = li;
            n.leaf_bits |= 1 << ind;
            (li, rst, len)
        } else {
            let ii = self.ints.len() as u32;
            self.ints.push(Internal::new());
            self.ints[node as usize].child[ind] = ii;
            self.locate_cell(ii, rst, len, ori, dir, side)
        }
    }

    fn locate_leaf_from(&self, parent: u32, len: i64, st: [i64; 3]) -> u32 {
        let mut node = parent;
        let mut i = len / 2;
        let mut last = parent;
        while i >= self.mindimen {
            let index = (if st[0] & i != 0 { 4 } else { 0 }) | (if st[1] & i != 0 { 2 } else { 0 }) | (if st[2] & i != 0 { 1 } else { 0 });
            last = self.ints[node as usize].child[index];
            node = last;
            i >>= 1;
        }
        last
    }

    fn connect_face(&mut self, node: u32, st: [i64; 3], len: i64, dir: usize, f1: usize, f2: usize) {
        let pos = st[dir] + len / 2;
        let xdir = (dir + 1) % 3;
        let ydir = (dir + 2) % 3;
        let f2n = self.pes[f2].next;
        let (x1, y1, p1, q1) = self.get_face_point(self.pes[f2n].pos, dir);
        let (x2, y2, p2, q2) = self.get_face_point(self.pes[f2].pos, dir);
        let dx = x2 as f32 + p2 - x1 as f32 - p1;
        let dy = y2 as f32 + q2 - y1 as f32 - q1;

        let (mut rx, mut ry) = (p1, q1);
        let (mut incx, mut incy) = (1i64, 1i64);
        let (mut lx, mut ly, mut hx, mut hy) = (x1, y1, x2, y2);
        if x2 < x1 {
            incx = -1;
            rx = 1.0 - rx;
            lx = x2;
            hx = x1;
        }
        if y2 < y1 {
            incy = -1;
            ry = 1.0 - ry;
            ly = y2;
            hy = y1;
        }
        let sx = dx * incx as f32;
        let sy = dy * incy as f32;

        let mut ori = [0i64; 3];
        ori[dir] = pos / self.mindimen;
        ori[xdir] = x1;
        ori[ydir] = y1;

        let mut cur_ele_n = f1;
        let mut cur_ele_p = f2n;
        let mut cur_n = self.locate_leaf_from(node, len, self.pes[f1].pos);
        let mut cur_p = self.locate_leaf_from(node, len, self.pes[f2n].pos);
        let mut guard = 0;
        while ori[xdir] != x2 || ori[ydir] != y2 {
            guard += 1;
            if guard > 1_000_000 {
                break;
            }
            let mut choice;
            let mut next;
            if sy * (1.0 - rx) > sx * (1.0 - ry) {
                choice = 1;
                next = ori[ydir] + incy;
                if next < ly || next > hy {
                    choice = 4;
                    next = ori[xdir] + incx;
                }
            } else {
                choice = 2;
                next = ori[xdir] + incx;
                if next < lx || next > hx {
                    choice = 3;
                    next = ori[ydir] + incy;
                }
            }
            let walkdir: i64;
            let inc: i64;
            let alpha: f32;
            if choice & 1 != 0 {
                ori[ydir] = next;
                if choice == 1 {
                    rx += if sy == 0.0 { 0.0 } else { (1.0 - ry) * sx / sy };
                    ry = 0.0;
                }
                walkdir = 2;
                inc = incy;
                alpha = if x2 < x1 { 1.0 - rx } else { rx };
            } else {
                ori[xdir] = next;
                if choice == 2 {
                    ry += if sx == 0.0 { 0.0 } else { (1.0 - rx) * sy / sx };
                    rx = 0.0;
                }
                walkdir = 1;
                inc = incx;
                alpha = if y2 < y1 { 1.0 - ry } else { ry };
            }
            let nori = [ori[0] * self.mindimen, ori[1] * self.mindimen, ori[2] * self.mindimen];
            let (node_n, st_n, _) = self.locate_cell(node, st, len, nori, dir, 1);
            let (node_p, st_p, _) = self.locate_cell(node, st, len, nori, dir, 0);

            if self.pes[cur_ele_n].pos != st_n {
                let nxt = self.pes[cur_ele_n].next;
                let new_n;
                if self.pes[nxt].pos != st_n {
                    new_n = self.pe_new(st_n, nxt);
                    self.pes[cur_ele_n].next = new_n;
                } else {
                    new_n = nxt;
                }
                cur_n = self.patch_adjacent(cur_n, node_n, walkdir, inc, dir, 1, alpha);
                cur_ele_n = new_n;
            }
            if self.pes[cur_ele_p].pos != st_p {
                let new_p;
                if self.pes[f2].pos != st_p {
                    new_p = self.pe_new(st_p, cur_ele_p);
                    self.pes[f2].next = new_p;
                } else {
                    new_p = f2;
                }
                cur_p = self.patch_adjacent(cur_p, node_p, walkdir, inc, dir, 0, alpha);
                cur_ele_p = new_p;
            }
        }
    }
}
