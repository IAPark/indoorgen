"""Render a Rust-generated scene (glb + sidecar json) with Infinigen's own materials/lighting.
usage: render_rust.py scene.glb out.png --cam camera.json [--samples N] [--top]"""
import os, sys, json, math, time, argparse
import bpy, mathutils
ap = argparse.ArgumentParser()
ap.add_argument("glb"); ap.add_argument("out")
ap.add_argument("--cam"); ap.add_argument("--samples", type=int, default=16); ap.add_argument("--top", action="store_true"); ap.add_argument("--showcase", action="store_true")
ap.add_argument("--seed", type=int, default=0); ap.add_argument("--clay", action="store_true"); ap.add_argument("--room"); ap.add_argument("--light-scale", type=float, default=1.0); ap.add_argument("--view"); ap.add_argument("--norender", action="store_true"); ap.add_argument("--dataset", help="tools/dataset capture spec json: multi-view RGB+depth+cameras for one variant (see tools/dataset/README.md)"); ap.add_argument("--no-cloth", action="store_true", help="keep the Rust cloth approximation instead of re-simulating in Blender")
ap.add_argument("--exposure", type=float, default=3.0, help="Cycles film exposure (3.0 suits closed rooms lit by the scene lights; open test layouts such as featureshot grids need ~0.5-1)")
args = ap.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
_T0 = [time.time()]
def phase(name):
    """PHASES=1: wall time of each setup phase"""
    if os.environ.get("PHASES"):
        t = time.time(); print("PHASE %-14s %6.1fs" % (name, t - _T0[0]), flush=True); _T0[0] = t
def _dumpsig(path):
    # per-object scene data as it would render, for comparing import paths (tools/render/sigcmp.py): world positions, and per face
    # corner (sorted by corner position + face centre) world normal and uv; materials and smoothing counts in a json beside it
    import numpy as _np
    arrs, meta = {}, {}
    for o in bpy.data.objects:
        if o.type != 'MESH': continue
        me = o.data
        M = _np.array(o.matrix_world); R = M[:3, :3]
        co = _np.zeros(3 * len(me.vertices)); me.vertices.foreach_get("co", co); co = co.reshape(-1, 3) @ R.T + M[:3, 3]
        lv = _np.zeros(len(me.loops), dtype=_np.int64); me.loops.foreach_get("vertex_index", lv)
        cn = _np.zeros(3 * len(me.loops)); me.corner_normals.foreach_get("vector", cn); cn = cn.reshape(-1, 3) @ R.T
        cn /= _np.maximum(_np.linalg.norm(cn, axis=1, keepdims=True), 1e-12)
        uv = _np.zeros(2 * len(me.loops))
        if me.uv_layers.active: me.uv_layers.active.data.foreach_get("uv", uv)
        uv = uv.reshape(-1, 2)
        ls = _np.zeros(len(me.polygons), dtype=_np.int64); me.polygons.foreach_get("loop_start", ls)
        lt = _np.zeros(len(me.polygons), dtype=_np.int64); me.polygons.foreach_get("loop_total", lt)
        fidx = _np.repeat(_np.arange(len(me.polygons)), lt)
        cent = _np.zeros((len(me.polygons), 3)); _np.add.at(cent, fidx, co[lv]); cent /= _np.maximum(lt, 1)[:, None]
        key = _np.round(_np.c_[co[lv], cent[fidx]], 5)
        order = _np.lexsort(key.T[::-1]) if len(key) else _np.zeros(0, dtype=_np.int64)
        pv = _np.round(co, 5); pv = co[_np.lexsort(pv.T[::-1])] if len(co) else co
        arrs[o.name + "|pos"] = pv; arrs[o.name + "|cn"] = cn[order]; arrs[o.name + "|uv"] = uv[order]
        sm = _np.zeros(len(me.polygons), dtype=bool); me.polygons.foreach_get("use_smooth", sm)
        se = me.attributes.get("sharp_edge")
        meta[o.name] = dict(nv=len(me.vertices), nf=len(me.polygons), mats=[m.name if m else None for m in me.materials], smooth=int(sm.sum()),
                            sharp=int(sum(1 for d in se.data if d.value)) if se else 0, uvname=me.uv_layers.active.name if me.uv_layers.active else None)
    _np.savez(path + ".npz", **arrs)
    json.dump(meta, open(path + ".json", "w"))
    print("DUMPSIG", len(meta), "objects")
bpy.ops.wm.read_factory_settings(use_empty=True)
def import_usdc(path, side):
    """the scene's usdc (gen2 output.usd): Blender's C++ USD importer, then glTF-importer names, uv layer names and plain
    materials from the sidecar, so everything downstream sees the same objects as after the glTF import"""
    bpy.ops.wm.usd_import(filepath=path, import_materials=False, import_cameras=False, import_lights=False, import_curves=False, import_volumes=False,
                          import_shapes=False, import_skeletons=False, import_blendshapes=False, import_points=False, import_subdivision=False,
                          read_mesh_uvs=True, read_mesh_colors=False, read_mesh_attributes=False, create_collection=False, set_frame_range=False,
                          create_world_material=False, merge_parent_xform=False, validate_meshes=False)
    mats = {}
    for m in side.get("materials", []):
        mt = bpy.data.materials.new(m["name"])
        mt.use_backface_culling = False
        nt = mt.node_tree
        p = next(n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED')
        p.inputs["Base Color"].default_value = tuple(m["color"][:3]) + (1.0,)
        p.inputs["Alpha"].default_value = m["color"][3]
        p.inputs["Metallic"].default_value = m["metallic"]
        p.inputs["Roughness"].default_value = m["roughness"]
        if any(c > 0 for c in m["emissive"]):
            p.inputs["Emission Color"].default_value = tuple(m["emissive"]) + (1.0,)
            p.inputs["Emission Strength"].default_value = m["emissive_strength"] if m["emissive_strength"] > 0 else 1.0
        else:
            p.inputs["Emission Color"].default_value = (1.0, 1.0, 1.0, 1.0)
            p.inputs["Emission Strength"].default_value = 0.0
        if m["color"][3] < 1.0:
            mt.surface_render_method = 'BLENDED'
        mats[m["name"]] = mt
    nodes = side["nodes"]
    for o in list(bpy.data.objects):
        if o.type == 'MESH' and o.name[:1] == 'n' and o.name[1:].isdigit():
            n = nodes[int(o.name[1:])]
            w = o.matrix_world.copy(); o.parent = None; o.matrix_world = w
            o.name = n["name"]; o.data.name = n["name"]
            for uv in o.data.uv_layers:
                if uv.name == "st": uv.name = "UVMap"
            o.data.materials.clear(); o.data.materials.append(mats.get(n["material"]))
    for o in list(bpy.data.objects):
        if o.type == 'EMPTY' and not o.children:
            bpy.data.objects.remove(o, do_unlink=True)
_usd = args.glb[:-4] + ".usdc"
if os.path.exists(_usd) and not os.environ.get("NO_USD"):
    import_usdc(_usd, json.load(open(args.glb.replace(".glb", ".json"))))
    phase("usdc import")
else:
    bpy.ops.import_scene.gltf(filepath=args.glb, merge_vertices=True)
    phase("gltf import")
import math
# python-style object frames: the glb holds world-space meshes under a Y-up root; Infinigen shaders use Object texture
# coordinates, so put each mesh back into its asset's local Z-up frame (sidecar "pose", identity for rooms) like python does
_poses = {n["name"]: n.get("pose") for n in json.load(open(args.glb.replace(".glb", ".json")))["nodes"]}
for _o in list(bpy.data.objects):
    if _o.type != 'MESH': continue
    # (instanced nodes -- glTF meshes shared by many nodes, the outdoor trees -- keep their object transform: baking it into the
    # shared mesh would apply every instance's matrix to all of them)
    if _o.data.users > 1:
        _W = _o.matrix_world.copy(); _o.parent = None; _o.matrix_world = _W
        continue
    _W = _o.matrix_world.copy()
    _pn = _o.name if _o.name in _poses else next((k for k in _poses if _o.name.startswith(k)), None)
    _P = mathutils.Matrix(_poses[_pn]) if _pn and _poses.get(_pn) else mathutils.Matrix.Identity(4)
    _o.parent = None
    _o.data.transform(_P.inverted() @ _W)
    _o.matrix_world = _P
phase("object frames")
# smooth by angle: one operator call over every mesh (it handles each selected object on its own; per-object calls with a
# select-all in between cost ~45 s on a house). OLD_SMOOTH=1: the per-object loop.
if os.environ.get("OLD_SMOOTH"):
    for _o in bpy.data.objects:
        if _o.type == 'MESH':
            bpy.ops.object.select_all(action='DESELECT'); _o.select_set(True); bpy.context.view_layer.objects.active = _o
            bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
else:
    # (not the outdoor meshes: they carry the generator's normals -- terrain smooth, leaf cards bent away from their crown centre;
    # re-smoothing replaced the bent normals and dense crowns / shrubs shaded as black blobs)
    _meshes = [o for o in bpy.data.objects if o.type == 'MESH' and not o.name.startswith("outdoor_")]
    bpy.ops.object.select_all(action='DESELECT')
    for _o in _meshes:
        _o.select_set(True)
    if _meshes:
        bpy.context.view_layer.objects.active = _meshes[0]
        bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
    bpy.ops.object.select_all(action='DESELECT')
if os.environ.get("SMOOTH_HASH"):
    import hashlib
    _h = hashlib.sha1()
    for _o in sorted((o for o in bpy.data.objects if o.type == 'MESH'), key=lambda o: o.name):
        for _an in ("sharp_edge", "sharp_face"):
            _a = _o.data.attributes.get(_an)
            _h.update(_an.encode() + (bytes(bytearray(int(v.value) for v in _a.data)) if _a else b"-"))
        _h.update(bytes(bytearray(int(p.use_smooth) for p in _o.data.polygons)))
    print("SMOOTH_HASH", _h.hexdigest())
phase("smooth by angle")
if os.environ.get("DUMPSIG_EARLY"):
    _dumpsig(os.environ["DUMPSIG_EARLY"])

sidecar = json.load(open(args.glb.replace(".glb", ".json")))
if sidecar.get("cloth") and not args.no_cloth:
    # Rust exported the exact pre-sim cloth + python's cloth_sim settings: redo the sims with Blender's solver (tools/cloth_jobs.py)
    import numpy as _np
    for _t in (os.environ.get("INDOORGEN_TOOLS"), os.path.dirname(os.path.dirname(os.path.realpath(__file__)))):
        if _t and os.path.exists(os.path.join(_t, "cloth_jobs.py")):
            sys.path.insert(0, _t); break
    import cloth_jobs
    def _world_mesh(name):
        o = bpy.data.objects[name]
        dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg); me = e.to_mesh()
        M = _np.array(o.matrix_world)
        v = _np.array([tuple(x.co) for x in me.vertices]).reshape(-1, 3)
        v = v @ M[:3, :3].T + M[:3, 3]
        f = [list(p.vertices) for p in me.polygons]
        e.to_mesh_clear()
        return v, f
    def _set_mesh(name, v, f):
        o = bpy.data.objects[name]
        Mi = _np.array(o.matrix_world.inverted())
        lv = _np.asarray(v) @ Mi[:3, :3].T + Mi[:3, 3]
        o.data.clear_geometry()
        o.data.from_pydata([tuple(x) for x in lv], [], [list(map(int, p)) for p in f])
        o.data.update()
        bpy.ops.object.select_all(action='DESELECT'); o.select_set(True); bpy.context.view_layer.objects.active = o
        bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
    try:
        cloth_jobs.run_jobs(sidecar["cloth"], _world_mesh, _set_mesh)
    except Exception as _e:
        import traceback; traceback.print_exc(); print("CLOTH JOBS FAILED, keeping the Rust cloth:", repr(_e))
