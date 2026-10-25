//! bpy.ops.mesh.convex_hull on a BMesh (all elements selected): bmo_convex_hull_exec (bmo_hull.cc) on top of the Bullet hull port,
//! then edbm_convex_hull_exec's delete_unused and bmo_join_triangles_exec (angle limits 40 deg).
use super::*;
use crate::bm::qsort::qsort;
use crate::bm::query as q;

const INPUT: u8 = 1;
const INTERIOR: u8 = 2;
const OUTPUT: u8 = 4;
const DEL: u8 = 8;
const HOLE: u8 = 16;

struct Fl(Vec<u8>);
impl Fl {
    fn get(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }
    fn on(&mut self, i: usize, f: u8) {
        if i >= self.0.len() {
            self.0.resize(i + 1, 0);
        }
        self.0[i] |= f;
    }
    fn off(&mut self, i: usize, f: u8) {
        if i < self.0.len() {
            self.0[i] &= !f;
        }
    }
    fn has(&self, i: usize, f: u8) -> bool {
        self.get(i) & f != 0
    }
}

fn normal_tri(v1: V3, v2: V3, v3: V3) -> V3 {
    let n1 = q::sub(v1, v2);
    let n2 = q::sub(v2, v3);
    q::normalize(q::cross(n1, n2))
}
fn area_tri(v1: V3, v2: V3, v3: V3) -> f32 {
    let n1 = q::sub(v1, v2);
    let n2 = q::sub(v2, v3);
    q::len(q::cross(n1, n2)) * 0.5
}

#[derive(Clone, Copy)]
pub struct HullParams {
    pub use_existing_faces: bool,
    pub delete_unused: bool,
    pub make_holes: bool,
    pub join_triangles: bool,
    pub face_threshold: f32,
    pub shape_threshold: f32,
}
impl Default for HullParams {
    fn default() -> Self {
        let d = 40.0f32 * ((std::f64::consts::PI / 180.0) as f32);
        HullParams { use_existing_faces: true, delete_unused: true, make_holes: false, join_triangles: true, face_threshold: d, shape_threshold: d }
    }
}

