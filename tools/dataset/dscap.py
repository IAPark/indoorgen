"""dscap.py: dataset capture inside Blender (imported by tools/render/render_rust.py when --dataset SPEC.json is given).

Runs in the bpy process after the Rust glb is imported and materialised.  One process renders all views of one variant:
  apply_walls / apply_lighting / lamp_state : the `variation` labels (walls, lighting) -> Cycles scene
  run                                         : camera pool -> validity per variant -> per view RGB (Cycles) + depth/normal/instance
                                                (BVH ray casts through the pixel centres, exact and anti-alias free) + cameras.json
Conventions (documented in tools/dataset/README.md): OpenCV cameras (x right, y down, z forward), world = Blender world (z up, metres),
depth = z-depth along the optical axis in metres, 0 = no surface."""
import os, sys, json, math, time, re
import numpy as np
import bpy, mathutils
from mathutils.bvhtree import BVHTree

ROOM_RE = re.compile(r"\.(wall|floor|ceiling)(\.alt)?(\.\d+)?$")
BL2CV = np.diag([1.0, -1.0, -1.0, 1.0])


# ------------------------------------------------------------------------------------------------ variation: walls / lighting
def _kelvin_rgb(k):
    """linear-ish rgb of a blackbody (Tanner Helland fit), normalised to the 6500 K white point"""
    def f(t):
        t = t / 100.0
        r = 255 if t <= 66 else 329.698727446 * (t - 60) ** -0.1332047592
        g = 99.4708025861 * math.log(t) - 161.1195681661 if t <= 66 else 288.1221695283 * (t - 60) ** -0.0755148492
        b = 255 if t >= 66 else (0 if t <= 19 else 138.5177312231 * math.log(t - 10) - 305.0447927307)
        return np.clip(np.array([r, g, b]) / 255.0, 0.0, 1.0)
    return f(k) / f(6500.0)


def apply_walls(variation):
    """variation.walls = {room name: [r, g, b] linear}: replace the base colour of that room's wall materials (texture/bump/roughness kept)"""
    walls = (variation or {}).get("walls") or {}
    done = {}
    for room, rgb in walls.items():
        n = 0
        for o in bpy.data.objects:
            if o.type != "MESH" or not o.name.startswith(room + ".wall"): continue
            for slot in o.material_slots:
                m = slot.material
                if m is None or not m.use_nodes: continue
                m = m.copy(); slot.material = m  # materials may be shared between rooms
                for nd in m.node_tree.nodes:
                    if nd.type != "BSDF_PRINCIPLED": continue
                    s = nd.inputs["Base Color"]
                    for l in list(s.links): m.node_tree.links.remove(l)
                    s.default_value = (float(rgb[0]), float(rgb[1]), float(rgb[2]), 1.0); n += 1
        done[room] = n
    return done


def apply_lighting(sc, variation):
    """variation.lighting: sun_azimuth_deg -> sky sun_rotation, sun_elevation_deg -> sun_elevation, sun_strength -> sky sun_intensity,
    sky_strength -> world Background strength, color_temp_K -> world colour tint (relative to 6500 K)"""
    lg = (variation or {}).get("lighting") or {}
    if not lg or sc.world is None or not sc.world.use_nodes: return {}
    nt = sc.world.node_tree
    sky = next((n for n in nt.nodes if n.type == "TEX_SKY"), None)
    bg = next((n for n in nt.nodes if n.type == "BACKGROUND"), None)
    if sky is not None:
        if lg.get("sun_azimuth_deg") is not None: sky.sun_rotation = math.radians(lg["sun_azimuth_deg"])
        if lg.get("sun_elevation_deg") is not None: sky.sun_elevation = math.radians(lg["sun_elevation_deg"])
        if lg.get("sun_strength") is not None: sky.sun_intensity = float(lg["sun_strength"])
    if bg is not None:
        if lg.get("sky_strength") is not None: bg.inputs["Strength"].default_value = float(lg["sky_strength"])
        if lg.get("color_temp_K") is not None:
            tint = _kelvin_rgb(float(lg["color_temp_K"]))
            mix = nt.nodes.new("ShaderNodeMix"); mix.data_type = "RGBA"; mix.blend_type = "MULTIPLY"
            mix.inputs[0].default_value = 1.0  # factor
            src = bg.inputs["Color"].links[0].from_socket if bg.inputs["Color"].is_linked else None
            if src is not None: nt.links.new(src, mix.inputs[6])
            else: mix.inputs[6].default_value = tuple(bg.inputs["Color"].default_value)
            mix.inputs[7].default_value = (tint[0], tint[1], tint[2], 1.0)
            nt.links.new(mix.outputs[2], bg.inputs["Color"])
    return {k: lg.get(k) for k in ("sun_azimuth_deg", "sun_elevation_deg", "sun_strength", "sky_strength", "color_temp_K")}


