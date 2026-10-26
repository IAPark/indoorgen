#!/usr/bin/env python3
"""meshqc.py SCENE.glb [--json OUT.json] [--top N] : automatic mesh-quality checker over the exported glb nodes.

The glb is a triangle soup (every triangle owns its 3 vertices), so topology is rebuilt by welding vertices on a 0.1 mm grid.
Per node (= one part/asset, class from extras) it reports:
  holes      boundary edges (used by 1 triangle) on nodes that are mostly closed (boundary edges < 25% of all edges);
             clustered into loops; metric = total boundary length (m)
  nonmanif   edges used by >2 triangles
  tjunc      boundary vertices that lie on the interior of another boundary edge (cracks / T-junctions); count
  degenerate triangles with area < 1e-9 m2 (zero-area) ; sliver = longest edge >= 3 cm and (2*area/longest) / longest < 0.02
  flipped    closed manifold components with negative signed volume (inside out) ; inconsistent = manifold edges traversed
             in the same direction by both triangles (winding mismatch)
  dupface    triangles with identical welded vertex triples inside the node (z-fighting), and across nodes
  nan        non-finite vertices ; empty = node without any non-degenerate triangle
"""
import sys, json, struct, argparse
import numpy as np
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import connected_components
from scipy.spatial import cKDTree

Q = 1e4  # weld grid 0.1 mm


def load_glb(path):
    with open(path, 'rb') as f:
        data = f.read()
    jl = struct.unpack('<I', data[12:16])[0]
    js = json.loads(data[20:20 + jl])
    bo = 20 + jl + 8
    binb = memoryview(data)[bo:]
    out = []
    for n in js['nodes']:
        if 'mesh' not in n:
            continue
        m = js['meshes'][n['mesh']]
        p = m['primitives'][0]
        pa = js['accessors'][p['attributes']['POSITION']]
        bv = js['bufferViews'][pa['bufferView']]
        pos = np.frombuffer(binb[bv['byteOffset']:bv['byteOffset'] + bv['byteLength']], dtype='<f4').reshape(-1, 3)
        ia = js['accessors'][p['indices']]
        bv = js['bufferViews'][ia['bufferView']]
        idx = np.frombuffer(binb[bv['byteOffset']:bv['byteOffset'] + bv['byteLength']], dtype='<u4').reshape(-1, 3)
        out.append((n['name'], n.get('extras', {}).get('class', ''), pos.astype(np.float64), idx))
    return out