specs = {n["name"]: n for n in sidecar["nodes"]}
from infinigen.core.util.math import FixedSeed, int_hash
import importlib
from infinigen.assets.materials import ceramic, wood, fabric, metal, plastic, terrain, art, lamp_shaders, text
phase("infinigen import")
if not os.environ.get("RAW_PLANTS"):
    # potted plants: leafy houseplant kinds only (matches crates/ig_plants monocot kind_for_seed) in green hues (infinigen's monocot hues
    # reach straw yellow 0.15; houseplants are green 0.22-0.33)
    try:
        from infinigen.assets.objects.monocot import generate as _mg, growth as _mgr
        _mi0 = _mg.MonocotFactory.__init__
        def _mi(self, factory_seed, coarse=False, factory_method=None, grass=None): _mi0(self, factory_seed, coarse, factory_method, False if grass is None else grass)
        _mg.MonocotFactory.__init__ = _mi
        import numpy as _npp
        for _c in [_mgr.MonocotGrowthFactory] + _mgr.MonocotGrowthFactory.__subclasses__():
            _c.build_base_hue = staticmethod(lambda: _npp.random.uniform(0.22, 0.33))
    except Exception as _e:
        print("plant patch skipped", repr(_e))
if not os.environ.get("RAW_METALS"):
    # infinigen's metal_hsv gives 80% of metals a random saturated hue (teal fridges, purple microwaves); real interiors are mostly
    # natural metals: 85% natural, the rest infinigen's draw (RAW_METALS=1 keeps infinigen's mix)
    from infinigen.assets import colors as _colors
    _metal_hsv0 = _colors.metal_hsv
    import numpy as _np
    _colors.metal_hsv = lambda: _colors.metal_natural_hsv() if _np.random.uniform() < 0.85 else _metal_hsv0()
    # fabric_hsv (sofas, knits, plaid) draws any hue at saturation 0.3-0.8, value 0.6-0.9: pastel pink / lime / purple upholstery
    # everywhere. Real homes: mostly neutrals (greys, beiges, oatmeal, charcoal), some muted colours (navy, olive, rust, teal), rarely
    # bright: 60 % neutral, 25 % muted, 15 % infinigen's draw. textile_hsv (velvet, darker) likewise.
    _fabric_hsv0, _textile_hsv0 = _colors.fabric_hsv, _colors.textile_hsv
    def _home_fabric(v_lo, v_hi, orig):
        u = _np.random.uniform()
        if u < 0.6: return (_np.random.uniform(0.05, 0.13) if _np.random.uniform() < 0.7 else _np.random.uniform(0, 1), _np.random.uniform(0.02, 0.22), _np.random.uniform(v_lo, v_hi))
        if u < 0.85: return (_np.random.choice([0.0, 0.06, 0.1, 0.25, 0.45, 0.58, 0.62, 0.95]) + _np.random.uniform(-0.02, 0.02), _np.random.uniform(0.25, 0.5), _np.random.uniform(v_lo * 0.8, v_hi * 0.85))
        return orig()
    _colors.fabric_hsv = lambda: _home_fabric(0.3, 0.85, _fabric_hsv0)
    _colors.textile_hsv = lambda: _home_fabric(0.1, 0.45, _textile_hsv0)
    # the knits draw "_color" as three uniform RGB channels (random saturated bedding): same palette, as RGB
    import importlib as _il
    for _mn in ("fine_knit_fabric", "coarse_knit_fabric"):
        try:
            _m = _il.import_module("infinigen.assets.materials.fabric." + _mn)
            def _wrap(f0):
                def f():
                    d = f0(); d["_color"] = list(_colors.hsv2rgba(_home_fabric(0.25, 0.85, _fabric_hsv0))[:3]); return d
                return f
            _m.get_texture_params = _wrap(_m.get_texture_params)
        except Exception as _e:
            print("fabric palette patch skipped for", _mn, repr(_e))
import bl5compat; bl5compat.install()  # Blender 5: geometry-nodes modifier sockets via mod.properties (no-op on 4.x)
# CYCLES_DEVICE=GPU: Metal (Apple) when available; CPU otherwise
import os as _osd
_DEVICE = "CPU"
if _osd.environ.get("CYCLES_DEVICE", "CPU").upper() == "GPU":
    try:
        _cp = bpy.context.preferences.addons["cycles"].preferences
        for _t in ("METAL", "OPTIX", "CUDA", "HIP", "ONEAPI"):
            try:
                _cp.compute_device_type = _t; _cp.get_devices()
                if any(d.type == _t for d in _cp.devices):
                    for d in _cp.devices: d.use = d.type == _t
                    _DEVICE = "GPU"; print("CYCLES GPU", _t); break
            except Exception: pass
    except Exception as e: print("CYCLES GPU FAIL", repr(e))
mods = {"ceramic": ceramic, "wood": wood, "fabric": fabric, "metal": metal, "plastic": plastic, "terrain": terrain, "art": art, "lamp_shaders": lamp_shaders, "text": text}
for _n in ("plant", "creature", "dev"):  # extra material modules for specs ("plant.SpiderPlant", ...)
    try:
        mods[_n] = importlib.import_module("infinigen.assets.materials." + _n)
    except Exception as _e:
        print("MATERIAL MODULE FAIL", _n, repr(_e))
def _as_material(inst, **kw):
    """callable material class -> inst(**kw); apply-style class (terrain.Mud, plant.SpiderPlant) -> apply to a temp mesh and take its material; bpy Material -> itself"""
    if isinstance(inst, bpy.types.Material): return inst
    if callable(inst): return inst(**kw)
    me = bpy.data.meshes.new("_tmpmat"); me.from_pydata([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [], [(0, 1, 2)])
    ob = bpy.data.objects.new("_tmpmat", me); bpy.context.collection.objects.link(ob)
    inst.apply(ob, **kw)
    m = ob.data.materials[0] if len(ob.data.materials) else None
    if m is not None: m.use_fake_user = True
    bpy.data.objects.remove(ob)
    return m
cache = {}
clay = None
if args.clay:
    clay = bpy.data.materials.new("clay"); clay.use_nodes = True
    clay.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.6, 0.6, 0.6, 1)
    clay.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.9

