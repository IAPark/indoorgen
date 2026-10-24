//! Blender 4.2 geometry-node mesh ops needed for exact topology: Merge by Distance (mode ALL) and Subdivide Mesh (simple, OpenSubdiv).
//! Ports of geometry/intern/mesh_merge_by_distance.cc and blenkernel/intern/subdiv_foreach.cc (+ subdiv_mesh.cc vertex layout).
use super::cabnodes::GMesh;
use crate::dhash::HashMap;

const OOC: i32 = -1;
const COLLAPSED: i32 = -2;

#[derive(Clone, Copy, Default)]
struct WeldEdge {
    edge_orig: i32,
    vert_a: i32,
    vert_b: i32,
}
#[derive(Clone, Copy, Default)]
struct WeldLoop {
    // flag aliases `edge` in the C union: COLLAPSED stored in `edge` field slot via `flag`
    edge: i32,
    vert: i32,
    loop_orig: i32,
    loop_next: i32,
    collapsed: bool,
}
#[derive(Clone, Copy, Default)]
struct WeldPoly {
    poly_dst: i32,
    poly_orig: i32,
    loop_start: i32,
    loop_end: i32,
    loop_ctx_start: i32,
    loop_ctx_len: i32,
    collapsed: bool,
}

struct WeldMesh {
    edge_dest_map: Vec<i32>,
    wloop: Vec<WeldLoop>,
    wpoly: Vec<WeldPoly>,
    wpoly_new_len: usize,
    loop_map: Vec<i32>,
    face_map: Vec<i32>,
    face_kill_len: usize,
    loop_kill_len: usize,
}

fn len_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    let mut s = 0.0f32;
    for k in 0..3 {
        let d = a[k] - b[k];
        s += d * d;
    }
    s
}

/// corner (edge index) for every face corner: the edge between corner c and the next corner
fn corner_edges(m: &GMesh) -> Vec<Vec<i32>> {
    let mut map: HashMap<(u32, u32), i32> = HashMap::default();
    for (i, e) in m.e.iter().enumerate() {
        map.insert((e[0].min(e[1]), e[0].max(e[1])), i as i32);
    }
    m.p.iter().map(|f| (0..f.len()).map(|c| { let (a, b) = (f[c], f[(c + 1) % f.len()]); *map.get(&(a.min(b), a.max(b))).expect("edge") }).collect()).collect()
}