def lamp_state(variation, sidecar):
    """-> fn(light dict) -> (on, scale) from variation.lighting {lamps_on: bool | [bool per non-ceiling lamp in sidecar order],
    lamp_strength_scale, ceiling_lights_on}"""
    lg = (variation or {}).get("lighting") or {}
    lamps = [L for L in sidecar.get("lights", []) if "CeilingLight" not in L["name"]]
    idx = {L["name"]: i for i, L in enumerate(lamps)}
    lon = lg.get("lamps_on", True); cl = lg.get("ceiling_lights_on", True); sc_ = float(lg.get("lamp_strength_scale", 1.0))
    def f(L):
        if "CeilingLight" in L["name"]: return bool(cl), 1.0
        on = lon[idx[L["name"]] % len(lon)] if isinstance(lon, (list, tuple)) and lon else bool(lon)
        return bool(on), sc_
    return f


def switch_lamp_meshes(variation, sidecar):
    """emissive shaders of switched-off lamps / ceiling lights -> strength 0 (the point light is dropped in render_rust)"""
    fn = lamp_state(variation, sidecar)
    off = [L["name"] for L in sidecar.get("lights", []) if not fn(L)[0]]
    scale = {L["name"]: fn(L)[1] for L in sidecar.get("lights", [])}
    n = 0
    for o in bpy.data.objects:
        if o.type != "MESH": continue
        key = next((nm for nm in scale if o.name == nm or o.name.startswith(nm)), None)
        if key is None: continue
        for slot in o.material_slots:
            m = slot.material
            if m is None or not m.use_nodes: continue
            hit = [nd for nd in m.node_tree.nodes if nd.type == "EMISSION" or (nd.type == "BSDF_PRINCIPLED" and nd.inputs["Emission Strength"].default_value > 0)]
            if not hit: continue
            m = m.copy(); slot.material = m
            for nd in m.node_tree.nodes:
                k = "Strength" if nd.type == "EMISSION" else ("Emission Strength" if nd.type == "BSDF_PRINCIPLED" else None)
                if k is None or nd.inputs[k].is_linked: continue
                nd.inputs[k].default_value = 0.0 if key in off else nd.inputs[k].default_value * scale[key]; n += 1
    return n


