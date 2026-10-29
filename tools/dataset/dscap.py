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


def _colors(rec):
    """{room: [r,g,b] linear} from a variation record: gen2's {"colors": {...}, ...} or a plain mapping"""
    if not isinstance(rec, dict): return {}
    c = rec.get("colors") if "colors" in rec else rec
    return {k: v for k, v in c.items() if isinstance(v, (list, tuple)) and len(v) == 3} if isinstance(c, dict) else {}


def apply_walls(variation):
    """variation.walls / ceiling / floor colours (linear rgb per room): replace the base colour of that room's wall / ceiling / floor materials
    (texture, bump and roughness kept)"""
    done = {}; kept = []
    for part in ("walls", "ceiling", "floor"):
        suffix = "." + part.rstrip("s")
        for room, rgb in _colors((variation or {}).get(part)).items():
            n = 0
            for o in bpy.data.objects:
                if o.type != "MESH" or not o.name.startswith(room + suffix): continue
                # keep infinigen's wall alternatives (".alt": wainscoting, art wallpaper) and patterned finishes (tile, brick, wood):
                # painting them one flat colour removed the grout / brick / panel pattern; only plain plaster is repainted
                if part == "walls" and ".alt" in o.name: kept.append(o.name); continue
                for slot in o.material_slots:
                    m = slot.material
                    if m is None or not m.use_nodes: continue
                    if part == "walls" and "plaster" not in m.name.lower(): kept.append(m.name); continue
                    m = m.copy(); slot.material = m  # materials may be shared between rooms
                    for nd in m.node_tree.nodes:
                        if nd.type != "BSDF_PRINCIPLED": continue
                        s_ = nd.inputs["Base Color"]
                        for l in list(s_.links): m.node_tree.links.remove(l)
                        s_.default_value = (float(rgb[0]), float(rgb[1]), float(rgb[2]), 1.0); n += 1
            done[part + ":" + room] = n
    done["kept"] = sorted(set(kept))[:20]
    return done


# Blender sky texture: sun_rotation r puts the sun toward (sin r, -cos r) in world xy (measured with a shadow test, tools/dataset/README.md),
# i.e. compass bearing b (clockwise from +Y) = 180 deg - r.  gen2 records sun_dir (unit vector toward the sun, scene frame).
def sun_angles(lg):
    """-> (rotation rad, elevation rad) from variation.lighting (sun_dir preferred; else sun_azimuth_deg compass + sun_elevation_deg)"""
    if lg.get("sun_dir") is not None:
        x, y, z = lg["sun_dir"]; return math.pi - math.atan2(x, y), math.asin(max(-1.0, min(1.0, z)))
    az = lg.get("sun_azimuth_deg"); el = lg.get("sun_elevation_deg")
    return (None if az is None else math.pi - math.radians(az)), (None if el is None else math.radians(el))


def apply_lighting(sc, variation):
    """variation.lighting: sun_dir (or sun_azimuth_deg compass + sun_elevation_deg) -> sky sun rotation/elevation, sun_strength -> sky sun_intensity,
    sky_strength -> world Background strength, color_temp_K -> world colour tint (relative to 6500 K)"""
    lg = (variation or {}).get("lighting") or {}
    if not lg or sc.world is None or not sc.world.use_nodes: return {}
    nt = sc.world.node_tree
    sky = next((n for n in nt.nodes if n.type == "TEX_SKY"), None)
    bg = next((n for n in nt.nodes if n.type == "BACKGROUND"), None)
    ct = lg.get("color_temp_K", lg.get("color_temp_k"))
    if sky is not None:
        rot, el = sun_angles(lg)
        if rot is not None: sky.sun_rotation = rot
        if el is not None: sky.sun_elevation = el
        if lg.get("sun_strength") is not None: sky.sun_intensity = float(lg["sun_strength"])
    if bg is not None:
        if lg.get("sky_strength") is not None: bg.inputs["Strength"].default_value = float(lg["sky_strength"])
        if ct is not None:
            tint = _kelvin_rgb(float(ct))
            mix = nt.nodes.new("ShaderNodeMix"); mix.data_type = "RGBA"; mix.blend_type = "MULTIPLY"
            mix.inputs[0].default_value = 1.0  # factor
            src = bg.inputs["Color"].links[0].from_socket if bg.inputs["Color"].is_linked else None
            if src is not None: nt.links.new(src, mix.inputs[6])
            else: mix.inputs[6].default_value = tuple(bg.inputs["Color"].default_value)
            mix.inputs[7].default_value = (tint[0], tint[1], tint[2], 1.0)
            nt.links.new(mix.outputs[2], bg.inputs["Color"])
    return {k: lg.get(k) for k in ("sun_dir", "sun_elevation_deg", "sun_strength", "sky_strength")} | {"color_temp_K": ct}


