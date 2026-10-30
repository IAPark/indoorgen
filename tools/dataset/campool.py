"""campool.py: the dataset camera pool without Blender. dscap.py picks views inside a Blender process (BVH ray casts against the imported
scene), which made Blender start-up + USD import the slowest part of a preview house. This module loads gen2's glb with trimesh, casts
with Embree (embreex), and runs dscap's own pose search (make_pool, view_score, the strict pool + relaxed fallback) with a batched
port of its view rules (view_stats_np: the same checks in the same order, one Embree call per candidate instead of ~430 single casts).
Writes the same cameras_pool.json. Usage: campool.py scene.glb spec.json (spec as for dscap: room, width, height, n_views, camera_seed,
cameras, pool_path)."""
import os, sys, json, math, re, time, types
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))


def _dscap():
    """dscap.py with stand-ins for the Blender modules (only its pure-numpy pose search is used here)"""
    for m in ("bpy", "mathutils", "mathutils.bvhtree"):
        if m not in sys.modules: sys.modules[m] = types.ModuleType(m)
    sys.modules["mathutils.bvhtree"].BVHTree = None
    sys.modules["mathutils"].bvhtree = sys.modules["mathutils.bvhtree"]
    sys.path.insert(0, HERE)
    import dscap
    return dscap


D = _dscap()
ROOM_RE = D.ROOM_RE


class SceneNP:
    """the same interface dscap.Scene offers make_pool / view_stats, built from the glb (z-up world, metres)"""
    def __init__(self, glb, room, sidecar):
        import trimesh
        from trimesh.ray.ray_pyembree import RayMeshIntersector
        t = time.time()
        self.room = room
        sc = trimesh.load(glb, force="scene", process=False)
        classes = {n["name"]: n.get("class", "") for n in sidecar.get("nodes", [])}
        V, T, OB, self.names, kind, self.cls, self.okeys, self.bbox = [], [], [], [], [], [], [], []
        base = 0
        Y2Z = np.array([[1, 0, 0, 0], [0, 0, -1, 0], [0, 1, 0, 0], [0, 0, 0, 1]], np.float64)  # glTF y-up -> z-up
        for node in sc.graph.nodes_geometry:
            M, gname = sc.graph[node]
            g = sc.geometry[gname]
            if not hasattr(g, "faces") or len(g.faces) == 0: continue
            v = (np.c_[np.asarray(g.vertices, np.float64), np.ones(len(g.vertices))] @ (Y2Z @ M).T)[:, :3]
            f = np.asarray(g.faces, np.int64)
            V.append(v); T.append(f + base); OB.append(np.full(len(f), len(self.names), np.int32)); base += len(v)
            self.names.append(node); self.okeys.append(re.sub(r"\.\d+$", "", node.split("#")[0])); self.bbox.append((v.min(0), v.max(0)))
            m = ROOM_RE.search(node)
            kind.append({"wall": 1, "floor": 2, "ceiling": 3}[m.group(1)] if m else 0)
            c = classes.get(node)
            if c is None: c = classes.get(re.sub(r"\.\d+$", "", node))
            if c is None: c = next((cc for k, cc in classes.items() if node.startswith(k)), "")
            self.cls.append(c)
        self.V = np.concatenate(V); self.T = np.concatenate(T); self.tri_obj = np.concatenate(OB)
        self.kind = np.array(kind, np.int8); self.tri_kind = self.kind[self.tri_obj]
        self.mesh = trimesh.Trimesh(self.V, self.T, process=False)
        self.fn = np.asarray(self.mesh.face_normals)
        self.rx = RayMeshIntersector(self.mesh)
        fl = [i for i, n in enumerate(self.names) if n.startswith(room) and ".floor" in n]
        ft = np.nonzero(np.isin(self.tri_obj, fl))[0]
        self.floor_tris = self.V[self.T[ft]]
        e = {}
        for tri in self.T[ft]:
            for a, b in ((tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])):
                k = (min(a, b), max(a, b)); e[k] = e.get(k, 0) + 1
        bd = [k for k, c in e.items() if c == 1]
        self.bsegs = np.array([[self.V[a][:2], self.V[b][:2]] for a, b in bd]) if bd else np.zeros((0, 2, 2))
        self.floor_z = float(self.floor_tris[:, :, 2].mean()) if len(ft) else 0.0
        a = np.linalg.norm(np.cross(self.floor_tris[:, 1] - self.floor_tris[:, 0], self.floor_tris[:, 2] - self.floor_tris[:, 0]), axis=1) / 2
        self.floor_area = float(a.sum()); self._pa = a / a.sum() if a.sum() > 0 else a
        self.t_load = time.time() - t

    edge_dist = D.Scene.edge_dist
    sample_floor = D.Scene.sample_floor
    obj_class = D.Scene.obj_class

    def cast_many(self, o, dirs, far=100.0):
        """first hits of rays from o along unit dirs: (dist (inf = miss), triangle (-1), normal)"""
        dirs = np.asarray(dirs, np.float64); n = len(dirs)
        org = np.broadcast_to(np.asarray(o, np.float64), (n, 3)).copy()
        tri, ray, loc = self.rx.intersects_id(org, dirs, multiple_hits=False, return_locations=True)
        dist = np.full(n, np.inf); tr = np.full(n, -1, np.int64)
        if len(ray):
            dd = np.linalg.norm(loc - org[ray], axis=1)
            dist[ray] = dd; tr[ray] = tri
        tr[dist > far] = -1; dist[dist > far] = np.inf
        nrm = np.zeros((n, 3)); ok = tr >= 0; nrm[ok] = self.fn[tr[ok]]
        return dist, tr, nrm

    def cast(self, o, d, far=100.0):
        """dscap.Scene.cast: (location, normal, triangle, distance) or None"""
        d = np.asarray(d, np.float64); d = d / max(np.linalg.norm(d), 1e-12)
        dist, tr, nrm = self.cast_many(o, d[None], far)
        if tr[0] < 0: return None
        return (np.asarray(o) + d * dist[0], nrm[0], int(tr[0]), float(dist[0]))