# ------------------------------------------------------------------------------------------------ scene geometry
class Scene:
    """all render-visible meshes merged into one BVH (world space); tri -> object / kind lookup"""
    def __init__(self, room, sidecar):
        t = time.time()
        objs = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render and len(o.data.polygons)]
        classes = {n["name"]: n.get("class", "") for n in sidecar.get("nodes", [])}
        V, T, OB, self.names, self.kind, self.cls = [], [], [], [], [], []
        base = 0
        for i, o in enumerate(objs):
            me = o.data; me.calc_loop_triangles()
            nv = len(me.vertices); nt = len(me.loop_triangles)
            if nt == 0: continue
            co = np.empty(nv * 3, np.float32); me.vertices.foreach_get("co", co)
            tr = np.empty(nt * 3, np.int32); me.loop_triangles.foreach_get("vertices", tr)
            M = np.array(o.matrix_world, np.float64)
            V.append(co.reshape(-1, 3).astype(np.float64) @ M[:3, :3].T + M[:3, 3]); T.append(tr.reshape(-1, 3) + base)
            oi = len(self.names)
            OB.append(np.full(nt, oi, np.int32)); base += nv
            self.names.append(o.name)
            m = ROOM_RE.search(o.name)
            self.kind.append({"wall": 1, "floor": 2, "ceiling": 3}[m.group(1)] if m else 0)
            c = classes.get(o.name)
            if c is None: c = classes.get(re.sub(r"\.\d+$", "", o.name))
            if c is None: c = next((cc for k, cc in classes.items() if o.name.startswith(k)), "")
            self.cls.append(c)
        self.V = np.concatenate(V); self.T = np.concatenate(T); self.tri_obj = np.concatenate(OB)
        self.kind = np.array(self.kind, np.int8)
        self.tri_kind = self.kind[self.tri_obj]
        self.bvh = BVHTree.FromPolygons(self.V.tolist(), self.T.tolist())
        # target room floor (outline + triangles) for pose sampling
        fl = [i for i, n in enumerate(self.names) if n.startswith(room) and ".floor" in n]
        ft = np.nonzero(np.isin(self.tri_obj, fl))[0]
        self.floor_tris = self.V[self.T[ft]]  # (n,3,3)
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

    def edge_dist(self, p):
        s = self.bsegs
        if len(s) == 0: return 9.0
        a = s[:, 0]; ab = s[:, 1] - a; t = np.clip(((p - a) * ab).sum(1) / np.maximum((ab * ab).sum(1), 1e-12), 0, 1)
        return float(np.linalg.norm(a + ab * t[:, None] - p, axis=1).min())

    def sample_floor(self, rng):
        i = rng.choice(len(self._pa), p=self._pa); a, b, c = self.floor_tris[i]
        u, v = rng.rand(2)
        if u + v > 1: u, v = 1 - u, 1 - v
        return a + u * (b - a) + v * (c - a)

    def cast(self, o, d, far=100.0):
        h = self.bvh.ray_cast(tuple(o), tuple(d), far)
        return h if h[0] is not None else None


# ------------------------------------------------------------------------------------------------ cameras
def cam_pose_cv(pos, yaw, pitch):
    """c2w (OpenCV axes) for a camera at pos looking along azimuth yaw (rad, 0 = +x, ccw seen from above) and pitch (rad, + = up), roll 0"""
    fwd = np.array([math.cos(pitch) * math.cos(yaw), math.cos(pitch) * math.sin(yaw), math.sin(pitch)])
    right = np.cross(fwd, [0, 0, 1.0]); right /= np.linalg.norm(right)
    down = np.cross(fwd, right)
    M = np.eye(4); M[:3, 0] = right; M[:3, 1] = down; M[:3, 2] = fwd; M[:3, 3] = pos
    return M


def rays_cv(W, H, f, step=1):
    """camera-space direction (z = 1) per pixel centre, shape (H', W', 3)"""
    u = (np.arange(0, W, step) + 0.5 - W / 2) / f; v = (np.arange(0, H, step) + 0.5 - H / 2) / f
    return np.stack(np.broadcast_arrays(u[None, :], v[:, None], np.ones((1, 1))), -1)


def view_stats(S, c2w, f, W, H, grid=(9, 7)):
    """frustum probe: None if invalid else dict(med, p10, miss, wall)"""
    o = c2w[:3, 3]
    dirs = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (.7, .7, 0), (-.7, .7, 0), (.7, -.7, 0), (-.7, -.7, 0),
            (.7, 0, .7), (-.7, 0, .7), (0, .7, .7), (0, -.7, .7), (.7, 0, -.7), (-.7, 0, -.7), (0, .7, -.7), (0, -.7, -.7)]
    for d in dirs:
        h = S.cast(o, d, 0.3)
        if h is not None and (h[3] < 0.22 or (S.tri_kind[h[2]] == 0 and np.dot(h[1], d) > 0)): return None, "eye-in-object" if S.tri_kind[h[2]] == 0 else "eye-near-wall"
    gw, gh = grid; ray = rays_cv(W, H, f)
    ys = np.linspace(0, ray.shape[0] - 1, gh).astype(int); xs = np.linspace(0, ray.shape[1] - 1, gw).astype(int)
    ds = []; miss = 0; wall = 0
    for y in ys:
        for x in xs:
            d = c2w[:3, :3] @ ray[y, x]; n = np.linalg.norm(d); h = S.cast(o, d / n)
            if h is None: miss += 1; continue
            ds.append(h[3]); wall += S.tri_kind[h[2]] in (1, 3)
    n = gw * gh
    if not ds: return None, "all-miss"
    st = dict(med=float(np.median(ds)), p10=float(np.percentile(ds, 10)), miss=miss / n, wall=wall / n)
    if st["p10"] < 0.35: return None, "too-close"
    if st["med"] < 1.3: return None, "near-median"
    if st["miss"] > 0.2: return None, "sees-void"
    if st["wall"] > 0.7: return None, "mostly-wall"
    return st, "ok"