def apply_hdri(sc, variation, hdri_dir, seed, k=4.0):
    """outdoor panorama (CC0 Poly Haven HDRIs in hdri_dir, meta.json from hdri_meta.py) for everything but diffuse bounces: window views and
    reflections show streets / gardens / skylines; room lighting stays on the calibrated procedural sky. Picked by time of day (night ->
    night set, sun below 12 deg -> sunset set, else urban 60 % / nature 30 % / overcast 10 %), rotated so its sun is where the scene's sun is,
    strength k * sky_strength / panorama mean sky."""
    import random
    lg = (variation or {}).get("lighting") or {}
    mp = os.path.join(hdri_dir, "meta.json")
    if not lg or sc.world is None or not sc.world.use_nodes or not os.path.exists(mp): return None
    meta = json.load(open(mp))
    rnd = random.Random(int(seed) * 7919 + 17)
    el = lg.get("sun_elevation_deg")
    if lg.get("is_night") or (el is not None and el < 0):
        groups = ["night"]
    elif el is not None and el < 12:
        groups = ["sunset"]
    else:
        u = rnd.random()
        groups = ["day_urban", "day_urban2"] if u < 0.6 else (["day_nature", "day_nature2"] if u < 0.9 else ["overcast"])
    cands = sorted(k_ for k_, v in meta.items() if v["group"] in groups)
    if not cands: return None
    name = rnd.choice(cands); m = meta[name]
    nt = sc.world.node_tree
    out = next((n for n in nt.nodes if n.type == "OUTPUT_WORLD"), None)
    if out is None or not out.inputs["Surface"].is_linked: return None
    sky_shader = out.inputs["Surface"].links[0].from_socket
    env = nt.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(os.path.join(hdri_dir, name + "_2k.hdr"), check_existing=True)
    tc = nt.nodes.new("ShaderNodeTexCoord"); mapn = nt.nodes.new("ShaderNodeMapping")
    # blender equirect: u = atan2(d.y, -d.x) / 2pi + 0.5 ; rotate the lookup so the scene's sun direction lands on the panorama's sun
    sd = lg.get("sun_dir")
    rz = 0.0
    if sd is not None and m.get("sun_u") is not None:
        phi_t = math.atan2(sd[1], -sd[0]); phi_0 = (m["sun_u"] - 0.5) * 2 * math.pi
        rz = phi_0 - phi_t
    mapn.inputs["Rotation"].default_value = (0.0, 0.0, rz)
    nt.links.new(tc.outputs["Generated"], mapn.inputs["Vector"]); nt.links.new(mapn.outputs[0], env.inputs["Vector"])
    bgh = nt.nodes.new("ShaderNodeBackground")
    strength = k * float(lg.get("sky_strength") or 1.0) / max(m["mean_sky"], 0.05)
    bgh.inputs["Strength"].default_value = float(np.clip(strength, 0.05, 20.0))
    nt.links.new(env.outputs["Color"], bgh.inputs["Color"])
    lp = nt.nodes.new("ShaderNodeLightPath"); mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(lp.outputs["Is Diffuse Ray"], mix.inputs[0])
    nt.links.new(bgh.outputs[0], mix.inputs[1]); nt.links.new(sky_shader, mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    return dict(hdri=name, group=m["group"], rot_z_deg=round(math.degrees(rz), 1), strength=round(bgh.inputs["Strength"].default_value, 3))


def lamp_state(variation, sidecar):
    """-> fn(light dict) -> (on, scale). gen2 record: variation.lighting.lamps = {light name: {on, watt}}, lamp_strength_scale multiplies the watt;
    capture-fallback record: lamps_on bool | [bool per non-ceiling lamp in sidecar order], ceiling_lights_on"""
    lg = (variation or {}).get("lighting") or {}
    per = lg.get("lamps") if isinstance(lg.get("lamps"), dict) else None
    lamps = [L for L in sidecar.get("lights", []) if "CeilingLight" not in L["name"]]
    idx = {L["name"]: i for i, L in enumerate(lamps)}
    lon = lg.get("lamps_on", True); cl = lg.get("ceiling_lights_on", True); sc_ = float(lg.get("lamp_strength_scale", 1.0))
    def f(L):
        if per is not None and L["name"] in per: return bool(per[L["name"]].get("on", True)), sc_
        if "CeilingLight" in L["name"]: return bool(cl), 1.0
        if isinstance(lon, bool) or isinstance(lon, int) and per is None: on = bool(lon)
        elif isinstance(lon, (list, tuple)) and lon: on = bool(lon[idx[L["name"]] % len(lon)])
        else: on = True
        return on, sc_
    return f


def switch_lamp_meshes(variation, sidecar):
    """emissive shaders of switched-off lamps / ceiling lights -> strength 0 (the point light is dropped in render_rust)"""
    fn = lamp_state(variation, sidecar)
    lg = (variation or {}).get("lighting") or {}
    rec = lg.get("lamps") if isinstance(lg.get("lamps"), dict) else {}  # gen2 drops switched-off lamps from `lights`; the record keeps them
    off = [L["name"] for L in sidecar.get("lights", []) if not fn(L)[0]] + [k for k, v in rec.items() if not v.get("on", True)]
    scale = {L["name"]: fn(L)[1] for L in sidecar.get("lights", [])}; scale.update({k: 1.0 for k in off})
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
        self.room = room
        objs = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render and len(o.data.polygons)]
        classes = {n["name"]: n.get("class", "") for n in sidecar.get("nodes", [])}
        V, T, OB, self.names, self.kind, self.cls, self.okeys, self.bbox = [], [], [], [], [], [], [], []
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
            self.okeys.append(re.sub(r"\.\d+$", "", o.name.split("#")[0]))
            wv = V[-1]; self.bbox.append((wv.min(0), wv.max(0)))
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

    def obj_class(self, key):
        """class part of an object key '<id>_<Class>' ('mess_<room>_<n>_<Class>', 'personN_...' for people)"""
        if key.startswith("person"): return "person"
        return key.rsplit("_", 1)[-1] if key.startswith("mess_") else key.split("_", 1)[-1]

    def keys_by_class(self, cl):
        c = cl.lower()
        return sorted({k for k, kd in zip(self.okeys, self.kind) if kd == 0 and (self.obj_class(k).lower() == c or c in self.obj_class(k).lower())})

    def children_of(self, keys, objclasses_skip=("Window", "Door")):
        """objects resting on the given objects (inferred when the sidecar has no parent info): bottom within 6 cm of an object's top and
        centre inside its footprint; transitive"""
        box = {}
        for k, kd, (lo, hi) in zip(self.okeys, self.kind, self.bbox):
            if kd != 0: continue
            b = box.setdefault(k, [lo.copy(), hi.copy()]); b[0] = np.minimum(b[0], lo); b[1] = np.maximum(b[1], hi)
        out = set(keys); grew = True
        while grew:
            grew = False
            for k, (lo, hi) in box.items():
                if k in out or any(skip in k for skip in objclasses_skip): continue
                c = (lo + hi) / 2
                for r in list(out):
                    rlo, rhi = box[r]
                    if abs(lo[2] - rhi[2]) < 0.06 and rlo[0] - 0.02 <= c[0] <= rhi[0] + 0.02 and rlo[1] - 0.02 <= c[1] <= rhi[1] + 0.02 and (hi[2] - lo[2]) < (rhi[2] - rlo[2]) + 1.0:
                        out.add(k); grew = True; break
        return sorted(out)

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


