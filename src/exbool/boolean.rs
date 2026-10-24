//! Port of Blender 4.2 `bli_mesh_boolean.cc` (boolean_trimesh / boolean_mesh).

use crate::exbool::hashtab::*;
use crate::exbool::imesh::*;
use crate::exbool::intersect::*;
use crate::exbool::libcxx_sort;
use crate::exbool::mpq::*;
use num_traits::{Signed, Zero};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoolOpType {
    None,
    Intersect,
    Union,
    Difference,
}

const FLT_EPSILON: f32 = f32::EPSILON;
const FLT_MAX: f32 = f32::MAX;
const EXTRA_TRI_INDEX: i32 = i32::MAX;

/// Edge as two vertex ids in canonical order (lower id first).
type Edge = (u32, u32);

fn make_edge(a: u32, b: u32) -> Edge {
    // vertex id == arena index
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

struct TriMeshTopology {
    edge_tri: HashMap<Edge, Vec<i32>>,
    vert_edges: HashMap<u32, Vec<Edge>>,
}

impl TriMeshTopology {
    fn new(arena: &Arena, tm: &[usize]) -> Self {
        let mut edge_tri: HashMap<Edge, Vec<i32>> = HashMap::new();
        let mut vert_edges: HashMap<u32, Vec<Edge>> = HashMap::new();
        for (t, &fi) in tm.iter().enumerate() {
            let tri = &arena.faces[fi];
            for i in 0..3 {
                let v = tri.vert[i];
                let vnext = tri.vert[(i + 1) % 3];
                let e = make_edge(v, vnext);
                let ve = vert_edges.entry(v).or_default();
                if !ve.contains(&e) {
                    ve.push(e);
                }
                let et = edge_tri.entry(e).or_default();
                if !et.contains(&(t as i32)) {
                    et.push(t as i32);
                }
            }
        }
        TriMeshTopology { edge_tri, vert_edges }
    }
    fn other_tri_if_manifold(&self, e: Edge, t: i32) -> i32 {
        if let Some(p) = self.edge_tri.get(&e) {
            if p.len() == 2 {
                return if p[0] == t { p[1] } else { p[0] };
            }
        }
        NO_INDEX
    }
    fn edge_tris(&self, e: Edge) -> Option<&Vec<i32>> {
        self.edge_tri.get(&e)
    }
}

#[derive(Clone)]
struct Patch {
    tri: Vec<i32>,
    cell_above: i32,
    cell_below: i32,
    component: i32,
}

struct PatchesInfo {
    patch: Vec<Patch>,
    tri_patch: Vec<i32>,
    pp_edge: BMap<(i32, i32), Edge>,
}

impl PatchesInfo {
    fn new(ntri: usize) -> Self {
        let mut pp_edge = BMap::new(8);
        pp_edge.reserve(100);
        PatchesInfo { patch: vec![], tri_patch: vec![NO_INDEX; ntri], pp_edge }
    }
    fn add_patch(&mut self) -> i32 {
        self.patch.push(Patch { tri: vec![], cell_above: NO_INDEX, cell_below: NO_INDEX, component: NO_INDEX });
        self.patch.len() as i32 - 1
    }
    fn grow_patch(&mut self, p: i32, t: i32) {
        self.tri_patch[t as usize] = p;
        self.patch[p as usize].tri.push(t);
    }
    fn patch_patch_edge(&self, p1: i32, p2: i32) -> Option<Edge> {
        self.pp_edge.lookup(&(p1, p2)).copied()
    }
    fn add_new_patch_patch_edge(&mut self, p1: i32, p2: i32, e: Edge) {
        self.pp_edge.add_new((p1, p2), e);
        self.pp_edge.add_new((p2, p1), e);
    }
}

#[derive(Clone)]
struct Cell {
    patches: BSet<i32>,
    winding: Vec<i32>,
    merged_to: i32,
    winding_assigned: bool,
    in_output_volume: bool,
    zero_volume: bool,
}

impl Cell {
    fn new() -> Cell {
        Cell { patches: BSet::new(8), winding: vec![], merged_to: NO_INDEX, winding_assigned: false, in_output_volume: false, zero_volume: false }
    }
    fn add_patch(&mut self, p: i32) {
        self.patches.add(p);
        self.zero_volume = false;
    }
    fn patch_other(&self, p: i32) -> i32 {
        if self.patches.len() != 2 {
            return NO_INDEX;
        }
        for &pother in self.patches.iter() {
            if pother != p {
                return pother;
            }
        }
        NO_INDEX
    }
}

fn tris_have_same_verts(arena: &Arena, tm: &[usize], t1: i32, t2: i32) -> bool {
    let a = &arena.faces[tm[t1 as usize]].vert;
    let b = &arena.faces[tm[t2 as usize]].vert;
    if a[0] == b[0] {
        return (a[1] == b[1] && a[2] == b[2]) || (a[1] == b[2] && a[2] == b[1]);
    }
    if a[0] == b[1] {
        return (a[1] == b[0] && a[2] == b[2]) || (a[1] == b[2] && a[2] == b[0]);
    }
    if a[0] == b[2] {
        return (a[1] == b[0] && a[2] == b[1]) || (a[1] == b[1] && a[2] == b[0]);
    }
    false
}

fn check_for_zero_volume(cell: &mut Cell, pinfo: &PatchesInfo, arena: &Arena, tm: &[usize]) {
    if cell.patches.len() == 2 {
        let mut p1 = NO_INDEX;
        let mut p2 = NO_INDEX;
        for &p in cell.patches.iter() {
            if p1 == NO_INDEX {
                p1 = p;
            } else {
                p2 = p;
            }
        }
        let pa = &pinfo.patch[p1 as usize];
        let pb = &pinfo.patch[p2 as usize];
        if pa.tri.len() == 1 && pb.tri.len() == 1 && tris_have_same_verts(arena, tm, pa.tri[0], pb.tri[0]) {
            cell.zero_volume = true;
        }
    }
}

fn merge_cells(merge_to: i32, merge_from: i32, cells: &mut Vec<Cell>, pinfo: &mut PatchesInfo) {
    if merge_to == merge_from {
        return;
    }
    let mut final_merge_to = merge_to;
    while cells[merge_to as usize].merged_to != NO_INDEX {
        final_merge_to = cells[merge_to as usize].merged_to;
        // `merge_to_cell = cinfo.cell(final_merge_to);` is a copy assignment into the referenced cell.
        cells[merge_to as usize] = cells[final_merge_to as usize].clone();
    }
    let from_patches: Vec<i32> = cells[merge_from as usize].patches.iter().copied().collect();
    for cell_p in from_patches {
        cells[merge_to as usize].add_patch(cell_p);
        let patch = &mut pinfo.patch[cell_p as usize];
        if patch.cell_above == merge_from {
            patch.cell_above = merge_to;
        }
        if patch.cell_below == merge_from {
            patch.cell_below = merge_to;
        }
    }
    cells[merge_from as usize].merged_to = final_merge_to;
}

fn find_patches(arena: &Arena, tm: &[usize], topo: &TriMeshTopology) -> PatchesInfo {
    let ntri = tm.len();
    let mut pinfo = PatchesInfo::new(ntri);
    let mut t_others = vec![[0i32; 3]; ntri];
    for t in 0..ntri {
        let tri = &arena.faces[tm[t]];
        for i in 0..3 {
            let e = make_edge(tri.vert[i], tri.vert[(i + 1) % 3]);
            t_others[t][i] = topo.other_tri_if_manifold(e, t as i32);
        }
    }
    let mut cur_patch_grow: Vec<i32> = vec![];
    for t in 0..ntri as i32 {
        if pinfo.tri_patch[t as usize] == -1 {
            cur_patch_grow.push(t);
            let cur_patch_index = pinfo.add_patch();
            while let Some(tcand) = cur_patch_grow.pop() {
                if pinfo.tri_patch[tcand as usize] != NO_INDEX {
                    continue;
                }
                pinfo.grow_patch(cur_patch_index, tcand);
                let tri = &arena.faces[tm[tcand as usize]];
                for i in 0..3 {
                    let e = make_edge(tri.vert[i], tri.vert[(i + 1) % 3]);
                    let t_other = t_others[tcand as usize][i];
                    if t_other != NO_INDEX {
                        if pinfo.tri_patch[t_other as usize] == NO_INDEX {
                            cur_patch_grow.push(t_other);
                        }
                    } else if let Some(etris) = topo.edge_tris(e) {
                        for &t_other in etris {
                            if t_other != tcand && pinfo.tri_patch[t_other as usize] != NO_INDEX {
                                let p_other = pinfo.tri_patch[t_other as usize];
                                if p_other == cur_patch_index {
                                    continue;
                                }
                                if pinfo.patch_patch_edge(cur_patch_index, p_other).is_none() {
                                    pinfo.add_new_patch_patch_edge(cur_patch_index, p_other, e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    pinfo
}

/// find_flap_vert: returns (flap vert, reversed)
fn find_flap_vert(tri: &[u32], e: Edge) -> Option<(u32, bool)> {
    let flapv;
    let rev;
    if tri[0] == e.0 {
        if tri[1] == e.1 {
            rev = false;
            flapv = tri[2];
        } else {
            if tri[2] != e.1 {
                return None;
            }
            rev = true;
            flapv = tri[1];
        }
    } else if tri[1] == e.0 {
        if tri[2] == e.1 {
            rev = false;
            flapv = tri[0];
        } else {
            if tri[0] != e.1 {
                return None;
            }
            rev = true;
            flapv = tri[2];
        }
    } else {
        if tri[2] != e.0 {
            return None;
        }
        if tri[0] == e.1 {
            rev = false;
            flapv = tri[1];
        } else {
            if tri[1] != e.1 {
                return None;
            }
            rev = true;
            flapv = tri[0];
        }
    }
    Some((flapv, rev))
}

struct SortCtx<'a> {
    arena: &'a Arena,
    tm: &'a [usize],
    extra: Option<usize>,
}
impl<'a> SortCtx<'a> {
    fn face(&self, t: i32) -> &'a [u32] {
        if t == EXTRA_TRI_INDEX {
            &self.arena.faces[self.extra.unwrap()].vert
        } else {
            &self.arena.faces[self.tm[t as usize]].vert
        }
    }
}

fn sort_tris_class(ctx: &SortCtx, tri: &[u32], tri0: &[u32], e: Edge) -> i32 {
    let a0 = &ctx.arena.verts[tri0[0] as usize].co;
    let a1 = &ctx.arena.verts[tri0[1] as usize].co;
    let a2 = &ctx.arena.verts[tri0[2] as usize].co;
    let (flapv0, rev0) = find_flap_vert(tri0, e).unwrap();
    let (flapv, _rev) = find_flap_vert(tri, e).unwrap();
    let flap = &ctx.arena.verts[flapv as usize].co;
    let orient = q_orient3d(a0, a1, a2, flap);
    if orient > 0 {
        if rev0 {
            4
        } else {
            3
        }
    } else if orient < 0 {
        if rev0 {
            3
        } else {
            4
        }
    } else if flapv == flapv0 {
        1
    } else {
        2
    }
}

fn sort_by_signed_triangle_index(g: &mut Vec<i32>, e: Edge, ctx: &SortCtx) {
    let mut signed_g: Vec<i32> = Vec::with_capacity(g.len());
    for &gi in g.iter() {
        let tri = ctx.face(gi);
        let (_, rev) = find_flap_vert(tri, e).unwrap();
        signed_g.push(if rev { -gi } else { gi });
    }
    signed_g.sort();
    for i in 0..g.len() {
        g[i] = signed_g[i].abs();
    }
}

fn sort_tris_around_edge(ctx: &SortCtx, e: Edge, tris: &[i32], t0: i32) -> Vec<i32> {
    if tris.is_empty() {
        return vec![];
    }
    let mut g: [Vec<i32>; 4] = [vec![tris[0]], vec![], vec![], vec![]];
    let triref = ctx.face(tris[0]);
    for i in 1..tris.len() {
        let t = tris[i];
        let tri = ctx.face(t);
        let group_num = sort_tris_class(ctx, tri, triref, e);
        g[(group_num - 1) as usize].push(t);
    }
    if g[0].len() > 1 {
        sort_by_signed_triangle_index(&mut g[0], e, ctx);
    }
    if g[1].len() > 1 {
        sort_by_signed_triangle_index(&mut g[1], e, ctx);
    }
    if g[2].len() > 1 {
        let s = sort_tris_around_edge(ctx, e, &g[2], t0);
        g[2] = s;
    }
    if g[3].len() > 1 {
        let s = sort_tris_around_edge(ctx, e, &g[3], t0);
        g[3] = s;
    }
    let mut ans = vec![];
    if tris[0] == t0 {
        ans.extend(&g[0]);
        ans.extend(&g[3]);
        ans.extend(&g[1]);
        ans.extend(&g[2]);
    } else {
        ans.extend(&g[2]);
        ans.extend(&g[0]);
        ans.extend(&g[3]);
        ans.extend(&g[1]);
    }
    ans
}

fn find_cells_from_edge(arena: &Arena, tm: &[usize], topo: &TriMeshTopology, pinfo: &mut PatchesInfo, cells: &mut Vec<Cell>, e: Edge) {
    let edge_tris = topo.edge_tris(e).unwrap();
    let ctx = SortCtx { arena, tm, extra: None };
    let sorted_tris = sort_tris_around_edge(&ctx, e, edge_tris, edge_tris[0]);
    let n = edge_tris.len();
    let mut edge_patches = vec![0i32; n];
    for i in 0..n {
        edge_patches[i] = pinfo.tri_patch[sorted_tris[i] as usize];
    }
    for i in 0..n {
        let inext = (i + 1) % n;
        let r_index = edge_patches[i];
        let rnext_index = edge_patches[inext];
        let (_, r_flipped) = find_flap_vert(&arena.faces[tm[sorted_tris[i] as usize]].vert, e).unwrap();
        let (_, rnext_flipped) = find_flap_vert(&arena.faces[tm[sorted_tris[inext] as usize]].vert, e).unwrap();
        // r_follow_cell and rnext_prev_cell may alias the same patch (r_index == rnext_index): read through accessors.
        let get_r = |pinfo: &PatchesInfo| -> i32 {
            let r = &pinfo.patch[r_index as usize];
            if r_flipped {
                r.cell_below
            } else {
                r.cell_above
            }
        };
        let get_rn = |pinfo: &PatchesInfo| -> i32 {
            let r = &pinfo.patch[rnext_index as usize];
            if rnext_flipped {
                r.cell_above
            } else {
                r.cell_below
            }
        };
        let set_r = |pinfo: &mut PatchesInfo, c: i32| {
            let r = &mut pinfo.patch[r_index as usize];
            if r_flipped {
                r.cell_below = c;
            } else {
                r.cell_above = c;
            }
        };
        let set_rn = |pinfo: &mut PatchesInfo, c: i32| {
            let r = &mut pinfo.patch[rnext_index as usize];
            if rnext_flipped {
                r.cell_above = c;
            } else {
                r.cell_below = c;
            }
        };
        let r_follow = get_r(pinfo);
        let rn_prev = get_rn(pinfo);
        if r_follow == NO_INDEX && rn_prev == NO_INDEX {
            cells.push(Cell::new());
            let c = cells.len() as i32 - 1;
            set_r(pinfo, c);
            set_rn(pinfo, c);
            let cell = &mut cells[c as usize];
            cell.add_patch(r_index);
            cell.add_patch(rnext_index);
            check_for_zero_volume(cell, pinfo, arena, tm);
        } else if r_follow != NO_INDEX && rn_prev == NO_INDEX {
            let c = r_follow;
            set_rn(pinfo, c);
            let cell = &mut cells[c as usize];
            cell.add_patch(rnext_index);
            check_for_zero_volume(cell, pinfo, arena, tm);
        } else if r_follow == NO_INDEX && rn_prev != NO_INDEX {
            let c = rn_prev;
            set_r(pinfo, c);
            let cell = &mut cells[c as usize];
            cell.add_patch(r_index);
            check_for_zero_volume(cell, pinfo, arena, tm);
        } else if r_follow != rn_prev {
            let follow_n = cells[r_follow as usize].patches.len();
            let prev_n = cells[rn_prev as usize].patches.len();
            if follow_n >= prev_n {
                merge_cells(r_follow, rn_prev, cells, pinfo);
            }
        } else {
            merge_cells(rn_prev, r_follow, cells, pinfo);
        }
    }
}

fn find_cells(arena: &Arena, tm: &[usize], topo: &TriMeshTopology, pinfo: &mut PatchesInfo) -> Vec<Cell> {
    let mut cells: Vec<Cell> = vec![];
    let mut processed: std::collections::HashSet<Edge> = std::collections::HashSet::new();
    let items: Vec<((i32, i32), Edge)> = pinfo.pp_edge.items().map(|(k, v)| (*k, *v)).collect();
    for (k, e) in items {
        let (p, q) = k;
        if p < q && processed.insert(e) {
            find_cells_from_edge(arena, tm, topo, pinfo, &mut cells, e);
        }
    }
    for p in 0..pinfo.patch.len() as i32 {
        if pinfo.patch[p as usize].cell_above == NO_INDEX {
            cells.push(Cell::new());
            let c = cells.len() as i32 - 1;
            pinfo.patch[p as usize].cell_above = c;
            cells[c as usize].add_patch(p);
        }
        if pinfo.patch[p as usize].cell_below == NO_INDEX {
            cells.push(Cell::new());
            let c = cells.len() as i32 - 1;
            pinfo.patch[p as usize].cell_below = c;
            cells[c as usize].add_patch(p);
        }
    }
    cells
}

fn find_patch_components(cells: &[Cell], pinfo: &mut PatchesInfo) -> Vec<Vec<i32>> {
    if pinfo.patch.is_empty() {
        return vec![];
    }
    let mut current_component = 0;
    let mut cell_processed = vec![false; cells.len()];
    let mut ans: Vec<Vec<i32>> = vec![];
    for pstart in 0..pinfo.patch.len() as i32 {
        if pinfo.patch[pstart as usize].component != NO_INDEX {
            continue;
        }
        ans.push(vec![]);
        ans[current_component as usize].push(pstart);
        let mut stack = vec![pstart];
        pinfo.patch[pstart as usize].component = current_component;
        while let Some(p) = stack.pop() {
            let (ca, cb) = (pinfo.patch[p as usize].cell_above, pinfo.patch[p as usize].cell_below);
            for c in [ca, cb] {
                if cell_processed[c as usize] {
                    continue;
                }
                cell_processed[c as usize] = true;
                for &pn in cells[c as usize].patches.iter() {
                    if pinfo.patch[pn as usize].component == NO_INDEX {
                        pinfo.patch[pn as usize].component = current_component;
                        stack.push(pn);
                        ans[current_component as usize].push(pn);
                    }
                }
            }
        }
        current_component += 1;
    }
    ans
}

fn patch_cell_graph_ok(cells: &[Cell], pinfo: &PatchesInfo) -> bool {
    for c in cells {
        if c.merged_to != NO_INDEX {
            continue;
        }
        if c.patches.is_empty() {
            return false;
        }
        for &p in c.patches.iter() {
            if p as usize >= pinfo.patch.len() {
                return false;
            }
        }
    }
    for p in &pinfo.patch {
        if p.cell_above == NO_INDEX || p.cell_below == NO_INDEX {
            return false;
        }
        if p.cell_above as usize >= cells.len() || p.cell_below as usize >= cells.len() {
            return false;
        }
    }
    true
}

fn is_pwn(arena: &Arena, tm: &[usize], topo: &TriMeshTopology) -> bool {
    for (edge, tris) in topo.edge_tri.iter() {
        let mut tot_orient = 0;
        for &t in tris {
            let face = &arena.faces[tm[t as usize]].vert;
            for i in 0..3 {
                if face[i] == edge.0 {
                    if face[(i + 1) % 3] == edge.1 {
                        tot_orient += 1;
                    } else {
                        tot_orient -= 1;
                    }
                }
            }
        }
        if tot_orient != 0 {
            return false;
        }
    }
    true
}

fn find_cell_for_point_near_edge(p: &V3q, e: Edge, arena: &mut Arena, tm: &[usize], topo: &TriMeshTopology, pinfo: &PatchesInfo) -> i32 {
    let etris = topo.edge_tris(e).unwrap().clone();
    let dummy_vert = arena.add_or_find_vert_q(p.clone(), NO_INDEX);
    let dummy_tri = arena.add_face(vec![e.0, e.1, dummy_vert], NO_INDEX, vec![NO_INDEX; 3], vec![false; 3]);
    let mut edge_tris = etris.clone();
    edge_tris.push(EXTRA_TRI_INDEX);
    let ctx = SortCtx { arena, tm, extra: Some(dummy_tri) };
    let sorted_tris = sort_tris_around_edge(&ctx, e, &edge_tris, edge_tris[0]);
    let dummy_index = sorted_tris.iter().position(|&x| x == EXTRA_TRI_INDEX).unwrap();
    let prev_tri = if dummy_index == 0 { sorted_tris[sorted_tris.len() - 1] } else { sorted_tris[dummy_index - 1] };
    let prev_patch = &pinfo.patch[pinfo.tri_patch[prev_tri as usize] as usize];
    let (_, prev_flipped) = find_flap_vert(&arena.faces[tm[prev_tri as usize]].vert, e).unwrap();
    if prev_flipped {
        prev_patch.cell_below
    } else {
        prev_patch.cell_above
    }
}

fn find_ambient_cell(arena: &mut Arena, tm: &[usize], component_patches: Option<&Vec<i32>>, topo: &TriMeshTopology, pinfo: &PatchesInfo) -> i32 {
    let v_extreme: u32;
    match component_patches {
        None => {
            let mut ans = arena.faces[tm[0]].vert[0];
            for &fi in tm {
                for &v in &arena.faces[fi].vert {
                    if arena.verts[v as usize].co[0] > arena.verts[ans as usize].co[0] {
                        ans = v;
                    }
                }
            }
            v_extreme = ans;
        }
        Some(cp) => {
            let p0 = cp[0];
            let init = arena.faces[tm[pinfo.patch[p0 as usize].tri[0] as usize]].vert[0];
            let mut ans = init;
            for &p in cp {
                let mut v_ans = init;
                for &t in &pinfo.patch[p as usize].tri {
                    for &v in &arena.faces[tm[t as usize]].vert {
                        if arena.verts[v as usize].co[0] > arena.verts[v_ans as usize].co[0] {
                            v_ans = v;
                        }
                    }
                }
                if arena.verts[v_ans as usize].co[0] > arena.verts[ans as usize].co[0] {
                    ans = v_ans;
                }
            }
            v_extreme = ans;
        }
    }
    let edges = topo.vert_edges.get(&v_extreme).unwrap().clone();
    let extreme = arena.verts[v_extreme as usize].co.clone();
    let mut ehull: Option<Edge> = None;
    let mut max_abs_slope = q_int(-1);
    for e in edges {
        let v_other = if e.0 == v_extreme { e.1 } else { e.0 };
        let co_other = &arena.verts[v_other as usize].co;
        let delta_x = &co_other[0] - &extreme[0];
        if delta_x.is_zero() {
            ehull = Some(e);
            break;
        }
        let abs_slope = ((&co_other[1] - &extreme[1]) / &delta_x).abs();
        if abs_slope > max_abs_slope {
            ehull = Some(e);
            max_abs_slope = abs_slope;
        }
    }
    let mut p_in_ambient = extreme.clone();
    p_in_ambient[0] = &p_in_ambient[0] + q_one();
    find_cell_for_point_near_edge(&p_in_ambient, ehull.unwrap(), arena, tm, topo, pinfo)
}

fn find_good_sorting_edge(arena: &Arena, testp: u32, closestp: u32, topo: &TriMeshTopology) -> Edge {
    let co_closest = &arena.verts[closestp as usize].co;
    let co_test = &arena.verts[testp as usize].co;
    let abscissa = q3_sub(co_test, co_closest);
    let mut axis = 0;
    while axis < 3 {
        if !abscissa[axis].is_zero() {
            break;
        }
        axis += 1;
    }
    let axis_next = (axis + 1) % 3;
    let axis_next_next = (axis_next + 1) % 3;
    let mut ordinate = [q_zero(), q_zero(), q_zero()];
    ordinate[axis] = abscissa[axis_next].clone();
    ordinate[axis_next] = -abscissa[axis].clone();
    ordinate[axis_next_next] = q_zero();
    let normal = q3_cross(&abscissa, &ordinate);
    let nlen2 = q3_dot(&normal, &normal);
    let mut max_abs_slope = q_int(-1);
    let mut esort: Option<Edge> = None;
    for &e in topo.vert_edges.get(&closestp).unwrap() {
        let v_other = if e.0 == closestp { e.1 } else { e.0 };
        let co_other = &arena.verts[v_other as usize].co;
        let evec = q3_sub(co_other, co_closest);
        let f = q3_dot(&evec, &normal) / &nlen2;
        let proj_evec = [&evec[0] - &f * &normal[0], &evec[1] - &f * &normal[1], &evec[2] - &f * &normal[2]];
        let evec_a = q3_dot(&proj_evec, &abscissa);
        let evec_o = q3_dot(&proj_evec, &ordinate);
        if evec_a.is_zero() {
            esort = Some(e);
            break;
        }
        let abs_slope = (evec_o / evec_a).abs();
        if abs_slope > max_abs_slope {
            esort = Some(e);
            max_abs_slope = abs_slope;
        }
    }
    esort.unwrap()
}

fn find_containing_cell(v: u32, t: i32, close_edge0: i32, close_vert: i32, pinfo: &PatchesInfo, arena: &mut Arena, tm: &[usize], topo: &TriMeshTopology) -> i32 {
    let tri = arena.faces[tm[t as usize]].vert.clone();
    let mut close_edge = close_edge0;
    let etest: Edge;
    if close_edge == -1 && close_vert == -1 {
        close_edge = 0;
    }
    if close_edge != -1 {
        let v0 = tri[close_edge as usize];
        let v1 = tri[((close_edge + 1) % 3) as usize];
        let mut found = None;
        for &e in topo.vert_edges.get(&v0).unwrap() {
            if (e.0 == v0 && e.1 == v1) || (e.0 == v1 && e.1 == v0) {
                found = Some(e);
                break;
            }
        }
        etest = found.unwrap();
    } else {
        let cv = close_vert as usize;
        let mut vert_cv = tri[cv];
        if vert_cv == v {
            vert_cv = tri[(cv + 1) % 3];
        }
        etest = find_good_sorting_edge(arena, v, vert_cv, topo);
    }
    let co = arena.verts[v as usize].co.clone();
    find_cell_for_point_near_edge(&co, etest, arena, tm, topo, pinfo)
}

fn closest_on_tri_to_point(p: &V3q, a: &V3q, b: &V3q, c: &V3q, r_edge: &mut i32, r_vert: &mut i32) -> Q {
    let ab = q3_sub(b, a);
    let ac = q3_sub(c, a);
    let ap = q3_sub(p, a);
    let d1 = q3_dot(&ab, &ap);
    let d2 = q3_dot(&ac, &ap);
    let zero = q_zero();
    let dist2 = |x: &V3q, y: &V3q| -> Q {
        let d = q3_sub(x, y);
        q3_dot(&d, &d)
    };
    if d1 <= zero && d2 <= zero {
        *r_edge = -1;
        *r_vert = 0;
        return dist2(p, a);
    }
    let bp = q3_sub(p, b);
    let d3 = q3_dot(&ab, &bp);
    let d4 = q3_dot(&ac, &bp);
    if d3 >= zero && d4 <= d3 {
        *r_edge = -1;
        *r_vert = 1;
        return dist2(p, b);
    }
    let vc = &d1 * &d4 - &d3 * &d2;
    if vc <= zero && d1 >= zero && d3 <= zero {
        let v = &d1 / (&d1 - &d3);
        let r = [&a[0] + &ab[0] * &v, &a[1] + &ab[1] * &v, &a[2] + &ab[2] * &v];
        *r_vert = -1;
        *r_edge = 0;
        return dist2(p, &r);
    }
    let cp = q3_sub(p, c);
    let d5 = q3_dot(&ab, &cp);
    let d6 = q3_dot(&ac, &cp);
    if d6 >= zero && d5 <= d6 {
        *r_edge = -1;
        *r_vert = 2;
        return dist2(p, c);
    }
    let vb = &d5 * &d2 - &d1 * &d6;
    if vb <= zero && d2 >= zero && d6 <= zero {
        let w = &d2 / (&d2 - &d6);
        let r = [&a[0] + &ac[0] * &w, &a[1] + &ac[1] * &w, &a[2] + &ac[2] * &w];
        *r_vert = -1;
        *r_edge = 2;
        return dist2(p, &r);
    }
    let va = &d3 * &d6 - &d5 * &d4;
    if va <= zero && (&d4 - &d3) >= zero && (&d5 - &d6) >= zero {
        let w = (&d4 - &d3) / ((&d4 - &d3) + (&d5 - &d6));
        let cb = q3_sub(c, b);
        let r = [&b[0] + &cb[0] * &w, &b[1] + &cb[1] * &w, &b[2] + &cb[2] * &w];
        *r_vert = -1;
        *r_edge = 1;
        return dist2(p, &r);
    }
    let denom = q_one() / (&va + &vb + &vc);
    let v = &vb * &denom;
    let w = &vc * &denom;
    let acw = [&ac[0] * &w, &ac[1] * &w, &ac[2] * &w];
    let r = [&a[0] + &ab[0] * &v + &acw[0], &a[1] + &ab[1] * &v + &acw[1], &a[2] + &ab[2] * &v + &acw[2]];
    *r_vert = -1;
    *r_edge = -1;
    dist2(p, &r)
}

struct ComponentContainer {
    containing_component: i32,
    nearest_cell: i32,
    dist_to_cell: Q,
}

fn find_component_containers(
    comp: usize,
    components: &[Vec<i32>],
    ambient_cell: &[i32],
    tm: &[usize],
    pinfo: &PatchesInfo,
    topo: &TriMeshTopology,
    comp_bb: &[BoundingBox],
    arena: &mut Arena,
) -> Vec<ComponentContainer> {
    let mut ans = vec![];
    let test_p = components[comp][0];
    let test_t = pinfo.patch[test_p as usize].tri[0];
    let test_v = arena.faces[tm[test_t as usize]].vert[0];
    for comp_other in 0..components.len() {
        if comp == comp_other {
            continue;
        }
        if !bbs_might_intersect(&comp_bb[comp], &comp_bb[comp_other]) {
            continue;
        }
        let mut nearest_tri = NO_INDEX;
        let mut nearest_tri_close_vert = -1;
        let mut nearest_tri_close_edge = -1;
        let mut nearest_tri_dist_squared = q_zero();
        let test_co = arena.verts[test_v as usize].co.clone();
        for &p in &components[comp_other] {
            for &t in &pinfo.patch[p as usize].tri {
                let tri = &arena.faces[tm[t as usize]].vert;
                let (mut close_vert, mut close_edge) = (0, 0);
                let d2 = closest_on_tri_to_point(&test_co, &arena.verts[tri[0] as usize].co, &arena.verts[tri[1] as usize].co, &arena.verts[tri[2] as usize].co, &mut close_edge, &mut close_vert);
                if nearest_tri == NO_INDEX || d2 < nearest_tri_dist_squared {
                    nearest_tri = t;
                    nearest_tri_close_edge = close_edge;
                    nearest_tri_close_vert = close_vert;
                    nearest_tri_dist_squared = d2;
                }
            }
        }
        let containing_cell = find_containing_cell(test_v, nearest_tri, nearest_tri_close_edge, nearest_tri_close_vert, pinfo, arena, tm, topo);
        if containing_cell != ambient_cell[comp_other] {
            ans.push(ComponentContainer { containing_component: comp_other as i32, nearest_cell: containing_cell, dist_to_cell: nearest_tri_dist_squared });
        }
    }
    ans
}

fn populate_comp_bbs(components: &[Vec<i32>], pinfo: &PatchesInfo, arena: &Arena, tm: &[usize]) -> Vec<BoundingBox> {
    let mut comp_bb = vec![BoundingBox::new(); components.len()];
    let mut max_abs = vec![0.0f64; components.len()];
    for c in 0..components.len() {
        for &p in &components[c] {
            for &t in &pinfo.patch[p as usize].tri {
                for &v in &arena.faces[tm[t as usize]].vert {
                    let d = &arena.verts[v as usize].d;
                    comp_bb[c].combine_d(d);
                    for i in 0..3 {
                        max_abs[c] = max_abs[c].max(d[i].abs());
                    }
                }
            }
        }
    }
    let mut all_max_abs = 0.0f64;
    for c in 0..components.len() {
        all_max_abs = all_max_abs.max(max_abs[c]);
    }
    let pad_factor = 10.0f32;
    let mut pad: f32 = if all_max_abs == 0.0 { FLT_EPSILON } else { ((2.0f32 * FLT_EPSILON) as f64 * all_max_abs) as f32 };
    pad *= pad_factor;
    for bb in comp_bb.iter_mut() {
        bb.expand(pad);
    }
    comp_bb
}

fn finish_patch_cell_graph(arena: &mut Arena, tm: &[usize], cells: &mut Vec<Cell>, pinfo: &mut PatchesInfo, topo: &TriMeshTopology) {
    let components = find_patch_components(cells, pinfo);
    if components.len() <= 1 {
        return;
    }
    let mut ambient_cell = vec![0i32; components.len()];
    for comp in 0..components.len() {
        ambient_cell[comp] = find_ambient_cell(arena, tm, Some(&components[comp]), topo, pinfo);
    }
    let tot_components = components.len();
    let mut comp_cont: Vec<Vec<ComponentContainer>> = (0..tot_components).map(|_| vec![]).collect();
    if tot_components > 1 {
        let comp_bb = populate_comp_bbs(&components, pinfo, arena, tm);
        for comp in 0..tot_components {
            comp_cont[comp] = find_component_containers(comp, &components, &ambient_cell, tm, pinfo, topo, &comp_bb, arena);
        }
    }
    let mut outer_components = vec![];
    for comp in 0..comp_cont.len() {
        if comp_cont[comp].is_empty() {
            outer_components.push(comp);
        } else {
            // `ComponentContainer &closest = comp_cont[comp][0]; ... closest = comp_cont[comp][i]` assigns the
            // container *value* into slot 0.
            for i in 1..comp_cont[comp].len() {
                if comp_cont[comp][i].dist_to_cell < comp_cont[comp][0].dist_to_cell {
                    let (a, b) = (comp_cont[comp][i].containing_component, comp_cont[comp][i].nearest_cell);
                    let d = comp_cont[comp][i].dist_to_cell.clone();
                    comp_cont[comp][0] = ComponentContainer { containing_component: a, nearest_cell: b, dist_to_cell: d };
                }
            }
            let comp_ambient = ambient_cell[comp];
            let cont_cell = comp_cont[comp][0].nearest_cell;
            let _ = comp_cont[comp][0].containing_component;
            merge_cells(cont_cell, comp_ambient, cells, pinfo);
        }
    }
    if outer_components.len() > 1 {
        let merged_ambient = ambient_cell[outer_components[0]];
        for i in 1..outer_components.len() {
            merge_cells(merged_ambient, ambient_cell[outer_components[i]], cells, pinfo);
        }
    }
}

fn apply_bool_op(op: BoolOpType, winding: &[i32]) -> bool {
    let nw = winding.len();
    match op {
        BoolOpType::Intersect => {
            for i in 0..nw {
                if winding[i] == 0 {
                    return false;
                }
            }
            true
        }
        BoolOpType::Union => {
            for i in 0..nw {
                if winding[i] != 0 {
                    return true;
                }
            }
            false
        }
        BoolOpType::Difference => {
            if winding[0] == 0 {
                return false;
            }
            if nw == 1 {
                return true;
            }
            for i in 1..nw {
                if winding[i] >= 1 {
                    return false;
                }
            }
            true
        }
        BoolOpType::None => false,
    }
}

fn propagate_windings_and_in_output_volume(pinfo: &PatchesInfo, cells: &mut Vec<Cell>, c_ambient: i32, op: BoolOpType, nshapes: usize, shape_fn: &dyn Fn(i32) -> i32) {
    for c in cells.iter_mut() {
        c.winding = vec![0; nshapes];
    }
    {
        let ca = &mut cells[c_ambient as usize];
        ca.winding = vec![0; nshapes];
        ca.winding_assigned = true;
    }
    let mut queue: Vec<i32> = vec![c_ambient];
    let mut head = 0;
    while head < queue.len() {
        let c = queue[head];
        head += 1;
        let patches: Vec<i32> = cells[c as usize].patches.iter().copied().collect();
        for p in patches {
            let patch = &pinfo.patch[p as usize];
            let p_above_c = patch.cell_below == c;
            let c_neighbor = if p_above_c { patch.cell_above } else { patch.cell_below };
            if !cells[c_neighbor as usize].winding_assigned {
                let winding_delta = if p_above_c { -1 } else { 1 };
                let t = patch.tri[0];
                let shape = shape_fn(t);
                let from_winding = cells[c as usize].winding.clone();
                let cn = &mut cells[c_neighbor as usize];
                cn.winding = from_winding;
                if shape >= 0 {
                    cn.winding[shape as usize] += winding_delta;
                }
                cn.winding_assigned = true;
                cn.in_output_volume = apply_bool_op(op, &cn.winding);
                queue.push(c_neighbor);
            }
        }
    }
}

fn add_flipped(arena: &mut Arena, f: usize) -> usize {
    let tri = arena.faces[f].clone();
    arena.add_face(
        vec![tri.vert[0], tri.vert[2], tri.vert[1]],
        tri.orig,
        vec![tri.edge_orig[2], tri.edge_orig[1], tri.edge_orig[0]],
        vec![tri.is_intersect[2], tri.is_intersect[1], tri.is_intersect[0]],
    )
}

fn extract_zero_volume_cell_tris(r_tris: &mut Vec<usize>, tm: &[usize], pinfo: &PatchesInfo, cells: &[Cell], arena: &mut Arena) {
    let np = pinfo.patch.len();
    let mut adj_to_zv = vec![false; np];
    for p in 0..np {
        let patch = &pinfo.patch[p];
        adj_to_zv[p] = cells[patch.cell_above as usize].zero_volume || cells[patch.cell_below as usize].zero_volume;
    }
    let mut allocated_to_stack = vec![false; np];
    for p in 0..np as i32 {
        if !adj_to_zv[p as usize] || allocated_to_stack[p as usize] {
            continue;
        }
        let mut stack = vec![p];
        let mut flipped = vec![false];
        allocated_to_stack[p as usize] = true;
        let mut pwalk = p;
        let mut c = pinfo.patch[pwalk as usize].cell_above;
        while cells[c as usize].zero_volume {
            let pother = cells[c as usize].patch_other(pwalk);
            let flip = pinfo.patch[pother as usize].cell_above == c;
            flipped.push(flip);
            stack.push(pother);
            allocated_to_stack[pother as usize] = true;
            pwalk = pother;
            c = if flip { pinfo.patch[pwalk as usize].cell_below } else { pinfo.patch[pwalk as usize].cell_above };
        }
        let above_stack_cell = c;
        pwalk = p;
        c = pinfo.patch[pwalk as usize].cell_below;
        while cells[c as usize].zero_volume {
            let pother = cells[c as usize].patch_other(pwalk);
            let flip = pinfo.patch[pother as usize].cell_below == c;
            flipped.push(flip);
            stack.push(pother);
            allocated_to_stack[pother as usize] = true;
            pwalk = pother;
            c = if flip { pinfo.patch[pwalk as usize].cell_above } else { pinfo.patch[pwalk as usize].cell_below };
        }
        let below_stack_cell = c;
        if cells[above_stack_cell as usize].in_output_volume ^ cells[below_stack_cell as usize].in_output_volume {
            let need_flipped_tri = cells[above_stack_cell as usize].in_output_volume;
            let mut t_to_add = NO_INDEX;
            for i in 0..stack.len() {
                if flipped[i] == need_flipped_tri {
                    t_to_add = pinfo.patch[stack[i] as usize].tri[0];
                    r_tris.push(tm[t_to_add as usize]);
                    break;
                }
            }
            if t_to_add == NO_INDEX {
                let f = tm[pinfo.patch[p as usize].tri[0] as usize];
                let flipped_f = add_flipped(arena, f);
                r_tris.push(flipped_f);
            }
        }
    }
}

fn extract_from_in_output_volume_diffs(arena: &mut Arena, tm: &[usize], pinfo: &PatchesInfo, cells: &[Cell]) -> Vec<usize> {
    let mut out_tris = vec![];
    let mut any_zero_volume_cell = false;
    for t in 0..tm.len() {
        let p = pinfo.tri_patch[t];
        let patch = &pinfo.patch[p as usize];
        let cell_above = &cells[patch.cell_above as usize];
        let cell_below = &cells[patch.cell_below as usize];
        let adjacent_zero_volume_cell = cell_above.zero_volume || cell_below.zero_volume;
        any_zero_volume_cell |= adjacent_zero_volume_cell;
        if (cell_above.in_output_volume ^ cell_below.in_output_volume) && !adjacent_zero_volume_cell {
            let flip = cell_above.in_output_volume;
            let f = tm[t];
            if flip {
                let ff = add_flipped(arena, f);
                out_tris.push(ff);
            } else {
                out_tris.push(f);
            }
        }
    }
    if any_zero_volume_cell {
        extract_zero_volume_cell_tris(&mut out_tris, tm, pinfo, cells, arena);
    }
    out_tris
}

// ---------------------------------------------------------------------------------------------------------------------
// Ray casting (non-PWN input)
// ---------------------------------------------------------------------------------------------------------------------

struct RayTree {
    lo: Vec<[f32; 3]>,
    hi: Vec<[f32; 3]>,
    tris: Vec<[[f32; 3]; 3]>,
}

fn raycast_tree(arena: &Arena, tm: &[usize]) -> RayTree {
    let mut lo = vec![];
    let mut hi = vec![];
    let mut tris = vec![];
    for &fi in tm {
        let f = &arena.faces[fi];
        let mut t = [[0f32; 3]; 3];
        for j in 0..3 {
            let v = &arena.verts[f.vert[j] as usize];
            for k in 0..3 {
                t[j][k] = v.d[k] as f32;
            }
        }
        let mut l = [FLT_MAX; 3];
        let mut h = [-FLT_MAX; 3];
        for j in 0..3 {
            for k in 0..3 {
                l[k] = l[k].min(t[j][k]);
                h[k] = h[k].max(t[j][k]);
            }
        }
        for k in 0..3 {
            l[k] -= FLT_EPSILON;
            h[k] += FLT_EPSILON;
        }
        lo.push(l);
        hi.push(h);
        tris.push(t);
    }
    RayTree { lo, hi, tris }
}

fn ray_nearest_hit(origin: &[f32; 3], dir: &[f32; 3], lo: &[f32; 3], hi: &[f32; 3]) -> f32 {
    let mut low = 0.0f32;
    let mut upper = FLT_MAX;
    let radius = 0.0f32;
    for i in 0..3 {
        if dir[i] == 0.0 {
            if origin[i] < lo[i] - radius || origin[i] > hi[i] + radius {
                return FLT_MAX;
            }
        } else {
            let ll = (lo[i] - radius - origin[i]) / dir[i];
            let lu = (hi[i] + radius - origin[i]) / dir[i];
            if dir[i] > 0.0 {
                if ll > low {
                    low = ll;
                }
                if lu < upper {
                    upper = lu;
                }
            } else {
                if lu > low {
                    low = lu;
                }
                if ll < upper {
                    upper = ll;
                }
            }
            if low > upper {
                return FLT_MAX;
            }
        }
    }
    low
}

fn sub3f(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross3f(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot3f(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn isect_ray_tri_epsilon_v3(ray_origin: &[f32; 3], ray_direction: &[f32; 3], v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3], epsilon: f32) -> bool {
    let e1 = sub3f(v1, v0);
    let e2 = sub3f(v2, v0);
    let p = cross3f(ray_direction, &e2);
    let a = dot3f(&e1, &p);
    if a == 0.0 {
        return false;
    }
    let f = 1.0f32 / a;
    let s = sub3f(ray_origin, v0);
    let u = f * dot3f(&s, &p);
    if u < -epsilon || u > 1.0 + epsilon {
        return false;
    }
    let q = cross3f(&s, &e1);
    let v = f * dot3f(ray_direction, &q);
    if v < -epsilon || (u + v) > 1.0 + epsilon {
        return false;
    }
    let lambda = f * dot3f(&e2, &q);
    if lambda < 0.0 {
        return false;
    }
    true
}

fn orient3d_d(a: &[f64; 3], b: &[f64; 3], c: &[f64; 3], d: &[f64; 3]) -> i32 {
    use robust::{orient3d, Coord3D};
    let co = |p: &[f64; 3]| Coord3D { x: p[0], y: p[1], z: p[2] };
    let r = orient3d(co(a), co(b), co(c), co(d));
    if r > 0.0 {
        1
    } else if r < 0.0 {
        -1
    } else {
        0
    }
}

fn test_tri_inside_shapes(arena: &mut Arena, tm: &[usize], shape_fn: &dyn Fn(i32) -> i32, nshapes: usize, test_t_index: usize, tree: &RayTree, in_shape: &mut Vec<f32>) {
    let tri_test = tm[test_t_index];
    let shape = shape_fn(arena.faces[tri_test].orig);
    if shape == -1 {
        for x in in_shape.iter_mut() {
            *x = 0.0;
        }
        return;
    }
    let (v0, v1, v2) = {
        let f = &arena.faces[tri_test];
        (f.vert[0], f.vert[1], f.vert[2])
    };
    let (d0, d1, d2) = (arena.verts[v0 as usize].d, arena.verts[v1 as usize].d, arena.verts[v2 as usize].d);
    let test_point = [d0[0] / 3.0 + d1[0] / 3.0 + d2[0] / 3.0, d0[1] / 3.0 + d1[1] / 3.0 + d2[1] / 3.0, d0[2] / 3.0 + d1[2] / 3.0 + d2[2] / 3.0];
    arena.populate_plane(tri_test, false);
    let pn = arena.faces[tri_test].plane.as_ref().unwrap().norm;
    let len2 = dot3d(&pn, &pn);
    let norm = if len2 > 1.0e-70 {
        let l = len2.sqrt();
        [pn[0] / l, pn[1] / l, pn[2] / l]
    } else {
        [0.0; 3]
    };
    let offset_amount = 1e-5f64;
    let offset_test_point = [test_point[0] + offset_amount * norm[0], test_point[1] + offset_amount * norm[1], test_point[2] + offset_amount * norm[2]];
    const R1: f32 = 0.9987025295199663;
    const RA: f32 = 0.04993512647599832;
    const RB: f32 = 0.009987025295199663;
    let test_rays: [[f32; 3]; 6] = [[R1, RA, RB], [-R1, -RA, -RB], [RB, R1, RA], [-RB, -R1, -RA], [RA, RB, R1], [-RA, -RB, -R1]];
    let mut count_insides = vec![0i32; nshapes];
    let co = [offset_test_point[0] as f32, offset_test_point[1] as f32, offset_test_point[2] as f32];
    for ray in test_rays.iter() {
        let mut hit_parity = vec![0i32; nshapes];
        for (i, &fi) in tm.iter().enumerate() {
            if ray_nearest_hit(&co, ray, &tree.lo[i], &tree.hi[i]) >= FLT_MAX {
                continue;
            }
            let tri = &arena.faces[fi];
            let sh = shape_fn(tri.orig);
            if sh == -1 {
                continue;
            }
            let t = &tree.tris[i];
            if isect_ray_tri_epsilon_v3(&co, ray, &t[0], &t[1], &t[2], FLT_EPSILON) {
                let o_db = [co[0] as f64, co[1] as f64, co[2] as f64];
                let parity = orient3d_d(&arena.verts[tri.vert[0] as usize].d, &arena.verts[tri.vert[1] as usize].d, &arena.verts[tri.vert[2] as usize].d, &o_db);
                hit_parity[sh as usize] += parity;
            }
        }
        for j in 0..nshapes {
            if j as i32 != shape && hit_parity[j] > 0 {
                count_insides[j] += 1;
            }
        }
    }
    for j in 0..nshapes {
        if j as i32 == shape {
            in_shape[j] = 1.0;
        } else {
            in_shape[j] = count_insides[j] as f32 / 6.0f32;
        }
    }
}

fn raycast_test_remove(op: BoolOpType, winding: &mut Vec<i32>, shape: usize) -> (bool, bool) {
    winding[shape] = 0;
    let in0 = apply_bool_op(op, winding);
    winding[shape] = 1;
    let in1 = apply_bool_op(op, winding);
    let do_remove = in0 == in1;
    let do_flip = !do_remove && op == BoolOpType::Difference && shape != 0;
    (do_remove, do_flip)
}

fn raycast_tris_boolean(arena: &mut Arena, tm: &[usize], op: BoolOpType, nshapes: usize, shape_fn: &dyn Fn(i32) -> i32) -> Vec<usize> {
    let tree = raycast_tree(arena, tm);
    let mut out_faces = vec![];
    let mut in_shape = vec![0f32; nshapes];
    let mut winding = vec![0i32; nshapes];
    for t in 0..tm.len() {
        let shape = shape_fn(arena.faces[tm[t]].orig);
        test_tri_inside_shapes(arena, tm, shape_fn, nshapes, t, &tree, &mut in_shape);
        for other_shape in 0..nshapes {
            if other_shape as i32 == shape {
                continue;
            }
            let need_high_confidence = (op == BoolOpType::Difference && shape != 0) || op == BoolOpType::Intersect;
            let inside = in_shape[other_shape] >= if need_high_confidence { 0.5f32 } else { 0.1f32 };
            winding[other_shape] = inside as i32;
        }
        let (do_remove, do_flip) = raycast_test_remove(op, &mut winding, shape as usize);
        if !do_remove {
            if !do_flip {
                out_faces.push(tm[t]);
            } else {
                let ff = add_flipped(arena, tm[t]);
                out_faces.push(ff);
            }
        }
    }
    out_faces
}

fn raycast_patches_boolean(arena: &mut Arena, tm: &[usize], op: BoolOpType, nshapes: usize, shape_fn: &dyn Fn(i32) -> i32, pinfo: &PatchesInfo) -> Vec<usize> {
    let tree = raycast_tree(arena, tm);
    let mut out_faces = vec![];
    let mut in_shape = vec![0f32; nshapes];
    let mut winding = vec![0i32; nshapes];
    for p in 0..pinfo.patch.len() {
        let patch = &pinfo.patch[p];
        let test_t_index = patch.tri[patch.tri.len() / 2] as usize;
        let shape = shape_fn(arena.faces[tm[test_t_index]].orig);
        if shape == -1 {
            continue;
        }
        test_tri_inside_shapes(arena, tm, shape_fn, nshapes, test_t_index, &tree, &mut in_shape);
        for other_shape in 0..nshapes {
            if other_shape as i32 == shape {
                continue;
            }
            let need_high_confidence = (op == BoolOpType::Difference && shape != 0) || op == BoolOpType::Intersect;
            let inside = in_shape[other_shape] >= if need_high_confidence { 0.5f32 } else { 0.1f32 };
            winding[other_shape] = inside as i32;
        }
        let (do_remove, do_flip) = raycast_test_remove(op, &mut winding, shape as usize);
        if !do_remove {
            for &t in &patch.tri {
                let f = tm[t as usize];
                if !do_flip {
                    out_faces.push(f);
                } else {
                    let ff = add_flipped(arena, f);
                    out_faces.push(ff);
                }
            }
        }
    }
    out_faces
}

// ---------------------------------------------------------------------------------------------------------------------
// Merging triangles back into polygons
// ---------------------------------------------------------------------------------------------------------------------

fn find_tris_common_edge(tri1: &[u32], tri2: &[u32]) -> (i32, i32) {
    for i in 0..3 {
        for j in 0..3 {
            if tri1[(i + 1) % 3] == tri2[j] && tri1[i] == tri2[(j + 1) % 3] {
                return (i as i32, j as i32);
            }
        }
    }
    (-1, -1)
}

#[derive(Clone)]
struct MergeEdge {
    len_squared: f64,
    v1: u32,
    v2: u32,
    left_face: i32,
    right_face: i32,
    orig: i32,
    dissolvable: bool,
    is_intersect: bool,
}

#[derive(Clone)]
struct MergeFace {
    vert: Vec<u32>,
    edge: Vec<i32>,
    merge_to: i32,
    orig: i32,
}

struct FaceMergeState {
    face: Vec<MergeFace>,
    edge: Vec<MergeEdge>,
    edge_map: HashMap<(u32, u32), i32>,
}

fn init_face_merge_state(arena: &Arena, tris: &[i32], tm: &[usize], norm: &[f64; 3]) -> FaceMergeState {
    let mut fms = FaceMergeState { face: vec![], edge: vec![], edge_map: HashMap::new() };
    for &t in tris {
        let tri = &arena.faces[tm[t as usize]];
        let tn = tri.plane.as_ref().unwrap().norm;
        if dot3d(norm, &tn) <= 0.0 {
            continue;
        }
        let mf = MergeFace { vert: vec![tri.vert[0], tri.vert[1], tri.vert[2]], edge: vec![], merge_to: -1, orig: tri.orig };
        fms.face.push(mf);
        let f = fms.face.len() as i32 - 1;
        for i in 0..3 {
            let inext = (i + 1) % 3;
            let (va, vb) = (tri.vert[i], tri.vert[inext]);
            let (nv1, nv2) = if arena.verts[va as usize].id < arena.verts[vb as usize].id { (va, vb) } else { (vb, va) };
            let canon = (nv1, nv2);
            let mut me_index = fms.edge_map.get(&canon).copied().unwrap_or(-1);
            if me_index == -1 {
                let vec = sub3d(&arena.verts[nv2 as usize].d, &arena.verts[nv1 as usize].d);
                let len_squared = dot3d(&vec, &vec);
                let orig = tri.edge_orig[i];
                let is_intersect = tri.is_intersect[i];
                let dissolvable = orig == NO_INDEX && !is_intersect;
                fms.edge.push(MergeEdge { len_squared, v1: nv1, v2: nv2, left_face: -1, right_face: -1, orig, dissolvable, is_intersect });
                me_index = fms.edge.len() as i32 - 1;
                fms.edge_map.insert(canon, me_index);
            }
            let me = &mut fms.edge[me_index as usize];
            if me.dissolvable && tri.edge_orig[i] != NO_INDEX {
                me.dissolvable = false;
                me.orig = tri.edge_orig[i];
            }
            if me.dissolvable && tri.is_intersect[i] {
                me.dissolvable = false;
                me.is_intersect = true;
            }
            if me.v1 == tri.vert[i] {
                if me.left_face != -1 {
                    me.dissolvable = false;
                } else {
                    me.left_face = f;
                }
            } else if me.right_face != -1 {
                me.dissolvable = false;
            } else {
                me.right_face = f;
            }
            fms.face[f as usize].edge.push(me_index);
        }
    }
    fms
}

fn dissolve_leaves_valid_bmesh(fms: &FaceMergeState, me_index: i32, mf_left: &MergeFace, mf_right: &MergeFace) -> bool {
    let me = &fms.edge[me_index as usize];
    let a_edge_start = mf_left.edge.iter().position(|&e| e == me_index).unwrap();
    let alen = mf_left.vert.len();
    let blen = mf_right.vert.len();
    let b_left_face = me.right_face;
    let mut ok = true;
    let mut a_e_index = (a_edge_start + 1) % alen;
    while ok && a_e_index != a_edge_start {
        let a_me_cur = &fms.edge[mf_left.edge[a_e_index] as usize];
        if a_me_cur.right_face == b_left_face {
            ok = false;
        }
        a_e_index = (a_e_index + 1) % alen;
    }
    let mut a_v_index = 0;
    while ok && a_v_index < alen {
        let a_v = mf_left.vert[a_v_index];
        if a_v != me.v1 && a_v != me.v2 {
            for b_v_index in 0..blen {
                if a_v == mf_right.vert[b_v_index] {
                    ok = false;
                }
            }
        }
        a_v_index += 1;
    }
    ok
}

fn splice_faces(fms: &mut FaceMergeState, me_index: i32, left: usize, right: usize) {
    let mf_left = fms.face[left].clone();
    let mf_right = fms.face[right].clone();
    let a_edge_start = mf_left.edge.iter().position(|&e| e == me_index).unwrap();
    let b_edge_start = mf_right.edge.iter().position(|&e| e == me_index).unwrap();
    let alen = mf_left.vert.len();
    let blen = mf_right.vert.len();
    let me_left_face = fms.edge[me_index as usize].left_face;
    let mut splice_vert = vec![];
    let mut splice_edge = vec![];
    let mut ai = 0;
    while ai < a_edge_start {
        splice_vert.push(mf_left.vert[ai]);
        splice_edge.push(mf_left.edge[ai]);
        ai += 1;
    }
    let mut bi = b_edge_start + 1;
    while bi != b_edge_start {
        if bi >= blen {
            bi = 0;
            if bi == b_edge_start {
                break;
            }
        }
        splice_vert.push(mf_right.vert[bi]);
        splice_edge.push(mf_right.edge[bi]);
        let eidx = mf_right.edge[bi] as usize;
        if mf_right.vert[bi] == fms.edge[eidx].v1 {
            fms.edge[eidx].left_face = me_left_face;
        } else {
            fms.edge[eidx].right_face = me_left_face;
        }
        bi += 1;
    }
    ai = a_edge_start + 1;
    while ai < alen {
        splice_vert.push(mf_left.vert[ai]);
        splice_edge.push(mf_left.edge[ai]);
        ai += 1;
    }
    fms.face[right].merge_to = me_left_face;
    fms.face[left].vert = splice_vert;
    fms.face[left].edge = splice_edge;
    fms.edge[me_index as usize].left_face = -1;
    fms.edge[me_index as usize].right_face = -1;
}

fn do_dissolve(fms: &mut FaceMergeState) {
    let mut dissolve_edges: Vec<usize> = vec![];
    for e in 0..fms.edge.len() {
        if fms.edge[e].dissolvable {
            dissolve_edges.push(e);
        }
    }
    if dissolve_edges.is_empty() {
        return;
    }
    {
        let edges = &fms.edge;
        let less = |a: &usize, b: &usize| edges[*a].len_squared > edges[*b].len_squared;
        match std::env::var("DISSOLVE_SORT").as_deref() {
            Ok("stable") => dissolve_edges.sort_by(|a, b| if less(a, b) { std::cmp::Ordering::Less } else if less(b, a) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }),
            Ok("new") => libcxx_sort::std_sort_new(&mut dissolve_edges, less),
            Ok("gnu") => crate::exbool::hashtab::std_sort(&mut dissolve_edges, less),
            _ => libcxx_sort::std_sort(&mut dissolve_edges, less),
        }
    }
    for me_index in dissolve_edges {
        let (lf, rf) = (fms.edge[me_index].left_face, fms.edge[me_index].right_face);
        if lf == -1 || rf == -1 {
            continue;
        }
        if !dissolve_leaves_valid_bmesh(fms, me_index as i32, &fms.face[lf as usize], &fms.face[rf as usize]) {
            continue;
        }
        splice_faces(fms, me_index as i32, lf as usize, rf as usize);
    }
}

fn merge_tris_for_face(arena: &mut Arena, tris: &[i32], tm: &[usize], imesh_in: &[usize]) -> Vec<usize> {
    let mut ans = vec![];
    if tris.len() <= 1 {
        if tris.len() == 1 {
            ans.push(tm[tris[0] as usize]);
        }
        return ans;
    }
    let mut done = false;
    let first_tri_normal = arena.faces[tm[tris[0] as usize]].plane.as_ref().unwrap().norm;
    let second_tri_normal = arena.faces[tm[tris[1] as usize]].plane.as_ref().unwrap().norm;
    if tris.len() == 2 && dot3d(&first_tri_normal, &second_tri_normal) > 0.0 {
        let tri1 = arena.faces[tm[tris[0] as usize]].clone();
        let tri2 = arena.faces[tm[tris[1] as usize]].clone();
        let in_face = imesh_in[tri1.orig as usize];
        if arena.faces[in_face].size() == 4 {
            let estarts = find_tris_common_edge(&tri1.vert, &tri2.vert);
            if estarts.0 != -1 && tri1.edge_orig[estarts.0 as usize] == NO_INDEX {
                let i0 = estarts.0 as usize;
                let i1 = (i0 + 1) % 3;
                let i2 = (i0 + 2) % 3;
                let j2 = ((estarts.1 + 2) % 3) as usize;
                let tryface = [tri1.vert[i1], tri1.vert[i2], tri1.vert[i0], tri2.vert[j2]];
                if cyclic_equal(&tryface, &arena.faces[in_face].vert) {
                    ans.push(in_face);
                    done = true;
                }
            }
        }
    }
    if done {
        return ans;
    }
    let first_tri_normal_rev = [-first_tri_normal[0], -first_tri_normal[1], -first_tri_normal[2]];
    for norm in [first_tri_normal, first_tri_normal_rev] {
        let mut fms = init_face_merge_state(arena, tris, tm, &norm);
        do_dissolve(&mut fms);
        for mf in &fms.face {
            if mf.merge_to == -1 {
                let e_orig: Vec<i32> = mf.edge.iter().map(|&e| fms.edge[e as usize].orig).collect();
                let is_intersect: Vec<bool> = mf.edge.iter().map(|&e| fms.edge[e as usize].is_intersect).collect();
                let facep = arena.add_face(mf.vert.clone(), mf.orig, e_orig, is_intersect);
                ans.push(facep);
            }
        }
    }
    ans
}

fn approx_in_line(a: &[f64; 3], b: &[f64; 3], c: &[f64; 3]) -> bool {
    let normalize = |v: [f64; 3]| -> [f64; 3] {
        let l2 = dot3d(&v, &v);
        if l2 > 1.0e-70 {
            let l = l2.sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        } else {
            [0.0; 3]
        }
    };
    let vec1 = sub3d(b, a);
    let vec2 = sub3d(c, b);
    let cos_ang = dot3d(&normalize(vec1), &normalize(vec2));
    (cos_ang - 1.0).abs() < 1e-4
}

fn polymesh_from_trimesh_with_dissolve(arena: &mut Arena, tm_out: &[usize], imesh_in: &[usize]) -> Vec<usize> {
    for &f in tm_out {
        arena.populate_plane(f, false);
    }
    let tot_in_face = imesh_in.len();
    let mut face_output_tris: Vec<Vec<i32>> = vec![vec![]; tot_in_face];
    for t in 0..tm_out.len() {
        let in_face = arena.faces[tm_out[t]].orig;
        face_output_tris[in_face as usize].push(t as i32);
    }
    let mut face_output_face: Vec<Vec<usize>> = vec![vec![]; tot_in_face];
    for in_f in 0..tot_in_face {
        face_output_face[in_f] = merge_tris_for_face(arena, &face_output_tris[in_f], tm_out, imesh_in);
    }
    let mut faces: Vec<usize> = vec![];
    for in_f in 0..tot_in_face {
        faces.extend(face_output_face[in_f].iter().copied());
    }
    // find_dissolve_verts
    let (verts, vidx) = populate_vert(arena, &faces);
    let nv = verts.len();
    let mut dissolve: Vec<bool> = verts.iter().map(|&v| arena.verts[v as usize].orig == NO_INDEX).collect();
    let mut neighbors: Vec<(Option<u32>, Option<u32>)> = vec![(None, None); nv];
    for &f in &faces {
        let face = &arena.faces[f];
        let n = face.size();
        for i in 0..n {
            let v = face.vert[i];
            let v_index = vidx[&v];
            if dissolve[v_index] {
                let n1 = face.vert[(i + 1) % n];
                let n2 = face.vert[(i + n - 1) % n];
                let (f_n1, f_n2) = neighbors[v_index];
                if f_n1.is_some() {
                    if !((Some(n1) == f_n2 && Some(n2) == f_n1) || (Some(n1) == f_n1 && Some(n2) == f_n2)) {
                        dissolve[v_index] = false;
                    }
                } else {
                    neighbors[v_index] = (Some(n1), Some(n2));
                }
            }
        }
    }
    let mut count = 0;
    for v_out in 0..nv {
        if dissolve[v_out] {
            dissolve[v_out] = false;
            let nbrs = neighbors[v_out];
            if let (Some(a), Some(b)) = nbrs {
                let v_d = arena.verts[verts[v_out] as usize].d;
                if approx_in_line(&arena.verts[a as usize].d, &v_d, &arena.verts[b as usize].d) {
                    dissolve[v_out] = true;
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        // dissolve_verts
        let mut new_faces: Vec<Option<usize>> = vec![];
        let mut any_erased = false;
        for &f in &faces {
            let vs = arena.faces[f].vert.clone();
            let mut erase = vec![];
            let mut erase_num = 0;
            for &v in &vs {
                let vi = vidx[&v];
                if dissolve[vi] {
                    erase.push(true);
                    erase_num += 1;
                } else {
                    erase.push(false);
                }
            }
            if erase_num > 0 {
                let r = erase_face_positions(arena, f, &erase);
                if r.is_none() {
                    any_erased = true;
                }
                new_faces.push(r);
            } else {
                new_faces.push(Some(f));
            }
        }
        let _ = any_erased;
        faces = new_faces.into_iter().flatten().collect();
    }
    faces
}

/// `boolean_trimesh`
pub fn boolean_trimesh(arena: &mut Arena, tm_in: &[usize], op: BoolOpType, nshapes: usize, shape_fn: &dyn Fn(i32) -> i32, use_self: bool, hole_tolerant: bool) -> Vec<usize> {
    if tm_in.is_empty() {
        return vec![];
    }
    let tm_si = trimesh_nary_intersect(arena, tm_in, nshapes, shape_fn, use_self);
    if tm_si.is_empty() || op == BoolOpType::None {
        return tm_si;
    }
    let tm: Vec<usize> = tm_si.clone();
    let orig_of: Vec<i32> = tm.iter().map(|&f| arena.faces[f].orig).collect();
    let si_shape_fn = |t: i32| shape_fn(orig_of[t as usize]);
    let topo = TriMeshTopology::new(arena, &tm);
    let pwn = is_pwn(arena, &tm, &topo);
    let tm_out: Vec<usize>;
    if !pwn {
        if hole_tolerant {
            tm_out = raycast_tris_boolean(arena, &tm, op, nshapes, shape_fn);
        } else {
            let pinfo = find_patches(arena, &tm, &topo);
            tm_out = raycast_patches_boolean(arena, &tm, op, nshapes, shape_fn, &pinfo);
        }
    } else {
        let mut pinfo = find_patches(arena, &tm, &topo);
        let mut cells = find_cells(arena, &tm, &topo, &mut pinfo);
        finish_patch_cell_graph(arena, &tm, &mut cells, &mut pinfo, &topo);
        if !patch_cell_graph_ok(&cells, &pinfo) {
            return tm_in.to_vec();
        }
        let c_ambient = find_ambient_cell(arena, &tm, None, &topo, &pinfo);
        if c_ambient == NO_INDEX {
            return tm;
        }
        propagate_windings_and_in_output_volume(&pinfo, &mut cells, c_ambient, op, nshapes, &si_shape_fn);
        tm_out = extract_from_in_output_volume_diffs(arena, &tm, &pinfo, &cells);
    }
    tm_out
}

/// `boolean_mesh`: returns the output polygon mesh faces (arena face indices).
pub fn boolean_mesh(arena: &mut Arena, imesh: &[usize], op: BoolOpType, nshapes: usize, shape_fn: &dyn Fn(i32) -> i32, use_self: bool, hole_tolerant: bool) -> Vec<usize> {
    let tm_in = triangulate_polymesh(arena, imesh);
    let tm_out = boolean_trimesh(arena, &tm_in, op, nshapes, shape_fn, use_self, hole_tolerant);
    polymesh_from_trimesh_with_dissolve(arena, &tm_out, imesh)
}

#[allow(dead_code)]
fn _unused() {
    let _ = Q::zero();
}