/// Runs the operator with every vertex/edge/face of `bm` as input. Returns false if the op cancels (fewer than 3 vertices).
pub fn convex_hull(bm: &mut BMesh, p: &HullParams) -> bool {
    let in_verts = bm.vert_ids();
    if in_verts.len() < 3 {
        return false;
    }
    let in_edges = bm.edge_ids();
    let in_faces = bm.face_ids();
    let mut fv = Fl(vec![]);
    let mut fe = Fl(vec![]);
    let mut ff = Fl(vec![]);
    for &v in &in_verts {
        fv.on(v, INPUT | INTERIOR);
    }
    for &e in &in_edges {
        fe.on(e, INPUT);
    }
    for &f in &in_faces {
        ff.on(f, INPUT);
    }

    // hull_from_bullet
    let coords: Vec<V3> = in_verts.iter().map(|&v| bm.verts[v].co).collect();
    let h = crate::bm::bullet_hull::convex_hull_compute(&coords);
    let hull_verts: Vec<usize> = h.original_vertex_index.iter().map(|&i| in_verts[i as usize]).collect();
    // face vertex indices (walk next_edge_of_face; vertex = source of the edge)
    let mut tris: Vec<([usize; 3], bool)> = vec![];
    for &f in &h.faces {
        let e0 = f as usize;
        let mut e = e0;
        let mut fvi = vec![];
        loop {
            fvi.push(h.edges[e].target_vertex as usize);
            e = h.edges[e].next_edge_of_face as usize;
            if e == e0 {
                break;
            }
        }
        if fvi.len() > 2 {
            for j in 2..fvi.len() {
                let t = [hull_verts[fvi[0]], hull_verts[fvi[j - 1]], hull_verts[fvi[j]]];
                for &v in &t {
                    fv.off(v, INTERIOR);
                }
                tris.push((t, false));
            }
        }
    }

    // hull_final_edges
    let mut final_edges: crate::dhash::HashSet<(usize, usize)> = crate::dhash::HashSet::default();
    for (t, _) in &tris {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            final_edges.insert((a.min(b), a.max(b)));
        }
    }
    let on_hull = |a: usize, b: usize| final_edges.contains(&(a.min(b), a.max(b)));

    // hull_mark_interior_elements
    for &e in &in_edges {
        if !on_hull(bm.edges[e].v1, bm.edges[e].v2) {
            fe.on(e, INTERIOR);
        }
    }
    for &f in &in_faces {
        ff.on(f, INTERIOR);
    }

    if p.use_existing_faces {
        // hull_remove_overlapping
        for ti in 0..tris.len() {
            let t = tris[ti].0;
            for f in bm.faces_of_vert(t[0]) {
                let mut f_on_hull = true;
                for e in bm.edges_of_face(f) {
                    if !on_hull(bm.edges[e].v1, bm.edges[e].v2) {
                        f_on_hull = false;
                        break;
                    }
                }
                if bm.vert_in_face(t[1], f) && bm.vert_in_face(t[2], f) && f_on_hull {
                    tris[ti].1 = true;
                    ff.off(f, INTERIOR);
                    ff.on(f, HOLE);
                }
            }
        }
        // hull_tag_holes
        for &f in &in_faces {
            if ff.has(f, HOLE) {
                for e in bm.edges_of_face(f) {
                    if bm.edge_face_count(e) == 1 {
                        ff.off(f, HOLE);
                        break;
                    }
                }
            }
        }
        for &e in &in_edges {
            let mut hole = true;
            let mut any = false;
            for f in bm.faces_of_edge(e) {
                any = true;
                if !ff.has(f, HOLE) {
                    hole = false;
                    break;
                }
            }
            if hole && any {
                fe.on(e, HOLE);
            }
        }
    }

    // hull_output_triangles
    for (t, skip) in &tris {
        if !*skip {
            let edges = [
                bm.edge_create(t[0], t[1], None, true),
                bm.edge_create(t[1], t[2], None, true),
                bm.edge_create(t[2], t[0], None, true),
            ];
            let mut f = bm.face_exists(t);
            if f != NONE {
                ff.on(f, OUTPUT);
                ff.off(f, HOLE);
                ff.off(f, INTERIOR);
            } else {
                let mut example = None;
                for &e in &edges {
                    if example.is_none() {
                        for fx in bm.faces_of_edge(e) {
                            if ff.has(fx, INPUT) || !ff.has(fx, OUTPUT) {
                                example = Some(fx);
                                break;
                            }
                        }
                    }
                }
                f = bm.face_create_verts(t, example, true);
            }
            ff.on(f, OUTPUT);
            bm.faces[f].flag |= ELEM_SELECT;
            for &e in &edges {
                fe.on(e, OUTPUT);
            }
        } else {
            for i in 0..3 {
                let e = bm.edge_exists(t[i], t[(i + 1) % 3]);
                if e != NONE && fe.has(e, INPUT) && !fe.has(e, HOLE) {
                    fe.on(e, OUTPUT);
                }
            }
        }
        for &v in t {
            fv.on(v, OUTPUT);
        }
    }

    // hull_tag_unused
    for &v in &in_verts {
        if fv.has(v, INTERIOR) {
            let mut del = true;
            for e in bm.edges_of_vert(v) {
                if !fe.has(e, INPUT) {
                    del = false;
                    break;
                }
            }
            for f in bm.faces_of_vert(v) {
                if !ff.has(f, INPUT) {
                    del = false;
                    break;
                }
            }
            if del {
                fv.on(v, DEL);
            }
        }
    }
    for &e in &in_edges {
        if fe.has(e, INTERIOR) {
            let mut del = true;
            for f in bm.faces_of_edge(e) {
                if !ff.has(f, INPUT) {
                    del = false;
                    break;
                }
            }
            if del {
                fe.on(e, DEL);
            }
        }
    }
    for &f in &in_faces {
        if ff.has(f, INTERIOR) {
            ff.on(f, DEL);
        }
    }
    bm.normals_update();

    // geom.out snapshot (verts/edges/faces flagged OUTPUT) for the join op
    let out_faces: Vec<usize> = bm.face_ids().into_iter().filter(|&f| ff.has(f, OUTPUT)).collect();

    // delete geom_unused (DEL_ONLYTAGGED: faces, edges, verts in pool order)
    if p.delete_unused {
        for f in bm.face_ids() {
            if ff.has(f, DEL) {
                bm.face_kill(f);
            }
        }
        for e in bm.edge_ids() {
            if fe.has(e, DEL) {
                bm.edge_kill(e);
            }
        }
        for v in bm.vert_ids() {
            if fv.has(v, DEL) {
                bm.vert_kill(v);
            }
        }
        bm.normals_update();
    }
    if p.make_holes {
        for f in bm.face_ids() {
            if ff.has(f, HOLE) {
                bm.face_kill(f);
            }
        }
        for e in bm.edge_ids() {
            if fe.has(e, HOLE) {
                bm.edge_kill(e);
            }
        }
    }
    if p.join_triangles {
        let faces: Vec<usize> = out_faces.into_iter().filter(|&f| bm.faces[f].alive).collect();
        join_triangles(bm, &faces, p.face_threshold, p.shape_threshold);
        bm.normals_update();
    }
    true
}