# photographic prints for wall art (infinigen's art.Art noise patterns read as synthetic next to Hypersim's framed photos): horizon
# crops of the HDRI skies (tools: ~/work/assets/art_photos, ART_PHOTOS overrides), a different print per picture, the crop aspect
# nearest the canvas; 15 % keep the procedural art
_ART_DIR = os.environ.get("ART_PHOTOS", os.path.expanduser("~/work/assets/art_photos"))
_ART_FILES = sorted(f for f in os.listdir(_ART_DIR) if f.endswith(".jpg")) if os.path.isdir(_ART_DIR) else []
_ART_IMG = {}
def _photo_art(o):
    import zlib
    if not _ART_FILES: return None
    h = zlib.crc32(("%s:%s" % (args.seed, o.name.split("#")[0])).encode())
    if h % 100 < 15: return None
    bb = [mathutils.Vector(c) for c in o.bound_box]; ext = [max(v[i] for v in bb) - min(v[i] for v in bb) for i in range(3)]
    hor = 0 if ext[0] >= ext[1] else 1
    asp = ext[hor] / max(ext[2], 1e-3)
    best = min((0.75, 1.0, 1.33, 1.5), key=lambda a: abs(math.log(a / max(asp, 1e-3))))
    cands = [f for f in _ART_FILES if "_%s_" % best in f]
    f = cands[(h // 100) % len(cands)]
    if f not in _ART_IMG: _ART_IMG[f] = bpy.data.images.load(os.path.join(_ART_DIR, f))
    m = bpy.data.materials.new("photo_art_%s" % o.name.split("#")[0]); m.use_nodes = True; nt = m.node_tree
    bs = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"); bs.inputs["Roughness"].default_value = 0.45
    tc = nt.nodes.new("ShaderNodeTexCoord"); sep = nt.nodes.new("ShaderNodeSeparateXYZ"); com = nt.nodes.new("ShaderNodeCombineXYZ")
    tx = nt.nodes.new("ShaderNodeTexImage"); tx.image = _ART_IMG[f]; tx.extension = "EXTEND"
    nt.links.new(tc.outputs["Generated"], sep.inputs[0]); nt.links.new(sep.outputs[hor], com.inputs[0]); nt.links.new(sep.outputs[2], com.inputs[1])
    nt.links.new(com.outputs[0], tx.inputs[0]); nt.links.new(tx.outputs["Color"], bs.inputs["Base Color"])
    return m

for o in list(bpy.data.objects):
    if o.type != "MESH": continue
    if clay is not None:
        o.data.materials.clear(); o.data.materials.append(clay); continue
    key = o.name.split(".")[0] if o.name not in specs else o.name
    nm = o.name if o.name in specs else next((k for k in specs if o.name.startswith(k)), None)
    if nm is None: continue
    spec = specs[nm].get("spec")
    if not spec: continue
    if not os.environ.get("RAW_CERAMIC") and spec.get("cls") == "ceramic.Ceramic" and str(specs[nm].get("material", "")).split(":")[0] in ("toilet_ceramic", "tub_ceramic", "sink_ceramic"):
        # sanitary ware is white: infinigen's shader_ceramic draws a random hue at value 0.3-0.6 80 % of the time (mauve / mint toilets)
        spec = {"cls": "shader.ceramic.ceramic.shader_ceramic", "kwargs": {"clear": True, "roughness_max": 0.3}, "seed": spec.get("seed", 0), "whiten": True}
    if not os.environ.get("RAW_BEDFRAME") and spec.get("cls") in ("metal.HammeredMetal", "metal.GalvanizedMetal") and str(specs[nm].get("material", "")).split(":")[0] == "bed_frame":
        # hammered / galvanized bumps on a 1.5 m footboard read as crumpled paper; bed frames are powder-coated or brushed
        spec = dict(spec, cls="metal.BrushedBlackMetal")
    if spec.get("cls") == "ceramic.Marble" and not os.environ.get("RAW_MARBLE"):
        # ceramic.Marble is a grey camouflage blotch (reviewers: "camo / agate counters"); MarbleRegular is veined stone
        spec = dict(spec, cls="ceramic.MarbleRegular")
    if spec.get("cls") == "wood.Wood" and not os.environ.get("RAW_WOOD"):
        # large flat tops (table / desk / counter boards): shader_wood's noise-warped grain reads as burl / contour lines there;
        # straight-grain veneer (plywood) instead, 70 % blonde, 30 % dark
        _bb0 = [mathutils.Vector(c) for c in o.bound_box]
        _e0 = sorted([max(v[i] for v in _bb0) - min(v[i] for v in _bb0) for i in range(3)])
        _ez = max(v[2] for v in _bb0) - min(v[2] for v in _bb0)
        if _e0[1] > 0.35 and _ez <= _e0[0] + 1e-6 and _ez < 0.12 * _e0[1]:
            import zlib as _z
            spec = dict(spec, cls="wood.BlondePlywood" if _z.crc32(o.name.split("#")[0].encode()) % 10 < 7 else "wood.BlackPlywood")

        # wood.Wood stretches its grain along object x; on parts thin in x (headboards, cabinet doors, panels facing x) the visible
        # faces showed end grain (marble-like swirls): those get the shader's vertical grain instead
        _bb = [mathutils.Vector(c) for c in o.bound_box]
        _ext = [max(v[i] for v in _bb) - min(v[i] for v in _bb) for i in range(3)]
        if _ext[0] < 0.5 * max(_ext[1], _ext[2]):
            spec = {"cls": "shader.wood.wood.shader_wood", "kwargs": {"vertical": True}, "seed": spec.get("seed", 0)}
    if spec.get("cls") == "art.ArtRug" and not os.environ.get("RAW_ART"):
        # printed-noise rugs (saturated blobs) read as synthetic: most become woven rugs (fabric.Rug: tufted pile, muted hue)
        import zlib
        if zlib.crc32(("%s:rug:%s" % (args.seed, o.name.split("#")[0])).encode()) % 100 < 90: spec = dict(spec, cls="fabric.Rug")
    if spec.get("cls") in ("art.Art", "art.DarkArt") and not os.environ.get("RAW_ART"):
        _pm = _photo_art(o)
        if _pm is not None:
            o.data.materials.clear(); o.data.materials.append(_pm); continue
    ck = json.dumps(spec, sort_keys=True)
    if ck not in cache:
        mod, cls = spec["cls"].split(".", 1)
        with FixedSeed(int(spec.get("seed", 0)) % 2**32):
            try:
                if spec.get("skip"):  # replay the generator position of the python draw stream (cabinet drawer materials): burn `skip` 32-bit words
                    import numpy as _np; _np.random.randint(0, 2**32, size=int(spec["skip"]), dtype=_np.uint32)
                if mod == "people":  # Rocketbox avatars (ig_people): people.skin / people.cloth / people.hair, kwargs = texture paths
                    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import people_mats
                    cache[ck] = people_mats.make(spec)
                elif mod == "shader":  # generic: "shader.<module under infinigen.assets.materials>.<func>"
                    from infinigen.core import surface as _surf
                    _mp, _fn = cls.rsplit(".", 1)
                    cache[ck] = _surf.shaderfunc_to_material(getattr(importlib.import_module("infinigen.assets.materials." + _mp), _fn), **spec.get("kwargs", {}))
                elif mod == "lamp_shaders" and cls.startswith("shader_"):
                    from infinigen.core import surface as _surf
                    cache[ck] = _surf.shaderfunc_to_material(getattr(lamp_shaders, cls))
                elif mod == "window" and cls == "glass":
                    from infinigen.assets.objects.windows.window import shader_window_glass
                    from infinigen.core import surface as _surf
                    cache[ck] = _surf.shaderfunc_to_material(shader_window_glass)
                elif mod == "factory":  # "factory.<module under infinigen.assets.objects>.<Class>", kwargs.attr = dotted attribute path of the material (or apply-style material)
                    _mp, _cn = cls.rsplit(".", 1)
                    _o = getattr(importlib.import_module("infinigen.assets.objects." + _mp), _cn)(int(spec.get("seed", 0)))
                    for _a in spec["kwargs"]["attr"].split("."): _o = getattr(_o, _a)
                    cache[ck] = _as_material(_o, **{k: v for k, v in spec["kwargs"].items() if k != "attr"})
                else:
                    inst = getattr(mods[mod], cls)()
                    kw = dict(spec.get("kwargs", {}))
                    if mod == "ceramic" and cls == "Tile" and "shader" in kw:  # tile glaze by name (ig_features): "ceramic" | "marble" | "stone"
                        from infinigen.assets.materials.ceramic import ceramic as _cer, tile as _tile
                        kw["shader_func"] = {"ceramic": _cer.shader_ceramic, "marble": _tile.shader_marble}.get(kw.pop("shader"))
                        if kw["shader_func"] is None: kw.pop("shader_func")
                    cache[ck] = _as_material(inst, **kw)
                if spec.get("whiten") and cache[ck] is not None:  # clear=True gives grey value 0.3-0.6: lift to porcelain white
                    _m = cache[ck].copy(); _nt = _m.node_tree
                    for _bs in [n for n in _nt.nodes if n.type == 'BSDF_PRINCIPLED']:
                        _inp = _bs.inputs['Base Color']
                        for _l in list(_inp.links): _nt.links.remove(_l)
                        _inp.default_value = (0.9, 0.9, 0.88, 1.0)
                    cache[ck] = _m
                if spec.get("paint") and cache[ck] is not None:  # painted / whitewashed surface: mix the base colour toward paint.rgb by paint.amount
                    _m = cache[ck].copy(); _nt = _m.node_tree
                    for _bs in [n for n in _nt.nodes if n.type == 'BSDF_PRINCIPLED']:
                        _inp = _bs.inputs['Base Color']; _mix = _nt.nodes.new('ShaderNodeMix'); _mix.data_type = 'RGBA'
                        _mix.inputs['Factor'].default_value = float(spec["paint"]["amount"])
                        _mix.inputs[7].default_value = tuple(spec["paint"]["rgb"]) + (1.0,)
                        if _inp.is_linked: _nt.links.new(_inp.links[0].from_socket, _mix.inputs[6])
                        else: _mix.inputs[6].default_value = _inp.default_value
                        _nt.links.new(_mix.outputs[2], _inp)
                    cache[ck] = _m
            except Exception as e:
                print("MATERIAL FAIL", spec, repr(e)); cache[ck] = None
    if cache[ck] is not None:
        _mm = cache[ck]
        if not os.environ.get("RAW_GLASS") and str(specs[nm].get("material", "")).split(":")[0] == "shower_glass":
            # perfectly clear 8 mm panels vanished (reviewers: "shower is frame rods only, no glass"): real shower glass shows a
            # faint green-grey tint, edge darkening and soft reflections
            _mm = _mm.copy()
            for _b in [n for n in _mm.node_tree.nodes if n.type in ("BSDF_PRINCIPLED", "BSDF_GLASS")]:
                _ci = _b.inputs["Base Color"] if "Base Color" in _b.inputs else _b.inputs["Color"]
                if not _ci.is_linked: _ci.default_value = (0.80, 0.90, 0.88, 1.0)
                if not _b.inputs["Roughness"].is_linked: _b.inputs["Roughness"].default_value = max(_b.inputs["Roughness"].default_value, 0.04)
            cache[ck + ":shower"] = _mm
        o.data.materials.clear(); o.data.materials.append(_mm)
phase("materials")
# room shells: python's own room_walls/room_floors/room_ceilings sampling (roommats.py) replaces the Rust-side spec draws
if not args.clay and not os.environ.get("NO_ROOMMATS"):
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    try:
        import roommats
        print("ROOMMATS", roommats.apply_room_materials(args.seed, [n['name'] for n in sidecar['nodes']]))
    except Exception as _e:
        import traceback; traceback.print_exc(); print("ROOMMATS FAILED, keeping the Rust spec materials:", repr(_e))
def _white_ceilings(amount=0.85):
    """ceilings: infinigen's plaster draws any hue (pink / green ceilings); mix the base colour toward an off-white (texture kept)"""
    done = set(); n = 0
    for o in bpy.data.objects:
        if o.type != "MESH" or not o.name.endswith(".ceiling"): continue
        for sl in o.material_slots:
            m = sl.material
            if m is None or not m.use_nodes or m.name in done: continue
            done.add(m.name); nt = m.node_tree
            for b_ in [x for x in nt.nodes if x.type == "BSDF_PRINCIPLED"]:
                inp = b_.inputs["Base Color"]; mix = nt.nodes.new("ShaderNodeMix"); mix.data_type = "RGBA"
                mix.inputs["Factor"].default_value = amount; mix.inputs[7].default_value = (0.88, 0.87, 0.84, 1.0)
                if inp.is_linked: nt.links.new(inp.links[0].from_socket, mix.inputs[6])
                else: mix.inputs[6].default_value = inp.default_value
                nt.links.new(mix.outputs[2], inp); n += 1
    return n
if not args.clay and not os.environ.get("RAW_CEILING"):
    print("CEILINGS", _white_ceilings(), "ceiling shaders whitened")
DS = None
if args.dataset:
    DS = json.load(open(args.dataset))
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "dataset")); import dscap
    print("DSWALLS", dscap.apply_walls(DS.get("variation")))
if os.environ.get("DUMPMATS"):
    import roommats; json.dump(roommats.dump_room_materials(), open(os.environ["DUMPMATS"], "w")); print("DUMPMATS done"); sys.exit(0)
def _wood_depth(m):
    """infinigen shader_wood: grain relief scaled by log_uniform(0.0002, 0.01) (usually invisible) under a clear coat of up to 100 % (plastic
    look). Deeper relief (>= 0.0015) with fine pores stretched along the grain, none on end-grain faces; the varnish is kept."""
    nt = m.node_tree
    disp = next((n for n in nt.nodes if n.type == "DISPLACEMENT"), None)
    grain_map = next((n for n in nt.nodes if n.type == "MAPPING" and not n.inputs["Scale"].is_linked and tuple(round(x) for x in n.inputs["Scale"].default_value) == (5, 100, 100)), None)
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if disp is None or grain_map is None or bsdf is None or not disp.inputs["Height"].is_linked: return False
    mul = disp.inputs["Height"].links[0].from_node
    if mul.type != "MATH" or not mul.inputs[0].is_linked: return False
    grain = mul.inputs[0].links[0].from_socket  # 0..1, dark grain lines low
    # relief from the smoother grain layer: mix_2 = high-detail musgrave (fibre noise, crusty as relief on thin edges) x mix_1 (grain bands)
    g2 = mul.inputs[0].links[0].from_node
    soft = g2.inputs[7].links[0].from_socket if g2.type == "MIX" and g2.inputs[7].is_linked else grain
    relief = nt.nodes.new("ShaderNodeMath"); relief.operation = "MULTIPLY"
    # thick varnish fills grain and pores: relief scales down with the coat (glossy table tops stay smooth, matte panels get full depth)
    coat = bsdf.inputs["Coat Weight"].default_value if "Coat Weight" in bsdf.inputs and not bsdf.inputs["Coat Weight"].is_linked else 0.0
    fill = max(0.0, 1.0 - 0.9 * coat)
    nt.links.new(soft, relief.inputs[0]); relief.inputs[1].default_value = float(os.environ.get("WOOD_RELIEF", 0.0015)) * fill
    # pores: high-frequency noise in the stretched grain space
    pn = nt.nodes.new("ShaderNodeTexNoise"); pn.inputs["Scale"].default_value = 6.0; pn.inputs["Detail"].default_value = 3.0
    nt.links.new(grain_map.outputs[0], pn.inputs["Vector"])
    pr = nt.nodes.new("ShaderNodeMapRange"); pr.inputs[1].default_value = 0.55; pr.inputs[2].default_value = 0.75; pr.inputs[3].default_value = 0.0; pr.inputs[4].default_value = -float(os.environ.get("WOOD_PORES", 0.0005)) * fill
    nt.links.new(pn.outputs["Fac"], pr.inputs[0])
    add = nt.nodes.new("ShaderNodeMath"); add.operation = "ADD"
    nt.links.new(relief.outputs[0], add.inputs[0]); nt.links.new(pr.outputs[0], add.inputs[1])
    # end-grain faces (normal along the grain axis = x of the grain space) keep their swirl colour but get no relief (crusty edges otherwise)
    tc = nt.nodes.new("ShaderNodeTexCoord")
    nrm = tc.outputs["Normal"]
    src = grain_map.inputs["Vector"].links[0].from_node if grain_map.inputs["Vector"].is_linked else None
    if src is not None and src.type == "MAPPING":  # vertical grain: same rotation for the normal
        rm = nt.nodes.new("ShaderNodeMapping"); rm.vector_type = "NORMAL"
        rm.inputs["Rotation"].default_value = src.inputs["Rotation"].default_value
        nt.links.new(nrm, rm.inputs["Vector"]); nrm = rm.outputs[0]
    sep = nt.nodes.new("ShaderNodeSeparateXYZ"); nt.links.new(nrm, sep.inputs[0])
    sq = nt.nodes.new("ShaderNodeMath"); sq.operation = "MULTIPLY"; nt.links.new(sep.outputs[0], sq.inputs[0]); nt.links.new(sep.outputs[0], sq.inputs[1])
    side = nt.nodes.new("ShaderNodeMath"); side.operation = "SUBTRACT"; side.use_clamp = True; side.inputs[0].default_value = 1.0; nt.links.new(sq.outputs[0], side.inputs[1])
    att = nt.nodes.new("ShaderNodeMath"); att.operation = "MULTIPLY"; nt.links.new(add.outputs[0], att.inputs[0]); nt.links.new(side.outputs[0], att.inputs[1])
    # relief on the wood (base layer) only, as a bump into the BSDF normal: the varnish (coat) stays smooth on the true normal (bumping the
    # whole shading normal broke a glossy coat's highlight into sparkles); the material displacement output is disconnected
    for l in list(disp.outputs[0].links): nt.links.remove(l)
    bump = nt.nodes.new("ShaderNodeBump"); bump.inputs["Strength"].default_value = 1.0; bump.inputs["Distance"].default_value = 1.0
    nt.links.new(att.outputs[0], bump.inputs["Height"])
    if not bsdf.inputs["Normal"].is_linked: nt.links.new(bump.outputs[0], bsdf.inputs["Normal"])
    geo = nt.nodes.new("ShaderNodeNewGeometry")
    if "Coat Normal" in bsdf.inputs and not bsdf.inputs["Coat Normal"].is_linked: nt.links.new(geo.outputs["Normal"], bsdf.inputs["Coat Normal"])
    disp.inputs["Scale"].default_value = 1.0
    # base roughness floor 0.28: shader_wood draws roughness down to 0 (mirror-like), and relief on a mirror reads as sparkle; the varnish
    # (coat) keeps the gloss
    ri = bsdf.inputs["Roughness"]
    rmax = nt.nodes.new("ShaderNodeMath"); rmax.operation = "MAXIMUM"; rmax.inputs[1].default_value = 0.28
    if ri.is_linked: nt.links.new(ri.links[0].from_socket, rmax.inputs[0])
    else: rmax.inputs[0].default_value = ri.default_value
    nt.links.new(rmax.outputs[0], ri)
    try:
        m.displacement_method = "BUMP"  # relief as shading only: true displacement crumpled thin table tops / edges
    except Exception:
        m.cycles.displacement_method = "BUMP"
    return True