/// Merge by Distance, mode ALL, all vertices selected.
pub fn merge_by_distance(m: &GMesh, dist: f32) -> GMesh {
    let n = m.v.len();
    let range_sq = dist * dist;
    let mut dup = vec![OOC; n];
    let mut found = 0;
    for i in 0..n {
        if dup[i] == OOC || dup[i] == i as i32 {
            let fp = found;
            for j in 0..n {
                if j != i && dup[j] == OOC && len_sq(m.v[j], m.v[i]) <= range_sq {
                    dup[j] = i as i32;
                    found += 1;
                }
            }
            if found != fp {
                dup[i] = i as i32;
            }
        }
    }
    if found == 0 {
        return m.clone();
    }
    let vert_kill_len = found;
    let vert_dest_map = dup;
    let edges = &m.e;
    let ce = corner_edges(m);
    // ---- edges
    let mut edge_dest_map = vec![OOC; edges.len()];
    let mut wedge: Vec<WeldEdge> = vec![];
    let mut edge_collapsed_len = 0;
    for (i, e) in edges.iter().enumerate() {
        let (d1, d2) = (vert_dest_map[e[0] as usize], vert_dest_map[e[1] as usize]);
        if d1 == OOC && d2 == OOC {
            edge_dest_map[i] = OOC;
            continue;
        }
        let a = if d1 == OOC { e[0] as i32 } else { d1 };
        let b = if d2 == OOC { e[1] as i32 } else { d2 };
        if a == b {
            edge_dest_map[i] = COLLAPSED;
            edge_collapsed_len += 1;
        } else {
            wedge.push(WeldEdge { edge_orig: i as i32, vert_a: a, vert_b: b });
            edge_dest_map[i] = i as i32;
        }
    }
    let mut edge_double_kill_len = 0;
    if !wedge.is_empty() {
        let mut v_links = vec![0i32; n + 1];
        for we in &wedge {
            v_links[we.vert_a as usize] += 1;
            v_links[we.vert_b as usize] += 1;
        }
        let mut link_len = 0;
        for i in 0..n {
            link_len += v_links[i];
            v_links[i] = link_len;
        }
        v_links[n] = link_len;
        let mut buf = vec![0i32; link_len as usize];
        for i in (0..wedge.len()).rev() {
            let we = wedge[i];
            v_links[we.vert_a as usize] -= 1;
            buf[v_links[we.vert_a as usize] as usize] = i as i32;
            v_links[we.vert_b as usize] -= 1;
            buf[v_links[we.vert_b as usize] as usize] = i as i32;
        }
        for i in 0..wedge.len() {
            let we = wedge[i];
            if edge_dest_map[we.edge_orig as usize] != we.edge_orig {
                continue;
            }
            let (da, db) = (we.vert_a as usize, we.vert_b as usize);
            let (link_a, link_b) = (v_links[da] as usize, v_links[db] as usize);
            let mut len_a = (v_links[da + 1] - v_links[da]) as usize;
            let mut len_b = (v_links[db + 1] - v_links[db]) as usize;
            let edge_orig = we.edge_orig;
            if len_a <= 1 || len_b <= 1 {
                edge_dest_map[edge_orig as usize] = OOC;
                continue;
            }
            let (mut ia, mut ib) = (link_a, link_b);
            let prev = edge_double_kill_len;
            while len_a > 0 {
                len_a -= 1;
                let e_a = buf[ia];
                ia += 1;
                if e_a == i as i32 {
                    continue;
                }
                while len_b > 0 && buf[ib] < e_a {
                    ib += 1;
                    len_b -= 1;
                }
                if len_b == 0 {
                    break;
                }
                let e_b = buf[ib];
                if e_a == e_b {
                    let we_b = wedge[e_b as usize];
                    edge_dest_map[we_b.edge_orig as usize] = edge_orig;
                    edge_double_kill_len += 1;
                }
            }
            if prev == edge_double_kill_len {
                edge_dest_map[edge_orig as usize] = OOC;
            }
        }
    }
    let edge_kill_len = edge_collapsed_len + edge_double_kill_len;
    // ---- polys / loops
    let nfaces = m.p.len();
    let mut starts = vec![0usize; nfaces + 1];
    for i in 0..nfaces {
        starts[i + 1] = starts[i] + m.p[i].len();
    }
    let corner_verts: Vec<i32> = m.p.iter().flat_map(|f| f.iter().map(|&x| x as i32)).collect();
    let corner_edges: Vec<i32> = ce.iter().flat_map(|f| f.iter().copied()).collect();
    let mut loop_map = vec![OOC; corner_verts.len()];
    let mut face_map = vec![OOC; nfaces];
    let mut wloop: Vec<WeldLoop> = vec![];
    let mut wpoly: Vec<WeldPoly> = vec![];
    for i in 0..nfaces {
        let loopstart = starts[i];
        let totloop = m.p[i].len();
        let loop_end = loopstart + totloop - 1;
        let v_first = corner_verts[loopstart];
        let v_dest_first = vert_dest_map[v_first as usize];
        let is_first = v_dest_first != OOC;
        let (mut v_next, mut v_dest_next, mut is_next) = (v_first, v_dest_first, is_first);
        let prev_len = wloop.len();
        for loop_orig in loopstart..=loop_end {
            let (v, v_dest, is_ctx) = (v_next, v_dest_next, is_next);
            let loop_next;
            if loop_orig != loop_end {
                loop_next = loop_orig + 1;
                v_next = corner_verts[loop_next];
                v_dest_next = vert_dest_map[v_next as usize];
                is_next = v_dest_next != OOC;
            } else {
                loop_next = loopstart;
                v_next = v_first;
                v_dest_next = v_dest_first;
                is_next = is_first;
            }
            if is_ctx || is_next {
                let e = corner_edges[loop_orig];
                let e_dest = edge_dest_map[e as usize];
                let is_edge_ctx = e_dest != OOC;
                wloop.push(WeldLoop { vert: if is_ctx { v_dest } else { v }, edge: if is_edge_ctx { e_dest } else { e }, loop_orig: loop_orig as i32, loop_next: loop_next as i32, collapsed: false });
                loop_map[loop_orig] = (wloop.len() - 1) as i32;
            } else {
                loop_map[loop_orig] = OOC;
            }
        }
        if wloop.len() != prev_len {
            let loop_ctx_len = (wloop.len() - prev_len) as i32;
            wpoly.push(WeldPoly { poly_dst: OOC, poly_orig: i as i32, loop_start: loopstart as i32, loop_end: loop_end as i32, loop_ctx_start: prev_len as i32, loop_ctx_len, collapsed: false });
            face_map[i] = (wpoly.len() - 1) as i32;
        }
    }
    let mut wm = WeldMesh { edge_dest_map: edge_dest_map.clone(), wloop, wpoly, wpoly_new_len: 0, loop_map, face_map, face_kill_len: 0, loop_kill_len: 0 };
    let remain_edge_ctx_len = wedge.len() as i64 - edge_double_kill_len as i64;
    setup_collapsed_and_split(remain_edge_ctx_len, &vert_dest_map, &mut wm);
    poly_find_doubles_w(&corner_verts, &corner_edges, edges.len(), &mut wm);
    // ---- result
    let result_nverts = n - vert_kill_len;
    // vertex final map + averaged positions
    let mut vert_final = vec![0i32; n];
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::default();
    for i in 0..n {
        if vert_dest_map[i] >= 0 {
            groups.entry(vert_dest_map[i] as usize).or_default().push(i);
        }
    }
    let mut out = GMesh::new();
    let mut dest_index = 0i32;
    for i in 0..n {
        if vert_dest_map[i] == OOC {
            vert_final[i] = dest_index;
            out.v.push(m.v[i]);
            dest_index += 1;
        } else if vert_dest_map[i] == i as i32 {
            let g = &groups[&i];
            let w = 1.0f32 / g.len() as f32;
            let mut r = [0f32; 3];
            for &j in g {
                for k in 0..3 {
                    r[k] += m.v[j][k] * w;
                }
            }
            vert_final[i] = dest_index;
            out.v.push(r);
            dest_index += 1;
        }
    }
    for i in 0..n {
        let d = vert_dest_map[i];
        if d >= 0 && d != i as i32 {
            vert_final[i] = vert_final[d as usize];
        }
    }
    assert_eq!(out.v.len(), result_nverts);
    // edges
    let mut edge_final = vec![0i32; edges.len()];
    let mut ec = 0i32;
    let mut new_edges: Vec<[u32; 2]> = vec![];
    for i in 0..edges.len() {
        let d = edge_dest_map[i];
        if d == OOC || d == i as i32 {
            edge_final[i] = ec;
            ec += 1;
            new_edges.push([vert_final[edges[i][0] as usize] as u32, vert_final[edges[i][1] as usize] as u32]);
        }
    }
    for i in 0..edges.len() {
        let d = edge_dest_map[i];
        if d >= 0 && d != i as i32 {
            edge_final[i] = edge_final[d as usize];
        }
    }
    let _ = edge_kill_len;
    out.e = new_edges;
    // faces
    for i in 0..nfaces {
        let pc = wm.face_map[i];
        if pc == OOC {
            out.p.push(m.p[i].iter().map(|&x| vert_final[x as usize] as u32).collect());
            out.tag.push(m.tag[i]);
        } else {
            let wp = wm.wpoly[pc as usize];
            if let Some(f) = iter_poly(&wp, &wm, &corner_verts, &corner_edges) {
                if wp.poly_dst != OOC {
                    continue;
                }
                out.p.push(f.iter().map(|&(v, _)| vert_final[v as usize] as u32).collect());
                out.tag.push(m.tag[i]);
            }
        }
    }
    let new_start = wm.wpoly.len() - wm.wpoly_new_len;
    for k in new_start..wm.wpoly.len() {
        let wp = wm.wpoly[k];
        if let Some(f) = iter_poly(&wp, &wm, &corner_verts, &corner_edges) {
            if wp.poly_dst != OOC {
                continue;
            }
            out.p.push(f.iter().map(|&(v, _)| vert_final[v as usize] as u32).collect());
            out.tag.push(m.tag[wp.poly_orig as usize]);
        }
    }
    out
}