def make_pool(S, cfg, W, H, n, rng):
    """n candidate views: first of each trajectory/cluster is a fresh look-at-the-room pose, the rest either small trajectory steps
    (0.3-0.7 m, yaw +-20 deg) or new positions aimed at the previous view's centre hit point (overlap)"""
    lo_f, hi_f = cfg.get("focal_mm", [18, 35]); hlo, hhi = cfg.get("eye_height_m", [1.0, 1.8]); margin = cfg.get("wall_margin_m", 0.45)
    traj, cluster = cfg.get("traj_frac", 0.4), cfg.get("cluster_len", 3)
    pool, rejects, prev, run = [], {}, None, 0
    tries = 0
    while len(pool) < n and tries < n * 400:
        tries += 1
        fresh = prev is None or run >= cluster
        if fresh:
            run = 0
        kind = "fresh" if fresh else ("traj" if rng.rand() < traj else "overlap")
        if kind == "traj":
            ang = rng.uniform(0, 2 * math.pi); step = rng.uniform(0.3, 0.7)
            pos = prev["pos"] + np.array([math.cos(ang) * step, math.sin(ang) * step, rng.uniform(-0.1, 0.1)])
            yaw = prev["yaw"] + math.radians(rng.uniform(-20, 20)); pitch = prev["pitch"] + math.radians(rng.normal(0, 3))
            f = float(np.clip(prev["f_mm"] * rng.uniform(0.9, 1.1), lo_f, hi_f))
        else:
            p2 = S.sample_floor(rng); pos = np.array([p2[0], p2[1], 0.0])
            if kind == "overlap":
                tgt = prev["hit"] + rng.normal(0, 0.3, 3)
            else:
                q = S.sample_floor(rng); tgt = np.array([q[0], q[1], rng.uniform(0.4, 1.4) + S.floor_z])
                if np.linalg.norm(tgt[:2] - pos[:2]) < 1.5: continue
            f = float(math.exp(rng.uniform(math.log(lo_f), math.log(hi_f))))
        if kind == "traj":
            pos[2] = float(np.clip(pos[2], S.floor_z + hlo, S.floor_z + hhi))
        else:
            pos[2] = S.floor_z + rng.uniform(hlo, hhi)
            dv = tgt - pos; yaw = math.atan2(dv[1], dv[0]) + math.radians(rng.normal(0, 10))
            pitch = math.atan2(dv[2], np.linalg.norm(dv[:2]) + 1e-9)
        pitch = float(np.clip(pitch, math.radians(-25), math.radians(12)))
        if S.edge_dist(pos[:2]) < margin:
            rejects["wall-margin"] = rejects.get("wall-margin", 0) + 1; continue
        M = cam_pose_cv(pos, yaw, pitch); fpx = f / 36.0 * W
        st, why = view_stats(S, M, fpx, W, H)
        if st is None:
            rejects[why] = rejects.get(why, 0) + 1; continue
        d = M[:3, 2]; h = S.cast(pos, d); hit = np.array(h[0]) if h else pos + d * st["med"]
        v = dict(pos=pos, yaw=yaw, pitch=pitch, f_mm=f, kind=kind, hit=hit, stats=st)
        pool.append(v); prev = v; run += 1
    return pool, rejects, tries