def _knit_uv():
    """fine / coarse knit fabrics draw their weave from Object coordinates projected along one axis: moire rings on curved cushions and no
    weave at all on faces parallel to the projection. Switch them to the mesh UVs when every object using the material has a UV map."""
    users = {}
    for o in bpy.data.objects:
        if o.type != "MESH": continue
        for sl in o.material_slots:
            if sl.material: users.setdefault(sl.material.name, []).append(o)
    n = 0
    for m in bpy.data.materials:
        if not m.use_nodes: continue
        nt = m.node_tree
        rr = [nd for nd in nt.nodes if nd.type == "REROUTE" and nd.inputs[0].is_linked and nd.inputs[0].links[0].from_node.type == "TEX_COORD" and nd.inputs[0].links[0].from_socket.name == "Object"]
        if not rr or not any(nd.label == "displacement_scale" for nd in nt.nodes): continue
        us = users.get(m.name, [])
        if not us or not all(len(o.data.uv_layers) > 0 for o in us): continue
        for nd in rr:
            tc = nd.inputs[0].links[0].from_node
            nt.links.new(tc.outputs["UV"], nd.inputs[0])
        n += 1
    return n
def _cloth_wrinkles():
    """bedding (sheet / cover / blanket / throw / pillow / towel materials): the simulated cloth is smooth at render scale; add soft creases
    (stretched noise ridges, ~4-8 cm) and a fine crumple as a bump on the BSDF normal. Material name match only (no other materials)."""
    n = 0
    for m in bpy.data.materials:
        nm = m.name.lower()
        if not m.use_nodes or not any(k in nm for k in ("sheet", "cover", "blanket", "throw", "pillow", "towel", "comforter", "duvet")): continue
        nt = m.node_tree
        b = next((x for x in nt.nodes if x.type == "BSDF_PRINCIPLED"), None)
        if b is None or b.inputs["Normal"].is_linked: continue
        tc = nt.nodes.new("ShaderNodeTexCoord")
        mp = nt.nodes.new("ShaderNodeMapping"); mp.inputs["Scale"].default_value = (6.0, 18.0, 6.0)
        nt.links.new(tc.outputs["Object"], mp.inputs["Vector"])
        ridge = nt.nodes.new("ShaderNodeTexNoise"); ridge.inputs["Scale"].default_value = 1.0; ridge.inputs["Detail"].default_value = 2.0; ridge.inputs["Distortion"].default_value = 0.6
        nt.links.new(mp.outputs[0], ridge.inputs["Vector"])
        fine = nt.nodes.new("ShaderNodeTexNoise"); fine.inputs["Scale"].default_value = 60.0; fine.inputs["Detail"].default_value = 4.0
        nt.links.new(tc.outputs["Object"], fine.inputs["Vector"])
        mix = nt.nodes.new("ShaderNodeMath"); mix.operation = "MULTIPLY_ADD"; mix.inputs[1].default_value = 0.25
        nt.links.new(fine.outputs["Fac"], mix.inputs[0]); nt.links.new(ridge.outputs["Fac"], mix.inputs[2])
        bump = nt.nodes.new("ShaderNodeBump"); bump.inputs["Strength"].default_value = 0.35; bump.inputs["Distance"].default_value = 0.01
        nt.links.new(mix.outputs[0], bump.inputs["Height"]); nt.links.new(bump.outputs[0], b.inputs["Normal"])
        n += 1
    return n
if not os.environ.get("RAW_CLOTH_SHADE"):
    print("CLOTHWRINKLE", _cloth_wrinkles(), "bedding materials")
if not os.environ.get("RAW_KNIT"):
    print("KNITUV", _knit_uv(), "knit materials on UVs")
def _brick_depth(m):
    """infinigen shader_brick: height = colour x 0.01-0.04 (+ faint noise) x scale 0.05, i.e. 0.5-2 mm and inverted wherever the mortar is
    drawn lighter than the bricks. Relief from the brick pattern instead: mortar joints recessed 6-10 mm, a little roughness on each face
    (bump shading: walls are flat, unsubdivided meshes)"""
    if "brick" not in m.name.lower(): return False  # hardwood floors use the brick texture for their planks
    nt = m.node_tree
    bt = next((n for n in nt.nodes if n.type == "TEX_BRICK"), None)
    disp = next((n for n in nt.nodes if n.type == "DISPLACEMENT"), None)
    if bt is None or disp is None: return False
    import random as _r
    rr = _r.Random(m.name)
    face = nt.nodes.new("ShaderNodeMath"); face.operation = "SUBTRACT"; face.inputs[0].default_value = 1.0
    nt.links.new(bt.outputs["Fac"], face.inputs[1])  # Fac = 1 in the mortar
    depth = nt.nodes.new("ShaderNodeMath"); depth.operation = "MULTIPLY"; depth.inputs[1].default_value = rr.uniform(0.006, 0.010)
    nt.links.new(face.outputs[0], depth.inputs[0])
    nz = nt.nodes.new("ShaderNodeTexNoise"); nz.inputs["Scale"].default_value = rr.uniform(25, 40); nz.inputs["Detail"].default_value = 6
    if bt.inputs["Vector"].is_linked: nt.links.new(bt.inputs["Vector"].links[0].from_socket, nz.inputs["Vector"])
    rough = nt.nodes.new("ShaderNodeMath"); rough.operation = "MULTIPLY_ADD"; rough.inputs[1].default_value = 0.0015
    nt.links.new(nz.outputs["Fac"], rough.inputs[0]); nt.links.new(depth.outputs[0], rough.inputs[2])
    for l in list(disp.inputs["Height"].links): nt.links.remove(l)
    nt.links.new(rough.outputs[0], disp.inputs["Height"])
    disp.inputs["Midlevel"].default_value = 0.0; disp.inputs["Scale"].default_value = 1.0
    try:
        m.displacement_method = "BUMP"
    except Exception:
        m.cycles.displacement_method = "BUMP"
    return True