fn quad_calc_error(v1: V3, v2: V3, v3: V3, v4: V3) -> f32 {
    use std::f32::consts::PI;
    let mut error = 0.0f32;
    {
        let n1 = normal_tri(v1, v2, v3);
        let n2 = normal_tri(v1, v3, v4);
        let angle_a = if q::compare_v3v3(n1, n2, f32::EPSILON) { 0.0 } else { q::angle_normalized_v3v3(n1, n2) };
        let n1 = normal_tri(v2, v3, v4);
        let n2 = normal_tri(v4, v1, v2);
        let angle_b = if q::compare_v3v3(n1, n2, f32::EPSILON) { 0.0 } else { q::angle_normalized_v3v3(n1, n2) };
        error += (angle_a + angle_b) / (PI * 2.0);
    }
    {
        let e0 = q::normalize(q::sub(v1, v2));
        let e1 = q::normalize(q::sub(v2, v3));
        let e2 = q::normalize(q::sub(v3, v4));
        let e3 = q::normalize(q::sub(v4, v1));
        let h = std::f32::consts::FRAC_PI_2;
        let diff = ((q::angle_normalized_v3v3(e0, e1) - h).abs()
            + (q::angle_normalized_v3v3(e1, e2) - h).abs()
            + (q::angle_normalized_v3v3(e2, e3) - h).abs()
            + (q::angle_normalized_v3v3(e3, e0) - h).abs())
            / (PI * 2.0);
        error += diff;
    }
    {
        let area_a = area_tri(v1, v2, v3) + area_tri(v1, v3, v4);
        let area_b = area_tri(v2, v3, v4) + area_tri(v4, v1, v2);
        let (mn, mx) = (area_a.min(area_b), area_a.max(area_b));
        error += if mx != 0.0 { 1.0 - mn / mx } else { 1.0 };
    }
    error
}

fn edge_quad_verts(bm: &BMesh, e: usize) -> [usize; 4] {
    let l = bm.edges[e].l;
    let lr = bm.loops[l].rn;
    [bm.loops[l].v, bm.loops[bm.loops[l].p].v, bm.loops[bm.loops[l].n].v, bm.loops[bm.loops[lr].p].v]
}