def pool_to_json(pool):
    return [dict(pos=list(map(float, v["pos"])), yaw=v["yaw"], pitch=v["pitch"], f_mm=v["f_mm"], kind=v["kind"], hit=list(map(float, v["hit"])), stats=v["stats"]) for v in pool]


def pool_from_json(js):
    return [dict(pos=np.array(v["pos"]), yaw=v["yaw"], pitch=v["pitch"], f_mm=v["f_mm"], kind=v["kind"], hit=np.array(v["hit"]), stats=v.get("stats")) for v in js]


# ------------------------------------------------------------------------------------------------ depth / normals / ids
def trace(S, c2w, f, W, H):
    """z-depth (H,W) float32 (0 = no surface), world normals (H,W,3) float16 facing the camera, instance ids (H,W) uint16 (0 none, k = object k-1)"""
    ray = rays_cv(W, H, f).reshape(-1, 3)
    dw = ray @ c2w[:3, :3].T; nrm = np.linalg.norm(dw, axis=1); dwn = (dw / nrm[:, None])
    o = tuple(c2w[:3, 3]); rc = S.bvh.ray_cast
    N = len(dwn); depth = np.zeros(N, np.float32); normal = np.zeros((N, 3), np.float32); tri = np.full(N, -1, np.int32)
    dl = dwn.tolist(); far = 100.0
    for i in range(N):
        h = rc(o, dl[i], far)
        if h[0] is None: continue
        depth[i] = h[3] / nrm[i]; normal[i] = h[1]; tri[i] = h[2]
    hit = tri >= 0
    flip = (normal * dwn).sum(1) > 0; normal[flip] *= -1
    inst = np.zeros(N, np.uint16); inst[hit] = S.tri_obj[tri[hit]] + 1
    depth[depth < 0.05] = 0
    return depth.reshape(H, W), normal.reshape(H, W, 3).astype(np.float16), inst.reshape(H, W)


def write_exr(path, arr):
    import OpenEXR, Imath
    h, w = arr.shape
    hd = OpenEXR.Header(w, h); hd["channels"] = {"Z": Imath.Channel(Imath.PixelType(Imath.PixelType.FLOAT))}
    f = OpenEXR.OutputFile(path, hd); f.writePixels({"Z": arr.astype(np.float32).tobytes()}); f.close()