# micro-surface: most procedural materials have perfectly smooth normals and one roughness value, so every highlight is clean and
# even -- the "plastic" look. Materials without surface relief get a fine procedural bump and a +-roughness variation, scaled by family
# (name of the shader function): fabric weave, plaster orange-peel, wood pores, fine relief on paint / plastic / ceramic. RAW_MICRO opts out.
_MICRO = [  # (name substrings, bump noise scale (1/m), bump strength, bump distance (m), roughness variation, albedo variation)
    # room-scale views are 0.5-1 cm per pixel: weave-scale relief (mm) averages away; what reads as "not plastic" is cm-scale unevenness,
    # broken-up highlights and slight colour mottling
    (("fabric", "knit", "velvet", "rug", "towel", "cloth", "bedding", "linen", "comforter", "pillow", "curtain", "sofa_fabric", "chair_fabric", "sheet", "mattress", "duvet", "throw", "blanket", "cushion", "bed_panel", "upholster", "headboard"), 70.0, 0.22, 0.003, 0.15, 0.10),
    (("leather",), 35.0, 0.18, 0.002, 0.20, 0.08),
    (("plaster", "wall", "paint", "concrete", "stucco"), 22.0, 0.12, 0.002, 0.12, 0.05),
    (("wood", "plywood", "parquet", "hardwood"), 120.0, 0.10, 0.001, 0.22, 0.06),
    (("tile", "ceramic", "marble", "stone", "granite", "brick"), 40.0, 0.05, 0.001, 0.15, 0.04),
    (("plastic", "rubber"), 60.0, 0.05, 0.0008, 0.15, 0.04),
    (("metal", "steel", "aluminium", "brass", "chrome", "iron", "_leg", "_frame"), 90.0, 0.03, 0.0005, 0.25, 0.03),
]
_NOMICRO = ("glass", "mirror", "lamp", "bulb", "emiss", "people", "skin", "hair", "eye", "sky", "water", "window", "photo_art", "screen", "dscam")
def _micro_surface(m):
    nm = m.name.lower()
    if any(x in nm for x in _NOMICRO): return False
    fam = next((f for f in _MICRO if any(x in nm for x in f[0])), (("",), 50.0, 0.08, 0.001, 0.12, 0.05))
    if os.environ.get("MICRODBG"): print("MICRODBG", m.name, fam[0][0], [b.inputs["Normal"].is_linked for b in m.node_tree.nodes if b.type == "BSDF_PRINCIPLED"])
    _, scale, strength, dist, rvar, avar = fam
    nt = m.node_tree; done = False
    for b in [n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"]:
        # matte families: cloth is very rough with a soft sheen and no clear coat (glossy pillows / satin sheets read as plastic);
        # paint and plaster are matte too
        if fam[0][0] in ("fabric", "plaster"):
            _rmin = 0.85 if fam[0][0] == "fabric" else 0.75
            _ri = b.inputs["Roughness"]
            if _ri.is_linked:
                _mx = nt.nodes.new("ShaderNodeMath"); _mx.operation = "MAXIMUM"; _mx.inputs[1].default_value = _rmin
                nt.links.new(_ri.links[0].from_socket, _mx.inputs[0]); nt.links.new(_mx.outputs[0], _ri)
            else: _ri.default_value = max(_ri.default_value, _rmin)
            for _nm, _v in (("Specular IOR Level", 0.25 if fam[0][0] == "fabric" else 0.35), ("Coat Weight", 0.0)):
                if _nm in b.inputs and not b.inputs[_nm].is_linked: b.inputs[_nm].default_value = min(b.inputs[_nm].default_value, _v)
            if fam[0][0] == "fabric" and "Sheen Weight" in b.inputs and not b.inputs["Sheen Weight"].is_linked:
                b.inputs["Sheen Weight"].default_value = max(b.inputs["Sheen Weight"].default_value, 0.5)
                if "Sheen Roughness" in b.inputs: b.inputs["Sheen Roughness"].default_value = 0.5
        if b.inputs["Normal"].is_linked: continue
        if not b.inputs["Transmission Weight"].is_linked and b.inputs["Transmission Weight"].default_value > 0.5: continue
        tc = nt.nodes.new("ShaderNodeTexCoord")
        nz = nt.nodes.new("ShaderNodeTexNoise"); nz.inputs["Scale"].default_value = scale; nz.inputs["Detail"].default_value = 6.0; nz.inputs["Roughness"].default_value = 0.6
        nt.links.new(tc.outputs["Object"], nz.inputs["Vector"])
        bp = nt.nodes.new("ShaderNodeBump"); bp.inputs["Strength"].default_value = strength; bp.inputs["Distance"].default_value = dist
        nt.links.new(nz.outputs["Fac"], bp.inputs["Height"]); nt.links.new(bp.outputs["Normal"], b.inputs["Normal"])
        # roughness: a slower noise varies it +-rvar (relative) so highlights break up instead of reading as one smooth sheen
        ri = b.inputs["Roughness"]
        nz2 = nt.nodes.new("ShaderNodeTexNoise"); nz2.inputs["Scale"].default_value = scale * 0.08; nz2.inputs["Detail"].default_value = 3.0
        nt.links.new(tc.outputs["Object"], nz2.inputs["Vector"])
        mr = nt.nodes.new("ShaderNodeMapRange"); mr.inputs["To Min"].default_value = 1.0 - rvar; mr.inputs["To Max"].default_value = 1.0 + rvar
        nt.links.new(nz2.outputs["Fac"], mr.inputs["Value"])
        mul = nt.nodes.new("ShaderNodeMath"); mul.operation = "MULTIPLY"; mul.use_clamp = True
        if ri.is_linked: nt.links.new(ri.links[0].from_socket, mul.inputs[0])
        else: mul.inputs[0].default_value = ri.default_value
        nt.links.new(mr.outputs["Result"], mul.inputs[1]); nt.links.new(mul.outputs[0], ri)
        # albedo: slow +-avar mottling (one flat colour over a whole sofa or wall reads as CG)
        bc = b.inputs["Base Color"]
        nz3 = nt.nodes.new("ShaderNodeTexNoise"); nz3.inputs["Scale"].default_value = 6.0; nz3.inputs["Detail"].default_value = 4.0
        nt.links.new(tc.outputs["Object"], nz3.inputs["Vector"])
        mr3 = nt.nodes.new("ShaderNodeMapRange"); mr3.inputs["To Min"].default_value = 1.0 - avar; mr3.inputs["To Max"].default_value = 1.0 + 0.5 * avar
        nt.links.new(nz3.outputs["Fac"], mr3.inputs["Value"])
        mix = nt.nodes.new("ShaderNodeVectorMath"); mix.operation = "SCALE"
        if bc.is_linked: nt.links.new(bc.links[0].from_socket, mix.inputs[0])
        else: mix.inputs[0].default_value = tuple(bc.default_value)[:3]
        nt.links.new(mr3.outputs["Result"], mix.inputs["Scale"]); nt.links.new(mix.outputs["Vector"], bc)
        done = True
    return done
if not os.environ.get("RAW_MICRO"):
    print("MICROSURFACE", sum(1 for _m in list(bpy.data.materials) if _m.use_nodes and _micro_surface(_m)), "materials")
if not os.environ.get("RAW_BRICK"):
    print("BRICKDEPTH", sum(1 for _m in bpy.data.materials if _m.use_nodes and _brick_depth(_m)), "brick materials")
if not os.environ.get("RAW_WOOD"):
    _nw = sum(1 for _m in bpy.data.materials if _m.use_nodes and _wood_depth(_m))
    print("WOODDEPTH", _nw, "wood materials")
if not os.environ.get("RAW_FLOORS"):
    # floors: infinigen's tile / hardwood shaders go down to roughness ~0.05, so floors read as mirrors; real floors are satin at most
    _done = {}
    for _o in bpy.data.objects:
        if _o.type != "MESH" or ".floor" not in _o.name: continue
        for _sl in _o.material_slots:
            _m = _sl.material
            if _m is None or not _m.use_nodes: continue
            if _m.name not in _done:
                _nt = _m.node_tree
                for _nd in list(_nt.nodes):
                    if _nd.type != "BSDF_PRINCIPLED": continue
                    _ri = _nd.inputs["Roughness"]
                    if _ri.is_linked:
                        _src = _ri.links[0].from_socket
                        _mx = _nt.nodes.new("ShaderNodeMath"); _mx.operation = "MAXIMUM"; _mx.inputs[1].default_value = 0.42
                        _nt.links.new(_src, _mx.inputs[0]); _nt.links.new(_mx.outputs[0], _ri)
                    else:
                        _ri.default_value = max(_ri.default_value, 0.42)
                    # saturated orange planks / parquet: pull saturation down (reviewers: "orange, repetitive floors")
                    _bc = _nd.inputs["Base Color"]
                    if _bc.is_linked:
                        _hs = _nt.nodes.new("ShaderNodeHueSaturation"); _hs.inputs["Saturation"].default_value = 0.72; _hs.inputs["Value"].default_value = 0.97
                        _nt.links.new(_bc.links[0].from_socket, _hs.inputs["Color"]); _nt.links.new(_hs.outputs["Color"], _bc)
                    if "Coat Weight" in _nd.inputs and not _nd.inputs["Coat Weight"].is_linked: _nd.inputs["Coat Weight"].default_value = min(_nd.inputs["Coat Weight"].default_value, 0.1)
                _done[_m.name] = True
phase("room materials")
sc = bpy.context.scene
# lighting: Infinigen's sky (same seed convention as the reference)
from infinigen.core import init
import infinigen_examples.generate_indoors
init.apply_gin_configs(configs=["base_indoors.gin", "fast_solve.gin", "singleroom.gin"], overrides=["compose_indoors.terrain_enabled=False"],
    config_folders=["infinigen_examples/configs_indoor", "infinigen_examples/configs_nature"])
w = bpy.data.worlds.new("World"); w.use_nodes = True; bpy.context.scene.world = w
try:
    if args.top or args.showcase: raise RuntimeError('skip sky')
    from infinigen.assets import lighting
    if bpy.app.version >= (5, 0, 0):
        # Blender 5 renamed the sky model (NISHITA -> SINGLE_SCATTERING, same model) and dust_density -> aerosol_density;
        # re-exec infinigen's sky_lighting source with those names in this process only (same RNG draws, infinigen repo untouched)
        import inspect as _insp
        _sl = lighting.sky_lighting
        _src = _insp.getsource(_sl).replace('sky_type = "NISHITA"', 'sky_type = "SINGLE_SCATTERING"').replace(".dust_density", ".aerosol_density")
        import gin as _gin
        _gin.enter_interactive_mode()
        exec(compile(_src, _sl.__file__, "exec"), _sl.__dict__)
    with FixedSeed(int_hash((args.seed, "sky_lighting"))):
        lighting.sky_lighting.add_lighting()
except Exception as e:
    print("SKY FAIL", repr(e))
if DS:
    print("DSLIGHT", dscap.apply_lighting(sc, DS.get("variation")), "lamp meshes", dscap.switch_lamp_meshes(DS.get("variation"), sidecar))
    _hd = os.environ.get("HDRI_DIR", os.path.expanduser("~/work/hdri"))
    if not os.environ.get("NO_HDRI") and os.path.isdir(_hd):
        print("DSHDRI", dscap.apply_hdri(sc, DS.get("variation"), _hd, int((DS.get("variation") or {}).get("house", {}).get("seed", args.seed))))
if args.top:
    for o in list(bpy.data.objects):
        n = o.name.lower()
        # (and the roofs, cloud layer and rain: they cover the plan)
        if "ceiling" in n or "light" in n and "ceilinglight" in n or n.endswith(".roof") or n.startswith(("outdoor_sky", "outdoor_rain")): o.hide_render = True
    meshes = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in meshes if "floor" in o.name.lower() for c in o.bound_box] or [o.matrix_world @ mathutils.Vector(c) for o in meshes for c in o.bound_box]
    x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); y0 = min(p.y for p in pts); y1 = max(p.y for p in pts)
    cam = bpy.data.cameras.new("c"); cam.type = "ORTHO"; cam.ortho_scale = max(x1-x0, y1-y0) * 1.1
    co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co)
    co.location = ((x0+x1)/2, (y0+y1)/2, 30); sc.camera = co
    sun = bpy.data.lights.new("s", "SUN"); sun.energy = 4
    so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (0.3, 0.2, 0)
    w = bpy.data.worlds.new("w2"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; sc.world = w
    sc.cycles.film_exposure = args.exposure
    sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = _DEVICE; sc.cycles.use_denoising = True
    sc.render.resolution_x = sc.render.resolution_y = 640; sc.render.filepath = args.out
    bpy.ops.render.render(write_still=True); sys.exit(0)
if args.showcase:
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in meshes for c in o.bound_box]
    x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); zh = max(p.z for p in pts)
    cam = bpy.data.cameras.new("c"); co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co); sc.camera = co
    y0 = min(p.y for p in pts)
    cam.lens = 35; dist = max((x1 - x0) * 1.1, 2.0, zh * 1.6) * 1.15
    # SHOWCASE_DIST / SHOWCASE_Z: close-up framing of single assets
    dist = float(os.environ.get("SHOWCASE_DIST", dist))
    co.location = ((x0 + x1) / 2, y0 - dist, float(os.environ.get("SHOWCASE_Z", max(zh * 0.5, 0.6)))); co.rotation_euler = (1.45, 0, 0)
    sun = bpy.data.lights.new("s", "SUN"); sun.energy = 3
    so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (0.9, 0.2, 0.4)
    w = bpy.data.worlds.new("w3"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; sc.world = w
    sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = _DEVICE; sc.cycles.use_denoising = True
    sc.render.resolution_x, sc.render.resolution_y = 1200, 500; sc.render.filepath = args.out
    bpy.ops.render.render(write_still=True); sys.exit(0)
# outdoor vegetation (sidecar `outdoor.props`, src/outdoor.rs): Infinigen TreeFactory / BushFactory variants, built once by
# tools/render/tree_cache.py under Blender 4.2 (TREE_CACHE dir, IG42_PYTHON interpreter) and instanced at every spot
def add_outdoor(sc, side):
    import os, subprocess
    if not side.get("outdoor") or os.environ.get("NO_OUTDOOR"): return
    # (Rust trees are scene geometry and leave `props` empty: grass and the lawn tint still apply)
    props = (side.get("outdoor") or {}).get("props") or []
    if not props:
        add_grass(side)
        return
    cache = os.environ.get("TREE_CACHE", os.path.expanduser("~/work/bench/tree_cache")); os.makedirs(cache, exist_ok=True)
    py42 = os.environ.get("IG42_PYTHON", os.path.expanduser("~/work/infinigen-ref/.venv/bin/python"))
    src = os.environ.get("IG_REF_SRC", os.path.expanduser("~/work/infinigen-ref/src"))
    here = os.path.dirname(os.path.abspath(__file__))
    colls = {}
    for kind, variant in sorted({(p["kind"], int(p["variant"])) for p in props}):
        path = os.path.join(cache, f"{kind}_{variant}.blend")
        if not os.path.exists(path):
            r = subprocess.run([py42, os.path.join(here, "tree_cache.py"), kind, str(variant), path], cwd=src, capture_output=True, text=True)
            print("TREECACHE build", kind, variant, r.returncode, [l for l in r.stdout.splitlines() if l.startswith("TREECACHE")])
            if r.returncode != 0 or not os.path.exists(path): print("TREECACHE FAIL", r.stderr[-800:]); continue
        name = f"outdoor_{kind}_{variant}"
        with bpy.data.libraries.load(path, link=False) as (src_d, dst_d):
            dst_d.collections = [c for c in src_d.collections if c == name]
        if dst_d.collections: colls[(kind, variant)] = dst_d.collections[0]
    n = 0
    for i, p in enumerate(props):
        c = colls.get((p["kind"], int(p["variant"])))
        if c is None: continue
        e = bpy.data.objects.new(f"outdoor_{p['kind']}_inst{i}", None)
        e.instance_type = "COLLECTION"; e.instance_collection = c
        e.location = p["loc"]; e.rotation_euler = (0, 0, p["rot"]); e.scale = (p["scale"],) * 3
        sc.collection.objects.link(e); n += 1
    print("OUTDOOR", n, "instances of", len(colls), "variants")
    add_grass(side)

def tint_lawn(o):
    # the ground under the grass turns lawn-green (strands alone covered ~5 % of it: from the windows it read as bare earth): each
    # material of the terrain mixes toward a mottled green by the "grass" vertex-group weight
    for slot in o.material_slots:
        gm = slot.material
        if gm is None or not gm.use_nodes or gm.name.startswith("outdoor_grass") or gm.get("lawn"): continue
        gm = gm.copy(); gm["lawn"] = 1; slot.material = gm
        g = gm.node_tree; out = next((n for n in g.nodes if n.type == "OUTPUT_MATERIAL" and n.is_active_output), None)
        if out is None or not out.inputs["Surface"].is_linked: continue
        orig = out.inputs["Surface"].links[0].from_socket
        attr = g.nodes.new("ShaderNodeAttribute"); attr.attribute_name = "grass"
        noise = g.nodes.new("ShaderNodeTexNoise"); noise.inputs["Scale"].default_value = 0.6; noise.inputs["Detail"].default_value = 6.0
        col = g.nodes.new("ShaderNodeValToRGB")
        col.color_ramp.elements[0].color = (0.035, 0.07, 0.015, 1); col.color_ramp.elements[1].color = (0.14, 0.2, 0.05, 1)
        g.links.new(noise.outputs["Fac"], col.inputs["Fac"])
        lawn = g.nodes.new("ShaderNodeBsdfPrincipled"); lawn.inputs["Roughness"].default_value = 0.9
        for _k in ("Specular IOR Level", "Specular"):
            if _k in lawn.inputs: lawn.inputs[_k].default_value = 0.25
        g.links.new(col.outputs["Color"], lawn.inputs["Base Color"])
        # patchy edge: the weight through a noise threshold (some dry ground shows)
        patch = g.nodes.new("ShaderNodeTexNoise"); patch.inputs["Scale"].default_value = 0.15
        mulm = g.nodes.new("ShaderNodeMath"); mulm.operation = "MULTIPLY"
        g.links.new(attr.outputs["Fac"], mulm.inputs[0])
        rmp = g.nodes.new("ShaderNodeMapRange"); rmp.inputs["From Min"].default_value = 0.22; rmp.inputs["From Max"].default_value = 0.34
        g.links.new(patch.outputs["Fac"], rmp.inputs["Value"]); g.links.new(rmp.outputs["Result"], mulm.inputs[1])
        mix = g.nodes.new("ShaderNodeMixShader")
        g.links.new(mulm.outputs[0], mix.inputs["Fac"]); g.links.new(orig, mix.inputs[1]); g.links.new(lawn.outputs["BSDF"], mix.inputs[2])
        g.links.new(mix.outputs["Shader"], out.inputs["Surface"])

def add_haze():
    """aerial perspective as Infinigen does it (terrain/elements/atmosphere.py): a CLOSED volume of air over the terrain -- a cylinder
    2.4 km in radius and ~450 m tall -- with a Principled Volume (near-white albedo, anisotropy 0.5); the World keeps no volume (an
    unbounded world volume blacked out the sky and the sun). HAZE = density per m (default 0.0004: ~60 % transmittance at 1.3 km)"""
    import os
    d = float(os.environ.get("HAZE", "0.00016"))
    if d <= 0: return
    flo = [o for o in bpy.data.objects if o.type == "MESH" and ".floor" in o.name]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in flo for c in o.bound_box] or [mathutils.Vector((0, 0, 0))]
    cx = (min(p.x for p in pts) + max(p.x for p in pts)) / 2; cy = (min(p.y for p in pts) + max(p.y for p in pts)) / 2
    me = bpy.data.meshes.new("outdoor_haze"); import bmesh; bm = bmesh.new()
    # a flattened dome (ellipsoid 2.4 km x 450 m, centred on the ground), not a cylinder: rays leaving a flat top vs the side
    # switched path length at the rim and drew a hard grey band across the sky
    bmesh.ops.create_uvsphere(bm, u_segments=96, v_segments=48, radius=1.0)
    bmesh.ops.scale(bm, vec=(2400.0, 2400.0, 450.0), verts=bm.verts)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me); bm.free()
    ob = bpy.data.objects.new("outdoor_haze", me); ob.location = (cx, cy, 0.0); bpy.context.scene.collection.objects.link(ob)
    m = bpy.data.materials.new("outdoor_haze"); m.use_nodes = True; nt = m.node_tree
    # constant density: integrated exactly (ray marching stopped at its step limit a few km out and drew an edge across the sky)
    try: m.cycles.homogeneous_volume = True
    except Exception: pass
    for n in list(nt.nodes):
        if n.type != "OUTPUT_MATERIAL": nt.nodes.remove(n)
    pv = nt.nodes.new("ShaderNodeVolumePrincipled")
    pv.inputs["Density"].default_value = d; pv.inputs["Color"].default_value = (0.92, 0.95, 1.0, 1); pv.inputs["Anisotropy"].default_value = 0.5
    nt.links.new(pv.outputs[0], nt.nodes["Material Output"].inputs["Volume"])
    me.materials.append(m)
    ob.visible_shadow = False  # (the sun keeps lighting the ground at full strength)
    print("HAZE", d)