/// bmo_join_triangles_exec (no uv/vcol/seam/sharp/material delimiting)
pub fn join_triangles(bm: &mut BMesh, faces: &[usize], angle_face: f32, angle_shape: f32) {
    let deg180 = 180.0f32 * ((std::f64::consts::PI / 180.0) as f32);
    let do_face = angle_face < deg180;
    let cos_face = angle_face.cos();
    let do_shape = angle_shape < deg180;
    let mut input = Fl(vec![]);
    for &f in faces {
        if bm.faces[f].len == 3 {
            input.on(f, 1);
        }
    }
    let mut jedges: Vec<(usize, f32)> = vec![];
    let mut marked = vec![];
    for e in bm.edge_ids() {
        if let Some((fa, fb)) = bm.edge_face_pair(e) {
            if input.has(fa, 1) && input.has(fb, 1) {
                let mut delimit = false;
                if do_face && q::dot(bm.faces[fa].no, bm.faces[fb].no) < cos_face {
                    delimit = true;
                }
                if !delimit && do_shape {
                    let vs = edge_quad_verts(bm, e);
                    let c = vs.map(|v| bm.verts[v].co);
                    if crate::assets::polyfill::is_quad_flip_v3(c[0], c[1], c[2], c[3]) != 0 {
                        delimit = true;
                    } else {
                        let ev = [
                            q::normalize(q::sub(c[0], c[1])),
                            q::normalize(q::sub(c[1], c[2])),
                            q::normalize(q::sub(c[2], c[3])),
                            q::normalize(q::sub(c[3], c[0])),
                        ];
                        let h = std::f32::consts::FRAC_PI_2;
                        for i in 0..4 {
                            if (q::angle_normalized_v3v3(ev[i], ev[(i + 1) % 4]) - h).abs() > angle_shape {
                                delimit = true;
                            }
                        }
                    }
                }
                if !delimit {
                    marked.push(e);
                }
            }
        }
    }
    if marked.is_empty() {
        return;
    }
    for &e in &marked {
        let vs = edge_quad_verts(bm, e);
        let c = vs.map(|v| bm.verts[v].co);
        jedges.push((e, quad_calc_error(c[0], c[1], c[2], c[3])));
    }
    qsort(&mut jedges, &|a: &(usize, f32), b: &(usize, f32)| if a.1 < b.1 { -1 } else if a.1 > b.1 { 1 } else { 0 });
    if std::env::var("JTDBG").is_ok() {
        for &(e, w) in &jedges {
            eprintln!("JT {} {} {:e} {:08x}", bm.edges[e].v1, bm.edges[e].v2, w, w.to_bits());
        }
    }
    for &(e, _) in &jedges {
        let l_a = bm.edges[e].l;
        let l_b = bm.loops[l_a].rn;
        if bm.faces[bm.loops[l_a].f].len == 3 && bm.faces[bm.loops[l_b].f].len == 3 {
            faces_join_pair(bm, l_a, l_b);
        }
    }
}

/// BM_faces_join_pair(l_a, l_b, do_del = true)
pub fn faces_join_pair(bm: &mut BMesh, l_a: usize, l_b: usize) -> usize {
    if bm.loops[l_a].v == bm.loops[l_b].v {
        let f = bm.loops[l_b].f;
        bm.loop_reverse(f);
    }
    let faces = [bm.loops[l_a].f, bm.loops[l_b].f];
    faces_join(bm, &faces)
}

fn radial_flag_count(bm: &BMesh, l: usize, fl: &Fl) -> usize {
    let mut it = l;
    let mut i = 0;
    loop {
        if fl.has(bm.loops[it].f, 1) {
            i += 1;
        }
        it = bm.loops[it].rn;
        if it == l {
            break;
        }
    }
    i
}
fn vert_is_manifold_flagged(bm: &BMesh, v: usize, fl: &Fl) -> bool {
    let e0 = bm.verts[v].e;
    if e0 == NONE {
        return false;
    }
    let mut e = e0;
    loop {
        let l0 = bm.edges[e].l;
        if l0 == NONE {
            return false;
        }
        if bm.edge_face_count(bm.loops[l0].e) == 1 {
            return false;
        }
        let mut l = l0;
        loop {
            if !fl.has(bm.loops[l].f, 1) {
                return false;
            }
            l = bm.loops[l].rn;
            if l == l0 {
                break;
            }
        }
        e = bm.disk_edge_next(e, v);
        if e == e0 {
            break;
        }
    }
    true
}

/// bm_edges_sort_winding
fn edges_sort_winding(bm: &BMesh, v1: usize, v2: usize, edges: &[usize]) -> Option<(Vec<usize>, Vec<usize>)> {
    let len = edges.len();
    let mut mf = Fl(vec![]);
    let mut mv = Fl(vec![]);
    for &e in edges {
        mf.on(e, 1);
        mv.on(bm.edges[e].v1, 1);
        mv.on(bm.edges[e].v2, 1);
    }
    let mut e_iter = bm.verts[v1].e;
    let mut e_first = e_iter;
    let mut v_iter = v1;
    let mut found = false;
    loop {
        if mf.has(e_iter, 1) && bm.edge_other_vert(e_iter, v_iter) == v2 {
            found = true;
            break;
        }
        e_iter = bm.disk_edge_next(e_iter, v_iter);
        if e_iter == e_first {
            break;
        }
    }
    if !found {
        return None;
    }
    let mut es = vec![];
    let mut vs = vec![];
    loop {
        if mf.has(e_iter, 1) {
            if !mv.has(v_iter, 1) {
                return None;
            }
            mf.off(e_iter, 1);
            es.push(e_iter);
            mv.off(v_iter, 1);
            vs.push(v_iter);
            v_iter = bm.edge_other_vert(e_iter, v_iter);
            if es.len() == len {
                if v_iter != vs[0] {
                    return None;
                }
                break;
            }
            e_first = e_iter;
        }
        e_iter = bm.disk_edge_next(e_iter, v_iter);
        if e_iter == e_first {
            break;
        }
    }
    if es.len() == len {
        Some((es, vs))
    } else {
        None
    }
}