_EYE = np.array([(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (.7, .7, 0), (-.7, .7, 0), (.7, -.7, 0), (-.7, -.7, 0),
                 (.7, 0, .7), (-.7, 0, .7), (0, .7, .7), (0, -.7, .7), (.7, 0, -.7), (-.7, 0, -.7), (0, .7, -.7), (0, -.7, -.7)], np.float64)


def view_stats_np(S, c2w, f, W, H, grid=(24, 18), strict=True):
    """dscap.view_stats, batched: the same probes, accumulations and rules in the same order"""
    o = c2w[:3, 3]
    # eye probes (dscap casts the unnormalised directions; Blender normalises them, so do we)
    ed = _EYE / np.linalg.norm(_EYE, axis=1, keepdims=True)
    dist, tr, nrm = S.cast_many(o, ed, 0.3)
    for i in range(len(ed)):
        if tr[i] < 0: continue
        if dist[i] < 0.22 or (S.tri_kind[tr[i]] == 0 and np.dot(nrm[i], _EYE[i]) > 0):
            return None, "eye-in-object" if S.tri_kind[tr[i]] == 0 else "eye-near-wall"
    hd = S.cast(o, (0.0, 0.0, -1.0))
    if hd is not None and S.tri_kind[hd[2]] == 0 and hd[3] < (o[2] - S.floor_z) - 0.12 and "Rug" not in S.okeys[S.tri_obj[hd[2]]]:
        return None, "over-furniture"
    gw, gh = grid; ray = D.rays_cv(W, H, f)
    ys = np.linspace(0, ray.shape[0] - 1, gh).astype(int); xs = np.linspace(0, ray.shape[1] - 1, gw).astype(int)
    YY, XX = np.meshgrid(ys, xs, indexing="ij"); YY = YY.ravel(); XX = XX.ravel()
    d = ray[YY, XX] @ c2w[:3, :3].T; d /= np.linalg.norm(d, axis=1, keepdims=True)
    dist, tr, _ = S.cast_many(o, d)
    hit = tr >= 0
    n = gw * gh
    if not hit.any(): return None, "all-miss"
    kd = np.where(hit, S.tri_kind[np.maximum(tr, 0)], -1)
    ok_ = np.array([S.okeys[S.tri_obj[t]] if t >= 0 else "" for t in tr], dtype=object)
    ds = dist[hit]
    miss = int((~hit).sum()); wall = int(np.isin(kd, (1, 3)).sum())
    low = YY >= ys[len(ys) * 2 // 3]
    n_low = int((hit & low).sum()); near_low = int((hit & low & (kd == 0) & (dist < 1.1)).sum())
    near_obj = {}
    for i in np.nonzero(hit & (dist < 1.1))[0]:
        k = ok_[i] if kd[i] == 0 else "_wall"; near_obj[k] = near_obj.get(k, 0) + 1
    pend = int(sum(1 for i in np.nonzero(hit & (kd == 0) & (dist < 1.2))[0] if ok_[i].startswith("pendant")))
    xl, xr = xs[len(xs) // 5], xs[len(xs) - 1 - len(xs) // 5]
    edge_n = [int((hit & (XX <= xl)).sum()), int((hit & (XX > xl) & (XX >= xr)).sum())]
    edge_near = [int((hit & (XX <= xl) & (dist < 0.8)).sum()), int((hit & (XX > xl) & (XX >= xr) & (dist < 0.8)).sum())]
    anc = next((v for k, v in D.ANCHORS.items() if getattr(S, "room", "").startswith(k)), ())
    objs = set(); nobjpix = 0; person = 0; anchor = 0; pnear = 1e9; per = {}; door = 0; win = 0
    for i in np.nonzero(hit & (kd == 0))[0]:
        k = ok_[i]
        if ("Door" in k or k.startswith("door")) and dist[i] < 2.0: door += 1
        if "Window" in k or k.startswith("window"): win += 1
        if not ("Window" in k or "Door" in k or k.startswith("window") or k.startswith("door")):
            objs.add(k); nobjpix += 1; per[k] = per.get(k, 0) + 1
            if k.startswith("person"): person += 1; pnear = min(pnear, dist[i])
            anchor += any(a in k for a in anc)
    ncls = len({re.sub(r"^\d+_", "", k).split(".")[0] for k in objs})
    st = dict(med=float(np.median(ds)), p10=float(np.percentile(ds, 10)), miss=miss / n, wall=wall / n, nobj=len(objs), ncls=ncls, objfrac=nobjpix / n, person=person / n, anchor=anchor / n, near_low=near_low / max(n_low, 1), win=win / n)
    if st["p10"] < 0.35: return None, "too-close"
    if strict and st.get("near_low", 1.0) > 0.3: return None, "foreground-blocked"
    if strict and max(near_obj.values(), default=0) / n > 0.16: return None, "near-object-fills-frame"
    if st["med"] < float(np.clip(0.38 * math.sqrt(max(S.floor_area, 1.0)), 1.0, 1.3)): return None, "near-median"
    if st["miss"] > 0.2: return None, "sees-void"
    if st["wall"] > 0.45: return None, "mostly-wall"
    if st["ncls"] < 5: return None, "few-classes"
    if pend: return None, "pendant-at-eye"
    if anc and getattr(S, "has_anchor", None) is None:
        S.has_anchor = any(any(a_ in k for a_ in anc) for k in S.okeys)
    if anc and S.has_anchor and anchor / n < 0.03: return None, "no-anchor"
    if strict and any(edge_n[i] and edge_near[i] / edge_n[i] > 0.6 for i in (0, 1)): return None, "edge-occluded"
    if st["nobj"] < 4: return None, "few-objects"
    if st["person"] > 0.15: return None, "person-fills-frame"
    if max(per.values(), default=0) / n > 0.3: return None, "one-object-fills-frame"
    if door / n > 0.12: return None, "door-fills-frame"
    if door / n + max(per.values(), default=0) / n > 0.4: return None, "door-and-object-fill-frame"
    if pnear < 1.6: return None, "person-too-close"
    return st, "ok"


def make_pool_file(glb, spec, log=print):
    """dscap's pool block (strict pool, relaxed fallback for cramped rooms) -> spec['pool_path']"""
    side = json.load(open(glb[:-4] + ".json"))
    S = SceneNP(glb, spec["room"], side)
    log("CAMPOOL scene tris=%d objs=%d floor_area=%.1f m2 load+bvh %.1fs" % (len(S.T), len(S.names), S.floor_area, S.t_load))
    D.view_stats = view_stats_np  # make_pool looks view_stats up in its module
    W, H, K = int(spec["width"]), int(spec["height"]), int(spec["n_views"]); ccfg = spec.get("cameras", {})
    rng = np.random.RandomState(int(spec["camera_seed"]))
    t = time.time()
    pool, rej, tries = D.make_pool(S, ccfg, W, H, int(ccfg.get("pool_factor", 3)) * K, rng)
    if len(pool) < K:
        more, rej2, tries2 = D.make_pool(S, ccfg, W, H, int(ccfg.get("pool_factor", 3)) * K - len(pool), np.random.RandomState(int(spec["camera_seed"]) + 7919), strict=False)
        log("CAMPOOL strict pool %d views; relaxed fallback added %d" % (len(pool), len(more)))
        for v in more: v["relaxed"] = True
        pool = pool + more; tries += tries2; rej = dict(rej, relaxed=rej2)
    pp = spec["pool_path"]; os.makedirs(os.path.dirname(pp), exist_ok=True)
    json.dump(dict(camera_seed=spec["camera_seed"], width=W, height=H, views=D.pool_to_json(pool), rejects=rej, tries=tries, room=spec["room"], made_by=spec.get("variant"), engine="campool"), open(pp, "w"), indent=1)
    log("CAMPOOL pool %d views from %d tries in %.1fs, rejects %s" % (len(pool), tries, time.time() - t, rej))
    return len(pool)


if __name__ == "__main__":
    make_pool_file(sys.argv[1], json.load(open(sys.argv[2])))