# ------------------------------------------------------------------------------------------------ main entry
def run(sc, sidecar, spec, log=print):
    """render all views of this variant. spec keys: out_dir, room, width, height, samples, n_views, camera_seed, cameras{...},
    pool_path, depth_format (npy|exr|both), extras [normal, instance], variation"""
    out = spec["out_dir"]; W, H = int(spec["width"]), int(spec["height"]); K = int(spec["n_views"]); ccfg = spec.get("cameras", {})
    for d in ("images", "depth"): os.makedirs(os.path.join(out, d), exist_ok=True)
    extras = spec.get("extras", ["normal", "instance"])
    for d in extras: os.makedirs(os.path.join(out, d), exist_ok=True)
    t0 = time.time(); bpy.context.view_layer.update()
    S = Scene(spec["room"], sidecar)
    log("DSCAP scene tris=%d objs=%d floor_area=%.1f m2 bvh+load %.1fs" % (len(S.T), len(S.names), S.floor_area, S.t_load))
    pp = spec["pool_path"]; pool_new = False
    if os.path.exists(pp):
        pool = pool_from_json(json.load(open(pp))["views"])
    else:
        rng = np.random.RandomState(int(spec["camera_seed"]))
        pool, rej, tries = make_pool(S, ccfg, W, H, int(ccfg.get("pool_factor", 3)) * K, rng)
        os.makedirs(os.path.dirname(pp), exist_ok=True)
        json.dump(dict(camera_seed=spec["camera_seed"], width=W, height=H, views=pool_to_json(pool), rejects=rej, tries=tries, room=spec["room"], made_by=spec.get("variant")), open(pp, "w"), indent=1)
        pool_new = True; log("DSCAP pool %d views from %d tries, rejects %s" % (len(pool), tries, rej))
    if spec.get("pool_only"): return dict(n_views=0, pool_new=pool_new, pool=len(pool), t_scene=S.t_load, t_run=time.time() - t0)
    # validity per variant (furniture changed): first K valid in pool order
    chosen, dropped = [], []
    for j, v in enumerate(pool):
        if len(chosen) >= K: break
        M = cam_pose_cv(v["pos"], v["yaw"], v["pitch"]); st, why = view_stats(S, M, v["f_mm"] / 36.0 * W, W, H)
        (chosen if st is not None else dropped).append((j, why) if st is None else (j, st))
    log("DSCAP views chosen %s dropped %s" % ([j for j, _ in chosen], dropped))
    cam = bpy.data.cameras.new("dscam"); cam.sensor_fit = "HORIZONTAL"; cam.sensor_width = 36.0; cam.clip_start = 0.05; cam.clip_end = 100.0
    co = bpy.data.objects.new("dscam", cam); sc.collection.objects.link(co); sc.camera = co
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = W, H, 100
    sc.render.image_settings.file_format = "PNG"; sc.render.image_settings.color_mode = "RGB"; sc.render.image_settings.color_depth = "8"
    sc.render.use_persistent_data = True
    views, tt = [], dict(trace=0.0, render=0.0)
    for k, (j, st) in enumerate(chosen):
        v = pool[j]; M = cam_pose_cv(v["pos"], v["yaw"], v["pitch"]); fpx = v["f_mm"] / 36.0 * W
        co.matrix_world = mathutils.Matrix((M @ BL2CV).tolist()); cam.lens = v["f_mm"]
        bpy.context.view_layer.update()
        t = time.time(); depth, nrm, inst = trace(S, M, fpx, W, H); tt["trace"] += time.time() - t
        nm = "%03d" % k
        if spec.get("depth_format", "npy") in ("npy", "both"): np.save(os.path.join(out, "depth", nm + ".npy"), depth)
        if spec.get("depth_format", "npy") in ("exr", "both"): write_exr(os.path.join(out, "depth", nm + ".exr"), depth)
        if "normal" in extras: np.save(os.path.join(out, "normal", nm + ".npy"), nrm)
        if "instance" in extras:
            from PIL import Image
            Image.fromarray(inst).save(os.path.join(out, "instance", nm + ".png"))
        sc.render.filepath = os.path.join(out, "images", nm + ".png")
        t = time.time(); bpy.ops.render.render(write_still=True); tt["render"] += time.time() - t
        w2c = np.linalg.inv(M)
        views.append(dict(index=k, pool_index=j, kind=v["kind"], file="images/%s.png" % nm, depth="depth/%s.npy" % nm, width=W, height=H,
                          K=[[fpx, 0, W / 2], [0, fpx, H / 2], [0, 0, 1]], focal_mm=v["f_mm"], sensor_width_mm=36.0,
                          w2c=w2c.tolist(), c2w=M.tolist(), near=0.05, far=100.0, valid_frac=float((depth > 0).mean()),
                          depth_median=float(np.median(depth[depth > 0])) if (depth > 0).any() else 0.0))
        log("DSCAP view %s pool %d trace %.1fs render %.1fs valid %.2f" % (nm, j, tt["trace"], tt["render"], views[-1]["valid_frac"]))
    json.dump(dict(convention="opencv: x right, y down, z forward; w2c maps world->camera; world z up, metres; depth = z-depth along the optical axis, 0 = no surface",
                   views=views, dropped_pool_views=[dict(pool_index=j, reason=r) for j, r in dropped]), open(os.path.join(out, "cameras.json"), "w"), indent=1)
    if "instance" in extras:
        json.dump({str(i + 1): dict(name=S.names[i], cls=S.cls[i], kind=["object", "wall", "floor", "ceiling"][S.kind[i]]) for i in range(len(S.names))}, open(os.path.join(out, "instances.json"), "w"))
    return dict(n_views=len(views), pool_new=pool_new, dropped=len(dropped), t_scene=S.t_load, t_trace=tt["trace"], t_render=tt["render"], t_run=time.time() - t0)