/// WeldLoopOfPolyIter: list of (vert, edge) for the surviving loops of a weld poly (None when collapsed)
fn iter_poly(wp: &WeldPoly, wm: &WeldMesh, corner_verts: &[i32], corner_edges: &[i32]) -> Option<Vec<(i32, i32)>> {
    if wp.collapsed {
        return None;
    }
    let mut out = vec![];
    let mut l = wp.loop_start;
    loop {
        if l > wp.loop_end {
            break;
        }
        let lc = wm.loop_map[l as usize];
        let mut l_next = l + 1;
        if lc != OOC {
            let wl = wm.wloop[lc as usize];
            out.push((wl.vert, wl.edge));
            if wl.loop_next > l {
                l_next = wl.loop_next;
            }
        } else {
            out.push((corner_verts[l as usize], corner_edges[l as usize]));
        }
        l = l_next;
    }
    Some(out)
}

fn setup_collapsed_and_split(remain_edge_ctx_len: i64, vert_dest_map: &[i32], wm: &mut WeldMesh) {
    if remain_edge_ctx_len == 0 {
        wm.face_kill_len = wm.wpoly.len();
        wm.loop_kill_len = wm.wloop.len();
        for wp in wm.wpoly.iter_mut() {
            wp.collapsed = true;
        }
        return;
    }
    let mut face_kill_len = 0usize;
    let mut loop_kill_len = 0usize;
    let orig_n = wm.wpoly.len();
    for i in 0..orig_n {
        let mut wp = wm.wpoly[i];
        let mut poly_loop_len = (wp.loop_end - wp.loop_start) + 1;
        let mut wl_prev: Option<usize> = None;
        let mut chang_loop_start = false;
        let mut l = wp.loop_start;
        loop {
            let loop_ctx = wm.loop_map[l as usize];
            if loop_ctx == OOC {
                wl_prev = None;
            } else {
                let lc = loop_ctx as usize;
                let edge_dest = wm.wloop[lc].edge;
                if edge_dest == COLLAPSED {
                    wm.wloop[lc].collapsed = true;
                    if poly_loop_len == 3 {
                        wp.collapsed = true;
                        face_kill_len += 1;
                        loop_kill_len += 3;
                        poly_loop_len = 0;
                        break;
                    }
                    if l == wp.loop_start {
                        chang_loop_start = true;
                    }
                    loop_kill_len += 1;
                    poly_loop_len -= 1;
                } else {
                    if chang_loop_start {
                        wp.loop_start = l;
                        chang_loop_start = false;
                    }
                    if let Some(p) = wl_prev {
                        wm.wloop[p].loop_next = l;
                    }
                    wl_prev = Some(lc);
                }
            }
            let cont = l != wp.loop_end;
            l += 1;
            if !cont {
                break;
            }
        }
        if poly_loop_len != 0 {
            if let Some(p) = wl_prev {
                wm.wloop[p].loop_next = wp.loop_start;
                wp.loop_end = wm.wloop[p].loop_orig;
            }
            wm.wpoly[i] = wp;
            let mut wpc = wp;
            split_recursive(poly_loop_len, vert_dest_map, &mut wpc, wm, &mut face_kill_len, &mut loop_kill_len);
            wm.wpoly[i] = wpc;
        } else {
            wm.wpoly[i] = wp;
        }
    }
    wm.face_kill_len = face_kill_len;
    wm.loop_kill_len = loop_kill_len;
}