def add_grass(side):
    """grass on the near terrain (Blender hair particles: Infinigen's grass scatter builds its node groups in python that predates
    Blender 5): dense short blades from 1.2 m off the walls out to ~45 m, none under the house; GRASS_DENSITY parents per m2"""
    import os, math
    if os.environ.get("NO_GRASS"): return
    terr = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("outdoor_terrain_near")]
    # the far ground: a full-weight "grass" attribute (lawn tint, no strands) so the green does not stop at the fine grid's edge;
    # the ridge material (outdoor_terrain_far#1) keeps its rock
    for o in bpy.data.objects:
        if o.type == "MESH" and o.name.startswith("outdoor_terrain_far"):
            a_ = o.data.attributes.new("grass", "FLOAT", "POINT")
            if o.name.endswith("#0"):
                a_.data.foreach_set("value", [1.0] * len(o.data.vertices))
            else:
                # ridges: green where the slope is under ~25 deg, bare rock above ~40 deg (the striped sandstone read as painted)
                a_.data.foreach_set("value", [max(0.0, min(1.0, (v.normal.z - 0.77) / 0.14)) for v in o.data.vertices])
            tint_lawn(o)
    add_haze()
    floors = [o for o in bpy.data.objects if o.type == "MESH" and ".floor" in o.name and not o.name.startswith("outdoor")]
    if not terr or not floors: return
    boxes = []
    for f in floors:
        bb = [f.matrix_world @ mathutils.Vector(c) for c in f.bound_box]
        boxes.append((min(b.x for b in bb), max(b.x for b in bb), min(b.y for b in bb), max(b.y for b in bb)))
    def dist(x, y):
        return min(math.hypot(max(x0 - x, 0, x - x1), max(y0 - y, 0, y - y1)) for x0, x1, y0, y1 in boxes)
    mat = bpy.data.materials.new("outdoor_grass"); mat.use_nodes = True
    nt = mat.node_tree; bs = nt.nodes["Principled BSDF"]
    hi = nt.nodes.new("ShaderNodeHairInfo")
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].color = (0.018, 0.04, 0.008, 1); ramp.color_ramp.elements[1].color = (0.09, 0.17, 0.03, 1)
    nt.links.new(hi.outputs["Intercept"], ramp.inputs["Fac"])
    dry = nt.nodes.new("ShaderNodeMix"); dry.data_type = "RGBA"
    dry.inputs[7].default_value = (0.3, 0.27, 0.1, 1)
    gate = nt.nodes.new("ShaderNodeMath"); gate.operation = "GREATER_THAN"; gate.inputs[1].default_value = 0.82
    nt.links.new(hi.outputs["Random"], gate.inputs[0]); nt.links.new(gate.outputs[0], dry.inputs["Factor"])
    nt.links.new(ramp.outputs["Color"], dry.inputs[6]); nt.links.new(dry.outputs[2], bs.inputs["Base Color"])
    bs.inputs["Roughness"].default_value = 0.45
    # 70 % translucent (grass_blade.py): blades glow when backlit and stop reading as a glossy sheet
    tr = nt.nodes.new("ShaderNodeBsdfTranslucent"); nt.links.new(dry.outputs[2], tr.inputs["Color"])
    mx = nt.nodes.new("ShaderNodeMixShader"); mx.inputs["Fac"].default_value = 0.7
    out = nt.nodes["Material Output"]
    nt.links.new(bs.outputs["BSDF"], mx.inputs[1]); nt.links.new(tr.outputs["BSDF"], mx.inputs[2]); nt.links.new(mx.outputs["Shader"], out.inputs["Surface"])
    # per-strand tint: hair Random shifts the green toward yellow-olive or deeper green
    hue = nt.nodes.new("ShaderNodeHueSaturation"); hue.inputs["Saturation"].default_value = 1.0
    rmp2 = nt.nodes.new("ShaderNodeMapRange"); rmp2.inputs["To Min"].default_value = 0.46; rmp2.inputs["To Max"].default_value = 0.56
    nt.links.new(hi.outputs["Random"], rmp2.inputs["Value"]); nt.links.new(rmp2.outputs["Result"], hue.inputs["Hue"])
    nt.links.new(ramp.outputs["Color"], hue.inputs["Color"]); nt.links.new(hue.outputs["Color"], dry.inputs[6])
    for _k in ("Specular IOR Level", "Specular"):
        if _k in bs.inputs: bs.inputs[_k].default_value = 0.12  # (glossy blades read as frost at grazing angles)
    dens = float(os.environ.get("GRASS_DENSITY", "12"))
    for o in terr:
        me = o.data
        vg = o.vertex_groups.new(name="grass")
        w_area = 0.0
        for v in me.vertices:
            wv = o.matrix_world @ v.co
            d = dist(wv.x, wv.y)
            w = 0.0 if d < 1.2 else max(0.0, min(1.0, (d - 1.2) / 1.5)) * max(0.0, min(1.0, (68.0 - d) / 10.0))
            if w > 0: vg.add([v.index], w, "REPLACE")
        for poly in me.polygons:
            w_area += poly.area * sum(1.0 for i in poly.vertices if o.vertex_groups["grass"].index in [g.group for g in me.vertices[i].groups]) / len(poly.vertices)
        tint_lawn(o)
        o.data.materials.append(mat)
        psys = o.modifiers.new("grass", "PARTICLE_SYSTEM").particle_system
        psys.vertex_group_density = "grass"; psys.vertex_group_length = "grass"
        ps = psys.settings
        # (set one by one: Blender versions renamed or dropped some hair settings)
        for k, v in [("type", "HAIR"), ("use_advanced_hair", True), ("count", int(min(400000, w_area * dens))), ("hair_length", 0.13),
                     ("material_slot", mat.name), ("child_type", "INTERPOLATED"), ("rendered_child_count", 12), ("child_percent", 1),
                     ("child_radius", 0.25), ("child_length", 1.0), ("child_length_threshold", 0.45), ("roughness_1", 0.02),
                     ("roughness_endpoint", 0.06), ("roughness_2", 0.03), ("root_radius", 0.6), ("tip_radius", 0.0), ("radius_scale", 0.008),
                     ("render_step", 3), ("display_step", 2)]:
            try: setattr(ps, k, v)
            except Exception as e: print("GRASS setting", k, repr(e))
        print("GRASS", o.name, ps.count, "parents over", round(w_area), "m2")