def check_node(name, cls, pos, idx):
    r = {'name': name, 'class': cls, 'tris': int(len(idx)), 'defects': {}}
    D = r['defects']
    if len(idx) == 0:
        D['empty'] = 1
        return r, None
    bad = ~np.isfinite(pos).all(axis=1)
    if bad.any():
        D['nan'] = int(bad.sum())
        keep = ~bad[idx].any(axis=1)
        idx = idx[keep]
        if len(idx) == 0:
            D['empty'] = 1
            return r, None
    # weld
    keys = np.round(pos * Q).astype(np.int64)
    uk, inv = np.unique(keys, axis=0, return_inverse=True)
    inv = inv.reshape(-1)
    T = inv[idx]  # welded triangle vertex ids
    P = uk / Q
    a, b, c = P[T[:, 0]], P[T[:, 1]], P[T[:, 2]]
    cr = np.cross(b - a, c - a)
    area = 0.5 * np.linalg.norm(cr, axis=1)
    # degenerate (collapsed after weld or zero area)
    collapsed = (T[:, 0] == T[:, 1]) | (T[:, 1] == T[:, 2]) | (T[:, 0] == T[:, 2])
    e0, e1, e2 = np.linalg.norm(b - a, axis=1), np.linalg.norm(c - b, axis=1), np.linalg.norm(a - c, axis=1)
    longest = np.maximum(np.maximum(e0, e1), e2)
    # use the original (unwelded) geometry for area/shape so welding a tiny tri is not what flags it
    A, B, C = pos[idx[:, 0]], pos[idx[:, 1]], pos[idx[:, 2]]
    carea = 0.5 * np.linalg.norm(np.cross(B - A, C - A), axis=1)
    L = np.maximum(np.maximum(np.linalg.norm(B - A, axis=1), np.linalg.norm(C - B, axis=1)), np.linalg.norm(A - C, axis=1))
    zero = (carea < 1e-9)
    alt = np.where(L > 0, 2 * carea / np.maximum(L, 1e-30), 0)
    sliver = (~zero) & (L >= 0.03) & (alt / np.maximum(L, 1e-30) < 0.02)
    if zero.any():
        D['degenerate'] = int(zero.sum())
    if sliver.any():
        D['sliver'] = int(sliver.sum())
        r['sliver_ex'] = [(P[T[i, 0]].round(4).tolist(), float(L[i]), float(alt[i])) for i in np.flatnonzero(sliver)[:3]]
    live = ~(collapsed | zero)
    if not live.any():
        D['empty'] = 1
        return r, None
    # duplicates within node
    srt = np.sort(T[live], axis=1)
    tri_live = np.flatnonzero(live)
    u, cnt = np.unique(srt, axis=0, return_counts=True)
    if (cnt > 1).any():
        D['dupface'] = int((cnt[cnt > 1] - 1).sum())
    Tl = T[live]
    # edges
    ed = np.concatenate([Tl[:, [0, 1]], Tl[:, [1, 2]], Tl[:, [2, 0]]])
    lo, hi = np.minimum(ed[:, 0], ed[:, 1]), np.maximum(ed[:, 0], ed[:, 1])
    nv = len(P)
    ek = lo.astype(np.int64) * nv + hi
    ue, einv, ecnt = np.unique(ek, return_inverse=True, return_counts=True)
    nedges = len(ue)
    bnd = ecnt == 1
    nm = ecnt > 2
    frac = bnd.sum() / max(nedges, 1)
    r['edges'] = int(nedges)
    r['bnd_frac'] = float(frac)
    ev = np.stack([ue // nv, ue % nv], axis=1)
    elen = np.linalg.norm(P[ev[:, 0]] - P[ev[:, 1]], axis=1)
    if nm.any():
        D['nonmanif'] = int(nm.sum())
    # winding consistency for manifold edges (cnt==2): directed edge traversed same direction twice
    dir_fw = (ed[:, 0] < ed[:, 1])
    fwcount = np.bincount(einv, weights=dir_fw.astype(float), minlength=nedges)
    two = ecnt == 2
    incons = two & (fwcount != 1)
    if incons.any():
        D['inconsistent'] = int(incons.sum())
    # components
    ntri = len(Tl)
    tid = np.tile(np.arange(ntri), 3)
    # connect triangles through shared manifold-ish edges
    order = np.argsort(einv, kind='stable')
    se = einv[order]
    st = tid[order]
    same = se[1:] == se[:-1]
    ca, cb = st[:-1][same], st[1:][same]
    g = coo_matrix((np.ones(len(ca)), (ca, cb)), shape=(ntri, ntri))
    ncomp, lab = connected_components(g, directed=False)
    # per component: closed? signed volume
    a, b, c = P[Tl[:, 0]], P[Tl[:, 1]], P[Tl[:, 2]]
    vol6 = np.einsum('ij,ij->i', a, np.cross(b, c))
    cvol = np.bincount(lab, weights=vol6, minlength=ncomp) / 6.0
    tarea = 0.5 * np.linalg.norm(np.cross(b - a, c - a), axis=1)
    carea_c = np.bincount(lab, weights=tarea, minlength=ncomp)
    ebnd = np.zeros(ncomp)
    elab = lab[st[np.r_[True, ~same] & np.r_[~same, True]]] if False else None
    # boundary edge count per component
    tri_of_edge = np.zeros(nedges, dtype=np.int64)
    tri_of_edge[einv] = tid  # any triangle of the edge
    bc = np.bincount(lab[tri_of_edge[bnd]], minlength=ncomp) if bnd.any() else np.zeros(ncomp, int)
    nmc = np.bincount(lab[tri_of_edge[nm]], minlength=ncomp) if nm.any() else np.zeros(ncomp, int)
    closed = (bc == 0) & (nmc == 0)
    inside_out = closed & (cvol < -1e-9) & (carea_c > 1e-4)
    if inside_out.any():
        D['flipped'] = int(inside_out.sum())
        r['flipped_area'] = float(carea_c[inside_out].sum())
        r['flipped_vol'] = float(cvol[inside_out].sum())
    r['components'] = int(ncomp)
    # holes: boundary edges on mostly closed nodes (per-component: component has some closed sections)
    mostly_closed = frac < 0.25
    if bnd.any():
        bl = elen[bnd]
        r['bnd_len'] = float(bl.sum())
        # boundary loops: connected components of boundary edges (by shared vertex)
        be = ev[bnd]
        vs, vinv = np.unique(be.reshape(-1), return_inverse=True)
        vinv = vinv.reshape(-1, 2)
        gg = coo_matrix((np.ones(len(be)), (vinv[:, 0], vinv[:, 1])), shape=(len(vs), len(vs)))
        nl, ll = connected_components(gg, directed=False)
        loop_len = np.bincount(ll[vinv[:, 0]], weights=bl, minlength=nl)
        loop_n = np.bincount(ll[vinv[:, 0]], minlength=nl)
        r['loops'] = int(nl)
        if mostly_closed:
            D['holes'] = int(nl)
            D['hole_edges'] = int(bnd.sum())
            o = np.argsort(-loop_len)[:3]
            ex = []
            for k in o:
                sel = ll[vinv[:, 0]] == k
                pts = P[be[sel].reshape(-1)]
                ex.append({'len': float(loop_len[k]), 'edges': int(loop_n[k]), 'lo': pts.min(0).round(3).tolist(), 'hi': pts.max(0).round(3).tolist()})
            r['hole_ex'] = ex
            r['hole_len'] = float(bl.sum())
        # T-junctions: boundary vertices lying inside another boundary edge
        if len(be) >= 2:
            p0, p1 = P[be[:, 0]], P[be[:, 1]]
            mid, half = (p0 + p1) / 2, np.linalg.norm(p1 - p0, axis=1) / 2
            bv = P[vs]
            tree = cKDTree(bv)
            nt = 0
            tex = []
            maxr = float(half.max()) + 1e-3
            for i in range(len(be)):
                if half[i] < 1e-4:
                    continue
                cand = tree.query_ball_point(mid[i], half[i] + 1e-4)
                if len(cand) <= 2:
                    continue
                d = p1[i] - p0[i]
                dd = d @ d
                for k in cand:
                    if vs[k] in (be[i, 0], be[i, 1]):
                        continue
                    t = ((bv[k] - p0[i]) @ d) / dd
                    if 1e-3 < t < 1 - 1e-3:
                        dist = np.linalg.norm(p0[i] + t * d - bv[k])
                        if dist < 2e-4:
                            nt += 1
                            if len(tex) < 3:
                                tex.append(bv[k].round(3).tolist())
            if nt:
                D['tjunc'] = nt
                r['tjunc_ex'] = tex
    r['area'] = float(tarea.sum())
    lo, hi = P.min(0), P.max(0)
    r['bbox'] = [lo.round(3).tolist(), hi.round(3).tolist()]
    return r, srt_keys(uk, T[live])


def srt_keys(uk, T):
    """hashable keys of triangles by quantized coords (for cross-node duplicates)"""
    srt = np.sort(T, axis=1)
    h = (srt[:, 0].astype(np.uint64) * np.uint64(0x9E3779B97F4A7C15)) ^ (srt[:, 1].astype(np.uint64) * np.uint64(0xC2B2AE3D27D4EB4F)) ^ (srt[:, 2].astype(np.uint64) * np.uint64(0x165667B19E3779F9))
    # vertex ids are node-local; hash by coordinates instead
    kk = (uk * 1).astype(np.int64)
    vh = (kk[:, 0] * 73856093) ^ (kk[:, 1] * 19349663) ^ (kk[:, 2] * 83492791)
    vh = vh.astype(np.uint64)
    t = vh[T]
    return (t[:, 0] + t[:, 1] + t[:, 2]) ^ (t[:, 0] * t[:, 1] * t[:, 2])


def run(path, top=15, quiet=False):
    nodes = load_glb(path)
    res = []
    keysets = []
    for (name, cls, pos, idx) in nodes:
        r, k = check_node(name, cls, pos, idx)
        res.append(r)
        keysets.append(k)
    # cross-node duplicate triangles
    allk = [(i, k) for i, k in enumerate(keysets) if k is not None]
    if allk:
        ids = np.concatenate([np.full(len(k), i) for i, k in allk])
        ks = np.concatenate([k for _, k in allk])
        o = np.argsort(ks, kind='stable')
        ks_s, ids_s = ks[o], ids[o]
        same = ks_s[1:] == ks_s[:-1]
        diff = same & (ids_s[1:] != ids_s[:-1])
        for j in np.flatnonzero(diff):
            for q in (ids_s[j], ids_s[j + 1]):
                res[q]['defects']['xdup'] = res[q]['defects'].get('xdup', 0) + 1
    return res


CLASSES = ['holes', 'nonmanif', 'tjunc', 'degenerate', 'sliver', 'flipped', 'inconsistent', 'dupface', 'xdup', 'nan', 'empty']


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('glb')
    ap.add_argument('--json')
    ap.add_argument('--top', type=int, default=15)
    a = ap.parse_args()
    res = run(a.glb)
    if a.json:
        json.dump(res, open(a.json, 'w'))
    tot = {c: 0 for c in CLASSES}
    for r in res:
        for c in CLASSES:
            if r['defects'].get(c):
                tot[c] += 1
    print('nodes', len(res), 'nodes-with-defect', {c: v for c, v in tot.items() if v})
    bad = [r for r in res if r['defects']]
    bad.sort(key=lambda r: -(r.get('hole_len', 0) * 10 + r['defects'].get('flipped', 0) * 5 + r['defects'].get('tjunc', 0) * 0.01 + r['defects'].get('sliver', 0) * 0.01))
    for r in bad[:a.top]:
        print(r['name'], r['class'], r['tris'], r['defects'], {k: r[k] for k in ('hole_ex', 'tjunc_ex', 'sliver_ex') if k in r})


if __name__ == '__main__':
    main()