fn split_recursive(mut poly_loop_len: i32, vert_dest_map: &[i32], wp: &mut WeldPoly, wm: &mut WeldMesh, poly_kill: &mut usize, r_loop_kill: &mut usize) {
    if poly_loop_len < 3 {
        return;
    }
    let mut loop_kill = 0usize;
    let mut loop_end = wp.loop_end;
    let loop_ctx_a0 = wm.loop_map[loop_end as usize];
    let mut wla_prev: Option<usize> = if loop_ctx_a0 != OOC { Some(loop_ctx_a0 as usize) } else { None };
    let mut la = wp.loop_start;
    loop {
        let loop_ctx_a = wm.loop_map[la as usize];
        if loop_ctx_a == OOC {
            la += 1;
            wla_prev = None;
            if la == loop_end {
                break;
            }
            continue;
        }
        let mut wla = loop_ctx_a as usize;
        let vert_a = wm.wloop[wla].vert;
        if vert_dest_map[vert_a as usize] == OOC {
            la = wm.wloop[wla].loop_next;
            wla_prev = Some(wla);
            // `continue` of the do-while re-tests the loop condition
            if la == loop_end {
                break;
            }
            continue;
        }
        let mut dist_a = 1;
        let mut lb_prev = la;
        let mut wlb_prev: Option<usize> = Some(wla);
        let mut lb = wm.wloop[wla].loop_next;
        loop {
            let loop_ctx_b = wm.loop_map[lb as usize];
            if loop_ctx_b == OOC {
                dist_a += 1;
                lb_prev = lb;
                wlb_prev = None;
                lb += 1;
                // do-while: condition is `lb_prev != loop_end`
                if lb_prev == loop_end {
                    break;
                }
                continue;
            }
            let wlb = loop_ctx_b as usize;
            let vert_b = wm.wloop[wlb].vert;
            if vert_a != vert_b {
                dist_a += 1;
                lb_prev = lb;
                wlb_prev = Some(wlb);
                lb = wm.wloop[wlb].loop_next;
                if lb_prev == loop_end {
                    break;
                }
                continue;
            }
            let mut dist_b = poly_loop_len - dist_a;
            if dist_a == 1 || dist_b == 1 {
                // nothing
            } else if dist_a == 2 && dist_b == 2 {
                loop_kill += 4;
                dist_b = 0;
                let _ = dist_b;
                wp.collapsed = true;
                *poly_kill += 1;
                *r_loop_kill += loop_kill;
                return;
            } else {
                let wp_prev_a = wla_prev.unwrap();
                let wpb = wlb_prev.unwrap();
                wm.wloop[wp_prev_a].loop_next = lb;
                wm.wloop[wpb].loop_next = la;
                if wp.loop_start == la {
                    wp.loop_start = lb;
                }
                if dist_a == 2 {
                    wm.wloop[wla].collapsed = true;
                    wm.wloop[wpb].collapsed = true;
                    loop_kill += 2;
                } else if dist_b == 2 {
                    wm.wloop[wlb].collapsed = true;
                    wm.wloop[wp_prev_a].collapsed = true;
                    loop_kill += 2;
                    wp.loop_start = la;
                    wp.loop_end = lb_prev;
                    loop_end = lb_prev;
                    poly_loop_len = dist_a;
                    break;
                } else {
                    wm.wpoly.push(WeldPoly { poly_dst: OOC, poly_orig: wp.poly_orig, loop_start: la, loop_end: lb_prev, loop_ctx_start: wp.loop_ctx_start, loop_ctx_len: wp.loop_ctx_len, collapsed: false });
                    wm.wpoly_new_len += 1;
                    let li = wm.wpoly.len() - 1;
                    let mut nt = wm.wpoly[li];
                    split_recursive(dist_a, vert_dest_map, &mut nt, wm, poly_kill, r_loop_kill);
                    wm.wpoly[li] = nt;
                }
                la = lb;
                wla = wlb;
                poly_loop_len = dist_b;
                dist_a = 1;
            }
            wlb_prev = Some(wlb);
            lb_prev = lb;
            lb = wm.wloop[wlb].loop_next;
            if lb_prev == loop_end {
                break;
            }
        }
        wla_prev = Some(wla);
        if la == loop_end {
            break;
        }
        la = wm.wloop[wla].loop_next;
        if la == loop_end {
            break;
        }
    }
    let _ = loop_end;
    *r_loop_kill += loop_kill;
}