def auto_exposure(sc):
    """one tiny probe render: nudge the film exposure toward a mid-grey frame (rooms beside bright windows washed out, dark ones went
    murky once the terrain stopped lighting them from below); NO_AUTOEXP=1 keeps --exposure"""
    import os, tempfile
    if os.environ.get("NO_AUTOEXP") or args.top or args.showcase: return
    keep = (sc.render.resolution_percentage, sc.cycles.samples, sc.render.filepath, sc.cycles.use_denoising)
    sc.render.use_persistent_data = True
    # (16 samples, denoised: at 4 noisy samples many pixels came back near black, the mean read low and white kitchens were pushed to
    # x20 exposure, blown out)
    sc.render.resolution_percentage = max(5, keep[0] // 8); sc.cycles.samples = 16; sc.cycles.use_denoising = True
    tmp = os.path.join(tempfile.gettempdir(), "autoexp_probe.png"); sc.render.filepath = tmp
    import math
    # up to 3 probes: the image is tone mapped (AgX compresses), so one step undershoots
    for it in range(3):
        bpy.ops.render.render(write_still=True)
        img = bpy.data.images.load(tmp); px = list(img.pixels); bpy.data.images.remove(img)
        lum = [0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2] for i in range(0, len(px), 4)]
        lum.sort(); core = lum[len(lum) // 20: len(lum) * 19 // 20] or lum
        avg = sum(core) / len(core)
        # highlight guard: the share of near-clipped pixels (white kitchens: AgX saturates the cabinets while dark fronts keep the
        # mean low, and the mean-only rule pushed the exposure to x20 -- blown out)
        clip = sum(1 for v in lum if v > 0.95) / len(lum)
        if 0.36 < avg < 0.5 and clip < 0.18: break
        step = max(-2.5, min(1.5, math.log2(0.43 / max(avg, 1e-3)) * 2.0))
        if clip > 0.10: step = min(step, 0.0)
        if clip > 0.18: step = min(step, -0.6)
        if step == 0.0: break
        sc.cycles.film_exposure *= 2 ** step
        print("AUTOEXP probe %d mean %.3f clip %.2f -> x%.2f (exposure %.3f)" % (it, avg, clip, 2 ** step, sc.cycles.film_exposure))
    sc.render.resolution_percentage, sc.cycles.samples, sc.render.filepath, sc.cycles.use_denoising = keep

# point lights (PointLampFactory)
_lamp = dscap.lamp_state(DS.get("variation"), sidecar) if DS else (lambda L: (True, 1.0))
for L in sidecar.get("lights", []) + ([dict(L, _off=True) for L in sidecar.get("lights_off", [])] if DS else []):
    _on, _ls = _lamp(L)
    if L.get("_off"): _on = False
    if not _on and not DS: continue
    if not _on: _ls = 1.0  # dataset capture: switched-off lamps exist hidden (dscap switches them on when a view is too dark)
    ld = bpy.data.lights.new(L["name"], "POINT"); ld.energy = L["watt"] * args.light_scale * _ls; ld.shadow_soft_size = L["radius"]
    ld.use_nodes = True
    nt = ld.node_tree; nt.nodes.clear()
    bb = nt.nodes.new("ShaderNodeBlackbody"); bb.inputs["Temperature"].default_value = L["temperature"]
    em = nt.nodes.new("ShaderNodeEmission"); out = nt.nodes.new("ShaderNodeOutputLight")
    nt.links.new(bb.outputs[0], em.inputs["Color"]); nt.links.new(em.outputs[0], out.inputs[0])
    lo = bpy.data.objects.new(L["name"], ld); sc.collection.objects.link(lo)
    if not _on: lo.hide_render = True; lo["ig_off"] = 1
    lo.location = mathutils.Vector(L["loc"])  # z-up coords match after glTF import (importer converts the glb; lights are added in blender space)

def view_camera(sc, room, view):
    """cornerA / cornerB views inside `room`: farthest floor corners from the centroid; skips corners whose camera spot is blocked"""
    import bpy, mathutils, math
    fl = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith(room) and ".floor" in o.name]
    # layout-only inputs (independent of how the floor is triangulated/subdivided): area-weighted centroid, and the
    # outline corners (boundary vertices where the outline turns)
    import bmesh as _bm
    pts = []; area = 0.0; acc = mathutils.Vector((0, 0, 0))
    for o in fl:
        b = _bm.new(); b.from_mesh(o.data); b.transform(o.matrix_world)
        for f in b.faces:
            a = f.calc_area(); area += a; acc += f.calc_center_median() * a
        for v in b.verts:
            be = [e for e in v.link_edges if e.is_boundary]
            if len(be) != 2: continue
            p0 = be[0].other_vert(v).co - v.co; p1 = be[1].other_vert(v).co - v.co
            if p0.length < 1e-6 or p1.length < 1e-6: continue
            if abs(p0.x * p1.y - p0.y * p1.x) / (p0.length * p1.length) > 0.05:
                pts.append(mathutils.Vector((v.co.x, v.co.y, 0)))
        b.free()
    cen = mathutils.Vector((acc.x / area, acc.y / area, 0))
    # blockers: every visible object except the room shell / window frames+glass (curtains and doors DO block the camera)
    skip = (".floor", ".wall", ".ceiling", "skirting", "exterior", "entrance", "placeholder", "vc", "portal")
    def is_frame(n): return "window" in n or "door" in n  # doors: python door objects carry a large local AABB, Rust doors are split into parts; python joins frame+shutter+curtain into one window object; treat Rust parts the same
    boxes = []
    for o in bpy.data.objects:
        if o.type != "MESH" or o.hide_render or any(k in o.name.lower() for k in skip) or is_frame(o.name.lower()): continue
        if any(c.name.startswith("placeholders") for c in o.users_collection) or o.name.endswith(".meshed"): continue
        bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
        boxes.append((min(b.x for b in bb), max(b.x for b in bb), min(b.y for b in bb), max(b.y for b in bb), min(b.z for b in bb), max(b.z for b in bb)))
    def blocked(loc):
        for x0, x1, y0, y1, z0, z1 in boxes:
            if x0 - 0.8 < loc.x < x1 + 0.8 and y0 - 0.8 < loc.y < y1 + 0.8 and z0 < 1.9 and z1 > 0.3: return True
        return False
    uniq = []
    for p in sorted(pts, key=lambda p: -(p - cen).length):
        if all((p - q).length > 0.8 for q in uniq): uniq.append(p)
    cands = uniq[:6]
    def clearance(loc):
        best = 9.0
        for x0, x1, y0, y1, z0, z1 in boxes:
            if z0 >= 1.9 or z1 <= 0.3: continue
            dx = max(x0 - loc.x, 0, loc.x - x1); dy = max(y0 - loc.y, 0, loc.y - y1)
            best = min(best, math.hypot(dx, dy))
        return best
    far = None
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    def openness(loc):
        # rays across the horizontal field of view toward the room centre: how many travel > 1.2 m before hitting anything
        o3 = mathutils.Vector((loc.x, loc.y, 1.5)); t3 = mathutils.Vector((cen.x, cen.y, 0.9))
        fwd = (t3 - o3).normalized(); n = 0
        for k in range(-4, 5):
            rot = mathutils.Matrix.Rotation(math.radians(10 * k), 3, "Z")
            hit, *_ = sc.ray_cast(dg, o3, (rot @ fwd).normalized(), distance=2.0)
            n += 0 if hit else 1
        return n
    scored = []
    for p in cands:
        d = (cen - p); d.normalize(); q = p + d * 0.45
        scored.append((not blocked(q), openness(q), clearance(q), p))
    print("VIEWCAND", [(a, b, round(c, 2), round(p.x, 2), round(p.y, 2)) for a, b, c, p in scored]); print("VIEWBOX", sorted((round(b[0],1),round(b[1],1),round(b[2],1),round(b[3],1)) for b in boxes))
    # rank the corner spots: unblocked and open first, then openness, clearance; cornerA = best, cornerB = second best
    order = sorted(range(len(scored)), key=lambda i: (-(scored[i][0] and scored[i][1] >= 8), -(scored[i][1] >= 7), -min(scored[i][2], 0.6), -scored[i][1], i))
    far = scored[order[1 if view == "cornerB" and len(order) > 1 else 0]][3]
    d = (cen - far); d.normalize()
    loc = far + d * 0.45; loc.z = 1.5
    tgt = mathutils.Vector((cen.x, cen.y, 0.9))
    cam = bpy.data.cameras.new("vc"); cam.lens = 18; cam.sensor_width = 36
    co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co)
    co.location = loc; print("VIEWCAM", view, tuple(round(x, 2) for x in loc))
    co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
    sc.camera = co
# light portals at windows (the reference adds one is_portal area light per window; they steer sky light through windows)
import re as _re
for _o in list(bpy.data.objects):
    if _o.type == "MESH" and _re.fullmatch(r"window\d+", _o.name):
        _bb = [_o.matrix_world @ mathutils.Vector(c) for c in _o.bound_box]
        _lo = mathutils.Vector((min(b.x for b in _bb), min(b.y for b in _bb), min(b.z for b in _bb)))
        _hi = mathutils.Vector((max(b.x for b in _bb), max(b.y for b in _bb), max(b.z for b in _bb)))
        _d = _hi - _lo; _c = (_lo + _hi) / 2
        if _d.x < _d.y: _n, _u, _w = mathutils.Vector((1, 0, 0)), mathutils.Vector((0, 1, 0)), _d.y
        else: _n, _u, _w = mathutils.Vector((0, 1, 0)), mathutils.Vector((-1, 0, 0)), _d.x
        _v = mathutils.Vector((0, 0, 1))
        _m = mathutils.Matrix(((_u.x, _v.x, _n.x), (_u.y, _v.y, _n.y), (_u.z, _v.z, _n.z))).to_4x4(); _m.translation = _c
        _ld = bpy.data.lights.new("portal", "AREA"); _ld.size = 1; _ld.size_y = 1; _ld.cycles.is_portal = True
        _po = bpy.data.objects.new("portal", _ld); sc.collection.objects.link(_po)
        _po.matrix_world = _m; _po.scale = (_w, _d.z, 1)
# ==== VIEW ==== (blender_server.py saves the scene set up above and re-runs only the code below for further views of the same scene)
# camera
if DS:
    sc.cycles.film_exposure = args.exposure
    sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = _DEVICE; sc.cycles.use_denoising = True; sc.cycles.use_adaptive_sampling = True
    _r = dscap.run(sc, sidecar, DS); print("DSRESULT", json.dumps(_r)); sys.exit(0)
def _rust_corner(glb, room, view):
    """cornerA / cornerB from the Rust picker (tools/bevy_preview/src/viewpick.rs, the one the fast loop tunes: tools/views16.sh):
    the previewer dumps the camera as json; None when it is not built (then viewcam.py picks). PY_VIEWCAM=1 forces viewcam.py."""
    import os, subprocess, tempfile, glob as _glob
    bp = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "bevy_preview", "target", "release", "bevy_preview")
    if os.environ.get("PY_VIEWCAM") or not os.path.exists(bp): return None
    d = tempfile.mkdtemp(prefix="rustcam")
    r = subprocess.run([bp, glb, "--corner", "%s:%s" % (room, view[-1]), "--dump-cams", d, "--res", "64x36", "--ss", "1", "-o", os.path.join(d, "x.png")], capture_output=True, text=True)
    js = sorted(_glob.glob(os.path.join(d, "*.json")))
    print("RUSTCAM", [l for l in r.stderr.splitlines() if l.startswith("VIEWCAM")], js[:1])
    return json.load(open(js[0])) if js else None


if args.view in ("cornerA", "cornerB"):
    import os, sys
    _rc = _rust_corner(args.glb, args.room, args.view)
    if _rc is not None:
        cam = bpy.data.cameras.new("vc"); co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co); sc.camera = co
        co.matrix_world = mathutils.Matrix(_rc["matrix"]); cam.lens = _rc.get("lens", 18); cam.sensor_width = _rc.get("sensor_width", 36)
    else:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import viewcam; viewcam.view_camera(sc, args.room, args.view); co = sc.camera; cam = co.data
elif args.view and args.view.startswith("cam"):
  _cj = {c["name"]: c for c in sidecar.get("cameras", [])}
  assert args.view in _cj, "camera %s not in sidecar (have %s)" % (args.view, list(_cj))
  c = _cj[args.view]
  cam = bpy.data.cameras.new("cam");   co = bpy.data.objects.new("cam", cam); sc.collection.objects.link(co); sc.camera = co
  co.matrix_world = mathutils.Matrix(c["matrix"]); cam.lens = c.get("lens", 24); cam.sensor_width = c.get("sensor_width", 36); print("VIEWCAM", args.view, c.get("subject"))
else:
  cam = bpy.data.cameras.new("cam");   co = bpy.data.objects.new("cam", cam); sc.collection.objects.link(co); sc.camera = co
  if args.cam:
    c = json.load(open(args.cam))
    co.matrix_world = mathutils.Matrix(c["matrix"]); cam.lens = c.get("lens", 15); cam.sensor_width = c.get("sensor_width", 36)
# (far clip: the default 1 km cut camera rays still inside the 450 m haze below ~25 deg up -- a hard grey band across the sky; the
# terrain reaches 2.5 km and the cloud layer 9 km)
if sc.camera is not None: sc.camera.data.clip_end = 30000.0
sc.cycles.film_exposure = args.exposure
sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = _DEVICE; sc.cycles.use_denoising = True
try:  # keep fine texture through the denoiser (default settings smeared walls / fabric into a waxy look)
    sc.cycles.denoiser = "OPENIMAGEDENOISE"; sc.cycles.denoising_input_passes = "RGB_ALBEDO_NORMAL"; sc.cycles.denoising_prefilter = "ACCURATE"
    sc.cycles.denoising_quality = "HIGH"
except Exception as _e: print("DENOISE settings", repr(_e))
sc.render.resolution_x, sc.render.resolution_y = 960, 540
import os as _os
sc.render.resolution_percentage = int(_os.environ.get("RES_PCT", "100"))
sc.cycles.use_adaptive_sampling = True
sc.render.filepath = args.out
# (after the camera, which only needs the room)
add_outdoor(sc, sidecar)