# the furniture a photo of each room type is "of" (3D-FRONT / Hypersim bedroom shots show the bed, living rooms the sofa, ...)
ANCHORS = {"bedroom": ("BedFactory",), "living-room": ("SofaFactory", "TVStandFactory"), "dining-room": ("TableDiningFactory",),
           "kitchen": ("KitchenSpaceFactory", "FridgeFactory", "KitchenIslandFactory"), "bathroom": ("BathtubFactory", "ToiletFactory", "BathroomSinkFactory", "StandingSinkFactory")}


def view_stats(S, c2w, f, W, H, grid=(24, 18), strict=True):
    """frustum probe: None if invalid else dict(med, p10, miss, wall, nobj, objfrac, person); composition: a view must show at least 4
    objects, at most 55 % bare wall / ceiling and no person filling more than 22 % of the frame or nearer than 1.6 m (people up close read as occluders)"""
    o = c2w[:3, 3]
    dirs = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (.7, .7, 0), (-.7, .7, 0), (.7, -.7, 0), (-.7, -.7, 0),
            (.7, 0, .7), (-.7, 0, .7), (0, .7, .7), (0, -.7, .7), (.7, 0, -.7), (-.7, 0, -.7), (0, .7, -.7), (0, -.7, -.7)]
    for d in dirs:
        h = S.cast(o, d, 0.3)
        if h is not None and (h[3] < 0.22 or (S.tri_kind[h[2]] == 0 and np.dot(h[1], d) > 0)): return None, "eye-in-object" if S.tri_kind[h[2]] == 0 else "eye-near-wall"
    # the photographer's feet: straight down from the eye must reach the floor (or a rug), not a table top or bed
    hd = S.cast(o, (0.0, 0.0, -1.0))
    if hd is not None and S.tri_kind[hd[2]] == 0 and hd[3] < (o[2] - S.floor_z) - 0.12 and "Rug" not in S.okeys[S.tri_obj[hd[2]]]:
        return None, "over-furniture"
    gw, gh = grid; ray = rays_cv(W, H, f)
    ys = np.linspace(0, ray.shape[0] - 1, gh).astype(int); xs = np.linspace(0, ray.shape[1] - 1, gw).astype(int)
    ds = []; miss = 0; wall = 0; objs = set(); nobjpix = 0; person = 0; anchor = 0; pnear = 1e9; per = {}; near_low = 0; n_low = 0; near_obj = {}; pend = 0; edge_n = [0, 0]; edge_near = [0, 0]; door = 0; win = 0
    anc = next((v for k, v in ANCHORS.items() if getattr(S, "room", "").startswith(k)), ())
    for y in ys:
        for x in xs:
            d = c2w[:3, :3] @ ray[y, x]; n = np.linalg.norm(d); h = S.cast(o, d / n)
            if h is None: miss += 1; continue
            ds.append(h[3]); kd = S.tri_kind[h[2]]; wall += kd in (1, 3)
            if y >= ys[len(ys) * 2 // 3]:
                n_low += 1; near_low += (kd == 0 and h[3] < 1.1)
            if h[3] < 1.1:
                ko = S.okeys[S.tri_obj[h[2]]] if kd == 0 else "_wall"
                near_obj[ko] = near_obj.get(ko, 0) + 1
            if kd == 0 and h[3] < 1.2 and S.okeys[S.tri_obj[h[2]]].startswith("pendant"): pend += 1
            # side strips (outer 20 % columns): a wall edge / door leaf / wardrobe side right beside the lens
            if x <= xs[len(xs) // 5] or x >= xs[len(xs) - 1 - len(xs) // 5]:
                side = 0 if x <= xs[len(xs) // 5] else 1
                edge_n[side] += 1; edge_near[side] += h[3] < 0.8
            if kd == 0:
                k = S.okeys[S.tri_obj[h[2]]]
                if ("Door" in k or k.startswith("door")) and h[3] < 2.0: door += 1
                if "Window" in k or k.startswith("window"): win += 1
                if not ("Window" in k or "Door" in k or k.startswith("window") or k.startswith("door")):
                    objs.add(k); nobjpix += 1; per[k] = per.get(k, 0) + 1
                    if k.startswith("person"):
                        person += 1; pnear = min(pnear, h[3])
                    anchor += any(a in k for a in anc)
    n = gw * gh
    if not ds: return None, "all-miss"
    # distinct classes ("131945_BookColumnFactory" -> BookColumnFactory): a bookcase of 60 book columns is one kind of thing, not 60
    ncls = len({re.sub(r"^\d+_", "", k).split(".")[0] for k in objs})
    st = dict(med=float(np.median(ds)), p10=float(np.percentile(ds, 10)), miss=miss / n, wall=wall / n, nobj=len(objs), ncls=ncls, objfrac=nobjpix / n, person=person / n, anchor=anchor / n, near_low=near_low / max(n_low, 1), win=win / n)
    if st["p10"] < 0.35: return None, "too-close"
    # a big object 30-60 cm in front of the lens over the lower third reads as an obstruction (sofa cushions filling the frame)
    if strict and st.get("near_low", 1.0) > 0.3: return None, "foreground-blocked"
    if strict and max(near_obj.values(), default=0) / n > 0.16: return None, "near-object-fills-frame"  # a door leaf / cabinet side 0.5 m from the lens
    if st["med"] < float(np.clip(0.38 * math.sqrt(max(S.floor_area, 1.0)), 1.0, 1.3)): return None, "near-median"
    if st["miss"] > 0.2: return None, "sees-void"
    if st["wall"] > 0.45: return None, "mostly-wall"
    if st["ncls"] < 5: return None, "few-classes"
    if pend: return None, "pendant-at-eye"
    # rooms with an anchor piece (bed, sofa, dining table, counters, bath fixtures): every view shows some of it (>= 3 % of the frame)
    if anc and getattr(S, "has_anchor", None) is None:
        S.has_anchor = any(any(a_ in k for a_ in anc) for k in S.okeys)
    if anc and S.has_anchor and anchor / n < 0.03: return None, "no-anchor"
    if strict and any(edge_n[i] and edge_near[i] / edge_n[i] > 0.6 for i in (0, 1)): return None, "edge-occluded"
    if st["nobj"] < 4: return None, "few-objects"
    if st["person"] > 0.15: return None, "person-fills-frame"
    if max(per.values(), default=0) / n > 0.3: return None, "one-object-fills-frame"  # a fridge door is not a view of the kitchen
    # an open door leaf beside the lens (doorway views of kitchens were half door, half fridge side)
    if door / n > 0.12: return None, "door-fills-frame"
    if door / n + max(per.values(), default=0) / n > 0.4: return None, "door-and-object-fill-frame"
    if pnear < 1.6: return None, "person-too-close"  # a face filling the middle of the frame reads as a portrait, not a room
    return st, "ok"


def view_score(st):
    """composition score of a valid view: many kinds of objects and furniture coverage, little bare wall (raw object counts made views
    of book-filled shelves win every time)"""
    # + the room's anchor furniture in frame (bonus saturates at 15 % of the frame), - people (they crowded every kitchen frame)
    return 1.5 * st.get("ncls", st["nobj"]) + 0.3 * min(st["nobj"], 60) + 12.0 * st["objfrac"] - 6.0 * st["wall"] + 40.0 * min(st.get("anchor", 0.0), 0.15) - 45.0 * st.get("person", 0.0) + 15.0 * min(st.get("win", 0.0), 0.2)  # daylight through a window (Hypersim frames face them)


def white_balance(path, strength=0.75):
    """camera white balance on the written PNG: partial gray-world in linear light (warm 2200-3000 K lamps, sunset skies and peach
    walls left frames orange: mean R/B 1.79 against Hypersim's 1.18); gains are normalised to keep the luminance; returns the gains"""
    from PIL import Image
    im = np.asarray(Image.open(path).convert("RGB"), np.float32) / 255.0
    lin = np.where(im <= 0.04045, im / 12.92, ((im + 0.055) / 1.055) ** 2.4)
    m = lin.reshape(-1, 3).mean(0) + 1e-6
    g = (m.mean() / m) ** strength
    g = g / (g @ np.array([0.2126, 0.7152, 0.0722]))  # keep luminance
    lin = np.clip(lin * g, 0, 1)
    out = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * lin ** (1 / 2.4) - 0.055)
    # mid-tone lift (a photographer's curves): the exposure is held down by sunlit windows (blow-out guard), leaving rooms darker
    # than Hypersim's (L* 42 vs 53); a gamma toward mean display luminance 0.5 keeps the highlights unclipped
    y = float((out @ np.array([0.2126, 0.7152, 0.0722])).mean())
    gam = float(np.clip(math.log(0.5) / math.log(max(min(y, 0.99), 1e-3)), 0.70, 1.0))
    out = np.clip(out, 0, 1) ** gam
    # chroma x0.85 around luminance (procedural paints / woods / fabrics are more saturated than Hypersim's: 0.29 vs 0.20)
    lum = (out @ np.array([0.2126, 0.7152, 0.0722]))[..., None]
    out = np.clip(lum + (out - lum) * 0.85, 0, 1)
    Image.fromarray((out * 255 + 0.5).astype(np.uint8)).save(path)
    return [round(float(x), 3) for x in g] + [round(gam, 3)]


def auto_exposure(sc, base, tmp, target=0.52, lo=-2.0, hi=4.0):  # Hypersim frames average L* 53 (ours were 45-48)
    """camera auto exposure: a tiny, few-sample pre-render (1/8 size) measures the mean display luminance, the film exposure is shifted
    by log2(target / mean) stops (clamped) and re-measured once; returns the exposure set for the real render"""
    pct, smp, den = sc.render.resolution_percentage, sc.cycles.samples, sc.cycles.use_denoising
    fp = sc.render.filepath
    sc.render.resolution_percentage = 12; sc.cycles.samples = 4; sc.cycles.use_denoising = False; sc.render.filepath = tmp
    ev = base
    try:
        from PIL import Image
        for _ in range(2):
            sc.cycles.film_exposure = ev
            bpy.ops.render.render(write_still=True)
            a = np.asarray(Image.open(tmp).convert("RGB"), dtype=np.float32) / 255.0
            y = 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]
            lum = float(np.mean(y))
            step = math.log2(target / max(lum, 1e-3)) * 0.8
            # sunlit patches: keep blown-out (near white) pixels under ~3% of the frame even if the mean stays a little dark
            clip = float(np.mean(y > 0.97))
            if clip > 0.03:
                step = min(step, -0.35 - 2.0 * (clip - 0.03))
            ev = float(np.clip(ev + step, base + lo, base + hi))
            if abs(step) < 0.15: break
    finally:
        sc.render.resolution_percentage, sc.cycles.samples, sc.cycles.use_denoising, sc.render.filepath = pct, smp, den, fp
    sc.cycles.film_exposure = ev
    return ev


def make_pool(S, cfg, W, H, n, rng, strict=True):
    """n candidate views: first of each trajectory/cluster is a fresh look-at-the-room pose, the rest either small trajectory steps
    (0.3-0.7 m, yaw +-20 deg) or new positions aimed at the previous view's centre hit point (overlap)"""
    lo_f, hi_f = cfg.get("focal_mm", [18, 35]); hlo, hhi = cfg.get("eye_height_m", [1.0, 1.8]); margin = cfg.get("wall_margin_m", 0.45)
    # small rooms (10 m2 kitchens, 3 m wide bedrooms): photographers stand closer to walls and targets
    R = math.sqrt(max(S.floor_area, 1.0)); margin = min(margin, max(0.25, 0.12 * R))
    # lens: Hypersim's cameras are near-normal (~ 24-35 mm full-frame equivalent); 14-18 mm bowed ceilings and stretched walls in a third
    # of our frames. Tiny rooms (< 10 m2) may go to 20 mm; LENS_MIN overrides
    lo_f = max(lo_f, float(os.environ.get("LENS_MIN", 24.0))); hi_f = max(hi_f, lo_f + 6.0)
    if S.floor_area < 10.0: lo_f, hi_f = min(lo_f, 20.0), min(hi_f, 28.0)
    elif S.room.startswith("bedroom"):
        hlo = max(hlo, 1.35)  # standing height (low cameras looked at floor clutter over the bed edge)
    d_tgt = float(np.clip(0.5 * R, 1.2, 2.0))
    traj, cluster = cfg.get("traj_frac", 0.4), cfg.get("cluster_len", 3)
    pool, rejects, prev, run = [], {}, None, 0
    tries = 0
    # furniture targets for fresh views: object bounding-box centres in the room (weighted by footprint, so a sofa is aimed at more often
    # than a cup)
    rlo, rhi = S.floor_tris.reshape(-1, 3).min(0), S.floor_tris.reshape(-1, 3).max(0)
    targets, tw = [], []
    for k, kd, (lo, hi) in zip(S.okeys, S.kind, S.bbox):
        if kd != 0 or k.startswith("person") or "Window" in k or "Door" in k: continue
        c = (lo + hi) / 2
        if not (rlo[0] - 0.1 <= c[0] <= rhi[0] + 0.1 and rlo[1] - 0.1 <= c[1] <= rhi[1] + 0.1): continue
        fp = (hi[0] - lo[0]) * (hi[1] - lo[1])
        # furniture-sized things only: the dozens of book columns inside one shelf summed to more weight than the bed
        if fp < 0.12: continue
        anc = next((v for kk, v in ANCHORS.items() if S.room.startswith(kk)), ())
        targets.append(c); tw.append(fp * (4.0 if any(a in k for a in anc) else 1.0))
    tw = np.array(tw) / np.sum(tw) if tw else None
    best_of = cfg.get("fresh_candidates", 8)
    fresh_best = []
    while len(pool) < n and tries < n * 400:
        tries += 1
        # still collecting best-of fresh candidates counts as fresh
        fresh = prev is None or run >= cluster or bool(fresh_best)
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
                if targets and rng.rand() < 0.85:
                    tgt = np.array(targets[rng.choice(len(targets), p=tw)], dtype=float); tgt[2] = float(np.clip(tgt[2], S.floor_z + 0.3, S.floor_z + 1.5))
                    # stand back near the room's edge (0.45-1.3 m from the walls), looking across the room at the target
                    for _ in range(20):
                        p2 = S.sample_floor(rng)
                        if S.edge_dist(p2[:2]) < 1.3 and np.linalg.norm(tgt[:2] - p2[:2]) > d_tgt: break
                    pos = np.array([p2[0], p2[1], 0.0])
                else:
                    q = S.sample_floor(rng); tgt = np.array([q[0], q[1], rng.uniform(0.4, 1.4) + S.floor_z])
                if np.linalg.norm(tgt[:2] - pos[:2]) < min(1.5, d_tgt): continue
            f = float(math.exp(rng.uniform(math.log(lo_f), math.log(hi_f))))
        if kind == "traj":
            pos[2] = float(np.clip(pos[2], S.floor_z + hlo, S.floor_z + hhi))
        else:
            pos[2] = S.floor_z + rng.uniform(hlo, hhi)
            dv = tgt - pos; yaw = math.atan2(dv[1], dv[0]) + math.radians(rng.normal(0, 10))
            pitch = math.atan2(dv[2], np.linalg.norm(dv[:2]) + 1e-9)
        pitch = float(np.clip(pitch, math.radians(-22), math.radians(10)))
        if S.edge_dist(pos[:2]) < margin:
            rejects["wall-margin"] = rejects.get("wall-margin", 0) + 1; continue
        M = cam_pose_cv(pos, yaw, pitch); fpx = f / 36.0 * W
        st, why = view_stats(S, M, fpx, W, H, strict=strict)
        if st is None:
            rejects[why] = rejects.get(why, 0) + 1; continue
        d = M[:3, 2]; h = S.cast(pos, d); hit = np.array(h[0]) if h else pos + d * st["med"]
        v = dict(pos=pos, yaw=yaw, pitch=pitch, f_mm=f, kind=kind, hit=hit, stats=st)
        if kind == "fresh":
            # best of `best_of` valid fresh candidates by composition
            fresh_best.append(v)
            # tight rooms pass few candidates: settle for fewer later on (waiting for 8 discarded every valid view of a 3.5 m bedroom)
            need = best_of if tries < n * 100 else (2 if tries < n * 250 else 1)
            if len(fresh_best) < need and tries < n * 400 - 1: continue
            # and away from what earlier fresh views looked at (otherwise one big shelf collects most views)
            # ... and from where they stood / which way they faced (in a narrow room every hit lands on the same end wall)
            seen = [u for u in pool if u["kind"] == "fresh"]
            def nov(c):
                pen = 0.0
                for u in seen:
                    pen += 4.0 * math.exp(-float(np.sum((c["hit"][:2] - u["hit"][:2]) ** 2)) / 1.5)
                    dyaw = abs((c["yaw"] - u["yaw"] + math.pi) % (2 * math.pi) - math.pi)
                    pen += 4.0 * math.exp(-float(np.sum((c["pos"][:2] - u["pos"][:2]) ** 2)) / 1.0) * math.exp(-(dyaw / 0.6) ** 2)
                return -pen
            v = max(fresh_best, key=lambda c: view_score(c["stats"]) + nov(c)); fresh_best = []
        # the run restarts when a fresh view is taken (resetting it when one was drawn let rejected candidates restart the run)
        pool.append(v); prev = v; run = 1 if v["kind"] == "fresh" else run + 1
    return pool, rejects, tries


def pool_to_json(pool):
    return [dict(pos=list(map(float, v["pos"])), yaw=v["yaw"], pitch=v["pitch"], f_mm=v["f_mm"], kind=v["kind"], hit=list(map(float, v["hit"])), stats=v["stats"], relaxed=bool(v.get("relaxed"))) for v in pool]


def pool_from_json(js):
    return [dict(pos=np.array(v["pos"]), yaw=v["yaw"], pitch=v["pitch"], f_mm=v["f_mm"], kind=v["kind"], hit=np.array(v["hit"]), stats=v.get("stats"), relaxed=bool(v.get("relaxed"))) for v in js]


# ------------------------------------------------------------------------------------------------ depth / normals / ids
def trace(S, c2w, f, W, H, bvh=None, tri_map=None):
    """z-depth (H,W) float32 (0 = no surface), world normals (H,W,3) float16 facing the camera, instance ids (H,W) uint16 (0 none, k = object k-1)"""
    ray = rays_cv(W, H, f).reshape(-1, 3)
    dw = ray @ c2w[:3, :3].T; nrm = np.linalg.norm(dw, axis=1); dwn = (dw / nrm[:, None])
    o = tuple(c2w[:3, 3]); rc = (bvh or S.bvh).ray_cast
    N = len(dwn); depth = np.zeros(N, np.float32); normal = np.zeros((N, 3), np.float32); tri = np.full(N, -1, np.int32)
    dl = dwn.tolist(); far = 100.0
    for i in range(N):
        h = rc(o, dl[i], far)
        if h[0] is None: continue
        depth[i] = h[3] / nrm[i]; normal[i] = h[1]; tri[i] = h[2]
    hit = tri >= 0
    if tri_map is not None: tri[hit] = tri_map[tri[hit]]
    flip = (normal * dwn).sum(1) > 0; normal[flip] *= -1
    inst = np.zeros(N, np.uint16); inst[hit] = S.tri_obj[tri[hit]] + 1
    depth[depth < 0.05] = 0
    return depth.reshape(H, W), normal.reshape(H, W, 3).astype(np.float16), inst.reshape(H, W)


def write_exr(path, arr):
    import OpenEXR, Imath
    h, w = arr.shape
    hd = OpenEXR.Header(w, h); hd["channels"] = {"Z": Imath.Channel(Imath.PixelType(Imath.PixelType.FLOAT))}
    f = OpenEXR.OutputFile(path, hd); f.writePixels({"Z": arr.astype(np.float32).tobytes()}); f.close()


# ------------------------------------------------------------------------------------------------ object removal
def resolve_removal(S, rem, K, sidecar):
    """rem = {keys, classes, random: {count, classes, seed}, with_children, views: "all" | "random_subset" | [indices], views_fraction, views_count, seed}
    -> (sorted removed object keys, set of view indices that render without them)"""
    if not rem: return [], set()
    objs = sidecar.get("objects")  # factors worker's list {key, class, parent, room}; else inferred from names / geometry
    allk = sorted({k for k, kd in zip(S.okeys, S.kind) if kd == 0})
    keys = set(k for k in rem.get("keys", []) if k in allk)
    for c in rem.get("classes", []): keys.update(S.keys_by_class(c))
    rnd = rem.get("random")
    if rnd:
        r = np.random.RandomState(int(rnd.get("seed", 0)))
        cand = sorted({k for c in (rnd.get("classes") or [None]) for k in (S.keys_by_class(c) if c else allk)})
        cand = [k for k in cand if not any(x in k for x in ("Window", "Door", "CeilingLight", "Light")) or rnd.get("classes")]
        # visible-sized things only (bbox diagonal > 0.25 m)
        diag = {}
        for k, kd, (lo, hi) in zip(S.okeys, S.kind, S.bbox):
            if kd == 0: d = diag.get(k); diag[k] = max(d or 0.0, float(np.linalg.norm(hi - lo)))
        cand = [k for k in cand if diag.get(k, 0) > 0.25]
        n = min(int(rnd.get("count", 1)), len(cand))
        keys.update(r.choice(cand, n, replace=False).tolist() if n else [])
    keys = sorted(keys)
    if rem.get("with_children", True) and keys:
        if objs:
            par = {o["key"]: o.get("parent") for o in objs}; grew = True; ks = set(keys)
            while grew:
                grew = False
                for k, pk in par.items():
                    if pk in ks and k not in ks: ks.add(k); grew = True
            keys = sorted(ks)
        else:
            keys = S.children_of(keys)
    vs = rem.get("views", "all")
    if vs == "all": views = set(range(K))
    elif vs == "random_subset":
        r = np.random.RandomState(int(rem.get("seed", 0)) + 1)
        n = int(rem["views_count"]) if "views_count" in rem else max(1, int(round(float(rem.get("views_fraction", 0.5)) * K)))
        views = set(r.choice(K, min(n, K), replace=False).tolist())
    else: views = set(int(i) for i in vs if 0 <= int(i) < K)
    return keys, views


# ------------------------------------------------------------------------------------------------ main entry
def run(sc, sidecar, spec, log=print):
    """render all views of this variant. spec keys: out_dir, room, width, height, samples, n_views, camera_seed, cameras{...},
    pool_path, depth_format (npy|exr|both), extras [normal, instance], variation"""
    out = spec["out_dir"]; W, H = int(spec["width"]), int(spec["height"]); K = int(spec["n_views"]); ccfg = spec.get("cameras", {})
    for d in ("images", "depth"): os.makedirs(os.path.join(out, d), exist_ok=True)
    extras = spec.get("extras", ["normal", "instance"])
    for d in extras: os.makedirs(os.path.join(out, d), exist_ok=True)
    # outputs of an earlier run in the same directory would otherwise survive next to this run's cameras / instances.json
    for d in ["images", "depth", "depth_full", "removed_mask"] + list(extras):
        dd = os.path.join(out, d)
        if os.path.isdir(dd):
            for f in os.listdir(dd): os.remove(os.path.join(dd, f))
    t0 = time.time(); bpy.context.view_layer.update()
    S = Scene(spec["room"], sidecar)
    log("DSCAP scene tris=%d objs=%d floor_area=%.1f m2 bvh+load %.1fs" % (len(S.T), len(S.names), S.floor_area, S.t_load))
    pp = spec["pool_path"]; pool_new = False
    pool = None
    if os.path.exists(pp):
        pool = pool_from_json(json.load(open(pp))["views"])
        # a pool from an older scene / older rules where no view is valid any more: build a new one
        if spec.get("variant") in (None, "base") and not any(view_stats(S, cam_pose_cv(v["pos"], v["yaw"], v["pitch"]), v["f_mm"] / 36.0 * W, W, H, strict=not v.get("relaxed"))[0] for v in pool):
            log("DSCAP stored pool has no valid view for this scene: rebuilding"); pool = None
    if pool is None:
        rng = np.random.RandomState(int(spec["camera_seed"]))
        pool, rej, tries = make_pool(S, ccfg, W, H, int(ccfg.get("pool_factor", 3)) * K, rng)
        if len(pool) < K:
            # cramped room: too few views pass; retry with the distance rules relaxed (foreground / near object / edge), still requiring
            # the room's anchor piece and 5 object classes -- a house with no acceptable view gets fewer (or no) views, not bad ones
            more, rej2, tries2 = make_pool(S, ccfg, W, H, int(ccfg.get("pool_factor", 3)) * K - len(pool), np.random.RandomState(int(spec["camera_seed"]) + 7919), strict=False)
            log("DSCAP strict pool %d views; relaxed fallback added %d" % (len(pool), len(more)))
            for v in more: v["relaxed"] = True
            pool = pool + more; tries += tries2; rej = dict(rej, relaxed=rej2)
        os.makedirs(os.path.dirname(pp), exist_ok=True)
        json.dump(dict(camera_seed=spec["camera_seed"], width=W, height=H, views=pool_to_json(pool), rejects=rej, tries=tries, room=spec["room"], made_by=spec.get("variant")), open(pp, "w"), indent=1)
        pool_new = True; log("DSCAP pool %d views from %d tries, rejects %s" % (len(pool), tries, rej))
    if spec.get("pool_only"): return dict(n_views=0, pool_new=pool_new, pool=len(pool), t_scene=S.t_load, t_run=time.time() - t0)
    # clusters (a fresh view + the steps that follow it) best composition first: with few views per house the first cluster drawn
    # (say a view of bare floor) would otherwise always be the one rendered
    clusters, cur = [], []
    for j, v in enumerate(pool):
        if v["kind"] == "fresh" and cur: clusters.append(cur); cur = []
        cur.append(j)
    if cur: clusters.append(cur)
    sc_ = lambda c: view_score(pool[c[0]]["stats"]) if pool[c[0]].get("stats") else 0.0
    order = [j for c in sorted(clusters, key=lambda c: -sc_(c)) for j in c]
    # validity per variant (furniture changed): first K valid in that order
    chosen, dropped = [], []
    for j in order:
        v = pool[j]
        if len(chosen) >= K: break
        M = cam_pose_cv(v["pos"], v["yaw"], v["pitch"]); st, why = view_stats(S, M, v["f_mm"] / 36.0 * W, W, H, strict=not v.get("relaxed"))
        (chosen if st is not None else dropped).append((j, why) if st is None else (j, st))
    log("DSCAP views chosen %s dropped %s" % ([j for j, _ in chosen], dropped))
    cam = bpy.data.cameras.new("dscam"); cam.sensor_fit = "HORIZONTAL"; cam.sensor_width = 36.0; cam.clip_start = 0.05; cam.clip_end = 100.0
    co = bpy.data.objects.new("dscam", cam); sc.collection.objects.link(co); sc.camera = co
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = W, H, 100
    sc.render.image_settings.file_format = "PNG"; sc.render.image_settings.color_mode = "RGB"; sc.render.image_settings.color_depth = "8"
    sc.render.use_persistent_data = True
    views, tt = [], dict(trace=0.0, render=0.0)
    base_exposure = sc.cycles.film_exposure; ae_log = []; lights_on_log = []; wb_log = []
    # photographic tone: AgX with medium-high contrast (the base look was flat / washed out); spec "look" overrides
    try:
        sc.view_settings.view_transform = "AgX"; sc.view_settings.look = spec.get("look", "AgX - Medium High Contrast")
    except Exception as e:
        log("DSCAP look not set: %r" % (e,))
    rem_keys, rem_views = resolve_removal(S, spec.get("remove"), len(chosen), sidecar)
    bvh2 = tri_map2 = None; rem_objs = []
    if rem_keys:
        ks = set(rem_keys); rm = np.array([S.okeys[i] in ks and S.kind[i] == 0 for i in range(len(S.names))])
        keep = np.nonzero(~rm[S.tri_obj])[0]; tri_map2 = keep.astype(np.int32)
        bvh2 = BVHTree.FromPolygons(S.V.tolist(), S.T[keep].tolist())
        rem_ids = set(np.nonzero(rm)[0].tolist())
        rem_objs = [o for o in bpy.data.objects if o.type in ("MESH", "LIGHT") and re.sub(r"\.\d+$", "", o.name.split("#")[0]) in ks]
        os.makedirs(os.path.join(out, "depth_full"), exist_ok=True); os.makedirs(os.path.join(out, "removed_mask"), exist_ok=True)
        log("DSCAP remove %d objects %s in views %s (%d blender objects)" % (len(rem_keys), rem_keys[:12], sorted(rem_views), len(rem_objs)))
    for k, (j, st) in enumerate(chosen):
        v = pool[j]; M = cam_pose_cv(v["pos"], v["yaw"], v["pitch"]); fpx = v["f_mm"] / 36.0 * W
        co.matrix_world = mathutils.Matrix((M @ BL2CV).tolist()); cam.lens = v["f_mm"]
        bpy.context.view_layer.update()
        t = time.time(); depth, nrm, inst = trace(S, M, fpx, W, H); tt["trace"] += time.time() - t
        nm = "%03d" % k
        removed_here = bool(rem_keys) and k in rem_views
        if rem_keys:
            from PIL import Image
            np.save(os.path.join(out, "depth_full", nm + ".npy"), depth)  # the full scene: what was there
            mask = np.isin(inst, np.array(sorted(i + 1 for i in rem_ids))) if removed_here else np.zeros(depth.shape, bool)
            Image.fromarray((mask * 255).astype(np.uint8)).save(os.path.join(out, "removed_mask", nm + ".png"))
            if removed_here:  # what the camera sees without them
                depth, nrm, inst = trace(S, M, fpx, W, H, bvh2, tri_map2)
        for o in rem_objs: o.hide_render = removed_here
        if spec.get("depth_format", "npy") in ("npy", "both"): np.save(os.path.join(out, "depth", nm + ".npy"), depth)
        if spec.get("depth_format", "npy") in ("exr", "both"): write_exr(os.path.join(out, "depth", nm + ".exr"), depth)
        if "normal" in extras: np.save(os.path.join(out, "normal", nm + ".npy"), nrm)
        if "instance" in extras:
            from PIL import Image
            Image.fromarray(inst).save(os.path.join(out, "instance", nm + ".png"))
        sc.render.filepath = os.path.join(out, "images", nm + ".png")
        t = time.time()
        if spec.get("auto_exposure", True):
            ev = auto_exposure(sc, base_exposure, os.path.join(out, "_ae.png"))
            off = [o for o in bpy.data.objects if o.type == "LIGHT" and o.get("ig_off")]
            switched = False
            if ev >= base_exposure + 4.0 - 1e-3 and off:
                # too dark even at the exposure cap (lamps drawn off, small window): the photographer switches the lights on
                for o in off: o.hide_render = False
                ev = auto_exposure(sc, base_exposure, os.path.join(out, "_ae.png")); switched = True
            ae_log.append(round(ev - base_exposure, 2)); lights_on_log.append(switched)
        bpy.ops.render.render(write_still=True); tt["render"] += time.time() - t
        if spec.get("white_balance", 0.75) > 0: wb_log.append(white_balance(sc.render.filepath, float(spec.get("white_balance", 0.75))))
        for o in [o for o in bpy.data.objects if o.type == "LIGHT" and o.get("ig_off")]: o.hide_render = True
        w2c = np.linalg.inv(M)
        views.append(dict(index=k, pool_index=j, kind=v["kind"], file="images/%s.png" % nm, depth="depth/%s.npy" % nm, width=W, height=H,
                          K=[[fpx, 0, W / 2], [0, fpx, H / 2], [0, 0, 1]], focal_mm=v["f_mm"], sensor_width_mm=36.0,
                          w2c=w2c.tolist(), c2w=M.tolist(), near=0.05, far=100.0, valid_frac=float((depth > 0).mean()),
                          depth_median=float(np.median(depth[depth > 0])) if (depth > 0).any() else 0.0))
        if rem_keys:
            views[-1].update(removed_keys=rem_keys if removed_here else [], removed_classes=sorted({S.obj_class(x) for x in rem_keys}) if removed_here else [],
                             depth_full="depth_full/%s.npy" % nm, removed_mask="removed_mask/%s.png" % nm, removed_frac=float(mask.mean()))
        log("DSCAP view %s pool %d trace %.1fs render %.1fs valid %.2f" % (nm, j, tt["trace"], tt["render"], views[-1]["valid_frac"]))
    json.dump(dict(convention="opencv: x right, y down, z forward; w2c maps world->camera; world z up, metres; depth = z-depth along the optical axis, 0 = no surface",
                   views=views, dropped_pool_views=[dict(pool_index=j, reason=r) for j, r in dropped]), open(os.path.join(out, "cameras.json"), "w"), indent=1)
    if "instance" in extras:
        json.dump({str(i + 1): dict(name=S.names[i], cls=S.cls[i], kind=["object", "wall", "floor", "ceiling"][S.kind[i]]) for i in range(len(S.names))}, open(os.path.join(out, "instances.json"), "w"))
    for o in rem_objs: o.hide_render = False
    if os.path.exists(os.path.join(out, "_ae.png")): os.remove(os.path.join(out, "_ae.png"))
    sc.cycles.film_exposure = base_exposure
    return dict(removed_keys=rem_keys, removed_views=sorted(rem_views), n_views=len(views), pool_new=pool_new, dropped=len(dropped), t_scene=S.t_load, t_trace=tt["trace"], t_render=tt["render"], t_run=time.time() - t0, exposure_stops=ae_log, lights_switched_on=lights_on_log, white_balance_gains=wb_log)