/// BM_faces_join(faces, do_del = true)
pub fn faces_join(bm: &mut BMesh, faces: &[usize]) -> usize {
    let mut tag = Fl(vec![]);
    for &f in faces {
        tag.on(f, 1);
    }
    let mut etag = Fl(vec![]);
    let mut vtag = Fl(vec![]);
    let mut edges = vec![];
    let mut deledges = vec![];
    let mut delverts = vec![];
    let (mut v1, mut v2) = (NONE, NONE);
    for &f in faces {
        for l in bm.loops_of_face(f) {
            let rlen = radial_flag_count(bm, l, &tag);
            let e = bm.loops[l].e;
            if rlen > 2 {
                return NONE;
            } else if rlen == 1 {
                edges.push(e);
                if v1 == NONE {
                    v1 = bm.loops[l].v;
                    v2 = bm.edge_other_vert(e, v1);
                }
            } else if rlen == 2 {
                let (ev1, ev2) = (bm.edges[e].v1, bm.edges[e].v2);
                let d1 = vert_is_manifold_flagged(bm, ev1, &tag);
                let d2 = vert_is_manifold_flagged(bm, ev2, &tag);
                if !d1 && !d2 && !etag.has(e, 1) {
                    if bm.edge_face_count(e) <= 2 {
                        deledges.push(e);
                        etag.on(e, 1);
                    }
                } else {
                    if d1 && !vtag.has(ev1, 1) {
                        delverts.push(ev1);
                        vtag.on(ev1, 1);
                    }
                    if d2 && !vtag.has(ev2, 1) {
                        delverts.push(ev2);
                        vtag.on(ev2, 1);
                    }
                }
            }
        }
    }
    if edges.is_empty() {
        return NONE;
    }
    let (es, vs) = match edges_sort_winding(bm, v1, v2, &edges) {
        Some(x) => x,
        None => return NONE,
    };
    let f_new = bm.face_create(&vs, &es, Some(faces[0]), false);
    for e in deledges {
        bm.edge_kill(e);
    }
    for v in delverts {
        bm.vert_kill(v);
    }
    f_new
}

/// bpy.ops.mesh.poke() on every face (center_mode MEDIAN_WEIGHTED, offset 0)
pub fn poke_all(bm: &mut BMesh) {
    for f in bm.face_ids() {
        let ls = bm.loops_of_face(f);
        let k = ls.len();
        // BM_face_calc_center_median_weighted
        let mut cent = [0.0f32; 3];
        let mut totw = 0.0f32;
        let prev_l = ls[k - 1];
        let mut w_prev = bm.edge_calc_length(bm.loops[prev_l].e);
        for &l in &ls {
            let w_curr = bm.edge_calc_length(bm.loops[l].e);
            let w = w_curr + w_prev;
            cent = q::madd(cent, bm.verts[bm.loops[l].v].co, w);
            totw += w;
            w_prev = w_curr;
        }
        if totw != 0.0 {
            cent = q::mul(cent, 1.0 / totw);
        }
        let vc = bm.vert_create(cent, None);
        for i in 0..k {
            let v1 = bm.loops[ls[i]].v;
            let v2 = bm.loops[ls[(i + 1) % k]].v;
            bm.face_create_verts(&[v1, v2, vc], Some(f), true);
        }
        bm.face_kill(f);
    }
    bm.normals_update();
}

/// bpy.ops.mesh.tris_convert_to_quads() with all faces selected (default 40 degree limits)
pub fn tris_to_quads_all(bm: &mut BMesh) {
    let d = HullParams::default();
    let faces = bm.face_ids();
    join_triangles(bm, &faces, d.face_threshold, d.shape_threshold);
    bm.normals_update();
}