def leaf_look():
    """Rust tree leaf cards (rtree_leaf* materials from the glb): a third of the light passes through the leaf (Translucent BSDF of
    the same colour), as real foliage does -- dense crowns and shrubs rendered as black blobs with opaque cards; the alpha cutout
    is kept by a Transparent mix on the texture's alpha"""
    n_done = 0
    for m in bpy.data.materials:
        if not m.name.startswith("rtree_leaf") or not m.use_nodes or m.get("leaf_look"): continue
        nt = m.node_tree
        p = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
        out = next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL"), None)
        if p is None or out is None or not p.inputs["Base Color"].is_linked: continue
        col = p.inputs["Base Color"].links[0].from_socket
        alpha = p.inputs["Alpha"].links[0].from_socket if p.inputs["Alpha"].is_linked else None
        tr = nt.nodes.new("ShaderNodeBsdfTranslucent"); nt.links.new(col, tr.inputs["Color"])
        mx = nt.nodes.new("ShaderNodeMixShader"); mx.inputs["Fac"].default_value = 0.35
        nt.links.new(p.outputs[0], mx.inputs[1]); nt.links.new(tr.outputs[0], mx.inputs[2])
        final = mx.outputs[0]
        if alpha is not None:
            for l in list(p.inputs["Alpha"].links): nt.links.remove(l)
            p.inputs["Alpha"].default_value = 1.0
            tp = nt.nodes.new("ShaderNodeBsdfTransparent")
            mx2 = nt.nodes.new("ShaderNodeMixShader"); nt.links.new(alpha, mx2.inputs["Fac"])
            nt.links.new(tp.outputs[0], mx2.inputs[1]); nt.links.new(final, mx2.inputs[2]); final = mx2.outputs[0]
        nt.links.new(final, out.inputs["Surface"])
        p.inputs["Roughness"].default_value = max(p.inputs["Roughness"].default_value, 0.6)
        m["leaf_look"] = 1
        n_done += 1
    print("LEAFLOOK", n_done, "leaf materials")


def apply_weather(sc, side):
    """the sidecar's outdoor.weather (crates/ig_outdoor/src/weather.rs): sun (lights and the sky texture's sun disc) by sun_strength,
    the world by sky_ambient, the haze volume by haze; the cloud layer and rain streaks cast no shadows; wet surfaces (terrain,
    road, house exteriors) get a clear coat by wetness. NO_WEATHER=1 skips it."""
    import os
    wx = (side.get("outdoor") or {}).get("weather")
    if not wx or os.environ.get("NO_WEATHER"): return
    s_sun, s_amb, s_haze, wet = wx["sun_strength"], wx["sky_ambient"], wx["haze"], wx["wetness"]
    for o in bpy.data.objects:
        if o.type == "LIGHT" and o.data.type == "SUN": o.data.energy *= s_sun
    wo = sc.world
    if wo and wo.use_nodes:
        for n in wo.node_tree.nodes:
            if n.type == "TEX_SKY":
                if hasattr(n, "sun_intensity"): n.sun_intensity *= s_sun
                # (overcast / rain: no sun disc at all -- scaling its intensity did not take in Blender 5's sky model, hard shadows stayed)
                if s_sun < 0.2 and hasattr(n, "sun_disc"): n.sun_disc = False
            elif n.type == "BACKGROUND":
                n.inputs["Strength"].default_value *= s_amb
    hz = bpy.data.materials.get("outdoor_haze")
    if hz and hz.use_nodes:
        for n in hz.node_tree.nodes:
            if n.type == "PRINCIPLED_VOLUME": n.inputs["Density"].default_value *= s_haze
    for o in bpy.data.objects:
        if o.type == "MESH" and o.name.startswith(("outdoor_sky", "outdoor_rain")):
            o.visible_shadow = False
            if os.environ.get("NO_CLOUDS") and o.name.startswith("outdoor_sky"): o.hide_render = True
    if wet > 0:
        done = set()
        for o in bpy.data.objects:
            # (roads, paving and house exteriors; not the terrain: a coat on the lawn read as patches of snow)
            if o.type != "MESH" or not o.name.startswith(("outdoor_road", "exterior_")): continue
            for sl in o.material_slots:
                m = sl.material
                if m is None or not m.use_nodes or m.name in done: continue
                done.add(m.name)
                for n in m.node_tree.nodes:
                    if n.type == "BSDF_PRINCIPLED" and "Coat Weight" in n.inputs and not n.inputs["Coat Weight"].is_linked:
                        n.inputs["Coat Weight"].default_value = 0.8 * wet
                        n.inputs["Coat Roughness"].default_value = 0.05
    if os.environ.get("VOLDBG"):
        for o in bpy.data.objects:
            ms = [sl.material for sl in getattr(o, "material_slots", []) if sl.material and sl.material.use_nodes]
            if any(n.type in ("PRINCIPLED_VOLUME", "VOLUME_SCATTER", "VOLUME_ABSORPTION") for m in ms for n in m.node_tree.nodes) or "haze" in o.name or "sky" in o.name.lower():
                print("VOLDBG", o.name, o.type, tuple(round(v, 1) for v in o.location), tuple(round(v, 1) for v in o.dimensions), o.hide_render)
        wv = sc.world.node_tree.nodes if sc.world and sc.world.use_nodes else []
        print("VOLDBG world", [n.type for n in wv], "volume linked", any(n.type == "OUTPUT_WORLD" and n.inputs["Volume"].is_linked for n in wv))
    print("WEATHER", wx["state"], "sun", s_sun, "ambient", s_amb, "haze", s_haze, "wet", wet)


leaf_look()
apply_weather(sc, sidecar)
# --view outdoor[:AZ[:DIST]]: the outdoor scene alone (the house hidden), a camera on the terrain at eye height DIST m (default 25)
# from the house centre toward compass azimuth AZ (degrees, default 0 = +y), looking outward 4 deg down
if args.view and (args.view.startswith("outdoor") or args.view in ("street", "neighbour")):
    import math
    from mathutils.bvhtree import BVHTree
    parts = (args.view.split(":") + ["0", "25"])[1:3]
    az, dist = math.radians(float(parts[0] or 0)), float(parts[1] or 25)
    for o in bpy.data.objects:
        # (only the house's own objects: the instanced tree collections are not linked into the scene)
        # (the house exteriors -- this house's and the neighbours', exterior_* -- stay: the street around it; the street view
        # keeps the whole house: its windows show the rooms, not the sky through an empty shell)
        if args.view in ("street", "neighbour"): break
        if o.type in ("MESH", "CURVES") and o.users_scene and not o.name.startswith(("outdoor", "exterior_")): o.hide_render = True
    for o in bpy.data.objects:
        if o.type == "LIGHT" and not o.data.type == "SUN": o.hide_render = True
    flo = [o for o in bpy.data.objects if o.type == "MESH" and ".floor" in o.name]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in flo for c in o.bound_box]
    cx = (min(p.x for p in pts) + max(p.x for p in pts)) / 2; cy = (min(p.y for p in pts) + max(p.y for p in pts)) / 2
    x, y = cx + dist * math.sin(az), cy + dist * math.cos(az)
    dg = bpy.context.evaluated_depsgraph_get(); z = 0.0
    for o in bpy.data.objects:
        if o.type == "MESH" and o.name.startswith("outdoor_terrain_near"):
            me = o.evaluated_get(dg).to_mesh()
            bvh = BVHTree.FromPolygons([o.matrix_world @ v.co for v in me.vertices], [list(f.vertices) for f in me.polygons])
            hit = bvh.ray_cast(mathutils.Vector((x, y, 500)), mathutils.Vector((0, 0, -1)))
            if hit[0] is not None: z = hit[0].z
    cam = bpy.data.cameras.new("ocam"); cam.lens = float(os.environ.get("OUT_LENS", 24)); co = bpy.data.objects.new("ocam", cam)
    cam.clip_end = 30000.0
    sc.collection.objects.link(co); sc.camera = co
    co.location = (x, y, z + 1.7)
    co.rotation_euler = (math.radians(86), 0, -az)
    print("OUTCAM", round(x, 1), round(y, 1), round(z + 1.7, 2), "az", parts[0])
    if args.view in ("street", "neighbour"):
        # the generator's site cameras (crates/ig_outdoor/src/site.rs Site::record): street = from the road side of the front
        # lawn looking at the house, neighbour = from the road at the nearest neighbour house
        _st = ((sidecar.get("outdoor") or {}).get("site") or {}).get(args.view + "_cam")
        assert _st, "no outdoor.site.%s_cam in the sidecar (site off?)" % args.view
        _eye, _tgt = mathutils.Vector(_st["eye"]), mathutils.Vector(_st["target"])
        co.location = _eye; co.rotation_euler = (_tgt - _eye).to_track_quat("-Z", "Y").to_euler()
        print("STREETCAM", [round(v, 1) for v in _eye], "->", [round(v, 1) for v in _tgt])
    # photographic tone: AgX with more contrast (the default look left outdoor views flat and milky)
    try:
        sc.view_settings.view_transform = "AgX"
        for _lk in ("AgX - Medium High Contrast", "Medium High Contrast"):
            try: sc.view_settings.look = _lk; break
            except Exception: pass
    except Exception as e: print("LOOK", repr(e))
if os.environ.get('DEBUGL'):
    from collections import Counter
    print('LIGHTS', Counter((o.data.type, round(o.data.energy,1), getattr(o.data.cycles,'is_portal',False)) for o in bpy.data.objects if o.type=='LIGHT'))
    for n in sc.world.node_tree.nodes:
        if n.type=='BACKGROUND': print('BG', n.inputs[1].default_value)
        if n.type=='TEX_SKY': print('SKY', n.sun_elevation, n.sun_rotation, n.air_density, n.dust_density, getattr(n,'sun_intensity',None))
    print('VIEW', sc.view_settings.view_transform, sc.view_settings.exposure, sc.cycles.film_exposure)
phase("sky lights cam")
if os.environ.get("DUMPSIG"):
    _dumpsig(os.environ["DUMPSIG"])
if os.environ.get("DUMPOBJ"):
    _o = bpy.data.objects[os.environ["DUMPOBJ"]]
    for _m in _o.data.materials:
        print("OBJMAT", _m.name, "nodes", len(_m.node_tree.nodes), sorted((n.type, n.label) for n in _m.node_tree.nodes)[:12])
        for n in _m.node_tree.nodes:
            if n.type in ("TEX_COORD", "ATTRIBUTE", "UVMAP", "TEX_IMAGE", "VERTEX_COLOR"):
                print("  ", n.type, getattr(n, "attribute_name", ""), getattr(n, "uv_map", ""), getattr(n, "object", None), [o.name for o in n.outputs if o.is_linked])
    print("OBJDATA attrs", sorted((a.name, a.domain, a.data_type) for a in _o.data.attributes), "uv", [(u.name, u.active_render) for u in _o.data.uv_layers], "modifiers", [m.type for m in _o.modifiers])
if os.environ.get("DUMPLIGHTS"):
    json.dump(sorted([[o.name, o.type, [round(x, 3) for x in o.matrix_world.translation], round(o.data.energy, 3) if o.type == 'LIGHT' else None,
                       o.data.type if o.type == 'LIGHT' else None, getattr(o.data.cycles, 'is_portal', None) if o.type == 'LIGHT' else None] for o in bpy.data.objects if o.type != 'MESH']),
              open(os.environ["DUMPLIGHTS"], "w"))
if args.norender: print('NORENDER done'); sys.exit(0)
auto_exposure(sc)
if os.environ.get("ENGINE") == "EEVEE":  # fast material-accurate preview (no path tracing)
    sc.render.engine = "BLENDER_EEVEE"
    try:
        sc.eevee.taa_render_samples = int(os.environ.get("EEVEE_SAMPLES", "32"))
        sc.eevee.use_shadows = True; sc.eevee.use_raytracing = True
    except Exception as e: print("EEVEE settings", repr(e))
t = time.time(); bpy.ops.render.render(write_still=True); print("render time", time.time() - t)