fn poly_find_doubles_w(corner_verts: &[i32], corner_edges: &[i32], medge_len: usize, wm: &mut WeldMesh) {
    if wm.face_kill_len == wm.wpoly.len() {
        return;
    }
    let face_len = wm.wpoly.len();
    let mut poly_offs: Vec<usize> = vec![0; face_len + 1];
    let mut new_corner_edges: Vec<i32> = vec![];
    for (fi, wp) in wm.wpoly.clone().iter().enumerate() {
        poly_offs[fi] = new_corner_edges.len();
        let it = match iter_poly(wp, wm, corner_verts, corner_edges) {
            Some(x) => x,
            None => continue,
        };
        if wp.poly_dst != OOC {
            continue;
        }
        for (_, e) in it {
            new_corner_edges.push(e);
        }
    }
    poly_offs[face_len] = new_corner_edges.len();
    // poly_find_doubles
    let range = |i: usize| poly_offs[i]..poly_offs[i + 1];
    let mut linked_off = vec![0i32; medge_len + 1];
    for &e in &new_corner_edges {
        linked_off[e as usize] += 1;
    }
    let mut tot = 0;
    for i in 0..medge_len {
        tot += linked_off[i];
        linked_off[i] = tot;
    }
    linked_off[medge_len] = tot;
    if tot == 0 {
        return;
    }
    let mut linked = vec![0i32; tot as usize];
    for fi in (0..face_len).rev() {
        let r = range(fi);
        if r.is_empty() {
            continue;
        }
        for ci in r.rev() {
            let e = new_corner_edges[ci] as usize;
            linked_off[e] -= 1;
            linked[linked_off[e] as usize] = fi as i32;
        }
    }
    let mut doubles_buffer = vec![0i32; face_len];
    let mut doubles_offsets: Vec<usize> = vec![0];
    let mut is_double = vec![false; face_len];
    let mut doubles_buffer_num = 0usize;
    let mut doubles_num;
    let intersect = |a: &[i32], b: &[i32], is_double: &[bool], out: &mut Vec<i32>| -> usize {
        out.clear();
        let (mut ia, mut ib) = (0, 0);
        while ia < a.len() && ib < b.len() {
            let (va, vb) = (a[ia], b[ib]);
            if va < vb {
                ia += 1;
            } else if vb < va {
                ib += 1;
            } else {
                if !is_double[va as usize] {
                    out.push(va);
                }
                ia += 1;
                ib += 1;
            }
        }
        out.len()
    };
    for fi in 0..face_len {
        if is_double[fi] {
            continue;
        }
        let r = range(fi);
        let corner_num = r.len();
        if corner_num == 0 {
            continue;
        }
        doubles_buffer[doubles_buffer_num] = fi as i32;
        let corner_first = r.start;
        let e0 = new_corner_edges[corner_first] as usize;
        let link_offs = linked_off[e0] as usize;
        let faces_a_num = (linked_off[e0 + 1] - linked_off[e0]) as usize;
        if faces_a_num == 1 {
            continue;
        }
        let mut faces_a: Vec<i32> = linked[link_offs..link_offs + faces_a_num].to_vec();
        // skip polygons with lower index
        let pos = faces_a.iter().position(|&x| x == fi as i32).unwrap();
        faces_a = faces_a[pos + 1..].to_vec();
        doubles_num = 0;
        let mut isect: Vec<i32> = vec![];
        for ci in corner_first + 1..corner_first + corner_num {
            let e = new_corner_edges[ci] as usize;
            let lo = linked_off[e] as usize;
            let nb = (linked_off[e + 1] - linked_off[e]) as usize;
            let b_all = &linked[lo..lo + nb];
            let pos = b_all.iter().position(|&x| x == fi as i32).unwrap();
            let faces_b = &b_all[pos + 1..];
            doubles_num = intersect(&faces_a, faces_b, &is_double, &mut isect);
            if doubles_num == 0 {
                break;
            }
            faces_a = isect.clone();
        }
        if doubles_num != 0 {
            // isect holds the final result (copied into the buffer after the group head)
            for (k, &pd) in faces_a.iter().enumerate().take(doubles_num) {
                doubles_buffer[doubles_buffer_num + 1 + k] = pd;
                is_double[pd as usize] = true;
            }
            doubles_buffer_num += doubles_num;
            doubles_buffer_num += 1;
            doubles_offsets.push(doubles_buffer_num);
            if doubles_buffer_num + 1 == face_len {
                break;
            }
        }
    }
    let n_groups = doubles_offsets.len() - 1;
    let doubles_total = doubles_buffer_num as i64 - n_groups as i64;
    if doubles_total > 0 {
        let mut loop_kill_num = 0usize;
        for g in 0..n_groups {
            let (s, e) = (doubles_offsets[g], doubles_offsets[g + 1]);
            let poly_dst = wm.wpoly[doubles_buffer[s] as usize].poly_orig;
            for off in s + 1..e {
                let wi = doubles_buffer[off] as usize;
                wm.wpoly[wi].poly_dst = poly_dst;
                loop_kill_num += range(wi).len();
            }
        }
        wm.face_kill_len += doubles_total as usize;
        wm.loop_kill_len += loop_kill_num;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Subdivide Mesh (simple subdivision, subdiv_to_mesh): vertex layout [coarse verts][coarse-edge verts][per-face inner verts]
// ---------------------------------------------------------------------------------------------------------------

fn osd_mode() -> u32 {
    std::env::var("OSDV").ok().and_then(|s| s.parse().ok()).unwrap_or(9)
}

/// OpenSubdiv bilinear (uniform level 1, simple scheme) limit evaluation of a ptex face with control corners a,b,c,d at (u,v):
/// level-1 points via stencils (edge: 0.5/0.5, face: 1/n each), patch selection + PatchParam normalisation, linear basis, sequential accumulation.
fn osd_quad(cv: [[f32; 3]; 4], mid: [[f32; 3]; 4], ctr: [f32; 3], u: f32, v: f32) -> [f32; 3] {
    let mode = osd_mode();
    // quadrant of the base quad
    let qx = if u >= 0.5 { 1 } else { 0 };
    let qy = if v >= 0.5 { 1 } else { 0 };
    let s = u * 2.0 - qx as f32;
    let t = v * 2.0 - qy as f32;
    let pts: [[f32; 3]; 4] = match (qx, qy) {
        (0, 0) => [cv[0], mid[0], ctr, mid[3]],
        (1, 0) => [mid[0], cv[1], mid[1], ctr],
        (1, 1) => [ctr, mid[1], cv[2], mid[2]],
        _ => [mid[3], ctr, mid[2], cv[3]],
    };
    bilin(pts, s, t, mode)
}

fn bilin(pts: [[f32; 3]; 4], s: f32, t: f32, mode: u32) -> [f32; 3] {
    let sc = 1.0f32 - s;
    let tc = 1.0f32 - t;
    let w = [sc * tc, s * tc, s * t, sc * t];
    let mut r = [0f32; 3];
    for j in 0..4 {
        for k in 0..3 {
            r[k] = if mode & 1 == 1 { pts[j][k].mul_add(w[j], r[k]) } else { r[k] + pts[j][k] * w[j] };
        }
    }
    r
}

pub fn subdivide(m: &GMesh, level: u32) -> GMesh {
    subdivide_ev(m, level, None)
}

/// `ev(face, ptex_face, u, v)` replaces the simple-mode bilinear evaluation (used for the Catmull-Clark limit surface)
pub fn subdivide_ev(m: &GMesh, level: u32, ev: Option<&dyn Fn(usize, usize, f32, f32) -> [f32; 3]>) -> GMesh {
    if level == 0 {
        return m.clone();
    }
    let res = (1usize << level) + 1;
    let nv = m.v.len();
    let ne = m.e.len();
    let ce = corner_edges(m);
    let res2 = res - 2;
    let nq_res = (res >> 1) + 1;
    let edge_off = nv;
    let inner_off = nv + ne * res2;
    // per-face inner offsets
    let mut vtx_off = vec![0usize; m.p.len()];
    let mut acc = 0usize;
    for (fi, f) in m.p.iter().enumerate() {
        vtx_off[fi] = acc;
        if f.len() == 4 {
            acc += res2 * res2;
        } else {
            acc += 1 + f.len() * ((nq_res - 2) * (nq_res - 1));
        }
    }
    let total = inner_off + acc;
    let mut pos = vec![[0f32; 3]; total];
    for i in 0..nv {
        pos[i] = m.v[i];
    }
    if let Some(e) = ev {
        let mut done = vec![false; nv];
        for (fi, f) in m.p.iter().enumerate() {
            for (c, &vi) in f.iter().enumerate() {
                if !done[vi as usize] && f.len() == 4 {
                    done[vi as usize] = true;
                    let (u, v) = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)][c];
                    pos[vi as usize] = e(fi, 0, u, v);
                }
            }
        }
    }
    // ptex evaluation
    let center_of = |f: &Vec<u32>| -> [f32; 3] {
        let mut s = [0f32; 3];
        for &i in f {
            for k in 0..3 {
                s[k] += m.v[i as usize][k];
            }
        }
        let inv = 1.0f32 / f.len() as f32;
        [s[0] * inv, s[1] * inv, s[2] * inv]
    };
    let mode = osd_mode();
    let eval = |fi: usize, ptex: usize, u: f32, v: f32| -> [f32; 3] {
        if let Some(e) = ev {
            return e(fi, ptex, u, v);
        }
        let f = &m.p[fi];
        let n = f.len();
        let cvs: Vec<[f32; 3]> = f.iter().map(|&i| m.v[i as usize]).collect();
        let half = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] {
            let mut r = [0f32; 3];
            for k in 0..3 {
                r[k] = if mode & 2 == 2 { (0.0f32 + b[k] * 0.5) + a[k] * 0.5 } else { (0.0f32 + a[k] * 0.5) + b[k] * 0.5 };
            }
            r
        };
        let w = 1.0f32 / n as f32;
        let mut ctr = [0f32; 3];
        let order: Vec<usize> = if mode & 4 == 4 {
            let mut idx: Vec<usize> = (0..n).collect();
            idx.sort_by_key(|&i| f[i]);
            idx
        } else {
            (0..n).collect()
        };
        for &i in &order {
            for k in 0..3 {
                ctr[k] = if mode & 8 == 8 { cvs[i][k].mul_add(w, ctr[k]) } else { ctr[k] + cvs[i][k] * w };
            }
        }
        if n == 4 {
            let mid = [half(cvs[0], cvs[1]), half(cvs[1], cvs[2]), half(cvs[2], cvs[3]), half(cvs[3], cvs[0])];
            osd_quad([cvs[0], cvs[1], cvs[2], cvs[3]], mid, ctr, u, v)
        } else {
            let a = cvs[ptex];
            let nx = half(cvs[ptex], cvs[(ptex + 1) % n]);
            let pv = half(cvs[(ptex + n - 1) % n], cvs[ptex]);
            bilin([a, nx, ctr, pv], u, v, mode)
        }
    };
    let mut edge_used = vec![false; ne];
    let inv_res1 = 1.0f32 / (res - 1) as f32;
    for (fi, f) in m.p.iter().enumerate() {
        let n = f.len();
        if n == 4 {
            for corner in 0..4 {
                let coarse_vert = f[corner];
                let ei = ce[fi][corner] as usize;
                if edge_used[ei] {
                    continue;
                }
                edge_used[ei] = true;
                let flip = m.e[ei][1] == coarse_vert;
                let base = edge_off + ei * res2;
                for vi in 0..res2 {
                    let mut fac = (vi + 1) as f32 * inv_res1;
                    if flip {
                        fac = 1.0 - fac;
                    }
                    if corner >= 2 {
                        fac = 1.0 - fac;
                    }
                    let (u, v) = if corner & 1 == 0 { (fac, if corner == 2 { 1.0 } else { 0.0 }) } else { (if corner == 1 { 1.0 } else { 0.0 }, fac) };
                    pos[base + vi] = eval(fi, 0, u, v);
                }
            }
        } else {
            let nvp = (res >> 1) + 1; // vertices per ptex edge
            let inv_p1 = 1.0f32 / (nvp - 1) as f32;
            for corner in 0..n {
                let coarse_vert = f[corner];
                let ei = ce[fi][corner] as usize;
                if edge_used[ei] {
                    continue;
                }
                edge_used[ei] = true;
                let flip = m.e[ei][1] == coarse_vert;
                let mut idx = (edge_off + ei * res2) as i64;
                let mut delta = 1i64;
                if flip {
                    idx += res2 as i64 - 1;
                    delta = -1;
                }
                for vi in 1..nvp {
                    let u = vi as f32 * inv_p1;
                    pos[idx as usize] = eval(fi, corner, u, 0.0);
                    idx += delta;
                }
                let next_corner = (corner + 1) % n;
                for vi in 1..nvp - 1 {
                    let v = 1.0 - vi as f32 * inv_p1;
                    pos[idx as usize] = eval(fi, next_corner, 0.0, v);
                    idx += delta;
                }
            }
        }
        // inner vertices
        let start = inner_off + vtx_off[fi];
        if n == 4 {
            let mut idx = start;
            for y in 1..res - 1 {
                let v = y as f32 * inv_res1;
                for x in 1..res - 1 {
                    let u = x as f32 * inv_res1;
                    pos[idx] = eval(fi, 0, u, v);
                    idx += 1;
                }
            }
        } else {
            let ptex_res = nq_res;
            let inv_p = 1.0f32 / (ptex_res - 1) as f32;
            let mut idx = start;
            pos[idx] = eval(fi, 0, 1.0, 1.0);
            idx += 1;
            for corner in 0..n {
                for y in 1..ptex_res - 1 {
                    let v = y as f32 * inv_p;
                    for x in 1..ptex_res {
                        let u = x as f32 * inv_p;
                        pos[idx] = eval(fi, corner, u, v);
                        idx += 1;
                    }
                }
            }
        }
    }
    // ---- faces
    let mut out = GMesh::new();
    out.v = pos;
    out.e = subdivide_edges(m, level).0;
    let rot = |r: usize, a: i64, b: i64, c: i64, d: i64| -> [i64; 4] {
        let vals = [a, b, c, d];
        [vals[(4 - r) % 4], vals[(1 + 4 - r) % 4], vals[(2 + 4 - r) % 4], vals[(3 + 4 - r) % 4]]
    };
    let mut emit = |out: &mut GMesh, r: usize, a: i64, b: i64, c: i64, d: i64, tag: u8| {
        let q = rot(r, a, b, c, d);
        out.p.push(q.iter().map(|&x| x as u32).collect());
        out.tag.push(tag);
    };
    for (fi, f) in m.p.iter().enumerate() {
        let n = f.len();
        let tag = m.tag[fi];
        let start_vertex_index = (inner_off + vtx_off[fi]) as i64;
        if n == 4 {
            let ptex_res = res as i64;
            let inner_res = ptex_res - 2;
            for y in 1..ptex_res - 2 {
                for x in 1..ptex_res - 2 {
                    let (iy, ix) = (y - 1, x - 1);
                    let v0 = start_vertex_index + iy * inner_res + ix;
                    emit(&mut out, 0, v0, v0 + 1, v0 + inner_res + 1, v0 + inner_res, tag);
                }
            }
            let mut prev = n - 1;
            for corner in 0..n {
                let coarse_vert = f[corner];
                let ei = ce[fi][corner] as usize;
                let prev_vert = f[prev];
                let pei = ce[fi][prev] as usize;
                let coarse_edge = m.e[ei];
                let prev_edge = m.e[pei];
                let start_edge_vertex = (edge_off + ei * res2) as i64;
                let flip = coarse_edge[1] == coarse_vert;
                let mut side_start = start_vertex_index;
                let side_stride: i64;
                let mut v0 = coarse_vert as i64;
                let mut v3 = if prev_vert == prev_edge[0] { (edge_off + pei * res2) as i64 + res2 as i64 - 1 } else { (edge_off + pei * res2) as i64 };
                let r2 = res2 as i64;
                match corner {
                    0 => side_stride = 1,
                    1 => {
                        side_start += r2 - 1;
                        side_stride = r2;
                    }
                    2 => {
                        side_start += r2 * r2 - 1;
                        side_stride = -1;
                    }
                    _ => {
                        side_start += r2 * (r2 - 1);
                        side_stride = -r2;
                    }
                }
                for i in 0..res2 as i64 {
                    let v1 = if flip { start_edge_vertex + (res as i64 - i - 3) } else { start_edge_vertex + i };
                    let v2 = side_start + side_stride * i;
                    emit(&mut out, corner, v0, v1, v2, v3, tag);
                    v0 = v1;
                    v3 = v2;
                }
                prev = corner;
            }
        } else {
            let ptex_res = nq_res as i64;
            let inner_res = ptex_res - 2;
            let nipp = (ptex_res - 1) * (ptex_res - 2);
            let center = start_vertex_index;
            let start_vertex = center + 1;
            let nn = n as i64;
            for corner in 0..n as i64 {
                let corner_vertex_index = start_vertex + corner * nipp;
                for y in 1..inner_res {
                    for x in 1..inner_res + 1 {
                        let (iy, ix) = (y - 1, x - 1);
                        let v0 = corner_vertex_index + iy * (inner_res + 1) + ix;
                        emit(&mut out, 0, v0, v0 + 1, v0 + inner_res + 2, v0 + inner_res + 1, tag);
                    }
                }
            }
            // connections between ptex faces
            for corner in 0..nn {
                let next_corner = (corner + 1) % nn;
                let mut cur = start_vertex + corner * nipp + inner_res;
                let mut nxt = start_vertex + next_corner * nipp + nipp - ptex_res + 1;
                let mut v0 = cur;
                let mut v1 = nxt;
                cur += inner_res + 1;
                nxt += 1;
                for _row in 1..inner_res {
                    let v2 = nxt;
                    let v3 = cur;
                    emit(&mut out, 3, v0, v1, v2, v3, tag);
                    cur += inner_res + 1;
                    nxt += 1;
                    v0 = v3;
                    v1 = v2;
                }
            }
            // loops from center
            if ptex_res >= 3 {
                let mut prev_corner = nn - 1;
                for corner in 0..nn {
                    let cur_end = start_vertex + corner * nipp + nipp - 1;
                    let prev_end = start_vertex + prev_corner * nipp + nipp - 1;
                    emit(&mut out, 2, center, prev_end, cur_end - 1, cur_end, tag);
                    prev_corner = corner;
                }
            }
            // connecting inner ptex part with boundary
            let mut prev_corner = nn - 1;
            for corner in 0..nn {
                let coarse_vert = f[corner as usize];
                let ei = ce[fi][corner as usize] as usize;
                let prev_vert = f[prev_corner as usize];
                let pei = ce[fi][prev_corner as usize] as usize;
                let coarse_edge = m.e[ei];
                let prev_edge = m.e[pei];
                let flip = coarse_edge[1] == coarse_vert;
                let start_edge_vertex = (edge_off + ei * res2) as i64;
                let corner_vertex_index = start_vertex + corner * nipp;
                let mut v0 = coarse_vert as i64;
                let mut v3 = if prev_vert == prev_edge[0] { (edge_off + pei * res2) as i64 + res2 as i64 - 1 } else { (edge_off + pei * res2) as i64 };
                for i in 0..=inner_res {
                    let v1 = if flip { start_edge_vertex + (res as i64 - i - 3) } else { start_edge_vertex + i };
                    let v2 = if inner_res >= 1 { corner_vertex_index + i } else { center };
                    emit(&mut out, 0, v0, v1, v2, v3, tag);
                    v0 = v1;
                    v3 = v2;
                }
                let flip_prev = prev_edge[1] == coarse_vert;
                v0 = corner_vertex_index;
                v3 = if prev_vert == prev_edge[0] { (edge_off + pei * res2) as i64 + res2 as i64 - 1 } else { (edge_off + pei * res2) as i64 };
                for i in 0..=inner_res - 1 {
                    let v1 = if i == inner_res - 1 { start_vertex + prev_corner * nipp + inner_res } else { v0 + inner_res + 1 };
                    let v2 = if flip_prev { v3 - 1 } else { v3 + 1 };
                    emit(&mut out, 1, v0, v1, v2, v3, tag);
                    v0 = v1;
                    v3 = v2;
                }
                prev_corner = corner;
            }
        }
    }
    out
}


