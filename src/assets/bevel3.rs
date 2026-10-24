//! Blender BEVEL modifier (edges, angle limit, circular profile) for meshes whose beveled vertices have exactly three beveled
//! edges (box corners; valence 3, or valence 4 with one coplanar unbeveled edge): bmesh_bevel.cc BoundVerts, superellipse profiles,
//! cube-corner / adj vmesh (cubic_subdiv) corner patches. Ported from the Blender 4.2 source; surface-equal only.
use crate::mesh::Mesh;
use crate::dhash::HashMap;
use std::f64::consts::PI;

type V3 = [f64; 3];
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        a
    }
}

fn face_normal(m: &Mesh, f: &[u32]) -> V3 {
    let mut n = [0.0; 3];
    for i in 0..f.len() {
        let (a, b) = (m.pos[f[i] as usize], m.pos[f[(i + 1) % f.len()] as usize]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    unit(n)
}

// ---------------------------------------------------------------- vmesh
#[derive(Clone)]
struct VM {
    n: usize,
    ns: usize,
    d: Vec<V3>,
}
impl VM {
    fn new(n: usize, ns: usize) -> VM {
        VM { n, ns, d: vec![[0.0; 3]; n * (ns / 2 + 1) * (ns + 1)] }
    }
    fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (i * (self.ns / 2 + 1) + j) * (self.ns + 1) + k
    }
    fn get(&self, i: usize, j: usize, k: usize) -> V3 {
        self.d[self.idx(i, j, k)]
    }
    fn set(&mut self, i: usize, j: usize, k: usize, v: V3) {
        let ix = self.idx(i, j, k);
        self.d[ix] = v;
    }
    fn canon(&self, i: usize, j: usize, k: usize) -> (usize, usize, usize) {
        let (n, ns) = (self.n, self.ns);
        let ns2 = ns / 2;
        let odd = ns % 2;
        if odd == 0 && j == ns2 && k == ns2 {
            return (0, j, k);
        }
        if j + 1 <= ns2 + odd && k <= ns2 {
            return (i, j, k);
        }
        if k <= ns2 {
            return ((i + n - 1) % n, k, ns - j);
        }
        ((i + 1) % n, ns - k, j)
    }
    fn is_canon(&self, i: usize, j: usize, k: usize) -> bool {
        let ns2 = self.ns / 2;
        if self.ns % 2 == 1 {
            return j <= ns2 && k <= ns2;
        }
        (j < ns2 && k <= ns2) || (j == ns2 && k == ns2 && i == 0)
    }
    fn getc(&self, i: usize, j: usize, k: usize) -> V3 {
        let (a, b, c) = self.canon(i, j, k);
        self.get(a, b, c)
    }
    fn setc(&mut self, i: usize, j: usize, k: usize, v: V3) {
        let (a, b, c) = self.canon(i, j, k);
        self.set(a, b, c, v);
    }
    fn copy_equiv(&mut self) {
        let ns2 = self.ns / 2;
        for i in 0..self.n {
            for j in 0..=ns2 {
                for k in 0..=self.ns {
                    if self.is_canon(i, j, k) {
                        continue;
                    }
                    let v = self.getc(i, j, k);
                    self.set(i, j, k, v);
                }
            }
        }
    }
}

fn avg4(a: V3, b: V3, c: V3, d: V3) -> V3 {
    mul(add(add(a, b), add(c, d)), 0.25)
}

fn sabin_gamma(n: usize) -> f64 {
    match n {
        3 => 0.065247584,
        4 => 0.25,
        5 => 0.401983447,
        6 => 0.523423277,
        _ => 0.0,
    }
}

/// bmesh_bevel.cc cubic_subdiv; `prof(i, k, ns)` = get_profile_point of boundary vert i
fn cubic_subdiv(vin: &mut VM, prof: &dyn Fn(usize, usize, usize) -> V3) -> VM {
    let n = vin.n;
    let ns_in = vin.ns;
    let ns_in2 = ns_in / 2;
    let ns_out = 2 * ns_in;
    let mut out = VM::new(n, ns_out);
    for i in 0..n {
        out.set(i, 0, 0, vin.get(i, 0, 0));
        for k in 1..ns_in {
            let co = vin.get(i, 0, k);
            let (co1, co2) = (vin.get(i, 0, k - 1), vin.get(i, 0, k + 1));
            let acc = sub(add(co1, co2), mul(co, 2.0));
            out.setc(i, 0, 2 * k, add(co, mul(acc, -1.0 / 6.0)));
        }
    }
    for i in 0..n {
        let mut k = 1;
        while k < ns_out {
            let co = prof(i, k, ns_out);
            let (co1, co2) = (out.getc(i, 0, k - 1), out.getc(i, 0, k + 1));
            let acc = sub(add(co1, co2), mul(co, 2.0));
            out.setc(i, 0, k, add(co, mul(acc, -1.0 / 6.0)));
            k += 2;
        }
    }
    out.copy_equiv();
    for i in 0..n {
        for k in 0..ns_in {
            let v = out.get(i, 0, 2 * k);
            vin.set(i, 0, k, v);
        }
    }
    vin.copy_equiv();
    for i in 0..n {
        for j in 0..ns_in2 {
            for k in 0..ns_in2 {
                let c = avg4(vin.get(i, j, k), vin.get(i, j, k + 1), vin.get(i, j + 1, k), vin.get(i, j + 1, k + 1));
                out.set(i, 2 * j + 1, 2 * k + 1, c);
            }
        }
    }
    for i in 0..n {
        for j in 0..ns_in2 {
            for k in 1..=ns_in2 {
                let c = avg4(vin.get(i, j, k), vin.get(i, j + 1, k), out.getc(i, 2 * j + 1, 2 * k - 1), out.getc(i, 2 * j + 1, 2 * k + 1));
                out.set(i, 2 * j + 1, 2 * k, c);
            }
        }
    }
    for i in 0..n {
        for j in 1..ns_in2 {
            for k in 0..ns_in2 {
                let c = avg4(vin.get(i, j, k), vin.get(i, j, k + 1), out.getc(i, 2 * j - 1, 2 * k + 1), out.getc(i, 2 * j + 1, 2 * k + 1));
                out.set(i, 2 * j, 2 * k + 1, c);
            }
        }
    }
    let (gamma, beta) = (0.25, -0.25);
    for i in 0..n {
        for j in 1..ns_in2 {
            for k in 1..=ns_in2 {
                let co1 = avg4(out.getc(i, 2 * j, 2 * k - 1), out.getc(i, 2 * j, 2 * k + 1), out.getc(i, 2 * j - 1, 2 * k), out.getc(i, 2 * j + 1, 2 * k));
                let co2 = avg4(out.getc(i, 2 * j - 1, 2 * k - 1), out.getc(i, 2 * j + 1, 2 * k - 1), out.getc(i, 2 * j - 1, 2 * k + 1), out.getc(i, 2 * j + 1, 2 * k + 1));
                let co = add(add(co1, mul(co2, beta)), mul(vin.get(i, j, k), gamma));
                out.set(i, 2 * j, 2 * k, co);
            }
        }
    }
    out.copy_equiv();
    let gamma = sabin_gamma(n);
    let beta = -gamma;
    let (mut co1, mut co2) = ([0.0; 3], [0.0; 3]);
    for i in 0..n {
        co1 = add(co1, out.get(i, ns_in, ns_in - 1));
        co2 = add(co2, out.get(i, ns_in - 1, ns_in - 1));
        co2 = add(co2, out.get(i, ns_in - 1, ns_in + 1));
    }
    let mut co = mul(co1, 1.0 / n as f64);
    co = add(co, mul(co2, beta / (2.0 * n as f64)));
    co = add(co, mul(vin.get(0, ns_in2, ns_in2), gamma));
    for i in 0..n {
        out.set(i, ns_in, ns_in, co);
    }
    // boundary: exact profile points
    for i in 0..n {
        let inext = (i + 1) % n;
        for k in 0..=ns_out {
            let c = prof(i, k, ns_out);
            out.set(i, 0, k, c);
            if k >= ns_in && k < ns_out {
                out.set(inext, ns_out - k, 0, c);
            }
        }
    }
    out
}

// ---------------------------------------------------------------- bevel
struct VInfo {
    nbr: Vec<u32>,       // neighbours in cyclic order
    bev: Vec<bool>,      // edge to nbr[i] beveled
    gap_face: Vec<usize>, // face between nbr[i] and nbr[i+1]
    bvs: Vec<V3>,        // boundary verts in cyclic order
    gap_bv: Vec<usize>,  // boundary vert index of each gap
    // profile j runs bvs[j] -> bvs[j+1] along edge nbr[prof_edge[j]]
    prof_edge: Vec<usize>,
}

pub fn bevel3(m: &Mesh, w0: f64, seg: usize, angle: f64) -> Option<Mesh> {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
    for (fi, f) in m.faces.iter().enumerate() {
        for i in 0..f.len() {
            edges.entry(key(f[i], f[(i + 1) % f.len()])).or_default().push(fi);
        }
    }
    let fnorm: Vec<V3> = m.faces.iter().map(|f| face_normal(m, f)).collect();
    let mut bev: HashMap<(u32, u32), bool> = HashMap::default();
    let mut signed: HashMap<(u32, u32), f64> = HashMap::default();
    for (&e, fs) in &edges {
        if fs.len() != 2 {
            return None;
        }
        let ang = dot(fnorm[fs[0]], fnorm[fs[1]]).clamp(-1.0, 1.0).acos();
        // convex when a vertex of face 2 lies behind the plane of face 1
        let f1 = &m.faces[fs[0]];
        let f2 = &m.faces[fs[1]];
        let x = f2.iter().find(|&&v| v != e.0 && v != e.1).copied().unwrap();
        let conv = dot(sub(m.pos[x as usize], m.pos[e.0 as usize]), fnorm[fs[0]]) < 0.0;
        let _ = f1;
        signed.insert(e, if conv { ang } else { -ang });
        bev.insert(e, ang > angle);
    }
    if !bev.values().any(|&b| b) {
        return Some(m.clone());
    }
    let n = m.pos.len();
    let mut vfaces: Vec<Vec<usize>> = vec![vec![]; n];
    for (fi, f) in m.faces.iter().enumerate() {
        for &v in f {
            vfaces[v as usize].push(fi);
        }
    }
    // clamp: beveled edge length / 2 (both ends take w along the edge for right-angle corners)
    let mut w = w0;
    for (&e, &b) in &bev {
        if b {
            let l = len(sub(m.pos[e.0 as usize], m.pos[e.1 as usize]));
            w = w.min(l / 2.0);
        }
    }
    let mut info: HashMap<u32, VInfo> = HashMap::default();
    for v in 0..n as u32 {
        let vf = &vfaces[v as usize];
        let nb_e: Vec<bool> = vf
            .iter()
            .flat_map(|&fi| {
                let f = &m.faces[fi];
                let i = f.iter().position(|&x| x == v).unwrap();
                [f[(i + f.len() - 1) % f.len()], f[(i + 1) % f.len()]]
            })
            .map(|u| bev[&key(u, v)])
            .collect();
        if !nb_e.iter().any(|&b| b) {
            continue;
        }
        // cyclic neighbour order through the faces at v
        let mut pairs: Vec<(u32, u32, usize)> = vec![];
        for &fi in vf {
            let f = &m.faces[fi];
            let i = f.iter().position(|&x| x == v).unwrap();
            pairs.push((f[(i + f.len() - 1) % f.len()], f[(i + 1) % f.len()], fi));
        }
        // faces wind prev -> v -> next ; walk next(face) = pair whose prev == this next
        let mut order = vec![pairs[0]];
        loop {
            let last = order.last().unwrap().1;
            match pairs.iter().find(|p| p.0 == last) {
                Some(p) if p.2 != order[0].2 => order.push(*p),
                _ => break,
            }
        }
        if order.len() != pairs.len() {
            return None;
        }
        let nbr: Vec<u32> = order.iter().map(|p| p.0).collect();
        let k = nbr.len();
        if !(k == 3 || k == 4) {
            return None;
        }
        let bevf: Vec<bool> = nbr.iter().map(|&u| bev[&key(u, v)]).collect();
        if bevf.iter().filter(|&&b| b).count() != 3 {
            return None;
        }
        // gap i lies between nbr[i] and nbr[i+1]; face order[i] has prev=nbr[i], next=nbr[i+1]
        let pv = m.pos[v as usize];
        let dirs: Vec<V3> = nbr.iter().map(|&u| unit(sub(m.pos[u as usize], pv))).collect();
        // boundary point of each gap
        let mut gap_pt: Vec<V3> = vec![];
        for i in 0..k {
            let (a, b) = (i, (i + 1) % k);
            let th = dot(dirs[a], dirs[b]).clamp(-1.0, 1.0).acos();
            let s = th.sin();
            let pt = match (bevf[a], bevf[b]) {
                (true, true) => add(pv, mul(add(dirs[a], dirs[b]), w / s)),
                (true, false) => add(pv, mul(dirs[b], w / s)),
                (false, true) => add(pv, mul(dirs[a], w / s)),
                _ => return None,
            };
            gap_pt.push(pt);
        }
        // merge the two gap points around an unbeveled edge (mean distance along the edge)
        let mut gap_bv = vec![usize::MAX; k];
        let mut bvs: Vec<V3> = vec![];
        let mut prof_edge: Vec<usize> = vec![];
        let start = (0..k).find(|&i| bevf[i]).unwrap();
        let mut idx = start;
        let mut last_edge_bev = false;
        let _ = &mut last_edge_bev;
        // walk gaps starting just after a beveled edge nbr[start]: gap `start`
        let mut count = 0;
        while count < k {
            let g = (start + count) % k;
            let next_edge = (g + 1) % k; // edge after gap g
            if bevf[next_edge] || (count + 1 == k) {
                // gap g stands alone or closes the list
            }
            // is the edge before gap g (nbr[g]) unbeveled? then gap g shares its point with gap g-1
            if !bevf[g] {
                // shared with previous gap: handled when previous gap created
            }
            let _ = idx;
            idx = g;
            count += 1;
        }
        // simple construction: iterate edges starting at a beveled edge; between consecutive beveled edges collect gaps
        let mut bidx = 0usize;
        let beveled_edges: Vec<usize> = (0..k).filter(|&i| bevf[i]).collect();
        let nb = beveled_edges.len();
        for t in 0..nb {
            let e_from = beveled_edges[t];
            let e_to = beveled_edges[(t + 1) % nb];
            // gaps e_from .. e_to-1 (cyclic)
            let mut gaps = vec![];
            let mut g = e_from;
            while g != e_to {
                gaps.push(g);
                g = (g + 1) % k;
            }
            let p = if gaps.len() == 1 {
                gap_pt[gaps[0]]
            } else if gaps.len() == 2 {
                // one unbeveled edge d between two beveled edges: meet of the two offset lines, slid onto d
                let d = dirs[(e_from + 1) % k];
                let (a, b) = (dirs[e_from], dirs[e_to]);
                let na = unit(sub(d, mul(a, dot(d, a))));
                let nb2 = unit(sub(d, mul(b, dot(d, b))));
                let beta = w / dot(b, na);
                let alpha = w / dot(a, nb2);
                let mp = add(mul(a, alpha), mul(b, beta));
                add(pv, mul(d, dot(mp, d)))
            } else {
                return None;
            };
            for &g in &gaps {
                gap_bv[g] = bidx;
            }
            bvs.push(p);
            // the beveled edge e_to follows this boundary vert
            prof_edge.push(e_to);
            bidx += 1;
        }
        if bvs.len() != 3 {
            return None;
        }
        // profile j runs bvs[j] -> bvs[j+1]; it belongs to the beveled edge between them = prof_edge[j]
        info.insert(v, VInfo { nbr, bev: bevf, gap_face: order.iter().map(|p| p.2).collect(), bvs, gap_bv, prof_edge });
    }
    // ---- output vertices
    let mut out = Mesh::new();
    let mut plain: Vec<u32> = vec![u32::MAX; n];
    for v in 0..n {
        if !info.contains_key(&(v as u32)) {
            plain[v] = out.pos.len() as u32;
            out.pos.push(m.pos[v]);
        }
    }
    let ns = seg;
    // per beveled vertex: vmesh vertex ids and coordinates
    let mut vm_ids: HashMap<u32, Vec<u32>> = HashMap::default(); // flat index per VM
    let mut vms: HashMap<u32, VM> = HashMap::default();
    let mut patch: Vec<Vec<u32>> = vec![];
    for (&v, inf) in &info {
        let pv = m.pos[v as usize];
        // profile points
        let nbv = 3;
        let prof_pts: Vec<Box<dyn Fn(usize, usize) -> V3>> = (0..nbv)
            .map(|j| {
                let s = inf.bvs[j];
                let e = inf.bvs[(j + 1) % nbv];
                let u = m.pos[inf.nbr[inf.prof_edge[j]] as usize];
                // closest point of the edge line (pv, u) to the line (s, e)
                let d1 = sub(u, pv);
                let d2 = sub(e, s);
                let r = sub(pv, s);
                let (a, b, c, d_, ee) = (dot(d1, d1), dot(d1, d2), dot(d2, d2), dot(d1, r), dot(d2, r));
                let den = a * c - b * b;
                let t = if den.abs() > 1e-18 { (b * ee - c * d_) / den } else { 0.0 };
                let mid = add(pv, mul(d1, t));
                let cc = sub(add(s, e), mid);
                let lin = den.abs() <= 1e-18;
                Box::new(move |k: usize, nseg: usize| -> V3 {
                    if lin {
                        return add(mul(s, 1.0 - k as f64 / nseg as f64), mul(e, k as f64 / nseg as f64));
                    }
                    let ph = PI / 2.0 * k as f64 / nseg as f64;
                    let (sn, cs) = ph.sin_cos();
                    add(cc, add(mul(sub(s, cc), cs), mul(sub(e, cc), sn)))
                }) as Box<dyn Fn(usize, usize) -> V3>
            })
            .collect();
        if ns == 1 {
            let mut ids = vec![];
            for j in 0..3 {
                ids.push(out.pos.len() as u32);
                out.pos.push(inf.bvs[j]);
            }
            patch.push(ids.clone());
            vm_ids.insert(v, ids);
            continue;
        }
        if ns % 2 != 0 || ns < 2 {
            return None;
        }
        let ns2 = ns / 2;
        // signed face angles at the vertex
        let mut tot = 0.0;
        let mut ok_tri = true;
        let mut in_plane = 0;
        for &u in &inf.nbr {
            let sa = signed[&key(u, v)];
            if sa.abs() <= PI / 4.0 {
                in_plane += 1;
            } else if sa.abs() >= 3.0 * PI / 4.0 {
                ok_tri = false;
            }
            tot += sa;
        }
        let k = inf.nbr.len();
        if in_plane != k - 3 {
            ok_tri = false;
        }
        // offsets actually realised on each side of every beveled edge must all equal the requested width
        for (ei, &isb) in inf.bev.iter().enumerate() {
            if !isb {
                continue;
            }
            let dir = unit(sub(m.pos[inf.nbr[ei] as usize], pv));
            for g in [ (ei + k - 1) % k, ei ] {
                let p = sub(inf.bvs[inf.gap_bv[g]], pv);
                let perp = sub(p, mul(dir, dot(p, dir)));
                if (len(perp) - w).abs() > 1e-6 {
                    ok_tri = false;
                }
            }
        }
        if ((tot.abs() - 3.0 * PI / 2.0).abs()) > PI / 4.0 {
            ok_tri = false;
        }
        let vmf: VM;
        if ok_tri {
            // cube corner: unit octant
            let unit_prof = |i: usize, kk: usize, nseg: usize| -> V3 {
                let ph = PI / 2.0 * kk as f64 / nseg as f64;
                let (sn, cs) = ph.sin_cos();
                let mut p = [0.0; 3];
                p[i] = cs;
                p[(i + 1) % 3] = sn;
                p
            };
            let mut vm0 = VM::new(3, 2);
            for i in 0..3 {
                let mut e = [0.0; 3];
                e[i] = 1.0;
                vm0.set(i, 0, 0, e);
                vm0.set(i, 0, 1, unit_prof(i, 2, 4));
            }
            let mut cen = [1.0 / 3f64.sqrt(); 3];
            cen = mul(cen, 1.4);
            vm0.set(0, 1, 1, cen);
            vm0.copy_equiv();
            let mut vm1 = vm0;
            while vm1.ns < ns {
                vm1 = cubic_subdiv(&mut vm1, &unit_prof);
            }
            for i in 0..3 {
                for j in 0..=ns2 {
                    for kk in 0..=ns {
                        let p = unit(vm1.get(i, j, kk));
                        vm1.set(i, j, kk, p);
                    }
                }
            }
            // make_unit_cube_map(B0, B1, B2, v)
            let (a, b, c) = (inf.bvs[0], inf.bvs[1], inf.bvs[2]);
            let m0 = mul(sub(add(sub(a, b), pv), c), 0.5);
            let m1 = mul(sub(add(sub(b, a), pv), c), 0.5);
            let m2 = mul(sub(add(sub(c, a), pv), b), 0.5);
            let t = mul(sub(add(add(a, b), c), pv), 0.5);
            for i in 0..3 {
                for j in 0..=ns2 {
                    for kk in 0..=ns {
                        let p = vm1.get(i, j, kk);
                        let q = add(add(add(mul(m0, p[0]), mul(m1, p[1])), mul(m2, p[2])), t);
                        vm1.set(i, j, kk, q);
                    }
                }
            }
            vmf = vm1;
        } else {
            let mut vm0 = VM::new(3, 2);
            let mut cen = [0.0; 3];
            for i in 0..3 {
                vm0.set(i, 0, 0, inf.bvs[i]);
                vm0.set(i, 0, 1, prof_pts[i](2, 4));
                cen = add(cen, inf.bvs[i]);
            }
            let cen = mul(cen, 1.0 / 3.0);
            let fullness = 0.551; // circle_fullness[nseg-1], nseg = 4
            let dir = sub(pv, cen);
            let c0 = if dot(dir, dir) > 1e-12 { add(cen, mul(dir, fullness)) } else { cen };
            vm0.set(0, 1, 1, c0);
            vm0.copy_equiv();
            let pf = |i: usize, kk: usize, nseg: usize| prof_pts[i](kk, nseg);
            let mut vm1 = vm0;
            loop {
                vm1 = cubic_subdiv(&mut vm1, &pf);
                if vm1.ns >= ns {
                    break;
                }
            }
            vmf = vm1;
        }
        // create vertices for canonical entries
        let mut ids = vec![u32::MAX; 3 * (ns2 + 1) * (ns + 1)];
        for i in 0..3 {
            for j in 0..=ns2 {
                for kk in 0..=ns {
                    if j == 0 && kk == ns {
                        continue;
                    }
                    if !vmf.is_canon(i, j, kk) {
                        continue;
                    }
                    ids[vmf.idx(i, j, kk)] = out.pos.len() as u32;
                    out.pos.push(vmf.get(i, j, kk));
                }
            }
        }
        for i in 0..3 {
            for j in 0..=ns2 {
                for kk in 0..=ns {
                    let ix = vmf.idx(i, j, kk);
                    if ids[ix] == u32::MAX {
                        let (a, b, c) = vmf.canon(i, j, kk);
                        ids[ix] = ids[vmf.idx(a, b, c)];
                    }
                }
            }
        }
        // patch quads
        let mut quads = vec![];
        for i in 0..3 {
            for j in 0..ns2 {
                for kk in 0..ns2 {
                    quads.push(vec![ids[vmf.idx(i, j, kk)], ids[vmf.idx(i, j, kk + 1)], ids[vmf.idx(i, j + 1, kk + 1)], ids[vmf.idx(i, j + 1, kk)]]);
                }
            }
        }
        // orientation: the patch normal must agree with the surface normal at v
        let mut nv = [0.0; 3];
        for &fi in &vfaces[v as usize] {
            nv = add(nv, fnorm[fi]);
        }
        let mut pn = [0.0; 3];
        for q in &quads {
            let tmp = Mesh { pos: out.pos.clone(), faces: vec![q.clone()], uv: vec![] };
            pn = add(pn, face_normal(&tmp, q));
        }
        if dot(pn, nv) < 0.0 {
            for q in quads.iter_mut() {
                q.reverse();
            }
        }
        patch.extend(quads);
        vm_ids.insert(v, ids);
        vms.insert(v, vmf);
    }
    // profile row accessor: ids of profile j of vertex v, k = 0..ns
    let row = |v: u32, j: usize, k: usize| -> u32 {
        let ids = &vm_ids[&v];
        if ns == 1 {
            return ids[if k == 0 { j } else { (j + 1) % 3 }];
        }
        let vm = &vms[&v];
        ids[vm.idx(j, 0, k)]
    };
    // corner of face f at beveled vertex v
    let corner = |v: u32, fi: usize| -> Option<u32> {
        let inf = &info[&v];
        let g = inf.gap_face.iter().position(|&x| x == fi)?;
        let b = inf.gap_bv[g];
        Some(row(v, b, 0))
    };
    for (fi, f) in m.faces.iter().enumerate() {
        let mut g = vec![];
        for &v in f {
            if info.contains_key(&v) {
                g.push(corner(v, fi)?);
            } else {
                g.push(plain[v as usize]);
            }
        }
        out.faces.push(g);
    }
    // edge strips
    for (&e, fs) in &edges {
        if !bev[&e] {
            continue;
        }
        let fa = fs[0];
        let f = &m.faces[fa];
        // directed edge of the first face
        let i = (0..f.len()).find(|&i| key(f[i], f[(i + 1) % f.len()]) == e).unwrap();
        let (a, b) = (f[i], f[(i + 1) % f.len()]);
        // rows from the fa side, for each end
        let rows_for = |v: u32, other: u32| -> Option<Vec<u32>> {
            let inf = &info[&v];
            let ei = inf.nbr.iter().position(|&u| u == other)?;
            let j = inf.prof_edge.iter().position(|&pe| pe == ei)?;
            // profile j: bvs[j] -> bvs[j+1]; bvs[j] is the boundary vert of the gap before edge ei, i.e. gap ei-1
            let gp = (ei + inf.nbr.len() - 1) % inf.nbr.len();
            let start_face = inf.gap_face[gp];
            let r: Vec<u32> = (0..=ns).map(|k| row(v, j, k)).collect();
            if start_face == fa {
                Some(r)
            } else {
                Some(r.into_iter().rev().collect())
            }
        };
        let ra = rows_for(a, b)?;
        let rb = rows_for(b, a)?;
        for j in 0..ns {
            out.faces.push(vec![rb[j], ra[j], ra[j + 1], rb[j + 1]]);
        }
    }
    if ns == 1 {
        for (&v, ids) in &vm_ids {
            let mut q = ids.clone();
            let mut nv = [0.0; 3];
            for &fi in &vfaces[v as usize] {
                nv = add(nv, fnorm[fi]);
            }
            let tmp = Mesh { pos: out.pos.clone(), faces: vec![q.clone()], uv: vec![] };
            if dot(face_normal(&tmp, &q), nv) < 0.0 {
                q.reverse();
            }
            out.faces.push(q);
        }
    } else {
        out.faces.extend(patch);
    }
    Some(out)
}