/// Edges of the subdivided mesh in Blender's order (subdiv_foreach_* in subdiv_foreach.cc), with the coarse edge
/// each one originates from (-1 for edges created inside faces).
pub fn subdivide_edges(m: &GMesh, level: u32) -> (Vec<[u32; 2]>, Vec<i32>) {
    let res = (1usize << level) + 1;
    let nv = m.v.len();
    let ne = m.e.len();
    let ce = corner_edges(m);
    let res2 = res - 2;
    let pres = (res >> 1) + 1;
    let edge_off = nv;
    let inner_off = nv + ne * res2;
    let mut vtx_off = vec![0usize; m.p.len()];
    let mut acc = 0usize;
    for (fi, f) in m.p.iter().enumerate() {
        vtx_off[fi] = acc;
        acc += if f.len() == 4 { res2 * res2 } else { 1 + f.len() * ((pres - 2) * (pres - 1)) };
    }
    let mut edges: Vec<[u32; 2]> = vec![];
    let mut src: Vec<i32> = vec![];
    // boundary edges: res-1 pieces per coarse edge
    for (ei, e) in m.e.iter().enumerate() {
        let mut last = e[0] as usize;
        for i in 0..res - 2 {
            let v2 = edge_off + ei * res2 + i;
            edges.push([last as u32, v2 as u32]);
            src.push(ei as i32);
            last = v2;
        }
        edges.push([last as u32, e[1]]);
        src.push(ei as i32);
    }
    let mut row = |edges: &mut Vec<[u32; 2]>, src: &mut Vec<i32>, start: usize, n: usize| {
        let mut v = start;
        for _ in 0..n.saturating_sub(1) {
            edges.push([v as u32, v as u32 + 1]);
            src.push(-1);
            v += 1;
        }
    };
    let column = |edges: &mut Vec<[u32; 2]>, src: &mut Vec<i32>, start: usize, n: usize| {
        let mut v = start;
        for _ in 0..n {
            edges.push([v as u32, (v + n) as u32]);
            src.push(-1);
            v += 1;
        }
    };
    for (fi, f) in m.p.iter().enumerate() {
        let n = f.len();
        let start_vertex = inner_off + vtx_off[fi];
        if n == 4 {
            row(&mut edges, &mut src, start_vertex, res - 2);
            for r in 0..(res as i64 - 3).max(0) as usize {
                let srv = start_vertex + r * (res - 2);
                column(&mut edges, &mut src, srv, res - 2);
                row(&mut edges, &mut src, srv + res - 2, res - 2);
            }
            for corner in 0..n {
                let cv = f[corner];
                let ei = ce[fi][corner] as usize;
                let ed = m.e[ei];
                let start_edge_vertex = edge_off + ei * res2;
                let flip = ed[1] == cv;
                let mut side_start = start_vertex as i64;
                let stride: i64;
                match corner {
                    0 => stride = 1,
                    1 => {
                        side_start += res as i64 - 3;
                        stride = res as i64 - 2;
                    }
                    2 => {
                        side_start += (res2 * res2) as i64 - 1;
                        stride = -1;
                    }
                    _ => {
                        side_start += (res2 * (res2.max(1) - 1)) as i64;
                        stride = -(res as i64 - 2);
                    }
                }
                for i in 0..res - 2 {
                    let v1 = if flip { start_edge_vertex + (res - i - 3) } else { start_edge_vertex + i };
                    let v2 = side_start + stride * i as i64;
                    edges.push([v1 as u32, v2 as u32]);
                    src.push(-1);
                }
            }
        } else {
            let pinner = pres - 2;
            let nipp = (pres - 1) * (pres - 2);
            let center = start_vertex;
            let start = center + 1;
            for corner in 0..n {
                let sp = start + corner * nipp;
                row(&mut edges, &mut src, sp, pinner + 1);
                for r in 0..pinner.saturating_sub(1) {
                    let srv = sp + r * (pinner + 1);
                    column(&mut edges, &mut src, srv, pinner + 1);
                    row(&mut edges, &mut src, srv + pinner + 1, pinner + 1);
                }
            }
            for corner in 0..n {
                let next = (corner + 1) % n;
                let mut cur = start + corner * nipp + pinner;
                let mut nxt = start + next * nipp + nipp - pres + 1;
                for _ in 0..pinner {
                    edges.push([cur as u32, nxt as u32]);
                    src.push(-1);
                    cur += pinner + 1;
                    nxt += 1;
                }
            }
            if pres >= 3 {
                for corner in 0..n {
                    let end = start + corner * nipp + nipp - 1;
                    edges.push([center as u32, end as u32]);
                    src.push(-1);
                }
            }
            let mut prev_corner = n - 1;
            for corner in 0..n {
                let cv = f[corner];
                let ei = ce[fi][corner] as usize;
                let pei = ce[fi][prev_corner] as usize;
                {
                    let ed = m.e[ei];
                    let start_edge_vertex = edge_off + ei * res2;
                    let flip = ed[1] == cv;
                    let side_start = if pres >= 3 { start + nipp * corner } else { center };
                    for i in 0..pres - 1 {
                        let v1 = if flip { start_edge_vertex + (res - i - 3) } else { start_edge_vertex + i };
                        edges.push([v1 as u32, (side_start + i) as u32]);
                        src.push(-1);
                    }
                }
                if pres >= 3 {
                    let ed = m.e[pei];
                    let start_edge_vertex = edge_off + pei * res2;
                    let flip = ed[1] == cv;
                    let side_start = start + nipp * corner;
                    for i in 0..pres - 2 {
                        let v1 = if flip { start_edge_vertex + (res - i - 3) } else { start_edge_vertex + i };
                        edges.push([v1 as u32, (side_start + (pinner + 1) * i) as u32]);
                        src.push(-1);
                    }
                }
                prev_corner = corner;
            }
        }
    }
    (edges, src)
}
